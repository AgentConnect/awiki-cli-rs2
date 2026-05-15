use super::handle_input::{default_string, derive_full_handle_from_did, normalize_handle_input};
use super::layout::{sanitize_identity_name, Manager};
use super::store::choose_named_identity;
use super::types::{IdentityError, IdentitySummary, RecoverParams, LEGACY_BACKUP_DIR_NAME};
use super::CommandResult;
use serde_json::{json, Value};
use std::cmp::Ordering;
use std::path::Path;
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
        Path::new(self.root_dir())
            .join(LEGACY_BACKUP_DIR_NAME)
            .join("recover-handle")
            .join(backup_preview_name(handle))
            .to_string_lossy()
            .into_owned()
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
