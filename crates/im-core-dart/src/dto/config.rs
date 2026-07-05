use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartImCoreConfig {
    pub service_base_url: String,
    pub did_domain: String,
    pub user_service_endpoint: Option<String>,
    pub message_service_endpoint: Option<String>,
    pub mail_service_endpoint: Option<String>,
    pub anp_service_endpoint: Option<String>,
    pub anp_service_did: Option<String>,
    pub transport_policy: DartMessageTransportPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DartMessageTransportPolicy {
    Auto,
    HttpOnly,
    RealtimePreferred,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartImCorePaths {
    pub identity_root_dir: String,
    pub registry_path: String,
    pub default_identity_path: Option<String>,
    pub sqlite_path: String,
    pub cache_dir: String,
    pub temp_dir: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartImCoreOpenOptions {
    pub identity_secret_storage_policy: DartIdentitySecretStoragePolicy,
    pub identity_secret_vault: Option<DartImCoreSecretVaultOptions>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DartIdentitySecretStoragePolicy {
    FileCompat,
    VaultPreferred,
    VaultRequired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartImCoreSecretVaultOptions {
    pub root_key: DartDeviceVaultRootKey,
    pub vault_dir: String,
    pub workspace_id: String,
    pub device_id: String,
}

#[derive(Clone, PartialEq, Eq)]
pub struct DartDeviceVaultRootKey {
    pub bytes: Vec<u8>,
}

impl fmt::Debug for DartDeviceVaultRootKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DartDeviceVaultRootKey")
            .field("len", &self.bytes.len())
            .field("value", &"[REDACTED]")
            .finish()
    }
}
