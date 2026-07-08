use serde::Deserialize;
use serde_json::{json, Value};

pub(crate) const CONTENT_TYPE_JSON: &str = "application/json";
const JSON_RPC_VERSION: &str = "2.0";
const JSON_RPC_ID: &str = "req-1";

#[derive(Debug, Deserialize)]
struct JsonRpcResponseError {
    code: i64,
    message: String,
    #[serde(default)]
    data: Option<Value>,
}

pub(crate) fn build_payload(method: &str, params: Value) -> Value {
    json!({
        "jsonrpc": JSON_RPC_VERSION,
        "id": JSON_RPC_ID,
        "method": method,
        "params": params,
    })
}

pub(crate) fn decode_response(raw: &[u8]) -> crate::ImResult<Value> {
    let envelope: Value =
        serde_json::from_slice(raw).map_err(|err| crate::ImError::Serialization {
            detail: err.to_string(),
        })?;
    if let Some(error) = envelope.get("error").filter(|error| !error.is_null()) {
        let error: JsonRpcResponseError =
            serde_json::from_value(error.clone()).map_err(|err| crate::ImError::Serialization {
                detail: err.to_string(),
            })?;
        return Err(crate::ImError::Service {
            status_code: None,
            code: Some(error.code.to_string()),
            message: error.message,
            data: error.data,
        });
    }
    Ok(envelope.get("result").cloned().unwrap_or(Value::Null))
}

pub(crate) fn decode_plain_response(raw: &[u8]) -> crate::ImResult<Value> {
    serde_json::from_slice(raw).map_err(|err| crate::ImError::Serialization {
        detail: err.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn json_rpc_payload_golden_direct_send() {
        assert_eq!(
            build_payload(
                "direct.send",
                json!({
                    "meta": {
                        "profile": "anp.direct.base.v1",
                        "security_profile": "transport-protected",
                        "sender_did": "did:wba:awiki.ai:user:alice:e1",
                        "target": {
                            "kind": "agent",
                            "did": "did:wba:awiki.ai:user:bob:e1",
                        },
                    },
                    "auth": {
                        "scheme": "did-wba",
                    },
                    "body": {
                        "text": "hello",
                    },
                }),
            ),
            json!({
                "jsonrpc": "2.0",
                "id": "req-1",
                "method": "direct.send",
                "params": {
                    "meta": {
                        "profile": "anp.direct.base.v1",
                        "security_profile": "transport-protected",
                        "sender_did": "did:wba:awiki.ai:user:alice:e1",
                        "target": {
                            "kind": "agent",
                            "did": "did:wba:awiki.ai:user:bob:e1",
                        },
                    },
                    "auth": {
                        "scheme": "did-wba",
                    },
                    "body": {
                        "text": "hello",
                    },
                },
            })
        );
    }

    #[test]
    fn json_rpc_payload_golden_group_send() {
        assert_eq!(
            build_payload(
                "group.send",
                json!({
                    "meta": {
                        "profile": "anp.group.base.v1",
                        "security_profile": "transport-protected",
                        "sender_did": "did:wba:awiki.ai:user:alice:e1",
                        "target": {
                            "kind": "group",
                            "did": "did:wba:awiki.ai:groups:demo:e1_group",
                        },
                    },
                    "auth": {
                        "scheme": "did-wba",
                    },
                    "body": {
                        "text": "hello group",
                    },
                }),
            ),
            json!({
                "jsonrpc": "2.0",
                "id": "req-1",
                "method": "group.send",
                "params": {
                    "meta": {
                        "profile": "anp.group.base.v1",
                        "security_profile": "transport-protected",
                        "sender_did": "did:wba:awiki.ai:user:alice:e1",
                        "target": {
                            "kind": "group",
                            "did": "did:wba:awiki.ai:groups:demo:e1_group",
                        },
                    },
                    "auth": {
                        "scheme": "did-wba",
                    },
                    "body": {
                        "text": "hello group",
                    },
                },
            })
        );
    }

    #[test]
    fn json_rpc_payload_golden_read_methods() {
        let inbox = json!({
            "meta": {
                "profile": "anp.inbox.local.v1",
                "sender_did": "did:wba:awiki.ai:user:alice:e1",
            },
            "body": {
                "user_did": "did:wba:awiki.ai:user:alice:e1",
                "limit": 20,
            },
        });
        assert_eq!(
            build_payload("inbox.get", inbox.clone()),
            json!({
                "jsonrpc": "2.0",
                "id": "req-1",
                "method": "inbox.get",
                "params": inbox,
            })
        );

        let history = json!({
            "meta": {
                "profile": "anp.direct.local.v1",
                "sender_did": "did:wba:awiki.ai:user:alice:e1",
            },
            "body": {
                "user_did": "did:wba:awiki.ai:user:alice:e1",
                "peer_did": "did:wba:awiki.ai:user:bob:e1",
                "limit": 50,
                "since_seq": "42",
            },
        });
        assert_eq!(
            build_payload("direct.get_history", history.clone()),
            json!({
                "jsonrpc": "2.0",
                "id": "req-1",
                "method": "direct.get_history",
                "params": history,
            })
        );

        let mark_read = json!({
            "meta": {
                "profile": "anp.inbox.local.v1",
                "sender_did": "did:wba:awiki.ai:user:alice:e1",
            },
            "body": {
                "user_did": "did:wba:awiki.ai:user:alice:e1",
                "message_ids": ["msg-1", "msg-2"],
            },
        });
        assert_eq!(
            build_payload("inbox.mark_read", mark_read.clone()),
            json!({
                "jsonrpc": "2.0",
                "id": "req-1",
                "method": "inbox.mark_read",
                "params": mark_read,
            })
        );
    }

    #[test]
    fn json_rpc_payload_golden_auth_refresh() {
        assert_eq!(
            build_payload("get_me", json!({})),
            json!({
                "jsonrpc": "2.0",
                "id": "req-1",
                "method": "get_me",
                "params": {},
            })
        );
    }
}
