use super::App;
use crate::cli::ParsedCommand;
use crate::config::Resolved;
use crate::identity;
use crate::im_core_adapter::message_result::{
    CommandResult, IdentityErrorKind, MessageAdapterError,
};
use crate::output::ExitError;
use im_core::prelude::{MessageBody, MessageKind};
use serde_json::{json, Map, Value};

struct MsgSendPlan<'a> {
    identity: &'a str,
    to: &'a str,
    group: &'a str,
    text: &'a str,
    message_type: &'a str,
    file_path: &'a str,
    mime_type: &'a str,
    has_attachment: bool,
}

impl App {
    pub fn run_msg_send(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        self.run_msg_send_im_core(command)
    }

    fn run_msg_send_im_core(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let file_path = string_flag(command, "file");
        let mime_type = string_flag(command, "mime-type");
        if file_path.trim().is_empty() && !mime_type.trim().is_empty() {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "mime_type requires an attachment file",
                "Use --mime-type only together with --file.",
            ));
        }

        let resolved = self.resolve_config_for_workspace()?;
        if !file_path.trim().is_empty() {
            return self
                .run_msg_attachment_send_im_core(command, &resolved, &file_path, &mime_type);
        }
        let request =
            crate::im_core_adapter::messages::send_message_request(command, &resolved.did_domain)?;
        let (text, message_type) = send_text_plan_fields(&request)?;
        if self.globals.dry_run {
            return self.render_msg_send_plan(
                &resolved,
                MsgSendPlan {
                    identity: &self.globals.identity,
                    to: &string_flag(command, "to"),
                    group: &string_flag(command, "group"),
                    text: &text,
                    message_type,
                    file_path: "",
                    mime_type: "",
                    has_attachment: false,
                },
            );
        }

        let manager = self.identity_manager(&resolved);
        let client = crate::im_core_adapter::build_im_client(
            &resolved,
            &manager,
            crate::im_core_adapter::cli_identity_selector(&self.globals.identity),
        )?;
        let result = crate::im_core_adapter::messages::send_text_via_im_core(
            &resolved,
            &manager,
            &self.globals.identity,
            &client,
            request,
        )
        .map_err(|err| {
            message_exit(
                err,
                "Ensure the active identity is ready and the message service is reachable.",
            )
        })?;
        self.render_message_result("awiki-cli msg send", &resolved, result)
    }

    fn run_msg_attachment_send_im_core(
        &self,
        command: &ParsedCommand,
        resolved: &Resolved,
        file_path: &str,
        mime_type: &str,
    ) -> Result<(), ExitError> {
        let (target, request) = crate::im_core_adapter::messages::send_attachment_request(
            command,
            &resolved.did_domain,
        )?;
        let caption = request.caption.clone().unwrap_or_default();
        if self.globals.dry_run {
            return self.render_msg_send_plan(
                resolved,
                MsgSendPlan {
                    identity: &self.globals.identity,
                    to: &string_flag(command, "to"),
                    group: &string_flag(command, "group"),
                    text: &caption,
                    message_type: "",
                    file_path,
                    mime_type,
                    has_attachment: true,
                },
            );
        }

        let manager = self.identity_manager(resolved);
        let client = crate::im_core_adapter::build_im_client(
            resolved,
            &manager,
            crate::im_core_adapter::cli_identity_selector(&self.globals.identity),
        )?;
        let result = crate::im_core_adapter::messages::send_attachment_via_im_core(
            resolved, &client, target, request,
        )
        .or_else(|err| {
            if !should_fallback_attachment_send(&err) {
                return Err(err);
            }
            legacy_attachment_send(resolved, &manager, command)
        })
        .map_err(|err| {
            message_exit(
                err,
                "Ensure the active identity is ready and the attachment service is reachable.",
            )
        })?;
        self.render_message_result("awiki-cli msg send", resolved, result)
    }

    fn render_msg_send_plan(
        &self,
        resolved: &Resolved,
        input: MsgSendPlan<'_>,
    ) -> Result<(), ExitError> {
        let mut plan = Map::new();
        plan.insert(
            "action".to_string(),
            Value::String(
                if input.has_attachment {
                    "attachment.send"
                } else if input.group.trim().is_empty() {
                    "direct.send"
                } else {
                    "group.send"
                }
                .to_string(),
            ),
        );
        plan.insert(
            "identity".to_string(),
            Value::String(input.identity.to_string()),
        );
        plan.insert(
            "target".to_string(),
            target_value(input.to, input.group, resolved),
        );
        plan.insert(
            "message_type".to_string(),
            Value::String(if input.has_attachment {
                "attachment_manifest".to_string()
            } else {
                default_string(input.message_type, "text")
            }),
        );
        plan.insert(
            "runtime_mode".to_string(),
            Value::String(resolved.runtime_mode.clone()),
        );
        plan.insert(
            "transport".to_string(),
            Value::String(if input.has_attachment {
                "http".to_string()
            } else {
                resolved.runtime_mode.clone()
            }),
        );
        plan.insert("local_writes".to_string(), json!(["messages"]));
        if input.has_attachment {
            plan.insert(
                "attachment".to_string(),
                json!({
                    "path": input.file_path,
                    "mime_type": input.mime_type,
                    "caption": input.text,
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
        let resolved = self.resolve_config_for_workspace()?;
        let request = crate::im_core_adapter::messages::download_attachment_request(
            command,
            &resolved.did_domain,
        )?;
        if self.globals.dry_run {
            return self.render_msg_attachment_download_plan(command, &resolved);
        }

        let manager = self.identity_manager(&resolved);
        let client = crate::im_core_adapter::build_im_client(
            &resolved,
            &manager,
            crate::im_core_adapter::cli_identity_selector(&self.globals.identity),
        )?;
        let result = crate::im_core_adapter::messages::download_attachment_via_im_core(
            &resolved, &client, request,
        )
        .or_else(|err| {
            if !should_fallback_attachment_download(&err) {
                return Err(err);
            }
            legacy_attachment_download(&resolved, &manager, command)
        })
        .map_err(|err| {
            message_exit(
                err,
                "Ensure the message id, attachment id, and attachment service are reachable.",
            )
        })?;
        self.render_message_result("awiki-cli msg attachment download", &resolved, result)
    }

    fn render_msg_attachment_download_plan(
        &self,
        command: &ParsedCommand,
        resolved: &Resolved,
    ) -> Result<(), ExitError> {
        let with = string_flag(command, "with");
        let group = string_flag(command, "group");
        let mut plan = Map::new();
        plan.insert(
            "action".to_string(),
            Value::String("attachment.download".to_string()),
        );
        plan.insert(
            "identity".to_string(),
            Value::String(self.globals.identity.clone()),
        );
        plan.insert("target".to_string(), target_value(&with, &group, resolved));
        plan.insert(
            "message_id".to_string(),
            Value::String(string_flag(command, "message-id")),
        );
        plan.insert(
            "attachment_id".to_string(),
            Value::String(string_flag(command, "attachment-id")),
        );
        plan.insert(
            "output_path".to_string(),
            Value::String(string_flag(command, "output")),
        );
        plan.insert("overwrite".to_string(), Value::Bool(true));
        plan.insert("transport".to_string(), Value::String("http".to_string()));
        plan.insert("local_writes".to_string(), json!(["output_file"]));
        self.render_success(
            "awiki-cli msg attachment download",
            resolved,
            json!({ "plan": plan }),
            "Dry run: attachment download planned",
            Vec::new(),
        )
    }

    pub fn run_msg_inbox(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        if !string_flag(command, "with").trim().is_empty()
            || !string_flag(command, "group").trim().is_empty()
        {
            return Err(super::unsupported::unsupported_cutover_command(
                "msg.inbox",
                "inbox-target-filters",
                "Phase 3",
            ));
        }
        if bool_flag(command, "mark-read") {
            return Err(super::unsupported::unsupported_cutover_command(
                "msg.inbox",
                "inbox-mark-read-side-effect",
                "Phase 3",
            ));
        }
        self.run_msg_inbox_im_core(command)
    }

    fn run_msg_inbox_im_core(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let query = crate::im_core_adapter::messages::inbox_query(command)?;
        if !self.globals.dry_run {
            let manager = self.identity_manager(&resolved);
            let client = crate::im_core_adapter::build_im_client(
                &resolved,
                &manager,
                crate::im_core_adapter::cli_identity_selector(&self.globals.identity),
            )?;
            let result = crate::im_core_adapter::messages::read_inbox_via_im_core(
                &resolved,
                &manager,
                &client,
                &self.globals.identity,
                query,
            )
            .map_err(|err| {
                message_exit(
                    err,
                    "Ensure the active identity is ready and the message service is reachable.",
                )
            })?;
            return self.render_message_result("awiki-cli msg inbox", &resolved, result);
        }
        self.render_msg_inbox_plan(command, &resolved)
    }

    fn render_msg_inbox_plan(
        &self,
        command: &ParsedCommand,
        resolved: &Resolved,
    ) -> Result<(), ExitError> {
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
        self.run_msg_history_im_core(command)
    }

    fn run_msg_history_im_core(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let (thread, query) =
            crate::im_core_adapter::messages::history_request(command, &resolved.did_domain)?;
        if !self.globals.dry_run {
            let manager = self.identity_manager(&resolved);
            let client = crate::im_core_adapter::build_im_client(
                &resolved,
                &manager,
                crate::im_core_adapter::cli_identity_selector(&self.globals.identity),
            )?;
            let result = crate::im_core_adapter::messages::read_history_via_im_core(
                &resolved,
                &manager,
                &client,
                &self.globals.identity,
                thread,
                query,
            )
            .map_err(|err| {
                message_exit(
                    err,
                    "Ensure the peer is valid and the message service is reachable.",
                )
            })?;
            return self.render_message_result("awiki-cli msg history", &resolved, result);
        }
        self.render_msg_history_plan(command, &resolved)
    }

    fn render_msg_history_plan(
        &self,
        command: &ParsedCommand,
        resolved: &Resolved,
    ) -> Result<(), ExitError> {
        let (thread, query) =
            crate::im_core_adapter::messages::history_request(command, &resolved.did_domain)?;
        let mut plan = Map::new();
        let summary = match thread {
            im_core::prelude::ThreadRef::Direct(_) => {
                let with = string_flag(command, "with");
                plan.insert(
                    "action".to_string(),
                    Value::String("direct.get_history".to_string()),
                );
                plan.insert("with".to_string(), Value::String(with.clone()));
                insert_completed_handle(&mut plan, "with_handle", &with, &resolved.did_domain);
                "Dry run: direct history read planned"
            }
            im_core::prelude::ThreadRef::Group(group) => {
                plan.insert(
                    "action".to_string(),
                    Value::String("group.list_messages".to_string()),
                );
                plan.insert(
                    "group".to_string(),
                    Value::String(group.as_str().to_string()),
                );
                "Dry run: group history read planned"
            }
            im_core::prelude::ThreadRef::Thread(_) => {
                return Err(ExitError::new(
                    "unsupported_capability",
                    2,
                    "thread history is not supported by the im-core CLI cutover path.",
                    "Use --with <handle|did> or --group <group_did>.",
                ));
            }
        };
        plan.insert(
            "identity".to_string(),
            Value::String(self.globals.identity.clone()),
        );
        plan.insert(
            "runtime_mode".to_string(),
            Value::String(resolved.runtime_mode.clone()),
        );
        plan.insert("limit".to_string(), json!(query.limit.0));
        plan.insert(
            "cursor".to_string(),
            Value::String(
                query
                    .cursor
                    .as_ref()
                    .map(|cursor| cursor.as_str().to_string())
                    .unwrap_or_default(),
            ),
        );
        self.render_success(
            "awiki-cli msg history",
            &resolved,
            json!({ "plan": plan }),
            summary,
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
        let resolved = self.resolve_config_for_workspace()?;
        if !self.globals.dry_run {
            let manager = self.identity_manager(&resolved);
            let client = crate::im_core_adapter::build_im_client(
                &resolved,
                &manager,
                crate::im_core_adapter::cli_identity_selector(&self.globals.identity),
            )?;
            let result = crate::im_core_adapter::messages::mark_read_via_im_core(
                &resolved,
                &manager,
                &client,
                &self.globals.identity,
                command.args.clone(),
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
        unsupported_secure_command(command, "msg.secure.status")
    }

    pub fn run_msg_secure_init(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        unsupported_secure_command(command, "msg.secure.init")
    }

    pub fn run_msg_secure_repair(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        unsupported_secure_command(command, "msg.secure.repair")
    }

    pub fn run_msg_secure_failed(&self) -> Result<(), ExitError> {
        Err(super::unsupported::unsupported_cutover_command(
            "msg.secure.failed",
            "secure-direct",
            "Phase 6",
        ))
    }

    pub fn run_msg_secure_retry(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        unsupported_secure_command(command, "msg.secure.retry")
    }

    pub fn run_msg_secure_drop(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        unsupported_secure_command(command, "msg.secure.drop")
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

fn unsupported_secure_command(
    _command: &ParsedCommand,
    command_name: &str,
) -> Result<(), ExitError> {
    Err(super::unsupported::unsupported_cutover_command(
        command_name,
        "secure-direct",
        "Phase 6",
    ))
}

fn should_fallback_attachment_send(err: &MessageAdapterError) -> bool {
    matches!(
        err,
        MessageAdapterError::AttachmentNotSupported | MessageAdapterError::GroupNotSupported
    )
}

fn should_fallback_attachment_download(err: &MessageAdapterError) -> bool {
    matches!(err, MessageAdapterError::AttachmentNotSupported)
}

fn legacy_attachment_send(
    resolved: &Resolved,
    manager: &crate::identity::Manager,
    command: &ParsedCommand,
) -> Result<CommandResult, MessageAdapterError> {
    crate::message::send(resolved, manager, legacy_send_request(command))
        .map(legacy_command_result)
        .map_err(message_error_to_adapter)
}

fn legacy_attachment_download(
    resolved: &Resolved,
    manager: &crate::identity::Manager,
    command: &ParsedCommand,
) -> Result<CommandResult, MessageAdapterError> {
    crate::message::download_attachment(resolved, manager, legacy_download_request(command))
        .map(legacy_command_result)
        .map_err(message_error_to_adapter)
}

fn legacy_command_result(result: crate::message::CommandResult) -> CommandResult {
    CommandResult {
        data: result.data,
        summary: result.summary,
        warnings: result.warnings,
    }
}

fn legacy_send_request(command: &ParsedCommand) -> crate::message::SendRequest {
    crate::message::SendRequest {
        identity_name: command.globals.identity.clone(),
        target: string_flag(command, "to"),
        group: string_flag(command, "group"),
        text: string_flag(command, "text"),
        message_type: string_flag(command, "type"),
        secure_mode: string_flag(command, "secure"),
        file_path: string_flag(command, "file"),
        mime_type: string_flag(command, "mime-type"),
    }
}

fn legacy_download_request(command: &ParsedCommand) -> crate::message::AttachmentDownloadRequest {
    crate::message::AttachmentDownloadRequest {
        identity_name: command.globals.identity.clone(),
        with: string_flag(command, "with"),
        group: string_flag(command, "group"),
        message_id: string_flag(command, "message-id"),
        attachment_id: string_flag(command, "attachment-id"),
        output_path: string_flag(command, "output"),
    }
}

fn message_error_to_adapter(err: crate::message::MessageError) -> MessageAdapterError {
    match err {
        crate::message::MessageError::TargetRequired => MessageAdapterError::TargetRequired,
        crate::message::MessageError::GroupRequired => MessageAdapterError::GroupRequired,
        crate::message::MessageError::MemberRequired => MessageAdapterError::MemberRequired,
        crate::message::MessageError::GroupOwnerCannotLeave => {
            MessageAdapterError::GroupOwnerCannotLeave
        }
        crate::message::MessageError::TextRequired => MessageAdapterError::TextRequired,
        crate::message::MessageError::FilePathRequired => MessageAdapterError::FilePathRequired,
        crate::message::MessageError::MimeTypeWithoutFile => {
            MessageAdapterError::MimeTypeWithoutFile
        }
        crate::message::MessageError::MessageIdRequired => MessageAdapterError::MessageIdRequired,
        crate::message::MessageError::OutputPathRequired => MessageAdapterError::OutputPathRequired,
        crate::message::MessageError::DownloadTargetNeeded => {
            MessageAdapterError::DownloadTargetNeeded
        }
        crate::message::MessageError::DownloadTargetConflict => {
            MessageAdapterError::DownloadTargetConflict
        }
        crate::message::MessageError::AttachmentNotFound => MessageAdapterError::AttachmentNotFound,
        crate::message::MessageError::AttachmentIdRequired => {
            MessageAdapterError::AttachmentIdRequired
        }
        crate::message::MessageError::AttachmentMessageInvalid => {
            MessageAdapterError::AttachmentMessageInvalid
        }
        crate::message::MessageError::AttachmentSenderRequired => {
            MessageAdapterError::AttachmentSenderRequired
        }
        crate::message::MessageError::TransportUnavailable(detail) => {
            MessageAdapterError::TransportUnavailable(detail)
        }
        crate::message::MessageError::SecureNotSupported => MessageAdapterError::SecureNotSupported,
        crate::message::MessageError::AttachmentNotSupported => {
            MessageAdapterError::AttachmentNotSupported
        }
        crate::message::MessageError::GroupNotSupported => MessageAdapterError::GroupNotSupported,
        crate::message::MessageError::GroupE2eeSelfLeaveUnsupported => {
            MessageAdapterError::GroupE2eeSelfLeaveUnsupported
        }
        crate::message::MessageError::MessageNotFound => MessageAdapterError::MessageNotFound,
        crate::message::MessageError::IdentityRequired(message) => {
            MessageAdapterError::IdentityRequired(message)
        }
        crate::message::MessageError::Service(service) => {
            MessageAdapterError::Service(service.into())
        }
        crate::message::MessageError::Identity(identity) => {
            MessageAdapterError::Identity(identity.into())
        }
        crate::message::MessageError::Internal(message) => MessageAdapterError::Internal(message),
        crate::message::MessageError::InvalidAttachmentServiceEndpoint(message) => {
            MessageAdapterError::InvalidAttachmentServiceEndpoint(message)
        }
        crate::message::MessageError::MissingMessageServiceDid => {
            MessageAdapterError::MissingMessageServiceDid
        }
        crate::message::MessageError::MissingAttachmentServiceDid => {
            MessageAdapterError::MissingAttachmentServiceDid
        }
        crate::message::MessageError::Json(message) => MessageAdapterError::Json(message),
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

fn send_text_plan_fields(
    request: &im_core::prelude::SendMessageRequest,
) -> Result<(String, &'static str), ExitError> {
    match &request.body {
        MessageBody::Text { text, kind } => Ok((text.clone(), message_type_for_kind(kind))),
        MessageBody::Attachment { .. } => Err(ExitError::new(
            "unsupported_capability",
            2,
            "attachments are not supported by the Phase 1 IM Core adapter.",
            "Use the existing legacy attachment command path until attachment migration starts.",
        )),
    }
}

fn message_type_for_kind(kind: &MessageKind) -> &'static str {
    match kind {
        MessageKind::Text => "text",
        MessageKind::Markdown => "markdown",
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

pub(super) fn message_exit(err: impl Into<MessageAdapterError>, hint: &str) -> ExitError {
    let err = err.into();
    match err {
        MessageAdapterError::TargetRequired
        | MessageAdapterError::TextRequired
        | MessageAdapterError::AttachmentIdRequired
        | MessageAdapterError::AttachmentMessageInvalid
        | MessageAdapterError::AttachmentSenderRequired
        | MessageAdapterError::GroupRequired
        | MessageAdapterError::MemberRequired
        | MessageAdapterError::GroupOwnerCannotLeave
        | MessageAdapterError::FilePathRequired
        | MessageAdapterError::MimeTypeWithoutFile
        | MessageAdapterError::MessageIdRequired
        | MessageAdapterError::OutputPathRequired
        | MessageAdapterError::DownloadTargetNeeded
        | MessageAdapterError::DownloadTargetConflict
        | MessageAdapterError::MissingMessageServiceDid
        | MessageAdapterError::MissingAttachmentServiceDid
        | MessageAdapterError::InvalidAttachmentServiceEndpoint(_)
        | MessageAdapterError::Json(_) => ExitError::new(
            "invalid_argument",
            2,
            err.to_string(),
            "Check the message command arguments and try again.",
        ),
        MessageAdapterError::MessageNotFound | MessageAdapterError::AttachmentNotFound => {
            ExitError::new("not_found", 5, err.to_string(), hint)
        }
        MessageAdapterError::IdentityRequired(message) => ExitError::new(
            "identity_required",
            3,
            message,
            "Complete user setup with `awiki-cli id register --handle <handle> ...` or recover an existing handle before using `awiki-cli msg` commands.",
        ),
        MessageAdapterError::SecureNotSupported => ExitError::new(
            "unsupported_mode",
            1,
            err.to_string(),
            "Secure messaging is currently supported only for direct text messaging.",
        ),
        MessageAdapterError::GroupE2eeSelfLeaveUnsupported => ExitError::new(
            "unsupported_mode",
            1,
            err.to_string(),
            "For PR-A group E2EE, ask the group owner to remove the member; self-leave requires a future epoch-advancing leave-request flow.",
        ),
        MessageAdapterError::TransportUnavailable(_) => ExitError::new(
            "transport_unavailable",
            1,
            err.to_string(),
            "Start the websocket listener/daemon or switch runtime.mode back to http.",
        ),
        MessageAdapterError::PathUnavailable(message) => ExitError::new(
            "invalid_argument",
            2,
            message,
            "Check the attachment file/output path and permissions.",
        ),
        MessageAdapterError::AttachmentNotSupported | MessageAdapterError::GroupNotSupported => {
            ExitError::new("not_implemented", 1, err.to_string(), hint)
        }
        MessageAdapterError::Service(service_err) => match () {
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
        MessageAdapterError::Identity(err) => match err.kind {
            IdentityErrorKind::InvalidInput => ExitError::new(
                "invalid_argument",
                2,
                err.message,
                "Run `awiki-cli id list` to inspect available identities.",
            ),
            IdentityErrorKind::NotFound => ExitError::new(
                "not_found",
                5,
                err.message,
                "Run `awiki-cli id list` to inspect available identities.",
            ),
            IdentityErrorKind::Conflict => ExitError::new(
                "conflict",
                1,
                err.message,
                "Use a different --identity value if the alias is already occupied.",
            ),
            IdentityErrorKind::AuthRequired => ExitError::new(
                "auth_required",
                3,
                err.message,
                "Use an identity with valid DID key material, or run `awiki-cli id refresh-token` / `awiki-cli id register` / `awiki-cli id recover` first.",
            ),
            IdentityErrorKind::Internal => ExitError::new(
                "internal_error",
                1,
                err.message,
                "Run `awiki-cli doctor` to inspect configuration and storage paths.",
            ),
        },
        MessageAdapterError::Internal(message) => ExitError::new("internal_error", 1, message, hint),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_exit_maps_transport_unavailable_like_go() {
        let exit = message_exit(
            MessageAdapterError::transport_unavailable("bridge offline"),
            "fallback hint",
        );

        assert_eq!(exit.exit_code, 1);
        assert_eq!(exit.detail.code, "transport_unavailable");
        assert_eq!(
            exit.detail.message,
            "message transport is unavailable: bridge offline"
        );
        assert_eq!(
            exit.detail.hint,
            "Start the websocket listener/daemon or switch runtime.mode back to http."
        );
    }
}
