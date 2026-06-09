use serde_json::{json, Map, Value};

use super::common::{self, WireIdentity};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HistoryWireRequest {
    pub peer_did: String,
    pub limit: i64,
    pub cursor: Option<String>,
    pub skip: i64,
    pub auth: Option<HistoryWireAuth>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HistoryWireAuth {
    pub inbox_owner_did: String,
    pub inbox_auth_verification_method: String,
    pub service_did: String,
}

pub(crate) fn build_history_rpc_params(
    identity: &WireIdentity,
    request: HistoryWireRequest,
) -> crate::ImResult<Value> {
    if request.peer_did.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("peer_did".to_string()),
            "peer_did must not be empty",
        ));
    }
    let limit = if request.limit <= 0 {
        50
    } else {
        request.limit
    };
    let mut body = Map::new();
    let owner_did = request
        .auth
        .as_ref()
        .map(|auth| auth.inbox_owner_did.clone())
        .unwrap_or_else(|| identity.did.clone());
    let service_did = request.auth.as_ref().map(|auth| auth.service_did.clone());
    let delegated = request.auth.is_some();
    body.insert("user_did".to_string(), Value::String(owner_did.clone()));
    body.insert("peer_did".to_string(), Value::String(request.peer_did));
    body.insert("limit".to_string(), json!(limit));
    if let Some(cursor) = request.cursor.filter(|cursor| !cursor.is_empty()) {
        body.insert("since_seq".to_string(), Value::String(cursor));
    }
    if request.skip > 0 {
        body.insert("skip".to_string(), json!(request.skip));
    }
    if let Some(auth) = request.auth {
        body.insert(
            "inbox_owner_did".to_string(),
            Value::String(auth.inbox_owner_did),
        );
        body.insert(
            "inbox_auth_verification_method".to_string(),
            Value::String(auth.inbox_auth_verification_method),
        );
    }
    Ok(json!({
        "meta": if delegated {
            common::delegated_local_meta(
                &owner_did,
                service_did.as_deref().unwrap_or(""),
                "anp.direct.local.v1",
            )
        } else {
            common::local_meta(&owner_did, "anp.direct.local.v1")
        },
        "body": body,
    }))
}
