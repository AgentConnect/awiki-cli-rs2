use super::{store_exit, App};
use crate::cli::ParsedCommand;
use crate::identity::{self, RecoverFinalizeRequest};
use crate::output::ExitError;
use crate::store;
use serde_json::{json, Value};

const RECOVER_REPAIR_HINT: &str =
    "Inspect the returned backup path and temporary identity, then repair the local workspace state before retrying.";

impl App {
    pub fn run_id_recover(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let handle = required_string_flag(
            command,
            "handle",
            "id recover",
            "Usage: awiki-cli id recover --handle <handle> --phone <phone> [--otp <code>]",
        )?;
        let phone = required_string_flag(
            command,
            "phone",
            "id recover",
            "Usage: awiki-cli id recover --handle <handle> --phone <phone> [--otp <code>]",
        )?;
        let params = identity::RecoverParams {
            identity_name: self.globals.identity.clone(),
            handle,
            phone,
            otp: string_flag(command, "otp"),
        };
        let manager = self.identity_manager(&resolved);
        let mut result = if self.globals.dry_run {
            crate::im_core_adapter::identity::recover_handle_plan_via_im_core(
                &manager,
                &resolved.did_domain,
                params,
            )
        } else {
            crate::im_core_adapter::identity::recover_handle_via_im_core(
                &resolved, &manager, params,
            )
        }?;

        if result.data.get("action").and_then(Value::as_str) != Some("recover_handle") {
            if self.globals.identity_changed {
                result
                    .warnings
                    .push(identity::recover_identity_ignored_warning().to_string());
            }
            return self.render_identity_result("awiki-cli id recover", &resolved, result);
        }

        let final_identity_name = string_from_data(&result.data, "final_identity_name");
        let temp_identity_name = string_from_data(&result.data, "temp_identity_name");
        let backup_path = string_from_data(&result.data, "backup_path");
        let active_before = string_from_data(&result.data, "active_before");
        let old_dids = string_slice_from_data(&result.data, "old_dids");
        let archived_identities = string_slice_from_data(&result.data, "archived_identities");
        let new_did = result
            .data
            .get("identity")
            .and_then(|value| value.get("did"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();

        let merge_result =
            crate::im_core_adapter::identity::merge_recovered_handle_local_state_via_im_core(
                &resolved.paths,
                old_dids.clone(),
                new_did.clone(),
                final_identity_name.clone(),
            )
            .map_err(|err| recover_store_exit(err, &backup_path, &temp_identity_name, &new_did))?;
        let (store_merge_counts, e2ee_cleanup_counts) = (
            merge_result.store_merge_counts,
            merge_result.e2ee_cleanup_counts,
        );

        let promoted = identity::finalize_recovered_handle(
            &manager,
            RecoverFinalizeRequest {
                final_identity_name: &final_identity_name,
                temp_identity_name: &temp_identity_name,
                archived_identity_names: &archived_identities,
                active_before: &active_before,
                backup_path: &backup_path,
                new_did: &new_did,
                config_paths: Some(&resolved.paths),
            },
        )
        .map_err(recover_finalize_exit)?;

        let mut data = result.data;
        if let Some(object) = data.as_object_mut() {
            let identity = manager
                .list()
                .ok()
                .and_then(|items| {
                    items
                        .into_iter()
                        .find(|summary| summary.identity_name == promoted.identity.identity_name)
                })
                .map(|summary| serde_json::to_value(summary).unwrap_or_else(|_| json!({})))
                .unwrap_or_else(|| {
                    json!({
                        "identity_name": promoted.identity.identity_name,
                        "did": promoted.identity.did,
                        "handle": promoted.identity.handle,
                        "full_handle": promoted.identity.full_handle,
                        "created_at": promoted.identity.created_at,
                    })
                });
            object.insert("identity".to_string(), identity);
            object.insert(
                "store_merge_counts".to_string(),
                serde_json::to_value(store_merge_counts).unwrap_or_else(|_| json!({})),
            );
            object.insert(
                "e2ee_cleanup_counts".to_string(),
                serde_json::to_value(e2ee_cleanup_counts).unwrap_or_else(|_| json!({})),
            );
            object.remove("temp_identity_name");
            object.remove("active_before");
            object.remove("old_dids");
        }
        if !archived_identities.is_empty() {
            result.warnings.push(format!(
                "Archived {} same-handle local identities; they were removed from the live index, while their original directories and the recover backup were kept.",
                archived_identities.len()
            ));
        }
        if self.globals.identity_changed {
            result
                .warnings
                .push(identity::recover_identity_ignored_warning().to_string());
        }
        self.render_success(
            "awiki-cli id recover",
            &resolved,
            identity::sanitize_public_value(data),
            &result.summary,
            result.warnings,
        )
    }
}

fn recover_store_exit(
    err: store::StoreError,
    backup_path: &str,
    temp_identity_name: &str,
    new_did: &str,
) -> ExitError {
    let mut exit = store_exit(err, RECOVER_REPAIR_HINT);
    exit.detail.code = "internal_error".to_string();
    exit.exit_code = 1;
    exit.detail.message = format!(
        "merge recovered handle local state: {}",
        exit.detail.message
    );
    exit.detail.details = recover_error_details(backup_path, temp_identity_name, new_did);
    exit
}

fn recover_finalize_exit(err: identity::RecoverFinalizeError) -> ExitError {
    let details = recover_error_details(&err.backup_path, &err.temp_identity_name, &err.new_did);
    let mut exit = ExitError::new("internal_error", 1, err.to_string(), RECOVER_REPAIR_HINT);
    exit.detail.details = details;
    exit
}

fn recover_error_details(backup_path: &str, temp_identity_name: &str, new_did: &str) -> Value {
    json!({
        "backup_path": backup_path,
        "temp_identity_name": temp_identity_name,
        "new_did": new_did,
    })
}

fn string_from_data(data: &Value, key: &str) -> String {
    data.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn string_slice_from_data(data: &Value, key: &str) -> Vec<String> {
    data.get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn string_flag(command: &ParsedCommand, name: &str) -> String {
    command.flags.get(name).cloned().unwrap_or_default()
}

fn required_string_flag(
    command: &ParsedCommand,
    name: &str,
    command_name: &str,
    help: &str,
) -> Result<String, ExitError> {
    let value = string_flag(command, name);
    if value.trim().is_empty() {
        return Err(ExitError::new(
            "invalid_argument",
            2,
            format!("{command_name} requires --{name}."),
            help,
        ));
    }
    Ok(value)
}
