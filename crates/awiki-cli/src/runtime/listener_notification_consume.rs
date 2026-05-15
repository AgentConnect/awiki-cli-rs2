use serde_json::Value;
use std::time::Duration;

pub const SESSION_PING_INTERVAL: Duration = Duration::from_secs(60);
pub const SESSION_PING_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotificationPingOutcome {
    Ok,
    Error(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConsumeNotificationsEvent {
    ContextDone { error: String },
    PingTick { outcome: NotificationPingOutcome },
    Notification(Value),
    NotificationsClosed { reader_error: Option<String> },
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConsumeNotificationsAction {
    StartPing { timeout: Duration },
    CancelPing,
    HandleNotification(Value),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsumeNotificationsDecision {
    Continue,
    Exit { error: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConsumeNotificationsStep {
    pub actions: Vec<ConsumeNotificationsAction>,
    pub decision: ConsumeNotificationsDecision,
}

pub fn consume_notifications_step(event: ConsumeNotificationsEvent) -> ConsumeNotificationsStep {
    match event {
        ConsumeNotificationsEvent::ContextDone { error } => ConsumeNotificationsStep {
            actions: Vec::new(),
            decision: ConsumeNotificationsDecision::Exit { error },
        },
        ConsumeNotificationsEvent::PingTick { outcome } => consume_ping_tick(outcome),
        ConsumeNotificationsEvent::Notification(notification) => ConsumeNotificationsStep {
            actions: vec![ConsumeNotificationsAction::HandleNotification(notification)],
            decision: ConsumeNotificationsDecision::Continue,
        },
        ConsumeNotificationsEvent::NotificationsClosed { reader_error } => {
            ConsumeNotificationsStep {
                actions: Vec::new(),
                decision: ConsumeNotificationsDecision::Exit {
                    error: reader_error
                        .unwrap_or_else(|| "websocket notification loop closed".to_string()),
                },
            }
        }
    }
}

fn consume_ping_tick(outcome: NotificationPingOutcome) -> ConsumeNotificationsStep {
    let mut actions = vec![ConsumeNotificationsAction::StartPing {
        timeout: SESSION_PING_TIMEOUT,
    }];
    let decision = match outcome {
        NotificationPingOutcome::Ok => ConsumeNotificationsDecision::Continue,
        NotificationPingOutcome::Error(error) => ConsumeNotificationsDecision::Exit {
            error: format!("websocket ping failed: {error}"),
        },
    };
    actions.push(ConsumeNotificationsAction::CancelPing);
    ConsumeNotificationsStep { actions, decision }
}
