use std::collections::BTreeMap;

use crate::workspace_config::Resolved;

use super::listener_service::{
    self, ListenerPlatformStatusResult, ListenerServiceConfigPlan, ListenerServiceConfigValue,
};

pub const SERVICE_PLATFORM: &str = "windows-service";
pub const START_TYPE_AUTOMATIC: &str = "automatic";
pub const FAILURE_ACTION_RESTART: &str = "restart";
pub const FAILURE_ACTION_DELAY: &str = "1s";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowsListenerServiceContract {
    pub platform: &'static str,
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub arguments: Vec<String>,
    pub working_directory: String,
    pub env_vars: BTreeMap<String, String>,
    pub start_type: String,
    pub auto_start: bool,
    pub restart_on_failure: bool,
    pub restart_delay: String,
    pub log_output: bool,
    pub log_directory: String,
    pub pid_file: String,
    pub managed_by_scm: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowsServiceStatus {
    pub installed: bool,
    pub running: bool,
    pub state: String,
}

pub fn service_platform() -> &'static str {
    SERVICE_PLATFORM
}

pub fn service_contract_for(resolved: &Resolved) -> WindowsListenerServiceContract {
    let plan = service_config_plan_for(resolved);
    WindowsListenerServiceContract {
        platform: SERVICE_PLATFORM,
        name: plan.name.clone(),
        display_name: plan.display_name.clone(),
        description: plan.description.clone(),
        arguments: plan.arguments.clone(),
        working_directory: plan.working_directory.clone(),
        env_vars: plan.env_vars.clone(),
        start_type: config_string(&plan, "StartType").unwrap_or_default(),
        auto_start: config_string(&plan, "StartType")
            .map(|value| value.eq_ignore_ascii_case(START_TYPE_AUTOMATIC))
            .unwrap_or(false),
        restart_on_failure: config_string(&plan, "OnFailure")
            .map(|value| value.eq_ignore_ascii_case(FAILURE_ACTION_RESTART))
            .unwrap_or(false),
        restart_delay: config_string(&plan, "OnFailureDelayDuration").unwrap_or_default(),
        log_output: config_bool(&plan, "LogOutput").unwrap_or(false),
        log_directory: config_string(&plan, "LogDirectory").unwrap_or_default(),
        pid_file: config_string(&plan, "PIDFile").unwrap_or_default(),
        managed_by_scm: true,
    }
}

pub fn service_config_plan_for(resolved: &Resolved) -> ListenerServiceConfigPlan {
    listener_service::service_config_plan_for(resolved, true)
}

pub fn service_name_for(resolved: &Resolved) -> String {
    listener_service::service_name_for(resolved)
}

pub fn service_display_name_for(resolved: &Resolved) -> String {
    listener_service::service_display_name_for(Some(resolved))
}

pub fn service_arguments(resolved: &Resolved) -> Vec<String> {
    listener_service::service_arguments_for(resolved)
}

pub fn service_launch_arguments(
    resolved: &Resolved,
    user_sid: &str,
) -> anyhow::Result<Vec<String>> {
    if !valid_user_sid(user_sid) {
        anyhow::bail!("listener Windows service user SID is invalid");
    }
    let mut arguments = listener_service::service_arguments_for(resolved);
    let command_offset = arguments
        .iter()
        .position(|argument| argument == listener_service::SERVICE_COMMAND_ARGUMENTS[0])
        .unwrap_or(arguments.len());
    arguments.splice(
        command_offset..command_offset,
        [
            listener_service::INTERNAL_SERVICE_USER_SID_FLAG.to_string(),
            user_sid.to_string(),
        ],
    );
    Ok(arguments)
}

pub fn valid_user_sid(value: &str) -> bool {
    let mut parts = value.trim().split('-');
    if parts.next() != Some("S") {
        return false;
    }
    let numeric_parts = parts.collect::<Vec<_>>();
    numeric_parts.len() >= 3
        && numeric_parts
            .iter()
            .all(|part| !part.is_empty() && part.as_bytes().iter().all(u8::is_ascii_digit))
}

pub fn pipe_security_sddl(user_sid: &str) -> anyhow::Result<String> {
    if !valid_user_sid(user_sid) {
        anyhow::bail!("listener Windows service user SID is invalid");
    }
    Ok(format!(
        "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;{})",
        user_sid.trim()
    ))
}

#[cfg(windows)]
mod scm {
    use super::*;
    use crate::host_runtime::listener::{self, Status};
    use crate::host_runtime::{listener_service, listener_shutdown_signal};
    use crate::workspace_config::Resolved;
    use std::ffi::{OsStr, OsString};
    use std::fs;
    use std::sync::{mpsc, Mutex, OnceLock};
    use std::thread;
    use std::time::{Duration, Instant};
    use windows_service::define_windows_service;
    use windows_service::service::{
        ServiceAccess, ServiceAction, ServiceActionType, ServiceControl, ServiceControlAccept,
        ServiceErrorControl, ServiceExitCode, ServiceFailureActions, ServiceFailureResetPeriod,
        ServiceInfo, ServiceStartType, ServiceState, ServiceStatus, ServiceType,
    };
    use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
    use windows_service::service_dispatcher;
    use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

    static SERVICE_CONTEXT: OnceLock<Mutex<Option<Resolved>>> = OnceLock::new();
    const SERVICE_TYPE: ServiceType = ServiceType::OWN_PROCESS;

    define_windows_service!(ffi_service_main, service_main);

    pub fn status(resolved: &Resolved) -> anyhow::Result<WindowsServiceStatus> {
        let manager = service_manager(ServiceManagerAccess::CONNECT)?;
        let service =
            match manager.open_service(service_name_for(resolved), ServiceAccess::QUERY_STATUS) {
                Ok(service) => service,
                Err(err) if is_not_installed(&err) => {
                    return Ok(WindowsServiceStatus {
                        installed: false,
                        running: false,
                        state: "not-installed".to_string(),
                    });
                }
                Err(err) => return Err(anyhow::anyhow!("open listener Windows service: {err}")),
            };
        let service_status = service
            .query_status()
            .map_err(|err| anyhow::anyhow!("query listener Windows service: {err}"))?;
        Ok(WindowsServiceStatus {
            installed: true,
            running: service_status.current_state == ServiceState::Running,
            state: service_state_name(service_status.current_state).to_string(),
        })
    }

    pub fn install(resolved: &Resolved) -> anyhow::Result<Status> {
        fs::create_dir_all(&resolved.paths.logs_dir)
            .map_err(|err| anyhow::anyhow!("create listener log directory: {err}"))?;
        let manager =
            service_manager(ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE)?;
        let user_sid = current_process_user_sid()?;
        let info = service_info(resolved, &user_sid)?;
        let access = ServiceAccess::QUERY_STATUS
            | ServiceAccess::CHANGE_CONFIG
            | ServiceAccess::START
            | ServiceAccess::STOP
            | ServiceAccess::DELETE;
        let service = match manager.open_service(service_name_for(resolved), access) {
            Ok(service) => {
                service
                    .change_config(&info)
                    .map_err(|err| anyhow::anyhow!("update listener Windows service: {err}"))?;
                service
            }
            Err(err) if is_not_installed(&err) => manager
                .create_service(&info, access)
                .map_err(|err| anyhow::anyhow!("install listener Windows service: {err}"))?,
            Err(err) => return Err(anyhow::anyhow!("open listener Windows service: {err}")),
        };
        service
            .set_description(listener_service::SERVICE_DESCRIPTION)
            .map_err(|err| anyhow::anyhow!("set listener Windows service description: {err}"))?;
        service
            .update_failure_actions(ServiceFailureActions {
                reset_period: ServiceFailureResetPeriod::After(Duration::from_secs(60)),
                reboot_msg: None,
                command: None,
                actions: Some(vec![ServiceAction {
                    action_type: ServiceActionType::Restart,
                    delay: Duration::from_secs(1),
                }]),
            })
            .map_err(|err| anyhow::anyhow!("configure listener service recovery: {err}"))?;
        listener_status(resolved)
    }

    pub fn start(resolved: &Resolved) -> anyhow::Result<Status> {
        if crate::host_runtime::resolve(resolved).mode
            != crate::host_runtime::bridge::MODE_WEBSOCKET
        {
            anyhow::bail!("runtime mode must be websocket before starting the listener");
        }
        if !status(resolved)?.installed {
            install(resolved)?;
        }
        let manager = service_manager(ServiceManagerAccess::CONNECT)?;
        let service = manager
            .open_service(
                service_name_for(resolved),
                ServiceAccess::QUERY_STATUS | ServiceAccess::START | ServiceAccess::STOP,
            )
            .map_err(|err| anyhow::anyhow!("open listener Windows service: {err}"))?;
        if service
            .query_status()
            .map_err(|err| anyhow::anyhow!("query listener Windows service: {err}"))?
            .current_state
            != ServiceState::Stopped
        {
            let _ = service.stop();
            wait_for_scm_state(&service, ServiceState::Stopped, Duration::from_secs(15))?;
        }
        let expected_boot_id = listener_service::prepare_expected_boot_id(resolved)?;
        service
            .start::<&OsStr>(&[])
            .map_err(|err| anyhow::anyhow!("start listener Windows service: {err}"))?;
        wait_for_listener_status(resolved, true, &expected_boot_id)
    }

    pub fn stop(resolved: &Resolved) -> anyhow::Result<Status> {
        let manager = service_manager(ServiceManagerAccess::CONNECT)?;
        match manager.open_service(
            service_name_for(resolved),
            ServiceAccess::QUERY_STATUS | ServiceAccess::STOP,
        ) {
            Ok(service) => {
                let current = service
                    .query_status()
                    .map_err(|err| anyhow::anyhow!("query listener Windows service: {err}"))?;
                if current.current_state != ServiceState::Stopped {
                    service
                        .stop()
                        .map_err(|err| anyhow::anyhow!("stop listener Windows service: {err}"))?;
                    wait_for_scm_state(&service, ServiceState::Stopped, Duration::from_secs(15))?;
                }
            }
            Err(err) if is_not_installed(&err) => {}
            Err(err) => return Err(anyhow::anyhow!("open listener Windows service: {err}")),
        }
        listener_service::cleanup_runtime_artifacts(resolved);
        wait_for_listener_status(resolved, false, "")
    }

    pub fn restart(resolved: &Resolved) -> anyhow::Result<Status> {
        if !status(resolved)?.installed {
            anyhow::bail!("listener service is not installed");
        }
        stop(resolved)?;
        start(resolved)
    }

    pub fn uninstall(resolved: &Resolved) -> anyhow::Result<Status> {
        let manager = service_manager(ServiceManagerAccess::CONNECT)?;
        match manager.open_service(
            service_name_for(resolved),
            ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::DELETE,
        ) {
            Ok(service) => {
                if service
                    .query_status()
                    .map_err(|err| anyhow::anyhow!("query listener Windows service: {err}"))?
                    .current_state
                    != ServiceState::Stopped
                {
                    let _ = service.stop();
                    wait_for_scm_state(&service, ServiceState::Stopped, Duration::from_secs(15))?;
                }
                service
                    .delete()
                    .map_err(|err| anyhow::anyhow!("uninstall listener Windows service: {err}"))?;
            }
            Err(err) if is_not_installed(&err) => {}
            Err(err) => return Err(anyhow::anyhow!("open listener Windows service: {err}")),
        }
        listener_service::cleanup_runtime_artifacts(resolved);
        Ok(listener::status_for(
            resolved,
            false,
            false,
            service_platform(),
        )?)
    }

    pub fn listener_status(resolved: &Resolved) -> anyhow::Result<Status> {
        let service = status(resolved)?;
        let mut result = listener::status_for(
            resolved,
            service.installed,
            service.running,
            service_platform(),
        )?;
        if service.installed && service.state != "running" && service.state != "stopped" {
            result.warnings.push(format!(
                "listener Windows service state is {}",
                service.state
            ));
        }
        Ok(result)
    }

    pub fn run_dispatcher(resolved: Resolved) -> anyhow::Result<()> {
        let service_name = service_name_for(&resolved);
        let context = SERVICE_CONTEXT.get_or_init(|| Mutex::new(None));
        *context
            .lock()
            .map_err(|_| anyhow::anyhow!("listener Windows service context lock is poisoned"))? =
            Some(resolved);
        service_dispatcher::start(service_name, ffi_service_main)
            .map_err(|err| anyhow::anyhow!("start listener Windows service dispatcher: {err}"))
    }

    fn service_main(_arguments: Vec<OsString>) {
        let result = run_service_main();
        if let Err(err) = result {
            write_service_error(&err.to_string());
        }
    }

    fn run_service_main() -> anyhow::Result<()> {
        let resolved = SERVICE_CONTEXT
            .get()
            .ok_or_else(|| anyhow::anyhow!("listener Windows service context is unavailable"))?
            .lock()
            .map_err(|_| anyhow::anyhow!("listener Windows service context lock is poisoned"))?
            .clone()
            .ok_or_else(|| anyhow::anyhow!("listener Windows service context is missing"))?;
        let name = service_name_for(&resolved);
        let event_handler = move |event| match event {
            ServiceControl::Stop | ServiceControl::Shutdown => {
                listener_shutdown_signal::request_shutdown();
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        };
        let status_handle = service_control_handler::register(name, event_handler)
            .map_err(|err| anyhow::anyhow!("register listener service control handler: {err}"))?;
        status_handle
            .set_service_status(service_process_status(
                ServiceState::StartPending,
                ServiceControlAccept::empty(),
                Duration::from_secs(15),
            ))
            .map_err(|err| anyhow::anyhow!("report listener service start pending: {err}"))?;

        let (done_tx, done_rx) = mpsc::sync_channel(1);
        let worker_resolved = resolved.clone();
        thread::spawn(move || {
            let result = crate::host_runtime::listener_supervisor_run::run_service(worker_resolved);
            let _ = done_tx.send(result);
        });

        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if listener::bridge_endpoint_available(&listener::paths(&resolved)?.socket_path) {
                break;
            }
            if let Ok(result) = done_rx.try_recv() {
                return result;
            }
            if Instant::now() >= deadline {
                listener_shutdown_signal::request_shutdown();
                anyhow::bail!("listener bridge did not become ready before the service timeout");
            }
            thread::sleep(Duration::from_millis(100));
        }

        status_handle
            .set_service_status(service_process_status(
                ServiceState::Running,
                ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
                Duration::default(),
            ))
            .map_err(|err| anyhow::anyhow!("report listener service running: {err}"))?;
        let result = done_rx
            .recv()
            .map_err(|err| anyhow::anyhow!("wait for listener service worker: {err}"))?;
        status_handle
            .set_service_status(service_process_status(
                ServiceState::Stopped,
                ServiceControlAccept::empty(),
                Duration::default(),
            ))
            .map_err(|err| anyhow::anyhow!("report listener service stopped: {err}"))?;
        result
    }

    fn service_info(resolved: &Resolved, user_sid: &str) -> anyhow::Result<ServiceInfo> {
        Ok(ServiceInfo {
            name: OsString::from(service_name_for(resolved)),
            display_name: OsString::from(service_display_name_for(resolved)),
            service_type: SERVICE_TYPE,
            start_type: ServiceStartType::AutoStart,
            error_control: ServiceErrorControl::Normal,
            executable_path: std::env::current_exe()
                .map_err(|err| anyhow::anyhow!("resolve listener executable: {err}"))?,
            launch_arguments: service_launch_arguments(resolved, user_sid)?
                .into_iter()
                .map(OsString::from)
                .collect(),
            dependencies: Vec::new(),
            account_name: None,
            account_password: None,
        })
    }

    fn service_manager(access: ServiceManagerAccess) -> anyhow::Result<ServiceManager> {
        ServiceManager::local_computer(None::<&str>, access)
            .map_err(|err| anyhow::anyhow!("connect to Windows Service Control Manager: {err}"))
    }

    fn current_process_user_sid() -> anyhow::Result<String> {
        use std::os::windows::ffi::OsStringExt;
        use windows_sys::Win32::Foundation::{CloseHandle, LocalFree};
        use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
        use windows_sys::Win32::Security::{
            GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER,
        };
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

        let mut token = 0;
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return Err(anyhow::anyhow!(
                "open current process token: {}",
                std::io::Error::last_os_error()
            ));
        }

        let result = (|| {
            let mut required = 0_u32;
            unsafe {
                GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut required)
            };
            if required == 0 {
                return Err(anyhow::anyhow!(
                    "measure current process token user: {}",
                    std::io::Error::last_os_error()
                ));
            }
            let words = (required as usize).div_ceil(std::mem::size_of::<usize>());
            let mut buffer = vec![0_usize; words];
            if unsafe {
                GetTokenInformation(
                    token,
                    TokenUser,
                    buffer.as_mut_ptr().cast(),
                    required,
                    &mut required,
                )
            } == 0
            {
                return Err(anyhow::anyhow!(
                    "read current process token user: {}",
                    std::io::Error::last_os_error()
                ));
            }
            let token_user = unsafe { &*(buffer.as_ptr().cast::<TOKEN_USER>()) };
            let mut string_sid = std::ptr::null_mut();
            if unsafe { ConvertSidToStringSidW(token_user.User.Sid, &mut string_sid) } == 0 {
                return Err(anyhow::anyhow!(
                    "format current process user SID: {}",
                    std::io::Error::last_os_error()
                ));
            }
            let sid = unsafe {
                let mut len = 0;
                while *string_sid.add(len) != 0 {
                    len += 1;
                }
                OsString::from_wide(std::slice::from_raw_parts(string_sid, len))
                    .to_string_lossy()
                    .into_owned()
            };
            unsafe { LocalFree(string_sid.cast()) };
            if !valid_user_sid(&sid) {
                anyhow::bail!("current process returned an invalid Windows user SID");
            }
            Ok(sid)
        })();
        unsafe { CloseHandle(token) };
        result
    }

    fn wait_for_scm_state(
        service: &windows_service::service::Service,
        expected: ServiceState,
        timeout: Duration,
    ) -> anyhow::Result<()> {
        let deadline = Instant::now() + timeout;
        loop {
            let state = service
                .query_status()
                .map_err(|err| anyhow::anyhow!("query listener Windows service: {err}"))?
                .current_state;
            if state == expected {
                return Ok(());
            }
            if Instant::now() >= deadline {
                anyhow::bail!(
                    "listener Windows service did not become {} before the timeout",
                    service_state_name(expected)
                );
            }
            thread::sleep(Duration::from_millis(250));
        }
    }

    fn wait_for_listener_status(
        resolved: &Resolved,
        want_running: bool,
        expected_boot_id: &str,
    ) -> anyhow::Result<Status> {
        let wait_for_bridge = want_running
            && crate::host_runtime::resolve(resolved).mode
                == crate::host_runtime::bridge::MODE_WEBSOCKET
            && crate::host_runtime::resolve(resolved).listener.enabled;
        listener_service::wait_for_service_status_with(
            || listener_status(resolved),
            want_running,
            wait_for_bridge,
            expected_boot_id,
            Duration::from_secs(15),
            Duration::from_millis(250),
        )
    }

    fn service_process_status(
        state: ServiceState,
        controls: ServiceControlAccept,
        wait_hint: Duration,
    ) -> ServiceStatus {
        ServiceStatus {
            service_type: SERVICE_TYPE,
            current_state: state,
            controls_accepted: controls,
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint,
            process_id: None,
        }
    }

    fn service_state_name(state: ServiceState) -> &'static str {
        match state {
            ServiceState::Stopped => "stopped",
            ServiceState::StartPending => "start-pending",
            ServiceState::StopPending => "stop-pending",
            ServiceState::Running => "running",
            ServiceState::ContinuePending => "continue-pending",
            ServiceState::PausePending => "pause-pending",
            ServiceState::Paused => "paused",
        }
    }

    fn is_not_installed(err: &windows_service::Error) -> bool {
        matches!(
            err,
            windows_service::Error::Winapi(error)
                if error.raw_os_error()
                    == Some(windows_sys::Win32::Foundation::ERROR_SERVICE_DOES_NOT_EXIST as i32)
        )
    }

    fn write_service_error(message: &str) {
        let resolved = SERVICE_CONTEXT
            .get()
            .and_then(|context| context.lock().ok())
            .and_then(|resolved| resolved.clone());
        let Some(resolved) = resolved else {
            return;
        };
        let path = std::path::Path::new(&resolved.paths.logs_dir).join("listener.service.err.log");
        let _ = fs::create_dir_all(&resolved.paths.logs_dir);
        let _ = fs::write(path, format!("{message}\n"));
    }
}

#[cfg(windows)]
pub use scm::{install, listener_status, restart, run_dispatcher, start, status, stop, uninstall};

pub fn status_result_from_state(state: &str) -> ListenerPlatformStatusResult {
    let status = parse_status_state(state);
    if !status.installed {
        return ListenerPlatformStatusResult::ErrNotInstalled;
    }
    if status.running {
        ListenerPlatformStatusResult::Running
    } else {
        ListenerPlatformStatusResult::NotRunning
    }
}

pub fn parse_status_state(state: &str) -> WindowsServiceStatus {
    let normalized = normalize_state(state);
    let installed = !is_not_installed_state(&normalized);
    let running = matches!(
        normalized.as_str(),
        "running" | "start-pending" | "continue-pending"
    );
    WindowsServiceStatus {
        installed,
        running,
        state: normalized,
    }
}

fn normalize_state(state: &str) -> String {
    let state = extract_state_value(state).unwrap_or(state);
    let normalized = state
        .trim()
        .trim_matches('"')
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .fold(String::new(), |mut output, ch| {
            if ch == '-' {
                if !output.ends_with('-') {
                    output.push(ch);
                }
            } else {
                output.push(ch);
            }
            output
        })
        .trim_matches('-')
        .to_string();
    trim_state_prefixes(&normalized)
}

fn extract_state_value(input: &str) -> Option<&str> {
    for line in input.lines() {
        let (key, value) = line.split_once(':')?;
        let key = normalize_state(key);
        if matches!(key.as_str(), "state" | "status" | "current-state") {
            return Some(value.split('(').next().unwrap_or(value).trim());
        }
    }
    None
}

fn trim_state_prefixes(state: &str) -> String {
    let without_number = state.trim_start_matches(|ch: char| ch.is_ascii_digit() || ch == '-');
    let without_number = without_number
        .strip_prefix("state-")
        .unwrap_or(without_number);
    let without_number = without_number
        .strip_prefix("status-")
        .unwrap_or(without_number);
    without_number
        .strip_prefix("service-")
        .unwrap_or(without_number)
        .to_string()
}

fn is_not_installed_state(state: &str) -> bool {
    matches!(
        state,
        "" | "not-found" | "notinstalled" | "not-installed" | "missing"
    ) || state.contains("does-not-exist")
        || state.contains("not-installed")
}

fn config_string(plan: &ListenerServiceConfigPlan, key: &str) -> Option<String> {
    match plan.options.get(key) {
        Some(ListenerServiceConfigValue::String(value)) => Some(value.clone()),
        Some(ListenerServiceConfigValue::Bool(_)) | None => None,
    }
}

fn config_bool(plan: &ListenerServiceConfigPlan, key: &str) -> Option<bool> {
    match plan.options.get(key) {
        Some(ListenerServiceConfigValue::Bool(value)) => Some(*value),
        Some(ListenerServiceConfigValue::String(_)) | None => None,
    }
}
