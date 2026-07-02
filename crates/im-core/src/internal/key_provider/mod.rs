mod did_auth;
mod file;
pub(crate) mod vault;

pub(crate) use self::did_auth::ProviderBackedDidAuth;
pub(crate) use self::file::FileBackedKeyMaterialProvider;
pub(crate) use self::vault::VaultKeyMaterialRefs;

pub(crate) trait KeyMaterialProvider: Send + Sync {
    fn did_document(&self) -> crate::ImResult<serde_json::Value>;

    fn optional_did_document(&self) -> crate::ImResult<Option<serde_json::Value>>;

    fn default_signing_private_pem(&self) -> crate::ImResult<String>;

    fn e2ee_agreement_private_pem(&self) -> crate::ImResult<String>;

    fn auth_state(&self) -> crate::ImResult<crate::internal::auth::state::AuthStateSnapshot>;

    fn valid_auth_token(&self) -> crate::ImResult<Option<String>>;

    fn persist_auth_token(&self, token: &str) -> crate::ImResult<()>;
}
