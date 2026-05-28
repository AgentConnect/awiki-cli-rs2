use std::path::PathBuf;

use im_core::prelude::*;

#[test]
fn realtime_service_api_shape_is_available_from_client_and_prelude() {
    let core = test_core();
    let client = core
        .client(IdentitySelector::LocalAlias("alice".to_string()))
        .unwrap();

    let status = client.realtime().status().unwrap();
    assert!(!status.connected);
    assert_eq!(status.state, RealtimeConnectionState::Disconnected);
    assert!(status.subscriptions.is_empty());
    assert!(status.last_error.is_none());
}

#[test]
fn realtime_options_default_is_async_session_ready() {
    let options = RealtimeOptions::default();

    assert_eq!(options.reconnect, ReconnectPolicy::Disabled);
    assert_eq!(options.event_buffer, 128);
    assert_eq!(
        options.subscriptions,
        vec![
            RealtimeSubscription::Messages,
            RealtimeSubscription::Groups,
            RealtimeSubscription::Notifications,
        ]
    );
}

#[tokio::test]
async fn realtime_start_async_enters_transport_boundary() {
    let core = test_core();
    let client = core
        .client(IdentitySelector::LocalAlias("alice".to_string()))
        .unwrap();

    let start = client
        .realtime()
        .start_async(RealtimeOptions::default())
        .await;
    assert!(matches!(start, Err(ImError::AuthRequired)));
}

#[tokio::test]
async fn realtime_start_async_exposes_session_stream_and_keeps_validation() {
    let core = test_core();
    let client = core
        .client(IdentitySelector::LocalAlias("alice".to_string()))
        .unwrap();

    let zero_buffer = client
        .realtime()
        .start_async(RealtimeOptions {
            event_buffer: 0,
            ..RealtimeOptions::default()
        })
        .await;
    assert!(matches!(
        zero_buffer,
        Err(ImError::InvalidInput { field: Some(field), .. }) if field == "event_buffer"
    ));

    let auth_required = client
        .realtime()
        .start_async(RealtimeOptions::default())
        .await;
    assert!(matches!(auth_required, Err(ImError::AuthRequired)));
}

#[tokio::test]
async fn realtime_options_validate_without_touching_cli_runtime() {
    let core = test_core();
    let client = core
        .client(IdentitySelector::LocalAlias("alice".to_string()))
        .unwrap();

    let zero_buffer = client
        .realtime()
        .start_async(RealtimeOptions {
            event_buffer: 0,
            ..RealtimeOptions::default()
        })
        .await;
    assert!(matches!(
        zero_buffer,
        Err(ImError::InvalidInput { field: Some(field), .. }) if field == "event_buffer"
    ));

    let bad_fixed = client
        .realtime()
        .start_async(RealtimeOptions {
            reconnect: ReconnectPolicy::Fixed {
                delay_ms: 0,
                max_attempts: Some(1),
            },
            ..RealtimeOptions::default()
        })
        .await;
    assert!(matches!(
        bad_fixed,
        Err(ImError::InvalidInput { field: Some(field), .. }) if field == "reconnect.delay_ms"
    ));

    let bad_exponential = client
        .realtime()
        .start_async(RealtimeOptions {
            reconnect: ReconnectPolicy::Exponential {
                base_delay_ms: 5000,
                max_delay_ms: 1000,
                max_attempts: None,
            },
            ..RealtimeOptions::default()
        })
        .await;
    assert!(matches!(
        bad_exponential,
        Err(ImError::InvalidInput { field: Some(field), .. }) if field == "reconnect"
    ));
}

#[test]
fn realtime_event_dtos_do_not_require_raw_websocket_frames() {
    let message_id = MessageId::parse("msg-1").unwrap();
    let thread = ThreadRef::Direct(PeerRef::parse("bob", "awiki.info").unwrap());
    let updated = ImEvent::MessageUpdated(MessageUpdatedEvent {
        message_id,
        thread,
        update_kind: MessageUpdateKind::Read,
    });
    assert!(matches!(
        updated,
        ImEvent::MessageUpdated(MessageUpdatedEvent {
            update_kind: MessageUpdateKind::Read,
            ..
        })
    ));

    let unknown = ImEvent::UnknownNotification(UnknownNotificationEvent {
        content_type: Some("application/octet-stream".to_string()),
        notification_type: Some("attachment".to_string()),
        reason: "unsupported body".to_string(),
    });
    assert!(matches!(
        unknown,
        ImEvent::UnknownNotification(UnknownNotificationEvent {
            content_type: Some(_),
            notification_type: Some(_),
            ..
        })
    ));
}

fn test_core() -> ImCore {
    ImCore::new(test_config(), test_paths()).unwrap()
}

fn test_config() -> ImCoreConfig {
    ImCoreConfig {
        service_base_url: ServiceEndpoint::parse("https://example.test").unwrap(),
        did_domain: "awiki.info".to_string(),
        user_service_endpoint: None,
        message_service_endpoint: None,
        mail_service_endpoint: None,
        anp_service_endpoint: None,
        anp_service_did: None,
        ca_bundle: None,
        transport_policy: MessageTransportPolicy::Auto,
    }
}

fn test_paths() -> ImCorePaths {
    let root = unique_temp_root();
    ImCorePaths {
        identities: IdentityRegistryPaths {
            identity_root_dir: root.join("identities"),
            registry_path: root.join("identities").join("registry.json"),
            default_identity_path: Some(root.join("identities").join("default")),
        },
        local_state: LocalStatePaths {
            sqlite_path: root.join("local").join("im.sqlite"),
        },
        runtime: RuntimePaths {
            cache_dir: root.join("cache"),
            temp_dir: root.join("tmp"),
        },
    }
}

fn unique_temp_root() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "im-core-realtime-api-{}-{nanos}",
        std::process::id()
    ))
}
