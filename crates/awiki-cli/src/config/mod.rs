use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

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

#[derive(Debug, Serialize, Default)]
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

#[derive(Debug, Serialize, Default)]
pub struct IdentityConfig {
    #[serde(default)]
    pub active: String,
}

#[derive(Debug, Serialize, Default)]
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

#[derive(Debug, Serialize, Default)]
pub struct ListenerConfig {
    pub enabled: Option<bool>,
    pub auto_install: Option<bool>,
    pub auto_start: Option<bool>,
}

#[derive(Debug, Serialize, Default)]
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

#[derive(Debug, Serialize, Default)]
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

#[derive(Debug, Serialize, Default)]
pub struct HermesConfig {
    #[serde(default)]
    pub notify_url: String,
    #[serde(default)]
    pub deliver: String,
    #[serde(default)]
    pub secret: String,
}

#[derive(Debug, Serialize, Default)]
pub struct LegacyWebhookConfig {
    #[serde(default)]
    pub notify_url: String,
    #[serde(default)]
    pub secret: String,
}

#[derive(Debug, Serialize, Default)]
pub struct OutputConfig {
    #[serde(default)]
    pub format: String,
    pub no_color: Option<bool>,
}

#[derive(Debug, Serialize, Default)]
pub struct ServicesConfig {
    #[serde(default)]
    pub service_base_url: String,
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

#[derive(Debug, Serialize, Default)]
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
    let (openclaw_hook_url, openclaw_agent_id, openclaw_hook_name) =
        resolve_openclaw_fields(&file_config.runtime.host_notify, &host_notify_sink);
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

pub fn write_file_config(path: &str, resolved: &Resolved) -> anyhow::Result<()> {
    let config = FileConfig {
        schema_version: CONFIG_SCHEMA_VERSION,
        identity: IdentityConfig {
            active: resolved.active_identity.clone(),
        },
        runtime: RuntimeConfig {
            mode: resolved.runtime_mode.clone(),
            socket_path: resolved.runtime_socket_path.clone(),
            listener: ListenerConfig {
                enabled: Some(resolved.runtime_listener_enabled),
                auto_install: Some(resolved.runtime_listener_auto_install),
                auto_start: Some(resolved.runtime_listener_auto_start),
            },
            host_notify: HostNotifyConfig {
                enabled: Some(resolved.host_notify_enabled),
                sink: resolved.host_notify_sink.clone(),
                file_path: resolved.host_notify_file_path.clone(),
                openclaw: OpenClawConfig {
                    hook_url: resolved.host_notify_openclaw_hook_url.clone(),
                    agent_id: resolved.host_notify_openclaw_agent_id.clone(),
                    hook_name: resolved.host_notify_openclaw_hook_name.clone(),
                    token: String::new(),
                },
                hermes: HermesConfig {
                    notify_url: resolved.host_notify_hermes_notify_url.clone(),
                    deliver: resolved.host_notify_hermes_deliver.clone(),
                    secret: String::new(),
                },
                webhook: LegacyWebhookConfig::default(),
            },
        },
        output: OutputConfig {
            format: resolved.output_format.clone(),
            no_color: Some(resolved.no_color),
        },
        services: ServicesConfig {
            service_base_url: resolved.service_base_url.clone(),
            did_domain: resolved.did_domain.clone(),
            anp_service_endpoint: resolved.anp_service_endpoint.clone(),
            anp_service_did: resolved.anp_service_did.clone(),
            ca_bundle: resolved.ca_bundle.clone(),
            mail_service_url: resolved.mail_service_url.clone(),
        },
        update: UpdateConfig::default(),
    };
    write_raw_file_config(path, &config)
}

fn write_raw_file_config(path: &str, config: &FileConfig) -> anyhow::Result<()> {
    if let Some(parent) = Path::new(path).parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, render_file_config(config))?;
    Ok(())
}

pub fn update_runtime_settings(paths: &Paths, mode: &str, socket_path: &str) -> anyhow::Result<()> {
    update_file_config(&paths.config_file, |config| {
        config.runtime.mode = mode.trim().to_ascii_lowercase();
        if !socket_path.trim().is_empty() {
            config.runtime.socket_path = socket_path.trim().to_string();
        }
        Ok(())
    })
}

pub fn update_did_domain(paths: &Paths, value: &str) -> anyhow::Result<String> {
    let normalized = normalize_did_domain(value)?;
    update_file_config(&paths.config_file, |config| {
        config.services.did_domain = normalized.clone();
        Ok(())
    })?;
    Ok(normalized)
}

pub fn update_runtime_listener_settings(
    paths: &Paths,
    enabled: Option<bool>,
    auto_install: Option<bool>,
    auto_start: Option<bool>,
) -> anyhow::Result<()> {
    update_file_config(&paths.config_file, |config| {
        if let Some(value) = enabled {
            config.runtime.listener.enabled = Some(value);
        }
        if let Some(value) = auto_install {
            config.runtime.listener.auto_install = Some(value);
        }
        if let Some(value) = auto_start {
            config.runtime.listener.auto_start = Some(value);
        }
        Ok(())
    })
}

pub fn update_host_notify_sink(paths: &Paths, sink: &str) -> anyhow::Result<()> {
    let normalized = normalize_host_notify_sink_for_write(sink)?;
    update_file_config(&paths.config_file, |config| {
        config.runtime.host_notify.sink = normalized;
        config.runtime.host_notify.enabled = Some(true);
        Ok(())
    })
}

pub fn update_openclaw_settings(paths: &Paths, hook_url: Option<&str>) -> anyhow::Result<()> {
    update_file_config(&paths.config_file, |config| {
        if let Some(value) = hook_url {
            config.runtime.host_notify.openclaw.hook_url = value.trim().to_string();
        }
        Ok(())
    })
}

pub fn set_openclaw_token(paths: &Paths, token: &str) -> anyhow::Result<()> {
    update_file_config(&paths.config_file, |config| {
        config.runtime.host_notify.openclaw.token = token.to_string();
        Ok(())
    })
}

pub fn clear_openclaw_token(paths: &Paths) -> anyhow::Result<()> {
    update_file_config(&paths.config_file, |config| {
        config.runtime.host_notify.openclaw.token.clear();
        Ok(())
    })
}

pub fn read_openclaw_token(paths: &Paths) -> (String, String) {
    if paths.config_file.trim().is_empty() {
        return (String::new(), "unset".to_string());
    }
    let (config, _, error) = read_file_config(&paths.config_file);
    if error.is_empty() {
        let token = config.runtime.host_notify.openclaw.token.trim();
        if !token.is_empty() {
            return (token.to_string(), "config_file".to_string());
        }
    }
    (String::new(), "unset".to_string())
}

fn update_file_config(
    path: &str,
    mutate: impl FnOnce(&mut FileConfig) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    if let Some(parent) = Path::new(path).parent() {
        fs::create_dir_all(parent)?;
    }
    let (mut config, _, error) = read_file_config(path);
    if !error.is_empty() {
        anyhow::bail!(error);
    }
    mutate(&mut config)?;
    write_raw_file_config(path, &config)
}

fn read_file_config(path: &str) -> (FileConfig, bool, String) {
    match fs::read_to_string(path) {
        Ok(raw) => (parse_file_config(&raw), true, String::new()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            (FileConfig::default(), false, String::new())
        }
        Err(err) => (FileConfig::default(), false, err.to_string()),
    }
}

fn render_file_config(config: &FileConfig) -> String {
    format!(
        concat!(
            "schema_version: {}\n",
            "identity:\n",
            "  active: {}\n",
            "runtime:\n",
            "  mode: {}\n",
            "  socket_path: {}\n",
            "  listener:\n",
            "    enabled: {}\n",
            "    auto_install: {}\n",
            "    auto_start: {}\n",
            "  host_notify:\n",
            "    enabled: {}\n",
            "    sink: {}\n",
            "    file_path: {}\n",
            "    openclaw:\n",
            "      hook_url: {}\n",
            "      agent_id: {}\n",
            "      hook_name: {}\n",
            "      token: {}\n",
            "    hermes:\n",
            "      notify_url: {}\n",
            "      deliver: {}\n",
            "      secret: {}\n",
            "    webhook:\n",
            "      notify_url: {}\n",
            "      secret: {}\n",
            "output:\n",
            "  format: {}\n",
            "  no_color: {}\n",
            "services:\n",
            "  service_base_url: {}\n",
            "  did_domain: {}\n",
            "  anp_service_endpoint: {}\n",
            "  anp_service_did: {}\n",
            "  ca_bundle: {}\n",
            "  mail_service_url: {}\n",
            "update:\n",
            "  disable_strict_version: {}\n",
            "  metadata_cache_ttl_seconds: {}\n"
        ),
        config.schema_version,
        config.identity.active,
        config.runtime.mode,
        config.runtime.socket_path,
        config
            .runtime
            .listener
            .enabled
            .unwrap_or(DEFAULT_LISTENER_ENABLED),
        config
            .runtime
            .listener
            .auto_install
            .unwrap_or(DEFAULT_LISTENER_AUTO_INSTALL),
        config
            .runtime
            .listener
            .auto_start
            .unwrap_or(DEFAULT_LISTENER_AUTO_START),
        config
            .runtime
            .host_notify
            .enabled
            .unwrap_or(DEFAULT_HOST_NOTIFY_ENABLED),
        config.runtime.host_notify.sink,
        config.runtime.host_notify.file_path,
        config.runtime.host_notify.openclaw.hook_url,
        config.runtime.host_notify.openclaw.agent_id,
        config.runtime.host_notify.openclaw.hook_name,
        config.runtime.host_notify.openclaw.token,
        config.runtime.host_notify.hermes.notify_url,
        config.runtime.host_notify.hermes.deliver,
        config.runtime.host_notify.hermes.secret,
        config.runtime.host_notify.webhook.notify_url,
        config.runtime.host_notify.webhook.secret,
        config.output.format,
        config.output.no_color.unwrap_or(false),
        config.services.service_base_url,
        config.services.did_domain,
        config.services.anp_service_endpoint,
        config.services.anp_service_did,
        config.services.ca_bundle,
        config.services.mail_service_url,
        config.update.disable_strict_version,
        config.update.metadata_cache_ttl_seconds,
    )
}

fn parse_file_config(raw: &str) -> FileConfig {
    let mut config = FileConfig::default();
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
        set_config_value(&mut config, &path, &value);
    }
    config
}

fn strip_yaml_scalar(value: &str) -> String {
    value
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .to_string()
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

fn resolve_openclaw_fields(config: &HostNotifyConfig, sink: &str) -> (String, String, String) {
    if sink != "openclaw" {
        return (String::new(), String::new(), String::new());
    }
    (
        default_trimmed_string(&config.openclaw.hook_url, DEFAULT_OPENCLAW_HOOK_URL),
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

fn join_base_url(base_url: &str, path: &str) -> String {
    let base = normalize_base_url(base_url);
    if base.is_empty() {
        return path.trim().to_string();
    }
    let mut path = path.trim().to_string();
    if !path.starts_with('/') {
        path.insert(0, '/');
    }
    format!("{base}{path}")
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
