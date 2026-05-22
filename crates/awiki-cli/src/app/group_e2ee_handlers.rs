use super::App;
use crate::cli::ParsedCommand;
use crate::config::Resolved;
use crate::message::{
    self, GroupE2eePendingRequest, GroupE2eePublishKeyPackageRequest,
    GroupE2eeRecoverMemberRequest, GroupE2eeStatusRequest, GroupE2eeUpdateKeyRequest,
};
use crate::output::ExitError;
use serde_json::{json, Value};

const GROUP_E2EE_PROFILE: &str = "anp.group.e2ee.v1";
const GROUP_E2EE_SECURITY_PROFILE: &str = "group-e2ee";
const DEFAULT_DEVICE: &str = "default";
const REPAIR_SCOPE: &str = "compare local MLS status to service head, safely finalize accepted pending commits, replay welcome/commit notices, and fail closed on unrecoverable gaps";

impl App {
    pub fn run_group_e2ee_status(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let plan = json!({
            "action": "group.e2ee.status",
            "identity": self.globals.identity,
            "runtime_mode": resolved.runtime_mode,
            "profile": GROUP_E2EE_PROFILE,
            "security_profile": GROUP_E2EE_SECURITY_PROFILE,
            "provider": "exec",
            "binary": provider_binary(),
            "mls_data_dir": mls_data_dir(&resolved),
            "group": string_flag(command, "group"),
            "discovery_advertised": false,
        });
        if !self.globals.dry_run {
            let mut result = message::inspect_group_e2ee_status(
                &resolved,
                &self.identity_manager(&resolved),
                GroupE2eeStatusRequest {
                    identity_name: self.globals.identity.clone(),
                    group: string_flag(command, "group"),
                    limit: 50,
                },
            )
            .map_err(|err| {
                group_e2ee_message_exit(
                    err,
                    "Install anp-mls, set AWIKI_ANP_MLS_BINARY, and ensure message-service group E2EE APIs are enabled for focused validation.",
                )
            })?;
            if let Some(data) = result.data.as_object_mut() {
                data.insert("plan".to_string(), plan);
            }
            return self.render_success(
                "awiki-cli group e2ee status",
                &resolved,
                result.data,
                &result.summary,
                result.warnings,
            );
        }
        self.render_group_e2ee_plan(
            "awiki-cli group e2ee status",
            &resolved,
            plan,
            "Dry run: group e2ee status planned",
        )
    }

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
        if !self.globals.dry_run {
            let mut result = message::publish_group_e2ee_key_package(
                &resolved,
                &self.identity_manager(&resolved),
                GroupE2eePublishKeyPackageRequest {
                    identity_name: self.globals.identity.clone(),
                    device_id: string_flag_or(command, "device", DEFAULT_DEVICE),
                    group: string_flag(command, "group"),
                    purpose,
                    contract_test,
                },
            )
            .map_err(|err| {
                group_e2ee_message_exit(
                    err,
                    "Install anp-mls, set AWIKI_ANP_MLS_BINARY, pass --group when --recovery is used, and ensure message-service group E2EE APIs are enabled.",
                )
            })?;
            if let Some(data) = result.data.as_object_mut() {
                data.insert("plan".to_string(), plan);
            }
            return self.render_success(
                "awiki-cli group e2ee publish-key-package",
                &resolved,
                result.data,
                &result.summary,
                result.warnings,
            );
        }
        self.render_group_e2ee_plan(
            "awiki-cli group e2ee publish-key-package",
            &resolved,
            plan,
            "Dry run: group e2ee key package publish planned",
        )
    }

    pub fn run_group_e2ee_pending(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let plan = json!({
            "action": "group.e2ee.pending",
            "identity": self.globals.identity,
            "runtime_mode": resolved.runtime_mode,
            "provider": "exec",
            "mls_data_dir": mls_data_dir(&resolved),
            "group": string_flag(command, "group"),
        });
        if !self.globals.dry_run {
            let mut result = message::pull_group_e2ee_notices(
                &resolved,
                &self.identity_manager(&resolved),
                GroupE2eePendingRequest {
                    identity_name: self.globals.identity.clone(),
                    group: string_flag(command, "group"),
                    limit: 50,
                },
            )
            .map_err(|err| {
                group_e2ee_message_exit(
                    err,
                    "Ensure message-service group E2EE test flag is enabled for focused validation; discovery remains hidden by default.",
                )
            })?;
            if let Some(data) = result.data.as_object_mut() {
                data.insert("plan".to_string(), plan);
            }
            return self.render_success(
                "awiki-cli group e2ee pending",
                &resolved,
                result.data,
                &result.summary,
                result.warnings,
            );
        }
        self.render_group_e2ee_plan(
            "awiki-cli group e2ee pending",
            &resolved,
            plan,
            "Dry run: group e2ee pending planned",
        )
    }

    pub fn run_group_e2ee_repair(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let plan = json!({
            "action": "group.e2ee.repair",
            "identity": self.globals.identity,
            "runtime_mode": resolved.runtime_mode,
            "provider": "exec",
            "mls_data_dir": mls_data_dir(&resolved),
            "group": string_flag(command, "group"),
            "scope": REPAIR_SCOPE,
        });
        if !self.globals.dry_run {
            let mut result = message::repair_group_e2ee_notices(
                &resolved,
                &self.identity_manager(&resolved),
                &self.globals.identity,
                &string_flag(command, "group"),
                50,
            )
            .map_err(|err| {
                group_e2ee_message_exit(
                    err,
                    "Install anp-mls, set AWIKI_ANP_MLS_BINARY, and ensure message-service group E2EE APIs are enabled for focused validation.",
                )
            })?;
            if let Some(data) = result.data.as_object_mut() {
                data.insert("plan".to_string(), plan);
            }
            return self.render_success(
                "awiki-cli group e2ee repair",
                &resolved,
                result.data,
                &result.summary,
                result.warnings,
            );
        }
        self.render_group_e2ee_plan(
            "awiki-cli group e2ee repair",
            &resolved,
            plan,
            "Dry run: group e2ee repair planned",
        )
    }

    pub fn run_group_e2ee_process_leave_request(
        &self,
        command: &ParsedCommand,
    ) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
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
        if !self.globals.dry_run {
            let mut result = message::process_group_e2ee_leave_request(
                &resolved,
                &self.identity_manager(&resolved),
                message::GroupE2eeProcessLeaveRequest {
                    identity_name: self.globals.identity.clone(),
                    group,
                    member,
                    leave_request_id,
                    reason_text: reason,
                },
            )
            .map_err(|err| {
                group_e2ee_message_exit(
                    err,
                    "Ensure the leave request exists, the active identity can remove members, and anp-mls/message-service group E2EE APIs are enabled.",
                )
            })?;
            if let Some(data) = result.data.as_object_mut() {
                data.insert("plan".to_string(), plan);
            }
            return self.render_success(
                "awiki-cli group e2ee process-leave-request",
                &resolved,
                result.data,
                &result.summary,
                result.warnings,
            );
        }
        self.render_group_e2ee_plan(
            "awiki-cli group e2ee process-leave-request",
            &resolved,
            plan,
            "Dry run: group e2ee leave request process planned",
        )
    }

    pub fn run_group_e2ee_recover_member(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
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
        if !self.globals.dry_run {
            let mut result = message::recover_group_e2ee_member(
                &resolved,
                &self.identity_manager(&resolved),
                GroupE2eeRecoverMemberRequest {
                    identity_name: self.globals.identity.clone(),
                    group,
                    member,
                    device_id: device,
                },
            )
            .map_err(|err| {
                group_e2ee_message_exit(
                    err,
                    "Ensure the target remains an active P4 member, has published a --recovery --group KeyPackage, and anp-mls/message-service PR-B3 APIs are enabled.",
                )
            })?;
            if let Some(data) = result.data.as_object_mut() {
                data.insert("plan".to_string(), plan);
            }
            return self.render_success(
                "awiki-cli group e2ee recover-member",
                &resolved,
                result.data,
                &result.summary,
                result.warnings,
            );
        }
        self.render_group_e2ee_plan(
            "awiki-cli group e2ee recover-member",
            &resolved,
            plan,
            "Dry run: group e2ee recover-member planned",
        )
    }

    pub fn run_group_e2ee_update_key(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
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
        if !self.globals.dry_run {
            let mut result = message::update_group_e2ee_key(
                &resolved,
                &self.identity_manager(&resolved),
                GroupE2eeUpdateKeyRequest {
                    identity_name: self.globals.identity.clone(),
                    group,
                    member,
                    device_id: device,
                },
            )
            .map_err(|err| {
                group_e2ee_message_exit(
                    err,
                    "Ensure the target remains active, has published an --update --group KeyPackage, the active identity is the owner, and anp-mls/message-service PR-B3 update APIs are enabled.",
                )
            })?;
            if let Some(data) = result.data.as_object_mut() {
                data.insert("plan".to_string(), plan);
            }
            return self.render_success(
                "awiki-cli group e2ee update-key",
                &resolved,
                result.data,
                &result.summary,
                result.warnings,
            );
        }
        self.render_group_e2ee_plan(
            "awiki-cli group e2ee update-key",
            &resolved,
            plan,
            "Dry run: group e2ee update-key planned",
        )
    }

    pub fn run_group_e2ee_rejoin(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
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
        if !self.globals.dry_run {
            let request = message::GroupMemberRequest {
                identity_name: self.globals.identity.clone(),
                group,
                member,
                role,
                reason_text: String::new(),
                e2ee: true,
                leave_request_id: String::new(),
            };
            let mut result = message::add_group_member(
                &resolved,
                &self.identity_manager(&resolved),
                request,
            )
            .map_err(|err| {
                group_e2ee_message_exit(
                    err,
                    "Removed/left rejoin requires a fresh normal KeyPackage published after removal/leave, then owner-only `group add --e2ee`; do not use recover-member for removed/left members.",
                )
            })?;
            if let Some(data) = result.data.as_object_mut() {
                data.insert("plan".to_string(), plan);
            }
            return self.render_success(
                "awiki-cli group e2ee rejoin",
                &resolved,
                result.data,
                &result.summary,
                result.warnings,
            );
        }
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

fn group_e2ee_message_exit(err: message::MessageError, hint: &str) -> ExitError {
    match err {
        message::MessageError::TargetRequired
        | message::MessageError::TextRequired
        | message::MessageError::AttachmentIdRequired
        | message::MessageError::AttachmentMessageInvalid
        | message::MessageError::AttachmentSenderRequired
        | message::MessageError::GroupRequired
        | message::MessageError::MemberRequired
        | message::MessageError::GroupOwnerCannotLeave
        | message::MessageError::FilePathRequired
        | message::MessageError::MimeTypeWithoutFile
        | message::MessageError::MessageIdRequired
        | message::MessageError::OutputPathRequired
        | message::MessageError::DownloadTargetNeeded
        | message::MessageError::DownloadTargetConflict
        | message::MessageError::MissingMessageServiceDid
        | message::MessageError::MissingAttachmentServiceDid
        | message::MessageError::InvalidAttachmentServiceEndpoint(_)
        | message::MessageError::Json(_) => ExitError::new(
            "invalid_argument",
            2,
            err.to_string(),
            "Check the group E2EE command arguments and try again.",
        ),
        message::MessageError::MessageNotFound | message::MessageError::AttachmentNotFound => {
            ExitError::new("not_found", 5, err.to_string(), hint)
        }
        message::MessageError::IdentityRequired(message) => ExitError::new(
            "identity_required",
            3,
            message,
            "Complete user setup with `awiki-cli id register --handle <handle> ...` or recover an existing handle before using group E2EE diagnostic commands.",
        ),
        message::MessageError::SecureNotSupported => ExitError::new(
            "unsupported_mode",
            1,
            err.to_string(),
            "Secure messaging is currently supported only for direct text messaging.",
        ),
        message::MessageError::GroupE2eeSelfLeaveUnsupported => ExitError::new(
            "unsupported_mode",
            1,
            err.to_string(),
            "For PR-A group E2EE, ask the group owner to remove the member; self-leave requires a future epoch-advancing leave-request flow.",
        ),
        message::MessageError::TransportUnavailable(_) => ExitError::new(
            "transport_unavailable",
            1,
            err.to_string(),
            "Start the websocket listener/daemon or switch runtime.mode back to http.",
        ),
        message::MessageError::AttachmentNotSupported | message::MessageError::GroupNotSupported => {
            ExitError::new("not_implemented", 1, err.to_string(), hint)
        }
        message::MessageError::Service(service_err) => group_e2ee_service_exit(service_err, hint),
        message::MessageError::Identity(identity_err) => group_e2ee_identity_exit(identity_err),
        message::MessageError::Internal(message) => {
            ExitError::new("internal_error", 1, message, hint)
        }
    }
}

fn group_e2ee_service_exit(
    service_err: crate::identity::wire::ServiceError,
    hint: &str,
) -> ExitError {
    match () {
        _ if service_err.status_code == 400 || service_err.rpc_code == -32602 => {
            ExitError::new("invalid_argument", 2, service_err.to_string(), hint)
        }
        _ if service_err.status_code == 401 || service_err.rpc_code == -32000 => ExitError::new(
            "auth_required",
            3,
            service_err.to_string(),
            "Use an identity with a valid JWT or DID WBA auth material.",
        ),
        _ if service_err.rpc_code == 1401 => ExitError::new(
            "auth_required",
            3,
            service_err.to_string(),
            "Use an identity with a valid JWT or DID WBA auth material.",
        ),
        _ if service_err.status_code == 404
            || service_err.rpc_code == -32002
            || matches!(service_err.rpc_code, 6000 | 6005 | 6007 | 6012) =>
        {
            ExitError::new("not_found", 5, service_err.to_string(), hint)
        }
        _ if service_err.status_code == 409 || matches!(service_err.rpc_code, -32003 | -32004) => {
            ExitError::new("conflict", 1, service_err.to_string(), hint)
        }
        _ if matches!(
            service_err.rpc_code,
            6006 | 6008 | 6009 | 6010 | 6011 | 6013
        ) =>
        {
            ExitError::new("invalid_argument", 2, service_err.to_string(), hint)
        }
        _ => ExitError::new("internal_error", 1, service_err.to_string(), hint),
    }
}

fn group_e2ee_identity_exit(err: crate::identity::IdentityError) -> ExitError {
    match err {
        crate::identity::IdentityError::InvalidInput(message) => ExitError::new(
            "invalid_argument",
            2,
            message,
            "Run `awiki-cli id list` to inspect available identities.",
        ),
        crate::identity::IdentityError::NotFound(message)
        | crate::identity::IdentityError::LegacyNotFound(message)
        | crate::identity::IdentityError::NoDefaultIdentity(message) => ExitError::new(
            "not_found",
            5,
            message,
            "Run `awiki-cli id list` to inspect available identities.",
        ),
        crate::identity::IdentityError::Conflict(message) => ExitError::new(
            "conflict",
            1,
            message,
            "Use a different --identity value if the alias is already occupied.",
        ),
        crate::identity::IdentityError::AuthRequired(message) => ExitError::new(
            "auth_required",
            3,
            message,
            "Use an identity with valid DID key material, or run `awiki-cli id refresh-token` / `awiki-cli id register` / `awiki-cli id recover` first.",
        ),
        crate::identity::IdentityError::Service(service_err) => {
            group_e2ee_service_exit(service_err, "Use an identity with a valid JWT or DID WBA auth material.")
        }
        crate::identity::IdentityError::Io(error) => ExitError::new(
            "internal_error",
            1,
            error.to_string(),
            "Run `awiki-cli doctor` to inspect configuration and storage paths.",
        ),
        crate::identity::IdentityError::Json(error) => ExitError::new(
            "internal_error",
            1,
            error.to_string(),
            "Run `awiki-cli doctor` to inspect configuration and storage paths.",
        ),
        crate::identity::IdentityError::Internal(message) => ExitError::new(
            "internal_error",
            1,
            message,
            "Run `awiki-cli doctor` to inspect configuration and storage paths.",
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

fn provider_binary() -> String {
    String::new()
}

fn mls_data_dir(resolved: &Resolved) -> String {
    message::default_mls_data_dir(resolved)
        .to_string_lossy()
        .into_owned()
}
