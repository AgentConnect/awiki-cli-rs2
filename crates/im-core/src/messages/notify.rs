//! Typed text Notify intent. This is never receiver authorization.
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const NOTIFY_ANNOTATION: &str = "awiki.notify.v1";
pub const NOTIFY_LEVEL_ATTRIBUTE: &str = "notify_level";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotifyLevel {
    Normal,
    Urgent,
}
impl NotifyLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Urgent => "urgent",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "normal" => Some(Self::Normal),
            "urgent" => Some(Self::Urgent),
            _ => None,
        }
    }
}
/// Closed annotation: unknown members or malformed levels cannot request an alert.
pub fn notify_level_from_annotations(annotations: &Value) -> Option<NotifyLevel> {
    let entry = annotations
        .as_object()?
        .get(NOTIFY_ANNOTATION)?
        .as_object()?;
    if entry.len() != 1 {
        return None;
    }
    NotifyLevel::parse(entry.get("level")?.as_str()?)
}

/// Local projection distinguishes malformed Notify from ordinary chat without granting urgency.
pub fn notify_projection_level(annotations: &Value) -> Option<&'static str> {
    annotations.as_object()?.get(NOTIFY_ANNOTATION)?;
    Some(
        notify_level_from_annotations(annotations)
            .map(NotifyLevel::as_str)
            .unwrap_or("invalid"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn closed_notify_annotation_never_imports_sender_authorization() {
        for value in [
            serde_json::json!({"level":"urgent", "authorized":true}),
            serde_json::json!({"level":"alarm"}),
            serde_json::json!("urgent"),
            serde_json::json!({"level":true}),
        ] {
            assert_eq!(
                notify_projection_level(&serde_json::json!({"awiki.notify.v1":value})),
                Some("invalid")
            );
            assert_eq!(
                notify_level_from_annotations(&serde_json::json!({"awiki.notify.v1":value})),
                None
            );
        }
        assert_eq!(
            notify_level_from_annotations(
                &serde_json::json!({"awiki.notify.v1":{"level":"normal"}})
            ),
            Some(NotifyLevel::Normal)
        );
    }
}
