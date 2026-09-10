use super::*;

pub const LABEL: &str = "ai.awiki.deamon";

pub fn manage(
    config: &DaemonConfig,
    executable: &Path,
    action: ServiceAction,
) -> Result<ServiceStatus> {
    manage_with(
        config,
        executable,
        action,
        &launch_agent_path()?,
        &format!("gui/{}", unsafe { libc::getuid() }),
        &mut |args| std::process::Command::new("launchctl").args(args).output(),
    )
}

// Tests replace the command boundary, never the current user's launchd state.
fn manage_with(
    config: &DaemonConfig,
    executable: &Path,
    action: ServiceAction,
    path: &Path,
    domain: &str,
    run: &mut impl FnMut(&[&OsStr]) -> std::io::Result<std::process::Output>,
) -> Result<ServiceStatus> {
    let service = format!("{domain}/{LABEL}");
    let service = OsStr::new(&service);
    let loaded = query_service(run, service)?;
    match action {
        ServiceAction::Install => {
            ensure_runtime_env_dir(config)?;
            if loaded.is_some() {
                checked(run, &[OsStr::new("bootout"), service])?;
            }
            write_if_changed(path, &plist_content(config, executable))?;
            clear_ready_file(config)?;
            checked(run, &[OsStr::new("enable"), service])?;
            checked(
                run,
                &[
                    OsStr::new("bootstrap"),
                    OsStr::new(domain),
                    path.as_os_str(),
                ],
            )?;
        }
        ServiceAction::Start | ServiceAction::Restart => {
            checked(run, &[OsStr::new("enable"), service])?;
            if loaded.is_none() {
                if !path.is_file() {
                    bail!("LaunchAgent is not installed; install the service before starting it");
                }
                clear_ready_file(config)?;
                checked(
                    run,
                    &[
                        OsStr::new("bootstrap"),
                        OsStr::new(domain),
                        path.as_os_str(),
                    ],
                )?;
            } else if action == ServiceAction::Restart {
                clear_ready_file(config)?;
                checked(run, &[OsStr::new("kickstart"), OsStr::new("-k"), service])?;
            } else {
                checked(run, &[OsStr::new("kickstart"), service])?;
            }
        }
        ServiceAction::Stop | ServiceAction::Uninstall => {
            if loaded.is_some() {
                checked(run, &[OsStr::new("bootout"), service])?;
            }
            clear_ready_file(config)?;
            if action == ServiceAction::Uninstall && path.exists() {
                std::fs::remove_file(path).context("remove LaunchAgent registration")?;
            }
        }
        ServiceAction::RemoveRegistration => {
            checked(run, &[OsStr::new("disable"), service])?;
            if path.exists() {
                std::fs::remove_file(path).context("remove LaunchAgent registration")?;
            }
        }
        ServiceAction::Status => {}
    }
    let observed = if action == ServiceAction::Status {
        loaded
    } else {
        query_service(run, service)?
    };
    Ok(service_status(config, path, observed.as_deref()))
}

fn checked(
    run: &mut impl FnMut(&[&OsStr]) -> std::io::Result<std::process::Output>,
    args: &[&OsStr],
) -> Result<std::process::Output> {
    let step = format!("launchctl {}", args[0].to_string_lossy());
    let output = run(args).with_context(|| {
        format!("{step} could not run; check launchctl availability in the current user session")
    })?;
    if !output.status.success() {
        bail!("{}; check the current user's LaunchAgent and daemon logs, then retry the failed service action. Existing identity and installation files are retained; do not register again or run as root",
            compact_command_error(&step, &output));
    }
    Ok(output)
}

fn query_service(
    run: &mut impl FnMut(&[&OsStr]) -> std::io::Result<std::process::Output>,
    service: &OsStr,
) -> Result<Option<String>> {
    let output = run(&[OsStr::new("print"), service])
        .context("launchctl print could not run; check launchctl availability")?;
    if output.status.success() {
        return Ok(Some(String::from_utf8_lossy(&output.stdout).into_owned()));
    }
    // 113 can also mean a missing GUI domain: require the exact service diagnostic.
    if output.status.code() == Some(113)
        && String::from_utf8_lossy(&output.stderr)
            .contains(&format!("Could not find service \"{LABEL}\""))
    {
        return Ok(None);
    }
    bail!(
        "{}; check the current user GUI session and service status before retrying",
        compact_command_error("launchctl print", &output)
    )
}

fn clear_ready_file(config: &DaemonConfig) -> Result<()> {
    match std::fs::remove_file(ready_file(config)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).context("clear previous daemon initialization marker"),
    }
}

fn service_status(config: &DaemonConfig, path: &Path, observed: Option<&str>) -> ServiceStatus {
    let mut depth = 0usize;
    let mut state = None;
    let mut pid = None;
    for line in observed.unwrap_or_default().lines().map(str::trim) {
        if depth == 1 {
            if let Some(value) = line.strip_prefix("state = ") {
                state = Some(value);
            }
            if let Some(value) = line.strip_prefix("pid = ") {
                pid = value.parse::<u32>().ok().filter(|pid| *pid > 0);
            }
        }
        if line.ends_with('{') {
            depth += 1;
        }
        if line == "}" {
            depth = depth.saturating_sub(1);
        }
    }
    let running = state == Some("running") && pid.is_some();
    let initialized = running
        && std::fs::read(ready_file(config))
            .ok()
            .and_then(|raw| serde_json::from_slice::<serde_json::Value>(&raw).ok())
            .is_some_and(|ready| {
                ready["ready"] == true
                    && ready["process_id"].as_u64() == pid.map(u64::from)
                    && ready["state_root"].as_str() == config.state_root.to_str()
            });
    let detail = if initialized {
        "service loaded; process running; initialization complete".to_string()
    } else if running {
        "service loaded; process running; initialization not yet confirmed. Identity and installation are retained; check service status and daemon logs later".to_string()
    } else if observed.is_some() {
        format!("service loaded; process not running (state: {}). Startup is not yet confirmed; check service status and daemon logs. Identity and installation are retained", state.unwrap_or("unknown"))
    } else {
        "service not loaded".to_string()
    };
    ServiceStatus {
        platform: ServicePlatform::LaunchAgent,
        installed: path.exists(),
        running,
        unit_path: Some(path.to_path_buf()),
        detail: Some(detail),
    }
}

pub fn restart_after_upgrade(config: &DaemonConfig, executable: &Path) -> Result<ServiceStatus> {
    // Upgrades may originate in this service, so retain kickstart for a loaded job.
    write_if_changed(&launch_agent_path()?, &plist_content(config, executable))?;
    manage(config, executable, ServiceAction::Restart)
}

#[cfg(test)]
#[path = "macos_tests.rs"]
mod tests;

pub fn launch_agent_path() -> Result<PathBuf> {
    Ok(home_dir()?
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{LABEL}.plist")))
}

pub fn plist_content(config: &DaemonConfig, executable: &Path) -> String {
    plist_content_with_env_file(config, executable, &runtime_env_file_path(config))
}

pub(super) fn plist_content_with_env_file(
    config: &DaemonConfig,
    executable: &Path,
    env_file: &Path,
) -> String {
    let stdout = config.state_root.join("logs").join("daemon.stdout.log");
    let stderr = config.state_root.join("logs").join("daemon.stderr.log");
    let command = format!(
        "set -a; [ ! -f {env_file} ] || . {env_file}; set +a; exec {exe} foreground --state-root {state_root} --ready-file {ready_file}",
        env_file = shell_single_quote_path(env_file),
        exe = shell_single_quote_path(executable),
        state_root = shell_single_quote_path(&config.state_root),
        ready_file = shell_single_quote_path(&ready_file(config)),
    );
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{label}</string>
  <key>ProgramArguments</key>
  <array>
<string>/bin/sh</string>
<string>-c</string>
<string>{command}</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardOutPath</key>
  <string>{stdout}</string>
  <key>StandardErrorPath</key>
  <string>{stderr}</string>
</dict>
</plist>
"#,
        label = LABEL,
        command = xml_escape(&command),
        stdout = xml_escape(&stdout.display().to_string()),
        stderr = xml_escape(&stderr.display().to_string()),
    )
}
