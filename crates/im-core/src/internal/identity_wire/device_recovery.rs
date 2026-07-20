//! Strict AWiki-internal wire contract for Handle Recovery.
//!
//! These JSON-RPC methods are same-domain control-plane operations. They are
//! deliberately not ANP models; their authenticated account subject and
//! checkpoint/mapping fields never appear in public WNS, Core DTOs, or DID
//! Document extensions.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::RngCore as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use time::{format_description::well_known::Rfc3339, Duration, OffsetDateTime};
use zeroize::Zeroize;

use crate::identity::DeviceProof;
use crate::internal::identity_device_state::{
    DeviceAuthorizationProjection, DeviceAuthorizationRole, DeviceAuthorizationStatus,
    IdentityDeviceMode, IdentityDeviceState, IdentityInternalCheckpoint,
    IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
};

pub(crate) const RECOVERY_BEGIN_METHOD: &str = "device_recovery_begin";
pub(crate) const RECOVERY_STATUS_METHOD: &str = "device_recovery_status";
pub(crate) const RECOVERY_CANCEL_METHOD: &str = "device_recovery_cancel";
pub(crate) const RECOVERY_FINALIZE_METHOD: &str = "device_recovery_finalize";
pub(crate) const RECOVERY_CANCEL_PURPOSE: &str = "awiki.device.recovery.cancel.v1";
pub(crate) const RECOVERY_FINALIZE_PURPOSE: &str = "awiki.device.recovery.finalize.v1";

const DEVICE_PROOF_TYPE: &str = "awiki-device-signature-v1";
const PROOF_TTL_SECONDS: i64 = 300;
const PROOF_NONCE_LEN: usize = 24;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RecoveryRemoteState {
    Cooling,
    Ready,
    Cancelled,
    Consumed,
    Expired,
}

#[derive(PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecoverySessionResult {
    pub(crate) recovery_session_id: String,
    pub(crate) recovery_session_token: String,
    pub(crate) account_user_id: String,
    pub(crate) old_did: String,
    pub(crate) state: RecoveryRemoteState,
    pub(crate) cooling_until: String,
    pub(crate) expires_at: String,
}

impl std::fmt::Debug for RecoverySessionResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecoverySessionResult")
            .field("recovery_session_id", &self.recovery_session_id)
            .field("recovery_session_token", &"<redacted-recovery-token>")
            .field("account_user_id", &"<internal-account-subject>")
            .field("old_did", &self.old_did)
            .field("state", &self.state)
            .field("cooling_until", &self.cooling_until)
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

impl Drop for RecoverySessionResult {
    fn drop(&mut self) {
        self.recovery_session_token.zeroize();
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecoveryStatusResult {
    pub(crate) recovery_session_id: String,
    pub(crate) old_did: String,
    pub(crate) state: RecoveryRemoteState,
    pub(crate) cooling_until: String,
    pub(crate) expires_at: String,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PreparedRecoveryFinalize {
    pub(crate) operation_id: String,
    pub(crate) expected_handle_mapping_generation: u64,
    pub(crate) new_did_document: Value,
    pub(crate) bootstrap_device_id: String,
    pub(crate) bootstrap_device_proof: DeviceProof,
}

impl std::fmt::Debug for PreparedRecoveryFinalize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedRecoveryFinalize")
            .field("operation_id", &self.operation_id)
            .field(
                "expected_handle_mapping_generation",
                &self.expected_handle_mapping_generation,
            )
            .field("new_did", &self.new_did_document.get("id"))
            .field("bootstrap_device_id", &self.bootstrap_device_id)
            .field("bootstrap_device_proof", &"<redacted-device-proof>")
            .finish()
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PreparedRecoveryCancel {
    pub(crate) operation_id: String,
    pub(crate) recovery_session_id: String,
    pub(crate) authorizing_device_id: String,
    pub(crate) authorizing_device_proof: DeviceProof,
}

impl std::fmt::Debug for PreparedRecoveryCancel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedRecoveryCancel")
            .field("operation_id", &self.operation_id)
            .field("recovery_session_id", &self.recovery_session_id)
            .field("authorizing_device_id", &self.authorizing_device_id)
            .field("authorizing_device_proof", &"<redacted-device-proof>")
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecoveryCancelResult {
    pub(crate) recovery_session_id: String,
    pub(crate) state: RecoveryRemoteState,
}

#[derive(PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecoveryFinalizeResult {
    pub(crate) recovery_session_id: String,
    pub(crate) state: RecoveryRemoteState,
    pub(crate) old_did: String,
    pub(crate) did: String,
    pub(crate) handle: String,
    pub(crate) handle_mapping_generation: u64,
    pub(crate) user_id: String,
    pub(crate) checkpoint: IdentityInternalCheckpoint,
    pub(crate) device: super::device_genesis::GenesisDeviceResult,
    pub(crate) access_token: String,
    pub(crate) refresh_token: String,
    pub(crate) token_expires_at: String,
}

impl std::fmt::Debug for RecoveryFinalizeResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecoveryFinalizeResult")
            .field("recovery_session_id", &self.recovery_session_id)
            .field("state", &self.state)
            .field("old_did", &self.old_did)
            .field("did", &self.did)
            .field("handle", &self.handle)
            .field("handle_mapping_generation", &self.handle_mapping_generation)
            .field("user_id", &self.user_id)
            .field("checkpoint", &self.checkpoint)
            .field("device", &self.device)
            .field("access_token", &"<redacted-access-token>")
            .field("refresh_token", &"<redacted-refresh-token>")
            .field("token_expires_at", &self.token_expires_at)
            .finish()
    }
}

impl Drop for RecoveryFinalizeResult {
    fn drop(&mut self) {
        self.access_token.zeroize();
        self.refresh_token.zeroize();
    }
}

impl RecoveryFinalizeResult {
    pub(crate) fn device_state(&self) -> IdentityDeviceState {
        IdentityDeviceState {
            schema_version: IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
            mode: IdentityDeviceMode::VNext,
            authorization: Some(DeviceAuthorizationProjection {
                protocol_device_id: crate::ids::ProtocolDeviceId::parse(&self.device.device_id)
                    .expect("validated Recovery device id"),
                signing_key_id: self.device.signing_key_id.clone(),
                e2ee_key_id: self.device.e2ee_key_id.clone(),
                status: DeviceAuthorizationStatus::Active,
                role: DeviceAuthorizationRole::Admin,
                management_ready: true,
                auth_generation: self.device.auth_generation,
            }),
            checkpoint: Some(self.checkpoint.clone()),
        }
    }
}

#[derive(Debug, PartialEq)]
pub(crate) struct RecoveryFinalizeCutoverResult {
    pub(crate) recovery_session_id: String,
    pub(crate) state: RecoveryRemoteState,
    pub(crate) old_did: String,
    pub(crate) did: String,
    pub(crate) handle: String,
    pub(crate) handle_mapping_generation: u64,
    pub(crate) checkpoint: IdentityInternalCheckpoint,
    pub(crate) device: super::device_genesis::GenesisDeviceResult,
}

#[derive(Debug, PartialEq)]
pub(crate) enum RecoveryFinalizeParseOutcome {
    Ready(RecoveryFinalizeResult),
    TokenRefreshRequired(RecoveryFinalizeCutoverResult),
}

pub(crate) fn complete_recovery_finalize_with_fresh_tokens(
    cutover: RecoveryFinalizeCutoverResult,
    mut token: super::device_genesis::DeviceTokenIssueResult,
    expected_user_id: &str,
    generated: &crate::internal::identity_generation::GeneratedVNextIdentityWithDaemonSubkey,
) -> crate::ImResult<RecoveryFinalizeResult> {
    let valid = token.user_id == expected_user_id
        && token.device_id == cutover.device.device_id
        && token.device_id == generated.protocol_device_id.as_str()
        && token.auth_generation == cutover.device.auth_generation
        && token.auth_generation == 1
        && token.scopes
            == vec![
                "device:manage".to_owned(),
                "device:read".to_owned(),
                "message:connect".to_owned(),
            ];
    if !valid {
        token.access_token.zeroize();
        token.refresh_token.zeroize();
        return Err(crate::ImError::PermissionDenied);
    }
    let result = RecoveryFinalizeResult {
        recovery_session_id: cutover.recovery_session_id,
        state: cutover.state,
        old_did: cutover.old_did,
        did: cutover.did,
        handle: cutover.handle,
        handle_mapping_generation: cutover.handle_mapping_generation,
        user_id: std::mem::take(&mut token.user_id),
        checkpoint: cutover.checkpoint,
        device: cutover.device,
        access_token: std::mem::take(&mut token.access_token),
        refresh_token: std::mem::take(&mut token.refresh_token),
        token_expires_at: std::mem::take(&mut token.expires_at),
    };
    result.device_state().validate_for_did(&generated.did)?;
    Ok(result)
}

pub(crate) fn replace_recovery_finalize_tokens(
    result: &mut RecoveryFinalizeResult,
    mut token: super::device_genesis::DeviceTokenIssueResult,
    expected_user_id: &str,
    generated: &crate::internal::identity_generation::GeneratedVNextIdentityWithDaemonSubkey,
) -> crate::ImResult<()> {
    if token.user_id != expected_user_id
        || token.user_id != result.user_id
        || token.device_id != result.device.device_id
        || token.device_id != generated.protocol_device_id.as_str()
        || token.auth_generation != result.device.auth_generation
        || token.auth_generation != 1
        || token.scopes
            != vec![
                "device:manage".to_owned(),
                "device:read".to_owned(),
                "message:connect".to_owned(),
            ]
    {
        return Err(crate::ImError::PermissionDenied);
    }
    result.access_token.zeroize();
    result.refresh_token.zeroize();
    result.access_token = std::mem::take(&mut token.access_token);
    result.refresh_token = std::mem::take(&mut token.refresh_token);
    result.token_expires_at = std::mem::take(&mut token.expires_at);
    result.device_state().validate_for_did(&generated.did)
}

pub(crate) struct RecoveryWireCall {
    pub(crate) endpoint: &'static str,
    pub(crate) method: &'static str,
    pub(crate) params: Value,
}

impl std::fmt::Debug for RecoveryWireCall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecoveryWireCall")
            .field("endpoint", &self.endpoint)
            .field("method", &self.method)
            .field("params", &"<redacted-recovery-params>")
            .finish()
    }
}

pub(crate) fn build_recovery_begin_call(
    operation_id: &str,
    account_verification_token: &str,
    handle: &str,
) -> crate::ImResult<RecoveryWireCall> {
    Ok(call(
        RECOVERY_BEGIN_METHOD,
        json!({
            "operation_id": required(operation_id, "operation_id")?,
            "account_verification_token": required(
                account_verification_token,
                "account_verification_token",
            )?,
            "handle": normalized_full_handle(handle)?,
        }),
    ))
}

pub(crate) fn parse_recovery_begin_result(
    raw: Value,
    expected_old_did: &crate::ids::Did,
    now: OffsetDateTime,
) -> crate::ImResult<RecoverySessionResult> {
    let result: RecoverySessionResult = strict(raw, "Recovery begin result")?;
    validate_session_window(
        &result.recovery_session_id,
        &result.old_did,
        &result.cooling_until,
        &result.expires_at,
        now,
        false,
    )?;
    if result.state != RecoveryRemoteState::Cooling
        || result.old_did != expected_old_did.as_str()
        || required(&result.account_user_id, "account_user_id")? != result.account_user_id
        || required(&result.recovery_session_token, "recovery_session_token")?
            == result.recovery_session_id
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(result)
}

pub(crate) fn build_recovery_status_call(
    recovery_session_token: &str,
) -> crate::ImResult<RecoveryWireCall> {
    Ok(call(
        RECOVERY_STATUS_METHOD,
        json!({
            "recovery_session_token": required(
                recovery_session_token,
                "recovery_session_token",
            )?,
        }),
    ))
}

pub(crate) fn parse_recovery_status_result(
    raw: Value,
    expected: &RecoverySessionResult,
    now: OffsetDateTime,
) -> crate::ImResult<RecoveryStatusResult> {
    let result: RecoveryStatusResult = strict(raw, "Recovery status result")?;
    validate_session_window(
        &result.recovery_session_id,
        &result.old_did,
        &result.cooling_until,
        &result.expires_at,
        now,
        result.state == RecoveryRemoteState::Expired,
    )?;
    if result.recovery_session_id != expected.recovery_session_id
        || result.old_did != expected.old_did
        || result.cooling_until != expected.cooling_until
        || result.expires_at != expected.expires_at
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(result)
}

pub(crate) fn prepare_recovery_cancel(
    operation_id: String,
    recovery_session_id: &str,
    authorizing_device_id: &str,
    signing_key_id: &str,
    signing_private: &anp::PrivateKeyMaterial,
    did_document: &Value,
    now: OffsetDateTime,
) -> crate::ImResult<PreparedRecoveryCancel> {
    let operation_id = required(&operation_id, "operation_id")?;
    let recovery_session_id = required(recovery_session_id, "recovery_session_id")?;
    let authorizing_device_id = required(authorizing_device_id, "authorizing_device_id")?;
    let params = json!({
        "operation_id": operation_id,
        "recovery_session_id": recovery_session_id,
        "authorizing_device_id": authorizing_device_id,
    });
    let proof = sign_device_proof(
        signing_private,
        signing_key_id,
        RECOVERY_CANCEL_PURPOSE,
        RECOVERY_CANCEL_METHOD,
        &params,
        now,
    )?;
    verify_device_proof(
        &proof,
        RECOVERY_CANCEL_PURPOSE,
        RECOVERY_CANCEL_METHOD,
        &params,
        did_document,
    )?;
    Ok(PreparedRecoveryCancel {
        operation_id,
        recovery_session_id,
        authorizing_device_id,
        authorizing_device_proof: proof,
    })
}

pub(crate) fn build_recovery_cancel_call(
    prepared: &PreparedRecoveryCancel,
    did_document: &Value,
) -> crate::ImResult<RecoveryWireCall> {
    let params = json!({
        "operation_id": prepared.operation_id,
        "recovery_session_id": prepared.recovery_session_id,
        "authorizing_device_id": prepared.authorizing_device_id,
    });
    verify_device_proof(
        &prepared.authorizing_device_proof,
        RECOVERY_CANCEL_PURPOSE,
        RECOVERY_CANCEL_METHOD,
        &params,
        did_document,
    )?;
    Ok(call(
        RECOVERY_CANCEL_METHOD,
        json!({
            "operation_id": prepared.operation_id,
            "recovery_session_id": prepared.recovery_session_id,
            "authorizing_device_id": prepared.authorizing_device_id,
            "authorizing_device_proof": prepared.authorizing_device_proof,
        }),
    ))
}

pub(crate) fn parse_recovery_cancel_result(
    raw: Value,
    expected_session_id: &str,
) -> crate::ImResult<RecoveryCancelResult> {
    let result: RecoveryCancelResult = strict(raw, "Recovery cancel result")?;
    if result.recovery_session_id != expected_session_id
        || result.state != RecoveryRemoteState::Cancelled
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(result)
}

pub(crate) fn prepare_recovery_finalize(
    generated: &crate::internal::identity_generation::GeneratedVNextIdentityWithDaemonSubkey,
    operation_id: String,
    expected_handle_mapping_generation: u64,
    now: OffsetDateTime,
) -> crate::ImResult<PreparedRecoveryFinalize> {
    super::device_genesis::validate_generated_document(generated)?;
    if expected_handle_mapping_generation == 0 {
        return Err(crate::ImError::PermissionDenied);
    }
    let operation_id = required(&operation_id, "operation_id")?;
    let params = json!({
        "operation_id": operation_id,
        "expected_handle_mapping_generation": expected_handle_mapping_generation,
        "new_did_document": generated.did_document,
        "bootstrap_device_id": generated.protocol_device_id.as_str(),
    });
    let private = anp::PrivateKeyMaterial::from_pem(&generated.device_signing_private_pem)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    let proof = sign_device_proof(
        &private,
        &generated.device_signing_key_id,
        RECOVERY_FINALIZE_PURPOSE,
        RECOVERY_FINALIZE_METHOD,
        &params,
        now,
    )?;
    verify_device_proof(
        &proof,
        RECOVERY_FINALIZE_PURPOSE,
        RECOVERY_FINALIZE_METHOD,
        &params,
        &generated.did_document,
    )?;
    Ok(PreparedRecoveryFinalize {
        operation_id,
        expected_handle_mapping_generation,
        new_did_document: generated.did_document.clone(),
        bootstrap_device_id: generated.protocol_device_id.as_str().to_owned(),
        bootstrap_device_proof: proof,
    })
}

pub(crate) fn build_recovery_finalize_call(
    prepared: &PreparedRecoveryFinalize,
    recovery_session_token: &str,
    reconfirmation_token: &str,
) -> crate::ImResult<RecoveryWireCall> {
    let params = json!({
        "operation_id": prepared.operation_id,
        "expected_handle_mapping_generation": prepared.expected_handle_mapping_generation,
        "new_did_document": prepared.new_did_document,
        "bootstrap_device_id": prepared.bootstrap_device_id,
    });
    verify_device_proof(
        &prepared.bootstrap_device_proof,
        RECOVERY_FINALIZE_PURPOSE,
        RECOVERY_FINALIZE_METHOD,
        &params,
        &prepared.new_did_document,
    )?;
    Ok(call(
        RECOVERY_FINALIZE_METHOD,
        json!({
            "operation_id": prepared.operation_id,
            "recovery_session_token": required(
                recovery_session_token,
                "recovery_session_token",
            )?,
            "reconfirmation_token": required(
                reconfirmation_token,
                "reconfirmation_token",
            )?,
            "expected_handle_mapping_generation": prepared.expected_handle_mapping_generation,
            "new_did_document": prepared.new_did_document,
            "bootstrap_device_id": prepared.bootstrap_device_id,
            "bootstrap_device_proof": prepared.bootstrap_device_proof,
        }),
    ))
}

pub(crate) fn parse_recovery_finalize_result(
    raw: Value,
    expected_session: &RecoverySessionResult,
    expected_handle: &str,
    expected_user_id: &str,
    expected_mapping_generation: u64,
    generated: &crate::internal::identity_generation::GeneratedVNextIdentityWithDaemonSubkey,
    now: OffsetDateTime,
) -> crate::ImResult<RecoveryFinalizeParseOutcome> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct RawResult {
        recovery_session_id: String,
        state: RecoveryRemoteState,
        old_did: String,
        did: String,
        handle: String,
        handle_mapping_generation: u64,
        checkpoint: RawCheckpoint,
        device: super::device_genesis::GenesisDeviceResult,
        access_token: String,
        refresh_token: String,
        token_expires_at: String,
    }

    impl Drop for RawResult {
        fn drop(&mut self) {
            self.access_token.zeroize();
            self.refresh_token.zeroize();
        }
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct RawCheckpoint {
        document_version: u64,
        document_hash: String,
        registry_version: u64,
    }

    super::device_genesis::validate_generated_document(generated)?;
    let mut raw: RawResult = strict(raw, "Recovery finalize result")?;
    let expected_handle = normalized_full_handle(expected_handle)?;
    let next_mapping_generation = expected_mapping_generation
        .checked_add(1)
        .ok_or(crate::ImError::PermissionDenied)?;
    if raw.recovery_session_id != expected_session.recovery_session_id
        || raw.state != RecoveryRemoteState::Consumed
        || raw.old_did != expected_session.old_did
        || raw.did != generated.did.as_str()
        || raw.did == raw.old_did
        || raw.handle != expected_handle
        || raw.handle_mapping_generation != next_mapping_generation
        || raw.device.device_id != generated.protocol_device_id.as_str()
        || raw.device.signing_key_id != generated.device_signing_key_id
        || raw.device.e2ee_key_id != generated.device_e2ee_key_id
        || raw.device.status != "active"
        || raw.device.role != "admin"
        || !raw.device.management_ready
        || raw.device.auth_generation != 1
        || raw.checkpoint.document_version != 1
        || raw.checkpoint.registry_version != 1
        || raw.checkpoint.document_hash
            != super::device_genesis::document_hash(&generated.did_document)?
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let expected_user_id = required(expected_user_id, "expected_user_id")?;
    if raw.access_token.trim().is_empty()
        || raw.refresh_token.trim().is_empty()
        || raw.access_token.trim() == raw.refresh_token.trim()
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let token_expires_at = parse_time("token_expires_at", &raw.token_expires_at)?;
    let cutover = RecoveryFinalizeCutoverResult {
        recovery_session_id: std::mem::take(&mut raw.recovery_session_id),
        state: raw.state,
        old_did: std::mem::take(&mut raw.old_did),
        did: std::mem::take(&mut raw.did),
        handle: std::mem::take(&mut raw.handle),
        handle_mapping_generation: raw.handle_mapping_generation,
        checkpoint: IdentityInternalCheckpoint {
            document_version: raw.checkpoint.document_version,
            document_hash: std::mem::take(&mut raw.checkpoint.document_hash),
            registry_version: raw.checkpoint.registry_version,
        },
        device: raw.device.clone(),
    };
    if token_expires_at <= now {
        return Ok(RecoveryFinalizeParseOutcome::TokenRefreshRequired(cutover));
    }
    let user_id = super::device_genesis::validate_management_ready_token_pair(
        generated,
        &raw.access_token,
        &raw.refresh_token,
        &raw.token_expires_at,
        now,
    )?;
    if user_id != expected_user_id {
        return Err(crate::ImError::PermissionDenied);
    }
    let result = RecoveryFinalizeResult {
        recovery_session_id: cutover.recovery_session_id,
        state: cutover.state,
        old_did: cutover.old_did,
        did: cutover.did,
        handle: cutover.handle,
        handle_mapping_generation: cutover.handle_mapping_generation,
        user_id,
        checkpoint: cutover.checkpoint,
        device: cutover.device,
        access_token: std::mem::take(&mut raw.access_token),
        refresh_token: std::mem::take(&mut raw.refresh_token),
        token_expires_at: format_time(token_expires_at)?,
    };
    result.device_state().validate_for_did(&generated.did)?;
    Ok(RecoveryFinalizeParseOutcome::Ready(result))
}

fn call(method: &'static str, params: Value) -> RecoveryWireCall {
    RecoveryWireCall {
        endpoint: super::DID_AUTH_RPC_ENDPOINT,
        method,
        params,
    }
}

fn sign_device_proof(
    private: &anp::PrivateKeyMaterial,
    key_id: &str,
    purpose: &str,
    method: &str,
    params: &Value,
    now: OffsetDateTime,
) -> crate::ImResult<DeviceProof> {
    if !matches!(private, anp::PrivateKeyMaterial::Ed25519(_)) {
        return Err(crate::ImError::PermissionDenied);
    }
    let created_at = now.replace_nanosecond(0).unwrap_or(now);
    let expires_at = created_at + Duration::seconds(PROOF_TTL_SECONDS);
    let mut nonce = [0_u8; PROOF_NONCE_LEN];
    rand::rngs::OsRng
        .try_fill_bytes(&mut nonce)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    let mut proof = DeviceProof {
        proof_type: DEVICE_PROOF_TYPE.to_owned(),
        key_id: required(key_id, "signing_key_id")?,
        created_at: format_time(created_at)?,
        expires_at: format_time(expires_at)?,
        nonce: URL_SAFE_NO_PAD.encode(nonce),
        signature: String::new(),
    };
    proof.signature = URL_SAFE_NO_PAD.encode(
        private
            .sign_message(&device_proof_bytes(&proof, purpose, method, params)?)
            .map_err(|_| crate::ImError::PermissionDenied)?,
    );
    Ok(proof)
}

fn verify_device_proof(
    proof: &DeviceProof,
    purpose: &str,
    method: &str,
    params: &Value,
    did_document: &Value,
) -> crate::ImResult<()> {
    if proof.proof_type != DEVICE_PROOF_TYPE {
        return Err(crate::ImError::PermissionDenied);
    }
    let public_method = did_document
        .get("verificationMethod")
        .and_then(Value::as_array)
        .and_then(|methods| {
            methods.iter().find(|candidate| {
                candidate.get("id").and_then(Value::as_str) == Some(&proof.key_id)
            })
        })
        .ok_or(crate::ImError::PermissionDenied)?;
    let verification = anp::authentication::create_verification_method(public_method)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    if !matches!(verification.public_key, anp::PublicKeyMaterial::Ed25519(_)) {
        return Err(crate::ImError::PermissionDenied);
    }
    verification
        .verify_signature(
            &device_proof_bytes(proof, purpose, method, params)?,
            &proof.signature,
        )
        .map_err(|_| crate::ImError::PermissionDenied)
}

fn device_proof_bytes(
    proof: &DeviceProof,
    purpose: &str,
    method: &str,
    params: &Value,
) -> crate::ImResult<Vec<u8>> {
    let params = super::device_genesis::strip_proof_and_token_fields(params);
    serde_json_canonicalizer::to_vec(&json!({
        "type": proof.proof_type,
        "purpose": purpose,
        "method": method,
        "key_id": proof.key_id,
        "created_at": proof.created_at,
        "expires_at": proof.expires_at,
        "nonce": proof.nonce,
        "params": params,
    }))
    .map_err(|error| crate::ImError::Serialization {
        detail: error.to_string(),
    })
}

fn validate_session_window(
    session_id: &str,
    old_did: &str,
    cooling_until: &str,
    expires_at: &str,
    now: OffsetDateTime,
    allow_expired: bool,
) -> crate::ImResult<()> {
    required(session_id, "recovery_session_id")?;
    crate::ids::Did::parse(required(old_did, "old_did")?)?;
    let cooling = parse_time("cooling_until", cooling_until)?;
    let expires = parse_time("expires_at", expires_at)?;
    if expires <= cooling || (!allow_expired && expires <= now) {
        return Err(crate::ImError::SessionExpired);
    }
    Ok(())
}

fn parse_future_time(
    field: &str,
    value: &str,
    now: OffsetDateTime,
) -> crate::ImResult<OffsetDateTime> {
    let value = parse_time(field, value)?;
    if value <= now {
        return Err(crate::ImError::SessionExpired);
    }
    Ok(value)
}

fn parse_time(field: &str, value: &str) -> crate::ImResult<OffsetDateTime> {
    OffsetDateTime::parse(value.trim(), &Rfc3339).map_err(|_| {
        crate::ImError::invalid_input(Some(field.to_owned()), format!("invalid {field}"))
    })
}

fn format_time(value: OffsetDateTime) -> crate::ImResult<String> {
    value
        .format(&Rfc3339)
        .map_err(|error| crate::ImError::Serialization {
            detail: error.to_string(),
        })
}

fn normalized_full_handle(value: &str) -> crate::ImResult<String> {
    let handle = crate::ids::Handle::parse(value.trim(), "")?;
    let normalized = handle.as_str().trim_start_matches('@').to_ascii_lowercase();
    let (local, domain) = normalized.split_once('.').ok_or_else(|| {
        crate::ImError::invalid_input(Some("handle".to_owned()), "full Handle is required")
    })?;
    if local.is_empty()
        || domain.is_empty()
        || normalized != value.trim().trim_start_matches('@').to_ascii_lowercase()
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(normalized)
}

fn required(value: &str, field: &str) -> crate::ImResult<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some(field.to_owned()),
            format!("{field} is required"),
        ));
    }
    Ok(value.to_owned())
}

fn strict<T: for<'de> Deserialize<'de>>(value: Value, context: &str) -> crate::ImResult<T> {
    serde_json::from_value(value).map_err(|_| crate::ImError::Serialization {
        detail: format!("invalid {context}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn begin_call_redacts_token_and_has_exact_wire_shape() {
        let call =
            build_recovery_begin_call("op-begin-1", "account-secret", "alice.awiki.info").unwrap();
        assert_eq!(call.method, RECOVERY_BEGIN_METHOD);
        assert!(
            call.params
                == json!({
                    "operation_id": "op-begin-1",
                    "account_verification_token": "account-secret",
                    "handle": "alice.awiki.info",
                })
        );
        assert!(!format!("{call:?}").contains("account-secret"));
    }

    #[test]
    fn begin_result_binds_the_verified_internal_account_subject() {
        let now = OffsetDateTime::parse("2029-12-31T00:00:00Z", &Rfc3339).unwrap();
        let old_did = crate::ids::Did::parse("did:wba:awiki.info:user:alice:e1_old").unwrap();
        let raw = json!({
            "recovery_session_id": "recovery-1",
            "recovery_session_token": "session-secret",
            "account_user_id": "user-alice",
            "old_did": old_did.as_str(),
            "state": "cooling",
            "cooling_until": "2030-01-01T00:00:00Z",
            "expires_at": "2030-01-02T00:00:00Z",
        });

        let parsed = parse_recovery_begin_result(raw.clone(), &old_did, now).unwrap();
        assert_eq!(parsed.account_user_id, "user-alice");
        assert!(!format!("{parsed:?}").contains("user-alice"));

        for invalid_subject in [None, Some(""), Some(" user-alice ")] {
            let mut invalid = raw.clone();
            match invalid_subject {
                Some(value) => {
                    invalid["account_user_id"] = Value::String(value.to_owned());
                }
                None => {
                    invalid.as_object_mut().unwrap().remove("account_user_id");
                }
            }
            assert!(parse_recovery_begin_result(invalid, &old_did, now).is_err());
        }

        let mut legacy_public_subject = raw;
        legacy_public_subject
            .as_object_mut()
            .unwrap()
            .remove("account_user_id");
        legacy_public_subject["user_id"] = Value::String("user-alice".to_owned());
        assert!(parse_recovery_begin_result(legacy_public_subject, &old_did, now).is_err());
    }

    #[test]
    fn status_parser_rejects_unknown_fields_and_context_changes() {
        let expected = RecoverySessionResult {
            recovery_session_id: "recovery-1".to_owned(),
            recovery_session_token: "secret".to_owned(),
            account_user_id: "user-alice".to_owned(),
            old_did: "did:wba:awiki.info:user:alice:e1_old".to_owned(),
            state: RecoveryRemoteState::Cooling,
            cooling_until: "2030-01-01T00:00:00Z".to_owned(),
            expires_at: "2030-01-02T00:00:00Z".to_owned(),
        };
        let now = OffsetDateTime::parse("2029-12-31T00:00:00Z", &Rfc3339).unwrap();
        let raw = json!({
            "recovery_session_id": "recovery-1",
            "old_did": expected.old_did.clone(),
            "state": "ready",
            "cooling_until": expected.cooling_until.clone(),
            "expires_at": expected.expires_at.clone(),
            "unexpected": true,
        });
        assert!(parse_recovery_status_result(raw, &expected, now).is_err());
    }

    #[test]
    fn finalize_proof_is_bound_to_recovery_method_and_mapping_generation() {
        let generated = crate::internal::identity_generation::generate_vnext_handle_identity_with_default_daemon_subkey(
            "awiki.info", "alice", None, None,
        ).unwrap();
        let now = OffsetDateTime::parse("2029-12-31T00:00:00Z", &Rfc3339).unwrap();
        let prepared =
            prepare_recovery_finalize(&generated, "op-finalize-1".to_owned(), 8, now).unwrap();
        let call =
            build_recovery_finalize_call(&prepared, "session-secret", "confirm-secret").unwrap();
        assert_eq!(call.method, RECOVERY_FINALIZE_METHOD);
        assert_eq!(call.params["expected_handle_mapping_generation"], 8);
        assert!(!format!("{call:?}").contains("session-secret"));
        assert!(!format!("{call:?}").contains("confirm-secret"));

        let mut tampered = prepared.clone();
        tampered.expected_handle_mapping_generation = 9;
        assert!(
            build_recovery_finalize_call(&tampered, "session-secret", "confirm-secret").is_err()
        );
    }

    #[test]
    fn finalize_retry_refreshes_only_proof_evidence() {
        let generated = crate::internal::identity_generation::generate_vnext_handle_identity_with_default_daemon_subkey(
            "awiki.info", "alice", None, None,
        ).unwrap();
        let first_now = OffsetDateTime::parse("2029-12-31T00:00:00Z", &Rfc3339).unwrap();
        let first =
            prepare_recovery_finalize(&generated, "op-finalize-stable".to_owned(), 8, first_now)
                .unwrap();
        let refreshed = prepare_recovery_finalize(
            &generated,
            first.operation_id.clone(),
            first.expected_handle_mapping_generation,
            first_now + Duration::seconds(60),
        )
        .unwrap();

        assert_eq!(refreshed.operation_id, first.operation_id);
        assert_eq!(
            refreshed.expected_handle_mapping_generation,
            first.expected_handle_mapping_generation
        );
        assert_eq!(refreshed.new_did_document, first.new_did_document);
        assert_eq!(refreshed.bootstrap_device_id, first.bootstrap_device_id);
        assert_eq!(
            refreshed.bootstrap_device_proof.key_id,
            first.bootstrap_device_proof.key_id
        );
        assert_ne!(
            refreshed.bootstrap_device_proof.created_at,
            first.bootstrap_device_proof.created_at
        );
        assert_ne!(
            refreshed.bootstrap_device_proof.nonce,
            first.bootstrap_device_proof.nonce
        );
        assert_ne!(
            refreshed.bootstrap_device_proof.signature,
            first.bootstrap_device_proof.signature
        );
    }

    #[test]
    fn expired_finalize_tokens_preserve_cutover_and_require_credential_refresh() {
        let generated = crate::internal::identity_generation::generate_vnext_handle_identity_with_default_daemon_subkey(
            "awiki.info", "alice", None, None,
        ).unwrap();
        let now = OffsetDateTime::parse("2030-01-01T00:00:00Z", &Rfc3339).unwrap();
        let session = RecoverySessionResult {
            recovery_session_id: "recovery-expired-token".to_owned(),
            recovery_session_token: "session-secret".to_owned(),
            account_user_id: "user-alice".to_owned(),
            old_did: "did:wba:awiki.info:user:alice:e1_old".to_owned(),
            state: RecoveryRemoteState::Ready,
            cooling_until: "2029-12-30T00:00:00Z".to_owned(),
            expires_at: "2030-01-02T00:00:00Z".to_owned(),
        };
        let raw = json!({
            "recovery_session_id": session.recovery_session_id,
            "state": "consumed",
            "old_did": session.old_did,
            "did": generated.did.as_str(),
            "handle": "alice.awiki.info",
            "handle_mapping_generation": 9,
            "checkpoint": {
                "document_version": 1,
                "document_hash": super::super::device_genesis::document_hash(
                    &generated.did_document,
                ).unwrap(),
                "registry_version": 1,
            },
            "device": {
                "device_id": generated.protocol_device_id.as_str(),
                "signing_key_id": generated.device_signing_key_id,
                "e2ee_key_id": generated.device_e2ee_key_id,
                "status": "active",
                "role": "admin",
                "management_ready": true,
                "auth_generation": 1,
            },
            // A safe replay of finalize may return the originally issued pair
            // after its access TTL. The client must not persist either token.
            "access_token": "expired-access-token",
            "refresh_token": "expired-refresh-token",
            "token_expires_at": "2029-12-31T23:59:59Z",
        });

        let outcome = parse_recovery_finalize_result(
            raw,
            &session,
            "alice.awiki.info",
            "user-alice",
            8,
            &generated,
            now,
        )
        .unwrap();
        let cutover = match outcome {
            RecoveryFinalizeParseOutcome::TokenRefreshRequired(cutover) => cutover,
            RecoveryFinalizeParseOutcome::Ready(_) => panic!("expired token pair was accepted"),
        };
        assert_eq!(cutover.recovery_session_id, session.recovery_session_id);
        assert_eq!(cutover.old_did, session.old_did);
        assert_eq!(cutover.did, generated.did.as_str());
        assert_eq!(cutover.handle_mapping_generation, 9);
        assert_eq!(cutover.checkpoint.document_version, 1);
        assert_eq!(cutover.checkpoint.registry_version, 1);
        assert_eq!(
            cutover.device.device_id,
            generated.protocol_device_id.as_str()
        );
    }
}
