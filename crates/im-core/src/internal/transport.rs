use serde_json::Value;
use std::collections::BTreeMap;

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

pub(crate) trait RestTransport {
    fn rest_post(&mut self, endpoint: &str, method: &str, body: Value) -> crate::ImResult<Value>;

    fn rest_get(
        &mut self,
        endpoint: &str,
        method: &str,
        query: &std::collections::BTreeMap<String, String>,
    ) -> crate::ImResult<Value>;
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

pub(crate) struct CoreHttpTransport<'a> {
    client: &'a crate::core::ImClient,
    http: crate::internal::http::HttpClient,
    auth: anp::authentication::DIDWbaAuthHeader,
    jwt_token: Option<String>,
}

pub(crate) struct CorePlainTransport<'a> {
    core: &'a crate::core::ImCore,
    http: crate::internal::http::HttpClient,
}

impl<'a> CoreHttpTransport<'a> {
    pub(crate) fn new(client: &'a crate::core::ImClient) -> Self {
        let runtime = client.runtime();
        let jwt_token = read_jwt_token(&runtime.auth_state_path).ok().flatten();
        let mut auth = anp::authentication::DIDWbaAuthHeader::new(
            &runtime.did_document_path,
            &runtime.private_key_path,
            anp::authentication::AuthMode::HttpSignatures,
        );
        if let Some(token) = jwt_token.as_deref() {
            auth.update_token(
                client.core_inner().sdk_config().service_base_url.as_str(),
                &BTreeMap::from([("Authorization".to_string(), format!("Bearer {token}"))]),
            );
        }
        Self {
            client,
            http: crate::internal::http::HttpClient::new(),
            auth,
            jwt_token,
        }
    }

    fn rpc_url(&self, endpoint: &str) -> String {
        let base = if endpoint.starts_with("/im/") {
            self.client
                .core_inner()
                .sdk_config()
                .message_service_endpoint
                .as_ref()
                .unwrap_or(&self.client.core_inner().sdk_config().service_base_url)
        } else {
            self.client
                .core_inner()
                .sdk_config()
                .user_service_endpoint
                .as_ref()
                .unwrap_or(&self.client.core_inner().sdk_config().service_base_url)
        };
        join_base_url(base.as_str(), endpoint)
    }

    fn plain_rpc(&mut self, endpoint: &str, method: &str, params: Value) -> crate::ImResult<Value> {
        let url = self.rpc_url(endpoint);
        let body = serde_json::to_vec(&crate::internal::json_rpc::build_payload(method, params))
            .map_err(|err| crate::ImError::Serialization {
                detail: err.to_string(),
            })?;
        let response = self.execute_json_request("POST", &url, body, false)?;
        if response.status_code >= 400 {
            return Err(service_error_from_http(
                response.status_code,
                &response.body,
            ));
        }
        crate::internal::json_rpc::decode_response(&response.body)
    }

    fn authenticated_rpc_inner(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> crate::ImResult<Value> {
        let url = self.rpc_url(endpoint);
        let body = serde_json::to_vec(&crate::internal::json_rpc::build_payload(
            method,
            params.clone(),
        ))
        .map_err(|err| crate::ImError::Serialization {
            detail: err.to_string(),
        })?;
        let response = self.execute_json_request("POST", &url, body, false)?;
        if response.status_code >= 400 {
            return Err(service_error_from_http(
                response.status_code,
                &response.body,
            ));
        }
        crate::internal::json_rpc::decode_response(&response.body)
    }

    fn authenticated_rest(
        &mut self,
        endpoint: &str,
        method: &str,
        body: Vec<u8>,
        query: Option<&BTreeMap<String, String>>,
        signed: bool,
    ) -> crate::ImResult<Value> {
        let mut url = self.rpc_url(endpoint);
        if let Some(query) = query {
            url = append_query(&url, query);
        }
        let response = if signed {
            self.execute_json_request(method, &url, body, false)?
        } else {
            self.execute_unsigned_json_request(method, &url, body)?
        };
        if response.status_code >= 400 {
            return Err(service_error_from_http(
                response.status_code,
                &response.body,
            ));
        }
        crate::internal::json_rpc::decode_plain_response(&response.body)
    }

    fn execute_unsigned_json_request(
        &self,
        method: &str,
        url: &str,
        body: Vec<u8>,
    ) -> crate::ImResult<crate::internal::http::HttpResponse> {
        let request = crate::internal::http::HttpRequest {
            method: method.to_string(),
            url: url.to_string(),
            headers: BTreeMap::from([(
                "Content-Type".to_string(),
                crate::internal::json_rpc::CONTENT_TYPE_JSON.to_string(),
            )]),
            body,
        };
        self.http.execute(request)
    }

    fn execute_json_request(
        &mut self,
        method: &str,
        url: &str,
        body: Vec<u8>,
        force_new_auth: bool,
    ) -> crate::ImResult<crate::internal::http::HttpResponse> {
        let mut headers = BTreeMap::from([(
            "Content-Type".to_string(),
            crate::internal::json_rpc::CONTENT_TYPE_JSON.to_string(),
        )]);
        if let Some(token) = self.jwt_token.as_deref().filter(|token| !token.is_empty()) {
            self.auth.update_token(
                url,
                &BTreeMap::from([("Authorization".to_string(), format!("Bearer {token}"))]),
            );
        }
        let auth_headers = self
            .auth
            .get_auth_header(url, force_new_auth, method, Some(&headers), Some(&body))
            .map_err(|err| crate::ImError::TransportUnavailable {
                detail: err.to_string(),
            })?;
        headers.extend(auth_headers);
        let request = crate::internal::http::HttpRequest {
            method: method.to_string(),
            url: url.to_string(),
            headers,
            body: body.clone(),
        };
        let mut response = self.http.execute(request)?;
        if response.status_code == 401 {
            let headers = if self.auth.should_retry_after_401(&response.headers) {
                self.challenge_headers(url, method, &response.headers, body.as_slice())?
            } else {
                self.auth.clear_token(url);
                self.jwt_token = None;
                self.auth_headers(url, method, body.as_slice(), true)?
            };
            let request = crate::internal::http::HttpRequest {
                method: method.to_string(),
                url: url.to_string(),
                headers,
                body,
            };
            response = self.http.execute(request)?;
        }
        self.capture_token(url, &response.headers);
        Ok(response)
    }

    fn auth_headers(
        &mut self,
        url: &str,
        method: &str,
        body: &[u8],
        force_new: bool,
    ) -> crate::ImResult<BTreeMap<String, String>> {
        let mut headers = BTreeMap::from([(
            "Content-Type".to_string(),
            crate::internal::json_rpc::CONTENT_TYPE_JSON.to_string(),
        )]);
        let auth_headers = self
            .auth
            .get_auth_header(url, force_new, method, Some(&headers), Some(body))
            .map_err(|err| crate::ImError::TransportUnavailable {
                detail: err.to_string(),
            })?;
        headers.extend(auth_headers);
        Ok(headers)
    }

    fn challenge_headers(
        &mut self,
        url: &str,
        method: &str,
        response_headers: &BTreeMap<String, String>,
        body: &[u8],
    ) -> crate::ImResult<BTreeMap<String, String>> {
        let mut headers = BTreeMap::from([(
            "Content-Type".to_string(),
            crate::internal::json_rpc::CONTENT_TYPE_JSON.to_string(),
        )]);
        let auth_headers = self
            .auth
            .get_challenge_auth_header(url, response_headers, method, Some(&headers), Some(body))
            .map_err(|err| crate::ImError::TransportUnavailable {
                detail: err.to_string(),
            })?;
        headers.extend(auth_headers);
        Ok(headers)
    }

    pub(crate) fn refresh_jwt(&mut self) -> crate::ImResult<String> {
        let endpoint = crate::internal::identity_wire::DID_AUTH_RPC_ENDPOINT;
        let url = self.rpc_url(endpoint);
        self.auth.clear_token(&url);
        self.jwt_token = None;
        let body = serde_json::to_vec(&crate::internal::json_rpc::build_payload(
            "get_me",
            serde_json::json!({}),
        ))
        .map_err(|err| crate::ImError::Serialization {
            detail: err.to_string(),
        })?;
        let response = self.execute_json_request("POST", &url, body, true)?;
        if response.status_code >= 400 {
            return Err(service_error_from_http(
                response.status_code,
                &response.body,
            ));
        }
        let result = crate::internal::json_rpc::decode_response(&response.body)?;
        let token = result
            .get("access_token")
            .and_then(Value::as_str)
            .or(self.jwt_token.as_deref())
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .map(ToOwned::to_owned)
            .ok_or(crate::ImError::AuthRequired)?;
        self.jwt_token = Some(token.clone());
        persist_jwt_token(&self.client.runtime().auth_state_path, &token)?;
        self.auth.update_token(
            &url,
            &BTreeMap::from([("Authorization".to_string(), format!("Bearer {token}"))]),
        );
        Ok(token)
    }

    fn capture_token(&mut self, url: &str, headers: &BTreeMap<String, String>) {
        if let Some(token) = self.auth.update_token(url, headers) {
            if !token.trim().is_empty() {
                self.jwt_token = Some(token.clone());
                let _ = persist_jwt_token(&self.client.runtime().auth_state_path, &token);
            }
        }
    }
}

impl<'a> CorePlainTransport<'a> {
    pub(crate) fn new(core: &'a crate::core::ImCore) -> Self {
        Self {
            core,
            http: crate::internal::http::HttpClient::new(),
        }
    }

    fn rpc_url(&self, endpoint: &str) -> String {
        let base = if endpoint.starts_with("/im/") {
            self.core
                .inner()
                .sdk_config()
                .message_service_endpoint
                .as_ref()
                .unwrap_or(&self.core.inner().sdk_config().service_base_url)
        } else {
            self.core
                .inner()
                .sdk_config()
                .user_service_endpoint
                .as_ref()
                .unwrap_or(&self.core.inner().sdk_config().service_base_url)
        };
        join_base_url(base.as_str(), endpoint)
    }

    fn execute_unsigned_json_request(
        &self,
        method: &str,
        url: &str,
        body: Vec<u8>,
    ) -> crate::ImResult<crate::internal::http::HttpResponse> {
        self.http.execute(crate::internal::http::HttpRequest {
            method: method.to_string(),
            url: url.to_string(),
            headers: BTreeMap::from([(
                "Content-Type".to_string(),
                crate::internal::json_rpc::CONTENT_TYPE_JSON.to_string(),
            )]),
            body,
        })
    }
}

impl AuthenticatedRpcTransport for CoreHttpTransport<'_> {
    fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> crate::ImResult<Value> {
        match self.authenticated_rpc_inner(endpoint, method, params.clone()) {
            Err(crate::ImError::Service {
                code: Some(code), ..
            }) if code == "1401" => {
                self.refresh_jwt()?;
                self.authenticated_rpc_inner(endpoint, method, params)
            }
            result => result,
        }
    }
}

impl<T> AuthenticatedRpcTransport for &mut T
where
    T: AuthenticatedRpcTransport + ?Sized,
{
    fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> crate::ImResult<Value> {
        (**self).authenticated_rpc(endpoint, method, params)
    }
}

impl RpcTransport for CoreHttpTransport<'_> {
    fn rpc(&mut self, endpoint: &str, method: &str, params: Value) -> crate::ImResult<Value> {
        self.plain_rpc(endpoint, method, params)
    }
}

impl RpcTransport for CorePlainTransport<'_> {
    fn rpc(&mut self, endpoint: &str, method: &str, params: Value) -> crate::ImResult<Value> {
        let url = self.rpc_url(endpoint);
        let body = serde_json::to_vec(&crate::internal::json_rpc::build_payload(method, params))
            .map_err(|err| crate::ImError::Serialization {
                detail: err.to_string(),
            })?;
        let response = self.execute_unsigned_json_request("POST", &url, body)?;
        if response.status_code >= 400 {
            return Err(service_error_from_http(
                response.status_code,
                &response.body,
            ));
        }
        crate::internal::json_rpc::decode_response(&response.body)
    }
}

impl RestTransport for CoreHttpTransport<'_> {
    fn rest_post(&mut self, endpoint: &str, method: &str, body: Value) -> crate::ImResult<Value> {
        let body = serde_json::to_vec(&body).map_err(|err| crate::ImError::Serialization {
            detail: err.to_string(),
        })?;
        self.authenticated_rest(endpoint, method, body, None, false)
    }

    fn rest_get(
        &mut self,
        endpoint: &str,
        method: &str,
        query: &BTreeMap<String, String>,
    ) -> crate::ImResult<Value> {
        self.authenticated_rest(endpoint, method, Vec::new(), Some(query), false)
    }
}

impl RestTransport for CorePlainTransport<'_> {
    fn rest_post(&mut self, endpoint: &str, method: &str, body: Value) -> crate::ImResult<Value> {
        let url = self.rpc_url(endpoint);
        let body = serde_json::to_vec(&body).map_err(|err| crate::ImError::Serialization {
            detail: err.to_string(),
        })?;
        let response = self.execute_unsigned_json_request(method, &url, body)?;
        if response.status_code >= 400 {
            return Err(service_error_from_http(
                response.status_code,
                &response.body,
            ));
        }
        crate::internal::json_rpc::decode_plain_response(&response.body)
    }

    fn rest_get(
        &mut self,
        endpoint: &str,
        method: &str,
        query: &BTreeMap<String, String>,
    ) -> crate::ImResult<Value> {
        let url = append_query(&self.rpc_url(endpoint), query);
        let response = self.execute_unsigned_json_request(method, &url, Vec::new())?;
        if response.status_code >= 400 {
            return Err(service_error_from_http(
                response.status_code,
                &response.body,
            ));
        }
        crate::internal::json_rpc::decode_plain_response(&response.body)
    }
}

impl AuthenticatedRestTransport for CoreHttpTransport<'_> {
    fn authenticated_rest_post(
        &mut self,
        endpoint: &str,
        method: &str,
        body: Value,
    ) -> crate::ImResult<Value> {
        let body = serde_json::to_vec(&body).map_err(|err| crate::ImError::Serialization {
            detail: err.to_string(),
        })?;
        self.authenticated_rest(endpoint, method, body, None, true)
    }

    fn authenticated_rest_get(
        &mut self,
        endpoint: &str,
        method: &str,
        query: &BTreeMap<String, String>,
    ) -> crate::ImResult<Value> {
        self.authenticated_rest(endpoint, method, Vec::new(), Some(query), true)
    }
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

fn join_base_url(base: &str, endpoint: &str) -> String {
    let base = base.trim().trim_end_matches('/');
    let endpoint = endpoint.trim();
    if endpoint.starts_with('/') {
        format!("{base}{endpoint}")
    } else {
        format!("{base}/{endpoint}")
    }
}

fn append_query(base_url: &str, query: &BTreeMap<String, String>) -> String {
    if query.is_empty() {
        return base_url.to_string();
    }
    let separator = if base_url.contains('?') { '&' } else { '?' };
    let pairs = query
        .iter()
        .map(|(key, value)| format!("{}={}", percent_encode(key), percent_encode(value)))
        .collect::<Vec<_>>()
        .join("&");
    format!("{base_url}{separator}{pairs}")
}

fn percent_encode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn service_error_from_http(status_code: u16, body: &[u8]) -> crate::ImError {
    crate::ImError::Service {
        status_code: Some(status_code),
        code: None,
        message: String::from_utf8_lossy(body).trim().to_string(),
    }
}

fn read_jwt_token(path: &std::path::Path) -> crate::ImResult<Option<String>> {
    let raw = match std::fs::read(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(crate::ImError::from(err)),
    };
    let value: Value =
        serde_json::from_slice(&raw).map_err(|err| crate::ImError::Serialization {
            detail: err.to_string(),
        })?;
    Ok(value
        .get("jwt_token")
        .or_else(|| value.get("token"))
        .or_else(|| value.get("access_token"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(ToOwned::to_owned))
}

fn persist_jwt_token(path: &std::path::Path, token: &str) -> crate::ImResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| crate::ImError::PathUnavailable {
            path_kind: "auth_state".to_string(),
            detail: "auth state path has no parent".to_string(),
        })?;
    std::fs::create_dir_all(parent)?;
    let body =
        serde_json::to_vec_pretty(&serde_json::json!({ "jwt_token": token })).map_err(|err| {
            crate::ImError::Serialization {
                detail: err.to_string(),
            }
        })?;
    std::fs::write(path, body)?;
    Ok(())
}

impl RpcTransport for UnavailableTransport {
    fn rpc(&mut self, endpoint: &str, method: &str, _params: Value) -> crate::ImResult<Value> {
        Err(crate::ImError::TransportUnavailable {
            detail: format!("{method} transport is not configured for {endpoint}"),
        })
    }
}

impl RestTransport for UnavailableTransport {
    fn rest_post(&mut self, endpoint: &str, method: &str, _body: Value) -> crate::ImResult<Value> {
        Err(crate::ImError::TransportUnavailable {
            detail: format!("{method} transport is not configured for {endpoint}"),
        })
    }

    fn rest_get(
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
