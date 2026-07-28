use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Map, Value};

use super::common::{self, WireIdentity};

pub(crate) const SYNC_V2_PROFILE: &str = "anp.sync.local.v2";
pub(crate) const MESSAGE_GET_BATCH_MAX_EVENT_IDS: usize = 100;
pub(crate) const MESSAGE_GET_BATCH_CLIENT_CHUNK_EVENT_IDS: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SyncCursorV2 {
    pub(crate) stream_epoch: String,
    pub(crate) scan_seq: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SyncBootstrapV2 {
    pub(crate) account_id: String,
    pub(crate) device_id: String,
    pub(crate) server_time: String,
    pub(crate) cursor: SyncCursorV2,
    pub(crate) group_state_baseline: Vec<Value>,
    pub(crate) warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SyncDeltaPageV2 {
    pub(crate) server_time: String,
    pub(crate) events: Vec<SyncEventV2>,
    pub(crate) next_cursor: SyncCursorV2,
    pub(crate) has_more: bool,
    pub(crate) warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SyncEventV2 {
    pub(crate) event_id: String,
    pub(crate) stream_epoch: String,
    pub(crate) event_seq: String,
    pub(crate) event_type: String,
    pub(crate) schema_version: u32,
    pub(crate) ignore_safe: bool,
    pub(crate) account_id: String,
    pub(crate) recipient_device_id: Option<String>,
    pub(crate) origin_did: Option<String>,
    pub(crate) origin_device_id: Option<String>,
    pub(crate) aggregate_kind: String,
    pub(crate) aggregate_id: String,
    pub(crate) state_version: Option<String>,
    pub(crate) thread_key: Option<String>,
    pub(crate) occurred_at: String,
    pub(crate) payload: Value,
    pub(crate) source: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HydratedMessageV2 {
    pub(crate) event_id: String,
    pub(crate) message: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MessageBatchV2 {
    pub(crate) items: Vec<HydratedMessageV2>,
    pub(crate) unavailable: Vec<String>,
}

pub(crate) fn build_bootstrap_params(
    identity: &WireIdentity,
    client_instance_id: &str,
) -> crate::ImResult<Value> {
    let did = required_string("identity.did", identity.did.as_str())?;
    let client_instance_id = required_string("client_instance_id", client_instance_id)?;
    Ok(json!({
        "meta": common::local_meta(&did, SYNC_V2_PROFILE),
        "body": {
            "client_instance_id": client_instance_id,
            "capabilities": {
                "sync_profile": SYNC_V2_PROFILE,
                "event_schema_max": 1
            }
        }
    }))
}

pub(crate) fn build_delta_params(
    identity: &WireIdentity,
    cursor: &SyncCursorV2,
    limit: u32,
    reason: &str,
) -> crate::ImResult<Value> {
    let did = required_string("identity.did", identity.did.as_str())?;
    validate_cursor(cursor)?;
    let limit = validate_limit(limit)?;
    let reason = validate_reason(reason)?;
    Ok(json!({
        "meta": common::local_meta(&did, SYNC_V2_PROFILE),
        "body": {
            "cursor": {
                "stream_epoch": cursor.stream_epoch,
                "scan_seq": cursor.scan_seq
            },
            "limit": limit,
            "reason": reason
        }
    }))
}

pub(crate) fn build_message_get_batch_params(
    identity: &WireIdentity,
    event_ids: &[String],
) -> crate::ImResult<Value> {
    let did = required_string("identity.did", identity.did.as_str())?;
    validate_event_ids(event_ids)?;
    Ok(json!({
        "meta": common::local_meta(&did, SYNC_V2_PROFILE),
        "body": {
            "event_ids": event_ids
        }
    }))
}

pub(crate) fn parse_bootstrap(raw: &Value) -> crate::ImResult<SyncBootstrapV2> {
    let object = object(raw, "sync.bootstrap response")?;
    exact_mode(object, "tail_only")?;
    let read_state_baseline = object
        .get("read_state_baseline")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_page("read_state_baseline must be an array"))?;
    if !read_state_baseline.is_empty() {
        return Err(invalid_page(
            "Stage 2 sync.bootstrap read_state_baseline must be empty",
        ));
    }
    let groups = object
        .get("group_state_baseline")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_page("group_state_baseline must be an array"))?
        .clone();
    for group in &groups {
        if !group.is_object() {
            return Err(invalid_page("group_state_baseline items must be objects"));
        }
        reject_e2ee_value(group)?;
    }
    Ok(SyncBootstrapV2 {
        account_id: canonical_string_field(object, "account_id")?,
        device_id: canonical_string_field(object, "device_id")?,
        server_time: canonical_string_field(object, "server_time")?,
        cursor: parse_cursor(
            object
                .get("cursor")
                .ok_or_else(|| invalid_page("cursor is required"))?,
        )?,
        group_state_baseline: groups,
        warnings: warnings(object.get("warnings"))?,
    })
}

pub(crate) fn parse_delta(raw: &Value) -> crate::ImResult<SyncDeltaPageV2> {
    let object = object(raw, "sync.delta response")?;
    exact_mode(object, "delta")?;
    if !matches!(object.get("recovery"), None | Some(Value::Null)) {
        return Err(invalid_page(
            "Stage 2 sync.delta must not return an inline recovery secret",
        ));
    }
    let events = object
        .get("events")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_page("events must be an array"))?
        .iter()
        .map(parse_event)
        .collect::<crate::ImResult<Vec<_>>>()?;
    let mut event_ids = BTreeSet::new();
    let mut event_seqs = BTreeSet::new();
    for event in &events {
        if !event_ids.insert(event.event_id.as_str()) {
            return Err(invalid_page("sync.delta contains a duplicate event_id"));
        }
        if !event_seqs.insert(event.event_seq.as_str()) {
            return Err(invalid_page("sync.delta contains a duplicate event_seq"));
        }
    }
    Ok(SyncDeltaPageV2 {
        server_time: canonical_string_field(object, "server_time")?,
        events,
        next_cursor: parse_cursor(
            object
                .get("next_cursor")
                .ok_or_else(|| invalid_page("next_cursor is required"))?,
        )?,
        has_more: object
            .get("has_more")
            .and_then(Value::as_bool)
            .ok_or_else(|| invalid_page("has_more must be a boolean"))?,
        warnings: warnings(object.get("warnings"))?,
    })
}

pub(crate) fn parse_message_batch(
    raw: &Value,
    requested_event_ids: &[String],
) -> crate::ImResult<MessageBatchV2> {
    validate_event_ids(requested_event_ids)?;
    let request_order = requested_event_ids
        .iter()
        .enumerate()
        .map(|(index, event_id)| (event_id.as_str(), index))
        .collect::<BTreeMap<_, _>>();
    let response = object(raw, "message.get_batch response")?;
    let items = response
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_page("items must be an array"))?
        .iter()
        .map(|item| {
            let item = object(item, "message.get_batch item")?;
            let event_id = canonical_string_field(item, "event_id")?;
            let message = item
                .get("message")
                .filter(|message| message.is_object())
                .cloned()
                .ok_or_else(|| invalid_page("hydrated message must be an object"))?;
            reject_e2ee_value(&message)?;
            Ok(HydratedMessageV2 { event_id, message })
        })
        .collect::<crate::ImResult<Vec<_>>>()?;
    let unavailable = response
        .get("unavailable")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_page("unavailable must be an array"))?
        .iter()
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| invalid_page("unavailable entries must be strings"))
                .and_then(|value| required_string("unavailable.event_id", value))
        })
        .collect::<crate::ImResult<Vec<_>>>()?;

    let mut seen = BTreeSet::new();
    validate_response_order(
        items.iter().map(|item| item.event_id.as_str()),
        &request_order,
        "items",
        &mut seen,
    )?;
    validate_response_order(
        unavailable.iter().map(String::as_str),
        &request_order,
        "unavailable",
        &mut seen,
    )?;
    if seen.len() != requested_event_ids.len() {
        return Err(invalid_page(
            "message.get_batch response does not cover every requested event_id",
        ));
    }
    Ok(MessageBatchV2 { items, unavailable })
}

fn parse_event(value: &Value) -> crate::ImResult<SyncEventV2> {
    let object = object(value, "sync event")?;
    let payload = object
        .get("payload")
        .filter(|payload| payload.is_object())
        .cloned()
        .ok_or_else(|| invalid_page("event payload must be an object"))?;
    let schema_version = object
        .get("schema_version")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| invalid_page("schema_version must be a positive integer"))?;
    if schema_version == 0 {
        return Err(invalid_page("schema_version must be a positive integer"));
    }
    let state_version = optional_decimal_field(object, "state_version")?;
    let source = match object.get("source") {
        None | Some(Value::Null) => None,
        Some(value) if value.is_object() => Some(value.clone()),
        Some(_) => return Err(invalid_page("source must be an object or null")),
    };
    Ok(SyncEventV2 {
        event_id: canonical_string_field(object, "event_id")?,
        stream_epoch: positive_decimal_field(object, "stream_epoch")?,
        event_seq: positive_decimal_field(object, "event_seq")?,
        event_type: canonical_string_field(object, "event_type")?,
        schema_version,
        ignore_safe: object
            .get("ignore_safe")
            .and_then(Value::as_bool)
            .ok_or_else(|| invalid_page("ignore_safe must be a boolean"))?,
        account_id: canonical_string_field(object, "account_id")?,
        recipient_device_id: optional_canonical_string_field(object, "recipient_device_id")?,
        origin_did: optional_canonical_string_field(object, "origin_did")?,
        origin_device_id: optional_canonical_string_field(object, "origin_device_id")?,
        aggregate_kind: canonical_string_field(object, "aggregate_kind")?,
        aggregate_id: canonical_string_field(object, "aggregate_id")?,
        state_version,
        thread_key: optional_canonical_string_field(object, "thread_key")?,
        occurred_at: canonical_string_field(object, "occurred_at")?,
        payload,
        source,
    })
}

fn parse_cursor(value: &Value) -> crate::ImResult<SyncCursorV2> {
    let object = object(value, "cursor")?;
    let cursor = SyncCursorV2 {
        stream_epoch: positive_decimal_field(object, "stream_epoch")?,
        scan_seq: decimal_field(object, "scan_seq")?,
    };
    validate_cursor(&cursor)?;
    Ok(cursor)
}

fn validate_cursor(cursor: &SyncCursorV2) -> crate::ImResult<()> {
    crate::internal::local_state::sync_v2::validate_positive_decimal(
        "stream_epoch",
        &cursor.stream_epoch,
    )?;
    crate::internal::local_state::sync_v2::validate_decimal("scan_seq", &cursor.scan_seq)
}

fn validate_event_ids(event_ids: &[String]) -> crate::ImResult<()> {
    if event_ids.is_empty() || event_ids.len() > MESSAGE_GET_BATCH_MAX_EVENT_IDS {
        return Err(crate::ImError::invalid_input(
            Some("event_ids".to_owned()),
            format!(
                "event_ids must contain between 1 and {MESSAGE_GET_BATCH_MAX_EVENT_IDS} entries"
            ),
        ));
    }
    let mut unique = BTreeSet::new();
    for event_id in event_ids {
        required_string("event_ids", event_id)?;
        if !unique.insert(event_id.as_str()) {
            return Err(crate::ImError::invalid_input(
                Some("event_ids".to_owned()),
                "event_ids must not contain duplicates",
            ));
        }
    }
    Ok(())
}

fn validate_response_order<'a>(
    event_ids: impl Iterator<Item = &'a str>,
    request_order: &BTreeMap<&str, usize>,
    field: &str,
    seen: &mut BTreeSet<String>,
) -> crate::ImResult<()> {
    let mut previous = None;
    for event_id in event_ids {
        let Some(index) = request_order.get(event_id).copied() else {
            return Err(invalid_page(format!(
                "{field} contains an event_id that was not requested"
            )));
        };
        if previous.is_some_and(|previous| index <= previous) {
            return Err(invalid_page(format!(
                "{field} does not preserve request order"
            )));
        }
        if !seen.insert(event_id.to_owned()) {
            return Err(invalid_page(
                "message.get_batch repeats an event_id across response fields",
            ));
        }
        previous = Some(index);
    }
    Ok(())
}

fn reject_e2ee_value(value: &Value) -> crate::ImResult<()> {
    let object = value
        .as_object()
        .ok_or_else(|| invalid_page("ordinary sync v2 value must be an object"))?;
    reject_e2ee_discriminators(object)?;
    if let Some(metadata) = object.get("metadata").and_then(Value::as_object) {
        reject_e2ee_discriminators(metadata)?;
    }
    Ok(())
}

fn reject_e2ee_discriminators(object: &Map<String, Value>) -> crate::ImResult<()> {
    let forbidden = [
        "anp.direct.e2ee",
        "anp.group.e2ee",
        "group.e2ee.",
        "e2ee",
        "mls",
        "secure_direct",
    ];
    let has_forbidden_discriminator = ["profile", "security_profile", "thread_kind"]
        .into_iter()
        .filter_map(|field| object.get(field).and_then(Value::as_str))
        .map(str::to_ascii_lowercase)
        .any(|value| forbidden.iter().any(|token| value.contains(token)));
    if has_forbidden_discriminator
        || object
            .get("secure")
            .and_then(Value::as_bool)
            .is_some_and(|secure| secure)
    {
        return Err(invalid_page(
            "ordinary sync v2 response contains an E2EE/MLS value",
        ));
    }
    Ok(())
}

fn validate_reason(value: &str) -> crate::ImResult<String> {
    let value = required_string("reason", value)?;
    if matches!(
        value.as_str(),
        "session_start"
            | "app_resume"
            | "websocket_hint"
            | "websocket_reconnect"
            | "foreground_reconcile"
            | "manual_refresh"
            | "after_mutation"
    ) {
        Ok(value)
    } else {
        Err(crate::ImError::invalid_input(
            Some("reason".to_owned()),
            "unsupported message sync reason",
        ))
    }
}

fn validate_limit(limit: u32) -> crate::ImResult<u32> {
    if (1..=500).contains(&limit) {
        Ok(limit)
    } else {
        Err(crate::ImError::invalid_input(
            Some("limit".to_owned()),
            "message sync limit must be between 1 and 500",
        ))
    }
}

fn exact_mode(object: &Map<String, Value>, expected: &str) -> crate::ImResult<()> {
    let mode = canonical_string_field(object, "mode")?;
    if mode == expected {
        Ok(())
    } else {
        Err(invalid_page(format!(
            "response mode must be {expected:?}, got {mode:?}"
        )))
    }
}

fn warnings(value: Option<&Value>) -> crate::ImResult<Vec<String>> {
    let Some(values) = value.and_then(Value::as_array) else {
        return Err(invalid_page("warnings must be an array"));
    };
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| invalid_page("warning entries must be strings"))
                .and_then(|value| required_string("warning", value))
        })
        .collect()
}

fn object<'a>(value: &'a Value, label: &str) -> crate::ImResult<&'a Map<String, Value>> {
    value
        .as_object()
        .ok_or_else(|| invalid_page(format!("{label} must be an object")))
}

fn canonical_string_field(
    object: &Map<String, Value>,
    field: &'static str,
) -> crate::ImResult<String> {
    object
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_page(format!("{field} must be a string")))
        .and_then(|value| required_string(field, value))
}

fn optional_canonical_string_field(
    object: &Map<String, Value>,
    field: &'static str,
) -> crate::ImResult<Option<String>> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => required_string(field, value).map(Some),
        Some(_) => Err(invalid_page(format!("{field} must be a string or null"))),
    }
}

fn decimal_field(object: &Map<String, Value>, field: &'static str) -> crate::ImResult<String> {
    let value = canonical_string_field(object, field)?;
    crate::internal::local_state::sync_v2::validate_decimal(field, &value)
        .map_err(|_| invalid_page(format!("{field} must be a canonical decimal string")))?;
    Ok(value)
}

fn positive_decimal_field(
    object: &Map<String, Value>,
    field: &'static str,
) -> crate::ImResult<String> {
    let value = canonical_string_field(object, field)?;
    crate::internal::local_state::sync_v2::validate_positive_decimal(field, &value).map_err(
        |_| {
            invalid_page(format!(
                "{field} must be a canonical positive decimal string"
            ))
        },
    )?;
    Ok(value)
}

fn optional_decimal_field(
    object: &Map<String, Value>,
    field: &'static str,
) -> crate::ImResult<Option<String>> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => {
            crate::internal::local_state::sync_v2::validate_decimal(field, value).map_err(
                |_| {
                    invalid_page(format!(
                        "{field} must be a canonical decimal string or null"
                    ))
                },
            )?;
            Ok(Some(value.clone()))
        }
        Some(_) => Err(invalid_page(format!(
            "{field} must be a decimal string or null"
        ))),
    }
}

fn required_string(field: &'static str, value: &str) -> crate::ImResult<String> {
    if value.trim().is_empty() || value.trim() != value {
        return Err(crate::ImError::invalid_input(
            Some(field.to_owned()),
            format!("{field} must be a non-empty canonical string"),
        ));
    }
    Ok(value.to_owned())
}

fn invalid_page(message: impl Into<String>) -> crate::ImError {
    crate::ImError::Service {
        status_code: None,
        code: Some("SYNC_INVALID_PAGE".to_owned()),
        message: message.into(),
        data: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(event_id: &str, event_seq: &str) -> Value {
        json!({
            "event_id": event_id,
            "stream_epoch": "1",
            "event_seq": event_seq,
            "event_type": "message.created",
            "schema_version": 1,
            "ignore_safe": false,
            "account_id": "account-1",
            "recipient_device_id": null,
            "origin_did": "did:example:alice",
            "origin_device_id": "device-1",
            "aggregate_kind": "direct_message",
            "aggregate_id": "message-1",
            "state_version": null,
            "thread_key": "conversation-1",
            "occurred_at": "2026-07-28T10:00:00Z",
            "payload": {},
            "source": {}
        })
    }

    #[test]
    fn frozen_delta_fixture_decodes_decimal_strings_and_sparse_scan() {
        let raw = json!({
            "mode": "delta",
            "server_time": "2026-07-28T10:00:00Z",
            "events": [event("event-1", "100"), event("event-2", "102")],
            "next_cursor": {"stream_epoch": "1", "scan_seq": "105"},
            "has_more": false,
            "recovery": null,
            "warnings": []
        });
        let page = parse_delta(&raw).unwrap();
        assert_eq!(page.events[1].event_seq, "102");
        assert_eq!(page.next_cursor.scan_seq, "105");
    }

    #[test]
    fn unknown_required_fields_remain_available_to_the_reducer() {
        let mut raw = event("event-1", "1");
        raw["event_type"] = json!("future.required");
        raw["ignore_safe"] = json!(false);
        raw["schema_version"] = json!(2);
        let page = parse_delta(&json!({
            "mode": "delta",
            "server_time": "2026-07-28T10:00:00Z",
            "events": [raw],
            "next_cursor": {"stream_epoch": "1", "scan_seq": "1"},
            "has_more": false,
            "recovery": null,
            "warnings": []
        }))
        .unwrap();
        assert!(!page.events[0].ignore_safe);
        assert_eq!(page.events[0].schema_version, 2);
    }

    #[test]
    fn message_batch_rejects_duplicates_unavailable_and_order_drift() {
        let request = vec!["event-1".to_owned(), "event-2".to_owned()];
        assert!(parse_message_batch(
            &json!({
                "items": [
                    {"event_id": "event-2", "message": {}},
                    {"event_id": "event-1", "message": {}}
                ],
                "unavailable": []
            }),
            &request
        )
        .is_err());
        assert!(build_message_get_batch_params(
            &WireIdentity {
                did: "did:example:alice".to_owned()
            },
            &["event-1".to_owned(), "event-1".to_owned()]
        )
        .is_err());
    }

    #[test]
    fn ordinary_content_cannot_trigger_e2ee_discriminator_rejection() {
        let request = vec!["event-1".to_owned()];
        assert!(parse_message_batch(
            &json!({
                "items": [{
                    "event_id": "event-1",
                    "message": {
                        "thread_kind": "direct",
                        "content": "anp.direct.e2ee and mls are ordinary user text"
                    }
                }],
                "unavailable": []
            }),
            &request
        )
        .is_ok());
        assert!(parse_message_batch(
            &json!({
                "items": [{
                    "event_id": "event-1",
                    "message": {
                        "thread_kind": "direct",
                        "security_profile": "anp.direct.e2ee.v1"
                    }
                }],
                "unavailable": []
            }),
            &request
        )
        .is_err());
    }

    #[test]
    fn bootstrap_rejects_read_state_and_e2ee_baselines_in_stage_two() {
        assert!(parse_bootstrap(&json!({
            "mode": "tail_only",
            "account_id": "account-1",
            "device_id": "device-1",
            "server_time": "2026-07-28T10:00:00Z",
            "cursor": {"stream_epoch": "1", "scan_seq": "10"},
            "read_state_baseline": [{"thread_key": "direct-1"}],
            "group_state_baseline": [],
            "warnings": []
        }))
        .is_err());
    }
}
