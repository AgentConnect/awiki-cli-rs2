use serde_json::{json, Map, Value};

use super::common::{self, WireIdentity};

pub(crate) const READ_STATE_PROFILE: &str = "anp.read_state.local.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MarkReadStateWireRequest {
    pub(crate) thread: crate::messages::ThreadRef,
    pub(crate) read_up_to_server_seq: Option<String>,
    pub(crate) read_up_to_message_id: Option<String>,
    pub(crate) client_observed_at: Option<String>,
    pub(crate) fallback_max_message_ids: Option<u32>,
    pub(crate) device_id: Option<String>,
    pub(crate) operation_id: Option<String>,
    pub(crate) remote_thread_key: Option<String>,
}

pub(crate) fn build_mark_read_state_rpc_params(
    identity: &WireIdentity,
    request: MarkReadStateWireRequest,
) -> crate::ImResult<Value> {
    let user_did = required_string("user_did", identity.did.as_str())?;
    let remote_thread_key = request
        .remote_thread_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let thread = match remote_thread_key {
        Some(thread_key) => {
            let kind = match &request.thread {
                crate::messages::ThreadRef::Group(_) => "group",
                crate::messages::ThreadRef::Direct(_) | crate::messages::ThreadRef::Thread(_) => {
                    "direct"
                }
            };
            json!({"kind": kind, "thread_key": thread_key})
        }
        None => thread_to_wire(request.thread)?,
    };
    let read_up_to_server_seq = request
        .read_up_to_server_seq
        .as_deref()
        .map(normalize_decimal_seq)
        .transpose()?;
    let read_up_to_message_id = request
        .read_up_to_message_id
        .as_deref()
        .map(|value| required_string("read_up_to_message_id", value))
        .transpose()?;
    if read_up_to_server_seq.is_none() && read_up_to_message_id.is_none() {
        return Err(crate::ImError::invalid_input(
            Some("watermark".to_owned()),
            "read_up_to_server_seq or read_up_to_message_id is required",
        ));
    }

    let mut body = Map::new();
    body.insert("user_did".to_owned(), Value::String(user_did.clone()));
    body.insert("thread".to_owned(), thread);
    if let Some(seq) = read_up_to_server_seq {
        body.insert("read_up_to_server_seq".to_owned(), Value::String(seq));
    }
    if let Some(message_id) = read_up_to_message_id {
        body.insert(
            "read_up_to_message_id".to_owned(),
            Value::String(message_id),
        );
    }
    if let Some(observed_at) = request
        .client_observed_at
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        body.insert(
            "client_observed_at".to_owned(),
            Value::String(observed_at.to_owned()),
        );
    }
    if let Some(limit) = request.fallback_max_message_ids {
        body.insert(
            "fallback_max_message_ids".to_owned(),
            json!(limit.clamp(1, 500)),
        );
    }
    let operation_id = request
        .operation_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if remote_thread_key.is_none() {
        if let Some(device_id) = request
            .device_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            body.insert("device_id".to_owned(), Value::String(device_id.to_owned()));
        }
        if let Some(operation_id) = operation_id {
            body.insert(
                "operation_id".to_owned(),
                Value::String(operation_id.to_owned()),
            );
        }
    }

    let mut meta = common::local_meta(&user_did, READ_STATE_PROFILE);
    if remote_thread_key.is_some() {
        if let Some(operation_id) = operation_id {
            meta["operation_id"] = Value::String(operation_id.to_owned());
        }
    }
    Ok(json!({
        "meta": meta,
        "body": body,
    }))
}

fn thread_to_wire(thread: crate::messages::ThreadRef) -> crate::ImResult<Value> {
    match thread {
        crate::messages::ThreadRef::Direct(peer) => {
            let peer_did = required_string("thread.peer_did", peer.as_str())?;
            Ok(json!({
                "kind": "direct",
                "peer_did": peer_did,
            }))
        }
        crate::messages::ThreadRef::Group(group) => {
            let group_did = required_string("thread.group_did", group.as_str())?;
            Ok(json!({
                "kind": "group",
                "group_did": group_did,
            }))
        }
        crate::messages::ThreadRef::Thread(_) => Err(crate::ImError::invalid_input(
            Some("thread".to_owned()),
            "read_state.mark_read remote thread must resolve to direct or group",
        )),
    }
}

fn normalize_decimal_seq(value: &str) -> crate::ImResult<String> {
    crate::internal::local_state::sync_state::normalize_decimal_seq(value)
        .map_err(|_| invalid_decimal("read_up_to_server_seq", value))
}

fn required_string(field: &str, value: &str) -> crate::ImResult<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some(field.to_owned()),
            format!("{field} is required"),
        ));
    }
    Ok(value.to_owned())
}

fn invalid_decimal(field: &str, value: &str) -> crate::ImError {
    crate::ImError::invalid_input(
        Some(field.to_owned()),
        format!("{field} must be a non-negative decimal string, got {value:?}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mark_read_state_rejects_raw_storage_thread_wire_shape() {
        let identity = WireIdentity {
            did: "did:example:alice".to_owned(),
        };

        let err = build_mark_read_state_rpc_params(
            &identity,
            MarkReadStateWireRequest {
                thread: crate::messages::ThreadRef::Thread(
                    crate::ids::ThreadId::parse("dm:peer-scope:v1:abc").unwrap(),
                ),
                read_up_to_server_seq: Some("42".to_owned()),
                read_up_to_message_id: None,
                client_observed_at: None,
                fallback_max_message_ids: None,
                device_id: None,
                operation_id: None,
                remote_thread_key: None,
            },
        )
        .unwrap_err();

        assert!(matches!(
            err,
            crate::ImError::InvalidInput {
                field: Some(ref field),
                ..
            } if field == "thread"
        ));
    }

    #[test]
    fn v2_remote_thread_uses_stable_meta_operation_without_body_selectors() {
        let params = build_mark_read_state_rpc_params(
            &WireIdentity {
                did: "did:example:alice".to_owned(),
            },
            MarkReadStateWireRequest {
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                ),
                read_up_to_server_seq: Some("42".to_owned()),
                read_up_to_message_id: Some("message-42".to_owned()),
                client_observed_at: Some("2026-07-28T12:00:00Z".to_owned()),
                fallback_max_message_ids: Some(100),
                device_id: Some("must-not-be-sent".to_owned()),
                operation_id: Some("op-read-stable".to_owned()),
                remote_thread_key: Some("conversation-ref-bob".to_owned()),
            },
        )
        .unwrap();

        assert_eq!(params["meta"]["operation_id"], "op-read-stable");
        assert_eq!(
            params["body"],
            json!({
                "user_did": "did:example:alice",
                "thread": {
                    "kind": "direct",
                    "thread_key": "conversation-ref-bob"
                },
                "read_up_to_server_seq": "42",
                "read_up_to_message_id": "message-42",
                "client_observed_at": "2026-07-28T12:00:00Z",
                "fallback_max_message_ids": 100
            })
        );
    }
}
