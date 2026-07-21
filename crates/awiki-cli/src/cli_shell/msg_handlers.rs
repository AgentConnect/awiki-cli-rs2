use super::handle_helpers::complete_bare_handle;
use super::App;
use crate::cli_output::ExitError;
use crate::cli_parser::ParsedCommand;
use crate::m_core_cli_adapter::message_result::{
    CommandResult, IdentityErrorKind, MessageAdapterError,
};
use crate::workspace_config::Resolved;
use im_core::prelude::{MessageBody, MessageKind};
use serde_json::{json, Map, Value};

struct MsgSendPlan<'a> {
    identity: &'a str,
    to: &'a str,
    group: &'a str,
    text: &'a str,
    message_type: &'a str,
    payload: Option<&'a Value>,
    file_path: &'a str,
    mime_type: &'a str,
    has_attachment: bool,
    secure: bool,
}

struct MsgSendPlanBody<'a> {
    text: String,
    message_type: &'static str,
    payload: Option<&'a Value>,
}

impl App {
    pub fn run_msg_send(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        self.run_msg_send_im_core(command)
    }

    pub async fn run_msg_send_async(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        self.run_msg_send_im_core_async(command).await
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
        let (request, request_warnings) =
            crate::m_core_cli_adapter::messages::send_message_request(
                command,
                &resolved.did_domain,
            )?;
        let secure = message_security_is_required(&request.security);
        if self.globals.dry_run {
            let body = send_message_plan_body(&request)?;
            return self.render_msg_send_plan(
                &resolved,
                MsgSendPlan {
                    identity: &self.globals.identity,
                    to: &string_flag(command, "to"),
                    group: &string_flag(command, "group"),
                    text: &body.text,
                    message_type: body.message_type,
                    payload: body.payload,
                    file_path: "",
                    mime_type: "",
                    has_attachment: false,
                    secure,
                },
                request_warnings,
            );
        }

        let client = crate::m_core_cli_adapter::build_im_client(
            &resolved,
            crate::m_core_cli_adapter::cli_identity_selector(&self.globals.identity),
        )?;
        let mut result = crate::m_core_cli_adapter::messages::send_message_via_im_core(
            &resolved, &client, request,
        )
        .map_err(|err| {
            message_exit(
                err,
                "Ensure the active identity is ready and the message service is reachable.",
            )
        })?;
        result.warnings.extend(request_warnings);
        self.render_message_result("awiki-cli msg send", &resolved, result)
    }

    async fn run_msg_send_im_core_async(&self, command: &ParsedCommand) -> Result<(), ExitError> {
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
                .run_msg_attachment_send_im_core_async(command, &resolved, &file_path, &mime_type)
                .await;
        }
        let (request, request_warnings) =
            crate::m_core_cli_adapter::messages::send_message_request(
                command,
                &resolved.did_domain,
            )?;
        let secure = message_security_is_required(&request.security);
        if self.globals.dry_run {
            let body = send_message_plan_body(&request)?;
            return self.render_msg_send_plan(
                &resolved,
                MsgSendPlan {
                    identity: &self.globals.identity,
                    to: &string_flag(command, "to"),
                    group: &string_flag(command, "group"),
                    text: &body.text,
                    message_type: body.message_type,
                    payload: body.payload,
                    file_path: "",
                    mime_type: "",
                    has_attachment: false,
                    secure,
                },
                request_warnings,
            );
        }

        let client = crate::m_core_cli_adapter::build_im_client_async(
            &resolved,
            crate::m_core_cli_adapter::cli_identity_selector(&self.globals.identity),
        )
        .await?;
        let mut result = crate::m_core_cli_adapter::messages::send_message_via_im_core_async(
            &resolved, &client, request,
        )
        .await
        .map_err(|err| {
            message_exit(
                err,
                "Ensure the active identity is ready and the message service is reachable.",
            )
        })?;
        result.warnings.extend(request_warnings);
        self.render_message_result("awiki-cli msg send", &resolved, result)
    }

    fn run_msg_attachment_send_im_core(
        &self,
        command: &ParsedCommand,
        resolved: &Resolved,
        file_path: &str,
        mime_type: &str,
    ) -> Result<(), ExitError> {
        let (target, request, client_message_id, request_warnings) =
            crate::m_core_cli_adapter::messages::send_attachment_request(
                command,
                &resolved.did_domain,
            )?;
        let caption = request.caption.clone().unwrap_or_default();
        let secure = matches!(
            request.security,
            im_core::prelude::MessageSecurityMode::E2eeRequired
                | im_core::prelude::MessageSecurityMode::SecureDirect
                | im_core::prelude::MessageSecurityMode::GroupE2ee
        );
        if self.globals.dry_run {
            return self.render_msg_send_plan(
                resolved,
                MsgSendPlan {
                    identity: &self.globals.identity,
                    to: &string_flag(command, "to"),
                    group: &string_flag(command, "group"),
                    text: &caption,
                    message_type: "",
                    payload: None,
                    file_path,
                    mime_type,
                    has_attachment: true,
                    secure,
                },
                request_warnings,
            );
        }

        let client = crate::m_core_cli_adapter::build_im_client(
            resolved,
            crate::m_core_cli_adapter::cli_identity_selector(&self.globals.identity),
        )?;
        let mut result = crate::m_core_cli_adapter::messages::send_attachment_via_im_core(
            resolved,
            &client,
            target,
            request,
            client_message_id,
        )
        .map_err(|err| {
            message_exit(
                err,
                "Ensure the active identity is ready and the attachment service is reachable.",
            )
        })?;
        result.warnings.extend(request_warnings);
        self.render_message_result("awiki-cli msg send", resolved, result)
    }

    async fn run_msg_attachment_send_im_core_async(
        &self,
        command: &ParsedCommand,
        resolved: &Resolved,
        file_path: &str,
        mime_type: &str,
    ) -> Result<(), ExitError> {
        let (target, request, client_message_id, request_warnings) =
            crate::m_core_cli_adapter::messages::send_attachment_request(
                command,
                &resolved.did_domain,
            )?;
        let caption = request.caption.clone().unwrap_or_default();
        let secure = matches!(
            request.security,
            im_core::prelude::MessageSecurityMode::E2eeRequired
                | im_core::prelude::MessageSecurityMode::SecureDirect
                | im_core::prelude::MessageSecurityMode::GroupE2ee
        );
        if self.globals.dry_run {
            return self.render_msg_send_plan(
                resolved,
                MsgSendPlan {
                    identity: &self.globals.identity,
                    to: &string_flag(command, "to"),
                    group: &string_flag(command, "group"),
                    text: &caption,
                    message_type: "",
                    payload: None,
                    file_path,
                    mime_type,
                    has_attachment: true,
                    secure,
                },
                request_warnings,
            );
        }

        let client = crate::m_core_cli_adapter::build_im_client_async(
            resolved,
            crate::m_core_cli_adapter::cli_identity_selector(&self.globals.identity),
        )
        .await?;
        let mut result = crate::m_core_cli_adapter::messages::send_attachment_via_im_core_async(
            resolved,
            &client,
            target,
            request,
            client_message_id,
        )
        .await
        .map_err(|err| {
            message_exit(
                err,
                "Ensure the active identity is ready and the attachment service is reachable.",
            )
        })?;
        result.warnings.extend(request_warnings);
        self.render_message_result("awiki-cli msg send", resolved, result)
    }

    fn render_msg_send_plan(
        &self,
        resolved: &Resolved,
        input: MsgSendPlan<'_>,
        warnings: Vec<String>,
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
        plan.insert("secure".to_string(), Value::Bool(input.secure));
        plan.insert(
            "security".to_string(),
            Value::String(if input.secure { "required" } else { "off" }.to_string()),
        );
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
        if let Some(payload) = input.payload {
            plan.insert("payload".to_string(), payload.clone());
        }

        self.render_success(
            "awiki-cli msg send",
            &resolved,
            json!({ "plan": plan }),
            "Dry run: message send planned",
            warnings,
        )
    }

    pub fn run_msg_attachment_download(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let request = crate::m_core_cli_adapter::messages::download_attachment_request(
            command,
            &resolved.did_domain,
        )?;
        if self.globals.dry_run {
            return self.render_msg_attachment_download_plan(command, &resolved);
        }

        let client = crate::m_core_cli_adapter::build_im_client(
            &resolved,
            crate::m_core_cli_adapter::cli_identity_selector(&self.globals.identity),
        )?;
        let result = crate::m_core_cli_adapter::messages::download_attachment_via_im_core(
            &resolved, &client, request,
        )
        .map_err(|err| {
            message_exit(
                err,
                "Ensure the message id, attachment id, and attachment service are reachable.",
            )
        })?;
        self.render_message_result("awiki-cli msg attachment download", &resolved, result)
    }

    pub async fn run_msg_attachment_download_async(
        &self,
        command: &ParsedCommand,
    ) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let request = crate::m_core_cli_adapter::messages::download_attachment_request(
            command,
            &resolved.did_domain,
        )?;
        if self.globals.dry_run {
            return self.render_msg_attachment_download_plan(command, &resolved);
        }

        let client = crate::m_core_cli_adapter::build_im_client_async(
            &resolved,
            crate::m_core_cli_adapter::cli_identity_selector(&self.globals.identity),
        )
        .await?;
        let result = crate::m_core_cli_adapter::messages::download_attachment_via_im_core_async(
            &resolved, &client, request,
        )
        .await
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

    pub async fn run_msg_inbox_async(&self, command: &ParsedCommand) -> Result<(), ExitError> {
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
        self.run_msg_inbox_im_core_async(command).await
    }

    fn run_msg_inbox_im_core(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let query = crate::m_core_cli_adapter::messages::inbox_query(command)?;
        if !self.globals.dry_run {
            let client = crate::m_core_cli_adapter::build_im_client(
                &resolved,
                crate::m_core_cli_adapter::cli_identity_selector(&self.globals.identity),
            )?;
            let result = crate::m_core_cli_adapter::messages::read_inbox_via_im_core(
                &resolved, &client, query,
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

    async fn run_msg_inbox_im_core_async(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let query = crate::m_core_cli_adapter::messages::inbox_query(command)?;
        if !self.globals.dry_run {
            let client = crate::m_core_cli_adapter::build_im_client_async(
                &resolved,
                crate::m_core_cli_adapter::cli_identity_selector(&self.globals.identity),
            )
            .await?;
            let result = crate::m_core_cli_adapter::messages::read_inbox_via_im_core_async(
                &resolved, &client, query,
            )
            .await
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

    pub async fn run_msg_history_async(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        self.run_msg_history_im_core_async(command).await
    }

    fn run_msg_history_im_core(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let (thread, query) =
            crate::m_core_cli_adapter::messages::history_request(command, &resolved.did_domain)?;
        if !self.globals.dry_run {
            let client = crate::m_core_cli_adapter::build_im_client(
                &resolved,
                crate::m_core_cli_adapter::cli_identity_selector(&self.globals.identity),
            )?;
            let result = crate::m_core_cli_adapter::messages::read_history_via_im_core(
                &resolved, &client, thread, query,
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

    async fn run_msg_history_im_core_async(
        &self,
        command: &ParsedCommand,
    ) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let (thread, query) =
            crate::m_core_cli_adapter::messages::history_request(command, &resolved.did_domain)?;
        if !self.globals.dry_run {
            let client = crate::m_core_cli_adapter::build_im_client_async(
                &resolved,
                crate::m_core_cli_adapter::cli_identity_selector(&self.globals.identity),
            )
            .await?;
            let result = crate::m_core_cli_adapter::messages::read_history_via_im_core_async(
                &resolved, &client, thread, query,
            )
            .await
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
            crate::m_core_cli_adapter::messages::history_request(command, &resolved.did_domain)?;
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
            let client = crate::m_core_cli_adapter::build_im_client(
                &resolved,
                crate::m_core_cli_adapter::cli_identity_selector(&self.globals.identity),
            )?;
            let result = crate::m_core_cli_adapter::messages::mark_read_via_im_core(
                &resolved,
                &client,
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

    pub async fn run_msg_mark_read_async(&self, command: &ParsedCommand) -> Result<(), ExitError> {
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
            let client = crate::m_core_cli_adapter::build_im_client_async(
                &resolved,
                crate::m_core_cli_adapter::cli_identity_selector(&self.globals.identity),
            )
            .await?;
            let result = crate::m_core_cli_adapter::messages::mark_read_via_im_core_async(
                &resolved,
                &client,
                command.args.clone(),
            )
            .await
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
        let resolved = self.resolve_config_for_workspace()?;
        let peer = required_peer_flag(command, "msg secure status")?;
        if self.globals.dry_run {
            return self.render_msg_secure_plan(
                "awiki-cli msg secure status",
                &resolved,
                "secure.direct.status",
                &peer,
                "Dry run: direct secure status planned",
            );
        }
        let client = crate::m_core_cli_adapter::build_im_client(
            &resolved,
            crate::m_core_cli_adapter::cli_identity_selector(&self.globals.identity),
        )?;
        let result = crate::m_core_cli_adapter::messages::direct_secure_status_via_im_core(
            &client,
            peer,
            &resolved.did_domain,
        )
        .map_err(|err| {
            message_exit(
                err,
                "Ensure the active identity is ready and local secure state is available.",
            )
        })?;
        self.render_message_result("awiki-cli msg secure status", &resolved, result)
    }

    pub async fn run_msg_secure_status_async(
        &self,
        command: &ParsedCommand,
    ) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let peer = required_peer_flag(command, "msg secure status")?;
        if self.globals.dry_run {
            return self.render_msg_secure_plan(
                "awiki-cli msg secure status",
                &resolved,
                "secure.direct.status",
                &peer,
                "Dry run: direct secure status planned",
            );
        }
        let client = crate::m_core_cli_adapter::build_im_client_async(
            &resolved,
            crate::m_core_cli_adapter::cli_identity_selector(&self.globals.identity),
        )
        .await?;
        let result = crate::m_core_cli_adapter::messages::direct_secure_status_via_im_core_async(
            &client,
            peer,
            &resolved.did_domain,
        )
        .await
        .map_err(|err| {
            message_exit(
                err,
                "Ensure the active identity is ready and local secure state is available.",
            )
        })?;
        self.render_message_result("awiki-cli msg secure status", &resolved, result)
    }

    pub fn run_msg_secure_init(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        unsupported_secure_command(command, "msg.secure.init")
    }

    pub fn run_msg_secure_repair(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let peer = required_peer_flag(command, "msg secure repair")?;
        if self.globals.dry_run {
            return self.render_msg_secure_plan(
                "awiki-cli msg secure repair",
                &resolved,
                "secure.direct.repair",
                &peer,
                "Dry run: direct secure repair planned",
            );
        }
        let client = crate::m_core_cli_adapter::build_im_client(
            &resolved,
            crate::m_core_cli_adapter::cli_identity_selector(&self.globals.identity),
        )?;
        let result = crate::m_core_cli_adapter::messages::direct_secure_repair_via_im_core(
            &client,
            peer,
            &resolved.did_domain,
        )
        .map_err(|err| {
            message_exit(
                err,
                "Ensure the active identity is ready and local secure state is available.",
            )
        })?;
        self.render_message_result("awiki-cli msg secure repair", &resolved, result)
    }

    pub async fn run_msg_secure_repair_async(
        &self,
        command: &ParsedCommand,
    ) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let peer = required_peer_flag(command, "msg secure repair")?;
        if self.globals.dry_run {
            return self.render_msg_secure_plan(
                "awiki-cli msg secure repair",
                &resolved,
                "secure.direct.repair",
                &peer,
                "Dry run: direct secure repair planned",
            );
        }
        let client = crate::m_core_cli_adapter::build_im_client_async(
            &resolved,
            crate::m_core_cli_adapter::cli_identity_selector(&self.globals.identity),
        )
        .await?;
        let result = crate::m_core_cli_adapter::messages::direct_secure_repair_via_im_core_async(
            &client,
            peer,
            &resolved.did_domain,
        )
        .await
        .map_err(|err| {
            message_exit(
                err,
                "Ensure the active identity is ready and local secure state is available.",
            )
        })?;
        self.render_message_result("awiki-cli msg secure repair", &resolved, result)
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

    fn render_msg_secure_plan(
        &self,
        command: &str,
        resolved: &Resolved,
        action: &str,
        peer: &str,
        summary: &str,
    ) -> Result<(), ExitError> {
        self.render_success(
            command,
            resolved,
            json!({
                "plan": {
                    "action": action,
                    "identity": self.globals.identity,
                    "runtime_mode": resolved.runtime_mode,
                    "with": peer,
                    "target": target_value(peer, "", resolved),
                }
            }),
            summary,
            Vec::new(),
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

fn required_peer_flag(command: &ParsedCommand, command_name: &str) -> Result<String, ExitError> {
    let peer = string_flag(command, "with");
    if peer.trim().is_empty() {
        return Err(ExitError::new(
            "invalid_argument",
            2,
            format!("{command_name} requires --with."),
            format!("Usage: awiki-cli {command_name} --with <PEER>"),
        ));
    }
    Ok(peer)
}

fn message_security_is_required(security: &im_core::prelude::MessageSecurityMode) -> bool {
    matches!(
        security,
        im_core::prelude::MessageSecurityMode::E2eeRequired
            | im_core::prelude::MessageSecurityMode::SecureDirect
            | im_core::prelude::MessageSecurityMode::GroupE2ee
    )
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

fn send_message_plan_body(
    request: &im_core::prelude::SendMessageRequest,
) -> Result<MsgSendPlanBody<'_>, ExitError> {
    match &request.body {
        MessageBody::Text { text, kind } => Ok(MsgSendPlanBody {
            text: text.clone(),
            message_type: message_type_for_kind(kind),
            payload: None,
        }),
        MessageBody::Payload { payload } => Ok(MsgSendPlanBody {
            text: String::new(),
            message_type: "application/json",
            payload: Some(payload),
        }),
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
        MessageAdapterError::PermissionDenied => ExitError::new(
            "permission_denied",
            4,
            err.to_string(),
            "Check that the active device has the required group role and retry.",
        ),
        MessageAdapterError::LocalStateUnavailable(detail) => ExitError::new(
            "local_state_unavailable",
            5,
            detail,
            "Refresh authoritative group state, then run group secure repair and retry.",
        ),
        MessageAdapterError::SecureNotSupported => ExitError::new(
            "unsupported_mode",
            1,
            err.to_string(),
            "Secure messaging is currently supported only for direct text messaging.",
        ),
        MessageAdapterError::SecureAttachmentNotSupported => ExitError::new(
            "not_implemented",
            1,
            err.to_string(),
            "E2EE attachment sending is not supported yet. Send text with E2EE or use the non-E2EE attachment flow when available.",
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
        MessageAdapterError::PublicServiceCode(service_code) => {
            let mut mapped = ExitError::new(
                "service_error",
                5,
                "message operation: remote service request failed.",
                hint,
            );
            if crate::m_core_cli_adapter::error::is_public_service_code(&service_code) {
                mapped.detail.details = json!({"service_code": service_code});
            }
            mapped
        }
        MessageAdapterError::Service(service_err) => match () {
            _ if service_err.status_code == 403 => ExitError::new(
                "permission_denied",
                4,
                "message operation: permission denied.",
                "Check identity permissions and service access.",
            ),
            _ if service_err.status_code == 401 => ExitError::new(
                "auth_required",
                3,
                "message operation: authentication is required.",
                "Use an identity with a valid JWT or DID WBA auth material.",
            ),
            _ if service_err.status_code == 400 || service_err.rpc_code == -32602 => {
                ExitError::new(
                    "invalid_argument",
                    2,
                    "message operation: remote service rejected the request.",
                    hint,
                )
            }
            _ if service_err.rpc_code == -32000 => {
                ExitError::new(
                    "auth_required",
                    3,
                    "message operation: authentication is required.",
                    "Use an identity with a valid JWT or DID WBA auth material.",
                )
            }
            _ if service_err.rpc_code == 1401 => ExitError::new(
                "auth_required",
                3,
                "message operation: authentication is required.",
                "Use an identity with a valid JWT or DID WBA auth material.",
            ),
            _ if service_err.status_code == 404
                || service_err.rpc_code == -32002
                || matches!(service_err.rpc_code, 6000 | 6005 | 6007 | 6012) =>
            {
                ExitError::new(
                    "not_found",
                    5,
                    "message operation: remote resource was not found.",
                    hint,
                )
            }
            _ if service_err.status_code == 409
                || matches!(service_err.rpc_code, -32003 | -32004) =>
            {
                ExitError::new(
                    "conflict",
                    1,
                    "message operation: remote state conflict.",
                    hint,
                )
            }
            _ if matches!(
                service_err.rpc_code,
                6006 | 6008 | 6009 | 6010 | 6011 | 6013
            ) =>
            {
                ExitError::new(
                    "invalid_argument",
                    2,
                    "message operation: remote service rejected the request.",
                    hint,
                )
            }
            _ => ExitError::new(
                "internal_error",
                1,
                "message operation: remote service request failed.",
                hint,
            ),
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

    #[test]
    fn message_exit_preserves_group_fail_closed_categories() {
        let denied = message_exit(MessageAdapterError::PermissionDenied, "fallback hint");
        assert_eq!(denied.exit_code, 4);
        assert_eq!(denied.detail.code, "permission_denied");

        let unavailable = message_exit(
            MessageAdapterError::LocalStateUnavailable(
                "authoritative P4 member roster is incomplete".to_owned(),
            ),
            "fallback hint",
        );
        assert_eq!(unavailable.exit_code, 5);
        assert_eq!(unavailable.detail.code, "local_state_unavailable");
        assert_eq!(
            unavailable.detail.message,
            "authoritative P4 member roster is incomplete"
        );
    }

    #[test]
    fn message_exit_preserves_only_stable_public_service_code() {
        let exit = message_exit(
            MessageAdapterError::PublicServiceCode("anp.device_state_changed".to_owned()),
            "Refresh the device state and retry.",
        );

        assert_eq!(exit.exit_code, 5);
        assert_eq!(exit.detail.code, "service_error");
        assert_eq!(
            exit.detail.details,
            json!({"service_code": "anp.device_state_changed"})
        );
        assert_eq!(
            exit.detail.message,
            "message operation: remote service request failed."
        );
    }

    #[test]
    fn message_exit_rejects_unvalidated_service_code() {
        let private_marker = "remote-private-service-code";
        let exit = message_exit(
            MessageAdapterError::PublicServiceCode(private_marker.to_owned()),
            "Retry later.",
        );

        assert_eq!(exit.exit_code, 5);
        assert_eq!(exit.detail.code, "service_error");
        assert_eq!(exit.detail.details, serde_json::Value::Null);
        assert!(!format!("{exit:?}").contains(private_marker));
    }

    #[test]
    fn message_exit_does_not_expose_remote_auth_error_payload() {
        let private_marker = "remote-private-auth-payload";
        for (status_code, rpc_code, expected_code, expected_exit_code) in [
            (401, -32602, "auth_required", 3),
            (403, -32602, "permission_denied", 4),
        ] {
            let exit = message_exit(
                MessageAdapterError::Service(
                    crate::m_core_cli_adapter::message_result::ServiceError {
                        status_code,
                        rpc_code,
                        message: private_marker.to_owned(),
                        data: Some(json!({"private": private_marker})),
                    },
                ),
                "Retry later.",
            );

            assert_eq!(exit.exit_code, expected_exit_code);
            assert_eq!(exit.detail.code, expected_code);
            assert_eq!(exit.detail.details, serde_json::Value::Null);
            assert!(!format!("{exit:?}").contains(private_marker));
        }
    }
}
