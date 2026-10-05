use crate::cli_output::ExitError;
use crate::cli_parser::ParsedCommand;
use serde::Deserialize;
use std::io::Read;
use zeroize::Zeroize;

const MAX_VERIFICATION_STDIN_BYTES: u64 = 1024;
const REGISTER_USAGE: &str = "awiki-cli id register --handle <handle> --verification-stdin";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistrationVerificationInput {
    phone: Option<String>,
    email: Option<String>,
    #[serde(default)]
    otp: Option<String>,
    #[serde(default)]
    invite_code: Option<String>,
}

pub(super) fn command_with_registration_verification(
    command: &ParsedCommand,
) -> Result<ParsedCommand, ExitError> {
    if command.flags.get("verification-stdin").map(String::as_str) != Some("true") {
        return Ok(command.clone());
    }
    command_with_registration_verification_reader(command, std::io::stdin().lock())
}

fn command_with_registration_verification_reader(
    command: &ParsedCommand,
    reader: impl Read,
) -> Result<ParsedCommand, ExitError> {
    if ["phone", "email", "otp", "invite-code"].iter().any(|name| {
        command
            .flags
            .get(*name)
            .is_some_and(|value| !value.trim().is_empty())
    }) {
        return Err(invalid_verification_input(
            "--verification-stdin cannot be combined with --phone, --email, --otp, or --invite-code.",
        ));
    }

    let mut raw = String::new();
    reader
        .take(MAX_VERIFICATION_STDIN_BYTES + 1)
        .read_to_string(&mut raw)
        .map_err(|_| invalid_verification_input("Unable to read verification input from stdin."))?;
    if raw.len() as u64 > MAX_VERIFICATION_STDIN_BYTES {
        raw.zeroize();
        return Err(invalid_verification_input(
            "Verification input from stdin exceeds the size limit.",
        ));
    }

    let parsed = serde_json::from_str::<RegistrationVerificationInput>(&raw).map_err(|_| {
        raw.zeroize();
        invalid_verification_input("Verification input from stdin is invalid.")
    })?;
    raw.zeroize();

    let phone = parsed.phone.as_deref().map(str::trim);
    let email = parsed.email.as_deref().map(str::trim);
    let otp = parsed.otp.as_deref().map(str::trim);
    if phone.is_some() == email.is_some()
        || phone.is_some_and(|value| value.is_empty() || value.len() > 64)
        || email.is_some_and(|value| value.is_empty() || value.len() > 254)
        || (email.is_some() && otp.is_some())
        || otp.is_some_and(|value| {
            value.len() != 6
                || !value.is_ascii()
                || !value.bytes().all(|byte| byte.is_ascii_digit())
        })
    {
        return Err(invalid_verification_input(
            "Verification input from stdin is invalid.",
        ));
    }

    if parsed
        .invite_code
        .as_ref()
        .is_some_and(|value| value.trim().is_empty() || value.len() > 512)
    {
        return Err(invalid_verification_input("Invitation input is invalid."));
    }
    let mut augmented = command.clone();
    if let Some(invite) = parsed.invite_code {
        augmented
            .flags
            .insert("invite-code".to_owned(), invite.trim().to_owned());
    }
    if let Some(phone) = phone {
        augmented.flags.insert("phone".to_owned(), phone.to_owned());
    }
    if let Some(email) = email {
        augmented.flags.insert("email".to_owned(), email.to_owned());
    }
    if let Some(otp) = otp {
        augmented.flags.insert("otp".to_owned(), otp.to_owned());
    }
    Ok(augmented)
}

fn invalid_verification_input(message: impl Into<String>) -> ExitError {
    ExitError::new(
        "invalid_argument",
        2,
        message,
        format!("Pipe one JSON object with exactly one of phone/email and optional otp (phone only)/invite_code fields to `{REGISTER_USAGE}`."),
    )
}

#[cfg(test)]
#[path = "identity_register_tests.rs"]
mod tests;
