use super::App;
use crate::output::ExitError;
use crate::update::{self, Decision};
use serde_json::{json, Value};
use std::process::Command;

const NPM_MIRROR_REGISTRY_URL: &str = "https://registry.npmmirror.com";

impl App {
    pub fn run_upgrade(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let outcome = update::check_fresh(&resolved);
        let (mut data, summary, mut warnings) =
            build_upgrade_status(&outcome.decision, outcome.error.as_deref());
        let mut upgrade_attempted = false;

        if outcome.error.is_none()
            && !self.globals.dry_run
            && (outcome.decision.blocked || outcome.decision.has_newer_version)
        {
            upgrade_attempted = true;
            if let Err(err) = run_npm_global_install() {
                return Err(ExitError::new(
                    "upgrade_failed",
                    1,
                    err,
                    "Ensure npm is installed, your PATH is configured, you have permission to install global packages, and you can reach registry.npmjs.org or registry.npmmirror.com, then retry `awiki-cli upgrade`.",
                ));
            }
            warnings.push("Attempted to upgrade via npm with registry.npmjs.org first and registry.npmmirror.com fallback. Open a new shell and run `awiki-cli version` to verify.".to_string());
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
        "upgrade_hint": direct_upgrade_hint(),
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
                format!("Failed to fetch npm metadata for awiki-cli: {err}"),
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
            "Remote npm registries were unavailable; showing cached update metadata instead."
                .to_string(),
        );
    }

    (data, summary, warnings)
}

fn npm_global_install_attempts() -> Vec<Vec<&'static str>> {
    vec![
        vec!["install", "-g", "@awiki/cli@latest"],
        vec![
            "install",
            "-g",
            "@awiki/cli@latest",
            "--registry=https://registry.npmmirror.com",
        ],
    ]
}

fn format_npm_install_command(args: &[&str]) -> String {
    format!("npm {}", args.join(" "))
}

fn direct_npm_install_command() -> String {
    format_npm_install_command(&npm_global_install_attempts()[0])
}

fn mirror_npm_install_command() -> String {
    format_npm_install_command(&npm_global_install_attempts()[1])
}

fn direct_upgrade_hint() -> String {
    format!(
        "To upgrade awiki-cli, run: {}. If registry.npmjs.org is unreachable, retry with: {}",
        direct_npm_install_command(),
        mirror_npm_install_command()
    )
}

fn run_npm_global_install() -> Result<(), String> {
    let attempts = npm_global_install_attempts();
    let mut errors = Vec::new();
    for (index, args) in attempts.iter().enumerate() {
        if index > 0 {
            eprintln!(
                "[awiki-cli] npm install via registry.npmjs.org failed; retrying with {NPM_MIRROR_REGISTRY_URL}"
            );
        }
        match Command::new("npm").args(args).status() {
            Ok(status) if status.success() => return Ok(()),
            Ok(status) => errors.push(format!("{}: {}", format_npm_install_command(args), status)),
            Err(err) => errors.push(format!("{}: {}", format_npm_install_command(args), err)),
        }
    }
    Err(format!(
        "failed to upgrade awiki-cli via npm registries: {}",
        errors.join("; ")
    ))
}
