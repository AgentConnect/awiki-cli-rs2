use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[test]
fn identity_recover_phone_without_otp_live_posts_send_otp_and_does_not_create_identity_like_go() {
    let workspace = TempDir::new().expect("workspace");
    let server = TestServer::new(vec![TestResponse::ok(
        r#"{"jsonrpc":"2.0","result":{"sent":true},"id":"req-1"}"#,
    )]);
    write_service_config(workspace.path(), &server.base_url());

    let output = awiki_cmd(
        &[
            "id",
            "recover",
            "--handle",
            " Alice ",
            "--phone",
            "13800138000",
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(
        envelope["summary"],
        "OTP sent for handle alice.awiki.ai recovery"
    );
    assert_eq!(envelope["data"]["action"], "send_recover_otp");
    assert_eq!(envelope["data"]["identity_name"], "alice");
    assert_eq!(envelope["data"]["handle"], "alice");
    assert_eq!(envelope["data"]["full_handle"], "alice.awiki.ai");
    assert_eq!(envelope["data"]["method"], "phone");
    assert_eq!(envelope["data"]["phone"], "+8613800138000");
    assert_eq!(envelope["data"]["verification_state"], "otp_sent");
    assert_eq!(envelope["data"]["result"], json!({ "sent": true }));

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("POST /user-service/handle/rpc HTTP/1.1"));
    let body: Value = serde_json::from_str(request_body(&requests[0])).expect("request body");
    assert_eq!(
        body,
        json!({
            "jsonrpc": "2.0",
            "id": "req-1",
            "method": "send_otp",
            "params": { "phone": "+8613800138000" },
        })
    );
    assert!(
        !workspace
            .path()
            .join("identities")
            .join("index.json")
            .exists(),
        "send_recover_otp should not create a final recovered identity index"
    );
}

#[test]
fn identity_recover_phone_otp_live_posts_recover_handle_and_finalizes_identity_like_go() {
    let workspace = TempDir::new().expect("workspace");
    let server = TestServer::new(vec![
        TestResponse::ok(register_alice_response()),
        TestResponse::ok(
            r#"{"jsonrpc":"2.0","result":{"did":"did:wba:awiki.ai:alice:e1_recovered","user_id":"user-alice-recovered","message":"Recovery successful","handle":"alice","domain":"awiki.ai","full_handle":"alice.awiki.ai","access_token":"jwt-recover"},"id":"req-1"}"#,
        ),
    ]);
    write_service_config(workspace.path(), &server.base_url());
    register_alice(workspace.path());
    let old = read_stored_identity(workspace.path(), "alice");
    let old_dir_name = old.index["dir_name"].as_str().unwrap().to_string();
    assert_eq!(old.index["did"], "did:wba:awiki.ai:alice:e1_remote");

    let output = awiki_cmd(
        &[
            "id",
            "recover",
            "--handle",
            " Alice ",
            "--phone",
            "13800138000",
            "--otp",
            " 65 43 21 ",
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(
        envelope["summary"],
        "Handle alice.awiki.ai recovered successfully"
    );
    assert_eq!(envelope["data"]["action"], "recover_handle");
    assert_eq!(envelope["data"]["full_handle"], "alice.awiki.ai");
    assert_eq!(envelope["data"]["final_identity_name"], "alice");
    assert_eq!(envelope["data"]["archived_identities"], json!(["alice"]));
    assert_eq!(
        envelope["data"]["archived_dids"],
        json!(["did:wba:awiki.ai:alice:e1_remote"])
    );
    assert!(envelope["data"].get("temp_identity_name").is_none());
    assert!(envelope["data"].get("active_before").is_none());
    assert!(envelope["data"].get("old_dids").is_none());
    assert_eq!(
        envelope["data"]["store_merge_counts"],
        json!({
            "messages": 0,
            "contacts": 0,
            "contact_handle_bindings": 0,
            "relationship_events": 0,
            "groups": 0,
            "group_members": 0,
        })
    );
    assert_eq!(
        envelope["data"]["e2ee_cleanup_counts"],
        json!({
            "e2ee_outbox": 0,
            "e2ee_sessions": 0,
        })
    );
    assert!(envelope["data"]["backup_path"]
        .as_str()
        .unwrap()
        .contains(".legacy-backup/recover-handle/"));
    assert!(envelope["data"]["backup_path"]
        .as_str()
        .unwrap()
        .contains("alice.awiki.ai"));
    assert_eq!(envelope["data"]["identity"]["identity_name"], "alice");
    assert_eq!(
        envelope["data"]["identity"]["did"],
        "did:wba:awiki.ai:alice:e1_recovered"
    );
    assert_eq!(envelope["data"]["identity"]["handle"], "alice");
    assert_eq!(
        envelope["data"]["identity"]["full_handle"],
        "alice.awiki.ai"
    );
    assert!(envelope["data"]["identity"]["has_jwt"]
        .as_bool()
        .expect("identity has_jwt bool"));
    assert!(envelope["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|warning| warning
            .as_str()
            .unwrap()
            .contains("Archived 1 same-handle local identities")));

    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests[1].starts_with("POST /user-service/did-auth/rpc HTTP/1.1"));
    let body: Value = serde_json::from_str(request_body(&requests[1])).expect("request body");
    assert_eq!(body["jsonrpc"], "2.0");
    assert_eq!(body["id"], "req-1");
    assert_eq!(body["method"], "recover_handle");
    assert_eq!(body["params"]["handle"], "alice.awiki.ai");
    assert_eq!(body["params"]["phone"], "+8613800138000");
    assert_eq!(body["params"]["otp_code"], "654321");
    assert!(body["params"]["did_document"].is_object());
    assert!(body["params"]["did_document"]["id"]
        .as_str()
        .unwrap_or_default()
        .starts_with("did:wba:awiki.ai:alice:"));

    let stored = read_stored_identity(workspace.path(), "alice");
    assert_eq!(stored.index["handle"], "alice");
    assert_eq!(stored.index["full_handle"], "alice.awiki.ai");
    assert_eq!(stored.index["did"], "did:wba:awiki.ai:alice:e1_recovered");
    assert_eq!(stored.index["user_id"], "user-alice-recovered");
    assert_eq!(stored.identity["handle"], "alice");
    assert_eq!(stored.identity["full_handle"], "alice.awiki.ai");
    assert_eq!(
        stored.identity["did"],
        "did:wba:awiki.ai:alice:e1_recovered"
    );
    assert_eq!(stored.identity["user_id"], "user-alice-recovered");
    assert_eq!(stored.auth["jwt_token"], "jwt-recover");

    let index_path = workspace.path().join("identities").join("index.json");
    let index: Value = serde_json::from_slice(&std::fs::read(&index_path).unwrap()).unwrap();
    assert!(index["credentials"].get("alice-recover-tmp").is_none());
    assert!(workspace
        .path()
        .join("identities")
        .join(old_dir_name)
        .exists());
    assert!(workspace
        .path()
        .join(".legacy-backup")
        .join("recover-handle")
        .exists());
}

fn write_service_config(workspace: &Path, base_url: &str) {
    std::fs::write(
        workspace.join("config.yaml"),
        format!(
            "services:\n  service_base_url: {base_url}\n  anp_service_endpoint: https://awiki.ai/anp-im/rpc\n  anp_service_did: did:wba:awiki.ai\n"
        ),
    )
    .unwrap();
}

fn awiki_cmd(args: &[&str], workspace: &Path) -> Output {
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
        .env_remove("AVIKI_FORMAT");
    command.output().expect("run awiki-cli binary")
}

fn register_alice(workspace: &Path) {
    let output = awiki_cmd(
        &[
            "id",
            "register",
            "--handle",
            "alice",
            "--phone",
            "13800138000",
            "--otp",
            "123456",
        ],
        workspace,
    );
    assert_success(&output);
}

fn register_alice_response() -> &'static str {
    r#"{"jsonrpc":"2.0","result":{"did":"did:wba:awiki.ai:alice:e1_remote","user_id":"user-alice","message":"Registration successful","handle":"alice","domain":"awiki.ai","full_handle":"alice.awiki.ai","access_token":"jwt-register"},"id":"req-1"}"#
}

fn assert_success(output: &Output) {
    assert_eq!(
        output.status.code(),
        Some(0),
        "unexpected exit status; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn success_json(output: &Output) -> Value {
    assert!(
        output.stderr.is_empty(),
        "stderr should be empty: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be a JSON success envelope");
    assert_eq!(envelope["ok"], true);
    envelope
}

fn request_body(raw: &str) -> &str {
    raw.split("\r\n\r\n").nth(1).unwrap_or_default()
}

struct StoredIdentity {
    index: Value,
    identity: Value,
    auth: Value,
}

fn read_stored_identity(workspace: &Path, identity_name: &str) -> StoredIdentity {
    let index_path = workspace.join("identities").join("index.json");
    let index: Value = serde_json::from_slice(&std::fs::read(&index_path).unwrap()).unwrap();
    let entry = index["credentials"][identity_name].clone();
    let dir_name = entry["dir_name"].as_str().unwrap();
    let identity_dir = workspace.join("identities").join(dir_name);
    StoredIdentity {
        index: entry,
        identity: serde_json::from_slice(
            &std::fs::read(identity_dir.join("identity.json")).unwrap(),
        )
        .unwrap(),
        auth: serde_json::from_slice(&std::fs::read(identity_dir.join("auth.json")).unwrap())
            .unwrap(),
    }
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

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> std::io::Result<Self> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "awiki-cli-rs2-identity-recover-live-test-{}-{nanos}",
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
