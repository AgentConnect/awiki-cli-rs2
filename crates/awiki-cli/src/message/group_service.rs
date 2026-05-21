use super::group_e2ee_add::{
    add_group_member_e2ee, group_member_mutation_uses_e2ee, group_snapshot_uses_e2ee,
};
use super::group_e2ee_remove::{leave_group_e2ee, remove_group_member_e2ee_result};
use super::service::{
    auth_session, bool_value, content_string, int_value, metadata_string, normalize_handle_value,
    require_active_identity, resolve_target, runtime_mode, string_value, CommandResult,
};
use super::types::{
    GroupGetRequest, GroupJoinRequest, GroupLeaveRequest, GroupListRequest, GroupMemberRequest,
    GroupMembersRequest, GroupMessagesRequest, GroupUpdateRequest, MessageError, SendRequest,
    MESSAGE_RPC_ENDPOINT,
};
use super::{
    build_group_add_rpc_params, build_group_get_rpc_params, build_group_join_rpc_params,
    build_group_leave_rpc_params, build_group_list_rpc_params, build_group_members_rpc_params,
    build_group_remove_rpc_params, build_group_update_policy_rpc_params,
    build_group_update_profile_rpc_params, content_type_for_message_type, Client,
};
use crate::config::Resolved;
use crate::identity::types::StoredIdentity;
use crate::identity::Manager;
use crate::store::{self, MessageRecord};
use crate::transportcfg::Profile;
use serde_json::{json, Map, Value};

#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub(crate) struct GroupSendResult {
    pub(crate) accepted: bool,
    pub(crate) final_acceptance: bool,
    pub(crate) group_did: String,
    pub(crate) message_id: String,
    pub(crate) operation_id: String,
    pub(crate) group_event_seq: String,
    pub(crate) group_state_version: String,
    pub(crate) accepted_at: String,
}

pub fn get_group(
    resolved: &Resolved,
    manager: &Manager,
    request: GroupGetRequest,
) -> Result<CommandResult, MessageError> {
    if request.group.trim().is_empty() {
        return Err(MessageError::GroupRequired);
    }
    let record = require_active_identity(resolved, manager, &request.identity_name)?;
    let mut auth = auth_session(resolved, manager, &record)?;
    let client = Client::new(resolved)?;
    let params = build_group_get_rpc_params(&record, request.clone())?;
    let raw: Value = client.authenticated_rpc_call_profile(
        Profile::RpcReadHeavy,
        MESSAGE_RPC_ENDPOINT,
        "group.get",
        params,
        &mut auth,
    )?;
    let mut warnings = group_control_warnings(resolved);
    warnings.extend(persist_group_snapshot(resolved, &record, &raw));
    let snapshot = cached_group_snapshot(resolved, &record, &request.group)
        .or_else(|| normalize_group_snapshot(&raw))
        .unwrap_or(Value::Null);
    Ok(CommandResult {
        data: json!({
            "group": snapshot,
            "source": group_control_source(&raw),
        }),
        summary: "Loaded group snapshot".to_string(),
        warnings: compact_warnings(&mut warnings),
    })
}

pub fn join_group(
    resolved: &Resolved,
    manager: &Manager,
    request: GroupJoinRequest,
) -> Result<CommandResult, MessageError> {
    if request.group.trim().is_empty() {
        return Err(MessageError::GroupRequired);
    }
    let record = require_active_identity(resolved, manager, &request.identity_name)?;
    let mut auth = auth_session(resolved, manager, &record)?;
    let client = Client::new(resolved)?;
    let params = build_group_join_rpc_params(&record, request.clone())?;
    let raw: Value = client.authenticated_rpc_call_profile(
        Profile::RpcDefault,
        MESSAGE_RPC_ENDPOINT,
        "group.join",
        params,
        &mut auth,
    )?;
    let group_did = default_string(&group_did_from_result(&raw), &request.group);
    let mut warnings = group_control_warnings(resolved);
    warnings.extend(sync_group_state(
        resolved, manager, &record, &group_did, true,
    ));
    let snapshot = cached_group_snapshot(resolved, &record, &group_did)
        .or_else(|| normalize_group_snapshot(&raw))
        .unwrap_or_else(|| json!({ "group_did": group_did }));
    Ok(CommandResult {
        data: json!({
            "group": snapshot,
            "delivery": raw,
            "source": group_control_source(&raw),
        }),
        summary: format!("Joined group {group_did}"),
        warnings: compact_warnings(&mut warnings),
    })
}

pub fn add_group_member(
    resolved: &Resolved,
    manager: &Manager,
    request: GroupMemberRequest,
) -> Result<CommandResult, MessageError> {
    mutate_group_member(resolved, manager, request, "add")
}

pub fn remove_group_member(
    resolved: &Resolved,
    manager: &Manager,
    request: GroupMemberRequest,
) -> Result<CommandResult, MessageError> {
    mutate_group_member(resolved, manager, request, "remove")
}

fn mutate_group_member(
    resolved: &Resolved,
    manager: &Manager,
    mut request: GroupMemberRequest,
    action: &str,
) -> Result<CommandResult, MessageError> {
    if request.group.trim().is_empty() {
        return Err(MessageError::GroupRequired);
    }
    if request.member.trim().is_empty() {
        return Err(MessageError::MemberRequired);
    }
    let record = require_active_identity(resolved, manager, &request.identity_name)?;
    let member = resolve_target(resolved, &request.member)?;
    request.member = member.did.clone();
    let pre_mutation_snapshot = (action == "add" || action == "remove")
        .then(|| cached_group_snapshot(resolved, &record, &request.group))
        .flatten();
    if action == "remove"
        && group_member_mutation_uses_e2ee(&request, pre_mutation_snapshot.as_ref(), None)
    {
        return remove_group_member_e2ee_result(
            resolved,
            manager,
            &record,
            &request,
            &member.handle,
        );
    }
    let mut auth = auth_session(resolved, manager, &record)?;
    let client = Client::new(resolved)?;
    let (method, params) = if action == "add" {
        (
            "group.add",
            build_group_add_rpc_params(&record, request.clone())?,
        )
    } else {
        (
            "group.remove",
            build_group_remove_rpc_params(&record, request.clone())?,
        )
    };
    let raw: Value = client.authenticated_rpc_call_profile(
        Profile::RpcDefault,
        MESSAGE_RPC_ENDPOINT,
        method,
        params,
        &mut auth,
    )?;
    let mut warnings = group_control_warnings(resolved);
    warnings.extend(sync_group_state(
        resolved,
        manager,
        &record,
        &request.group,
        true,
    ));
    let snapshot = cached_group_snapshot(resolved, &record, &request.group)
        .or_else(|| normalize_group_snapshot(&raw))
        .unwrap_or_else(|| json!({ "group_did": request.group }));
    let members = cached_group_members(resolved, &record, &request.group, 100).unwrap_or_default();
    let mut data = json!({
        "group": snapshot,
        "members": members,
        "delivery": raw,
        "member": {
            "did": member.did,
            "handle": member.handle,
        },
    });
    if action == "add"
        && group_member_mutation_uses_e2ee(
            &request,
            pre_mutation_snapshot.as_ref(),
            data.get("group"),
        )
    {
        let (candidate, e2ee_warnings) =
            add_group_member_e2ee(resolved, manager, &record, &request.group, &request.member);
        warnings.extend(e2ee_warnings);
        if let (Some(e2ee), Some(object)) = (candidate, data.as_object_mut()) {
            object.insert("e2ee".to_string(), Value::Object(e2ee));
        }
    }
    Ok(CommandResult {
        data,
        summary: format!("Updated group membership via {action}"),
        warnings: compact_warnings(&mut warnings),
    })
}

pub fn leave_group(
    resolved: &Resolved,
    manager: &Manager,
    request: GroupLeaveRequest,
) -> Result<CommandResult, MessageError> {
    if request.group.trim().is_empty() {
        return Err(MessageError::GroupRequired);
    }
    let record = require_active_identity(resolved, manager, &request.identity_name)?;
    let cached_snapshot = cached_group_snapshot(resolved, &record, &request.group);
    if cached_snapshot.as_ref().is_some_and(is_active_group_owner) {
        return Err(MessageError::GroupOwnerCannotLeave);
    }
    if request.e2ee
        || cached_snapshot
            .as_ref()
            .is_some_and(group_snapshot_uses_e2ee)
    {
        let (e2ee_result, mut warnings) = leave_group_e2ee(resolved, manager, &record, &request)?;
        return Ok(CommandResult {
            data: json!({
                "delivery": e2ee_result.get("delivery").cloned().unwrap_or(Value::Null),
                "group": request.group,
                "e2ee": e2ee_result,
            }),
            summary: format!("Requested group E2EE leave for {}", request.group),
            warnings: compact_warnings(&mut warnings),
        });
    }
    let mut auth = auth_session(resolved, manager, &record)?;
    let client = Client::new(resolved)?;
    let params = build_group_leave_rpc_params(&record, request.clone())?;
    let raw: Value = client.authenticated_rpc_call_profile(
        Profile::RpcDefault,
        MESSAGE_RPC_ENDPOINT,
        "group.leave",
        params,
        &mut auth,
    )?;
    let mut warnings = group_control_warnings(resolved);
    warnings.extend(mark_cached_group_left(resolved, &record, &request.group));
    Ok(CommandResult {
        data: json!({
            "delivery": raw,
            "group": request.group,
        }),
        summary: format!("Left group {}", request.group),
        warnings: compact_warnings(&mut warnings),
    })
}

pub fn update_group(
    resolved: &Resolved,
    manager: &Manager,
    request: GroupUpdateRequest,
) -> Result<CommandResult, MessageError> {
    if request.group.trim().is_empty() {
        return Err(MessageError::GroupRequired);
    }
    let profile_patch = build_group_profile_patch(
        &request.name,
        &request.description,
        &request.discoverability,
        &request.slug,
        &request.goal,
        &request.rules,
        &request.message_prompt,
        &request.doc_url,
    );
    let policy_patch = build_group_policy_patch(
        &request.admission_mode,
        request.attachments_allowed,
        &request.max_members,
        request.member_max_messages,
        request.member_max_total_chars,
    );
    if profile_patch.is_empty() && policy_patch.is_empty() {
        return Err(MessageError::Internal(
            "group update requires at least one mutable field".to_string(),
        ));
    }
    let record = require_active_identity(resolved, manager, &request.identity_name)?;
    let mut auth = auth_session(resolved, manager, &record)?;
    let client = Client::new(resolved)?;
    let mut responses = Vec::new();
    if !profile_patch.is_empty() {
        let params = build_group_update_profile_rpc_params(&record, &request.group, profile_patch)?;
        let raw: Value = client.authenticated_rpc_call_profile(
            Profile::RpcDefault,
            MESSAGE_RPC_ENDPOINT,
            "group.update_profile",
            params,
            &mut auth,
        )?;
        responses.push(raw);
    }
    if !policy_patch.is_empty() {
        let params = build_group_update_policy_rpc_params(&record, &request.group, policy_patch)?;
        let raw: Value = client.authenticated_rpc_call_profile(
            Profile::RpcDefault,
            MESSAGE_RPC_ENDPOINT,
            "group.update_policy",
            params,
            &mut auth,
        )?;
        responses.push(raw);
    }
    let mut warnings = group_control_warnings(resolved);
    warnings.extend(sync_group_state(
        resolved,
        manager,
        &record,
        &request.group,
        false,
    ));
    let snapshot = cached_group_snapshot(resolved, &record, &request.group)
        .unwrap_or_else(|| json!({ "group_did": request.group }));
    Ok(CommandResult {
        data: json!({
            "group": snapshot,
            "delivery": responses,
        }),
        summary: format!("Updated group {}", request.group),
        warnings: compact_warnings(&mut warnings),
    })
}

pub fn list_groups(
    resolved: &Resolved,
    manager: &Manager,
    request: GroupListRequest,
) -> Result<CommandResult, MessageError> {
    let record = require_active_identity(resolved, manager, &request.identity_name)?;
    let mut auth = auth_session(resolved, manager, &record)?;
    let client = Client::new(resolved)?;
    let params = build_group_list_rpc_params(&record, request);
    let raw: Value = client.authenticated_rpc_call_profile(
        Profile::RpcReadHeavy,
        MESSAGE_RPC_ENDPOINT,
        "group.list",
        params,
        &mut auth,
    )?;
    let groups = values_from_array(raw.get("groups"));
    let total = int_value(raw.get("total"), groups.len() as i64);
    Ok(CommandResult {
        data: json!({
            "groups": groups,
            "total": total,
            "source": group_control_source(&raw),
        }),
        summary: format!("Loaded {total} groups"),
        warnings: group_control_warnings(resolved),
    })
}

pub fn group_members(
    resolved: &Resolved,
    manager: &Manager,
    request: GroupMembersRequest,
) -> Result<CommandResult, MessageError> {
    if request.group.trim().is_empty() {
        return Err(MessageError::GroupRequired);
    }
    let record = require_active_identity(resolved, manager, &request.identity_name)?;
    let mut auth = auth_session(resolved, manager, &record)?;
    let client = Client::new(resolved)?;
    let params = build_group_members_rpc_params(&record, request.clone())?;
    let raw: Value = client.authenticated_rpc_call_profile(
        Profile::RpcReadHeavy,
        MESSAGE_RPC_ENDPOINT,
        "group.list_members",
        params,
        &mut auth,
    )?;
    let mut warnings = group_control_warnings(resolved);
    warnings.extend(persist_group_members(
        resolved,
        &record,
        &request.group,
        &raw,
    ));
    let members = cached_group_members(resolved, &record, &request.group, request.limit)
        .filter(|items| !items.is_empty())
        .unwrap_or_else(|| values_from_array(raw.get("members")));
    let total = int_value(raw.get("total"), members.len() as i64);
    Ok(CommandResult {
        data: json!({
            "group": request.group,
            "members": members,
            "total": total,
            "source": group_control_source(&raw),
        }),
        summary: format!("Loaded {total} group members"),
        warnings: compact_warnings(&mut warnings),
    })
}

pub fn group_messages(
    resolved: &Resolved,
    manager: &Manager,
    request: GroupMessagesRequest,
) -> Result<CommandResult, MessageError> {
    super::group_ws::group_messages(resolved, manager, request)
}

pub fn send_group(
    resolved: &Resolved,
    manager: &Manager,
    request: SendRequest,
) -> Result<CommandResult, MessageError> {
    super::group_ws::send_group(resolved, manager, request)
}

pub(crate) fn sync_group_state(
    resolved: &Resolved,
    manager: &Manager,
    record: &StoredIdentity,
    group_did: &str,
    include_members: bool,
) -> Vec<String> {
    let group_did = group_did.trim();
    if group_did.is_empty() {
        return Vec::new();
    }
    let mut warnings = Vec::new();
    let mut auth = match auth_session(resolved, manager, record) {
        Ok(auth) => auth,
        Err(err) => return vec![format!("Failed to prepare group sync transport: {err}")],
    };
    let client = match Client::new(resolved) {
        Ok(client) => client,
        Err(err) => return vec![format!("Failed to prepare group sync transport: {err}")],
    };
    let group_params = match build_group_get_rpc_params(
        record,
        GroupGetRequest {
            group: group_did.to_string(),
            ..GroupGetRequest::default()
        },
    ) {
        Ok(params) => params,
        Err(err) => return vec![format!("Failed to refresh group snapshot: {err}")],
    };
    match client.authenticated_rpc_call_profile::<Value, _>(
        Profile::RpcReadHeavy,
        MESSAGE_RPC_ENDPOINT,
        "group.get",
        group_params,
        &mut auth,
    ) {
        Ok(raw) => warnings.extend(persist_group_snapshot(resolved, record, &raw)),
        Err(err) => return vec![format!("Failed to refresh group snapshot: {err}")],
    }
    if include_members {
        let member_params = match build_group_members_rpc_params(
            record,
            GroupMembersRequest {
                group: group_did.to_string(),
                limit: 100,
                ..GroupMembersRequest::default()
            },
        ) {
            Ok(params) => params,
            Err(err) => return vec![format!("Failed to refresh group members: {err}")],
        };
        match client.authenticated_rpc_call_profile::<Value, _>(
            Profile::RpcReadHeavy,
            MESSAGE_RPC_ENDPOINT,
            "group.list_members",
            member_params,
            &mut auth,
        ) {
            Ok(raw) => warnings.extend(persist_group_members(resolved, record, group_did, &raw)),
            Err(err) => warnings.push(format!("Failed to refresh group members: {err}")),
        }
    }
    warnings
}

pub(crate) fn group_control_warnings(resolved: &Resolved) -> Vec<String> {
    if runtime_mode(resolved) == crate::runtime::bridge::MODE_WEBSOCKET {
        vec![
            "Group lifecycle commands use HTTP transport even when runtime.mode is websocket."
                .to_string(),
        ]
    } else {
        Vec::new()
    }
}

pub(crate) fn persist_group_snapshot(
    resolved: &Resolved,
    record: &StoredIdentity,
    raw: &Value,
) -> Vec<String> {
    let Some(snapshot) = normalize_group_snapshot(raw) else {
        return Vec::new();
    };
    let mut phase = crate::traceutil::local_db_phase("persist_group_snapshot");
    let result = (|| {
        let group_did = string_value(snapshot.get("group_did"));
        if group_did.trim().is_empty() {
            return Vec::new();
        }
        let Ok(connection) = store::open(&resolved.paths) else {
            return vec!["Failed to open local store for group snapshot".to_string()];
        };
        if let Err(err) = store::ensure_schema(&connection) {
            return vec![format!(
                "Failed to ensure local schema for group snapshot: {err}"
            )];
        }
        let record_to_store = store::GroupRecord {
            owner_did: record.did.clone(),
            group_id: group_storage_key(&group_did),
            group_did: group_did.clone(),
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
            remote_created_at: string_value(snapshot.get("created_at")),
            remote_updated_at: string_value(snapshot.get("updated_at")),
            metadata: metadata_string(snapshot),
            credential_name: record.identity_name.clone(),
            ..store::GroupRecord::default()
        };
        if let Err(err) = store::upsert_group(&connection, record_to_store) {
            return vec![format!("Failed to persist group snapshot: {err}")];
        }
        Vec::new()
    })();
    phase.finish();
    result
}

pub(crate) fn persist_group_members(
    resolved: &Resolved,
    record: &StoredIdentity,
    group_did: &str,
    raw: &Value,
) -> Vec<String> {
    if raw.get("members").is_none() {
        return Vec::new();
    }
    let members = values_from_array(raw.get("members"));
    let mut phase = crate::traceutil::local_db_phase("persist_group_members");
    let result = (|| {
        let Ok(mut connection) = store::open(&resolved.paths) else {
            return vec!["Failed to open local store for group members".to_string()];
        };
        if let Err(err) = store::ensure_schema(&connection) {
            return vec![format!(
                "Failed to ensure local schema for group members: {err}"
            )];
        }
        let records = members
            .iter()
            .filter_map(|member| group_member_record(record, group_did, member))
            .collect::<Vec<_>>();
        if let Err(err) = store::replace_group_members(
            &mut connection,
            &record.did,
            &group_storage_key(group_did),
            &records,
            &record.identity_name,
        ) {
            return vec![format!("Failed to persist group members: {err}")];
        }
        Vec::new()
    })();
    phase.finish();
    result
}

pub(crate) fn persist_group_messages(
    resolved: &Resolved,
    record: &StoredIdentity,
    group_did: &str,
    raw: &Value,
) -> Vec<String> {
    let messages = values_from_array(raw.get("messages"));
    if messages.is_empty() {
        return Vec::new();
    }
    let mut phase = crate::traceutil::local_db_phase("persist_group_messages");
    let result = (|| {
        let Ok(mut connection) = store::open(&resolved.paths) else {
            return vec!["Failed to open local store for group messages".to_string()];
        };
        if let Err(err) = store::ensure_schema(&connection) {
            return vec![format!(
                "Failed to ensure local schema for group messages: {err}"
            )];
        }
        let records = messages
            .iter()
            .filter_map(|message| group_message_record(record, group_did, message))
            .collect::<Vec<_>>();
        if let Err(err) = store::store_messages_batch(&mut connection, &records) {
            return vec![format!("Failed to persist group messages: {err}")];
        }
        if let Some(latest) = messages.first() {
            let _ = store::touch_group_after_message(
                &connection,
                &record.did,
                &group_storage_key(group_did),
                group_did,
                &default_string(
                    &string_value(latest.get("sent_at")),
                    &string_value(latest.get("created_at")),
                ),
                i64_option(raw.get("next_since_seq")),
                &record.identity_name,
                &metadata_string(json!({ "source": "group.list_messages" })),
            );
        }
        Vec::new()
    })();
    phase.finish();
    result
}

pub(crate) fn persist_group_send_result(
    resolved: &Resolved,
    record: &StoredIdentity,
    request: &SendRequest,
    message_type: &str,
    result: &GroupSendResult,
) -> Vec<String> {
    let mut phase = crate::traceutil::local_db_phase("persist_group_send");
    let result = (|| {
        let mut warnings = Vec::new();
        let Ok(connection) = store::open(&resolved.paths) else {
            return vec!["Failed to open local store for group send".to_string()];
        };
        if let Err(err) = store::ensure_schema(&connection) {
            return vec![format!(
                "Failed to ensure local schema for group send: {err}"
            )];
        }
        let message_id = group_send_message_id(&request.group, result);
        if let Err(err) = store::store_message(
            &connection,
            MessageRecord {
                msg_id: message_id,
                owner_did: record.did.clone(),
                thread_id: store::make_thread_id(
                    &record.did,
                    "",
                    &group_storage_key(&request.group),
                ),
                direction: 1,
                sender_did: record.did.clone(),
                group_id: group_storage_key(&request.group),
                group_did: request.group.clone(),
                content_type: content_type_for_message_type(message_type).to_string(),
                content: request.text.clone(),
                sent_at: result.accepted_at.clone(),
                is_read: true,
                metadata: metadata_string(json!({
                    "group_event_seq": result.group_event_seq,
                    "group_state_version": result.group_state_version,
                    "operation_id": result.operation_id,
                })),
                credential_name: record.identity_name.clone(),
                ..MessageRecord::default()
            },
        ) {
            warnings.push(format!("Failed to persist local group message: {err}"));
        }
        let mut touch_phase = crate::traceutil::local_db_phase("touch_group_cache");
        let touch_result = store::touch_group_after_message(
            &connection,
            &record.did,
            &group_storage_key(&request.group),
            &request.group,
            &result.accepted_at,
            i64_option(Some(&Value::String(result.group_event_seq.clone()))),
            &record.identity_name,
            &metadata_string(json!({ "group_state_version": result.group_state_version })),
        );
        touch_phase.finish();
        if let Err(err) = touch_result {
            warnings.push(format!("Failed to update group cache: {err}"));
        }
        warnings
    })();
    phase.finish();
    result
}

pub(crate) fn mark_cached_group_left(
    resolved: &Resolved,
    record: &StoredIdentity,
    group_did: &str,
) -> Vec<String> {
    let mut phase = crate::traceutil::local_db_phase("mark_group_left");
    let result = (|| {
        let Ok(mut connection) = store::open(&resolved.paths) else {
            return vec!["Failed to open local store for leave projection".to_string()];
        };
        if let Err(err) = store::ensure_schema(&connection) {
            return vec![format!(
                "Failed to ensure local schema for leave projection: {err}"
            )];
        }
        let group_key = group_storage_key(group_did);
        let mut warnings = Vec::new();
        if let Err(err) = store::mark_group_left(
            &mut connection,
            &record.did,
            &group_key,
            group_did,
            &record.identity_name,
        ) {
            warnings.push(format!("Failed to update local group leave status: {err}"));
        }
        warnings
    })();
    phase.finish();
    result
}

pub(crate) fn cached_group_snapshot(
    resolved: &Resolved,
    record: &StoredIdentity,
    group_did: &str,
) -> Option<Value> {
    let mut phase = crate::traceutil::local_db_phase("read_group_snapshot_cache");
    let result = (|| {
        let connection = store::open(&resolved.paths).ok()?;
        store::ensure_schema(&connection).ok()?;
        store::get_group_snapshot(&connection, &record.did, &group_storage_key(group_did))
            .ok()
            .map(enrich_cached_group_snapshot)
    })();
    phase.finish();
    result
}

pub(crate) fn cached_group_members(
    resolved: &Resolved,
    record: &StoredIdentity,
    group_did: &str,
    limit: i64,
) -> Option<Vec<Value>> {
    let mut phase = crate::traceutil::local_db_phase("read_group_members_cache");
    let result = (|| {
        let connection = store::open(&resolved.paths).ok()?;
        store::ensure_schema(&connection).ok()?;
        store::list_cached_group_members(
            &connection,
            &record.did,
            &group_storage_key(group_did),
            limit,
        )
        .ok()
    })();
    phase.finish();
    result
}

pub(crate) fn cached_group_messages(
    resolved: &Resolved,
    record: &StoredIdentity,
    group_did: &str,
    limit: i64,
    cursor: &str,
) -> Option<Vec<Value>> {
    let mut phase = crate::traceutil::local_db_phase("read_group_messages_cache");
    let result = (|| {
        let connection = store::open(&resolved.paths).ok()?;
        store::ensure_schema(&connection).ok()?;
        store::list_group_messages(
            &connection,
            &record.did,
            &group_storage_key(group_did),
            limit,
            i64_option(Some(&Value::String(cursor.to_string()))),
        )
        .ok()
    })();
    phase.finish();
    result
}

fn group_member_record(
    record: &StoredIdentity,
    group_did: &str,
    member: &Value,
) -> Option<store::GroupMemberRecord> {
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
    Some(store::GroupMemberRecord {
        owner_did: record.did.clone(),
        group_id: group_storage_key(group_did),
        user_id: member_did.clone(),
        member_did,
        member_handle,
        role: string_value(member.get("role")),
        status: string_value(member.get("status")),
        joined_at: string_value(member.get("joined_at")),
        metadata: metadata_string(member.clone()),
        credential_name: record.identity_name.clone(),
        ..store::GroupMemberRecord::default()
    })
}

fn group_message_record(
    record: &StoredIdentity,
    group_did: &str,
    message: &Value,
) -> Option<MessageRecord> {
    let msg_id = default_string(
        &string_value(message.get("id")),
        &string_value(message.get("message_id")),
    );
    if msg_id.trim().is_empty() {
        return None;
    }
    let sender_did = string_value(message.get("sender_did"));
    let content_type = default_string(
        &string_value(message.get("content_type")),
        &infer_group_message_content_type(message),
    );
    Some(MessageRecord {
        msg_id,
        owner_did: record.did.clone(),
        thread_id: store::make_thread_id(&record.did, "", &group_storage_key(group_did)),
        direction: if sender_did == record.did { 1 } else { 0 },
        sender_did,
        group_id: group_storage_key(group_did),
        group_did: group_did.to_string(),
        content_type,
        content: content_string(message.get("content")),
        server_seq: i64_option(message.get("server_seq")),
        sent_at: default_string(
            &string_value(message.get("sent_at")),
            &string_value(message.get("created_at")),
        ),
        is_read: bool_value(message.get("is_read")),
        metadata: metadata_string(message.clone()),
        credential_name: record.identity_name.clone(),
        ..MessageRecord::default()
    })
}

pub(crate) fn normalize_group_snapshot(raw: &Value) -> Option<Value> {
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
        return Some(json!({
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

fn enrich_cached_group_snapshot(mut snapshot: Value) -> Value {
    let metadata = snapshot
        .get("metadata")
        .and_then(Value::as_str)
        .and_then(|value| serde_json::from_str::<Value>(value).ok())
        .and_then(|value| normalize_group_snapshot(&value));
    if let (Some(object), Some(Value::Object(metadata_object))) =
        (snapshot.as_object_mut(), metadata)
    {
        for (key, value) in metadata_object {
            object.entry(key).or_insert(value);
        }
    }
    if let Some(object) = snapshot.as_object_mut() {
        let group_did = string_value(object.get("group_did"));
        if !group_did.trim().is_empty() {
            object
                .entry("did".to_string())
                .or_insert(Value::String(group_did));
        }
        let my_role = string_value(object.get("my_role"));
        if !my_role.trim().is_empty() {
            object
                .entry("member_role".to_string())
                .or_insert(Value::String(my_role));
        }
        let status = string_value(object.get("membership_status"));
        if !status.trim().is_empty() {
            object
                .entry("member_status".to_string())
                .or_insert(Value::String(status));
        }
        if !object.contains_key("group_profile") {
            let mut profile = Map::new();
            insert_from_object(object, &mut profile, "display_name", "name");
            insert_from_object(object, &mut profile, "description", "description");
            insert_from_object(object, &mut profile, "slug", "slug");
            insert_from_object(object, &mut profile, "goal", "goal");
            insert_from_object(object, &mut profile, "rules", "rules");
            insert_from_object(object, &mut profile, "message_prompt", "message_prompt");
            insert_from_object(object, &mut profile, "doc_url", "doc_url");
            if !profile.is_empty() {
                object.insert("group_profile".to_string(), Value::Object(profile));
            }
        }
    }
    snapshot
}

fn insert_from_object(
    source: &Map<String, Value>,
    target: &mut Map<String, Value>,
    target_key: &str,
    source_key: &str,
) {
    if let Some(value) = source.get(source_key).filter(|value| !value.is_null()) {
        target.insert(target_key.to_string(), value.clone());
    }
}

pub(crate) fn values_from_array(value: Option<&Value>) -> Vec<Value> {
    value.and_then(Value::as_array).cloned().unwrap_or_default()
}

pub(crate) fn group_did_from_result(raw: &Value) -> String {
    string_value(raw.get("group_did"))
        .trim()
        .to_string()
        .or_else_nonempty(|| string_value(raw.get("did")))
}

trait NonEmptyString {
    fn or_else_nonempty(self, fallback: impl FnOnce() -> String) -> String;
}

impl NonEmptyString for String {
    fn or_else_nonempty(self, fallback: impl FnOnce() -> String) -> String {
        if self.trim().is_empty() {
            fallback()
        } else {
            self
        }
    }
}

pub(crate) fn group_send_message_id(group_did: &str, result: &GroupSendResult) -> String {
    if !result.group_did.trim().is_empty() && !result.group_event_seq.trim().is_empty() {
        return format!(
            "{}:{}",
            result.group_did.trim(),
            result.group_event_seq.trim()
        );
    }
    if !result.group_event_seq.trim().is_empty() {
        return format!("{}:{}", group_did.trim(), result.group_event_seq.trim());
    }
    if !result.message_id.trim().is_empty() {
        return result.message_id.clone();
    }
    format!("msg-{}", crate::message::wire::generate_operation_id())
}

fn build_group_profile_patch(
    name: &str,
    description: &str,
    discoverability: &str,
    slug: &str,
    goal: &str,
    rules: &str,
    message_prompt: &str,
    doc_url: &str,
) -> Map<String, Value> {
    let mut patch = Map::new();
    insert_trimmed_string(&mut patch, "display_name", name);
    insert_trimmed_string(&mut patch, "description", description);
    insert_trimmed_string(&mut patch, "discoverability", discoverability);
    insert_trimmed_string(&mut patch, "slug", slug);
    insert_trimmed_string(&mut patch, "goal", goal);
    insert_trimmed_string(&mut patch, "rules", rules);
    insert_trimmed_string(&mut patch, "message_prompt", message_prompt);
    insert_trimmed_string(&mut patch, "doc_url", doc_url);
    patch
}

fn build_group_policy_patch(
    admission_mode: &str,
    attachments_allowed: Option<bool>,
    max_members: &str,
    member_max_messages: Option<i64>,
    member_max_total_chars: Option<i64>,
) -> Map<String, Value> {
    let mut patch = Map::new();
    insert_trimmed_string(&mut patch, "admission_mode", admission_mode);
    if let Some(value) = attachments_allowed {
        patch.insert("attachments_allowed".to_string(), Value::Bool(value));
    }
    insert_trimmed_string(&mut patch, "max_members", max_members);
    if let Some(value) = member_max_messages {
        patch.insert("member_max_messages".to_string(), json!(value));
    }
    if let Some(value) = member_max_total_chars {
        patch.insert("member_max_total_chars".to_string(), json!(value));
    }
    if patch.is_empty() {
        return patch;
    }
    patch.insert(
        "message_security_profile".to_string(),
        Value::String("transport-protected".to_string()),
    );
    patch.insert(
        "bootstrap_security_profile".to_string(),
        Value::String("transport-protected".to_string()),
    );
    patch.insert(
        "permissions".to_string(),
        json!({
            "send": "member",
            "add": "admin",
            "remove": "admin",
            "update_profile": "admin",
            "update_policy": "owner",
        }),
    );
    patch
}

fn insert_trimmed_string(patch: &mut Map<String, Value>, key: &str, value: &str) {
    let value = value.trim();
    if !value.is_empty() {
        patch.insert(key.to_string(), Value::String(value.to_string()));
    }
}

pub(crate) fn is_active_group_owner(snapshot: &Value) -> bool {
    let role = default_string(
        &string_value(snapshot.get("my_role")),
        &string_value(snapshot.get("member_role")),
    );
    let status = default_string(
        &string_value(snapshot.get("membership_status")),
        &string_value(snapshot.get("member_status")),
    );
    role == "owner" && status == "active"
}

pub(crate) fn infer_group_message_content_type(message: &Value) -> String {
    let subject_method = message
        .get("system_event")
        .and_then(|event| event.get("subject_method"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    match subject_method {
        "group.join" | "group.add" => "group_system_member_joined".to_string(),
        "group.leave" => "group_system_member_left".to_string(),
        "group.remove" => "group_system_member_kicked".to_string(),
        _ if message.get("system_event").is_some() => "application/json".to_string(),
        _ => "text/plain".to_string(),
    }
}

pub(crate) fn group_storage_key(group_did: &str) -> String {
    group_did.trim().to_string()
}

pub(crate) fn group_control_source(raw: &Value) -> String {
    string_value(raw.get("source")).or_else_nonempty(|| "remote_http".to_string())
}

pub(crate) fn default_string(value: &str, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

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

pub(crate) fn i64_option(value: Option<&Value>) -> Option<i64> {
    match value {
        Some(Value::Number(number)) => number
            .as_i64()
            .or_else(|| number.as_u64().and_then(|value| i64::try_from(value).ok()))
            .or_else(|| number.as_f64().map(|value| value as i64)),
        Some(Value::String(value)) => value.trim().parse::<i64>().ok(),
        _ => None,
    }
}

pub(crate) fn compact_warnings(warnings: &mut Vec<String>) -> Vec<String> {
    let mut compact = Vec::new();
    for warning in warnings.drain(..) {
        let warning = warning.trim().to_string();
        if warning.is_empty() || compact.iter().any(|known| known == &warning) {
            continue;
        }
        compact.push(warning);
    }
    compact
}
