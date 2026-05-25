use awiki_cli::runtime_legacy::listener_secure_inbox_poll::{
    poll_unread_secure_direct_inbox_step, SecureInboxPollAction, SecureInboxPollDecision,
    SecureInboxPollEvent, SECURE_DIRECT_INBOX_POLL_INTERVAL,
};
use std::time::Duration;

#[test]
fn exported_poll_interval_matches_go_ticker_constant() {
    assert_eq!(SECURE_DIRECT_INBOX_POLL_INTERVAL, Duration::from_secs(2));
}

#[test]
fn start_runs_both_syncs_before_starting_two_second_ticker() {
    let step = poll_unread_secure_direct_inbox_step(SecureInboxPollEvent::Start);

    assert_eq!(
        step.actions,
        vec![
            SecureInboxPollAction::SyncUnreadSecureDirectInbox,
            SecureInboxPollAction::SyncPendingConfirmationSecureHistory,
            SecureInboxPollAction::StartTicker {
                interval: Duration::from_secs(2),
            },
        ]
    );
    assert_eq!(step.decision, SecureInboxPollDecision::Continue);
}

#[test]
fn ticker_tick_runs_both_syncs_in_go_order_and_continues() {
    let step = poll_unread_secure_direct_inbox_step(SecureInboxPollEvent::Tick);

    assert_eq!(
        step.actions,
        vec![
            SecureInboxPollAction::SyncUnreadSecureDirectInbox,
            SecureInboxPollAction::SyncPendingConfirmationSecureHistory,
        ]
    );
    assert_eq!(step.decision, SecureInboxPollDecision::Continue);
}

#[test]
fn context_done_stops_ticker_and_exits_without_syncing() {
    let step = poll_unread_secure_direct_inbox_step(SecureInboxPollEvent::ContextDone);

    assert_eq!(step.actions, vec![SecureInboxPollAction::StopTicker]);
    assert_eq!(step.decision, SecureInboxPollDecision::Exit);
}
