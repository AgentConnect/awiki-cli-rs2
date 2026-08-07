use super::{BridgeConfig, BridgeStatus};
use crate::workspace_config::{product_home_dir, Resolved};
use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
#[cfg(unix)]
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub const ENABLE_SYSTEMD_SERVICE_ENV: &str = "AWIKI_CLI_ENABLE_SYSTEMD_HERMES_BRIDGE_SERVICE";
pub const INTERNAL_ENTRY_ENV: &str = "AWIKI_CLI_INTERNAL_ENTRY";
pub const SERVICE_NAME_PREFIX: &str = "awiki-cli-hermes-bridge";
pub const SERVICE_DISPLAY_NAME_PREFIX: &str = "awiki-cli Hermes Bridge";
pub const SERVICE_DESCRIPTION: &str = "awiki-cli Hermes notify bridge";
pub const SERVICE_ARGUMENTS: &[&str] =
    &["runtime", "host-notify", "hermes", "bridge", "service-run"];
pub const BRIDGE_ADAPTER_STOP_TIMEOUT: Duration = Duration::from_secs(15);
pub const BRIDGE_SERVICE_STATUS_WAIT_TIMEOUT: Duration = Duration::from_secs(15);
pub const BRIDGE_SERVICE_STATUS_POLL_INTERVAL: Duration = Duration::from_millis(250);
const BRIDGE_ADAPTER_STOP_POLL_INTERVAL: Duration = Duration::from_millis(25);
const BRIDGE_SERVICE_RUN_POLL_INTERVAL: Duration = Duration::from_millis(250);

#[cfg(unix)]
static BRIDGE_SERVICE_SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BridgeAdapterExit {
    pub success: bool,
    pub code: Option<i32>,
}

pub struct BridgeAdapterProcess {
    child: Option<Child>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct BridgeSystemdUnit {
    pub name: String,
    pub path: PathBuf,
    pub content: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct BridgeSystemdStatus {
    pub installed: bool,
    pub running: bool,
    pub load_state: String,
    pub active_state: String,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeServiceLifecycleOperation {
    EnsureInstalled,
    InstallIfMissing,
    Start,
    Stop,
    Restart,
    Uninstall,
    ReturnStatus,
    WaitForRunning,
    WaitForStopped,
    ErrorNotInstalled,
}

pub trait BridgeServiceBackend {
    fn status_snapshot(
        &mut self,
        resolved: &Resolved,
    ) -> anyhow::Result<Option<BridgeServiceStatusSnapshot>>;
    fn install(&mut self, resolved: &Resolved) -> anyhow::Result<()>;
    fn start(&mut self, resolved: &Resolved) -> anyhow::Result<()>;
    fn stop(&mut self, resolved: &Resolved) -> anyhow::Result<()>;
    fn restart(&mut self, resolved: &Resolved) -> anyhow::Result<()>;
    fn uninstall(&mut self, resolved: &Resolved) -> anyhow::Result<()>;
}

pub trait BridgeSystemdCommandRunner {
    fn run(&mut self, args: &[&str]) -> anyhow::Result<String>;
}

#[derive(Debug, Default)]
pub struct SystemctlCommandRunner;

#[derive(Debug)]
pub struct SystemdBridgeServiceBackend<R> {
    runner: R,
}

impl Default for SystemdBridgeServiceBackend<SystemctlCommandRunner> {
    fn default() -> Self {
        Self {
            runner: SystemctlCommandRunner,
        }
    }
}

impl<R> SystemdBridgeServiceBackend<R> {
    pub fn new(runner: R) -> Self {
        Self { runner }
    }
}

impl BridgeSystemdCommandRunner for SystemctlCommandRunner {
    fn run(&mut self, args: &[&str]) -> anyhow::Result<String> {
        run_systemctl(args)
    }
}

impl<R> BridgeServiceBackend for SystemdBridgeServiceBackend<R>
where
    R: BridgeSystemdCommandRunner,
{
    fn status_snapshot(
        &mut self,
        resolved: &Resolved,
    ) -> anyhow::Result<Option<BridgeServiceStatusSnapshot>> {
        systemd_status_snapshot_with_runner(resolved, &mut self.runner).map(Some)
    }

    fn install(&mut self, resolved: &Resolved) -> anyhow::Result<()> {
        install_systemd_unit_with_runner(resolved, &mut self.runner)
    }

    fn start(&mut self, resolved: &Resolved) -> anyhow::Result<()> {
        self.runner.run(&["start", &unit_name_for(resolved)])?;
        Ok(())
    }

    fn stop(&mut self, resolved: &Resolved) -> anyhow::Result<()> {
        self.runner.run(&["stop", &unit_name_for(resolved)])?;
        Ok(())
    }

    fn restart(&mut self, resolved: &Resolved) -> anyhow::Result<()> {
        self.runner.run(&["restart", &unit_name_for(resolved)])?;
        Ok(())
    }

    fn uninstall(&mut self, resolved: &Resolved) -> anyhow::Result<()> {
        let unit_name = unit_name_for(resolved);
        self.runner.run(&["disable", &unit_name])?;
        let unit = systemd_unit_for(resolved)?;
        if unit.path.exists() {
            fs::remove_file(&unit.path)
                .map_err(|err| anyhow::anyhow!("remove Hermes bridge systemd unit: {err}"))?;
        }
        self.runner.run(&["daemon-reload"])?;
        Ok(())
    }
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

pub fn status_for_with_backend(
    resolved: &Resolved,
    backend: &mut (impl BridgeServiceBackend + ?Sized),
    health_available: &mut (impl FnMut(&str) -> bool + ?Sized),
) -> BridgeStatus {
    status_from_parts(
        service_name_for(Some(resolved)),
        super::resolve_bridge_config(resolved),
        backend.status_snapshot(resolved),
        health_available,
    )
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
        env_workspace_home_dir: product_home_dir(resolved).to_string(),
        env_hermes_home: hermes_home.trim().to_string(),
        user_service: true,
        keep_alive: true,
        on_failure: "restart".to_string(),
        on_failure_delay_duration: "1s".to_string(),
        log_output: true,
        log_directory: resolved.paths.logs_dir.clone(),
    }
}

pub fn systemd_service_supported() -> bool {
    cfg!(target_os = "linux") && enabled_by_env() && systemd_user_env_available()
}

pub fn service_enabled_by_env_value(value: Option<&str>) -> bool {
    value
        .map(|value| {
            let value = value.trim();
            value == "1" || value.eq_ignore_ascii_case("true")
        })
        .unwrap_or(false)
}

pub fn unit_name_for(resolved: &Resolved) -> String {
    format!("{}.service", service_name_for(Some(resolved)))
}

pub fn unit_path_for(resolved: &Resolved) -> anyhow::Result<PathBuf> {
    let home = env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("HOME is required for Hermes bridge service management"))?;
    Ok(home
        .join(".config")
        .join("systemd")
        .join("user")
        .join(unit_name_for(resolved)))
}

pub fn systemd_unit_for(resolved: &Resolved) -> anyhow::Result<BridgeSystemdUnit> {
    let bridge_config = super::resolve_bridge_config(resolved)?;
    let plan = service_config_plan_for(resolved, &bridge_config.hermes_home, cfg!(windows));
    let executable = env::current_exe().map_err(|err| {
        anyhow::anyhow!("resolve current executable for Hermes bridge service: {err}")
    })?;
    let executable = executable.to_string_lossy();
    let args = shell_join(plan.arguments.iter().map(String::as_str));
    let stdout_log = Path::new(&plan.log_directory).join("hermes-bridge.service.log");
    let stderr_log = Path::new(&plan.log_directory).join("hermes-bridge.service.err.log");
    let env_lines = [
        ("AWIKI_CLI_WORKSPACE_HOME_DIR", plan.env_workspace_home_dir),
        (INTERNAL_ENTRY_ENV, "1".to_string()),
        ("HERMES_HOME", plan.env_hermes_home),
    ]
    .into_iter()
    .map(|(key, value)| format!("Environment={key}={}", systemd_escape(&value)))
    .collect::<Vec<_>>()
    .join("\n");
    let content = format!(
        "[Unit]\nDescription={description}\n\n[Service]\nType=simple\nWorkingDirectory={working_dir}\nExecStart={exec} {args}\nRestart=on-failure\nRestartSec=1s\nStandardOutput=append:{stdout_log}\nStandardError=append:{stderr_log}\n{env_lines}\n\n[Install]\nWantedBy=default.target\n",
        description = SERVICE_DESCRIPTION,
        working_dir = systemd_escape(&plan.working_directory),
        exec = systemd_escape(&executable),
        args = args,
        stdout_log = systemd_escape(&stdout_log.to_string_lossy()),
        stderr_log = systemd_escape(&stderr_log.to_string_lossy()),
        env_lines = env_lines,
    );
    Ok(BridgeSystemdUnit {
        name: unit_name_for(resolved),
        path: unit_path_for(resolved)?,
        content,
    })
}

pub fn service_status_snapshot_for(
    resolved: &Resolved,
) -> anyhow::Result<Option<BridgeServiceStatusSnapshot>> {
    if !systemd_service_supported() {
        return Ok(None);
    }
    let mut backend = SystemdBridgeServiceBackend::default();
    backend.status_snapshot(resolved)
}

pub fn systemd_status_snapshot_with_runner(
    resolved: &Resolved,
    runner: &mut impl BridgeSystemdCommandRunner,
) -> anyhow::Result<BridgeServiceStatusSnapshot> {
    let status = systemd_status_with_runner(resolved, runner)?;
    Ok(BridgeServiceStatusSnapshot {
        installed: status.installed,
        running: status.running,
        platform: "linux-systemd".to_string(),
        service_name: service_name_for(Some(resolved)),
    })
}

pub fn systemd_status_with_runner(
    resolved: &Resolved,
    runner: &mut impl BridgeSystemdCommandRunner,
) -> anyhow::Result<BridgeSystemdStatus> {
    let output = runner.run(&[
        "show",
        &unit_name_for(resolved),
        "--property=LoadState",
        "--property=ActiveState",
        "--value",
    ])?;
    Ok(parse_systemd_status(&output))
}

pub fn parse_systemd_status(output: &str) -> BridgeSystemdStatus {
    let mut lines = output.lines();
    let load_state = lines.next().unwrap_or_default().trim().to_string();
    let active_state = lines.next().unwrap_or_default().trim().to_string();
    let installed = matches!(
        load_state.as_str(),
        "loaded" | "masked" | "static" | "generated" | "transient"
    );
    let running = active_state == "active" || active_state == "activating";
    BridgeSystemdStatus {
        installed,
        running,
        load_state,
        active_state,
    }
}

pub fn ensure_installed(resolved: &Resolved) -> anyhow::Result<BridgeStatus> {
    require_supported()?;
    super::resolve_bridge_config(resolved)?;
    let mut backend = SystemdBridgeServiceBackend::default();
    let mut health = bridge_health_available;
    ensure_installed_with_backend(resolved, &mut backend, &mut health)
}

pub fn start_service(resolved: &Resolved) -> anyhow::Result<BridgeStatus> {
    require_supported()?;
    super::resolve_bridge_config(resolved)?;
    let mut backend = SystemdBridgeServiceBackend::default();
    let mut health = bridge_health_available;
    start_service_with_backend(
        resolved,
        &mut backend,
        &mut health,
        BRIDGE_SERVICE_STATUS_WAIT_TIMEOUT,
        BRIDGE_SERVICE_STATUS_POLL_INTERVAL,
    )
}

pub fn stop_service(resolved: &Resolved) -> anyhow::Result<BridgeStatus> {
    require_supported()?;
    super::resolve_bridge_config(resolved)?;
    let mut backend = SystemdBridgeServiceBackend::default();
    let mut health = bridge_health_available;
    stop_service_with_backend(
        resolved,
        &mut backend,
        &mut health,
        BRIDGE_SERVICE_STATUS_WAIT_TIMEOUT,
        BRIDGE_SERVICE_STATUS_POLL_INTERVAL,
    )
}

pub fn restart_service(resolved: &Resolved) -> anyhow::Result<BridgeStatus> {
    require_supported()?;
    super::resolve_bridge_config(resolved)?;
    let mut backend = SystemdBridgeServiceBackend::default();
    let mut health = bridge_health_available;
    restart_service_with_backend(
        resolved,
        &mut backend,
        &mut health,
        BRIDGE_SERVICE_STATUS_WAIT_TIMEOUT,
        BRIDGE_SERVICE_STATUS_POLL_INTERVAL,
    )
}

pub fn uninstall_service(resolved: &Resolved) -> anyhow::Result<BridgeStatus> {
    require_supported()?;
    super::resolve_bridge_config(resolved)?;
    let mut backend = SystemdBridgeServiceBackend::default();
    let mut health = bridge_health_available;
    uninstall_service_with_backend(resolved, &mut backend, &mut health)
}

pub fn apply_service(resolved: &Resolved) -> anyhow::Result<BridgeStatus> {
    require_supported()?;
    super::resolve_bridge_config(resolved)?;
    let mut backend = SystemdBridgeServiceBackend::default();
    let mut health = bridge_health_available;
    apply_service_with_backend(
        resolved,
        &mut backend,
        &mut health,
        BRIDGE_SERVICE_STATUS_WAIT_TIMEOUT,
        BRIDGE_SERVICE_STATUS_POLL_INTERVAL,
    )
}

pub fn ensure_installed_with_backend(
    resolved: &Resolved,
    backend: &mut (impl BridgeServiceBackend + ?Sized),
    health_available: &mut (impl FnMut(&str) -> bool + ?Sized),
) -> anyhow::Result<BridgeStatus> {
    let snapshot = backend.status_snapshot(resolved)?;
    if snapshot
        .as_ref()
        .is_some_and(|snapshot| !snapshot.installed)
    {
        if let Err(err) = backend.install(resolved) {
            let message = err.to_string();
            if !message.to_ascii_lowercase().contains("exists") {
                return Err(err);
            }
        }
    }
    Ok(status_for_with_backend(resolved, backend, health_available))
}

pub fn start_service_with_backend(
    resolved: &Resolved,
    backend: &mut (impl BridgeServiceBackend + ?Sized),
    health_available: &mut (impl FnMut(&str) -> bool + ?Sized),
    timeout: Duration,
    interval: Duration,
) -> anyhow::Result<BridgeStatus> {
    let snapshot = backend.status_snapshot(resolved)?;
    let Some(snapshot) = snapshot else {
        return Ok(status_for_with_backend(resolved, backend, health_available));
    };
    if !snapshot.installed {
        ensure_installed_with_backend(resolved, backend, health_available)?;
    }
    if snapshot.running {
        return Ok(status_for_with_backend(resolved, backend, health_available));
    }
    backend.start(resolved)?;
    wait_for_bridge_status_with(
        || Ok(status_for_with_backend(resolved, backend, health_available)),
        true,
        timeout,
        interval,
    )
}

pub fn stop_service_with_backend(
    resolved: &Resolved,
    backend: &mut (impl BridgeServiceBackend + ?Sized),
    health_available: &mut (impl FnMut(&str) -> bool + ?Sized),
    timeout: Duration,
    interval: Duration,
) -> anyhow::Result<BridgeStatus> {
    let snapshot = backend.status_snapshot(resolved)?;
    let Some(snapshot) = snapshot else {
        return Ok(status_for_with_backend(resolved, backend, health_available));
    };
    if !snapshot.installed {
        return Ok(status_for_with_backend(resolved, backend, health_available));
    }
    if snapshot.running {
        backend.stop(resolved)?;
    }
    wait_for_bridge_status_with(
        || Ok(status_for_with_backend(resolved, backend, health_available)),
        false,
        timeout,
        interval,
    )
}

pub fn restart_service_with_backend(
    resolved: &Resolved,
    backend: &mut (impl BridgeServiceBackend + ?Sized),
    health_available: &mut (impl FnMut(&str) -> bool + ?Sized),
    timeout: Duration,
    interval: Duration,
) -> anyhow::Result<BridgeStatus> {
    let snapshot = backend.status_snapshot(resolved)?;
    let Some(snapshot) = snapshot else {
        return Ok(status_for_with_backend(resolved, backend, health_available));
    };
    if !snapshot.installed {
        anyhow::bail!("Hermes bridge service is not installed");
    }
    backend.restart(resolved)?;
    wait_for_bridge_status_with(
        || Ok(status_for_with_backend(resolved, backend, health_available)),
        true,
        timeout,
        interval,
    )
}

pub fn uninstall_service_with_backend(
    resolved: &Resolved,
    backend: &mut (impl BridgeServiceBackend + ?Sized),
    health_available: &mut (impl FnMut(&str) -> bool + ?Sized),
) -> anyhow::Result<BridgeStatus> {
    let snapshot = backend.status_snapshot(resolved)?;
    let Some(snapshot) = snapshot else {
        return Ok(status_for_with_backend(resolved, backend, health_available));
    };
    if !snapshot.installed {
        return Ok(status_for_with_backend(resolved, backend, health_available));
    }
    if snapshot.running {
        backend.stop(resolved)?;
    }
    if let Err(err) = backend.uninstall(resolved) {
        let message = err.to_string();
        if !message.to_ascii_lowercase().contains("not installed") {
            return Err(err);
        }
    }
    Ok(status_for_with_backend(resolved, backend, health_available))
}

pub fn apply_service_with_backend(
    resolved: &Resolved,
    backend: &mut (impl BridgeServiceBackend + ?Sized),
    health_available: &mut (impl FnMut(&str) -> bool + ?Sized),
    timeout: Duration,
    interval: Duration,
) -> anyhow::Result<BridgeStatus> {
    let status = status_for_with_backend(resolved, backend, health_available);
    match apply_decision_for(&status) {
        BridgeApplyDecision::EnsureInstalledThenStart => {
            ensure_installed_with_backend(resolved, backend, health_available)?;
            start_service_with_backend(resolved, backend, health_available, timeout, interval)
        }
        BridgeApplyDecision::Restart => {
            restart_service_with_backend(resolved, backend, health_available, timeout, interval)
        }
        BridgeApplyDecision::Start => {
            start_service_with_backend(resolved, backend, health_available, timeout, interval)
        }
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

impl BridgeAdapterProcess {
    pub fn new() -> Self {
        Self { child: None }
    }

    pub fn start(plan: &BridgeAdapterCommandPlan) -> anyhow::Result<Self> {
        let mut process = Self::new();
        process.start_with_plan(plan)?;
        Ok(process)
    }

    pub fn start_with_plan(&mut self, plan: &BridgeAdapterCommandPlan) -> anyhow::Result<()> {
        if self.child.is_some() {
            return Ok(());
        }
        let child = command_for_adapter_plan(plan)
            .spawn()
            .map_err(|err| anyhow::anyhow!("start Hermes notify adapter: {err}"))?;
        self.child = Some(child);
        Ok(())
    }

    pub fn is_running(&mut self) -> anyhow::Result<bool> {
        let Some(child) = self.child.as_mut() else {
            return Ok(false);
        };
        Ok(child.try_wait()?.is_none())
    }

    pub fn try_wait(&mut self) -> anyhow::Result<Option<BridgeAdapterExit>> {
        let status = match self.child.as_mut() {
            Some(child) => child.try_wait()?,
            None => return Ok(None),
        };
        let Some(status) = status else {
            return Ok(None);
        };
        self.child = None;
        Ok(Some(status.into()))
    }

    pub fn wait(&mut self) -> anyhow::Result<Option<BridgeAdapterExit>> {
        let Some(mut child) = self.child.take() else {
            return Ok(None);
        };
        let status = child.wait()?;
        Ok(Some(status.into()))
    }

    pub fn stop(&mut self) -> anyhow::Result<()> {
        self.stop_with_timeout(
            BRIDGE_ADAPTER_STOP_TIMEOUT,
            BRIDGE_ADAPTER_STOP_POLL_INTERVAL,
        )
    }

    pub fn stop_with_timeout(
        &mut self,
        timeout: Duration,
        interval: Duration,
    ) -> anyhow::Result<()> {
        let Some(mut child) = self.child.take() else {
            return Ok(());
        };
        if child.try_wait()?.is_none() {
            let _ = child.kill();
        }
        wait_for_adapter_child_exit(&mut child, timeout, interval).map(|_| ())
    }
}

impl Default for BridgeAdapterProcess {
    fn default() -> Self {
        Self::new()
    }
}

impl From<std::process::ExitStatus> for BridgeAdapterExit {
    fn from(status: std::process::ExitStatus) -> Self {
        Self {
            success: status.success(),
            code: status.code(),
        }
    }
}

pub fn run_bridge_service(plan: &BridgeAdapterCommandPlan) -> anyhow::Result<()> {
    install_bridge_service_shutdown_handler();
    run_bridge_service_with_stop(
        plan,
        bridge_service_shutdown_requested,
        BRIDGE_SERVICE_RUN_POLL_INTERVAL,
    )
}

pub fn run_bridge_service_with_stop(
    plan: &BridgeAdapterCommandPlan,
    mut stop_requested: impl FnMut() -> bool,
    interval: Duration,
) -> anyhow::Result<()> {
    let mut process = BridgeAdapterProcess::start(plan)?;
    while !stop_requested() {
        std::thread::sleep(interval);
    }
    process.stop()
}

fn command_for_adapter_plan(plan: &BridgeAdapterCommandPlan) -> Command {
    let mut command = Command::new(&plan.executable);
    command.args(&plan.arguments);
    if let Some(hermes_home) = plan.env_hermes_home.as_ref() {
        command.env("HERMES_HOME", hermes_home);
    }
    if plan.stdout_inherits_parent {
        command.stdout(Stdio::inherit());
    } else {
        command.stdout(Stdio::null());
    }
    if plan.stderr_inherits_parent {
        command.stderr(Stdio::inherit());
    } else {
        command.stderr(Stdio::null());
    }
    command
}

fn wait_for_adapter_child_exit(
    child: &mut Child,
    timeout: Duration,
    interval: Duration,
) -> anyhow::Result<BridgeAdapterExit> {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status.into());
        }
        if started.elapsed() >= timeout {
            anyhow::bail!("Hermes bridge stop timed out");
        }
        std::thread::sleep(interval);
    }
}

#[cfg(unix)]
fn install_bridge_service_shutdown_handler() {
    BRIDGE_SERVICE_SHUTDOWN_REQUESTED.store(false, Ordering::SeqCst);
    unsafe {
        signal(SIGTERM, bridge_service_signal_handler);
        signal(SIGINT, bridge_service_signal_handler);
    }
}

#[cfg(not(unix))]
fn install_bridge_service_shutdown_handler() {}

#[cfg(unix)]
fn bridge_service_shutdown_requested() -> bool {
    BRIDGE_SERVICE_SHUTDOWN_REQUESTED.load(Ordering::SeqCst)
}

#[cfg(not(unix))]
fn bridge_service_shutdown_requested() -> bool {
    false
}

#[cfg(unix)]
extern "C" fn bridge_service_signal_handler(_signal: std::os::raw::c_int) {
    BRIDGE_SERVICE_SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
}

#[cfg(unix)]
const SIGINT: std::os::raw::c_int = 2;
#[cfg(unix)]
const SIGTERM: std::os::raw::c_int = 15;

#[cfg(unix)]
extern "C" {
    fn signal(
        signum: std::os::raw::c_int,
        handler: extern "C" fn(std::os::raw::c_int),
    ) -> extern "C" fn(std::os::raw::c_int);
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

pub fn bridge_health_available(health_url: &str) -> bool {
    bridge_health_available_with(health_url, http_status_for)
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

pub fn ensure_installed_plan(installed: bool) -> Vec<BridgeServiceLifecycleOperation> {
    let mut operations = Vec::new();
    if !installed {
        operations.push(BridgeServiceLifecycleOperation::InstallIfMissing);
    }
    operations.push(BridgeServiceLifecycleOperation::ReturnStatus);
    operations
}

pub fn start_service_plan(installed: bool, running: bool) -> Vec<BridgeServiceLifecycleOperation> {
    let mut operations = Vec::new();
    if !installed {
        operations.push(BridgeServiceLifecycleOperation::EnsureInstalled);
    }
    if running {
        operations.push(BridgeServiceLifecycleOperation::ReturnStatus);
    } else {
        operations.push(BridgeServiceLifecycleOperation::Start);
        operations.push(BridgeServiceLifecycleOperation::WaitForRunning);
    }
    operations
}

pub fn stop_service_plan(installed: bool, running: bool) -> Vec<BridgeServiceLifecycleOperation> {
    let mut operations = Vec::new();
    if !installed {
        operations.push(BridgeServiceLifecycleOperation::ReturnStatus);
        return operations;
    }
    if running {
        operations.push(BridgeServiceLifecycleOperation::Stop);
    }
    operations.push(BridgeServiceLifecycleOperation::WaitForStopped);
    operations
}

pub fn restart_service_plan(installed: bool) -> Vec<BridgeServiceLifecycleOperation> {
    if !installed {
        return vec![BridgeServiceLifecycleOperation::ErrorNotInstalled];
    }
    vec![
        BridgeServiceLifecycleOperation::Restart,
        BridgeServiceLifecycleOperation::WaitForRunning,
    ]
}

pub fn uninstall_service_plan(
    installed: bool,
    running: bool,
) -> Vec<BridgeServiceLifecycleOperation> {
    let mut operations = Vec::new();
    if !installed {
        operations.push(BridgeServiceLifecycleOperation::ReturnStatus);
        return operations;
    }
    if running {
        operations.push(BridgeServiceLifecycleOperation::Stop);
    }
    operations.push(BridgeServiceLifecycleOperation::Uninstall);
    operations.push(BridgeServiceLifecycleOperation::ReturnStatus);
    operations
}

pub fn apply_service_plan(status: &BridgeStatus) -> Vec<BridgeServiceLifecycleOperation> {
    match apply_decision_for(status) {
        BridgeApplyDecision::EnsureInstalledThenStart => vec![
            BridgeServiceLifecycleOperation::EnsureInstalled,
            BridgeServiceLifecycleOperation::Start,
        ],
        BridgeApplyDecision::Restart => vec![BridgeServiceLifecycleOperation::Restart],
        BridgeApplyDecision::Start => vec![BridgeServiceLifecycleOperation::Start],
    }
}

fn install_systemd_unit_with_runner(
    resolved: &Resolved,
    runner: &mut impl BridgeSystemdCommandRunner,
) -> anyhow::Result<()> {
    let unit = systemd_unit_for(resolved)?;
    if let Some(parent) = unit.path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| anyhow::anyhow!("create Hermes bridge systemd unit dir: {err}"))?;
    }
    fs::create_dir_all(&resolved.paths.logs_dir)
        .map_err(|err| anyhow::anyhow!("create Hermes bridge service log dir: {err}"))?;
    fs::write(&unit.path, unit.content.as_bytes())
        .map_err(|err| anyhow::anyhow!("write Hermes bridge systemd unit: {err}"))?;
    runner.run(&["daemon-reload"])?;
    runner.run(&["enable", &unit.name])?;
    Ok(())
}

fn require_supported() -> anyhow::Result<()> {
    if systemd_service_supported() {
        return Ok(());
    }
    anyhow::bail!(
        "Hermes bridge service manager is unavailable; systemd user services require Linux, {ENABLE_SYSTEMD_SERVICE_ENV}=1, HOME, XDG_RUNTIME_DIR, and DBUS_SESSION_BUS_ADDRESS"
    )
}

fn systemd_user_env_available() -> bool {
    env::var_os("HOME").is_some_and(|value| !value.is_empty())
        && env::var_os("XDG_RUNTIME_DIR").is_some_and(|value| !value.is_empty())
        && env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some_and(|value| !value.is_empty())
}

fn enabled_by_env() -> bool {
    service_enabled_by_env_value(env::var(ENABLE_SYSTEMD_SERVICE_ENV).ok().as_deref())
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

fn http_status_for(url: &str) -> anyhow::Result<u16> {
    let parsed = ParsedHttpUrl::parse(url)?;
    let address = format!("{}:{}", parsed.host, parsed.port);
    let mut stream = address
        .to_socket_addrs()
        .map_err(|err| anyhow::anyhow!("resolve Hermes bridge health host: {err}"))?
        .find_map(|address| TcpStream::connect_timeout(&address, Duration::from_secs(2)).ok())
        .ok_or_else(|| anyhow::anyhow!("connect Hermes bridge health endpoint"))?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        parsed.path, parsed.host_header
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|err| anyhow::anyhow!("write Hermes bridge health request: {err}"))?;
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|err| anyhow::anyhow!("read Hermes bridge health response: {err}"))?;
    parse_http_status_line(&response)
}

fn parse_http_status_line(response: &str) -> anyhow::Result<u16> {
    let status = response
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .ok_or_else(|| anyhow::anyhow!("Hermes bridge health response missing status"))?;
    status
        .parse::<u16>()
        .map_err(|err| anyhow::anyhow!("parse Hermes bridge health status: {err}"))
}

struct ParsedHttpUrl {
    host: String,
    host_header: String,
    port: u16,
    path: String,
}

impl ParsedHttpUrl {
    fn parse(url: &str) -> anyhow::Result<Self> {
        let value = url
            .trim()
            .strip_prefix("http://")
            .ok_or_else(|| anyhow::anyhow!("Hermes bridge health URL must use http"))?;
        let (authority, path) = value
            .split_once('/')
            .map(|(authority, path)| (authority, format!("/{path}")))
            .unwrap_or((value, "/".to_string()));
        if authority.trim().is_empty() {
            anyhow::bail!("Hermes bridge health URL missing host");
        }
        let (host, port) = parse_http_authority(authority)?;
        let host_header = if authority.contains(':') {
            authority.to_string()
        } else {
            format!("{host}:{port}")
        };
        Ok(Self {
            host,
            host_header,
            port,
            path,
        })
    }
}

fn parse_http_authority(authority: &str) -> anyhow::Result<(String, u16)> {
    if let Some(rest) = authority.strip_prefix('[') {
        let (host, tail) = rest
            .split_once(']')
            .ok_or_else(|| anyhow::anyhow!("Hermes bridge health URL has invalid IPv6 host"))?;
        let port = tail
            .strip_prefix(':')
            .map(parse_http_port)
            .transpose()?
            .unwrap_or(80);
        return Ok((host.to_string(), port));
    }
    let (host, port) = if let Some((host, port)) = authority
        .rsplit_once(':')
        .filter(|(_, port)| !port.is_empty() && port.chars().all(|ch| ch.is_ascii_digit()))
    {
        (host.to_string(), parse_http_port(port)?)
    } else {
        (authority.to_string(), 80)
    };
    if host.trim().is_empty() {
        anyhow::bail!("Hermes bridge health URL missing host");
    }
    Ok((host, port))
}

fn parse_http_port(port: &str) -> anyhow::Result<u16> {
    port.parse::<u16>()
        .map_err(|err| anyhow::anyhow!("parse Hermes bridge health URL port: {err}"))
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
