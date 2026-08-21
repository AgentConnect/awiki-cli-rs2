use std::collections::BTreeMap;
use std::fmt;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::agent::AgentKind;

#[derive(Clone, PartialEq, Eq)]
pub struct RegistrationToken(String);

#[derive(Clone, PartialEq, Eq)]
pub struct AgentRegistrationExchangeRequest {
    pub token: RegistrationToken,
    pub agent_kind: AgentKind,
    pub controller_did: String,
    pub handle: String,
    pub name: Option<String>,
    pub did_document: Value,
    pub endpoint_url: Option<String>,
    pub key_algorithm: String,
    pub public_key: String,
    pub allow_existing_agent_did: bool,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRegistrationExchangeResult {
    pub token_id: String,
    pub did: String,
    pub user_id: Option<String>,
    pub agent_kind: AgentKind,
    pub controller_user_id: String,
    pub controller_full_handle: String,
    pub controller_did: String,
    pub handle: String,
    pub binding_generation: Option<String>,
    pub status: String,
    pub access_token: Option<String>,
}

impl fmt::Debug for AgentRegistrationExchangeResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentRegistrationExchangeResult")
            .field("token_id", &self.token_id)
            .field("did", &self.did)
            .field("user_id", &self.user_id)
            .field("agent_kind", &self.agent_kind)
            .field("controller_user_id", &self.controller_user_id)
            .field("controller_full_handle", &self.controller_full_handle)
            .field("controller_did", &self.controller_did)
            .field("handle", &self.handle)
            .field("binding_generation", &self.binding_generation)
            .field("status", &self.status)
            .field(
                "access_token",
                &self.access_token.as_ref().map(|_| "<redacted-token>"),
            )
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistrationTokenMetadata {
    pub token_id: String,
    pub agent_kind: AgentKind,
    pub handle: Option<String>,
    pub controller_user_id: String,
    pub controller_full_handle: String,
    pub controller_did: String,
    pub status: String,
    pub scope: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControllerSenderScope {
    pub controller_user_id: String,
    pub controller_full_handle: String,
    pub controller_did: String,
    pub sender_did: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentInvocationAuthorization {
    pub allowed: bool,
    pub reason: String,
    pub agent_did: String,
    pub sender_did: String,
    pub sender_user_id: Option<String>,
    pub sender_full_handle: Option<String>,
    pub active_mode: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentLatestStatusUpdateItem {
    pub agent_did: String,
    pub agent_kind: AgentKind,
    pub status: String,
    pub last_seen_at: Option<String>,
    pub version: Option<String>,
    pub latest_version: Option<String>,
    pub min_supported_version: Option<String>,
    pub platform: Option<String>,
    pub service: Option<String>,
    pub needs_upgrade: bool,
    pub needs_config: bool,
    pub last_error_code: Option<String>,
    pub last_error_summary: Option<String>,
    pub diagnostics_summary: Value,
}

#[derive(Clone, PartialEq, Eq)]
pub struct DidAuthMaterial {
    pub did_document: Value,
    pub private_key_pem: String,
    pub bearer_token: Option<String>,
}

impl fmt::Debug for DidAuthMaterial {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DidAuthMaterial")
            .field("did_document", &self.did_document)
            .field("private_key_pem", &"<redacted-private-key>")
            .field(
                "bearer_token",
                &self.bearer_token.as_ref().map(|_| "<redacted-token>"),
            )
            .finish()
    }
}

pub trait AgentRegistrationClient {
    fn exchange_token(
        &self,
        request: AgentRegistrationExchangeRequest,
    ) -> Result<AgentRegistrationExchangeResult>;
}

#[derive(Clone, PartialEq, Eq)]
pub struct AgentLegacyUpgradeRequest {
    pub agent_kind: AgentKind,
    pub did_document: Value,
    pub endpoint_url: Option<String>,
    pub legacy_auth: DidAuthMaterial,
}

impl fmt::Debug for AgentLegacyUpgradeRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentLegacyUpgradeRequest")
            .field("agent_kind", &self.agent_kind)
            .field("did_document", &"<redacted-did-document>")
            .field("endpoint_url", &self.endpoint_url)
            .field("legacy_auth", &self.legacy_auth)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentLegacyUpgradeResult {
    pub did: String,
    pub user_id: String,
    pub binding_generation: String,
    pub access_token: String,
}

impl fmt::Debug for AgentLegacyUpgradeResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentLegacyUpgradeResult")
            .field("did", &self.did)
            .field("user_id", &self.user_id)
            .field("binding_generation", &self.binding_generation)
            .field("access_token", &"<redacted-token>")
            .finish()
    }
}

pub trait AgentLegacyUpgradeClient {
    fn resolve_agent_document(&self, did: &str) -> Result<Value>;

    fn recover_committed_agent_device(
        &self,
        im_core: &crate::ImCoreAdapter,
        target: &im_core::VNextAgentBootstrapMaterial,
    ) -> Result<AgentLegacyUpgradeResult>;

    fn update_agent_document(
        &self,
        request: AgentLegacyUpgradeRequest,
    ) -> Result<AgentLegacyUpgradeResult>;
}

pub trait AgentInventoryClient {
    fn verify_token(&self, token: &RegistrationToken) -> Result<RegistrationTokenMetadata>;

    fn sync_controller_scope(
        &self,
        daemon_agent_did: &str,
        auth: &DidAuthMaterial,
    ) -> Result<Value>;

    fn verify_controller_sender(
        &self,
        daemon_agent_did: &str,
        sender_did: &str,
        auth: &DidAuthMaterial,
    ) -> Result<ControllerSenderScope>;

    fn authorize_agent_invocation(
        &self,
        daemon_agent_did: &str,
        agent_did: &str,
        sender_did: &str,
        source_conversation_id: Option<&str>,
        source_message_id: Option<&str>,
        auth: &DidAuthMaterial,
    ) -> Result<AgentInvocationAuthorization>;

    fn update_latest_status(
        &self,
        daemon_agent_did: &str,
        statuses: Vec<AgentLatestStatusUpdateItem>,
        auth: &DidAuthMaterial,
    ) -> Result<Value>;

    fn archive_agent(
        &self,
        daemon_agent_did: &str,
        agent_did: &str,
        auth: &DidAuthMaterial,
    ) -> Result<Value>;
}

#[derive(Clone)]
pub struct UserServiceAgentRegistrationClient {
    rpc_url: String,
    inventory_rpc_url: String,
    did_auth_rpc_url: String,
    client_version_header: String,
    http: reqwest::Client,
}

impl RegistrationToken {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if value.trim().is_empty() {
            bail!("registration token must not be empty");
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for RegistrationToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RegistrationToken(<redacted>)")
    }
}

impl fmt::Debug for AgentRegistrationExchangeRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentRegistrationExchangeRequest")
            .field("token", &self.token)
            .field("agent_kind", &self.agent_kind)
            .field("controller_did", &self.controller_did)
            .field("handle", &self.handle)
            .field("name", &self.name)
            .field("did_document", &self.did_document)
            .field("endpoint_url", &self.endpoint_url)
            .field("key_algorithm", &self.key_algorithm)
            .field("public_key", &"<redacted-public-key>")
            .finish()
    }
}

impl fmt::Debug for UserServiceAgentRegistrationClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UserServiceAgentRegistrationClient")
            .field("rpc_url", &self.rpc_url)
            .field("inventory_rpc_url", &self.inventory_rpc_url)
            .field("did_auth_rpc_url", &self.did_auth_rpc_url)
            .finish_non_exhaustive()
    }
}

impl UserServiceAgentRegistrationClient {
    pub fn new(service_base_url: impl Into<String>) -> Result<Self> {
        let service_base_url = service_base_url.into();
        let trimmed = service_base_url.trim().trim_end_matches('/');
        if trimmed.is_empty() {
            bail!("service_base_url must not be empty");
        }
        let rpc_url = if trimmed.ends_with("/user-service/v1/agent-registration/rpc") {
            trimmed.to_string()
        } else {
            format!("{trimmed}/user-service/v1/agent-registration/rpc")
        };
        let inventory_rpc_url = if trimmed.ends_with("/user-service/v1/agent-inventory/rpc") {
            trimmed.to_string()
        } else if trimmed.ends_with("/user-service/v1/agent-registration/rpc") {
            trimmed.replace(
                "/user-service/v1/agent-registration/rpc",
                "/user-service/v1/agent-inventory/rpc",
            )
        } else {
            format!("{trimmed}/user-service/v1/agent-inventory/rpc")
        };
        let did_auth_rpc_url = if trimmed.ends_with("/user-service/v1/did-auth/rpc") {
            trimmed.to_string()
        } else if let Some((prefix, _)) = trimmed.split_once("/user-service/v1/") {
            format!("{prefix}/user-service/v1/did-auth/rpc")
        } else {
            format!("{trimmed}/user-service/v1/did-auth/rpc")
        };
        Ok(Self {
            rpc_url,
            inventory_rpc_url,
            did_auth_rpc_url,
            client_version_header: crate::build_info::client_version_info()?.header_value(),
            // Registration and DID-auth requests carry bearer credentials or
            // signatures bound to the original URL. Never replay them through
            // an HTTP redirect, even when the redirect remains same-origin.
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .context("build user-service HTTP client")?,
        })
    }

    fn product_request(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        request.header(
            im_core::CLIENT_VERSION_HEADER,
            self.client_version_header.as_str(),
        )
    }

    pub fn rpc_url(&self) -> &str {
        &self.rpc_url
    }

    pub fn inventory_rpc_url(&self) -> &str {
        &self.inventory_rpc_url
    }

    pub async fn exchange_token_async(
        &self,
        request: AgentRegistrationExchangeRequest,
    ) -> Result<AgentRegistrationExchangeResult> {
        let body = exchange_token_body(request);
        let response = self
            .product_request(self.http.post(&self.rpc_url))
            .header("content-type", "application/json")
            .body(body.to_string())
            .send()
            .await
            .with_context(|| format!("call user-service agent registration {}", self.rpc_url))?;
        let bytes =
            read_user_service_json_rpc_response(response, "agent registration token exchange")
                .await?;
        parse_exchange_response(&bytes)
    }

    pub async fn update_agent_document_async(
        &self,
        request: AgentLegacyUpgradeRequest,
    ) -> Result<AgentLegacyUpgradeResult> {
        let call = im_core::compat::identity::build_update_document_rpc_call(
            im_core::compat::identity::UpdateDocumentRpcParams {
                did_document: request.did_document,
                is_public: None,
                is_agent: Some(true),
                role: Some(format!("agent:{}", request.agent_kind.as_str())),
                endpoint_url: request.endpoint_url,
            },
        );
        let body_bytes = call.payload().to_string().into_bytes();
        let headers = did_auth_headers(&self.did_auth_rpc_url, &body_bytes, &request.legacy_auth)?;
        let mut http_request =
            self.product_request(self.http.post(&self.did_auth_rpc_url).body(body_bytes));
        for (key, value) in headers {
            http_request = http_request.header(key, value);
        }
        let response = http_request
            .send()
            .await
            .with_context(|| format!("call user-service DID Auth {}", self.did_auth_rpc_url))?;
        let bytes = read_user_service_json_rpc_response(response, "Agent Legacy upgrade").await?;
        parse_legacy_upgrade_response(&bytes)
    }

    pub async fn resolve_agent_document_async(&self, did: &str) -> Result<Value> {
        let url = agent_did_document_url(&self.did_auth_rpc_url, did)?;
        let response = self
            .http
            .get(&url)
            .header("accept", "application/json")
            .send()
            .await
            .with_context(|| format!("resolve Agent DID document {did}"))?;
        let status = response.status();
        if status.is_redirection() {
            bail!("Agent DID document HTTP redirect rejected: HTTP status {status}");
        }
        let bytes = response
            .bytes()
            .await
            .context("read Agent DID document response")?;
        if !status.is_success() {
            bail!("Agent DID document resolution failed: HTTP status {status}");
        }
        let document: Value =
            serde_json::from_slice(&bytes).context("parse resolved Agent DID document response")?;
        if document.get("id").and_then(Value::as_str) != Some(did)
            || (did.starts_with("did:wba:")
                && !anp::authentication::validate_did_document_binding(&document, true))
        {
            bail!("resolved Agent DID document binding is invalid");
        }
        Ok(document)
    }

    pub async fn verify_token_async(
        &self,
        token: &RegistrationToken,
    ) -> Result<RegistrationTokenMetadata> {
        let body = json!({
            "jsonrpc": "2.0",
            "method": "verify_token",
            "params": {
                "token": token.as_str(),
            },
            "id": 1
        });
        let response = self
            .product_request(self.http.post(&self.rpc_url))
            .header("content-type", "application/json")
            .body(body.to_string())
            .send()
            .await
            .with_context(|| format!("call user-service verify token {}", self.rpc_url))?;
        let bytes =
            read_user_service_json_rpc_response(response, "agent registration token verify")
                .await?;
        parse_verify_response(&bytes)
    }

    pub async fn update_latest_status_async(
        &self,
        daemon_agent_did: &str,
        statuses: Vec<AgentLatestStatusUpdateItem>,
        auth: &DidAuthMaterial,
    ) -> Result<Value> {
        let body = json!({
            "jsonrpc": "2.0",
            "method": "update_latest_status",
            "params": {
                "daemon_agent_did": daemon_agent_did,
                "statuses": statuses.into_iter().map(status_update_item_body).collect::<Vec<_>>(),
            },
            "id": 1
        });
        let body_bytes = body.to_string().into_bytes();
        let headers = did_auth_headers(&self.inventory_rpc_url, &body_bytes, auth)?;
        let mut request =
            self.product_request(self.http.post(&self.inventory_rpc_url).body(body_bytes));
        for (key, value) in headers {
            request = request.header(key, value);
        }
        let response = request
            .send()
            .await
            .with_context(|| {
                format!(
                    "call user-service agent inventory {}",
                    self.inventory_rpc_url
                )
            })?
            .error_for_status()
            .context("user-service agent inventory HTTP error")?;
        let bytes = response
            .bytes()
            .await
            .context("read agent inventory response")?;
        parse_json_rpc_result(&bytes, "agent inventory latest status update")
    }

    pub async fn sync_controller_scope_async(
        &self,
        daemon_agent_did: &str,
        auth: &DidAuthMaterial,
    ) -> Result<Value> {
        let body = json!({
            "jsonrpc": "2.0",
            "method": "sync_controller_scope",
            "params": {
                "daemon_agent_did": daemon_agent_did,
            },
            "id": 1
        });
        let body_bytes = body.to_string().into_bytes();
        let headers = did_auth_headers(&self.inventory_rpc_url, &body_bytes, auth)?;
        let mut request =
            self.product_request(self.http.post(&self.inventory_rpc_url).body(body_bytes));
        for (key, value) in headers {
            request = request.header(key, value);
        }
        let response = request
            .send()
            .await
            .with_context(|| {
                format!(
                    "call user-service agent inventory {}",
                    self.inventory_rpc_url
                )
            })?
            .error_for_status()
            .context("user-service agent inventory HTTP error")?;
        let bytes = response
            .bytes()
            .await
            .context("read sync controller scope response")?;
        parse_json_rpc_result(&bytes, "agent inventory sync_controller_scope")
    }

    pub async fn verify_controller_sender_async(
        &self,
        daemon_agent_did: &str,
        sender_did: &str,
        auth: &DidAuthMaterial,
    ) -> Result<ControllerSenderScope> {
        let body = json!({
            "jsonrpc": "2.0",
            "method": "verify_controller_sender",
            "params": {
                "daemon_agent_did": daemon_agent_did,
                "sender_did": sender_did,
            },
            "id": 1
        });
        let body_bytes = body.to_string().into_bytes();
        let headers = did_auth_headers(&self.inventory_rpc_url, &body_bytes, auth)?;
        let mut request =
            self.product_request(self.http.post(&self.inventory_rpc_url).body(body_bytes));
        for (key, value) in headers {
            request = request.header(key, value);
        }
        let response = request
            .send()
            .await
            .with_context(|| {
                format!(
                    "call user-service agent inventory {}",
                    self.inventory_rpc_url
                )
            })?
            .error_for_status()
            .context("user-service agent inventory HTTP error")?;
        let bytes = response
            .bytes()
            .await
            .context("read verify controller sender response")?;
        parse_verify_controller_sender_response(&bytes)
    }

    pub async fn authorize_agent_invocation_async(
        &self,
        daemon_agent_did: &str,
        agent_did: &str,
        sender_did: &str,
        source_conversation_id: Option<&str>,
        source_message_id: Option<&str>,
        auth: &DidAuthMaterial,
    ) -> Result<AgentInvocationAuthorization> {
        let body = json!({
            "jsonrpc": "2.0",
            "method": "authorize_agent_invocation",
            "params": {
                "daemon_agent_did": daemon_agent_did,
                "agent_did": agent_did,
                "sender_did": sender_did,
                "source_conversation_id": source_conversation_id,
                "source_message_id": source_message_id,
            },
            "id": 1
        });
        let body_bytes = body.to_string().into_bytes();
        let headers = did_auth_headers(&self.inventory_rpc_url, &body_bytes, auth)?;
        let mut request =
            self.product_request(self.http.post(&self.inventory_rpc_url).body(body_bytes));
        for (key, value) in headers {
            request = request.header(key, value);
        }
        let response = request
            .send()
            .await
            .with_context(|| {
                format!(
                    "call user-service agent inventory {}",
                    self.inventory_rpc_url
                )
            })?
            .error_for_status()
            .context("user-service agent inventory HTTP error")?;
        let bytes = response
            .bytes()
            .await
            .context("read authorize agent invocation response")?;
        parse_authorize_agent_invocation_response(&bytes)
    }

    pub async fn archive_agent_async(
        &self,
        daemon_agent_did: &str,
        agent_did: &str,
        auth: &DidAuthMaterial,
    ) -> Result<Value> {
        let body = json!({
            "jsonrpc": "2.0",
            "method": "archive_agent",
            "params": {
                "daemon_agent_did": daemon_agent_did,
                "agent_did": agent_did,
            },
            "id": 1
        });
        let body_bytes = body.to_string().into_bytes();
        let headers = did_auth_headers(&self.inventory_rpc_url, &body_bytes, auth)?;
        let mut request =
            self.product_request(self.http.post(&self.inventory_rpc_url).body(body_bytes));
        for (key, value) in headers {
            request = request.header(key, value);
        }
        let response = request
            .send()
            .await
            .with_context(|| {
                format!(
                    "call user-service agent inventory {}",
                    self.inventory_rpc_url
                )
            })?
            .error_for_status()
            .context("user-service agent inventory HTTP error")?;
        let bytes = response
            .bytes()
            .await
            .context("read archive agent response")?;
        parse_json_rpc_result(&bytes, "agent inventory archive_agent")
    }
}

impl AgentRegistrationClient for UserServiceAgentRegistrationClient {
    fn exchange_token(
        &self,
        request: AgentRegistrationExchangeRequest,
    ) -> Result<AgentRegistrationExchangeResult> {
        if tokio::runtime::Handle::try_current().is_ok() {
            let client = self.clone();
            let join = std::thread::Builder::new()
                .name("awiki-agent-registration".to_string())
                .spawn(move || client.exchange_token_in_new_runtime(request))
                .context("spawn registration RPC runtime thread")?;
            return join
                .join()
                .map_err(|_| anyhow::anyhow!("registration RPC runtime thread panicked"))?;
        }
        self.exchange_token_in_new_runtime(request)
    }
}

impl AgentLegacyUpgradeClient for UserServiceAgentRegistrationClient {
    fn resolve_agent_document(&self, did: &str) -> Result<Value> {
        let did = did.to_owned();
        if tokio::runtime::Handle::try_current().is_ok() {
            let client = self.clone();
            let join = std::thread::Builder::new()
                .name("awiki-agent-did-resolution".to_string())
                .spawn(move || {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .context("create Agent DID resolution runtime")?;
                    runtime.block_on(client.resolve_agent_document_async(&did))
                })
                .context("spawn Agent DID resolution runtime thread")?;
            return join
                .join()
                .map_err(|_| anyhow::anyhow!("Agent DID resolution runtime thread panicked"))?;
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("create Agent DID resolution runtime")?;
        runtime.block_on(self.resolve_agent_document_async(&did))
    }

    fn recover_committed_agent_device(
        &self,
        im_core: &crate::ImCoreAdapter,
        target: &im_core::VNextAgentBootstrapMaterial,
    ) -> Result<AgentLegacyUpgradeResult> {
        let recovered = im_core.refresh_committed_vnext_agent_legacy_upgrade_session(target)?;
        Ok(AgentLegacyUpgradeResult {
            did: recovered.did.as_str().to_owned(),
            user_id: recovered.user_id,
            binding_generation: recovered.binding_generation,
            access_token: recovered.access_token,
        })
    }

    fn update_agent_document(
        &self,
        request: AgentLegacyUpgradeRequest,
    ) -> Result<AgentLegacyUpgradeResult> {
        if tokio::runtime::Handle::try_current().is_ok() {
            let client = self.clone();
            let join = std::thread::Builder::new()
                .name("awiki-agent-legacy-upgrade".to_string())
                .spawn(move || {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .context("create Agent Legacy upgrade runtime")?;
                    runtime.block_on(client.update_agent_document_async(request))
                })
                .context("spawn Agent Legacy upgrade RPC runtime thread")?;
            return join.join().map_err(|_| {
                anyhow::anyhow!("Agent Legacy upgrade RPC runtime thread panicked")
            })?;
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("create Agent Legacy upgrade runtime")?;
        runtime.block_on(self.update_agent_document_async(request))
    }
}

fn agent_did_document_url(service_rpc_url: &str, did: &str) -> Result<String> {
    let suffix = did
        .strip_prefix("did:wba:")
        .or_else(|| did.strip_prefix("did:web:"))
        .context("unsupported Agent DID method for resolution")?;
    let mut segments = suffix.split(':');
    let domain = segments
        .next()
        .filter(|value| !value.is_empty())
        .context("Agent DID domain is missing")?;
    if domain.contains('%') || domain.contains('/') || domain.contains('\\') {
        bail!("Agent DID domain is invalid");
    }
    let path = segments
        .map(percent_decode_did_segment)
        .collect::<Result<Vec<_>>>()?;
    let resolution_path = if path.is_empty() {
        "/.well-known/did.json".to_owned()
    } else {
        format!("/{}/did.json", path.join("/"))
    };
    let service_url = reqwest::Url::parse(service_rpc_url)
        .context("parse user-service URL for Agent DID resolution")?;
    let service_host = service_url.host_str().unwrap_or_default();
    let use_configured_origin = service_host.eq_ignore_ascii_case(domain)
        || service_host.eq_ignore_ascii_case("localhost")
        || service_host == "127.0.0.1"
        || service_host == "::1";
    if use_configured_origin {
        let mut origin = format!("{}://{}", service_url.scheme(), service_host);
        if let Some(port) = service_url.port() {
            origin.push_str(&format!(":{port}"));
        }
        return Ok(format!("{origin}{resolution_path}"));
    }
    Ok(format!("https://{domain}{resolution_path}"))
}

fn percent_decode_did_segment(value: &str) -> Result<String> {
    if value.is_empty() {
        bail!("Agent DID path segment is empty");
    }
    let mut decoded = Vec::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                bail!("Agent DID path percent encoding is invalid");
            }
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3])?;
            decoded.push(u8::from_str_radix(hex, 16).context("decode Agent DID path segment")?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    let decoded = String::from_utf8(decoded).context("Agent DID path segment is not utf-8")?;
    if decoded.is_empty()
        || decoded.contains('/')
        || decoded.contains('\\')
        || decoded == "."
        || decoded == ".."
    {
        bail!("Agent DID path segment is invalid");
    }
    Ok(decoded)
}

impl AgentInventoryClient for UserServiceAgentRegistrationClient {
    fn verify_token(&self, token: &RegistrationToken) -> Result<RegistrationTokenMetadata> {
        if tokio::runtime::Handle::try_current().is_ok() {
            let client = self.clone();
            let token = token.clone();
            let join = std::thread::Builder::new()
                .name("awiki-agent-token-verify".to_string())
                .spawn(move || client.verify_token_in_new_runtime(&token))
                .context("spawn verify token RPC runtime thread")?;
            return join
                .join()
                .map_err(|_| anyhow::anyhow!("verify token RPC runtime thread panicked"))?;
        }
        self.verify_token_in_new_runtime(token)
    }

    fn update_latest_status(
        &self,
        daemon_agent_did: &str,
        statuses: Vec<AgentLatestStatusUpdateItem>,
        auth: &DidAuthMaterial,
    ) -> Result<Value> {
        if tokio::runtime::Handle::try_current().is_ok() {
            let client = self.clone();
            let daemon_agent_did = daemon_agent_did.to_string();
            let auth = auth.clone();
            let join = std::thread::Builder::new()
                .name("awiki-agent-inventory".to_string())
                .spawn(move || {
                    client.update_latest_status_in_new_runtime(&daemon_agent_did, statuses, &auth)
                })
                .context("spawn inventory RPC runtime thread")?;
            return join
                .join()
                .map_err(|_| anyhow::anyhow!("inventory RPC runtime thread panicked"))?;
        }
        self.update_latest_status_in_new_runtime(daemon_agent_did, statuses, auth)
    }

    fn sync_controller_scope(
        &self,
        daemon_agent_did: &str,
        auth: &DidAuthMaterial,
    ) -> Result<Value> {
        if tokio::runtime::Handle::try_current().is_ok() {
            let client = self.clone();
            let daemon_agent_did = daemon_agent_did.to_string();
            let auth = auth.clone();
            let join = std::thread::Builder::new()
                .name("awiki-agent-scope-sync".to_string())
                .spawn(move || {
                    client.sync_controller_scope_in_new_runtime(&daemon_agent_did, &auth)
                })
                .context("spawn scope sync RPC runtime thread")?;
            return join
                .join()
                .map_err(|_| anyhow::anyhow!("scope sync RPC runtime thread panicked"))?;
        }
        self.sync_controller_scope_in_new_runtime(daemon_agent_did, auth)
    }

    fn verify_controller_sender(
        &self,
        daemon_agent_did: &str,
        sender_did: &str,
        auth: &DidAuthMaterial,
    ) -> Result<ControllerSenderScope> {
        if tokio::runtime::Handle::try_current().is_ok() {
            let client = self.clone();
            let daemon_agent_did = daemon_agent_did.to_string();
            let sender_did = sender_did.to_string();
            let auth = auth.clone();
            let join = std::thread::Builder::new()
                .name("awiki-agent-sender-scope".to_string())
                .spawn(move || {
                    client.verify_controller_sender_in_new_runtime(
                        &daemon_agent_did,
                        &sender_did,
                        &auth,
                    )
                })
                .context("spawn sender scope RPC runtime thread")?;
            return join
                .join()
                .map_err(|_| anyhow::anyhow!("sender scope RPC runtime thread panicked"))?;
        }
        self.verify_controller_sender_in_new_runtime(daemon_agent_did, sender_did, auth)
    }

    fn authorize_agent_invocation(
        &self,
        daemon_agent_did: &str,
        agent_did: &str,
        sender_did: &str,
        source_conversation_id: Option<&str>,
        source_message_id: Option<&str>,
        auth: &DidAuthMaterial,
    ) -> Result<AgentInvocationAuthorization> {
        if tokio::runtime::Handle::try_current().is_ok() {
            let client = self.clone();
            let daemon_agent_did = daemon_agent_did.to_string();
            let agent_did = agent_did.to_string();
            let sender_did = sender_did.to_string();
            let source_conversation_id = source_conversation_id.map(str::to_string);
            let source_message_id = source_message_id.map(str::to_string);
            let auth = auth.clone();
            let join = std::thread::Builder::new()
                .name("awiki-agent-invocation-auth".to_string())
                .spawn(move || {
                    client.authorize_agent_invocation_in_new_runtime(
                        &daemon_agent_did,
                        &agent_did,
                        &sender_did,
                        source_conversation_id.as_deref(),
                        source_message_id.as_deref(),
                        &auth,
                    )
                })
                .context("spawn invocation authorization RPC runtime thread")?;
            return join.join().map_err(|_| {
                anyhow::anyhow!("invocation authorization RPC runtime thread panicked")
            })?;
        }
        self.authorize_agent_invocation_in_new_runtime(
            daemon_agent_did,
            agent_did,
            sender_did,
            source_conversation_id,
            source_message_id,
            auth,
        )
    }

    fn archive_agent(
        &self,
        daemon_agent_did: &str,
        agent_did: &str,
        auth: &DidAuthMaterial,
    ) -> Result<Value> {
        if tokio::runtime::Handle::try_current().is_ok() {
            let client = self.clone();
            let daemon_agent_did = daemon_agent_did.to_string();
            let agent_did = agent_did.to_string();
            let auth = auth.clone();
            let join = std::thread::Builder::new()
                .name("awiki-agent-archive".to_string())
                .spawn(move || {
                    client.archive_agent_in_new_runtime(&daemon_agent_did, &agent_did, &auth)
                })
                .context("spawn archive agent RPC runtime thread")?;
            return join
                .join()
                .map_err(|_| anyhow::anyhow!("archive agent RPC runtime thread panicked"))?;
        }
        self.archive_agent_in_new_runtime(daemon_agent_did, agent_did, auth)
    }
}

impl UserServiceAgentRegistrationClient {
    fn exchange_token_in_new_runtime(
        &self,
        request: AgentRegistrationExchangeRequest,
    ) -> Result<AgentRegistrationExchangeResult> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("create registration RPC runtime")?;
        runtime.block_on(self.exchange_token_async(request))
    }

    fn verify_token_in_new_runtime(
        &self,
        token: &RegistrationToken,
    ) -> Result<RegistrationTokenMetadata> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("create verify token RPC runtime")?;
        runtime.block_on(self.verify_token_async(token))
    }

    fn update_latest_status_in_new_runtime(
        &self,
        daemon_agent_did: &str,
        statuses: Vec<AgentLatestStatusUpdateItem>,
        auth: &DidAuthMaterial,
    ) -> Result<Value> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("create inventory RPC runtime")?;
        runtime.block_on(self.update_latest_status_async(daemon_agent_did, statuses, auth))
    }

    fn sync_controller_scope_in_new_runtime(
        &self,
        daemon_agent_did: &str,
        auth: &DidAuthMaterial,
    ) -> Result<Value> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("create scope sync RPC runtime")?;
        runtime.block_on(self.sync_controller_scope_async(daemon_agent_did, auth))
    }

    fn verify_controller_sender_in_new_runtime(
        &self,
        daemon_agent_did: &str,
        sender_did: &str,
        auth: &DidAuthMaterial,
    ) -> Result<ControllerSenderScope> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("create sender scope RPC runtime")?;
        runtime.block_on(self.verify_controller_sender_async(daemon_agent_did, sender_did, auth))
    }

    fn authorize_agent_invocation_in_new_runtime(
        &self,
        daemon_agent_did: &str,
        agent_did: &str,
        sender_did: &str,
        source_conversation_id: Option<&str>,
        source_message_id: Option<&str>,
        auth: &DidAuthMaterial,
    ) -> Result<AgentInvocationAuthorization> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("create invocation authorization RPC runtime")?;
        runtime.block_on(self.authorize_agent_invocation_async(
            daemon_agent_did,
            agent_did,
            sender_did,
            source_conversation_id,
            source_message_id,
            auth,
        ))
    }

    fn archive_agent_in_new_runtime(
        &self,
        daemon_agent_did: &str,
        agent_did: &str,
        auth: &DidAuthMaterial,
    ) -> Result<Value> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("create archive agent RPC runtime")?;
        runtime.block_on(self.archive_agent_async(daemon_agent_did, agent_did, auth))
    }
}

fn exchange_token_body(request: AgentRegistrationExchangeRequest) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "exchange_token",
        "params": {
            "token": request.token.as_str(),
            "agent_kind": request.agent_kind.as_str(),
            "controller_did": request.controller_did,
            "handle": request.handle,
            "name": request.name,
            "did_document": request.did_document,
            "endpoint_url": request.endpoint_url,
            "key_algorithm": request.key_algorithm,
            "public_key": request.public_key,
            "allow_existing_agent_did": request.allow_existing_agent_did,
        },
        "id": 1
    })
}

fn parse_exchange_response(bytes: &[u8]) -> Result<AgentRegistrationExchangeResult> {
    let result = parse_json_rpc_result(bytes, "agent registration token exchange")?;
    let parsed = AgentRegistrationExchangeResult {
        token_id: required_string(&result, "token_id")?,
        did: required_string(&result, "did")?,
        user_id: optional_string(&result, "user_id"),
        agent_kind: AgentKind::parse(&required_string(&result, "agent_kind")?)?,
        controller_did: required_string(&result, "controller_did")?,
        controller_user_id: required_string(&result, "controller_user_id")?,
        controller_full_handle: required_string(&result, "controller_full_handle")?,
        handle: required_string(&result, "handle")?,
        binding_generation: optional_string(&result, "binding_generation"),
        status: required_string(&result, "status")?,
        access_token: optional_auth_token(&result),
    };
    Ok(parsed)
}

fn parse_legacy_upgrade_response(bytes: &[u8]) -> Result<AgentLegacyUpgradeResult> {
    let result = parse_json_rpc_result(bytes, "Agent Legacy upgrade")?;
    let did = required_string(&result, "did")?;
    let user_id = required_string(&result, "user_id")?;
    let binding_generation = required_string(&result, "binding_generation")?;
    let access_token = required_string(&result, "access_token")?;
    if did.trim().is_empty()
        || user_id.trim().is_empty()
        || access_token.trim().is_empty()
        || binding_generation.starts_with('0')
        || !binding_generation.bytes().all(|byte| byte.is_ascii_digit())
    {
        bail!("Agent Legacy upgrade response contains an invalid exact-device binding");
    }
    Ok(AgentLegacyUpgradeResult {
        did,
        user_id,
        binding_generation,
        access_token,
    })
}

fn parse_verify_response(bytes: &[u8]) -> Result<RegistrationTokenMetadata> {
    let result = parse_json_rpc_result(bytes, "agent registration token verify")?;
    Ok(RegistrationTokenMetadata {
        token_id: required_string(&result, "token_id")?,
        agent_kind: AgentKind::parse(&required_string(&result, "agent_kind")?)?,
        handle: optional_string(&result, "handle"),
        controller_user_id: required_string(&result, "controller_user_id")?,
        controller_full_handle: required_string(&result, "controller_full_handle")?,
        controller_did: required_string(&result, "controller_did")?,
        status: required_string(&result, "status")?,
        scope: result.get("scope").cloned().unwrap_or(Value::Null),
    })
}

async fn read_user_service_json_rpc_response(
    response: reqwest::Response,
    context: &str,
) -> Result<Vec<u8>> {
    let status = response.status();
    if status.is_redirection() {
        // Do not expose Location: it is untrusted and unnecessary diagnostic
        // data. More importantly, never include request credentials here.
        bail!("{context} HTTP redirect rejected: HTTP status {status}");
    }
    let bytes = response
        .bytes()
        .await
        .with_context(|| format!("read {context} response"))?
        .to_vec();
    if status.is_success() {
        return Ok(bytes);
    }
    match parse_json_rpc_result(&bytes, context) {
        Ok(_) => bail!("{context} HTTP error: HTTP status {status}"),
        Err(error) if is_json_rpc_business_error(&bytes) => Err(error),
        Err(_) => bail!("{context} HTTP error: HTTP status {status}"),
    }
}

fn is_json_rpc_business_error(bytes: &[u8]) -> bool {
    serde_json::from_slice::<Value>(bytes)
        .ok()
        .and_then(|value| value.get("error").cloned())
        .is_some_and(|error| !error.is_null())
}

fn parse_verify_controller_sender_response(bytes: &[u8]) -> Result<ControllerSenderScope> {
    let result = parse_json_rpc_result(bytes, "agent inventory verify controller sender")?;
    Ok(ControllerSenderScope {
        controller_user_id: required_string(&result, "controller_user_id")?,
        controller_full_handle: required_string(&result, "controller_full_handle")?,
        controller_did: required_string(&result, "controller_did")?,
        sender_did: required_string(&result, "sender_did")?,
    })
}

fn parse_authorize_agent_invocation_response(bytes: &[u8]) -> Result<AgentInvocationAuthorization> {
    let result = parse_json_rpc_result(bytes, "agent inventory authorize agent invocation")?;
    let allowed = result
        .get("allowed")
        .and_then(Value::as_bool)
        .context("authorization response missing bool field allowed")?;
    let sender_user_id = optional_string(&result, "sender_user_id");
    if allowed && sender_user_id.as_deref().is_none_or(str::is_empty) {
        bail!("authorization allowed response missing sender_user_id");
    }
    Ok(AgentInvocationAuthorization {
        allowed,
        reason: required_string(&result, "reason")?,
        agent_did: required_string(&result, "agent_did")?,
        sender_did: required_string(&result, "sender_did")?,
        sender_user_id,
        sender_full_handle: optional_string(&result, "sender_full_handle"),
        active_mode: required_string(&result, "active_mode")?,
    })
}

fn parse_json_rpc_result(bytes: &[u8], context: &str) -> Result<Value> {
    let value: Value =
        serde_json::from_slice(bytes).with_context(|| format!("parse {context} response"))?;
    if let Some(error) = value.get("error").filter(|error| !error.is_null()) {
        let reason = error
            .get("data")
            .and_then(|data| data.get("reason"))
            .and_then(Value::as_str)
            .or_else(|| error.get("data").and_then(Value::as_str))
            .or_else(|| error.get("message").and_then(Value::as_str))
            .unwrap_or("unknown");
        bail!("{context} failed: {reason}");
    }
    value
        .get("result")
        .cloned()
        .with_context(|| format!("{context} response missing result"))
}

fn status_update_item_body(item: AgentLatestStatusUpdateItem) -> Value {
    json!({
        "agent_did": item.agent_did,
        "agent_kind": item.agent_kind.as_str(),
        "status": item.status,
        "last_seen_at": item.last_seen_at,
        "version": item.version,
        "latest_version": item.latest_version,
        "min_supported_version": item.min_supported_version,
        "platform": item.platform,
        "service": item.service,
        "needs_upgrade": item.needs_upgrade,
        "needs_config": item.needs_config,
        "last_error_code": item.last_error_code,
        "last_error_summary": item.last_error_summary,
        "diagnostics_summary": if item.diagnostics_summary.is_object() {
            item.diagnostics_summary
        } else {
            json!({})
        },
    })
}

fn did_auth_headers(
    url: &str,
    body: &[u8],
    auth: &DidAuthMaterial,
) -> Result<BTreeMap<String, String>> {
    let mut headers =
        BTreeMap::from([("Content-Type".to_string(), "application/json".to_string())]);
    if let Some(token) = auth
        .bearer_token
        .as_deref()
        .map(str::trim)
        .filter(|token| !token.is_empty())
    {
        headers.insert("Authorization".to_string(), format!("Bearer {token}"));
        return Ok(headers);
    }
    let private_key =
        anp::PrivateKeyMaterial::from_pem(&auth.private_key_pem).map_err(|error| {
            anyhow::anyhow!("build DID auth headers: invalid private key material: {error}")
        })?;
    let auth_headers = anp::authentication::generate_http_signature_headers(
        &auth.did_document,
        url,
        "POST",
        &private_key,
        Some(&headers),
        Some(body),
        anp::authentication::HttpSignatureOptions::default(),
    )
    .map_err(|error| anyhow::anyhow!("build DID auth headers: {error}"))?;
    headers.extend(auth_headers);
    Ok(headers)
}

fn required_string(value: &Value, field: &str) -> Result<String> {
    let value = value
        .get(field)
        .and_then(Value::as_str)
        .with_context(|| format!("registration response missing string field {field}"))?;
    Ok(value.to_string())
}

fn optional_string(value: &Value, field: &str) -> Option<String> {
    value.get(field).and_then(Value::as_str).map(str::to_string)
}

fn optional_auth_token(value: &Value) -> Option<String> {
    ["access_token", "jwt_token", "bearer_token"]
        .iter()
        .filter_map(|field| value.get(*field).and_then(Value::as_str))
        .map(str::trim)
        .find(|token| !token.is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
pub(crate) fn mock_vnext_exchange_fields(
    request: &AgentRegistrationExchangeRequest,
    account_id: &str,
) -> Result<(String, String, String)> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use std::time::{SystemTime, UNIX_EPOCH};

    let did = request
        .did_document
        .get("id")
        .and_then(Value::as_str)
        .context("mock vNext DID document missing id")?
        .to_owned();
    let manifest = anp::authentication::validate_device_manifest(&request.did_document)
        .map_err(|error| anyhow::anyhow!("mock vNext Manifest is invalid: {error}"))?
        .context("mock vNext DID document missing Manifest")?;
    if manifest.devices.len() != 1 {
        bail!("mock vNext Manifest must contain exactly one device");
    }
    let device = &manifest.devices[0];
    let response_handle = request.handle.clone();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before Unix epoch")?
        .as_secs();
    let claims = json!({
        "iss": "user-service",
        "aud": ["awiki-user-service", "awiki-message-service"],
        "sub": did,
        "type": "access",
        "purpose": "awiki.device.access.v1",
        "did": did,
        "user_id": account_id,
        "device_id": device.device_id,
        "key_id": device.signing_key_id,
        "auth_generation": 1,
        "scopes": ["device:manage", "device:read", "message:connect"],
        "iat": now,
        "nbf": now,
        "exp": now + 3600,
        "jti": format!("mock-device-{}", device.device_id),
    });
    let token = format!(
        "e30.{}.test-signature",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims)?)
    );
    Ok((did, response_handle, token))
}

#[cfg(test)]
pub(crate) fn store_mock_vnext_device_identity(
    config: &crate::DaemonConfig,
    state: &crate::state::DaemonState,
    agent_kind: AgentKind,
    handle_local_part: &str,
) -> Result<crate::state::AgentDeviceIdentityRecord> {
    let adapter = crate::ImCoreAdapter::open(config)?;
    let generated = adapter.generate_vnext_agent_bootstrap(agent_kind, handle_local_part)?;
    let account_id = format!("test-account-{handle_local_part}");
    let request = AgentRegistrationExchangeRequest {
        token: RegistrationToken::new("test-registration-token")?,
        agent_kind,
        controller_did: "did:wba:test:controller".to_owned(),
        handle: handle_local_part.to_owned(),
        name: None,
        did_document: generated.did_document.clone(),
        endpoint_url: None,
        key_algorithm: "Ed25519".to_owned(),
        public_key: generated.device_signing_public_key_pem.clone(),
        allow_existing_agent_did: false,
    };
    let (did, response_handle, access_token) = mock_vnext_exchange_fields(&request, &account_id)?;
    let full_handle = if response_handle.contains('.') {
        response_handle.clone()
    } else {
        format!("{}.{}", response_handle, config.did_domain)
    };
    let identity = crate::state::AgentDeviceIdentityRecord {
        identity_id: generated.identity_id,
        agent_did: did,
        handle: response_handle,
        display_name: handle_local_part.to_owned(),
        agent_kind,
        account_id,
        full_handle,
        binding_generation: "1".to_owned(),
        did_document: generated.did_document,
        protocol_device_id: generated.protocol_device_id.as_str().to_owned(),
        root_key_id: generated.root_key_id,
        root_private_key_pem: generated.root_private_key_pem,
        device_signing_key_id: generated.device_signing_key_id,
        device_signing_private_key_pem: generated.device_signing_private_key_pem,
        device_e2ee_key_id: generated.device_e2ee_key_id,
        device_e2ee_private_key_pem: generated.device_e2ee_private_key_pem,
        daemon_subkey_package_json: None,
        authorization_status: "active".to_owned(),
        role: "admin".to_owned(),
        management_ready: true,
        auth_generation: 1,
        access_token,
        document_version: 1,
        document_hash: generated.document_hash,
        registry_version: 1,
        identity_status: "active".to_owned(),
        legacy_migration_state: "not_required".to_owned(),
        last_error_code: None,
    };
    adapter.client_for_agent_device_identity(&identity)?;
    state.store_agent_device_identity(&identity)?;
    Ok(identity)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    #[test]
    fn token_debug_is_redacted() {
        let token = RegistrationToken::new("tok_secret_value").unwrap();
        assert!(!format!("{token:?}").contains("tok_secret_value"));
    }

    #[test]
    fn json_rpc_error_uses_stable_reason_without_token() {
        let response = br#"{
            "jsonrpc": "2.0",
            "error": {
                "code": -32004,
                "message": "scope mismatch",
                "data": { "reason": "scope_mismatch" }
            },
            "id": 1
        }"#;
        let error = parse_exchange_response(response).unwrap_err();
        assert!(error.to_string().contains("scope_mismatch"));
    }

    #[test]
    fn json_rpc_business_error_is_detected_before_http_status_mapping() {
        let response = br#"{
            "jsonrpc": "2.0",
            "error": {
                "code": -32000,
                "message": "Registration token is expired",
                "data": { "reason": "expired" }
            },
            "id": 1
        }"#;

        assert!(is_json_rpc_business_error(response));
        let error = parse_verify_response(response).unwrap_err();
        assert!(error.to_string().contains("expired"));
    }

    #[test]
    fn json_rpc_success_ignores_null_error_field() {
        let response = br#"{
            "jsonrpc": "2.0",
            "result": {
                "token_id": "areg_tok_1",
                "did": "did:agent:daemon",
                "user_id": "user-1",
                "agent_kind": "daemon",
                "controller_user_id": "user-alice",
                "controller_full_handle": "alice.anpclaw.com",
                "controller_did": "did:human:alice",
                "handle": "alice-daemon",
                "status": "registered",
                "access_token": "jwt-agent-secret"
            },
            "error": null,
            "id": 1
        }"#;

        let parsed = parse_exchange_response(response).unwrap();

        assert_eq!(parsed.token_id, "areg_tok_1");
        assert_eq!(parsed.did, "did:agent:daemon");
        assert_eq!(parsed.agent_kind, AgentKind::Daemon);
        assert_eq!(parsed.access_token.as_deref(), Some("jwt-agent-secret"));
        assert!(!format!("{parsed:?}").contains("jwt-agent-secret"));
        assert!(format!("{parsed:?}").contains("<redacted-token>"));
    }

    fn redirect_exchange_request(token: &str) -> AgentRegistrationExchangeRequest {
        AgentRegistrationExchangeRequest {
            token: RegistrationToken::new(token).unwrap(),
            agent_kind: AgentKind::Daemon,
            controller_did: "did:wba:awiki.info:controller".to_owned(),
            handle: "redirect-test".to_owned(),
            name: Some("redirect-test".to_owned()),
            did_document: serde_json::json!({"id": "did:wba:awiki.info:agent:daemon:redirect"}),
            endpoint_url: None,
            key_algorithm: "Ed25519".to_owned(),
            public_key: "test-public-key".to_owned(),
            allow_existing_agent_did: false,
        }
    }

    fn spawn_redirect_server(
        status: u16,
        location: String,
    ) -> (String, Arc<AtomicUsize>, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&requests);
        let join = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            observed.fetch_add(1, Ordering::SeqCst);
            stream
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            let mut request = [0_u8; 16 * 1024];
            let _ = stream.read(&mut request);
            write!(
                stream,
                "HTTP/1.1 {status} Temporary Redirect\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
            drop(stream);

            listener.set_nonblocking(true).unwrap();
            let deadline = Instant::now() + Duration::from_millis(300);
            while Instant::now() < deadline {
                match listener.accept() {
                    Ok((_stream, _)) => {
                        observed.fetch_add(1, Ordering::SeqCst);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("accept redirected request: {error}"),
                }
            }
        });
        (format!("http://{address}"), requests, join)
    }

    #[tokio::test]
    async fn registration_product_request_captures_one_daemon_client_version_header() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let join = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 2048];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let read = stream.read(&mut buffer).unwrap();
                assert!(read > 0, "request closed before headers");
                request.extend_from_slice(&buffer[..read]);
            }
            stream
                .write_all(
                    b"HTTP/1.1 307 Temporary Redirect\r\nLocation: /not-replayed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
            String::from_utf8_lossy(&request).into_owned()
        });
        let client = UserServiceAgentRegistrationClient::new(format!("http://{address}")).unwrap();

        let error = client
            .exchange_token_async(redirect_exchange_request("capture-version-header"))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("HTTP redirect rejected"));

        let request = join.join().unwrap();
        let version_headers = request
            .lines()
            .filter(|line| {
                line.split_once(':').is_some_and(|(name, _)| {
                    name.eq_ignore_ascii_case(im_core::CLIENT_VERSION_HEADER)
                })
            })
            .collect::<Vec<_>>();
        assert_eq!(version_headers.len(), 1);
        let expected = crate::build_info::client_version_info()
            .unwrap()
            .header_value();
        assert_eq!(
            version_headers[0]
                .split_once(':')
                .map(|(_, value)| value.trim()),
            Some(expected.as_str())
        );
    }

    #[tokio::test]
    async fn exchange_token_rejects_same_origin_redirect_without_replaying_token() {
        let secret = "registration-token-must-not-leak-same-origin";
        let (base_url, requests, join) =
            spawn_redirect_server(307, "/redirected-registration".to_owned());
        let client = UserServiceAgentRegistrationClient::new(&base_url).unwrap();

        let error = client
            .exchange_token_async(redirect_exchange_request(secret))
            .await
            .unwrap_err();
        let error = format!("{error:#}");
        assert!(error.contains("HTTP redirect rejected"));
        assert!(!error.contains(secret));
        join.join().unwrap();
        assert_eq!(requests.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn exchange_token_rejects_cross_origin_redirect_without_replaying_token() {
        let secret = "registration-token-must-not-leak-cross-origin";
        let target = TcpListener::bind("127.0.0.1:0").unwrap();
        target.set_nonblocking(true).unwrap();
        let target_address = target.local_addr().unwrap();
        let (base_url, requests, join) =
            spawn_redirect_server(308, format!("http://{target_address}/credential-collector"));
        let client = UserServiceAgentRegistrationClient::new(&base_url).unwrap();

        let error = client
            .exchange_token_async(redirect_exchange_request(secret))
            .await
            .unwrap_err();
        let error = format!("{error:#}");
        assert!(error.contains("HTTP redirect rejected"));
        assert!(!error.contains(secret));
        join.join().unwrap();
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        assert!(
            target.accept().is_err(),
            "token request followed cross-origin redirect"
        );
    }
}
