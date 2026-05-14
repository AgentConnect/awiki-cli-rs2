use crate::identity::types::StoredIdentity;
use crate::message::types::{
    GroupCreateRequest, GroupGetRequest, GroupInfoRequest, GroupJoinRequest, GroupLeaveRequest,
    GroupListRequest, GroupMemberRequest, GroupMembersRequest, GroupMessagesRequest, MessageError,
};
use crate::message::{
    build_origin_proof, content_type_for_message_type, origin_auth_value, DirectPayload,
};
use serde_json::{json, Map, Value};

pub const GROUP_E2EE_PROFILE: &str = "anp.group.e2ee.v1";
pub const GROUP_E2EE_SECURITY_PROFILE: &str = "group-e2ee";
pub const GROUP_E2EE_TRANSPORT_PROFILE: &str = "transport-protected";

pub fn build_group_create_rpc_params(
    record: &StoredIdentity,
    service_did: &str,
    request: GroupCreateRequest,
) -> Result<Value, MessageError> {
    if service_did.trim().is_empty() {
        return Err(MessageError::MissingMessageServiceDid);
    }

    let profile = build_group_profile_patch(
        &request.name,
        &request.description,
        &request.discoverability,
        &request.slug,
        &request.goal,
        &request.rules,
        &request.message_prompt,
        &request.doc_url,
    );
    if !profile.contains_key("display_name") {
        return Err(MessageError::Json(
            "group display name is required".to_string(),
        ));
    }

    let mut policy = build_group_policy_patch(
        &request.admission_mode,
        request.attachments_allowed,
        &request.max_members,
        request.member_max_messages,
        request.member_max_total_chars,
    );
    if policy.is_empty() {
        policy = build_group_policy_patch("open-join", Some(true), "500", None, None);
    }
    if let Some(security_profile) = normalized_group_security_profile(&request) {
        policy.insert(
            "message_security_profile".to_string(),
            Value::String(security_profile.clone()),
        );
        policy.insert(
            "bootstrap_security_profile".to_string(),
            Value::String(security_profile),
        );
    }

    let body = json!({
        "group_profile": profile,
        "group_policy": policy,
    });
    let payload = DirectPayload {
        method: "group.create".to_string(),
        meta: signed_group_meta(
            &record.did,
            "service",
            service_did,
            "application/json",
            false,
        ),
        body,
    };
    signed_params(record, payload)
}

pub fn build_group_get_info_rpc_params(
    record: &StoredIdentity,
    request: GroupInfoRequest,
) -> Result<Value, MessageError> {
    let group_did = request.group.trim();
    if group_did.is_empty() {
        return Err(MessageError::GroupRequired);
    }

    let mut body = Map::new();
    if request.include_policy {
        body.insert("include_policy".to_string(), Value::Bool(true));
    }
    if request.include_member_list {
        body.insert("include_member_list".to_string(), Value::Bool(true));
    }

    Ok(json!({
        "meta": group_base_meta(&record.did, Some(("group", group_did))),
        "body": body,
    }))
}

pub fn build_group_join_rpc_params(
    record: &StoredIdentity,
    request: GroupJoinRequest,
) -> Result<Value, MessageError> {
    let mut body = Map::new();
    let reason_text = request.reason_text.trim();
    if !reason_text.is_empty() {
        body.insert(
            "reason_text".to_string(),
            Value::String(reason_text.to_string()),
        );
    }
    build_group_mutation_rpc_params(record, &request.group, "group.join", Value::Object(body))
}

pub fn build_group_add_rpc_params(
    record: &StoredIdentity,
    request: GroupMemberRequest,
) -> Result<Value, MessageError> {
    let member_did = request.member.trim();
    if member_did.is_empty() {
        return Err(MessageError::MemberRequired);
    }

    let mut body = Map::new();
    body.insert(
        "member_did".to_string(),
        Value::String(member_did.to_string()),
    );
    let role = request.role.trim();
    if !role.is_empty() {
        body.insert("role".to_string(), Value::String(role.to_string()));
    }
    let reason_text = request.reason_text.trim();
    if !reason_text.is_empty() {
        body.insert(
            "reason_text".to_string(),
            Value::String(reason_text.to_string()),
        );
    }

    build_group_mutation_rpc_params(record, &request.group, "group.add", Value::Object(body))
}

pub fn build_group_remove_rpc_params(
    record: &StoredIdentity,
    request: GroupMemberRequest,
) -> Result<Value, MessageError> {
    let member_did = request.member.trim();
    if member_did.is_empty() {
        return Err(MessageError::MemberRequired);
    }

    let mut body = Map::new();
    body.insert(
        "member_did".to_string(),
        Value::String(member_did.to_string()),
    );
    let reason_text = request.reason_text.trim();
    if !reason_text.is_empty() {
        body.insert(
            "reason_text".to_string(),
            Value::String(reason_text.to_string()),
        );
    }

    build_group_mutation_rpc_params(record, &request.group, "group.remove", Value::Object(body))
}

pub fn build_group_leave_rpc_params(
    record: &StoredIdentity,
    request: GroupLeaveRequest,
) -> Result<Value, MessageError> {
    build_group_mutation_rpc_params(record, &request.group, "group.leave", json!({}))
}

pub fn build_group_update_profile_rpc_params(
    record: &StoredIdentity,
    group_did: &str,
    patch: Map<String, Value>,
) -> Result<Value, MessageError> {
    if patch.is_empty() {
        return Err(MessageError::Json(
            "group profile patch is required".to_string(),
        ));
    }
    build_group_mutation_rpc_params(
        record,
        group_did,
        "group.update_profile",
        json!({ "group_profile_patch": patch }),
    )
}

pub fn build_group_update_policy_rpc_params(
    record: &StoredIdentity,
    group_did: &str,
    patch: Map<String, Value>,
) -> Result<Value, MessageError> {
    if patch.is_empty() {
        return Err(MessageError::Json(
            "group policy patch is required".to_string(),
        ));
    }
    build_group_mutation_rpc_params(
        record,
        group_did,
        "group.update_policy",
        json!({ "group_policy_patch": patch }),
    )
}

pub fn build_group_send_rpc_params(
    record: &StoredIdentity,
    group_did: &str,
    text: &str,
    message_type: &str,
) -> Result<Value, MessageError> {
    if group_did.trim().is_empty() {
        return Err(MessageError::GroupRequired);
    }
    if text.trim().is_empty() {
        return Err(MessageError::TextRequired);
    }

    let body = json!({ "text": text });
    let payload = DirectPayload {
        method: "group.send".to_string(),
        meta: signed_group_meta(
            &record.did,
            "group",
            group_did,
            content_type_for_message_type(message_type),
            true,
        ),
        body,
    };
    signed_params(record, payload)
}

pub fn build_group_get_rpc_params(
    record: &StoredIdentity,
    request: GroupGetRequest,
) -> Result<Value, MessageError> {
    let group_did = request.group.trim();
    if group_did.is_empty() {
        return Err(MessageError::GroupRequired);
    }
    Ok(json!({
        "meta": group_local_meta(&record.did, Some(group_did)),
        "body": {
            "group_did": group_did,
        },
    }))
}

pub fn build_group_list_rpc_params(record: &StoredIdentity, request: GroupListRequest) -> Value {
    let limit = if request.limit <= 0 {
        50
    } else {
        request.limit
    };
    json!({
        "meta": group_local_meta(&record.did, None),
        "body": {
            "limit": limit,
        },
    })
}

pub fn build_group_members_rpc_params(
    record: &StoredIdentity,
    request: GroupMembersRequest,
) -> Result<Value, MessageError> {
    let group_did = request.group.trim();
    if group_did.is_empty() {
        return Err(MessageError::GroupRequired);
    }
    let limit = if request.limit <= 0 {
        100
    } else {
        request.limit
    };
    Ok(json!({
        "meta": group_local_meta(&record.did, Some(group_did)),
        "body": {
            "group_did": group_did,
            "limit": limit,
        },
    }))
}

pub fn build_group_messages_rpc_params(
    record: &StoredIdentity,
    request: GroupMessagesRequest,
) -> Result<Value, MessageError> {
    let group_did = request.group.trim();
    if group_did.is_empty() {
        return Err(MessageError::GroupRequired);
    }
    let limit = if request.limit <= 0 {
        50
    } else {
        request.limit
    };
    let mut body = Map::new();
    body.insert(
        "group_did".to_string(),
        Value::String(group_did.to_string()),
    );
    body.insert("limit".to_string(), json!(limit));
    let cursor = request.cursor.trim();
    if !cursor.is_empty() {
        body.insert("since_seq".to_string(), Value::String(cursor.to_string()));
    }
    if request.skip > 0 {
        body.insert("skip".to_string(), json!(request.skip));
    }
    Ok(json!({
        "meta": group_local_meta(&record.did, Some(group_did)),
        "body": body,
    }))
}

fn build_group_mutation_rpc_params(
    record: &StoredIdentity,
    group_did: &str,
    method: &str,
    body: Value,
) -> Result<Value, MessageError> {
    let group_did = group_did.trim();
    if group_did.is_empty() {
        return Err(MessageError::GroupRequired);
    }
    let payload = DirectPayload {
        method: method.to_string(),
        meta: signed_group_meta(&record.did, "group", group_did, "application/json", false),
        body,
    };
    signed_params(record, payload)
}

fn signed_params(record: &StoredIdentity, payload: DirectPayload) -> Result<Value, MessageError> {
    let origin_proof = build_origin_proof(record, &payload)?;
    Ok(json!({
        "meta": payload.meta,
        "auth": origin_auth_value(&origin_proof),
        "body": payload.body,
    }))
}

fn signed_group_meta(
    sender_did: &str,
    target_kind: &str,
    target_did: &str,
    content_type: &str,
    include_message_id: bool,
) -> Value {
    let mut meta = match group_base_meta(sender_did, Some((target_kind, target_did))) {
        Value::Object(meta) => meta,
        _ => Map::new(),
    };
    meta.insert(
        "operation_id".to_string(),
        Value::String(format!(
            "op-{}",
            crate::message::wire::generate_operation_id()
        )),
    );
    if include_message_id {
        meta.insert(
            "message_id".to_string(),
            Value::String(format!(
                "msg-{}",
                crate::message::wire::generate_operation_id()
            )),
        );
    }
    meta.insert(
        "created_at".to_string(),
        Value::String(crate::message::wire::now_rfc3339()),
    );
    meta.insert(
        "content_type".to_string(),
        Value::String(content_type.to_string()),
    );
    Value::Object(meta)
}

fn group_base_meta(sender_did: &str, target: Option<(&str, &str)>) -> Value {
    let mut meta = Map::new();
    meta.insert("anp_version".to_string(), Value::String("1.0".to_string()));
    meta.insert(
        "profile".to_string(),
        Value::String("anp.group.base.v1".to_string()),
    );
    meta.insert(
        "security_profile".to_string(),
        Value::String("transport-protected".to_string()),
    );
    meta.insert(
        "sender_did".to_string(),
        Value::String(sender_did.to_string()),
    );
    if let Some((kind, did)) = target {
        meta.insert(
            "target".to_string(),
            json!({
                "kind": kind,
                "did": did,
            }),
        );
    }
    Value::Object(meta)
}

fn group_local_meta(sender_did: &str, group_did: Option<&str>) -> Value {
    let mut meta = Map::new();
    meta.insert("anp_version".to_string(), Value::String("1.0".to_string()));
    meta.insert(
        "profile".to_string(),
        Value::String("anp.group.local.v1".to_string()),
    );
    meta.insert(
        "security_profile".to_string(),
        Value::String("transport-protected".to_string()),
    );
    meta.insert(
        "sender_did".to_string(),
        Value::String(sender_did.to_string()),
    );
    if let Some(group_did) = group_did {
        meta.insert(
            "target".to_string(),
            json!({
                "kind": "group",
                "did": group_did,
            }),
        );
    }
    Value::Object(meta)
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
        Value::String(GROUP_E2EE_TRANSPORT_PROFILE.to_string()),
    );
    patch.insert(
        "bootstrap_security_profile".to_string(),
        Value::String(GROUP_E2EE_TRANSPORT_PROFILE.to_string()),
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

fn normalized_group_security_profile(request: &GroupCreateRequest) -> Option<String> {
    if request.e2ee {
        return Some(GROUP_E2EE_SECURITY_PROFILE.to_string());
    }
    let value = request.message_security_profile.trim();
    match value {
        "" | GROUP_E2EE_TRANSPORT_PROFILE => None,
        GROUP_E2EE_SECURITY_PROFILE => Some(GROUP_E2EE_SECURITY_PROFILE.to_string()),
        _ => Some(value.to_string()),
    }
}

fn insert_trimmed_string(patch: &mut Map<String, Value>, key: &str, value: &str) {
    let value = value.trim();
    if !value.is_empty() {
        patch.insert(key.to_string(), Value::String(value.to_string()));
    }
}
