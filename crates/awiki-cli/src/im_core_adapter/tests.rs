use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use im_core::prelude::{
    AuthScope, GroupRef, IdentitySelector, ImError, InboxScope, MessageBody, MessageKind,
    MessageSecurityMode, MessageTarget, ThreadRef, VerificationInput,
};

use crate::cli::ParsedCommand;

use super::{auth, config, error, identity, messages, paths, render};

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
    assert!(matches!(
        bridge.sdk.verification,
        VerificationInput::Phone { ref phone, ref otp } if phone == "+15551234567"
            && otp.as_deref() == Some("123456")
    ));
    assert_eq!(bridge.sdk.invite_code.as_deref(), Some("invite-1"));
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
fn replace_did_plan_bridge_builds_sdk_plan_request() {
    let workspace = TempDir::new("replace-did-plan-bridge").expect("workspace");
    let paths = crate::config::Paths {
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
    let resolved = crate::config::Resolved {
        paths: paths.clone(),
        config_schema_version: crate::config::CONFIG_SCHEMA_VERSION,
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
    let manager = crate::identity::Manager::new(paths);
    let generated = crate::identity::generate_identity_with_path_segments(
        "awiki.test",
        ["alice", "e1_old"],
        "https://example.test/anp-im/rpc",
        "did:wba:example.test",
    )
    .expect("generate identity");
    let generated_did = generated.did.clone();
    manager
        .save(crate::identity::types::SaveInput {
            identity_name: "alice".to_string(),
            did: generated.did,
            unique_id: generated.unique_id,
            display_name: "Alice".to_string(),
            handle: "alice".to_string(),
            full_handle: "alice.awiki.test".to_string(),
            did_document: Some(generated.did_document),
            key1_private_pem: generated.key1_private_pem,
            key1_public_pem: generated.key1_public_pem,
            e2ee_signing_private_pem: generated.e2ee_signing_private_pem,
            e2ee_agreement_private_pem: generated.e2ee_agreement_private_pem,
            ..Default::default()
        })
        .expect("save identity");

    let bridge = identity::replace_did_plan_bridge_request(
        &resolved,
        &manager,
        "alice",
        Some(false),
        Some(true),
        Some(""),
        Some("https://example.test/agent"),
    )
    .unwrap();

    assert_eq!(bridge.identity_name, "alice");
    assert_eq!(bridge.sdk.identity.local_alias.as_deref(), Some("alice"));
    assert_eq!(bridge.sdk.identity.did.as_str(), generated_did.as_str());
    let expected_replacement_prefix = format!(
        "{}:e1_replacement_",
        generated_did
            .rsplit_once(':')
            .map(|(base, _)| base)
            .unwrap()
    );
    assert!(bridge
        .sdk
        .planned_new_did
        .as_str()
        .starts_with(&expected_replacement_prefix));
    assert!(bridge
        .sdk
        .backup_path_preview
        .contains(".legacy-backup/replace-did/<timestamp>"));
    assert!(bridge.sdk.backup_path_preview.contains("-alice-"));
    assert_eq!(bridge.sdk.is_public, Some(false));
    assert_eq!(bridge.sdk.is_agent, Some(true));
    assert_eq!(bridge.sdk.role.as_deref(), Some(""));
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
fn send_message_request_builds_group_text_dto() {
    let command = command_with_flags([("group", "did:example:group"), ("text", "hello group")]);
    let request = messages::send_message_request(&command, "awiki.test").unwrap();
    assert!(matches!(
        request.target,
        MessageTarget::Group(ref group) if group == &GroupRef::parse("did:example:group").unwrap()
    ));
}

#[test]
fn send_message_request_builds_markdown_plain_direct_sdk_dto() {
    let command = command_with_flags([
        ("to", "bob"),
        ("text", "hello"),
        ("type", "markdown"),
        ("secure", "plain"),
    ]);
    let request = messages::send_message_request(&command, "awiki.test").unwrap();

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
