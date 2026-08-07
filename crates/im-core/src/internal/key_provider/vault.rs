use std::fmt;
use std::fs::{self, File};
use std::sync::{Arc, RwLock};

use crate::internal::platform_secret::SecretBytes;
use crate::internal::secret_vault::policy::SecretAccessPolicy;
use crate::internal::secret_vault::record::{SecretKind, SecretMetadata, SecretRef};
use crate::internal::secret_vault::{SealSecretRequest, SecretVault};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LegacyVaultKeyMaterialRefs {
    pub(crate) default_signing_private: SecretRef,
    pub(crate) e2ee_agreement_private: SecretRef,
    pub(crate) auth_jwt: SecretRef,
}

/// vNext key refs keep device request signing separate from optional DID root
/// control. This is side-by-side with legacy vault migration metadata so old
/// records continue to deserialize unchanged.
#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct VNextVaultKeyMaterialRefs {
    pub(crate) device_request_signing_private: SecretRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) did_document_root_private: Option<SecretRef>,
    pub(crate) e2ee_agreement_private: SecretRef,
    pub(crate) auth_jwt: SecretRef,
}

impl fmt::Debug for VNextVaultKeyMaterialRefs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VNextVaultKeyMaterialRefs")
            .field("refs", &"<redacted-secret-refs>")
            .field(
                "has_did_document_root_private",
                &self.did_document_root_private.is_some(),
            )
            .finish()
    }
}

enum VaultKeyRoleRefs {
    Legacy(LegacyVaultKeyMaterialRefs),
    VNext(VNextVaultKeyMaterialRefs),
}

pub(crate) struct VaultBackedKeyMaterialProvider {
    file_provider: super::FileBackedKeyMaterialProvider,
    vault: Arc<dyn SecretVault + Send + Sync>,
    refs: VaultKeyRoleRefs,
    did_document_root_private_ref: RwLock<Option<SecretRef>>,
    auth_jwt_ref: RwLock<SecretRef>,
}

impl VaultBackedKeyMaterialProvider {
    pub(crate) fn new(
        identity_dir: std::path::PathBuf,
        vault: Arc<dyn SecretVault + Send + Sync>,
        refs: LegacyVaultKeyMaterialRefs,
    ) -> Self {
        let auth_jwt_ref = refs.auth_jwt.clone();
        Self {
            file_provider: super::FileBackedKeyMaterialProvider::new(identity_dir),
            vault,
            refs: VaultKeyRoleRefs::Legacy(refs),
            did_document_root_private_ref: RwLock::new(None),
            auth_jwt_ref: RwLock::new(auth_jwt_ref),
        }
    }

    pub(crate) fn new_vnext(
        identity_dir: std::path::PathBuf,
        vault: Arc<dyn SecretVault + Send + Sync>,
        refs: VNextVaultKeyMaterialRefs,
    ) -> Self {
        let auth_jwt_ref = refs.auth_jwt.clone();
        let did_document_root_private_ref = refs.did_document_root_private.clone();
        Self {
            file_provider: super::FileBackedKeyMaterialProvider::new(identity_dir),
            vault,
            refs: VaultKeyRoleRefs::VNext(refs),
            did_document_root_private_ref: RwLock::new(did_document_root_private_ref),
            auth_jwt_ref: RwLock::new(auth_jwt_ref),
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

    fn open_utf8_secret_for_kind(
        &self,
        secret_ref: &SecretRef,
        expected_kind: SecretKind,
        role: &str,
    ) -> crate::ImResult<String> {
        if secret_ref.kind != expected_kind {
            return Err(crate::ImError::IdentityNotReady {
                identity: secret_ref
                    .did
                    .clone()
                    .or_else(|| secret_ref.identity_id.clone())
                    .unwrap_or_else(|| "vault-backed".to_owned()),
                missing: vec![format!("{role}_secret_kind")],
            });
        }
        self.open_utf8_secret(secret_ref, role)
    }

    fn legacy_key1_role_adapter(
        &self,
        refs: &LegacyVaultKeyMaterialRefs,
    ) -> crate::ImResult<super::LegacyKey1RoleAdapter> {
        self.open_utf8_secret_for_kind(
            &refs.default_signing_private,
            SecretKind::IdentityRootPrivate,
            "legacy_key1_private_key",
        )
        .map(super::LegacyKey1RoleAdapter::new)
    }

    fn e2ee_agreement_private_ref(&self) -> &SecretRef {
        match &self.refs {
            VaultKeyRoleRefs::Legacy(refs) => &refs.e2ee_agreement_private,
            VaultKeyRoleRefs::VNext(refs) => &refs.e2ee_agreement_private,
        }
    }

    fn auth_jwt_ref(&self) -> crate::ImResult<SecretRef> {
        self.auth_jwt_ref
            .read()
            .map(|secret_ref| secret_ref.clone())
            .map_err(|_| crate::ImError::LocalStateUnavailable {
                detail: "vault auth ref lock poisoned".to_owned(),
            })
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
            .field("refs", &"<redacted-secret-refs>")
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

    fn device_request_signing_private_pem(&self) -> crate::ImResult<String> {
        match &self.refs {
            VaultKeyRoleRefs::Legacy(refs) => Ok(self
                .legacy_key1_role_adapter(refs)?
                .device_request_signing_private_pem()),
            VaultKeyRoleRefs::VNext(refs) => self.open_utf8_secret_for_kind(
                &refs.device_request_signing_private,
                SecretKind::IdentityDeviceSigningPrivate,
                "device_request_signing_private_key",
            ),
        }
    }

    fn device_request_signing_material(
        &self,
    ) -> crate::ImResult<super::DeviceRequestSigningMaterial> {
        match &self.refs {
            VaultKeyRoleRefs::Legacy(_) => Ok(super::DeviceRequestSigningMaterial {
                key_id: super::file::request_signing_key_id(&self.did_document()?)?,
                private_key_pem: self.device_request_signing_private_pem()?,
            }),
            VaultKeyRoleRefs::VNext(refs) => Ok(super::DeviceRequestSigningMaterial {
                key_id: refs.device_request_signing_private.key_id.clone(),
                private_key_pem: self.device_request_signing_private_pem()?,
            }),
        }
    }

    fn did_document_root_private_pem(&self) -> crate::ImResult<String> {
        match &self.refs {
            VaultKeyRoleRefs::Legacy(refs) => Ok(self
                .legacy_key1_role_adapter(refs)?
                .did_document_root_private_pem()),
            VaultKeyRoleRefs::VNext(refs) => {
                let secret_ref = self
                    .did_document_root_private_ref
                    .read()
                    .map_err(|_| crate::ImError::LocalStateUnavailable {
                        detail: "vault root ref lock poisoned".to_owned(),
                    })?
                    .clone()
                    .ok_or_else(|| crate::ImError::IdentityNotReady {
                        identity: refs
                            .device_request_signing_private
                            .did
                            .clone()
                            .or_else(|| refs.device_request_signing_private.identity_id.clone())
                            .unwrap_or_else(|| "vault-backed".to_owned()),
                        missing: vec!["did_document_root_private_key".to_owned()],
                    })?;
                self.open_utf8_secret_for_kind(
                    &secret_ref,
                    SecretKind::IdentityRootPrivate,
                    "did_document_root_private_key",
                )
            }
        }
    }

    fn e2ee_agreement_private_pem(&self) -> crate::ImResult<String> {
        self.open_utf8_secret(
            self.e2ee_agreement_private_ref(),
            "vault_e2ee_agreement_private_key",
        )
    }

    fn auth_state(&self) -> crate::ImResult<crate::internal::auth::state::AuthStateSnapshot> {
        let auth_ref = self.auth_jwt_ref()?;
        let secret = self.vault.open(&auth_ref)?;
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
        // A normal access-token refresh updates the authoritative record in
        // place. Keep the read guard until the replacement is verified so a
        // concurrent root-import ref advance cannot redirect this write to a
        // superseded SecretRef.
        let auth_ref =
            self.auth_jwt_ref
                .read()
                .map_err(|_| crate::ImError::LocalStateUnavailable {
                    detail: "vault auth ref lock poisoned".to_owned(),
                })?;
        if auth_ref.kind != SecretKind::AuthJwt {
            return Err(crate::ImError::PermissionDenied);
        }
        let raw = crate::internal::auth::state::auth_state_json_for_token(token)?;
        let candidate = crate::internal::auth::state::parse_auth_state(&raw)?;
        let sealed = self.vault.seal(SealSecretRequest {
            metadata: Self::metadata_from_ref(&auth_ref),
            plaintext: SecretBytes::from_vec(raw),
        })?;
        if sealed != *auth_ref {
            return Err(crate::ImError::PermissionDenied);
        }
        let opened = self.vault.open(&sealed)?;
        let persisted = crate::internal::auth::state::parse_auth_state(opened.expose_secret())?;
        if persisted.bearer_token.as_deref() != Some(token.trim())
            || persisted.expires_at != candidate.expires_at
        {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(())
    }

    fn advance_vault_auth_ref(&self, committed: &SecretRef) -> crate::ImResult<()> {
        let mut current =
            self.auth_jwt_ref
                .write()
                .map_err(|_| crate::ImError::LocalStateUnavailable {
                    detail: "vault auth ref lock poisoned".to_owned(),
                })?;
        if committed.workspace_id != current.workspace_id
            || committed.device_id != current.device_id
            || committed.identity_id != current.identity_id
            || committed.did != current.did
            || committed.kind != SecretKind::AuthJwt
            || current.kind != SecretKind::AuthJwt
            || committed.key_id != current.key_id
            || committed.key_version < current.key_version
        {
            return Err(crate::ImError::PermissionDenied);
        }
        // Validate the target before exposing it to any subsequent request.
        let opened = self.vault.open(committed)?;
        let snapshot = crate::internal::auth::state::parse_auth_state(opened.expose_secret())?;
        if !snapshot.has_token {
            return Err(crate::ImError::PermissionDenied);
        }
        if *current == *committed {
            return Ok(());
        }
        *current = committed.clone();
        Ok(())
    }

    fn advance_vault_root_ref(&self, committed: &SecretRef) -> crate::ImResult<()> {
        let VaultKeyRoleRefs::VNext(refs) = &self.refs else {
            return Err(crate::ImError::PermissionDenied);
        };
        let binding = &refs.device_request_signing_private;
        let did = binding
            .did
            .as_deref()
            .ok_or(crate::ImError::PermissionDenied)?;
        let expected_key_id = format!("{did}#{}", anp::authentication::VM_KEY_AUTH);
        if committed.workspace_id != binding.workspace_id
            || committed.device_id != binding.device_id
            || committed.identity_id != binding.identity_id
            || committed.did != binding.did
            || committed.kind != SecretKind::IdentityRootPrivate
            || committed.key_id != expected_key_id
            || committed.key_version != 1
        {
            return Err(crate::ImError::PermissionDenied);
        }
        let mut current = self.did_document_root_private_ref.write().map_err(|_| {
            crate::ImError::LocalStateUnavailable {
                detail: "vault root ref lock poisoned".to_owned(),
            }
        })?;
        if let Some(existing) = current.as_ref() {
            if existing.workspace_id != committed.workspace_id
                || existing.device_id != committed.device_id
                || existing.identity_id != committed.identity_id
                || existing.did != committed.did
                || existing.kind != SecretKind::IdentityRootPrivate
                || existing.key_id != committed.key_id
                || committed.key_version < existing.key_version
            {
                return Err(crate::ImError::PermissionDenied);
            }
        }
        // Validate and parse the target before exposing it to management code.
        let root_pem = self.open_utf8_secret_for_kind(
            committed,
            SecretKind::IdentityRootPrivate,
            "did_document_root_private_key",
        )?;
        if !matches!(
            anp::PrivateKeyMaterial::from_pem(&root_pem),
            Ok(anp::PrivateKeyMaterial::Ed25519(_))
        ) {
            return Err(crate::ImError::PermissionDenied);
        }
        if current.as_ref() == Some(committed) {
            return Ok(());
        }
        *current = Some(committed.clone());
        Ok(())
    }
}

fn set_private_lock_file_mode(file: &File) -> crate::ImResult<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    let _ = file;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::internal::key_provider::KeyMaterialProvider;
    use crate::internal::platform_secret::DeviceVaultRootKey;
    use crate::internal::secret_vault::record::SecretKind;
    use crate::internal::secret_vault::{FileSecretVault, FileSecretVaultStore};
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
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
            LegacyVaultKeyMaterialRefs {
                default_signing_private: signing_ref,
                e2ee_agreement_private: agreement_ref,
                auth_jwt: auth_ref,
            },
        );

        assert_eq!(
            provider.device_request_signing_private_pem().unwrap(),
            "signing-secret"
        );
        assert_eq!(
            provider.did_document_root_private_pem().unwrap(),
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

    #[test]
    fn secret_kind_keeps_legacy_root_deserialization_and_adds_device_signing() {
        let legacy: SecretKind = serde_json::from_str(r#""identity_root_private""#).unwrap();
        let device: SecretKind =
            serde_json::from_str(r#""identity_device_signing_private""#).unwrap();

        assert_eq!(legacy, SecretKind::IdentityRootPrivate);
        assert_eq!(legacy.as_str(), "identity.root.private");
        assert_eq!(device, SecretKind::IdentityDeviceSigningPrivate);
        assert_eq!(device.as_str(), "identity.device.signing.private");
    }

    #[test]
    fn vnext_member_uses_device_signing_without_did_root() {
        let root = tempfile::tempdir().unwrap();
        let identity_dir = root.path().join("identity");
        std::fs::create_dir_all(&identity_dir).unwrap();
        let bundle = anp::authentication::create_did_wba_document(
            "member.example",
            anp::authentication::DidDocumentOptions::default(),
        )
        .unwrap();
        let device_signing_pem = bundle.keys["key-1"].private_key_pem.clone();
        std::fs::write(
            identity_dir.join("did_document.json"),
            serde_json::to_vec(&bundle.did_document).unwrap(),
        )
        .unwrap();
        let vault = Arc::new(FileSecretVault::new(
            DeviceVaultRootKey::from_bytes([5_u8; 32]),
            FileSecretVaultStore::new(root.path().join("vault")),
        ));
        let device_signing_ref = seal_test_secret(
            vault.as_ref(),
            SecretKind::IdentityDeviceSigningPrivate,
            "member-sign",
            device_signing_pem.as_bytes(),
        );
        let agreement_ref = seal_test_secret(
            vault.as_ref(),
            SecretKind::IdentityE2eeAgreementPrivate,
            "member-e2ee",
            b"agreement-secret",
        );
        let auth_ref = seal_test_secret(
            vault.as_ref(),
            SecretKind::AuthJwt,
            "auth.json",
            &crate::internal::auth::state::auth_state_json_for_token("token-secret").unwrap(),
        );
        let refs = VNextVaultKeyMaterialRefs {
            device_request_signing_private: device_signing_ref,
            did_document_root_private: None,
            e2ee_agreement_private: agreement_ref,
            auth_jwt: auth_ref,
        };
        let serialized_refs = serde_json::to_value(&refs).unwrap();
        assert!(serialized_refs.get("did_document_root_private").is_none());
        let refs: VNextVaultKeyMaterialRefs = serde_json::from_value(serialized_refs).unwrap();
        let provider = Arc::new(VaultBackedKeyMaterialProvider::new_vnext(
            identity_dir,
            vault,
            refs,
        ));

        assert_eq!(
            provider.device_request_signing_private_pem().unwrap(),
            device_signing_pem
        );
        assert!(matches!(
            provider.did_document_root_private_pem(),
            Err(crate::ImError::IdentityNotReady { missing, .. })
                if missing == vec!["did_document_root_private_key"]
        ));
        assert_eq!(
            provider.valid_auth_token().unwrap().as_deref(),
            Some("token-secret")
        );
        let mut auth = crate::internal::key_provider::ProviderBackedDidAuth::new(
            provider.clone(),
            anp::authentication::AuthMode::HttpSignatures,
        );
        let headers = auth
            .get_auth_header(
                "https://api.member.example/messages",
                false,
                "POST",
                None,
                Some(br#"{"message":"hello"}"#),
            )
            .unwrap();
        assert!(headers.contains_key("Signature-Input"));
        assert!(headers.contains_key("Signature"));
        assert!(headers["Signature-Input"].contains("keyid=\"member-sign\""));
        let debug = format!("{provider:?}");
        assert!(!debug.contains(&device_signing_pem));
        assert!(!debug.contains("agreement-secret"));
        assert!(!debug.contains("token-secret"));
    }

    #[test]
    fn live_vnext_provider_advances_to_committed_versioned_auth_ref() {
        let root = tempfile::tempdir().unwrap();
        let identity_dir = root.path().join("identity");
        std::fs::create_dir_all(&identity_dir).unwrap();
        let vault = Arc::new(FileSecretVault::new(
            DeviceVaultRootKey::from_bytes([7_u8; 32]),
            FileSecretVaultStore::new(root.path().join("vault")),
        ));
        let device_ref = seal_test_secret(
            vault.as_ref(),
            SecretKind::IdentityDeviceSigningPrivate,
            "device-sign",
            b"device-secret",
        );
        let agreement_ref = seal_test_secret(
            vault.as_ref(),
            SecretKind::IdentityE2eeAgreementPrivate,
            "device-e2ee",
            b"agreement-secret",
        );
        let old_auth_ref = seal_test_secret(
            vault.as_ref(),
            SecretKind::AuthJwt,
            "auth.json",
            &crate::internal::auth::state::auth_state_json_for_token("old-token").unwrap(),
        );
        let mut new_metadata = VaultBackedKeyMaterialProvider::metadata_from_ref(&old_auth_ref);
        new_metadata.key_version += 1;
        let new_auth_ref = vault
            .seal(SealSecretRequest {
                metadata: new_metadata,
                plaintext: SecretBytes::from_vec(
                    crate::internal::auth::state::auth_state_json_for_token("new-token").unwrap(),
                ),
            })
            .unwrap();
        let provider = VaultBackedKeyMaterialProvider::new_vnext(
            identity_dir,
            vault,
            VNextVaultKeyMaterialRefs {
                device_request_signing_private: device_ref,
                did_document_root_private: None,
                e2ee_agreement_private: agreement_ref,
                auth_jwt: old_auth_ref.clone(),
            },
        );

        assert_eq!(
            provider.valid_auth_token().unwrap().as_deref(),
            Some("old-token")
        );
        provider.advance_vault_auth_ref(&new_auth_ref).unwrap();
        assert_eq!(
            provider.valid_auth_token().unwrap().as_deref(),
            Some("new-token")
        );
        assert!(provider.advance_vault_auth_ref(&old_auth_ref).is_err());
        let mut missing_ref = new_auth_ref;
        missing_ref.key_version += 1;
        assert!(provider.advance_vault_auth_ref(&missing_ref).is_err());
        assert_eq!(
            provider.valid_auth_token().unwrap().as_deref(),
            Some("new-token")
        );
    }

    #[test]
    fn vnext_access_token_refresh_preserves_committed_auth_ref() {
        let root = tempfile::tempdir().unwrap();
        let identity_dir = root.path().join("identity");
        std::fs::create_dir_all(&identity_dir).unwrap();
        let vault = Arc::new(FileSecretVault::new(
            DeviceVaultRootKey::from_bytes([9_u8; 32]),
            FileSecretVaultStore::new(root.path().join("vault")),
        ));
        let device_ref = seal_test_secret(
            vault.as_ref(),
            SecretKind::IdentityDeviceSigningPrivate,
            "device-sign",
            b"device-secret",
        );
        let agreement_ref = seal_test_secret(
            vault.as_ref(),
            SecretKind::IdentityE2eeAgreementPrivate,
            "device-e2ee",
            b"agreement-secret",
        );
        let auth_ref = seal_test_secret(
            vault.as_ref(),
            SecretKind::AuthJwt,
            "auth.json",
            &crate::internal::auth::state::auth_state_json_for_token("expired-access-token")
                .unwrap(),
        );
        let provider = VaultBackedKeyMaterialProvider::new_vnext(
            identity_dir,
            vault.clone(),
            VNextVaultKeyMaterialRefs {
                device_request_signing_private: device_ref,
                did_document_root_private: None,
                e2ee_agreement_private: agreement_ref,
                auth_jwt: auth_ref.clone(),
            },
        );
        let refreshed_access = test_jwt(4_102_444_800);

        provider.persist_auth_token(&refreshed_access).unwrap();

        assert_eq!(provider.auth_jwt_ref().unwrap(), auth_ref);
        assert_eq!(
            vault
                .list()
                .unwrap()
                .into_iter()
                .filter(|secret_ref| secret_ref.kind == SecretKind::AuthJwt)
                .collect::<Vec<_>>(),
            vec![auth_ref.clone()]
        );
        let opened = vault.open(&auth_ref).unwrap();
        let refreshed =
            crate::internal::auth::state::parse_auth_state(opened.expose_secret()).unwrap();
        assert_eq!(
            refreshed.bearer_token.as_deref(),
            Some(refreshed_access.as_str())
        );
        assert_eq!(
            refreshed.expires_at.as_deref(),
            Some("2100-01-01T00:00:00Z")
        );
        assert!(refreshed.has_valid_token);
    }

    #[test]
    fn live_vnext_provider_advances_to_committed_root_ref() {
        let root = tempfile::tempdir().unwrap();
        let identity_dir = root.path().join("identity");
        std::fs::create_dir_all(&identity_dir).unwrap();
        let vault = Arc::new(FileSecretVault::new(
            DeviceVaultRootKey::from_bytes([8_u8; 32]),
            FileSecretVaultStore::new(root.path().join("vault")),
        ));
        let device_ref = seal_test_secret(
            vault.as_ref(),
            SecretKind::IdentityDeviceSigningPrivate,
            "device-sign",
            b"device-secret",
        );
        let agreement_ref = seal_test_secret(
            vault.as_ref(),
            SecretKind::IdentityE2eeAgreementPrivate,
            "device-e2ee",
            b"agreement-secret",
        );
        let auth_ref = seal_test_secret(
            vault.as_ref(),
            SecretKind::AuthJwt,
            "auth.json",
            &crate::internal::auth::state::auth_state_json_for_token("token-secret").unwrap(),
        );
        let generated = anp::authentication::create_did_wba_document(
            "root.example",
            anp::authentication::DidDocumentOptions::default(),
        )
        .unwrap();
        let root_pem = generated.keys[anp::authentication::VM_KEY_AUTH]
            .private_key_pem
            .clone();
        let root_ref = seal_test_secret(
            vault.as_ref(),
            SecretKind::IdentityRootPrivate,
            "did:example:alice#key-1",
            root_pem.as_bytes(),
        );
        let provider = VaultBackedKeyMaterialProvider::new_vnext(
            identity_dir,
            vault,
            VNextVaultKeyMaterialRefs {
                device_request_signing_private: device_ref,
                did_document_root_private: None,
                e2ee_agreement_private: agreement_ref,
                auth_jwt: auth_ref,
            },
        );

        assert!(provider.did_document_root_private_pem().is_err());
        provider.advance_vault_root_ref(&root_ref).unwrap();
        assert_eq!(provider.did_document_root_private_pem().unwrap(), root_pem);

        let mut wrong_version = root_ref.clone();
        wrong_version.key_version = 2;
        assert!(provider.advance_vault_root_ref(&wrong_version).is_err());
        let mut wrong_binding = root_ref;
        wrong_binding.device_id = "other-device".to_owned();
        assert!(provider.advance_vault_root_ref(&wrong_binding).is_err());
        assert_eq!(provider.did_document_root_private_pem().unwrap(), root_pem);
    }

    #[test]
    fn vnext_provider_rejects_root_and_device_signing_kind_swap() {
        let root = tempfile::tempdir().unwrap();
        let identity_dir = root.path().join("identity");
        std::fs::create_dir_all(&identity_dir).unwrap();
        let vault = Arc::new(FileSecretVault::new(
            DeviceVaultRootKey::from_bytes([6_u8; 32]),
            FileSecretVaultStore::new(root.path().join("vault")),
        ));
        let root_ref = seal_test_secret(
            vault.as_ref(),
            SecretKind::IdentityRootPrivate,
            "root",
            b"root-secret",
        );
        let device_ref = seal_test_secret(
            vault.as_ref(),
            SecretKind::IdentityDeviceSigningPrivate,
            "device-sign",
            b"device-secret",
        );
        let agreement_ref = seal_test_secret(
            vault.as_ref(),
            SecretKind::IdentityE2eeAgreementPrivate,
            "device-e2ee",
            b"agreement-secret",
        );
        let auth_ref = seal_test_secret(
            vault.as_ref(),
            SecretKind::AuthJwt,
            "auth.json",
            &crate::internal::auth::state::auth_state_json_for_token("token-secret").unwrap(),
        );
        let provider = VaultBackedKeyMaterialProvider::new_vnext(
            identity_dir,
            vault,
            VNextVaultKeyMaterialRefs {
                device_request_signing_private: root_ref,
                did_document_root_private: Some(device_ref),
                e2ee_agreement_private: agreement_ref,
                auth_jwt: auth_ref,
            },
        );

        assert!(matches!(
            provider.device_request_signing_private_pem(),
            Err(crate::ImError::IdentityNotReady { missing, .. })
                if missing == vec!["device_request_signing_private_key_secret_kind"]
        ));
        assert!(matches!(
            provider.did_document_root_private_pem(),
            Err(crate::ImError::IdentityNotReady { missing, .. })
                if missing == vec!["did_document_root_private_key_secret_kind"]
        ));
    }

    fn seal_test_secret(
        vault: &FileSecretVault,
        kind: SecretKind,
        key_id: &str,
        plaintext: &[u8],
    ) -> SecretRef {
        vault
            .seal(SealSecretRequest {
                metadata: test_metadata(kind, key_id),
                plaintext: SecretBytes::from_vec(plaintext.to_vec()),
            })
            .unwrap()
    }

    fn test_jwt(expires_at: i64) -> String {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#);
        let payload = URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&json!({
                "sub": "did:example:alice",
                "iat": 1_767_225_600_i64,
                "exp": expires_at,
            }))
            .unwrap(),
        );
        format!("{header}.{payload}.test-signature")
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
