#![cfg(feature = "blocking")]

use std::collections::VecDeque;

use im_core::prelude::*;
use im_core::realtime::{
    run_realtime_transport_until_shutdown, run_realtime_transport_with_event_sink_until_shutdown,
    RealtimeRunnerEventSink, RealtimeRunnerTransport,
};
use serde_json::{json, Value};

#[test]
fn realtime_runner_connects_projects_notification_and_exits_on_closed_stream() {
    let mut transport = FakeRunnerTransport {
        connect_attempts: 0,
        connect_results: VecDeque::from([Ok(())]),
        notifications: VecDeque::from([
            Ok(Some(json!({
                "method": "direct.incoming",
                "params": {
                    "meta": {
                        "message_id": "msg-1",
                        "sender_did": "did:example:bob",
                        "target": {"did": "did:example:alice"},
                        "content_type": "text/plain"
                    },
                    "body": {"text": "hello"}
                }
            }))),
            Ok(None),
        ]),
        shutdown_after_notifications: None,
    };
    let control = RealtimeControl::default();

    let outcome = run_realtime_transport_until_shutdown(
        RealtimeOptions::default(),
        ShutdownSignal::pending(),
        control.clone(),
        &mut transport,
    )
    .expect("runner exit");

    assert_eq!(outcome.exit.reason, RealtimeExitReason::ConnectionClosed);
    assert_eq!(outcome.exit.reconnect_attempts, 0);
    assert!(outcome.exit.warnings.is_empty());
    assert_eq!(transport.connect_attempts, 1);
    let events = outcome.handle.events.into_iter().collect::<Vec<_>>();
    assert!(matches!(
        events.as_slice(),
        [
            ImEvent::ConnectionStateChanged(ConnectionStateChanged {
                state: RealtimeConnectionState::Connecting,
                reason: None,
            }),
            ImEvent::ConnectionStateChanged(ConnectionStateChanged {
                state: RealtimeConnectionState::Connected,
                reason: None,
            }),
            ImEvent::MessageReceived(MessageReceivedEvent { message, .. }),
            ImEvent::ConnectionStateChanged(ConnectionStateChanged {
                state: RealtimeConnectionState::Closed,
                reason: None,
            }),
        ] if message.id.as_str() == "msg-1"
            && message.sender.as_str() == "did:example:bob"
            && message.body == (MessageBodyView::Text {
                text: "hello".to_string(),
                kind: MessageKind::Text,
            })
    ));
    assert!(!control.is_closed());
}

#[test]
fn realtime_runner_retries_connect_until_fixed_policy_attempts_are_exhausted() {
    let mut transport = FakeRunnerTransport {
        connect_attempts: 0,
        connect_results: VecDeque::from([
            Err(ImError::TransportUnavailable {
                detail: "dial one".to_string(),
            }),
            Err(ImError::TransportUnavailable {
                detail: "dial two".to_string(),
            }),
        ]),
        notifications: VecDeque::new(),
        shutdown_after_notifications: None,
    };

    let outcome = run_realtime_transport_until_shutdown(
        RealtimeOptions {
            reconnect: ReconnectPolicy::Fixed {
                delay_ms: 1,
                max_attempts: Some(1),
            },
            ..RealtimeOptions::default()
        },
        ShutdownSignal::pending(),
        RealtimeControl::default(),
        &mut transport,
    )
    .expect("runner exit");

    assert_eq!(
        outcome.exit.reason,
        RealtimeExitReason::TransportUnavailable
    );
    assert_eq!(outcome.exit.reconnect_attempts, 1);
    assert_eq!(
        outcome.exit.warnings,
        vec![
            "transport unavailable: dial one".to_string(),
            "transport unavailable: dial two".to_string(),
        ]
    );
    assert_eq!(transport.connect_attempts, 2);
    let states = outcome
        .handle
        .events
        .into_iter()
        .filter_map(|event| match event {
            ImEvent::ConnectionStateChanged(state) => Some(state.state),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        states,
        vec![
            RealtimeConnectionState::Connecting,
            RealtimeConnectionState::Reconnecting,
            RealtimeConnectionState::Disconnected,
        ]
    );
}

#[test]
fn realtime_runner_reconnects_after_closed_stream_when_policy_allows() {
    let mut transport = FakeRunnerTransport {
        connect_attempts: 0,
        connect_results: VecDeque::from([Ok(()), Ok(())]),
        notifications: VecDeque::from([
            Ok(Some(json!({
                "method": "local.notification",
                "params": {"id": "local-before-reconnect", "title": "before"}
            }))),
            Ok(None),
            Ok(Some(json!({
                "method": "local.notification",
                "params": {"id": "local-after-reconnect", "title": "after"}
            }))),
            Ok(None),
        ]),
        shutdown_after_notifications: None,
    };

    let outcome = run_realtime_transport_until_shutdown(
        RealtimeOptions {
            reconnect: ReconnectPolicy::Fixed {
                delay_ms: 1,
                max_attempts: Some(1),
            },
            ..RealtimeOptions::default()
        },
        ShutdownSignal::pending(),
        RealtimeControl::default(),
        &mut transport,
    )
    .expect("runner exit");

    assert_eq!(outcome.exit.reason, RealtimeExitReason::ConnectionClosed);
    assert_eq!(outcome.exit.reconnect_attempts, 1);
    assert!(outcome.exit.warnings.is_empty());
    assert_eq!(transport.connect_attempts, 2);

    let events = outcome.handle.events.into_iter().collect::<Vec<_>>();
    let states = events
        .iter()
        .filter_map(|event| match event {
            ImEvent::ConnectionStateChanged(state) => Some(state.state.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        states,
        vec![
            RealtimeConnectionState::Connecting,
            RealtimeConnectionState::Connected,
            RealtimeConnectionState::Reconnecting,
            RealtimeConnectionState::Connected,
            RealtimeConnectionState::Closed,
        ]
    );
    let local_ids = events
        .into_iter()
        .filter_map(|event| match event {
            ImEvent::LocalNotification(event) => event.notification_id,
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        local_ids,
        vec![
            "local-before-reconnect".to_string(),
            "local-after-reconnect".to_string(),
        ]
    );
}

#[test]
fn realtime_runner_control_shutdown_exits_after_current_notification() {
    let control = RealtimeControl::default();
    let mut transport = FakeRunnerTransport {
        connect_attempts: 0,
        connect_results: VecDeque::from([Ok(())]),
        notifications: VecDeque::from([Ok(Some(json!({
            "method": "local.notification",
            "params": {"id": "local-1", "title": "title"}
        })))]),
        shutdown_after_notifications: Some((1, control.clone())),
    };

    let outcome = run_realtime_transport_until_shutdown(
        RealtimeOptions::default(),
        ShutdownSignal::pending(),
        control.clone(),
        &mut transport,
    )
    .expect("runner exit");

    assert!(control.is_closed());
    assert_eq!(outcome.exit.reason, RealtimeExitReason::ShutdownRequested);
    let events = outcome.handle.events.into_iter().collect::<Vec<_>>();
    assert!(matches!(
        events.as_slice(),
        [
            ImEvent::ConnectionStateChanged(ConnectionStateChanged {
                state: RealtimeConnectionState::Connecting,
                reason: None,
            }),
            ImEvent::ConnectionStateChanged(ConnectionStateChanged {
                state: RealtimeConnectionState::Connected,
                reason: None,
            }),
            ImEvent::LocalNotification(LocalNotificationEvent {
                notification_id: Some(id),
                ..
            }),
            ImEvent::ConnectionStateChanged(ConnectionStateChanged {
                state: RealtimeConnectionState::Closed,
                reason: Some(reason),
            }),
        ] if id == "local-1" && reason == "shutdown requested"
    ));
}

#[test]
fn realtime_runner_external_shutdown_exits_before_connect() {
    let mut transport = FakeRunnerTransport {
        connect_attempts: 0,
        connect_results: VecDeque::from([Ok(())]),
        notifications: VecDeque::new(),
        shutdown_after_notifications: None,
    };

    let outcome = run_realtime_transport_until_shutdown(
        RealtimeOptions::default(),
        ShutdownSignal::requested(),
        RealtimeControl::default(),
        &mut transport,
    )
    .expect("runner exit");

    assert_eq!(outcome.exit.reason, RealtimeExitReason::ShutdownRequested);
    assert_eq!(transport.connect_attempts, 0);
    let events = outcome.handle.events.into_iter().collect::<Vec<_>>();
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0],
        ImEvent::ConnectionStateChanged(ConnectionStateChanged {
            state: RealtimeConnectionState::Closed,
            reason: Some(reason),
        }) if reason == "shutdown requested"
    ));
}

#[test]
fn realtime_runner_can_emit_through_migration_event_sink() {
    let mut transport = FakeRunnerTransport {
        connect_attempts: 0,
        connect_results: VecDeque::from([Ok(())]),
        notifications: VecDeque::from([
            Ok(Some(json!({
                "method": "local.notification",
                "params": {"id": "local-1", "title": "title"}
            }))),
            Ok(None),
        ]),
        shutdown_after_notifications: None,
    };
    let mut sink = RecordingSink::default();

    let outcome = run_realtime_transport_with_event_sink_until_shutdown(
        RealtimeOptions::default(),
        ShutdownSignal::pending(),
        RealtimeControl::default(),
        &mut transport,
        &mut sink,
    )
    .expect("runner exit");

    assert_eq!(outcome.exit.reason, RealtimeExitReason::ConnectionClosed);
    assert_eq!(transport.connect_attempts, 1);
    let handle_events = outcome.handle.events.into_iter().collect::<Vec<_>>();
    assert!(handle_events.is_empty());
    assert!(matches!(
        sink.events.as_slice(),
        [
            ImEvent::ConnectionStateChanged(ConnectionStateChanged {
                state: RealtimeConnectionState::Connecting,
                ..
            }),
            ImEvent::ConnectionStateChanged(ConnectionStateChanged {
                state: RealtimeConnectionState::Connected,
                ..
            }),
            ImEvent::LocalNotification(LocalNotificationEvent {
                notification_id: Some(id),
                ..
            }),
            ImEvent::ConnectionStateChanged(ConnectionStateChanged {
                state: RealtimeConnectionState::Closed,
                ..
            }),
        ] if id == "local-1"
    ));
}

#[test]
fn realtime_runner_maps_auth_error_to_auth_failed_exit() {
    let mut transport = FakeRunnerTransport {
        connect_attempts: 0,
        connect_results: VecDeque::from([Err(ImError::AuthRequired)]),
        notifications: VecDeque::new(),
        shutdown_after_notifications: None,
    };

    let outcome = run_realtime_transport_until_shutdown(
        RealtimeOptions::default(),
        ShutdownSignal::pending(),
        RealtimeControl::default(),
        &mut transport,
    )
    .expect("runner exit");

    assert_eq!(outcome.exit.reason, RealtimeExitReason::AuthFailed);
    assert_eq!(
        outcome.exit.warnings,
        vec!["authentication is required".to_string()]
    );
}

#[derive(Default)]
struct RecordingSink {
    events: Vec<ImEvent>,
}

impl RealtimeRunnerEventSink for RecordingSink {
    fn emit(&mut self, event: ImEvent) -> ImResult<()> {
        self.events.push(event);
        Ok(())
    }
}

struct FakeRunnerTransport {
    connect_attempts: usize,
    connect_results: VecDeque<ImResult<()>>,
    notifications: VecDeque<ImResult<Option<Value>>>,
    shutdown_after_notifications: Option<(usize, RealtimeControl)>,
}

impl FakeRunnerTransport {
    fn maybe_shutdown(&mut self) {
        let Some((remaining, control)) = self.shutdown_after_notifications.as_mut() else {
            return;
        };
        *remaining = remaining.saturating_sub(1);
        if *remaining == 0 {
            control.shutdown();
        }
    }
}

impl RealtimeRunnerTransport for FakeRunnerTransport {
    fn connect(&mut self) -> ImResult<()> {
        let result = self.connect_results.pop_front().unwrap_or(Ok(()));
        self.connect_attempts += 1;
        result
    }

    fn next_notification(&mut self) -> ImResult<Option<Value>> {
        let result = self.notifications.pop_front().unwrap_or(Ok(None));
        self.maybe_shutdown();
        result
    }
}
