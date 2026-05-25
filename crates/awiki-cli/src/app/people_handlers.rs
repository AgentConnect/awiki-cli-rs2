use super::App;
use crate::cli::ParsedCommand;
use crate::config::Resolved;
use crate::im_core_adapter::message_result::CommandResult;
use crate::output::ExitError;

impl App {
    pub fn run_people_follow(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let result = if self.globals.dry_run {
            crate::im_core_adapter::people::follow_plan(command, &resolved.did_domain)?
        } else {
            let client = self.people_client(&resolved)?;
            crate::im_core_adapter::people::follow_via_im_core(
                &client,
                command,
                &resolved.did_domain,
            )?
        };
        self.render_people_result("awiki-cli people follow", &resolved, result)
    }

    pub fn run_people_unfollow(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let result = if self.globals.dry_run {
            crate::im_core_adapter::people::unfollow_plan(command, &resolved.did_domain)?
        } else {
            let client = self.people_client(&resolved)?;
            crate::im_core_adapter::people::unfollow_via_im_core(
                &client,
                command,
                &resolved.did_domain,
            )?
        };
        self.render_people_result("awiki-cli people unfollow", &resolved, result)
    }

    pub fn run_people_status(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let result = if self.globals.dry_run {
            crate::im_core_adapter::people::relationship_status_plan(command, &resolved.did_domain)?
        } else {
            let client = self.people_client(&resolved)?;
            crate::im_core_adapter::people::relationship_status_via_im_core(
                &client,
                command,
                &resolved.did_domain,
            )?
        };
        self.render_people_result("awiki-cli people status", &resolved, result)
    }

    pub fn run_people_followers(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let result = if self.globals.dry_run {
            crate::im_core_adapter::people::followers_plan(command)?
        } else {
            let client = self.people_client(&resolved)?;
            crate::im_core_adapter::people::followers_via_im_core(&client, command)?
        };
        self.render_people_result("awiki-cli people followers", &resolved, result)
    }

    pub fn run_people_following(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let result = if self.globals.dry_run {
            crate::im_core_adapter::people::following_plan(command)?
        } else {
            let client = self.people_client(&resolved)?;
            crate::im_core_adapter::people::following_via_im_core(&client, command)?
        };
        self.render_people_result("awiki-cli people following", &resolved, result)
    }

    pub fn run_people_contacts_list(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let result = if self.globals.dry_run {
            crate::im_core_adapter::people::contacts_list_plan(command)?
        } else {
            let client = self.people_client(&resolved)?;
            crate::im_core_adapter::people::contacts_list_via_im_core(&client, command)?
        };
        self.render_people_result("awiki-cli people contacts list", &resolved, result)
    }

    pub fn run_people_contacts_save(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let result = if self.globals.dry_run {
            crate::im_core_adapter::people::save_contact_plan(command, &resolved.did_domain)?
        } else {
            let client = self.people_client(&resolved)?;
            crate::im_core_adapter::people::contacts_save_via_im_core(
                &client,
                command,
                &resolved.did_domain,
            )?
        };
        self.render_people_result("awiki-cli people contacts save", &resolved, result)
    }

    fn people_client(&self, resolved: &Resolved) -> Result<im_core::ImClient, ExitError> {
        let manager = self.identity_manager(resolved);
        crate::im_core_adapter::build_im_client(
            resolved,
            &manager,
            crate::im_core_adapter::cli_identity_selector(&self.globals.identity),
        )
    }

    fn render_people_result(
        &self,
        command: &str,
        resolved: &Resolved,
        result: CommandResult,
    ) -> Result<(), ExitError> {
        self.render_success(
            command,
            resolved,
            result.data,
            &result.summary,
            result.warnings,
        )
    }
}
