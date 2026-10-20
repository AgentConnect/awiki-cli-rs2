use super::*;
use im_core::identity::{HandleRegistrationJoinMode, HandleRegistrationJoinRequiredPreparation};

fn registration() -> HandleRegistrationResult {
    let handle = Handle::parse("alice.awiki.test", "").unwrap();
    HandleRegistrationResult {
        identity: None,
        account_id: None,
        handle: handle.clone(),
        method: RegistrationMethod::Phone,
        state: HandleRegistrationState::JoinRequired,
        join_required: Some(HandleRegistrationJoinRequiredPreparation {
            preparation_id: "regjoin_process_local_only".into(),
            mode: HandleRegistrationJoinMode::Ordinary,
            requires_user_presence: false,
            expected_did: Did::parse("did:wba:awiki.test:alice:e1_root").unwrap(),
            full_handle: handle,
        }),
        default_identity_change: None,
        retry_after_seconds: None,
        retry_at: None,
        warnings: vec!["existing_account_verified".into()],
    }
}

#[test]
fn ordinary_phone_join_consumes_the_exact_preparation_without_grant_inputs() {
    let result = registration();
    let first = begin_request(&result).unwrap();
    let second = begin_request(&result).unwrap();
    assert_eq!(first.preparation_id, "regjoin_process_local_only");
    assert_eq!(first.ttl_seconds, 600);
    assert!(!first.user_presence_confirmed);
    assert!(first.operation_id.starts_with("cli-register-join-"));
    assert_ne!(first.operation_id, second.operation_id);
    assert!(!first.operation_id.contains(&first.preparation_id));
}

#[test]
fn registration_join_never_automatically_confirms_recovery() {
    for (mode, requires_presence) in [
        (HandleRegistrationJoinMode::Ordinary, true),
        (HandleRegistrationJoinMode::HandleRecoveryRebind, true),
        (HandleRegistrationJoinMode::HandleRecoveryRebind, false),
    ] {
        let mut result = registration();
        let preparation = result.join_required.as_mut().unwrap();
        preparation.mode = mode;
        preparation.requires_user_presence = requires_presence;
        let error = begin_request(&result).unwrap_err();
        assert_eq!(error.detail.code, "registration_join_confirmation_required");
        assert!(!serde_json::to_string(&error.detail)
            .unwrap()
            .contains("regjoin_process_local_only"));
    }
}

#[test]
fn malformed_registration_preparation_fails_before_join() {
    for change in ["missing", "empty", "handle", "state"] {
        let mut result = registration();
        match change {
            "missing" => result.join_required = None,
            "empty" => result
                .join_required
                .as_mut()
                .unwrap()
                .preparation_id
                .clear(),
            "handle" => {
                result.join_required.as_mut().unwrap().full_handle =
                    Handle::parse("bob.awiki.test", "").unwrap()
            }
            _ => result.state = HandleRegistrationState::OtpSent,
        }
        assert_eq!(
            begin_request(&result).unwrap_err().detail.code,
            "registration_join_unavailable"
        );
    }
}

fn progress(phase: &str, remote_state: &str) -> im_core::identity::DeviceJoinProgress {
    serde_json::from_value(json!({
        "session": {
            "join_session_id": "join-phone-entry",
            "did": "did:wba:awiki.test:alice:e1_root",
            "protocol_device_id": "device-phone-entry",
            "side": "new_device",
            "phase": phase,
            "expires_at": "2026-10-05T00:00:00Z"
        },
        "remote_state": remote_state,
        "sas": "012345",
        "authorized_device": null
    }))
    .unwrap()
}

#[test]
fn phone_join_returns_a_durable_poll_target_without_ephemeral_or_secret_data() {
    let result = registration();
    let output = command_result(&result, progress("pending", "pending")).unwrap();
    assert_eq!(output.data["action"], "device_join_start");
    assert_eq!(output.data["verification_state"], "join_pending");
    assert_eq!(
        output.data["result"]["session"]["join_session_id"],
        "join-phone-entry"
    );
    assert_eq!(output.warnings, result.warnings);
    let encoded = serde_json::to_string(&output.data).unwrap();
    for secret in [
        "012345",
        "regjoin_process_local_only",
        "preparation_id",
        "account_verification_token",
    ] {
        assert!(!encoded.contains(secret), "{secret}");
    }
}

#[test]
fn resumed_authorized_join_does_not_claim_it_still_needs_approval() {
    let output = command_result(&registration(), progress("authorized", "consumed")).unwrap();
    assert_eq!(output.data["verification_state"], "join_authorized");
    assert!(!output.summary.contains("Approve the request"));
}
