use anyhow::{bail, Context, Result};
use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine as _,
};
use im_core::vault::{
    DeviceVaultRootKey, FileSecretVault, FileSecretVaultStore, SealSecretRequest, SecretBytes,
    SecretRef, SecretVault, DEVICE_VAULT_ROOT_KEY_LEN,
};

use crate::DaemonConfig;

pub const DAEMON_VAULT_ROOT_KEY_ENV: &str = "AWIKI_DAEMON_VAULT_ROOT_KEY_B64";

#[derive(Debug)]
pub struct DaemonSecretVault {
    inner: FileSecretVault,
}

impl DaemonSecretVault {
    pub fn from_config_and_env(config: &DaemonConfig) -> Result<Self> {
        let raw = std::env::var(DAEMON_VAULT_ROOT_KEY_ENV)
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());
        let root_key = parse_root_key(raw.as_deref())?;
        Ok(Self::from_root_key(config, root_key))
    }

    pub fn from_root_key_bytes(
        config: &DaemonConfig,
        bytes: [u8; DEVICE_VAULT_ROOT_KEY_LEN],
    ) -> Self {
        Self::from_root_key(config, DeviceVaultRootKey::from_bytes(bytes))
    }

    fn from_root_key(config: &DaemonConfig, root_key: DeviceVaultRootKey) -> Self {
        Self {
            inner: FileSecretVault::new(
                root_key,
                FileSecretVaultStore::new(config.secret_vault_dir.clone()),
            ),
        }
    }

    pub fn seal(&self, request: SealSecretRequest) -> im_core::ImResult<SecretRef> {
        let secret_ref = self.inner.seal(request)?;
        self.inner.open(&secret_ref)?;
        Ok(secret_ref)
    }

    pub fn open(&self, secret_ref: &SecretRef) -> im_core::ImResult<SecretBytes> {
        self.inner.open(secret_ref)
    }

    pub fn list(&self) -> im_core::ImResult<Vec<SecretRef>> {
        self.inner.list()
    }
}

pub fn parse_root_key(raw: Option<&str>) -> Result<DeviceVaultRootKey> {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        bail!(
            "{DAEMON_VAULT_ROOT_KEY_ENV} is required for daemon secret vault persistence; refusing plaintext fallback"
        );
    };
    let decoded = URL_SAFE_NO_PAD
        .decode(raw)
        .or_else(|_| STANDARD.decode(raw))
        .with_context(|| format!("{DAEMON_VAULT_ROOT_KEY_ENV} must be base64url/base64"))?;
    if decoded.len() != DEVICE_VAULT_ROOT_KEY_LEN {
        bail!("{DAEMON_VAULT_ROOT_KEY_ENV} must decode to {DEVICE_VAULT_ROOT_KEY_LEN} bytes");
    }
    let mut bytes = [0_u8; DEVICE_VAULT_ROOT_KEY_LEN];
    bytes.copy_from_slice(&decoded);
    Ok(DeviceVaultRootKey::from_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use im_core::vault::{SecretAccessPolicy, SecretKind, SecretMetadata};

    #[test]
    fn parse_root_key_accepts_base64url_no_pad_32_bytes() {
        let raw = URL_SAFE_NO_PAD.encode([7_u8; DEVICE_VAULT_ROOT_KEY_LEN]);

        let root_key = parse_root_key(Some(&raw)).unwrap();

        assert!(!format!("{root_key:?}").contains(&raw));
    }

    #[test]
    fn parse_root_key_rejects_missing_value_without_plaintext_fallback() {
        let err = parse_root_key(None).unwrap_err();

        assert!(err.to_string().contains("refusing plaintext fallback"));
        assert!(!err.to_string().contains("private"));
    }

    #[test]
    fn parse_root_key_rejects_wrong_length() {
        let raw = URL_SAFE_NO_PAD.encode([3_u8; 31]);

        let err = parse_root_key(Some(&raw)).unwrap_err();

        assert!(err.to_string().contains("32 bytes"));
    }

    #[test]
    fn daemon_secret_vault_seals_opens_and_lists_secret() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        let vault = DaemonSecretVault::from_root_key_bytes(&config, [9_u8; 32]);

        let secret_ref = vault
            .seal(SealSecretRequest {
                metadata: test_metadata("daemon-auth-key"),
                plaintext: SecretBytes::from_vec(b"daemon-private-key".to_vec()),
            })
            .unwrap();
        let opened = vault.open(&secret_ref).unwrap();

        assert_eq!(opened.expose_secret(), b"daemon-private-key");
        assert_eq!(vault.list().unwrap(), vec![secret_ref]);
        assert!(config.secret_vault_dir.join("records").is_dir());
    }

    fn test_metadata(key_id: &str) -> SecretMetadata {
        SecretMetadata {
            workspace_id: "daemon-workspace".to_owned(),
            device_id: "daemon-device".to_owned(),
            identity_id: Some("daemon-agent".to_owned()),
            did: Some("did:wba:daemon.example".to_owned()),
            kind: SecretKind::IdentityDaemonPrivate,
            key_id: key_id.to_owned(),
            key_version: 1,
            policy: SecretAccessPolicy::no_prompt_local_secret(),
        }
    }
}
