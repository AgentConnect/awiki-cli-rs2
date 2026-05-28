use super::App;
use crate::cli_output::ExitError;
use crate::cli_parser::ParsedCommand;
use crate::m_core_cli_adapter::email::{self, CommandResult};
use std::io::Write;
use std::path::Path;
#[cfg(unix)]
use std::{fs::DirBuilder, os::unix::fs::DirBuilderExt, os::unix::fs::OpenOptionsExt};

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
            email::inbox_plan(&self.globals.identity, &folder, limit, offset, unread_only)
        } else {
            let query = email::inbox_query(command)?;
            let client = crate::m_core_cli_adapter::build_im_client(
                &resolved,
                crate::m_core_cli_adapter::cli_identity_selector(&self.globals.identity),
            )?;
            email::render_inbox(
                client
                    .email()
                    .inbox(query)
                    .map_err(|err| crate::m_core_cli_adapter::map_im_error(err, "mail inbox"))?,
            )
        };
        self.render_mail_result("awiki-cli mail inbox", &resolved, result)
    }

    pub async fn run_mail_inbox_async(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        if self.globals.dry_run {
            return self.run_mail_inbox(command);
        }
        let resolved = self.resolve_config()?;
        let query = email::inbox_query(command)?;
        let client = self.mail_client_async(&resolved).await?;
        let result = email::render_inbox(
            client
                .email()
                .inbox_async(query)
                .await
                .map_err(|err| crate::m_core_cli_adapter::map_im_error(err, "mail inbox"))?,
        );
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
            email::read_plan(&self.globals.identity, &message_id)
        } else {
            let id = email::read_id(command)?;
            let client = crate::m_core_cli_adapter::build_im_client(
                &resolved,
                crate::m_core_cli_adapter::cli_identity_selector(&self.globals.identity),
            )?;
            email::render_read(
                client
                    .email()
                    .read(id)
                    .map_err(|err| crate::m_core_cli_adapter::map_im_error(err, "mail read"))?,
            )
        };
        self.render_mail_result("awiki-cli mail read", &resolved, result)
    }

    pub async fn run_mail_read_async(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        if self.globals.dry_run {
            return self.run_mail_read(command);
        }
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
        let id = email::read_id(command)?;
        let client = self.mail_client_async(&resolved).await?;
        let result = email::render_read(
            client
                .email()
                .read_async(id)
                .await
                .map_err(|err| crate::m_core_cli_adapter::map_im_error(err, "mail read"))?,
        );
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
        let result =
            if self.globals.dry_run {
                email::mark_read_plan(&self.globals.identity, &command.args)
            } else {
                let request = email::mark_read_request(command)?;
                let client = crate::m_core_cli_adapter::build_im_client(
                    &resolved,
                    crate::m_core_cli_adapter::cli_identity_selector(&self.globals.identity),
                )?;
                email::render_mark_read(client.email().mark_read(request).map_err(|err| {
                    crate::m_core_cli_adapter::map_im_error(err, "mail mark-read")
                })?)
            };
        self.render_mail_result("awiki-cli mail mark-read", &resolved, result)
    }

    pub async fn run_mail_mark_read_async(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        if self.globals.dry_run {
            return self.run_mail_mark_read(command);
        }
        let resolved = self.resolve_config()?;
        if command.args.is_empty() {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "mail mark-read requires at least one message id.",
                "Usage: awiki-cli mail mark-read <MESSAGE_ID...>",
            ));
        }
        let request = email::mark_read_request(command)?;
        let client = self.mail_client_async(&resolved).await?;
        let result = email::render_mark_read(
            client
                .email()
                .mark_read_async(request)
                .await
                .map_err(|err| crate::m_core_cli_adapter::map_im_error(err, "mail mark-read"))?,
        );
        self.render_mail_result("awiki-cli mail mark-read", &resolved, result)
    }

    pub fn run_mail_account(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let result = if self.globals.dry_run {
            email::account_plan(&self.globals.identity)
        } else {
            let client = crate::m_core_cli_adapter::build_im_client(
                &resolved,
                crate::m_core_cli_adapter::cli_identity_selector(&self.globals.identity),
            )?;
            email::render_account(
                client
                    .email()
                    .account()
                    .map_err(|err| crate::m_core_cli_adapter::map_im_error(err, "mail account"))?,
            )
        };
        self.render_mail_result("awiki-cli mail account", &resolved, result)
    }

    pub async fn run_mail_account_async(&self) -> Result<(), ExitError> {
        if self.globals.dry_run {
            return self.run_mail_account();
        }
        let resolved = self.resolve_config()?;
        let client = self.mail_client_async(&resolved).await?;
        let result = email::render_account(
            client
                .email()
                .account_async()
                .await
                .map_err(|err| crate::m_core_cli_adapter::map_im_error(err, "mail account"))?,
        );
        self.render_mail_result("awiki-cli mail account", &resolved, result)
    }

    pub fn run_mail_send(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let to_raw = command.flags.get("to").cloned().unwrap_or_default();
        let cc_raw = command.flags.get("cc").cloned().unwrap_or_default();
        let subject = command.flags.get("subject").cloned().unwrap_or_default();
        let body = command.flags.get("body").cloned().unwrap_or_default();
        let html = command.flags.get("html").cloned().unwrap_or_default();
        let to = email::split_mail_list(&to_raw);
        let cc = email::split_mail_list(&cc_raw);
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
            email::send_plan(&self.globals.identity, &to, &cc, &subject, &html)
        } else {
            let request = email::send_request(command)?;
            let client = crate::m_core_cli_adapter::build_im_client(
                &resolved,
                crate::m_core_cli_adapter::cli_identity_selector(&self.globals.identity),
            )?;
            email::render_send(
                client
                    .email()
                    .send(request)
                    .map_err(|err| crate::m_core_cli_adapter::map_im_error(err, "mail send"))?,
            )
        };
        self.render_mail_result("awiki-cli mail send", &resolved, result)
    }

    pub async fn run_mail_send_async(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        if self.globals.dry_run {
            return self.run_mail_send(command);
        }
        let resolved = self.resolve_config()?;
        let to_raw = command.flags.get("to").cloned().unwrap_or_default();
        let subject = command.flags.get("subject").cloned().unwrap_or_default();
        let body = command.flags.get("body").cloned().unwrap_or_default();
        let to = email::split_mail_list(&to_raw);
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
        let request = email::send_request(command)?;
        let client = self.mail_client_async(&resolved).await?;
        let result = email::render_send(
            client
                .email()
                .send_async(request)
                .await
                .map_err(|err| crate::m_core_cli_adapter::map_im_error(err, "mail send"))?,
        );
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
                email::attachment_download_plan(
                    &self.globals.identity,
                    &message_id,
                    attachment_index,
                    &output,
                ),
            );
        }
        let request = email::attachment_request(command)?;
        let client = crate::m_core_cli_adapter::build_im_client(
            &resolved,
            crate::m_core_cli_adapter::cli_identity_selector(&self.globals.identity),
        )?;
        let result = client.email().download_attachment(request).map_err(|err| {
            crate::m_core_cli_adapter::map_im_error(err, "mail attachment download")
        })?;
        self.render_attachment_download_result(
            &resolved,
            &message_id,
            attachment_index,
            &output,
            result,
        )
    }

    pub async fn run_mail_attachment_download_async(
        &self,
        command: &ParsedCommand,
    ) -> Result<(), ExitError> {
        if self.globals.dry_run {
            return self.run_mail_attachment_download(command);
        }
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
        let request = email::attachment_request(command)?;
        let client = self.mail_client_async(&resolved).await?;
        let result = client
            .email()
            .download_attachment_async(request)
            .await
            .map_err(|err| {
                crate::m_core_cli_adapter::map_im_error(err, "mail attachment download")
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
            email::notifications_plan(&self.globals.identity, limit)
        } else {
            let query = email::notification_query(command)?;
            let client = crate::m_core_cli_adapter::build_im_client(
                &resolved,
                crate::m_core_cli_adapter::cli_identity_selector(&self.globals.identity),
            )?;
            email::render_notifications(
                client
                    .email()
                    .notifications(query)
                    .map_err(|err| crate::m_core_cli_adapter::map_im_error(err, "mail notify"))?,
            )
        };
        self.render_mail_result("awiki-cli mail notify", &resolved, result)
    }

    pub async fn run_mail_notify_async(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        if self.globals.dry_run {
            return self.run_mail_notify(command);
        }
        let resolved = self.resolve_config()?;
        let query = email::notification_query(command)?;
        let client = self.mail_client_async(&resolved).await?;
        let result = email::render_notifications(
            client
                .email()
                .notifications_async(query)
                .await
                .map_err(|err| crate::m_core_cli_adapter::map_im_error(err, "mail notify"))?,
        );
        self.render_mail_result("awiki-cli mail notify", &resolved, result)
    }

    fn render_mail_result(
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

    fn render_attachment_download_result(
        &self,
        resolved: &crate::workspace_config::Resolved,
        _message_id: &str,
        attachment_index: i64,
        output_path: &str,
        content: im_core::email::EmailAttachmentContent,
    ) -> Result<(), ExitError> {
        let filename = if content.filename.trim().is_empty() {
            format!("attachment_{attachment_index}")
        } else {
            content.filename.clone()
        };
        let final_path = if output_path.trim().is_empty() {
            filename.clone()
        } else {
            output_path.to_string()
        };
        if let Some(parent) = Path::new(&final_path).parent() {
            if parent != Path::new("") && parent != Path::new(".") {
                create_attachment_parent_dir(parent).map_err(|err| {
                    ExitError::new(
                        "internal_error",
                        1,
                        err.to_string(),
                        "Check write permissions for the output directory.",
                    )
                })?;
            }
        }
        write_attachment_file(Path::new(&final_path), &content.bytes).map_err(|err| {
            ExitError::new(
                "internal_error",
                1,
                err.to_string(),
                "Check write permissions for the output file.",
            )
        })?;
        let (data, summary, warnings) = email::render_attachment_saved(&content, &final_path);
        self.render_success(
            "awiki-cli mail attachment download",
            resolved,
            data,
            &summary,
            warnings,
        )
    }

    async fn mail_client_async(
        &self,
        resolved: &crate::workspace_config::Resolved,
    ) -> Result<im_core::ImClient, ExitError> {
        crate::m_core_cli_adapter::build_im_client_async(
            resolved,
            crate::m_core_cli_adapter::cli_identity_selector(&self.globals.identity),
        )
        .await
    }
}

#[cfg(unix)]
fn create_attachment_parent_dir(path: &Path) -> std::io::Result<()> {
    DirBuilder::new().recursive(true).mode(0o700).create(path)
}

#[cfg(not(unix))]
fn create_attachment_parent_dir(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)
}

#[cfg(unix)]
fn write_attachment_file(path: &Path, content: &[u8]) -> std::io::Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(content)
}

#[cfg(not(unix))]
fn write_attachment_file(path: &Path, content: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, content)
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
