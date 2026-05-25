use super::listener_service_did::{disconnected_websocket_session_error, ListenerServiceDidRpc};
use super::listener_ws_transport::WsTransport;
use super::listener_wsclient::{
    build_ws_rpc_request, decode_ws_rpc_result, next_ws_rpc_request_id, pending_failure_response,
    ListenerWsDispatchOutcome, ListenerWsPendingDispatch,
};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone)]
pub(super) struct SessionRpcSender {
    pub(super) tx: mpsc::Sender<SessionRpcRequest>,
    pub(super) active: Arc<Mutex<bool>>,
}

#[derive(Default)]
pub(super) struct SessionRpcRegistry {
    senders: BTreeMap<String, SessionRpcSender>,
}

impl SessionRpcRegistry {
    fn set(&mut self, identity_name: &str, sender: SessionRpcSender) {
        self.senders.insert(identity_name.to_string(), sender);
    }

    fn remove(&mut self, identity_name: &str) {
        self.senders.remove(identity_name);
    }

    fn get(&self, identity_name: &str) -> Option<SessionRpcSender> {
        self.senders.get(identity_name).cloned()
    }
}

pub(super) struct SessionRpcRequest {
    method: String,
    params: Map<String, Value>,
    timeout: Option<Duration>,
    response_tx: mpsc::Sender<anyhow::Result<Map<String, Value>>>,
}

pub(super) struct PendingSessionRpc {
    response_tx: mpsc::Sender<anyhow::Result<Map<String, Value>>>,
    expires_at: Option<Instant>,
}

pub(super) struct SessionSharedRpc {
    sender: SessionRpcSender,
}

impl SessionSharedRpc {
    pub(super) fn new(
        registry: &Arc<Mutex<SessionRpcRegistry>>,
        identity_name: &str,
    ) -> anyhow::Result<Self> {
        let sender = registry
            .lock()
            .map(|registry| registry.get(identity_name))
            .unwrap_or(None)
            .ok_or_else(|| {
                anyhow::anyhow!("{}", disconnected_websocket_session_error(identity_name))
            })?;
        Ok(Self { sender })
    }

    pub(super) fn send_rpc_with_timeout(
        &mut self,
        method: &str,
        params: Map<String, Value>,
        timeout: Option<Duration>,
    ) -> anyhow::Result<Map<String, Value>> {
        let (response_tx, response_rx) = mpsc::channel();
        {
            let active = self
                .sender
                .active
                .lock()
                .map_err(|_| anyhow::anyhow!("websocket session rpc loop is closed"))?;
            if !*active {
                anyhow::bail!("websocket session rpc loop is closed");
            }
            self.sender
                .tx
                .send(SessionRpcRequest {
                    method: method.to_string(),
                    params,
                    timeout,
                    response_tx,
                })
                .map_err(|_| anyhow::anyhow!("websocket session rpc loop is closed"))?;
        }
        response_rx
            .recv()
            .map_err(|_| anyhow::anyhow!("websocket session rpc loop is closed"))?
    }
}

impl ListenerServiceDidRpc for SessionSharedRpc {
    fn send_rpc(
        &mut self,
        method: &str,
        params: Map<String, Value>,
    ) -> anyhow::Result<Map<String, Value>> {
        self.send_rpc_with_timeout(method, params, None)
    }
}

pub(super) fn set_session_rpc_sender(
    registry: &Arc<Mutex<SessionRpcRegistry>>,
    identity_name: &str,
    sender: SessionRpcSender,
) {
    if let Ok(mut registry) = registry.lock() {
        registry.set(identity_name, sender);
    }
}

pub(super) fn remove_session_rpc_sender(
    registry: &Arc<Mutex<SessionRpcRegistry>>,
    identity_name: &str,
) {
    if let Ok(mut registry) = registry.lock() {
        registry.remove(identity_name);
    }
}

pub(super) fn drain_session_rpc_requests(
    transport: &mut WsTransport,
    rpc_rx: &mpsc::Receiver<SessionRpcRequest>,
    next_id: &mut i64,
    dispatch: &mut ListenerWsPendingDispatch,
    pending_rpc_responses: &mut BTreeMap<String, PendingSessionRpc>,
) -> anyhow::Result<()> {
    loop {
        match rpc_rx.try_recv() {
            Ok(request) => {
                let request_id = next_ws_rpc_request_id(next_id);
                dispatch.register_pending(request_id.clone());
                pending_rpc_responses.insert(
                    request_id.clone(),
                    PendingSessionRpc {
                        response_tx: request.response_tx,
                        expires_at: request.timeout.map(|timeout| Instant::now() + timeout),
                    },
                );
                let payload =
                    build_ws_rpc_request(&request_id, &request.method, Some(request.params));
                if let Err(err) = transport.send_json(&payload) {
                    let _ = dispatch.remove_pending(&request_id);
                    if let Some(pending) = pending_rpc_responses.remove(&request_id) {
                        let _ = pending.response_tx.send(Err(err));
                    }
                }
            }
            Err(mpsc::TryRecvError::Empty) => return Ok(()),
            Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
        }
    }
}

pub(super) fn route_session_rpc_response(
    message: Map<String, Value>,
    dispatch: &mut ListenerWsPendingDispatch,
    pending_rpc_responses: &mut BTreeMap<String, PendingSessionRpc>,
) {
    let outcome = dispatch.route_incoming_message(message);
    let ListenerWsDispatchOutcome::RoutedResponse { request_id } = outcome else {
        return;
    };
    let Some(response) = dispatch.take_pending_response(&request_id) else {
        return;
    };
    let _ = dispatch.remove_pending(&request_id);
    if let Some(pending) = pending_rpc_responses.remove(&request_id) {
        let _ = pending.response_tx.send(decode_ws_rpc_result(&response));
    }
}

pub(super) fn fail_session_rpc_pending(
    error: &str,
    dispatch: &mut ListenerWsPendingDispatch,
    pending_rpc_responses: &mut BTreeMap<String, PendingSessionRpc>,
) {
    for request_id in dispatch.fail_pending_requests(error) {
        let response = dispatch
            .take_pending_response(&request_id)
            .unwrap_or_else(|| pending_failure_response(&request_id, error));
        let _ = dispatch.remove_pending(&request_id);
        if let Some(pending) = pending_rpc_responses.remove(&request_id) {
            let _ = pending.response_tx.send(decode_ws_rpc_result(&response));
        }
    }
}

pub(super) fn fail_session_rpc_queued(error: &str, rpc_rx: &mpsc::Receiver<SessionRpcRequest>) {
    loop {
        match rpc_rx.try_recv() {
            Ok(request) => {
                let _ = request
                    .response_tx
                    .send(Err(anyhow::anyhow!(error.to_string())));
            }
            Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected) => break,
        }
    }
}

pub(super) fn close_session_rpc_active(active: &Arc<Mutex<bool>>) {
    if let Ok(mut active) = active.lock() {
        *active = false;
    }
}

pub(super) fn expire_session_rpc_pending(
    now: Instant,
    dispatch: &mut ListenerWsPendingDispatch,
    pending_rpc_responses: &mut BTreeMap<String, PendingSessionRpc>,
) {
    let expired = pending_rpc_responses
        .iter()
        .filter_map(|(request_id, pending)| {
            pending
                .expires_at
                .filter(|expires_at| *expires_at <= now)
                .map(|_| request_id.clone())
        })
        .collect::<Vec<_>>();
    for request_id in expired {
        let _ = dispatch.remove_pending(&request_id);
        if let Some(pending) = pending_rpc_responses.remove(&request_id) {
            let _ = pending
                .response_tx
                .send(Err(anyhow::anyhow!("context deadline exceeded")));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::listener_secure_sync::SECURE_DIRECT_SYNC_TIMEOUT;
    use super::*;
    use serde_json::json;
    use std::thread;

    #[test]
    fn session_shared_rpc_sends_timeout_bound_request_through_session_channel() {
        let (tx, rx) = mpsc::channel();
        let mut rpc = SessionSharedRpc {
            sender: SessionRpcSender {
                tx,
                active: Arc::new(Mutex::new(true)),
            },
        };
        let handle = thread::spawn(move || {
            let mut params = Map::new();
            params.insert("limit".to_string(), json!(100));
            rpc.send_rpc_with_timeout("inbox.get", params, Some(SECURE_DIRECT_SYNC_TIMEOUT))
        });

        let request = rx.recv().expect("session rpc request");
        assert_eq!(request.method, "inbox.get");
        assert_eq!(request.timeout, Some(SECURE_DIRECT_SYNC_TIMEOUT));
        assert_eq!(request.params["limit"], 100);
        request
            .response_tx
            .send(Ok(Map::from_iter([("ok".to_string(), json!(true))])))
            .expect("send response");

        let result = handle.join().expect("rpc thread").expect("rpc result");
        assert_eq!(result["ok"], true);
    }

    #[test]
    fn inactive_session_shared_rpc_fails_before_queueing_request() {
        let (tx, rx) = mpsc::channel();
        let mut rpc = SessionSharedRpc {
            sender: SessionRpcSender {
                tx,
                active: Arc::new(Mutex::new(false)),
            },
        };

        let error = rpc
            .send_rpc_with_timeout("inbox.get", Map::new(), Some(SECURE_DIRECT_SYNC_TIMEOUT))
            .expect_err("inactive rpc should fail");

        assert!(error.to_string().contains("rpc loop is closed"));
        assert!(matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
    }

    #[test]
    fn expired_session_rpc_pending_removes_pending_and_wakes_caller() {
        let mut dispatch = ListenerWsPendingDispatch::default();
        dispatch.register_pending("req-1");
        let (response_tx, response_rx) = mpsc::channel();
        let now = Instant::now();
        let mut pending = BTreeMap::from_iter([(
            "req-1".to_string(),
            PendingSessionRpc {
                response_tx,
                expires_at: Some(now - Duration::from_millis(1)),
            },
        )]);

        expire_session_rpc_pending(now, &mut dispatch, &mut pending);

        assert!(!dispatch.has_pending("req-1"));
        assert!(pending.is_empty());
        let error = response_rx
            .recv()
            .expect("timeout response")
            .expect_err("timeout should fail");
        assert_eq!(error.to_string(), "context deadline exceeded");
    }
}
