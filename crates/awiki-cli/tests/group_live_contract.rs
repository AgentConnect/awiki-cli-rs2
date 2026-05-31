use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[test]
fn group_get_live_posts_group_get_and_maps_snapshot_like_go() {
    let workspace = TempDir::new("group-live-get").expect("workspace");
    register_ready_group_identity(workspace.path(), "alice-group", "alice", "jwt-alice");
    let group_did = "did:wba:awiki.ai:groups:demo:e1_group";
    let server = TestServer::new(vec![TestResponse::ok(&json_rpc_result(json!({
        "group_did": group_did,
        "group_state_version": "v7",
        "group_event_seq": 7,
        "group_profile": {
            "display_name": "Demo Group",
            "description": "Group contract fixture",
            "slug": "demo-group"
        },
        "group_policy": {
            "message_security_profile": "transport-protected",
            "admission_mode": "open-join"
        },
        "member_role": "member",
        "member_status": "active",
        "member_count": 2,
        "source": "remote_http"
    })))]);
    write_group_config(workspace.path(), &server.base_url());

    let output = awiki_cmd(
        &[
            "--identity",
            "alice-group",
            "group",
            "get",
            "--group",
            group_did,
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Loaded group snapshot");
    assert_eq!(envelope["data"]["group"]["group_did"], group_did);
    assert_eq!(envelope["data"]["group"]["name"], "Demo Group");
    assert_eq!(
        envelope["data"]["group"]["group_profile"]["description"],
        "Group contract fixture"
    );
    assert_eq!(envelope["data"]["group"]["member_role"], "member");
    assert_eq!(envelope["data"]["source"], "remote_http");

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("POST /im/rpc HTTP/1.1"));
    assert_contains_text(&requests[0], "Authorization: Bearer jwt-alice\r\n");
    let body: Value = serde_json::from_str(request_body(&requests[0])).expect("request body");
    assert_eq!(body["method"], "group.get");
    assert_eq!(body["params"]["meta"]["profile"], "anp.group.local.v1");
    assert_eq!(
        body["params"]["meta"]["target"],
        json!({"kind": "group", "did": group_did})
    );
    assert_eq!(body["params"]["body"]["group_did"], group_did);
    assert!(body["params"].get("auth").is_none());
}

#[test]
fn group_members_live_posts_group_list_members_and_maps_members_like_go() {
    let workspace = TempDir::new("group-live-members").expect("workspace");
    register_ready_group_identity(workspace.path(), "alice-group", "alice", "jwt-alice");
    let group_did = "did:wba:awiki.ai:groups:demo:e1_group";
    let bob_did = "did:wba:awiki.ai:bob:e1_bob";
    let server = TestServer::new(vec![TestResponse::ok(&json_rpc_result(json!({
        "members": [
            {
                "member_did": bob_did,
                "member_handle": "bob.awiki.ai",
                "role": "member",
                "status": "active",
                "joined_at": "2026-04-07T01:02:03Z"
            }
        ],
        "total": 1,
        "source": "remote_http"
    })))]);
    write_group_config(workspace.path(), &server.base_url());

    let output = awiki_cmd(
        &[
            "--identity",
            "alice-group",
            "group",
            "members",
            "--group",
            group_did,
            "--limit",
            "25",
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Loaded 1 group members");
    assert_eq!(envelope["data"]["group"], group_did);
    assert_eq!(envelope["data"]["total"], 1);
    assert_eq!(envelope["data"]["members"][0]["member_did"], bob_did);
    assert_eq!(envelope["data"]["members"][0]["member_handle"], "bob");
    assert_eq!(envelope["data"]["members"][0]["role"], "member");
    assert_eq!(envelope["data"]["source"], "remote_http");

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("POST /im/rpc HTTP/1.1"));
    assert_contains_text(&requests[0], "Authorization: Bearer jwt-alice\r\n");
    let body: Value = serde_json::from_str(request_body(&requests[0])).expect("request body");
    assert_eq!(body["method"], "group.list_members");
    assert_eq!(body["params"]["meta"]["profile"], "anp.group.local.v1");
    assert_eq!(
        body["params"]["meta"]["target"],
        json!({"kind": "group", "did": group_did})
    );
    assert_eq!(body["params"]["body"]["group_did"], group_did);
    assert_eq!(body["params"]["body"]["limit"], 25);
    assert!(body["params"].get("auth").is_none());
}

#[test]
fn group_control_websocket_mode_stays_http_and_warns_like_go() {
    let workspace = TempDir::new("group-live-ws-control-warning").expect("workspace");
    register_ready_group_identity(workspace.path(), "alice-group", "alice", "jwt-alice");
    let group_did = "did:wba:awiki.ai:groups:demo:e1_group";
    let server = TestServer::new(vec![
        TestResponse::ok(&json_rpc_result(json!({
            "groups": [{
                "group_did": group_did,
                "name": "Demo Group",
                "member_role": "owner",
                "member_status": "active"
            }],
            "total": 1,
            "source": "remote_http"
        }))),
        TestResponse::ok(&json_rpc_result(json!({
            "group_did": group_did,
            "group_profile": {"display_name": "Demo Group"},
            "member_role": "owner",
            "member_status": "active",
            "source": "remote_http"
        }))),
        TestResponse::ok(&json_rpc_result(json!({
            "members": [{
                "member_did": "did:wba:awiki.ai:bob:e1_bob",
                "member_handle": "bob.awiki.ai",
                "role": "member",
                "status": "active"
            }],
            "total": 1,
            "source": "remote_http"
        }))),
    ]);
    write_group_websocket_config(workspace.path(), &server.base_url());

    let list = success_json(&awiki_cmd(
        &["--identity", "alice-group", "group", "list"],
        workspace.path(),
    ));
    assert_eq!(list["summary"], "Loaded 1 groups");
    assert_eq!(list["data"]["source"], "remote_http");
    assert_has_group_control_websocket_warning(&list);

    let get = success_json(&awiki_cmd(
        &[
            "--identity",
            "alice-group",
            "group",
            "get",
            "--group",
            group_did,
        ],
        workspace.path(),
    ));
    assert_eq!(get["summary"], "Loaded group snapshot");
    assert_eq!(get["data"]["source"], "remote_http");
    assert_has_group_control_websocket_warning(&get);

    let members = success_json(&awiki_cmd(
        &[
            "--identity",
            "alice-group",
            "group",
            "members",
            "--group",
            group_did,
        ],
        workspace.path(),
    ));
    assert_eq!(members["summary"], "Loaded 1 group members");
    assert_eq!(members["data"]["source"], "remote_http");
    assert_has_group_control_websocket_warning(&members);

    let requests = server.requests();
    assert_eq!(requests.len(), 3);
    let methods = requests
        .iter()
        .map(|raw| {
            serde_json::from_str::<Value>(request_body(raw)).expect("request body")["method"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        methods,
        vec!["group.list", "group.get", "group.list_members"]
    );
    for request in requests {
        assert!(request.starts_with("POST /im/rpc HTTP/1.1"));
        assert_contains_text(&request, "Authorization: Bearer jwt-alice\r\n");
    }
}

#[test]
fn group_add_live_error_preserves_go_owner_hint() {
    let workspace = TempDir::new("group-live-add-owner-hint").expect("workspace");
    register_ready_group_identity(workspace.path(), "bob-group", "bob", "jwt-bob");
    let group_did = "did:wba:awiki.ai:groups:demo:e1_group";
    let member_did = "did:wba:awiki.ai:alice:e1_alice";
    let server = TestServer::new(vec![TestResponse::ok(&json_rpc_error(
        2403,
        "actor cannot add members",
    ))]);
    write_group_config(workspace.path(), &server.base_url());

    let output = awiki_cmd(
        &[
            "--identity",
            "bob-group",
            "group",
            "add",
            "--group",
            group_did,
            "--member",
            member_did,
            "--role",
            "member",
        ],
        workspace.path(),
    );

    assert_code(&output, 1);
    let envelope = error_json(&output);
    assert_eq!(envelope["error"]["code"], "internal_error");
    assert_contains_text(
        envelope["error"]["message"].as_str().unwrap_or_default(),
        "actor cannot add members",
    );
    assert_contains_text(
        envelope["error"]["hint"].as_str().unwrap_or_default(),
        "owner role required for membership changes",
    );

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("POST /im/rpc HTTP/1.1"));
    let body: Value = serde_json::from_str(request_body(&requests[0])).expect("request body");
    assert_eq!(body["method"], "group.add");
    assert_eq!(body["params"]["body"]["member_did"], member_did);
    assert_eq!(body["params"]["body"]["role"], "member");
}

#[test]
fn msg_send_group_live_posts_group_send_and_maps_message_id_suffix_like_go() {
    let workspace = TempDir::new("group-live-send").expect("workspace");
    register_ready_group_identity(workspace.path(), "alice-group", "alice", "jwt-alice");
    let group_did = "did:wba:awiki.ai:groups:demo:e1_group";
    let server = TestServer::new(vec![TestResponse::ok(&json_rpc_result(json!({
        "accepted": true,
        "final_acceptance": true,
        "group_did": group_did,
        "message_id": "server-message-id",
        "operation_id": "server-operation-id",
        "group_event_seq": "42",
        "group_state_version": "v42",
        "accepted_at": "2026-04-07T01:02:03Z",
        "source": "remote_http"
    })))]);
    write_group_config(workspace.path(), &server.base_url());

    let output = awiki_cmd(
        &[
            "--identity",
            "alice-group",
            "msg",
            "send",
            "--group",
            group_did,
            "--text",
            "hello group",
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Sent a group text message");
    assert_eq!(envelope["data"]["action"], "send_message");
    assert_eq!(envelope["data"]["target"]["kind"], "group");
    assert_eq!(envelope["data"]["target"]["did"], group_did);
    assert_eq!(envelope["data"]["message"]["type"], "text");
    assert_eq!(envelope["data"]["message"]["secure"], false);
    assert_eq!(envelope["data"]["message"]["id"], format!("{group_did}:42"));
    assert_eq!(
        envelope["data"]["message"]["sent_at"],
        "2026-04-07T01:02:03Z"
    );
    assert_eq!(
        envelope["data"]["delivery"]["message_id"],
        "server-message-id"
    );
    assert_eq!(
        envelope["data"]["delivery"]["operation_id"],
        "server-operation-id"
    );
    assert_eq!(envelope["data"]["delivery"]["group_event_seq"], "42");
    assert_eq!(envelope["data"]["source"], "remote_http");

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("POST /im/rpc HTTP/1.1"));
    assert_contains_text(&requests[0], "Authorization: Bearer jwt-alice\r\n");
    let body: Value = serde_json::from_str(request_body(&requests[0])).expect("request body");
    assert_eq!(body["method"], "group.send");
    assert_eq!(body["params"]["meta"]["profile"], "anp.group.base.v1");
    assert_eq!(
        body["params"]["meta"]["target"],
        json!({"kind": "group", "did": group_did})
    );
    assert_eq!(body["params"]["meta"]["content_type"], "text/plain");
    assert!(body["params"]["meta"]["message_id"]
        .as_str()
        .expect("message id")
        .starts_with("msg-"));
    assert_eq!(
        body["params"]["auth"]["scheme"],
        "anp-rfc9421-origin-proof-v1"
    );
    assert_eq!(body["params"]["body"], json!({"text": "hello group"}));
}

fn register_ready_group_identity(
    workspace: &Path,
    identity_name: &str,
    handle: &str,
    jwt_token: &str,
) {
    let create = awiki_cmd(
        &[
            "--migration",
            "id",
            "create",
            "--name",
            "Group User",
            "--identity",
            identity_name,
        ],
        workspace,
    );
    assert_success(&create);

    let index_path = workspace.join("identities").join("index.json");
    let mut index: Value = serde_json::from_slice(&std::fs::read(&index_path).unwrap()).unwrap();
    let did = format!("did:wba:awiki.ai:{handle}:e1_{handle}");
    index["credentials"][identity_name]["did"] = json!(did);
    index["credentials"][identity_name]["handle"] = json!(handle);
    index["credentials"][identity_name]["full_handle"] = json!(format!("{handle}.awiki.ai"));
    index["credentials"][identity_name]["user_id"] = json!(format!("user-{handle}"));
    std::fs::write(&index_path, serde_json::to_vec_pretty(&index).unwrap()).unwrap();

    let dir_name = index["credentials"][identity_name]["dir_name"]
        .as_str()
        .unwrap();
    let identity_dir = workspace.join("identities").join(dir_name);
    let identity_path = identity_dir.join("identity.json");
    let mut identity: Value =
        serde_json::from_slice(&std::fs::read(&identity_path).unwrap()).unwrap();
    let original_did = identity["did"].as_str().unwrap().to_string();
    identity["handle"] = json!(handle);
    identity["full_handle"] = json!(format!("{handle}.awiki.ai"));
    identity["user_id"] = json!(format!("user-{handle}"));
    std::fs::write(
        &identity_path,
        serde_json::to_vec_pretty(&identity).unwrap(),
    )
    .unwrap();

    let document_path = identity_dir.join("did_document.json");
    let mut document: Value =
        serde_json::from_slice(&std::fs::read(&document_path).unwrap()).unwrap();
    document["id"] = json!(original_did);
    std::fs::write(
        &document_path,
        serde_json::to_vec_pretty(&document).unwrap(),
    )
    .unwrap();

    std::fs::write(
        identity_dir.join("auth.json"),
        serde_json::to_vec_pretty(&json!({ "jwt_token": jwt_token })).unwrap(),
    )
    .unwrap();
}

fn write_group_config(workspace: &Path, base_url: &str) {
    std::fs::write(
        workspace.join("config.yaml"),
        format!("runtime:\n  mode: http\nservices:\n  service_base_url: {base_url}\n"),
    )
    .unwrap();
}

fn write_group_websocket_config(workspace: &Path, base_url: &str) {
    std::fs::write(
        workspace.join("config.yaml"),
        format!("runtime:\n  mode: websocket\nservices:\n  service_base_url: {base_url}\n"),
    )
    .unwrap();
}

fn awiki_cmd(args: &[&str], workspace: &Path) -> Output {
    awiki_cmd_owned(
        &args
            .iter()
            .map(|arg| (*arg).to_string())
            .collect::<Vec<_>>(),
        workspace,
    )
}

fn awiki_cmd_owned(args: &[String], workspace: &Path) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_awiki-cli"));
    command
        .args(args)
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace)
        .env("HOME", workspace.join("home"))
        .env("USERPROFILE", workspace.join("home"))
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

fn assert_code(output: &Output, expected: i32) {
    assert_eq!(
        output.status.code(),
        Some(expected),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn request_body(raw: &str) -> &str {
    raw.split("\r\n\r\n").nth(1).unwrap_or_default()
}

fn assert_contains_text(haystack: &str, needle: &str) {
    if let Some((header_name, expected_value)) = needle
        .strip_suffix("\r\n")
        .and_then(|line| line.split_once(':'))
    {
        let header_name = header_name.trim();
        let expected_value = expected_value.trim();
        if haystack.lines().any(|line| {
            line.split_once(':').is_some_and(|(name, value)| {
                name.trim().eq_ignore_ascii_case(header_name) && value.trim() == expected_value
            })
        }) {
            return;
        }
    }
    assert!(
        haystack.contains(needle),
        "expected request to contain {needle:?}, got:\n{haystack}"
    );
}

fn assert_has_group_control_websocket_warning(envelope: &Value) {
    let warnings = envelope["warnings"].as_array().cloned().unwrap_or_default();
    assert!(
        warnings.iter().any(|warning| {
            warning.as_str().unwrap_or_default()
                == "Group lifecycle commands use HTTP transport even when runtime.mode is websocket."
        }),
        "expected group lifecycle websocket warning, got: {warnings:?}"
    );
}

fn json_rpc_result(result: Value) -> String {
    json!({
        "jsonrpc": "2.0",
        "result": result,
        "id": "req-1",
    })
    .to_string()
}

fn json_rpc_error(code: i64, message: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "error": {
            "code": code,
            "message": message,
        },
        "id": "req-1",
    })
    .to_string()
}

#[derive(Debug, Clone)]
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
                .find_map(|line| {
                    line.split_once(':').and_then(|(name, value)| {
                        name.trim()
                            .eq_ignore_ascii_case("content-length")
                            .then_some(value)
                    })
                })
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
    fn new(prefix: &str) -> std::io::Result<Self> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "awiki-cli-rs2-{prefix}-{}-{nanos}",
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
