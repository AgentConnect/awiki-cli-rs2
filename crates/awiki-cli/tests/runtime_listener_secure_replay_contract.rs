use awiki_cli::runtime::listener_secure_replay::{
    secure_pending_history_replay_candidates, secure_unread_replay_candidates, ReplayStoreLookup,
};
use serde_json::{json, Value};

#[test]
fn unread_replay_filters_secure_missing_messages_and_preserves_order() {
    let messages = vec![
        json!("not-object"),
        message("plain", "text/plain", "did:bob", "did:alice"),
        message(
            "existing",
            "application/anp-direct-init+json",
            "did:bob",
            "did:alice",
        ),
        message(
            "store-error",
            "application/anp-direct-cipher+json",
            "did:bob",
            "did:alice",
        ),
        message(
            "missing-1",
            "application/anp-direct-init+json",
            "did:bob",
            "did:alice",
        ),
        message(
            "missing-2",
            "application/anp-direct-cipher+json",
            "did:carol",
            "did:alice",
        ),
    ];
    let mut lookups = Vec::new();

    let candidates = secure_unread_replay_candidates(
        &messages,
        "did:alice",
        "alice",
        |id, owner, credential| {
            lookups.push((id.to_string(), owner.to_string(), credential.to_string()));
            match id {
                "existing" => ReplayStoreLookup::Exists,
                "store-error" => ReplayStoreLookup::Error,
                _ => ReplayStoreLookup::Missing,
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
                "store-error".to_string(),
                "did:alice".to_string(),
                "alice".to_string()
            ),
            (
                "missing-1".to_string(),
                "did:alice".to_string(),
                "alice".to_string()
            ),
            (
                "missing-2".to_string(),
                "did:alice".to_string(),
                "alice".to_string()
            ),
        ]
    );
    assert_eq!(
        candidates
            .iter()
            .map(|candidate| candidate.message_id.as_str())
            .collect::<Vec<_>>(),
        vec!["missing-1", "missing-2"]
    );
    assert_eq!(candidates[0].notification["method"], "direct.incoming");
    assert_eq!(
        candidates[1].notification["params"]["meta"]["sender_did"],
        "did:carol"
    );
}

#[test]
fn unread_replay_uses_session_did_for_lookup_then_skips_missing_receiver_notification() {
    let messages = vec![message(
        "missing-receiver",
        "application/anp-direct-cipher+json",
        "did:bob",
        "",
    )];
    let mut lookups = Vec::new();

    let candidates = secure_unread_replay_candidates(
        &messages,
        "did:alice",
        "alice",
        |id, owner, credential| {
            lookups.push((id.to_string(), owner.to_string(), credential.to_string()));
            ReplayStoreLookup::Missing
        },
    );

    assert_eq!(
        lookups,
        vec![(
            "missing-receiver".to_string(),
            "did:alice".to_string(),
            "alice".to_string()
        )]
    );
    assert!(
        candidates.is_empty(),
        "Go uses owner fallback only for store lookup; notification conversion still requires receiver_did"
    );
}

#[test]
fn replay_skips_malformed_secure_message_views_after_store_lookup() {
    let messages = vec![json!({
        "id": "bad-secure",
        "sender_did": "did:bob",
        "receiver_did": "did:alice",
        "content_type": "application/anp-direct-init+json",
        "content": "not-object"
    })];
    let mut lookups = Vec::new();

    let candidates = secure_unread_replay_candidates(
        &messages,
        "did:alice",
        "alice",
        |id, owner, credential| {
            lookups.push((id.to_string(), owner.to_string(), credential.to_string()));
            ReplayStoreLookup::Missing
        },
    );

    assert_eq!(
        lookups,
        vec![(
            "bad-secure".to_string(),
            "did:alice".to_string(),
            "alice".to_string()
        )]
    );
    assert!(candidates.is_empty());
}

#[test]
fn pending_history_replay_skips_self_sent_secure_messages_before_store_lookup() {
    let messages = vec![
        message(
            "self-sent",
            "application/anp-direct-init+json",
            "did:alice",
            "did:bob",
        ),
        message(
            "peer-sent",
            "application/anp-direct-cipher+json",
            "did:bob",
            "did:alice",
        ),
    ];
    let mut lookups = Vec::new();

    let candidates = secure_pending_history_replay_candidates(
        &messages,
        "did:alice",
        "alice",
        |id, owner, credential| {
            lookups.push((id.to_string(), owner.to_string(), credential.to_string()));
            ReplayStoreLookup::Missing
        },
    );

    assert_eq!(
        lookups,
        vec![(
            "peer-sent".to_string(),
            "did:alice".to_string(),
            "alice".to_string()
        )]
    );
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].message_id, "peer-sent");
}

#[test]
fn pending_history_replay_uses_same_secure_type_and_store_skip_rules() {
    let messages = vec![
        message("plain", "text/plain", "did:bob", "did:alice"),
        message(
            "existing",
            "application/anp-direct-cipher+json",
            "did:bob",
            "did:alice",
        ),
        message(
            "missing",
            "application/anp-direct-init+json",
            "did:bob",
            "did:alice",
        ),
    ];

    let candidates =
        secure_pending_history_replay_candidates(&messages, "did:alice", "alice", |id, _, _| {
            if id == "existing" {
                ReplayStoreLookup::Exists
            } else {
                ReplayStoreLookup::Missing
            }
        });

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].message_id, "missing");
    assert_eq!(
        candidates[0].notification["params"]["meta"]["content_type"],
        "application/anp-direct-init+json"
    );
}

fn message(id: &str, content_type: &str, sender_did: &str, receiver_did: &str) -> Value {
    json!({
        "id": id,
        "sender_did": sender_did,
        "receiver_did": receiver_did,
        "content_type": content_type,
        "server_seq": 7,
        "content": {
            "session_id": "sess-001",
            "ciphertext": format!("cipher-{id}"),
        }
    })
}
