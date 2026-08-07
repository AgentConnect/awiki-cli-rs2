use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::agent::AgentDefinition;
use crate::app_bridge::action::{parse_app_capabilities_payload, APP_CAPABILITIES_SCHEMA};
use crate::commands::{
    create_runtime_agent_from_request, RuntimeAgentCreateOutcome, RuntimeAgentCreateRequest,
};
use crate::plugins::hermes::HERMES_RUNTIME_PLUGIN_ID;
use crate::registration::AgentRegistrationClient;
use crate::runtime::normalize_preferred_language;
use crate::state::{AppPersonalAgentBindingRecord, DaemonState, UserDelegatedIdentityRecord};
use crate::DaemonConfig;

pub const APP_MESSAGE_HANDLER_ROLE: &str = "app_message_handler";
pub const APP_PERSONAL_AGENT_RUNTIME_PROVIDER_HERMES: &str = "hermes";
pub const APP_PERSONAL_AGENT_RUNTIME_PROFILE: &str = "personal_agent";
pub const APP_PERSONAL_AGENT_STATUS_READY: &str = "personal_agent_ready";
pub const APP_PERSONAL_AGENT_STATUS_ACTIVE: &str = "personal_agent_active";
pub const APP_PERSONAL_AGENT_STATUS_ENSURING: &str = "personal_agent_ensuring";

const LEGACY_MESSAGE_AGENT_RUNTIME_PROFILE: &str = "message_agent";
const DEFAULT_PERSONAL_AGENT_DISPLAY_NAME: &str = "Hermes Personal Agent";
const LEGACY_MESSAGE_AGENT_DISPLAY_NAME: &str = "Hermes Message Agent";
const PERSONAL_AGENT_BINDING_PREFIX: &str = "app-personal-agent:";
const LEGACY_MESSAGE_AGENT_BINDING_PREFIX: &str = "app-message-agent:";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnsureAppPersonalAgentOutcome {
    pub binding: AppPersonalAgentBindingRecord,
    pub created_runtime_agent: bool,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesiredPersonalAgent {
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub runtime: Option<String>,
    #[serde(default)]
    pub runtime_provider: Option<String>,
    #[serde(default)]
    pub runtime_profile: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
    pub preferred_language: String,
    #[serde(default)]
    pub ensure_once_key: Option<String>,
    #[serde(default)]
    pub runtime_registration_token: Option<String>,
    #[serde(default)]
    pub auto_create: Option<bool>,
    #[serde(default)]
    pub allowed_actions: Vec<String>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

impl std::fmt::Debug for DesiredPersonalAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DesiredPersonalAgent")
            .field("role", &self.role)
            .field("runtime", &self.runtime)
            .field("runtime_provider", &self.runtime_provider)
            .field("runtime_profile", &self.runtime_profile)
            .field("display_name", &self.display_name)
            .field("preferred_language", &self.preferred_language)
            .field("ensure_once_key", &self.ensure_once_key)
            .field(
                "runtime_registration_token",
                &"<redacted-registration-token>",
            )
            .field("auto_create", &self.auto_create)
            .field("allowed_actions", &self.allowed_actions)
            .field("extra", &"<redacted-control-payload>")
            .finish()
    }
}

pub fn ensure_app_personal_agent<C>(
    config: &DaemonConfig,
    state: &DaemonState,
    registration_client: &C,
    daemon_agent: &AgentDefinition,
    identity: &UserDelegatedIdentityRecord,
    desired_personal_agent: &Value,
    capability_policy: &Value,
) -> Result<EnsureAppPersonalAgentOutcome>
where
    C: AgentRegistrationClient,
{
    identity.validate()?;
    let desired = parse_desired_personal_agent(desired_personal_agent)?;
    let role = desired
        .role
        .as_deref()
        .unwrap_or(APP_MESSAGE_HANDLER_ROLE)
        .trim();
    if role != APP_MESSAGE_HANDLER_ROLE {
        bail!("desired_personal_agent.role must be app_message_handler");
    }
    let runtime_provider = desired
        .runtime_provider
        .as_deref()
        .or(desired.runtime.as_deref())
        .unwrap_or(APP_PERSONAL_AGENT_RUNTIME_PROVIDER_HERMES)
        .trim();
    if runtime_provider != APP_PERSONAL_AGENT_RUNTIME_PROVIDER_HERMES {
        bail!("MVP app personal agent runtime_provider must be hermes");
    }
    let runtime = desired
        .runtime
        .as_deref()
        .unwrap_or(APP_PERSONAL_AGENT_RUNTIME_PROVIDER_HERMES)
        .trim();
    if runtime != APP_PERSONAL_AGENT_RUNTIME_PROVIDER_HERMES {
        bail!("MVP app personal agent runtime must be hermes");
    }
    let runtime_profile = desired
        .runtime_profile
        .as_deref()
        .unwrap_or(APP_PERSONAL_AGENT_RUNTIME_PROFILE)
        .trim();
    if runtime_profile != APP_PERSONAL_AGENT_RUNTIME_PROFILE
        && runtime_profile != LEGACY_MESSAGE_AGENT_RUNTIME_PROFILE
    {
        bail!("MVP app personal agent runtime_profile must be personal_agent");
    }
    let preferred_language = normalize_preferred_language(&desired.preferred_language)
        .with_context(|| {
            format!(
                "unsupported preferred_language: {}",
                desired.preferred_language
            )
        })?;
    let requested_binding_id = desired
        .ensure_once_key
        .clone()
        .unwrap_or_else(|| default_binding_id(&identity.user_did, &identity.app_instance_id));
    let binding_id = canonical_binding_id(&requested_binding_id);
    if let Some(existing) = state.load_active_app_personal_agent_binding(
        &identity.user_did,
        &identity.app_instance_id,
        APP_MESSAGE_HANDLER_ROLE,
    )? {
        if !binding_ids_are_compatible(&existing.binding_id, &binding_id) {
            bail!("active app personal agent binding conflicts with ensure_once_key");
        }
        let _profile = state
            .load_runtime_agent_profile(&existing.runtime_agent_did)
            .context("load existing app personal agent runtime profile")?;
        revoke_superseded_bindings(
            state,
            daemon_agent,
            &existing.user_did,
            &existing.binding_id,
            &existing.app_instance_id,
        )?;
        return Ok(EnsureAppPersonalAgentOutcome {
            binding: existing,
            created_runtime_agent: false,
        });
    }
    parse_app_capabilities_payload(capability_policy.clone())
        .context("validate app personal agent capability_policy")?;

    let token = desired
        .runtime_registration_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .context(
            "desired_personal_agent.runtime_registration_token is required for first create",
        )?;
    let outcome = create_runtime_agent_from_request(
        config,
        state,
        registration_client,
        daemon_agent,
        RuntimeAgentCreateRequest {
            command_id: format!("cmd_{binding_id}"),
            handle: Some(default_handle(
                &identity.user_did,
                &identity.app_instance_id,
            )),
            runtime: APP_PERSONAL_AGENT_RUNTIME_PROVIDER_HERMES.to_string(),
            display_name: Some(canonical_display_name(desired.display_name.as_deref())),
            driver_id: None,
            driver_config: None,
            recipient_policy: Some(personal_agent_recipient_policy(&identity.user_did)),
            workspace: None,
            workspace_mode: None,
            workspace_strategy: None,
            default_sandbox: None,
            default_model: None,
            preferred_language: Some(preferred_language),
            controller_did: daemon_agent.controller_did.clone(),
            registration_token: token.to_string(),
            client_request_id: Some(binding_id.clone()),
        },
    )?;
    validate_created_runtime(&outcome)?;
    let now = crate::security::runtime_token::current_time_millis()?;
    let binding = AppPersonalAgentBindingRecord {
        binding_id,
        user_did: identity.user_did.clone(),
        inbox_auth_verification_method: identity.verification_method.clone(),
        app_instance_id: identity.app_instance_id.clone(),
        bootstrap_id: identity.bootstrap_id.clone(),
        idempotency_key: identity.idempotency_key.clone(),
        daemon_agent_did: daemon_agent.agent_did.clone(),
        runtime_agent_did: outcome.agent_did,
        runtime_profile_id: outcome.runtime_profile_id,
        role: APP_MESSAGE_HANDLER_ROLE.to_string(),
        desired_agent_json: sanitized_desired_agent_json(desired_personal_agent),
        capability_policy_json: sanitized_capability_policy_json(capability_policy),
        status: APP_PERSONAL_AGENT_STATUS_READY.to_string(),
        created_at_ms: now,
        updated_at_ms: now,
        revoked_at_ms: None,
    };
    state.upsert_app_personal_agent_binding(&binding)?;
    revoke_superseded_bindings(
        state,
        daemon_agent,
        &binding.user_did,
        &binding.binding_id,
        &binding.app_instance_id,
    )?;
    state.insert_audit_event_json(
        "app_personal_agent.binding.ready",
        Some(&daemon_agent.agent_did),
        Some(&binding.runtime_profile_id),
        None,
        None,
        json!({
            "binding_id": binding.binding_id,
            "user_did": binding.user_did,
            "app_instance_id": binding.app_instance_id,
            "runtime_agent_did": binding.runtime_agent_did,
            "role": binding.role,
            "status": binding.status,
        }),
    )?;
    Ok(EnsureAppPersonalAgentOutcome {
        binding,
        created_runtime_agent: true,
    })
}

fn revoke_superseded_bindings(
    state: &DaemonState,
    daemon_agent: &AgentDefinition,
    user_did: &str,
    keep_binding_id: &str,
    app_instance_id: &str,
) -> Result<()> {
    let revoked = state.revoke_other_active_app_personal_agent_bindings(
        user_did,
        APP_MESSAGE_HANDLER_ROLE,
        keep_binding_id,
    )?;
    if revoked > 0 {
        state.insert_audit_event_json(
            "app_personal_agent.binding.superseded",
            Some(&daemon_agent.agent_did),
            None,
            None,
            None,
            json!({
                "user_did": user_did,
                "role": APP_MESSAGE_HANDLER_ROLE,
                "active_binding_id": keep_binding_id,
                "active_app_instance_id": app_instance_id,
                "revoked_count": revoked,
            }),
        )?;
    }
    Ok(())
}

fn parse_desired_personal_agent(value: &Value) -> Result<DesiredPersonalAgent> {
    if !value.is_object() {
        bail!("desired_personal_agent must be a JSON object");
    }
    let desired: DesiredPersonalAgent =
        serde_json::from_value(value.clone()).context("parse desired_personal_agent")?;
    if desired.preferred_language.trim().is_empty() {
        bail!("desired_personal_agent.preferred_language is required");
    }
    Ok(desired)
}

fn validate_created_runtime(outcome: &RuntimeAgentCreateOutcome) -> Result<()> {
    if outcome.runtime_plugin_id != HERMES_RUNTIME_PLUGIN_ID {
        bail!("created app personal agent must use Hermes runtime");
    }
    Ok(())
}

fn sanitized_desired_agent_json(value: &Value) -> Value {
    let mut sanitized = value.clone();
    if let Some(object) = sanitized.as_object_mut() {
        object.remove("runtime_registration_token");
        object.remove("registration_token");
        object.remove("token");
        if object.get("runtime_profile").and_then(Value::as_str)
            == Some(LEGACY_MESSAGE_AGENT_RUNTIME_PROFILE)
        {
            object.insert(
                "runtime_profile".to_string(),
                Value::String(APP_PERSONAL_AGENT_RUNTIME_PROFILE.to_string()),
            );
        }
        if object.get("display_name").and_then(Value::as_str)
            == Some(LEGACY_MESSAGE_AGENT_DISPLAY_NAME)
        {
            object.insert(
                "display_name".to_string(),
                Value::String(DEFAULT_PERSONAL_AGENT_DISPLAY_NAME.to_string()),
            );
        }
        let canonical_ensure_once_key = object
            .get("ensure_once_key")
            .and_then(Value::as_str)
            .and_then(|value| value.strip_prefix(LEGACY_MESSAGE_AGENT_BINDING_PREFIX))
            .map(|suffix| format!("{PERSONAL_AGENT_BINDING_PREFIX}{suffix}"));
        if let Some(canonical_ensure_once_key) = canonical_ensure_once_key {
            object.insert(
                "ensure_once_key".to_string(),
                Value::String(canonical_ensure_once_key),
            );
        }
    }
    sanitized
}

fn sanitized_capability_policy_json(value: &Value) -> Value {
    if value.get("schema").and_then(Value::as_str) == Some(APP_CAPABILITIES_SCHEMA)
        && value.is_object()
    {
        value.clone()
    } else {
        json!({
            "schema": APP_CAPABILITIES_SCHEMA,
            "capabilities": [],
            "require_confirmation_for_write_actions": true
        })
    }
}

fn personal_agent_recipient_policy(user_did: &str) -> Value {
    json!({
        "allowed_dids": [user_did],
        "allowed_security": ["default_plain"]
    })
}

fn default_binding_id(user_did: &str, app_instance_id: &str) -> String {
    format!("{PERSONAL_AGENT_BINDING_PREFIX}{user_did}:{app_instance_id}")
}

fn canonical_binding_id(binding_id: &str) -> String {
    if let Some(suffix) = binding_id.strip_prefix(LEGACY_MESSAGE_AGENT_BINDING_PREFIX) {
        format!("{PERSONAL_AGENT_BINDING_PREFIX}{suffix}")
    } else {
        binding_id.to_string()
    }
}

fn binding_ids_are_compatible(existing_binding_id: &str, requested_binding_id: &str) -> bool {
    existing_binding_id == requested_binding_id
        || canonical_binding_id(existing_binding_id) == requested_binding_id
}

fn canonical_display_name(display_name: Option<&str>) -> String {
    match display_name {
        Some(LEGACY_MESSAGE_AGENT_DISPLAY_NAME) | None => {
            DEFAULT_PERSONAL_AGENT_DISPLAY_NAME.to_string()
        }
        Some(display_name) => display_name.to_string(),
    }
}

fn default_handle(user_did: &str, app_instance_id: &str) -> String {
    const PREFIX: &str = "hermes-personal";
    const MAX_HANDLE_LENGTH: usize = 48;
    let seed = format!("{}|{}", user_did.trim(), app_instance_id.trim());
    let digest = Sha256::digest(seed.as_bytes());
    let hash = digest[..6]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let app_part = safe_handle_component(app_instance_id);
    let max_app_len = MAX_HANDLE_LENGTH - PREFIX.len() - hash.len() - 2;
    let app_tail = if app_part.chars().count() > max_app_len {
        app_part
            .chars()
            .rev()
            .take(max_app_len)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<String>()
    } else {
        app_part
    };
    let app_tail = app_tail.trim_matches('-').to_string();
    let app_tail = if app_tail.is_empty() {
        "agent".to_string()
    } else {
        app_tail
    };
    let handle = format!("{PREFIX}-{app_tail}-{hash}");
    if handle.chars().count() > MAX_HANDLE_LENGTH {
        handle.chars().take(MAX_HANDLE_LENGTH).collect::<String>()
    } else {
        handle
    }
}

#[cfg(any(test, feature = "system-test-probe"))]
pub fn personal_agent_runtime_handle_for_system_test(
    user_did: &str,
    app_instance_id: &str,
) -> String {
    default_handle(user_did, app_instance_id)
}

fn safe_handle_component(value: &str) -> String {
    let mut normalized = value
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    while normalized.contains("--") {
        normalized = normalized.replace("--", "-");
    }
    let normalized = normalized.trim_matches('-').to_string();
    if normalized.is_empty() {
        "agent".to_string()
    } else {
        normalized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desired_personal_agent_debug_redacts_runtime_registration_token() {
        let desired = parse_desired_personal_agent(&json!({
            "role": "app_message_handler",
            "runtime": "hermes",
            "runtime_provider": "hermes",
            "runtime_profile": "personal_agent",
            "preferred_language": "en",
            "runtime_registration_token": "tok_runtime_secret"
        }))
        .unwrap();
        let debug = format!("{desired:?}");
        assert!(!debug.contains("tok_runtime_secret"));
        assert!(debug.contains("<redacted-registration-token>"));
        assert!(debug.contains("runtime_provider"));
        assert!(debug.contains("runtime_profile"));
    }

    #[test]
    fn desired_personal_agent_requires_object_and_preferred_language() {
        assert!(parse_desired_personal_agent(&Value::Null).is_err());
        assert!(parse_desired_personal_agent(&json!({
            "role": "app_message_handler",
            "runtime": "hermes"
        }))
        .is_err());

        let desired = parse_desired_personal_agent(&json!({
            "role": "app_message_handler",
            "runtime": "hermes",
            "runtime_provider": "hermes",
            "runtime_profile": "personal_agent",
            "preferred_language": "zh-Hans"
        }))
        .unwrap();
        assert_eq!(
            desired.runtime_provider.as_deref(),
            Some(APP_PERSONAL_AGENT_RUNTIME_PROVIDER_HERMES)
        );
        assert_eq!(
            desired.runtime_profile.as_deref(),
            Some(APP_PERSONAL_AGENT_RUNTIME_PROFILE)
        );
        assert_eq!(desired.preferred_language, "zh-Hans");
    }

    #[test]
    fn legacy_runtime_profile_and_binding_id_decode_to_canonical_values() {
        let desired = parse_desired_personal_agent(&json!({
            "role": "app_message_handler",
            "runtime": "hermes",
            "runtime_provider": "hermes",
            "runtime_profile": LEGACY_MESSAGE_AGENT_RUNTIME_PROFILE,
            "preferred_language": "en"
        }))
        .unwrap();
        assert_eq!(
            desired.runtime_profile.as_deref(),
            Some(LEGACY_MESSAGE_AGENT_RUNTIME_PROFILE)
        );

        let legacy = format!(
            "{LEGACY_MESSAGE_AGENT_BINDING_PREFIX}{}:{}",
            "did:human:alice", "app_1"
        );
        let canonical = default_binding_id("did:human:alice", "app_1");
        assert_eq!(canonical_binding_id(&legacy), canonical);
        assert!(binding_ids_are_compatible(&legacy, &canonical));
        assert_eq!(
            canonical_display_name(Some(LEGACY_MESSAGE_AGENT_DISPLAY_NAME)),
            DEFAULT_PERSONAL_AGENT_DISPLAY_NAME
        );
    }

    #[test]
    fn sanitized_desired_agent_removes_registration_token() {
        let sanitized = sanitized_desired_agent_json(&json!({
            "role": "app_message_handler",
            "runtime_profile": LEGACY_MESSAGE_AGENT_RUNTIME_PROFILE,
            "display_name": LEGACY_MESSAGE_AGENT_DISPLAY_NAME,
            "ensure_once_key": "app-message-agent:did:human:alice:app_1",
            "runtime_registration_token": "tok_runtime_secret",
            "token": "tok_other"
        }));
        assert!(!sanitized.to_string().contains("tok_runtime_secret"));
        assert!(sanitized.get("runtime_registration_token").is_none());
        assert!(sanitized.get("token").is_none());
        assert_eq!(
            sanitized["runtime_profile"],
            APP_PERSONAL_AGENT_RUNTIME_PROFILE
        );
        assert_eq!(
            sanitized["display_name"],
            DEFAULT_PERSONAL_AGENT_DISPLAY_NAME
        );
        assert_eq!(
            sanitized["ensure_once_key"],
            "app-personal-agent:did:human:alice:app_1"
        );
    }

    #[test]
    fn default_handle_keeps_app_instance_entropy_under_handle_limit() {
        assert_eq!(
            default_handle("did:human:me", "app_1"),
            "hermes-personal-app-1-334c10a06052"
        );
        assert_eq!(
            personal_agent_runtime_handle_for_system_test("did:human:me", "app_1"),
            default_handle("did:human:me", "app_1")
        );

        let first = default_handle(
            "did:wba:awiki.info:e2e-agent-app:e1_5evGr1VeOQe1AJVtBkrMvLYOPaVN-PtjDtpOxPvyqaw",
            "macos-e2e-app-20260614T012433250Z",
        );
        let second = default_handle(
            "did:wba:awiki.info:e2e-agent-app:e1_5evGr1VeOQe1AJVtBkrMvLYOPaVN-PtjDtpOxPvyqaw",
            "macos-e2e-app-20260614T011342011Z",
        );
        assert_ne!(first, second);
        assert!(first.len() <= 48);
        assert!(second.len() <= 48);
        assert!(!first.contains("--"));
        assert!(!second.contains("--"));
    }
}
