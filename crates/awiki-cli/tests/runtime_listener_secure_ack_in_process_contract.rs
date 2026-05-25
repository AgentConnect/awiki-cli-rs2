use awiki_cli::legacy_identity::types::StoredIdentity;
use awiki_cli::runtime_legacy::listener_secure_ack_in_process::{
    deliver_local_secure_ack_in_process_plan, ActiveRecipientSessionOutcome,
    EncryptFollowUpOutcome, LocalSecureAckInProcessAction, LocalSecureAckInProcessDecision,
    LocalSecureAckInProcessOutcomes, LocalSecureAckInProcessSkipReason, ProcessIncomingOutcome,
    SenderSessionLookupOutcome,
};
use serde_json::json;

#[test]
fn missing_sender_record_skips_before_recipient_lookup() {
    let plan = deliver_local_secure_ack_in_process_plan(
        None,
        Some(&record("alice", "did:alice")),
        "did:alice",
        "sess-1",
        "msg-1",
        "ack-sess-1",
        LocalSecureAckInProcessOutcomes::default(),
    );

    assert_eq!(plan.decision, LocalSecureAckInProcessDecision::Skipped);
    assert_eq!(
        plan.actions,
        vec![LocalSecureAckInProcessAction::LogSkipped {
            reason: LocalSecureAckInProcessSkipReason::SenderRecordMissing,
        }]
    );
}

#[test]
fn missing_recipient_record_skips_after_lookup_like_go() {
    let bob = record("bob", "did:bob");
    let plan = deliver_local_secure_ack_in_process_plan(
        Some(&bob),
        None,
        "did:alice",
        "sess-1",
        "msg-1",
        "ack-sess-1",
        LocalSecureAckInProcessOutcomes::default(),
    );

    assert_eq!(plan.decision, LocalSecureAckInProcessDecision::Skipped);
    assert_eq!(
        plan.actions,
        vec![
            LocalSecureAckInProcessAction::LookupRecipientRecord {
                recipient_did: "did:alice".to_string(),
            },
            LocalSecureAckInProcessAction::LogSkipped {
                reason: LocalSecureAckInProcessSkipReason::RecipientNotManaged,
            },
        ]
    );
}

#[test]
fn sender_setup_and_encrypt_failures_stop_in_go_order() {
    let cases = [
        (
            outcomes(|o| o.sender_paths_ok = false),
            LocalSecureAckInProcessSkipReason::SenderPathsError,
        ),
        (
            outcomes(|o| o.sender_store_ok = false),
            LocalSecureAckInProcessSkipReason::SenderSessionStoreError,
        ),
        (
            outcomes(|o| o.sender_session_lookup = SenderSessionLookupOutcome::Missing),
            LocalSecureAckInProcessSkipReason::SenderSessionLookupFailed,
        ),
        (
            outcomes(|o| o.sender_session_lookup = SenderSessionLookupOutcome::Error),
            LocalSecureAckInProcessSkipReason::SenderSessionLookupFailed,
        ),
        (
            outcomes(|o| o.encrypt_follow_up = EncryptFollowUpOutcome::Error),
            LocalSecureAckInProcessSkipReason::EncryptFollowUpError,
        ),
    ];

    for (outcomes, reason) in cases {
        let plan = plan(outcomes);

        assert_eq!(plan.decision, LocalSecureAckInProcessDecision::Skipped);
        assert_eq!(
            plan.actions.last(),
            Some(&LocalSecureAckInProcessAction::LogSkipped { reason })
        );
    }
}

#[test]
fn encrypted_ack_notification_matches_go_shape_and_payload() {
    let plan = plan(outcomes(|o| {
        o.active_recipient_session = ActiveRecipientSessionOutcome::Present {
            secure_rpc_available: false,
            flush_warnings: Vec::new(),
        };
    }));

    let LocalSecureAckInProcessAction::EncryptFollowUp {
        sender_did,
        recipient_did,
        message_id,
        payload,
    } = &plan.actions[4]
    else {
        panic!("expected EncryptFollowUp action");
    };
    assert_eq!(sender_did, "did:bob");
    assert_eq!(recipient_did, "did:alice");
    assert_eq!(message_id, "ack-sess-1");
    assert_eq!(
        payload,
        &im_core::secure::build_secure_ack_payload(" sess-1 ", " msg-1\n")
    );

    let LocalSecureAckInProcessAction::ProcessIncoming { notification } = &plan.actions[6] else {
        panic!("expected ProcessIncoming action");
    };
    assert_eq!(
        notification,
        &json!({
            "meta": {
                "sender_did": "did:bob",
                "target": {"kind": "agent", "did": "did:alice"},
                "message_id": "ack-sess-1",
                "profile": "anp.direct.e2ee.v1",
                "security_profile": "direct-e2ee",
                "content_type": "application/anp-direct-cipher+json",
            },
            "body": {"ciphertext": "ack"},
        })
    );
}

#[test]
fn recipient_client_init_failure_skips_after_notification_is_built() {
    let plan = plan(outcomes(|o| o.recipient_client_ok = false));

    assert_eq!(plan.decision, LocalSecureAckInProcessDecision::Skipped);
    assert!(matches!(
        plan.actions[5],
        LocalSecureAckInProcessAction::BuildRecipientSecureE2eeClient { .. }
    ));
    assert_eq!(
        plan.actions.last(),
        Some(&LocalSecureAckInProcessAction::LogSkipped {
            reason: LocalSecureAckInProcessSkipReason::RecipientClientInitError,
        })
    );
}

#[test]
fn recipient_process_failure_runs_fallback_ladder_before_sender_save() {
    let plan = plan(outcomes(|o| {
        o.process_incoming = ProcessIncomingOutcome::Error;
        o.active_recipient_session = ActiveRecipientSessionOutcome::Missing;
        o.runtime_session_exists = true;
    }));

    assert_eq!(plan.decision, LocalSecureAckInProcessDecision::Queued);
    let process_index = plan
        .actions
        .iter()
        .position(|action| {
            matches!(
                action,
                LocalSecureAckInProcessAction::ProcessIncoming { .. }
            )
        })
        .expect("ProcessIncoming action");
    assert_order(
        &plan.actions[process_index..],
        &[
            "ProcessIncoming",
            "ResolveRecipientPaths",
            "OpenRecipientSessionStore",
            "LoadRecipientSession",
            "MarshalAckBody",
            "UnmarshalAckCipher",
            "DecryptFollowUp",
            "SaveRecipientSession",
            "SaveSenderSession",
            "LookupActiveRecipientSession",
            "CheckRuntimeSessionForDID",
            "QueueLocalNotification",
            "LogQueued",
        ],
    );
}

#[test]
fn non_decrypted_process_result_uses_same_fallback_ladder() {
    let plan = plan(outcomes(|o| {
        o.process_incoming =
            ProcessIncomingOutcome::Result(json!({"state": "pending-confirmation"}));
        o.save_recipient_session_ok = false;
    }));

    assert_eq!(plan.decision, LocalSecureAckInProcessDecision::Skipped);
    assert!(plan.actions.iter().any(|action| matches!(
        action,
        LocalSecureAckInProcessAction::SaveRecipientSession { .. }
    )));
    assert_eq!(
        plan.actions.last(),
        Some(&LocalSecureAckInProcessAction::LogSkipped {
            reason: LocalSecureAckInProcessSkipReason::SaveRecipientSessionError,
        })
    );
}

#[test]
fn fallback_ladder_failures_skip_at_the_failing_step() {
    let cases = [
        (
            outcomes(|o| {
                o.process_incoming = ProcessIncomingOutcome::Error;
                o.recipient_paths_ok = false;
            }),
            LocalSecureAckInProcessSkipReason::RecipientPathsError,
        ),
        (
            outcomes(|o| {
                o.process_incoming = ProcessIncomingOutcome::Error;
                o.recipient_store_ok = false;
            }),
            LocalSecureAckInProcessSkipReason::RecipientSessionStoreError,
        ),
        (
            outcomes(|o| {
                o.process_incoming = ProcessIncomingOutcome::Error;
                o.recipient_session_load_ok = false;
            }),
            LocalSecureAckInProcessSkipReason::RecipientSessionLoadError,
        ),
        (
            outcomes(|o| {
                o.process_incoming = ProcessIncomingOutcome::Error;
                o.marshal_ack_body_ok = false;
            }),
            LocalSecureAckInProcessSkipReason::MarshalAckBodyError,
        ),
        (
            outcomes(|o| {
                o.process_incoming = ProcessIncomingOutcome::Error;
                o.unmarshal_ack_cipher_ok = false;
            }),
            LocalSecureAckInProcessSkipReason::UnmarshalAckCipherError,
        ),
        (
            outcomes(|o| {
                o.process_incoming = ProcessIncomingOutcome::Error;
                o.decrypt_follow_up_ok = false;
            }),
            LocalSecureAckInProcessSkipReason::DecryptFallbackError,
        ),
    ];

    for (outcomes, reason) in cases {
        let plan = plan(outcomes);

        assert_eq!(plan.decision, LocalSecureAckInProcessDecision::Skipped);
        assert_eq!(
            plan.actions.last(),
            Some(&LocalSecureAckInProcessAction::LogSkipped { reason })
        );
    }
}

#[test]
fn sender_save_failure_skips_before_active_session_lookup() {
    let plan = plan(outcomes(|o| o.save_sender_session_ok = false));

    assert_eq!(plan.decision, LocalSecureAckInProcessDecision::Skipped);
    assert!(plan.actions.iter().any(|action| matches!(
        action,
        LocalSecureAckInProcessAction::SaveSenderSession { .. }
    )));
    assert!(!plan.actions.iter().any(|action| matches!(
        action,
        LocalSecureAckInProcessAction::LookupActiveRecipientSession { .. }
    )));
    assert_eq!(
        plan.actions.last(),
        Some(&LocalSecureAckInProcessAction::LogSkipped {
            reason: LocalSecureAckInProcessSkipReason::SaveSenderSessionError,
        })
    );
}

#[test]
fn active_recipient_with_secure_rpc_flushes_then_logs_delivered_like_go() {
    let plan = plan(outcomes(|o| {
        o.active_recipient_session = ActiveRecipientSessionOutcome::Present {
            secure_rpc_available: true,
            flush_warnings: vec!["warn-1".to_string()],
        };
    }));

    assert_eq!(plan.decision, LocalSecureAckInProcessDecision::Delivered);
    assert_eq!(
        last_actions(&plan.actions, 3),
        vec![
            LocalSecureAckInProcessAction::FlushQueuedSecureOutbox {
                owner_did: "did:alice".to_string(),
                peer_did: "did:bob".to_string(),
                warnings: vec!["warn-1".to_string()],
            },
            LocalSecureAckInProcessAction::LogDeliveredWithFlushWarnings {
                recipient_did: "did:alice".to_string(),
                sender_did: "did:bob".to_string(),
                warnings: vec!["warn-1".to_string()],
            },
            LocalSecureAckInProcessAction::LogDelivered {
                recipient_did: "did:alice".to_string(),
                sender_did: "did:bob".to_string(),
            },
        ]
    );
}

#[test]
fn active_recipient_without_secure_rpc_only_logs_delivered() {
    let plan = plan(outcomes(|o| {
        o.active_recipient_session = ActiveRecipientSessionOutcome::Present {
            secure_rpc_available: false,
            flush_warnings: vec!["ignored".to_string()],
        };
    }));

    assert_eq!(plan.decision, LocalSecureAckInProcessDecision::Delivered);
    assert!(!plan.actions.iter().any(|action| matches!(
        action,
        LocalSecureAckInProcessAction::FlushQueuedSecureOutbox { .. }
    )));
    assert_eq!(
        plan.actions.last(),
        Some(&LocalSecureAckInProcessAction::LogDelivered {
            recipient_did: "did:alice".to_string(),
            sender_did: "did:bob".to_string(),
        })
    );
}

#[test]
fn managed_inactive_runtime_session_queues_direct_incoming_wrapper() {
    let plan = plan(outcomes(|o| {
        o.active_recipient_session = ActiveRecipientSessionOutcome::Missing;
        o.runtime_session_exists = true;
    }));

    assert_eq!(plan.decision, LocalSecureAckInProcessDecision::Queued);
    let LocalSecureAckInProcessAction::QueueLocalNotification {
        recipient_did,
        notification,
    } = &plan.actions[plan.actions.len() - 2]
    else {
        panic!("expected queue action before LogQueued");
    };
    assert_eq!(recipient_did, "did:alice");
    assert_eq!(notification["method"], json!("direct.incoming"));
    assert_eq!(
        notification["params"]["meta"]["content_type"],
        json!("application/anp-direct-cipher+json")
    );
    assert_eq!(
        plan.actions.last(),
        Some(&LocalSecureAckInProcessAction::LogQueued {
            recipient_did: "did:alice".to_string(),
            sender_did: "did:bob".to_string(),
        })
    );
}

#[test]
fn unmanaged_runtime_session_falls_back_to_network_after_runtime_check() {
    let plan = plan(outcomes(|o| {
        o.active_recipient_session = ActiveRecipientSessionOutcome::Missing;
        o.runtime_session_exists = false;
    }));

    assert_eq!(
        plan.decision,
        LocalSecureAckInProcessDecision::NetworkFallback
    );
    assert_eq!(
        last_actions(&plan.actions, 2),
        vec![
            LocalSecureAckInProcessAction::CheckRuntimeSessionForDID {
                recipient_did: "did:alice".to_string(),
            },
            LocalSecureAckInProcessAction::LogNetworkFallback {
                recipient_did: "did:alice".to_string(),
                sender_did: "did:bob".to_string(),
            },
        ]
    );
}

fn plan(
    outcomes: LocalSecureAckInProcessOutcomes,
) -> awiki_cli::runtime_legacy::listener_secure_ack_in_process::LocalSecureAckInProcessPlan {
    let sender = record("bob", "did:bob");
    let recipient = record("alice", "did:alice");
    deliver_local_secure_ack_in_process_plan(
        Some(&sender),
        Some(&recipient),
        "did:alice",
        " sess-1 ",
        " msg-1\n",
        "ack-sess-1",
        outcomes,
    )
}

fn outcomes(
    edit: impl FnOnce(&mut LocalSecureAckInProcessOutcomes),
) -> LocalSecureAckInProcessOutcomes {
    let mut outcomes = LocalSecureAckInProcessOutcomes::default();
    edit(&mut outcomes);
    outcomes
}

fn record(identity_name: &str, did: &str) -> StoredIdentity {
    StoredIdentity {
        identity_name: identity_name.to_string(),
        did: did.to_string(),
        ..StoredIdentity::default()
    }
}

fn last_actions(
    actions: &[LocalSecureAckInProcessAction],
    count: usize,
) -> Vec<LocalSecureAckInProcessAction> {
    actions[actions.len() - count..].to_vec()
}

fn assert_order(actions: &[LocalSecureAckInProcessAction], expected: &[&str]) {
    let names: Vec<&str> = actions.iter().map(action_name).collect();
    assert_eq!(names, expected);
}

fn action_name(action: &LocalSecureAckInProcessAction) -> &'static str {
    match action {
        LocalSecureAckInProcessAction::LookupRecipientRecord { .. } => "LookupRecipientRecord",
        LocalSecureAckInProcessAction::ResolveSenderPaths { .. } => "ResolveSenderPaths",
        LocalSecureAckInProcessAction::OpenSenderSessionStore { .. } => "OpenSenderSessionStore",
        LocalSecureAckInProcessAction::FindSenderSessionByPeerDID { .. } => {
            "FindSenderSessionByPeerDID"
        }
        LocalSecureAckInProcessAction::EncryptFollowUp { .. } => "EncryptFollowUp",
        LocalSecureAckInProcessAction::BuildRecipientSecureE2eeClient { .. } => {
            "BuildRecipientSecureE2eeClient"
        }
        LocalSecureAckInProcessAction::ProcessIncoming { .. } => "ProcessIncoming",
        LocalSecureAckInProcessAction::ResolveRecipientPaths { .. } => "ResolveRecipientPaths",
        LocalSecureAckInProcessAction::OpenRecipientSessionStore { .. } => {
            "OpenRecipientSessionStore"
        }
        LocalSecureAckInProcessAction::LoadRecipientSession { .. } => "LoadRecipientSession",
        LocalSecureAckInProcessAction::MarshalAckBody => "MarshalAckBody",
        LocalSecureAckInProcessAction::UnmarshalAckCipher => "UnmarshalAckCipher",
        LocalSecureAckInProcessAction::DecryptFollowUp { .. } => "DecryptFollowUp",
        LocalSecureAckInProcessAction::SaveRecipientSession { .. } => "SaveRecipientSession",
        LocalSecureAckInProcessAction::SaveSenderSession { .. } => "SaveSenderSession",
        LocalSecureAckInProcessAction::LookupActiveRecipientSession { .. } => {
            "LookupActiveRecipientSession"
        }
        LocalSecureAckInProcessAction::FlushQueuedSecureOutbox { .. } => "FlushQueuedSecureOutbox",
        LocalSecureAckInProcessAction::LogDeliveredWithFlushWarnings { .. } => {
            "LogDeliveredWithFlushWarnings"
        }
        LocalSecureAckInProcessAction::LogDelivered { .. } => "LogDelivered",
        LocalSecureAckInProcessAction::CheckRuntimeSessionForDID { .. } => {
            "CheckRuntimeSessionForDID"
        }
        LocalSecureAckInProcessAction::QueueLocalNotification { .. } => "QueueLocalNotification",
        LocalSecureAckInProcessAction::LogQueued { .. } => "LogQueued",
        LocalSecureAckInProcessAction::LogNetworkFallback { .. } => "LogNetworkFallback",
        LocalSecureAckInProcessAction::LogSkipped { .. } => "LogSkipped",
    }
}
