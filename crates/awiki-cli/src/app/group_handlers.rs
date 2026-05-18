use super::{msg_handlers::message_exit, App};
use crate::cli::ParsedCommand;
use crate::identity;
use crate::message::{self, CommandResult};
use crate::output::ExitError;
use serde_json::{json, Value};

impl App {
    pub fn run_group_create(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let name = string_flag(command, "name");
        if !changed(command, "name") {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "group create requires --name.",
                "Usage: awiki-cli group create --name <NAME>",
            ));
        }
        if !self.globals.dry_run {
            let result = message::create_group(
                &resolved,
                &self.identity_manager(&resolved),
                message::GroupCreateRequest {
                    identity_name: self.globals.identity.clone(),
                    name,
                    description: string_flag(command, "description"),
                    discoverability: string_flag_or(command, "discoverability", "private"),
                    admission_mode: string_flag_or(command, "admission-mode", "open-join"),
                    message_security_profile: string_flag_or(
                        command,
                        "message-security-profile",
                        "transport-protected",
                    ),
                    e2ee: bool_flag(command, "e2ee")?.unwrap_or(false),
                    slug: string_flag(command, "slug"),
                    goal: string_flag(command, "goal"),
                    rules: string_flag(command, "rules"),
                    message_prompt: string_flag(command, "message-prompt"),
                    doc_url: string_flag(command, "doc-url"),
                    attachments_allowed: optional_bool(command, "attachments-allowed")?,
                    max_members: string_flag(command, "max-members"),
                    member_max_messages: optional_i64(command, "member-max-messages")?,
                    member_max_total_chars: optional_i64(command, "member-max-total-chars")?,
                },
            )
            .map_err(group_exit)?;
            return self.render_group_result("awiki-cli group create", &resolved, result);
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

    pub fn run_group_get(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let group = required_string_flag(
            command,
            "group",
            "group get",
            "Usage: awiki-cli group get --group <GROUP_DID>",
        )?;
        if !self.globals.dry_run {
            let result = message::get_group(
                &resolved,
                &self.identity_manager(&resolved),
                message::GroupGetRequest {
                    identity_name: self.globals.identity.clone(),
                    group,
                },
            )
            .map_err(group_exit)?;
            return self.render_group_result("awiki-cli group get", &resolved, result);
        }
        self.render_success(
            "awiki-cli group get",
            &resolved,
            json!({
                "plan": {
                    "action": "group.show",
                    "identity": self.globals.identity,
                    "runtime_mode": resolved.runtime_mode,
                    "group": group,
                }
            }),
            "Dry run: group show planned",
            Vec::new(),
        )
    }

    pub fn run_group_join(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let group = required_string_flag(
            command,
            "group",
            "group join",
            "Usage: awiki-cli group join --group <GROUP_DID>",
        )?;
        if !self.globals.dry_run {
            let result = message::join_group(
                &resolved,
                &self.identity_manager(&resolved),
                message::GroupJoinRequest {
                    identity_name: self.globals.identity.clone(),
                    group,
                    reason_text: string_flag(command, "reason"),
                },
            )
            .map_err(group_exit)?;
            return self.render_group_result("awiki-cli group join", &resolved, result);
        }
        self.render_success(
            "awiki-cli group join",
            &resolved,
            json!({
                "plan": {
                    "action": "group.join",
                    "identity": self.globals.identity,
                    "runtime_mode": resolved.runtime_mode,
                    "request": {
                        "IdentityName": self.globals.identity,
                        "Group": group,
                        "ReasonText": string_flag(command, "reason"),
                    },
                }
            }),
            "Dry run: group join planned",
            Vec::new(),
        )
    }

    pub fn run_group_add(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        self.run_group_member_mutation(command, "add", "group add", "member", false)
    }

    pub fn run_group_remove(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        self.run_group_member_mutation(command, "kick", "group remove", "", true)
    }

    fn run_group_member_mutation(
        &self,
        command: &ParsedCommand,
        public_action: &str,
        command_name: &str,
        role_fallback: &str,
        include_reason: bool,
    ) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let group = required_string_flag(
            command,
            "group",
            command_name,
            &format!("Usage: awiki-cli {command_name} --group <GROUP_DID> --member <MEMBER>"),
        )?;
        let member = required_string_flag(
            command,
            "member",
            command_name,
            &format!("Usage: awiki-cli {command_name} --group <GROUP_DID> --member <MEMBER>"),
        )?;
        if !self.globals.dry_run {
            let request = message::GroupMemberRequest {
                identity_name: self.globals.identity.clone(),
                group,
                member,
                role: string_flag_or(command, "role", role_fallback),
                reason_text: if include_reason {
                    string_flag(command, "reason")
                } else {
                    String::new()
                },
                e2ee: bool_flag(command, "e2ee")?.unwrap_or(false),
                leave_request_id: String::new(),
            };
            let mut result = if public_action == "add" {
                message::add_group_member(&resolved, &self.identity_manager(&resolved), request)
            } else {
                message::remove_group_member(&resolved, &self.identity_manager(&resolved), request)
            }
            .map_err(group_membership_exit)?;
            result.summary = if public_action == "add" {
                "Added member to group".to_string()
            } else {
                "Removed member from group".to_string()
            };
            return self.render_group_result(
                &format!("awiki-cli {command_name}"),
                &resolved,
                result,
            );
        }
        let reason = if include_reason {
            string_flag(command, "reason")
        } else {
            String::new()
        };
        let mut plan = json!({
            "action": format!("group.{public_action}"),
            "identity": self.globals.identity,
            "runtime_mode": resolved.runtime_mode,
            "request": {
                "IdentityName": self.globals.identity,
                "Group": group,
                "Member": member,
                "Role": string_flag_or(command, "role", role_fallback),
                "ReasonText": reason,
                "E2EE": bool_flag(command, "e2ee")?.unwrap_or(false),
                "LeaveRequestID": "",
            },
        });
        let member_handle = complete_bare_handle(&member, &resolved.did_domain);
        if member_handle != member.trim() {
            plan["member_handle"] = json!(member_handle);
        }
        self.render_success(
            &format!("awiki-cli {command_name}"),
            &resolved,
            json!({ "plan": plan }),
            "Dry run: group membership change planned",
            Vec::new(),
        )
    }

    pub fn run_group_leave(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let group = required_string_flag(
            command,
            "group",
            "group leave",
            "Usage: awiki-cli group leave --group <GROUP_DID>",
        )?;
        if !self.globals.dry_run {
            let result = message::leave_group(
                &resolved,
                &self.identity_manager(&resolved),
                message::GroupLeaveRequest {
                    identity_name: self.globals.identity.clone(),
                    group,
                    reason_text: string_flag(command, "reason"),
                    e2ee: bool_flag(command, "e2ee")?.unwrap_or(false),
                },
            )
            .map_err(group_exit)?;
            return self.render_group_result("awiki-cli group leave", &resolved, result);
        }
        self.render_success(
            "awiki-cli group leave",
            &resolved,
            json!({
                "plan": {
                    "action": "group.leave",
                    "identity": self.globals.identity,
                    "runtime_mode": resolved.runtime_mode,
                    "request": {
                        "IdentityName": self.globals.identity,
                        "Group": group,
                        "ReasonText": string_flag(command, "reason"),
                        "E2EE": bool_flag(command, "e2ee")?.unwrap_or(false),
                    },
                }
            }),
            "Dry run: group leave planned",
            Vec::new(),
        )
    }

    pub fn run_group_update(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let group = required_string_flag(
            command,
            "group",
            "group update",
            "Usage: awiki-cli group update --group <GROUP_DID>",
        )?;
        if !self.globals.dry_run {
            let result = message::update_group(
                &resolved,
                &self.identity_manager(&resolved),
                message::GroupUpdateRequest {
                    identity_name: self.globals.identity.clone(),
                    group,
                    name: string_flag(command, "name"),
                    description: string_flag(command, "description"),
                    discoverability: string_flag(command, "discoverability"),
                    admission_mode: string_flag(command, "admission-mode"),
                    slug: string_flag(command, "slug"),
                    goal: string_flag(command, "goal"),
                    rules: string_flag(command, "rules"),
                    message_prompt: string_flag(command, "message-prompt"),
                    doc_url: string_flag(command, "doc-url"),
                    attachments_allowed: optional_bool(command, "attachments-allowed")?,
                    max_members: string_flag(command, "max-members"),
                    member_max_messages: optional_i64(command, "member-max-messages")?,
                    member_max_total_chars: optional_i64(command, "member-max-total-chars")?,
                },
            )
            .map_err(group_exit)?;
            return self.render_group_result("awiki-cli group update", &resolved, result);
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

    pub fn run_group_list(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let limit = int_flag(command, "limit", 50)?;
        if !self.globals.dry_run {
            let result = message::list_groups(
                &resolved,
                &self.identity_manager(&resolved),
                message::GroupListRequest {
                    identity_name: self.globals.identity.clone(),
                    limit,
                },
            )
            .map_err(group_exit)?;
            return self.render_group_result("awiki-cli group list", &resolved, result);
        }
        self.render_success(
            "awiki-cli group list",
            &resolved,
            json!({
                "plan": {
                    "action": "group.list",
                    "identity": self.globals.identity,
                    "runtime_mode": resolved.runtime_mode,
                    "request": {
                        "IdentityName": self.globals.identity,
                        "Limit": limit,
                    },
                }
            }),
            "Dry run: group list planned",
            Vec::new(),
        )
    }

    pub fn run_group_members(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let group = required_string_flag(
            command,
            "group",
            "group members",
            "Usage: awiki-cli group members --group <GROUP_DID>",
        )?;
        let limit = int_flag(command, "limit", 100)?;
        if !self.globals.dry_run {
            let result = message::group_members(
                &resolved,
                &self.identity_manager(&resolved),
                message::GroupMembersRequest {
                    identity_name: self.globals.identity.clone(),
                    group,
                    limit,
                },
            )
            .map_err(group_exit)?;
            return self.render_group_result("awiki-cli group members", &resolved, result);
        }
        self.render_success(
            "awiki-cli group members",
            &resolved,
            json!({
                "plan": {
                    "action": "group.list_members",
                    "identity": self.globals.identity,
                    "runtime_mode": resolved.runtime_mode,
                    "request": {
                        "IdentityName": self.globals.identity,
                        "Group": group,
                        "Limit": limit,
                    },
                }
            }),
            "Dry run: group members planned",
            Vec::new(),
        )
    }

    pub fn run_group_messages(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let group = required_string_flag(
            command,
            "group",
            "group messages",
            "Usage: awiki-cli group messages --group <GROUP_DID>",
        )?;
        let limit = int_flag(command, "limit", 50)?;
        if !self.globals.dry_run {
            let result = message::group_messages(
                &resolved,
                &self.identity_manager(&resolved),
                message::GroupMessagesRequest {
                    identity_name: self.globals.identity.clone(),
                    group,
                    limit,
                    cursor: string_flag(command, "cursor"),
                    skip: 0,
                },
            )
            .map_err(group_exit)?;
            return self.render_group_result("awiki-cli group messages", &resolved, result);
        }
        self.render_success(
            "awiki-cli group messages",
            &resolved,
            json!({
                "plan": {
                    "action": "group.list_messages",
                    "identity": self.globals.identity,
                    "runtime_mode": resolved.runtime_mode,
                    "request": {
                        "IdentityName": self.globals.identity,
                        "Group": group,
                        "Limit": limit,
                        "Cursor": string_flag(command, "cursor"),
                        "Skip": 0,
                    },
                }
            }),
            "Dry run: group messages planned",
            Vec::new(),
        )
    }

    fn render_group_result(
        &self,
        command: &str,
        resolved: &crate::config::Resolved,
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

fn group_exit(err: message::MessageError) -> ExitError {
    message_exit(
        err,
        "Ensure the active identity is ready and the message service is reachable.",
    )
}

fn group_membership_exit(err: message::MessageError) -> ExitError {
    message_exit(
        err,
        "Make sure the group and member exist and the active identity has the owner role required for membership changes.",
    )
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
    if !changed(command, name) {
        return Err(ExitError::new(
            "invalid_argument",
            2,
            format!("{command_name} requires --{name}."),
            help,
        ));
    }
    Ok(value)
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

fn int_flag(command: &ParsedCommand, name: &str, fallback: i64) -> Result<i64, ExitError> {
    command
        .flags
        .get(name)
        .filter(|value| !value.trim().is_empty())
        .map(|value| {
            value.trim().parse::<i64>().map_err(|_| {
                ExitError::new(
                    "invalid_argument",
                    2,
                    format!("--{name} must be an integer."),
                    "Pass a numeric value after the flag.",
                )
            })
        })
        .unwrap_or(Ok(fallback))
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

fn optional_bool(command: &ParsedCommand, name: &str) -> Result<Option<bool>, ExitError> {
    if changed(command, name) {
        Ok(Some(bool_flag(command, name)?.unwrap_or(false)))
    } else {
        Ok(None)
    }
}

fn optional_i64(command: &ParsedCommand, name: &str) -> Result<Option<i64>, ExitError> {
    if !changed(command, name) {
        return Ok(None);
    }
    let raw = command.flags.get(name).cloned().unwrap_or_default();
    raw.trim().parse::<i64>().map(Some).map_err(|_| {
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

fn complete_bare_handle(target: &str, did_domain: &str) -> String {
    identity::complete_bare_handle(target, did_domain)
}
