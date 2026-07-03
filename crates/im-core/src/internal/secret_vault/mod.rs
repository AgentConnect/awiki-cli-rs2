mod crypto;
pub(crate) mod policy;
pub(crate) mod record;
mod store;

use crate::internal::platform_secret::{DeviceVaultRootKey, SecretBytes};

pub use self::policy::SecretAccessPolicy;
pub use self::record::{SecretKind, SecretMetadata, SecretRef};
pub use self::store::FileSecretVaultStore;

pub struct SealSecretRequest {
    pub metadata: SecretMetadata,
    pub plaintext: SecretBytes,
}

pub trait SecretVault {
    fn seal(&self, request: SealSecretRequest) -> crate::ImResult<SecretRef>;

    fn open(&self, secret_ref: &SecretRef) -> crate::ImResult<SecretBytes>;

    fn delete(&self, secret_ref: &SecretRef) -> crate::ImResult<()>;

    fn list(&self) -> crate::ImResult<Vec<SecretRef>>;
}

#[derive(Debug)]
pub struct FileSecretVault {
    root_key: DeviceVaultRootKey,
    store: FileSecretVaultStore,
}

impl FileSecretVault {
    pub fn new(root_key: DeviceVaultRootKey, store: FileSecretVaultStore) -> Self {
        Self { root_key, store }
    }

    pub fn store(&self) -> &FileSecretVaultStore {
        &self.store
    }
}

impl SecretVault for FileSecretVault {
    fn seal(&self, request: SealSecretRequest) -> crate::ImResult<SecretRef> {
        let record = crypto::seal_record(&self.root_key, request.metadata, &request.plaintext)?;
        self.store.put(&record)
    }

    fn open(&self, secret_ref: &SecretRef) -> crate::ImResult<SecretBytes> {
        let record = self.store.get(secret_ref)?;
        crypto::open_record(&self.root_key, &record)
    }

    fn delete(&self, secret_ref: &SecretRef) -> crate::ImResult<()> {
        self.store.delete(secret_ref)
    }

    fn list(&self) -> crate::ImResult<Vec<SecretRef>> {
        self.store.list()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::internal::platform_secret::DeviceVaultRootKey;
    use crate::internal::secret_vault::policy::SecretAccessPolicy;
    use crate::internal::secret_vault::record::{SecretKind, VaultSecretRecord};
    use std::fs;

    #[test]
    fn secret_vault_file_store_seals_opens_lists_and_deletes_secret() {
        let root = tempfile::tempdir().unwrap();
        let vault = test_vault(root.path().join("vault"), [3_u8; 32]);
        let metadata = test_metadata("workspace-a", "device-a");

        let secret_ref = vault
            .seal(SealSecretRequest {
                metadata,
                plaintext: SecretBytes::from_vec(b"private-key-pem".to_vec()),
            })
            .unwrap();
        let opened = vault.open(&secret_ref).unwrap();

        assert_eq!(opened.expose_secret(), b"private-key-pem");
        assert_eq!(vault.list().unwrap(), vec![secret_ref.clone()]);
        vault.delete(&secret_ref).unwrap();
        assert!(vault.list().unwrap().is_empty());
    }

    #[test]
    fn secret_vault_file_store_rejects_wrong_device_metadata_tamper() {
        let root = tempfile::tempdir().unwrap();
        let vault = test_vault(root.path().join("vault"), [3_u8; 32]);
        let secret_ref = vault
            .seal(SealSecretRequest {
                metadata: test_metadata("workspace-a", "device-a"),
                plaintext: SecretBytes::from_vec(b"private-key-pem".to_vec()),
            })
            .unwrap();
        let path = vault.store().record_path(&secret_ref);
        let mut record: VaultSecretRecord =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        record.device_id = "device-b".to_owned();
        fs::write(&path, serde_json::to_vec_pretty(&record).unwrap()).unwrap();

        let err = vault.open(&secret_ref).unwrap_err();

        assert_eq!(err, crate::ImError::PermissionDenied);
    }

    #[test]
    fn secret_vault_debug_redacts_record_secret_fields() {
        let root_key = DeviceVaultRootKey::from_bytes([3_u8; 32]);
        let metadata = test_metadata("workspace-a", "device-a");
        let plaintext = SecretBytes::from_vec(b"debug-private-key".to_vec());
        let record = crypto::seal_record(&root_key, metadata, &plaintext).unwrap();

        let debug = format!("{record:?}");

        assert!(!debug.contains(&record.ciphertext_b64u));
        assert!(!debug.contains(&record.nonce_b64u));
        assert!(!debug.contains("debug-private-key"));
        assert!(debug.contains("[REDACTED]"));
    }

    fn test_vault(path: std::path::PathBuf, key: [u8; 32]) -> FileSecretVault {
        FileSecretVault::new(
            DeviceVaultRootKey::from_bytes(key),
            FileSecretVaultStore::new(path),
        )
    }

    fn test_metadata(workspace_id: &str, device_id: &str) -> SecretMetadata {
        SecretMetadata {
            workspace_id: workspace_id.to_owned(),
            device_id: device_id.to_owned(),
            identity_id: Some("identity-a".to_owned()),
            did: Some("did:wba:alice@example.com".to_owned()),
            kind: SecretKind::IdentityRootPrivate,
            key_id: "key-1".to_owned(),
            key_version: 1,
            policy: SecretAccessPolicy::no_prompt_local_secret(),
        }
    }
}
