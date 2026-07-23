use std::io::{self, IsTerminal, Write};

use super::App;
use crate::cli_output::ExitError;
use crate::cli_parser::ParsedCommand;

impl App {
    pub async fn run_id_device_root_key_send_async(
        &self,
        command: &ParsedCommand,
    ) -> Result<(), ExitError> {
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
            confirm_root_key_transfer,
        )
        .await?;
        self.render_identity_result("awiki-cli id device root-key send", &resolved, result)
    }
}

fn flag<'a>(command: &'a ParsedCommand, name: &str) -> &'a str {
    command.flags.get(name).map(String::as_str).unwrap_or("")
}

fn confirm_root_key_transfer(
    did: &str,
    recipient_device_id: &str,
    signing_key_id: &str,
    e2ee_key_id: &str,
    expires_at: &str,
) -> Result<(), ExitError> {
    require_interactive_terminal("transfer")?;
    let mut stderr = io::stderr().lock();
    writeln!(stderr, "DID: {did}").map_err(io_error)?;
    writeln!(stderr, "Target device: {recipient_device_id}").map_err(io_error)?;
    writeln!(stderr, "Signing key ID: {signing_key_id}").map_err(io_error)?;
    writeln!(stderr, "E2EE key ID: {e2ee_key_id}").map_err(io_error)?;
    writeln!(stderr, "Preparation expires at: {expires_at}").map_err(io_error)?;
    write!(stderr, "Type TRANSFER to send the root key: ").map_err(io_error)?;
    stderr.flush().map_err(io_error)?;
    if read_line()? != "TRANSFER" {
        return Err(ExitError::new(
            "permission_denied",
            4,
            "Root-key transfer was not confirmed.",
            "No root-key transfer was sent.",
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
        "Run the command directly on the management device; scripted root-key transfer is disabled.",
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
        format!("interactive root-key confirmation failed: {error}"),
        "Retry from a foreground terminal.",
    )
}
