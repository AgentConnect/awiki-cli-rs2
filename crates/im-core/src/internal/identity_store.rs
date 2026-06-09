use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
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

impl<'a> IdentityStore<'a> {
    pub(crate) fn new(paths: &'a crate::paths::IdentityRegistryPaths) -> Self {
        Self { paths }
    }

    pub(crate) fn save_identity(
        &self,
        mut input: SaveIdentityInput,
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
        fs::create_dir_all(&identity_dir)?;
        set_private_dir_mode(&identity_dir)?;
        let created_at = index
            .credentials
            .get(&local_alias)
            .map(|entry| entry.created_at.clone())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(now_rfc3339);

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
        write_secure_json(
            &identity_dir.join(AUTH_FILE_NAME),
            &json!({ "jwt_token": nullable_string(&input.jwt_token) }),
        )?;
        if let Some(document) = &input.did_document {
            write_secure_json(&identity_dir.join(DID_DOCUMENT_FILE_NAME), document)?;
        }
        write_secure_text_if_present(
            &identity_dir.join(KEY1_PRIVATE_FILE_NAME),
            &input.key1_private_pem,
        )?;
        write_secure_text_if_present(
            &identity_dir.join(KEY1_PUBLIC_FILE_NAME),
            &input.key1_public_pem,
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
            if package.user_did != input.did {
                return Err(crate::ImError::invalid_input(
                    Some("daemon_subkey_package.user_did".to_string()),
                    "daemon subkey package user_did must match identity did",
                ));
            }
            write_secure_text_if_present(
                &identity_dir.join(DAEMON_SUBKEY_PRIVATE_FILE_NAME),
                &package.private_key_multibase,
            )?;
            write_secure_json(&identity_dir.join(DAEMON_SUBKEY_PACKAGE_FILE_NAME), package)?;
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

    #[test]
    fn empty_sdk_registry_parses_as_empty_index() {
        let index = parse_index_payload(br#"{"identities":[]}"#).unwrap();

        assert_eq!(index.schema_version, INDEX_SCHEMA_VERSION);
        assert!(index.default_credential_name.is_empty());
        assert!(index.credentials.is_empty());
    }
}
