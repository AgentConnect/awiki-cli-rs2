use serde_json::{json, Map, Value};

use super::direct::DirectPayload;

pub(crate) fn build_group_send_payload(
    sender_did: &str,
    group_did: &str,
    text: &str,
    content_type: &str,
) -> crate::ImResult<DirectPayload> {
    let group_did = group_did.trim();
    if group_did.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("group_did".to_string()),
            "group target is required",
        ));
    }
    if text.trim().is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("text".to_string()),
            "message text is required",
        ));
    }

    Ok(DirectPayload {
        method: "group.send".to_string(),
        meta: signed_group_meta(sender_did, "group", group_did, content_type, true),
        body: json!({ "text": text }),
    })
}

pub(crate) fn build_group_json_send_payload(
    sender_did: &str,
    group_did: &str,
    payload: Value,
) -> crate::ImResult<DirectPayload> {
    let group_did = group_did.trim();
    if group_did.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("group_did".to_string()),
            "group target is required",
        ));
    }
    if !payload.is_object() {
        return Err(crate::ImError::invalid_input(
            Some("payload".to_string()),
            "message payload must be a JSON object",
        ));
    }

    Ok(DirectPayload {
        method: "group.send".to_string(),
        meta: signed_group_meta(sender_did, "group", group_did, "application/json", true),
        body: json!({ "payload": payload }),
    })
}

pub(crate) fn build_group_create_payload(
    sender_did: &str,
    request: &crate::groups::GroupCreateRequest,
    service_did: &crate::ids::Did,
) -> crate::ImResult<DirectPayload> {
    let mut profile = Map::new();
    insert_required_trimmed_string(&mut profile, "display_name", &request.name, "name")?;
    insert_optional_trimmed_string(&mut profile, "description", request.description.as_deref());
    insert_optional_trimmed_string(&mut profile, "avatar_uri", request.avatar_uri.as_deref());
    insert_optional_trimmed_string(
        &mut profile,
        "discoverability",
        request.discoverability.as_ref().map(|value| value.as_str()),
    );
    insert_optional_trimmed_string(&mut profile, "slug", request.slug.as_deref());
    insert_optional_trimmed_string(&mut profile, "goal", request.goal.as_deref());
    insert_optional_trimmed_string(&mut profile, "rules", request.rules.as_deref());
    insert_optional_trimmed_string(
        &mut profile,
        "message_prompt",
        request.message_prompt.as_deref(),
    );
    insert_optional_trimmed_string(&mut profile, "doc_url", request.doc_url.as_deref());

    let mut policy = group_policy_patch(
        request.admission_mode.as_ref(),
        request.attachments_allowed,
        request.max_members,
        request.member_max_messages,
        request.member_max_total_chars,
    );
    if policy.is_empty() {
        policy.insert(
            "admission_mode".to_string(),
            Value::String("open-join".to_string()),
        );
        policy.insert("attachments_allowed".to_string(), Value::Bool(true));
        policy.insert("max_members".to_string(), Value::String("500".to_string()));
        enrich_group_policy_defaults(&mut policy);
    }
    if let Some(security_profile) = normalized_security_profile(
        request.e2ee || request.security.required(),
        request.message_security_profile.as_ref(),
    ) {
        policy.insert(
            "message_security_profile".to_string(),
            Value::String(security_profile.clone()),
        );
        policy.insert(
            "bootstrap_security_profile".to_string(),
            Value::String(security_profile),
        );
    }

    let mut body = Map::new();
    body.insert("group_profile".to_string(), Value::Object(profile));
    body.insert("group_policy".to_string(), Value::Object(policy));
    insert_optional_trimmed_string(
        &mut body,
        "creator_handle",
        request
            .creator_handle
            .as_ref()
            .map(crate::ids::Handle::as_str),
    );
    Ok(DirectPayload {
        method: "group.create".to_string(),
        meta: signed_group_meta(
            sender_did,
            "service",
            service_did.as_str(),
            "application/json",
            false,
        ),
        body: Value::Object(body),
    })
}

pub(crate) fn build_group_join_payload(
    sender_did: &str,
    request: &crate::groups::GroupJoinRequest,
) -> crate::ImResult<DirectPayload> {
    let mut body = Map::new();
    insert_optional_trimmed_string(
        &mut body,
        "member_handle",
        request
            .member_handle
            .as_ref()
            .map(crate::ids::Handle::as_str),
    );
    insert_optional_trimmed_string(&mut body, "reason_text", request.reason_text.as_deref());
    build_group_lifecycle_payload(
        sender_did,
        request.group.as_str(),
        "group.join",
        Value::Object(body),
    )
}

pub(crate) fn build_group_leave_payload(
    sender_did: &str,
    request: &crate::groups::GroupLeaveRequest,
) -> crate::ImResult<DirectPayload> {
    let mut body = group_security_body(request.security.required());
    insert_optional_trimmed_string(&mut body, "reason_text", request.reason_text.as_deref());
    build_group_lifecycle_payload(
        sender_did,
        request.group.as_str(),
        "group.leave",
        Value::Object(body),
    )
}

pub(crate) fn build_group_add_member_payload(
    sender_did: &str,
    request: &crate::groups::GroupMemberMutationRequest,
) -> crate::ImResult<DirectPayload> {
    let mut body = Map::new();
    let field = if request.member.is_did() {
        "member_did"
    } else {
        "member_handle"
    };
    body.insert(
        field.to_string(),
        Value::String(request.member.as_str().to_string()),
    );
    insert_optional_trimmed_string(
        &mut body,
        "role",
        request.role.as_ref().map(|role| role.as_str()),
    );
    insert_optional_trimmed_string(&mut body, "reason_text", request.reason_text.as_deref());
    insert_group_security(&mut body, request.security.required());
    build_group_lifecycle_payload(
        sender_did,
        request.group.as_str(),
        "group.add",
        Value::Object(body),
    )
}

pub(crate) fn build_group_rebind_member_payload(
    sender_did: &str,
    request: &crate::groups::GroupRebindMemberRequest,
) -> crate::ImResult<DirectPayload> {
    if sender_did != request.new_member_did.as_str() {
        return Err(crate::ImError::invalid_input(
            Some("new_member_did".to_string()),
            "group rebind must be initiated by the new DID",
        ));
    }
    let generation = request.handle_binding_generation.as_str();
    if generation.is_empty()
        || generation == "0"
        || generation.starts_with('0')
        || !generation.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(crate::ImError::invalid_input(
            Some("handle_binding_generation".to_string()),
            "handle binding generation must be a canonical positive decimal string",
        ));
    }
    build_group_lifecycle_payload(
        sender_did,
        request.group.as_str(),
        "group.rebind_member",
        json!({
            "member_handle": request.member_handle.as_str(),
            "previous_member_did": request.previous_member_did.as_str(),
            "new_member_did": request.new_member_did.as_str(),
            "handle_binding_generation": generation,
        }),
    )
}

pub(crate) fn build_group_remove_member_payload(
    sender_did: &str,
    request: &crate::groups::GroupMemberMutationRequest,
) -> crate::ImResult<DirectPayload> {
    let mut body = Map::new();
    let member = request.member.as_did()?;
    body.insert(
        "member_did".to_string(),
        Value::String(member.as_str().to_string()),
    );
    insert_optional_trimmed_string(&mut body, "reason_text", request.reason_text.as_deref());
    insert_group_security(&mut body, request.security.required());
    build_group_lifecycle_payload(
        sender_did,
        request.group.as_str(),
        "group.remove",
        Value::Object(body),
    )
}

pub(crate) fn build_group_update_profile_payload(
    sender_did: &str,
    request: &crate::groups::GroupUpdateProfileRequest,
) -> crate::ImResult<DirectPayload> {
    let patch = group_profile_patch(&request.patch);
    build_group_update_profile_patch_payload(sender_did, request.group.as_str(), patch)
}

pub(crate) fn build_group_update_profile_patch_payload(
    sender_did: &str,
    group_did: &str,
    patch: Map<String, Value>,
) -> crate::ImResult<DirectPayload> {
    if patch.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("profile_patch".to_string()),
            "group profile patch is required",
        ));
    }
    build_group_lifecycle_payload(
        sender_did,
        group_did,
        "group.update_profile",
        json!({ "group_profile_patch": patch }),
    )
}

pub(crate) fn build_group_update_policy_payload(
    sender_did: &str,
    request: &crate::groups::GroupUpdatePolicyRequest,
) -> crate::ImResult<DirectPayload> {
    let patch = group_policy_patch(
        request.patch.admission_mode.as_ref(),
        request.patch.attachments_allowed,
        request.patch.max_members,
        request.patch.member_max_messages,
        request.patch.member_max_total_chars,
    );
    build_group_update_policy_patch_payload(sender_did, request.group.as_str(), patch)
}

pub(crate) fn build_group_update_policy_patch_payload(
    sender_did: &str,
    group_did: &str,
    patch: Map<String, Value>,
) -> crate::ImResult<DirectPayload> {
    if patch.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("policy_patch".to_string()),
            "group policy patch is required",
        ));
    }
    build_group_lifecycle_payload(
        sender_did,
        group_did,
        "group.update_policy",
        json!({ "group_policy_patch": patch }),
    )
}

pub(crate) fn build_group_get_rpc_params(
    sender_did: &str,
    group_did: &str,
) -> crate::ImResult<Value> {
    let group_did = require_group(group_did)?;
    Ok(json!({
        "meta": group_local_meta(sender_did, Some(group_did)),
        "body": {
            "group_did": group_did,
        },
    }))
}

pub(crate) fn build_group_get_info_rpc_params(
    sender_did: &str,
    group_did: &str,
    operation_id: &str,
    include_policy: bool,
) -> crate::ImResult<Value> {
    let group_did = require_group(group_did)?;
    let operation_id = operation_id.trim();
    if operation_id.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("operation_id".to_string()),
            "operation id is required",
        ));
    }
    Ok(json!({
        "meta": {
            "anp_version": "2.0",
            "profile": "anp.group.base.v2",
            "security_profile": "transport-protected",
            "sender_did": sender_did,
            "target": { "kind": "group", "did": group_did },
            "operation_id": format!("p4-group-info-{operation_id}"),
            "content_type": "application/json",
            "created_at": super::common::now_rfc3339()
        },
        "body": {
            "include_policy": include_policy,
            "include_member_list": false
        }
    }))
}

pub(crate) fn build_group_list_rpc_params(
    sender_did: &str,
    limit: i64,
    cursor: Option<&str>,
) -> Value {
    let mut body = Map::new();
    body.insert(
        "limit".to_string(),
        json!(if limit <= 0 { 50 } else { limit }),
    );
    if let Some(cursor) = cursor.map(str::trim).filter(|value| !value.is_empty()) {
        body.insert("cursor".to_string(), Value::String(cursor.to_string()));
    }
    json!({
        "meta": group_local_meta(sender_did, None),
        "body": body,
    })
}

pub(crate) fn build_group_members_rpc_params(
    sender_did: &str,
    group_did: &str,
    limit: i64,
    cursor: Option<&str>,
) -> crate::ImResult<Value> {
    let group_did = require_group(group_did)?;
    let mut body = Map::new();
    body.insert(
        "group_did".to_string(),
        Value::String(group_did.to_string()),
    );
    body.insert(
        "limit".to_string(),
        json!(if limit <= 0 { 100 } else { limit }),
    );
    if let Some(cursor) = cursor.map(str::trim).filter(|value| !value.is_empty()) {
        body.insert("cursor".to_string(), Value::String(cursor.to_string()));
    }
    Ok(json!({
        "meta": group_local_meta(sender_did, Some(group_did)),
        "body": body,
    }))
}

pub(crate) fn build_group_messages_rpc_params(
    sender_did: &str,
    group_did: &str,
    limit: i64,
    cursor: Option<&str>,
    skip: i64,
) -> crate::ImResult<Value> {
    let group_did = require_group(group_did)?;
    let mut body = Map::new();
    body.insert(
        "group_did".to_string(),
        Value::String(group_did.to_string()),
    );
    body.insert(
        "limit".to_string(),
        json!(if limit <= 0 { 50 } else { limit }),
    );
    if let Some(cursor) = cursor.map(str::trim).filter(|value| !value.is_empty()) {
        body.insert("since_seq".to_string(), Value::String(cursor.to_string()));
    }
    if skip > 0 {
        body.insert("skip".to_string(), json!(skip));
    }
    Ok(json!({
        "meta": group_local_meta(sender_did, Some(group_did)),
        "body": body,
    }))
}

fn require_group(group_did: &str) -> crate::ImResult<&str> {
    let group_did = group_did.trim();
    if group_did.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("group".to_string()),
            "group target is required",
        ));
    }
    Ok(group_did)
}

fn build_group_lifecycle_payload(
    sender_did: &str,
    group_did: &str,
    method: &str,
    body: Value,
) -> crate::ImResult<DirectPayload> {
    let group_did = require_group(group_did)?;
    Ok(DirectPayload {
        method: method.to_string(),
        meta: signed_group_meta(sender_did, "group", group_did, "application/json", false),
        body,
    })
}

fn group_profile_patch(request: &crate::groups::GroupProfilePatch) -> Map<String, Value> {
    let mut patch = Map::new();
    insert_optional_trimmed_string(&mut patch, "display_name", request.name.as_deref());
    insert_optional_trimmed_string(&mut patch, "description", request.description.as_deref());
    insert_optional_trimmed_string(&mut patch, "avatar_uri", request.avatar_uri.as_deref());
    insert_optional_trimmed_string(
        &mut patch,
        "discoverability",
        request.discoverability.as_ref().map(|value| value.as_str()),
    );
    insert_optional_trimmed_string(&mut patch, "slug", request.slug.as_deref());
    insert_optional_trimmed_string(&mut patch, "goal", request.goal.as_deref());
    insert_optional_trimmed_string(&mut patch, "rules", request.rules.as_deref());
    insert_optional_trimmed_string(
        &mut patch,
        "message_prompt",
        request.message_prompt.as_deref(),
    );
    insert_optional_trimmed_string(&mut patch, "doc_url", request.doc_url.as_deref());
    patch
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
        Value::String(format!("op-{}", super::common::generate_operation_id())),
    );
    if include_message_id {
        meta.insert(
            "message_id".to_string(),
            Value::String(format!("msg-{}", super::common::generate_operation_id())),
        );
    }
    meta.insert(
        "created_at".to_string(),
        Value::String(super::common::now_rfc3339()),
    );
    meta.insert(
        "content_type".to_string(),
        Value::String(content_type.to_string()),
    );
    Value::Object(meta)
}

fn group_policy_patch(
    admission_mode: Option<&crate::groups::GroupAdmissionMode>,
    attachments_allowed: Option<bool>,
    max_members: Option<crate::groups::GroupMemberLimit>,
    member_max_messages: Option<i64>,
    member_max_total_chars: Option<i64>,
) -> Map<String, Value> {
    let mut patch = Map::new();
    insert_optional_trimmed_string(
        &mut patch,
        "admission_mode",
        admission_mode.map(|value| value.as_str()),
    );
    if let Some(value) = attachments_allowed {
        patch.insert("attachments_allowed".to_string(), Value::Bool(value));
    }
    if let Some(value) = max_members {
        patch.insert(
            "max_members".to_string(),
            Value::String(value.to_protocol_string()),
        );
    }
    if let Some(value) = member_max_messages {
        patch.insert("member_max_messages".to_string(), json!(value));
    }
    if let Some(value) = member_max_total_chars {
        patch.insert("member_max_total_chars".to_string(), json!(value));
    }
    if patch.is_empty() {
        return patch;
    }
    enrich_group_policy_defaults(&mut patch);
    patch
}

fn enrich_group_policy_defaults(patch: &mut Map<String, Value>) {
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
}

fn normalized_security_profile(
    e2ee: bool,
    message_security_profile: Option<&crate::groups::GroupMessageSecurityProfile>,
) -> Option<String> {
    if e2ee {
        return Some("group-e2ee".to_string());
    }
    match message_security_profile
        .map(|value| value.as_str())
        .unwrap_or_default()
    {
        "" | "transport-protected" => None,
        value => Some(value.to_string()),
    }
}

fn group_security_body(required: bool) -> Map<String, Value> {
    let mut body = Map::new();
    insert_group_security(&mut body, required);
    body
}

fn insert_group_security(body: &mut Map<String, Value>, required: bool) {
    if required {
        body.insert(
            "message_security_profile".to_string(),
            Value::String("group-e2ee".to_string()),
        );
        body.insert("secure".to_string(), Value::String("required".to_string()));
    }
}

fn insert_required_trimmed_string(
    patch: &mut Map<String, Value>,
    key: &str,
    value: &str,
    field: &'static str,
) -> crate::ImResult<()> {
    let value = value.trim();
    if value.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some(field.to_string()),
            format!("{field} must not be empty"),
        ));
    }
    patch.insert(key.to_string(), Value::String(value.to_string()));
    Ok(())
}

fn insert_optional_trimmed_string(patch: &mut Map<String, Value>, key: &str, value: Option<&str>) {
    let value = value.map(str::trim).unwrap_or_default();
    if !value.is_empty() {
        patch.insert(key.to_string(), Value::String(value.to_string()));
    }
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

#[cfg(test)]
mod handle_identity_tests {
    use super::*;

    #[test]
    fn authoritative_p4_group_get_info_has_exact_target_and_policy_projection() {
        let legacy = build_group_get_rpc_params("did:example:alice", "did:example:group").unwrap();
        assert!(legacy["body"].get("include_policy").is_none());

        let p4 = build_group_get_info_rpc_params(
            "did:example:alice",
            "did:example:group",
            "read-1",
            true,
        )
        .unwrap();
        assert_eq!(p4["meta"]["anp_version"], "2.0");
        assert_eq!(p4["meta"]["profile"], "anp.group.base.v2");
        assert_eq!(
            p4["meta"]["target"],
            json!({"kind": "group", "did": "did:example:group"})
        );
        assert_eq!(p4["body"]["include_policy"], true);
        assert_eq!(p4["body"]["include_member_list"], false);
    }

    #[test]
    fn p4_wire_preserves_explicit_handle_mode_without_internal_ids() {
        let mut create = crate::groups::GroupCreateRequest::new("Demo");
        create.creator_handle = Some(crate::ids::Handle::parse("alice.example.com", "").unwrap());
        let created = build_group_create_payload(
            "did:example:alice",
            &create,
            &crate::ids::Did::parse("did:example:service").unwrap(),
        )
        .unwrap();
        assert_eq!(created.body["creator_handle"], "alice.example.com");

        let joined = build_group_join_payload(
            "did:example:alice",
            &crate::groups::GroupJoinRequest {
                group: crate::ids::GroupRef::parse("did:example:group").unwrap(),
                member_handle: Some(crate::ids::Handle::parse("alice.example.com", "").unwrap()),
                reason_text: None,
            },
        )
        .unwrap();
        assert_eq!(joined.body["member_handle"], "alice.example.com");

        let added = build_group_add_member_payload(
            "did:example:alice",
            &crate::groups::GroupMemberMutationRequest {
                group: crate::ids::GroupRef::parse("did:example:group").unwrap(),
                member: crate::groups::GroupMemberRef::parse("bob.example.com", "").unwrap(),
                role: None,
                reason_text: None,
                leave_request_id: None,
                security: crate::groups::GroupSecurityRequirement::Default,
            },
        )
        .unwrap();
        assert_eq!(added.body["member_handle"], "bob.example.com");
        assert!(added.body.get("member_did").is_none());
        let encoded = serde_json::to_string(&added.body).unwrap();
        for forbidden in ["user_id", "member_user_id", "peer_user_id"] {
            assert!(!encoded.contains(forbidden));
        }
    }

    #[test]
    fn p4_wire_keeps_did_only_and_validates_rebind_generation() {
        let added = build_group_add_member_payload(
            "did:example:alice",
            &crate::groups::GroupMemberMutationRequest {
                group: crate::ids::GroupRef::parse("did:example:group").unwrap(),
                member: crate::groups::GroupMemberRef::parse("did:example:bob", "").unwrap(),
                role: None,
                reason_text: None,
                leave_request_id: None,
                security: crate::groups::GroupSecurityRequirement::Default,
            },
        )
        .unwrap();
        assert_eq!(added.body["member_did"], "did:example:bob");
        assert!(added.body.get("member_handle").is_none());

        let request = crate::groups::GroupRebindMemberRequest {
            group: crate::ids::GroupRef::parse("did:example:group").unwrap(),
            member_handle: crate::ids::Handle::parse("bob.example.com", "").unwrap(),
            previous_member_did: crate::ids::Did::parse("did:example:bob-old").unwrap(),
            new_member_did: crate::ids::Did::parse("did:example:bob-new").unwrap(),
            handle_binding_generation: "100000000000000000000000000000000000000".to_owned(),
        };
        let rebound = build_group_rebind_member_payload("did:example:bob-new", &request).unwrap();
        assert_eq!(rebound.method, "group.rebind_member");
        assert_eq!(
            rebound.body["handle_binding_generation"],
            "100000000000000000000000000000000000000"
        );
        let mut invalid = request;
        invalid.handle_binding_generation = "01".to_owned();
        assert!(build_group_rebind_member_payload("did:example:bob-new", &invalid).is_err());
    }
}
