use std::collections::BTreeMap;

use im_core::prelude::{
    AuthScope, GroupRef, IdentitySelector, ImError, InboxScope, MessageBody,
    MessageDeliveryOptions, MessageKind, MessageSecurityMode, MessageTarget, SendMessageRequest,
    ThreadRef, VerificationInput,
};

use crate::cli::ParsedCommand;

use super::{auth, config, error, feature_flag, identity, messages, paths, render};

#[test]
fn identity_selector_empty_uses_default() {
    assert!(matches!(
        identity::cli_identity_selector("  "),
        IdentitySelector::Default
    ));
}

#[test]
fn identity_selector_local_alias_trims_input() {
    assert!(matches!(
        identity::cli_identity_selector(" alice "),
        IdentitySelector::LocalAlias(alias) if alias == "alice"
    ));
}

#[test]
fn identity_selector_did_parses_did() {
    assert!(matches!(
        identity::cli_identity_selector("did:example:alice"),
        IdentitySelector::Did(did) if did.as_str() == "did:example:alice"
    ));
}

#[test]
fn register_handle_request_builds_sdk_dto() {
    let mut command = command_with_flags([
        ("handle", "Alice.Awiki.Test"),
        ("otp", " 123456 "),
        ("display-name", "Alice"),
    ]);
    command.globals.identity = "alice".to_string();

    let request = identity::register_handle_request(&command).unwrap();

    assert_eq!(request.local_alias.as_deref(), Some("alice"));
    assert_eq!(request.requested_handle.as_str(), "alice.awiki.test");
    assert!(matches!(
        request.verification,
        VerificationInput::Otp { ref code } if code == "123456"
    ));
    assert_eq!(request.profile.display_name.as_deref(), Some("Alice"));
    assert!(request.make_default);
}

#[test]
fn register_handle_bridge_preserves_legacy_registration_inputs() {
    let command = command_with_flags([
        ("handle", "alice"),
        ("phone", "+15551234567"),
        ("otp", "123456"),
        ("invite-code", "invite-1"),
        ("wait", "true"),
    ]);

    let bridge = identity::register_handle_bridge_request(&command, "alice-local").unwrap();

    assert_eq!(bridge.sdk.local_alias.as_deref(), Some("alice-local"));
    assert_eq!(bridge.sdk.requested_handle.as_str(), "alice");
    assert_eq!(bridge.legacy.identity_name, "alice-local");
    assert_eq!(bridge.legacy.phone, "+15551234567");
    assert_eq!(bridge.legacy.otp, "123456");
    assert_eq!(bridge.legacy.invite_code, "invite-1");
    assert!(bridge.legacy.wait);
}

#[test]
fn recover_handle_bridge_builds_sdk_request_and_preserves_legacy_inputs() {
    let params = crate::identity::RecoverParams {
        identity_name: "ignored".to_string(),
        handle: "Alice".to_string(),
        phone: "13800138000".to_string(),
        otp: " 12 34 56 ".to_string(),
    };
    let bridge = identity::recover_handle_bridge_request(params, None, "awiki.test").unwrap();

    assert_eq!(bridge.sdk.handle.as_str(), "alice.awiki.test");
    assert_eq!(bridge.sdk.phone, "13800138000");
    assert_eq!(bridge.sdk.otp.as_deref(), Some("12 34 56"));
    assert!(bridge.sdk.generated_identity.is_none());
    assert_eq!(bridge.legacy.identity_name, "ignored");
    assert_eq!(bridge.legacy.otp, " 12 34 56 ");
}

#[test]
fn im_error_unsupported_maps_to_phase_unsupported_exit_error() {
    let err = error::map_im_error(
        ImError::UnsupportedCapability {
            capability: "attachments".to_string(),
        },
        "msg send",
    );
    assert_eq!(err.exit_code, 2);
    assert_eq!(err.detail.code, "unsupported_capability");
    assert!(err.detail.message.contains("Phase 1"));
}

#[test]
fn common_im_errors_map_to_exit_errors() {
    let cases = [
        ImError::DefaultIdentityMissing,
        ImError::IdentityRequired,
        ImError::AuthRequired,
        ImError::TransportUnavailable {
            detail: "offline".to_string(),
        },
    ];
    for err in cases {
        let mapped = error::map_im_error(err, "adapter test");
        assert!(mapped.exit_code > 0);
        assert!(!mapped.detail.code.is_empty());
        assert!(!mapped.detail.message.is_empty());
    }
}

#[test]
fn auth_scope_from_cli_accepts_phase1_scopes() {
    assert_eq!(
        auth::auth_scope_from_cli("").unwrap(),
        AuthScope::UserProfile
    );
    assert_eq!(
        auth::auth_scope_from_cli("messaging").unwrap(),
        AuthScope::Messaging
    );
    assert_eq!(
        auth::auth_scope_from_cli("group-messaging").unwrap(),
        AuthScope::GroupMessaging
    );
}

#[test]
fn env_flag_enabled_accepts_explicit_truthy_values() {
    for value in ["1", "true", "YES", " on "] {
        assert!(feature_flag::env_flag_enabled(value));
    }
    for value in ["", "0", "false", "off", "anything-else"] {
        assert!(!feature_flag::env_flag_enabled(value));
    }
}

#[test]
fn build_im_core_config_from_parts_maps_fields() {
    let cfg = config::build_im_core_config_from_parts(
        "https://example.test/",
        "awiki.test",
        Some("https://users.example.test"),
        Some("https://messages.example.test"),
        "websocket",
    )
    .unwrap();
    assert_eq!(cfg.service_base_url.as_str(), "https://example.test");
    assert_eq!(cfg.did_domain, "awiki.test");
    assert_eq!(
        cfg.user_service_endpoint.unwrap().as_str(),
        "https://users.example.test"
    );
    assert_eq!(
        cfg.message_service_endpoint.unwrap().as_str(),
        "https://messages.example.test"
    );
    assert_eq!(
        cfg.transport_policy,
        im_core::MessageTransportPolicy::RealtimePreferred
    );
}

#[test]
fn build_im_core_paths_from_parts_maps_workspace_paths() {
    let paths = paths::build_im_core_paths_from_parts(
        "/tmp/awiki/identities",
        "/tmp/awiki/data/awiki-cli.db",
        "/tmp/awiki/cache",
        "/tmp/awiki/runtime",
    )
    .unwrap();
    assert_eq!(
        paths.identities.identity_root_dir.to_string_lossy(),
        "/tmp/awiki/identities"
    );
    assert_eq!(
        paths.identities.registry_path.to_string_lossy(),
        "/tmp/awiki/identities/index.json"
    );
    assert_eq!(
        paths.local_state.sqlite_path.to_string_lossy(),
        "/tmp/awiki/data/awiki-cli.db"
    );
    assert_eq!(
        paths.runtime.cache_dir.to_string_lossy(),
        "/tmp/awiki/cache"
    );
    assert_eq!(
        paths.runtime.temp_dir.to_string_lossy(),
        "/tmp/awiki/runtime/tmp"
    );
}

#[test]
fn send_message_request_builds_direct_text_dto() {
    let command = command_with_flags([("to", "bob"), ("text", "hello"), ("type", "text")]);
    let request = messages::send_message_request(&command, "awiki.test").unwrap();
    assert!(matches!(request.target, MessageTarget::Direct(_)));
    assert!(matches!(
        request.body,
        MessageBody::Text { text, .. } if text == "hello"
    ));
    assert_eq!(request.security, MessageSecurityMode::DefaultPlain);
}

#[test]
fn legacy_direct_text_send_request_maps_sdk_dto_to_legacy_request() {
    let command = command_with_flags([
        ("to", "bob"),
        ("text", "hello"),
        ("type", "markdown"),
        ("secure", "plain"),
    ]);
    let request = messages::send_message_request(&command, "awiki.test").unwrap();
    let legacy = messages::legacy_text_send_request("alice", request).unwrap();

    assert_eq!(legacy.identity_name, "alice");
    assert_eq!(legacy.target, "bob.awiki.test");
    assert_eq!(legacy.text, "hello");
    assert_eq!(legacy.message_type, "markdown");
    assert_eq!(legacy.secure_mode, "off");
    assert_eq!(legacy.group, "");
}

#[test]
fn legacy_text_send_request_maps_group_dto_to_legacy_request() {
    let request = SendMessageRequest {
        target: MessageTarget::Group(GroupRef::parse("did:example:group").unwrap()),
        body: MessageBody::Text {
            text: "hello group".to_string(),
            kind: MessageKind::Text,
        },
        security: MessageSecurityMode::Plain,
        client_message_id: None,
        delivery: MessageDeliveryOptions::default(),
    };
    let legacy = messages::legacy_text_send_request("alice", request).unwrap();
    assert_eq!(legacy.identity_name, "alice");
    assert_eq!(legacy.target, "");
    assert_eq!(legacy.group, "did:example:group");
    assert_eq!(legacy.text, "hello group");
    assert_eq!(legacy.message_type, "text");
    assert_eq!(legacy.secure_mode, "off");
}

#[test]
fn send_message_request_builds_group_text_dto() {
    let command = command_with_flags([("group", "did:example:group"), ("text", "hello group")]);
    let request = messages::send_message_request(&command, "awiki.test").unwrap();
    assert!(matches!(
        request.target,
        MessageTarget::Group(ref group) if group == &GroupRef::parse("did:example:group").unwrap()
    ));
}

#[test]
fn history_request_builds_direct_thread_and_query() {
    let command = command_with_flags([("with", "bob"), ("limit", "5"), ("cursor", "abc")]);
    let (thread, query) = messages::history_request(&command, "awiki.test").unwrap();
    assert!(matches!(thread, ThreadRef::Direct(_)));
    assert_eq!(query.limit.0, 5);
    assert_eq!(query.cursor.unwrap().as_str(), "abc");
}

#[test]
fn inbox_query_builds_scope_limit_cursor_and_unread_flag() {
    let command = command_with_flags([
        ("scope", "group"),
        ("limit", "7"),
        ("cursor", "page-2"),
        ("unread", "true"),
    ]);
    let query = messages::inbox_query(&command).unwrap();
    assert_eq!(query.scope, InboxScope::GroupOnly);
    assert_eq!(query.limit.0, 7);
    assert_eq!(query.cursor.unwrap().as_str(), "page-2");
    assert!(query.unread_only);
}

#[test]
fn legacy_inbox_request_maps_query_without_filters_or_mark_read() {
    let command = command_with_flags([("scope", "group"), ("limit", "7"), ("unread", "true")]);
    let query = messages::inbox_query(&command).unwrap();
    let legacy = messages::legacy_inbox_request("alice", query).unwrap();

    assert_eq!(legacy.identity_name, "alice");
    assert_eq!(legacy.scope, "group");
    assert_eq!(legacy.with, "");
    assert_eq!(legacy.group, "");
    assert_eq!(legacy.limit, 7);
    assert!(legacy.unread_only);
    assert!(!legacy.mark_read);
}

#[test]
fn legacy_inbox_request_rejects_cursor_bridge() {
    let command = command_with_flags([("cursor", "page-2")]);
    let query = messages::inbox_query(&command).unwrap();
    let err = messages::legacy_inbox_request("alice", query).unwrap_err();

    assert_eq!(err.detail.code, "unsupported_capability");
    assert!(err.detail.message.contains("cursor"));
}

#[test]
fn legacy_history_request_maps_direct_thread_to_legacy_request() {
    let command = command_with_flags([("with", "bob"), ("limit", "5"), ("cursor", "abc")]);
    let (thread, query) = messages::history_request(&command, "awiki.test").unwrap();
    let legacy = messages::legacy_history_request("alice", thread, query).unwrap();

    assert_eq!(legacy.identity_name, "alice");
    assert_eq!(legacy.with, "bob.awiki.test");
    assert_eq!(legacy.limit, 5);
    assert_eq!(legacy.cursor, "abc");
}

#[test]
fn legacy_history_request_rejects_group_thread_for_cli_contract() {
    let command = command_with_flags([("group", "did:example:group")]);
    let (thread, query) = messages::history_request(&command, "awiki.test").unwrap();
    let err = messages::legacy_history_request("alice", thread, query).unwrap_err();

    assert_eq!(err.detail.code, "unsupported_capability");
    assert!(err.detail.message.contains("group history"));
}

#[test]
fn send_message_request_rejects_attachments_without_legacy_send() {
    let command = command_with_flags([("to", "bob"), ("text", "caption"), ("file", "a.png")]);
    let err = messages::send_message_request(&command, "awiki.test").unwrap_err();
    assert_eq!(err.detail.code, "unsupported_capability");
}

#[test]
fn render_success_envelope_wraps_sdk_value() {
    let meta = crate::output::Meta {
        version: "test".to_string(),
        identity: None,
        dry_run: false,
        format: "json".to_string(),
    };
    let envelope = render::success_envelope_for_sdk_value(
        "awiki-cli adapter test",
        &vec!["ok"],
        meta,
        "done",
        vec![],
    )
    .unwrap();
    assert!(envelope.ok);
    assert_eq!(envelope.command, "awiki-cli adapter test");
    assert_eq!(envelope.summary, "done");
}

fn command_with_flags<const N: usize>(flags: [(&str, &str); N]) -> ParsedCommand {
    ParsedCommand {
        flags: flags
            .into_iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect::<BTreeMap<_, _>>(),
        changed_flags: Vec::new(),
        ..ParsedCommand::default()
    }
}
