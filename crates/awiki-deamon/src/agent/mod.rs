use std::path::PathBuf;

use anp::authentication::{
    build_anp_message_service, create_did_wba_document, AnpMessageServiceOptions,
    DidDocumentOptions, DidProfile, VM_KEY_AUTH, VM_KEY_E2EE_AGREEMENT, VM_KEY_E2EE_SIGNING,
};
use anyhow::{bail, Context, Result};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::config::DaemonConfig;

pub const GENERIC_CLI_RUNTIME_PLUGIN_ID: &str = "generic-cli";
pub const CODEX_CLI_DRIVER_ID: &str = "codex";
pub const CLAUDE_CODE_CLI_DRIVER_ID: &str = "claude-code";
pub const GEMINI_CLI_DRIVER_ID: &str = "gemini";

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
    pub controller_user_id: String,
    pub controller_full_handle: String,
    pub controller_scope_key: String,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeResolution {
    pub runtime_plugin_id: String,
    pub driver_id: Option<String>,
    pub legacy_runtime_plugin_id: Option<String>,
    pub defaulted_driver_id: bool,
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
        if self.controller_user_id.trim().is_empty() {
            bail!("controller_user_id must not be empty");
        }
        if self.controller_full_handle.trim().is_empty() {
            bail!("controller_full_handle must not be empty");
        }
        if self.controller_scope_key.trim().is_empty() {
            bail!("controller_scope_key must not be empty");
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

pub fn generate_product_handle(prefix: &str) -> Result<String> {
    let prefix = prefix.trim();
    if prefix.is_empty() {
        bail!("handle prefix must not be empty");
    }
    if !prefix
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
    {
        bail!("handle prefix contains unsupported characters");
    }
    const ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";
    let mut random = [0_u8; 16];
    rand::thread_rng().fill_bytes(&mut random);
    let suffix = random
        .iter()
        .map(|byte| ALPHABET[usize::from(*byte) % ALPHABET.len()] as char)
        .collect::<String>();
    normalize_handle(&format!("{prefix}{suffix}"))
}

pub fn resolve_runtime(
    runtime: &str,
    driver_id_override: Option<&str>,
) -> Result<RuntimeResolution> {
    let runtime = runtime.trim();
    if runtime.is_empty() {
        bail!("runtime must not be empty");
    }

    let key = runtime.to_ascii_lowercase();
    match key.as_str() {
        GENERIC_CLI_RUNTIME_PLUGIN_ID => {
            let (driver_id, defaulted_driver_id) = match driver_id_override {
                Some(driver_id) => (normalize_cli_driver_id(driver_id)?, false),
                None => (CODEX_CLI_DRIVER_ID.to_string(), true),
            };
            Ok(RuntimeResolution {
                runtime_plugin_id: GENERIC_CLI_RUNTIME_PLUGIN_ID.to_string(),
                driver_id: Some(driver_id),
                legacy_runtime_plugin_id: None,
                defaulted_driver_id,
            })
        }
        "codex" | "codex-cli" => {
            cli_alias_resolution(driver_id_override, CODEX_CLI_DRIVER_ID, "runtime.cli.codex")
        }
        "claude-code" => cli_alias_resolution(
            driver_id_override,
            CLAUDE_CODE_CLI_DRIVER_ID,
            "runtime.cli.claude-code",
        ),
        "gemini" | "gemini-cli" => cli_alias_resolution(
            driver_id_override,
            GEMINI_CLI_DRIVER_ID,
            "runtime.cli.gemini-cli",
        ),
        "hermes" => native_runtime_resolution("runtime.hermes", driver_id_override),
        "openclaw" => native_runtime_resolution("runtime.openclaw", driver_id_override),
        _ => native_runtime_resolution(runtime, driver_id_override),
    }
}

// Legacy helper kept for compatibility until all create/read paths move to
// RuntimeResolution. New generic CLI writes must use resolve_runtime instead.
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
                "anp.group.base.v1",
                "anp.attachment.v1",
            ])
            .with_security_profiles(["transport-protected"]),
    );
    let handle_service = json!({
        "id": "#handle",
        "type": anp::wns::ANP_HANDLE_SERVICE_TYPE,
        "serviceEndpoint": anp::wns::build_resolution_url(&handle, domain),
    });
    let options = DidDocumentOptions {
        path_segments: vec![
            "agent".to_string(),
            agent_kind.as_str().to_string(),
            stable_id_segment(&handle)?,
        ],
        domain: Some(domain.to_string()),
        challenge: Some(random_hex(16)),
        services: vec![service, handle_service],
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

fn cli_alias_resolution(
    driver_id_override: Option<&str>,
    canonical_driver_id: &str,
    legacy_runtime_plugin_id: &str,
) -> Result<RuntimeResolution> {
    if let Some(driver_id_override) = driver_id_override {
        let driver_id = normalize_cli_driver_id(driver_id_override)?;
        if driver_id != canonical_driver_id {
            bail!("runtime alias requires driver_id {canonical_driver_id}, got {driver_id}");
        }
    }
    Ok(RuntimeResolution {
        runtime_plugin_id: GENERIC_CLI_RUNTIME_PLUGIN_ID.to_string(),
        driver_id: Some(canonical_driver_id.to_string()),
        legacy_runtime_plugin_id: Some(legacy_runtime_plugin_id.to_string()),
        defaulted_driver_id: false,
    })
}

fn native_runtime_resolution(
    runtime_plugin_id: &str,
    driver_id_override: Option<&str>,
) -> Result<RuntimeResolution> {
    if driver_id_override.is_some() {
        bail!("driver_id is only supported for generic-cli runtimes");
    }
    Ok(RuntimeResolution {
        runtime_plugin_id: runtime_plugin_id.to_string(),
        driver_id: None,
        legacy_runtime_plugin_id: None,
        defaulted_driver_id: false,
    })
}

fn normalize_cli_driver_id(input: &str) -> Result<String> {
    let value = input.trim().to_ascii_lowercase();
    if value.is_empty() {
        bail!("driver_id must not be empty");
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        bail!("driver_id contains unsupported characters");
    }
    let normalized = match value.as_str() {
        CODEX_CLI_DRIVER_ID | CLAUDE_CODE_CLI_DRIVER_ID | GEMINI_CLI_DRIVER_ID => value,
        "codex-cli" => CODEX_CLI_DRIVER_ID.to_string(),
        "gemini-cli" => GEMINI_CLI_DRIVER_ID.to_string(),
        other => bail!("unsupported generic-cli driver_id: {other}"),
    };
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
    fn generated_agent_identity_declares_handle_provider_in_signed_did_document() {
        let root = tempfile::tempdir().unwrap();
        let mut config = DaemonConfig::for_state_root(root.path()).unwrap();
        config.service_base_url = "https://awiki.info".to_string();
        config.did_domain = "awiki.info".to_string();

        for kind in [AgentKind::Daemon, AgentKind::Runtime] {
            let generated = generate_agent_identity(&config, kind, "Codex-1").unwrap();
            let services = generated.did_document["service"].as_array().unwrap();
            let handle_service = services
                .iter()
                .find(|service| service["type"] == anp::wns::ANP_HANDLE_SERVICE_TYPE)
                .expect("generated agent DID document should declare its Handle Provider");

            assert_eq!(handle_service["id"], format!("{}#handle", generated.did));
            assert_eq!(
                handle_service["serviceEndpoint"],
                "https://awiki.info/.well-known/handle/codex-1"
            );
            assert!(anp::authentication::validate_did_document_binding(
                &generated.did_document,
                true,
            ));
        }
    }

    #[test]
    fn handle_normalization_rejects_blank_or_shell_like_values() {
        assert_eq!(normalize_handle("@Alice-Coder").unwrap(), "alice-coder");
        assert!(normalize_handle("").is_err());
        assert!(normalize_handle("alice/coder").is_err());
    }

    #[test]
    fn resolve_runtime_maps_cli_family_aliases_to_generic_cli_driver_ids() {
        for (runtime, driver_id, legacy_plugin_id) in [
            ("codex", CODEX_CLI_DRIVER_ID, "runtime.cli.codex"),
            ("codex-cli", CODEX_CLI_DRIVER_ID, "runtime.cli.codex"),
            (
                "claude-code",
                CLAUDE_CODE_CLI_DRIVER_ID,
                "runtime.cli.claude-code",
            ),
            ("gemini", GEMINI_CLI_DRIVER_ID, "runtime.cli.gemini-cli"),
            ("gemini-cli", GEMINI_CLI_DRIVER_ID, "runtime.cli.gemini-cli"),
        ] {
            let resolution = resolve_runtime(runtime, None).unwrap();

            assert_eq!(resolution.runtime_plugin_id, GENERIC_CLI_RUNTIME_PLUGIN_ID);
            assert_eq!(resolution.driver_id.as_deref(), Some(driver_id));
            assert_eq!(
                resolution.legacy_runtime_plugin_id.as_deref(),
                Some(legacy_plugin_id)
            );
            assert!(!resolution.defaulted_driver_id);
        }
    }

    #[test]
    fn resolve_runtime_supports_generic_cli_default_and_explicit_driver() {
        let defaulted = resolve_runtime(" generic-cli ", None).unwrap();
        assert_eq!(defaulted.runtime_plugin_id, GENERIC_CLI_RUNTIME_PLUGIN_ID);
        assert_eq!(defaulted.driver_id.as_deref(), Some(CODEX_CLI_DRIVER_ID));
        assert_eq!(defaulted.legacy_runtime_plugin_id, None);
        assert!(defaulted.defaulted_driver_id);

        let explicit = resolve_runtime("generic-cli", Some(" Gemini-CLI ")).unwrap();
        assert_eq!(explicit.runtime_plugin_id, GENERIC_CLI_RUNTIME_PLUGIN_ID);
        assert_eq!(explicit.driver_id.as_deref(), Some(GEMINI_CLI_DRIVER_ID));
        assert_eq!(explicit.legacy_runtime_plugin_id, None);
        assert!(!explicit.defaulted_driver_id);
    }

    #[test]
    fn resolve_runtime_keeps_native_runtime_types_without_driver_ids() {
        for (runtime, plugin_id) in [
            ("hermes", "runtime.hermes"),
            ("openclaw", "runtime.openclaw"),
            ("runtime.custom", "runtime.custom"),
        ] {
            let resolution = resolve_runtime(runtime, None).unwrap();

            assert_eq!(resolution.runtime_plugin_id, plugin_id);
            assert_eq!(resolution.driver_id, None);
            assert_eq!(resolution.legacy_runtime_plugin_id, None);
            assert!(!resolution.defaulted_driver_id);
        }
    }

    #[test]
    fn resolve_runtime_rejects_empty_or_conflicting_driver_contracts() {
        assert!(resolve_runtime("", None).is_err());
        assert!(resolve_runtime("generic-cli", Some(" ")).is_err());
        assert!(resolve_runtime("codex", Some("gemini")).is_err());
        assert!(resolve_runtime("hermes", Some("codex")).is_err());

        let alias_with_matching_driver = resolve_runtime("codex", Some("codex-cli")).unwrap();
        assert_eq!(
            alias_with_matching_driver.runtime_plugin_id,
            GENERIC_CLI_RUNTIME_PLUGIN_ID
        );
        assert_eq!(
            alias_with_matching_driver.driver_id.as_deref(),
            Some(CODEX_CLI_DRIVER_ID)
        );
    }
}
