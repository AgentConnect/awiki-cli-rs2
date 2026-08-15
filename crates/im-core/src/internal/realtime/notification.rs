use std::collections::VecDeque;

use serde_json::{Map, Value};

pub const LISTENER_WS_NOTIFICATION_QUEUE_CAPACITY: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InlineSyncLaneV3 {
    Ordinary,
    P5Device,
    P6Group,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct InlineSyncEventV3 {
    pub(crate) account_scan_seq_hint: Option<String>,
    pub(crate) lane: InlineSyncLaneV3,
    pub(crate) event_id: String,
    pub(crate) stream_epoch: String,
    pub(crate) event_seq: String,
    pub(crate) event_type: String,
    pub(crate) ordinary_event: Option<crate::internal::wire::sync_v2::SyncEventV2>,
    pub(crate) group_did: Option<String>,
    pub(crate) group_event_seq: Option<String>,
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
    let event_value = sync
        .get("event")
        .ok_or_else(|| invalid_inline_event("schema 3 event is required"))?;
    let event_object = event_value
        .as_object()
        .ok_or_else(|| invalid_inline_event("schema 3 event must be an object"))?;
    let lane = match event_object.get("lane").and_then(Value::as_str) {
        None | Some("ordinary") => InlineSyncLaneV3::Ordinary,
        Some("p5_device") => InlineSyncLaneV3::P5Device,
        Some("p6_group") => InlineSyncLaneV3::P6Group,
        Some(_) => return Err(invalid_inline_event("schema 3 event lane is unsupported")),
    };
    let projection_value = sync
        .get("projection")
        .ok_or_else(|| invalid_inline_event("schema 3 projection is required"))?;
    let (
        event_id,
        stream_epoch,
        event_seq,
        event_type,
        ordinary_event,
        group_did,
        group_event_seq,
        projection,
    ) = match lane {
        InlineSyncLaneV3::Ordinary => {
            let mut ordinary_event_value = event_value.clone();
            ordinary_event_value
                .as_object_mut()
                .expect("schema 3 event object was validated")
                .remove("lane");
            let event = crate::internal::wire::sync_v2::parse_inline_event(&ordinary_event_value)?;
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
                projection_value,
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
            (
                event.event_id.clone(),
                event.stream_epoch.clone(),
                event.event_seq.clone(),
                event.event_type.clone(),
                Some(event),
                None,
                None,
                projection,
            )
        }
        InlineSyncLaneV3::P5Device => {
            if !has_exact_keys(
                event_object,
                &[
                    "lane",
                    "event_id",
                    "stream_epoch",
                    "event_seq",
                    "event_type",
                ],
            ) || event_object.get("event_type").and_then(Value::as_str)
                != Some("p5.delivery.created")
            {
                return Err(invalid_inline_event("schema 3 P5 event shape is invalid"));
            }
            crate::internal::wire::sync_v2::validate_lane_envelope_v3(
                projection_value,
                "anp.direct.e2ee.v2",
                "direct-e2ee",
            )?;
            (
                canonical_string(event_object, "event_id")?,
                canonical_positive_decimal(event_object, "stream_epoch")?,
                canonical_positive_decimal(event_object, "event_seq")?,
                "p5.delivery.created".to_owned(),
                None,
                None,
                None,
                projection_value.clone(),
            )
        }
        InlineSyncLaneV3::P6Group => {
            if !has_exact_keys(
                event_object,
                &[
                    "lane",
                    "event_id",
                    "stream_epoch",
                    "event_seq",
                    "event_type",
                    "group_did",
                    "group_event_seq",
                ],
            ) || event_object.get("event_type").and_then(Value::as_str)
                != Some("p6.delivery.created")
            {
                return Err(invalid_inline_event("schema 3 P6 event shape is invalid"));
            }
            crate::internal::wire::sync_v2::validate_lane_envelope_v3(
                projection_value,
                "anp.group.e2ee.v2",
                "group-e2ee",
            )?;
            let group_did = canonical_string(event_object, "group_did")?;
            let group_event_seq = canonical_positive_decimal(event_object, "group_event_seq")?;
            (
                canonical_string(event_object, "event_id")?,
                canonical_positive_decimal(event_object, "stream_epoch")?,
                canonical_positive_decimal(event_object, "event_seq")?,
                "p6.delivery.created".to_owned(),
                None,
                Some(group_did),
                Some(group_event_seq),
                projection_value.clone(),
            )
        }
    };
    Ok(Some(InlineSyncEventV3 {
        account_scan_seq_hint,
        lane,
        event_id,
        stream_epoch,
        event_seq,
        event_type,
        ordinary_event,
        group_did,
        group_event_seq,
        projection,
    }))
}

fn canonical_string(object: &Map<String, Value>, field: &str) -> crate::ImResult<String> {
    object
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && value.trim() == *value)
        .map(ToOwned::to_owned)
        .ok_or_else(|| invalid_inline_event(format!("schema 3 {field} must be canonical")))
}

fn canonical_positive_decimal(object: &Map<String, Value>, field: &str) -> crate::ImResult<String> {
    let value = canonical_string(object, field)?;
    crate::internal::local_state::sync_v2::validate_positive_decimal(field, &value)
        .map_err(|_| invalid_inline_event(format!("schema 3 {field} must be positive")))?;
    Ok(value)
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
