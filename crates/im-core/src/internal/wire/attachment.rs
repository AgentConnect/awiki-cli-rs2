use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use super::direct::DirectPayload;

const ATTACHMENT_PROFILE: &str = "anp.attachment.v1";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentCreateSlotResult {
    pub attachment_id: String,
    pub slot_id: String,
    pub upload_uri: String,
    pub upload_headers: Map<String, Value>,
    pub object_uri: String,
    pub commit_token: String,
    pub expires_at: String,
    #[serde(skip)]
    pub request_service_did: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentCommitObjectResult {
    pub committed: bool,
    pub attachment_id: String,
    pub object_uri: String,
    pub committed_at: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentDownloadTicketResult {
    #[serde(default)]
    pub download_ticket_b64u: String,
    #[serde(default)]
    pub expires_at: String,
    #[serde(default)]
    pub ticket_binding: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AttachmentSigningIdentity {
    pub identity_name: String,
    pub did: String,
    pub did_document: Option<Value>,
    pub key1_private_pem: String,
}

pub(crate) fn build_attachment_create_slot_rpc_params(
    sender_did: &str,
    service_did: &str,
    target_kind: &str,
    target_did: &str,
    prepared: &crate::attachments::manifest::PreparedAttachment,
) -> crate::ImResult<Value> {
    build_attachment_create_slot_rpc_params_with_security_profile(
        sender_did,
        service_did,
        target_kind,
        target_did,
        "transport-protected",
        prepared,
    )
}

pub(crate) fn build_attachment_create_slot_rpc_params_with_security_profile(
    sender_did: &str,
    service_did: &str,
    target_kind: &str,
    target_did: &str,
    message_security_profile: &str,
    prepared: &crate::attachments::manifest::PreparedAttachment,
) -> crate::ImResult<Value> {
    if prepared.filename.trim().is_empty() && prepared.size_string.trim().is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("file_path".to_string()),
            "attachment file path is required",
        ));
    }
    if service_did.trim().is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("service_did".to_string()),
            "message service did is required",
        ));
    }
    if target_kind.trim().is_empty() || target_did.trim().is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("target".to_string()),
            "direct message target is required",
        ));
    }
    let message_security_profile = message_security_profile.trim();
    if message_security_profile.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("message_security_profile".to_string()),
            "message security profile is required",
        ));
    }
    if prepared.is_object_e2ee() && message_security_profile == "transport-protected" {
        return Err(crate::ImError::invalid_input(
            Some("message_security_profile".to_string()),
            "object-e2ee attachments require direct-e2ee or group-e2ee message security profile",
        ));
    }
    Ok(json!({
        "meta": super::common::message_meta(sender_did, service_did, ATTACHMENT_PROFILE),
        "body": {
            "expected_size": prepared.size_string,
            "expected_digest": {
                "alg": "sha-256",
                "value_b64u": prepared.digest_b64u,
            },
            "mime_type": prepared.mime_type,
            "filename": prepared.filename,
            "intended_message_security_profile": message_security_profile,
            "intended_target": {
                "kind": target_kind,
                "did": target_did,
            },
            "object_encryption_mode": prepared.object_encryption_mode(),
        },
    }))
}

pub(crate) fn build_attachment_commit_object_rpc_params(
    sender_did: &str,
    service_did: &str,
    prepared: &crate::attachments::manifest::PreparedAttachment,
    slot: &AttachmentCreateSlotResult,
) -> crate::ImResult<Value> {
    if prepared.filename.trim().is_empty() || slot.attachment_id.trim().is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("file_path".to_string()),
            "attachment file path is required",
        ));
    }
    let mut body = Map::new();
    body.insert(
        "attachment_id".to_string(),
        Value::String(slot.attachment_id.clone()),
    );
    body.insert("slot_id".to_string(), Value::String(slot.slot_id.clone()));
    body.insert(
        "commit_token".to_string(),
        Value::String(slot.commit_token.clone()),
    );
    body.insert(
        "size".to_string(),
        Value::String(prepared.size_string.clone()),
    );
    body.insert(
        "object_encryption_mode".to_string(),
        Value::String(prepared.object_encryption_mode()),
    );
    body.insert(
        "digest".to_string(),
        json!({
            "alg": "sha-256",
            "value_b64u": prepared.digest_b64u,
        }),
    );
    if let Some(plaintext_size) = prepared.plaintext_size_string.as_ref() {
        body.insert(
            "plaintext_size".to_string(),
            Value::String(plaintext_size.clone()),
        );
    }
    Ok(json!({
        "meta": super::common::message_meta(sender_did, service_did, ATTACHMENT_PROFILE),
        "body": Value::Object(body),
    }))
}

pub(crate) fn build_attachment_download_ticket_rpc_params(
    requester_did: &str,
    service_did: &str,
    sender_did: &str,
    message_id: &str,
    group_did: &str,
    selection: &crate::attachments::selection::AttachmentSelection,
) -> crate::ImResult<Value> {
    if selection.attachment_id.trim().is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("attachment_id".to_string()),
            crate::attachments::selection::ERR_ATTACHMENT_NOT_FOUND,
        ));
    }
    if service_did.trim().is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("service_did".to_string()),
            "attachment service did is required",
        ));
    }
    if sender_did.trim().is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("sender_did".to_string()),
            "attachment message sender_did is required",
        ));
    }
    let mut body = Map::new();
    body.insert(
        "attachment_id".to_string(),
        Value::String(selection.attachment_id.clone()),
    );
    body.insert(
        "object_uri".to_string(),
        Value::String(selection.object_uri.clone()),
    );
    body.insert(
        "sender_did".to_string(),
        Value::String(sender_did.to_string()),
    );
    body.insert(
        "requester_did".to_string(),
        Value::String(requester_did.to_string()),
    );
    body.insert(
        "message_security_profile".to_string(),
        Value::String("transport-protected".to_string()),
    );
    body.insert(
        "message_id".to_string(),
        Value::String(message_id.to_string()),
    );
    body.insert("one_time".to_string(), Value::Bool(true));
    if !group_did.trim().is_empty() {
        body.insert(
            "group_did".to_string(),
            Value::String(group_did.to_string()),
        );
    } else {
        body.insert(
            "message_target_did".to_string(),
            Value::String(requester_did.to_string()),
        );
    }
    Ok(json!({
        "meta": super::common::message_meta(requester_did, service_did, ATTACHMENT_PROFILE),
        "body": body,
    }))
}

pub(crate) fn build_direct_attachment_send_rpc_params(
    identity: &AttachmentSigningIdentity,
    target_did: &str,
    manifest: Value,
) -> crate::ImResult<Value> {
    if target_did.trim().is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("target_did".to_string()),
            "direct message target is required",
        ));
    }
    build_signed_attachment_send_rpc_params(
        identity,
        "direct.send",
        "agent",
        target_did,
        "anp.direct.base.v1",
        manifest,
    )
}

pub(crate) fn build_group_attachment_send_rpc_params(
    identity: &AttachmentSigningIdentity,
    group_did: &str,
    manifest: Value,
) -> crate::ImResult<Value> {
    if group_did.trim().is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("group_did".to_string()),
            "group target is required",
        ));
    }
    build_signed_attachment_send_rpc_params(
        identity,
        "group.send",
        "group",
        group_did,
        "anp.group.base.v1",
        manifest,
    )
}

fn build_signed_attachment_send_rpc_params(
    identity: &AttachmentSigningIdentity,
    method: &str,
    target_kind: &str,
    target_did: &str,
    profile: &str,
    manifest: Value,
) -> crate::ImResult<Value> {
    let body = json!({ "payload": manifest });
    let payload = DirectPayload {
        method: method.to_string(),
        meta: super::common::signed_message_meta(
            &identity.did,
            target_kind,
            target_did,
            profile,
            crate::attachments::manifest::attachment_manifest_content_type(),
        ),
        body,
    };
    let origin_proof = crate::internal::proof::origin::build_origin_proof(
        &crate::internal::proof::origin::OriginProofIdentity {
            identity_name: identity.identity_name.clone(),
            did_document: identity.did_document.clone(),
            key1_private_pem: identity.key1_private_pem.clone(),
        },
        &payload,
    )?;
    Ok(json!({
        "meta": payload.meta,
        "auth": crate::internal::proof::origin::origin_auth_value(&origin_proof),
        "body": payload.body,
    }))
}
