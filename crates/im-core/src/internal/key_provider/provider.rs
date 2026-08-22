use std::path::PathBuf;
use std::sync::Arc;

use crate::internal::identity_provider::{
    IdentitySession, ProviderKeyPurpose, ProviderPublicIdentity,
};

/// Public-data cache plus asynchronous Host Provider session.
///
/// Secret operations deliberately fail on the legacy synchronous methods.
/// External mode must use `async_session()` so no Rust worker blocks while a
/// JavaScript Promise is being resolved.
pub(crate) struct ProviderIdentitySigner {
    public: ProviderPublicIdentity,
    session: Arc<dyn IdentitySession>,
    auth_state_path: PathBuf,
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
            auth_state_path,
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
        crate::internal::auth::state::read_auth_state(&self.auth_state_path)
    }

    fn valid_auth_token(&self) -> crate::ImResult<Option<String>> {
        crate::internal::auth::state::read_jwt_token(&self.auth_state_path)
    }

    fn persist_auth_token(&self, token: &str) -> crate::ImResult<()> {
        crate::internal::auth::state::persist_jwt_token(&self.auth_state_path, token)
    }

    fn reload_custody(&self) -> crate::ImResult<()> {
        // Recovery is asynchronous for an External Provider. Callers must use
        // the provider session rather than blocking a runtime worker here.
        Err(crate::ImError::PermissionDenied)
    }
}
