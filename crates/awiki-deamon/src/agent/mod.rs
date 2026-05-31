use std::path::PathBuf;

use anp::authentication::{
    build_anp_message_service, create_did_wba_document, AnpMessageServiceOptions,
    DidDocumentOptions, DidProfile, VM_KEY_AUTH, VM_KEY_E2EE_AGREEMENT, VM_KEY_E2EE_SIGNING,
};
use anyhow::{bail, Context, Result};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::DaemonConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    Daemon,
    Runtime,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentDefinition {
    pub agent_did: String,
    pub handle: String,
    pub agent_kind: AgentKind,
    pub controller_did: String,
    pub runtime_plugin_id: Option<String>,
    pub runtime_profile_id: Option<String>,
    pub workspace_id: Option<String>,
    pub policy_id: String,
    pub local_agent_db_path: String,
    pub message_db_path: String,
    pub status: String,
}

#[derive(Clone, PartialEq, Eq)]
pub struct GeneratedAgentIdentity {
    pub did: String,
    pub did_document: Value,
    pub endpoint_url: Option<String>,
    pub key_algorithm: String,
    pub public_key: String,
    pub auth_private_key_pem: String,
    pub e2ee_signing_private_key_pem: String,
    pub e2ee_agreement_private_key_pem: String,
}

#[derive(Clone, PartialEq, Eq)]
pub struct AgentIdentityRecord {
    pub agent_did: String,
    pub handle: String,
    pub agent_kind: AgentKind,
    pub did_document: Value,
    pub endpoint_url: Option<String>,
    pub key_algorithm: String,
    pub public_key: String,
    pub auth_private_key_pem: String,
    pub e2ee_signing_private_key_pem: String,
    pub e2ee_agreement_private_key_pem: String,
}

impl AgentKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Daemon => "daemon",
            Self::Runtime => "runtime",
        }
    }

    pub fn parse(input: &str) -> Result<Self> {
        match input {
            "daemon" => Ok(Self::Daemon),
            "runtime" => Ok(Self::Runtime),
            other => bail!("unsupported agent kind: {other}"),
        }
    }
}

impl AgentDefinition {
    pub fn validate(&self) -> Result<()> {
        if self.agent_did.trim().is_empty() {
            bail!("agent_did must not be empty");
        }
        if self.handle.trim().is_empty() {
            bail!("handle must not be empty");
        }
        if self.controller_did.trim().is_empty() {
            bail!("controller_did must not be empty");
        }
        if self.policy_id.trim().is_empty() {
            bail!("policy_id must not be empty");
        }
        if self.local_agent_db_path.trim().is_empty() {
            bail!("local_agent_db_path must not be empty");
        }
        if self.message_db_path.trim().is_empty() {
            bail!("message_db_path must not be empty");
        }
        if self.status.trim().is_empty() {
            bail!("status must not be empty");
        }
        if self.agent_kind == AgentKind::Runtime
            && self
                .runtime_profile_id
                .as_deref()
                .is_none_or(|value| value.trim().is_empty())
        {
            bail!("runtime agent must have runtime_profile_id");
        }
        Ok(())
    }
}

impl std::fmt::Debug for GeneratedAgentIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GeneratedAgentIdentity")
            .field("did", &self.did)
            .field("did_document", &self.did_document)
            .field("endpoint_url", &self.endpoint_url)
            .field("key_algorithm", &self.key_algorithm)
            .field("public_key", &"<redacted-public-key>")
            .field("auth_private_key_pem", &"<redacted-private-key>")
            .field("e2ee_signing_private_key_pem", &"<redacted-private-key>")
            .field("e2ee_agreement_private_key_pem", &"<redacted-private-key>")
            .finish()
    }
}

impl std::fmt::Debug for AgentIdentityRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentIdentityRecord")
            .field("agent_did", &self.agent_did)
            .field("handle", &self.handle)
            .field("agent_kind", &self.agent_kind)
            .field("did_document", &self.did_document)
            .field("endpoint_url", &self.endpoint_url)
            .field("key_algorithm", &self.key_algorithm)
            .field("public_key", &"<redacted-public-key>")
            .field("auth_private_key_pem", &"<redacted-private-key>")
            .field("e2ee_signing_private_key_pem", &"<redacted-private-key>")
            .field("e2ee_agreement_private_key_pem", &"<redacted-private-key>")
            .finish()
    }
}

impl GeneratedAgentIdentity {
    pub fn into_record(self, handle: String, agent_kind: AgentKind) -> AgentIdentityRecord {
        AgentIdentityRecord {
            agent_did: self.did,
            handle,
            agent_kind,
            did_document: self.did_document,
            endpoint_url: self.endpoint_url,
            key_algorithm: self.key_algorithm,
            public_key: self.public_key,
            auth_private_key_pem: self.auth_private_key_pem,
            e2ee_signing_private_key_pem: self.e2ee_signing_private_key_pem,
            e2ee_agreement_private_key_pem: self.e2ee_agreement_private_key_pem,
        }
    }
}

pub fn normalize_handle(input: &str) -> Result<String> {
    let value = input.trim().trim_start_matches('@').to_ascii_lowercase();
    if value.is_empty() {
        bail!("handle must not be empty");
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        bail!("handle contains unsupported characters");
    }
    Ok(value)
}

pub fn runtime_plugin_id(runtime: &str) -> Result<String> {
    let runtime = runtime.trim();
    if runtime.is_empty() {
        bail!("runtime must not be empty");
    }
    let plugin = match runtime {
        "generic-cli" => "generic-cli",
        "claude-code" => "runtime.cli.claude-code",
        "codex" | "codex-cli" => "runtime.cli.codex",
        "gemini" | "gemini-cli" => "runtime.cli.gemini-cli",
        "hermes" => "runtime.hermes",
        "openclaw" => "runtime.openclaw",
        other => other,
    };
    Ok(plugin.to_string())
}

pub fn runtime_profile_id(runtime: &str, handle: &str) -> Result<String> {
    Ok(format!(
        "profile_{}_{}",
        stable_id_segment(runtime)?,
        stable_id_segment(handle)?
    ))
}

pub fn workspace_id(handle: &str) -> Result<String> {
    Ok(format!("workspace_{}", stable_id_segment(handle)?))
}

pub fn agent_data_paths(agent_did: &str) -> Result<(String, String)> {
    let segment = stable_id_segment(agent_did)?;
    Ok((
        format!("agents/{segment}/agent.db"),
        format!("agents/{segment}/messages.db"),
    ))
}

pub fn generate_agent_identity(
    config: &DaemonConfig,
    agent_kind: AgentKind,
    handle: &str,
) -> Result<GeneratedAgentIdentity> {
    let handle = normalize_handle(handle)?;
    let domain = config.did_domain.trim();
    if domain.is_empty() {
        bail!("did_domain must not be empty");
    }
    let endpoint_url = Some(format!(
        "{}/anp-im/rpc",
        config.service_base_url.trim_end_matches('/')
    ));
    let service = build_anp_message_service(
        "#message",
        endpoint_url.clone().unwrap_or_default(),
        AnpMessageServiceOptions::default()
            .with_service_did(format!("did:wba:{domain}"))
            .with_profiles([
                "anp.core.binding.v1",
                "anp.direct.base.v1",
                "anp.attachment.v1",
            ])
            .with_security_profiles(["transport-protected"]),
    );
    let options = DidDocumentOptions {
        path_segments: vec![
            "agent".to_string(),
            agent_kind.as_str().to_string(),
            stable_id_segment(&handle)?,
        ],
        domain: Some(domain.to_string()),
        challenge: Some(random_hex(16)),
        services: vec![service],
        did_profile: DidProfile::E1,
        ..DidDocumentOptions::default()
    };
    let bundle =
        create_did_wba_document(domain, options).map_err(|err| anyhow::anyhow!("{err}"))?;
    let did = bundle
        .did()
        .filter(|value| !value.is_empty())
        .context("generated DID document is missing id")?
        .to_string();
    let public_key = required_public_key(&bundle, VM_KEY_AUTH)?;
    let auth_private_key_pem = required_private_key(&bundle, VM_KEY_AUTH)?;
    let e2ee_signing_private_key_pem = bundle
        .private_key_pem(VM_KEY_E2EE_SIGNING)
        .unwrap_or_default()
        .to_string();
    let e2ee_agreement_private_key_pem = bundle
        .private_key_pem(VM_KEY_E2EE_AGREEMENT)
        .unwrap_or_default()
        .to_string();
    Ok(GeneratedAgentIdentity {
        did,
        did_document: bundle.did_document,
        endpoint_url,
        key_algorithm: "JsonWebKey2020".to_string(),
        public_key,
        auth_private_key_pem,
        e2ee_signing_private_key_pem,
        e2ee_agreement_private_key_pem,
    })
}

pub fn workspace_path(input: Option<&str>) -> Result<Option<PathBuf>> {
    let Some(raw) = input else {
        return Ok(None);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        bail!("workspace must not be empty when present");
    }
    if let Some(rest) = trimmed.strip_prefix("~/") {
        let home = std::env::var_os("HOME").context("workspace uses ~/ but HOME is not set")?;
        return Ok(Some(PathBuf::from(home).join(rest)));
    }
    Ok(Some(PathBuf::from(trimmed)))
}

fn stable_id_segment(input: &str) -> Result<String> {
    let normalized = input
        .trim()
        .trim_start_matches('@')
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string();
    if normalized.is_empty() {
        bail!("stable id segment source must not be empty");
    }
    Ok(normalized)
}

fn required_private_key(
    bundle: &anp::authentication::DidDocumentBundle,
    name: &str,
) -> Result<String> {
    bundle
        .private_key_pem(name)
        .map(ToString::to_string)
        .with_context(|| format!("generated DID document is missing private key {name}"))
}

fn required_public_key(
    bundle: &anp::authentication::DidDocumentBundle,
    name: &str,
) -> Result<String> {
    bundle
        .public_key_pem(name)
        .map(ToString::to_string)
        .with_context(|| format!("generated DID document is missing public key {name}"))
}

fn random_hex(num_bytes: usize) -> String {
    let mut buffer = vec![0_u8; num_bytes];
    rand::thread_rng().fill_bytes(&mut buffer);
    buffer.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handle_normalization_rejects_blank_or_shell_like_values() {
        assert_eq!(normalize_handle("@Alice-Coder").unwrap(), "alice-coder");
        assert!(normalize_handle("").is_err());
        assert!(normalize_handle("alice/coder").is_err());
    }
}
