use std::collections::VecDeque;

use serde_json::{Map, Value};

pub const LISTENER_WS_NOTIFICATION_QUEUE_CAPACITY: usize = 128;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct InlineSyncEventV3 {
    pub(crate) account_scan_seq_hint: Option<String>,
    pub(crate) event: crate::internal::wire::sync_v2::SyncEventV2,
    pub(crate) projection: Value,
}

pub(crate) fn parse_inline_sync_event_v3(
    notification: &Value,
) -> crate::ImResult<Option<InlineSyncEventV3>> {
    let Some(sync) = notification.get("sync").and_then(Value::as_object) else {
        return Ok(None);
    };
    if sync.get("schema_version").and_then(Value::as_u64) != Some(3) {
        return Ok(None);
    }
    if notification.get("method").and_then(Value::as_str) != Some("sync.changed") {
        return Err(invalid_inline_event(
            "schema 3 sync notification must use sync.changed",
        ));
    }
    let Some(params) = notification.get("params").and_then(Value::as_object) else {
        return Err(invalid_inline_event(
            "schema 3 sync notification params must be an object",
        ));
    };
    if !has_exact_keys(params, &["domains", "reason"])
        || params.get("domains") != Some(&Value::Array(vec![Value::String("message".to_owned())]))
        || !params
            .get("reason")
            .and_then(Value::as_str)
            .is_some_and(valid_reason)
    {
        return Err(invalid_inline_event(
            "schema 3 sync notification params are invalid",
        ));
    }
    let expected_sync_fields = if sync.contains_key("account_scan_seq_hint") {
        &[
            "schema_version",
            "account_scan_seq_hint",
            "domain_versions",
            "event",
            "projection",
        ][..]
    } else {
        &["schema_version", "domain_versions", "event", "projection"][..]
    };
    if !has_exact_keys(sync, expected_sync_fields) {
        return Err(invalid_inline_event(
            "schema 3 sync payload contains unexpected fields",
        ));
    }
    let account_scan_seq_hint = sync
        .get("account_scan_seq_hint")
        .map(|value| canonical_decimal(value, "account_scan_seq_hint"))
        .transpose()?;
    let Some(domain_versions) = sync.get("domain_versions").and_then(Value::as_object) else {
        return Err(invalid_inline_event(
            "schema 3 domain_versions must be an object",
        ));
    };
    for (domain, version) in domain_versions {
        if !matches!(
            domain.as_str(),
            "message" | "profile" | "agent_inventory" | "agent_status" | "device_registry"
        ) {
            return Err(invalid_inline_event(
                "schema 3 domain_versions contains an unknown domain",
            ));
        }
        canonical_decimal(version, "domain_versions value")?;
    }
    let event = crate::internal::wire::sync_v2::parse_inline_event(
        sync.get("event")
            .ok_or_else(|| invalid_inline_event("schema 3 event is required"))?,
    )?;
    if event.event_type != "message.created"
        || event.schema_version != 1
        || event.ignore_safe
        || !matches!(
            event.aggregate_kind.as_str(),
            "direct_message" | "group_message"
        )
    {
        return Err(invalid_inline_event(
            "schema 3 event is not an ordinary message.created event",
        ));
    }
    if account_scan_seq_hint.as_deref().is_some_and(|hint| {
        crate::internal::local_state::sync_v2::compare_decimal(&event.event_seq, hint)
            .map(|order| order == std::cmp::Ordering::Greater)
            .unwrap_or(true)
    }) {
        return Err(invalid_inline_event(
            "schema 3 event sequence is ahead of its account scan hint",
        ));
    }
    let projection = crate::internal::wire::sync_v2::parse_inline_message(
        &event.event_id,
        sync.get("projection")
            .ok_or_else(|| invalid_inline_event("schema 3 projection is required"))?,
    )?;
    let expected_thread_kind = if event.aggregate_kind == "group_message" {
        "group"
    } else {
        "direct"
    };
    if projection.get("thread_kind").and_then(Value::as_str) != Some(expected_thread_kind) {
        return Err(invalid_inline_event(
            "schema 3 projection conflicts with the event aggregate kind",
        ));
    }
    Ok(Some(InlineSyncEventV3 {
        account_scan_seq_hint,
        event,
        projection,
    }))
}

fn has_exact_keys(object: &Map<String, Value>, expected: &[&str]) -> bool {
    object.len() == expected.len() && expected.iter().all(|field| object.contains_key(*field))
}

fn valid_reason(reason: &str) -> bool {
    !reason.is_empty() && reason.len() <= 128 && !reason.chars().any(char::is_control)
}

fn canonical_decimal(value: &Value, field: &str) -> crate::ImResult<String> {
    let value = value
        .as_str()
        .ok_or_else(|| invalid_inline_event(format!("{field} must be a decimal string")))?;
    crate::internal::local_state::sync_v2::validate_decimal(field, value)
        .map_err(|_| invalid_inline_event(format!("{field} must be canonical")))?;
    Ok(value.to_owned())
}

fn invalid_inline_event(detail: impl Into<String>) -> crate::ImError {
    crate::ImError::Serialization {
        detail: detail.into(),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ListenerWsNotificationQueue {
    notifications: VecDeque<Map<String, Value>>,
    capacity: usize,
}

impl ListenerWsNotificationQueue {
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self {
            notifications: VecDeque::new(),
            capacity,
        }
    }

    pub(crate) fn push(&mut self, notification: Map<String, Value>) -> bool {
        if self.notifications.len() >= self.capacity {
            return false;
        }
        self.notifications.push_back(notification);
        true
    }

    pub(crate) fn pop(&mut self) -> Option<Map<String, Value>> {
        self.notifications.pop_front()
    }

    pub(crate) fn len(&self) -> usize {
        self.notifications.len()
    }
}

#[cfg(test)]
#[path = "notification_tests.rs"]
mod tests;
