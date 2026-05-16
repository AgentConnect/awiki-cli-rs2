use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const IDENTITY: &str = "alice-group-e2ee";
const GROUP_DID: &str = "did:wba:awiki.ai:groups:demo:e1_group";

#[test]
fn group_e2ee_publish_live_uses_configured_service_did_then_publishes_normal_key_package() {
    let workspace = TempDir::new("group-e2ee-publish-normal").expect("workspace");
    register_ready_group_identity(workspace.path(), IDENTITY, "alice", "jwt-alice");
    let bin_dir = TempDir::new("group-e2ee-publish-bin").expect("bin dir");
    let fake_mls = bin_dir.path().join("anp-mls");
    let args_log = workspace.path().join("mls-args.log");
    let stdin_log = workspace.path().join("mls-stdin.json");
    write_fake_anp_mls_key_package(
        &fake_mls, &args_log, &stdin_log, "normal", "", "default", false,
    );
    let server = TestServer::new(vec![TestResponse::ok(&json_rpc_result(json!({
            "published": true,
            "key_package_id": "kp-normal-default"
    })))]);
    write_group_config_without_service_did(workspace.path(), &server.base_url());

    let output = awiki_cmd_with_env(
        &[
            "--identity",
            IDENTITY,
            "group",
            "e2ee",
            "publish-key-package",
        ],
        workspace.path(),
        &[("AWIKI_ANP_MLS_BINARY", fake_mls.as_path())],
    );

    assert_success(&output);
    assert_provider_called_key_package_generate(&args_log);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Published group E2EE KeyPackage");
    let data = &envelope["data"];
    assert_eq!(data["purpose"], "normal");
    assert_eq!(data["recovery"], false);
    assert_eq!(data["group"], "");
    assert_eq!(data["device_id"], "default");
    assert_eq!(data["argv_safe"], true);
    assert_eq!(data["p4_mutates"], false);
    assert_eq!(data["plan"]["action"], "group.e2ee.publish_key_package");
    assert_binding_has_proof(&data["mls"]["group_key_package"]["did_wba_binding"]);

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    let bodies = request_json_bodies(&requests);
    assert_eq!(bodies[0]["method"], "group.e2ee.publish_key_package");
    assert_publish_body_does_not_leak_private_key_package(&bodies[0]);
}

#[test]
fn group_e2ee_publish_live_tags_recovery_key_package_with_group_device_and_contract_test() {
    let workspace = TempDir::new("group-e2ee-publish-recovery").expect("workspace");
    register_ready_group_identity(workspace.path(), IDENTITY, "alice", "jwt-alice");
    let bin_dir = TempDir::new("group-e2ee-publish-recovery-bin").expect("bin dir");
    let fake_mls = bin_dir.path().join("anp-mls");
    let args_log = workspace.path().join("mls-args.log");
    let stdin_log = workspace.path().join("mls-stdin.json");
    write_fake_anp_mls_key_package(
        &fake_mls, &args_log, &stdin_log, "recovery", GROUP_DID, "bob-main", true,
    );
    let server = TestServer::new(vec![TestResponse::ok(&json_rpc_result(json!({
            "published": true,
            "key_package_id": "kp-recovery-bob-main"
    })))]);
    write_group_config_without_service_did(workspace.path(), &server.base_url());

    let output = awiki_cmd_with_env(
        &[
            "--identity",
            IDENTITY,
            "group",
            "e2ee",
            "publish-key-package",
            "--recovery",
            "--group",
            GROUP_DID,
            "--device",
            "bob-main",
            "--contract-test",
        ],
        workspace.path(),
        &[("AWIKI_ANP_MLS_BINARY", fake_mls.as_path())],
    );

    assert_success(&output);
    assert_provider_called_key_package_generate(&args_log);
    let provider_stdin = provider_stdin_json(&stdin_log);
    assert_eq!(provider_stdin["contract_test_enabled"], true);
    assert_eq!(provider_stdin["params"]["purpose"], "recovery");
    assert_eq!(provider_stdin["params"]["group_did"], GROUP_DID);
    assert_eq!(provider_stdin["params"]["device_id"], "bob-main");

    let envelope = success_json(&output);
    let data = &envelope["data"];
    assert_eq!(data["purpose"], "recovery");
    assert_eq!(data["recovery"], true);
    assert_eq!(data["group"], GROUP_DID);

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    let bodies = request_json_bodies(&requests);
    assert_eq!(bodies[0]["method"], "group.e2ee.publish_key_package");
    let package = &bodies[0]["params"]["body"]["group_key_package"];
    assert_eq!(package["purpose"], "recovery");
    assert_eq!(package["group_did"], GROUP_DID);
    assert_eq!(package["device_id"], "bob-main");
    assert_publish_body_does_not_leak_private_key_package(&bodies[0]);
}

#[test]
fn group_e2ee_publish_recovery_requires_group_did() {
    let workspace = TempDir::new("group-e2ee-publish-missing-group").expect("workspace");
    register_ready_group_identity(workspace.path(), IDENTITY, "alice", "jwt-alice");
    write_group_config_without_service_did(workspace.path(), "http://127.0.0.1:9");

    let output = awiki_cmd(
        &[
            "--identity",
            IDENTITY,
            "group",
            "e2ee",
            "publish-key-package",
            "--recovery",
        ],
        workspace.path(),
    );

    assert_failure(&output);
    assert_output_contains(
        &output,
        "group DID is required when publishing a recovery KeyPackage",
    );
}

#[test]
fn group_e2ee_publish_rejects_invalid_key_package_purpose() {
    let workspace = TempDir::new("group-e2ee-publish-invalid-purpose").expect("workspace");
    register_ready_group_identity(workspace.path(), IDENTITY, "alice", "jwt-alice");
    write_group_config_without_service_did(workspace.path(), "http://127.0.0.1:9");

    let output = awiki_cmd(
        &[
            "--identity",
            IDENTITY,
            "group",
            "e2ee",
            "publish-key-package",
            "--purpose",
            "archive",
        ],
        workspace.path(),
    );

    assert_failure(&output);
    assert_output_contains(
        &output,
        "group E2EE KeyPackage purpose must be normal, recovery, or update",
    );
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

fn write_group_config_without_service_did(workspace: &Path, base_url: &str) {
    std::fs::write(
        workspace.join("config.yaml"),
        format!("runtime:\n  mode: http\nservices:\n  service_base_url: {base_url}\n"),
    )
    .unwrap();
}

fn write_fake_anp_mls_key_package(
    path: &Path,
    args_log: &Path,
    stdin_log: &Path,
    purpose: &str,
    group_did: &str,
    device_id: &str,
    contract_test: bool,
) {
    let owner_did = "did:wba:awiki.ai:alice:e1_alice";
    let mut group_key_package = json!({
        "owner_did": owner_did,
        "device_id": device_id,
        "key_package_id": format!("kp-{purpose}-{device_id}"),
        "suite": "MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519",
        "mls_key_package_b64u": "bWxzLWtleS1wYWNrYWdl",
        "private_key_package_b64u": "secret-private-key-package",
        "did_wba_binding": {
            "agent_did": owner_did,
            "device_id": device_id,
            "leaf_signature_key_b64u": "bGVhZg",
            "issued_at": "2026-01-01T00:00:00Z",
            "expires_at": "2030-01-01T00:00:00Z",
            "proof": {
                "type": "DataIntegrityProof",
                "cryptosuite": "eddsa-jcs-2022",
                "created": "2026-05-16T00:00:00Z",
                "proofValue": format!("zproof-{purpose}-{device_id}")
            },
            "signature": format!("sig-{purpose}-{device_id}"),
            "signature_b64u": "c2lnbmF0dXJl"
        },
        "expires_at": "2030-01-01T00:00:00Z",
        "purpose": purpose,
        "non_cryptographic": contract_test,
        "artifact_mode": if contract_test { "contract-test" } else { "live" }
    });
    if !group_did.trim().is_empty() {
        group_key_package
            .as_object_mut()
            .unwrap()
            .insert("group_did".to_string(), json!(group_did));
    }
    let response = json!({
        "ok": true,
        "api_version": "anp-mls/v1",
        "request_id": "group-e2ee-publish-test",
        "result": {
            "purpose": purpose,
            "recovery": purpose == "recovery",
            "group": group_did,
            "device_id": device_id,
            "argv_safe": true,
            "p4_mutates": false,
            "group_key_package": group_key_package
        }
    })
    .to_string();
    let wrong_command = json!({
        "ok": false,
        "api_version": "anp-mls/v1",
        "request_id": "group-e2ee-publish-test",
        "error": {
            "code": "wrong-command",
            "message": "expected key-package generate"
        }
    })
    .to_string();
    let script = format!(
        r#"#!/bin/sh
printf '%s\n' "$@" > {args_log}
body=$(cat)
printf '%s\n' "$body" > {stdin_log}
if [ "$1" != "key-package" ] || [ "$2" != "generate" ]; then
  printf '%s\n' {wrong_command}
  exit 2
fi
printf '%s\n' {response}
"#,
        args_log = shell_quote_path(args_log),
        stdin_log = shell_quote_path(stdin_log),
        wrong_command = shell_quote(&wrong_command),
        response = shell_quote(&response),
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

fn assert_failure(output: &Output) {
    assert_ne!(
        output.status.code(),
        Some(0),
        "expected failure; stdout:\n{}\nstderr:\n{}",
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

fn assert_output_contains(output: &Output, needle: &str) {
    let mut text = String::new();
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    assert!(
        text.contains(needle),
        "expected output to contain {needle:?}, got:\n{text}"
    );
}

fn assert_provider_called_key_package_generate(args_log: &Path) {
    let args = std::fs::read_to_string(args_log).expect("read fake anp-mls args");
    let lines = args.lines().collect::<Vec<_>>();
    assert!(
        lines.len() >= 2,
        "expected fake anp-mls args to include domain/action, got:\n{args}"
    );
    assert_eq!(lines[0], "key-package");
    assert_eq!(lines[1], "generate");
    assert!(
        lines.windows(2).any(|window| window == ["--json-in", "-"]),
        "expected fake anp-mls args to include --json-in -, got:\n{args}"
    );
}

fn provider_stdin_json(stdin_log: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(stdin_log).expect("read fake anp-mls stdin"))
        .expect("fake anp-mls stdin should be JSON")
}

fn assert_binding_has_proof(binding: &Value) {
    let binding = binding.as_object().expect("did_wba_binding object");
    assert_eq!(
        binding.get("agent_did").and_then(Value::as_str),
        Some("did:wba:awiki.ai:alice:e1_alice")
    );
    assert_eq!(
        binding
            .get("leaf_signature_key_b64u")
            .and_then(Value::as_str),
        Some("bGVhZg")
    );
    let proof = binding
        .get("proof")
        .and_then(Value::as_object)
        .expect("did_wba_binding proof object");
    assert!(
        proof
            .get("proofValue")
            .and_then(Value::as_str)
            .is_some_and(|value| !value.is_empty()),
        "did_wba_binding proof should include proofValue"
    );
    assert_eq!(
        proof
            .get("verificationMethod")
            .and_then(Value::as_str)
            .map(|value| value.starts_with("did:wba:awiki.ai:alice:e1_alice#")),
        Some(true)
    );
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

fn assert_publish_body_does_not_leak_private_key_package(body: &Value) {
    let params_text = body["params"].to_string();
    assert!(
        !params_text.contains("private_key_package_b64u"),
        "publish params leaked private_key_package_b64u: {params_text}"
    );
    assert!(
        !params_text.contains("secret-private-key-package"),
        "publish params leaked private KeyPackage material: {params_text}"
    );
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
