use anyhow::{bail, Context, Result};
use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine as _,
};
use hkdf::Hkdf;
use im_core::vault::{
    DeviceVaultRootKey, FileSecretVault, FileSecretVaultStore, SealIfAbsentResult,
    SealSecretRequest, SecretBytes, SecretRef, SecretVault, DEVICE_VAULT_ROOT_KEY_LEN,
};
use rand::rngs::OsRng;
use rand::RngCore;
use sha2::Sha256;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

use crate::DaemonConfig;

pub const DAEMON_VAULT_ROOT_KEY_ENV: &str = "AWIKI_DAEMON_VAULT_ROOT_KEY_B64";
const DAEMON_LOCAL_ROOT_KEY_FILE_NAME: &str = "root-key.b64u";
const ANP_IDENTITY_ROOT_KEY_INFO: &[u8] = b"awiki-daemon/anp-identity/root-key/v1";

#[derive(Debug)]
pub struct DaemonSecretVault {
    inner: FileSecretVault,
    anp_identity_root_key: Zeroizing<[u8; DEVICE_VAULT_ROOT_KEY_LEN]>,
}

impl DaemonSecretVault {
    pub fn from_config(config: &DaemonConfig) -> Result<Self> {
        Ok(Self::from_root_key_bytes(
            config,
            load_or_create_root_key_bytes(config)?,
        ))
    }

    pub fn from_config_and_env(config: &DaemonConfig) -> Result<Self> {
        let raw = std::env::var(DAEMON_VAULT_ROOT_KEY_ENV)
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());
        Ok(Self::from_root_key_bytes(
            config,
            parse_root_key_bytes_from_source(raw.as_deref(), DAEMON_VAULT_ROOT_KEY_ENV)?,
        ))
    }

    pub fn from_root_key_bytes(
        config: &DaemonConfig,
        mut bytes: [u8; DEVICE_VAULT_ROOT_KEY_LEN],
    ) -> Self {
        let mut anp_identity_root_key = Zeroizing::new([0_u8; DEVICE_VAULT_ROOT_KEY_LEN]);
        Hkdf::<Sha256>::new(None, &bytes)
            .expand(ANP_IDENTITY_ROOT_KEY_INFO, anp_identity_root_key.as_mut())
            .expect("32-byte HKDF output is valid");
        let root_key = DeviceVaultRootKey::from_bytes(bytes);
        bytes.fill(0);
        Self {
            inner: FileSecretVault::new(
                root_key,
                FileSecretVaultStore::new(config.secret_vault_dir.clone()),
            ),
            anp_identity_root_key,
        }
    }

    pub(crate) fn anp_identity_root_key(&self) -> [u8; DEVICE_VAULT_ROOT_KEY_LEN] {
        *self.anp_identity_root_key
    }

    pub fn seal(&self, request: SealSecretRequest) -> im_core::ImResult<SecretRef> {
        let secret_ref = self.inner.seal(request)?;
        self.inner.open(&secret_ref)?;
        Ok(secret_ref)
    }

    pub fn seal_if_absent(
        &self,
        request: SealSecretRequest,
    ) -> im_core::ImResult<SealIfAbsentResult> {
        let result = self.inner.seal_if_absent(request)?;
        if let SealIfAbsentResult::Sealed(secret_ref) = &result {
            if let Err(error) = self.inner.open(secret_ref) {
                let _ = self.inner.delete(secret_ref);
                return Err(error);
            }
        }
        Ok(result)
    }

    pub fn open(&self, secret_ref: &SecretRef) -> im_core::ImResult<SecretBytes> {
        self.inner.open(secret_ref)
    }

    pub fn delete(&self, secret_ref: &SecretRef) -> im_core::ImResult<()> {
        self.inner.delete(secret_ref)
    }

    pub fn list(&self) -> im_core::ImResult<Vec<SecretRef>> {
        self.inner.list()
    }
}

pub fn parse_root_key(raw: Option<&str>) -> Result<DeviceVaultRootKey> {
    parse_root_key_bytes_from_source(raw, DAEMON_VAULT_ROOT_KEY_ENV)
        .map(DeviceVaultRootKey::from_bytes)
}

fn local_root_key_file(config: &DaemonConfig) -> PathBuf {
    config
        .secret_vault_dir
        .join(DAEMON_LOCAL_ROOT_KEY_FILE_NAME)
}

fn load_or_create_root_key_bytes(config: &DaemonConfig) -> Result<[u8; DEVICE_VAULT_ROOT_KEY_LEN]> {
    let raw = std::env::var(DAEMON_VAULT_ROOT_KEY_ENV)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    if raw.is_some() {
        return parse_root_key_bytes_from_source(raw.as_deref(), DAEMON_VAULT_ROOT_KEY_ENV);
    }
    load_or_create_local_root_key(&local_root_key_file(config))
}

fn load_or_create_local_root_key(path: &Path) -> Result<[u8; DEVICE_VAULT_ROOT_KEY_LEN]> {
    if let Some(raw) = read_local_root_key_file(path)? {
        return parse_root_key_bytes_from_source(Some(&raw), "daemon local vault root key file");
    }

    let mut bytes = [0_u8; DEVICE_VAULT_ROOT_KEY_LEN];
    OsRng.fill_bytes(&mut bytes);
    match write_local_root_key_file(path, &bytes)? {
        LocalRootKeyWriteOutcome::Created => Ok(bytes),
        LocalRootKeyWriteOutcome::AlreadyExists => {
            bytes.fill(0);
            let raw = read_local_root_key_file(path)?
                .context("daemon local vault root key file appeared but could not be read")?;
            parse_root_key_bytes_from_source(Some(&raw), "daemon local vault root key file")
        }
    }
}

fn read_local_root_key_file(path: &Path) -> Result<Option<String>> {
    reject_root_key_symlink(path)?;
    match fs::read_to_string(path) {
        Ok(raw) => {
            if let Some(parent) = path.parent() {
                reject_root_key_symlink(parent)?;
                set_private_dir_mode(parent).with_context(|| {
                    format!(
                        "secure daemon vault root key directory {}",
                        parent.display()
                    )
                })?;
            }
            set_private_file_mode(path)
                .with_context(|| format!("secure daemon vault root key file {}", path.display()))?;
            Ok(Some(raw))
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => {
            Err(err).with_context(|| format!("read daemon vault root key file {}", path.display()))
        }
    }
}

enum LocalRootKeyWriteOutcome {
    Created,
    AlreadyExists,
}

fn write_local_root_key_file(
    path: &Path,
    root_key: &[u8; DEVICE_VAULT_ROOT_KEY_LEN],
) -> Result<LocalRootKeyWriteOutcome> {
    if let Some(parent) = path.parent() {
        reject_root_key_symlink(parent)?;
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "create daemon vault root key directory {}",
                parent.display()
            )
        })?;
        reject_root_key_symlink(parent)?;
        set_private_dir_mode(parent).with_context(|| {
            format!(
                "secure daemon vault root key directory {}",
                parent.display()
            )
        })?;
    }
    reject_root_key_symlink(path)?;
    let encoded = URL_SAFE_NO_PAD.encode(root_key);
    let mut file = match create_private_file(path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
            return Ok(LocalRootKeyWriteOutcome::AlreadyExists);
        }
        Err(err) => {
            return Err(err)
                .with_context(|| format!("create daemon vault root key file {}", path.display()));
        }
    };
    file.write_all(encoded.as_bytes())
        .with_context(|| format!("write daemon vault root key file {}", path.display()))?;
    file.write_all(b"\n")
        .with_context(|| format!("write daemon vault root key file {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("sync daemon vault root key file {}", path.display()))?;
    set_private_file_mode(path)
        .with_context(|| format!("secure daemon vault root key file {}", path.display()))?;
    Ok(LocalRootKeyWriteOutcome::Created)
}

fn reject_root_key_symlink(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            bail!(
                "refusing to use daemon vault root key symlink {}",
                path.display()
            );
        }
        Ok(_) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err)
            .with_context(|| format!("inspect daemon vault root key file {}", path.display())),
    }
}

#[cfg(unix)]
fn create_private_file(path: &Path) -> std::io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn create_private_file(path: &Path) -> std::io::Result<fs::File> {
    fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
}

#[cfg(unix)]
fn set_private_dir_mode(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .with_context(|| format!("chmod 0700 {}", path.display()))
}

#[cfg(not(unix))]
fn set_private_dir_mode(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn set_private_file_mode(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("chmod 0600 {}", path.display()))
}

#[cfg(not(unix))]
fn set_private_file_mode(_path: &Path) -> Result<()> {
    Ok(())
}

fn parse_root_key_bytes_from_source(
    raw: Option<&str>,
    source_name: &str,
) -> Result<[u8; DEVICE_VAULT_ROOT_KEY_LEN]> {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        bail!(
            "{source_name} is required for daemon secret vault persistence; refusing plaintext fallback"
        );
    };
    let decoded = URL_SAFE_NO_PAD
        .decode(raw)
        .or_else(|_| STANDARD.decode(raw))
        .with_context(|| format!("{source_name} must be base64url/base64"))?;
    if decoded.len() != DEVICE_VAULT_ROOT_KEY_LEN {
        bail!("{source_name} must decode to {DEVICE_VAULT_ROOT_KEY_LEN} bytes");
    }
    let mut bytes = [0_u8; DEVICE_VAULT_ROOT_KEY_LEN];
    bytes.copy_from_slice(&decoded);
    Ok(bytes)
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

    #[test]
    fn identity_custody_uses_a_stable_domain_separated_root_key() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        let source = [21_u8; DEVICE_VAULT_ROOT_KEY_LEN];
        let first = DaemonSecretVault::from_root_key_bytes(&config, source);
        let second = DaemonSecretVault::from_root_key_bytes(&config, source);

        assert_eq!(
            first.anp_identity_root_key(),
            second.anp_identity_root_key()
        );
        assert_ne!(first.anp_identity_root_key(), source);
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
