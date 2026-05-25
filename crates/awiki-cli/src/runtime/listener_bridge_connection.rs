use crate::legacy_identity::types::StoredIdentity;
use crate::runtime::bridge::{handle_bridge_connection_once, BridgeRequest};
use crate::runtime::listener_bridge_dispatch::build_bridge_rpc_call;
use crate::runtime::listener_service_did::disconnected_websocket_session_error;
use serde_json::{Map, Value};
use std::io;

#[derive(Debug, Clone)]
pub struct ListenerBridgeSession {
    pub identity_name: String,
    pub record: Option<StoredIdentity>,
    pub has_client: bool,
}

impl ListenerBridgeSession {
    pub fn connected(identity_name: impl Into<String>, record: StoredIdentity) -> Self {
        Self {
            identity_name: identity_name.into(),
            record: Some(record),
            has_client: true,
        }
    }

    pub fn disconnected(identity_name: impl Into<String>, record: Option<StoredIdentity>) -> Self {
        Self {
            identity_name: identity_name.into(),
            record,
            has_client: false,
        }
    }
}

pub trait ListenerBridgeRuntime {
    fn ensure_session(&mut self, identity_name: &str) -> anyhow::Result<ListenerBridgeSession>;

    fn fetch_message_service_did(
        &mut self,
        session: &ListenerBridgeSession,
    ) -> anyhow::Result<String>;

    fn send_rpc(
        &mut self,
        session: &ListenerBridgeSession,
        method: &str,
        params: Value,
    ) -> anyhow::Result<Map<String, Value>>;

    fn mark_messages_read(&mut self, owner_did: &str, message_ids: &[String])
        -> anyhow::Result<()>;
}

pub fn execute_listener_bridge_request<R>(
    runtime: &mut R,
    request: BridgeRequest,
) -> anyhow::Result<Map<String, Value>>
where
    R: ListenerBridgeRuntime,
{
    let session = runtime.ensure_session(&request.identity_name)?;
    let record = match (&session.record, session.has_client) {
        (Some(record), true) => record,
        _ => anyhow::bail!(
            "{}",
            disconnected_websocket_session_error(&session.identity_name)
        ),
    };

    let service_did = if request.method == "group.create" {
        runtime.fetch_message_service_did(&session)?
    } else {
        String::new()
    };

    let rpc_call = build_bridge_rpc_call(record, &service_did, &request)?;
    let result = runtime.send_rpc(&session, &rpc_call.method, rpc_call.params)?;

    if request.method == "inbox.mark_read" {
        let _ = runtime.mark_messages_read(&record.did, &rpc_call.mark_read_message_ids);
    }

    Ok(result)
}

pub fn handle_listener_bridge_connection_once<RW, R>(stream: RW, runtime: &mut R) -> io::Result<()>
where
    RW: io::Read + io::Write,
    R: ListenerBridgeRuntime,
{
    handle_bridge_connection_once(stream, |request| {
        execute_listener_bridge_request(runtime, request)
    })
}
