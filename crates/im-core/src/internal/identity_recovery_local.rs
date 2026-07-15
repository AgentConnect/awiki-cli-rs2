use serde::Serialize;
use serde_json::Value;
use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

const LEGACY_BACKUP_DIR_NAME: &str = ".legacy-backup";

#[derive(Debug, Clone)]
pub(crate) struct RecoverTarget {
    pub(crate) target_handle: crate::ids::Handle,
    pub(crate) target_local_part: String,
    pub(crate) effective_domain: String,
    pub(crate) explicit_domain: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct RecoverLocalPlan {
    pub(crate) target: RecoverTarget,
    pub(crate) final_identity_name: String,
    pub(crate) temp_identity_name: String,
    pub(crate) backup_path_preview: String,
    pub(crate) same_handle_candidates: Vec<crate::identity::RecoverLocalIdentitySummary>,
    pub(crate) excluded_identities: Vec<crate::identity::RecoverLocalIdentitySummary>,
}

#[derive(Debug, Clone)]
pub(crate) struct RecoverBackupResult {
    pub(crate) backup_path: String,
}

#[derive(Debug, Clone, Serialize)]
struct RecoverBackupManifest {
    reason: String,
    created_at: String,
    handle: String,
    archived_identity_names: Vec<String>,
    archived_dids: Vec<String>,
    archived_dir_names: Vec<String>,
    default_before: String,
    active_before: String,
    planned_final_identity: String,
    planned_temp_identity: String,
}

impl RecoverLocalPlan {
    pub(crate) fn stable_owner_identity_id(&self, active_before: &str) -> Option<String> {
        self.same_handle_candidates
            .iter()
            .find(|candidate| {
                !active_before.trim().is_empty()
                    && candidate.identity_name.trim() == active_before.trim()
            })
            .or_else(|| {
                self.same_handle_candidates
                    .iter()
                    .find(|candidate| candidate.is_default)
            })
            .or_else(|| self.same_handle_candidates.first())
            .map(|candidate| candidate.unique_id.trim().to_owned())
            .filter(|value| !value.is_empty())
    }

    pub(crate) fn archived_identity_names(&self) -> Vec<String> {
        self.same_handle_candidates
            .iter()
            .map(|summary| summary.identity_name.clone())
            .collect()
    }

    pub(crate) fn archived_dids(&self) -> Vec<String> {
        self.same_handle_candidates
            .iter()
            .filter_map(|summary| {
                let did = summary.did.trim();
                (!did.is_empty()).then(|| did.to_string())
            })
            .collect()
    }

    pub(crate) fn archived_dir_names(&self) -> Vec<String> {
        self.same_handle_candidates
            .iter()
            .map(|summary| summary.dir_name.clone())
            .collect()
    }

    pub(crate) fn old_owner_dids_in_merge_order(&self) -> Vec<String> {
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

    pub(crate) fn public_plan(
        &self,
        phone: &str,
        otp: Option<&str>,
    ) -> crate::identity::RecoverHandlePlan {
        let otp_present = otp.is_some_and(|value| !value.trim().is_empty());
        let (action, remote_calls, local_writes, backup_path) = if otp_present {
            (
                "recover_handle",
                vec!["did-auth.recover_handle".to_string()],
                Some(vec![
                    ".legacy-backup/recover-handle".to_string(),
                    "index.json".to_string(),
                    "config.yaml".to_string(),
                    "identity.json".to_string(),
                    "auth.json".to_string(),
                    "did_document.json".to_string(),
                    "key-1-private.pem".to_string(),
                    "key-1-public.pem".to_string(),
                    "e2ee-signing-private.pem".to_string(),
                    "e2ee-agreement-private.pem".to_string(),
                    "sqlite.recover_handle_merge".to_string(),
                    "sqlite.e2ee_cleanup".to_string(),
                ]),
                self.backup_path_preview.clone(),
            )
        } else {
            (
                "send_recover_otp",
                vec!["handle.send_otp".to_string()],
                None,
                String::new(),
            )
        };
        crate::identity::RecoverHandlePlan {
            action: action.to_string(),
            target_handle: self.target.target_handle.as_str().to_string(),
            identity_name: self.final_identity_name.clone(),
            final_identity_name: self.final_identity_name.clone(),
            temp_identity_name: self.temp_identity_name.clone(),
            same_handle_candidates: self.same_handle_candidates.clone(),
            excluded_identities: self.excluded_identities.clone(),
            backup_path,
            phone: phone.to_string(),
            remote_calls,
            local_writes,
        }
    }
}

pub(crate) fn plan_recover_handle(
    paths: &crate::paths::IdentityRegistryPaths,
    handle: &crate::ids::Handle,
    raw_handle: Option<&str>,
    did_domain: &str,
) -> crate::ImResult<RecoverLocalPlan> {
    let target = recover_target(raw_handle.unwrap_or_else(|| handle.as_str()), did_domain)?;
    let existing = local_identity_summaries(paths)?;
    let identity_base = if target.explicit_domain {
        target.target_handle.as_str().to_string()
    } else {
        target.target_local_part.clone()
    };
    let final_identity_name = sanitize_identity_name(&identity_base);
    if final_identity_name.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("handle".to_string()),
            format!(
                "handle {:?} cannot be used as an identity name",
                handle.as_str()
            ),
        ));
    }

    let handle_key = canonical_handle(target.target_handle.as_str());
    let mut same_handle_candidates = Vec::new();
    let mut excluded_identities = Vec::new();
    for summary in existing {
        let full_handle = default_string(
            &summary.full_handle,
            &derive_full_handle_from_did(&summary.handle, &summary.did),
        );
        if canonical_handle(&full_handle) == handle_key {
            same_handle_candidates.push(summary);
        } else {
            excluded_identities.push(summary);
        }
    }
    same_handle_candidates.sort_by(|left, right| {
        match compare_rfc3339(&left.created_at, &right.created_at) {
            Ordering::Equal => left.identity_name.cmp(&right.identity_name),
            ordering => ordering,
        }
    });
    for summary in &excluded_identities {
        if summary.identity_name == final_identity_name {
            return Err(crate::ImError::invalid_input(
                Some("identity".to_string()),
                format!(
                    "identity conflict: identity name {final_identity_name} is already used by another handle"
                ),
            ));
        }
    }

    let temp_base = format!("{final_identity_name}-recover-tmp");
    let mut all_existing =
        Vec::with_capacity(same_handle_candidates.len() + excluded_identities.len());
    all_existing.extend_from_slice(&same_handle_candidates);
    all_existing.extend_from_slice(&excluded_identities);
    let temp_identity_name = choose_named_identity(&temp_base, &all_existing, &temp_base);
    let backup_path_preview =
        preview_recover_handle_backup_path(paths, target.target_handle.as_str());

    Ok(RecoverLocalPlan {
        target,
        final_identity_name,
        temp_identity_name,
        backup_path_preview,
        same_handle_candidates,
        excluded_identities,
    })
}

pub(crate) fn create_recover_backup(
    paths: &crate::paths::IdentityRegistryPaths,
    plan: &RecoverLocalPlan,
    active_before: &str,
    config_file: Option<&Path>,
    secret_storage: crate::internal::identity_store::SaveIdentitySecretStorage,
) -> crate::ImResult<RecoverBackupResult> {
    fs::create_dir_all(&paths.identity_root_dir)?;
    set_private_dir_mode(&paths.identity_root_dir)?;
    let store = crate::internal::identity_store::IdentityStore::new(paths);
    let index = store.load_index()?;
    let created_at = OffsetDateTime::now_utc();
    let backup_root = recover_backup_root(paths).join("recover-handle");
    let backup_dir = unique_backup_dir(backup_root.join(backup_dir_name(
        created_at,
        plan.target.target_handle.as_str(),
    )));
    fs::create_dir_all(&backup_dir)?;
    set_private_dir_mode(&backup_dir)?;

    write_secure_json(&backup_dir.join("index.before.json"), &index)?;
    if let Some(config_file) = config_file.filter(|path| !path.as_os_str().is_empty()) {
        match fs::read(config_file) {
            Ok(raw) => {
                write_secure_text(
                    &backup_dir.join("config.before.yaml"),
                    &String::from_utf8_lossy(&raw),
                )?;
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err.into()),
        }
    }

    let archived_identity_names = plan.archived_identity_names();
    let archived_dids = plan.archived_dids();
    let archived_dir_names = plan.archived_dir_names();
    for (idx, summary) in plan.same_handle_candidates.iter().enumerate() {
        let source = paths.identity_root_dir.join(&summary.dir_name);
        let metadata = match fs::metadata(&source) {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Err(err.into()),
        };
        if !metadata.is_dir() {
            return Err(crate::ImError::invalid_input(
                Some("identity_dir".to_string()),
                format!("identity path is not a directory: {}", source.display()),
            ));
        }
        let mut target_name = format!(
            "{:02}-{}",
            idx + 1,
            sanitize_component(&summary.identity_name)
        );
        if target_name.trim().is_empty() {
            target_name = format!("{:02}-{}", idx + 1, sanitize_component(&summary.dir_name));
        }
        copy_dir(
            &source,
            &backup_dir.join("identities").join(target_name),
            secret_storage.writes_secrets_to_vault(),
        )?;
    }

    let manifest = RecoverBackupManifest {
        reason: "recover_handle".to_string(),
        created_at: created_at
            .format(&Rfc3339)
            .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string()),
        handle: plan.target.target_handle.as_str().to_string(),
        archived_identity_names,
        archived_dids,
        archived_dir_names,
        default_before: index.default_credential_name,
        active_before: active_before.trim().to_string(),
        planned_final_identity: plan.final_identity_name.clone(),
        planned_temp_identity: plan.temp_identity_name.clone(),
    };
    write_secure_json(&backup_dir.join("backup_manifest.json"), &manifest)?;
    Ok(RecoverBackupResult {
        backup_path: backup_dir.to_string_lossy().into_owned(),
    })
}

pub(crate) fn update_active_identity_in_config(
    config_file: &Path,
    final_identity_name: &str,
) -> crate::ImResult<bool> {
    if config_file.as_os_str().is_empty() {
        return Ok(false);
    }
    let mut text = match fs::read_to_string(config_file) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err.into()),
    };
    let mut lines = text
        .lines()
        .map(ToString::to_string)
        .collect::<Vec<String>>();
    let mut identity_idx = None;
    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if !line.starts_with(' ') && trimmed == "identity:" {
            identity_idx = Some(idx);
            break;
        }
    }
    if let Some(identity_idx) = identity_idx {
        let mut insert_at = identity_idx + 1;
        let mut active_idx = None;
        while insert_at < lines.len() {
            let line = &lines[insert_at];
            if !line.trim().is_empty() && !line.starts_with(' ') {
                break;
            }
            if line.trim_start().starts_with("active:") {
                active_idx = Some(insert_at);
                break;
            }
            insert_at += 1;
        }
        if let Some(active_idx) = active_idx {
            lines[active_idx] = format!("  active: {final_identity_name}");
        } else {
            lines.insert(insert_at, format!("  active: {final_identity_name}"));
        }
        text = lines.join("\n");
        text.push('\n');
    } else {
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str("identity:\n");
        text.push_str(&format!("  active: {final_identity_name}\n"));
    }
    fs::write(config_file, text)?;
    set_private_file_mode(config_file)?;
    Ok(true)
}

pub(crate) fn local_identity_summary_from_stored(
    stored: &crate::internal::identity_store::StoredIdentity,
) -> crate::identity::RecoverLocalIdentitySummary {
    let mut summary = crate::identity::RecoverLocalIdentitySummary {
        identity_name: stored.local_alias.clone(),
        did: stored.did.as_str().to_string(),
        unique_id: stored.unique_id.clone(),
        user_id: stored.user_id.clone(),
        display_name: stored.display_name.clone(),
        handle: stored.handle.clone(),
        full_handle: stored.full_handle.clone(),
        created_at: stored.created_at.clone(),
        dir_name: stored.dir_name.clone(),
        is_default: stored.is_default,
        has_jwt: !stored.jwt_token.trim().is_empty(),
        has_did_document: stored.has_did_document,
        has_key1_private: stored.has_key1_private,
        has_key1_public: stored.has_key1_public,
        has_e2ee_signing_private: stored.has_e2ee_signing_private,
        has_e2ee_agreement_private: stored.has_e2ee_agreement_private,
        user_state: crate::identity::RecoverLocalUserState::default(),
    };
    summary.user_state = evaluate_identity_summary_user_state(&summary);
    summary
}

pub(crate) fn identity_summary_from_generated(
    generated: &crate::internal::identity_generation::GeneratedIdentity,
    raw: &Value,
    target: &RecoverTarget,
    alias: &str,
) -> crate::ImResult<crate::identity::IdentitySummary> {
    let did = raw
        .get("did")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| generated.did.as_str());
    let full_handle = raw
        .get("full_handle")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| target.target_handle.as_str());
    Ok(crate::identity::IdentitySummary {
        id: crate::ids::IdentityId::parse(&generated.unique_id)?,
        did: crate::ids::Did::parse(did)?,
        handle: Some(crate::ids::Handle::parse(full_handle, "")?),
        display_name: Some(string_value(raw, "handle", &target.target_local_part))
            .filter(|value| !value.trim().is_empty()),
        local_alias: Some(alias.to_string()),
        device_id: None,
        is_default: false,
        readiness: crate::identity::IdentityReadiness {
            ready_for_auth: raw
                .get("access_token")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.trim().is_empty()),
            ready_for_messaging: raw
                .get("user_id")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.trim().is_empty()),
            missing: Vec::new(),
        },
    })
}

pub(crate) fn recover_target(raw: &str, did_domain: &str) -> crate::ImResult<RecoverTarget> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("handle".to_string()),
            "handle is required",
        ));
    }
    let lower = trimmed.trim_start_matches('@').to_ascii_lowercase();
    if lower.starts_with("did:") {
        return Err(crate::ImError::invalid_input(
            Some("handle".to_string()),
            "DID values are not supported in handle input",
        ));
    }
    let handle = lower.strip_prefix("wba://").unwrap_or(&lower);
    let (local_part, domain, explicit_domain) = if let Some(dot) = handle.find('.') {
        (
            handle[..dot].trim().to_string(),
            handle[dot + 1..].trim().trim_end_matches('.').to_string(),
            true,
        )
    } else {
        (
            handle.to_string(),
            did_domain.trim().trim_end_matches('.').to_ascii_lowercase(),
            false,
        )
    };
    if local_part.is_empty() || domain.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("handle".to_string()),
            "handle domain and local part are required",
        ));
    }
    Ok(RecoverTarget {
        target_handle: crate::ids::Handle::parse(format!("{local_part}.{domain}"), "")?,
        target_local_part: local_part,
        effective_domain: domain,
        explicit_domain,
    })
}

pub(crate) fn string_value(raw: &Value, key: &str, fallback: &str) -> String {
    raw.get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| fallback.to_string())
}

fn local_identity_summaries(
    paths: &crate::paths::IdentityRegistryPaths,
) -> crate::ImResult<Vec<crate::identity::RecoverLocalIdentitySummary>> {
    let store = crate::internal::identity_store::IdentityStore::new(paths);
    let index = store.load_index()?;
    let default_name = index.default_credential_name.trim().to_string();
    let mut summaries = Vec::with_capacity(index.credentials.len());
    for (name, entry) in index.credentials {
        let did = entry.did.trim();
        let unique_id = default_string(&entry.unique_id, &name);
        let full_handle = default_string(
            &entry.full_handle,
            &derive_full_handle_from_did(&entry.handle, did),
        );
        let handle = if entry.handle.trim().is_empty() {
            full_handle
                .split_once('.')
                .map(|(local, _)| local)
                .unwrap_or(full_handle.as_str())
                .to_string()
        } else {
            entry.handle.clone()
        };
        let mut summary = crate::identity::RecoverLocalIdentitySummary {
            identity_name: name.clone(),
            did: did.to_string(),
            unique_id,
            display_name: entry.name.clone(),
            handle,
            full_handle,
            created_at: entry.created_at.clone(),
            dir_name: default_string(&entry.dir_name, &default_string(&entry.unique_id, &name)),
            is_default: entry.is_default || (!default_name.is_empty() && default_name == name),
            has_jwt: true,
            has_did_document: true,
            has_key1_private: true,
            has_key1_public: true,
            has_e2ee_signing_private: true,
            has_e2ee_agreement_private: true,
            user_state: crate::identity::RecoverLocalUserState::default(),
            user_id: entry.user_id.clone(),
        };
        summary.user_state = evaluate_identity_summary_user_state(&summary);
        summaries.push(summary);
    }
    Ok(summaries)
}

fn preview_recover_handle_backup_path(
    paths: &crate::paths::IdentityRegistryPaths,
    handle: &str,
) -> String {
    recover_backup_root(paths)
        .join("recover-handle")
        .join(backup_preview_name(handle))
        .to_string_lossy()
        .into_owned()
}

fn recover_backup_root(paths: &crate::paths::IdentityRegistryPaths) -> PathBuf {
    paths
        .identity_root_dir
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| paths.identity_root_dir.clone())
        .join(LEGACY_BACKUP_DIR_NAME)
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

fn copy_dir(source: &Path, target: &Path, omit_secret_files: bool) -> crate::ImResult<()> {
    fs::create_dir_all(target)?;
    set_private_dir_mode(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            copy_dir(&source_path, &target_path, omit_secret_files)?;
        } else if metadata.is_file() {
            if omit_secret_files && is_identity_secret_file(&source_path) {
                continue;
            }
            fs::copy(&source_path, &target_path)?;
            set_private_file_mode(&target_path)?;
        }
    }
    Ok(())
}

fn is_identity_secret_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    matches!(
        name,
        "auth.json"
            | "private.key"
            | "key-1-private.pem"
            | "key-3-private.pem"
            | "e2ee-signing-private.pem"
            | "e2ee-agreement-private.pem"
            | "daemon-key-1-private.pem"
            | "daemon-subkey-package.json"
    )
}

fn write_secure_json(path: &Path, payload: &impl Serialize) -> crate::ImResult<()> {
    let raw = serde_json::to_vec_pretty(payload).map_err(|err| crate::ImError::Serialization {
        detail: err.to_string(),
    })?;
    fs::write(path, raw)?;
    set_private_file_mode(path)?;
    Ok(())
}

fn write_secure_text(path: &Path, payload: &str) -> crate::ImResult<()> {
    fs::write(path, payload)?;
    set_private_file_mode(path)?;
    Ok(())
}

fn evaluate_identity_summary_user_state(
    summary: &crate::identity::RecoverLocalIdentitySummary,
) -> crate::identity::RecoverLocalUserState {
    let mut missing = Vec::new();
    if summary.user_id.trim().is_empty() {
        missing.push("registration".to_string());
    }
    if summary.handle.trim().is_empty() {
        missing.push("handle".to_string());
    }
    let registration_state = match missing.len() {
        0 => "registered_user",
        1 => "partial_user",
        _ => "local_identity",
    };
    crate::identity::RecoverLocalUserState {
        registration_state: registration_state.to_string(),
        ready_for_messaging: missing.is_empty(),
        missing,
    }
}

fn choose_named_identity(
    requested: &str,
    existing: &[crate::identity::RecoverLocalIdentitySummary],
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

fn canonical_handle(raw: &str) -> String {
    raw.trim().to_ascii_lowercase()
}

fn default_string(value: &str, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
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

fn compare_rfc3339(left: &str, right: &str) -> Ordering {
    let left = left.trim();
    let right = right.trim();
    match (left.is_empty(), right.is_empty()) {
        (true, true) => return Ordering::Equal,
        (true, false) => return Ordering::Greater,
        (false, true) => return Ordering::Less,
        (false, false) => {}
    }
    if let (Ok(left_time), Ok(right_time)) = (
        OffsetDateTime::parse(left, &Rfc3339),
        OffsetDateTime::parse(right, &Rfc3339),
    ) {
        return left_time.cmp(&right_time);
    }
    left.cmp(right)
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
    fn recover_target_normalizes_bare_handle() {
        let target = recover_target(" Alice ", "Tenant.Example.").unwrap();
        assert_eq!(target.target_handle.as_str(), "alice.tenant.example");
        assert_eq!(target.target_local_part, "alice");
        assert_eq!(target.effective_domain, "tenant.example");
        assert!(!target.explicit_domain);
    }

    #[test]
    fn stable_owner_identity_prefers_active_same_handle_persona() {
        let summary =
            |name: &str, id: &str, is_default: bool| crate::identity::RecoverLocalIdentitySummary {
                identity_name: name.to_owned(),
                unique_id: id.to_owned(),
                is_default,
                ..Default::default()
            };
        let plan = RecoverLocalPlan {
            target: recover_target("alice", "awiki.test").unwrap(),
            final_identity_name: "alice".to_owned(),
            temp_identity_name: "alice-recovering".to_owned(),
            backup_path_preview: String::new(),
            same_handle_candidates: vec![
                summary("alice-old", "owner-active", false),
                summary("alice-default", "owner-default", true),
            ],
            excluded_identities: vec![summary("bob", "other-persona", false)],
        };
        assert_eq!(
            plan.stable_owner_identity_id("alice-old").as_deref(),
            Some("owner-active")
        );
        assert_eq!(
            plan.stable_owner_identity_id("missing").as_deref(),
            Some("owner-default")
        );
    }

    #[test]
    fn recover_backup_omits_secret_files_when_vault_storage_is_used() {
        let root = tempfile::tempdir().unwrap();
        let paths = test_paths(root.path());
        let identities = &paths.identity_root_dir;
        std::fs::create_dir_all(identities.join("alice-id")).unwrap();
        std::fs::write(
            &paths.registry_path,
            serde_json::to_vec_pretty(&serde_json::json!({
                "schema_version": 3,
                "default_credential_name": "alice",
                "credentials": {
                    "alice": {
                        "credential_name": "alice",
                        "dir_name": "alice-id",
                        "did": "did:wba:awiki.test:alice:e1_old",
                        "unique_id": "alice-id",
                        "user_id": "user-alice",
                        "name": "Alice",
                        "handle": "alice",
                        "full_handle": "alice.awiki.test",
                        "created_at": "2026-05-21T00:00:00Z",
                        "is_default": true
                    }
                }
            }))
            .unwrap(),
        )
        .unwrap();
        for (name, text) in [
            (
                "identity.json",
                r#"{"did":"did:wba:awiki.test:alice:e1_old"}"#,
            ),
            (
                "did_document.json",
                r#"{"id":"did:wba:awiki.test:alice:e1_old"}"#,
            ),
            ("auth.json", r#"{"jwt_token":"test-token-for-alice"}"#),
            ("private.key", "test-private-key-for-alice"),
            ("key-1-private.pem", "-----BEGIN PRIVATE KEY-----\nsecret\n"),
            (
                "daemon-subkey-package.json",
                r#"{"private_key_pem":"daemon-secret"}"#,
            ),
        ] {
            std::fs::write(identities.join("alice-id").join(name), text).unwrap();
        }
        let plan = plan_recover_handle(
            &paths,
            &crate::ids::Handle::parse("alice.awiki.test", "").unwrap(),
            None,
            "awiki.test",
        )
        .unwrap();

        let backup = create_recover_backup(
            &paths,
            &plan,
            "alice",
            None,
            crate::internal::identity_store::SaveIdentitySecretStorage::Vault {
                workspace_id: "workspace-a".to_string(),
                device_id: "device-a".to_string(),
                vault: std::sync::Arc::new(crate::internal::secret_vault::FileSecretVault::new(
                    crate::internal::platform_secret::DeviceVaultRootKey::from_bytes([16_u8; 32]),
                    crate::internal::secret_vault::FileSecretVaultStore::new(
                        root.path().join("vault"),
                    ),
                )),
            },
        )
        .unwrap();

        let backup_text = collect_text_files(std::path::Path::new(&backup.backup_path));
        assert!(backup_text.contains("did:wba:awiki.test:alice:e1_old"));
        for marker in [
            "test-token-for-alice",
            "test-private-key-for-alice",
            "-----BEGIN PRIVATE KEY-----",
            "private_key_pem",
            "daemon-secret",
        ] {
            assert!(!backup_text.contains(marker), "backup leaked {marker}");
        }
    }

    fn test_paths(root: &Path) -> crate::paths::IdentityRegistryPaths {
        crate::paths::IdentityRegistryPaths {
            identity_root_dir: root.join("identities"),
            registry_path: root.join("identities").join("registry.json"),
            default_identity_path: Some(root.join("identities").join("default")),
        }
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
                if let Ok(text) = std::fs::read_to_string(&path) {
                    out.push_str(&text);
                    out.push('\n');
                }
            }
        }
    }
}
