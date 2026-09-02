//! Process-local opaque continuations for existing-Handle registration Join.
//!
//! Account verification tokens never cross the Core API. Preparations are
//! short-lived, bound to the exact registration response and local identity
//! state, and disappear when the Core process exits.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::RngCore as _;
use sha2::{Digest as _, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

const PREPARATION_TTL: Duration = Duration::from_secs(600);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RegistrationJoinTransition {
    pub(crate) account_user_id: String,
    pub(crate) previous_did: String,
    pub(crate) current_did: String,
    pub(crate) binding_generation: String,
}

pub(crate) struct RegistrationJoinPreparationInput {
    pub(crate) raw_result_hash: String,
    pub(crate) expected_did: crate::ids::Did,
    pub(crate) full_handle: crate::ids::Handle,
    pub(crate) account_verification_token: crate::internal::platform_secret::SecretBytes,
    pub(crate) transition: Option<RegistrationJoinTransition>,
    pub(crate) mode: crate::identity::HandleRegistrationJoinMode,
    pub(crate) owner_identity_id: Option<String>,
    pub(crate) retired_owner_evidence:
        Option<crate::internal::identity_local_owner_matcher::RetiredOwnerEvidence>,
    pub(crate) resume_join_session_id: Option<String>,
    pub(crate) pending_registration_cleanup: Option<RegistrationPendingCleanup>,
    pub(crate) state_root_fingerprint: String,
    pub(crate) identity_index_fingerprint: String,
}

#[derive(Clone, PartialEq)]
pub(crate) struct RegistrationPendingCleanup {
    pub(crate) secret_ref: crate::internal::secret_vault::record::SecretRef,
    pub(crate) identity:
        crate::internal::identity_registration_pending::PendingRegistrationIdentity,
}

struct RegistrationJoinPreparationEntry {
    input: RegistrationJoinPreparationInput,
    created_at: Instant,
    begin_operation_id: Option<String>,
    begin_input_hash: Option<String>,
    join_session_id: Option<String>,
    remote_started: bool,
    lock: Arc<tokio::sync::Mutex<()>>,
}

#[derive(Clone, PartialEq)]
pub(crate) struct RegistrationJoinPreparationSnapshot {
    pub(crate) expected_did: crate::ids::Did,
    pub(crate) full_handle: crate::ids::Handle,
    pub(crate) account_verification_token: Vec<u8>,
    pub(crate) transition: Option<RegistrationJoinTransition>,
    pub(crate) mode: crate::identity::HandleRegistrationJoinMode,
    pub(crate) owner_identity_id: Option<String>,
    pub(crate) retired_owner_evidence:
        Option<crate::internal::identity_local_owner_matcher::RetiredOwnerEvidence>,
    pub(crate) resume_join_session_id: Option<String>,
    pub(crate) pending_registration_cleanup: Option<RegistrationPendingCleanup>,
    pub(crate) state_root_fingerprint: String,
    pub(crate) identity_index_fingerprint: String,
    pub(crate) join_session_id: Option<String>,
    pub(crate) remote_started: bool,
}

#[derive(Default)]
pub(crate) struct RegistrationJoinPreparationStore {
    entries: std::sync::Mutex<HashMap<String, RegistrationJoinPreparationEntry>>,
}

impl std::fmt::Debug for RegistrationJoinPreparationStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let entry_count = self.entries.lock().map(|entries| entries.len()).ok();
        formatter
            .debug_struct("RegistrationJoinPreparationStore")
            .field("entry_count", &entry_count)
            .finish()
    }
}

impl RegistrationJoinPreparationStore {
    pub(crate) fn issue(
        &self,
        input: RegistrationJoinPreparationInput,
    ) -> crate::ImResult<crate::identity::HandleRegistrationJoinRequiredPreparation> {
        validate_input(&input)?;
        let mut random = [0_u8; 24];
        rand::rngs::OsRng
            .try_fill_bytes(&mut random)
            .map_err(|_| crate::ImError::Internal {
                message: "generate registration Join preparation failed".to_owned(),
            })?;
        let preparation_id = format!("regjoin_{}", URL_SAFE_NO_PAD.encode(random));
        let projection = crate::identity::HandleRegistrationJoinRequiredPreparation {
            preparation_id: preparation_id.clone(),
            mode: input.mode,
            requires_user_presence: matches!(
                input.mode,
                crate::identity::HandleRegistrationJoinMode::HandleRecoveryRebind
            ),
            expected_did: input.expected_did.clone(),
            full_handle: input.full_handle.clone(),
        };
        let resume_join_session_id = input.resume_join_session_id.clone();
        let mut entries = self.lock_entries()?;
        retain_live(&mut entries);
        entries.insert(
            preparation_id,
            RegistrationJoinPreparationEntry {
                input,
                created_at: Instant::now(),
                begin_operation_id: None,
                begin_input_hash: None,
                join_session_id: resume_join_session_id,
                remote_started: false,
                lock: Arc::new(tokio::sync::Mutex::new(())),
            },
        );
        Ok(projection)
    }

    pub(crate) fn operation_lock(
        &self,
        preparation_id: &str,
    ) -> crate::ImResult<Arc<tokio::sync::Mutex<()>>> {
        let mut entries = self.lock_entries()?;
        retain_live(&mut entries);
        entries
            .get(preparation_id)
            .map(|entry| Arc::clone(&entry.lock))
            .ok_or_else(preparation_not_found)
    }

    pub(crate) fn bind_and_snapshot(
        &self,
        preparation_id: &str,
        operation_id: &str,
        begin_input_hash: &str,
    ) -> crate::ImResult<RegistrationJoinPreparationSnapshot> {
        let mut entries = self.lock_entries()?;
        retain_live(&mut entries);
        let entry = entries
            .get_mut(preparation_id)
            .ok_or_else(preparation_not_found)?;
        match (&entry.begin_operation_id, &entry.begin_input_hash) {
            (Some(existing_operation), Some(existing_hash))
                if existing_operation == operation_id && existing_hash == begin_input_hash => {}
            (None, None) => {
                entry.begin_operation_id = Some(operation_id.to_owned());
                entry.begin_input_hash = Some(begin_input_hash.to_owned());
            }
            _ => return Err(crate::ImError::PermissionDenied),
        }
        Ok(RegistrationJoinPreparationSnapshot {
            expected_did: entry.input.expected_did.clone(),
            full_handle: entry.input.full_handle.clone(),
            account_verification_token: entry
                .input
                .account_verification_token
                .expose_secret()
                .to_vec(),
            transition: entry.input.transition.clone(),
            mode: entry.input.mode,
            owner_identity_id: entry.input.owner_identity_id.clone(),
            retired_owner_evidence: entry.input.retired_owner_evidence.clone(),
            resume_join_session_id: entry.input.resume_join_session_id.clone(),
            pending_registration_cleanup: entry.input.pending_registration_cleanup.clone(),
            state_root_fingerprint: entry.input.state_root_fingerprint.clone(),
            identity_index_fingerprint: entry.input.identity_index_fingerprint.clone(),
            join_session_id: entry.join_session_id.clone(),
            remote_started: entry.remote_started,
        })
    }

    pub(crate) fn bind_local_session(
        &self,
        preparation_id: &str,
        operation_id: &str,
        join_session_id: &str,
    ) -> crate::ImResult<()> {
        let mut entries = self.lock_entries()?;
        retain_live(&mut entries);
        let entry = entries
            .get_mut(preparation_id)
            .ok_or_else(preparation_not_found)?;
        if entry.begin_operation_id.as_deref() != Some(operation_id) {
            return Err(crate::ImError::PermissionDenied);
        }
        match entry.join_session_id.as_deref() {
            Some(existing) if existing == join_session_id => Ok(()),
            Some(_) => Err(crate::ImError::PermissionDenied),
            None => {
                entry.join_session_id = Some(join_session_id.to_owned());
                Ok(())
            }
        }
    }

    pub(crate) fn mark_remote_started(
        &self,
        preparation_id: &str,
        operation_id: &str,
        join_session_id: &str,
    ) -> crate::ImResult<()> {
        let mut entries = self.lock_entries()?;
        retain_live(&mut entries);
        let entry = entries
            .get_mut(preparation_id)
            .ok_or_else(preparation_not_found)?;
        if entry.begin_operation_id.as_deref() != Some(operation_id)
            || entry.join_session_id.as_deref() != Some(join_session_id)
        {
            return Err(crate::ImError::PermissionDenied);
        }
        entry.remote_started = true;
        Ok(())
    }

    fn lock_entries(
        &self,
    ) -> crate::ImResult<std::sync::MutexGuard<'_, HashMap<String, RegistrationJoinPreparationEntry>>>
    {
        self.entries
            .lock()
            .map_err(|_| crate::ImError::LocalStateUnavailable {
                detail: "registration Join preparation lock poisoned".to_owned(),
            })
    }
}

pub(crate) fn identity_index_fingerprint(
    index: &crate::internal::identity_store::IndexPayload,
) -> crate::ImResult<String> {
    let canonical =
        serde_json_canonicalizer::to_vec(index).map_err(|error| crate::ImError::Serialization {
            detail: error.to_string(),
        })?;
    Ok(format!("sha256:{:x}", Sha256::digest(canonical)))
}

pub(crate) fn registration_result_hash(raw: &serde_json::Value) -> crate::ImResult<String> {
    let canonical =
        serde_json_canonicalizer::to_vec(raw).map_err(|error| crate::ImError::Serialization {
            detail: error.to_string(),
        })?;
    Ok(format!("sha256:{:x}", Sha256::digest(canonical)))
}

pub(crate) fn begin_input_hash(
    operation_id: &str,
    ttl_seconds: u64,
    user_presence_confirmed: bool,
) -> crate::ImResult<String> {
    let value = serde_json::json!({
        "operation_id": operation_id,
        "ttl_seconds": ttl_seconds,
        "user_presence_confirmed": user_presence_confirmed,
    });
    registration_result_hash(&value)
}

pub(crate) fn continuity_error(code: &'static str) -> crate::ImError {
    crate::ImError::Service {
        status_code: None,
        code: Some(code.to_owned()),
        message: code.to_owned(),
        data: None,
    }
}

fn validate_input(input: &RegistrationJoinPreparationInput) -> crate::ImResult<()> {
    let is_rebind = matches!(
        input.mode,
        crate::identity::HandleRegistrationJoinMode::HandleRecoveryRebind
    );
    let is_retired_ordinary = input.retired_owner_evidence.is_some();
    let is_resume = input.resume_join_session_id.is_some();
    let transition_invalid = input.transition.as_ref().is_some_and(|transition| {
        transition.account_user_id.trim().is_empty()
            || transition.previous_did == transition.current_did
            || transition.current_did != input.expected_did.as_str()
            || anp::wns::BindingGeneration::new(transition.binding_generation.clone()).is_err()
    });
    if input.raw_result_hash.trim().is_empty()
        || input.state_root_fingerprint.trim().is_empty()
        || input.identity_index_fingerprint.trim().is_empty()
        || input
            .account_verification_token
            .expose_secret()
            .iter()
            .all(u8::is_ascii_whitespace)
    {
        return Err(continuity_error(
            "identity.registration_join.preparation_base_invalid",
        ));
    }
    if is_rebind && is_retired_ordinary {
        return Err(continuity_error(
            "identity.registration_join.preparation_mode_invalid",
        ));
    }
    if (is_rebind || is_retired_ordinary) != input.owner_identity_id.is_some() {
        return Err(continuity_error(
            "identity.registration_join.preparation_owner_invalid",
        ));
    }
    if (is_rebind || is_retired_ordinary) && input.transition.is_none() {
        return Err(continuity_error(
            "identity.registration_join.preparation_transition_missing",
        ));
    }
    if is_resume && (!is_rebind || input.owner_identity_id.is_none()) {
        return Err(continuity_error(
            "identity.registration_join.preparation_resume_invalid",
        ));
    }
    if input
        .resume_join_session_id
        .as_deref()
        .is_some_and(|join_session_id| join_session_id.trim().is_empty())
    {
        return Err(continuity_error(
            "identity.registration_join.preparation_resume_invalid",
        ));
    }
    if input
        .pending_registration_cleanup
        .as_ref()
        .is_some_and(|cleanup| {
            cleanup.secret_ref.kind
                != crate::internal::secret_vault::record::SecretKind::IdentityRegistrationPending
                || cleanup.identity.validate().is_err()
        })
    {
        return Err(continuity_error(
            "identity.registration_join.preparation_cleanup_invalid",
        ));
    }
    if input
        .owner_identity_id
        .as_deref()
        .is_some_and(|owner| owner.trim().is_empty())
    {
        return Err(continuity_error(
            "identity.registration_join.preparation_owner_invalid",
        ));
    }
    if input
        .retired_owner_evidence
        .as_ref()
        .is_some_and(|evidence| {
            input.owner_identity_id.as_deref() != Some(evidence.owner_identity_id.as_str())
                || evidence.retired_did.trim().is_empty()
                || evidence.retired_protocol_device_id.trim().is_empty()
                || evidence.retired_binding_generation.trim().is_empty()
        })
    {
        return Err(continuity_error(
            "identity.registration_join.preparation_retirement_invalid",
        ));
    }
    if transition_invalid {
        return Err(continuity_error(
            "identity.registration_join.preparation_transition_invalid",
        ));
    }
    Ok(())
}

fn retain_live(entries: &mut HashMap<String, RegistrationJoinPreparationEntry>) {
    entries.retain(|_, entry| entry.created_at.elapsed() <= PREPARATION_TTL);
}

fn preparation_not_found() -> crate::ImError {
    crate::ImError::invalid_input(
        Some("preparation_id".to_owned()),
        "registration Join preparation is unavailable or expired",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registration_recovery_join_preparation_is_opaque_and_input_bound() {
        let store = RegistrationJoinPreparationStore::default();
        let projection = store
            .issue(RegistrationJoinPreparationInput {
                raw_result_hash: "sha256:registration".to_owned(),
                expected_did: crate::ids::Did::parse("did:wba:example.test:alice:new").unwrap(),
                full_handle: crate::ids::Handle::parse("alice.example.test", "").unwrap(),
                account_verification_token: crate::internal::platform_secret::SecretBytes::from_vec(
                    b"single-use-token".to_vec(),
                ),
                transition: Some(RegistrationJoinTransition {
                    account_user_id: "account-alice".to_owned(),
                    previous_did: "did:wba:example.test:alice:old".to_owned(),
                    current_did: "did:wba:example.test:alice:new".to_owned(),
                    binding_generation: "8".to_owned(),
                }),
                mode: crate::identity::HandleRegistrationJoinMode::HandleRecoveryRebind,
                owner_identity_id: Some("owner-alice".to_owned()),
                retired_owner_evidence: None,
                resume_join_session_id: None,
                pending_registration_cleanup: None,
                state_root_fingerprint: format!("sha256:{}", "a".repeat(64)),
                identity_index_fingerprint: format!("sha256:{}", "b".repeat(64)),
            })
            .unwrap();

        assert!(projection.preparation_id.starts_with("regjoin_"));
        assert!(projection.requires_user_presence);
        let hash = begin_input_hash("join-operation", 600, true).unwrap();
        let snapshot = store
            .bind_and_snapshot(&projection.preparation_id, "join-operation", &hash)
            .unwrap();
        assert_eq!(snapshot.account_verification_token, b"single-use-token");
        assert!(matches!(
            store.bind_and_snapshot(
                &projection.preparation_id,
                "different-operation",
                &begin_input_hash("different-operation", 600, true).unwrap(),
            ),
            Err(crate::ImError::PermissionDenied)
        ));
    }

    #[test]
    fn registration_recovery_join_allows_transition_on_a_fresh_device_without_rebind() {
        let store = RegistrationJoinPreparationStore::default();
        let projection = store
            .issue(RegistrationJoinPreparationInput {
                raw_result_hash: "sha256:fresh-registration".to_owned(),
                expected_did: crate::ids::Did::parse("did:wba:example.test:alice:new").unwrap(),
                full_handle: crate::ids::Handle::parse("alice.example.test", "").unwrap(),
                account_verification_token: crate::internal::platform_secret::SecretBytes::from_vec(
                    b"token".to_vec(),
                ),
                transition: Some(RegistrationJoinTransition {
                    account_user_id: "account-alice".to_owned(),
                    previous_did: "did:wba:example.test:alice:old".to_owned(),
                    current_did: "did:wba:example.test:alice:new".to_owned(),
                    binding_generation: "8".to_owned(),
                }),
                mode: crate::identity::HandleRegistrationJoinMode::Ordinary,
                owner_identity_id: None,
                retired_owner_evidence: None,
                resume_join_session_id: None,
                pending_registration_cleanup: None,
                state_root_fingerprint: format!("sha256:{}", "a".repeat(64)),
                identity_index_fingerprint: format!("sha256:{}", "b".repeat(64)),
            })
            .unwrap();

        assert_eq!(
            projection.mode,
            crate::identity::HandleRegistrationJoinMode::Ordinary
        );
        assert!(!projection.requires_user_presence);
    }

    #[test]
    fn registration_resume_preparation_keeps_existing_session_private() {
        let store = RegistrationJoinPreparationStore::default();
        let projection = store
            .issue(RegistrationJoinPreparationInput {
                raw_result_hash: "sha256:resume-registration".to_owned(),
                expected_did: crate::ids::Did::parse("did:wba:example.test:alice:new").unwrap(),
                full_handle: crate::ids::Handle::parse("alice.example.test", "").unwrap(),
                account_verification_token: crate::internal::platform_secret::SecretBytes::from_vec(
                    b"fresh-verification-grant".to_vec(),
                ),
                transition: Some(RegistrationJoinTransition {
                    account_user_id: "account-alice".to_owned(),
                    previous_did: "did:wba:example.test:alice:old".to_owned(),
                    current_did: "did:wba:example.test:alice:new".to_owned(),
                    binding_generation: "8".to_owned(),
                }),
                mode: crate::identity::HandleRegistrationJoinMode::HandleRecoveryRebind,
                owner_identity_id: Some("owner-alice".to_owned()),
                retired_owner_evidence: None,
                resume_join_session_id: Some("existing-join-session".to_owned()),
                pending_registration_cleanup: None,
                state_root_fingerprint: format!("sha256:{}", "a".repeat(64)),
                identity_index_fingerprint: format!("sha256:{}", "b".repeat(64)),
            })
            .unwrap();
        let public_keys = serde_json::to_value(&projection)
            .unwrap()
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            public_keys,
            std::collections::BTreeSet::from([
                "expected_did".to_owned(),
                "full_handle".to_owned(),
                "mode".to_owned(),
                "preparation_id".to_owned(),
                "requires_user_presence".to_owned(),
            ])
        );
        let hash = begin_input_hash("new-process-invocation", 600, true).unwrap();
        let snapshot = store
            .bind_and_snapshot(&projection.preparation_id, "new-process-invocation", &hash)
            .unwrap();
        assert_eq!(
            snapshot.resume_join_session_id.as_deref(),
            Some("existing-join-session")
        );
        assert_eq!(
            snapshot.join_session_id.as_deref(),
            Some("existing-join-session")
        );
        assert!(!snapshot.remote_started);
    }
}
