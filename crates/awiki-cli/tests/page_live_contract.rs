use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn page_list_live_posts_authenticated_json_rpc_like_go() {
    let workspace = TempDir::new().expect("workspace");
    register_ready_identity(workspace.path(), "alice-page", "alice", "jwt-page");
    let server = TestServer::new(vec![TestResponse::ok(
        r#"{"jsonrpc":"2.0","result":{"count":2,"pages":[{"slug":"one"},{"slug":"two"}]},"id":"req-1"}"#,
    )]);
    write_service_config(workspace.path(), &server.base_url());

    let output = awiki_cmd(
        &["--identity", "alice-page", "page", "list"],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Fetched 2 content pages");
    assert_eq!(envelope["data"]["action"], "list_pages");
    assert_eq!(envelope["data"]["count"], 2);
    assert_eq!(envelope["data"]["identity"]["handle"], "alice");
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("POST /content/rpc HTTP/1.1"));
    assert_contains_text(&requests[0], "Authorization: Bearer jwt-page\r\n");
    let body: Value = serde_json::from_str(request_body(&requests[0])).expect("request body");
    assert_eq!(
        body,
        json!({
            "jsonrpc": "2.0",
            "id": "req-1",
            "method": "list",
            "params": {},
        })
    );
}

#[test]
fn page_create_live_posts_content_payload_like_go() {
    let workspace = TempDir::new().expect("workspace");
    register_ready_identity(workspace.path(), "alice-page", "alice", "jwt-page");
    let server = TestServer::new(vec![TestResponse::ok(
        r#"{"jsonrpc":"2.0","result":{"slug":"hello","title":"Hello","visibility":"draft"},"id":"req-1"}"#,
    )]);
    write_service_config(workspace.path(), &server.base_url());

    let output = awiki_cmd(
        &[
            "--identity",
            "alice-page",
            "page",
            "create",
            "--slug",
            " hello ",
            "--title",
            " Hello ",
            "--markdown",
            "Body",
            "--visibility",
            "DRAFT",
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Created content page hello");
    assert_eq!(envelope["data"]["action"], "create_page");
    assert_eq!(envelope["data"]["page"]["slug"], "hello");
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    let body: Value = serde_json::from_str(request_body(&requests[0])).expect("request body");
    assert_eq!(
        body,
        json!({
            "jsonrpc": "2.0",
            "id": "req-1",
            "method": "create",
            "params": {
                "slug": "hello",
                "title": "Hello",
                "body": "Body",
                "visibility": "draft",
            },
        })
    );
}

#[test]
fn page_get_live_maps_rpc_not_found_like_go() {
    let workspace = TempDir::new().expect("workspace");
    register_ready_identity(workspace.path(), "alice-page", "alice", "jwt-page");
    let server = TestServer::new(vec![TestResponse::ok(
        r#"{"jsonrpc":"2.0","error":{"code":-32002,"message":"page not found","data":{"slug":"missing"}},"id":"req-1"}"#,
    )]);
    write_service_config(workspace.path(), &server.base_url());

    let output = awiki_cmd(
        &[
            "--identity",
            "alice-page",
            "page",
            "get",
            "--slug",
            "missing",
        ],
        workspace.path(),
    );

    assert_code(&output, 5);
    let envelope = error_json(&output);
    assert_eq!(envelope["error"]["code"], "not_found");
    assert_eq!(
        envelope["error"]["message"],
        "service rpc error -32002: page not found"
    );
    assert_contains(
        &envelope["error"]["hint"],
        "Make sure the page exists and the active identity can access it.",
    );
}

#[test]
fn page_list_live_bootstraps_and_persists_jwt_like_go() {
    let workspace = TempDir::new().expect("workspace");
    register_ready_identity(workspace.path(), "alice-page", "alice", "");
    let server = TestServer::new(vec![
        TestResponse::ok(
            r#"{"jsonrpc":"2.0","result":{"access_token":"fresh-page","handle":"alice"},"id":"req-1"}"#,
        ),
        TestResponse::ok(
            r#"{"jsonrpc":"2.0","result":{"count":1,"pages":[{"slug":"fresh"}]},"id":"req-1"}"#,
        ),
    ]);
    write_service_config(workspace.path(), &server.base_url());

    let output = awiki_cmd(
        &["--identity", "alice-page", "page", "list"],
        workspace.path(),
    );

    assert_success(&output);
    assert_eq!(
        read_identity_auth_token(workspace.path(), "alice-page"),
        "fresh-page"
    );
    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].starts_with("POST /user-service/did-auth/rpc HTTP/1.1"));
    let bootstrap_body: Value =
        serde_json::from_str(request_body(&requests[0])).expect("bootstrap request body");
    assert_eq!(
        bootstrap_body,
        json!({
            "jsonrpc": "2.0",
            "id": "req-1",
            "method": "get_me",
            "params": {},
        })
    );
    assert!(requests[1].starts_with("POST /content/rpc HTTP/1.1"));
    assert_contains_text(&requests[1], "Authorization: Bearer fresh-page\r\n");
}

fn register_ready_identity(workspace: &Path, identity_name: &str, handle: &str, jwt_token: &str) {
    let create = awiki_cmd(
        &[
            "id",
            "create",
            "--name",
            "Alice Page",
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

fn read_identity_auth_token(workspace: &Path, identity_name: &str) -> String {
    let index_path = workspace.join("identities").join("index.json");
    let index: Value = serde_json::from_slice(&std::fs::read(&index_path).unwrap()).unwrap();
    let dir_name = index["credentials"][identity_name]["dir_name"]
        .as_str()
        .unwrap();
    let auth_path = workspace
        .join("identities")
        .join(dir_name)
        .join("auth.json");
    let auth: Value = serde_json::from_slice(&std::fs::read(auth_path).unwrap()).unwrap();
    auth["jwt_token"].as_str().unwrap_or_default().to_string()
}

fn write_service_config(workspace: &Path, base_url: &str) {
    std::fs::write(
        workspace.join("config.yaml"),
        format!("services:\n  service_base_url: {base_url}\n"),
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

fn assert_success(output: &Output) {
    assert_code(output, 0);
}

fn assert_code(output: &Output, code: i32) {
    assert_eq!(
        output.status.code(),
        Some(code),
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

fn error_json(output: &Output) -> Value {
    assert!(
        output.stdout.is_empty(),
        "stdout should be empty: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let envelope: Value =
        serde_json::from_slice(&output.stderr).expect("stderr should be a JSON error envelope");
    assert_eq!(envelope["ok"], false);
    envelope
}

fn assert_contains(value: &Value, needle: &str) {
    let haystack = value
        .as_str()
        .unwrap_or_else(|| panic!("expected string containing {needle:?}, got {value:?}"));
    assert!(
        haystack.contains(needle),
        "expected {haystack:?} to contain {needle:?}"
    );
}

fn assert_contains_text(haystack: &str, needle: &str) {
    assert!(
        haystack.contains(needle),
        "expected request to contain {needle:?}, got:\n{haystack}"
    );
}

fn request_body(raw: &str) -> &str {
    raw.split("\r\n\r\n").nth(1).unwrap_or_default()
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
        let address = format!("http://{}", listener.local_addr().expect("local addr"));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let server_requests = Arc::clone(&requests);
        let join = thread::spawn(move || {
            for response in responses {
                let Ok((stream, _)) = listener.accept() else {
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
            "awiki-cli-rs2-page-live-test-{}-{nanos}",
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
