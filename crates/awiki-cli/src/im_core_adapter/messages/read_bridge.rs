use im_core::prelude::{AuthScope, HistoryQuery, InboxQuery, InboxScope, SessionBundle, ThreadRef};
use serde_json::{json, Value};

use crate::config::Resolved;
use crate::identity::Manager;
use crate::message::{self, MessageError};
use crate::transportcfg::Profile;

pub fn read_inbox_via_im_core(
    resolved: &Resolved,
    manager: &Manager,
    client: &im_core::ImClient,
    identity_name: &str,
    query: InboxQuery,
) -> Result<message::CommandResult, MessageError> {
    let record = message::require_active_identity(resolved, manager, identity_name)?;
    let publish_warnings = message::maybe_publish_secure_prekeys(resolved, manager, &record);
    let bridge_result = im_core::compat::messages::read_inbox_with_bridge(
        client,
        ReadSessionProvider {
            subject: client.did().clone(),
            resolved,
            manager,
            record: record.clone(),
        },
        ReadLegacyTransport {
            resolved,
            manager,
            record: record.clone(),
        },
        im_core::compat::messages::InboxReadBridgeRequest {
            query: query.clone(),
        },
    )
    .map_err(super::im_error_to_message_error)?;
    let mut warnings = publish_warnings;
    warnings.extend(
        bridge_result
            .page
            .items
            .into_iter()
            .flat_map(|_| Vec::<String>::new()),
    );
    let mut messages = message::persist_inbox_messages(
        resolved,
        manager,
        &record,
        &bridge_result.raw,
        "",
        &mut warnings,
    );
    let source = source_with_default_for_mode(&bridge_result.raw, message::runtime_mode(resolved));
    messages =
        message::apply_inbox_filters(messages, "", query.unread_only, i64::from(query.limit.0));
    let total = messages.len();
    let data = match query.scope {
        InboxScope::DirectOnly => json!({
            "messages": messages,
            "total": total,
            "source": source,
            "with": "",
        }),
        InboxScope::All | InboxScope::GroupOnly => json!({
            "messages": messages,
            "total": total,
            "source": source,
        }),
    };
    Ok(message::CommandResult {
        data,
        summary: format!("Loaded {total} inbox messages"),
        warnings: message::compact_warnings(warnings),
    })
}

pub fn read_history_via_im_core(
    resolved: &Resolved,
    manager: &Manager,
    client: &im_core::ImClient,
    identity_name: &str,
    thread: ThreadRef,
    query: HistoryQuery,
) -> Result<message::CommandResult, MessageError> {
    let record = message::require_active_identity(resolved, manager, identity_name)?;
    let publish_warnings = message::maybe_publish_secure_prekeys(resolved, manager, &record);
    let (thread, target, target_is_handle) = resolve_history_thread(resolved, thread)?;
    let bridge_result = im_core::compat::messages::read_history_with_bridge(
        client,
        ReadSessionProvider {
            subject: client.did().clone(),
            resolved,
            manager,
            record: record.clone(),
        },
        ReadLegacyTransport {
            resolved,
            manager,
            record: record.clone(),
        },
        im_core::compat::messages::HistoryReadBridgeRequest {
            thread,
            query: query.clone(),
            resolved_peer_did: Some(target.did.clone()),
        },
    )
    .map_err(super::im_error_to_message_error)?;
    let mut warnings = publish_warnings;
    warnings.extend(
        bridge_result
            .page
            .items
            .into_iter()
            .flat_map(|_| Vec::<String>::new()),
    );
    let mut messages = message::persist_history_messages(
        resolved,
        manager,
        &record,
        &target.did,
        &target.handle,
        &bridge_result.raw,
        &mut warnings,
    );
    let mut source =
        source_with_default_for_mode(&bridge_result.raw, message::runtime_mode(resolved));
    let mut resolved_dids = message::resolved_dids_value(&bridge_result.raw);
    if target_is_handle {
        let dids = message::merge_handle_history_messages(
            resolved,
            &record.did,
            &target,
            i64::from(query.limit.0),
            false,
            false,
            &mut messages,
            &mut source,
            &mut warnings,
        );
        if let Some(dids) = dids {
            resolved_dids = json!(dids);
        }
    }
    let total = messages.len();
    Ok(message::CommandResult {
        data: json!({
            "messages": messages,
            "total": total,
            "source": source,
            "with": message::peer_handle_or_did(&target),
            "resolved_dids": resolved_dids,
        }),
        summary: format!("Loaded {total} direct history messages"),
        warnings: message::compact_warnings(warnings),
    })
}

struct ReadSessionProvider<'a> {
    subject: im_core::prelude::Did,
    resolved: &'a Resolved,
    manager: &'a Manager,
    record: crate::identity::types::StoredIdentity,
}

impl im_core::compat::messages::BridgeSessionProvider for ReadSessionProvider<'_> {
    fn ensure_messaging_session(&self) -> im_core::ImResult<SessionBundle> {
        let session = message::auth_session(self.resolved, self.manager, &self.record)
            .map_err(super::message_error_to_im_error)?;
        Ok(SessionBundle {
            subject: self.subject.clone(),
            scope: AuthScope::Messaging,
            expires_at: None,
            refreshed: session.current_jwt().trim() != self.record.jwt_token.trim(),
        })
    }
}

struct ReadLegacyTransport<'a> {
    resolved: &'a Resolved,
    manager: &'a Manager,
    record: crate::identity::types::StoredIdentity,
}

impl im_core::compat::messages::BridgeAuthenticatedRpcTransport for ReadLegacyTransport<'_> {
    fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> im_core::ImResult<Value> {
        send_authenticated_read_rpc_with_fallback(
            self.resolved,
            self.manager,
            &self.record,
            endpoint,
            method,
            params,
        )
        .map_err(super::message_error_to_im_error)
    }
}

fn send_authenticated_read_rpc_with_fallback(
    resolved: &Resolved,
    manager: &Manager,
    record: &crate::identity::types::StoredIdentity,
    endpoint: &str,
    method: &str,
    params: Value,
) -> Result<Value, MessageError> {
    match send_authenticated_read_rpc(resolved, manager, record, endpoint, method, params.clone()) {
        Ok(result) => Ok(result),
        Err(err) if message::is_session_unauthorized(&err) => {
            let refreshed = message::refresh_jwt_fallback(resolved, manager, record).ok();
            match send_authenticated_read_rpc(
                resolved,
                manager,
                refreshed.as_ref().unwrap_or(record),
                endpoint,
                method,
                params,
            ) {
                Ok(result) => Ok(result),
                Err(_) => Err(err),
            }
        }
        Err(err) => Err(err),
    }
}

fn send_authenticated_read_rpc(
    resolved: &Resolved,
    manager: &Manager,
    record: &crate::identity::types::StoredIdentity,
    endpoint: &str,
    method: &str,
    params: Value,
) -> Result<Value, MessageError> {
    let mut auth = message::auth_session(resolved, manager, record)?;
    let client = message::Client::new(resolved)?;
    client.authenticated_rpc_call_profile(
        Profile::RpcReadHeavy,
        endpoint,
        method,
        params,
        &mut auth,
    )
}

fn resolve_history_thread(
    resolved: &Resolved,
    thread: ThreadRef,
) -> Result<(ThreadRef, message::TargetResolution, bool), MessageError> {
    let ThreadRef::Direct(peer) = thread else {
        return Err(MessageError::GroupNotSupported);
    };
    let original = peer.as_str().trim().to_string();
    let target_is_handle = !original.is_empty() && !original.starts_with("did:");
    let target = message::resolve_target(resolved, &original)?;
    Ok((ThreadRef::Direct(peer), target, target_is_handle))
}

fn source_with_default_for_mode(raw: &Value, mode: &str) -> String {
    raw.get("source")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(if mode == crate::runtime::bridge::MODE_WEBSOCKET {
            "local_ws_cache"
        } else {
            "remote_http"
        })
        .to_string()
}
