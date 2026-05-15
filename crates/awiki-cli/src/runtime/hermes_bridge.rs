use anyhow::Context;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

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
