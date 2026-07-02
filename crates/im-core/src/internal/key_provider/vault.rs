use std::fmt;
use std::sync::Arc;

use serde_json::Value;

use crate::internal::platform_secret::SecretBytes;
use crate::internal::secret_vault::policy::SecretAccessPolicy;
use crate::internal::secret_vault::record::{SecretMetadata, SecretRef};
use crate::internal::secret_vault::{SealSecretRequest, SecretVault};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VaultKeyMaterialRefs {
    pub(crate) default_signing_private: SecretRef,
    pub(crate) e2ee_agreement_private: SecretRef,
    pub(crate) auth_jwt: SecretRef,
}

pub(crate) struct VaultBackedKeyMaterialProvider {
    file_provider: super::FileBackedKeyMaterialProvider,
    vault: Arc<dyn SecretVault + Send + Sync>,
    refs: VaultKeyMaterialRefs,
}

impl VaultBackedKeyMaterialProvider {
    pub(crate) fn new(
        identity_dir: std::path::PathBuf,
        vault: Arc<dyn SecretVault + Send + Sync>,
        refs: VaultKeyMaterialRefs,
    ) -> Self {
        Self {
            file_provider: super::FileBackedKeyMaterialProvider::new(identity_dir),
            vault,
            refs,
        }
    }

    fn open_utf8_secret(&self, secret_ref: &SecretRef, path_kind: &str) -> crate::ImResult<String> {
        let secret = self.vault.open(secret_ref)?;
        let value = String::from_utf8(secret.expose_secret().to_vec()).map_err(|_| {
            crate::ImError::CredentialFileUnreadable {
                path_kind: path_kind.to_owned(),
                detail: "vault secret is not valid utf-8".to_owned(),
            }
        })?;
        if value.trim().is_empty() {
            return Err(crate::ImError::CredentialFileUnreadable {
                path_kind: path_kind.to_owned(),
                detail: "vault secret is empty".to_owned(),
            });
        }
        Ok(value)
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
}

impl fmt::Debug for VaultBackedKeyMaterialProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VaultBackedKeyMaterialProvider")
            .field("file_provider", &self.file_provider)
            .field("backend", &"vault-backed")
            .field("refs", &self.refs)
            .finish_non_exhaustive()
    }
}

impl super::KeyMaterialProvider for VaultBackedKeyMaterialProvider {
    fn did_document(&self) -> crate::ImResult<Value> {
        self.file_provider.did_document()
    }

    fn optional_did_document(&self) -> crate::ImResult<Option<Value>> {
        self.file_provider.optional_did_document()
    }

    fn default_signing_private_pem(&self) -> crate::ImResult<String> {
        self.open_utf8_secret(
            &self.refs.default_signing_private,
            "vault_default_signing_private_key",
        )
    }

    fn e2ee_agreement_private_pem(&self) -> crate::ImResult<String> {
        self.open_utf8_secret(
            &self.refs.e2ee_agreement_private,
            "vault_e2ee_agreement_private_key",
        )
    }

    fn auth_state(&self) -> crate::ImResult<crate::internal::auth::state::AuthStateSnapshot> {
        let secret = self.vault.open(&self.refs.auth_jwt)?;
        crate::internal::auth::state::parse_auth_state(secret.expose_secret())
    }

    fn valid_auth_token(&self) -> crate::ImResult<Option<String>> {
        let snapshot = self.auth_state()?;
        if snapshot.has_valid_token {
            Ok(snapshot.bearer_token)
        } else {
            Ok(None)
        }
    }

    fn persist_auth_token(&self, token: &str) -> crate::ImResult<()> {
        let raw = crate::internal::auth::state::auth_state_json_for_token(token)?;
        self.vault.seal(SealSecretRequest {
            metadata: Self::metadata_from_ref(&self.refs.auth_jwt),
            plaintext: SecretBytes::from_vec(raw),
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::internal::key_provider::KeyMaterialProvider;
    use crate::internal::platform_secret::DeviceVaultRootKey;
    use crate::internal::secret_vault::record::SecretKind;
    use crate::internal::secret_vault::{FileSecretVault, FileSecretVaultStore};
    use serde_json::json;

    #[test]
    fn identity_key_provider_vault_reads_secret_material_without_debug_leak() {
        let root = tempfile::tempdir().unwrap();
        let identity_dir = root.path().join("identity");
        std::fs::create_dir_all(&identity_dir).unwrap();
        std::fs::write(
            identity_dir.join("did_document.json"),
            serde_json::to_vec(&json!({"id": "did:example:alice"})).unwrap(),
        )
        .unwrap();
        let vault = Arc::new(FileSecretVault::new(
            DeviceVaultRootKey::from_bytes([4_u8; 32]),
            FileSecretVaultStore::new(root.path().join("vault")),
        ));
        let signing_ref = vault
            .seal(SealSecretRequest {
                metadata: test_metadata(SecretKind::IdentityRootPrivate, "key-1"),
                plaintext: SecretBytes::from_vec(b"signing-secret".to_vec()),
            })
            .unwrap();
        let agreement_ref = vault
            .seal(SealSecretRequest {
                metadata: test_metadata(SecretKind::IdentityE2eeAgreementPrivate, "key-3"),
                plaintext: SecretBytes::from_vec(b"agreement-secret".to_vec()),
            })
            .unwrap();
        let auth_ref = vault
            .seal(SealSecretRequest {
                metadata: test_metadata(SecretKind::AuthJwt, "auth.json"),
                plaintext: SecretBytes::from_vec(
                    crate::internal::auth::state::auth_state_json_for_token("token-secret")
                        .unwrap(),
                ),
            })
            .unwrap();
        let provider = VaultBackedKeyMaterialProvider::new(
            identity_dir,
            vault,
            VaultKeyMaterialRefs {
                default_signing_private: signing_ref,
                e2ee_agreement_private: agreement_ref,
                auth_jwt: auth_ref,
            },
        );

        assert_eq!(
            provider.default_signing_private_pem().unwrap(),
            "signing-secret"
        );
        assert_eq!(
            provider.e2ee_agreement_private_pem().unwrap(),
            "agreement-secret"
        );
        assert_eq!(
            provider.valid_auth_token().unwrap().as_deref(),
            Some("token-secret")
        );
        let debug = format!("{provider:?}");
        assert!(!debug.contains("signing-secret"));
        assert!(!debug.contains("agreement-secret"));
        assert!(!debug.contains("token-secret"));
    }

    fn test_metadata(kind: SecretKind, key_id: &str) -> SecretMetadata {
        SecretMetadata {
            workspace_id: "workspace-a".to_owned(),
            device_id: "device-a".to_owned(),
            identity_id: Some("identity-a".to_owned()),
            did: Some("did:example:alice".to_owned()),
            kind,
            key_id: key_id.to_owned(),
            key_version: 1,
            policy: SecretAccessPolicy::no_prompt_local_secret(),
        }
    }
}
