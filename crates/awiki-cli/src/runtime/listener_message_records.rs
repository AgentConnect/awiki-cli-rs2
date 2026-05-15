use crate::store::{self, MessageRecord};
use serde_json::{Map, Value};

pub fn message_record_from_direct_incoming(
    notification: &Value,
    identity_name: &str,
) -> Option<MessageRecord> {
    if string_value(notification.get("method")) != "direct.incoming" {
        return None;
    }
    let params_value = notification.get("params")?;
    let params = params_value.as_object()?;
    let null = Value::Null;
    let meta_value = object_value(params, "meta").unwrap_or(&null);
    let body_value = object_value(params, "body").unwrap_or(&null);
    let meta = meta_value.as_object();
    let body = body_value.as_object();
    let target = meta.and_then(|meta| object_value(meta, "target"));

    let target_did = string_value(target.and_then(|target| target.get("did")));
    let sender_did = string_from_object(meta, "sender_did");
    if target_did.is_empty() || sender_did.is_empty() {
        return None;
    }

    let mut content_type = string_from_object(meta, "content_type");
    if content_type.is_empty() {
        content_type = "text/plain".to_string();
    }
    let mut sent_at = string_from_object(meta, "created_at");
    if sent_at.is_empty() {
        sent_at = store::now_utc();
    }

    let content_value = direct_content_value(body_value, body);
    let mut content = string_value(Some(&content_value));
    if content.is_empty() {
        content = metadata_value(&content_value);
    }

    Some(MessageRecord {
        msg_id: string_from_object(meta, "message_id"),
        owner_did: target_did.clone(),
        thread_id: store::make_thread_id(&target_did, &sender_did, ""),
        direction: 0,
        sender_did,
        receiver_did: target_did,
        content_type,
        content,
        is_e2ee: string_from_object(meta, "security_profile") == "direct-e2ee"
            || string_from_object(Some(params), "secure_state") == "decrypted",
        sent_at,
        is_read: false,
        metadata: metadata_value(params_value),
        credential_name: identity_name.to_string(),
        ..MessageRecord::default()
    })
}

pub fn message_record_from_group_incoming(
    notification: &Value,
    identity_name: &str,
) -> Option<MessageRecord> {
    if string_value(notification.get("method")) != "group.incoming" {
        return None;
    }
    let params_value = notification.get("params")?;
    let params = params_value.as_object()?;
    let null = Value::Null;
    let meta_value = object_value(params, "meta").unwrap_or(&null);
    let body_value = object_value(params, "body").unwrap_or(&null);
    let meta = meta_value.as_object();
    let body = body_value.as_object();
    let target = meta.and_then(|meta| object_value(meta, "target"));

    let owner_did = string_value(target.and_then(|target| target.get("did")));
    let group_did = string_from_object(body, "group_did");
    let sender_did = string_from_object(meta, "sender_did");
    if owner_did.is_empty() || group_did.is_empty() {
        return None;
    }

    let mut content = string_from_object(body, "text");
    if content.is_empty() {
        content = metadata_value(value_from_object(body, "payload").unwrap_or(&Value::Null));
    }
    let mut content_type = string_from_object(meta, "content_type");
    if content_type.is_empty() {
        content_type = "text/plain".to_string();
    }
    let mut sent_at = string_from_object(body, "accepted_at");
    if sent_at.is_empty() {
        sent_at = string_from_object(meta, "created_at");
    }
    let sent_by_self = sender_did == owner_did;

    Some(MessageRecord {
        msg_id: fallback_string(
            string_from_object(meta, "message_id"),
            format!(
                "{group_did}:{}",
                string_from_object(body, "group_event_seq")
            ),
        ),
        owner_did: owner_did.clone(),
        thread_id: store::make_thread_id(&owner_did, "", &group_did),
        direction: bool_to_direction(sent_by_self),
        sender_did,
        group_id: group_did.clone(),
        group_did,
        content_type,
        content,
        server_seq: int64_value(value_from_object(body, "group_event_seq")),
        sent_at,
        is_read: sent_by_self,
        metadata: metadata_value(params_value),
        credential_name: identity_name.to_string(),
        ..MessageRecord::default()
    })
}

fn direct_content_value(body_value: &Value, body: Option<&Map<String, Value>>) -> Value {
    let text_value = value_from_object(body, "text");
    if !string_value(text_value).is_empty() {
        return text_value.cloned().unwrap_or(Value::Null);
    }
    if let Some(payload) = value_from_object(body, "payload").filter(|value| !value.is_null()) {
        return payload.clone();
    }
    if let Some(payload_b64u) = value_from_object(body, "payload_b64u") {
        if !string_value(Some(payload_b64u)).is_empty() {
            return payload_b64u.clone();
        }
    }
    body_value.clone()
}

fn object_value<'a>(object: &'a Map<String, Value>, key: &str) -> Option<&'a Value> {
    object.get(key).filter(|value| value.as_object().is_some())
}

fn value_from_object<'a>(object: Option<&'a Map<String, Value>>, key: &str) -> Option<&'a Value> {
    object.and_then(|object| object.get(key))
}

fn string_from_object(object: Option<&Map<String, Value>>, key: &str) -> String {
    string_value(value_from_object(object, key))
}

fn string_value(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) => value.clone(),
        _ => String::new(),
    }
}

fn metadata_value(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

fn fallback_string(value: String, fallback: String) -> String {
    if value.trim().is_empty() {
        fallback
    } else {
        value
    }
}

fn int64_value(value: Option<&Value>) -> Option<i64> {
    match value {
        Some(Value::Number(number)) => number.as_i64().or_else(|| {
            number
                .as_u64()
                .and_then(|value| i64::try_from(value).ok())
                .or_else(|| number.as_f64().map(|value| value as i64))
        }),
        Some(Value::String(value)) if !value.is_empty() => value.parse::<i64>().ok(),
        _ => None,
    }
}

fn bool_to_direction(sent_by_self: bool) -> i64 {
    if sent_by_self {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use time::format_description::well_known::Rfc3339;
    use time::OffsetDateTime;

    #[test]
    fn direct_incoming_uses_protocol_fields_only() {
        let notification = json!({
            "jsonrpc": "2.0",
            "method": "direct.incoming",
            "server": {"ignored": true},
            "params": {
                "meta": {
                    "sender_did": "did:wba:example.com:user:bob:e1_yyy",
                    "message_id": "msg-001",
                    "created_at": "2026-04-07T00:00:00Z",
                    "content_type": "text/plain",
                    "target": {
                        "kind": "agent",
                        "did": "did:wba:example.com:user:alice:e1_xxx"
                    }
                },
                "auth": {
                    "scheme": "anp-rfc9421-origin-proof-v1",
                    "origin_proof": {
                        "contentDigest": "sha-256=:digest:",
                        "signatureInput": "sig1=(\"@method\");created=1;keyid=\"did:wba:example.com:user:bob:e1_yyy#key-1\"",
                        "signature": "sig1=:signature:"
                    }
                },
                "body": {
                    "text": "hello back"
                }
            }
        });

        let record =
            message_record_from_direct_incoming(&notification, "alice").expect("direct record");

        assert_eq!(record.msg_id, "msg-001");
        assert_eq!(record.owner_did, "did:wba:example.com:user:alice:e1_xxx");
        assert_eq!(
            record.thread_id,
            "dm:did:wba:example.com:user:alice:e1_xxx:did:wba:example.com:user:bob:e1_yyy"
        );
        assert_eq!(record.direction, 0);
        assert_eq!(record.sender_did, "did:wba:example.com:user:bob:e1_yyy");
        assert_eq!(record.receiver_did, "did:wba:example.com:user:alice:e1_xxx");
        assert_eq!(record.content_type, "text/plain");
        assert_eq!(record.content, "hello back");
        assert!(!record.is_e2ee);
        assert_eq!(record.sent_at, "2026-04-07T00:00:00Z");
        assert!(!record.is_read);
        assert!(record.metadata.contains("anp-rfc9421-origin-proof-v1"));
        assert!(!record.metadata.contains("\"server\""));
        assert_eq!(record.credential_name, "alice");
    }

    #[test]
    fn direct_incoming_rejects_non_direct_or_missing_required_fields() {
        assert!(
            message_record_from_direct_incoming(&json!({"method": "group.incoming"}), "alice",)
                .is_none()
        );
        assert!(message_record_from_direct_incoming(
            &json!({"method": "direct.incoming", "params": []}),
            "alice",
        )
        .is_none());
        assert!(message_record_from_direct_incoming(
            &json!({
                "method": "direct.incoming",
                "params": {
                    "meta": {
                        "sender_did": "did:sender",
                        "target": {"did": ""}
                    },
                    "body": {"text": "hello"}
                }
            }),
            "alice",
        )
        .is_none());
    }

    #[test]
    fn direct_incoming_defaults_and_content_fallbacks_match_go() {
        let payload_record = message_record_from_direct_incoming(
            &json!({
                "method": "direct.incoming",
                "params": {
                    "secure_state": "decrypted",
                    "meta": {
                        "sender_did": "did:sender",
                        "target": {"did": "did:owner"}
                    },
                    "body": {"payload": {"kind": "custom", "count": 2}}
                }
            }),
            "alice",
        )
        .expect("payload record");
        assert_eq!(payload_record.content_type, "text/plain");
        assert_eq!(payload_record.content, r#"{"count":2,"kind":"custom"}"#);
        assert!(payload_record.is_e2ee);
        assert_go_rfc3339_timestamp(&payload_record.sent_at);

        let payload_b64u_record = message_record_from_direct_incoming(
            &json!({
                "method": "direct.incoming",
                "params": {
                    "meta": {
                        "sender_did": "did:sender",
                        "target": {"did": "did:owner"}
                    },
                    "body": {"payload_b64u": "YWJj"}
                }
            }),
            "alice",
        )
        .expect("payload_b64u record");
        assert_eq!(payload_b64u_record.content, "YWJj");

        let empty_payload_record = message_record_from_direct_incoming(
            &json!({
                "method": "direct.incoming",
                "params": {
                    "meta": {
                        "sender_did": "did:sender",
                        "security_profile": "direct-e2ee",
                        "target": {"did": "did:owner"}
                    },
                    "body": {"text": "", "payload": ""}
                }
            }),
            "alice",
        )
        .expect("empty payload record");
        assert_eq!(empty_payload_record.content, r#""""#);
        assert!(empty_payload_record.is_e2ee);
    }

    #[test]
    fn group_incoming_uses_protocol_fields_only() {
        let notification = json!({
            "jsonrpc": "2.0",
            "method": "group.incoming",
            "server": {"ignored": true},
            "params": {
                "meta": {
                    "sender_did": "did:wba:example.com:user:bob:e1_bob",
                    "message_id": "msg-group-001",
                    "content_type": "text/plain",
                    "target": {
                        "kind": "agent",
                        "did": "did:wba:example.com:user:alice:e1_alice"
                    }
                },
                "body": {
                    "text": "hello group",
                    "group_did": "did:wba:example.com:groups:demo:e1_group",
                    "group_event_seq": "5",
                    "accepted_at": "2026-04-07T09:11:01Z"
                }
            }
        });

        let record =
            message_record_from_group_incoming(&notification, "alice").expect("group record");

        assert_eq!(record.msg_id, "msg-group-001");
        assert_eq!(record.owner_did, "did:wba:example.com:user:alice:e1_alice");
        assert_eq!(
            record.thread_id,
            "group:did:wba:example.com:groups:demo:e1_group"
        );
        assert_eq!(record.direction, 0);
        assert_eq!(record.sender_did, "did:wba:example.com:user:bob:e1_bob");
        assert_eq!(record.group_id, "did:wba:example.com:groups:demo:e1_group");
        assert_eq!(record.group_did, "did:wba:example.com:groups:demo:e1_group");
        assert_eq!(record.content_type, "text/plain");
        assert_eq!(record.content, "hello group");
        assert_eq!(record.server_seq, Some(5));
        assert_eq!(record.sent_at, "2026-04-07T09:11:01Z");
        assert!(!record.is_read);
        assert!(!record.metadata.contains("\"server\""));
        assert_eq!(record.credential_name, "alice");
    }

    #[test]
    fn group_incoming_rejects_non_group_or_missing_required_fields() {
        assert!(
            message_record_from_group_incoming(&json!({"method": "direct.incoming"}), "alice",)
                .is_none()
        );
        assert!(message_record_from_group_incoming(
            &json!({"method": "group.incoming", "params": []}),
            "alice",
        )
        .is_none());
        assert!(message_record_from_group_incoming(
            &json!({
                "method": "group.incoming",
                "params": {
                    "meta": {"target": {"did": "did:owner"}},
                    "body": {"group_did": ""}
                }
            }),
            "alice",
        )
        .is_none());
    }

    #[test]
    fn group_incoming_defaults_self_sent_and_numeric_seq_match_go() {
        let record = message_record_from_group_incoming(
            &json!({
                "method": "group.incoming",
                "params": {
                    "meta": {
                        "sender_did": "did:owner",
                        "created_at": "2026-04-07T09:11:01Z",
                        "target": {"did": "did:owner"}
                    },
                    "body": {
                        "group_did": "did:group",
                        "group_event_seq": 7,
                        "payload": {"notice": "fallback"}
                    }
                }
            }),
            "alice",
        )
        .expect("group record");

        assert_eq!(record.msg_id, "did:group:");
        assert_eq!(record.direction, 1);
        assert!(record.is_read);
        assert_eq!(record.content_type, "text/plain");
        assert_eq!(record.content, r#"{"notice":"fallback"}"#);
        assert_eq!(record.server_seq, Some(7));
        assert_eq!(record.sent_at, "2026-04-07T09:11:01Z");

        let missing_payload = message_record_from_group_incoming(
            &json!({
                "method": "group.incoming",
                "params": {
                    "meta": {
                        "sender_did": "did:peer",
                        "target": {"did": "did:owner"}
                    },
                    "body": {
                        "group_did": "did:group",
                        "group_event_seq": "8"
                    }
                }
            }),
            "alice",
        )
        .expect("missing payload record");
        assert_eq!(missing_payload.msg_id, "did:group:8");
        assert_eq!(missing_payload.content, "null");
        assert_eq!(missing_payload.server_seq, Some(8));
    }

    fn assert_go_rfc3339_timestamp(value: &str) {
        assert_eq!(value.len(), "2026-05-14T11:38:35Z".len());
        assert!(value.ends_with('Z'));
        assert!(!value.contains('.'));
        OffsetDateTime::parse(value, &Rfc3339).expect("timestamp should parse as RFC3339");
    }
}
