//! Internal transport capabilities shared by the product runtimes.
//!
//! Authenticated business RPC, anonymous control RPC and raw DID resolution are
//! separate traits. In particular, a pending Join device may resolve a public DID
//! Document without gaining an authenticated device client.

use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

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

    /// Reloads bearer state after an authenticated control message may have
    /// advanced the current device authorization generation.
    fn reload_authentication_state(&mut self) -> crate::ImResult<()> {
        Ok(())
    }
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

    fn directory_get_json_url(
        &mut self,
        _url: &str,
        _headers: BTreeMap<String, String>,
    ) -> crate::ImResult<Value> {
        Err(crate::ImError::unsupported("directory-raw-json-resolution"))
    }

    fn reconcile_pending_registration(
        &mut self,
        _pending: &crate::internal::identity_registration_pending::PendingRegistration,
    ) -> crate::ImResult<PendingRegistrationReconciliation> {
        Err(crate::ImError::unsupported(
            "pending-registration-reconciliation",
        ))
    }
}

pub(crate) trait AsyncRpcTransport {
    async fn rpc(&mut self, endpoint: &str, method: &str, params: Value) -> crate::ImResult<Value>;

    async fn directory_get_json_url(
        &mut self,
        _url: &str,
        _headers: BTreeMap<String, String>,
    ) -> crate::ImResult<Value> {
        Err(crate::ImError::unsupported("directory-raw-json-resolution"))
    }

    async fn reconcile_pending_registration(
        &mut self,
        _pending: &crate::internal::identity_registration_pending::PendingRegistration,
    ) -> crate::ImResult<PendingRegistrationReconciliation> {
        Err(crate::ImError::unsupported(
            "pending-registration-reconciliation",
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PendingRegistrationReconciliation {
    Absent,
    Committed {
        user_id: String,
        binding_generation: String,
        access_token: String,
    },
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
    auth: crate::internal::key_provider::ProviderBackedDidAuth,
    jwt_token: Option<String>,
    /// Exact-device product traffic must never silently downgrade to HTTP
    /// Signatures when the persisted bearer state could not be read.
    deferred_auth_state_error: Option<DeferredAuthStateError>,
    /// A business response has already succeeded; only this local auth commit
    /// may be retried. It must never cause the business RPC to be replayed.
    pending_auth_commit: Option<(String, String)>,
    expected_device_access: Option<ExpectedDeviceAccessOwned>,
    alternate_expected_device_access: Option<ExpectedDeviceAccessOwned>,
    ephemeral_bearer: bool,
    last_auth_retry_consumed: bool,
}

#[derive(Clone)]
pub(crate) struct ExpectedDeviceAccessOwned {
    pub(crate) did: String,
    pub(crate) user_id: String,
    pub(crate) device_id: String,
    pub(crate) key_id: String,
    pub(crate) auth_generation: u64,
    pub(crate) role: crate::internal::identity_device_state::DeviceAuthorizationRole,
    pub(crate) management_ready: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeferredAuthStateError {
    Missing,
    Unreadable,
}

pub(crate) struct CorePlainTransport<'a> {
    core: &'a crate::core::ImCore,
    http: crate::internal::http::HttpClient,
    register_authorization: Option<String>,
}

impl<'a> CoreHttpTransport<'a> {
    pub(crate) fn new(client: &'a crate::core::ImClient) -> Self {
        let runtime = client.runtime();
        let requires_exact_device_bearer = runtime.owner.sync_account.is_some();
        let (jwt_token, deferred_auth_state_error) =
            persisted_bearer_selection(runtime.key_provider.as_ref(), requires_exact_device_bearer);
        let expected_device_access = expected_device_access_for_client(client);
        let mut auth = crate::internal::key_provider::ProviderBackedDidAuth::new(
            runtime.key_provider.clone(),
            anp::authentication::AuthMode::HttpSignatures,
        );
        if let Some(token) = jwt_token.as_deref() {
            let config = client.core_inner().sdk_config();
            let user_origin = config
                .user_service_endpoint
                .as_ref()
                .unwrap_or(&config.service_base_url);
            auth.store_token(user_origin.as_str(), token);
            if let Some(message_origin) = config.message_service_endpoint.as_ref() {
                auth.store_token(message_origin.as_str(), token);
            }
        }
        let mut transport = Self {
            client,
            http: crate::internal::http::HttpClient::from_config(client.core_inner().sdk_config()),
            auth,
            jwt_token,
            deferred_auth_state_error,
            pending_auth_commit: None,
            expected_device_access,
            alternate_expected_device_access: None,
            ephemeral_bearer: false,
            last_auth_retry_consumed: false,
        };
        transport.drain_durable_auth_commits();
        transport
    }

    /// Uses the current device signing key without attaching a potentially
    /// stale generation-bound bearer token.
    pub(crate) fn new_signature_only(client: &'a crate::core::ImClient) -> Self {
        let runtime = client.runtime();
        let expected_device_access = expected_device_access_for_client(client);
        Self {
            client,
            http: crate::internal::http::HttpClient::from_config(client.core_inner().sdk_config()),
            auth: crate::internal::key_provider::ProviderBackedDidAuth::new(
                runtime.key_provider.clone(),
                anp::authentication::AuthMode::HttpSignatures,
            ),
            jwt_token: None,
            deferred_auth_state_error: None,
            pending_auth_commit: None,
            expected_device_access,
            alternate_expected_device_access: None,
            ephemeral_bearer: false,
            last_auth_retry_consumed: false,
        }
    }

    pub(crate) fn new_pending_device(
        client: &'a crate::core::ImClient,
        provider: Arc<dyn crate::internal::key_provider::KeyMaterialProvider>,
        expected: ExpectedDeviceAccessOwned,
    ) -> Self {
        Self {
            client,
            http: crate::internal::http::HttpClient::from_config(client.core_inner().sdk_config()),
            auth: crate::internal::key_provider::ProviderBackedDidAuth::new(
                provider,
                anp::authentication::AuthMode::HttpSignatures,
            ),
            jwt_token: None,
            deferred_auth_state_error: None,
            pending_auth_commit: None,
            expected_device_access: Some(expected),
            alternate_expected_device_access: None,
            ephemeral_bearer: true,
            last_auth_retry_consumed: false,
        }
    }

    /// Issues one signature-only request while a root-import completion may
    /// have committed remotely without returning its response. Exactly the
    /// pre-completion member principal or the post-completion admin principal
    /// is accepted; callers classify the returned token against the same pair.
    pub(crate) fn new_pending_device_transition(
        client: &'a crate::core::ImClient,
        provider: Arc<dyn crate::internal::key_provider::KeyMaterialProvider>,
        before: ExpectedDeviceAccessOwned,
        after: ExpectedDeviceAccessOwned,
    ) -> Self {
        let mut transport = Self::new_pending_device(client, provider, before);
        transport.alternate_expected_device_access = Some(after);
        transport
    }

    /// Uses one already-validated bearer without persisting it through the
    /// client's current key provider. This is reserved for generation changes:
    /// the old token is invalid as soon as the control plane advances the
    /// device generation, while the replacement must first validate the new
    /// Registry checkpoint before it can be committed locally.
    pub(crate) fn new_with_ephemeral_bearer(
        client: &'a crate::core::ImClient,
        bearer_token: &str,
    ) -> crate::ImResult<Self> {
        let bearer_token = bearer_token.trim();
        if bearer_token.is_empty() {
            return Err(crate::ImError::AuthRequired);
        }
        let mut transport = Self::new_signature_only(client);
        let config = client.core_inner().sdk_config();
        let user_origin = config
            .user_service_endpoint
            .as_ref()
            .unwrap_or(&config.service_base_url);
        transport
            .auth
            .store_token(user_origin.as_str(), bearer_token);
        if let Some(message_origin) = config.message_service_endpoint.as_ref() {
            transport
                .auth
                .store_token(message_origin.as_str(), bearer_token);
        }
        transport.jwt_token = Some(bearer_token.to_owned());
        transport.ephemeral_bearer = true;
        Ok(transport)
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
        self.last_auth_retry_consumed = false;
        let url = self.rpc_url(endpoint);
        let body = serde_json::to_vec(&crate::internal::json_rpc::build_payload(method, params))
            .map_err(|err| crate::ImError::Serialization {
                detail: err.to_string(),
            })?;
        let response = self.execute_json_request("POST", &url, body, false)?;
        let result = decode_rpc_http_response(response.status_code, &response.body)?;
        self.capture_token(&url, &response.headers)?;
        Ok(result)
    }

    async fn plain_rpc_async(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> crate::ImResult<Value> {
        self.last_auth_retry_consumed = false;
        let url = self.rpc_url(endpoint);
        let body = serde_json::to_vec(&crate::internal::json_rpc::build_payload(method, params))
            .map_err(|err| crate::ImError::Serialization {
                detail: err.to_string(),
            })?;
        let response = self
            .execute_json_request_async("POST", &url, body, false)
            .await?;
        let result = decode_rpc_http_response(response.status_code, &response.body)?;
        self.capture_token(&url, &response.headers)?;
        Ok(result)
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
        let result = decode_rpc_http_response(response.status_code, &response.body)?;
        self.capture_token(&url, &response.headers)?;
        Ok(result)
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
        let result = decode_rpc_http_response(response.status_code, &response.body)?;
        self.capture_token(&url, &response.headers)?;
        Ok(result)
    }

    fn authenticated_rest(
        &mut self,
        endpoint: &str,
        method: &str,
        body: Vec<u8>,
        query: Option<&BTreeMap<String, String>>,
        signed: bool,
    ) -> crate::ImResult<Value> {
        self.last_auth_retry_consumed = false;
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
        let result = crate::internal::json_rpc::decode_plain_response(&response.body)?;
        self.capture_token(&url, &response.headers)?;
        Ok(result)
    }

    async fn authenticated_rest_async(
        &mut self,
        endpoint: &str,
        method: &str,
        body: Vec<u8>,
        query: Option<&BTreeMap<String, String>>,
        signed: bool,
    ) -> crate::ImResult<Value> {
        self.last_auth_retry_consumed = false;
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
        let result = crate::internal::json_rpc::decode_plain_response(&response.body)?;
        self.capture_token(&url, &response.headers)?;
        Ok(result)
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
        if !force_new_auth {
            self.ensure_auth_state_ready()?;
        }
        let mut headers = BTreeMap::from([(
            "Content-Type".to_string(),
            crate::internal::json_rpc::CONTENT_TYPE_JSON.to_string(),
        )]);
        self.retry_pending_auth_commit();
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
        if response.status_code == 401 && !self.ephemeral_bearer && !self.last_auth_retry_consumed {
            self.last_auth_retry_consumed = true;
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
        Ok(response)
    }

    async fn execute_json_request_async(
        &mut self,
        method: &str,
        url: &str,
        body: Vec<u8>,
        force_new_auth: bool,
    ) -> crate::ImResult<crate::internal::http::HttpResponse> {
        if !force_new_auth {
            self.ensure_auth_state_ready()?;
        }
        let mut headers = BTreeMap::from([(
            "Content-Type".to_string(),
            crate::internal::json_rpc::CONTENT_TYPE_JSON.to_string(),
        )]);
        self.retry_pending_auth_commit();
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
        if response.status_code == 401 && !self.ephemeral_bearer && !self.last_auth_retry_consumed {
            self.last_auth_retry_consumed = true;
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
        self.last_auth_retry_consumed = false;
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
        let result = decode_rpc_http_response(response.status_code, &response.body)?;
        let header_token = crate::internal::key_provider::ProviderBackedDidAuth::response_token(
            &response.headers,
        )?;
        let body_token = result
            .get("access_token")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .map(ToOwned::to_owned);
        if matches!((&header_token, &body_token), (Some(left), Some(right)) if left != right) {
            return Err(crate::ImError::PermissionDenied);
        }
        self.hydrate_pending_device_user_id(&result)?;
        self.capture_token(&url, &response.headers)?;
        let token = body_token
            .or(header_token)
            .ok_or(crate::ImError::AuthRequired)?;
        if self.jwt_token.as_deref() != Some(token.as_str()) {
            self.validate_received_access_token(&token)?;
            self.client
                .runtime()
                .key_provider
                .persist_auth_token(&token)?;
            self.deferred_auth_state_error = None;
        }
        self.jwt_token = Some(token.clone());
        self.auth.update_token(
            &url,
            &BTreeMap::from([("Authorization".to_string(), format!("Bearer {token}"))]),
        )?;
        Ok(token)
    }

    pub(crate) async fn refresh_jwt_async(&mut self) -> crate::ImResult<String> {
        self.last_auth_retry_consumed = false;
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
        let result = decode_rpc_http_response(response.status_code, &response.body)?;
        let header_token = crate::internal::key_provider::ProviderBackedDidAuth::response_token(
            &response.headers,
        )?;
        let body_token = result
            .get("access_token")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .map(ToOwned::to_owned);
        if matches!((&header_token, &body_token), (Some(left), Some(right)) if left != right) {
            return Err(crate::ImError::PermissionDenied);
        }
        self.hydrate_pending_device_user_id(&result)?;
        self.capture_token(&url, &response.headers)?;
        let token = body_token
            .or(header_token)
            .ok_or(crate::ImError::AuthRequired)?;
        if self.jwt_token.as_deref() != Some(token.as_str()) {
            self.validate_received_access_token(&token)?;
            self.client
                .runtime()
                .key_provider
                .persist_auth_token(&token)?;
            self.deferred_auth_state_error = None;
        }
        self.jwt_token = Some(token.clone());
        self.auth.update_token(
            &url,
            &BTreeMap::from([("Authorization".to_string(), format!("Bearer {token}"))]),
        )?;
        Ok(token)
    }

    fn capture_token(
        &mut self,
        url: &str,
        headers: &BTreeMap<String, String>,
    ) -> crate::ImResult<()> {
        if let Some(token) =
            crate::internal::key_provider::ProviderBackedDidAuth::response_token(headers)?
        {
            if !token.trim().is_empty() {
                self.validate_received_access_token(&token)?;
                if !self.ephemeral_bearer {
                    if self
                        .client
                        .runtime()
                        .key_provider
                        .persist_auth_token(&token)
                        .is_ok()
                    {
                        self.deferred_auth_state_error = None;
                    } else {
                        self.pending_auth_commit = Some((url.to_owned(), token.clone()));
                        let _ = crate::internal::auth::convergence::stage(self.client, url, &token);
                        if self.deferred_auth_state_error.is_some() {
                            return Err(crate::ImError::CredentialFileUnreadable {
                                path_kind: "identity_auth_state".to_owned(),
                                detail: "refreshed exact-device bearer could not be persisted"
                                    .to_owned(),
                            });
                        }
                    }
                }
                self.auth.store_token(url, &token);
                self.jwt_token = Some(token);
            }
        }
        Ok(())
    }

    fn validate_received_access_token(&self, token: &str) -> crate::ImResult<()> {
        let Some(expected) = &self.expected_device_access else {
            return validate_access_token_for_client(self.client, token);
        };
        let primary = validate_expected_device_access_token(token, expected);
        if primary.is_ok() {
            return Ok(());
        }
        let alternate = self
            .alternate_expected_device_access
            .as_ref()
            .ok_or(crate::ImError::PermissionDenied)?;
        validate_expected_device_access_token(token, alternate)
    }

    fn hydrate_pending_device_user_id(&mut self, get_me: &Value) -> crate::ImResult<()> {
        let Some(expected) = self
            .expected_device_access
            .as_mut()
            .filter(|expected| expected.user_id.trim().is_empty())
        else {
            return Ok(());
        };
        let did = get_me
            .get("did")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or(crate::ImError::PermissionDenied)?;
        let user_id = get_me
            .get("user_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or(crate::ImError::PermissionDenied)?;
        if did != expected.did {
            return Err(crate::ImError::PermissionDenied);
        }
        expected.user_id = user_id.to_owned();
        if let Some(alternate) = self.alternate_expected_device_access.as_mut() {
            if alternate.did != did {
                return Err(crate::ImError::PermissionDenied);
            }
            if alternate.user_id.trim().is_empty() {
                alternate.user_id = user_id.to_owned();
            } else if alternate.user_id != user_id {
                return Err(crate::ImError::PermissionDenied);
            }
        }
        Ok(())
    }

    pub(crate) fn pending_device_user_id(&self) -> crate::ImResult<String> {
        self.expected_device_access
            .as_ref()
            .and_then(|expected| {
                let user_id = expected.user_id.trim();
                (!user_id.is_empty()).then(|| user_id.to_owned())
            })
            .ok_or(crate::ImError::PermissionDenied)
    }

    fn retry_pending_auth_commit(&mut self) {
        let Some((origin, token)) = self.pending_auth_commit.clone() else {
            return;
        };
        if self
            .client
            .runtime()
            .key_provider
            .persist_auth_token(&token)
            .is_ok()
        {
            self.auth.store_token(&origin, &token);
            self.pending_auth_commit = None;
            self.drain_durable_auth_commits();
        }
    }

    fn drain_durable_auth_commits(&mut self) {
        if let Ok(committed) = crate::internal::auth::convergence::drain(self.client) {
            let recovered = !committed.is_empty();
            for (origin, token) in committed {
                self.auth.store_token(&origin, &token);
                self.jwt_token = Some(token);
            }
            if recovered {
                self.deferred_auth_state_error = None;
            }
        }
    }

    fn ensure_auth_state_ready(&self) -> crate::ImResult<()> {
        if let Some(error) = self.deferred_auth_state_error {
            return Err(crate::ImError::CredentialFileUnreadable {
                path_kind: "identity_auth_state".to_owned(),
                detail: match error {
                    DeferredAuthStateError::Missing => {
                        "persisted exact-device bearer is missing".to_owned()
                    }
                    DeferredAuthStateError::Unreadable => {
                        "persisted exact-device bearer state could not be read".to_owned()
                    }
                },
            });
        }
        Ok(())
    }
}

fn persisted_bearer_selection(
    provider: &dyn crate::internal::key_provider::KeyMaterialProvider,
    requires_exact_device_bearer: bool,
) -> (Option<String>, Option<DeferredAuthStateError>) {
    match provider.valid_auth_token() {
        Ok(Some(token)) => (Some(token), None),
        Ok(None) if requires_exact_device_bearer => (None, Some(DeferredAuthStateError::Missing)),
        Ok(None) => (None, None),
        Err(_) if requires_exact_device_bearer => (None, Some(DeferredAuthStateError::Unreadable)),
        Err(_) => (None, None),
    }
}

fn expected_device_access_for_client(
    client: &crate::core::ImClient,
) -> Option<ExpectedDeviceAccessOwned> {
    // File-backed identities can advance auth_generation in the identity index
    // while an ImClient remains alive. They must keep using the dynamic index
    // validator; only host-backed identities freeze exact claims in this seed.
    if client.current_identity().local_alias.is_some() {
        return None;
    }
    let seed = client.runtime().owner.sync_account.as_ref()?;
    Some(ExpectedDeviceAccessOwned {
        did: client.did().as_str().to_owned(),
        user_id: seed.account_id.clone(),
        device_id: seed.protocol_device_id.as_str().to_owned(),
        key_id: seed.device_signing_key_id.clone(),
        auth_generation: seed.device_auth_generation.parse::<u64>().unwrap_or(0),
        role: seed.role,
        management_ready: seed.management_ready,
    })
}

fn validate_expected_device_access_token(
    token: &str,
    expected: &ExpectedDeviceAccessOwned,
) -> crate::ImResult<()> {
    crate::internal::access_token::validate_device_access_token(
        token,
        &crate::internal::access_token::ExpectedDeviceAccess {
            did: &expected.did,
            user_id: &expected.user_id,
            device_id: &expected.device_id,
            key_id: &expected.key_id,
            auth_generation: expected.auth_generation,
            role: expected.role,
            management_ready: expected.management_ready,
        },
    )
}

impl<'a> CorePlainTransport<'a> {
    pub(crate) fn new(core: &'a crate::core::ImCore) -> Self {
        Self {
            core,
            http: crate::internal::http::HttpClient::from_config(core.inner().sdk_config()),
            register_authorization: None,
        }
    }

    pub(crate) fn new_no_redirect(core: &'a crate::core::ImCore) -> Self {
        Self {
            core,
            http: crate::internal::http::HttpClient::from_config_no_redirect(
                core.inner().sdk_config(),
            ),
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

fn validate_access_token_for_client(
    client: &crate::core::ImClient,
    token: &str,
) -> crate::ImResult<()> {
    let Some(local_alias) = client.current_identity().local_alias.as_deref() else {
        // Hosted identities (daemon/runtime agents) have no on-disk identity
        // registry entry. Their current registration contract is Legacy DID,
        // so validate the newly issued token against the hosted DID directly.
        return crate::internal::access_token::validate_legacy_access_token(
            token,
            client.did().as_str(),
        );
    };
    let store = crate::internal::identity_store::IdentityStore::new(
        &client.core_inner().sdk_paths().identities,
    );
    let index = store.load_index()?;
    let entry = index
        .credentials
        .get(local_alias)
        .filter(|entry| entry.did == client.did().as_str())
        .ok_or(crate::ImError::PermissionDenied)?;
    match entry.device_state.as_ref() {
        Some(state)
            if state.mode == crate::internal::identity_device_state::IdentityDeviceMode::VNext =>
        {
            state.validate_for_did(client.did())?;
            let authorization = state
                .authorization
                .as_ref()
                .ok_or(crate::ImError::PermissionDenied)?;
            crate::internal::access_token::validate_device_access_token(
                token,
                &crate::internal::access_token::ExpectedDeviceAccess {
                    did: client.did().as_str(),
                    user_id: &entry.user_id,
                    device_id: authorization.protocol_device_id.as_str(),
                    key_id: &authorization.signing_key_id,
                    auth_generation: authorization.auth_generation,
                    role: authorization.role,
                    management_ready: authorization.management_ready,
                },
            )
        }
        Some(state)
            if state.mode == crate::internal::identity_device_state::IdentityDeviceMode::Legacy =>
        {
            state.validate_for_did(client.did())?;
            crate::internal::access_token::validate_legacy_access_token(
                token,
                client.did().as_str(),
            )
        }
        None => crate::internal::access_token::validate_legacy_access_token(
            token,
            client.did().as_str(),
        ),
        Some(_) => Err(crate::ImError::PermissionDenied),
    }
}

impl AuthenticatedRpcTransport for CoreHttpTransport<'_> {
    fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> crate::ImResult<Value> {
        self.last_auth_retry_consumed = false;
        match self.authenticated_rpc_inner(endpoint, method, params.clone()) {
            Err(crate::ImError::Service {
                code: Some(code), ..
            }) if code == "1401" && !self.ephemeral_bearer && !self.last_auth_retry_consumed => {
                let url = self.rpc_url(endpoint);
                self.auth.clear_token(&url);
                self.jwt_token = None;
                self.last_auth_retry_consumed = true;
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
        self.last_auth_retry_consumed = false;
        match self
            .authenticated_rpc_inner_async(endpoint, method, params.clone())
            .await
        {
            Err(crate::ImError::Service {
                code: Some(code), ..
            }) if code == "1401" && !self.ephemeral_bearer && !self.last_auth_retry_consumed => {
                let url = self.rpc_url(endpoint);
                self.auth.clear_token(&url);
                self.jwt_token = None;
                self.last_auth_retry_consumed = true;
                self.authenticated_rpc_inner_async(endpoint, method, params)
                    .await
            }
            result => result,
        }
    }

    fn reload_authentication_state(&mut self) -> crate::ImResult<()> {
        *self = Self::new(self.client);
        Ok(())
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

impl AsyncRawJsonTransport for CorePlainTransport<'_> {
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

    fn directory_get_json_url(
        &mut self,
        url: &str,
        headers: BTreeMap<String, String>,
    ) -> crate::ImResult<Value> {
        RawJsonTransport::get_json_url(self, url, headers)
    }
}

impl AsyncRpcTransport for CoreHttpTransport<'_> {
    async fn rpc(&mut self, endpoint: &str, method: &str, params: Value) -> crate::ImResult<Value> {
        self.plain_rpc_async(endpoint, method, params).await
    }

    async fn directory_get_json_url(
        &mut self,
        url: &str,
        headers: BTreeMap<String, String>,
    ) -> crate::ImResult<Value> {
        AsyncRawJsonTransport::get_json_url(self, url, headers).await
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
        decode_rpc_http_response(response.status_code, &response.body)
    }

    fn reconcile_pending_registration(
        &mut self,
        pending: &crate::internal::identity_registration_pending::PendingRegistration,
    ) -> crate::ImResult<PendingRegistrationReconciliation> {
        reconcile_pending_registration(self.core, pending)
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
        decode_rpc_http_response(response.status_code, &response.body)
    }

    async fn reconcile_pending_registration(
        &mut self,
        pending: &crate::internal::identity_registration_pending::PendingRegistration,
    ) -> crate::ImResult<PendingRegistrationReconciliation> {
        reconcile_pending_registration_async(self.core, pending).await
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
        data: None,
    }
}

fn reconcile_pending_registration(
    core: &crate::core::ImCore,
    pending: &crate::internal::identity_registration_pending::PendingRegistration,
) -> crate::ImResult<PendingRegistrationReconciliation> {
    let client = pending_registration_client(core, pending)?;
    let mut transport = pending_registration_transport(&client, pending);
    let access_token = match transport.refresh_jwt() {
        Ok(token) => token,
        Err(error) if registration_is_explicitly_absent(&error) => {
            return Ok(PendingRegistrationReconciliation::Absent);
        }
        Err(error) => return Err(error),
    };
    validate_pending_registration_registry(&mut transport, pending)?;
    let lookup = crate::internal::handle_discovery::resolve_authoritative_handle_binding(
        &client,
        &format!("{}.{}", pending.target_handle, pending.target_domain),
    )?;
    if lookup.did != pending.generated.did {
        return Err(crate::ImError::PermissionDenied);
    }
    let binding_generation = lookup
        .binding_generation
        .ok_or(crate::ImError::PermissionDenied)?;
    Ok(PendingRegistrationReconciliation::Committed {
        user_id: transport.pending_device_user_id()?,
        binding_generation,
        access_token,
    })
}

async fn reconcile_pending_registration_async(
    core: &crate::core::ImCore,
    pending: &crate::internal::identity_registration_pending::PendingRegistration,
) -> crate::ImResult<PendingRegistrationReconciliation> {
    let client = pending_registration_client(core, pending)?;
    let mut transport = pending_registration_transport(&client, pending);
    let access_token = match transport.refresh_jwt_async().await {
        Ok(token) => token,
        Err(error) if registration_is_explicitly_absent(&error) => {
            return Ok(PendingRegistrationReconciliation::Absent);
        }
        Err(error) => return Err(error),
    };
    validate_pending_registration_registry_async(&mut transport, pending).await?;
    let lookup = crate::internal::handle_discovery::resolve_authoritative_handle_binding_async(
        &client,
        &format!("{}.{}", pending.target_handle, pending.target_domain),
    )
    .await?;
    if lookup.did != pending.generated.did {
        return Err(crate::ImError::PermissionDenied);
    }
    let binding_generation = lookup
        .binding_generation
        .ok_or(crate::ImError::PermissionDenied)?;
    Ok(PendingRegistrationReconciliation::Committed {
        user_id: transport.pending_device_user_id()?,
        binding_generation,
        access_token,
    })
}

fn pending_registration_client(
    core: &crate::core::ImCore,
    pending: &crate::internal::identity_registration_pending::PendingRegistration,
) -> crate::ImResult<crate::core::ImClient> {
    core.client_with_identity_material_and_signing_key_id(
        crate::identity::HostedIdentityMaterial {
            identity_id: pending.generated.unique_id.clone(),
            did: pending.generated.did.as_str().to_owned(),
            handle: Some(format!(
                "{}.{}",
                pending.target_handle, pending.target_domain
            )),
            display_name: Some(pending.display_name.clone()),
            did_document: pending.generated.did_document.clone(),
            // The pending hosted client is used only for device-signed probing.
            // Give the hosted provider the exact Manifest device private key; the
            // root private key remains solely in PendingRegistration.
            default_signing_private_key_pem: pending.generated.device_signing_private_pem.clone(),
            e2ee_agreement_private_key_pem: Some(pending.generated.device_e2ee_private_pem.clone()),
            auth_token: None,
        },
        &pending.generated.device_signing_key_id,
    )
}

fn pending_registration_transport<'a>(
    client: &'a crate::core::ImClient,
    pending: &crate::internal::identity_registration_pending::PendingRegistration,
) -> CoreHttpTransport<'a> {
    CoreHttpTransport::new_pending_device(
        client,
        client.runtime().key_provider.clone(),
        ExpectedDeviceAccessOwned {
            did: pending.generated.did.as_str().to_owned(),
            user_id: String::new(),
            device_id: pending.generated.protocol_device_id.as_str().to_owned(),
            key_id: pending.generated.device_signing_key_id.clone(),
            auth_generation: 1,
            role: crate::internal::identity_device_state::DeviceAuthorizationRole::Admin,
            management_ready: true,
        },
    )
}

fn validate_pending_registration_registry(
    transport: &mut CoreHttpTransport<'_>,
    pending: &crate::internal::identity_registration_pending::PendingRegistration,
) -> crate::ImResult<()> {
    let call = crate::internal::identity_wire::device_join::build_registry_call(
        &pending.generated.did,
        false,
    );
    let raw = AuthenticatedRpcTransport::authenticated_rpc(
        transport,
        call.endpoint,
        call.method,
        call.params,
    )?;
    validate_pending_registration_registry_value(pending, raw)
}

async fn validate_pending_registration_registry_async(
    transport: &mut CoreHttpTransport<'_>,
    pending: &crate::internal::identity_registration_pending::PendingRegistration,
) -> crate::ImResult<()> {
    let call = crate::internal::identity_wire::device_join::build_registry_call(
        &pending.generated.did,
        false,
    );
    let raw = AsyncAuthenticatedRpcTransport::authenticated_rpc(
        transport,
        call.endpoint,
        call.method,
        call.params,
    )
    .await?;
    validate_pending_registration_registry_value(pending, raw)
}

fn validate_pending_registration_registry_value(
    pending: &crate::internal::identity_registration_pending::PendingRegistration,
    raw: Value,
) -> crate::ImResult<()> {
    let registry = crate::internal::identity_wire::device_join::parse_registry_result(
        raw,
        &pending.generated.did,
        false,
    )?;
    let device = registry
        .devices
        .iter()
        .find(|device| device.device_id == pending.generated.protocol_device_id.as_str())
        .filter(|device| {
            device.signing_key_id == pending.generated.device_signing_key_id
                && device.e2ee_key_id == pending.generated.device_e2ee_key_id
                && device.role
                    == crate::internal::identity_device_state::DeviceAuthorizationRole::Admin
                && device.management_ready
                && device.auth_generation == 1
        })
        .ok_or(crate::ImError::PermissionDenied)?;
    let _ = device;
    if registry.devices.len() != 1
        || registry.checkpoint.document_version != 1
        || registry.checkpoint.registry_version != 1
        || registry.checkpoint.document_hash != pending.document_hash
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

fn registration_is_explicitly_absent(error: &crate::ImError) -> bool {
    const ACTIVE_DID_NOT_FOUND: &str = "did_auth.active_did_not_found";
    match error {
        crate::ImError::Service {
            status_code: Some(404),
            ..
        } => true,
        crate::ImError::Service {
            code: Some(code),
            data: Some(data),
            ..
        } if code == "-32000" => {
            data.get("awiki_code").and_then(Value::as_str) == Some(ACTIVE_DID_NOT_FOUND)
        }
        crate::ImError::Service {
            code: Some(code), ..
        } => code == "-32002",
        _ => false,
    }
}

fn decode_rpc_http_response(status_code: u16, body: &[u8]) -> crate::ImResult<Value> {
    if (200..300).contains(&status_code) && body.is_empty() {
        return Err(crate::ImError::TransportUnavailable {
            detail: "JSON-RPC response body is empty".to_owned(),
        });
    }
    match crate::internal::json_rpc::decode_response(body) {
        Err(crate::ImError::Service {
            code,
            message,
            data,
            ..
        }) => Err(crate::ImError::Service {
            status_code: Some(status_code),
            code,
            message,
            data,
        }),
        Ok(result) if status_code < 400 => Ok(result),
        Err(error @ crate::ImError::Serialization { .. }) if (200..300).contains(&status_code) => {
            Err(error)
        }
        _ => Err(service_error_from_http(status_code, body)),
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
