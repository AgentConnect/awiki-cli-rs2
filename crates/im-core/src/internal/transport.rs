use serde_json::Value;

pub(crate) trait AuthenticatedRpcTransport {
    fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> crate::ImResult<Value>;
}

pub(crate) trait RpcTransport {
    fn rpc(&mut self, endpoint: &str, method: &str, params: Value) -> crate::ImResult<Value>;
}

pub(crate) trait AuthenticatedRestTransport {
    fn authenticated_rest_post(
        &mut self,
        endpoint: &str,
        method: &str,
        body: Value,
    ) -> crate::ImResult<Value>;

    fn authenticated_rest_get(
        &mut self,
        endpoint: &str,
        method: &str,
        query: &std::collections::BTreeMap<String, String>,
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

impl RpcTransport for UnavailableTransport {
    fn rpc(&mut self, endpoint: &str, method: &str, _params: Value) -> crate::ImResult<Value> {
        Err(crate::ImError::TransportUnavailable {
            detail: format!("{method} transport is not configured for {endpoint}"),
        })
    }
}

impl AuthenticatedRestTransport for UnavailableTransport {
    fn authenticated_rest_post(
        &mut self,
        endpoint: &str,
        method: &str,
        _body: Value,
    ) -> crate::ImResult<Value> {
        Err(crate::ImError::TransportUnavailable {
            detail: format!("{method} transport is not configured for {endpoint}"),
        })
    }

    fn authenticated_rest_get(
        &mut self,
        endpoint: &str,
        method: &str,
        _query: &std::collections::BTreeMap<String, String>,
    ) -> crate::ImResult<Value> {
        Err(crate::ImError::TransportUnavailable {
            detail: format!("{method} transport is not configured for {endpoint}"),
        })
    }
}
