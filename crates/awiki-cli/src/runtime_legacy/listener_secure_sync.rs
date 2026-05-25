use super::listener_secure_replay::{
    secure_pending_history_replay_candidates, secure_unread_replay_candidates, ReplayStoreLookup,
};
use crate::legacy_identity::types::StoredIdentity;
use im_core::realtime::wire::{self, HistoryWireRequest, InboxWireRequest, WireIdentity};
use serde_json::Value;
use std::time::Duration;

pub const SECURE_DIRECT_SYNC_TIMEOUT: Duration = Duration::from_secs(15);
pub const SECURE_UNREAD_INBOX_LIMIT: i64 = 100;
pub const SECURE_PENDING_HISTORY_LIMIT: i64 = 50;

#[derive(Debug, Clone, PartialEq)]
pub struct SecureSyncRpcCall {
    pub method: String,
    pub params: Value,
    pub timeout: Duration,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SecureSyncAction {
    SendRpc(SecureSyncRpcCall),
    CancelRpcContext,
    HandleNotification { notification: Value },
}

#[derive(Debug, Clone, PartialEq)]
pub struct SecureSyncPlan {
    pub actions: Vec<SecureSyncAction>,
}

pub fn secure_unread_direct_inbox_rpc_call(record: &StoredIdentity) -> SecureSyncRpcCall {
    SecureSyncRpcCall {
        method: "inbox.get".to_string(),
        params: wire::build_inbox_rpc_params(
            &wire_identity(record),
            InboxWireRequest {
                limit: SECURE_UNREAD_INBOX_LIMIT,
            },
        ),
        timeout: SECURE_DIRECT_SYNC_TIMEOUT,
    }
}

pub fn secure_pending_confirmation_history_rpc_call(
    record: &StoredIdentity,
    peer_did: &str,
) -> Result<SecureSyncRpcCall, im_core::ImError> {
    Ok(SecureSyncRpcCall {
        method: "direct.get_history".to_string(),
        params: wire::build_history_rpc_params(
            &wire_identity(record),
            HistoryWireRequest {
                peer_did: peer_did.to_string(),
                limit: SECURE_PENDING_HISTORY_LIMIT,
                cursor: None,
                skip: 0,
            },
        )?,
        timeout: SECURE_DIRECT_SYNC_TIMEOUT,
    })
}

pub fn secure_unread_direct_inbox_replay_actions(
    record: &StoredIdentity,
    rpc_result: &Value,
    mut lookup: impl FnMut(&str, &str, &str) -> ReplayStoreLookup,
) -> Vec<SecureSyncAction> {
    let messages = messages_from_rpc_result(rpc_result);
    replay_actions(secure_unread_replay_candidates(
        &messages,
        &record.did,
        &record.identity_name,
        &mut lookup,
    ))
}

pub fn secure_pending_confirmation_history_replay_actions(
    record: &StoredIdentity,
    rpc_result: &Value,
    mut lookup: impl FnMut(&str, &str, &str) -> ReplayStoreLookup,
) -> Vec<SecureSyncAction> {
    let messages = messages_from_rpc_result(rpc_result);
    replay_actions(secure_pending_history_replay_candidates(
        &messages,
        &record.did,
        &record.identity_name,
        &mut lookup,
    ))
}

pub fn sync_unread_secure_direct_inbox_plan(
    record: Option<&StoredIdentity>,
    rpc_result: Option<&Value>,
    mut lookup: impl FnMut(&str, &str, &str) -> ReplayStoreLookup,
) -> SecureSyncPlan {
    let Some(record) = record else {
        return SecureSyncPlan {
            actions: Vec::new(),
        };
    };
    let mut actions = vec![SecureSyncAction::SendRpc(
        secure_unread_direct_inbox_rpc_call(record),
    )];
    let Some(rpc_result) = rpc_result else {
        actions.push(SecureSyncAction::CancelRpcContext);
        return SecureSyncPlan { actions };
    };
    actions.extend(secure_unread_direct_inbox_replay_actions(
        record,
        rpc_result,
        &mut lookup,
    ));
    actions.push(SecureSyncAction::CancelRpcContext);
    SecureSyncPlan { actions }
}

pub fn sync_pending_confirmation_secure_history_plan(
    record: Option<&StoredIdentity>,
    peer_dids: &[String],
    rpc_results: &[Option<Value>],
    mut lookup: impl FnMut(&str, &str, &str) -> ReplayStoreLookup,
) -> SecureSyncPlan {
    let Some(record) = record else {
        return SecureSyncPlan {
            actions: Vec::new(),
        };
    };
    if peer_dids.is_empty() {
        return SecureSyncPlan {
            actions: Vec::new(),
        };
    }
    let mut actions = Vec::new();
    for (index, peer_did) in peer_dids.iter().enumerate() {
        let Ok(call) = secure_pending_confirmation_history_rpc_call(record, peer_did) else {
            actions.push(SecureSyncAction::CancelRpcContext);
            continue;
        };
        actions.push(SecureSyncAction::SendRpc(call));
        actions.push(SecureSyncAction::CancelRpcContext);
        let Some(Some(rpc_result)) = rpc_results.get(index) else {
            continue;
        };
        actions.extend(secure_pending_confirmation_history_replay_actions(
            record,
            rpc_result,
            &mut lookup,
        ));
    }
    SecureSyncPlan { actions }
}

fn messages_from_rpc_result(result: &Value) -> Vec<Value> {
    result
        .get("messages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn replay_actions(
    candidates: Vec<super::listener_secure_replay::SecureReplayCandidate>,
) -> Vec<SecureSyncAction> {
    candidates
        .into_iter()
        .map(|candidate| SecureSyncAction::HandleNotification {
            notification: candidate.notification,
        })
        .collect()
}

fn wire_identity(record: &StoredIdentity) -> WireIdentity {
    WireIdentity {
        did: record.did.clone(),
    }
}
