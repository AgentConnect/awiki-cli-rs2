use anp::authentication::{create_did_wba_document, DidDocumentOptions};
use awiki_cli::host_runtime::bridge::BridgeRequest;
use awiki_cli::host_runtime::listener_bridge_dispatch::{
    bridge_request_flow_plan, build_bridge_rpc_call, BridgeEnsureSessionOutcome,
    BridgeRequestFlowAction, BridgeRequestFlowDecision, BridgeRpcBuildOutcome,
    BridgeSendRpcOutcome, BridgeServiceDidOutcome, BridgeSessionSnapshot,
};
use awiki_cli::host_runtime::listener_identity_record::RuntimeIdentityRecord;
use serde_json::{json, Map, Value};

#[test]
fn listener_bridge_dispatch_maps_direct_and_inbox_methods_like_go() {
    let signed_record = signed_record("alice");
    let record = record("did:wba:awiki.ai:user:alice:e1_alice");

    let direct = build_bridge_rpc_call(
        &signed_record,
        "",
        &bridge_request(
            "direct.send",
            json!({
                "target": "did:wba:awiki.ai:user:bob:e1_bob",
                "text": "hello",
                "type": "event",
            }),
        ),
    )
    .expect("direct send call");
    assert_eq!(direct.method, "direct.send");
    assert_eq!(direct.params["meta"]["profile"], "anp.direct.base.v1");
    assert_eq!(
        direct.params["meta"]["target"],
        json!({ "kind": "agent", "did": "did:wba:awiki.ai:user:bob:e1_bob" })
    );
    assert_eq!(direct.params["meta"]["content_type"], "application/json");
    assert_eq!(direct.params["body"]["text"], "hello");
    assert!(direct.params["auth"]["origin_proof"].is_object());

    let inbox = build_bridge_rpc_call(
        &record,
        "",
        &bridge_request(
            "inbox.get",
            json!({
                "with": "ignored-by-go-builder",
                "limit": 5.9,
                "unread": "true",
                "mark_read": 1,
            }),
        ),
    )
    .expect("inbox call");
    assert_eq!(inbox.method, "inbox.get");
    assert_eq!(inbox.params["meta"]["profile"], "anp.inbox.local.v1");
    assert_eq!(inbox.params["body"]["user_did"], record.did);
    assert_eq!(inbox.params["body"]["limit"], 5);
    assert!(inbox.params["body"].get("with").is_none());

    let history = build_bridge_rpc_call(
        &record,
        "",
        &bridge_request(
            "direct.get_history",
            json!({
                "with": "did:wba:awiki.ai:user:bob:e1_bob",
                "limit": 6,
                "cursor": "42",
                "skip": 3.8,
            }),
        ),
    )
    .expect("history call");
    assert_eq!(history.method, "direct.get_history");
    assert_eq!(history.params["meta"]["profile"], "anp.direct.local.v1");
    assert_eq!(
        history.params["body"]["peer_did"],
        "did:wba:awiki.ai:user:bob:e1_bob"
    );
    assert_eq!(history.params["body"]["since_seq"], "42");
    assert_eq!(history.params["body"]["skip"], 3);

    let mark_read = build_bridge_rpc_call(
        &record,
        "",
        &bridge_request(
            "inbox.mark_read",
            json!({ "message_ids": ["msg-1", "", 3, "msg-2"] }),
        ),
    )
    .expect("mark read call");
    assert_eq!(mark_read.method, "inbox.mark_read");
    assert_eq!(
        mark_read.params["body"]["message_ids"],
        json!(["msg-1", "msg-2"])
    );
    assert_eq!(
        mark_read.mark_read_message_ids,
        vec!["msg-1".to_string(), "msg-2".to_string()]
    );
}

#[test]
fn listener_bridge_dispatch_maps_group_methods_like_go() {
    let record = signed_record("alice");
    let service_did = "did:wba:awiki.ai:services:message:e1_service";
    let group = "did:wba:awiki.ai:groups:demo:e1_group";

    let create = build_bridge_rpc_call(
        &record,
        service_did,
        &bridge_request(
            "group.create",
            json!({
                "name": "Bridge Group",
                "description": "from listener",
                "avatar_uri": "https://example.test/group.png",
                "discoverability": "public",
                "admission_mode": "approval",
                "slug": "bridge-group",
                "goal": "ship",
                "rules": "clear",
                "message_prompt": "brief",
                "doc_url": "https://example.test/group",
                "attachments_allowed": true,
                "max_members": "25",
                "member_max_messages": "9",
                "member_max_total_chars": 4000.8,
            }),
        ),
    )
    .expect("create group call");
    assert_eq!(create.method, "group.create");
    assert_eq!(
        create.params["meta"]["target"],
        json!({ "kind": "service", "did": service_did })
    );
    assert_eq!(
        create.params["body"]["group_profile"]["display_name"],
        "Bridge Group"
    );
    assert_eq!(
        create.params["body"]["group_profile"]["avatar_uri"],
        "https://example.test/group.png"
    );
    assert_eq!(
        create.params["body"]["group_policy"]["admission_mode"],
        "approval"
    );
    assert_eq!(
        create.params["body"]["group_policy"]["attachments_allowed"],
        true
    );
    assert_eq!(
        create.params["body"]["group_policy"]["member_max_messages"],
        9
    );
    assert_eq!(
        create.params["body"]["group_policy"]["member_max_total_chars"],
        4000
    );
    assert!(create.params["auth"]["origin_proof"].is_object());

    let info = build_bridge_rpc_call(
        &record,
        service_did,
        &bridge_request(
            "group.get_info",
            json!({ "group": group, "include_policy": "true", "include_member_list": 1 }),
        ),
    )
    .expect("group info call");
    assert_eq!(info.method, "group.get_info");
    assert_eq!(info.params["body"]["include_policy"], true);
    assert_eq!(info.params["body"]["include_member_list"], true);
    assert!(info.params.get("auth").is_none());

    let add = build_bridge_rpc_call(
        &record,
        service_did,
        &bridge_request(
            "group.add",
            json!({
                "group": group,
                "member": " did:wba:awiki.ai:user:bob:e1_bob ",
                "role": " admin ",
                "reason_text": " invite ",
            }),
        ),
    )
    .expect("group add call");
    assert_eq!(add.method, "group.add");
    assert_eq!(
        add.params["body"]["member_did"],
        "did:wba:awiki.ai:user:bob:e1_bob"
    );
    assert_eq!(add.params["body"]["role"], "admin");
    assert_eq!(add.params["body"]["reason_text"], "invite");
    assert!(add.params["auth"]["origin_proof"].is_object());

    let profile = build_bridge_rpc_call(
        &record,
        service_did,
        &bridge_request(
            "group.update_profile",
            json!({ "group": group, "patch": { "display_name": "Renamed" } }),
        ),
    )
    .expect("profile update call");
    assert_eq!(profile.method, "group.update_profile");
    assert_eq!(
        profile.params["body"]["group_profile_patch"]["display_name"],
        "Renamed"
    );

    let send = build_bridge_rpc_call(
        &record,
        service_did,
        &bridge_request(
            "group.send",
            json!({ "group": group, "text": "hello group", "type": "event" }),
        ),
    )
    .expect("group send call");
    assert_eq!(send.method, "group.send");
    assert_eq!(send.params["meta"]["content_type"], "application/json");
    assert_eq!(send.params["body"]["text"], "hello group");

    let list = build_bridge_rpc_call(
        &record,
        service_did,
        &bridge_request("group.list", json!({ "limit": -1 })),
    )
    .expect("group list call");
    assert_eq!(list.method, "group.list");
    assert_eq!(list.params["body"]["limit"], 50);

    let messages = build_bridge_rpc_call(
        &record,
        service_did,
        &bridge_request(
            "group.list_messages",
            json!({ "group": group, "limit": 8, "cursor": "7", "skip": 2 }),
        ),
    )
    .expect("group messages call");
    assert_eq!(messages.method, "group.list_messages");
    assert_eq!(messages.params["body"]["group_did"], group);
    assert_eq!(messages.params["body"]["since_seq"], "7");
    assert_eq!(messages.params["body"]["skip"], 2);
}

#[test]
fn listener_bridge_dispatch_preserves_go_error_boundaries() {
    let record = record("did:wba:awiki.ai:user:alice:e1_alice");

    let unsupported = build_bridge_rpc_call(
        &record,
        "",
        &bridge_request("group.unknown", json!({ "group": "did:group" })),
    )
    .expect_err("unsupported method");
    assert_eq!(
        unsupported.to_string(),
        "unsupported websocket bridge method: group.unknown"
    );

    let missing_ids = build_bridge_rpc_call(
        &record,
        "",
        &bridge_request("inbox.mark_read", json!({ "message_ids": [3, "", false] })),
    )
    .expect_err("empty mark read ids");
    assert!(missing_ids.to_string().contains("message_ids are required"));

    let missing_service = build_bridge_rpc_call(
        &record,
        "",
        &bridge_request("group.create", json!({ "name": "Bridge Group" })),
    )
    .expect_err("missing service did");
    assert!(missing_service
        .to_string()
        .contains("message service did is required"));
}

#[test]
fn bridge_request_flow_returns_ensure_session_error_before_current_session_reads() {
    let plan = bridge_request_flow_plan(
        &bridge_request("direct.send", json!({})),
        BridgeEnsureSessionOutcome::Error("identity missing".to_string()),
        unused_service_did(),
        unused_build_rpc(),
        unused_send_rpc(),
    );

    assert_eq!(
        plan.actions,
        vec![BridgeRequestFlowAction::EnsureSession {
            identity_name: "alice".to_string(),
        }]
    );
    assert_eq!(
        plan.decision,
        BridgeRequestFlowDecision::ReturnError("identity missing".to_string())
    );
}

#[test]
fn bridge_request_flow_requires_current_record_and_client_like_go() {
    let missing_record = bridge_request_flow_plan(
        &bridge_request("direct.send", json!({})),
        BridgeEnsureSessionOutcome::Ok(session("alice", None, true)),
        unused_service_did(),
        unused_build_rpc(),
        unused_send_rpc(),
    );

    assert_eq!(
        missing_record.actions,
        vec![
            BridgeRequestFlowAction::EnsureSession {
                identity_name: "alice".to_string(),
            },
            BridgeRequestFlowAction::ReadCurrentRecord {
                identity_name: "alice".to_string(),
            },
            BridgeRequestFlowAction::ReadCurrentClient {
                identity_name: "alice".to_string(),
            },
        ]
    );
    assert_eq!(
        missing_record.decision,
        BridgeRequestFlowDecision::ReturnError(
            "websocket session is not connected for identity alice".to_string(),
        )
    );

    let missing_client = bridge_request_flow_plan(
        &bridge_request("direct.send", json!({})),
        BridgeEnsureSessionOutcome::Ok(session("bob", Some("did:bob"), false)),
        unused_service_did(),
        unused_build_rpc(),
        unused_send_rpc(),
    );
    assert_eq!(
        missing_client.decision,
        BridgeRequestFlowDecision::ReturnError(
            "websocket session is not connected for identity bob".to_string(),
        )
    );
}

#[test]
fn bridge_request_flow_fetches_group_create_service_did_before_building_rpc() {
    let plan = bridge_request_flow_plan(
        &bridge_request("group.create", json!({ "name": "Bridge Group" })),
        BridgeEnsureSessionOutcome::Ok(session("alice", Some("did:alice"), true)),
        BridgeServiceDidOutcome::Ok {
            service_did: "did:service".to_string(),
        },
        build_rpc("group.create", &[]),
        send_rpc_ok(json!({ "group_did": "did:group" })),
    );

    assert_eq!(
        plan.actions,
        vec![
            BridgeRequestFlowAction::EnsureSession {
                identity_name: "alice".to_string(),
            },
            BridgeRequestFlowAction::ReadCurrentRecord {
                identity_name: "alice".to_string(),
            },
            BridgeRequestFlowAction::ReadCurrentClient {
                identity_name: "alice".to_string(),
            },
            BridgeRequestFlowAction::FetchMessageServiceDID {
                identity_name: "alice".to_string(),
            },
            BridgeRequestFlowAction::BuildRpcCall {
                method: "group.create".to_string(),
                service_did: Some("did:service".to_string()),
            },
            BridgeRequestFlowAction::SendRpc {
                method: "group.create".to_string(),
            },
        ]
    );
    assert_eq!(
        plan.decision,
        BridgeRequestFlowDecision::ReturnOk {
            result: object_map(json!({ "group_did": "did:group" })),
        }
    );
}

#[test]
fn bridge_request_flow_returns_group_create_service_did_error_before_building_rpc() {
    let plan = bridge_request_flow_plan(
        &bridge_request("group.create", json!({ "name": "Bridge Group" })),
        BridgeEnsureSessionOutcome::Ok(session("alice", Some("did:alice"), true)),
        BridgeServiceDidOutcome::Error("capabilities missing service did".to_string()),
        unused_build_rpc(),
        unused_send_rpc(),
    );

    assert_eq!(
        plan.actions,
        vec![
            BridgeRequestFlowAction::EnsureSession {
                identity_name: "alice".to_string(),
            },
            BridgeRequestFlowAction::ReadCurrentRecord {
                identity_name: "alice".to_string(),
            },
            BridgeRequestFlowAction::ReadCurrentClient {
                identity_name: "alice".to_string(),
            },
            BridgeRequestFlowAction::FetchMessageServiceDID {
                identity_name: "alice".to_string(),
            },
        ]
    );
    assert_eq!(
        plan.decision,
        BridgeRequestFlowDecision::ReturnError("capabilities missing service did".to_string())
    );
}

#[test]
fn bridge_request_flow_returns_build_or_send_errors_before_later_side_effects() {
    let build_error = bridge_request_flow_plan(
        &bridge_request("group.unknown", json!({})),
        BridgeEnsureSessionOutcome::Ok(session("alice", Some("did:alice"), true)),
        unused_service_did(),
        BridgeRpcBuildOutcome::Error(
            "unsupported websocket bridge method: group.unknown".to_string(),
        ),
        unused_send_rpc(),
    );

    assert_eq!(
        build_error.actions,
        vec![
            BridgeRequestFlowAction::EnsureSession {
                identity_name: "alice".to_string(),
            },
            BridgeRequestFlowAction::ReadCurrentRecord {
                identity_name: "alice".to_string(),
            },
            BridgeRequestFlowAction::ReadCurrentClient {
                identity_name: "alice".to_string(),
            },
            BridgeRequestFlowAction::BuildRpcCall {
                method: "group.unknown".to_string(),
                service_did: None,
            },
        ]
    );
    assert_eq!(
        build_error.decision,
        BridgeRequestFlowDecision::ReturnError(
            "unsupported websocket bridge method: group.unknown".to_string(),
        )
    );

    let send_error = bridge_request_flow_plan(
        &bridge_request("inbox.mark_read", json!({ "message_ids": ["msg-1"] })),
        BridgeEnsureSessionOutcome::Ok(session("alice", Some("did:alice"), true)),
        unused_service_did(),
        build_rpc("inbox.mark_read", &["msg-1"]),
        BridgeSendRpcOutcome::Error("rpc failed".to_string()),
    );
    assert_eq!(
        send_error.actions,
        vec![
            BridgeRequestFlowAction::EnsureSession {
                identity_name: "alice".to_string(),
            },
            BridgeRequestFlowAction::ReadCurrentRecord {
                identity_name: "alice".to_string(),
            },
            BridgeRequestFlowAction::ReadCurrentClient {
                identity_name: "alice".to_string(),
            },
            BridgeRequestFlowAction::BuildRpcCall {
                method: "inbox.mark_read".to_string(),
                service_did: None,
            },
            BridgeRequestFlowAction::SendRpc {
                method: "inbox.mark_read".to_string(),
            },
        ]
    );
    assert_eq!(
        send_error.decision,
        BridgeRequestFlowDecision::ReturnError("rpc failed".to_string())
    );
}

#[test]
fn bridge_request_flow_marks_messages_read_only_after_successful_mark_read_rpc() {
    let plan = bridge_request_flow_plan(
        &bridge_request(
            "inbox.mark_read",
            json!({ "message_ids": ["msg-1", "msg-2"] }),
        ),
        BridgeEnsureSessionOutcome::Ok(session("alice", Some("did:alice"), true)),
        unused_service_did(),
        build_rpc("inbox.mark_read", &["msg-1", "msg-2"]),
        send_rpc_ok(json!({ "updated": 2 })),
    );

    assert_eq!(
        plan.actions,
        vec![
            BridgeRequestFlowAction::EnsureSession {
                identity_name: "alice".to_string(),
            },
            BridgeRequestFlowAction::ReadCurrentRecord {
                identity_name: "alice".to_string(),
            },
            BridgeRequestFlowAction::ReadCurrentClient {
                identity_name: "alice".to_string(),
            },
            BridgeRequestFlowAction::BuildRpcCall {
                method: "inbox.mark_read".to_string(),
                service_did: None,
            },
            BridgeRequestFlowAction::SendRpc {
                method: "inbox.mark_read".to_string(),
            },
            BridgeRequestFlowAction::MarkMessagesRead {
                owner_did: "did:alice".to_string(),
                message_ids: vec!["msg-1".to_string(), "msg-2".to_string()],
            },
        ]
    );
    assert_eq!(
        plan.decision,
        BridgeRequestFlowDecision::ReturnOk {
            result: object_map(json!({ "updated": 2 })),
        }
    );
}

#[test]
fn bridge_request_flow_success_without_mark_read_returns_rpc_result_only() {
    let plan = bridge_request_flow_plan(
        &bridge_request("direct.send", json!({ "text": "hello" })),
        BridgeEnsureSessionOutcome::Ok(session("alice", Some("did:alice"), true)),
        unused_service_did(),
        build_rpc("direct.send", &[]),
        send_rpc_ok(json!({ "message_id": "msg-1" })),
    );

    assert_eq!(
        plan.actions,
        vec![
            BridgeRequestFlowAction::EnsureSession {
                identity_name: "alice".to_string(),
            },
            BridgeRequestFlowAction::ReadCurrentRecord {
                identity_name: "alice".to_string(),
            },
            BridgeRequestFlowAction::ReadCurrentClient {
                identity_name: "alice".to_string(),
            },
            BridgeRequestFlowAction::BuildRpcCall {
                method: "direct.send".to_string(),
                service_did: None,
            },
            BridgeRequestFlowAction::SendRpc {
                method: "direct.send".to_string(),
            },
        ]
    );
    assert_eq!(
        plan.decision,
        BridgeRequestFlowDecision::ReturnOk {
            result: object_map(json!({ "message_id": "msg-1" })),
        }
    );
}

fn bridge_request(method: &str, params: Value) -> BridgeRequest {
    BridgeRequest {
        method: method.to_string(),
        params: object_map(params),
        identity_name: "alice".to_string(),
    }
}

fn record(did: &str) -> RuntimeIdentityRecord {
    RuntimeIdentityRecord {
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
            challenge: Some("runtime-bridge-dispatch-contract".to_string()),
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

fn session(
    identity_name: &str,
    record_did: Option<&str>,
    has_client: bool,
) -> BridgeSessionSnapshot {
    BridgeSessionSnapshot {
        identity_name: identity_name.to_string(),
        record_did: record_did.map(str::to_string),
        has_client,
    }
}

fn unused_service_did() -> BridgeServiceDidOutcome {
    BridgeServiceDidOutcome::Error("service did should not be used".to_string())
}

fn unused_build_rpc() -> BridgeRpcBuildOutcome {
    BridgeRpcBuildOutcome::Error("build rpc should not be used".to_string())
}

fn unused_send_rpc() -> BridgeSendRpcOutcome {
    BridgeSendRpcOutcome::Error("send rpc should not be used".to_string())
}

fn build_rpc(method: &str, mark_read_message_ids: &[&str]) -> BridgeRpcBuildOutcome {
    BridgeRpcBuildOutcome::Ok {
        method: method.to_string(),
        mark_read_message_ids: mark_read_message_ids
            .iter()
            .map(|message_id| (*message_id).to_string())
            .collect(),
    }
}

fn send_rpc_ok(result: Value) -> BridgeSendRpcOutcome {
    BridgeSendRpcOutcome::Ok {
        result: object_map(result),
    }
}

fn object_map(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}
