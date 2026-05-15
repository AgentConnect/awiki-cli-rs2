use crate::anpsdk::{DIDWbaAuthHeader, AUTH_MODE_HTTP_SIGNATURES};
use crate::transportcfg::{HttpClient, HttpRequest, HttpResponse, Profile};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

mod wire;

pub use wire::{
    build_json_rpc_payload, decode_json_rpc_response, decode_json_rpc_response_optional,
    decode_plain_json_response, flatten_header_values, http_status_error, JsonRpcResponseError,
    CONTENT_TYPE_JSON, JSON_RPC_ID, JSON_RPC_VERSION,
};

pub type PersistToken = Box<dyn FnMut(&str) -> anyhow::Result<()> + 'static>;

pub struct Session {
    helper: DIDWbaAuthHeader,
    identity_name: String,
    did: String,
    jwt_token: String,
    persist_token: Option<PersistToken>,
    persistent: BTreeSet<String>,
}

impl Session {
    pub fn new(
        did_document_path: impl AsRef<Path>,
        private_key_path: impl AsRef<Path>,
        identity_name: impl Into<String>,
        did: impl Into<String>,
        jwt_token: impl Into<String>,
        persist_token: Option<PersistToken>,
    ) -> Self {
        Self {
            helper: DIDWbaAuthHeader::new(
                did_document_path,
                private_key_path,
                AUTH_MODE_HTTP_SIGNATURES,
            ),
            identity_name: identity_name.into(),
            did: did.into(),
            jwt_token: jwt_token.into(),
            persist_token,
            persistent: BTreeSet::new(),
        }
    }

    pub fn identity_name(&self) -> &str {
        &self.identity_name
    }

    pub fn did(&self) -> &str {
        &self.did
    }

    pub fn remember_scope(&mut self, server_url: &str) {
        let scope = auth_scope(server_url);
        if !scope.is_empty() {
            self.persistent.insert(scope);
        }
    }

    pub fn set_bearer(&mut self, server_url: &str, token: &str) {
        let token = token.trim();
        if token.is_empty() {
            return;
        }
        let mut headers = BTreeMap::new();
        headers.insert("Authorization".to_string(), format!("Bearer {token}"));
        let _ = self.helper.update_token(server_url, &headers);
        self.remember_scope(server_url);
        self.jwt_token = token.to_string();
    }

    pub fn current_jwt(&self) -> &str {
        &self.jwt_token
    }

    pub fn headers(
        &mut self,
        server_url: &str,
        method: &str,
        body: &[u8],
        force_new: bool,
    ) -> anyhow::Result<BTreeMap<String, String>> {
        let mut base_headers = auth_json_headers();
        let auth_headers = self.helper.get_auth_header(
            server_url,
            force_new,
            method,
            Some(&base_headers),
            Some(body),
        )?;
        base_headers.extend(auth_headers);
        Ok(base_headers)
    }

    pub fn challenge_headers(
        &mut self,
        server_url: &str,
        headers: &BTreeMap<String, String>,
        method: &str,
        body: &[u8],
    ) -> anyhow::Result<BTreeMap<String, String>> {
        let base_headers = auth_json_headers();
        Ok(self.helper.get_challenge_auth_header(
            server_url,
            headers,
            method,
            Some(&base_headers),
            Some(body),
        )?)
    }

    pub fn ensure_jwt_from_result(
        &mut self,
        request_url: &str,
        result: &serde_json::Value,
    ) -> anyhow::Result<String> {
        self.remember_scope(request_url);
        if let Some(token) = result
            .get("access_token")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|token| !token.is_empty())
        {
            self.set_bearer(request_url, token);
            self.jwt_token = token.to_string();
            if let Some(persist_token) = self.persist_token.as_mut() {
                let _ = persist_token(token);
            }
            return Ok(self.jwt_token.clone());
        }
        let token = self.jwt_token.trim();
        if !token.is_empty() {
            return Ok(token.to_string());
        }
        anyhow::bail!("did-auth get_me succeeded but no access token was returned")
    }

    pub fn capture_token(
        &mut self,
        server_url: &str,
        headers: &BTreeMap<String, String>,
    ) -> String {
        let Some(token) = self.helper.update_token(server_url, headers) else {
            return String::new();
        };
        if self.is_persistent_scope(server_url) {
            self.jwt_token = token.clone();
            if let Some(persist_token) = self.persist_token.as_mut() {
                let _ = persist_token(&token);
            }
        }
        token
    }

    pub fn clear_token(&mut self, server_url: &str) {
        self.helper.clear_token(server_url);
        if self.is_persistent_scope(server_url) {
            self.jwt_token.clear();
        }
    }

    pub fn should_retry_after_401(&self, headers: &BTreeMap<String, String>) -> bool {
        self.helper.should_retry_after_401(headers)
    }

    pub fn do_json_rpc<T, P>(
        &mut self,
        client: &HttpClient,
        request_url: &str,
        http_method: &str,
        rpc_method: &str,
        params: P,
    ) -> anyhow::Result<T>
    where
        T: DeserializeOwned,
        P: Serialize,
    {
        let payload = build_json_rpc_payload(rpc_method, serde_json::to_value(params)?);
        let body = serde_json::to_vec(&payload)?;
        let response = self.do_request(client, request_url, http_method, &body)?;
        decode_json_rpc_response(&response.body)
    }

    pub fn do_json_rpc_profile<T, P>(
        &mut self,
        client: &HttpClient,
        profile: Profile,
        request_url: &str,
        http_method: &str,
        rpc_method: &str,
        params: P,
    ) -> anyhow::Result<T>
    where
        T: DeserializeOwned,
        P: Serialize,
    {
        let payload = build_json_rpc_payload(rpc_method, serde_json::to_value(params)?);
        let body = serde_json::to_vec(&payload)?;
        let response = self.do_request_profile(client, profile, request_url, http_method, &body)?;
        decode_json_rpc_response(&response.body)
    }

    pub fn do_json_rpc_optional<P>(
        &mut self,
        client: &HttpClient,
        request_url: &str,
        http_method: &str,
        rpc_method: &str,
        params: P,
    ) -> anyhow::Result<()>
    where
        P: Serialize,
    {
        let payload = build_json_rpc_payload(rpc_method, serde_json::to_value(params)?);
        let body = serde_json::to_vec(&payload)?;
        let response = self.do_request(client, request_url, http_method, &body)?;
        decode_json_rpc_response_optional(&response.body)
    }

    pub fn ensure_jwt(&mut self, client: &HttpClient, request_url: &str) -> anyhow::Result<String> {
        self.remember_scope(request_url);
        let result: serde_json::Value =
            self.do_json_rpc(client, request_url, "POST", "get_me", serde_json::json!({}))?;
        self.ensure_jwt_from_result(request_url, &result)
    }

    pub fn ensure_jwt_profile(
        &mut self,
        client: &HttpClient,
        profile: Profile,
        request_url: &str,
    ) -> anyhow::Result<String> {
        self.remember_scope(request_url);
        let result: serde_json::Value = self.do_json_rpc_profile(
            client,
            profile,
            request_url,
            "POST",
            "get_me",
            serde_json::json!({}),
        )?;
        self.ensure_jwt_from_result(request_url, &result)
    }

    pub fn do_json<T, P>(
        &mut self,
        client: &HttpClient,
        method: &str,
        request_url: &str,
        payload: P,
    ) -> anyhow::Result<T>
    where
        T: DeserializeOwned,
        P: Serialize,
    {
        let body = serde_json::to_vec(&payload)?;
        let response = self.do_request(client, request_url, method, &body)?;
        Ok(decode_plain_json_response(&response.body)?)
    }

    pub fn do_json_profile<T, P>(
        &mut self,
        client: &HttpClient,
        profile: Profile,
        method: &str,
        request_url: &str,
        payload: P,
    ) -> anyhow::Result<T>
    where
        T: DeserializeOwned,
        P: Serialize,
    {
        let body = serde_json::to_vec(&payload)?;
        let response = self.do_request_profile(client, profile, request_url, method, &body)?;
        Ok(decode_plain_json_response(&response.body)?)
    }

    pub fn do_json_optional<P>(
        &mut self,
        client: &HttpClient,
        method: &str,
        request_url: &str,
        payload: P,
    ) -> anyhow::Result<()>
    where
        P: Serialize,
    {
        let body = serde_json::to_vec(&payload)?;
        let response = self.do_request(client, request_url, method, &body)?;
        let _ = response;
        Ok(())
    }

    fn do_request(
        &mut self,
        client: &HttpClient,
        request_url: &str,
        method: &str,
        body: &[u8],
    ) -> anyhow::Result<HttpResponse> {
        self.do_request_with_timeout(client, None, request_url, method, body)
    }

    fn do_request_profile(
        &mut self,
        client: &HttpClient,
        profile: Profile,
        request_url: &str,
        method: &str,
        body: &[u8],
    ) -> anyhow::Result<HttpResponse> {
        let timeout = client.config().timeout_for_profile(profile);
        self.do_request_with_timeout(client, Some(timeout), request_url, method, body)
    }

    fn do_request_with_timeout(
        &mut self,
        client: &HttpClient,
        timeout: Option<std::time::Duration>,
        request_url: &str,
        method: &str,
        body: &[u8],
    ) -> anyhow::Result<HttpResponse> {
        let headers = self.headers(request_url, method, body, false)?;
        let mut response =
            execute_with_headers(client, timeout, method, request_url, body, headers)?;
        if response.status_code == 401 {
            let response_headers = response_header_map(&response);
            let headers = if self.should_retry_after_401(&response_headers) {
                self.challenge_headers(request_url, &response_headers, method, body)?
            } else {
                self.clear_token(request_url);
                self.headers(request_url, method, body, true)?
            };
            response = execute_with_headers(client, timeout, method, request_url, body, headers)?;
        }
        if let Some(err) = http_status_error(response.status_code, &response.body) {
            return Err(err.into());
        }
        let response_headers = response_header_map(&response);
        self.capture_token(request_url, &response_headers);
        Ok(response)
    }

    fn is_persistent_scope(&self, server_url: &str) -> bool {
        self.persistent.contains(&auth_scope(server_url))
    }
}

fn execute_with_headers(
    client: &HttpClient,
    timeout: Option<std::time::Duration>,
    method: &str,
    request_url: &str,
    body: &[u8],
    headers: BTreeMap<String, String>,
) -> anyhow::Result<HttpResponse> {
    let mut request = HttpRequest::new(method, request_url).body(body.to_vec());
    if let Some(timeout) = timeout.filter(|timeout| !timeout.is_zero()) {
        request = request.timeout(timeout);
    }
    for (key, value) in headers {
        request.headers.push((key, value));
    }
    Ok(client.execute(request)?)
}

fn response_header_map(response: &HttpResponse) -> BTreeMap<String, String> {
    let mut headers = BTreeMap::new();
    for (key, value) in &response.headers {
        headers.entry(key.clone()).or_insert_with(|| value.clone());
    }
    headers
}

pub fn auth_json_headers() -> BTreeMap<String, String> {
    BTreeMap::from([("Content-Type".to_string(), CONTENT_TYPE_JSON.to_string())])
}

pub fn auth_scope(server_url: &str) -> String {
    let trimmed = server_url.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let Some((_, after_scheme)) = trimmed.split_once("://") else {
        return trimmed.to_string();
    };
    let authority = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(after_scheme);
    let host_port = authority
        .rsplit_once('@')
        .map(|(_, host)| host)
        .unwrap_or(authority);
    if host_port.is_empty() {
        return trimmed.to_string();
    }
    if let Some(stripped) = host_port.strip_prefix('[') {
        return stripped
            .split_once(']')
            .map(|(host, _)| host.to_string())
            .unwrap_or_else(|| host_port.to_string());
    }
    host_port
        .split_once(':')
        .map(|(host, _)| host.to_string())
        .unwrap_or_else(|| host_port.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpError {
    pub status_code: u16,
    pub message: String,
}

impl fmt::Display for HttpError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "http error {}: {}",
            self.status_code, self.message
        )
    }
}

impl std::error::Error for HttpError {}

#[derive(Debug, Clone, PartialEq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    pub data: Option<serde_json::Value>,
}

impl fmt::Display for RpcError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "rpc error {}: {}", self.code, self.message)
    }
}

impl std::error::Error for RpcError {}
