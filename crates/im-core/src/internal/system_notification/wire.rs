use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub(crate) const DIRECT_PROFILE: &str = "anp.direct.base.v2";
pub(crate) const TRANSPORT_SECURITY: &str = "transport-protected";
pub(crate) const JSON_CONTENT_TYPE: &str = "application/json";
pub(crate) const SYSTEM_EVENT_TYPE: &str = "system.notification";
const JOIN_TYPE_PREFIX: &str = "awiki.device.join-";

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SystemNotificationEnvelope {
    pub(crate) meta: DirectMeta,
    pub(crate) auth: DirectAuth,
    pub(crate) body: DirectBody,
    pub(crate) notification: JoinNotification,
    pub(crate) signed_meta: Value,
    pub(crate) signed_body: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DirectMeta {
    pub(crate) anp_version: String,
    pub(crate) profile: String,
    pub(crate) security_profile: String,
    pub(crate) sender_did: String,
    pub(crate) target: DirectTarget,
    pub(crate) operation_id: String,
    pub(crate) message_id: String,
    pub(crate) created_at: String,
    pub(crate) content_type: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DirectTarget {
    pub(crate) kind: String,
    pub(crate) did: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DirectAuth {
    pub(crate) scheme: String,
    pub(crate) origin_proof: anp::proof::Rfc9421OriginProof,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DirectBody {
    pub(crate) payload: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct JoinNotification {
    pub(crate) kind: crate::system_notifications::SystemNotificationKind,
    pub(crate) event_id: String,
    pub(crate) did: String,
    pub(crate) join_session_id: String,
    pub(crate) state: crate::system_notifications::SystemNotificationState,
    pub(crate) session_revision: u64,
    pub(crate) issued_at: String,
    pub(crate) expires_at: String,
    pub(crate) payload: JoinPayload,
    pub(crate) initial_join_request: Option<JoinRequest>,
    pub(crate) canonical_value: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum JoinPayload {
    Requested(JoinRequestedPayload),
    Claimed(JoinClaimedPayload),
    ResponseVerified(JoinResponseVerifiedPayload),
    Completed(JoinCompletedPayload),
    Cancelled(JoinCancelledPayload),
    Rejected(JoinRejectedPayload),
    Expired(JoinExpiredPayload),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct JoinRequestedPayload {
    pub(crate) join_request: JoinRequest,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct JoinClaimedPayload {
    pub(crate) state: String,
    pub(crate) claimed_by_device_id: String,
    pub(crate) challenge_id: String,
    pub(crate) challenge_expires_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct JoinResponseVerifiedPayload {
    pub(crate) claimed_by_device_id: String,
    pub(crate) challenge_response: ChallengeResponse,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChallengeResponse {
    pub(crate) operation_id: String,
    pub(crate) join_session_id: String,
    pub(crate) challenge_id: String,
    pub(crate) challenge_hash: String,
    pub(crate) join_request_hash: String,
    pub(crate) pairing_transcript_hash: String,
    pub(crate) response_signature_b64u: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct JoinCompletedPayload {
    pub(crate) state: String,
    pub(crate) checkpoint: JoinCheckpoint,
    pub(crate) device: JoinedDevice,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct JoinCheckpoint {
    pub(crate) document_version: u64,
    pub(crate) document_hash: String,
    pub(crate) registry_version: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct JoinedDevice {
    pub(crate) device_id: String,
    pub(crate) signing_key_id: String,
    pub(crate) e2ee_key_id: String,
    pub(crate) status: String,
    pub(crate) role: String,
    pub(crate) management_ready: bool,
    pub(crate) auth_generation: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct JoinCancelledPayload {
    pub(crate) state: String,
    pub(crate) reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct JoinRejectedPayload {
    pub(crate) state: String,
    pub(crate) reason: String,
    pub(crate) rejected_by_device_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct JoinExpiredPayload {
    pub(crate) state: String,
    pub(crate) reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct JoinRequest {
    #[serde(rename = "type")]
    pub(crate) request_type: String,
    pub(crate) did: String,
    pub(crate) join_session_id: String,
    pub(crate) device_id: String,
    pub(crate) signing_public_key: VerificationMethod,
    pub(crate) e2ee_public_key: VerificationMethod,
    pub(crate) pairing_public_key: String,
    pub(crate) profiles: Vec<String>,
    pub(crate) requested_role: String,
    pub(crate) issued_at: String,
    pub(crate) expires_at: String,
    pub(crate) join_request_proof: JoinRequestProof,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct VerificationMethod {
    pub(crate) id: String,
    #[serde(rename = "type")]
    pub(crate) method_type: String,
    pub(crate) controller: String,
    #[serde(rename = "publicKeyJwk")]
    pub(crate) public_key_jwk: OkpJwk,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OkpJwk {
    pub(crate) kty: String,
    pub(crate) crv: String,
    pub(crate) x: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct JoinRequestProof {
    #[serde(rename = "type")]
    pub(crate) proof_type: String,
    pub(crate) algorithm: String,
    pub(crate) verification_method: String,
    pub(crate) created_at: String,
    pub(crate) proof_value_b64u: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CommonNotification {
    #[serde(rename = "type")]
    notification_type: String,
    event_id: String,
    did: String,
    join_session_id: String,
    state: String,
    session_revision: u64,
    issued_at: String,
    expires_at: String,
    payload: Value,
}

pub(crate) fn is_system_namespace(value: &Value) -> bool {
    payload_value(value)
        .and_then(|payload| payload.get("type"))
        .and_then(Value::as_str)
        .is_some_and(|value| value.starts_with(JOIN_TYPE_PREFIX))
}

pub(crate) fn is_trusted_delivery_marker(value: &Value) -> bool {
    value.get("projection_kind").and_then(Value::as_str) == Some("system_notification")
}

pub(crate) fn is_system_notification_hint(value: &Value) -> bool {
    value.get("event_type").and_then(Value::as_str) == Some(SYSTEM_EVENT_TYPE)
}

pub(crate) fn parse_envelope(value: &Value) -> crate::ImResult<SystemNotificationEnvelope> {
    if value.get("method").and_then(Value::as_str) != Some("direct.incoming") {
        return Err(invalid(
            "system notification method must be direct.incoming",
        ));
    }
    let params = value
        .get("params")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid("system notification params must be an object"))?;
    let signed_meta = required_object_value(params, "meta")?;
    let signed_body = required_object_value(params, "body")?;
    let meta: DirectMeta = strict_decode(signed_meta.clone(), "meta")?;
    let auth: DirectAuth = strict_decode(
        params
            .get("auth")
            .cloned()
            .ok_or_else(|| invalid("system notification auth is required"))?,
        "auth",
    )?;
    let body: DirectBody = strict_decode(signed_body.clone(), "body")?;
    let notification = parse_verified_notification(body.payload.clone())?;
    Ok(SystemNotificationEnvelope {
        meta,
        auth,
        body,
        notification,
        signed_meta,
        signed_body,
    })
}

pub(crate) fn parse_verified_notification(value: Value) -> crate::ImResult<JoinNotification> {
    let common: CommonNotification = strict_decode(value.clone(), "body.payload")?;
    let kind = kind_for_type(&common.notification_type)?;
    let state = state_for_value(&common.state)?;
    let expected_state = state_for_kind(kind);
    if state != expected_state {
        return Err(invalid("notification type and state do not match"));
    }
    let payload = match kind {
        crate::system_notifications::SystemNotificationKind::JoinRequested => {
            JoinPayload::Requested(strict_decode(common.payload, "payload")?)
        }
        crate::system_notifications::SystemNotificationKind::JoinClaimed => {
            JoinPayload::Claimed(strict_decode(common.payload, "payload")?)
        }
        crate::system_notifications::SystemNotificationKind::JoinResponseVerified => {
            JoinPayload::ResponseVerified(strict_decode(common.payload, "payload")?)
        }
        crate::system_notifications::SystemNotificationKind::JoinCompleted => {
            JoinPayload::Completed(strict_decode(common.payload, "payload")?)
        }
        crate::system_notifications::SystemNotificationKind::JoinCancelled => {
            JoinPayload::Cancelled(strict_decode(common.payload, "payload")?)
        }
        crate::system_notifications::SystemNotificationKind::JoinRejected => {
            JoinPayload::Rejected(strict_decode(common.payload, "payload")?)
        }
        crate::system_notifications::SystemNotificationKind::JoinExpired => {
            JoinPayload::Expired(strict_decode(common.payload, "payload")?)
        }
    };
    let initial_join_request = match &payload {
        JoinPayload::Requested(payload) => Some(payload.join_request.clone()),
        _ => None,
    };
    Ok(JoinNotification {
        kind,
        event_id: common.event_id,
        did: common.did,
        join_session_id: common.join_session_id,
        state,
        session_revision: common.session_revision,
        issued_at: common.issued_at,
        expires_at: common.expires_at,
        payload,
        initial_join_request,
        canonical_value: value,
    })
}

fn payload_value(value: &Value) -> Option<&Value> {
    value
        .pointer("/params/body/payload")
        .or_else(|| value.pointer("/body/payload"))
}

fn required_object_value(params: &Map<String, Value>, key: &str) -> crate::ImResult<Value> {
    params
        .get(key)
        .filter(|value| value.is_object())
        .cloned()
        .ok_or_else(|| invalid(format!("system notification {key} must be an object")))
}

fn strict_decode<T: for<'de> Deserialize<'de>>(value: Value, field: &str) -> crate::ImResult<T> {
    serde_json::from_value(value).map_err(|_| {
        invalid(format!(
            "system notification {field} does not match closed schema"
        ))
    })
}

fn kind_for_type(
    value: &str,
) -> crate::ImResult<crate::system_notifications::SystemNotificationKind> {
    crate::system_notifications::SystemNotificationKind::parse(value)
        .map_err(|_| invalid("unknown system notification type"))
}

fn state_for_value(
    value: &str,
) -> crate::ImResult<crate::system_notifications::SystemNotificationState> {
    crate::system_notifications::SystemNotificationState::parse(value)
        .map_err(|_| invalid("unknown system notification state"))
}

fn state_for_kind(
    kind: crate::system_notifications::SystemNotificationKind,
) -> crate::system_notifications::SystemNotificationState {
    use crate::system_notifications::{SystemNotificationKind as K, SystemNotificationState as S};
    match kind {
        K::JoinRequested => S::Pending,
        K::JoinClaimed => S::ChallengeSent,
        K::JoinResponseVerified => S::ResponseVerified,
        K::JoinCompleted => S::Consumed,
        K::JoinCancelled => S::Cancelled,
        K::JoinRejected => S::Rejected,
        K::JoinExpired => S::Expired,
    }
}

pub(crate) fn invalid(message: impl Into<String>) -> crate::ImError {
    crate::ImError::Service {
        status_code: None,
        code: Some("system.notification.invalid_notification".to_owned()),
        message: message.into(),
        data: None,
    }
}
