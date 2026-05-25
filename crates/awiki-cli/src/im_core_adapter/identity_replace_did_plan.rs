// Advanced replace-did dry-run planning adapter.
// Non-dry-run execution is intentionally unsupported until im-core exposes a
// stable public execution API that owns the local mutation boundary.

use im_core::identity::{ReplaceDidAffectedLocalState, ReplaceDidPlan, ReplaceDidPlanRequest};
use im_core::prelude::{Did, IdentitySelector};
use serde_json::json;
use std::path::Path;

use crate::legacy_identity as identity;
use crate::output::ExitError;

#[derive(Debug, Clone)]
pub struct ReplaceDidPlanCommandRequest {
    pub sdk: ReplaceDidPlanRequest,
    pub identity_name: String,
}

pub fn replace_did_plan_via_im_core(
    resolved: &crate::config::Resolved,
    identity_name: &str,
    is_public: Option<bool>,
    is_agent: Option<bool>,
    role: Option<&str>,
    endpoint_url: Option<&str>,
) -> Result<identity::CommandResult, ExitError> {
    let request = replace_did_plan_command_request(
        resolved,
        identity_name,
        is_public,
        is_agent,
        role,
        endpoint_url,
    )?;
    let client = super::build_im_client(
        resolved,
        IdentitySelector::LocalAlias(request.identity_name.clone()),
    )?;
    let plan = client
        .identity()
        .replace_did_plan(request.sdk)
        .map_err(|err| super::map_im_error(err, "id replace-did"))?;
    replace_did_plan_command_result(plan)
}

pub fn replace_did_plan_command_request(
    resolved: &crate::config::Resolved,
    identity_name: &str,
    is_public: Option<bool>,
    is_agent: Option<bool>,
    role: Option<&str>,
    endpoint_url: Option<&str>,
) -> Result<ReplaceDidPlanCommandRequest, ExitError> {
    build_replace_did_plan_command_request(
        resolved,
        identity_name,
        None,
        is_public,
        is_agent,
        role,
        endpoint_url,
    )
}

fn build_replace_did_plan_command_request(
    resolved: &crate::config::Resolved,
    identity_name: &str,
    planned_new_did_override: Option<String>,
    is_public: Option<bool>,
    is_agent: Option<bool>,
    role: Option<&str>,
    endpoint_url: Option<&str>,
) -> Result<ReplaceDidPlanCommandRequest, ExitError> {
    let core = super::build_im_core(resolved)?;
    let summary = core
        .identities()
        .resolve(super::cli_identity_selector(identity_name))
        .map_err(|err| super::map_im_error(err, "id replace-did"))?;
    let planned_new_did = match planned_new_did_override {
        Some(value) => value,
        None => planned_replace_did(&summary)?,
    };
    let sdk = ReplaceDidPlanRequest {
        identity: summary.clone(),
        linked_identity_names: linked_identity_names(&summary),
        planned_new_did: Did::parse(&planned_new_did)
            .map_err(|err| super::map_im_error(err, "id replace-did"))?,
        backup_path_preview: replace_did_backup_path_preview(resolved, &summary),
        old_dir_name: identity_dir_name(&summary),
        is_public,
        is_agent,
        role: role.map(str::to_string),
        endpoint_url: endpoint_url.map(str::to_string),
        affected_local_state: ReplaceDidAffectedLocalState::default(),
    };
    Ok(ReplaceDidPlanCommandRequest {
        sdk,
        identity_name: identity_name_from_summary(&summary),
    })
}

fn replace_did_plan_command_result(
    plan: ReplaceDidPlan,
) -> Result<identity::CommandResult, ExitError> {
    let value = serde_json::to_value(&plan).map_err(|err| {
        ExitError::new(
            "serialization_error",
            1,
            format!("serialize replace DID plan: {err}"),
            "Report this issue with the command output.",
        )
    })?;
    Ok(identity::CommandResult {
        data: json!({
            "plan": value,
        }),
        summary: "Dry run: DID replacement planned".to_string(),
        warnings: vec![identity::replace_did_danger_warning().to_string()],
    })
}

fn planned_replace_did(summary: &im_core::IdentitySummary) -> Result<String, ExitError> {
    let suffix = summary
        .id
        .as_str()
        .trim()
        .strip_prefix("e1_")
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| summary.id.as_str().trim());
    let suffix = if suffix.is_empty() { "planned" } else { suffix };
    let base = summary
        .did
        .as_str()
        .rsplit_once(':')
        .map(|(base, _)| base)
        .filter(|base| !base.trim().is_empty())
        .ok_or_else(|| {
            ExitError::new(
                "invalid_argument",
                2,
                format!("invalid current DID {:?}.", summary.did.as_str()),
                "Use a handle-backed did:wba identity before replacing DID.",
            )
        })?;
    Ok(format!("{base}:e1_replacement_{suffix}"))
}

fn replace_did_backup_path_preview(
    resolved: &crate::config::Resolved,
    summary: &im_core::IdentitySummary,
) -> String {
    let identity_name = sanitize_component(&identity_name_from_summary(summary));
    let dir_name = sanitize_component(&identity_dir_name(summary));
    Path::new(&resolved.paths.identity_dir)
        .join(identity::types::LEGACY_BACKUP_DIR_NAME)
        .join("replace-did")
        .join(format!("<timestamp>-{identity_name}-{dir_name}"))
        .to_string_lossy()
        .into_owned()
}

fn linked_identity_names(summary: &im_core::IdentitySummary) -> Vec<String> {
    let mut names = vec![identity_name_from_summary(summary)];
    names.sort();
    names.dedup();
    names
}

fn identity_name_from_summary(summary: &im_core::IdentitySummary) -> String {
    summary
        .local_alias
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| summary.id.as_str())
        .to_string()
}

fn identity_dir_name(summary: &im_core::IdentitySummary) -> String {
    identity_name_from_summary(summary)
}

fn sanitize_component(raw: &str) -> String {
    raw.trim()
        .to_ascii_lowercase()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches(['.', '_', '-'])
        .to_string()
}
