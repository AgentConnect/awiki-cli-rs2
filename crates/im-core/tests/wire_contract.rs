use im_core::compat;
use im_core::prelude::*;
use serde_json::json;

#[test]
fn wire_content_type_for_message_kind_matches_p1_contract() {
    assert_eq!(
        compat::wire::content_type_for_message_kind(MessageKind::Text, None),
        "text/plain"
    );
    assert_eq!(
        compat::wire::content_type_for_message_kind(MessageKind::Markdown, None),
        "text/markdown"
    );
    assert_eq!(
        compat::wire::content_type_for_message_kind(MessageKind::Text, Some("event")),
        "application/json"
    );
    assert_eq!(
        compat::wire::content_type_for_message_kind(MessageKind::Text, Some("attachment_manifest")),
        "application/anp-attachment-manifest+json"
    );
}

#[test]
fn wire_operation_id_is_lower_hex_without_prefix() {
    let first = compat::wire::generate_operation_id();
    let second = compat::wire::generate_operation_id();

    assert_eq!(first.len(), 16);
    assert_eq!(second.len(), 16);
    assert_ne!(first, second);
    assert!(first.chars().all(|ch| ch.is_ascii_hexdigit()));
    assert!(first.chars().all(|ch| !ch.is_ascii_uppercase()));
}

#[test]
fn wire_now_rfc3339_uses_second_precision_utc_shape() {
    let value = compat::wire::now_rfc3339();
    let bytes = value.as_bytes();

    assert_eq!(bytes.len(), 20);
    assert_eq!(bytes[4], b'-');
    assert_eq!(bytes[7], b'-');
    assert_eq!(bytes[10], b'T');
    assert_eq!(bytes[13], b':');
    assert_eq!(bytes[16], b':');
    assert_eq!(bytes[19], b'Z');
    assert!(bytes
        .iter()
        .enumerate()
        .filter(|(index, _)| !matches!(index, 4 | 7 | 10 | 13 | 16 | 19))
        .all(|(_, byte)| byte.is_ascii_digit()));
}

#[test]
fn wire_direct_text_payload_matches_go_contract() {
    let payload = compat::wire::build_direct_text_payload(
        "did:wba:awiki.ai:user:alice:e1",
        "did:wba:awiki.ai:user:bob:e1",
        "hello",
        "text/plain",
    )
    .expect("direct payload");

    assert_eq!(payload.method, "direct.send");
    assert_eq!(payload.meta["profile"], "anp.direct.base.v1");
    assert_eq!(payload.meta["security_profile"], "transport-protected");
    assert_eq!(
        payload.meta["target"],
        json!({
            "kind": "agent",
            "did": "did:wba:awiki.ai:user:bob:e1",
        })
    );
    assert_eq!(payload.body, json!({ "text": "hello" }));
    assert_has_generated_meta(&payload.meta);
    assert!(payload.meta["message_id"]
        .as_str()
        .expect("message id")
        .starts_with("msg-"));
}

#[test]
fn wire_inbox_history_and_mark_read_params_match_go_contracts() {
    let identity = compat::wire::WireIdentity {
        did: "did:wba:awiki.ai:user:alice:e1_alice".to_string(),
    };

    let inbox = compat::wire::build_inbox_rpc_params(
        &identity,
        compat::wire::InboxWireRequest {
            limit: 0,
            auth: None,
        },
    );
    assert_eq!(inbox["meta"]["profile"], "anp.inbox.local.v1");
    assert_eq!(inbox["meta"]["security_profile"], "transport-protected");
    assert_eq!(inbox["meta"]["sender_did"], identity.did);
    assert_has_generated_meta(&inbox["meta"]);
    assert_eq!(inbox["body"]["user_did"], identity.did);
    assert_eq!(inbox["body"]["limit"], 20);

    let history = compat::wire::build_history_rpc_params(
        &identity,
        compat::wire::HistoryWireRequest {
            peer_did: "did:wba:awiki.ai:user:bob:e1_bob".to_string(),
            limit: 0,
            cursor: Some("42".to_string()),
            skip: 3,
            auth: None,
        },
    )
    .expect("history params");
    assert_eq!(history["meta"]["profile"], "anp.direct.local.v1");
    assert_eq!(
        history["body"]["peer_did"],
        "did:wba:awiki.ai:user:bob:e1_bob"
    );
    assert_eq!(history["body"]["limit"], 50);
    assert_eq!(history["body"]["since_seq"], "42");
    assert_eq!(history["body"]["skip"], 3);

    assert!(compat::wire::build_mark_read_rpc_params(
        &identity,
        compat::wire::MarkReadWireRequest {
            message_ids: Vec::new(),
        }
    )
    .is_err());
    let mark_read = compat::wire::build_mark_read_rpc_params(
        &identity,
        compat::wire::MarkReadWireRequest {
            message_ids: vec!["msg-1".to_string(), "msg-2".to_string()],
        },
    )
    .expect("mark-read params");
    assert_eq!(mark_read["meta"]["profile"], "anp.inbox.local.v1");
    assert_eq!(mark_read["body"]["message_ids"], json!(["msg-1", "msg-2"]));
}

#[test]
fn wire_delegated_inbox_history_params_include_inbox_auth_fields() {
    let identity = compat::wire::WireIdentity {
        did: "did:wba:awiki.ai:agent:daemon:e1_daemon".to_string(),
    };
    let inbox_owner_did = "did:wba:awiki.ai:user:alice:e1_alice".to_string();
    let inbox_auth_verification_method =
        "did:wba:awiki.ai:user:alice:e1_alice#daemon-key-1".to_string();

    let inbox = compat::wire::build_inbox_rpc_params(
        &identity,
        compat::wire::InboxWireRequest {
            limit: 7,
            auth: Some(compat::wire::InboxWireAuth {
                inbox_owner_did: inbox_owner_did.clone(),
                inbox_auth_verification_method: inbox_auth_verification_method.clone(),
            }),
        },
    );
    assert_eq!(inbox["meta"]["sender_did"], inbox_owner_did);
    assert_eq!(
        inbox["meta"]["target"],
        json!({"kind": "service", "did": "did:awiki:message-service"})
    );
    assert_eq!(inbox["body"]["user_did"], inbox_owner_did);
    assert_eq!(inbox["body"]["inbox_owner_did"], inbox_owner_did);
    assert_eq!(
        inbox["body"]["inbox_auth_verification_method"],
        inbox_auth_verification_method
    );

    let history = compat::wire::build_history_rpc_params(
        &identity,
        compat::wire::HistoryWireRequest {
            peer_did: "did:wba:awiki.ai:user:bob:e1_bob".to_string(),
            limit: 3,
            cursor: None,
            skip: 0,
            auth: Some(compat::wire::HistoryWireAuth {
                inbox_owner_did: inbox_owner_did.clone(),
                inbox_auth_verification_method: inbox_auth_verification_method.clone(),
            }),
        },
    )
    .expect("delegated history params");
    assert_eq!(history["meta"]["sender_did"], inbox_owner_did);
    assert_eq!(
        history["meta"]["target"],
        json!({"kind": "service", "did": "did:awiki:message-service"})
    );
    assert_eq!(history["body"]["user_did"], inbox_owner_did);
    assert_eq!(history["body"]["inbox_owner_did"], inbox_owner_did);
    assert_eq!(
        history["body"]["inbox_auth_verification_method"],
        inbox_auth_verification_method
    );
}

fn assert_has_generated_meta(meta: &serde_json::Value) {
    assert_eq!(meta["anp_version"], "1.0");
    assert!(meta["operation_id"]
        .as_str()
        .expect("operation id")
        .starts_with("op-"));
    assert_eq!(meta["created_at"].as_str().expect("created at").len(), 20);
}
