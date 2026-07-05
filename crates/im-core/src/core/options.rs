use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::vault::{DeviceVaultRootKey, FileSecretVault, FileSecretVaultStore, SecretVault};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentitySecretStoragePolicy {
    FileCompat,
    VaultPreferred,
    VaultRequired,
}

impl Default for IdentitySecretStoragePolicy {
    fn default() -> Self {
        Self::FileCompat
    }
}

pub struct ImCoreSecretVaultOptions {
    pub root_key: DeviceVaultRootKey,
    pub vault_dir: PathBuf,
    pub workspace_id: String,
    pub device_id: String,
}

impl ImCoreSecretVaultOptions {
    pub fn new(
        root_key: DeviceVaultRootKey,
        vault_dir: impl Into<PathBuf>,
        workspace_id: impl Into<String>,
        device_id: impl Into<String>,
    ) -> Self {
        Self {
            root_key,
            vault_dir: vault_dir.into(),
            workspace_id: workspace_id.into(),
            device_id: device_id.into(),
        }
    }
}

impl fmt::Debug for ImCoreSecretVaultOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ImCoreSecretVaultOptions")
            .field("root_key", &"<redacted-root-key>")
            .field("vault_dir", &self.vault_dir)
            .field("workspace_id", &self.workspace_id)
            .field("device_id", &self.device_id)
            .finish()
    }
}

#[derive(Debug, Default)]
pub struct ImCoreOpenOptions {
    pub identity_secret_storage_policy: IdentitySecretStoragePolicy,
    pub identity_secret_vault: Option<ImCoreSecretVaultOptions>,
}

impl ImCoreOpenOptions {
    pub fn file_compat() -> Self {
        Self::default()
    }

    pub fn with_identity_secret_vault(
        mut self,
        identity_secret_storage_policy: IdentitySecretStoragePolicy,
        identity_secret_vault: ImCoreSecretVaultOptions,
    ) -> Self {
        self.identity_secret_storage_policy = identity_secret_storage_policy;
        self.identity_secret_vault = Some(identity_secret_vault);
        self
    }
}

#[derive(Clone)]
pub(crate) struct IdentityVaultContext {
    policy: IdentitySecretStoragePolicy,
    vault: Arc<dyn SecretVault + Send + Sync>,
    workspace_id: String,
    device_id: String,
}

impl IdentityVaultContext {
    pub(crate) fn from_options(options: ImCoreSecretVaultOptions) -> crate::ImResult<Self> {
        let workspace_id = required_non_empty("workspace_id", options.workspace_id)?;
        let device_id = required_non_empty("device_id", options.device_id)?;
        if options.vault_dir.as_os_str().is_empty() {
            return Err(crate::ImError::invalid_input(
                Some("vault_dir".to_owned()),
                "vault directory must not be empty",
            ));
        }
        let vault = Arc::new(FileSecretVault::new(
            options.root_key,
            FileSecretVaultStore::new(options.vault_dir),
        ));
        Ok(Self {
            policy: IdentitySecretStoragePolicy::FileCompat,
            vault,
            workspace_id,
            device_id,
        })
    }

    pub(crate) fn with_policy(mut self, policy: IdentitySecretStoragePolicy) -> Self {
        self.policy = policy;
        self
    }

    pub(crate) fn vault(&self) -> Arc<dyn SecretVault + Send + Sync> {
        self.vault.clone()
    }

    pub(crate) fn workspace_id(&self) -> &str {
        &self.workspace_id
    }

    pub(crate) fn device_id(&self) -> &str {
        &self.device_id
    }
}

impl fmt::Debug for IdentityVaultContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IdentityVaultContext")
            .field("policy", &self.policy)
            .field("vault", &"<redacted-secret-vault>")
            .field("workspace_id", &self.workspace_id)
            .field("device_id", &self.device_id)
            .finish()
    }
}

fn required_non_empty(field: &str, value: String) -> crate::ImResult<String> {
    let value = value.trim().to_owned();
    if value.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some(field.to_owned()),
            format!("{field} must not be empty"),
        ));
    }
    Ok(value)
}
