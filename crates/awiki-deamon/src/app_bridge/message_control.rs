use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agent::AgentKind;
use crate::app_bridge::action::{
    is_app_action_result_payload, is_app_capabilities_payload, parse_app_action_result_payload,
    parse_app_capabilities_payload,
};
use crate::app_bridge::bootstrap::{
    is_daemon_bootstrap_payload, is_daemon_secure_bootstrap_payload,
    parse_secure_bootstrap_payload, process_secure_bootstrap_envelope, BootstrapProcessOutcome,
    DefaultBootstrapDidDocumentResolver,
};
use crate::app_bridge::personal_agent::{ensure_app_personal_agent, EnsureAppPersonalAgentOutcome};
use crate::registration::{AgentInventoryClient, AgentRegistrationClient};
use crate::state::DaemonState;
use crate::DaemonConfig;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncomingAppControlPayload {
    pub message_id: String,
    pub conversation_id: Option<String>,
    pub sender_did: String,
    pub target_agent_did: String,
    pub content_type: String,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[allow(
    clippy::large_enum_variant,
    reason = "control outcomes are short-lived and preserve a direct typed bootstrap result"
)]
pub enum AppControlOutcome {
    BootstrapReceived {
        bootstrap: BootstrapProcessOutcome,
        personal_agent: EnsureAppPersonalAgentOutcome,
    },
    CapabilitiesReceived {
        capabilities: Vec<String>,
    },
    ActionResultReceived {
        action_id: String,
        action: String,
        state: String,
    },
}

pub fn is_app_control_payload(payload: &Value) -> bool {
    is_daemon_secure_bootstrap_payload(payload)
        || is_daemon_bootstrap_payload(payload)
        || is_app_capabilities_payload(payload)
        || is_app_action_result_payload(payload)
}

pub fn handle_app_control_payload<C>(
    config: &DaemonConfig,
    state: &DaemonState,
    registration_client: &C,
    message: IncomingAppControlPayload,
) -> Result<AppControlOutcome>
where
    C: AgentRegistrationClient + AgentInventoryClient,
{
    validate_application_json_payload(&message)?;
    let daemon_agent = state
        .load_agent_definition(&message.target_agent_did)
        .context("load target daemon agent")?;
    if daemon_agent.agent_kind != AgentKind::Daemon {
        bail!("target agent is not a daemon agent");
    }
    crate::controller_scope::verify_daemon_controller_sender(
        config,
        state,
        registration_client,
        &daemon_agent,
        &message.sender_did,
    )?;
    let daemon_agent = state
        .load_agent_definition(&message.target_agent_did)
        .context("reload target daemon agent after controller sender verification")?;
    if message.sender_did != daemon_agent.controller_did {
        bail!("message sender is not the configured controller_did");
    }
    if is_daemon_secure_bootstrap_payload(&message.payload) {
        let envelope = parse_secure_bootstrap_payload(message.payload)?;
        let did_resolver = DefaultBootstrapDidDocumentResolver::new(config);
        let secure_outcome = process_secure_bootstrap_envelope(
            state,
            &daemon_agent.agent_did,
            &message.sender_did,
            &did_resolver,
            envelope,
        )?;
        let identity = state
            .load_user_delegated_identity(&secure_outcome.bootstrap.verification_method)?
            .context("load user delegated identity after bootstrap")?;
        let personal_agent = ensure_app_personal_agent(
            config,
            state,
            registration_client,
            &daemon_agent,
            &identity,
            &secure_outcome.desired_personal_agent,
            &secure_outcome.capability_policy,
        )?;
        return Ok(AppControlOutcome::BootstrapReceived {
            bootstrap: secure_outcome.bootstrap,
            personal_agent,
        });
    }
    if is_daemon_bootstrap_payload(&message.payload) {
        bail!("plain daemon bootstrap payload is not accepted in production; use awiki.daemon.bootstrap.secure.v1");
    }
    if is_app_capabilities_payload(&message.payload) {
        let envelope = parse_app_capabilities_payload(message.payload)?;
        state.insert_audit_event_json(
            "app.capabilities.received",
            Some(&message.target_agent_did),
            None,
            None,
            None,
            serde_json::json!({
                "sender_did": message.sender_did,
                "capabilities": envelope.capabilities,
                "require_confirmation_for_write_actions": envelope.require_confirmation_for_write_actions,
            }),
        )?;
        return Ok(AppControlOutcome::CapabilitiesReceived {
            capabilities: envelope.capabilities,
        });
    }
    if is_app_action_result_payload(&message.payload) {
        let envelope = parse_app_action_result_payload(message.payload)?;
        state.insert_audit_event_json(
            "app.action.result.received",
            Some(&message.target_agent_did),
            None,
            None,
            None,
            serde_json::json!({
                "sender_did": message.sender_did,
                "action_id": envelope.action_id,
                "action": envelope.action,
                "state": envelope.state,
                "error_code": envelope.error_code,
            }),
        )?;
        return Ok(AppControlOutcome::ActionResultReceived {
            action_id: envelope.action_id,
            action: envelope.action,
            state: envelope.state,
        });
    }
    bail!("unsupported app control payload schema")
}

fn validate_application_json_payload(message: &IncomingAppControlPayload) -> Result<()> {
    if message.content_type != "application/json" {
        bail!("app control payload must use application/json");
    }
    if !message.payload.is_object() {
        bail!("app control payload body.payload must be a JSON object");
    }
    if message.sender_did.trim().is_empty() {
        bail!("sender_did must not be empty");
    }
    if message.target_agent_did.trim().is_empty() {
        bail!("target_agent_did must not be empty");
    }
    Ok(())
}
