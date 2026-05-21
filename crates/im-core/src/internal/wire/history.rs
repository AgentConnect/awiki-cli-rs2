use serde_json::{json, Map, Value};

use super::common::{self, WireIdentity};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HistoryWireRequest {
    pub peer_did: String,
    pub limit: i64,
    pub cursor: Option<String>,
    pub skip: i64,
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
    body.insert("user_did".to_string(), Value::String(identity.did.clone()));
    body.insert("peer_did".to_string(), Value::String(request.peer_did));
    body.insert("limit".to_string(), json!(limit));
    if let Some(cursor) = request.cursor.filter(|cursor| !cursor.is_empty()) {
        body.insert("since_seq".to_string(), Value::String(cursor));
    }
    if request.skip > 0 {
        body.insert("skip".to_string(), json!(request.skip));
    }
    Ok(json!({
        "meta": common::local_meta(&identity.did, "anp.direct.local.v1"),
        "body": body,
    }))
}
