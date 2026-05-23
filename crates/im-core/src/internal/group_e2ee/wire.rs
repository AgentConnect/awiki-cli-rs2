use serde_json::{json, Map, Value};

use crate::internal::wire::direct::DirectPayload;

pub(crate) const GROUP_E2EE_PROFILE: &str = anp::group_e2ee::PROFILE;
pub(crate) const GROUP_E2EE_SECURITY_PROFILE: &str = anp::group_e2ee::SECURITY_PROFILE;
pub(crate) const GROUP_E2EE_TRANSPORT_SECURITY_PROFILE: &str =
    anp::group_e2ee::TRANSPORT_SECURITY_PROFILE;
pub(crate) const GROUP_E2EE_CIPHER_CONTENT_TYPE: &str =
    anp::group_e2ee::commands::GROUP_CIPHER_CONTENT_TYPE;

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
