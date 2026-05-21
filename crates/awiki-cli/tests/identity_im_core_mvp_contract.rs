use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[test]
fn identity_im_core_mvp_register_and_refresh_dry_run_keep_legacy_contract() {
    let workspace = TempDir::new().expect("workspace");

    let register = success_json(&awiki_cmd_with_env(
        &[
            "--dry-run",
            "--identity",
            "alice-local",
            "id",
            "register",
            "--handle",
            "Alice",
            "--phone",
            "+15551234567",
            "--otp",
            "123456",
            "--invite-code",
            "invite-1",
        ],
        workspace.path(),
        &[("AWIKI_USE_IM_CORE_MVP", "1")],
    ));
    assert_eq!(
        register["summary"],
        "Dry run: handle registration flow planned"
    );
    assert_eq!(register["data"]["plan"]["action"], "register_handle");
    assert_eq!(register["data"]["plan"]["identity_name"], "alice-local");
    assert_eq!(register["data"]["plan"]["full_handle"], "alice.awiki.ai");
    assert_eq!(register["data"]["plan"]["phone"], "+15551234567");
    assert!(register["data"]["plan"]["remote_calls"]
        .as_array()
        .unwrap()
        .contains(&json!("did-auth.register")));

    let refresh = success_json(&awiki_cmd_with_env(
        &[
            "--identity",
            "alice-local",
            "id",
            "refresh-token",
            "--dry-run",
        ],
        workspace.path(),
        &[("AWIKI_USE_IM_CORE_MVP", "1")],
    ));
    assert_eq!(refresh["data"]["plan"]["action"], "refresh_token");
    assert_eq!(refresh["data"]["plan"]["identity_name"], "alice-local");
    assert_eq!(
        refresh["data"]["plan"]["auth_flow"],
        "did_auth_get_me_without_stored_bearer"
    );
}

#[test]
fn identity_im_core_mvp_refresh_selects_identity_before_legacy_auth() {
    let workspace = TempDir::new().expect("workspace");
    let workspace_home = workspace.path().join(".awiki-cli");
    let manager = identity_manager(&workspace_home);
    manager
        .save(awiki_cli::identity::types::SaveInput {
            identity_name: "alice".to_string(),
            did: "did:wba:awiki.ai:user:e1_alice".to_string(),
            unique_id: "e1_alice".to_string(),
            display_name: "Alice".to_string(),
            ..Default::default()
        })
        .expect("save alice");
    manager
        .save(awiki_cli::identity::types::SaveInput {
            identity_name: "bob".to_string(),
            did: "did:wba:awiki.ai:user:e1_bob".to_string(),
            unique_id: "e1_bob".to_string(),
            display_name: "Bob".to_string(),
            ..Default::default()
        })
        .expect("save bob");

    let result = awiki_cmd_with_env(
        &["--identity", "bob", "id", "refresh-token"],
        workspace.path(),
        &[("AWIKI_USE_IM_CORE_MVP", "1")],
    );
    assert_code(&result, 3);
    let result = error_json(&result);
    assert_eq!(result["error"]["code"], "auth_required");
    assert!(result["error"]["message"].as_str().unwrap().contains("bob"));
}

#[test]
fn identity_im_core_mvp_profile_get_self_routes_get_me_through_bridge() {
    let workspace = TempDir::new().expect("workspace");
    let server = TestServer::new(vec![
        TestResponse::ok(register_alice_response()),
        TestResponse::ok(
            r###"{"jsonrpc":"2.0","result":{"nick_name":"Alice Remote","bio":"Rust port","tags":["rust","cli"],"profile_md":"## Alice"},"id":"req-1"}"###,
        ),
    ]);
    write_service_config(&workspace.path().join(".awiki-cli"), &server.base_url());

    let register = awiki_cmd_with_env(
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
        workspace.path(),
        &[],
    );
    assert_code(&register, 0);

    let profile = success_json(&awiki_cmd_with_env(
        &["--identity", "alice", "id", "profile", "get", "--self"],
        workspace.path(),
        &[("AWIKI_USE_IM_CORE_MVP", "1")],
    ));
    assert_eq!(profile["summary"], "Fetched current identity profile");
    assert_eq!(profile["data"]["subject"], "self");
    assert_eq!(profile["data"]["profile"]["nick_name"], "Alice Remote");
    assert_eq!(profile["data"]["profile"]["profile_md"], "## Alice");

    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests[1].starts_with("POST /user-service/did/profile/rpc HTTP/1.1"));
    assert_contains_text(&requests[1], "Authorization: Bearer jwt-register\r\n");
    let body: Value = serde_json::from_str(request_body(&requests[1])).expect("request body");
    assert_eq!(
        body,
        json!({
            "jsonrpc": "2.0",
            "id": "req-1",
            "method": "get_me",
            "params": {},
        })
    );
}

#[test]
fn identity_im_core_mvp_profile_set_routes_update_me_through_bridge() {
    let workspace = TempDir::new().expect("workspace");
    let markdown_file = workspace.path().join("profile.md");
    std::fs::write(&markdown_file, " \n# Alice\n\nProfile body\n ").expect("write markdown");
    let server = TestServer::new(vec![
        TestResponse::ok(register_alice_response()),
        TestResponse::ok(
            r###"{"jsonrpc":"2.0","result":{"nick_name":"Alice Updated","bio":"Rust port","tags":["rust","cli"],"profile_md":" \n# Alice\n\nProfile body\n "},"id":"req-1"}"###,
        ),
    ]);
    write_service_config(&workspace.path().join(".awiki-cli"), &server.base_url());

    let register = awiki_cmd_with_env(
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
        workspace.path(),
        &[],
    );
    assert_code(&register, 0);

    let profile = success_json(&awiki_cmd_with_env(
        &[
            "--identity",
            "alice",
            "id",
            "profile",
            "set",
            "--display-name",
            " Alice Updated ",
            "--bio",
            " Rust port ",
            "--tags",
            " rust, ,cli ",
            "--markdown-file",
            markdown_file.to_str().unwrap(),
        ],
        workspace.path(),
        &[("AWIKI_USE_IM_CORE_MVP", "1")],
    ));
    assert_eq!(profile["summary"], "Profile updated successfully");
    assert_eq!(profile["data"]["action"], "update_profile");
    assert_eq!(
        profile["data"]["changed_fields"],
        json!(["display_name", "bio", "tags", "profile_md"])
    );
    assert_eq!(profile["data"]["profile"]["nick_name"], "Alice Updated");
    assert_eq!(
        profile["data"]["profile"]["profile_md"],
        " \n# Alice\n\nProfile body\n "
    );

    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests[1].starts_with("POST /user-service/did/profile/rpc HTTP/1.1"));
    assert_contains_text(&requests[1], "Authorization: Bearer jwt-register\r\n");
    let body: Value = serde_json::from_str(request_body(&requests[1])).expect("request body");
    assert_eq!(
        body,
        json!({
            "jsonrpc": "2.0",
            "id": "req-1",
            "method": "update_me",
            "params": {
                "nick_name": "Alice Updated",
                "bio": "Rust port",
                "tags": ["rust", "cli"],
                "profile_md": " \n# Alice\n\nProfile body\n ",
            },
        })
    );
}

#[test]
fn identity_im_core_mvp_resolve_handle_routes_directory_sequence() {
    let workspace = TempDir::new().expect("workspace");
    let server = TestServer::new(vec![
        TestResponse::ok(register_alice_response()),
        TestResponse::ok(
            r#"{"jsonrpc":"2.0","result":{"did":"did:wba:awiki.ai:alice:e1_remote","handle":"alice","full_handle":"alice.awiki.ai"},"id":"req-1"}"#,
        ),
        TestResponse::ok(r#"{"jsonrpc":"2.0","result":{"nick_name":"Alice Public"},"id":"req-1"}"#),
        TestResponse::ok(
            r#"{"jsonrpc":"2.0","result":{"did":"did:wba:awiki.ai:alice:e1_remote","service_endpoint":"https://service.example"},"id":"req-1"}"#,
        ),
    ]);
    write_service_config(&workspace.path().join(".awiki-cli"), &server.base_url());

    let register = awiki_cmd_with_env(
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
        workspace.path(),
        &[],
    );
    assert_code(&register, 0);

    let resolve = success_json(&awiki_cmd_with_env(
        &["id", "resolve", "--handle", "Alice"],
        workspace.path(),
        &[("AWIKI_USE_IM_CORE_MVP", "1")],
    ));
    assert_eq!(resolve["summary"], "Identity resolved successfully");
    assert_eq!(
        resolve["data"]["lookup"]["did"],
        "did:wba:awiki.ai:alice:e1_remote"
    );
    assert_eq!(
        resolve["data"]["public_profile"]["nick_name"],
        "Alice Public"
    );
    assert_eq!(
        resolve["data"]["resolve"]["did"],
        "did:wba:awiki.ai:alice:e1_remote"
    );

    let requests = server.requests();
    assert_eq!(requests.len(), 4);
    let lookup_body: Value = serde_json::from_str(request_body(&requests[1])).unwrap();
    let profile_body: Value = serde_json::from_str(request_body(&requests[2])).unwrap();
    let resolve_body: Value = serde_json::from_str(request_body(&requests[3])).unwrap();
    assert_eq!(lookup_body["method"], "lookup");
    assert_eq!(lookup_body["params"], json!({ "handle": "alice.awiki.ai" }));
    assert_eq!(profile_body["method"], "get_public_profile");
    assert_eq!(
        profile_body["params"],
        json!({ "did": "did:wba:awiki.ai:alice:e1_remote" })
    );
    assert_eq!(resolve_body["method"], "resolve");
    assert_eq!(
        resolve_body["params"],
        json!({ "did": "did:wba:awiki.ai:alice:e1_remote" })
    );
}

#[test]
fn identity_im_core_mvp_resolve_did_keeps_nonfatal_directory_warnings() {
    let workspace = TempDir::new().expect("workspace");
    let did = "did:wba:awiki.ai:alice:e1_remote";
    let server = TestServer::new(vec![
        TestResponse::ok(register_alice_response()),
        TestResponse::ok(&format!(
            r#"{{"jsonrpc":"2.0","result":{{"did":"{did}","service_endpoint":"https://service.example"}},"id":"req-1"}}"#
        )),
        TestResponse::status(502, "lookup unavailable"),
        TestResponse::status(502, "profile unavailable"),
    ]);
    write_service_config(&workspace.path().join(".awiki-cli"), &server.base_url());

    let register = awiki_cmd_with_env(
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
        workspace.path(),
        &[],
    );
    assert_code(&register, 0);

    let resolve = success_json(&awiki_cmd_with_env(
        &["id", "resolve", "--did", did],
        workspace.path(),
        &[("AWIKI_USE_IM_CORE_MVP", "1")],
    ));
    assert_eq!(resolve["summary"], "Identity resolved successfully");
    assert_eq!(resolve["data"]["resolve"]["did"], did);
    assert!(resolve["data"].get("lookup").is_none());
    assert!(resolve["data"].get("public_profile").is_none());
    assert_eq!(
        resolve["warnings"],
        json!([
            "Handle lookup failed: service error 502: lookup unavailable",
            "Public profile lookup failed: service error 502: profile unavailable",
        ])
    );
}

fn awiki_cmd_with_env(args: &[&str], workspace: &Path, envs: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_awiki-cli"));
    command
        .args(args)
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace.join(".awiki-cli"))
        .env("HOME", workspace)
        .env("AWIKI_CLI_UPDATE_CACHE_ONLY", "1")
        .env_remove("AWIKI_WORKSPACE")
        .env_remove("AWIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_HOME")
        .env_remove("AVIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_FORMAT")
        .env_remove("AVIKI_FORMAT");
    for (key, value) in envs {
        command.env(key, value);
    }
    command.output().expect("run awiki-cli")
}

fn write_service_config(workspace: &Path, base_url: &str) {
    std::fs::create_dir_all(workspace).unwrap();
    std::fs::write(
        workspace.join("config.yaml"),
        format!(
            "services:\n  service_base_url: {base_url}\n  anp_service_endpoint: https://awiki.ai/anp-im/rpc\n  anp_service_did: did:wba:awiki.ai\n"
        ),
    )
    .unwrap();
}

fn register_alice_response() -> &'static str {
    r#"{"jsonrpc":"2.0","result":{"did":"did:wba:awiki.ai:alice:e1_remote","user_id":"user-alice","message":"Registration successful","handle":"alice","domain":"awiki.ai","full_handle":"alice.awiki.ai","access_token":"jwt-register"},"id":"req-1"}"#
}

fn identity_manager(workspace: &Path) -> awiki_cli::identity::Manager {
    awiki_cli::identity::Manager::new(awiki_cli::config::Paths {
        workspace_home_dir: path_string(workspace),
        root_dir: path_string(workspace),
        config_dir: path_string(&workspace.join("config")),
        data_dir: path_string(&workspace.join("data")),
        state_dir: path_string(&workspace.join("state")),
        cache_dir: path_string(&workspace.join("cache")),
        logs_dir: path_string(&workspace.join("logs")),
        config_file: path_string(&workspace.join("config").join("config.yaml")),
        identity_dir: path_string(&workspace.join("identities")),
        database_file: path_string(&workspace.join("data").join("awiki.db")),
        legacy_credentials_dir: path_string(&workspace.join("legacy")),
        legacy_data_dir: path_string(&workspace.join("legacy-data")),
    })
}

fn success_json(output: &Output) -> Value {
    assert_code(output, 0);
    assert!(
        output.stderr.is_empty(),
        "stderr should be empty: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("success JSON")
}

fn error_json(output: &Output) -> Value {
    assert!(
        !output.status.success(),
        "command should fail\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stderr).expect("error JSON")
}

fn assert_code(output: &Output, expected: i32) {
    assert_eq!(
        output.status.code(),
        Some(expected),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn request_body(raw: &str) -> &str {
    raw.split("\r\n\r\n").nth(1).unwrap_or_default()
}

fn assert_contains_text(haystack: &str, needle: &str) {
    assert!(
        haystack.contains(needle),
        "expected request to contain {needle:?}, got:\n{haystack}"
    );
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

    fn status(status: u16, body: &str) -> Self {
        Self {
            status,
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
            "awiki-cli-rs2-id-im-core-test-{}-{nanos}",
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
