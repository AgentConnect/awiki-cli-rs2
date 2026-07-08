use serde_json::{json, Map, Value};

use super::common::{self, WireIdentity};

pub(crate) const SYNC_PROFILE: &str = "anp.sync.local.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SyncDeltaWireRequest {
    pub(crate) since_event_seq: String,
    pub(crate) limit: u32,
    pub(crate) device_id: Option<String>,
    pub(crate) reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SyncDeltaPage {
    pub(crate) events: Vec<SyncDeltaEvent>,
    pub(crate) next_event_seq: String,
    pub(crate) has_more: bool,
    pub(crate) snapshot_required: bool,
    pub(crate) retention_floor_event_seq: Option<String>,
    pub(crate) warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SyncDeltaEvent {
    pub(crate) event_id: String,
    pub(crate) event_seq: String,
    pub(crate) event_type: String,
    pub(crate) aggregate_kind: Option<String>,
    pub(crate) aggregate_id: Option<String>,
    pub(crate) owner_subject_id: Option<String>,
    pub(crate) created_at: Option<String>,
    pub(crate) payload: Value,
}

pub(crate) fn build_sync_delta_rpc_params(
    identity: &WireIdentity,
    request: SyncDeltaWireRequest,
) -> crate::ImResult<Value> {
    let user_did = required_string("user_did", Some(identity.did.as_str()))?;
    let since_event_seq =
        crate::internal::local_state::sync_state::normalize_decimal_seq(&request.since_event_seq)
            .map_err(|_| invalid_decimal("since_event_seq", &request.since_event_seq))?;
    let limit = validate_limit(request.limit)?;
    let mut body = Map::new();
    body.insert("user_did".to_owned(), Value::String(user_did.clone()));
    body.insert("since_event_seq".to_owned(), Value::String(since_event_seq));
    body.insert("limit".to_owned(), json!(limit));
    insert_optional_string(&mut body, "device_id", request.device_id.as_deref());
    insert_optional_string(&mut body, "reason", request.reason.as_deref());

    Ok(json!({
        "meta": common::local_meta(&user_did, SYNC_PROFILE),
        "body": body,
    }))
}

pub(crate) fn parse_sync_delta_page(raw: &Value) -> crate::ImResult<SyncDeltaPage> {
    let object = raw
        .as_object()
        .ok_or_else(|| invalid_page("response must be an object"))?;
    let events = object
        .get("events")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_page("events must be an array"))?
        .iter()
        .map(parse_sync_delta_event)
        .collect::<crate::ImResult<Vec<_>>>()?;
    let next_event_seq = decimal_string_field(object, "next_event_seq")?;
    let retention_floor_event_seq =
        optional_decimal_string_field(object, "retention_floor_event_seq")?;
    let has_more = object
        .get("has_more")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let snapshot_required = object
        .get("snapshot_required")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Ok(SyncDeltaPage {
        events,
        next_event_seq,
        has_more,
        snapshot_required,
        retention_floor_event_seq,
        warnings: warnings_from_value(object.get("warnings")),
    })
}

pub(crate) fn validate_limit(limit: u32) -> crate::ImResult<u32> {
    if limit == 0 {
        return Err(crate::ImError::invalid_input(
            Some("limit".to_owned()),
            "sync.delta limit must be greater than zero",
        ));
    }
    if limit > 500 {
        return Err(crate::ImError::invalid_input(
            Some("limit".to_owned()),
            "sync.delta limit must be less than or equal to 500",
        ));
    }
    Ok(limit)
}

fn parse_sync_delta_event(value: &Value) -> crate::ImResult<SyncDeltaEvent> {
    let object = value
        .as_object()
        .ok_or_else(|| invalid_page("event must be an object"))?;
    let event_id = required_string("event_id", string_field(object, "event_id").as_deref())?;
    let event_seq = decimal_string_field(object, "event_seq")?;
    let event_type = required_string("event_type", string_field(object, "event_type").as_deref())?;
    let payload = object.get("payload").cloned().unwrap_or(Value::Null);
    if !payload.is_object() {
        return Err(invalid_page("event payload must be an object"));
    }
    Ok(SyncDeltaEvent {
        event_id,
        event_seq,
        event_type,
        aggregate_kind: string_field(object, "aggregate_kind"),
        aggregate_id: string_field(object, "aggregate_id"),
        owner_subject_id: string_field(object, "owner_subject_id"),
        created_at: string_field(object, "created_at"),
        payload,
    })
}

fn decimal_string_field(
    object: &serde_json::Map<String, Value>,
    field: &'static str,
) -> crate::ImResult<String> {
    let value = string_or_u64(object.get(field))
        .ok_or_else(|| invalid_page(format!("{field} must be a decimal string")))?;
    crate::internal::local_state::sync_state::normalize_decimal_seq(&value)
        .map_err(|_| invalid_decimal(field, &value))
}

fn optional_decimal_string_field(
    object: &serde_json::Map<String, Value>,
    field: &'static str,
) -> crate::ImResult<Option<String>> {
    let Some(value) = string_or_u64(object.get(field)) else {
        return Ok(None);
    };
    if value.trim().is_empty() {
        return Ok(None);
    }
    crate::internal::local_state::sync_state::normalize_decimal_seq(&value)
        .map(Some)
        .map_err(|_| invalid_decimal(field, &value))
}

fn string_or_u64(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(value)) => Some(value.clone()),
        Some(Value::Number(number)) => number.as_u64().map(|value| value.to_string()),
        _ => None,
    }
}

fn string_field(object: &serde_json::Map<String, Value>, field: &str) -> Option<String> {
    object
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn warnings_from_value(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn insert_optional_string(object: &mut Map<String, Value>, key: &str, value: Option<&str>) {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    object.insert(key.to_owned(), Value::String(value.to_owned()));
}

fn required_string(field: &'static str, value: Option<&str>) -> crate::ImResult<String> {
    let value = value.unwrap_or_default().trim();
    if value.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some(field.to_owned()),
            format!("{field} is required"),
        ));
    }
    Ok(value.to_owned())
}

fn invalid_decimal(field: &'static str, value: &str) -> crate::ImError {
    crate::ImError::invalid_input(
        Some(field.to_owned()),
        format!("{field} must be a non-negative decimal string: {value:?}"),
    )
}

fn invalid_page(message: impl Into<String>) -> crate::ImError {
    crate::ImError::Service {
        status_code: None,
        code: Some("sync.invalid_page".to_owned()),
        message: message.into(),
        data: None,
    }
}
