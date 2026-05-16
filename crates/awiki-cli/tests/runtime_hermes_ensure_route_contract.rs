use awiki_cli::runtime::hermes_bridge::{self, EnsureRouteOptions};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn ensure_route_creates_missing_feishu_notify_route_like_go_contract() {
    let home = TempDir::new("creates-feishu-route").expect("temp home");
    std::fs::write(
        home.path().join(".env"),
        "FEISHU_APP_ID=app-id\nFEISHU_APP_SECRET=app-secret\n",
    )
    .expect("write .env");

    let state = hermes_bridge::ensure_route(options(home.path(), "notify", "feishu", 0, ""))
        .expect("ensure route");

    assert_eq!(state.hermes_home, path_string(home.path()));
    assert_eq!(
        state.config_file,
        path_string(&home.path().join("config.yaml"))
    );
    assert_eq!(state.env_file, path_string(&home.path().join(".env")));
    assert_eq!(state.webhook_enabled, true);
    assert_eq!(state.webhook_port, hermes_bridge::DEFAULT_WEBHOOK_PORT);
    assert_eq!(state.route_name, "notify");
    assert_eq!(state.route_configured, true);
    assert_eq!(state.route_secret_configured, true);
    assert!(!state.route_secret.trim().is_empty());
    assert_eq!(state.deliver, "feishu");
    assert_eq!(state.deliver_uses_home_channel, true);
    assert_eq!(state.home_channel_key, "FEISHU_HOME_CHANNEL");
    assert_eq!(state.home_channel_supported, true);
    assert_eq!(state.home_channel_configured, false);
    assert_eq!(state.feishu_credentials_configured, true);
    assert_eq!(
        state.notify_webhook_url,
        "http://127.0.0.1:8644/webhooks/notify"
    );

    let config = read_config(home.path());
    assert_contains(&config, "enabled: true");
    assert_contains(&config, "port: 8644");
    assert_contains(&config, "notify:");
    assert_contains(&config, "secret:");
    assert_contains(&config, state.route_secret.as_str());
    assert_contains(&config, "deliver: feishu");
    assert_contains(&config, "收到外部邮件通知");
    assert_contains(&config, "source_kind=mail");
    assert_contains(&config, "发件邮箱：<from_addr，如存在且与发件人不同>");
    assert_contains(&config, "收到外部IM消息通知");
    assert_not_contains(&config, "skills:");
    assert_not_contains(&config, "chat_id:");
}

#[test]
fn ensure_route_removes_fixed_feishu_targets_and_legacy_notify_skill_like_go_contract() {
    let home = TempDir::new("cleans-fixed-feishu-targets").expect("temp home");
    std::fs::write(
        home.path().join("config.yaml"),
        r#"platforms:
  webhook:
    enabled: true
    extra:
      port: 8644
      routes:
        notify:
          secret: route-secret
          events: []
          prompt: hello
          skills: ["notify"]
          deliver: feishu
          deliver_extra:
            chat_id: oc_fixed
            thread_id: fixed-thread
            message_thread_id: fixed-message-thread
            keep_me: yes
FEISHU_HOME_CHANNEL: oc_home
"#,
    )
    .expect("write config");

    let state = hermes_bridge::ensure_route(options(home.path(), "notify", "feishu", 0, ""))
        .expect("ensure route");

    assert_eq!(state.route_secret, "route-secret");
    assert_eq!(state.route_secret_configured, true);
    assert_eq!(state.deliver, "feishu");
    assert_eq!(state.deliver_uses_home_channel, true);
    assert_eq!(state.home_channel_key, "FEISHU_HOME_CHANNEL");
    assert_eq!(state.home_channel, "oc_home");
    assert_eq!(state.home_channel_configured, true);
    assert_eq!(state.home_channel_supported, true);

    let config = read_config(home.path());
    assert_contains(&config, "secret: route-secret");
    assert_contains(&config, "deliver: feishu");
    assert_contains(&config, "keep_me:");
    assert_not_contains(&config, "chat_id:");
    assert_not_contains(&config, "thread_id:");
    assert_not_contains(&config, "message_thread_id:");
    assert_not_contains(&config, "skills:");
}

#[test]
fn ensure_route_tracks_telegram_home_channel_like_go_contract() {
    let home = TempDir::new("telegram-home-channel").expect("temp home");
    std::fs::write(
        home.path().join("config.yaml"),
        "TELEGRAM_HOME_CHANNEL: tg_home\n",
    )
    .expect("write config");

    let state = hermes_bridge::ensure_route(options(home.path(), "notify", "telegram", 0, ""))
        .expect("ensure route");

    assert_eq!(state.deliver, "telegram");
    assert_eq!(state.deliver_uses_home_channel, true);
    assert_eq!(state.home_channel_key, "TELEGRAM_HOME_CHANNEL");
    assert_eq!(state.home_channel, "tg_home");
    assert_eq!(state.home_channel_configured, true);
    assert_eq!(state.home_channel_supported, true);

    let config = read_config(home.path());
    assert_contains(&config, "TELEGRAM_HOME_CHANNEL: tg_home");
    assert_contains(&config, "deliver: telegram");
    assert_not_contains(&config, "chat_id:");
}

#[test]
fn ensure_route_migrates_legacy_english_prompt_to_current_chinese_default_like_go_contract() {
    let home = TempDir::new("migrates-english-prompt").expect("temp home");
    std::fs::write(
        home.path().join("config.yaml"),
        r#"platforms:
  webhook:
    enabled: true
    extra:
      port: 8644
      routes:
        notify:
          secret: route-secret
          events: []
          prompt: |
            You are an awiki external IM notification formatter.

            Format the incoming notification into one concise IM message suitable for the target platform.
            Rules:
            1. Output only the final notification body.
            2. Do not ask follow-up questions.
            3. Prefer readable sender/recipient names when present.
            4. If a DID exists, include it on a separate line.
            5. Convert time to Asia/Shanghai using YYYY-MM-DD HH:mm (Asia/Shanghai).
            6. Summarize message content in 1 to 5 short lines.
            7. If links are present, list them at the end.

            Suggested layout:
            Received External IM Notification
            Sender: <name or DID>
            Sender DID: <did if present>
            Recipient: <name or DID>
            Recipient DID: <did if present>
            Type: <private/group/state/topic>
            Time: <Asia/Shanghai time>
            Message Summary:
            <1-5 lines>

            Raw notification JSON:
            {notify_payload}
          skills: ["notify"]
          deliver: feishu
"#,
    )
    .expect("write config");

    let state = hermes_bridge::ensure_route(options(home.path(), "notify", "feishu", 0, ""))
        .expect("ensure route");
    assert_eq!(state.route_secret, "route-secret");
    assert_eq!(state.route_configured, true);

    let config = read_config(home.path());
    assert_contains(&config, "收到外部邮件通知");
    assert_contains(&config, "source_kind=mail");
    assert_contains(&config, "不要使用“收到外部IM消息通知”作为标题");
    assert_contains(&config, "发件邮箱：<from_addr，如存在且与发件人不同>");
    assert_contains(&config, "去掉重复署名和邮箱签名");
    assert_not_contains(&config, "Received External IM Notification");
    assert_not_contains(&config, "skills:");
}

#[test]
fn ensure_route_preserves_custom_prompt_and_removes_single_notify_skill_like_go_contract() {
    let home = TempDir::new("preserves-custom-prompt").expect("temp home");
    std::fs::write(
        home.path().join("config.yaml"),
        r#"platforms:
  webhook:
    enabled: true
    extra:
      port: 8644
      routes:
        notify:
          secret: route-secret
          events: []
          prompt: |
            自定义提示词：请保持这一段不被覆盖。
          skills: ["notify"]
          deliver: feishu
"#,
    )
    .expect("write config");

    let state = hermes_bridge::ensure_route(options(home.path(), "notify", "feishu", 0, ""))
        .expect("ensure route");
    assert_eq!(state.route_secret, "route-secret");

    let config = read_config(home.path());
    assert_contains(&config, "自定义提示词：请保持这一段不被覆盖。");
    assert_not_contains(&config, "skills:");
    assert_not_contains(&config, "source_kind=mail");
}

#[test]
fn ensure_route_preserves_custom_non_notify_skills_like_go_contract() {
    let home = TempDir::new("preserves-custom-skills").expect("temp home");
    std::fs::write(
        home.path().join("config.yaml"),
        r#"platforms:
  webhook:
    enabled: true
    extra:
      port: 8644
      routes:
        notify:
          secret: route-secret
          events: []
          prompt: |
            自定义提示词：保留其它自定义 skills。
          skills: ["custom-skill", "formatter"]
          deliver: feishu
"#,
    )
    .expect("write config");

    let state = hermes_bridge::ensure_route(options(home.path(), "notify", "feishu", 0, ""))
        .expect("ensure route");
    assert_eq!(state.route_secret, "route-secret");

    let config = read_config(home.path());
    assert_contains(&config, "自定义提示词：保留其它自定义 skills。");
    assert_contains(&config, "custom-skill");
    assert_contains(&config, "formatter");
}

#[test]
fn ensure_route_preserves_unmanaged_hermes_config_blocks_like_go_map_update_contract() {
    let home = TempDir::new("preserves-unmanaged-blocks").expect("temp home");
    std::fs::write(
        home.path().join("config.yaml"),
        r#"LOG_LEVEL: debug
platforms:
  telegram:
    enabled: true
    token: tg-token
  webhook:
    timeout: 30
    enabled: false
    extra:
      retries: 2
      port: 8644
      routes:
        other:
          secret: other-secret
          deliver: log
        notify:
          secret: route-secret
          events: []
          prompt: hello
          deliver: feishu
custom_top:
  nested: keep
"#,
    )
    .expect("write config");

    let state = hermes_bridge::ensure_route(options(home.path(), "notify", "feishu", 0, ""))
        .expect("ensure route");
    assert_eq!(state.route_secret, "route-secret");
    assert_eq!(state.webhook_enabled, true);

    let config = read_config(home.path());
    assert_contains(&config, "LOG_LEVEL: debug");
    assert_contains(&config, "custom_top:");
    assert_contains(&config, "nested: keep");
    assert_contains(&config, "telegram:");
    assert_contains(&config, "token: tg-token");
    assert_contains(&config, "timeout: 30");
    assert_contains(&config, "retries: 2");
    assert_contains(&config, "other:");
    assert_contains(&config, "secret: other-secret");
    assert_contains(&config, "enabled: true");
    assert_contains(&config, "deliver: feishu");
}

#[test]
fn ensure_route_preserves_existing_route_events_like_go_contract() {
    let home = TempDir::new("preserves-events").expect("temp home");
    std::fs::write(
        home.path().join("config.yaml"),
        r#"platforms:
  webhook:
    enabled: true
    extra:
      port: 8644
      routes:
        notify:
          secret: route-secret
          events:
            - im.message.received
            - mail.message.received
          prompt: hello
          deliver: feishu
"#,
    )
    .expect("write config");

    let state = hermes_bridge::ensure_route(options(home.path(), "notify", "feishu", 0, ""))
        .expect("ensure route");
    assert_eq!(state.route_secret, "route-secret");

    let config = read_config(home.path());
    assert_contains(&config, "events:");
    assert_contains(&config, "- im.message.received");
    assert_contains(&config, "- mail.message.received");
}

fn options(
    hermes_home: &Path,
    route_name: &str,
    deliver: &str,
    webhook_port: u32,
    prompt: &str,
) -> EnsureRouteOptions {
    EnsureRouteOptions {
        hermes_home: path_string(hermes_home),
        route_name: route_name.to_string(),
        deliver: deliver.to_string(),
        webhook_port,
        prompt: prompt.to_string(),
    }
}

fn read_config(home: &Path) -> String {
    std::fs::read_to_string(home.join("config.yaml")).expect("config.yaml should exist")
}

fn assert_contains(haystack: &str, needle: &str) {
    assert!(
        haystack.contains(needle),
        "expected config to contain {needle:?}, got {haystack:?}"
    );
}

fn assert_not_contains(haystack: &str, needle: &str) {
    assert!(
        !haystack.contains(needle),
        "expected config not to contain {needle:?}, got {haystack:?}"
    );
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> std::io::Result<Self> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "awiki-cli-rs2-hermes-ensure-route-{label}-{}-{nonce}",
            std::process::id()
        ));
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

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
