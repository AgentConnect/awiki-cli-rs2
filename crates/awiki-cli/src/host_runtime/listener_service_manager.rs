#![allow(unreachable_code)]

use crate::workspace_config::Resolved;
use serde_json::Value;

use super::listener::{self, Status};

pub fn service_platform() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        return super::listener_launchd::service_platform();
    }
    #[cfg(target_os = "linux")]
    {
        return super::listener_systemd::service_platform();
    }
    #[cfg(windows)]
    {
        return super::listener_windows_service::service_platform();
    }
    #[allow(unreachable_code)]
    "unsupported"
}

pub fn status(resolved: &Resolved) -> anyhow::Result<Status> {
    #[cfg(target_os = "macos")]
    {
        return super::listener_launchd::listener_status(resolved);
    }
    #[cfg(target_os = "linux")]
    {
        return super::listener_systemd::listener_status(resolved);
    }
    #[cfg(windows)]
    {
        return super::listener_windows_service::listener_status(resolved);
    }
    anyhow::bail!("listener services are not supported on this operating system")
}

pub fn status_value(resolved: &Resolved) -> Value {
    match status(resolved) {
        Ok(status) => listener::to_value(status),
        Err(err) => unavailable_status_value(resolved, &err),
    }
}

pub fn apply_runtime_policy(resolved: &Resolved) -> anyhow::Result<Status> {
    let runtime = super::resolve(resolved);
    if runtime.mode != super::bridge::MODE_WEBSOCKET || !runtime.listener.enabled {
        return stop(resolved);
    }

    let mut current = if runtime.listener.auto_install {
        install(resolved)?
    } else {
        status(resolved)?
    };
    if runtime.listener.auto_start {
        current = start(resolved)?;
    }
    Ok(current)
}

pub fn install(resolved: &Resolved) -> anyhow::Result<Status> {
    #[cfg(target_os = "macos")]
    {
        return super::listener_launchd::install(resolved);
    }
    #[cfg(target_os = "linux")]
    {
        return super::listener_systemd::install(resolved);
    }
    #[cfg(windows)]
    {
        return super::listener_windows_service::install(resolved);
    }
    anyhow::bail!("listener services are not supported on this operating system")
}

pub fn start(resolved: &Resolved) -> anyhow::Result<Status> {
    #[cfg(target_os = "macos")]
    {
        return super::listener_launchd::start(resolved);
    }
    #[cfg(target_os = "linux")]
    {
        return super::listener_systemd::start(resolved);
    }
    #[cfg(windows)]
    {
        return super::listener_windows_service::start(resolved);
    }
    anyhow::bail!("listener services are not supported on this operating system")
}

pub fn stop(resolved: &Resolved) -> anyhow::Result<Status> {
    #[cfg(target_os = "macos")]
    {
        return super::listener_launchd::stop(resolved);
    }
    #[cfg(target_os = "linux")]
    {
        return super::listener_systemd::stop(resolved);
    }
    #[cfg(windows)]
    {
        return super::listener_windows_service::stop(resolved);
    }
    anyhow::bail!("listener services are not supported on this operating system")
}

pub fn restart(resolved: &Resolved) -> anyhow::Result<Status> {
    #[cfg(target_os = "macos")]
    {
        return super::listener_launchd::restart(resolved);
    }
    #[cfg(target_os = "linux")]
    {
        return super::listener_systemd::restart(resolved);
    }
    #[cfg(windows)]
    {
        return super::listener_windows_service::restart(resolved);
    }
    anyhow::bail!("listener services are not supported on this operating system")
}

pub fn uninstall(resolved: &Resolved) -> anyhow::Result<Status> {
    #[cfg(target_os = "macos")]
    {
        return super::listener_launchd::uninstall(resolved);
    }
    #[cfg(target_os = "linux")]
    {
        return super::listener_systemd::uninstall(resolved);
    }
    #[cfg(windows)]
    {
        return super::listener_windows_service::uninstall(resolved);
    }
    anyhow::bail!("listener services are not supported on this operating system")
}

fn unavailable_status_value(resolved: &Resolved, err: &anyhow::Error) -> Value {
    let mut status = listener::status_for(resolved, false, false, service_platform())
        .unwrap_or_else(|_| Status {
            mode: super::resolve(resolved).mode,
            service_platform: service_platform().to_string(),
            ..Status::default()
        });
    status
        .warnings
        .push(format!("listener service manager is unavailable: {err}"));
    listener::to_value(status)
}
