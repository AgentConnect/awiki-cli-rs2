use serde_json::{json, Map, Value};

#[cfg(feature = "sqlite")]
pub(crate) const GROUP_SYSTEM_EVENT_SCHEMA: &str = "awiki.group.system_event.v1";

#[cfg(feature = "sqlite")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GroupSystemEventInput {
    pub(crate) event_type: String,
    pub(crate) group_did: String,
    pub(crate) group_event_seq: i64,
    pub(crate) group_state_version: Option<String>,
    pub(crate) actor_did: Option<String>,
    pub(crate) subject_did: Option<String>,
    pub(crate) membership_status: Option<String>,
    pub(crate) changed_at: Option<String>,
    pub(crate) sync_event_id: Option<String>,
    pub(crate) sync_event_seq: Option<String>,
    pub(crate) sync_event_type: Option<String>,
    pub(crate) source: String,
}

#[cfg(feature = "sqlite")]
pub(crate) fn record_from_input(
    client: &crate::core::ImClient,
    input: GroupSystemEventInput,
) -> Option<crate::internal::local_state::messages::MessageRecord> {
    let group_did = input.group_did.trim();
    if group_did.is_empty() || input.group_event_seq < 0 {
        return None;
    }
    let event_type = normalize_event_type(&input.event_type, input.membership_status.as_deref());
    if event_type.is_empty() {
        return None;
    }
    let actor_did =
        trim_optional(input.actor_did).unwrap_or_else(|| client.did().as_str().to_owned());
    let subject_did = trim_optional(input.subject_did);
    let changed_at = trim_optional(input.changed_at).unwrap_or_else(now_utc_like);
    let conversation_id =
        crate::internal::local_state::owner_scope::group_conversation_id(group_did);
    let content = payload_json(&GroupSystemEventPayload {
        event_type: event_type.clone(),
        group_did: group_did.to_owned(),
        group_event_seq: input.group_event_seq,
        group_state_version: trim_optional(input.group_state_version),
        actor_did: Some(actor_did.clone()).filter(|value| !value.trim().is_empty()),
        subject_did,
        membership_status: trim_optional(input.membership_status),
        changed_at: changed_at.clone(),
        sync_event_id: trim_optional(input.sync_event_id),
        sync_event_seq: trim_optional(input.sync_event_seq),
        sync_event_type: trim_optional(input.sync_event_type),
        source: input.source,
    });
    Some(crate::internal::local_state::messages::MessageRecord {
        msg_id: format!("{group_did}:{}", input.group_event_seq),
        owner_identity_id: client.current_identity().id.as_str().to_owned(),
        owner_did: client.did().as_str().to_owned(),
        conversation_id: conversation_id.clone(),
        thread_id: conversation_id,
        direction: if actor_did.trim() == client.did().as_str() {
            1
        } else {
            0
        },
        sender_did: actor_did,
        group_id: group_did.to_owned(),
        group_did: group_did.to_owned(),
        content_type: "application/json".to_owned(),
        content,
        server_seq: Some(input.group_event_seq),
        sent_at: changed_at.clone(),
        stored_at: changed_at,
        is_read: true,
        metadata: metadata_json(input.group_event_seq),
        credential_name: client.current_identity().id.as_str().to_owned(),
        ..crate::internal::local_state::messages::MessageRecord::default()
    })
}

#[cfg(feature = "sqlite")]
pub(crate) fn record_from_group_read_result(
    client: &crate::core::ImClient,
    group: &str,
    result: &crate::groups::GroupReadResult,
) -> Option<crate::internal::local_state::messages::MessageRecord> {
    let raw = result.raw_response()?.as_object()?;
    let group_did = string_like_from_object(Some(raw), "group_did")
        .or_else(|| non_empty(group))
        .unwrap_or_default();
    let group_event_seq = i64_from_object(raw, "group_event_seq")?;
    let subject_did = string_like_from_object(Some(raw), "member_did").or_else(|| {
        result
            .resolved_member
            .as_ref()
            .map(|member| member.did.as_str().to_owned())
    });
    let membership_status = string_like_from_object(Some(raw), "membership_status");
    let event_type = event_type_from_membership_status(membership_status.as_deref())
        .unwrap_or_else(|| "member_added".to_owned());
    record_from_input(
        client,
        GroupSystemEventInput {
            event_type,
            group_did,
            group_event_seq,
            group_state_version: string_like_from_object(Some(raw), "group_state_version"),
            actor_did: Some(client.did().as_str().to_owned()),
            subject_did,
            membership_status,
            changed_at: string_like_from_object(Some(raw), "accepted_at")
                .or_else(|| string_like_from_object(Some(raw), "changed_at")),
            sync_event_id: None,
            sync_event_seq: None,
            sync_event_type: None,
            source: "im-core.group_mutation".to_owned(),
        },
    )
}

#[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
pub(crate) fn persist_group_read_result(
    client: &crate::core::ImClient,
    group: &str,
    result: &crate::groups::GroupReadResult,
) -> crate::ImResult<()> {
    let Some(record) = record_from_group_read_result(client, group, result) else {
        return Ok(());
    };
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    crate::internal::local_state::messages::upsert_message(&connection, &record)
}

#[cfg(all(feature = "sqlite", not(any(feature = "blocking", test))))]
pub(crate) fn persist_group_read_result(
    _client: &crate::core::ImClient,
    _group: &str,
    _result: &crate::groups::GroupReadResult,
) -> crate::ImResult<()> {
    Err(crate::ImError::unsupported("group-system-event-projection"))
}

#[cfg(feature = "sqlite")]
pub(crate) async fn persist_group_read_result_async(
    client: &crate::core::ImClient,
    group: &str,
    result: &crate::groups::GroupReadResult,
) -> crate::ImResult<()> {
    let Some(record) = record_from_group_read_result(client, group, result) else {
        return Ok(());
    };
    client
        .core_inner()
        .local_state_db()
        .await?
        .store_messages(vec![record])
        .await
}

#[cfg(not(feature = "sqlite"))]
pub(crate) fn persist_group_read_result(
    _client: &crate::core::ImClient,
    _group: &str,
    _result: &crate::groups::GroupReadResult,
) -> crate::ImResult<()> {
    Ok(())
}

#[cfg(not(feature = "sqlite"))]
pub(crate) async fn persist_group_read_result_async(
    _client: &crate::core::ImClient,
    _group: &str,
    _result: &crate::groups::GroupReadResult,
) -> crate::ImResult<()> {
    Ok(())
}

#[cfg(feature = "sqlite")]
struct GroupSystemEventPayload {
    event_type: String,
    group_did: String,
    group_event_seq: i64,
    group_state_version: Option<String>,
    actor_did: Option<String>,
    subject_did: Option<String>,
    membership_status: Option<String>,
    changed_at: String,
    sync_event_id: Option<String>,
    sync_event_seq: Option<String>,
    sync_event_type: Option<String>,
    source: String,
}

#[cfg(feature = "sqlite")]
fn payload_json(payload: &GroupSystemEventPayload) -> String {
    let mut object = Map::new();
    object.insert(
        "schema".to_owned(),
        Value::String(GROUP_SYSTEM_EVENT_SCHEMA.to_owned()),
    );
    object.insert("type".to_owned(), Value::String(payload.event_type.clone()));
    object.insert(
        "group_did".to_owned(),
        Value::String(payload.group_did.clone()),
    );
    object.insert(
        "group_event_seq".to_owned(),
        Value::String(payload.group_event_seq.to_string()),
    );
    insert_optional(
        &mut object,
        "group_state_version",
        payload.group_state_version.as_deref(),
    );
    insert_optional(&mut object, "actor_did", payload.actor_did.as_deref());
    insert_optional(&mut object, "subject_did", payload.subject_did.as_deref());
    insert_optional(
        &mut object,
        "membership_status",
        payload.membership_status.as_deref(),
    );
    object.insert(
        "changed_at".to_owned(),
        Value::String(payload.changed_at.clone()),
    );
    insert_optional(
        &mut object,
        "sync_event_id",
        payload.sync_event_id.as_deref(),
    );
    insert_optional(
        &mut object,
        "sync_event_seq",
        payload.sync_event_seq.as_deref(),
    );
    insert_optional(
        &mut object,
        "sync_event_type",
        payload.sync_event_type.as_deref(),
    );
    insert_optional(&mut object, "source", Some(payload.source.as_str()));
    Value::Object(object).to_string()
}

#[cfg(feature = "sqlite")]
fn metadata_json(group_event_seq: i64) -> String {
    json!({
        "content_type": "application/json",
        "group_event_seq": group_event_seq.to_string(),
        "is_read": "true",
        "message_role": "group_system_event",
    })
    .to_string()
}

#[cfg(feature = "sqlite")]
fn normalize_event_type(event_type: &str, membership_status: Option<&str>) -> String {
    let event_type = event_type.trim().to_ascii_lowercase().replace('-', "_");
    match event_type.as_str() {
        "member_activated" => return "member_added".to_owned(),
        "member_removed" | "member_left" | "member_added" => return event_type,
        "group_profile_updated" | "profile_updated" => {
            return "group_profile_updated".to_owned();
        }
        _ => {}
    }
    if !event_type.is_empty() && event_type != "unknown" {
        return event_type;
    }
    event_type_from_membership_status(membership_status).unwrap_or_default()
}

#[cfg(feature = "sqlite")]
fn event_type_from_membership_status(status: Option<&str>) -> Option<String> {
    match status.map(str::trim).unwrap_or_default() {
        "active" | "activated" => Some("member_added".to_owned()),
        "removed" => Some("member_removed".to_owned()),
        "left" => Some("member_left".to_owned()),
        _ => None,
    }
}

#[cfg(feature = "sqlite")]
fn insert_optional(object: &mut Map<String, Value>, key: &str, value: Option<&str>) {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    object.insert(key.to_owned(), Value::String(value.to_owned()));
}

#[cfg(feature = "sqlite")]
fn trim_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

#[cfg(feature = "sqlite")]
fn non_empty(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

#[cfg(feature = "sqlite")]
pub(crate) fn string_like_from_object(
    object: Option<&Map<String, Value>>,
    key: &str,
) -> Option<String> {
    object
        .and_then(|object| object.get(key))
        .and_then(string_like_value)
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

#[cfg(feature = "sqlite")]
fn string_like_value(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(number) => {
            if let Some(value) = number.as_i64() {
                Some(value.to_string())
            } else if let Some(value) = number.as_u64() {
                Some(value.to_string())
            } else {
                number.as_f64().map(|value| format!("{value:.0}"))
            }
        }
        _ => None,
    }
}

#[cfg(feature = "sqlite")]
pub(crate) fn i64_from_object(object: &Map<String, Value>, key: &str) -> Option<i64> {
    match object.get(key) {
        Some(Value::Number(number)) => number.as_i64().or_else(|| {
            number
                .as_u64()
                .and_then(|value| i64::try_from(value).ok())
                .or_else(|| number.as_f64().map(|value| value as i64))
        }),
        Some(Value::String(value)) => value.trim().parse::<i64>().ok(),
        _ => None,
    }
}

#[cfg(feature = "sqlite")]
fn now_utc_like() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
}
