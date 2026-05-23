use std::path::PathBuf;

use im_core::prelude::*;

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
fn attachments_service_send_and_download_are_explicitly_unsupported() {
    let core = test_core();
    let client = core
        .client(IdentitySelector::LocalAlias("alice".to_string()))
        .unwrap();
    let peer = PeerRef::parse("bob", "awiki.info").unwrap();

    let send = client.attachments().send(
        MessageTarget::Direct(peer.clone()),
        AttachmentSendRequest {
            input: AttachmentInput::Bytes {
                filename: Some("note.txt".to_string()),
                mime_type: Some("text/plain".to_string()),
                bytes: b"hello".to_vec(),
            },
            caption: Some("hello".to_string()),
            mime_type: None,
            filename: None,
            delivery: MessageDeliveryOptions::default(),
        },
    );
    assert!(matches!(
        send,
        Err(ImError::UnsupportedCapability { capability }) if capability == "attachments"
    ));

    let download = client.attachments().download(DownloadAttachmentRequest {
        thread: ThreadRef::Direct(peer),
        message_id: MessageId::parse("msg-1").unwrap(),
        attachment_id: Some("att-1".to_string()),
        destination: AttachmentDestination::Memory,
        overwrite: false,
    });
    assert!(matches!(
        download,
        Err(ImError::UnsupportedCapability { capability }) if capability == "attachments"
    ));
}

#[test]
fn message_body_attachments_reuse_canonical_attachment_input() {
    let body = MessageBody::Attachment {
        input: AttachmentInput::LocalFile(PathBuf::from("image.png")),
        caption: Some("caption".to_string()),
        mime_type: Some("image/png".to_string()),
    };

    match body {
        MessageBody::Attachment {
            input: AttachmentInput::LocalFile(path),
            caption,
            mime_type,
        } => {
            assert_eq!(path, PathBuf::from("image.png"));
            assert_eq!(caption.as_deref(), Some("caption"));
            assert_eq!(mime_type.as_deref(), Some("image/png"));
        }
        _ => panic!("expected attachment body"),
    }
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
        anp_service_endpoint: None,
        anp_service_did: None,
        transport_policy: MessageTransportPolicy::HttpOnly,
    }
}

fn test_paths() -> ImCorePaths {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/im-core-tests/attachment-api")
        .join(format!("{:?}", std::thread::current().id()));
    ImCorePaths {
        identities: IdentityRegistryPaths {
            identity_root_dir: base.join("identities"),
            registry_path: base.join("identities").join("registry.json"),
            default_identity_path: Some(base.join("identities").join("default")),
        },
        local_state: LocalStatePaths {
            sqlite_path: base.join("local").join("im.sqlite"),
        },
        runtime: RuntimePaths {
            cache_dir: base.join("cache"),
            temp_dir: base.join("tmp"),
        },
    }
}
