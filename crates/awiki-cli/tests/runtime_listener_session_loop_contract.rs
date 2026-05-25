use awiki_cli::runtime_legacy::listener_session_loop::{
    secure_prekey_retry_decision, session_loop_start_decision, ConnectedSessionAction,
    ConsumeFinishedAction, ConsumeFinishedDecision, ContextSleep, InitialSessionSignal,
    SecurePrekeyRetryDecision, SessionLoopBackoff, SessionLoopRetryDecision, SessionLoopRetryPhase,
    SessionLoopStartDecision, CONNECTED_SESSION_ACTIONS, CONSUME_FINISHED_ACTIONS,
    SECURE_PREKEY_RETRY_DELAY, SESSION_RECONNECT_BASE_DELAY, SESSION_RECONNECT_MAX_DELAY,
};
use std::time::Duration;

#[test]
fn session_loop_start_closes_current_client_when_context_is_already_cancelled() {
    assert_eq!(
        session_loop_start_decision(true),
        SessionLoopStartDecision::CloseCurrentClientAndExit
    );
    assert_eq!(
        session_loop_start_decision(false),
        SessionLoopStartDecision::Connect
    );
}

#[test]
fn connect_failures_sleep_current_delay_then_double_until_capped() {
    let mut backoff = SessionLoopBackoff::new();
    let mut sleeps = Vec::new();
    let mut next_delays = Vec::new();

    for _ in 0..7 {
        match backoff.connect_failed(ContextSleep::Completed) {
            SessionLoopRetryDecision::Retry {
                phase,
                sleep_delay,
                next_delay,
            } => {
                assert_eq!(phase, SessionLoopRetryPhase::ConnectFailure);
                sleeps.push(sleep_delay);
                next_delays.push(next_delay);
            }
            other => panic!("expected retry, got {other:?}"),
        }
    }

    assert_eq!(
        sleeps,
        vec![
            Duration::from_secs(1),
            Duration::from_secs(2),
            Duration::from_secs(4),
            Duration::from_secs(8),
            Duration::from_secs(16),
            Duration::from_secs(30),
            Duration::from_secs(30),
        ]
    );
    assert_eq!(
        next_delays,
        vec![
            Duration::from_secs(2),
            Duration::from_secs(4),
            Duration::from_secs(8),
            Duration::from_secs(16),
            Duration::from_secs(30),
            Duration::from_secs(30),
            Duration::from_secs(30),
        ]
    );
}

#[test]
fn successful_connect_resets_increased_delay_before_connected_work() {
    let mut backoff = SessionLoopBackoff::new();
    backoff.connect_failed(ContextSleep::Completed);
    backoff.connect_failed(ContextSleep::Completed);
    assert_eq!(backoff.next_delay(), Duration::from_secs(4));

    assert_eq!(backoff.connected(), SESSION_RECONNECT_BASE_DELAY);
    assert_eq!(backoff.next_delay(), SESSION_RECONNECT_BASE_DELAY);
    assert_eq!(
        CONNECTED_SESSION_ACTIONS,
        &[
            ConnectedSessionAction::MarkConnected,
            ConnectedSessionAction::SignalInitialSuccess,
            ConnectedSessionAction::RefreshStatus,
            ConnectedSessionAction::FlushQueuedLocalNotifications,
            ConnectedSessionAction::StartSecurePrekeyRetry,
            ConnectedSessionAction::StartUnreadSecureDirectInboxPolling,
            ConnectedSessionAction::ConsumeNotifications,
        ]
    );
}

#[test]
fn consume_failure_uses_same_backoff_after_success_reset() {
    let mut backoff = SessionLoopBackoff::new();
    backoff.connect_failed(ContextSleep::Completed);
    backoff.connect_failed(ContextSleep::Completed);
    backoff.connected();

    let decision = backoff.consume_finished(false, ContextSleep::Completed);

    assert_eq!(
        decision,
        ConsumeFinishedDecision::Sleep(SessionLoopRetryDecision::Retry {
            phase: SessionLoopRetryPhase::ConsumeFailure,
            sleep_delay: Duration::from_secs(1),
            next_delay: Duration::from_secs(2),
        })
    );
    assert_eq!(backoff.next_delay(), Duration::from_secs(2));
    assert_eq!(
        CONSUME_FINISHED_ACTIONS,
        &[
            ConsumeFinishedAction::CancelChildTasks,
            ConsumeFinishedAction::CloseClient,
            ConsumeFinishedAction::MarkDisconnected,
            ConsumeFinishedAction::RefreshStatus,
        ]
    );
}

#[test]
fn context_cancellation_during_sleep_exits_without_doubling_delay() {
    let mut backoff = SessionLoopBackoff::new();

    let decision = backoff.connect_failed(ContextSleep::Cancelled);

    assert_eq!(
        decision,
        SessionLoopRetryDecision::ExitAfterCancelledSleep {
            phase: SessionLoopRetryPhase::ConnectFailure,
            sleep_delay: SESSION_RECONNECT_BASE_DELAY,
            next_delay: SESSION_RECONNECT_BASE_DELAY,
        }
    );
    assert_eq!(backoff.next_delay(), SESSION_RECONNECT_BASE_DELAY);
}

#[test]
fn context_cancellation_after_consume_exits_without_sleep_or_doubling() {
    let mut backoff = SessionLoopBackoff::new();
    backoff.connected();

    let decision = backoff.consume_finished(true, ContextSleep::Completed);

    assert_eq!(
        decision,
        ConsumeFinishedDecision::ExitAfterSessionCancelled {
            next_delay: SESSION_RECONNECT_BASE_DELAY,
        }
    );
    assert_eq!(backoff.next_delay(), SESSION_RECONNECT_BASE_DELAY);
}

#[test]
fn initial_signal_is_one_shot_like_go_sync_once() {
    let mut signal = InitialSessionSignal::default();

    assert!(!signal.has_signaled());
    assert!(signal.signal());
    assert!(signal.has_signaled());
    assert!(!signal.signal());
}

#[test]
fn secure_prekey_retry_finishes_on_empty_warnings() {
    assert_eq!(
        secure_prekey_retry_decision("alice", &[]),
        SecurePrekeyRetryDecision::Finished
    );
}

#[test]
fn secure_prekey_retry_logs_joined_warnings_and_sleeps_one_second() {
    let warnings = vec![
        "missing signed prekey".to_string(),
        "publish failed".to_string(),
    ];

    assert_eq!(
        secure_prekey_retry_decision("alice", &warnings),
        SecurePrekeyRetryDecision::Retry {
            log_line: "listener secure prekey publish retry identity=alice warnings=missing signed prekey; publish failed".to_string(),
            sleep_delay: SECURE_PREKEY_RETRY_DELAY,
        }
    );
    assert_eq!(SECURE_PREKEY_RETRY_DELAY, Duration::from_secs(1));
}

#[test]
fn exported_delays_match_go_constants() {
    assert_eq!(SESSION_RECONNECT_BASE_DELAY, Duration::from_secs(1));
    assert_eq!(SESSION_RECONNECT_MAX_DELAY, Duration::from_secs(30));
}
