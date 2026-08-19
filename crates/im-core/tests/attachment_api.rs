use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use awiki_im_core::messages::direct_peer_scope_thread_id;
use awiki_im_core::prelude::*;
use serde_json::json;
use serde_json::Value;

#[test]
fn attachments_public_api_accepts_canonical_inputs() {
    let local_file = AttachmentInput::LocalFile(PathBuf::from("image.png"));
    assert!(
        matches!(local_file, AttachmentInput::LocalFile(path) if path == PathBuf::from("image.png"))
    );

    let bytes = AttachmentInput::Bytes {
        filename: Some("note.txt".to_string()),
        mime_type: Some("text/plain".to_string()),
        bytes: b"hello".to_vec(),
    };
    assert!(matches!(
        bytes,
        AttachmentInput::Bytes {
            filename: Some(filename),
            mime_type: Some(mime_type),
            bytes
        } if filename == "note.txt" && mime_type == "text/plain" && bytes == b"hello"
    ));

    let destination = AttachmentDestination::Memory;
    assert!(matches!(destination, AttachmentDestination::Memory));
}

#[test]
fn attachments_input_is_the_canonical_message_body_input() {
    let input = AttachmentInput::Bytes {
        filename: Some("note.txt".to_string()),
        mime_type: Some("text/plain".to_string()),
        bytes: b"hello attachment".to_vec(),
    };

    let body = MessageBody::Attachment {
        input: input.clone(),
        caption: Some("caption".to_string()),
        mention_payload: None,
        mime_type: Some("text/plain".to_string()),
        filename: None,
    };

    assert!(matches!(
        body,
        MessageBody::Attachment {
            input: AttachmentInput::Bytes { bytes, .. },
            ..
        } if bytes == b"hello attachment".to_vec()
    ));
}

#[test]
fn conversation_attachment_request_is_conversation_first() {
    let conversation_id = direct_peer_scope_thread_id("user-bob", "bob.awiki.info")
        .unwrap()
        .as_str()
        .to_owned();
    let request = SendConversationAttachmentRequest {
        conversation: ConversationReadRef::new(conversation_id.clone()).unwrap(),
        input: AttachmentInput::Bytes {
            filename: Some("note.txt".to_string()),
            mime_type: Some("text/plain".to_string()),
            bytes: b"hello attachment".to_vec(),
        },
        caption: Some("caption".to_string()),
        mention_payload: None,
        mime_type: Some("text/plain".to_string()),
        filename: None,
        security: MessageSecurityMode::DefaultPlain,
        client_message_id: Some(MessageId::parse("msg-client-attachment").unwrap()),
        idempotency_key: Some("op-client-attachment".to_string()),
        wait_for_final_acceptance: true,
    };

    assert_eq!(request.conversation.conversation_id, conversation_id);
    assert_eq!(
        request.client_message_id.as_ref().map(MessageId::as_str),
        Some("msg-client-attachment")
    );
    assert_eq!(
        request.idempotency_key.as_deref(),
        Some("op-client-attachment")
    );
    assert!(request.wait_for_final_acceptance);
}

#[tokio::test]
async fn attachments_service_send_conversation_direct_uses_canonical_projection() {
    let server = AttachmentServiceTestServer::spawn(vec![
        ExpectedHttp::rpc_result(handle_lookup_result()),
        ExpectedHttp::rpc_result(json!({
            "attachment_id": "att-conv-direct",
            "slot_id": "slot-conv-direct",
            "upload_uri": "__BASE__/objects/slot-conv-direct",
            "upload_headers": {},
            "object_uri": "__BASE__/objects/att-conv-direct",
            "commit_token": "commit-token-conv-direct",
            "expires_at": "2026-05-23T01:00:00Z"
        })),
        ExpectedHttp::json(json!({})),
        ExpectedHttp::rpc_result(json!({
            "committed": true,
            "attachment_id": "att-conv-direct",
            "object_uri": "__BASE__/objects/att-conv-direct",
            "committed_at": "2026-05-23T00:00:01Z"
        })),
        ExpectedHttp::rpc_result(json!({
            "accepted": true,
            "message_id": "msg-conv-attachment-direct",
            "operation_id": "retry-msg-conv-attachment-direct",
            "target_did": "did:example:bob",
            "accepted_at": "2026-05-23T00:00:02Z",
            "delivery_state": "accepted"
        })),
    ]);
    let (core, paths) = test_core_with_base_url_ready_identity_and_service_did(
        server.base_url(),
        "did:example:message-service",
    );
    let client = core
        .client(IdentitySelector::LocalAlias("alice".to_string()))
        .unwrap();
    let lookup = client
        .directory()
        .lookup_handle_async(Handle::parse("bob.awiki.info", "").unwrap())
        .await
        .expect("verified Handle lookup should establish the canonical Direct route");
    let conversation_id = lookup.direct_conversation_id();

    let result = client
        .attachments()
        .send_conversation_async(SendConversationAttachmentRequest {
            conversation: ConversationReadRef::new(conversation_id.clone()).unwrap(),
            input: AttachmentInput::Bytes {
                filename: Some("conversation.txt".to_string()),
                mime_type: Some("text/plain".to_string()),
                bytes: b"conversation attachment".to_vec(),
            },
            caption: Some("conversation caption".to_string()),
            mention_payload: None,
            mime_type: None,
            filename: None,
            security: MessageSecurityMode::DefaultPlain,
            client_message_id: Some(MessageId::parse("msg-conv-attachment-direct").unwrap()),
            idempotency_key: Some("retry-msg-conv-attachment-direct".to_string()),
            wait_for_final_acceptance: true,
        })
        .await
        .expect("conversation attachment send should run upload flow");

    assert_eq!(
        result.message.message.id.as_str(),
        "msg-conv-attachment-direct"
    );
    assert_eq!(
        result
            .message
            .message
            .metadata
            .conversation_identity
            .as_ref()
            .map(|identity| identity.conversation_id.as_str()),
        Some(conversation_id.as_str())
    );
    assert_eq!(result.target_kind, "agent");
    assert_eq!(result.target_did, "did:example:bob");
    let attachment_id = result.attachment.attachment_id.clone();
    assert!(attachment_id.starts_with("att-"));

    let rows = local_message_rows(
        &paths,
        "SELECT msg_id, conversation_id, thread_id, receiver_did, content_type, content, metadata FROM messages WHERE msg_id = 'msg-conv-attachment-direct'",
    );
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["conversation_id"], conversation_id);
    assert_eq!(rows[0]["thread_id"], conversation_id);
    assert_eq!(rows[0]["receiver_did"], "did:example:bob");
    assert_eq!(
        rows[0]["content_type"],
        awiki_im_core::attachments::attachment_manifest_content_type()
    );
    let stored_manifest: Value =
        serde_json::from_str(rows[0]["content"].as_str().unwrap()).unwrap();
    assert_eq!(stored_manifest["primary_attachment_id"], attachment_id);
    assert_eq!(stored_manifest["caption"], "conversation caption");
    let metadata: Value = serde_json::from_str(rows[0]["metadata"].as_str().unwrap()).unwrap();
    assert_eq!(metadata["operation_id"], "retry-msg-conv-attachment-direct");
    assert_eq!(metadata["attachment_id"], attachment_id);

    let requests = server.join();
    assert_eq!(requests.len(), 5);
    assert_eq!(requests[0].rpc_method().as_deref(), Some("lookup"));
    assert_eq!(
        requests[1].rpc_method().as_deref(),
        Some("attachment.create_slot")
    );
    assert_eq!(
        requests[1].params()["body"]["intended_target"],
        json!({ "kind": "agent", "did": "did:example:bob" })
    );
    assert_eq!(requests[4].rpc_method().as_deref(), Some("direct.send"));
    assert_eq!(
        requests[4].params()["meta"]["target"],
        json!({ "kind": "agent", "did": "did:example:bob" })
    );
    assert_eq!(
        requests[4].params()["meta"]["message_id"],
        "msg-conv-attachment-direct"
    );
    assert_eq!(
        requests[4].params()["meta"]["operation_id"],
        "retry-msg-conv-attachment-direct"
    );
}

#[tokio::test]
async fn conversation_attachment_rebind_reuses_one_committed_object_and_logical_message() {
    let server = AttachmentServiceTestServer::spawn(vec![
        ExpectedHttp::rpc_result(rebind_directory_lookup(REBIND_OLD_DID)),
        ExpectedHttp::rpc_result(json!({
            "attachment_id": "att-rebind",
            "slot_id": "slot-rebind",
            "upload_uri": "__BASE__/objects/slot-rebind",
            "upload_headers": {},
            "object_uri": "__BASE__/objects/att-rebind",
            "commit_token": "commit-token-rebind",
            "expires_at": "2026-08-19T01:00:00Z"
        })),
        ExpectedHttp::json(json!({})),
        ExpectedHttp::rpc_result(json!({
            "committed": true,
            "attachment_id": "att-rebind",
            "object_uri": "__BASE__/objects/att-rebind",
            "committed_at": "2026-08-19T00:00:01Z"
        })),
        ExpectedHttp::stale_binding_error(),
        ExpectedHttp::rpc_result(rebind_directory_lookup(REBIND_NEW_DID)),
        ExpectedHttp::json(rebind_public_binding(REBIND_NEW_DID, "2")),
        ExpectedHttp::rpc_result(json!({
            "accepted": true,
            "message_id": "msg-conv-attachment-rebind",
            "operation_id": "op-conv-attachment-rebind",
            "target_did": REBIND_NEW_DID,
            "accepted_at": "2026-08-19T00:00:02Z",
            "delivery_state": "accepted"
        })),
    ]);
    let (core, paths) = test_core_with_base_url_ready_identity_and_service_did(
        server.base_url(),
        "did:example:message-service",
    );
    let client = core
        .client(IdentitySelector::LocalAlias("alice".to_string()))
        .unwrap();
    let lookup = client
        .directory()
        .lookup_handle_async(Handle::parse(REBIND_HANDLE, "").unwrap())
        .await
        .unwrap();
    let conversation_id = lookup.direct_conversation_id();
    let before = direct_binding_snapshot(&paths, &conversation_id);

    let result = client
        .attachments()
        .send_conversation_async(SendConversationAttachmentRequest {
            conversation: ConversationReadRef::new(&conversation_id).unwrap(),
            input: AttachmentInput::Bytes {
                filename: Some("rebind.txt".to_string()),
                mime_type: Some("text/plain".to_string()),
                bytes: b"one uploaded attachment".to_vec(),
            },
            caption: Some("one logical attachment".to_string()),
            mention_payload: None,
            mime_type: None,
            filename: None,
            security: MessageSecurityMode::DefaultPlain,
            client_message_id: Some(MessageId::parse("msg-conv-attachment-rebind").unwrap()),
            idempotency_key: Some("op-conv-attachment-rebind".to_string()),
            wait_for_final_acceptance: true,
        })
        .await
        .unwrap();

    assert_eq!(
        result.message.message.id.as_str(),
        "msg-conv-attachment-rebind"
    );
    assert_eq!(result.target_did, REBIND_NEW_DID);
    assert_eq!(
        result
            .message
            .message
            .metadata
            .conversation_identity
            .as_ref()
            .map(|identity| identity.conversation_id.as_str()),
        Some(conversation_id.as_str())
    );
    let after = direct_binding_snapshot(&paths, &conversation_id);
    assert_eq!(after.peer_persona_id, before.peer_persona_id);
    assert_eq!(after.conversation_id, before.conversation_id);
    assert_eq!(after.full_handle, before.full_handle);
    assert_eq!(after.current_did, REBIND_NEW_DID);
    assert_eq!(after.binding_generation.as_deref(), Some("2"));

    let rows = local_message_rows(
        &paths,
        "SELECT msg_id, conversation_id, receiver_did, content, metadata FROM messages WHERE msg_id = 'msg-conv-attachment-rebind'",
    );
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["conversation_id"], conversation_id);
    assert_eq!(rows[0]["receiver_did"], REBIND_NEW_DID);
    let stored_manifest: Value =
        serde_json::from_str(rows[0]["content"].as_str().unwrap()).unwrap();
    assert_eq!(stored_manifest, result.manifest);
    let metadata: Value = serde_json::from_str(rows[0]["metadata"].as_str().unwrap()).unwrap();
    assert_eq!(metadata["operation_id"], "op-conv-attachment-rebind");

    let requests = server.join();
    assert_eq!(requests.len(), 8);
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.rpc_method().as_deref() == Some("attachment.create_slot"))
            .count(),
        1
    );
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.method == "PUT")
            .count(),
        1
    );
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.rpc_method().as_deref() == Some("attachment.commit_object"))
            .count(),
        1
    );
    assert_eq!(requests[6].method, "GET");
    assert_eq!(requests[6].path, "/.well-known/handle/bob");
    assert_eq!(requests[4].rpc_method().as_deref(), Some("direct.send"));
    assert_eq!(requests[7].rpc_method().as_deref(), Some("direct.send"));
    assert_eq!(
        requests[4].params()["meta"]["target"]["did"],
        REBIND_OLD_DID
    );
    assert_eq!(
        requests[7].params()["meta"]["target"]["did"],
        REBIND_NEW_DID
    );
    for request in [&requests[4], &requests[7]] {
        assert_eq!(
            request.params()["meta"]["message_id"],
            "msg-conv-attachment-rebind"
        );
        assert_eq!(
            request.params()["meta"]["operation_id"],
            "op-conv-attachment-rebind"
        );
    }
    assert_eq!(requests[4].params()["body"], requests[7].params()["body"]);
}

#[tokio::test]
async fn group_attachment_stale_shaped_error_never_enters_direct_rebind() {
    let server = AttachmentServiceTestServer::spawn(vec![
        ExpectedHttp::rpc_result(json!({
            "attachment_id": "att-group-stale",
            "slot_id": "slot-group-stale",
            "upload_uri": "__BASE__/objects/slot-group-stale",
            "upload_headers": {},
            "object_uri": "__BASE__/objects/att-group-stale",
            "commit_token": "commit-token-group-stale",
            "expires_at": "2026-08-19T01:00:00Z"
        })),
        ExpectedHttp::json(json!({})),
        ExpectedHttp::rpc_result(json!({
            "committed": true,
            "attachment_id": "att-group-stale",
            "object_uri": "__BASE__/objects/att-group-stale",
            "committed_at": "2026-08-19T00:00:01Z"
        })),
        ExpectedHttp::stale_binding_error(),
    ]);
    let (core, paths) = test_core_with_base_url_ready_identity_and_service_did(
        server.base_url(),
        "did:example:message-service",
    );
    let client = core
        .client(IdentitySelector::LocalAlias("alice".to_string()))
        .unwrap();
    seed_active_group_conversation(&paths, "did:example:group-stale");

    let error = client
        .attachments()
        .send_conversation_async(SendConversationAttachmentRequest {
            conversation: ConversationReadRef::new("group:did:example:group-stale").unwrap(),
            input: AttachmentInput::Bytes {
                filename: Some("group-stale.txt".to_string()),
                mime_type: Some("text/plain".to_string()),
                bytes: b"group".to_vec(),
            },
            caption: None,
            mention_payload: None,
            mime_type: None,
            filename: None,
            security: MessageSecurityMode::DefaultPlain,
            client_message_id: Some(MessageId::parse("msg-group-stale").unwrap()),
            idempotency_key: Some("op-group-stale".to_string()),
            wait_for_final_acceptance: true,
        })
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        ImError::Service { code: Some(code), .. } if code == "anp.invalid_target_binding"
    ));
    let requests = server.join();
    assert_eq!(requests.len(), 4);
    assert_eq!(requests[3].rpc_method().as_deref(), Some("group.send"));
    assert!(requests.iter().all(|request| request.method != "GET"));
    assert!(requests
        .iter()
        .all(|request| request.rpc_method().as_deref() != Some("lookup")));
}

#[tokio::test]
async fn attachments_service_send_conversation_group_uses_group_route() {
    let server = AttachmentServiceTestServer::spawn(vec![
        ExpectedHttp::rpc_result(json!({
            "attachment_id": "att-conv-group",
            "slot_id": "slot-conv-group",
            "upload_uri": "__BASE__/objects/slot-conv-group",
            "upload_headers": {},
            "object_uri": "__BASE__/objects/att-conv-group",
            "commit_token": "commit-token-conv-group",
            "expires_at": "2026-05-23T01:00:00Z"
        })),
        ExpectedHttp::json(json!({})),
        ExpectedHttp::rpc_result(json!({
            "committed": true,
            "attachment_id": "att-conv-group",
            "object_uri": "__BASE__/objects/att-conv-group",
            "committed_at": "2026-05-23T00:00:01Z"
        })),
        ExpectedHttp::rpc_result(json!({
            "accepted": true,
            "message_id": "msg-conv-attachment-group",
            "operation_id": "op-msg-conv-attachment-group",
            "group_id": "did:example:group",
            "group_did": "did:example:group",
            "accepted_at": "2026-05-23T00:00:02Z",
            "delivery_state": "accepted"
        })),
    ]);
    let (core, paths) = test_core_with_base_url_ready_identity_and_service_did(
        server.base_url(),
        "did:example:message-service",
    );
    let client = core
        .client(IdentitySelector::LocalAlias("alice".to_string()))
        .unwrap();
    seed_active_group_conversation(&paths, "did:example:group");

    let result = client
        .attachments()
        .send_conversation_async(SendConversationAttachmentRequest {
            conversation: ConversationReadRef::new("group:did:example:group").unwrap(),
            input: AttachmentInput::Bytes {
                filename: Some("group.txt".to_string()),
                mime_type: Some("text/plain".to_string()),
                bytes: b"group attachment".to_vec(),
            },
            caption: Some("group caption".to_string()),
            mention_payload: None,
            mime_type: None,
            filename: None,
            security: MessageSecurityMode::DefaultPlain,
            client_message_id: Some(MessageId::parse("msg-conv-attachment-group").unwrap()),
            idempotency_key: Some("op-msg-conv-attachment-group".to_string()),
            wait_for_final_acceptance: true,
        })
        .await
        .expect("conversation group attachment send should run upload flow");

    assert_eq!(
        result.message.message.id.as_str(),
        "msg-conv-attachment-group"
    );
    assert_eq!(
        result
            .message
            .message
            .metadata
            .conversation_identity
            .as_ref()
            .map(|identity| identity.conversation_id.as_str()),
        Some("group:did:example:group")
    );
    assert_eq!(result.target_kind, "group");
    assert_eq!(result.target_did, "did:example:group");
    let attachment_id = result.attachment.attachment_id.clone();
    assert!(attachment_id.starts_with("att-"));

    let rows = local_message_rows(
        &paths,
        "SELECT msg_id, conversation_id, thread_id, group_did, content_type, content, metadata FROM messages WHERE msg_id = 'msg-conv-attachment-group'",
    );
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["conversation_id"], "group:did:example:group");
    assert_eq!(rows[0]["thread_id"], "group:did:example:group");
    assert_eq!(rows[0]["group_did"], "did:example:group");
    assert_eq!(
        rows[0]["content_type"],
        awiki_im_core::attachments::attachment_manifest_content_type()
    );
    let stored_manifest: Value =
        serde_json::from_str(rows[0]["content"].as_str().unwrap()).unwrap();
    assert_eq!(stored_manifest["primary_attachment_id"], attachment_id);
    assert_eq!(stored_manifest["caption"], "group caption");
    let metadata: Value = serde_json::from_str(rows[0]["metadata"].as_str().unwrap()).unwrap();
    assert_eq!(metadata["operation_id"], "op-msg-conv-attachment-group");
    assert_eq!(metadata["attachment_id"], attachment_id);

    let requests = server.join();
    assert_eq!(requests.len(), 4);
    assert_eq!(
        requests[0].rpc_method().as_deref(),
        Some("attachment.create_slot")
    );
    assert_eq!(
        requests[0].params()["body"]["intended_target"],
        json!({ "kind": "group", "did": "did:example:group" })
    );
    assert_eq!(requests[3].rpc_method().as_deref(), Some("group.send"));
    assert_eq!(
        requests[3].params()["meta"]["target"],
        json!({ "kind": "group", "did": "did:example:group" })
    );
    assert_eq!(
        requests[3].params()["meta"]["message_id"],
        "msg-conv-attachment-group"
    );
    assert_eq!(
        requests[3].params()["meta"]["operation_id"],
        "op-msg-conv-attachment-group"
    );
}

#[test]
fn message_body_attachments_reuse_canonical_attachment_input() {
    let body = MessageBody::Attachment {
        input: AttachmentInput::LocalFile(PathBuf::from("image.png")),
        caption: Some("caption".to_string()),
        mention_payload: None,
        mime_type: Some("image/png".to_string()),
        filename: Some("override.png".to_string()),
    };

    match body {
        MessageBody::Attachment {
            input: AttachmentInput::LocalFile(path),
            caption,
            mime_type,
            filename,
            ..
        } => {
            assert_eq!(path, PathBuf::from("image.png"));
            assert_eq!(caption.as_deref(), Some("caption"));
            assert_eq!(mime_type.as_deref(), Some("image/png"));
            assert_eq!(filename.as_deref(), Some("override.png"));
        }
        _ => panic!("expected attachment body"),
    }
}

#[test]
fn attachments_service_send_and_memory_download_are_public_runtime_paths() {
    let core = test_core();
    let client = core
        .client(IdentitySelector::LocalAlias("alice".to_string()))
        .unwrap();

    let send = client.attachments().send(
        MessageTarget::Direct(PeerRef::parse("did:example:bob", "").unwrap()),
        AttachmentSendRequest {
            input: AttachmentInput::Bytes {
                filename: Some("image.png".to_string()),
                mime_type: Some("image/png".to_string()),
                bytes: b"png".to_vec(),
            },
            caption: Some("caption".to_string()),
            mention_payload: None,
            mime_type: Some("image/png".to_string()),
            filename: None,
            delivery: MessageDeliveryOptions::default(),
            security: MessageSecurityMode::DefaultPlain,
        },
    );
    assert!(matches!(
        send,
        Err(ImError::InvalidInput { field: Some(field), message })
            if field == "service_did" && message == "message service did is required"
    ));

    let download = client.attachments().download(DownloadAttachmentRequest {
        thread: ThreadRef::Direct(PeerRef::parse("did:example:bob", "").unwrap()),
        message_id: MessageId::parse("msg-1").unwrap(),
        attachment_id: Some("att-1".to_string()),
        destination: AttachmentDestination::Memory,
        overwrite: false,
    });
    assert!(matches!(download, Err(ImError::AuthRequired)));
}

#[tokio::test]
async fn attachments_service_target_send_preserves_explicit_identity_without_conversation() {
    let server = AttachmentServiceTestServer::spawn(vec![
        ExpectedHttp::rpc_result(json!({
            "attachment_id": "att-explicit-id",
            "slot_id": "slot-explicit-id",
            "upload_uri": "__BASE__/objects/slot-explicit-id",
            "upload_headers": {},
            "object_uri": "__BASE__/objects/att-explicit-id",
            "commit_token": "commit-token-explicit-id",
            "expires_at": "2026-05-23T01:00:00Z"
        })),
        ExpectedHttp::json(json!({})),
        ExpectedHttp::rpc_result(json!({
            "committed": true,
            "attachment_id": "att-explicit-id",
            "object_uri": "__BASE__/objects/att-explicit-id",
            "committed_at": "2026-05-23T00:00:01Z"
        })),
        ExpectedHttp::rpc_result(json!({
            "accepted": true,
            "message_id": "msg-explicit-attachment",
            "operation_id": "op-msg-explicit-attachment",
            "target_did": "did:example:bob",
            "accepted_at": "2026-05-23T00:00:02Z",
            "delivery_state": "accepted"
        })),
    ]);
    let (core, _) = test_core_with_base_url_ready_identity_and_service_did(
        server.base_url(),
        "did:example:message-service",
    );
    let client = core
        .client(IdentitySelector::LocalAlias("alice".to_string()))
        .unwrap();

    let result = client
        .attachments()
        .send_with_client_message_id_async(
            MessageTarget::Direct(PeerRef::parse("did:example:bob", "").unwrap()),
            AttachmentSendRequest {
                input: AttachmentInput::Bytes {
                    filename: Some("explicit.txt".to_string()),
                    mime_type: Some("text/plain".to_string()),
                    bytes: b"explicit identity".to_vec(),
                },
                caption: None,
                mention_payload: None,
                mime_type: None,
                filename: None,
                delivery: MessageDeliveryOptions {
                    idempotency_key: Some("op-msg-explicit-attachment".to_string()),
                    wait_for_final_acceptance: false,
                },
                security: MessageSecurityMode::DefaultPlain,
            },
            MessageId::parse("msg-explicit-attachment").unwrap(),
        )
        .await
        .expect("target-based attachment send should not require a conversation registry row");

    assert_eq!(
        result.message.message.id.as_str(),
        "msg-explicit-attachment"
    );
    let requests = server.join();
    assert_eq!(requests.len(), 4);
    assert_eq!(
        requests[0].rpc_method().as_deref(),
        Some("attachment.create_slot")
    );
    assert_eq!(requests[3].rpc_method().as_deref(), Some("direct.send"));
    assert_eq!(
        requests[3].params()["meta"]["message_id"],
        "msg-explicit-attachment"
    );
    assert_eq!(
        requests[3].params()["meta"]["operation_id"],
        "op-msg-explicit-attachment"
    );
}

#[tokio::test]
async fn attachments_service_send_resolves_direct_handle_before_upload_flow() {
    let server = AttachmentServiceTestServer::spawn(vec![
        ExpectedHttp::rpc_result(handle_lookup_result()),
        ExpectedHttp::rpc_result(json!({
            "attachment_id": "att-1",
            "slot_id": "slot-1",
            "upload_uri": "__BASE__/objects/slot-1",
            "upload_headers": {
                "X-Upload-Token": "token-1",
                "Ignored-Number": 7
            },
            "object_uri": "__BASE__/objects/att-1",
            "commit_token": "commit-token-1",
            "expires_at": "2026-05-23T01:00:00Z"
        })),
        ExpectedHttp::json(json!({})),
        ExpectedHttp::rpc_result(json!({
            "committed": true,
            "attachment_id": "att-1",
            "object_uri": "__BASE__/objects/att-1",
            "committed_at": "2026-05-23T00:00:01Z"
        })),
        ExpectedHttp::rpc_result(json!({
            "accepted": true,
            "message_id": "msg-attachment-send-1",
            "operation_id": "op-attachment-send-1",
            "target_did": "did:example:bob",
            "accepted_at": "2026-05-23T00:00:02Z",
            "delivery_state": "accepted"
        })),
    ]);
    let (core, paths) = test_core_with_base_url_ready_identity_and_service_did(
        server.base_url(),
        "did:example:message-service",
    );
    let client = core
        .client(IdentitySelector::LocalAlias("alice".to_string()))
        .unwrap();

    let result = client
        .attachments()
        .send_async(
            MessageTarget::Direct(PeerRef::parse("bob.awiki.info", "").unwrap()),
            AttachmentSendRequest {
                input: AttachmentInput::Bytes {
                    filename: Some("input.txt".to_string()),
                    mime_type: Some("text/plain".to_string()),
                    bytes: b"hello".to_vec(),
                },
                caption: Some("caption".to_string()),
                mention_payload: None,
                mime_type: Some("application/custom".to_string()),
                filename: Some("override.bin".to_string()),
                delivery: MessageDeliveryOptions::default(),
                security: MessageSecurityMode::DefaultPlain,
            },
        )
        .await
        .expect("public attachment send should resolve handle and run upload");

    assert_eq!(result.message.message.id.as_str(), "msg-attachment-send-1");
    assert_eq!(
        result.message.message.receiver.as_ref().unwrap().as_str(),
        "bob.awiki.info"
    );
    assert!(matches!(result.message.delivery, DeliveryState::Accepted));
    assert_eq!(result.target_kind, "agent");
    assert_eq!(result.target_did, "did:example:bob");
    let attachment_id = result.attachment.attachment_id.clone();
    assert!(attachment_id.starts_with("att-"));
    assert_eq!(result.attachment.filename, "override.bin");
    assert_eq!(result.attachment.mime_type, "application/custom");
    assert_eq!(result.attachment.size_bytes, 5);
    assert_eq!(result.attachment.size, "5");
    assert_eq!(
        result.attachment.object_uri,
        format!("{}/objects/att-1", server.base_url())
    );
    assert_eq!(result.manifest["primary_attachment_id"], attachment_id);
    assert_eq!(result.manifest["caption"], "caption");
    let rows = local_message_rows(
        &paths,
        "SELECT msg_id, owner_identity_id, owner_did, receiver_did, content_type, content, is_read, metadata FROM messages WHERE msg_id = 'msg-attachment-send-1'",
    );
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["owner_identity_id"], "alice-id");
    assert_eq!(rows[0]["owner_did"], "did:example:alice");
    assert_eq!(rows[0]["receiver_did"], "did:example:bob");
    assert_eq!(
        rows[0]["content_type"],
        awiki_im_core::attachments::attachment_manifest_content_type()
    );
    assert_eq!(rows[0]["is_read"], 1);
    let stored_manifest: Value =
        serde_json::from_str(rows[0]["content"].as_str().unwrap()).unwrap();
    assert_eq!(stored_manifest["primary_attachment_id"], attachment_id);
    assert_eq!(stored_manifest["caption"], "caption");
    let metadata: Value = serde_json::from_str(rows[0]["metadata"].as_str().unwrap()).unwrap();
    assert_eq!(metadata["operation_id"], "op-attachment-send-1");
    assert_eq!(metadata["delivery_state"], "accepted");
    assert_eq!(metadata["target_handle"], "bob.awiki.info");
    assert_eq!(metadata["attachment_id"], attachment_id);
    assert_eq!(
        metadata["object_uri"],
        format!("{}/objects/att-1", server.base_url())
    );

    let requests = server.join();
    assert_eq!(requests.len(), 5);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/user-service/v1/handle/rpc");
    assert_eq!(requests[0].rpc_method().as_deref(), Some("lookup"));
    assert_eq!(requests[0].params(), json!({ "handle": "bob.awiki.info" }));

    assert_eq!(requests[1].method, "POST");
    assert_eq!(requests[1].path, "/im/rpc");
    assert_eq!(
        requests[1].rpc_method().as_deref(),
        Some("attachment.create_slot")
    );
    assert_eq!(
        requests[1].params()["meta"]["target"],
        json!({ "kind": "service", "did": "did:example:message-service" })
    );
    assert_eq!(requests[1].params()["body"]["filename"], "override.bin");
    assert_eq!(
        requests[1].params()["body"]["mime_type"],
        "application/custom"
    );
    assert_eq!(
        requests[1].params()["body"]["intended_target"],
        json!({ "kind": "agent", "did": "did:example:bob" })
    );

    assert_eq!(requests[2].method, "PUT");
    assert_eq!(requests[2].path, "/objects/slot-1");
    assert_eq!(requests[2].body, b"hello".to_vec());

    assert_eq!(requests[3].method, "POST");
    assert_eq!(requests[3].path, "/im/rpc");
    assert_eq!(
        requests[3].rpc_method().as_deref(),
        Some("attachment.commit_object")
    );
    assert_eq!(requests[3].params()["body"]["attachment_id"], attachment_id);
    assert_eq!(requests[3].params()["body"]["slot_id"], "slot-1");

    assert_eq!(requests[4].method, "POST");
    assert_eq!(requests[4].path, "/im/rpc");
    assert_eq!(requests[4].rpc_method().as_deref(), Some("direct.send"));
    assert_eq!(
        requests[4].params()["meta"]["target"],
        json!({ "kind": "agent", "did": "did:example:bob" })
    );
    assert_eq!(
        requests[4].params()["meta"]["content_type"],
        awiki_im_core::compat::attachments::attachment_manifest_content_type()
    );
    assert_eq!(
        requests[4].params()["body"]["payload"]["primary_attachment_id"],
        attachment_id
    );
    assert_eq!(
        requests[4].params()["body"]["payload"]["caption"],
        "caption"
    );
}

#[tokio::test]
async fn attachments_service_download_resolves_direct_handle_before_history_lookup() {
    let server = AttachmentServiceTestServer::spawn(vec![
        ExpectedHttp::rpc_result(handle_lookup_result()),
        ExpectedHttp::rpc_result(json!({
            "messages": [{
                "id": "msg-attachment-1",
                "message_id": "msg-attachment-1",
                "sender_did": "did:example:bob",
                "content": {
                    "attachments": [{
                        "attachment_id": "att-1",
                        "filename": "report.txt",
                        "mime_type": "text/plain",
                        "size": "16",
                        "digest": { "alg": "sha-256", "value_b64u": "digest-1" },
                        "access_info": { "object_uri": "__BASE__/objects/att-1" }
                    }],
                    "primary_attachment_id": "att-1",
                    "caption": "report"
                }
            }],
            "has_more": false
        })),
    ]);
    let core = test_core_with_base_url_and_ready_identity(server.base_url());
    let client = core
        .client(IdentitySelector::LocalAlias("alice".to_string()))
        .unwrap();

    let err = client
        .attachments()
        .download_async(DownloadAttachmentRequest {
            thread: ThreadRef::Direct(PeerRef::parse("bob.awiki.info", "").unwrap()),
            message_id: MessageId::parse("msg-attachment-1").unwrap(),
            attachment_id: Some("att-1".to_string()),
            destination: AttachmentDestination::Memory,
            overwrite: false,
        })
        .await
        .expect_err("unsupported sender DID should stop after resolved history lookup");
    assert!(matches!(
        err,
        ImError::InvalidInput { field: Some(field), message }
            if field == "did" && message.contains("unsupported DID method")
    ));

    let requests = server.join();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/user-service/v1/handle/rpc");
    assert_eq!(requests[0].rpc_method().as_deref(), Some("lookup"));
    assert_eq!(requests[0].params(), json!({ "handle": "bob.awiki.info" }));
    assert_eq!(requests[1].method, "POST");
    assert_eq!(requests[1].path, "/im/rpc");
    assert_eq!(
        requests[1].rpc_method().as_deref(),
        Some("direct.get_history")
    );
    assert_eq!(requests[1].params()["body"]["peer_did"], "did:example:bob");
}

#[test]
fn attachments_dto_supports_explicit_local_file_destination() {
    let request = DownloadAttachmentRequest {
        thread: ThreadRef::Group(GroupRef::parse("did:example:group").unwrap()),
        message_id: MessageId::parse("msg-1").unwrap(),
        attachment_id: None,
        destination: AttachmentDestination::LocalFile(PathBuf::from("/tmp/output.bin")),
        overwrite: true,
    };

    assert!(matches!(
        request.destination,
        AttachmentDestination::LocalFile(path) if path == PathBuf::from("/tmp/output.bin")
    ));
    assert!(request.overwrite);
}

#[test]
fn attachments_manifest_and_content_type_match_wire_contract() {
    let prepared = awiki_im_core::compat::attachments::PreparedAttachment {
        filename: "hello.txt".to_string(),
        mime_type: "text/plain".to_string(),
        size_string: "5".to_string(),
        digest_b64u: "digest".to_string(),
        ..awiki_im_core::compat::attachments::PreparedAttachment::default()
    };
    let descriptor = awiki_im_core::compat::attachments::AttachmentDescriptor::from_prepared(
        &prepared,
        "att-1",
        "http://127.0.0.1:8080/objects/obj-1",
    );

    let manifest =
        awiki_im_core::compat::attachments::build_attachment_manifest(&descriptor, "hello");

    assert_eq!(
        awiki_im_core::compat::attachments::attachment_manifest_content_type(),
        "application/anp-attachment-manifest+json"
    );
    assert_eq!(manifest["primary_attachment_id"], "att-1");
    assert_eq!(manifest["caption"], "hello");
    assert_eq!(
        manifest["attachments"][0]["digest"],
        json!({ "alg": "sha-256", "value_b64u": "digest" })
    );
    assert_eq!(
        manifest["attachments"][0]["access_info"]["object_uri"],
        "http://127.0.0.1:8080/objects/obj-1"
    );
    assert!(
        awiki_im_core::compat::attachments::manifest_content_string(&manifest)
            .contains("\"primary_attachment_id\":\"att-1\"")
    );
}

#[test]
fn attachments_prepare_payload_normalizes_digest_filename_and_mime_type() {
    let prepared = awiki_im_core::compat::attachments::prepare_attachment_payload(
        "hello.txt",
        "",
        b"hello".to_vec(),
    )
    .expect("prepared attachment");

    assert_eq!(prepared.filename, "hello.txt");
    assert_eq!(prepared.mime_type, "text/plain; charset=utf-8");
    assert_eq!(prepared.size_bytes, 5);
    assert_eq!(prepared.size_string, "5");
    assert_eq!(
        prepared.digest_b64u,
        "LPJNul-wow4m6DsqxbninhsWHlwfp0JecwQzYpOLmCQ"
    );
    assert_eq!(prepared.payload, b"hello".to_vec());

    let overridden = awiki_im_core::compat::attachments::prepare_attachment_payload(
        "payload.bin",
        "application/custom",
        b"\x89PNG\r\n\x1a\n".to_vec(),
    )
    .expect("prepared with mime override");
    assert_eq!(overridden.mime_type, "application/custom");

    let detected_png = awiki_im_core::compat::attachments::prepare_attachment_payload(
        "payload.bin",
        "",
        b"\x89PNG\r\n\x1a\n".to_vec(),
    )
    .expect("prepared png");
    assert_eq!(detected_png.mime_type, "image/png");
}

#[test]
fn attachment_manifest_object_e2ee_redacted_and_grant_ref_contract() {
    let descriptor = awiki_im_core::compat::attachments::AttachmentDescriptor {
        attachment_id: "att-e2ee-1".to_string(),
        filename: "secret.pdf".to_string(),
        mime_type: "application/pdf".to_string(),
        size: "33".to_string(),
        digest_b64u: "ciphertext-digest".to_string(),
        object_uri: "https://objects.example/att-e2ee-1".to_string(),
        encryption_mode: "object-e2ee".to_string(),
        object_cipher: Some("chacha20-poly1305".to_string()),
        plaintext_size: Some("16".to_string()),
    };

    let redacted = awiki_im_core::compat::attachments::build_attachment_manifest_internal(
        &descriptor,
        "secret",
    );
    assert_eq!(redacted["primary_attachment_id"], "att-e2ee-1");
    assert_eq!(
        redacted["attachments"][0]["encryption_info"]["mode"],
        "object-e2ee"
    );
    assert_eq!(
        redacted["attachments"][0]["encryption_info"]["object_cipher"],
        "chacha20-poly1305"
    );
    assert_eq!(
        redacted["attachments"][0]["encryption_info"]["plaintext_size"],
        "16"
    );
    assert_eq!(
        redacted["attachments"][0]["encryption_info"].get("object_key_b64u"),
        None
    );
    assert_eq!(
        redacted["attachments"][0]["encryption_info"].get("nonce_b64u"),
        None
    );

    let default_public =
        awiki_im_core::compat::attachments::build_attachment_manifest(&descriptor, "secret");
    assert_eq!(default_public, redacted);

    let parsed = awiki_im_core::compat::attachments::parse_attachment_manifest(&redacted)
        .expect("manifest parses");
    let parsed_descriptor = &parsed.attachments[0];
    assert_eq!(parsed_descriptor.encryption_mode, "object-e2ee");
    assert_eq!(parsed_descriptor.plaintext_size.as_deref(), Some("16"));

    let grant_ref = awiki_im_core::compat::attachments::build_attachment_grant_ref(&descriptor)
        .expect("grant ref");
    assert_eq!(grant_ref["attachment_id"], "att-e2ee-1");
    assert_eq!(
        grant_ref["object_uri"],
        "https://objects.example/att-e2ee-1"
    );
    assert_eq!(grant_ref["object_encryption_mode"], "object-e2ee");
    assert_eq!(grant_ref["plaintext_size"], "16");
    assert_eq!(grant_ref["digest"]["value_b64u"], "ciphertext-digest");
    assert_eq!(grant_ref.get("object_key_b64u"), None);
    assert_eq!(grant_ref.get("nonce_b64u"), None);
}

#[test]
fn attachment_manifest_plain_mode_remains_redacted_compatible() {
    let prepared = awiki_im_core::compat::attachments::prepare_attachment_payload(
        "hello.txt",
        "",
        b"hello".to_vec(),
    )
    .expect("plain prepared");
    let descriptor = awiki_im_core::compat::attachments::AttachmentDescriptor::from_prepared(
        &prepared,
        "att-plain-1",
        "https://objects.example/att-plain-1",
    );

    let manifest = awiki_im_core::compat::attachments::build_attachment_manifest_internal(
        &descriptor,
        "hello",
    );
    assert_eq!(
        manifest["attachments"][0]["encryption_info"],
        json!({"mode": "none"})
    );
    assert_eq!(
        awiki_im_core::compat::attachments::build_attachment_manifest(&descriptor, "hello"),
        manifest
    );
    let grant_ref = awiki_im_core::compat::attachments::build_attachment_grant_ref(&descriptor)
        .expect("grant ref");
    assert_eq!(grant_ref["object_encryption_mode"], "none");
    assert_eq!(grant_ref.get("plaintext_size"), None);
}

#[test]
fn attachment_api_uploaded_attachment_public_dto_never_exposes_key_nonce() {
    let attachment = UploadedAttachment {
        attachment_id: "att-e2ee-1".to_string(),
        filename: "secret.pdf".to_string(),
        mime_type: "application/pdf".to_string(),
        size_bytes: 33,
        size: "33".to_string(),
        digest_b64u: "ciphertext-digest".to_string(),
        object_uri: "https://objects.example/att-e2ee-1".to_string(),
        object_encryption_mode: "object-e2ee".to_string(),
        plaintext_size_bytes: Some(16),
    };

    let value = serde_json::to_value(&attachment).expect("uploaded attachment serializes");
    assert_eq!(value["object_encryption_mode"], "object-e2ee");
    assert_eq!(value["plaintext_size_bytes"], 16);
    assert_eq!(value.get("object_key_b64u"), None);
    assert_eq!(value.get("nonce_b64u"), None);
    assert!(!value.to_string().contains("BASE64URL_32_BYTES_OBJECT_KEY"));
}

#[test]
fn attachments_selection_matches_visible_or_raw_message_id() {
    let messages = vec![json!({
        "id": "did:wba:awiki.ai:groups:test:e1_group:7",
        "message_id": "msg-raw-1",
        "sender_did": "did:wba:awiki.ai:user:alice:e1",
        "content": {
            "attachments": [{
                "attachment_id": "att-1",
                "filename": "hello.txt",
                "mime_type": "text/plain",
                "size": "5",
                "digest": { "alg": "sha-256", "value_b64u": "digest" },
                "access_info": { "object_uri": "http://127.0.0.1:8080/objects/obj-1" }
            }],
            "primary_attachment_id": "att-1",
            "caption": "hello"
        }
    })];

    let by_visible_id = awiki_im_core::compat::attachments::find_attachment_selection(
        &messages,
        "did:wba:awiki.ai:groups:test:e1_group:7",
        "",
    )
    .expect("selection by visible id");
    assert_eq!(by_visible_id.message_id, "msg-raw-1");
    assert_eq!(
        by_visible_id.requested_id,
        "did:wba:awiki.ai:groups:test:e1_group:7"
    );
    assert_eq!(by_visible_id.attachment_id, "att-1");
    assert_eq!(by_visible_id.sender_did, "did:wba:awiki.ai:user:alice:e1");
    assert_eq!(by_visible_id.caption, "hello");

    let by_raw_id = awiki_im_core::compat::attachments::find_attachment_selection(
        &messages,
        "msg-raw-1",
        "att-1",
    )
    .expect("selection by raw id");
    assert_eq!(by_raw_id.object_uri, "http://127.0.0.1:8080/objects/obj-1");
    assert_eq!(by_raw_id.digest_b64u, "digest");
}

#[test]
fn attachments_selection_parses_object_e2ee_metadata_but_redacts_key_nonce() {
    let messages = vec![json!({
        "id": "msg-e2ee-1",
        "sender_did": "did:wba:awiki.ai:user:alice:e1",
        "security": "secure-direct",
        "content": {
            "attachments": [{
                "attachment_id": "att-e2ee-1",
                "filename": "secret.pdf",
                "mime_type": "application/pdf",
                "size": "33",
                "digest": { "alg": "sha-256", "value_b64u": "ciphertext-digest" },
                "access_info": { "object_uri": "https://objects.example/att-e2ee-1" },
                "encryption_info": {
                    "mode": "object-e2ee",
                    "object_cipher": "chacha20-poly1305",
                    "object_key_b64u": "BASE64URL_OBJECT_KEY",
                    "nonce_b64u": "BASE64URL_NONCE",
                    "plaintext_size": "16"
                }
            }],
            "primary_attachment_id": "att-e2ee-1"
        }
    })];

    let selection =
        awiki_im_core::compat::attachments::find_attachment_selection(&messages, "msg-e2ee-1", "")
            .expect("public selection");
    assert_eq!(selection.message_security_profile, "direct-e2ee");
    assert_eq!(selection.object_encryption_mode, "object-e2ee");
    assert_eq!(
        selection.object_cipher.as_deref(),
        Some("chacha20-poly1305")
    );
    assert_eq!(selection.plaintext_size.as_deref(), Some("16"));

    let public_json = serde_json::to_string(&selection).expect("selection serializes");
    assert!(!public_json.contains("object_key_b64u"));
    assert!(!public_json.contains("nonce_b64u"));
    assert!(!public_json.contains("BASE64URL_OBJECT_KEY"));
    assert!(!public_json.contains("BASE64URL_NONCE"));
}

#[test]
fn attachments_selection_requires_id_for_multiple_attachments() {
    let messages = vec![json!({
        "id": "msg-1",
        "sender_did": "did:wba:awiki.ai:user:alice:e1",
        "content": {
            "attachments": [
                {
                    "attachment_id": "att-1",
                    "access_info": { "object_uri": "http://example.test/1" }
                },
                {
                    "attachment_id": "att-2",
                    "access_info": { "object_uri": "http://example.test/2" }
                }
            ]
        }
    })];

    let err = awiki_im_core::compat::attachments::find_attachment_selection(&messages, "msg-1", "")
        .expect_err("missing attachment id should fail");

    assert!(matches!(
        err,
        ImError::InvalidInput { field: Some(field), message }
            if field == "attachment_id"
                && message == awiki_im_core::compat::attachments::ERR_ATTACHMENT_ID_REQUIRED
    ));
}

#[test]
fn attachments_selection_with_paging_advances_until_match() {
    let mut requested_skips = Vec::new();
    let selection = awiki_im_core::compat::attachments::find_attachment_selection_with_paging(
        |skip| {
            requested_skips.push(skip);
            if skip == 0 {
                Ok((vec![json!({ "id": "other", "content": {} })], true))
            } else {
                Ok((
                    vec![json!({
                        "id": "msg-2",
                        "sender_did": "did:wba:awiki.ai:user:alice:e1",
                        "content": serde_json::to_string(&json!({
                            "attachments": [{
                                "attachment_id": "att-2",
                                "filename": "two.bin",
                                "access_info": { "object_uri": "http://example.test/2" }
                            }]
                        }))
                        .unwrap()
                    })],
                    false,
                ))
            }
        },
        "msg-2",
        "att-2",
    )
    .expect("paged selection");

    assert_eq!(requested_skips, vec![0, 1]);
    assert_eq!(selection.attachment_id, "att-2");
    assert_eq!(selection.filename, "two.bin");
    assert_eq!(selection.object_uri, "http://example.test/2");
}

#[test]
fn attachment_discovery_selects_lowest_priority_compatible_service() {
    let document = json!({
        "service": [
            {
                "id": "#direct",
                "type": "ANPMessageService",
                "serviceEndpoint": "https://example.com/direct/rpc",
                "serviceDid": "did:wba:example.com",
                "profiles": ["anp.direct.base.v1"],
                "securityProfiles": ["transport-protected"],
                "priority": 9
            },
            {
                "id": "#secondary",
                "type": "ANPMessageService",
                "serviceEndpoint": "https://example.com/secondary/rpc",
                "serviceDid": "did:wba:example.com",
                "profiles": ["anp.attachment.v1"],
                "securityProfiles": ["transport-protected"],
                "priority": 1
            },
            {
                "id": "#primary",
                "type": "ANPMessageService",
                "serviceEndpoint": "https://example.com/primary/rpc",
                "serviceDid": "did:wba:example.com",
                "profiles": ["anp.attachment.v1"],
                "securityProfiles": ["transport-protected"],
                "priority": "2"
            }
        ]
    });

    let service = awiki_im_core::compat::attachments::select_attachment_rpc_service_from_document(
        "did:wba:example.com:user:alice:e1",
        &document,
    )
    .expect("service");

    assert_eq!(service.rpc_endpoint, "https://example.com/secondary/rpc");
    assert_eq!(service.service_did, "did:wba:example.com");
    assert_eq!(service.sender_did, "did:wba:example.com:user:alice:e1");
}

#[test]
fn attachment_discovery_keeps_explicit_legacy_v1_compatibility() {
    let document = json!({
        "service": [{
            "id": "#attachment",
            "type": "ANPMessageService",
            "serviceEndpoint": "https://example.com/attachment/rpc",
            "serviceDid": "did:wba:example.com",
            "profiles": ["anp.attachment.v1"],
            "securityProfiles": ["transport-protected"]
        }]
    });

    let service = awiki_im_core::compat::attachments::select_attachment_rpc_service_from_document(
        "did:wba:example.com:user:alice:e1",
        &document,
    )
    .expect("legacy service");

    assert_eq!(service.rpc_endpoint, "https://example.com/attachment/rpc");
}

#[test]
fn attachment_discovery_uses_explicit_v1_when_other_profiles_are_v2() {
    let document = json!({
        "service": [{
            "id": "#message",
            "type": "ANPMessageService",
            "serviceEndpoint": "https://example.com/anp-im/rpc",
            "serviceDid": "did:wba:example.com",
            "profiles": ["anp.core.binding.v1", "anp.attachment.v1"],
            "securityProfiles": ["transport-protected"]
        }]
    });

    let service = awiki_im_core::compat::attachments::select_attachment_rpc_service_from_document(
        "did:wba:example.com:user:alice:e1",
        &document,
    )
    .expect("explicit attachment v1 remains an independent negotiated contract");

    assert_eq!(service.rpc_endpoint, "https://example.com/anp-im/rpc");
}

#[test]
fn attachment_discovery_requires_attachment_profile_and_transport_security() {
    let document = json!({
        "service": [
            {
                "id": "#direct-only",
                "type": "ANPMessageService",
                "serviceEndpoint": "https://example.com/direct/rpc",
                "serviceDid": "did:wba:example.com",
                "profiles": ["anp.direct.base.v1"],
                "securityProfiles": ["transport-protected"],
                "priority": 1
            },
            {
                "id": "#wrong-security",
                "type": "ANPMessageService",
                "serviceEndpoint": "https://example.com/attachment/rpc",
                "serviceDid": "did:wba:example.com",
                "profiles": ["anp.attachment.v1"],
                "securityProfiles": ["secure-channel"],
                "priority": 2
            }
        ]
    });

    let err = awiki_im_core::compat::attachments::select_attachment_rpc_service_from_document(
        "did:wba:example.com:user:alice:e1",
        &document,
    )
    .expect_err("missing compatible attachment service should fail");

    assert!(matches!(
        err,
        ImError::InvalidInput { field: Some(field), message }
            if field == "did_document"
                && message.contains("does not expose a compatible ANPMessageService")
    ));
}

#[test]
fn attachment_discovery_rejects_endpoint_without_http_scheme() {
    let document = json!({
        "service": [{
            "id": "#attachment",
            "type": "ANPMessageService",
            "serviceEndpoint": "example.com/attachment/rpc",
            "serviceDid": "did:wba:example.com",
            "profiles": ["anp.attachment.v1"],
            "security_profiles": ["transport-protected"]
        }]
    });

    let err = awiki_im_core::compat::attachments::select_attachment_rpc_service_from_document(
        "did:wba:example.com:user:alice:e1",
        &document,
    )
    .expect_err("invalid endpoint should fail");

    assert!(matches!(
        err,
        ImError::InvalidInput { field: Some(field), message }
            if field == "service_endpoint"
                && message == "attachment service endpoint is invalid: missing protocol scheme"
    ));
}

#[test]
fn attachment_wire_slot_commit_ticket_and_manifest_send_match_contracts() {
    let prepared = awiki_im_core::compat::attachments::PreparedAttachment {
        filename: "hello.txt".to_string(),
        mime_type: "text/plain".to_string(),
        size_string: "5".to_string(),
        digest_b64u: "digest".to_string(),
        ..awiki_im_core::compat::attachments::PreparedAttachment::default()
    };

    let create_slot = awiki_im_core::compat::attachments::build_attachment_create_slot_rpc_params(
        "did:wba:awiki.ai:user:alice:e1_alice",
        "did:wba:awiki.ai:services:message:e1",
        "agent",
        "did:wba:awiki.ai:user:bob:e1_bob",
        &prepared,
    )
    .expect("create-slot params");
    assert_eq!(create_slot["meta"]["profile"], "anp.attachment.v1");
    assert!(create_slot["meta"].get("anp_version").is_none());
    assert_eq!(
        create_slot["meta"]["target"],
        json!({ "kind": "service", "did": "did:wba:awiki.ai:services:message:e1" })
    );
    assert_eq!(create_slot["body"]["expected_size"], "5");
    assert!(create_slot["body"]["attachment_id"]
        .as_str()
        .is_some_and(|attachment_id| attachment_id.starts_with("att-") && attachment_id.len() > 4));
    assert_eq!(
        create_slot["body"]["expected_digest"]["value_b64u"],
        "digest"
    );
    assert_eq!(
        create_slot["body"]["intended_target"],
        json!({ "kind": "agent", "did": "did:wba:awiki.ai:user:bob:e1_bob" })
    );
    assert_eq!(create_slot.get("auth"), None);

    let slot = awiki_im_core::compat::attachments::AttachmentCreateSlotResult {
        attachment_id: "att-1".to_string(),
        slot_id: "slot-1".to_string(),
        commit_token: "commit-token".to_string(),
        object_uri: "http://127.0.0.1:8080/objects/obj-1".to_string(),
        ..awiki_im_core::compat::attachments::AttachmentCreateSlotResult::default()
    };
    let commit = awiki_im_core::compat::attachments::build_attachment_commit_object_rpc_params(
        "did:wba:awiki.ai:user:alice:e1_alice",
        "did:wba:awiki.ai:services:message:e1",
        &prepared,
        &slot,
    )
    .expect("commit params");
    assert_eq!(commit["body"]["attachment_id"], "att-1");
    assert_eq!(commit["body"]["slot_id"], "slot-1");
    assert_eq!(commit["body"]["commit_token"], "commit-token");
    assert_eq!(commit["body"]["size"], "5");
    assert_eq!(commit["body"]["digest"]["value_b64u"], "digest");
    assert_eq!(commit["body"]["object_encryption_mode"], "none");
    assert_eq!(commit["body"].get("object_key_b64u"), None);
    assert_eq!(commit["body"].get("nonce_b64u"), None);

    let selection = awiki_im_core::compat::attachments::AttachmentSelection {
        attachment_id: "att-1".to_string(),
        object_uri: "http://127.0.0.1:8080/objects/obj-1".to_string(),
        ..awiki_im_core::compat::attachments::AttachmentSelection::default()
    };
    let ticket = awiki_im_core::compat::attachments::build_attachment_download_ticket_rpc_params(
        "did:wba:awiki.ai:user:alice:e1_alice",
        "did:wba:awiki.ai",
        "did:wba:awiki.ai:user:bob:e1_bob",
        "msg-1",
        "",
        &selection,
    )
    .expect("ticket params");
    assert_eq!(ticket["meta"]["profile"], "anp.attachment.v1");
    assert!(ticket["meta"].get("anp_version").is_none());
    assert_eq!(ticket["body"].get("sender_did"), None);
    assert_eq!(
        ticket["body"]["requester_did"],
        "did:wba:awiki.ai:user:alice:e1_alice"
    );
    assert_eq!(
        ticket["body"]["message_target_did"],
        "did:wba:awiki.ai:user:alice:e1_alice"
    );
    assert_eq!(ticket.get("auth"), None);

    let direct_e2ee_selection = awiki_im_core::compat::attachments::AttachmentSelection {
        attachment_id: "att-e2ee-1".to_string(),
        object_uri: "http://127.0.0.1:8080/objects/e2ee-1".to_string(),
        object_encryption_mode: "object-e2ee".to_string(),
        ..awiki_im_core::compat::attachments::AttachmentSelection::default()
    };
    let direct_e2ee_ticket =
        awiki_im_core::compat::attachments::build_attachment_download_ticket_rpc_params(
            "did:wba:awiki.ai:user:alice:e1_alice",
            "did:wba:awiki.ai",
            "did:wba:awiki.ai:user:bob:e1_bob",
            "msg-e2ee-1",
            "",
            &direct_e2ee_selection,
        )
        .expect("direct e2ee ticket params");
    assert_eq!(
        direct_e2ee_ticket["body"]["message_security_profile"],
        "direct-e2ee"
    );
    assert_eq!(
        direct_e2ee_ticket["body"]["message_target_did"],
        "did:wba:awiki.ai:user:alice:e1_alice"
    );
    assert_eq!(direct_e2ee_ticket["body"].get("group_did"), None);

    let group_e2ee_ticket =
        awiki_im_core::compat::attachments::build_attachment_download_ticket_rpc_params(
            "did:wba:awiki.ai:user:alice:e1_alice",
            "did:wba:awiki.ai",
            "did:wba:awiki.ai:user:bob:e1_bob",
            "msg-e2ee-1",
            "did:wba:awiki.ai:groups:test:e1_group",
            &direct_e2ee_selection,
        )
        .expect("group e2ee ticket params");
    assert_eq!(
        group_e2ee_ticket["body"]["message_security_profile"],
        "group-e2ee"
    );
    assert_eq!(
        group_e2ee_ticket["body"]["group_did"],
        "did:wba:awiki.ai:groups:test:e1_group"
    );
    assert_eq!(group_e2ee_ticket["body"].get("message_target_did"), None);

    let identity = generated_attachment_wire_identity();
    let manifest = json!({
        "attachments": [{
            "attachment_id": "att-1",
            "filename": "hello.txt",
        }],
        "primary_attachment_id": "att-1"
    });
    let direct = awiki_im_core::compat::attachments::build_direct_attachment_send_rpc_params(
        &identity,
        "did:wba:awiki.ai:user:bob:e1_bob",
        manifest.clone(),
    )
    .expect("direct attachment params");
    assert_eq!(direct["meta"]["profile"], "anp.direct.base.v1");
    assert_eq!(
        direct["meta"]["target"],
        json!({ "kind": "agent", "did": "did:wba:awiki.ai:user:bob:e1_bob" })
    );
    assert_eq!(
        direct["meta"]["content_type"],
        awiki_im_core::compat::attachments::attachment_manifest_content_type()
    );
    assert_eq!(direct["body"]["payload"], manifest);
    assert_eq!(
        direct["auth"]["scheme"],
        awiki_im_core::compat::proof::ORIGIN_PROOF_SCHEME
    );

    let group = awiki_im_core::compat::attachments::build_group_attachment_send_rpc_params(
        &identity,
        "did:wba:awiki.ai:groups:test:e1_group",
        direct["body"]["payload"].clone(),
    )
    .expect("group attachment params");
    assert_eq!(group["meta"]["profile"], "anp.group.base.v1");
    assert_eq!(
        group["meta"]["target"],
        json!({ "kind": "group", "did": "did:wba:awiki.ai:groups:test:e1_group" })
    );
    assert_eq!(
        group["meta"]["content_type"],
        awiki_im_core::compat::attachments::attachment_manifest_content_type()
    );
    assert_eq!(
        group["auth"]["scheme"],
        awiki_im_core::compat::proof::ORIGIN_PROOF_SCHEME
    );
}

#[test]
fn attachment_wire_object_e2ee_control_params_exclude_key_nonce() {
    let prepared = awiki_im_core::compat::attachments::PreparedAttachment {
        filename: "hello.txt".to_string(),
        mime_type: "text/plain".to_string(),
        size_bytes: 33,
        size_string: "33".to_string(),
        digest_b64u: "ciphertext-digest".to_string(),
        payload: b"ciphertext bytes with auth tag".to_vec(),
        object_encryption_mode: "object-e2ee".to_string(),
        object_cipher: Some("chacha20-poly1305".to_string()),
        plaintext_size_bytes: Some(12),
        plaintext_size_string: Some("12".to_string()),
    };

    let err = awiki_im_core::compat::attachments::build_attachment_create_slot_rpc_params(
        "did:wba:awiki.ai:user:alice:e1_alice",
        "did:wba:awiki.ai:services:message:e1",
        "agent",
        "did:wba:awiki.ai:user:bob:e1_bob",
        &prepared,
    )
    .expect_err("plain control profile should reject object-e2ee");
    assert!(matches!(
        err,
        ImError::InvalidInput { field: Some(field), message }
            if field == "message_security_profile" && message.contains("object-e2ee")
    ));

    let create_slot =
        awiki_im_core::compat::attachments::build_attachment_create_slot_rpc_params_with_security_profile(
            "did:wba:awiki.ai:user:alice:e1_alice",
            "did:wba:awiki.ai:services:message:e1",
            "agent",
            "did:wba:awiki.ai:user:bob:e1_bob",
            "direct-e2ee",
            &prepared,
        )
        .expect("e2ee create-slot params");
    assert_eq!(
        create_slot["body"]["intended_message_security_profile"],
        "direct-e2ee"
    );
    assert_eq!(create_slot["body"]["object_encryption_mode"], "object-e2ee");
    assert_eq!(create_slot["body"]["expected_size"], prepared.size_string);
    assert_eq!(
        create_slot["body"]["expected_digest"]["value_b64u"],
        prepared.digest_b64u
    );
    assert_eq!(create_slot["body"].get("object_key_b64u"), None);
    assert_eq!(create_slot["body"].get("nonce_b64u"), None);

    let slot = awiki_im_core::compat::attachments::AttachmentCreateSlotResult {
        attachment_id: "att-1".to_string(),
        slot_id: "slot-1".to_string(),
        commit_token: "commit-token".to_string(),
        object_uri: "http://127.0.0.1:8080/objects/obj-1".to_string(),
        ..awiki_im_core::compat::attachments::AttachmentCreateSlotResult::default()
    };
    let commit = awiki_im_core::compat::attachments::build_attachment_commit_object_rpc_params(
        "did:wba:awiki.ai:user:alice:e1_alice",
        "did:wba:awiki.ai:services:message:e1",
        &prepared,
        &slot,
    )
    .expect("e2ee commit params");
    assert_eq!(commit["body"]["object_encryption_mode"], "object-e2ee");
    assert_eq!(
        commit["body"]["plaintext_size"],
        prepared.plaintext_size_string.as_deref().unwrap()
    );
    assert_eq!(commit["body"].get("object_key_b64u"), None);
    assert_eq!(commit["body"].get("nonce_b64u"), None);
}

#[test]
fn attachment_wire_maps_validation_to_core_errors() {
    let prepared = awiki_im_core::compat::attachments::PreparedAttachment::default();
    let err = awiki_im_core::compat::attachments::build_attachment_create_slot_rpc_params(
        "did:wba:awiki.ai:user:alice:e1_alice",
        "did:wba:awiki.ai:services:message:e1",
        "agent",
        "did:wba:awiki.ai:user:bob:e1_bob",
        &prepared,
    )
    .expect_err("empty prepared attachment should fail");

    assert!(matches!(
        err,
        ImError::InvalidInput { field: Some(field), message }
            if field == "file_path" && message == "attachment file path is required"
    ));

    let selection = awiki_im_core::compat::attachments::AttachmentSelection::default();
    let err = awiki_im_core::compat::attachments::build_attachment_download_ticket_rpc_params(
        "did:wba:awiki.ai:user:alice:e1_alice",
        "did:wba:awiki.ai",
        "did:wba:awiki.ai:user:bob:e1_bob",
        "msg-1",
        "",
        &selection,
    )
    .expect_err("missing attachment id should fail");

    assert!(matches!(
        err,
        ImError::InvalidInput { field: Some(field), message }
            if field == "attachment_id"
                && message == awiki_im_core::compat::attachments::ERR_ATTACHMENT_NOT_FOUND
    ));
}

fn test_core() -> ImCore {
    ImCore::new(
        test_config_with_base_url("https://example.test"),
        test_paths(),
    )
    .unwrap()
}

fn test_core_with_base_url_and_ready_identity(base_url: &str) -> ImCore {
    test_core_with_base_url_ready_identity_and_service_did(base_url, "").0
}

fn test_core_with_base_url_ready_identity_and_service_did(
    base_url: &str,
    service_did: &str,
) -> (ImCore, ImCorePaths) {
    let paths = test_paths();
    write_ready_identity(&paths.identities.identity_root_dir, "alice");
    fs::write(
        paths
            .identities
            .default_identity_path
            .as_ref()
            .expect("default identity path"),
        "alice\n",
    )
    .unwrap();
    fs::write(
        &paths.identities.registry_path,
        r#"{
          "default_identity": "alice",
          "identities": [{
            "id": "alice-id",
            "did": "did:example:alice",
            "handle": "alice.awiki.info",
            "display_name": "Alice",
            "local_alias": "alice",
            "ready_for_auth": true,
            "ready_for_messaging": true,
            "missing": []
          }]
        }"#,
    )
    .unwrap();
    let mut config = test_config_with_base_url(base_url);
    config.anp_service_did =
        (!service_did.trim().is_empty()).then(|| Did::parse(service_did).unwrap());
    let core = ImCore::new(config, paths.clone()).unwrap();
    (core, paths)
}

fn local_message_rows(paths: &ImCorePaths, statement: &str) -> Vec<Value> {
    let db = rusqlite::Connection::open(&paths.local_state.sqlite_path).unwrap();
    awiki_im_core::compat::local_state::ensure_schema(&db).unwrap();
    let mut statement = db.prepare(statement).unwrap();
    let names = statement
        .column_names()
        .into_iter()
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    statement
        .query_map([], |row| {
            let mut object = serde_json::Map::new();
            for (index, name) in names.iter().enumerate() {
                object.insert(name.clone(), sqlite_value_to_json(row.get_ref(index)?));
            }
            Ok(Value::Object(object))
        })
        .unwrap()
        .map(|row| row.unwrap())
        .collect()
}

#[derive(Debug, PartialEq, Eq)]
struct DirectBindingSnapshot {
    peer_persona_id: String,
    conversation_id: String,
    full_handle: String,
    current_did: String,
    binding_generation: Option<String>,
}

fn direct_binding_snapshot(paths: &ImCorePaths, conversation_id: &str) -> DirectBindingSnapshot {
    let db = rusqlite::Connection::open(&paths.local_state.sqlite_path).unwrap();
    db.query_row(
        r#"SELECT p.peer_persona_id, r.conversation_id, p.full_handle,
                  r.current_did, p.binding_generation
FROM peer_personas p
JOIN direct_peer_routes r
  ON r.owner_identity_id = p.owner_identity_id
 AND r.peer_persona_id = p.peer_persona_id
WHERE p.owner_identity_id = 'alice-id' AND r.conversation_id = ?1"#,
        [conversation_id],
        |row| {
            Ok(DirectBindingSnapshot {
                peer_persona_id: row.get(0)?,
                conversation_id: row.get(1)?,
                full_handle: row.get(2)?,
                current_did: row.get(3)?,
                binding_generation: row.get(4)?,
            })
        },
    )
    .unwrap()
}

fn seed_active_group_conversation(paths: &ImCorePaths, group_did: &str) {
    fs::create_dir_all(
        paths
            .local_state
            .sqlite_path
            .parent()
            .expect("local state database parent"),
    )
    .unwrap();
    let db = rusqlite::Connection::open(&paths.local_state.sqlite_path).unwrap();
    awiki_im_core::compat::local_state::ensure_schema(&db).unwrap();
    db.execute(
        r#"
INSERT INTO groups
    (owner_identity_id, owner_did, group_id, group_did, membership_status, stored_at, metadata)
VALUES ('alice-id', 'did:example:alice', ?1, ?1, 'active', '2026-07-14T00:00:00Z', '{}')
"#,
        [group_did],
    )
    .unwrap();
}

fn sqlite_value_to_json(value: rusqlite::types::ValueRef<'_>) -> Value {
    match value {
        rusqlite::types::ValueRef::Null => Value::Null,
        rusqlite::types::ValueRef::Integer(value) => json!(value),
        rusqlite::types::ValueRef::Real(value) => json!(value),
        rusqlite::types::ValueRef::Text(value) => {
            Value::String(String::from_utf8_lossy(value).into_owned())
        }
        rusqlite::types::ValueRef::Blob(value) => {
            Value::Array(value.iter().copied().map(|byte| json!(byte)).collect())
        }
    }
}

fn write_ready_identity(identities: &Path, alias: &str) {
    let identity_dir = identities.join(alias);
    fs::create_dir_all(&identity_dir).unwrap();
    let bundle = anp::authentication::create_did_wba_document(
        "awiki.info",
        anp::authentication::DidDocumentOptions {
            path_segments: vec!["user".to_string()],
            domain: Some("awiki.info".to_string()),
            challenge: Some(format!("attachment-api-{alias}")),
            ..anp::authentication::DidDocumentOptions::default()
        },
    )
    .unwrap();
    fs::write(
        identity_dir.join("did.json"),
        serde_json::to_vec_pretty(&bundle.did_document).unwrap(),
    )
    .unwrap();
    fs::write(
        identity_dir.join("private.key"),
        bundle.private_key_pem("key-1").unwrap(),
    )
    .unwrap();
    fs::write(
        identity_dir.join("auth.json"),
        format!(r#"{{"jwt_token":"test-token-for-{alias}"}}"#),
    )
    .unwrap();
}

fn generated_attachment_wire_identity(
) -> awiki_im_core::compat::attachments::AttachmentSigningIdentity {
    use anp::authentication::{create_did_wba_document, DidDocumentOptions};

    let bundle = create_did_wba_document(
        "awiki.ai",
        DidDocumentOptions {
            path_segments: vec!["user".to_string()],
            domain: Some("awiki.ai".to_string()),
            challenge: Some("attachment-wire-test".to_string()),
            ..DidDocumentOptions::default()
        },
    )
    .expect("generated did document");
    let key1_private_pem = bundle
        .private_key_pem("key-1")
        .expect("key-1 private pem")
        .to_string();
    awiki_im_core::compat::attachments::AttachmentSigningIdentity {
        identity_name: "alice".to_string(),
        did: bundle.did().expect("generated did").to_string(),
        did_document: Some(bundle.did_document),
        key1_private_pem,
    }
}

fn test_config_with_base_url(base_url: &str) -> ImCoreConfig {
    ImCoreConfig {
        service_base_url: ServiceEndpoint::parse(base_url).unwrap(),
        did_domain: "awiki.info".to_string(),
        client_version_info: None,
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
        "im-core-attachment-api-{}-{nanos}",
        std::process::id()
    ))
}

fn handle_lookup_result() -> Value {
    json!({
        "handle": "bob",
        "full_handle": "bob.awiki.info",
        "did": "did:example:bob",
        "user_id": "user-bob",
        "domain": "awiki.info",
        "status": "active",
    })
}

const REBIND_HANDLE: &str = "bob.awiki.info";
const REBIND_OLD_DID: &str = "did:wba:awiki.info:user:bob:e1-old";
const REBIND_NEW_DID: &str = "did:wba:awiki.info:user:bob:e1-new";

fn rebind_directory_lookup(did: &str) -> Value {
    json!({
        "handle": "bob",
        "full_handle": REBIND_HANDLE,
        "did": did,
        "user_id": "user-bob",
        "domain": "awiki.info",
        "status": "active"
    })
}

fn rebind_public_binding(did: &str, generation: &str) -> Value {
    json!({
        "status": "active",
        "handle": REBIND_HANDLE,
        "did": did,
        "binding_generation": generation
    })
}

struct AttachmentServiceTestServer {
    base_url: String,
    handle: thread::JoinHandle<Vec<CapturedHttp>>,
}

impl AttachmentServiceTestServer {
    fn spawn(responses: Vec<ExpectedHttp>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let handle_base_url = base_url.clone();
        let handle = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut captured = Vec::new();
            for response in responses {
                let mut stream = accept_before_deadline(&listener, deadline);
                let request = read_http_request(&mut stream);
                write_http_response(
                    &mut stream,
                    response
                        .replace_base(&handle_base_url)
                        .bind_attachment_id(&request),
                );
                captured.push(request);
            }
            captured
        });
        Self { base_url, handle }
    }

    fn base_url(&self) -> &str {
        &self.base_url
    }

    fn join(self) -> Vec<CapturedHttp> {
        self.handle.join().unwrap()
    }
}

struct ExpectedHttp {
    status_code: u16,
    content_type: String,
    body: Vec<u8>,
}

impl ExpectedHttp {
    fn json(body: Value) -> Self {
        Self {
            status_code: 200,
            content_type: "application/json".to_string(),
            body: body.to_string().into_bytes(),
        }
    }

    fn rpc_result(result: Value) -> Self {
        Self::json(json!({
            "jsonrpc": "2.0",
            "id": "req-1",
            "result": result,
        }))
    }

    fn stale_binding_error() -> Self {
        Self::json(json!({
            "jsonrpc": "2.0",
            "id": "req-1",
            "error": {
                "code": 1406,
                "message": "target binding is stale",
                "data": {
                    "anp_code": "anp.invalid_target_binding",
                    "reason": "stale_did",
                    "current_did": REBIND_NEW_DID,
                    "full_handle": REBIND_HANDLE
                }
            }
        }))
    }

    fn replace_base(mut self, base_url: &str) -> Self {
        let body = String::from_utf8(self.body.clone())
            .ok()
            .map(|body| body.replace("__BASE__", base_url).as_bytes().to_vec());
        if let Some(body) = body {
            self.body = body;
        }
        self
    }

    fn bind_attachment_id(mut self, request: &CapturedHttp) -> Self {
        if !matches!(
            request.rpc_method().as_deref(),
            Some("attachment.create_slot" | "attachment.commit_object")
        ) {
            return self;
        }
        let Some(attachment_id) = request.params()["body"]["attachment_id"]
            .as_str()
            .map(ToOwned::to_owned)
        else {
            return self;
        };
        let Ok(mut envelope) = serde_json::from_slice::<Value>(&self.body) else {
            return self;
        };
        envelope["result"]["attachment_id"] = Value::String(attachment_id);
        self.body = envelope.to_string().into_bytes();
        self
    }
}

#[derive(Debug)]
struct CapturedHttp {
    method: String,
    path: String,
    body: Vec<u8>,
}

impl CapturedHttp {
    fn json_body(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }

    fn rpc_method(&self) -> Option<String> {
        self.json_body()
            .get("method")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
    }

    fn params(&self) -> Value {
        self.json_body()
            .get("params")
            .cloned()
            .unwrap_or(Value::Null)
    }
}

fn accept_before_deadline(listener: &TcpListener, deadline: Instant) -> TcpStream {
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).unwrap();
                return stream;
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < deadline, "timed out waiting for request");
                thread::sleep(Duration::from_millis(10));
            }
            Err(err) => panic!("accept request: {err}"),
        }
    }
}

fn read_http_request(stream: &mut TcpStream) -> CapturedHttp {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut raw = Vec::new();
    let mut buffer = [0_u8; 4096];
    let header_end = loop {
        let count = stream.read(&mut buffer).unwrap();
        assert!(count > 0, "request closed before headers");
        raw.extend_from_slice(&buffer[..count]);
        if let Some(index) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
            break index;
        }
    };
    let headers_text = std::str::from_utf8(&raw[..header_end]).unwrap();
    let mut lines = headers_text.lines();
    let request_line = lines.next().unwrap();
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().unwrap().to_string();
    let path = request_parts.next().unwrap().to_string();
    let headers = lines
        .filter_map(|line| {
            let (key, value) = line.split_once(':')?;
            Some((key.trim().to_ascii_lowercase(), value.trim().to_string()))
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let content_length = headers
        .get("content-length")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let body_start = header_end + 4;
    while raw.len() < body_start + content_length {
        let count = stream.read(&mut buffer).unwrap();
        assert!(count > 0, "request closed before body");
        raw.extend_from_slice(&buffer[..count]);
    }
    CapturedHttp {
        method,
        path,
        body: raw[body_start..body_start + content_length].to_vec(),
    }
}

fn write_http_response(stream: &mut TcpStream, response: ExpectedHttp) {
    write!(
        stream,
        "HTTP/1.1 {} OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        response.status_code,
        response.content_type,
        response.body.len()
    )
    .unwrap();
    stream.write_all(&response.body).unwrap();
}
