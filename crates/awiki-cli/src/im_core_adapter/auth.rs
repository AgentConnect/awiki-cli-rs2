use im_core::prelude::AuthScope;

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
