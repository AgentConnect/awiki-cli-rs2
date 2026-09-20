use super::read_app_action;

#[test]
fn app_action_stdin_is_bounded_and_parse_errors_do_not_echo_content() {
    let parsed = read_app_action(
        br#"{"action":"message.create_draft","args":{"draft_text":"hello"}}"#.as_slice(),
    )
    .unwrap();
    assert_eq!(parsed["args"]["draft_text"], "hello");
    let oversized = vec![b' '; 64 * 1024 + 1];
    assert_eq!(
        read_app_action(oversized.as_slice())
            .unwrap_err()
            .to_string(),
        "app_action_request_too_large"
    );
    for invalid in [
        "[1,2]",
        "\"secret-content\"",
        "private confidential malformed request",
    ] {
        let error = read_app_action(invalid.as_bytes()).unwrap_err().to_string();
        assert_eq!(error, "invalid_app_action_json");
        assert!(!error.contains(invalid));
    }
}
