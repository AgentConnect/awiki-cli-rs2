use serde::Deserialize;
use serde_json::{json, Value};

pub(crate) const CONTENT_TYPE_JSON: &str = "application/json";
const JSON_RPC_VERSION: &str = "2.0";
const JSON_RPC_ID: &str = "req-1";
const PUBLIC_SERVICE_CODE_MAX_LEN: usize = 96;
const PUBLIC_SERVICE_CODE_NAMESPACES: &[&str] = &[
    "anp",
    "attachment",
    "awiki",
    "client",
    "device",
    "direct",
    "group",
    "identity",
    "inbox",
    "read_state",
    "sync",
];

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
        let public_code = select_public_service_code(error.data.as_ref());
        if public_code.is_some() {
            if let Some(data) = error.data.as_mut().and_then(Value::as_object_mut) {
                data.insert("json_rpc_code".to_owned(), Value::Number(error.code.into()));
            }
        }
        return Err(crate::ImError::Service {
            status_code: None,
            code: Some(public_code.unwrap_or_else(|| error.code.to_string())),
            message: error.message,
            data: error.data,
        });
    }
    Ok(envelope.get("result").cloned().unwrap_or(Value::Null))
}

fn select_public_service_code(data: Option<&Value>) -> Option<String> {
    let data = data?.as_object()?;
    let mut selected = None;
    for field in ["awiki_code", "anp_code", "code"] {
        match parse_public_service_code_field(data.get(field)) {
            PublicCodeField::Absent => {}
            PublicCodeField::Invalid => return None,
            PublicCodeField::Valid(code) => {
                if selected.is_some_and(|selected| selected != code) {
                    return None;
                }
                selected = Some(code);
            }
        }
    }
    selected.map(str::to_owned)
}

#[derive(Clone, Copy)]
enum PublicCodeField<'a> {
    Absent,
    Invalid,
    Valid(&'a str),
}

fn parse_public_service_code_field(value: Option<&Value>) -> PublicCodeField<'_> {
    match value {
        None => PublicCodeField::Absent,
        Some(Value::String(code)) if is_public_service_code(code) => PublicCodeField::Valid(code),
        Some(_) => PublicCodeField::Invalid,
    }
}

fn is_public_service_code(code: &str) -> bool {
    if code.is_empty() || code.len() > PUBLIC_SERVICE_CODE_MAX_LEN || !code.is_ascii() {
        return false;
    }
    if !code
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte))
    {
        return false;
    }

    let Some((namespace, suffix)) = code.split_once('.') else {
        return false;
    };
    PUBLIC_SERVICE_CODE_NAMESPACES.contains(&namespace)
        && !suffix.is_empty()
        && code.split('.').all(|segment| !segment.is_empty())
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
                    "json_rpc_code": 999,
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
    fn json_rpc_error_accepts_frozen_join_contract_code() {
        let response = json!({
            "jsonrpc": "2.0",
            "id": "req-1",
            "error": {
                "code": 409,
                "message": "Join request is invalid",
                "data": {
                    "code": "device.join.invalid_request",
                    "retryable": false
                }
            }
        });

        let err = decode_response(&serde_json::to_vec(&response).expect("encode"))
            .expect_err("must reject JSON-RPC error");
        match err {
            crate::ImError::Service { code, data, .. } => {
                assert_eq!(code.as_deref(), Some("device.join.invalid_request"));
                assert_eq!(
                    data.and_then(|value| value.get("json_rpc_code").cloned()),
                    Some(json!(409))
                );
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn json_rpc_error_accepts_stable_anp_code_and_preserves_numeric_code() {
        let raw = serde_json::to_vec(&json!({
            "jsonrpc": "2.0",
            "id": "req-1",
            "error": {
                "code": -32002,
                "message": "device state changed",
                "data": {"anp_code": "anp.device_state_changed"}
            }
        }))
        .unwrap();

        let error = decode_response(&raw).unwrap_err();
        let crate::ImError::Service { code, data, .. } = error else {
            panic!("expected service error")
        };
        assert_eq!(code.as_deref(), Some("anp.device_state_changed"));
        assert_eq!(
            data.and_then(|value| value.get("json_rpc_code").cloned())
                .and_then(|value| value.as_i64()),
            Some(-32002)
        );
    }

    #[test]
    fn json_rpc_error_accepts_matching_public_code_fields() {
        let raw = serde_json::to_vec(&json!({
            "jsonrpc": "2.0",
            "id": "req-1",
            "error": {
                "code": -32003,
                "message": "root control expired",
                "data": {
                    "awiki_code": "awiki.root_control_expired",
                    "anp_code": "awiki.root_control_expired",
                    "code": "awiki.root_control_expired"
                }
            }
        }))
        .unwrap();

        let error = decode_response(&raw).unwrap_err();
        let crate::ImError::Service { code, .. } = error else {
            panic!("expected service error")
        };
        assert_eq!(code.as_deref(), Some("awiki.root_control_expired"));
    }

    #[test]
    fn json_rpc_error_conflicting_public_codes_fail_closed_to_numeric_code() {
        let raw = serde_json::to_vec(&json!({
            "jsonrpc": "2.0",
            "id": "req-1",
            "error": {
                "code": -32004,
                "message": "ambiguous public error",
                "data": {
                    "awiki_code": "device.join.expired",
                    "anp_code": "anp.device_state_changed",
                    "code": "device.join.expired"
                }
            }
        }))
        .unwrap();

        let error = decode_response(&raw).unwrap_err();
        let crate::ImError::Service { code, data, .. } = error else {
            panic!("expected service error")
        };
        assert_eq!(code.as_deref(), Some("-32004"));
        assert!(data
            .as_ref()
            .and_then(|value| value.get("json_rpc_code"))
            .is_none());
    }

    #[test]
    fn json_rpc_error_malformed_or_overlong_public_codes_use_numeric_code() {
        let overlong = format!("anp.{}", "x".repeat(PUBLIC_SERVICE_CODE_MAX_LEN));
        let cases = [
            json!({"awiki_code": "device.join.Expired"}),
            json!({"anp_code": " anp.device_state_changed"}),
            json!({"anp_code": "anp..device_state_changed"}),
            json!({"anp_code": overlong}),
            json!({"code": "device.join.Invalid"}),
            json!({"code": 42}),
            json!({
                "awiki_code": "device.join.expired",
                "anp_code": 42
            }),
        ];

        for data in cases {
            let raw = serde_json::to_vec(&json!({
                "jsonrpc": "2.0",
                "id": "req-1",
                "error": {
                    "code": -32005,
                    "message": "invalid public error code",
                    "data": data
                }
            }))
            .unwrap();

            let error = decode_response(&raw).unwrap_err();
            let crate::ImError::Service { code, .. } = error else {
                panic!("expected service error")
            };
            assert_eq!(code.as_deref(), Some("-32005"));
        }
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
