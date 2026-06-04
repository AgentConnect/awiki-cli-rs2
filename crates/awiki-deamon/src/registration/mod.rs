use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

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
    pub did_document: Value,
    pub endpoint_url: Option<String>,
    pub key_algorithm: String,
    pub public_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRegistrationExchangeResult {
    pub token_id: String,
    pub did: String,
    pub user_id: Option<String>,
    pub agent_kind: AgentKind,
    pub controller_did: String,
    pub handle: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistrationTokenMetadata {
    pub token_id: String,
    pub agent_kind: AgentKind,
    pub handle: Option<String>,
    pub controller_did: String,
    pub status: String,
    pub scope: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentLatestStatusUpdateItem {
    pub agent_did: String,
    pub agent_kind: AgentKind,
    pub status: String,
    pub last_seen_at: Option<String>,
    pub version: Option<String>,
    pub min_supported_version: Option<String>,
    pub platform: Option<String>,
    pub service: Option<String>,
    pub needs_upgrade: bool,
    pub needs_config: bool,
    pub last_error_code: Option<String>,
    pub last_error_summary: Option<String>,
    pub diagnostics_summary: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DidAuthMaterial {
    pub did_document_path: PathBuf,
    pub private_key_path: PathBuf,
    pub bearer_token: Option<String>,
}

pub trait AgentRegistrationClient {
    fn exchange_token(
        &self,
        request: AgentRegistrationExchangeRequest,
    ) -> Result<AgentRegistrationExchangeResult>;
}

pub trait AgentInventoryClient {
    fn verify_token(&self, token: &RegistrationToken) -> Result<RegistrationTokenMetadata>;

    fn update_latest_status(
        &self,
        daemon_agent_did: &str,
        statuses: Vec<AgentLatestStatusUpdateItem>,
        auth: &DidAuthMaterial,
    ) -> Result<Value>;
}

#[derive(Clone)]
pub struct UserServiceAgentRegistrationClient {
    rpc_url: String,
    inventory_rpc_url: String,
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
        let rpc_url = if trimmed.ends_with("/user-service/agent-registration/rpc") {
            trimmed.to_string()
        } else {
            format!("{trimmed}/user-service/agent-registration/rpc")
        };
        let inventory_rpc_url = if trimmed.ends_with("/user-service/agent-inventory/rpc") {
            trimmed.to_string()
        } else if trimmed.ends_with("/user-service/agent-registration/rpc") {
            trimmed.replace(
                "/user-service/agent-registration/rpc",
                "/user-service/agent-inventory/rpc",
            )
        } else {
            format!("{trimmed}/user-service/agent-inventory/rpc")
        };
        Ok(Self {
            rpc_url,
            inventory_rpc_url,
            http: reqwest::Client::new(),
        })
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
            .http
            .post(&self.rpc_url)
            .header("content-type", "application/json")
            .body(body.to_string())
            .send()
            .await
            .with_context(|| format!("call user-service agent registration {}", self.rpc_url))?
            .error_for_status()
            .context("user-service agent registration HTTP error")?;
        let bytes = response
            .bytes()
            .await
            .context("read registration response")?;
        parse_exchange_response(&bytes)
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
            .http
            .post(&self.rpc_url)
            .header("content-type", "application/json")
            .body(body.to_string())
            .send()
            .await
            .with_context(|| format!("call user-service verify token {}", self.rpc_url))?
            .error_for_status()
            .context("user-service verify token HTTP error")?;
        let bytes = response
            .bytes()
            .await
            .context("read verify token response")?;
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
        let mut request = self.http.post(&self.inventory_rpc_url).body(body_bytes);
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
            "did_document": request.did_document,
            "endpoint_url": request.endpoint_url,
            "key_algorithm": request.key_algorithm,
            "public_key": request.public_key,
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
        handle: required_string(&result, "handle")?,
        status: required_string(&result, "status")?,
    };
    Ok(parsed)
}

fn parse_verify_response(bytes: &[u8]) -> Result<RegistrationTokenMetadata> {
    let result = parse_json_rpc_result(bytes, "agent registration token verify")?;
    Ok(RegistrationTokenMetadata {
        token_id: required_string(&result, "token_id")?,
        agent_kind: AgentKind::parse(&required_string(&result, "agent_kind")?)?,
        handle: optional_string(&result, "handle"),
        controller_did: required_string(&result, "controller_did")?,
        status: required_string(&result, "status")?,
        scope: result.get("scope").cloned().unwrap_or(Value::Null),
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
    let mut helper = anp::authentication::DIDWbaAuthHeader::new(
        &auth.did_document_path,
        &auth.private_key_path,
        anp::authentication::AuthMode::HttpSignatures,
    );
    if let Some(token) = auth
        .bearer_token
        .as_deref()
        .map(str::trim)
        .filter(|token| !token.is_empty())
    {
        helper.update_token(
            url,
            &BTreeMap::from([("Authorization".to_string(), format!("Bearer {token}"))]),
        );
    }
    let auth_headers = helper
        .get_auth_header(url, false, "POST", Some(&headers), Some(body))
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn json_rpc_success_ignores_null_error_field() {
        let response = br#"{
            "jsonrpc": "2.0",
            "result": {
                "token_id": "areg_tok_1",
                "did": "did:agent:daemon",
                "user_id": "user-1",
                "agent_kind": "daemon",
                "controller_did": "did:human:alice",
                "handle": "alice-daemon",
                "status": "registered"
            },
            "error": null,
            "id": 1
        }"#;

        let parsed = parse_exchange_response(response).unwrap();

        assert_eq!(parsed.token_id, "areg_tok_1");
        assert_eq!(parsed.did, "did:agent:daemon");
        assert_eq!(parsed.agent_kind, AgentKind::Daemon);
    }
}
