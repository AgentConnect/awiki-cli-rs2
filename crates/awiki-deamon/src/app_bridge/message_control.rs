use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agent::AgentKind;
use crate::app_bridge::bootstrap::{
    is_daemon_bootstrap_payload, parse_bootstrap_payload, process_bootstrap_envelope,
    BootstrapProcessOutcome,
};
use crate::state::DaemonState;

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
pub enum AppControlOutcome {
    BootstrapReceived(BootstrapProcessOutcome),
}

pub fn is_app_control_payload(payload: &Value) -> bool {
    is_daemon_bootstrap_payload(payload)
}

pub fn handle_app_control_payload(
    state: &DaemonState,
    message: IncomingAppControlPayload,
) -> Result<AppControlOutcome> {
    validate_application_json_payload(&message)?;
    let daemon_agent = state
        .load_agent_definition(&message.target_agent_did)
        .context("load target daemon agent")?;
    if daemon_agent.agent_kind != AgentKind::Daemon {
        bail!("target agent is not a daemon agent");
    }
    if message.sender_did != daemon_agent.controller_did {
        bail!("message sender is not the configured controller_did");
    }
    if is_daemon_bootstrap_payload(&message.payload) {
        let envelope = parse_bootstrap_payload(message.payload)?;
        if envelope.controller_did != daemon_agent.controller_did {
            bail!("bootstrap controller_did does not match daemon controller_did");
        }
        let outcome = process_bootstrap_envelope(
            state,
            &daemon_agent.agent_did,
            &message.sender_did,
            envelope,
        )?;
        return Ok(AppControlOutcome::BootstrapReceived(outcome));
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
