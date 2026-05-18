use crate::config::{self, Resolved};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::env;
use std::fs;
use std::net::IpAddr;
use std::path::PathBuf;

pub mod bridge;
pub mod hermes_bridge;
pub mod hermes_host_notify;
pub mod host_notify;
pub mod host_notify_sink;
pub mod listener;
pub mod listener_bridge_connection;
pub mod listener_bridge_dispatch;
pub mod listener_bridge_runtime;
pub mod listener_connect_session;
pub mod listener_contact_sync;
pub mod listener_foreground;
pub mod listener_handle_lookup;
pub mod listener_identity_watch;
pub mod listener_json_helpers;
pub mod listener_known_sessions;
pub mod listener_local_notification_flush;
pub mod listener_local_notifications;
pub mod listener_message_records;
pub mod listener_notification_consume;
pub mod listener_notification_execute;
pub mod listener_notification_handler;
pub mod listener_notification_plan;
pub mod listener_secure_ack_delivery;
pub mod listener_secure_ack_in_process;
pub mod listener_secure_inbox_poll;
pub mod listener_secure_normalize;
pub mod listener_secure_notifications;
pub mod listener_secure_outbox_flush;
pub mod listener_secure_replay;
pub mod listener_secure_sessions;
pub mod listener_secure_sync;
pub mod listener_service;
pub mod listener_service_did;
pub mod listener_session_bootstrap;
pub mod listener_session_lookup;
pub mod listener_session_loop;
pub mod listener_session_methods;
pub mod listener_session_state;
pub mod listener_shutdown_signal;
pub mod listener_supervisor_init;
pub mod listener_supervisor_run;
pub mod listener_supervisor_shutdown;
pub mod listener_systemd;
pub mod listener_ws_transport;
pub mod listener_wsclient;
pub mod openclaw_host_notify;
pub mod openclaw_routes;
pub mod openclaw_webhook;

const OPENCLAW_HOOK_TOKEN_ENV: &str = "OPENCLAW_HOOK_TOKEN";
const OPENCLAW_GATEWAY_PORT_ENV: &str = "OPENCLAW_GATEWAY_PORT";
const OPENCLAW_CONFIG_PATH_ENV: &str = "OPENCLAW_CONFIG_PATH";
const DEFAULT_OPENCLAW_GATEWAY_PORT: i64 = 18789;
const DEFAULT_OPENCLAW_HOOKS_PATH: &str = "/hooks";

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeResolved {
    pub mode: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub socket_path: String,
    pub listener: ListenerConfig,
    pub host_notify: HostNotifyConfig,
}

#[derive(Debug, Clone, Serialize)]
pub struct ListenerConfig {
    pub enabled: bool,
    pub auto_install: bool,
    pub auto_start: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct HostNotifyConfig {
    pub enabled: bool,
    pub sink: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub file_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub openclaw: Option<OpenClawConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hermes: Option<HermesConfig>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenClawConfig {
    pub hook_url: String,
    pub agent_id: String,
    pub hook_name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct HermesConfig {
    pub notify_url: String,
    pub deliver: String,
}

pub fn resolve(resolved: &Resolved) -> RuntimeResolved {
    let mode = normalize_runtime_mode(&resolved.runtime_mode);
    let mut sink = default_string(&resolved.host_notify_sink, "log").to_ascii_lowercase();
    if sink == "webhook" {
        sink = "hermes".to_string();
    }
    RuntimeResolved {
        mode,
        socket_path: bridge::resolved_bridge_endpoint(resolved),
        listener: ListenerConfig {
            enabled: resolved.runtime_listener_enabled,
            auto_install: resolved.runtime_listener_auto_install,
            auto_start: resolved.runtime_listener_auto_start,
        },
        host_notify: HostNotifyConfig {
            enabled: resolved.host_notify_enabled,
            sink: sink.clone(),
            file_path: if sink == "file" {
                resolved.host_notify_file_path.trim().to_string()
            } else {
                String::new()
            },
            openclaw: (sink == "openclaw").then(|| OpenClawConfig {
                hook_url: effective_openclaw_settings(resolved).hook_url,
                agent_id: default_string(&resolved.host_notify_openclaw_agent_id, "main"),
                hook_name: default_string(&resolved.host_notify_openclaw_hook_name, "AWiki"),
            }),
            hermes: (sink == "hermes").then(|| HermesConfig {
                notify_url: default_string(
                    &resolved.host_notify_hermes_notify_url,
                    hermes_bridge::DEFAULT_NOTIFY_URL,
                ),
                deliver: default_string(
                    &resolved.host_notify_hermes_deliver,
                    hermes_bridge::DEFAULT_DELIVER_TARGET,
                ),
            }),
        },
    }
}

pub fn runtime_value(resolved: &Resolved) -> Value {
    serde_json::to_value(resolve(resolved)).unwrap_or_else(|_| json!({}))
}

pub fn listener_status(resolved: &Resolved, installed: bool, running: bool) -> Value {
    listener_status_with_platform(resolved, installed, running, "rust-local")
}

pub fn listener_status_with_platform(
    resolved: &Resolved,
    installed: bool,
    running: bool,
    service_platform: &str,
) -> Value {
    match listener::status_for(resolved, installed, running, service_platform) {
        Ok(status) => listener::to_value(status),
        Err(err) => json!({
            "mode": resolve(resolved).mode,
            "installed": installed,
            "running": running,
            "service_platform": service_platform,
            "bridge_available": false,
            "host_notify": {},
            "warnings": [format!("listener status unavailable: {err}")],
        }),
    }
}

pub fn current_listener_status(resolved: &Resolved) -> Value {
    if listener_systemd::is_supported() {
        if let Ok(status) = listener_systemd::status_value(resolved) {
            return status;
        }
    }
    listener_status(
        resolved,
        listener_installed(resolved),
        listener_running(resolved),
    )
}

pub fn apply_runtime_policy(resolved: &Resolved) -> anyhow::Result<Value> {
    ensure_runtime_dirs(resolved)?;
    if listener_systemd::is_supported() {
        if resolve(resolved).mode != "websocket" || !resolved.runtime_listener_enabled {
            return Ok(listener::to_value(listener_systemd::stop(resolved)?));
        }
        let mut status = if resolved.runtime_listener_auto_install {
            listener_systemd::install(resolved)?
        } else {
            listener_systemd::status(resolved).and_then(|status| {
                listener::status_for(
                    resolved,
                    status.installed,
                    status.running,
                    listener_systemd::service_platform(),
                )
            })?
        };
        if resolved.runtime_listener_auto_start {
            status = listener_systemd::start(resolved)?;
        }
        return Ok(listener::to_value(status));
    }
    if resolve(resolved).mode == "websocket" && resolved.runtime_listener_enabled {
        let installed = resolved.runtime_listener_auto_install || listener_installed(resolved);
        let running = installed && resolved.runtime_listener_auto_start;
        write_listener_state(resolved, installed, running)?;
        Ok(listener_status(resolved, installed, running))
    } else {
        write_listener_state(resolved, listener_installed(resolved), false)?;
        Ok(listener_status(
            resolved,
            listener_installed(resolved),
            false,
        ))
    }
}

pub fn install_listener(resolved: &Resolved) -> anyhow::Result<Value> {
    ensure_runtime_dirs(resolved)?;
    if listener_systemd::is_supported() {
        return Ok(listener::to_value(listener_systemd::install(resolved)?));
    }
    write_listener_state(resolved, true, false)?;
    Ok(listener_status(resolved, true, false))
}

pub fn start_listener(resolved: &Resolved) -> anyhow::Result<Value> {
    ensure_runtime_dirs(resolved)?;
    if listener_systemd::is_supported() {
        return Ok(listener::to_value(listener_systemd::start(resolved)?));
    }
    if resolve(resolved).mode != "websocket" {
        anyhow::bail!("runtime mode must be websocket before starting the listener");
    }
    write_listener_state(resolved, true, true)?;
    Ok(listener_status(resolved, true, true))
}

pub fn restart_listener(resolved: &Resolved) -> anyhow::Result<Value> {
    ensure_runtime_dirs(resolved)?;
    if listener_systemd::is_supported() {
        return Ok(listener::to_value(listener_systemd::restart(resolved)?));
    }
    start_listener(resolved)
}

pub fn stop_listener(resolved: &Resolved) -> anyhow::Result<Value> {
    ensure_runtime_dirs(resolved)?;
    if listener_systemd::is_supported() {
        return Ok(listener::to_value(listener_systemd::stop(resolved)?));
    }
    write_listener_state(resolved, listener_installed(resolved), false)?;
    Ok(listener_status(
        resolved,
        listener_installed(resolved),
        false,
    ))
}

pub fn uninstall_listener(resolved: &Resolved) -> anyhow::Result<Value> {
    ensure_runtime_dirs(resolved)?;
    if listener_systemd::is_supported() {
        return Ok(listener::to_value(listener_systemd::uninstall(resolved)?));
    }
    write_listener_state(resolved, false, false)?;
    Ok(listener_status(resolved, false, false))
}

pub fn host_notify_config_view(resolved: &Resolved) -> anyhow::Result<Value> {
    let (file_config, _, _) = config::read_file_config(&resolved.paths.config_file);
    let settings = effective_openclaw_settings(resolved);
    let routes = openclaw_routes::load_routes(&resolved.paths)?;
    let hermes_notify_url = resolved.host_notify_hermes_notify_url.trim();
    let (_, hermes_secret_source) = hermes_host_notify::resolve_hermes_notify_secret_with_source(
        Some(resolved),
        hermes_notify_url,
    );
    let hermes_deliver = if resolved.host_notify_hermes_deliver.is_empty()
        && !file_config
            .runtime
            .host_notify
            .hermes
            .deliver
            .trim()
            .is_empty()
    {
        file_config.runtime.host_notify.hermes.deliver.trim()
    } else if resolved.host_notify_hermes_deliver.is_empty() {
        hermes_bridge::DEFAULT_DELIVER_TARGET
    } else {
        &resolved.host_notify_hermes_deliver
    };
    Ok(json!({
        "enabled": resolved.host_notify_enabled,
        "sink": resolved.host_notify_sink,
        "file_path": resolved.host_notify_file_path,
        "route_registry_path": openclaw_routes::routes_path(&resolved.paths),
        "routes": routes,
        "openclaw": {
            "hook_url": settings.hook_url,
            "hook_url_source": settings.hook_url_source,
            "detected_webhook_port": settings.detected_webhook_port,
            "detected_webhook_source": settings.detected_webhook_source,
            "detected_webhook_path": settings.detected_webhook_path,
            "detected_webhook_path_source": settings.detected_webhook_path_source,
            "token_configured": settings.token_configured,
            "token_source": settings.token_source,
        },
        "hermes": {
            "notify_url": resolved.host_notify_hermes_notify_url,
            "deliver": hermes_deliver,
            "secret_configured": hermes_secret_source != "unset",
            "secret_source": hermes_secret_source,
            "secret_env_fallback": "AWIKI_HOST_NOTIFY_HERMES_SECRET",
            "secret_env_legacy": hermes_host_notify::LEGACY_WEBHOOK_NOTIFY_SECRET_ENV,
        }
    }))
}

pub fn validate_openclaw_hook_url(value: &str) -> anyhow::Result<()> {
    let trimmed = value.trim();
    let Some((scheme, rest)) = trimmed.split_once("://") else {
        anyhow::bail!("runtime.host_notify.openclaw.hook_url must use http or https");
    };
    if scheme != "http" && scheme != "https" {
        anyhow::bail!("runtime.host_notify.openclaw.hook_url must use http or https");
    }
    let host_port = rest.split('/').next().unwrap_or_default();
    let host = host_port
        .strip_prefix('[')
        .and_then(|body| body.split(']').next())
        .unwrap_or_else(|| host_port.split(':').next().unwrap_or_default())
        .trim();
    if host.is_empty() {
        anyhow::bail!("runtime.host_notify.openclaw.hook_url must include a host");
    }
    if host.eq_ignore_ascii_case("localhost") {
        return Ok(());
    }
    if host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback()) {
        return Ok(());
    }
    anyhow::bail!("runtime.host_notify.openclaw.hook_url must use a loopback host")
}

#[derive(Debug)]
pub(crate) struct OpenClawSettings {
    hook_url: String,
    hook_url_source: &'static str,
    detected_webhook_port: i64,
    detected_webhook_source: String,
    detected_webhook_path: String,
    detected_webhook_path_source: String,
    token: String,
    token_configured: bool,
    token_source: String,
}

pub(crate) fn effective_openclaw_settings(resolved: &Resolved) -> OpenClawSettings {
    let probe = probe_openclaw_config(DEFAULT_OPENCLAW_GATEWAY_PORT, DEFAULT_OPENCLAW_HOOKS_PATH);
    let detected_port = probe.gateway_port;
    let detected_source = probe.gateway_source.clone();
    let detected_path = build_openclaw_hook_endpoint_path(&probe.hooks_path);
    let configured = resolved.host_notify_openclaw_hook_url.trim();
    let explicit_hook_url = resolved
        .sources
        .get("host_notify_openclaw_hook_url")
        .is_some_and(|source| source.source == "config_file");
    let (hook_url, hook_url_source) = if explicit_hook_url && !configured.is_empty() {
        (configured.to_string(), "config_file")
    } else {
        (
            format!("http://127.0.0.1:{detected_port}{detected_path}"),
            "auto_detected",
        )
    };
    let (config_token, config_source) = config::read_openclaw_token(&resolved.paths);
    let env_token = env::var(OPENCLAW_HOOK_TOKEN_ENV)
        .unwrap_or_default()
        .trim()
        .to_string();
    let (token, token_source) = if !config_token.trim().is_empty() {
        (config_token, config_source)
    } else if !env_token.is_empty() {
        (env_token, "environment".to_string())
    } else if !probe.hook_token.is_empty() {
        (probe.hook_token, "openclaw_config".to_string())
    } else {
        (String::new(), "unset".to_string())
    };
    OpenClawSettings {
        hook_url,
        hook_url_source,
        detected_webhook_port: detected_port,
        detected_webhook_source: detected_source,
        detected_webhook_path: detected_path,
        detected_webhook_path_source: probe.hooks_source,
        token,
        token_configured: token_source != "unset",
        token_source,
    }
}

struct OpenClawConfigProbe {
    gateway_port: i64,
    gateway_source: String,
    hooks_path: String,
    hooks_source: String,
    hook_token: String,
}

#[derive(Debug, Default, Deserialize)]
struct OpenClawConfigFile {
    #[serde(default)]
    gateway: OpenClawGatewayConfigFile,
    #[serde(default)]
    hooks: OpenClawHooksConfigFile,
}

#[derive(Debug, Default, Deserialize)]
struct OpenClawGatewayConfigFile {
    #[serde(default)]
    port: i64,
}

#[derive(Debug, Default, Deserialize)]
struct OpenClawHooksConfigFile {
    #[serde(default)]
    path: String,
    #[serde(default)]
    token: String,
}

fn probe_openclaw_config(default_port: i64, default_hooks_path: &str) -> OpenClawConfigProbe {
    let config_path = openclaw_config_path();
    let mut probe = OpenClawConfigProbe {
        gateway_port: default_port,
        gateway_source: "default".to_string(),
        hooks_path: normalize_openclaw_hooks_base_path(default_hooks_path),
        hooks_source: "default".to_string(),
        hook_token: String::new(),
    };
    if let Ok(env_port) = env::var(OPENCLAW_GATEWAY_PORT_ENV) {
        if let Some(port) = parse_openclaw_port(env_port.trim()) {
            probe.gateway_port = port;
            probe.gateway_source = "environment".to_string();
        }
    }
    if let Ok(raw) = fs::read_to_string(config_path) {
        let Ok(payload) = serde_json::from_str::<OpenClawConfigFile>(&raw) else {
            return probe;
        };
        if probe.gateway_source != "environment" && payload.gateway.port > 0 {
            let port = payload.gateway.port;
            probe.gateway_port = port;
            probe.gateway_source = "openclaw_config".to_string();
        }
        let path = payload.hooks.path.trim();
        if !path.is_empty() {
            probe.hooks_path = normalize_openclaw_hooks_base_path(path);
            probe.hooks_source = "openclaw_config".to_string();
        }
        probe.hook_token = payload.hooks.token.trim().to_string();
    }
    probe
}

fn parse_openclaw_port(raw: &str) -> Option<i64> {
    raw.parse::<i64>().ok().filter(|port| *port > 0)
}

fn openclaw_config_path() -> PathBuf {
    env::var(OPENCLAW_CONFIG_PATH_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| openclaw_home_dir().map(|home| home.join(".openclaw/openclaw.json")))
        .unwrap_or_else(|| PathBuf::from(".openclaw/openclaw.json"))
}

fn openclaw_home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        if let Some(home) = env::var_os("USERPROFILE") {
            return Some(PathBuf::from(home));
        }
        if let (Some(drive), Some(path)) = (env::var_os("HOMEDRIVE"), env::var_os("HOMEPATH")) {
            let mut home = PathBuf::from(drive);
            home.push(path);
            return Some(home);
        }
    }
    env::var_os("HOME").map(PathBuf::from)
}

fn normalize_openclaw_hooks_base_path(raw: &str) -> String {
    let mut value = raw.trim().to_string();
    if value.is_empty() {
        return DEFAULT_OPENCLAW_HOOKS_PATH.to_string();
    }
    if !value.starts_with('/') {
        value.insert(0, '/');
    }
    let mut parts = Vec::new();
    for part in value.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            segment => parts.push(segment),
        }
    }
    if parts.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", parts.join("/"))
    }
}

fn build_openclaw_hook_endpoint_path(base_path: &str) -> String {
    let base_path = normalize_openclaw_hooks_base_path(base_path);
    if base_path.ends_with("/agent") {
        return base_path;
    }
    if base_path == "/" {
        return "/agent".to_string();
    }
    format!("{}/agent", base_path.trim_end_matches('/'))
}

fn ensure_runtime_dirs(resolved: &Resolved) -> anyhow::Result<()> {
    fs::create_dir_all(&resolved.paths.state_dir)?;
    fs::create_dir_all(&resolved.paths.logs_dir)?;
    Ok(())
}

fn write_listener_state(resolved: &Resolved, installed: bool, running: bool) -> anyhow::Result<()> {
    ensure_runtime_dirs(resolved)?;
    let state = json!({ "installed": installed, "running": running });
    fs::write(
        listener_state_path(resolved),
        serde_json::to_vec_pretty(&state)?,
    )?;
    Ok(())
}

fn listener_installed(resolved: &Resolved) -> bool {
    read_listener_state_bool(resolved, "installed")
}

fn listener_running(resolved: &Resolved) -> bool {
    read_listener_state_bool(resolved, "running")
}

fn read_listener_state_bool(resolved: &Resolved, key: &str) -> bool {
    fs::read(listener_state_path(resolved))
        .ok()
        .and_then(|raw| serde_json::from_slice::<Value>(&raw).ok())
        .and_then(|value| value.get(key).and_then(Value::as_bool))
        .unwrap_or(false)
}

fn listener_state_path(resolved: &Resolved) -> String {
    std::path::Path::new(&resolved.paths.state_dir)
        .join("listener.local-state.json")
        .to_string_lossy()
        .into_owned()
}

fn normalize_runtime_mode(value: &str) -> String {
    if value.trim().eq_ignore_ascii_case("http") {
        "http".to_string()
    } else {
        "websocket".to_string()
    }
}

fn default_string(value: &str, default: &str) -> String {
    if value.trim().is_empty() {
        default.to_string()
    } else {
        value.trim().to_string()
    }
}
