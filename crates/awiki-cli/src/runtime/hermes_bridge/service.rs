use super::{BridgeConfig, BridgeStatus};
use crate::config::Resolved;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::time::{Duration, Instant};

pub const SERVICE_NAME_PREFIX: &str = "awiki-cli-hermes-bridge";
pub const SERVICE_DISPLAY_NAME_PREFIX: &str = "awiki-cli Hermes Bridge";
pub const SERVICE_DESCRIPTION: &str = "awiki-cli Hermes notify bridge";
pub const SERVICE_ARGUMENTS: &[&str] =
    &["runtime", "host-notify", "hermes", "bridge", "service-run"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeServiceConfigPlan {
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub arguments: Vec<String>,
    pub working_directory: String,
    pub env_workspace_home_dir: String,
    pub env_hermes_home: String,
    pub user_service: bool,
    pub keep_alive: bool,
    pub on_failure: String,
    pub on_failure_delay_duration: String,
    pub log_output: bool,
    pub log_directory: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeAdapterCommandPlan {
    pub executable: String,
    pub arguments: Vec<String>,
    pub env_hermes_home: Option<String>,
    pub stdout_inherits_parent: bool,
    pub stderr_inherits_parent: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeServiceStatusSnapshot {
    pub installed: bool,
    pub running: bool,
    pub platform: String,
    pub service_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeApplyDecision {
    EnsureInstalledThenStart,
    Restart,
    Start,
}

pub fn status_from_parts(
    service_name: String,
    config_result: anyhow::Result<BridgeConfig>,
    service_status_result: anyhow::Result<Option<BridgeServiceStatusSnapshot>>,
    mut health_available: impl FnMut(&str) -> bool,
) -> BridgeStatus {
    let mut status = BridgeStatus {
        service_name,
        service_platform: String::new(),
        installed: false,
        running: false,
        bridge_available: false,
        health_url: String::new(),
        config: None,
        warnings: Vec::new(),
    };
    let config = match config_result {
        Ok(config) => config,
        Err(err) => {
            status.warnings.push(err.to_string());
            return status;
        }
    };
    status.health_url = config.health_url.clone();
    match service_status_result {
        Ok(Some(service_status)) => {
            status.installed = service_status.installed;
            status.running = service_status.running;
            status.service_platform = service_status.platform;
            status.service_name = service_status.service_name;
        }
        Ok(None) => {
            status.service_platform = "rust-local".to_string();
        }
        Err(err) => status
            .warnings
            .push(format!("Hermes bridge service status unavailable: {err}")),
    }
    if status.running {
        status.bridge_available = health_available(&config.health_url);
        if !status.bridge_available {
            status
                .warnings
                .push("Hermes bridge health endpoint is not responding".to_string());
        }
    }
    status.warnings.extend(config.route_state.warnings.clone());
    status.config = Some(config);
    status
}

pub fn service_name_for(resolved: Option<&Resolved>) -> String {
    let workspace = resolved
        .map(|resolved| resolved.paths.workspace_home_dir.trim())
        .unwrap_or("default");
    if workspace.is_empty() {
        return SERVICE_NAME_PREFIX.to_string();
    }
    let digest = Sha256::digest(workspace.as_bytes());
    format!("{SERVICE_NAME_PREFIX}-{}", &format!("{digest:x}")[..12])
}

pub fn service_display_name_for(resolved: Option<&Resolved>) -> String {
    let Some(resolved) = resolved else {
        return SERVICE_DISPLAY_NAME_PREFIX.to_string();
    };
    let workspace = resolved.paths.workspace_home_dir.trim();
    let base = Path::new(workspace)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if base.is_empty() || base == "." || base == std::path::MAIN_SEPARATOR.to_string() {
        return SERVICE_DISPLAY_NAME_PREFIX.to_string();
    }
    format!("{SERVICE_DISPLAY_NAME_PREFIX} ({base})")
}

pub fn service_config_plan_for(
    resolved: &Resolved,
    hermes_home: &str,
    is_windows: bool,
) -> BridgeServiceConfigPlan {
    BridgeServiceConfigPlan {
        name: service_name_for(Some(resolved)),
        display_name: service_display_name_for(Some(resolved)),
        description: SERVICE_DESCRIPTION.to_string(),
        arguments: SERVICE_ARGUMENTS
            .iter()
            .map(|argument| (*argument).to_string())
            .collect(),
        working_directory: if is_windows {
            String::new()
        } else {
            resolved.paths.workspace_home_dir.clone()
        },
        env_workspace_home_dir: resolved.paths.workspace_home_dir.clone(),
        env_hermes_home: hermes_home.trim().to_string(),
        user_service: true,
        keep_alive: true,
        on_failure: "restart".to_string(),
        on_failure_delay_duration: "1s".to_string(),
        log_output: true,
        log_directory: resolved.paths.logs_dir.clone(),
    }
}

pub fn adapter_command_plan_for(config: &BridgeConfig) -> BridgeAdapterCommandPlan {
    BridgeAdapterCommandPlan {
        executable: config.python_executable.clone(),
        arguments: vec![
            config.adapter_script.clone(),
            "--host".to_string(),
            config.adapter_host.clone(),
            "--port".to_string(),
            config.adapter_port.to_string(),
            "--notify-secret".to_string(),
            config.notify_secret.clone(),
            "--hermes-webhook-url".to_string(),
            config.hermes_webhook_url.clone(),
            "--hermes-route-secret".to_string(),
            config.route_secret.clone(),
            "--log-level".to_string(),
            "INFO".to_string(),
        ],
        env_hermes_home: (!config.hermes_home.is_empty()).then(|| config.hermes_home.clone()),
        stdout_inherits_parent: true,
        stderr_inherits_parent: true,
    }
}

pub fn running_in_bridge_service_mode() -> bool {
    let args = std::env::args().collect::<Vec<_>>();
    running_in_bridge_service_mode_with(&args)
}

pub fn running_in_bridge_service_mode_with(args: &[String]) -> bool {
    if args.len() < 6 {
        return false;
    }
    args[1].trim().eq_ignore_ascii_case("runtime")
        && args[2].trim().eq_ignore_ascii_case("host-notify")
        && args[3].trim().eq_ignore_ascii_case("hermes")
        && args[4].trim().eq_ignore_ascii_case("bridge")
        && args[5].trim().eq_ignore_ascii_case("service-run")
}

pub fn bridge_health_available_with(
    health_url: &str,
    mut get_status: impl FnMut(&str) -> anyhow::Result<u16>,
) -> bool {
    let health_url = health_url.trim();
    if health_url.is_empty() {
        return false;
    }
    get_status(health_url).is_ok_and(|status| (200..300).contains(&status))
}

pub fn bridge_status_ready(status: &BridgeStatus, want_running: bool) -> bool {
    if want_running {
        return status.running && status.bridge_available;
    }
    !status.running
}

pub fn wait_for_bridge_status_with(
    mut status_fn: impl FnMut() -> anyhow::Result<BridgeStatus>,
    want_running: bool,
    timeout: Duration,
    interval: Duration,
) -> anyhow::Result<BridgeStatus> {
    let deadline = Instant::now() + timeout;
    let mut last_status = BridgeStatus::default();
    loop {
        if let Ok(status) = status_fn() {
            last_status = status;
            if bridge_status_ready(&last_status, want_running) {
                return Ok(last_status);
            }
        }
        if Instant::now() > deadline {
            return Ok(last_status);
        }
        std::thread::sleep(interval);
    }
}

pub fn apply_decision_for(status: &BridgeStatus) -> BridgeApplyDecision {
    if !status.installed {
        return BridgeApplyDecision::EnsureInstalledThenStart;
    }
    if status.running {
        return BridgeApplyDecision::Restart;
    }
    BridgeApplyDecision::Start
}
