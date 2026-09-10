use super::looks_like_codex_resume_missing;

#[test]
fn only_explicit_missing_native_session_before_execution_can_fall_back() {
    assert!(looks_like_codex_resume_missing(
        b"",
        b"session not found: previous-session"
    ));
    assert!(looks_like_codex_resume_missing(
        br#"{"type":"error","message":"unknown thread: old-thread"}"#,
        b""
    ));
    assert!(!looks_like_codex_resume_missing(
        br#"{"type":"thread.started","thread_id":"active"}"#,
        b"proxy host not found"
    ));
    assert!(!looks_like_codex_resume_missing(
        br#"{"type":"turn.started"}"#,
        b"session not found: remote side"
    ));
    assert!(!looks_like_codex_resume_missing(
        b"resuming session",
        b"proxy host not found"
    ));
    assert!(!looks_like_codex_resume_missing(
        br#"{"type":"error","message":"Reconnecting... 1/5: host not found"}"#,
        b"thread connect failed"
    ));
}
