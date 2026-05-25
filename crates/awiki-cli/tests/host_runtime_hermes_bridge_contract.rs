use awiki_cli::host_runtime::hermes_bridge;
use serde_json::{json, Map, Value};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn hermes_bridge_defaults_match_go_constants() {
    assert_eq!(hermes_bridge::DEFAULT_WEBHOOK_PORT, 8644);
    assert_eq!(hermes_bridge::DEFAULT_WEBHOOK_ROUTE_NAME, "notify");
    assert_eq!(
        hermes_bridge::DEFAULT_NOTIFY_URL,
        "http://127.0.0.1:8765/notify/host-event"
    );
    assert_eq!(hermes_bridge::DEFAULT_DELIVER_TARGET, "feishu");
    assert!(hermes_bridge::default_notify_prompt().contains("收到外部邮件通知"));
    assert!(hermes_bridge::default_notify_prompt().contains("source_kind=mail"));
    assert!(hermes_bridge::default_notify_prompt()
        .contains("发件邮箱：<from_addr，如存在且与发件人不同>"));
}

#[test]
fn validate_local_notify_url_matches_go_loopback_contract() {
    let defaulted = hermes_bridge::validate_local_notify_url("   ").expect("default URL");
    assert_eq!(defaulted.url, hermes_bridge::DEFAULT_NOTIFY_URL);
    assert_eq!(defaulted.host, "127.0.0.1");
    assert_eq!(defaulted.port, 8765);

    let localhost =
        hermes_bridge::validate_local_notify_url(" http://localhost/notify/host-event ")
            .expect("localhost");
    assert_eq!(localhost.host, "localhost");
    assert_eq!(localhost.port, 80);

    let empty_port =
        hermes_bridge::validate_local_notify_url("http://127.0.0.1:/notify/host-event")
            .expect("empty port behaves like no port");
    assert_eq!(empty_port.host, "127.0.0.1");
    assert_eq!(empty_port.port, 80);

    let ipv6 = hermes_bridge::validate_local_notify_url("http://[::1]:8765/notify/host-event")
        .expect("ipv6 loopback");
    assert_eq!(ipv6.host, "::1");
    assert_eq!(ipv6.port, 8765);
}

#[test]
fn validate_local_notify_url_rejects_non_go_local_inputs() {
    let https =
        hermes_bridge::validate_local_notify_url("https://127.0.0.1:8765/notify/host-event")
            .expect_err("https rejected");
    assert!(https
        .to_string()
        .contains("notify URL must use http for local Hermes bridge"));

    let remote = hermes_bridge::validate_local_notify_url("http://10.0.0.1:8765/notify/host-event")
        .expect_err("remote rejected");
    assert!(remote.to_string().contains(
        "notify URL host \"10.0.0.1\" is not local; full Hermes setup only supports a local bridge"
    ));

    let bad_port =
        hermes_bridge::validate_local_notify_url("http://127.0.0.1:bad/notify/host-event")
            .expect_err("bad port rejected");
    assert!(bad_port.to_string().contains("parse notify URL"));
    assert!(bad_port
        .to_string()
        .contains("invalid port \":bad\" after host"));

    let zero_port =
        hermes_bridge::validate_local_notify_url("http://127.0.0.1:0/notify/host-event")
            .expect_err("zero port rejected");
    assert!(zero_port
        .to_string()
        .contains("notify URL has invalid port"));
}

#[test]
fn deliver_target_helpers_match_go_contract() {
    assert_eq!(
        hermes_bridge::normalize_deliver_target("  FeiShu  "),
        "feishu"
    );
    assert_eq!(hermes_bridge::normalize_deliver_target(""), "feishu");
    assert!(hermes_bridge::is_supported_deliver_target(""));
    assert!(hermes_bridge::is_supported_deliver_target(" Telegram "));
    assert!(!hermes_bridge::is_supported_deliver_target("custom"));
    assert_eq!(
        hermes_bridge::supported_deliver_targets(),
        vec![
            "bluebubbles",
            "discord",
            "email",
            "feishu",
            "log",
            "matrix",
            "mattermost",
            "qqbot",
            "signal",
            "slack",
            "sms",
            "telegram",
            "wecom",
            "weixin",
        ]
    );
}

#[test]
fn home_channel_env_keys_and_display_names_match_go_contract() {
    let cases = [
        ("bluebubbles", "BLUEBUBBLES_HOME_CHANNEL", "BlueBubbles"),
        ("discord", "DISCORD_HOME_CHANNEL", "Discord"),
        ("email", "EMAIL_HOME_ADDRESS", "Email"),
        ("feishu", "FEISHU_HOME_CHANNEL", "Feishu"),
        ("matrix", "MATRIX_HOME_ROOM", "Matrix"),
        ("mattermost", "MATTERMOST_HOME_CHANNEL", "Mattermost"),
        ("qqbot", "QQ_HOME_CHANNEL", "QQ Bot"),
        ("signal", "SIGNAL_HOME_CHANNEL", "Signal"),
        ("slack", "SLACK_HOME_CHANNEL", "Slack"),
        ("sms", "SMS_HOME_CHANNEL", "SMS"),
        ("telegram", "TELEGRAM_HOME_CHANNEL", "Telegram"),
        ("wecom", "WECOM_HOME_CHANNEL", "WeCom"),
        ("weixin", "WEIXIN_HOME_CHANNEL", "Weixin"),
    ];
    for (target, env_key, display_name) in cases {
        assert_eq!(hermes_bridge::home_channel_env_key(target), env_key);
        assert_eq!(hermes_bridge::deliver_display_name(target), display_name);
    }

    assert_eq!(
        hermes_bridge::home_channel_env_key(""),
        "FEISHU_HOME_CHANNEL"
    );
    assert_eq!(hermes_bridge::home_channel_env_key("log"), "");
    assert_eq!(hermes_bridge::home_channel_env_key("custom"), "");
    assert_eq!(hermes_bridge::deliver_display_name(""), "Feishu");
    assert_eq!(hermes_bridge::deliver_display_name("custom"), "Custom");
}

#[test]
fn read_env_file_matches_go_line_parser_contract() {
    let workspace = TempDir::new().expect("temp workspace");
    let missing = workspace.path().join("missing.env");
    assert!(hermes_bridge::read_env_file(&missing)
        .expect("missing env")
        .is_empty());

    let path = workspace.path().join(".env");
    std::fs::write(
        &path,
        r#"
# comment
export FEISHU_APP_ID = "app-id"
FEISHU_APP_SECRET='secret'
NO_EQUALS
INLINE = value # not a comment
EMPTY_VALUE=
=empty-key
FEISHU_APP_ID=last-wins
"#,
    )
    .expect("write env");

    let values = hermes_bridge::read_env_file(&path).expect("parse env");
    assert_eq!(
        values.get("FEISHU_APP_ID").map(String::as_str),
        Some("last-wins")
    );
    assert_eq!(
        values.get("FEISHU_APP_SECRET").map(String::as_str),
        Some("secret")
    );
    assert_eq!(
        values.get("INLINE").map(String::as_str),
        Some("value # not a comment")
    );
    assert_eq!(values.get("EMPTY_VALUE").map(String::as_str), Some(""));
    assert_eq!(values.get("").map(String::as_str), Some("empty-key"));
    assert!(!values.contains_key("NO_EQUALS"));
}

#[test]
fn inspect_route_reads_local_config_and_env_like_go_status_view() {
    let workspace = TempDir::new().expect("temp workspace");
    std::fs::write(
        workspace.path().join("config.yaml"),
        r#"
TELEGRAM_HOME_CHANNEL: chat-123
platforms:
  webhook:
    enabled: true
    extra:
      port: 8644
      routes:
        notify:
          secret: route-secret
          events: []
          prompt: "{notify_payload}"
          skills: ["notify"]
          deliver: telegram
"#,
    )
    .expect("write Hermes config");
    std::fs::write(
        workspace.path().join(".env"),
        "FEISHU_APP_ID=app-id\nFEISHU_APP_SECRET=app-secret\n",
    )
    .expect("write Hermes env");

    let state =
        hermes_bridge::inspect_route(&path_string(workspace.path()), "notify").expect("route");
    assert_eq!(state.hermes_home, path_string(workspace.path()));
    assert_eq!(
        state.config_file,
        path_string(&workspace.path().join("config.yaml"))
    );
    assert_eq!(state.config_exists, true);
    assert_eq!(state.webhook_enabled, true);
    assert_eq!(state.webhook_port, 8644);
    assert_eq!(state.route_name, "notify");
    assert_eq!(state.route_configured, true);
    assert_eq!(state.route_secret_configured, true);
    assert_eq!(state.deliver, "telegram");
    assert_eq!(state.deliver_uses_home_channel, true);
    assert_eq!(state.home_channel_key, "TELEGRAM_HOME_CHANNEL");
    assert_eq!(state.home_channel, "chat-123");
    assert_eq!(state.home_channel_configured, true);
    assert_eq!(state.home_channel_supported, true);
    assert_eq!(state.feishu_credentials_configured, true);
    assert_eq!(
        state.notify_webhook_url,
        "http://127.0.0.1:8644/webhooks/notify"
    );
    assert!(
        state.warnings.is_empty(),
        "configured route should not warn: {:?}",
        state.warnings
    );
}

#[test]
fn cleanup_deliver_extra_removes_fixed_targets_and_preserves_unrelated_keys() {
    let mut route = object(json!({
        "deliver_extra": {
            "chat_id": "oc_xxx",
            "thread_id": "thread",
            "message_thread_id": "message-thread",
            "z_keep": true,
            "a_keep": "yes"
        }
    }));

    hermes_bridge::cleanup_deliver_extra(&mut route);
    let extra = route
        .get("deliver_extra")
        .and_then(Value::as_object)
        .expect("deliver_extra remains");
    assert_eq!(
        extra.keys().cloned().collect::<Vec<_>>(),
        vec!["a_keep", "z_keep"]
    );
    assert_eq!(extra["a_keep"], "yes");
    assert_eq!(extra["z_keep"], true);

    let mut route = object(json!({
        "deliver_extra": {
            "chat_id": "oc_xxx",
            "thread_id": "thread",
            "message_thread_id": "message-thread"
        }
    }));
    hermes_bridge::cleanup_deliver_extra(&mut route);
    assert!(route.get("deliver_extra").is_none());

    let mut route = object(json!({"deliver_extra": "fixed"}));
    hermes_bridge::cleanup_deliver_extra(&mut route);
    assert_eq!(route["deliver_extra"], "fixed");
}

#[test]
fn cleanup_legacy_notify_skill_matches_go_sequence_contract() {
    let mut route = object(json!({"skills": [" notify "]}));
    hermes_bridge::cleanup_legacy_notify_skill(&mut route);
    assert!(route.get("skills").is_none());

    let mut route = object(json!({"skills": ["Notify"]}));
    hermes_bridge::cleanup_legacy_notify_skill(&mut route);
    assert_eq!(route["skills"], json!(["Notify"]));

    let mut route = object(json!({"skills": ["notify", "formatter"]}));
    hermes_bridge::cleanup_legacy_notify_skill(&mut route);
    assert_eq!(route["skills"], json!(["notify", "formatter"]));

    let mut route = object(json!({"skills": "notify"}));
    hermes_bridge::cleanup_legacy_notify_skill(&mut route);
    assert_eq!(route["skills"], "notify");
}

#[test]
fn should_replace_notify_prompt_matches_go_migration_predicates() {
    assert!(hermes_bridge::should_replace_notify_prompt(""));
    assert!(hermes_bridge::should_replace_notify_prompt(
        r#"
你是 awiki 外部 IM 消息通知整理助手。
收到外部IM消息通知
消息内容摘要：
{notify_payload}
"#
    ));
    assert!(hermes_bridge::should_replace_notify_prompt(
        r#"
你是 awiki 外部消息通知整理助手。
收到外部邮件通知
如果 topic 是 mail.message.received，建议格式：
原始通知 JSON：
{notify_payload}
"#
    ));
    assert!(hermes_bridge::should_replace_notify_prompt(
        r#"
你是 awiki 外部消息通知整理助手。
收到外部邮件通知
必须按邮件通知处理
原始通知 JSON：
{notify_payload}
"#
    ));
    assert!(!hermes_bridge::should_replace_notify_prompt(
        "自定义提示词：请保持这一段不被覆盖。"
    ));
}

fn object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => panic!("value should be an object"),
    }
}

struct TempDir {
    path: std::path::PathBuf,
}

impl TempDir {
    fn new() -> std::io::Result<Self> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "awiki-cli-rs2-hermes-bridge-test-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn path_string(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}
