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
        let mut error: JsonRpcResponseError =
            serde_json::from_value(error.clone()).map_err(|err| crate::ImError::Serialization {
                detail: err.to_string(),
            })?;
        let awiki_code = error
            .data
            .as_ref()
            .and_then(Value::as_object)
            .and_then(|data| data.get("awiki_code"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|code| !code.is_empty())
            .map(ToOwned::to_owned);
        if awiki_code.is_some() {
            if let Some(data) = error.data.as_mut().and_then(Value::as_object_mut) {
                data.entry("json_rpc_code".to_owned())
                    .or_insert_with(|| Value::Number(error.code.into()));
            }
        }
        return Err(crate::ImError::Service {
            status_code: None,
            code: Some(awiki_code.unwrap_or_else(|| error.code.to_string())),
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

    #[test]
    fn json_rpc_error_prefers_stable_awiki_code_and_preserves_numeric_code() {
        let raw = serde_json::to_vec(&json!({
            "jsonrpc": "2.0",
            "id": "req-1",
            "error": {
                "code": -32001,
                "message": "device join expired",
                "data": {
                    "awiki_code": "device.join.expired",
                    "join_session_id": "join-1"
                }
            }
        }))
        .unwrap();

        let error = decode_response(&raw).unwrap_err();
        let crate::ImError::Service { code, data, .. } = error else {
            panic!("expected service error")
        };
        assert_eq!(code.as_deref(), Some("device.join.expired"));
        let data = data.unwrap();
        assert_eq!(
            data.get("json_rpc_code").and_then(Value::as_i64),
            Some(-32001)
        );
        assert_eq!(
            data.get("join_session_id").and_then(Value::as_str),
            Some("join-1")
        );
    }

    #[test]
    fn json_rpc_error_without_awiki_code_keeps_numeric_code() {
        let raw = serde_json::to_vec(&json!({
            "jsonrpc": "2.0",
            "id": "req-1",
            "error": {
                "code": 1401,
                "message": "authentication required",
                "data": {"reason": "expired"}
            }
        }))
        .unwrap();

        let error = decode_response(&raw).unwrap_err();
        let crate::ImError::Service { code, data, .. } = error else {
            panic!("expected service error")
        };
        assert_eq!(code.as_deref(), Some("1401"));
        assert!(data
            .as_ref()
            .and_then(|value| value.get("json_rpc_code"))
            .is_none());
    }
}
