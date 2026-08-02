use serde_json::{json, Value};

use super::common::{self, WireIdentity};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InboxWireRequest {
    pub limit: i64,
    pub auth: Option<InboxWireAuth>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InboxWireAuth {
    pub inbox_owner_did: String,
    pub inbox_auth_verification_method: String,
    pub service_did: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MarkReadWireRequest {
    pub message_ids: Vec<String>,
}

pub(crate) fn build_inbox_rpc_params(identity: &WireIdentity, request: InboxWireRequest) -> Value {
    let limit = if request.limit <= 0 {
        20
    } else {
        request.limit
    };
    if let Some(auth) = request.auth {
        return json!({
            "meta": common::delegated_local_meta(
                &auth.inbox_owner_did,
                &auth.service_did,
                "anp.inbox.local.v1",
            ),
            "body": {
                "user_did": auth.inbox_owner_did,
                "limit": limit,
                "inbox_owner_did": auth.inbox_owner_did,
                "inbox_auth_verification_method": auth.inbox_auth_verification_method,
            },
        });
    }
    json!({
        "meta": common::local_meta(&identity.did, "anp.inbox.local.v1"),
        "body": {
            "user_did": identity.did,
            "limit": limit,
        },
    })
}

/// Builds the closed exact-device P5 Inbox hydration request.
///
/// This is deliberately separate from the generic/delegated local Inbox
/// builder: ordinary messages reconcile through Sync v2, while this request
/// may return only Direct E2EE rows for the authenticated device principal.
pub(crate) fn build_exact_device_secure_inbox_rpc_params(
    identity: &WireIdentity,
    limit: u32,
) -> Value {
    json!({
        "meta": common::local_meta(&identity.did, "anp.inbox.local.v1"),
        "body": {
            "user_did": identity.did,
            "limit": limit,
            "security_profile": "direct-e2ee",
        },
    })
}

pub(crate) fn build_mark_read_rpc_params(
    identity: &WireIdentity,
    request: MarkReadWireRequest,
) -> crate::ImResult<Value> {
    if request.message_ids.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("message_ids".to_string()),
            "message not found: message_ids are required",
        ));
    }
    Ok(json!({
        "meta": common::local_meta(&identity.did, "anp.inbox.local.v1"),
        "body": {
            "user_did": identity.did,
            "message_ids": request.message_ids,
        },
    }))
}
