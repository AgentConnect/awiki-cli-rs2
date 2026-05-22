use im_core::prelude::AuthScope;
use serde_json::json;

use crate::config::Resolved;
use crate::identity::{self, Manager};
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

pub fn refresh_token_via_im_core(
    resolved: &Resolved,
    manager: &Manager,
    identity_name: &str,
) -> Result<identity::CommandResult, ExitError> {
    let previous = manager
        .load(&selected_identity_name(resolved, manager, identity_name)?)
        .map_err(crate::app::identity_exit)?;
    let identity_name = previous.identity_name.clone();
    let previous_token_present = !previous.jwt_token.trim().is_empty();
    let client = super::build_im_client(
        resolved,
        manager,
        super::cli_identity_selector(&identity_name),
    )?;
    client
        .auth()
        .refresh_session()
        .map_err(|err| refresh_token_error_from_im(err, "id refresh-token", &identity_name))?;
    let refreshed = manager
        .load(&identity_name)
        .map_err(crate::app::identity_exit)?;
    let identity = identity::store::identity_summary_from_record(&refreshed);
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

fn selected_identity_name(
    resolved: &Resolved,
    manager: &Manager,
    requested: &str,
) -> Result<String, ExitError> {
    identity::service::load_identity_for_mutation(resolved, manager, requested)
        .map(|identity| identity.identity_name)
        .map_err(crate::app::identity_exit)
}
