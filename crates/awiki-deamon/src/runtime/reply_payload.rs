use serde_json::{json, Value};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuredGroupReply {
    pub text: String,
    pub payload: Value,
}

#[derive(Debug, Clone, Copy)]
pub struct StructuredGroupReplyInput<'a> {
    pub run_id: &'a str,
    pub agent_did: &'a str,
    pub requester_did: &'a str,
    pub requester_full_handle: Option<&'a str>,
    pub source_message_id: Option<&'a str>,
    pub reply_text: &'a str,
}

#[derive(Debug, Clone, Copy)]
pub struct StructuredDirectReplyInput<'a> {
    pub agent_did: &'a str,
    pub source_message_id: &'a str,
    pub reply_text: &'a str,
}

pub fn structured_direct_reply(
    input: StructuredDirectReplyInput<'_>,
) -> Option<StructuredGroupReply> {
    let agent_did = input.agent_did.trim();
    let source_message_id = input.source_message_id.trim();
    let reply_text = input.reply_text.trim();
    if agent_did.is_empty() || source_message_id.is_empty() || reply_text.is_empty() {
        return None;
    }
    Some(StructuredGroupReply {
        text: reply_text.to_owned(),
        payload: json!({
            "text": reply_text,
            "mentions": [],
            "annotations": {
                "awiki_reply_to_message_id": source_message_id,
                "awiki_reply_from_agent_did": agent_did,
            }
        }),
    })
}

pub fn structured_group_reply(
    input: StructuredGroupReplyInput<'_>,
) -> Option<StructuredGroupReply> {
    let requester_did = input.requester_did.trim();
    if requester_did.is_empty() {
        return None;
    }
    let reply_text = input.reply_text.trim();
    if reply_text.is_empty() {
        return None;
    }

    let mention_surface = mention_surface_for_sender(input.requester_full_handle, requester_did);
    let text = text_with_leading_mention(&mention_surface, reply_text);
    let mention_end = mention_surface.chars().count();
    let source_message_id = input
        .source_message_id
        .map(|value| json!(value))
        .unwrap_or(Value::Null);

    Some(StructuredGroupReply {
        text: text.clone(),
        payload: json!({
            "text": text,
            "mentions": [{
                "id": format!("reply_{}", stable_id_suffix(&format!("{}:{}", input.run_id, requester_did))),
                "range": {
                    "start": 0,
                    "end": mention_end,
                    "unit": "unicode_code_point"
                },
                "target": {
                    "kind": "human",
                    "did": requester_did,
                    "display_name": mention_surface.trim_start_matches('@')
                },
                "mention_role": "addressee"
            }],
            "annotations": {
                "awiki_reply_to_message_id": source_message_id,
                "awiki_reply_from_agent_did": input.agent_did
            }
        }),
    })
}

pub fn group_did_from_conversation_id(conversation_id: &str) -> Option<&str> {
    conversation_id
        .trim()
        .strip_prefix("group:")
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub fn mention_surface_for_sender(full_handle: Option<&str>, sender_did: &str) -> String {
    if let Some(handle) = full_handle.and_then(short_handle) {
        return format!("@{handle}");
    }
    if let Some(handle) = short_handle_from_wba_did(sender_did) {
        return format!("@{handle}");
    }
    let sender_did = sender_did.trim();
    let compact = if sender_did.len() <= 18 {
        sender_did.to_string()
    } else {
        format!(
            "{}...{}",
            &sender_did[..10],
            &sender_did[sender_did.len().saturating_sub(6)..]
        )
    };
    format!("@{compact}")
}

fn text_with_leading_mention(mention_surface: &str, reply_text: &str) -> String {
    let reply_text = reply_text.trim();
    if reply_text == mention_surface {
        return reply_text.to_string();
    }
    if reply_text
        .strip_prefix(mention_surface)
        .is_some_and(|rest| rest.starts_with(char::is_whitespace))
    {
        return reply_text.to_string();
    }
    format!("{mention_surface} {reply_text}")
}

fn short_handle(value: &str) -> Option<String> {
    let mut trimmed = value.trim();
    if trimmed.is_empty() || trimmed.starts_with("did:") {
        return None;
    }
    while let Some(rest) = trimmed.strip_prefix('@') {
        trimmed = rest.trim_start();
    }
    if let Some(rest) = trimmed.strip_prefix("wba://") {
        trimmed = rest.trim_start();
    }
    let handle = match trimmed.find('.') {
        Some(index) if index > 0 => &trimmed[..index],
        _ => trimmed,
    }
    .trim();
    if handle.is_empty() {
        None
    } else {
        Some(handle.to_string())
    }
}

fn short_handle_from_wba_did(did: &str) -> Option<String> {
    let parts = did.trim().split(':').collect::<Vec<_>>();
    if parts.len() >= 6 && parts[0] == "did" && parts[1] == "wba" {
        if parts[3] == "user" {
            return short_handle(parts[4]);
        }
        if parts[3] == "agent" && parts.len() >= 7 {
            return short_handle(parts[5]);
        }
        return short_handle(parts[3]);
    }
    None
}

fn stable_id_suffix(input: &str) -> String {
    let digest = Sha256::digest(input.as_bytes());
    digest
        .iter()
        .take(16)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mention_surface_uses_short_handle_from_full_handle() {
        assert_eq!(
            mention_surface_for_sender(Some("bob.anpclaw.com"), "did:human:bob"),
            "@bob"
        );
    }

    #[test]
    fn mention_surface_derives_short_handle_from_wba_did() {
        assert_eq!(
            mention_surface_for_sender(None, "did:wba:awiki.info:user:alice:e1_sender"),
            "@alice"
        );
    }

    #[test]
    fn structured_group_reply_keeps_existing_leading_mention() {
        let reply = structured_group_reply(StructuredGroupReplyInput {
            run_id: "run_1",
            agent_did: "did:agent:codex",
            requester_did: "did:human:bob",
            requester_full_handle: Some("bob.anpclaw.com"),
            source_message_id: Some("msg_1"),
            reply_text: "@bob done",
        })
        .unwrap();

        assert_eq!(reply.text, "@bob done");
        assert_eq!(reply.payload["text"], "@bob done");
        assert_eq!(reply.payload["mentions"][0]["range"]["end"], 4);
        assert_eq!(
            reply.payload["mentions"][0]["target"]["did"],
            "did:human:bob"
        );
    }

    #[test]
    fn structured_direct_reply_preserves_text_and_exact_source_message() {
        let reply = structured_direct_reply(StructuredDirectReplyInput {
            agent_did: "did:agent:codex",
            source_message_id: "msg_2",
            reply_text: "still here",
        })
        .unwrap();

        assert_eq!(reply.text, "still here");
        assert_eq!(reply.payload["text"], "still here");
        assert_eq!(reply.payload["mentions"], json!([]));
        assert_eq!(
            reply.payload["annotations"]["awiki_reply_to_message_id"],
            "msg_2"
        );
    }
}
