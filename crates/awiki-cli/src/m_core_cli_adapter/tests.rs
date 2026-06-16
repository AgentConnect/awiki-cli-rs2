use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use im_core::prelude::{
    AttachmentInput, AuthScope, GroupRef, IdentitySelector, ImError, InboxScope, MessageBody,
    MessageId, MessageKind, MessageSecurityMode, MessageTarget, ThreadRef, VerificationInput,
};

use crate::cli_parser::ParsedCommand;

use super::{auth, core_config, error, identity, messages, paths, render};

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
        ("phone", "+15551234567"),
        ("otp", " 123456 "),
        ("invite-code", "invite-1"),
        ("display-name", "Alice"),
    ]);
    command.globals.identity = "alice".to_string();

    let request = identity::register_handle_request(&command).unwrap();

    assert_eq!(request.local_alias.as_deref(), Some("alice"));
    assert_eq!(request.requested_handle.as_str(), "alice.awiki.test");
    assert!(matches!(
        request.verification,
        VerificationInput::Phone { ref phone, ref otp } if phone == "+15551234567"
            && otp.as_deref() == Some("123456")
    ));
    assert_eq!(request.invite_code.as_deref(), Some("invite-1"));
    assert_eq!(request.profile.display_name.as_deref(), Some("Alice"));
    assert!(request.make_default);
}

#[test]
fn register_handle_request_builds_email_sdk_dto() {
    let mut command = command_with_flags([
        ("handle", "alice"),
        ("email", " alice@example.test "),
        ("wait", "true"),
    ]);
    command.globals.identity = "alice".to_string();

    let request = identity::register_handle_request(&command).unwrap();

    assert!(matches!(
        request.verification,
        VerificationInput::Email {
            ref email,
            wait_for_verification: true,
        } if email == "alice@example.test"
    ));
    assert_eq!(request.invite_code, None);
}

#[test]
fn register_handle_command_request_uses_cli_identity_alias() {
    let command = command_with_flags([
        ("handle", "alice"),
        ("phone", "+15551234567"),
        ("otp", "123456"),
        ("invite-code", "invite-1"),
        ("wait", "true"),
    ]);

    let request = identity::register_handle_command_request(&command, "alice-local").unwrap();

    assert_eq!(request.local_alias.as_deref(), Some("alice-local"));
    assert_eq!(request.requested_handle.as_str(), "alice");
    assert!(matches!(
        request.verification,
        VerificationInput::Phone { ref phone, ref otp } if phone == "+15551234567"
            && otp.as_deref() == Some("123456")
    ));
    assert_eq!(request.invite_code.as_deref(), Some("invite-1"));
}

#[test]
fn recover_handle_request_builds_sdk_request() {
    let request = identity::recover_handle_request(
        "Alice".to_string(),
        "13800138000".to_string(),
        Some("12 34 56".to_string()),
        None,
        "awiki.test",
    )
    .unwrap();

    assert_eq!(request.handle.as_str(), "alice.awiki.test");
    assert_eq!(request.raw_handle.as_deref(), Some("Alice"));
    assert_eq!(request.phone, "13800138000");
    assert_eq!(request.otp.as_deref(), Some("12 34 56"));
    assert!(request.generated_identity.is_none());
    assert!(request.local_finalize.is_none());
}

#[test]
fn replace_did_plan_command_request_builds_sdk_plan_request() {
    let workspace = TempDir::new("replace-did-plan-command").expect("workspace");
    let paths = crate::workspace_config::Paths {
        workspace_home_dir: workspace.path().to_string_lossy().into_owned(),
        root_dir: workspace.path().to_string_lossy().into_owned(),
        config_dir: workspace
            .path()
            .join("config")
            .to_string_lossy()
            .into_owned(),
        data_dir: workspace.path().join("data").to_string_lossy().into_owned(),
        state_dir: workspace
            .path()
            .join("state")
            .to_string_lossy()
            .into_owned(),
        cache_dir: workspace
            .path()
            .join("cache")
            .to_string_lossy()
            .into_owned(),
        logs_dir: workspace.path().join("logs").to_string_lossy().into_owned(),
        config_file: workspace
            .path()
            .join("config.yaml")
            .to_string_lossy()
            .into_owned(),
        identity_dir: workspace
            .path()
            .join("identities")
            .to_string_lossy()
            .into_owned(),
        database_file: workspace
            .path()
            .join("data")
            .join("awiki.db")
            .to_string_lossy()
            .into_owned(),
        legacy_credentials_dir: workspace
            .path()
            .join("legacy")
            .to_string_lossy()
            .into_owned(),
        legacy_data_dir: workspace
            .path()
            .join("legacy-data")
            .to_string_lossy()
            .into_owned(),
    };
    let resolved = crate::workspace_config::Resolved {
        paths: paths.clone(),
        config_schema_version: crate::workspace_config::CONFIG_SCHEMA_VERSION,
        active_identity: "alice".to_string(),
        runtime_mode: "http".to_string(),
        runtime_socket_path: String::new(),
        runtime_listener_enabled: false,
        runtime_listener_auto_install: false,
        runtime_listener_auto_start: false,
        host_notify_enabled: false,
        host_notify_sink: "log".to_string(),
        host_notify_file_path: String::new(),
        host_notify_openclaw_hook_url: String::new(),
        host_notify_openclaw_agent_id: String::new(),
        host_notify_openclaw_hook_name: String::new(),
        host_notify_hermes_notify_url: String::new(),
        host_notify_hermes_deliver: String::new(),
        output_format: "json".to_string(),
        no_color: false,
        service_base_url: "https://example.test".to_string(),
        user_service_endpoint: "https://users.example.test".to_string(),
        message_service_endpoint: "https://messages.example.test".to_string(),
        did_domain: "awiki.test".to_string(),
        anp_service_endpoint: "https://example.test/anp-im/rpc".to_string(),
        anp_service_did: "did:wba:example.test".to_string(),
        mail_service_url: "https://example.test".to_string(),
        ca_bundle: String::new(),
        update_disable_strict_version: false,
        update_metadata_cache_ttl_seconds: 0,
        config_exists: false,
        config_error: String::new(),
        env_hits: Vec::new(),
        sources: BTreeMap::new(),
    };
    let generated_did = "did:wba:awiki.test:alice:e1_old".to_string();
    std::fs::create_dir_all(&resolved.paths.identity_dir).expect("identity dir");
    std::fs::write(
        std::path::Path::new(&resolved.paths.identity_dir).join("index.json"),
        serde_json::json!({
            "default_identity": "alice",
            "identities": [{
                "id": "e1_old",
                "did": generated_did,
                "dir_name": "alice",
                "handle": "alice.awiki.test",
                "display_name": "Alice",
                "local_alias": "alice",
                "is_default": true,
                "ready_for_auth": true,
                "ready_for_messaging": true,
                "missing": [],
            }]
        })
        .to_string(),
    )
    .expect("write identity registry");
    std::fs::write(
        std::path::Path::new(&resolved.paths.identity_dir).join("default"),
        "alice\n",
    )
    .expect("write default identity");
    let request = identity::replace_did_plan_command_request(
        &resolved,
        "alice",
        Some(false),
        Some(true),
        Some(""),
        Some("https://example.test/agent"),
    )
    .unwrap();

    assert_eq!(request.identity_name, "alice");
    assert_eq!(request.sdk.identity.local_alias.as_deref(), Some("alice"));
    assert_eq!(request.sdk.identity.did.as_str(), generated_did.as_str());
    let expected_replacement_prefix = format!(
        "{}:e1_replacement_",
        generated_did
            .rsplit_once(':')
            .map(|(base, _)| base)
            .unwrap()
    );
    assert!(request
        .sdk
        .planned_new_did
        .as_str()
        .starts_with(&expected_replacement_prefix));
    assert!(request
        .sdk
        .backup_path_preview
        .contains(".legacy-backup/replace-did/<timestamp>"));
    assert!(request.sdk.backup_path_preview.contains("-alice-"));
    assert_eq!(request.sdk.is_public, Some(false));
    assert_eq!(request.sdk.is_agent, Some(true));
    assert_eq!(request.sdk.role.as_deref(), Some(""));
    assert!(request
        .sdk
        .affected_local_state
        .store_rebind_counts
        .is_empty());
}

#[test]
fn build_im_core_config_uses_resolved_service_endpoints() {
    let workspace = TempDir::new("build-im-core-config-endpoints").expect("workspace");
    let resolved = crate::workspace_config::Resolved {
        paths: test_paths(workspace.path()),
        config_schema_version: crate::workspace_config::CONFIG_SCHEMA_VERSION,
        active_identity: "alice".to_string(),
        runtime_mode: "http".to_string(),
        runtime_socket_path: String::new(),
        runtime_listener_enabled: false,
        runtime_listener_auto_install: false,
        runtime_listener_auto_start: false,
        host_notify_enabled: false,
        host_notify_sink: "log".to_string(),
        host_notify_file_path: String::new(),
        host_notify_openclaw_hook_url: String::new(),
        host_notify_openclaw_agent_id: String::new(),
        host_notify_openclaw_hook_name: String::new(),
        host_notify_hermes_notify_url: String::new(),
        host_notify_hermes_deliver: String::new(),
        output_format: "json".to_string(),
        no_color: false,
        service_base_url: "https://base.example.test".to_string(),
        user_service_endpoint: "https://users.example.test".to_string(),
        message_service_endpoint: "https://messages.example.test".to_string(),
        did_domain: "awiki.test".to_string(),
        anp_service_endpoint: "https://anp.example.test/rpc".to_string(),
        anp_service_did: "did:wba:anp.example.test".to_string(),
        mail_service_url: "https://mail.example.test".to_string(),
        ca_bundle: String::new(),
        update_disable_strict_version: false,
        update_metadata_cache_ttl_seconds: 0,
        config_exists: false,
        config_error: String::new(),
        env_hits: Vec::new(),
        sources: BTreeMap::new(),
    };

    let cfg = core_config::build_im_core_config(&resolved).unwrap();

    assert_eq!(cfg.service_base_url.as_str(), "https://base.example.test");
    assert_eq!(
        cfg.user_service_endpoint.unwrap().as_str(),
        "https://users.example.test"
    );
    assert_eq!(
        cfg.message_service_endpoint.unwrap().as_str(),
        "https://messages.example.test"
    );
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> std::io::Result<Self> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("awiki-cli-{prefix}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn test_paths(root: &Path) -> crate::workspace_config::Paths {
    crate::workspace_config::Paths {
        workspace_home_dir: root.to_string_lossy().into_owned(),
        root_dir: root.to_string_lossy().into_owned(),
        config_dir: root.join("config").to_string_lossy().into_owned(),
        data_dir: root.join("data").to_string_lossy().into_owned(),
        state_dir: root.join("state").to_string_lossy().into_owned(),
        cache_dir: root.join("cache").to_string_lossy().into_owned(),
        logs_dir: root.join("logs").to_string_lossy().into_owned(),
        config_file: root.join("config.yaml").to_string_lossy().into_owned(),
        identity_dir: root.join("identities").to_string_lossy().into_owned(),
        database_file: root
            .join("data")
            .join("awiki.db")
            .to_string_lossy()
            .into_owned(),
        legacy_credentials_dir: root.join("legacy").to_string_lossy().into_owned(),
        legacy_data_dir: root.join("legacy-data").to_string_lossy().into_owned(),
    }
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
fn build_im_core_config_from_parts_maps_fields() {
    let cfg = core_config::build_im_core_config_from_parts(
        "https://example.test/",
        "awiki.test",
        Some("https://users.example.test"),
        Some("https://messages.example.test"),
        Some("https://mail.example.test"),
        Some("https://anp.example.test/rpc"),
        Some("did:wba:anp.example.test"),
        Some("/tmp/awiki-ca.pem"),
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
        cfg.mail_service_endpoint.unwrap().as_str(),
        "https://mail.example.test"
    );
    assert_eq!(
        cfg.anp_service_endpoint.unwrap().as_str(),
        "https://anp.example.test/rpc"
    );
    assert_eq!(
        cfg.anp_service_did.unwrap().as_str(),
        "did:wba:anp.example.test"
    );
    assert_eq!(cfg.ca_bundle.as_deref(), Some("/tmp/awiki-ca.pem"));
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
    let (request, warnings) = messages::send_message_request(&command, "awiki.test").unwrap();
    assert!(warnings.is_empty());
    assert!(matches!(request.target, MessageTarget::Direct(_)));
    assert!(matches!(
        request.body,
        MessageBody::Text { text, .. } if text == "hello"
    ));
    assert_eq!(request.security, MessageSecurityMode::DefaultPlain);
}

#[test]
fn send_message_request_builds_group_text_dto() {
    let command = command_with_flags([("group", "did:example:group"), ("text", "hello group")]);
    let (request, warnings) = messages::send_message_request(&command, "awiki.test").unwrap();
    assert!(warnings.is_empty());
    assert!(matches!(
        request.target,
        MessageTarget::Group(ref group) if group == &GroupRef::parse("did:example:group").unwrap()
    ));
}

#[test]
fn send_message_request_builds_group_payload_dto() {
    let command = command_with_flags([
        ("group", "did:example:group"),
        ("payload", r#"{"text":"@agent hi","mentions":[]}"#),
        ("client-message-id", "msg_payload_1"),
        ("idempotency-key", "idem-payload-1"),
    ]);
    let (request, warnings) = messages::send_message_request(&command, "awiki.test").unwrap();

    assert!(warnings.is_empty());
    assert!(matches!(
        request.target,
        MessageTarget::Group(ref group) if group == &GroupRef::parse("did:example:group").unwrap()
    ));
    assert!(matches!(
        request.body,
        MessageBody::Payload { ref payload }
            if payload["text"] == "@agent hi"
                && payload["mentions"].as_array().is_some_and(Vec::is_empty)
    ));
    assert_eq!(
        request.client_message_id.as_ref().map(MessageId::as_str),
        Some("msg_payload_1")
    );
    assert_eq!(
        request.delivery.idempotency_key.as_deref(),
        Some("idem-payload-1")
    );
}

#[test]
fn send_message_request_rejects_payload_text_conflict() {
    let command = command_with_flags([
        ("to", "bob"),
        ("payload", r#"{"text":"structured"}"#),
        ("text", "plain"),
    ]);
    let err = messages::send_message_request(&command, "awiki.test").unwrap_err();

    assert_eq!(err.detail.code, "invalid_argument");
    assert!(err
        .detail
        .message
        .contains("--payload/--payload-file cannot be combined"));
}

#[test]
fn send_message_request_accepts_client_message_id_and_idempotency_key() {
    let command = command_with_flags([
        ("to", "bob"),
        ("text", "hello"),
        ("client-message-id", "msg_agent_im_run_001"),
        ("idempotency-key", "agent-im-run-001"),
    ]);
    let (request, warnings) = messages::send_message_request(&command, "awiki.test").unwrap();

    assert!(warnings.is_empty());
    assert_eq!(
        request.client_message_id.as_ref().map(MessageId::as_str),
        Some("msg_agent_im_run_001")
    );
    assert_eq!(
        request.delivery.idempotency_key.as_deref(),
        Some("agent-im-run-001")
    );
}

#[test]
fn send_message_request_builds_markdown_plain_direct_sdk_dto() {
    let command = command_with_flags([
        ("to", "bob"),
        ("text", "hello"),
        ("type", "markdown"),
        ("secure", "plain"),
    ]);
    let (request, warnings) = messages::send_message_request(&command, "awiki.test").unwrap();

    assert!(matches!(
        request.target,
        MessageTarget::Direct(ref peer) if peer.as_str() == "bob.awiki.test"
    ));
    assert!(matches!(
        request.body,
        MessageBody::Text {
            ref text,
            kind: MessageKind::Markdown
        } if text == "hello"
    ));
    assert_eq!(request.security, MessageSecurityMode::Plain);
    assert_eq!(
        warnings,
        vec!["--secure plain is deprecated; use --secure off."]
    );
}

#[test]
fn send_message_request_maps_secure_flag_to_e2ee_required_policy() {
    let direct = command_with_flags([("to", "bob"), ("text", "hello"), ("secure", "on")]);
    let (direct_request, direct_warnings) =
        messages::send_message_request(&direct, "awiki.test").unwrap();
    assert_eq!(direct_request.security, MessageSecurityMode::E2eeRequired);
    assert_eq!(
        direct_warnings,
        vec!["--secure on is deprecated; use --secure required."]
    );

    let group = command_with_flags([
        ("group", "did:example:group"),
        ("text", "hello group"),
        ("secure", "e2ee"),
    ]);
    let (group_request, group_warnings) =
        messages::send_message_request(&group, "awiki.test").unwrap();
    assert_eq!(group_request.security, MessageSecurityMode::E2eeRequired);
    assert_eq!(
        group_warnings,
        vec!["--secure e2ee is deprecated; use --secure required."]
    );
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
fn history_request_builds_group_thread_and_query() {
    let command = command_with_flags([
        ("group", "did:example:group"),
        ("limit", "7"),
        ("cursor", "group-2"),
    ]);
    let (thread, query) = messages::history_request(&command, "awiki.test").unwrap();
    assert!(matches!(
        thread,
        ThreadRef::Group(ref group) if group == &GroupRef::parse("did:example:group").unwrap()
    ));
    assert_eq!(query.limit.0, 7);
    assert_eq!(query.cursor.unwrap().as_str(), "group-2");
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
fn send_message_request_builds_attachment_sdk_dto() {
    let command = command_with_flags([("to", "bob"), ("text", "caption"), ("file", "a.png")]);
    let (request, warnings) = messages::send_message_request(&command, "awiki.test").unwrap();
    assert!(warnings.is_empty());

    assert!(matches!(
        request.target,
        MessageTarget::Direct(ref peer) if peer.as_str() == "bob.awiki.test"
    ));
    assert!(matches!(
        request.body,
        MessageBody::Attachment {
            input: AttachmentInput::LocalFile(ref path),
            ref caption,
            mime_type: None,
            filename: None,
        } if path == Path::new("a.png") && caption.as_deref() == Some("caption")
    ));
    assert_eq!(request.security, MessageSecurityMode::DefaultPlain);
}

#[test]
fn render_success_envelope_wraps_sdk_value() {
    let meta = crate::cli_output::Meta {
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
