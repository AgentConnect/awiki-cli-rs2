#![cfg(unix)]

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

use support::{tenant_workspace, write_default_tenant_registry, write_tenant_config};

#[test]
fn msg_history_websocket_mode_uses_im_core_http_not_legacy_bridge() {
    let workspace = TempDir::new("msg-ws-history-http-cutover").expect("workspace");
    let alice_did = "did:wba:awiki.ai:alice:e1_alice";
    let missing_socket = tenant_workspace(workspace.path())
        .join("runtime")
        .join("missing.sock");
    let server = TestServer::new(vec![
        TestResponse::registration(),
        TestResponse::prekey_publication(),
        TestResponse::capabilities(),
        TestResponse::sync_bootstrap(),
        TestResponse::sync_delta_direct(),
        TestResponse::message_batch(),
        TestResponse::directory_lookup(),
    ]);
    write_msg_ws_config(
        workspace.path(),
        &server.base_url(),
        missing_socket.to_str().expect("socket path"),
    );
    register_sync_v2_identity(workspace.path(), "bob");
    let history_request_start = server.requests().len();

    let output = awiki_cmd(
        &[
            "--identity",
            "bob",
            "msg",
            "history",
            "--with",
            alice_did,
            "--limit",
            "7",
        ],
        workspace.path(),
    );

    if !output.status.success() {
        panic!(
            "history failed after methods {:?}",
            rpc_methods(&request_json_bodies(&server.requests()))
        );
    }

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Loaded 1 direct history messages");
    assert_eq!(envelope["data"]["source"], "local");
    assert_eq!(
        envelope["data"]["messages"][0]["id"],
        "msg-ws-history-http-1"
    );
    assert_no_legacy_websocket_fallback_warning(&envelope);

    let requests = server.requests();
    let bodies = request_json_bodies(&requests);
    assert_eq!(
        rpc_methods(&bodies[..history_request_start]),
        vec!["register", "direct.e2ee.publish_prekey_bundle"]
    );
    assert_sync_v2_history_without_legacy(&bodies[history_request_start..], false);
}

#[test]
fn msg_history_websocket_handle_resolution_uses_directory_then_im_core_http() {
    let workspace = TempDir::new("msg-ws-history-handle-http").expect("workspace");
    let missing_socket = tenant_workspace(workspace.path())
        .join("runtime")
        .join("missing.sock");
    let server = TestServer::new(vec![
        TestResponse::registration(),
        TestResponse::prekey_publication(),
        TestResponse::capabilities(),
        TestResponse::sync_bootstrap(),
        TestResponse::sync_delta_direct(),
        TestResponse::message_batch(),
        TestResponse::directory_lookup(),
    ]);
    write_msg_ws_config(
        workspace.path(),
        &server.base_url(),
        missing_socket.to_str().expect("socket path"),
    );
    register_sync_v2_identity(workspace.path(), "bob");
    let history_request_start = server.requests().len();

    let output = awiki_cmd(
        &[
            "--identity",
            "bob",
            "msg",
            "history",
            "--with",
            "alice.awiki.ai",
            "--limit",
            "5",
        ],
        workspace.path(),
    );

    if !output.status.success() {
        panic!(
            "history failed after methods {:?}",
            rpc_methods(&request_json_bodies(&server.requests()))
        );
    }

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Loaded 1 direct history messages");
    assert_eq!(envelope["data"]["source"], "local");
    assert_eq!(envelope["data"]["with"], "alice.awiki.ai");
    assert_eq!(
        envelope["data"]["messages"][0]["id"],
        "msg-ws-history-http-1"
    );
    assert_no_legacy_websocket_fallback_warning(&envelope);

    let requests = server.requests();
    let bodies = request_json_bodies(&requests);
    assert_eq!(
        rpc_methods(&bodies[..history_request_start]),
        vec!["register", "direct.e2ee.publish_prekey_bundle"]
    );
    assert_sync_v2_history_without_legacy(&bodies[history_request_start..], true);
}

#[test]
fn msg_history_websocket_mode_reports_http_transport_failure_without_cache_fallback() {
    let workspace = TempDir::new("msg-ws-history-http-failure").expect("workspace");
    let alice_did = "did:wba:awiki.ai:alice:e1_alice";
    let missing_socket = tenant_workspace(workspace.path())
        .join("runtime")
        .join("missing.sock");
    let registration_server = TestServer::new(vec![
        TestResponse::registration(),
        TestResponse::prekey_publication(),
    ]);
    write_msg_ws_config(
        workspace.path(),
        &registration_server.base_url(),
        missing_socket.to_str().expect("socket path"),
    );
    register_sync_v2_identity(workspace.path(), "bob");
    drop(registration_server);
    write_msg_ws_config(
        workspace.path(),
        &closed_local_url(),
        missing_socket.to_str().expect("socket path"),
    );

    let output = awiki_cmd(
        &["--identity", "bob", "msg", "history", "--with", alice_did],
        workspace.path(),
    );

    assert_transport_unavailable_without_legacy_fallback(&output);
}

#[test]
fn msg_history_websocket_unresolved_handle_does_not_use_legacy_local_history_cache() {
    let workspace = TempDir::new("msg-ws-history-handle-failure").expect("workspace");
    let missing_socket = tenant_workspace(workspace.path())
        .join("runtime")
        .join("missing.sock");
    let registration_server = TestServer::new(vec![
        TestResponse::registration(),
        TestResponse::prekey_publication(),
    ]);
    write_msg_ws_config(
        workspace.path(),
        &registration_server.base_url(),
        missing_socket.to_str().expect("socket path"),
    );
    register_sync_v2_identity(workspace.path(), "bob");
    drop(registration_server);
    write_msg_ws_config(
        workspace.path(),
        &closed_local_url(),
        missing_socket.to_str().expect("socket path"),
    );

    let output = awiki_cmd(
        &[
            "--identity",
            "bob",
            "msg",
            "history",
            "--with",
            "alice.awiki.ai",
            "--limit",
            "5",
        ],
        workspace.path(),
    );

    assert_transport_unavailable_without_legacy_fallback(&output);
}

fn register_sync_v2_identity(workspace: &Path, handle: &str) {
    let register = awiki_cmd(
        &[
            "id",
            "register",
            "--handle",
            handle,
            "--phone",
            "13800138000",
            "--otp",
            "123456",
        ],
        workspace,
    );
    assert_success(&register);
}

fn request_json_bodies(requests: &[String]) -> Vec<Value> {
    requests
        .iter()
        .map(|request| serde_json::from_str(request_body(request)).expect("request body JSON"))
        .collect()
}

fn rpc_methods(bodies: &[Value]) -> Vec<&str> {
    bodies
        .iter()
        .map(|body| body["method"].as_str().expect("RPC method"))
        .collect()
}

fn assert_sync_v2_history_without_legacy(bodies: &[Value], resolves_handle: bool) {
    let methods = rpc_methods(bodies);
    assert_eq!(
        methods,
        vec![
            "anp.get_capabilities",
            "sync.bootstrap",
            "sync.delta",
            "message.get_batch",
            "lookup",
        ]
    );
    if resolves_handle {
        assert_eq!(
            bodies
                .iter()
                .find(|body| body["method"] == "lookup")
                .expect("directory lookup request")["params"]["did"],
            "did:wba:awiki.ai:alice:e1_alice"
        );
    }
    assert_eq!(
        bodies
            .iter()
            .find(|body| body["method"] == "sync.delta")
            .expect("sync.delta request")["params"]["body"]["reason"],
        "foreground_reconcile"
    );
    assert!(!methods.contains(&"direct.get_history"));
    assert!(!methods.contains(&"group.list_messages"));
    assert!(!methods.contains(&"inbox.get"));
}

fn write_msg_ws_config(workspace: &Path, base_url: &str, socket_path: &str) {
    write_default_tenant_registry(workspace, base_url, "awiki.ai");
    write_tenant_config(
        workspace,
        format!("runtime:\n  mode: websocket\n  socket_path: {socket_path}\n").as_str(),
    );
}

fn closed_local_url() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind closed local url");
    let address = listener.local_addr().expect("local addr");
    drop(listener);
    format!("http://{address}")
}

fn awiki_cmd(args: &[&str], workspace: &Path) -> Output {
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

#[track_caller]
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

fn failure_json(output: &Output) -> Value {
    assert!(
        !output.status.success(),
        "expected failure; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stderr).expect("stderr should be a JSON error envelope")
}

fn assert_transport_unavailable_without_legacy_fallback(output: &Output) {
    assert_eq!(
        output.status.code(),
        Some(1),
        "unexpected exit status; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope = failure_json(output);
    assert_eq!(envelope["error"]["code"], "transport_unavailable");
    let message = envelope["error"]["message"].as_str().expect("message");
    assert_contains_text(message, "message transport is unavailable");
    assert!(
        !message.contains("local websocket bridge request failed"),
        "legacy bridge fallback should not be used, got: {message}"
    );
    assert!(
        !message.contains("local history cache"),
        "legacy local history cache should not be used, got: {message}"
    );
}

fn assert_no_legacy_websocket_fallback_warning(envelope: &Value) {
    let warnings = envelope["warnings"].as_array().cloned().unwrap_or_default();
    assert!(
        warnings.iter().all(|warning| {
            let warning = warning.as_str().unwrap_or_default();
            !warning.contains("used HTTP fallback")
                && !warning.contains("loaded data from local cache")
                && !warning.contains("local handle history cache")
        }),
        "unexpected legacy websocket fallback warning: {warnings:?}"
    );
}

fn request_body(raw: &str) -> &str {
    raw.split("\r\n\r\n").nth(1).unwrap_or_default()
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
        "expected text to contain {needle:?}, got:\n{haystack}"
    );
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

    fn registration() -> Self {
        Self::ok("__DYNAMIC_REGISTRATION_RESPONSE__")
    }

    fn prekey_publication() -> Self {
        Self::ok("__DYNAMIC_PREKEY_PUBLICATION_RESPONSE__")
    }

    fn sync_bootstrap() -> Self {
        Self::ok("__DYNAMIC_SYNC_BOOTSTRAP_RESPONSE__")
    }

    fn capabilities() -> Self {
        Self::ok("__DYNAMIC_CAPABILITIES_RESPONSE__")
    }

    fn sync_delta_direct() -> Self {
        Self::ok("__DYNAMIC_SYNC_DELTA_DIRECT_RESPONSE__")
    }

    fn message_batch() -> Self {
        Self::ok("__DYNAMIC_MESSAGE_BATCH_RESPONSE__")
    }

    fn directory_lookup() -> Self {
        Self::ok("__DYNAMIC_DIRECTORY_LOOKUP_RESPONSE__")
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
    // Parallel workspace tests can delay the debug CLI process startup on macOS.
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

fn handle_connection(
    mut stream: TcpStream,
    requests: &Arc<Mutex<Vec<String>>>,
    response: TestResponse,
) {
    let request = read_http_request(&mut stream);
    let response_body = dynamic_response_body(&request, &response.body);
    requests.lock().expect("requests mutex").push(request);
    let body = response_body.as_bytes();
    let raw = format!(
        "HTTP/1.1 {} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        response.status,
        body.len(),
        response_body
    );
    stream.write_all(raw.as_bytes()).expect("write response");
}

fn dynamic_response_body(request: &str, marker: &str) -> String {
    match marker {
        "__DYNAMIC_REGISTRATION_RESPONSE__" => registration_response(request),
        "__DYNAMIC_PREKEY_PUBLICATION_RESPONSE__" => prekey_publication_response(request),
        "__DYNAMIC_CAPABILITIES_RESPONSE__" => rpc_result_for_request(
            request,
            json!({
                "supported_profiles": [
                    "awiki.message-sync.explicit-negotiation.v1",
                    "sync.snapshot_paging.v1"
                ]
            }),
        ),
        "__DYNAMIC_SYNC_BOOTSTRAP_RESPONSE__" => {
            let binding = device_binding_from_request(request);
            let rpc: Value =
                serde_json::from_str(request_body(request)).expect("sync.bootstrap request JSON");
            let client_instance_id = rpc["params"]["body"]["client_instance_id"]
                .as_str()
                .expect("sync.bootstrap client_instance_id");
            let requested = rpc["params"]["body"]["capabilities"]["requested_sync_capabilities"]
                .as_array()
                .expect("sync.bootstrap requested capabilities")
                .clone();
            let has_p5 = requested.iter().any(|value| value == "lanes.p5_device.v1");
            let has_p6 = requested.iter().any(|value| value == "lanes.p6_group.v1");
            let mut lanes = serde_json::Map::new();
            if has_p5 {
                lanes.insert(
                    "p5_device".to_owned(),
                    json!({
                        "cursor": {"stream_epoch": "41", "scan_seq": "0"},
                        "committed_seq": "0"
                    }),
                );
            }
            if has_p6 {
                lanes.insert(
                    "p6_group".to_owned(),
                    json!({
                        "cursor": {"stream_epoch": "42", "scan_seq": "0"},
                        "committed_seq": "0"
                    }),
                );
            }
            let mut result = json!({
                "mode": "tail_only",
                "account_id": binding.account_id,
                "device_id": binding.device_id,
                "server_time": "2026-08-02T00:00:00Z",
                "cursor": {"stream_epoch": "1", "scan_seq": "0"},
                "read_state_baseline": [],
                "group_state_baseline": [],
                "warnings": [],
                "snapshot_capability": {"schema": 3, "delivery": "paged_v1"},
                "sync_capabilities": requested,
                "lanes": lanes
            });
            if has_p6 {
                result["p6_delivery"] = json!({
                    "profile": "p6.delivery_context.v1",
                    "client_instance_id": client_instance_id,
                    "activated": true
                });
            }
            rpc_result_for_request(request, result)
        }
        "__DYNAMIC_SYNC_DELTA_DIRECT_RESPONSE__" => {
            let binding = device_binding_from_request(request);
            rpc_result_for_request(
                request,
                json!({
                    "mode": "delta",
                    "server_time": "2026-08-02T00:00:01Z",
                    "events": [{
                        "event_id": "event-ws-history-http-1",
                        "stream_epoch": "1",
                        "event_seq": "1",
                        "event_type": "message.created",
                        "schema_version": 1,
                        "ignore_safe": false,
                        "account_id": binding.account_id,
                        "recipient_device_id": null,
                        "origin_did": "did:wba:awiki.ai:alice:e1_alice",
                        "origin_device_id": "device-alice",
                        "aggregate_kind": "direct_message",
                        "aggregate_id": "msg-ws-history-http-1",
                        "state_version": null,
                        "thread_key": "remote-thread-alice",
                        "occurred_at": "2026-08-02T00:00:01Z",
                        "payload": {
                            "message_kind": "direct_plain",
                            "direction": "incoming",
                            "sender_did_snapshot": "did:wba:awiki.ai:alice:e1_alice",
                            "recipient_did_snapshot": binding.did,
                            "client_message_id": "msg-ws-history-http-1"
                        },
                        "source": {}
                    }],
                    "next_cursor": {"stream_epoch": "1", "scan_seq": "1"},
                    "has_more": false,
                    "recovery": null,
                    "warnings": []
                }),
            )
        }
        "__DYNAMIC_MESSAGE_BATCH_RESPONSE__" => {
            let binding = device_binding_from_request(request);
            rpc_result_for_request(
                request,
                json!({
                    "items": [{
                        "event_id": "event-ws-history-http-1",
                        "message": direct_message(&binding.did)
                    }],
                    "unavailable": []
                }),
            )
        }
        "__DYNAMIC_DIRECTORY_LOOKUP_RESPONSE__" => rpc_result_for_request(
            request,
            json!({
                "did": "did:wba:awiki.ai:alice:e1_alice",
                "user_id": "user-alice",
                "handle": "alice",
                "full_handle": "alice.awiki.ai",
                "domain": "awiki.ai",
                "status": "active",
                "binding_generation": "1"
            }),
        ),
        body => body.to_owned(),
    }
}

fn direct_message(receiver_did: &str) -> Value {
    json!({
        "id": "msg-ws-history-http-1",
        "type": "text",
        "thread_kind": "direct",
        "sender_did": "did:wba:awiki.ai:alice:e1_alice",
        "receiver_did": receiver_did,
        "content_type": "text/plain",
        "content": "hello from Sync V2",
        "server_seq": "1",
        "created_at": "2026-08-02T00:00:01Z",
        "sent_at": "2026-08-02T00:00:01Z",
        "client_msg_id": "msg-ws-history-http-1",
        "is_read": false,
        "peer_user_id": "user-alice",
        "peer_full_handle": "alice.awiki.ai"
    })
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
            "access_token": device_access_token(did, &account_id, device_id, key_id),
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

struct DeviceBinding {
    did: String,
    account_id: String,
    device_id: String,
}

fn device_binding_from_request(request: &str) -> DeviceBinding {
    let token = request
        .lines()
        .find_map(|line| {
            line.split_once(':').and_then(|(name, value)| {
                name.trim()
                    .eq_ignore_ascii_case("authorization")
                    .then(|| value.trim().strip_prefix("Bearer ").map(str::to_owned))
                    .flatten()
            })
        })
        .expect("device bearer token");
    let payload = token.split('.').nth(1).expect("JWT payload");
    let claims: Value = serde_json::from_slice(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(payload)
            .expect("decode JWT payload"),
    )
    .expect("JWT claims");
    DeviceBinding {
        did: claims["did"].as_str().expect("token DID").to_owned(),
        account_id: claims["user_id"]
            .as_str()
            .expect("token account ID")
            .to_owned(),
        device_id: claims["device_id"]
            .as_str()
            .expect("token device ID")
            .to_owned(),
    }
}

fn device_access_token(did: &str, account_id: &str, device_id: &str, key_id: &str) -> String {
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
        "jti": format!("msg-ws-history-{device_id}")
    });
    format!(
        "e30.{}.signature",
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&claims).expect("serialize device access token"))
    )
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
