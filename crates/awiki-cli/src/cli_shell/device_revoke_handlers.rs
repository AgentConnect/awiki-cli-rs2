use std::io::{self, IsTerminal, Write};

use super::App;
use crate::cli_output::ExitError;
use crate::cli_parser::ParsedCommand;

impl App {
    pub async fn run_id_device_revoke_async(
        &self,
        command: &ParsedCommand,
    ) -> Result<(), ExitError> {
        crate::m_core_cli_adapter::device_revoke::require_rollout_enabled()?;
        if self.globals.dry_run {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "id device revoke does not support --dry-run.",
                "Use `id device list` to inspect the target before interactive permanent revocation.",
            ));
        }
        require_interactive_terminal()?;
        let resolved = self.resolve_config_for_workspace()?;
        let result = crate::m_core_cli_adapter::device_revoke::revoke_via_im_core_async(
            &resolved,
            crate::m_core_cli_adapter::cli_identity_selector(&self.globals.identity),
            command
                .flags
                .get("device")
                .map(String::as_str)
                .unwrap_or(""),
            confirm_device_revoke,
        )
        .await?;
        self.render_identity_result("awiki-cli id device revoke", &resolved, result)
    }
}

fn confirm_device_revoke(did: &str, target_device_id: &str) -> Result<(), ExitError> {
    require_interactive_terminal()?;
    let mut stderr = io::stderr().lock();
    writeln!(stderr, "DID: {did}").map_err(io_error)?;
    writeln!(stderr, "Device to revoke permanently: {target_device_id}").map_err(io_error)?;
    write!(stderr, "Re-enter the target device ID: ").map_err(io_error)?;
    stderr.flush().map_err(io_error)?;
    if read_line()? != target_device_id {
        return Err(ExitError::new(
            "device_mismatch",
            4,
            "The entered target device ID does not match.",
            "No device revocation was submitted.",
        ));
    }
    write!(stderr, "Type REVOKE to confirm permanent revocation: ").map_err(io_error)?;
    stderr.flush().map_err(io_error)?;
    if read_line()? != "REVOKE" {
        return Err(ExitError::new(
            "permission_denied",
            4,
            "Permanent device revocation was not confirmed.",
            "No device revocation was submitted.",
        ));
    }
    Ok(())
}

fn require_interactive_terminal() -> Result<(), ExitError> {
    if io::stdin().is_terminal() && io::stderr().is_terminal() {
        return Ok(());
    }
    Err(ExitError::new(
        "user_presence_required",
        3,
        "Permanent device revocation requires an interactive foreground terminal.",
        "Run the command directly on a ready management device; scripted revocation is intentionally disabled.",
    ))
}

fn read_line() -> Result<String, ExitError> {
    let mut value = String::new();
    io::stdin().read_line(&mut value).map_err(io_error)?;
    Ok(value.trim().to_owned())
}

fn io_error(error: io::Error) -> ExitError {
    ExitError::new(
        "io_error",
        1,
        format!("read device revocation confirmation: {error}"),
        "Retry from an interactive foreground terminal.",
    )
}
