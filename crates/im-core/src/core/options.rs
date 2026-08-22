use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

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
    /// Local SecretVault context identifier. This is not an ANP protocol device ID.
    pub device_id: String,
}

impl ImCoreSecretVaultOptions {
    pub fn new(
        root_key: DeviceVaultRootKey,
        vault_dir: impl Into<PathBuf>,
        workspace_id: impl Into<String>,
        vault_context_device_id: impl Into<String>,
    ) -> Self {
        Self {
            root_key,
            vault_dir: vault_dir.into(),
            workspace_id: workspace_id.into(),
            device_id: vault_context_device_id.into(),
        }
    }
}

impl fmt::Debug for ImCoreSecretVaultOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ImCoreSecretVaultOptions")
            .field("root_key", &"<redacted-root-key>")
            .field("vault_dir", &self.vault_dir)
            .field("workspace_id", &self.workspace_id)
            .field("vault_context_device_id", &self.device_id)
            .finish()
    }
}

#[derive(Debug, Default)]
pub struct ImCoreOpenOptions {
    pub identity_secret_storage_policy: IdentitySecretStoragePolicy,
    pub identity_secret_vault: Option<ImCoreSecretVaultOptions>,
    /// Enables AWiki-local permanent device revocation.
    ///
    /// This rollout gate defaults to `false`. It is independent from Join and
    /// root transfer, and is never serialized into ANP or a DID Document.
    pub multi_device_device_revoke_enabled: bool,
    /// Enables the exact-device P5 v2 Direct product path.
    ///
    /// This rollout gate defaults to `false`, is independent from Join and
    /// root transfer, and is never serialized into ANP, a DID Document, or a
    /// cross-domain request.
    pub multi_device_direct_e2ee_enabled: bool,
    /// Enables the device-scoped P6 v2 group E2EE product path.
    ///
    /// This rollout gate defaults to `false`, is independent from Join and is
    /// never serialized into ANP, a DID Document, or a cross-domain request.
    pub multi_device_group_e2ee_enabled: bool,
    /// Enables the hidden same-deployment Manifest Handle Recovery v1 path.
    /// This gate is local, defaults to false, and does not advertise support.
    pub multi_device_handle_recovery_enabled: bool,
    /// Explicit same-deployment control-plane audience used in Recovery V4
    /// key-possession proofs. It must equal User Service
    /// `AWIKI_MULTI_DEVICE_AUDIENCE`; Core never derives or hard-codes it.
    pub multi_device_audience: Option<String>,
    #[cfg(feature = "provider-traits")]
    pub(crate) identity_custody_provider: Option<IdentityCustodyProvider>,
}

/// Opaque trusted-host handle for an externally supplied identity provider.
///
/// The handle is intentionally available only with `provider-traits`; ordinary
/// application consumers should use the product-level identity APIs instead.
#[cfg(feature = "provider-traits")]
#[derive(Clone)]
pub struct IdentityCustodyProvider {
    pub(crate) inner: Arc<dyn crate::provider::IdentityCustody>,
}

#[cfg(feature = "provider-traits")]
impl fmt::Debug for IdentityCustodyProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("IdentityCustodyProvider(<host-provider>)")
    }
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

    pub fn with_multi_device_device_revoke_enabled(mut self, enabled: bool) -> Self {
        self.multi_device_device_revoke_enabled = enabled;
        self
    }

    pub fn with_multi_device_direct_e2ee_enabled(mut self, enabled: bool) -> Self {
        self.multi_device_direct_e2ee_enabled = enabled;
        self
    }

    pub fn with_multi_device_group_e2ee_enabled(mut self, enabled: bool) -> Self {
        self.multi_device_group_e2ee_enabled = enabled;
        self
    }

    pub fn with_multi_device_handle_recovery_enabled(mut self, enabled: bool) -> Self {
        self.multi_device_handle_recovery_enabled = enabled;
        self
    }

    pub fn with_multi_device_audience(mut self, audience: impl Into<String>) -> Self {
        self.multi_device_audience = Some(audience.into());
        self
    }

    /// Installs the trusted Host SPI used by External identity custody mode.
    #[cfg(feature = "provider-traits")]
    pub fn with_identity_custody_provider(
        mut self,
        provider: Arc<dyn crate::provider::IdentityCustody>,
    ) -> Self {
        self.identity_custody_provider = Some(IdentityCustodyProvider { inner: provider });
        self
    }
}

#[derive(Clone)]
pub(crate) struct IdentityVaultContext {
    policy: IdentitySecretStoragePolicy,
    vault: Arc<dyn SecretVault + Send + Sync>,
    workspace_id: String,
    vault_context_device_id: crate::ids::VaultContextDeviceId,
    anp_identity_root_key: Zeroizing<[u8; 32]>,
}

impl IdentityVaultContext {
    pub(crate) fn from_options(options: ImCoreSecretVaultOptions) -> crate::ImResult<Self> {
        let workspace_id = required_non_empty("workspace_id", options.workspace_id)?;
        let vault_context_device_id = crate::ids::VaultContextDeviceId::parse(options.device_id)?;
        if options.vault_dir.as_os_str().is_empty() {
            return Err(crate::ImError::invalid_input(
                Some("vault_dir".to_owned()),
                "vault directory must not be empty",
            ));
        }
        let anp_identity_root_key = Zeroizing::new(*options.root_key.expose_secret());
        let vault = Arc::new(FileSecretVault::new(
            options.root_key,
            FileSecretVaultStore::new(options.vault_dir),
        ));
        Ok(Self {
            policy: IdentitySecretStoragePolicy::FileCompat,
            vault,
            workspace_id,
            vault_context_device_id,
            anp_identity_root_key,
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

    pub(crate) fn vault_context_device_id(&self) -> &crate::ids::VaultContextDeviceId {
        &self.vault_context_device_id
    }

    pub(crate) fn anp_identity_root_key(&self) -> [u8; 32] {
        *self.anp_identity_root_key
    }
}

impl fmt::Debug for IdentityVaultContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IdentityVaultContext")
            .field("policy", &self.policy)
            .field("vault", &"<redacted-secret-vault>")
            .field("workspace_id", &self.workspace_id)
            .field("vault_context_device_id", &self.vault_context_device_id)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_e2ee_v2_rollout_gate_is_local_and_default_off() {
        let default_options = ImCoreOpenOptions::default();
        assert!(!default_options.multi_device_group_e2ee_enabled);

        let enabled = ImCoreOpenOptions::default().with_multi_device_group_e2ee_enabled(true);
        assert!(enabled.multi_device_group_e2ee_enabled);
    }

    #[test]
    fn direct_e2ee_v2_rollout_gate_is_local_and_default_off() {
        let default_options = ImCoreOpenOptions::default();
        assert!(!default_options.multi_device_direct_e2ee_enabled);

        let enabled = ImCoreOpenOptions::default().with_multi_device_direct_e2ee_enabled(true);
        assert!(enabled.multi_device_direct_e2ee_enabled);
    }

    #[test]
    fn handle_recovery_rollout_gate_is_local_and_default_off() {
        let default_options = ImCoreOpenOptions::default();
        assert!(!default_options.multi_device_handle_recovery_enabled);
        assert!(
            ImCoreOpenOptions::default()
                .with_multi_device_handle_recovery_enabled(true)
                .multi_device_handle_recovery_enabled
        );
    }
}
