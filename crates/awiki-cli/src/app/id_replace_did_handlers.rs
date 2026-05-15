use super::{changed_string_flag, identity_exit, optional_bool_flag, App};
use crate::cli::ParsedCommand;
use crate::identity;
use crate::output::ExitError;
use crate::store;
use serde_json::json;

const REPLACE_DID_HINT: &str =
    "Use a handle-backed identity with valid DID credentials before retrying.";

impl App {
    pub fn run_id_replace_did(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let is_public = optional_bool_flag(command, "is-public")?;
        let is_agent = optional_bool_flag(command, "is-agent")?;
        let role = changed_string_flag(command, "role");
        let endpoint_url = changed_string_flag(command, "endpoint-url");
        if self.globals.dry_run {
            let result = identity::replace_did_plan(
                &self.globals.identity,
                is_public,
                is_agent,
                role.as_deref(),
                endpoint_url.as_deref(),
            );
            return self.render_identity_result("awiki-cli id replace-did", &resolved, result);
        }

        let manager = self.identity_manager(&resolved);
        let mut result = identity::replace_did(
            &resolved,
            &manager,
            identity::ReplaceDidParams {
                identity_name: self.globals.identity.clone(),
                is_public,
                is_agent,
                role,
                endpoint_url,
            },
        )
        .map_err(replace_did_identity_exit)?;
        result
            .warnings
            .insert(0, identity::replace_did_danger_warning().to_string());

        let old_did = result
            .data
            .get("old_did")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string();
        let new_did = result
            .data
            .get("did")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string();
        match store::rebind_local_identity_state_with_partial(&resolved.paths, &old_did, &new_did) {
            Ok(outcome) => {
                insert_rebind_counts(&mut result.data, outcome.store_rebind, outcome.e2ee_cleanup)
            }
            Err(err) => {
                insert_rebind_counts(&mut result.data, err.store_rebind, err.e2ee_cleanup);
                result
                    .warnings
                    .push(format!("Local SQLite rebinding failed: {}", err.error));
            }
        }

        self.render_identity_result("awiki-cli id replace-did", &resolved, result)
    }
}

fn replace_did_identity_exit(err: identity::IdentityError) -> ExitError {
    let mut exit = identity_exit(err);
    exit.detail.hint = REPLACE_DID_HINT.to_string();
    exit
}

fn insert_rebind_counts(
    data: &mut serde_json::Value,
    store_rebind: std::collections::BTreeMap<String, i64>,
    e2ee_cleanup: std::collections::BTreeMap<String, i64>,
) {
    if let Some(object) = data.as_object_mut() {
        object.insert(
            "store_rebind".to_string(),
            serde_json::to_value(store_rebind).unwrap_or_else(|_| json!({})),
        );
        object.insert(
            "e2ee_cleanup".to_string(),
            serde_json::to_value(e2ee_cleanup).unwrap_or_else(|_| json!({})),
        );
    }
}
