use super::App;
use crate::cli::ParsedCommand;
use crate::config::Resolved;
use crate::identity;
use crate::message::{CommandResult, MessageError};
use crate::output::ExitError;
use im_core::prelude::MessageTarget;
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
        let request =
            crate::im_core_adapter::messages::send_message_request(command, &resolved.did_domain)?;
        let legacy_request = crate::im_core_adapter::messages::legacy_text_send_request(
            &self.globals.identity,
            request.clone(),
        )?;
        if self.globals.dry_run {
            return self.render_msg_send_plan(
                &resolved,
                MsgSendPlan {
                    identity: &self.globals.identity,
                    to: &string_flag(command, "to"),
                    group: &string_flag(command, "group"),
                    text: &legacy_request.text,
                    message_type: &legacy_request.message_type,
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
        let result = match request.target {
            MessageTarget::Direct(_) => {
                crate::im_core_adapter::messages::send_direct_text_via_im_core(
                    &resolved,
                    &manager,
                    &client,
                    &self.globals.identity,
                    request,
                )
            }
            MessageTarget::Group(_) => {
                crate::im_core_adapter::messages::send_group_text_via_im_core(
                    &resolved,
                    &manager,
                    &client,
                    &self.globals.identity,
                    request,
                )
            }
        }
        .map_err(|err| {
            message_exit(
                err,
                "Ensure the active identity is ready and the message service is reachable.",
            )
        })?;
        self.render_message_result("awiki-cli msg send", &resolved, result)
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

    pub fn run_msg_attachment_download(&self, _command: &ParsedCommand) -> Result<(), ExitError> {
        Err(super::unsupported::unsupported_cutover_command(
            "msg.attachment.download",
            "attachments",
            "Phase 4",
        ))
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
        require_flags(command, &["with"])?;
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
        MessageError::SecureNotSupported => ExitError::new(
            "unsupported_mode",
            1,
            err.to_string(),
            "Secure messaging is currently supported only for direct text messaging.",
        ),
        MessageError::GroupE2eeSelfLeaveUnsupported => ExitError::new(
            "unsupported_mode",
            1,
            err.to_string(),
            "For PR-A group E2EE, ask the group owner to remove the member; self-leave requires a future epoch-advancing leave-request flow.",
        ),
        MessageError::TransportUnavailable(_) => ExitError::new(
            "transport_unavailable",
            1,
            err.to_string(),
            "Start the websocket listener/daemon or switch runtime.mode back to http.",
        ),
        MessageError::AttachmentNotSupported | MessageError::GroupNotSupported => {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_exit_maps_transport_unavailable_like_go() {
        let exit = message_exit(
            MessageError::transport_unavailable("bridge offline"),
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
