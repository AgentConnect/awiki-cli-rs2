use serde_json::Value;

pub(crate) trait AuthenticatedRpcTransport {
    fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> crate::ImResult<Value>;
}

pub(crate) struct UnavailableTransport;

impl AuthenticatedRpcTransport for UnavailableTransport {
    fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        _params: Value,
    ) -> crate::ImResult<Value> {
        Err(crate::ImError::TransportUnavailable {
            detail: format!("{method} transport is not configured for {endpoint}"),
        })
    }
}
