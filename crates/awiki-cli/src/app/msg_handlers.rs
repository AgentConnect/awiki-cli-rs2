use super::{not_implemented_side_effect, App};
use crate::cli::ParsedCommand;
use crate::config::Resolved;
use crate::identity;
use crate::message::{self, CommandResult, MessageError};
use crate::output::ExitError;
use serde_json::{json, Map, Value};
use std::fs;

impl App {
    pub fn run_msg_send(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let mut text = string_flag(command, "text");
        let to = string_flag(command, "to");
        let group = string_flag(command, "group");
        let file_path = string_flag(command, "file");
        let mime_type = string_flag(command, "mime-type");
        let message_type = string_flag(command, "type");
        let secure_mode = string_flag(command, "secure");
        let has_attachment = !file_path.trim().is_empty();

        if group.trim().is_empty() && to.trim().is_empty() {
            return Err(ExitError::new("invalid_argument", 2, "msg send requires either --to or --group.", "Usage: awiki-cli msg send --to <handle|did> --text \"Hello\" or awiki-cli msg send --group <group_did> --text \"Hello group\""));
        }
        if !group.trim().is_empty() && !to.trim().is_empty() {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "msg send accepts either --to or --group, but not both.",
                "Choose direct messaging with --to or group messaging with --group.",
            ));
        }
        if text.trim().is_empty() && !string_flag(command, "text-file").trim().is_empty() {
            text = read_text_file(command)?;
        }
        if !has_attachment && !mime_type.trim().is_empty() {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "mime_type requires an attachment file",
                "Use --mime-type only together with --file.",
            ));
        }
        if has_attachment && changed_flag(command, "type") {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "msg send does not accept --type together with --file.",
                "Attachment sends always use attachment manifests.",
            ));
        }
        if !has_attachment && text.trim().is_empty() {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "msg send requires --text or --text-file.",
                "Provide the message body via --text or --text-file.",
            ));
        }

        let resolved = self.resolve_config()?;
        if !self.globals.dry_run {
            let result = message::send(
                &resolved,
                &self.identity_manager(&resolved),
                message::SendRequest {
                    identity_name: self.globals.identity.clone(),
                    target: to,
                    group,
                    text,
                    message_type,
                    secure_mode,
                    file_path,
                    mime_type,
                    ..message::SendRequest::default()
                },
            )
            .map_err(|err| {
                message_exit(
                    err,
                    "Ensure the active identity is ready and the message service is reachable.",
                )
            })?;
            return self.render_message_result("awiki-cli msg send", &resolved, result);
        }

        let mut plan = Map::new();
        plan.insert(
            "action".to_string(),
            Value::String(
                if has_attachment {
                    "attachment.send"
                } else if group.trim().is_empty() {
                    "direct.send"
                } else {
                    "group.send"
                }
                .to_string(),
            ),
        );
        plan.insert(
            "identity".to_string(),
            Value::String(self.globals.identity.clone()),
        );
        plan.insert("target".to_string(), target_value(&to, &group, &resolved));
        plan.insert(
            "message_type".to_string(),
            Value::String(if has_attachment {
                "attachment_manifest".to_string()
            } else {
                default_string(&message_type, "text")
            }),
        );
        plan.insert(
            "runtime_mode".to_string(),
            Value::String(resolved.runtime_mode.clone()),
        );
        plan.insert(
            "transport".to_string(),
            Value::String(if has_attachment {
                "http".to_string()
            } else {
                resolved.runtime_mode.clone()
            }),
        );
        plan.insert("local_writes".to_string(), json!(["messages"]));
        if has_attachment {
            plan.insert(
                "attachment".to_string(),
                json!({
                    "path": file_path,
                    "mime_type": mime_type,
                    "caption": text,
                }),
            );
        }

        self.render_success(
            "awiki-cli msg send",
            &resolved,
            json!({ "plan": plan }),
            "Dry run: message send planned",
            Vec::new(),
        )
    }

    pub fn run_msg_attachment_download(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        require_flags(command, &["message-id", "output"])?;
        let with = string_flag(command, "with");
        let group = string_flag(command, "group");
        if with.trim().is_empty() && group.trim().is_empty() {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "attachment download requires either --with or --group",
                "Use --with <handle|did> for direct messages or --group <group_did> for group messages.",
            ));
        }
        if !with.trim().is_empty() && !group.trim().is_empty() {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "attachment download accepts either --with or --group, but not both",
                "Choose direct attachment download with --with or group attachment download with --group.",
            ));
        }
        let resolved = self.resolve_config()?;
        if !self.globals.dry_run {
            let result = message::download_attachment(
                &resolved,
                &self.identity_manager(&resolved),
                message::AttachmentDownloadRequest {
                    identity_name: self.globals.identity.clone(),
                    with,
                    group,
                    message_id: string_flag(command, "message-id"),
                    attachment_id: string_flag(command, "attachment-id"),
                    output_path: string_flag(command, "output"),
                },
            )
            .map_err(|err| {
                message_exit(
                    err,
                    "Make sure the message id, attachment id, and target context are correct.",
                )
            })?;
            return self.render_message_result(
                "awiki-cli msg attachment download",
                &resolved,
                result,
            );
        }
        let mut plan = Map::new();
        plan.insert(
            "action".to_string(),
            Value::String("download_attachment".to_string()),
        );
        plan.insert(
            "identity".to_string(),
            Value::String(self.globals.identity.clone()),
        );
        plan.insert("with".to_string(), Value::String(with.clone()));
        plan.insert("group".to_string(), Value::String(group));
        plan.insert(
            "message_id".to_string(),
            Value::String(string_flag(command, "message-id")),
        );
        plan.insert(
            "attachment_id".to_string(),
            Value::String(string_flag(command, "attachment-id")),
        );
        plan.insert(
            "output".to_string(),
            Value::String(string_flag(command, "output")),
        );
        plan.insert("transport".to_string(), Value::String("http".to_string()));
        insert_completed_handle(&mut plan, "with_handle", &with, &resolved.did_domain);
        self.render_success(
            "awiki-cli msg attachment download",
            &resolved,
            json!({ "plan": plan }),
            "Dry run: attachment download planned",
            Vec::new(),
        )
    }

    pub fn run_msg_inbox(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        if !self.globals.dry_run {
            let result = message::inbox(
                &resolved,
                &self.identity_manager(&resolved),
                message::InboxRequest {
                    identity_name: self.globals.identity.clone(),
                    scope: default_string(&string_flag(command, "scope"), "all"),
                    with: string_flag(command, "with"),
                    group: string_flag(command, "group"),
                    limit: int_flag(command, "limit", 20)?,
                    unread_only: bool_flag(command, "unread"),
                    mark_read: bool_flag(command, "mark-read"),
                },
            )
            .map_err(|err| {
                message_exit(
                    err,
                    "Ensure the active identity is ready and the message service is reachable.",
                )
            })?;
            return self.render_message_result("awiki-cli msg inbox", &resolved, result);
        }
        let with = string_flag(command, "with");
        let mut plan = Map::new();
        plan.insert("action".to_string(), Value::String("inbox.get".to_string()));
        plan.insert(
            "identity".to_string(),
            Value::String(self.globals.identity.clone()),
        );
        plan.insert(
            "runtime_mode".to_string(),
            Value::String(resolved.runtime_mode.clone()),
        );
        plan.insert(
            "scope".to_string(),
            Value::String(default_string(&string_flag(command, "scope"), "all")),
        );
        plan.insert("with".to_string(), Value::String(with.clone()));
        plan.insert(
            "group".to_string(),
            Value::String(string_flag(command, "group")),
        );
        plan.insert("limit".to_string(), json!(int_flag(command, "limit", 20)?));
        plan.insert(
            "mark_read".to_string(),
            Value::Bool(bool_flag(command, "mark-read")),
        );
        insert_completed_handle(&mut plan, "with_handle", &with, &resolved.did_domain);
        self.render_success(
            "awiki-cli msg inbox",
            &resolved,
            json!({ "plan": plan }),
            "Dry run: inbox read planned",
            Vec::new(),
        )
    }

    pub fn run_msg_history(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        require_flags(command, &["with"])?;
        let resolved = self.resolve_config()?;
        if !self.globals.dry_run {
            let result = message::history(
                &resolved,
                &self.identity_manager(&resolved),
                message::HistoryRequest {
                    identity_name: self.globals.identity.clone(),
                    with: string_flag(command, "with"),
                    limit: int_flag(command, "limit", 50)?,
                    cursor: string_flag(command, "cursor"),
                    ..message::HistoryRequest::default()
                },
            )
            .map_err(|err| {
                message_exit(
                    err,
                    "Ensure the peer is valid and the message service is reachable.",
                )
            })?;
            return self.render_message_result("awiki-cli msg history", &resolved, result);
        }
        let with = string_flag(command, "with");
        let mut plan = Map::new();
        plan.insert(
            "action".to_string(),
            Value::String("direct.get_history".to_string()),
        );
        plan.insert(
            "identity".to_string(),
            Value::String(self.globals.identity.clone()),
        );
        plan.insert(
            "runtime_mode".to_string(),
            Value::String(resolved.runtime_mode.clone()),
        );
        plan.insert("with".to_string(), Value::String(with.clone()));
        plan.insert("limit".to_string(), json!(int_flag(command, "limit", 50)?));
        plan.insert(
            "cursor".to_string(),
            Value::String(string_flag(command, "cursor")),
        );
        insert_completed_handle(&mut plan, "with_handle", &with, &resolved.did_domain);
        self.render_success(
            "awiki-cli msg history",
            &resolved,
            json!({ "plan": plan }),
            "Dry run: direct history read planned",
            Vec::new(),
        )
    }

    pub fn run_msg_mark_read(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        if command.args.is_empty() {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "msg mark-read requires at least one message id.",
                "Usage: awiki-cli msg mark-read <MESSAGE_ID...>",
            ));
        }
        let resolved = self.resolve_config()?;
        if !self.globals.dry_run {
            let result = message::mark_read(
                &resolved,
                &self.identity_manager(&resolved),
                message::MarkReadRequest {
                    identity_name: self.globals.identity.clone(),
                    message_ids: command.args.clone(),
                },
            )
            .map_err(|err| {
                message_exit(
                    err,
                    "Ensure the message ids are valid and the message service is reachable.",
                )
            })?;
            return self.render_message_result("awiki-cli msg mark-read", &resolved, result);
        }
        self.render_success(
            "awiki-cli msg mark-read",
            &resolved,
            json!({
                "plan": {
                    "action": "inbox.mark_read",
                    "identity": self.globals.identity,
                    "runtime_mode": resolved.runtime_mode,
                    "message_ids": command.args,
                }
            }),
            "Dry run: mark-read planned",
            Vec::new(),
        )
    }

    pub fn run_msg_secure_status(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let with = string_flag(command, "with");
        let resolved = self.resolve_config()?;
        if !self.globals.dry_run {
            let result = message::secure_status(
                &resolved,
                &self.identity_manager(&resolved),
                message::SecureStatusRequest {
                    identity_name: self.globals.identity.clone(),
                    with,
                },
            )
            .map_err(|err| {
                message_exit(
                    err,
                    "Make sure the active identity exists and the peer filter is valid.",
                )
            })?;
            return self.render_message_result("awiki-cli msg secure status", &resolved, result);
        }
        self.render_success(
            "awiki-cli msg secure status",
            &resolved,
            json!({
                "plan": {
                    "action": "msg.secure.status",
                    "identity": self.globals.identity,
                    "with": with,
                }
            }),
            "Dry run: secure status planned",
            Vec::new(),
        )
    }

    pub fn run_msg_secure_init(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let with = string_flag(command, "with");
        require_flags(command, &["with"])?;
        let resolved = self.resolve_config()?;
        if !self.globals.dry_run {
            let result = message::secure_init(
                &resolved,
                &self.identity_manager(&resolved),
                message::SecurePeerRequest {
                    identity_name: self.globals.identity.clone(),
                    with,
                },
            )
            .map_err(|err| {
                message_exit(
                    err,
                    "Make sure the target exists and the active identity has secure E2EE key material.",
                )
            })?;
            return self.render_message_result("awiki-cli msg secure init", &resolved, result);
        }
        self.render_success(
            "awiki-cli msg secure init",
            &resolved,
            json!({
                "plan": {
                    "action": "msg.secure.init",
                    "identity": self.globals.identity,
                    "with": with,
                }
            }),
            "Dry run: secure init planned",
            Vec::new(),
        )
    }

    pub fn run_msg_secure_repair(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let with = string_flag(command, "with");
        require_flags(command, &["with"])?;
        let resolved = self.resolve_config()?;
        if !self.globals.dry_run {
            let result = message::secure_repair(
                &resolved,
                &self.identity_manager(&resolved),
                message::SecurePeerRequest {
                    identity_name: self.globals.identity.clone(),
                    with,
                },
            )
            .map_err(|err| {
                message_exit(
                    err,
                    "Make sure the target exists and the active identity can rebuild secure state.",
                )
            })?;
            return self.render_message_result("awiki-cli msg secure repair", &resolved, result);
        }
        self.render_success(
            "awiki-cli msg secure repair",
            &resolved,
            json!({
                "plan": {
                    "action": "msg.secure.repair",
                    "identity": self.globals.identity,
                    "with": with,
                }
            }),
            "Dry run: secure repair planned",
            Vec::new(),
        )
    }

    pub fn run_msg_secure_failed(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        if !self.globals.dry_run {
            let result = message::secure_failed(
                &resolved,
                &self.identity_manager(&resolved),
                message::SecureStatusRequest {
                    identity_name: self.globals.identity.clone(),
                    ..message::SecureStatusRequest::default()
                },
            )
            .map_err(|err| {
                message_exit(
                    err,
                    "Make sure the active identity exists and local storage is readable.",
                )
            })?;
            return self.render_message_result("awiki-cli msg secure failed", &resolved, result);
        }
        self.render_success(
            "awiki-cli msg secure failed",
            &resolved,
            json!({
                "plan": {
                    "action": "msg.secure.failed",
                    "identity": self.globals.identity,
                }
            }),
            "Dry run: secure failed listing planned",
            Vec::new(),
        )
    }

    pub fn run_msg_secure_retry(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        self.run_msg_secure_outbox_plan(
            command,
            "awiki-cli msg secure retry",
            "msg.secure.retry",
            "Dry run: secure retry planned",
            "msg secure retry requires one outbox id.",
            "Usage: awiki-cli msg secure retry <OUTBOX_ID>",
        )
    }

    pub fn run_msg_secure_drop(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        self.run_msg_secure_outbox_plan(
            command,
            "awiki-cli msg secure drop",
            "msg.secure.drop",
            "Dry run: secure drop planned",
            "msg secure drop requires one outbox id.",
            "Usage: awiki-cli msg secure drop <OUTBOX_ID>",
        )
    }

    fn run_msg_secure_outbox_plan(
        &self,
        command: &ParsedCommand,
        command_name: &str,
        action: &str,
        summary: &str,
        missing_message: &str,
        usage: &str,
    ) -> Result<(), ExitError> {
        if command.args.len() != 1 {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                missing_message,
                usage,
            ));
        }
        let resolved = self.resolve_config()?;
        if !self.globals.dry_run {
            if action == "msg.secure.drop" {
                let result = message::secure_drop(
                    &resolved,
                    &self.identity_manager(&resolved),
                    message::SecureOutboxActionRequest {
                        identity_name: self.globals.identity.clone(),
                        outbox_id: command.args[0].clone(),
                    },
                )
                .map_err(|err| {
                    message_exit(
                        err,
                        "Make sure the outbox id exists for the active identity.",
                    )
                })?;
                return self.render_message_result(command_name, &resolved, result);
            }
            if action == "msg.secure.retry" {
                let result = message::secure_retry(
                    &resolved,
                    &self.identity_manager(&resolved),
                    message::SecureOutboxActionRequest {
                        identity_name: self.globals.identity.clone(),
                        outbox_id: command.args[0].clone(),
                    },
                )
                .map_err(|err| {
                    message_exit(
                        err,
                        "Make sure the outbox id exists and the active identity can reach the target service.",
                    )
                })?;
                return self.render_message_result(command_name, &resolved, result);
            }
            return Err(not_implemented_side_effect(
                command_name.trim_start_matches("awiki-cli "),
            ));
        }
        self.render_success(
            command_name,
            &resolved,
            json!({
                "plan": {
                    "action": action,
                    "identity": self.globals.identity,
                    "outbox_id": command.args[0],
                }
            }),
            summary,
            Vec::new(),
        )
    }

    fn render_message_result(
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

fn target_value(to: &str, group: &str, resolved: &Resolved) -> Value {
    if !group.trim().is_empty() {
        return json!({ "did": group, "kind": "group" });
    }
    let mut target = Map::new();
    target.insert("did".to_string(), Value::String(to.to_string()));
    target.insert("kind".to_string(), Value::String("direct".to_string()));
    insert_completed_handle(&mut target, "handle", to, &resolved.did_domain);
    Value::Object(target)
}

fn insert_completed_handle(
    map: &mut Map<String, Value>,
    key: &str,
    target: &str,
    did_domain: &str,
) {
    let completed = complete_bare_handle(target, did_domain);
    if completed != target.trim() {
        map.insert(key.to_string(), Value::String(completed));
    }
}

fn complete_bare_handle(target: &str, did_domain: &str) -> String {
    identity::complete_bare_handle(target, did_domain)
}

fn read_text_file(command: &ParsedCommand) -> Result<String, ExitError> {
    let path = string_flag(command, "text-file");
    fs::read_to_string(&path).map_err(|err| {
        ExitError::new(
            "invalid_argument",
            2,
            err.to_string(),
            "Make sure --text-file points to a readable file.",
        )
    })
}

fn string_flag(command: &ParsedCommand, name: &str) -> String {
    command.flags.get(name).cloned().unwrap_or_default()
}

fn default_string(value: &str, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

fn int_flag(command: &ParsedCommand, name: &str, fallback: i64) -> Result<i64, ExitError> {
    command
        .flags
        .get(name)
        .filter(|value| !value.trim().is_empty())
        .map(|value| {
            value.parse::<i64>().map_err(|_| {
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

fn bool_flag(command: &ParsedCommand, name: &str) -> bool {
    command
        .flags
        .get(name)
        .is_some_and(|value| value.eq_ignore_ascii_case("true"))
}

fn changed_flag(command: &ParsedCommand, name: &str) -> bool {
    command.changed_flags.iter().any(|flag| flag == name)
}

fn require_flags(command: &ParsedCommand, names: &[&str]) -> Result<(), ExitError> {
    let missing: Vec<_> = names
        .iter()
        .copied()
        .filter(|name| string_flag(command, name).trim().is_empty())
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    let quoted = missing
        .iter()
        .map(|name| format!("{name:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    Err(ExitError::new(
        "internal_error",
        1,
        format!("required flag(s) {quoted} not set"),
        "",
    ))
}

pub(super) fn message_exit(err: MessageError, hint: &str) -> ExitError {
    match err {
        MessageError::TargetRequired
        | MessageError::TextRequired
        | MessageError::AttachmentIdRequired
        | MessageError::AttachmentMessageInvalid
        | MessageError::AttachmentSenderRequired
        | MessageError::GroupRequired
        | MessageError::MemberRequired
        | MessageError::GroupOwnerCannotLeave
        | MessageError::FilePathRequired
        | MessageError::MimeTypeWithoutFile
        | MessageError::MessageIdRequired
        | MessageError::OutputPathRequired
        | MessageError::DownloadTargetNeeded
        | MessageError::DownloadTargetConflict
        | MessageError::MissingMessageServiceDid
        | MessageError::MissingAttachmentServiceDid
        | MessageError::InvalidAttachmentServiceEndpoint(_)
        | MessageError::Json(_) => ExitError::new(
            "invalid_argument",
            2,
            err.to_string(),
            "Check the message command arguments and try again.",
        ),
        MessageError::MessageNotFound | MessageError::AttachmentNotFound => {
            ExitError::new("not_found", 5, err.to_string(), hint)
        }
        MessageError::IdentityRequired(message) => ExitError::new(
            "identity_required",
            3,
            message,
            "Complete user setup with `awiki-cli id register --handle <handle> ...` or recover an existing handle before using `awiki-cli msg` commands.",
        ),
        MessageError::SecureNotSupported
        | MessageError::AttachmentNotSupported
        | MessageError::GroupNotSupported
        | MessageError::GroupE2eeSelfLeaveUnsupported
        | MessageError::TransportUnavailable(_) => {
            ExitError::new("not_implemented", 1, err.to_string(), hint)
        }
        MessageError::Service(service_err) => match () {
            _ if service_err.status_code == 400 || service_err.rpc_code == -32602 => {
                ExitError::new("invalid_argument", 2, service_err.to_string(), hint)
            }
            _ if service_err.status_code == 401 || service_err.rpc_code == -32000 => {
                ExitError::new(
                    "auth_required",
                    3,
                    service_err.to_string(),
                    "Use an identity with a valid JWT or DID WBA auth material.",
                )
            }
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
            _ if service_err.status_code == 409
                || matches!(service_err.rpc_code, -32003 | -32004) =>
            {
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
        },
        MessageError::Identity(err) => super::identity_exit(err),
        MessageError::Internal(message) => ExitError::new("internal_error", 1, message, hint),
    }
}
