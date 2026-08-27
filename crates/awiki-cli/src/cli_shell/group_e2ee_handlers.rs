use super::App;
use crate::cli_output::ExitError;
use crate::cli_parser::ParsedCommand;
use crate::workspace_config::Resolved;
use serde_json::{json, Value};

impl App {
    pub fn run_group_e2ee_publish_key_package(
        &self,
        command: &ParsedCommand,
    ) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let mut purpose = string_flag_or(command, "purpose", "normal");
        if bool_flag(command, "recovery")? {
            purpose = "recovery".to_string();
        }
        if purpose.trim().is_empty() {
            purpose = "normal".to_string();
        }
        let device = required_string_flag(
            command,
            "device",
            "group e2ee publish-key-package",
            "Usage: awiki-cli group e2ee publish-key-package --device <PROTOCOL_DEVICE_ID>",
        )?;
        let group = string_flag(command, "group");
        if !self.globals.dry_run {
            let client = crate::m_core_cli_adapter::build_im_client(
                &resolved,
                crate::m_core_cli_adapter::cli_identity_selector(&self.globals.identity),
            )?;
            let result = crate::m_core_cli_adapter::groups::publish_group_key_package_via_im_core(
                &client,
                crate::m_core_cli_adapter::groups::GroupKeyPackagePublishRequest {
                    identity_name: self.globals.identity.clone(),
                    group,
                    purpose,
                    device,
                },
            )
            .map_err(|err| group_e2ee_exit(err, "group.e2ee.publish-key-package"))?;
            return self.render_success(
                "awiki-cli group e2ee publish-key-package",
                &resolved,
                result.data,
                &result.summary,
                result.warnings,
            );
        }
        let contract_test = bool_flag(command, "contract-test")?;
        let plan = json!({
            "action": "group.e2ee.publish_key_package",
            "identity": self.globals.identity,
            "runtime_mode": resolved.runtime_mode,
            "provider": "internal",
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

    pub async fn run_group_e2ee_publish_key_package_async(
        &self,
        command: &ParsedCommand,
    ) -> Result<(), ExitError> {
        if self.globals.dry_run {
            return self.run_group_e2ee_publish_key_package(command);
        }
        let resolved = self.resolve_config()?;
        let mut purpose = string_flag_or(command, "purpose", "normal");
        if bool_flag(command, "recovery")? {
            purpose = "recovery".to_string();
        }
        if purpose.trim().is_empty() {
            purpose = "normal".to_string();
        }
        let device = required_string_flag(
            command,
            "device",
            "group e2ee publish-key-package",
            "Usage: awiki-cli group e2ee publish-key-package --device <PROTOCOL_DEVICE_ID>",
        )?;
        let group = string_flag(command, "group");
        let client = crate::m_core_cli_adapter::build_im_client_async(
            &resolved,
            crate::m_core_cli_adapter::cli_identity_selector(&self.globals.identity),
        )
        .await?;
        let result =
            crate::m_core_cli_adapter::groups::publish_group_key_package_via_im_core_async(
                &client,
                crate::m_core_cli_adapter::groups::GroupKeyPackagePublishRequest {
                    identity_name: self.globals.identity.clone(),
                    group,
                    purpose,
                    device,
                },
            )
            .await
            .map_err(|err| group_e2ee_exit(err, "group.e2ee.publish-key-package"))?;
        self.render_success(
            "awiki-cli group e2ee publish-key-package",
            &resolved,
            result.data,
            &result.summary,
            result.warnings,
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

fn group_e2ee_exit(
    err: crate::m_core_cli_adapter::message_result::MessageAdapterError,
    command: &str,
) -> ExitError {
    match err {
        crate::m_core_cli_adapter::message_result::MessageAdapterError::GroupNotSupported => {
            ExitError {
                exit_code: 2,
                detail: crate::cli_output::ErrorDetail {
                    code: "unsupported_capability".to_string(),
                    message: format!("group E2EE is unavailable for {command}."),
                    hint: "Use internal group E2EE commands only when the workspace and service have Group E2EE enabled.".to_string(),
                    retryable: false,
                    details: json!({
                        "command": command,
                        "capability": "group-e2ee",
                        "cutover_status": "im_core",
                    }),
                },
            }
        }
        err => super::msg_handlers::message_exit(
            err,
            "Ensure the active identity is ready and the message service is reachable.",
        ),
    }
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
