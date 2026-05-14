use super::{not_implemented_side_effect, App};
use crate::cli::ParsedCommand;
use crate::output::ExitError;
use serde_json::{json, Value};

impl App {
    pub fn run_group_create(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let name = string_flag(command, "name");
        if name.trim().is_empty() {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "group create requires --name.",
                "Usage: awiki-cli group create --name <NAME>",
            ));
        }
        if !self.globals.dry_run {
            return Err(not_implemented_side_effect("group create"));
        }
        self.render_success(
            "awiki-cli group create",
            &resolved,
            json!({
                "plan": {
                    "action": "group.create",
                    "identity": self.globals.identity,
                    "runtime_mode": resolved.runtime_mode,
                    "request": {
                        "IdentityName": self.globals.identity,
                        "Name": name,
                        "Description": string_flag(command, "description"),
                        "Discoverability": string_flag_or(command, "discoverability", "private"),
                        "AdmissionMode": string_flag_or(command, "admission-mode", "open-join"),
                        "MessageSecurityProfile": string_flag_or(command, "message-security-profile", "transport-protected"),
                        "E2EE": bool_flag(command, "e2ee")?.unwrap_or(false),
                        "Slug": string_flag(command, "slug"),
                        "Goal": string_flag(command, "goal"),
                        "Rules": string_flag(command, "rules"),
                        "MessagePrompt": string_flag(command, "message-prompt"),
                        "DocURL": string_flag(command, "doc-url"),
                        "AttachmentsAllowed": optional_bool_value(command, "attachments-allowed")?,
                        "MaxMembers": string_flag(command, "max-members"),
                        "MemberMaxMessages": optional_i64_value(command, "member-max-messages")?,
                        "MemberMaxTotalChars": optional_i64_value(command, "member-max-total-chars")?,
                    },
                }
            }),
            "Dry run: group create planned",
            Vec::new(),
        )
    }

    pub fn run_group_update(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let group = string_flag(command, "group");
        if group.trim().is_empty() {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "group update requires --group.",
                "Usage: awiki-cli group update --group <GROUP_DID>",
            ));
        }
        if !self.globals.dry_run {
            return Err(not_implemented_side_effect("group update"));
        }
        self.render_success(
            "awiki-cli group update",
            &resolved,
            json!({
                "plan": {
                    "action": "group.update",
                    "identity": self.globals.identity,
                    "runtime_mode": resolved.runtime_mode,
                    "request": {
                        "IdentityName": self.globals.identity,
                        "Group": group,
                        "Name": string_flag(command, "name"),
                        "Description": string_flag(command, "description"),
                        "Discoverability": string_flag(command, "discoverability"),
                        "AdmissionMode": string_flag(command, "admission-mode"),
                        "Slug": string_flag(command, "slug"),
                        "Goal": string_flag(command, "goal"),
                        "Rules": string_flag(command, "rules"),
                        "MessagePrompt": string_flag(command, "message-prompt"),
                        "DocURL": string_flag(command, "doc-url"),
                        "AttachmentsAllowed": optional_bool_value(command, "attachments-allowed")?,
                        "MaxMembers": string_flag(command, "max-members"),
                        "MemberMaxMessages": optional_i64_value(command, "member-max-messages")?,
                        "MemberMaxTotalChars": optional_i64_value(command, "member-max-total-chars")?,
                    },
                }
            }),
            "Dry run: group update planned",
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

fn optional_bool_value(command: &ParsedCommand, name: &str) -> Result<Value, ExitError> {
    if changed(command, name) {
        Ok(json!(bool_flag(command, name)?.unwrap_or(false)))
    } else {
        Ok(Value::Null)
    }
}

fn bool_flag(command: &ParsedCommand, name: &str) -> Result<Option<bool>, ExitError> {
    let Some(raw) = command.flags.get(name) else {
        return Ok(None);
    };
    match raw.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Ok(Some(true)),
        "false" | "0" | "no" | "off" => Ok(Some(false)),
        _ => Err(ExitError::new(
            "invalid_argument",
            2,
            format!("--{name} must be a boolean."),
            "Use true or false.",
        )),
    }
}

fn optional_i64_value(command: &ParsedCommand, name: &str) -> Result<Value, ExitError> {
    if !changed(command, name) {
        return Ok(Value::Null);
    }
    let raw = command.flags.get(name).cloned().unwrap_or_default();
    raw.trim().parse::<i64>().map(Value::from).map_err(|_| {
        ExitError::new(
            "invalid_argument",
            2,
            format!("--{name} must be an integer."),
            "Pass a numeric value after the flag.",
        )
    })
}

fn changed(command: &ParsedCommand, name: &str) -> bool {
    command.changed_flags.iter().any(|flag| flag == name)
}
