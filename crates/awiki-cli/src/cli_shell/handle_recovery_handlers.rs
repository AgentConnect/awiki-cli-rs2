use std::io::{self, IsTerminal, Write};

use super::App;
use crate::cli_output::ExitError;
use crate::cli_parser::ParsedCommand;

impl App {
    pub async fn run_id_recovery_sessions_async(&self) -> Result<(), ExitError> {
        require_enabled()?;
        let resolved = self.resolve_config_for_workspace()?;
        let result =
            crate::m_core_cli_adapter::handle_recovery::local_sessions_via_im_core_async(&resolved)
                .await?;
        self.render_identity_result("awiki-cli id recovery sessions", &resolved, result)
    }

    pub async fn run_id_recovery_begin_async(
        &self,
        command: &ParsedCommand,
    ) -> Result<(), ExitError> {
        require_enabled()?;
        reject_dry_run(self, "id recovery begin")?;
        let resolved = self.resolve_config_for_workspace()?;
        let result = crate::m_core_cli_adapter::handle_recovery::begin_via_im_core_async(
            &resolved,
            flag(command, "handle"),
        )
        .await?;
        self.render_identity_result("awiki-cli id recovery begin", &resolved, result)
    }

    pub async fn run_id_recovery_status_async(
        &self,
        command: &ParsedCommand,
    ) -> Result<(), ExitError> {
        require_enabled()?;
        let resolved = self.resolve_config_for_workspace()?;
        let result = crate::m_core_cli_adapter::handle_recovery::status_via_im_core_async(
            &resolved,
            flag(command, "session"),
        )
        .await?;
        self.render_identity_result("awiki-cli id recovery status", &resolved, result)
    }

    pub async fn run_id_recovery_cancel_async(
        &self,
        command: &ParsedCommand,
    ) -> Result<(), ExitError> {
        require_enabled()?;
        reject_dry_run(self, "id recovery cancel")?;
        let session = flag(command, "session");
        confirm_destructive_recovery_action(session, "CANCEL")?;
        let resolved = self.resolve_config_for_workspace()?;
        let result = crate::m_core_cli_adapter::handle_recovery::cancel_via_im_core_async(
            &resolved,
            crate::m_core_cli_adapter::cli_identity_selector(&self.globals.identity),
            session,
        )
        .await?;
        self.render_identity_result("awiki-cli id recovery cancel", &resolved, result)
    }

    pub async fn run_id_recovery_finalize_async(
        &self,
        command: &ParsedCommand,
    ) -> Result<(), ExitError> {
        require_enabled()?;
        reject_dry_run(self, "id recovery finalize")?;
        let session = flag(command, "session");
        confirm_destructive_recovery_action(session, "RESET")?;
        let resolved = self.resolve_config_for_workspace()?;
        let result = crate::m_core_cli_adapter::handle_recovery::finalize_via_im_core_async(
            &resolved, session,
        )
        .await?;
        self.render_identity_result("awiki-cli id recovery finalize", &resolved, result)
    }

    pub async fn run_id_recovery_activate_async(
        &self,
        command: &ParsedCommand,
    ) -> Result<(), ExitError> {
        require_enabled()?;
        reject_dry_run(self, "id recovery activate")?;
        let resolved = self.resolve_config_for_workspace()?;
        let result = crate::m_core_cli_adapter::handle_recovery::activate_via_im_core_async(
            &resolved,
            flag(command, "session"),
        )
        .await?;
        self.render_identity_result("awiki-cli id recovery activate", &resolved, result)
    }
}

fn require_enabled() -> Result<(), ExitError> {
    crate::m_core_cli_adapter::handle_recovery::require_rollout_enabled()
}

fn flag<'a>(command: &'a ParsedCommand, name: &str) -> &'a str {
    command.flags.get(name).map(String::as_str).unwrap_or("")
}

fn reject_dry_run(app: &App, command: &str) -> Result<(), ExitError> {
    if !app.globals.dry_run {
        return Ok(());
    }
    Err(ExitError::new(
        "invalid_argument",
        2,
        format!("{command} does not support --dry-run."),
        "Use sessions or status to inspect Recovery without changing state.",
    ))
}

fn confirm_destructive_recovery_action(session: &str, expected: &str) -> Result<(), ExitError> {
    if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
        return Err(ExitError::new(
            "user_presence_required",
            3,
            "Handle Recovery cancellation and finalization require an interactive foreground terminal.",
            "Run the command directly; scripted user-presence confirmation is intentionally disabled.",
        ));
    }
    let session = session.trim();
    if session.is_empty() {
        return Err(ExitError::new(
            "invalid_argument",
            2,
            "--session is required.",
            "Pass the Recovery Session id shown by begin or status.",
        ));
    }
    let mut stderr = io::stderr().lock();
    writeln!(
        stderr,
        "Recovery Session: {session}\nThis operation does not recover the old DID root key."
    )
    .map_err(io_error)?;
    write!(stderr, "Re-enter the Recovery Session id: ").map_err(io_error)?;
    stderr.flush().map_err(io_error)?;
    if read_line()? != session {
        return Err(ExitError::new(
            "session_mismatch",
            4,
            "The entered Recovery Session id does not match.",
            "No Recovery control operation was submitted.",
        ));
    }
    write!(stderr, "Type {expected} to confirm local user presence: ").map_err(io_error)?;
    stderr.flush().map_err(io_error)?;
    if read_line()? != expected {
        return Err(ExitError::new(
            "permission_denied",
            4,
            "Handle Recovery action was not confirmed.",
            "No Recovery control operation was submitted.",
        ));
    }
    Ok(())
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
        format!("interactive Handle Recovery confirmation failed: {error}"),
        "Retry from a foreground terminal.",
    )
}
