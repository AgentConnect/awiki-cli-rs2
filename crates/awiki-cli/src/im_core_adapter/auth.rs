use im_core::prelude::AuthScope;
use serde_json::json;

use crate::config::Resolved;
use crate::legacy_identity as identity;
use crate::output::ExitError;

pub fn auth_scope_from_cli(value: &str) -> Result<AuthScope, ExitError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "" | "profile" | "user-profile" | "user_profile" => Ok(AuthScope::UserProfile),
        "message" | "messages" | "messaging" => Ok(AuthScope::Messaging),
        "group" | "groups" | "group-messaging" | "group_messaging" => Ok(AuthScope::GroupMessaging),
        value => Err(ExitError::new(
            "invalid_argument",
            2,
            format!("unsupported auth scope {value:?}."),
            "Use profile, messaging, or group-messaging.",
        )),
    }
}

pub fn refresh_token_plan_via_im_core(identity_name: &str) -> identity::CommandResult {
    identity::CommandResult {
        data: json!({
            "plan": {
                "action": "refresh_token",
                "identity_name": identity_name.trim(),
                "remote_calls": ["did-auth.get_me"],
                "local_writes": ["auth.json"],
                "auth_flow": "did_auth_get_me_without_stored_bearer",
            }
        }),
        summary: "Dry run: JWT refresh planned".to_string(),
        warnings: Vec::new(),
    }
}

pub fn refresh_token_via_im_core(
    resolved: &Resolved,
    identity_name: &str,
) -> Result<identity::CommandResult, ExitError> {
    let selector = super::cli_identity_selector(identity_name);
    let client = super::build_im_client(resolved, selector)?;
    let identity_name = super::identity::sdk_identity_name(client.current_identity());
    let previous_status = client
        .auth()
        .status()
        .map_err(|err| super::map_im_error(err, "id refresh-token"))?;
    let previous_token_present = previous_status.has_session || !previous_status.needs_refresh;
    client
        .auth()
        .refresh_session()
        .map_err(|err| refresh_token_error_from_im(err, "id refresh-token", &identity_name))?;
    let status = client
        .auth()
        .status()
        .map_err(|err| super::map_im_error(err, "id refresh-token"))?;
    let identity = super::identity::cli_identity_summary_from_sdk_with_status(
        client.current_identity(),
        &status,
    );
    Ok(identity::CommandResult {
        data: json!({
            "action": "refresh_token",
            "identity": identity,
            "previous_token_present": previous_token_present,
            "auth_flow": "did_auth_get_me_without_stored_bearer",
        }),
        summary: format!("JWT refreshed for identity {}", identity.identity_name),
        warnings: Vec::new(),
    })
}

fn refresh_token_error_from_im(
    err: im_core::ImError,
    context: &'static str,
    identity_name: &str,
) -> ExitError {
    match err {
        im_core::ImError::CredentialFileUnreadable { path_kind, .. }
            if matches!(path_kind.as_str(), "did_document" | "private_key") =>
        {
            crate::app::identity_exit(identity::IdentityError::AuthRequired(format!(
                "authentication required: failed to refresh jwt for identity {identity_name}"
            )))
        }
        err => super::map_im_error(err, context),
    }
}
