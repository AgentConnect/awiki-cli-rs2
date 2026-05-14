use crate::anpsdk::{DIDWbaAuthHeader, AUTH_MODE_HTTP_SIGNATURES};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

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
