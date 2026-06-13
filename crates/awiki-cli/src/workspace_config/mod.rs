use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

mod write;
pub(crate) use write::write_file_config_raw;
pub use write::{
    clear_hermes_secret, clear_openclaw_token, configure_hermes_host_notify,
    ensure_config_schema_version, read_openclaw_token, set_hermes_secret, set_openclaw_token,
    update_active_identity, update_did_domain, update_hermes_settings, update_host_notify_enabled,
    update_host_notify_sink, update_openclaw_settings, update_runtime_listener_settings,
    update_runtime_settings, write_file_config,
};

const APP_NAME: &str = "awiki-cli";
const CONFIG_FILE_NAME: &str = "config.yaml";
const DEFAULT_SERVICE_BASE_URL: &str = "https://awiki.ai";
const DEFAULT_DID_DOMAIN: &str = "awiki.ai";
const DEFAULT_ANP_PATH: &str = "/anp-im/rpc";
const DEFAULT_RUNTIME_MODE: &str = "websocket";
const DEFAULT_OUTPUT_FORMAT: &str = "json";
const DEFAULT_LISTENER_ENABLED: bool = true;
const DEFAULT_LISTENER_AUTO_INSTALL: bool = true;
const DEFAULT_LISTENER_AUTO_START: bool = true;
const DEFAULT_HOST_NOTIFY_ENABLED: bool = true;
const DEFAULT_HOST_NOTIFY_SINK: &str = "log";
const DEFAULT_HOST_NOTIFY_FILE: &str = "host-notify.events.jsonl";
const DEFAULT_OPENCLAW_HOOK_URL: &str = "http://127.0.0.1:18789/hooks/agent";
const DEFAULT_OPENCLAW_AGENT_ID: &str = "main";
const DEFAULT_OPENCLAW_HOOK_NAME: &str = "AWiki";
const DEFAULT_HERMES_NOTIFY_URL: &str = "http://127.0.0.1:8765/notify/host-event";
const DEFAULT_HERMES_DELIVER_TARGET: &str = "feishu";
pub const CONFIG_SCHEMA_VERSION: i64 = 1;

#[derive(Debug, Clone, Default)]
pub struct Overrides {
    pub identity: String,
    pub identity_changed: bool,
    pub format: String,
    pub format_changed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Paths {
    pub workspace_home_dir: String,
    pub root_dir: String,
    pub config_dir: String,
    pub data_dir: String,
    pub state_dir: String,
    pub cache_dir: String,
    pub logs_dir: String,
    pub config_file: String,
    pub identity_dir: String,
    pub database_file: String,
    pub legacy_credentials_dir: String,
    pub legacy_data_dir: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct EnvHit {
    pub key: String,
    pub value: String,
    pub tier: String,
    pub target: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValueSource {
    pub source: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub key: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub value: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Resolved {
    pub paths: Paths,
    pub config_schema_version: i64,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub active_identity: String,
    pub runtime_mode: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub runtime_socket_path: String,
    pub runtime_listener_enabled: bool,
    pub runtime_listener_auto_install: bool,
    pub runtime_listener_auto_start: bool,
    pub host_notify_enabled: bool,
    pub host_notify_sink: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub host_notify_file_path: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub host_notify_openclaw_hook_url: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub host_notify_openclaw_agent_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub host_notify_openclaw_hook_name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub host_notify_hermes_notify_url: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub host_notify_hermes_deliver: String,
    pub output_format: String,
    pub no_color: bool,
    pub service_base_url: String,
    pub user_service_endpoint: String,
    pub message_service_endpoint: String,
    pub did_domain: String,
    pub anp_service_endpoint: String,
    pub anp_service_did: String,
    pub mail_service_url: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub ca_bundle: String,
    pub update_disable_strict_version: bool,
    pub update_metadata_cache_ttl_seconds: i64,
    pub config_exists: bool,
    pub config_error: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub env_hits: Vec<EnvHit>,
    pub sources: BTreeMap<String, ValueSource>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct FileConfig {
    #[serde(default)]
    pub schema_version: i64,
    #[serde(default)]
    pub identity: IdentityConfig,
    #[serde(default)]
    pub runtime: RuntimeConfig,
    #[serde(default)]
    pub output: OutputConfig,
    #[serde(default)]
    pub services: ServicesConfig,
    #[serde(default)]
    pub update: UpdateConfig,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct IdentityConfig {
    #[serde(default)]
    pub active: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct RuntimeConfig {
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub socket_path: String,
    #[serde(default)]
    pub listener: ListenerConfig,
    #[serde(default)]
    pub host_notify: HostNotifyConfig,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ListenerConfig {
    pub enabled: Option<bool>,
    pub auto_install: Option<bool>,
    pub auto_start: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct HostNotifyConfig {
    pub enabled: Option<bool>,
    #[serde(default)]
    pub sink: String,
    #[serde(default)]
    pub file_path: String,
    #[serde(default)]
    pub openclaw: OpenClawConfig,
    #[serde(default)]
    pub hermes: HermesConfig,
    #[serde(default)]
    pub webhook: LegacyWebhookConfig,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct OpenClawConfig {
    #[serde(default)]
    pub hook_url: String,
    #[serde(default)]
    pub agent_id: String,
    #[serde(default)]
    pub hook_name: String,
    #[serde(default)]
    pub token: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct HermesConfig {
    #[serde(default)]
    pub notify_url: String,
    #[serde(default)]
    pub deliver: String,
    #[serde(default)]
    pub secret: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct LegacyWebhookConfig {
    #[serde(default)]
    pub notify_url: String,
    #[serde(default)]
    pub secret: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct OutputConfig {
    #[serde(default)]
    pub format: String,
    pub no_color: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ServicesConfig {
    #[serde(default)]
    pub service_base_url: String,
    #[serde(default)]
    pub user_service_endpoint: String,
    #[serde(default)]
    pub message_service_endpoint: String,
    #[serde(default)]
    pub did_domain: String,
    #[serde(default)]
    pub anp_service_endpoint: String,
    #[serde(default)]
    pub anp_service_did: String,
    #[serde(default)]
    pub ca_bundle: String,
    #[serde(default)]
    pub mail_service_url: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct UpdateConfig {
    #[serde(default)]
    pub disable_strict_version: bool,
    #[serde(default)]
    pub metadata_cache_ttl_seconds: i64,
}

pub fn resolve(overrides: Overrides) -> anyhow::Result<Resolved> {
    let home = home_dir()?;
    let (workspace_home_dir, workspace_source) = resolve_workspace_home(&home);
    let paths = build_paths(&home, &workspace_home_dir);
    validate_deprecated_config_fields(&paths.config_file)?;
    let (file_config, config_exists, config_error) = read_file_config(&paths.config_file);
    let mut sources = BTreeMap::new();
    sources.insert("workspace_home_dir".to_string(), workspace_source.clone());
    sources.insert("root_dir".to_string(), workspace_source);
    for (key, value) in [
        ("config_dir", &paths.config_dir),
        ("data_dir", &paths.data_dir),
        ("state_dir", &paths.state_dir),
        ("cache_dir", &paths.cache_dir),
        ("logs_dir", &paths.logs_dir),
    ] {
        sources.insert(
            key.to_string(),
            ValueSource {
                source: "derived".to_string(),
                key: "workspace_home_dir".to_string(),
                value: value.clone(),
            },
        );
    }

    let (active_identity, active_source) = choose_value(
        &overrides.identity,
        overrides.identity_changed,
        &file_config.identity.active,
        "",
    );
    sources.insert("active_identity".to_string(), active_source);
    let (runtime_mode, runtime_source) =
        choose_value("", false, &file_config.runtime.mode, DEFAULT_RUNTIME_MODE);
    sources.insert("runtime_mode".to_string(), runtime_source);
    let default_socket = default_runtime_bridge_path(&paths);
    let (runtime_socket_path, socket_source) =
        choose_value("", false, &file_config.runtime.socket_path, &default_socket);
    sources.insert("runtime_socket_path".to_string(), socket_source);
    let (output_format, output_source) = choose_value(
        &overrides.format,
        overrides.format_changed,
        &file_config.output.format,
        DEFAULT_OUTPUT_FORMAT,
    );
    sources.insert("output_format".to_string(), output_source);
    let (service_base_url, service_source) = choose_value(
        "",
        false,
        &file_config.services.service_base_url,
        DEFAULT_SERVICE_BASE_URL,
    );
    let service_base_url = normalize_base_url(&service_base_url);
    sources.insert(
        "service_base_url".to_string(),
        ValueSource {
            value: service_base_url.clone(),
            ..service_source
        },
    );
    let user_service_endpoint = resolve_service_endpoint_field(
        "user_service_endpoint",
        &file_config.services.user_service_endpoint,
        &service_base_url,
        &mut sources,
    );
    let message_service_endpoint = resolve_service_endpoint_field(
        "message_service_endpoint",
        &file_config.services.message_service_endpoint,
        &service_base_url,
        &mut sources,
    );
    let (did_domain, did_source) = choose_value(
        "",
        false,
        &file_config.services.did_domain,
        DEFAULT_DID_DOMAIN,
    );
    sources.insert("did_domain".to_string(), did_source);
    let (mut anp_endpoint, anp_endpoint_source) =
        choose_value("", false, &file_config.services.anp_service_endpoint, "");
    if anp_endpoint.trim().is_empty() {
        anp_endpoint = derive_anp_service_endpoint(&service_base_url);
        sources.insert(
            "anp_service_endpoint".to_string(),
            ValueSource {
                source: "derived_default".to_string(),
                key: "service_base_url".to_string(),
                value: anp_endpoint.clone(),
            },
        );
    } else {
        sources.insert("anp_service_endpoint".to_string(), anp_endpoint_source);
    }
    let (mut anp_did, anp_did_source) =
        choose_value("", false, &file_config.services.anp_service_did, "");
    if anp_did.trim().is_empty() {
        anp_did = derive_anp_service_did(&service_base_url);
        sources.insert(
            "anp_service_did".to_string(),
            ValueSource {
                source: "derived_default".to_string(),
                key: "service_base_url".to_string(),
                value: anp_did.clone(),
            },
        );
    } else {
        sources.insert("anp_service_did".to_string(), anp_did_source);
    }
    let (mail_service_url, mail_source) = if file_config.services.mail_service_url.trim().is_empty()
    {
        (
            service_base_url.clone(),
            ValueSource {
                source: "derived_default".to_string(),
                key: "service_base_url".to_string(),
                value: service_base_url.clone(),
            },
        )
    } else {
        let value = file_config.services.mail_service_url.trim().to_string();
        (
            value.clone(),
            ValueSource {
                source: "config_file".to_string(),
                key: String::new(),
                value,
            },
        )
    };
    sources.insert("mail_service_url".to_string(), mail_source);

    let host_notify_sink = normalize_host_notify_sink(&file_config.runtime.host_notify.sink);
    validate_host_notify_sink(&host_notify_sink)?;
    let host_notify_file_path =
        host_notify_file_path(&paths, &file_config.runtime.host_notify, &host_notify_sink);
    let (openclaw_hook_url, openclaw_hook_source, openclaw_agent_id, openclaw_hook_name) =
        resolve_openclaw_fields(&file_config.runtime.host_notify, &host_notify_sink);
    sources.insert(
        "host_notify_openclaw_hook_url".to_string(),
        openclaw_hook_source,
    );
    let (hermes_notify_url, hermes_deliver) =
        resolve_hermes_fields(&file_config.runtime.host_notify, &host_notify_sink);

    Ok(Resolved {
        paths,
        config_schema_version: normalized_config_schema_version(file_config.schema_version),
        active_identity,
        runtime_mode,
        runtime_socket_path,
        runtime_listener_enabled: file_config
            .runtime
            .listener
            .enabled
            .unwrap_or(DEFAULT_LISTENER_ENABLED),
        runtime_listener_auto_install: file_config
            .runtime
            .listener
            .auto_install
            .unwrap_or(DEFAULT_LISTENER_AUTO_INSTALL),
        runtime_listener_auto_start: file_config
            .runtime
            .listener
            .auto_start
            .unwrap_or(DEFAULT_LISTENER_AUTO_START),
        host_notify_enabled: file_config
            .runtime
            .host_notify
            .enabled
            .unwrap_or(DEFAULT_HOST_NOTIFY_ENABLED),
        host_notify_sink,
        host_notify_file_path,
        host_notify_openclaw_hook_url: openclaw_hook_url,
        host_notify_openclaw_agent_id: openclaw_agent_id,
        host_notify_openclaw_hook_name: openclaw_hook_name,
        host_notify_hermes_notify_url: hermes_notify_url,
        host_notify_hermes_deliver: hermes_deliver,
        output_format,
        no_color: file_config.output.no_color.unwrap_or(false),
        service_base_url,
        user_service_endpoint,
        message_service_endpoint,
        did_domain,
        anp_service_endpoint: anp_endpoint,
        anp_service_did: anp_did,
        mail_service_url: normalize_base_url(&mail_service_url),
        ca_bundle: file_config.services.ca_bundle,
        update_disable_strict_version: file_config.update.disable_strict_version,
        update_metadata_cache_ttl_seconds: file_config.update.metadata_cache_ttl_seconds,
        config_exists,
        config_error,
        env_hits: collect_env_hits(),
        sources,
    })
}

pub fn snapshot(resolved: &Resolved) -> Value {
    serde_json::to_value(resolved).unwrap_or_else(|_| json!({}))
}

pub(crate) fn read_file_config(path: &str) -> (FileConfig, bool, String) {
    match fs::read_to_string(path) {
        Ok(raw) => match parse_file_config(&raw) {
            Ok(config) => (config, true, String::new()),
            Err(err) => (FileConfig::default(), true, err),
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            (FileConfig::default(), false, String::new())
        }
        Err(err) => (FileConfig::default(), false, err.to_string()),
    }
}

fn parse_file_config(raw: &str) -> Result<FileConfig, String> {
    let trimmed = raw.trim_start();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return serde_json::from_str::<FileConfig>(raw).map_err(|err| err.to_string());
    }
    let mut config = FileConfig::default();
    let mut stack: Vec<(usize, String)> = Vec::new();
    for line in raw.lines() {
        let without_comment = strip_yaml_inline_comment(line).trim_end();
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
        let key = key.trim();
        if key.is_empty() {
            return Err(format!("yaml: missing mapping key in line {line:?}"));
        }
        let raw_value = value.trim();
        if raw_value.is_empty() {
            stack.push((indent, key.to_string()));
            continue;
        }
        let value = strip_yaml_scalar(raw_value)?;
        let mut path: Vec<String> = stack.iter().map(|(_, key)| key.clone()).collect();
        path.push(key.to_string());
        set_config_value(&mut config, &path, &value);
    }
    Ok(config)
}

fn validate_deprecated_config_fields(path: &str) -> anyhow::Result<()> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => anyhow::bail!("read config yaml for policy validation: {err}"),
    };
    let deprecated_fields = collect_deprecated_config_fields(&raw);
    if deprecated_fields.is_empty() {
        return Ok(());
    }
    anyhow::bail!(
        "deprecated config.yaml fields are no longer supported: {}",
        deprecated_fields.join(", ")
    )
}

fn collect_deprecated_config_fields(raw: &str) -> Vec<String> {
    let trimmed = raw.trim_start();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return collect_deprecated_config_fields_json(raw);
    }
    collect_deprecated_config_fields_yaml(raw)
}

fn collect_deprecated_config_fields_json(raw: &str) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<Value>(raw) else {
        return Vec::new();
    };
    let Some(services) = value.get("services").and_then(Value::as_object) else {
        return Vec::new();
    };
    deprecated_service_keys()
        .iter()
        .filter(|key| services.contains_key(**key))
        .map(|key| format!("services.{key}"))
        .collect()
}

fn collect_deprecated_config_fields_yaml(raw: &str) -> Vec<String> {
    let mut stack: Vec<(usize, String)> = Vec::new();
    let mut deprecated = Vec::new();
    for line in raw.lines() {
        let without_comment = strip_yaml_inline_comment(line).trim_end();
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
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        if stack.len() == 1 && stack[0].1 == "services" && deprecated_service_keys().contains(&key)
        {
            deprecated.push(format!("services.{key}"));
        }
        if value.trim().is_empty() {
            stack.push((indent, key.to_string()));
            continue;
        }
    }
    deprecated
}

fn strip_yaml_inline_comment(line: &str) -> &str {
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;
    for (idx, ch) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if in_double && ch == '\\' {
            escaped = true;
            continue;
        }
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '#' if !in_single && !in_double => return &line[..idx],
            _ => {}
        }
    }
    line
}

fn deprecated_service_keys() -> [&'static str; 3] {
    [
        "user_service_url",
        "message_service_url",
        "message_service_ws_url",
    ]
}

fn strip_yaml_scalar(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() || value == "~" || value.eq_ignore_ascii_case("null") {
        return Ok(String::new());
    }
    if let Some(inner) = value.strip_prefix('"') {
        let Some(inner) = inner.strip_suffix('"') else {
            return Err(
                "yaml: found unexpected end of stream while scanning a quoted scalar".into(),
            );
        };
        return decode_yaml_double_quoted(inner);
    }
    if let Some(inner) = value.strip_prefix('\'') {
        let Some(inner) = inner.strip_suffix('\'') else {
            return Err(
                "yaml: found unexpected end of stream while scanning a quoted scalar".into(),
            );
        };
        return Ok(inner.replace("''", "'"));
    }
    Ok(value.to_string())
}

fn decode_yaml_double_quoted(value: &str) -> Result<String, String> {
    let mut output = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            output.push(ch);
            continue;
        }
        let Some(escaped) = chars.next() else {
            return Err("yaml: found unknown escape character".into());
        };
        match escaped {
            '0' => output.push('\0'),
            'a' => output.push('\u{0007}'),
            'b' => output.push('\u{0008}'),
            't' | '\t' => output.push('\t'),
            'n' => output.push('\n'),
            'v' => output.push('\u{000b}'),
            'f' => output.push('\u{000c}'),
            'r' => output.push('\r'),
            'e' => output.push('\u{001b}'),
            '"' => output.push('"'),
            '/' => output.push('/'),
            '\\' => output.push('\\'),
            'N' => output.push('\u{0085}'),
            '_' => output.push('\u{00a0}'),
            'L' => output.push('\u{2028}'),
            'P' => output.push('\u{2029}'),
            _ => return Err(format!("yaml: found unknown escape character {escaped:?}")),
        }
    }
    Ok(output)
}

fn set_config_value(config: &mut FileConfig, path: &[String], value: &str) {
    let path = path.iter().map(String::as_str).collect::<Vec<_>>();
    match path.as_slice() {
        ["schema_version"] => config.schema_version = value.parse().unwrap_or_default(),
        ["identity", "active"] => config.identity.active = value.to_string(),
        ["runtime", "mode"] => config.runtime.mode = value.to_string(),
        ["runtime", "socket_path"] => config.runtime.socket_path = value.to_string(),
        ["runtime", "listener", "enabled"] => config.runtime.listener.enabled = parse_bool(value),
        ["runtime", "listener", "auto_install"] => {
            config.runtime.listener.auto_install = parse_bool(value)
        }
        ["runtime", "listener", "auto_start"] => {
            config.runtime.listener.auto_start = parse_bool(value)
        }
        ["runtime", "host_notify", "enabled"] => {
            config.runtime.host_notify.enabled = parse_bool(value)
        }
        ["runtime", "host_notify", "sink"] => config.runtime.host_notify.sink = value.to_string(),
        ["runtime", "host_notify", "file_path"] => {
            config.runtime.host_notify.file_path = value.to_string()
        }
        ["runtime", "host_notify", "openclaw", "hook_url"] => {
            config.runtime.host_notify.openclaw.hook_url = value.to_string()
        }
        ["runtime", "host_notify", "openclaw", "agent_id"] => {
            config.runtime.host_notify.openclaw.agent_id = value.to_string()
        }
        ["runtime", "host_notify", "openclaw", "hook_name"] => {
            config.runtime.host_notify.openclaw.hook_name = value.to_string()
        }
        ["runtime", "host_notify", "openclaw", "token"] => {
            config.runtime.host_notify.openclaw.token = value.to_string()
        }
        ["runtime", "host_notify", "hermes", "notify_url"] => {
            config.runtime.host_notify.hermes.notify_url = value.to_string()
        }
        ["runtime", "host_notify", "hermes", "deliver"] => {
            config.runtime.host_notify.hermes.deliver = value.to_string()
        }
        ["runtime", "host_notify", "hermes", "secret"] => {
            config.runtime.host_notify.hermes.secret = value.to_string()
        }
        ["runtime", "host_notify", "webhook", "notify_url"] => {
            config.runtime.host_notify.webhook.notify_url = value.to_string()
        }
        ["runtime", "host_notify", "webhook", "secret"] => {
            config.runtime.host_notify.webhook.secret = value.to_string()
        }
        ["output", "format"] => config.output.format = value.to_string(),
        ["output", "no_color"] => config.output.no_color = parse_bool(value),
        ["services", "service_base_url"] => config.services.service_base_url = value.to_string(),
        ["services", "user_service_endpoint"] => {
            config.services.user_service_endpoint = value.to_string()
        }
        ["services", "message_service_endpoint"] => {
            config.services.message_service_endpoint = value.to_string()
        }
        ["services", "did_domain"] => config.services.did_domain = value.to_string(),
        ["services", "anp_service_endpoint"] => {
            config.services.anp_service_endpoint = value.to_string()
        }
        ["services", "anp_service_did"] => config.services.anp_service_did = value.to_string(),
        ["services", "ca_bundle"] => config.services.ca_bundle = value.to_string(),
        ["services", "mail_service_url"] => config.services.mail_service_url = value.to_string(),
        ["update", "disable_strict_version"] => {
            config.update.disable_strict_version = parse_bool(value).unwrap_or(false)
        }
        ["update", "metadata_cache_ttl_seconds"] => {
            config.update.metadata_cache_ttl_seconds = value.parse().unwrap_or_default()
        }
        _ => {}
    }
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn home_dir() -> anyhow::Result<PathBuf> {
    if let Some(home) = env::var_os("HOME") {
        return Ok(PathBuf::from(home));
    }
    anyhow::bail!("resolve user home: HOME is not set")
}

fn resolve_workspace_home(home: &Path) -> (PathBuf, ValueSource) {
    if let Ok(value) = env::var("AWIKI_CLI_WORKSPACE_HOME_DIR") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            let expanded = expand_home(home, trimmed);
            return (
                expanded.clone(),
                ValueSource {
                    source: "canonical_env".to_string(),
                    key: "AWIKI_CLI_WORKSPACE_HOME_DIR".to_string(),
                    value: path_string(&expanded),
                },
            );
        }
    }
    let default = home.join(format!(".{APP_NAME}"));
    (
        default.clone(),
        ValueSource {
            source: "default".to_string(),
            key: String::new(),
            value: path_string(&default),
        },
    )
}

fn build_paths(home: &Path, workspace_home_dir: &Path) -> Paths {
    let data_dir = workspace_home_dir.join("data");
    let state_dir = workspace_home_dir.join("runtime");
    let cache_dir = workspace_home_dir.join("cache");
    let logs_dir = workspace_home_dir.join("logs");
    Paths {
        workspace_home_dir: path_string(workspace_home_dir),
        root_dir: path_string(workspace_home_dir),
        config_dir: path_string(workspace_home_dir),
        data_dir: path_string(&data_dir),
        state_dir: path_string(&state_dir),
        cache_dir: path_string(&cache_dir),
        logs_dir: path_string(&logs_dir),
        config_file: path_string(&workspace_home_dir.join(CONFIG_FILE_NAME)),
        identity_dir: path_string(&workspace_home_dir.join("identities")),
        database_file: path_string(&data_dir.join(format!("{APP_NAME}.db"))),
        legacy_credentials_dir: path_string(
            &home
                .join(".openclaw")
                .join("credentials")
                .join("awiki-agent-id-message"),
        ),
        legacy_data_dir: path_string(
            &home
                .join(".openclaw")
                .join("workspace")
                .join("data")
                .join("awiki-agent-id-message"),
        ),
    }
}

fn choose_value(
    flag_value: &str,
    flag_changed: bool,
    file_value: &str,
    default_value: &str,
) -> (String, ValueSource) {
    if flag_changed && !flag_value.trim().is_empty() {
        let value = flag_value.trim().to_string();
        return (
            value.clone(),
            ValueSource {
                source: "flag".to_string(),
                key: String::new(),
                value,
            },
        );
    }
    if !file_value.trim().is_empty() {
        let value = file_value.trim().to_string();
        return (
            value.clone(),
            ValueSource {
                source: "config_file".to_string(),
                key: String::new(),
                value,
            },
        );
    }
    (
        default_value.to_string(),
        ValueSource {
            source: "default".to_string(),
            key: String::new(),
            value: default_value.to_string(),
        },
    )
}

fn resolve_service_endpoint_field(
    key: &'static str,
    file_value: &str,
    service_base_url: &str,
    sources: &mut BTreeMap<String, ValueSource>,
) -> String {
    if file_value.trim().is_empty() {
        let value = service_base_url.to_string();
        sources.insert(
            key.to_string(),
            ValueSource {
                source: "derived_default".to_string(),
                key: "service_base_url".to_string(),
                value: value.clone(),
            },
        );
        return value;
    }
    let value = normalize_base_url(file_value);
    sources.insert(
        key.to_string(),
        ValueSource {
            source: "config_file".to_string(),
            key: String::new(),
            value: value.clone(),
        },
    );
    value
}

fn normalized_config_schema_version(version: i64) -> i64 {
    version.max(0)
}

fn collect_env_hits() -> Vec<EnvHit> {
    env::var("AWIKI_CLI_WORKSPACE_HOME_DIR")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(|value| {
            vec![EnvHit {
                key: "AWIKI_CLI_WORKSPACE_HOME_DIR".to_string(),
                value,
                tier: "canonical_env".to_string(),
                target: "workspace_home_dir".to_string(),
            }]
        })
        .unwrap_or_default()
}

fn host_notify_file_path(paths: &Paths, config: &HostNotifyConfig, sink: &str) -> String {
    if sink != "file" {
        return String::new();
    }
    if !config.file_path.trim().is_empty() {
        return config.file_path.trim().to_string();
    }
    Path::new(&paths.state_dir)
        .join(DEFAULT_HOST_NOTIFY_FILE)
        .to_string_lossy()
        .into_owned()
}

fn resolve_openclaw_fields(
    config: &HostNotifyConfig,
    sink: &str,
) -> (String, ValueSource, String, String) {
    if sink != "openclaw" {
        return (
            String::new(),
            ValueSource {
                source: "unset".to_string(),
                key: String::new(),
                value: String::new(),
            },
            String::new(),
            String::new(),
        );
    }
    let (hook_url, hook_source) = choose_value(
        "",
        false,
        &config.openclaw.hook_url,
        DEFAULT_OPENCLAW_HOOK_URL,
    );
    (
        hook_url,
        hook_source,
        default_trimmed_string(&config.openclaw.agent_id, DEFAULT_OPENCLAW_AGENT_ID),
        default_trimmed_string(&config.openclaw.hook_name, DEFAULT_OPENCLAW_HOOK_NAME),
    )
}

fn resolve_hermes_fields(config: &HostNotifyConfig, sink: &str) -> (String, String) {
    if sink != "hermes" {
        return (String::new(), String::new());
    }
    let notify_url = if config.hermes.notify_url.trim().is_empty() {
        &config.webhook.notify_url
    } else {
        &config.hermes.notify_url
    };
    (
        default_trimmed_string(notify_url, DEFAULT_HERMES_NOTIFY_URL),
        default_string(&config.hermes.deliver, DEFAULT_HERMES_DELIVER_TARGET),
    )
}

fn normalize_host_notify_sink(value: &str) -> String {
    let normalized = default_string(value, DEFAULT_HOST_NOTIFY_SINK);
    if normalized == "webhook" {
        "hermes".to_string()
    } else {
        normalized
    }
}

pub fn normalize_host_notify_sink_for_write(value: &str) -> anyhow::Result<String> {
    let normalized = normalize_host_notify_sink(value);
    validate_host_notify_sink(&normalized)?;
    Ok(normalized)
}

fn validate_host_notify_sink(value: &str) -> anyhow::Result<()> {
    match value {
        "noop" | "log" | "file" | "openclaw" | "hermes" => Ok(()),
        _ => anyhow::bail!("unsupported host notify sink"),
    }
}

pub fn derive_anp_service_endpoint(service_base_url: &str) -> String {
    join_base_url(service_base_url, DEFAULT_ANP_PATH)
}

pub fn derive_anp_service_did(service_base_url: &str) -> String {
    format!("did:wba:{}", service_host_from_base_url(service_base_url))
}

pub fn normalize_did_domain(raw: &str) -> anyhow::Result<String> {
    let normalized = raw.trim().to_ascii_lowercase();
    let normalized = normalized.trim_end_matches('.').to_string();
    if normalized.is_empty() {
        anyhow::bail!("did_domain is required");
    }
    if normalized.contains("://") {
        anyhow::bail!("did_domain must be a bare domain without a URL scheme");
    }
    if normalized.contains(['/', '?', '#']) {
        anyhow::bail!("did_domain must not include a path, query, or fragment");
    }
    if normalized.contains(':') {
        anyhow::bail!("did_domain must not include a port");
    }
    if normalized.chars().any(char::is_whitespace) {
        anyhow::bail!("did_domain must not contain whitespace");
    }
    if normalized.contains('@') || normalized.contains('%') {
        anyhow::bail!("did_domain must be a bare domain");
    }
    Ok(normalized)
}

pub fn normalize_base_url(base_url: &str) -> String {
    base_url.trim().trim_end_matches('/').to_string()
}

pub fn join_base_url(base_url: &str, path: &str) -> String {
    let base = normalize_base_url(base_url);
    if base.is_empty() {
        return path.trim().to_string();
    }
    let mut path = path.trim().to_string();
    if path.is_empty() {
        return base;
    }
    if !path.starts_with('/') {
        path.insert(0, '/');
    }
    format!("{base}{path}")
}

pub fn derive_websocket_url(base_url: &str, path: &str) -> String {
    let http_url = join_base_url(base_url, path);
    let trimmed = http_url.trim();
    if let Some(rest) = trimmed.strip_prefix("https://") {
        return format!("wss://{rest}");
    }
    if let Some(rest) = trimmed.strip_prefix("http://") {
        return format!("ws://{rest}");
    }
    trimmed.to_string()
}

fn service_host_from_base_url(service_base_url: &str) -> String {
    let normalized = normalize_base_url(service_base_url);
    normalized
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or(DEFAULT_DID_DOMAIN)
        .split(':')
        .next()
        .unwrap_or(DEFAULT_DID_DOMAIN)
        .to_ascii_lowercase()
}

fn default_runtime_bridge_path(paths: &Paths) -> String {
    if cfg!(windows) {
        let digest = Sha256::digest(paths.workspace_home_dir.as_bytes());
        let hex = format!("{digest:x}");
        return format!(r"\\.\pipe\awiki-cli-{}", &hex[..16]);
    }
    Path::new(&paths.state_dir)
        .join("message-daemon.sock")
        .to_string_lossy()
        .into_owned()
}

fn default_string(value: &str, default: &str) -> String {
    if value.trim().is_empty() {
        default.to_string()
    } else {
        value.trim().to_ascii_lowercase()
    }
}

fn default_trimmed_string(value: &str, default: &str) -> String {
    if value.trim().is_empty() {
        default.to_string()
    } else {
        value.trim().to_string()
    }
}

fn expand_home(home: &Path, value: &str) -> PathBuf {
    value
        .strip_prefix("~/")
        .map(|rest| home.join(rest))
        .unwrap_or_else(|| PathBuf::from(value))
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_config_accepts_distinct_service_endpoints() {
        let raw = r#"
schema_version: 1
services:
  service_base_url: https://base.example.test/
  user_service_endpoint: https://users.example.test/
  message_service_endpoint: https://messages.example.test/
  did_domain: example.test
"#;

        let config = parse_file_config(raw).expect("parse config");

        assert_eq!(
            config.services.service_base_url,
            "https://base.example.test/"
        );
        assert_eq!(
            config.services.user_service_endpoint,
            "https://users.example.test/"
        );
        assert_eq!(
            config.services.message_service_endpoint,
            "https://messages.example.test/"
        );
    }

    #[test]
    fn resolve_service_endpoint_field_defaults_to_service_base_url() {
        let mut sources = BTreeMap::new();

        let endpoint = super::resolve_service_endpoint_field(
            "message_service_endpoint",
            "",
            "https://base.example.test",
            &mut sources,
        );

        assert_eq!(endpoint, "https://base.example.test");
        let source = sources
            .get("message_service_endpoint")
            .expect("source entry");
        assert_eq!(source.source, "derived_default");
        assert_eq!(source.key, "service_base_url");
    }
}
