//! Restart-safe AWiki-local device Join crypto, intent and state.
//!
//! Private signing, E2EE and pairing material is always sealed in SecretVault.
//! The adjacent state file contains only public control objects, digests and
//! opaque SecretVault references. Remote tokens stay sealed, approval requests
//! are frozen before network I/O, and SAS values are derived on demand rather
//! than persisted. Join state mutations are serialized across both threads and
//! processes that share the same identity root.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use fs2::FileExt as _;
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
    DeviceJoinObjectProof, DeviceJoinRequest, DeviceJoinRequestProof, DeviceJoinSessionSummary,
    DeviceJoinSide, DeviceJoinStartRequest, DeviceJoinStartResult, EncryptedJoinChallenge,
    DEVICE_JOIN_CHALLENGE_ALGORITHM, DEVICE_JOIN_LEGACY_DRAFT_PROFILES,
    DEVICE_JOIN_MAX_CHALLENGE_TTL_SECONDS, DEVICE_JOIN_MAX_TTL_SECONDS,
    DEVICE_JOIN_REQUEST_PROOF_INPUT_TYPE, DEVICE_JOIN_REQUEST_PROOF_TYPE, DEVICE_JOIN_REQUEST_TYPE,
    DEVICE_JOIN_RESPONSE_SIGNATURE_INPUT_TYPE, DEVICE_JOIN_VNEXT_PROFILES,
};
use crate::internal::platform_secret::SecretBytes;
use crate::internal::secret_vault::record::{SecretKind, SecretMetadata, SecretRef};
use crate::internal::secret_vault::{SealSecretRequest, SecretAccessPolicy, SecretVault};

const JOIN_STATE_SCHEMA_VERSION: u32 = 1;
const JOIN_STATE_DIR: &str = ".device-join";
const JOIN_STATE_LOCK_FILE: &str = ".awiki-device-join-state.lock";
const JOIN_CHALLENGE_LEN: usize = 32;
const JOIN_NONCE_LEN: usize = 12;
const JOIN_RANDOM_ID_LEN: usize = 16;
const CHALLENGE_KDF_INFO: &[u8] = b"awiki-device-join-challenge-v1";
const SAS_KDF_INFO: &[u8] = b"awiki-device-join-sas-v1";
const JOIN_PROOF_ALGORITHM: &str = "Ed25519";
const JOIN_CHALLENGE_PLAINTEXT_TYPE: &str = "awiki.device.join.challenge-plaintext.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct InternalCheckpoint {
    document_version: u64,
    document_hash: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JoinChallengePlaintext {
    #[serde(rename = "type")]
    plaintext_type: String,
    random_challenge_b64u: String,
    document_version: u64,
    document_hash: String,
}

impl Drop for JoinChallengePlaintext {
    fn drop(&mut self) {
        self.random_challenge_b64u.zeroize();
    }
}

struct DecryptedJoinChallenge {
    canonical_plaintext: SecretBytes,
    checkpoint: InternalCheckpoint,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct StoredAdminApproval {
    operation_id: String,
    input_hash: String,
    expected_checkpoint: crate::internal::identity_device_state::IdentityInternalCheckpoint,
    new_document: Value,
    pairing_confirmation:
        crate::internal::identity_device_join_runtime::DeviceJoinRemotePairingConfirmation,
    authorizing_device_id: String,
    proof: DeviceJoinObjectProof,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PreparedAdminApproval {
    pub(crate) operation_id: String,
    pub(crate) join_session_id: String,
    pub(crate) expected_checkpoint:
        crate::internal::identity_device_state::IdentityInternalCheckpoint,
    pub(crate) new_document: Value,
    pub(crate) pairing_confirmation:
        crate::internal::identity_device_join_runtime::DeviceJoinRemotePairingConfirmation,
    pub(crate) authorizing_device_id: String,
    pub(crate) proof: DeviceJoinObjectProof,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PreparedAdminRejection {
    pub(crate) operation_id: String,
    pub(crate) rejecting_device_id: String,
    pub(crate) proof: DeviceJoinObjectProof,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    join_session_token_ref: Option<SecretRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    approval: Option<StoredAdminApproval>,
    #[serde(default, skip_serializing_if = "is_false")]
    activation_pending: bool,
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
            .field(
                "has_join_session_token",
                &self.join_session_token_ref.is_some(),
            )
            .field("has_approval", &self.approval.is_some())
            .field("activation_pending", &self.activation_pending)
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
    if let Some(stored) = store.find_new_device_by_create_operation(&request.operation_id)? {
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
        &signing_private.public_key(),
    )?;
    let e2ee_method = verification_method(
        request.did.as_str(),
        &e2ee_key_id,
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
        join_request_proof: DeviceJoinRequestProof {
            proof_type: DEVICE_JOIN_REQUEST_PROOF_TYPE.to_owned(),
            algorithm: JOIN_PROOF_ALGORITHM.to_owned(),
            verification_method: signing_key_id.clone(),
            created_at: format_time(now)?,
            proof_value_b64u: String::new(),
        },
    };
    join_request.join_request_proof.proof_value_b64u =
        sign_join_request(&join_request, &signing_private)?;
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
        join_session_token_ref: None,
        approval: None,
        activation_pending: false,
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

pub(crate) fn bind_new_device_remote_session(
    core: &crate::core::ImCore,
    join_session_id: &str,
    join_session_token: &SecretBytes,
    remote_expires_at: &str,
) -> crate::ImResult<DeviceJoinSessionSummary> {
    let _guard = lock_join_state(core)?;
    let join_session_id = required("join_session_id", join_session_id)?;
    let store = JoinStateStore::new(core);
    let mut stored = store
        .load(&join_session_id, DeviceJoinSide::NewDevice)?
        .ok_or_else(|| crate::ImError::IdentityNotFound {
            selector: join_session_id.clone(),
        })?;
    ensure_not_expired(&stored)?;
    if remote_expires_at != stored.join_request.expires_at {
        return Err(crate::ImError::PermissionDenied);
    }
    if join_session_token.expose_secret().is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("join_session_token".to_owned()),
            "Join session token is required",
        ));
    }

    let vault = required_vault(core)?;
    if let Some(secret_ref) = stored.join_session_token_ref.as_ref() {
        if secret_ref.kind != SecretKind::IdentityJoinSessionToken {
            return Err(invalid_state("Join session token reference mismatch"));
        }
        let existing = vault.open(secret_ref)?;
        if existing.expose_secret() != join_session_token.expose_secret() {
            return Err(idempotency_conflict("bind_new_device_remote_session"));
        }
        return summary(&stored);
    }

    let secret_ref = seal_join_secret(
        core,
        &*vault,
        &crate::ids::Did::parse(&stored.join_request.did)?,
        SecretKind::IdentityJoinSessionToken,
        &format!("{join_session_id}:session-token"),
        join_session_token.expose_secret(),
    )?;
    stored.join_session_token_ref = Some(secret_ref.clone());
    if let Err(error) = store.save(&stored) {
        let _ = vault.delete(&secret_ref);
        return Err(error);
    }
    summary(&stored)
}

pub(crate) fn open_new_device_remote_session_token(
    core: &crate::core::ImCore,
    join_session_id: &str,
) -> crate::ImResult<SecretBytes> {
    let _guard = lock_join_state(core)?;
    let join_session_id = required("join_session_id", join_session_id)?;
    let store = JoinStateStore::new(core);
    let stored = store
        .load(&join_session_id, DeviceJoinSide::NewDevice)?
        .ok_or_else(|| crate::ImError::IdentityNotFound {
            selector: join_session_id.clone(),
        })?;
    // The remote terminal state is authoritative. Keep the token available
    // after a local-clock expiry so status/cancel can converge with the
    // server before candidate secrets are deleted.
    let secret_ref = stored
        .join_session_token_ref
        .as_ref()
        .filter(|value| value.kind == SecretKind::IdentityJoinSessionToken)
        .ok_or_else(|| invalid_state("Join session token is missing"))?;
    required_vault(core)?.open(secret_ref)
}

pub(crate) fn load_prepared_admin_challenge(
    core: &crate::core::ImCore,
    join_session_id: &str,
    operation_id: &str,
) -> crate::ImResult<Option<DeviceJoinAdminPrepareResult>> {
    let _guard = lock_join_state(core)?;
    let join_session_id = required("join_session_id", join_session_id)?;
    let operation_id = required("operation_id", operation_id)?;
    let store = JoinStateStore::new(core);
    let Some(stored) = store.load(&join_session_id, DeviceJoinSide::Admin)? else {
        return Ok(None);
    };
    ensure_not_expired(&stored)?;
    if stored.transition_operation_id.as_deref() != Some(operation_id.as_str()) {
        return Err(idempotency_conflict("load_prepared_admin_challenge"));
    }
    if !matches!(
        stored.phase,
        DeviceJoinLocalPhase::ChallengePrepared
            | DeviceJoinLocalPhase::ResponseVerified
            | DeviceJoinLocalPhase::ApprovalPrepared
    ) {
        return Err(invalid_state("admin challenge is not prepared"));
    }
    Ok(Some(DeviceJoinAdminPrepareResult {
        session: summary(&stored)?,
        challenge: stored
            .challenge
            .clone()
            .ok_or_else(|| invalid_state("challenge missing"))?,
        sas: derive_stored_sas(core, &stored)?,
    }))
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
    if let Some(stored) =
        store.load(&request.join_request.join_session_id, DeviceJoinSide::Admin)?
    {
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

    let (client, admin_device_id, admin_signing_key_id) = ready_admin_context(
        core,
        &request.admin_identity,
        Some(request.join_request.did.as_str()),
    )?;
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
    let mut random_challenge = Zeroizing::new([0_u8; JOIN_CHALLENGE_LEN]);
    rand::rngs::OsRng
        .try_fill_bytes(random_challenge.as_mut())
        .map_err(|_| crate::ImError::Internal {
            message: "secure Join challenge generation failed".to_owned(),
        })?;
    let challenge_plaintext = encode_challenge_plaintext(&random_challenge, &checkpoint)?;
    let challenge_hash = hash_bytes(challenge_plaintext.as_slice());
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
        challenge_plaintext.as_slice(),
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
    let proof = sign_object_proof(
        &signing_private,
        &admin_signing_key_id,
        &params,
        &created_at,
    )?;
    let challenge = DeviceJoinChallenge {
        operation_id: request.operation_id.clone(),
        join_session_id: request.join_request.join_session_id.clone(),
        challenge_id,
        admin_device_id,
        admin_pairing_public_key,
        ciphertext,
        challenge_expires_at,
        proof,
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
        join_session_token_ref: None,
        approval: None,
        activation_pending: false,
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
    let expected_checkpoint =
        validate_checkpoint(request.document_version, &request.document_hash)?;
    respond_as_new_device_inner(
        core,
        request.operation_id,
        request.challenge,
        request.admin_did_document,
        Some(expected_checkpoint),
    )
}

pub(crate) fn respond_as_new_device_to_resolved_document(
    core: &crate::core::ImCore,
    operation_id: String,
    challenge: DeviceJoinChallenge,
    admin_did_document: Value,
) -> crate::ImResult<DeviceJoinNewDeviceRespondResult> {
    respond_as_new_device_inner(core, operation_id, challenge, admin_did_document, None)
}

fn respond_as_new_device_inner(
    core: &crate::core::ImCore,
    operation_id: String,
    challenge: DeviceJoinChallenge,
    admin_did_document: Value,
    expected_checkpoint: Option<InternalCheckpoint>,
) -> crate::ImResult<DeviceJoinNewDeviceRespondResult> {
    let _guard = lock_join_state(core)?;
    validate_operation_id(&operation_id)?;
    let store = JoinStateStore::new(core);
    let mut stored = store
        .load(&challenge.join_session_id, DeviceJoinSide::NewDevice)?
        .ok_or_else(|| crate::ImError::IdentityNotFound {
            selector: challenge.join_session_id.clone(),
        })?;
    ensure_not_expired(&stored)?;
    if !matches!(
        stored.phase,
        DeviceJoinLocalPhase::Pending | DeviceJoinLocalPhase::ResponsePrepared
    ) {
        return Err(invalid_state("new-device Join is not pending"));
    }
    if challenge.join_session_id != stored.join_request.join_session_id {
        return Err(crate::ImError::PermissionDenied);
    }
    let challenge_expires = parse_time(
        "challenge.challenge_expires_at",
        &challenge.challenge_expires_at,
    )?;
    if challenge_expires <= OffsetDateTime::now_utc() {
        return Err(crate::ImError::SessionExpired);
    }
    let admin_signing_method = admin_signing_method(
        &admin_did_document,
        &challenge.admin_device_id,
        &challenge.proof.verification_method,
    )?;
    let challenge_params = challenge_params_value(
        &challenge.operation_id,
        &challenge.join_session_id,
        &challenge.challenge_id,
        &challenge.admin_device_id,
        &challenge.admin_pairing_public_key,
        &challenge.ciphertext,
        &challenge.challenge_expires_at,
    );
    verify_object_proof(
        &challenge.proof,
        &challenge_params,
        &stored.join_request.did,
        &admin_did_document,
        &admin_signing_method,
    )?;

    let vault = required_vault(core)?;
    let e2ee_ref = stored
        .e2ee_private_ref
        .as_ref()
        .ok_or_else(|| invalid_state("new-device E2EE key reference missing"))?;
    let e2ee_private =
        open_private_key(&*vault, e2ee_ref, SecretKind::IdentityE2eeAgreementPrivate)?;
    let decrypted_challenge = decrypt_challenge(
        &e2ee_private,
        &stored.join_request,
        &stored.join_request_hash,
        &challenge,
    )?;
    if let Some(expected_checkpoint) = expected_checkpoint.as_ref() {
        ensure_challenge_checkpoint(&decrypted_challenge.checkpoint, expected_checkpoint)?;
    }
    validate_current_document(
        &admin_did_document,
        &stored.join_request.did,
        &decrypted_challenge.checkpoint.document_hash,
    )?;
    let input_hash = canonical_hash(&json!({
        "operation_id": operation_id,
        "challenge": challenge,
        "admin_did_document": admin_did_document,
        "document_version": decrypted_challenge.checkpoint.document_version,
        "document_hash": decrypted_challenge.checkpoint.document_hash,
    }))?;
    if stored.phase == DeviceJoinLocalPhase::ResponsePrepared {
        if stored.transition_operation_id.as_deref() != Some(operation_id.as_str())
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
    let challenge_hash = hash_bytes(decrypted_challenge.canonical_plaintext.expose_secret());
    let transcript = join_transcript(
        &stored.join_request,
        &stored.join_request_hash,
        &challenge,
        &challenge_hash,
        &decrypted_challenge.checkpoint,
    )?;
    let pairing_transcript_hash = canonical_hash(&transcript)?;
    let signing_ref = stored
        .signing_private_ref
        .as_ref()
        .ok_or_else(|| invalid_state("new-device signing key reference missing"))?;
    let signing_private = open_private_key(
        &*vault,
        signing_ref,
        SecretKind::IdentityDeviceSigningPrivate,
    )?;
    let response_params = response_params_value(
        &operation_id,
        &stored.join_request.join_session_id,
        &challenge.challenge_id,
        &challenge_hash,
        &stored.join_request_hash,
        &pairing_transcript_hash,
    );
    let response_signature_b64u =
        sign_bytes(&signing_private, &canonical_bytes(&response_params)?)?;
    let response = DeviceJoinChallengeResponse {
        operation_id: operation_id.clone(),
        join_session_id: stored.join_request.join_session_id.clone(),
        challenge_id: challenge.challenge_id.clone(),
        challenge_hash: challenge_hash.clone(),
        join_request_hash: stored.join_request_hash.clone(),
        pairing_transcript_hash,
        response_signature_b64u,
    };
    stored.phase = DeviceJoinLocalPhase::ResponsePrepared;
    stored.transition_operation_id = Some(operation_id);
    stored.transition_input_hash = Some(input_hash);
    stored.checkpoint = Some(decrypted_challenge.checkpoint);
    stored.challenge = Some(challenge);
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
    let verification_method =
        create_join_verification_method(&stored.join_request.signing_public_key)?;
    verification_method
        .verify_signature(
            &canonical_bytes(&response_params)?,
            &request.response.response_signature_b64u,
        )
        .map_err(|_| crate::ImError::PermissionDenied)?;
    stored.phase = DeviceJoinLocalPhase::ResponseVerified;
    stored.verification_operation_id = Some(request.operation_id);
    stored.verification_input_hash = Some(input_hash);
    stored.response = Some(request.response);
    store.save(&stored)?;
    verified_result(core, &stored)
}

pub(crate) fn prepare_admin_approval(
    core: &crate::core::ImCore,
    operation_id: &str,
    join_session_id: &str,
    expected_checkpoint: &crate::internal::identity_device_state::IdentityInternalCheckpoint,
    user_presence_at: &str,
    sas_confirmed: bool,
) -> crate::ImResult<PreparedAdminApproval> {
    let _guard = lock_join_state(core)?;
    let operation_id = required("operation_id", operation_id)?;
    let join_session_id = required("join_session_id", join_session_id)?;
    let user_presence_at = required("user_presence_at", user_presence_at)?;
    parse_time("user_presence_at", &user_presence_at)?;
    if !sas_confirmed {
        return Err(crate::ImError::PermissionDenied);
    }
    let store = JoinStateStore::new(core);
    let mut stored = store
        .load(&join_session_id, DeviceJoinSide::Admin)?
        .ok_or_else(|| crate::ImError::IdentityNotFound {
            selector: join_session_id.clone(),
        })?;
    ensure_not_expired(&stored)?;
    let checkpoint = stored
        .checkpoint
        .as_ref()
        .ok_or_else(|| invalid_state("checkpoint missing"))?;
    if checkpoint.document_version != expected_checkpoint.document_version
        || checkpoint.document_hash != expected_checkpoint.document_hash
        || expected_checkpoint.registry_version == 0
    {
        return Err(crate::ImError::PermissionDenied);
    }
    if stored.join_request.profiles == DEVICE_JOIN_LEGACY_DRAFT_PROFILES {
        return Err(crate::ImError::invalid_input(
            Some("join_request.profiles".to_owned()),
            "legacy draft Device Join requires both devices to upgrade before approval",
        ));
    }
    let input_hash = canonical_hash(&json!({
        "operation_id": operation_id,
        "join_session_id": join_session_id,
        "expected_document_version": expected_checkpoint.document_version,
        "expected_document_hash": expected_checkpoint.document_hash,
        "expected_registry_version": expected_checkpoint.registry_version,
        "user_presence_at": user_presence_at,
        "sas_confirmed": sas_confirmed,
    }))?;
    if stored.phase == DeviceJoinLocalPhase::ApprovalPrepared {
        let approval = stored
            .approval
            .as_ref()
            .ok_or_else(|| invalid_state("prepared approval missing"))?;
        if approval.operation_id != operation_id || approval.input_hash != input_hash {
            return Err(idempotency_conflict("prepare_admin_approval"));
        }
        return prepared_approval_result(&stored, approval);
    }
    if stored.phase != DeviceJoinLocalPhase::ResponseVerified {
        return Err(invalid_state("admin Join response is not verified"));
    }
    let admin_identity = stored
        .admin_identity
        .as_ref()
        .ok_or_else(|| invalid_state("admin identity missing"))?;
    let (client, admin_device_id, admin_signing_key_id) =
        ready_admin_context(core, admin_identity, Some(stored.join_request.did.as_str()))?;
    let current_document = client.runtime().key_provider.did_document()?;
    validate_current_document(
        &current_document,
        &stored.join_request.did,
        &expected_checkpoint.document_hash,
    )?;
    let root_key_id = format!("{}#key-1", stored.join_request.did);
    let device = anp::authentication::DeviceManifestEntry {
        device_id: stored.join_request.device_id.clone(),
        signing_key_id: method_id(
            &stored.join_request.signing_public_key,
            "join_request.signing_public_key",
        )?
        .to_owned(),
        e2ee_key_id: method_id(
            &stored.join_request.e2ee_public_key,
            "join_request.e2ee_public_key",
        )?
        .to_owned(),
        profiles: DEVICE_JOIN_VNEXT_PROFILES
            .iter()
            .map(|value| (*value).to_owned())
            .collect(),
    };
    let mut new_document = anp::authentication::add_device_to_did_document(
        &current_document,
        &root_key_id,
        &device,
        &stored.join_request.signing_public_key,
        &stored.join_request.e2ee_public_key,
        &[],
    )
    .map_err(|error| {
        crate::ImError::invalid_input(
            Some("new_document".to_owned()),
            format!("cannot add Join device to DID Document: {error}"),
        )
    })?;
    let root_private_pem = Zeroizing::new(
        client
            .runtime()
            .key_provider
            .did_document_root_private_pem()?,
    );
    crate::internal::identity_daemon_subkey::resign_did_document_with_key1(
        &mut new_document,
        &crate::ids::Did::parse(&stored.join_request.did)?,
        &root_private_pem,
    )?;
    validate_authorized_document(&stored.join_request, &new_document)?;
    let response = stored
        .response
        .as_ref()
        .ok_or_else(|| invalid_state("verified response missing"))?;
    let pairing_confirmation =
        crate::internal::identity_device_join_runtime::DeviceJoinRemotePairingConfirmation {
            join_request_hash: stored.join_request_hash.clone(),
            pairing_transcript_hash: response.pairing_transcript_hash.clone(),
            sas_confirmed,
            user_presence_at,
        };
    let now = OffsetDateTime::now_utc();
    let created_at = format_time(now)?;
    let params = json!({
        "operation_id": operation_id,
        "join_session_id": join_session_id,
        "expected_document_version": expected_checkpoint.document_version,
        "expected_document_hash": expected_checkpoint.document_hash,
        "expected_registry_version": expected_checkpoint.registry_version,
        "new_document": new_document,
        "pairing_confirmation": pairing_confirmation,
        "authorizing_device_id": admin_device_id,
    });
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
    let proof = sign_object_proof(
        &signing_private,
        &admin_signing_key_id,
        &params,
        &created_at,
    )?;
    let approval = StoredAdminApproval {
        operation_id,
        input_hash,
        expected_checkpoint: expected_checkpoint.clone(),
        new_document,
        pairing_confirmation,
        authorizing_device_id: admin_device_id,
        proof,
    };
    stored.phase = DeviceJoinLocalPhase::ApprovalPrepared;
    stored.approval = Some(approval.clone());
    store.save(&stored)?;
    prepared_approval_result(&stored, &approval)
}

pub(crate) fn prepare_admin_rejection(
    core: &crate::core::ImCore,
    admin_identity: crate::identity::IdentitySelector,
    join_session_id: &str,
    reason: crate::identity::DeviceJoinRejectReason,
) -> crate::ImResult<PreparedAdminRejection> {
    let join_session_id = required("join_session_id", join_session_id)?;
    let (client, rejecting_device_id, signing_key_id) =
        ready_admin_context(core, &admin_identity, None)?;
    let mut digest = Sha256::new();
    for value in [
        "awiki.device.join.reject.operation.v1",
        client.did().as_str(),
        join_session_id.as_str(),
        reason.as_str(),
        rejecting_device_id.as_str(),
    ] {
        digest.update((value.len() as u64).to_be_bytes());
        digest.update(value.as_bytes());
    }
    let operation_id = format!("join-reject-{}", URL_SAFE_NO_PAD.encode(digest.finalize()));
    let params = json!({
        "operation_id": operation_id,
        "join_session_id": join_session_id,
        "rejecting_device_id": rejecting_device_id,
        "reason": reason.as_str(),
    });
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
    let proof = sign_object_proof(
        &signing_private,
        &signing_key_id,
        &params,
        &format_time(OffsetDateTime::now_utc())?,
    )?;
    Ok(PreparedAdminRejection {
        operation_id,
        rejecting_device_id,
        proof,
    })
}

pub(crate) fn load_prepared_admin_approval(
    core: &crate::core::ImCore,
    join_session_id: &str,
) -> crate::ImResult<Option<PreparedAdminApproval>> {
    let _guard = lock_join_state(core)?;
    let join_session_id = required("join_session_id", join_session_id)?;
    let store = JoinStateStore::new(core);
    let stored = match store.load(&join_session_id, DeviceJoinSide::Admin)? {
        Some(value) => value,
        None => return Ok(None),
    };
    ensure_not_expired(&stored)?;
    stored
        .approval
        .as_ref()
        .map(|approval| prepared_approval_result(&stored, approval))
        .transpose()
}

pub(crate) fn mark_join_authorized(
    core: &crate::core::ImCore,
    join_session_id: &str,
    side: DeviceJoinSide,
    authorization: &crate::internal::identity_device_join_runtime::DeviceJoinRemoteAuthorization,
    resolved_document: &Value,
) -> crate::ImResult<DeviceJoinSessionSummary> {
    if side == DeviceJoinSide::NewDevice {
        return Err(invalid_state(
            "new-device authorization requires token issue and local activation",
        ));
    }
    let _guard = lock_join_state(core)?;
    let join_session_id = required("join_session_id", join_session_id)?;
    let store = JoinStateStore::new(core);
    let mut stored =
        store
            .load(&join_session_id, side)?
            .ok_or_else(|| crate::ImError::IdentityNotFound {
                selector: join_session_id.clone(),
            })?;
    if stored.phase == DeviceJoinLocalPhase::Authorized {
        validate_remote_authorization(&stored, authorization, resolved_document)?;
        if side == DeviceJoinSide::Admin {
            commit_admin_join_projection(
                core,
                stored
                    .admin_identity
                    .as_ref()
                    .ok_or_else(|| invalid_state("admin identity missing"))?,
                &authorization.checkpoint,
                resolved_document,
            )?;
        }
        cleanup_consumed_join_secrets(core, &stored)?;
        return summary(&stored);
    }
    ensure_not_expired(&stored)?;
    match side {
        DeviceJoinSide::NewDevice if stored.phase != DeviceJoinLocalPhase::ResponsePrepared => {
            return Err(invalid_state("new-device response is not prepared"));
        }
        DeviceJoinSide::Admin if stored.phase != DeviceJoinLocalPhase::ApprovalPrepared => {
            return Err(invalid_state("admin approval is not prepared"));
        }
        _ => {}
    }
    validate_remote_authorization(&stored, authorization, resolved_document)?;
    if side == DeviceJoinSide::Admin {
        commit_admin_join_projection(
            core,
            stored
                .admin_identity
                .as_ref()
                .ok_or_else(|| invalid_state("admin identity missing"))?,
            &authorization.checkpoint,
            resolved_document,
        )?;
    }
    stored.phase = DeviceJoinLocalPhase::Authorized;
    store.save(&stored)?;
    cleanup_consumed_join_secrets(core, &stored)?;
    summary(&stored)
}

fn commit_admin_join_projection(
    core: &crate::core::ImCore,
    admin_identity: &crate::identity::IdentitySelector,
    checkpoint: &crate::internal::identity_device_state::IdentityInternalCheckpoint,
    resolved_document: &Value,
) -> crate::ImResult<()> {
    use crate::internal::identity_device_state::{
        DeviceAuthorizationRole, DeviceAuthorizationStatus, IdentityDeviceMode,
    };

    if canonical_hash(resolved_document)? != checkpoint.document_hash {
        return Err(crate::ImError::PermissionDenied);
    }
    let client = core.client(admin_identity.clone())?;
    let local_alias = client
        .current_identity()
        .local_alias
        .as_deref()
        .ok_or(crate::ImError::PermissionDenied)?;
    let identity_store =
        crate::internal::identity_store::IdentityStore::new(&core.inner().sdk_paths().identities);
    let index = identity_store.load_index()?;
    let entry = index
        .credentials
        .get(local_alias)
        .ok_or(crate::ImError::PermissionDenied)?;
    let mut state = entry
        .device_state
        .clone()
        .ok_or(crate::ImError::PermissionDenied)?;
    let local_authorization = state
        .authorization
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    if state.mode != IdentityDeviceMode::VNext
        || local_authorization.status != DeviceAuthorizationStatus::Active
        || local_authorization.role != DeviceAuthorizationRole::Admin
        || !local_authorization.management_ready
    {
        return Err(crate::ImError::PermissionDenied);
    }
    if let Some(current) = state.checkpoint.as_ref() {
        if checkpoint.document_version < current.document_version
            || checkpoint.registry_version < current.registry_version
            || (checkpoint.document_version == current.document_version
                && checkpoint.document_hash != current.document_hash)
        {
            return Err(crate::ImError::PermissionDenied);
        }
    }
    state.checkpoint = Some(checkpoint.clone());
    state.validate_for_did(client.did())?;

    let raw = serde_json::to_vec_pretty(resolved_document).map_err(|error| {
        crate::ImError::Serialization {
            detail: error.to_string(),
        }
    })?;
    write_private_atomic(&client.runtime().did_document_path, &raw)?;
    identity_store.save_device_state(local_alias, state)
}

pub(crate) fn load_pending_new_device_activation(
    core: &crate::core::ImCore,
    join_session_id: &str,
) -> crate::ImResult<Option<crate::internal::identity_join_activation_pending::PendingJoinActivation>>
{
    let _guard = lock_join_state(core)?;
    let join_session_id = required("join_session_id", join_session_id)?;
    let store = JoinStateStore::new(core);
    let stored = store
        .load(&join_session_id, DeviceJoinSide::NewDevice)?
        .ok_or_else(|| crate::ImError::IdentityNotFound {
            selector: join_session_id.clone(),
        })?;
    if !stored.activation_pending {
        return Ok(None);
    }
    let did = crate::ids::Did::parse(&stored.join_request.did)?;
    crate::internal::identity_join_activation_pending::PendingJoinActivationStore::from_core(core)?
        .load(&join_session_id, &did)
        .map(|pending| pending.map(|(_, record)| record))
}

pub(crate) fn prepare_new_device_activation(
    core: &crate::core::ImCore,
    join_session_id: &str,
    authorization: &crate::internal::identity_device_join_runtime::DeviceJoinRemoteAuthorization,
    resolved_document: &Value,
) -> crate::ImResult<crate::internal::identity_join_activation_pending::PendingJoinActivation> {
    let _guard = lock_join_state(core)?;
    let join_session_id = required("join_session_id", join_session_id)?;
    let state_store = JoinStateStore::new(core);
    let mut stored = state_store
        .load(&join_session_id, DeviceJoinSide::NewDevice)?
        .ok_or_else(|| crate::ImError::IdentityNotFound {
            selector: join_session_id.clone(),
        })?;
    if stored.phase == DeviceJoinLocalPhase::Authorized && !stored.activation_pending {
        return Err(invalid_state("new-device Join is already activated"));
    }
    if !stored.activation_pending {
        ensure_not_expired(&stored)?;
        if stored.phase != DeviceJoinLocalPhase::ResponsePrepared {
            return Err(invalid_state("new-device response is not prepared"));
        }
        validate_remote_authorization(&stored, authorization, resolved_document)?;
    }

    let did = crate::ids::Did::parse(&stored.join_request.did)?;
    let pending_store =
        crate::internal::identity_join_activation_pending::PendingJoinActivationStore::from_core(
            core,
        )?;
    if let Some((_, pending)) = pending_store.load(&join_session_id, &did)? {
        if pending.authorization != *authorization
            || pending.resolved_document != *resolved_document
        {
            return Err(crate::ImError::PermissionDenied);
        }
        if !stored.activation_pending {
            stored.activation_pending = true;
            state_store.save(&stored)?;
        }
        return Ok(pending);
    }
    validate_remote_authorization(&stored, authorization, resolved_document)?;
    let signing_ref = stored
        .signing_private_ref
        .as_ref()
        .ok_or_else(|| invalid_state("new-device signing private key is missing"))?;
    let signing_private = open_private_key(
        &*required_vault(core)?,
        signing_ref,
        SecretKind::IdentityDeviceSigningPrivate,
    )?;
    let e2ee_ref = stored
        .e2ee_private_ref
        .as_ref()
        .ok_or_else(|| invalid_state("new-device E2EE private key is missing"))?;
    let e2ee_private = open_private_key(
        &*required_vault(core)?,
        e2ee_ref,
        SecretKind::IdentityE2eeAgreementPrivate,
    )?;
    let domain = crate::internal::identity_join_activation_pending::service_domain_from_did(&did)?;
    if domain
        != core
            .inner()
            .sdk_config()
            .did_domain
            .trim()
            .to_ascii_lowercase()
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let pending = crate::internal::identity_join_activation_pending::PendingJoinActivation::new(
        join_session_id,
        did,
        resolved_document.clone(),
        authorization.clone(),
        signing_private.to_pem(),
        e2ee_private.to_pem(),
    )?;
    pending_store.save(&pending)?;
    if !stored.activation_pending {
        stored.activation_pending = true;
        state_store.save(&stored)?;
    }
    Ok(pending)
}

pub(crate) fn record_new_device_access_result(
    core: &crate::core::ImCore,
    join_session_id: &str,
    result: crate::internal::identity_device_join_runtime::DeviceJoinAccessResult,
) -> crate::ImResult<crate::internal::identity_join_activation_pending::PendingJoinActivation> {
    let _guard = lock_join_state(core)?;
    let join_session_id = required("join_session_id", join_session_id)?;
    let state_store = JoinStateStore::new(core);
    let stored = state_store
        .load(&join_session_id, DeviceJoinSide::NewDevice)?
        .ok_or_else(|| crate::ImError::IdentityNotFound {
            selector: join_session_id.clone(),
        })?;
    if !stored.activation_pending {
        return Err(invalid_state("new-device activation is not pending"));
    }
    let did = crate::ids::Did::parse(&stored.join_request.did)?;
    let pending_store =
        crate::internal::identity_join_activation_pending::PendingJoinActivationStore::from_core(
            core,
        )?;
    let (_, mut pending) = pending_store
        .load(&join_session_id, &did)?
        .ok_or_else(|| invalid_state("new-device activation record is missing"))?;
    if let Some(existing) = pending.access_result.as_ref() {
        if existing != &result {
            return Err(idempotency_conflict("record_new_device_access_result"));
        }
        return Ok(pending);
    }
    pending.access_result = Some(result);
    pending.validate()?;
    pending_store.save(&pending)?;
    Ok(pending)
}

pub(crate) fn finalize_new_device_activation(
    core: &crate::core::ImCore,
    join_session_id: &str,
) -> crate::ImResult<DeviceJoinSessionSummary> {
    let _guard = lock_join_state(core)?;
    let join_session_id = required("join_session_id", join_session_id)?;
    let state_store = JoinStateStore::new(core);
    let mut stored = state_store
        .load(&join_session_id, DeviceJoinSide::NewDevice)?
        .ok_or_else(|| crate::ImError::IdentityNotFound {
            selector: join_session_id.clone(),
        })?;
    if stored.phase == DeviceJoinLocalPhase::Authorized {
        finish_authorized_new_device_cleanup(core, &state_store, &mut stored)?;
        return summary(&stored);
    }
    if !stored.activation_pending || stored.phase != DeviceJoinLocalPhase::ResponsePrepared {
        return Err(invalid_state("new-device activation is not ready"));
    }
    let did = crate::ids::Did::parse(&stored.join_request.did)?;
    let pending_store =
        crate::internal::identity_join_activation_pending::PendingJoinActivationStore::from_core(
            core,
        )?;
    let (_, pending) = pending_store
        .load(&join_session_id, &did)?
        .ok_or_else(|| invalid_state("new-device activation record is missing"))?;
    let access = pending
        .access_result
        .as_ref()
        .ok_or_else(|| invalid_state("new-device access result is missing"))?;
    validate_remote_authorization(&stored, &pending.authorization, &pending.resolved_document)?;
    promote_join_identity(core, &stored, &pending, access)?;

    stored.phase = DeviceJoinLocalPhase::Authorized;
    state_store.save(&stored)?;
    finish_authorized_new_device_cleanup(core, &state_store, &mut stored)?;
    summary(&stored)
}

fn finish_authorized_new_device_cleanup(
    core: &crate::core::ImCore,
    state_store: &JoinStateStore<'_>,
    stored: &mut StoredJoinSession,
) -> crate::ImResult<()> {
    if stored.side != DeviceJoinSide::NewDevice || stored.phase != DeviceJoinLocalPhase::Authorized
    {
        return Err(invalid_state("new-device cleanup phase mismatch"));
    }
    if !stored.activation_pending {
        return Ok(());
    }
    let vault = required_vault(core)?;
    let mut refs = vec![&stored.pairing_private_ref];
    if let Some(secret_ref) = stored.join_session_token_ref.as_ref() {
        refs.push(secret_ref);
    }
    if let Some(secret_ref) = stored.signing_private_ref.as_ref() {
        refs.push(secret_ref);
    }
    if let Some(secret_ref) = stored.e2ee_private_ref.as_ref() {
        refs.push(secret_ref);
    }
    delete_secret_refs(&*vault, refs)?;
    let did = crate::ids::Did::parse(&stored.join_request.did)?;
    let pending_store =
        crate::internal::identity_join_activation_pending::PendingJoinActivationStore::from_core(
            core,
        )?;
    if let Some((secret_ref, _)) = pending_store.load(&stored.join_request.join_session_id, &did)? {
        pending_store.delete(&secret_ref)?;
    }
    stored.activation_pending = false;
    stored.signing_private_ref = None;
    stored.e2ee_private_ref = None;
    stored.join_session_token_ref = None;
    state_store.save(stored)
}

fn promote_join_identity(
    core: &crate::core::ImCore,
    stored: &StoredJoinSession,
    pending: &crate::internal::identity_join_activation_pending::PendingJoinActivation,
    access: &crate::internal::identity_device_join_runtime::DeviceJoinAccessResult,
) -> crate::ImResult<()> {
    if pending.authorization.device.role
        != crate::internal::identity_device_state::DeviceAuthorizationRole::Member
        || pending.authorization.device.management_ready
    {
        return Err(crate::ImError::PermissionDenied);
    }
    use crate::internal::identity_device_state::{
        DeviceAuthorizationProjection, IdentityDeviceMode, IdentityDeviceState,
        IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
    };

    let did = crate::ids::Did::parse(&stored.join_request.did)?;
    if pending.did != did
        || pending.authorization.device.device_id != stored.join_request.device_id
        || pending.authorization.device.signing_key_id
            != method_id(
                &stored.join_request.signing_public_key,
                "join_request.signing_public_key",
            )?
        || pending.authorization.device.e2ee_key_id
            != method_id(
                &stored.join_request.e2ee_public_key,
                "join_request.e2ee_public_key",
            )?
        || pending.authorization.device.management_ready
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let vault = required_vault(core)?;
    let signing_ref = stored
        .signing_private_ref
        .as_ref()
        .ok_or_else(|| invalid_state("new-device signing private key is missing"))?;
    let e2ee_ref = stored
        .e2ee_private_ref
        .as_ref()
        .ok_or_else(|| invalid_state("new-device E2EE private key is missing"))?;
    let signing_private = open_private_key(
        &*vault,
        signing_ref,
        SecretKind::IdentityDeviceSigningPrivate,
    )?;
    let e2ee_private =
        open_private_key(&*vault, e2ee_ref, SecretKind::IdentityE2eeAgreementPrivate)?;
    let expected_signing_public =
        extract_identity_public_key(&stored.join_request.signing_public_key)?;
    let expected_e2ee_public = extract_identity_public_key(&stored.join_request.e2ee_public_key)?;
    let signing_public = signing_private.public_key();
    let e2ee_public = e2ee_private.public_key();
    if signing_public.to_pem() != expected_signing_public.to_pem()
        || e2ee_public.to_pem() != expected_e2ee_public.to_pem()
    {
        return Err(crate::ImError::PermissionDenied);
    }

    let root_key_id = format!("{}#key-1", did.as_str());
    let root_method = pending
        .resolved_document
        .get("verificationMethod")
        .and_then(Value::as_array)
        .and_then(|methods| {
            methods.iter().find(|method| {
                method.get("id").and_then(Value::as_str) == Some(root_key_id.as_str())
            })
        })
        .ok_or(crate::ImError::PermissionDenied)?;
    let root_public =
        crate::internal::identity_wire::document::extract_identity_public_key(root_method)?;
    if !matches!(root_public, anp::PublicKeyMaterial::Ed25519(_)) {
        return Err(crate::ImError::PermissionDenied);
    }

    let identity_store =
        crate::internal::identity_store::IdentityStore::new(&core.inner().sdk_paths().identities);
    let index = identity_store.load_index()?;
    if let Some(marker) = crate::internal::identity_transition_pending::load_joined_device(
        &core.inner().sdk_paths().local_state.sqlite_path,
        &stored.join_request.join_session_id,
    )? {
        if marker.current_did != did.as_str() || marker.account_user_id != access.user_id {
            return Err(crate::ImError::Service {
                status_code: None,
                code: Some("handle_recovery_transition_mismatch".to_owned()),
                message: "handle_recovery_transition_mismatch".to_owned(),
                data: None,
            });
        }
        let matches = index
            .credentials
            .iter()
            .filter(|(_, entry)| entry.unique_id == marker.owner_identity_id)
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            return Err(crate::ImError::PermissionDenied);
        }
        let (local_alias, previous_entry) = matches[0];
        if marker.phase != crate::internal::identity_transition_pending::TransitionPhase::Pending {
            if previous_entry.did != marker.current_did
                || previous_entry.user_id != marker.account_user_id
                || previous_entry.binding_generation.as_deref()
                    != Some(marker.binding_generation.as_str())
            {
                return Err(crate::ImError::Service {
                    status_code: None,
                    code: Some("handle_recovery_transition_mismatch".to_owned()),
                    message: "handle_recovery_transition_mismatch".to_owned(),
                    data: None,
                });
            }
            if marker.phase
                == crate::internal::identity_transition_pending::TransitionPhase::IdentitySwitched
            {
                crate::internal::identity_transition_pending::mark_applied(
                    &core.inner().sdk_paths().local_state.sqlite_path,
                    &marker.recovery_id,
                    crate::internal::identity_transition_pending::TransitionPhase::IdentitySwitched,
                    pending.authorization.device.device_id.as_str(),
                    &pending.authorization.device.auth_generation.to_string(),
                    &pending
                        .authorization
                        .checkpoint
                        .registry_version
                        .to_string(),
                    "{}",
                )?;
            }
            return ensure_existing_join_identity_is_rootless(
                &index,
                local_alias,
                &did,
                &pending.authorization,
            );
        }
        let identity_already_switched = previous_entry.did == marker.current_did
            && previous_entry.user_id == marker.account_user_id
            && previous_entry.full_handle == marker.handle
            && previous_entry.binding_generation.as_deref()
                == Some(marker.binding_generation.as_str());
        let historical_binding_generation_missing =
            previous_entry.binding_generation.as_deref().is_none();
        let historical_binding_generation_matches = previous_entry
            .binding_generation
            .as_deref()
            .and_then(|value| value.parse::<u128>().ok())
            .and_then(|previous| previous.checked_add(1))
            == marker.binding_generation.parse::<u128>().ok();
        if identity_already_switched {
            ensure_existing_join_identity_is_rootless(
                &index,
                local_alias,
                &did,
                &pending.authorization,
            )?;
        } else if previous_entry.did != marker.previous_did
            || previous_entry.user_id != marker.account_user_id
            || previous_entry.full_handle != marker.handle
            || (!historical_binding_generation_missing && !historical_binding_generation_matches)
        {
            return Err(crate::ImError::Service {
                status_code: None,
                code: Some("handle_recovery_transition_mismatch".to_owned()),
                message: "handle_recovery_transition_mismatch".to_owned(),
                data: None,
            });
        }
        if historical_binding_generation_missing {
            crate::internal::identity_transition_pending::migrate_joined_device_local_state_without_historical_binding(
                &core.inner().sdk_paths().local_state.sqlite_path,
                &marker,
                &pending.authorization.device.device_id,
                pending.authorization.device.auth_generation,
            )?;
        } else {
            crate::internal::identity_transition_pending::migrate_local_state(
                &core.inner().sdk_paths().local_state.sqlite_path,
                &marker,
                &pending.authorization.device.device_id,
                pending.authorization.device.auth_generation,
            )?;
        }
        if !identity_already_switched {
            let handle =
                crate::internal::identity_wire::handle_recovery::canonical_handle(&marker.handle)?;
            let secret_storage =
                crate::internal::identity_store::SaveIdentitySecretStorage::from_core(core)?;
            identity_store.save_identity_with_secret_storage(
                crate::internal::identity_store::SaveIdentityInput {
                    local_alias: (*local_alias).clone(),
                    did: did.clone(),
                    unique_id: marker.owner_identity_id.clone(),
                    user_id: access.user_id.clone(),
                    display_name: previous_entry.name.clone(),
                    handle: handle.local_part,
                    full_handle: handle.full,
                    binding_generation: Some(marker.binding_generation.clone()),
                    jwt_token: access.access_token.clone(),
                    did_document: Some(pending.resolved_document.clone()),
                    key_mode: crate::internal::identity_store::SaveIdentityKeyMode::VNext {
                        root_key_id,
                        device_signing_key_id: pending.authorization.device.signing_key_id.clone(),
                        device_e2ee_key_id: pending.authorization.device.e2ee_key_id.clone(),
                    },
                    device_state: Some(IdentityDeviceState {
                        schema_version: IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
                        mode: IdentityDeviceMode::VNext,
                        authorization: Some(DeviceAuthorizationProjection {
                            protocol_device_id: crate::ids::ProtocolDeviceId::parse(
                                &pending.authorization.device.device_id,
                            )?,
                            signing_key_id: pending.authorization.device.signing_key_id.clone(),
                            e2ee_key_id: pending.authorization.device.e2ee_key_id.clone(),
                            status: pending.authorization.device.status,
                            role: pending.authorization.device.role,
                            management_ready: false,
                            auth_generation: pending.authorization.device.auth_generation,
                        }),
                        checkpoint: Some(pending.authorization.checkpoint.clone()),
                    }),
                    key1_private_pem: String::new(),
                    key1_public_pem: root_public.to_pem(),
                    e2ee_signing_private_pem: signing_private.to_pem(),
                    e2ee_agreement_private_pem: e2ee_private.to_pem(),
                    daemon_subkey_package: None,
                    make_default: previous_entry.is_default,
                },
                secret_storage,
            )?;
            #[cfg(test)]
            if FAIL_AFTER_RECOVERY_JOIN_IDENTITY_SAVE
                .swap(false, std::sync::atomic::Ordering::SeqCst)
            {
                return Err(crate::ImError::Internal {
                    message: "injected crash after recovery Join identity save".to_owned(),
                });
            }
        }
        crate::internal::identity_transition_pending::update_phase(
            &core.inner().sdk_paths().local_state.sqlite_path,
            &marker.recovery_id,
            crate::internal::identity_transition_pending::TransitionPhase::Pending,
            crate::internal::identity_transition_pending::TransitionPhase::IdentitySwitched,
        )?;
        crate::internal::identity_transition_pending::mark_applied(
            &core.inner().sdk_paths().local_state.sqlite_path,
            &marker.recovery_id,
            crate::internal::identity_transition_pending::TransitionPhase::IdentitySwitched,
            pending.authorization.device.device_id.as_str(),
            &pending.authorization.device.auth_generation.to_string(),
            &pending
                .authorization
                .checkpoint
                .registry_version
                .to_string(),
            "{}",
        )?;
        // Group repair has its own durable journal and retry lifecycle. A
        // queueing failure must not roll back an already-applied identity.
        let _ = crate::internal::group_rebind_recovery::enqueue_recovery_jobs(
            &core.inner().sdk_paths().local_state.sqlite_path,
            &marker.owner_identity_id,
            &marker.handle,
            std::slice::from_ref(&marker.previous_did),
            &marker.current_did,
            &marker.binding_generation,
        );
        let committed = identity_store.load_index()?;
        return ensure_existing_join_identity_is_rootless(
            &committed,
            local_alias,
            &did,
            &pending.authorization,
        );
    }
    let (local_alias, handle, full_handle, make_default) =
        join_local_identity_projection(&did, &stored.join_request.device_id, &index)?;
    ensure_existing_join_identity_is_rootless(&index, &local_alias, &did, &pending.authorization)?;
    let unique_id =
        crate::internal::identity_join_activation_pending::identity_suffix(&pending.did);
    let secret_storage =
        crate::internal::identity_store::SaveIdentitySecretStorage::from_core(core)?;
    identity_store.save_identity_with_secret_storage(
        crate::internal::identity_store::SaveIdentityInput {
            local_alias: local_alias.clone(),
            did: did.clone(),
            unique_id,
            user_id: access.user_id.clone(),
            display_name: handle.clone(),
            handle,
            full_handle,
            binding_generation: None,
            jwt_token: access.access_token.clone(),
            did_document: Some(pending.resolved_document.clone()),
            key_mode: crate::internal::identity_store::SaveIdentityKeyMode::VNext {
                root_key_id,
                device_signing_key_id: pending.authorization.device.signing_key_id.clone(),
                device_e2ee_key_id: pending.authorization.device.e2ee_key_id.clone(),
            },
            device_state: Some(IdentityDeviceState {
                schema_version: IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
                mode: IdentityDeviceMode::VNext,
                authorization: Some(DeviceAuthorizationProjection {
                    protocol_device_id: crate::ids::ProtocolDeviceId::parse(
                        &pending.authorization.device.device_id,
                    )?,
                    signing_key_id: pending.authorization.device.signing_key_id.clone(),
                    e2ee_key_id: pending.authorization.device.e2ee_key_id.clone(),
                    status: pending.authorization.device.status,
                    role: pending.authorization.device.role,
                    management_ready: false,
                    auth_generation: pending.authorization.device.auth_generation,
                }),
                checkpoint: Some(pending.authorization.checkpoint.clone()),
            }),
            key1_private_pem: String::new(),
            key1_public_pem: root_public.to_pem(),
            e2ee_signing_private_pem: signing_private.to_pem(),
            e2ee_agreement_private_pem: e2ee_private.to_pem(),
            daemon_subkey_package: None,
            make_default,
        },
        secret_storage.clone(),
    )?;
    let committed = identity_store.load_index()?;
    ensure_existing_join_identity_is_rootless(
        &committed,
        &local_alias,
        &did,
        &pending.authorization,
    )
}

#[cfg(test)]
static FAIL_AFTER_RECOVERY_JOIN_IDENTITY_SAVE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

fn join_local_identity_projection(
    did: &crate::ids::Did,
    device_id: &str,
    index: &crate::internal::identity_store::IndexPayload,
) -> crate::ImResult<(String, String, String, bool)> {
    let existing = index
        .credentials
        .iter()
        .filter(|(_, entry)| entry.did == did.as_str())
        .collect::<Vec<_>>();
    if existing.len() > 1 {
        return Err(crate::ImError::PermissionDenied);
    }
    let domain = crate::internal::identity_join_activation_pending::service_domain_from_did(did)?;
    let rest = did
        .as_str()
        .strip_prefix(&format!("did:wba:{domain}:"))
        .ok_or(crate::ImError::PermissionDenied)?;
    let components = rest.split(':').collect::<Vec<_>>();
    if components.len() != 3
        || components[0] != "user"
        || components[1].trim().is_empty()
        || !components[2].starts_with("e1_")
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let handle = components[1].to_ascii_lowercase();
    let full_handle = format!("{handle}.{domain}");
    if let Some((alias, _)) = existing.first() {
        return Ok(((*alias).clone(), handle, full_handle, false));
    }
    let mut local_alias = handle.clone();
    if index.credentials.contains_key(&local_alias) {
        let suffix = device_id
            .strip_prefix("dev-")
            .unwrap_or(device_id)
            .chars()
            .take(10)
            .collect::<String>();
        local_alias = format!("{handle}-{suffix}").to_ascii_lowercase();
        if index.credentials.contains_key(&local_alias) {
            return Err(crate::ImError::PermissionDenied);
        }
    }
    Ok((
        local_alias,
        handle,
        full_handle,
        index.credentials.is_empty(),
    ))
}

fn ensure_existing_join_identity_is_rootless(
    index: &crate::internal::identity_store::IndexPayload,
    local_alias: &str,
    did: &crate::ids::Did,
    authorization: &crate::internal::identity_device_join_runtime::DeviceJoinRemoteAuthorization,
) -> crate::ImResult<()> {
    let Some(entry) = index.credentials.get(local_alias) else {
        return Ok(());
    };
    let state = entry
        .device_state
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    let projection = state
        .authorization
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    let root_absent = entry
        .vault_migration
        .as_ref()
        .and_then(|metadata| metadata.vnext_refs.as_ref())
        .is_some_and(|refs| refs.did_document_root_private.is_none());
    if entry.did != did.as_str()
        || state.mode != crate::internal::identity_device_state::IdentityDeviceMode::VNext
        || !root_absent
        || projection.protocol_device_id.as_str() != authorization.device.device_id
        || projection.signing_key_id != authorization.device.signing_key_id
        || projection.e2ee_key_id != authorization.device.e2ee_key_id
        || projection.status != authorization.device.status
        || projection.role != authorization.device.role
        || projection.management_ready
        || projection.auth_generation != authorization.device.auth_generation
        || state.checkpoint.as_ref() != Some(&authorization.checkpoint)
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

pub(crate) fn cancel_join(
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
            .ok_or(crate::ImError::IdentityNotFound {
                selector: join_session_id,
            })?;
    if stored.phase == DeviceJoinLocalPhase::Authorized || stored.activation_pending {
        return Err(invalid_state("authorized Join cannot be cancelled"));
    }
    if stored.phase != DeviceJoinLocalPhase::Cancelled {
        stored.phase = DeviceJoinLocalPhase::Cancelled;
        store.save(&stored)?;
    }
    cleanup_cancelled_join_secrets(core, &stored)?;
    summary(&stored)
}

pub(crate) fn mark_join_expired(
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
            .ok_or(crate::ImError::IdentityNotFound {
                selector: join_session_id,
            })?;
    if stored.phase == DeviceJoinLocalPhase::Authorized || stored.activation_pending {
        return Err(invalid_state("authorized Join cannot expire"));
    }
    if stored.phase != DeviceJoinLocalPhase::Expired {
        stored.phase = DeviceJoinLocalPhase::Expired;
        store.save(&stored)?;
    }
    cleanup_cancelled_join_secrets(core, &stored)?;
    summary(&stored)
}

pub(crate) fn session(
    core: &crate::core::ImCore,
    join_session_id: &str,
    side: DeviceJoinSide,
) -> crate::ImResult<DeviceJoinSessionSummary> {
    let _guard = lock_join_state(core)?;
    let join_session_id = required("join_session_id", join_session_id)?;
    let store = JoinStateStore::new(core);
    let stored = store
        .load(&join_session_id, side)?
        .ok_or(crate::ImError::IdentityNotFound {
            selector: join_session_id,
        })?;
    summary(&stored)
}

pub(crate) fn local_new_device_verification_sas(
    core: &crate::core::ImCore,
    join_session_id: &str,
) -> crate::ImResult<String> {
    let _guard = lock_join_state(core)?;
    let join_session_id = required("join_session_id", join_session_id)?;
    let store = JoinStateStore::new(core);
    let stored = store
        .load(&join_session_id, DeviceJoinSide::NewDevice)?
        .ok_or_else(|| crate::ImError::IdentityNotFound {
            selector: join_session_id.clone(),
        })?;
    ensure_not_expired(&stored)?;
    if stored.phase != DeviceJoinLocalPhase::ResponsePrepared {
        return Err(invalid_state(
            "new-device Join response is not ready for SAS display",
        ));
    }
    derive_stored_sas(core, &stored)
}

pub(crate) fn list_sessions(
    core: &crate::core::ImCore,
) -> crate::ImResult<Vec<DeviceJoinSessionSummary>> {
    let _guard = lock_join_state(core)?;
    let store = JoinStateStore::new(core);
    let mut sessions = Vec::new();
    for stored in store.list()? {
        sessions.push(summary(&stored)?);
    }
    sessions.sort_by(|left, right| {
        left.expires_at
            .cmp(&right.expires_at)
            .then_with(|| left.join_session_id.cmp(&right.join_session_id))
    });
    Ok(sessions)
}

pub(crate) fn retire_authorized_new_device_sessions(
    core: &crate::core::ImCore,
    did: &str,
    protocol_device_id: &str,
) -> crate::ImResult<()> {
    let _guard = lock_join_state(core)?;
    let did = crate::ids::Did::parse(did)?;
    let protocol_device_id = crate::ids::ProtocolDeviceId::parse(protocol_device_id)?;
    let state_store = JoinStateStore::new(core);
    let matching = state_store
        .list()?
        .into_iter()
        .filter(|stored| {
            stored.side == DeviceJoinSide::NewDevice
                && stored.phase == DeviceJoinLocalPhase::Authorized
                && stored.join_request.did == did.as_str()
                && stored.join_request.device_id == protocol_device_id.as_str()
        })
        .collect::<Vec<_>>();

    for stored in matching {
        cleanup_cancelled_join_secrets(core, &stored)?;
        if stored.activation_pending {
            let pending_store = crate::internal::identity_join_activation_pending::PendingJoinActivationStore::from_core(core)?;
            if let Some((secret_ref, _)) =
                pending_store.load(&stored.join_request.join_session_id, &did)?
            {
                pending_store.delete(&secret_ref)?;
            }
        }
        state_store.delete(
            &stored.join_request.join_session_id,
            DeviceJoinSide::NewDevice,
        )?;
    }
    Ok(())
}

pub(crate) fn admin_approval_context(
    core: &crate::core::ImCore,
    join_session_id: &str,
) -> crate::ImResult<(
    DeviceJoinSessionSummary,
    String,
    Option<PreparedAdminApproval>,
)> {
    let _guard = lock_join_state(core)?;
    let join_session_id = required("join_session_id", join_session_id)?;
    let store = JoinStateStore::new(core);
    let stored = store
        .load(&join_session_id, DeviceJoinSide::Admin)?
        .ok_or_else(|| crate::ImError::IdentityNotFound {
            selector: join_session_id.clone(),
        })?;
    ensure_not_expired(&stored)?;
    if !matches!(
        stored.phase,
        DeviceJoinLocalPhase::ResponseVerified | DeviceJoinLocalPhase::ApprovalPrepared
    ) {
        return Err(invalid_state("admin Join response is not verified"));
    }
    let approval = stored
        .approval
        .as_ref()
        .map(|value| prepared_approval_result(&stored, value))
        .transpose()?;
    Ok((
        summary(&stored)?,
        derive_stored_sas(core, &stored)?,
        approval,
    ))
}

pub(crate) fn local_admin_verification_progress(
    core: &crate::core::ImCore,
    admin_identity: &crate::identity::IdentitySelector,
    join_session_id: &str,
) -> crate::ImResult<(DeviceJoinSessionSummary, String)> {
    let _guard = lock_join_state(core)?;
    let join_session_id = required("join_session_id", join_session_id)?;
    let store = JoinStateStore::new(core);
    let stored = store
        .load(&join_session_id, DeviceJoinSide::Admin)?
        .ok_or_else(|| crate::ImError::IdentityNotFound {
            selector: join_session_id.clone(),
        })?;
    ensure_not_expired(&stored)?;
    if !matches!(
        stored.phase,
        DeviceJoinLocalPhase::ResponseVerified | DeviceJoinLocalPhase::ApprovalPrepared
    ) {
        return Err(invalid_state(
            "admin Join verification progress is not available",
        ));
    }

    // This is a local read, but it still has to bind the caller-selected
    // identity to the exact Vault owner that created this admin session.
    let (client, _, _) =
        ready_admin_context(core, admin_identity, Some(stored.join_request.did.as_str()))?;
    if stored.pairing_private_ref.identity_id.as_deref()
        != Some(client.current_identity().id.as_str())
        || stored.pairing_private_ref.did.as_deref() != Some(stored.join_request.did.as_str())
    {
        return Err(crate::ImError::PermissionDenied);
    }

    Ok((summary(&stored)?, derive_stored_sas(core, &stored)?))
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

fn prepared_approval_result(
    stored: &StoredJoinSession,
    approval: &StoredAdminApproval,
) -> crate::ImResult<PreparedAdminApproval> {
    Ok(PreparedAdminApproval {
        operation_id: approval.operation_id.clone(),
        join_session_id: stored.join_request.join_session_id.clone(),
        expected_checkpoint: approval.expected_checkpoint.clone(),
        new_document: approval.new_document.clone(),
        pairing_confirmation: approval.pairing_confirmation.clone(),
        authorizing_device_id: approval.authorizing_device_id.clone(),
        proof: approval.proof.clone(),
    })
}

pub(crate) fn ready_admin_context(
    core: &crate::core::ImCore,
    admin_identity: &crate::identity::IdentitySelector,
    expected_did: Option<&str>,
) -> crate::ImResult<(crate::core::ImClient, String, String)> {
    let client = core.client(admin_identity.clone())?;
    if expected_did.is_some_and(|did| client.did().as_str() != did) {
        return Err(crate::ImError::PermissionDenied);
    }
    let summary = core.identities().device_summary(admin_identity.clone())?;
    if summary.mode != crate::identity::IdentityDeviceMode::VNext
        || summary.role != Some(crate::identity::IdentityDeviceRole::Admin)
        || summary.readiness != crate::identity::IdentityDeviceReadiness::AdminReady
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let device_id = summary
        .protocol_device_id
        .ok_or_else(|| invalid_state("ready admin is missing protocol device id"))?
        .as_str()
        .to_owned();
    let signing_key_id = summary
        .signing_key_id
        .ok_or_else(|| invalid_state("ready admin is missing signing key id"))?;
    Ok((client, device_id, signing_key_id))
}

fn validate_remote_authorization(
    stored: &StoredJoinSession,
    authorization: &crate::internal::identity_device_join_runtime::DeviceJoinRemoteAuthorization,
    resolved_document: &Value,
) -> crate::ImResult<()> {
    if authorization.device.role
        != crate::internal::identity_device_state::DeviceAuthorizationRole::Member
        || authorization.device.management_ready
    {
        return Err(crate::ImError::PermissionDenied);
    }
    use crate::internal::identity_device_state::DeviceAuthorizationStatus;

    let signing_key_id = method_id(
        &stored.join_request.signing_public_key,
        "join_request.signing_public_key",
    )?;
    let e2ee_key_id = method_id(
        &stored.join_request.e2ee_public_key,
        "join_request.e2ee_public_key",
    )?;
    if authorization.device.device_id != stored.join_request.device_id
        || authorization.device.signing_key_id != signing_key_id
        || authorization.device.e2ee_key_id != e2ee_key_id
        || authorization.device.status != DeviceAuthorizationStatus::Active
        || authorization.device.management_ready
        || authorization.device.auth_generation == 0
        || authorization.checkpoint.document_version == 0
        || authorization.checkpoint.registry_version == 0
    {
        return Err(crate::ImError::PermissionDenied);
    }
    if let Some(approval) = stored.approval.as_ref() {
        if &approval.new_document != resolved_document {
            return Err(crate::ImError::PermissionDenied);
        }
    }
    if canonical_hash(resolved_document)? != authorization.checkpoint.document_hash {
        return Err(crate::ImError::PermissionDenied);
    }
    validate_authorized_document(&stored.join_request, resolved_document)
}

fn validate_authorized_document(
    join_request: &DeviceJoinRequest,
    did_document: &Value,
) -> crate::ImResult<()> {
    if did_document.get("id").and_then(Value::as_str) != Some(join_request.did.as_str())
        || !anp::authentication::validate_did_document_binding(did_document, true)
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let manifest = anp::authentication::validate_device_manifest(did_document)
        .map_err(|_| crate::ImError::PermissionDenied)?
        .ok_or(crate::ImError::PermissionDenied)?;
    let signing_key_id = method_id(
        &join_request.signing_public_key,
        "join_request.signing_public_key",
    )?;
    let e2ee_key_id = method_id(
        &join_request.e2ee_public_key,
        "join_request.e2ee_public_key",
    )?;
    let entry = manifest
        .devices
        .iter()
        .find(|entry| entry.device_id == join_request.device_id)
        .ok_or(crate::ImError::PermissionDenied)?;
    if entry.signing_key_id != signing_key_id
        || entry.e2ee_key_id != e2ee_key_id
        || entry.profiles
            != DEVICE_JOIN_VNEXT_PROFILES
                .iter()
                .map(|value| (*value).to_owned())
                .collect::<Vec<_>>()
        || did_document
            .get("verificationMethod")
            .and_then(Value::as_array)
            .and_then(|methods| {
                methods
                    .iter()
                    .find(|method| method.get("id").and_then(Value::as_str) == Some(signing_key_id))
            })
            != Some(&join_request.signing_public_key)
        || did_document
            .get("verificationMethod")
            .and_then(Value::as_array)
            .and_then(|methods| {
                methods
                    .iter()
                    .find(|method| method.get("id").and_then(Value::as_str) == Some(e2ee_key_id))
            })
            != Some(&join_request.e2ee_public_key)
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

fn cleanup_consumed_join_secrets(
    core: &crate::core::ImCore,
    stored: &StoredJoinSession,
) -> crate::ImResult<()> {
    let vault = required_vault(core)?;
    let mut refs = vec![&stored.pairing_private_ref];
    if let Some(secret_ref) = stored.join_session_token_ref.as_ref() {
        refs.push(secret_ref);
    }
    delete_secret_refs(&*vault, refs)
}

fn cleanup_cancelled_join_secrets(
    core: &crate::core::ImCore,
    stored: &StoredJoinSession,
) -> crate::ImResult<()> {
    let vault = required_vault(core)?;
    let mut refs = vec![&stored.pairing_private_ref];
    if let Some(secret_ref) = stored.join_session_token_ref.as_ref() {
        refs.push(secret_ref);
    }
    if stored.side == DeviceJoinSide::NewDevice {
        if let Some(secret_ref) = stored.signing_private_ref.as_ref() {
            refs.push(secret_ref);
        }
        if let Some(secret_ref) = stored.e2ee_private_ref.as_ref() {
            refs.push(secret_ref);
        }
    }
    delete_secret_refs(&*vault, refs)
}

fn delete_secret_refs(vault: &dyn SecretVault, refs: Vec<&SecretRef>) -> crate::ImResult<()> {
    let mut first_error = None;
    for secret_ref in refs {
        if let Err(error) = vault.delete(secret_ref) {
            if first_error.is_none() {
                first_error = Some(error);
            }
        }
    }
    first_error.map_or(Ok(()), Err)
}

struct JoinStateMutationLock<'a> {
    _process_guard: std::sync::MutexGuard<'a, ()>,
    file: fs::File,
}

impl Drop for JoinStateMutationLock<'_> {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.file);
    }
}

fn lock_join_state(core: &crate::core::ImCore) -> crate::ImResult<JoinStateMutationLock<'_>> {
    let process_guard = core.inner().device_join_lock.lock().map_err(|_| {
        crate::ImError::LocalStateUnavailable {
            detail: "device Join state lock poisoned".to_owned(),
        }
    })?;
    let dir = JoinStateStore::new(core).dir();
    fs::create_dir_all(&dir)?;
    set_private_dir_mode(&dir)?;
    let path = dir.join(JOIN_STATE_LOCK_FILE);
    let mut options = fs::OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let file = options.open(&path)?;
    set_private_file_mode(&path)?;
    file.lock_exclusive().map_err(crate::ImError::from)?;
    Ok(JoinStateMutationLock {
        _process_guard: process_guard,
        file,
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
    let valid = matches!(
        (&expected_kind, &key),
        (
            SecretKind::IdentityDeviceSigningPrivate,
            anp::PrivateKeyMaterial::Ed25519(_)
        ) | (
            SecretKind::IdentityE2eeAgreementPrivate | SecretKind::IdentityJoinPairingPrivate,
            anp::PrivateKeyMaterial::X25519(_),
        )
    );
    if !valid {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(key)
}

fn sign_join_request(
    request: &DeviceJoinRequest,
    private_key: &anp::PrivateKeyMaterial,
) -> crate::ImResult<String> {
    let content = join_request_proof_input_bytes(request)?;
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
    if !join_profiles_are_supported(&request.profiles) {
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
    let method = create_join_verification_method(&request.signing_public_key)?;
    if request.join_request_proof.proof_type != DEVICE_JOIN_REQUEST_PROOF_TYPE
        || request.join_request_proof.algorithm != JOIN_PROOF_ALGORITHM
        || request.join_request_proof.verification_method
            != method_id(&request.signing_public_key, "signing_public_key")?
        || request.join_request_proof.created_at != request.issued_at
    {
        return Err(crate::ImError::PermissionDenied);
    }
    method
        .verify_signature(
            &join_request_proof_input_bytes(request)?,
            &request.join_request_proof.proof_value_b64u,
        )
        .map_err(|_| crate::ImError::PermissionDenied)
}

fn join_profiles_are_supported(profiles: &[String]) -> bool {
    profiles == DEVICE_JOIN_VNEXT_PROFILES || profiles == DEVICE_JOIN_LEGACY_DRAFT_PROFILES
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
    let material = extract_identity_public_key(method)?;
    if signing && !matches!(material, anp::PublicKeyMaterial::Ed25519(_)) {
        return Err(crate::ImError::PermissionDenied);
    }
    if !signing && !matches!(material, anp::PublicKeyMaterial::X25519(_)) {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

fn unsigned_join_request_value(request: &DeviceJoinRequest) -> crate::ImResult<Value> {
    let mut value = serde_json::to_value(request).map_err(|err| crate::ImError::Serialization {
        detail: err.to_string(),
    })?;
    value
        .as_object_mut()
        .ok_or_else(|| invalid_state("Join Request serialization is not an object"))?
        .remove("join_request_proof");
    Ok(value)
}

fn join_request_proof_input_bytes(request: &DeviceJoinRequest) -> crate::ImResult<Vec<u8>> {
    canonical_bytes(&json!({
        "type": DEVICE_JOIN_REQUEST_PROOF_INPUT_TYPE,
        "join_request": unsigned_join_request_value(request)?,
        "proof_options": {
            "type": request.join_request_proof.proof_type,
            "algorithm": request.join_request_proof.algorithm,
            "verification_method": request.join_request_proof.verification_method,
            "created_at": request.join_request_proof.created_at,
        }
    }))
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
        "type": DEVICE_JOIN_RESPONSE_SIGNATURE_INPUT_TYPE,
        "operation_id": operation_id,
        "join_session_id": join_session_id,
        "challenge_id": challenge_id,
        "challenge_hash": challenge_hash,
        "join_request_hash": join_request_hash,
        "pairing_transcript_hash": pairing_transcript_hash,
    })
}

fn sign_object_proof(
    private_key: &anp::PrivateKeyMaterial,
    key_id: &str,
    params: &Value,
    created_at: &str,
) -> crate::ImResult<DeviceJoinObjectProof> {
    let issuer_did = key_id
        .split_once('#')
        .map(|(did, _)| did)
        .ok_or(crate::ImError::PermissionDenied)?;
    let signed = anp::proof::generate_object_proof(
        params,
        private_key,
        key_id,
        issuer_did,
        Some(created_at.to_owned()),
    )
    .map_err(|_| crate::ImError::PermissionDenied)?;
    serde_json::from_value(
        signed
            .get("proof")
            .cloned()
            .ok_or(crate::ImError::PermissionDenied)?,
    )
    .map_err(|_| crate::ImError::PermissionDenied)
}

fn verify_object_proof(
    proof: &DeviceJoinObjectProof,
    params: &Value,
    issuer_did: &str,
    issuer_document: &Value,
    expected_method: &Value,
) -> crate::ImResult<()> {
    if method_id(expected_method, "proof.public_method")? != proof.verification_method {
        return Err(crate::ImError::PermissionDenied);
    }
    let mut signed = params.clone();
    signed
        .as_object_mut()
        .ok_or(crate::ImError::PermissionDenied)?
        .insert(
            "proof".to_owned(),
            serde_json::to_value(proof).map_err(|_| crate::ImError::PermissionDenied)?,
        );
    anp::proof::verify_object_proof(&signed, issuer_did, issuer_document)
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
) -> crate::ImResult<DecryptedJoinChallenge> {
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
    parse_challenge_plaintext(SecretBytes::from_vec(plaintext))
}

fn encode_challenge_plaintext(
    random_challenge: &[u8; JOIN_CHALLENGE_LEN],
    checkpoint: &InternalCheckpoint,
) -> crate::ImResult<Zeroizing<Vec<u8>>> {
    let plaintext = JoinChallengePlaintext {
        plaintext_type: JOIN_CHALLENGE_PLAINTEXT_TYPE.to_owned(),
        random_challenge_b64u: URL_SAFE_NO_PAD.encode(random_challenge),
        document_version: checkpoint.document_version,
        document_hash: checkpoint.document_hash.clone(),
    };
    serde_json_canonicalizer::to_vec(&plaintext)
        .map(Zeroizing::new)
        .map_err(|err| crate::ImError::Serialization {
            detail: err.to_string(),
        })
}

fn parse_challenge_plaintext(
    canonical_plaintext: SecretBytes,
) -> crate::ImResult<DecryptedJoinChallenge> {
    let plaintext: JoinChallengePlaintext =
        serde_json::from_slice(canonical_plaintext.expose_secret())
            .map_err(|_| crate::ImError::PermissionDenied)?;
    if plaintext.plaintext_type != JOIN_CHALLENGE_PLAINTEXT_TYPE {
        return Err(crate::ImError::PermissionDenied);
    }
    let canonical = Zeroizing::new(
        serde_json_canonicalizer::to_vec(&plaintext)
            .map_err(|_| crate::ImError::PermissionDenied)?,
    );
    if canonical.as_slice() != canonical_plaintext.expose_secret() {
        return Err(crate::ImError::PermissionDenied);
    }
    let mut random_challenge = URL_SAFE_NO_PAD
        .decode(plaintext.random_challenge_b64u.as_bytes())
        .map_err(|_| crate::ImError::PermissionDenied)?;
    let canonical_random_challenge = Zeroizing::new(URL_SAFE_NO_PAD.encode(&random_challenge));
    let valid_random_challenge = random_challenge.len() == JOIN_CHALLENGE_LEN
        && canonical_random_challenge.as_str() == plaintext.random_challenge_b64u;
    random_challenge.zeroize();
    if !valid_random_challenge {
        return Err(crate::ImError::PermissionDenied);
    }
    let checkpoint = validate_checkpoint(plaintext.document_version, &plaintext.document_hash)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    Ok(DecryptedJoinChallenge {
        canonical_plaintext,
        checkpoint,
    })
}

fn ensure_challenge_checkpoint(
    challenge_checkpoint: &InternalCheckpoint,
    resolved_checkpoint: &InternalCheckpoint,
) -> crate::ImResult<()> {
    if challenge_checkpoint != resolved_checkpoint {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
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
    match extract_identity_public_key(method)? {
        anp::PublicKeyMaterial::X25519(value) => Ok(value),
        _ => Err(crate::ImError::PermissionDenied),
    }
}

fn create_join_verification_method(
    method: &Value,
) -> crate::ImResult<anp::authentication::VerificationMethod> {
    let normalized = normalize_identity_okp_method(method)?;
    anp::authentication::create_verification_method(&normalized)
        .map_err(|_| crate::ImError::PermissionDenied)
}

fn extract_identity_public_key(method: &Value) -> crate::ImResult<anp::PublicKeyMaterial> {
    crate::internal::identity_wire::document::extract_identity_public_key(method)
}

fn normalize_identity_okp_method(method: &Value) -> crate::ImResult<Value> {
    crate::internal::identity_wire::document::normalize_identity_okp_method(method)
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
    public_key: &anp::PublicKeyMaterial,
) -> crate::ImResult<Value> {
    let (crv, x) = match public_key {
        anp::PublicKeyMaterial::Ed25519(key) => ("Ed25519", URL_SAFE_NO_PAD.encode(key.to_bytes())),
        anp::PublicKeyMaterial::X25519(key) => ("X25519", URL_SAFE_NO_PAD.encode(key)),
        _ => return Err(crate::ImError::PermissionDenied),
    };
    Ok(json!({
        "id": key_id,
        "type": "JsonWebKey2020",
        "controller": did,
        "publicKeyJwk": {
            "kty": "OKP",
            "crv": crv,
            "x": x,
        },
    }))
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

fn ensure_not_expired(stored: &StoredJoinSession) -> crate::ImResult<()> {
    match stored.phase {
        DeviceJoinLocalPhase::Expired => return Err(crate::ImError::SessionExpired),
        DeviceJoinLocalPhase::Cancelled => return Err(invalid_state("Join is cancelled")),
        DeviceJoinLocalPhase::Authorized if !stored.activation_pending => return Ok(()),
        _ if stored.activation_pending => return Ok(()),
        _ => {}
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
    if effective_expires_at <= OffsetDateTime::now_utc() {
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

fn is_false(value: &bool) -> bool {
    !*value
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
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let raw = fs::read(path)?;
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

    fn delete(&self, join_session_id: &str, side: DeviceJoinSide) -> crate::ImResult<()> {
        let path = self.path(join_session_id, side);
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(crate::ImError::Io {
                detail: format!("delete device Join state {}: {error}", path.display()),
            }),
        }
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
                let long_term_refs_present =
                    stored.signing_private_ref.as_ref().is_some_and(|value| {
                        value.kind == SecretKind::IdentityDeviceSigningPrivate
                    }) && stored.e2ee_private_ref.as_ref().is_some_and(|value| {
                        value.kind == SecretKind::IdentityE2eeAgreementPrivate
                    });
                let long_term_refs_cleaned = stored.phase == DeviceJoinLocalPhase::Authorized
                    && !stored.activation_pending
                    && stored.signing_private_ref.is_none()
                    && stored.e2ee_private_ref.is_none();
                if (!long_term_refs_present && !long_term_refs_cleaned)
                    || stored.admin_identity.is_some()
                    || stored.approval.is_some()
                    || (stored.activation_pending
                        && !matches!(
                            stored.phase,
                            DeviceJoinLocalPhase::ResponsePrepared
                                | DeviceJoinLocalPhase::Authorized
                        ))
                    || stored
                        .join_session_token_ref
                        .as_ref()
                        .is_some_and(|value| value.kind != SecretKind::IdentityJoinSessionToken)
                {
                    return Err(invalid_state("new-device secret reference mismatch"));
                }
            }
            DeviceJoinSide::Admin => {
                if stored.signing_private_ref.is_some()
                    || stored.e2ee_private_ref.is_some()
                    || stored.activation_pending
                    || stored.admin_identity.is_none()
                    || stored.join_session_token_ref.is_some()
                    || (stored.approval.is_some()
                        && !matches!(
                            stored.phase,
                            DeviceJoinLocalPhase::ApprovalPrepared
                                | DeviceJoinLocalPhase::Authorized
                                | DeviceJoinLocalPhase::Cancelled
                                | DeviceJoinLocalPhase::Expired
                        ))
                    || (stored.phase == DeviceJoinLocalPhase::ApprovalPrepared
                        && stored.approval.is_none())
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
