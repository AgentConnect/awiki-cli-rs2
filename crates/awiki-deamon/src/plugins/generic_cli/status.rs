use serde_json::{json, Value};

use crate::agent::{
    CLAUDE_CODE_CLI_DRIVER_ID, CODEX_CLI_DRIVER_ID, COMMAND_CLI_DRIVER_ID, GEMINI_CLI_DRIVER_ID,
};
use crate::public_error::sanitize_public_error;
use crate::runtime::{RuntimeInstallStatus, RuntimePlugin};
use crate::state::CliRuntimeProfileRecord;

use super::{codex::codex_profile_auth_ready, GenericCliDriverRegistry};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenericCliStatusSummary {
    pub profile_status: String,
    pub driver_id: Option<String>,
    pub binary_installed: bool,
    pub binary_detail: Option<String>,
    pub driver_status_code: String,
    pub auth_status: String,
    pub setup_ready: bool,
    pub setup_status: String,
    pub config_home: Option<String>,
    pub config_home_exists: bool,
    pub default_workspace_mode: Option<String>,
    pub default_sandbox: Option<String>,
}

impl GenericCliStatusSummary {
    pub fn from_profile(profile: CliRuntimeProfileRecord) -> Self {
        let effective_config_home = effective_config_home(&profile);
        let config_home_configured = effective_config_home.is_some();
        let config_home_exists = effective_config_home.is_some_and(|path| path.is_dir());
        let missing_config_home = profile.driver_id == CODEX_CLI_DRIVER_ID && !config_home_exists;
        let auth_status = auth_status(&profile, effective_config_home).to_string();
        let early_status_code = pre_probe_driver_status_code(&profile, missing_config_home);
        let (install_status, driver_status_code) = if let Some(status_code) = early_status_code {
            (
                RuntimeInstallStatus {
                    installed: false,
                    detail: None,
                },
                status_code.to_string(),
            )
        } else {
            let install_probe =
                GenericCliDriverRegistry::new(profile.clone()).check_install_status();
            let install_probe_failed = install_probe.is_err();
            let install_status = install_probe.unwrap_or_else(|error| RuntimeInstallStatus {
                installed: false,
                detail: Some(sanitize_public_error(&error.to_string())),
            });
            let driver_status_code =
                probed_driver_status_code(&install_status, install_probe_failed);
            (install_status, driver_status_code)
        };
        let setup_ready = setup_ready(&driver_status_code, &auth_status);
        let setup_status = setup_status(&driver_status_code, setup_ready, &auth_status);
        Self {
            profile_status: profile.status,
            driver_id: Some(profile.driver_id),
            binary_installed: install_status.installed,
            binary_detail: install_status.detail.as_deref().map(sanitize_public_error),
            driver_status_code,
            auth_status,
            setup_ready,
            setup_status,
            config_home: config_home_configured.then(|| "configured".to_string()),
            config_home_exists,
            default_workspace_mode: Some(profile.default_workspace_mode.as_str().to_string()),
            default_sandbox: profile.default_sandbox,
        }
    }

    pub fn missing(detail: &str) -> Self {
        Self {
            profile_status: "missing".to_string(),
            driver_id: None,
            binary_installed: false,
            binary_detail: Some(detail.to_string()),
            driver_status_code: "profile_missing".to_string(),
            auth_status: "unknown".to_string(),
            setup_ready: false,
            setup_status: "needs_setup".to_string(),
            config_home: None,
            config_home_exists: false,
            default_workspace_mode: None,
            default_sandbox: None,
        }
    }

    pub fn error_code(&self) -> Option<&'static str> {
        match (self.driver_status_code.as_str(), self.auth_status.as_str()) {
            ("profile_missing", _) => Some("generic_cli_profile_missing"),
            ("profile_inactive", _) => Some("generic_cli_profile_inactive"),
            ("config_home_missing", _) => Some("generic_cli_config_home_missing"),
            ("driver_config_missing", _) => Some("generic_cli_driver_config_missing"),
            ("missing_binary", _) => Some("generic_cli_driver_missing"),
            ("probe_failed", _) => Some("generic_cli_driver_probe_failed"),
            ("not_implemented", _) => Some("generic_cli_driver_not_implemented"),
            ("unsupported_driver", _) => Some("generic_cli_driver_unsupported"),
            ("ok", "missing") => Some("generic_cli_auth_missing"),
            ("ok", "unknown") => Some("generic_cli_auth_unknown"),
            _ => None,
        }
    }

    pub fn diagnostics_summary(&self) -> Value {
        json!({
            "profile_status": self.profile_status,
            "driver_id": self.driver_id,
            "config_summary": {
                "binary_installed": self.binary_installed,
                "binary_detail": self.binary_detail,
                "driver_status_code": self.driver_status_code,
                "auth_status": self.auth_status,
                "setup_ready": self.setup_ready,
                "setup_status": self.setup_status,
                "config_home": self.config_home,
                "config_home_exists": self.config_home_exists,
                "default_workspace_mode": self.default_workspace_mode,
                "default_sandbox": self.default_sandbox,
            },
        })
    }
}

fn effective_config_home(profile: &CliRuntimeProfileRecord) -> Option<&std::path::Path> {
    profile.config_home.as_deref().or_else(|| {
        profile
            .driver_config_json
            .get("config_home")
            .and_then(Value::as_str)
            .map(std::path::Path::new)
    })
}

fn auth_status(
    profile: &CliRuntimeProfileRecord,
    effective_config_home: Option<&std::path::Path>,
) -> &'static str {
    match profile.driver_id.as_str() {
        CODEX_CLI_DRIVER_ID => {
            if effective_config_home.is_some_and(codex_profile_auth_ready) {
                "ok"
            } else {
                "missing"
            }
        }
        CLAUDE_CODE_CLI_DRIVER_ID | COMMAND_CLI_DRIVER_ID => "not_applicable",
        _ => "unknown",
    }
}

fn pre_probe_driver_status_code(
    profile: &CliRuntimeProfileRecord,
    missing_config_home: bool,
) -> Option<&'static str> {
    if profile.driver_id == GEMINI_CLI_DRIVER_ID {
        return Some("not_implemented");
    }
    if profile.status != "active" {
        return Some("profile_inactive");
    }
    if !matches!(
        profile.driver_id.as_str(),
        CODEX_CLI_DRIVER_ID | CLAUDE_CODE_CLI_DRIVER_ID | COMMAND_CLI_DRIVER_ID
    ) {
        return Some("unsupported_driver");
    }
    if missing_config_home {
        return Some("config_home_missing");
    }
    if profile.driver_id == COMMAND_CLI_DRIVER_ID && command_program(profile).is_none() {
        return Some("driver_config_missing");
    }
    None
}

fn command_program(profile: &CliRuntimeProfileRecord) -> Option<&std::path::Path> {
    profile.binary_path.as_deref().or_else(|| {
        profile
            .driver_config_json
            .get("program")
            .and_then(Value::as_str)
            .map(std::path::Path::new)
    })
}

fn probed_driver_status_code(
    install_status: &RuntimeInstallStatus,
    install_probe_failed: bool,
) -> String {
    if install_probe_failed {
        return "probe_failed".to_string();
    }
    if !install_status.installed {
        return "missing_binary".to_string();
    }
    "ok".to_string()
}

fn setup_ready(driver_status_code: &str, auth_status: &str) -> bool {
    driver_status_code == "ok" && matches!(auth_status, "ok" | "not_applicable")
}

fn setup_status(driver_status_code: &str, setup_ready: bool, auth_status: &str) -> String {
    if setup_ready {
        return "ready".to_string();
    }
    match driver_status_code {
        "profile_missing" | "config_home_missing" | "driver_config_missing" | "missing_binary" => {
            "needs_setup"
        }
        "profile_inactive" => "inactive",
        "probe_failed" => "probe_failed",
        "not_implemented" | "unsupported_driver" => "unsupported",
        "ok" if auth_status == "missing" => "needs_setup",
        _ => "unknown",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::CliRuntimeProfileRecord;

    #[test]
    fn codex_status_requires_auth_json_without_leaking_paths() {
        let root = tempfile::tempdir().unwrap();
        let binary = root.path().join("codex");
        write_fake_codex_executable(&binary).unwrap();
        let config_home = root.path().join("codex-home");
        std::fs::create_dir_all(&config_home).unwrap();
        let mut profile = CliRuntimeProfileRecord::for_driver("profile_codex", "codex").unwrap();
        profile.binary_path = Some(binary);
        profile.config_home = Some(config_home.clone());

        let missing = GenericCliStatusSummary::from_profile(profile.clone());
        assert!(!missing.setup_ready);
        assert_eq!(missing.error_code(), Some("generic_cli_auth_missing"));
        assert_eq!(
            missing.diagnostics_summary()["config_summary"]["auth_status"],
            "missing"
        );

        std::fs::write(config_home.join("auth.json"), "{}").unwrap();
        let ready = GenericCliStatusSummary::from_profile(profile);
        assert!(ready.setup_ready);
        assert_eq!(ready.error_code(), None);
        assert_eq!(
            ready.diagnostics_summary()["config_summary"]["auth_status"],
            "ok"
        );
        let dump = ready.diagnostics_summary().to_string();
        assert!(!dump.contains(root.path().to_string_lossy().as_ref()));
        assert!(!dump.contains("auth.json"));
    }

    #[test]
    fn codex_status_uses_effective_config_home_from_driver_config() {
        let root = tempfile::tempdir().unwrap();
        let binary = root.path().join("codex");
        write_fake_codex_executable(&binary).unwrap();
        let config_home = root.path().join("codex-home-from-driver-config");
        std::fs::create_dir_all(&config_home).unwrap();
        std::fs::write(config_home.join("auth.json"), "{}").unwrap();
        let mut profile = CliRuntimeProfileRecord::for_driver("profile_codex", "codex").unwrap();
        profile.binary_path = Some(binary);
        profile.driver_config_json = json!({
            "config_home": config_home,
        });

        let ready = GenericCliStatusSummary::from_profile(profile);

        assert!(ready.setup_ready);
        assert_eq!(ready.error_code(), None);
        assert_eq!(ready.driver_status_code, "ok");
        assert_eq!(
            ready.diagnostics_summary()["config_summary"]["auth_status"],
            "ok"
        );
        assert_eq!(
            ready.diagnostics_summary()["config_summary"]["config_home"],
            "configured"
        );
        assert_eq!(
            ready.diagnostics_summary()["config_summary"]["config_home_exists"],
            true
        );
        let dump = ready.diagnostics_summary().to_string();
        assert!(!dump.contains(root.path().to_string_lossy().as_ref()));
        assert!(!dump.contains("auth.json"));
    }

    #[test]
    fn codex_status_requires_nonempty_auth_json_like_runtime_launch() {
        let root = tempfile::tempdir().unwrap();
        let binary = root.path().join("codex");
        write_fake_codex_executable(&binary).unwrap();
        let config_home = root.path().join("codex-home");
        std::fs::create_dir_all(&config_home).unwrap();
        std::fs::write(config_home.join("auth.json"), "").unwrap();
        let mut profile = CliRuntimeProfileRecord::for_driver("profile_codex", "codex").unwrap();
        profile.binary_path = Some(binary);
        profile.config_home = Some(config_home);

        let missing = GenericCliStatusSummary::from_profile(profile);

        assert!(!missing.setup_ready);
        assert_eq!(missing.error_code(), Some("generic_cli_auth_missing"));
        assert_eq!(
            missing.diagnostics_summary()["config_summary"]["auth_status"],
            "missing"
        );
    }

    #[test]
    fn unsupported_driver_reports_stable_status_and_error_code() {
        let profile =
            CliRuntimeProfileRecord::for_driver("profile_unsupported", "unsupported").unwrap();
        let summary = GenericCliStatusSummary::from_profile(profile);

        assert!(!summary.setup_ready);
        assert_eq!(summary.driver_status_code, "unsupported_driver");
        assert_eq!(summary.error_code(), Some("generic_cli_driver_unsupported"));
        assert_eq!(
            summary.diagnostics_summary()["config_summary"]["setup_status"],
            "unsupported"
        );
    }

    #[test]
    fn command_status_reports_missing_program_as_configuration_issue() {
        let profile =
            CliRuntimeProfileRecord::for_driver("profile_command", COMMAND_CLI_DRIVER_ID).unwrap();

        let summary = GenericCliStatusSummary::from_profile(profile);

        assert!(!summary.setup_ready);
        assert_eq!(summary.driver_status_code, "driver_config_missing");
        assert_eq!(
            summary.error_code(),
            Some("generic_cli_driver_config_missing")
        );
        assert_eq!(
            summary.diagnostics_summary()["config_summary"]["setup_status"],
            "needs_setup"
        );
        assert!(summary.binary_detail.is_none());
    }

    #[test]
    fn inactive_profile_is_not_reported_ready_even_when_driver_and_auth_are_valid() {
        let root = tempfile::tempdir().unwrap();
        let binary = root.path().join("codex");
        write_fake_codex_executable(&binary).unwrap();
        let config_home = root.path().join("codex-home");
        std::fs::create_dir_all(&config_home).unwrap();
        std::fs::write(config_home.join("auth.json"), "{}").unwrap();
        let mut profile = CliRuntimeProfileRecord::for_driver("profile_codex", "codex").unwrap();
        profile.binary_path = Some(binary);
        profile.config_home = Some(config_home);
        profile.status = "archived".to_string();

        let summary = GenericCliStatusSummary::from_profile(profile);

        assert!(!summary.setup_ready);
        assert_eq!(summary.profile_status, "archived");
        assert_eq!(summary.driver_status_code, "profile_inactive");
        assert_eq!(summary.error_code(), Some("generic_cli_profile_inactive"));
        assert_eq!(
            summary.diagnostics_summary()["config_summary"]["setup_status"],
            "inactive"
        );
    }

    #[test]
    fn inactive_profile_does_not_probe_binary() {
        let root = tempfile::tempdir().unwrap();
        let binary = root.path().join("codex");
        let config_home = root.path().join("codex-home");
        let marker = root.path().join("probe-marker");
        write_probe_marker_executable(&binary, &marker).unwrap();
        std::fs::create_dir_all(&config_home).unwrap();
        std::fs::write(config_home.join("auth.json"), "{}").unwrap();
        let mut profile = CliRuntimeProfileRecord::for_driver("profile_codex", "codex").unwrap();
        profile.binary_path = Some(binary);
        profile.config_home = Some(config_home);
        profile.status = "archived".to_string();

        let summary = GenericCliStatusSummary::from_profile(profile);

        assert_eq!(summary.driver_status_code, "profile_inactive");
        assert!(!marker.exists());
    }

    #[test]
    fn claude_code_status_uses_install_probe_without_codex_auth_requirement() {
        let root = tempfile::tempdir().unwrap();
        let binary = root.path().join("claude");
        write_fake_version_executable(&binary, "claude 9.9.9").unwrap();
        let mut profile =
            CliRuntimeProfileRecord::for_driver("profile_claude", "claude-code").unwrap();
        profile.binary_path = Some(binary);

        let ready = GenericCliStatusSummary::from_profile(profile);

        assert!(ready.setup_ready);
        assert_eq!(ready.error_code(), None);
        assert_eq!(ready.driver_status_code, "ok");
        assert_eq!(
            ready.diagnostics_summary()["config_summary"]["auth_status"],
            "not_applicable"
        );
        assert_eq!(
            ready.diagnostics_summary()["config_summary"]["setup_status"],
            "ready"
        );
        let dump = ready.diagnostics_summary().to_string();
        assert!(!dump.contains(root.path().to_string_lossy().as_ref()));
    }

    #[test]
    fn public_errors_redact_paths_and_secret_words() {
        let sanitized = sanitize_public_error(
            "spawn /Users/alice/bin/codex failed with token abc and ApiKey xyz",
        );

        assert!(!sanitized.contains("/Users/alice"));
        assert!(!sanitized.contains("token"));
        assert!(!sanitized.contains("ApiKey"));
        assert!(sanitized.contains("<path>"));
        assert!(sanitized.contains("<redacted>"));
    }

    fn write_fake_codex_executable(path: &std::path::Path) -> std::io::Result<()> {
        write_fake_version_executable(path, "codex-cli 9.9.9")
    }

    fn write_probe_marker_executable(
        path: &std::path::Path,
        marker: &std::path::Path,
    ) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(
            path,
            format!(
                r#"#!/bin/sh
touch "{}"
exit 0
"#,
                marker.display()
            ),
        )?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = std::fs::metadata(path)?.permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(path, permissions)?;
        }
        Ok(())
    }

    fn write_fake_version_executable(path: &std::path::Path, version: &str) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(
            path,
            format!(
                r#"#!/bin/sh
if [ "${{1-}}" = "--version" ]; then
  echo "{}"
  exit 0
fi
cat >/dev/null
exit 0
"#,
                version
            ),
        )?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = std::fs::metadata(path)?.permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(path, permissions)?;
        }
        Ok(())
    }
}
