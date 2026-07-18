//! Restart-safe AWiki-local device Join crypto and state.
//!
//! Private signing, E2EE and pairing material is always sealed in SecretVault.
//! The adjacent state file contains only public protocol objects, digests and
//! opaque SecretVault references. SAS values are derived on demand and are
//! never persisted.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret as X25519StaticSecret};
use zeroize::{Zeroize, Zeroizing};

use crate::identity::{
    DeviceJoinAdminPrepareRequest, DeviceJoinAdminPrepareResult, DeviceJoinAdminVerifyRequest,
    DeviceJoinAdminVerifyResult, DeviceJoinChallenge, DeviceJoinChallengeResponse,
    DeviceJoinLocalPhase, DeviceJoinNewDeviceRespondRequest, DeviceJoinNewDeviceRespondResult,
    DeviceJoinRequest, DeviceJoinSessionSummary, DeviceJoinSide, DeviceJoinStartRequest,
    DeviceJoinStartResult, DeviceProof, EncryptedJoinChallenge, DEVICE_JOIN_CHALLENGE_ALGORITHM,
    DEVICE_JOIN_MAX_CHALLENGE_TTL_SECONDS, DEVICE_JOIN_MAX_TTL_SECONDS, DEVICE_JOIN_REQUEST_TYPE,
    DEVICE_JOIN_VNEXT_PROFILES, DEVICE_PROOF_TYPE,
};
use crate::internal::platform_secret::SecretBytes;
use crate::internal::secret_vault::record::{SecretKind, SecretMetadata, SecretRef};
use crate::internal::secret_vault::{SealSecretRequest, SecretAccessPolicy, SecretVault};

const JOIN_STATE_SCHEMA_VERSION: u32 = 1;
const JOIN_STATE_DIR: &str = ".device-join";
const JOIN_CHALLENGE_LEN: usize = 32;
const JOIN_NONCE_LEN: usize = 12;
const JOIN_RANDOM_ID_LEN: usize = 16;
const JOIN_PROOF_NONCE_LEN: usize = 24;
const CHALLENGE_KDF_INFO: &[u8] = b"awiki-device-join-challenge-v1";
const SAS_KDF_INFO: &[u8] = b"awiki-device-join-sas-v1";
const JOIN_REQUEST_PURPOSE: &str = "awiki.device.join.v1";
const JOIN_CHALLENGE_PURPOSE: &str = "awiki.device.join.challenge.v1";
const JOIN_RESPONSE_PURPOSE: &str = "awiki.device.join.challenge-response.v1";
const JOIN_CHALLENGE_METHOD: &str = "device_join_challenge";
const JOIN_RESPONSE_METHOD: &str = "device_join_challenge_response";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct InternalCheckpoint {
    document_version: u64,
    document_hash: String,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
struct StoredJoinSession {
    schema_version: u32,
    side: DeviceJoinSide,
    phase: DeviceJoinLocalPhase,
    create_operation_id: Option<String>,
    create_input_hash: Option<String>,
    transition_operation_id: Option<String>,
    transition_input_hash: Option<String>,
    verification_operation_id: Option<String>,
    verification_input_hash: Option<String>,
    join_request: DeviceJoinRequest,
    join_request_hash: String,
    checkpoint: Option<InternalCheckpoint>,
    challenge: Option<DeviceJoinChallenge>,
    challenge_hash: Option<String>,
    response: Option<DeviceJoinChallengeResponse>,
    signing_private_ref: Option<SecretRef>,
    e2ee_private_ref: Option<SecretRef>,
    pairing_private_ref: SecretRef,
    admin_identity: Option<crate::identity::IdentitySelector>,
}

impl std::fmt::Debug for StoredJoinSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoredJoinSession")
            .field("schema_version", &self.schema_version)
            .field("side", &self.side)
            .field("phase", &self.phase)
            .field("join_session_id", &self.join_request.join_session_id)
            .field("did", &self.join_request.did)
            .field("device_id", &self.join_request.device_id)
            .field("join_request_hash", &self.join_request_hash)
            .field("checkpoint", &self.checkpoint)
            .field(
                "challenge_id",
                &self.challenge.as_ref().map(|value| &value.challenge_id),
            )
            .field("has_response", &self.response.is_some())
            .field("secret_refs", &"<redacted-secret-refs>")
            .finish()
    }
}

pub(crate) fn start(
    core: &crate::core::ImCore,
    request: DeviceJoinStartRequest,
) -> crate::ImResult<DeviceJoinStartResult> {
    let _guard = lock_join_state(core)?;
    validate_operation_id(&request.operation_id)?;
    validate_join_ttl(request.ttl_seconds)?;
    let input_hash = canonical_hash(&json!({
        "did": request.did.as_str(),
        "ttl_seconds": request.ttl_seconds,
    }))?;
    let store = JoinStateStore::new(core);
    if let Some(mut stored) = store.find_new_device_by_create_operation(&request.operation_id)? {
        normalize_expiry(core, &store, &mut stored)?;
        if stored.create_input_hash.as_deref() != Some(input_hash.as_str()) {
            return Err(idempotency_conflict("start"));
        }
        ensure_not_expired(&stored)?;
        return Ok(DeviceJoinStartResult {
            session: summary(&stored)?,
            join_request: stored.join_request,
        });
    }

    let vault = required_vault(core)?;
    let now = OffsetDateTime::now_utc();
    let expires_at = now + Duration::seconds(request.ttl_seconds as i64);
    let join_session_id = random_id("join", JOIN_RANDOM_ID_LEN)?;
    let protocol_device_id = crate::ids::ProtocolDeviceId::generate()?;
    let signing_key_id = format!(
        "{}#{}-sign",
        request.did.as_str(),
        protocol_device_id.as_str()
    );
    let e2ee_key_id = format!(
        "{}#{}-e2ee",
        request.did.as_str(),
        protocol_device_id.as_str()
    );

    let signing_private = anp::PrivateKeyMaterial::Ed25519(ed25519_dalek::SigningKey::generate(
        &mut rand::rngs::OsRng,
    ));
    let e2ee_private =
        anp::PrivateKeyMaterial::X25519(X25519StaticSecret::random_from_rng(rand::rngs::OsRng));
    let pairing_private =
        anp::PrivateKeyMaterial::X25519(X25519StaticSecret::random_from_rng(rand::rngs::OsRng));
    let signing_method = verification_method(
        request.did.as_str(),
        &signing_key_id,
        "Multikey",
        &signing_private.public_key(),
    )?;
    let e2ee_method = verification_method(
        request.did.as_str(),
        &e2ee_key_id,
        "X25519KeyAgreementKey2019",
        &e2ee_private.public_key(),
    )?;
    let pairing_public_key = x25519_public_b64u(&pairing_private.public_key())?;
    let mut join_request = DeviceJoinRequest {
        request_type: DEVICE_JOIN_REQUEST_TYPE.to_owned(),
        did: request.did.as_str().to_owned(),
        join_session_id: join_session_id.clone(),
        device_id: protocol_device_id.as_str().to_owned(),
        signing_public_key: signing_method,
        e2ee_public_key: e2ee_method,
        pairing_public_key,
        profiles: DEVICE_JOIN_VNEXT_PROFILES
            .iter()
            .map(|value| (*value).to_owned())
            .collect(),
        requested_role: "member".to_owned(),
        issued_at: format_time(now)?,
        expires_at: format_time(expires_at)?,
        signature: String::new(),
    };
    join_request.signature = sign_join_request(&join_request, &signing_private)?;
    validate_join_request(&join_request, now)?;
    let join_request_hash =
        canonical_hash(&serde_json::to_value(&join_request).map_err(|err| {
            crate::ImError::Serialization {
                detail: err.to_string(),
            }
        })?)?;

    let signing_pem = Zeroizing::new(signing_private.to_pem());
    let e2ee_pem = Zeroizing::new(e2ee_private.to_pem());
    let pairing_pem = Zeroizing::new(pairing_private.to_pem());
    let mut sealed = Vec::new();
    let signing_ref = seal_join_secret(
        core,
        &*vault,
        &request.did,
        SecretKind::IdentityDeviceSigningPrivate,
        &signing_key_id,
        signing_pem.as_bytes(),
    )?;
    sealed.push(signing_ref.clone());
    let e2ee_ref = match seal_join_secret(
        core,
        &*vault,
        &request.did,
        SecretKind::IdentityE2eeAgreementPrivate,
        &e2ee_key_id,
        e2ee_pem.as_bytes(),
    ) {
        Ok(value) => value,
        Err(err) => {
            cleanup_secrets(&*vault, &sealed);
            return Err(err);
        }
    };
    sealed.push(e2ee_ref.clone());
    let pairing_ref = match seal_join_secret(
        core,
        &*vault,
        &request.did,
        SecretKind::IdentityJoinPairingPrivate,
        &format!("{join_session_id}:new-pairing"),
        pairing_pem.as_bytes(),
    ) {
        Ok(value) => value,
        Err(err) => {
            cleanup_secrets(&*vault, &sealed);
            return Err(err);
        }
    };
    sealed.push(pairing_ref.clone());

    let stored = StoredJoinSession {
        schema_version: JOIN_STATE_SCHEMA_VERSION,
        side: DeviceJoinSide::NewDevice,
        phase: DeviceJoinLocalPhase::Pending,
        create_operation_id: Some(request.operation_id),
        create_input_hash: Some(input_hash),
        transition_operation_id: None,
        transition_input_hash: None,
        verification_operation_id: None,
        verification_input_hash: None,
        join_request: join_request.clone(),
        join_request_hash,
        checkpoint: None,
        challenge: None,
        challenge_hash: None,
        response: None,
        signing_private_ref: Some(signing_ref),
        e2ee_private_ref: Some(e2ee_ref),
        pairing_private_ref: pairing_ref,
        admin_identity: None,
    };
    if let Err(err) = store.save(&stored) {
        cleanup_secrets(&*vault, &sealed);
        return Err(err);
    }
    Ok(DeviceJoinStartResult {
        session: summary(&stored)?,
        join_request,
    })
}

pub(crate) fn prepare_admin_challenge(
    core: &crate::core::ImCore,
    request: DeviceJoinAdminPrepareRequest,
) -> crate::ImResult<DeviceJoinAdminPrepareResult> {
    let _guard = lock_join_state(core)?;
    validate_operation_id(&request.operation_id)?;
    validate_challenge_ttl(request.challenge_ttl_seconds)?;
    let now = OffsetDateTime::now_utc();
    validate_join_request(&request.join_request, now)?;
    let checkpoint = validate_checkpoint(request.document_version, &request.document_hash)?;
    let join_request_hash =
        canonical_hash(&serde_json::to_value(&request.join_request).map_err(|err| {
            crate::ImError::Serialization {
                detail: err.to_string(),
            }
        })?)?;
    let input_hash = canonical_hash(&json!({
        "admin_identity": request.admin_identity,
        "operation_id": request.operation_id,
        "join_request": request.join_request,
        "challenge_ttl_seconds": request.challenge_ttl_seconds,
        "document_version": request.document_version,
        "document_hash": request.document_hash,
    }))?;
    let store = JoinStateStore::new(core);
    if let Some(mut stored) =
        store.load(&request.join_request.join_session_id, DeviceJoinSide::Admin)?
    {
        normalize_expiry(core, &store, &mut stored)?;
        if stored.transition_operation_id.as_deref() != Some(request.operation_id.as_str())
            || stored.transition_input_hash.as_deref() != Some(input_hash.as_str())
        {
            return Err(idempotency_conflict("prepare_admin_challenge"));
        }
        ensure_not_expired(&stored)?;
        let sas = derive_stored_sas(core, &stored)?;
        return Ok(DeviceJoinAdminPrepareResult {
            session: summary(&stored)?,
            challenge: stored
                .challenge
                .ok_or_else(|| invalid_state("challenge missing"))?,
            sas,
        });
    }

    let client = core.client(request.admin_identity.clone())?;
    if client.did().as_str() != request.join_request.did {
        return Err(crate::ImError::PermissionDenied);
    }
    let admin_summary = core
        .identities()
        .device_summary(request.admin_identity.clone())?;
    if admin_summary.mode != crate::identity::IdentityDeviceMode::VNext
        || admin_summary.role != Some(crate::identity::IdentityDeviceRole::Admin)
        || admin_summary.readiness != crate::identity::IdentityDeviceReadiness::AdminReady
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let admin_device_id = admin_summary
        .protocol_device_id
        .as_ref()
        .ok_or_else(|| invalid_state("ready admin is missing protocol device id"))?
        .as_str()
        .to_owned();
    let admin_signing_key_id = admin_summary
        .signing_key_id
        .as_deref()
        .ok_or_else(|| invalid_state("ready admin is missing signing key id"))?
        .to_owned();
    let admin_document = client.runtime().key_provider.did_document()?;
    validate_current_document(
        &admin_document,
        &request.join_request.did,
        &request.document_hash,
    )?;

    let join_expires = parse_time("join_request.expires_at", &request.join_request.expires_at)?;
    let proposed_expiry = now + Duration::seconds(request.challenge_ttl_seconds as i64);
    let challenge_expires = std::cmp::min(join_expires, proposed_expiry);
    if challenge_expires <= now {
        return Err(crate::ImError::SessionExpired);
    }
    let challenge_id = random_id("challenge", JOIN_RANDOM_ID_LEN)?;
    let mut challenge_plaintext = Zeroizing::new([0_u8; JOIN_CHALLENGE_LEN]);
    rand::rngs::OsRng
        .try_fill_bytes(challenge_plaintext.as_mut())
        .map_err(|_| crate::ImError::Internal {
            message: "secure Join challenge generation failed".to_owned(),
        })?;
    let challenge_hash = hash_bytes(challenge_plaintext.as_ref());
    let admin_pairing_private =
        anp::PrivateKeyMaterial::X25519(X25519StaticSecret::random_from_rng(rand::rngs::OsRng));
    let admin_pairing_public_key = x25519_public_b64u(&admin_pairing_private.public_key())?;
    let challenge_expires_at = format_time(challenge_expires)?;
    let ciphertext = encrypt_challenge(
        &admin_pairing_private,
        &request.join_request,
        &join_request_hash,
        &challenge_id,
        &admin_device_id,
        &admin_pairing_public_key,
        &challenge_expires_at,
        challenge_plaintext.as_ref(),
    )?;
    let created_at = format_time(now)?;
    let params = challenge_params_value(
        &request.operation_id,
        &request.join_request.join_session_id,
        &challenge_id,
        &admin_device_id,
        &admin_pairing_public_key,
        &ciphertext,
        &challenge_expires_at,
    );
    let signing_pem = Zeroizing::new(
        client
            .runtime()
            .key_provider
            .device_request_signing_private_pem()?,
    );
    let signing_private = private_key_from_pem(
        signing_pem.as_bytes(),
        SecretKind::IdentityDeviceSigningPrivate,
    )?;
    let proof = sign_device_proof(
        &signing_private,
        &admin_signing_key_id,
        JOIN_CHALLENGE_PURPOSE,
        JOIN_CHALLENGE_METHOD,
        &params,
        &created_at,
        &challenge_expires_at,
    )?;
    let challenge = DeviceJoinChallenge {
        operation_id: request.operation_id.clone(),
        join_session_id: request.join_request.join_session_id.clone(),
        challenge_id,
        admin_device_id,
        admin_pairing_public_key,
        ciphertext,
        challenge_expires_at,
        authorizing_device_proof: proof,
    };

    let vault = required_vault(core)?;
    let pairing_pem = Zeroizing::new(admin_pairing_private.to_pem());
    let pairing_ref = seal_join_secret_with_identity(
        core,
        &*vault,
        &crate::ids::Did::parse(&request.join_request.did)?,
        Some(client.current_identity().id.as_str()),
        SecretKind::IdentityJoinPairingPrivate,
        &format!("{}:admin-pairing", request.join_request.join_session_id),
        pairing_pem.as_bytes(),
    )?;
    let stored = StoredJoinSession {
        schema_version: JOIN_STATE_SCHEMA_VERSION,
        side: DeviceJoinSide::Admin,
        phase: DeviceJoinLocalPhase::ChallengePrepared,
        create_operation_id: None,
        create_input_hash: None,
        transition_operation_id: Some(request.operation_id),
        transition_input_hash: Some(input_hash),
        verification_operation_id: None,
        verification_input_hash: None,
        join_request: request.join_request,
        join_request_hash,
        checkpoint: Some(checkpoint),
        challenge: Some(challenge.clone()),
        challenge_hash: Some(challenge_hash),
        response: None,
        signing_private_ref: None,
        e2ee_private_ref: None,
        pairing_private_ref: pairing_ref.clone(),
        admin_identity: Some(request.admin_identity),
    };
    if let Err(err) = store.save(&stored) {
        let _ = vault.delete(&pairing_ref);
        return Err(err);
    }
    Ok(DeviceJoinAdminPrepareResult {
        session: summary(&stored)?,
        challenge,
        sas: derive_stored_sas(core, &stored)?,
    })
}

pub(crate) fn respond_as_new_device(
    core: &crate::core::ImCore,
    request: DeviceJoinNewDeviceRespondRequest,
) -> crate::ImResult<DeviceJoinNewDeviceRespondResult> {
    let _guard = lock_join_state(core)?;
    validate_operation_id(&request.operation_id)?;
    let store = JoinStateStore::new(core);
    let mut stored = store
        .load(
            &request.challenge.join_session_id,
            DeviceJoinSide::NewDevice,
        )?
        .ok_or_else(|| crate::ImError::IdentityNotFound {
            selector: request.challenge.join_session_id.clone(),
        })?;
    normalize_expiry(core, &store, &mut stored)?;
    ensure_not_expired(&stored)?;
    let checkpoint = validate_checkpoint(request.document_version, &request.document_hash)?;
    let input_hash = canonical_hash(&json!({
        "operation_id": request.operation_id,
        "challenge": request.challenge,
        "admin_did_document": request.admin_did_document,
        "document_version": request.document_version,
        "document_hash": request.document_hash,
    }))?;
    if stored.phase == DeviceJoinLocalPhase::ResponsePrepared {
        if stored.transition_operation_id.as_deref() != Some(request.operation_id.as_str())
            || stored.transition_input_hash.as_deref() != Some(input_hash.as_str())
        {
            return Err(idempotency_conflict("respond_as_new_device"));
        }
        let sas = derive_stored_sas(core, &stored)?;
        return Ok(DeviceJoinNewDeviceRespondResult {
            session: summary(&stored)?,
            response: stored
                .response
                .ok_or_else(|| invalid_state("response missing"))?,
            sas,
        });
    }
    if stored.phase != DeviceJoinLocalPhase::Pending {
        return Err(invalid_state("new-device Join is not pending"));
    }
    if request.challenge.join_session_id != stored.join_request.join_session_id {
        return Err(crate::ImError::PermissionDenied);
    }
    let challenge_expires = parse_time(
        "challenge.challenge_expires_at",
        &request.challenge.challenge_expires_at,
    )?;
    if challenge_expires <= OffsetDateTime::now_utc() {
        return Err(crate::ImError::SessionExpired);
    }
    validate_current_document(
        &request.admin_did_document,
        &stored.join_request.did,
        &request.document_hash,
    )?;
    let admin_signing_method = admin_signing_method(
        &request.admin_did_document,
        &request.challenge.admin_device_id,
        &request.challenge.authorizing_device_proof.key_id,
    )?;
    let challenge_params = challenge_params_value(
        &request.challenge.operation_id,
        &request.challenge.join_session_id,
        &request.challenge.challenge_id,
        &request.challenge.admin_device_id,
        &request.challenge.admin_pairing_public_key,
        &request.challenge.ciphertext,
        &request.challenge.challenge_expires_at,
    );
    verify_device_proof(
        &request.challenge.authorizing_device_proof,
        JOIN_CHALLENGE_PURPOSE,
        JOIN_CHALLENGE_METHOD,
        &challenge_params,
        &admin_signing_method,
        OffsetDateTime::now_utc(),
    )?;

    let vault = required_vault(core)?;
    let e2ee_ref = stored
        .e2ee_private_ref
        .as_ref()
        .ok_or_else(|| invalid_state("new-device E2EE key reference missing"))?;
    let e2ee_private =
        open_private_key(&*vault, e2ee_ref, SecretKind::IdentityE2eeAgreementPrivate)?;
    let challenge_plaintext = decrypt_challenge(
        &e2ee_private,
        &stored.join_request,
        &stored.join_request_hash,
        &request.challenge,
    )?;
    let challenge_hash = hash_bytes(challenge_plaintext.expose_secret());
    let transcript = join_transcript(
        &stored.join_request,
        &stored.join_request_hash,
        &request.challenge,
        &challenge_hash,
        &checkpoint,
    )?;
    let pairing_transcript_hash = canonical_hash(&transcript)?;
    let created_at = format_time(OffsetDateTime::now_utc())?;
    let signing_ref = stored
        .signing_private_ref
        .as_ref()
        .ok_or_else(|| invalid_state("new-device signing key reference missing"))?;
    let signing_private = open_private_key(
        &*vault,
        signing_ref,
        SecretKind::IdentityDeviceSigningPrivate,
    )?;
    let signing_key_id = method_id(
        &stored.join_request.signing_public_key,
        "signing_public_key",
    )?;
    let response_params = response_params_value(
        &request.operation_id,
        &stored.join_request.join_session_id,
        &request.challenge.challenge_id,
        &challenge_hash,
        &stored.join_request_hash,
        &pairing_transcript_hash,
    );
    let proof = sign_device_proof(
        &signing_private,
        signing_key_id,
        JOIN_RESPONSE_PURPOSE,
        JOIN_RESPONSE_METHOD,
        &response_params,
        &created_at,
        &request.challenge.challenge_expires_at,
    )?;
    let response = DeviceJoinChallengeResponse {
        operation_id: request.operation_id.clone(),
        join_session_id: stored.join_request.join_session_id.clone(),
        challenge_id: request.challenge.challenge_id.clone(),
        challenge_hash: challenge_hash.clone(),
        join_request_hash: stored.join_request_hash.clone(),
        pairing_transcript_hash,
        new_device_proof: proof,
    };
    stored.phase = DeviceJoinLocalPhase::ResponsePrepared;
    stored.transition_operation_id = Some(request.operation_id);
    stored.transition_input_hash = Some(input_hash);
    stored.checkpoint = Some(checkpoint);
    stored.challenge = Some(request.challenge);
    stored.challenge_hash = Some(challenge_hash);
    stored.response = Some(response.clone());
    store.save(&stored)?;
    Ok(DeviceJoinNewDeviceRespondResult {
        session: summary(&stored)?,
        response,
        sas: derive_sas_for_state(core, &stored, &transcript)?,
    })
}

pub(crate) fn verify_response_as_admin(
    core: &crate::core::ImCore,
    request: DeviceJoinAdminVerifyRequest,
) -> crate::ImResult<DeviceJoinAdminVerifyResult> {
    let _guard = lock_join_state(core)?;
    validate_operation_id(&request.operation_id)?;
    let store = JoinStateStore::new(core);
    let mut stored = store
        .load(&request.join_session_id, DeviceJoinSide::Admin)?
        .ok_or_else(|| crate::ImError::IdentityNotFound {
            selector: request.join_session_id.clone(),
        })?;
    normalize_expiry(core, &store, &mut stored)?;
    ensure_not_expired(&stored)?;
    let input_hash = canonical_hash(&json!({
        "operation_id": request.operation_id,
        "join_session_id": request.join_session_id,
        "response": request.response,
    }))?;
    if stored.phase == DeviceJoinLocalPhase::ResponseVerified {
        if stored.verification_operation_id.as_deref() != Some(request.operation_id.as_str())
            || stored.verification_input_hash.as_deref() != Some(input_hash.as_str())
        {
            return Err(idempotency_conflict("verify_response_as_admin"));
        }
        return verified_result(core, &stored);
    }
    if stored.phase != DeviceJoinLocalPhase::ChallengePrepared {
        return Err(invalid_state("admin Join is not waiting for a response"));
    }
    let challenge = stored
        .challenge
        .as_ref()
        .ok_or_else(|| invalid_state("challenge missing"))?;
    let checkpoint = stored
        .checkpoint
        .as_ref()
        .ok_or_else(|| invalid_state("checkpoint missing"))?;
    let expected_challenge_hash = stored
        .challenge_hash
        .as_deref()
        .ok_or_else(|| invalid_state("challenge hash missing"))?;
    if request.response.join_session_id != stored.join_request.join_session_id
        || request.response.challenge_id != challenge.challenge_id
        || request.response.challenge_hash != expected_challenge_hash
        || request.response.join_request_hash != stored.join_request_hash
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let transcript = join_transcript(
        &stored.join_request,
        &stored.join_request_hash,
        challenge,
        expected_challenge_hash,
        checkpoint,
    )?;
    let expected_transcript_hash = canonical_hash(&transcript)?;
    if request.response.pairing_transcript_hash != expected_transcript_hash {
        return Err(crate::ImError::PermissionDenied);
    }
    let response_params = response_params_value(
        &request.response.operation_id,
        &request.response.join_session_id,
        &request.response.challenge_id,
        &request.response.challenge_hash,
        &request.response.join_request_hash,
        &request.response.pairing_transcript_hash,
    );
    verify_device_proof(
        &request.response.new_device_proof,
        JOIN_RESPONSE_PURPOSE,
        JOIN_RESPONSE_METHOD,
        &response_params,
        &stored.join_request.signing_public_key,
        OffsetDateTime::now_utc(),
    )?;
    stored.phase = DeviceJoinLocalPhase::ResponseVerified;
    stored.verification_operation_id = Some(request.operation_id);
    stored.verification_input_hash = Some(input_hash);
    stored.response = Some(request.response);
    store.save(&stored)?;
    verified_result(core, &stored)
}

pub(crate) fn session(
    core: &crate::core::ImCore,
    join_session_id: &str,
    side: DeviceJoinSide,
) -> crate::ImResult<DeviceJoinSessionSummary> {
    let _guard = lock_join_state(core)?;
    let join_session_id = required("join_session_id", join_session_id)?;
    let store = JoinStateStore::new(core);
    let mut stored =
        store
            .load(&join_session_id, side)?
            .ok_or_else(|| crate::ImError::IdentityNotFound {
                selector: join_session_id,
            })?;
    normalize_expiry(core, &store, &mut stored)?;
    summary(&stored)
}

fn verified_result(
    core: &crate::core::ImCore,
    stored: &StoredJoinSession,
) -> crate::ImResult<DeviceJoinAdminVerifyResult> {
    let response = stored
        .response
        .as_ref()
        .ok_or_else(|| invalid_state("response missing"))?;
    Ok(DeviceJoinAdminVerifyResult {
        session: summary(stored)?,
        join_request_hash: stored.join_request_hash.clone(),
        pairing_transcript_hash: response.pairing_transcript_hash.clone(),
        sas: derive_stored_sas(core, stored)?,
    })
}

fn lock_join_state(core: &crate::core::ImCore) -> crate::ImResult<std::sync::MutexGuard<'_, ()>> {
    core.inner()
        .device_join_lock
        .lock()
        .map_err(|_| crate::ImError::LocalStateUnavailable {
            detail: "device Join state lock poisoned".to_owned(),
        })
}

fn required_vault(
    core: &crate::core::ImCore,
) -> crate::ImResult<std::sync::Arc<dyn SecretVault + Send + Sync>> {
    core.inner()
        .identity_vault()
        .map(|context| context.vault())
        .ok_or(crate::ImError::IdentityVault {
            failure: crate::IdentityVaultFailure::Unavailable,
        })
}

fn seal_join_secret(
    core: &crate::core::ImCore,
    vault: &dyn SecretVault,
    did: &crate::ids::Did,
    kind: SecretKind,
    key_id: &str,
    plaintext: &[u8],
) -> crate::ImResult<SecretRef> {
    seal_join_secret_with_identity(core, vault, did, None, kind, key_id, plaintext)
}

fn seal_join_secret_with_identity(
    core: &crate::core::ImCore,
    vault: &dyn SecretVault,
    did: &crate::ids::Did,
    identity_id: Option<&str>,
    kind: SecretKind,
    key_id: &str,
    plaintext: &[u8],
) -> crate::ImResult<SecretRef> {
    let context = core
        .inner()
        .identity_vault()
        .ok_or(crate::ImError::IdentityVault {
            failure: crate::IdentityVaultFailure::Unavailable,
        })?;
    vault.seal(SealSecretRequest {
        metadata: SecretMetadata {
            workspace_id: context.workspace_id().to_owned(),
            device_id: context.vault_context_device_id().as_str().to_owned(),
            identity_id: identity_id.map(ToOwned::to_owned),
            did: Some(did.as_str().to_owned()),
            kind,
            key_id: key_id.to_owned(),
            key_version: 1,
            policy: SecretAccessPolicy::no_prompt_local_secret(),
        },
        plaintext: SecretBytes::from_vec(plaintext.to_vec()),
    })
}

fn cleanup_secrets(vault: &dyn SecretVault, refs: &[SecretRef]) {
    for secret_ref in refs {
        let _ = vault.delete(secret_ref);
    }
}

fn open_private_key(
    vault: &dyn SecretVault,
    secret_ref: &SecretRef,
    expected_kind: SecretKind,
) -> crate::ImResult<anp::PrivateKeyMaterial> {
    if secret_ref.kind != expected_kind {
        return Err(crate::ImError::PermissionDenied);
    }
    let secret = vault.open(secret_ref)?;
    private_key_from_pem(secret.expose_secret(), expected_kind)
}

fn private_key_from_pem(
    pem: &[u8],
    expected_kind: SecretKind,
) -> crate::ImResult<anp::PrivateKeyMaterial> {
    let pem = std::str::from_utf8(pem).map_err(|_| crate::ImError::Serialization {
        detail: "Join private key PEM is not UTF-8".to_owned(),
    })?;
    let key = anp::PrivateKeyMaterial::from_pem(pem).map_err(|_| {
        crate::ImError::CredentialFileUnreadable {
            path_kind: "device_join_private_key".to_owned(),
            detail: "invalid private key material".to_owned(),
        }
    })?;
    let valid = match (&expected_kind, &key) {
        (SecretKind::IdentityDeviceSigningPrivate, anp::PrivateKeyMaterial::Ed25519(_)) => true,
        (
            SecretKind::IdentityE2eeAgreementPrivate | SecretKind::IdentityJoinPairingPrivate,
            anp::PrivateKeyMaterial::X25519(_),
        ) => true,
        _ => false,
    };
    if !valid {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(key)
}

fn sign_join_request(
    request: &DeviceJoinRequest,
    private_key: &anp::PrivateKeyMaterial,
) -> crate::ImResult<String> {
    let content = unsigned_join_request_bytes(request)?;
    sign_bytes(private_key, &content)
}

fn validate_join_request(request: &DeviceJoinRequest, now: OffsetDateTime) -> crate::ImResult<()> {
    if request.request_type != DEVICE_JOIN_REQUEST_TYPE {
        return Err(crate::ImError::invalid_input(
            Some("join_request.type".to_owned()),
            "unsupported Join Request type",
        ));
    }
    let did = crate::ids::Did::parse(&request.did)?;
    crate::ids::ProtocolDeviceId::parse(&request.device_id)?;
    required("join_request.join_session_id", &request.join_session_id)?;
    if request.requested_role != "member" {
        return Err(crate::ImError::invalid_input(
            Some("join_request.requested_role".to_owned()),
            "new devices must request member role",
        ));
    }
    let expected_profiles = DEVICE_JOIN_VNEXT_PROFILES
        .iter()
        .map(|value| (*value).to_owned())
        .collect::<Vec<_>>();
    if request.profiles != expected_profiles {
        return Err(crate::ImError::invalid_input(
            Some("join_request.profiles".to_owned()),
            "Join Request must use the complete AWiki vNext device Profile closure",
        ));
    }
    let issued = parse_time("join_request.issued_at", &request.issued_at)?;
    let expires = parse_time("join_request.expires_at", &request.expires_at)?;
    if expires <= issued || (expires - issued).whole_seconds() > DEVICE_JOIN_MAX_TTL_SECONDS as i64
    {
        return Err(crate::ImError::invalid_input(
            Some("join_request.expires_at".to_owned()),
            "Join Request lifetime is invalid",
        ));
    }
    if expires <= now {
        return Err(crate::ImError::SessionExpired);
    }
    if issued > now + Duration::seconds(30) {
        return Err(crate::ImError::invalid_input(
            Some("join_request.issued_at".to_owned()),
            "Join Request issued_at is in the future",
        ));
    }
    validate_method_binding(
        &request.signing_public_key,
        &did,
        &request.device_id,
        "signing_public_key",
        true,
    )?;
    validate_method_binding(
        &request.e2ee_public_key,
        &did,
        &request.device_id,
        "e2ee_public_key",
        false,
    )?;
    if method_id(&request.signing_public_key, "signing_public_key")?
        == method_id(&request.e2ee_public_key, "e2ee_public_key")?
    {
        return Err(crate::ImError::invalid_input(
            Some("join_request".to_owned()),
            "device signing and E2EE key ids must be distinct",
        ));
    }
    decode_x25519_b64u(
        "join_request.pairing_public_key",
        &request.pairing_public_key,
    )?;
    let method = anp::authentication::create_verification_method(&request.signing_public_key)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    method
        .verify_signature(&unsigned_join_request_bytes(request)?, &request.signature)
        .map_err(|_| crate::ImError::PermissionDenied)
}

fn validate_method_binding(
    method: &Value,
    did: &crate::ids::Did,
    device_id: &str,
    field: &str,
    signing: bool,
) -> crate::ImResult<()> {
    let object = method.as_object().ok_or_else(|| {
        crate::ImError::invalid_input(Some(format!("join_request.{field}")), "must be an object")
    })?;
    if object
        .keys()
        .any(|key| key.to_ascii_lowercase().contains("private"))
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let id = method_id(method, field)?;
    let expected_suffix = if signing { "-sign" } else { "-e2ee" };
    if id != format!("{}#{}{}", did.as_str(), device_id, expected_suffix)
        || object.get("controller").and_then(Value::as_str) != Some(did.as_str())
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let material = anp::authentication::extract_public_key(method)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    if signing && !matches!(material, anp::PublicKeyMaterial::Ed25519(_)) {
        return Err(crate::ImError::PermissionDenied);
    }
    if !signing && !matches!(material, anp::PublicKeyMaterial::X25519(_)) {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

fn unsigned_join_request_bytes(request: &DeviceJoinRequest) -> crate::ImResult<Vec<u8>> {
    let mut value = serde_json::to_value(request).map_err(|err| crate::ImError::Serialization {
        detail: err.to_string(),
    })?;
    value
        .as_object_mut()
        .ok_or_else(|| invalid_state("Join Request serialization is not an object"))?
        .remove("signature");
    canonical_bytes(&value)
}

fn challenge_params_value(
    operation_id: &str,
    join_session_id: &str,
    challenge_id: &str,
    admin_device_id: &str,
    admin_pairing_public_key: &str,
    ciphertext: &EncryptedJoinChallenge,
    challenge_expires_at: &str,
) -> Value {
    json!({
        "operation_id": operation_id,
        "join_session_id": join_session_id,
        "challenge_id": challenge_id,
        "admin_device_id": admin_device_id,
        "admin_pairing_public_key": admin_pairing_public_key,
        "ciphertext": ciphertext,
        "challenge_expires_at": challenge_expires_at,
    })
}

fn response_params_value(
    operation_id: &str,
    join_session_id: &str,
    challenge_id: &str,
    challenge_hash: &str,
    join_request_hash: &str,
    pairing_transcript_hash: &str,
) -> Value {
    json!({
        "operation_id": operation_id,
        "join_session_id": join_session_id,
        "challenge_id": challenge_id,
        "challenge_hash": challenge_hash,
        "join_request_hash": join_request_hash,
        "pairing_transcript_hash": pairing_transcript_hash,
    })
}

fn device_proof_bytes(
    proof: &DeviceProof,
    purpose: &str,
    method: &str,
    params: &Value,
) -> crate::ImResult<Vec<u8>> {
    canonical_bytes(&json!({
        "type": proof.proof_type,
        "purpose": purpose,
        "method": method,
        "key_id": proof.key_id,
        "created_at": proof.created_at,
        "expires_at": proof.expires_at,
        "nonce": proof.nonce,
        "params": params,
    }))
}

fn sign_device_proof(
    private_key: &anp::PrivateKeyMaterial,
    key_id: &str,
    purpose: &str,
    method: &str,
    params: &Value,
    created_at: &str,
    expires_at: &str,
) -> crate::ImResult<DeviceProof> {
    let mut proof = DeviceProof {
        proof_type: DEVICE_PROOF_TYPE.to_owned(),
        key_id: key_id.to_owned(),
        created_at: created_at.to_owned(),
        expires_at: expires_at.to_owned(),
        nonce: random_b64u(JOIN_PROOF_NONCE_LEN)?,
        signature: String::new(),
    };
    proof.signature = sign_bytes(
        private_key,
        &device_proof_bytes(&proof, purpose, method, params)?,
    )?;
    Ok(proof)
}

fn verify_device_proof(
    proof: &DeviceProof,
    purpose: &str,
    method: &str,
    params: &Value,
    public_method: &Value,
    now: OffsetDateTime,
) -> crate::ImResult<()> {
    if proof.proof_type != DEVICE_PROOF_TYPE
        || method_id(public_method, "proof.public_method")? != proof.key_id
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let created = parse_time("proof.created_at", &proof.created_at)?;
    let expires = parse_time("proof.expires_at", &proof.expires_at)?;
    if expires <= created
        || expires <= now
        || created > now + Duration::seconds(30)
        || (expires - created).whole_seconds() > DEVICE_JOIN_MAX_CHALLENGE_TTL_SECONDS as i64
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let verification_method = anp::authentication::create_verification_method(public_method)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    if !matches!(
        verification_method.public_key,
        anp::PublicKeyMaterial::Ed25519(_)
    ) {
        return Err(crate::ImError::PermissionDenied);
    }
    verification_method
        .verify_signature(
            &device_proof_bytes(proof, purpose, method, params)?,
            &proof.signature,
        )
        .map_err(|_| crate::ImError::PermissionDenied)
}

fn admin_signing_method(
    did_document: &Value,
    admin_device_id: &str,
    proof_key_id: &str,
) -> crate::ImResult<Value> {
    let entry = anp::authentication::find_eligible_device(
        did_document,
        admin_device_id,
        anp::authentication::PROFILE_DIRECT_E2EE_V2,
    )
    .map_err(|_| crate::ImError::PermissionDenied)?
    .ok_or(crate::ImError::PermissionDenied)?;
    if entry.signing_key_id != proof_key_id {
        return Err(crate::ImError::PermissionDenied);
    }
    did_document
        .get("verificationMethod")
        .and_then(Value::as_array)
        .and_then(|methods| {
            methods
                .iter()
                .find(|method| method.get("id").and_then(Value::as_str) == Some(proof_key_id))
        })
        .cloned()
        .ok_or(crate::ImError::PermissionDenied)
}

fn validate_current_document(
    did_document: &Value,
    did: &str,
    expected_hash: &str,
) -> crate::ImResult<()> {
    if did_document.get("id").and_then(Value::as_str) != Some(did)
        || canonical_hash(did_document)? != expected_hash
        || !anp::authentication::validate_did_document_binding(did_document, true)
    {
        return Err(crate::ImError::PermissionDenied);
    }
    anp::authentication::validate_device_manifest(did_document)
        .map_err(|_| crate::ImError::PermissionDenied)?
        .ok_or(crate::ImError::PermissionDenied)?;
    Ok(())
}

fn encrypt_challenge(
    admin_pairing_private: &anp::PrivateKeyMaterial,
    join_request: &DeviceJoinRequest,
    join_request_hash: &str,
    challenge_id: &str,
    admin_device_id: &str,
    admin_pairing_public_key: &str,
    challenge_expires_at: &str,
    plaintext: &[u8],
) -> crate::ImResult<EncryptedJoinChallenge> {
    let peer = x25519_public_from_method(&join_request.e2ee_public_key)?;
    let shared = x25519_shared(admin_pairing_private, peer)?;
    let aad = challenge_aad(
        join_request,
        join_request_hash,
        challenge_id,
        admin_device_id,
        admin_pairing_public_key,
        challenge_expires_at,
    )?;
    let key = derive_key(&shared, &aad, CHALLENGE_KDF_INFO)?;
    let mut nonce = [0_u8; JOIN_NONCE_LEN];
    rand::rngs::OsRng
        .try_fill_bytes(&mut nonce)
        .map_err(|_| crate::ImError::Internal {
            message: "secure Join challenge nonce generation failed".to_owned(),
        })?;
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key.as_ref()));
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad: &aad,
            },
        )
        .map_err(|_| crate::ImError::Internal {
            message: "encrypt Join challenge failed".to_owned(),
        })?;
    Ok(EncryptedJoinChallenge {
        algorithm: DEVICE_JOIN_CHALLENGE_ALGORITHM.to_owned(),
        nonce_b64u: URL_SAFE_NO_PAD.encode(nonce),
        ciphertext_b64u: URL_SAFE_NO_PAD.encode(ciphertext),
    })
}

fn decrypt_challenge(
    new_device_e2ee_private: &anp::PrivateKeyMaterial,
    join_request: &DeviceJoinRequest,
    join_request_hash: &str,
    challenge: &DeviceJoinChallenge,
) -> crate::ImResult<SecretBytes> {
    if challenge.ciphertext.algorithm != DEVICE_JOIN_CHALLENGE_ALGORITHM {
        return Err(crate::ImError::PermissionDenied);
    }
    let peer = decode_x25519_b64u(
        "challenge.admin_pairing_public_key",
        &challenge.admin_pairing_public_key,
    )?;
    let shared = x25519_shared(new_device_e2ee_private, peer)?;
    let aad = challenge_aad(
        join_request,
        join_request_hash,
        &challenge.challenge_id,
        &challenge.admin_device_id,
        &challenge.admin_pairing_public_key,
        &challenge.challenge_expires_at,
    )?;
    let key = derive_key(&shared, &aad, CHALLENGE_KDF_INFO)?;
    let nonce = URL_SAFE_NO_PAD
        .decode(challenge.ciphertext.nonce_b64u.as_bytes())
        .map_err(|_| crate::ImError::PermissionDenied)?;
    let nonce: [u8; JOIN_NONCE_LEN] = nonce
        .try_into()
        .map_err(|_| crate::ImError::PermissionDenied)?;
    let ciphertext = URL_SAFE_NO_PAD
        .decode(challenge.ciphertext.ciphertext_b64u.as_bytes())
        .map_err(|_| crate::ImError::PermissionDenied)?;
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key.as_ref()));
    let plaintext = cipher
        .decrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: &ciphertext,
                aad: &aad,
            },
        )
        .map_err(|_| crate::ImError::PermissionDenied)?;
    if plaintext.len() != JOIN_CHALLENGE_LEN {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(SecretBytes::from_vec(plaintext))
}

fn challenge_aad(
    join_request: &DeviceJoinRequest,
    join_request_hash: &str,
    challenge_id: &str,
    admin_device_id: &str,
    admin_pairing_public_key: &str,
    challenge_expires_at: &str,
) -> crate::ImResult<Vec<u8>> {
    canonical_bytes(&json!({
        "type": "awiki.device.join.challenge-aad.v1",
        "did": join_request.did,
        "join_session_id": join_request.join_session_id,
        "challenge_id": challenge_id,
        "admin_device_id": admin_device_id,
        "new_device_id": join_request.device_id,
        "join_request_hash": join_request_hash,
        "new_pairing_public_key": join_request.pairing_public_key,
        "admin_pairing_public_key": admin_pairing_public_key,
        "new_e2ee_key_id": method_id(&join_request.e2ee_public_key, "e2ee_public_key")?,
        "challenge_expires_at": challenge_expires_at,
    }))
}

fn join_transcript(
    join_request: &DeviceJoinRequest,
    join_request_hash: &str,
    challenge: &DeviceJoinChallenge,
    challenge_hash: &str,
    checkpoint: &InternalCheckpoint,
) -> crate::ImResult<Value> {
    Ok(json!({
        "type": "awiki.device.join.transcript.v1",
        "did": join_request.did,
        "join_session_id": join_request.join_session_id,
        "admin_device_id": challenge.admin_device_id,
        "new_device_id": join_request.device_id,
        "join_request_hash": join_request_hash,
        "challenge_id": challenge.challenge_id,
        "challenge_hash": challenge_hash,
        "new_pairing_public_key": join_request.pairing_public_key,
        "admin_pairing_public_key": challenge.admin_pairing_public_key,
        "new_signing_public_key": join_request.signing_public_key,
        "new_e2ee_public_key": join_request.e2ee_public_key,
        "document_version": checkpoint.document_version,
        "document_hash": checkpoint.document_hash,
    }))
}

fn derive_stored_sas(
    core: &crate::core::ImCore,
    stored: &StoredJoinSession,
) -> crate::ImResult<String> {
    let challenge = stored
        .challenge
        .as_ref()
        .ok_or_else(|| invalid_state("challenge missing"))?;
    let checkpoint = stored
        .checkpoint
        .as_ref()
        .ok_or_else(|| invalid_state("checkpoint missing"))?;
    let challenge_hash = stored
        .challenge_hash
        .as_deref()
        .ok_or_else(|| invalid_state("challenge hash missing"))?;
    let transcript = join_transcript(
        &stored.join_request,
        &stored.join_request_hash,
        challenge,
        challenge_hash,
        checkpoint,
    )?;
    derive_sas_for_state(core, stored, &transcript)
}

fn derive_sas_for_state(
    core: &crate::core::ImCore,
    stored: &StoredJoinSession,
    transcript: &Value,
) -> crate::ImResult<String> {
    let vault = required_vault(core)?;
    let pairing_private = open_private_key(
        &*vault,
        &stored.pairing_private_ref,
        SecretKind::IdentityJoinPairingPrivate,
    )?;
    let challenge = stored
        .challenge
        .as_ref()
        .ok_or_else(|| invalid_state("challenge missing"))?;
    let peer = match stored.side {
        DeviceJoinSide::NewDevice => decode_x25519_b64u(
            "challenge.admin_pairing_public_key",
            &challenge.admin_pairing_public_key,
        )?,
        DeviceJoinSide::Admin => decode_x25519_b64u(
            "join_request.pairing_public_key",
            &stored.join_request.pairing_public_key,
        )?,
    };
    let shared = x25519_shared(&pairing_private, peer)?;
    derive_sas(&shared, transcript)
}

fn derive_sas(shared: &[u8; 32], transcript: &Value) -> crate::ImResult<String> {
    let transcript = canonical_bytes(transcript)?;
    let mut salt = [0_u8; 32];
    salt.copy_from_slice(&Sha256::digest(&transcript));
    let hkdf = Hkdf::<Sha256>::new(Some(&salt), shared);
    let mut sas_key = Zeroizing::new([0_u8; 32]);
    hkdf.expand(SAS_KDF_INFO, sas_key.as_mut())
        .map_err(|_| crate::ImError::Internal {
            message: "derive Join SAS key failed".to_owned(),
        })?;
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(sas_key.as_ref()).map_err(|_| {
        crate::ImError::Internal {
            message: "initialize Join SAS MAC failed".to_owned(),
        }
    })?;
    mac.update(&transcript);
    let mut digest = mac.finalize().into_bytes();
    let value = u64::from_be_bytes(digest[..8].try_into().expect("eight digest bytes"));
    digest.zeroize();
    Ok(format!("{:06}", value % 1_000_000))
}

fn derive_key(shared: &[u8; 32], aad: &[u8], info: &[u8]) -> crate::ImResult<Zeroizing<[u8; 32]>> {
    let salt = Sha256::digest(aad);
    let hkdf = Hkdf::<Sha256>::new(Some(salt.as_slice()), shared);
    let mut key = Zeroizing::new([0_u8; 32]);
    hkdf.expand(info, key.as_mut())
        .map_err(|_| crate::ImError::Internal {
            message: "derive Join challenge key failed".to_owned(),
        })?;
    Ok(key)
}

fn x25519_shared(
    private_key: &anp::PrivateKeyMaterial,
    peer: [u8; 32],
) -> crate::ImResult<Zeroizing<[u8; 32]>> {
    let anp::PrivateKeyMaterial::X25519(private_key) = private_key else {
        return Err(crate::ImError::PermissionDenied);
    };
    let shared = private_key.diffie_hellman(&X25519PublicKey::from(peer));
    let bytes = Zeroizing::new(shared.to_bytes());
    if bytes.iter().all(|value| *value == 0) {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(bytes)
}

fn x25519_public_from_method(method: &Value) -> crate::ImResult<[u8; 32]> {
    match anp::authentication::extract_public_key(method)
        .map_err(|_| crate::ImError::PermissionDenied)?
    {
        anp::PublicKeyMaterial::X25519(value) => Ok(value),
        _ => Err(crate::ImError::PermissionDenied),
    }
}

fn x25519_public_b64u(public_key: &anp::PublicKeyMaterial) -> crate::ImResult<String> {
    match public_key {
        anp::PublicKeyMaterial::X25519(value) => Ok(URL_SAFE_NO_PAD.encode(value)),
        _ => Err(crate::ImError::PermissionDenied),
    }
}

fn decode_x25519_b64u(field: &str, value: &str) -> crate::ImResult<[u8; 32]> {
    let decoded = URL_SAFE_NO_PAD
        .decode(value.as_bytes())
        .map_err(|_| crate::ImError::invalid_input(Some(field.to_owned()), "invalid X25519 key"))?;
    decoded
        .try_into()
        .map_err(|_| crate::ImError::invalid_input(Some(field.to_owned()), "invalid X25519 key"))
}

fn verification_method(
    did: &str,
    key_id: &str,
    method_type: &str,
    public_key: &anp::PublicKeyMaterial,
) -> crate::ImResult<Value> {
    Ok(json!({
        "id": key_id,
        "type": method_type,
        "controller": did,
        "publicKeyMultibase": public_key_multibase(public_key)?,
    }))
}

fn public_key_multibase(public_key: &anp::PublicKeyMaterial) -> crate::ImResult<String> {
    let (codec, bytes): ([u8; 2], Vec<u8>) = match public_key {
        anp::PublicKeyMaterial::Ed25519(key) => ([0xed, 0x01], key.to_bytes().to_vec()),
        anp::PublicKeyMaterial::X25519(key) => ([0xec, 0x01], key.to_vec()),
        _ => return Err(crate::ImError::PermissionDenied),
    };
    let mut encoded = Vec::with_capacity(codec.len() + bytes.len());
    encoded.extend_from_slice(&codec);
    encoded.extend_from_slice(&bytes);
    Ok(format!("z{}", bs58::encode(encoded).into_string()))
}

fn sign_bytes(private_key: &anp::PrivateKeyMaterial, content: &[u8]) -> crate::ImResult<String> {
    let signature = private_key
        .sign_message(content)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    Ok(URL_SAFE_NO_PAD.encode(signature))
}

fn canonical_bytes(value: &Value) -> crate::ImResult<Vec<u8>> {
    serde_json_canonicalizer::to_vec(value).map_err(|err| crate::ImError::Serialization {
        detail: err.to_string(),
    })
}

fn canonical_hash(value: &Value) -> crate::ImResult<String> {
    canonical_bytes(value).map(|bytes| hash_bytes(&bytes))
}

fn hash_bytes(value: &[u8]) -> String {
    format!("sha256:{}", URL_SAFE_NO_PAD.encode(Sha256::digest(value)))
}

fn validate_checkpoint(version: u64, hash: &str) -> crate::ImResult<InternalCheckpoint> {
    if version == 0 || !valid_sha256_hash(hash) {
        return Err(crate::ImError::invalid_input(
            Some("identity_checkpoint".to_owned()),
            "document version/hash is invalid",
        ));
    }
    Ok(InternalCheckpoint {
        document_version: version,
        document_hash: hash.to_owned(),
    })
}

fn valid_sha256_hash(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|encoded| {
        encoded.len() == 43
            && encoded
                .bytes()
                .all(|value| value.is_ascii_alphanumeric() || matches!(value, b'-' | b'_'))
    })
}

fn method_id<'a>(method: &'a Value, field: &str) -> crate::ImResult<&'a str> {
    method.get("id").and_then(Value::as_str).ok_or_else(|| {
        crate::ImError::invalid_input(Some(field.to_owned()), "verification method id is required")
    })
}

fn validate_join_ttl(ttl_seconds: u64) -> crate::ImResult<()> {
    if !(30..=DEVICE_JOIN_MAX_TTL_SECONDS).contains(&ttl_seconds) {
        return Err(crate::ImError::invalid_input(
            Some("ttl_seconds".to_owned()),
            "Join TTL must be between 30 and 600 seconds",
        ));
    }
    Ok(())
}

fn validate_challenge_ttl(ttl_seconds: u64) -> crate::ImResult<()> {
    if !(30..=DEVICE_JOIN_MAX_CHALLENGE_TTL_SECONDS).contains(&ttl_seconds) {
        return Err(crate::ImError::invalid_input(
            Some("challenge_ttl_seconds".to_owned()),
            "Join challenge TTL must be between 30 and 300 seconds",
        ));
    }
    Ok(())
}

fn validate_operation_id(value: &str) -> crate::ImResult<()> {
    required("operation_id", value).map(|_| ())
}

fn required(field: &str, value: &str) -> crate::ImResult<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some(field.to_owned()),
            format!("{field} is required"),
        ));
    }
    Ok(value.to_owned())
}

fn random_id(prefix: &str, len: usize) -> crate::ImResult<String> {
    random_b64u(len).map(|value| format!("{prefix}-{value}"))
}

fn random_b64u(len: usize) -> crate::ImResult<String> {
    let mut value = vec![0_u8; len];
    rand::rngs::OsRng
        .try_fill_bytes(&mut value)
        .map_err(|_| crate::ImError::Internal {
            message: "secure random generation failed".to_owned(),
        })?;
    Ok(URL_SAFE_NO_PAD.encode(value))
}

fn format_time(value: OffsetDateTime) -> crate::ImResult<String> {
    value
        .format(&Rfc3339)
        .map_err(|err| crate::ImError::Serialization {
            detail: err.to_string(),
        })
}

fn parse_time(field: &str, value: &str) -> crate::ImResult<OffsetDateTime> {
    OffsetDateTime::parse(value, &Rfc3339)
        .map_err(|_| crate::ImError::invalid_input(Some(field.to_owned()), "must be UTC RFC 3339"))
}

fn summary(stored: &StoredJoinSession) -> crate::ImResult<DeviceJoinSessionSummary> {
    Ok(DeviceJoinSessionSummary {
        join_session_id: stored.join_request.join_session_id.clone(),
        did: crate::ids::Did::parse(&stored.join_request.did)?,
        protocol_device_id: crate::ids::ProtocolDeviceId::parse(&stored.join_request.device_id)?,
        side: stored.side,
        phase: stored.phase,
        join_request_hash: stored.join_request_hash.clone(),
        challenge_id: stored
            .challenge
            .as_ref()
            .map(|value| value.challenge_id.clone()),
        expires_at: stored.join_request.expires_at.clone(),
    })
}

fn normalize_expiry(
    core: &crate::core::ImCore,
    store: &JoinStateStore<'_>,
    stored: &mut StoredJoinSession,
) -> crate::ImResult<()> {
    if stored.phase == DeviceJoinLocalPhase::Authorized {
        return Ok(());
    }

    let join_expires_at = parse_time("join_request.expires_at", &stored.join_request.expires_at)?;
    let effective_expires_at = match stored.challenge.as_ref() {
        Some(challenge) => std::cmp::min(
            join_expires_at,
            parse_time(
                "challenge.challenge_expires_at",
                &challenge.challenge_expires_at,
            )?,
        ),
        None => join_expires_at,
    };
    if stored.phase == DeviceJoinLocalPhase::Expired
        || effective_expires_at <= OffsetDateTime::now_utc()
    {
        cleanup_expired_session_secrets(core, stored)?;
        stored.phase = DeviceJoinLocalPhase::Expired;
        store.save(stored)?;
    }
    Ok(())
}

fn cleanup_expired_session_secrets(
    core: &crate::core::ImCore,
    stored: &StoredJoinSession,
) -> crate::ImResult<()> {
    let vault = required_vault(core)?;
    let mut refs = vec![&stored.pairing_private_ref];
    if stored.side == DeviceJoinSide::NewDevice {
        if let Some(secret_ref) = stored.signing_private_ref.as_ref() {
            refs.push(secret_ref);
        }
        if let Some(secret_ref) = stored.e2ee_private_ref.as_ref() {
            refs.push(secret_ref);
        }
    }

    let mut first_error = None;
    for secret_ref in refs {
        if let Err(error) = vault.delete(secret_ref) {
            if first_error.is_none() {
                first_error = Some(error);
            }
        }
    }
    match first_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

fn ensure_not_expired(stored: &StoredJoinSession) -> crate::ImResult<()> {
    if stored.phase == DeviceJoinLocalPhase::Expired {
        Err(crate::ImError::SessionExpired)
    } else {
        Ok(())
    }
}

fn invalid_state(message: &str) -> crate::ImError {
    crate::ImError::LocalStateUnavailable {
        detail: format!("device Join state invalid: {message}"),
    }
}

fn idempotency_conflict(operation: &str) -> crate::ImError {
    crate::ImError::InvalidInput {
        field: Some("operation_id".to_owned()),
        message: format!("device Join idempotency conflict during {operation}"),
    }
}

struct JoinStateStore<'a> {
    paths: &'a crate::paths::IdentityRegistryPaths,
}

impl<'a> JoinStateStore<'a> {
    fn new(core: &'a crate::core::ImCore) -> Self {
        Self {
            paths: &core.inner().sdk_paths().identities,
        }
    }

    fn find_new_device_by_create_operation(
        &self,
        operation_id: &str,
    ) -> crate::ImResult<Option<StoredJoinSession>> {
        for stored in self.list()? {
            if stored.side == DeviceJoinSide::NewDevice
                && stored.create_operation_id.as_deref() == Some(operation_id)
            {
                return Ok(Some(stored));
            }
        }
        Ok(None)
    }

    fn load(
        &self,
        join_session_id: &str,
        side: DeviceJoinSide,
    ) -> crate::ImResult<Option<StoredJoinSession>> {
        let path = self.path(join_session_id, side);
        let raw = match fs::read(&path) {
            Ok(value) => value,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(crate::ImError::from(err)),
        };
        let stored: StoredJoinSession =
            serde_json::from_slice(&raw).map_err(|_| invalid_state("state JSON unreadable"))?;
        self.validate_loaded(stored, join_session_id, side)
            .map(Some)
    }

    fn list(&self) -> crate::ImResult<Vec<StoredJoinSession>> {
        let entries = match fs::read_dir(self.dir()) {
            Ok(value) => value,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(crate::ImError::from(err)),
        };
        let mut result = Vec::new();
        for entry in entries {
            let entry = entry.map_err(crate::ImError::from)?;
            if entry.path().extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let raw = fs::read(entry.path())?;
            let stored: StoredJoinSession =
                serde_json::from_slice(&raw).map_err(|_| invalid_state("state JSON unreadable"))?;
            let session_id = stored.join_request.join_session_id.clone();
            let side = stored.side;
            result.push(self.validate_loaded(stored, &session_id, side)?);
        }
        Ok(result)
    }

    fn save(&self, stored: &StoredJoinSession) -> crate::ImResult<()> {
        self.validate_loaded(
            stored.clone(),
            &stored.join_request.join_session_id,
            stored.side,
        )?;
        let raw =
            serde_json::to_vec_pretty(stored).map_err(|err| crate::ImError::Serialization {
                detail: err.to_string(),
            })?;
        write_private_atomic(
            &self.path(&stored.join_request.join_session_id, stored.side),
            &raw,
        )
    }

    fn validate_loaded(
        &self,
        stored: StoredJoinSession,
        expected_session_id: &str,
        expected_side: DeviceJoinSide,
    ) -> crate::ImResult<StoredJoinSession> {
        if stored.schema_version != JOIN_STATE_SCHEMA_VERSION
            || stored.side != expected_side
            || stored.join_request.join_session_id != expected_session_id
            || stored.join_request_hash
                != canonical_hash(&serde_json::to_value(&stored.join_request).map_err(|err| {
                    crate::ImError::Serialization {
                        detail: err.to_string(),
                    }
                })?)?
            || stored.pairing_private_ref.kind != SecretKind::IdentityJoinPairingPrivate
        {
            return Err(invalid_state("state binding mismatch"));
        }
        match stored.side {
            DeviceJoinSide::NewDevice => {
                if stored
                    .signing_private_ref
                    .as_ref()
                    .is_none_or(|value| value.kind != SecretKind::IdentityDeviceSigningPrivate)
                    || stored
                        .e2ee_private_ref
                        .as_ref()
                        .is_none_or(|value| value.kind != SecretKind::IdentityE2eeAgreementPrivate)
                    || stored.admin_identity.is_some()
                {
                    return Err(invalid_state("new-device secret reference mismatch"));
                }
            }
            DeviceJoinSide::Admin => {
                if stored.signing_private_ref.is_some()
                    || stored.e2ee_private_ref.is_some()
                    || stored.admin_identity.is_none()
                {
                    return Err(invalid_state("admin state secret reference mismatch"));
                }
            }
        }
        Ok(stored)
    }

    fn dir(&self) -> PathBuf {
        self.paths.identity_root_dir.join(JOIN_STATE_DIR)
    }

    fn path(&self, join_session_id: &str, side: DeviceJoinSide) -> PathBuf {
        let mut hasher = Sha256::new();
        hasher.update(match side {
            DeviceJoinSide::NewDevice => b"new".as_slice(),
            DeviceJoinSide::Admin => b"admin".as_slice(),
        });
        hasher.update([0]);
        hasher.update(join_session_id.as_bytes());
        self.dir().join(format!(
            "{}.json",
            URL_SAFE_NO_PAD.encode(hasher.finalize())
        ))
    }
}

fn write_private_atomic(path: &Path, raw: &[u8]) -> crate::ImResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| crate::ImError::PathUnavailable {
            path_kind: "device_join_state".to_owned(),
            detail: "state path has no parent".to_owned(),
        })?;
    fs::create_dir_all(parent)?;
    set_private_dir_mode(parent)?;
    let temp = path.with_file_name(format!(
        ".{}.{}.{}.tmp",
        path.file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("join.json"),
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or(0)
    ));
    {
        let mut file = create_private_file(&temp)?;
        file.write_all(raw)?;
        file.sync_all()?;
    }
    fs::rename(&temp, path).map_err(|err| crate::ImError::Io {
        detail: format!(
            "rename device Join state {} to {}: {err}",
            temp.display(),
            path.display()
        ),
    })?;
    set_private_file_mode(path)
}

#[cfg(unix)]
fn create_private_file(path: &Path) -> crate::ImResult<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .map_err(crate::ImError::from)
}

#[cfg(not(unix))]
fn create_private_file(path: &Path) -> crate::ImResult<fs::File> {
    fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(crate::ImError::from)
}

#[cfg(unix)]
fn set_private_dir_mode(path: &Path) -> crate::ImResult<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_private_dir_mode(_path: &Path) -> crate::ImResult<()> {
    Ok(())
}

#[cfg(unix)]
fn set_private_file_mode(path: &Path) -> crate::ImResult<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_private_file_mode(_path: &Path) -> crate::ImResult<()> {
    Ok(())
}

#[cfg(test)]
mod tests;
