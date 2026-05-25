use awiki_cli::identity::types::StoredIdentity;
use awiki_cli::runtime_legacy::listener_secure_outbox_flush::{
    flush_peer_queued_secure_outbox_plan, SecureOutboxFlushAction, SecureOutboxFlushSession,
};

#[test]
fn missing_owner_session_plans_no_actions() {
    let sessions = vec![
        session("empty", None, true),
        session("bob", Some(record("bob", "did:bob")), true),
    ];

    let plan = flush_peer_queued_secure_outbox_plan(&sessions, "did:alice", "did:bob", |_, _| {
        panic!("flush should not run without matching owner session")
    });

    assert!(plan.actions.is_empty());
}

#[test]
fn matching_session_without_secure_rpc_returns_without_later_matches() {
    let sessions = vec![
        session("first", Some(record("first", "did:alice")), false),
        session("second", Some(record("second", "did:alice")), true),
    ];

    let plan = flush_peer_queued_secure_outbox_plan(&sessions, "did:alice", "did:bob", |_, _| {
        panic!("Go returns on the first owner match when secureRPC is nil")
    });

    assert!(plan.actions.is_empty());
}

#[test]
fn first_matching_owner_with_secure_rpc_flushes_and_logs_warnings() {
    let sessions = vec![
        session("ignored", Some(record("ignored", "did:other")), true),
        session("alice", Some(record("alice", "did:alice")), true),
        session(
            "alice-later",
            Some(record("alice-later", "did:alice")),
            true,
        ),
    ];
    let mut calls = Vec::new();

    let plan =
        flush_peer_queued_secure_outbox_plan(&sessions, "did:alice", "did:bob", |record, peer| {
            calls.push((
                record.identity_name.clone(),
                record.did.clone(),
                peer.to_string(),
            ));
            vec!["retry warning".to_string()]
        });

    assert_eq!(
        calls,
        vec![(
            "alice".to_string(),
            "did:alice".to_string(),
            "did:bob".to_string()
        )]
    );
    assert_eq!(
        plan.actions,
        vec![
            SecureOutboxFlushAction::FlushQueuedSecureOutbox {
                owner_did: "did:alice".to_string(),
                peer_did: "did:bob".to_string(),
                identity_name: "alice".to_string(),
            },
            SecureOutboxFlushAction::LogQueuedSecureOutboxFlush {
                owner_did: "did:alice".to_string(),
                peer_did: "did:bob".to_string(),
                warnings: vec!["retry warning".to_string()],
            },
        ]
    );
}

#[test]
fn owner_and_peer_dids_use_exact_go_string_matching_without_trim() {
    let sessions = vec![session("alice", Some(record("alice", " did:alice ")), true)];

    assert!(
        flush_peer_queued_secure_outbox_plan(&sessions, "did:alice", " did:bob ", |_, _| {
            panic!("trimmed owner DID should not match")
        })
        .actions
        .is_empty()
    );

    let plan =
        flush_peer_queued_secure_outbox_plan(&sessions, " did:alice ", " did:bob ", |_, peer| {
            assert_eq!(peer, " did:bob ");
            Vec::new()
        });

    assert_eq!(
        plan.actions,
        vec![
            SecureOutboxFlushAction::FlushQueuedSecureOutbox {
                owner_did: " did:alice ".to_string(),
                peer_did: " did:bob ".to_string(),
                identity_name: "alice".to_string(),
            },
            SecureOutboxFlushAction::LogQueuedSecureOutboxFlush {
                owner_did: " did:alice ".to_string(),
                peer_did: " did:bob ".to_string(),
                warnings: Vec::new(),
            },
        ]
    );
}

fn session(
    identity_name: &str,
    current_record: Option<StoredIdentity>,
    secure_rpc_available: bool,
) -> SecureOutboxFlushSession {
    SecureOutboxFlushSession {
        identity_name: identity_name.to_string(),
        current_record,
        secure_rpc_available,
    }
}

fn record(identity_name: &str, did: &str) -> StoredIdentity {
    StoredIdentity {
        identity_name: identity_name.to_string(),
        did: did.to_string(),
        ..StoredIdentity::default()
    }
}
