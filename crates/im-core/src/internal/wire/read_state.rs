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
}

pub(crate) fn build_mark_read_state_rpc_params(
    identity: &WireIdentity,
    request: MarkReadStateWireRequest,
) -> crate::ImResult<Value> {
    let user_did = required_string("user_did", identity.did.as_str())?;
    let thread = thread_to_wire(request.thread)?;
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
    if let Some(device_id) = request
        .device_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        body.insert("device_id".to_owned(), Value::String(device_id.to_owned()));
    }

    Ok(json!({
        "meta": common::local_meta(&user_did, READ_STATE_PROFILE),
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
        crate::messages::ThreadRef::Thread(_) => {
            Err(crate::ImError::unsupported("read-state-raw-thread-ref"))
        }
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
