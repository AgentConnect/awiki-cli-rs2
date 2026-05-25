use std::time::Duration;

pub const SECURE_DIRECT_INBOX_POLL_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecureInboxPollEvent {
    Start,
    Tick,
    ContextDone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecureInboxPollAction {
    SyncUnreadSecureDirectInbox,
    SyncPendingConfirmationSecureHistory,
    StartTicker { interval: Duration },
    StopTicker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecureInboxPollDecision {
    Continue,
    Exit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecureInboxPollStep {
    pub actions: Vec<SecureInboxPollAction>,
    pub decision: SecureInboxPollDecision,
}

pub fn poll_unread_secure_direct_inbox_step(event: SecureInboxPollEvent) -> SecureInboxPollStep {
    match event {
        SecureInboxPollEvent::Start => {
            let mut actions = sync_pair_actions();
            actions.push(SecureInboxPollAction::StartTicker {
                interval: SECURE_DIRECT_INBOX_POLL_INTERVAL,
            });
            SecureInboxPollStep {
                actions,
                decision: SecureInboxPollDecision::Continue,
            }
        }
        SecureInboxPollEvent::Tick => SecureInboxPollStep {
            actions: sync_pair_actions(),
            decision: SecureInboxPollDecision::Continue,
        },
        SecureInboxPollEvent::ContextDone => SecureInboxPollStep {
            actions: vec![SecureInboxPollAction::StopTicker],
            decision: SecureInboxPollDecision::Exit,
        },
    }
}

fn sync_pair_actions() -> Vec<SecureInboxPollAction> {
    vec![
        SecureInboxPollAction::SyncUnreadSecureDirectInbox,
        SecureInboxPollAction::SyncPendingConfirmationSecureHistory,
    ]
}
