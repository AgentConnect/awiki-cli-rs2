use super::*;
use im_core::prelude::{Did, Handle, MessageId};

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

    let error = greeting_pending_error(&result);
    assert_eq!(error.exit_code, 5);
    assert_eq!(error.detail.code, "skill_onboarding_greeting_pending");
    assert!(error.detail.retryable);
    let rendered = serde_json::to_string(&error.detail).unwrap();
    assert!(rendered.contains("greeting_status"));
    assert!(!rendered.contains("token"));
    assert!(!rendered.contains("jwt"));
    assert!(!rendered.contains("private"));
}
