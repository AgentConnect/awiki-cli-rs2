use awiki_cli::identity::types::StoredIdentity;
use awiki_cli::runtime::listener_secure_replay::ReplayStoreLookup;
use awiki_cli::runtime::listener_secure_sync::{
    sync_pending_confirmation_secure_history_plan, sync_unread_secure_direct_inbox_plan,
    SecureSyncAction, SECURE_DIRECT_SYNC_TIMEOUT, SECURE_PENDING_HISTORY_LIMIT,
    SECURE_UNREAD_INBOX_LIMIT,
};
use serde_json::{json, Value};
use std::time::Duration;

#[test]
fn exported_secure_sync_constants_match_go() {
    assert_eq!(SECURE_DIRECT_SYNC_TIMEOUT, Duration::from_secs(15));
    assert_eq!(SECURE_UNREAD_INBOX_LIMIT, 100);
    assert_eq!(SECURE_PENDING_HISTORY_LIMIT, 50);
}

#[test]
fn unread_sync_skips_when_current_record_is_missing() {
    let plan = sync_unread_secure_direct_inbox_plan(None, Some(&json!({})), |_, _, _| {
        ReplayStoreLookup::Missing
    });

    assert!(plan.actions.is_empty());
}

#[test]
fn unread_sync_sends_inbox_get_with_go_direct_unread_request_shape() {
    let record = record();
    let plan = sync_unread_secure_direct_inbox_plan(Some(&record), None, |_, _, _| {
        panic!("store lookup should not run without RPC result")
    });

    assert_eq!(plan.actions.len(), 2);
    let SecureSyncAction::SendRpc(call) = &plan.actions[0] else {
        panic!("expected SendRpc");
    };
    assert_eq!(call.method, "inbox.get");
    assert_eq!(call.timeout, Duration::from_secs(15));
    assert_eq!(call.params["meta"]["profile"], "anp.inbox.local.v1");
    assert_eq!(call.params["meta"]["sender_did"], record.did);
    assert_eq!(call.params["body"]["user_did"], record.did);
    assert_eq!(call.params["body"]["limit"], 100);
    assert!(call.params["body"].get("scope").is_none());
    assert!(call.params["body"].get("unread_only").is_none());
    assert_eq!(plan.actions[1], SecureSyncAction::CancelRpcContext);
}

#[test]
fn unread_sync_replays_secure_missing_messages_after_rpc_success() {
    let record = record();
    let rpc_result = json!({
        "messages": [
            secure_message("plain", "text/plain", "did:bob", "did:alice"),
            secure_message("existing", "application/anp-direct-cipher+json", "did:bob", "did:alice"),
            secure_message("missing", "application/anp-direct-init+json", "did:bob", "did:alice")
        ]
    });
    let mut lookups = Vec::new();

    let plan = sync_unread_secure_direct_inbox_plan(
        Some(&record),
        Some(&rpc_result),
        |id, owner, credential| {
            lookups.push((id.to_string(), owner.to_string(), credential.to_string()));
            if id == "existing" {
                ReplayStoreLookup::Exists
            } else {
                ReplayStoreLookup::Missing
            }
        },
    );

    assert_eq!(
        lookups,
        vec![
            (
                "existing".to_string(),
                "did:alice".to_string(),
                "alice".to_string()
            ),
            (
                "missing".to_string(),
                "did:alice".to_string(),
                "alice".to_string()
            ),
        ]
    );
    assert_eq!(plan.actions.len(), 3);
    let SecureSyncAction::HandleNotification { notification } = &plan.actions[1] else {
        panic!("expected replay notification");
    };
    assert_eq!(notification["method"], "direct.incoming");
    assert_eq!(notification["params"]["meta"]["message_id"], "missing");
    assert_eq!(plan.actions[2], SecureSyncAction::CancelRpcContext);
}

#[test]
fn pending_history_skips_when_record_or_peers_missing() {
    let record = record();

    assert!(sync_pending_confirmation_secure_history_plan(
        None,
        &["did:bob".to_string()],
        &[],
        |_, _, _| ReplayStoreLookup::Missing
    )
    .actions
    .is_empty());
    assert!(
        sync_pending_confirmation_secure_history_plan(Some(&record), &[], &[], |_, _, _| {
            ReplayStoreLookup::Missing
        })
        .actions
        .is_empty()
    );
}

#[test]
fn pending_history_build_error_cancels_context_and_continues() {
    let record = record();
    let peers = vec!["".to_string(), "did:bob".to_string()];

    let plan = sync_pending_confirmation_secure_history_plan(
        Some(&record),
        &peers,
        &[None, None],
        |_, _, _| ReplayStoreLookup::Missing,
    );

    assert_eq!(plan.actions.len(), 3);
    assert_eq!(plan.actions[0], SecureSyncAction::CancelRpcContext);
    let SecureSyncAction::SendRpc(call) = &plan.actions[1] else {
        panic!("expected second peer RPC");
    };
    assert_eq!(call.method, "direct.get_history");
    assert_eq!(call.timeout, Duration::from_secs(15));
    assert_eq!(call.params["body"]["peer_did"], "did:bob");
    assert_eq!(call.params["body"]["limit"], 50);
    assert_eq!(plan.actions[2], SecureSyncAction::CancelRpcContext);
}

#[test]
fn pending_history_replays_rpc_success_messages_after_cancel_action() {
    let record = record();
    let peers = vec!["did:bob".to_string()];
    let result = json!({
        "messages": [
            secure_message("self", "application/anp-direct-init+json", "did:alice", "did:bob"),
            secure_message("missing", "application/anp-direct-cipher+json", "did:bob", "did:alice")
        ]
    });
    let mut lookups = Vec::new();

    let plan = sync_pending_confirmation_secure_history_plan(
        Some(&record),
        &peers,
        &[Some(result)],
        |id, owner, credential| {
            lookups.push((id.to_string(), owner.to_string(), credential.to_string()));
            ReplayStoreLookup::Missing
        },
    );

    assert_eq!(
        lookups,
        vec![(
            "missing".to_string(),
            "did:alice".to_string(),
            "alice".to_string()
        )]
    );
    assert_eq!(plan.actions.len(), 3);
    assert!(matches!(plan.actions[0], SecureSyncAction::SendRpc(_)));
    assert_eq!(plan.actions[1], SecureSyncAction::CancelRpcContext);
    let SecureSyncAction::HandleNotification { notification } = &plan.actions[2] else {
        panic!("expected replay notification");
    };
    assert_eq!(notification["params"]["meta"]["message_id"], "missing");
}

fn record() -> StoredIdentity {
    StoredIdentity {
        identity_name: "alice".to_string(),
        did: "did:alice".to_string(),
        ..StoredIdentity::default()
    }
}

fn secure_message(id: &str, content_type: &str, sender: &str, receiver: &str) -> Value {
    json!({
        "id": id,
        "sender_did": sender,
        "receiver_did": receiver,
        "content_type": content_type,
        "content": {"ciphertext": "abc"},
    })
}
