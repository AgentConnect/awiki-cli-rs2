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
        let stdout = config.state_root.join("logs").join("daemon.stdout.log");
        let stderr = config.state_root.join("logs").join("daemon.stderr.log");
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{label}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{exe}</string>
    <string>foreground</string>
    <string>--state-root</string>
    <string>{state_root}</string>
    <string>--ready-file</string>
    <string>{ready_file}</string>
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
            exe = xml_escape(&executable.display().to_string()),
            state_root = xml_escape(&config.state_root.display().to_string()),
            ready_file = xml_escape(&ready_file(config).display().to_string()),
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

    pub fn unit_path() -> Result<PathBuf> {
        Ok(home_dir()?
            .join(".config")
            .join("systemd")
            .join("user")
            .join(UNIT_NAME))
    }

    pub fn unit_content(config: &DaemonConfig, executable: &Path) -> String {
        format!(
            r#"[Unit]
Description=Awiki Daemon Agent Runtime Host

[Service]
ExecStart={} foreground --state-root {} --ready-file {}
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
"#,
            shell_escape_systemd(executable),
            shell_escape_systemd(&config.state_root),
            shell_escape_systemd(&ready_file(config)),
        )
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
        match action {
            ServiceAction::Install => {
                write_if_changed(&path, &unit_content(config, executable))?;
                let _ = run_status(
                    std::process::Command::new("systemctl").args(["--user", "daemon-reload"]),
                )?;
                let _ = run_status(
                    std::process::Command::new("systemctl").args(["--user", "enable", UNIT_NAME]),
                )?;
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
            ServiceAction::Status => {}
        }
        let installed = path.exists();
        let running = run_status(std::process::Command::new("systemctl").args([
            "--user",
            "is-active",
            "--quiet",
            UNIT_NAME,
        ]))?;
        Ok(ServiceStatus {
            platform: ServicePlatform::SystemdUser,
            installed,
            running,
            unit_path: Some(path),
            detail: None,
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
        assert!(plist.contains("<string>foreground</string>"));
        assert!(plist.contains("<string>--ready-file</string>"));
        assert!(plist.contains("daemon.stdout.log"));
        assert!(plist.contains("daemon.stderr.log"));

        let unit = linux::unit_content(&config, &executable);
        assert!(unit.contains("foreground --state-root"));
        assert!(unit.contains("--ready-file"));
        assert!(unit.contains("Restart=on-failure"));
    }

    #[test]
    fn service_mode_rejects_custom_state_root() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();

        let error = require_service_state_root_is_product(&config).unwrap_err();

        assert!(error.to_string().contains("product state root"));
        assert!(error.to_string().contains("--foreground"));
    }
}
