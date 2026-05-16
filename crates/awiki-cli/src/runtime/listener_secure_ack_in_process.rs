use super::listener_json_helpers::struct_to_map;
use super::listener_secure_ack_delivery::build_secure_ack_payload;
use crate::identity::types::StoredIdentity;
use serde_json::{json, Map, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalSecureAckInProcessDecision {
    Skipped,
    Delivered,
    Queued,
    NetworkFallback,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalSecureAckInProcessSkipReason {
    SenderRecordMissing,
    RecipientNotManaged,
    SenderPathsError,
    SenderSessionStoreError,
    SenderSessionLookupFailed,
    EncryptFollowUpError,
    RecipientClientInitError,
    RecipientPathsError,
    RecipientSessionStoreError,
    RecipientSessionLoadError,
    MarshalAckBodyError,
    UnmarshalAckCipherError,
    DecryptFallbackError,
    SaveRecipientSessionError,
    SaveSenderSessionError,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LocalSecureAckInProcessAction {
    LookupRecipientRecord {
        recipient_did: String,
    },
    ResolveSenderPaths {
        identity_name: String,
    },
    OpenSenderSessionStore {
        identity_name: String,
    },
    FindSenderSessionByPeerDID {
        peer_did: String,
    },
    EncryptFollowUp {
        sender_did: String,
        recipient_did: String,
        message_id: String,
        payload: Map<String, Value>,
    },
    BuildRecipientSecureE2eeClient {
        identity_name: String,
    },
    ProcessIncoming {
        notification: Value,
    },
    ResolveRecipientPaths {
        identity_name: String,
    },
    OpenRecipientSessionStore {
        identity_name: String,
    },
    LoadRecipientSession {
        session_id: String,
    },
    MarshalAckBody,
    UnmarshalAckCipher,
    DecryptFollowUp {
        sender_did: String,
        recipient_did: String,
        message_id: String,
    },
    SaveRecipientSession {
        identity_name: String,
    },
    SaveSenderSession {
        identity_name: String,
    },
    LookupActiveRecipientSession {
        recipient_did: String,
    },
    FlushQueuedSecureOutbox {
        owner_did: String,
        peer_did: String,
        warnings: Vec<String>,
    },
    LogDeliveredWithFlushWarnings {
        recipient_did: String,
        sender_did: String,
        warnings: Vec<String>,
    },
    LogDelivered {
        recipient_did: String,
        sender_did: String,
    },
    CheckRuntimeSessionForDID {
        recipient_did: String,
    },
    QueueLocalNotification {
        recipient_did: String,
        notification: Value,
    },
    LogQueued {
        recipient_did: String,
        sender_did: String,
    },
    LogNetworkFallback {
        recipient_did: String,
        sender_did: String,
    },
    LogSkipped {
        reason: LocalSecureAckInProcessSkipReason,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct LocalSecureAckInProcessPlan {
    pub actions: Vec<LocalSecureAckInProcessAction>,
    pub decision: LocalSecureAckInProcessDecision,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SenderSessionLookupOutcome {
    Found,
    Missing,
    Error,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EncryptFollowUpOutcome {
    Error,
    AckBody(Value),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProcessIncomingOutcome {
    Error,
    Result(Value),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActiveRecipientSessionOutcome {
    Missing,
    Present {
        secure_rpc_available: bool,
        flush_warnings: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct LocalSecureAckInProcessOutcomes {
    pub sender_paths_ok: bool,
    pub sender_store_ok: bool,
    pub sender_session_lookup: SenderSessionLookupOutcome,
    pub encrypt_follow_up: EncryptFollowUpOutcome,
    pub recipient_client_ok: bool,
    pub process_incoming: ProcessIncomingOutcome,
    pub recipient_paths_ok: bool,
    pub recipient_store_ok: bool,
    pub recipient_session_load_ok: bool,
    pub marshal_ack_body_ok: bool,
    pub unmarshal_ack_cipher_ok: bool,
    pub decrypt_follow_up_ok: bool,
    pub save_recipient_session_ok: bool,
    pub save_sender_session_ok: bool,
    pub active_recipient_session: ActiveRecipientSessionOutcome,
    pub runtime_session_exists: bool,
}

impl Default for LocalSecureAckInProcessOutcomes {
    fn default() -> Self {
        Self {
            sender_paths_ok: true,
            sender_store_ok: true,
            sender_session_lookup: SenderSessionLookupOutcome::Found,
            encrypt_follow_up: EncryptFollowUpOutcome::AckBody(json!({"ciphertext": "ack"})),
            recipient_client_ok: true,
            process_incoming: ProcessIncomingOutcome::Result(json!({"state": "decrypted"})),
            recipient_paths_ok: true,
            recipient_store_ok: true,
            recipient_session_load_ok: true,
            marshal_ack_body_ok: true,
            unmarshal_ack_cipher_ok: true,
            decrypt_follow_up_ok: true,
            save_recipient_session_ok: true,
            save_sender_session_ok: true,
            active_recipient_session: ActiveRecipientSessionOutcome::Present {
                secure_rpc_available: false,
                flush_warnings: Vec::new(),
            },
            runtime_session_exists: false,
        }
    }
}

pub fn deliver_local_secure_ack_in_process_plan(
    sender_record: Option<&StoredIdentity>,
    recipient_record: Option<&StoredIdentity>,
    recipient_did: &str,
    session_id: &str,
    replied_message_id: &str,
    ack_message_id: &str,
    outcomes: LocalSecureAckInProcessOutcomes,
) -> LocalSecureAckInProcessPlan {
    let mut actions = Vec::new();
    let Some(sender_record) = sender_record else {
        return skipped(
            actions,
            LocalSecureAckInProcessSkipReason::SenderRecordMissing,
        );
    };

    actions.push(LocalSecureAckInProcessAction::LookupRecipientRecord {
        recipient_did: recipient_did.to_string(),
    });
    let Some(recipient_record) = recipient_record else {
        return skipped(
            actions,
            LocalSecureAckInProcessSkipReason::RecipientNotManaged,
        );
    };

    actions.push(LocalSecureAckInProcessAction::ResolveSenderPaths {
        identity_name: sender_record.identity_name.clone(),
    });
    if !outcomes.sender_paths_ok {
        return skipped(actions, LocalSecureAckInProcessSkipReason::SenderPathsError);
    }

    actions.push(LocalSecureAckInProcessAction::OpenSenderSessionStore {
        identity_name: sender_record.identity_name.clone(),
    });
    if !outcomes.sender_store_ok {
        return skipped(
            actions,
            LocalSecureAckInProcessSkipReason::SenderSessionStoreError,
        );
    }

    actions.push(LocalSecureAckInProcessAction::FindSenderSessionByPeerDID {
        peer_did: recipient_did.to_string(),
    });
    if outcomes.sender_session_lookup != SenderSessionLookupOutcome::Found {
        return skipped(
            actions,
            LocalSecureAckInProcessSkipReason::SenderSessionLookupFailed,
        );
    }

    let payload = build_secure_ack_payload(session_id, replied_message_id);
    actions.push(LocalSecureAckInProcessAction::EncryptFollowUp {
        sender_did: sender_record.did.clone(),
        recipient_did: recipient_did.to_string(),
        message_id: ack_message_id.to_string(),
        payload,
    });
    let EncryptFollowUpOutcome::AckBody(ack_body) = outcomes.encrypt_follow_up else {
        return skipped(
            actions,
            LocalSecureAckInProcessSkipReason::EncryptFollowUpError,
        );
    };
    let notification =
        encrypted_ack_notification(sender_record, recipient_did, ack_message_id, ack_body);

    actions.push(
        LocalSecureAckInProcessAction::BuildRecipientSecureE2eeClient {
            identity_name: recipient_record.identity_name.clone(),
        },
    );
    if !outcomes.recipient_client_ok {
        return skipped(
            actions,
            LocalSecureAckInProcessSkipReason::RecipientClientInitError,
        );
    }

    actions.push(LocalSecureAckInProcessAction::ProcessIncoming {
        notification: notification.clone(),
    });
    if !process_decrypted(&outcomes.process_incoming) {
        actions.push(LocalSecureAckInProcessAction::ResolveRecipientPaths {
            identity_name: recipient_record.identity_name.clone(),
        });
        if !outcomes.recipient_paths_ok {
            return skipped(
                actions,
                LocalSecureAckInProcessSkipReason::RecipientPathsError,
            );
        }
        actions.push(LocalSecureAckInProcessAction::OpenRecipientSessionStore {
            identity_name: recipient_record.identity_name.clone(),
        });
        if !outcomes.recipient_store_ok {
            return skipped(
                actions,
                LocalSecureAckInProcessSkipReason::RecipientSessionStoreError,
            );
        }
        actions.push(LocalSecureAckInProcessAction::LoadRecipientSession {
            session_id: session_id.to_string(),
        });
        if !outcomes.recipient_session_load_ok {
            return skipped(
                actions,
                LocalSecureAckInProcessSkipReason::RecipientSessionLoadError,
            );
        }
        actions.push(LocalSecureAckInProcessAction::MarshalAckBody);
        if !outcomes.marshal_ack_body_ok {
            return skipped(
                actions,
                LocalSecureAckInProcessSkipReason::MarshalAckBodyError,
            );
        }
        actions.push(LocalSecureAckInProcessAction::UnmarshalAckCipher);
        if !outcomes.unmarshal_ack_cipher_ok {
            return skipped(
                actions,
                LocalSecureAckInProcessSkipReason::UnmarshalAckCipherError,
            );
        }
        actions.push(LocalSecureAckInProcessAction::DecryptFollowUp {
            sender_did: sender_record.did.clone(),
            recipient_did: recipient_did.to_string(),
            message_id: ack_message_id.to_string(),
        });
        if !outcomes.decrypt_follow_up_ok {
            return skipped(
                actions,
                LocalSecureAckInProcessSkipReason::DecryptFallbackError,
            );
        }
        actions.push(LocalSecureAckInProcessAction::SaveRecipientSession {
            identity_name: recipient_record.identity_name.clone(),
        });
        if !outcomes.save_recipient_session_ok {
            return skipped(
                actions,
                LocalSecureAckInProcessSkipReason::SaveRecipientSessionError,
            );
        }
    }

    actions.push(LocalSecureAckInProcessAction::SaveSenderSession {
        identity_name: sender_record.identity_name.clone(),
    });
    if !outcomes.save_sender_session_ok {
        return skipped(
            actions,
            LocalSecureAckInProcessSkipReason::SaveSenderSessionError,
        );
    }

    actions.push(
        LocalSecureAckInProcessAction::LookupActiveRecipientSession {
            recipient_did: recipient_did.to_string(),
        },
    );
    if let ActiveRecipientSessionOutcome::Present {
        secure_rpc_available,
        flush_warnings,
    } = outcomes.active_recipient_session
    {
        if secure_rpc_available {
            actions.push(LocalSecureAckInProcessAction::FlushQueuedSecureOutbox {
                owner_did: recipient_record.did.clone(),
                peer_did: sender_record.did.clone(),
                warnings: flush_warnings.clone(),
            });
            actions.push(
                LocalSecureAckInProcessAction::LogDeliveredWithFlushWarnings {
                    recipient_did: recipient_record.did.clone(),
                    sender_did: sender_record.did.clone(),
                    warnings: flush_warnings,
                },
            );
        }
        actions.push(LocalSecureAckInProcessAction::LogDelivered {
            recipient_did: recipient_record.did.clone(),
            sender_did: sender_record.did.clone(),
        });
        return LocalSecureAckInProcessPlan {
            actions,
            decision: LocalSecureAckInProcessDecision::Delivered,
        };
    }

    actions.push(LocalSecureAckInProcessAction::CheckRuntimeSessionForDID {
        recipient_did: recipient_did.to_string(),
    });
    if outcomes.runtime_session_exists {
        actions.push(LocalSecureAckInProcessAction::QueueLocalNotification {
            recipient_did: recipient_did.to_string(),
            notification: json!({
                "method": "direct.incoming",
                "params": notification,
            }),
        });
        actions.push(LocalSecureAckInProcessAction::LogQueued {
            recipient_did: recipient_record.did.clone(),
            sender_did: sender_record.did.clone(),
        });
        return LocalSecureAckInProcessPlan {
            actions,
            decision: LocalSecureAckInProcessDecision::Queued,
        };
    }

    actions.push(LocalSecureAckInProcessAction::LogNetworkFallback {
        recipient_did: recipient_record.did.clone(),
        sender_did: sender_record.did.clone(),
    });
    LocalSecureAckInProcessPlan {
        actions,
        decision: LocalSecureAckInProcessDecision::NetworkFallback,
    }
}

fn process_decrypted(outcome: &ProcessIncomingOutcome) -> bool {
    match outcome {
        ProcessIncomingOutcome::Error => false,
        ProcessIncomingOutcome::Result(result) => {
            result.get("state").and_then(Value::as_str) == Some("decrypted")
        }
    }
}

fn encrypted_ack_notification(
    sender_record: &StoredIdentity,
    recipient_did: &str,
    ack_message_id: &str,
    ack_body: Value,
) -> Value {
    json!({
        "meta": {
            "sender_did": sender_record.did,
            "target": {"kind": "agent", "did": recipient_did},
            "message_id": ack_message_id,
            "profile": "anp.direct.e2ee.v1",
            "security_profile": "direct-e2ee",
            "content_type": "application/anp-direct-cipher+json",
        },
        "body": struct_to_map(ack_body),
    })
}

fn skipped(
    mut actions: Vec<LocalSecureAckInProcessAction>,
    reason: LocalSecureAckInProcessSkipReason,
) -> LocalSecureAckInProcessPlan {
    actions.push(LocalSecureAckInProcessAction::LogSkipped { reason });
    LocalSecureAckInProcessPlan {
        actions,
        decision: LocalSecureAckInProcessDecision::Skipped,
    }
}
