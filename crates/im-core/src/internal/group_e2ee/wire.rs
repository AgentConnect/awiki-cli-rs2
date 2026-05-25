use serde_json::{json, Map, Value};

use crate::internal::wire::direct::DirectPayload;

pub(crate) const GROUP_E2EE_PROFILE: &str = anp::group_e2ee::PROFILE;
pub(crate) const GROUP_E2EE_SECURITY_PROFILE: &str = anp::group_e2ee::SECURITY_PROFILE;
pub(crate) const GROUP_E2EE_TRANSPORT_SECURITY_PROFILE: &str =
    anp::group_e2ee::TRANSPORT_SECURITY_PROFILE;
pub(crate) const GROUP_E2EE_CIPHER_CONTENT_TYPE: &str = anp::group_e2ee::GROUP_CIPHER_CONTENT_TYPE;

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
) -> crate::ImResult<Value> {
    let service_did = require_non_empty("service_did", service_did)?;
    let group_did = require_non_empty("group_did", group_did)?;
    let target_did = require_non_empty("target_did", target_did)?;
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
        json!({
            "target_did": target_did,
            "group_did": group_did,
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
        },
        &payload,
    )?;
    Ok(json!({
        "meta": payload.meta,
        "auth": crate::internal::proof::origin::origin_auth_value(&origin_proof),
        "body": payload.body,
    }))
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
