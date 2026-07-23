use std::io::{self, IsTerminal, Write};

use super::App;
use crate::cli_output::ExitError;
use crate::cli_parser::ParsedCommand;

impl App {
    pub async fn run_id_device_list_async(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let result = crate::m_core_cli_adapter::device_join::registry_via_im_core_async(
            &resolved,
            identity_selector(self),
        )
        .await?;
        self.render_identity_result("awiki-cli id device list", &resolved, result)
    }

    pub async fn run_id_device_join_sessions_async(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let result =
            crate::m_core_cli_adapter::device_join::local_sessions_via_im_core_async(&resolved)
                .await?;
        self.render_identity_result("awiki-cli id device join sessions", &resolved, result)
    }

    pub async fn run_id_device_join_requests_async(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let result = crate::m_core_cli_adapter::device_join::local_requests_via_im_core_async(
            &resolved,
            identity_selector(self),
        )
        .await?;
        self.render_identity_result("awiki-cli id device join requests", &resolved, result)
    }

    pub async fn run_id_device_join_start_async(
        &self,
        command: &ParsedCommand,
    ) -> Result<(), ExitError> {
        reject_dry_run(self, "id device join start")?;
        let resolved = self.resolve_config_for_workspace()?;
        let result = crate::m_core_cli_adapter::device_join::begin_via_im_core_async(
            &resolved,
            flag(command, "did"),
            flag(command, "operation-id"),
            u64_flag(command, "ttl-seconds", 600)?,
        )
        .await?;
        self.render_identity_result("awiki-cli id device join start", &resolved, result)
    }

    pub async fn run_id_device_join_poll_async(
        &self,
        command: &ParsedCommand,
    ) -> Result<(), ExitError> {
        reject_dry_run(self, "id device join poll")?;
        let resolved = self.resolve_config_for_workspace()?;
        let result = crate::m_core_cli_adapter::device_join::poll_new_device_via_im_core_async(
            &resolved,
            flag(command, "session"),
        )
        .await?;
        self.render_identity_result("awiki-cli id device join poll", &resolved, result)
    }

    pub async fn run_id_device_join_verify_async(
        &self,
        command: &ParsedCommand,
    ) -> Result<(), ExitError> {
        reject_dry_run(self, "id device join verify")?;
        let resolved = self.resolve_config_for_workspace()?;
        let result = crate::m_core_cli_adapter::device_join::start_verification_via_im_core_async(
            &resolved,
            identity_selector(self),
            flag(command, "session"),
            flag(command, "operation-id"),
            u64_flag(command, "challenge-ttl-seconds", 300)?,
        )
        .await?;
        self.render_identity_result("awiki-cli id device join verify", &resolved, result)
    }

    pub async fn run_id_device_join_approve_async(
        &self,
        command: &ParsedCommand,
    ) -> Result<(), ExitError> {
        reject_dry_run(self, "id device join approve")?;
        let resolved = self.resolve_config_for_workspace()?;
        let result = crate::m_core_cli_adapter::device_join::approve_via_im_core_async(
            &resolved,
            identity_selector(self),
            flag(command, "session"),
            confirm_sas_and_user_presence,
        )
        .await?;
        self.render_identity_result("awiki-cli id device join approve", &resolved, result)
    }

    pub async fn run_id_device_join_reject_async(
        &self,
        command: &ParsedCommand,
    ) -> Result<(), ExitError> {
        reject_dry_run(self, "id device join reject")?;
        let resolved = self.resolve_config_for_workspace()?;
        let result = crate::m_core_cli_adapter::device_join::reject_via_im_core_async(
            &resolved,
            identity_selector(self),
            flag(command, "session"),
            flag(command, "reason"),
        )
        .await?;
        self.render_identity_result("awiki-cli id device join reject", &resolved, result)
    }

    pub async fn run_id_device_join_cancel_async(
        &self,
        command: &ParsedCommand,
    ) -> Result<(), ExitError> {
        reject_dry_run(self, "id device join cancel")?;
        let resolved = self.resolve_config_for_workspace()?;
        let result = crate::m_core_cli_adapter::device_join::cancel_via_im_core_async(
            &resolved,
            flag(command, "session"),
        )
        .await?;
        self.render_identity_result("awiki-cli id device join cancel", &resolved, result)
    }
}

fn identity_selector(app: &App) -> im_core::IdentitySelector {
    crate::m_core_cli_adapter::cli_identity_selector(&app.globals.identity)
}

fn flag<'a>(command: &'a ParsedCommand, name: &str) -> &'a str {
    command.flags.get(name).map(String::as_str).unwrap_or("")
}

fn u64_flag(command: &ParsedCommand, name: &str, default: u64) -> Result<u64, ExitError> {
    let value = flag(command, name);
    if value.is_empty() {
        return Ok(default);
    }
    value.parse().map_err(|_| {
        ExitError::new(
            "invalid_argument",
            2,
            format!("--{name} must be a positive integer."),
            "Pass a duration in whole seconds.",
        )
    })
}

fn reject_dry_run(app: &App, command: &str) -> Result<(), ExitError> {
    if !app.globals.dry_run {
        return Ok(());
    }
    Err(ExitError::new(
        "invalid_argument",
        2,
        format!("{command} does not support --dry-run."),
        "Use the read-only sessions or list command to inspect state.",
    ))
}

fn confirm_sas_and_user_presence(sas: &str) -> Result<(), ExitError> {
    if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
        return Err(ExitError::new(
            "user_presence_required",
            3,
            "Device Join approval requires an interactive foreground terminal.",
            "Run the approve command directly and compare the SAS on both devices; scripted approval is intentionally disabled.",
        ));
    }

    let mut stderr = io::stderr().lock();
    writeln!(
        stderr,
        "Compare this one-time SAS with the new device: {sas}"
    )
    .map_err(io_error)?;
    write!(stderr, "Type the same 6-digit SAS to continue: ").map_err(io_error)?;
    stderr.flush().map_err(io_error)?;
    let entered_sas = read_confirmation_line()?;
    if entered_sas != sas {
        return Err(ExitError::new(
            "sas_mismatch",
            4,
            "The entered SAS does not match the locally derived SAS.",
            "Cancel this Join request and investigate a possible device or relay mismatch.",
        ));
    }

    write!(
        stderr,
        "Type APPROVE to confirm local user presence and authorize this device: "
    )
    .map_err(io_error)?;
    stderr.flush().map_err(io_error)?;
    if read_confirmation_line()? != "APPROVE" {
        return Err(ExitError::new(
            "permission_denied",
            4,
            "Device Join approval was not confirmed.",
            "No device authorization was submitted.",
        ));
    }
    Ok(())
}

fn read_confirmation_line() -> Result<String, ExitError> {
    let mut line = String::new();
    io::stdin().read_line(&mut line).map_err(io_error)?;
    Ok(line.trim().to_owned())
}

fn io_error(err: io::Error) -> ExitError {
    ExitError::new(
        "io_error",
        1,
        format!("read device Join confirmation: {err}"),
        "Retry in an interactive foreground terminal.",
    )
}
