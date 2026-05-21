use im_core::prelude::{
    Did, Handle, IdentitySelector, InitialProfile, RegisterHandleRequest, VerificationInput,
};

use crate::cli::ParsedCommand;
use crate::output::ExitError;

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
    reject_unsupported_registration_flags(command)?;
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

fn looks_like_handle(value: &str) -> bool {
    value.starts_with('@') || value.contains('.')
}

fn reject_unsupported_registration_flags(command: &ParsedCommand) -> Result<(), ExitError> {
    for flag in ["phone", "email", "invite-code", "wait"] {
        if command
            .flags
            .get(flag)
            .is_some_and(|value| !value.trim().is_empty())
        {
            return Err(ExitError::new(
                "unsupported_capability",
                2,
                format!("id register --{flag} is not supported by the Phase 1 IM Core adapter."),
                "Use the existing legacy id register path until this registration flow is migrated.",
            ));
        }
    }
    Ok(())
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
