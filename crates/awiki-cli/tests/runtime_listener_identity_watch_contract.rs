use awiki_cli::runtime::listener_identity_watch::{
    start_known_sessions_step, watch_new_identities_start_step, watch_new_identities_step,
    EnsureSessionOutcome, ListenerIdentitySummary, ListenerIdentityWatchAction,
    StartKnownSessionEnsureOutcome, StartKnownSessionsDecision, StartKnownSessionsEvent,
    WatchNewIdentitiesDecision, WatchNewIdentitiesEvent, WATCH_NEW_IDENTITIES_INTERVAL,
};
use std::time::Duration;

#[test]
fn watch_interval_matches_go_ticker() {
    assert_eq!(WATCH_NEW_IDENTITIES_INTERVAL, Duration::from_secs(3));
    assert_eq!(
        watch_new_identities_start_step().actions,
        vec![ListenerIdentityWatchAction::StartTicker {
            interval: Duration::from_secs(3),
        }]
    );
}

#[test]
fn start_known_sessions_returns_manager_list_error_without_refresh() {
    let step = start_known_sessions_step(
        StartKnownSessionsEvent::ManagerListError("index read failed".to_string()),
        &[],
    );

    assert!(step.actions.is_empty());
    assert_eq!(
        step.decision,
        StartKnownSessionsDecision::ReturnError("index read failed".to_string())
    );
}

#[test]
fn start_known_sessions_ensures_each_identity_records_errors_and_refreshes() {
    let step = start_known_sessions_step(
        StartKnownSessionsEvent::Identities(vec![
            summary("alice", "did:alice"),
            summary("bob", "did:bob"),
        ]),
        &[
            StartKnownSessionEnsureOutcome::Ensure(EnsureSessionOutcome::Ok),
            StartKnownSessionEnsureOutcome::Ensure(EnsureSessionOutcome::Error(
                "auth bootstrap failed".to_string(),
            )),
        ],
    );

    assert_eq!(
        step.actions,
        vec![
            ListenerIdentityWatchAction::EnsureSession {
                identity_name: "alice".to_string(),
            },
            ListenerIdentityWatchAction::EnsureSession {
                identity_name: "bob".to_string(),
            },
            ListenerIdentityWatchAction::RecordSessionError {
                identity_name: "bob".to_string(),
                did: "did:bob".to_string(),
                error: "auth bootstrap failed".to_string(),
            },
            ListenerIdentityWatchAction::RefreshStatus,
            ListenerIdentityWatchAction::RefreshStatus,
        ]
    );
    assert_eq!(step.decision, StartKnownSessionsDecision::ReturnOk);
}

#[test]
fn start_known_sessions_stops_before_current_identity_when_context_is_done() {
    let step = start_known_sessions_step(
        StartKnownSessionsEvent::Identities(vec![
            summary("alice", "did:alice"),
            summary("bob", "did:bob"),
        ]),
        &[
            StartKnownSessionEnsureOutcome::Ensure(EnsureSessionOutcome::Ok),
            StartKnownSessionEnsureOutcome::ContextDone("context deadline exceeded".to_string()),
        ],
    );

    assert_eq!(
        step.actions,
        vec![ListenerIdentityWatchAction::EnsureSession {
            identity_name: "alice".to_string(),
        }]
    );
    assert_eq!(
        step.decision,
        StartKnownSessionsDecision::ReturnError("context deadline exceeded".to_string())
    );
}

#[test]
fn watch_new_identities_context_done_stops_ticker_and_exits() {
    let step = watch_new_identities_step(WatchNewIdentitiesEvent::ContextDone, &[], |_| {
        EnsureSessionOutcome::Ok
    });

    assert_eq!(step.actions, vec![ListenerIdentityWatchAction::StopTicker]);
    assert_eq!(step.decision, WatchNewIdentitiesDecision::Exit);
}

#[test]
fn watch_new_identities_ignores_manager_list_error_without_refresh() {
    let step =
        watch_new_identities_step(WatchNewIdentitiesEvent::TickManagerListError, &[], |_| {
            EnsureSessionOutcome::Ok
        });

    assert!(step.actions.is_empty());
    assert_eq!(step.decision, WatchNewIdentitiesDecision::Continue);
}

#[test]
fn watch_new_identities_skips_existing_sessions_records_new_errors_and_refreshes() {
    let existing = vec!["alice".to_string()];
    let step = watch_new_identities_step(
        WatchNewIdentitiesEvent::TickIdentities(vec![
            summary("alice", "did:alice"),
            summary("bob", "did:bob"),
            summary("carol", "did:carol"),
        ]),
        &existing,
        |identity_name| {
            if identity_name == "carol" {
                EnsureSessionOutcome::Error("dial failed".to_string())
            } else {
                EnsureSessionOutcome::Ok
            }
        },
    );

    assert_eq!(
        step.actions,
        vec![
            ListenerIdentityWatchAction::EnsureSession {
                identity_name: "bob".to_string(),
            },
            ListenerIdentityWatchAction::EnsureSession {
                identity_name: "carol".to_string(),
            },
            ListenerIdentityWatchAction::RecordSessionError {
                identity_name: "carol".to_string(),
                did: "did:carol".to_string(),
                error: "dial failed".to_string(),
            },
            ListenerIdentityWatchAction::RefreshStatus,
            ListenerIdentityWatchAction::RefreshStatus,
        ]
    );
    assert_eq!(step.decision, WatchNewIdentitiesDecision::Continue);
}

#[test]
fn watch_new_identities_skips_duplicates_after_first_ensure_like_session_map() {
    let step = watch_new_identities_step(
        WatchNewIdentitiesEvent::TickIdentities(vec![
            summary("bob", "did:bob"),
            summary("bob", "did:bob-duplicate"),
        ]),
        &[],
        |_| EnsureSessionOutcome::Ok,
    );

    assert_eq!(
        step.actions,
        vec![
            ListenerIdentityWatchAction::EnsureSession {
                identity_name: "bob".to_string(),
            },
            ListenerIdentityWatchAction::RefreshStatus,
        ]
    );
}

fn summary(identity_name: &str, did: &str) -> ListenerIdentitySummary {
    ListenerIdentitySummary {
        identity_name: identity_name.to_string(),
        did: did.to_string(),
    }
}
