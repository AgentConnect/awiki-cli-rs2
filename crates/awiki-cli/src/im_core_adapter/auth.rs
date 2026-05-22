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
    let client = super::build_im_client(
        resolved,
        manager,
        super::cli_identity_selector(identity_name),
    )?;
    let identity_name = client
        .current_identity()
        .local_alias
        .as_deref()
        .unwrap_or_else(|| client.current_identity().id.as_str())
        .to_string();
    let previous = manager
        .load(&identity_name)
        .map_err(crate::app::identity_exit)?;
    let previous_token_present = !previous.jwt_token.trim().is_empty();
    client
        .auth()
        .refresh_session()
        .map_err(|err| super::map_im_error(err, "id refresh-token"))?;
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
