use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const IDENTITY: &str = "alice-group-e2ee-create";
const AGENT_DID: &str = "did:wba:awiki.ai:alice:e1_alice";
const GROUP_DID: &str = "did:wba:awiki.ai:groups:demo:e1_group";

#[test]
fn group_create_e2ee_live_creates_group_bootstraps_mls_and_publishes_hidden_head() {
    let workspace = TempDir::new("group-e2ee-create-live").expect("workspace");
    register_ready_group_identity(workspace.path(), IDENTITY, "alice", "jwt-alice");
    let bin_dir = TempDir::new("group-e2ee-create-bin").expect("bin dir");
    let fake_mls = bin_dir.path().join("anp-mls");
    let args_log = workspace.path().join("mls-args.log");
    let stdin_log = workspace.path().join("mls-stdin.json");
    write_fake_anp_mls_group_create(&fake_mls, &args_log, &stdin_log, true, "", 0);
    let server = TestServer::new(vec![
        TestResponse::ok(&json_rpc_result(json!({
            "accepted": true,
            "group_did": GROUP_DID,
            "operation_id": "op-create-group",
            "group_event_seq": 1,
            "group_state_version": "v1",
            "created_at": "2026-05-16T01:02:03Z",
            "source": "remote_http"
        }))),
        TestResponse::ok(&json_rpc_result(group_snapshot())),
        TestResponse::ok(&json_rpc_result(json!({
            "members": [
                {
                    "member_did": AGENT_DID,
                    "member_handle": "alice.awiki.ai",
                    "role": "owner",
                    "status": "active",
                    "joined_at": "2026-05-16T01:02:03Z"
                }
            ],
            "total": 1,
            "source": "remote_http"
        }))),
        TestResponse::ok(&json_rpc_result(json!({
            "accepted": true,
            "group_did": GROUP_DID,
            "operation_id": "op-e2ee-create",
            "group_event_seq": 2,
            "group_state_version": "v2",
            "source": "remote_http"
        }))),
    ]);
    write_group_config(workspace.path(), &server.base_url());

    let output = awiki_cmd_with_env(
        &[
            "--identity",
            IDENTITY,
            "group",
            "create",
            "--name",
            "Encrypted Group",
            "--description",
            "Group E2EE create contract",
            "--e2ee",
        ],
        workspace.path(),
        &[("AWIKI_ANP_MLS_BINARY", fake_mls.as_path())],
    );

    assert_success(&output);
    assert_provider_called_group_create(&args_log);
    let provider_stdin = provider_stdin_json(&stdin_log);
    assert_eq!(provider_stdin["api_version"], "anp-mls/v1");
    assert_eq!(provider_stdin["agent_did"], AGENT_DID);
    assert_eq!(provider_stdin["device_id"], "default");
    assert_eq!(provider_stdin["params"]["group_did"], GROUP_DID);

    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], format!("Created group {GROUP_DID}"));
    assert!(envelope.get("warnings").is_none());
    assert_eq!(envelope["data"]["delivery"]["group_did"], GROUP_DID);
    assert_eq!(
        envelope["data"]["delivery"]["operation_id"],
        "op-create-group"
    );
    assert_eq!(envelope["data"]["group"]["group_did"], GROUP_DID);
    assert!(envelope["data"]["group"].is_object());
    assert_eq!(envelope["data"]["members"][0]["member_did"], AGENT_DID);
    assert_eq!(envelope["data"]["members"][0]["role"], "owner");
    assert_eq!(
        envelope["data"]["e2ee"]["mls"]["crypto_group_id_b64u"],
        "Y3J5cHRvLWdyb3Vw"
    );
    assert_eq!(envelope["data"]["e2ee"]["mls"]["epoch"], 0);
    assert_eq!(
        envelope["data"]["e2ee"]["delivery"]["operation_id"],
        "op-e2ee-create"
    );

    let requests = server.requests();
    assert_eq!(requests.len(), 4);
    let bodies = request_json_bodies(&requests);
    assert_eq!(bodies[0]["method"], "group.create");
    assert_eq!(
        bodies[0]["params"]["body"]["group_policy"]["message_security_profile"],
        "group-e2ee"
    );
    assert_eq!(
        bodies[0]["params"]["body"]["group_policy"]["bootstrap_security_profile"],
        "group-e2ee"
    );
    assert_eq!(bodies[1]["method"], "group.get");
    assert_eq!(bodies[1]["params"]["body"]["group_did"], GROUP_DID);
    assert_eq!(bodies[2]["method"], "group.list_members");
    assert_eq!(bodies[2]["params"]["body"]["group_did"], GROUP_DID);
    assert_eq!(bodies[3]["method"], "group.e2ee.create");
    assert_eq!(bodies[3]["params"]["meta"]["profile"], "anp.group.e2ee.v1");
    assert_eq!(
        bodies[3]["params"]["meta"]["security_profile"],
        "group-e2ee"
    );
    assert_eq!(
        bodies[3]["params"]["meta"]["target"],
        json!({"kind": "service", "did": "did:wba:awiki.ai:service:e1_message"})
    );
    assert_eq!(bodies[3]["params"]["body"]["group_did"], GROUP_DID);
    assert_eq!(
        bodies[3]["params"]["body"]["crypto_group_id_b64u"],
        "Y3J5cHRvLWdyb3Vw"
    );
    assert_eq!(bodies[3]["params"]["body"]["epoch"], 0);
    assert_eq!(
        bodies[3]["params"]["body"]["group_state_ref"]["group_did"],
        GROUP_DID
    );
    assert_eq!(
        bodies[3]["params"]["auth"]["scheme"],
        "anp-rfc9421-origin-proof-v1"
    );
}

#[test]
fn group_create_e2ee_live_downgrades_provider_failure_to_warning_without_e2ee_data() {
    let workspace = TempDir::new("group-e2ee-create-warning").expect("workspace");
    register_ready_group_identity(workspace.path(), IDENTITY, "alice", "jwt-alice");
    let bin_dir = TempDir::new("group-e2ee-create-warning-bin").expect("bin dir");
    let fake_mls = bin_dir.path().join("anp-mls");
    let args_log = workspace.path().join("mls-args.log");
    let stdin_log = workspace.path().join("mls-stdin.json");
    write_fake_anp_mls_group_create(
        &fake_mls,
        &args_log,
        &stdin_log,
        false,
        "simulated create failure",
        1,
    );
    let server = TestServer::new(vec![
        TestResponse::ok(&json_rpc_result(json!({
            "accepted": true,
            "group_did": GROUP_DID,
            "operation_id": "op-create-group",
            "group_event_seq": 1,
            "group_state_version": "v1",
            "created_at": "2026-05-16T01:02:03Z",
            "source": "remote_http"
        }))),
        TestResponse::ok(&json_rpc_result(group_snapshot())),
        TestResponse::ok(&json_rpc_result(json!({
            "members": [
                {
                    "member_did": AGENT_DID,
                    "member_handle": "alice.awiki.ai",
                    "role": "owner",
                    "status": "active"
                }
            ],
            "total": 1,
            "source": "remote_http"
        }))),
    ]);
    write_group_config(workspace.path(), &server.base_url());

    let output = awiki_cmd_with_env(
        &[
            "--identity",
            IDENTITY,
            "group",
            "create",
            "--name",
            "Encrypted Group",
            "--e2ee",
        ],
        workspace.path(),
        &[("AWIKI_ANP_MLS_BINARY", fake_mls.as_path())],
    );

    assert_success(&output);
    assert_provider_called_group_create(&args_log);
    let provider_stdin = provider_stdin_json(&stdin_log);
    assert_eq!(provider_stdin["params"]["group_did"], GROUP_DID);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], format!("Created group {GROUP_DID}"));
    assert_eq!(envelope["data"]["delivery"]["group_did"], GROUP_DID);
    assert_eq!(envelope["data"]["group"]["group_did"], GROUP_DID);
    assert_eq!(envelope["data"]["members"][0]["member_did"], AGENT_DID);
    assert!(envelope["data"].get("e2ee").is_none());
    let warnings = envelope["warnings"].as_array().expect("warnings array");
    assert_eq!(warnings.len(), 1);
    assert!(
        warnings[0]
            .as_str()
            .expect("warning")
            .starts_with("Group E2EE MLS create failed:"),
        "unexpected warning: {warnings:?}"
    );

    let requests = server.requests();
    assert_eq!(requests.len(), 3);
    let bodies = request_json_bodies(&requests);
    assert_eq!(bodies[0]["method"], "group.create");
    assert_eq!(bodies[1]["method"], "group.get");
    assert_eq!(bodies[2]["method"], "group.list_members");
}

fn group_snapshot() -> Value {
    json!({
        "group_did": GROUP_DID,
        "group_state_version": "v1",
        "group_event_seq": 1,
        "group_profile": {
            "display_name": "Encrypted Group",
            "description": "Group E2EE create contract",
            "slug": "encrypted-group"
        },
        "group_policy": {
            "message_security_profile": "group-e2ee",
            "bootstrap_security_profile": "group-e2ee",
            "admission_mode": "open-join"
        },
        "member_role": "owner",
        "member_status": "active",
        "member_count": 1,
        "source": "remote_http"
    })
}

fn register_ready_group_identity(
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
    identity["did"] = json!(did);
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
    rewrite_did_document_ids(&mut document, &original_did, &did);
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
        format!(
            "runtime:\n  mode: http\nservices:\n  service_base_url: {base_url}\n  anp_service_did: did:wba:awiki.ai:service:e1_message\n"
        ),
    )
    .unwrap();
}

fn write_fake_anp_mls_group_create(
    path: &Path,
    args_log: &Path,
    stdin_log: &Path,
    ok: bool,
    error_message: &str,
    exit_code: i32,
) {
    let response = if ok {
        json!({
            "ok": true,
            "api_version": "anp-mls/v1",
            "request_id": "group-e2ee-create-test",
            "result": {
                "crypto_group_id_b64u": "Y3J5cHRvLWdyb3Vw",
                "epoch": 0,
                "epoch_authenticator_b64u": "ZXBvY2gtYXV0aA",
                "suite": "MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519",
                "last_handshake_digest": "aGFuZHNoYWtl",
                "group_state_ref": {
                    "group_did": GROUP_DID,
                    "group_state_version": "v1",
                    "group_event_seq": 1
                }
            }
        })
    } else {
        json!({
            "ok": false,
            "api_version": "anp-mls/v1",
            "request_id": "group-e2ee-create-test",
            "error": {
                "code": "create-failed",
                "message": error_message
            }
        })
    }
    .to_string();
    let wrong_command = json!({
        "ok": false,
        "api_version": "anp-mls/v1",
        "request_id": "group-e2ee-create-test",
        "error": {
            "code": "wrong-command",
            "message": "expected group create"
        }
    })
    .to_string();
    let script = format!(
        r#"#!/bin/sh
printf '%s\n' "$@" > {args_log}
body=$(cat)
printf '%s\n' "$body" > {stdin_log}
if [ "$1" != "group" ] || [ "$2" != "create" ]; then
  printf '%s\n' {wrong_command}
  exit 2
fi
printf '%s\n' {response}
exit {exit_code}
"#,
        args_log = shell_quote_path(args_log),
        stdin_log = shell_quote_path(stdin_log),
        wrong_command = shell_quote(&wrong_command),
        response = shell_quote(&response),
        exit_code = exit_code,
    );
    std::fs::write(path, script).expect("write fake anp-mls");
    make_executable(path);
}

fn make_executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
            .expect("chmod fake anp-mls");
    }
}

fn shell_quote_path(path: &Path) -> String {
    shell_quote(&path.to_string_lossy())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn awiki_cmd(args: &[&str], workspace: &Path) -> Output {
    awiki_cmd_with_env(args, workspace, &[])
}

fn awiki_cmd_with_env(args: &[&str], workspace: &Path, envs: &[(&str, &Path)]) -> Output {
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
        .env_remove("AWIKI_ANP_MLS_BINARY");
    for (key, value) in envs {
        command.env(key, value);
    }
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

fn assert_provider_called_group_create(args_log: &Path) {
    let args = std::fs::read_to_string(args_log).expect("read fake anp-mls args");
    let lines = args.lines().collect::<Vec<_>>();
    assert!(
        lines.len() >= 2,
        "expected fake anp-mls args to include domain/action, got:\n{args}"
    );
    assert_eq!(lines[0], "group");
    assert_eq!(lines[1], "create");
    assert!(
        lines.windows(2).any(|window| window == ["--json-in", "-"]),
        "expected fake anp-mls args to include --json-in -, got:\n{args}"
    );
}

fn provider_stdin_json(stdin_log: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(stdin_log).expect("read fake anp-mls stdin"))
        .expect("fake anp-mls stdin should be JSON")
}

fn rewrite_did_document_ids(document: &mut Value, old_did: &str, new_did: &str) {
    if let Some(text) = document.as_str() {
        *document = Value::String(text.replace(old_did, new_did));
        return;
    }
    if let Some(array) = document.as_array_mut() {
        for value in array {
            rewrite_did_document_ids(value, old_did, new_did);
        }
        return;
    }
    if let Some(object) = document.as_object_mut() {
        for value in object.values_mut() {
            rewrite_did_document_ids(value, old_did, new_did);
        }
    }
}

fn request_json_bodies(requests: &[String]) -> Vec<Value> {
    requests
        .iter()
        .map(|request| serde_json::from_str(request_body(request)).expect("json body"))
        .collect()
}

fn request_body(raw: &str) -> &str {
    raw.split("\r\n\r\n").nth(1).unwrap_or_default()
}

fn json_rpc_result(result: Value) -> String {
    json!({
        "jsonrpc": "2.0",
        "result": result,
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
