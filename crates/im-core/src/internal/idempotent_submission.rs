use serde_json::Value;

use crate::internal::transport::{
    AsyncAuthenticatedRpcTransport, AsyncRpcTransport, AuthenticatedRpcTransport, RpcTransport,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RpcReplayIdentity {
    Message,
    Operation,
}

pub(crate) fn submit_idempotent_rpc<T>(
    transport: &mut T,
    endpoint: &str,
    method: &str,
    params: Value,
    identity: RpcReplayIdentity,
) -> crate::ImResult<Value>
where
    T: AuthenticatedRpcTransport,
{
    let can_replay = has_replay_identity(&params, identity);
    match transport.authenticated_rpc(endpoint, method, params.clone()) {
        Err(crate::ImError::TransportUnavailable { .. }) if can_replay => {
            transport.authenticated_rpc(endpoint, method, params)
        }
        result => result,
    }
}

pub(crate) async fn submit_idempotent_rpc_async<T>(
    transport: &mut T,
    endpoint: &str,
    method: &str,
    params: Value,
    identity: RpcReplayIdentity,
) -> crate::ImResult<Value>
where
    T: AsyncAuthenticatedRpcTransport,
{
    let can_replay = has_replay_identity(&params, identity);
    match transport
        .authenticated_rpc(endpoint, method, params.clone())
        .await
    {
        Err(crate::ImError::TransportUnavailable { .. }) if can_replay => {
            transport.authenticated_rpc(endpoint, method, params).await
        }
        result => result,
    }
}

pub(crate) fn submit_read_rpc<T>(
    transport: &mut T,
    endpoint: &str,
    method: &str,
    params: Value,
) -> crate::ImResult<Value>
where
    T: RpcTransport,
{
    match transport.rpc(endpoint, method, params.clone()) {
        Err(crate::ImError::TransportUnavailable { .. }) => transport.rpc(endpoint, method, params),
        result => result,
    }
}

pub(crate) async fn submit_read_rpc_async<T>(
    transport: &mut T,
    endpoint: &str,
    method: &str,
    params: Value,
) -> crate::ImResult<Value>
where
    T: AsyncRpcTransport,
{
    match transport.rpc(endpoint, method, params.clone()).await {
        Err(crate::ImError::TransportUnavailable { .. }) => {
            transport.rpc(endpoint, method, params).await
        }
        result => result,
    }
}

fn has_replay_identity(params: &Value, identity: RpcReplayIdentity) -> bool {
    let required = match identity {
        RpcReplayIdentity::Message => &["message_id", "operation_id"][..],
        RpcReplayIdentity::Operation => &["operation_id"][..],
    };
    required.iter().all(|field| {
        params
            .get("meta")
            .and_then(|meta| meta.get(field))
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty())
    })
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use serde_json::{json, Value};

    use super::{
        submit_idempotent_rpc, submit_idempotent_rpc_async, submit_read_rpc, submit_read_rpc_async,
        RpcReplayIdentity,
    };
    use crate::internal::transport::{
        AsyncAuthenticatedRpcTransport, AsyncRpcTransport, AuthenticatedRpcTransport, RpcTransport,
    };

    struct RecordingTransport {
        calls: Rc<RefCell<Vec<Value>>>,
        results: Vec<crate::ImResult<Value>>,
    }

    impl AuthenticatedRpcTransport for RecordingTransport {
        fn authenticated_rpc(
            &mut self,
            _endpoint: &str,
            _method: &str,
            params: Value,
        ) -> crate::ImResult<Value> {
            self.calls.borrow_mut().push(params);
            self.results.remove(0)
        }
    }

    impl AsyncAuthenticatedRpcTransport for RecordingTransport {
        async fn authenticated_rpc(
            &mut self,
            endpoint: &str,
            method: &str,
            params: Value,
        ) -> crate::ImResult<Value> {
            AuthenticatedRpcTransport::authenticated_rpc(self, endpoint, method, params)
        }
    }

    impl RpcTransport for RecordingTransport {
        fn rpc(&mut self, endpoint: &str, method: &str, params: Value) -> crate::ImResult<Value> {
            AuthenticatedRpcTransport::authenticated_rpc(self, endpoint, method, params)
        }
    }

    impl AsyncRpcTransport for RecordingTransport {
        async fn rpc(
            &mut self,
            endpoint: &str,
            method: &str,
            params: Value,
        ) -> crate::ImResult<Value> {
            AuthenticatedRpcTransport::authenticated_rpc(self, endpoint, method, params)
        }
    }

    #[test]
    fn message_submit_replays_the_exact_request_once() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let params = message_params();
        let result = submit_idempotent_rpc(
            &mut RecordingTransport {
                calls: Rc::clone(&calls),
                results: vec![
                    transport_unavailable(),
                    Ok(json!({"delivery_state": "accepted"})),
                ],
            },
            "/im/rpc",
            "direct.send",
            params.clone(),
            RpcReplayIdentity::Message,
        )
        .unwrap();

        assert_eq!(result, json!({"delivery_state": "accepted"}));
        assert_eq!(calls.borrow().as_slice(), &[params.clone(), params]);
    }

    #[tokio::test]
    async fn operation_submit_async_replays_the_exact_request_once() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let params = json!({"meta": {"operation_id": "op-1"}});
        let result = submit_idempotent_rpc_async(
            &mut RecordingTransport {
                calls: Rc::clone(&calls),
                results: vec![
                    transport_unavailable(),
                    Ok(json!({"group_did": "did:example:group"})),
                ],
            },
            "/im/rpc",
            "group.create",
            params.clone(),
            RpcReplayIdentity::Operation,
        )
        .await
        .unwrap();

        assert_eq!(result, json!({"group_did": "did:example:group"}));
        assert_eq!(calls.borrow().as_slice(), &[params.clone(), params]);
    }

    #[test]
    fn read_submit_replays_the_exact_request_once() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let params = json!({"handle": "alice.awiki.info"});
        let result = submit_read_rpc(
            &mut RecordingTransport {
                calls: Rc::clone(&calls),
                results: vec![
                    transport_unavailable(),
                    Ok(json!({"did": "did:example:alice"})),
                ],
            },
            "/user-service/handle/rpc",
            "lookup",
            params.clone(),
        )
        .unwrap();

        assert_eq!(result, json!({"did": "did:example:alice"}));
        assert_eq!(calls.borrow().as_slice(), &[params.clone(), params]);
    }

    #[tokio::test]
    async fn read_submit_async_replays_the_exact_request_once() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let params = json!({"handle": "alice.awiki.info"});
        let result = submit_read_rpc_async(
            &mut RecordingTransport {
                calls: Rc::clone(&calls),
                results: vec![
                    transport_unavailable(),
                    Ok(json!({"did": "did:example:alice"})),
                ],
            },
            "/user-service/handle/rpc",
            "lookup",
            params.clone(),
        )
        .await
        .unwrap();

        assert_eq!(result, json!({"did": "did:example:alice"}));
        assert_eq!(calls.borrow().as_slice(), &[params.clone(), params]);
    }

    #[test]
    fn read_service_failure_is_not_replayed() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let error = submit_read_rpc(
            &mut RecordingTransport {
                calls: Rc::clone(&calls),
                results: vec![Err(crate::ImError::Service {
                    status_code: Some(503),
                    code: Some("service_unavailable".to_owned()),
                    message: "retry later".to_owned(),
                    data: None,
                })],
            },
            "/user-service/handle/rpc",
            "lookup",
            json!({"handle": "alice.awiki.info"}),
        )
        .unwrap_err();

        assert!(matches!(error, crate::ImError::Service { .. }));
        assert_eq!(calls.borrow().len(), 1);
    }

    #[test]
    fn message_submit_without_message_id_is_not_replayed() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let error = submit_idempotent_rpc(
            &mut RecordingTransport {
                calls: Rc::clone(&calls),
                results: vec![transport_unavailable()],
            },
            "/im/rpc",
            "direct.send",
            json!({"meta": {"operation_id": "op-1"}}),
            RpcReplayIdentity::Message,
        )
        .unwrap_err();

        assert!(matches!(error, crate::ImError::TransportUnavailable { .. }));
        assert_eq!(calls.borrow().len(), 1);
    }

    #[test]
    fn service_failure_is_not_replayed() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let error = submit_idempotent_rpc(
            &mut RecordingTransport {
                calls: Rc::clone(&calls),
                results: vec![Err(crate::ImError::Service {
                    status_code: Some(503),
                    code: Some("service_unavailable".to_owned()),
                    message: "retry later".to_owned(),
                    data: None,
                })],
            },
            "/im/rpc",
            "direct.send",
            message_params(),
            RpcReplayIdentity::Message,
        )
        .unwrap_err();

        assert!(matches!(error, crate::ImError::Service { .. }));
        assert_eq!(calls.borrow().len(), 1);
    }

    fn message_params() -> Value {
        json!({
            "meta": {
                "message_id": "msg-1",
                "operation_id": "op-1"
            },
            "body": {"text": "hello"}
        })
    }

    fn transport_unavailable() -> crate::ImResult<Value> {
        Err(crate::ImError::TransportUnavailable {
            detail: "connection reset after submit".to_owned(),
        })
    }
}
