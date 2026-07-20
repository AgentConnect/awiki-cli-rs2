use std::io::{self, IsTerminal, Write};

use super::App;
use crate::cli_output::ExitError;
use crate::cli_parser::ParsedCommand;

impl App {
    pub async fn run_id_device_root_key_send_async(
        &self,
        command: &ParsedCommand,
    ) -> Result<(), ExitError> {
        crate::m_core_cli_adapter::root_key_transfer::require_rollout_enabled()?;
        if self.globals.dry_run {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "id device root-key send does not support --dry-run.",
                "Use `id device list` to inspect the target before an interactive transfer.",
            ));
        }
        let resolved = self.resolve_config_for_workspace()?;
        let result = crate::m_core_cli_adapter::root_key_transfer::send_via_im_core_async(
            &resolved,
            crate::m_core_cli_adapter::cli_identity_selector(&self.globals.identity),
            flag(command, "device"),
            flag(command, "message-id"),
            confirm_root_key_transfer,
        )
        .await?;
        self.render_identity_result("awiki-cli id device root-key send", &resolved, result)
    }

    pub async fn run_id_device_root_key_list_async(
        &self,
        command: &ParsedCommand,
    ) -> Result<(), ExitError> {
        crate::m_core_cli_adapter::root_key_transfer::require_rollout_enabled()?;
        let resolved = self.resolve_config_for_workspace()?;
        let result = crate::m_core_cli_adapter::root_key_transfer::list_via_im_core_async(
            &resolved,
            crate::m_core_cli_adapter::cli_identity_selector(&self.globals.identity),
            bool_flag(command, "include-completed")?,
        )
        .await?;
        self.render_identity_result("awiki-cli id device root-key list", &resolved, result)
    }

    pub async fn run_id_device_root_key_retry_async(
        &self,
        command: &ParsedCommand,
    ) -> Result<(), ExitError> {
        crate::m_core_cli_adapter::root_key_transfer::require_rollout_enabled()?;
        if self.globals.dry_run {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "id device root-key retry does not support --dry-run.",
                "Use `id device root-key list` to inspect retryable operations.",
            ));
        }
        require_interactive_terminal("retry")?;
        let resolved = self.resolve_config_for_workspace()?;
        let result = crate::m_core_cli_adapter::root_key_transfer::retry_via_im_core_async(
            &resolved,
            crate::m_core_cli_adapter::cli_identity_selector(&self.globals.identity),
            flag(command, "message-id"),
            confirm_root_key_retry,
        )
        .await?;
        self.render_identity_result("awiki-cli id device root-key retry", &resolved, result)
    }
}

fn flag<'a>(command: &'a ParsedCommand, name: &str) -> &'a str {
    command.flags.get(name).map(String::as_str).unwrap_or("")
}

fn confirm_root_key_transfer(did: &str, recipient_device_id: &str) -> Result<(), ExitError> {
    require_interactive_terminal("transfer")?;
    let mut stderr = io::stderr().lock();
    writeln!(stderr, "DID: {did}").map_err(io_error)?;
    writeln!(stderr, "Target management device: {recipient_device_id}").map_err(io_error)?;
    write!(stderr, "Re-enter the target device ID: ").map_err(io_error)?;
    stderr.flush().map_err(io_error)?;
    if read_line()? != recipient_device_id {
        return Err(ExitError::new(
            "device_mismatch",
            4,
            "The entered target device ID does not match.",
            "No root-key control was prepared or sent.",
        ));
    }
    write!(stderr, "Type TRANSFER to confirm local user presence: ").map_err(io_error)?;
    stderr.flush().map_err(io_error)?;
    if read_line()? != "TRANSFER" {
        return Err(ExitError::new(
            "permission_denied",
            4,
            "Root-key transfer was not confirmed.",
            "No root-key control was prepared or sent.",
        ));
    }
    Ok(())
}

fn confirm_root_key_retry(did: &str, message_id: &str) -> Result<(), ExitError> {
    require_interactive_terminal("retry")?;
    let mut stderr = io::stderr().lock();
    writeln!(stderr, "DID: {did}").map_err(io_error)?;
    writeln!(stderr, "Persisted root-control message: {message_id}").map_err(io_error)?;
    write!(
        stderr,
        "Type RETRY to confirm local user presence and resend the exact persisted ciphertext: "
    )
    .map_err(io_error)?;
    stderr.flush().map_err(io_error)?;
    if read_line()? != "RETRY" {
        return Err(ExitError::new(
            "permission_denied",
            4,
            "Root-key transfer retry was not confirmed.",
            "No persisted root-control ciphertext was resent.",
        ));
    }
    Ok(())
}

fn require_interactive_terminal(action: &str) -> Result<(), ExitError> {
    if io::stdin().is_terminal() && io::stderr().is_terminal() {
        return Ok(());
    }
    Err(ExitError::new(
        "user_presence_required",
        3,
        format!("Root-key transfer {action} requires an interactive foreground terminal."),
        "Run the command directly on the management device; scripted root-key control is intentionally disabled.",
    ))
}

fn bool_flag(command: &ParsedCommand, name: &str) -> Result<bool, ExitError> {
    match flag(command, name) {
        "" | "false" => Ok(false),
        "true" => Ok(true),
        _ => Err(ExitError::new(
            "invalid_argument",
            2,
            format!("--{name} must be a boolean flag."),
            format!("Pass --{name} without a value to enable it."),
        )),
    }
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
        format!("interactive root-key confirmation failed: {error}"),
        "Retry from a foreground terminal.",
    )
}
