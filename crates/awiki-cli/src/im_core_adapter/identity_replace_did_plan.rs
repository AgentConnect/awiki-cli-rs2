// Advanced replace-did dry-run planning adapter.
// Non-dry-run execution is intentionally unsupported until im-core exposes a
// stable public execution API that owns the local mutation boundary.

use im_core::prelude::{
    Did, Handle, IdentityId, IdentitySelector, ReplaceDidAffectedLocalState, ReplaceDidPlan,
    ReplaceDidPlanRequest,
};
use serde_json::json;

use crate::identity;
use crate::output::ExitError;
use crate::store;

#[derive(Debug, Clone)]
pub struct ReplaceDidPlanBridgeRequest {
    pub sdk: ReplaceDidPlanRequest,
    pub identity_name: String,
}

pub fn replace_did_plan_via_im_core(
    resolved: &crate::config::Resolved,
    manager: &identity::Manager,
    identity_name: &str,
    is_public: Option<bool>,
    is_agent: Option<bool>,
    role: Option<&str>,
    endpoint_url: Option<&str>,
) -> Result<identity::CommandResult, ExitError> {
    let bridge = replace_did_plan_bridge_request(
        resolved,
        manager,
        identity_name,
        is_public,
        is_agent,
        role,
        endpoint_url,
    )?;
    let client = super::build_im_client(
        resolved,
        manager,
        IdentitySelector::LocalAlias(bridge.identity_name.clone()),
    )?;
    let plan = client
        .identity()
        .replace_did_plan(bridge.sdk)
        .map_err(|err| super::map_im_error(err, "id replace-did"))?;
    replace_did_plan_command_result(plan)
}

pub fn replace_did_plan_bridge_request(
    resolved: &crate::config::Resolved,
    manager: &identity::Manager,
    identity_name: &str,
    is_public: Option<bool>,
    is_agent: Option<bool>,
    role: Option<&str>,
    endpoint_url: Option<&str>,
) -> Result<ReplaceDidPlanBridgeRequest, ExitError> {
    build_replace_did_plan_bridge_request(
        resolved,
        manager,
        identity_name,
        None,
        is_public,
        is_agent,
        role,
        endpoint_url,
    )
}

fn build_replace_did_plan_bridge_request(
    resolved: &crate::config::Resolved,
    manager: &identity::Manager,
    identity_name: &str,
    planned_new_did_override: Option<String>,
    is_public: Option<bool>,
    is_agent: Option<bool>,
    role: Option<&str>,
    endpoint_url: Option<&str>,
) -> Result<ReplaceDidPlanBridgeRequest, ExitError> {
    let record = identity::service::load_identity_for_mutation(resolved, manager, identity_name)
        .map_err(crate::app::identity_exit)?;
    let planned_new_did = match planned_new_did_override {
        Some(value) => value,
        None => planned_replace_did(&record)?,
    };
    let (store_rebind_counts, e2ee_cleanup_counts) =
        store::plan_rebind_local_identity_state(&resolved.paths, &record.did, &planned_new_did)
            .map_err(|err| super::map_im_error(store_error_to_im_error(err), "id replace-did"))?;
    let summary = identity::store::identity_summary_from_record(&record);
    let sdk = ReplaceDidPlanRequest {
        identity: sdk_identity_summary(&summary)?,
        linked_identity_names: linked_identity_names(manager, &record)?,
        planned_new_did: Did::parse(&planned_new_did)
            .map_err(|err| super::map_im_error(err, "id replace-did"))?,
        backup_path_preview: replace_did_backup_path_preview(manager, &record),
        old_dir_name: record.dir_name.clone(),
        is_public,
        is_agent,
        role: role.map(str::to_string),
        endpoint_url: endpoint_url.map(str::to_string),
        affected_local_state: ReplaceDidAffectedLocalState {
            store_rebind_counts,
            e2ee_cleanup_counts,
        },
    };
    Ok(ReplaceDidPlanBridgeRequest {
        sdk,
        identity_name: record.identity_name,
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

fn sdk_identity_summary(
    summary: &identity::IdentitySummary,
) -> Result<im_core::IdentitySummary, ExitError> {
    Ok(im_core::IdentitySummary {
        id: IdentityId::parse(first_non_empty([
            &summary.unique_id,
            &summary.identity_name,
            &summary.dir_name,
        ]))
        .map_err(|err| super::map_im_error(err, "id replace-did"))?,
        did: Did::parse(&summary.did).map_err(|err| super::map_im_error(err, "id replace-did"))?,
        handle: trimmed_optional(&summary.full_handle)
            .map(|handle| {
                Handle::parse(handle, "").map_err(|err| super::map_im_error(err, "id replace-did"))
            })
            .transpose()?,
        display_name: trimmed_optional(&summary.display_name),
        local_alias: trimmed_optional(&summary.identity_name),
        device_id: None,
        is_default: summary.is_default,
        readiness: im_core::identity::IdentityReadiness {
            ready_for_auth: summary.has_did_document
                && summary.has_key1_private
                && summary.has_key1_public,
            ready_for_messaging: summary.user_state.ready_for_messaging,
            missing: summary
                .user_state
                .missing
                .iter()
                .map(|item| match item.as_str() {
                    "handle" => im_core::identity::IdentityMissingItem::Handle,
                    "registration" => {
                        im_core::identity::IdentityMissingItem::Other("registration".to_string())
                    }
                    other => im_core::identity::IdentityMissingItem::Other(other.to_string()),
                })
                .collect(),
        },
    })
}

fn planned_replace_did(record: &identity::types::StoredIdentity) -> Result<String, ExitError> {
    let suffix = record
        .unique_id
        .trim()
        .strip_prefix("e1_")
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| record.unique_id.trim());
    let suffix = if suffix.is_empty() { "planned" } else { suffix };
    let base = record
        .did
        .rsplit_once(':')
        .map(|(base, _)| base)
        .filter(|base| !base.trim().is_empty())
        .ok_or_else(|| {
            ExitError::new(
                "invalid_argument",
                2,
                format!("invalid current DID {:?}.", record.did),
                "Use a handle-backed did:wba identity before replacing DID.",
            )
        })?;
    Ok(format!("{base}:e1_replacement_{suffix}"))
}

fn replace_did_backup_path_preview(
    manager: &identity::Manager,
    record: &identity::types::StoredIdentity,
) -> String {
    let identity_name = sanitize_component(&record.identity_name);
    let dir_name = sanitize_component(&record.dir_name);
    std::path::Path::new(manager.root_dir())
        .join(identity::types::LEGACY_BACKUP_DIR_NAME)
        .join("replace-did")
        .join(format!("<timestamp>-{identity_name}-{dir_name}"))
        .to_string_lossy()
        .into_owned()
}

fn linked_identity_names(
    manager: &identity::Manager,
    record: &identity::types::StoredIdentity,
) -> Result<Vec<String>, ExitError> {
    let index = manager.load_index().map_err(crate::app::identity_exit)?;
    let mut names = index
        .credentials
        .iter()
        .filter_map(|(name, entry)| {
            (entry.dir_name == record.dir_name || entry.did == record.did).then(|| name.clone())
        })
        .collect::<Vec<_>>();
    if names.is_empty() {
        names.push(record.identity_name.clone());
    }
    names.sort();
    Ok(names)
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

fn first_non_empty<const N: usize>(values: [&str; N]) -> String {
    values
        .into_iter()
        .find(|value| !value.trim().is_empty())
        .unwrap_or("identity")
        .to_string()
}

fn trimmed_optional(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn store_error_to_im_error(err: store::StoreError) -> im_core::ImError {
    match err {
        store::StoreError::Invalid(message) => im_core::ImError::invalid_input(None, message),
        store::StoreError::NotFound(message) => {
            im_core::ImError::LocalStateUnavailable { detail: message }
        }
        err => im_core::ImError::LocalStateUnavailable {
            detail: err.to_string(),
        },
    }
}
