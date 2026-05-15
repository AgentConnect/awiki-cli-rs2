use awiki_cli::identity::generate_identity;
use awiki_cli::identity::types::{GeneratedIdentity, StoredIdentity};
use awiki_cli::runtime::bridge::BridgeRequest;
use awiki_cli::runtime::listener_bridge_dispatch::build_bridge_rpc_call;
use serde_json::{json, Map, Value};

#[test]
fn listener_bridge_dispatch_maps_direct_and_inbox_methods_like_go() {
    let generated =
        generate_identity("awiki.ai", "", "").expect("generated identity should be valid");
    let signed_record = generated_record("alice", &generated);
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
    let generated =
        generate_identity("awiki.ai", "", "").expect("generated identity should be valid");
    let record = generated_record("alice", &generated);
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

fn bridge_request(method: &str, params: Value) -> BridgeRequest {
    BridgeRequest {
        method: method.to_string(),
        params: object_map(params),
        identity_name: "alice".to_string(),
    }
}

fn record(did: &str) -> StoredIdentity {
    StoredIdentity {
        did: did.to_string(),
        ..StoredIdentity::default()
    }
}

fn generated_record(identity_name: &str, generated: &GeneratedIdentity) -> StoredIdentity {
    StoredIdentity {
        identity_name: identity_name.to_string(),
        did: generated.did.clone(),
        unique_id: generated.unique_id.clone(),
        did_document: Some(generated.did_document.clone()),
        key1_private_pem: generated.key1_private_pem.clone(),
        key1_public_pem: generated.key1_public_pem.clone(),
        ..StoredIdentity::default()
    }
}

fn object_map(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}
