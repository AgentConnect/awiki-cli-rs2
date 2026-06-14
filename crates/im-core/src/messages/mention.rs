use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

const RANGE_UNIT_UNICODE_CODE_POINT: &str = "unicode_code_point";
const FORBIDDEN_MENTION_FIELDS: &[&str] = &[
    "sender",
    "sender_did",
    "from",
    "actor_did",
    "auth",
    "origin_proof",
    "proof",
    "signature",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageMentionPayload {
    pub text: String,
    pub mentions: Vec<MessageMention>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Value>,
}

impl MessageMentionPayload {
    pub fn from_value(value: &Value) -> crate::ImResult<Self> {
        validate_message_mention_payload_shape(value)?;
        let payload: Self = serde_json::from_value(value.clone()).map_err(|err| {
            crate::ImError::invalid_input(
                Some("payload".to_owned()),
                format!("invalid ANP P9 mention payload: {err}"),
            )
        })?;
        payload.validate()?;
        Ok(payload)
    }

    pub fn to_value(&self) -> crate::ImResult<Value> {
        self.validate()?;
        serde_json::to_value(self).map_err(|err| crate::ImError::Serialization {
            detail: err.to_string(),
        })
    }

    pub fn validate(&self) -> crate::ImResult<()> {
        if self.text.is_empty() {
            return invalid_mention_payload("text must not be empty");
        }
        if self.mentions.is_empty() {
            return invalid_mention_payload("mentions must not be empty");
        }
        let code_points = self.text.chars().count();
        let mut ids = HashSet::new();
        for mention in &self.mentions {
            mention.validate(code_points)?;
            if !ids.insert(mention.id.as_str()) {
                return invalid_mention_payload(format!("duplicate mention id: {}", mention.id));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageMention {
    pub id: String,
    pub range: MessageMentionRange,
    pub target: MessageMentionTarget,
    #[serde(default = "default_mention_role")]
    pub mention_role: MessageMentionRole,
}

impl MessageMention {
    fn validate(&self, text_code_points: usize) -> crate::ImResult<()> {
        if self.id.trim().is_empty() {
            return invalid_mention_payload("mention id must not be empty");
        }
        self.range.validate(text_code_points)?;
        self.target.validate()?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageMentionRange {
    pub start: usize,
    pub end: usize,
    pub unit: MessageMentionRangeUnit,
}

impl MessageMentionRange {
    fn validate(&self, text_code_points: usize) -> crate::ImResult<()> {
        if self.start >= self.end {
            return invalid_mention_payload("mention range start must be less than end");
        }
        if self.end > text_code_points {
            return invalid_mention_payload("mention range end exceeds text length");
        }
        if self.unit != MessageMentionRangeUnit::UnicodeCodePoint {
            return invalid_mention_payload("mention range unit must be unicode_code_point");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageMentionRangeUnit {
    UnicodeCodePoint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MessageMentionTarget {
    Human {
        did: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        display_name: Option<String>,
    },
    Agent {
        did: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        display_name: Option<String>,
    },
    GroupSelector {
        selector: MessageMentionSelector,
    },
}

impl MessageMentionTarget {
    fn validate(&self) -> crate::ImResult<()> {
        match self {
            Self::Human { did, .. } | Self::Agent { did, .. } => {
                if !looks_like_did(did) {
                    return invalid_mention_payload("human/agent mention target did must be a DID");
                }
                Ok(())
            }
            Self::GroupSelector { .. } => Ok(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageMentionSelector {
    All,
    Agents,
    Humans,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageMentionRole {
    Addressee,
    Cc,
}

fn default_mention_role() -> MessageMentionRole {
    MessageMentionRole::Addressee
}

pub fn parse_message_mention_payload(value: &Value) -> crate::ImResult<MessageMentionPayload> {
    MessageMentionPayload::from_value(value)
}

pub fn validate_message_mention_payload(value: &Value) -> crate::ImResult<()> {
    MessageMentionPayload::from_value(value).map(|_| ())
}

pub fn is_message_mention_payload(value: &Value) -> bool {
    value
        .as_object()
        .and_then(|object| object.get("mentions"))
        .is_some_and(Value::is_array)
}

fn validate_message_mention_payload_shape(value: &Value) -> crate::ImResult<()> {
    let object = value.as_object().ok_or_else(|| {
        crate::ImError::invalid_input(
            Some("payload".to_owned()),
            "ANP P9 mention payload must be a JSON object".to_owned(),
        )
    })?;
    match object.get("text") {
        Some(Value::String(_)) => {}
        _ => return invalid_mention_payload("mention payload text must be a string"),
    }
    let mentions = match object.get("mentions") {
        Some(Value::Array(mentions)) => mentions,
        _ => return invalid_mention_payload("mention payload mentions must be an array"),
    };
    if let Some(annotations) = object.get("annotations") {
        if !annotations.is_object() {
            return invalid_mention_payload("mention payload annotations must be an object");
        }
    }
    for (index, mention) in mentions.iter().enumerate() {
        validate_raw_mention(index, mention)?;
    }
    Ok(())
}

fn validate_raw_mention(index: usize, mention: &Value) -> crate::ImResult<()> {
    let object = mention.as_object().ok_or_else(|| {
        crate::ImError::invalid_input(
            Some(format!("mentions[{index}]")),
            "mention must be a JSON object".to_owned(),
        )
    })?;
    for field in FORBIDDEN_MENTION_FIELDS {
        if object.contains_key(*field) {
            return invalid_mention_payload(format!(
                "mention must not contain forbidden field `{field}`"
            ));
        }
    }
    let target = object
        .get("target")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            crate::ImError::invalid_input(
                Some(format!("mentions[{index}].target")),
                "mention target must be an object".to_owned(),
            )
        })?;
    match target.get("kind").and_then(Value::as_str) {
        Some("human") | Some("agent") => {
            if target.get("selector").is_some() {
                return invalid_mention_payload(
                    "human/agent mention target must not contain selector",
                );
            }
        }
        Some("group_selector") => {
            if target.get("did").is_some() {
                return invalid_mention_payload(
                    "group selector mention target must not contain did",
                );
            }
        }
        Some(_) => return invalid_mention_payload("unsupported mention target kind"),
        None => return invalid_mention_payload("mention target kind must be present"),
    }
    Ok(())
}

fn looks_like_did(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.starts_with("did:") && trimmed.len() > "did:".len()
}

fn invalid_mention_payload<T>(message: impl Into<String>) -> crate::ImResult<T> {
    Err(crate::ImError::invalid_input(
        Some("mentions".to_owned()),
        message.into(),
    ))
}
