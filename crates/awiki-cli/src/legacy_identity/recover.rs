use super::handle_input::{default_string, derive_full_handle_from_did, normalize_handle_input};
use super::layout::{
    copy_dir, ensure_dir, sanitize_component, sanitize_identity_name, write_secure_json,
    write_secure_text, Manager,
};
use super::legacy_store::choose_named_identity;
use super::types::{
    IdentityError, IdentitySummary, RecoverParams, StoredIdentity, LEGACY_BACKUP_DIR_NAME,
};
use super::CommandResult;
use crate::config;
use serde::Serialize;
use serde_json::{json, Value};
use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

const RECOVER_IDENTITY_IGNORED_WARNING: &str = "The --identity flag is ignored by `awiki-cli id recover`; the recover target and final live identity are derived only from --handle.";

#[derive(Debug, Clone)]
pub struct RecoverPlan {
    pub target_handle: String,
    pub target_local_part: String,
    pub effective_domain: String,
    pub handle_key: String,
    pub final_identity_name: String,
    pub temp_identity_name: String,
    pub backup_path_preview: String,
    pub same_handle_candidates: Vec<IdentitySummary>,
    pub excluded_identities: Vec<IdentitySummary>,
}

#[derive(Debug, Clone)]
pub struct RecoverBackupRequest<'a> {
    pub handle: &'a str,
    pub candidates: &'a [IdentitySummary],
    pub planned_final_identity_name: &'a str,
    pub planned_temp_identity_name: &'a str,
    pub active_before: &'a str,
    pub config_file: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct RecoverBackupResult {
    pub backup_path: String,
    pub manifest: RecoverBackupManifest,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecoverBackupManifest {
    pub reason: String,
    pub created_at: String,
    pub handle: String,
    pub archived_identity_names: Vec<String>,
    pub archived_dids: Vec<String>,
    pub archived_dir_names: Vec<String>,
    pub default_before: String,
    pub active_before: String,
    pub planned_final_identity: String,
    pub planned_temp_identity: String,
}

#[derive(Debug, Clone)]
pub struct RecoverPromotionResult {
    pub identity: StoredIdentity,
    pub default_updated: bool,
}

#[derive(Debug, Clone)]
pub struct RecoverFinalizeRequest<'a> {
    pub final_identity_name: &'a str,
    pub temp_identity_name: &'a str,
    pub archived_identity_names: &'a [String],
    pub active_before: &'a str,
    pub backup_path: &'a str,
    pub new_did: &'a str,
    pub config_paths: Option<&'a config::Paths>,
}

#[derive(Debug, Clone)]
pub struct RecoverFinalizeResult {
    pub identity: StoredIdentity,
    pub default_updated: bool,
    pub active_config_updated: bool,
}

#[derive(Debug)]
pub struct RecoverFinalizeError {
    pub err: IdentityError,
    pub backup_path: String,
    pub temp_identity_name: String,
    pub new_did: String,
}

impl std::fmt::Display for RecoverFinalizeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} (backup_path={} temp_identity_name={} new_did={})",
            self.err, self.backup_path, self.temp_identity_name, self.new_did
        )
    }
}

impl std::error::Error for RecoverFinalizeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.err)
    }
}

impl RecoverPlan {
    pub fn archived_identity_names(&self) -> Vec<String> {
        self.same_handle_candidates
            .iter()
            .map(|summary| summary.identity_name.clone())
            .collect()
    }

    pub fn archived_dids(&self) -> Vec<String> {
        self.same_handle_candidates
            .iter()
            .filter_map(|summary| {
                let did = summary.did.trim();
                (!did.is_empty()).then(|| did.to_string())
            })
            .collect()
    }

    pub fn old_owner_dids_in_merge_order(&self) -> Vec<String> {
        let mut dids = Vec::with_capacity(self.same_handle_candidates.len());
        for summary in &self.same_handle_candidates {
            let did = summary.did.trim();
            if did.is_empty() || dids.iter().any(|seen| seen == did) {
                continue;
            }
            dids.push(did.to_string());
        }
        dids
    }
}

pub fn recover_identity_ignored_warning() -> &'static str {
    RECOVER_IDENTITY_IGNORED_WARNING
}

pub fn recover_active_before(config_file: &str) -> Result<String, IdentityError> {
    if config_file.trim().is_empty() {
        return Ok(String::new());
    }
    let (file_config, _, error) = config::read_file_config(config_file);
    if !error.is_empty() {
        return Err(IdentityError::Internal(format!(
            "read config before handle recover: {error}"
        )));
    }
    Ok(file_config.identity.active.trim().to_string())
}

pub fn finalize_recovered_handle(
    manager: &Manager,
    request: RecoverFinalizeRequest<'_>,
) -> Result<RecoverFinalizeResult, RecoverFinalizeError> {
    let index_before = manager.load_index().map_err(|err| {
        recover_finalize_error("load live identity index before promotion", err, &request)
    })?;

    let promoted = manager
        .promote_recovered_handle(
            request.final_identity_name,
            request.temp_identity_name,
            request.archived_identity_names,
        )
        .map_err(|err| {
            recover_finalize_error(
                "promote recovered handle into the live index",
                err,
                &request,
            )
        })?;

    let active_was_archived = request
        .archived_identity_names
        .iter()
        .any(|name| name.trim() == request.active_before.trim() && !name.trim().is_empty());
    let mut active_config_updated = false;
    if active_was_archived {
        if let Some(paths) = request.config_paths {
            if let Err(err) = config::update_active_identity(paths, request.final_identity_name) {
                if let Err(restore_err) = manager.save_index(index_before) {
                    return Err(recover_finalize_error_message(
                        format!(
                            "update config active identity: {err} (also failed to restore live index: {restore_err})"
                        ),
                        &request,
                    ));
                }
                return Err(recover_finalize_error_message(
                    format!("update config active identity: {err}"),
                    &request,
                ));
            }
            active_config_updated = true;
        }
    }

    Ok(RecoverFinalizeResult {
        identity: promoted.identity,
        default_updated: promoted.default_updated,
        active_config_updated,
    })
}

pub fn recover_preview(
    manager: &Manager,
    did_domain: &str,
    params: RecoverParams,
) -> Result<CommandResult, IdentityError> {
    let plan = build_recover_plan(manager, did_domain, &params)?;
    let (action, remote_calls, local_writes, backup_path) = if params.otp.trim().is_empty() {
        (
            "send_recover_otp",
            json!(["handle.send_otp"]),
            Value::Null,
            String::new(),
        )
    } else {
        (
            "recover_handle",
            json!(["did-auth.recover_handle"]),
            json!([
                ".legacy-backup/recover-handle",
                "index.json",
                "config.yaml",
                "identity.json",
                "auth.json",
                "did_document.json",
                "key-1-private.pem",
                "key-1-public.pem",
                "e2ee-signing-private.pem",
                "e2ee-agreement-private.pem",
                "sqlite.recover_handle_merge",
                "sqlite.e2ee_cleanup",
            ]),
            plan.backup_path_preview.clone(),
        )
    };

    Ok(CommandResult {
        data: json!({
            "plan": {
                "action": action,
                "target_handle": plan.target_handle,
                "identity_name": plan.final_identity_name,
                "final_identity_name": plan.final_identity_name,
                "temp_identity_name": plan.temp_identity_name,
                "same_handle_candidates": plan.same_handle_candidates,
                "excluded_identities": plan.excluded_identities,
                "backup_path": backup_path,
                "phone": params.phone,
                "remote_calls": remote_calls,
                "local_writes": local_writes,
            }
        }),
        summary: "Dry run: handle recovery planned".to_string(),
        warnings: Vec::new(),
    })
}

pub fn build_recover_plan(
    manager: &Manager,
    did_domain: &str,
    params: &RecoverParams,
) -> Result<RecoverPlan, IdentityError> {
    let target = normalize_handle_input(&params.handle, did_domain)?;
    let existing = manager.list()?;
    let identity_base = if target.explicit_domain {
        target.full_handle.clone()
    } else {
        target.local_part.clone()
    };
    let final_identity_name = sanitize_identity_name(&identity_base);
    if final_identity_name.is_empty() {
        return Err(IdentityError::InvalidInput(format!(
            "invalid input: handle {:?} cannot be used as an identity name",
            params.handle
        )));
    }

    let handle_key = canonical_handle(&target.full_handle);
    let mut same_handle = Vec::new();
    let mut excluded = Vec::new();
    for summary in existing {
        let full_handle = default_string(
            &summary.full_handle,
            &derive_full_handle_from_did(&summary.handle, &summary.did),
        );
        if canonical_handle(&full_handle) == handle_key {
            same_handle.push(summary);
        } else {
            excluded.push(summary);
        }
    }
    same_handle.sort_by(
        |left, right| match compare_rfc3339(&left.created_at, &right.created_at) {
            Ordering::Equal => left.identity_name.cmp(&right.identity_name),
            ordering => ordering,
        },
    );
    for summary in &excluded {
        if summary.identity_name == final_identity_name {
            return Err(IdentityError::Conflict(format!(
                "identity conflict: identity name {final_identity_name} is already used by another handle"
            )));
        }
    }

    let temp_base = format!("{final_identity_name}-recover-tmp");
    let existing = same_handle_and_excluded(&same_handle, &excluded);
    let temp_identity_name = choose_named_identity(&temp_base, &existing, &temp_base);

    let backup_path_preview = manager.preview_recover_handle_backup_path(&target.full_handle);

    Ok(RecoverPlan {
        target_handle: target.full_handle,
        target_local_part: target.local_part,
        effective_domain: target.effective_domain,
        handle_key,
        final_identity_name,
        temp_identity_name,
        backup_path_preview,
        same_handle_candidates: same_handle,
        excluded_identities: excluded,
    })
}

impl Manager {
    pub fn preview_recover_handle_backup_path(&self, handle: &str) -> String {
        self.recover_backup_root()
            .join("recover-handle")
            .join(backup_preview_name(handle))
            .to_string_lossy()
            .into_owned()
    }

    pub fn backup_identities_for_handle_recovery(
        &self,
        request: RecoverBackupRequest<'_>,
    ) -> Result<RecoverBackupResult, IdentityError> {
        if request.handle.trim().is_empty() {
            return Err(IdentityError::InvalidInput(
                "invalid input: handle is required".to_string(),
            ));
        }
        self.ensure_root()?;
        let index = self.load_index()?;
        let created_at = OffsetDateTime::now_utc();
        let backup_root = self.recover_backup_root().join("recover-handle");
        let backup_dir =
            unique_backup_dir(backup_root.join(backup_dir_name(created_at, request.handle)));
        ensure_dir(&backup_dir).map_err(|err| {
            IdentityError::Internal(format!("create recover backup directory: {err}"))
        })?;

        write_secure_json(
            &backup_dir.join("index.before.json").to_string_lossy(),
            &index,
        )
        .map_err(|err| {
            IdentityError::Internal(format!("write recover backup index snapshot: {err}"))
        })?;
        if let Some(config_file) = request.config_file {
            let config_file = config_file.trim();
            if !config_file.is_empty() {
                match fs::read(config_file) {
                    Ok(raw) => {
                        write_secure_text(
                            &backup_dir.join("config.before.yaml").to_string_lossy(),
                            &String::from_utf8_lossy(&raw),
                        )
                        .map_err(|err| {
                            IdentityError::Internal(format!(
                                "write recover backup config snapshot: {err}"
                            ))
                        })?;
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                    Err(err) => {
                        return Err(IdentityError::Internal(format!(
                            "read config before recover backup: {err}"
                        )));
                    }
                }
            }
        }

        let mut archived_identity_names = Vec::with_capacity(request.candidates.len());
        let mut archived_dids = Vec::with_capacity(request.candidates.len());
        let mut archived_dir_names = Vec::with_capacity(request.candidates.len());
        for (idx, summary) in request.candidates.iter().enumerate() {
            archived_identity_names.push(summary.identity_name.clone());
            archived_dids.push(summary.did.clone());
            archived_dir_names.push(summary.dir_name.clone());
            let source = PathBuf::from(self.build_paths(&summary.dir_name).identity_dir);
            let metadata = match fs::metadata(&source) {
                Ok(metadata) => metadata,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
                Err(err) => {
                    return Err(IdentityError::Internal(format!(
                        "stat identity directory before recover backup: {err}"
                    )));
                }
            };
            if !metadata.is_dir() {
                return Err(IdentityError::InvalidInput(format!(
                    "invalid input: identity path is not a directory: {}",
                    source.to_string_lossy()
                )));
            }
            let mut target_name = format!(
                "{:02}-{}",
                idx + 1,
                sanitize_component(&summary.identity_name)
            );
            if target_name.trim().is_empty() {
                target_name = format!("{:02}-{}", idx + 1, sanitize_component(&summary.dir_name));
            }
            copy_dir(&source, &backup_dir.join("identities").join(target_name)).map_err(|err| {
                IdentityError::Internal(format!("backup identity directory before recover: {err}"))
            })?;
        }

        let manifest = RecoverBackupManifest {
            reason: "recover_handle".to_string(),
            created_at: created_at
                .format(&Rfc3339)
                .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string()),
            handle: request.handle.to_string(),
            archived_identity_names,
            archived_dids,
            archived_dir_names,
            default_before: index.default_credential_name,
            active_before: request.active_before.trim().to_string(),
            planned_final_identity: request.planned_final_identity_name.to_string(),
            planned_temp_identity: request.planned_temp_identity_name.to_string(),
        };
        write_secure_json(
            &backup_dir.join("backup_manifest.json").to_string_lossy(),
            &manifest,
        )
        .map_err(|err| IdentityError::Internal(format!("write recover backup manifest: {err}")))?;
        Ok(RecoverBackupResult {
            backup_path: backup_dir.to_string_lossy().into_owned(),
            manifest,
        })
    }

    pub fn promote_recovered_handle(
        &self,
        final_identity_name: &str,
        temp_identity_name: &str,
        archived_identity_names: &[String],
    ) -> Result<RecoverPromotionResult, IdentityError> {
        let final_identity_name = final_identity_name.trim();
        let temp_identity_name = temp_identity_name.trim();
        if final_identity_name.is_empty() {
            return Err(IdentityError::InvalidInput(
                "invalid input: final identity name is required".to_string(),
            ));
        }
        if temp_identity_name.is_empty() {
            return Err(IdentityError::InvalidInput(
                "invalid input: temporary identity name is required".to_string(),
            ));
        }
        let mut index = self.load_index()?;
        let mut temp_entry = index
            .credentials
            .get(temp_identity_name)
            .cloned()
            .ok_or_else(|| {
                IdentityError::NotFound(format!("identity not found: {temp_identity_name}"))
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
                return Err(IdentityError::Conflict(format!(
                    "identity conflict: identity name {final_identity_name} is already used by another live identity"
                )));
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
        if !current_default.is_empty() {
            if current_default == temp_identity_name || archived_set.contains(&current_default) {
                index.default_credential_name = final_identity_name.to_string();
                default_updated = true;
            }
        }
        self.save_index(index)?;
        let identity = self.load(final_identity_name)?;
        Ok(RecoverPromotionResult {
            identity,
            default_updated,
        })
    }

    fn recover_backup_root(&self) -> PathBuf {
        Path::new(self.root_dir())
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(self.root_dir()))
            .join(LEGACY_BACKUP_DIR_NAME)
    }
}

fn same_handle_and_excluded(
    same_handle: &[IdentitySummary],
    excluded: &[IdentitySummary],
) -> Vec<IdentitySummary> {
    let mut existing = Vec::with_capacity(same_handle.len() + excluded.len());
    existing.extend_from_slice(same_handle);
    existing.extend_from_slice(excluded);
    existing
}

fn backup_preview_name(handle: &str) -> String {
    let mut handle_part = sanitize_identity_name(handle);
    if handle_part.is_empty() {
        handle_part = "handle".to_string();
    }
    format!("<timestamp>-{handle_part}")
}

fn backup_dir_name(created_at: OffsetDateTime, handle: &str) -> String {
    let mut handle_part = sanitize_identity_name(handle);
    if handle_part.is_empty() {
        handle_part = "handle".to_string();
    }
    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}.{:09}Z-{handle_part}",
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

fn recover_finalize_error(
    context: &str,
    err: IdentityError,
    request: &RecoverFinalizeRequest<'_>,
) -> RecoverFinalizeError {
    recover_finalize_error_message(format!("{context}: {err}"), request)
}

fn recover_finalize_error_message(
    message: String,
    request: &RecoverFinalizeRequest<'_>,
) -> RecoverFinalizeError {
    RecoverFinalizeError {
        err: IdentityError::Internal(message),
        backup_path: request.backup_path.to_string(),
        temp_identity_name: request.temp_identity_name.to_string(),
        new_did: request.new_did.to_string(),
    }
}

fn canonical_handle(raw: &str) -> String {
    raw.trim().to_ascii_lowercase()
}

fn compare_rfc3339(left: &str, right: &str) -> Ordering {
    let left = left.trim();
    let right = right.trim();
    match (left.is_empty(), right.is_empty()) {
        (true, true) => return Ordering::Equal,
        (true, false) => return Ordering::Greater,
        (false, true) => return Ordering::Less,
        (false, false) => {}
    }
    match (
        OffsetDateTime::parse(left, &Rfc3339),
        OffsetDateTime::parse(right, &Rfc3339),
    ) {
        (Ok(left_time), Ok(right_time)) => return left_time.cmp(&right_time),
        _ => {}
    }
    left.cmp(right)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compare_rfc3339_matches_go_empty_and_lexical_fallback() {
        assert_eq!(compare_rfc3339("2026-01-01T00:00:00Z", ""), Ordering::Less);
        assert_eq!(
            compare_rfc3339("", "2026-01-01T00:00:00Z"),
            Ordering::Greater
        );
        assert_eq!(compare_rfc3339("b", "a"), Ordering::Greater);
    }
}
