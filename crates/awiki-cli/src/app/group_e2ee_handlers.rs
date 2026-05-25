use super::App;
use crate::cli::ParsedCommand;
use crate::config::Resolved;
use crate::output::ExitError;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const DEFAULT_DEVICE: &str = "default";

impl App {
    pub fn run_group_e2ee_publish_key_package(
        &self,
        command: &ParsedCommand,
    ) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        if !self.globals.dry_run {
            return Err(unsupported_group_e2ee_command(
                "group.e2ee.publish-key-package",
            ));
        }
        let mut purpose = string_flag_or(command, "purpose", "normal");
        if bool_flag(command, "recovery")? {
            purpose = "recovery".to_string();
        }
        if purpose.trim().is_empty() {
            purpose = "normal".to_string();
        }
        let device = string_flag_or(command, "device", DEFAULT_DEVICE);
        let group = string_flag(command, "group");
        let contract_test = bool_flag(command, "contract-test")?;
        let plan = json!({
            "action": "group.e2ee.publish_key_package",
            "identity": self.globals.identity,
            "runtime_mode": resolved.runtime_mode,
            "provider": "exec",
            "binary": provider_binary(),
            "mls_data_dir": mls_data_dir(&resolved),
            "device": device,
            "group": group,
            "recovery": purpose == "recovery",
            "purpose": purpose,
            "contract_test_only": contract_test,
        });
        self.render_group_e2ee_plan(
            "awiki-cli group e2ee publish-key-package",
            &resolved,
            plan,
            "Dry run: group e2ee key package publish planned",
        )
    }

    pub fn run_group_e2ee_pending(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        if !self.globals.dry_run {
            return Err(unsupported_group_e2ee_command("group.e2ee.pending"));
        }
        let plan = json!({
            "action": "group.e2ee.pending",
            "identity": self.globals.identity,
            "runtime_mode": resolved.runtime_mode,
            "provider": "exec",
            "mls_data_dir": mls_data_dir(&resolved),
            "group": string_flag(command, "group"),
        });
        self.render_group_e2ee_plan(
            "awiki-cli group e2ee pending",
            &resolved,
            plan,
            "Dry run: group e2ee pending planned",
        )
    }

    pub fn run_group_e2ee_process_leave_request(
        &self,
        command: &ParsedCommand,
    ) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        if !self.globals.dry_run {
            return Err(unsupported_group_e2ee_command(
                "group.e2ee.process-leave-request",
            ));
        }
        let group = required_string_flag(
            command,
            "group",
            "group e2ee process-leave-request",
            "Usage: awiki-cli group e2ee process-leave-request --group <GROUP_DID> --member <MEMBER>",
        )?;
        let member = required_string_flag(
            command,
            "member",
            "group e2ee process-leave-request",
            "Usage: awiki-cli group e2ee process-leave-request --group <GROUP_DID> --member <MEMBER>",
        )?;
        let leave_request_id = string_flag(command, "leave-request-id");
        let reason = string_flag(command, "reason");
        let plan = json!({
            "action": "group.e2ee.process_leave_request",
            "identity": self.globals.identity,
            "runtime_mode": resolved.runtime_mode,
            "provider": "exec",
            "mls_data_dir": mls_data_dir(&resolved),
            "group": group,
            "member": member,
            "leave_request_id": leave_request_id,
            "request": {
                "IdentityName": self.globals.identity,
                "Group": group,
                "Member": member,
                "LeaveRequestID": leave_request_id,
                "ReasonText": reason,
            },
        });
        self.render_group_e2ee_plan(
            "awiki-cli group e2ee process-leave-request",
            &resolved,
            plan,
            "Dry run: group e2ee leave request process planned",
        )
    }

    pub fn run_group_e2ee_recover_member(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        if !self.globals.dry_run {
            return Err(unsupported_group_e2ee_command("group.e2ee.recover-member"));
        }
        let group = required_string_flag(
            command,
            "group",
            "group e2ee recover-member",
            "Usage: awiki-cli group e2ee recover-member --group <GROUP_DID> --member <MEMBER>",
        )?;
        let member = required_string_flag(
            command,
            "member",
            "group e2ee recover-member",
            "Usage: awiki-cli group e2ee recover-member --group <GROUP_DID> --member <MEMBER>",
        )?;
        let device = string_flag_or(command, "device", DEFAULT_DEVICE);
        let plan = json!({
            "action": "group.e2ee.recover_member",
            "identity": self.globals.identity,
            "runtime_mode": resolved.runtime_mode,
            "provider": "exec",
            "mls_data_dir": mls_data_dir(&resolved),
            "group": group,
            "member": member,
            "device": device,
            "p4_membership_mutate": false,
            "orchestration": [
                "lease recovery KeyPackage",
                "anp-mls recover-member-prepare",
                "hidden group.e2ee.recover_member",
                "finalize on accept",
                "abort on deterministic rejection",
            ],
        });
        self.render_group_e2ee_plan(
            "awiki-cli group e2ee recover-member",
            &resolved,
            plan,
            "Dry run: group e2ee recover-member planned",
        )
    }

    pub fn run_group_e2ee_update_key(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        if !self.globals.dry_run {
            return Err(unsupported_group_e2ee_command("group.e2ee.update-key"));
        }
        let group = required_string_flag(
            command,
            "group",
            "group e2ee update-key",
            "Usage: awiki-cli group e2ee update-key --group <GROUP_DID> --member <MEMBER>",
        )?;
        let member = required_string_flag(
            command,
            "member",
            "group e2ee update-key",
            "Usage: awiki-cli group e2ee update-key --group <GROUP_DID> --member <MEMBER>",
        )?;
        let device = string_flag_or(command, "device", DEFAULT_DEVICE);
        let plan = json!({
            "action": "group.e2ee.update_key",
            "identity": self.globals.identity,
            "runtime_mode": resolved.runtime_mode,
            "provider": "exec",
            "mls_data_dir": mls_data_dir(&resolved),
            "group": group,
            "member": member,
            "device": device,
            "key_package_purpose": "update",
            "hidden_awiki_extension": true,
            "p4_membership_mutate": false,
            "orchestration": [
                "lease purpose=update KeyPackage",
                "anp-mls update-member-prepare",
                "hidden group.e2ee.update",
                "finalize on accept",
                "abort on deterministic rejection",
            ],
        });
        self.render_group_e2ee_plan(
            "awiki-cli group e2ee update-key",
            &resolved,
            plan,
            "Dry run: group e2ee update-key planned",
        )
    }

    pub fn run_group_e2ee_rejoin(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        if !self.globals.dry_run {
            return Err(unsupported_group_e2ee_command("group.e2ee.rejoin"));
        }
        let group = required_string_flag(
            command,
            "group",
            "group e2ee rejoin",
            "Usage: awiki-cli group e2ee rejoin --group <GROUP_DID> --member <MEMBER>",
        )?;
        let member = required_string_flag(
            command,
            "member",
            "group e2ee rejoin",
            "Usage: awiki-cli group e2ee rejoin --group <GROUP_DID> --member <MEMBER>",
        )?;
        let role = string_flag_or(command, "role", "member");
        let plan = json!({
            "action": "group.e2ee.rejoin",
            "canonical_command": "group add --e2ee",
            "identity": self.globals.identity,
            "runtime_mode": resolved.runtime_mode,
            "group": group,
            "member": member,
            "role": role,
            "key_package_purpose": "normal",
            "recovery_command": "group e2ee recover-member is only for active-member crypto recovery, not removed/left rejoin",
            "external_commit": false,
            "p4_membership_mutate": true,
        });
        self.render_group_e2ee_plan(
            "awiki-cli group e2ee rejoin",
            &resolved,
            plan,
            "Dry run: group e2ee rejoin planned",
        )
    }

    fn render_group_e2ee_plan(
        &self,
        command: &str,
        resolved: &Resolved,
        plan: Value,
        summary: &str,
    ) -> Result<(), ExitError> {
        self.render_success(
            command,
            resolved,
            json!({ "plan": plan }),
            summary,
            Vec::new(),
        )
    }
}

fn string_flag(command: &ParsedCommand, name: &str) -> String {
    command.flags.get(name).cloned().unwrap_or_default()
}

fn string_flag_or(command: &ParsedCommand, name: &str, default: &str) -> String {
    command
        .flags
        .get(name)
        .filter(|value| !value.trim().is_empty())
        .cloned()
        .unwrap_or_else(|| default.to_string())
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

fn unsupported_group_e2ee_command(command: &str) -> ExitError {
    super::unsupported::unsupported_cutover_command(command, "group e2ee", "Phase 6")
}

fn bool_flag(command: &ParsedCommand, name: &str) -> Result<bool, ExitError> {
    let Some(raw) = command.flags.get(name) else {
        return Ok(false);
    };
    match raw.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Ok(true),
        "false" | "0" | "no" | "off" => Ok(false),
        _ => Err(ExitError::new(
            "invalid_argument",
            2,
            format!("--{name} must be a boolean."),
            "Use true or false.",
        )),
    }
}

fn provider_binary() -> String {
    String::new()
}

fn mls_data_dir(resolved: &Resolved) -> String {
    default_mls_data_dir(resolved)
        .to_string_lossy()
        .into_owned()
}

fn default_mls_data_dir(resolved: &Resolved) -> PathBuf {
    if resolved.paths.workspace_home_dir.trim().is_empty() {
        return PathBuf::from(".awiki-cli").join("mls");
    }
    Path::new(&resolved.paths.workspace_home_dir).join("mls")
}
