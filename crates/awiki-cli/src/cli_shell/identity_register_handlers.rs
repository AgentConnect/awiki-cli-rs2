use crate::cli_output::ExitError;
use crate::cli_parser::ParsedCommand;
use serde::Deserialize;
use std::io::Read;
use zeroize::Zeroize;

const MAX_VERIFICATION_STDIN_BYTES: u64 = 256;
const REGISTER_USAGE: &str = "awiki-cli id register --handle <handle> --verification-stdin";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistrationVerificationInput {
    phone: String,
    #[serde(default)]
    otp: Option<String>,
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
    if ["phone", "email", "otp"].iter().any(|name| {
        command
            .flags
            .get(*name)
            .is_some_and(|value| !value.trim().is_empty())
    }) {
        return Err(invalid_verification_input(
            "--verification-stdin cannot be combined with --phone, --email, or --otp.",
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

    let phone = parsed.phone.trim();
    let otp = parsed.otp.as_deref().map(str::trim);
    if phone.is_empty()
        || phone.len() > 64
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

    let mut augmented = command.clone();
    augmented.flags.insert("phone".to_owned(), phone.to_owned());
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
        format!("Pipe one JSON object with phone and optional otp fields to `{REGISTER_USAGE}`."),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn stdin_command() -> ParsedCommand {
        ParsedCommand {
            name: "id.register".to_owned(),
            flags: BTreeMap::from([
                ("handle".to_owned(), "alice".to_owned()),
                ("verification-stdin".to_owned(), "true".to_owned()),
            ]),
            ..ParsedCommand::default()
        }
    }

    #[test]
    fn registration_verification_accepts_phone_and_optional_otp_from_stdin() {
        let request_only = command_with_registration_verification_reader(
            &stdin_command(),
            br#"{"phone":"+15550000001"}"#.as_slice(),
        )
        .unwrap();
        assert_eq!(
            request_only.flags.get("phone").map(String::as_str),
            Some("+15550000001")
        );
        assert!(!request_only.flags.contains_key("otp"));

        let completion = command_with_registration_verification_reader(
            &stdin_command(),
            br#"{"phone":"+15550000001","otp":"731946"}"#.as_slice(),
        )
        .unwrap();
        assert_eq!(
            completion.flags.get("phone").map(String::as_str),
            Some("+15550000001")
        );
        assert_eq!(
            completion.flags.get("otp").map(String::as_str),
            Some("731946")
        );
    }

    #[test]
    fn registration_verification_rejects_argv_mix_and_malformed_input_without_echoing_it() {
        let mut mixed = stdin_command();
        mixed
            .flags
            .insert("phone".to_owned(), "+15550000002".to_owned());
        let error = command_with_registration_verification_reader(
            &mixed,
            br#"{"phone":"+15550000003","otp":"819274"}"#.as_slice(),
        )
        .unwrap_err();
        assert_eq!(error.detail.code, "invalid_argument");
        assert!(!error.detail.message.contains("819274"));

        for raw in [
            br#"{"phone":"+15550000003","otp":"81927"}"#.as_slice(),
            br#"{"phone":"+15550000003","otp":"819274","extra":true}"#.as_slice(),
            b"not-json".as_slice(),
        ] {
            let error =
                command_with_registration_verification_reader(&stdin_command(), raw).unwrap_err();
            assert_eq!(error.detail.code, "invalid_argument");
            assert!(!error.detail.message.contains("819274"));
        }
    }
}
