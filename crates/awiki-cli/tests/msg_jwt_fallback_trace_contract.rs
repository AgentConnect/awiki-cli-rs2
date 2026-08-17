use base64::Engine;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

mod support;

use support::{
    set_secret_storage_mode, tenant_workspace, write_default_tenant_registry, write_tenant_config,
};

#[test]
fn direct_send_http_401_refreshes_inside_im_core_transport() {
    let workspace = TempDir::new("msg-jwt-fallback-send").expect("workspace");
    register_ready_msg_identity(workspace.path(), "alice-msg-fallback", "alice", "jwt-stale");
    let bob_did = "did:wba:awiki.ai:bob:e1_bob";
    let refreshed_token = legacy_access_token("did:wba:awiki.ai:alice:e1_alice");
    let server = TestServer::new(vec![
        TestResponse::ok(&json_rpc_error(1401, "expired jwt")),
        TestResponse::ok(&json_rpc_result(json!({
            "accepted": true,
            "final_acceptance": true,
            "message_id": "msg-fallback-refresh-1",
            "operation_id": "op-fallback-refresh-1",
            "target_did": bob_did,
            "accepted_at": "2026-05-17T01:02:03Z",
            "delivery_state": "accepted"
        })))
        .with_access_token(&refreshed_token),
    ]);
    write_msg_config(workspace.path(), &server.base_url());

    let output = awiki_trace_cmd(
        &[
            "--identity",
            "alice-msg-fallback",
            "msg",
            "send",
            "--to",
            bob_did,
            "--text",
            "hello after fallback refresh",
        ],
        workspace.path(),
    );
    assert_success(&output);
    let envelope = success_json_with_stderr(&output);
    assert_eq!(envelope["summary"], "Sent a direct text message");
    assert_eq!(envelope["data"]["message"]["id"], "msg-fallback-refresh-1");

    let trace = stderr_text(&output);
    assert_text_contains(&trace, "远端 RPC / direct send");
    assert_text_not_contains(&trace, "消息回退时刷新 JWT");

    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].starts_with("POST /im/rpc HTTP/1.1"));
    assert_contains_text(&requests[0], "Authorization: Bearer jwt-stale\r\n");
    assert!(requests[1].starts_with("POST /im/rpc HTTP/1.1"));
    assert_contains_text(&requests[1], "signature-input:");
    assert_eq!(json_body(&requests[1])["method"], "direct.send");

    assert_vault_auth_token_is_used(
        workspace.path(),
        "alice-msg-fallback",
        &refreshed_token,
        &[
            "--identity",
            "alice-msg-fallback",
            "msg",
            "send",
            "--to",
            bob_did,
            "--text",
            "hello with cached refreshed token",
        ],
    );
}

#[test]
fn inbox_http_1401_refreshes_inside_im_core_transport() {
    let workspace = TempDir::new("msg-jwt-fallback-inbox").expect("workspace");
    let server = TestServer::new(vec![
        TestResponse::registration(),
        TestResponse::prekey_publication(),
        TestResponse::ok(&json_rpc_error(1401, "expired inbox jwt")),
        TestResponse::sync_bootstrap().with_dynamic_access_token(),
        TestResponse::sync_delta_empty(),
    ]);
    write_msg_config(workspace.path(), &server.base_url());
    let register = awiki_cmd(
        &[
            "id",
            "register",
            "--handle",
            "bob",
            "--phone",
            "13800138000",
            "--otp",
            "123456",
        ],
        workspace.path(),
    );
    assert_success(&register);

    let output = awiki_trace_cmd_without_direct_e2ee(
        &[
            "--identity",
            "bob",
            "msg",
            "inbox",
            "--scope",
            "direct",
            "--limit",
            "3",
        ],
        workspace.path(),
    );
    assert_success(&output);
    let envelope = success_json_with_stderr(&output);
    assert_eq!(envelope["summary"], "Loaded 0 inbox messages");
    assert_eq!(envelope["data"]["source"], "local_projection");
    assert_eq!(envelope["data"]["messages"], json!([]));
    assert_eq!(envelope["data"]["with"], "");

    let warnings = envelope["warnings"].as_array().cloned().unwrap_or_default();
    assert!(
        warnings.is_empty(),
        "HTTP inbox refresh should not emit websocket fallback warnings: {warnings:?}"
    );

    let trace = stderr_text(&output);
    assert_text_contains(&trace, "远端 RPC / sync v2 foreground reconcile");
    assert_text_not_contains(&trace, "消息回退时刷新 JWT");

    let requests = server.requests();
    assert_eq!(requests.len(), 5);
    assert_eq!(json_body(&requests[0])["method"], "register");
    assert_eq!(
        json_body(&requests[1])["method"],
        "direct.e2ee.publish_prekey_bundle"
    );
    assert_eq!(json_body(&requests[2])["method"], "sync.bootstrap");
    assert!(!bearer_token(&requests[2]).is_empty());
    assert_eq!(json_body(&requests[3])["method"], "sync.bootstrap");
    assert_contains_text(&requests[3], "signature-input:");
    assert_eq!(json_body(&requests[4])["method"], "sync.delta");
    let refreshed_token = bearer_token(&requests[4]);
    assert_eq!(
        json_body(&requests[4])["params"]["body"]["reason"],
        "foreground_reconcile"
    );

    assert_vault_auth_token_is_used(
        workspace.path(),
        "bob",
        &refreshed_token,
        &[
            "--identity",
            "bob",
            "msg",
            "inbox",
            "--scope",
            "direct",
            "--limit",
            "1",
        ],
    );
}

fn register_ready_msg_identity(
    workspace: &Path,
    identity_name: &str,
    handle: &str,
    jwt_token: &str,
) {
    set_secret_storage_mode(workspace, "file_compat");
    let create = awiki_cmd(
        &[
            "--migration",
            "id",
            "create",
            "--name",
            "Message User",
            "--identity",
            identity_name,
        ],
        workspace,
    );
    assert_success(&create);

    let tenant = tenant_workspace(workspace);
    let index_path = tenant.join("identities").join("index.json");
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
    let identity_dir = tenant.join("identities").join(dir_name);
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

    set_secret_storage_mode(workspace, "vault_required");
    let migrate = awiki_cmd(&["--migration", "id", "vault", "migrate"], workspace);
    assert_success(&migrate);
}

fn assert_vault_auth_token_is_used(
    workspace: &Path,
    identity_name: &str,
    expected_token: &str,
    args: &[&str],
) {
    let response = if args.windows(2).any(|pair| pair == ["msg", "inbox"]) {
        TestResponse::sync_delta_empty()
    } else {
        TestResponse::ok(&json_rpc_result(json!({
            "accepted": true,
            "final_acceptance": true,
            "messages": [],
            "total": 0,
            "source": "remote_http"
        })))
    };
    let server = TestServer::new(vec![response]);
    write_msg_config(workspace, &server.base_url());

    let output = if args.windows(2).any(|pair| pair == ["msg", "inbox"]) {
        awiki_cmd_without_direct_e2ee(args, workspace)
    } else {
        awiki_cmd(args, workspace)
    };
    assert_success(&output);
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert_contains_text(
        &requests[0],
        &format!("Authorization: Bearer {expected_token}\r\n"),
    );

    let index_path = tenant_workspace(workspace)
        .join("identities")
        .join("index.json");
    let index: Value = serde_json::from_slice(&std::fs::read(index_path).unwrap()).unwrap();
    assert!(
        index["credentials"][identity_name]["vault_migration"]["refs"]["auth_jwt"].is_object(),
        "refreshed token should remain stored behind the vault auth_jwt ref"
    );
}

fn write_msg_config(workspace: &Path, base_url: &str) {
    write_default_tenant_registry(workspace, base_url, "awiki.ai");
    write_tenant_config(workspace, "runtime:\n  mode: http\n");
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

fn awiki_trace_cmd(args: &[&str], workspace: &Path) -> Output {
    let args = args
        .iter()
        .map(|arg| (*arg).to_string())
        .collect::<Vec<_>>();
    awiki_trace_cmd_owned(&args, workspace)
}

fn awiki_cmd_without_direct_e2ee(args: &[&str], workspace: &Path) -> Output {
    let args = args
        .iter()
        .map(|arg| (*arg).to_string())
        .collect::<Vec<_>>();
    let mut command = awiki_command(&args, workspace);
    command.env("AWIKI_MULTI_DEVICE_DIRECT_E2EE_ENABLED", "0");
    command.output().expect("run awiki-cli binary")
}

fn awiki_trace_cmd_without_direct_e2ee(args: &[&str], workspace: &Path) -> Output {
    let args = args
        .iter()
        .map(|arg| (*arg).to_string())
        .collect::<Vec<_>>();
    let mut command = awiki_command(&args, workspace);
    command
        .env("AWIKI_CLI_TRACE_TIMING", "1")
        .env("AWIKI_MULTI_DEVICE_DIRECT_E2EE_ENABLED", "0");
    command.output().expect("run awiki-cli binary")
}

fn awiki_cmd_owned(args: &[String], workspace: &Path) -> Output {
    let mut command = awiki_command(args, workspace);
    command.output().expect("run awiki-cli binary")
}

fn awiki_command(args: &[String], workspace: &Path) -> Command {
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
    command
}

fn awiki_trace_cmd_owned(args: &[String], workspace: &Path) -> Output {
    let mut command = awiki_command(args, workspace);
    command.env("AWIKI_CLI_TRACE_TIMING", "1");
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

fn success_json_with_stderr(output: &Output) -> Value {
    let envelope: Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be a JSON success envelope");
    assert_eq!(envelope["ok"], true);
    envelope
}

fn stderr_text(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(!stderr.is_empty(), "stderr should contain trace output");
    stderr
}

fn request_body(raw: &str) -> &str {
    raw.split("\r\n\r\n").nth(1).unwrap_or_default()
}

fn json_body(raw: &str) -> Value {
    serde_json::from_str(request_body(raw)).expect("request body json")
}

fn assert_text_contains(haystack: &str, needle: &str) {
    assert!(
        haystack.contains(needle),
        "expected text to contain {needle:?}, got:\n{haystack}"
    );
}

fn assert_text_not_contains(haystack: &str, needle: &str) {
    assert!(
        !haystack.contains(needle),
        "expected text not to contain {needle:?}, got:\n{haystack}"
    );
}

fn assert_contains_text(haystack: &str, needle: &str) {
    let header_probe = needle.strip_suffix("\r\n").unwrap_or(needle);
    if let Some((header_name, expected_value)) = header_probe.split_once(':') {
        let header_name = header_name.trim();
        let expected_value = expected_value.trim();
        if !header_name.is_empty()
            && haystack.lines().any(|line| {
                line.split_once(':').is_some_and(|(name, value)| {
                    name.trim().eq_ignore_ascii_case(header_name)
                        && (expected_value.is_empty() || value.trim() == expected_value)
                })
            })
        {
            return;
        }
    }
    assert!(
        haystack.contains(needle),
        "expected request to contain {needle:?}, got:\n{haystack}"
    );
}

fn bearer_token(request: &str) -> String {
    request
        .lines()
        .find_map(|line| {
            line.split_once(':').and_then(|(name, value)| {
                name.trim()
                    .eq_ignore_ascii_case("authorization")
                    .then(|| value.trim().strip_prefix("Bearer ").map(str::to_owned))
                    .flatten()
            })
        })
        .expect("bearer access token")
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

fn legacy_access_token(did: &str) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let claims = json!({
        "iss": "user-service",
        "sub": did,
        "type": "access",
        "iat": now,
        "exp": now + 3600,
    });
    format!(
        "e30.{}.signature",
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&claims).expect("serialize legacy access token claims"))
    )
}

#[derive(Debug, Clone)]
struct TestResponse {
    status: u16,
    body: String,
    access_token: Option<String>,
}

impl TestResponse {
    fn ok(body: &str) -> Self {
        Self {
            status: 200,
            body: body.to_string(),
            access_token: None,
        }
    }

    fn with_access_token(mut self, access_token: &str) -> Self {
        self.access_token = Some(access_token.to_owned());
        self
    }

    fn with_dynamic_access_token(mut self) -> Self {
        self.access_token = Some("__DYNAMIC_DEVICE_ACCESS_TOKEN__".to_owned());
        self
    }

    fn registration() -> Self {
        Self::ok("__DYNAMIC_REGISTRATION_RESPONSE__")
    }

    fn prekey_publication() -> Self {
        Self::ok("__DYNAMIC_PREKEY_PUBLICATION_RESPONSE__")
    }

    fn sync_bootstrap() -> Self {
        Self::ok("__DYNAMIC_SYNC_BOOTSTRAP_RESPONSE__")
    }

    fn sync_delta_empty() -> Self {
        Self::ok("__DYNAMIC_SYNC_DELTA_EMPTY_RESPONSE__")
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
    // The real CLI opens the identity, Vault, and transport before the first
    // request; parallel workspace tests can make that exceed five seconds.
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream
                    .set_nonblocking(false)
                    .expect("set test stream blocking");
                return Some(stream);
            }
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

fn dynamic_response_body(request: &str, marker: &str) -> String {
    match marker {
        "__DYNAMIC_REGISTRATION_RESPONSE__" => registration_response(request),
        "__DYNAMIC_PREKEY_PUBLICATION_RESPONSE__" => prekey_publication_response(request),
        "__DYNAMIC_SYNC_BOOTSTRAP_RESPONSE__" => {
            let binding = signed_device_binding(request);
            rpc_result_for_request(
                request,
                json!({
                    "mode": "tail_only",
                    "account_id": binding.account_id,
                    "device_id": binding.device_id,
                    "server_time": "2026-08-02T00:00:00Z",
                    "cursor": {"stream_epoch": "1", "scan_seq": "0"},
                    "read_state_baseline": [],
                    "group_state_baseline": [],
                    "warnings": []
                }),
            )
        }
        "__DYNAMIC_SYNC_DELTA_EMPTY_RESPONSE__" => rpc_result_for_request(
            request,
            json!({
                "mode": "delta",
                "server_time": "2026-08-02T00:00:01Z",
                "events": [],
                "next_cursor": {"stream_epoch": "1", "scan_seq": "0"},
                "has_more": false,
                "recovery": null,
                "warnings": []
            }),
        ),
        body => body.to_owned(),
    }
}

fn registration_response(request: &str) -> String {
    let rpc: Value =
        serde_json::from_str(request_body(request)).expect("registration request JSON");
    let params = &rpc["params"];
    let document = &params["did_document"];
    let did = document["id"].as_str().expect("registration DID");
    let device = &document["deviceManifest"]["devices"][0];
    let device_id = device["device_id"]
        .as_str()
        .expect("registration device ID");
    let key_id = device["signing_key_id"]
        .as_str()
        .expect("registration signing key ID");
    let handle = params["handle"].as_str().expect("registration handle");
    let account_id = format!("user-{handle}");
    json!({
        "jsonrpc": "2.0",
        "result": {
            "state": "registered",
            "did": did,
            "user_id": account_id,
            "message": "Registration successful",
            "access_token": device_access_token(
                did,
                &account_id,
                device_id,
                key_id,
                "initial"
            ),
            "handle": handle,
            "domain": "awiki.ai",
            "full_handle": format!("{handle}.awiki.ai"),
            "binding_generation": "1"
        },
        "id": rpc["id"].clone()
    })
    .to_string()
}

fn prekey_publication_response(request: &str) -> String {
    let rpc: Value = serde_json::from_str(request_body(request)).expect("prekey request JSON");
    let bundle = &rpc["params"]["body"]["prekey_bundle"];
    let published_opk_count = rpc["params"]["body"]["one_time_prekeys"]
        .as_array()
        .map(Vec::len)
        .expect("one-time prekeys");
    rpc_result_for_request(
        request,
        json!({
            "published": true,
            "owner_did": bundle["owner_did"].clone(),
            "owner_device_id": bundle["owner_device_id"].clone(),
            "bundle_id": bundle["bundle_id"].clone(),
            "published_at": "2026-08-02T00:00:00Z",
            "published_opk_count": published_opk_count
        }),
    )
}

fn rpc_result_for_request(request: &str, result: Value) -> String {
    let rpc: Value = serde_json::from_str(request_body(request)).expect("RPC request JSON");
    json!({"jsonrpc": "2.0", "result": result, "id": rpc["id"].clone()}).to_string()
}

struct SignedDeviceBinding {
    did: String,
    account_id: String,
    device_id: String,
    key_id: String,
}

fn signed_device_binding(request: &str) -> SignedDeviceBinding {
    let signature_input = request
        .lines()
        .find(|line| {
            line.split_once(':')
                .is_some_and(|(name, _)| name.trim().eq_ignore_ascii_case("signature-input"))
        })
        .expect("signed request signature-input");
    let key_id = signature_input
        .split_once("keyid=\"")
        .and_then(|(_, value)| value.split_once('"').map(|(key_id, _)| key_id))
        .expect("signed request key ID")
        .to_owned();
    let (did, fragment) = key_id.rsplit_once('#').expect("device signing key ID");
    let device_id = fragment
        .strip_suffix("-sign")
        .expect("device signing key fragment");
    SignedDeviceBinding {
        did: did.to_owned(),
        account_id: "user-bob".to_owned(),
        device_id: device_id.to_owned(),
        key_id,
    }
}

fn refreshed_access_token(request: &str) -> String {
    let binding = signed_device_binding(request);
    device_access_token(
        &binding.did,
        &binding.account_id,
        &binding.device_id,
        &binding.key_id,
        "refreshed",
    )
}

fn device_access_token(
    did: &str,
    account_id: &str,
    device_id: &str,
    key_id: &str,
    token_generation: &str,
) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let claims = json!({
        "iss": "user-service",
        "aud": ["awiki-user-service", "awiki-message-service"],
        "sub": did,
        "type": "access",
        "purpose": "awiki.device.access.v1",
        "did": did,
        "user_id": account_id,
        "device_id": device_id,
        "key_id": key_id,
        "auth_generation": 1,
        "scopes": ["device:manage", "device:read", "message:connect"],
        "iat": now,
        "nbf": now,
        "exp": now + 3600,
        "jti": format!("inbox-{token_generation}-{device_id}")
    });
    format!(
        "e30.{}.signature",
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&claims).expect("serialize device access token"))
    )
}

fn handle_connection(
    mut stream: TcpStream,
    requests: &Arc<Mutex<Vec<String>>>,
    response: TestResponse,
) {
    let request = read_http_request(&mut stream);
    let body = dynamic_response_body(&request, &response.body);
    let access_token = match response.access_token.as_deref() {
        Some("__DYNAMIC_DEVICE_ACCESS_TOKEN__") => Some(refreshed_access_token(&request)),
        Some(access_token) => Some(access_token.to_owned()),
        None => None,
    };
    requests.lock().expect("requests mutex").push(request);
    let body = body.as_bytes();
    let authentication_info = access_token
        .as_deref()
        .map(|token| format!("Authentication-Info: access_token=\"{token}\"\r\n"))
        .unwrap_or_default();
    let raw = format!(
        "HTTP/1.1 {} OK\r\nContent-Type: application/json\r\n{}Content-Length: {}\r\nConnection: close\r\n\r\n{}",
        response.status,
        authentication_info,
        body.len(),
        String::from_utf8_lossy(body)
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
