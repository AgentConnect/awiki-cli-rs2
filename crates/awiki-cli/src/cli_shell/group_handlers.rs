use super::handle_helpers::complete_bare_handle;
use super::{msg_handlers::message_exit, App};
use crate::cli_output::ExitError;
use crate::cli_parser::ParsedCommand;
use crate::m_core_cli_adapter::message_result::{CommandResult, MessageAdapterError};
use serde_json::{json, Value};

const GROUP_E2EE_PROFILE: &str = crate::m_core_cli_adapter::groups::GROUP_E2EE_SECURITY_PROFILE;

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
        let message_security_profile =
            string_flag_or(command, "message-security-profile", "transport-protected");
        let (secure_required, warnings) =
            group_secure_requirement(command, Some(message_security_profile.as_str()))?;
        if !self.globals.dry_run {
            let request = crate::m_core_cli_adapter::groups::GroupCreateRequest {
                identity_name: self.globals.identity.clone(),
                name,
                description: string_flag(command, "description"),
                discoverability: string_flag_or(command, "discoverability", "private"),
                admission_mode: string_flag_or(command, "admission-mode", "open-join"),
                message_security_profile: if secure_required {
                    GROUP_E2EE_PROFILE.to_string()
                } else {
                    message_security_profile.clone()
                },
                secure_required,
                e2ee: secure_required,
                slug: string_flag(command, "slug"),
                goal: string_flag(command, "goal"),
                rules: string_flag(command, "rules"),
                message_prompt: string_flag(command, "message-prompt"),
                doc_url: string_flag(command, "doc-url"),
                attachments_allowed: optional_bool(command, "attachments-allowed")?,
                max_members: string_flag(command, "max-members"),
                member_max_messages: optional_i64(command, "member-max-messages")?,
                member_max_total_chars: optional_i64(command, "member-max-total-chars")?,
            };
            let client = crate::m_core_cli_adapter::build_im_client(
                &resolved,
                crate::m_core_cli_adapter::cli_identity_selector(&self.globals.identity),
            )?;
            let mut result = crate::m_core_cli_adapter::groups::create_group_via_im_core(
                &resolved, &client, request,
            )
            .map_err(|err| group_cutover_exit(err, "group.create"))?;
            result.warnings.extend(warnings);
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
                        "MessageSecurityProfile": if secure_required { GROUP_E2EE_PROFILE } else { message_security_profile.as_str() },
                        "Secure": if secure_required { "required" } else { "off" },
                        "E2EE": secure_required,
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
            warnings,
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
            let client = crate::m_core_cli_adapter::build_im_client(
                &resolved,
                crate::m_core_cli_adapter::cli_identity_selector(&self.globals.identity),
            )?;
            let result =
                crate::m_core_cli_adapter::groups::get_group_via_im_core(&resolved, &client, group)
                    .map_err(|err| group_cutover_exit(err, "group.get"))?;
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
            let request = crate::m_core_cli_adapter::groups::GroupJoinRequest {
                identity_name: self.globals.identity.clone(),
                group,
                reason_text: string_flag(command, "reason"),
            };
            let client = crate::m_core_cli_adapter::build_im_client(
                &resolved,
                crate::m_core_cli_adapter::cli_identity_selector(&self.globals.identity),
            )?;
            let result = crate::m_core_cli_adapter::groups::join_group_via_im_core(
                &resolved, &client, request,
            )
            .map_err(|err| group_cutover_exit(err, "group.join"))?;
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
        self.run_group_member_mutation(command, "add", "group add", "group.add", "member", false)
    }

    pub fn run_group_remove(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        self.run_group_member_mutation(command, "kick", "group remove", "group.remove", "", true)
    }

    fn run_group_member_mutation(
        &self,
        command: &ParsedCommand,
        public_action: &str,
        command_name: &str,
        command_id: &str,
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
        let (secure_required, warnings) = group_secure_requirement(command, None)?;
        if !self.globals.dry_run {
            let request = crate::m_core_cli_adapter::groups::GroupMemberRequest {
                identity_name: self.globals.identity.clone(),
                group,
                member,
                role: string_flag_or(command, "role", role_fallback),
                reason_text: if include_reason {
                    string_flag(command, "reason")
                } else {
                    String::new()
                },
                secure_required,
                e2ee: secure_required,
                leave_request_id: String::new(),
            };
            let client = crate::m_core_cli_adapter::build_im_client(
                &resolved,
                crate::m_core_cli_adapter::cli_identity_selector(&self.globals.identity),
            )?;
            let mut result = if public_action == "add" {
                crate::m_core_cli_adapter::groups::add_group_member_via_im_core(
                    &resolved, &client, request,
                )
            } else {
                crate::m_core_cli_adapter::groups::remove_group_member_via_im_core(
                    &resolved, &client, request,
                )
            }
            .map_err(|err| group_membership_cutover_exit(err, command_id))?;
            result.warnings.extend(warnings);
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
                "Secure": if secure_required { "required" } else { "off" },
                "E2EE": secure_required,
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
            warnings,
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
        let (secure_required, warnings) = group_secure_requirement(command, None)?;
        if !self.globals.dry_run {
            let request = crate::m_core_cli_adapter::groups::GroupLeaveRequest {
                identity_name: self.globals.identity.clone(),
                group,
                reason_text: string_flag(command, "reason"),
                secure_required,
                e2ee: secure_required,
            };
            let client = crate::m_core_cli_adapter::build_im_client(
                &resolved,
                crate::m_core_cli_adapter::cli_identity_selector(&self.globals.identity),
            )?;
            let mut result = crate::m_core_cli_adapter::groups::leave_group_via_im_core(
                &resolved, &client, request,
            )
            .map_err(|err| group_cutover_exit(err, "group.leave"))?;
            result.warnings.extend(warnings);
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
                        "Secure": if secure_required { "required" } else { "off" },
                        "E2EE": secure_required,
                    },
                }
            }),
            "Dry run: group leave planned",
            warnings,
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
            let request = crate::m_core_cli_adapter::groups::GroupUpdateRequest {
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
            };
            let client = crate::m_core_cli_adapter::build_im_client(
                &resolved,
                crate::m_core_cli_adapter::cli_identity_selector(&self.globals.identity),
            )?;
            let result = crate::m_core_cli_adapter::groups::update_group_via_im_core(
                &resolved, &client, request,
            )
            .map_err(|err| group_cutover_exit(err, "group.update"))?;
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
            let client = crate::m_core_cli_adapter::build_im_client(
                &resolved,
                crate::m_core_cli_adapter::cli_identity_selector(&self.globals.identity),
            )?;
            let result = crate::m_core_cli_adapter::groups::list_groups_via_im_core(
                &resolved, &client, limit,
            )
            .map_err(|err| group_cutover_exit(err, "group.list"))?;
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
            let client = crate::m_core_cli_adapter::build_im_client(
                &resolved,
                crate::m_core_cli_adapter::cli_identity_selector(&self.globals.identity),
            )?;
            let result = crate::m_core_cli_adapter::groups::group_members_via_im_core(
                &resolved, &client, group, limit,
            )
            .map_err(|err| group_cutover_exit(err, "group.members"))?;
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
            let cursor = string_flag(command, "cursor");
            let client = crate::m_core_cli_adapter::build_im_client(
                &resolved,
                crate::m_core_cli_adapter::cli_identity_selector(&self.globals.identity),
            )?;
            let result = crate::m_core_cli_adapter::groups::group_messages_via_im_core(
                &resolved, &client, group, limit, cursor,
            )
            .map_err(|err| group_cutover_exit(err, "group.messages"))?;
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

    pub fn run_group_secure_status(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        self.run_group_secure(command, "status", false, false)
    }

    pub fn run_group_secure_repair(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        if bool_flag(command, "explain")?.unwrap_or(false) {
            return Err(super::unsupported::unsupported_cutover_command(
                "group.secure.repair",
                "group secure diagnostics",
                "future diagnostics plan",
            ));
        }
        self.run_group_secure(command, "repair", true, false)
    }

    pub fn run_group_secure_diagnostics(&self) -> Result<(), ExitError> {
        Err(super::unsupported::unsupported_cutover_command(
            "group.secure.diagnostics",
            "group secure diagnostics",
            "future diagnostics plan",
        ))
    }

    pub fn run_group_e2ee_status_alias(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        self.run_group_secure(command, "status", false, true)
    }

    pub fn run_group_e2ee_repair_alias(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        self.run_group_secure(command, "repair", true, true)
    }

    fn run_group_secure(
        &self,
        command: &ParsedCommand,
        action: &str,
        side_effect: bool,
        deprecated_alias: bool,
    ) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let group = required_string_flag(
            command,
            "group",
            &format!("group secure {action}"),
            &format!("Usage: awiki-cli group secure {action} --group <GROUP_DID>"),
        )?;
        let warnings = if deprecated_alias {
            vec![format!(
                "group e2ee {action} is deprecated; use group secure {action}."
            )]
        } else {
            Vec::new()
        };
        if self.globals.dry_run {
            let mut plan = json!({
                "action": format!("secure.group.{action}"),
                "identity": self.globals.identity,
                "runtime_mode": resolved.runtime_mode,
                "group": group,
            });
            if side_effect {
                plan["local_writes"] = json!(["group_mls_state"]);
            }
            return self.render_success(
                &format!("awiki-cli group secure {action}"),
                &resolved,
                json!({ "plan": plan }),
                &format!("Dry run: group secure {action} planned"),
                warnings,
            );
        }
        let client = crate::m_core_cli_adapter::build_im_client(
            &resolved,
            crate::m_core_cli_adapter::cli_identity_selector(&self.globals.identity),
        )?;
        let mut result = if action == "status" {
            crate::m_core_cli_adapter::groups::group_secure_status_via_im_core(&client, group)
        } else {
            crate::m_core_cli_adapter::groups::group_secure_repair_via_im_core(&client, group)
        }
        .map_err(|err| group_cutover_exit(err, &format!("group.secure.{action}")))?;
        result.warnings.extend(warnings);
        self.render_group_result(
            &format!("awiki-cli group secure {action}"),
            &resolved,
            result,
        )
    }

    fn render_group_result(
        &self,
        command: &str,
        resolved: &crate::workspace_config::Resolved,
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

fn group_exit(err: MessageAdapterError) -> ExitError {
    message_exit(
        err,
        "Ensure the active identity is ready and the message service is reachable.",
    )
}

fn group_cutover_exit(err: MessageAdapterError, command: &str) -> ExitError {
    match err {
        MessageAdapterError::GroupNotSupported => group_e2ee_unavailable(command),
        err => group_exit(err),
    }
}

fn group_membership_exit(err: MessageAdapterError) -> ExitError {
    message_exit(
        err,
        "Make sure the group and member exist and the active identity has the owner role required for membership changes.",
    )
}

fn group_membership_cutover_exit(err: MessageAdapterError, command: &str) -> ExitError {
    match err {
        MessageAdapterError::GroupNotSupported => group_e2ee_unavailable(command),
        err => group_membership_exit(err),
    }
}

fn group_e2ee_unavailable(command: &str) -> ExitError {
    ExitError {
        exit_code: 2,
        detail: crate::cli_output::ErrorDetail {
            code: "unsupported_capability".to_string(),
            message: format!("group E2EE is unavailable for {command}."),
            hint: "Use --secure required only when group E2EE is available for this identity, workspace, and service.".to_string(),
            retryable: false,
            details: json!({
                "command": command,
                "capability": "group-e2ee",
                "cutover_status": "im_core",
            }),
        },
    }
}

fn group_secure_requirement(
    command: &ParsedCommand,
    message_security_profile: Option<&str>,
) -> Result<(bool, Vec<String>), ExitError> {
    let mut warnings = Vec::new();
    let secure = string_flag(command, "secure");
    let secure_required = match secure.trim().to_ascii_lowercase().as_str() {
        "" | "off" | "false" | "default" => false,
        "required" => true,
        "on" | "true" | "e2ee" | "group-e2ee" => {
            warnings.push(format!(
                "--secure {} is deprecated; use --secure required.",
                secure.trim()
            ));
            true
        }
        value => {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                format!("unsupported --secure value {value:?}."),
                "Use --secure required or leave it unset.",
            ))
        }
    };
    let e2ee = bool_flag(command, "e2ee")?.unwrap_or(false);
    if e2ee {
        warnings.push("--e2ee is deprecated; use --secure required.".to_string());
    }
    let profile_e2ee =
        message_security_profile.is_some_and(|profile| profile.trim() == GROUP_E2EE_PROFILE);
    if profile_e2ee {
        warnings.push(
            "--message-security-profile group-e2ee is deprecated; use --secure required."
                .to_string(),
        );
    }
    Ok((secure_required || e2ee || profile_e2ee, warnings))
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
