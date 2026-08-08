//! Vault-only exact-retry state for Manifest Handle Recovery v1.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::internal::platform_secret::SecretBytes;
use crate::internal::secret_vault::policy::SecretAccessPolicy;
use crate::internal::secret_vault::record::{SecretKind, SecretMetadata, SecretRef};
use crate::internal::secret_vault::{SealSecretRequest, SecretVault};

pub(crate) const V4_CONTRACT_VERSION: &str = "awiki.handle-recovery.v1.contract.4.20260807";
pub(crate) const V4_CONTRACT_HASH: &str =
    "173d53051fc690f35f958bff7f08a51fd8458c729230d33563a16e0db1db3b84";
const V4_SCHEMA_VERSION: u32 = 1;
const V4_KEY_VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecoveryAuthoritativeBindingV4 {
    pub(crate) account_user_id: String,
    pub(crate) full_handle: String,
    pub(crate) current_did: String,
    pub(crate) binding_generation: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecoveryIntentV4 {
    pub(crate) schema_version: String,
    pub(crate) contract_version: String,
    pub(crate) operation_id: String,
    pub(crate) account_user_id: String,
    pub(crate) full_handle: String,
    pub(crate) expected_previous_did: String,
    pub(crate) expected_binding_generation: String,
    pub(crate) new_did: String,
    pub(crate) new_did_document_hash: String,
    pub(crate) bootstrap_device_id: String,
    pub(crate) bootstrap_signing_key_id: String,
    /// The closed public-only `{kty,crv,x}` JWK object. It is intentionally
    /// stored directly in the immutable intent, never through a second lookup.
    pub(crate) bootstrap_signing_public_key: serde_json::Value,
}

impl RecoveryIntentV4 {
    pub(crate) fn hash(&self) -> crate::ImResult<String> {
        let canonical = serde_json_canonicalizer::to_vec(self).map_err(|error| {
            crate::ImError::Serialization {
                detail: error.to_string(),
            }
        })?;
        Ok(format!(
            "sha256:{}",
            URL_SAFE_NO_PAD.encode(Sha256::digest(canonical))
        ))
    }

    pub(crate) fn validate(&self) -> crate::ImResult<()> {
        let canonical_jwk = canonical_ed25519_public_jwk(&self.bootstrap_signing_public_key)?;
        if self.schema_version != "1"
            || self.contract_version != V4_CONTRACT_VERSION
            || self.account_user_id.trim().is_empty()
            || self.full_handle.len() > 320
            || self.full_handle.len() > 320
            || self.new_did == self.expected_previous_did
            || self.bootstrap_device_id.trim().is_empty()
            || !self
                .bootstrap_signing_key_id
                .starts_with(&format!("{}#", self.new_did))
            || crate::internal::identity_wire::handle_recovery::canonical_handle(&self.full_handle)
                .is_err()
            || crate::internal::identity_wire::handle_recovery::validate_operation_id(
                &self.operation_id,
            )
            .is_err()
            || !canonical_generation(&self.expected_binding_generation)
            || !valid_sha256_digest(&self.new_did_document_hash)
            || canonical_jwk != self.bootstrap_signing_public_key
        {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RecoveryFactorStateV4 {
    AwaitingOtp,
    Exchanged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PendingRecoveryPhaseV4 {
    AwaitingFactor,
    ReadyToCommit,
    RemoteOutcomeUnknown,
    RemoteCommitted,
    LocalTransitionPending,
    Applied,
    QuarantinedKeyUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LocalTransitionStateV4 {
    NotStarted,
    Pending,
    Applied,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecoveryRetryMetadataV4 {
    pub(crate) consecutive_attempts: u32,
    pub(crate) next_retry_at: Option<String>,
    pub(crate) last_retryable_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecoveryCheckpointV4 {
    pub(crate) document_version: u64,
    pub(crate) document_hash: String,
    pub(crate) registry_version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecoveryBootstrapDeviceV4 {
    pub(crate) device_id: String,
    pub(crate) status: String,
    pub(crate) role: String,
    pub(crate) management_ready: bool,
    pub(crate) auth_generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecoveryRemoteResultV4 {
    pub(crate) state: String,
    pub(crate) operation_id: String,
    pub(crate) intent_hash: String,
    pub(crate) intent_schema_version: String,
    pub(crate) contract_version: String,
    pub(crate) account_user_id: String,
    pub(crate) full_handle: String,
    pub(crate) previous_did: String,
    pub(crate) current_did: String,
    pub(crate) binding_generation: String,
    pub(crate) checkpoint: RecoveryCheckpointV4,
    pub(crate) bootstrap_device: RecoveryBootstrapDeviceV4,
    pub(crate) committed_at: String,
}

impl RecoveryRemoteResultV4 {
    pub(crate) fn validate_against(
        &self,
        operation_id: &str,
        intent_hash: &str,
    ) -> crate::ImResult<()> {
        if self.state != "recovered"
            || self.operation_id != operation_id
            || self.intent_hash != intent_hash
            || self.intent_schema_version != "1"
            || self.contract_version != V4_CONTRACT_VERSION
            || self.account_user_id.trim().is_empty()
            || crate::internal::identity_wire::handle_recovery::canonical_handle(&self.full_handle)
                .is_err()
            || self.previous_did == self.current_did
            || !canonical_generation(&self.binding_generation)
            || self.checkpoint.document_version == 0
            || self.checkpoint.registry_version == 0
            || !valid_sha256_digest(&self.checkpoint.document_hash)
            || self.bootstrap_device.device_id.trim().is_empty()
            || self.bootstrap_device.status != "active"
            || self.bootstrap_device.role != "admin"
            || !self.bootstrap_device.management_ready
            || self.bootstrap_device.auth_generation != 1
            || !is_exact_rfc3339_second_z(&self.committed_at)
        {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(())
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PendingHandleRecoveryV4 {
    schema_version: u32,
    contract_version: String,
    contract_hash: String,
    pub(crate) revision: u64,
    pub(crate) operation_id: String,
    pub(crate) owner_identity_id: String,
    pub(crate) local_alias: String,
    pub(crate) display_name: String,
    pub(crate) make_default: bool,
    #[serde(default)]
    pub(crate) fresh_local_state: bool,
    pub(crate) full_handle: String,
    pub(crate) local_previous_did: String,
    pub(crate) generated: crate::internal::identity_generation::GeneratedHandleRecoveryIdentity,
    pub(crate) factor_state: RecoveryFactorStateV4,
    pub(crate) authoritative_binding: Option<RecoveryAuthoritativeBindingV4>,
    pub(crate) intent: Option<RecoveryIntentV4>,
    pub(crate) intent_hash: Option<String>,
    recovery_grant: Option<String>,
    pub(crate) grant_expires_at: Option<String>,
    pub(crate) commit_attempted: bool,
    pub(crate) last_commit_attempt_at: Option<String>,
    pub(crate) last_result_get_at: Option<String>,
    pub(crate) remote_result: Option<RecoveryRemoteResultV4>,
    pub(crate) local_transition_state: LocalTransitionStateV4,
    pub(crate) retry_metadata: RecoveryRetryMetadataV4,
    pub(crate) phase: PendingRecoveryPhaseV4,
    pub(crate) last_error_code: Option<String>,
}

impl std::fmt::Debug for PendingHandleRecoveryV4 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PendingHandleRecoveryV4")
            .field("revision", &self.revision)
            .field("operation_id", &self.operation_id)
            .field("owner_identity_id", &self.owner_identity_id)
            .field("full_handle", &self.full_handle)
            .field("generated", &"<redacted-generated-identity>")
            .field("factor_state", &self.factor_state)
            .field("has_intent", &self.intent.is_some())
            .field("recovery_grant", &"<redacted>")
            .field("commit_attempted", &self.commit_attempted)
            .field("last_commit_attempt_at", &self.last_commit_attempt_at)
            .field("last_result_get_at", &self.last_result_get_at)
            .field("has_remote_result", &self.remote_result.is_some())
            .field("phase", &self.phase)
            .field("last_error_code", &self.last_error_code)
            .finish()
    }
}

impl PendingHandleRecoveryV4 {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new_pre_otp(
        operation_id: String,
        owner_identity_id: String,
        local_alias: String,
        display_name: String,
        make_default: bool,
        fresh_local_state: bool,
        full_handle: String,
        local_previous_did: String,
        generated: crate::internal::identity_generation::GeneratedHandleRecoveryIdentity,
    ) -> crate::ImResult<Self> {
        let value = Self {
            schema_version: V4_SCHEMA_VERSION,
            contract_version: V4_CONTRACT_VERSION.to_owned(),
            contract_hash: V4_CONTRACT_HASH.to_owned(),
            revision: 1,
            operation_id,
            owner_identity_id,
            local_alias,
            display_name,
            make_default,
            fresh_local_state,
            full_handle,
            local_previous_did,
            generated,
            factor_state: RecoveryFactorStateV4::AwaitingOtp,
            authoritative_binding: None,
            intent: None,
            intent_hash: None,
            recovery_grant: None,
            grant_expires_at: None,
            commit_attempted: false,
            last_commit_attempt_at: None,
            last_result_get_at: None,
            remote_result: None,
            local_transition_state: LocalTransitionStateV4::NotStarted,
            retry_metadata: RecoveryRetryMetadataV4 {
                consecutive_attempts: 0,
                next_retry_at: None,
                last_retryable_code: None,
            },
            phase: PendingRecoveryPhaseV4::AwaitingFactor,
            last_error_code: None,
        };
        value.validate()?;
        Ok(value)
    }

    pub(crate) fn freeze_exchange(
        &mut self,
        authoritative_binding: RecoveryAuthoritativeBindingV4,
        recovery_grant: String,
        grant_expires_at: String,
    ) -> crate::ImResult<()> {
        if self.factor_state != RecoveryFactorStateV4::AwaitingOtp
            || self.intent.is_some()
            || recovery_grant.trim().is_empty()
            || grant_expires_at.trim().is_empty()
        {
            return Err(crate::ImError::PermissionDenied);
        }
        if self.fresh_local_state {
            self.local_previous_did = authoritative_binding.current_did.clone();
        }
        let signing_public_key = generated_signing_public_jwk(&self.generated)?;
        let intent = RecoveryIntentV4 {
            schema_version: "1".to_owned(),
            contract_version: V4_CONTRACT_VERSION.to_owned(),
            operation_id: self.operation_id.clone(),
            account_user_id: authoritative_binding.account_user_id.clone(),
            full_handle: authoritative_binding.full_handle.clone(),
            expected_previous_did: authoritative_binding.current_did.clone(),
            expected_binding_generation: authoritative_binding.binding_generation.clone(),
            new_did: self.generated.did.as_str().to_owned(),
            new_did_document_hash: v4_did_document_hash(&self.generated.did_document)?,
            bootstrap_device_id: self.generated.protocol_device_id.as_str().to_owned(),
            bootstrap_signing_key_id: self.generated.device_signing_key_id.clone(),
            bootstrap_signing_public_key: signing_public_key,
        };
        intent.validate()?;
        let intent_hash = intent.hash()?;
        if authoritative_binding.full_handle != self.full_handle {
            return Err(crate::ImError::PermissionDenied);
        }
        self.authoritative_binding = Some(authoritative_binding);
        self.intent = Some(intent);
        self.intent_hash = Some(intent_hash);
        self.recovery_grant = Some(recovery_grant);
        self.grant_expires_at = Some(grant_expires_at);
        self.factor_state = RecoveryFactorStateV4::Exchanged;
        self.phase = PendingRecoveryPhaseV4::ReadyToCommit;
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or(crate::ImError::PermissionDenied)?;
        self.validate()
    }

    pub(crate) fn recovery_grant(&self) -> crate::ImResult<SecretBytes> {
        self.recovery_grant
            .as_ref()
            .filter(|grant| !grant.trim().is_empty())
            .map(|grant| SecretBytes::from_vec(grant.as_bytes().to_vec()))
            .ok_or(crate::ImError::PermissionDenied)
    }

    pub(crate) fn bootstrap_signing_public_key(&self) -> crate::ImResult<serde_json::Value> {
        generated_signing_public_jwk(&self.generated)
    }

    /// Reissues only the expiring Grant/JTI. The immutable intent and its
    /// authoritative binding snapshot must remain byte-for-byte unchanged.
    pub(crate) fn refresh_grant(
        &mut self,
        authoritative_binding: &RecoveryAuthoritativeBindingV4,
        recovery_grant: String,
        grant_expires_at: String,
    ) -> crate::ImResult<()> {
        if self.factor_state != RecoveryFactorStateV4::Exchanged
            || self.authoritative_binding.as_ref() != Some(authoritative_binding)
            || recovery_grant.trim().is_empty()
            || grant_expires_at.trim().is_empty()
        {
            return Err(crate::ImError::PermissionDenied);
        }
        let frozen_hash = self.intent_hash.clone();
        self.recovery_grant = Some(recovery_grant);
        self.grant_expires_at = Some(grant_expires_at);
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or(crate::ImError::PermissionDenied)?;
        self.validate()?;
        if self.intent_hash != frozen_hash {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(())
    }

    pub(crate) fn mark_commit_attempted(&mut self, attempted_at: String) -> crate::ImResult<()> {
        if self.factor_state != RecoveryFactorStateV4::Exchanged || self.intent.is_none() {
            return Err(crate::ImError::PermissionDenied);
        }
        if attempted_at.trim().is_empty() {
            return Err(crate::ImError::PermissionDenied);
        }
        self.commit_attempted = true;
        self.last_commit_attempt_at = Some(attempted_at);
        self.retry_metadata.consecutive_attempts =
            self.retry_metadata.consecutive_attempts.saturating_add(1);
        self.phase = PendingRecoveryPhaseV4::RemoteOutcomeUnknown;
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or(crate::ImError::PermissionDenied)?;
        self.validate()
    }

    pub(crate) fn record_result_get(
        &mut self,
        attempted_at: String,
        next_retry_at: Option<String>,
        retryable_code: Option<String>,
    ) -> crate::ImResult<()> {
        if !self.commit_attempted || attempted_at.trim().is_empty() {
            return Err(crate::ImError::PermissionDenied);
        }
        self.last_result_get_at = Some(attempted_at);
        self.retry_metadata.consecutive_attempts =
            self.retry_metadata.consecutive_attempts.saturating_add(1);
        self.retry_metadata.next_retry_at = next_retry_at;
        self.retry_metadata.last_retryable_code = retryable_code.clone();
        self.last_error_code = retryable_code.filter(|code| code != "committed");
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or(crate::ImError::PermissionDenied)?;
        self.validate()
    }

    pub(crate) fn record_retryable_error(&mut self, code: String) -> crate::ImResult<()> {
        if code.trim().is_empty() {
            return Err(crate::ImError::PermissionDenied);
        }
        self.retry_metadata.last_retryable_code = Some(code.clone());
        self.last_error_code = Some(code);
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or(crate::ImError::PermissionDenied)?;
        self.validate()
    }

    pub(crate) fn record_remote_result(
        &mut self,
        result: RecoveryRemoteResultV4,
    ) -> crate::ImResult<()> {
        if !self.commit_attempted {
            return Err(crate::ImError::PermissionDenied);
        }
        self.validate_remote_result(&result)?;
        if let Some(existing) = &self.remote_result {
            if existing != &result {
                return Err(crate::ImError::PermissionDenied);
            }
            return Ok(());
        }
        self.remote_result = Some(result);
        self.last_error_code = None;
        self.retry_metadata.last_retryable_code = None;
        self.retry_metadata.next_retry_at = None;
        self.phase = PendingRecoveryPhaseV4::RemoteCommitted;
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or(crate::ImError::PermissionDenied)?;
        self.validate()
    }

    pub(crate) fn mark_local_transition_pending(&mut self) -> crate::ImResult<()> {
        if self.remote_result.is_none() {
            return Err(crate::ImError::PermissionDenied);
        }
        self.local_transition_state = LocalTransitionStateV4::Pending;
        self.phase = PendingRecoveryPhaseV4::LocalTransitionPending;
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or(crate::ImError::PermissionDenied)?;
        self.validate()
    }

    pub(crate) fn mark_applied(&mut self) -> crate::ImResult<()> {
        if self.remote_result.is_none()
            || self.local_transition_state != LocalTransitionStateV4::Pending
        {
            return Err(crate::ImError::PermissionDenied);
        }
        self.local_transition_state = LocalTransitionStateV4::Applied;
        self.phase = PendingRecoveryPhaseV4::Applied;
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or(crate::ImError::PermissionDenied)?;
        self.validate()
    }

    pub(crate) fn validate(&self) -> crate::ImResult<()> {
        if self.schema_version != V4_SCHEMA_VERSION
            || self.contract_version != V4_CONTRACT_VERSION
            || self.contract_hash != V4_CONTRACT_HASH
            || self.revision == 0
            || self.owner_identity_id.trim().is_empty()
            || self.local_alias.trim().is_empty()
            || self.display_name.trim().is_empty()
            || self.generated.did.as_str() == self.local_previous_did
            || self.full_handle.len() > 320
            || crate::internal::identity_wire::handle_recovery::canonical_handle(&self.full_handle)
                .is_err()
            || crate::internal::identity_wire::handle_recovery::validate_operation_id(
                &self.operation_id,
            )
            .is_err()
        {
            return Err(crate::ImError::PermissionDenied);
        }
        match self.factor_state {
            RecoveryFactorStateV4::AwaitingOtp
                if self.authoritative_binding.is_none()
                    && self.intent.is_none()
                    && self.intent_hash.is_none()
                    && self.recovery_grant.is_none()
                    && self.grant_expires_at.is_none()
                    && !self.commit_attempted
                    && self.last_commit_attempt_at.is_none()
                    && self.last_result_get_at.is_none()
                    && self.local_transition_state == LocalTransitionStateV4::NotStarted
                    && self.retry_metadata.consecutive_attempts == 0
                    && self.phase == PendingRecoveryPhaseV4::AwaitingFactor => {}
            RecoveryFactorStateV4::Exchanged => {
                let binding = self
                    .authoritative_binding
                    .as_ref()
                    .ok_or(crate::ImError::PermissionDenied)?;
                let intent = self
                    .intent
                    .as_ref()
                    .ok_or(crate::ImError::PermissionDenied)?;
                intent.validate()?;
                let calculated_intent_hash = intent.hash()?;
                if let Some(result) = &self.remote_result {
                    self.validate_remote_result(result)?;
                }
                let expected_document_hash = v4_did_document_hash(&self.generated.did_document)?;
                let expected_public_key = generated_signing_public_jwk(&self.generated)?;
                if binding.account_user_id != intent.account_user_id
                    || binding.full_handle != intent.full_handle
                    || binding.current_did != intent.expected_previous_did
                    || binding.binding_generation != intent.expected_binding_generation
                    || intent.operation_id != self.operation_id
                    || intent.full_handle != self.full_handle
                    || intent.new_did != self.generated.did.as_str()
                    || intent.new_did_document_hash != expected_document_hash
                    || intent.bootstrap_device_id != self.generated.protocol_device_id.as_str()
                    || intent.bootstrap_signing_key_id != self.generated.device_signing_key_id
                    || intent.bootstrap_signing_public_key != expected_public_key
                    || self.intent_hash.as_deref() != Some(calculated_intent_hash.as_str())
                    || self
                        .recovery_grant
                        .as_deref()
                        .unwrap_or("")
                        .trim()
                        .is_empty()
                    || self
                        .grant_expires_at
                        .as_deref()
                        .unwrap_or("")
                        .trim()
                        .is_empty()
                    || !is_exact_rfc3339_second_z(self.grant_expires_at.as_deref().unwrap_or(""))
                    || (self.commit_attempted
                        != (self.phase != PendingRecoveryPhaseV4::ReadyToCommit))
                    || (self.commit_attempted != self.last_commit_attempt_at.is_some())
                    || (self.last_result_get_at.is_some() && !self.commit_attempted)
                    || (self.remote_result.is_none()
                        && self.local_transition_state != LocalTransitionStateV4::NotStarted)
                    || (self.local_transition_state == LocalTransitionStateV4::Pending
                        && self.phase != PendingRecoveryPhaseV4::LocalTransitionPending)
                    || (self.local_transition_state == LocalTransitionStateV4::Applied
                        && self.phase != PendingRecoveryPhaseV4::Applied)
                {
                    return Err(crate::ImError::PermissionDenied);
                }
            }
            _ => return Err(crate::ImError::PermissionDenied),
        }
        Ok(())
    }

    fn validate_remote_result(&self, result: &RecoveryRemoteResultV4) -> crate::ImResult<()> {
        let intent = self
            .intent
            .as_ref()
            .ok_or(crate::ImError::PermissionDenied)?;
        let intent_hash = self
            .intent_hash
            .as_deref()
            .ok_or(crate::ImError::PermissionDenied)?;
        result.validate_against(&self.operation_id, intent_hash)?;
        let expected_generation =
            increment_canonical_generation(&intent.expected_binding_generation)
                .ok_or(crate::ImError::PermissionDenied)?;
        let expected_document_hash =
            crate::internal::identity_wire::document::document_hash(&self.generated.did_document)?;
        if result.account_user_id != intent.account_user_id
            || result.full_handle != intent.full_handle
            || result.previous_did != intent.expected_previous_did
            || result.current_did != intent.new_did
            || result.binding_generation != expected_generation
            || result.checkpoint.document_hash != expected_document_hash
            || result.bootstrap_device.device_id != intent.bootstrap_device_id
        {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(())
    }
}

fn generated_signing_public_jwk(
    generated: &crate::internal::identity_generation::GeneratedHandleRecoveryIdentity,
) -> crate::ImResult<serde_json::Value> {
    let public = anp::PublicKeyMaterial::from_pem(&generated.device_signing_public_pem)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    let anp::PublicKeyMaterial::Ed25519(public) = public else {
        return Err(crate::ImError::PermissionDenied);
    };
    Ok(serde_json::json!({
        "kty": "OKP",
        "crv": "Ed25519",
        "x": URL_SAFE_NO_PAD.encode(public.to_bytes()),
    }))
}

fn v4_did_document_hash(document: &serde_json::Value) -> crate::ImResult<String> {
    let mut projection = document.clone();
    projection
        .as_object_mut()
        .ok_or(crate::ImError::PermissionDenied)?
        .remove("proof");
    crate::internal::identity_wire::document::document_hash(&projection)
}

fn canonical_ed25519_public_jwk(value: &serde_json::Value) -> crate::ImResult<serde_json::Value> {
    let object = value.as_object().ok_or(crate::ImError::PermissionDenied)?;
    if object.len() != 3
        || object.get("kty").and_then(serde_json::Value::as_str) != Some("OKP")
        || object.get("crv").and_then(serde_json::Value::as_str) != Some("Ed25519")
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let x = object
        .get("x")
        .and_then(serde_json::Value::as_str)
        .ok_or(crate::ImError::PermissionDenied)?;
    let decoded = URL_SAFE_NO_PAD
        .decode(x)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    if decoded.len() != 32 || URL_SAFE_NO_PAD.encode(decoded) != x {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(serde_json::json!({"kty":"OKP","crv":"Ed25519","x":x}))
}

fn valid_sha256_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .and_then(|encoded| URL_SAFE_NO_PAD.decode(encoded).ok())
        .is_some_and(|bytes| bytes.len() == 32)
}

fn is_exact_rfc3339_second_z(value: &str) -> bool {
    value.len() == 20
        && value.ends_with('Z')
        && time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
            .is_ok()
}

pub(crate) struct PendingHandleRecoveryStore {
    workspace_id: String,
    device_id: String,
    vault: std::sync::Arc<dyn SecretVault + Send + Sync>,
}

impl PendingHandleRecoveryStore {
    pub(crate) fn from_core(core: &crate::core::ImCore) -> crate::ImResult<Self> {
        if core.inner().identity_secret_storage_policy()
            != crate::core::IdentitySecretStoragePolicy::VaultRequired
        {
            return Err(crate::ImError::LocalStateUnavailable {
                detail: "Handle Recovery requires IdentitySecretStoragePolicy::VaultRequired"
                    .to_owned(),
            });
        }
        let context =
            core.inner()
                .identity_vault()
                .ok_or_else(|| crate::ImError::LocalStateUnavailable {
                    detail: "Handle Recovery requires an available identity SecretVault".to_owned(),
                })?;
        Ok(Self {
            workspace_id: context.workspace_id().to_owned(),
            device_id: context.vault_context_device_id().as_str().to_owned(),
            vault: context.vault(),
        })
    }

    pub(crate) fn load_v4(
        &self,
        operation_id: &str,
    ) -> crate::ImResult<Option<(SecretRef, PendingHandleRecoveryV4)>> {
        crate::internal::identity_wire::handle_recovery::validate_operation_id(operation_id)?;
        let matches = self
            .vault
            .list()?
            .into_iter()
            .filter(|secret_ref| {
                secret_ref.workspace_id == self.workspace_id
                    && secret_ref.device_id == self.device_id
                    && secret_ref.kind == SecretKind::IdentityHandleRecoveryPending
                    && secret_ref.key_id == pending_v4_key_id(operation_id)
                    && secret_ref.key_version == V4_KEY_VERSION
            })
            .collect::<Vec<_>>();
        if matches.len() > 1 {
            return Err(crate::ImError::PermissionDenied);
        }
        let Some(secret_ref) = matches.into_iter().next() else {
            return Ok(None);
        };
        let plaintext = self.vault.open(&secret_ref)?;
        let pending: PendingHandleRecoveryV4 = serde_json::from_slice(plaintext.expose_secret())
            .map_err(|_| crate::ImError::PermissionDenied)?;
        pending.validate()?;
        if pending.operation_id != operation_id {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(Some((secret_ref, pending)))
    }

    pub(crate) fn list_v4_for_owner(
        &self,
        owner_identity_id: &str,
    ) -> crate::ImResult<Vec<(SecretRef, PendingHandleRecoveryV4)>> {
        let mut matches = Vec::new();
        for secret_ref in self.vault.list()?.into_iter().filter(|secret_ref| {
            secret_ref.workspace_id == self.workspace_id
                && secret_ref.device_id == self.device_id
                && secret_ref.kind == SecretKind::IdentityHandleRecoveryPending
                && secret_ref.key_version == V4_KEY_VERSION
                && secret_ref.identity_id.as_deref() == Some(owner_identity_id)
        }) {
            let plaintext = self.vault.open(&secret_ref)?;
            let pending: PendingHandleRecoveryV4 =
                serde_json::from_slice(plaintext.expose_secret())
                    .map_err(|_| crate::ImError::PermissionDenied)?;
            pending.validate()?;
            if pending.owner_identity_id != owner_identity_id
                || secret_ref.key_id != pending_v4_key_id(&pending.operation_id)
                || secret_ref.did.as_deref() != Some(pending.generated.did.as_str())
            {
                return Err(crate::ImError::PermissionDenied);
            }
            matches.push((secret_ref, pending));
        }
        matches.sort_by(|left, right| left.1.operation_id.cmp(&right.1.operation_id));
        Ok(matches)
    }

    pub(crate) fn list_v4_for_handle(
        &self,
        full_handle: &str,
    ) -> crate::ImResult<Vec<(SecretRef, PendingHandleRecoveryV4)>> {
        let mut matches = Vec::new();
        for secret_ref in self.vault.list()?.into_iter().filter(|secret_ref| {
            secret_ref.workspace_id == self.workspace_id
                && secret_ref.device_id == self.device_id
                && secret_ref.kind == SecretKind::IdentityHandleRecoveryPending
                && secret_ref.key_version == V4_KEY_VERSION
        }) {
            let plaintext = self.vault.open(&secret_ref)?;
            let pending: PendingHandleRecoveryV4 =
                serde_json::from_slice(plaintext.expose_secret())
                    .map_err(|_| crate::ImError::PermissionDenied)?;
            pending.validate()?;
            if secret_ref.identity_id.as_deref() != Some(pending.owner_identity_id.as_str())
                || secret_ref.key_id != pending_v4_key_id(&pending.operation_id)
                || secret_ref.did.as_deref() != Some(pending.generated.did.as_str())
            {
                return Err(crate::ImError::PermissionDenied);
            }
            if pending.full_handle == full_handle {
                matches.push((secret_ref, pending));
            }
        }
        matches.sort_by(|left, right| left.1.operation_id.cmp(&right.1.operation_id));
        Ok(matches)
    }

    /// Creates the pre-OTP journal without replacing an operation that already
    /// exists. The caller persists the SQLite operation index immediately after
    /// this succeeds and before making the OTP network request.
    pub(crate) fn create_v4(
        &self,
        pending: &PendingHandleRecoveryV4,
    ) -> crate::ImResult<SecretRef> {
        use crate::internal::secret_vault::SealIfAbsentResult;

        pending.validate()?;
        if pending.revision != 1
            || pending.factor_state != RecoveryFactorStateV4::AwaitingOtp
            || pending.commit_attempted
        {
            return Err(crate::ImError::PermissionDenied);
        }
        let result = self.vault.seal_if_absent(SealSecretRequest {
            metadata: self.v4_metadata(pending),
            plaintext: serialize_v4(pending)?,
        })?;
        match result {
            SealIfAbsentResult::Sealed(secret_ref) => Ok(secret_ref),
            SealIfAbsentResult::AlreadyExists(_) => Err(crate::ImError::PermissionDenied),
        }
    }

    /// Application-level revision CAS. Runtime owner locking serializes writers
    /// inside one Core process; the revision check closes stale restart writes.
    pub(crate) fn save_v4_cas(
        &self,
        pending: &PendingHandleRecoveryV4,
        expected_revision: u64,
    ) -> crate::ImResult<SecretRef> {
        pending.validate()?;
        if pending.revision != expected_revision.saturating_add(1) {
            return Err(crate::ImError::PermissionDenied);
        }
        let (_, current) = self
            .load_v4(&pending.operation_id)?
            .ok_or(crate::ImError::PermissionDenied)?;
        if current.revision != expected_revision {
            return Err(crate::ImError::PermissionDenied);
        }
        self.vault.seal(SealSecretRequest {
            metadata: self.v4_metadata(pending),
            plaintext: serialize_v4(pending)?,
        })
    }

    /// Destroys private material only while both durable authorities still say
    /// that no Commit attempt was made. Post-attempt operations are reconciled,
    /// never abandoned.
    pub(crate) fn delete_v4_pre_attempt(&self, operation_id: &str) -> crate::ImResult<()> {
        let Some((secret_ref, pending)) = self.load_v4(operation_id)? else {
            return Ok(());
        };
        if pending.commit_attempted {
            return Err(crate::ImError::PermissionDenied);
        }
        self.vault.delete(&secret_ref)
    }

    fn v4_metadata(&self, pending: &PendingHandleRecoveryV4) -> SecretMetadata {
        SecretMetadata {
            workspace_id: self.workspace_id.clone(),
            device_id: self.device_id.clone(),
            identity_id: Some(pending.owner_identity_id.clone()),
            did: Some(pending.generated.did.as_str().to_owned()),
            kind: SecretKind::IdentityHandleRecoveryPending,
            key_id: pending_v4_key_id(&pending.operation_id),
            key_version: V4_KEY_VERSION,
            policy: SecretAccessPolicy::no_prompt_local_secret(),
        }
    }
}

fn serialize_v4(pending: &PendingHandleRecoveryV4) -> crate::ImResult<SecretBytes> {
    serde_json::to_vec(pending)
        .map(SecretBytes::from_vec)
        .map_err(|error| crate::ImError::Serialization {
            detail: error.to_string(),
        })
}

pub(crate) fn pending_v4_key_id(operation_id: &str) -> String {
    format!(
        "handle-recovery-v4-{}",
        URL_SAFE_NO_PAD.encode(Sha256::digest(operation_id.as_bytes()))
    )
}

fn canonical_generation(value: &str) -> bool {
    !value.is_empty()
        && value.len()
            <= crate::internal::identity_wire::handle_recovery::MAX_BINDING_GENERATION_DIGITS
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value.as_bytes()[0] != b'0'
}

pub(crate) fn increment_canonical_generation(value: &str) -> Option<String> {
    if !canonical_generation(value) {
        return None;
    }
    let mut bytes = value.as_bytes().to_vec();
    for digit in bytes.iter_mut().rev() {
        if *digit == b'9' {
            *digit = b'0';
        } else {
            *digit += 1;
            return String::from_utf8(bytes).ok();
        }
    }
    let mut next = Vec::with_capacity(bytes.len() + 1);
    next.push(b'1');
    next.extend(bytes);
    (next.len() <= crate::internal::identity_wire::handle_recovery::MAX_BINDING_GENERATION_DIGITS)
        .then(|| String::from_utf8(next).ok())
        .flatten()
}

pub(crate) fn previous_canonical_generation(value: &str) -> Option<String> {
    if !canonical_generation(value) || value == "1" {
        return None;
    }
    let mut bytes = value.as_bytes().to_vec();
    for index in (0..bytes.len()).rev() {
        if bytes[index] == b'0' {
            bytes[index] = b'9';
        } else {
            bytes[index] -= 1;
            break;
        }
    }
    if bytes.first() == Some(&b'0') {
        bytes.remove(0);
    }
    String::from_utf8(bytes).ok()
}

#[cfg(test)]
mod tests {
    fn v4_pending() -> super::PendingHandleRecoveryV4 {
        let generated = crate::internal::identity_generation::generate_handle_recovery_identity(
            "example.invalid",
            "alice",
            None,
            None,
        )
        .unwrap();
        super::PendingHandleRecoveryV4::new_pre_otp(
            "op_v4_12345678".to_owned(),
            "owner-1".to_owned(),
            "alice".to_owned(),
            "Alice".to_owned(),
            true,
            false,
            "alice.example.invalid".to_owned(),
            "did:wba:example.invalid:user:alice:old".to_owned(),
            generated,
        )
        .unwrap()
    }

    #[test]
    fn contract_identity_is_frozen() {
        assert_eq!(
            super::V4_CONTRACT_HASH,
            "173d53051fc690f35f958bff7f08a51fd8458c729230d33563a16e0db1db3b84"
        );
    }

    #[test]
    fn v4_generation_uses_the_frozen_255_digit_decimal_profile() {
        let max_non_overflowing = format!("8{}", "9".repeat(254));
        let successor = super::increment_canonical_generation(&max_non_overflowing).unwrap();
        assert_eq!(successor.len(), 255);
        assert_eq!(
            super::previous_canonical_generation(&successor),
            Some(max_non_overflowing)
        );
        assert_eq!(
            super::increment_canonical_generation(&"9".repeat(255)),
            None
        );
        assert_eq!(
            super::increment_canonical_generation(&"1".repeat(256)),
            None
        );
        assert_eq!(super::previous_canonical_generation(&"1".repeat(256)), None);
    }

    #[test]
    fn v4_journal_exists_before_factor_and_freezes_authoritative_intent_once() {
        let mut pending = v4_pending();
        assert_eq!(
            pending.factor_state,
            super::RecoveryFactorStateV4::AwaitingOtp
        );
        assert!(pending.intent.is_none());
        pending
            .freeze_exchange(
                super::RecoveryAuthoritativeBindingV4 {
                    account_user_id: "user-1".to_owned(),
                    full_handle: "alice.example.invalid".to_owned(),
                    current_did: pending.local_previous_did.clone(),
                    binding_generation: "7".to_owned(),
                },
                "secret-grant".to_owned(),
                "2026-08-07T00:05:00Z".to_owned(),
            )
            .unwrap();
        let intent = pending.intent.as_ref().unwrap();
        assert_eq!(intent.expected_binding_generation, "7");
        assert_eq!(
            pending.intent_hash.as_deref(),
            Some(intent.hash().unwrap().as_str())
        );
        assert!(pending
            .freeze_exchange(
                pending.authoritative_binding.clone().unwrap(),
                "another-grant".to_owned(),
                "2026-08-07T00:06:00Z".to_owned(),
            )
            .is_err());
        let frozen_hash = pending.intent_hash.clone();
        pending
            .refresh_grant(
                &pending.authoritative_binding.clone().unwrap(),
                "refreshed-grant".to_owned(),
                "2026-08-07T00:07:00Z".to_owned(),
            )
            .unwrap();
        assert_eq!(pending.intent_hash, frozen_hash);
        let mut changed_binding = pending.authoritative_binding.clone().unwrap();
        changed_binding.binding_generation = "8".to_owned();
        assert!(pending
            .refresh_grant(
                &changed_binding,
                "invalid-grant".to_owned(),
                "2026-08-07T00:08:00Z".to_owned(),
            )
            .is_err());
    }

    #[test]
    fn fresh_machine_freezes_the_authoritative_previous_did() {
        let mut pending = v4_pending();
        pending.fresh_local_state = true;
        pending.local_previous_did = format!("{}:unbound", pending.generated.did.as_str());
        let authoritative_previous = "did:wba:example.invalid:user:alice:remote";

        pending
            .freeze_exchange(
                super::RecoveryAuthoritativeBindingV4 {
                    account_user_id: "user-1".to_owned(),
                    full_handle: "alice.example.invalid".to_owned(),
                    current_did: authoritative_previous.to_owned(),
                    binding_generation: "7".to_owned(),
                },
                "secret-grant".to_owned(),
                "2026-08-07T00:05:00Z".to_owned(),
            )
            .unwrap();

        assert_eq!(pending.local_previous_did, authoritative_previous);
        assert_eq!(
            pending.intent.as_ref().unwrap().expected_previous_did,
            authoritative_previous
        );
    }

    #[test]
    fn v4_commit_attempt_is_monotonic_in_the_vault_journal() {
        let mut pending = v4_pending();
        pending
            .freeze_exchange(
                super::RecoveryAuthoritativeBindingV4 {
                    account_user_id: "user-1".to_owned(),
                    full_handle: "alice.example.invalid".to_owned(),
                    current_did: pending.local_previous_did.clone(),
                    binding_generation: "7".to_owned(),
                },
                "secret-grant".to_owned(),
                "2026-08-07T00:05:00Z".to_owned(),
            )
            .unwrap();
        pending
            .mark_commit_attempted("2026-08-07T00:01:00Z".to_owned())
            .unwrap();
        pending
            .record_result_get(
                "2026-08-07T00:01:05Z".to_owned(),
                Some("2026-08-07T00:01:15Z".to_owned()),
                Some("result_absent".to_owned()),
            )
            .unwrap();
        assert!(pending.commit_attempted);
        assert_eq!(
            pending.phase,
            super::PendingRecoveryPhaseV4::RemoteOutcomeUnknown
        );
        assert_eq!(pending.retry_metadata.consecutive_attempts, 2);
        assert_eq!(
            pending.retry_metadata.last_retryable_code.as_deref(),
            Some("result_absent")
        );
        assert!(pending.validate().is_ok());
    }

    #[test]
    fn v4_remote_result_is_closed_over_the_frozen_intent_and_generated_identity() {
        let mut pending = v4_pending();
        pending
            .freeze_exchange(
                super::RecoveryAuthoritativeBindingV4 {
                    account_user_id: "user-1".to_owned(),
                    full_handle: pending.full_handle.clone(),
                    current_did: pending.local_previous_did.clone(),
                    binding_generation: "99999999999999999999999999999999999999".to_owned(),
                },
                "secret-grant".to_owned(),
                "2026-08-07T00:05:00Z".to_owned(),
            )
            .unwrap();
        pending
            .mark_commit_attempted("2026-08-07T00:01:00Z".to_owned())
            .unwrap();
        let intent = pending.intent.as_ref().unwrap();
        let result = super::RecoveryRemoteResultV4 {
            state: "recovered".to_owned(),
            operation_id: pending.operation_id.clone(),
            intent_hash: pending.intent_hash.clone().unwrap(),
            intent_schema_version: "1".to_owned(),
            contract_version: super::V4_CONTRACT_VERSION.to_owned(),
            account_user_id: intent.account_user_id.clone(),
            full_handle: intent.full_handle.clone(),
            previous_did: intent.expected_previous_did.clone(),
            current_did: intent.new_did.clone(),
            binding_generation: "100000000000000000000000000000000000000".to_owned(),
            checkpoint: super::RecoveryCheckpointV4 {
                document_version: 1,
                document_hash: crate::internal::identity_wire::document::document_hash(
                    &pending.generated.did_document,
                )
                .unwrap(),
                registry_version: 1,
            },
            bootstrap_device: super::RecoveryBootstrapDeviceV4 {
                device_id: intent.bootstrap_device_id.clone(),
                status: "active".to_owned(),
                role: "admin".to_owned(),
                management_ready: true,
                auth_generation: 1,
            },
            committed_at: "2026-08-07T00:01:01Z".to_owned(),
        };
        pending.record_remote_result(result.clone()).unwrap();

        for tampered in [
            {
                let mut value = result.clone();
                value.account_user_id = "user-other".to_owned();
                value
            },
            {
                let mut value = result.clone();
                value.previous_did = "did:wba:example.invalid:user:other".to_owned();
                value
            },
            {
                let mut value = result.clone();
                value.current_did = "did:wba:example.invalid:user:other-new".to_owned();
                value
            },
            {
                let mut value = result.clone();
                value.binding_generation = "100000000000000000000000000000000000001".to_owned();
                value
            },
            {
                let mut value = result.clone();
                value.checkpoint.document_hash =
                    "sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_owned();
                value
            },
            {
                let mut value = result;
                value.bootstrap_device.device_id = "device-other".to_owned();
                value
            },
        ] {
            let mut candidate = pending.clone();
            candidate.remote_result = None;
            candidate.phase = super::PendingRecoveryPhaseV4::RemoteOutcomeUnknown;
            assert!(candidate.record_remote_result(tampered).is_err());
        }
    }

    #[test]
    fn v4_intent_matches_the_frozen_contract_golden_hash() {
        let intent: super::RecoveryIntentV4 = serde_json::from_value(serde_json::json!({
            "schema_version": "1",
            "contract_version": "awiki.handle-recovery.v1.contract.4.20260807",
            "operation_id": "recover-v4-001",
            "account_user_id": "user-fixture-1",
            "full_handle": "alice.example.invalid",
            "expected_previous_did": "did:wba:example.invalid:users:alice-old",
            "expected_binding_generation": "7",
            "new_did": "did:wba:example.invalid:users:alice-new",
            "new_did_document_hash": "sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            "bootstrap_device_id": "device-fixture-new",
            "bootstrap_signing_key_id": "did:wba:example.invalid:users:alice-new#device-signing-key-1",
            "bootstrap_signing_public_key": {
                "kty": "OKP",
                "crv": "Ed25519",
                "x": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
            }
        }))
        .unwrap();
        intent.validate().unwrap();
        assert_eq!(
            intent.hash().unwrap(),
            "sha256:SlQnFpLKCK0OFEKnA2492wGZ8WsD_w35-l_wTccWbUA"
        );
    }
}
