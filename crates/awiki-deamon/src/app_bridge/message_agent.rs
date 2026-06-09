use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::agent::AgentDefinition;
use crate::commands::{
    create_runtime_agent_from_request, RuntimeAgentCreateOutcome, RuntimeAgentCreateRequest,
};
use crate::plugins::hermes::HERMES_RUNTIME_PLUGIN_ID;
use crate::registration::AgentRegistrationClient;
use crate::state::{AppMessageAgentBindingRecord, DaemonState, UserDelegatedIdentityRecord};
use crate::DaemonConfig;

pub const APP_MESSAGE_HANDLER_ROLE: &str = "app_message_handler";
pub const APP_MESSAGE_AGENT_STATUS_READY: &str = "message_agent_ready";
pub const APP_MESSAGE_AGENT_STATUS_ACTIVE: &str = "message_agent_active";
pub const APP_MESSAGE_AGENT_STATUS_ENSURING: &str = "message_agent_ensuring";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnsureAppMessageAgentOutcome {
    pub binding: AppMessageAgentBindingRecord,
    pub created_runtime_agent: bool,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesiredMessageAgent {
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub runtime: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
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

impl std::fmt::Debug for DesiredMessageAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DesiredMessageAgent")
            .field("role", &self.role)
            .field("runtime", &self.runtime)
            .field("display_name", &self.display_name)
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

pub fn ensure_app_message_agent<C>(
    config: &DaemonConfig,
    state: &DaemonState,
    registration_client: &C,
    daemon_agent: &AgentDefinition,
    identity: &UserDelegatedIdentityRecord,
    desired_message_agent: &Value,
    capability_policy: &Value,
) -> Result<EnsureAppMessageAgentOutcome>
where
    C: AgentRegistrationClient,
{
    identity.validate()?;
    let desired = parse_desired_message_agent(desired_message_agent)?;
    let role = desired
        .role
        .as_deref()
        .unwrap_or(APP_MESSAGE_HANDLER_ROLE)
        .trim();
    if role != APP_MESSAGE_HANDLER_ROLE {
        bail!("desired_message_agent.role must be app_message_handler");
    }
    let runtime = desired.runtime.as_deref().unwrap_or("hermes").trim();
    if runtime != "hermes" {
        bail!("MVP app message agent runtime must be hermes");
    }
    let binding_id = desired
        .ensure_once_key
        .clone()
        .unwrap_or_else(|| default_binding_id(&identity.user_did, &identity.app_instance_id));
    if let Some(existing) = state.load_active_app_message_agent_binding(
        &identity.user_did,
        &identity.app_instance_id,
        APP_MESSAGE_HANDLER_ROLE,
    )? {
        if existing.binding_id != binding_id {
            bail!("active app message agent binding conflicts with ensure_once_key");
        }
        let _profile = state
            .load_runtime_agent_profile(&existing.runtime_agent_did)
            .context("load existing app message runtime profile")?;
        return Ok(EnsureAppMessageAgentOutcome {
            binding: existing,
            created_runtime_agent: false,
        });
    }

    let token = desired
        .runtime_registration_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .context("desired_message_agent.runtime_registration_token is required for first create")?;
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
            runtime: "hermes".to_string(),
            display_name: Some(
                desired
                    .display_name
                    .clone()
                    .unwrap_or_else(|| "Hermes Message Agent".to_string()),
            ),
            driver_id: None,
            driver_config: None,
            recipient_policy: Some(message_agent_recipient_policy(&identity.user_did)),
            workspace: None,
            controller_did: daemon_agent.controller_did.clone(),
            registration_token: token.to_string(),
            client_request_id: Some(binding_id.clone()),
        },
    )?;
    validate_created_runtime(&outcome)?;
    let now = crate::security::runtime_token::current_time_millis()?;
    let binding = AppMessageAgentBindingRecord {
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
        desired_agent_json: sanitized_desired_agent_json(desired_message_agent),
        capability_policy_json: sanitized_capability_policy_json(capability_policy),
        status: APP_MESSAGE_AGENT_STATUS_READY.to_string(),
        created_at_ms: now,
        updated_at_ms: now,
        revoked_at_ms: None,
    };
    state.upsert_app_message_agent_binding(&binding)?;
    state.insert_audit_event_json(
        "app_message_agent.binding.ready",
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
    Ok(EnsureAppMessageAgentOutcome {
        binding,
        created_runtime_agent: true,
    })
}

fn parse_desired_message_agent(value: &Value) -> Result<DesiredMessageAgent> {
    if value.is_null() {
        return Ok(DesiredMessageAgent {
            role: Some(APP_MESSAGE_HANDLER_ROLE.to_string()),
            runtime: Some("hermes".to_string()),
            display_name: Some("Hermes Message Agent".to_string()),
            ensure_once_key: None,
            runtime_registration_token: None,
            auto_create: Some(true),
            allowed_actions: Vec::new(),
            extra: serde_json::Map::new(),
        });
    }
    if !value.is_object() {
        bail!("desired_message_agent must be a JSON object");
    }
    serde_json::from_value(value.clone()).context("parse desired_message_agent")
}

fn validate_created_runtime(outcome: &RuntimeAgentCreateOutcome) -> Result<()> {
    if outcome.runtime_plugin_id != HERMES_RUNTIME_PLUGIN_ID {
        bail!("created app message agent must use Hermes runtime");
    }
    Ok(())
}

fn sanitized_desired_agent_json(value: &Value) -> Value {
    let mut sanitized = value.clone();
    if let Some(object) = sanitized.as_object_mut() {
        object.remove("runtime_registration_token");
        object.remove("registration_token");
        object.remove("token");
    }
    sanitized
}

fn sanitized_capability_policy_json(value: &Value) -> Value {
    if value.is_object() {
        value.clone()
    } else {
        json!({})
    }
}

fn message_agent_recipient_policy(user_did: &str) -> Value {
    json!({
        "allowed_dids": [user_did],
        "allowed_security": ["default_plain"]
    })
}

fn default_binding_id(user_did: &str, app_instance_id: &str) -> String {
    format!("app-message-agent:{user_did}:{app_instance_id}")
}

fn default_handle(user_did: &str, app_instance_id: &str) -> String {
    let mut source = format!("hermes-msg-{user_did}-{app_instance_id}")
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    while source.contains("--") {
        source = source.replace("--", "-");
    }
    let source = source.trim_matches('-');
    if source.is_empty() {
        "hermes-msg-agent".to_string()
    } else {
        let shortened = source.chars().take(48).collect::<String>();
        let shortened = shortened.trim_matches('-');
        if shortened.is_empty() {
            "hermes-msg-agent".to_string()
        } else {
            shortened.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desired_message_agent_debug_redacts_runtime_registration_token() {
        let desired = parse_desired_message_agent(&json!({
            "role": "app_message_handler",
            "runtime": "hermes",
            "runtime_registration_token": "tok_runtime_secret"
        }))
        .unwrap();
        let debug = format!("{desired:?}");
        assert!(!debug.contains("tok_runtime_secret"));
        assert!(debug.contains("<redacted-registration-token>"));
    }

    #[test]
    fn sanitized_desired_agent_removes_registration_token() {
        let sanitized = sanitized_desired_agent_json(&json!({
            "role": "app_message_handler",
            "runtime_registration_token": "tok_runtime_secret",
            "token": "tok_other"
        }));
        assert!(!sanitized.to_string().contains("tok_runtime_secret"));
        assert!(sanitized.get("runtime_registration_token").is_none());
        assert!(sanitized.get("token").is_none());
    }
}
