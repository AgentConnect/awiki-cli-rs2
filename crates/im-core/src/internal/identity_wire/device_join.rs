//! Closed AWiki-local wire contract for the frozen v1 device Join RPCs.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::identity::{DeviceJoinChallenge, DeviceJoinChallengeResponse};
use crate::internal::identity_device_join_runtime::{
    DeviceJoinRemoteApproveRequest, DeviceJoinRemoteApproveResult, DeviceJoinRemoteAuthorization,
    DeviceJoinRemoteCancelRequest, DeviceJoinRemoteChallengeResult, DeviceJoinRemoteCreateRequest,
    DeviceJoinRemoteCreateResult, DeviceJoinRemoteDeviceSummary, DeviceJoinRemoteNewDeviceStatus,
    DeviceJoinRemoteRegistry, DeviceJoinRemoteRejectRequest, DeviceJoinRemoteResponseRequest,
    DeviceJoinRemoteState, DeviceJoinRemoteTransitionResult,
};
use crate::internal::identity_device_state::{
    DeviceAuthorizationRole, DeviceAuthorizationStatus, IdentityInternalCheckpoint,
};
use crate::internal::platform_secret::SecretBytes;

pub(crate) const DEVICE_REGISTRY_GET_METHOD: &str = "device_registry_get";
pub(crate) const DEVICE_JOIN_CREATE_METHOD: &str = "device_join_create";
pub(crate) const DEVICE_JOIN_STATUS_METHOD: &str = "device_join_status";
pub(crate) const DEVICE_JOIN_SUBMIT_CHALLENGE_METHOD: &str = "device_join_submit_challenge";
pub(crate) const DEVICE_JOIN_CHALLENGE_RESPONSE_METHOD: &str = "device_join_challenge_response";
pub(crate) const DEVICE_JOIN_APPROVE_METHOD: &str = "device_join_approve";
pub(crate) const DEVICE_JOIN_REJECT_METHOD: &str = "device_join_reject";
pub(crate) const DEVICE_JOIN_CANCEL_METHOD: &str = "device_join_cancel";

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
    _compatibility_flag: bool,
) -> DeviceJoinWireCall {
    call(
        DEVICE_REGISTRY_GET_METHOD,
        json!({"did": did.as_str()}),
        false,
    )
}

pub(crate) fn build_create_call(
    request: DeviceJoinRemoteCreateRequest<'_>,
) -> crate::ImResult<DeviceJoinWireCall> {
    Ok(call(
        DEVICE_JOIN_CREATE_METHOD,
        json!({
            "operation_id": required("operation_id", request.operation_id)?,
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

pub(crate) fn build_submit_challenge_call(
    challenge: &DeviceJoinChallenge,
) -> crate::ImResult<DeviceJoinWireCall> {
    Ok(call(
        DEVICE_JOIN_SUBMIT_CHALLENGE_METHOD,
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
            "operation_id": required("operation_id", &response.operation_id)?,
            "join_session_token": secret_token(
                request.join_session_token,
                "join_session_token",
            )?,
            "join_session_id": required("join_session_id", &response.join_session_id)?,
            "challenge_id": required("challenge_id", &response.challenge_id)?,
            "challenge_hash": required("challenge_hash", &response.challenge_hash)?,
            "join_request_hash": required(
                "join_request_hash",
                &response.join_request_hash,
            )?,
            "pairing_transcript_hash": required(
                "pairing_transcript_hash",
                &response.pairing_transcript_hash,
            )?,
            "response_signature_b64u": required(
                "response_signature_b64u",
                &response.response_signature_b64u,
            )?,
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
            "new_document": request.new_document,
            "pairing_confirmation": request.pairing_confirmation,
            "authorizing_device_id": required(
                "authorizing_device_id",
                request.authorizing_device_id,
            )?,
            "proof": request.proof,
        }),
        false,
    ))
}

pub(crate) fn build_reject_call(
    request: DeviceJoinRemoteRejectRequest<'_>,
) -> crate::ImResult<DeviceJoinWireCall> {
    Ok(call(
        DEVICE_JOIN_REJECT_METHOD,
        json!({
            "operation_id": required("operation_id", request.operation_id)?,
            "join_session_id": required("join_session_id", request.join_session_id)?,
            "rejecting_device_id": required(
                "rejecting_device_id",
                request.rejecting_device_id,
            )?,
            "reason": match request.reason {
                "user_rejected" | "sas_mismatch" => request.reason,
                _ => return Err(invalid_wire("invalid device Join reject reason")),
            },
            "proof": request.proof,
        }),
        false,
    ))
}

pub(crate) fn build_cancel_call(
    request: DeviceJoinRemoteCancelRequest<'_>,
) -> crate::ImResult<DeviceJoinWireCall> {
    if request.reason != "user_cancelled" {
        return Err(invalid_wire("invalid device Join cancel reason"));
    }
    Ok(call(
        DEVICE_JOIN_CANCEL_METHOD,
        json!({
            "operation_id": required("operation_id", request.operation_id)?,
            "join_session_token": secret_token(
                request.join_session_token,
                "join_session_token",
            )?,
            "join_session_id": required("join_session_id", request.join_session_id)?,
            "reason": request.reason,
        }),
        true,
    ))
}

pub(crate) fn parse_registry_result(
    raw: Value,
    expected_did: &crate::ids::Did,
    _compatibility_flag: bool,
) -> crate::ImResult<DeviceJoinRemoteRegistry> {
    let raw: RawRegistry = parse(raw, "device_registry_get result")?;
    if raw.did != expected_did.as_str() {
        return Err(invalid_wire("Registry DID does not match the request"));
    }
    Ok(DeviceJoinRemoteRegistry {
        did: expected_did.clone(),
        checkpoint: raw.checkpoint.try_into()?,
        devices: raw
            .devices
            .into_iter()
            .map(|device| device.into_validated(Some(expected_did)))
            .collect::<crate::ImResult<Vec<_>>>()?,
    })
}

pub(crate) fn parse_create_result(
    raw: Value,
    expected_join_session_id: &str,
) -> crate::ImResult<DeviceJoinRemoteCreateResult> {
    let raw: RawCreateResult = parse(raw, "device_join_create result")?;
    require_session(&raw.join_session_id, expected_join_session_id)?;
    require_state(raw.state, DeviceJoinRemoteState::Pending, "create")?;
    require_revision_for_state(raw.state, raw.session_revision)?;
    validate_utc_time("expires_at", &raw.expires_at)?;
    Ok(DeviceJoinRemoteCreateResult {
        join_session_id: raw.join_session_id,
        join_session_token: nonempty_secret(raw.join_session_token, "join_session_token")?,
        state: raw.state,
        session_revision: raw.session_revision,
        expires_at: required("expires_at", &raw.expires_at)?,
    })
}

pub(crate) fn parse_new_status_result(
    raw: Value,
    expected_join_session_id: &str,
) -> crate::ImResult<DeviceJoinRemoteNewDeviceStatus> {
    reject_explicit_nulls(
        &raw,
        &[
            "challenge",
            "authorization",
            "claimed_by_device_id",
            "challenge_id",
            "pairing_transcript_hash",
            "reason",
            "rejected_by_device_id",
            "occurred_at",
        ],
    )?;
    let raw: RawStatus = parse(raw, "new-device device_join_status result")?;
    require_session(&raw.join_session_id, expected_join_session_id)?;
    require_revision_for_state(raw.state, raw.session_revision)?;
    validate_utc_time("expires_at", &raw.expires_at)?;
    validate_status(&raw)?;
    Ok(DeviceJoinRemoteNewDeviceStatus {
        join_session_id: raw.join_session_id,
        state: raw.state,
        session_revision: raw.session_revision,
        expires_at: required("expires_at", &raw.expires_at)?,
        challenge: raw.challenge,
        authorization: raw.authorization.map(TryInto::try_into).transpose()?,
    })
}

pub(crate) fn parse_challenge_result(
    raw: Value,
    challenge: &DeviceJoinChallenge,
) -> crate::ImResult<DeviceJoinRemoteChallengeResult> {
    let raw: RawSubmitChallengeResult = parse(raw, "device_join_submit_challenge result")?;
    require_session(&raw.join_session_id, &challenge.join_session_id)?;
    require_state(
        raw.state,
        DeviceJoinRemoteState::ChallengeSent,
        "submit challenge",
    )?;
    require_revision_for_state(raw.state, raw.session_revision)?;
    if raw.challenge_id != challenge.challenge_id
        || raw.challenge_expires_at != challenge.challenge_expires_at
        || raw.claimed_by_device_id != challenge.admin_device_id
    {
        return Err(invalid_wire("submit challenge result binding mismatch"));
    }
    Ok(DeviceJoinRemoteChallengeResult {
        join_session_id: raw.join_session_id,
        state: raw.state,
        session_revision: raw.session_revision,
        claimed_by_device_id: raw.claimed_by_device_id,
        challenge_id: raw.challenge_id,
        challenge_expires_at: raw.challenge_expires_at,
    })
}

pub(crate) fn parse_response_result(
    raw: Value,
    response: &DeviceJoinChallengeResponse,
) -> crate::ImResult<DeviceJoinRemoteTransitionResult> {
    parse_transition(
        raw,
        &response.join_session_id,
        DeviceJoinRemoteState::ResponseVerified,
        "device_join_challenge_response result",
    )
}

pub(crate) fn parse_reject_result(
    raw: Value,
    expected_join_session_id: &str,
) -> crate::ImResult<DeviceJoinRemoteTransitionResult> {
    parse_transition(
        raw,
        expected_join_session_id,
        DeviceJoinRemoteState::Rejected,
        "device_join_reject result",
    )
}

pub(crate) fn parse_cancel_result(
    raw: Value,
    expected_join_session_id: &str,
) -> crate::ImResult<DeviceJoinRemoteTransitionResult> {
    parse_transition(
        raw,
        expected_join_session_id,
        DeviceJoinRemoteState::Cancelled,
        "device_join_cancel result",
    )
}

pub(crate) fn parse_approve_result(
    raw: Value,
    expected_join_session_id: &str,
) -> crate::ImResult<DeviceJoinRemoteApproveResult> {
    let raw: RawApproveResult = parse(raw, "device_join_approve result")?;
    require_session(&raw.join_session_id, expected_join_session_id)?;
    require_state(raw.state, DeviceJoinRemoteState::Consumed, "approve")?;
    require_revision_for_state(raw.state, raw.session_revision)?;
    let device = raw.device.into_validated(None)?;
    validate_join_authorization_device(&device)?;
    Ok(DeviceJoinRemoteApproveResult {
        join_session_id: raw.join_session_id,
        state: raw.state,
        session_revision: raw.session_revision,
        checkpoint: raw.checkpoint.try_into()?,
        device,
    })
}

fn parse_transition(
    raw: Value,
    expected_join_session_id: &str,
    expected_state: DeviceJoinRemoteState,
    context: &str,
) -> crate::ImResult<DeviceJoinRemoteTransitionResult> {
    let raw: RawTransitionResult = parse(raw, context)?;
    require_session(&raw.join_session_id, expected_join_session_id)?;
    require_state(raw.state, expected_state, context)?;
    require_revision_for_state(raw.state, raw.session_revision)?;
    Ok(DeviceJoinRemoteTransitionResult {
        join_session_id: raw.join_session_id,
        state: raw.state,
        session_revision: raw.session_revision,
    })
}

fn validate_status(raw: &RawStatus) -> crate::ImResult<()> {
    let no_terminal_fields =
        raw.reason.is_none() && raw.occurred_at.is_none() && raw.rejected_by_device_id.is_none();
    let valid = match raw.state {
        DeviceJoinRemoteState::Pending => {
            raw.challenge.is_none()
                && raw.authorization.is_none()
                && raw.claimed_by_device_id.is_none()
                && raw.challenge_id.is_none()
                && raw.pairing_transcript_hash.is_none()
                && no_terminal_fields
        }
        DeviceJoinRemoteState::ChallengeSent => {
            raw.challenge.is_some()
                && raw.authorization.is_none()
                && raw.claimed_by_device_id.is_none()
                && raw.challenge_id.is_none()
                && raw.pairing_transcript_hash.is_none()
                && no_terminal_fields
        }
        DeviceJoinRemoteState::ResponseVerified => {
            raw.challenge.is_none()
                && raw.authorization.is_none()
                && raw.claimed_by_device_id.is_some()
                && raw.challenge_id.is_some()
                && raw.pairing_transcript_hash.is_some()
                && no_terminal_fields
        }
        DeviceJoinRemoteState::Consumed => {
            raw.authorization.is_some()
                && raw.challenge.is_none()
                && raw.claimed_by_device_id.is_none()
                && raw.challenge_id.is_none()
                && raw.pairing_transcript_hash.is_none()
                && no_terminal_fields
        }
        DeviceJoinRemoteState::Cancelled | DeviceJoinRemoteState::Expired => {
            raw.challenge.is_none()
                && raw.authorization.is_none()
                && raw.claimed_by_device_id.is_none()
                && raw.challenge_id.is_none()
                && raw.pairing_transcript_hash.is_none()
                && raw.reason.is_some()
                && raw.occurred_at.is_some()
                && raw.rejected_by_device_id.is_none()
        }
        DeviceJoinRemoteState::Rejected => {
            raw.challenge.is_none()
                && raw.authorization.is_none()
                && raw.claimed_by_device_id.is_none()
                && raw.challenge_id.is_none()
                && raw.pairing_transcript_hash.is_none()
                && raw.reason.is_some()
                && raw.occurred_at.is_some()
                && raw.rejected_by_device_id.is_some()
        }
    };
    if !valid {
        return Err(invalid_wire("Join status state/field invariant failed"));
    }
    if let Some(challenge) = raw.challenge.as_ref() {
        if challenge.join_session_id != raw.join_session_id {
            return Err(invalid_wire("status challenge session does not match"));
        }
        validate_status_challenge(challenge, &raw.expires_at)?;
    }
    if let Some(authorization) = raw.authorization.as_ref() {
        let device = authorization.device.clone().into_validated(None)?;
        validate_join_authorization_device(&device)?;
    }
    match raw.state {
        DeviceJoinRemoteState::Cancelled => {
            if raw.reason.as_deref() != Some("user_cancelled") {
                return Err(invalid_wire("cancelled status reason is invalid"));
            }
        }
        DeviceJoinRemoteState::Rejected => {
            if !matches!(
                raw.reason.as_deref(),
                Some("user_rejected" | "sas_mismatch")
            ) {
                return Err(invalid_wire("rejected status reason is invalid"));
            }
            crate::ids::ProtocolDeviceId::parse(
                raw.rejected_by_device_id.as_deref().unwrap_or_default(),
            )
            .map_err(|_| invalid_wire("rejected_by_device_id is invalid"))?;
        }
        DeviceJoinRemoteState::Expired => {
            if !matches!(
                raw.reason.as_deref(),
                Some("session_expired" | "challenge_expired")
            ) {
                return Err(invalid_wire("expired status reason is invalid"));
            }
        }
        _ => {}
    }
    if let Some(occurred_at) = raw.occurred_at.as_deref() {
        validate_utc_time("occurred_at", occurred_at)?;
    }
    if raw.state == DeviceJoinRemoteState::ResponseVerified {
        crate::ids::ProtocolDeviceId::parse(
            raw.claimed_by_device_id.as_deref().unwrap_or_default(),
        )
        .map_err(|_| invalid_wire("claimed_by_device_id is invalid"))?;
        required(
            "challenge_id",
            raw.challenge_id.as_deref().unwrap_or_default(),
        )?;
        validate_sha256(
            "pairing_transcript_hash",
            raw.pairing_transcript_hash.as_deref().unwrap_or_default(),
        )?;
    }
    Ok(())
}

fn validate_status_challenge(
    challenge: &DeviceJoinChallenge,
    session_expires_at: &str,
) -> crate::ImResult<()> {
    required("challenge.operation_id", &challenge.operation_id)?;
    required("challenge.challenge_id", &challenge.challenge_id)?;
    crate::ids::ProtocolDeviceId::parse(&challenge.admin_device_id)
        .map_err(|_| invalid_wire("challenge.admin_device_id is invalid"))?;
    validate_utc_time(
        "challenge.challenge_expires_at",
        &challenge.challenge_expires_at,
    )?;
    if OffsetDateTime::parse(&challenge.challenge_expires_at, &Rfc3339)
        .map_err(|_| invalid_wire("challenge.challenge_expires_at is invalid"))?
        > OffsetDateTime::parse(session_expires_at, &Rfc3339)
            .map_err(|_| invalid_wire("expires_at is invalid"))?
    {
        return Err(invalid_wire(
            "challenge expiry exceeds the Join session expiry",
        ));
    }
    Ok(())
}

fn validate_join_authorization_device(
    device: &DeviceJoinRemoteDeviceSummary,
) -> crate::ImResult<()> {
    if device.role != DeviceAuthorizationRole::Member || device.management_ready {
        return Err(invalid_wire(
            "device Join authorization must be a rootless member",
        ));
    }
    Ok(())
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

fn require_revision_for_state(state: DeviceJoinRemoteState, revision: u64) -> crate::ImResult<()> {
    let valid = match state {
        DeviceJoinRemoteState::Pending => revision == 1,
        DeviceJoinRemoteState::ChallengeSent => revision == 2,
        DeviceJoinRemoteState::ResponseVerified => revision == 3,
        DeviceJoinRemoteState::Consumed => revision == 4,
        DeviceJoinRemoteState::Cancelled
        | DeviceJoinRemoteState::Rejected
        | DeviceJoinRemoteState::Expired => (2..=4).contains(&revision),
    };
    if !valid {
        return Err(invalid_wire(
            "session_revision does not match the Join state",
        ));
    }
    Ok(())
}

fn validate_checkpoint(checkpoint: &IdentityInternalCheckpoint) -> crate::ImResult<()> {
    if checkpoint.document_version == 0 || checkpoint.registry_version == 0 {
        return Err(invalid_wire(
            "identity checkpoint versions must be positive",
        ));
    }
    validate_sha256("document_hash", &checkpoint.document_hash)
}

fn validate_sha256(field: &str, value: &str) -> crate::ImResult<()> {
    let encoded = value
        .strip_prefix("sha256:")
        .ok_or_else(|| invalid_wire(format!("{field} must use sha256:base64url-no-padding")))?;
    let decoded = URL_SAFE_NO_PAD
        .decode(encoded.as_bytes())
        .map_err(|_| invalid_wire(format!("{field} is not valid base64url")))?;
    if decoded.len() != 32 || URL_SAFE_NO_PAD.encode(&decoded) != encoded {
        return Err(invalid_wire(format!(
            "{field} must encode exactly 32 SHA-256 bytes"
        )));
    }
    Ok(())
}

fn validate_utc_time(field: &str, value: &str) -> crate::ImResult<()> {
    required(field, value)?;
    if !value.ends_with('Z') || OffsetDateTime::parse(value, &Rfc3339).is_err() {
        return Err(invalid_wire(format!("{field} must be UTC RFC 3339")));
    }
    Ok(())
}

fn reject_explicit_nulls(raw: &Value, optional_fields: &[&str]) -> crate::ImResult<()> {
    let object = raw
        .as_object()
        .ok_or_else(|| invalid_wire("Join result must be an object"))?;
    if optional_fields
        .iter()
        .any(|field| object.get(*field).is_some_and(Value::is_null))
    {
        return Err(invalid_wire(
            "Join result optional fields must be omitted instead of null",
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

#[derive(Clone, Deserialize)]
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
struct RawRegistry {
    did: String,
    checkpoint: RawCheckpoint,
    devices: Vec<RawDeviceSummary>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCreateResult {
    join_session_id: String,
    join_session_token: String,
    state: DeviceJoinRemoteState,
    session_revision: u64,
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
        let device = value.device.into_validated(None)?;
        validate_join_authorization_device(&device)?;
        Ok(Self {
            checkpoint: value.checkpoint.try_into()?,
            device,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawStatus {
    join_session_id: String,
    state: DeviceJoinRemoteState,
    session_revision: u64,
    expires_at: String,
    #[serde(default)]
    challenge: Option<DeviceJoinChallenge>,
    #[serde(default)]
    authorization: Option<RawAuthorization>,
    #[serde(default)]
    claimed_by_device_id: Option<String>,
    #[serde(default)]
    challenge_id: Option<String>,
    #[serde(default)]
    pairing_transcript_hash: Option<String>,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    rejected_by_device_id: Option<String>,
    #[serde(default)]
    occurred_at: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSubmitChallengeResult {
    join_session_id: String,
    state: DeviceJoinRemoteState,
    session_revision: u64,
    claimed_by_device_id: String,
    challenge_id: String,
    challenge_expires_at: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTransitionResult {
    join_session_id: String,
    state: DeviceJoinRemoteState,
    session_revision: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawApproveResult {
    join_session_id: String,
    state: DeviceJoinRemoteState,
    session_revision: u64,
    checkpoint: RawCheckpoint,
    device: RawDeviceSummary,
}

#[cfg(test)]
mod tests;
