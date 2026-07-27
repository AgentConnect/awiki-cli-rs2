use serde_json::{json, Map, Value};

use crate::internal::wire::direct::DirectPayload;

pub(crate) const GROUP_E2EE_PROFILE: &str = anp::group_e2ee::PROFILE;
pub(crate) const GROUP_E2EE_SECURITY_PROFILE: &str = anp::group_e2ee::SECURITY_PROFILE;
pub(crate) const GROUP_E2EE_TRANSPORT_SECURITY_PROFILE: &str =
    anp::group_e2ee::TRANSPORT_SECURITY_PROFILE;
pub(crate) const GROUP_E2EE_CIPHER_CONTENT_TYPE: &str = anp::group_e2ee::GROUP_CIPHER_CONTENT_TYPE;

pub(crate) fn build_group_e2ee_publish_key_package_rpc_params(
    credentials: &crate::internal::message_runtime::group::GroupTextCredentials,
    sender_did: &str,
    service_did: &str,
    key_package: &anp::group_e2ee::GroupKeyPackage,
    operation_id: &str,
) -> crate::ImResult<Value> {
    let service_did = require_non_empty("service_did", service_did)?;
    let operation_id = require_non_empty("operation_id", operation_id)?;
    build_signed_group_e2ee_params(
        credentials,
        "group.e2ee.publish_key_package",
        group_e2ee_meta(
            sender_did,
            "service",
            service_did,
            GROUP_E2EE_TRANSPORT_SECURITY_PROFILE,
            "application/json",
            operation_id,
            None,
        )?,
        json!({
            "group_key_package": sanitize_group_key_package_for_service(key_package)?,
        }),
    )
}

pub(crate) fn build_group_e2ee_create_rpc_params(
    credentials: &crate::internal::message_runtime::group::GroupTextCredentials,
    sender_did: &str,
    service_did: &str,
    group_did: &str,
    prepared: &anp::group_e2ee::operations::PreparedMlsCommitOutput,
    group_state_ref: Option<&anp::group_e2ee::GroupStateRef>,
) -> crate::ImResult<Value> {
    let service_did = require_non_empty("service_did", service_did)?;
    let group_did = require_non_empty("group_did", group_did)?;
    let mut prepared_map = prepared_commit_map(prepared)?;
    insert_group_state_ref(&mut prepared_map, group_state_ref)?;
    build_signed_group_e2ee_params(
        credentials,
        "group.e2ee.create",
        group_e2ee_meta(
            sender_did,
            "service",
            service_did,
            GROUP_E2EE_SECURITY_PROFILE,
            "application/json",
            prepared.operation_id.as_str(),
            None,
        )?,
        Value::Object(e2ee_head_body(group_did, "", &prepared_map)),
    )
}

pub(crate) fn build_group_e2ee_add_rpc_params(
    credentials: &crate::internal::message_runtime::group::GroupTextCredentials,
    sender_did: &str,
    group_did: &str,
    member_did: &str,
    prepared: &anp::group_e2ee::operations::PreparedMlsCommitOutput,
    group_key_package: &anp::group_e2ee::GroupKeyPackage,
    group_state_ref: Option<&anp::group_e2ee::GroupStateRef>,
) -> crate::ImResult<Value> {
    let group_did = require_non_empty("group_did", group_did)?;
    let member_did = require_non_empty("member_did", member_did)?;
    let mut prepared_map = prepared_commit_map(prepared)?;
    insert_group_state_ref(&mut prepared_map, group_state_ref)?;
    prepared_map.insert(
        "key_package_id".to_owned(),
        Value::String(group_key_package.key_package_id.clone()),
    );
    prepared_map.insert(
        "group_key_package".to_owned(),
        serde_json::to_value(group_key_package).map_err(|err| crate::ImError::Serialization {
            detail: err.to_string(),
        })?,
    );
    let mut body = e2ee_head_body(group_did, member_did, &prepared_map);
    copy_keys(
        &prepared_map,
        &mut body,
        &[
            "welcome_b64u",
            "commit_b64u",
            "ratchet_tree_b64u",
            "group_info_b64u",
            "key_package_id",
            "group_key_package",
        ],
    );
    if let Some(value) = prepared_map.get("key_package_id") {
        body.insert("subject_key_package_id".to_owned(), value.clone());
    }
    build_signed_group_e2ee_params(
        credentials,
        "group.e2ee.add",
        group_e2ee_meta(
            sender_did,
            "group",
            group_did,
            GROUP_E2EE_SECURITY_PROFILE,
            "application/json",
            prepared.operation_id.as_str(),
            None,
        )?,
        Value::Object(body),
    )
}

pub(crate) fn build_group_e2ee_remove_rpc_params(
    credentials: &crate::internal::message_runtime::group::GroupTextCredentials,
    sender_did: &str,
    group_did: &str,
    member_did: &str,
    prepared: &anp::group_e2ee::operations::PreparedMlsCommitOutput,
    group_state_ref: Option<&anp::group_e2ee::GroupStateRef>,
    reason_text: Option<&str>,
    leave_request_id: Option<&str>,
) -> crate::ImResult<Value> {
    let group_did = require_non_empty("group_did", group_did)?;
    let member_did = require_non_empty("member_did", member_did)?;
    let mut prepared_map = prepared_commit_map(prepared)?;
    insert_group_state_ref(&mut prepared_map, group_state_ref)?;
    let mut body = e2ee_membership_commit_body(group_did, member_did, "removed", &prepared_map);
    insert_optional_trimmed_string(&mut body, "reason_text", reason_text);
    insert_optional_trimmed_string(&mut body, "leave_request_id", leave_request_id);
    build_signed_group_e2ee_params(
        credentials,
        "group.e2ee.remove",
        group_e2ee_meta(
            sender_did,
            "group",
            group_did,
            GROUP_E2EE_SECURITY_PROFILE,
            "application/json",
            prepared.operation_id.as_str(),
            None,
        )?,
        Value::Object(body),
    )
}

pub(crate) fn build_group_e2ee_leave_request_rpc_params(
    credentials: &crate::internal::message_runtime::group::GroupTextCredentials,
    sender_did: &str,
    group_did: &str,
    reason_text: Option<&str>,
) -> crate::ImResult<Value> {
    let group_did = require_non_empty("group_did", group_did)?;
    let mut body = json_object(json!({
        "group_did": group_did,
        "subject_did": sender_did,
        "member_did": sender_did,
        "subject_status": "leave_requested",
        "group_state_ref": { "group_did": group_did },
    }));
    insert_optional_trimmed_string(&mut body, "reason_text", reason_text);
    build_signed_group_e2ee_params(
        credentials,
        "group.e2ee.leave_request",
        group_e2ee_meta(
            sender_did,
            "group",
            group_did,
            GROUP_E2EE_TRANSPORT_SECURITY_PROFILE,
            "application/json",
            &format!(
                "op-{}",
                crate::internal::wire::common::generate_operation_id()
            ),
            None,
        )?,
        Value::Object(body),
    )
}

pub(crate) fn build_group_e2ee_leave_rpc_params(
    credentials: &crate::internal::message_runtime::group::GroupTextCredentials,
    sender_did: &str,
    group_did: &str,
    prepared: &anp::group_e2ee::operations::PreparedMlsCommitOutput,
    group_state_ref: Option<&anp::group_e2ee::GroupStateRef>,
) -> crate::ImResult<Value> {
    let group_did = require_non_empty("group_did", group_did)?;
    let mut prepared_map = prepared_commit_map(prepared)?;
    insert_group_state_ref(&mut prepared_map, group_state_ref)?;
    let body = e2ee_membership_commit_body(group_did, sender_did, "left", &prepared_map);
    build_signed_group_e2ee_params(
        credentials,
        "group.e2ee.leave",
        group_e2ee_meta(
            sender_did,
            "group",
            group_did,
            GROUP_E2EE_SECURITY_PROFILE,
            "application/json",
            prepared.operation_id.as_str(),
            None,
        )?,
        Value::Object(body),
    )
}

pub(crate) fn build_group_e2ee_get_key_package_rpc_params(
    credentials: &crate::internal::message_runtime::group::GroupTextCredentials,
    sender_did: &str,
    service_did: &str,
    group_did: &str,
    target_did: &str,
    purpose: Option<&str>,
    device_id: Option<&str>,
) -> crate::ImResult<Value> {
    let service_did = require_non_empty("service_did", service_did)?;
    let group_did = require_non_empty("group_did", group_did)?;
    let target_did = require_non_empty("target_did", target_did)?;
    let mut body = json_object(json!({
        "target_did": target_did,
        "group_did": group_did,
    }));
    insert_optional_trimmed_string(&mut body, "purpose", purpose);
    insert_optional_trimmed_string(&mut body, "device_id", device_id);
    build_signed_group_e2ee_params(
        credentials,
        "group.e2ee.get_key_package",
        group_e2ee_meta(
            sender_did,
            "service",
            service_did,
            GROUP_E2EE_TRANSPORT_SECURITY_PROFILE,
            "application/json",
            &format!(
                "op-{}",
                crate::internal::wire::common::generate_operation_id()
            ),
            None,
        )?,
        Value::Object(body),
    )
}

pub(crate) fn build_group_e2ee_update_rpc_params(
    credentials: &crate::internal::message_runtime::group::GroupTextCredentials,
    sender_did: &str,
    group_did: &str,
    member_did: &str,
    device_id: &str,
    prepared: &anp::group_e2ee::operations::PreparedMlsCommitOutput,
    group_key_package: &anp::group_e2ee::GroupKeyPackage,
    group_state_ref: Option<&anp::group_e2ee::GroupStateRef>,
) -> crate::ImResult<Value> {
    build_group_e2ee_key_replacement_rpc_params(
        credentials,
        "group.e2ee.update",
        sender_did,
        group_did,
        member_did,
        device_id,
        prepared,
        group_key_package,
        group_state_ref,
        "update_key_package_id",
    )
}

pub(crate) fn build_group_e2ee_recover_member_rpc_params(
    credentials: &crate::internal::message_runtime::group::GroupTextCredentials,
    sender_did: &str,
    group_did: &str,
    member_did: &str,
    device_id: &str,
    prepared: &anp::group_e2ee::operations::PreparedMlsCommitOutput,
    group_key_package: &anp::group_e2ee::GroupKeyPackage,
    group_state_ref: Option<&anp::group_e2ee::GroupStateRef>,
) -> crate::ImResult<Value> {
    build_group_e2ee_key_replacement_rpc_params(
        credentials,
        "group.e2ee.recover_member",
        sender_did,
        group_did,
        member_did,
        device_id,
        prepared,
        group_key_package,
        group_state_ref,
        "recovery_key_package_id",
    )
}

pub(crate) fn build_group_e2ee_process_leave_request_rpc_params(
    credentials: &crate::internal::message_runtime::group::GroupTextCredentials,
    sender_did: &str,
    group_did: &str,
    leave_request_id: &str,
) -> crate::ImResult<Value> {
    let group_did = require_non_empty("group_did", group_did)?;
    let leave_request_id = require_non_empty("leave_request_id", leave_request_id)?;
    build_signed_group_e2ee_params(
        credentials,
        "group.e2ee.process_leave_request",
        group_e2ee_meta(
            sender_did,
            "group",
            group_did,
            GROUP_E2EE_TRANSPORT_SECURITY_PROFILE,
            "application/json",
            &format!(
                "op-{}",
                crate::internal::wire::common::generate_operation_id()
            ),
            None,
        )?,
        json!({
            "group_did": group_did,
            "group_state_ref": {
                "group_did": group_did,
            },
            "leave_request_id": leave_request_id,
        }),
    )
}

pub(crate) fn build_group_e2ee_head_rpc_params(
    credentials: &crate::internal::message_runtime::group::GroupTextCredentials,
    sender_did: &str,
    group_did: &str,
) -> crate::ImResult<Value> {
    let group_did = require_non_empty("group_did", group_did)?;
    build_signed_group_e2ee_params(
        credentials,
        "group.e2ee.head",
        group_e2ee_meta(
            sender_did,
            "group",
            group_did,
            GROUP_E2EE_TRANSPORT_SECURITY_PROFILE,
            "application/json",
            &format!(
                "op-{}",
                crate::internal::wire::common::generate_operation_id()
            ),
            None,
        )?,
        json!({
            "group_did": group_did,
            "group_state_ref": {
                "group_did": group_did,
            },
        }),
    )
}

pub(crate) fn build_group_e2ee_send_rpc_params(
    credentials: &crate::internal::message_runtime::group::GroupTextCredentials,
    sender_did: &str,
    group_did: &str,
    cipher: &anp::group_e2ee::GroupCipherObject,
    operation_id: &str,
    message_id: &str,
) -> crate::ImResult<Value> {
    build_signed_group_e2ee_params(
        credentials,
        "group.e2ee.send",
        group_e2ee_meta(
            sender_did,
            "group",
            group_did,
            GROUP_E2EE_SECURITY_PROFILE,
            GROUP_E2EE_CIPHER_CONTENT_TYPE,
            operation_id,
            Some(message_id),
        )?,
        Value::Object(group_cipher_body(cipher)?),
    )
}

pub(crate) fn build_group_e2ee_send_rpc_params_with_client_context(
    credentials: &crate::internal::message_runtime::group::GroupTextCredentials,
    sender_did: &str,
    group_did: &str,
    cipher: &anp::group_e2ee::GroupCipherObject,
    operation_id: &str,
    message_id: &str,
    client_context: Option<Value>,
) -> crate::ImResult<Value> {
    let mut params = build_group_e2ee_send_rpc_params(
        credentials,
        sender_did,
        group_did,
        cipher,
        operation_id,
        message_id,
    )?;
    if let Some(client_context) = client_context {
        if let Some(object) = params.as_object_mut() {
            object.insert("client".to_owned(), client_context);
        }
    }
    Ok(params)
}

pub(crate) fn build_group_e2ee_notice_rpc_params(
    credentials: &crate::internal::message_runtime::group::GroupTextCredentials,
    sender_did: &str,
    group_did: &str,
    limit: i64,
    mark_delivered: bool,
    notice_ids: &[String],
) -> crate::ImResult<serde_json::Value> {
    let group_did = require_non_empty("group_did", group_did)?;
    let limit = limit.clamp(1, 100);
    let mut body = serde_json::Map::from_iter([
        ("limit".to_owned(), serde_json::json!(limit)),
        ("group_did".to_owned(), serde_json::json!(group_did)),
    ]);
    if mark_delivered {
        body.insert("mark_delivered".to_owned(), serde_json::Value::Bool(true));
    }
    let ids = notice_ids
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| serde_json::Value::String(value.to_owned()))
        .collect::<Vec<_>>();
    if !ids.is_empty() {
        body.insert("notice_ids".to_owned(), serde_json::Value::Array(ids));
    }
    build_signed_group_e2ee_params(
        credentials,
        "group.e2ee.notice",
        group_e2ee_meta(
            sender_did,
            "agent",
            sender_did,
            GROUP_E2EE_TRANSPORT_SECURITY_PROFILE,
            "application/json",
            &format!(
                "op-{}",
                crate::internal::wire::common::generate_operation_id()
            ),
            None,
        )?,
        serde_json::Value::Object(body),
    )
}

fn build_signed_group_e2ee_params(
    credentials: &crate::internal::message_runtime::group::GroupTextCredentials,
    method: &str,
    meta: Value,
    body: Value,
) -> crate::ImResult<Value> {
    let payload = DirectPayload {
        method: method.to_owned(),
        meta,
        body,
    };
    let origin_proof = crate::internal::proof::origin::build_origin_proof(
        &crate::internal::proof::origin::OriginProofIdentity {
            identity_name: credentials.identity_name.clone(),
            did_document: credentials.did_document.clone(),
            key1_private_pem: credentials.key1_private_pem.clone(),
            verification_method: credentials.verification_method.clone(),
        },
        &payload,
    )?;
    Ok(json!({
        "meta": payload.meta,
        "auth": crate::internal::proof::origin::origin_auth_value(&origin_proof),
        "body": payload.body,
    }))
}

fn build_group_e2ee_key_replacement_rpc_params(
    credentials: &crate::internal::message_runtime::group::GroupTextCredentials,
    method: &str,
    sender_did: &str,
    group_did: &str,
    member_did: &str,
    device_id: &str,
    prepared: &anp::group_e2ee::operations::PreparedMlsCommitOutput,
    group_key_package: &anp::group_e2ee::GroupKeyPackage,
    group_state_ref: Option<&anp::group_e2ee::GroupStateRef>,
    key_package_id_field: &str,
) -> crate::ImResult<Value> {
    let group_did = require_non_empty("group_did", group_did)?;
    let member_did = require_non_empty("member_did", member_did)?;
    let device_id = require_non_empty("device_id", device_id)?;
    let mut prepared_map = prepared_commit_map(prepared)?;
    insert_group_state_ref(&mut prepared_map, group_state_ref)?;
    let mut body = e2ee_head_body(group_did, "", &prepared_map);
    copy_keys(
        &prepared_map,
        &mut body,
        &[
            "pending_commit_id",
            "operation_id",
            "commit_b64u",
            "welcome_b64u",
            "ratchet_tree_b64u",
            "group_info_b64u",
            "from_epoch",
            "to_epoch",
            "actor_did",
        ],
    );
    if !body.contains_key("epoch") {
        if let Some(value) = prepared_map.get("to_epoch") {
            body.insert("epoch".to_owned(), value.clone());
        }
    }
    if !body.contains_key("epoch_authenticator") {
        if let Some(value) = prepared_map.get("epoch_authenticator_b64u") {
            body.insert("epoch_authenticator".to_owned(), value.clone());
        }
    }
    body.insert(
        "target".to_owned(),
        json!({
            "agent_did": member_did,
            "device_id": device_id,
        }),
    );
    body.insert("device_id".to_owned(), Value::String(device_id.to_owned()));
    body.insert(
        key_package_id_field.to_owned(),
        Value::String(group_key_package.key_package_id.clone()),
    );
    body.insert(
        "key_package_id".to_owned(),
        Value::String(group_key_package.key_package_id.clone()),
    );
    body.insert(
        "group_key_package".to_owned(),
        serde_json::to_value(group_key_package).map_err(|err| crate::ImError::Serialization {
            detail: err.to_string(),
        })?,
    );
    augment_group_state_ref_with_crypto_claims(&mut body, true);
    build_signed_group_e2ee_params(
        credentials,
        method,
        group_e2ee_meta(
            sender_did,
            "group",
            group_did,
            GROUP_E2EE_SECURITY_PROFILE,
            "application/json",
            prepared.operation_id.as_str(),
            None,
        )?,
        Value::Object(body),
    )
}

fn group_e2ee_meta(
    sender_did: &str,
    target_kind: &str,
    target_did: &str,
    security_profile: &str,
    content_type: &str,
    operation_id: &str,
    message_id: Option<&str>,
) -> crate::ImResult<Value> {
    let sender_did = require_non_empty("sender_did", sender_did)?;
    let target_did = require_non_empty("group_did", target_did)?;
    let operation_id = require_non_empty("operation_id", operation_id)?;
    let content_type = if content_type.trim().is_empty() {
        "application/json"
    } else {
        content_type.trim()
    };
    let mut meta = json_object(json!({
        "anp_version": "1.0",
        "profile": GROUP_E2EE_PROFILE,
        "security_profile": if security_profile.trim().is_empty() {
            GROUP_E2EE_SECURITY_PROFILE
        } else {
            security_profile.trim()
        },
        "sender_did": sender_did,
        "target": {
            "kind": if target_kind.trim().is_empty() { "group" } else { target_kind.trim() },
            "did": target_did,
        },
        "operation_id": operation_id,
        "created_at": crate::internal::wire::common::now_rfc3339(),
        "content_type": content_type,
    }));
    if let Some(message_id) = message_id.map(str::trim).filter(|value| !value.is_empty()) {
        meta.insert(
            "message_id".to_owned(),
            Value::String(message_id.to_owned()),
        );
    }
    Ok(Value::Object(meta))
}

fn group_cipher_body(
    cipher: &anp::group_e2ee::GroupCipherObject,
) -> crate::ImResult<Map<String, Value>> {
    let mut body = Map::new();
    body.insert(
        "crypto_group_id_b64u".to_owned(),
        non_empty_json_string("crypto_group_id_b64u", &cipher.crypto_group_id_b64u)?,
    );
    body.insert(
        "epoch".to_owned(),
        non_empty_json_string("epoch", &cipher.epoch)?,
    );
    body.insert(
        "private_message_b64u".to_owned(),
        non_empty_json_string("private_message_b64u", &cipher.private_message_b64u)?,
    );
    body.insert(
        "group_state_ref".to_owned(),
        serde_json::to_value(&cipher.group_state_ref).map_err(|err| {
            crate::ImError::Serialization {
                detail: err.to_string(),
            }
        })?,
    );
    if let Some(epoch_authenticator) = cipher
        .epoch_authenticator
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        body.insert(
            "epoch_authenticator".to_owned(),
            Value::String(epoch_authenticator.to_owned()),
        );
    }
    Ok(body)
}

fn e2ee_head_body(
    group_did: &str,
    member_did: &str,
    source: &Map<String, Value>,
) -> Map<String, Value> {
    let mut body = Map::new();
    body.insert(
        "group_did".to_owned(),
        Value::String(group_did.trim().to_owned()),
    );
    body.insert(
        "group_state_ref".to_owned(),
        Value::Object(group_state_ref_from_source(group_did, source)),
    );
    copy_keys(
        source,
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
        body.insert("epoch_authenticator".to_owned(), value);
    }
    if !member_did.trim().is_empty() {
        body.insert(
            "member_did".to_owned(),
            Value::String(member_did.trim().to_owned()),
        );
        body.insert(
            "subject_did".to_owned(),
            Value::String(member_did.trim().to_owned()),
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
            body.insert("epoch".to_owned(), value.clone());
        }
    }
    if !body.contains_key("epoch_authenticator") {
        if let Some(value) = prepared_commit.get("epoch_authenticator_b64u") {
            body.insert("epoch_authenticator".to_owned(), value.clone());
        }
    }
    if !body.contains_key("subject_status") && !default_subject_status.is_empty() {
        body.insert(
            "subject_status".to_owned(),
            Value::String(default_subject_status.to_owned()),
        );
    }
    augment_group_state_ref_with_crypto_claims(&mut body, true);
    body
}

fn prepared_commit_map(
    prepared: &anp::group_e2ee::operations::PreparedMlsCommitOutput,
) -> crate::ImResult<Map<String, Value>> {
    serde_json::to_value(prepared)
        .map_err(|err| crate::ImError::Serialization {
            detail: err.to_string(),
        })
        .map(json_object)
}

fn insert_group_state_ref(
    target: &mut Map<String, Value>,
    group_state_ref: Option<&anp::group_e2ee::GroupStateRef>,
) -> crate::ImResult<()> {
    if let Some(group_state_ref) = group_state_ref {
        target.insert(
            "group_state_ref".to_owned(),
            serde_json::to_value(group_state_ref).map_err(|err| crate::ImError::Serialization {
                detail: err.to_string(),
            })?,
        );
    }
    Ok(())
}

fn group_state_ref_from_source(group_did: &str, source: &Map<String, Value>) -> Map<String, Value> {
    let mut reference = Map::new();
    reference.insert(
        "group_did".to_owned(),
        Value::String(group_did.trim().to_owned()),
    );
    if let Some(source_ref) = source.get("group_state_ref").and_then(Value::as_object) {
        for (key, value) in source_ref {
            reference.insert(key.clone(), value.clone());
        }
    }
    reference.insert(
        "group_did".to_owned(),
        Value::String(group_did.trim().to_owned()),
    );
    let version = first_non_empty_value(&[
        source.get("group_state_version"),
        reference.get("group_state_version"),
    ]);
    if !version.is_empty() {
        reference.insert("group_state_version".to_owned(), Value::String(version));
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
            "group_state_ref".to_owned(),
            json!({ "group_did": group_did }),
        );
    }
    let crypto_group_id = string_from_value(body.get("crypto_group_id_b64u"));
    let epoch = if prefer_from_epoch {
        let from_epoch = string_from_value(body.get("from_epoch"));
        if from_epoch.is_empty() {
            string_from_value(body.get("epoch"))
        } else {
            from_epoch
        }
    } else {
        string_from_value(body.get("epoch"))
    };
    if let Some(reference) = body
        .get_mut("group_state_ref")
        .and_then(Value::as_object_mut)
    {
        if !crypto_group_id.is_empty() {
            reference.insert(
                "crypto_group_id_b64u".to_owned(),
                Value::String(crypto_group_id),
            );
        }
        if !epoch.is_empty() {
            reference.insert("epoch".to_owned(), Value::String(epoch));
        }
    }
}

pub(crate) fn sanitize_group_key_package_for_service(
    input: &anp::group_e2ee::GroupKeyPackage,
) -> crate::ImResult<Map<String, Value>> {
    let raw = serde_json::to_value(input).map_err(|err| crate::ImError::Serialization {
        detail: err.to_string(),
    })?;
    let mut output = Map::new();
    let Value::Object(input) = raw else {
        return Ok(output);
    };
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
            output.insert(key.to_owned(), value.clone());
        }
    }
    Ok(output)
}

fn copy_keys(source: &Map<String, Value>, target: &mut Map<String, Value>, keys: &[&str]) {
    for key in keys {
        if let Some(value) = source.get(*key) {
            target.insert((*key).to_owned(), value.clone());
        }
    }
}

fn insert_optional_trimmed_string(target: &mut Map<String, Value>, key: &str, value: Option<&str>) {
    let value = value.map(str::trim).unwrap_or_default();
    if !value.is_empty() {
        target.insert(key.to_owned(), Value::String(value.to_owned()));
    }
}

fn string_from_value(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_default()
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

fn non_empty_json_string(field: &'static str, value: &str) -> crate::ImResult<Value> {
    Ok(Value::String(require_non_empty(field, value)?.to_owned()))
}

fn require_non_empty<'a>(field: &'static str, value: &'a str) -> crate::ImResult<&'a str> {
    let value = value.trim();
    if value.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some(field.to_owned()),
            format!("{field} must not be empty"),
        ));
    }
    Ok(value)
}

fn json_object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_e2ee_wire_builders_follow_p6_targets_and_security_profiles() {
        let credentials = credentials();
        let sender_did = "did:wba:awiki.test:user:alice";
        let service_did = "did:wba:awiki.test";
        let group_did = "did:wba:awiki.test:groups:secure";
        let member_did = "did:wba:awiki.test:user:bob";
        let state_ref = group_state_ref(group_did, "state-7");
        let prepared = prepared_commit("op-create", "7");

        let publish = build_group_e2ee_publish_key_package_rpc_params(
            &credentials,
            sender_did,
            service_did,
            &group_key_package(sender_did),
            "op-publish",
        )
        .expect("publish params");
        assert_meta(
            &publish,
            "service",
            service_did,
            GROUP_E2EE_TRANSPORT_SECURITY_PROFILE,
            "application/json",
        );
        assert_eq!(
            publish["body"]["group_key_package"]["owner_did"],
            sender_did
        );
        assert_eq!(
            publish["body"]["group_key_package"]["did_wba_binding"]["proof"]["verificationMethod"],
            format!("{sender_did}#key-1")
        );

        let create = build_group_e2ee_create_rpc_params(
            &credentials,
            sender_did,
            service_did,
            group_did,
            &prepared,
            Some(&state_ref),
        )
        .expect("create params");
        assert_meta(
            &create,
            "service",
            service_did,
            GROUP_E2EE_SECURITY_PROFILE,
            "application/json",
        );
        assert_eq!(create["body"]["group_did"], group_did);
        assert_eq!(
            create["body"]["group_state_ref"]["group_state_version"],
            "state-7"
        );

        let add = build_group_e2ee_add_rpc_params(
            &credentials,
            sender_did,
            group_did,
            member_did,
            &prepared_commit("op-add", "8"),
            &group_key_package(member_did),
            Some(&group_state_ref(group_did, "state-8")),
        )
        .expect("add params");
        assert_meta(
            &add,
            "group",
            group_did,
            GROUP_E2EE_SECURITY_PROFILE,
            "application/json",
        );
        assert_eq!(add["body"]["member_did"], member_did);
        assert_eq!(add["body"]["subject_did"], member_did);
        assert_eq!(add["body"]["welcome_b64u"], "welcome-op-add");
        assert_eq!(add["body"]["ratchet_tree_b64u"], "tree-op-add");
        assert_eq!(add["body"]["subject_key_package_id"], "kp-member");

        let remove = build_group_e2ee_remove_rpc_params(
            &credentials,
            sender_did,
            group_did,
            member_did,
            &prepared_commit_with_subject_status("op-remove", "9", "removed"),
            Some(&group_state_ref(group_did, "state-9")),
            Some("  removed by owner  "),
            Some("  leave-1  "),
        )
        .expect("remove params");
        assert_meta(
            &remove,
            "group",
            group_did,
            GROUP_E2EE_SECURITY_PROFILE,
            "application/json",
        );
        assert_eq!(remove["body"]["member_did"], member_did);
        assert_eq!(remove["body"]["subject_status"], "removed");
        assert_eq!(remove["body"]["reason_text"], "removed by owner");
        assert_eq!(remove["body"]["leave_request_id"], "leave-1");

        let get_key_package = build_group_e2ee_get_key_package_rpc_params(
            &credentials,
            sender_did,
            service_did,
            group_did,
            member_did,
            Some("update"),
            Some("default"),
        )
        .expect("get key package params");
        assert_meta(
            &get_key_package,
            "service",
            service_did,
            GROUP_E2EE_TRANSPORT_SECURITY_PROFILE,
            "application/json",
        );
        assert_eq!(get_key_package["body"]["target_did"], member_did);
        assert_eq!(get_key_package["body"]["group_did"], group_did);
        assert_eq!(get_key_package["body"]["purpose"], "update");
        assert_eq!(get_key_package["body"]["device_id"], "default");

        let update = build_group_e2ee_update_rpc_params(
            &credentials,
            sender_did,
            group_did,
            member_did,
            "default",
            &prepared_commit("op-update", "10"),
            &group_key_package_with_purpose(member_did, "update"),
            Some(&group_state_ref(group_did, "state-10")),
        )
        .expect("update params");
        assert_meta(
            &update,
            "group",
            group_did,
            GROUP_E2EE_SECURITY_PROFILE,
            "application/json",
        );
        assert_eq!(update["body"]["target"]["agent_did"], member_did);
        assert_eq!(update["body"]["target"]["device_id"], "default");
        assert_eq!(update["body"]["update_key_package_id"], "kp-member");
        assert_eq!(update["body"]["group_key_package"]["purpose"], "update");

        let process_leave = build_group_e2ee_process_leave_request_rpc_params(
            &credentials,
            sender_did,
            group_did,
            "leave-1",
        )
        .expect("process leave params");
        assert_meta(
            &process_leave,
            "group",
            group_did,
            GROUP_E2EE_TRANSPORT_SECURITY_PROFILE,
            "application/json",
        );
        assert_eq!(process_leave["body"]["leave_request_id"], "leave-1");

        let notice = build_group_e2ee_notice_rpc_params(
            &credentials,
            sender_did,
            group_did,
            500,
            true,
            &[
                " notice-1 ".to_owned(),
                " ".to_owned(),
                "notice-2".to_owned(),
            ],
        )
        .expect("notice params");
        assert_meta(
            &notice,
            "agent",
            sender_did,
            GROUP_E2EE_TRANSPORT_SECURITY_PROFILE,
            "application/json",
        );
        assert_eq!(notice["body"]["group_did"], group_did);
        assert_eq!(notice["body"]["limit"], 100);
        assert_eq!(notice["body"]["mark_delivered"], true);
        assert_eq!(
            notice["body"]["notice_ids"],
            json!(["notice-1", "notice-2"])
        );
    }

    #[test]
    fn group_e2ee_send_body_is_direct_cipher_object_not_wrapped() {
        let params = build_group_e2ee_send_rpc_params(
            &credentials(),
            "did:wba:awiki.test:user:alice",
            "did:wba:awiki.test:groups:secure",
            &anp::group_e2ee::GroupCipherObject {
                crypto_group_id_b64u: "crypto-group".to_owned(),
                epoch: "12".to_owned(),
                private_message_b64u: "private-message".to_owned(),
                group_state_ref: group_state_ref("did:wba:awiki.test:groups:secure", "state-12"),
                epoch_authenticator: Some("epoch-auth".to_owned()),
                non_cryptographic: false,
                artifact_mode: None,
            },
            "op-send",
            "msg-send",
        )
        .expect("send params");

        assert_meta(
            &params,
            "group",
            "did:wba:awiki.test:groups:secure",
            GROUP_E2EE_SECURITY_PROFILE,
            GROUP_E2EE_CIPHER_CONTENT_TYPE,
        );
        assert_eq!(params["meta"]["message_id"], "msg-send");
        assert_eq!(params["body"]["crypto_group_id_b64u"], "crypto-group");
        assert_eq!(params["body"]["epoch"], "12");
        assert_eq!(params["body"]["private_message_b64u"], "private-message");
        assert_eq!(
            params["body"]["group_state_ref"]["group_state_version"],
            "state-12"
        );
        assert_eq!(params["body"]["epoch_authenticator"], "epoch-auth");
        assert!(params["body"].get("group_cipher_object").is_none());
        assert!(params
            .get("body")
            .and_then(|body| body.get("body"))
            .is_none());
        assert!(params["auth"]["origin_proof"].is_object());
    }

    fn assert_meta(
        params: &Value,
        target_kind: &str,
        target_did: &str,
        security_profile: &str,
        content_type: &str,
    ) {
        assert_eq!(params["meta"]["profile"], GROUP_E2EE_PROFILE);
        assert_eq!(
            params["meta"]["target"],
            json!({"kind": target_kind, "did": target_did})
        );
        assert_eq!(params["meta"]["security_profile"], security_profile);
        assert_eq!(params["meta"]["content_type"], content_type);
        assert!(params["auth"]["origin_proof"].is_object());
    }

    fn credentials() -> crate::internal::message_runtime::group::GroupTextCredentials {
        let bundle = anp::authentication::create_did_wba_document(
            "awiki.test",
            anp::authentication::DidDocumentOptions {
                path_segments: vec!["user".to_owned(), "alice".to_owned()],
                domain: Some("awiki.test".to_owned()),
                challenge: Some("group-e2ee-wire-test".to_owned()),
                ..anp::authentication::DidDocumentOptions::default()
            },
        )
        .expect("did document");
        let key1_private_pem = bundle
            .private_key_pem("key-1")
            .expect("private key")
            .to_owned();
        crate::internal::message_runtime::group::GroupTextCredentials {
            identity_name: "alice".to_owned(),
            did_document: Some(bundle.did_document),
            key1_private_pem,
            verification_method: None,
        }
    }

    fn prepared_commit(
        operation_id: &str,
        epoch: &str,
    ) -> anp::group_e2ee::operations::PreparedMlsCommitOutput {
        prepared_commit_with_subject_status(operation_id, epoch, "active")
    }

    fn prepared_commit_with_subject_status(
        operation_id: &str,
        epoch: &str,
        subject_status: &str,
    ) -> anp::group_e2ee::operations::PreparedMlsCommitOutput {
        anp::group_e2ee::operations::PreparedMlsCommitOutput {
            pending_commit_id: format!("pending-{operation_id}"),
            operation_id: operation_id.to_owned(),
            status: "prepared".to_owned(),
            actor_did: "did:wba:awiki.test:user:alice".to_owned(),
            subject_did: "did:wba:awiki.test:user:bob".to_owned(),
            subject_status: subject_status.to_owned(),
            group_did: "did:wba:awiki.test:groups:secure".to_owned(),
            commit_b64u: format!("commit-{operation_id}"),
            welcome_b64u: Some(format!("welcome-{operation_id}")),
            ratchet_tree_b64u: Some(format!("tree-{operation_id}")),
            group_info_b64u: Some(format!("group-info-{operation_id}")),
            crypto_group_id_b64u: "crypto-group".to_owned(),
            from_epoch: epoch.to_owned(),
            epoch: epoch.to_owned(),
            to_epoch: epoch.to_owned(),
            local_epoch: epoch.to_owned(),
            epoch_authenticator: Some(format!("auth-{operation_id}")),
            epoch_authenticator_b64u: Some(format!("auth-{operation_id}")),
            suite: anp::group_e2ee::MTI_SUITE.to_owned(),
            member_did: Some("did:wba:awiki.test:user:bob".to_owned()),
        }
    }

    fn group_state_ref(group_did: &str, version: &str) -> anp::group_e2ee::GroupStateRef {
        anp::group_e2ee::GroupStateRef {
            group_did: group_did.to_owned(),
            group_state_version: version.to_owned(),
            policy_hash: None,
        }
    }

    fn group_key_package(owner_did: &str) -> anp::group_e2ee::GroupKeyPackage {
        group_key_package_with_purpose(owner_did, "normal")
    }

    fn group_key_package_with_purpose(
        owner_did: &str,
        purpose: &str,
    ) -> anp::group_e2ee::GroupKeyPackage {
        anp::group_e2ee::GroupKeyPackage {
            key_package_id: "kp-member".to_owned(),
            owner_did: owner_did.to_owned(),
            device_id: Some("default".to_owned()),
            purpose: Some(purpose.to_owned()),
            group_did: None,
            suite: anp::group_e2ee::MTI_SUITE.to_owned(),
            mls_key_package_b64u: "mls-key-package".to_owned(),
            did_wba_binding: json!({
                "agent_did": owner_did,
                "verification_method": format!("{owner_did}#key-1"),
                "leaf_signature_key_b64u": "leaf-key",
                "issued_at": "2026-01-01T00:00:00Z",
                "expires_at": "2099-01-01T00:00:00Z",
                "proof": {
                    "type": "DataIntegrityProof",
                    "verificationMethod": format!("{owner_did}#key-1")
                }
            }),
            expires_at: None,
            non_cryptographic: false,
            artifact_mode: None,
        }
    }
}
