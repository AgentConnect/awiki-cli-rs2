use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::sync::Arc;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

const INDEX_SCHEMA_VERSION: i64 = 3;
const IDENTITY_FILE_NAME: &str = "identity.json";
const AUTH_FILE_NAME: &str = "auth.json";
const DID_DOCUMENT_FILE_NAME: &str = "did_document.json";
const KEY1_PRIVATE_FILE_NAME: &str = "key-1-private.pem";
const KEY1_PUBLIC_FILE_NAME: &str = "key-1-public.pem";
const E2EE_SIGNING_PRIVATE_FILE_NAME: &str = "e2ee-signing-private.pem";
const E2EE_AGREEMENT_PRIVATE_FILE_NAME: &str = "e2ee-agreement-private.pem";
const DAEMON_SUBKEY_PRIVATE_FILE_NAME: &str = "daemon-key-1-private.pem";
const DAEMON_SUBKEY_PACKAGE_FILE_NAME: &str = "daemon-subkey-package.json";
const IDENTITY_VAULT_MIGRATION_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub(crate) struct IdentityStore<'a> {
    paths: &'a crate::paths::IdentityRegistryPaths,
}

#[derive(Debug, Clone)]
pub(crate) struct SaveIdentityInput {
    pub(crate) local_alias: String,
    pub(crate) did: crate::ids::Did,
    pub(crate) unique_id: String,
    pub(crate) user_id: String,
    pub(crate) display_name: String,
    pub(crate) handle: String,
    pub(crate) full_handle: String,
    pub(crate) jwt_token: String,
    pub(crate) did_document: Option<Value>,
    pub(crate) key1_private_pem: String,
    pub(crate) key1_public_pem: String,
    pub(crate) e2ee_signing_private_pem: String,
    pub(crate) e2ee_agreement_private_pem: String,
    pub(crate) daemon_subkey_package: Option<crate::identity::DaemonSubkeyPrivatePackage>,
    pub(crate) make_default: bool,
}

#[derive(Clone)]
pub(crate) enum SaveIdentitySecretStorage {
    FileCompat,
    Vault {
        workspace_id: String,
        device_id: String,
        vault: Arc<dyn crate::internal::secret_vault::SecretVault + Send + Sync>,
    },
}

impl SaveIdentitySecretStorage {
    pub(crate) fn from_core(core: &crate::core::ImCore) -> crate::ImResult<Self> {
        match core.inner().identity_secret_storage_policy() {
            crate::core::IdentitySecretStoragePolicy::FileCompat => Ok(Self::FileCompat),
            crate::core::IdentitySecretStoragePolicy::VaultPreferred => {
                match core.inner().identity_vault() {
                    Some(context) => Ok(Self::Vault {
                        workspace_id: context.workspace_id().to_owned(),
                        device_id: context.device_id().to_owned(),
                        vault: context.vault(),
                    }),
                    None => Ok(Self::FileCompat),
                }
            }
            crate::core::IdentitySecretStoragePolicy::VaultRequired => {
                let context = core.inner().identity_vault().ok_or_else(|| {
                    crate::ImError::LocalStateUnavailable {
                        detail: "identity secret storage policy is VaultRequired but no identity secret vault was provided"
                            .to_owned(),
                    }
                })?;
                Ok(Self::Vault {
                    workspace_id: context.workspace_id().to_owned(),
                    device_id: context.device_id().to_owned(),
                    vault: context.vault(),
                })
            }
        }
    }

    pub(crate) fn writes_secrets_to_vault(&self) -> bool {
        matches!(self, Self::Vault { .. })
    }
}

impl std::fmt::Debug for SaveIdentitySecretStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FileCompat => f.write_str("SaveIdentitySecretStorage::FileCompat"),
            Self::Vault {
                workspace_id,
                device_id,
                ..
            } => f
                .debug_struct("SaveIdentitySecretStorage::Vault")
                .field("workspace_id", workspace_id)
                .field("device_id", device_id)
                .field("vault", &"<redacted-secret-vault>")
                .finish(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredIdentity {
    pub(crate) local_alias: String,
    pub(crate) dir_name: String,
    pub(crate) did: crate::ids::Did,
    pub(crate) unique_id: String,
    pub(crate) user_id: String,
    pub(crate) display_name: String,
    pub(crate) handle: String,
    pub(crate) full_handle: String,
    pub(crate) created_at: String,
    pub(crate) jwt_token: String,
    pub(crate) is_default: bool,
    pub(crate) has_did_document: bool,
    pub(crate) has_key1_private: bool,
    pub(crate) has_key1_public: bool,
    pub(crate) has_e2ee_signing_private: bool,
    pub(crate) has_e2ee_agreement_private: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RecoverPromotionResult {
    pub(crate) default_updated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IdentityVaultMigrationResult {
    pub(crate) local_alias: String,
    pub(crate) dir_name: String,
    pub(crate) metadata: IdentityVaultMigrationMetadata,
}

impl<'a> IdentityStore<'a> {
    pub(crate) fn new(paths: &'a crate::paths::IdentityRegistryPaths) -> Self {
        Self { paths }
    }

    pub(crate) fn save_identity(
        &self,
        input: SaveIdentityInput,
    ) -> crate::ImResult<StoredIdentity> {
        self.save_identity_with_secret_storage(input, SaveIdentitySecretStorage::FileCompat)
    }

    pub(crate) fn save_identity_with_secret_storage(
        &self,
        mut input: SaveIdentityInput,
        secret_storage: SaveIdentitySecretStorage,
    ) -> crate::ImResult<StoredIdentity> {
        let local_alias = sanitize_identity_name(&input.local_alias);
        if local_alias.is_empty() {
            return Err(crate::ImError::invalid_input(
                Some("local_alias".to_string()),
                "local alias is required",
            ));
        }
        if input.unique_id.trim().is_empty() {
            return Err(crate::ImError::invalid_input(
                Some("unique_id".to_string()),
                "unique_id is required",
            ));
        }
        let (handle, full_handle) =
            stored_handle_fields(&input.handle, &input.full_handle, input.did.as_str());
        input.handle = handle;
        input.full_handle = full_handle;

        fs::create_dir_all(&self.paths.identity_root_dir)?;
        set_private_dir_mode(&self.paths.identity_root_dir)?;
        let mut index = self.load_index()?;
        let dir_name = preferred_dir_name(&input.unique_id)?;
        for (name, entry) in &index.credentials {
            if name == &local_alias {
                continue;
            }
            if entry.dir_name == dir_name && entry.did != input.did.as_str() {
                return Err(crate::ImError::invalid_input(
                    Some("identity".to_string()),
                    format!("identity dir {dir_name} already used by {name}"),
                ));
            }
        }
        let identity_dir = self.paths.identity_root_dir.join(&dir_name);
        let created_at = index
            .credentials
            .get(&local_alias)
            .map(|entry| entry.created_at.clone())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(now_rfc3339);
        if let Some(package) = &input.daemon_subkey_package {
            if package.user_did != input.did {
                return Err(crate::ImError::invalid_input(
                    Some("daemon_subkey_package.user_did".to_string()),
                    "daemon subkey package user_did must match identity did",
                ));
            }
        }
        let vault_metadata = match &secret_storage {
            SaveIdentitySecretStorage::FileCompat => None,
            SaveIdentitySecretStorage::Vault {
                workspace_id,
                device_id,
                vault,
            } => Some(seal_identity_input_to_vault(
                &input,
                workspace_id,
                device_id,
                vault.as_ref(),
            )?),
        };

        fs::create_dir_all(&identity_dir)?;
        set_private_dir_mode(&identity_dir)?;
        if vault_metadata.is_some() {
            remove_known_plaintext_secret_files(&identity_dir)?;
        }

        write_secure_json(
            &identity_dir.join(IDENTITY_FILE_NAME),
            &IdentityPayload {
                did: input.did.as_str().to_string(),
                unique_id: input.unique_id.clone(),
                created_at: created_at.clone(),
                user_id: input.user_id.clone(),
                display_name: input.display_name.clone(),
                handle: input.handle.clone(),
                full_handle: input.full_handle.clone(),
            },
        )?;
        if vault_metadata.is_none() {
            write_secure_json(
                &identity_dir.join(AUTH_FILE_NAME),
                &json!({ "jwt_token": nullable_string(&input.jwt_token) }),
            )?;
        }
        if let Some(document) = &input.did_document {
            write_secure_json(&identity_dir.join(DID_DOCUMENT_FILE_NAME), document)?;
        }
        write_secure_text_if_present(
            &identity_dir.join(KEY1_PUBLIC_FILE_NAME),
            &input.key1_public_pem,
        )?;
        match vault_metadata.as_ref() {
            Some(_) => {
                if let Some(package) = &input.daemon_subkey_package {
                    write_sanitized_daemon_subkey_package(
                        &identity_dir.join(DAEMON_SUBKEY_PACKAGE_FILE_NAME),
                        package,
                    )?;
                } else {
                    remove_file_if_exists(&identity_dir.join(DAEMON_SUBKEY_PACKAGE_FILE_NAME))?;
                }
            }
            None => {
                write_secure_text_if_present(
                    &identity_dir.join(KEY1_PRIVATE_FILE_NAME),
                    &input.key1_private_pem,
                )?;
                write_secure_text_if_present(
                    &identity_dir.join(E2EE_SIGNING_PRIVATE_FILE_NAME),
                    &input.e2ee_signing_private_pem,
                )?;
                write_secure_text_if_present(
                    &identity_dir.join(E2EE_AGREEMENT_PRIVATE_FILE_NAME),
                    &input.e2ee_agreement_private_pem,
                )?;
                if let Some(package) = &input.daemon_subkey_package {
                    write_secure_text_if_present(
                        &identity_dir.join(DAEMON_SUBKEY_PRIVATE_FILE_NAME),
                        package.private_key_material(),
                    )?;
                    write_secure_json(
                        &identity_dir.join(DAEMON_SUBKEY_PACKAGE_FILE_NAME),
                        package,
                    )?;
                }
            }
        }

        if vault_metadata.is_some() {
            for path in [
                identity_dir.join(AUTH_FILE_NAME),
                identity_dir.join(KEY1_PRIVATE_FILE_NAME),
                identity_dir.join("private.key"),
                identity_dir.join(E2EE_SIGNING_PRIVATE_FILE_NAME),
                identity_dir.join(E2EE_AGREEMENT_PRIVATE_FILE_NAME),
                identity_dir.join("key-3-private.pem"),
                identity_dir.join(DAEMON_SUBKEY_PRIVATE_FILE_NAME),
            ] {
                remove_file_if_exists(&path)?;
            }
        }

        if input.make_default || index.default_credential_name.is_empty() {
            index.default_credential_name = local_alias.clone();
        }
        let is_default = index.default_credential_name == local_alias;
        index.credentials.insert(
            local_alias.clone(),
            IndexEntry {
                credential_name: local_alias.clone(),
                dir_name: dir_name.clone(),
                did: input.did.as_str().to_string(),
                unique_id: input.unique_id.clone(),
                user_id: input.user_id.clone(),
                name: input.display_name.clone(),
                handle: input.handle.clone(),
                full_handle: input.full_handle.clone(),
                created_at: created_at.clone(),
                is_default,
                vault_migration: vault_metadata,
            },
        );
        self.save_index(index)?;
        if is_default {
            self.write_default_identity(&local_alias)?;
        }
        Ok(StoredIdentity {
            local_alias,
            dir_name,
            did: input.did,
            unique_id: input.unique_id,
            user_id: input.user_id,
            display_name: input.display_name,
            handle: input.handle,
            full_handle: input.full_handle,
            created_at,
            jwt_token: input.jwt_token,
            is_default,
            has_did_document: input.did_document.is_some(),
            has_key1_private: !input.key1_private_pem.trim().is_empty(),
            has_key1_public: !input.key1_public_pem.trim().is_empty(),
            has_e2ee_signing_private: !input.e2ee_signing_private_pem.trim().is_empty(),
            has_e2ee_agreement_private: !input.e2ee_agreement_private_pem.trim().is_empty(),
        })
    }

    pub(crate) async fn save_identity_async(
        paths: crate::paths::IdentityRegistryPaths,
        input: SaveIdentityInput,
    ) -> crate::ImResult<StoredIdentity> {
        crate::internal::runtime::worker::run_blocking(move || {
            IdentityStore::new(&paths).save_identity(input)
        })
        .await
        .map_err(|err| crate::ImError::Internal {
            message: err.to_string(),
        })?
    }

    pub(crate) async fn save_identity_with_secret_storage_async(
        paths: crate::paths::IdentityRegistryPaths,
        input: SaveIdentityInput,
        secret_storage: SaveIdentitySecretStorage,
    ) -> crate::ImResult<StoredIdentity> {
        crate::internal::runtime::worker::run_blocking(move || {
            IdentityStore::new(&paths).save_identity_with_secret_storage(input, secret_storage)
        })
        .await
        .map_err(|err| crate::ImError::Internal {
            message: err.to_string(),
        })?
    }

    pub(crate) fn load_daemon_subkey_package(
        &self,
        identity_dir_name: &str,
    ) -> crate::ImResult<crate::identity::DaemonSubkeyPrivatePackage> {
        let identity_dir = local_identity_dir(&self.paths.identity_root_dir, identity_dir_name)?;
        let package_path = identity_dir.join(DAEMON_SUBKEY_PACKAGE_FILE_NAME);
        match fs::read(&package_path) {
            Ok(raw) => serde_json::from_slice(&raw).map_err(|err| crate::ImError::Serialization {
                detail: err.to_string(),
            }),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                Err(crate::ImError::IdentityNotFound {
                    selector: format!("daemon subkey package for {identity_dir_name}"),
                })
            }
            Err(err) => Err(crate::ImError::CredentialFileUnreadable {
                path_kind: "daemon_subkey_package".to_string(),
                detail: err.to_string(),
            }),
        }
    }

    pub(crate) fn load_daemon_subkey_package_vault_aware(
        &self,
        identity_dir_name: &str,
        did: &crate::ids::Did,
        metadata: Option<&IdentityVaultMigrationMetadata>,
        context: Option<&crate::core::options::IdentityVaultContext>,
        policy: crate::core::IdentitySecretStoragePolicy,
    ) -> crate::ImResult<crate::identity::DaemonSubkeyPrivatePackage> {
        if let Some(metadata) = metadata.filter(|metadata| {
            matches!(metadata.status, IdentityVaultMigrationStatus::Verified)
                && metadata.refs.daemon_subkey_private.is_some()
        }) {
            if let Some(context) = context {
                if metadata.workspace_id == context.workspace_id()
                    && metadata.device_id == context.device_id()
                {
                    let did_document = self.load_did_document(identity_dir_name)?;
                    return self.load_daemon_subkey_package_from_vault(
                        identity_dir_name,
                        did,
                        &did_document,
                        metadata,
                        context.workspace_id(),
                        context.device_id(),
                        context.vault().as_ref(),
                    );
                }
                if matches!(
                    policy,
                    crate::core::IdentitySecretStoragePolicy::VaultRequired
                ) || !metadata.plaintext_compat_retained
                {
                    return Err(crate::ImError::IdentityNotReady {
                        identity: did.as_str().to_string(),
                        missing: vec!["identity_vault_context_mismatch".to_string()],
                    });
                }
            } else if matches!(
                policy,
                crate::core::IdentitySecretStoragePolicy::VaultRequired
            ) || !metadata.plaintext_compat_retained
            {
                return Err(crate::ImError::LocalStateUnavailable {
                    detail:
                        "identity daemon subkey is stored in vault but no identity secret vault was provided"
                            .to_owned(),
                });
            }
        } else if matches!(
            policy,
            crate::core::IdentitySecretStoragePolicy::VaultRequired
        ) {
            return Err(crate::ImError::IdentityNotFound {
                selector: format!("daemon subkey package for {identity_dir_name}"),
            });
        }
        self.load_daemon_subkey_package(identity_dir_name)
    }

    pub(crate) fn load_daemon_subkey_package_from_vault(
        &self,
        identity_dir_name: &str,
        did: &crate::ids::Did,
        did_document: &Value,
        metadata: &IdentityVaultMigrationMetadata,
        workspace_id: &str,
        device_id: &str,
        vault: &dyn crate::internal::secret_vault::SecretVault,
    ) -> crate::ImResult<crate::identity::DaemonSubkeyPrivatePackage> {
        let _ = local_identity_dir(&self.paths.identity_root_dir, identity_dir_name)?;
        ensure_verified_vault_metadata_context(metadata, workspace_id, device_id)?;
        let secret_ref = metadata
            .refs
            .daemon_subkey_private
            .as_ref()
            .ok_or_else(|| crate::ImError::IdentityNotReady {
                identity: did.as_str().to_string(),
                missing: vec!["daemon_subkey_private_ref".to_string()],
            })?;
        let private_key_pem = open_vault_utf8_secret(vault, secret_ref, "daemon_subkey_private")?;
        crate::internal::identity_daemon_subkey::package_from_private_pem_and_document(
            did.clone(),
            private_key_pem,
            did_document,
        )
    }

    pub(crate) fn load_key1_private_pem_from_vault(
        &self,
        identity_dir_name: &str,
        did: &crate::ids::Did,
        metadata: &IdentityVaultMigrationMetadata,
        workspace_id: &str,
        device_id: &str,
        vault: &dyn crate::internal::secret_vault::SecretVault,
    ) -> crate::ImResult<String> {
        let _ = local_identity_dir(&self.paths.identity_root_dir, identity_dir_name)?;
        ensure_verified_vault_metadata_context(metadata, workspace_id, device_id)?;
        if metadata.refs.default_signing_private.did.as_deref() != Some(did.as_str()) {
            return Err(crate::ImError::IdentityNotReady {
                identity: did.as_str().to_string(),
                missing: vec!["identity_vault_did_match".to_string()],
            });
        }
        open_vault_utf8_secret(
            vault,
            &metadata.refs.default_signing_private,
            "default_signing_private_key",
        )
    }

    pub(crate) fn save_daemon_subkey_package_with_secret_storage(
        &self,
        identity_dir_name: &str,
        identity_id: &str,
        package: &crate::identity::DaemonSubkeyPrivatePackage,
        secret_storage: SaveIdentitySecretStorage,
    ) -> crate::ImResult<()> {
        match secret_storage {
            SaveIdentitySecretStorage::FileCompat => {
                self.save_daemon_subkey_package(identity_dir_name, package)
            }
            SaveIdentitySecretStorage::Vault {
                workspace_id,
                device_id,
                vault,
            } => self.save_daemon_subkey_package_to_vault(
                identity_dir_name,
                identity_id,
                package,
                &workspace_id,
                &device_id,
                vault.as_ref(),
            ),
        }
    }

    fn save_daemon_subkey_package_to_vault(
        &self,
        identity_dir_name: &str,
        identity_id: &str,
        package: &crate::identity::DaemonSubkeyPrivatePackage,
        workspace_id: &str,
        device_id: &str,
        vault: &dyn crate::internal::secret_vault::SecretVault,
    ) -> crate::ImResult<()> {
        let identity_dir = local_identity_dir(&self.paths.identity_root_dir, identity_dir_name)?;
        let mut index = self.load_index()?;
        let entry = index
            .credentials
            .values_mut()
            .find(|entry| {
                entry.dir_name == identity_dir_name
                    || entry.unique_id == identity_id
                    || entry.did == package.user_did.as_str()
            })
            .ok_or_else(|| crate::ImError::IdentityNotFound {
                selector: identity_dir_name.to_string(),
            })?;
        let metadata =
            entry
                .vault_migration
                .as_mut()
                .ok_or_else(|| crate::ImError::IdentityNotReady {
                    identity: package.user_did.as_str().to_string(),
                    missing: vec!["identity_vault_metadata".to_string()],
                })?;
        ensure_verified_vault_metadata_context(metadata, workspace_id, device_id)?;
        let secret_ref = seal_utf8_secret(
            vault,
            vault_secret_metadata(
                workspace_id,
                device_id,
                identity_id,
                package.user_did.as_str(),
                crate::internal::secret_vault::record::SecretKind::IdentityDaemonPrivate,
                "daemon-key-1",
            ),
            package.private_key_material(),
        )?;
        verify_vault_utf8_secret(vault, &secret_ref, package.private_key_material())?;
        metadata.refs.daemon_subkey_private = Some(secret_ref);
        self.save_index(index)?;
        remove_file_if_exists(&identity_dir.join(DAEMON_SUBKEY_PRIVATE_FILE_NAME))?;
        write_sanitized_daemon_subkey_package(
            &identity_dir.join(DAEMON_SUBKEY_PACKAGE_FILE_NAME),
            package,
        )
    }

    pub(crate) fn load_daemon_subkey_package_or_legacy(
        &self,
        identity_dir_name: &str,
        did: &crate::ids::Did,
        did_document: &Value,
    ) -> crate::ImResult<Option<crate::identity::DaemonSubkeyPrivatePackage>> {
        match self.load_daemon_subkey_package(identity_dir_name) {
            Ok(package) => return Ok(Some(package)),
            Err(crate::ImError::IdentityNotFound { .. }) => {}
            Err(err) => return Err(err),
        }
        let identity_dir = local_identity_dir(&self.paths.identity_root_dir, identity_dir_name)?;
        let legacy_private_path = identity_dir.join(DAEMON_SUBKEY_PRIVATE_FILE_NAME);
        let private_key_pem = match fs::read_to_string(&legacy_private_path) {
            Ok(value) => value,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => {
                return Err(crate::ImError::CredentialFileUnreadable {
                    path_kind: "daemon_subkey_private".to_string(),
                    detail: err.to_string(),
                });
            }
        };
        let package =
            crate::internal::identity_daemon_subkey::package_from_private_pem_and_document(
                did.clone(),
                private_key_pem,
                did_document,
            )?;
        self.save_daemon_subkey_package(identity_dir_name, &package)?;
        Ok(Some(package))
    }

    pub(crate) async fn load_daemon_subkey_package_async(
        paths: crate::paths::IdentityRegistryPaths,
        identity_dir_name: String,
    ) -> crate::ImResult<crate::identity::DaemonSubkeyPrivatePackage> {
        crate::internal::runtime::worker::run_blocking(move || {
            IdentityStore::new(&paths).load_daemon_subkey_package(&identity_dir_name)
        })
        .await
        .map_err(|err| crate::ImError::Internal {
            message: err.to_string(),
        })?
    }

    pub(crate) fn load_did_document(&self, identity_dir_name: &str) -> crate::ImResult<Value> {
        let identity_dir = local_identity_dir(&self.paths.identity_root_dir, identity_dir_name)?;
        let path = first_existing_path(&identity_dir, &[DID_DOCUMENT_FILE_NAME, "did.json"]);
        match fs::read(&path) {
            Ok(raw) => serde_json::from_slice(&raw).map_err(|err| crate::ImError::Serialization {
                detail: err.to_string(),
            }),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                Err(crate::ImError::CredentialFileUnreadable {
                    path_kind: "did_document".to_string(),
                    detail: "file is missing".to_string(),
                })
            }
            Err(err) => Err(crate::ImError::CredentialFileUnreadable {
                path_kind: "did_document".to_string(),
                detail: err.to_string(),
            }),
        }
    }

    pub(crate) fn save_did_document(
        &self,
        identity_dir_name: &str,
        did_document: &Value,
    ) -> crate::ImResult<()> {
        let identity_dir = local_identity_dir(&self.paths.identity_root_dir, identity_dir_name)?;
        write_secure_json(&identity_dir.join(DID_DOCUMENT_FILE_NAME), did_document)
    }

    pub(crate) fn load_key1_private_pem(&self, identity_dir_name: &str) -> crate::ImResult<String> {
        let identity_dir = local_identity_dir(&self.paths.identity_root_dir, identity_dir_name)?;
        let path = first_existing_path(&identity_dir, &[KEY1_PRIVATE_FILE_NAME, "private.key"]);
        match fs::read_to_string(&path) {
            Ok(value) if !value.trim().is_empty() => Ok(value),
            Ok(_) => Err(crate::ImError::CredentialFileUnreadable {
                path_kind: "private_key".to_string(),
                detail: "file is empty".to_string(),
            }),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                Err(crate::ImError::CredentialFileUnreadable {
                    path_kind: "private_key".to_string(),
                    detail: "file is missing".to_string(),
                })
            }
            Err(err) => Err(crate::ImError::CredentialFileUnreadable {
                path_kind: "private_key".to_string(),
                detail: err.to_string(),
            }),
        }
    }

    pub(crate) fn migrate_identity_to_vault(
        &self,
        local_alias: &str,
        workspace_id: &str,
        device_id: &str,
        vault: &dyn crate::internal::secret_vault::SecretVault,
    ) -> crate::ImResult<IdentityVaultMigrationResult> {
        let local_alias = local_alias.trim();
        if local_alias.is_empty() {
            return Err(crate::ImError::invalid_input(
                Some("local_alias".to_string()),
                "local alias is required",
            ));
        }
        let workspace_id = workspace_id.trim();
        let device_id = device_id.trim();
        if workspace_id.is_empty() {
            return Err(crate::ImError::invalid_input(
                Some("workspace_id".to_string()),
                "workspace id is required",
            ));
        }
        if device_id.is_empty() {
            return Err(crate::ImError::invalid_input(
                Some("device_id".to_string()),
                "device id is required",
            ));
        }

        let mut index = self.load_index()?;
        let entry = index.credentials.get(local_alias).cloned().ok_or_else(|| {
            crate::ImError::IdentityNotFound {
                selector: local_alias.to_string(),
            }
        })?;
        let identity_dir = local_identity_dir(&self.paths.identity_root_dir, &entry.dir_name)?;
        let key1_private_pem = read_required_non_empty_text(
            &identity_dir,
            &[KEY1_PRIVATE_FILE_NAME, "private.key"],
            "private_key",
        )?;
        let e2ee_signing_private_pem = read_optional_non_empty_text(
            &identity_dir.join(E2EE_SIGNING_PRIVATE_FILE_NAME),
            "e2ee_signing_private_key",
        )?;
        let e2ee_agreement_private_pem = read_required_non_empty_text(
            &identity_dir,
            &[E2EE_AGREEMENT_PRIVATE_FILE_NAME, "key-3-private.pem"],
            "e2ee_agreement_private_key",
        )?;
        let auth_state_raw =
            read_optional_file_or_default(&identity_dir.join(AUTH_FILE_NAME), b"{}")?;
        crate::internal::auth::state::parse_auth_state(&auth_state_raw)?;
        let daemon_subkey_private_pem = read_daemon_subkey_private_material(&identity_dir)?;

        let did = entry.did.trim();
        let identity_id = entry.unique_id.trim();
        let default_signing_private = seal_utf8_secret(
            vault,
            vault_secret_metadata(
                workspace_id,
                device_id,
                identity_id,
                did,
                crate::internal::secret_vault::record::SecretKind::IdentityRootPrivate,
                "key-1",
            ),
            &key1_private_pem,
        )?;
        let e2ee_signing_private = match e2ee_signing_private_pem.as_deref() {
            Some(value) => Some(seal_utf8_secret(
                vault,
                vault_secret_metadata(
                    workspace_id,
                    device_id,
                    identity_id,
                    did,
                    crate::internal::secret_vault::record::SecretKind::IdentityE2eeSigningPrivate,
                    "key-2",
                ),
                value,
            )?),
            None => None,
        };
        let e2ee_agreement_private = seal_utf8_secret(
            vault,
            vault_secret_metadata(
                workspace_id,
                device_id,
                identity_id,
                did,
                crate::internal::secret_vault::record::SecretKind::IdentityE2eeAgreementPrivate,
                "key-3",
            ),
            &e2ee_agreement_private_pem,
        )?;
        let daemon_subkey_private = match daemon_subkey_private_pem.as_deref() {
            Some(value) => Some(seal_utf8_secret(
                vault,
                vault_secret_metadata(
                    workspace_id,
                    device_id,
                    identity_id,
                    did,
                    crate::internal::secret_vault::record::SecretKind::IdentityDaemonPrivate,
                    "daemon-key-1",
                ),
                value,
            )?),
            None => None,
        };
        let auth_jwt = vault.seal(crate::internal::secret_vault::SealSecretRequest {
            metadata: vault_secret_metadata(
                workspace_id,
                device_id,
                identity_id,
                did,
                crate::internal::secret_vault::record::SecretKind::AuthJwt,
                AUTH_FILE_NAME,
            ),
            plaintext: crate::internal::platform_secret::SecretBytes::from_vec(
                auth_state_raw.clone(),
            ),
        })?;

        verify_vault_utf8_secret(vault, &default_signing_private, &key1_private_pem)?;
        if let (Some(secret_ref), Some(expected)) =
            (&e2ee_signing_private, e2ee_signing_private_pem.as_deref())
        {
            verify_vault_utf8_secret(vault, secret_ref, expected)?;
        }
        verify_vault_utf8_secret(vault, &e2ee_agreement_private, &e2ee_agreement_private_pem)?;
        if let (Some(secret_ref), Some(expected)) =
            (&daemon_subkey_private, daemon_subkey_private_pem.as_deref())
        {
            verify_vault_utf8_secret(vault, secret_ref, expected)?;
        }
        let opened_auth = vault.open(&auth_jwt)?;
        if opened_auth.expose_secret() != auth_state_raw.as_slice() {
            return Err(crate::ImError::PermissionDenied);
        }
        crate::internal::auth::state::parse_auth_state(opened_auth.expose_secret())?;

        let metadata = IdentityVaultMigrationMetadata {
            schema_version: IDENTITY_VAULT_MIGRATION_SCHEMA_VERSION,
            status: IdentityVaultMigrationStatus::Verified,
            backend: "vault".to_owned(),
            unlock_policy: "explicit_root_key".to_owned(),
            migrated_at: now_rfc3339(),
            workspace_id: workspace_id.to_owned(),
            device_id: device_id.to_owned(),
            plaintext_compat_retained: true,
            refs: IdentityVaultSecretRefs {
                default_signing_private,
                e2ee_signing_private,
                e2ee_agreement_private,
                daemon_subkey_private,
                auth_jwt,
            },
        };
        let index_entry = index.credentials.get_mut(local_alias).ok_or_else(|| {
            crate::ImError::IdentityNotFound {
                selector: local_alias.to_string(),
            }
        })?;
        index_entry.vault_migration = Some(metadata.clone());
        self.save_index(index)?;

        Ok(IdentityVaultMigrationResult {
            local_alias: local_alias.to_string(),
            dir_name: entry.dir_name,
            metadata,
        })
    }

    pub(crate) fn save_daemon_subkey_package(
        &self,
        identity_dir_name: &str,
        package: &crate::identity::DaemonSubkeyPrivatePackage,
    ) -> crate::ImResult<()> {
        let identity_dir = local_identity_dir(&self.paths.identity_root_dir, identity_dir_name)?;
        write_secure_text_if_present(
            &identity_dir.join(DAEMON_SUBKEY_PRIVATE_FILE_NAME),
            package.private_key_material(),
        )?;
        write_secure_json(&identity_dir.join(DAEMON_SUBKEY_PACKAGE_FILE_NAME), package)
    }

    pub(crate) fn promote_recovered_handle(
        &self,
        final_identity_name: &str,
        temp_identity_name: &str,
        archived_identity_names: &[String],
    ) -> crate::ImResult<RecoverPromotionResult> {
        let final_identity_name = final_identity_name.trim();
        let temp_identity_name = temp_identity_name.trim();
        if final_identity_name.is_empty() {
            return Err(crate::ImError::invalid_input(
                Some("final_identity_name".to_string()),
                "final identity name is required",
            ));
        }
        if temp_identity_name.is_empty() {
            return Err(crate::ImError::invalid_input(
                Some("temp_identity_name".to_string()),
                "temporary identity name is required",
            ));
        }
        let mut index = self.load_index()?;
        let mut temp_entry = index
            .credentials
            .get(temp_identity_name)
            .cloned()
            .ok_or_else(|| crate::ImError::IdentityNotFound {
                selector: temp_identity_name.to_string(),
            })?;
        let archived_set = archived_identity_names
            .iter()
            .filter_map(|name| {
                let name = name.trim();
                (!name.is_empty()).then(|| name.to_string())
            })
            .collect::<BTreeSet<_>>();

        for name in index.credentials.keys() {
            if name == temp_identity_name || archived_set.contains(name) {
                continue;
            }
            if name == final_identity_name {
                return Err(crate::ImError::invalid_input(
                    Some("final_identity_name".to_string()),
                    format!(
                        "identity conflict: identity name {final_identity_name} is already used by another live identity"
                    ),
                ));
            }
        }

        for name in &archived_set {
            index.credentials.remove(name);
        }
        index.credentials.remove(temp_identity_name);
        temp_entry.credential_name = final_identity_name.to_string();
        temp_entry.is_default = false;
        index
            .credentials
            .insert(final_identity_name.to_string(), temp_entry);

        let mut default_updated = false;
        let current_default = index.default_credential_name.trim().to_string();
        if !current_default.is_empty()
            && (current_default == temp_identity_name || archived_set.contains(&current_default))
        {
            index.default_credential_name = final_identity_name.to_string();
            default_updated = true;
        }
        self.save_index(index)?;
        if default_updated {
            self.write_default_identity(final_identity_name)?;
        }
        Ok(RecoverPromotionResult { default_updated })
    }

    pub(crate) fn load_index(&self) -> crate::ImResult<IndexPayload> {
        match fs::read(&self.paths.registry_path) {
            Ok(raw) => {
                let payload = parse_index_payload(&raw)?;
                normalize_index_payload(payload)
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(IndexPayload::default()),
            Err(err) => Err(crate::ImError::CredentialFileUnreadable {
                path_kind: "identity_registry".to_string(),
                detail: err.to_string(),
            }),
        }
    }

    pub(crate) fn save_index(&self, index: IndexPayload) -> crate::ImResult<()> {
        if let Some(parent) = self.paths.registry_path.parent() {
            fs::create_dir_all(parent)?;
            set_private_dir_mode(parent)?;
        }
        let index = normalize_index_payload(index)?;
        let raw =
            serde_json::to_vec_pretty(&index).map_err(|err| crate::ImError::Serialization {
                detail: err.to_string(),
            })?;
        fs::write(&self.paths.registry_path, raw)?;
        set_private_file_mode(&self.paths.registry_path)?;
        Ok(())
    }

    pub(crate) fn write_default_identity(&self, local_alias: &str) -> crate::ImResult<()> {
        let Some(path) = self.paths.default_identity_path.as_deref() else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
            set_private_dir_mode(parent)?;
        }
        fs::write(path, format!("{local_alias}\n"))?;
        set_private_file_mode(path)?;
        Ok(())
    }

    pub(crate) fn update_display_name_projection(
        &self,
        identity: &crate::identity::IdentitySummary,
        display_name: &str,
    ) -> crate::ImResult<()> {
        let display_name = display_name.trim();
        if display_name.is_empty() {
            return Ok(());
        }
        let Some((alias, dir_name)) = self.local_alias_and_dir_name(identity)? else {
            return Ok(());
        };
        let identity_path = self
            .paths
            .identity_root_dir
            .join(dir_name)
            .join(IDENTITY_FILE_NAME);
        match fs::read(&identity_path) {
            Ok(raw) => {
                let mut payload: Value =
                    serde_json::from_slice(&raw).map_err(|err| crate::ImError::Serialization {
                        detail: err.to_string(),
                    })?;
                let object =
                    payload
                        .as_object_mut()
                        .ok_or_else(|| crate::ImError::Serialization {
                            detail: "identity payload must be a JSON object".to_string(),
                        })?;
                object.insert("name".to_string(), Value::String(display_name.to_string()));
                write_secure_json(&identity_path, &payload)?;
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err.into()),
        }
        self.update_registry_display_name(identity, &alias, display_name)?;
        Ok(())
    }

    fn local_alias_and_dir_name(
        &self,
        identity: &crate::identity::IdentitySummary,
    ) -> crate::ImResult<Option<(String, String)>> {
        let index = self.load_index()?;
        let alias = identity.local_alias.as_deref().unwrap_or_default();
        if !alias.is_empty() {
            if let Some(entry) = index.credentials.get(alias) {
                return Ok(Some((alias.to_string(), entry.dir_name.clone())));
            }
        }
        for (candidate_alias, entry) in &index.credentials {
            if entry.unique_id == identity.id.as_str() || entry.did == identity.did.as_str() {
                return Ok(Some((candidate_alias.clone(), entry.dir_name.clone())));
            }
        }
        Ok(None)
    }

    fn update_registry_display_name(
        &self,
        identity: &crate::identity::IdentitySummary,
        alias: &str,
        display_name: &str,
    ) -> crate::ImResult<()> {
        let raw = match fs::read(&self.paths.registry_path) {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(err.into()),
        };
        let mut registry: Value = match serde_json::from_slice(&raw) {
            Ok(value) => value,
            Err(_) => return Ok(()),
        };
        let mut changed = false;
        if let Some(entry) = registry
            .as_object_mut()
            .and_then(|object| object.get_mut("credentials"))
            .and_then(Value::as_object_mut)
            .and_then(|credentials| credentials.get_mut(alias))
            .and_then(Value::as_object_mut)
        {
            entry.insert("name".to_string(), Value::String(display_name.to_string()));
            changed = true;
        } else if let Some(identities) = registry
            .as_object_mut()
            .and_then(|object| object.get_mut("identities"))
            .and_then(Value::as_array_mut)
        {
            let local_alias = identity.local_alias.as_deref().unwrap_or_default();
            for item in identities {
                let Some(object) = item.as_object_mut() else {
                    continue;
                };
                let id_matches =
                    object.get("id").and_then(Value::as_str) == Some(identity.id.as_str());
                let did_matches =
                    object.get("did").and_then(Value::as_str) == Some(identity.did.as_str());
                let alias_matches = !local_alias.is_empty()
                    && object.get("local_alias").and_then(Value::as_str) == Some(local_alias);
                if id_matches || did_matches || alias_matches {
                    object.insert(
                        "display_name".to_string(),
                        Value::String(display_name.to_string()),
                    );
                    changed = true;
                    break;
                }
            }
        }
        if changed {
            write_secure_json(&self.paths.registry_path, &registry)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct IndexEntry {
    #[serde(default)]
    pub(crate) credential_name: String,
    #[serde(default)]
    pub(crate) dir_name: String,
    #[serde(default)]
    pub(crate) did: String,
    #[serde(default)]
    pub(crate) unique_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) user_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) handle: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) full_handle: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) created_at: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub(crate) is_default: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) vault_migration: Option<IdentityVaultMigrationMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct IdentityVaultMigrationMetadata {
    pub(crate) schema_version: u32,
    pub(crate) status: IdentityVaultMigrationStatus,
    pub(crate) backend: String,
    pub(crate) unlock_policy: String,
    pub(crate) migrated_at: String,
    pub(crate) workspace_id: String,
    pub(crate) device_id: String,
    pub(crate) plaintext_compat_retained: bool,
    pub(crate) refs: IdentityVaultSecretRefs,
}

impl IdentityVaultMigrationMetadata {
    pub(crate) fn key_material_refs(&self) -> crate::internal::key_provider::VaultKeyMaterialRefs {
        crate::internal::key_provider::VaultKeyMaterialRefs {
            default_signing_private: self.refs.default_signing_private.clone(),
            e2ee_agreement_private: self.refs.e2ee_agreement_private.clone(),
            auth_jwt: self.refs.auth_jwt.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum IdentityVaultMigrationStatus {
    Verified,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct IdentityVaultSecretRefs {
    pub(crate) default_signing_private: crate::internal::secret_vault::record::SecretRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) e2ee_signing_private: Option<crate::internal::secret_vault::record::SecretRef>,
    pub(crate) e2ee_agreement_private: crate::internal::secret_vault::record::SecretRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) daemon_subkey_private: Option<crate::internal::secret_vault::record::SecretRef>,
    pub(crate) auth_jwt: crate::internal::secret_vault::record::SecretRef,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct IndexPayload {
    pub(crate) schema_version: i64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) default_credential_name: String,
    #[serde(default)]
    pub(crate) credentials: BTreeMap<String, IndexEntry>,
}

impl Default for IndexPayload {
    fn default() -> Self {
        Self {
            schema_version: INDEX_SCHEMA_VERSION,
            default_credential_name: String::new(),
            credentials: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Serialize)]
struct IdentityPayload {
    did: String,
    unique_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    created_at: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    user_id: String,
    #[serde(rename = "name", skip_serializing_if = "String::is_empty")]
    display_name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    handle: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    full_handle: String,
}

#[derive(Debug, Deserialize)]
struct SdkRegistryFile {
    #[serde(default)]
    default_identity: Option<String>,
    #[serde(default)]
    identities: Vec<SdkIdentityRecord>,
}

#[derive(Debug, Deserialize)]
struct SdkIdentityRecord {
    id: String,
    did: String,
    #[serde(default)]
    dir_name: Option<String>,
    #[serde(default)]
    handle: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    local_alias: Option<String>,
    #[serde(default)]
    is_default: bool,
}

fn parse_index_payload(raw: &[u8]) -> crate::ImResult<IndexPayload> {
    if let Ok(payload) = serde_json::from_slice::<IndexPayload>(raw) {
        return Ok(payload);
    }
    let sdk = serde_json::from_slice::<SdkRegistryFile>(raw).map_err(|err| {
        crate::ImError::Serialization {
            detail: err.to_string(),
        }
    })?;
    if sdk.default_identity.is_none() && sdk.identities.is_empty() {
        return Ok(IndexPayload::default());
    }
    Ok(sdk_registry_to_index(sdk))
}

fn sdk_registry_to_index(file: SdkRegistryFile) -> IndexPayload {
    let mut payload = IndexPayload {
        default_credential_name: file.default_identity.unwrap_or_default(),
        ..IndexPayload::default()
    };
    for record in file.identities {
        let alias = first_non_empty([
            record.local_alias.as_deref().unwrap_or_default(),
            &record.id,
        ])
        .unwrap_or_default()
        .to_string();
        if alias.is_empty() {
            continue;
        }
        if payload.default_credential_name.is_empty() && record.is_default {
            payload.default_credential_name = alias.clone();
        }
        let full_handle = record.handle.unwrap_or_default();
        let handle = full_handle
            .split('.')
            .next()
            .unwrap_or(full_handle.as_str())
            .to_string();
        payload.credentials.insert(
            alias.clone(),
            IndexEntry {
                credential_name: alias.clone(),
                dir_name: first_non_empty([
                    record.dir_name.as_deref().unwrap_or_default(),
                    record.local_alias.as_deref().unwrap_or_default(),
                    &record.id,
                ])
                .unwrap_or(&alias)
                .to_string(),
                did: record.did,
                unique_id: record.id,
                name: record.display_name.unwrap_or_default(),
                handle,
                full_handle,
                is_default: record.is_default,
                ..IndexEntry::default()
            },
        );
    }
    payload
}

fn normalize_index_payload(mut payload: IndexPayload) -> crate::ImResult<IndexPayload> {
    if !matches!(payload.schema_version, 0 | 2 | INDEX_SCHEMA_VERSION) {
        return Err(crate::ImError::invalid_input(
            Some("identity_registry.schema_version".to_string()),
            format!(
                "unsupported identity index schema version: {}",
                payload.schema_version
            ),
        ));
    }
    if payload.schema_version == 0 {
        payload.schema_version = INDEX_SCHEMA_VERSION;
    }
    if payload.default_credential_name.is_empty() && payload.credentials.contains_key("default") {
        payload.default_credential_name = "default".to_string();
    }
    let default_name = payload.default_credential_name.clone();
    let names = payload.credentials.keys().cloned().collect::<Vec<_>>();
    for name in names {
        if let Some(entry) = payload.credentials.get_mut(&name) {
            entry.credential_name = name.clone();
            entry.is_default = default_name == name;
        }
    }
    Ok(payload)
}

fn stored_handle_fields(handle: &str, full_handle: &str, did: &str) -> (String, String) {
    let mut local_part = handle.trim().to_lowercase();
    if let Some(stripped) = local_part.strip_prefix("wba://") {
        local_part = stripped.to_string();
    }
    if let Some(index) = local_part.find('.') {
        local_part.truncate(index);
    }
    let full_handle = full_handle.trim().to_lowercase();
    if !full_handle.is_empty() {
        if local_part.is_empty() {
            local_part = full_handle
                .split('.')
                .next()
                .unwrap_or_default()
                .to_string();
        }
        return (local_part, full_handle);
    }
    if local_part.is_empty() {
        return (String::new(), String::new());
    }
    let full = derive_full_handle_from_did(&local_part, did);
    (local_part, full)
}

fn derive_full_handle_from_did(handle: &str, did: &str) -> String {
    let local_part = handle.trim().to_lowercase();
    if local_part.is_empty() {
        return String::new();
    }
    let Some(domain) = did
        .strip_prefix("did:wba:")
        .and_then(|rest| rest.split(':').next())
    else {
        return String::new();
    };
    format!("{local_part}.{}", domain.trim().to_lowercase())
}

fn local_identity_dir(root: &Path, dir_name: &str) -> crate::ImResult<std::path::PathBuf> {
    let relative = Path::new(dir_name);
    if dir_name.trim().is_empty()
        || relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(crate::ImError::invalid_input(
            Some("identity".to_string()),
            "local identity directory name must be a simple relative path segment",
        ));
    }
    Ok(root.join(relative))
}

fn first_existing_path(root: &Path, names: &[&str]) -> std::path::PathBuf {
    names
        .iter()
        .map(|name| root.join(name))
        .find(|path| path.exists())
        .unwrap_or_else(|| root.join(names[0]))
}

fn preferred_dir_name(unique_id: &str) -> crate::ImResult<String> {
    let value = sanitize_component(unique_id);
    if value.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("unique_id".to_string()),
            "unique_id is required",
        ));
    }
    Ok(value)
}

fn sanitize_identity_name(raw: &str) -> String {
    sanitize_component(&raw.trim().to_ascii_lowercase())
}

fn sanitize_component(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    out.trim_matches(['.', '_', '-']).to_string()
}

fn first_non_empty<const N: usize>(values: [&str; N]) -> Option<&str> {
    values.into_iter().find(|value| !value.trim().is_empty())
}

fn nullable_string(value: &str) -> Value {
    let value = value.trim();
    if value.is_empty() {
        Value::Null
    } else {
        Value::String(value.to_string())
    }
}

fn write_secure_json(path: &Path, payload: &impl Serialize) -> crate::ImResult<()> {
    let raw = serde_json::to_vec_pretty(payload).map_err(|err| crate::ImError::Serialization {
        detail: err.to_string(),
    })?;
    fs::write(path, raw)?;
    set_private_file_mode(path)?;
    Ok(())
}

fn read_required_non_empty_text(
    root: &Path,
    names: &[&str],
    path_kind: &str,
) -> crate::ImResult<String> {
    let path = first_existing_path(root, names);
    match fs::read_to_string(&path) {
        Ok(value) if !value.trim().is_empty() => Ok(value),
        Ok(_) => Err(crate::ImError::CredentialFileUnreadable {
            path_kind: path_kind.to_string(),
            detail: "file is empty".to_string(),
        }),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            Err(crate::ImError::CredentialFileUnreadable {
                path_kind: path_kind.to_string(),
                detail: "file is missing".to_string(),
            })
        }
        Err(err) => Err(crate::ImError::CredentialFileUnreadable {
            path_kind: path_kind.to_string(),
            detail: err.to_string(),
        }),
    }
}

fn read_optional_non_empty_text(path: &Path, path_kind: &str) -> crate::ImResult<Option<String>> {
    match fs::read_to_string(path) {
        Ok(value) if !value.trim().is_empty() => Ok(Some(value)),
        Ok(_) => Ok(None),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(crate::ImError::CredentialFileUnreadable {
            path_kind: path_kind.to_string(),
            detail: err.to_string(),
        }),
    }
}

fn read_optional_file_or_default(path: &Path, default: &[u8]) -> crate::ImResult<Vec<u8>> {
    match fs::read(path) {
        Ok(raw) => Ok(raw),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(default.to_vec()),
        Err(err) => Err(crate::ImError::CredentialFileUnreadable {
            path_kind: "auth_state".to_string(),
            detail: err.to_string(),
        }),
    }
}

fn read_daemon_subkey_private_material(identity_dir: &Path) -> crate::ImResult<Option<String>> {
    let package_path = identity_dir.join(DAEMON_SUBKEY_PACKAGE_FILE_NAME);
    match fs::read(&package_path) {
        Ok(raw) => {
            let package: crate::identity::DaemonSubkeyPrivatePackage = serde_json::from_slice(&raw)
                .map_err(|err| crate::ImError::Serialization {
                    detail: err.to_string(),
                })?;
            let private_key = package.private_key_material();
            if private_key.trim().is_empty() {
                Ok(None)
            } else {
                Ok(Some(private_key.to_string()))
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => read_optional_non_empty_text(
            &identity_dir.join(DAEMON_SUBKEY_PRIVATE_FILE_NAME),
            "daemon_subkey_private",
        ),
        Err(err) => Err(crate::ImError::CredentialFileUnreadable {
            path_kind: "daemon_subkey_package".to_string(),
            detail: err.to_string(),
        }),
    }
}

fn seal_identity_input_to_vault(
    input: &SaveIdentityInput,
    workspace_id: &str,
    device_id: &str,
    vault: &dyn crate::internal::secret_vault::SecretVault,
) -> crate::ImResult<IdentityVaultMigrationMetadata> {
    let identity_id = input.unique_id.trim();
    let did = input.did.as_str();
    let auth_state_raw = crate::internal::auth::state::auth_state_json_for_token(&input.jwt_token)?;
    crate::internal::auth::state::parse_auth_state(&auth_state_raw)?;

    let default_signing_private = seal_utf8_secret(
        vault,
        vault_secret_metadata(
            workspace_id,
            device_id,
            identity_id,
            did,
            crate::internal::secret_vault::record::SecretKind::IdentityRootPrivate,
            "key-1",
        ),
        &input.key1_private_pem,
    )?;
    let e2ee_signing_private = if input.e2ee_signing_private_pem.trim().is_empty() {
        None
    } else {
        Some(seal_utf8_secret(
            vault,
            vault_secret_metadata(
                workspace_id,
                device_id,
                identity_id,
                did,
                crate::internal::secret_vault::record::SecretKind::IdentityE2eeSigningPrivate,
                "key-2",
            ),
            &input.e2ee_signing_private_pem,
        )?)
    };
    let e2ee_agreement_private = seal_utf8_secret(
        vault,
        vault_secret_metadata(
            workspace_id,
            device_id,
            identity_id,
            did,
            crate::internal::secret_vault::record::SecretKind::IdentityE2eeAgreementPrivate,
            "key-3",
        ),
        &input.e2ee_agreement_private_pem,
    )?;
    let daemon_subkey_private = match &input.daemon_subkey_package {
        Some(package) if !package.private_key_material().trim().is_empty() => {
            Some(seal_utf8_secret(
                vault,
                vault_secret_metadata(
                    workspace_id,
                    device_id,
                    identity_id,
                    did,
                    crate::internal::secret_vault::record::SecretKind::IdentityDaemonPrivate,
                    "daemon-key-1",
                ),
                package.private_key_material(),
            )?)
        }
        _ => None,
    };
    let auth_jwt = vault.seal(crate::internal::secret_vault::SealSecretRequest {
        metadata: vault_secret_metadata(
            workspace_id,
            device_id,
            identity_id,
            did,
            crate::internal::secret_vault::record::SecretKind::AuthJwt,
            AUTH_FILE_NAME,
        ),
        plaintext: crate::internal::platform_secret::SecretBytes::from_vec(auth_state_raw.clone()),
    })?;

    verify_vault_utf8_secret(vault, &default_signing_private, &input.key1_private_pem)?;
    if let Some(secret_ref) = &e2ee_signing_private {
        verify_vault_utf8_secret(vault, secret_ref, &input.e2ee_signing_private_pem)?;
    }
    verify_vault_utf8_secret(
        vault,
        &e2ee_agreement_private,
        &input.e2ee_agreement_private_pem,
    )?;
    if let (Some(secret_ref), Some(package)) =
        (&daemon_subkey_private, input.daemon_subkey_package.as_ref())
    {
        verify_vault_utf8_secret(vault, secret_ref, package.private_key_material())?;
    }
    let opened_auth = vault.open(&auth_jwt)?;
    if opened_auth.expose_secret() != auth_state_raw.as_slice() {
        return Err(crate::ImError::PermissionDenied);
    }
    crate::internal::auth::state::parse_auth_state(opened_auth.expose_secret())?;

    Ok(IdentityVaultMigrationMetadata {
        schema_version: IDENTITY_VAULT_MIGRATION_SCHEMA_VERSION,
        status: IdentityVaultMigrationStatus::Verified,
        backend: "vault".to_owned(),
        unlock_policy: "explicit_root_key".to_owned(),
        migrated_at: now_rfc3339(),
        workspace_id: workspace_id.to_owned(),
        device_id: device_id.to_owned(),
        plaintext_compat_retained: false,
        refs: IdentityVaultSecretRefs {
            default_signing_private,
            e2ee_signing_private,
            e2ee_agreement_private,
            daemon_subkey_private,
            auth_jwt,
        },
    })
}

fn ensure_verified_vault_metadata_context(
    metadata: &IdentityVaultMigrationMetadata,
    workspace_id: &str,
    device_id: &str,
) -> crate::ImResult<()> {
    if !matches!(metadata.status, IdentityVaultMigrationStatus::Verified) {
        return Err(crate::ImError::IdentityNotReady {
            identity: metadata
                .refs
                .default_signing_private
                .did
                .clone()
                .unwrap_or_default(),
            missing: vec!["identity_vault_metadata_verified".to_string()],
        });
    }
    if metadata.workspace_id != workspace_id || metadata.device_id != device_id {
        return Err(crate::ImError::IdentityNotReady {
            identity: metadata
                .refs
                .default_signing_private
                .did
                .clone()
                .unwrap_or_default(),
            missing: vec!["identity_vault_context_mismatch".to_string()],
        });
    }
    Ok(())
}

fn open_vault_utf8_secret(
    vault: &dyn crate::internal::secret_vault::SecretVault,
    secret_ref: &crate::internal::secret_vault::record::SecretRef,
    path_kind: &str,
) -> crate::ImResult<String> {
    let secret = vault.open(secret_ref)?;
    let value = String::from_utf8(secret.expose_secret().to_vec()).map_err(|_| {
        crate::ImError::CredentialFileUnreadable {
            path_kind: path_kind.to_string(),
            detail: "vault secret is not valid utf-8".to_string(),
        }
    })?;
    if value.trim().is_empty() {
        return Err(crate::ImError::CredentialFileUnreadable {
            path_kind: path_kind.to_string(),
            detail: "vault secret is empty".to_string(),
        });
    }
    Ok(value)
}

#[derive(Debug, Serialize)]
struct SanitizedDaemonSubkeyPackage<'a> {
    schema: &'a str,
    user_did: &'a crate::ids::Did,
    verification_method: &'a str,
    key_type: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    key_algorithm: Option<&'a str>,
    public_key_multibase: &'a str,
    private_key_encoding: &'a str,
    private_key_storage: &'static str,
}

fn write_sanitized_daemon_subkey_package(
    path: &Path,
    package: &crate::identity::DaemonSubkeyPrivatePackage,
) -> crate::ImResult<()> {
    let private_key_encoding = if package.private_key_encoding.trim().is_empty() {
        crate::identity::DAEMON_SUBKEY_PRIVATE_KEY_ENCODING_PEM
    } else {
        package.private_key_encoding.trim()
    };
    write_secure_json(
        path,
        &SanitizedDaemonSubkeyPackage {
            schema: crate::identity::DAEMON_SUBKEY_PACKAGE_SCHEMA_V2,
            user_did: &package.user_did,
            verification_method: &package.verification_method,
            key_type: &package.key_type,
            key_algorithm: package.key_algorithm.as_deref(),
            public_key_multibase: &package.public_key_multibase,
            private_key_encoding,
            private_key_storage: "vault",
        },
    )
}

fn remove_known_plaintext_secret_files(identity_dir: &Path) -> crate::ImResult<()> {
    for name in [
        AUTH_FILE_NAME,
        KEY1_PRIVATE_FILE_NAME,
        "private.key",
        E2EE_SIGNING_PRIVATE_FILE_NAME,
        E2EE_AGREEMENT_PRIVATE_FILE_NAME,
        "key-3-private.pem",
        DAEMON_SUBKEY_PRIVATE_FILE_NAME,
    ] {
        remove_file_if_exists(&identity_dir.join(name))?;
    }
    Ok(())
}

fn remove_file_if_exists(path: &Path) -> crate::ImResult<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(crate::ImError::from(err)),
    }
}

fn vault_secret_metadata(
    workspace_id: &str,
    device_id: &str,
    identity_id: &str,
    did: &str,
    kind: crate::internal::secret_vault::record::SecretKind,
    key_id: &str,
) -> crate::internal::secret_vault::record::SecretMetadata {
    crate::internal::secret_vault::record::SecretMetadata {
        workspace_id: workspace_id.to_string(),
        device_id: device_id.to_string(),
        identity_id: Some(identity_id.to_string()).filter(|value| !value.trim().is_empty()),
        did: Some(did.to_string()).filter(|value| !value.trim().is_empty()),
        kind,
        key_id: key_id.to_string(),
        key_version: 1,
        policy: crate::internal::secret_vault::policy::SecretAccessPolicy::no_prompt_local_secret(),
    }
}

fn seal_utf8_secret(
    vault: &dyn crate::internal::secret_vault::SecretVault,
    metadata: crate::internal::secret_vault::record::SecretMetadata,
    value: &str,
) -> crate::ImResult<crate::internal::secret_vault::record::SecretRef> {
    vault.seal(crate::internal::secret_vault::SealSecretRequest {
        metadata,
        plaintext: crate::internal::platform_secret::SecretBytes::from_vec(
            value.as_bytes().to_vec(),
        ),
    })
}

fn verify_vault_utf8_secret(
    vault: &dyn crate::internal::secret_vault::SecretVault,
    secret_ref: &crate::internal::secret_vault::record::SecretRef,
    expected: &str,
) -> crate::ImResult<()> {
    let opened = vault.open(secret_ref)?;
    if opened.expose_secret() != expected.as_bytes() {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

fn write_secure_text_if_present(path: &Path, payload: &str) -> crate::ImResult<()> {
    if payload.trim().is_empty() {
        return Ok(());
    }
    fs::write(path, payload)?;
    set_private_file_mode(path)?;
    Ok(())
}

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[cfg(unix)]
fn set_private_dir_mode(path: &Path) -> crate::ImResult<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_private_dir_mode(_path: &Path) -> crate::ImResult<()> {
    Ok(())
}

#[cfg(unix)]
fn set_private_file_mode(path: &Path) -> crate::ImResult<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_private_file_mode(_path: &Path) -> crate::ImResult<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::internal::key_provider::KeyMaterialProvider;
    use crate::internal::platform_secret::DeviceVaultRootKey;
    use crate::internal::secret_vault::record::SecretRef;
    use crate::internal::secret_vault::{FileSecretVault, FileSecretVaultStore, SecretVault};
    use std::sync::Arc;

    #[test]
    fn empty_sdk_registry_parses_as_empty_index() {
        let index = parse_index_payload(br#"{"identities":[]}"#).unwrap();

        assert_eq!(index.schema_version, INDEX_SCHEMA_VERSION);
        assert!(index.default_credential_name.is_empty());
        assert!(index.credentials.is_empty());
    }

    #[test]
    fn identity_vault_migration_seals_and_marks_verified_without_deleting_plaintext() {
        let root = tempfile::tempdir().unwrap();
        let paths = test_paths(root.path());
        let store = IdentityStore::new(&paths);
        let did = crate::ids::Did::parse("did:example:alice").unwrap();
        let daemon_package = crate::identity::DaemonSubkeyPrivatePackage::new_v2_pem(
            did.clone(),
            "did:example:alice#daemon-key-1".to_owned(),
            "Multikey".to_owned(),
            Some("Ed25519".to_owned()),
            "zDaemonPublic".to_owned(),
            "daemon-private-secret".to_owned(),
        );
        store
            .save_identity(SaveIdentityInput {
                local_alias: "alice".to_owned(),
                did,
                unique_id: "alice-id".to_owned(),
                user_id: "user-1".to_owned(),
                display_name: "Alice".to_owned(),
                handle: "alice".to_owned(),
                full_handle: "alice.example".to_owned(),
                jwt_token: "jwt-secret-value".to_owned(),
                did_document: Some(json!({"id": "did:example:alice"})),
                key1_private_pem: "signing-private-secret".to_owned(),
                key1_public_pem: "signing-public".to_owned(),
                e2ee_signing_private_pem: "e2ee-signing-secret".to_owned(),
                e2ee_agreement_private_pem: "e2ee-agreement-secret".to_owned(),
                daemon_subkey_package: Some(daemon_package),
                make_default: true,
            })
            .unwrap();
        let vault = Arc::new(FileSecretVault::new(
            DeviceVaultRootKey::from_bytes([8_u8; 32]),
            FileSecretVaultStore::new(root.path().join("vault")),
        ));

        let result = store
            .migrate_identity_to_vault("alice", "workspace-a", "device-a", vault.as_ref())
            .unwrap();

        assert_eq!(result.local_alias, "alice");
        assert_eq!(result.dir_name, "alice-id");
        assert_eq!(
            result.metadata.status,
            IdentityVaultMigrationStatus::Verified
        );
        assert_eq!(result.metadata.backend, "vault");
        assert_eq!(result.metadata.unlock_policy, "explicit_root_key");
        assert!(result.metadata.plaintext_compat_retained);
        assert_eq!(
            open_utf8(
                vault.as_ref(),
                &result.metadata.refs.default_signing_private
            ),
            "signing-private-secret"
        );
        assert_eq!(
            open_utf8(vault.as_ref(), &result.metadata.refs.e2ee_agreement_private),
            "e2ee-agreement-secret"
        );
        assert_eq!(
            open_utf8(
                vault.as_ref(),
                result.metadata.refs.e2ee_signing_private.as_ref().unwrap()
            ),
            "e2ee-signing-secret"
        );
        assert_eq!(
            open_utf8(
                vault.as_ref(),
                result.metadata.refs.daemon_subkey_private.as_ref().unwrap()
            ),
            "daemon-private-secret"
        );

        let identity_dir = paths.identity_root_dir.join("alice-id");
        assert_eq!(
            std::fs::read_to_string(identity_dir.join(KEY1_PRIVATE_FILE_NAME)).unwrap(),
            "signing-private-secret"
        );
        assert_eq!(
            std::fs::read_to_string(identity_dir.join(E2EE_AGREEMENT_PRIVATE_FILE_NAME)).unwrap(),
            "e2ee-agreement-secret"
        );
        assert!(identity_dir.join(AUTH_FILE_NAME).exists());

        let provider = crate::internal::key_provider::vault::VaultBackedKeyMaterialProvider::new(
            identity_dir,
            vault,
            result.metadata.key_material_refs(),
        );
        assert_eq!(
            provider.default_signing_private_pem().unwrap(),
            "signing-private-secret"
        );
        assert_eq!(
            provider.e2ee_agreement_private_pem().unwrap(),
            "e2ee-agreement-secret"
        );
        assert_eq!(
            provider.valid_auth_token().unwrap().as_deref(),
            Some("jwt-secret-value")
        );

        let index = store.load_index().unwrap();
        assert!(index
            .credentials
            .get("alice")
            .unwrap()
            .vault_migration
            .is_some());
    }

    #[test]
    fn identity_vault_migration_verify_failure_keeps_file_backend_metadata() {
        let root = tempfile::tempdir().unwrap();
        let paths = test_paths(root.path());
        let store = IdentityStore::new(&paths);
        store
            .save_identity(SaveIdentityInput {
                local_alias: "alice".to_owned(),
                did: crate::ids::Did::parse("did:example:alice").unwrap(),
                unique_id: "alice-id".to_owned(),
                user_id: "user-1".to_owned(),
                display_name: "Alice".to_owned(),
                handle: "alice".to_owned(),
                full_handle: "alice.example".to_owned(),
                jwt_token: "jwt-secret-value".to_owned(),
                did_document: Some(json!({"id": "did:example:alice"})),
                key1_private_pem: "signing-private-secret".to_owned(),
                key1_public_pem: "signing-public".to_owned(),
                e2ee_signing_private_pem: String::new(),
                e2ee_agreement_private_pem: "e2ee-agreement-secret".to_owned(),
                daemon_subkey_package: None,
                make_default: true,
            })
            .unwrap();
        let failing_vault = AlwaysFailsOpenVault {
            inner: FileSecretVault::new(
                DeviceVaultRootKey::from_bytes([9_u8; 32]),
                FileSecretVaultStore::new(root.path().join("vault")),
            ),
        };

        let err = store
            .migrate_identity_to_vault("alice", "workspace-a", "device-a", &failing_vault)
            .unwrap_err();

        assert_eq!(err, crate::ImError::PermissionDenied);
        let index = store.load_index().unwrap();
        assert!(index
            .credentials
            .get("alice")
            .unwrap()
            .vault_migration
            .is_none());
        let identity_dir = paths.identity_root_dir.join("alice-id");
        assert_eq!(
            std::fs::read_to_string(identity_dir.join(KEY1_PRIVATE_FILE_NAME)).unwrap(),
            "signing-private-secret"
        );
        assert_eq!(
            std::fs::read_to_string(identity_dir.join(AUTH_FILE_NAME)).unwrap(),
            "{\n  \"jwt_token\": \"jwt-secret-value\"\n}"
        );
    }

    #[test]
    fn save_identity_with_vault_seals_refs_and_omits_plaintext_files() {
        let root = tempfile::tempdir().unwrap();
        let paths = test_paths(root.path());
        let store = IdentityStore::new(&paths);
        let did = crate::ids::Did::parse("did:example:alice").unwrap();
        let daemon_package = crate::identity::DaemonSubkeyPrivatePackage::new_v2_pem(
            did.clone(),
            "did:example:alice#daemon-key-1".to_owned(),
            "Multikey".to_owned(),
            Some("Ed25519".to_owned()),
            "zDaemonPublic".to_owned(),
            "daemon-private-secret".to_owned(),
        );
        let vault = Arc::new(FileSecretVault::new(
            DeviceVaultRootKey::from_bytes([11_u8; 32]),
            FileSecretVaultStore::new(root.path().join("vault")),
        ));

        let stored = store
            .save_identity_with_secret_storage(
                SaveIdentityInput {
                    local_alias: "alice".to_owned(),
                    did,
                    unique_id: "alice-id".to_owned(),
                    user_id: "user-1".to_owned(),
                    display_name: "Alice".to_owned(),
                    handle: "alice".to_owned(),
                    full_handle: "alice.example".to_owned(),
                    jwt_token: "jwt-secret-value".to_owned(),
                    did_document: Some(json!({"id": "did:example:alice"})),
                    key1_private_pem: "signing-private-secret".to_owned(),
                    key1_public_pem: "signing-public".to_owned(),
                    e2ee_signing_private_pem: "e2ee-signing-secret".to_owned(),
                    e2ee_agreement_private_pem: "e2ee-agreement-secret".to_owned(),
                    daemon_subkey_package: Some(daemon_package),
                    make_default: true,
                },
                SaveIdentitySecretStorage::Vault {
                    workspace_id: "workspace-a".to_owned(),
                    device_id: "device-a".to_owned(),
                    vault: vault.clone(),
                },
            )
            .unwrap();

        assert_eq!(stored.local_alias, "alice");
        let identity_dir = paths.identity_root_dir.join("alice-id");
        assert!(identity_dir.join(IDENTITY_FILE_NAME).exists());
        assert!(identity_dir.join(DID_DOCUMENT_FILE_NAME).exists());
        assert_eq!(
            std::fs::read_to_string(identity_dir.join(KEY1_PUBLIC_FILE_NAME)).unwrap(),
            "signing-public"
        );
        for name in [
            AUTH_FILE_NAME,
            KEY1_PRIVATE_FILE_NAME,
            "private.key",
            E2EE_SIGNING_PRIVATE_FILE_NAME,
            E2EE_AGREEMENT_PRIVATE_FILE_NAME,
            "key-3-private.pem",
            DAEMON_SUBKEY_PRIVATE_FILE_NAME,
        ] {
            assert!(!identity_dir.join(name).exists(), "{name} should not exist");
        }

        let package_text =
            std::fs::read_to_string(identity_dir.join(DAEMON_SUBKEY_PACKAGE_FILE_NAME)).unwrap();
        assert!(package_text.contains(r#""private_key_storage": "vault""#));
        assert!(!package_text.contains("private_key_pem"));
        assert!(!package_text.contains("private_key_multibase"));
        assert!(!package_text.contains("daemon-private-secret"));

        let index = store.load_index().unwrap();
        let metadata = index
            .credentials
            .get("alice")
            .unwrap()
            .vault_migration
            .as_ref()
            .unwrap();
        assert_eq!(metadata.status, IdentityVaultMigrationStatus::Verified);
        assert!(!metadata.plaintext_compat_retained);
        assert_eq!(
            open_utf8(vault.as_ref(), &metadata.refs.default_signing_private),
            "signing-private-secret"
        );
        assert_eq!(
            open_utf8(vault.as_ref(), &metadata.refs.e2ee_agreement_private),
            "e2ee-agreement-secret"
        );
        assert_eq!(
            open_utf8(
                vault.as_ref(),
                metadata.refs.e2ee_signing_private.as_ref().unwrap()
            ),
            "e2ee-signing-secret"
        );
        assert_eq!(
            open_utf8(
                vault.as_ref(),
                metadata.refs.daemon_subkey_private.as_ref().unwrap()
            ),
            "daemon-private-secret"
        );
        let auth_raw = vault.open(&metadata.refs.auth_jwt).unwrap();
        assert_eq!(
            crate::internal::auth::state::parse_auth_state(auth_raw.expose_secret())
                .unwrap()
                .bearer_token
                .as_deref(),
            Some("jwt-secret-value")
        );

        let persisted_text = collect_text_files(&paths.identity_root_dir);
        for secret in [
            "signing-private-secret",
            "e2ee-signing-secret",
            "e2ee-agreement-secret",
            "daemon-private-secret",
            "jwt-secret-value",
            "-----BEGIN PRIVATE KEY-----",
        ] {
            assert!(
                !persisted_text.contains(secret),
                "identity root leaked secret marker {secret}"
            );
        }
    }

    #[test]
    fn save_identity_with_vault_verify_failure_leaves_no_plaintext_identity() {
        let root = tempfile::tempdir().unwrap();
        let paths = test_paths(root.path());
        let store = IdentityStore::new(&paths);
        let failing_vault = Arc::new(AlwaysFailsOpenVault {
            inner: FileSecretVault::new(
                DeviceVaultRootKey::from_bytes([12_u8; 32]),
                FileSecretVaultStore::new(root.path().join("vault")),
            ),
        });

        let err = store
            .save_identity_with_secret_storage(
                SaveIdentityInput {
                    local_alias: "alice".to_owned(),
                    did: crate::ids::Did::parse("did:example:alice").unwrap(),
                    unique_id: "alice-id".to_owned(),
                    user_id: "user-1".to_owned(),
                    display_name: "Alice".to_owned(),
                    handle: "alice".to_owned(),
                    full_handle: "alice.example".to_owned(),
                    jwt_token: "jwt-secret-value".to_owned(),
                    did_document: Some(json!({"id": "did:example:alice"})),
                    key1_private_pem: "signing-private-secret".to_owned(),
                    key1_public_pem: "signing-public".to_owned(),
                    e2ee_signing_private_pem: String::new(),
                    e2ee_agreement_private_pem: "e2ee-agreement-secret".to_owned(),
                    daemon_subkey_package: None,
                    make_default: true,
                },
                SaveIdentitySecretStorage::Vault {
                    workspace_id: "workspace-a".to_owned(),
                    device_id: "device-a".to_owned(),
                    vault: failing_vault,
                },
            )
            .unwrap_err();

        assert_eq!(err, crate::ImError::PermissionDenied);
        assert!(store.load_index().unwrap().credentials.is_empty());
        let persisted_text = collect_text_files(&paths.identity_root_dir);
        for secret in [
            "signing-private-secret",
            "e2ee-agreement-secret",
            "jwt-secret-value",
        ] {
            assert!(
                !persisted_text.contains(secret),
                "failed secure save leaked secret marker {secret}"
            );
        }
    }

    #[derive(Debug)]
    struct AlwaysFailsOpenVault {
        inner: FileSecretVault,
    }

    impl SecretVault for AlwaysFailsOpenVault {
        fn seal(
            &self,
            request: crate::internal::secret_vault::SealSecretRequest,
        ) -> crate::ImResult<SecretRef> {
            self.inner.seal(request)
        }

        fn open(
            &self,
            _secret_ref: &crate::internal::secret_vault::record::SecretRef,
        ) -> crate::ImResult<crate::internal::platform_secret::SecretBytes> {
            Err(crate::ImError::PermissionDenied)
        }

        fn delete(
            &self,
            secret_ref: &crate::internal::secret_vault::record::SecretRef,
        ) -> crate::ImResult<()> {
            self.inner.delete(secret_ref)
        }

        fn list(&self) -> crate::ImResult<Vec<SecretRef>> {
            self.inner.list()
        }
    }

    fn test_paths(root: &Path) -> crate::paths::IdentityRegistryPaths {
        crate::paths::IdentityRegistryPaths {
            identity_root_dir: root.join("identities"),
            registry_path: root.join("identities").join("registry.json"),
            default_identity_path: Some(root.join("identities").join("default")),
        }
    }

    fn open_utf8(vault: &dyn SecretVault, secret_ref: &SecretRef) -> String {
        String::from_utf8(vault.open(secret_ref).unwrap().expose_secret().to_vec()).unwrap()
    }

    fn collect_text_files(root: &Path) -> String {
        let mut out = String::new();
        collect_text_files_inner(root, &mut out);
        out
    }

    fn collect_text_files_inner(root: &Path, out: &mut String) {
        let entries = match std::fs::read_dir(root) {
            Ok(entries) => entries,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if metadata.is_dir() {
                collect_text_files_inner(&path, out);
            } else if metadata.is_file() {
                if path
                    .components()
                    .any(|component| component.as_os_str() == "vault")
                {
                    continue;
                }
                if let Ok(text) = std::fs::read_to_string(&path) {
                    out.push_str(&text);
                    out.push('\n');
                }
            }
        }
    }
}
