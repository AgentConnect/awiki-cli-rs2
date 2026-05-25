use crate::identity::types::StoredIdentity;

#[derive(Debug, Clone)]
pub struct SecureOutboxFlushSession {
    pub identity_name: String,
    pub current_record: Option<StoredIdentity>,
    pub secure_rpc_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecureOutboxFlushAction {
    FlushQueuedSecureOutbox {
        owner_did: String,
        peer_did: String,
        identity_name: String,
    },
    LogQueuedSecureOutboxFlush {
        owner_did: String,
        peer_did: String,
        warnings: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecureOutboxFlushPlan {
    pub actions: Vec<SecureOutboxFlushAction>,
}

pub fn flush_peer_queued_secure_outbox_plan(
    sessions: &[SecureOutboxFlushSession],
    owner_did: &str,
    peer_did: &str,
    flush_warnings: impl FnOnce(&StoredIdentity, &str) -> Vec<String>,
) -> SecureOutboxFlushPlan {
    let mut flush_warnings = Some(flush_warnings);
    for session in sessions {
        let Some(record) = session.current_record.as_ref() else {
            continue;
        };
        if record.did != owner_did {
            continue;
        }
        if !session.secure_rpc_available {
            return no_actions();
        }
        let warnings =
            flush_warnings
                .take()
                .expect("flush warnings are evaluated at most once")(record, peer_did);
        return SecureOutboxFlushPlan {
            actions: vec![
                SecureOutboxFlushAction::FlushQueuedSecureOutbox {
                    owner_did: owner_did.to_string(),
                    peer_did: peer_did.to_string(),
                    identity_name: record.identity_name.clone(),
                },
                SecureOutboxFlushAction::LogQueuedSecureOutboxFlush {
                    owner_did: owner_did.to_string(),
                    peer_did: peer_did.to_string(),
                    warnings,
                },
            ],
        };
    }
    no_actions()
}

fn no_actions() -> SecureOutboxFlushPlan {
    SecureOutboxFlushPlan {
        actions: Vec::new(),
    }
}
