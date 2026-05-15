use crate::anpsdk::{DIDWbaAuthHeader, AUTH_MODE_HTTP_SIGNATURES};
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

    fn is_persistent_scope(&self, server_url: &str) -> bool {
        self.persistent.contains(&auth_scope(server_url))
    }
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
