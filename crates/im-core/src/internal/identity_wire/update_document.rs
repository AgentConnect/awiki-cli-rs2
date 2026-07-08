use serde_json::{Map, Value};

pub(crate) fn build_update_document_rpc_call(
    params: super::UpdateDocumentRpcParams,
) -> super::RpcCall {
    let mut payload = Map::new();
    payload.insert("did_document".to_string(), params.did_document);
    if let Some(value) = params.is_public {
        payload.insert("is_public".to_string(), Value::Bool(value));
    }
    if let Some(value) = params.is_agent {
        payload.insert("is_agent".to_string(), Value::Bool(value));
    }
    if let Some(value) = params.role {
        payload.insert("role".to_string(), super::nullable_trimmed(value));
    }
    if let Some(value) = params.endpoint_url {
        payload.insert("endpoint_url".to_string(), super::nullable_trimmed(value));
    }
    super::rpc_call(
        super::DID_AUTH_RPC_ENDPOINT,
        "update_document",
        super::TransportProfile::RpcDefault,
        Value::Object(payload),
    )
}
