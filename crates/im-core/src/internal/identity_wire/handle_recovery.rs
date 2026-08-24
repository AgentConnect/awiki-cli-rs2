//! Closed wire contract for same-deployment Manifest Handle Recovery v1.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};

use crate::internal::platform_secret::SecretBytes;

#[cfg(test)]
mod tests;

pub(crate) const HANDLE_RECOVERY_PURPOSE: &str = "awiki.identity.handle-recovery.v1";

pub(crate) const HANDLE_RECOVERY_V4_CONTRACT_VERSION: &str =
    "awiki.handle-recovery.v1.contract.4.20260807";
pub(crate) const HANDLE_RECOVERY_V4_EXCHANGE_ENDPOINT: &str =
    "/user-service/v1/auth/handle-recovery/v4/exchange";
pub(crate) const HANDLE_RECOVERY_COMMIT_V4_METHOD: &str = "handle_recovery_commit_v4";
pub(crate) const HANDLE_RECOVERY_RESULT_GET_V4_METHOD: &str = "handle_recovery_result_get_v4";
pub(crate) const HANDLE_RECOVERY_ATTESTATION_ISSUE_V1_METHOD: &str =
    "handle_recovery_attestation_issue_v1";
pub(crate) const HANDLE_RECOVERY_COMMIT_V4_PURPOSE: &str =
    "awiki.identity.handle-recovery.commit.v4";
pub(crate) const HANDLE_RECOVERY_RESULT_GET_V4_PURPOSE: &str =
    "awiki.identity.handle-recovery.result-get.v4";
pub(crate) const HANDLE_RECOVERY_KEY_POSSESSION_PROOF_TYPE: &str =
    "awiki-handle-recovery-key-possession-v1";
pub(crate) const HANDLE_RECOVERY_RESULT_ABSENT_PADDING: &str =
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
pub(crate) const MAX_BINDING_GENERATION_DIGITS: usize = 255;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Ed25519PublicJwkV4 {
    pub(crate) kty: String,
    pub(crate) crv: String,
    pub(crate) x: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RecoveryCurrentBindingV4 {
    pub(crate) account_user_id: String,
    pub(crate) full_handle: String,
    pub(crate) current_did: String,
    pub(crate) binding_generation: String,
}

pub(crate) struct RecoveryGrantExchangeResultV4 {
    pub(crate) recovery_grant: SecretBytes,
    pub(crate) expires_at: String,
    pub(crate) current_binding: RecoveryCurrentBindingV4,
}

impl std::fmt::Debug for RecoveryGrantExchangeResultV4 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RecoveryGrantExchangeResultV4")
            .field("recovery_grant", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .field("current_binding", &self.current_binding)
            .finish()
    }
}

pub(crate) struct KeyPossessionProofInputV4<'a> {
    pub(crate) intent: &'a crate::internal::identity_handle_recovery_pending::RecoveryIntentV4,
    pub(crate) intent_hash: &'a str,
    pub(crate) audience: &'a str,
    pub(crate) created_at: &'a str,
    pub(crate) expires_at: &'a str,
    pub(crate) nonce: &'a [u8],
}

pub(crate) struct PendingCommitV4 {
    intent: crate::internal::identity_handle_recovery_pending::RecoveryIntentV4,
    intent_hash: String,
    recovery_grant: SecretBytes,
    new_did_document: Value,
    proof: PendingKeyPossessionProofV4,
}

impl PendingCommitV4 {
    pub(crate) fn signing_input(&self) -> &[u8] {
        &self.proof.signing_input
    }
}

pub(crate) struct PendingResultGetV4 {
    intent: crate::internal::identity_handle_recovery_pending::RecoveryIntentV4,
    intent_hash: String,
    proof: PendingKeyPossessionProofV4,
}

impl PendingResultGetV4 {
    pub(crate) fn signing_input(&self) -> &[u8] {
        &self.proof.signing_input
    }
}

struct PendingKeyPossessionProofV4 {
    proof: Value,
    signed_object: Value,
    signing_input: Vec<u8>,
}

pub(crate) struct CommitProofInputV4<'a> {
    pub(crate) proof: KeyPossessionProofInputV4<'a>,
    pub(crate) recovery_grant: SecretBytes,
    pub(crate) new_did_document: Value,
}

pub(crate) struct PreparedCommitV4 {
    pub(crate) call: super::RpcCall,
    pub(crate) intent_hash: String,
    pub(crate) signed_object: Value,
}

impl std::fmt::Debug for PreparedCommitV4 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedCommitV4")
            .field("endpoint", &self.call.endpoint)
            .field("method", &self.call.method)
            .field("intent_hash", &self.intent_hash)
            .finish()
    }
}

pub(crate) struct PreparedResultGetV4 {
    pub(crate) call: super::RpcCall,
    pub(crate) intent_hash: String,
    pub(crate) signed_object: Value,
}

impl std::fmt::Debug for PreparedResultGetV4 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedResultGetV4")
            .field("endpoint", &self.call.endpoint)
            .field("method", &self.call.method)
            .field("intent_hash", &self.intent_hash)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RecoveryResultGetV4 {
    Committed(crate::internal::identity_handle_recovery_pending::RecoveryRemoteResultV4),
    ResultAbsent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecoveryExchangeErrorCodeV4 {
    InvalidRequest,
    FactorInvalid,
    CapabilityDisabled,
    RateLimited,
    TemporarilyUnavailable,
}

impl RecoveryExchangeErrorCodeV4 {
    pub(crate) fn parse(code: &str) -> Option<Self> {
        match code {
            "handle_recovery_exchange.invalid_request" => Some(Self::InvalidRequest),
            "handle_recovery_exchange.factor_invalid" => Some(Self::FactorInvalid),
            "handle_recovery_exchange.capability_disabled" => Some(Self::CapabilityDisabled),
            "handle_recovery_exchange.rate_limited" => Some(Self::RateLimited),
            "handle_recovery_exchange.temporarily_unavailable" => {
                Some(Self::TemporarilyUnavailable)
            }
            _ => None,
        }
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidRequest => "handle_recovery_exchange.invalid_request",
            Self::FactorInvalid => "handle_recovery_exchange.factor_invalid",
            Self::CapabilityDisabled => "handle_recovery_exchange.capability_disabled",
            Self::RateLimited => "handle_recovery_exchange.rate_limited",
            Self::TemporarilyUnavailable => "handle_recovery_exchange.temporarily_unavailable",
        }
    }

    pub(crate) const fn retryable(self) -> bool {
        matches!(self, Self::RateLimited | Self::TemporarilyUnavailable)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecoveryServerErrorCodeV4 {
    InvalidRequest,
    CapabilityDisabled,
    GrantInvalid,
    GrantExpired,
    ProofInvalid,
    IntentConflict,
    StateChangedRequiresNewOperation,
    TemporarilyUnavailable,
}

impl RecoveryServerErrorCodeV4 {
    pub(crate) fn parse(code: &str) -> Option<Self> {
        match code {
            "handle_recovery.invalid_request" => Some(Self::InvalidRequest),
            "handle_recovery.capability_disabled" => Some(Self::CapabilityDisabled),
            "handle_recovery.grant_invalid" => Some(Self::GrantInvalid),
            "handle_recovery.grant_expired" => Some(Self::GrantExpired),
            "handle_recovery.proof_invalid" => Some(Self::ProofInvalid),
            "handle_recovery.intent_conflict" => Some(Self::IntentConflict),
            "handle_recovery.state_changed_requires_new_operation" => {
                Some(Self::StateChangedRequiresNewOperation)
            }
            "handle_recovery.temporarily_unavailable" => Some(Self::TemporarilyUnavailable),
            _ => None,
        }
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidRequest => "handle_recovery.invalid_request",
            Self::CapabilityDisabled => "handle_recovery.capability_disabled",
            Self::GrantInvalid => "handle_recovery.grant_invalid",
            Self::GrantExpired => "handle_recovery.grant_expired",
            Self::ProofInvalid => "handle_recovery.proof_invalid",
            Self::IntentConflict => "handle_recovery.intent_conflict",
            Self::StateChangedRequiresNewOperation => {
                "handle_recovery.state_changed_requires_new_operation"
            }
            Self::TemporarilyUnavailable => "handle_recovery.temporarily_unavailable",
        }
    }

    pub(crate) const fn json_rpc_code(self) -> i64 {
        match self {
            Self::GrantInvalid | Self::GrantExpired => -32000,
            Self::CapabilityDisabled => -32001,
            Self::IntentConflict | Self::StateChangedRequiresNewOperation => -32003,
            Self::InvalidRequest | Self::ProofInvalid | Self::TemporarilyUnavailable => -32004,
        }
    }

    pub(crate) const fn retryable(self) -> bool {
        matches!(self, Self::GrantExpired | Self::TemporarilyUnavailable)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CanonicalHandle {
    pub(crate) local_part: String,
    pub(crate) domain: String,
    pub(crate) full: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IdentityTransition {
    pub(crate) previous_did: String,
    pub(crate) current_did: String,
    pub(crate) binding_generation: String,
}

pub(crate) struct AccountVerificationResult {
    pub(crate) account_verification_token: SecretBytes,
    pub(crate) expires_at: String,
    pub(crate) account_user_id: Option<String>,
    pub(crate) handle: Option<String>,
    pub(crate) did: Option<String>,
    pub(crate) identity_transition: Option<IdentityTransition>,
}

impl std::fmt::Debug for AccountVerificationResult {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AccountVerificationResult")
            .field("account_verification_token", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .field("account_user_id", &self.account_user_id)
            .field("handle", &self.handle)
            .field("did", &self.did)
            .field("identity_transition", &self.identity_transition)
            .finish()
    }
}

pub(crate) fn canonical_handle(value: &str) -> crate::ImResult<CanonicalHandle> {
    if value.is_empty() || value != value.trim() || value.ends_with('.') {
        return Err(invalid("handle", "full Handle must already be canonical"));
    }
    let Some((local_part, domain)) = value.split_once('.') else {
        return Err(invalid(
            "handle",
            "full Handle must include a provider domain",
        ));
    };
    if local_part.is_empty()
        || domain.is_empty()
        || value.bytes().any(|byte| byte.is_ascii_uppercase())
        || !anp::wns::validate_local_part(local_part)
        || domain.split('.').any(|label| {
            label.is_empty()
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
    {
        return Err(invalid("handle", "full Handle is not canonical"));
    }
    Ok(CanonicalHandle {
        local_part: local_part.to_owned(),
        domain: domain.to_owned(),
        full: value.to_owned(),
    })
}

pub(crate) fn validate_operation_id(value: &str) -> crate::ImResult<String> {
    let scalar_count = value.chars().count();
    if scalar_count == 0
        || scalar_count > 128
        || value.chars().any(|character| {
            character.is_whitespace() || character == '\u{7f}' || (character as u32) < 0x20
        })
    {
        return Err(invalid(
            "operation_id",
            "operation id is outside the closed Handle Recovery profile",
        ));
    }
    Ok(value.to_owned())
}

pub(crate) fn recovery_otp_target(handle: &str, operation_id: &str) -> crate::ImResult<String> {
    let handle = canonical_handle(handle)?;
    let operation_id = validate_operation_id(operation_id)?;
    let target = json!({
        "domain": handle.domain,
        "full_handle": handle.full,
        "handle": handle.local_part,
        "operation_id": operation_id,
        "purpose": HANDLE_RECOVERY_PURPOSE,
    });
    let canonical = serde_json_canonicalizer::to_vec(&target).map_err(serialization)?;
    Ok(format!("sha256:{:x}", Sha256::digest(canonical)))
}

pub(crate) fn build_send_otp_call(
    phone: &str,
    handle: &str,
    operation_id: &str,
) -> crate::ImResult<super::RpcCall> {
    let phone = super::normalize_phone(phone)?;
    let handle = canonical_handle(handle)?;
    let operation_id = validate_operation_id(operation_id)?;
    let _ = recovery_otp_target(&handle.full, &operation_id)?;
    Ok(super::rpc_call(
        super::HANDLE_RPC_ENDPOINT,
        "send_otp",
        super::TransportProfile::RpcDefault,
        json!({
            "phone": phone,
            "purpose": HANDLE_RECOVERY_PURPOSE,
            "handle": handle.local_part,
            "domain": handle.domain,
            "full_handle": handle.full,
            "operation_id": operation_id,
        }),
    ))
}

pub(crate) fn parse_account_verification_result(
    value: Value,
) -> crate::ImResult<AccountVerificationResult> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Raw {
        account_verification_token: String,
        purpose: String,
        expires_at: String,
        account_user_id: Option<String>,
        handle: Option<String>,
        did: Option<String>,
        identity_transition: Option<RawTransition>,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct RawTransition {
        kind: String,
        previous_did: String,
        current_did: String,
        binding_generation: String,
    }
    let raw: Raw = closed(value, "account verification exchange")?;
    if raw.purpose != "awiki.device.join.v1" {
        return Err(invalid(
            "purpose",
            "unexpected account verification purpose",
        ));
    }
    validate_timestamp("expires_at", &raw.expires_at)?;
    let projection_count = [
        raw.account_user_id.is_some(),
        raw.handle.is_some(),
        raw.did.is_some(),
        raw.identity_transition.is_some(),
    ]
    .into_iter()
    .filter(|present| *present)
    .count();
    if projection_count != 0 && projection_count != 4 {
        return Err(invalid(
            "identity_transition",
            "identity transition projection must be all-or-none",
        ));
    }
    let transition = raw
        .identity_transition
        .map(|transition| {
            if transition.kind != "handle_recovery"
                || !canonical_generation(&transition.binding_generation)
            {
                return Err(invalid(
                    "identity_transition",
                    "unsupported identity transition",
                ));
            }
            let previous = crate::ids::Did::parse(transition.previous_did.clone())?;
            let current = crate::ids::Did::parse(transition.current_did.clone())?;
            if current.as_str() != raw.did.as_deref().unwrap_or_default() {
                return Err(invalid(
                    "did",
                    "projected DID must match identity transition current DID",
                ));
            }
            if canonical_handle(raw.handle.as_deref().unwrap_or_default()).is_err() {
                return Err(invalid("handle", "projected Handle is not canonical"));
            }
            Ok(IdentityTransition {
                previous_did: previous.as_str().to_owned(),
                current_did: current.as_str().to_owned(),
                binding_generation: transition.binding_generation,
            })
        })
        .transpose()?;
    Ok(AccountVerificationResult {
        account_verification_token: required_secret(
            raw.account_verification_token,
            "account_verification_token",
        )?,
        expires_at: raw.expires_at,
        account_user_id: raw.account_user_id,
        handle: raw.handle,
        did: raw.did,
        identity_transition: transition,
    })
}

pub(crate) fn build_grant_exchange_call_v4(
    phone: &str,
    code: &str,
    full_handle: &str,
    operation_id: &str,
    bootstrap_signing_key_id: &str,
    bootstrap_signing_public_key: &Value,
) -> crate::ImResult<super::RestCall> {
    let phone = super::normalize_phone(phone)?;
    let code = super::sanitize_otp(code);
    if code.is_empty() {
        return Err(invalid("code", "OTP code is required"));
    }
    let full_handle = canonical_handle_v4(full_handle)?.full;
    let operation_id = validate_operation_id(operation_id)?;
    let bootstrap_signing_key_id = required(bootstrap_signing_key_id, "bootstrap_signing_key_id")?;
    let bootstrap_signing_public_key =
        canonical_ed25519_public_jwk_v4(bootstrap_signing_public_key)?;
    Ok(super::rest_call(
        HANDLE_RECOVERY_V4_EXCHANGE_ENDPOINT,
        "POST",
        json!({
            "contract_version": HANDLE_RECOVERY_V4_CONTRACT_VERSION,
            "phone": phone,
            "code": code,
            "full_handle": full_handle,
            "operation_id": operation_id,
            "bootstrap_signing_key_id": bootstrap_signing_key_id,
            "bootstrap_signing_public_key": bootstrap_signing_public_key,
        }),
        Default::default(),
        false,
    ))
}

pub(crate) fn parse_grant_exchange_result_v4(
    value: Value,
    expected_full_handle: &str,
) -> crate::ImResult<RecoveryGrantExchangeResultV4> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Raw {
        contract_version: String,
        recovery_grant: String,
        purpose: String,
        expires_at: String,
        current_binding: RawBinding,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct RawBinding {
        account_user_id: String,
        full_handle: String,
        current_did: String,
        binding_generation: String,
    }

    let expected_full_handle = canonical_handle_v4(expected_full_handle)?.full;
    let raw: Raw = closed(value, "Handle Recovery V4 grant exchange")?;
    if raw.contract_version != HANDLE_RECOVERY_V4_CONTRACT_VERSION {
        return Err(invalid(
            "contract_version",
            "unexpected Recovery contract version",
        ));
    }
    if raw.purpose != HANDLE_RECOVERY_PURPOSE {
        return Err(invalid("purpose", "unexpected Recovery grant purpose"));
    }
    validate_timestamp_v4("expires_at", &raw.expires_at)?;
    if raw.current_binding.account_user_id.chars().count() == 0
        || raw.current_binding.account_user_id.chars().count() > 255
    {
        return Err(invalid(
            "account_user_id",
            "account user id is outside the closed Recovery profile",
        ));
    }
    let full_handle = canonical_handle_v4(&raw.current_binding.full_handle)?.full;
    if full_handle != expected_full_handle {
        return Err(invalid(
            "full_handle",
            "authoritative binding does not match the requested Handle",
        ));
    }
    let current_did = canonical_did_wba_v4(&raw.current_binding.current_did)?;
    if !canonical_generation(&raw.current_binding.binding_generation) {
        return Err(invalid(
            "binding_generation",
            "binding generation must be canonical positive decimal",
        ));
    }
    Ok(RecoveryGrantExchangeResultV4 {
        recovery_grant: required_secret(raw.recovery_grant, "recovery_grant")?,
        expires_at: raw.expires_at,
        current_binding: RecoveryCurrentBindingV4 {
            account_user_id: raw.current_binding.account_user_id,
            full_handle,
            current_did: current_did.as_str().to_owned(),
            binding_generation: raw.current_binding.binding_generation,
        },
    })
}

pub(crate) fn new_did_document_hash_v4(document: &Value) -> crate::ImResult<String> {
    let mut projected = document.clone();
    projected
        .as_object_mut()
        .ok_or_else(|| invalid("new_did_document", "DID document must be an object"))?
        .remove("proof");
    let canonical = serde_json_canonicalizer::to_vec(&projected).map_err(serialization)?;
    Ok(format!(
        "sha256:{}",
        URL_SAFE_NO_PAD.encode(Sha256::digest(canonical))
    ))
}

pub(crate) fn canonical_intent_v4(
    intent: &crate::internal::identity_handle_recovery_pending::RecoveryIntentV4,
) -> crate::ImResult<Vec<u8>> {
    validate_intent_v4(intent)?;
    serde_json_canonicalizer::to_vec(intent).map_err(serialization)
}

pub(crate) fn intent_hash_v4(
    intent: &crate::internal::identity_handle_recovery_pending::RecoveryIntentV4,
) -> crate::ImResult<String> {
    Ok(format!(
        "sha256:{}",
        URL_SAFE_NO_PAD.encode(Sha256::digest(canonical_intent_v4(intent)?))
    ))
}

pub(crate) fn prepare_commit_v4(input: CommitProofInputV4<'_>) -> crate::ImResult<PendingCommitV4> {
    let computed_intent_hash = intent_hash_v4(input.proof.intent)?;
    require_matching_intent_hash(input.proof.intent_hash, &computed_intent_hash)?;
    let computed_document_hash = new_did_document_hash_v4(&input.new_did_document)?;
    if !constant_time_equal(
        computed_document_hash.as_bytes(),
        input.proof.intent.new_did_document_hash.as_bytes(),
    ) {
        return Err(invalid(
            "new_did_document",
            "DID document does not match the immutable Recovery intent",
        ));
    }
    if input.new_did_document.get("id").and_then(Value::as_str)
        != Some(input.proof.intent.new_did.as_str())
    {
        return Err(invalid(
            "new_did_document",
            "DID document id does not match the immutable Recovery intent",
        ));
    }
    let proof = prepare_key_possession_proof_v4(
        &input.proof,
        HANDLE_RECOVERY_COMMIT_V4_METHOD,
        HANDLE_RECOVERY_COMMIT_V4_PURPOSE,
    )?;
    Ok(PendingCommitV4 {
        intent: input.proof.intent.clone(),
        intent_hash: computed_intent_hash,
        recovery_grant: input.recovery_grant,
        new_did_document: input.new_did_document,
        proof,
    })
}

pub(crate) fn complete_commit_v4(
    pending: PendingCommitV4,
    signature: &[u8],
) -> crate::ImResult<PreparedCommitV4> {
    let (proof, signed_object) = complete_key_possession_proof_v4(pending.proof, signature)?;
    let recovery_grant = String::from_utf8(pending.recovery_grant.expose_secret().to_vec())
        .map_err(|_| invalid("recovery_grant", "Recovery grant must be UTF-8"))?;
    let params = json!({
        "intent": pending.intent,
        "intent_hash": pending.intent_hash,
        "recovery_grant": required(&recovery_grant, "recovery_grant")?,
        "new_did_document": pending.new_did_document,
        "bootstrap_key_possession_proof": proof,
    });
    Ok(PreparedCommitV4 {
        call: super::rpc_call(
            super::DID_AUTH_RPC_ENDPOINT,
            HANDLE_RECOVERY_COMMIT_V4_METHOD,
            super::TransportProfile::RpcDefault,
            params,
        ),
        intent_hash: pending.intent_hash,
        signed_object,
    })
}

pub(crate) fn prepare_result_get_v4(
    input: KeyPossessionProofInputV4<'_>,
) -> crate::ImResult<PendingResultGetV4> {
    let computed_intent_hash = intent_hash_v4(input.intent)?;
    require_matching_intent_hash(input.intent_hash, &computed_intent_hash)?;
    let proof = prepare_key_possession_proof_v4(
        &input,
        HANDLE_RECOVERY_RESULT_GET_V4_METHOD,
        HANDLE_RECOVERY_RESULT_GET_V4_PURPOSE,
    )?;
    Ok(PendingResultGetV4 {
        intent: input.intent.clone(),
        intent_hash: computed_intent_hash,
        proof,
    })
}

pub(crate) fn complete_result_get_v4(
    pending: PendingResultGetV4,
    signature: &[u8],
) -> crate::ImResult<PreparedResultGetV4> {
    let (proof, signed_object) = complete_key_possession_proof_v4(pending.proof, signature)?;
    let params = json!({
        "contract_version": HANDLE_RECOVERY_V4_CONTRACT_VERSION,
        "intent": pending.intent,
        "intent_hash": pending.intent_hash,
        "bootstrap_key_possession_proof": proof,
    });
    Ok(PreparedResultGetV4 {
        call: super::rpc_call(
            super::DID_AUTH_RPC_ENDPOINT,
            HANDLE_RECOVERY_RESULT_GET_V4_METHOD,
            super::TransportProfile::RpcDefault,
            params,
        ),
        intent_hash: pending.intent_hash,
        signed_object,
    })
}

pub(crate) fn parse_commit_result_v4(
    value: Value,
    operation_id: &str,
    intent_hash: &str,
) -> crate::ImResult<crate::internal::identity_handle_recovery_pending::RecoveryRemoteResultV4> {
    validate_operation_id(operation_id)?;
    validate_sha256_digest_v4("intent_hash", intent_hash)?;
    let result: crate::internal::identity_handle_recovery_pending::RecoveryRemoteResultV4 =
        closed(value, "Handle Recovery V4 commit")?;
    result.validate_against(operation_id, intent_hash)?;
    if result.account_user_id.chars().count() > 255
        || result.full_handle.chars().count() > 320
        || result.checkpoint.document_version > i64::MAX as u64
        || result.checkpoint.registry_version > i64::MAX as u64
    {
        return Err(invalid(
            "result",
            "committed result is outside the closed Recovery V4 profile",
        ));
    }
    canonical_handle_v4(&result.full_handle)?;
    canonical_did_wba_v4(&result.previous_did)?;
    canonical_did_wba_v4(&result.current_did)?;
    Ok(result)
}

pub(crate) fn build_attestation_issue_call_v1(
    operation_id: &str,
) -> crate::ImResult<super::RpcCall> {
    validate_operation_id(operation_id)?;
    Ok(super::rpc_call(
        super::DID_AUTH_RPC_ENDPOINT,
        HANDLE_RECOVERY_ATTESTATION_ISSUE_V1_METHOD,
        super::TransportProfile::RpcDefault,
        json!({"operation_id": operation_id}),
    ))
}

pub(crate) fn parse_attestation_issue_result_v1(
    value: Value,
) -> crate::ImResult<crate::identity::HandleRecoveryAttestation> {
    let object = value.as_object().ok_or_else(|| {
        invalid(
            "result",
            "Handle Recovery attestation response must be an object",
        )
    })?;
    if object.len() != 2
        || !object.contains_key("attestation")
        || !object.contains_key("expires_at")
    {
        return Err(invalid(
            "result",
            "Handle Recovery attestation response is not closed",
        ));
    }
    let attestation = object
        .get("attestation")
        .and_then(Value::as_str)
        .filter(|value| valid_compact_jwt(value))
        .ok_or_else(|| invalid("attestation", "Recovery attestation is invalid"))?;
    let expires_at = object
        .get("expires_at")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("expires_at", "Recovery attestation expiry is invalid"))?;
    parse_timestamp_v4("expires_at", expires_at)?;
    Ok(crate::identity::HandleRecoveryAttestation::new(
        attestation.to_owned(),
        expires_at.to_owned(),
    ))
}

fn valid_compact_jwt(value: &str) -> bool {
    if value.len() < 32 || value.len() > 16 * 1024 {
        return false;
    }
    let segments = value.split('.').collect::<Vec<_>>();
    segments.len() == 3
        && segments.iter().all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        })
}

pub(crate) fn parse_result_get_v4(
    value: Value,
    operation_id: &str,
    intent_hash: &str,
) -> crate::ImResult<RecoveryResultGetV4> {
    let object = value.as_object().ok_or_else(|| {
        invalid(
            "result",
            "Handle Recovery V4 result-get response must be an object",
        )
    })?;
    match object.get("state").and_then(Value::as_str) {
        Some("committed") if object.len() == 2 && object.contains_key("result") => {
            parse_commit_result_v4(
                object
                    .get("result")
                    .cloned()
                    .expect("checked result member"),
                operation_id,
                intent_hash,
            )
            .map(RecoveryResultGetV4::Committed)
        }
        Some("result_absent")
            if object.len() == 2
                && object.get("padding").and_then(Value::as_str)
                    == Some(HANDLE_RECOVERY_RESULT_ABSENT_PADDING) =>
        {
            validate_operation_id(operation_id)?;
            validate_sha256_digest_v4("intent_hash", intent_hash)?;
            Ok(RecoveryResultGetV4::ResultAbsent)
        }
        _ => Err(invalid(
            "result",
            "invalid closed Handle Recovery V4 result-get response",
        )),
    }
}

fn prepare_key_possession_proof_v4(
    input: &KeyPossessionProofInputV4<'_>,
    method: &'static str,
    purpose: &'static str,
) -> crate::ImResult<PendingKeyPossessionProofV4> {
    validate_intent_v4(input.intent)?;
    let computed_intent_hash = intent_hash_v4(input.intent)?;
    require_matching_intent_hash(input.intent_hash, &computed_intent_hash)?;
    let audience = required(input.audience, "audience")?;
    let created_at = parse_timestamp_v4("created_at", input.created_at)?;
    let expires_at = parse_timestamp_v4("expires_at", input.expires_at)?;
    let lifetime = (expires_at - created_at).whole_seconds();
    if !(1..=120).contains(&lifetime) {
        return Err(invalid(
            "expires_at",
            "proof lifetime must be between one and 120 seconds",
        ));
    }
    if input.nonce.len() != 32 {
        return Err(invalid(
            "nonce",
            "proof nonce must contain exactly 32 bytes",
        ));
    }
    let nonce = URL_SAFE_NO_PAD.encode(input.nonce);
    let signed_object = json!({
        "type": HANDLE_RECOVERY_KEY_POSSESSION_PROOF_TYPE,
        "purpose": purpose,
        "method": method,
        "audience": audience,
        "operation_id": input.intent.operation_id,
        "intent_hash": computed_intent_hash,
        "key_id": input.intent.bootstrap_signing_key_id,
        "created_at": input.created_at,
        "expires_at": input.expires_at,
        "nonce": nonce,
    });
    let signing_bytes = serde_json_canonicalizer::to_vec(&signed_object).map_err(serialization)?;
    let proof = json!({
        "type": HANDLE_RECOVERY_KEY_POSSESSION_PROOF_TYPE,
        "key_id": input.intent.bootstrap_signing_key_id,
        "created_at": input.created_at,
        "expires_at": input.expires_at,
        "nonce": nonce,
    });
    Ok(PendingKeyPossessionProofV4 {
        proof,
        signed_object,
        signing_input: signing_bytes,
    })
}

fn complete_key_possession_proof_v4(
    mut pending: PendingKeyPossessionProofV4,
    signature: &[u8],
) -> crate::ImResult<(Value, Value)> {
    if signature.len() != 64 {
        return Err(crate::ImError::PermissionDenied);
    }
    pending.proof["signature"] = Value::String(URL_SAFE_NO_PAD.encode(signature));
    Ok((pending.proof, pending.signed_object))
}

fn validate_intent_v4(
    intent: &crate::internal::identity_handle_recovery_pending::RecoveryIntentV4,
) -> crate::ImResult<()> {
    if intent.schema_version != "1" {
        return Err(invalid(
            "schema_version",
            "unsupported Recovery intent schema",
        ));
    }
    if intent.contract_version != HANDLE_RECOVERY_V4_CONTRACT_VERSION {
        return Err(invalid(
            "contract_version",
            "unexpected Recovery contract version",
        ));
    }
    validate_operation_id(&intent.operation_id)?;
    if intent.account_user_id.chars().count() == 0 || intent.account_user_id.chars().count() > 255 {
        return Err(invalid(
            "account_user_id",
            "account user id is outside the closed Recovery profile",
        ));
    }
    canonical_handle_v4(&intent.full_handle)?;
    let previous_did = canonical_did_wba_v4(&intent.expected_previous_did)?;
    let new_did = canonical_did_wba_v4(&intent.new_did)?;
    if previous_did == new_did {
        return Err(invalid("new_did", "new DID must differ from previous DID"));
    }
    if !canonical_generation(&intent.expected_binding_generation) {
        return Err(invalid(
            "expected_binding_generation",
            "binding generation must be canonical positive decimal",
        ));
    }
    validate_sha256_digest_v4("new_did_document_hash", &intent.new_did_document_hash)?;
    required(&intent.bootstrap_device_id, "bootstrap_device_id")?;
    let key_id = required(&intent.bootstrap_signing_key_id, "bootstrap_signing_key_id")?;
    if !key_id.starts_with(&format!("{}#", new_did.as_str())) {
        return Err(invalid(
            "bootstrap_signing_key_id",
            "bootstrap signing key must be controlled by the new DID",
        ));
    }
    canonical_ed25519_public_jwk_v4(&intent.bootstrap_signing_public_key)?;
    Ok(())
}

fn canonical_ed25519_public_jwk_v4(value: &Value) -> crate::ImResult<Value> {
    let jwk: Ed25519PublicJwkV4 = closed(value.clone(), "Ed25519 public JWK")?;
    if jwk.kty != "OKP" || jwk.crv != "Ed25519" {
        return Err(invalid(
            "bootstrap_signing_public_key",
            "bootstrap signing key must use the closed Ed25519 JWK profile",
        ));
    }
    let decoded = URL_SAFE_NO_PAD.decode(jwk.x.as_bytes()).map_err(|_| {
        invalid(
            "bootstrap_signing_public_key",
            "Ed25519 x must be canonical unpadded base64url",
        )
    })?;
    if decoded.len() != 32 || URL_SAFE_NO_PAD.encode(&decoded) != jwk.x {
        return Err(invalid(
            "bootstrap_signing_public_key",
            "Ed25519 x must encode exactly 32 bytes canonically",
        ));
    }
    serde_json::to_value(jwk).map_err(serialization)
}

fn canonical_handle_v4(value: &str) -> crate::ImResult<CanonicalHandle> {
    if value.chars().count() > 320 {
        return Err(invalid(
            "full_handle",
            "full Handle exceeds the closed Recovery profile",
        ));
    }
    canonical_handle(value)
}

fn canonical_did_wba_v4(value: &str) -> crate::ImResult<crate::ids::Did> {
    if value != value.trim() || !value.starts_with("did:wba:") {
        return Err(invalid("did", "DID must be a canonical did:wba identifier"));
    }
    crate::ids::Did::parse(value)
}

fn validate_sha256_digest_v4(field: &str, value: &str) -> crate::ImResult<()> {
    let Some(encoded) = value.strip_prefix("sha256:") else {
        return Err(invalid(field, "digest must use the sha256 profile"));
    };
    let decoded = URL_SAFE_NO_PAD
        .decode(encoded.as_bytes())
        .map_err(|_| invalid(field, "digest must be canonical unpadded base64url"))?;
    if decoded.len() != 32 || URL_SAFE_NO_PAD.encode(&decoded) != encoded {
        return Err(invalid(
            field,
            "digest must canonically encode exactly 32 bytes",
        ));
    }
    Ok(())
}

fn require_matching_intent_hash(supplied: &str, computed: &str) -> crate::ImResult<()> {
    validate_sha256_digest_v4("intent_hash", supplied)?;
    if !constant_time_equal(supplied.as_bytes(), computed.as_bytes()) {
        return Err(invalid(
            "intent_hash",
            "intent hash does not match the immutable Recovery intent",
        ));
    }
    Ok(())
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

fn parse_timestamp_v4(field: &str, value: &str) -> crate::ImResult<time::OffsetDateTime> {
    validate_timestamp_v4(field, value)?;
    time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
        .map_err(|_| invalid(field, "timestamp must be valid RFC 3339 UTC"))
}

fn validate_timestamp_v4(field: &str, value: &str) -> crate::ImResult<()> {
    let bytes = value.as_bytes();
    let exact_shape = bytes.len() == 20
        && bytes[0..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'-'
        && bytes[8..10].iter().all(u8::is_ascii_digit)
        && bytes[10] == b'T'
        && bytes[11..13].iter().all(u8::is_ascii_digit)
        && bytes[13] == b':'
        && bytes[14..16].iter().all(u8::is_ascii_digit)
        && bytes[16] == b':'
        && bytes[17..19].iter().all(u8::is_ascii_digit)
        && bytes[19] == b'Z';
    if !exact_shape
        || time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
            .is_err()
    {
        return Err(invalid(
            field,
            "timestamp must use RFC 3339 second precision with a Z suffix",
        ));
    }
    Ok(())
}

fn canonical_generation(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_BINDING_GENERATION_DIGITS
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value.as_bytes()[0] != b'0'
}

fn validate_timestamp(field: &str, value: &str) -> crate::ImResult<()> {
    if !value.ends_with('Z')
        || time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
            .is_err()
    {
        return Err(invalid(field, "timestamp must be canonical RFC 3339 UTC"));
    }
    Ok(())
}

fn closed<T: for<'de> Deserialize<'de>>(value: Value, context: &str) -> crate::ImResult<T> {
    serde_json::from_value(value).map_err(|_| crate::ImError::Serialization {
        detail: format!("invalid closed {context} response"),
    })
}

fn required(value: &str, field: &str) -> crate::ImResult<String> {
    if value.is_empty() || value != value.trim() {
        return Err(invalid(field, format!("{field} is required")));
    }
    Ok(value.to_owned())
}

fn required_secret(value: String, field: &str) -> crate::ImResult<SecretBytes> {
    required(&value, field).map(|value| SecretBytes::from_vec(value.into_bytes()))
}

fn serialization(error: serde_json::Error) -> crate::ImError {
    crate::ImError::Serialization {
        detail: error.to_string(),
    }
}

fn invalid(field: &str, message: impl Into<String>) -> crate::ImError {
    crate::ImError::invalid_input(Some(field.to_owned()), message.into())
}
