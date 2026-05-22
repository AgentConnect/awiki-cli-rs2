use super::{changed_string_flag, optional_bool_flag, App};
use crate::cli::ParsedCommand;
use crate::output::ExitError;

impl App {
    pub fn run_id_replace_did(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let is_public = optional_bool_flag(command, "is-public")?;
        let is_agent = optional_bool_flag(command, "is-agent")?;
        let role = changed_string_flag(command, "role");
        let endpoint_url = changed_string_flag(command, "endpoint-url");
        if self.globals.dry_run {
            let result = crate::im_core_adapter::identity::replace_did_plan_via_im_core(
                &resolved,
                &self.identity_manager(&resolved),
                &self.globals.identity,
                is_public,
                is_agent,
                role.as_deref(),
                endpoint_url.as_deref(),
            )?;
            return self.render_identity_result("awiki-cli id replace-did", &resolved, result);
        }

        let manager = self.identity_manager(&resolved);
        let result = crate::im_core_adapter::identity::replace_did_via_im_core(
            &resolved,
            &manager,
            &self.globals.identity,
            is_public,
            is_agent,
            role,
            endpoint_url,
        )?;

        self.render_identity_result("awiki-cli id replace-did", &resolved, result)
    }
}
