use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Duration, Utc};
use ed25519_dalek::{Signature, Verifier as _, VerifyingKey};
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;

use super::wire::{
    JoinNotification, JoinPayload, SystemNotificationEnvelope, DIRECT_PROFILE, JSON_CONTENT_TYPE,
    TRANSPORT_SECURITY,
};

const ORIGIN_PROOF_SCHEME: &str = "anp-rfc9421-origin-proof-v1";
const JOIN_REQUEST_TYPE: &str = "awiki.device.join.v1";
const JOIN_REQUEST_PROOF_TYPE: &str = "awiki.device.join-request-proof.v1";
const JOIN_REQUEST_PROOF_INPUT_TYPE: &str = "awiki.device.join-request-proof-input.v1";
const MAX_ACTIVE_TTL_SECONDS: i64 = 600;
const MAX_TERMINAL_TTL_SECONDS: i64 = 86_400;
const CLOCK_SKEW_SECONDS: i64 = 30;
const EXPECTED_PROFILES: [&str; 6] = [
    "anp.core.binding.v2",
    "anp.identity.discovery.v2",
    "anp.direct.base.v2",
    "anp.direct.e2ee.v2",
    "anp.group.base.v2",
    "anp.group.e2ee.v2",
];

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct VerifiedSystemNotification {
    pub(crate) envelope: SystemNotificationEnvelope,
    pub(crate) payload_hash: String,
    pub(crate) proof_hash: String,
}

pub(crate) fn verify_with_transport<T>(
    transport: &mut T,
    local_did: &str,
    value: &Value,
    received_at: DateTime<Utc>,
) -> crate::ImResult<VerifiedSystemNotification>
where
    T: crate::internal::transport::RpcTransport,
{
    let envelope = super::wire::parse_envelope(value)?;
    validate_shape_and_payload(local_did, &envelope, received_at)?;
    let local_document = resolve_did_document(transport, local_did)?;
    let service_did = select_unique_service_did(&local_document)?;
    validate_system_notification_origin_did(&envelope.meta.sender_did, &service_did)?;
    let origin_document = resolve_did_document(transport, &envelope.meta.sender_did)?;
    validate_e1_origin_key_binding(&envelope.meta.sender_did, &origin_document)?;
    verify_origin_proof(&envelope, &envelope.meta.sender_did, origin_document)?;
    verified(envelope)
}

pub(crate) async fn verify_with_transport_async<T>(
    transport: &mut T,
    local_did: &str,
    value: &Value,
    received_at: DateTime<Utc>,
) -> crate::ImResult<VerifiedSystemNotification>
where
    T: crate::internal::transport::AsyncRpcTransport,
{
    let envelope = super::wire::parse_envelope(value)?;
    validate_shape_and_payload(local_did, &envelope, received_at)?;
    let local_document = resolve_did_document_async(transport, local_did).await?;
    let service_did = select_unique_service_did(&local_document)?;
    validate_system_notification_origin_did(&envelope.meta.sender_did, &service_did)?;
    let origin_document = resolve_did_document_async(transport, &envelope.meta.sender_did).await?;
    validate_e1_origin_key_binding(&envelope.meta.sender_did, &origin_document)?;
    verify_origin_proof(&envelope, &envelope.meta.sender_did, origin_document)?;
    verified(envelope)
}

fn resolve_did_document<T>(transport: &mut T, did: &str) -> crate::ImResult<Value>
where
    T: crate::internal::transport::RpcTransport,
{
    let url = crate::internal::discovery::did_document::did_document_url(did)?;
    let document = transport.directory_get_json_url(
        &url,
        BTreeMap::from([("Accept".to_owned(), "application/json".to_owned())]),
    )?;
    crate::internal::discovery::did_document::validate_resolved_did_document(did, document)
}

async fn resolve_did_document_async<T>(transport: &mut T, did: &str) -> crate::ImResult<Value>
where
    T: crate::internal::transport::AsyncRpcTransport,
{
    let url = crate::internal::discovery::did_document::did_document_url(did)?;
    let document = transport
        .directory_get_json_url(
            &url,
            BTreeMap::from([("Accept".to_owned(), "application/json".to_owned())]),
        )
        .await?;
    crate::internal::discovery::did_document::validate_resolved_did_document(did, document)
}

fn verified(envelope: SystemNotificationEnvelope) -> crate::ImResult<VerifiedSystemNotification> {
    let payload_hash = canonical_hash(&envelope.notification.canonical_value)?;
    let proof_hash =
        canonical_hash(&serde_json::to_value(&envelope.auth.origin_proof).map_err(serialization)?)?;
    Ok(VerifiedSystemNotification {
        envelope,
        payload_hash,
        proof_hash,
    })
}

fn validate_shape_and_payload(
    local_did: &str,
    envelope: &SystemNotificationEnvelope,
    received_at: DateTime<Utc>,
) -> crate::ImResult<()> {
    if envelope.meta.anp_version != "2.0"
        || envelope.meta.profile != DIRECT_PROFILE
        || envelope.meta.security_profile != TRANSPORT_SECURITY
        || envelope.meta.content_type != JSON_CONTENT_TYPE
        || envelope.meta.target.kind != "agent"
        || envelope.auth.scheme != ORIGIN_PROOF_SCHEME
    {
        return Err(super::wire::invalid(
            "system notification P3 profile is invalid",
        ));
    }
    let notification = &envelope.notification;
    if envelope.meta.target.did != local_did || notification.did != local_did {
        return Err(super::wire::invalid(
            "system notification target DID does not match local identity",
        ));
    }
    if envelope.meta.operation_id != notification.event_id
        || envelope.meta.message_id != notification.event_id
    {
        return Err(super::wire::invalid(
            "event_id, operation_id, and message_id must match",
        ));
    }
    if !valid_prefixed_id(&notification.event_id, "evt-")
        || !valid_prefixed_id(&notification.join_session_id, "join-")
        || notification.session_revision == 0
    {
        return Err(super::wire::invalid(
            "system notification identifiers and revision are required",
        ));
    }
    let issued = parse_time(&notification.issued_at)?;
    let expires = parse_time(&notification.expires_at)?;
    let meta_created = parse_time(&envelope.meta.created_at)?;
    if meta_created != issued || expires <= issued {
        return Err(super::wire::invalid(
            "system notification time binding is invalid",
        ));
    }
    let max_ttl = if notification.state.is_terminal() {
        MAX_TERMINAL_TTL_SECONDS
    } else {
        MAX_ACTIVE_TTL_SECONDS
    };
    if expires - issued > Duration::seconds(max_ttl)
        || issued > received_at + Duration::seconds(CLOCK_SKEW_SECONDS)
        || expires <= received_at
    {
        return Err(system_error(
            "system.notification.expired",
            "system notification is outside its validity window",
        ));
    }
    verify_proof_time_binding(envelope, issued.timestamp(), expires.timestamp())?;
    validate_type_payload(notification, received_at)
}

fn validate_type_payload(
    notification: &JoinNotification,
    received_at: DateTime<Utc>,
) -> crate::ImResult<()> {
    match &notification.payload {
        JoinPayload::Requested(payload) => {
            if notification.session_revision != 1 {
                return Err(invalid_payload());
            }
            verify_join_request(
                &payload.join_request,
                &notification.did,
                &notification.join_session_id,
                received_at,
            )?;
        }
        JoinPayload::Claimed(payload) => {
            if notification.session_revision != 2
                || payload.state != "challenge_sent"
                || payload.claimed_by_device_id.trim().is_empty()
                || payload.challenge_id.trim().is_empty()
            {
                return Err(invalid_payload());
            }
            let challenge_expires = parse_time(&payload.challenge_expires_at)?;
            let notification_expires = parse_time(&notification.expires_at)?;
            if challenge_expires > notification_expires {
                return Err(invalid_payload());
            }
        }
        JoinPayload::ResponseVerified(payload) => {
            let response = &payload.challenge_response;
            if notification.session_revision != 3
                || payload.claimed_by_device_id.trim().is_empty()
                || response.operation_id.trim().is_empty()
                || response.challenge_id.trim().is_empty()
                || response.join_session_id != notification.join_session_id
                || !valid_hash(&response.challenge_hash)
                || !valid_hash(&response.join_request_hash)
                || !valid_hash(&response.pairing_transcript_hash)
                || decode_canonical_b64u(&response.response_signature_b64u, 64).is_err()
            {
                return Err(invalid_payload());
            }
        }
        JoinPayload::Completed(payload) => {
            if notification.session_revision != 4
                || payload.state != "consumed"
                || payload.checkpoint.document_version == 0
                || payload.checkpoint.registry_version == 0
                || !valid_hash(&payload.checkpoint.document_hash)
                || payload.device.device_id.trim().is_empty()
                || payload.device.signing_key_id.trim().is_empty()
                || payload.device.e2ee_key_id.trim().is_empty()
                || payload.device.status != "active"
                || payload.device.role != "member"
                || payload.device.management_ready
                || payload.device.auth_generation != 1
            {
                return Err(invalid_payload());
            }
        }
        JoinPayload::Cancelled(payload) => {
            validate_terminal_revision(notification.session_revision)?;
            if payload.state != "cancelled" || payload.reason != "user_cancelled" {
                return Err(invalid_payload());
            }
        }
        JoinPayload::Rejected(payload) => {
            validate_terminal_revision(notification.session_revision)?;
            if payload.state != "rejected"
                || !matches!(payload.reason.as_str(), "user_rejected" | "sas_mismatch")
                || payload.rejected_by_device_id.trim().is_empty()
            {
                return Err(invalid_payload());
            }
        }
        JoinPayload::Expired(payload) => {
            validate_terminal_revision(notification.session_revision)?;
            if payload.state != "expired"
                || !matches!(
                    payload.reason.as_str(),
                    "session_expired" | "challenge_expired"
                )
            {
                return Err(invalid_payload());
            }
        }
    }
    Ok(())
}

fn verify_join_request(
    request: &super::wire::JoinRequest,
    expected_did: &str,
    expected_session: &str,
    received_at: DateTime<Utc>,
) -> crate::ImResult<()> {
    if request.request_type != JOIN_REQUEST_TYPE
        || request.did != expected_did
        || request.join_session_id != expected_session
        || request.requested_role != "member"
        || request.profiles
            != EXPECTED_PROFILES
                .iter()
                .map(|value| (*value).to_owned())
                .collect::<Vec<_>>()
    {
        return Err(invalid_join_request());
    }
    crate::ids::ProtocolDeviceId::parse(&request.device_id).map_err(|_| invalid_join_request())?;
    validate_key(
        &request.signing_public_key,
        "Ed25519",
        expected_did,
        &request.device_id,
        "sign",
    )?;
    validate_key(
        &request.e2ee_public_key,
        "X25519",
        expected_did,
        &request.device_id,
        "e2ee",
    )?;
    decode_canonical_b64u(&request.pairing_public_key, 32).map_err(|_| invalid_join_request())?;
    let issued = parse_time(&request.issued_at)?;
    let expires = parse_time(&request.expires_at)?;
    if expires <= issued
        || expires - issued > Duration::seconds(MAX_ACTIVE_TTL_SECONDS)
        || issued > received_at + Duration::seconds(CLOCK_SKEW_SECONDS)
        || expires <= received_at
    {
        return Err(invalid_join_request());
    }
    let proof = &request.join_request_proof;
    if proof.proof_type != JOIN_REQUEST_PROOF_TYPE
        || proof.algorithm != "Ed25519"
        || proof.verification_method != request.signing_public_key.id
        || proof.created_at != request.issued_at
    {
        return Err(invalid_join_request());
    }
    let public_key = decode_canonical_b64u(&request.signing_public_key.public_key_jwk.x, 32)
        .map_err(|_| invalid_join_request())?;
    let signature =
        decode_canonical_b64u(&proof.proof_value_b64u, 64).map_err(|_| invalid_join_request())?;
    let unsigned_request = json!({
        "type": request.request_type,
        "did": request.did,
        "join_session_id": request.join_session_id,
        "device_id": request.device_id,
        "signing_public_key": request.signing_public_key,
        "e2ee_public_key": request.e2ee_public_key,
        "pairing_public_key": request.pairing_public_key,
        "profiles": request.profiles,
        "requested_role": request.requested_role,
        "issued_at": request.issued_at,
        "expires_at": request.expires_at,
    });
    let proof_options = json!({
        "type": proof.proof_type,
        "algorithm": proof.algorithm,
        "verification_method": proof.verification_method,
        "created_at": proof.created_at,
    });
    let signing_input = json!({
        "type": JOIN_REQUEST_PROOF_INPUT_TYPE,
        "join_request": unsigned_request,
        "proof_options": proof_options,
    });
    let canonical = serde_json_canonicalizer::to_vec(&signing_input).map_err(serialization)?;
    let public_key: [u8; 32] = public_key.try_into().map_err(|_| invalid_join_request())?;
    let signature: [u8; 64] = signature.try_into().map_err(|_| invalid_join_request())?;
    VerifyingKey::from_bytes(&public_key)
        .map_err(|_| invalid_join_request())?
        .verify(&canonical, &Signature::from_bytes(&signature))
        .map_err(|_| invalid_join_request())
}

fn validate_key(
    method: &super::wire::VerificationMethod,
    curve: &str,
    did: &str,
    device_id: &str,
    suffix: &str,
) -> crate::ImResult<()> {
    if method.method_type != "JsonWebKey2020"
        || method.controller != did
        || method.id != format!("{did}#{device_id}-{suffix}")
        || method.public_key_jwk.kty != "OKP"
        || method.public_key_jwk.crv != curve
    {
        return Err(invalid_join_request());
    }
    decode_canonical_b64u(&method.public_key_jwk.x, 32)
        .map(|_| ())
        .map_err(|_| invalid_join_request())
}

fn verify_origin_proof(
    envelope: &SystemNotificationEnvelope,
    origin_did: &str,
    mut origin_document: Value,
) -> crate::ImResult<()> {
    normalize_ed25519_jwk_methods(&mut origin_document);
    anp::proof::verify_rfc9421_origin_proof(
        &envelope.auth.origin_proof,
        "direct.send",
        &envelope.signed_meta,
        &envelope.signed_body,
        anp::proof::Rfc9421OriginProofVerificationOptions {
            did_document: Some(origin_document),
            expected_signer_did: Some(origin_did.to_owned()),
            ..anp::proof::Rfc9421OriginProofVerificationOptions::default()
        },
    )
    .map(|_| ())
    .map_err(|_| {
        system_error(
            "system.notification.invalid_origin_proof",
            "system notification origin proof is invalid",
        )
    })
}

fn validate_system_notification_origin_did(
    origin_did: &str,
    home_service_did: &str,
) -> crate::ImResult<()> {
    let home_domain = did_wba_domain(home_service_did).ok_or_else(service_mismatch)?;
    let prefix = format!("did:wba:{home_domain}:agents:system-notification:e1_");
    let Some(fingerprint) = origin_did.strip_prefix(&prefix) else {
        return Err(service_mismatch());
    };
    if fingerprint.len() != 43
        || !fingerprint
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(service_mismatch());
    }
    Ok(())
}

fn validate_e1_origin_key_binding(origin_did: &str, document: &Value) -> crate::ImResult<()> {
    if document.get("id").and_then(Value::as_str) != Some(origin_did)
        || !anp::authentication::validate_did_document_binding(document, true)
    {
        return Err(service_mismatch());
    }
    let authentication = document
        .get("authentication")
        .and_then(Value::as_array)
        .ok_or_else(service_mismatch)?;
    let Some(authentication_id) = (authentication.len() == 1)
        .then(|| authentication[0].as_str())
        .flatten()
    else {
        return Err(service_mismatch());
    };
    let matching_keys = document
        .get("verificationMethod")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .filter(|method| method.get("controller").and_then(Value::as_str) == Some(origin_did))
        .filter(|method| method.get("id").and_then(Value::as_str) == Some(authentication_id))
        .count();
    if matching_keys != 1 {
        return Err(service_mismatch());
    }
    Ok(())
}

fn did_wba_domain(did: &str) -> Option<&str> {
    did.strip_prefix("did:wba:")
        .and_then(|value| value.split(':').next())
        .filter(|value| !value.is_empty())
}

fn normalize_ed25519_jwk_methods(document: &mut Value) {
    let Some(methods) = document
        .get_mut("verificationMethod")
        .and_then(Value::as_array_mut)
    else {
        return;
    };
    for method in methods {
        let Some(method) = method.as_object_mut() else {
            continue;
        };
        let jwk = method.get("publicKeyJwk").and_then(Value::as_object);
        let is_ed25519_jwk = method.get("type").and_then(Value::as_str) == Some("JsonWebKey2020")
            && jwk.and_then(|jwk| jwk.get("kty")).and_then(Value::as_str) == Some("OKP")
            && jwk.and_then(|jwk| jwk.get("crv")).and_then(Value::as_str) == Some("Ed25519");
        if is_ed25519_jwk {
            // The ANP proof verifier accepts Ed25519 JWK material under the
            // Ed25519 verification-method type but does not yet dispatch OKP
            // keys from JsonWebKey2020. This in-memory normalization preserves
            // the resolved key bytes and authentication relationship.
            method.insert(
                "type".to_owned(),
                Value::String("Ed25519VerificationKey2020".to_owned()),
            );
        }
    }
}

fn select_unique_service_did(document: &Value) -> crate::ImResult<String> {
    let candidates = document
        .get("service")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .filter(|service| service.get("type").and_then(Value::as_str) == Some("ANPMessageService"))
        .filter(|service| string_array_contains(service.get("profiles"), DIRECT_PROFILE))
        .filter(|service| {
            string_array_contains(
                service
                    .get("securityProfiles")
                    .or_else(|| service.get("security_profiles")),
                TRANSPORT_SECURITY,
            )
        })
        .collect::<Vec<_>>();
    if candidates.len() != 1 {
        return Err(service_mismatch());
    }
    candidates[0]
        .get("serviceDid")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(service_mismatch)
}

fn string_array_contains(value: Option<&Value>, expected: &str) -> bool {
    value
        .and_then(Value::as_array)
        .is_some_and(|values| values.iter().any(|value| value.as_str() == Some(expected)))
}

fn verify_proof_time_binding(
    envelope: &SystemNotificationEnvelope,
    created: i64,
    expires: i64,
) -> crate::ImResult<()> {
    let input = &envelope.auth.origin_proof.signature_input;
    if !input.contains(&format!(";created={created};"))
        || !input.contains(&format!(";expires={expires};"))
    {
        return Err(system_error(
            "system.notification.invalid_origin_proof",
            "system notification proof time binding is invalid",
        ));
    }
    Ok(())
}

fn validate_terminal_revision(revision: u64) -> crate::ImResult<()> {
    if (2..=4).contains(&revision) {
        Ok(())
    } else {
        Err(invalid_payload())
    }
}

fn decode_canonical_b64u(value: &str, length: usize) -> Result<Vec<u8>, ()> {
    if value.contains('=') {
        return Err(());
    }
    let decoded = URL_SAFE_NO_PAD.decode(value).map_err(|_| ())?;
    if decoded.len() != length || URL_SAFE_NO_PAD.encode(&decoded) != value {
        return Err(());
    }
    Ok(decoded)
}

fn valid_hash(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|value| decode_canonical_b64u(value, 32).is_ok())
}

fn valid_prefixed_id(value: &str, prefix: &str) -> bool {
    value
        .strip_prefix(prefix)
        .is_some_and(|suffix| decode_canonical_b64u(suffix, 16).is_ok())
}

fn parse_time(value: &str) -> crate::ImResult<DateTime<Utc>> {
    if !value.ends_with('Z') {
        return Err(super::wire::invalid(
            "system notification timestamp must use UTC Z form",
        ));
    }
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| super::wire::invalid("system notification timestamp is invalid"))
}

fn canonical_hash(value: &Value) -> crate::ImResult<String> {
    let canonical = serde_json_canonicalizer::to_vec(value).map_err(serialization)?;
    Ok(format!(
        "sha256:{}",
        URL_SAFE_NO_PAD.encode(Sha256::digest(canonical))
    ))
}

fn invalid_payload() -> crate::ImError {
    super::wire::invalid("system notification type-specific payload is invalid")
}

fn invalid_join_request() -> crate::ImError {
    system_error(
        "system.notification.invalid_join_request",
        "Join Request proof or binding is invalid",
    )
}

fn service_mismatch() -> crate::ImError {
    system_error(
        "system.notification.service_mismatch",
        "system notification origin is not anchored to the target Home Service",
    )
}

fn serialization(error: impl std::fmt::Display) -> crate::ImError {
    crate::ImError::Serialization {
        detail: error.to_string(),
    }
}

fn system_error(code: &str, message: &str) -> crate::ImError {
    crate::ImError::Service {
        status_code: None,
        code: Some(code.to_owned()),
        message: message.to_owned(),
        data: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_payload_accepts_independent_agent_origin_anchored_to_home_service() {
        let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../../../plan/20260718-awiki-multi-device-implementation/refactor/fixtures/system-notification-v1.json",
        );
        let fixture: Value = serde_json::from_slice(&std::fs::read(fixture_path).unwrap()).unwrap();
        assert_eq!(
            fixture["file_digest"]["sha256_b64u"],
            "ard6V0xyos_GlNuSNfjeLicFUd3jac-enpdgQmXcrX0"
        );
        let mut incoming = fixture["p3_vector"]["request"].clone();
        incoming["method"] = Value::String("direct.incoming".to_owned());
        incoming.as_object_mut().unwrap().remove("id");
        let origin_did = incoming["params"]["meta"]["sender_did"]
            .as_str()
            .unwrap()
            .to_owned();
        let origin_document = fixture["p3_vector"]["origin_did_document"].clone();
        let envelope = super::super::wire::parse_envelope(&incoming).unwrap();
        validate_shape_and_payload(
            "did:wba:example.com:agents:alice:e1_alice",
            &envelope,
            DateTime::parse_from_rfc3339("2026-07-23T02:00:01Z")
                .unwrap()
                .with_timezone(&Utc),
        )
        .unwrap();
        assert_ne!(envelope.meta.sender_did, "did:wba:example.com");
        validate_system_notification_origin_did(&envelope.meta.sender_did, "did:wba:example.com")
            .unwrap();
        validate_e1_origin_key_binding(&origin_did, &origin_document).unwrap();
        verify_origin_proof(&envelope, &origin_did, origin_document.clone()).unwrap();

        assert!(
            validate_system_notification_origin_did(&origin_did, "did:wba:other.example").is_err()
        );
        assert!(validate_system_notification_origin_did(
            &origin_did.replace(":agents:system-notification:", ":services:message:"),
            "did:wba:example.com"
        )
        .is_err());
        let mut mismatched_document = origin_document.clone();
        mismatched_document["id"] = Value::String(origin_did.replace("e1_", "e1_A"));
        assert!(validate_e1_origin_key_binding(&origin_did, &mismatched_document).is_err());

        let mut bad_proof_value = incoming;
        bad_proof_value["params"]["auth"]["origin_proof"]["signature"] =
            Value::String("sig1=:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA:".to_owned());
        let bad_envelope = super::super::wire::parse_envelope(&bad_proof_value).unwrap();
        assert!(verify_origin_proof(&bad_envelope, &origin_did, origin_document).is_err());
    }

    #[test]
    fn unknown_or_secret_join_payload_fields_fail_closed() {
        let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../../../plan/20260718-awiki-multi-device-implementation/refactor/fixtures/system-notification-v1.json",
        );
        let fixture: Value = serde_json::from_slice(&std::fs::read(fixture_path).unwrap()).unwrap();
        let mut incoming = fixture["p3_vector"]["request"].clone();
        incoming["method"] = Value::String("direct.incoming".to_owned());
        incoming["params"]["body"]["payload"]["payload"]["join_session_token"] =
            Value::String("must-not-pass".to_owned());
        assert!(super::super::wire::parse_envelope(&incoming).is_err());
    }

    #[test]
    fn device_targeting_extension_in_p3_meta_fails_closed() {
        let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../../../plan/20260718-awiki-multi-device-implementation/refactor/fixtures/system-notification-v1.json",
        );
        let fixture: Value = serde_json::from_slice(&std::fs::read(fixture_path).unwrap()).unwrap();
        let mut incoming = fixture["p3_vector"]["request"].clone();
        incoming["method"] = Value::String("direct.incoming".to_owned());
        incoming["params"]["meta"]["recipient_device_id"] =
            Value::String("dev-must-stay-out-of-p3".to_owned());

        assert!(super::super::wire::parse_envelope(&incoming).is_err());
    }

    #[test]
    fn service_selector_rejects_ambiguous_compatible_entry_even_if_only_one_has_service_did() {
        let document = json!({
            "service": [
                {
                    "type": "ANPMessageService",
                    "profiles": [DIRECT_PROFILE],
                    "securityProfiles": [TRANSPORT_SECURITY]
                },
                {
                    "type": "ANPMessageService",
                    "profiles": [DIRECT_PROFILE],
                    "securityProfiles": [TRANSPORT_SECURITY],
                    "serviceDid": "did:wba:example.com"
                }
            ]
        });

        assert!(select_unique_service_did(&document).is_err());
    }

    #[test]
    fn notification_timestamps_require_utc_z_form() {
        assert!(parse_time("2026-07-23T02:00:00Z").is_ok());
        assert!(parse_time("2026-07-23T02:00:00+00:00").is_err());
        assert!(parse_time("2026-07-23T10:00:00+08:00").is_err());
    }
}
