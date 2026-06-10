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
    assert!(request.delivery.wait_for_final_acceptance);
    assert!(matches!(
        request.security,
        im_core::messages::MessageSecurityMode::E2eeRequired
    ));
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
