use crate::identity::types::StoredIdentity;
use crate::message::group_wire::{
    GROUP_E2EE_PROFILE, GROUP_E2EE_SECURITY_PROFILE, GROUP_E2EE_TRANSPORT_PROFILE,
};
use crate::message::{build_origin_proof, origin_auth_value, DirectPayload, MessageError};
use serde_json::{json, Map, Value};

pub const GROUP_E2EE_CIPHER_CONTENT_TYPE: &str = "application/anp-group-cipher+json";

pub fn build_group_e2ee_create_rpc_params(
    record: &StoredIdentity,
    service_did: &str,
    group_did: &str,
    mls_head: Map<String, Value>,
) -> Result<Value, MessageError> {
    build_group_e2ee_rpc_params(
        record,
        "service",
        service_did,
        "group.e2ee.create",
        Value::Object(e2ee_head_body(group_did, "", &mls_head)),
        "",
        "",
        "",
        GROUP_E2EE_SECURITY_PROFILE,
    )
}

pub fn build_group_e2ee_add_rpc_params(
    record: &StoredIdentity,
    group_did: &str,
    member_did: &str,
    mls_head: Map<String, Value>,
) -> Result<Value, MessageError> {
    let mut body = e2ee_head_body(group_did, member_did, &mls_head);
    copy_keys(
        &mls_head,
        &mut body,
        &[
            "welcome_b64u",
            "commit_b64u",
            "ratchet_tree_b64u",
            "key_package_id",
            "group_key_package",
        ],
    );
    if let Some(value) = mls_head.get("key_package_id") {
        body.insert("subject_key_package_id".to_string(), value.clone());
    }
    build_group_e2ee_rpc_params(
        record,
        "group",
        group_did,
        "group.e2ee.add",
        Value::Object(body),
        "",
        "",
        "",
        GROUP_E2EE_SECURITY_PROFILE,
    )
}

pub fn build_group_e2ee_remove_rpc_params(
    record: &StoredIdentity,
    group_did: &str,
    member_did: &str,
    prepared_commit: Map<String, Value>,
    reason_text: &str,
    leave_request_id: &str,
) -> Result<Value, MessageError> {
    let mut body = e2ee_membership_commit_body(group_did, member_did, "removed", &prepared_commit);
    let reason = reason_text.trim();
    if !reason.is_empty() {
        body.insert("reason_text".to_string(), Value::String(reason.to_string()));
    }
    let request_id = leave_request_id.trim();
    if !request_id.is_empty() {
        body.insert(
            "leave_request_id".to_string(),
            Value::String(request_id.to_string()),
        );
    }
    build_group_e2ee_rpc_params(
        record,
        "group",
        group_did,
        "group.e2ee.remove",
        Value::Object(body),
        "",
        &string_from_value(prepared_commit.get("operation_id")),
        "",
        GROUP_E2EE_SECURITY_PROFILE,
    )
}

pub fn build_group_e2ee_leave_request_rpc_params(
    record: &StoredIdentity,
    group_did: &str,
    reason_text: &str,
) -> Result<Value, MessageError> {
    let trimmed_group = group_did.trim();
    let mut body = json_object(json!({
        "group_did": trimmed_group,
        "subject_did": record.did,
        "member_did": record.did,
        "subject_status": "leave_requested",
        "group_state_ref": { "group_did": trimmed_group },
    }));
    let reason = reason_text.trim();
    if !reason.is_empty() {
        body.insert("reason_text".to_string(), Value::String(reason.to_string()));
    }
    build_group_e2ee_rpc_params(
        record,
        "group",
        group_did,
        "group.e2ee.leave_request",
        Value::Object(body),
        "",
        "",
        "",
        GROUP_E2EE_TRANSPORT_PROFILE,
    )
}

pub fn build_group_e2ee_leave_rpc_params(
    record: &StoredIdentity,
    group_did: &str,
    prepared_commit: Map<String, Value>,
) -> Result<Value, MessageError> {
    let body = e2ee_membership_commit_body(group_did, &record.did, "left", &prepared_commit);
    build_group_e2ee_rpc_params(
        record,
        "group",
        group_did,
        "group.e2ee.leave",
        Value::Object(body),
        "",
        &string_from_value(prepared_commit.get("operation_id")),
        "",
        GROUP_E2EE_SECURITY_PROFILE,
    )
}

pub fn build_group_e2ee_send_rpc_params(
    record: &StoredIdentity,
    group_did: &str,
    cipher: Map<String, Value>,
    operation_id: &str,
    message_id: &str,
) -> Result<Value, MessageError> {
    build_group_e2ee_rpc_params(
        record,
        "group",
        group_did,
        "group.e2ee.send",
        Value::Object(sanitize_group_cipher_object_for_service(&cipher)),
        GROUP_E2EE_CIPHER_CONTENT_TYPE,
        operation_id,
        message_id,
        GROUP_E2EE_SECURITY_PROFILE,
    )
}

pub fn build_group_e2ee_publish_key_package_rpc_params(
    record: &StoredIdentity,
    service_did: &str,
    package_result: Map<String, Value>,
) -> Result<Value, MessageError> {
    let service_did = service_did.trim();
    if service_did.is_empty() {
        return Err(MessageError::MissingMessageServiceDid);
    }
    let group_key_package = package_result
        .get("group_key_package")
        .and_then(Value::as_object)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| MessageError::Json("group_key_package is required".to_string()))?;
    let body = json!({
        "group_key_package": sanitize_group_key_package_for_service(group_key_package),
    });
    signed_e2ee_params(
        record,
        "group.e2ee.publish_key_package",
        e2ee_meta(
            &record.did,
            "service",
            service_did,
            GROUP_E2EE_TRANSPORT_PROFILE,
            "",
            "",
            false,
        ),
        body,
    )
}

pub fn build_group_e2ee_get_key_package_rpc_params(
    record: &StoredIdentity,
    service_did: &str,
    group_did: &str,
    target_did: &str,
) -> Result<Value, MessageError> {
    build_group_e2ee_get_key_package_rpc_params_from_body(
        record,
        service_did,
        json_object(json!({
            "target_did": target_did.trim(),
            "group_did": group_did.trim(),
        })),
    )
}

pub fn build_group_e2ee_get_recovery_key_package_rpc_params(
    record: &StoredIdentity,
    service_did: &str,
    group_did: &str,
    target_did: &str,
    device_id: &str,
) -> Result<Value, MessageError> {
    build_group_e2ee_get_key_package_rpc_params_from_body(
        record,
        service_did,
        json_object(json!({
            "target_did": target_did,
            "purpose": "recovery",
            "group_did": group_did.trim(),
            "device_id": default_string(device_id.trim(), "default"),
        })),
    )
}

pub fn build_group_e2ee_get_update_key_package_rpc_params(
    record: &StoredIdentity,
    service_did: &str,
    group_did: &str,
    target_did: &str,
    device_id: &str,
) -> Result<Value, MessageError> {
    build_group_e2ee_get_key_package_rpc_params_from_body(
        record,
        service_did,
        json_object(json!({
            "target_did": target_did,
            "purpose": "update",
            "group_did": group_did.trim(),
            "device_id": default_string(device_id.trim(), "default"),
        })),
    )
}

pub fn build_group_e2ee_recover_member_rpc_params(
    record: &StoredIdentity,
    group_did: &str,
    member_did: &str,
    device_id: &str,
    prepared: Map<String, Value>,
    leased_package: Map<String, Value>,
) -> Result<Value, MessageError> {
    let body =
        e2ee_recovery_commit_body(group_did, member_did, device_id, &prepared, &leased_package);
    build_group_e2ee_rpc_params(
        record,
        "group",
        group_did,
        "group.e2ee.recover_member",
        Value::Object(body),
        "",
        &string_from_value(prepared.get("operation_id")),
        "",
        GROUP_E2EE_SECURITY_PROFILE,
    )
}

pub fn build_group_e2ee_update_member_rpc_params(
    record: &StoredIdentity,
    group_did: &str,
    member_did: &str,
    device_id: &str,
    prepared: Map<String, Value>,
    leased_package: Map<String, Value>,
) -> Result<Value, MessageError> {
    let body =
        e2ee_update_commit_body(group_did, member_did, device_id, &prepared, &leased_package);
    build_group_e2ee_rpc_params(
        record,
        "group",
        group_did,
        "group.e2ee.update",
        Value::Object(body),
        "",
        &string_from_value(prepared.get("operation_id")),
        "",
        GROUP_E2EE_SECURITY_PROFILE,
    )
}

pub fn build_group_e2ee_notice_rpc_params(
    record: &StoredIdentity,
    group_did: &str,
    limit: i64,
    mark_delivered: bool,
    notice_ids: Vec<String>,
) -> Result<Value, MessageError> {
    let limit = if limit <= 0 {
        50
    } else if limit > 100 {
        100
    } else {
        limit
    };
    let mut body = Map::new();
    body.insert("limit".to_string(), json!(limit));
    let group_did = group_did.trim();
    if !group_did.is_empty() {
        body.insert(
            "group_did".to_string(),
            Value::String(group_did.to_string()),
        );
    }
    if mark_delivered {
        body.insert("mark_delivered".to_string(), Value::Bool(true));
    }
    let ids: Vec<Value> = notice_ids
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(Value::String)
        .collect();
    if !ids.is_empty() {
        body.insert("notice_ids".to_string(), Value::Array(ids));
    }
    signed_e2ee_params(
        record,
        "group.e2ee.notice",
        e2ee_meta(
            &record.did,
            "agent",
            &record.did,
            GROUP_E2EE_TRANSPORT_PROFILE,
            "",
            "",
            false,
        ),
        Value::Object(body),
    )
}

pub fn build_group_e2ee_head_rpc_params(
    record: &StoredIdentity,
    group_did: &str,
) -> Result<Value, MessageError> {
    let group_did = group_did.trim();
    if group_did.is_empty() {
        return Err(MessageError::GroupRequired);
    }
    signed_e2ee_params(
        record,
        "group.e2ee.head",
        e2ee_meta(
            &record.did,
            "group",
            group_did,
            GROUP_E2EE_TRANSPORT_PROFILE,
            "",
            "",
            false,
        ),
        json!({
            "group_did": group_did,
            "group_state_ref": { "group_did": group_did },
        }),
    )
}

fn build_group_e2ee_get_key_package_rpc_params_from_body(
    record: &StoredIdentity,
    service_did: &str,
    body: Map<String, Value>,
) -> Result<Value, MessageError> {
    let service_did = service_did.trim();
    let target_did = string_from_value(body.get("target_did"));
    if service_did.is_empty() {
        return Err(MessageError::MissingMessageServiceDid);
    }
    if target_did.trim().is_empty() {
        return Err(MessageError::MemberRequired);
    }
    if string_from_value(body.get("group_did")).trim().is_empty() {
        return Err(MessageError::GroupRequired);
    }
    signed_e2ee_params(
        record,
        "group.e2ee.get_key_package",
        e2ee_meta(
            &record.did,
            "service",
            service_did,
            GROUP_E2EE_TRANSPORT_PROFILE,
            "",
            "",
            false,
        ),
        Value::Object(body),
    )
}

fn build_group_e2ee_rpc_params(
    record: &StoredIdentity,
    target_kind: &str,
    target_did: &str,
    method: &str,
    body: Value,
    content_type: &str,
    operation_id: &str,
    message_id: &str,
    security_profile: &str,
) -> Result<Value, MessageError> {
    let target_kind = default_string(target_kind.trim(), "group");
    let target_did = target_did.trim();
    if target_did.is_empty() {
        return Err(MessageError::GroupRequired);
    }
    signed_e2ee_params(
        record,
        method,
        e2ee_meta(
            &record.did,
            target_kind,
            target_did,
            security_profile,
            content_type,
            message_id,
            method == "group.e2ee.send",
        )
        .with_operation_id(operation_id),
        body,
    )
}

trait E2eeMetaExt {
    fn with_operation_id(self, operation_id: &str) -> Value;
}

impl E2eeMetaExt for Value {
    fn with_operation_id(mut self, operation_id: &str) -> Value {
        if let Value::Object(meta) = &mut self {
            let operation_id = operation_id.trim();
            if !operation_id.is_empty() {
                meta.insert(
                    "operation_id".to_string(),
                    Value::String(operation_id.to_string()),
                );
            }
        }
        self
    }
}

fn signed_e2ee_params(
    record: &StoredIdentity,
    method: &str,
    meta: Value,
    body: Value,
) -> Result<Value, MessageError> {
    let payload = DirectPayload {
        method: method.to_string(),
        meta,
        body,
    };
    let origin_proof = build_origin_proof(record, &payload)?;
    Ok(json!({
        "meta": payload.meta,
        "auth": origin_auth_value(&origin_proof),
        "body": payload.body,
    }))
}

fn e2ee_meta(
    sender_did: &str,
    target_kind: &str,
    target_did: &str,
    security_profile: &str,
    content_type: &str,
    message_id: &str,
    include_message_id: bool,
) -> Value {
    let security_profile = default_string(security_profile.trim(), GROUP_E2EE_SECURITY_PROFILE);
    let content_type = default_string(content_type, "application/json");
    let mut meta = json_object(json!({
        "anp_version": "1.0",
        "profile": GROUP_E2EE_PROFILE,
        "security_profile": security_profile,
        "sender_did": sender_did,
        "target": {
            "kind": target_kind,
            "did": target_did,
        },
        "operation_id": format!("op-{}", crate::message::wire::generate_operation_id()),
        "created_at": crate::message::wire::now_rfc3339(),
        "content_type": content_type,
    }));
    if include_message_id {
        let message_id = message_id.trim();
        meta.insert(
            "message_id".to_string(),
            Value::String(if message_id.is_empty() {
                format!("msg-{}", crate::message::wire::generate_operation_id())
            } else {
                message_id.to_string()
            }),
        );
    }
    Value::Object(meta)
}

fn e2ee_head_body(
    group_did: &str,
    member_did: &str,
    mls_head: &Map<String, Value>,
) -> Map<String, Value> {
    let mut body = Map::new();
    body.insert(
        "group_did".to_string(),
        Value::String(group_did.to_string()),
    );
    body.insert(
        "group_state_ref".to_string(),
        Value::Object(group_state_ref_from_source(group_did, mls_head)),
    );
    copy_keys(
        mls_head,
        &mut body,
        &[
            "crypto_group_id_b64u",
            "epoch",
            "epoch_authenticator",
            "epoch_authenticator_b64u",
            "suite",
            "last_handshake_digest",
        ],
    );
    if let Some(value) = body.get("epoch_authenticator_b64u").cloned() {
        body.insert("epoch_authenticator".to_string(), value);
    }
    if !member_did.is_empty() {
        body.insert(
            "member_did".to_string(),
            Value::String(member_did.to_string()),
        );
        body.insert(
            "subject_did".to_string(),
            Value::String(member_did.to_string()),
        );
    }
    augment_group_state_ref_with_crypto_claims(&mut body, false);
    body
}

fn e2ee_membership_commit_body(
    group_did: &str,
    subject_did: &str,
    default_subject_status: &str,
    prepared_commit: &Map<String, Value>,
) -> Map<String, Value> {
    let mut body = e2ee_head_body(group_did, subject_did, prepared_commit);
    copy_keys(
        prepared_commit,
        &mut body,
        &[
            "pending_commit_id",
            "operation_id",
            "commit_b64u",
            "ratchet_tree_b64u",
            "group_info_b64u",
            "from_epoch",
            "to_epoch",
            "actor_did",
            "subject_status",
        ],
    );
    if !body.contains_key("epoch") {
        if let Some(value) = prepared_commit.get("to_epoch") {
            body.insert("epoch".to_string(), value.clone());
        }
    }
    if !body.contains_key("epoch_authenticator") {
        if let Some(value) = prepared_commit.get("epoch_authenticator_b64u") {
            body.insert("epoch_authenticator".to_string(), value.clone());
        }
    }
    if !body.contains_key("subject_status") && !default_subject_status.is_empty() {
        body.insert(
            "subject_status".to_string(),
            Value::String(default_subject_status.to_string()),
        );
    }
    augment_group_state_ref_with_crypto_claims(&mut body, true);
    body
}

fn e2ee_recovery_commit_body(
    group_did: &str,
    member_did: &str,
    device_id: &str,
    prepared: &Map<String, Value>,
    leased_package: &Map<String, Value>,
) -> Map<String, Value> {
    let mut body = json_object(json!({
        "group_did": group_did,
        "group_state_ref": group_state_ref_from_source(group_did, prepared),
        "target": {
            "agent_did": member_did,
            "device_id": default_string(device_id.trim(), "default"),
        },
    }));
    copy_keys(
        prepared,
        &mut body,
        &[
            "crypto_group_id_b64u",
            "epoch",
            "epoch_authenticator",
            "epoch_authenticator_b64u",
            "suite",
            "last_handshake_digest",
            "pending_commit_id",
            "operation_id",
            "commit_b64u",
            "welcome_b64u",
            "ratchet_tree_b64u",
            "group_info_b64u",
            "from_epoch",
            "to_epoch",
            "old_generation_id",
            "new_generation_id",
        ],
    );
    if !body.contains_key("epoch") {
        if let Some(value) = prepared.get("to_epoch") {
            body.insert("epoch".to_string(), value.clone());
        }
    }
    if !body.contains_key("epoch_authenticator") {
        if let Some(value) = prepared.get("epoch_authenticator_b64u") {
            body.insert("epoch_authenticator".to_string(), value.clone());
        }
    }
    let key_package_id = first_non_empty_value(&[
        prepared.get("recovery_key_package_id"),
        prepared.get("key_package_id"),
        leased_package.get("key_package_id"),
    ]);
    if !key_package_id.is_empty() {
        body.insert(
            "recovery_key_package_id".to_string(),
            Value::String(key_package_id),
        );
    }
    if let Some(group_key_package) = leased_package
        .get("group_key_package")
        .and_then(Value::as_object)
        .filter(|value| !value.is_empty())
    {
        body.insert(
            "group_key_package".to_string(),
            Value::Object(sanitize_group_key_package_for_service(group_key_package)),
        );
    }
    augment_group_state_ref_with_crypto_claims(&mut body, true);
    body
}

fn e2ee_update_commit_body(
    group_did: &str,
    member_did: &str,
    device_id: &str,
    prepared: &Map<String, Value>,
    leased_package: &Map<String, Value>,
) -> Map<String, Value> {
    let mut body =
        e2ee_recovery_commit_body(group_did, member_did, device_id, prepared, leased_package);
    let key_package_id = first_non_empty_value(&[
        prepared.get("update_key_package_id"),
        prepared.get("key_package_id"),
        leased_package.get("key_package_id"),
    ]);
    if !key_package_id.is_empty() {
        body.insert(
            "update_key_package_id".to_string(),
            Value::String(key_package_id),
        );
        body.remove("recovery_key_package_id");
    }
    if let Some(group_key_package) = body
        .get_mut("group_key_package")
        .and_then(Value::as_object_mut)
    {
        if string_from_value(group_key_package.get("purpose")).is_empty() {
            group_key_package.insert("purpose".to_string(), Value::String("update".to_string()));
        }
    }
    body
}

fn sanitize_group_cipher_object_for_service(cipher: &Map<String, Value>) -> Map<String, Value> {
    let mut sanitized = Map::new();
    copy_keys(
        cipher,
        &mut sanitized,
        &[
            "crypto_group_id_b64u",
            "epoch",
            "private_message_b64u",
            "group_state_ref",
            "epoch_authenticator",
        ],
    );
    sanitized
}

fn sanitize_group_key_package_for_service(input: &Map<String, Value>) -> Map<String, Value> {
    let mut output = Map::new();
    for key in [
        "owner_did",
        "device_id",
        "key_package_id",
        "suite",
        "mls_key_package_b64u",
        "did_wba_binding",
        "expires_at",
        "purpose",
        "group_did",
        "non_cryptographic",
        "artifact_mode",
    ] {
        if let Some(value) = input.get(key) {
            if matches!(key, "group_did" | "purpose")
                && string_from_value(Some(value)).trim().is_empty()
            {
                continue;
            }
            output.insert(key.to_string(), value.clone());
        }
    }
    output
}

fn group_state_ref_from_source(group_did: &str, source: &Map<String, Value>) -> Map<String, Value> {
    let mut reference = Map::new();
    reference.insert(
        "group_did".to_string(),
        Value::String(group_did.trim().to_string()),
    );
    if let Some(source_ref) = source.get("group_state_ref").and_then(Value::as_object) {
        for (key, value) in source_ref {
            reference.insert(key.clone(), value.clone());
        }
    }
    reference.insert(
        "group_did".to_string(),
        Value::String(group_did.trim().to_string()),
    );
    let version = first_non_empty_value(&[
        source.get("group_state_version"),
        reference.get("group_state_version"),
    ]);
    if !version.is_empty() {
        reference.insert("group_state_version".to_string(), Value::String(version));
    }
    reference
}

fn augment_group_state_ref_with_crypto_claims(
    body: &mut Map<String, Value>,
    prefer_from_epoch: bool,
) {
    if !body
        .get("group_state_ref")
        .map(Value::is_object)
        .unwrap_or(false)
    {
        let group_did = string_from_value(body.get("group_did"));
        body.insert(
            "group_state_ref".to_string(),
            json!({ "group_did": group_did }),
        );
    }
    let crypto_group_id = string_from_value(body.get("crypto_group_id_b64u"));
    let epoch = if prefer_from_epoch {
        let from_epoch = string_from_value(body.get("from_epoch"));
        if !from_epoch.is_empty() {
            Some(from_epoch)
        } else {
            let value = string_from_value(body.get("epoch"));
            (!value.is_empty()).then_some(value)
        }
    } else {
        let value = string_from_value(body.get("epoch"));
        (!value.is_empty()).then_some(value)
    };
    if let Some(reference) = body
        .get_mut("group_state_ref")
        .and_then(Value::as_object_mut)
    {
        if !crypto_group_id.is_empty() {
            reference.insert(
                "crypto_group_id_b64u".to_string(),
                Value::String(crypto_group_id),
            );
        }
        if let Some(epoch) = epoch {
            reference.insert("epoch".to_string(), Value::String(epoch));
        }
    }
}

fn copy_keys(source: &Map<String, Value>, target: &mut Map<String, Value>, keys: &[&str]) {
    for key in keys {
        if let Some(value) = source.get(*key) {
            target.insert((*key).to_string(), value.clone());
        }
    }
}

fn string_from_value(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .unwrap_or_default()
}

fn default_string<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    if value.trim().is_empty() {
        fallback
    } else {
        value
    }
}

fn first_non_empty_value(values: &[Option<&Value>]) -> String {
    for value in values {
        let text = string_from_value(*value);
        if !text.is_empty() {
            return text;
        }
    }
    String::new()
}

fn json_object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}
