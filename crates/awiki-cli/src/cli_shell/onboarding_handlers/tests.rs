use super::*;
use im_core::prelude::{Did, Handle, MessageId};
use std::collections::BTreeMap;

#[test]
fn skill_token_stdin_accepts_one_line_and_redacts_debug() {
    let token = read_skill_token("awsk1_1234567890abcdef\r\n".as_bytes()).unwrap();
    let debug = format!("{token:?}");
    assert!(!debug.contains("1234567890abcdef"));
    assert!(debug.contains("redacted"));
}

#[test]
fn skill_token_stdin_rejects_empty_multiple_or_oversized_lines() {
    for raw in ["", "\n", "awsk1_1234567890abcdef\nsecond"] {
        let error = read_skill_token(raw.as_bytes()).unwrap_err();
        assert_eq!(error.detail.code, "invalid_argument");
        assert!(!error.detail.message.contains("1234567890abcdef"));
    }
    let error =
        read_skill_token(vec![b'x'; MAX_TOKEN_STDIN_BYTES as usize + 1].as_slice()).unwrap_err();
    assert_eq!(error.detail.code, "invalid_argument");
}

#[test]
fn greeting_pending_is_retryable_and_contains_only_public_result_fields() {
    let result = im_core::SkillClaimResult {
        phase: im_core::SkillClaimPhase::ControllerGreetingPending,
        status: im_core::SkillClaimStatus::GreetingPending,
        agent_did: Did::parse("did:wba:awiki.info:agent:test").unwrap(),
        agent_handle: Handle::parse("skill-test.awiki.info", "awiki.info").unwrap(),
        controller_handle: Handle::parse("alice.awiki.info", "awiki.info").unwrap(),
        greeting_message_id: MessageId::parse("skill-greeting-test").unwrap(),
        retryable: true,
        error_code: Some("skill_onboarding_greeting_pending".to_owned()),
    };

    let error = greeting_pending_error(&result, "awiki-cli onboarding resume");
    assert_eq!(error.exit_code, 5);
    assert_eq!(error.detail.code, "skill_onboarding_greeting_pending");
    assert!(error.detail.retryable);
    let rendered = serde_json::to_string(&error.detail).unwrap();
    assert!(rendered.contains("greeting_status"));
    assert!(!rendered.contains("token"));
    assert!(!rendered.contains("jwt"));
    assert!(!rendered.contains("private"));
    assert!(error.detail.hint.contains("onboarding resume"));
    assert!(!error.detail.hint.contains("same authorized Token"));
}

#[test]
fn resume_request_has_exact_scope_and_no_token_input() {
    let command = ParsedCommand {
        name: "onboarding.resume".to_owned(),
        flags: BTreeMap::from([
            (
                "service-base-url".to_owned(),
                "https://awiki.info".to_owned(),
            ),
            (
                "expected-controller-handle".to_owned(),
                "alice.awiki.info".to_owned(),
            ),
            (
                "expected-agent-handle".to_owned(),
                "skill-test.awiki.info".to_owned(),
            ),
        ]),
        ..ParsedCommand::default()
    };

    let request = resume_request(&command).unwrap();
    assert_eq!(request.service_base_url, "https://awiki.info");
    assert_eq!(request.expected_controller_handle, "alice.awiki.info");
    assert_eq!(request.expected_agent_handle, "skill-test.awiki.info");
    assert!(!format!("{request:?}").contains("token"));
}

#[test]
fn prekey_claim_error_points_to_tokenless_resume() {
    let error = map_claim_error(im_core::ImError::SkillOnboarding {
        code: "anp.prekey.conflict".to_owned(),
        phase: "device_prekey".to_owned(),
        retryable: false,
    });

    assert_eq!(error.detail.code, "anp.prekey.conflict");
    assert!(error.detail.hint.contains("onboarding resume"));
    assert!(error.detail.hint.contains("no Token"));
    assert!(!error.detail.hint.contains("original authorized Token"));
}

#[test]
fn pending_v1_claim_points_to_real_recovery_command_without_suggesting_reclaim() {
    let error = map_claim_error(im_core::ImError::SkillOnboarding {
        code: "skill_onboarding_legacy_claim_recovery_required".to_owned(),
        phase: "legacy_journal".to_owned(),
        retryable: false,
    });

    assert_eq!(
        error.detail.code,
        "skill_onboarding_legacy_claim_recovery_required"
    );
    assert!(error.detail.hint.contains("recover-legacy-claim"));
    assert!(error.detail.hint.contains("do not delete"));
    assert!(error
        .detail
        .hint
        .contains("do not delete the journal or start a new claim"));
}

#[test]
fn missing_v1_pending_material_requires_operator_reconciliation() {
    let error = map_legacy_claim_recovery_error(im_core::ImError::SkillOnboarding {
        code: "blocked_requires_operator_reconciliation".to_owned(),
        phase: "legacy_pending_identity".to_owned(),
        retryable: false,
    });

    assert_eq!(
        error.detail.code,
        "blocked_requires_operator_reconciliation"
    );
    assert!(!error.detail.retryable);
    assert!(error.detail.hint.contains("Preserve the workspace"));
    assert!(error.detail.hint.contains("do not run a new claim"));
    let rendered = serde_json::to_string(&error.detail).unwrap();
    for forbidden in ["access_token", "jwt", "private", "cursor"] {
        assert!(!rendered.contains(forbidden));
    }
}

#[test]
fn legacy_migration_retry_is_stable_and_secret_free() {
    let status = im_core::identity::LegacyUpgradeStatus::RetryRequired {
        identity_id: "skill-local-id".to_owned(),
        code: "transport_unavailable".to_owned(),
    };
    let error = legacy_migration_retry_error("transport_unavailable", &status);

    assert_eq!(error.exit_code, 5);
    assert_eq!(error.detail.code, "skill_legacy_migration_retry_required");
    assert!(error.detail.retryable);
    assert!(error.detail.hint.contains("onboarding migrate-legacy"));
    let rendered = serde_json::to_string(&error.detail).unwrap();
    assert!(rendered.contains("transport_unavailable"));
    for forbidden in ["access_token", "jwt", "private", "cursor"] {
        assert!(!rendered.contains(forbidden));
    }
}
