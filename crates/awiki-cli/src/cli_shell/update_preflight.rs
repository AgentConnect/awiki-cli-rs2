use super::App;
use crate::cli_output::{self, ExitError};
use crate::cli_parser::ParsedCommand;
use crate::self_update;
use std::io::{self, IsTerminal, Write};

impl App {
    pub(super) fn preflight(&mut self, command: &ParsedCommand) -> Result<(), ExitError> {
        cli_output::normalize_format(&self.globals.format).map_err(|err| {
            ExitError::new(
                "invalid_argument",
                2,
                err.to_string(),
                "Use --format json, pretty, ndjson, or table.",
            )
        })?;
        self.maybe_check_for_updates(command)?;
        Ok(())
    }

    fn maybe_check_for_updates(&mut self, command: &ParsedCommand) -> Result<(), ExitError> {
        if is_update_exempt_command(command) {
            return Ok(());
        }

        let context = crate::workspace_config::resolve_update_policy_context(
            &self.globals.tenant,
            self.globals.tenant_changed,
        )
        .map_err(super::internal_anyhow)?;
        let outcome = self_update::check_context(&context);
        if let Some(err) = outcome.error {
            if self.globals.verbose {
                let _ = writeln!(io::stderr(), "[awiki-cli] update check failed: {err}");
            }
            return Ok(());
        }

        let decision = outcome.decision;
        if decision.blocked {
            let upgrade_command = if self.globals.tenant_changed {
                format!("awiki-cli --tenant {} upgrade", decision.tenant_alias)
            } else {
                "awiki-cli upgrade".to_string()
            };
            return Err(update_blocked_error(&decision, &upgrade_command));
        }

        if decision.has_newer_version
            && !decision.strict_disabled
            && !decision.dev_build
            && matches!(self.globals.format.as_str(), "pretty" | "table")
            && io::stdout().is_terminal()
        {
            self.update_warning = format!(
                "A newer awiki-cli version ({}) is available; you are running {}. Run `awiki-cli upgrade` for details.",
                decision.latest_version, decision.current_version
            );
        }
        Ok(())
    }
}

fn update_blocked_error(decision: &self_update::Decision, upgrade_command: &str) -> ExitError {
    ExitError::new(
        "version_unsupported",
        3,
        format!(
            "awiki-cli {} is no longer supported (minimum supported version is {}).",
            decision.current_version, decision.min_supported_version
        ),
        format!(
            "Please upgrade awiki-cli for tenant {} before running this command. Run `{}`.",
            decision.tenant_alias, upgrade_command
        ),
    )
}

pub(super) fn merge_update_warning(update_warning: &str, warnings: Vec<String>) -> Vec<String> {
    if update_warning.is_empty() {
        return warnings;
    }
    let mut merged = Vec::with_capacity(warnings.len() + 1);
    merged.push(update_warning.to_string());
    merged.extend(warnings);
    merged
}

fn is_update_exempt_command(command: &ParsedCommand) -> bool {
    matches!(
        command.name.as_str(),
        "version" | "upgrade" | "help" | "tenant.list" | "tenant.current" | "tenant.use"
    )
}

pub(super) fn npm_install_command(installer_url: &str) -> String {
    format!("npm install -g {}", installer_url.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_exempt_command_only_keeps_update_help_and_tenant_switch_available() {
        for name in [
            "help",
            "version",
            "upgrade",
            "tenant.list",
            "tenant.current",
            "tenant.use",
        ] {
            let command = ParsedCommand {
                name: name.to_string(),
                ..ParsedCommand::default()
            };
            assert!(
                is_update_exempt_command(&command),
                "{name} should be exempt"
            );
        }

        for name in [
            "status",
            "init",
            "config.show",
            "doctor",
            "docs",
            "schema",
            "completion",
            "tenant.create",
            "onboarding.resume",
            "id.vault.status",
            "runtime.listener.run",
        ] {
            let guarded = ParsedCommand {
                name: name.to_string(),
                ..ParsedCommand::default()
            };
            assert!(
                !is_update_exempt_command(&guarded),
                "{name} must be blocked below the tenant minimum"
            );
        }
    }

    #[test]
    fn update_warning_is_prepended_to_existing_warnings() {
        let warnings = merge_update_warning(
            "A newer awiki-cli version (1.0.2) is available; you are running 1.0.1. Run `awiki-cli upgrade` for details.",
            vec!["command warning".to_string()],
        );

        assert_eq!(
            warnings,
            vec![
                "A newer awiki-cli version (1.0.2) is available; you are running 1.0.1. Run `awiki-cli upgrade` for details.",
                "command warning"
            ]
        );
        assert_eq!(
            merge_update_warning("", vec!["command warning".to_string()]),
            vec!["command warning"]
        );
    }

    #[test]
    fn update_blocked_error_matches_go_exit_contract() {
        let err = update_blocked_error(
            &self_update::Decision {
                tenant_alias: "global".to_string(),
                current_version: "1.0.0".to_string(),
                latest_version: "1.0.2".to_string(),
                min_supported_version: "1.0.1".to_string(),
                installer_url: "https://awiki.example/cli/stable/awiki-cli.tgz".to_string(),
                metadata_source: "cache".to_string(),
                strict_disabled: false,
                dev_build: false,
                has_newer_version: true,
                blocked: true,
                ..self_update::Decision::default()
            },
            "awiki-cli --tenant global upgrade",
        );

        assert_eq!(err.exit_code, 3);
        assert_eq!(err.detail.code, "version_unsupported");
        assert_eq!(
            err.detail.message,
            "awiki-cli 1.0.0 is no longer supported (minimum supported version is 1.0.1)."
        );
        assert!(err
            .detail
            .hint
            .contains("Run `awiki-cli --tenant global upgrade`"));
    }
}
