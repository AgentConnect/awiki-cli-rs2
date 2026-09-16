use anyhow::{bail, Context, Result};
use serde_json::Value;

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
