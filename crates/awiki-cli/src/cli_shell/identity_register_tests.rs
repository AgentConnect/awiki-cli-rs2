use super::*;
#[test]
fn registration_verification_accepts_email_and_rejects_ambiguous_factors() {
    let command = command_with_registration_verification_reader(
        &stdin_command(),
        br#"{"email":" alice@example.test "}"#.as_slice(),
    )
    .unwrap();
    assert_eq!(
        command.flags.get("email").map(String::as_str),
        Some("alice@example.test")
    );
    assert!(!command.flags.contains_key("phone"));
    for raw in [
        br#"{}"#.as_slice(),
        br#"{"email":" "}"#.as_slice(),
        br#"{"phone":"+15550000001","email":"alice@example.test"}"#.as_slice(),
        br#"{"email":"alice@example.test","otp":"123456"}"#.as_slice(),
        br#"{"email":"alice@example.test","email":"bob@example.test"}"#.as_slice(),
    ] {
        let error =
            command_with_registration_verification_reader(&stdin_command(), raw).unwrap_err();
        assert_eq!(error.detail.code, "invalid_argument");
        assert!(!error.detail.message.contains("example.test"));
    }
}
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
fn registration_invite_is_carried_without_argv() {
    let command = command_with_registration_verification_reader(
        &stdin_command(),
        br#"{"phone":"+15550000001","otp":"123456","invite_code":"local-invite"}"#.as_slice(),
    )
    .unwrap();
    assert_eq!(
        command.flags.get("invite-code").map(String::as_str),
        Some("local-invite")
    );
    let mut duplicate = stdin_command();
    duplicate
        .flags
        .insert("invite-code".to_owned(), "argv-invite".to_owned());
    assert!(command_with_registration_verification_reader(
        &duplicate,
        br#"{"phone":"+15550000001","invite_code":"stdin-invite"}"#.as_slice()
    )
    .is_err());
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
