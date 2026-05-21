use im_core::prelude::{
    Did, Handle, IdentitySelector, InitialProfile, RegisterHandleRequest, VerificationInput,
};

use crate::cli::ParsedCommand;
use crate::identity;
use crate::output::ExitError;

#[derive(Debug, Clone)]
pub struct RegisterHandleBridgeRequest {
    pub sdk: RegisterHandleRequest,
    pub legacy: identity::RegisterParams,
}

pub fn cli_identity_selector(identity_flag: &str) -> IdentitySelector {
    let value = identity_flag.trim();
    if value.is_empty() || value == "default" {
        return IdentitySelector::Default;
    }
    if value.starts_with("did:") {
        return Did::parse(value)
            .map(IdentitySelector::Did)
            .unwrap_or_else(|_| IdentitySelector::LocalAlias(value.to_string()));
    }
    if looks_like_handle(value) {
        return Handle::parse(value, "")
            .map(IdentitySelector::Handle)
            .unwrap_or_else(|_| IdentitySelector::LocalAlias(value.to_string()));
    }
    IdentitySelector::LocalAlias(value.to_string())
}

pub fn register_handle_request(
    command: &ParsedCommand,
) -> Result<RegisterHandleRequest, ExitError> {
    let handle = string_flag(command, "handle");
    let requested_handle = Handle::parse(&handle, "").map_err(|err| {
        ExitError::new(
            "invalid_argument",
            2,
            format!("invalid --handle: {err}"),
            "Use a non-empty handle local part or full handle.",
        )
    })?;
    let local_alias = trimmed_optional(&command.globals.identity);
    let otp = string_flag(command, "otp");
    Ok(RegisterHandleRequest {
        local_alias,
        requested_handle,
        verification: if otp.trim().is_empty() {
            VerificationInput::AlreadyVerified
        } else {
            VerificationInput::Otp {
                code: otp.trim().to_string(),
            }
        },
        profile: InitialProfile {
            display_name: trimmed_optional(&string_flag(command, "display-name")),
            avatar_url: trimmed_optional(&string_flag(command, "avatar-url")),
        },
        make_default: !command
            .flags
            .get("no-default")
            .is_some_and(|value| value == "true"),
    })
}

pub fn register_handle_bridge_request(
    command: &ParsedCommand,
    identity_flag: &str,
) -> Result<RegisterHandleBridgeRequest, ExitError> {
    let mut sdk_command = command.clone();
    sdk_command.globals.identity = identity_flag.to_string();
    let sdk = register_handle_request(&sdk_command)?;
    let legacy = identity::RegisterParams {
        identity_name: identity_flag.to_string(),
        handle: string_flag(command, "handle"),
        phone: string_flag(command, "phone"),
        email: string_flag(command, "email"),
        otp: string_flag(command, "otp"),
        invite_code: string_flag(command, "invite-code"),
        wait: command
            .flags
            .get("wait")
            .is_some_and(|value| value == "true"),
        verification_timeout: 300,
        poll_interval_seconds: 5.0,
    };
    Ok(RegisterHandleBridgeRequest { sdk, legacy })
}

pub fn register_handle_plan_via_im_core(
    manager: &identity::Manager,
    did_domain: &str,
    command: &ParsedCommand,
    identity_flag: &str,
) -> Result<identity::CommandResult, ExitError> {
    let bridge = register_handle_bridge_request(command, identity_flag)?;
    let _sdk_request = bridge.sdk;
    identity::register_plan(manager, did_domain, &bridge.legacy).map_err(crate::app::identity_exit)
}

pub fn register_handle_via_im_core(
    resolved: &crate::config::Resolved,
    manager: &identity::Manager,
    command: &ParsedCommand,
    identity_flag: &str,
) -> Result<identity::CommandResult, ExitError> {
    let bridge = register_handle_bridge_request(command, identity_flag)?;
    let core = super::build_im_core(resolved, manager)?;
    core.identities()
        .register_handle(bridge.sdk)
        .map_err(|err| super::map_im_error(err, "id register"))?;
    identity::register(resolved, manager, bridge.legacy).map_err(crate::app::identity_exit)
}

fn looks_like_handle(value: &str) -> bool {
    value.starts_with('@') || value.contains('.')
}

fn string_flag(command: &ParsedCommand, name: &str) -> String {
    command.flags.get(name).cloned().unwrap_or_default()
}

fn trimmed_optional(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
