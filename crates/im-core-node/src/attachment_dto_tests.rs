use super::*;
use sha2::Digest as _;

#[test]
fn encrypted_history_attachment_uses_plaintext_display_size() {
    let content = message_content(
        im_core::messages::MessageBodyView::Payload {
            payload: serde_json::json!({
                "attachments": [{
                    "attachment_id": "attachment-1", "filename": "test.txt", "mime_type": "text/plain",
                    "size": "29", "encryption_info": {"mode": "object-e2ee", "plaintext_size": "13"}
                }],
                "primary_attachment_id": "attachment-1"
            }),
        },
        Some(im_core::attachments::attachment_manifest_content_type()),
    );
    assert_eq!(content.kind, "attachment");
    assert_eq!(content.attachment.unwrap().size_bytes, "13");
}

#[test]
fn encrypted_download_metadata_describes_authenticated_plaintext_bytes() {
    let bytes = b"original file".to_vec();
    let object_digest = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([42u8; 32]);
    let selection = im_core::attachments::AttachmentSelection {
        attachment_id: "attachment-1".into(),
        filename: "test.txt".into(),
        mime_type: "text/plain".into(),
        size: (bytes.len() + 16).to_string(),
        digest_b64u: object_digest.clone(),
        object_encryption_mode: "object-e2ee".into(),
        plaintext_size: Some(bytes.len().to_string()),
        ..Default::default()
    };
    let mapped = downloaded_attachment(im_core::attachments::DownloadedAttachment {
        attachment_id: "attachment-1".into(),
        filename: None,
        mime_type: None,
        size_bytes: Some(bytes.len() as u64),
        selection: Some(selection),
        destination: im_core::attachments::DownloadedAttachmentDestination::Memory(bytes.clone()),
        warnings: vec![],
    })
    .unwrap();
    let digest =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(&bytes));
    assert_eq!(mapped.bytes.to_vec(), bytes);
    assert_eq!(mapped.attachment.size_bytes, bytes.len().to_string());
    assert_eq!(mapped.attachment.digest_b64u, digest);
    assert_eq!(mapped.attachment.sha256_hex, digest_hex(&digest));
    assert_ne!(mapped.attachment.digest_b64u, object_digest);
}
