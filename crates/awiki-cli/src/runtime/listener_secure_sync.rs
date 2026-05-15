use super::listener_secure_replay::{
    secure_pending_history_replay_candidates, secure_unread_replay_candidates, ReplayStoreLookup,
    SecureReplayCandidate,
};
use crate::identity::types::StoredIdentity;
use crate::message::{
    build_history_rpc_params, build_inbox_rpc_params, HistoryRequest, InboxRequest,
};
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
    let mut actions = vec![SecureSyncAction::SendRpc(SecureSyncRpcCall {
        method: "inbox.get".to_string(),
        params: build_inbox_rpc_params(
            record,
            InboxRequest {
                scope: "direct".to_string(),
                unread_only: true,
                limit: SECURE_UNREAD_INBOX_LIMIT,
                ..InboxRequest::default()
            },
        ),
        timeout: SECURE_DIRECT_SYNC_TIMEOUT,
    })];
    let Some(rpc_result) = rpc_result else {
        actions.push(SecureSyncAction::CancelRpcContext);
        return SecureSyncPlan { actions };
    };
    let messages = messages_from_rpc_result(rpc_result);
    extend_replay_actions(
        &mut actions,
        secure_unread_replay_candidates(&messages, &record.did, &record.identity_name, &mut lookup),
    );
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
        let Ok(params) = build_history_rpc_params(
            record,
            HistoryRequest {
                with: peer_did.clone(),
                limit: SECURE_PENDING_HISTORY_LIMIT,
                ..HistoryRequest::default()
            },
        ) else {
            actions.push(SecureSyncAction::CancelRpcContext);
            continue;
        };
        actions.push(SecureSyncAction::SendRpc(SecureSyncRpcCall {
            method: "direct.get_history".to_string(),
            params,
            timeout: SECURE_DIRECT_SYNC_TIMEOUT,
        }));
        actions.push(SecureSyncAction::CancelRpcContext);
        let Some(Some(rpc_result)) = rpc_results.get(index) else {
            continue;
        };
        let messages = messages_from_rpc_result(rpc_result);
        extend_replay_actions(
            &mut actions,
            secure_pending_history_replay_candidates(
                &messages,
                &record.did,
                &record.identity_name,
                &mut lookup,
            ),
        );
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

fn extend_replay_actions(
    actions: &mut Vec<SecureSyncAction>,
    candidates: Vec<SecureReplayCandidate>,
) {
    actions.extend(
        candidates
            .into_iter()
            .map(|candidate| SecureSyncAction::HandleNotification {
                notification: candidate.notification,
            }),
    );
}
