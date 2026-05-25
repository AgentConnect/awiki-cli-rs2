use crate::workspace_config::Resolved;
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::bridge::MODE_WEBSOCKET;
use super::listener::{self, Status};

pub const SERVICE_NAME_PREFIX: &str = "awiki-cli-listener";
pub const SERVICE_DISPLAY_NAME_PREFIX: &str = "awiki-cli Listener";
pub const SERVICE_DESCRIPTION: &str = "awiki-cli realtime websocket listener";
pub const SERVICE_ARGUMENTS: &[&str] = &["runtime", "listener", "service-run"];
pub const SERVICE_PID_FILE_NAME: &str = "listener.service.pid";
pub const WORKSPACE_HOME_ENV: &str = "AWIKI_CLI_WORKSPACE_HOME_DIR";
pub const LISTENER_SERVICE_MODE_ENV: &str = "AWIKI_LISTENER_SERVICE_MODE";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ForegroundSignal {
    Interrupt,
    Sigterm,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ListenerChildProcessPlan {
    pub setsid: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ListenerServiceConfigValue {
    Bool(bool),
    String(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListenerServiceConfigPlan {
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub arguments: Vec<String>,
    pub working_directory: String,
    pub options: BTreeMap<String, ListenerServiceConfigValue>,
    pub env_vars: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ListenerPlatformStatusResult {
    Running,
    NotRunning,
    ErrNotInstalled,
    ErrOther(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ListenerServiceStatusForAction {
    NewService,
    ServiceStatus,
    ServicePlatform,
    ServiceName,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListenerServiceStatusForPlan {
    pub actions: Vec<ListenerServiceStatusForAction>,
    pub installed: bool,
    pub running: bool,
    pub platform: String,
    pub service_name: String,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ListenerEnsureInstallDecision {
    ReturnStatus,
    ReturnError(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ListenerRunServiceAction {
    CreateServiceProgram,
    NewService,
    ServiceRun,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ListenerRunServiceDecision {
    ReturnOk,
    ReturnError(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListenerRunServicePlan {
    pub actions: Vec<ListenerRunServiceAction>,
    pub decision: ListenerRunServiceDecision,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ListenerServiceProgramState {
    pub has_supervisor: bool,
    pub has_cancel: bool,
    pub has_done: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ListenerServiceProgramAction {
    LockProgram,
    NewSupervisor,
    CreateCancelContext,
    CreateDoneChannel,
    StoreSupervisor,
    StoreCancel,
    StoreDone,
    SpawnRunLoop,
    SnapshotState,
    ClearCancel,
    ClearDone,
    ClearSupervisor,
    UnlockProgram,
    CancelContext,
    CloseSupervisor,
    WaitDone { timeout: Duration },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ListenerServiceProgramRunAction {
    RunSupervisor,
    SendRunResult,
    CleanupRuntimeArtifacts,
    CloseDone,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ListenerServiceProgramDecision {
    ReturnOk,
    ReturnError(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListenerServiceProgramPlan {
    pub actions: Vec<ListenerServiceProgramAction>,
    pub run_loop_actions: Vec<ListenerServiceProgramRunAction>,
    pub decision: ListenerServiceProgramDecision,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ListenerServiceLifecycleOperation {
    ValidateWebSocketMode,
    NewService,
    CheckStatus,
    InstallIfMissing,
    CallEnsureInstalled,
    RecheckStatus,
    ErrorNotInstalledAfterAutoInstall,
    ErrorNotInstalled,
    PrepareBootId,
    ServiceStart,
    ServiceStop,
    ServiceRestart,
    ServiceUninstall,
    CleanupRuntimeArtifacts,
    WaitForRunning { expected_boot_id: String },
    WaitForStopped,
    ReturnStatus,
    CallStartService,
    CallStopService,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ListenerServiceStatusSnapshot {
    pub installed: bool,
    pub running: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ListenerRuntimePolicy {
    pub websocket_mode: bool,
    pub listener_enabled: bool,
    pub auto_install: bool,
    pub auto_start: bool,
}

impl ListenerRuntimePolicy {
    pub fn from_resolved(resolved: &Resolved) -> Self {
        Self {
            websocket_mode: resolved
                .runtime_mode
                .trim()
                .eq_ignore_ascii_case(MODE_WEBSOCKET),
            listener_enabled: resolved.runtime_listener_enabled,
            auto_install: resolved.runtime_listener_auto_install,
            auto_start: resolved.runtime_listener_auto_start,
        }
    }
}

pub fn service_program_start_plan(
    state: ListenerServiceProgramState,
    new_supervisor_error: Option<&str>,
) -> ListenerServiceProgramPlan {
    let mut actions = vec![ListenerServiceProgramAction::LockProgram];
    if state.has_done {
        actions.push(ListenerServiceProgramAction::UnlockProgram);
        return service_program_plan(
            actions,
            Vec::new(),
            ListenerServiceProgramDecision::ReturnOk,
        );
    }

    actions.push(ListenerServiceProgramAction::NewSupervisor);
    if let Some(error) = new_supervisor_error {
        actions.push(ListenerServiceProgramAction::UnlockProgram);
        return service_program_plan(
            actions,
            Vec::new(),
            ListenerServiceProgramDecision::ReturnError(error.to_string()),
        );
    }

    actions.push(ListenerServiceProgramAction::CreateCancelContext);
    actions.push(ListenerServiceProgramAction::CreateDoneChannel);
    actions.push(ListenerServiceProgramAction::StoreSupervisor);
    actions.push(ListenerServiceProgramAction::StoreCancel);
    actions.push(ListenerServiceProgramAction::StoreDone);
    actions.push(ListenerServiceProgramAction::SpawnRunLoop);
    actions.push(ListenerServiceProgramAction::UnlockProgram);

    service_program_plan(
        actions,
        vec![
            ListenerServiceProgramRunAction::RunSupervisor,
            ListenerServiceProgramRunAction::SendRunResult,
            ListenerServiceProgramRunAction::CleanupRuntimeArtifacts,
            ListenerServiceProgramRunAction::CloseDone,
        ],
        ListenerServiceProgramDecision::ReturnOk,
    )
}

pub fn service_program_stop_plan(
    state: ListenerServiceProgramState,
    done_completed_before_timeout: bool,
) -> ListenerServiceProgramPlan {
    let mut actions = vec![
        ListenerServiceProgramAction::LockProgram,
        ListenerServiceProgramAction::SnapshotState,
        ListenerServiceProgramAction::ClearCancel,
        ListenerServiceProgramAction::ClearDone,
        ListenerServiceProgramAction::ClearSupervisor,
        ListenerServiceProgramAction::UnlockProgram,
    ];

    if state.has_cancel {
        actions.push(ListenerServiceProgramAction::CancelContext);
    }
    if state.has_supervisor {
        actions.push(ListenerServiceProgramAction::CloseSupervisor);
    }
    if state.has_done {
        actions.push(ListenerServiceProgramAction::WaitDone {
            timeout: Duration::from_secs(15),
        });
        if !done_completed_before_timeout {
            return service_program_plan(
                actions,
                Vec::new(),
                ListenerServiceProgramDecision::ReturnError(
                    "listener service stop timed out".to_string(),
                ),
            );
        }
    }

    service_program_plan(
        actions,
        Vec::new(),
        ListenerServiceProgramDecision::ReturnOk,
    )
}

fn service_program_plan(
    actions: Vec<ListenerServiceProgramAction>,
    run_loop_actions: Vec<ListenerServiceProgramRunAction>,
    decision: ListenerServiceProgramDecision,
) -> ListenerServiceProgramPlan {
    ListenerServiceProgramPlan {
        actions,
        run_loop_actions,
        decision,
    }
}

pub fn service_name_for(resolved: &Resolved) -> String {
    let workspace = resolved.paths.workspace_home_dir.trim();
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

pub fn service_config_plan_for(resolved: &Resolved, is_windows: bool) -> ListenerServiceConfigPlan {
    let mut options = BTreeMap::new();
    options.insert(
        "RunAtLoad".to_string(),
        ListenerServiceConfigValue::Bool(true),
    );
    options.insert(
        "KeepAlive".to_string(),
        ListenerServiceConfigValue::Bool(true),
    );
    options.insert(
        "UserService".to_string(),
        ListenerServiceConfigValue::Bool(true),
    );
    options.insert(
        "DelayedAutoStart".to_string(),
        ListenerServiceConfigValue::Bool(true),
    );
    options.insert(
        "StartType".to_string(),
        ListenerServiceConfigValue::String("automatic".to_string()),
    );
    options.insert(
        "OnFailure".to_string(),
        ListenerServiceConfigValue::String("restart".to_string()),
    );
    options.insert(
        "OnFailureDelayDuration".to_string(),
        ListenerServiceConfigValue::String("1s".to_string()),
    );
    options.insert(
        "LogOutput".to_string(),
        ListenerServiceConfigValue::Bool(true),
    );
    options.insert(
        "LogDirectory".to_string(),
        ListenerServiceConfigValue::String(resolved.paths.logs_dir.clone()),
    );
    options.insert(
        "PIDFile".to_string(),
        ListenerServiceConfigValue::String(
            PathBuf::from(&resolved.paths.state_dir)
                .join(SERVICE_PID_FILE_NAME)
                .to_string_lossy()
                .into_owned(),
        ),
    );

    let mut env_vars = BTreeMap::new();
    env_vars.insert(
        WORKSPACE_HOME_ENV.to_string(),
        resolved.paths.workspace_home_dir.clone(),
    );
    env_vars.insert(LISTENER_SERVICE_MODE_ENV.to_string(), "1".to_string());

    ListenerServiceConfigPlan {
        name: service_name_for(resolved),
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
        options,
        env_vars,
    }
}

pub fn service_status_for_plan(
    resolved: &Resolved,
    platform: &str,
    new_service_error: Option<&str>,
    status_result: ListenerPlatformStatusResult,
) -> ListenerServiceStatusForPlan {
    if let Some(error) = new_service_error {
        return ListenerServiceStatusForPlan {
            actions: vec![ListenerServiceStatusForAction::NewService],
            installed: false,
            running: false,
            platform: String::new(),
            service_name: String::new(),
            error: Some(error.to_string()),
        };
    }

    let mut plan = ListenerServiceStatusForPlan {
        actions: vec![
            ListenerServiceStatusForAction::NewService,
            ListenerServiceStatusForAction::ServiceStatus,
            ListenerServiceStatusForAction::ServicePlatform,
            ListenerServiceStatusForAction::ServiceName,
        ],
        installed: false,
        running: false,
        platform: platform.to_string(),
        service_name: service_name_for(resolved),
        error: None,
    };

    match status_result {
        ListenerPlatformStatusResult::Running => {
            plan.installed = true;
            plan.running = true;
        }
        ListenerPlatformStatusResult::NotRunning => {
            plan.installed = true;
        }
        ListenerPlatformStatusResult::ErrNotInstalled => {}
        ListenerPlatformStatusResult::ErrOther(error) => {
            plan.error = Some(error);
        }
    }

    plan
}

pub fn ensure_installed_install_decision(
    install_error: Option<&str>,
) -> ListenerEnsureInstallDecision {
    let Some(error) = install_error else {
        return ListenerEnsureInstallDecision::ReturnStatus;
    };
    if error.to_lowercase().contains("exists") {
        return ListenerEnsureInstallDecision::ReturnStatus;
    }
    ListenerEnsureInstallDecision::ReturnError(error.to_string())
}

pub fn run_service_plan(
    new_service_error: Option<&str>,
    run_error: Option<&str>,
) -> ListenerRunServicePlan {
    let mut actions = vec![
        ListenerRunServiceAction::CreateServiceProgram,
        ListenerRunServiceAction::NewService,
    ];

    if let Some(error) = new_service_error {
        return ListenerRunServicePlan {
            actions,
            decision: ListenerRunServiceDecision::ReturnError(error.to_string()),
        };
    }

    actions.push(ListenerRunServiceAction::ServiceRun);

    let decision = match run_error {
        Some(error) => ListenerRunServiceDecision::ReturnError(error.to_string()),
        None => ListenerRunServiceDecision::ReturnOk,
    };

    ListenerRunServicePlan { actions, decision }
}

pub fn service_status_ready(
    status: &Status,
    want_running: bool,
    wait_for_bridge: bool,
    expected_boot_id: &str,
) -> bool {
    if want_running {
        if !status.installed || !status.running {
            return false;
        }
        if wait_for_bridge && !status.bridge_available {
            return false;
        }
        let expected_boot_id = expected_boot_id.trim();
        if !expected_boot_id.is_empty() && status.boot_id.trim() != expected_boot_id {
            return false;
        }
        return true;
    }
    !status.running
}

pub fn wait_for_service_status_with(
    mut status_fn: impl FnMut() -> anyhow::Result<Status>,
    want_running: bool,
    wait_for_bridge: bool,
    expected_boot_id: &str,
    timeout: Duration,
    interval: Duration,
) -> anyhow::Result<Status> {
    let deadline = Instant::now() + timeout;
    let mut last_status = Status::default();
    let mut last_err = None;
    loop {
        match status_fn() {
            Ok(status) => {
                last_status = status;
                if service_status_ready(
                    &last_status,
                    want_running,
                    wait_for_bridge,
                    expected_boot_id,
                ) {
                    return Ok(last_status);
                }
            }
            Err(err) => last_err = Some(err),
        }
        if Instant::now() > deadline {
            if let Some(err) = last_err {
                return Err(err);
            }
            return Ok(last_status);
        }
        std::thread::sleep(interval);
    }
}

pub fn cleanup_runtime_artifacts(resolved: &Resolved) {
    let Ok(paths) = listener::paths(resolved) else {
        return;
    };
    let _ = fs::remove_file(paths.pid_file);
    let _ = fs::remove_file(paths.status_file);
    let _ = fs::remove_file(paths.socket_path);
    if let Ok(boot_id_file) = listener::boot_id_path(resolved) {
        let _ = fs::remove_file(boot_id_file);
    }
}

pub fn running_in_listener_service_mode() -> bool {
    let env_value = env::var(LISTENER_SERVICE_MODE_ENV).ok();
    let args = env::args().collect::<Vec<_>>();
    running_in_listener_service_mode_with(env_value.as_deref(), &args)
}

pub fn running_in_listener_service_mode_with(env_value: Option<&str>, args: &[String]) -> bool {
    if let Some(value) = env_value {
        let value = value.trim();
        if value == "1" || value.eq_ignore_ascii_case("true") {
            return true;
        }
    }
    if args.len() < 4 {
        return false;
    }
    args[1].trim().eq_ignore_ascii_case("runtime")
        && args[2].trim().eq_ignore_ascii_case("listener")
        && args[3].trim().eq_ignore_ascii_case("service-run")
}

pub fn foreground_signal_plan_for_platform(is_windows: bool) -> Vec<ForegroundSignal> {
    if is_windows {
        return vec![ForegroundSignal::Interrupt];
    }
    vec![ForegroundSignal::Interrupt, ForegroundSignal::Sigterm]
}

pub fn foreground_signal_plan() -> Vec<ForegroundSignal> {
    foreground_signal_plan_for_platform(cfg!(windows))
}

pub fn listener_child_process_plan_for_platform(is_windows: bool) -> ListenerChildProcessPlan {
    ListenerChildProcessPlan {
        setsid: !is_windows,
    }
}

pub fn listener_child_process_plan() -> ListenerChildProcessPlan {
    listener_child_process_plan_for_platform(cfg!(windows))
}

pub fn ensure_installed_plan(
    status: ListenerServiceStatusSnapshot,
) -> Vec<ListenerServiceLifecycleOperation> {
    let mut operations = vec![
        ListenerServiceLifecycleOperation::NewService,
        ListenerServiceLifecycleOperation::CheckStatus,
    ];
    if !status.installed {
        operations.push(ListenerServiceLifecycleOperation::InstallIfMissing);
    }
    operations.push(ListenerServiceLifecycleOperation::ReturnStatus);
    operations
}

pub fn start_service_plan(
    runtime_mode: &str,
    before: ListenerServiceStatusSnapshot,
    after_auto_install: Option<ListenerServiceStatusSnapshot>,
    expected_boot_id: &str,
) -> Vec<ListenerServiceLifecycleOperation> {
    let mut operations = vec![ListenerServiceLifecycleOperation::ValidateWebSocketMode];
    if !runtime_mode.trim().eq_ignore_ascii_case(MODE_WEBSOCKET) {
        return operations;
    }
    operations.push(ListenerServiceLifecycleOperation::NewService);
    operations.push(ListenerServiceLifecycleOperation::CheckStatus);
    let mut status = before;
    if !status.installed {
        operations.push(ListenerServiceLifecycleOperation::CallEnsureInstalled);
        operations.push(ListenerServiceLifecycleOperation::RecheckStatus);
        status = after_auto_install.unwrap_or(status);
        if !status.installed {
            operations.push(ListenerServiceLifecycleOperation::ErrorNotInstalledAfterAutoInstall);
            return operations;
        }
    }
    if status.running {
        operations.push(ListenerServiceLifecycleOperation::ReturnStatus);
        return operations;
    }
    operations.push(ListenerServiceLifecycleOperation::PrepareBootId);
    operations.push(ListenerServiceLifecycleOperation::ServiceStart);
    operations.push(ListenerServiceLifecycleOperation::WaitForRunning {
        expected_boot_id: expected_boot_id.to_string(),
    });
    operations
}

pub fn stop_service_plan(
    status: ListenerServiceStatusSnapshot,
) -> Vec<ListenerServiceLifecycleOperation> {
    let mut operations = vec![
        ListenerServiceLifecycleOperation::NewService,
        ListenerServiceLifecycleOperation::CheckStatus,
    ];
    if !status.installed {
        operations.push(ListenerServiceLifecycleOperation::ReturnStatus);
        return operations;
    }
    if status.running {
        operations.push(ListenerServiceLifecycleOperation::ServiceStop);
    }
    operations.push(ListenerServiceLifecycleOperation::CleanupRuntimeArtifacts);
    operations.push(ListenerServiceLifecycleOperation::WaitForStopped);
    operations
}

pub fn restart_service_plan(
    status: ListenerServiceStatusSnapshot,
    expected_boot_id: &str,
) -> Vec<ListenerServiceLifecycleOperation> {
    let mut operations = vec![
        ListenerServiceLifecycleOperation::NewService,
        ListenerServiceLifecycleOperation::CheckStatus,
    ];
    if !status.installed {
        operations.push(ListenerServiceLifecycleOperation::ErrorNotInstalled);
        return operations;
    }
    operations.push(ListenerServiceLifecycleOperation::PrepareBootId);
    operations.push(ListenerServiceLifecycleOperation::ServiceRestart);
    operations.push(ListenerServiceLifecycleOperation::WaitForRunning {
        expected_boot_id: expected_boot_id.to_string(),
    });
    operations
}

pub fn uninstall_service_plan(
    status: ListenerServiceStatusSnapshot,
) -> Vec<ListenerServiceLifecycleOperation> {
    let mut operations = vec![
        ListenerServiceLifecycleOperation::NewService,
        ListenerServiceLifecycleOperation::CheckStatus,
    ];
    if !status.installed {
        operations.push(ListenerServiceLifecycleOperation::CleanupRuntimeArtifacts);
        operations.push(ListenerServiceLifecycleOperation::ReturnStatus);
        return operations;
    }
    if status.running {
        operations.push(ListenerServiceLifecycleOperation::ServiceStop);
    }
    operations.push(ListenerServiceLifecycleOperation::ServiceUninstall);
    operations.push(ListenerServiceLifecycleOperation::CleanupRuntimeArtifacts);
    operations.push(ListenerServiceLifecycleOperation::ReturnStatus);
    operations
}

pub fn apply_runtime_policy_plan(
    policy: ListenerRuntimePolicy,
    status: ListenerServiceStatusSnapshot,
) -> Vec<ListenerServiceLifecycleOperation> {
    if !policy.websocket_mode || !policy.listener_enabled {
        return vec![ListenerServiceLifecycleOperation::CallStopService];
    }
    if policy.auto_install {
        let mut operations = vec![ListenerServiceLifecycleOperation::CallEnsureInstalled];
        if policy.auto_start {
            operations.push(ListenerServiceLifecycleOperation::CallStartService);
        } else {
            operations.push(ListenerServiceLifecycleOperation::ReturnStatus);
        }
        return operations;
    }
    if policy.auto_start {
        let mut operations = vec![ListenerServiceLifecycleOperation::CheckStatus];
        if status.installed {
            operations.push(ListenerServiceLifecycleOperation::CallStartService);
        } else {
            operations.push(ListenerServiceLifecycleOperation::ReturnStatus);
        }
        return operations;
    }
    vec![ListenerServiceLifecycleOperation::ReturnStatus]
}

pub fn generate_boot_id() -> String {
    let timestamp = unix_time_nanos();
    let mut random_suffix = [0u8; 4];
    let mut rng = rand::rngs::OsRng;
    if rng.try_fill_bytes(&mut random_suffix).is_err() {
        return format!("boot-{timestamp}");
    }
    format!("boot-{timestamp}-{}", hex_lower(&random_suffix))
}

pub fn prepare_expected_boot_id(resolved: &Resolved) -> anyhow::Result<String> {
    let path = listener::boot_id_path(resolved)?;
    let boot_id = generate_boot_id();
    listener::write_expected_boot_id(&path, &boot_id)
        .map_err(|err| anyhow::anyhow!("write expected listener boot id: {err}"))?;
    Ok(boot_id)
}

pub fn resolve_runtime_boot_id(resolved: &Resolved) -> anyhow::Result<String> {
    let path = listener::boot_id_path(resolved)?;
    match listener::read_expected_boot_id(&path) {
        Ok(boot_id) if !boot_id.trim().is_empty() => Ok(boot_id.trim().to_string()),
        Ok(_) => Ok(generate_boot_id()),
        Err(err) => {
            if err
                .root_cause()
                .downcast_ref::<std::io::Error>()
                .is_some_and(|err| err.kind() == std::io::ErrorKind::NotFound)
            {
                return Ok(generate_boot_id());
            }
            Err(anyhow::anyhow!("read expected listener boot id: {err}"))
        }
    }
}

fn unix_time_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}
