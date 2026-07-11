use serde_json::Value;

#[cfg(feature = "sqlite")]
use crate::internal::local_state::owner_scope::OwnerScope;

#[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
pub(crate) fn project_group_snapshot(
    client: &crate::core::ImClient,
    result: &crate::groups::GroupReadResult,
) {
    let Ok(scope) = OwnerScope::for_client(client) else {
        return;
    };
    let Some(record) = group_record(&scope, result) else {
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
    let scope = OwnerScope::for_client(client)?;
    let Some(record) = group_record(&scope, result) else {
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
    let Ok(scope) = OwnerScope::for_client(client) else {
        return;
    };
    let records = group_summary_records(&scope, result);
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
    let scope = OwnerScope::for_client(client)?;
    let records = group_summary_records(&scope, result);
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
    let Ok(scope) = OwnerScope::for_client(client) else {
        return;
    };
    let Ok(mut connection) = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    ) else {
        return;
    };
    let _ = project_group_members_with_connection(&mut connection, &scope, group_did, result);
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
    let scope = OwnerScope::for_client(client)?;
    let members = group_member_records(&scope, group_did, result)?;
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
            scope.owner_identity_id.as_str(),
            scope.owner_did.as_str(),
            group_storage_key(group_did),
            members,
            credential_name(&scope),
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
    let Ok(scope) = OwnerScope::for_client(client) else {
        return;
    };
    let records = group_message_records(&scope, group_did, result);
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
    let scope = OwnerScope::for_client(client)?;
    let records = group_message_records(&scope, group_did, result);
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
    let Ok(scope) = OwnerScope::for_client(client) else {
        return;
    };
    let Ok(mut connection) = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    ) else {
        return;
    };
    let _ = crate::internal::local_state::groups::mark_group_left(
        &mut connection,
        scope.owner_identity_id.as_str(),
        scope.owner_did.as_str(),
        &group_storage_key(group_did),
        group_did,
        credential_name(&scope).as_str(),
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
    let scope = OwnerScope::for_client(client)?;
    client
        .core_inner()
        .local_state_db()
        .await?
        .mark_group_left(
            scope.owner_identity_id.as_str(),
            scope.owner_did.as_str(),
            group_storage_key(group_did),
            group_did,
            credential_name(&scope),
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
    scope: &OwnerScope,
    result: &crate::groups::GroupReadResult,
) -> Option<crate::internal::local_state::groups::GroupRecord> {
    let raw = result.raw_response().cloned().unwrap_or(Value::Null);
    let snapshot =
        snapshot_from_result(result).or_else(|| group_snapshot_from_raw_response(&raw))?;
    let group_did = string_value(snapshot.get("group_did"));
    if group_did.trim().is_empty() {
        return None;
    }
    let last_synced_seq = i64_option(snapshot.get("group_event_seq"))
        .or_else(|| i64_option(snapshot.get("last_synced_seq")));
    Some(crate::internal::local_state::groups::GroupRecord {
        owner_identity_id: scope.owner_identity_id.clone(),
        owner_did: scope.owner_did.clone(),
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
        credential_name: credential_name(scope),
        ..crate::internal::local_state::groups::GroupRecord::default()
    })
}

#[cfg(feature = "sqlite")]
fn group_member_records(
    scope: &OwnerScope,
    group_did: &str,
    result: &crate::groups::GroupReadResult,
) -> crate::ImResult<Vec<crate::internal::local_state::groups::GroupMemberRecord>> {
    let raw = result.raw_response().cloned().unwrap_or(Value::Null);
    let members = members_from_result(result, &raw);
    let mut records = Vec::with_capacity(members.len());
    for member in &members {
        if let Some(record) = group_member_record(scope, group_did, member)? {
            records.push(record);
        }
    }
    Ok(records)
}

#[cfg(feature = "sqlite")]
fn project_group_members_with_connection(
    connection: &mut rusqlite::Connection,
    scope: &OwnerScope,
    group_did: &str,
    result: &crate::groups::GroupReadResult,
) -> crate::ImResult<()> {
    let members = group_member_records(scope, group_did, result)?;
    let raw_has_members = result
        .raw_response()
        .and_then(|raw| raw.get("members"))
        .is_some();
    if members.is_empty() && !raw_has_members {
        return Ok(());
    }
    crate::internal::local_state::groups::replace_group_members(
        connection,
        scope.owner_identity_id.as_str(),
        scope.owner_did.as_str(),
        &group_storage_key(group_did),
        &members,
        credential_name(scope).as_str(),
    )
}

#[cfg(feature = "sqlite")]
fn group_summary_records(
    scope: &OwnerScope,
    result: &crate::groups::GroupReadResult,
) -> Vec<crate::internal::local_state::groups::GroupRecord> {
    let raw = result.raw_response().cloned().unwrap_or(Value::Null);
    if !result.groups.is_empty() {
        return result
            .groups
            .iter()
            .filter_map(|group| {
                group_record_from_snapshot(
                    scope,
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
            group_record_from_snapshot(scope, snapshot)
        })
        .collect()
}

#[cfg(feature = "sqlite")]
fn group_member_record(
    scope: &OwnerScope,
    group_did: &str,
    member: &Value,
) -> crate::ImResult<Option<crate::internal::local_state::groups::GroupMemberRecord>> {
    let member_did = default_string(
        &string_value(member.get("did")),
        &default_string(
            &string_value(member.get("member_did")),
            &string_value(member.get("agent_did")),
        ),
    );
    if member_did.trim().is_empty() {
        return Ok(None);
    }
    let use_agent_handle = member_identity_uses_agent_fields(member);
    let protocol_member_handle = normalize_handle_value(&string_value(member.get("member_handle")));
    let member_handle = normalize_handle_value(&default_string(
        &string_value(member.get("handle")),
        &default_string(
            &string_value(member.get("member_handle")),
            &use_agent_handle
                .then(|| string_value(member.get("agent_handle")))
                .unwrap_or_default(),
        ),
    ));
    let handle_binding_generation = string_value(member.get("handle_binding_generation"));
    let has_protocol_handle = !protocol_member_handle.is_empty();
    let has_generation = !handle_binding_generation.is_empty();
    if has_protocol_handle != has_generation {
        return Err(crate::ImError::invalid_input(
            Some("group_member".to_owned()),
            "member_handle and handle_binding_generation must appear together",
        ));
    }
    let handle_backed = has_protocol_handle;
    Ok(Some(
        crate::internal::local_state::groups::GroupMemberRecord {
            owner_identity_id: scope.owner_identity_id.clone(),
            owner_did: scope.owner_did.clone(),
            group_id: group_storage_key(group_did),
            user_id: String::new(),
            member_did,
            member_handle: member_handle.clone(),
            anchor_kind: if handle_backed { "handle" } else { "did" }.to_owned(),
            anchor_value: if handle_backed {
                protocol_member_handle
            } else {
                default_string(
                    &string_value(member.get("did")),
                    &default_string(
                        &string_value(member.get("member_did")),
                        &string_value(member.get("agent_did")),
                    ),
                )
            },
            handle_binding_generation,
            role: string_value(member.get("role")),
            status: string_value(member.get("status")),
            joined_at: string_value(member.get("joined_at")),
            metadata: metadata_string(member.clone()),
            credential_name: credential_name(scope),
            ..crate::internal::local_state::groups::GroupMemberRecord::default()
        },
    ))
}

#[cfg(feature = "sqlite")]
fn group_message_records(
    scope: &OwnerScope,
    group_did: &str,
    result: &crate::groups::GroupReadResult,
) -> Vec<crate::internal::local_state::messages::MessageRecord> {
    result
        .messages
        .items
        .iter()
        .map(|message| group_message_record(scope, group_did, message))
        .collect()
}

#[cfg(feature = "sqlite")]
fn group_message_record(
    scope: &OwnerScope,
    group_did: &str,
    message: &crate::messages::Message,
) -> crate::internal::local_state::messages::MessageRecord {
    let group_did = message
        .group
        .as_ref()
        .map(crate::ids::GroupRef::as_str)
        .unwrap_or(group_did);
    let conversation_id = group_conversation_id(group_did);
    crate::internal::local_state::messages::MessageRecord {
        msg_id: message.id.as_str().to_string(),
        owner_identity_id: scope.owner_identity_id.clone(),
        owner_did: scope.owner_did.clone(),
        conversation_id: conversation_id.clone(),
        thread_id: conversation_id,
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
        credential_name: credential_name(scope),
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
    scope: &OwnerScope,
    snapshot: Value,
) -> Option<crate::internal::local_state::groups::GroupRecord> {
    let group_did = string_value(snapshot.get("group_did"));
    if group_did.trim().is_empty() {
        return None;
    }
    Some(crate::internal::local_state::groups::GroupRecord {
        owner_identity_id: scope.owner_identity_id.clone(),
        owner_did: scope.owner_did.clone(),
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
        credential_name: credential_name(scope),
        ..crate::internal::local_state::groups::GroupRecord::default()
    })
}

#[cfg(feature = "sqlite")]
fn members_from_result(result: &crate::groups::GroupReadResult, raw: &Value) -> Vec<Value> {
    if raw.get("members").is_some() {
        return values_from_array(raw.get("members"))
            .into_iter()
            .map(normalize_group_member_json)
            .collect();
    }
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
                let generation = member
                    .handle_binding_generation
                    .as_deref()
                    .unwrap_or_default();
                serde_json::json!({
                    "member_did": did,
                    "did": did,
                    "member_handle": if generation.is_empty() { "" } else { handle.as_str() },
                    "handle": handle,
                    "handle_binding_generation": generation,
                    "subject_type": member.subject_type,
                    "role": member.role,
                    "status": member.status,
                    "joined_at": member.joined_at,
                })
            })
            .collect();
    }
    Vec::new()
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
    if raw_is_group_snapshot(raw) {
        Some(raw.clone())
    } else {
        None
    }
}

#[cfg(feature = "sqlite")]
fn group_snapshot_from_raw_response(raw: &Value) -> Option<Value> {
    if let Some(group) = raw.get("group").filter(|value| value.is_object()) {
        return normalize_group_snapshot(group);
    }
    normalize_group_snapshot(raw)
}

#[cfg(feature = "sqlite")]
fn raw_is_group_snapshot(raw: &Value) -> bool {
    let Some(object) = raw.as_object() else {
        return false;
    };
    if object.contains_key("accepted")
        || object.contains_key("final_acceptance")
        || object.contains_key("operation_id")
        || object.contains_key("group_receipt")
        || object.contains_key("member_did")
        || object.contains_key("leaver_did")
    {
        return false;
    }
    object.contains_key("group_snapshot")
        || object.contains_key("group_profile")
        || object.contains_key("name")
        || object.contains_key("display_name")
        || object.contains_key("description")
        || object.contains_key("member_role")
        || object.contains_key("my_role")
        || object.contains_key("actor_membership_role")
        || object.contains_key("member_status")
        || object.contains_key("actor_membership_status")
}

#[cfg(feature = "sqlite")]
fn normalize_group_member_json(mut member: Value) -> Value {
    let Some(object) = member.as_object_mut() else {
        return member;
    };
    let use_agent_handle = member_identity_uses_agent_fields_object(object);
    let did = default_string(
        &string_value(object.get("did")),
        &default_string(
            &string_value(object.get("member_did")),
            &string_value(object.get("agent_did")),
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
    let protocol_member_handle = normalize_handle_value(&string_value(object.get("member_handle")));
    let handle = normalize_handle_value(&default_string(
        &string_value(object.get("handle")),
        &default_string(
            &protocol_member_handle,
            &use_agent_handle
                .then(|| string_value(object.get("agent_handle")))
                .unwrap_or_default(),
        ),
    ));
    if !protocol_member_handle.is_empty() {
        object.insert(
            "member_handle".to_string(),
            Value::String(protocol_member_handle),
        );
    }
    if !handle.is_empty() {
        object
            .entry("handle".to_string())
            .or_insert_with(|| Value::String(handle));
    }
    member
}

#[cfg(feature = "sqlite")]
fn member_identity_uses_agent_fields(member: &Value) -> bool {
    let Some(object) = member.as_object() else {
        return false;
    };
    member_identity_uses_agent_fields_object(object)
}

#[cfg(feature = "sqlite")]
fn member_identity_uses_agent_fields_object(object: &serde_json::Map<String, Value>) -> bool {
    let explicit_member_did =
        string_value(object.get("did")).or_else_nonempty(|| string_value(object.get("member_did")));
    if explicit_member_did.trim().is_empty() {
        return true;
    }
    if let Some(subject_type) = object
        .get("subject_type")
        .or_else(|| object.get("subjectType"))
        .or_else(|| object.get("member_subject_type"))
        .and_then(Value::as_str)
    {
        return matches!(
            subject_type.trim().to_ascii_lowercase().as_str(),
            "agent" | "runtime_agent" | "bot"
        );
    }
    let normalized_did = explicit_member_did.trim().to_ascii_lowercase();
    normalized_did.starts_with("did:agent:") || normalized_did.contains(":agent:")
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

#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use super::*;
    use serde_json::json;

    fn scope() -> OwnerScope {
        OwnerScope::new("owner-identity", "did:example:alice").expect("owner scope")
    }

    #[test]
    fn group_record_ignores_member_mutation_receipt_status() {
        let result = crate::groups::GroupReadResult::from_raw_response(
            json!({
                "accepted": true,
                "final_acceptance": true,
                "group_did": "did:example:group",
                "group_state_version": "12",
                "group_event_seq": "7",
                "operation_id": "op-remove-bob",
                "member_did": "did:example:bob",
                "membership_status": "removed"
            }),
            Vec::new(),
        );

        assert!(
            group_record(&scope(), &result).is_none(),
            "target member removal status must not be projected as the viewer's group membership status"
        );
    }

    #[test]
    fn group_record_accepts_explicit_local_group_snapshot() {
        let result = crate::groups::GroupReadResult::from_raw_response(
            json!({
                "group_snapshot": {
                    "group_did": "did:example:group",
                    "name": "Demo",
                    "member_role": "owner",
                    "member_status": "active",
                    "member_count": 2
                }
            }),
            Vec::new(),
        );

        let record = group_record(&scope(), &result).expect("group record");
        assert_eq!(record.group_did, "did:example:group");
        assert_eq!(record.my_role, "owner");
        assert_eq!(record.membership_status, "active");
    }

    #[test]
    fn group_member_record_prefers_member_identity_over_auxiliary_agent_fields() {
        let result = crate::groups::GroupReadResult::from_raw_response(
            json!({
                "group_did": "did:example:group",
                "members": [{
                    "member_did": "did:wba:anpclaw.com:zhuocheng:e1_human",
                    "agent_did": "did:wba:anpclaw.com:agent:runtime:helper:e1_agent",
                    "handle": "zhuocheng",
                    "agent_handle": "helper.anpclaw.com",
                    "agent_subject_type": "agent",
                    "role": "member",
                    "status": "active"
                }]
            }),
            Vec::new(),
        );

        let records = group_member_records(&scope(), "did:example:group", &result).unwrap();

        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].member_did,
            "did:wba:anpclaw.com:zhuocheng:e1_human"
        );
        assert!(records[0].user_id.is_empty());
        assert_eq!(records[0].anchor_kind, "did");
        assert_eq!(records[0].anchor_value, records[0].member_did);
        assert_eq!(records[0].member_handle, "zhuocheng");
    }

    fn assert_partial_handle_pair_preserves_existing_snapshot(member: Value) {
        let mut db = rusqlite::Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        crate::internal::local_state::groups::replace_group_members(
            &mut db,
            "owner-identity",
            "did:example:alice",
            "did:example:group",
            &[crate::internal::local_state::groups::GroupMemberRecord {
                user_id: "existing-peer-id".to_owned(),
                member_did: "did:example:existing".to_owned(),
                role: "admin".to_owned(),
                joined_at: "2026-01-01T00:00:00Z".to_owned(),
                ..crate::internal::local_state::groups::GroupMemberRecord::default()
            }],
            "owner-identity",
        )
        .unwrap();
        let result = crate::groups::GroupReadResult::from_raw_response(
            json!({
                "group_did": "did:example:group",
                "members": [member]
            }),
            Vec::new(),
        );

        let error =
            project_group_members_with_connection(&mut db, &scope(), "did:example:group", &result)
                .unwrap_err();
        assert!(error.to_string().contains("must appear together"));
        let preserved: (String, String, String, String) = db
            .query_row(
                "SELECT user_id, member_did, role, joined_at FROM group_members",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            preserved,
            (
                "existing-peer-id".to_owned(),
                "did:example:existing".to_owned(),
                "admin".to_owned(),
                "2026-01-01T00:00:00Z".to_owned(),
            )
        );
    }

    #[test]
    fn snapshot_member_handle_without_generation_fails_closed_before_replace() {
        assert_partial_handle_pair_preserves_existing_snapshot(json!({
            "member_did": "did:example:new",
            "member_handle": "new.example.com",
            "role": "member",
            "status": "active"
        }));
    }

    #[test]
    fn snapshot_generation_without_member_handle_fails_closed_before_replace() {
        assert_partial_handle_pair_preserves_existing_snapshot(json!({
            "member_did": "did:example:new",
            "handle_binding_generation": "2",
            "role": "member",
            "status": "active"
        }));
    }
}

#[cfg(feature = "sqlite")]
fn group_storage_key(group_did: &str) -> String {
    group_did.trim().to_string()
}

#[cfg(feature = "sqlite")]
fn group_conversation_id(group_did: &str) -> String {
    let value = group_did.trim();
    if value.is_empty() {
        "group:unknown".to_string()
    } else {
        format!("group:{value}")
    }
}

#[cfg(feature = "sqlite")]
fn credential_name(scope: &OwnerScope) -> String {
    scope
        .credential_name
        .clone()
        .unwrap_or_else(|| scope.owner_identity_id.clone())
}

#[cfg(feature = "sqlite")]
fn message_content(message: &crate::messages::Message) -> String {
    match &message.body {
        crate::messages::MessageBodyView::Text { text, .. } => text.clone(),
        crate::messages::MessageBodyView::Payload { payload } => {
            serde_json::to_string(payload).unwrap_or_default()
        }
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
            crate::messages::MessageBodyView::Payload { .. } => "application/json".to_string(),
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
