use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const IDENTITY: &str = "alice-group-e2ee-send";
const AGENT_DID: &str = "did:wba:awiki.ai:alice:e1_alice";
const GROUP_DID: &str = "did:wba:awiki.ai:groups:send:e1_group";

#[test]
fn msg_send_group_secure_on_encrypts_and_posts_hidden_group_e2ee_send_like_go() {
    let workspace = TempDir::new("group-e2ee-send-live").expect("workspace");
    register_ready_group_identity(workspace.path(), IDENTITY, "alice", "jwt-alice");
    seed_cached_e2ee_group_snapshot(workspace.path());

    let bin_dir = TempDir::new("group-e2ee-send-bin").expect("bin dir");
    let fake_mls = bin_dir.path().join("anp-mls");
    let args_log = workspace.path().join("mls-args.log");
    let stdin_log = workspace.path().join("mls-stdin.jsonl");
    write_fake_anp_mls_group_send(&fake_mls, &args_log, &stdin_log);

    let server = TestServer::new(vec![
        TestResponse::ok(&json_rpc_result(group_snapshot())),
        TestResponse::ok(&json_rpc_result(json!({
            "accepted": true,
            "final_acceptance": true,
            "group_event_seq": "44",
            "group_state_version": "v44",
            "accepted_at": "2026-05-16T01:02:03Z",
            "source": "remote_http"
        }))),
    ]);
    write_group_config(workspace.path(), &server.base_url());

    let output = awiki_cmd_with_env(
        &[
            "--identity",
            IDENTITY,
            "msg",
            "send",
            "--group",
            GROUP_DID,
            "--secure",
            "on",
            "--text",
            "hello group",
        ],
        workspace.path(),
        &[("AWIKI_ANP_MLS_BINARY", fake_mls.as_path())],
    );

    assert_success(&output);
    assert_eq!(
        provider_commands(&args_log),
        vec!["group status", "message encrypt"]
    );
    let provider_stdin = provider_stdin_jsonl(&stdin_log);
    assert_eq!(provider_stdin.len(), 2);
    assert_eq!(provider_stdin[0]["api_version"], "anp-mls/v1");
    assert_eq!(provider_stdin[0]["agent_did"], AGENT_DID);
    assert_eq!(provider_stdin[0]["device_id"], "default");
    assert_eq!(provider_stdin[0]["params"]["group_did"], GROUP_DID);
    let encrypt_request = &provider_stdin[1];
    assert_eq!(encrypt_request["api_version"], "anp-mls/v1");
    assert_eq!(encrypt_request["agent_did"], AGENT_DID);
    assert_eq!(encrypt_request["device_id"], "default");
    assert_eq!(encrypt_request["params"]["agent_did"], AGENT_DID);
    assert_eq!(encrypt_request["params"]["device_id"], "default");
    assert_eq!(encrypt_request["params"]["group_did"], GROUP_DID);
    assert_eq!(
        encrypt_request["params"]["group_state_ref"]["group_did"],
        GROUP_DID
    );
    assert_eq!(encrypt_request["params"]["sender_did"], AGENT_DID);
    assert_eq!(
        encrypt_request["params"]["content_type"],
        "application/anp-group-cipher+json"
    );
    assert_eq!(encrypt_request["params"]["security_profile"], "group-e2ee");
    assert_eq!(encrypt_request["params"]["message_type"], "text");
    assert_eq!(
        encrypt_request["params"]["application_plaintext"]["application_content_type"],
        "text/plain"
    );
    assert_eq!(
        encrypt_request["params"]["application_plaintext"]["text"],
        "hello group"
    );
    let generated_operation_id = encrypt_request["params"]["operation_id"]
        .as_str()
        .expect("operation id");
    let generated_message_id = encrypt_request["params"]["message_id"]
        .as_str()
        .expect("message id");
    assert!(generated_operation_id.starts_with("op-"));
    assert!(generated_message_id.starts_with("msg-"));

    let bodies = request_json_bodies(&server.requests());
    assert_eq!(rpc_methods(&bodies), vec!["group.get", "group.e2ee.send"]);
    assert_eq!(bodies[0]["params"]["body"]["group_did"], GROUP_DID);
    assert_eq!(bodies[1]["params"]["meta"]["profile"], "anp.group.e2ee.v1");
    assert_eq!(
        bodies[1]["params"]["meta"]["security_profile"],
        "group-e2ee"
    );
    assert_eq!(
        bodies[1]["params"]["meta"]["content_type"],
        "application/anp-group-cipher+json"
    );
    assert_eq!(
        bodies[1]["params"]["meta"]["target"],
        json!({"kind": "group", "did": GROUP_DID})
    );
    assert_eq!(
        bodies[1]["params"]["meta"]["operation_id"],
        generated_operation_id
    );
    assert_eq!(
        bodies[1]["params"]["meta"]["message_id"],
        generated_message_id
    );
    assert_eq!(
        bodies[1]["params"]["auth"]["scheme"],
        "anp-rfc9421-origin-proof-v1"
    );
    let cipher_body = bodies[1]["params"]["body"]
        .as_object()
        .expect("cipher body");
    assert_eq!(cipher_body.len(), 5);
    assert_eq!(cipher_body["crypto_group_id_b64u"], "Y3J5cHRvLWdyb3Vw");
    assert_eq!(cipher_body["epoch"], 44);
    assert_eq!(cipher_body["private_message_b64u"], "c2VjcmV0LWJvZHk");
    assert_eq!(cipher_body["group_state_ref"]["group_did"], GROUP_DID);
    assert_eq!(cipher_body["epoch_authenticator"], "YXV0aC00NA");
    assert!(
        !cipher_body.contains_key("application_plaintext"),
        "hidden group E2EE send must post only the opaque cipher object"
    );
    assert!(
        !cipher_body.contains_key("debug_plaintext"),
        "hidden group E2EE send must sanitize unknown cipher fields"
    );

    let envelope = success_json(&output);
    assert_eq!(
        envelope["summary"],
        "Sent a group text message with group E2EE"
    );
    assert!(envelope.get("warnings").is_none());
    assert_eq!(envelope["data"]["action"], "send_message");
    assert_eq!(envelope["data"]["target"]["kind"], "group");
    assert_eq!(envelope["data"]["target"]["did"], GROUP_DID);
    assert_eq!(envelope["data"]["message"]["type"], "text");
    assert_eq!(envelope["data"]["message"]["secure"], true);
    assert_eq!(
        envelope["data"]["message"]["security_profile"],
        "group-e2ee"
    );
    assert_eq!(envelope["data"]["message"]["id"], format!("{GROUP_DID}:44"));
    assert_eq!(
        envelope["data"]["message"]["sent_at"],
        "2026-05-16T01:02:03Z"
    );
    assert_eq!(envelope["data"]["delivery"]["group_did"], GROUP_DID);
    assert_eq!(
        envelope["data"]["delivery"]["message_id"],
        generated_message_id
    );
    assert_eq!(
        envelope["data"]["delivery"]["operation_id"],
        generated_operation_id
    );
    assert_eq!(envelope["data"]["e2ee"]["encrypted"], true);
    assert_eq!(
        envelope["data"]["e2ee"]["group_state_ref"]["group_did"],
        GROUP_DID
    );
    assert_eq!(envelope["data"]["e2ee"]["cipher_object_sent"], true);
}

#[test]
fn msg_send_group_e2ee_websocket_mode_stays_http_only_like_go() {
    let workspace = TempDir::new("group-e2ee-send-ws-http").expect("workspace");
    register_ready_group_identity(workspace.path(), IDENTITY, "alice", "jwt-alice");
    seed_cached_e2ee_group_snapshot(workspace.path());

    let bin_dir = TempDir::new("group-e2ee-send-ws-bin").expect("bin dir");
    let fake_mls = bin_dir.path().join("anp-mls");
    let args_log = workspace.path().join("mls-args-ws.log");
    let stdin_log = workspace.path().join("mls-stdin-ws.jsonl");
    write_fake_anp_mls_group_send(&fake_mls, &args_log, &stdin_log);

    let server = TestServer::new(vec![
        TestResponse::ok(&json_rpc_result(group_snapshot())),
        TestResponse::ok(&json_rpc_result(json!({
            "accepted": true,
            "final_acceptance": true,
            "group_did": GROUP_DID,
            "message_id": "msg-e2ee-ws-http",
            "operation_id": "op-e2ee-ws-http",
            "group_event_seq": "45",
            "group_state_version": "v45",
            "accepted_at": "2026-05-16T06:07:08Z",
            "source": "remote_http"
        }))),
    ]);
    write_group_websocket_config(workspace.path(), &server.base_url());

    let output = awiki_cmd_with_env(
        &[
            "--identity",
            IDENTITY,
            "msg",
            "send",
            "--group",
            GROUP_DID,
            "--secure",
            "on",
            "--text",
            "hello e2ee in websocket mode",
        ],
        workspace.path(),
        &[("AWIKI_ANP_MLS_BINARY", fake_mls.as_path())],
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(
        envelope["summary"],
        "Sent a group text message with group E2EE"
    );
    assert!(envelope.get("warnings").is_none());
    assert_eq!(envelope["data"]["message"]["secure"], true);
    assert_eq!(
        envelope["data"]["message"]["security_profile"],
        "group-e2ee"
    );
    assert_eq!(envelope["data"]["source"], "remote_http");
    assert_eq!(envelope["data"]["e2ee"]["encrypted"], true);

    let provider_stdin = provider_stdin_jsonl(&stdin_log);
    assert_eq!(provider_stdin.len(), 2);
    assert_eq!(
        provider_stdin[1]["params"]["application_plaintext"]["text"],
        "hello e2ee in websocket mode"
    );

    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].starts_with("POST /im/rpc HTTP/1.1"));
    assert!(requests[1].starts_with("POST /im/rpc HTTP/1.1"));
    let bodies = request_json_bodies(&requests);
    assert_eq!(rpc_methods(&bodies), vec!["group.get", "group.e2ee.send"]);
    let body = &bodies[1];
    assert_eq!(body["params"]["meta"]["profile"], "anp.group.e2ee.v1");
    assert_eq!(body["params"]["meta"]["security_profile"], "group-e2ee");
    assert_eq!(
        body["params"]["meta"]["content_type"],
        "application/anp-group-cipher+json"
    );
    assert_eq!(
        body["params"]["meta"]["target"],
        json!({"kind": "group", "did": GROUP_DID})
    );
    assert!(
        body["params"]["body"]
            .get("application_plaintext")
            .is_none(),
        "hidden group E2EE send must not leak plaintext through HTTP"
    );
}

fn seed_cached_e2ee_group_snapshot(workspace: &Path) {
    let server = TestServer::new(vec![TestResponse::ok(&json_rpc_result(group_snapshot()))]);
    write_group_config(workspace, &server.base_url());
    let output = awiki_cmd(
        &["--identity", IDENTITY, "group", "get", "--group", GROUP_DID],
        workspace,
    );
    assert_success(&output);
    let bodies = request_json_bodies(&server.requests());
    assert_eq!(rpc_methods(&bodies), vec!["group.get"]);
}

fn group_snapshot() -> Value {
    json!({
        "group_did": GROUP_DID,
        "group_state_version": "v43",
        "group_event_seq": 43,
        "group_profile": {
            "display_name": "Encrypted Send Group",
            "description": "Group E2EE send contract",
            "slug": "encrypted-send-group"
        },
        "group_policy": {
            "message_security_profile": "group-e2ee",
            "bootstrap_security_profile": "group-e2ee",
            "admission_mode": "closed"
        },
        "member_role": "member",
        "member_status": "active",
        "member_count": 2,
        "source": "remote_http"
    })
}

fn write_fake_anp_mls_group_send(path: &Path, args_log: &Path, stdin_log: &Path) {
    let status_response = json!({
        "ok": true,
        "api_version": "anp-mls/v1",
        "request_id": "group-e2ee-send-status-test",
        "result": {
            "status": "active",
            "epoch": "43",
            "crypto_group_id_b64u": "Y3J5cHRvLWdyb3Vw"
        }
    })
    .to_string();
    let encrypt_response = json!({
        "ok": true,
        "api_version": "anp-mls/v1",
        "request_id": "group-e2ee-encrypt-test",
        "result": {
            "group_cipher_object": {
                "crypto_group_id_b64u": "Y3J5cHRvLWdyb3Vw",
                "epoch": 44,
                "private_message_b64u": "c2VjcmV0LWJvZHk",
                "group_state_ref": {
                    "group_did": GROUP_DID,
                    "group_state_version": "v44",
                    "group_event_seq": 44
                },
                "epoch_authenticator": "YXV0aC00NA",
                "application_plaintext": {
                    "text": "must-not-leak"
                },
                "debug_plaintext": "must-not-leak"
            }
        }
    })
    .to_string();
    let wrong_command = json!({
        "ok": false,
        "api_version": "anp-mls/v1",
        "request_id": "group-e2ee-send-test",
        "error": {
            "code": "wrong-command",
            "message": "expected group status or message encrypt"
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
if [ "$1" = "message" ] && [ "$2" = "encrypt" ]; then
  printf '%s\n' {encrypt_response}
  exit 0
fi
printf '%s\n' {wrong_command}
exit 2
"#,
        args_log = shell_quote_path(args_log),
        stdin_log = shell_quote_path(stdin_log),
        status_response = shell_quote(&status_response),
        encrypt_response = shell_quote(&encrypt_response),
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
