use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const IDENTITY: &str = "alice-group-e2ee-repair";
const AGENT_DID: &str = "did:wba:awiki.ai:alice:e1_alice";
const GROUP_DID: &str = "did:wba:awiki.ai:groups:repair:e1_group";

#[test]
fn group_e2ee_repair_live_replays_commit_notice_marks_delivered_and_reports_diagnosis() {
    let workspace = TempDir::new("group-e2ee-repair-live").expect("workspace");
    register_ready_group_identity(workspace.path(), IDENTITY, "alice", "jwt-alice");
    let bin_dir = TempDir::new("group-e2ee-repair-bin").expect("bin dir");
    let fake_mls = bin_dir.path().join("anp-mls");
    let args_log = workspace.path().join("mls-args.log");
    let stdin_log = workspace.path().join("mls-stdin.jsonl");
    write_fake_anp_mls_group_repair(&fake_mls, &args_log, &stdin_log);
    let server = TestServer::new(vec![
        TestResponse::ok(&json_rpc_result(json!({
            "group_did": GROUP_DID,
            "epoch": "2",
            "group_state_version": "v2",
            "crypto_group_id_b64u": "crypto-1",
            "actor_membership_status": "active",
            "actor_membership_role": "member"
        }))),
        TestResponse::ok(&json_rpc_result(json!({
            "notices": [
                {
                    "notice_id": "notice-commit-1",
                    "notice_type": "commit-delivery",
                    "group_did": GROUP_DID,
                    "recipient_did": AGENT_DID,
                    "subject_did": "did:wba:awiki.ai:carol:e1_carol",
                    "subject_status": "removed",
                    "commit_b64u": "Y29tbWl0LTE",
                    "ratchet_tree_b64u": "cmF0Y2hldC0x",
                    "group_info_b64u": "Z3JvdXAtaW5mby0x",
                    "operation_id": "op-notice-commit",
                    "crypto_group_id_b64u": "crypto-1",
                    "epoch_authenticator_b64u": "YXV0aC0y",
                    "from_epoch": "1",
                    "to_epoch": "2",
                    "delivery_state": "pending"
                }
            ],
            "pending_count": 1,
            "delivered": 0
        }))),
        TestResponse::ok(&json_rpc_result(json!({
            "notices": [],
            "pending_count": 0,
            "delivered": 1,
            "notice_ids": ["notice-commit-1"]
        }))),
    ]);
    write_group_config(workspace.path(), &server.base_url());

    let output = awiki_cmd_with_env(
        &[
            "--identity",
            IDENTITY,
            "group",
            "e2ee",
            "repair",
            "--group",
            GROUP_DID,
        ],
        workspace.path(),
        &[("AWIKI_ANP_MLS_BINARY", fake_mls.as_path())],
    );

    assert_success(&output);
    assert_eq!(
        provider_commands(&args_log),
        vec!["group status", "commit process", "group status"]
    );
    let provider_stdin = provider_stdin_jsonl(&stdin_log);
    assert_eq!(provider_stdin.len(), 3);
    assert_eq!(provider_stdin[0]["params"]["group_did"], GROUP_DID);
    assert_eq!(provider_stdin[0]["params"]["device_id"], "default");
    assert_eq!(provider_stdin[1]["api_version"], "anp-mls/v1");
    assert_eq!(provider_stdin[1]["agent_did"], AGENT_DID);
    assert_eq!(provider_stdin[1]["device_id"], "default");
    assert_eq!(provider_stdin[1]["params"]["agent_did"], AGENT_DID);
    assert_eq!(provider_stdin[1]["params"]["device_id"], "default");
    assert_eq!(provider_stdin[1]["params"]["group_did"], GROUP_DID);
    assert_eq!(provider_stdin[1]["params"]["commit_b64u"], "Y29tbWl0LTE");
    assert_eq!(
        provider_stdin[1]["params"]["ratchet_tree_b64u"],
        "cmF0Y2hldC0x"
    );
    assert_eq!(
        provider_stdin[1]["params"]["group_info_b64u"],
        "Z3JvdXAtaW5mby0x"
    );
    assert_eq!(provider_stdin[1]["params"]["notice_id"], "notice-commit-1");
    assert_eq!(
        provider_stdin[1]["params"]["group_state_ref"]["group_did"],
        GROUP_DID
    );
    assert_eq!(provider_stdin[2]["params"]["group_did"], GROUP_DID);

    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Replayed group E2EE pending notices");
    assert!(envelope.get("warnings").is_none());
    assert_eq!(envelope["data"]["group"], GROUP_DID);
    assert_eq!(envelope["data"]["processed_count"], 1);
    assert_eq!(envelope["data"]["processed"][0]["processed"], true);
    assert_eq!(
        envelope["data"]["processed"][0]["notice_type"],
        "commit-delivery"
    );
    assert_eq!(
        envelope["data"]["processed"][0]["notice_id"],
        "notice-commit-1"
    );
    assert_eq!(envelope["data"]["processed"][0]["epoch"], "2");
    assert_eq!(envelope["data"]["finalized_pending_count"], 0);
    assert_eq!(envelope["data"]["pending_count"], 1);
    assert_eq!(envelope["data"]["delivered_result"]["delivered"], 1);
    assert_eq!(envelope["data"]["local"]["status"], "active");
    assert_eq!(envelope["data"]["local"]["epoch"], "2");
    assert_eq!(envelope["data"]["local_device_id"], "default");
    assert_eq!(envelope["data"]["service_head"]["epoch"], "2");
    assert_eq!(envelope["data"]["diagnosis"]["state"], "in_sync");
    assert_eq!(envelope["data"]["diagnosis"]["next_action"], "none");
    assert_eq!(envelope["data"]["plan"]["action"], "group.e2ee.repair");
    assert_eq!(envelope["data"]["plan"]["group"], GROUP_DID);

    let bodies = request_json_bodies(&server.requests());
    assert_eq!(
        rpc_methods(&bodies),
        vec!["group.e2ee.head", "group.e2ee.notice", "group.e2ee.notice"]
    );
    assert_eq!(bodies[0]["params"]["body"]["group_did"], GROUP_DID);
    assert_eq!(bodies[1]["params"]["body"]["group_did"], GROUP_DID);
    assert_eq!(bodies[1]["params"]["body"]["limit"], 50);
    assert!(bodies[1]["params"]["body"]
        .as_object()
        .expect("first notice body")
        .get("mark_delivered")
        .is_none());
    assert_eq!(bodies[2]["params"]["body"]["group_did"], GROUP_DID);
    assert_eq!(bodies[2]["params"]["body"]["limit"], 1);
    assert_eq!(bodies[2]["params"]["body"]["mark_delivered"], true);
    assert_eq!(
        bodies[2]["params"]["body"]["notice_ids"],
        json!(["notice-commit-1"])
    );
}

fn write_fake_anp_mls_group_repair(path: &Path, args_log: &Path, stdin_log: &Path) {
    let status_response = json!({
        "ok": true,
        "api_version": "anp-mls/v1",
        "request_id": "group-e2ee-repair-status-test",
        "result": {
            "status": "active",
            "epoch": "2",
            "crypto_group_id_b64u": "crypto-1",
            "pending_commits": []
        }
    })
    .to_string();
    let commit_response = json!({
        "ok": true,
        "api_version": "anp-mls/v1",
        "request_id": "group-e2ee-commit-repair-test",
        "result": {
            "processed": true,
            "epoch": "2",
            "crypto_group_id_b64u": "crypto-1",
            "epoch_authenticator_b64u": "YXV0aC0y",
            "group_state_ref": {
                "group_did": GROUP_DID,
                "group_state_version": "v2",
                "group_event_seq": 2,
                "epoch": "2"
            }
        }
    })
    .to_string();
    let wrong_command = json!({
        "ok": false,
        "api_version": "anp-mls/v1",
        "request_id": "group-e2ee-repair-test",
        "error": {
            "code": "wrong-command",
            "message": "expected group status or commit process"
        }
    })
    .to_string();
    let script = format!(
        r#"#!/bin/sh
printf '%s %s\n' "$1" "$2" >> {args_log}
body=$(cat)
printf '%s\n' "$body" >> {stdin_log}
if [ "$1" = "group" ] && [ "$2" = "status" ]; then
  printf '%s\n' {status_response}
  exit 0
fi
if [ "$1" = "commit" ] && [ "$2" = "process" ]; then
  printf '%s\n' {commit_response}
  exit 0
fi
printf '%s\n' {wrong_command}
exit 2
"#,
        args_log = shell_quote_path(args_log),
        stdin_log = shell_quote_path(stdin_log),
        status_response = shell_quote(&status_response),
        commit_response = shell_quote(&commit_response),
        wrong_command = shell_quote(&wrong_command),
    );
    std::fs::write(path, script).expect("write fake anp-mls");
    make_executable(path);
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

fn write_group_config(workspace: &Path, base_url: &str) {
    std::fs::write(
        workspace.join("config.yaml"),
        format!("runtime:\n  mode: http\nservices:\n  service_base_url: {base_url}\n"),
    )
    .unwrap();
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

fn provider_commands(args_log: &Path) -> Vec<String> {
    std::fs::read_to_string(args_log)
        .expect("read fake anp-mls args")
        .lines()
        .map(str::to_string)
        .collect()
}

fn provider_stdin_jsonl(stdin_log: &Path) -> Vec<Value> {
    std::fs::read_to_string(stdin_log)
        .expect("read fake anp-mls stdin")
        .lines()
        .map(|line| serde_json::from_str(line).expect("fake anp-mls stdin line should be JSON"))
        .collect()
}

fn request_json_bodies(requests: &[String]) -> Vec<Value> {
    requests
        .iter()
        .map(|request| serde_json::from_str(request_body(request)).expect("json body"))
        .collect()
}

fn rpc_methods(bodies: &[Value]) -> Vec<&str> {
    bodies
        .iter()
        .map(|body| body["method"].as_str().expect("rpc method"))
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
