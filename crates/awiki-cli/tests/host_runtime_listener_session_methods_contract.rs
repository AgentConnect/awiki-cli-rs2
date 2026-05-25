use awiki_cli::host_runtime::listener_identity_record::RuntimeIdentityRecord;
use awiki_cli::host_runtime::listener_session_methods::{
    ListenerSessionMethods, SecureRpcSource, SessionDisconnectReason, SessionMethodAction,
};

#[test]
fn current_accessors_and_snapshot_return_lock_protected_state_like_go() {
    let mut session = ListenerSessionMethods::new("alice");
    assert_eq!(session.identity_name(), "alice");
    assert_eq!(session.current_client(), None);
    assert!(session.current_record().is_none());

    session.mark_connected(
        Some(record("alice", "did:alice")),
        Some("client-1".to_string()),
    );

    assert_eq!(session.current_client(), Some("client-1"));
    assert_eq!(
        session.current_record().map(|record| record.did.as_str()),
        Some("did:alice")
    );
    let snapshot = session.snapshot();
    assert_eq!(
        snapshot
            .record
            .as_ref()
            .map(|record| record.identity_name.as_str()),
        Some("alice")
    );
    assert!(snapshot.connected);
    assert_eq!(snapshot.last_error, "");
}

#[test]
fn secure_rpc_prefers_override_then_current_client_then_none() {
    let mut session = ListenerSessionMethods::new("alice");
    assert_eq!(session.secure_rpc_source(), SecureRpcSource::None);

    session.mark_connected(
        Some(record("alice", "did:alice")),
        Some("client-1".to_string()),
    );
    assert_eq!(
        session.secure_rpc_source(),
        SecureRpcSource::Client("client-1".to_string())
    );

    session.set_secure_rpc_override_available(true);
    assert_eq!(session.secure_rpc_source(), SecureRpcSource::Override);

    session.close_current_client();
    assert_eq!(
        session.secure_rpc_source(),
        SecureRpcSource::Override,
        "Go returns secureRPCCall before checking whether client is nil"
    );

    session.set_secure_rpc_override_available(false);
    assert_eq!(session.secure_rpc_source(), SecureRpcSource::None);
}

#[test]
fn mark_connected_closes_only_replaced_existing_client_and_clears_error() {
    let mut session = ListenerSessionMethods::new("alice");
    session.mark_connected(
        Some(record("alice", "did:alice")),
        Some("client-1".to_string()),
    );

    let actions = session.mark_connected(
        Some(record("alice", "did:alice-v2")),
        Some("client-2".to_string()),
    );

    assert_eq!(
        actions,
        vec![SessionMethodAction::CloseClient {
            client_id: "client-1".to_string(),
        }]
    );
    assert_eq!(session.current_client(), Some("client-2"));
    let snapshot = session.snapshot();
    assert_eq!(
        snapshot.record.as_ref().map(|record| record.did.as_str()),
        Some("did:alice-v2")
    );
    assert!(snapshot.connected);
    assert_eq!(snapshot.last_error, "");

    let actions = session.mark_connected(
        Some(record("alice", "did:alice-v3")),
        Some("client-2".to_string()),
    );
    assert!(
        actions.is_empty(),
        "Go does not close the current client when the replacement is the same pointer"
    );
}

#[test]
fn mark_connected_after_disconnected_does_not_close_previous_client_again() {
    let mut session = ListenerSessionMethods::new("alice");
    session.mark_connected(
        Some(record("alice", "did:alice")),
        Some("client-1".to_string()),
    );
    let disconnected = session.mark_disconnected(Some(SessionDisconnectReason::Other(
        "dial failed".to_string(),
    )));
    assert_eq!(
        disconnected,
        vec![SessionMethodAction::CloseClient {
            client_id: "client-1".to_string(),
        }]
    );

    let reconnected = session.mark_connected(
        Some(record("alice", "did:alice-v2")),
        Some("client-2".to_string()),
    );

    assert!(
        reconnected.is_empty(),
        "Go markDisconnected clears s.client, so reconnecting does not close it again"
    );
    assert_eq!(session.current_client(), Some("client-2"));
}

#[test]
fn mark_connected_with_nil_record_or_client_still_marks_connected_like_go() {
    let mut session = ListenerSessionMethods::new("alice");

    let actions = session.mark_connected(None, None);

    assert!(actions.is_empty());
    assert!(session.current_record().is_none());
    assert_eq!(session.current_client(), None);
    let snapshot = session.snapshot();
    assert!(snapshot.connected);
    assert_eq!(snapshot.last_error, "");
}

#[test]
fn mark_disconnected_closes_current_client_and_preserves_context_cancel_error() {
    let mut session = ListenerSessionMethods::new("alice");
    session.mark_connected(
        Some(record("alice", "did:alice")),
        Some("client-1".to_string()),
    );

    let actions = session.mark_disconnected(Some(SessionDisconnectReason::Other(
        "reader stopped".to_string(),
    )));

    assert_eq!(
        actions,
        vec![SessionMethodAction::CloseClient {
            client_id: "client-1".to_string(),
        }]
    );
    let snapshot = session.snapshot();
    assert!(!snapshot.connected);
    assert_eq!(snapshot.last_error, "reader stopped");
    assert_eq!(session.current_client(), None);

    session.mark_connected(
        Some(record("alice", "did:alice")),
        Some("client-2".to_string()),
    );
    let actions = session.mark_disconnected(Some(SessionDisconnectReason::ContextCanceled));

    assert_eq!(
        actions,
        vec![SessionMethodAction::CloseClient {
            client_id: "client-2".to_string(),
        }]
    );
    let snapshot = session.snapshot();
    assert!(!snapshot.connected);
    assert_eq!(
        snapshot.last_error, "",
        "Go ignores context.Canceled when recording lastError"
    );
}

#[test]
fn close_current_client_closes_only_when_present_and_does_not_clear_last_error() {
    let mut session = ListenerSessionMethods::new("alice");
    session.mark_disconnected(Some(SessionDisconnectReason::Other(
        "previous error".to_string(),
    )));

    let actions = session.close_current_client();
    assert!(actions.is_empty());
    assert_eq!(session.snapshot().last_error, "previous error");

    session.mark_connected(
        Some(record("alice", "did:alice")),
        Some("client-1".to_string()),
    );
    session.mark_disconnected(Some(SessionDisconnectReason::Other(
        "error to preserve".to_string(),
    )));
    session.mark_connected(
        Some(record("alice", "did:alice")),
        Some("client-2".to_string()),
    );

    let actions = session.close_current_client();
    assert_eq!(
        actions,
        vec![SessionMethodAction::CloseClient {
            client_id: "client-2".to_string(),
        }]
    );
    assert!(!session.snapshot().connected);
    assert_eq!(session.current_client(), None);
    assert_eq!(
        session.snapshot().last_error,
        "",
        "Go closeCurrentClient does not change lastError; markConnected already cleared it"
    );
}

#[test]
fn signal_initial_sends_result_once_and_closes_channel_like_go_sync_once() {
    let mut session = ListenerSessionMethods::new("alice");

    let first = session
        .signal_initial(Some("bootstrap failed".to_string()))
        .expect("first signal sends");
    assert_eq!(first.error.as_deref(), Some("bootstrap failed"));
    assert!(first.channel_closed);
    assert!(session.initial_signaled());

    assert!(
        session.signal_initial(None).is_none(),
        "Go sync.Once prevents later initResult sends"
    );
}

fn record(identity_name: &str, did: &str) -> RuntimeIdentityRecord {
    RuntimeIdentityRecord {
        identity_name: identity_name.to_string(),
        did: did.to_string(),
        ..RuntimeIdentityRecord::default()
    }
}
