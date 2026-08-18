use awiki_cli::host_runtime::listener_notification_consume::{
    consume_notifications_step, ConsumeNotificationsAction, ConsumeNotificationsDecision,
    ConsumeNotificationsEvent, NotificationPingOutcome, SESSION_PING_INTERVAL,
    SESSION_PING_TIMEOUT,
};
use serde_json::json;
use std::time::Duration;

#[test]
fn exported_ping_durations_match_reliable_sync_contract() {
    assert_eq!(SESSION_PING_INTERVAL, Duration::from_secs(20));
    assert_eq!(SESSION_PING_TIMEOUT, Duration::from_secs(15));
}

#[test]
fn context_done_exits_with_context_error_without_actions() {
    let step = consume_notifications_step(ConsumeNotificationsEvent::ContextDone {
        error: "context canceled".to_string(),
    });

    assert!(step.actions.is_empty());
    assert_eq!(
        step.decision,
        ConsumeNotificationsDecision::Exit {
            error: "context canceled".to_string(),
        }
    );
}

#[test]
fn ping_tick_starts_fifteen_second_ping_and_cancels_context_on_success() {
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
fn ping_error_wraps_go_message_after_cancel_action() {
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
fn notification_event_plans_handle_notification_and_continues() {
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
fn notification_channel_closed_prefers_reader_error() {
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
fn notification_channel_closed_without_reader_error_uses_go_fixed_error() {
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
