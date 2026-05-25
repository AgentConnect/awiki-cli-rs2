use awiki_cli::host_runtime::listener::{SessionStatus, Status};
use awiki_cli::host_runtime::listener_bridge_runtime::BridgeSessionBootstrapResult;
use awiki_cli::host_runtime::listener_known_sessions::{
    known_session_startup_decision, known_session_startup_error,
    record_known_session_startup_error, KnownSessionStartupDecision,
};
use awiki_cli::host_runtime::listener_session_bootstrap::SESSION_BOOTSTRAP_TIMEOUT;
use std::time::Duration;

#[test]
fn existing_known_session_skips_startup_wait() {
    assert_eq!(
        known_session_startup_decision("alice", true),
        KnownSessionStartupDecision::Existing
    );
}

#[test]
fn new_known_session_starts_and_waits_for_go_bootstrap_timeout() {
    assert_eq!(
        known_session_startup_decision("alice", false),
        KnownSessionStartupDecision::StartAndWait {
            identity_name: "alice".to_string(),
            timeout: SESSION_BOOTSTRAP_TIMEOUT,
        }
    );
    assert_eq!(SESSION_BOOTSTRAP_TIMEOUT, Duration::from_secs(15));
}

#[test]
fn connected_bootstrap_does_not_record_startup_error() {
    assert_eq!(
        known_session_startup_error(
            "alice",
            "did:awiki:alice",
            BridgeSessionBootstrapResult::Connected,
        ),
        None
    );
}

#[test]
fn initial_error_is_recorded_without_failing_whole_startup() {
    let error = known_session_startup_error(
        "bob",
        "did:awiki:bob",
        BridgeSessionBootstrapResult::InitialError("auth bootstrap failed".to_string()),
    )
    .expect("initial error");

    assert_eq!(error.identity_name, "bob");
    assert_eq!(error.did, "did:awiki:bob");
    assert_eq!(error.error, "auth bootstrap failed");
}

#[test]
fn timeout_uses_go_error_text_for_recorded_startup_error() {
    let error = known_session_startup_error(
        "carol",
        "did:awiki:carol",
        BridgeSessionBootstrapResult::Timeout(
            "websocket session bootstrap timed out for identity carol".to_string(),
        ),
    )
    .expect("timeout error");

    assert_eq!(error.identity_name, "carol");
    assert_eq!(error.did, "did:awiki:carol");
    assert_eq!(
        error.error,
        "websocket session bootstrap timed out for identity carol"
    );
}

#[test]
fn recording_startup_error_creates_missing_session_but_preserves_existing_did() {
    let mut status = Status::default();
    let missing = known_session_startup_error(
        "alice",
        "did:awiki:alice",
        BridgeSessionBootstrapResult::InitialError("dial failed".to_string()),
    )
    .expect("missing error");

    record_known_session_startup_error(&mut status, missing);
    assert_eq!(status.sessions.len(), 1);
    assert_eq!(status.sessions[0].identity_name, "alice");
    assert_eq!(status.sessions[0].did, "did:awiki:alice");
    assert!(!status.sessions[0].connected);
    assert_eq!(status.sessions[0].last_error, "dial failed");

    status.sessions.push(SessionStatus {
        identity_name: "bob".to_string(),
        did: "did:awiki:bob-existing".to_string(),
        connected: true,
        last_error: String::new(),
    });
    let existing = known_session_startup_error(
        "bob",
        "did:awiki:bob-new",
        BridgeSessionBootstrapResult::InitialError("auth failed".to_string()),
    )
    .expect("existing error");

    record_known_session_startup_error(&mut status, existing);
    let bob = status
        .sessions
        .iter()
        .find(|session| session.identity_name == "bob")
        .expect("bob status");
    assert_eq!(bob.did, "did:awiki:bob-existing");
    assert!(!bob.connected);
    assert_eq!(bob.last_error, "auth failed");

    status.sessions.push(SessionStatus {
        identity_name: "carol".to_string(),
        did: String::new(),
        connected: false,
        last_error: String::new(),
    });
    let empty_existing = known_session_startup_error(
        "carol",
        "did:awiki:carol",
        BridgeSessionBootstrapResult::InitialError("dial failed".to_string()),
    )
    .expect("empty did existing error");

    record_known_session_startup_error(&mut status, empty_existing);
    let carol = status
        .sessions
        .iter()
        .find(|session| session.identity_name == "carol")
        .expect("carol status");
    assert_eq!(carol.did, "did:awiki:carol");
    assert!(!carol.connected);
    assert_eq!(carol.last_error, "dial failed");
}
