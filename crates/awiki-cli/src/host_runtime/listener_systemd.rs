use crate::workspace_config::Resolved;
use serde_json::{json, Value};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::listener::{self, Status};
use super::listener_service::{
    self, ListenerServiceConfigValue, SERVICE_DESCRIPTION, SERVICE_PID_FILE_NAME,
};

pub const ENABLE_SYSTEMD_SERVICE_ENV: &str = "AWIKI_CLI_ENABLE_SYSTEMD_LISTENER_SERVICE";

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SystemdUnit {
    pub name: String,
    pub path: PathBuf,
    pub content: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SystemdStatus {
    pub installed: bool,
    pub running: bool,
    pub load_state: String,
    pub active_state: String,
}

pub fn is_supported() -> bool {
    cfg!(target_os = "linux") && enabled_by_env() && systemd_user_env_available()
}

pub fn service_platform() -> &'static str {
    if cfg!(target_os = "linux") {
        "linux-systemd"
    } else if cfg!(target_os = "macos") {
        "launchd"
    } else if cfg!(windows) {
        "windows-service"
    } else {
        "unsupported"
    }
}

pub fn unit_name_for(resolved: &Resolved) -> String {
    format!("{}.service", listener_service::service_name_for(resolved))
}

pub fn unit_path_for(resolved: &Resolved) -> anyhow::Result<PathBuf> {
    let home = env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("HOME is required for listener service management"))?;
    Ok(home
        .join(".config")
        .join("systemd")
        .join("user")
        .join(unit_name_for(resolved)))
}

pub fn unit_for(resolved: &Resolved) -> anyhow::Result<SystemdUnit> {
    let plan = listener_service::service_config_plan_for(resolved, false);
    let executable = env::current_exe()
        .map_err(|err| anyhow::anyhow!("resolve current executable for listener service: {err}"))?;
    let executable = executable.to_string_lossy();
    let args = shell_join(plan.arguments.iter().map(String::as_str));
    let pid_file = plan
        .options
        .get("PIDFile")
        .and_then(config_string)
        .unwrap_or_else(|| {
            Path::new(&resolved.paths.state_dir)
                .join(SERVICE_PID_FILE_NAME)
                .to_string_lossy()
                .into_owned()
        });
    let log_dir = plan
        .options
        .get("LogDirectory")
        .and_then(config_string)
        .unwrap_or_else(|| resolved.paths.logs_dir.clone());
    let stdout_log = Path::new(&log_dir).join("listener.service.log");
    let stderr_log = Path::new(&log_dir).join("listener.service.err.log");
    let mut env_lines = Vec::new();
    for (key, value) in &plan.env_vars {
        env_lines.push(format!("Environment={key}={}", systemd_escape(value)));
    }
    let content = format!(
        "[Unit]\nDescription={description}\n\n[Service]\nType=simple\nWorkingDirectory={working_dir}\nExecStart={exec} {args}\nRestart=on-failure\nRestartSec=1s\nPIDFile={pid_file}\nStandardOutput=append:{stdout_log}\nStandardError=append:{stderr_log}\n{env_lines}\n\n[Install]\nWantedBy=default.target\n",
        description = SERVICE_DESCRIPTION,
        working_dir = systemd_escape(&plan.working_directory),
        exec = systemd_escape(&executable),
        args = args,
        pid_file = systemd_escape(&pid_file),
        stdout_log = systemd_escape(&stdout_log.to_string_lossy()),
        stderr_log = systemd_escape(&stderr_log.to_string_lossy()),
        env_lines = env_lines.join("\n"),
    );
    Ok(SystemdUnit {
        name: unit_name_for(resolved),
        path: unit_path_for(resolved)?,
        content,
    })
}

pub fn status(resolved: &Resolved) -> anyhow::Result<SystemdStatus> {
    if !is_supported() {
        return Ok(SystemdStatus {
            installed: false,
            running: false,
            load_state: String::new(),
            active_state: String::new(),
        });
    }
    status_with_runner(resolved, run_systemctl)
}

pub fn install(resolved: &Resolved) -> anyhow::Result<Status> {
    require_supported()?;
    install_with_runner(resolved, run_systemctl)?;
    listener_status(resolved)
}

pub fn start(resolved: &Resolved) -> anyhow::Result<Status> {
    require_supported()?;
    if super::resolve(resolved).mode != "websocket" {
        anyhow::bail!("runtime mode must be websocket before starting the listener");
    }
    if !status(resolved)?.installed {
        install_with_runner(resolved, run_systemctl)?;
    }
    let expected_boot_id = listener_service::prepare_expected_boot_id(resolved)?;
    run_systemctl(&["start", &unit_name_for(resolved)])?;
    wait_for_listener_status(resolved, true, &expected_boot_id)
}

pub fn stop(resolved: &Resolved) -> anyhow::Result<Status> {
    require_supported()?;
    let service_status = status(resolved)?;
    if service_status.installed && service_status.running {
        run_systemctl(&["stop", &unit_name_for(resolved)])?;
    }
    listener_service::cleanup_runtime_artifacts(resolved);
    wait_for_listener_status(resolved, false, "")
}

pub fn restart(resolved: &Resolved) -> anyhow::Result<Status> {
    require_supported()?;
    if !status(resolved)?.installed {
        anyhow::bail!("listener service is not installed");
    }
    let expected_boot_id = listener_service::prepare_expected_boot_id(resolved)?;
    run_systemctl(&["restart", &unit_name_for(resolved)])?;
    wait_for_listener_status(resolved, true, &expected_boot_id)
}

pub fn uninstall(resolved: &Resolved) -> anyhow::Result<Status> {
    require_supported()?;
    let service_status = status(resolved)?;
    if service_status.installed && service_status.running {
        run_systemctl(&["stop", &unit_name_for(resolved)])?;
    }
    let unit_name = unit_name_for(resolved);
    if service_status.installed {
        run_systemctl(&["disable", &unit_name])?;
    }
    let unit = unit_for(resolved)?;
    if unit.path.exists() {
        fs::remove_file(&unit.path)
            .map_err(|err| anyhow::anyhow!("remove listener systemd unit: {err}"))?;
    }
    run_systemctl(&["daemon-reload"])?;
    listener_service::cleanup_runtime_artifacts(resolved);
    listener_status(resolved)
}

pub fn status_value(resolved: &Resolved) -> anyhow::Result<Value> {
    Ok(listener::to_value(listener_status(resolved)?))
}

pub fn unit_value(resolved: &Resolved) -> anyhow::Result<Value> {
    let unit = unit_for(resolved)?;
    Ok(json!({
        "unit_name": unit.name,
        "unit_path": unit.path,
        "content": unit.content,
    }))
}

pub fn status_with_runner(
    resolved: &Resolved,
    mut runner: impl FnMut(&[&str]) -> anyhow::Result<String>,
) -> anyhow::Result<SystemdStatus> {
    let output = runner(&[
        "show",
        &unit_name_for(resolved),
        "--property=LoadState",
        "--property=ActiveState",
        "--value",
    ])?;
    Ok(parse_systemd_status(&output))
}

pub fn parse_systemd_status(output: &str) -> SystemdStatus {
    let mut lines = output.lines();
    let load_state = lines.next().unwrap_or_default().trim().to_string();
    let active_state = lines.next().unwrap_or_default().trim().to_string();
    let installed = matches!(
        load_state.as_str(),
        "loaded" | "masked" | "static" | "generated" | "transient"
    );
    let running = active_state == "active" || active_state == "activating";
    SystemdStatus {
        installed,
        running,
        load_state,
        active_state,
    }
}

fn install_with_runner(
    resolved: &Resolved,
    mut runner: impl FnMut(&[&str]) -> anyhow::Result<String>,
) -> anyhow::Result<()> {
    let unit = unit_for(resolved)?;
    if let Some(parent) = unit.path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| anyhow::anyhow!("create listener systemd unit dir: {err}"))?;
    }
    fs::create_dir_all(&resolved.paths.logs_dir)
        .map_err(|err| anyhow::anyhow!("create listener service log dir: {err}"))?;
    fs::write(&unit.path, unit.content.as_bytes())
        .map_err(|err| anyhow::anyhow!("write listener systemd unit: {err}"))?;
    runner(&["daemon-reload"])?;
    runner(&["enable", &unit.name])?;
    Ok(())
}

fn listener_status(resolved: &Resolved) -> anyhow::Result<Status> {
    let service_status = status(resolved)?;
    let mut listener_status = listener::status_for(
        resolved,
        service_status.installed,
        service_status.running,
        service_platform(),
    )?;
    if !service_status.load_state.is_empty() && service_status.load_state != "loaded" {
        listener_status.warnings.push(format!(
            "listener service load state is {}",
            service_status.load_state
        ));
    }
    Ok(listener_status)
}

fn wait_for_listener_status(
    resolved: &Resolved,
    want_running: bool,
    expected_boot_id: &str,
) -> anyhow::Result<Status> {
    let runtime_resolved = super::resolve(resolved);
    let wait_for_bridge =
        want_running && runtime_resolved.mode == "websocket" && runtime_resolved.listener.enabled;
    listener_service::wait_for_service_status_with(
        || listener_status(resolved),
        want_running,
        wait_for_bridge,
        expected_boot_id,
        std::time::Duration::from_secs(15),
        std::time::Duration::from_millis(250),
    )
}

fn require_supported() -> anyhow::Result<()> {
    if is_supported() {
        return Ok(());
    }
    anyhow::bail!(
        "listener service manager is unavailable; systemd user services require Linux with HOME, XDG_RUNTIME_DIR, and DBUS_SESSION_BUS_ADDRESS"
    )
}

fn systemd_user_env_available() -> bool {
    env::var_os("HOME").is_some_and(|value| !value.is_empty())
        && env::var_os("XDG_RUNTIME_DIR").is_some_and(|value| !value.is_empty())
        && env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some_and(|value| !value.is_empty())
}

fn enabled_by_env() -> bool {
    env::var(ENABLE_SYSTEMD_SERVICE_ENV)
        .ok()
        .map(|value| {
            let value = value.trim();
            value == "1" || value.eq_ignore_ascii_case("true")
        })
        .unwrap_or(false)
}

fn run_systemctl(args: &[&str]) -> anyhow::Result<String> {
    let output = Command::new("systemctl")
        .arg("--user")
        .args(args)
        .output()
        .map_err(|err| anyhow::anyhow!("run systemctl --user {}: {err}", args.join(" ")))?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).to_string());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let message = if stderr.is_empty() { stdout } else { stderr };
    Err(anyhow::anyhow!(
        "systemctl --user {} failed: {}",
        args.join(" "),
        if message.is_empty() {
            output.status.to_string()
        } else {
            message
        }
    ))
}

fn config_string(value: &ListenerServiceConfigValue) -> Option<String> {
    match value {
        ListenerServiceConfigValue::String(value) => Some(value.clone()),
        ListenerServiceConfigValue::Bool(_) => None,
    }
}

fn shell_join<'a>(values: impl IntoIterator<Item = &'a str>) -> String {
    values
        .into_iter()
        .map(systemd_escape)
        .collect::<Vec<_>>()
        .join(" ")
}

fn systemd_escape(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '-' | '_' | ':' | '='))
    {
        return value.to_string();
    }
    let mut output = String::from("'");
    for ch in value.chars() {
        if ch == '\'' {
            output.push_str("'\\''");
        } else {
            output.push(ch);
        }
    }
    output.push('\'');
    output
}
