use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::DaemonConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServicePlatform {
    LaunchAgent,
    SystemdUser,
    Foreground,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceAction {
    Status,
    Install,
    Start,
    Stop,
    Restart,
    Uninstall,
    RemoveRegistration,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceStatus {
    pub platform: ServicePlatform,
    pub installed: bool,
    pub running: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit_path: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

pub fn manage_service(
    config: &DaemonConfig,
    executable: &Path,
    action: ServiceAction,
) -> Result<ServiceStatus> {
    match platform_manager() {
        ServicePlatform::LaunchAgent => macos::manage(config, executable, action),
        ServicePlatform::SystemdUser => linux::manage(config, executable, action),
        ServicePlatform::Foreground => Ok(ServiceStatus {
            platform: ServicePlatform::Foreground,
            installed: false,
            running: false,
            unit_path: None,
            detail: Some("systemd user environment unavailable; foreground mode required".into()),
        }),
        ServicePlatform::Unsupported => Ok(ServiceStatus {
            platform: ServicePlatform::Unsupported,
            installed: false,
            running: false,
            unit_path: None,
            detail: Some("service manager unsupported on this platform".into()),
        }),
    }
}

pub fn current_platform_label() -> String {
    platform_label_for(std::env::consts::OS, std::env::consts::ARCH)
}

fn platform_label_for(os: &str, arch: &str) -> String {
    let os = match os {
        "macos" | "darwin" => "darwin",
        "linux" => "linux",
        other => other,
    };
    let arch = match arch {
        "x86_64" | "amd64" => "amd64",
        "aarch64" | "arm64" => "arm64",
        other => other,
    };
    format!("{os}-{arch}")
}

pub fn default_executable_path() -> Result<PathBuf> {
    std::env::current_exe().context("resolve awiki-deamon executable path")
}

pub fn product_current_executable_path() -> Result<PathBuf> {
    Ok(DaemonConfig::default_product_state_root()?
        .parent()
        .context("resolve daemon product root")?
        .join("bin")
        .join("current")
        .join("awiki-deamon"))
}

pub fn runtime_env_file_path(config: &DaemonConfig) -> PathBuf {
    let daemon_root = config
        .state_root
        .file_name()
        .filter(|name| *name == OsStr::new("state"))
        .and_then(|_| config.state_root.parent())
        .unwrap_or(&config.state_root);
    daemon_root.join("env").join("agent-cli.env")
}

fn ensure_runtime_env_dir(config: &DaemonConfig) -> Result<()> {
    let env_file = runtime_env_file_path(config);
    let Some(parent) = env_file.parent() else {
        return Ok(());
    };
    std::fs::create_dir_all(parent)
        .with_context(|| format!("create daemon runtime env directory {}", parent.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
            .with_context(|| format!("secure daemon runtime env directory {}", parent.display()))?;
    }
    Ok(())
}

fn platform_manager() -> ServicePlatform {
    if cfg!(target_os = "macos") {
        ServicePlatform::LaunchAgent
    } else if cfg!(target_os = "linux") {
        if systemd_user_available() {
            ServicePlatform::SystemdUser
        } else {
            ServicePlatform::Foreground
        }
    } else {
        ServicePlatform::Unsupported
    }
}

fn command_available(command: &str) -> bool {
    std::process::Command::new(command)
        .arg("--version")
        .output()
        .is_ok()
}

fn systemd_user_available() -> bool {
    if !command_available("systemctl") {
        return false;
    }
    std::process::Command::new("systemctl")
        .args(["--user", "show-environment"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn ready_file(config: &DaemonConfig) -> PathBuf {
    config.state_root.join("run").join("ready.json")
}

fn ensure_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create service directory {}", parent.display()))?;
    }
    Ok(())
}

fn write_if_changed(path: &Path, content: &str) -> Result<()> {
    ensure_parent(path)?;
    if std::fs::read_to_string(path).ok().as_deref() == Some(content) {
        return Ok(());
    }
    std::fs::write(path, content).with_context(|| format!("write {}", path.display()))
}

fn run_status(command: &mut std::process::Command) -> Result<bool> {
    Ok(command
        .status()
        .map(|status| status.success())
        .unwrap_or(false))
}

fn compact_command_error(command: &str, output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let message = if !stderr.is_empty() { stderr } else { stdout };
    let base = if message.is_empty() {
        format!("{command} exited with {}", output.status)
    } else {
        format!("{command}: {message}")
    };
    compact_detail(&base)
}

fn compact_detail(value: &str) -> String {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    const LIMIT: usize = 240;
    if normalized.chars().count() <= LIMIT {
        normalized
    } else {
        let mut truncated = normalized.chars().take(LIMIT).collect::<String>();
        truncated.push_str("...");
        truncated
    }
}

fn current_user_name() -> Option<String> {
    for key in ["USER", "LOGNAME"] {
        if let Some(value) = std::env::var_os(key)
            .and_then(|value| value.into_string().ok())
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        {
            return Some(value);
        }
    }

    std::process::Command::new("id")
        .arg("-un")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .context("HOME is required for user service management")
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn shell_escape_systemd(value: &Path) -> String {
    value
        .display()
        .to_string()
        .replace('\\', "\\\\")
        .replace(' ', "\\x20")
        .replace('%', "%%")
}

fn systemd_environment_escape(value: &OsStr) -> String {
    value
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('%', "%%")
}

fn shell_single_quote_path(value: &Path) -> String {
    let escaped = value.display().to_string().replace('\'', "'\\''");
    format!("'{escaped}'")
}

pub(crate) mod macos {
    use super::*;

    pub const LABEL: &str = "ai.awiki.deamon";

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

    pub fn manage(
        config: &DaemonConfig,
        executable: &Path,
        action: ServiceAction,
    ) -> Result<ServiceStatus> {
        let path = launch_agent_path()?;
        let domain = format!("gui/{}", unsafe { libc::getuid() });
        match action {
            ServiceAction::Install => {
                ensure_runtime_env_dir(config)?;
                write_if_changed(&path, &plist_content(config, executable))?;
                let _ = run_status(
                    std::process::Command::new("launchctl")
                        .arg("bootstrap")
                        .arg(&domain)
                        .arg(&path),
                )?;
                let _ = run_status(
                    std::process::Command::new("launchctl")
                        .arg("kickstart")
                        .arg("-k")
                        .arg(format!("{domain}/{LABEL}")),
                )?;
            }
            ServiceAction::Start | ServiceAction::Restart => {
                let _ = run_status(
                    std::process::Command::new("launchctl")
                        .arg("kickstart")
                        .arg("-k")
                        .arg(format!("{domain}/{LABEL}")),
                )?;
            }
            ServiceAction::Stop => {
                let _ = run_status(
                    std::process::Command::new("launchctl")
                        .arg("bootout")
                        .arg(&domain)
                        .arg(&path),
                )?;
            }
            ServiceAction::Uninstall => {
                let _ = run_status(
                    std::process::Command::new("launchctl")
                        .arg("bootout")
                        .arg(&domain)
                        .arg(&path),
                )?;
                if path.exists() {
                    std::fs::remove_file(&path)
                        .with_context(|| format!("remove LaunchAgent {}", path.display()))?;
                }
            }
            ServiceAction::RemoveRegistration => {
                let _ = run_status(
                    std::process::Command::new("launchctl")
                        .arg("disable")
                        .arg(format!("{domain}/{LABEL}")),
                )?;
                if path.exists() {
                    std::fs::remove_file(&path)
                        .with_context(|| format!("remove LaunchAgent {}", path.display()))?;
                }
            }
            ServiceAction::Status => {}
        }
        let running = run_status(
            std::process::Command::new("launchctl")
                .arg("print")
                .arg(format!("{domain}/{LABEL}")),
        )?;
        Ok(ServiceStatus {
            platform: ServicePlatform::LaunchAgent,
            installed: path.exists(),
            running,
            unit_path: Some(path),
            detail: None,
        })
    }
}

pub(crate) mod linux {
    use super::*;

    pub const UNIT_NAME: &str = "awiki-deamon.service";

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(super) enum LingerState {
        Enabled,
        Disabled,
        Unknown,
    }

    pub fn unit_path() -> Result<PathBuf> {
        Ok(home_dir()?
            .join(".config")
            .join("systemd")
            .join("user")
            .join(UNIT_NAME))
    }

    pub fn unit_content(config: &DaemonConfig, executable: &Path) -> String {
        unit_content_with_path_and_env_file(
            config,
            executable,
            std::env::var_os("PATH").as_deref(),
            &runtime_env_file_path(config),
        )
    }

    #[cfg(test)]
    pub(super) fn unit_content_with_path(
        config: &DaemonConfig,
        executable: &Path,
        path: Option<&OsStr>,
    ) -> String {
        unit_content_with_path_and_env_file(
            config,
            executable,
            path,
            &runtime_env_file_path(config),
        )
    }

    pub(super) fn unit_content_with_path_and_env_file(
        config: &DaemonConfig,
        executable: &Path,
        path: Option<&OsStr>,
        env_file: &Path,
    ) -> String {
        let environment = path
            .filter(|value| !value.is_empty())
            .map(|value| {
                format!(
                    "Environment=\"PATH={}\"\n",
                    systemd_environment_escape(value)
                )
            })
            .unwrap_or_default();
        let environment_file = format!("EnvironmentFile=-{}\n", shell_escape_systemd(env_file));
        format!(
            r#"[Unit]
Description=Awiki Daemon Agent Runtime Host

[Service]
{}{}
ExecStart={} foreground --state-root {} --ready-file {}
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
"#,
            environment,
            environment_file,
            shell_escape_systemd(executable),
            shell_escape_systemd(&config.state_root),
            shell_escape_systemd(&ready_file(config)),
        )
    }

    pub(super) fn parse_linger_state(raw: &str) -> LingerState {
        for line in raw.lines() {
            let value = line
                .trim()
                .strip_prefix("Linger=")
                .unwrap_or_else(|| line.trim())
                .trim()
                .to_ascii_lowercase();
            match value.as_str() {
                "yes" | "true" | "1" => return LingerState::Enabled,
                "no" | "false" | "0" => return LingerState::Disabled,
                _ => {}
            }
        }
        LingerState::Unknown
    }

    fn current_user_linger_state() -> LingerState {
        let Some(user) = current_user_name() else {
            return LingerState::Unknown;
        };
        std::process::Command::new("loginctl")
            .args(["show-user", &user, "-p", "Linger"])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| parse_linger_state(&String::from_utf8_lossy(&output.stdout)))
            .unwrap_or(LingerState::Unknown)
    }

    fn enable_linger_for_current_user() -> Option<String> {
        if current_user_linger_state() == LingerState::Enabled {
            return None;
        }
        let Some(user) = current_user_name() else {
            return Some("unable to resolve current user for login linger".to_string());
        };
        match std::process::Command::new("loginctl")
            .env("SYSTEMD_ASK_PASSWORD", "0")
            .args(["enable-linger", &user])
            .output()
        {
            Ok(output) if output.status.success() => None,
            Ok(output) => Some(compact_command_error("loginctl enable-linger", &output)),
            Err(error) => Some(compact_detail(&format!(
                "loginctl enable-linger unavailable: {error}"
            ))),
        }
    }

    pub(super) fn service_detail(
        installed: bool,
        linger_state: LingerState,
        enable_linger_error: Option<&str>,
    ) -> Option<String> {
        if !installed {
            return enable_linger_error.map(compact_detail);
        }

        let base = match linger_state {
            LingerState::Enabled => return None,
            LingerState::Disabled => {
                "systemd user service is enabled, but login linger is disabled; daemon starts after user login, not after unattended reboot"
            }
            LingerState::Unknown => {
                "systemd user service is enabled, but login linger could not be verified; unattended reboot autostart may require loginctl enable-linger"
            }
        };

        Some(match enable_linger_error {
            Some(error) if !error.trim().is_empty() => {
                compact_detail(&format!("{base}; automatic enable-linger failed: {error}"))
            }
            _ => base.to_string(),
        })
    }

    pub fn manage(
        config: &DaemonConfig,
        executable: &Path,
        action: ServiceAction,
    ) -> Result<ServiceStatus> {
        if !systemd_user_available() {
            return Ok(ServiceStatus {
                platform: ServicePlatform::Foreground,
                installed: false,
                running: false,
                unit_path: None,
                detail: Some(
                    "systemd user environment unavailable; foreground mode required".into(),
                ),
            });
        }
        let path = unit_path()?;
        let mut enable_linger_error = None;
        match action {
            ServiceAction::Install => {
                ensure_runtime_env_dir(config)?;
                write_if_changed(&path, &unit_content(config, executable))?;
                let _ = run_status(
                    std::process::Command::new("systemctl").args(["--user", "daemon-reload"]),
                )?;
                let _ = run_status(
                    std::process::Command::new("systemctl").args(["--user", "enable", UNIT_NAME]),
                )?;
                enable_linger_error = enable_linger_for_current_user();
                let _ = run_status(
                    std::process::Command::new("systemctl").args(["--user", "restart", UNIT_NAME]),
                )?;
            }
            ServiceAction::Start => {
                let _ = run_status(
                    std::process::Command::new("systemctl").args(["--user", "start", UNIT_NAME]),
                )?;
            }
            ServiceAction::Stop => {
                let _ = run_status(
                    std::process::Command::new("systemctl").args(["--user", "stop", UNIT_NAME]),
                )?;
            }
            ServiceAction::Restart => {
                let _ = run_status(
                    std::process::Command::new("systemctl").args(["--user", "restart", UNIT_NAME]),
                )?;
            }
            ServiceAction::Uninstall => {
                let _ = run_status(
                    std::process::Command::new("systemctl")
                        .args(["--user", "disable", "--now", UNIT_NAME]),
                )?;
                if path.exists() {
                    std::fs::remove_file(&path)
                        .with_context(|| format!("remove systemd unit {}", path.display()))?;
                }
                let _ = run_status(
                    std::process::Command::new("systemctl").args(["--user", "daemon-reload"]),
                )?;
            }
            ServiceAction::RemoveRegistration => {
                let _ = run_status(
                    std::process::Command::new("systemctl").args(["--user", "disable", UNIT_NAME]),
                )?;
                if path.exists() {
                    std::fs::remove_file(&path)
                        .with_context(|| format!("remove systemd unit {}", path.display()))?;
                }
                let _ = run_status(
                    std::process::Command::new("systemctl").args(["--user", "daemon-reload"]),
                )?;
                let _ = run_status(std::process::Command::new("systemctl").args([
                    "--user",
                    "reset-failed",
                    UNIT_NAME,
                ]))?;
            }
            ServiceAction::Status => {}
        }
        let installed = path.exists();
        let running = run_status(std::process::Command::new("systemctl").args([
            "--user",
            "is-active",
            "--quiet",
            UNIT_NAME,
        ]))?;
        let linger_state = current_user_linger_state();
        let detail = service_detail(installed, linger_state, enable_linger_error.as_deref());
        Ok(ServiceStatus {
            platform: ServicePlatform::SystemdUser,
            installed,
            running,
            unit_path: Some(path),
            detail,
        })
    }
}

pub fn require_service_state_root_is_product(config: &DaemonConfig) -> Result<()> {
    let product_root = DaemonConfig::default_product_state_root()?;
    if config.state_root != product_root {
        bail!(
            "service mode supports only the product state root {}; use --foreground or --no-service for custom --state-root",
            product_root.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_label_uses_release_contract_names() {
        assert_eq!(platform_label_for("macos", "aarch64"), "darwin-arm64");
        assert_eq!(platform_label_for("darwin", "x86_64"), "darwin-amd64");
        assert_eq!(platform_label_for("linux", "aarch64"), "linux-arm64");
        assert_eq!(platform_label_for("linux", "x86_64"), "linux-amd64");
    }

    #[test]
    fn service_units_run_foreground_with_ready_file() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        let executable = root.path().join("bin").join("awiki-deamon");

        let plist = macos::plist_content(&config, &executable);
        assert!(plist.contains("<string>/bin/sh</string>"));
        assert!(plist.contains("agent-cli.env"));
        assert!(plist.contains("exec "));
        assert!(plist.contains("foreground --state-root"));
        assert!(plist.contains("--ready-file"));
        assert!(plist.contains("daemon.stdout.log"));
        assert!(plist.contains("daemon.stderr.log"));

        let unit = linux::unit_content(&config, &executable);
        assert!(unit.contains("EnvironmentFile=-"));
        assert!(unit.contains("agent-cli.env"));
        assert!(unit.contains("foreground --state-root"));
        assert!(unit.contains("--ready-file"));
        assert!(unit.contains("Restart=on-failure"));

        let env_file = root.path().join("runtime env").join("agent%cli.env");
        let unit = linux::unit_content_with_path(
            &config,
            &executable,
            Some(OsStr::new(r#"/home/alice/.nvm/bin:/tmp/a"b:%p"#)),
        );
        assert!(unit.contains(r#"Environment="PATH=/home/alice/.nvm/bin:/tmp/a\"b:%%p""#));

        let unit =
            linux::unit_content_with_path_and_env_file(&config, &executable, None, &env_file);
        assert!(unit.contains("EnvironmentFile=-"));
        assert!(unit.contains("runtime\\x20env"));
        assert!(unit.contains("agent%%cli.env"));
    }

    #[test]
    fn runtime_env_file_uses_daemon_product_root_when_state_root_is_named_state() {
        let root = tempfile::tempdir().unwrap();
        let state_root = root.path().join("deamon").join("state");
        let config = DaemonConfig::for_state_root(&state_root).unwrap();

        assert_eq!(
            runtime_env_file_path(&config),
            root.path().join("deamon").join("env").join("agent-cli.env")
        );
    }

    #[test]
    fn service_mode_rejects_custom_state_root() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();

        let error = require_service_state_root_is_product(&config).unwrap_err();

        assert!(error.to_string().contains("product state root"));
        assert!(error.to_string().contains("--foreground"));
    }

    #[test]
    fn linux_linger_state_parser_accepts_loginctl_show_user_output() {
        assert_eq!(
            linux::parse_linger_state("Linger=yes\n"),
            linux::LingerState::Enabled
        );
        assert_eq!(
            linux::parse_linger_state("Linger=no\n"),
            linux::LingerState::Disabled
        );
        assert_eq!(
            linux::parse_linger_state("no\n"),
            linux::LingerState::Disabled
        );
        assert_eq!(
            linux::parse_linger_state("unexpected\n"),
            linux::LingerState::Unknown
        );
    }

    #[test]
    fn linux_service_detail_warns_when_linger_is_not_ready() {
        assert!(linux::service_detail(true, linux::LingerState::Enabled, None).is_none());

        let disabled = linux::service_detail(
            true,
            linux::LingerState::Disabled,
            Some("permission denied"),
        )
        .unwrap();
        assert!(disabled.contains("login linger is disabled"));
        assert!(disabled.contains("automatic enable-linger failed"));

        let unknown = linux::service_detail(true, linux::LingerState::Unknown, None).unwrap();
        assert!(unknown.contains("could not be verified"));
    }
}
