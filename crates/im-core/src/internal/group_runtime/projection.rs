use serde_json::Value;

#[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
pub(crate) fn project_group_snapshot(
    client: &crate::core::ImClient,
    result: &crate::groups::GroupReadResult,
) {
    let Some(record) = group_record(client, result) else {
        return;
    };
    let Ok(connection) = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    ) else {
        return;
    };
    let _ = crate::internal::local_state::groups::upsert_group(&connection, record);
}

#[cfg(all(feature = "sqlite", not(any(feature = "blocking", test))))]
pub(crate) fn project_group_snapshot(
    _client: &crate::core::ImClient,
    _result: &crate::groups::GroupReadResult,
) {
}

#[cfg(not(feature = "sqlite"))]
pub(crate) fn project_group_snapshot(
    _client: &crate::core::ImClient,
    _result: &crate::groups::GroupReadResult,
) {
}

#[cfg(feature = "sqlite")]
pub(crate) async fn project_group_snapshot_async(
    client: &crate::core::ImClient,
    result: &crate::groups::GroupReadResult,
) -> crate::ImResult<()> {
    let Some(record) = group_record(client, result) else {
        return Ok(());
    };
    client
        .core_inner()
        .local_state_db()
        .await?
        .upsert_group(record)
        .await
}

#[cfg(not(feature = "sqlite"))]
pub(crate) async fn project_group_snapshot_async(
    _client: &crate::core::ImClient,
    _result: &crate::groups::GroupReadResult,
) -> crate::ImResult<()> {
    Ok(())
}

#[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
pub(crate) fn project_group_summaries(
    client: &crate::core::ImClient,
    result: &crate::groups::GroupReadResult,
) {
    let records = group_summary_records(client, result);
    if records.is_empty() {
        return;
    }
    let Ok(connection) = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    ) else {
        return;
    };
    for record in records {
        let _ = crate::internal::local_state::groups::upsert_group(&connection, record);
    }
}

#[cfg(all(feature = "sqlite", not(any(feature = "blocking", test))))]
pub(crate) fn project_group_summaries(
    _client: &crate::core::ImClient,
    _result: &crate::groups::GroupReadResult,
) {
}

#[cfg(not(feature = "sqlite"))]
pub(crate) fn project_group_summaries(
    _client: &crate::core::ImClient,
    _result: &crate::groups::GroupReadResult,
) {
}

#[cfg(feature = "sqlite")]
pub(crate) async fn project_group_summaries_async(
    client: &crate::core::ImClient,
    result: &crate::groups::GroupReadResult,
) -> crate::ImResult<()> {
    let records = group_summary_records(client, result);
    if records.is_empty() {
        return Ok(());
    }
    let db = client.core_inner().local_state_db().await?;
    for record in records {
        db.upsert_group(record).await?;
    }
    Ok(())
}

#[cfg(not(feature = "sqlite"))]
pub(crate) async fn project_group_summaries_async(
    _client: &crate::core::ImClient,
    _result: &crate::groups::GroupReadResult,
) -> crate::ImResult<()> {
    Ok(())
}

#[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
pub(crate) fn project_group_members(
    client: &crate::core::ImClient,
    group_did: &str,
    result: &crate::groups::GroupReadResult,
) {
    let members = group_member_records(client, group_did, result);
    let raw_has_members = result
        .raw_response()
        .and_then(|raw| raw.get("members"))
        .is_some();
    if members.is_empty() && !raw_has_members {
        return;
    }
    let Ok(mut connection) = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    ) else {
        return;
    };
    let _ = crate::internal::local_state::groups::replace_group_members(
        &mut connection,
        client.current_identity().id.as_str(),
        client.did().as_str(),
        &group_storage_key(group_did),
        &members,
        client.current_identity().id.as_str(),
    );
}

#[cfg(all(feature = "sqlite", not(any(feature = "blocking", test))))]
pub(crate) fn project_group_members(
    _client: &crate::core::ImClient,
    _group_did: &str,
    _result: &crate::groups::GroupReadResult,
) {
}

#[cfg(not(feature = "sqlite"))]
pub(crate) fn project_group_members(
    _client: &crate::core::ImClient,
    _group_did: &str,
    _result: &crate::groups::GroupReadResult,
) {
}

#[cfg(feature = "sqlite")]
pub(crate) async fn project_group_members_async(
    client: &crate::core::ImClient,
    group_did: &str,
    result: &crate::groups::GroupReadResult,
) -> crate::ImResult<()> {
    let members = group_member_records(client, group_did, result);
    let raw_has_members = result
        .raw_response()
        .and_then(|raw| raw.get("members"))
        .is_some();
    if members.is_empty() && !raw_has_members {
        return Ok(());
    }
    client
        .core_inner()
        .local_state_db()
        .await?
        .replace_group_members(
            client.current_identity().id.as_str(),
            client.did().as_str(),
            group_storage_key(group_did),
            members,
            client.current_identity().id.as_str(),
        )
        .await
}

#[cfg(not(feature = "sqlite"))]
pub(crate) async fn project_group_members_async(
    _client: &crate::core::ImClient,
    _group_did: &str,
    _result: &crate::groups::GroupReadResult,
) -> crate::ImResult<()> {
    Ok(())
}

#[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
pub(crate) fn project_group_messages(
    client: &crate::core::ImClient,
    group_did: &str,
    result: &crate::groups::GroupReadResult,
) {
    let records = group_message_records(client, group_did, result);
    if records.is_empty() {
        return;
    }
    let Ok(connection) = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    ) else {
        return;
    };
    let _ = crate::internal::local_state::messages::upsert_messages(&connection, &records);
}

#[cfg(all(feature = "sqlite", not(any(feature = "blocking", test))))]
pub(crate) fn project_group_messages(
    _client: &crate::core::ImClient,
    _group_did: &str,
    _result: &crate::groups::GroupReadResult,
) {
}

#[cfg(not(feature = "sqlite"))]
pub(crate) fn project_group_messages(
    _client: &crate::core::ImClient,
    _group_did: &str,
    _result: &crate::groups::GroupReadResult,
) {
}

#[cfg(feature = "sqlite")]
pub(crate) async fn project_group_messages_async(
    client: &crate::core::ImClient,
    group_did: &str,
    result: &crate::groups::GroupReadResult,
) -> crate::ImResult<()> {
    let records = group_message_records(client, group_did, result);
    if records.is_empty() {
        return Ok(());
    }
    client
        .core_inner()
        .local_state_db()
        .await?
        .store_messages(records)
        .await
}

#[cfg(not(feature = "sqlite"))]
pub(crate) async fn project_group_messages_async(
    _client: &crate::core::ImClient,
    _group_did: &str,
    _result: &crate::groups::GroupReadResult,
) -> crate::ImResult<()> {
    Ok(())
}

#[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
pub(crate) fn project_group_left(client: &crate::core::ImClient, group_did: &str) {
    let Ok(mut connection) = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    ) else {
        return;
    };
    let _ = crate::internal::local_state::groups::mark_group_left(
        &mut connection,
        client.current_identity().id.as_str(),
        client.did().as_str(),
        &group_storage_key(group_did),
        group_did,
        client.current_identity().id.as_str(),
    );
}

#[cfg(all(feature = "sqlite", not(any(feature = "blocking", test))))]
pub(crate) fn project_group_left(_client: &crate::core::ImClient, _group_did: &str) {}

#[cfg(not(feature = "sqlite"))]
pub(crate) fn project_group_left(_client: &crate::core::ImClient, _group_did: &str) {}

#[cfg(feature = "sqlite")]
pub(crate) async fn project_group_left_async(
    client: &crate::core::ImClient,
    group_did: &str,
) -> crate::ImResult<()> {
    client
        .core_inner()
        .local_state_db()
        .await?
        .mark_group_left(
            client.current_identity().id.as_str(),
            client.did().as_str(),
            group_storage_key(group_did),
            group_did,
            client.current_identity().id.as_str(),
        )
        .await
}

#[cfg(not(feature = "sqlite"))]
pub(crate) async fn project_group_left_async(
    _client: &crate::core::ImClient,
    _group_did: &str,
) -> crate::ImResult<()> {
    Ok(())
}

#[cfg(feature = "sqlite")]
fn group_record(
    client: &crate::core::ImClient,
    result: &crate::groups::GroupReadResult,
) -> Option<crate::internal::local_state::groups::GroupRecord> {
    let raw = result.raw_response().cloned().unwrap_or(Value::Null);
    let snapshot = snapshot_from_result(result).or_else(|| normalize_group_snapshot(&raw))?;
    let group_did = string_value(snapshot.get("group_did"));
    if group_did.trim().is_empty() {
        return None;
    }
    let last_synced_seq = i64_option(snapshot.get("group_event_seq"))
        .or_else(|| i64_option(snapshot.get("last_synced_seq")));
    Some(crate::internal::local_state::groups::GroupRecord {
        owner_identity_id: client.current_identity().id.as_str().to_string(),
        owner_did: client.did().as_str().to_string(),
        group_id: group_storage_key(&group_did),
        group_did,
        name: string_value(snapshot.get("name")),
        slug: string_value(snapshot.get("slug")),
        description: string_value(snapshot.get("description")),
        goal: string_value(snapshot.get("goal")),
        rules: string_value(snapshot.get("rules")),
        message_prompt: string_value(snapshot.get("message_prompt")),
        doc_url: string_value(snapshot.get("doc_url")),
        group_owner_did: string_value(snapshot.get("owner_did")),
        my_role: default_string(
            &string_value(snapshot.get("member_role")),
            &string_value(snapshot.get("my_role")),
        ),
        membership_status: default_string(
            &string_value(snapshot.get("member_status")),
            &string_value(snapshot.get("membership_status")),
        ),
        join_enabled: bool_option(snapshot.get("join_enabled")),
        member_count: i64_option(snapshot.get("member_count")),
        last_synced_seq,
        remote_created_at: string_value(snapshot.get("created_at")),
        remote_updated_at: string_value(snapshot.get("updated_at")),
        metadata: metadata_string(snapshot),
        credential_name: client.current_identity().id.as_str().to_string(),
        ..crate::internal::local_state::groups::GroupRecord::default()
    })
}

#[cfg(feature = "sqlite")]
fn group_member_records(
    client: &crate::core::ImClient,
    group_did: &str,
    result: &crate::groups::GroupReadResult,
) -> Vec<crate::internal::local_state::groups::GroupMemberRecord> {
    let raw = result.raw_response().cloned().unwrap_or(Value::Null);
    let members = members_from_result(result, &raw);
    members
        .iter()
        .filter_map(|member| group_member_record(client, group_did, member))
        .collect()
}

#[cfg(feature = "sqlite")]
fn group_summary_records(
    client: &crate::core::ImClient,
    result: &crate::groups::GroupReadResult,
) -> Vec<crate::internal::local_state::groups::GroupRecord> {
    let raw = result.raw_response().cloned().unwrap_or(Value::Null);
    if !result.groups.is_empty() {
        return result
            .groups
            .iter()
            .filter_map(|group| {
                group_record_from_snapshot(
                    client,
                    serde_json::json!({
                        "id": group.id,
                        "group_did": group.did.as_str(),
                        "did": group.did.as_str(),
                        "name": group.name,
                        "member_role": group.my_role,
                        "my_role": group.my_role,
                        "member_status": group.membership_status,
                        "membership_status": group.membership_status,
                        "member_count": group.member_count,
                        "last_message_at": group.last_message_at,
                    }),
                )
            })
            .collect();
    }
    values_from_array(raw.get("groups"))
        .into_iter()
        .filter_map(|group| {
            let snapshot = normalize_group_snapshot(&group).unwrap_or(group);
            group_record_from_snapshot(client, snapshot)
        })
        .collect()
}

#[cfg(feature = "sqlite")]
fn group_member_record(
    client: &crate::core::ImClient,
    group_did: &str,
    member: &Value,
) -> Option<crate::internal::local_state::groups::GroupMemberRecord> {
    let member_did = default_string(
        &string_value(member.get("agent_did")),
        &default_string(
            &string_value(member.get("member_did")),
            &string_value(member.get("did")),
        ),
    );
    if member_did.trim().is_empty() {
        return None;
    }
    let member_handle = normalize_handle_value(&default_string(
        &string_value(member.get("handle")),
        &default_string(
            &string_value(member.get("member_handle")),
            &string_value(member.get("agent_handle")),
        ),
    ));
    Some(crate::internal::local_state::groups::GroupMemberRecord {
        owner_identity_id: client.current_identity().id.as_str().to_string(),
        owner_did: client.did().as_str().to_string(),
        group_id: group_storage_key(group_did),
        user_id: member_did.clone(),
        member_did,
        member_handle,
        role: string_value(member.get("role")),
        status: string_value(member.get("status")),
        joined_at: string_value(member.get("joined_at")),
        metadata: metadata_string(member.clone()),
        credential_name: client.current_identity().id.as_str().to_string(),
        ..crate::internal::local_state::groups::GroupMemberRecord::default()
    })
}

#[cfg(feature = "sqlite")]
fn group_message_records(
    client: &crate::core::ImClient,
    group_did: &str,
    result: &crate::groups::GroupReadResult,
) -> Vec<crate::internal::local_state::messages::MessageRecord> {
    result
        .messages
        .items
        .iter()
        .map(|message| group_message_record(client, group_did, message))
        .collect()
}

#[cfg(feature = "sqlite")]
fn group_message_record(
    client: &crate::core::ImClient,
    group_did: &str,
    message: &crate::messages::Message,
) -> crate::internal::local_state::messages::MessageRecord {
    let group_did = message
        .group
        .as_ref()
        .map(crate::ids::GroupRef::as_str)
        .unwrap_or(group_did);
    crate::internal::local_state::messages::MessageRecord {
        msg_id: message.id.as_str().to_string(),
        owner_identity_id: client.current_identity().id.as_str().to_string(),
        owner_did: client.did().as_str().to_string(),
        thread_id: group_thread_id(group_did),
        direction: match message.direction {
            crate::messages::MessageDirection::Outgoing => 1,
            crate::messages::MessageDirection::Incoming => 0,
            crate::messages::MessageDirection::Unknown => -1,
        },
        sender_did: message.sender.as_str().to_string(),
        receiver_did: message
            .receiver
            .as_ref()
            .map(crate::ids::PeerRef::as_str)
            .unwrap_or_default()
            .to_string(),
        group_id: group_storage_key(group_did),
        group_did: group_did.trim().to_string(),
        content_type: message_content_type(message),
        content: message_content(message),
        server_seq: message.metadata.server_sequence,
        sent_at: message.sent_at.clone().unwrap_or_default(),
        is_e2ee: false,
        is_read: false,
        metadata: message_metadata_string(message),
        credential_name: client.current_identity().id.as_str().to_string(),
        ..crate::internal::local_state::messages::MessageRecord::default()
    }
}

#[cfg(feature = "sqlite")]
fn snapshot_from_result(result: &crate::groups::GroupReadResult) -> Option<Value> {
    let snapshot = result.group.as_ref()?;
    Some(serde_json::json!({
        "id": snapshot.id,
        "group_did": snapshot.did.as_str(),
        "did": snapshot.did.as_str(),
        "name": snapshot.name,
        "description": snapshot.description,
        "member_role": snapshot.my_role,
        "my_role": snapshot.my_role,
        "member_status": snapshot.membership_status,
        "membership_status": snapshot.membership_status,
        "member_count": snapshot.member_count,
        "last_message_at": snapshot.last_message_at,
    }))
}

#[cfg(feature = "sqlite")]
fn group_record_from_snapshot(
    client: &crate::core::ImClient,
    snapshot: Value,
) -> Option<crate::internal::local_state::groups::GroupRecord> {
    let group_did = string_value(snapshot.get("group_did"));
    if group_did.trim().is_empty() {
        return None;
    }
    Some(crate::internal::local_state::groups::GroupRecord {
        owner_identity_id: client.current_identity().id.as_str().to_string(),
        owner_did: client.did().as_str().to_string(),
        group_id: group_storage_key(&group_did),
        group_did,
        name: string_value(snapshot.get("name")),
        slug: string_value(snapshot.get("slug")),
        description: string_value(snapshot.get("description")),
        goal: string_value(snapshot.get("goal")),
        rules: string_value(snapshot.get("rules")),
        message_prompt: string_value(snapshot.get("message_prompt")),
        doc_url: string_value(snapshot.get("doc_url")),
        group_owner_did: string_value(snapshot.get("owner_did")),
        my_role: default_string(
            &string_value(snapshot.get("member_role")),
            &string_value(snapshot.get("my_role")),
        ),
        membership_status: default_string(
            &string_value(snapshot.get("member_status")),
            &string_value(snapshot.get("membership_status")),
        ),
        join_enabled: bool_option(snapshot.get("join_enabled")),
        member_count: i64_option(snapshot.get("member_count")),
        last_synced_seq: i64_option(snapshot.get("group_event_seq")),
        last_message_at: string_value(snapshot.get("last_message_at")),
        remote_created_at: string_value(snapshot.get("created_at")),
        remote_updated_at: string_value(snapshot.get("updated_at")),
        metadata: metadata_string(snapshot),
        credential_name: client.current_identity().id.as_str().to_string(),
        ..crate::internal::local_state::groups::GroupRecord::default()
    })
}

#[cfg(feature = "sqlite")]
fn members_from_result(result: &crate::groups::GroupReadResult, raw: &Value) -> Vec<Value> {
    if !result.members.is_empty() {
        return result
            .members
            .iter()
            .map(|member| {
                let did = member
                    .did
                    .as_ref()
                    .map(crate::ids::Did::as_str)
                    .unwrap_or_default();
                let handle = member
                    .handle
                    .as_ref()
                    .map(crate::ids::Handle::as_str)
                    .map(normalize_handle_value)
                    .unwrap_or_default();
                serde_json::json!({
                    "member_did": did,
                    "did": did,
                    "member_handle": handle,
                    "handle": handle,
                    "role": member.role,
                    "status": member.status,
                    "joined_at": member.joined_at,
                })
            })
            .collect();
    }
    values_from_array(raw.get("members"))
        .into_iter()
        .map(normalize_group_member_json)
        .collect()
}

#[cfg(feature = "sqlite")]
fn normalize_group_snapshot(raw: &Value) -> Option<Value> {
    if raw.is_null() {
        return None;
    }
    if let Some(snapshot) = raw.get("group_snapshot").filter(|value| value.is_object()) {
        return Some(snapshot.clone());
    }
    let group_did = group_did_from_result(raw);
    if group_did.trim().is_empty() {
        return None;
    }
    if let Some(profile) = raw.get("group_profile").filter(|value| value.is_object()) {
        return Some(serde_json::json!({
            "group_did": group_did,
            "did": group_did,
            "group_state_version": raw.get("group_state_version").cloned().unwrap_or(Value::Null),
            "name": string_value(profile.get("display_name")),
            "description": profile.get("description").cloned().unwrap_or(Value::Null),
            "discoverability": profile.get("discoverability").cloned().unwrap_or(Value::Null),
            "slug": profile.get("slug").cloned().unwrap_or(Value::Null),
            "goal": profile.get("goal").cloned().unwrap_or(Value::Null),
            "rules": profile.get("rules").cloned().unwrap_or(Value::Null),
            "message_prompt": profile.get("message_prompt").cloned().unwrap_or(Value::Null),
            "doc_url": profile.get("doc_url").cloned().unwrap_or(Value::Null),
            "owner_did": raw.get("owner_did").cloned().unwrap_or(Value::Null),
            "member_role": raw.get("member_role").or_else(|| raw.get("my_role")).cloned().unwrap_or(Value::Null),
            "my_role": raw.get("my_role").or_else(|| raw.get("member_role")).cloned().unwrap_or(Value::Null),
            "member_status": raw.get("member_status").or_else(|| raw.get("membership_status")).cloned().unwrap_or(Value::Null),
            "membership_status": raw.get("membership_status").or_else(|| raw.get("member_status")).cloned().unwrap_or(Value::Null),
            "join_enabled": raw.get("join_enabled").cloned().unwrap_or(Value::Null),
            "member_count": raw.get("member_count").cloned().unwrap_or(Value::Null),
            "group_profile": profile,
            "group_policy": raw.get("group_policy").cloned().unwrap_or(Value::Null),
            "created_at": raw.get("created_at").cloned().unwrap_or(Value::Null),
            "updated_at": raw.get("updated_at").cloned().unwrap_or(Value::Null),
        }));
    }
    Some(raw.clone())
}

#[cfg(feature = "sqlite")]
fn normalize_group_member_json(mut member: Value) -> Value {
    let Some(object) = member.as_object_mut() else {
        return member;
    };
    let did = default_string(
        &string_value(object.get("agent_did")),
        &default_string(
            &string_value(object.get("member_did")),
            &string_value(object.get("did")),
        ),
    );
    if !did.trim().is_empty() {
        object
            .entry("member_did".to_string())
            .or_insert_with(|| Value::String(did.clone()));
        object
            .entry("did".to_string())
            .or_insert_with(|| Value::String(did));
    }
    let handle = normalize_handle_value(&default_string(
        &string_value(object.get("handle")),
        &default_string(
            &string_value(object.get("member_handle")),
            &string_value(object.get("agent_handle")),
        ),
    ));
    if !handle.is_empty() {
        object.insert("member_handle".to_string(), Value::String(handle.clone()));
        object
            .entry("handle".to_string())
            .or_insert_with(|| Value::String(handle));
    }
    member
}

#[cfg(feature = "sqlite")]
fn values_from_array(value: Option<&Value>) -> Vec<Value> {
    value.and_then(Value::as_array).cloned().unwrap_or_default()
}

#[cfg(feature = "sqlite")]
fn group_did_from_result(raw: &Value) -> String {
    string_value(raw.get("group_did"))
        .trim()
        .to_string()
        .or_else_nonempty(|| string_value(raw.get("did")))
}

#[cfg(feature = "sqlite")]
fn group_storage_key(group_did: &str) -> String {
    group_did.trim().to_string()
}

#[cfg(feature = "sqlite")]
fn group_thread_id(group_did: &str) -> String {
    let value = group_did.trim();
    if value.is_empty() {
        "group:unknown".to_string()
    } else {
        format!("group:{value}")
    }
}

#[cfg(feature = "sqlite")]
fn message_content(message: &crate::messages::Message) -> String {
    match &message.body {
        crate::messages::MessageBodyView::Text { text, .. } => text.clone(),
        crate::messages::MessageBodyView::Unsupported { .. } => String::new(),
    }
}

#[cfg(feature = "sqlite")]
fn message_content_type(message: &crate::messages::Message) -> String {
    message
        .metadata
        .content_type
        .clone()
        .unwrap_or_else(|| match &message.body {
            crate::messages::MessageBodyView::Text {
                kind: crate::messages::MessageKind::Markdown,
                ..
            } => "text/markdown".to_string(),
            crate::messages::MessageBodyView::Text { .. } => "text/plain".to_string(),
            crate::messages::MessageBodyView::Unsupported { content_type } => content_type
                .clone()
                .unwrap_or_else(|| "application/octet-stream".to_string()),
        })
}

#[cfg(feature = "sqlite")]
fn message_metadata_string(message: &crate::messages::Message) -> String {
    let mut metadata = serde_json::Map::new();
    insert_string(
        &mut metadata,
        "operation_id",
        message.metadata.operation_id.as_deref(),
    );
    insert_string(
        &mut metadata,
        "delivery_state",
        message.metadata.delivery_state.as_deref(),
    );
    if let Some(server_sequence) = message.metadata.server_sequence {
        metadata.insert(
            "server_seq".to_string(),
            Value::Number(serde_json::Number::from(server_sequence)),
        );
    }
    Value::Object(metadata).to_string()
}

#[cfg(feature = "sqlite")]
fn insert_string(object: &mut serde_json::Map<String, Value>, key: &str, value: Option<&str>) {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    object.insert(key.to_string(), Value::String(value.to_string()));
}

#[cfg(feature = "sqlite")]
fn normalize_handle_value(value: &str) -> String {
    let value = value.trim().to_ascii_lowercase();
    if value.is_empty() {
        return String::new();
    }
    let value = value.trim_start_matches("wba://");
    match value.find('.') {
        Some(index) if index > 0 => value[..index].to_string(),
        _ => value.to_string(),
    }
}

#[cfg(feature = "sqlite")]
fn string_value(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

#[cfg(feature = "sqlite")]
fn default_string(value: &str, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

#[cfg(feature = "sqlite")]
fn bool_option(value: Option<&Value>) -> Option<bool> {
    match value {
        Some(Value::Bool(value)) => Some(*value),
        Some(Value::Number(number)) => number.as_i64().map(|value| value != 0),
        Some(Value::String(value)) if value.trim().is_empty() => None,
        Some(Value::String(value)) => Some(matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "y" | "on"
        )),
        _ => None,
    }
}

#[cfg(feature = "sqlite")]
fn i64_option(value: Option<&Value>) -> Option<i64> {
    match value {
        Some(Value::Number(number)) => number
            .as_i64()
            .or_else(|| number.as_u64().and_then(|value| i64::try_from(value).ok()))
            .or_else(|| number.as_f64().map(|value| value as i64)),
        Some(Value::String(value)) => value.trim().parse::<i64>().ok(),
        _ => None,
    }
}

#[cfg(feature = "sqlite")]
fn metadata_string(value: Value) -> String {
    serde_json::to_string(&value).unwrap_or_default()
}

#[cfg(feature = "sqlite")]
trait NonEmptyString {
    fn or_else_nonempty(self, fallback: impl FnOnce() -> String) -> String;
}

#[cfg(feature = "sqlite")]
impl NonEmptyString for String {
    fn or_else_nonempty(self, fallback: impl FnOnce() -> String) -> String {
        if self.trim().is_empty() {
            fallback()
        } else {
            self
        }
    }
}
