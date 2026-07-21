mod did_auth;
mod file;
mod hosted;
pub(crate) mod vault;

pub(crate) use self::did_auth::ProviderBackedDidAuth;
pub(crate) use self::file::FileBackedKeyMaterialProvider;
pub(crate) use self::hosted::HostedKeyMaterialProvider;
pub(crate) use self::vault::LegacyVaultKeyMaterialRefs;

/// Explicit compatibility adapter for identities where legacy `key-1` has both
/// request-signing and DID Document root-control semantics.
///
/// New multi-device identities must not use this adapter: their device signing
/// and root-control keys are separate vault records.
pub(crate) struct LegacyKey1RoleAdapter {
    key1_private_pem: String,
}

impl LegacyKey1RoleAdapter {
    pub(crate) fn new(key1_private_pem: String) -> Self {
        Self { key1_private_pem }
    }

    pub(crate) fn device_request_signing_private_pem(&self) -> String {
        self.key1_private_pem.clone()
    }

    pub(crate) fn did_document_root_private_pem(&self) -> String {
        self.key1_private_pem.clone()
    }
}

impl std::fmt::Debug for LegacyKey1RoleAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LegacyKey1RoleAdapter")
            .field("key1_private_pem", &"<redacted-private-key>")
            .finish()
    }
}

pub(crate) trait KeyMaterialProvider: Send + Sync {
    fn did_document(&self) -> crate::ImResult<serde_json::Value>;

    fn optional_did_document(&self) -> crate::ImResult<Option<serde_json::Value>>;

    /// Returns the device key used for login, HTTP auth, and daily requests.
    fn device_request_signing_private_pem(&self) -> crate::ImResult<String>;

    /// Returns the DID root key used only to create or update a DID Document.
    fn did_document_root_private_pem(&self) -> crate::ImResult<String>;

    fn e2ee_agreement_private_pem(&self) -> crate::ImResult<String>;

    fn auth_state(&self) -> crate::ImResult<crate::internal::auth::state::AuthStateSnapshot>;

    fn valid_auth_token(&self) -> crate::ImResult<Option<String>>;

    fn persist_auth_token(&self, token: &str) -> crate::ImResult<()>;

    /// True only for the Vault-backed vNext device-token profile. Those
    /// identities rotate an access/refresh pair through the internal device
    /// refresh RPC and must never fall back to legacy DID-auth `get_me`.
    fn uses_vnext_device_tokens(&self) -> bool {
        false
    }

    /// Atomically replaces a vNext device access/refresh pair under the
    /// provider's current authoritative auth SecretRef.
    fn persist_vnext_auth_token_pair(
        &self,
        _access_token: &str,
        _refresh_token: &str,
        _expires_at: &str,
    ) -> crate::ImResult<()> {
        Err(crate::ImError::PermissionDenied)
    }

    /// Advances a live Vault-backed provider to the auth SecretRef committed
    /// by the identity index. Non-Vault providers cannot participate in this
    /// local vNext convergence operation.
    fn advance_vault_auth_ref(
        &self,
        _committed: &crate::internal::secret_vault::record::SecretRef,
    ) -> crate::ImResult<()> {
        Err(crate::ImError::PermissionDenied)
    }

    /// Advances a live vNext Vault-backed provider to the DID root SecretRef
    /// committed by the identity index after root import.
    fn advance_vault_root_ref(
        &self,
        _committed: &crate::internal::secret_vault::record::SecretRef,
    ) -> crate::ImResult<()> {
        Err(crate::ImError::PermissionDenied)
    }
}
