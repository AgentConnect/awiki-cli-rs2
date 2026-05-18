use awiki_cli::authsdk::{HttpError, RpcError};
use awiki_cli::config::{Paths, Resolved};
use awiki_cli::mail::{
    account_summary, attachment_summary, build_account_rpc_call, build_attachment_rpc_call,
    build_inbox_rpc_call, build_mark_read_rpc_call, build_read_rpc_call, build_send_rpc_call,
    inbox_summary, mark_read_summary, read_summary, send_summary, AccountRequest,
    AttachmentRequest, Client, InboxRequest, MailError, MarkReadRequest, ReadRequest, SendRequest,
    ServiceError, MAIL_RPC_ENDPOINT,
};
use awiki_cli::transportcfg::Profile;
use serde_json::{json, Value};

#[test]
fn mail_rpc_calls_match_go_methods_profiles_and_params() {
    let inbox = build_inbox_rpc_call(InboxRequest {
        identity_name: "alice".to_string(),
        folder: String::new(),
        limit: 0,
        offset: 3,
        unread_only: true,
    });
    assert_eq!(inbox.endpoint, MAIL_RPC_ENDPOINT);
    assert_eq!(inbox.method, "mail.getInbox");
    assert_eq!(inbox.profile, Profile::RpcReadHeavy);
    assert_eq!(
        inbox.params,
        json!({
            "folder": "inbox",
            "limit": 20,
            "offset": 3,
            "unread_only": true,
        })
    );

    let read = build_read_rpc_call(ReadRequest {
        identity_name: "alice".to_string(),
        message_id: "msg-1".to_string(),
    })
    .expect("read rpc call");
    assert_eq!(read.endpoint, MAIL_RPC_ENDPOINT);
    assert_eq!(read.method, "mail.getMessage");
    assert_eq!(read.profile, Profile::RpcReadHeavy);
    assert_eq!(read.params, json!({ "message_id": "msg-1" }));

    let mark_read = build_mark_read_rpc_call(MarkReadRequest {
        identity_name: "alice".to_string(),
        message_ids: vec!["msg-1".to_string(), "msg-2".to_string()],
        is_read: false,
    })
    .expect("mark-read rpc call");
    assert_eq!(mark_read.method, "mail.markRead");
    assert_eq!(mark_read.profile, Profile::RpcDefault);
    assert_eq!(
        mark_read.params,
        json!({ "message_ids": ["msg-1", "msg-2"], "is_read": false })
    );

    let account = build_account_rpc_call(AccountRequest {
        identity_name: "alice".to_string(),
    });
    assert_eq!(account.method, "mail.getMailbox");
    assert_eq!(account.profile, Profile::RpcDefault);
    assert_eq!(account.params, json!({}));

    let attachment = build_attachment_rpc_call(AttachmentRequest {
        identity_name: "alice".to_string(),
        message_id: "msg-1".to_string(),
        attachment_index: 2,
    })
    .expect("attachment rpc call");
    assert_eq!(attachment.method, "mail.getAttachment");
    assert_eq!(attachment.profile, Profile::RpcReadHeavy);
    assert_eq!(
        attachment.params,
        json!({ "message_id": "msg-1", "attachment_index": 2 })
    );

    let send = build_send_rpc_call(SendRequest {
        identity_name: "alice".to_string(),
        to: vec!["bob@example.com".to_string()],
        cc: vec!["copy@example.com".to_string()],
        subject: "Hello".to_string(),
        body_text: "Body".to_string(),
        body_html: String::new(),
    })
    .expect("send rpc call");
    assert_eq!(send.method, "mail.send");
    assert_eq!(send.profile, Profile::RpcDefault);
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

    let send_html = build_send_rpc_call(SendRequest {
        body_html: "<p>Body</p>".to_string(),
        ..SendRequest {
            to: vec!["bob@example.com".to_string()],
            subject: "Hello".to_string(),
            body_text: "Body".to_string(),
            ..Default::default()
        }
    })
    .expect("html send rpc call");
    assert_eq!(send_html.params["body_html"], "<p>Body</p>");
}

#[test]
fn mail_wire_validation_errors_match_go_service_boundaries() {
    assert!(matches!(
        build_read_rpc_call(ReadRequest::default()).expect_err("missing read id"),
        MailError::MessageIdRequired
    ));
    assert!(matches!(
        build_mark_read_rpc_call(MarkReadRequest::default()).expect_err("missing mark ids"),
        MailError::MessageIdRequired
    ));
    assert!(matches!(
        build_attachment_rpc_call(AttachmentRequest {
            message_id: "msg-1".to_string(),
            attachment_index: -1,
            ..Default::default()
        })
        .expect_err("negative attachment index"),
        MailError::AttachmentIndexZero
    ));
    assert!(matches!(
        build_send_rpc_call(SendRequest::default()).expect_err("missing recipient"),
        MailError::RecipientRequired
    ));
    assert!(matches!(
        build_send_rpc_call(SendRequest {
            to: vec!["bob@example.com".to_string()],
            ..Default::default()
        })
        .expect_err("missing subject"),
        MailError::SubjectRequired
    ));
    assert!(matches!(
        build_send_rpc_call(SendRequest {
            to: vec!["bob@example.com".to_string()],
            subject: "Hello".to_string(),
            ..Default::default()
        })
        .expect_err("missing body"),
        MailError::BodyRequired
    ));
}

#[test]
fn mail_result_summaries_match_go_service_contracts() {
    assert_eq!(
        inbox_summary(
            &json!({ "total": 0, "messages": [{"id": "m1"}, {"id": "m2"}] }),
            "archive"
        ),
        "Loaded 2 messages from archive"
    );
    assert_eq!(
        inbox_summary(&json!({ "total": 3, "messages": [] }), "inbox"),
        "Loaded 3 messages"
    );
    assert_eq!(read_summary("msg-1"), "Loaded message msg-1");
    assert_eq!(
        mark_read_summary(&json!({ "updated": 2.0 })),
        "Marked 2 message(s) as read"
    );
    assert_eq!(account_summary(), "Loaded mailbox account");
    assert_eq!(
        attachment_summary(&json!({ "filename": "report.pdf" }), 1),
        "Fetched attachment report.pdf"
    );
    assert_eq!(
        attachment_summary(&json!({}), 4),
        "Fetched attachment attachment_4"
    );
    assert_eq!(send_summary(), "Mail send request accepted");
}

#[test]
fn mail_service_error_display_matches_go_client_mapping() {
    let rpc: ServiceError = RpcError {
        code: -32602,
        message: "bad params".to_string(),
        data: Some(json!({ "field": "id" })),
    }
    .into();
    assert_eq!(rpc.to_string(), "service rpc error -32602: bad params");
    assert_eq!(rpc.rpc_code, -32602);
    assert_eq!(rpc.data, Some(json!({ "field": "id" })));

    let http: ServiceError = HttpError {
        status_code: 503,
        message: "unavailable".to_string(),
    }
    .into();
    assert_eq!(http.to_string(), "service http error 503: unavailable");
    assert_eq!(http.status_code, 503);

    let plain = ServiceError {
        status_code: 0,
        rpc_code: 0,
        message: "plain".to_string(),
        data: None,
    };
    assert_eq!(plain.to_string(), "plain");
}

#[test]
fn mail_client_rejects_empty_mail_service_url_like_go() {
    let mut resolved = test_resolved();
    resolved.mail_service_url = "  \t  ".to_string();

    let err = Client::new(&resolved).expect_err("empty mail service url should fail");
    assert!(matches!(err, MailError::Internal(_)));
    assert_eq!(err.to_string(), "mail service url is required");
}

fn test_resolved() -> Resolved {
    Resolved {
        paths: Paths {
            workspace_home_dir: String::new(),
            root_dir: String::new(),
            config_dir: String::new(),
            data_dir: String::new(),
            state_dir: String::new(),
            cache_dir: String::new(),
            logs_dir: String::new(),
            config_file: String::new(),
            identity_dir: String::new(),
            database_file: String::new(),
            legacy_credentials_dir: String::new(),
            legacy_data_dir: String::new(),
        },
        config_schema_version: 0,
        active_identity: String::new(),
        runtime_mode: String::new(),
        runtime_socket_path: String::new(),
        runtime_listener_enabled: false,
        runtime_listener_auto_install: false,
        runtime_listener_auto_start: false,
        host_notify_enabled: false,
        host_notify_sink: String::new(),
        host_notify_file_path: String::new(),
        host_notify_openclaw_hook_url: String::new(),
        host_notify_openclaw_agent_id: String::new(),
        host_notify_openclaw_hook_name: String::new(),
        host_notify_hermes_notify_url: String::new(),
        host_notify_hermes_deliver: String::new(),
        output_format: "json".to_string(),
        no_color: false,
        service_base_url: String::new(),
        did_domain: String::new(),
        anp_service_endpoint: String::new(),
        anp_service_did: String::new(),
        mail_service_url: "https://mail.example.test".to_string(),
        ca_bundle: String::new(),
        update_disable_strict_version: false,
        update_metadata_cache_ttl_seconds: 0,
        config_exists: false,
        config_error: String::new(),
        env_hits: Vec::new(),
        sources: Default::default(),
    }
}
