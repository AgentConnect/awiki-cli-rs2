use super::handle_input::stored_handle_fields;
use super::layout::{
    ensure_dir, file_exists, preferred_dir_name, read_json_value, read_text,
    sanitize_identity_name, write_secure_json, write_secure_text, Manager,
};
use super::types::{
    IdentityError, IdentitySummary, IndexEntry, SaveInput, StoredIdentity, UserState,
};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

#[derive(Debug, Serialize, Deserialize, Default)]
struct IdentityPayload {
    #[serde(default, deserialize_with = "deserialize_string_lossy")]
    did: String,
    #[serde(default, deserialize_with = "deserialize_string_lossy")]
    unique_id: String,
    #[serde(
        default,
        deserialize_with = "deserialize_string_lossy",
        skip_serializing_if = "String::is_empty"
    )]
    created_at: String,
    #[serde(
        default,
        deserialize_with = "deserialize_string_lossy",
        skip_serializing_if = "String::is_empty"
    )]
    user_id: String,
    #[serde(
        default,
        rename = "name",
        deserialize_with = "deserialize_string_lossy",
        skip_serializing_if = "String::is_empty"
    )]
    display_name: String,
    #[serde(
        default,
        deserialize_with = "deserialize_string_lossy",
        skip_serializing_if = "String::is_empty"
    )]
    handle: String,
    #[serde(
        default,
        deserialize_with = "deserialize_string_lossy",
        skip_serializing_if = "String::is_empty"
    )]
    full_handle: String,
}

impl Manager {
    pub fn save(&self, mut input: SaveInput) -> Result<StoredIdentity, IdentityError> {
        if input.identity_name.trim().is_empty() {
            return Err(IdentityError::InvalidInput(
                "invalid input: identity name is required".to_string(),
            ));
        }
        if input.did.trim().is_empty() || input.unique_id.trim().is_empty() {
            return Err(IdentityError::InvalidInput(
                "invalid input: did and unique_id are required".to_string(),
            ));
        }
        let handle_fields = stored_handle_fields(&input.handle, &input.full_handle, &input.did);
        input.handle = handle_fields.0;
        input.full_handle = handle_fields.1;
        self.ensure_root()?;
        let mut index = self.load_index()?;
        if let Some(existing) = index.credentials.get(&input.identity_name) {
            if existing.did != input.did && !input.replace_existing {
                return Err(IdentityError::Conflict(format!(
                    "identity conflict: identity {} already exists for did {}",
                    input.identity_name, existing.did
                )));
            }
        }
        let dir_name = preferred_dir_name(&input.unique_id)?;
        for (name, entry) in &index.credentials {
            if name == &input.identity_name {
                continue;
            }
            if entry.dir_name == dir_name && entry.did != input.did {
                return Err(IdentityError::Conflict(format!(
                    "identity conflict: dir {dir_name} already used by identity {name}"
                )));
            }
        }

        let paths = self.build_paths(&dir_name);
        ensure_dir(Path::new(&paths.identity_dir))?;
        let created_at = self
            .load(&input.identity_name)
            .ok()
            .filter(|existing| !existing.created_at.is_empty())
            .map(|existing| existing.created_at)
            .unwrap_or_else(now_rfc3339);

        let payload = IdentityPayload {
            did: input.did.clone(),
            unique_id: input.unique_id.clone(),
            created_at: created_at.clone(),
            user_id: input.user_id.clone(),
            display_name: input.display_name.clone(),
            handle: input.handle.clone(),
            full_handle: input.full_handle.clone(),
        };
        write_secure_json(&paths.identity_path, &payload)?;
        write_secure_json(
            &paths.auth_path,
            &json!({ "jwt_token": nullable_string(&input.jwt_token) }),
        )?;
        if let Some(document) = &input.did_document {
            write_secure_json(&paths.did_document_path, document)?;
        }
        if !input.key1_private_pem.is_empty() {
            write_secure_text(&paths.key1_private_path, &input.key1_private_pem)?;
        }
        if !input.key1_public_pem.is_empty() {
            write_secure_text(&paths.key1_public_path, &input.key1_public_pem)?;
        }
        if !input.e2ee_signing_private_pem.is_empty() {
            write_secure_text(
                &paths.e2ee_signing_private_path,
                &input.e2ee_signing_private_pem,
            )?;
        }
        if !input.e2ee_agreement_private_pem.is_empty() {
            write_secure_text(
                &paths.e2ee_agreement_private_path,
                &input.e2ee_agreement_private_pem,
            )?;
        }

        let is_default = index.default_credential_name == input.identity_name
            || index.default_credential_name.is_empty();
        if index.default_credential_name.is_empty() {
            index.default_credential_name = input.identity_name.clone();
        }
        index.credentials.insert(
            input.identity_name.clone(),
            IndexEntry {
                credential_name: input.identity_name.clone(),
                dir_name,
                did: input.did,
                unique_id: input.unique_id,
                user_id: input.user_id,
                name: input.display_name,
                handle: input.handle,
                full_handle: input.full_handle,
                created_at,
                is_default,
            },
        );
        self.save_index(index)?;
        self.load(&input.identity_name)
    }

    pub fn load(&self, name: &str) -> Result<StoredIdentity, IdentityError> {
        let mut index = self.load_index()?;
        let (resolved_name, mut entry) = self
            .resolve_entry_name(name, &index)
            .ok_or_else(|| IdentityError::NotFound(format!("identity not found: {name}")))?;
        let paths = self.build_paths(&entry.dir_name);
        let mut identity_value = read_json_value(&paths.identity_path)?;
        let payload: IdentityPayload = serde_json::from_value(identity_value.clone())?;
        let auth = read_json_value(&paths.auth_path).unwrap_or(Value::Null);
        let mut record = StoredIdentity {
            identity_name: resolved_name.clone(),
            dir_name: entry.dir_name.clone(),
            did: fallback_string(&payload.did, &entry.did),
            unique_id: fallback_string(&payload.unique_id, &entry.unique_id),
            user_id: fallback_string(&payload.user_id, &entry.user_id),
            display_name: fallback_string(&payload.display_name, &entry.name),
            handle: fallback_string(&payload.handle, &entry.handle),
            full_handle: fallback_string(&payload.full_handle, &entry.full_handle),
            created_at: fallback_string(&payload.created_at, &entry.created_at),
            jwt_token: auth
                .get("jwt_token")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            did_document: read_json_value(&paths.did_document_path).ok(),
            key1_private_pem: read_text(&paths.key1_private_path),
            key1_public_pem: read_text(&paths.key1_public_path),
            e2ee_signing_private_pem: read_text(&paths.e2ee_signing_private_path),
            e2ee_agreement_private_pem: read_text(&paths.e2ee_agreement_private_path),
            is_default: index.default_credential_name == resolved_name,
        };
        let (handle, full_handle) =
            stored_handle_fields(&record.handle, &record.full_handle, &record.did);
        record.handle = handle;
        record.full_handle = full_handle;
        persist_identity_handle_backfill(&paths.identity_path, &mut identity_value, &record)?;
        if (entry.handle != record.handle && !record.handle.is_empty())
            || (entry.full_handle != record.full_handle && !record.full_handle.is_empty())
        {
            if !record.handle.is_empty() {
                entry.handle = record.handle.clone();
            }
            if !record.full_handle.is_empty() {
                entry.full_handle = record.full_handle.clone();
            }
            index.credentials.insert(resolved_name, entry);
            self.save_index(index)?;
        }
        Ok(record)
    }

    pub fn update_jwt(&self, name: &str, jwt_token: &str) -> Result<(), IdentityError> {
        let record = self.load(name)?;
        let paths = self.build_paths(&record.dir_name);
        write_secure_json(
            &paths.auth_path,
            &json!({ "jwt_token": nullable_string(jwt_token) }),
        )
    }

    pub fn update_display_name(&self, name: &str, display_name: &str) -> Result<(), IdentityError> {
        let record = self.load(name)?;
        let paths = self.build_paths(&record.dir_name);
        let mut payload = read_json_value(&paths.identity_path)?;
        let object = payload.as_object_mut().ok_or_else(|| {
            IdentityError::Internal("identity payload must be a JSON object".to_string())
        })?;
        object.insert("name".to_string(), Value::String(display_name.to_string()));
        write_secure_json(&paths.identity_path, &payload)?;

        let mut index = self.load_index()?;
        let mut entry = index
            .credentials
            .get(&record.identity_name)
            .cloned()
            .ok_or_else(|| {
                IdentityError::NotFound(format!("identity not found: {}", record.identity_name))
            })?;
        entry.name = display_name.to_string();
        index.credentials.insert(record.identity_name, entry);
        self.save_index(index)
    }

    pub fn list(&self) -> Result<Vec<IdentitySummary>, IdentityError> {
        let index = self.load_index()?;
        let mut items = Vec::with_capacity(index.credentials.len());
        for entry in index.credentials.values() {
            items.push(self.summary_for(entry, &index.default_credential_name)?);
        }
        Ok(items)
    }

    pub fn current(&self) -> Result<IdentitySummary, IdentityError> {
        let mut index = self.load_index()?;
        if index.default_credential_name.is_empty() && index.credentials.contains_key("default") {
            index.default_credential_name = "default".to_string();
        }
        let name = index.default_credential_name.clone();
        if name.is_empty() {
            return Err(IdentityError::NoDefaultIdentity(
                "no default identity".to_string(),
            ));
        }
        let entry = index.credentials.get(&name).ok_or_else(|| {
            IdentityError::NoDefaultIdentity(format!("no default identity: {name}"))
        })?;
        self.summary_for(entry, &name)
    }

    pub fn set_default(&self, name: &str) -> Result<IdentitySummary, IdentityError> {
        let mut index = self.load_index()?;
        let entry = index
            .credentials
            .get(name)
            .cloned()
            .ok_or_else(|| IdentityError::NotFound(format!("identity not found: {name}")))?;
        index.default_credential_name = name.to_string();
        self.save_index(index)?;
        self.summary_for(&entry, name)
    }

    pub fn summary_for(
        &self,
        entry: &IndexEntry,
        default_name: &str,
    ) -> Result<IdentitySummary, IdentityError> {
        let paths = self.build_paths(&entry.dir_name);
        if entry.full_handle.trim().is_empty() && !entry.credential_name.trim().is_empty() {
            if let Ok(record) = self.load(&entry.credential_name) {
                let mut summary = identity_summary_from_record(&record);
                summary.is_default = default_name == entry.credential_name;
                summary.user_state = evaluate_identity_summary_user_state(&summary);
                return Ok(summary);
            }
        }
        let auth = read_json_value(&paths.auth_path).unwrap_or(Value::Null);
        let mut summary = IdentitySummary {
            identity_name: entry.credential_name.clone(),
            did: entry.did.clone(),
            unique_id: entry.unique_id.clone(),
            user_id: entry.user_id.clone(),
            display_name: entry.name.clone(),
            handle: entry.handle.clone(),
            full_handle: entry.full_handle.clone(),
            created_at: entry.created_at.clone(),
            dir_name: entry.dir_name.clone(),
            is_default: default_name == entry.credential_name,
            has_jwt: auth
                .get("jwt_token")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.is_empty()),
            has_did_document: file_exists(&paths.did_document_path),
            has_key1_private: file_exists(&paths.key1_private_path),
            has_key1_public: file_exists(&paths.key1_public_path),
            has_e2ee_signing_private: file_exists(&paths.e2ee_signing_private_path),
            has_e2ee_agreement_private: file_exists(&paths.e2ee_agreement_private_path),
            user_state: UserState::default(),
        };
        summary.user_state = evaluate_identity_summary_user_state(&summary);
        Ok(summary)
    }
}

fn persist_identity_handle_backfill(
    identity_path: &str,
    identity_value: &mut Value,
    record: &StoredIdentity,
) -> Result<(), IdentityError> {
    let Some(payload) = identity_value.as_object_mut() else {
        return Ok(());
    };
    let mut changed = false;
    if !record.handle.is_empty()
        && payload
            .get("handle")
            .and_then(Value::as_str)
            .unwrap_or_default()
            != record.handle
    {
        payload.insert("handle".to_string(), Value::String(record.handle.clone()));
        changed = true;
    }
    if !record.full_handle.is_empty()
        && payload
            .get("full_handle")
            .and_then(Value::as_str)
            .unwrap_or_default()
            != record.full_handle
    {
        payload.insert(
            "full_handle".to_string(),
            Value::String(record.full_handle.clone()),
        );
        changed = true;
    }
    if changed {
        write_secure_json(identity_path, identity_value)?;
    }
    Ok(())
}

pub fn identity_summary_from_record(record: &StoredIdentity) -> IdentitySummary {
    let mut summary = IdentitySummary {
        identity_name: record.identity_name.clone(),
        did: record.did.clone(),
        unique_id: record.unique_id.clone(),
        user_id: record.user_id.clone(),
        display_name: record.display_name.clone(),
        handle: record.handle.clone(),
        full_handle: record.full_handle.clone(),
        created_at: record.created_at.clone(),
        dir_name: record.dir_name.clone(),
        is_default: record.is_default,
        has_jwt: !record.jwt_token.is_empty(),
        has_did_document: record.did_document.is_some(),
        has_key1_private: !record.key1_private_pem.is_empty(),
        has_key1_public: !record.key1_public_pem.is_empty(),
        has_e2ee_signing_private: !record.e2ee_signing_private_pem.is_empty(),
        has_e2ee_agreement_private: !record.e2ee_agreement_private_pem.is_empty(),
        user_state: UserState::default(),
    };
    summary.user_state = evaluate_identity_summary_user_state(&summary);
    summary
}

pub fn evaluate_user_state(user_id: &str, handle: &str) -> UserState {
    let mut missing = Vec::new();
    if user_id.trim().is_empty() {
        missing.push("registration".to_string());
    }
    if handle.trim().is_empty() {
        missing.push("handle".to_string());
    }
    let registration_state = match missing.len() {
        0 => "registered_user",
        1 => "partial_user",
        _ => "local_identity",
    };
    UserState {
        registration_state: registration_state.to_string(),
        ready_for_messaging: missing.is_empty(),
        missing,
    }
}

pub fn evaluate_identity_summary_user_state(summary: &IdentitySummary) -> UserState {
    evaluate_user_state(&summary.user_id, &summary.handle)
}

pub fn choose_default_identity_name(
    requested: &str,
    existing: &[IdentitySummary],
    fallback: &str,
) -> String {
    let sanitized = sanitize_identity_name(requested);
    if !sanitized.is_empty() {
        return sanitized;
    }
    if existing.is_empty() {
        return "default".to_string();
    }
    choose_named_identity("", existing, fallback)
}

pub fn choose_named_identity(
    requested: &str,
    existing: &[IdentitySummary],
    fallback: &str,
) -> String {
    let requested = sanitize_identity_name(requested);
    if !requested.is_empty() {
        return requested;
    }
    let mut base = sanitize_identity_name(fallback);
    if base.is_empty() {
        base = "identity".to_string();
    }
    if !existing.iter().any(|summary| summary.identity_name == base) {
        return base;
    }
    for idx in 2..1000 {
        let candidate = format!("{base}-{idx}");
        if !existing
            .iter()
            .any(|summary| summary.identity_name == candidate)
        {
            return candidate;
        }
    }
    format!("{base}-{}", OffsetDateTime::now_utc().unix_timestamp())
}

pub fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

pub fn copy_optional_legacy_e2ee_state(src: &Path, dst: &str) {
    if src.is_file() {
        if let Ok(raw) = fs::read(src) {
            let _ = fs::write(dst, raw);
        }
    }
}

fn nullable_string(value: &str) -> Value {
    if value.is_empty() {
        Value::Null
    } else {
        Value::String(value.to_string())
    }
}

fn fallback_string(value: &str, fallback: &str) -> String {
    if value.is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

fn deserialize_string_lossy<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    Ok(value
        .and_then(|value| value.as_str().map(ToString::to_string))
        .unwrap_or_default())
}
