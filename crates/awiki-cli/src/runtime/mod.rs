use crate::config::{self, Resolved};
use serde::Serialize;
use serde_json::{json, Value};
use std::fs;

pub mod hermes_bridge;
pub mod listener;
pub mod openclaw_routes;

const OPENCLAW_HOOK_TOKEN_ENV: &str = "OPENCLAW_HOOK_TOKEN";
const OPENCLAW_GATEWAY_PORT_ENV: &str = "OPENCLAW_GATEWAY_PORT";
const DEFAULT_OPENCLAW_GATEWAY_PORT: u16 = 18789;
const DEFAULT_OPENCLAW_HOOK_PATH: &str = "/hooks/agent";

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
    let sink = resolved.host_notify_sink.to_ascii_lowercase();
    RuntimeResolved {
        mode,
        socket_path: resolved.runtime_socket_path.clone(),
        listener: ListenerConfig {
            enabled: resolved.runtime_listener_enabled,
            auto_install: resolved.runtime_listener_auto_install,
            auto_start: resolved.runtime_listener_auto_start,
        },
        host_notify: HostNotifyConfig {
            enabled: resolved.host_notify_enabled,
            sink: sink.clone(),
            file_path: if sink == "file" {
                resolved.host_notify_file_path.clone()
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
    match listener::status_for(resolved, installed, running) {
        Ok(status) => listener::to_value(status),
        Err(err) => json!({
            "mode": resolve(resolved).mode,
            "installed": installed,
            "running": running,
            "service_platform": "rust-local",
            "bridge_available": false,
            "host_notify": {},
            "warnings": [format!("listener status unavailable: {err}")],
        }),
    }
}

pub fn current_listener_status(resolved: &Resolved) -> Value {
    listener_status(
        resolved,
        listener_installed(resolved),
        listener_running(resolved),
    )
}

pub fn apply_runtime_policy(resolved: &Resolved) -> anyhow::Result<Value> {
    ensure_runtime_dirs(resolved)?;
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
    write_listener_state(resolved, true, false)?;
    Ok(listener_status(resolved, true, false))
}

pub fn start_listener(resolved: &Resolved) -> anyhow::Result<Value> {
    ensure_runtime_dirs(resolved)?;
    if resolve(resolved).mode != "websocket" {
        anyhow::bail!("runtime mode must be websocket before starting the listener");
    }
    write_listener_state(resolved, true, true)?;
    Ok(listener_status(resolved, true, true))
}

pub fn stop_listener(resolved: &Resolved) -> anyhow::Result<Value> {
    ensure_runtime_dirs(resolved)?;
    write_listener_state(resolved, listener_installed(resolved), false)?;
    Ok(listener_status(
        resolved,
        listener_installed(resolved),
        false,
    ))
}

pub fn uninstall_listener(resolved: &Resolved) -> anyhow::Result<Value> {
    ensure_runtime_dirs(resolved)?;
    write_listener_state(resolved, false, false)?;
    Ok(listener_status(resolved, false, false))
}

pub fn host_notify_config_view(resolved: &Resolved) -> anyhow::Result<Value> {
    let settings = effective_openclaw_settings(resolved);
    let routes = openclaw_routes::load_routes(&resolved.paths)?;
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
            "deliver": if resolved.host_notify_hermes_deliver.is_empty() { hermes_bridge::DEFAULT_DELIVER_TARGET } else { &resolved.host_notify_hermes_deliver },
            "secret_configured": false,
            "secret_source": "unset",
            "secret_env_fallback": "AWIKI_HOST_NOTIFY_HERMES_SECRET",
            "secret_env_legacy": "AWIKI_WEBHOOK_SECRET",
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
    if host.eq_ignore_ascii_case("localhost") || host == "127.0.0.1" || host == "::1" {
        return Ok(());
    }
    anyhow::bail!("runtime.host_notify.openclaw.hook_url must use a loopback host")
}

#[derive(Debug)]
struct OpenClawSettings {
    hook_url: String,
    hook_url_source: &'static str,
    detected_webhook_port: u16,
    detected_webhook_source: &'static str,
    detected_webhook_path: &'static str,
    detected_webhook_path_source: &'static str,
    token_configured: bool,
    token_source: String,
}

fn effective_openclaw_settings(resolved: &Resolved) -> OpenClawSettings {
    let detected_port = std::env::var(OPENCLAW_GATEWAY_PORT_ENV)
        .ok()
        .and_then(|value| value.trim().parse::<u16>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_OPENCLAW_GATEWAY_PORT);
    let detected_source = if std::env::var(OPENCLAW_GATEWAY_PORT_ENV)
        .ok()
        .is_some_and(|value| value.trim().parse::<u16>().is_ok())
    {
        "environment"
    } else {
        "default"
    };
    let configured = resolved.host_notify_openclaw_hook_url.trim();
    let (hook_url, hook_url_source) = if configured.is_empty() {
        (
            format!("http://127.0.0.1:{detected_port}{DEFAULT_OPENCLAW_HOOK_PATH}"),
            "auto_detected",
        )
    } else {
        (configured.to_string(), "config_file")
    };
    let (config_token, config_source) = config::read_openclaw_token(&resolved.paths);
    let env_token = std::env::var(OPENCLAW_HOOK_TOKEN_ENV)
        .unwrap_or_default()
        .trim()
        .to_string();
    let token_source = if !config_token.trim().is_empty() {
        config_source
    } else if !env_token.is_empty() {
        "environment".to_string()
    } else {
        "unset".to_string()
    };
    OpenClawSettings {
        hook_url,
        hook_url_source,
        detected_webhook_port: detected_port,
        detected_webhook_source: detected_source,
        detected_webhook_path: DEFAULT_OPENCLAW_HOOK_PATH,
        detected_webhook_path_source: "default",
        token_configured: token_source != "unset",
        token_source,
    }
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
