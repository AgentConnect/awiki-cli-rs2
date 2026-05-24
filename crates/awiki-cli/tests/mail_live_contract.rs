use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[test]
fn mail_inbox_live_posts_mail_rpc_through_im_core() {
    let workspace = TempDir::new("mail-live-inbox").expect("workspace");
    register_ready_mail_identity(workspace.path(), "alice-mail", "alice", "jwt-mail");
    let server = TestServer::new(vec![TestResponse::ok(
        r#"{"jsonrpc":"2.0","result":{"total":2,"messages":[{"id":"m1","subject":"First"},{"id":"m2","subject":"Second"}]},"id":"req-1"}"#,
    )]);
    write_mail_config(workspace.path(), &server.base_url());

    let output = awiki_cmd(
        &[
            "--identity",
            "alice-mail",
            "mail",
            "inbox",
            "--folder",
            "inbox",
            "--limit",
            "2",
            "--offset",
            "3",
            "--unread",
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Loaded 2 messages");
    assert_eq!(envelope["data"]["messages"][0]["id"], "m1");
    assert_eq!(envelope["data"]["messages"][1]["subject"], "Second");

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("POST /mail/rpc HTTP/1.1"));
    assert_contains_text(&requests[0], "Authorization: Bearer jwt-mail\r\n");
    let body: Value = serde_json::from_str(request_body(&requests[0])).expect("request body");
    assert_eq!(body["jsonrpc"], "2.0");
    assert_eq!(body["method"], "mail.getInbox");
    assert_eq!(
        body["params"],
        json!({
            "folder": "inbox",
            "limit": 2,
            "offset": 3,
            "unread_only": true
        })
    );
}

#[test]
fn mail_attachment_live_downloads_via_im_core_and_cli_writes_decoded_bytes() {
    let workspace = TempDir::new("mail-live-attachment").expect("workspace");
    register_ready_mail_identity(workspace.path(), "alice-mail", "alice", "jwt-mail");
    let server = TestServer::new(vec![TestResponse::ok(
        r#"{"jsonrpc":"2.0","result":{"filename":"remote.txt","content_type":"text/plain","content_base64":"aGVsbG8="},"id":"req-1"}"#,
    )]);
    write_mail_config(workspace.path(), &server.base_url());
    let output_path = workspace.path().join("downloads").join("out.txt");

    let output = awiki_cmd_owned(
        &mail_attachment_download_args(&output_path),
        workspace.path(),
    );

    assert_success(&output);
    assert_eq!(std::fs::read(&output_path).unwrap(), b"hello");
    let envelope = success_json(&output);
    assert_eq!(
        envelope["summary"],
        format!("Attachment saved to {}", output_path.display())
    );
    assert_eq!(envelope["data"]["message_id"], "msg-1");
    assert_eq!(envelope["data"]["attachment_index"], 0);
    assert_eq!(envelope["data"]["filename"], "remote.txt");
    assert_eq!(envelope["data"]["content_type"], "text/plain");
    assert_eq!(envelope["data"]["path"], output_path.display().to_string());

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("POST /mail/rpc HTTP/1.1"));
    let body: Value = serde_json::from_str(request_body(&requests[0])).expect("request body");
    assert_eq!(body["method"], "mail.getAttachment");
    assert_eq!(
        body["params"],
        json!({
            "message_id": "msg-1",
            "attachment_index": 0
        })
    );
}

#[test]
fn mail_attachment_dry_run_does_not_hit_remote_or_write_output() {
    let workspace = TempDir::new("mail-live-attachment-dry-run").expect("workspace");
    register_ready_mail_identity(workspace.path(), "alice-mail", "alice", "jwt-mail");
    let server = TestServer::new(Vec::new());
    write_mail_config(workspace.path(), &server.base_url());
    let output_path = workspace.path().join("downloads").join("dry-run.txt");
    let mut args = vec!["--dry-run".to_string()];
    args.extend(mail_attachment_download_args(&output_path));

    let output = awiki_cmd_owned(&args, workspace.path());

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(
        envelope["summary"],
        "Dry run: mail attachment download planned"
    );
    assert_eq!(envelope["data"]["plan"]["action"], "mail.getAttachment");
    assert!(
        !output_path.exists(),
        "dry-run mail attachment command must not write output files"
    );
    assert!(
        server.requests().is_empty(),
        "dry-run mail attachment command must not call mail-service"
    );
}

fn register_ready_mail_identity(
    workspace: &Path,
    identity_name: &str,
    handle: &str,
    jwt_token: &str,
) {
    let create = awiki_cmd(
        &[
            "id",
            "create",
            "--name",
            "Alice Mail",
            "--identity",
            identity_name,
        ],
        workspace,
    );
    assert_success(&create);

    let index_path = workspace.join("identities").join("index.json");
    let mut index: Value = serde_json::from_slice(&std::fs::read(&index_path).unwrap()).unwrap();
    index["credentials"][identity_name]["handle"] = json!(handle);
    index["credentials"][identity_name]["full_handle"] = json!(format!("{handle}@awiki.ai"));
    index["credentials"][identity_name]["user_id"] = json!("user-alice");
    std::fs::write(&index_path, serde_json::to_vec_pretty(&index).unwrap()).unwrap();

    let dir_name = index["credentials"][identity_name]["dir_name"]
        .as_str()
        .unwrap();
    let identity_dir = workspace.join("identities").join(dir_name);
    let identity_path = identity_dir.join("identity.json");
    let mut identity: Value =
        serde_json::from_slice(&std::fs::read(&identity_path).unwrap()).unwrap();
    identity["handle"] = json!(handle);
    identity["full_handle"] = json!(format!("{handle}@awiki.ai"));
    identity["user_id"] = json!("user-alice");
    std::fs::write(
        &identity_path,
        serde_json::to_vec_pretty(&identity).unwrap(),
    )
    .unwrap();

    std::fs::write(
        identity_dir.join("auth.json"),
        serde_json::to_vec_pretty(&json!({ "jwt_token": jwt_token })).unwrap(),
    )
    .unwrap();
}

fn write_mail_config(workspace: &Path, base_url: &str) {
    std::fs::write(
        workspace.join("config.yaml"),
        format!(
            "services:\n  service_base_url: https://awiki.ai\n  did_domain: awiki.ai\n  mail_service_url: {base_url}\n"
        ),
    )
    .unwrap();
}

fn mail_attachment_download_args(output_path: &Path) -> Vec<String> {
    vec![
        "--identity".to_string(),
        "alice-mail".to_string(),
        "mail".to_string(),
        "attachment".to_string(),
        "download".to_string(),
        "--message-id".to_string(),
        "msg-1".to_string(),
        "--attachment-index".to_string(),
        "0".to_string(),
        "--output".to_string(),
        output_path.to_string_lossy().into_owned(),
    ]
}

fn awiki_cmd(args: &[&str], workspace: &Path) -> Output {
    let args = args
        .iter()
        .map(|arg| (*arg).to_string())
        .collect::<Vec<_>>();
    awiki_cmd_owned(&args, workspace)
}

fn awiki_cmd_owned(args: &[String], workspace: &Path) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_awiki-cli"));
    command
        .args(args)
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace)
        .env("AWIKI_CLI_UPDATE_CACHE_ONLY", "1")
        .env_remove("AWIKI_WORKSPACE")
        .env_remove("AWIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_HOME")
        .env_remove("AVIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_FORMAT")
        .env_remove("AVIKI_FORMAT")
        .env_remove("AWIKI_CLI_TRACE_TIMING");
    command.output().expect("run awiki-cli binary")
}

fn assert_success(output: &Output) {
    assert_eq!(
        output.status.code(),
        Some(0),
        "unexpected exit status; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "stderr should be empty: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn success_json(output: &Output) -> Value {
    let envelope: Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be a JSON success envelope");
    assert_eq!(envelope["ok"], true);
    envelope
}

#[derive(Clone)]
struct TestResponse {
    status: u16,
    body: String,
}

impl TestResponse {
    fn ok(body: &str) -> Self {
        Self {
            status: 200,
            body: body.to_string(),
        }
    }
}

struct TestServer {
    address: String,
    requests: Arc<Mutex<Vec<String>>>,
    join: Option<thread::JoinHandle<()>>,
}

impl TestServer {
    fn new(responses: Vec<TestResponse>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        listener
            .set_nonblocking(true)
            .expect("set test server nonblocking");
        let address = format!("http://{}", listener.local_addr().expect("local addr"));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let server_requests = Arc::clone(&requests);
        let join = thread::spawn(move || {
            for response in responses {
                let stream = accept_with_timeout(&listener);
                let Some(stream) = stream else {
                    break;
                };
                handle_connection(stream, &server_requests, response);
            }
        });
        Self {
            address,
            requests,
            join: Some(join),
        }
    }

    fn base_url(&self) -> String {
        self.address.clone()
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().expect("requests mutex").clone()
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn accept_with_timeout(listener: &TcpListener) -> Option<TcpStream> {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        match listener.accept() {
            Ok((stream, _)) => return Some(stream),
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                if std::time::Instant::now() >= deadline {
                    return None;
                }
                thread::sleep(Duration::from_millis(10));
            }
            Err(_) => return None,
        }
    }
}

fn handle_connection(
    mut stream: TcpStream,
    requests: &Arc<Mutex<Vec<String>>>,
    response: TestResponse,
) {
    let request = read_http_request(&mut stream);
    requests.lock().expect("requests mutex").push(request);
    let body = response.body.as_bytes();
    let raw = format!(
        "HTTP/1.1 {} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        response.status,
        body.len(),
        response.body
    );
    stream.write_all(raw.as_bytes()).expect("write response");
}

fn read_http_request(stream: &mut TcpStream) -> String {
    let mut raw = Vec::new();
    let mut buf = [0_u8; 512];
    loop {
        let count = stream.read(&mut buf).expect("read request");
        if count == 0 {
            break;
        }
        raw.extend_from_slice(&buf[..count]);
        if let Some(header_end) = find_header_end(&raw) {
            let headers = String::from_utf8_lossy(&raw[..header_end]).to_string();
            let content_length = headers
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length: "))
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or_default();
            let expected = header_end + content_length;
            while raw.len() < expected {
                let count = stream.read(&mut buf).expect("read request body");
                if count == 0 {
                    break;
                }
                raw.extend_from_slice(&buf[..count]);
            }
            break;
        }
    }
    String::from_utf8_lossy(&raw).into_owned()
}

fn find_header_end(raw: &[u8]) -> Option<usize> {
    raw.windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4)
}

fn request_body(raw: &str) -> &str {
    raw.split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .unwrap_or_default()
}

fn assert_contains_text(haystack: &str, needle: &str) {
    assert!(
        haystack.contains(needle),
        "expected request to contain {needle:?}, got:\n{haystack}"
    );
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> std::io::Result<Self> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "awiki-cli-rs2-{name}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
