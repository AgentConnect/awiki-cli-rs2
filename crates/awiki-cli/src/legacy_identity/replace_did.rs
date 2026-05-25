use super::client::Client;
use super::did::{did_suffix, generate_identity_with_path_segments};
use super::handle_input::default_string;
use super::layout::{
    copy_dir, path_string, preferred_dir_name, sanitize_component, sanitize_identity_name,
    write_secure_json, Manager,
};
use super::legacy_store::identity_summary_from_record;
use super::service::{auth_session, load_identity_for_mutation, CommandResult};
use super::types::{
    IdentityError, IndexEntry, ReplaceDidParams, SaveInput, StoredIdentity, LEGACY_BACKUP_DIR_NAME,
};
use super::wire::{build_replace_did_rpc_call, replace_did_result, ReplaceDidRpcParams};
use crate::config::{self, Resolved};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

#[derive(Debug, Clone)]
pub struct ReplaceDidBackupResult {
    pub backup_path: String,
    pub manifest: ReplaceDidBackupManifest,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReplaceDidBackupManifest {
    pub reason: String,
    pub created_at: String,
    pub identity_name: String,
    pub linked_identity_names: Vec<String>,
    pub old_did: String,
    pub old_dir_name: String,
    pub planned_new_did: String,
}

pub fn replace_did(
    resolved: &Resolved,
    manager: &Manager,
    params: ReplaceDidParams,
) -> Result<CommandResult, IdentityError> {
    let record = load_identity_for_mutation(resolved, manager, &params.identity_name)?;
    let (did_domain, path_segments) = handle_path_prefix_from_did(&record.did)?;
    let generated = generate_identity_with_path_segments(
        &did_domain,
        path_segments.iter().map(String::as_str),
        &default_value_for_replacement(
            &resolved.anp_service_endpoint,
            &config::derive_anp_service_endpoint(&resolved.service_base_url),
        ),
        &default_value_for_replacement(
            &resolved.anp_service_did,
            &config::derive_anp_service_did(&resolved.service_base_url),
        ),
    )?;
    let backup =
        manager.backup_identity_for_did_replacement(&record.identity_name, &generated.did)?;
    let mut auth = auth_session(resolved, manager, &record)?;
    let call = build_replace_did_rpc_call(ReplaceDidRpcParams {
        new_did_document: generated.did_document.clone(),
        is_public: params.is_public,
        is_agent: params.is_agent,
        role: params.role,
        endpoint_url: params.endpoint_url,
    });
    let client = Client::new(resolved)?;
    let result: Value = client.authenticated_rpc_call_profile(
        call.profile,
        call.endpoint,
        call.method,
        call.params,
        &mut auth,
    )?;
    let new_did = string_value(&result, "did", &generated.did);
    let replaced = manager.replace_identity(
        &record.identity_name,
        SaveInput {
            identity_name: record.identity_name.clone(),
            did: new_did.clone(),
            unique_id: did_suffix(&new_did),
            user_id: string_value(&result, "user_id", &record.user_id),
            display_name: record.display_name.clone(),
            handle: string_value(&result, "handle", &record.handle),
            full_handle: default_string(
                &string_value(&result, "full_handle", ""),
                &record.full_handle,
            ),
            jwt_token: string_value(&result, "access_token", auth.current_jwt()),
            did_document: Some(generated.did_document),
            key1_private_pem: generated.key1_private_pem,
            key1_public_pem: generated.key1_public_pem,
            e2ee_signing_private_pem: generated.e2ee_signing_private_pem,
            e2ee_agreement_private_pem: generated.e2ee_agreement_private_pem,
            replace_existing: true,
        },
    )?;
    Ok(replace_did_result(
        &identity_summary_from_record(&replaced),
        &record.did,
        &replaced.did,
        &backup.backup_path,
        result,
    ))
}

impl Manager {
    pub fn backup_identity_for_did_replacement(
        &self,
        name: &str,
        planned_new_did: &str,
    ) -> Result<ReplaceDidBackupResult, IdentityError> {
        if name.trim().is_empty() {
            return Err(IdentityError::InvalidInput(
                "invalid input: identity name is required".to_string(),
            ));
        }
        if planned_new_did.trim().is_empty() {
            return Err(IdentityError::InvalidInput(
                "invalid input: planned new did is required".to_string(),
            ));
        }
        self.ensure_root()?;
        let index = self.load_index()?;
        let (resolved_name, entry) = self
            .resolve_entry_name(name, &index)
            .ok_or_else(|| IdentityError::NotFound(format!("identity not found: {name}")))?;
        let current = self.load(&resolved_name)?;
        let old_paths = self.build_paths(&entry.dir_name);
        let source = Path::new(&old_paths.identity_dir);
        let metadata = fs::metadata(source).map_err(|err| {
            IdentityError::Internal(format!("stat identity directory before backup: {err}"))
        })?;
        if !metadata.is_dir() {
            return Err(IdentityError::InvalidInput(format!(
                "invalid input: identity path is not a directory: {}",
                old_paths.identity_dir
            )));
        }

        let linked_identity_names = linked_identity_names(&index.credentials, &entry);
        let created_at = OffsetDateTime::now_utc();
        let backup_dir =
            unique_backup_dir(self.replace_did_backup_root().join("replace-did").join(
                replace_did_backup_name(created_at, &resolved_name, &entry.dir_name),
            ));
        copy_dir(source, &backup_dir).map_err(|err| {
            IdentityError::Internal(format!(
                "backup identity directory before DID replacement: {err}"
            ))
        })?;

        let manifest = ReplaceDidBackupManifest {
            reason: "replace_did".to_string(),
            created_at: created_at
                .format(&Rfc3339)
                .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string()),
            identity_name: resolved_name,
            linked_identity_names,
            old_did: current.did,
            old_dir_name: entry.dir_name,
            planned_new_did: planned_new_did.to_string(),
        };
        write_secure_json(
            &backup_dir.join("backup_manifest.json").to_string_lossy(),
            &manifest,
        )
        .map_err(|err| {
            IdentityError::Internal(format!("write DID replacement backup manifest: {err}"))
        })?;
        Ok(ReplaceDidBackupResult {
            backup_path: path_string(&backup_dir),
            manifest,
        })
    }

    pub fn replace_identity(
        &self,
        name: &str,
        mut input: SaveInput,
    ) -> Result<StoredIdentity, IdentityError> {
        let requested = name.trim();
        if requested.is_empty() {
            return Err(IdentityError::InvalidInput(
                "invalid input: identity name is required".to_string(),
            ));
        }
        if input.did.trim().is_empty() || input.unique_id.trim().is_empty() {
            return Err(IdentityError::InvalidInput(
                "invalid input: did and unique_id are required".to_string(),
            ));
        }
        self.ensure_root()?;
        let index = self.load_index()?;
        let (resolved_name, entry) = self
            .resolve_entry_name(requested, &index)
            .ok_or_else(|| IdentityError::NotFound(format!("identity not found: {requested}")))?;
        let current = self.load(&resolved_name)?;

        if input.identity_name.trim().is_empty() {
            input.identity_name = resolved_name.clone();
        }
        if input.user_id.trim().is_empty() {
            input.user_id = current.user_id.clone();
        }
        if input.display_name.trim().is_empty() {
            input.display_name = current.display_name.clone();
        }
        if input.handle.trim().is_empty() {
            input.handle = current.handle.clone();
        }
        if input.full_handle.trim().is_empty() {
            input.full_handle = current.full_handle.clone();
        }

        let new_dir_name = preferred_dir_name(&input.unique_id)?;
        let linked_names = linked_identity_names(&index.credentials, &entry);
        let linked_lookup = linked_names.iter().cloned().collect::<BTreeSet<_>>();
        for (candidate_name, candidate_entry) in &index.credentials {
            if linked_lookup.contains(candidate_name) {
                continue;
            }
            if candidate_entry.did == input.did {
                return Err(IdentityError::Conflict(format!(
                    "identity conflict: did {} already belongs to identity {candidate_name}",
                    input.did
                )));
            }
            if candidate_entry.dir_name == new_dir_name {
                return Err(IdentityError::Conflict(format!(
                    "identity conflict: dir {new_dir_name} already used by identity {candidate_name}"
                )));
            }
        }

        let old_paths = self.build_paths(&entry.dir_name);
        let replaced = self.save(input)?;
        let _ = fs::remove_file(&self.build_paths(&replaced.dir_name).e2ee_state_path);
        let mut index_after = self.load_index()?;
        for linked_name in linked_lookup {
            if let Some(linked_entry) = index_after.credentials.get_mut(&linked_name) {
                linked_entry.credential_name = linked_name.clone();
                linked_entry.dir_name = replaced.dir_name.clone();
                linked_entry.did = replaced.did.clone();
                linked_entry.unique_id = replaced.unique_id.clone();
                linked_entry.user_id = replaced.user_id.clone();
                linked_entry.name = replaced.display_name.clone();
                linked_entry.handle = replaced.handle.clone();
                linked_entry.full_handle = replaced.full_handle.clone();
                linked_entry.created_at = replaced.created_at.clone();
                linked_entry.is_default = index_after.default_credential_name == linked_name;
            }
        }
        self.save_index(index_after)?;
        let new_paths = self.build_paths(&replaced.dir_name);
        if old_paths.identity_dir != new_paths.identity_dir {
            let _ = fs::remove_dir_all(&old_paths.identity_dir);
        }
        self.load(&resolved_name)
    }

    fn replace_did_backup_root(&self) -> PathBuf {
        Path::new(self.root_dir()).join(LEGACY_BACKUP_DIR_NAME)
    }
}

fn handle_path_prefix_from_did(did: &str) -> Result<(String, Vec<String>), IdentityError> {
    let (domain, path_segments) = parse_did_path(did)?;
    if path_segments.is_empty() || path_segments[0].eq_ignore_ascii_case("user") {
        return Err(IdentityError::InvalidInput(
            "invalid input: current did is not a handle did".to_string(),
        ));
    }
    Ok((domain, path_segments))
}

fn parse_did_path(did: &str) -> Result<(String, Vec<String>), IdentityError> {
    let trimmed = did.trim();
    if !trimmed.starts_with("did:wba:") {
        return Err(IdentityError::InvalidInput(format!(
            "invalid input: invalid did {did:?}"
        )));
    }
    let parts = trimmed.split(':').collect::<Vec<_>>();
    if parts.len() < 5 {
        return Err(IdentityError::InvalidInput(format!(
            "invalid input: invalid did {did:?}"
        )));
    }
    let domain = path_unescape(parts[2]).ok_or_else(|| {
        IdentityError::InvalidInput(format!("invalid input: invalid did domain {:?}", parts[2]))
    })?;
    let path_segments = parts[3..parts.len() - 1]
        .iter()
        .map(|segment| (*segment).to_string())
        .collect::<Vec<_>>();
    if path_segments.is_empty() {
        return Err(IdentityError::InvalidInput(
            "invalid input: missing did path segments".to_string(),
        ));
    }
    Ok((domain, path_segments))
}

fn path_unescape(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            output.push(bytes[index]);
            index += 1;
            continue;
        }
        if index + 2 >= bytes.len() {
            return None;
        }
        let hi = hex_value(bytes[index + 1])?;
        let lo = hex_value(bytes[index + 2])?;
        output.push((hi << 4) | lo);
        index += 3;
    }
    String::from_utf8(output).ok()
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn linked_identity_names(
    credentials: &std::collections::BTreeMap<String, IndexEntry>,
    entry: &IndexEntry,
) -> Vec<String> {
    let mut names = credentials
        .iter()
        .filter_map(|(name, candidate)| {
            (candidate.dir_name == entry.dir_name || candidate.did == entry.did)
                .then(|| name.clone())
        })
        .collect::<Vec<_>>();
    if names.is_empty() && !entry.credential_name.trim().is_empty() {
        names.push(entry.credential_name.clone());
    }
    names.sort();
    names
}

fn replace_did_backup_name(
    created_at: OffsetDateTime,
    identity_name: &str,
    dir_name: &str,
) -> String {
    format!(
        "{}-{}-{}",
        compact_timestamp(created_at),
        sanitize_identity_name(identity_name),
        sanitize_component(dir_name)
    )
}

fn compact_timestamp(created_at: OffsetDateTime) -> String {
    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}.{:09}Z",
        created_at.year(),
        u8::from(created_at.month()),
        created_at.day(),
        created_at.hour(),
        created_at.minute(),
        created_at.second(),
        created_at.nanosecond()
    )
}

fn unique_backup_dir(base: PathBuf) -> PathBuf {
    if !base.exists() {
        return base;
    }
    let parent = base.parent().map(Path::to_path_buf).unwrap_or_default();
    let name = base
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "backup".to_string());
    for idx in 2..1000 {
        let candidate = parent.join(format!("{name}-{idx}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    parent.join(format!(
        "{name}-{}",
        OffsetDateTime::now_utc().unix_timestamp_nanos()
    ))
}

fn default_value_for_replacement(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

fn string_value(result: &Value, key: &str, fallback: &str) -> String {
    result
        .get(key)
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .unwrap_or_else(|| fallback.to_string())
}
