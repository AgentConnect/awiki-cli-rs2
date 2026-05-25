use im_core::prelude::{
    ContactListQuery, Did, FollowRequest, Handle, PageLimit, PeerRef, RelationshipListQuery,
    SaveContactRequest, UnfollowRequest,
};
use serde_json::json;

use crate::cli_output::ExitError;
use crate::cli_parser::ParsedCommand;
use crate::m_core_cli_adapter::message_result::CommandResult;

pub fn follow_plan(command: &ParsedCommand, did_domain: &str) -> Result<CommandResult, ExitError> {
    let peer = required_peer_arg(command, did_domain, "people follow")?;
    Ok(CommandResult {
        data: json!({
            "plan": {
                "action": "follow",
                "service": "im-core.directory",
                "operation": "people.follow",
                "remote_call": "directory.follow",
                "status_refresh": "directory.relationship_status",
                "target": peer.as_str(),
                "local_writes": ["contacts", "relationship_events"],
            }
        }),
        summary: "Dry run: follow planned".to_string(),
        warnings: Vec::new(),
    })
}

pub fn unfollow_plan(
    command: &ParsedCommand,
    did_domain: &str,
) -> Result<CommandResult, ExitError> {
    let peer = required_peer_arg(command, did_domain, "people unfollow")?;
    Ok(CommandResult {
        data: json!({
            "plan": {
                "action": "unfollow",
                "service": "im-core.directory",
                "operation": "people.unfollow",
                "remote_call": "directory.unfollow",
                "status_refresh": "directory.relationship_status",
                "target": peer.as_str(),
                "local_writes": ["contacts", "relationship_events"],
            }
        }),
        summary: "Dry run: unfollow planned".to_string(),
        warnings: Vec::new(),
    })
}

pub fn save_contact_plan(
    command: &ParsedCommand,
    did_domain: &str,
) -> Result<CommandResult, ExitError> {
    let request = contact_save_request(command, did_domain)?;
    Ok(CommandResult {
        data: json!({
            "plan": {
                "action": "contacts.save",
                "service": "im-core.directory",
                "operation": "people.contacts.save",
                "peer": request.peer.as_str(),
                "did": request.did.as_ref().map(|did| did.as_str()),
                "handle": request.handle.as_ref().map(|handle| handle.as_str()),
                "relationship": request.relationship,
                "note": request.note,
                "local_writes": ["contacts", "contact_handle_bindings"],
            }
        }),
        summary: "Dry run: contact save planned".to_string(),
        warnings: Vec::new(),
    })
}

pub fn relationship_status_plan(
    command: &ParsedCommand,
    did_domain: &str,
) -> Result<CommandResult, ExitError> {
    let peer = required_peer_arg(command, did_domain, "people status")?;
    Ok(CommandResult {
        data: json!({
            "plan": {
                "action": "relationship.status",
                "service": "im-core.directory",
                "operation": "people.status",
                "remote_call": "directory.relationship_status",
                "target": peer.as_str(),
            }
        }),
        summary: "Dry run: relationship status planned".to_string(),
        warnings: Vec::new(),
    })
}

pub fn followers_plan(command: &ParsedCommand) -> Result<CommandResult, ExitError> {
    let query = relationship_list_query(command)?;
    Ok(CommandResult {
        data: json!({
            "plan": {
                "action": "relationships.followers",
                "service": "im-core.directory",
                "operation": "people.followers",
                "remote_call": "directory.followers",
                "query": relationship_list_query_value(&query),
            }
        }),
        summary: "Dry run: followers list planned".to_string(),
        warnings: Vec::new(),
    })
}

pub fn following_plan(command: &ParsedCommand) -> Result<CommandResult, ExitError> {
    let query = relationship_list_query(command)?;
    Ok(CommandResult {
        data: json!({
            "plan": {
                "action": "relationships.following",
                "service": "im-core.directory",
                "operation": "people.following",
                "remote_call": "directory.following",
                "query": relationship_list_query_value(&query),
            }
        }),
        summary: "Dry run: following list planned".to_string(),
        warnings: Vec::new(),
    })
}

pub fn contacts_list_plan(command: &ParsedCommand) -> Result<CommandResult, ExitError> {
    let limit = optional_page_limit(command, "limit")?;
    Ok(CommandResult {
        data: json!({
            "plan": {
                "action": "contacts.list",
                "service": "im-core.directory",
                "operation": "people.contacts.list",
                "local_read": "contacts",
                "query": {
                    "limit": limit.as_ref().map(|limit| limit.0),
                },
            }
        }),
        summary: "Dry run: contacts list planned".to_string(),
        warnings: Vec::new(),
    })
}

pub fn follow_via_im_core(
    client: &im_core::ImClient,
    command: &ParsedCommand,
    did_domain: &str,
) -> Result<CommandResult, ExitError> {
    let result = client
        .directory()
        .follow(FollowRequest {
            peer: required_peer_arg(command, did_domain, "people follow")?,
        })
        .map_err(|err| super::map_im_error(err, "people follow"))?;
    let summary = format!("Followed {}", result.did.as_str());
    Ok(CommandResult {
        data: serde_json::to_value(&result).map_err(serialization_exit)?,
        summary,
        warnings: result.warnings,
    })
}

pub fn unfollow_via_im_core(
    client: &im_core::ImClient,
    command: &ParsedCommand,
    did_domain: &str,
) -> Result<CommandResult, ExitError> {
    let result = client
        .directory()
        .unfollow(UnfollowRequest {
            peer: required_peer_arg(command, did_domain, "people unfollow")?,
        })
        .map_err(|err| super::map_im_error(err, "people unfollow"))?;
    let summary = format!("Unfollowed {}", result.did.as_str());
    Ok(CommandResult {
        data: serde_json::to_value(&result).map_err(serialization_exit)?,
        summary,
        warnings: result.warnings,
    })
}

pub fn relationship_status_via_im_core(
    client: &im_core::ImClient,
    command: &ParsedCommand,
    did_domain: &str,
) -> Result<CommandResult, ExitError> {
    let result = client
        .directory()
        .relationship_status(required_peer_arg(command, did_domain, "people status")?)
        .map_err(|err| super::map_im_error(err, "people status"))?;
    let summary = format!("Loaded relationship status for {}", result.did.as_str());
    Ok(CommandResult {
        data: serde_json::to_value(&result).map_err(serialization_exit)?,
        summary,
        warnings: result.warnings,
    })
}

pub fn followers_via_im_core(
    client: &im_core::ImClient,
    command: &ParsedCommand,
) -> Result<CommandResult, ExitError> {
    let query = relationship_list_query(command)?;
    let page = client
        .directory()
        .followers(query)
        .map_err(|err| super::map_im_error(err, "people followers"))?;
    let total = page.items.len();
    Ok(CommandResult {
        data: json!({
            "followers": page.items,
            "items": page.items,
            "has_more": page.has_more,
            "next_cursor": page.next_cursor.as_ref().map(|cursor| cursor.as_str()),
        }),
        summary: format!("Loaded {total} followers"),
        warnings: relationship_list_warnings(&page.items),
    })
}

pub fn following_via_im_core(
    client: &im_core::ImClient,
    command: &ParsedCommand,
) -> Result<CommandResult, ExitError> {
    let query = relationship_list_query(command)?;
    let page = client
        .directory()
        .following(query)
        .map_err(|err| super::map_im_error(err, "people following"))?;
    let total = page.items.len();
    Ok(CommandResult {
        data: json!({
            "following": page.items,
            "items": page.items,
            "has_more": page.has_more,
            "next_cursor": page.next_cursor.as_ref().map(|cursor| cursor.as_str()),
        }),
        summary: format!("Loaded {total} following"),
        warnings: relationship_list_warnings(&page.items),
    })
}

pub fn contacts_list_via_im_core(
    client: &im_core::ImClient,
    command: &ParsedCommand,
) -> Result<CommandResult, ExitError> {
    let limit = optional_page_limit(command, "limit")?;
    let page = client
        .directory()
        .contacts(ContactListQuery { limit })
        .map_err(|err| super::map_im_error(err, "people contacts list"))?;
    let total = page.items.len();
    Ok(CommandResult {
        data: json!({
            "contacts": page.items,
            "items": page.items,
            "has_more": page.has_more,
            "next_cursor": page.next_cursor.as_ref().map(|cursor| cursor.as_str()),
        }),
        summary: format!("Loaded {total} contacts"),
        warnings: Vec::new(),
    })
}

pub fn contacts_save_via_im_core(
    client: &im_core::ImClient,
    command: &ParsedCommand,
    did_domain: &str,
) -> Result<CommandResult, ExitError> {
    let contact = client
        .directory()
        .save_contact(contact_save_request(command, did_domain)?)
        .map_err(|err| super::map_im_error(err, "people contacts save"))?;
    let summary = format!("Saved contact {}", contact.did.as_str());
    Ok(CommandResult {
        data: serde_json::to_value(&contact).map_err(serialization_exit)?,
        summary,
        warnings: Vec::new(),
    })
}

fn required_peer_arg(
    command: &ParsedCommand,
    did_domain: &str,
    usage: &str,
) -> Result<PeerRef, ExitError> {
    if command.args.len() != 1 {
        return Err(ExitError::new(
            "invalid_argument",
            2,
            format!("{usage} requires exactly one target."),
            format!("Usage: awiki-cli {usage} <handle|did>."),
        ));
    }
    parse_peer(&command.args[0], did_domain)
}

fn contact_save_request(
    command: &ParsedCommand,
    did_domain: &str,
) -> Result<SaveContactRequest, ExitError> {
    let raw_did = string_flag(command, "did");
    if raw_did.trim().is_empty() {
        return Err(ExitError::new(
            "invalid_argument",
            2,
            "people contacts save requires --did.",
            "Use --did <did> with optional --handle and --reason.",
        ));
    }
    let did =
        Did::parse(&raw_did).map_err(|err| super::map_im_error(err, "people contacts save"))?;
    let handle = optional_handle(command, "handle", did_domain, "people contacts save")?;
    let peer = match handle.as_ref() {
        Some(handle) => PeerRef::parse(handle.as_str(), "")
            .map_err(|err| super::map_im_error(err, "people contacts save"))?,
        None => PeerRef::parse(did.as_str(), "")
            .map_err(|err| super::map_im_error(err, "people contacts save"))?,
    };
    Ok(SaveContactRequest {
        peer,
        did: Some(did),
        handle,
        display_name: trimmed_optional(&string_flag(command, "name")),
        relationship: trimmed_optional(&string_flag(command, "relationship")),
        note: trimmed_optional(&string_flag(command, "reason")),
    })
}

fn relationship_list_query(command: &ParsedCommand) -> Result<RelationshipListQuery, ExitError> {
    Ok(RelationshipListQuery {
        limit: optional_page_limit(command, "limit")?,
        offset: optional_u32_flag(command, "offset")?,
        hydrate_profiles: bool_flag(command, "profile"),
    })
}

fn relationship_list_query_value(query: &RelationshipListQuery) -> serde_json::Value {
    json!({
        "limit": query.limit.as_ref().map(|limit| limit.0),
        "offset": query.offset,
        "hydrate_profiles": query.hydrate_profiles,
    })
}

fn parse_peer(raw: &str, did_domain: &str) -> Result<PeerRef, ExitError> {
    PeerRef::parse(raw, did_domain).map_err(|err| super::map_im_error(err, "people target"))
}

fn optional_handle(
    command: &ParsedCommand,
    name: &str,
    did_domain: &str,
    context: &'static str,
) -> Result<Option<Handle>, ExitError> {
    let value = string_flag(command, name);
    if value.trim().is_empty() {
        return Ok(None);
    }
    Handle::parse(&value, did_domain)
        .map(Some)
        .map_err(|err| super::map_im_error(err, context))
}

fn optional_page_limit(
    command: &ParsedCommand,
    name: &str,
) -> Result<Option<PageLimit>, ExitError> {
    optional_u32_flag(command, name)?
        .map(PageLimit::new)
        .transpose()
        .map_err(|err| super::map_im_error(err, "people limit"))
}

fn optional_u32_flag(command: &ParsedCommand, name: &str) -> Result<Option<u32>, ExitError> {
    let Some(value) = command
        .flags
        .get(name)
        .filter(|value| !value.trim().is_empty())
    else {
        return Ok(None);
    };
    let parsed = value.parse::<u32>().map_err(|_| {
        ExitError::new(
            "invalid_argument",
            2,
            format!("--{name} must be a non-negative integer."),
            "Pass a numeric value after the flag.",
        )
    })?;
    Ok(Some(parsed))
}

fn relationship_list_warnings(items: &[im_core::directory::RelationshipListItem]) -> Vec<String> {
    items
        .iter()
        .flat_map(|item| item.warnings.clone())
        .collect()
}

fn string_flag(command: &ParsedCommand, name: &str) -> String {
    command.flags.get(name).cloned().unwrap_or_default()
}

fn bool_flag(command: &ParsedCommand, name: &str) -> bool {
    command
        .flags
        .get(name)
        .is_some_and(|value| value.eq_ignore_ascii_case("true"))
}

fn trimmed_optional(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn serialization_exit(err: serde_json::Error) -> ExitError {
    ExitError::new(
        "serialization_error",
        1,
        format!("people output serialization failed: {err}"),
        "Report this issue with the command output.",
    )
}
