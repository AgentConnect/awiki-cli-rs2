use crate::config::Resolved;
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::listener::{self, Status};

pub const SERVICE_NAME_PREFIX: &str = "awiki-cli-listener";
pub const SERVICE_DISPLAY_NAME_PREFIX: &str = "awiki-cli Listener";
pub const LISTENER_SERVICE_MODE_ENV: &str = "AWIKI_LISTENER_SERVICE_MODE";

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
