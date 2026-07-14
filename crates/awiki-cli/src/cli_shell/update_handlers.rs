use super::App;
use crate::cli_output::ExitError;
use crate::self_update::{self, Decision};
use serde_json::{json, Value};
use std::process::Command;

impl App {
    pub fn run_upgrade(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let outcome = self_update::check_fresh(&resolved);
        let (mut data, summary, mut warnings) =
            build_upgrade_status(&outcome.decision, outcome.error.as_deref());
        let mut upgrade_attempted = false;

        if outcome.error.is_none()
            && !self.globals.dry_run
            && (outcome.decision.blocked || outcome.decision.has_newer_version)
        {
            upgrade_attempted = true;
            if let Err(err) = run_npm_global_install(&outcome.decision.installer_url) {
                return Err(ExitError::new(
                    "upgrade_failed",
                    1,
                    err,
                    "Ensure npm is installed, your PATH is configured, you have permission to install global packages, and the configured awiki-cli release server is reachable, then retry `awiki-cli upgrade`.",
                ));
            }
            warnings.push("Installed the current channel tgz from the configured awiki-cli release server. Open a new shell and run `awiki-cli version` to verify.".to_string());
        }
        data["upgrade_attempted"] = json!(upgrade_attempted);

        self.render_success("awiki-cli upgrade", &resolved, data, &summary, warnings)
    }
}

fn build_upgrade_status(
    decision: &Decision,
    check_error: Option<&str>,
) -> (Value, String, Vec<String>) {
    let mut data = json!({
        "current_version": decision.current_version,
        "latest_version": decision.latest_version,
        "min_supported_version": decision.min_supported_version,
        "strict_disabled": decision.strict_disabled,
        "dev_build": decision.dev_build,
        "has_newer_version": decision.has_newer_version,
        "blocked": decision.blocked,
        "upgrade_hint": direct_upgrade_hint(&decision.installer_url),
    });
    if !decision.metadata_source.is_empty() {
        data["update_metadata_source"] = json!(decision.metadata_source);
    }

    if let Some(err) = check_error {
        data["update_check_status"] = json!("unavailable");
        data["update_check_error"] = json!(err);
        return (
            data,
            "Unable to check for awiki-cli updates".to_string(),
            vec![
                format!("Failed to fetch the awiki-cli release manifest: {err}"),
                "Showing local version status only. Retry `awiki-cli upgrade` when network access is available.".to_string(),
            ],
        );
    }

    data["update_check_status"] = if decision.metadata_source == "cache_stale" {
        json!("stale_cache")
    } else {
        json!("ok")
    };

    let mut summary = "awiki-cli is up to date".to_string();
    let mut warnings = Vec::new();
    if decision.blocked {
        summary = "awiki-cli version is below the minimum supported version".to_string();
        warnings.push(format!(
            "awiki-cli {} is below the minimum supported version {}. Upgrading is required before using remote APIs.",
            decision.current_version, decision.min_supported_version
        ));
    } else if decision.has_newer_version {
        summary = if decision.latest_version.is_empty() {
            "A newer awiki-cli version may be available".to_string()
        } else {
            format!(
                "A newer awiki-cli version ({}) is available",
                decision.latest_version
            )
        };
        warnings.push("Upgrading is recommended to stay on a supported version.".to_string());
    }
    if decision.metadata_source == "cache_stale" {
        warnings.push(
            "The awiki-cli release server was unavailable; showing cached update metadata instead."
                .to_string(),
        );
    }

    (data, summary, warnings)
}

fn direct_upgrade_hint(installer_url: &str) -> String {
    format!(
        "To upgrade awiki-cli, run: {}",
        super::update_preflight::npm_install_command(installer_url)
    )
}

fn run_npm_global_install(installer_url: &str) -> Result<(), String> {
    let installer_url = installer_url.trim();
    if installer_url.is_empty() {
        return Err("release manifest does not provide an installer URL".to_string());
    }
    if !installer_url.starts_with("https://")
        || installer_url["https://".len()..].is_empty()
        || installer_url.chars().any(char::is_whitespace)
    {
        return Err(format!(
            "release manifest installer URL must use HTTPS: {installer_url}"
        ));
    }
    let args = ["install", "-g", installer_url];
    match Command::new("npm").args(args).status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!(
            "{}: {}",
            super::update_preflight::npm_install_command(installer_url),
            status
        )),
        Err(err) => Err(format!(
            "{}: {}",
            super::update_preflight::npm_install_command(installer_url),
            err
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::run_npm_global_install;

    #[test]
    fn upgrade_rejects_non_https_installer_before_invoking_npm() {
        let error = run_npm_global_install("http://downloads.example/awiki-cli.tgz")
            .expect_err("HTTP installer must be rejected");
        assert!(error.contains("must use HTTPS"));
    }
}
