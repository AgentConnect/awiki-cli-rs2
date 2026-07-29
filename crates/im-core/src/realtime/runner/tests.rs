#[cfg(feature = "group-e2ee")]
use anp::group_e2ee::operations::{
    add_member_prepare, create_group_prepare, encrypt, finalize_commit, generate_key_package,
    process_welcome, AddMemberInput, CreateGroupInput, EncryptInput, FinalizeCommitInput,
    GenerateKeyPackageInput, ProcessWelcomeInput,
};
#[cfg(feature = "group-e2ee")]
use anp::group_e2ee::storage::ImCoreSqliteGroupMlsStore;
#[cfg(feature = "group-e2ee")]
use anp::group_e2ee::{GroupApplicationPlaintext, GroupStateRef};
use serde_json::json;
use std::collections::VecDeque;

use super::*;

#[test]
fn realtime_v2_product_recognition_is_profile_and_method_scoped() {
    let p5 = json!({
        "method": "direct.incoming",
        "params": {"meta": {"profile": anp::direct_e2ee::DIRECT_E2EE_PROFILE_V2}}
    });
    let p6_message = json!({
        "method": "group.incoming",
        "params": {"meta": {"profile": anp::group_e2ee::GROUP_E2EE_PROFILE_V2}}
    });
    let p6_notice = json!({
        "method": anp::group_e2ee::METHOD_GROUP_NOTICE_V2,
        "params": {"meta": {"profile": anp::group_e2ee::GROUP_E2EE_PROFILE_V2}}
    });

    assert!(is_p5_v2_realtime_candidate(&p5));
    assert!(is_p6_v2_realtime_candidate(&p6_message));
    assert!(is_p6_v2_realtime_candidate(&p6_notice));

    let mut wrong_method = p5.clone();
    wrong_method["method"] = json!("direct.other");
    assert!(!is_p5_v2_realtime_candidate(&wrong_method));

    let mut wrong_profile = p6_message;
    wrong_profile["params"]["meta"]["profile"] = json!("anp.group.e2ee.v1");
    assert!(!is_p6_v2_realtime_candidate(&wrong_profile));
}

#[test]
#[cfg(all(feature = "blocking", feature = "group-e2ee"))]
fn realtime_unknown_group_notice_is_hidden_without_leaking_control_material() {
    let fixture = TestClientFixture::new("unknown-p6-control");
    let client = fixture.client();
    let notification = json!({
        "method": anp::group_e2ee::METHOD_GROUP_NOTICE_V2,
        "params": {
            "meta": {"profile": "anp.group.e2ee.unknown"},
            "body": {"welcome_b64u": "SECRET-UNKNOWN-WELCOME"}
        }
    });
    let mut warnings = Vec::new();

    let projected = normalize_group_e2ee_realtime_notification_async_first(
        &client,
        None,
        notification,
        &mut warnings,
    );

    assert!(projected.is_none());
    assert_eq!(
        warnings,
        vec!["unknown group E2EE control notice was rejected"]
    );
    assert!(!format!("{warnings:?}").contains("SECRET-UNKNOWN-WELCOME"));
}

#[test]
#[cfg(feature = "blocking")]
fn realtime_local_state_projector_stores_direct_message_and_contact() {
    let fixture = TestClientFixture::new("direct");
    let client = fixture.client();
    let mut projector = LocalStateRealtimeNotificationProjector {
        client: &client,
        inner: FixedProjector {
            event: Some(super::super::ImEvent::MessageReceived(
                direct_message_event(client.did().as_str()),
            )),
        },
    };

    let outcome = projector.project(json!({"method": "test.direct"}));

    assert!(outcome.warnings.is_empty());
    assert!(matches!(
        outcome.event,
        Some(super::super::ImEvent::MessageReceived(_))
    ));
    let connection = rusqlite::Connection::open(fixture.sqlite_path()).unwrap();
    let message = connection
        .query_row(
            r#"
SELECT owner_identity_id, owner_did, sender_did, receiver_did, content_type, content, is_read, credential_name
FROM messages
WHERE msg_id = ?1 AND owner_did = ?2"#,
            rusqlite::params!["msg-direct-1", client.did().as_str()],
            |row| {
                Ok(StoredDirectMessage {
                    owner_identity_id: row.get(0)?,
                    owner_did: row.get(1)?,
                    sender_did: row.get(2)?,
                    receiver_did: row.get(3)?,
                    content_type: row.get(4)?,
                    content: row.get(5)?,
                    is_read: row.get::<_, i64>(6)?,
                    credential_name: row.get(7)?,
                })
            },
        )
        .unwrap();
    assert_eq!(message.owner_identity_id, "alice");
    assert_eq!(message.owner_did, client.did().as_str());
    assert_eq!(message.sender_did, "did:example:bob");
    assert_eq!(message.receiver_did, client.did().as_str());
    assert_eq!(message.content_type, "text/plain");
    assert_eq!(message.content, "hello from realtime");
    assert_eq!(message.is_read, 0);
    assert_eq!(message.credential_name, "alice");
    let contact_count: i64 = connection
        .query_row(
            r#"
SELECT COUNT(*)
FROM contacts
WHERE owner_identity_id = ?1 AND owner_did = ?2 AND did = ?3 AND messaged = 1"#,
            rusqlite::params!["alice", client.did().as_str(), "did:example:bob"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(contact_count, 1);
}

#[test]
fn realtime_sync_hint_parses_dirty_and_gap_without_checkpoint_owner_logic() {
    let notification = json!({
        "jsonrpc": "2.0",
        "method": "direct.incoming",
        "params": {
            "meta": {
                "sender_did": "did:example:bob",
                "target": {"kind": "agent", "did": "did:example:alice"},
                "message_id": "msg-sync-hint",
                "content_type": "text/plain"
            },
            "body": {"text": "hint"}
        },
        "sync": {
            "event_id": "sev-12",
            "event_seq": "12",
            "event_type": "message.created"
        }
    });

    let hint = crate::internal::realtime::projection::sync_hint_with_gap(&notification, Some("10"))
        .unwrap();

    assert_eq!(hint.event_id.as_deref(), Some("sev-12"));
    assert_eq!(hint.event_seq.as_deref(), Some("12"));
    assert_eq!(hint.event_type.as_deref(), Some("message.created"));
    assert!(hint.sync_dirty);
    assert!(hint.gap_detected);
    assert!(
        crate::internal::realtime::projection::sync_hint_with_gap(&notification, Some("11"))
            .unwrap()
            .sync_dirty
    );
    assert!(
        !crate::internal::realtime::projection::sync_hint_with_gap(&notification, Some("11"))
            .unwrap()
            .gap_detected
    );
}

#[test]
fn realtime_sync_hint_ignores_non_integral_event_seq() {
    let notification = json!({
        "method": "direct.incoming",
        "sync": {
            "event_id": "sev-float",
            "event_seq": 12.5,
            "event_type": "message.created"
        }
    });

    let hint = crate::internal::realtime::projection::sync_hint_with_gap(&notification, Some("11"))
        .unwrap();

    assert_eq!(hint.event_id.as_deref(), Some("sev-float"));
    assert_eq!(hint.event_seq, None);
    assert!(hint.gap_detected);
}

#[test]
#[cfg(feature = "blocking")]
fn realtime_gap_hint_projection_does_not_write_sync_checkpoint() {
    let fixture = TestClientFixture::new("realtime-no-checkpoint");
    let client = fixture.client();
    let mut projector = LocalStateRealtimeNotificationProjector {
        client: &client,
        inner: FixedProjector {
            event: Some(super::super::ImEvent::MessageReceived(
                direct_message_event_with_sync(client.did().as_str(), true),
            )),
        },
    };

    let outcome = projector.project(json!({"method": "test.direct"}));

    assert!(outcome.warnings.is_empty());
    let connection = rusqlite::Connection::open(fixture.sqlite_path()).unwrap();
    let checkpoint = crate::internal::local_state::sync_state::load_global_checkpoint(
        &connection,
        "alice",
        client.did().as_str(),
    )
    .unwrap();
    assert!(checkpoint.is_none());
}

#[test]
#[cfg(feature = "blocking")]
fn realtime_local_state_projector_stores_group_update() {
    let fixture = TestClientFixture::new("group");
    let client = fixture.client();
    let mut projector = LocalStateRealtimeNotificationProjector {
        client: &client,
        inner: FixedProjector {
            event: Some(super::super::ImEvent::GroupUpdated(
                super::super::GroupUpdatedEvent {
                    group: crate::ids::GroupRef::parse("did:example:group:blue").unwrap(),
                    update_kind: super::super::GroupUpdateKind::Updated,
                    event_type: None,
                    group_event_seq: None,
                    group_state_version: None,
                    actor_did: None,
                    subject_did: None,
                    subject_handle: None,
                    previous_subject_did: None,
                    handle_binding_generation: None,
                    membership_status: None,
                    changed_at: None,
                    sync: None,
                },
            )),
        },
    };

    let outcome = projector.project(json!({"method": "test.group"}));

    assert!(outcome.warnings.is_empty());
    assert!(matches!(
        outcome.event,
        Some(super::super::ImEvent::GroupUpdated(_))
    ));
    let connection = rusqlite::Connection::open(fixture.sqlite_path()).unwrap();
    let group = connection
        .query_row(
            r#"
SELECT owner_identity_id, owner_did, group_id, group_did, metadata, credential_name
FROM groups
WHERE owner_did = ?1 AND group_id = ?2"#,
            rusqlite::params![client.did().as_str(), "did:example:group:blue"],
            |row| {
                Ok(StoredGroup {
                    owner_identity_id: row.get(0)?,
                    owner_did: row.get(1)?,
                    group_id: row.get(2)?,
                    group_did: row.get(3)?,
                    metadata: row.get(4)?,
                    credential_name: row.get(5)?,
                })
            },
        )
        .unwrap();
    assert_eq!(group.owner_identity_id, "alice");
    assert_eq!(group.owner_did, client.did().as_str());
    assert_eq!(group.group_id, "did:example:group:blue");
    assert_eq!(group.group_did, "did:example:group:blue");
    assert_eq!(group.credential_name, "alice");
    let metadata = serde_json::from_str::<serde_json::Value>(&group.metadata).unwrap();
    assert_eq!(metadata["source"], "im-core.realtime");
    assert_eq!(metadata["update_kind"], "updated");
}

#[tokio::test]
async fn realtime_async_local_state_projector_uses_db_actor_for_message_projection() {
    let fixture = TestClientFixture::new("async-local-state");
    let client = fixture.client();
    let mut projector = AsyncLocalStateRealtimeNotificationProjector {
        client: client.clone(),
        inner: FixedProjector {
            event: Some(super::super::ImEvent::MessageReceived(
                direct_message_event(client.did().as_str()),
            )),
        },
    };

    let outcome = projector
        .project_async(json!({"method": "test.direct"}))
        .await;

    assert!(outcome.warnings.is_empty());
    assert!(matches!(
        outcome.event,
        Some(super::super::ImEvent::MessageReceived(_))
    ));
    let db = client.core_inner().local_state_db().await.unwrap();
    db.shutdown().await.unwrap();
    let connection = rusqlite::Connection::open(fixture.sqlite_path()).unwrap();
    let message = connection
        .query_row(
            r#"
SELECT owner_identity_id, owner_did, sender_did, receiver_did, content_type, content, is_read, credential_name
FROM messages
WHERE msg_id = ?1 AND owner_did = ?2"#,
            rusqlite::params!["msg-direct-1", client.did().as_str()],
            |row| {
                Ok(StoredDirectMessage {
                    owner_identity_id: row.get(0)?,
                    owner_did: row.get(1)?,
                    sender_did: row.get(2)?,
                    receiver_did: row.get(3)?,
                    content_type: row.get(4)?,
                    content: row.get(5)?,
                    is_read: row.get::<_, i64>(6)?,
                    credential_name: row.get(7)?,
                })
            },
        )
        .unwrap();
    assert_eq!(message.owner_identity_id, "alice");
    assert_eq!(message.owner_did, client.did().as_str());
    assert_eq!(message.sender_did, "did:example:bob");
    assert_eq!(message.receiver_did, client.did().as_str());
    assert_eq!(message.content_type, "text/plain");
    assert_eq!(message.content, "hello from realtime");
    assert_eq!(message.is_read, 0);
    assert_eq!(message.credential_name, "alice");
    let contact_count: i64 = connection
        .query_row(
            r#"
SELECT COUNT(*)
FROM contacts
WHERE owner_identity_id = ?1 AND owner_did = ?2 AND did = ?3 AND messaged = 1"#,
            rusqlite::params!["alice", client.did().as_str(), "did:example:bob"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(contact_count, 1);
}

#[tokio::test]
async fn realtime_verified_lookup_commits_first_inbound_direct_to_canonical_persona() {
    let fixture = TestClientFixture::new_without_verified_peer("async-first-inbound-persona");
    let client = fixture.client();
    let lookup = verified_bob_lookup();
    let expected_conversation_id = lookup.direct_conversation_id();

    project_realtime_message_received_async_with_lookup(
        &client,
        &direct_message_event(client.did().as_str()),
        Some(lookup.clone()),
    )
    .await
    .unwrap();

    let db = client.core_inner().local_state_db().await.unwrap();
    db.shutdown().await.unwrap();
    let connection = rusqlite::Connection::open(fixture.sqlite_path()).unwrap();
    let stored = connection
        .query_row(
            r#"SELECT conversation_id FROM messages
WHERE owner_identity_id = 'alice' AND msg_id = 'msg-direct-1'"#,
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap();
    assert_eq!(stored, expected_conversation_id);
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM peer_personas WHERE owner_identity_id = 'alice'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1
    );
    assert_eq!(
        crate::internal::local_state::inbound_resolution_backlog::pending_count(
            &connection,
            "alice",
        )
        .unwrap(),
        0
    );
}

#[tokio::test]
async fn realtime_async_local_state_projector_emits_committed_message_patches() {
    let fixture = TestClientFixture::new("async-local-state-patches");
    let client = fixture.client();
    let (conversation_patches, thread_patches) = watch_direct_realtime_patches(&client).await;

    project_realtime_message_received_async(&client, &direct_message_event(client.did().as_str()))
        .await
        .unwrap();

    assert_realtime_direct_patches(conversation_patches, thread_patches).await;
}

#[tokio::test]
async fn realtime_async_local_state_projector_does_not_emit_patch_without_projection() {
    let fixture = TestClientFixture::new("async-local-state-no-projection");
    let client = fixture.client();
    let (mut conversation_patches, mut thread_patches) =
        watch_direct_realtime_patches(&client).await;
    let mut event = direct_message_event(client.did().as_str());
    event.message.id = serde_json::from_value(json!("")).unwrap();

    project_realtime_message_received_async(&client, &event)
        .await
        .unwrap();

    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(100),
            conversation_patches.next_patch(),
        )
        .await
        .is_err(),
        "no projection must not emit conversation patch"
    );
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(100),
            thread_patches.next_patch(),
        )
        .await
        .is_err(),
        "no projection must not emit thread patch"
    );
}

#[tokio::test]
#[cfg(feature = "blocking")]
async fn realtime_blocking_local_state_projector_emits_committed_message_patches() {
    let fixture = TestClientFixture::new("blocking-local-state-patches");
    let client = fixture.client();
    let (conversation_patches, thread_patches) = watch_direct_realtime_patches(&client).await;

    project_realtime_message_received(&client, &direct_message_event(client.did().as_str()))
        .unwrap();

    assert_realtime_direct_patches(conversation_patches, thread_patches).await;
}

#[tokio::test]
async fn realtime_async_projector_uses_actor_cas_for_direct_cipher() {
    let fixture = TestClientFixture::new("async-direct-cipher");
    fixture.write_identity("did:example:alice", "test-key", "test-key");
    let client = fixture.client();
    let exchange =
        crate::internal::secure_direct::async_receive::test_support::established_exchange();
    let db = client.core_inner().local_state_db().await.unwrap();
    db.save_direct_secure_session_if_revision(
        crate::internal::secure_direct::sqlite_store::DirectSessionRecord {
            owner_identity_id: "alice".to_owned(),
            owner_did: "did:example:alice".to_owned(),
            peer_did: "did:example:bob".to_owned(),
            session_id: exchange.alice_session.session_id.clone(),
            state_blob: crate::internal::secure_direct::sqlite_store::direct_session_to_blob(
                &exchange.alice_session,
            )
            .unwrap(),
            metadata_json:
                crate::internal::secure_direct::sqlite_store::direct_session_metadata_json(
                    &exchange.alice_session,
                )
                .unwrap(),
            revision: 0,
            created_at: "2026-05-24T00:00:00Z".to_owned(),
            updated_at: "2026-05-24T00:00:00Z".to_owned(),
        },
        0,
    )
    .await
    .unwrap();
    let mut projector = AsyncSecureRealtimeNotificationProjector::new(&client);
    let outcome = projector
        .project_async(
            crate::internal::secure_direct::async_receive::test_support::direct_cipher_realtime_notification(
                &exchange.message_metadata,
                &exchange.cipher_body,
            ),
        )
        .await;

    assert!(outcome.warnings.is_empty());
    let Some(super::super::ImEvent::MessageReceived(event)) = outcome.event else {
        panic!("expected direct message event");
    };
    assert_eq!(event.message.id.as_str(), "msg-async-receive");
    assert_eq!(
        event.message.body,
        crate::messages::MessageBodyView::Text {
            text: "async receive secret".to_owned(),
            kind: crate::messages::MessageKind::Text,
        }
    );

    let saved = db
        .get_direct_secure_session("alice", "did:example:bob")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.revision, 1);
    db.shutdown().await.unwrap();
}

#[tokio::test]
async fn realtime_async_projector_replays_pending_direct_cipher_after_init() {
    let exchange =
        crate::internal::secure_direct::async_receive::test_support::incoming_init_exchange();
    let fixture = TestClientFixture::new("async-direct-pending-replay");
    fixture.write_identity(
        &exchange.recipient_did,
        "test-key",
        &exchange.recipient_agreement_private.to_pem(),
    );
    fixture.cache_identity_document(&exchange.sender_did, &exchange.sender_document);
    seed_direct_init_prekeys(&fixture, &exchange);
    let client = fixture.client();
    let mut sender_session = exchange.sender_session.clone();
    sender_session.status = anp::direct_e2ee::models::SESSION_STATUS_ESTABLISHED.to_owned();
    sender_session.recv_chain_key_b64u = sender_session.send_chain_key_b64u.clone();
    sender_session.peer_ratchet_public_key_b64u =
        Some(sender_session.ratchet_public_key_b64u.clone());
    sender_session.send_n = 1;
    let follow_up_metadata = anp::direct_e2ee::DirectEnvelopeMetadata {
        sender_did: exchange.sender_did.clone(),
        recipient_did: exchange.recipient_did.clone(),
        message_id: "msg-realtime-pending-follow-up".to_owned(),
        profile: "anp.direct.e2ee.v1".to_owned(),
        security_profile: "direct-e2ee".to_owned(),
    };
    let (_, follow_up_body) = anp::direct_e2ee::DirectE2eeSession::encrypt_follow_up(
        &mut sender_session,
        &follow_up_metadata,
        "msg-realtime-pending-follow-up",
        &anp::direct_e2ee::ApplicationPlaintext::new_text(
            "text/plain",
            "replayed realtime follow-up",
        ),
    )
    .unwrap();
    let mut projector = AsyncSecureRealtimeNotificationProjector::new(&client);

    let pending_outcome = projector
        .project_async(
            crate::internal::secure_direct::async_receive::test_support::direct_cipher_realtime_notification(
                &follow_up_metadata,
                &follow_up_body,
            ),
        )
        .await;

    assert!(pending_outcome.additional_events.is_empty());
    let init_outcome = projector
        .project_async(json!({
            "method": "direct.incoming",
            "params": crate::internal::secure_direct::async_receive::test_support::direct_init_notification(
                &exchange.metadata,
                &exchange.init_body,
            )
        }))
        .await;

    assert!(init_outcome.warnings.is_empty());
    let replayed = init_outcome
        .additional_events
        .iter()
        .find_map(|event| match event {
            super::super::ImEvent::MessageReceived(event)
                if event.message.id.as_str() == "msg-realtime-pending-follow-up" =>
            {
                Some(event)
            }
            _ => None,
        })
        .expect("expected pending direct cipher to replay after init");
    assert_eq!(
        replayed.message.body,
        crate::messages::MessageBodyView::Text {
            text: "replayed realtime follow-up".to_owned(),
            kind: crate::messages::MessageKind::Text,
        }
    );
    let db = client.core_inner().local_state_db().await.unwrap();
    let saved = db
        .get_direct_secure_session("alice", &exchange.sender_did)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.revision, 1);
    let saved_session =
        crate::internal::secure_direct::sqlite_store::direct_session_from_blob(&saved.state_blob)
            .unwrap();
    assert_eq!(saved_session.recv_n, 2);
    db.shutdown().await.unwrap();
}
#[cfg(feature = "group-e2ee")]
#[tokio::test]
async fn realtime_async_projector_uses_async_group_e2ee_normalizer() {
    let fixture = TestClientFixture::new("async-group-e2ee-realtime");
    fixture.write_identity_with_device(
        "did:wba:example.com:users:bob:e1",
        "test-key",
        "test-key",
        crate::internal::group_e2ee::DEFAULT_GROUP_MLS_DEVICE_ID,
    );
    let client = fixture.client();
    let group_did = "did:wba:example.com:groups:realtime-e2ee:e1";
    let cipher = prepare_group_e2ee_realtime_cipher(&fixture, group_did);
    let mut projector = AsyncSecureRealtimeNotificationProjector::new(&client);

    let outcome = projector
        .project_async(group_e2ee_realtime_notification(group_did, cipher))
        .await;

    assert!(outcome.warnings.is_empty());
    let Some(super::super::ImEvent::MessageReceived(event)) = outcome.event else {
        panic!("expected group message event");
    };
    assert_eq!(
        event.message.id.as_str(),
        "did:wba:example.com:groups:realtime-e2ee:e1:7"
    );
    assert_eq!(
        event.message.body,
        crate::messages::MessageBodyView::Text {
            text: "async realtime group secret".to_owned(),
            kind: crate::messages::MessageKind::Text,
        }
    );
    assert_eq!(event.message.group.as_ref().unwrap().as_str(), group_did);
}

#[tokio::test]
async fn realtime_async_runner_uses_tokio_channels_and_status_watch() {
    let options = super::super::RealtimeOptions {
        event_buffer: 8,
        ..super::super::RealtimeOptions::default()
    };
    let (sender, mut receiver) = tokio_mpsc::channel(options.event_buffer);
    let (status_sender, status_receiver) =
        watch::channel(super::super::session::initial_realtime_status(
            &options,
            super::super::RealtimeConnectionState::Disconnected,
            None,
        ));
    let events = TokioRunnerEvents {
        sender,
        status: status_sender,
        subscriptions: options.subscriptions.clone(),
    };
    let mut transport = FakeAsyncRealtimeTransport {
        connect_attempts: 0,
        connect_results: VecDeque::from([Ok(())]),
        notifications: VecDeque::from([
            Ok(Some(json!({
                "method": "local.notification",
                "params": {"id": "async-local-1", "title": "async title"}
            }))),
            Ok(None),
        ]),
        shutdown_on_next_notification: None,
    };
    let control = super::super::RealtimeControl::default();
    let mut projector = PlainRealtimeNotificationProjector;

    let exit = run_realtime_async_transport_until_shutdown(
        options,
        super::super::ShutdownSignal::pending(),
        control,
        &mut transport,
        events,
        &mut projector,
    )
    .await
    .unwrap();

    assert_eq!(
        exit.reason,
        super::super::RealtimeExitReason::ConnectionClosed
    );
    assert_eq!(transport.connect_attempts, 1);
    let mut observed = Vec::new();
    while let Some(event) = receiver.recv().await {
        observed.push(event);
    }
    assert!(matches!(
        observed.as_slice(),
        [
            super::super::ImEvent::ConnectionStateChanged(
                super::super::ConnectionStateChanged {
                    state: super::super::RealtimeConnectionState::Connecting,
                    ..
                },
            ),
            super::super::ImEvent::ConnectionStateChanged(
                super::super::ConnectionStateChanged {
                    state: super::super::RealtimeConnectionState::Connected,
                    ..
                },
            ),
            super::super::ImEvent::LocalNotification(
                super::super::LocalNotificationEvent {
                    notification_id: Some(id),
                    ..
                },
            ),
            super::super::ImEvent::ConnectionStateChanged(
                super::super::ConnectionStateChanged {
                    state: super::super::RealtimeConnectionState::Closed,
                    ..
                },
            ),
        ] if id == "async-local-1"
    ));
    assert_eq!(
        status_receiver.borrow().state,
        super::super::RealtimeConnectionState::Closed
    );
}

#[test]
fn plain_realtime_projector_never_falls_system_notifications_back_to_chat() {
    let mut projector = PlainRealtimeNotificationProjector;
    let outcome = projector.project(json!({
        "projection_kind": "system_notification",
        "method": "direct.incoming",
        "params": {
            "body": {
                "payload": {
                    "type": "awiki.device.join-requested.v1"
                }
            }
        }
    }));

    assert!(outcome.event.is_none());
    assert!(outcome.additional_events.is_empty());
    assert_eq!(
        outcome.warnings,
        vec!["system.notification.secure_projector_required".to_owned()]
    );
}

#[tokio::test]
async fn realtime_async_runner_stops_on_shutdown_signal() {
    let options = super::super::RealtimeOptions::default();
    let (sender, mut receiver) = tokio_mpsc::channel(options.event_buffer);
    let (status_sender, _status_receiver) =
        watch::channel(super::super::session::initial_realtime_status(
            &options,
            super::super::RealtimeConnectionState::Disconnected,
            None,
        ));
    let events = TokioRunnerEvents {
        sender,
        status: status_sender,
        subscriptions: options.subscriptions.clone(),
    };
    let control = super::super::RealtimeControl::default();
    let mut transport = FakeAsyncRealtimeTransport {
        connect_attempts: 0,
        connect_results: VecDeque::from([Ok(())]),
        notifications: VecDeque::from([Ok(Some(Value::Null))]),
        shutdown_on_next_notification: Some(control.clone()),
    };
    let mut projector = PlainRealtimeNotificationProjector;

    let exit = run_realtime_async_transport_until_shutdown(
        options,
        super::super::ShutdownSignal::pending(),
        control,
        &mut transport,
        events,
        &mut projector,
    )
    .await
    .unwrap();

    assert_eq!(
        exit.reason,
        super::super::RealtimeExitReason::ShutdownRequested
    );
    let events = {
        let mut observed = Vec::new();
        while let Some(event) = receiver.recv().await {
            observed.push(event);
        }
        observed
    };
    assert!(matches!(
        events.last(),
        Some(super::super::ImEvent::ConnectionStateChanged(
            super::super::ConnectionStateChanged {
                state: super::super::RealtimeConnectionState::Closed,
                reason: Some(reason),
            },
        )) if reason == "shutdown requested"
    ));
}

#[tokio::test]
async fn realtime_async_runner_exits_when_event_buffer_is_full() {
    let options = super::super::RealtimeOptions {
        event_buffer: 1,
        ..super::super::RealtimeOptions::default()
    };
    let (sender, receiver) = tokio_mpsc::channel(options.event_buffer);
    let (status_sender, status_receiver) =
        watch::channel(super::super::session::initial_realtime_status(
            &options,
            super::super::RealtimeConnectionState::Disconnected,
            None,
        ));
    let events = TokioRunnerEvents {
        sender,
        status: status_sender,
        subscriptions: options.subscriptions.clone(),
    };
    let mut transport = FakeAsyncRealtimeTransport {
        connect_attempts: 0,
        connect_results: VecDeque::from([Ok(())]),
        notifications: VecDeque::from([Ok(Some(json!({
            "method": "local.notification",
            "params": {"id": "buffered-local", "title": "buffered"}
        })))]),
        shutdown_on_next_notification: None,
    };
    let mut projector = PlainRealtimeNotificationProjector;

    let exit = run_realtime_async_transport_until_shutdown(
        options,
        super::super::ShutdownSignal::pending(),
        super::super::RealtimeControl::default(),
        &mut transport,
        events,
        &mut projector,
    )
    .await
    .unwrap();

    assert_eq!(
        exit.reason,
        super::super::RealtimeExitReason::ConnectionClosed
    );
    assert_eq!(
        exit.warnings,
        vec!["realtime event buffer is full or closed".to_owned()]
    );
    assert_eq!(
        status_receiver.borrow().state,
        super::super::RealtimeConnectionState::Closed
    );
    assert_eq!(
        status_receiver.borrow().last_error.as_deref(),
        Some("realtime event buffer is full or closed")
    );
    drop(receiver);
}

struct FixedProjector {
    event: Option<super::super::ImEvent>,
}

struct FakeAsyncRealtimeTransport {
    connect_attempts: usize,
    connect_results: VecDeque<crate::ImResult<()>>,
    notifications: VecDeque<crate::ImResult<Option<Value>>>,
    shutdown_on_next_notification: Option<super::super::RealtimeControl>,
}

impl AsyncRealtimeRunnerTransport for FakeAsyncRealtimeTransport {
    async fn connect(&mut self) -> crate::ImResult<()> {
        self.connect_attempts += 1;
        self.connect_results.pop_front().unwrap_or(Ok(()))
    }

    async fn next_notification(&mut self) -> crate::ImResult<Option<Value>> {
        if let Some(control) = self.shutdown_on_next_notification.take() {
            control.shutdown();
        }
        self.notifications.pop_front().unwrap_or(Ok(None))
    }
}

impl RealtimeNotificationProjector for FixedProjector {
    fn project(&mut self, _notification: serde_json::Value) -> RealtimeProjectionOutcome {
        RealtimeProjectionOutcome {
            event: self.event.take(),
            additional_events: Vec::new(),
            warnings: Vec::new(),
        }
    }
}

async fn watch_direct_realtime_patches(
    client: &crate::core::ImClient,
) -> (
    crate::messages::ConversationPatchSession,
    crate::messages::ThreadMessagePatchSession,
) {
    let mut conversation_patches = client.messages().watch_conversation_patches().unwrap();
    let mut thread_patches = client
        .messages()
        .watch_thread_patches(
            crate::messages::ThreadRef::Direct(
                crate::ids::PeerRef::parse("did:example:bob", "awiki.info").unwrap(),
            ),
            Some(100),
        )
        .unwrap();
    assert!(matches!(
        conversation_patches.next_patch().await,
        Some(crate::messages::ConversationStorePatch::Reset { .. })
    ));
    assert!(matches!(
        thread_patches.next_patch().await,
        Some(crate::messages::ThreadMessageStorePatch::Reset { .. })
    ));
    (conversation_patches, thread_patches)
}

async fn assert_realtime_direct_patches(
    mut conversation_patches: crate::messages::ConversationPatchSession,
    mut thread_patches: crate::messages::ThreadMessagePatchSession,
) {
    let conversation_patch = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        conversation_patches.next_patch(),
    )
    .await
    .expect("conversation patch emitted")
    .expect("conversation patch stream open");
    match conversation_patch {
        crate::messages::ConversationStorePatch::Upsert {
            owner_identity_id,
            owner_did,
            item,
            ..
        } => {
            assert_eq!(owner_identity_id, "alice");
            assert_eq!(owner_did, "did:example:alice");
            assert_eq!(item.thread_kind, "direct");
            assert_eq!(item.thread_id, "did:example:bob");
            let last_message = item.last_message.expect("last message projected");
            assert_eq!(last_message.id, "msg-direct-1");
            assert_eq!(
                last_message.body.text.as_deref(),
                Some("hello from realtime")
            );
            assert_eq!(item.unread_count, 1);
        }
        other => panic!("expected realtime conversation upsert patch, got {other:?}"),
    }

    let thread_patch = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        thread_patches.next_patch(),
    )
    .await
    .expect("thread patch emitted")
    .expect("thread patch stream open");
    match thread_patch {
        crate::messages::ThreadMessageStorePatch::Upsert {
            owner_identity_id,
            owner_did,
            thread_kind,
            thread_id,
            message,
            index,
            ..
        } => {
            assert_eq!(owner_identity_id, "alice");
            assert_eq!(owner_did, "did:example:alice");
            assert_eq!(thread_kind, "direct");
            assert_eq!(thread_id, "did:example:bob");
            assert_eq!(index, 0);
            assert_eq!(message.id.as_str(), "msg-direct-1");
            assert_eq!(
                message.body,
                crate::messages::MessageBodyView::Text {
                    text: "hello from realtime".to_owned(),
                    kind: crate::messages::MessageKind::Text,
                }
            );
        }
        other => panic!("expected realtime thread upsert patch, got {other:?}"),
    }
}

#[derive(Debug, PartialEq, Eq)]
struct StoredDirectMessage {
    owner_identity_id: String,
    owner_did: String,
    sender_did: String,
    receiver_did: String,
    content_type: String,
    content: String,
    is_read: i64,
    credential_name: String,
}

#[derive(Debug, PartialEq, Eq)]
struct StoredGroup {
    owner_identity_id: String,
    owner_did: String,
    group_id: String,
    group_did: String,
    metadata: String,
    credential_name: String,
}

struct TestClientFixture {
    root: std::path::PathBuf,
}

impl TestClientFixture {
    fn new(name: &str) -> Self {
        let fixture = Self::new_without_verified_peer(name);
        fixture.seed_verified_peer();
        fixture
    }

    fn new_without_verified_peer(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "im-core-realtime-runner-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("identities").join("alice")).unwrap();
        std::fs::create_dir_all(root.join("local")).unwrap();
        std::fs::write(root.join("identities").join("default"), "alice\n").unwrap();
        let fixture = Self { root };
        fixture.write_identity("did:example:alice", "test-key", "test-key");
        fixture
    }

    fn seed_verified_peer(&self) {
        let mut connection =
            crate::internal::local_state::open_writable(&self.sqlite_path()).unwrap();
        crate::internal::local_state::peer_personas::project_verified_handle(
            &mut connection,
            "alice",
            "did:example:alice",
            &verified_bob_lookup(),
        )
        .unwrap();
    }

    fn client(&self) -> crate::core::ImClient {
        self.core()
            .client(crate::identity::IdentitySelector::LocalAlias(
                "alice".to_string(),
            ))
            .unwrap()
    }

    fn core(&self) -> crate::core::ImCore {
        crate::core::ImCore::new_with_options(
            crate::ImCoreConfig {
                service_base_url: crate::ServiceEndpoint::parse("https://example.test").unwrap(),
                did_domain: "awiki.info".to_string(),
                user_service_endpoint: None,
                message_service_endpoint: None,
                mail_service_endpoint: None,
                anp_service_endpoint: None,
                anp_service_did: None,
                ca_bundle: None,
                transport_policy: crate::MessageTransportPolicy::Auto,
            },
            crate::ImCorePaths {
                identities: crate::IdentityRegistryPaths {
                    identity_root_dir: self.root.join("identities"),
                    registry_path: self.root.join("identities").join("registry.json"),
                    default_identity_path: Some(self.root.join("identities").join("default")),
                },
                local_state: crate::LocalStatePaths {
                    sqlite_path: self.sqlite_path(),
                },
                runtime: crate::RuntimePaths {
                    cache_dir: self.root.join("cache"),
                    temp_dir: self.root.join("tmp"),
                },
            },
            crate::ImCoreOpenOptions::default(),
        )
        .unwrap()
    }

    fn sqlite_path(&self) -> std::path::PathBuf {
        self.root.join("local").join("im.sqlite")
    }

    fn write_identity(&self, did: &str, signing_private_pem: &str, agreement_private_pem: &str) {
        self.write_identity_with_device(did, signing_private_pem, agreement_private_pem, "");
    }

    fn write_identity_with_device(
        &self,
        did: &str,
        signing_private_pem: &str,
        agreement_private_pem: &str,
        device_id: &str,
    ) {
        let identity_dir = self.root.join("identities").join("alice");
        let mut identity = json!({
            "id": "alice",
            "did": did,
            "local_alias": "alice",
            "ready_for_auth": true,
            "ready_for_messaging": true,
            "missing": []
        });
        if !device_id.trim().is_empty() {
            identity
                .as_object_mut()
                .unwrap()
                .insert("device_id".to_owned(), json!(device_id));
        }
        std::fs::write(
            self.root.join("identities").join("registry.json"),
            json!({
                "default_identity": "alice",
                "identities": [identity]
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            identity_dir.join("did.json"),
            json!({
                "id": did,
                "verificationMethod": [],
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(identity_dir.join("private.key"), signing_private_pem).unwrap();
        std::fs::write(
            identity_dir.join("e2ee-agreement-private.pem"),
            agreement_private_pem,
        )
        .unwrap();
        std::fs::write(
            identity_dir.join("auth.json"),
            r#"{"jwt_token":"test-token"}"#,
        )
        .unwrap();
    }

    fn cache_identity_document(&self, did: &str, document: &serde_json::Value) {
        let safe_name = did
            .chars()
            .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
            .collect::<String>();
        let identity_dir = self.root.join("identities").join(safe_name.clone());
        std::fs::create_dir_all(&identity_dir).unwrap();
        std::fs::write(
            identity_dir.join("did.json"),
            serde_json::to_vec_pretty(document).unwrap(),
        )
        .unwrap();
        let registry_path = self.root.join("identities").join("registry.json");
        let mut registry = std::fs::read_to_string(&registry_path)
            .ok()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
            .unwrap_or_else(|| {
                json!({
                    "default_identity": "alice",
                    "identities": []
                })
            });
        let identities = registry
            .get_mut("identities")
            .and_then(serde_json::Value::as_array_mut)
            .unwrap();
        identities.push(json!({
            "id": safe_name,
            "did": did,
            "dir_name": safe_name,
            "local_alias": safe_name,
            "ready_for_auth": false,
            "ready_for_messaging": false,
            "missing": []
        }));
        std::fs::write(registry_path, registry.to_string()).unwrap();
    }
}

fn verified_bob_lookup() -> crate::directory::HandleLookupResult {
    crate::directory::HandleLookupResult {
        handle: crate::ids::Handle::parse("bob.awiki.info", "").unwrap(),
        did: crate::ids::Did::parse("did:example:bob").unwrap(),
        user_id: "user-bob".to_owned(),
        domain: Some("awiki.info".to_owned()),
        status: Some("active".to_owned()),
        binding_generation: Some("1".to_owned()),
        profile: None,
        warnings: Vec::new(),
    }
}

fn seed_direct_init_prekeys(
    fixture: &TestClientFixture,
    exchange: &crate::internal::secure_direct::async_receive::test_support::IncomingInitExchange,
) {
    let connection = crate::internal::local_state::open_writable(&fixture.sqlite_path()).unwrap();
    let store = crate::internal::secure_direct::sqlite_store::SqliteDirectSecureStateStore::new(
        &connection,
    )
    .unwrap();
    store
        .upsert_signed_prekey(
            &crate::internal::secure_direct::sqlite_store::DirectSignedPrekeyRecord {
                owner_identity_id: "alice".to_owned(),
                owner_did: exchange.recipient_did.clone(),
                key_id: exchange.recipient_signed_prekey.key_id.clone(),
                private_key_blob: exchange
                    .recipient_signed_prekey_private
                    .to_pem()
                    .into_bytes(),
                public_key_blob: exchange
                    .recipient_signed_prekey
                    .public_key_b64u
                    .as_bytes()
                    .to_vec(),
                status: crate::internal::secure_direct::sqlite_store::DirectPrekeyStatus::Active,
                metadata_json: serde_json::to_string(&json!({
                    "metadata": exchange.recipient_signed_prekey,
                }))
                .unwrap(),
                created_at: "2026-05-24T00:00:00Z".to_owned(),
                updated_at: "2026-05-24T00:00:00Z".to_owned(),
            },
        )
        .unwrap();
    store
        .upsert_one_time_prekey(
            &crate::internal::secure_direct::sqlite_store::DirectOneTimePrekeyRecord {
                owner_identity_id: "alice".to_owned(),
                owner_did: exchange.recipient_did.clone(),
                key_id: exchange.recipient_one_time_prekey.key_id.clone(),
                private_key_blob: exchange
                    .recipient_one_time_prekey_private
                    .to_pem()
                    .into_bytes(),
                public_key_blob: exchange
                    .recipient_one_time_prekey
                    .public_key_b64u
                    .as_bytes()
                    .to_vec(),
                status: crate::internal::secure_direct::sqlite_store::DirectPrekeyStatus::Available,
                metadata_json: serde_json::to_string(&json!({
                    "metadata": exchange.recipient_one_time_prekey,
                }))
                .unwrap(),
                created_at: "2026-05-24T00:00:00Z".to_owned(),
                consumed_at: String::new(),
            },
        )
        .unwrap();
}

#[cfg(feature = "group-e2ee")]
fn prepare_group_e2ee_realtime_cipher(
    fixture: &TestClientFixture,
    group_did: &str,
) -> anp::group_e2ee::GroupCipherObject {
    let alice_store = ImCoreSqliteGroupMlsStore::from_local_state_sqlite_path(
        fixture.root.join("alice-local").join("im.sqlite"),
        "alice-sender",
        "did:wba:example.com:users:alice:e1",
        crate::internal::group_e2ee::DEFAULT_GROUP_MLS_DEVICE_ID,
    )
    .unwrap();
    let bob_store = ImCoreSqliteGroupMlsStore::from_local_state_sqlite_path(
        fixture.sqlite_path(),
        "alice",
        "did:wba:example.com:users:bob:e1",
        crate::internal::group_e2ee::DEFAULT_GROUP_MLS_DEVICE_ID,
    )
    .unwrap();
    let bob_key_package = generate_key_package(
        &bob_store,
        GenerateKeyPackageInput {
            owner_did: "did:wba:example.com:users:bob:e1".to_owned(),
            device_id: crate::internal::group_e2ee::DEFAULT_GROUP_MLS_DEVICE_ID.to_owned(),
            operation_id: "op-realtime-bob-kp".to_owned(),
            request_id: "req-realtime-bob-kp".to_owned(),
            key_package_id: None,
            purpose: None,
            group_did: Some(group_did.to_owned()),
        },
    )
    .unwrap();
    let create = create_group_prepare(
        &alice_store,
        CreateGroupInput {
            creator_did: "did:wba:example.com:users:alice:e1".to_owned(),
            device_id: crate::internal::group_e2ee::DEFAULT_GROUP_MLS_DEVICE_ID.to_owned(),
            group_did: group_did.to_owned(),
            operation_id: "op-realtime-create".to_owned(),
            request_id: "req-realtime-create".to_owned(),
            pending_commit_id: Some("pc-realtime-create".to_owned()),
        },
    )
    .unwrap();
    finalize_commit(
        &alice_store,
        FinalizeCommitInput {
            pending_commit_id: create.pending_commit_id,
            request_id: "req-realtime-create-finalize".to_owned(),
        },
    )
    .unwrap();
    let add = add_member_prepare(
        &alice_store,
        AddMemberInput {
            group_state_ref: None,
            actor_did: "did:wba:example.com:users:alice:e1".to_owned(),
            device_id: crate::internal::group_e2ee::DEFAULT_GROUP_MLS_DEVICE_ID.to_owned(),
            group_did: group_did.to_owned(),
            member_did: "did:wba:example.com:users:bob:e1".to_owned(),
            group_key_package: bob_key_package.group_key_package,
            operation_id: "op-realtime-add-bob".to_owned(),
            request_id: "req-realtime-add-bob".to_owned(),
            pending_commit_id: Some("pc-realtime-add-bob".to_owned()),
        },
    )
    .unwrap();
    finalize_commit(
        &alice_store,
        FinalizeCommitInput {
            pending_commit_id: add.pending_commit_id.clone(),
            request_id: "req-realtime-add-finalize".to_owned(),
        },
    )
    .unwrap();
    process_welcome(
        &bob_store,
        ProcessWelcomeInput {
            agent_did: "did:wba:example.com:users:bob:e1".to_owned(),
            device_id: crate::internal::group_e2ee::DEFAULT_GROUP_MLS_DEVICE_ID.to_owned(),
            group_did: group_did.to_owned(),
            welcome_b64u: add.welcome_b64u.expect("welcome"),
            ratchet_tree_b64u: add.ratchet_tree_b64u.expect("ratchet tree"),
            group_state_ref: GroupStateRef {
                group_did: group_did.to_owned(),
                group_state_version: "1".to_owned(),
                policy_hash: None,
            },
            crypto_group_id_b64u: add.crypto_group_id_b64u,
            epoch: add.epoch,
            request_id: "req-realtime-bob-welcome".to_owned(),
        },
    )
    .unwrap();
    encrypt(
        &alice_store,
        EncryptInput {
            sender_did: "did:wba:example.com:users:alice:e1".to_owned(),
            device_id: crate::internal::group_e2ee::DEFAULT_GROUP_MLS_DEVICE_ID.to_owned(),
            group_state_ref: GroupStateRef {
                group_did: group_did.to_owned(),
                group_state_version: "1".to_owned(),
                policy_hash: None,
            },
            message_id: "msg-group-realtime-e2ee".to_owned(),
            operation_id: "op-group-realtime-e2ee".to_owned(),
            application_plaintext: GroupApplicationPlaintext {
                application_content_type: "text/plain".to_owned(),
                thread_id: Some(group_did.to_owned()),
                reply_to_message_id: None,
                annotations: Default::default(),
                text: Some("async realtime group secret".to_owned()),
                payload: None,
                payload_b64u: None,
            },
            request_id: "req-realtime-encrypt".to_owned(),
        },
    )
    .unwrap()
    .group_cipher_object
}

#[cfg(feature = "group-e2ee")]
fn group_e2ee_realtime_notification(
    group_did: &str,
    cipher: anp::group_e2ee::GroupCipherObject,
) -> serde_json::Value {
    let mut body = serde_json::to_value(cipher).unwrap();
    if let Some(object) = body.as_object_mut() {
        object.insert("group_did".to_owned(), json!(group_did));
        object.insert("group_event_seq".to_owned(), json!(7));
    }
    json!({
        "method": "group.incoming",
        "params": {
            "meta": {
                "message_id": "msg-group-realtime-e2ee",
                "operation_id": "op-group-realtime-e2ee",
                "sender_did": "did:wba:example.com:users:alice:e1",
                "target": {
                    "kind": "group",
                    "did": group_did
                },
                "content_type": anp::group_e2ee::GROUP_CIPHER_CONTENT_TYPE,
                "created_at": "2026-05-24T00:00:00Z"
            },
            "body": body
        }
    })
}

fn direct_message_event(owner_did: &str) -> super::super::MessageReceivedEvent {
    direct_message_event_with_sync(owner_did, false)
}

fn direct_message_event_with_sync(
    owner_did: &str,
    include_sync: bool,
) -> super::super::MessageReceivedEvent {
    let sender = crate::ids::PeerRef::parse("did:example:bob", "awiki.info").unwrap();
    let receiver = crate::ids::PeerRef::parse(owner_did, "awiki.info").unwrap();
    super::super::MessageReceivedEvent {
        message: crate::messages::Message {
            id: crate::ids::MessageId::parse("msg-direct-1").unwrap(),
            thread: crate::messages::ThreadRef::Direct(sender.clone()),
            direction: crate::messages::MessageDirection::Incoming,
            sender,
            receiver: Some(receiver),
            group: None,
            body: crate::messages::MessageBodyView::Text {
                text: "hello from realtime".to_string(),
                kind: crate::messages::MessageKind::Text,
            },
            sent_at: Some("2026-05-25T00:00:00Z".to_string()),
            received_at: None,
            metadata: crate::messages::MessageMetadata {
                content_type: Some("text/plain".to_string()),
                ..crate::messages::MessageMetadata::default()
            },
        },
        attachment_summary: None,
        download_action: None,
        sync: include_sync.then(|| super::super::RealtimeSyncHint {
            event_id: Some("sev-99".to_owned()),
            event_seq: Some("99".to_owned()),
            event_type: Some("message.created".to_owned()),
            sync_dirty: true,
            gap_detected: true,
        }),
        warnings: Vec::new(),
    }
}
