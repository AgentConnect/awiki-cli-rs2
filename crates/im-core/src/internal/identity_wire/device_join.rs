//! Strict AWiki-local wire contract for device Registry and Join JSON-RPC.
//!
//! This module maps only the frozen first-party methods. It rejects unknown
//! response fields and invalid state/field combinations before they reach the
//! local Join state machine. Token-bearing calls redact their params in Debug.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::identity::{DeviceJoinChallenge, DeviceJoinChallengeResponse, DeviceJoinRequest};
use crate::internal::identity_device_join_runtime::{
    DeviceJoinRemoteAdminStatus, DeviceJoinRemoteApproveRequest, DeviceJoinRemoteApproveResult,
    DeviceJoinRemoteAuthorization, DeviceJoinRemoteChallengeResult, DeviceJoinRemoteClaimRequest,
    DeviceJoinRemoteClaimResult, DeviceJoinRemoteCreateRequest, DeviceJoinRemoteCreateResult,
    DeviceJoinRemoteDeviceSummary, DeviceJoinRemoteNewDeviceStatus, DeviceJoinRemotePendingSummary,
    DeviceJoinRemoteRegistry, DeviceJoinRemoteResponseRequest, DeviceJoinRemoteState,
    DeviceJoinRemoteTransitionResult,
};
use crate::internal::identity_device_state::{
    DeviceAuthorizationRole, DeviceAuthorizationStatus, IdentityInternalCheckpoint,
};
use crate::internal::platform_secret::SecretBytes;

pub(crate) const DEVICE_REGISTRY_GET_METHOD: &str = "device_registry_get";
pub(crate) const DEVICE_JOIN_CREATE_METHOD: &str = "device_join_create";
pub(crate) const DEVICE_JOIN_STATUS_METHOD: &str = "device_join_status";
pub(crate) const DEVICE_JOIN_CLAIM_METHOD: &str = "device_join_claim";
pub(crate) const DEVICE_JOIN_CHALLENGE_METHOD: &str = "device_join_challenge";
pub(crate) const DEVICE_JOIN_CHALLENGE_RESPONSE_METHOD: &str = "device_join_challenge_response";
pub(crate) const DEVICE_JOIN_APPROVE_METHOD: &str = "device_join_approve";

pub(crate) struct DeviceJoinWireCall {
    pub(crate) endpoint: &'static str,
    pub(crate) method: &'static str,
    pub(crate) params: Value,
    sensitive: bool,
}

impl std::fmt::Debug for DeviceJoinWireCall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut debug = f.debug_struct("DeviceJoinWireCall");
        debug
            .field("endpoint", &self.endpoint)
            .field("method", &self.method);
        if self.sensitive {
            debug.field("params", &"<redacted>");
        } else {
            debug.field("params", &self.params);
        }
        debug.finish()
    }
}

pub(crate) fn build_registry_call(
    did: &crate::ids::Did,
    include_pending_join_requests: bool,
) -> DeviceJoinWireCall {
    call(
        DEVICE_REGISTRY_GET_METHOD,
        json!({
            "did": did.as_str(),
            "include_pending_join_requests": include_pending_join_requests,
        }),
        false,
    )
}

pub(crate) fn build_create_call(
    request: DeviceJoinRemoteCreateRequest<'_>,
) -> crate::ImResult<DeviceJoinWireCall> {
    Ok(call(
        DEVICE_JOIN_CREATE_METHOD,
        json!({
            "operation_id": request.operation_id,
            "account_verification_token": secret_token(
                request.account_verification_token,
                "account_verification_token",
            )?,
            "join_request": request.join_request,
        }),
        true,
    ))
}

pub(crate) fn build_new_status_call(
    join_session_token: &SecretBytes,
) -> crate::ImResult<DeviceJoinWireCall> {
    Ok(call(
        DEVICE_JOIN_STATUS_METHOD,
        json!({
            "join_session_token": secret_token(join_session_token, "join_session_token")?,
        }),
        true,
    ))
}

pub(crate) fn build_admin_status_call(
    join_session_id: &str,
) -> crate::ImResult<DeviceJoinWireCall> {
    Ok(call(
        DEVICE_JOIN_STATUS_METHOD,
        json!({"join_session_id": required("join_session_id", join_session_id)?}),
        false,
    ))
}

pub(crate) fn build_claim_call(
    request: DeviceJoinRemoteClaimRequest<'_>,
) -> crate::ImResult<DeviceJoinWireCall> {
    Ok(call(
        DEVICE_JOIN_CLAIM_METHOD,
        json!({
            "operation_id": required("operation_id", request.operation_id)?,
            "join_session_id": required("join_session_id", request.join_session_id)?,
            "authorizing_device_id": required(
                "authorizing_device_id",
                request.authorizing_device_id,
            )?,
            "authorizing_device_proof": request.authorizing_device_proof,
        }),
        false,
    ))
}

pub(crate) fn build_challenge_call(
    challenge: &DeviceJoinChallenge,
) -> crate::ImResult<DeviceJoinWireCall> {
    Ok(call(
        DEVICE_JOIN_CHALLENGE_METHOD,
        serde_json::to_value(challenge).map_err(serialization_error)?,
        false,
    ))
}

pub(crate) fn build_response_call(
    request: DeviceJoinRemoteResponseRequest<'_>,
) -> crate::ImResult<DeviceJoinWireCall> {
    let response = request.response;
    Ok(call(
        DEVICE_JOIN_CHALLENGE_RESPONSE_METHOD,
        json!({
            "operation_id": response.operation_id,
            "join_session_token": secret_token(
                request.join_session_token,
                "join_session_token",
            )?,
            "join_session_id": response.join_session_id,
            "challenge_id": response.challenge_id,
            "challenge_hash": response.challenge_hash,
            "join_request_hash": response.join_request_hash,
            "pairing_transcript_hash": response.pairing_transcript_hash,
            "new_device_proof": response.new_device_proof,
        }),
        true,
    ))
}

pub(crate) fn build_approve_call(
    request: DeviceJoinRemoteApproveRequest<'_>,
) -> crate::ImResult<DeviceJoinWireCall> {
    Ok(call(
        DEVICE_JOIN_APPROVE_METHOD,
        json!({
            "operation_id": required("operation_id", request.operation_id)?,
            "join_session_id": required("join_session_id", request.join_session_id)?,
            "expected_document_version": request.expected_checkpoint.document_version,
            "expected_document_hash": request.expected_checkpoint.document_hash,
            "expected_registry_version": request.expected_checkpoint.registry_version,
            "role": request.role,
            "new_document": request.new_document,
            "pairing_confirmation": request.pairing_confirmation,
            "authorizing_device_id": required(
                "authorizing_device_id",
                request.authorizing_device_id,
            )?,
            "authorizing_device_proof": request.authorizing_device_proof,
        }),
        false,
    ))
}

pub(crate) fn parse_registry_result(
    raw: Value,
    expected_did: &crate::ids::Did,
    include_pending_join_requests: bool,
) -> crate::ImResult<DeviceJoinRemoteRegistry> {
    let raw: RawRegistry = parse(raw, "device_registry_get result")?;
    if raw.did != expected_did.as_str() {
        return Err(invalid_wire("Registry DID does not match the request"));
    }
    if !include_pending_join_requests && raw.pending_join_requests.is_some() {
        return Err(invalid_wire(
            "Registry unexpectedly returned pending Join requests",
        ));
    }
    let checkpoint = raw.checkpoint.try_into()?;
    let devices = raw
        .devices
        .into_iter()
        .map(|device| device.into_validated(expected_did))
        .collect::<crate::ImResult<Vec<_>>>()?;
    let pending_join_requests = raw
        .pending_join_requests
        .unwrap_or_default()
        .into_iter()
        .map(|pending| pending.into_validated(expected_did))
        .collect::<crate::ImResult<Vec<_>>>()?;
    Ok(DeviceJoinRemoteRegistry {
        did: expected_did.clone(),
        checkpoint,
        devices,
        pending_join_requests,
    })
}

pub(crate) fn parse_create_result(
    raw: Value,
    expected_join_session_id: &str,
) -> crate::ImResult<DeviceJoinRemoteCreateResult> {
    let raw: RawCreateResult = parse(raw, "device_join_create result")?;
    require_session(&raw.join_session_id, expected_join_session_id)?;
    require_state(raw.state, DeviceJoinRemoteState::Pending, "create")?;
    Ok(DeviceJoinRemoteCreateResult {
        join_session_id: raw.join_session_id,
        join_session_token: nonempty_secret(raw.join_session_token, "join_session_token")?,
        state: raw.state,
        expires_at: required("expires_at", &raw.expires_at)?,
    })
}

pub(crate) fn parse_new_status_result(
    raw: Value,
    expected_join_session_id: &str,
) -> crate::ImResult<DeviceJoinRemoteNewDeviceStatus> {
    let raw: RawNewStatus = parse(raw, "new-device device_join_status result")?;
    require_session(&raw.join_session_id, expected_join_session_id)?;
    validate_status(
        raw.state,
        raw.challenge.as_ref(),
        None,
        raw.authorization.as_ref(),
        false,
    )?;
    validate_status_bindings(&raw.join_session_id, raw.challenge.as_ref(), None)?;
    Ok(DeviceJoinRemoteNewDeviceStatus {
        join_session_id: required("join_session_id", &raw.join_session_id)?,
        state: raw.state,
        expires_at: required("expires_at", &raw.expires_at)?,
        challenge: raw.challenge,
        authorization: raw.authorization.map(TryInto::try_into).transpose()?,
    })
}

pub(crate) fn parse_admin_status_result(
    raw: Value,
    expected_join_session_id: &str,
) -> crate::ImResult<DeviceJoinRemoteAdminStatus> {
    let raw: RawAdminStatus = parse(raw, "admin device_join_status result")?;
    require_session(&raw.join_session_id, expected_join_session_id)?;
    validate_status(
        raw.state,
        raw.challenge.as_ref(),
        raw.challenge_response.as_ref(),
        raw.authorization.as_ref(),
        true,
    )?;
    validate_status_bindings(
        &raw.join_session_id,
        raw.challenge.as_ref(),
        raw.challenge_response.as_ref(),
    )?;
    Ok(DeviceJoinRemoteAdminStatus {
        join_session_id: raw.join_session_id,
        state: raw.state,
        expires_at: required("expires_at", &raw.expires_at)?,
        challenge: raw.challenge,
        challenge_response: raw.challenge_response,
        authorization: raw.authorization.map(TryInto::try_into).transpose()?,
    })
}

pub(crate) fn parse_claim_result(
    raw: Value,
    expected_join_session_id: &str,
) -> crate::ImResult<DeviceJoinRemoteClaimResult> {
    let raw: RawClaimResult = parse(raw, "device_join_claim result")?;
    require_session(&raw.join_session_id, expected_join_session_id)?;
    require_state(raw.state, DeviceJoinRemoteState::Claimed, "claim")?;
    if raw.join_request.join_session_id != raw.join_session_id {
        return Err(invalid_wire("claimed Join Request session does not match"));
    }
    Ok(DeviceJoinRemoteClaimResult {
        join_session_id: raw.join_session_id,
        state: raw.state,
        claimed_by_device_id: required("claimed_by_device_id", &raw.claimed_by_device_id)?,
        claim_expires_at: required("claim_expires_at", &raw.claim_expires_at)?,
        join_request: raw.join_request,
    })
}

pub(crate) fn parse_challenge_result(
    raw: Value,
    challenge: &DeviceJoinChallenge,
) -> crate::ImResult<DeviceJoinRemoteChallengeResult> {
    let raw: RawChallengeResult = parse(raw, "device_join_challenge result")?;
    require_session(&raw.join_session_id, &challenge.join_session_id)?;
    require_state(raw.state, DeviceJoinRemoteState::ChallengeSent, "challenge")?;
    if raw.challenge_id != challenge.challenge_id {
        return Err(invalid_wire("challenge result id does not match"));
    }
    Ok(DeviceJoinRemoteChallengeResult {
        join_session_id: raw.join_session_id,
        state: raw.state,
        challenge_id: raw.challenge_id,
    })
}

pub(crate) fn parse_response_result(
    raw: Value,
    response: &DeviceJoinChallengeResponse,
) -> crate::ImResult<DeviceJoinRemoteTransitionResult> {
    let raw: RawTransitionResult = parse(raw, "device_join_challenge_response result")?;
    require_session(&raw.join_session_id, &response.join_session_id)?;
    require_state(
        raw.state,
        DeviceJoinRemoteState::ResponseVerified,
        "challenge response",
    )?;
    Ok(DeviceJoinRemoteTransitionResult {
        join_session_id: raw.join_session_id,
        state: raw.state,
    })
}

pub(crate) fn parse_approve_result(
    raw: Value,
    expected_join_session_id: &str,
) -> crate::ImResult<DeviceJoinRemoteApproveResult> {
    let raw: RawApproveResult = parse(raw, "device_join_approve result")?;
    require_session(&raw.join_session_id, expected_join_session_id)?;
    require_state(raw.state, DeviceJoinRemoteState::Consumed, "approve")?;
    Ok(DeviceJoinRemoteApproveResult {
        join_session_id: raw.join_session_id,
        state: raw.state,
        checkpoint: raw.checkpoint.try_into()?,
        device: raw.device.into_unbound_validated()?,
    })
}

fn call(method: &'static str, params: Value, sensitive: bool) -> DeviceJoinWireCall {
    DeviceJoinWireCall {
        endpoint: super::DID_AUTH_RPC_ENDPOINT,
        method,
        params,
        sensitive,
    }
}

fn secret_token(secret: &SecretBytes, field: &str) -> crate::ImResult<String> {
    let token = std::str::from_utf8(secret.expose_secret()).map_err(|_| {
        crate::ImError::invalid_input(Some(field.to_owned()), format!("{field} must be UTF-8"))
    })?;
    if token.is_empty() || token.trim() != token {
        return Err(crate::ImError::invalid_input(
            Some(field.to_owned()),
            format!("{field} is invalid"),
        ));
    }
    Ok(token.to_owned())
}

fn nonempty_secret(value: String, field: &str) -> crate::ImResult<SecretBytes> {
    if value.is_empty() || value.trim() != value {
        return Err(invalid_wire(format!("{field} is empty")));
    }
    Ok(SecretBytes::from_vec(value.into_bytes()))
}

fn parse<T: DeserializeOwned>(raw: Value, context: &str) -> crate::ImResult<T> {
    serde_json::from_value(raw).map_err(|error| invalid_wire(format!("invalid {context}: {error}")))
}

fn serialization_error(error: serde_json::Error) -> crate::ImError {
    crate::ImError::Serialization {
        detail: error.to_string(),
    }
}

fn invalid_wire(detail: impl Into<String>) -> crate::ImError {
    crate::ImError::Serialization {
        detail: detail.into(),
    }
}

fn required(field: &str, value: &str) -> crate::ImResult<String> {
    if value.is_empty() || value.trim() != value {
        return Err(invalid_wire(format!("{field} is required")));
    }
    Ok(value.to_owned())
}

fn require_session(actual: &str, expected: &str) -> crate::ImResult<()> {
    if actual != expected || actual.trim().is_empty() {
        return Err(invalid_wire("Join session id does not match"));
    }
    Ok(())
}

fn require_state(
    actual: DeviceJoinRemoteState,
    expected: DeviceJoinRemoteState,
    operation: &str,
) -> crate::ImResult<()> {
    if actual != expected {
        return Err(invalid_wire(format!(
            "{operation} returned unexpected state {actual:?}"
        )));
    }
    Ok(())
}

fn validate_status(
    state: DeviceJoinRemoteState,
    challenge: Option<&DeviceJoinChallenge>,
    response: Option<&DeviceJoinChallengeResponse>,
    authorization: Option<&RawAuthorization>,
    admin_view: bool,
) -> crate::ImResult<()> {
    let challenge_expected = matches!(
        state,
        DeviceJoinRemoteState::ChallengeSent | DeviceJoinRemoteState::ResponseVerified
    );
    if challenge.is_some() != challenge_expected {
        return Err(invalid_wire("Join status challenge/state invariant failed"));
    }
    let authorization_expected = state == DeviceJoinRemoteState::Consumed;
    if authorization.is_some() != authorization_expected {
        return Err(invalid_wire(
            "Join status authorization/state invariant failed",
        ));
    }
    if !admin_view && response.is_some() {
        return Err(invalid_wire(
            "new-device Join status exposed a challenge response",
        ));
    }
    let response_valid = match state {
        DeviceJoinRemoteState::ResponseVerified if admin_view => response.is_some(),
        DeviceJoinRemoteState::ResponseVerified => response.is_none(),
        DeviceJoinRemoteState::Consumed if admin_view => true,
        _ => response.is_none(),
    };
    if !response_valid {
        return Err(invalid_wire("Join status response/state invariant failed"));
    }
    Ok(())
}

fn validate_status_bindings(
    join_session_id: &str,
    challenge: Option<&DeviceJoinChallenge>,
    response: Option<&DeviceJoinChallengeResponse>,
) -> crate::ImResult<()> {
    required("join_session_id", join_session_id)?;
    if challenge.is_some_and(|challenge| challenge.join_session_id != join_session_id) {
        return Err(invalid_wire("status challenge session does not match"));
    }
    if response.is_some_and(|response| response.join_session_id != join_session_id) {
        return Err(invalid_wire("status response session does not match"));
    }
    if let (Some(challenge), Some(response)) = (challenge, response) {
        if challenge.challenge_id != response.challenge_id {
            return Err(invalid_wire("status challenge/response ids do not match"));
        }
    }
    Ok(())
}

fn validate_checkpoint(checkpoint: &IdentityInternalCheckpoint) -> crate::ImResult<()> {
    if checkpoint.document_version == 0 || checkpoint.registry_version == 0 {
        return Err(invalid_wire(
            "identity checkpoint versions must be positive",
        ));
    }
    let encoded = checkpoint
        .document_hash
        .strip_prefix("sha256:")
        .ok_or_else(|| invalid_wire("document hash must use sha256:base64url-no-padding"))?;
    let decoded = URL_SAFE_NO_PAD
        .decode(encoded.as_bytes())
        .map_err(|_| invalid_wire("document hash is not valid base64url"))?;
    if decoded.len() != 32 || URL_SAFE_NO_PAD.encode(&decoded) != encoded {
        return Err(invalid_wire(
            "document hash must encode exactly 32 SHA-256 bytes",
        ));
    }
    Ok(())
}

fn validate_device_summary(
    device: &DeviceJoinRemoteDeviceSummary,
    did: Option<&crate::ids::Did>,
) -> crate::ImResult<()> {
    crate::ids::ProtocolDeviceId::parse(&device.device_id)
        .map_err(|_| invalid_wire("invalid protocol device id"))?;
    if device.auth_generation == 0
        || device.signing_key_id.trim().is_empty()
        || device.e2ee_key_id.trim().is_empty()
        || device.signing_key_id == device.e2ee_key_id
        || (device.role == DeviceAuthorizationRole::Member && device.management_ready)
        || (device.status == DeviceAuthorizationStatus::Revoked && device.management_ready)
    {
        return Err(invalid_wire("invalid device summary"));
    }
    if let Some(did) = did {
        let prefix = format!("{}#", did.as_str());
        if !device.signing_key_id.starts_with(&prefix) || !device.e2ee_key_id.starts_with(&prefix) {
            return Err(invalid_wire("device key id is outside the Registry DID"));
        }
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCheckpoint {
    document_version: u64,
    document_hash: String,
    registry_version: u64,
}

impl TryFrom<RawCheckpoint> for IdentityInternalCheckpoint {
    type Error = crate::ImError;

    fn try_from(value: RawCheckpoint) -> Result<Self, Self::Error> {
        let checkpoint = Self {
            document_version: value.document_version,
            document_hash: value.document_hash,
            registry_version: value.registry_version,
        };
        validate_checkpoint(&checkpoint)?;
        Ok(checkpoint)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDeviceSummary {
    device_id: String,
    signing_key_id: String,
    e2ee_key_id: String,
    status: DeviceAuthorizationStatus,
    role: DeviceAuthorizationRole,
    management_ready: bool,
    auth_generation: u64,
}

impl RawDeviceSummary {
    fn into_validated(
        self,
        did: &crate::ids::Did,
    ) -> crate::ImResult<DeviceJoinRemoteDeviceSummary> {
        self.into_summary(Some(did))
    }

    fn into_unbound_validated(self) -> crate::ImResult<DeviceJoinRemoteDeviceSummary> {
        self.into_summary(None)
    }

    fn into_summary(
        self,
        did: Option<&crate::ids::Did>,
    ) -> crate::ImResult<DeviceJoinRemoteDeviceSummary> {
        let summary = DeviceJoinRemoteDeviceSummary {
            device_id: self.device_id,
            signing_key_id: self.signing_key_id,
            e2ee_key_id: self.e2ee_key_id,
            status: self.status,
            role: self.role,
            management_ready: self.management_ready,
            auth_generation: self.auth_generation,
        };
        validate_device_summary(&summary, did)?;
        Ok(summary)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPendingSummary {
    join_session_id: String,
    device_id: String,
    signing_key_id: String,
    e2ee_key_id: String,
    requested_role: DeviceAuthorizationRole,
    issued_at: String,
    expires_at: String,
}

impl RawPendingSummary {
    fn into_validated(
        self,
        did: &crate::ids::Did,
    ) -> crate::ImResult<DeviceJoinRemotePendingSummary> {
        if self.requested_role != DeviceAuthorizationRole::Member {
            return Err(invalid_wire("pending Join role must be member"));
        }
        crate::ids::ProtocolDeviceId::parse(&self.device_id)
            .map_err(|_| invalid_wire("invalid pending protocol device id"))?;
        let prefix = format!("{}#", did.as_str());
        if self.signing_key_id == self.e2ee_key_id
            || !self.signing_key_id.starts_with(&prefix)
            || !self.e2ee_key_id.starts_with(&prefix)
        {
            return Err(invalid_wire("invalid pending Join key ids"));
        }
        Ok(DeviceJoinRemotePendingSummary {
            join_session_id: required("join_session_id", &self.join_session_id)?,
            device_id: self.device_id,
            signing_key_id: self.signing_key_id,
            e2ee_key_id: self.e2ee_key_id,
            requested_role: self.requested_role,
            issued_at: required("issued_at", &self.issued_at)?,
            expires_at: required("expires_at", &self.expires_at)?,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRegistry {
    did: String,
    checkpoint: RawCheckpoint,
    devices: Vec<RawDeviceSummary>,
    #[serde(default)]
    pending_join_requests: Option<Vec<RawPendingSummary>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCreateResult {
    join_session_id: String,
    join_session_token: String,
    state: DeviceJoinRemoteState,
    expires_at: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAuthorization {
    checkpoint: RawCheckpoint,
    device: RawDeviceSummary,
}

impl TryFrom<RawAuthorization> for DeviceJoinRemoteAuthorization {
    type Error = crate::ImError;

    fn try_from(value: RawAuthorization) -> Result<Self, Self::Error> {
        Ok(Self {
            checkpoint: value.checkpoint.try_into()?,
            device: value.device.into_unbound_validated()?,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawNewStatus {
    join_session_id: String,
    state: DeviceJoinRemoteState,
    expires_at: String,
    #[serde(default)]
    challenge: Option<DeviceJoinChallenge>,
    #[serde(default)]
    authorization: Option<RawAuthorization>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAdminStatus {
    join_session_id: String,
    state: DeviceJoinRemoteState,
    expires_at: String,
    #[serde(default)]
    challenge: Option<DeviceJoinChallenge>,
    #[serde(default)]
    challenge_response: Option<DeviceJoinChallengeResponse>,
    #[serde(default)]
    authorization: Option<RawAuthorization>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawClaimResult {
    join_session_id: String,
    state: DeviceJoinRemoteState,
    claimed_by_device_id: String,
    claim_expires_at: String,
    join_request: DeviceJoinRequest,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawChallengeResult {
    join_session_id: String,
    state: DeviceJoinRemoteState,
    challenge_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTransitionResult {
    join_session_id: String,
    state: DeviceJoinRemoteState,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawApproveResult {
    join_session_id: String,
    state: DeviceJoinRemoteState,
    checkpoint: RawCheckpoint,
    device: RawDeviceSummary,
}

#[cfg(test)]
mod tests;
