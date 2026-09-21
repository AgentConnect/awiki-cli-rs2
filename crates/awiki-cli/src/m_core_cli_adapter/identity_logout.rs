use im_core::identity::{DeleteLocalIdentityResult, IdentitySelector};
use serde_json::json;

use super::message_result::CommandResult;
use crate::cli_output::ExitError;
use crate::workspace_config::Resolved;

pub fn explicit_selector(identity: &str) -> Result<IdentitySelector, ExitError> {
    let selector = super::cli_identity_selector(identity);
    if matches!(selector, IdentitySelector::Default) {
        return Err(ExitError::new(
            "invalid_argument", 2,
            "id logout requires an explicit --identity; default is not allowed.",
            "Use --identity <alias, DID, or Handle> id logout. Local credentials will be retired; message history is kept.",
        ));
    }
    Ok(selector)
}

pub fn plan(identity: &str) -> CommandResult {
    CommandResult {
        data: json!({"plan": {"action": "logout", "identity_name": identity.trim(), "mode": "credential_only", "preserves_business_data": true, "remote_calls": []}}),
        summary: "Dry run: local identity logout planned; message history is kept".into(),
        warnings: Vec::new(),
    }
}

pub fn logout(resolved: &Resolved, selector: IdentitySelector) -> Result<CommandResult, ExitError> {
    let core = super::build_im_core(resolved)?;
    let result = core
        .identities()
        .delete_local_identity(selector)
        .map_err(|err| super::map_im_error(err, "id logout"))?;
    Ok(command_result(result))
}

pub async fn logout_async(
    resolved: &Resolved,
    selector: IdentitySelector,
) -> Result<CommandResult, ExitError> {
    let core = super::build_im_core_async(resolved).await?;
    let result = core
        .identities()
        .delete_local_identity_async(selector)
        .await
        .map_err(|err| super::map_im_error(err, "id logout"))?;
    Ok(command_result(result))
}

fn command_result(result: DeleteLocalIdentityResult) -> CommandResult {
    CommandResult {
        data: json!({"action": "logout", "mode": "credential_only", "preserves_business_data": true,
            "deleted": result.deleted, "was_default": result.was_default, "next_default": result.next_default}),
        summary: "Local identity credentials retired; message history is kept. Start device join in this workspace to sign in again.".into(),
        warnings: result.warnings,
    }
}
