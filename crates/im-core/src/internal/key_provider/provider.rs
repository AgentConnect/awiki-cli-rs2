use std::path::PathBuf;
use std::sync::Arc;
use std::sync::RwLock;

use crate::internal::identity_provider::{
    IdentitySession, ProviderKeyPurpose, ProviderPublicIdentity,
};
use crate::internal::platform_secret::SecretBytes;
use crate::internal::secret_vault::policy::SecretAccessPolicy;
use crate::internal::secret_vault::record::{SecretKind, SecretMetadata, SecretRef};
use crate::internal::secret_vault::{SealSecretRequest, SecretVault};

/// Public-data cache plus asynchronous Host Provider session.
///
/// Secret operations deliberately fail on the legacy synchronous methods.
/// External mode must use `async_session()` so no Rust worker blocks while a
/// JavaScript Promise is being resolved.
pub(crate) struct ProviderIdentitySigner {
    public: ProviderPublicIdentity,
    session: Arc<dyn IdentitySession>,
    auth: ProviderIdentityAuth,
}

pub(crate) struct ProviderEnrollmentIdentitySigner {
    document: serde_json::Value,
    signing_key_id: String,
    agreement_key_id: String,
    session: Arc<dyn crate::internal::identity_provider::ProviderEnrollmentSession>,
    auth: RwLock<crate::internal::auth::state::AuthStateSnapshot>,
}

enum ProviderIdentityAuth {
    Ephemeral(RwLock<crate::internal::auth::state::AuthStateSnapshot>),
    File(PathBuf),
    Vault {
        vault: Arc<dyn SecretVault + Send + Sync>,
        auth_ref: RwLock<SecretRef>,
    },
}

impl ProviderIdentitySigner {
    pub(crate) fn new(
        public: ProviderPublicIdentity,
        session: Arc<dyn IdentitySession>,
        auth_state_path: PathBuf,
    ) -> crate::ImResult<Self> {
        if public.state != crate::internal::identity_provider::ProviderIdentityState::Active {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(Self {
            public,
            session,
            auth: ProviderIdentityAuth::File(auth_state_path),
        })
    }

    pub(crate) fn new_ephemeral(
        public: ProviderPublicIdentity,
        session: Arc<dyn IdentitySession>,
    ) -> crate::ImResult<Self> {
        if public.state != crate::internal::identity_provider::ProviderIdentityState::Active {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(Self {
            public,
            session,
            auth: ProviderIdentityAuth::Ephemeral(RwLock::new(Default::default())),
        })
    }

    pub(crate) fn new_vault(
        public: ProviderPublicIdentity,
        session: Arc<dyn IdentitySession>,
        vault: Arc<dyn SecretVault + Send + Sync>,
        auth_ref: SecretRef,
    ) -> crate::ImResult<Self> {
        validate_auth_ref(&auth_ref)?;
        if public.state != crate::internal::identity_provider::ProviderIdentityState::Active {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(Self {
            public,
            session,
            auth: ProviderIdentityAuth::Vault {
                vault,
                auth_ref: RwLock::new(auth_ref),
            },
        })
    }

    fn active_kid(&self, purpose: ProviderKeyPurpose) -> crate::ImResult<String> {
        self.public
            .active_keys
            .iter()
            .find(|key| key.purposes.contains(&purpose))
            .map(|key| key.kid.clone())
            .ok_or(crate::ImError::PermissionDenied)
    }
}

impl ProviderEnrollmentIdentitySigner {
    pub(crate) fn new(
        proposal: &crate::internal::identity_provider::ProviderEnrollmentProposal,
        session: Arc<dyn crate::internal::identity_provider::ProviderEnrollmentSession>,
        document: serde_json::Value,
        signing_key_id: String,
        agreement_key_id: String,
    ) -> crate::ImResult<Self> {
        let crate::internal::identity_provider::ProviderEnrollmentProposalKind::Device {
            signing_key,
            agreement_key,
            ..
        } = &proposal.kind
        else {
            return Err(crate::ImError::PermissionDenied);
        };
        if signing_key.kid != signing_key_id
            || agreement_key.kid != agreement_key_id
            || document.get("id").and_then(serde_json::Value::as_str)
                != Some(proposal.identity.did.as_str())
            || !anp::authentication::is_authentication_authorized(&document, &signing_key_id)
            || !anp::authentication::is_assertion_method_authorized(&document, &signing_key_id)
        {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(Self {
            document,
            signing_key_id,
            agreement_key_id,
            session,
            auth: RwLock::new(Default::default()),
        })
    }
}

impl super::IdentitySigner for ProviderIdentitySigner {
    fn async_session(&self) -> Option<Arc<dyn IdentitySession>> {
        Some(self.session.clone())
    }

    fn did_document(&self) -> crate::ImResult<serde_json::Value> {
        Ok(self.public.document.clone())
    }

    fn optional_did_document(&self) -> crate::ImResult<Option<serde_json::Value>> {
        Ok(Some(self.public.document.clone()))
    }

    fn request_signing_key_id(&self) -> crate::ImResult<String> {
        self.active_kid(ProviderKeyPurpose::DeviceAssertion)
            .or_else(|_| self.active_kid(ProviderKeyPurpose::Authentication))
    }

    fn agreement_key_id(&self) -> crate::ImResult<String> {
        self.active_kid(ProviderKeyPurpose::KeyAgreement)
    }

    fn root_control_key_id(&self) -> crate::ImResult<String> {
        self.active_kid(ProviderKeyPurpose::RootControl)
    }

    fn sign(&self, _kid: &str, _message: &[u8]) -> crate::ImResult<Vec<u8>> {
        Err(crate::ImError::PermissionDenied)
    }

    fn sign_root(&self, _kid: &str, _message: &[u8]) -> crate::ImResult<Vec<u8>> {
        Err(crate::ImError::PermissionDenied)
    }

    fn ecdh(
        &self,
        _kid: &str,
        _peer_public: &[u8],
    ) -> crate::ImResult<zeroize::Zeroizing<[u8; 32]>> {
        Err(crate::ImError::PermissionDenied)
    }

    fn auth_state(&self) -> crate::ImResult<crate::internal::auth::state::AuthStateSnapshot> {
        match &self.auth {
            ProviderIdentityAuth::Ephemeral(state) => state
                .read()
                .map(|state| state.clone())
                .map_err(|_| crate::ImError::LocalStateUnavailable {
                    detail: "ephemeral provider auth state lock poisoned".to_owned(),
                }),
            ProviderIdentityAuth::File(path) => crate::internal::auth::state::read_auth_state(path),
            ProviderIdentityAuth::Vault { vault, auth_ref } => {
                let auth_ref = auth_ref
                    .read()
                    .map_err(|_| crate::ImError::LocalStateUnavailable {
                        detail: "provider identity auth ref lock poisoned".to_owned(),
                    })?
                    .clone();
                validate_auth_ref(&auth_ref)?;
                let secret = vault.open(&auth_ref)?;
                crate::internal::auth::state::parse_auth_state(secret.expose_secret())
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
        match &self.auth {
            ProviderIdentityAuth::Ephemeral(state) => {
                let raw = crate::internal::auth::state::auth_state_json_for_token(token)?;
                let snapshot = crate::internal::auth::state::parse_auth_state(&raw)?;
                *state
                    .write()
                    .map_err(|_| crate::ImError::LocalStateUnavailable {
                        detail: "ephemeral provider auth state lock poisoned".to_owned(),
                    })? = snapshot;
                Ok(())
            }
            ProviderIdentityAuth::File(path) => {
                crate::internal::auth::state::persist_jwt_token(path, token)
            }
            ProviderIdentityAuth::Vault { vault, auth_ref } => {
                let auth_ref =
                    auth_ref
                        .read()
                        .map_err(|_| crate::ImError::LocalStateUnavailable {
                            detail: "provider identity auth ref lock poisoned".to_owned(),
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
        // Recovery is asynchronous for an External Provider. Callers must use
        // the provider session rather than blocking a runtime worker here.
        Err(crate::ImError::PermissionDenied)
    }

    fn advance_vault_auth_ref(&self, committed: &SecretRef) -> crate::ImResult<()> {
        let ProviderIdentityAuth::Vault { vault, auth_ref } = &self.auth else {
            return Err(crate::ImError::PermissionDenied);
        };
        let mut current = auth_ref
            .write()
            .map_err(|_| crate::ImError::LocalStateUnavailable {
                detail: "provider identity auth ref lock poisoned".to_owned(),
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

impl super::IdentitySigner for ProviderEnrollmentIdentitySigner {
    fn async_enrollment_session(
        &self,
    ) -> Option<Arc<dyn crate::internal::identity_provider::ProviderEnrollmentSession>> {
        Some(self.session.clone())
    }

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
        Ok(self.agreement_key_id.clone())
    }

    fn sign(&self, _kid: &str, _message: &[u8]) -> crate::ImResult<Vec<u8>> {
        Err(crate::ImError::PermissionDenied)
    }

    fn sign_root(&self, _kid: &str, _message: &[u8]) -> crate::ImResult<Vec<u8>> {
        Err(crate::ImError::PermissionDenied)
    }

    fn ecdh(
        &self,
        _kid: &str,
        _peer_public: &[u8],
    ) -> crate::ImResult<zeroize::Zeroizing<[u8; 32]>> {
        Err(crate::ImError::PermissionDenied)
    }

    fn auth_state(&self) -> crate::ImResult<crate::internal::auth::state::AuthStateSnapshot> {
        self.auth.read().map(|state| state.clone()).map_err(|_| {
            crate::ImError::LocalStateUnavailable {
                detail: "provider enrollment auth state lock poisoned".to_owned(),
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
                detail: "provider enrollment auth state lock poisoned".to_owned(),
            })? = snapshot;
        Ok(())
    }
}
