use im_core::compat::email;
use im_core::prelude::*;
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
