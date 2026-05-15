use super::{identity_exit, App};
use crate::cli::ParsedCommand;
use crate::mail::{self, CommandResult, MailError};
use crate::output::ExitError;
use base64::Engine;
use serde_json::{json, Value};
use std::path::Path;

impl App {
    pub fn run_mail_inbox(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let folder = command
            .flags
            .get("folder")
            .cloned()
            .unwrap_or_else(|| "inbox".to_string());
        let limit = int_flag(command, "limit", 20)?;
        let offset = int_flag(command, "offset", 0)?;
        let unread_only = bool_flag(command, "unread");
        let result = if self.globals.dry_run {
            mail::inbox_plan(&self.globals.identity, &folder, limit, offset, unread_only)
        } else {
            mail::inbox(
                &resolved,
                &self.identity_manager(&resolved),
                mail::InboxRequest {
                    identity_name: self.globals.identity.clone(),
                    folder,
                    limit,
                    offset,
                    unread_only,
                },
            )
            .map_err(|err| {
                mail_exit(
                    err,
                    "Ensure the active identity is valid and mail service is reachable.",
                )
            })?
        };
        self.render_mail_result("awiki-cli mail inbox", &resolved, result)
    }

    pub fn run_mail_read(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let message_id = command.flags.get("id").cloned().unwrap_or_default();
        if message_id.trim().is_empty() {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "mail read requires --id.",
                "Usage: awiki-cli mail read --id <MESSAGE_ID>",
            ));
        }
        let result = if self.globals.dry_run {
            mail::read_plan(&self.globals.identity, &message_id)
        } else {
            mail::read(
                &resolved,
                &self.identity_manager(&resolved),
                mail::ReadRequest {
                    identity_name: self.globals.identity.clone(),
                    message_id: message_id.clone(),
                },
            )
            .map_err(|err| {
                mail_exit(
                    err,
                    "Ensure the message id is valid and mail service is reachable.",
                )
            })?
        };
        self.render_mail_result("awiki-cli mail read", &resolved, result)
    }

    pub fn run_mail_mark_read(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        if command.args.is_empty() {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "mail mark-read requires at least one message id.",
                "Usage: awiki-cli mail mark-read <MESSAGE_ID...>",
            ));
        }
        let result = if self.globals.dry_run {
            mail::mark_read_plan(&self.globals.identity, &command.args)
        } else {
            mail::mark_read(
                &resolved,
                &self.identity_manager(&resolved),
                mail::MarkReadRequest {
                    identity_name: self.globals.identity.clone(),
                    message_ids: command.args.clone(),
                    is_read: true,
                },
            )
            .map_err(|err| {
                mail_exit(
                    err,
                    "Ensure the message ids are valid and mail service is reachable.",
                )
            })?
        };
        self.render_mail_result("awiki-cli mail mark-read", &resolved, result)
    }

    pub fn run_mail_account(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let result = if self.globals.dry_run {
            mail::account_plan(&self.globals.identity)
        } else {
            mail::account(
                &resolved,
                &self.identity_manager(&resolved),
                mail::AccountRequest {
                    identity_name: self.globals.identity.clone(),
                },
            )
            .map_err(|err| mail_exit(err, "Ensure the mail service is reachable."))?
        };
        self.render_mail_result("awiki-cli mail account", &resolved, result)
    }

    pub fn run_mail_send(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let to_raw = command.flags.get("to").cloned().unwrap_or_default();
        let cc_raw = command.flags.get("cc").cloned().unwrap_or_default();
        let subject = command.flags.get("subject").cloned().unwrap_or_default();
        let body = command.flags.get("body").cloned().unwrap_or_default();
        let html = command.flags.get("html").cloned().unwrap_or_default();
        let to = mail::split_mail_list(&to_raw);
        let cc = mail::split_mail_list(&cc_raw);
        if to.is_empty() {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "mail send requires --to.",
                "Usage: awiki-cli mail send --to alice@example.com --subject \"Hello\" --body \"Hi\"",
            ));
        }
        if subject.trim().is_empty() {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "mail send requires --subject.",
                "Provide a subject with --subject.",
            ));
        }
        if body.trim().is_empty() {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "mail send requires --body.",
                "Provide the plain text body with --body.",
            ));
        }
        let result = if self.globals.dry_run {
            mail::send_plan(&self.globals.identity, &to, &cc, &subject, &html)
        } else {
            mail::send(
                &resolved,
                &self.identity_manager(&resolved),
                mail::SendRequest {
                    identity_name: self.globals.identity.clone(),
                    to,
                    cc,
                    subject,
                    body_text: body,
                    body_html: html,
                },
            )
            .map_err(|err| {
                mail_exit(
                    err,
                    "Ensure the mail service is reachable and the active identity is valid.",
                )
            })?
        };
        self.render_mail_result("awiki-cli mail send", &resolved, result)
    }

    pub fn run_mail_attachment_download(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let message_id = command.flags.get("message-id").cloned().unwrap_or_default();
        let attachment_index = int_flag(command, "attachment-index", 0)?;
        let output = command.flags.get("output").cloned().unwrap_or_default();
        if message_id.trim().is_empty() {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "mail attachment download requires --message-id.",
                "Usage: awiki-cli mail attachment download --message-id <MESSAGE_ID> --attachment-index 0",
            ));
        }
        if attachment_index < 0 {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "attachment index must be >= 0.",
                "Use --attachment-index 0 for the first attachment.",
            ));
        }
        if self.globals.dry_run {
            return self.render_mail_result(
                "awiki-cli mail attachment download",
                &resolved,
                mail::attachment_download_plan(
                    &self.globals.identity,
                    &message_id,
                    attachment_index,
                    &output,
                ),
            );
        }
        let result = mail::attachment(
            &resolved,
            &self.identity_manager(&resolved),
            mail::AttachmentRequest {
                identity_name: self.globals.identity.clone(),
                message_id: message_id.clone(),
                attachment_index,
            },
        )
        .map_err(|err| {
            mail_exit(
                err,
                "Ensure the message id is valid and mail service is reachable.",
            )
        })?;
        self.render_attachment_download_result(
            &resolved,
            &message_id,
            attachment_index,
            &output,
            result,
        )
    }

    pub fn run_mail_notify(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let limit = int_flag(command, "limit", 20)?;
        let result = if self.globals.dry_run {
            mail::notifications_plan(&self.globals.identity, limit)
        } else {
            mail::notifications(
                &resolved,
                &self.identity_manager(&resolved),
                &self.globals.identity,
                limit,
            )
            .map_err(|err| {
                mail_exit(
                    err,
                    "Ensure the runtime listener is running in websocket mode and has received notifications.",
                )
            })?
        };
        self.render_mail_result("awiki-cli mail notify", &resolved, result)
    }

    fn render_mail_result(
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

    fn render_attachment_download_result(
        &self,
        resolved: &crate::config::Resolved,
        message_id: &str,
        attachment_index: i64,
        output_path: &str,
        result: CommandResult,
    ) -> Result<(), ExitError> {
        let filename = string_value(result.data.get("filename"))
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| format!("attachment_{attachment_index}"));
        let content_base64 = string_value(result.data.get("content_base64")).unwrap_or_default();
        let content_type = string_value(result.data.get("content_type"))
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "application/octet-stream".to_string());
        let size = result.data.get("size").cloned().unwrap_or(Value::Null);
        if content_base64.is_empty() {
            return Err(ExitError::new(
                "internal_error",
                1,
                "attachment content is empty",
                "Try fetching the attachment again or verify the mail service response.",
            ));
        }
        let content = base64::engine::general_purpose::STANDARD
            .decode(content_base64.as_bytes())
            .map_err(|err| {
                ExitError::new(
                    "internal_error",
                    1,
                    format!("attachment base64 decode failed: {err}"),
                    "Ensure the mail service response is valid.",
                )
            })?;
        let final_path = if output_path.trim().is_empty() {
            filename.clone()
        } else {
            output_path.to_string()
        };
        if let Some(parent) = Path::new(&final_path).parent() {
            if parent != Path::new("") && parent != Path::new(".") {
                std::fs::create_dir_all(parent).map_err(|err| {
                    ExitError::new(
                        "internal_error",
                        1,
                        err.to_string(),
                        "Check write permissions for the output directory.",
                    )
                })?;
            }
        }
        std::fs::write(&final_path, content).map_err(|err| {
            ExitError::new(
                "internal_error",
                1,
                err.to_string(),
                "Check write permissions for the output file.",
            )
        })?;
        self.render_success(
            "awiki-cli mail attachment download",
            resolved,
            json!({
                "message_id": message_id,
                "attachment_index": attachment_index,
                "filename": filename,
                "content_type": content_type,
                "size": size,
                "path": final_path,
            }),
            &format!("Attachment saved to {final_path}"),
            result.warnings,
        )
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

fn mail_exit(err: MailError, hint: &str) -> ExitError {
    match err {
        MailError::MessageIdRequired
        | MailError::RecipientRequired
        | MailError::SubjectRequired
        | MailError::BodyRequired
        | MailError::AttachmentIndexZero => ExitError::new(
            "invalid_argument",
            2,
            err.to_string(),
            "Check the mail command arguments and try again.",
        ),
        MailError::IdentityRequired(message) => ExitError::new(
            "identity_required",
            3,
            message,
            "Complete user setup with `awiki-cli id register --handle <handle> ...` or recover an existing handle before using `awiki-cli mail` commands.",
        ),
        MailError::Service(service_err) => match () {
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
            _ if service_err.status_code == 404 || service_err.rpc_code == -32002 => {
                ExitError::new("not_found", 5, service_err.to_string(), hint)
            }
            _ if service_err.status_code == 409
                || matches!(service_err.rpc_code, -32003 | -32004) =>
            {
                ExitError::new("conflict", 1, service_err.to_string(), hint)
            }
            _ => ExitError::new("internal_error", 1, service_err.to_string(), hint),
        },
        MailError::Identity(err) => identity_exit(err),
        MailError::Store(err) => super::store_exit(
            err,
            "Ensure the runtime listener is running in websocket mode and has received notifications.",
        ),
        MailError::Internal(message) => ExitError::new("internal_error", 1, message, hint),
    }
}

fn string_value(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(ToOwned::to_owned)
}
