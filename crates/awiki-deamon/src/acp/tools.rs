//! Safe, bounded tool metadata. Raw input/output and shell commands are never
//! copied into the durable activity projection.
use serde_json::{json, Value};

pub fn update_summary(previous: Option<&Value>, update: &Value) -> Value {
    let mut summary = previous
        .cloned()
        .unwrap_or_else(|| json!({"id":update["toolCallId"]}));
    for key in ["status", "kind"] {
        if update[key].is_string() {
            summary[key] = update[key].clone();
        }
    }
    if let Some(title) = update["title"].as_str() {
        let lower = title.to_ascii_lowercase();
        let safe_title = match lower.as_str() {
            "read" | "read_file" => "Read",
            "shell" | "bash" | "run_shell_command" => "Shell",
            "search" | "googlesearch" | "google_web_search" | "websearch" => "Search",
            "question" | "ask_user" | "askuserquestion" => "question",
            _ if lower.contains("request_user_input") => "request_user_input",
            _ => "Tool",
        };
        summary["title"] = json!(safe_title);
        // Some older clients provide a bare file path as title and no locations.
        // Preserve only its basename; it does not imply a Read action.
        if title.starts_with('/') || title.starts_with("Users/") {
            summary["target"] = basename(title).map(Value::String).unwrap_or(Value::Null);
        }
    }
    if let Some(locations) = update["locations"].as_array() {
        summary["target"] = locations
            .iter()
            .find_map(|location| location["path"].as_str().and_then(basename))
            .map(Value::String)
            .unwrap_or(Value::Null);
    }
    summary
}

fn basename(path: &str) -> Option<String> {
    let normalized = path.replace('\\', "/");
    let name = normalized.rsplit('/').next()?;
    let lower = name.to_ascii_lowercase();
    if name.is_empty()
        || name.len() > 200
        || name.chars().any(|c| c.is_control() || "?=#".contains(c))
        || lower.contains("sk-")
        || lower.contains("token:")
        || lower.contains("secret:")
    {
        return None;
    }
    Some(name.to_owned())
}
