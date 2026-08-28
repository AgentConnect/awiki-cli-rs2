use awiki_im_core::compat::email;
use awiki_im_core::prelude::*;
use serde_json::{json, Value};

#[test]
fn email_rpc_calls_match_legacy_methods_and_params() {
    let inbox = email::build_inbox_rpc_call(EmailInboxQuery {
        folder: EmailFolder::inbox(),
        limit: PageLimit::new(20).unwrap(),
        offset: 3,
        unread_only: true,
    });
    assert_eq!(inbox.endpoint, email::MAIL_RPC_ENDPOINT);
    assert_eq!(inbox.method, "mail.getInbox");
    assert_eq!(
        inbox.params,
        json!({
            "folder": "inbox",
            "limit": 20,
            "offset": 3,
            "unread_only": true,
        })
    );

    let read = email::build_read_rpc_call(EmailMessageId::parse("msg-1").unwrap());
    assert_eq!(read.method, "mail.getMessage");
    assert_eq!(read.params, json!({ "message_id": "msg-1" }));

    let mark_read = email::build_mark_read_rpc_call(EmailMarkReadRequest {
        message_ids: vec![
            EmailMessageId::parse("msg-1").unwrap(),
            EmailMessageId::parse("msg-2").unwrap(),
        ],
        is_read: false,
    })
    .expect("mark-read rpc call");
    assert_eq!(mark_read.method, "mail.markRead");
    assert_eq!(
        mark_read.params,
        json!({ "message_ids": ["msg-1", "msg-2"], "is_read": false })
    );

    let account = email::build_account_rpc_call();
    assert_eq!(account.method, "mail.getMailbox");
    assert_eq!(account.params, json!({}));

    let attachment = email::build_attachment_rpc_call(EmailAttachmentDownloadRequest {
        message_id: EmailMessageId::parse("msg-1").unwrap(),
        attachment_index: 2,
    });
    assert_eq!(attachment.method, "mail.getAttachment");
    assert_eq!(
        attachment.params,
        json!({ "message_id": "msg-1", "attachment_index": 2 })
    );

    let send = email::build_send_rpc_call(SendEmailRequest {
        to: vec![EmailAddress::parse("bob@example.com").unwrap()],
        cc: vec![EmailAddress::parse("copy@example.com").unwrap()],
        subject: "Hello".to_string(),
        body_text: "Body".to_string(),
        body_html: None,
    })
    .expect("send rpc call");
    assert_eq!(send.method, "mail.send");
    assert_eq!(
        send.params,
        json!({
            "to": ["bob@example.com"],
            "cc": ["copy@example.com"],
            "subject": "Hello",
            "body_text": "Body",
            "body_html": Value::Null,
        })
    );

    let send_html = email::build_send_rpc_call(SendEmailRequest {
        body_html: Some("<p>Body</p>".to_string()),
        ..SendEmailRequest {
            to: vec![EmailAddress::parse("bob@example.com").unwrap()],
            cc: Vec::new(),
            subject: "Hello".to_string(),
            body_text: "Body".to_string(),
            body_html: None,
        }
    })
    .expect("html send rpc call");
    assert_eq!(send_html.params["body_html"], "<p>Body</p>");
}

#[test]
fn email_wire_validation_errors_are_sdk_inputs() {
    assert!(EmailMessageId::parse("").is_err());
    assert!(EmailAddress::parse("not-an-address").is_err());
    assert!(email::build_mark_read_rpc_call(EmailMarkReadRequest {
        message_ids: Vec::new(),
        is_read: true,
    })
    .is_err());
    assert!(email::build_send_rpc_call(SendEmailRequest {
        to: Vec::new(),
        cc: Vec::new(),
        subject: "Hello".to_string(),
        body_text: "Body".to_string(),
        body_html: None,
    })
    .is_err());
    assert!(email::build_send_rpc_call(SendEmailRequest {
        to: vec![EmailAddress::parse("bob@example.com").unwrap()],
        cc: Vec::new(),
        subject: String::new(),
        body_text: "Body".to_string(),
        body_html: None,
    })
    .is_err());
    assert!(email::build_send_rpc_call(SendEmailRequest {
        to: vec![EmailAddress::parse("bob@example.com").unwrap()],
        cc: Vec::new(),
        subject: "Hello".to_string(),
        body_text: String::new(),
        body_html: None,
    })
    .is_err());
}

#[test]
fn email_attachment_send_wire_encodes_bytes_and_legacy_serde_defaults_empty() {
    let legacy: SendEmailRequest = serde_json::from_value(json!({
        "to": ["bob@example.com"],
        "cc": [],
        "subject": "Legacy",
        "body_text": "Body",
        "body_html": null
    }))
    .expect("legacy request deserializes");
    assert_eq!(legacy.subject, "Legacy");

    let send = email::build_send_with_attachments_rpc_call(SendEmailWithAttachmentsRequest {
        to: vec![EmailAddress::parse("bob@example.com").unwrap()],
        cc: Vec::new(),
        subject: "Attachments".to_string(),
        body_text: "Body".to_string(),
        body_html: None,
        attachments: vec![
            EmailAttachmentInput {
                filename: "fixture.txt".to_string(),
                content_type: "text/plain".to_string(),
                bytes: b"mail attachment".to_vec(),
            },
            EmailAttachmentInput {
                filename: "empty.png".to_string(),
                content_type: "image/png".to_string(),
                bytes: Vec::new(),
            },
        ],
    })
    .expect("attachment send rpc call");

    assert_eq!(
        send.params["attachments"],
        json!([
            {
                "filename": "fixture.txt",
                "content_type": "text/plain",
                "content_base64": "bWFpbCBhdHRhY2htZW50"
            },
            {
                "filename": "empty.png",
                "content_type": "image/png",
                "content_base64": ""
            }
        ])
    );
}

#[test]
fn email_attachment_send_wire_rejects_unsafe_names_types_and_limits() {
    let request = |attachments| SendEmailWithAttachmentsRequest {
        to: vec![EmailAddress::parse("bob@example.com").unwrap()],
        cc: Vec::new(),
        subject: "Attachments".to_string(),
        body_text: "Body".to_string(),
        body_html: None,
        attachments,
    };
    let attachment = |filename: &str, content_type: &str, size: usize| EmailAttachmentInput {
        filename: filename.to_string(),
        content_type: content_type.to_string(),
        bytes: vec![0; size],
    };

    for attachments in [
        vec![attachment(" ", "text/plain", 1)],
        vec![attachment("..", "text/plain", 1)],
        vec![attachment("../secret.txt", "text/plain", 1)],
        vec![attachment("bad\nname.txt", "text/plain", 1)],
        vec![attachment("payload.exe ", "application/octet-stream", 1)],
        vec![attachment("safe\u{202e}gnp.txt", "text/plain", 1)],
        vec![attachment("private\u{e000}.txt", "text/plain", 1)],
        vec![attachment("unassigned\u{0378}.txt", "text/plain", 1)],
        vec![attachment(
            &format!("{}.txt", "界".repeat(84)),
            "text/plain",
            1,
        )],
        vec![attachment("file.txt", "text/plain; charset=utf-8", 1)],
        vec![attachment("file.txt", "not-a-type", 1)],
        vec![attachment(
            "large.bin",
            "application/octet-stream",
            awiki_im_core::email::EMAIL_ATTACHMENT_MAX_BYTES + 1,
        )],
        (0..=awiki_im_core::email::EMAIL_ATTACHMENT_MAX_COUNT)
            .map(|index| attachment(&format!("{index}.txt"), "text/plain", 0))
            .collect(),
    ] {
        assert!(email::build_send_with_attachments_rpc_call(request(attachments)).is_err());
    }

    let over_total = (0..3)
        .map(|index| {
            attachment(
                &format!("{index}.bin"),
                "application/octet-stream",
                9 * 1024 * 1024,
            )
        })
        .collect();
    assert!(email::build_send_with_attachments_rpc_call(request(over_total)).is_err());
}

#[test]
fn email_send_normalizer_requires_unambiguous_explicit_success() {
    let sent = email::normalize_send(json!({
        "accepted": true,
        "status": "sent",
        "message_id": "mail-sent-1",
        "warnings": ["queued"]
    }))
    .expect("explicit success");
    assert!(sent.accepted);
    assert_eq!(sent.message_id.unwrap().as_str(), "mail-sent-1");
    assert_eq!(sent.warnings, ["queued"]);

    for rejected in [
        json!({"accepted": false, "status": "failed"}),
        json!({"accepted": true, "status": "failed"}),
        json!({"accepted": false, "status": "sent"}),
        json!({"accepted": true}),
        json!({"status": "sent"}),
        json!({"accepted": true, "status": "sent", "success": false}),
        json!({}),
        Value::Null,
    ] {
        assert!(email::normalize_send(rejected).is_err());
    }
}

#[test]
fn email_attachment_download_is_canonical_and_size_checked() {
    let request = || EmailAttachmentDownloadRequest {
        message_id: EmailMessageId::parse("mail-1").unwrap(),
        attachment_index: 2,
    };
    let attachment = email::normalize_attachment(
        request(),
        json!({
            "index": 2,
            "filename": "fixture.bin",
            "content_type": "application/octet-stream",
            "size": 3,
            "content_base64": "AP+A"
        }),
    )
    .expect("canonical attachment");
    assert_eq!(attachment.bytes, [0, 255, 128]);
    assert_eq!(attachment.size, Some(3));
    assert_eq!(attachment.filename, "fixture.bin");

    let empty = email::normalize_attachment(
        request(),
        json!({
            "index": 2,
            "filename": "empty.txt",
            "content_type": "text/plain",
            "size": 0,
            "content_base64": ""
        }),
    )
    .expect("empty attachment");
    assert!(empty.bytes.is_empty());

    for invalid in [
        json!({"index": 2, "filename": "a", "content_type": "text/plain", "size": 1, "content_base64": "/x=="}),
        json!({"index": 2, "filename": "a", "content_type": "text/plain", "size": 2, "content_base64": "YQ=="}),
        json!({"index": 3, "filename": "a", "content_type": "text/plain", "size": 1, "content_base64": "YQ=="}),
        json!({"index": 2, "filename": "a", "content_type": "text/plain", "size": 1, "content_base64": "%%%"}),
        json!({"index": 2, "filename": "a", "content_type": "text/plain", "size": 1}),
        json!({"index": 2.0, "filename": "a", "content_type": "text/plain", "size": 1, "content_base64": "YQ=="}),
        json!({"index": 2, "filename": "a", "content_type": "text/plain", "size": 1.0, "content_base64": "YQ=="}),
        json!({"index": 2, "filename": "a", "content_type": "text/plain", "size": 10 * 1024 * 1024 + 1, "content_base64": ""}),
        json!({"index": 2, "filename": "../escape", "content_type": "text/plain", "size": 1, "content_base64": "YQ=="}),
        json!({"index": 2, "filename": "safe\u{202e}gnp.txt", "content_type": "text/plain", "size": 1, "content_base64": "YQ=="}),
    ] {
        assert!(email::normalize_attachment(request(), invalid).is_err());
    }
}

#[test]
fn email_inbox_normalization_accepts_legacy_shapes() {
    let page = email::normalize_inbox(json!({
        "messages": [{
            "message_id": "mail-1",
            "folder": "inbox",
            "from_addr": "alice@example.com",
            "to": ["bob@example.com"],
            "subject": "",
            "preview": "Hi",
            "is_read": false,
            "has_attachments": "true",
            "attachment_count": 2,
            "custom": "kept"
        }],
        "has_more": true
    }))
    .expect("normalized inbox");

    assert!(page.has_more);
    assert_eq!(page.items.len(), 1);
    let item = &page.items[0];
    assert_eq!(item.id.as_str(), "mail-1");
    assert_eq!(item.subject, "(no subject)");
    assert_eq!(item.from[0].as_str(), "alice@example.com");
    assert!(item.unread);
    assert!(item.has_attachments);
    assert_eq!(item.attachment_count, Some(2));
    assert_eq!(item.attributes[0].key, "custom");
}
