use std::fs;

use im_core::prelude::{
    Cursor, GroupRef, HistoryQuery, InboxQuery, InboxScope, MessageBody, MessageDeliveryOptions,
    MessageKind, MessageSecurityMode, MessageTarget, PageLimit, PeerRef, SendMessageRequest,
    ThreadRef,
};

use crate::cli::ParsedCommand;
use crate::output::ExitError;

pub fn send_message_request(
    command: &ParsedCommand,
    default_domain: &str,
) -> Result<SendMessageRequest, ExitError> {
    let target = message_target(command, default_domain)?;
    let body = message_body(command)?;
    let security = message_security(command)?;
    Ok(SendMessageRequest {
        target,
        body,
        security,
        client_message_id: None,
        delivery: MessageDeliveryOptions::default(),
    })
}

pub fn inbox_query(command: &ParsedCommand) -> Result<InboxQuery, ExitError> {
    Ok(InboxQuery {
        scope: inbox_scope(&string_flag(command, "scope"))?,
        limit: page_limit(command, "limit", 20)?,
        cursor: optional_cursor(command)?,
        unread_only: bool_flag(command, "unread"),
    })
}

pub fn history_request(
    command: &ParsedCommand,
    default_domain: &str,
) -> Result<(ThreadRef, HistoryQuery), ExitError> {
    let with = string_flag(command, "with");
    let group = string_flag(command, "group");
    let thread = match (with.trim().is_empty(), group.trim().is_empty()) {
        (false, true) => ThreadRef::Direct(parse_peer(&with, default_domain)?),
        (true, false) => ThreadRef::Group(parse_group(&group)?),
        (true, true) => {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "history requires either --with or --group.",
                "Use --with <handle|did> for direct history or --group <group_did>.",
            ));
        }
        (false, false) => {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "history accepts either --with or --group, but not both.",
                "Choose direct history with --with or group history with --group.",
            ));
        }
    };
    Ok((
        thread,
        HistoryQuery {
            limit: page_limit(command, "limit", 50)?,
            cursor: optional_cursor(command)?,
        },
    ))
}

fn message_target(
    command: &ParsedCommand,
    default_domain: &str,
) -> Result<MessageTarget, ExitError> {
    let to = string_flag(command, "to");
    let group = string_flag(command, "group");
    match (to.trim().is_empty(), group.trim().is_empty()) {
        (false, true) => Ok(MessageTarget::Direct(parse_peer(&to, default_domain)?)),
        (true, false) => Ok(MessageTarget::Group(parse_group(&group)?)),
        (true, true) => Err(ExitError::new(
            "invalid_argument",
            2,
            "msg send requires either --to or --group.",
            "Use --to <handle|did> or --group <group_did>.",
        )),
        (false, false) => Err(ExitError::new(
            "invalid_argument",
            2,
            "msg send accepts either --to or --group, but not both.",
            "Choose direct messaging with --to or group messaging with --group.",
        )),
    }
}

fn message_body(command: &ParsedCommand) -> Result<MessageBody, ExitError> {
    let file_path = string_flag(command, "file");
    if !file_path.trim().is_empty() {
        return Err(ExitError::new(
            "unsupported_capability",
            2,
            "attachments are not supported by the Phase 1 IM Core adapter.",
            "Use the existing legacy attachment command path until attachment migration starts.",
        ));
    }
    let mut text = string_flag(command, "text");
    let text_file = string_flag(command, "text-file");
    if !text.trim().is_empty() && !text_file.trim().is_empty() {
        return Err(ExitError::new(
            "invalid_argument",
            2,
            "Use either --text or --text-file, not both.",
            "Choose one message body source.",
        ));
    }
    if text.trim().is_empty() && !text_file.trim().is_empty() {
        text = fs::read_to_string(&text_file).map_err(|err| {
            ExitError::new(
                "invalid_argument",
                2,
                format!("read text file {text_file:?}: {err}"),
                "Check the --text-file path and permissions.",
            )
        })?;
    }
    if text.trim().is_empty() {
        return Err(ExitError::new(
            "invalid_argument",
            2,
            "msg send requires --text or --text-file.",
            "Provide a text body for Phase 1 IM Core messages.",
        ));
    }
    Ok(MessageBody::Text {
        text,
        kind: message_kind(&string_flag(command, "type"))?,
    })
}

fn message_kind(raw: &str) -> Result<MessageKind, ExitError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "" | "text" => Ok(MessageKind::Text),
        "markdown" => Ok(MessageKind::Markdown),
        value => Err(ExitError::new(
            "unsupported_capability",
            2,
            format!("message type {value:?} is not supported by the Phase 1 IM Core adapter."),
            "Use --type text or --type markdown.",
        )),
    }
}

fn message_security(command: &ParsedCommand) -> Result<MessageSecurityMode, ExitError> {
    match string_flag(command, "secure")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "" | "default" => Ok(MessageSecurityMode::DefaultPlain),
        "plain" | "off" | "false" => Ok(MessageSecurityMode::Plain),
        "direct" | "secure-direct" | "on" | "true" => Err(ExitError::new(
            "unsupported_capability",
            2,
            "secure direct messages are not supported by the Phase 1 IM Core adapter.",
            "Use the existing legacy secure command path until secure migration starts.",
        )),
        "group-e2ee" | "e2ee" => Err(ExitError::new(
            "unsupported_capability",
            2,
            "group E2EE is not supported by the Phase 1 IM Core adapter.",
            "Use the existing legacy group E2EE command path until secure migration starts.",
        )),
        value => Err(ExitError::new(
            "invalid_argument",
            2,
            format!("unsupported --secure value {value:?}."),
            "Use --secure plain, --secure off, or leave it unset for Phase 1.",
        )),
    }
}

fn inbox_scope(raw: &str) -> Result<InboxScope, ExitError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "" | "all" => Ok(InboxScope::All),
        "direct" | "direct-only" => Ok(InboxScope::DirectOnly),
        "group" | "group-only" => Ok(InboxScope::GroupOnly),
        value => Err(ExitError::new(
            "invalid_argument",
            2,
            format!("unsupported inbox scope {value:?}."),
            "Use --scope all, --scope direct, or --scope group.",
        )),
    }
}

fn page_limit(command: &ParsedCommand, flag: &str, default: u32) -> Result<PageLimit, ExitError> {
    let raw = string_flag(command, flag);
    let value = if raw.trim().is_empty() {
        default
    } else {
        raw.trim().parse::<u32>().map_err(|err| {
            ExitError::new(
                "invalid_argument",
                2,
                format!("invalid --{flag}: {err}"),
                "Use a positive integer limit.",
            )
        })?
    };
    PageLimit::new(value).map_err(|err| {
        ExitError::new(
            "invalid_argument",
            2,
            format!("invalid --{flag}: {err}"),
            "Use a positive integer limit.",
        )
    })
}

fn optional_cursor(command: &ParsedCommand) -> Result<Option<Cursor>, ExitError> {
    let raw = string_flag(command, "cursor");
    if raw.trim().is_empty() {
        return Ok(None);
    }
    Cursor::parse(raw).map(Some).map_err(|err| {
        ExitError::new(
            "invalid_argument",
            2,
            format!("invalid --cursor: {err}"),
            "Use a non-empty cursor returned by the service.",
        )
    })
}

fn parse_peer(raw: &str, default_domain: &str) -> Result<PeerRef, ExitError> {
    PeerRef::parse(raw, default_domain).map_err(|err| {
        ExitError::new(
            "invalid_argument",
            2,
            format!("invalid peer target: {err}"),
            "Use a peer DID or handle.",
        )
    })
}

fn parse_group(raw: &str) -> Result<GroupRef, ExitError> {
    GroupRef::parse(raw).map_err(|err| {
        ExitError::new(
            "invalid_argument",
            2,
            format!("invalid group target: {err}"),
            "Use an existing group DID or id.",
        )
    })
}

fn bool_flag(command: &ParsedCommand, name: &str) -> bool {
    string_flag(command, name).trim() == "true"
}

fn string_flag(command: &ParsedCommand, name: &str) -> String {
    command.flags.get(name).cloned().unwrap_or_default()
}
