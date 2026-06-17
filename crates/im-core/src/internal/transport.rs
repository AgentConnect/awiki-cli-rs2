use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::internal::auth::state::{persist_jwt_token, read_jwt_token};

pub(crate) trait AuthenticatedRpcTransport {
    fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> crate::ImResult<Value>;
}

pub(crate) trait AsyncAuthenticatedRpcTransport {
    async fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> crate::ImResult<Value>;
}

pub(crate) trait AttachmentObjectTransport {
    fn put_attachment_object(
        &mut self,
        upload_uri: &str,
        headers: BTreeMap<String, String>,
        body: Vec<u8>,
    ) -> crate::ImResult<()>;

    fn get_attachment_object(
        &mut self,
        object_uri: &str,
        download_ticket: &str,
    ) -> crate::ImResult<AttachmentObjectResponse>;
}

pub(crate) trait AsyncAttachmentObjectTransport {
    async fn put_attachment_object(
        &mut self,
        upload_uri: &str,
        headers: BTreeMap<String, String>,
        body: Vec<u8>,
    ) -> crate::ImResult<()>;

    async fn get_attachment_object(
        &mut self,
        object_uri: &str,
        download_ticket: &str,
    ) -> crate::ImResult<AttachmentObjectResponse>;

    async fn put_attachment_object_stream(
        &mut self,
        upload_uri: &str,
        headers: BTreeMap<String, String>,
        body: AsyncAttachmentObjectBody,
    ) -> crate::ImResult<()> {
        self.put_attachment_object(upload_uri, headers, body.into_bytes().await?)
            .await
    }

    async fn get_attachment_object_stream(
        &mut self,
        object_uri: &str,
        download_ticket: &str,
    ) -> crate::ImResult<AsyncAttachmentObjectResponse> {
        self.get_attachment_object(object_uri, download_ticket)
            .await
            .map(AsyncAttachmentObjectResponse::from)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AttachmentObjectResponse {
    pub(crate) body: Vec<u8>,
    pub(crate) content_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AsyncAttachmentObjectBody {
    Bytes(Vec<u8>),
    File {
        path: PathBuf,
        len: u64,
        content_type: Option<String>,
    },
}

impl AsyncAttachmentObjectBody {
    async fn into_bytes(self) -> crate::ImResult<Vec<u8>> {
        match self {
            Self::Bytes(bytes) => Ok(bytes),
            Self::File { path, .. } => {
                tokio::fs::read(&path)
                    .await
                    .map_err(|err| crate::ImError::Io {
                        detail: format!("read attachment file {}: {err}", path.display()),
                    })
            }
        }
    }
}

pub(crate) enum AsyncAttachmentObjectResponse {
    Bytes {
        body: Vec<u8>,
        content_type: Option<String>,
        consumed: bool,
    },
    Response {
        response: reqwest::Response,
        content_type: Option<String>,
    },
}

impl AsyncAttachmentObjectResponse {
    pub(crate) fn content_type(&self) -> Option<&str> {
        match self {
            Self::Bytes { content_type, .. } | Self::Response { content_type, .. } => {
                content_type.as_deref()
            }
        }
    }

    pub(crate) async fn next_chunk(&mut self) -> crate::ImResult<Option<Vec<u8>>> {
        match self {
            Self::Bytes { body, consumed, .. } => {
                if *consumed {
                    Ok(None)
                } else {
                    *consumed = true;
                    Ok(Some(std::mem::take(body)))
                }
            }
            Self::Response { response, .. } => response
                .chunk()
                .await
                .map(|chunk| chunk.map(|bytes| bytes.to_vec()))
                .map_err(reqwest_transport_unavailable),
        }
    }

    pub(crate) async fn into_bytes(mut self) -> crate::ImResult<Vec<u8>> {
        let mut bytes = Vec::new();
        while let Some(chunk) = self.next_chunk().await? {
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }
}

impl From<AttachmentObjectResponse> for AsyncAttachmentObjectResponse {
    fn from(value: AttachmentObjectResponse) -> Self {
        Self::Bytes {
            body: value.body,
            content_type: value.content_type,
            consumed: false,
        }
    }
}

pub(crate) trait RawJsonTransport {
    fn get_json_url(
        &mut self,
        url: &str,
        headers: BTreeMap<String, String>,
    ) -> crate::ImResult<Value>;
}

pub(crate) trait AsyncRawJsonTransport {
    async fn get_json_url(
        &mut self,
        url: &str,
        headers: BTreeMap<String, String>,
    ) -> crate::ImResult<Value>;
}

pub(crate) trait RpcTransport {
    fn rpc(&mut self, endpoint: &str, method: &str, params: Value) -> crate::ImResult<Value>;
}

pub(crate) trait AsyncRpcTransport {
    async fn rpc(&mut self, endpoint: &str, method: &str, params: Value) -> crate::ImResult<Value>;
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

pub(crate) trait AsyncRestTransport {
    async fn rest_post(
        &mut self,
        endpoint: &str,
        method: &str,
        body: Value,
    ) -> crate::ImResult<Value>;

    async fn rest_get(
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

pub(crate) trait AsyncAuthenticatedRestTransport {
    async fn authenticated_rest_post(
        &mut self,
        endpoint: &str,
        method: &str,
        body: Value,
    ) -> crate::ImResult<Value>;

    async fn authenticated_rest_get(
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
    register_authorization: Option<String>,
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
            http: crate::internal::http::HttpClient::from_config(client.core_inner().sdk_config()),
            auth,
            jwt_token,
        }
    }

    fn rpc_url(&self, endpoint: &str) -> String {
        let endpoint = endpoint.trim();
        if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
            return endpoint.to_string();
        }
        let config = self.client.core_inner().sdk_config();
        let base = if endpoint.starts_with("/im/") {
            config
                .message_service_endpoint
                .as_ref()
                .unwrap_or(&config.service_base_url)
        } else if endpoint.starts_with("/mail/") {
            config
                .mail_service_endpoint
                .as_ref()
                .unwrap_or(&config.service_base_url)
        } else {
            config
                .user_service_endpoint
                .as_ref()
                .unwrap_or(&config.service_base_url)
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

    async fn plain_rpc_async(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> crate::ImResult<Value> {
        let url = self.rpc_url(endpoint);
        let body = serde_json::to_vec(&crate::internal::json_rpc::build_payload(method, params))
            .map_err(|err| crate::ImError::Serialization {
                detail: err.to_string(),
            })?;
        let response = self
            .execute_json_request_async("POST", &url, body, false)
            .await?;
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

    async fn authenticated_rpc_inner_async(
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
        let response = self
            .execute_json_request_async("POST", &url, body, false)
            .await?;
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

    async fn authenticated_rest_async(
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
            self.execute_json_request_async(method, &url, body, false)
                .await?
        } else {
            self.execute_unsigned_json_request_async(method, &url, body)
                .await?
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

    async fn execute_unsigned_json_request_async(
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
        self.http.execute_async(request).await
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

    async fn execute_json_request_async(
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
        let mut response = self.http.execute_async(request).await?;
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
            response = self.http.execute_async(request).await?;
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

    pub(crate) async fn refresh_jwt_async(&mut self) -> crate::ImResult<String> {
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
        let response = self
            .execute_json_request_async("POST", &url, body, true)
            .await?;
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
            http: crate::internal::http::HttpClient::from_config(core.inner().sdk_config()),
            register_authorization: None,
        }
    }

    #[cfg(feature = "mcp-trusted-registration")]
    pub(crate) fn new_with_register_bearer_token(
        core: &'a crate::core::ImCore,
        token: impl Into<String>,
    ) -> Self {
        let mut transport = Self::new(core);
        let token = token.into();
        let trimmed = token.trim();
        if !trimmed.is_empty() {
            transport.register_authorization = Some(format!("Bearer {trimmed}"));
        }
        transport
    }

    fn rpc_url(&self, endpoint: &str) -> String {
        let endpoint = endpoint.trim();
        if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
            return endpoint.to_string();
        }
        let config = self.core.inner().sdk_config();
        let base = if endpoint.starts_with("/im/") {
            config
                .message_service_endpoint
                .as_ref()
                .unwrap_or(&config.service_base_url)
        } else if endpoint.starts_with("/mail/") {
            config
                .mail_service_endpoint
                .as_ref()
                .unwrap_or(&config.service_base_url)
        } else {
            config
                .user_service_endpoint
                .as_ref()
                .unwrap_or(&config.service_base_url)
        };
        join_base_url(base.as_str(), endpoint)
    }

    fn execute_unsigned_json_request(
        &self,
        method: &str,
        url: &str,
        body: Vec<u8>,
        headers: BTreeMap<String, String>,
    ) -> crate::ImResult<crate::internal::http::HttpResponse> {
        self.http.execute(crate::internal::http::HttpRequest {
            method: method.to_string(),
            url: url.to_string(),
            headers,
            body,
        })
    }

    async fn execute_unsigned_json_request_async(
        &self,
        method: &str,
        url: &str,
        body: Vec<u8>,
        headers: BTreeMap<String, String>,
    ) -> crate::ImResult<crate::internal::http::HttpResponse> {
        self.http
            .execute_async(crate::internal::http::HttpRequest {
                method: method.to_string(),
                url: url.to_string(),
                headers,
                body,
            })
            .await
    }

    fn unsigned_headers(
        &self,
        endpoint: &str,
        rpc_method: Option<&str>,
    ) -> BTreeMap<String, String> {
        let mut headers = BTreeMap::from([(
            "Content-Type".to_string(),
            crate::internal::json_rpc::CONTENT_TYPE_JSON.to_string(),
        )]);
        if endpoint == crate::internal::identity_wire::DID_AUTH_RPC_ENDPOINT
            && rpc_method == Some("register")
        {
            if let Some(authorization) = self.register_authorization.as_ref() {
                headers.insert("Authorization".to_string(), authorization.clone());
            }
        }
        headers
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

impl AsyncAuthenticatedRpcTransport for CoreHttpTransport<'_> {
    async fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> crate::ImResult<Value> {
        match self
            .authenticated_rpc_inner_async(endpoint, method, params.clone())
            .await
        {
            Err(crate::ImError::Service {
                code: Some(code), ..
            }) if code == "1401" => {
                self.refresh_jwt_async().await?;
                self.authenticated_rpc_inner_async(endpoint, method, params)
                    .await
            }
            result => result,
        }
    }
}

impl AttachmentObjectTransport for CoreHttpTransport<'_> {
    fn put_attachment_object(
        &mut self,
        upload_uri: &str,
        headers: BTreeMap<String, String>,
        body: Vec<u8>,
    ) -> crate::ImResult<()> {
        let response = self.http.execute(crate::internal::http::HttpRequest {
            method: "PUT".to_string(),
            url: upload_uri.to_string(),
            headers,
            body,
        })?;
        if response.status_code >= 400 {
            return Err(service_error_from_http(
                response.status_code,
                &response.body,
            ));
        }
        Ok(())
    }

    fn get_attachment_object(
        &mut self,
        object_uri: &str,
        download_ticket: &str,
    ) -> crate::ImResult<AttachmentObjectResponse> {
        let response = self.http.execute(crate::internal::http::HttpRequest {
            method: "GET".to_string(),
            url: object_uri.to_string(),
            headers: BTreeMap::from([(
                "Authorization".to_string(),
                format!("Bearer {}", download_ticket.trim()),
            )]),
            body: Vec::new(),
        })?;
        if response.status_code >= 400 {
            return Err(service_error_from_http(
                response.status_code,
                &response.body,
            ));
        }
        Ok(AttachmentObjectResponse {
            content_type: response_header_value(&response.headers, "Content-Type"),
            body: response.body,
        })
    }
}

impl AsyncAttachmentObjectTransport for CoreHttpTransport<'_> {
    async fn put_attachment_object(
        &mut self,
        upload_uri: &str,
        headers: BTreeMap<String, String>,
        body: Vec<u8>,
    ) -> crate::ImResult<()> {
        let response = self
            .http
            .execute_async(crate::internal::http::HttpRequest {
                method: "PUT".to_string(),
                url: upload_uri.to_string(),
                headers,
                body,
            })
            .await?;
        if response.status_code >= 400 {
            return Err(service_error_from_http(
                response.status_code,
                &response.body,
            ));
        }
        Ok(())
    }

    async fn get_attachment_object(
        &mut self,
        object_uri: &str,
        download_ticket: &str,
    ) -> crate::ImResult<AttachmentObjectResponse> {
        let response = self
            .get_attachment_object_stream(object_uri, download_ticket)
            .await?;
        let content_type = response.content_type().map(ToOwned::to_owned);
        let body = response.into_bytes().await?;
        Ok(AttachmentObjectResponse { body, content_type })
    }

    async fn put_attachment_object_stream(
        &mut self,
        upload_uri: &str,
        headers: BTreeMap<String, String>,
        body: AsyncAttachmentObjectBody,
    ) -> crate::ImResult<()> {
        let client = self.http.async_client()?;
        let mut builder = client
            .request(reqwest::Method::PUT, upload_uri.trim())
            .timeout(crate::internal::http::RESPONSE_TIMEOUT);
        for (key, value) in headers {
            builder = builder.header(key.trim(), value.trim());
        }
        builder = match body {
            AsyncAttachmentObjectBody::Bytes(bytes) => builder.body(bytes),
            AsyncAttachmentObjectBody::File {
                path,
                len,
                content_type,
            } => {
                let file =
                    tokio::fs::File::open(&path)
                        .await
                        .map_err(|err| crate::ImError::Io {
                            detail: format!("open attachment file {}: {err}", path.display()),
                        })?;
                let stream = tokio_util::io::ReaderStream::new(file);
                let body = reqwest::Body::wrap_stream(stream);
                let mut builder = builder
                    .header(reqwest::header::CONTENT_LENGTH, len.to_string())
                    .body(body);
                if let Some(content_type) = content_type
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
                {
                    builder = builder.header(reqwest::header::CONTENT_TYPE, content_type.trim());
                }
                builder
            }
        };
        let response = builder
            .send()
            .await
            .map_err(reqwest_transport_unavailable)?;
        let status_code = response.status().as_u16();
        if status_code >= 400 {
            let body = response
                .bytes()
                .await
                .map_err(reqwest_transport_unavailable)?
                .to_vec();
            return Err(service_error_from_http(status_code, &body));
        }
        Ok(())
    }

    async fn get_attachment_object_stream(
        &mut self,
        object_uri: &str,
        download_ticket: &str,
    ) -> crate::ImResult<AsyncAttachmentObjectResponse> {
        let client = self.http.async_client()?;
        let response = client
            .request(reqwest::Method::GET, object_uri.trim())
            .header(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {}", download_ticket.trim()),
            )
            .timeout(crate::internal::http::RESPONSE_TIMEOUT)
            .send()
            .await
            .map_err(reqwest_transport_unavailable)?;
        let status_code = response.status().as_u16();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        if status_code >= 400 {
            let body = response
                .bytes()
                .await
                .map_err(reqwest_transport_unavailable)?
                .to_vec();
            return Err(service_error_from_http(status_code, &body));
        }
        Ok(AsyncAttachmentObjectResponse::Response {
            response,
            content_type,
        })
    }
}

impl RawJsonTransport for CoreHttpTransport<'_> {
    fn get_json_url(
        &mut self,
        url: &str,
        headers: BTreeMap<String, String>,
    ) -> crate::ImResult<Value> {
        let response = self.http.execute(crate::internal::http::HttpRequest {
            method: "GET".to_string(),
            url: url.to_string(),
            headers,
            body: Vec::new(),
        })?;
        if response.status_code >= 400 {
            return Err(service_error_from_http(
                response.status_code,
                &response.body,
            ));
        }
        serde_json::from_slice(&response.body).map_err(|err| crate::ImError::Serialization {
            detail: err.to_string(),
        })
    }
}

impl AsyncRawJsonTransport for CoreHttpTransport<'_> {
    async fn get_json_url(
        &mut self,
        url: &str,
        headers: BTreeMap<String, String>,
    ) -> crate::ImResult<Value> {
        let response = self
            .http
            .execute_async(crate::internal::http::HttpRequest {
                method: "GET".to_string(),
                url: url.to_string(),
                headers,
                body: Vec::new(),
            })
            .await?;
        if response.status_code >= 400 {
            return Err(service_error_from_http(
                response.status_code,
                &response.body,
            ));
        }
        serde_json::from_slice(&response.body).map_err(|err| crate::ImError::Serialization {
            detail: err.to_string(),
        })
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

impl<T> AsyncAuthenticatedRpcTransport for &mut T
where
    T: AsyncAuthenticatedRpcTransport + ?Sized,
{
    async fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> crate::ImResult<Value> {
        (**self).authenticated_rpc(endpoint, method, params).await
    }
}

impl RpcTransport for CoreHttpTransport<'_> {
    fn rpc(&mut self, endpoint: &str, method: &str, params: Value) -> crate::ImResult<Value> {
        self.plain_rpc(endpoint, method, params)
    }
}

impl AsyncRpcTransport for CoreHttpTransport<'_> {
    async fn rpc(&mut self, endpoint: &str, method: &str, params: Value) -> crate::ImResult<Value> {
        self.plain_rpc_async(endpoint, method, params).await
    }
}

impl RpcTransport for CorePlainTransport<'_> {
    fn rpc(&mut self, endpoint: &str, method: &str, params: Value) -> crate::ImResult<Value> {
        let url = self.rpc_url(endpoint);
        let body = serde_json::to_vec(&crate::internal::json_rpc::build_payload(method, params))
            .map_err(|err| crate::ImError::Serialization {
                detail: err.to_string(),
            })?;
        let response = self.execute_unsigned_json_request(
            "POST",
            &url,
            body,
            self.unsigned_headers(endpoint, Some(method)),
        )?;
        if response.status_code >= 400 {
            return Err(service_error_from_http(
                response.status_code,
                &response.body,
            ));
        }
        crate::internal::json_rpc::decode_response(&response.body)
    }
}

impl AsyncRpcTransport for CorePlainTransport<'_> {
    async fn rpc(&mut self, endpoint: &str, method: &str, params: Value) -> crate::ImResult<Value> {
        let url = self.rpc_url(endpoint);
        let body = serde_json::to_vec(&crate::internal::json_rpc::build_payload(method, params))
            .map_err(|err| crate::ImError::Serialization {
                detail: err.to_string(),
            })?;
        let response = self
            .execute_unsigned_json_request_async(
                "POST",
                &url,
                body,
                self.unsigned_headers(endpoint, Some(method)),
            )
            .await?;
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

impl AsyncRestTransport for CoreHttpTransport<'_> {
    async fn rest_post(
        &mut self,
        endpoint: &str,
        method: &str,
        body: Value,
    ) -> crate::ImResult<Value> {
        let body = serde_json::to_vec(&body).map_err(|err| crate::ImError::Serialization {
            detail: err.to_string(),
        })?;
        self.authenticated_rest_async(endpoint, method, body, None, false)
            .await
    }

    async fn rest_get(
        &mut self,
        endpoint: &str,
        method: &str,
        query: &BTreeMap<String, String>,
    ) -> crate::ImResult<Value> {
        self.authenticated_rest_async(endpoint, method, Vec::new(), Some(query), false)
            .await
    }
}

impl RestTransport for CorePlainTransport<'_> {
    fn rest_post(&mut self, endpoint: &str, method: &str, body: Value) -> crate::ImResult<Value> {
        let url = self.rpc_url(endpoint);
        let body = serde_json::to_vec(&body).map_err(|err| crate::ImError::Serialization {
            detail: err.to_string(),
        })?;
        let response = self.execute_unsigned_json_request(
            method,
            &url,
            body,
            self.unsigned_headers(endpoint, None),
        )?;
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
        let response = self.execute_unsigned_json_request(
            method,
            &url,
            Vec::new(),
            self.unsigned_headers(endpoint, None),
        )?;
        if response.status_code >= 400 {
            return Err(service_error_from_http(
                response.status_code,
                &response.body,
            ));
        }
        crate::internal::json_rpc::decode_plain_response(&response.body)
    }
}

impl AsyncRestTransport for CorePlainTransport<'_> {
    async fn rest_post(
        &mut self,
        endpoint: &str,
        method: &str,
        body: Value,
    ) -> crate::ImResult<Value> {
        let url = self.rpc_url(endpoint);
        let body = serde_json::to_vec(&body).map_err(|err| crate::ImError::Serialization {
            detail: err.to_string(),
        })?;
        let response = self
            .execute_unsigned_json_request_async(
                method,
                &url,
                body,
                self.unsigned_headers(endpoint, None),
            )
            .await?;
        if response.status_code >= 400 {
            return Err(service_error_from_http(
                response.status_code,
                &response.body,
            ));
        }
        crate::internal::json_rpc::decode_plain_response(&response.body)
    }

    async fn rest_get(
        &mut self,
        endpoint: &str,
        method: &str,
        query: &BTreeMap<String, String>,
    ) -> crate::ImResult<Value> {
        let url = append_query(&self.rpc_url(endpoint), query);
        let response = self
            .execute_unsigned_json_request_async(
                method,
                &url,
                Vec::new(),
                self.unsigned_headers(endpoint, None),
            )
            .await?;
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

impl AsyncAuthenticatedRestTransport for CoreHttpTransport<'_> {
    async fn authenticated_rest_post(
        &mut self,
        endpoint: &str,
        method: &str,
        body: Value,
    ) -> crate::ImResult<Value> {
        let body = serde_json::to_vec(&body).map_err(|err| crate::ImError::Serialization {
            detail: err.to_string(),
        })?;
        self.authenticated_rest_async(endpoint, method, body, None, true)
            .await
    }

    async fn authenticated_rest_get(
        &mut self,
        endpoint: &str,
        method: &str,
        query: &BTreeMap<String, String>,
    ) -> crate::ImResult<Value> {
        self.authenticated_rest_async(endpoint, method, Vec::new(), Some(query), true)
            .await
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

impl AsyncAuthenticatedRpcTransport for UnavailableTransport {
    async fn authenticated_rpc(
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

impl AttachmentObjectTransport for UnavailableTransport {
    fn put_attachment_object(
        &mut self,
        upload_uri: &str,
        _headers: BTreeMap<String, String>,
        _body: Vec<u8>,
    ) -> crate::ImResult<()> {
        Err(crate::ImError::TransportUnavailable {
            detail: format!("PUT transport is not configured for {upload_uri}"),
        })
    }

    fn get_attachment_object(
        &mut self,
        object_uri: &str,
        _download_ticket: &str,
    ) -> crate::ImResult<AttachmentObjectResponse> {
        Err(crate::ImError::TransportUnavailable {
            detail: format!("GET transport is not configured for {object_uri}"),
        })
    }
}

impl AsyncAttachmentObjectTransport for UnavailableTransport {
    async fn put_attachment_object(
        &mut self,
        upload_uri: &str,
        _headers: BTreeMap<String, String>,
        _body: Vec<u8>,
    ) -> crate::ImResult<()> {
        Err(crate::ImError::TransportUnavailable {
            detail: format!("PUT transport is not configured for {upload_uri}"),
        })
    }

    async fn get_attachment_object(
        &mut self,
        object_uri: &str,
        _download_ticket: &str,
    ) -> crate::ImResult<AttachmentObjectResponse> {
        Err(crate::ImError::TransportUnavailable {
            detail: format!("GET transport is not configured for {object_uri}"),
        })
    }

    async fn put_attachment_object_stream(
        &mut self,
        upload_uri: &str,
        _headers: BTreeMap<String, String>,
        _body: AsyncAttachmentObjectBody,
    ) -> crate::ImResult<()> {
        Err(crate::ImError::TransportUnavailable {
            detail: format!("PUT transport is not configured for {upload_uri}"),
        })
    }

    async fn get_attachment_object_stream(
        &mut self,
        object_uri: &str,
        _download_ticket: &str,
    ) -> crate::ImResult<AsyncAttachmentObjectResponse> {
        Err(crate::ImError::TransportUnavailable {
            detail: format!("GET transport is not configured for {object_uri}"),
        })
    }
}

impl RawJsonTransport for UnavailableTransport {
    fn get_json_url(
        &mut self,
        url: &str,
        _headers: BTreeMap<String, String>,
    ) -> crate::ImResult<Value> {
        Err(crate::ImError::TransportUnavailable {
            detail: format!("GET transport is not configured for {url}"),
        })
    }
}

impl AsyncRawJsonTransport for UnavailableTransport {
    async fn get_json_url(
        &mut self,
        url: &str,
        _headers: BTreeMap<String, String>,
    ) -> crate::ImResult<Value> {
        Err(crate::ImError::TransportUnavailable {
            detail: format!("GET transport is not configured for {url}"),
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

fn response_header_value(headers: &BTreeMap<String, String>, name: &str) -> Option<String> {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.clone())
        .filter(|value| !value.trim().is_empty())
}

fn reqwest_transport_unavailable(err: reqwest::Error) -> crate::ImError {
    crate::ImError::TransportUnavailable {
        detail: err.to_string(),
    }
}

impl RpcTransport for UnavailableTransport {
    fn rpc(&mut self, endpoint: &str, method: &str, _params: Value) -> crate::ImResult<Value> {
        Err(crate::ImError::TransportUnavailable {
            detail: format!("{method} transport is not configured for {endpoint}"),
        })
    }
}

impl AsyncRpcTransport for UnavailableTransport {
    async fn rpc(
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

impl AsyncRestTransport for UnavailableTransport {
    async fn rest_post(
        &mut self,
        endpoint: &str,
        method: &str,
        _body: Value,
    ) -> crate::ImResult<Value> {
        Err(crate::ImError::TransportUnavailable {
            detail: format!("{method} transport is not configured for {endpoint}"),
        })
    }

    async fn rest_get(
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

impl AsyncAuthenticatedRestTransport for UnavailableTransport {
    async fn authenticated_rest_post(
        &mut self,
        endpoint: &str,
        method: &str,
        _body: Value,
    ) -> crate::ImResult<Value> {
        Err(crate::ImError::TransportUnavailable {
            detail: format!("{method} transport is not configured for {endpoint}"),
        })
    }

    async fn authenticated_rest_get(
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

#[cfg(test)]
mod tests;
