use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionInteraction {
    pub source: String,
    pub definition_hash: String,
    pub can_custom_answer: bool,
    pub can_additional_text: bool,
    pub can_cancel_question: bool,
}
impl QuestionInteraction {
    pub fn new(request: &Value, shared: bool) -> Result<Self> {
        let source = if shared {
            "awiki_mcp"
        } else {
            "acp_elicitation"
        };
        let digest = Sha256::digest(serde_json::to_vec(&(source, request))?);
        Ok(Self {
            source: source.into(),
            definition_hash: format!("{digest:x}"),
            can_custom_answer: shared,
            can_additional_text: shared,
            can_cancel_question: !shared,
        })
    }
}

/// Resolve expiry under the same transaction as answering/stopping. A response
/// committed after the poller's read still wins; timeout must not overwrite it.
pub fn expire_or_answer(question: &mut super::store::Question, now: i64) -> Result<Option<Value>> {
    if let Some(answer) = &question.response {
        return Ok(Some(answer.clone()));
    }
    if question.end_reason.is_some() {
        bail!("question_closed");
    }
    if now >= question.expires_at_ms {
        question.end_reason = Some("expired".into());
    }
    Ok(None)
}

/// Validate against the immutable stored definition, never caller capabilities.
pub fn validate_response(question: &super::store::Question, args: &Value) -> Result<Value> {
    let answer = &args["response"];
    let fields = answer.as_object().context("invalid_answer")?;
    if serde_json::to_vec(answer)?.len() > 64 * 1024 {
        bail!("answer_size_limit");
    }
    let interaction = question.interaction.as_ref();
    if let Some(hash) = args.get("definition_hash") {
        if hash.as_str() != interaction.map(|i| i.definition_hash.as_str()) {
            bail!("question_definition_changed");
        }
    }
    let enhanced = match answer.get("answer_format") {
        None => false,
        Some(format) if format == "awiki.answer.v2" => true,
        _ => bail!("unsupported_answer_format"),
    };
    if enhanced && (interaction.is_none() || args["definition_hash"].as_str().is_none()) {
        bail!("question_definition_required");
    }
    let action = answer["action"].as_str().context("invalid_answer_action")?;
    let allowed: &[&str] = match (action, enhanced) {
        ("decline" | "cancel", false) => &["action"],
        ("decline" | "cancel", true) => &["action", "answer_format"],
        ("accept", false) => &["action", "content"],
        ("accept", true) => &["action", "answer_format", "mode", "content", "text"],
        _ => bail!("invalid_answer_action"),
    };
    if fields.keys().any(|key| !allowed.contains(&key.as_str())) {
        bail!("unknown_answer_field");
    }
    if action != "accept" {
        if action == "cancel" && enhanced && !interaction.unwrap().can_cancel_question {
            bail!("question_cancel_unsupported");
        }
    } else if enhanced {
        let interaction = interaction.unwrap();
        let text = match answer.get("text") {
            None => None,
            Some(value) => Some(value.as_str().context("invalid_answer_text")?),
        };
        if text.is_some_and(|s| s.len() > 16_384) {
            bail!("answer_text_too_long");
        }
        match answer["mode"].as_str() {
            Some("structured") => {
                if text.is_some() && !interaction.can_additional_text {
                    bail!("additional_text_unsupported");
                }
                super::store::validate_answer(&question.request, answer)?;
            }
            Some("custom") => {
                if !interaction.can_custom_answer {
                    bail!("custom_answer_unsupported");
                }
                if answer.get("content").is_some() {
                    bail!("custom_answer_has_content");
                }
                if text.is_none_or(|s| s.trim().is_empty()) {
                    bail!("answer_text_required");
                }
            }
            _ => bail!("invalid_answer_mode"),
        }
    } else {
        super::store::validate_answer(&question.request, answer)?;
    }
    let mut result = answer.clone();
    if interaction.is_some_and(|i| i.source == "awiki_mcp") {
        result["answer_format"] = "awiki.answer.v2".into();
        if action == "accept" && !enhanced {
            result["mode"] = "structured".into();
        }
    }
    Ok(result)
}

/// Forms have a deliberately bounded rendering/validation surface. Unknown
/// interactions terminate with an explicit error instead of inventing an answer.
pub fn validate_schema(request: &Value) -> Result<()> {
    if request["mode"].as_str().is_some_and(|m| m != "form") {
        bail!("unsupported_question_mode");
    }
    let schema = &request["requestedSchema"];
    if schema["type"] != "object" {
        bail!("unsupported_question_schema");
    }
    let properties = schema["properties"]
        .as_object()
        .context("unsupported_question_schema")?;
    if properties.len() > 32 || serde_json::to_vec(request)?.len() > 64 * 1024 {
        bail!("question_size_limit");
    }
    for property in properties.values() {
        match property["type"].as_str() {
            Some("string" | "number" | "integer" | "boolean") => {}
            Some("array")
                if property["items"]["enum"].is_array()
                    || property["items"]["anyOf"].is_array() => {}
            _ => bail!("unsupported_question_field"),
        }
        if let Some(pattern) = property["pattern"].as_str() {
            regex::Regex::new(pattern).context("unsupported_question_pattern")?;
        }
        if let Some(format) = property["format"].as_str() {
            if !matches!(format, "email" | "uri" | "date" | "date-time") {
                bail!("unsupported_question_format");
            }
        }
    }
    if let Some(required) = schema["required"].as_array() {
        for key in required {
            if !key.as_str().is_some_and(|k| properties.contains_key(k)) {
                bail!("invalid_question_required");
            }
        }
    }
    Ok(())
}

pub fn validate_property(property: &Value, value: &Value) -> Result<()> {
    if let Some(s) = value.as_str() {
        if let Some(pattern) = property["pattern"].as_str() {
            if !regex::Regex::new(pattern)?.is_match(s) {
                bail!("answer_pattern_mismatch");
            }
        }
        let valid = match property["format"].as_str() {
            Some("email") => s.split_once('@').is_some_and(|(local, domain)| {
                !local.is_empty() && domain.contains('.') && !s.chars().any(char::is_whitespace)
            }),
            Some("uri") => reqwest::Url::parse(s).is_ok(),
            Some("date-time") => {
                time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339)
                    .is_ok()
            }
            Some("date") => time::format_description::parse("[year]-[month]-[day]")
                .is_ok_and(|f| time::Date::parse(s, &f).is_ok()),
            None => true,
            _ => false,
        };
        if !valid {
            bail!("answer_format_mismatch");
        }
    }
    if let Some(items) = value.as_array() {
        if property["minItems"]
            .as_u64()
            .is_some_and(|v| items.len() < (v as usize))
            || property["maxItems"]
                .as_u64()
                .is_some_and(|v| items.len() > (v as usize))
        {
            bail!("answer_selection_count");
        }
        for (index, item) in items.iter().enumerate() {
            if items[..index].contains(item) {
                bail!("duplicate_answer_choice");
            }
            if let Some(choices) = property["items"]["anyOf"].as_array() {
                if !choices.iter().any(|v| v["const"] == *item) {
                    bail!("invalid_answer_choice");
                }
            }
        }
    }
    Ok(())
}
