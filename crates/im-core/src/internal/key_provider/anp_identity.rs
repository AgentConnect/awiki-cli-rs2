use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use anp_identity::{DidIdentity, KeyRole, KeyState};

use crate::internal::platform_secret::SecretBytes;
use crate::internal::secret_vault::policy::SecretAccessPolicy;
use crate::internal::secret_vault::record::{SecretKind, SecretMetadata, SecretRef};
use crate::internal::secret_vault::{SealSecretRequest, SecretVault};

pub(crate) struct AnpIdentitySigner {
    identity: Mutex<DidIdentity>,
    auth: AnpIdentityAuth,
}

pub(crate) struct PendingAnpEnrollmentSigner {
    identity: DidIdentity,
    enrollment_id: String,
    document: serde_json::Value,
    signing_key_id: String,
    e2ee_key_id: String,
    auth: RwLock<crate::internal::auth::state::AuthStateSnapshot>,
}

enum AnpIdentityAuth {
    Ephemeral {
        state: RwLock<crate::internal::auth::state::AuthStateSnapshot>,
    },
    File {
        auth_state_path: PathBuf,
    },
    Vault {
        vault: Arc<dyn SecretVault + Send + Sync>,
        auth_ref: RwLock<SecretRef>,
    },
}

impl fmt::Debug for AnpIdentitySigner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AnpIdentitySigner")
            .field("identity", &"<anp-identity-handle>")
            .field("auth", &self.auth)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for AnpIdentityAuth {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ephemeral { .. } => formatter.write_str("EphemeralAuth(<memory-only>)"),
            Self::File { auth_state_path } => formatter
                .debug_struct("FileAuth")
                .field("auth_state_path", auth_state_path)
                .finish(),
            Self::Vault { .. } => formatter.write_str("VaultAuth(<redacted-secret-ref>)"),
        }
    }
}

impl AnpIdentitySigner {
    pub(crate) fn new_ephemeral(identity: DidIdentity) -> Self {
        Self {
            identity: Mutex::new(identity),
            auth: AnpIdentityAuth::Ephemeral {
                state: RwLock::new(Default::default()),
            },
        }
    }

    pub(crate) fn new_file(identity: DidIdentity, auth_state_path: PathBuf) -> Self {
        Self {
            identity: Mutex::new(identity),
            auth: AnpIdentityAuth::File { auth_state_path },
        }
    }

    pub(crate) fn new_vault(
        identity: DidIdentity,
        vault: Arc<dyn SecretVault + Send + Sync>,
        auth_ref: SecretRef,
    ) -> crate::ImResult<Self> {
        validate_auth_ref(&auth_ref)?;
        Ok(Self {
            identity: Mutex::new(identity),
            auth: AnpIdentityAuth::Vault {
                vault,
                auth_ref: RwLock::new(auth_ref),
            },
        })
    }

    pub(crate) fn reload(&self) -> crate::ImResult<()> {
        self.lock_identity()?.reload().map_err(map_identity_error)
    }

    fn lock_identity(&self) -> crate::ImResult<std::sync::MutexGuard<'_, DidIdentity>> {
        self.identity.lock().map_err(|_| crate::ImError::Internal {
            message: "anp identity handle lock poisoned".to_string(),
        })
    }

    fn active_kid(&self, roles: &[KeyRole]) -> crate::ImResult<String> {
        let identity = self.lock_identity()?;
        if identity.state() != anp_identity::IdentityState::Active {
            return Err(crate::ImError::PermissionDenied);
        }
        identity
            .keys()
            .iter()
            .find(|key| {
                roles.contains(&key.role)
                    && key.state == KeyState::Active
                    && key.origin == anp_identity::KeyOrigin::Managed
                    && !key.material_erased
            })
            .map(|key| key.kid.clone())
            .ok_or(crate::ImError::PermissionDenied)
    }

    fn auth_state_from_vault(
        vault: &Arc<dyn SecretVault + Send + Sync>,
        auth_ref: &RwLock<SecretRef>,
    ) -> crate::ImResult<crate::internal::auth::state::AuthStateSnapshot> {
        let auth_ref = auth_ref
            .read()
            .map_err(|_| crate::ImError::LocalStateUnavailable {
                detail: "anp identity auth ref lock poisoned".to_string(),
            })?
            .clone();
        validate_auth_ref(&auth_ref)?;
        let secret = vault.open(&auth_ref)?;
        crate::internal::auth::state::parse_auth_state(secret.expose_secret())
    }
}

impl PendingAnpEnrollmentSigner {
    pub(crate) fn new(
        identity: DidIdentity,
        enrollment_id: impl Into<String>,
        document: serde_json::Value,
        signing_key_id: impl Into<String>,
        e2ee_key_id: impl Into<String>,
    ) -> crate::ImResult<Self> {
        let enrollment_id = enrollment_id.into();
        let signing_key_id = signing_key_id.into();
        let e2ee_key_id = e2ee_key_id.into();
        let pending = identity
            .pending_enrollment()
            .ok_or(crate::ImError::PermissionDenied)?;
        if pending.enrollment_id != enrollment_id
            || pending.device_signing_key.kid != signing_key_id
            || pending.device_e2ee_key.kid != e2ee_key_id
            || document.get("id").and_then(serde_json::Value::as_str) != Some(identity.did())
            || !anp::authentication::is_authentication_authorized(&document, &signing_key_id)
            || !anp::authentication::is_assertion_method_authorized(&document, &signing_key_id)
        {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(Self {
            identity,
            enrollment_id,
            document,
            signing_key_id,
            e2ee_key_id,
            auth: RwLock::new(Default::default()),
        })
    }
}

impl super::IdentitySigner for PendingAnpEnrollmentSigner {
    fn did_document(&self) -> crate::ImResult<serde_json::Value> {
        Ok(self.document.clone())
    }

    fn optional_did_document(&self) -> crate::ImResult<Option<serde_json::Value>> {
        Ok(Some(self.document.clone()))
    }

    fn request_signing_key_id(&self) -> crate::ImResult<String> {
        Ok(self.signing_key_id.clone())
    }

    fn agreement_key_id(&self) -> crate::ImResult<String> {
        Ok(self.e2ee_key_id.clone())
    }

    fn sign(&self, kid: &str, message: &[u8]) -> crate::ImResult<Vec<u8>> {
        self.identity
            .sign_pending_enrollment(&self.enrollment_id, kid, message)
            .map_err(map_identity_error)
    }

    fn sign_device_assertion(&self, kid: &str, message: &[u8]) -> crate::ImResult<Vec<u8>> {
        self.sign(kid, message)
    }

    fn sign_root(&self, _kid: &str, _message: &[u8]) -> crate::ImResult<Vec<u8>> {
        Err(crate::ImError::PermissionDenied)
    }

    fn ecdh(&self, kid: &str, peer_public: &[u8]) -> crate::ImResult<zeroize::Zeroizing<[u8; 32]>> {
        self.identity
            .ecdh_pending_enrollment(&self.enrollment_id, kid, peer_public)
            .map(|shared| zeroize::Zeroizing::new(*shared.as_bytes()))
            .map_err(map_identity_error)
    }

    fn auth_state(&self) -> crate::ImResult<crate::internal::auth::state::AuthStateSnapshot> {
        self.auth.read().map(|state| state.clone()).map_err(|_| {
            crate::ImError::LocalStateUnavailable {
                detail: "pending enrollment auth state lock poisoned".to_owned(),
            }
        })
    }

    fn valid_auth_token(&self) -> crate::ImResult<Option<String>> {
        let state = self.auth_state()?;
        Ok(state
            .has_valid_token
            .then_some(state.bearer_token)
            .flatten())
    }

    fn persist_auth_token(&self, token: &str) -> crate::ImResult<()> {
        let raw = crate::internal::auth::state::auth_state_json_for_token(token)?;
        let snapshot = crate::internal::auth::state::parse_auth_state(&raw)?;
        *self
            .auth
            .write()
            .map_err(|_| crate::ImError::LocalStateUnavailable {
                detail: "pending enrollment auth state lock poisoned".to_owned(),
            })? = snapshot;
        Ok(())
    }
}

impl super::IdentitySigner for AnpIdentitySigner {
    fn did_document(&self) -> crate::ImResult<serde_json::Value> {
        Ok(self.lock_identity()?.document().clone())
    }

    fn optional_did_document(&self) -> crate::ImResult<Option<serde_json::Value>> {
        self.did_document().map(Some)
    }

    fn request_signing_key_id(&self) -> crate::ImResult<String> {
        self.active_kid(&[KeyRole::DeviceSigning, KeyRole::RequestSigning])
    }

    fn agreement_key_id(&self) -> crate::ImResult<String> {
        self.active_kid(&[KeyRole::E2eeAgreement])
    }

    fn root_control_key_id(&self) -> crate::ImResult<String> {
        self.active_kid(&[KeyRole::RootControl])
    }

    fn sign(&self, kid: &str, message: &[u8]) -> crate::ImResult<Vec<u8>> {
        self.lock_identity()?
            .sign(kid, message)
            .map_err(map_identity_error)
    }

    fn sign_device_assertion(&self, kid: &str, message: &[u8]) -> crate::ImResult<Vec<u8>> {
        self.lock_identity()?
            .sign_device_assertion(kid, message)
            .map_err(map_identity_error)
    }

    fn sign_root(&self, _kid: &str, _message: &[u8]) -> crate::ImResult<Vec<u8>> {
        Err(crate::ImError::PermissionDenied)
    }

    fn ecdh(&self, kid: &str, peer_public: &[u8]) -> crate::ImResult<zeroize::Zeroizing<[u8; 32]>> {
        self.lock_identity()?
            .ecdh(kid, peer_public)
            .map(|secret| zeroize::Zeroizing::new(*secret.as_bytes()))
            .map_err(map_identity_error)
    }

    fn sign_object_proof(
        &self,
        kid: &str,
        document: &serde_json::Value,
        issuer_did: &str,
        created: Option<String>,
    ) -> crate::ImResult<serde_json::Value> {
        self.lock_identity()?
            .sign_object_proof(kid, document, issuer_did, created)
            .map_err(map_identity_error)
    }

    fn sign_document_proof(
        &self,
        document: &serde_json::Value,
        verification_method: &str,
        options: anp::proof::ProofGenerationOptions,
    ) -> crate::ImResult<serde_json::Value> {
        self.lock_identity()?
            .sign_document_proof(document, verification_method, options)
            .map_err(map_identity_error)
    }

    fn sign_origin_proof(
        &self,
        method: &str,
        meta: &serde_json::Value,
        body: &serde_json::Value,
        kid: &str,
        options: anp::proof::Rfc9421OriginProofGenerationOptions,
    ) -> crate::ImResult<anp::proof::Rfc9421OriginProof> {
        self.lock_identity()?
            .sign_origin_proof(method, meta, body, kid, options)
            .map_err(map_identity_error)
    }

    fn legacy_did_wba_header(
        &self,
        kid: &str,
        service_domain: &str,
        version: &str,
    ) -> crate::ImResult<String> {
        self.lock_identity()?
            .legacy_did_wba_header(kid, service_domain, version)
            .map_err(map_identity_error)
    }

    fn ensure_root_control_available(&self) -> crate::ImResult<()> {
        let identity = self.lock_identity()?;
        let root_kid = identity
            .keys()
            .iter()
            .find(|key| {
                key.role == KeyRole::RootControl
                    && key.state == KeyState::Active
                    && key.origin == anp_identity::KeyOrigin::Managed
            })
            .map(|key| key.kid.clone())
            .ok_or(crate::ImError::PermissionDenied)?;
        let mut unsigned = identity.document().clone();
        let domain = unsigned
            .get("proof")
            .and_then(serde_json::Value::as_object)
            .and_then(|proof| proof.get("domain"))
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned);
        unsigned
            .as_object_mut()
            .ok_or(crate::ImError::PermissionDenied)?
            .remove("proof");
        identity
            .sign_document_proof(
                &unsigned,
                &root_kid,
                anp::proof::ProofGenerationOptions {
                    proof_purpose: Some("assertionMethod".to_string()),
                    proof_type: Some(anp::proof::PROOF_TYPE_DATA_INTEGRITY.to_string()),
                    cryptosuite: Some(anp::proof::CRYPTOSUITE_EDDSA_JCS_2022.to_string()),
                    domain,
                    ..Default::default()
                },
            )
            .map(|_| ())
            .map_err(map_identity_error)
    }

    fn http_signature_headers(
        &self,
        kid: &str,
        request_url: &str,
        request_method: &str,
        headers: Option<&BTreeMap<String, String>>,
        body: Option<&[u8]>,
        options: anp::authentication::HttpSignatureOptions,
    ) -> crate::ImResult<BTreeMap<String, String>> {
        self.lock_identity()?
            .http_signature_headers_with_options(
                kid,
                request_url,
                request_method,
                headers,
                body,
                options,
            )
            .map_err(map_identity_error)
    }

    fn auth_state(&self) -> crate::ImResult<crate::internal::auth::state::AuthStateSnapshot> {
        if self.lock_identity()?.state() != anp_identity::IdentityState::Active {
            return Err(crate::ImError::PermissionDenied);
        }
        match &self.auth {
            AnpIdentityAuth::Ephemeral { state } => state
                .read()
                .map(|state| state.clone())
                .map_err(|_| crate::ImError::LocalStateUnavailable {
                    detail: "ephemeral auth state lock poisoned".to_owned(),
                }),
            AnpIdentityAuth::File { auth_state_path } => {
                crate::internal::auth::state::read_auth_state(auth_state_path)
            }
            AnpIdentityAuth::Vault { vault, auth_ref } => {
                Self::auth_state_from_vault(vault, auth_ref)
            }
        }
    }

    fn valid_auth_token(&self) -> crate::ImResult<Option<String>> {
        let state = self.auth_state()?;
        Ok(state
            .has_valid_token
            .then_some(state.bearer_token)
            .flatten())
    }

    fn persist_auth_token(&self, token: &str) -> crate::ImResult<()> {
        if self.lock_identity()?.state() != anp_identity::IdentityState::Active {
            return Err(crate::ImError::PermissionDenied);
        }
        match &self.auth {
            AnpIdentityAuth::Ephemeral { state } => {
                let raw = crate::internal::auth::state::auth_state_json_for_token(token)?;
                let snapshot = crate::internal::auth::state::parse_auth_state(&raw)?;
                *state
                    .write()
                    .map_err(|_| crate::ImError::LocalStateUnavailable {
                        detail: "ephemeral auth state lock poisoned".to_owned(),
                    })? = snapshot;
                Ok(())
            }
            AnpIdentityAuth::File { auth_state_path } => {
                crate::internal::auth::state::persist_jwt_token(auth_state_path, token)
            }
            AnpIdentityAuth::Vault { vault, auth_ref } => {
                let auth_ref =
                    auth_ref
                        .read()
                        .map_err(|_| crate::ImError::LocalStateUnavailable {
                            detail: "anp identity auth ref lock poisoned".to_string(),
                        })?;
                validate_auth_ref(&auth_ref)?;
                let raw = crate::internal::auth::state::auth_state_json_for_token(token)?;
                let candidate = crate::internal::auth::state::parse_auth_state(&raw)?;
                let sealed = vault.seal(SealSecretRequest {
                    metadata: metadata_from_ref(&auth_ref),
                    plaintext: SecretBytes::from_vec(raw),
                })?;
                if sealed != *auth_ref {
                    return Err(crate::ImError::PermissionDenied);
                }
                let persisted = vault.open(&sealed)?;
                let persisted =
                    crate::internal::auth::state::parse_auth_state(persisted.expose_secret())?;
                if persisted.bearer_token.as_deref() != Some(token.trim())
                    || persisted.expires_at != candidate.expires_at
                {
                    return Err(crate::ImError::PermissionDenied);
                }
                Ok(())
            }
        }
    }

    fn advance_vault_auth_ref(&self, committed: &SecretRef) -> crate::ImResult<()> {
        if self.lock_identity()?.state() != anp_identity::IdentityState::Active {
            return Err(crate::ImError::PermissionDenied);
        }
        let AnpIdentityAuth::Vault { vault, auth_ref } = &self.auth else {
            return Err(crate::ImError::PermissionDenied);
        };
        let mut current = auth_ref
            .write()
            .map_err(|_| crate::ImError::LocalStateUnavailable {
                detail: "anp identity auth ref lock poisoned".to_string(),
            })?;
        if committed.workspace_id != current.workspace_id
            || committed.device_id != current.device_id
            || committed.identity_id != current.identity_id
            || committed.did != current.did
            || committed.kind != SecretKind::AuthJwt
            || committed.key_id != current.key_id
            || committed.key_version < current.key_version
        {
            return Err(crate::ImError::PermissionDenied);
        }
        let opened = vault.open(committed)?;
        if !crate::internal::auth::state::parse_auth_state(opened.expose_secret())?.has_token {
            return Err(crate::ImError::PermissionDenied);
        }
        *current = committed.clone();
        Ok(())
    }
}

fn validate_auth_ref(auth_ref: &SecretRef) -> crate::ImResult<()> {
    if auth_ref.kind != SecretKind::AuthJwt
        || auth_ref.key_id.trim().is_empty()
        || auth_ref.key_version == 0
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

fn metadata_from_ref(secret_ref: &SecretRef) -> SecretMetadata {
    SecretMetadata {
        workspace_id: secret_ref.workspace_id.clone(),
        device_id: secret_ref.device_id.clone(),
        identity_id: secret_ref.identity_id.clone(),
        did: secret_ref.did.clone(),
        kind: secret_ref.kind.clone(),
        key_id: secret_ref.key_id.clone(),
        key_version: secret_ref.key_version,
        policy: SecretAccessPolicy::no_prompt_local_secret(),
    }
}

fn map_identity_error(error: anp_identity::DidError) -> crate::ImError {
    match error {
        anp_identity::DidError::KeyNotFound
        | anp_identity::DidError::ExternalKeyOperation
        | anp_identity::DidError::KeyRoleViolation
        | anp_identity::DidError::KeyNotUsable
        | anp_identity::DidError::KeyMaterialErased
        | anp_identity::DidError::RootCapabilityUnavailable => crate::ImError::PermissionDenied,
        anp_identity::DidError::Conflict => crate::ImError::LocalStateUnavailable {
            detail: "anp identity handle requires reload after a generation conflict".to_string(),
        },
        error => crate::ImError::LocalStateUnavailable {
            detail: format!("anp identity operation failed: {error}"),
        },
    }
}

#[cfg(test)]
mod tests;
