use super::*;
use crate::internal::auth::session::SessionProvider;
use crate::internal::transport::{
    AsyncAuthenticatedRpcTransport, AsyncRpcTransport, AuthenticatedRpcTransport, RpcTransport,
};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;

#[test]
fn system_notification_delta_is_checkpoint_only_and_never_projects_chat_state() {
    let fixture = Fixture::new("sync-delta-system-notification");
    let client = fixture.client();
    let raw = delta_page(
        vec![json!({
            "event_id": "sev-system-1",
            "event_seq": "1",
            "event_type": "system.notification",
            "aggregate_kind": "device",
            "aggregate_id": "dev-local",
            "owner_subject_id": "did:example:alice",
            "created_at": "2026-07-23T02:00:00Z",
            "payload": {
                "projection_kind": "system_notification"
            }
        })],
        "1",
        false,
    );
    let page = crate::internal::wire::sync::parse_sync_delta_page(&raw).unwrap();
    let apply = sync_delta_apply_event(&client, &page.events[0], &Default::default()).unwrap();

    assert_eq!(apply.event_type, "system.notification");
    assert!(apply.messages.is_empty());
    assert!(apply.groups.is_empty());
}

#[test]
fn chat_shaped_delta_with_system_marker_is_also_checkpoint_only() {
    let fixture = Fixture::new("sync-delta-system-marker");
    let client = fixture.client();
    let raw = delta_page(
        vec![json!({
            "event_id": "sev-system-2",
            "event_seq": "2",
            "event_type": "message.created",
            "aggregate_kind": "direct_message",
            "aggregate_id": "must-not-be-chat",
            "owner_subject_id": "did:example:alice",
            "created_at": "2026-07-23T02:00:00Z",
            "payload": {
                "message": {
                    "projection_kind": "system_notification",
                    "content": {
                        "type": "awiki.device.join-requested.v1"
                    }
                }
            }
        })],
        "2",
        false,
    );
    let page = crate::internal::wire::sync::parse_sync_delta_page(&raw).unwrap();
    let apply = sync_delta_apply_event(&client, &page.events[0], &Default::default()).unwrap();

    assert!(apply.messages.is_empty());
    assert!(apply.groups.is_empty());
}

#[tokio::test]
async fn sync_delta_reads_checkpoint_calls_wire_and_advances_checkpoint() {
    let fixture = Fixture::new("sync-delta-basic");
    fixture.seed_verified_peer();
    let client = fixture.client();
    fixture.store_checkpoint("4");
    let calls = Rc::new(RefCell::new(Vec::new()));
    let runtime = MessageSyncRuntime::new(
        &client,
        ReadyAnySessionProvider,
        RecordingTransport::queued(
            Rc::clone(&calls),
            vec![delta_page(
                vec![message_created_event("sev-5", "5", "msg-delta-5", 5)],
                "5",
                false,
            )],
        ),
        NoopDirectoryTransport,
    );

    let result = runtime
        .sync_delta_async(SyncDeltaInput {
            request: crate::messages::SyncDeltaRequest {
                limit: Some(50),
                device_id: Some("device-a".to_owned()),
                reason: Some("app_resumed".to_owned()),
            },
        })
        .await
        .unwrap();

    assert_eq!(result.events_applied, 1);
    assert_eq!(result.pages_fetched, 1);
    assert_eq!(result.last_applied_event_seq.as_deref(), Some("5"));
    assert_eq!(fixture.checkpoint(), Some("5".to_owned()));
    assert_eq!(fixture.message_server_seq("msg-delta-5"), Some(5));
    let calls = calls.borrow();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].method, "sync.delta");
    assert_eq!(calls[0].params["body"]["since_event_seq"], "4");
    assert_eq!(calls[0].params["body"]["limit"], 50);
    assert_eq!(calls[0].params["body"]["device_id"], "device-a");
    assert_eq!(calls[0].params["body"]["reason"], "app_resumed");
}

#[tokio::test]
async fn sync_delta_advances_past_v2_wire_without_persisting_private_objects() {
    let fixture = Fixture::new("sync-delta-v2-private-wire");
    let client = fixture.client();
    let runtime = MessageSyncRuntime::new(
        &client,
        ReadyAnySessionProvider,
        RecordingTransport::queued(
            Rc::new(RefCell::new(Vec::new())),
            vec![delta_page(
                vec![
                    v2_message_event(
                        "sev-p5",
                        "1",
                        "message.created",
                        "wire-p5-secret",
                        json!({
                            "profile": anp::direct_e2ee::DIRECT_E2EE_PROFILE_V2
                        }),
                        false,
                    ),
                    v2_message_event(
                        "sev-p6",
                        "2",
                        "conversation.updated",
                        "wire-p6-secret",
                        json!({
                            "profile": anp::group_e2ee::GROUP_E2EE_PROFILE_V2
                        }),
                        true,
                    ),
                ],
                "2",
                false,
            )],
        ),
        NoopDirectoryTransport,
    );

    let result = runtime
        .sync_delta_async(SyncDeltaInput {
            request: crate::messages::SyncDeltaRequest::default(),
        })
        .await
        .unwrap();

    assert_eq!(result.events_applied, 2);
    assert_eq!(result.last_applied_event_seq.as_deref(), Some("2"));
    assert_eq!(fixture.checkpoint().as_deref(), Some("2"));
    assert_eq!(fixture.message_server_seq("wire-p5-secret"), None);
    assert_eq!(fixture.message_server_seq("wire-p6-secret"), None);
}

#[tokio::test]
async fn sync_delta_after_did_rotation_starts_new_subject_at_zero_and_receives_direct_message() {
    let old_did = "did:example:alice:old";
    let new_did = "did:example:alice:new";
    let fixture = Fixture::new_with_did("sync-delta-did-rotation", new_did);
    fixture.seed_verified_peer();
    fixture.store_checkpoint_for_subject(old_did, "48");
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let runtime = MessageSyncRuntime::new(
        &client,
        ReadyAnySessionProvider,
        RecordingTransport::queued(
            Rc::clone(&calls),
            vec![delta_page(
                vec![message_created_event_for_owner(
                    "sev-1",
                    "1",
                    "msg-after-rotation",
                    1,
                    new_did,
                )],
                "1",
                false,
            )],
        ),
        NoopDirectoryTransport,
    );

    let result = runtime
        .sync_delta_async(SyncDeltaInput {
            request: crate::messages::SyncDeltaRequest::default(),
        })
        .await
        .unwrap();

    assert_eq!(calls.borrow()[0].params["body"]["since_event_seq"], "0");
    assert_eq!(result.events_applied, 1);
    assert_eq!(fixture.message_server_seq("msg-after-rotation"), Some(1));
    assert_eq!(
        fixture.checkpoint_for_subject(old_did).as_deref(),
        Some("48")
    );
    assert_eq!(
        fixture.checkpoint_for_subject(new_did).as_deref(),
        Some("1")
    );
}

#[tokio::test]
async fn sync_delta_backlogs_unresolved_direct_before_advancing_checkpoint() {
    let fixture = Fixture::new("sync-delta-unresolved-backlog");
    let client = fixture.client();
    let runtime = MessageSyncRuntime::new(
        &client,
        ReadyAnySessionProvider,
        RecordingTransport::queued(
            Rc::new(RefCell::new(Vec::new())),
            vec![delta_page(
                vec![message_created_event("sev-1", "1", "msg-unresolved", 1)],
                "1",
                false,
            )],
        ),
        NoopDirectoryTransport,
    );

    let result = runtime
        .sync_delta_async(SyncDeltaInput {
            request: crate::messages::SyncDeltaRequest::default(),
        })
        .await
        .unwrap();

    assert_eq!(fixture.checkpoint(), Some("1".to_owned()));
    assert_eq!(fixture.message_count("msg-unresolved"), 0);
    assert_eq!(fixture.unresolved_backlog_count(), 1);
    assert!(result
        .warnings
        .iter()
        .any(|warning| warning == "identity_unresolved_backlog:1"));
}

#[tokio::test]
async fn sync_delta_hydrates_metadata_only_outbound_direct_before_checkpoint() {
    let fixture = Fixture::new("sync-delta-outbound-thread-hydration");
    let client = fixture.client();
    let mut conversation_patches = client.messages().watch_conversation_patches().unwrap();
    assert!(matches!(
        conversation_patches.next_patch().await,
        Some(crate::messages::ConversationStorePatch::Reset { .. })
    ));
    let calls = Rc::new(RefCell::new(Vec::new()));
    let runtime = MessageSyncRuntime::new(
        &client,
        ReadyAnySessionProvider,
        RecordingTransport::queued(
            Rc::clone(&calls),
            vec![
                delta_page(
                    vec![outbound_message_created_event_without_content(
                        "sev-outbound-1",
                        "1",
                        "msg-outbound-1",
                        41,
                    )],
                    "1",
                    false,
                ),
                json!({
                    "messages": [{
                        "id": "msg-outbound-1",
                        "sender_did": "did:example:alice",
                        "receiver_did": "did:example:bob",
                        "content": "outbound body from authoritative history",
                        "content_type": "text/plain",
                        "server_seq": "41",
                        "sent_at": "2026-07-27T00:00:00Z"
                    }],
                    "has_more": false,
                    "warnings": []
                }),
            ],
        ),
        FixedLookupDirectoryTransport(json!({
            "handle": "bob",
            "full_handle": "bob.example",
            "did": "did:example:bob",
            "domain": "example",
            "status": "active",
            "user_id": "user-bob"
        })),
    );

    let result = runtime
        .sync_delta_async(SyncDeltaInput {
            request: crate::messages::SyncDeltaRequest::default(),
        })
        .await
        .unwrap();

    assert_eq!(result.events_applied, 1);
    assert_eq!(fixture.checkpoint().as_deref(), Some("1"));
    assert_eq!(fixture.unresolved_backlog_count(), 0);
    assert_eq!(
        fixture.message_content("msg-outbound-1").as_deref(),
        Some("outbound body from authoritative history")
    );
    let patch = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        conversation_patches.next_patch(),
    )
    .await
    .expect("Direct hydration must emit a conversation patch")
    .expect("conversation patch stream must remain open");
    assert!(matches!(
        patch,
        crate::messages::ConversationStorePatch::Upsert { item, .. }
            if item.last_message.as_ref().map(|message| message.id.as_str())
                == Some("msg-outbound-1")
    ));
    let calls = calls.borrow();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].method, "sync.delta");
    assert_eq!(calls[1].method, "direct.get_history");
    assert_eq!(calls[1].params["body"]["peer_did"], "did:example:bob");
    assert_eq!(calls[1].params["body"]["since_seq"], "0");
}

#[tokio::test]
async fn sync_delta_hydrates_metadata_only_group_before_checkpoint() {
    let fixture = Fixture::new("sync-delta-group-thread-hydration");
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let runtime = MessageSyncRuntime::new(
        &client,
        ReadyAnySessionProvider,
        RecordingTransport::queued(
            Rc::clone(&calls),
            vec![
                delta_page(
                    vec![group_message_created_event_without_content(
                        "sev-group-1",
                        "1",
                        "wire-group-9",
                        9,
                    )],
                    "1",
                    false,
                ),
                json!({
                    "messages": [{
                        "id": "wire-group-9",
                        "thread_kind": "group",
                        "group_did": "did:example:group",
                        "sender_did": "did:example:bob",
                        "content": "group body from authoritative history",
                        "content_type": "text/plain",
                        "group_event_seq": 9
                    }],
                    "next_since_seq": "9",
                    "has_more": false,
                    "warnings": []
                }),
            ],
        ),
        NoopDirectoryTransport,
    );

    let result = runtime
        .sync_delta_async(SyncDeltaInput {
            request: crate::messages::SyncDeltaRequest::default(),
        })
        .await
        .unwrap();

    assert_eq!(result.events_applied, 1);
    assert_eq!(fixture.checkpoint().as_deref(), Some("1"));
    assert_eq!(
        fixture.message_content("did:example:group:9").as_deref(),
        Some("group body from authoritative history")
    );
    let calls = calls.borrow();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].method, "sync.delta");
    assert_eq!(calls[1].method, "group.list_messages");
    assert_eq!(calls[1].params["body"]["group_did"], "did:example:group");
    assert_eq!(calls[1].params["body"]["since_seq"], "0");
}

#[tokio::test]
async fn sync_delta_batches_metadata_only_outbound_hydration_per_direct_peer() {
    let fixture = Fixture::new("sync-delta-outbound-thread-batched-hydration");
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let runtime = MessageSyncRuntime::new(
        &client,
        ReadyAnySessionProvider,
        RecordingTransport::queued(
            Rc::clone(&calls),
            vec![
                delta_page(
                    vec![
                        outbound_message_created_event_without_content(
                            "sev-outbound-1",
                            "1",
                            "msg-outbound-1",
                            41,
                        ),
                        outbound_message_created_event_without_content(
                            "sev-outbound-2",
                            "2",
                            "msg-outbound-2",
                            42,
                        ),
                    ],
                    "2",
                    false,
                ),
                json!({
                    "messages": [
                        {
                            "id": "msg-outbound-1",
                            "sender_did": "did:example:alice",
                            "receiver_did": "did:example:bob",
                            "content": "first outbound body",
                            "content_type": "text/plain",
                            "server_seq": "41",
                            "sent_at": "2026-07-27T00:00:00Z"
                        },
                        {
                            "id": "msg-outbound-2",
                            "sender_did": "did:example:alice",
                            "receiver_did": "did:example:bob",
                            "content": "second outbound body",
                            "content_type": "text/plain",
                            "server_seq": "42",
                            "sent_at": "2026-07-27T00:00:01Z"
                        }
                    ],
                    "has_more": false,
                    "warnings": []
                }),
            ],
        ),
        FixedLookupDirectoryTransport(json!({
            "handle": "bob",
            "full_handle": "bob.example",
            "did": "did:example:bob",
            "domain": "example",
            "status": "active",
            "user_id": "user-bob"
        })),
    );

    let result = runtime
        .sync_delta_async(SyncDeltaInput {
            request: crate::messages::SyncDeltaRequest::default(),
        })
        .await
        .unwrap();

    assert_eq!(result.events_applied, 2);
    assert_eq!(fixture.checkpoint().as_deref(), Some("2"));
    assert_eq!(
        fixture.message_content("msg-outbound-1").as_deref(),
        Some("first outbound body")
    );
    assert_eq!(
        fixture.message_content("msg-outbound-2").as_deref(),
        Some("second outbound body")
    );
    let calls = calls.borrow();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].method, "sync.delta");
    assert_eq!(calls[1].method, "direct.get_history");
}

#[tokio::test]
async fn sync_delta_hydrates_missing_message_even_when_thread_sequence_is_ahead() {
    let fixture = Fixture::new("sync-delta-outbound-message-gap");
    let conversation_id = fixture.seed_verified_peer();
    fixture.seed_message(
        "msg-outbound-later",
        &conversation_id,
        "",
        Some(50),
        "did:example:alice",
        "did:example:bob",
    );
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let runtime = MessageSyncRuntime::new(
        &client,
        ReadyAnySessionProvider,
        RecordingTransport::queued(
            Rc::clone(&calls),
            vec![
                delta_page(
                    vec![outbound_message_created_event_without_content(
                        "sev-outbound-1",
                        "1",
                        "msg-outbound-missing",
                        41,
                    )],
                    "1",
                    false,
                ),
                json!({
                    "messages": [{
                        "id": "msg-outbound-missing",
                        "sender_did": "did:example:alice",
                        "receiver_did": "did:example:bob",
                        "content": "missing outbound body",
                        "content_type": "text/plain",
                        "server_seq": "41",
                        "sent_at": "2026-07-27T00:00:00Z"
                    }],
                    "has_more": false,
                    "warnings": []
                }),
            ],
        ),
        FixedLookupDirectoryTransport(json!({
            "handle": "bob",
            "full_handle": "bob.example",
            "did": "did:example:bob",
            "domain": "example",
            "status": "active",
            "user_id": "user-bob"
        })),
    );

    let result = runtime
        .sync_delta_async(SyncDeltaInput {
            request: crate::messages::SyncDeltaRequest::default(),
        })
        .await
        .unwrap();

    assert_eq!(result.events_applied, 1);
    assert_eq!(fixture.checkpoint().as_deref(), Some("1"));
    assert_eq!(
        fixture.message_content("msg-outbound-missing").as_deref(),
        Some("missing outbound body")
    );
    assert_eq!(
        fixture.message_content("msg-outbound-later").as_deref(),
        Some("local")
    );
    let calls = calls.borrow();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[1].method, "direct.get_history");
    assert_eq!(calls[1].params["body"]["since_seq"], "40");
}

#[tokio::test]
async fn sync_delta_does_not_checkpoint_unhydrated_outbound_direct() {
    let fixture = Fixture::new("sync-delta-outbound-thread-hydration-required");
    let client = fixture.client();
    let runtime = MessageSyncRuntime::new(
        &client,
        ReadyAnySessionProvider,
        RecordingTransport::queued(
            Rc::new(RefCell::new(Vec::new())),
            vec![
                delta_page(
                    vec![outbound_message_created_event_without_content(
                        "sev-outbound-1",
                        "1",
                        "msg-outbound-1",
                        41,
                    )],
                    "1",
                    false,
                ),
                json!({
                    "messages": [],
                    "has_more": false,
                    "warnings": []
                }),
            ],
        ),
        NoopDirectoryTransport,
    );

    let error = runtime
        .sync_delta_async(SyncDeltaInput {
            request: crate::messages::SyncDeltaRequest::default(),
        })
        .await
        .unwrap_err();

    assert!(error
        .to_string()
        .contains("did not commit the Direct delta projection"));
    assert_eq!(fixture.checkpoint(), None);
    assert_eq!(fixture.message_count("msg-outbound-1"), 0);
}

#[tokio::test]
async fn sync_delta_success_emits_committed_invalidation_after_apply() {
    let fixture = Fixture::new("sync-delta-invalidation");
    let conversation_id = fixture.seed_verified_peer();
    let client = fixture.client();
    fixture.store_checkpoint("776");
    let runtime = MessageSyncRuntime::new(
        &client,
        ReadyAnySessionProvider,
        RecordingTransport::queued(
            Rc::new(RefCell::new(Vec::new())),
            vec![delta_page(
                vec![message_created_event(
                    "sev-777",
                    "777",
                    "msg-delta-777",
                    777,
                )],
                "777",
                false,
            )],
        ),
        NoopDirectoryTransport,
    );

    let result = runtime
        .sync_delta_async(SyncDeltaInput {
            request: crate::messages::SyncDeltaRequest::default(),
        })
        .await
        .unwrap();

    assert_eq!(result.last_applied_event_seq.as_deref(), Some("777"));
    assert_eq!(fixture.checkpoint(), Some("777".to_owned()));
    let invalidations = committed_sync_invalidations_for_test();
    let invalidation = invalidations
        .iter()
        .find(|item| item.checkpoint_event_seq == "777")
        .expect("committed invalidation for checkpoint 777");
    assert_eq!(invalidation.owner_identity_id, "alice-id");
    assert_eq!(invalidation.owner_did, "did:example:alice");
    assert_eq!(invalidation.reason, "sync_delta");
    assert_eq!(invalidation.conversation_ids, vec![conversation_id.clone()]);
    assert_eq!(invalidation.thread_ids, vec![conversation_id]);
}

#[tokio::test]
async fn sync_delta_success_emits_conversation_store_patch_after_commit() {
    let fixture = Fixture::new("sync-delta-store-patch");
    fixture.seed_verified_peer();
    let client = fixture.client();
    fixture.store_checkpoint("776");
    let mut patches = client.messages().watch_conversation_patches().unwrap();
    let _initial_hydrate = patches.next_patch().await.unwrap();
    let runtime = MessageSyncRuntime::new(
        &client,
        ReadyAnySessionProvider,
        RecordingTransport::queued(
            Rc::new(RefCell::new(Vec::new())),
            vec![delta_page(
                vec![message_created_event(
                    "sev-777",
                    "777",
                    "msg-delta-777",
                    777,
                )],
                "777",
                false,
            )],
        ),
        NoopDirectoryTransport,
    );

    runtime
        .sync_delta_async(SyncDeltaInput {
            request: crate::messages::SyncDeltaRequest::default(),
        })
        .await
        .unwrap();
    let patch = patches.next_patch().await.unwrap();

    match patch {
        crate::messages::ConversationStorePatch::Upsert {
            owner_identity_id,
            owner_did,
            item,
            ..
        } => {
            assert_eq!(owner_identity_id, "alice-id");
            assert_eq!(owner_did, "did:example:alice");
            assert_eq!(item.last_message.unwrap().id, "msg-delta-777");
        }
        other => panic!("expected conversation upsert patch, got {other:?}"),
    }
}

#[tokio::test]
async fn sync_delta_preserves_attachment_manifest_content_type() {
    let fixture = Fixture::new("sync-delta-attachment-manifest");
    let client = fixture.client();
    let runtime = MessageSyncRuntime::new(
        &client,
        ReadyAnySessionProvider,
        RecordingTransport::queued(
            Rc::new(RefCell::new(Vec::new())),
            vec![delta_page(
                vec![attachment_manifest_message_created_event(
                    "sev-attachment-1",
                    "1",
                    "msg-attachment-1",
                    1,
                )],
                "1",
                false,
            )],
        ),
        NoopDirectoryTransport,
    );

    let result = runtime
        .sync_delta_async(SyncDeltaInput {
            request: crate::messages::SyncDeltaRequest::default(),
        })
        .await
        .unwrap();

    assert_eq!(result.events_applied, 1);
    let canonical_message_id = "did:example:group:1";
    assert_eq!(
        fixture
            .message_content_type(canonical_message_id)
            .as_deref(),
        Some(crate::attachments::manifest::attachment_manifest_content_type())
    );
    let content = fixture
        .message_content(canonical_message_id)
        .expect("stored attachment manifest");
    let stored: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(stored["attachments"][0]["attachment_id"], "att-sync-1");
}

#[tokio::test]
async fn sync_delta_group_member_changed_projects_system_timeline_message() {
    let fixture = Fixture::new("sync-delta-group-system-event");
    let client = fixture.client();
    fixture.store_checkpoint("9");
    let runtime = MessageSyncRuntime::new(
        &client,
        ReadyAnySessionProvider,
        RecordingTransport::queued(
            Rc::new(RefCell::new(Vec::new())),
            vec![delta_page(
                vec![group_member_changed_event("sev-10", "10", 7)],
                "10",
                false,
            )],
        ),
        NoopDirectoryTransport,
    );

    let result = runtime
        .sync_delta_async(SyncDeltaInput {
            request: crate::messages::SyncDeltaRequest {
                limit: Some(50),
                device_id: None,
                reason: Some("test".to_owned()),
            },
        })
        .await
        .unwrap();

    assert_eq!(result.events_applied, 1);
    let msg_id = "did:example:group:7";
    assert_eq!(fixture.message_server_seq(msg_id), Some(7));
    assert_eq!(
        fixture.message_content_type(msg_id).as_deref(),
        Some("application/json")
    );
    let content = fixture.message_content(msg_id).unwrap();
    let payload = serde_json::from_str::<Value>(&content).unwrap();
    assert_eq!(payload["schema"], "awiki.group.system_event.v1");
    assert_eq!(payload["type"], "member_added");
    assert_eq!(payload["actor_did"], "did:example:alice");
    assert_eq!(payload["subject_did"], "did:example:bob");
    assert_eq!(payload["group_event_seq"], "7");
    assert_eq!(fixture.message_is_read(msg_id), Some(true));
}

#[tokio::test]
async fn sync_delta_has_more_reads_committed_checkpoint_for_next_page() {
    let fixture = Fixture::new("sync-delta-has-more");
    fixture.seed_verified_peer();
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let runtime = MessageSyncRuntime::new(
        &client,
        ReadyAnySessionProvider,
        RecordingTransport::queued(
            Rc::clone(&calls),
            vec![
                delta_page(
                    vec![message_created_event("sev-1", "1", "msg-delta-1", 1)],
                    "1",
                    true,
                ),
                delta_page(
                    vec![message_created_event("sev-2", "2", "msg-delta-2", 2)],
                    "2",
                    false,
                ),
            ],
        ),
        NoopDirectoryTransport,
    );

    let result = runtime
        .sync_delta_async(SyncDeltaInput {
            request: crate::messages::SyncDeltaRequest {
                limit: Some(1),
                device_id: None,
                reason: Some("gap".to_owned()),
            },
        })
        .await
        .unwrap();

    assert_eq!(result.events_applied, 2);
    assert_eq!(result.pages_fetched, 2);
    assert_eq!(fixture.checkpoint(), Some("2".to_owned()));
    assert_eq!(fixture.message_server_seq("msg-delta-1"), Some(1));
    assert_eq!(fixture.message_server_seq("msg-delta-2"), Some(2));
    let calls = calls.borrow();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].params["body"]["since_event_seq"], "0");
    assert_eq!(calls[1].params["body"]["since_event_seq"], "1");
}

#[tokio::test]
async fn sync_delta_snapshot_required_is_fail_closed() {
    let fixture = Fixture::new("sync-delta-snapshot");
    let client = fixture.client();
    fixture.store_checkpoint("7");
    let runtime = MessageSyncRuntime::new(
        &client,
        ReadyAnySessionProvider,
        RecordingTransport::queued(
            Rc::new(RefCell::new(Vec::new())),
            vec![json!({
                "events": [message_created_event("sev-8", "8", "msg-delta-8", 8)],
                "next_event_seq": "8",
                "has_more": false,
                "snapshot_required": true,
                "retention_floor_event_seq": "10",
                "warnings": ["delta_retention_gap"],
            })],
        ),
        NoopDirectoryTransport,
    );

    let result = runtime
        .sync_delta_async(SyncDeltaInput {
            request: crate::messages::SyncDeltaRequest::default(),
        })
        .await
        .unwrap();

    assert!(result.snapshot_required);
    assert_eq!(result.retention_floor_event_seq.as_deref(), Some("10"));
    assert_eq!(result.events_applied, 0);
    assert_eq!(fixture.checkpoint(), Some("7".to_owned()));
    assert_eq!(fixture.message_server_seq("msg-delta-8"), None);
    assert!(!committed_sync_invalidations_for_test()
        .iter()
        .any(|item| item.checkpoint_event_seq == "8"));
}

#[tokio::test]
async fn sync_delta_accepts_sparse_exact_device_projection() {
    let fixture = Fixture::new("sync-delta-sparse-device-projection");
    fixture.seed_verified_peer();
    let client = fixture.client();
    let runtime = MessageSyncRuntime::new(
        &client,
        ReadyAnySessionProvider,
        RecordingTransport::queued(
            Rc::new(RefCell::new(Vec::new())),
            vec![delta_page(
                vec![
                    message_created_event("sev-1", "1", "msg-before-gap", 1),
                    message_created_event("sev-3", "3", "msg-after-gap", 3),
                ],
                "3",
                false,
            )],
        ),
        NoopDirectoryTransport,
    );

    let result = runtime
        .sync_delta_async(SyncDeltaInput {
            request: crate::messages::SyncDeltaRequest {
                device_id: Some("device-a".to_owned()),
                ..crate::messages::SyncDeltaRequest::default()
            },
        })
        .await
        .unwrap();

    assert_eq!(result.events_applied, 2);
    assert_eq!(result.last_applied_event_seq.as_deref(), Some("3"));
    assert_eq!(fixture.checkpoint().as_deref(), Some("3"));
    assert_eq!(fixture.message_server_seq("msg-before-gap"), Some(1));
    assert_eq!(fixture.message_server_seq("msg-after-gap"), Some(3));
    assert!(committed_sync_invalidations_for_test()
        .iter()
        .any(|item| item.checkpoint_event_seq == "3"));
}

#[tokio::test]
async fn sync_delta_empty_exact_device_page_advances_before_next_page() {
    let fixture = Fixture::new("sync-delta-empty-device-page");
    fixture.seed_verified_peer();
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let runtime = MessageSyncRuntime::new(
        &client,
        ReadyAnySessionProvider,
        RecordingTransport::queued(
            Rc::clone(&calls),
            vec![
                delta_page(Vec::new(), "2", true),
                delta_page(
                    vec![message_created_event("sev-4", "4", "msg-visible-4", 4)],
                    "4",
                    false,
                ),
            ],
        ),
        NoopDirectoryTransport,
    );

    let result = runtime
        .sync_delta_async(SyncDeltaInput {
            request: crate::messages::SyncDeltaRequest {
                limit: Some(2),
                device_id: Some("device-a".to_owned()),
                reason: Some("exact_device_projection".to_owned()),
            },
        })
        .await
        .unwrap();

    assert_eq!(result.events_applied, 1);
    assert_eq!(result.pages_fetched, 2);
    assert_eq!(result.last_applied_event_seq.as_deref(), Some("4"));
    assert_eq!(fixture.checkpoint().as_deref(), Some("4"));
    assert_eq!(fixture.message_server_seq("msg-visible-4"), Some(4));
    let calls = calls.borrow();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].params["body"]["since_event_seq"], "0");
    assert_eq!(calls[1].params["body"]["since_event_seq"], "2");
}

#[tokio::test]
async fn sync_delta_rejects_visible_event_ahead_of_server_checkpoint() {
    let fixture = Fixture::new("sync-delta-event-ahead-of-checkpoint");
    let client = fixture.client();
    let runtime = MessageSyncRuntime::new(
        &client,
        ReadyAnySessionProvider,
        RecordingTransport::queued(
            Rc::new(RefCell::new(Vec::new())),
            vec![delta_page(
                vec![message_created_event("sev-3", "3", "msg-ahead", 3)],
                "2",
                false,
            )],
        ),
        NoopDirectoryTransport,
    );

    let error = runtime
        .sync_delta_async(SyncDeltaInput {
            request: crate::messages::SyncDeltaRequest::default(),
        })
        .await
        .unwrap_err();

    assert!(error.to_string().contains("ahead of next_event_seq"));
    assert_eq!(fixture.checkpoint(), None);
    assert_eq!(fixture.message_server_seq("msg-ahead"), None);
}

#[tokio::test]
async fn sync_delta_metadata_only_message_without_thread_sequence_is_fail_closed() {
    let fixture = Fixture::new("sync-delta-metadata-without-sequence");
    fixture.seed_verified_peer();
    let client = fixture.client();
    let mut event = message_created_event_without_content("sev-1", "1", "msg-no-seq", 1);
    event["payload"]["message"]
        .as_object_mut()
        .unwrap()
        .remove("server_seq");
    let error = MessageSyncRuntime::new(
        &client,
        ReadyAnySessionProvider,
        RecordingTransport::queued(
            Rc::new(RefCell::new(Vec::new())),
            vec![delta_page(vec![event], "1", false)],
        ),
        NoopDirectoryTransport,
    )
    .sync_delta_async(SyncDeltaInput {
        request: crate::messages::SyncDeltaRequest::default(),
    })
    .await
    .unwrap_err();

    assert!(error
        .to_string()
        .contains("metadata-only Direct sync event is missing server_seq"));
    assert_eq!(fixture.checkpoint(), None);
    assert_eq!(fixture.message_count("msg-no-seq"), 0);
}

#[tokio::test]
async fn sync_delta_duplicate_page_is_idempotent() {
    let fixture = Fixture::new("sync-delta-duplicate");
    let client = fixture.client();
    fixture.store_checkpoint("1");
    fixture.seed_message(
        "msg-delta-1",
        "dm:did:example:bob",
        "",
        Some(1),
        "did:example:bob",
        "did:example:alice",
    );
    let runtime = MessageSyncRuntime::new(
        &client,
        ReadyAnySessionProvider,
        RecordingTransport::queued(
            Rc::new(RefCell::new(Vec::new())),
            vec![delta_page(
                vec![message_created_event("sev-1", "1", "msg-delta-1", 1)],
                "1",
                false,
            )],
        ),
        NoopDirectoryTransport,
    );

    let result = runtime
        .sync_delta_async(SyncDeltaInput {
            request: crate::messages::SyncDeltaRequest::default(),
        })
        .await
        .unwrap();

    assert_eq!(result.events_applied, 0);
    assert_eq!(fixture.checkpoint(), Some("1".to_owned()));
    assert_eq!(fixture.message_count("msg-delta-1"), 1);
}

#[tokio::test]
async fn sync_delta_metadata_only_event_preserves_existing_message_body() {
    let fixture = Fixture::new("sync-delta-metadata-only");
    let conversation_id = fixture.seed_verified_peer();
    let client = fixture.client();
    fixture.seed_message(
        "msg-delta-body",
        "dm:did:example:bob",
        "",
        Some(10),
        "did:example:bob",
        "did:example:alice",
    );
    let runtime = MessageSyncRuntime::new(
        &client,
        ReadyAnySessionProvider,
        RecordingTransport::queued(
            Rc::new(RefCell::new(Vec::new())),
            vec![delta_page(
                vec![message_created_event_without_content(
                    "sev-1",
                    "1",
                    "msg-delta-body",
                    10,
                )],
                "1",
                false,
            )],
        ),
        NoopDirectoryTransport,
    );

    let result = runtime
        .sync_delta_async(SyncDeltaInput {
            request: crate::messages::SyncDeltaRequest::default(),
        })
        .await
        .unwrap();

    assert_eq!(result.events_applied, 1);
    assert_eq!(fixture.checkpoint(), Some("1".to_owned()));
    assert_eq!(
        fixture.message_content("msg-delta-body").as_deref(),
        Some("local")
    );
    assert_eq!(
        fixture.message_hydration_state("msg-delta-body").as_deref(),
        Some("hydrated")
    );
    assert_eq!(
        fixture
            .conversation_last_content(&conversation_id)
            .as_deref(),
        Some("local")
    );
}

#[tokio::test]
async fn sync_delta_has_more_duplicate_page_without_progress_is_invalid() {
    let fixture = Fixture::new("sync-delta-has-more-no-progress");
    let client = fixture.client();
    fixture.store_checkpoint("1");
    let calls = Rc::new(RefCell::new(Vec::new()));
    let runtime = MessageSyncRuntime::new(
        &client,
        ReadyAnySessionProvider,
        RecordingTransport::queued(
            Rc::clone(&calls),
            vec![delta_page(
                vec![message_created_event("sev-1", "1", "msg-delta-1", 1)],
                "1",
                true,
            )],
        ),
        NoopDirectoryTransport,
    );

    let err = runtime
        .sync_delta_async(SyncDeltaInput {
            request: crate::messages::SyncDeltaRequest::default(),
        })
        .await
        .unwrap_err();

    assert!(err.to_string().contains("without checkpoint progress"));
    assert_eq!(fixture.checkpoint(), Some("1".to_owned()));
    assert_eq!(calls.borrow().len(), 1);
}

#[tokio::test]
async fn sync_thread_after_uses_local_max_seq_and_applies_ascending_page() {
    let fixture = Fixture::new("thread-after-direct");
    let conversation_id = fixture.seed_verified_peer();
    fixture.seed_sync_thread_binding(&conversation_id, "conversation-ref-bob", "direct");
    fixture.store_checkpoint("77");
    let client = fixture.client();
    fixture.seed_message(
        "local-direct-newest",
        &conversation_id,
        "",
        Some(42),
        "did:example:bob",
        "did:example:alice",
    );
    fixture.seed_message(
        "local-other-thread",
        "dm:did:example:carol",
        "",
        Some(100),
        "did:example:carol",
        "did:example:alice",
    );
    let mut patch_session = client
        .messages()
        .watch_thread_patches_async(
            crate::messages::ThreadRef::Direct(
                crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
            ),
            Some(100),
        )
        .await
        .unwrap();
    assert!(matches!(
        patch_session.next_patch().await,
        Some(crate::messages::ThreadMessageStorePatch::Reset { .. })
    ));
    let calls = Rc::new(RefCell::new(Vec::new()));
    let runtime = MessageSyncRuntime::new(
        &client,
        ReadyAnySessionProvider,
        RecordingTransport::queued(
            Rc::clone(&calls),
            vec![json!({
                "messages": [
                    {
                        "id": "remote-new-43",
                        "thread_kind": "direct",
                        "sender_did": "did:example:bob",
                        "receiver_did": "did:example:alice",
                        "content": "new 43",
                        "content_type": "text/plain",
                        "server_seq": 43
                    },
                    {
                        "id": "remote-new-44",
                        "thread_kind": "direct",
                        "sender_did": "did:example:bob",
                        "receiver_did": "did:example:alice",
                        "content": "new 44",
                        "content_type": "text/plain",
                        "server_seq": "44"
                    }
                ],
                "next_after_server_seq": "44",
                "has_more": false,
                "warnings": []
            })],
        ),
        NoopDirectoryTransport,
    );

    let result = runtime
        .sync_thread_after_async(SyncThreadAfterInput {
            request: crate::messages::SyncThreadAfterRequest {
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                ),
                after_server_seq: None,
                limit: Some(10),
            },
            resolved_peer_did: Some("did:example:bob".to_owned()),
            peer_scope: Some(
                crate::internal::local_state::owner_scope::DirectPeerScope::new(
                    "user-bob",
                    "bob.awiki.test",
                )
                .unwrap(),
            ),
        })
        .await
        .unwrap();

    assert_eq!(
        result
            .messages
            .iter()
            .map(|message| message.id.as_str())
            .collect::<Vec<_>>(),
        vec!["remote-new-43", "remote-new-44"]
    );
    assert_eq!(result.next_after_server_seq.as_deref(), Some("44"));
    assert!(!result.has_more);
    {
        let calls = calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].method, "sync.thread_after");
        assert_eq!(
            calls[0].params["body"],
            json!({
                "thread_key": "conversation-ref-bob",
                "after_server_seq": "42",
                "limit": 10
            })
        );
    }
    assert_eq!(fixture.checkpoint().as_deref(), Some("77"));
    match patch_session.next_patch().await {
        Some(crate::messages::ThreadMessageStorePatch::Reset { items, .. }) => {
            assert!(items
                .iter()
                .any(|message| message.id.as_str() == "remote-new-44"));
        }
        Some(crate::messages::ThreadMessageStorePatch::Upsert { message, .. }) => {
            assert_eq!(message.id.as_str(), "remote-new-44");
        }
        other => panic!("unexpected thread patch after sync_thread_after commit: {other:?}"),
    }
}

#[tokio::test]
async fn sync_thread_after_clamps_explicit_cursor_behind_hydration_hole() {
    let fixture = Fixture::new("thread-after-explicit-hydration-hole");
    let conversation_id = fixture.seed_verified_peer();
    fixture.seed_sync_thread_binding(&conversation_id, "conversation-ref-bob", "direct");
    fixture.seed_message(
        "complete-1",
        &conversation_id,
        "",
        Some(1),
        "did:example:bob",
        "did:example:alice",
    );
    fixture.seed_message_with_hydration(
        "hole-2",
        &conversation_id,
        "",
        Some(2),
        "did:example:bob",
        "did:example:alice",
        crate::internal::local_state::messages::MessageHydrationState::Discovered,
    );
    fixture.seed_message(
        "complete-3",
        &conversation_id,
        "",
        Some(3),
        "did:example:bob",
        "did:example:alice",
    );
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let result = MessageSyncRuntime::new(
        &client,
        ReadyAnySessionProvider,
        RecordingTransport::queued(
            Rc::clone(&calls),
            vec![json!({
                "messages": [
                    {
                        "id": "hole-2",
                        "thread_kind": "direct",
                        "sender_did": "did:example:bob",
                        "receiver_did": "did:example:alice",
                        "content": "recovered two",
                        "content_type": "text/plain",
                        "server_seq": 2
                    },
                    {
                        "id": "complete-3",
                        "thread_kind": "direct",
                        "sender_did": "did:example:bob",
                        "receiver_did": "did:example:alice",
                        "content": "confirmed three",
                        "content_type": "text/plain",
                        "server_seq": 3
                    }
                ],
                "next_after_server_seq": "3",
                "has_more": false,
                "warnings": []
            })],
        ),
        NoopDirectoryTransport,
    )
    .sync_thread_after_async(SyncThreadAfterInput {
        request: crate::messages::SyncThreadAfterRequest {
            thread: crate::messages::ThreadRef::Direct(
                crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
            ),
            after_server_seq: Some("3".to_owned()),
            limit: Some(10),
        },
        resolved_peer_did: Some("did:example:bob".to_owned()),
        peer_scope: Some(
            crate::internal::local_state::owner_scope::DirectPeerScope::new(
                "user-bob",
                "bob.awiki.test",
            )
            .unwrap(),
        ),
    })
    .await
    .unwrap();

    assert_eq!(calls.borrow()[0].params["body"]["after_server_seq"], "1");
    assert_eq!(
        result
            .messages
            .iter()
            .map(|message| message.id.as_str())
            .collect::<Vec<_>>(),
        vec!["hole-2", "complete-3"]
    );
    assert_eq!(
        fixture.message_hydration_state("hole-2").as_deref(),
        Some("hydrated")
    );
    assert_eq!(
        fixture.message_content("hole-2").as_deref(),
        Some("recovered two")
    );
}

#[test]
fn sync_thread_after_blocking_clamps_explicit_cursor_behind_hydration_hole() {
    let fixture = Fixture::new("thread-after-blocking-hydration-hole");
    let conversation_id = fixture.seed_verified_peer();
    fixture.seed_sync_thread_binding(&conversation_id, "conversation-ref-bob", "direct");
    fixture.seed_message_with_hydration(
        "blocking-hole-2",
        &conversation_id,
        "",
        Some(2),
        "did:example:bob",
        "did:example:alice",
        crate::internal::local_state::messages::MessageHydrationState::Discovered,
    );
    fixture.seed_message(
        "blocking-complete-3",
        &conversation_id,
        "",
        Some(3),
        "did:example:bob",
        "did:example:alice",
    );
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let result = MessageSyncRuntime::new(
        &client,
        ReadyAnySessionProvider,
        RecordingTransport::queued(
            Rc::clone(&calls),
            vec![json!({
                "messages": [{
                    "id": "blocking-hole-2",
                    "thread_kind": "direct",
                    "sender_did": "did:example:bob",
                    "receiver_did": "did:example:alice",
                    "content": "blocking recovered two",
                    "content_type": "text/plain",
                    "server_seq": 2
                }],
                "next_after_server_seq": "2",
                "has_more": false,
                "warnings": []
            })],
        ),
        StaticHandleDirectoryTransport,
    )
    .sync_thread_after(SyncThreadAfterInput {
        request: crate::messages::SyncThreadAfterRequest {
            thread: crate::messages::ThreadRef::Direct(
                crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
            ),
            after_server_seq: Some("3".to_owned()),
            limit: Some(10),
        },
        resolved_peer_did: Some("did:example:bob".to_owned()),
        peer_scope: Some(
            crate::internal::local_state::owner_scope::DirectPeerScope::new(
                "user-bob",
                "bob.awiki.test",
            )
            .unwrap(),
        ),
    })
    .unwrap();

    assert_eq!(calls.borrow()[0].params["body"]["after_server_seq"], "1");
    assert_eq!(result.messages[0].id.as_str(), "blocking-hole-2");
    assert_eq!(
        fixture
            .message_hydration_state("blocking-hole-2")
            .as_deref(),
        Some("hydrated")
    );
    assert_eq!(
        fixture.message_conversation_and_wire_identity("blocking-hole-2"),
        Some((
            conversation_id,
            "direct".to_owned(),
            "did:example:bob".to_owned(),
            "resolved".to_owned(),
        ))
    );
}

#[tokio::test]
async fn sync_thread_after_fails_closed_without_cursor_progress() {
    let fixture = Fixture::new("thread-after-legacy-probe-no-coverage");
    let conversation_id = fixture.seed_verified_peer();
    fixture.seed_sync_thread_binding(&conversation_id, "conversation-ref-bob", "direct");
    for (msg_id, server_seq) in [("legacy-empty-10", 10), ("legacy-unsupported-11", 11)] {
        fixture.seed_message_with_hydration(
            msg_id,
            &conversation_id,
            "",
            Some(server_seq),
            "did:example:bob",
            "did:example:alice",
            crate::internal::local_state::messages::MessageHydrationState::LegacyProbe,
        );
    }
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let error = MessageSyncRuntime::new(
        &client,
        ReadyAnySessionProvider,
        RecordingTransport::queued(
            Rc::clone(&calls),
            vec![json!({
                "messages": [],
                "next_after_server_seq": "9",
                "has_more": true,
                "warnings": []
            })],
        ),
        NoopDirectoryTransport,
    )
    .sync_thread_after_async(SyncThreadAfterInput {
        request: crate::messages::SyncThreadAfterRequest {
            thread: crate::messages::ThreadRef::Direct(
                crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
            ),
            after_server_seq: Some("11".to_owned()),
            limit: Some(10),
        },
        resolved_peer_did: Some("did:example:bob".to_owned()),
        peer_scope: Some(
            crate::internal::local_state::owner_scope::DirectPeerScope::new(
                "user-bob",
                "bob.awiki.test",
            )
            .unwrap(),
        ),
    })
    .await
    .unwrap_err();

    assert!(error.to_string().contains("made no cursor progress"));
    assert_eq!(calls.borrow()[0].params["body"]["after_server_seq"], "9");
    for msg_id in ["legacy-empty-10", "legacy-unsupported-11"] {
        assert_eq!(
            fixture.message_hydration_state(msg_id).as_deref(),
            Some("legacy_probe")
        );
    }
}

#[tokio::test]
async fn sync_thread_after_rejects_items_at_or_before_requested_cursor() {
    let fixture = Fixture::new("thread-after-explicit");
    let conversation_id = fixture.seed_verified_peer();
    fixture.seed_sync_thread_binding(&conversation_id, "conversation-ref-bob", "direct");
    let client = fixture.client();
    fixture.seed_message(
        "local-direct-would-merge",
        &conversation_id,
        "",
        Some(99),
        "did:example:bob",
        "did:example:alice",
    );
    let calls = Rc::new(RefCell::new(Vec::new()));
    let runtime = MessageSyncRuntime::new(
        &client,
        ReadyAnySessionProvider,
        RecordingTransport::queued(
            Rc::clone(&calls),
            vec![json!({
                "messages": [
                    {
                        "id": "remote-old-1",
                        "thread_kind": "direct",
                        "sender_did": "did:example:bob",
                        "receiver_did": "did:example:alice",
                        "content": "old 1",
                        "content_type": "text/plain",
                        "server_seq": 1
                    },
                    {
                        "id": "remote-new-8",
                        "thread_kind": "direct",
                        "sender_did": "did:example:bob",
                        "receiver_did": "did:example:alice",
                        "content": "new 8",
                        "content_type": "text/plain",
                        "server_seq": 8
                    }
                ],
                "next_after_server_seq": "8",
                "has_more": false,
                "warnings": []
            })],
        ),
        NoopDirectoryTransport,
    );

    let error = runtime
        .sync_thread_after_async(SyncThreadAfterInput {
            request: crate::messages::SyncThreadAfterRequest {
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                ),
                after_server_seq: Some("7".to_owned()),
                limit: None,
            },
            resolved_peer_did: Some("did:example:bob".to_owned()),
            peer_scope: Some(
                crate::internal::local_state::owner_scope::DirectPeerScope::new(
                    "user-bob",
                    "bob.awiki.test",
                )
                .unwrap(),
            ),
        })
        .await
        .unwrap_err();

    assert!(error.to_string().contains("at or before after_server_seq"));
    let calls = calls.borrow();
    assert_eq!(calls[0].method, "sync.thread_after");
    assert_eq!(calls[0].params["body"]["after_server_seq"], "7");
    assert_eq!(calls[0].params["body"]["limit"], 100);
}

#[tokio::test]
async fn sync_thread_after_rejects_message_without_thread_local_sequence() {
    let fixture = Fixture::new("thread-after-missing-sequence");
    let conversation_id = fixture.seed_verified_peer();
    fixture.seed_sync_thread_binding(&conversation_id, "conversation-ref-bob", "direct");
    fixture.seed_message_with_hydration(
        "legacy-probe-10",
        &conversation_id,
        "",
        Some(10),
        "did:example:bob",
        "did:example:alice",
        crate::internal::local_state::messages::MessageHydrationState::LegacyProbe,
    );
    let client = fixture.client();

    let error = MessageSyncRuntime::new(
        &client,
        ReadyAnySessionProvider,
        RecordingTransport::queued(
            Rc::new(RefCell::new(Vec::new())),
            vec![json!({
                "messages": [{
                    "id": "legacy-probe-10",
                    "thread_kind": "direct",
                    "sender_did": "did:example:bob",
                    "receiver_did": "did:example:alice",
                    "content": "must not be accepted without a sequence",
                    "content_type": "text/plain"
                }],
                "next_after_server_seq": "10",
                "has_more": false,
                "warnings": []
            })],
        ),
        NoopDirectoryTransport,
    )
    .sync_thread_after_async(SyncThreadAfterInput {
        request: crate::messages::SyncThreadAfterRequest {
            thread: crate::messages::ThreadRef::Direct(
                crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
            ),
            after_server_seq: Some("10".to_owned()),
            limit: Some(10),
        },
        resolved_peer_did: Some("did:example:bob".to_owned()),
        peer_scope: Some(
            crate::internal::local_state::owner_scope::DirectPeerScope::new(
                "user-bob",
                "bob.awiki.test",
            )
            .unwrap(),
        ),
    })
    .await
    .unwrap_err();

    assert!(matches!(
        error,
        crate::ImError::Service {
            code: Some(ref code),
            ..
        } if code == "sync.invalid_page"
    ));
    assert!(error
        .to_string()
        .contains("missing thread-local server_seq"));
    assert_eq!(
        fixture
            .message_hydration_state("legacy-probe-10")
            .as_deref(),
        Some("legacy_probe")
    );
    assert_eq!(fixture.message_content("legacy-probe-10"), None);
}

#[tokio::test]
async fn sync_thread_after_preserves_all_messages_across_two_full_pages() {
    let fixture = Fixture::new("thread-after-two-full-pages");
    let conversation_id = fixture.seed_verified_peer();
    fixture.seed_sync_thread_binding(&conversation_id, "conversation-ref-bob", "direct");
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let page = |start: i64, end: i64, has_more: bool| {
        let messages = (start..=end)
            .map(|server_seq| {
                json!({
                    "id": format!("remote-{server_seq}"),
                    "thread_kind": "direct",
                    "sender_did": "did:example:bob",
                    "receiver_did": "did:example:alice",
                    "content": format!("body {server_seq}"),
                    "content_type": "text/plain",
                    "server_seq": server_seq
                })
            })
            .collect::<Vec<_>>();
        json!({
            "messages": messages,
            "next_after_server_seq": end.to_string(),
            "has_more": has_more,
            "warnings": []
        })
    };
    let transport = RecordingTransport::queued(
        Rc::clone(&calls),
        vec![page(1, 100, true), page(101, 200, false)],
    );
    let thread = crate::messages::ThreadRef::Direct(
        crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
    );
    let first = MessageSyncRuntime::new(
        &client,
        ReadyAnySessionProvider,
        transport,
        NoopDirectoryTransport,
    )
    .sync_thread_after_async(SyncThreadAfterInput {
        request: crate::messages::SyncThreadAfterRequest {
            thread: thread.clone(),
            after_server_seq: Some("0".to_owned()),
            limit: Some(100),
        },
        resolved_peer_did: Some("did:example:bob".to_owned()),
        peer_scope: Some(
            crate::internal::local_state::owner_scope::DirectPeerScope::new(
                "user-bob",
                "bob.awiki.test",
            )
            .unwrap(),
        ),
    })
    .await
    .unwrap();

    assert_eq!(first.messages.len(), 100);
    assert_eq!(first.messages.first().unwrap().id.as_str(), "remote-1");
    assert_eq!(first.messages.last().unwrap().id.as_str(), "remote-100");
    assert_eq!(first.next_after_server_seq.as_deref(), Some("100"));
    assert!(first.has_more);

    let second = MessageSyncRuntime::new(
        &client,
        ReadyAnySessionProvider,
        RecordingTransport {
            calls: Rc::clone(&calls),
            responses: RefCell::new(VecDeque::from([page(101, 200, false)])),
        },
        NoopDirectoryTransport,
    )
    .sync_thread_after_async(SyncThreadAfterInput {
        request: crate::messages::SyncThreadAfterRequest {
            thread,
            after_server_seq: first.next_after_server_seq.clone(),
            limit: Some(100),
        },
        resolved_peer_did: Some("did:example:bob".to_owned()),
        peer_scope: Some(
            crate::internal::local_state::owner_scope::DirectPeerScope::new(
                "user-bob",
                "bob.awiki.test",
            )
            .unwrap(),
        ),
    })
    .await
    .unwrap();

    assert_eq!(second.messages.len(), 100);
    assert_eq!(second.messages.first().unwrap().id.as_str(), "remote-101");
    assert_eq!(second.messages.last().unwrap().id.as_str(), "remote-200");
    assert_eq!(second.next_after_server_seq.as_deref(), Some("200"));
    assert!(!second.has_more);
    assert_eq!(fixture.message_count("remote-1"), 1);
    assert_eq!(fixture.message_count("remote-100"), 1);
    assert_eq!(fixture.message_count("remote-101"), 1);
    assert_eq!(fixture.message_count("remote-200"), 1);
    let calls = calls.borrow();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].params["body"]["after_server_seq"], "0");
    assert_eq!(calls[1].params["body"]["after_server_seq"], "100");
}

#[tokio::test]
async fn legacy_direct_catch_up_does_not_hydrate_probe_when_full_message_is_backlogged() {
    let fixture = Fixture::new("thread-after-unresolved-persona");
    let conversation_id = "dm:did:example:bob";
    fixture.seed_message_with_hydration(
        "needs-persona-10",
        conversation_id,
        "",
        Some(10),
        "did:example:bob",
        "did:example:alice",
        crate::internal::local_state::messages::MessageHydrationState::LegacyProbe,
    );
    let client = fixture.client();

    let mut runtime = MessageSyncRuntime::new(
        &client,
        ReadyAnySessionProvider,
        RecordingTransport::queued(
            Rc::new(RefCell::new(Vec::new())),
            vec![json!({
                "messages": [{
                    "id": "needs-persona-10",
                    "sender_did": "did:example:bob",
                    "receiver_did": "did:example:alice",
                    "content": "complete but not yet canonically routable",
                    "content_type": "text/plain",
                    "server_seq": 10
                }],
                "next_after_server_seq": "10",
                "has_more": false
            })],
        ),
        NoopDirectoryTransport,
    );
    let result = runtime
        .sync_legacy_direct_thread_after_async(
            crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
            Some("did:example:bob".to_owned()),
            None,
            9,
            10,
        )
        .await
        .unwrap();

    assert_eq!(result.messages.len(), 1);
    assert_eq!(fixture.unresolved_backlog_count(), 1);
    assert_eq!(
        fixture
            .message_hydration_state("needs-persona-10")
            .as_deref(),
        Some("legacy_probe")
    );
    assert_eq!(fixture.message_content("needs-persona-10"), None);
    let db = crate::internal::local_state::open_writable(&fixture.sqlite_path()).unwrap();
    let local =
        crate::internal::local_state::messages::list_messages_for_thread_ref_for_owner_identity(
            &db,
            "alice-id",
            "did:example:alice",
            &crate::messages::ThreadRef::Direct(
                crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
            ),
            10,
            None,
        )
        .unwrap();
    assert!(local.records.is_empty());
}

#[tokio::test]
async fn sync_thread_after_group_clamps_cursor_behind_hydration_hole() {
    let fixture = Fixture::new("thread-after-group-hydration-hole");
    let canonical_message_id = "did:example:group:6";
    fixture.seed_message_with_hydration(
        canonical_message_id,
        "group:did:example:group",
        "did:example:group",
        Some(6),
        "did:example:bob",
        "",
        crate::internal::local_state::messages::MessageHydrationState::Discovered,
    );
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    MessageSyncRuntime::new(
        &client,
        ReadyAnySessionProvider,
        RecordingTransport::queued(
            Rc::clone(&calls),
            vec![json!({
                "messages": [{
                    "id": "group-wire-6",
                    "thread_kind": "group",
                    "group_did": "did:example:group",
                    "sender_did": "did:example:bob",
                    "content": "group hydration marker",
                    "content_type": "text/plain",
                    "group_event_seq": 6
                }],
                "next_after_server_seq": "6",
                "has_more": false,
                "warnings": []
            })],
        ),
        NoopDirectoryTransport,
    )
    .sync_thread_after_async(SyncThreadAfterInput {
        request: crate::messages::SyncThreadAfterRequest {
            thread: crate::messages::ThreadRef::Group(
                crate::ids::GroupRef::parse("did:example:group").unwrap(),
            ),
            after_server_seq: Some("6".to_owned()),
            limit: Some(10),
        },
        resolved_peer_did: None,
        peer_scope: None,
    })
    .await
    .unwrap();

    assert_eq!(calls.borrow()[0].params["body"]["after_server_seq"], "5");
    assert_eq!(calls.borrow()[0].params["body"]["thread_key"], "did:example:group");
    assert_eq!(
        fixture.message_content(canonical_message_id).as_deref(),
        Some("group hydration marker")
    );
    assert_eq!(
        fixture
            .message_hydration_state(canonical_message_id)
            .as_deref(),
        Some("hydrated")
    );
}

#[tokio::test]
async fn sync_thread_after_direct_fails_before_rpc_without_durable_binding() {
    let fixture = Fixture::new("thread-after-direct-unbound");
    fixture.seed_verified_peer();
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let runtime = MessageSyncRuntime::new(
        &client,
        ReadyAnySessionProvider,
        RecordingTransport::queued(Rc::clone(&calls), Vec::new()),
        NoopDirectoryTransport,
    );

    let error = runtime
        .sync_thread_after_async(SyncThreadAfterInput {
            request: crate::messages::SyncThreadAfterRequest {
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                ),
                after_server_seq: Some("0".to_owned()),
                limit: Some(10),
            },
            resolved_peer_did: Some("did:example:bob".to_owned()),
            peer_scope: Some(
                crate::internal::local_state::owner_scope::DirectPeerScope::new(
                    "user-bob",
                    "bob.awiki.test",
                )
                .unwrap(),
            ),
        })
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        crate::ImError::Service {
            code: Some(code),
            ..
        } if code == "SYNC_THREAD_BINDING_REQUIRED"
    ));
    assert!(calls.borrow().is_empty());
}

#[tokio::test]
async fn sync_thread_after_group_uses_raw_group_messages_since_seq() {
    let fixture = Fixture::new("thread-after-group");
    let client = fixture.client();
    let mut conversation_patches = client.messages().watch_conversation_patches().unwrap();
    assert!(matches!(
        conversation_patches.next_patch().await,
        Some(crate::messages::ConversationStorePatch::Reset { .. })
    ));
    let calls = Rc::new(RefCell::new(Vec::new()));
    let runtime = MessageSyncRuntime::new(
        &client,
        ReadyAnySessionProvider,
        RecordingTransport::queued(
            Rc::clone(&calls),
            vec![json!({
                "messages": [
                    {
                        "id": "group-new",
                        "thread_kind": "group",
                        "group_did": "did:example:group",
                        "sender_did": "did:example:bob",
                        "content": "new",
                        "content_type": "text/plain",
                        "group_event_seq": 6
                    }
                ],
                "next_after_server_seq": "6",
                "has_more": true,
                "warnings": ["partial"]
            })],
        ),
        NoopDirectoryTransport,
    );

    let result = runtime
        .sync_thread_after_async(SyncThreadAfterInput {
            request: crate::messages::SyncThreadAfterRequest {
                thread: crate::messages::ThreadRef::Group(
                    crate::ids::GroupRef::parse("did:example:group").unwrap(),
                ),
                after_server_seq: Some("5".to_owned()),
                limit: Some(50),
            },
            resolved_peer_did: None,
            peer_scope: None,
        })
        .await
        .unwrap();

    assert_eq!(
        result
            .messages
            .iter()
            .map(|message| message.metadata.server_sequence)
            .collect::<Vec<_>>(),
        vec![Some(6)]
    );
    assert_eq!(result.messages[0].metadata.server_sequence, Some(6));
    assert_eq!(result.next_after_server_seq.as_deref(), Some("6"));
    assert!(result.has_more);
    assert_eq!(result.warnings, vec!["partial"]);
    let patch = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        conversation_patches.next_patch(),
    )
    .await
    .expect("Group hydration must emit a conversation patch")
    .expect("conversation patch stream must remain open");
    assert!(
        matches!(
            &patch,
            crate::messages::ConversationStorePatch::Upsert { item, .. }
                if item.last_message.as_ref().map(|message| message.id.as_str())
                    == Some("did:example:group:6")
        ),
        "unexpected Group conversation patch: {patch:?}"
    );
    let calls = calls.borrow();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].method, "sync.thread_after");
    assert_eq!(
        calls[0].params["body"],
        json!({
            "thread_key": "did:example:group",
            "after_server_seq": "5",
            "limit": 50
        })
    );
}

#[test]
fn sync_thread_after_blocking_uses_v2_rpc_without_advancing_account_cursor() {
    let fixture = Fixture::new("thread-after-blocking-group");
    fixture.store_checkpoint("91");
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let runtime = MessageSyncRuntime::new(
        &client,
        ReadyAnySessionProvider,
        RecordingTransport::queued(
            Rc::clone(&calls),
            vec![json!({
                "messages": [{
                    "id": "group-blocking-new",
                    "thread_kind": "group",
                    "group_did": "did:example:group",
                    "sender_did": "did:example:bob",
                    "content": "new",
                    "content_type": "text/plain",
                    "group_event_seq": 12
                }],
                "next_after_server_seq": "12",
                "has_more": false,
                "warnings": []
            })],
        ),
        NoopDirectoryTransport,
    );

    let result = runtime
        .sync_thread_after(SyncThreadAfterInput {
            request: crate::messages::SyncThreadAfterRequest {
                thread: crate::messages::ThreadRef::Group(
                    crate::ids::GroupRef::parse("did:example:group").unwrap(),
                ),
                after_server_seq: Some("11".to_owned()),
                limit: Some(25),
            },
            resolved_peer_did: None,
            peer_scope: None,
        })
        .unwrap();

    assert_eq!(result.messages.len(), 1);
    assert_eq!(fixture.checkpoint().as_deref(), Some("91"));
    let calls = calls.borrow();
    assert_eq!(calls[0].method, "sync.thread_after");
    assert_eq!(
        calls[0].params["body"],
        json!({
            "thread_key": "did:example:group",
            "after_server_seq": "11",
            "limit": 25
        })
    );
}

#[test]
fn sync_thread_after_rejects_invalid_after_seq_and_limit() {
    assert!(explicit_after_server_seq(Some("01")).is_err());
    assert!(explicit_after_server_seq(Some("-1")).is_err());
    assert_eq!(explicit_after_server_seq(Some("0")).unwrap(), Some(0));
    assert!(sync_thread_after_limit(Some(0)).is_err());
    assert!(sync_thread_after_limit(Some(501)).is_err());
    assert_eq!(sync_thread_after_limit(None).unwrap(), 100);
}

struct ReadyAnySessionProvider;

impl SessionProvider for ReadyAnySessionProvider {
    fn ensure_session(
        &self,
        scope: crate::auth::AuthScope,
    ) -> crate::ImResult<crate::auth::SessionBundle> {
        assert!(matches!(
            scope,
            crate::auth::AuthScope::Messaging | crate::auth::AuthScope::GroupMessaging
        ));
        Ok(crate::auth::SessionBundle {
            subject: crate::ids::Did::parse("did:example:alice")?,
            scope,
            expires_at: None,
            refreshed: false,
            bearer_token: None,
        })
    }

    fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
        unreachable!("sync runtime should not refresh through the session provider")
    }

    fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
        unreachable!("sync runtime should not read status")
    }
}

impl crate::internal::auth::session::AsyncSessionProvider for ReadyAnySessionProvider {
    async fn ensure_session(
        &self,
        scope: crate::auth::AuthScope,
    ) -> crate::ImResult<crate::auth::SessionBundle> {
        SessionProvider::ensure_session(self, scope)
    }

    async fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
        SessionProvider::refresh_session(self)
    }

    async fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
        SessionProvider::status(self)
    }
}

struct RecordingTransport {
    calls: Rc<RefCell<Vec<RecordedCall>>>,
    responses: RefCell<VecDeque<Value>>,
}

impl RecordingTransport {
    fn queued(calls: Rc<RefCell<Vec<RecordedCall>>>, responses: Vec<Value>) -> Self {
        Self {
            calls,
            responses: RefCell::new(responses.into()),
        }
    }
}

impl AuthenticatedRpcTransport for RecordingTransport {
    fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> crate::ImResult<Value> {
        self.calls.borrow_mut().push(RecordedCall {
            endpoint: endpoint.to_owned(),
            method: method.to_owned(),
            params,
        });
        Ok(self
            .responses
            .borrow_mut()
            .pop_front()
            .unwrap_or_else(|| json!({ "messages": [], "has_more": false })))
    }
}

impl AsyncAuthenticatedRpcTransport for RecordingTransport {
    async fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> crate::ImResult<Value> {
        AuthenticatedRpcTransport::authenticated_rpc(self, endpoint, method, params)
    }
}

struct RecordedCall {
    endpoint: String,
    method: String,
    params: Value,
}

struct NoopDirectoryTransport;

impl RpcTransport for NoopDirectoryTransport {
    fn rpc(&mut self, _endpoint: &str, _method: &str, _params: Value) -> crate::ImResult<Value> {
        Err(crate::ImError::PeerNotFound {
            peer: "noop-directory".to_owned(),
        })
    }
}

impl AsyncRpcTransport for NoopDirectoryTransport {
    async fn rpc(&mut self, endpoint: &str, method: &str, params: Value) -> crate::ImResult<Value> {
        RpcTransport::rpc(self, endpoint, method, params)
    }
}

struct StaticHandleDirectoryTransport;

impl RpcTransport for StaticHandleDirectoryTransport {
    fn rpc(&mut self, _endpoint: &str, method: &str, params: Value) -> crate::ImResult<Value> {
        let did = params
            .get("did")
            .and_then(Value::as_str)
            .unwrap_or("did:example:bob");
        if method == "lookup" {
            return Ok(json!({
                "handle": "bob",
                "full_handle": "bob.awiki.test",
                "did": did,
                "domain": "awiki.test",
                "status": "active",
                "user_id": "user-bob"
            }));
        }
        Ok(json!({
            "did": did,
            "service_endpoints": []
        }))
    }
}

impl AsyncRpcTransport for StaticHandleDirectoryTransport {
    async fn rpc(&mut self, endpoint: &str, method: &str, params: Value) -> crate::ImResult<Value> {
        RpcTransport::rpc(self, endpoint, method, params)
    }
}

struct FixedLookupDirectoryTransport(Value);

impl RpcTransport for FixedLookupDirectoryTransport {
    fn rpc(&mut self, _endpoint: &str, _method: &str, _params: Value) -> crate::ImResult<Value> {
        Ok(self.0.clone())
    }
}

impl AsyncRpcTransport for FixedLookupDirectoryTransport {
    async fn rpc(&mut self, endpoint: &str, method: &str, params: Value) -> crate::ImResult<Value> {
        RpcTransport::rpc(self, endpoint, method, params)
    }
}

struct Fixture {
    root: PathBuf,
    owner_did: String,
}

impl Fixture {
    fn new(name: &str) -> Self {
        Self::new_with_did(name, "did:example:alice")
    }

    fn new_with_did(name: &str, owner_did: &str) -> Self {
        let root = unique_temp_root(name);
        let identities = root.join("identities");
        fs::create_dir_all(identities.join("alice")).unwrap();
        fs::write(identities.join("default"), "alice\n").unwrap();
        fs::write(
            identities.join("registry.json"),
            serde_json::to_vec_pretty(&json!({
                "default_identity": "alice",
                "identities": [{
                    "id": "alice-id",
                    "did": owner_did,
                    "local_alias": "alice",
                    "ready_for_auth": true,
                    "ready_for_messaging": true,
                    "missing": []
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        Self {
            root,
            owner_did: owner_did.to_owned(),
        }
    }

    fn client(&self) -> crate::core::ImClient {
        crate::core::ImCore::new(
            crate::ImCoreConfig {
                service_base_url: crate::ServiceEndpoint::parse("https://example.test").unwrap(),
                did_domain: "awiki.test".to_owned(),
                user_service_endpoint: None,
                message_service_endpoint: None,
                mail_service_endpoint: None,
                anp_service_endpoint: None,
                anp_service_did: None,
                ca_bundle: None,
                transport_policy: crate::MessageTransportPolicy::HttpOnly,
            },
            crate::ImCorePaths {
                identities: crate::paths::IdentityRegistryPaths {
                    identity_root_dir: self.root.join("identities"),
                    registry_path: self.root.join("identities").join("registry.json"),
                    default_identity_path: Some(self.root.join("identities").join("default")),
                },
                local_state: crate::paths::LocalStatePaths {
                    sqlite_path: self.root.join("local").join("im.sqlite"),
                },
                runtime: crate::paths::RuntimePaths {
                    cache_dir: self.root.join("cache"),
                    temp_dir: self.root.join("tmp"),
                },
            },
        )
        .unwrap()
        .client(crate::identity::IdentitySelector::LocalAlias(
            "alice".to_owned(),
        ))
        .unwrap()
    }

    fn sqlite_path(&self) -> PathBuf {
        self.root.join("local").join("im.sqlite")
    }

    fn seed_verified_peer(&self) -> String {
        let mut db = crate::internal::local_state::open_writable(&self.sqlite_path()).unwrap();
        crate::internal::local_state::peer_personas::project_verified_handle(
            &mut db,
            "alice-id",
            &self.owner_did,
            &crate::directory::HandleLookupResult {
                handle: crate::ids::Handle::parse("bob.awiki.test", "").unwrap(),
                did: crate::ids::Did::parse("did:example:bob").unwrap(),
                user_id: "user-bob".to_owned(),
                domain: Some("awiki.test".to_owned()),
                status: Some("active".to_owned()),
                binding_generation: Some("1".to_owned()),
                profile: None,
                warnings: Vec::new(),
            },
        )
        .unwrap()
    }

    fn seed_sync_thread_binding(
        &self,
        conversation_id: &str,
        remote_thread_key: &str,
        thread_kind: &str,
    ) {
        let db = crate::internal::local_state::open_writable(&self.sqlite_path()).unwrap();
        crate::internal::local_state::sync_v2::upsert_identity_account_binding(
            &db,
            &crate::internal::local_state::sync_v2::IdentityAccountBinding {
                owner_identity_id: "alice-id".to_owned(),
                account_id: "account-alice".to_owned(),
                handle_scope: Some("alice.awiki.test".to_owned()),
                current_did: "did:example:alice".to_owned(),
                protocol_device_id: "device-alice".to_owned(),
                identity_generation: "1".to_owned(),
                device_auth_generation: "1".to_owned(),
                created_at: 1,
                updated_at: 1,
            },
        )
        .unwrap();
        crate::internal::local_state::sync_v2::upsert_sync_thread_binding(
            &db,
            &crate::internal::local_state::sync_v2::SyncThreadBinding {
                owner_identity_id: "alice-id".to_owned(),
                remote_thread_key: remote_thread_key.to_owned(),
                thread_kind: thread_kind.to_owned(),
                conversation_id: conversation_id.to_owned(),
                updated_at: 1,
            },
        )
        .unwrap();
    }

    fn seed_message(
        &self,
        msg_id: &str,
        conversation_id: &str,
        group_did: &str,
        server_seq: Option<i64>,
        sender_did: &str,
        receiver_did: &str,
    ) {
        self.seed_message_with_hydration(
            msg_id,
            conversation_id,
            group_did,
            server_seq,
            sender_did,
            receiver_did,
            crate::internal::local_state::messages::MessageHydrationState::Hydrated,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn seed_message_with_hydration(
        &self,
        msg_id: &str,
        conversation_id: &str,
        group_did: &str,
        server_seq: Option<i64>,
        sender_did: &str,
        receiver_did: &str,
        hydration_state: crate::internal::local_state::messages::MessageHydrationState,
    ) {
        let db = crate::internal::local_state::open_writable(&self.sqlite_path()).unwrap();
        crate::internal::local_state::messages::upsert_message(
            &db,
            &crate::internal::local_state::messages::MessageRecord {
                msg_id: msg_id.to_owned(),
                owner_identity_id: "alice-id".to_owned(),
                owner_did: "did:example:alice".to_owned(),
                conversation_id: conversation_id.to_owned(),
                thread_id: conversation_id.to_owned(),
                direction: 0,
                sender_did: sender_did.to_owned(),
                receiver_did: receiver_did.to_owned(),
                group_id: String::new(),
                group_did: group_did.to_owned(),
                content_type: "text/plain".to_owned(),
                content: if hydration_state
                    == crate::internal::local_state::messages::MessageHydrationState::Hydrated
                {
                    "local".to_owned()
                } else {
                    String::new()
                },
                server_seq,
                hydration_state,
                sent_at: "2026-06-27T00:00:00Z".to_owned(),
                stored_at: "2026-06-27T00:00:00Z".to_owned(),
                ..Default::default()
            },
        )
        .unwrap();
    }

    fn store_checkpoint(&self, event_seq: &str) {
        self.store_checkpoint_for_subject(&self.owner_did, event_seq);
    }

    fn store_checkpoint_for_subject(&self, sync_subject_id: &str, event_seq: &str) {
        let mut db = crate::internal::local_state::open_writable(&self.sqlite_path()).unwrap();
        let tx = db.transaction().unwrap();
        crate::internal::local_state::sync_state::store_global_checkpoint_tx(
            &tx,
            "alice-id",
            sync_subject_id,
            event_seq,
            None,
        )
        .unwrap();
        tx.commit().unwrap();
    }

    fn checkpoint(&self) -> Option<String> {
        self.checkpoint_for_subject(&self.owner_did)
    }

    fn checkpoint_for_subject(&self, sync_subject_id: &str) -> Option<String> {
        let db = crate::internal::local_state::open_writable(&self.sqlite_path()).unwrap();
        crate::internal::local_state::sync_state::load_global_checkpoint(
            &db,
            "alice-id",
            sync_subject_id,
        )
        .unwrap()
        .map(|checkpoint| checkpoint.event_seq)
    }

    fn message_server_seq(&self, msg_id: &str) -> Option<i64> {
        let db = rusqlite::Connection::open(self.sqlite_path()).unwrap();
        db.query_row(
            "SELECT server_seq FROM messages WHERE owner_identity_id = 'alice-id' AND msg_id = ?1",
            rusqlite::params![msg_id],
            |row| row.get::<_, Option<i64>>(0),
        )
        .ok()
        .flatten()
    }

    fn message_content(&self, msg_id: &str) -> Option<String> {
        let db = rusqlite::Connection::open(self.sqlite_path()).unwrap();
        db.query_row(
            "SELECT content FROM messages WHERE owner_identity_id = 'alice-id' AND msg_id = ?1",
            rusqlite::params![msg_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .ok()
        .flatten()
    }

    fn message_content_type(&self, msg_id: &str) -> Option<String> {
        let db = rusqlite::Connection::open(self.sqlite_path()).unwrap();
        db.query_row(
            "SELECT content_type FROM messages WHERE owner_identity_id = 'alice-id' AND msg_id = ?1",
            rusqlite::params![msg_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .ok()
        .flatten()
    }

    fn message_hydration_state(&self, msg_id: &str) -> Option<String> {
        let db = rusqlite::Connection::open(self.sqlite_path()).unwrap();
        db.query_row(
            "SELECT hydration_state FROM messages WHERE owner_identity_id = 'alice-id' AND msg_id = ?1",
            rusqlite::params![msg_id],
            |row| row.get::<_, String>(0),
        )
        .ok()
    }

    fn message_conversation_and_wire_identity(
        &self,
        msg_id: &str,
    ) -> Option<(String, String, String, String)> {
        let db = rusqlite::Connection::open(self.sqlite_path()).unwrap();
        db.query_row(
            r#"SELECT conversation_id, wire_thread_kind, wire_thread_ref,
                      wire_identity_resolution_state
FROM messages
WHERE owner_identity_id = 'alice-id' AND msg_id = ?1"#,
            rusqlite::params![msg_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .ok()
    }

    fn message_is_read(&self, msg_id: &str) -> Option<bool> {
        let db = rusqlite::Connection::open(self.sqlite_path()).unwrap();
        db.query_row(
            "SELECT is_read FROM messages WHERE owner_identity_id = 'alice-id' AND msg_id = ?1",
            rusqlite::params![msg_id],
            |row| row.get::<_, i64>(0),
        )
        .ok()
        .map(|value| value != 0)
    }

    fn conversation_last_content(&self, conversation_id: &str) -> Option<String> {
        let db = rusqlite::Connection::open(self.sqlite_path()).unwrap();
        db.query_row(
            "SELECT last_content FROM conversation_summaries WHERE owner_identity_id = 'alice-id' AND conversation_id = ?1",
            rusqlite::params![conversation_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .ok()
        .flatten()
    }

    fn message_count(&self, msg_id: &str) -> i64 {
        let db = rusqlite::Connection::open(self.sqlite_path()).unwrap();
        db.query_row(
            "SELECT COUNT(*) FROM messages WHERE owner_identity_id = 'alice-id' AND msg_id = ?1",
            rusqlite::params![msg_id],
            |row| row.get::<_, i64>(0),
        )
        .unwrap()
    }

    fn unresolved_backlog_count(&self) -> u64 {
        let db = crate::internal::local_state::open_writable(&self.sqlite_path()).unwrap();
        crate::internal::local_state::inbound_resolution_backlog::pending_count(&db, "alice-id")
            .unwrap()
    }
}

fn unique_temp_root(name: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let counter = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "im-core-sync-runtime-{name}-{}-{nanos}-{counter}",
        std::process::id()
    ))
}

fn delta_page(events: Vec<Value>, next_event_seq: &str, has_more: bool) -> Value {
    json!({
        "events": events,
        "next_event_seq": next_event_seq,
        "has_more": has_more,
        "snapshot_required": false,
        "retention_floor_event_seq": "1",
        "warnings": [],
    })
}

fn message_created_event_without_content(
    event_id: &str,
    event_seq: &str,
    message_id: &str,
    server_seq: i64,
) -> Value {
    json!({
        "event_id": event_id,
        "event_seq": event_seq,
        "event_type": "message.created",
        "aggregate_kind": "direct_message",
        "aggregate_id": message_id,
        "owner_subject_id": "did:example:alice",
        "created_at": "2026-06-27T00:00:00Z",
        "payload": {
            "thread_kind": "direct",
            "thread": {
                "kind": "direct",
                "peer_did": "did:example:bob"
            },
            "message": {
                "id": message_id,
                "server_seq": server_seq.to_string(),
                "sender_did": "did:example:bob",
                "receiver_did": "did:example:alice",
                "content_type": "text/plain",
                "sent_at": "2026-06-27T00:00:00Z"
            }
        }
    })
}

fn outbound_message_created_event_without_content(
    event_id: &str,
    event_seq: &str,
    message_id: &str,
    server_seq: i64,
) -> Value {
    json!({
        "event_id": event_id,
        "event_seq": event_seq,
        "event_type": "message.created",
        "aggregate_kind": "direct_message",
        "aggregate_id": message_id,
        "owner_subject_id": "did:example:alice",
        "created_at": "2026-07-27T00:00:00Z",
        "payload": {
            "thread_kind": "direct",
            "thread": {
                "kind": "direct",
                "peer_did": "did:example:bob"
            },
            "message": {
                "id": message_id,
                "server_seq": server_seq.to_string(),
                "sender_did": "did:example:alice",
                "receiver_did": "did:example:bob",
                "content_type": "text/plain",
                "sent_at": "2026-07-27T00:00:00Z"
            }
        }
    })
}

fn group_message_created_event_without_content(
    event_id: &str,
    event_seq: &str,
    message_id: &str,
    server_seq: i64,
) -> Value {
    json!({
        "event_id": event_id,
        "event_seq": event_seq,
        "event_type": "message.created",
        "aggregate_kind": "group_message",
        "aggregate_id": message_id,
        "owner_subject_id": "did:example:alice",
        "created_at": "2026-07-27T00:00:00Z",
        "payload": {
            "thread_kind": "group",
            "thread": {
                "kind": "group",
                "group_did": "did:example:group"
            },
            "message": {
                "id": message_id,
                "server_seq": server_seq.to_string(),
                "group_event_seq": server_seq.to_string(),
                "sender_did": "did:example:bob",
                "group_did": "did:example:group",
                "content_type": "text/plain",
                "sent_at": "2026-07-27T00:00:00Z"
            }
        }
    })
}

fn message_created_event(
    event_id: &str,
    event_seq: &str,
    message_id: &str,
    server_seq: i64,
) -> Value {
    message_created_event_for_owner(
        event_id,
        event_seq,
        message_id,
        server_seq,
        "did:example:alice",
    )
}

fn message_created_event_for_owner(
    event_id: &str,
    event_seq: &str,
    message_id: &str,
    server_seq: i64,
    owner_did: &str,
) -> Value {
    json!({
        "event_id": event_id,
        "event_seq": event_seq,
        "event_type": "message.created",
        "aggregate_kind": "direct_message",
        "aggregate_id": message_id,
        "owner_subject_id": owner_did,
        "created_at": "2026-06-27T00:00:00Z",
        "payload": {
            "thread_kind": "direct",
            "thread": {
                "kind": "direct",
                "peer_did": "did:example:bob"
            },
            "message": {
                "id": message_id,
                "server_seq": server_seq.to_string(),
                "sender_did": "did:example:bob",
                "receiver_did": owner_did,
                "content_type": "text/plain",
                "content": "hello from sync.delta",
                "sent_at": "2026-06-27T00:00:00Z"
            }
        }
    })
}

fn v2_message_event(
    event_id: &str,
    event_seq: &str,
    event_type: &str,
    message_id: &str,
    meta: Value,
    json_rpc_shape: bool,
) -> Value {
    let private_message = if json_rpc_shape {
        json!({
            "id": message_id,
            "params": {
                "meta": meta,
                "body": {"ciphertext_b64u": "PRIVATE-CIPHERTEXT"}
            }
        })
    } else {
        json!({
            "id": message_id,
            "meta": meta,
            "body": {"ciphertext_b64u": "PRIVATE-CIPHERTEXT"}
        })
    };
    let message_key = if event_type == "conversation.updated" {
        "latest_message"
    } else {
        "message"
    };
    json!({
        "event_id": event_id,
        "event_seq": event_seq,
        "event_type": event_type,
        "aggregate_kind": "direct_message",
        "aggregate_id": message_id,
        "owner_subject_id": "did:example:alice",
        "created_at": "2026-07-20T00:00:00Z",
        "payload": {
            "thread_kind": "direct",
            "thread": {
                "kind": "direct",
                "peer_did": "did:example:bob"
            },
            (message_key): private_message
        }
    })
}

fn attachment_manifest_message_created_event(
    event_id: &str,
    event_seq: &str,
    message_id: &str,
    server_seq: i64,
) -> Value {
    json!({
        "event_id": event_id,
        "event_seq": event_seq,
        "event_type": "message.created",
        "aggregate_kind": "group_message",
        "aggregate_id": message_id,
        "owner_subject_id": "did:example:alice",
        "created_at": "2026-06-27T00:00:00Z",
        "payload": {
            "thread_kind": "group",
            "thread": {
                "kind": "group",
                "group_did": "did:example:group"
            },
            "message": {
                "id": message_id,
                "server_seq": server_seq.to_string(),
                "group_event_seq": server_seq.to_string(),
                "sender_did": "did:example:bob",
                "group_did": "did:example:group",
                "content_type": crate::attachments::manifest::attachment_manifest_content_type(),
                "content": {
                    "attachments": [{
                        "attachment_id": "att-sync-1",
                        "filename": "sync.md",
                        "mime_type": "text/markdown",
                        "size": "12"
                    }],
                    "caption": "sync attachment",
                    "primary_attachment_id": "att-sync-1"
                },
                "sent_at": "2026-06-27T00:00:00Z"
            }
        }
    })
}

fn group_member_changed_event(event_id: &str, event_seq: &str, group_event_seq: i64) -> Value {
    json!({
        "event_id": event_id,
        "event_seq": event_seq,
        "event_type": "group.member_changed",
        "aggregate_kind": "group",
        "aggregate_id": "did:example:group",
        "owner_subject_id": "did:example:alice",
        "created_at": "2026-07-07T00:00:00Z",
        "payload": {
            "thread_kind": "group",
            "thread": {
                "kind": "group",
                "group_did": "did:example:group"
            },
            "group": {
                "group_did": "did:example:group",
                "group_state_version": "3",
                "group_event_seq": group_event_seq.to_string()
            },
            "membership": {
                "actor_did": "did:example:alice",
                "subject_did": "did:example:bob",
                "status": "active"
            }
        }
    })
}

#[test]
fn thread_after_client_defense_keeps_only_ordinary_plain_rows() {
    let mut direct = json!({
        "messages": [
            {"id": "plain", "security_profile": "transport-protected"},
            {"id": "p5", "security_profile": "anp.direct.e2ee.v2"},
            {
                "id": "device-copy",
                "security_profile": "transport-protected",
                "recipient_device_id": "device-2"
            }
        ]
    });
    super::retain_ordinary_plain_wire_messages(&mut direct, "direct");
    assert_eq!(
        direct
            .get("messages")
            .and_then(Value::as_array)
            .unwrap()
            .iter()
            .map(|row| row.get("id").and_then(Value::as_str).unwrap())
            .collect::<Vec<_>>(),
        vec!["plain"]
    );

    let mut group = json!({
        "messages": [
            {"id": "plain", "security_profile": "transport-protected", "subject_method": "group.send"},
            {"id": "mls", "security_profile": "transport-protected", "subject_method": "group.e2ee.send"}
        ]
    });
    super::retain_ordinary_plain_wire_messages(&mut group, "group");
    assert_eq!(
        group
            .get("messages")
            .and_then(Value::as_array)
            .unwrap()
            .iter()
            .map(|row| row.get("id").and_then(Value::as_str).unwrap())
            .collect::<Vec<_>>(),
        vec!["plain"]
    );
}
