#[test]
fn dart_error_unsupported_has_stable_code() {
    let err = awiki_im_core::dto::error::DartImError::unsupported("relationship-remote-mutation");
    assert_eq!(err.code, "unsupported_capability");
    assert_eq!(
        err.capability.as_deref(),
        Some("relationship-remote-mutation")
    );
}

#[test]
fn retry_message_is_explicitly_unsupported_until_im_core_has_retry_api() {
    let err = awiki_im_core::dto::error::DartImError::unsupported("message-retry");
    assert_eq!(err.code, "unsupported_capability");
    assert_eq!(err.capability.as_deref(), Some("message-retry"));
}

#[test]
fn service_error_preserves_server_code_and_data_for_dart() {
    let err = awiki_im_core::dto::error::DartImError::from(im_core::ImError::Service {
        status_code: Some(409),
        code: Some("1007".to_string()),
        message: "target did is inactive".to_string(),
        data: Some(serde_json::json!({
            "did": "did:example:old",
            "handle": "alice",
        })),
    });

    assert_eq!(err.code, "service_error");
    assert_eq!(err.status_code, Some(409));
    assert_eq!(err.service_code.as_deref(), Some("1007"));
    assert_eq!(
        err.service_data_json.as_deref(),
        Some(r#"{"did":"did:example:old","handle":"alice"}"#)
    );
}

#[test]
fn thread_mark_read_result_preserves_best_effort_state_for_dart() {
    let core = im_core::messages::MarkThreadReadResult {
        updated_count: 1,
        remote_acknowledged: false,
        partial: true,
        fallback_used: true,
        pending_remote_ack: true,
        effective_watermark: Some(im_core::messages::ReadWatermark {
            last_read_message_id: Some(
                im_core::ids::MessageId::parse("msg-1").expect("message id"),
            ),
            last_read_thread_seq: Some("42".to_string()),
            read_at: None,
        }),
        legacy_message_ids: vec![im_core::ids::MessageId::parse("msg-1").expect("message id")],
        warnings: vec!["Remote read-state mark-read failed".to_string()],
    };

    let dart: awiki_im_core::dto::message::DartMarkThreadReadResult = core.into();

    assert_eq!(dart.updated_count, 1);
    assert!(!dart.remote_acknowledged);
    assert!(dart.partial);
    assert!(dart.fallback_used);
    assert!(dart.pending_remote_ack);
    assert_eq!(dart.legacy_message_ids, vec!["msg-1"]);
    let watermark = dart.effective_watermark.expect("effective watermark");
    assert_eq!(watermark.last_read_message_id.as_deref(), Some("msg-1"));
    assert_eq!(watermark.last_read_thread_seq.as_deref(), Some("42"));
    assert_eq!(dart.warnings, vec!["Remote read-state mark-read failed"]);
}

#[test]
fn sync_delta_request_exposes_only_app_safe_controls() {
    let request = awiki_im_core::dto::message::DartSyncDeltaRequest {
        limit: Some(100),
        device_id: Some("device-main".to_string()),
        reason: Some("app_resumed".to_string()),
    };

    let core: im_core::messages::SyncDeltaRequest = request.into();
    assert_eq!(core.limit, Some(100));
    assert_eq!(core.device_id.as_deref(), Some("device-main"));
    assert_eq!(core.reason.as_deref(), Some("app_resumed"));
}

#[test]
fn sync_delta_result_preserves_diagnostics_without_next_checkpoint_setter() {
    let core = im_core::messages::SyncDeltaResult {
        events_applied: 3,
        pages_fetched: 2,
        last_applied_event_seq: Some("42".to_string()),
        has_more: false,
        snapshot_required: true,
        retention_floor_event_seq: Some("10".to_string()),
        warnings: vec!["snapshot required".to_string()],
    };

    let dart: awiki_im_core::dto::message::DartSyncDeltaResult = core.into();
    assert_eq!(dart.events_applied, 3);
    assert_eq!(dart.pages_fetched, 2);
    assert_eq!(dart.last_applied_event_seq.as_deref(), Some("42"));
    assert!(!dart.has_more);
    assert!(dart.snapshot_required);
    assert_eq!(dart.retention_floor_event_seq.as_deref(), Some("10"));
    assert_eq!(dart.warnings, vec!["snapshot required"]);
}

#[test]
fn sync_thread_after_request_uses_thread_local_sequence_only() {
    let request = awiki_im_core::dto::message::DartSyncThreadAfterRequest {
        thread: awiki_im_core::dto::message::DartThreadRef::Direct {
            peer: "did:example:bob".to_string(),
        },
        after_server_seq: Some("991".to_string()),
        limit: Some(50),
    };

    let core: im_core::messages::SyncThreadAfterRequest =
        request.try_into().expect("sync thread-after maps");
    assert!(matches!(
        core.thread,
        im_core::messages::ThreadRef::Direct(peer) if peer.as_str() == "did:example:bob"
    ));
    assert_eq!(core.after_server_seq.as_deref(), Some("991"));
    assert_eq!(core.limit, Some(50));
}

#[test]
fn sync_conversation_after_request_uses_canonical_conversation_ref() {
    let request = awiki_im_core::dto::message::DartSyncConversationAfterRequest {
        conversation: awiki_im_core::dto::message::DartConversationReadRef {
            conversation_id: "dm:peer-scope:v1:abc".to_string(),
        },
        after_server_seq: Some("992".to_string()),
        limit: Some(25),
    };

    let core: im_core::messages::SyncConversationAfterRequest =
        request.try_into().expect("sync conversation-after maps");
    assert_eq!(core.conversation.conversation_id, "dm:peer-scope:v1:abc");
    assert!(matches!(
        core.conversation.as_thread_ref().expect("conversation thread ref"),
        im_core::messages::ThreadRef::Thread(thread)
            if thread.as_str() == "dm:peer-scope:v1:abc"
    ));
    assert_eq!(core.after_server_seq.as_deref(), Some("992"));
    assert_eq!(core.limit, Some(25));
}

#[test]
fn sync_thread_after_result_preserves_ordered_message_page_metadata() {
    let peer_scope_thread_id =
        im_core::messages::direct_peer_scope_thread_id("user-bob", "bob.example")
            .expect("peer-scope thread id");
    let conversation_identity = im_core::messages::ConversationIdentity::from_thread_ref_for_owner(
        &im_core::messages::ThreadRef::Thread(peer_scope_thread_id),
        "did:example:alice",
    );
    let core = im_core::messages::SyncThreadAfterResult {
        messages: vec![im_core::messages::Message {
            id: im_core::ids::MessageId::parse("msg-1").expect("message id"),
            thread: im_core::messages::ThreadRef::Direct(
                im_core::ids::PeerRef::parse("did:example:bob", "example.com").expect("peer ref"),
            ),
            direction: im_core::messages::MessageDirection::Incoming,
            sender: im_core::ids::PeerRef::parse("did:example:bob", "example.com")
                .expect("sender ref"),
            receiver: Some(
                im_core::ids::PeerRef::parse("did:example:alice", "example.com")
                    .expect("receiver ref"),
            ),
            group: None,
            body: im_core::messages::MessageBodyView::Text {
                text: "hello".to_string(),
                kind: im_core::messages::MessageKind::Text,
            },
            sent_at: None,
            received_at: None,
            metadata: im_core::messages::MessageMetadata {
                server_sequence: Some(992),
                conversation_identity: Some(conversation_identity),
                ..Default::default()
            },
        }],
        next_after_server_seq: Some("992".to_string()),
        has_more: false,
        warnings: vec![],
    };

    let dart: awiki_im_core::dto::message::DartSyncThreadAfterResult = core.into();
    assert_eq!(dart.messages.len(), 1);
    assert_eq!(dart.messages[0].id, "msg-1");
    assert_eq!(dart.messages[0].metadata.server_sequence, Some(992));
    let identity = dart.messages[0]
        .metadata
        .conversation_identity
        .as_ref()
        .expect("conversation identity");
    assert!(identity.conversation_id.starts_with("dm:peer-scope:v1:"));
    assert_eq!(identity.canonical_thread_kind, "direct");
    assert_eq!(identity.canonical_thread_id, identity.conversation_id);
    assert_eq!(identity.storage_thread_ref.kind, "thread");
    assert!(identity
        .storage_thread_ref
        .id
        .starts_with("dm:peer-scope:v1:"));
    assert_eq!(
        identity.identity_scope,
        awiki_im_core::dto::message::DartConversationIdentityScope::Direct
    );
    assert_eq!(
        identity.migration_state,
        awiki_im_core::dto::message::DartConversationMigrationState::Canonical
    );
    assert!(identity.aliases.is_empty());
    assert_eq!(dart.next_after_server_seq.as_deref(), Some("992"));
    assert!(!dart.has_more);
}

#[test]
fn conversation_store_patches_preserve_identity_for_removal_and_reorder() {
    let identity = im_core::messages::ConversationIdentity::from_storage_parts_for_owner(
        "thread",
        "dm:did:example:alice:did:example:bob",
        "did:example:alice",
    );
    let remove = im_core::messages::ConversationStorePatch::Remove {
        owner_identity_id: "identity-1".to_string(),
        owner_did: "did:example:alice".to_string(),
        version: 7,
        unread_total: 0,
        thread_kind: "thread".to_string(),
        thread_id: "dm:did:example:alice:did:example:bob".to_string(),
        conversation_identity: Some(identity.clone()),
    };
    let reorder = im_core::messages::ConversationStorePatch::Reorder {
        owner_identity_id: "identity-1".to_string(),
        owner_did: "did:example:alice".to_string(),
        version: 8,
        unread_total: 0,
        thread_kind: "thread".to_string(),
        thread_id: "dm:did:example:alice:did:example:bob".to_string(),
        conversation_identity: Some(identity),
        index: 1,
    };

    let remove: awiki_im_core::dto::message::DartConversationStorePatch = remove.into();
    let reorder: awiki_im_core::dto::message::DartConversationStorePatch = reorder.into();

    match remove {
        awiki_im_core::dto::message::DartConversationStorePatch::Remove {
            conversation_identity,
            ..
        } => {
            let identity = conversation_identity.expect("remove identity");
            assert_eq!(identity.conversation_id, "dm:did:example:bob");
            assert_eq!(
                identity.migration_state,
                awiki_im_core::dto::message::DartConversationMigrationState::LegacyInput
            );
            assert!(identity.aliases.iter().any(|alias| {
                alias.source
                    == awiki_im_core::dto::message::DartConversationAliasSource::OldFlutterSortedDirect
                    && alias.id == "dm:did:example:alice:did:example:bob"
            }));
        }
        other => panic!("expected remove patch, got {other:?}"),
    }

    match reorder {
        awiki_im_core::dto::message::DartConversationStorePatch::Reorder {
            conversation_identity,
            index,
            ..
        } => {
            assert_eq!(index, 1);
            let identity = conversation_identity.expect("reorder identity");
            assert_eq!(identity.conversation_id, "dm:did:example:bob");
        }
        other => panic!("expected reorder patch, got {other:?}"),
    }
}

#[test]
fn local_history_query_is_public_core_contract_for_dart_facade() {
    let query = im_core::messages::LocalHistoryQuery {
        limit: im_core::ids::PageLimit::new(50).expect("limit"),
        cursor: Some(im_core::ids::Cursor::parse("local-history:v1:dHM:bXNn").expect("cursor")),
    };

    assert_eq!(query.limit.0, 50);
    assert_eq!(
        query.cursor.as_ref().map(im_core::ids::Cursor::as_str),
        Some("local-history:v1:dHM:bXNn")
    );
}

#[test]
fn attachment_request_maps_bytes_input_without_bytes_len_placeholder() {
    let request = awiki_im_core::dto::attachment::DartAttachmentSendRequest {
        target: awiki_im_core::dto::message::DartMessageTarget::Direct {
            peer: "did:example:bob".to_string(),
        },
        input: awiki_im_core::dto::attachment::DartAttachmentInput::Bytes {
            filename: Some("note.txt".to_string()),
            mime_type: Some("text/plain".to_string()),
            bytes: b"hello".to_vec(),
        },
        caption: Some("caption".to_string()),
        mention_payload_json: Some(
            serde_json::json!({
                "text": "@Hermes caption",
                "mentions": [{
                    "id": "men_agent",
                    "range": {"start": 0, "end": 7, "unit": "unicode_code_point"},
                    "target": {"kind": "agent", "did": "did:agent:hermes"},
                    "mention_role": "addressee"
                }]
            })
            .to_string(),
        ),
        mime_type: None,
        filename: None,
        security: awiki_im_core::dto::message::DartMessageSecurityMode::E2eeRequired,
        idempotency_key: Some("idem-1".to_string()),
        wait_for_final_acceptance: true,
    };

    let (target, request) = request.into_core().expect("attachment maps to im-core");
    assert!(matches!(
        target,
        im_core::messages::MessageTarget::Direct(peer) if peer.as_str() == "did:example:bob"
    ));
    assert!(matches!(
        request.input,
        im_core::attachments::AttachmentInput::Bytes { bytes, .. } if bytes == b"hello".to_vec()
    ));
    assert_eq!(request.delivery.idempotency_key.as_deref(), Some("idem-1"));
    assert_eq!(
        request
            .mention_payload
            .as_ref()
            .and_then(|payload| payload.get("mentions"))
            .and_then(serde_json::Value::as_array)
            .map(Vec::len),
        Some(1)
    );
    assert!(request.delivery.wait_for_final_acceptance);
    assert!(matches!(
        request.security,
        im_core::messages::MessageSecurityMode::E2eeRequired
    ));
}

#[test]
fn conversation_attachment_request_maps_to_core_conversation_contract() {
    let request = awiki_im_core::dto::attachment::DartSendConversationAttachmentRequest {
        conversation: awiki_im_core::dto::message::DartConversationReadRef {
            conversation_id: "dm:did:example:bob".to_string(),
        },
        input: awiki_im_core::dto::attachment::DartAttachmentInput::Bytes {
            filename: Some("note.txt".to_string()),
            mime_type: Some("text/plain".to_string()),
            bytes: b"hello".to_vec(),
        },
        caption: Some("caption".to_string()),
        mention_payload_json: Some(
            serde_json::json!({
                "text": "@Hermes caption",
                "mentions": [{
                    "id": "men_agent",
                    "range": {"start": 0, "end": 7, "unit": "unicode_code_point"},
                    "target": {"kind": "agent", "did": "did:agent:hermes"},
                    "mention_role": "addressee"
                }]
            })
            .to_string(),
        ),
        mime_type: Some("text/plain".to_string()),
        filename: None,
        security: awiki_im_core::dto::message::DartMessageSecurityMode::DefaultPlain,
        client_message_id: Some("msg-client-attachment".to_string()),
        idempotency_key: Some("op-client-attachment".to_string()),
        wait_for_final_acceptance: true,
    };

    let core: im_core::attachments::SendConversationAttachmentRequest =
        request.try_into().expect("conversation attachment maps");
    assert_eq!(core.conversation.conversation_id, "dm:did:example:bob");
    assert!(matches!(
        core.input,
        im_core::attachments::AttachmentInput::Bytes { bytes, .. } if bytes == b"hello".to_vec()
    ));
    assert_eq!(
        core.client_message_id
            .as_ref()
            .map(im_core::ids::MessageId::as_str),
        Some("msg-client-attachment")
    );
    assert_eq!(
        core.idempotency_key.as_deref(),
        Some("op-client-attachment")
    );
    assert!(core.wait_for_final_acceptance);
    assert_eq!(
        core.mention_payload
            .as_ref()
            .and_then(|payload| payload.get("mentions"))
            .and_then(serde_json::Value::as_array)
            .map(Vec::len),
        Some(1)
    );
}

#[test]
fn attachment_send_result_preserves_upload_metadata_for_dart() {
    let core = im_core::attachments::AttachmentSendResult {
        message: im_core::messages::SendMessageResult {
            message: im_core::messages::Message {
                id: im_core::ids::MessageId::parse("msg-1").expect("message id"),
                thread: im_core::messages::ThreadRef::Direct(
                    im_core::ids::PeerRef::parse("did:example:bob", "example.com")
                        .expect("peer ref"),
                ),
                direction: im_core::messages::MessageDirection::Outgoing,
                sender: im_core::ids::PeerRef::parse("did:example:alice", "example.com")
                    .expect("sender ref"),
                receiver: Some(
                    im_core::ids::PeerRef::parse("did:example:bob", "example.com")
                        .expect("receiver ref"),
                ),
                group: None,
                body: im_core::messages::MessageBodyView::Unsupported {
                    content_type: Some(
                        im_core::attachments::attachment_manifest_content_type().to_string(),
                    ),
                },
                sent_at: Some("2026-05-24T00:00:00Z".to_string()),
                received_at: None,
                metadata: im_core::messages::MessageMetadata::default(),
            },
            delivery: im_core::messages::DeliveryState::Sent,
            warnings: vec!["message warning".to_string()],
        },
        target_kind: "direct".to_string(),
        target_did: "did:example:bob".to_string(),
        attachment: im_core::attachments::UploadedAttachment {
            attachment_id: "att-1".to_string(),
            filename: "note.txt".to_string(),
            mime_type: "text/plain".to_string(),
            size_bytes: 5,
            size: "5".to_string(),
            digest_b64u: "digest".to_string(),
            object_uri: "object://att-1".to_string(),
            object_encryption_mode: "object-e2ee".to_string(),
            plaintext_size_bytes: Some(4),
        },
        manifest: serde_json::json!({
            "attachments": [{
                "attachment_id": "att-1",
                "filename": "note.txt",
                "encryption_info": {
                    "mode": "object-e2ee",
                    "object_cipher": "chacha20-poly1305",
                    "plaintext_size": "4"
                }
            }]
        }),
    };

    let dart: awiki_im_core::dto::attachment::DartAttachmentSendResult = core.into();
    assert_eq!(dart.message.message.id, "msg-1");
    assert_eq!(dart.message.delivery_state, "sent");
    assert_eq!(dart.message.warnings, vec!["message warning"]);
    assert_eq!(dart.target_kind, "direct");
    assert_eq!(dart.target_did, "did:example:bob");
    assert_eq!(dart.attachment.attachment_id, "att-1");
    assert_eq!(dart.attachment.filename, "note.txt");
    assert_eq!(dart.attachment.mime_type, "text/plain");
    assert_eq!(dart.attachment.size_bytes, 5);
    assert_eq!(dart.attachment.size, "5");
    assert_eq!(dart.attachment.digest_b64u, "digest");
    assert_eq!(dart.attachment.object_uri, "object://att-1");
    assert_eq!(dart.attachment.object_encryption_mode, "object-e2ee");
    assert_eq!(dart.attachment.plaintext_size_bytes, Some(4));

    let manifest: serde_json::Value =
        serde_json::from_str(&dart.manifest_json).expect("manifest json is preserved");
    assert_eq!(manifest["attachments"][0]["attachment_id"], "att-1");
    assert_eq!(
        manifest["attachments"][0]["encryption_info"]["mode"],
        "object-e2ee"
    );
    assert_eq!(
        manifest["attachments"][0]["encryption_info"].get("object_key_b64u"),
        None
    );
    assert_eq!(
        manifest["attachments"][0]["encryption_info"].get("nonce_b64u"),
        None
    );
}

#[test]
fn dart_message_security_exposes_target_independent_e2ee_required() {
    let mode = awiki_im_core::dto::message::DartMessageSecurityMode::E2eeRequired;
    let mapped: im_core::messages::MessageSecurityMode = mode.into();
    assert!(matches!(
        mapped,
        im_core::messages::MessageSecurityMode::E2eeRequired
    ));
}

#[test]
fn payload_request_and_body_view_preserve_json_for_dart() {
    let request = awiki_im_core::dto::message::DartSendPayloadRequest {
        target: awiki_im_core::dto::message::DartMessageTarget::Direct {
            peer: "did:example:bob".to_string(),
        },
        payload_json: r#"{"schema":"awiki.agent.command.v1","command":"runtime.agent.create"}"#
            .to_string(),
        security: awiki_im_core::dto::message::DartMessageSecurityMode::Plain,
        client_message_id: None,
        idempotency_key: Some("op-payload".to_string()),
        wait_for_final_acceptance: true,
        delegated_signing: None,
    };

    let core: im_core::messages::SendMessageRequest =
        request.try_into().expect("payload request maps");
    assert!(matches!(
        core.body,
        im_core::messages::MessageBody::Payload { ref payload }
            if payload["schema"] == "awiki.agent.command.v1"
                && payload["command"] == "runtime.agent.create"
    ));
    assert_eq!(core.delivery.idempotency_key.as_deref(), Some("op-payload"));
    assert!(core.delivery.wait_for_final_acceptance);

    let dart_body: awiki_im_core::dto::message::DartMessageBodyView =
        im_core::messages::MessageBodyView::Payload {
            payload: serde_json::json!({
                "schema": "awiki.agent.status.v1",
                "state": "running"
            }),
        }
        .into();
    assert!(dart_body.text.is_none());
    assert_eq!(dart_body.kind.as_deref(), Some("payload"));
    let payload: serde_json::Value =
        serde_json::from_str(dart_body.payload_json.as_deref().unwrap()).unwrap();
    assert_eq!(payload["schema"], "awiki.agent.status.v1");
    assert_eq!(payload["state"], "running");
    assert!(dart_body.unsupported_content_type.is_none());
}

#[test]
fn dart_delegated_message_options_map_to_im_core_optional_params() {
    let request = awiki_im_core::dto::message::DartSendTextRequest {
        target: awiki_im_core::dto::message::DartMessageTarget::Direct {
            peer: "did:example:bob".to_string(),
        },
        text: "hello".to_string(),
        markdown: false,
        security: awiki_im_core::dto::message::DartMessageSecurityMode::DefaultPlain,
        client_message_id: None,
        idempotency_key: None,
        wait_for_final_acceptance: false,
        delegated_signing: Some(awiki_im_core::dto::message::DartDelegatedSigningOptions {
            logical_sender_did: Some("did:example:alice".to_string()),
            signing_verification_method: Some("did:example:alice#daemon-key-1".to_string()),
            signing_key_ref: Some("local:daemon-key-1".to_string()),
            actor_agent_did: Some("did:example:daemon".to_string()),
        }),
    };

    let core: im_core::messages::SendMessageRequest =
        request.try_into().expect("text request maps");
    let delegated = core
        .delegated_signing
        .expect("delegated signing is preserved");
    assert_eq!(
        delegated.logical_sender_did.as_deref(),
        Some("did:example:alice")
    );
    assert_eq!(
        delegated.signing_verification_method.as_deref(),
        Some("did:example:alice#daemon-key-1")
    );
    assert_eq!(
        delegated.signing_key_ref.as_deref(),
        Some("local:daemon-key-1")
    );
    assert_eq!(
        delegated.actor_agent_did.as_deref(),
        Some("did:example:daemon")
    );
}

#[test]
fn dart_inbox_history_options_map_to_im_core_optional_params() {
    let options = awiki_im_core::dto::message::DartInboxHistoryOptions {
        inbox_owner_did: Some("did:example:alice".to_string()),
        inbox_auth_verification_method: Some("did:example:alice#daemon-key-1".to_string()),
        inbox_auth_key_ref: Some("local:daemon-key-1".to_string()),
        inbox_auth: Some(
            awiki_im_core::dto::message::DartInboxAuth::ScopedInboxToken {
                token: awiki_im_core::dto::message::DartScopedInboxToken {
                    token: "token-1".to_string(),
                },
            },
        ),
    };

    let core: im_core::messages::InboxHistoryOptions = options.into();
    assert_eq!(core.inbox_owner_did.as_deref(), Some("did:example:alice"));
    assert_eq!(
        core.inbox_auth_verification_method.as_deref(),
        Some("did:example:alice#daemon-key-1")
    );
    assert_eq!(
        core.inbox_auth_key_ref.as_deref(),
        Some("local:daemon-key-1")
    );
    assert!(matches!(
        core.inbox_auth,
        Some(im_core::messages::InboxAuth::ScopedInboxToken { token })
            if token.token == "token-1"
    ));
}

#[test]
fn secure_outbox_entry_does_not_expose_plaintext_or_crypto_material() {
    let entry = awiki_im_core::dto::secure::DartSecureOutboxEntry {
        id: "outbox-1".to_string(),
        target: awiki_im_core::dto::message::DartMessageTarget::Direct {
            peer: "did:example:bob".to_string(),
        },
        message_kind: "text".to_string(),
        status: awiki_im_core::dto::secure::DartSecureOutboxStatus::Failed,
        attempt_count: 2,
        last_error: Some(awiki_im_core::dto::secure::DartSecureProblem {
            code: awiki_im_core::dto::secure::DartSecureProblemCode::PeerKeysUnavailable,
            message: "peer keys unavailable".to_string(),
            retryable: true,
        }),
        created_at: Some("2026-05-24T00:00:00Z".to_string()),
        updated_at: Some("2026-05-24T00:01:00Z".to_string()),
    };

    assert_eq!(entry.id, "outbox-1");
    assert_eq!(entry.message_kind, "text");
    assert_eq!(entry.attempt_count, 2);
}

#[test]
fn config_maps_mail_service_endpoint_into_im_core() {
    let config = awiki_im_core::dto::config::DartImCoreConfig {
        service_base_url: "https://awiki.ai".to_string(),
        did_domain: "awiki.ai".to_string(),
        user_service_endpoint: None,
        message_service_endpoint: None,
        mail_service_endpoint: Some("https://mail.awiki.ai".to_string()),
        anp_service_endpoint: None,
        anp_service_did: None,
        transport_policy: awiki_im_core::dto::config::DartMessageTransportPolicy::Auto,
    };

    let core: im_core::ImCoreConfig = config
        .try_into()
        .expect("mail endpoint maps into ImCoreConfig");
    assert_eq!(
        core.mail_service_endpoint.unwrap().as_str(),
        "https://mail.awiki.ai"
    );
}

#[test]
fn vault_open_options_map_to_im_core_without_debug_secret_leak() {
    let root_key = awiki_im_core::dto::config::DartDeviceVaultRootKey {
        bytes: vec![7_u8; im_core::vault::DEVICE_VAULT_ROOT_KEY_LEN],
    };
    let debug = format!("{root_key:?}");
    assert!(debug.contains("DartDeviceVaultRootKey"));
    assert!(debug.contains("[REDACTED]"));
    assert!(!debug.contains("7, 7"));

    let options = awiki_im_core::dto::config::DartImCoreOpenOptions {
        identity_secret_storage_policy:
            awiki_im_core::dto::config::DartIdentitySecretStoragePolicy::VaultRequired,
        identity_secret_vault: Some(awiki_im_core::dto::config::DartImCoreSecretVaultOptions {
            root_key,
            vault_dir: "/tmp/awiki-vault".to_string(),
            workspace_id: "workspace-a".to_string(),
            device_id: "device-a".to_string(),
        }),
    };

    let mapped: im_core::ImCoreOpenOptions = options.try_into().expect("open options map");
    assert!(matches!(
        mapped.identity_secret_storage_policy,
        im_core::IdentitySecretStoragePolicy::VaultRequired
    ));
    let vault = mapped.identity_secret_vault.expect("vault options");
    assert_eq!(
        vault.vault_dir,
        std::path::PathBuf::from("/tmp/awiki-vault")
    );
    assert_eq!(vault.workspace_id, "workspace-a");
    assert_eq!(vault.device_id, "device-a");
    let vault_debug = format!("{vault:?}");
    assert!(vault_debug.contains("<redacted-root-key>"));
    assert!(!vault_debug.contains("7, 7"));
}

#[test]
fn vault_root_key_mapping_rejects_wrong_length_without_echoing_secret() {
    let options = awiki_im_core::dto::config::DartImCoreOpenOptions {
        identity_secret_storage_policy:
            awiki_im_core::dto::config::DartIdentitySecretStoragePolicy::VaultPreferred,
        identity_secret_vault: Some(awiki_im_core::dto::config::DartImCoreSecretVaultOptions {
            root_key: awiki_im_core::dto::config::DartDeviceVaultRootKey {
                bytes: b"short-secret".to_vec(),
            },
            vault_dir: "/tmp/awiki-vault".to_string(),
            workspace_id: "workspace-a".to_string(),
            device_id: "device-a".to_string(),
        }),
    };

    let error = im_core::ImCoreOpenOptions::try_from(options).unwrap_err();
    assert_eq!(error.code, "invalid_input");
    assert_eq!(error.field.as_deref(), Some("root_key"));
    assert!(error.message.contains("32 bytes"));
    assert!(!error.message.contains("short-secret"));
}

#[test]
fn identity_vault_status_maps_without_secret_refs() {
    let core_status = im_core::identity::IdentityVaultStatus {
        identity: im_core::identity::IdentitySummary {
            id: im_core::ids::IdentityId::parse("id-alice").expect("identity id"),
            did: im_core::ids::Did::parse("did:example:alice").expect("did"),
            handle: None,
            display_name: Some("Alice".to_string()),
            local_alias: Some("alice".to_string()),
            device_id: Some("device-a".to_string()),
            is_default: true,
            readiness: im_core::identity::IdentityReadiness {
                ready_for_auth: true,
                ready_for_messaging: true,
                missing: vec![],
            },
        },
        storage_policy: im_core::IdentitySecretStoragePolicy::VaultPreferred,
        selected_backend: im_core::identity::IdentitySecretStorageBackend::Vault,
        vault_available: true,
        vault_metadata_present: true,
        vault_metadata_verified: true,
        workspace_id: Some("workspace-a".to_string()),
        device_id: Some("device-a".to_string()),
        plaintext_compat_retained: Some(false),
        missing: vec![],
        warnings: vec![],
    };

    let dart: awiki_im_core::dto::identity::DartIdentityVaultStatus = core_status.into();
    assert_eq!(dart.identity.did, "did:example:alice");
    assert!(matches!(
        dart.storage_policy,
        awiki_im_core::dto::config::DartIdentitySecretStoragePolicy::VaultPreferred
    ));
    assert!(matches!(
        dart.selected_backend,
        awiki_im_core::dto::identity::DartIdentitySecretStorageBackend::Vault
    ));
    assert!(dart.vault_available);
    assert!(dart.vault_metadata_present);
    assert!(dart.vault_metadata_verified);
    assert_eq!(dart.workspace_id.as_deref(), Some("workspace-a"));
    assert_eq!(dart.device_id.as_deref(), Some("device-a"));
    assert_eq!(dart.plaintext_compat_retained, Some(false));
}

#[test]
fn identity_vault_reports_map_without_secret_refs() {
    let core_status = im_core::identity::IdentityVaultStatus {
        identity: im_core::identity::IdentitySummary {
            id: im_core::ids::IdentityId::parse("id-alice").expect("identity id"),
            did: im_core::ids::Did::parse("did:example:alice").expect("did"),
            handle: None,
            display_name: Some("Alice".to_string()),
            local_alias: Some("alice".to_string()),
            device_id: Some("device-a".to_string()),
            is_default: true,
            readiness: im_core::identity::IdentityReadiness {
                ready_for_auth: true,
                ready_for_messaging: true,
                missing: vec![],
            },
        },
        storage_policy: im_core::IdentitySecretStoragePolicy::VaultRequired,
        selected_backend: im_core::identity::IdentitySecretStorageBackend::Vault,
        vault_available: true,
        vault_metadata_present: true,
        vault_metadata_verified: true,
        workspace_id: Some("workspace-a".to_string()),
        device_id: Some("device-a".to_string()),
        plaintext_compat_retained: Some(true),
        missing: vec![],
        warnings: vec!["identity plaintext compatibility files are still retained".to_string()],
    };
    let migration = im_core::identity::IdentityVaultMigrationReport {
        identity: core_status.identity.clone(),
        status: core_status.clone(),
        migrated: true,
        verified: true,
        plaintext_compat_retained: true,
        warnings: core_status.warnings.clone(),
    };
    let verification = im_core::identity::IdentityVaultVerificationReport {
        identity: core_status.identity.clone(),
        status: core_status,
        verified: true,
        warnings: vec!["identity plaintext compatibility files are still retained".to_string()],
    };

    let dart_migration: awiki_im_core::dto::identity::DartIdentityVaultMigrationReport =
        migration.into();
    let dart_verification: awiki_im_core::dto::identity::DartIdentityVaultVerificationReport =
        verification.into();

    assert!(dart_migration.migrated);
    assert!(dart_migration.verified);
    assert!(dart_migration.plaintext_compat_retained);
    assert_eq!(dart_migration.identity.did, "did:example:alice");
    assert!(dart_migration.warnings[0].contains("plaintext compatibility"));
    assert!(dart_verification.verified);
    assert_eq!(
        dart_verification.status.workspace_id.as_deref(),
        Some("workspace-a")
    );
    let debug = format!("{dart_migration:?} {dart_verification:?}");
    assert!(!debug.contains("SecretRef"));
    assert!(!debug.contains("private"));
}

#[test]
fn email_send_bridge_request_maps_to_typed_core_addresses() {
    let request = awiki_im_core::dto::email::DartSendEmailRequest {
        to: vec!["bob@awiki.ai".to_string()],
        cc: vec!["copy@awiki.ai".to_string()],
        subject: "Hello".to_string(),
        body_text: "Body".to_string(),
        body_html: Some("<p>Body</p>".to_string()),
    };

    let core: im_core::email::SendEmailRequest =
        request.try_into().expect("email request maps to im-core");
    assert_eq!(core.to[0].as_str(), "bob@awiki.ai");
    assert_eq!(core.cc[0].as_str(), "copy@awiki.ai");
    assert_eq!(core.body_html.as_deref(), Some("<p>Body</p>"));
}

#[test]
fn realtime_runner_capability_is_exposed_after_bridge_plan_lands() {
    let capability = awiki_im_core::dto::realtime::DartRealtimeCapability {
        status_supported: true,
        connect_supported: true,
        runner_exposed: true,
        reason: None,
    };
    assert!(capability.connect_supported);
    assert!(capability.runner_exposed);
    assert!(capability.reason.is_none());
}

#[test]
fn group_create_bridge_request_no_longer_accepts_per_request_service_did() {
    let request = awiki_im_core::dto::group::DartCreateGroupRequest {
        name: "test".to_string(),
        identity_mode: awiki_im_core::dto::group::DartGroupIdentityMode::DidOnly,
        identity_handle: None,
        description: None,
        avatar_uri: None,
        discoverability: None,
        admission_mode: None,
        message_security_profile: None,
        e2ee: false,
        slug: None,
        goal: None,
        rules: None,
        message_prompt: None,
        doc_url: None,
        attachments_allowed: None,
        max_members: None,
        member_max_messages: None,
        member_max_total_chars: None,
    };
    let core = request
        .into_core()
        .expect("service DID is resolved by ImCoreConfig at create time");
    assert_eq!(core.name, "test");
    assert!(core.discoverability.is_none());
}

#[test]
fn group_create_bridge_preserves_explicit_handle_mode_without_fallback() {
    use awiki_im_core::dto::group::{DartCreateGroupRequest, DartGroupIdentityMode};

    let request = DartCreateGroupRequest {
        name: "handle group".to_owned(),
        identity_mode: DartGroupIdentityMode::Handle,
        identity_handle: Some("alice.example.com".to_owned()),
        description: None,
        avatar_uri: None,
        discoverability: None,
        admission_mode: None,
        message_security_profile: None,
        e2ee: false,
        slug: None,
        goal: None,
        rules: None,
        message_prompt: None,
        doc_url: None,
        attachments_allowed: None,
        max_members: None,
        member_max_messages: None,
        member_max_total_chars: None,
    };
    let core = request
        .clone()
        .into_core()
        .expect("Handle mode maps to creator_handle");
    assert_eq!(
        core.creator_handle
            .as_ref()
            .map(im_core::ids::Handle::as_str),
        Some("alice.example.com")
    );

    let mut invalid = request.clone();
    invalid.identity_mode = DartGroupIdentityMode::DidOnly;
    assert!(invalid.into_core().is_err());
    let mut missing = request;
    missing.identity_handle = None;
    assert!(missing.into_core().is_err());
}
