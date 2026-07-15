use super::*;
use crate::internal::transport::{
    AttachmentObjectResponse, AttachmentObjectTransport, AuthenticatedRpcTransport,
    RawJsonTransport, RpcTransport,
};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;

#[test]
fn attachments_download_runtime_memory_fetches_ticket_and_bytes() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let sessions = Rc::new(RefCell::new(Vec::new()));

    let result = AttachmentDownloadRuntime::new(
        &client,
        ReadySessionProvider {
            scopes: Rc::clone(&sessions),
        },
        RecordingTransport {
            calls: Rc::clone(&calls),
        },
    )
    .download(AttachmentDownloadInput {
        request: crate::attachments::DownloadAttachmentRequest {
            thread: crate::messages::ThreadRef::Direct(
                crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
            ),
            message_id: crate::ids::MessageId::parse("msg-attachment-1").unwrap(),
            attachment_id: Some("att-1".to_string()),
            destination: crate::attachments::AttachmentDestination::Memory,
            overwrite: false,
        },
        resolved_peer_did: None,
    })
    .unwrap();

    assert_eq!(
        sessions.borrow().as_slice(),
        &[crate::auth::AuthScope::Messaging]
    );
    assert_eq!(result.selection.attachment_id, "att-1");
    assert_eq!(result.ticket.download_ticket_b64u, "ticket-1");
    assert_eq!(result.sdk_result.attachment_id, "att-1");
    assert_eq!(result.sdk_result.filename.as_deref(), Some("report.txt"));
    assert_eq!(result.sdk_result.mime_type.as_deref(), Some("text/plain"));
    assert_eq!(result.sdk_result.size_bytes, Some(16));
    assert!(matches!(
        result.sdk_result.destination,
        crate::attachments::DownloadedAttachmentDestination::Memory(bytes)
            if bytes == b"downloaded bytes".to_vec()
    ));

    let calls = calls.borrow();
    assert_eq!(calls.len(), 4);
    let history = calls[0].rpc("direct.get_history");
    assert_eq!(history.endpoint, MESSAGE_RPC_ENDPOINT);
    assert_eq!(history.params["body"]["peer_did"], "did:example:bob");
    assert_eq!(
        history.params["body"]["limit"],
        ATTACHMENT_DOWNLOAD_LOOKUP_PAGE_SIZE
    );
    let did_doc = calls[1].get_json("https://example.com/bob/did.json");
    assert_eq!(
        did_doc.headers.get("Accept").map(String::as_str),
        Some("application/json")
    );
    let ticket = calls[2].rpc("attachment.get_download_ticket");
    assert_eq!(ticket.endpoint, "https://attachment.example/rpc");
    assert_eq!(
        ticket.params["meta"]["target"],
        json!({"kind": "service", "did": "did:web:attachment.example"})
    );
    assert_eq!(ticket.params["body"]["attachment_id"], "att-1");
    assert_eq!(
        ticket.params["body"]["object_uri"],
        "https://objects.example/att-1"
    );
    assert_eq!(
        ticket.params["body"]["sender_did"],
        "did:web:example.com:bob"
    );
    assert_eq!(ticket.params["body"]["requester_did"], "did:example:alice");
    assert_eq!(
        ticket.params["body"]["message_target_did"],
        "did:example:alice"
    );
    let object = calls[3].object_get("https://objects.example/att-1");
    assert_eq!(object.ticket, "ticket-1");
}

#[test]
fn attachments_download_runtime_pages_until_selection_matches() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));

    let result = AttachmentDownloadRuntime::new(
        &client,
        ReadySessionProvider {
            scopes: Rc::new(RefCell::new(Vec::new())),
        },
        RecordingTransport {
            calls: Rc::clone(&calls),
        },
    )
    .download(AttachmentDownloadInput {
        request: crate::attachments::DownloadAttachmentRequest {
            thread: crate::messages::ThreadRef::Direct(
                crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
            ),
            message_id: crate::ids::MessageId::parse("msg-second-page").unwrap(),
            attachment_id: Some("att-2".to_string()),
            destination: crate::attachments::AttachmentDestination::Memory,
            overwrite: false,
        },
        resolved_peer_did: None,
    })
    .unwrap();

    assert_eq!(result.selection.attachment_id, "att-2");
    let calls = calls.borrow();
    let first = calls[0].rpc("direct.get_history");
    assert_eq!(first.params["body"].get("skip"), None);
    let second = calls[1].rpc("direct.get_history");
    assert_eq!(second.params["body"]["skip"], 1);
}

#[test]
fn attachments_download_runtime_group_uses_group_scope_and_ticket_body() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let sessions = Rc::new(RefCell::new(Vec::new()));

    let result = AttachmentDownloadRuntime::new(
        &client,
        ReadySessionProvider {
            scopes: Rc::clone(&sessions),
        },
        RecordingTransport {
            calls: Rc::clone(&calls),
        },
    )
    .download(AttachmentDownloadInput {
        request: crate::attachments::DownloadAttachmentRequest {
            thread: crate::messages::ThreadRef::Group(
                crate::ids::GroupRef::parse("did:example:group").unwrap(),
            ),
            message_id: crate::ids::MessageId::parse("group-msg-1").unwrap(),
            attachment_id: None,
            destination: crate::attachments::AttachmentDestination::Memory,
            overwrite: false,
        },
        resolved_peer_did: None,
    })
    .unwrap();

    assert_eq!(
        sessions.borrow().as_slice(),
        &[crate::auth::AuthScope::GroupMessaging]
    );
    assert_eq!(result.selection.attachment_id, "att-group-1");
    let calls = calls.borrow();
    let list = calls[0].rpc("group.list_messages");
    assert_eq!(list.params["body"]["group_did"], "did:example:group");
    let ticket = calls[2].rpc("attachment.get_download_ticket");
    assert_eq!(ticket.params["body"]["group_did"], "did:example:group");
    assert_eq!(ticket.params["body"].get("message_target_did"), None);
}

#[test]
fn attachments_download_runtime_local_file_destination_writes_file() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let output = fixture.root.join("downloads").join("report.txt");
    fs::create_dir_all(output.parent().unwrap()).unwrap();

    let result = AttachmentDownloadRuntime::new(
        &client,
        ReadySessionProvider {
            scopes: Rc::new(RefCell::new(Vec::new())),
        },
        RecordingTransport {
            calls: Rc::clone(&calls),
        },
    )
    .download(AttachmentDownloadInput {
        request: crate::attachments::DownloadAttachmentRequest {
            thread: crate::messages::ThreadRef::Direct(
                crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
            ),
            message_id: crate::ids::MessageId::parse("msg-attachment-1").unwrap(),
            attachment_id: Some("att-1".to_string()),
            destination: crate::attachments::AttachmentDestination::LocalFile(output.clone()),
            overwrite: false,
        },
        resolved_peer_did: None,
    })
    .unwrap();

    assert!(matches!(
        result.sdk_result.destination,
        crate::attachments::DownloadedAttachmentDestination::LocalFile(path)
            if path == output
    ));
    assert_eq!(fs::read(&output).unwrap(), b"downloaded bytes");
    assert_no_attachment_temp_files(output.parent().unwrap());
    assert_eq!(calls.borrow().len(), 4);
}

#[tokio::test]
async fn attachments_download_runtime_local_file_async_streams_to_file() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let output = fixture.root.join("downloads").join("report-async.txt");
    fs::create_dir_all(output.parent().unwrap()).unwrap();

    let result = AttachmentDownloadRuntime::new(
        &client,
        ReadySessionProvider {
            scopes: Rc::new(RefCell::new(Vec::new())),
        },
        RecordingTransport {
            calls: Rc::clone(&calls),
        },
    )
    .download_async(AttachmentDownloadInput {
        request: crate::attachments::DownloadAttachmentRequest {
            thread: crate::messages::ThreadRef::Direct(
                crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
            ),
            message_id: crate::ids::MessageId::parse("msg-attachment-1").unwrap(),
            attachment_id: Some("att-1".to_string()),
            destination: crate::attachments::AttachmentDestination::LocalFile(output.clone()),
            overwrite: false,
        },
        resolved_peer_did: None,
    })
    .await
    .unwrap();

    assert!(matches!(
        result.sdk_result.destination,
        crate::attachments::DownloadedAttachmentDestination::LocalFile(path)
            if path == output
    ));
    assert_eq!(fs::read(&output).unwrap(), b"downloaded bytes");
    assert_no_attachment_temp_files(output.parent().unwrap());
    let calls = calls.borrow();
    assert_eq!(calls.len(), 4);
    let object = calls[3].object_get_stream("https://objects.example/att-1");
    assert_eq!(object.ticket, "ticket-1");
}

#[test]
fn attachments_download_runtime_local_file_rejects_existing_destination_without_network() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let output = fixture.root.join("downloads").join("report.txt");
    fs::create_dir_all(output.parent().unwrap()).unwrap();
    fs::write(&output, b"existing").unwrap();

    let err = AttachmentDownloadRuntime::new(
        &client,
        ReadySessionProvider {
            scopes: Rc::new(RefCell::new(Vec::new())),
        },
        RecordingTransport {
            calls: Rc::clone(&calls),
        },
    )
    .download(AttachmentDownloadInput {
        request: crate::attachments::DownloadAttachmentRequest {
            thread: crate::messages::ThreadRef::Direct(
                crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
            ),
            message_id: crate::ids::MessageId::parse("msg-attachment-1").unwrap(),
            attachment_id: Some("att-1".to_string()),
            destination: crate::attachments::AttachmentDestination::LocalFile(output.clone()),
            overwrite: false,
        },
        resolved_peer_did: None,
    })
    .unwrap_err();

    assert!(matches!(
        err,
        crate::ImError::InvalidInput { field: Some(field), message }
            if field == "destination" && message.contains("overwrite is false")
    ));
    assert_eq!(fs::read(&output).unwrap(), b"existing");
    assert_no_attachment_temp_files(output.parent().unwrap());
    assert!(calls.borrow().is_empty());
}

#[test]
fn attachments_download_runtime_falls_back_to_local_identity_document_for_sender_service() {
    let fixture = Fixture::new();
    fixture.write_attachment_service_document(
        "sender",
        "did:web:example:alice",
        "https://local-attachment.example/rpc",
        "did:example:local-message-service",
    );
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));

    let result = AttachmentDownloadRuntime::new(
        &client,
        ReadySessionProvider {
            scopes: Rc::new(RefCell::new(Vec::new())),
        },
        LocalFallbackTransport {
            calls: Rc::clone(&calls),
        },
    )
    .download(AttachmentDownloadInput {
        request: crate::attachments::DownloadAttachmentRequest {
            thread: crate::messages::ThreadRef::Direct(
                crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
            ),
            message_id: crate::ids::MessageId::parse("msg-local-sender").unwrap(),
            attachment_id: Some("att-local".to_string()),
            destination: crate::attachments::AttachmentDestination::Memory,
            overwrite: false,
        },
        resolved_peer_did: None,
    })
    .unwrap();

    assert_eq!(result.selection.sender_did, "did:web:example:alice");
    assert_eq!(result.ticket.download_ticket_b64u, "ticket-local");
    assert!(matches!(
        result.sdk_result.destination,
        crate::attachments::DownloadedAttachmentDestination::Memory(bytes)
            if bytes == b"local document bytes".to_vec()
    ));

    let calls = calls.borrow();
    assert_eq!(calls.len(), 4);
    let history = calls[0].rpc("direct.get_history");
    assert_eq!(history.params["body"]["peer_did"], "did:example:bob");
    calls[1].get_json("https://example/alice/did.json");
    let ticket = calls[2].rpc("attachment.get_download_ticket");
    assert_eq!(ticket.endpoint, "https://local-attachment.example/rpc");
    assert_eq!(
        ticket.params["meta"]["target"],
        json!({"kind": "service", "did": "did:example:local-message-service"})
    );
    assert_eq!(ticket.params["body"]["sender_did"], "did:web:example:alice");
    let object = calls[3].object_get("https://objects.example/att-local");
    assert_eq!(object.ticket, "ticket-local");
}

#[test]
fn attachments_download_runtime_object_e2ee_memory_returns_plaintext_and_redacts_selection() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let object = object_e2ee_case(b"e2ee plaintext".to_vec());
    let calls = Rc::new(RefCell::new(Vec::new()));

    let result = AttachmentDownloadRuntime::new(
        &client,
        ReadySessionProvider {
            scopes: Rc::new(RefCell::new(Vec::new())),
        },
        E2eeTransport {
            calls: Rc::clone(&calls),
            history: e2ee_history_response_with_legacy_missing_profile(
                object.full_manifest.clone(),
            ),
            object_body: object.ciphertext.clone(),
            object_content_type: Some("application/octet-stream".to_string()),
        },
    )
    .download(AttachmentDownloadInput {
        request: crate::attachments::DownloadAttachmentRequest {
            thread: crate::messages::ThreadRef::Direct(
                crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
            ),
            message_id: crate::ids::MessageId::parse("msg-e2ee-1").unwrap(),
            attachment_id: Some("att-e2ee-1".to_string()),
            destination: crate::attachments::AttachmentDestination::Memory,
            overwrite: false,
        },
        resolved_peer_did: None,
    })
    .unwrap();

    assert_eq!(result.selection.attachment_id, "att-e2ee-1");
    assert_eq!(result.selection.message_security_profile, "direct-e2ee");
    assert_eq!(
        result
            .sdk_result
            .selection
            .as_ref()
            .unwrap()
            .message_security_profile,
        "direct-e2ee"
    );
    assert_eq!(result.selection.object_encryption_mode, "object-e2ee");
    assert_eq!(
        result.selection.object_cipher.as_deref(),
        Some("chacha20-poly1305")
    );
    let expected_plaintext_size = object.plaintext.len().to_string();
    assert_eq!(
        result.selection.plaintext_size.as_deref(),
        Some(expected_plaintext_size.as_str())
    );
    assert_eq!(
        result.sdk_result.size_bytes,
        Some(object.plaintext.len() as u64)
    );
    assert!(matches!(
        result.sdk_result.destination,
        crate::attachments::DownloadedAttachmentDestination::Memory(bytes)
            if bytes == object.plaintext
    ));

    let public_selection = serde_json::to_string(&result.selection).unwrap();
    assert!(!public_selection.contains("object_key_b64u"));
    assert!(!public_selection.contains("nonce_b64u"));
    assert!(!public_selection.contains(&object.object_key_b64u));
    assert!(!public_selection.contains(&object.nonce_b64u));
    let sdk_selection = serde_json::to_string(&result.sdk_result.selection).unwrap();
    assert!(!sdk_selection.contains("object_key_b64u"));
    assert!(!sdk_selection.contains("nonce_b64u"));
    assert!(!sdk_selection.contains(&object.object_key_b64u));
    assert!(!sdk_selection.contains(&object.nonce_b64u));

    let calls = calls.borrow();
    assert_eq!(calls.len(), 4);
    let ticket = calls[2].rpc("attachment.get_download_ticket");
    assert_eq!(
        ticket.params["body"]["message_security_profile"],
        "direct-e2ee"
    );
    assert_eq!(
        ticket.params["body"]["message_target_did"],
        "did:example:alice"
    );
    assert_eq!(ticket.params["body"].get("group_did"), None);
    assert_eq!(
        ticket.params["body"].get("object_key_b64u"),
        None,
        "download ticket body must not expose object key"
    );
    assert_eq!(
        ticket.params["body"].get("nonce_b64u"),
        None,
        "download ticket body must not expose nonce"
    );
    calls[3].object_get("https://objects.example/att-e2ee-1");
}

#[test]
fn attachments_download_runtime_group_object_e2ee_uses_internal_manifest_cache() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let object = object_e2ee_case(b"group cached plaintext".to_vec());
    {
        let connection = crate::internal::local_state::open_writable(
            &client.core_inner().sdk_paths().local_state.sqlite_path,
        )
        .unwrap();
        crate::internal::local_state::attachment_manifest_cache::upsert_attachment_manifest_cache(
            &connection,
            &crate::internal::local_state::attachment_manifest_cache::AttachmentManifestCacheRecord {
                owner_identity_id: client.current_identity().id.as_str().to_owned(),
                owner_did: client.did().as_str().to_owned(),
                thread_kind: "group".to_owned(),
                thread_id: "did:example:group:e2ee".to_owned(),
                message_id: "did:example:group:e2ee:7".to_owned(),
                sender_did: "did:web:example.com:bob".to_owned(),
                message_security_profile: "group-e2ee".to_owned(),
                content: serde_json::to_string(&object.full_manifest).unwrap(),
                stored_at: "2026-06-02T00:00:00Z".to_owned(),
            },
        )
        .unwrap();
    }
    let calls = Rc::new(RefCell::new(Vec::new()));

    let result = AttachmentDownloadRuntime::new(
        &client,
        ReadySessionProvider {
            scopes: Rc::new(RefCell::new(Vec::new())),
        },
        E2eeTransport {
            calls: Rc::clone(&calls),
            history: json!({
                "messages": [],
                "has_more": false
            }),
            object_body: object.ciphertext.clone(),
            object_content_type: Some("application/octet-stream".to_string()),
        },
    )
    .download(AttachmentDownloadInput {
        request: crate::attachments::DownloadAttachmentRequest {
            thread: crate::messages::ThreadRef::Group(
                crate::ids::GroupRef::parse("did:example:group:e2ee").unwrap(),
            ),
            message_id: crate::ids::MessageId::parse("did:example:group:e2ee:7").unwrap(),
            attachment_id: Some("att-e2ee-1".to_string()),
            destination: crate::attachments::AttachmentDestination::Memory,
            overwrite: false,
        },
        resolved_peer_did: None,
    })
    .unwrap();

    assert_eq!(result.selection.message_security_profile, "group-e2ee");
    assert_eq!(result.selection.object_encryption_mode, "object-e2ee");
    assert!(matches!(
        result.sdk_result.destination,
        crate::attachments::DownloadedAttachmentDestination::Memory(bytes)
            if bytes == object.plaintext
    ));
    let public_selection = serde_json::to_string(&result.selection).unwrap();
    assert!(!public_selection.contains("object_key_b64u"));
    assert!(!public_selection.contains("nonce_b64u"));
    assert!(!public_selection.contains(&object.object_key_b64u));
    assert!(!public_selection.contains(&object.nonce_b64u));

    let calls = calls.borrow();
    assert_eq!(calls.len(), 3);
    calls[0].get_json("https://example.com/bob/did.json");
    let ticket = calls[1].rpc("attachment.get_download_ticket");
    assert_eq!(
        ticket.params["body"]["message_security_profile"],
        "group-e2ee"
    );
    assert_eq!(ticket.params["body"]["group_did"], "did:example:group:e2ee");
    assert_eq!(ticket.params["body"].get("object_key_b64u"), None);
    assert_eq!(ticket.params["body"].get("nonce_b64u"), None);
    calls[2].object_get("https://objects.example/att-e2ee-1");
}

#[test]
fn attachments_download_runtime_direct_object_e2ee_uses_internal_manifest_cache() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let object = object_e2ee_case(b"direct cached plaintext".to_vec());
    {
        let connection = crate::internal::local_state::open_writable(
            &client.core_inner().sdk_paths().local_state.sqlite_path,
        )
        .unwrap();
        crate::internal::local_state::attachment_manifest_cache::upsert_attachment_manifest_cache(
            &connection,
            &crate::internal::local_state::attachment_manifest_cache::AttachmentManifestCacheRecord {
                owner_identity_id: client.current_identity().id.as_str().to_owned(),
                owner_did: client.did().as_str().to_owned(),
                thread_kind: "direct".to_owned(),
                thread_id: "did:web:example.com:bob".to_owned(),
                message_id: "msg-direct-e2ee-7".to_owned(),
                sender_did: "did:web:example.com:bob".to_owned(),
                message_security_profile: "direct-e2ee".to_owned(),
                content: serde_json::to_string(&object.full_manifest).unwrap(),
                stored_at: "2026-06-02T00:00:00Z".to_owned(),
            },
        )
        .unwrap();
    }
    let calls = Rc::new(RefCell::new(Vec::new()));

    let result = AttachmentDownloadRuntime::new(
        &client,
        ReadySessionProvider {
            scopes: Rc::new(RefCell::new(Vec::new())),
        },
        E2eeTransport {
            calls: Rc::clone(&calls),
            history: json!({
                "messages": [],
                "has_more": false
            }),
            object_body: object.ciphertext.clone(),
            object_content_type: Some("application/octet-stream".to_string()),
        },
    )
    .download(AttachmentDownloadInput {
        request: crate::attachments::DownloadAttachmentRequest {
            thread: crate::messages::ThreadRef::Direct(
                crate::ids::PeerRef::parse("did:web:example.com:bob", "").unwrap(),
            ),
            message_id: crate::ids::MessageId::parse("msg-direct-e2ee-7").unwrap(),
            attachment_id: Some("att-e2ee-1".to_string()),
            destination: crate::attachments::AttachmentDestination::Memory,
            overwrite: false,
        },
        resolved_peer_did: Some("did:web:example.com:bob".to_string()),
    })
    .unwrap();

    assert_eq!(result.selection.message_security_profile, "direct-e2ee");
    assert_eq!(result.selection.object_encryption_mode, "object-e2ee");
    assert!(matches!(
        result.sdk_result.destination,
        crate::attachments::DownloadedAttachmentDestination::Memory(bytes)
            if bytes == object.plaintext
    ));
    let public_selection = serde_json::to_string(&result.selection).unwrap();
    assert!(!public_selection.contains("object_key_b64u"));
    assert!(!public_selection.contains("nonce_b64u"));
    assert!(!public_selection.contains(&object.object_key_b64u));
    assert!(!public_selection.contains(&object.nonce_b64u));

    let calls = calls.borrow();
    assert_eq!(calls.len(), 3, "cached manifest must avoid history replay");
    calls[0].get_json("https://example.com/bob/did.json");
    let ticket = calls[1].rpc("attachment.get_download_ticket");
    assert_eq!(
        ticket.params["body"]["message_security_profile"],
        "direct-e2ee"
    );
    assert_eq!(ticket.params["body"].get("group_did"), None);
    assert_eq!(ticket.params["body"].get("object_key_b64u"), None);
    assert_eq!(ticket.params["body"].get("nonce_b64u"), None);
    calls[2].object_get("https://objects.example/att-e2ee-1");
}

#[test]
fn attachments_download_runtime_object_e2ee_local_file_writes_plaintext() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let object = object_e2ee_case(b"local e2ee plaintext".to_vec());
    let calls = Rc::new(RefCell::new(Vec::new()));
    let output = fixture.root.join("downloads").join("secret.pdf");
    fs::create_dir_all(output.parent().unwrap()).unwrap();

    let result = AttachmentDownloadRuntime::new(
        &client,
        ReadySessionProvider {
            scopes: Rc::new(RefCell::new(Vec::new())),
        },
        E2eeTransport {
            calls: Rc::clone(&calls),
            history: e2ee_history_response(object.full_manifest.clone(), "direct-e2ee"),
            object_body: object.ciphertext.clone(),
            object_content_type: Some("application/octet-stream".to_string()),
        },
    )
    .download(AttachmentDownloadInput {
        request: crate::attachments::DownloadAttachmentRequest {
            thread: crate::messages::ThreadRef::Direct(
                crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
            ),
            message_id: crate::ids::MessageId::parse("msg-e2ee-1").unwrap(),
            attachment_id: Some("att-e2ee-1".to_string()),
            destination: crate::attachments::AttachmentDestination::LocalFile(output.clone()),
            overwrite: false,
        },
        resolved_peer_did: None,
    })
    .unwrap();

    assert!(matches!(
        result.sdk_result.destination,
        crate::attachments::DownloadedAttachmentDestination::LocalFile(path)
            if path == output
    ));
    assert_eq!(fs::read(&output).unwrap(), object.plaintext);
    assert_no_attachment_temp_files(output.parent().unwrap());
    assert_eq!(calls.borrow().len(), 4);
}

#[tokio::test]
async fn attachments_download_runtime_object_e2ee_local_file_async_writes_plaintext() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let object = object_e2ee_case(b"async local e2ee plaintext".to_vec());
    let calls = Rc::new(RefCell::new(Vec::new()));
    let output = fixture.root.join("downloads").join("secret-async.pdf");
    fs::create_dir_all(output.parent().unwrap()).unwrap();

    let result = AttachmentDownloadRuntime::new(
        &client,
        ReadySessionProvider {
            scopes: Rc::new(RefCell::new(Vec::new())),
        },
        E2eeTransport {
            calls: Rc::clone(&calls),
            history: e2ee_history_response(object.full_manifest.clone(), "direct-e2ee"),
            object_body: object.ciphertext.clone(),
            object_content_type: Some("application/octet-stream".to_string()),
        },
    )
    .download_async(AttachmentDownloadInput {
        request: crate::attachments::DownloadAttachmentRequest {
            thread: crate::messages::ThreadRef::Direct(
                crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
            ),
            message_id: crate::ids::MessageId::parse("msg-e2ee-1").unwrap(),
            attachment_id: Some("att-e2ee-1".to_string()),
            destination: crate::attachments::AttachmentDestination::LocalFile(output.clone()),
            overwrite: false,
        },
        resolved_peer_did: None,
    })
    .await
    .unwrap();

    assert!(matches!(
        result.sdk_result.destination,
        crate::attachments::DownloadedAttachmentDestination::LocalFile(path)
            if path == output
    ));
    assert_eq!(fs::read(&output).unwrap(), object.plaintext);
    assert_no_attachment_temp_files(output.parent().unwrap());
    let calls = calls.borrow();
    assert_eq!(calls.len(), 4);
    calls[3].object_get_stream("https://objects.example/att-e2ee-1");
}

#[test]
fn attachments_download_runtime_rejects_digest_mismatch_without_writing() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let mut object = object_e2ee_case(b"secret with bad digest".to_vec());
    object.full_manifest["attachments"][0]["digest"]["value_b64u"] =
        Value::String("wrong-digest".to_string());
    let calls = Rc::new(RefCell::new(Vec::new()));
    let output = fixture.root.join("downloads").join("bad-digest.pdf");
    fs::create_dir_all(output.parent().unwrap()).unwrap();

    let err = AttachmentDownloadRuntime::new(
        &client,
        ReadySessionProvider {
            scopes: Rc::new(RefCell::new(Vec::new())),
        },
        E2eeTransport {
            calls: Rc::clone(&calls),
            history: e2ee_history_response(object.full_manifest.clone(), "direct-e2ee"),
            object_body: object.ciphertext.clone(),
            object_content_type: None,
        },
    )
    .download(AttachmentDownloadInput {
        request: crate::attachments::DownloadAttachmentRequest {
            thread: crate::messages::ThreadRef::Direct(
                crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
            ),
            message_id: crate::ids::MessageId::parse("msg-e2ee-1").unwrap(),
            attachment_id: Some("att-e2ee-1".to_string()),
            destination: crate::attachments::AttachmentDestination::LocalFile(output.clone()),
            overwrite: false,
        },
        resolved_peer_did: None,
    })
    .unwrap_err();

    assert_service_error_code(err, "anp.attachment.digest_mismatch");
    assert!(!output.exists());
    assert_no_attachment_temp_files(output.parent().unwrap());
    assert_eq!(calls.borrow().len(), 4);
}

#[test]
fn attachments_download_runtime_rejects_wrong_object_key_without_writing() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let mut object = object_e2ee_case(b"secret with wrong key".to_vec());
    let other = object_e2ee_case(b"other secret".to_vec());
    object.full_manifest["attachments"][0]["encryption_info"]["object_key_b64u"] =
        Value::String(other.object_key_b64u);
    let calls = Rc::new(RefCell::new(Vec::new()));
    let output = fixture.root.join("downloads").join("wrong-key.pdf");
    fs::create_dir_all(output.parent().unwrap()).unwrap();

    let err = AttachmentDownloadRuntime::new(
        &client,
        ReadySessionProvider {
            scopes: Rc::new(RefCell::new(Vec::new())),
        },
        E2eeTransport {
            calls: Rc::clone(&calls),
            history: e2ee_history_response(object.full_manifest.clone(), "direct-e2ee"),
            object_body: object.ciphertext.clone(),
            object_content_type: None,
        },
    )
    .download(AttachmentDownloadInput {
        request: crate::attachments::DownloadAttachmentRequest {
            thread: crate::messages::ThreadRef::Direct(
                crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
            ),
            message_id: crate::ids::MessageId::parse("msg-e2ee-1").unwrap(),
            attachment_id: Some("att-e2ee-1".to_string()),
            destination: crate::attachments::AttachmentDestination::LocalFile(output.clone()),
            overwrite: false,
        },
        resolved_peer_did: None,
    })
    .unwrap_err();

    assert_service_error_code(err, "anp.attachment.decrypt_failed");
    assert!(!output.exists());
    assert_no_attachment_temp_files(output.parent().unwrap());
    assert_eq!(calls.borrow().len(), 4);
}

#[test]
fn attachments_download_runtime_rejects_plaintext_size_mismatch_without_writing() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let mut object = object_e2ee_case(b"secret with wrong plaintext size".to_vec());
    object.full_manifest["attachments"][0]["encryption_info"]["plaintext_size"] =
        Value::String((object.plaintext.len() + 1).to_string());
    let calls = Rc::new(RefCell::new(Vec::new()));
    let output = fixture.root.join("downloads").join("wrong-size.pdf");
    fs::create_dir_all(output.parent().unwrap()).unwrap();

    let err = AttachmentDownloadRuntime::new(
        &client,
        ReadySessionProvider {
            scopes: Rc::new(RefCell::new(Vec::new())),
        },
        E2eeTransport {
            calls: Rc::clone(&calls),
            history: e2ee_history_response(object.full_manifest.clone(), "direct-e2ee"),
            object_body: object.ciphertext.clone(),
            object_content_type: None,
        },
    )
    .download(AttachmentDownloadInput {
        request: crate::attachments::DownloadAttachmentRequest {
            thread: crate::messages::ThreadRef::Direct(
                crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
            ),
            message_id: crate::ids::MessageId::parse("msg-e2ee-1").unwrap(),
            attachment_id: Some("att-e2ee-1".to_string()),
            destination: crate::attachments::AttachmentDestination::LocalFile(output.clone()),
            overwrite: false,
        },
        resolved_peer_did: None,
    })
    .unwrap_err();

    assert_service_error_code(err, "anp.attachment.decrypt_failed");
    assert!(!output.exists());
    assert_no_attachment_temp_files(output.parent().unwrap());
    assert_eq!(calls.borrow().len(), 4);
}

#[derive(Clone)]
struct ReadySessionProvider {
    scopes: Rc<RefCell<Vec<crate::auth::AuthScope>>>,
}

impl SessionProvider for ReadySessionProvider {
    fn ensure_session(
        &self,
        scope: crate::auth::AuthScope,
    ) -> crate::ImResult<crate::auth::SessionBundle> {
        self.scopes.borrow_mut().push(scope);
        Ok(crate::auth::SessionBundle {
            subject: crate::ids::Did::parse("did:example:alice")?,
            scope,
            expires_at: None,
            refreshed: false,
            bearer_token: None,
        })
    }

    fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
        unreachable!("attachment download runtime should not refresh through test provider")
    }

    fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
        unreachable!("attachment download runtime should not read status")
    }
}

impl crate::internal::auth::session::AsyncSessionProvider for ReadySessionProvider {
    async fn ensure_session(
        &self,
        scope: crate::auth::AuthScope,
    ) -> crate::ImResult<crate::auth::SessionBundle> {
        self.scopes.borrow_mut().push(scope);
        Ok(crate::auth::SessionBundle {
            subject: crate::ids::Did::parse("did:example:alice")?,
            scope,
            expires_at: None,
            refreshed: false,
            bearer_token: None,
        })
    }

    async fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
        unreachable!("attachment download runtime should not refresh through test provider")
    }

    async fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
        unreachable!("attachment download runtime should not read status")
    }
}

struct RecordingTransport {
    calls: Rc<RefCell<Vec<RecordedCall>>>,
}

impl AuthenticatedRpcTransport for RecordingTransport {
    fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> crate::ImResult<Value> {
        self.calls.borrow_mut().push(RecordedCall::Rpc {
            endpoint: endpoint.to_string(),
            method: method.to_string(),
            params: params.clone(),
        });
        match method {
            "direct.get_history" => direct_history_response(params["body"]["skip"].as_i64()),
            "group.list_messages" => Ok(group_history_response()),
            "attachment.get_download_ticket" => Ok(json!({
                "download_ticket_b64u": "ticket-1",
                "expires_at": "2026-05-23T01:00:00Z",
                "ticket_binding": {
                    "attachment_id": params["body"]["attachment_id"].clone()
                }
            })),
            _ => Err(crate::ImError::TransportUnavailable {
                detail: format!("unexpected rpc method {method} at {endpoint}"),
            }),
        }
    }
}

impl RawJsonTransport for RecordingTransport {
    fn get_json_url(
        &mut self,
        url: &str,
        headers: BTreeMap<String, String>,
    ) -> crate::ImResult<Value> {
        self.calls.borrow_mut().push(RecordedCall::GetJson {
            url: url.to_string(),
            headers,
        });
        Ok(json!({
            "id": "did:web:example.com:bob",
            "service": [{
                "id": "#attachment",
                "type": "ANPMessageService",
                "serviceEndpoint": "https://attachment.example/rpc",
                "serviceDid": "did:web:attachment.example",
                "profiles": ["anp.attachment.v1"],
                "securityProfiles": ["transport-protected"],
                "priority": 1
            }]
        }))
    }
}

impl RpcTransport for RecordingTransport {
    fn rpc(&mut self, endpoint: &str, method: &str, params: Value) -> crate::ImResult<Value> {
        AuthenticatedRpcTransport::authenticated_rpc(self, endpoint, method, params)
    }
}

impl AttachmentObjectTransport for RecordingTransport {
    fn put_attachment_object(
        &mut self,
        _upload_uri: &str,
        _headers: BTreeMap<String, String>,
        _body: Vec<u8>,
    ) -> crate::ImResult<()> {
        unreachable!("download runtime should not upload objects")
    }

    fn get_attachment_object(
        &mut self,
        object_uri: &str,
        download_ticket: &str,
    ) -> crate::ImResult<AttachmentObjectResponse> {
        self.calls.borrow_mut().push(RecordedCall::GetObject {
            object_uri: object_uri.to_string(),
            ticket: download_ticket.to_string(),
        });
        Ok(AttachmentObjectResponse {
            body: b"downloaded bytes".to_vec(),
            content_type: Some("application/octet-stream".to_string()),
        })
    }
}

impl crate::internal::transport::AsyncAuthenticatedRpcTransport for RecordingTransport {
    async fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> crate::ImResult<Value> {
        AuthenticatedRpcTransport::authenticated_rpc(self, endpoint, method, params)
    }
}

impl crate::internal::transport::AsyncRawJsonTransport for RecordingTransport {
    async fn get_json_url(
        &mut self,
        url: &str,
        headers: BTreeMap<String, String>,
    ) -> crate::ImResult<Value> {
        RawJsonTransport::get_json_url(self, url, headers)
    }
}

impl crate::internal::transport::AsyncRpcTransport for RecordingTransport {
    async fn rpc(&mut self, endpoint: &str, method: &str, params: Value) -> crate::ImResult<Value> {
        RpcTransport::rpc(self, endpoint, method, params)
    }
}

impl crate::internal::transport::AsyncAttachmentObjectTransport for RecordingTransport {
    async fn put_attachment_object(
        &mut self,
        upload_uri: &str,
        headers: BTreeMap<String, String>,
        body: Vec<u8>,
    ) -> crate::ImResult<()> {
        AttachmentObjectTransport::put_attachment_object(self, upload_uri, headers, body)
    }

    async fn get_attachment_object(
        &mut self,
        object_uri: &str,
        download_ticket: &str,
    ) -> crate::ImResult<AttachmentObjectResponse> {
        AttachmentObjectTransport::get_attachment_object(self, object_uri, download_ticket)
    }

    async fn get_attachment_object_stream(
        &mut self,
        object_uri: &str,
        download_ticket: &str,
    ) -> crate::ImResult<crate::internal::transport::AsyncAttachmentObjectResponse> {
        self.calls.borrow_mut().push(RecordedCall::GetObjectStream {
            object_uri: object_uri.to_string(),
            ticket: download_ticket.to_string(),
        });
        Ok(
            crate::internal::transport::AsyncAttachmentObjectResponse::Bytes {
                body: b"downloaded bytes".to_vec(),
                content_type: Some("application/octet-stream".to_string()),
                consumed: false,
            },
        )
    }
}

struct LocalFallbackTransport {
    calls: Rc<RefCell<Vec<RecordedCall>>>,
}

impl AuthenticatedRpcTransport for LocalFallbackTransport {
    fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> crate::ImResult<Value> {
        self.calls.borrow_mut().push(RecordedCall::Rpc {
            endpoint: endpoint.to_string(),
            method: method.to_string(),
            params: params.clone(),
        });
        match method {
            "direct.get_history" => Ok(json!({
                "messages": [{
                    "id": "msg-local-sender",
                    "message_id": "msg-local-sender",
                    "sender_did": "did:web:example:alice",
                    "content": {
                        "attachments": [{
                            "attachment_id": "att-local",
                            "filename": "local.txt",
                            "mime_type": "text/plain",
                            "size": "20",
                            "digest": { "alg": "sha-256", "value_b64u": "nckhHGQ875uyaFS-XN4okcnICwLgNUjey_suVwwrcNY" },
                            "access_info": { "object_uri": "https://objects.example/att-local" }
                        }],
                        "primary_attachment_id": "att-local"
                    }
                }],
                "has_more": false
            })),
            "attachment.get_download_ticket" => Ok(json!({
                "download_ticket_b64u": "ticket-local",
                "expires_at": "2026-05-23T01:00:00Z",
                "ticket_binding": {
                    "attachment_id": params["body"]["attachment_id"].clone()
                }
            })),
            _ => Err(crate::ImError::TransportUnavailable {
                detail: format!("unexpected rpc method {method} at {endpoint}"),
            }),
        }
    }
}

impl RawJsonTransport for LocalFallbackTransport {
    fn get_json_url(
        &mut self,
        url: &str,
        headers: BTreeMap<String, String>,
    ) -> crate::ImResult<Value> {
        self.calls.borrow_mut().push(RecordedCall::GetJson {
            url: url.to_string(),
            headers,
        });
        Err(crate::ImError::TransportUnavailable {
            detail: format!("forced DID document miss for {url}"),
        })
    }
}

impl RpcTransport for LocalFallbackTransport {
    fn rpc(&mut self, endpoint: &str, method: &str, params: Value) -> crate::ImResult<Value> {
        AuthenticatedRpcTransport::authenticated_rpc(self, endpoint, method, params)
    }
}

impl AttachmentObjectTransport for LocalFallbackTransport {
    fn put_attachment_object(
        &mut self,
        _upload_uri: &str,
        _headers: BTreeMap<String, String>,
        _body: Vec<u8>,
    ) -> crate::ImResult<()> {
        unreachable!("download runtime should not upload objects")
    }

    fn get_attachment_object(
        &mut self,
        object_uri: &str,
        download_ticket: &str,
    ) -> crate::ImResult<AttachmentObjectResponse> {
        self.calls.borrow_mut().push(RecordedCall::GetObject {
            object_uri: object_uri.to_string(),
            ticket: download_ticket.to_string(),
        });
        Ok(AttachmentObjectResponse {
            body: b"local document bytes".to_vec(),
            content_type: Some("text/plain".to_string()),
        })
    }
}

struct E2eeTransport {
    calls: Rc<RefCell<Vec<RecordedCall>>>,
    history: Value,
    object_body: Vec<u8>,
    object_content_type: Option<String>,
}

impl AuthenticatedRpcTransport for E2eeTransport {
    fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> crate::ImResult<Value> {
        self.calls.borrow_mut().push(RecordedCall::Rpc {
            endpoint: endpoint.to_string(),
            method: method.to_string(),
            params: params.clone(),
        });
        match method {
            "direct.get_history" => Ok(self.history.clone()),
            "attachment.get_download_ticket" => Ok(json!({
                "download_ticket_b64u": "ticket-e2ee",
                "expires_at": "2026-05-23T01:00:00Z",
                "ticket_binding": {
                    "attachment_id": params["body"]["attachment_id"].clone()
                }
            })),
            _ => Err(crate::ImError::TransportUnavailable {
                detail: format!("unexpected rpc method {method} at {endpoint}"),
            }),
        }
    }
}

impl RawJsonTransport for E2eeTransport {
    fn get_json_url(
        &mut self,
        url: &str,
        headers: BTreeMap<String, String>,
    ) -> crate::ImResult<Value> {
        self.calls.borrow_mut().push(RecordedCall::GetJson {
            url: url.to_string(),
            headers,
        });
        Ok(json!({
            "id": "did:web:example.com:bob",
            "service": [{
                "id": "#attachment",
                "type": "ANPMessageService",
                "serviceEndpoint": "https://attachment.example/rpc",
                "serviceDid": "did:web:attachment.example",
                "profiles": ["anp.attachment.v1"],
                "securityProfiles": ["transport-protected"],
                "priority": 1
            }]
        }))
    }
}

impl RpcTransport for E2eeTransport {
    fn rpc(&mut self, endpoint: &str, method: &str, params: Value) -> crate::ImResult<Value> {
        AuthenticatedRpcTransport::authenticated_rpc(self, endpoint, method, params)
    }
}

impl AttachmentObjectTransport for E2eeTransport {
    fn put_attachment_object(
        &mut self,
        _upload_uri: &str,
        _headers: BTreeMap<String, String>,
        _body: Vec<u8>,
    ) -> crate::ImResult<()> {
        unreachable!("download runtime should not upload objects")
    }

    fn get_attachment_object(
        &mut self,
        object_uri: &str,
        download_ticket: &str,
    ) -> crate::ImResult<AttachmentObjectResponse> {
        self.calls.borrow_mut().push(RecordedCall::GetObject {
            object_uri: object_uri.to_string(),
            ticket: download_ticket.to_string(),
        });
        Ok(AttachmentObjectResponse {
            body: self.object_body.clone(),
            content_type: self.object_content_type.clone(),
        })
    }
}

impl crate::internal::transport::AsyncAuthenticatedRpcTransport for E2eeTransport {
    async fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> crate::ImResult<Value> {
        AuthenticatedRpcTransport::authenticated_rpc(self, endpoint, method, params)
    }
}

impl crate::internal::transport::AsyncRawJsonTransport for E2eeTransport {
    async fn get_json_url(
        &mut self,
        url: &str,
        headers: BTreeMap<String, String>,
    ) -> crate::ImResult<Value> {
        RawJsonTransport::get_json_url(self, url, headers)
    }
}

impl crate::internal::transport::AsyncRpcTransport for E2eeTransport {
    async fn rpc(&mut self, endpoint: &str, method: &str, params: Value) -> crate::ImResult<Value> {
        RpcTransport::rpc(self, endpoint, method, params)
    }
}

impl crate::internal::transport::AsyncAttachmentObjectTransport for E2eeTransport {
    async fn put_attachment_object(
        &mut self,
        upload_uri: &str,
        headers: BTreeMap<String, String>,
        body: Vec<u8>,
    ) -> crate::ImResult<()> {
        AttachmentObjectTransport::put_attachment_object(self, upload_uri, headers, body)
    }

    async fn get_attachment_object(
        &mut self,
        object_uri: &str,
        download_ticket: &str,
    ) -> crate::ImResult<AttachmentObjectResponse> {
        AttachmentObjectTransport::get_attachment_object(self, object_uri, download_ticket)
    }

    async fn get_attachment_object_stream(
        &mut self,
        object_uri: &str,
        download_ticket: &str,
    ) -> crate::ImResult<crate::internal::transport::AsyncAttachmentObjectResponse> {
        self.calls.borrow_mut().push(RecordedCall::GetObjectStream {
            object_uri: object_uri.to_string(),
            ticket: download_ticket.to_string(),
        });
        Ok(
            crate::internal::transport::AsyncAttachmentObjectResponse::Bytes {
                body: self.object_body.clone(),
                content_type: self.object_content_type.clone(),
                consumed: false,
            },
        )
    }
}

#[derive(Debug, Clone)]
enum RecordedCall {
    Rpc {
        endpoint: String,
        method: String,
        params: Value,
    },
    GetJson {
        url: String,
        headers: BTreeMap<String, String>,
    },
    GetObject {
        object_uri: String,
        ticket: String,
    },
    GetObjectStream {
        object_uri: String,
        ticket: String,
    },
}

impl RecordedCall {
    fn rpc(&self, expected_method: &str) -> RecordedRpc<'_> {
        match self {
            Self::Rpc {
                endpoint,
                method,
                params,
            } => {
                assert_eq!(method, expected_method);
                RecordedRpc { endpoint, params }
            }
            _ => panic!("expected rpc call {expected_method}, got {self:?}"),
        }
    }

    fn get_json(&self, expected_url: &str) -> RecordedGetJson<'_> {
        match self {
            Self::GetJson { url, headers } => {
                assert_eq!(url, expected_url);
                RecordedGetJson { headers }
            }
            _ => panic!("expected get-json call {expected_url}, got {self:?}"),
        }
    }

    fn object_get(&self, expected_uri: &str) -> RecordedGetObject<'_> {
        match self {
            Self::GetObject { object_uri, ticket } => {
                assert_eq!(object_uri, expected_uri);
                RecordedGetObject { ticket }
            }
            _ => panic!("expected object GET call {expected_uri}, got {self:?}"),
        }
    }

    fn object_get_stream(&self, expected_uri: &str) -> RecordedGetObject<'_> {
        match self {
            Self::GetObjectStream { object_uri, ticket } => {
                assert_eq!(object_uri, expected_uri);
                RecordedGetObject { ticket }
            }
            _ => panic!("expected object streaming GET call {expected_uri}, got {self:?}"),
        }
    }
}

struct RecordedRpc<'a> {
    endpoint: &'a str,
    params: &'a Value,
}

struct RecordedGetJson<'a> {
    headers: &'a BTreeMap<String, String>,
}

struct RecordedGetObject<'a> {
    ticket: &'a str,
}

fn direct_history_response(skip: Option<i64>) -> crate::ImResult<Value> {
    if skip.unwrap_or_default() == 0 {
        Ok(json!({
            "messages": [{
                "id": "msg-attachment-1",
                "message_id": "msg-attachment-1",
                "sender_did": "did:web:example.com:bob",
                "content": {
                    "attachments": [{
                        "attachment_id": "att-1",
                        "filename": "report.txt",
                        "mime_type": "text/plain",
                        "size": "16",
                        "digest": { "alg": "sha-256", "value_b64u": "exNCHlmX3QP0Pkz7u3ndQu3b5zESko9lysH2TsoflvQ" },
                        "access_info": { "object_uri": "https://objects.example/att-1" }
                    }],
                    "primary_attachment_id": "att-1",
                    "caption": "report"
                }
            }],
            "has_more": true
        }))
    } else {
        Ok(json!({
            "messages": [{
                "id": "msg-second-page",
                "message_id": "msg-second-page",
                "sender_did": "did:web:example.com:bob",
                "content": {
                    "attachments": [{
                        "attachment_id": "att-2",
                        "filename": "second.txt",
                        "mime_type": "text/plain",
                        "size": "16",
                        "digest": { "alg": "sha-256", "value_b64u": "exNCHlmX3QP0Pkz7u3ndQu3b5zESko9lysH2TsoflvQ" },
                        "access_info": { "object_uri": "https://objects.example/att-2" }
                    }],
                    "primary_attachment_id": "att-2"
                }
            }],
            "has_more": false
        }))
    }
}

fn group_history_response() -> Value {
    json!({
        "messages": [{
            "id": "group-msg-1",
            "message_id": "group-msg-1",
            "sender_did": "did:web:example.com:bob",
            "content": serde_json::to_string(&json!({
                "attachments": [{
                    "attachment_id": "att-group-1",
                    "filename": "group.txt",
                    "mime_type": "text/plain",
                    "size": "16",
                    "digest": { "alg": "sha-256", "value_b64u": "exNCHlmX3QP0Pkz7u3ndQu3b5zESko9lysH2TsoflvQ" },
                    "access_info": { "object_uri": "https://objects.example/att-1" }
                }],
                "primary_attachment_id": "att-group-1"
            }))
            .unwrap()
        }],
        "has_more": false
    })
}

struct ObjectE2eeCase {
    plaintext: Vec<u8>,
    ciphertext: Vec<u8>,
    full_manifest: Value,
    object_key_b64u: String,
    nonce_b64u: String,
}

fn object_e2ee_case(plaintext: Vec<u8>) -> ObjectE2eeCase {
    let prepared = crate::attachments::manifest::prepare_object_e2ee_attachment_payload(
        "secret.pdf",
        "application/pdf",
        plaintext.clone(),
    )
    .expect("object-e2ee attachment prepares");
    let descriptor = crate::attachments::manifest::AttachmentDescriptor::from_prepared(
        &prepared.prepared,
        "att-e2ee-1",
        "https://objects.example/att-e2ee-1",
    );
    let full_manifest =
        crate::attachments::manifest::build_attachment_manifest_with_object_e2ee_secrets(
            &descriptor,
            "secret",
            &prepared.secrets,
        )
        .expect("full manifest");
    ObjectE2eeCase {
        plaintext,
        ciphertext: prepared.prepared.payload,
        full_manifest,
        object_key_b64u: prepared.secrets.object_key_b64u,
        nonce_b64u: prepared.secrets.nonce_b64u,
    }
}

fn e2ee_history_response(manifest: Value, message_security_profile: &str) -> Value {
    let mut message = json!({
        "id": "msg-e2ee-1",
        "message_id": "msg-e2ee-1",
        "sender_did": "did:web:example.com:bob",
        "content": manifest
    });
    if !message_security_profile.trim().is_empty() {
        message["message_security_profile"] = json!(message_security_profile);
    }
    json!({
        "messages": [message],
        "has_more": false
    })
}

fn e2ee_history_response_with_legacy_missing_profile(manifest: Value) -> Value {
    json!({
        "messages": [{
            "id": "msg-e2ee-1",
            "message_id": "msg-e2ee-1",
            "sender_did": "did:web:example.com:bob",
            "content": manifest
        }],
        "has_more": false
    })
}

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = unique_temp_root();
        let identities = root.join("identities");
        fs::create_dir_all(identities.join("alice")).unwrap();
        fs::write(identities.join("default"), "alice\n").unwrap();
        fs::write(
            identities.join("registry.json"),
            r#"{
              "default_identity": "alice",
              "identities": [{
                "id": "alice-id",
                "did": "did:example:alice",
                "local_alias": "alice",
                "ready_for_auth": true,
                "ready_for_messaging": true,
                "missing": []
              }]
            }"#,
        )
        .unwrap();
        Self { root }
    }

    fn write_attachment_service_document(
        &self,
        alias: &str,
        did: &str,
        rpc_endpoint: &str,
        service_did: &str,
    ) {
        let identity_dir = self.root.join("identities").join(alias);
        fs::create_dir_all(&identity_dir).unwrap();
        fs::write(
            self.root.join("identities").join("registry.json"),
            format!(
                r#"{{
                  "default_identity": "alice",
                  "identities": [
                    {{
                      "id": "alice-id",
                      "did": "did:example:alice",
                      "local_alias": "alice",
                      "ready_for_auth": true,
                      "ready_for_messaging": true,
                      "missing": []
                    }},
                    {{
                      "id": "{alias}-id",
                      "did": "{did}",
                      "local_alias": "{alias}",
                      "ready_for_auth": true,
                      "ready_for_messaging": true,
                      "missing": []
                    }}
                  ]
                }}"#
            ),
        )
        .unwrap();
        fs::write(
            identity_dir.join("did.json"),
            serde_json::to_vec_pretty(&json!({
                "id": did,
                "service": [{
                    "id": "#attachment",
                    "type": "ANPMessageService",
                    "serviceEndpoint": rpc_endpoint,
                    "serviceDid": service_did,
                    "profiles": ["anp.attachment.v1"],
                    "securityProfiles": ["transport-protected"],
                    "priority": 1
                }]
            }))
            .unwrap(),
        )
        .unwrap();
    }

    fn client(&self) -> crate::core::ImClient {
        crate::core::ImCore::new(
            crate::ImCoreConfig {
                service_base_url: crate::ServiceEndpoint::parse("https://example.test").unwrap(),
                did_domain: "awiki.test".to_string(),
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
            "alice".to_string(),
        ))
        .unwrap()
    }
}

fn unique_temp_root() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "im-core-attachment-download-runtime-{}-{nanos}",
        std::process::id()
    ))
}

fn assert_no_attachment_temp_files(path: &std::path::Path) {
    let leftovers: Vec<_> = fs::read_dir(path)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(".awiki-attachment-download-"))
        .collect();
    assert_eq!(leftovers, Vec::<String>::new());
}

fn assert_service_error_code(err: crate::ImError, expected: &str) {
    assert!(matches!(
        err,
        crate::ImError::Service { code: Some(code), .. } if code == expected
    ));
}
