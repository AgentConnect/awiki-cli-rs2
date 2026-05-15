use awiki_cli::runtime::listener::{has_disconnected_sessions, session_warnings, SessionStatus};
use awiki_cli::runtime::listener_session_state::ListenerSessionState;

#[test]
fn mark_connected_clears_prior_error_and_sets_did() {
    let mut state = ListenerSessionState::default();
    state.record_session_error("alice", "did:awiki:old", "bootstrap failed");

    state.mark_connected("alice", "did:awiki:alice");

    let session = session_for(&state, "alice");
    assert_eq!(session.identity_name, "alice");
    assert_eq!(session.did, "did:awiki:alice");
    assert!(session.connected);
    assert_eq!(session.last_error, "");
}

#[test]
fn mark_disconnected_with_none_or_empty_error_preserves_existing_error() {
    let mut state = ListenerSessionState::default();
    state.record_session_error("alice", "did:awiki:alice", "first dial failed");

    state.mark_disconnected("alice", None);
    let session = session_for(&state, "alice");
    assert!(!session.connected);
    assert_eq!(session.last_error, "first dial failed");

    state.mark_disconnected("alice", Some(""));
    let session = session_for(&state, "alice");
    assert!(!session.connected);
    assert_eq!(session.last_error, "first dial failed");
}

#[test]
fn mark_disconnected_with_nonempty_error_records_it() {
    let mut state = ListenerSessionState::default();
    state.mark_connected("alice", "did:awiki:alice");

    state.mark_disconnected("alice", Some("websocket notification loop closed"));

    let session = session_for(&state, "alice");
    assert_eq!(session.did, "did:awiki:alice");
    assert!(!session.connected);
    assert_eq!(session.last_error, "websocket notification loop closed");
}

#[test]
fn record_session_error_creates_missing_and_keeps_existing_record_did_like_go() {
    let mut state = ListenerSessionState::default();

    state.record_session_error("missing", "did:awiki:missing", "bootstrap timed out");
    let missing = session_for(&state, "missing");
    assert_eq!(missing.did, "did:awiki:missing");
    assert!(!missing.connected);
    assert_eq!(missing.last_error, "bootstrap timed out");

    state.mark_connected("alice", "did:awiki:alice");
    state.record_session_error("alice", "", "dial failed");
    let alice = session_for(&state, "alice");
    assert_eq!(alice.did, "did:awiki:alice");
    assert!(!alice.connected);
    assert_eq!(alice.last_error, "dial failed");

    state.record_session_error("alice", "did:awiki:alice-updated", "auth failed");
    let alice = session_for(&state, "alice");
    assert_eq!(alice.did, "did:awiki:alice");
    assert!(!alice.connected);
    assert_eq!(alice.last_error, "auth failed");
}

#[test]
fn snapshot_uses_map_key_identity_and_stable_order() {
    let mut state = ListenerSessionState::default();
    state.mark_connected("zhuocheng-map-key", "did:awiki:zhuocheng");
    state.record_session_error("alice-map-key", "did:awiki:alice", "dial failed");

    let sessions = state.snapshot_sessions();

    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0].identity_name, "alice-map-key");
    assert_eq!(sessions[0].did, "did:awiki:alice");
    assert_eq!(sessions[1].identity_name, "zhuocheng-map-key");
    assert_eq!(sessions[1].did, "did:awiki:zhuocheng");
}

#[test]
fn set_bridge_available_reports_changes_and_tracks_current_value() {
    let mut state = ListenerSessionState::default();

    assert!(!state.bridge_available());
    assert!(!state.set_bridge_available(false));
    assert!(!state.bridge_available());

    assert!(state.set_bridge_available(true));
    assert!(state.bridge_available());
    assert!(!state.set_bridge_available(true));
    assert!(state.bridge_available());

    assert!(state.set_bridge_available(false));
    assert!(!state.bridge_available());
}

#[test]
fn snapshots_feed_existing_listener_session_warning_helpers() {
    let mut state = ListenerSessionState::default();
    state.mark_connected("alice", "did:awiki:alice");
    state.record_session_error("bob", "did:awiki:bob", "dial failed");

    let sessions = state.snapshot_sessions();

    assert!(has_disconnected_sessions(&sessions));
    assert_eq!(
        session_warnings(&sessions),
        vec!["websocket session for identity bob is disconnected: dial failed"]
    );
}

fn session_for(state: &ListenerSessionState, identity_name: &str) -> SessionStatus {
    state
        .snapshot_sessions()
        .into_iter()
        .find(|session| session.identity_name == identity_name)
        .expect("session exists")
}
