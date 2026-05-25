use anp::authentication::{create_did_wba_document, DidDocumentOptions};
use awiki_cli::host_runtime::bridge::BridgeRequest;
use awiki_cli::host_runtime::listener_bridge_connection::{
    execute_listener_bridge_request, handle_listener_bridge_connection_once, ListenerBridgeRuntime,
    ListenerBridgeSession,
};
use awiki_cli::host_runtime::listener_identity_record::RuntimeIdentityRecord;
use serde_json::{json, Map, Value};

#[test]
fn bridge_connection_executes_direct_rpc_and_returns_result_like_go() {
    let mut runtime = RuntimeStub::connected("alice", signed_record("alice"))
        .with_send_result(json!({ "message_id": "msg-1" }));

    let result = execute_listener_bridge_request(
        &mut runtime,
        bridge_request(
            "direct.send",
            json!({
                "target": "did:bob",
                "text": "hello",
                "type": "event",
            }),
        ),
    )
    .expect("direct send succeeds");

    assert_eq!(result, object_map(json!({ "message_id": "msg-1" })));
    assert_eq!(
        runtime.actions,
        vec![
            "ensure_session:alice",
            "send_rpc:alice:direct.send:anp.direct.base.v1",
        ]
    );
    assert!(runtime.mark_read_calls.is_empty());
}

#[test]
fn bridge_connection_returns_ensure_session_error_before_record_or_client_work() {
    let mut runtime =
        RuntimeStub::ensure_error("identity missing").with_send_result(json!({ "unused": true }));

    let err =
        execute_listener_bridge_request(&mut runtime, bridge_request("direct.send", json!({})))
            .expect_err("ensure error");

    assert_eq!(err.to_string(), "identity missing");
    assert_eq!(runtime.actions, vec!["ensure_session:alice"]);
    assert!(runtime.mark_read_calls.is_empty());
}

#[test]
fn bridge_connection_requires_current_record_and_client_like_go() {
    let missing_record = RuntimeStub::session(ListenerBridgeSession::connected(
        "alice",
        record("alice", "did:alice"),
    ))
    .without_record();
    let mut runtime = missing_record;

    let err =
        execute_listener_bridge_request(&mut runtime, bridge_request("direct.send", json!({})))
            .expect_err("missing record");
    assert_eq!(
        err.to_string(),
        "websocket session is not connected for identity alice"
    );
    assert_eq!(runtime.actions, vec!["ensure_session:alice"]);

    let mut runtime = RuntimeStub::session(ListenerBridgeSession::disconnected(
        "bob",
        Some(record("bob", "did:bob")),
    ));
    let err =
        execute_listener_bridge_request(&mut runtime, bridge_request("direct.send", json!({})))
            .expect_err("missing client");
    assert_eq!(
        err.to_string(),
        "websocket session is not connected for identity bob"
    );
    assert_eq!(runtime.actions, vec!["ensure_session:alice"]);
}

#[test]
fn bridge_connection_fetches_group_create_service_did_before_building_rpc() {
    let mut runtime = RuntimeStub::connected("alice", signed_record("alice"))
        .with_service_did("did:service")
        .with_send_result(json!({ "group_did": "did:group" }));

    let result = execute_listener_bridge_request(
        &mut runtime,
        bridge_request("group.create", json!({ "name": "Bridge Group" })),
    )
    .expect("group create succeeds");

    assert_eq!(result, object_map(json!({ "group_did": "did:group" })));
    assert_eq!(
        runtime.actions,
        vec![
            "ensure_session:alice",
            "fetch_service_did:alice",
            "send_rpc:alice:group.create:anp.group.base.v1",
        ]
    );
}

#[test]
fn bridge_connection_returns_group_create_service_did_error_before_building_rpc() {
    let mut runtime = RuntimeStub::connected("alice", signed_record("alice"))
        .with_service_error("capabilities unavailable")
        .with_send_result(json!({ "unused": true }));

    let err = execute_listener_bridge_request(
        &mut runtime,
        bridge_request("group.create", json!({ "name": "Bridge Group" })),
    )
    .expect_err("service DID error");

    assert_eq!(err.to_string(), "capabilities unavailable");
    assert_eq!(
        runtime.actions,
        vec!["ensure_session:alice", "fetch_service_did:alice"]
    );
    assert!(runtime.mark_read_calls.is_empty());
}

#[test]
fn bridge_connection_returns_build_error_before_sending_rpc() {
    let mut runtime =
        RuntimeStub::connected("alice", record("alice", "did:alice")).with_send_result(json!({}));

    let err =
        execute_listener_bridge_request(&mut runtime, bridge_request("group.unknown", json!({})))
            .expect_err("build error");

    assert_eq!(
        err.to_string(),
        "unsupported websocket bridge method: group.unknown"
    );
    assert_eq!(runtime.actions, vec!["ensure_session:alice"]);
    assert!(runtime.mark_read_calls.is_empty());
}

#[test]
fn bridge_connection_returns_send_error_before_mark_read_side_effect() {
    let mut runtime =
        RuntimeStub::connected("alice", record("alice", "did:alice")).with_send_error("rpc failed");

    let err = execute_listener_bridge_request(
        &mut runtime,
        bridge_request("inbox.mark_read", json!({ "message_ids": ["msg-1"] })),
    )
    .expect_err("send error");

    assert_eq!(err.to_string(), "rpc failed");
    assert_eq!(
        runtime.actions,
        vec![
            "ensure_session:alice",
            "send_rpc:alice:inbox.mark_read:anp.inbox.local.v1",
        ]
    );
    assert!(runtime.mark_read_calls.is_empty());
}

#[test]
fn bridge_connection_marks_messages_read_only_after_successful_mark_read_rpc() {
    let mut runtime = RuntimeStub::connected("alice", record("alice", "did:alice"))
        .with_send_result(json!({ "updated": 2 }))
        .with_mark_read_error("local db failed");

    let result = execute_listener_bridge_request(
        &mut runtime,
        bridge_request(
            "inbox.mark_read",
            json!({ "message_ids": ["msg-1", "", 3, "msg-2"] }),
        ),
    )
    .expect("mark read RPC succeeds even if local DB update fails");

    assert_eq!(result, object_map(json!({ "updated": 2 })));
    assert_eq!(
        runtime.actions,
        vec![
            "ensure_session:alice",
            "send_rpc:alice:inbox.mark_read:anp.inbox.local.v1",
            "mark_messages_read:did:alice:msg-1,msg-2",
        ]
    );
    assert_eq!(
        runtime.mark_read_calls,
        vec![(
            "did:alice".to_string(),
            vec!["msg-1".to_string(), "msg-2".to_string()]
        )]
    );
}

#[test]
fn bridge_connection_framing_wraps_execute_result_like_go_handle_conn() {
    let request = json!({
        "method": "inbox.mark_read",
        "identity_name": "alice",
        "params": { "message_ids": ["msg-1"] }
    });
    let mut stream = MemoryDuplex::new(format!("{request}\nignored\n").into_bytes());
    let mut runtime = RuntimeStub::connected("alice", record("alice", "did:alice"))
        .with_send_result(json!({ "updated": 1 }));

    handle_listener_bridge_connection_once(&mut stream, &mut runtime).expect("bridge response");

    let response: Value = serde_json::from_slice(stream.output()).expect("response json");
    assert_eq!(response["ok"], true);
    assert_eq!(response["result"]["updated"], 1);
    assert!(response.get("error").is_none());
    assert_eq!(
        runtime.mark_read_calls,
        vec![("did:alice".to_string(), vec!["msg-1".to_string()])]
    );
}

#[test]
fn bridge_connection_framing_writes_execute_errors_as_bridge_errors() {
    let request = json!({
        "method": "inbox.mark_read",
        "identity_name": "alice",
        "params": { "message_ids": [] }
    });
    let mut stream = MemoryDuplex::new(format!("{request}\n").into_bytes());
    let mut runtime = RuntimeStub::connected("alice", record("alice", "did:alice"))
        .with_send_result(json!({ "unused": true }));

    handle_listener_bridge_connection_once(&mut stream, &mut runtime).expect("bridge response");

    let response: Value = serde_json::from_slice(stream.output()).expect("response json");
    assert_eq!(response["ok"], false);
    assert!(response["error"]["message"]
        .as_str()
        .expect("error message")
        .contains("message_ids are required"));
    assert!(response.get("result").is_none());
    assert!(runtime.mark_read_calls.is_empty());
}

#[derive(Debug, Clone)]
struct RuntimeStub {
    session: Option<ListenerBridgeSession>,
    ensure_error: Option<String>,
    service_did: Result<String, String>,
    send_result: Result<Map<String, Value>, String>,
    mark_read_error: Option<String>,
    actions: Vec<String>,
    mark_read_calls: Vec<(String, Vec<String>)>,
}

impl RuntimeStub {
    fn connected(identity_name: &str, record: RuntimeIdentityRecord) -> Self {
        Self::session(ListenerBridgeSession::connected(identity_name, record))
    }

    fn session(session: ListenerBridgeSession) -> Self {
        Self {
            session: Some(session),
            ensure_error: None,
            service_did: Ok(String::new()),
            send_result: Ok(Map::new()),
            mark_read_error: None,
            actions: Vec::new(),
            mark_read_calls: Vec::new(),
        }
    }

    fn ensure_error(error: &str) -> Self {
        Self {
            session: None,
            ensure_error: Some(error.to_string()),
            service_did: Ok(String::new()),
            send_result: Ok(Map::new()),
            mark_read_error: None,
            actions: Vec::new(),
            mark_read_calls: Vec::new(),
        }
    }

    fn without_record(mut self) -> Self {
        if let Some(session) = &mut self.session {
            session.record = None;
        }
        self
    }

    fn with_service_did(mut self, service_did: &str) -> Self {
        self.service_did = Ok(service_did.to_string());
        self
    }

    fn with_service_error(mut self, error: &str) -> Self {
        self.service_did = Err(error.to_string());
        self
    }

    fn with_send_result(mut self, result: Value) -> Self {
        self.send_result = Ok(object_map(result));
        self
    }

    fn with_send_error(mut self, error: &str) -> Self {
        self.send_result = Err(error.to_string());
        self
    }

    fn with_mark_read_error(mut self, error: &str) -> Self {
        self.mark_read_error = Some(error.to_string());
        self
    }
}

impl ListenerBridgeRuntime for RuntimeStub {
    fn ensure_session(&mut self, identity_name: &str) -> anyhow::Result<ListenerBridgeSession> {
        self.actions.push(format!("ensure_session:{identity_name}"));
        if let Some(error) = &self.ensure_error {
            anyhow::bail!(error.clone());
        }
        Ok(self.session.clone().expect("test session"))
    }

    fn fetch_message_service_did(
        &mut self,
        session: &ListenerBridgeSession,
    ) -> anyhow::Result<String> {
        self.actions
            .push(format!("fetch_service_did:{}", session.identity_name));
        self.service_did.clone().map_err(anyhow::Error::msg)
    }

    fn send_rpc(
        &mut self,
        session: &ListenerBridgeSession,
        method: &str,
        params: Value,
    ) -> anyhow::Result<Map<String, Value>> {
        let profile = params["meta"]["profile"].as_str().unwrap_or_default();
        self.actions.push(format!(
            "send_rpc:{}:{method}:{profile}",
            session.identity_name
        ));
        self.send_result.clone().map_err(anyhow::Error::msg)
    }

    fn mark_messages_read(
        &mut self,
        owner_did: &str,
        message_ids: &[String],
    ) -> anyhow::Result<()> {
        self.actions.push(format!(
            "mark_messages_read:{owner_did}:{}",
            message_ids.join(",")
        ));
        self.mark_read_calls
            .push((owner_did.to_string(), message_ids.to_vec()));
        if let Some(error) = &self.mark_read_error {
            anyhow::bail!(error.clone());
        }
        Ok(())
    }
}

#[derive(Debug)]
struct MemoryDuplex {
    input: std::io::Cursor<Vec<u8>>,
    output: Vec<u8>,
}

impl MemoryDuplex {
    fn new(input: Vec<u8>) -> Self {
        Self {
            input: std::io::Cursor::new(input),
            output: Vec::new(),
        }
    }

    fn output(&self) -> &[u8] {
        &self.output
    }
}

impl std::io::Read for MemoryDuplex {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        std::io::Read::read(&mut self.input, buffer)
    }
}

impl std::io::Write for MemoryDuplex {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.output.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn bridge_request(method: &str, params: Value) -> BridgeRequest {
    BridgeRequest {
        method: method.to_string(),
        params: object_map(params),
        identity_name: "alice".to_string(),
    }
}

fn record(identity_name: &str, did: &str) -> RuntimeIdentityRecord {
    RuntimeIdentityRecord {
        identity_name: identity_name.to_string(),
        did: did.to_string(),
        ..RuntimeIdentityRecord::default()
    }
}

fn signed_record(identity_name: &str) -> RuntimeIdentityRecord {
    let generated = generated_identity();
    RuntimeIdentityRecord {
        identity_name: identity_name.to_string(),
        did: generated.did,
        did_document: Some(generated.did_document),
        key1_private_pem: generated.key1_private_pem,
        ..RuntimeIdentityRecord::default()
    }
}

struct GeneratedIdentity {
    did: String,
    did_document: Value,
    key1_private_pem: String,
}

fn generated_identity() -> GeneratedIdentity {
    let bundle = create_did_wba_document(
        "awiki.ai",
        DidDocumentOptions {
            path_segments: vec!["user".to_string(), "alice".to_string()],
            domain: Some("awiki.ai".to_string()),
            challenge: Some("runtime-bridge-connection-contract".to_string()),
            ..DidDocumentOptions::default()
        },
    )
    .expect("generated did document");
    let key1_private_pem = bundle
        .private_key_pem("key-1")
        .expect("key-1 private pem")
        .to_string();
    GeneratedIdentity {
        did: bundle.did().expect("generated did").to_string(),
        did_document: bundle.did_document,
        key1_private_pem,
    }
}

fn object_map(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}
