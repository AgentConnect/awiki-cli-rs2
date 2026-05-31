use super::App;
use crate::cli_output::ExitError;
use crate::cli_parser::ParsedCommand;
use crate::m_core_cli_adapter::message_result::CommandResult;
use crate::workspace_config::Resolved;

impl App {
    pub fn run_people_follow(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let result = if self.globals.dry_run {
            crate::m_core_cli_adapter::people::follow_plan(command, &resolved.did_domain)?
        } else {
            let client = self.people_client(&resolved)?;
            crate::m_core_cli_adapter::people::follow_via_im_core(
                &client,
                command,
                &resolved.did_domain,
            )?
        };
        self.render_people_result("awiki-cli people follow", &resolved, result)
    }

    pub async fn run_people_follow_async(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        if self.globals.dry_run {
            return self.run_people_follow(command);
        }
        let resolved = self.resolve_config_for_workspace()?;
        let client = self.people_client_async(&resolved).await?;
        let result = crate::m_core_cli_adapter::people::follow_via_im_core_async(
            &client,
            command,
            &resolved.did_domain,
        )
        .await?;
        self.render_people_result("awiki-cli people follow", &resolved, result)
    }

    pub fn run_people_unfollow(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let result = if self.globals.dry_run {
            crate::m_core_cli_adapter::people::unfollow_plan(command, &resolved.did_domain)?
        } else {
            let client = self.people_client(&resolved)?;
            crate::m_core_cli_adapter::people::unfollow_via_im_core(
                &client,
                command,
                &resolved.did_domain,
            )?
        };
        self.render_people_result("awiki-cli people unfollow", &resolved, result)
    }

    pub async fn run_people_unfollow_async(
        &self,
        command: &ParsedCommand,
    ) -> Result<(), ExitError> {
        if self.globals.dry_run {
            return self.run_people_unfollow(command);
        }
        let resolved = self.resolve_config_for_workspace()?;
        let client = self.people_client_async(&resolved).await?;
        let result = crate::m_core_cli_adapter::people::unfollow_via_im_core_async(
            &client,
            command,
            &resolved.did_domain,
        )
        .await?;
        self.render_people_result("awiki-cli people unfollow", &resolved, result)
    }

    pub fn run_people_status(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let result = if self.globals.dry_run {
            crate::m_core_cli_adapter::people::relationship_status_plan(
                command,
                &resolved.did_domain,
            )?
        } else {
            let client = self.people_client(&resolved)?;
            crate::m_core_cli_adapter::people::relationship_status_via_im_core(
                &client,
                command,
                &resolved.did_domain,
            )?
        };
        self.render_people_result("awiki-cli people status", &resolved, result)
    }

    pub async fn run_people_status_async(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        if self.globals.dry_run {
            return self.run_people_status(command);
        }
        let resolved = self.resolve_config_for_workspace()?;
        let client = self.people_client_async(&resolved).await?;
        let result = crate::m_core_cli_adapter::people::relationship_status_via_im_core_async(
            &client,
            command,
            &resolved.did_domain,
        )
        .await?;
        self.render_people_result("awiki-cli people status", &resolved, result)
    }

    pub fn run_people_followers(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let result = if self.globals.dry_run {
            crate::m_core_cli_adapter::people::followers_plan(command)?
        } else {
            let client = self.people_client(&resolved)?;
            crate::m_core_cli_adapter::people::followers_via_im_core(&client, command)?
        };
        self.render_people_result("awiki-cli people followers", &resolved, result)
    }

    pub async fn run_people_followers_async(
        &self,
        command: &ParsedCommand,
    ) -> Result<(), ExitError> {
        if self.globals.dry_run {
            return self.run_people_followers(command);
        }
        let resolved = self.resolve_config_for_workspace()?;
        let client = self.people_client_async(&resolved).await?;
        let result =
            crate::m_core_cli_adapter::people::followers_via_im_core_async(&client, command)
                .await?;
        self.render_people_result("awiki-cli people followers", &resolved, result)
    }

    pub fn run_people_following(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let result = if self.globals.dry_run {
            crate::m_core_cli_adapter::people::following_plan(command)?
        } else {
            let client = self.people_client(&resolved)?;
            crate::m_core_cli_adapter::people::following_via_im_core(&client, command)?
        };
        self.render_people_result("awiki-cli people following", &resolved, result)
    }

    pub async fn run_people_following_async(
        &self,
        command: &ParsedCommand,
    ) -> Result<(), ExitError> {
        if self.globals.dry_run {
            return self.run_people_following(command);
        }
        let resolved = self.resolve_config_for_workspace()?;
        let client = self.people_client_async(&resolved).await?;
        let result =
            crate::m_core_cli_adapter::people::following_via_im_core_async(&client, command)
                .await?;
        self.render_people_result("awiki-cli people following", &resolved, result)
    }

    pub fn run_people_contacts_list(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let result = if self.globals.dry_run {
            crate::m_core_cli_adapter::people::contacts_list_plan(command)?
        } else {
            let client = self.people_client(&resolved)?;
            crate::m_core_cli_adapter::people::contacts_list_via_im_core(&client, command)?
        };
        self.render_people_result("awiki-cli people contacts list", &resolved, result)
    }

    pub async fn run_people_contacts_list_async(
        &self,
        command: &ParsedCommand,
    ) -> Result<(), ExitError> {
        if self.globals.dry_run {
            return self.run_people_contacts_list(command);
        }
        let resolved = self.resolve_config_for_workspace()?;
        let client = self.people_client_async(&resolved).await?;
        let result =
            crate::m_core_cli_adapter::people::contacts_list_via_im_core_async(&client, command)
                .await?;
        self.render_people_result("awiki-cli people contacts list", &resolved, result)
    }

    pub fn run_people_contacts_save(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let result = if self.globals.dry_run {
            crate::m_core_cli_adapter::people::save_contact_plan(command, &resolved.did_domain)?
        } else {
            let client = self.people_client(&resolved)?;
            crate::m_core_cli_adapter::people::contacts_save_via_im_core(
                &client,
                command,
                &resolved.did_domain,
            )?
        };
        self.render_people_result("awiki-cli people contacts save", &resolved, result)
    }

    pub async fn run_people_contacts_save_async(
        &self,
        command: &ParsedCommand,
    ) -> Result<(), ExitError> {
        if self.globals.dry_run {
            return self.run_people_contacts_save(command);
        }
        let resolved = self.resolve_config_for_workspace()?;
        let client = self.people_client_async(&resolved).await?;
        let result = crate::m_core_cli_adapter::people::contacts_save_via_im_core_async(
            &client,
            command,
            &resolved.did_domain,
        )
        .await?;
        self.render_people_result("awiki-cli people contacts save", &resolved, result)
    }

    fn people_client(&self, resolved: &Resolved) -> Result<im_core::ImClient, ExitError> {
        crate::m_core_cli_adapter::build_im_client(
            resolved,
            crate::m_core_cli_adapter::cli_identity_selector(&self.globals.identity),
        )
    }

    async fn people_client_async(
        &self,
        resolved: &Resolved,
    ) -> Result<im_core::ImClient, ExitError> {
        crate::m_core_cli_adapter::build_im_client_async(
            resolved,
            crate::m_core_cli_adapter::cli_identity_selector(&self.globals.identity),
        )
        .await
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
