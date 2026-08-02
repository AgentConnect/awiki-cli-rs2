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

use support::{
    open_local_state, set_secret_storage_mode, tenant_workspace, write_default_tenant_registry,
    write_tenant_config,
};

#[test]
fn msg_inbox_websocket_target_filter_is_unsupported_before_legacy_bridge() {
    let workspace = TempDir::new("msg-ws-inbox-filter-unsupported").expect("workspace");
    register_ready_msg_identity(workspace.path(), "bob-ws-inbox-filter", "bob", "jwt-bob");
    let alice_did = "did:wba:awiki.ai:alice:e1_alice";
    let missing_socket = tenant_workspace(workspace.path())
        .join("runtime")
        .join("missing.sock");
    let server = TestServer::new(Vec::new());
    write_msg_ws_config(
        workspace.path(),
        &server.base_url(),
        missing_socket.to_str().expect("socket path"),
    );

    let output = awiki_cmd(
        &[
            "--identity",
            "bob-ws-inbox-filter",
            "msg",
            "inbox",
            "--scope",
            "direct",
            "--with",
            alice_did,
            "--limit",
            "5",
            "--unread",
        ],
        workspace.path(),
    );

    assert_unsupported_capability(&output, "msg.inbox", "inbox-target-filters", "Phase 3");
    assert!(server.requests().is_empty());
}

#[test]
fn msg_inbox_websocket_mark_read_side_effect_is_unsupported() {
    let workspace = TempDir::new("msg-ws-inbox-mark-read-unsupported").expect("workspace");
    register_ready_msg_identity(workspace.path(), "bob-ws-inbox-mark", "bob", "jwt-bob");
    let missing_socket = tenant_workspace(workspace.path())
        .join("runtime")
        .join("missing.sock");
    let server = TestServer::new(Vec::new());
    write_msg_ws_config(
        workspace.path(),
        &server.base_url(),
        missing_socket.to_str().expect("socket path"),
    );

    let output = awiki_cmd(
        &[
            "--identity",
            "bob-ws-inbox-mark",
            "msg",
            "inbox",
            "--mark-read",
            "--limit",
            "5",
        ],
        workspace.path(),
    );

    assert_unsupported_capability(
        &output,
        "msg.inbox",
        "inbox-mark-read-side-effect",
        "Phase 3",
    );
    assert!(server.requests().is_empty());
}

#[test]
fn msg_inbox_websocket_mode_uses_im_core_http_not_legacy_bridge_for_default_scope() {
    let workspace = TempDir::new("msg-ws-inbox-http-cutover").expect("workspace");
    let missing_socket = tenant_workspace(workspace.path())
        .join("runtime")
        .join("missing.sock");
    let server = TestServer::new(vec![
        TestResponse::registration(),
        TestResponse::prekey_publication(),
        TestResponse::sync_bootstrap(),
        TestResponse::sync_delta_group(),
        TestResponse::message_batch(),
    ]);
    write_msg_ws_config(
        workspace.path(),
        &server.base_url(),
        missing_socket.to_str().expect("socket path"),
    );
    let _bob = register_exact_msg_identity(workspace.path());

    let output = awiki_cmd(
        &[
            "--identity",
            "bob",
            "msg",
            "inbox",
            "--scope",
            "all",
            "--limit",
            "7",
            "--unread",
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Loaded 1 inbox messages");
    assert_eq!(envelope["data"]["source"], "local_projection");
    assert_eq!(
        envelope["data"]["messages"][0]["id"],
        "did:wba:awiki.ai:groups:sync:e1_group:1"
    );
    assert_no_legacy_websocket_fallback_warning(&envelope);

    let requests = server.requests();
    assert_eq!(requests.len(), 5);
    let bodies = requests
        .iter()
        .map(|request| {
            serde_json::from_str::<Value>(request_body(request)).expect("request body JSON")
        })
        .collect::<Vec<_>>();
    let methods = bodies
        .iter()
        .filter_map(|body| body["method"].as_str())
        .collect::<Vec<_>>();
    assert!(methods.contains(&"sync.bootstrap"));
    assert!(methods.contains(&"sync.delta"));
    assert!(methods.contains(&"message.get_batch"));
    assert!(!methods.contains(&"inbox.get"));
    assert!(!methods.contains(&"group.list"));
    let delta = bodies
        .iter()
        .find(|body| body["method"] == "sync.delta")
        .expect("sync.delta request");
    assert_eq!(delta["params"]["body"]["reason"], "foreground_reconcile");
}

#[test]
fn msg_inbox_hydrates_exact_device_controls_before_foreground_sync() {
    let workspace = TempDir::new("msg-ws-inbox-secure-before-sync").expect("workspace");
    let missing_socket = tenant_workspace(workspace.path())
        .join("runtime")
        .join("missing.sock");
    let server = TestServer::new(vec![
        TestResponse::registration(),
        TestResponse::prekey_publication(),
        TestResponse::empty_secure_inbox(),
        TestResponse::sync_bootstrap(),
        TestResponse::empty_sync_delta(),
    ]);
    write_msg_ws_config(
        workspace.path(),
        &server.base_url(),
        missing_socket.to_str().expect("socket path"),
    );
    let _bob = register_exact_msg_identity(workspace.path());
    let request_start = server.requests().len();

    let mut command = Command::new(env!("CARGO_BIN_EXE_awiki-cli"));
    command
        .args([
            "--identity",
            "bob",
            "msg",
            "inbox",
            "--scope",
            "direct",
            "--limit",
            "7",
        ])
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace.path())
        .env("HOME", workspace.path().join("home"))
        .env("USERPROFILE", workspace.path().join("home"))
        .env("AWIKI_CLI_UPDATE_CACHE_ONLY", "1")
        .env("AWIKI_MULTI_DEVICE_DIRECT_E2EE_ENABLED", "1");
    let output = command.output().expect("run secure awiki-cli inbox");

    assert_success(&output);
    let methods = server.requests()[request_start..]
        .iter()
        .map(|request| {
            serde_json::from_str::<Value>(request_body(request)).expect("request body JSON")
        })
        .filter_map(|body| body["method"].as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    assert_eq!(methods, vec!["inbox.get", "sync.bootstrap", "sync.delta"]);
}

#[test]
fn msg_inbox_websocket_mode_reports_http_transport_failure_without_cache_fallback() {
    let workspace = TempDir::new("msg-ws-inbox-http-failure").expect("workspace");
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
    let bob = register_exact_msg_identity(workspace.path());
    drop(registration_server);
    seed_stale_local_message(workspace.path(), &bob);
    write_msg_ws_config(
        workspace.path(),
        &closed_local_url(),
        missing_socket.to_str().expect("socket path"),
    );

    let output = awiki_cmd(
        &["--identity", "bob", "msg", "inbox", "--limit", "5"],
        workspace.path(),
    );

    assert_transport_unavailable_without_legacy_fallback(&output);
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("stale-ws-inbox-message"),
        "failed Sync V2 reconcile must not return a stale local projection"
    );
}

struct ExactTestIdentity {
    identity_id: String,
    did: String,
}

fn register_exact_msg_identity(workspace: &Path) -> ExactTestIdentity {
    let output = awiki_cmd(
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
        workspace,
    );
    assert_success(&output);
    let envelope = success_json(&output);
    let did = envelope["data"]["identity"]["did"]
        .as_str()
        .expect("registered DID")
        .to_owned();
    let index: Value = serde_json::from_slice(
        &std::fs::read(
            tenant_workspace(workspace)
                .join("identities")
                .join("index.json"),
        )
        .expect("identity index"),
    )
    .expect("identity index JSON");
    let identity_id = index["credentials"]["bob"]["unique_id"]
        .as_str()
        .expect("registered identity ID")
        .to_owned();
    ExactTestIdentity { identity_id, did }
}

fn seed_stale_local_message(workspace: &Path, identity: &ExactTestIdentity) {
    open_local_state(workspace)
        .execute(
            "INSERT INTO messages (msg_id, owner_identity_id, owner_did, conversation_id, wire_thread_kind, wire_thread_ref, hydration_state, thread_id, direction, sender_did, receiver_did, content_type, content, sent_at, stored_at, is_read, credential_name) VALUES (?1, ?2, ?3, ?4, 'direct', ?5, 'hydrated', ?4, 0, ?5, ?3, 'text/plain', 'stale local projection', '2026-08-02T00:00:00Z', '2026-08-02T00:00:00Z', 0, 'bob')",
            rusqlite::params![
                "stale-ws-inbox-message",
                identity.identity_id,
                identity.did,
                "dm:did:wba:awiki.ai:stale:e1_stale",
                "did:wba:awiki.ai:stale:e1_stale",
            ],
        )
        .expect("seed stale exact-owner projection");
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

fn assert_unsupported_capability(
    output: &Output,
    command: &str,
    capability: &str,
    required_phase: &str,
) {
    assert_eq!(
        output.status.code(),
        Some(2),
        "unexpected exit status; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope = failure_json(output);
    assert_eq!(envelope["error"]["code"], "unsupported_capability");
    assert_eq!(envelope["error"]["details"]["command"], command);
    assert_eq!(envelope["error"]["details"]["capability"], capability);
    assert_eq!(
        envelope["error"]["details"]["required_phase"],
        required_phase
    );
    assert_eq!(
        envelope["error"]["details"]["cutover_status"],
        "unsupported"
    );
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
        !message.contains("loaded data from local cache"),
        "legacy local cache fallback should not be used, got: {message}"
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

    fn sync_delta_group() -> Self {
        Self::ok("__DYNAMIC_SYNC_DELTA_GROUP_RESPONSE__")
    }

    fn empty_secure_inbox() -> Self {
        Self::ok("__DYNAMIC_EMPTY_SECURE_INBOX_RESPONSE__")
    }

    fn empty_sync_delta() -> Self {
        Self::ok("__DYNAMIC_EMPTY_SYNC_DELTA_RESPONSE__")
    }

    fn message_batch() -> Self {
        Self::ok("__DYNAMIC_MESSAGE_BATCH_RESPONSE__")
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
        "__DYNAMIC_SYNC_BOOTSTRAP_RESPONSE__" => {
            let binding = device_binding_from_request(request);
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
        "__DYNAMIC_SYNC_DELTA_GROUP_RESPONSE__" => {
            let binding = device_binding_from_request(request);
            rpc_result_for_request(
                request,
                json!({
                    "mode": "delta",
                    "server_time": "2026-08-02T00:00:01Z",
                    "events": [{
                        "event_id": "event-ws-inbox-http-1",
                        "stream_epoch": "1",
                        "event_seq": "1",
                        "event_type": "message.created",
                        "schema_version": 1,
                        "ignore_safe": false,
                        "account_id": binding.account_id,
                        "recipient_device_id": null,
                        "origin_did": "did:wba:awiki.ai:alice:e1_alice",
                        "origin_device_id": "device-alice",
                        "aggregate_kind": "group_message",
                        "aggregate_id": "msg-ws-inbox-http-1",
                        "state_version": null,
                        "thread_key": "did:wba:awiki.ai:groups:sync:e1_group",
                        "occurred_at": "2026-08-02T00:00:01Z",
                        "payload": {
                            "message_kind": "group_plain",
                            "direction": "incoming",
                            "group_did": "did:wba:awiki.ai:groups:sync:e1_group",
                            "sender_did_snapshot": "did:wba:awiki.ai:alice:e1_alice",
                            "recipient_did_snapshot": binding.did,
                            "client_message_id": "msg-ws-inbox-http-1"
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
        "__DYNAMIC_EMPTY_SECURE_INBOX_RESPONSE__" => rpc_result_for_request(
            request,
            json!({"messages": [], "has_more": false, "warnings": []}),
        ),
        "__DYNAMIC_EMPTY_SYNC_DELTA_RESPONSE__" => rpc_result_for_request(
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
        "__DYNAMIC_MESSAGE_BATCH_RESPONSE__" => {
            let binding = device_binding_from_request(request);
            rpc_result_for_request(
                request,
                json!({
                    "items": [{
                        "event_id": "event-ws-inbox-http-1",
                        "message": group_message(&binding.did)
                    }],
                    "unavailable": []
                }),
            )
        }
        body => body.to_owned(),
    }
}

fn group_message(receiver_did: &str) -> Value {
    json!({
        "id": "msg-ws-inbox-http-1",
        "type": "text",
        "thread_kind": "group",
        "group_did": "did:wba:awiki.ai:groups:sync:e1_group",
        "sender_did": "did:wba:awiki.ai:alice:e1_alice",
        "receiver_did": receiver_did,
        "content_type": "text/plain",
        "content": "hello from HTTP inbox",
        "server_seq": "1",
        "created_at": "2026-08-02T00:00:01Z",
        "sent_at": "2026-08-02T00:00:01Z",
        "client_msg_id": "msg-ws-inbox-http-1",
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
        did: claims["did"].as_str().expect("token did").to_owned(),
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
        "jti": format!("msg-ws-inbox-{device_id}")
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
