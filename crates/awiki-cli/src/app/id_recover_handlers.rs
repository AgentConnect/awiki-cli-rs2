use super::App;
use crate::cli::ParsedCommand;
use crate::im_core_adapter::identity::{
    recover_handle_command_via_im_core, RecoverHandleCommandRequest,
};
use crate::output::ExitError;

impl App {
    pub fn run_id_recover(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let handle = required_string_flag(
            command,
            "handle",
            "id recover",
            "Usage: awiki-cli id recover --handle <handle> --phone <phone> [--otp <code>]",
        )?;
        let phone = required_string_flag(
            command,
            "phone",
            "id recover",
            "Usage: awiki-cli id recover --handle <handle> --phone <phone> [--otp <code>]",
        )?;
        let request = RecoverHandleCommandRequest {
            identity_name: self.globals.identity.clone(),
            handle,
            phone,
            otp: string_flag(command, "otp"),
        };
        let manager = self.identity_manager(&resolved);
        let result = recover_handle_command_via_im_core(
            &resolved,
            &manager,
            request,
            self.globals.dry_run,
            self.globals.identity_changed,
        )?;
        self.render_identity_result("awiki-cli id recover", &resolved, result)
    }
}

fn string_flag(command: &ParsedCommand, name: &str) -> String {
    command.flags.get(name).cloned().unwrap_or_default()
}

fn required_string_flag(
    command: &ParsedCommand,
    name: &str,
    command_name: &str,
    help: &str,
) -> Result<String, ExitError> {
    let value = string_flag(command, name);
    if value.trim().is_empty() {
        return Err(ExitError::new(
            "invalid_argument",
            2,
            format!("{command_name} requires --{name}."),
            help,
        ));
    }
    Ok(value)
}
