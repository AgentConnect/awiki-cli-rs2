use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use anp_identity::host::{
    DocumentProofOptions, DocumentProofRequest, ExactHttpRequest, HttpHeader,
    HttpRequestSigningOptions, HttpRequestSigningPort, IdentityStatusPort, KeyAgreementPort,
    KeyAgreementRequest, LegacyDidWbaPort, ObjectProofRequest, TypedProofPort,
};
use anp_identity::{
    KeyPurpose, KeySelector, ManagedIdentity, OriginProofOptions, OriginProofRequest,
    PublicIdentityState, SignRequest, SigningPurpose,
};

use crate::internal::platform_secret::SecretBytes;
use crate::internal::secret_vault::policy::SecretAccessPolicy;
use crate::internal::secret_vault::record::{SecretKind, SecretMetadata, SecretRef};
use crate::internal::secret_vault::{SealSecretRequest, SecretVault};

pub(crate) struct AnpIdentitySigner {
    identity: Arc<ManagedIdentity>,
    auth: AnpIdentityAuth,
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
    pub(crate) fn new_ephemeral(identity: ManagedIdentity) -> Self {
        Self {
            identity: Arc::new(identity),
            auth: AnpIdentityAuth::Ephemeral {
                state: RwLock::new(Default::default()),
            },
        }
    }

    pub(crate) fn new_file(identity: ManagedIdentity, auth_state_path: PathBuf) -> Self {
        Self {
            identity: Arc::new(identity),
            auth: AnpIdentityAuth::File { auth_state_path },
        }
    }

    pub(crate) fn new_vault(
        identity: ManagedIdentity,
        vault: Arc<dyn SecretVault + Send + Sync>,
        auth_ref: SecretRef,
    ) -> crate::ImResult<Self> {
        validate_auth_ref(&auth_ref)?;
        Ok(Self {
            identity: Arc::new(identity),
            auth: AnpIdentityAuth::Vault {
                vault,
                auth_ref: RwLock::new(auth_ref),
            },
        })
    }

    pub(crate) fn reload(&self) -> crate::ImResult<()> {
        self.identity
            .recover_identity()
            .map_err(map_facade_identity_error)
    }

    pub(crate) fn provider_session(
        &self,
    ) -> Arc<dyn crate::internal::identity_provider::IdentitySession> {
        Arc::new(
            crate::internal::identity_provider::DirectAnpIdentitySession::from_shared(
                self.identity.clone(),
            ),
        )
    }

    fn identity_operation<T>(
        &self,
        mut operation: impl FnMut(&ManagedIdentity) -> anp_identity::IdentityResult<T>,
    ) -> crate::ImResult<T> {
        match operation(&self.identity) {
            Err(anp_identity::IdentityError::Conflict) => {
                self.identity
                    .recover_identity()
                    .map_err(map_facade_identity_error)?;
                operation(&self.identity).map_err(map_facade_identity_error)
            }
            result => result.map_err(map_facade_identity_error),
        }
    }

    fn active_kid(&self, purposes: &[KeyPurpose]) -> crate::ImResult<String> {
        let identity = self
            .identity
            .public_identity()
            .map_err(map_facade_identity_error)?;
        if identity.state != PublicIdentityState::Active {
            return Err(crate::ImError::PermissionDenied);
        }
        identity
            .active_keys
            .iter()
            .find(|key| {
                purposes
                    .iter()
                    .any(|purpose| key.purposes.contains(purpose))
            })
            .map(|key| key.kid.clone())
            .ok_or(crate::ImError::PermissionDenied)
    }

    fn signing_purpose(&self, kid: &str) -> crate::ImResult<SigningPurpose> {
        let identity = self
            .identity
            .public_identity()
            .map_err(map_facade_identity_error)?;
        let canonical = if kid.starts_with('#') {
            format!("{}{}", identity.reference.did, kid)
        } else {
            kid.to_owned()
        };
        let key = identity
            .active_keys
            .iter()
            .find(|key| key.kid == canonical)
            .ok_or(crate::ImError::PermissionDenied)?;
        if key.purposes.contains(&KeyPurpose::DeviceAssertion) {
            Ok(SigningPurpose::DeviceAssertion)
        } else if key.purposes.contains(&KeyPurpose::Authentication) {
            Ok(SigningPurpose::Authentication)
        } else {
            Err(crate::ImError::PermissionDenied)
        }
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

impl super::IdentitySigner for AnpIdentitySigner {
    fn async_session(
        &self,
    ) -> Option<Arc<dyn crate::internal::identity_provider::IdentitySession>> {
        Some(self.provider_session())
    }

    fn did_document(&self) -> crate::ImResult<serde_json::Value> {
        self.identity
            .public_identity()
            .map(|identity| identity.document.into_value())
            .map_err(map_facade_identity_error)
    }

    fn optional_did_document(&self) -> crate::ImResult<Option<serde_json::Value>> {
        self.did_document().map(Some)
    }

    fn request_signing_key_id(&self) -> crate::ImResult<String> {
        self.active_kid(&[KeyPurpose::DeviceAssertion, KeyPurpose::Authentication])
    }

    fn agreement_key_id(&self) -> crate::ImResult<String> {
        self.active_kid(&[KeyPurpose::KeyAgreement])
    }

    fn root_control_key_id(&self) -> crate::ImResult<String> {
        self.active_kid(&[KeyPurpose::RootControl])
    }

    fn sign(&self, kid: &str, message: &[u8]) -> crate::ImResult<Vec<u8>> {
        let purpose = self.signing_purpose(kid)?;
        self.identity_operation(|identity| {
            identity
                .sign(SignRequest {
                    purpose: purpose.clone(),
                    key: KeySelector::Kid(kid.to_owned()),
                    payload: message.to_vec(),
                })
                .map(|signature| signature.bytes)
        })
    }

    fn sign_device_assertion(&self, kid: &str, message: &[u8]) -> crate::ImResult<Vec<u8>> {
        self.identity_operation(|identity| {
            identity
                .sign(SignRequest {
                    purpose: SigningPurpose::DeviceAssertion,
                    key: KeySelector::Kid(kid.to_owned()),
                    payload: message.to_vec(),
                })
                .map(|signature| signature.bytes)
        })
    }

    fn sign_root(&self, _kid: &str, _message: &[u8]) -> crate::ImResult<Vec<u8>> {
        Err(crate::ImError::PermissionDenied)
    }

    fn ecdh(&self, kid: &str, peer_public: &[u8]) -> crate::ImResult<zeroize::Zeroizing<[u8; 32]>> {
        let peer_public: [u8; 32] = peer_public
            .try_into()
            .map_err(|_| crate::ImError::PermissionDenied)?;
        self.identity_operation(|identity| {
            identity.derive_shared_secret(KeyAgreementRequest {
                key: KeySelector::Kid(kid.to_owned()),
                peer_public,
            })
        })
        .map(|secret| zeroize::Zeroizing::new(*secret.as_bytes()))
    }

    fn sign_object_proof(
        &self,
        kid: &str,
        document: &serde_json::Value,
        issuer_did: &str,
        created: Option<String>,
    ) -> crate::ImResult<serde_json::Value> {
        self.identity_operation(|identity| {
            identity.sign_object_proof(ObjectProofRequest {
                key: KeySelector::Kid(kid.to_owned()),
                document: document.clone(),
                issuer_did: issuer_did.to_owned(),
                created: created.clone(),
            })
        })
    }

    fn sign_document_proof(
        &self,
        document: &serde_json::Value,
        verification_method: &str,
        options: anp::proof::ProofGenerationOptions,
    ) -> crate::ImResult<serde_json::Value> {
        self.identity_operation(|identity| {
            identity.sign_document_proof(DocumentProofRequest {
                key: KeySelector::Kid(verification_method.to_owned()),
                document: document.clone(),
                options: DocumentProofOptions {
                    proof_purpose: options.proof_purpose.clone(),
                    proof_type: options.proof_type.clone(),
                    cryptosuite: options.cryptosuite.clone(),
                    created: options.created.clone(),
                    domain: options.domain.clone(),
                    challenge: options.challenge.clone(),
                },
            })
        })
    }

    fn sign_origin_proof(
        &self,
        method: &str,
        meta: &serde_json::Value,
        body: &serde_json::Value,
        kid: &str,
        options: anp::proof::Rfc9421OriginProofGenerationOptions,
    ) -> crate::ImResult<anp::proof::Rfc9421OriginProof> {
        self.identity_operation(|identity| {
            identity
                .sign_origin_proof(OriginProofRequest {
                    method: method.to_owned(),
                    meta: meta.clone(),
                    body: body.clone(),
                    key: KeySelector::Kid(kid.to_owned()),
                    options: OriginProofOptions {
                        created: options.created,
                        expires: options.expires,
                        nonce: options.nonce.clone(),
                    },
                })
                .map(|proof| anp::proof::Rfc9421OriginProof {
                    content_digest: proof.content_digest,
                    signature_input: proof.signature_input,
                    signature: proof.signature,
                })
        })
    }

    fn legacy_did_wba_header(
        &self,
        kid: &str,
        service_domain: &str,
        version: &str,
    ) -> crate::ImResult<String> {
        self.identity_operation(|identity| {
            identity.prepare_legacy_did_wba(
                KeySelector::Kid(kid.to_owned()),
                service_domain,
                version,
            )
        })
    }

    fn ensure_root_control_available(&self) -> crate::ImResult<()> {
        let root_kid = self.root_control_key_id()?;
        self.identity_operation(|identity| {
            let mut unsigned = identity.public_identity()?.document.into_value();
            let domain = unsigned
                .get("proof")
                .and_then(serde_json::Value::as_object)
                .and_then(|proof| proof.get("domain"))
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned);
            unsigned
                .as_object_mut()
                .ok_or(anp_identity::IdentityError::CorruptState)?
                .remove("proof");
            identity.sign_document_proof(DocumentProofRequest {
                key: KeySelector::Kid(root_kid.clone()),
                document: unsigned,
                options: DocumentProofOptions {
                    proof_purpose: Some("assertionMethod".to_string()),
                    proof_type: Some(anp::proof::PROOF_TYPE_DATA_INTEGRITY.to_string()),
                    cryptosuite: Some(anp::proof::CRYPTOSUITE_EDDSA_JCS_2022.to_string()),
                    domain,
                    ..Default::default()
                },
            })
        })
        .map(|_| ())
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
        self.identity_operation(|identity| {
            identity
                .prepare_http_signature(ExactHttpRequest {
                    key: KeySelector::Kid(kid.to_owned()),
                    url: request_url.to_owned(),
                    method: request_method.to_owned(),
                    headers: headers
                        .into_iter()
                        .flat_map(|headers| headers.iter())
                        .map(|(name, value)| HttpHeader {
                            name: name.clone(),
                            value: value.clone(),
                        })
                        .collect(),
                    body: body.map(ToOwned::to_owned),
                    options: HttpRequestSigningOptions {
                        nonce: options.nonce.clone(),
                        created: options.created,
                        expires: options.expires,
                        covered_components: options.covered_components.clone(),
                    },
                })
                .map(|attempt| {
                    attempt
                        .header_patch
                        .into_iter()
                        .map(|header| (header.name, header.value))
                        .collect()
                })
        })
    }

    fn auth_state(&self) -> crate::ImResult<crate::internal::auth::state::AuthStateSnapshot> {
        if self
            .identity
            .public_identity()
            .map_err(map_facade_identity_error)?
            .state
            != PublicIdentityState::Active
        {
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
        if self
            .identity
            .public_identity()
            .map_err(map_facade_identity_error)?
            .state
            != PublicIdentityState::Active
        {
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

    fn reload_custody(&self) -> crate::ImResult<()> {
        self.reload()
    }

    fn advance_vault_auth_ref(&self, committed: &SecretRef) -> crate::ImResult<()> {
        if self
            .identity
            .public_identity()
            .map_err(map_facade_identity_error)?
            .state
            != PublicIdentityState::Active
        {
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

fn map_facade_identity_error(error: anp_identity::IdentityError) -> crate::ImError {
    match error {
        anp_identity::IdentityError::KeyNotFound
        | anp_identity::IdentityError::KeyUnavailable
        | anp_identity::IdentityError::KeyPurposeViolation
        | anp_identity::IdentityError::AmbiguousKey
        | anp_identity::IdentityError::CapabilityUnavailable => crate::ImError::PermissionDenied,
        anp_identity::IdentityError::Conflict => crate::ImError::LocalStateUnavailable {
            detail: "anp identity handle requires recovery after a generation conflict".to_owned(),
        },
        error => crate::ImError::LocalStateUnavailable {
            detail: format!("anp identity operation failed: {error}"),
        },
    }
}

#[cfg(test)]
mod tests;
