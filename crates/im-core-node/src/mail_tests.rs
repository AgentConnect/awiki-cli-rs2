use crate::client::{
    core_config, mail_attachment_download_request, mail_inbox_query, mail_message_id,
    mark_mail_read_request, send_mail_request,
};
use crate::dto::{
    self, NodeDownloadMailAttachmentInput, NodeMailInboxInput, NodeMarkMailReadInput,
    NodeOpenOptions, NodeSendMailAttachmentInput, NodeSendMailInput,
};
use napi::bindgen_prelude::Buffer;

fn open_options() -> NodeOpenOptions {
    NodeOpenOptions {
        state_root: "/tmp/awiki-im-core-node-mail-tests".to_owned(),
        service_base_url: "https://example.test".to_owned(),
        did_domain: "example.test".to_owned(),
        user_service_endpoint: None,
        message_service_endpoint: None,
        mail_service_endpoint: None,
        anp_service_endpoint: None,
        anp_service_did: None,
        client_version_product: "awiki-cli".to_owned(),
        client_version_release: "0815".to_owned(),
        client_version_version: "1.0.16".to_owned(),
        operation_timeout_ms: None,
        sync_timeout_ms: None,
        multi_device_handle_recovery_enabled: None,
        multi_device_audience: None,
        external_http_allow_insecure_loopback_for_testing: None,
    }
}

fn summary(subject: String) -> im_core::email::EmailMessageSummary {
    im_core::email::EmailMessageSummary {
        id: im_core::email::EmailMessageId::parse("mail-1").unwrap(),
        folder: Some(im_core::email::EmailFolder::parse("inbox").unwrap()),
        from: vec![im_core::email::EmailAddress::parse("sender@example.test").unwrap()],
        to: vec![im_core::email::EmailAddress::parse("me@example.test").unwrap()],
        cc: Vec::new(),
        subject,
        preview: Some("preview".to_owned()),
        received_at: Some("2026-08-18T07:00:00Z".to_owned()),
        sent_at: None,
        unread: true,
        has_attachments: false,
        attachment_count: Some(0),
        attributes: vec![im_core::email::EmailAttribute {
            key: "private".to_owned(),
            value: "must-not-cross".to_owned(),
        }],
    }
}

#[test]
fn mail_endpoint_is_optional_and_maps_to_core_config() {
    let absent = core_config(&open_options()).unwrap();
    assert_eq!(absent.mail_service_endpoint, None);
    assert_eq!(
        absent
            .client_version_info
            .as_ref()
            .map(im_core::ClientVersionInfo::header_value)
            .as_deref(),
        Some("awiki-cli/0815/1.0.16")
    );

    let mut options = open_options();
    options.mail_service_endpoint = Some("https://mail.example.test/".to_owned());
    let explicit = core_config(&options).unwrap();
    assert_eq!(
        explicit.mail_service_endpoint.unwrap().as_str(),
        "https://mail.example.test"
    );
}

#[test]
fn inbox_input_defaults_and_bounds_are_exact() {
    let (query, offset, limit) = mail_inbox_query(None).unwrap();
    assert_eq!(query.folder.as_str(), "inbox");
    assert_eq!(query.limit.0, 20);
    assert_eq!(offset, 0);
    assert_eq!(limit, 20);
    assert!(!query.unread_only);

    let (query, offset, limit) = mail_inbox_query(Some(NodeMailInboxInput {
        folder: Some("archive".to_owned()),
        limit: Some(100),
        offset: Some(u32::MAX),
        unread_only: Some(true),
    }))
    .unwrap();
    assert_eq!(query.folder.as_str(), "archive");
    assert_eq!(query.limit.0, 100);
    assert_eq!(offset, u32::MAX);
    assert_eq!(limit, 100);
    assert!(query.unread_only);

    for input in [
        NodeMailInboxInput {
            folder: None,
            limit: Some(0),
            offset: None,
            unread_only: None,
        },
        NodeMailInboxInput {
            folder: None,
            limit: Some(101),
            offset: None,
            unread_only: None,
        },
        NodeMailInboxInput {
            folder: Some("bad\nfolder".to_owned()),
            limit: None,
            offset: None,
            unread_only: None,
        },
    ] {
        assert_eq!(
            mail_inbox_query(Some(input)).unwrap_err().code,
            "invalid_input"
        );
    }
}

#[test]
fn mark_read_validates_ids_and_always_sets_read_true() {
    let request = mark_mail_read_request(NodeMarkMailReadInput {
        message_ids: vec!["mail-1".to_owned(), "mail-2".to_owned()],
    })
    .unwrap();
    assert!(request.is_read);
    assert_eq!(request.message_ids[0].as_str(), "mail-1");
    assert!(mark_mail_read_request(NodeMarkMailReadInput {
        message_ids: Vec::new(),
    })
    .is_err());
    assert!(mail_message_id(" mail-1".to_owned()).is_err());
    assert!(mail_message_id("mail\n1".to_owned()).is_err());
}

#[test]
fn send_input_preserves_content_and_copies_attachment_bytes() {
    let source = vec![0, 1, 2, 255];
    let request = send_mail_request(NodeSendMailInput {
        to: vec!["to@example.test".to_owned()],
        cc: Some(vec!["cc@example.test".to_owned()]),
        subject: "Subject".to_owned(),
        body_text: "  body content  ".to_owned(),
        attachments: Some(vec![
            NodeSendMailAttachmentInput {
                file_name: "fixture.bin".to_owned(),
                content_type: "application/octet-stream".to_owned(),
                bytes: Buffer::from(source.clone()),
            },
            NodeSendMailAttachmentInput {
                file_name: "empty.txt".to_owned(),
                content_type: "text/plain".to_owned(),
                bytes: Buffer::from(Vec::new()),
            },
        ]),
    })
    .unwrap();
    assert_eq!(request.to[0].as_str(), "to@example.test");
    assert_eq!(request.cc[0].as_str(), "cc@example.test");
    assert_eq!(request.subject, "Subject");
    assert_eq!(request.body_text, "  body content  ");
    assert_eq!(request.body_html, None);
    assert_eq!(request.attachments[0].bytes, source);
    assert!(request.attachments[1].bytes.is_empty());

    for input in [
        NodeSendMailInput {
            to: Vec::new(),
            cc: None,
            subject: "Subject".to_owned(),
            body_text: "Body".to_owned(),
            attachments: None,
        },
        NodeSendMailInput {
            to: vec!["bad address@example.test".to_owned()],
            cc: None,
            subject: "Subject".to_owned(),
            body_text: "Body".to_owned(),
            attachments: None,
        },
        NodeSendMailInput {
            to: vec!["bad\u{7}@example.test".to_owned()],
            cc: None,
            subject: "Subject".to_owned(),
            body_text: "Body".to_owned(),
            attachments: None,
        },
        NodeSendMailInput {
            to: vec!["same@example.test".to_owned()],
            cc: Some(vec!["same@example.test".to_owned()]),
            subject: "Subject".to_owned(),
            body_text: "Body".to_owned(),
            attachments: None,
        },
        NodeSendMailInput {
            to: vec!["to@example.test".to_owned()],
            cc: None,
            subject: " Subject".to_owned(),
            body_text: "Body".to_owned(),
            attachments: None,
        },
        NodeSendMailInput {
            to: vec!["to@example.test".to_owned()],
            cc: None,
            subject: "Subject".to_owned(),
            body_text: " \n ".to_owned(),
            attachments: None,
        },
    ] {
        assert_eq!(send_mail_request(input).unwrap_err().code, "invalid_input");
    }
}

#[test]
fn send_attachment_input_rejects_unsafe_names_types_and_limits() {
    let input = |attachments| NodeSendMailInput {
        to: vec!["to@example.test".to_owned()],
        cc: None,
        subject: "Subject".to_owned(),
        body_text: "Body".to_owned(),
        attachments: Some(attachments),
    };
    let attachment =
        |file_name: &str, content_type: &str, size: usize| NodeSendMailAttachmentInput {
            file_name: file_name.to_owned(),
            content_type: content_type.to_owned(),
            bytes: Buffer::from(vec![0; size]),
        };

    for attachments in [
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
            10 * 1024 * 1024 + 1,
        )],
        (0..11)
            .map(|index| attachment(&format!("{index}.txt"), "text/plain", 0))
            .collect(),
    ] {
        assert_eq!(
            send_mail_request(input(attachments)).unwrap_err().code,
            "invalid_input"
        );
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
    assert_eq!(
        send_mail_request(input(over_total)).unwrap_err().code,
        "invalid_input"
    );
}

#[test]
fn mail_attachment_download_request_and_projection_are_byte_exact() {
    let request = mail_attachment_download_request(NodeDownloadMailAttachmentInput {
        message_id: "mail-1".to_owned(),
        attachment_index: u32::MAX,
    })
    .unwrap();
    assert_eq!(request.message_id.as_str(), "mail-1");
    assert_eq!(request.attachment_index, u32::MAX);

    let download = dto::mail_attachment_download(im_core::email::EmailAttachmentContent {
        message_id: im_core::email::EmailMessageId::parse("mail-1").unwrap(),
        attachment_index: 0,
        filename: "fixture.bin".to_owned(),
        content_type: "application/octet-stream".to_owned(),
        size: Some(4),
        bytes: vec![0, 1, 2, 255],
    })
    .unwrap();
    assert_eq!(download.file_name, "fixture.bin");
    assert_eq!(download.content_type, "application/octet-stream");
    assert_eq!(download.size_bytes, "4");
    assert_eq!(download.bytes.as_ref(), &[0, 1, 2, 255]);

    for invalid in [
        im_core::email::EmailAttachmentContent {
            message_id: im_core::email::EmailMessageId::parse("mail-1").unwrap(),
            attachment_index: 0,
            filename: "../secret".to_owned(),
            content_type: "application/octet-stream".to_owned(),
            size: Some(1),
            bytes: vec![0],
        },
        im_core::email::EmailAttachmentContent {
            message_id: im_core::email::EmailMessageId::parse("mail-1").unwrap(),
            attachment_index: 0,
            filename: "fixture.bin".to_owned(),
            content_type: "invalid".to_owned(),
            size: Some(1),
            bytes: vec![0],
        },
        im_core::email::EmailAttachmentContent {
            message_id: im_core::email::EmailMessageId::parse("mail-1").unwrap(),
            attachment_index: 0,
            filename: "fixture.bin".to_owned(),
            content_type: "application/octet-stream".to_owned(),
            size: Some(2),
            bytes: vec![0],
        },
    ] {
        let error = match dto::mail_attachment_download(invalid) {
            Ok(_) => panic!("invalid mail download must fail closed"),
            Err(error) => error,
        };
        assert_eq!(error.code, "remote_response_invalid");
    }
}

#[test]
fn inbox_projection_bounds_content_and_computes_offset() {
    let page = dto::mail_inbox(
        im_core::ids::Page {
            items: vec![summary("é".repeat(600))],
            next_cursor: None,
            has_more: true,
        },
        40,
        20,
    )
    .unwrap();
    assert_eq!(page.next_offset, Some(41));
    assert!(page.items[0].subject_truncated);
    assert!(page.items[0].subject.len() <= 1_024);
    assert_eq!(page.items[0].from, ["sender@example.test"]);

    assert!(dto::mail_inbox(
        im_core::ids::Page {
            items: Vec::new(),
            next_cursor: None,
            has_more: true,
        },
        0,
        20,
    )
    .is_err());
    assert!(dto::mail_inbox(
        im_core::ids::Page {
            items: vec![summary("subject".to_owned())],
            next_cursor: None,
            has_more: true,
        },
        u32::MAX,
        20,
    )
    .is_err());
}

#[test]
fn message_projection_omits_html_and_attributes_and_truncates_utf8() {
    let message = dto::mail_message(im_core::email::EmailMessage {
        summary: summary("subject".to_owned()),
        body_text: Some("€".repeat(21_846)),
        body_html: Some("<p>must-not-cross</p>".to_owned()),
        attachments: vec![im_core::email::EmailAttachmentMetadata {
            index: 0,
            filename: Some("report.txt".to_owned()),
            content_type: Some("text/plain".to_owned()),
            size: Some(u64::MAX),
        }],
    })
    .unwrap();
    assert!(message.body_truncated);
    assert!(message.body_text.as_ref().unwrap().len() <= 65_536);
    assert!(message.has_html_body);
    assert_eq!(
        message.attachments[0].size_bytes.as_deref(),
        Some("18446744073709551615")
    );
    let debug = format!("{message:?}");
    assert!(!debug.contains("must-not-cross"));
    assert!(!debug.contains("private"));
}

#[test]
fn malformed_remote_mail_fields_fail_closed() {
    let mut bad_timestamp = summary("subject".to_owned());
    bad_timestamp.received_at = Some("not-a-timestamp".to_owned());
    let error = dto::mail_inbox(
        im_core::ids::Page {
            items: vec![bad_timestamp],
            next_cursor: None,
            has_more: false,
        },
        0,
        20,
    )
    .unwrap_err();
    assert_eq!(error.code, "remote_response_invalid");
    assert!(!error.safe_message.contains("not-a-timestamp"));

    let mut bad_address = summary("subject".to_owned());
    bad_address.from =
        vec![im_core::email::EmailAddress::parse("sender\u{7}@example.test").unwrap()];
    let error = dto::mail_inbox(
        im_core::ids::Page {
            items: vec![bad_address],
            next_cursor: None,
            has_more: false,
        },
        0,
        20,
    )
    .unwrap_err();
    assert_eq!(error.code, "remote_response_invalid");
    assert!(!error.safe_message.contains("sender"));
}

#[test]
fn python_naive_mail_timestamp_is_canonicalized_as_utc() {
    let mut value = summary("subject".to_owned());
    value.received_at = None;
    value.sent_at = Some("2026-08-19T09:30:34.123456".to_owned());
    let page = dto::mail_inbox(
        im_core::ids::Page {
            items: vec![value],
            next_cursor: None,
            has_more: false,
        },
        0,
        20,
    )
    .unwrap();
    assert_eq!(
        page.items[0].sent_at.as_deref(),
        Some("2026-08-19T09:30:34.123456Z")
    );
}
