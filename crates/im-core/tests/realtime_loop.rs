use awiki_im_core::realtime::{
    consume_notifications_step, secure_prekey_retry_decision, session_loop_start_decision,
    shutdown_decision, ConnectedSessionAction, ConsumeFinishedAction, ConsumeFinishedDecision,
    ConsumeNotificationsAction, ConsumeNotificationsDecision, ConsumeNotificationsEvent,
    ContextSleep, InitialSessionSignal, NotificationPingOutcome, RealtimeShutdownDecision,
    SecurePrekeyRetryDecision, SessionLoopBackoff, SessionLoopRetryDecision, SessionLoopRetryPhase,
    SessionLoopStartDecision, CONNECTED_SESSION_ACTIONS, CONSUME_FINISHED_ACTIONS,
    SECURE_PREKEY_RETRY_DELAY, SESSION_PING_INTERVAL, SESSION_PING_TIMEOUT,
    SESSION_RECONNECT_BASE_DELAY, SESSION_RECONNECT_MAX_DELAY,
};
use serde_json::json;
use std::time::Duration;

#[test]
fn realtime_loop_start_closes_current_client_when_context_is_already_cancelled() {
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
fn realtime_loop_connect_failures_sleep_current_delay_then_double_until_capped() {
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
fn realtime_loop_successful_connect_resets_delay_before_connected_work() {
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
fn realtime_loop_consume_failure_uses_same_backoff_after_success_reset() {
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
fn realtime_loop_context_cancellation_during_sleep_exits_without_doubling_delay() {
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
fn realtime_loop_context_cancellation_after_consume_exits_without_sleep_or_doubling() {
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
fn realtime_loop_initial_signal_is_one_shot() {
    let mut signal = InitialSessionSignal::default();

    assert!(!signal.has_signaled());
    assert!(signal.signal());
    assert!(signal.has_signaled());
    assert!(!signal.signal());
}

#[test]
fn realtime_loop_secure_prekey_retry_finishes_on_empty_warnings() {
    assert_eq!(
        secure_prekey_retry_decision("alice", &[]),
        SecurePrekeyRetryDecision::Finished
    );
}

#[test]
fn realtime_loop_secure_prekey_retry_logs_joined_warnings_and_sleeps_one_second() {
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
fn realtime_loop_exported_delays_match_current_constants() {
    assert_eq!(SESSION_RECONNECT_BASE_DELAY, Duration::from_secs(1));
    assert_eq!(SESSION_RECONNECT_MAX_DELAY, Duration::from_secs(30));
    assert_eq!(SESSION_PING_INTERVAL, Duration::from_secs(20));
    assert_eq!(SESSION_PING_TIMEOUT, Duration::from_secs(15));
}

#[test]
fn realtime_loop_ping_tick_starts_fifteen_second_ping_and_cancels_context_on_success() {
    let step = consume_notifications_step(ConsumeNotificationsEvent::PingTick {
        outcome: NotificationPingOutcome::Ok,
    });

    assert_eq!(
        step.actions,
        vec![
            ConsumeNotificationsAction::StartPing {
                timeout: SESSION_PING_TIMEOUT,
            },
            ConsumeNotificationsAction::CancelPing,
        ]
    );
    assert_eq!(step.decision, ConsumeNotificationsDecision::Continue);
}

#[test]
fn realtime_loop_ping_error_wraps_legacy_message_after_cancel_action() {
    let step = consume_notifications_step(ConsumeNotificationsEvent::PingTick {
        outcome: NotificationPingOutcome::Error("broken pipe".to_string()),
    });

    assert_eq!(
        step.actions,
        vec![
            ConsumeNotificationsAction::StartPing {
                timeout: Duration::from_secs(15),
            },
            ConsumeNotificationsAction::CancelPing,
        ]
    );
    assert_eq!(
        step.decision,
        ConsumeNotificationsDecision::Exit {
            error: "websocket ping failed: broken pipe".to_string(),
        }
    );
}

#[test]
fn realtime_loop_notification_event_plans_handle_notification_and_continues() {
    let notification = json!({
        "method": "direct.incoming",
        "params": {"body": {"text": "hello"}}
    });
    let step = consume_notifications_step(ConsumeNotificationsEvent::Notification(
        notification.clone(),
    ));

    assert_eq!(
        step.actions,
        vec![ConsumeNotificationsAction::HandleNotification(notification)]
    );
    assert_eq!(step.decision, ConsumeNotificationsDecision::Continue);
}

#[test]
fn realtime_loop_notification_channel_closed_prefers_reader_error() {
    let step = consume_notifications_step(ConsumeNotificationsEvent::NotificationsClosed {
        reader_error: Some("reader failed".to_string()),
    });

    assert!(step.actions.is_empty());
    assert_eq!(
        step.decision,
        ConsumeNotificationsDecision::Exit {
            error: "reader failed".to_string(),
        }
    );
}

#[test]
fn realtime_loop_notification_channel_closed_without_reader_error_uses_fixed_error() {
    let step = consume_notifications_step(ConsumeNotificationsEvent::NotificationsClosed {
        reader_error: None,
    });

    assert!(step.actions.is_empty());
    assert_eq!(
        step.decision,
        ConsumeNotificationsDecision::Exit {
            error: "websocket notification loop closed".to_string(),
        }
    );
}

#[test]
fn realtime_loop_shutdown_decision_exits_only_when_requested() {
    assert_eq!(shutdown_decision(false), RealtimeShutdownDecision::Continue);
    assert_eq!(shutdown_decision(true), RealtimeShutdownDecision::Exit);
}
