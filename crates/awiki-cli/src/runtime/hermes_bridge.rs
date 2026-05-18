use crate::config::Resolved;
use crate::runtime::hermes_host_notify;
use anyhow::Context;
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

mod route;
mod service;

pub use route::{ensure_route, EnsureRouteOptions};
pub use service::{
    adapter_command_plan_for, apply_decision_for, apply_service, apply_service_plan,
    apply_service_with_backend, bridge_health_available, bridge_health_available_with,
    bridge_status_ready, ensure_installed, ensure_installed_plan, ensure_installed_with_backend,
    parse_systemd_status, restart_service, restart_service_plan, restart_service_with_backend,
    run_bridge_service, run_bridge_service_with_stop, running_in_bridge_service_mode,
    running_in_bridge_service_mode_with, service_config_plan_for, service_display_name_for,
    service_enabled_by_env_value, service_name_for, service_platform, service_status_snapshot_for,
    start_service, start_service_plan, start_service_with_backend, status_for_with_backend,
    status_from_parts, stop_service, stop_service_plan, stop_service_with_backend,
    systemd_service_supported, systemd_status_snapshot_with_runner, systemd_status_with_runner,
    systemd_unit_for, uninstall_service, uninstall_service_plan, uninstall_service_with_backend,
    unit_name_for, unit_path_for, wait_for_bridge_status_with, BridgeAdapterCommandPlan,
    BridgeAdapterExit, BridgeAdapterProcess, BridgeApplyDecision, BridgeServiceBackend,
    BridgeServiceConfigPlan, BridgeServiceLifecycleOperation, BridgeServiceStatusSnapshot,
    BridgeSystemdCommandRunner, BridgeSystemdStatus, BridgeSystemdUnit, SystemctlCommandRunner,
    SystemdBridgeServiceBackend, BRIDGE_ADAPTER_STOP_TIMEOUT, ENABLE_SYSTEMD_SERVICE_ENV,
    SERVICE_ARGUMENTS, SERVICE_DESCRIPTION, SERVICE_DISPLAY_NAME_PREFIX, SERVICE_NAME_PREFIX,
};

pub const DEFAULT_WEBHOOK_PORT: u32 = 8644;
pub const DEFAULT_WEBHOOK_ROUTE_NAME: &str = "notify";
pub const DEFAULT_NOTIFY_URL: &str = "http://127.0.0.1:8765/notify/host-event";
pub const DEFAULT_DELIVER_TARGET: &str = "feishu";

const SUPPORTED_DELIVER_TARGETS: &[&str] = &[
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
];

const LEGACY_DEFAULT_NOTIFY_PROMPT: &str = r#"
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
"#;

const DEFAULT_NOTIFY_PROMPT_V1: &str = r#"
你是 awiki 外部消息通知整理助手。

请根据收到的通知 topic 和 data，把它整理成一条简洁、稳定、适合目标 IM 平台阅读的中文消息。
规则：
1. 只输出最终通知正文，不要加解释。
2. 不要提问，不要添加无关寒暄。
3. 时间统一转换为 Asia/Shanghai，格式为 YYYY-MM-DD HH:mm (Asia/Shanghai)。
4. 字段标题统一使用中文。
5. 不存在的字段不要臆造，缺失时直接省略对应行。
6. 摘要控制在 1 到 5 行短句内。
7. 如果有链接，放在最后单独列出。
8. `topic=mail.message.received` 时，优先使用邮箱地址字段，如 `from_addr`、`mailbox_address`、`subject`、`preview`。
9. IM 通知优先使用可读的人名、handle 或显示名；没有时再使用 DID。

如果 topic 是 `mail.message.received`，建议格式：
收到外部邮件通知
发件人：<邮箱地址或名称>
收件邮箱：<mailbox_address>
收件人 DID：<recipient_did，如存在>
时间：<Asia/Shanghai 时间>
邮件摘要：
主题：<subject，如存在>
<preview 1-5 行>
附件：<有附件时再展示，例如：有>

如果 topic 是 IM 相关事件，例如 `im.message.received`、`im.group.message.received`、`im.group.state.changed`，建议格式：
收到外部IM消息通知
发送者：<名称或 DID>
发送者 DID：<如存在>
接收者：<名称或 DID>
接收者 DID：<如存在>
类型：<私信/群消息/状态变更/事件>
时间：<Asia/Shanghai 时间>
消息内容摘要：
<1-5 行>

原始通知 JSON：
{notify_payload}
"#;

const DEFAULT_NOTIFY_PROMPT: &str = r#"
你是 awiki 外部消息通知整理助手。

请根据收到的通知 topic 和 data，把它整理成一条简洁、稳定、适合目标 IM 平台阅读的中文消息。
规则：
1. 只输出最终通知正文，不要加解释。
2. 不要提问，不要添加无关寒暄。
3. 时间统一转换为 Asia/Shanghai，格式为 YYYY-MM-DD HH:mm (Asia/Shanghai)。
4. 字段标题统一使用中文。
5. 不存在的字段不要臆造，缺失时直接省略对应行。
6. 摘要控制在 1 到 5 行短句内。
7. 如果有链接，放在最后单独列出。
8. 若 data 中 `source_kind=mail`，或存在 `mailbox_address`、`from_addr`、`subject`、`preview` 等邮件字段，则必须按邮件通知处理，不强依赖 topic 名称。
9. IM 通知优先使用可读的人名、handle 或显示名；没有时再使用 DID。
10. 命中邮件通知时，不要使用“收到外部IM消息通知”作为标题，也不要套用 IM 模板。
11. 处理邮件时，如果 `from_addr` 存在且能识别出发件人姓名，优先把姓名写到“发件人”，并把邮箱单独写到“发件邮箱”。
12. 如果 `preview` 末尾包含类似“姓名 / 邮箱：...”的署名块，应将其从摘要中提取出来，不要原样重复在“邮件摘要”最后。

如果 data 中 `source_kind=mail`，或存在 `mailbox_address`、`from_addr`、`subject`、`preview` 这类邮件字段，必须使用下面这个模板：
收到外部邮件通知
发件人：<姓名；如果没有姓名则用邮箱地址>
发件邮箱：<from_addr，如存在且与发件人不同>
收件邮箱：<mailbox_address>
收件人 DID：<recipient_did，如存在>
时间：<Asia/Shanghai 时间>
邮件摘要：
主题：<subject，如存在>
<preview 1-5 行，去掉重复署名和邮箱签名>
附件：<有附件时再展示，例如：有>

否则，如果 topic 是 IM 相关事件，例如 `im.message.received`、`im.group.message.received`、`im.group.state.changed`，使用下面这个模板：
收到外部IM消息通知
发送者：<名称或 DID>
发送者 DID：<如存在>
接收者：<名称或 DID>
接收者 DID：<如存在>
类型：<私信/群消息/状态变更/事件>
时间：<Asia/Shanghai 时间>
消息内容摘要：
<1-5 行>

原始通知 JSON：
{notify_payload}
"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalNotifyUrl {
    pub url: String,
    pub host: String,
    pub port: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct RouteState {
    pub hermes_home: String,
    pub config_file: String,
    pub env_file: String,
    pub config_exists: bool,
    pub webhook_enabled: bool,
    pub webhook_port: u32,
    pub route_name: String,
    pub route_configured: bool,
    #[serde(skip_serializing)]
    pub route_secret: String,
    pub route_secret_configured: bool,
    pub deliver: String,
    pub deliver_uses_home_channel: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub home_channel_key: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub home_channel: String,
    pub home_channel_configured: bool,
    pub home_channel_supported: bool,
    pub feishu_credentials_configured: bool,
    pub notify_webhook_url: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BridgeConfig {
    pub notify_url: String,
    pub health_url: String,
    pub adapter_host: String,
    pub adapter_port: u32,
    #[serde(skip_serializing)]
    pub notify_secret: String,
    pub notify_secret_source: String,
    pub hermes_home: String,
    pub hermes_config_file: String,
    pub hermes_webhook_url: String,
    pub route_name: String,
    #[serde(skip_serializing)]
    pub route_secret: String,
    pub route_state: RouteState,
    pub adapter_script: String,
    pub python_executable: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct BridgeStatus {
    pub service_name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub service_platform: String,
    pub installed: bool,
    pub running: bool,
    pub bridge_available: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub health_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<BridgeConfig>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

pub fn default_notify_prompt() -> &'static str {
    DEFAULT_NOTIFY_PROMPT.trim()
}

pub fn validate_local_notify_url(raw_url: &str) -> anyhow::Result<LocalNotifyUrl> {
    let value = default_string(raw_url, DEFAULT_NOTIFY_URL);
    let (scheme, rest) = value
        .split_once("://")
        .ok_or_else(|| anyhow::anyhow!("notify URL must use http for local Hermes bridge"))?;
    if !scheme.eq_ignore_ascii_case("http") {
        anyhow::bail!("notify URL must use http for local Hermes bridge");
    }
    let authority = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .trim();
    let (host, port_text) = split_host_port(authority, &value)?;
    let host = if host.trim().is_empty() {
        "127.0.0.1".to_string()
    } else {
        host.trim().to_string()
    };
    match host.to_ascii_lowercase().as_str() {
        "127.0.0.1" | "localhost" | "::1" => {}
        _ => anyhow::bail!(
            "notify URL host {:?} is not local; full Hermes setup only supports a local bridge",
            host
        ),
    }
    let port = match port_text {
        Some(value) => value
            .parse::<u32>()
            .ok()
            .filter(|value| *value > 0)
            .ok_or_else(|| anyhow::anyhow!("notify URL has invalid port"))?,
        None => 80,
    };
    Ok(LocalNotifyUrl {
        url: value,
        host,
        port,
    })
}

pub fn normalize_deliver_target(value: &str) -> String {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        DEFAULT_DELIVER_TARGET.to_string()
    } else {
        normalized
    }
}

pub fn supported_deliver_targets() -> Vec<&'static str> {
    let mut values = SUPPORTED_DELIVER_TARGETS.to_vec();
    values.sort_unstable();
    values
}

pub fn is_supported_deliver_target(value: &str) -> bool {
    let normalized = normalize_deliver_target(value);
    SUPPORTED_DELIVER_TARGETS
        .iter()
        .any(|candidate| *candidate == normalized)
}

pub fn home_channel_env_key(deliver: &str) -> &'static str {
    match normalize_deliver_target(deliver).as_str() {
        "bluebubbles" => "BLUEBUBBLES_HOME_CHANNEL",
        "discord" => "DISCORD_HOME_CHANNEL",
        "email" => "EMAIL_HOME_ADDRESS",
        "feishu" => "FEISHU_HOME_CHANNEL",
        "matrix" => "MATRIX_HOME_ROOM",
        "mattermost" => "MATTERMOST_HOME_CHANNEL",
        "qqbot" => "QQ_HOME_CHANNEL",
        "signal" => "SIGNAL_HOME_CHANNEL",
        "slack" => "SLACK_HOME_CHANNEL",
        "sms" => "SMS_HOME_CHANNEL",
        "telegram" => "TELEGRAM_HOME_CHANNEL",
        "wecom" => "WECOM_HOME_CHANNEL",
        "weixin" => "WEIXIN_HOME_CHANNEL",
        _ => "",
    }
}

pub fn deliver_display_name(deliver: &str) -> String {
    match normalize_deliver_target(deliver).as_str() {
        "bluebubbles" => "BlueBubbles".to_string(),
        "discord" => "Discord".to_string(),
        "email" => "Email".to_string(),
        "feishu" => "Feishu".to_string(),
        "log" => "log".to_string(),
        "matrix" => "Matrix".to_string(),
        "mattermost" => "Mattermost".to_string(),
        "qqbot" => "QQ Bot".to_string(),
        "signal" => "Signal".to_string(),
        "slack" => "Slack".to_string(),
        "sms" => "SMS".to_string(),
        "telegram" => "Telegram".to_string(),
        "wecom" => "WeCom".to_string(),
        "weixin" => "Weixin".to_string(),
        other => capitalize_ascii(other),
    }
}

pub fn read_env_file(path: impl AsRef<Path>) -> std::io::Result<BTreeMap<String, String>> {
    let raw = match fs::read(path.as_ref()) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(err) => return Err(err),
    };
    let raw = String::from_utf8_lossy(&raw);
    let mut values = BTreeMap::new();
    for line in raw.split('\n') {
        let mut trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("export ") {
            trimmed = rest.trim();
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        values.insert(
            key.trim().to_string(),
            value
                .trim()
                .trim_matches(|ch| ch == '"' || ch == '\'')
                .to_string(),
        );
    }
    Ok(values)
}

pub fn resolve_hermes_home() -> anyhow::Result<String> {
    if let Ok(value) = env::var("HERMES_HOME") {
        let value = value.trim();
        if !value.is_empty() {
            return Ok(value.to_string());
        }
    }
    let home = env::var("HOME").context("resolve user home: HOME is not set")?;
    Ok(Path::new(home.trim())
        .join(".hermes")
        .to_string_lossy()
        .into_owned())
}

pub fn inspect_route(home: &str, route_name: &str) -> anyhow::Result<RouteState> {
    let route_name = default_string(route_name, DEFAULT_WEBHOOK_ROUTE_NAME);
    let config_path = Path::new(home).join("config.yaml");
    let env_path = Path::new(home).join(".env");
    let mut state = RouteState {
        hermes_home: home.to_string(),
        config_file: config_path.to_string_lossy().into_owned(),
        env_file: env_path.to_string_lossy().into_owned(),
        config_exists: false,
        webhook_enabled: false,
        webhook_port: DEFAULT_WEBHOOK_PORT,
        route_name: route_name.clone(),
        route_configured: false,
        route_secret: String::new(),
        route_secret_configured: false,
        deliver: String::new(),
        deliver_uses_home_channel: true,
        home_channel_key: String::new(),
        home_channel: String::new(),
        home_channel_configured: false,
        home_channel_supported: false,
        feishu_credentials_configured: false,
        notify_webhook_url: String::new(),
        warnings: Vec::new(),
    };

    let fields = match fs::read_to_string(&config_path) {
        Ok(raw) => {
            state.config_exists = true;
            parse_yaml_scalar_fields(&raw)
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
        Err(err) => {
            anyhow::bail!("read Hermes config.yaml: {err}");
        }
    };

    state.webhook_enabled = bool_field(&fields, &["platforms", "webhook", "enabled"], false);
    state.webhook_port = int_field(
        &fields,
        &["platforms", "webhook", "extra", "port"],
        DEFAULT_WEBHOOK_PORT,
    );
    let route_prefix = vec![
        "platforms".to_string(),
        "webhook".to_string(),
        "extra".to_string(),
        "routes".to_string(),
        route_name.clone(),
    ];
    state.route_configured = fields
        .keys()
        .any(|path| path.len() > route_prefix.len() && path.starts_with(&route_prefix));
    state.route_secret = string_field_with_prefix(&fields, &route_prefix, "secret");
    state.route_secret_configured = !state.route_secret.trim().is_empty();
    state.deliver = string_field_with_prefix(&fields, &route_prefix, "deliver");
    if state.deliver.trim().is_empty() {
        state.deliver = "log".to_string();
    }
    let fixed_chat_id =
        string_field_with_suffix(&fields, &route_prefix, &["deliver_extra", "chat_id"]);
    state.deliver_uses_home_channel = fixed_chat_id.trim().is_empty();
    state.home_channel_key = home_channel_env_key(&state.deliver).to_string();
    state.home_channel_supported = !state.home_channel_key.is_empty();
    if state.home_channel_supported {
        state.home_channel = fields
            .get(&vec![state.home_channel_key.clone()])
            .cloned()
            .unwrap_or_default();
        state.home_channel_configured = !state.home_channel.trim().is_empty();
    }

    match read_env_file_context(&env_path) {
        Ok(env_values) => {
            state.feishu_credentials_configured = env_values
                .get("FEISHU_APP_ID")
                .is_some_and(|value| !value.trim().is_empty())
                && env_values
                    .get("FEISHU_APP_SECRET")
                    .is_some_and(|value| !value.trim().is_empty());
        }
        Err(err) => state
            .warnings
            .push(format!("Failed to read Hermes .env: {err}")),
    }

    state.notify_webhook_url = format!(
        "http://127.0.0.1:{}/webhooks/{}",
        state.webhook_port, state.route_name
    );
    if state.deliver != "log" && !state.deliver_uses_home_channel {
        if !state.home_channel_key.is_empty() {
            state.warnings.push(format!(
                "Hermes notify route still has deliver_extra.chat_id; notifications will not follow {} until that fixed target is removed.",
                state.home_channel_key
            ));
        } else {
            state.warnings.push(
                "Hermes notify route still has deliver_extra.chat_id; notifications will not follow the platform home channel until that fixed target is removed."
                    .to_string(),
            );
        }
    }
    if state.deliver_uses_home_channel && state.deliver != "log" {
        if !state.home_channel_supported {
            state.warnings.push(format!(
                "Hermes notify route deliver target {:?} does not have a known home-channel config key. Use an explicitly supported messaging platform or set deliver_extra.chat_id manually.",
                state.deliver
            ));
        } else if !state.home_channel_configured {
            state.warnings.push(format!(
                "{} is not configured in Hermes yet. Run /sethome from the desired {} chat before expecting auto delivery.",
                state.home_channel_key,
                deliver_display_name(&state.deliver)
            ));
        }
    }
    Ok(state)
}

pub fn status_for(resolved: &Resolved) -> BridgeStatus {
    status_from_parts(
        service_name_for(Some(resolved)),
        resolve_bridge_config(resolved),
        service_status_snapshot_for(resolved),
        bridge_health_available,
    )
}

pub fn resolve_bridge_config(resolved: &Resolved) -> anyhow::Result<BridgeConfig> {
    let notify_url = default_string(
        &super::resolve(resolved)
            .host_notify
            .hermes
            .as_ref()
            .map(|config| config.notify_url.as_str())
            .unwrap_or_default(),
        DEFAULT_NOTIFY_URL,
    );
    let local = validate_local_notify_url(&notify_url)?;
    let (notify_secret, notify_secret_source) =
        hermes_host_notify::resolve_hermes_notify_secret_with_source(Some(resolved), &notify_url);
    if notify_secret.trim().is_empty() {
        anyhow::bail!("Hermes host notify secret is not configured in awiki-cli");
    }
    let hermes_home = resolve_hermes_home()?;
    let route_state = inspect_route(&hermes_home, DEFAULT_WEBHOOK_ROUTE_NAME)?;
    if route_state.route_secret.trim().is_empty() {
        anyhow::bail!("Hermes notify route secret is not configured");
    }
    let python_executable = resolve_python_executable()?;
    let adapter_script = resolve_adapter_script_path()?;
    Ok(BridgeConfig {
        notify_url: notify_url.clone(),
        health_url: health_url_for(&notify_url),
        adapter_host: normalize_adapter_bind_host(&local.host),
        adapter_port: local.port,
        notify_secret,
        notify_secret_source,
        hermes_home,
        hermes_config_file: route_state.config_file.clone(),
        hermes_webhook_url: route_state.notify_webhook_url.clone(),
        route_name: route_state.route_name.clone(),
        route_secret: route_state.route_secret.clone(),
        route_state,
        adapter_script,
        python_executable,
    })
}

pub fn cleanup_deliver_extra(route: &mut Map<String, Value>) {
    let Some(extra) = route
        .get_mut("deliver_extra")
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    extra.remove("chat_id");
    extra.remove("thread_id");
    extra.remove("message_thread_id");
    if extra.is_empty() {
        route.remove("deliver_extra");
        return;
    }
    let mut cleaned = Map::new();
    let mut keys: Vec<String> = extra.keys().cloned().collect();
    keys.sort();
    for key in keys {
        if let Some(value) = extra.remove(&key) {
            cleaned.insert(key, value);
        }
    }
    route.insert("deliver_extra".to_string(), Value::Object(cleaned));
}

pub fn cleanup_legacy_notify_skill(route: &mut Map<String, Value>) {
    let Some(skills) = route.get("skills").and_then(Value::as_array) else {
        return;
    };
    if skills.len() != 1 {
        return;
    }
    if string_value(&skills[0]).trim() == "notify" {
        route.remove("skills");
    }
}

pub fn should_replace_notify_prompt(current: &str) -> bool {
    let normalized = current.trim();
    if normalized.is_empty()
        || normalized == LEGACY_DEFAULT_NOTIFY_PROMPT.trim()
        || normalized == DEFAULT_NOTIFY_PROMPT_V1.trim()
    {
        return true;
    }
    if is_legacy_im_only_notify_prompt(normalized) {
        return true;
    }
    if normalized.contains("你是 awiki 外部消息通知整理助手。")
        && normalized.contains("{notify_payload}")
        && normalized.contains("收到外部邮件通知")
        && (normalized.contains("如果 topic 是 mail.message.received")
            || normalized.contains("topic=mail.message.received 时")
            || normalized.contains("不强依赖 topic 名称")
            || normalized.contains("优先按邮件通知处理"))
    {
        return true;
    }
    if normalized.contains("你是 awiki 外部消息通知整理助手。")
        && normalized.contains("{notify_payload}")
        && normalized.contains("收到外部邮件通知")
        && normalized.contains("必须按邮件通知处理")
        && (!normalized.contains("不要使用“收到外部IM消息通知”作为标题")
            || !normalized.contains("发件邮箱：<from_addr，如存在且与发件人不同>")
            || !normalized.contains("去掉重复署名和邮箱签名"))
    {
        return true;
    }
    false
}

fn split_host_port(authority: &str, full_url: &str) -> anyhow::Result<(String, Option<String>)> {
    let authority = authority
        .rsplit_once('@')
        .map(|(_, host)| host)
        .unwrap_or(authority);
    if let Some(rest) = authority.strip_prefix('[') {
        let Some((host, after)) = rest.split_once(']') else {
            anyhow::bail!("parse notify URL: missing closing bracket in host");
        };
        if after.is_empty() {
            return Ok((host.to_string(), None));
        }
        let Some(port) = after.strip_prefix(':') else {
            anyhow::bail!("parse notify URL: invalid host");
        };
        return parse_optional_port(host, port, full_url);
    }
    let colon_count = authority
        .as_bytes()
        .iter()
        .filter(|byte| **byte == b':')
        .count();
    if colon_count == 0 {
        return Ok((authority.to_string(), None));
    }
    if colon_count == 1 {
        let (host, port) = authority.rsplit_once(':').unwrap_or((authority, ""));
        return parse_optional_port(host, port, full_url);
    }
    Ok((authority.to_string(), None))
}

fn parse_optional_port(
    host: &str,
    port: &str,
    full_url: &str,
) -> anyhow::Result<(String, Option<String>)> {
    if port.is_empty() {
        return Ok((host.to_string(), None));
    }
    if !port.as_bytes().iter().all(u8::is_ascii_digit) {
        anyhow::bail!(
            "parse notify URL: parse {:?}: invalid port {:?} after host",
            full_url,
            format!(":{port}")
        );
    }
    Ok((host.to_string(), Some(port.to_string())))
}

fn is_legacy_im_only_notify_prompt(normalized: &str) -> bool {
    !normalized.is_empty()
        && normalized.contains("你是 awiki 外部 IM 消息通知整理助手。")
        && normalized.contains("收到外部IM消息通知")
        && normalized.contains("消息内容摘要：")
        && normalized.contains("{notify_payload}")
        && !normalized.contains("收到外部邮件通知")
        && !normalized.contains("source_kind=mail")
}

fn default_string(value: &str, default: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        default.to_string()
    } else {
        trimmed.to_string()
    }
}

fn capitalize_ascii(value: &str) -> String {
    if value.is_empty() {
        return "Unknown".to_string();
    }
    let first_len = value.chars().next().map(char::len_utf8).unwrap_or_default();
    let (first, rest) = value.split_at(first_len);
    format!("{}{}", first.to_ascii_uppercase(), rest)
}

fn string_value(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(value) => value.clone(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::Array(_) | Value::Object(_) => value.to_string(),
    }
}

pub fn read_env_file_context(path: impl AsRef<Path>) -> anyhow::Result<BTreeMap<String, String>> {
    read_env_file(path.as_ref())
        .with_context(|| format!("read Hermes .env: {}", path.as_ref().to_string_lossy()))
}

fn parse_yaml_scalar_fields(raw: &str) -> BTreeMap<Vec<String>, String> {
    let mut fields = BTreeMap::new();
    let mut stack: Vec<(usize, String)> = Vec::new();
    for line in raw.lines() {
        let without_comment = line.split('#').next().unwrap_or("").trim_end();
        if without_comment.trim().is_empty() {
            continue;
        }
        let indent = without_comment.chars().take_while(|ch| *ch == ' ').count();
        while stack.last().is_some_and(|(level, _)| *level >= indent) {
            stack.pop();
        }
        let trimmed = without_comment.trim_start();
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        let key = key.trim().to_string();
        let value = strip_yaml_scalar(value.trim());
        if value.is_empty() {
            stack.push((indent, key));
            continue;
        }
        let mut path: Vec<String> = stack.iter().map(|(_, key)| key.clone()).collect();
        path.push(key);
        fields.insert(path, value);
    }
    fields
}

fn strip_yaml_scalar(value: &str) -> String {
    value
        .trim()
        .trim_matches(|ch| ch == '"' || ch == '\'')
        .to_string()
}

fn bool_field(fields: &BTreeMap<Vec<String>, String>, path: &[&str], default: bool) -> bool {
    fields
        .get(&path.iter().map(|part| part.to_string()).collect::<Vec<_>>())
        .and_then(|value| match value.trim().to_ascii_lowercase().as_str() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        })
        .unwrap_or(default)
}

fn int_field(fields: &BTreeMap<Vec<String>, String>, path: &[&str], default: u32) -> u32 {
    fields
        .get(&path.iter().map(|part| part.to_string()).collect::<Vec<_>>())
        .and_then(|value| value.trim().parse::<u32>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

fn string_field_with_prefix(
    fields: &BTreeMap<Vec<String>, String>,
    prefix: &[String],
    leaf: &str,
) -> String {
    let mut path = prefix.to_vec();
    path.push(leaf.to_string());
    fields.get(&path).cloned().unwrap_or_default()
}

fn string_field_with_suffix(
    fields: &BTreeMap<Vec<String>, String>,
    prefix: &[String],
    suffix: &[&str],
) -> String {
    let mut path = prefix.to_vec();
    path.extend(suffix.iter().map(|part| part.to_string()));
    fields.get(&path).cloned().unwrap_or_default()
}

fn resolve_python_executable() -> anyhow::Result<String> {
    let Some(paths) = env::var_os("PATH") else {
        anyhow::bail!("python3 or python was not found in PATH")
    };
    let search_paths = env::split_paths(&paths).collect::<Vec<_>>();
    if let Some(path) = resolve_python_executable_from_paths(&search_paths) {
        return Ok(path.to_string_lossy().into_owned());
    }
    anyhow::bail!("python3 or python was not found in PATH")
}

fn resolve_python_executable_from_paths(paths: &[PathBuf]) -> Option<PathBuf> {
    for candidate in ["python3", "python"] {
        if let Some(path) = command_on_path(candidate, paths) {
            return Some(path);
        }
    }
    None
}

fn command_on_path(candidate: &str, paths: &[PathBuf]) -> Option<PathBuf> {
    paths
        .iter()
        .flat_map(|dir| executable_candidate_paths(dir, candidate))
        .find(|path| executable_candidate_exists(path))
}

fn executable_candidate_paths(dir: &Path, candidate: &str) -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        let mut paths = vec![dir.join(candidate)];
        let has_extension = Path::new(candidate).extension().is_some();
        if !has_extension {
            paths.push(dir.join(format!("{candidate}.exe")));
        }
        paths
    }
    #[cfg(not(windows))]
    {
        vec![dir.join(candidate)]
    }
}

fn executable_candidate_exists(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        return path
            .metadata()
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false);
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn resolve_adapter_script_path() -> anyhow::Result<String> {
    let exe_path = env::current_exe().context("resolve awiki-cli executable path")?;
    let exe_dir = exe_path.parent().unwrap_or_else(|| Path::new(""));
    if let Some(path) = resolve_adapter_script_path_from_exe_dir(exe_dir) {
        return Ok(path.to_string_lossy().into_owned());
    }
    anyhow::bail!(
        "could not locate scripts/hermes_notify_adapter.py next to the awiki-cli installation"
    )
}

fn resolve_adapter_script_path_from_exe_dir(exe_dir: &Path) -> Option<PathBuf> {
    adapter_script_candidates(exe_dir)
        .into_iter()
        .find_map(|candidate| {
            if !candidate.is_file() {
                return None;
            }
            Some(candidate.canonicalize().unwrap_or(candidate))
        })
}

fn adapter_script_candidates(exe_dir: &Path) -> Vec<PathBuf> {
    vec![
        exe_dir
            .join("..")
            .join("scripts")
            .join("hermes_notify_adapter.py"),
        exe_dir.join("scripts").join("hermes_notify_adapter.py"),
        exe_dir
            .join("..")
            .join("..")
            .join("scripts")
            .join("hermes_notify_adapter.py"),
    ]
}

fn normalize_adapter_bind_host(host: &str) -> String {
    match host.trim().to_ascii_lowercase().as_str() {
        "" | "localhost" => "127.0.0.1".to_string(),
        _ => host.to_string(),
    }
}

fn health_url_for(notify_url: &str) -> String {
    let Some((scheme, rest)) = notify_url.split_once("://") else {
        return String::new();
    };
    let authority = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .trim();
    if authority.is_empty() {
        return String::new();
    }
    format!("{scheme}://{authority}/healthz")
}

#[cfg(test)]
mod tests {
    use super::{
        adapter_script_candidates, resolve_adapter_script_path_from_exe_dir,
        resolve_python_executable_from_paths,
    };
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn resolve_python_executable_from_paths_prefers_python3_like_go() {
        let temp = TempDir::new("python-path-priority").expect("temp dir");
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        fs::create_dir_all(&first).expect("first dir");
        fs::create_dir_all(&second).expect("second dir");
        make_executable(&first.join("python")).expect("python executable");
        make_executable(&second.join("python3")).expect("python3 executable");

        let resolved = resolve_python_executable_from_paths(&[first.clone(), second.clone()])
            .expect("resolved python");

        assert_eq!(resolved, second.join("python3"));
    }

    #[cfg(unix)]
    #[test]
    fn resolve_python_executable_from_paths_ignores_non_executable_files_like_go() {
        let temp = TempDir::new("python-path-permissions").expect("temp dir");
        let bin = temp.path().join("bin");
        fs::create_dir_all(&bin).expect("bin dir");
        fs::write(bin.join("python3"), b"not executable").expect("python3 file");
        make_executable(&bin.join("python")).expect("python executable");

        let resolved =
            resolve_python_executable_from_paths(std::slice::from_ref(&bin)).expect("python");

        assert_eq!(resolved, bin.join("python"));
    }

    #[test]
    fn resolve_adapter_script_path_from_exe_dir_matches_go_candidate_order() {
        let temp = TempDir::new("adapter-script-order").expect("temp dir");
        let exe_dir = temp.path().join("bin");
        let first = temp.path().join("scripts");
        let second = exe_dir.join("scripts");
        fs::create_dir_all(&exe_dir).expect("exe dir");
        fs::create_dir_all(&first).expect("first scripts dir");
        fs::create_dir_all(&second).expect("second scripts dir");
        let first_script = first.join("hermes_notify_adapter.py");
        let second_script = second.join("hermes_notify_adapter.py");
        fs::write(&first_script, b"first").expect("first script");
        fs::write(&second_script, b"second").expect("second script");

        let resolved = resolve_adapter_script_path_from_exe_dir(&exe_dir).expect("adapter script");

        assert_eq!(resolved, first_script.canonicalize().unwrap());
    }

    #[test]
    fn adapter_script_candidates_match_go_layout_order() {
        let exe_dir = PathBuf::from("/opt/awiki/bin");
        let candidates = adapter_script_candidates(&exe_dir);

        assert_eq!(
            candidates,
            vec![
                PathBuf::from("/opt/awiki/bin/../scripts/hermes_notify_adapter.py"),
                PathBuf::from("/opt/awiki/bin/scripts/hermes_notify_adapter.py"),
                PathBuf::from("/opt/awiki/bin/../../scripts/hermes_notify_adapter.py"),
            ]
        );
    }

    fn make_executable(path: &Path) -> std::io::Result<()> {
        fs::write(path, b"#!/bin/sh\n")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
        }
        Ok(())
    }

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(name: &str) -> std::io::Result<Self> {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "awiki-cli-rs2-hermes-bridge-{name}-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir_all(&path)?;
            Ok(Self { path })
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
