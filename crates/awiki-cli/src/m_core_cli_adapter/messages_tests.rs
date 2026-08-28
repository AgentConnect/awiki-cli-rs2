use super::*;
use im_core::prelude::{Message, MessageMetadata, ThreadId};
use serde_json::json;

#[test]
fn foreground_message_reads_use_standard_reconcile_reason() {
    assert_eq!(foreground_message_sync_reason(), "foreground_reconcile");
}

#[test]
fn secure_lane_drain_failure_is_a_closed_cli_warning() {
    assert_eq!(
        secure_lane_drain_warning(&im_core::ImError::LocalStateUnavailable {
            detail: "secure lane consumer drain timed out with durable domain work still pending"
                .to_owned(),
        }),
        "sync.secure_lane_drain_pending"
    );
    assert_eq!(
        secure_lane_drain_warning(&im_core::ImError::LocalStateUnavailable {
            detail: "local database unavailable".to_owned(),
        }),
        "sync.secure_lane_drain_failed"
    );
}

#[test]
fn cli_message_read_state_is_boolean_and_drives_unread_filter() {
    let read = thread_scoped_send_result(&[("is_read", "true")]).message;
    let unread = thread_scoped_send_result(&[("is_read", "false")]).message;
    let read_json = message_to_cli_json(&read);
    let unread_json = message_to_cli_json(&unread);

    assert_eq!(read_json["is_read"], Value::Bool(true));
    assert_eq!(unread_json["is_read"], Value::Bool(false));
    assert_eq!(
        apply_inbox_filters(vec![read_json, unread_json.clone()], "", true, 20),
        vec![unread_json]
    );
}

#[test]
fn foreground_message_reads_projection_only_after_terminal_sync_success() {
    for status in [MessageSyncStatus::Idle, MessageSyncStatus::Changed] {
        assert!(require_foreground_message_sync(&sync_outcome(status)).is_ok());
    }
    assert!(matches!(
        require_foreground_message_sync(&sync_outcome(MessageSyncStatus::RecoveryRequired)),
        Err(MessageAdapterError::LocalStateUnavailable(_))
    ));
    assert!(matches!(
        require_foreground_message_sync(&sync_outcome(MessageSyncStatus::RetryableFailure)),
        Err(MessageAdapterError::TransportUnavailable(_))
    ));
    assert!(matches!(
        require_foreground_message_sync(&sync_outcome(MessageSyncStatus::Blocked)),
        Err(MessageAdapterError::LocalStateUnavailable(_))
    ));
    assert!(matches!(
        require_foreground_message_sync(&sync_outcome(MessageSyncStatus::AuthRevoked)),
        Err(MessageAdapterError::IdentityRequired(_))
    ));
}

fn sync_outcome(status: MessageSyncStatus) -> MessageSyncOutcome {
    MessageSyncOutcome {
        status,
        events_applied: 0,
        pages_fetched: 0,
        messages_hydrated: 0,
        duplicates_skipped: 0,
        changed_conversation_ids: Vec::new(),
        committed_incoming_messages: Vec::new(),
        error_code: None,
        warnings: Vec::new(),
    }
}

#[test]
fn attachment_transport_warnings_match_legacy_websocket_contract() {
    assert_eq!(
        attachment_transport_warnings_for_mode("websocket", false),
        vec!["Attachment messages use HTTP transport even when runtime.mode is websocket."]
    );
    assert_eq!(
        attachment_transport_warnings_for_mode("websocket", true),
        vec!["Attachment downloads use HTTP transport even when runtime.mode is websocket."]
    );
    assert!(attachment_transport_warnings_for_mode("http", false).is_empty());
}

#[test]
fn direct_attachment_download_target_uses_resolved_did() {
    let thread = ThreadRef::Direct(PeerRef::parse("bob", "").expect("peer"));
    let selection = AttachmentSelection {
        sender_did: "did:wba:example:bob".to_string(),
        ..AttachmentSelection::default()
    };

    assert_eq!(
        download_target_value(&thread, Some(&selection)),
        json!({"kind": "direct", "did": "did:wba:example:bob"})
    );
}

#[test]
fn direct_thread_send_target_uses_metadata_identity() {
    let result = thread_scoped_send_result(&[
        ("peer_full_handle", "bob.awiki.ai"),
        ("resolved_target_did", "did:wba:awiki.ai:bob:e1"),
    ]);

    let target = direct_target_from_result(&result);

    assert_eq!(target.handle, "bob.awiki.ai");
    assert_eq!(target.did, "did:wba:awiki.ai:bob:e1");
}

#[test]
fn direct_thread_send_result_renders_as_direct_delivery() {
    let result = thread_scoped_send_result(&[
        ("target_handle", "bob.awiki.ai"),
        ("resolved_target_did", "did:wba:awiki.ai:bob:e1"),
    ]);
    let target = direct_target_from_result(&result);

    let rendered = render_send_result(&target, &result, true).expect("render");

    assert_eq!(rendered.data["action"], "send_message");
    assert_eq!(rendered.data["target"]["kind"], "direct");
    assert_eq!(rendered.data["target"]["handle"], "bob.awiki.ai");
    assert_eq!(rendered.data["target"]["did"], "did:wba:awiki.ai:bob:e1");
    assert_eq!(rendered.data["message"]["secure"], true);
    assert_eq!(rendered.data["delivery"]["accepted"], true);
    assert_eq!(
        rendered.data["delivery"]["target_did"],
        "did:wba:awiki.ai:bob:e1"
    );
}

#[test]
fn direct_delivery_preserves_final_acceptance_from_core_metadata() {
    let result = thread_scoped_send_result(&[
        ("resolved_target_did", "did:wba:awiki.ai:bob:e1"),
        ("final_acceptance", "true"),
    ]);
    let target = direct_target_from_result(&result);

    let rendered = render_send_result(&target, &result, false).expect("render");

    assert_eq!(rendered.data["delivery"]["accepted"], true);
    assert_eq!(rendered.data["delivery"]["delivery_state"], "accepted");
    assert_eq!(rendered.data["delivery"]["final_acceptance"], true);
}

#[test]
fn direct_partial_delivery_preserves_non_final_acceptance_from_core_metadata() {
    let mut result = thread_scoped_send_result(&[
        ("resolved_target_did", "did:wba:awiki.ai:bob:e1"),
        ("final_acceptance", "false"),
    ]);
    result.delivery = DeliveryState::Sent;
    result.message.metadata.delivery_state = Some("sent".to_owned());
    let target = direct_target_from_result(&result);

    let rendered = render_send_result(&target, &result, false).expect("render");

    assert_eq!(rendered.data["delivery"]["accepted"], true);
    assert_eq!(rendered.data["delivery"]["delivery_state"], "sent");
    assert_eq!(rendered.data["delivery"]["final_acceptance"], false);
}

#[test]
fn direct_delivery_projects_internal_partial_retry_counts() {
    let result = thread_scoped_send_result(&[
        ("resolved_target_did", "did:wba:awiki.ai:bob:e1"),
        ("attempted_device_count", "1"),
        ("target_device_count", "1"),
        ("own_sync_device_count", "1"),
        ("previously_accepted_device_count", "1"),
        ("newly_accepted_device_count", "1"),
        ("accepted_device_count", "2"),
        ("failed_device_count", "0"),
    ]);
    let target = direct_target_from_result(&result);

    let rendered = render_send_result(&target, &result, true).expect("render");
    let delivery = &rendered.data["delivery"];

    assert_eq!(delivery["attempted_device_count"], 1);
    assert_eq!(delivery["target_device_count"], 1);
    assert_eq!(delivery["own_sync_device_count"], 1);
    assert_eq!(delivery["previously_accepted_device_count"], 1);
    assert_eq!(delivery["newly_accepted_device_count"], 1);
    assert_eq!(delivery["accepted_device_count"], 2);
    assert_eq!(delivery["failed_device_count"], 0);
}

#[test]
fn group_delivery_preserves_final_acceptance_from_core_metadata() {
    let result = thread_scoped_send_result(&[
        ("raw_message_id", "msg-group"),
        ("final_acceptance", "true"),
    ]);

    let mapped = GroupSendResult::from_sdk_result(&result, "did:example:group");

    assert!(mapped.accepted);
    assert!(mapped.final_acceptance);
    assert_eq!(mapped.delivery_state, "accepted");
}

#[test]
fn direct_thread_attachment_target_falls_back_to_attachment_target_did() {
    let message = thread_scoped_send_result(&[("target_handle", "bob.awiki.ai")]);
    let result = AttachmentSendResult {
        message,
        target_kind: "direct".to_string(),
        target_did: "did:wba:awiki.ai:bob:e1".to_string(),
        attachment: uploaded_attachment(),
        manifest: json!({"caption": "hello"}),
    };

    let target = direct_target_from_attachment_result(&result);

    assert_eq!(target.handle, "bob.awiki.ai");
    assert_eq!(target.did, "did:wba:awiki.ai:bob:e1");
}

#[test]
fn attachment_output_preparation_errors_map_to_cli_path_errors() {
    let root = unique_temp_root("attachment-output-path-error");
    std::fs::create_dir_all(&root).unwrap();
    let parent_file = root.join("not-a-directory");
    std::fs::write(&parent_file, b"file").unwrap();
    let request = DownloadAttachmentRequest {
        thread: ThreadRef::Direct(PeerRef::parse("did:example:bob", "").unwrap()),
        message_id: MessageId::parse("msg-1").unwrap(),
        attachment_id: Some("att-1".to_string()),
        destination: AttachmentDestination::LocalFile(parent_file.join("out.bin")),
        overwrite: true,
    };

    let err = prepare_download_destination(&request).unwrap_err();

    assert!(matches!(err, MessageAdapterError::PathUnavailable(message)
        if message.contains("create attachment output directory")
            && message.contains("not-a-directory")));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn attachment_destination_errors_map_to_cli_path_errors() {
    assert_eq!(
        im_error_to_message_error(im_core::ImError::invalid_input(
            Some("destination".to_string()),
            "destination already exists and overwrite is false: out.bin",
        )),
        MessageAdapterError::PathUnavailable(
            "destination already exists and overwrite is false: out.bin".to_string()
        )
    );
    assert_eq!(
        im_error_to_message_error(im_core::ImError::PathUnavailable {
            path_kind: "attachment_output".to_string(),
            detail: "parent is not writable".to_string(),
        }),
        MessageAdapterError::PathUnavailable(
            "attachment_output path unavailable: parent is not writable".to_string()
        )
    );
    assert_eq!(
        im_error_to_message_error(im_core::ImError::Io {
            detail: "write temp file failed".to_string(),
        }),
        MessageAdapterError::PathUnavailable("write temp file failed".to_string())
    );
}

#[test]
fn message_service_public_code_does_not_degrade_to_internal_error() {
    let private_marker = "remote-private-message-data";
    for service_code in ["anp.device_not_eligible", "anp.device_state_changed"] {
        let mapped = im_error_to_message_error(im_core::ImError::Service {
            status_code: None,
            code: Some(service_code.to_owned()),
            message: private_marker.to_owned(),
            data: Some(json!({"private": private_marker})),
        });

        assert_eq!(
            mapped,
            MessageAdapterError::PublicServiceCode(service_code.to_owned())
        );
        assert!(!format!("{mapped:?}").contains(private_marker));
        assert!(!mapped.to_string().contains(private_marker));
    }
}

#[test]
fn message_service_auth_status_precedes_public_service_code() {
    let private_marker = "remote-private-auth-error";
    for status_code in [401, 403] {
        let mapped = im_error_to_message_error(im_core::ImError::Service {
            status_code: Some(status_code),
            code: Some("anp.device_not_eligible".to_owned()),
            message: private_marker.to_owned(),
            data: Some(json!({"private": private_marker})),
        });

        assert_eq!(
            mapped,
            MessageAdapterError::Service(ServiceError {
                status_code,
                rpc_code: 0,
                message: "remote service request failed".to_owned(),
                data: None,
            })
        );
        assert!(!format!("{mapped:?}").contains(private_marker));
        assert!(!mapped.to_string().contains(private_marker));
    }
}

#[test]
fn message_service_private_code_and_payload_are_not_preserved() {
    let private_marker = "remote-private-error-payload";
    let mapped = im_error_to_message_error(im_core::ImError::Service {
        status_code: None,
        code: Some("private diagnostic code".to_owned()),
        message: private_marker.to_owned(),
        data: Some(json!({"private": private_marker})),
    });

    assert_eq!(
        mapped,
        MessageAdapterError::Service(ServiceError {
            status_code: 0,
            rpc_code: 0,
            message: "remote service request failed".to_owned(),
            data: None,
        })
    );
    assert!(!format!("{mapped:?}").contains(private_marker));
    assert!(!mapped.to_string().contains(private_marker));
}

fn unique_temp_root(name: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("awiki-cli-{name}-{}-{nanos}", std::process::id()))
}

fn thread_scoped_send_result(attributes: &[(&str, &str)]) -> SendMessageResult {
    SendMessageResult {
        message: Message {
            id: MessageId::parse("msg-thread-direct").unwrap(),
            thread: ThreadRef::Thread(ThreadId::parse("direct-peer-scope-thread").unwrap()),
            direction: MessageDirection::Outgoing,
            sender: PeerRef::parse("did:wba:awiki.ai:alice:e1", "").unwrap(),
            receiver: None,
            group: None,
            body: MessageBodyView::Text {
                text: "hello".to_string(),
                kind: MessageKind::Text,
            },
            sent_at: Some("2026-07-04T04:00:00Z".to_string()),
            received_at: None,
            metadata: MessageMetadata {
                operation_id: Some("op-thread-direct".to_string()),
                attributes: attributes
                    .iter()
                    .map(|(key, value)| MessageMetadataAttribute {
                        key: (*key).to_string(),
                        value: (*value).to_string(),
                    })
                    .collect(),
                ..MessageMetadata::default()
            },
        },
        delivery: DeliveryState::Accepted,
        warnings: Vec::new(),
    }
}

fn uploaded_attachment() -> UploadedAttachment {
    UploadedAttachment {
        attachment_id: "att-1".to_string(),
        filename: "hello.txt".to_string(),
        mime_type: "text/plain".to_string(),
        size_bytes: 5,
        size: "5 B".to_string(),
        digest_b64u: "digest".to_string(),
        object_uri: "awiki://attachments/att-1".to_string(),
        object_encryption_mode: "none".to_string(),
        plaintext_size_bytes: Some(5),
    }
}

#[test]
fn secure_attachment_unsupported_maps_to_specific_adapter_error() {
    let err = im_error_to_message_error(im_core::ImError::UnsupportedCapability {
        capability: "secure-attachments".to_string(),
    });

    assert_eq!(err, MessageAdapterError::SecureAttachmentNotSupported);
}
