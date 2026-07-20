use fs2::FileExt as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use zeroize::Zeroizing;

const INDEX_SCHEMA_VERSION: i64 = 5;
const IDENTITY_FILE_NAME: &str = "identity.json";
const AUTH_FILE_NAME: &str = "auth.json";
const DID_DOCUMENT_FILE_NAME: &str = "did_document.json";
const KEY1_PRIVATE_FILE_NAME: &str = "key-1-private.pem";
const KEY1_PUBLIC_FILE_NAME: &str = "key-1-public.pem";
const E2EE_SIGNING_PRIVATE_FILE_NAME: &str = "e2ee-signing-private.pem";
const E2EE_AGREEMENT_PRIVATE_FILE_NAME: &str = "e2ee-agreement-private.pem";
const DAEMON_SUBKEY_PRIVATE_FILE_NAME: &str = "daemon-key-1-private.pem";
const DAEMON_SUBKEY_PACKAGE_FILE_NAME: &str = "daemon-subkey-package.json";
const IDENTITY_INDEX_MUTATION_LOCK_FILE: &str = ".identity-index-mutation.lock";
pub(crate) const IDENTITY_VAULT_MIGRATION_SCHEMA_VERSION: u32 = 1;

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
    pub(crate) key_mode: SaveIdentityKeyMode,
    pub(crate) device_state: Option<crate::internal::identity_device_state::IdentityDeviceState>,
    pub(crate) key1_private_pem: String,
    pub(crate) key1_public_pem: String,
    pub(crate) e2ee_signing_private_pem: String,
    pub(crate) e2ee_agreement_private_pem: String,
    pub(crate) daemon_subkey_package: Option<crate::identity::DaemonSubkeyPrivatePackage>,
    pub(crate) make_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SaveIdentityKeyMode {
    LegacyKey1,
    VNext {
        root_key_id: String,
        device_signing_key_id: String,
        device_e2ee_key_id: String,
    },
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
                        device_id: context.vault_context_device_id().as_str().to_owned(),
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
                    device_id: context.vault_context_device_id().as_str().to_owned(),
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

/// Process-wide/repository-wide serialization token for identity-index
/// load-modify-save operations. Atomic rename protects crash integrity only;
/// this lock additionally prevents a stale writer from replacing a newer
/// complete index image.
pub(crate) struct IdentityIndexMutationLock {
    file: fs::File,
    registry_path: PathBuf,
}

impl Drop for IdentityIndexMutationLock {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.file);
    }
}

impl<'a> IdentityStore<'a> {
    pub(crate) fn new(paths: &'a crate::paths::IdentityRegistryPaths) -> Self {
        Self { paths }
    }

    /// Resolves one identity index directory without allowing traversal or an
    /// existing filesystem object that could redirect identity-local writes.
    pub(crate) fn local_identity_dir(&self, dir_name: &str) -> crate::ImResult<PathBuf> {
        local_identity_dir(&self.paths.identity_root_dir, dir_name)
    }

    pub(crate) fn save_identity(
        &self,
        input: SaveIdentityInput,
    ) -> crate::ImResult<StoredIdentity> {
        self.save_identity_with_secret_storage(input, SaveIdentitySecretStorage::FileCompat)
    }

    pub(crate) fn save_identity_with_secret_storage(
        &self,
        input: SaveIdentityInput,
        secret_storage: SaveIdentitySecretStorage,
    ) -> crate::ImResult<StoredIdentity> {
        let lock = self.lock_index_mutation()?;
        self.save_identity_with_secret_storage_locked(input, secret_storage, &lock)
    }

    fn save_identity_with_secret_storage_locked(
        &self,
        mut input: SaveIdentityInput,
        secret_storage: SaveIdentitySecretStorage,
        lock: &IdentityIndexMutationLock,
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
        if let Some(state) = input.device_state.as_ref() {
            state.validate_for_did(&input.did)?;
        }
        let (handle, full_handle) =
            stored_handle_fields(&input.handle, &input.full_handle, input.did.as_str());
        input.handle = handle;
        input.full_handle = full_handle;

        let mut index = self.load_index()?;
        let preserved_root_import =
            preserve_imported_root_for_resave(&index, &local_alias, &input, &secret_storage)?;
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
        let identity_dir = self.local_identity_dir(&dir_name)?;
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
        fs::create_dir_all(&self.paths.identity_root_dir)?;
        set_private_dir_mode(&self.paths.identity_root_dir)?;
        let vault_metadata = if let Some(preserved) = preserved_root_import.as_ref() {
            // Imported-root identities preserve all deterministic Vault refs.
            // The preflight below has already proven immutable device/root
            // material and auth state byte-for-byte, so ordinary save performs
            // no Vault write. Token rotation uses its dedicated mutation path.
            Some(preserved.metadata.clone())
        } else {
            match &secret_storage {
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
            }
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
        let device_state = input.device_state.or_else(|| {
            index
                .credentials
                .get(&local_alias)
                .and_then(|entry| entry.device_state.clone())
        });
        let root_key_import = preserved_root_import.map(|preserved| preserved.import);
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
                device_state,
                root_key_import,
            },
        );
        self.save_index_locked(lock, index)?;
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

    pub(crate) fn save_recovered_identity_with_secret_storage(
        &self,
        mut input: SaveIdentityInput,
        secret_storage: SaveIdentitySecretStorage,
        archived_identity_names: &[String],
    ) -> crate::ImResult<StoredIdentity> {
        let lock = self.lock_index_mutation()?;
        let original = self.load_index()?;
        let mut prepared = original.clone();
        let archived = archived_identity_names
            .iter()
            .map(|name| name.trim())
            .filter(|name| !name.is_empty())
            .collect::<std::collections::BTreeSet<_>>();
        let default_was_archived = archived.contains(prepared.default_credential_name.trim());
        prepared
            .credentials
            .retain(|name, _| !archived.contains(name.as_str()));
        if default_was_archived {
            prepared.default_credential_name.clear();
            input.make_default = true;
        }
        self.save_index_locked(&lock, prepared)?;
        match self.save_identity_with_secret_storage_locked(input, secret_storage, &lock) {
            Ok(stored) => Ok(stored),
            Err(error) => {
                let _ = self.save_index_locked(&lock, original);
                Err(error)
            }
        }
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

    /// Replaces the Vault auth record with the complete rotating device token
    /// pair. The refresh token is never projected into ordinary identity files.
    pub(crate) fn persist_vnext_auth_token_pair(
        &self,
        local_alias: &str,
        access_token: &str,
        refresh_token: &str,
        token_expires_at: &str,
        secret_storage: &SaveIdentitySecretStorage,
    ) -> crate::ImResult<()> {
        let SaveIdentitySecretStorage::Vault {
            workspace_id,
            device_id,
            vault,
        } = secret_storage
        else {
            return Err(crate::ImError::LocalStateUnavailable {
                detail: "vNext device refresh tokens require Vault storage".to_owned(),
            });
        };
        let alias = sanitize_identity_name(local_alias);
        let index = self.load_index()?;
        let entry =
            index
                .credentials
                .get(&alias)
                .ok_or_else(|| crate::ImError::IdentityNotFound {
                    selector: alias.clone(),
                })?;
        if entry.device_state.as_ref().is_none_or(|state| {
            state.mode != crate::internal::identity_device_state::IdentityDeviceMode::VNext
        }) {
            return Err(crate::ImError::PermissionDenied);
        }
        let metadata = entry
            .vault_migration
            .as_ref()
            .filter(|metadata| {
                metadata.workspace_id == *workspace_id
                    && metadata.device_id == *device_id
                    && metadata.status == IdentityVaultMigrationStatus::Verified
            })
            .ok_or(crate::ImError::PermissionDenied)?;
        let auth_ref = metadata
            .vnext_refs
            .as_ref()
            .map(|refs| &refs.auth_jwt)
            .filter(|secret_ref| {
                secret_ref.workspace_id == *workspace_id
                    && secret_ref.device_id == *device_id
                    && secret_ref.kind == crate::internal::secret_vault::record::SecretKind::AuthJwt
            })
            .ok_or(crate::ImError::PermissionDenied)?;
        let raw = crate::internal::auth::state::auth_state_json_for_token_pair(
            access_token,
            Some(refresh_token),
            Some(token_expires_at),
        )?;
        let sealed = vault.seal(crate::internal::secret_vault::SealSecretRequest {
            metadata: crate::internal::secret_vault::record::SecretMetadata {
                workspace_id: auth_ref.workspace_id.clone(),
                device_id: auth_ref.device_id.clone(),
                identity_id: auth_ref.identity_id.clone(),
                did: auth_ref.did.clone(),
                kind: auth_ref.kind.clone(),
                key_id: auth_ref.key_id.clone(),
                key_version: auth_ref.key_version,
                policy: crate::internal::secret_vault::policy::SecretAccessPolicy::no_prompt_local_secret(),
            },
            plaintext: crate::internal::platform_secret::SecretBytes::from_vec(raw),
        })?;
        let opened = vault.open(&sealed)?;
        let snapshot = crate::internal::auth::state::parse_auth_state(opened.expose_secret())?;
        if snapshot.bearer_token.as_deref() != Some(access_token.trim())
            || snapshot.refresh_token.as_deref() != Some(refresh_token.trim())
        {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(())
    }

    /// Returns the stable token-issue operation for one imported-root ACK,
    /// persisting `candidate` before its first remote use.
    pub(crate) fn reserve_root_import_management_token_operation(
        &self,
        local_alias: &str,
        completed_message_id: &str,
        candidate: &str,
    ) -> crate::ImResult<String> {
        validate_root_import_token_operation(candidate)?;
        let alias = sanitize_identity_name(local_alias);
        let lock = self.lock_index_mutation()?;
        let mut index = self.load_index()?;
        let record = index
            .credentials
            .get_mut(&alias)
            .and_then(|entry| entry.root_key_import.as_mut())
            .filter(|record| record.message_id() == completed_message_id)
            .ok_or(crate::ImError::PermissionDenied)?;
        if let Some(existing) = record.management_token_operation_id.as_deref() {
            validate_root_import_token_operation(existing)?;
            return Ok(existing.to_owned());
        }
        record.management_token_operation_id = Some(candidate.to_owned());
        self.save_index_locked(&lock, index)?;
        Ok(candidate.to_owned())
    }

    /// Rotates an expired token-issue operation with compare-and-swap. If a
    /// concurrent retry already rotated it, that winning operation is reused.
    pub(crate) fn rotate_root_import_management_token_operation(
        &self,
        local_alias: &str,
        completed_message_id: &str,
        expected: &str,
        replacement: &str,
    ) -> crate::ImResult<String> {
        validate_root_import_token_operation(expected)?;
        validate_root_import_token_operation(replacement)?;
        if expected == replacement {
            return Err(crate::ImError::PermissionDenied);
        }
        let alias = sanitize_identity_name(local_alias);
        let lock = self.lock_index_mutation()?;
        let mut index = self.load_index()?;
        let record = index
            .credentials
            .get_mut(&alias)
            .and_then(|entry| entry.root_key_import.as_mut())
            .filter(|record| record.message_id() == completed_message_id)
            .ok_or(crate::ImError::PermissionDenied)?;
        let current = record
            .management_token_operation_id
            .as_deref()
            .ok_or(crate::ImError::PermissionDenied)?;
        validate_root_import_token_operation(current)?;
        if current != expected {
            return Ok(current.to_owned());
        }
        record.management_token_operation_id = Some(replacement.to_owned());
        self.save_index_locked(&lock, index)?;
        Ok(replacement.to_owned())
    }

    /// Loads the auth ref from an already committed imported-root transition.
    /// Exact ACK retries use this to repair a live provider whose first ref
    /// advance failed after the index commit.
    pub(crate) fn committed_root_import_management_auth_ref(
        &self,
        local_alias: &str,
        completed_message_id: &str,
        auth_generation: u64,
        secret_storage: &SaveIdentitySecretStorage,
    ) -> crate::ImResult<crate::internal::secret_vault::record::SecretRef> {
        let SaveIdentitySecretStorage::Vault {
            workspace_id,
            device_id,
            vault,
        } = secret_storage
        else {
            return Err(crate::ImError::LocalStateUnavailable {
                detail: "vNext device refresh tokens require Vault storage".to_owned(),
            });
        };
        let alias = sanitize_identity_name(local_alias);
        let _lock = self.lock_index_mutation()?;
        let index = self.load_index()?;
        let entry =
            index
                .credentials
                .get(&alias)
                .ok_or_else(|| crate::ImError::IdentityNotFound {
                    selector: alias.clone(),
                })?;
        if entry
            .root_key_import
            .as_ref()
            .map(|record| record.message_id())
            != Some(completed_message_id)
        {
            return Err(crate::ImError::PermissionDenied);
        }
        let state = entry
            .device_state
            .as_ref()
            .ok_or(crate::ImError::PermissionDenied)?;
        let authorization = state
            .authorization
            .as_ref()
            .ok_or(crate::ImError::PermissionDenied)?;
        if state.mode != crate::internal::identity_device_state::IdentityDeviceMode::VNext
            || authorization.status
                != crate::internal::identity_device_state::DeviceAuthorizationStatus::Active
            || authorization.role
                != crate::internal::identity_device_state::DeviceAuthorizationRole::Admin
            || !authorization.management_ready
            || authorization.auth_generation != auth_generation
        {
            return Err(crate::ImError::PermissionDenied);
        }
        let metadata = entry
            .vault_migration
            .as_ref()
            .filter(|metadata| {
                metadata.workspace_id == *workspace_id
                    && metadata.device_id == *device_id
                    && metadata.status == IdentityVaultMigrationStatus::Verified
            })
            .ok_or(crate::ImError::PermissionDenied)?;
        let auth_ref = metadata
            .vnext_refs
            .as_ref()
            .map(|refs| refs.auth_jwt.clone())
            .filter(|auth_ref| {
                metadata.refs.auth_jwt == *auth_ref
                    && auth_ref.workspace_id == *workspace_id
                    && auth_ref.device_id == *device_id
                    && auth_ref.identity_id.as_deref() == Some(entry.unique_id.as_str())
                    && auth_ref.did.as_deref() == Some(entry.did.as_str())
                    && auth_ref.kind == crate::internal::secret_vault::record::SecretKind::AuthJwt
                    && auth_ref.key_id == AUTH_FILE_NAME
                    && auth_ref.key_version > 0
            })
            .ok_or(crate::ImError::PermissionDenied)?;
        let opened = vault.open(&auth_ref)?;
        let snapshot = crate::internal::auth::state::parse_auth_state(opened.expose_secret())?;
        if !snapshot.has_valid_token || snapshot.refresh_token.is_none() {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(auth_ref)
    }

    /// Seals the replacement token under a new Vault `SecretRef`, then commits
    /// that ref, authorization generation and checkpoint in one atomic index
    /// image. A crash before the index rename leaves the old ref/state active;
    /// a crash after it leaves the complete new ref/state active.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn converge_root_import_management_ready(
        &self,
        local_alias: &str,
        completed_message_id: &str,
        auth_generation: u64,
        checkpoint: &crate::internal::identity_device_state::IdentityInternalCheckpoint,
        access_token: &str,
        refresh_token: &str,
        token_expires_at: &str,
        secret_storage: &SaveIdentitySecretStorage,
    ) -> crate::ImResult<crate::internal::secret_vault::record::SecretRef> {
        if completed_message_id.trim().is_empty() || auth_generation == 0 {
            return Err(crate::ImError::PermissionDenied);
        }
        let alias = sanitize_identity_name(local_alias);
        let lock = self.lock_index_mutation()?;
        let mut index = self.load_index()?;
        let SaveIdentitySecretStorage::Vault {
            workspace_id,
            device_id,
            vault,
        } = secret_storage
        else {
            return Err(crate::ImError::LocalStateUnavailable {
                detail: "vNext device refresh tokens require Vault storage".to_owned(),
            });
        };
        let entry =
            index
                .credentials
                .get_mut(&alias)
                .ok_or_else(|| crate::ImError::IdentityNotFound {
                    selector: alias.clone(),
                })?;
        if entry
            .root_key_import
            .as_ref()
            .map(|record| record.message_id())
            != Some(completed_message_id)
        {
            return Err(crate::ImError::PermissionDenied);
        }
        let state = entry
            .device_state
            .as_mut()
            .ok_or(crate::ImError::PermissionDenied)?;
        let authorization = state
            .authorization
            .as_mut()
            .ok_or(crate::ImError::PermissionDenied)?;
        if state.mode != crate::internal::identity_device_state::IdentityDeviceMode::VNext
            || authorization.status
                != crate::internal::identity_device_state::DeviceAuthorizationStatus::Active
            || authorization.role
                != crate::internal::identity_device_state::DeviceAuthorizationRole::Admin
            || authorization.auth_generation > auth_generation
        {
            return Err(crate::ImError::PermissionDenied);
        }
        if let Some(current) = state.checkpoint.as_ref() {
            if checkpoint.document_version < current.document_version
                || checkpoint.registry_version < current.registry_version
                || (checkpoint.document_version == current.document_version
                    && checkpoint.document_hash != current.document_hash)
            {
                return Err(crate::ImError::PermissionDenied);
            }
        }
        let already_ready = authorization.management_ready;
        if already_ready && authorization.auth_generation != auth_generation {
            return Err(crate::ImError::PermissionDenied);
        }
        if !already_ready && authorization.auth_generation >= auth_generation {
            return Err(crate::ImError::PermissionDenied);
        }

        let metadata = entry
            .vault_migration
            .as_mut()
            .filter(|metadata| {
                metadata.workspace_id == *workspace_id
                    && metadata.device_id == *device_id
                    && metadata.status == IdentityVaultMigrationStatus::Verified
            })
            .ok_or(crate::ImError::PermissionDenied)?;
        let refs = metadata
            .vnext_refs
            .as_mut()
            .ok_or(crate::ImError::PermissionDenied)?;
        let current_auth_ref = refs.auth_jwt.clone();
        if metadata.refs.auth_jwt != current_auth_ref
            || current_auth_ref.workspace_id != *workspace_id
            || current_auth_ref.device_id != *device_id
            || current_auth_ref.identity_id.as_deref() != Some(entry.unique_id.as_str())
            || current_auth_ref.did.as_deref() != Some(entry.did.as_str())
            || current_auth_ref.kind != crate::internal::secret_vault::record::SecretKind::AuthJwt
            || current_auth_ref.key_id != AUTH_FILE_NAME
            || current_auth_ref.key_version == 0
        {
            return Err(crate::ImError::PermissionDenied);
        }
        if already_ready {
            return Ok(current_auth_ref);
        }
        // Prove that the currently committed ref is readable before staging a
        // successor. The plaintext is held only by zeroizing SecretBytes.
        let current_auth = vault.open(&current_auth_ref)?;
        drop(current_auth);
        let next_key_version = current_auth_ref
            .key_version
            .checked_add(1)
            .ok_or(crate::ImError::PermissionDenied)?;
        let raw = crate::internal::auth::state::auth_state_json_for_token_pair(
            access_token,
            Some(refresh_token),
            Some(token_expires_at),
        )?;
        let staged_auth_ref = vault.seal(crate::internal::secret_vault::SealSecretRequest {
            metadata: crate::internal::secret_vault::record::SecretMetadata {
                workspace_id: current_auth_ref.workspace_id.clone(),
                device_id: current_auth_ref.device_id.clone(),
                identity_id: current_auth_ref.identity_id.clone(),
                did: current_auth_ref.did.clone(),
                kind: current_auth_ref.kind.clone(),
                key_id: current_auth_ref.key_id.clone(),
                key_version: next_key_version,
                policy: crate::internal::secret_vault::policy::SecretAccessPolicy::no_prompt_local_secret(),
            },
            plaintext: crate::internal::platform_secret::SecretBytes::from_vec(raw),
        })?;
        let opened = match vault.open(&staged_auth_ref) {
            Ok(opened) => opened,
            Err(error) => {
                let _ = vault.delete(&staged_auth_ref);
                return Err(error);
            }
        };
        let snapshot = match crate::internal::auth::state::parse_auth_state(opened.expose_secret())
        {
            Ok(snapshot) => snapshot,
            Err(error) => {
                let _ = vault.delete(&staged_auth_ref);
                return Err(error);
            }
        };
        if snapshot.bearer_token.as_deref() != Some(access_token.trim())
            || snapshot.refresh_token.as_deref() != Some(refresh_token.trim())
        {
            let _ = vault.delete(&staged_auth_ref);
            return Err(crate::ImError::PermissionDenied);
        }
        drop(snapshot);
        drop(opened);

        refs.auth_jwt = staged_auth_ref.clone();
        metadata.refs.auth_jwt = staged_auth_ref.clone();
        authorization.management_ready = true;
        authorization.auth_generation = auth_generation;
        state.checkpoint = Some(checkpoint.clone());
        index.schema_version = INDEX_SCHEMA_VERSION;
        if let Err(error) = self.save_index_locked(&lock, index) {
            let _ = vault.delete(&staged_auth_ref);
            return Err(error);
        }
        // Keep the superseded encrypted record until live providers have
        // advanced to the committed ref. It is no longer authoritative and a
        // later Vault compaction may remove it safely.
        Ok(staged_auth_ref)
    }

    pub(crate) async fn persist_vnext_auth_token_pair_async(
        paths: crate::paths::IdentityRegistryPaths,
        local_alias: String,
        access_token: String,
        refresh_token: String,
        token_expires_at: String,
        secret_storage: SaveIdentitySecretStorage,
    ) -> crate::ImResult<()> {
        crate::internal::runtime::worker::run_blocking(move || {
            IdentityStore::new(&paths).persist_vnext_auth_token_pair(
                &local_alias,
                &access_token,
                &refresh_token,
                &token_expires_at,
                &secret_storage,
            )
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
                    && metadata.device_id == context.vault_context_device_id().as_str()
                {
                    let did_document = self.load_did_document(identity_dir_name)?;
                    return self.load_daemon_subkey_package_from_vault(
                        identity_dir_name,
                        did,
                        &did_document,
                        metadata,
                        context.workspace_id(),
                        context.vault_context_device_id().as_str(),
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
        let lock = self.lock_index_mutation()?;
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
        self.save_index_locked(&lock, index)?;
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

        let lock = self.lock_index_mutation()?;
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
            vnext_refs: None,
        };
        let index_entry = index.credentials.get_mut(local_alias).ok_or_else(|| {
            crate::ImError::IdentityNotFound {
                selector: local_alias.to_string(),
            }
        })?;
        index_entry.vault_migration = Some(metadata.clone());
        self.save_index_locked(&lock, index)?;

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
        let lock = self.lock_index_mutation()?;
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
        self.save_index_locked(&lock, index)?;
        if default_updated {
            self.write_default_identity(final_identity_name)?;
        }
        Ok(RecoverPromotionResult { default_updated })
    }

    pub(crate) fn lock_index_mutation(&self) -> crate::ImResult<IdentityIndexMutationLock> {
        let parent =
            self.paths
                .registry_path
                .parent()
                .ok_or_else(|| crate::ImError::PathUnavailable {
                    path_kind: "identity_registry".to_owned(),
                    detail: "identity registry path has no parent directory".to_owned(),
                })?;
        fs::create_dir_all(parent)?;
        set_private_dir_mode(parent)?;
        let path = parent.join(IDENTITY_INDEX_MUTATION_LOCK_FILE);
        let file = open_private_lock_file(&path)?;
        set_private_file_mode(&path)?;
        file.lock_exclusive().map_err(crate::ImError::from)?;
        Ok(IdentityIndexMutationLock {
            file,
            registry_path: self.paths.registry_path.clone(),
        })
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

    pub(crate) fn save_index_locked(
        &self,
        lock: &IdentityIndexMutationLock,
        index: IndexPayload,
    ) -> crate::ImResult<()> {
        if lock.registry_path != self.paths.registry_path {
            return Err(crate::ImError::PermissionDenied);
        }
        if let Some(parent) = self.paths.registry_path.parent() {
            fs::create_dir_all(parent)?;
            set_private_dir_mode(parent)?;
        }
        let index = normalize_index_payload(index)?;
        let raw =
            serde_json::to_vec_pretty(&index).map_err(|err| crate::ImError::Serialization {
                detail: err.to_string(),
            })?;
        write_secure_bytes_atomic(&self.paths.registry_path, &raw)
    }

    pub(crate) fn save_device_state(
        &self,
        local_alias: &str,
        state: crate::internal::identity_device_state::IdentityDeviceState,
    ) -> crate::ImResult<()> {
        let local_alias = local_alias.trim();
        if local_alias.is_empty() {
            return Err(crate::ImError::invalid_input(
                Some("local_alias".to_owned()),
                "local alias is required",
            ));
        }
        let lock = self.lock_index_mutation()?;
        let mut index = self.load_index()?;
        let entry = index.credentials.get_mut(local_alias).ok_or_else(|| {
            crate::ImError::IdentityNotFound {
                selector: local_alias.to_owned(),
            }
        })?;
        let did = crate::ids::Did::parse(&entry.did)?;
        state.validate_for_did(&did)?;
        entry.device_state = Some(state);
        index.schema_version = INDEX_SCHEMA_VERSION;
        self.save_index_locked(&lock, index)
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
        let _lock = self.lock_index_mutation()?;
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
            let raw = serde_json::to_vec_pretty(&registry).map_err(|error| {
                crate::ImError::Serialization {
                    detail: error.to_string(),
                }
            })?;
            write_secure_bytes_atomic(&self.paths.registry_path, &raw)?;
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) device_state: Option<crate::internal::identity_device_state::IdentityDeviceState>,
    /// Non-secret, AWiki-local replay record for the one imported root ACK.
    /// The root private key itself remains only in SecretVault.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) root_key_import:
        Option<crate::internal::identity_root_transfer::PersistedRootKeyImport>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) vnext_refs: Option<crate::internal::key_provider::vault::VNextVaultKeyMaterialRefs>,
}

impl IdentityVaultMigrationMetadata {
    /// Maps schema-v1 migration metadata through the explicit legacy key-1
    /// compatibility provider. vNext identities use separate refs.
    pub(crate) fn legacy_key_material_refs(
        &self,
    ) -> crate::internal::key_provider::LegacyVaultKeyMaterialRefs {
        crate::internal::key_provider::LegacyVaultKeyMaterialRefs {
            default_signing_private: self.refs.default_signing_private.clone(),
            e2ee_agreement_private: self.refs.e2ee_agreement_private.clone(),
            auth_jwt: self.refs.auth_jwt.clone(),
        }
    }

    pub(crate) fn vnext_key_material_refs(
        &self,
    ) -> Option<crate::internal::key_provider::vault::VNextVaultKeyMaterialRefs> {
        self.vnext_refs.clone()
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
    if !matches!(payload.schema_version, 0 | 2 | 3 | 4 | INDEX_SCHEMA_VERSION) {
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

struct PreservedImportedRoot {
    import: crate::internal::identity_root_transfer::PersistedRootKeyImport,
    metadata: IdentityVaultMigrationMetadata,
}

/// Root import is a one-way security transition. Ordinary identity save paths
/// may refresh the same rootless vNext identity, but must not silently move,
/// replace, or downgrade its imported root state.
fn preserve_imported_root_for_resave(
    index: &IndexPayload,
    local_alias: &str,
    input: &SaveIdentityInput,
    storage: &SaveIdentitySecretStorage,
) -> crate::ImResult<Option<PreservedImportedRoot>> {
    for (name, entry) in &index.credentials {
        if name != local_alias
            && entry.root_key_import.is_some()
            && (entry.did == input.did.as_str() || entry.unique_id == input.unique_id)
        {
            return Err(crate::ImError::PermissionDenied);
        }
    }

    let Some(entry) = index
        .credentials
        .get(local_alias)
        .filter(|entry| entry.root_key_import.is_some())
    else {
        return Ok(None);
    };
    if input.local_alias.trim() != local_alias
        || entry.did != input.did.as_str()
        || entry.unique_id != input.unique_id
        || entry.dir_name != preferred_dir_name(&input.unique_id)?
        || !input.key1_private_pem.trim().is_empty()
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let SaveIdentitySecretStorage::Vault {
        workspace_id,
        device_id,
        vault,
    } = storage
    else {
        return Err(crate::ImError::PermissionDenied);
    };
    let metadata = entry
        .vault_migration
        .as_ref()
        .filter(|metadata| {
            metadata.status == IdentityVaultMigrationStatus::Verified
                && metadata.backend == "vault"
                && metadata.workspace_id == *workspace_id
                && metadata.device_id == *device_id
        })
        .ok_or(crate::ImError::PermissionDenied)?;
    let refs = metadata
        .vnext_refs
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    let root_ref = refs
        .did_document_root_private
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    let SaveIdentityKeyMode::VNext {
        root_key_id,
        device_signing_key_id,
        device_e2ee_key_id,
    } = &input.key_mode
    else {
        return Err(crate::ImError::PermissionDenied);
    };
    if root_ref.workspace_id != *workspace_id
        || root_ref.device_id != *device_id
        || root_ref.identity_id.as_deref() != Some(input.unique_id.as_str())
        || root_ref.did.as_deref() != Some(input.did.as_str())
        || root_ref.kind != crate::internal::secret_vault::record::SecretKind::IdentityRootPrivate
        || root_ref.key_id != *root_key_id
        || root_ref.key_version != 1
        || refs.device_request_signing_private.key_id != *device_signing_key_id
        || refs.e2ee_agreement_private.key_id != *device_e2ee_key_id
    {
        return Err(crate::ImError::PermissionDenied);
    }
    if metadata.refs.default_signing_private != refs.device_request_signing_private
        || metadata.refs.e2ee_signing_private.as_ref() != Some(&refs.device_request_signing_private)
        || metadata.refs.e2ee_agreement_private != refs.e2ee_agreement_private
        || metadata.refs.auth_jwt != refs.auth_jwt
        || refs.auth_jwt.workspace_id != *workspace_id
        || refs.auth_jwt.device_id != *device_id
        || refs.auth_jwt.identity_id.as_deref() != Some(input.unique_id.as_str())
        || refs.auth_jwt.did.as_deref() != Some(input.did.as_str())
        || refs.auth_jwt.kind != crate::internal::secret_vault::record::SecretKind::AuthJwt
        || refs.auth_jwt.key_id != AUTH_FILE_NAME
        || refs.auth_jwt.key_version == 0
    {
        return Err(crate::ImError::PermissionDenied);
    }

    for (secret_ref, kind, expected) in [
        (
            &refs.device_request_signing_private,
            crate::internal::secret_vault::record::SecretKind::IdentityDeviceSigningPrivate,
            input.e2ee_signing_private_pem.as_bytes(),
        ),
        (
            &refs.e2ee_agreement_private,
            crate::internal::secret_vault::record::SecretKind::IdentityE2eeAgreementPrivate,
            input.e2ee_agreement_private_pem.as_bytes(),
        ),
    ] {
        if secret_ref.workspace_id != *workspace_id
            || secret_ref.device_id != *device_id
            || secret_ref.identity_id.as_deref() != Some(input.unique_id.as_str())
            || secret_ref.did.as_deref() != Some(input.did.as_str())
            || secret_ref.kind != kind
            || secret_ref.key_version != 1
        {
            return Err(crate::ImError::PermissionDenied);
        }
        let opened = vault.open(secret_ref)?;
        if opened.expose_secret() != expected {
            return Err(crate::ImError::PermissionDenied);
        }
    }
    let root_secret = vault.open(root_ref)?;
    let root_private = std::str::from_utf8(root_secret.expose_secret())
        .map_err(|_| crate::ImError::PermissionDenied)?;
    let root_material = anp::PrivateKeyMaterial::from_pem(root_private)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    if !matches!(root_material, anp::PrivateKeyMaterial::Ed25519(_))
        || root_material.public_key().to_pem() != input.key1_public_pem
    {
        return Err(crate::ImError::PermissionDenied);
    }
    drop(root_material);
    drop(root_secret);
    let expected_auth = Zeroizing::new(crate::internal::auth::state::auth_state_json_for_token(
        &input.jwt_token,
    )?);
    let opened_auth = vault.open(&refs.auth_jwt)?;
    if opened_auth.expose_secret() != expected_auth.as_slice() {
        return Err(crate::ImError::PermissionDenied);
    }
    match (
        metadata.refs.daemon_subkey_private.as_ref(),
        input.daemon_subkey_package.as_ref(),
    ) {
        (None, None) => {}
        (Some(secret_ref), Some(package)) => {
            let opened = vault.open(secret_ref)?;
            if opened.expose_secret() != package.private_key_material().as_bytes() {
                return Err(crate::ImError::PermissionDenied);
            }
        }
        _ => return Err(crate::ImError::PermissionDenied),
    }
    if let Some(incoming_state) = input.device_state.as_ref() {
        let existing_authorization = entry
            .device_state
            .as_ref()
            .and_then(|state| state.authorization.as_ref())
            .ok_or(crate::ImError::PermissionDenied)?;
        let incoming_authorization = incoming_state
            .authorization
            .as_ref()
            .ok_or(crate::ImError::PermissionDenied)?;
        if incoming_state.mode != crate::internal::identity_device_state::IdentityDeviceMode::VNext
            || incoming_authorization.protocol_device_id
                != existing_authorization.protocol_device_id
            || incoming_authorization.signing_key_id != existing_authorization.signing_key_id
            || incoming_authorization.e2ee_key_id != existing_authorization.e2ee_key_id
            || incoming_authorization.signing_key_id != *device_signing_key_id
            || incoming_authorization.e2ee_key_id != *device_e2ee_key_id
        {
            return Err(crate::ImError::PermissionDenied);
        }
        if let Some(document) = input.did_document.as_ref() {
            if !anp::authentication::validate_did_document_binding(document, true) {
                return Err(crate::ImError::PermissionDenied);
            }
            let manifest_device = anp::authentication::find_eligible_device(
                document,
                incoming_authorization.protocol_device_id.as_str(),
                anp::authentication::PROFILE_DIRECT_E2EE_V2,
            )
            .map_err(|_| crate::ImError::PermissionDenied)?
            .ok_or(crate::ImError::PermissionDenied)?;
            if manifest_device.signing_key_id != incoming_authorization.signing_key_id
                || manifest_device.e2ee_key_id != incoming_authorization.e2ee_key_id
            {
                return Err(crate::ImError::PermissionDenied);
            }
        }
    }

    Ok(Some(PreservedImportedRoot {
        import: entry
            .root_key_import
            .clone()
            .ok_or(crate::ImError::PermissionDenied)?,
        metadata: metadata.clone(),
    }))
}

fn local_identity_dir(root: &Path, dir_name: &str) -> crate::ImResult<std::path::PathBuf> {
    let relative = Path::new(dir_name);
    let mut components = relative.components();
    let is_single_normal_segment =
        matches!(components.next(), Some(std::path::Component::Normal(_)))
            && components.next().is_none();
    if dir_name.trim().is_empty() || relative.is_absolute() || !is_single_normal_segment {
        return Err(crate::ImError::invalid_input(
            Some("identity".to_string()),
            "local identity directory name must be a simple relative path segment",
        ));
    }
    let path = root.join(relative);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            Err(crate::ImError::PermissionDenied)
        }
        Ok(_) => Ok(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(path),
        Err(error) => Err(crate::ImError::CredentialFileUnreadable {
            path_kind: "identity_directory".to_owned(),
            detail: error.to_string(),
        }),
    }
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

fn validate_root_import_token_operation(value: &str) -> crate::ImResult<()> {
    if value.is_empty()
        || value != value.trim()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

fn write_secure_json(path: &Path, payload: &impl Serialize) -> crate::ImResult<()> {
    let raw = serde_json::to_vec_pretty(payload).map_err(|err| crate::ImError::Serialization {
        detail: err.to_string(),
    })?;
    fs::write(path, raw)?;
    set_private_file_mode(path)?;
    Ok(())
}

/// Atomically replaces a security-sensitive metadata file.
///
/// Root import commits a Vault reference and its non-secret replay record in
/// one index image. A crash may leave the previous complete image or the new
/// complete image, but never a truncated JSON registry.
fn write_secure_bytes_atomic(path: &Path, raw: &[u8]) -> crate::ImResult<()> {
    use std::io::Write as _;

    let parent = path
        .parent()
        .ok_or_else(|| crate::ImError::PathUnavailable {
            path_kind: "identity_registry".to_owned(),
            detail: "identity registry path has no parent directory".to_owned(),
        })?;
    fs::create_dir_all(parent)?;
    set_private_dir_mode(parent)?;
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("identity-index.json");
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let temporary = parent.join(format!(".{file_name}.{}.{}.tmp", std::process::id(), nonce));
    let write_result = (|| -> crate::ImResult<()> {
        let mut file = create_private_file(&temporary)?;
        file.write_all(raw)?;
        file.sync_all()?;
        crate::internal::atomic_file::replace(&temporary, path)?;
        set_private_file_mode(path)?;
        if let Ok(directory) = fs::File::open(parent) {
            let _ = directory.sync_all();
        }
        Ok(())
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    write_result
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

    let (root_key_id, device_signing_key_id, device_e2ee_key_id, is_vnext) = match &input.key_mode {
        SaveIdentityKeyMode::LegacyKey1 => (
            "key-1".to_owned(),
            "key-2".to_owned(),
            "key-3".to_owned(),
            false,
        ),
        SaveIdentityKeyMode::VNext {
            root_key_id,
            device_signing_key_id,
            device_e2ee_key_id,
        } => {
            for (field, value) in [
                ("root_key_id", root_key_id),
                ("device_signing_key_id", device_signing_key_id),
                ("device_e2ee_key_id", device_e2ee_key_id),
            ] {
                if value.trim().is_empty() {
                    return Err(crate::ImError::invalid_input(
                        Some(format!("identity_key_mode.{field}")),
                        format!("{field} is required for vNext identity storage"),
                    ));
                }
            }
            if input.e2ee_signing_private_pem.trim().is_empty() {
                return Err(crate::ImError::invalid_input(
                    Some("device_signing_private_pem".to_owned()),
                    "vNext identity storage requires a device signing private key",
                ));
            }
            (
                root_key_id.clone(),
                device_signing_key_id.clone(),
                device_e2ee_key_id.clone(),
                true,
            )
        }
    };
    let did_document_root_private = if input.key1_private_pem.trim().is_empty() {
        if is_vnext {
            None
        } else {
            return Err(crate::ImError::invalid_input(
                Some("key1_private_pem".to_owned()),
                "legacy identity storage requires a signing private key",
            ));
        }
    } else {
        Some(seal_utf8_secret(
            vault,
            vault_secret_metadata(
                workspace_id,
                device_id,
                identity_id,
                did,
                crate::internal::secret_vault::record::SecretKind::IdentityRootPrivate,
                &root_key_id,
            ),
            &input.key1_private_pem,
        )?)
    };
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
                if is_vnext {
                    crate::internal::secret_vault::record::SecretKind::IdentityDeviceSigningPrivate
                } else {
                    crate::internal::secret_vault::record::SecretKind::IdentityE2eeSigningPrivate
                },
                &device_signing_key_id,
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
            &device_e2ee_key_id,
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

    if let Some(secret_ref) = &did_document_root_private {
        verify_vault_utf8_secret(vault, secret_ref, &input.key1_private_pem)?;
    }
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

    let vnext_refs = if is_vnext {
        let device_request_signing_private =
            e2ee_signing_private
                .clone()
                .ok_or_else(|| crate::ImError::IdentityNotReady {
                    identity: did.to_owned(),
                    missing: vec!["vNext device signing key was not sealed".to_owned()],
                })?;
        Some(
            crate::internal::key_provider::vault::VNextVaultKeyMaterialRefs {
                device_request_signing_private,
                did_document_root_private: did_document_root_private.clone(),
                e2ee_agreement_private: e2ee_agreement_private.clone(),
                auth_jwt: auth_jwt.clone(),
            },
        )
    } else {
        None
    };
    // `refs` is the schema-v1 legacy compatibility projection. vNext runtime
    // never reads it: `vnext_refs` is authoritative and the registry rejects
    // vNext refs without vNext device state. A rootless member therefore uses
    // its device-signing ref only as the required compatibility anchor here.
    let legacy_default_signing_private = did_document_root_private
        .clone()
        .or_else(|| e2ee_signing_private.clone())
        .ok_or_else(|| crate::ImError::IdentityNotReady {
            identity: did.to_owned(),
            missing: vec!["identity_signing_key".to_owned()],
        })?;
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
            default_signing_private: legacy_default_signing_private,
            e2ee_signing_private,
            e2ee_agreement_private,
            daemon_subkey_private,
            auth_jwt,
        },
        vnext_refs,
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
fn open_private_lock_file(path: &Path) -> crate::ImResult<fs::File> {
    use std::os::unix::fs::OpenOptionsExt as _;
    fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .map_err(crate::ImError::from)
}

#[cfg(not(unix))]
fn open_private_lock_file(path: &Path) -> crate::ImResult<fs::File> {
    fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .map_err(crate::ImError::from)
}

#[cfg(unix)]
fn create_private_file(path: &Path) -> crate::ImResult<fs::File> {
    use std::os::unix::fs::OpenOptionsExt as _;
    fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .map_err(crate::ImError::from)
}

#[cfg(not(unix))]
fn create_private_file(path: &Path) -> crate::ImResult<fs::File> {
    fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(crate::ImError::from)
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
    fn identity_directory_helper_rejects_traversal_and_existing_non_directory() {
        let root = tempfile::tempdir().unwrap();
        let paths = test_paths(root.path());
        let store = IdentityStore::new(&paths);
        std::fs::create_dir_all(&paths.identity_root_dir).unwrap();
        std::fs::write(paths.identity_root_dir.join("not-a-directory"), b"sentinel").unwrap();

        for invalid in ["", "../outside", "nested/child", "not-a-directory"] {
            assert!(store.local_identity_dir(invalid).is_err());
        }
    }

    #[cfg(unix)]
    #[test]
    fn identity_directory_helper_rejects_existing_symlink() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let paths = test_paths(root.path());
        let store = IdentityStore::new(&paths);
        let outside = root.path().join("outside");
        std::fs::create_dir_all(&paths.identity_root_dir).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        symlink(&outside, paths.identity_root_dir.join("linked")).unwrap();

        assert_eq!(
            store.local_identity_dir("linked").unwrap_err(),
            crate::ImError::PermissionDenied
        );
    }

    #[test]
    fn schema_three_identity_remains_legacy_until_explicit_device_state_write() {
        let index = parse_index_payload(
            br#"{
              "schema_version": 3,
              "default_credential_name": "alice",
              "credentials": {
                "alice": {
                  "credential_name": "alice",
                  "dir_name": "alice-id",
                  "did": "did:example:alice",
                  "unique_id": "alice-id"
                }
              }
            }"#,
        )
        .unwrap();

        assert_eq!(index.schema_version, 3);
        assert!(index.credentials["alice"].device_state.is_none());
    }

    #[test]
    fn save_device_state_is_explicit_validated_and_repeat_safe() {
        use crate::internal::identity_device_state::{
            DeviceAuthorizationProjection, DeviceAuthorizationRole, DeviceAuthorizationStatus,
            IdentityDeviceMode, IdentityDeviceState, IdentityInternalCheckpoint,
            IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
        };

        let root = tempfile::tempdir().unwrap();
        let paths = test_paths(root.path());
        let store = IdentityStore::new(&paths);
        let did = "did:wba:awiki.info:user:alice:e1_root";
        store
            .save_identity(SaveIdentityInput {
                local_alias: "alice".to_owned(),
                did: crate::ids::Did::parse(did).unwrap(),
                unique_id: "alice-id".to_owned(),
                user_id: "user-1".to_owned(),
                display_name: "Alice".to_owned(),
                handle: "alice".to_owned(),
                full_handle: "alice.awiki.info".to_owned(),
                jwt_token: "token".to_owned(),
                did_document: Some(json!({"id": did})),
                key_mode: crate::internal::identity_store::SaveIdentityKeyMode::LegacyKey1,
                device_state: None,
                key1_private_pem: "root-private".to_owned(),
                key1_public_pem: "root-public".to_owned(),
                e2ee_signing_private_pem: "device-signing-private".to_owned(),
                e2ee_agreement_private_pem: "device-e2ee-private".to_owned(),
                daemon_subkey_package: None,
                make_default: true,
            })
            .unwrap();
        let state = IdentityDeviceState {
            schema_version: IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
            mode: IdentityDeviceMode::VNext,
            authorization: Some(DeviceAuthorizationProjection {
                protocol_device_id: crate::ids::ProtocolDeviceId::parse("dev-device-a").unwrap(),
                signing_key_id: format!("{did}#device-sign"),
                e2ee_key_id: format!("{did}#device-e2ee"),
                status: DeviceAuthorizationStatus::Active,
                role: DeviceAuthorizationRole::Member,
                management_ready: false,
                auth_generation: 1,
            }),
            checkpoint: Some(IdentityInternalCheckpoint {
                document_version: 1,
                document_hash: "sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_owned(),
                registry_version: 1,
            }),
        };

        store.save_device_state("alice", state.clone()).unwrap();
        store.save_device_state("alice", state.clone()).unwrap();

        let index = store.load_index().unwrap();
        assert_eq!(index.schema_version, INDEX_SCHEMA_VERSION);
        assert_eq!(index.credentials["alice"].device_state, Some(state));
        assert!(!std::fs::read_to_string(&paths.registry_path)
            .unwrap()
            .contains("root-private"));
    }

    #[test]
    fn identity_index_mutations_serialize_load_modify_save_without_lost_updates() {
        use crate::internal::identity_device_state::{
            DeviceAuthorizationProjection, DeviceAuthorizationRole, DeviceAuthorizationStatus,
            IdentityDeviceMode, IdentityDeviceState, IdentityInternalCheckpoint,
            IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
        };
        use std::sync::mpsc;
        use std::time::Duration as StdDuration;

        let root = tempfile::tempdir().unwrap();
        let paths = test_paths(root.path());
        let store = IdentityStore::new(&paths);
        let did = "did:example:alice";
        let initial_lock = store.lock_index_mutation().unwrap();
        let mut initial = IndexPayload::default();
        initial.credentials.insert(
            "alice".to_owned(),
            IndexEntry {
                credential_name: "alice".to_owned(),
                dir_name: "alice-id".to_owned(),
                did: did.to_owned(),
                unique_id: "alice-id".to_owned(),
                name: "before".to_owned(),
                ..IndexEntry::default()
            },
        );
        store.save_index_locked(&initial_lock, initial).unwrap();
        drop(initial_lock);

        let state = IdentityDeviceState {
            schema_version: IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
            mode: IdentityDeviceMode::VNext,
            authorization: Some(DeviceAuthorizationProjection {
                protocol_device_id: crate::ids::ProtocolDeviceId::parse("dev-device-a").unwrap(),
                signing_key_id: format!("{did}#device-sign"),
                e2ee_key_id: format!("{did}#device-e2ee"),
                status: DeviceAuthorizationStatus::Active,
                role: DeviceAuthorizationRole::Member,
                management_ready: false,
                auth_generation: 2,
            }),
            checkpoint: Some(IdentityInternalCheckpoint {
                document_version: 2,
                document_hash: "sha256:BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB".to_owned(),
                registry_version: 2,
            }),
        };

        let writer_a_lock = store.lock_index_mutation().unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (finished_tx, finished_rx) = mpsc::channel();
        let writer_paths = paths.clone();
        let expected_state = state.clone();
        let writer = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            let result = IdentityStore::new(&writer_paths).save_device_state("alice", state);
            finished_tx.send(result).unwrap();
        });
        started_rx.recv().unwrap();
        assert!(matches!(
            finished_rx.recv_timeout(StdDuration::from_millis(100)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));

        let mut writer_a_index = store.load_index().unwrap();
        writer_a_index.credentials.get_mut("alice").unwrap().name = "root-import-commit".to_owned();
        store
            .save_index_locked(&writer_a_lock, writer_a_index)
            .unwrap();
        drop(writer_a_lock);

        finished_rx
            .recv_timeout(StdDuration::from_secs(2))
            .unwrap()
            .unwrap();
        writer.join().unwrap();
        let committed = store.load_index().unwrap();
        assert_eq!(committed.credentials["alice"].name, "root-import-commit");
        assert_eq!(
            committed.credentials["alice"].device_state,
            Some(expected_state)
        );
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
                key_mode: crate::internal::identity_store::SaveIdentityKeyMode::LegacyKey1,
                device_state: None,
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
            result.metadata.legacy_key_material_refs(),
        );
        assert_eq!(
            provider.device_request_signing_private_pem().unwrap(),
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
                key_mode: crate::internal::identity_store::SaveIdentityKeyMode::LegacyKey1,
                device_state: None,
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
                    key_mode: crate::internal::identity_store::SaveIdentityKeyMode::LegacyKey1,
                    device_state: None,
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
    fn vnext_secure_save_persists_separate_role_refs_and_device_state() {
        use crate::internal::identity_device_state::{
            DeviceAuthorizationProjection, DeviceAuthorizationRole, DeviceAuthorizationStatus,
            IdentityDeviceMode, IdentityDeviceState, IdentityInternalCheckpoint,
            IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
        };
        use crate::internal::secret_vault::record::SecretKind;

        let root = tempfile::tempdir().unwrap();
        let paths = test_paths(root.path());
        let store = IdentityStore::new(&paths);
        let did = crate::ids::Did::parse("did:wba:awiki.info:alice:e1_root").unwrap();
        let root_key_id = format!("{}#key-1", did.as_str());
        let signing_key_id = format!("{}#dev-a-sign", did.as_str());
        let e2ee_key_id = format!("{}#dev-a-e2ee", did.as_str());
        let device_state = IdentityDeviceState {
            schema_version: IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
            mode: IdentityDeviceMode::VNext,
            authorization: Some(DeviceAuthorizationProjection {
                protocol_device_id: crate::ids::ProtocolDeviceId::parse("dev-a").unwrap(),
                signing_key_id: signing_key_id.clone(),
                e2ee_key_id: e2ee_key_id.clone(),
                status: DeviceAuthorizationStatus::Active,
                role: DeviceAuthorizationRole::Admin,
                management_ready: true,
                auth_generation: 1,
            }),
            checkpoint: Some(IdentityInternalCheckpoint {
                document_version: 1,
                document_hash: "sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_owned(),
                registry_version: 1,
            }),
        };
        let vault = Arc::new(FileSecretVault::new(
            DeviceVaultRootKey::from_bytes([17_u8; 32]),
            FileSecretVaultStore::new(root.path().join("vault")),
        ));

        store
            .save_identity_with_secret_storage(
                SaveIdentityInput {
                    local_alias: "alice".to_owned(),
                    did: did.clone(),
                    unique_id: "alice-vnext".to_owned(),
                    user_id: "user-1".to_owned(),
                    display_name: "Alice".to_owned(),
                    handle: "alice".to_owned(),
                    full_handle: "alice.awiki.info".to_owned(),
                    jwt_token: "device-token".to_owned(),
                    did_document: Some(json!({"id": did.as_str()})),
                    key_mode: SaveIdentityKeyMode::VNext {
                        root_key_id: root_key_id.clone(),
                        device_signing_key_id: signing_key_id.clone(),
                        device_e2ee_key_id: e2ee_key_id.clone(),
                    },
                    device_state: Some(device_state.clone()),
                    key1_private_pem: "root-private-secret".to_owned(),
                    key1_public_pem: "root-public".to_owned(),
                    e2ee_signing_private_pem: "device-signing-private-secret".to_owned(),
                    e2ee_agreement_private_pem: "device-e2ee-private-secret".to_owned(),
                    daemon_subkey_package: None,
                    make_default: true,
                },
                SaveIdentitySecretStorage::Vault {
                    workspace_id: "workspace-a".to_owned(),
                    device_id: "vault-context-a".to_owned(),
                    vault: vault.clone(),
                },
            )
            .unwrap();

        let index = store.load_index().unwrap();
        let entry = &index.credentials["alice"];
        assert_eq!(entry.device_state, Some(device_state));
        let metadata = entry.vault_migration.as_ref().unwrap();
        let refs = metadata.vnext_refs.as_ref().unwrap();
        assert_eq!(
            refs.device_request_signing_private.kind,
            SecretKind::IdentityDeviceSigningPrivate
        );
        assert_eq!(
            refs.did_document_root_private.as_ref().unwrap().kind,
            SecretKind::IdentityRootPrivate
        );
        assert_eq!(refs.device_request_signing_private.key_id, signing_key_id);
        assert_eq!(
            refs.did_document_root_private.as_ref().unwrap().key_id,
            root_key_id
        );
        assert_eq!(refs.e2ee_agreement_private.key_id, e2ee_key_id);
        assert_eq!(
            open_utf8(vault.as_ref(), &refs.device_request_signing_private),
            "device-signing-private-secret"
        );
        assert_eq!(
            open_utf8(
                vault.as_ref(),
                refs.did_document_root_private.as_ref().unwrap()
            ),
            "root-private-secret"
        );
        assert!(!paths
            .identity_root_dir
            .join("alice-vnext")
            .join(KEY1_PRIVATE_FILE_NAME)
            .exists());
    }

    #[test]
    fn vnext_member_secure_save_omits_did_root_private() {
        use crate::internal::identity_device_state::{
            DeviceAuthorizationProjection, DeviceAuthorizationRole, DeviceAuthorizationStatus,
            IdentityDeviceMode, IdentityDeviceState, IdentityInternalCheckpoint,
            IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
        };
        use crate::internal::key_provider::KeyMaterialProvider;

        let root = tempfile::tempdir().unwrap();
        let paths = test_paths(root.path());
        let store = IdentityStore::new(&paths);
        let did = crate::ids::Did::parse("did:wba:awiki.info:alice:e1_root").unwrap();
        let signing_key_id = format!("{}#dev-member-sign", did.as_str());
        let e2ee_key_id = format!("{}#dev-member-e2ee", did.as_str());
        let vault = Arc::new(FileSecretVault::new(
            DeviceVaultRootKey::from_bytes([18_u8; 32]),
            FileSecretVaultStore::new(root.path().join("vault")),
        ));

        store
            .save_identity_with_secret_storage(
                SaveIdentityInput {
                    local_alias: "alice-member".to_owned(),
                    did: did.clone(),
                    unique_id: "alice-member-vnext".to_owned(),
                    user_id: "user-1".to_owned(),
                    display_name: "Alice member".to_owned(),
                    handle: "alice".to_owned(),
                    full_handle: "alice.awiki.info".to_owned(),
                    jwt_token: "device-token".to_owned(),
                    did_document: Some(json!({"id": did.as_str()})),
                    key_mode: SaveIdentityKeyMode::VNext {
                        root_key_id: format!("{}#key-1", did.as_str()),
                        device_signing_key_id: signing_key_id.clone(),
                        device_e2ee_key_id: e2ee_key_id,
                    },
                    device_state: Some(IdentityDeviceState {
                        schema_version: IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
                        mode: IdentityDeviceMode::VNext,
                        authorization: Some(DeviceAuthorizationProjection {
                            protocol_device_id: crate::ids::ProtocolDeviceId::parse("dev-member")
                                .unwrap(),
                            signing_key_id,
                            e2ee_key_id: format!("{}#dev-member-e2ee", did.as_str()),
                            status: DeviceAuthorizationStatus::Active,
                            role: DeviceAuthorizationRole::Member,
                            management_ready: false,
                            auth_generation: 1,
                        }),
                        checkpoint: Some(IdentityInternalCheckpoint {
                            document_version: 2,
                            document_hash: "sha256:BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB"
                                .to_owned(),
                            registry_version: 2,
                        }),
                    }),
                    key1_private_pem: String::new(),
                    key1_public_pem: "root-public".to_owned(),
                    e2ee_signing_private_pem: "member-device-signing-private".to_owned(),
                    e2ee_agreement_private_pem: "member-device-e2ee-private".to_owned(),
                    daemon_subkey_package: None,
                    make_default: true,
                },
                SaveIdentitySecretStorage::Vault {
                    workspace_id: "workspace-a".to_owned(),
                    device_id: "vault-context-member".to_owned(),
                    vault: vault.clone(),
                },
            )
            .unwrap();

        let index = store.load_index().unwrap();
        let entry = &index.credentials["alice-member"];
        let refs = entry
            .vault_migration
            .as_ref()
            .and_then(IdentityVaultMigrationMetadata::vnext_key_material_refs)
            .expect("vNext refs");
        assert!(refs.did_document_root_private.is_none());
        let provider =
            crate::internal::key_provider::vault::VaultBackedKeyMaterialProvider::new_vnext(
                paths.identity_root_dir.join("alice-member-vnext"),
                vault,
                refs,
            );
        assert_eq!(
            provider.device_request_signing_private_pem().unwrap(),
            "member-device-signing-private"
        );
        assert!(provider.did_document_root_private_pem().is_err());
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
                    key_mode: crate::internal::identity_store::SaveIdentityKeyMode::LegacyKey1,
                    device_state: None,
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

        fn seal_if_absent(
            &self,
            request: crate::internal::secret_vault::SealSecretRequest,
        ) -> crate::ImResult<crate::internal::secret_vault::SealIfAbsentResult> {
            self.inner.seal_if_absent(request)
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

    #[test]
    fn recovered_identity_reuses_stable_id_after_removing_archived_alias() {
        let root = tempfile::tempdir().unwrap();
        let paths = test_paths(root.path());
        let store = IdentityStore::new(&paths);
        let input = |alias: &str, did: &str| SaveIdentityInput {
            local_alias: alias.to_owned(),
            did: crate::ids::Did::parse(did).unwrap(),
            unique_id: "stable-owner".to_owned(),
            user_id: "user-alice".to_owned(),
            display_name: "Alice".to_owned(),
            handle: "alice".to_owned(),
            full_handle: "alice.example".to_owned(),
            jwt_token: "jwt".to_owned(),
            did_document: Some(json!({"id": did})),
            key_mode: crate::internal::identity_store::SaveIdentityKeyMode::LegacyKey1,
            device_state: None,
            key1_private_pem: "private".to_owned(),
            key1_public_pem: "public".to_owned(),
            e2ee_signing_private_pem: "signing".to_owned(),
            e2ee_agreement_private_pem: "agreement".to_owned(),
            daemon_subkey_package: None,
            make_default: true,
        };
        store
            .save_identity(input("alice-old", "did:example:old"))
            .unwrap();
        let recovered = store
            .save_recovered_identity_with_secret_storage(
                input("alice-recovering", "did:example:new"),
                SaveIdentitySecretStorage::FileCompat,
                &["alice-old".to_owned()],
            )
            .unwrap();
        assert_eq!(recovered.unique_id, "stable-owner");
        let index = store.load_index().unwrap();
        assert!(!index.credentials.contains_key("alice-old"));
        assert_eq!(index.default_credential_name, "alice-recovering");
        assert_eq!(index.credentials["alice-recovering"].did, "did:example:new");
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
