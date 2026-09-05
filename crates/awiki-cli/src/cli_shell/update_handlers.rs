use super::App;
use crate::cli_http::{new_http_client_with_proxy_env, HttpRequest};
use crate::cli_output::ExitError;
use crate::self_update::{self, Decision};
use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256, Sha512};
use std::fs;
use std::path::{Path, PathBuf};
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
            let package = download_verified_installer(&outcome.decision, &resolved.paths.cache_dir)
                .map_err(|err| {
                    ExitError::new(
                        "upgrade_failed",
                        1,
                        err,
                        "Verify the selected tenant's release policy and retry the same tenant-scoped upgrade command.",
                    )
                })?;
            let install_result = run_npm_global_install(&package);
            let _ = fs::remove_file(&package);
            if let Err(err) = install_result {
                return Err(ExitError::new(
                    "upgrade_failed",
                    1,
                    err,
                    "Ensure npm is installed, your PATH is configured, and you have permission to install global packages, then retry the same tenant-scoped upgrade command.",
                ));
            }
            warnings.push(format!(
                "Installed awiki-cli {} from the policy published by tenant {}. Open a new shell and run `awiki-cli version` to verify.",
                outcome.decision.latest_version, outcome.decision.tenant_alias
            ));
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
        "tenant": decision.tenant_alias,
        "policy_origin": decision.policy_origin,
        "policy_revision": decision.policy_revision,
        "current_version": decision.current_version,
        "latest_version": decision.latest_version,
        "min_supported_version": decision.min_supported_version,
        "installer_url": decision.installer_url,
        "installer_mirrors": decision.installer_mirrors,
        "installer_sha256": decision.installer_sha256,
        "installer_size": decision.installer_size,
        "installer_integrity": decision.installer_integrity,
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
            format!(
                "Unable to check for awiki-cli updates for tenant {}",
                decision.tenant_alias
            ),
            vec![
                format!(
                    "Failed to fetch the awiki-cli release manifest for tenant {}: {err}",
                    decision.tenant_alias
                ),
                "No other tenant's update policy was used. Retry when this tenant's policy origin is available."
                    .to_string(),
            ],
        );
    }

    data["update_check_status"] = if decision.metadata_source == "cache_stale" {
        json!("stale_cache")
    } else {
        json!("ok")
    };

    let mut summary = format!(
        "awiki-cli is up to date for tenant {}",
        decision.tenant_alias
    );
    let mut warnings = Vec::new();
    if decision.blocked {
        summary = format!(
            "awiki-cli is below the minimum supported version for tenant {}",
            decision.tenant_alias
        );
        warnings.push(format!(
            "awiki-cli {} is below tenant {}'s minimum supported version {}. Upgrading is required before using that tenant's remote APIs.",
            decision.current_version, decision.tenant_alias, decision.min_supported_version
        ));
    } else if decision.has_newer_version {
        summary = format!(
            "awiki-cli {} is available for tenant {}",
            decision.latest_version, decision.tenant_alias
        );
        warnings.push("Upgrading is recommended to stay on a supported version.".to_string());
    }
    if decision.metadata_source == "cache_stale" {
        warnings.push(format!(
            "Tenant {}'s release server was unavailable; showing only that tenant's cached update metadata.",
            decision.tenant_alias
        ));
    }

    (data, summary, warnings)
}

fn direct_upgrade_hint(installer_url: &str) -> String {
    format!(
        "To upgrade awiki-cli, run: {}",
        super::update_preflight::npm_install_command(installer_url)
    )
}

fn download_verified_installer(decision: &Decision, cache_dir: &str) -> Result<PathBuf, String> {
    if decision.installer_sha256.len() != 64 || decision.installer_size == 0 {
        return Err(
            "release manifest must provide installer sha256 and positive size before upgrade"
                .to_string(),
        );
    }
    let client = new_http_client_with_proxy_env("").map_err(|err| err.to_string())?;
    let mut errors = Vec::new();
    for url in std::iter::once(&decision.installer_url).chain(decision.installer_mirrors.iter()) {
        if let Err(err) = validate_installer_url(url) {
            errors.push(format!("{url}: {err}"));
            continue;
        }
        let response = match client.execute(HttpRequest::new("GET", url)) {
            Ok(response) => response,
            Err(err) => {
                errors.push(format!("{url}: {err}"));
                continue;
            }
        };
        if response.status_code != 200 {
            errors.push(format!("{url}: status {}", response.status_code));
            continue;
        }
        if response.body.len() as u64 != decision.installer_size {
            errors.push(format!("{url}: installer size mismatch"));
            continue;
        }
        let actual_sha256 = format!("{:x}", Sha256::digest(&response.body));
        if !actual_sha256.eq_ignore_ascii_case(&decision.installer_sha256) {
            errors.push(format!("{url}: installer SHA-256 mismatch"));
            continue;
        }
        verify_npm_integrity(&response.body, &decision.installer_integrity)?;
        let base = if cache_dir.trim().is_empty() {
            std::env::temp_dir()
        } else {
            PathBuf::from(cache_dir)
        };
        let directory = base.join("update").join("downloads");
        fs::create_dir_all(&directory).map_err(|err| err.to_string())?;
        let path = directory.join(format!(
            "awiki-cli-{}-{}.tgz",
            decision.latest_version,
            std::process::id()
        ));
        fs::write(&path, &response.body).map_err(|err| err.to_string())?;
        return Ok(path);
    }
    Err(format!(
        "all policy-approved installer mirrors failed: {}",
        errors.join("; ")
    ))
}

fn verify_npm_integrity(bytes: &[u8], integrity: &str) -> Result<(), String> {
    let integrity = integrity.trim();
    if integrity.is_empty() {
        return Ok(());
    }
    let encoded = integrity
        .strip_prefix("sha512-")
        .ok_or_else(|| "only sha512 npm integrity values are supported".to_string())?;
    let expected = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| "invalid npm integrity encoding".to_string())?;
    let actual = Sha512::digest(bytes);
    if actual.as_slice() != expected.as_slice() {
        return Err("installer npm integrity mismatch".to_string());
    }
    Ok(())
}

fn validate_installer_url(installer_url: &str) -> Result<(), String> {
    let value = installer_url.trim();
    if value.starts_with("https://")
        && !value["https://".len()..].is_empty()
        && !value.chars().any(char::is_whitespace)
    {
        return Ok(());
    }
    if cfg!(debug_assertions)
        && (value.starts_with("http://localhost:")
            || value.starts_with("http://127.0.0.1:")
            || value.starts_with("http://[::1]:"))
    {
        return Ok(());
    }
    Err("release manifest installer URL must use HTTPS".to_string())
}

fn run_npm_global_install(installer_path: &Path) -> Result<(), String> {
    let display = installer_path.display().to_string();
    let args = ["install", "-g", display.as_str()];
    match Command::new("npm").args(args).status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("npm install -g {display}: {status}")),
        Err(err) => Err(format!("npm install -g {display}: {err}")),
    }
}

#[cfg(test)]
mod tests {
    use super::{validate_installer_url, verify_npm_integrity};
    use base64::Engine;
    use sha2::{Digest, Sha512};

    #[test]
    fn upgrade_rejects_non_https_installer_before_download() {
        let error = validate_installer_url("http://downloads.example/awiki-cli.tgz")
            .expect_err("HTTP installer must be rejected");
        assert!(error.contains("must use HTTPS"));
    }

    #[test]
    fn npm_integrity_is_verified() {
        let bytes = b"tenant-scoped-package";
        let integrity = format!(
            "sha512-{}",
            base64::engine::general_purpose::STANDARD.encode(Sha512::digest(bytes))
        );
        verify_npm_integrity(bytes, &integrity).expect("valid integrity");
        assert!(verify_npm_integrity(b"other", &integrity).is_err());
    }
}
