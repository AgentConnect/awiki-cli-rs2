use super::*;
use im_core::prelude::{Message, MessageMetadata, ThreadId};
use serde_json::json;

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
fn direct_delivery_projects_internal_partial_retry_counts() {
    let result = thread_scoped_send_result(&[
        ("resolved_target_did", "did:wba:awiki.ai:bob:e1"),
        ("attempted_device_count", "1"),
        ("previously_accepted_device_count", "1"),
        ("newly_accepted_device_count", "1"),
        ("accepted_device_count", "2"),
        ("failed_device_count", "0"),
    ]);
    let target = direct_target_from_result(&result);

    let rendered = render_send_result(&target, &result, true).expect("render");
    let delivery = &rendered.data["delivery"];

    assert_eq!(delivery["attempted_device_count"], 1);
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
