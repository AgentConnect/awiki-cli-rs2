use super::{identity_exit, internal_anyhow, not_implemented_side_effect, App};
use crate::cli::ParsedCommand;
use crate::mail::{self, CommandResult, MailError};
use crate::output::ExitError;

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
        if !self.globals.dry_run {
            return Err(not_implemented_side_effect("mail inbox"));
        }
        self.render_mail_result(
            "awiki-cli mail inbox",
            &resolved,
            mail::inbox_plan(&self.globals.identity, &folder, limit, offset, unread_only),
        )
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
        if !self.globals.dry_run {
            return Err(not_implemented_side_effect("mail read"));
        }
        self.render_mail_result(
            "awiki-cli mail read",
            &resolved,
            mail::read_plan(&self.globals.identity, &message_id),
        )
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
        if !self.globals.dry_run {
            return Err(not_implemented_side_effect("mail mark-read"));
        }
        self.render_mail_result(
            "awiki-cli mail mark-read",
            &resolved,
            mail::mark_read_plan(&self.globals.identity, &command.args),
        )
    }

    pub fn run_mail_account(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        if !self.globals.dry_run {
            return Err(not_implemented_side_effect("mail account"));
        }
        self.render_mail_result(
            "awiki-cli mail account",
            &resolved,
            mail::account_plan(&self.globals.identity),
        )
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
        if !self.globals.dry_run {
            return Err(not_implemented_side_effect("mail send"));
        }
        self.render_mail_result(
            "awiki-cli mail send",
            &resolved,
            mail::send_plan(&self.globals.identity, &to, &cc, &subject, &html),
        )
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
        if !self.globals.dry_run {
            return Err(not_implemented_side_effect("mail attachment download"));
        }
        self.render_mail_result(
            "awiki-cli mail attachment download",
            &resolved,
            mail::attachment_download_plan(
                &self.globals.identity,
                &message_id,
                attachment_index,
                &output,
            ),
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
            .map_err(mail_exit)?
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

fn mail_exit(err: MailError) -> ExitError {
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
        MailError::Identity(err) => identity_exit(err),
        MailError::Store(err) => super::store_exit(
            err,
            "Ensure the runtime listener is running in websocket mode and has received notifications.",
        ),
        MailError::Internal(message) => internal_anyhow(anyhow::anyhow!(message)),
    }
}
