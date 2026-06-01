use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::agent::{
    agent_data_paths, generate_agent_identity, normalize_handle, resolve_runtime,
    runtime_plugin_id, runtime_profile_id, workspace_id, workspace_path, AgentDefinition,
    AgentKind,
};
use crate::outbox::{AgentManagementOutbox, AgentStatusResponse};
use crate::plugins::hermes::{
    initialize_hermes_profile, mark_hermes_profile_failed, HERMES_RUNTIME_PLUGIN_ID,
};
use crate::registration::{
    AgentRegistrationClient, AgentRegistrationExchangeRequest, AgentRegistrationExchangeResult,
    RegistrationToken,
};
use crate::runtime::RuntimeAgentProfile;
use crate::state::DaemonState;
use crate::workspace::WorkspaceMode;
use crate::DaemonConfig;

const AGENT_COMMAND_SCHEMA: &str = "awiki.agent.command.v1";
const AGENT_STATUS_SCHEMA: &str = "awiki.agent.status.v1";
const RUNTIME_AGENT_CREATE: &str = "runtime.agent.create";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncomingAgentPayloadMessage {
    pub message_id: String,
    pub conversation_id: Option<String>,
    pub sender_did: String,
    pub target_agent_did: String,
    pub content_type: String,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeAgentCreateOutcome {
    pub command_id: String,
    pub agent_did: String,
    pub handle: String,
    pub runtime_profile_id: String,
    pub runtime_plugin_id: String,
    pub workspace_id: Option<String>,
    pub registration_token_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentCommandOutcome {
    RuntimeAgentCreated(RuntimeAgentCreateOutcome),
}

#[derive(Deserialize)]
struct RuntimeAgentCreatePayload {
    schema: String,
    command_id: String,
    command: String,
    #[serde(default)]
    target_agent_kind: Option<String>,
    args: RuntimeAgentCreateArgs,
    #[serde(default)]
    reply_policy: Option<ReplyPolicy>,
}

#[derive(Deserialize)]
struct RuntimeAgentCreateArgs {
    handle: String,
    runtime: String,
    #[serde(default)]
    driver_id: Option<String>,
    #[serde(default)]
    driver_config: Option<Value>,
    #[serde(default)]
    recipient_policy: Option<Value>,
    #[serde(default)]
    workspace: Option<String>,
    controller_did: String,
    registration_token: String,
}

#[derive(Deserialize)]
struct ReplyPolicy {
    #[serde(default = "default_true")]
    progress: bool,
    #[serde(default = "default_true", alias = "final")]
    final_report: bool,
}

pub fn handle_agent_payload_message<C, O>(
    config: &DaemonConfig,
    state: &DaemonState,
    registration_client: &C,
    outbox: &O,
    message: IncomingAgentPayloadMessage,
) -> Result<AgentCommandOutcome>
where
    C: AgentRegistrationClient,
    O: AgentManagementOutbox,
{
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

    let payload: RuntimeAgentCreatePayload =
        serde_json::from_value(message.payload.clone()).context("parse agent command payload")?;
    if payload.schema != AGENT_COMMAND_SCHEMA {
        bail!("unsupported agent command schema: {}", payload.schema);
    }
    if payload.command != RUNTIME_AGENT_CREATE {
        bail!("unsupported agent command: {}", payload.command);
    }
    if payload
        .target_agent_kind
        .as_deref()
        .is_some_and(|kind| kind != AgentKind::Runtime.as_str())
    {
        bail!("runtime.agent.create target_agent_kind must be runtime");
    }

    let outcome =
        match create_runtime_agent(config, state, registration_client, &daemon_agent, &payload) {
            Ok(outcome) => outcome,
            Err(error) => {
                send_command_status(
                    outbox,
                    &daemon_agent,
                    &message,
                    &payload.command_id,
                    "failed",
                    Some(error.to_string()),
                    json!({ "command": RUNTIME_AGENT_CREATE }),
                )?;
                return Err(error);
            }
        };

    let should_send_final = payload
        .reply_policy
        .as_ref()
        .map(|policy| policy.final_report || policy.progress)
        .unwrap_or(true);
    if should_send_final {
        send_command_status(
            outbox,
            &daemon_agent,
            &message,
            &payload.command_id,
            "ready",
            Some("runtime agent ready".to_string()),
            json!({
                "command": RUNTIME_AGENT_CREATE,
                "agent_did": outcome.agent_did,
                "handle": outcome.handle,
                "runtime_profile_id": outcome.runtime_profile_id,
                "runtime_plugin_id": outcome.runtime_plugin_id,
                "workspace_id": outcome.workspace_id,
                "registration_token_id": outcome.registration_token_id,
            }),
        )?;
    }

    Ok(AgentCommandOutcome::RuntimeAgentCreated(outcome))
}

pub fn setup_daemon_agent<C>(
    config: &DaemonConfig,
    state: &DaemonState,
    registration_client: &C,
    handle: &str,
    controller_did: &str,
    registration_token: RegistrationToken,
) -> Result<AgentDefinition>
where
    C: AgentRegistrationClient,
{
    let handle = normalize_handle(handle)?;
    if let Some(existing) = state.load_daemon_agent_by_handle(&handle)? {
        if existing.controller_did != controller_did {
            bail!("existing daemon agent controller_did does not match requested controller_did");
        }
        return Ok(existing);
    }
    let identity = generate_agent_identity(config, AgentKind::Daemon, &handle)?;
    let exchange = registration_client.exchange_token(AgentRegistrationExchangeRequest {
        token: registration_token,
        agent_kind: AgentKind::Daemon,
        controller_did: controller_did.to_string(),
        handle: handle.clone(),
        did_document: identity.did_document.clone(),
        endpoint_url: identity.endpoint_url.clone(),
        key_algorithm: identity.key_algorithm.clone(),
        public_key: identity.public_key.clone(),
    })?;
    verify_exchange_result(&exchange, AgentKind::Daemon, controller_did, &handle)?;
    if exchange.did != identity.did {
        bail!("registration token exchange returned a different DID");
    }
    state.store_agent_identity(&identity.into_record(handle.clone(), AgentKind::Daemon))?;
    let (local_agent_db_path, message_db_path) = agent_data_paths(&exchange.did)?;
    let definition = AgentDefinition {
        agent_did: exchange.did,
        handle: exchange.handle,
        agent_kind: AgentKind::Daemon,
        controller_did: exchange.controller_did,
        runtime_plugin_id: None,
        runtime_profile_id: None,
        workspace_id: None,
        policy_id: "default".to_string(),
        local_agent_db_path,
        message_db_path,
        status: "active".to_string(),
    };
    state.upsert_agent_definition(&definition)?;
    Ok(definition)
}

fn create_runtime_agent<C>(
    config: &DaemonConfig,
    state: &DaemonState,
    registration_client: &C,
    daemon_agent: &AgentDefinition,
    payload: &RuntimeAgentCreatePayload,
) -> Result<RuntimeAgentCreateOutcome>
where
    C: AgentRegistrationClient,
{
    let handle = normalize_handle(&payload.args.handle)?;
    let controller_did = payload.args.controller_did.trim();
    if controller_did.is_empty() {
        bail!("controller_did must not be empty");
    }
    validate_runtime_create_args_contract(&payload.args)?;
    let plugin_id = runtime_plugin_id(&payload.args.runtime)?;
    let profile_id = runtime_profile_id(&payload.args.runtime, &handle)?;
    let workspace_root = workspace_path(payload.args.workspace.as_deref())?;
    let workspace_id = workspace_root
        .as_ref()
        .map(|_| workspace_id(&handle))
        .transpose()?;
    let identity = generate_agent_identity(config, AgentKind::Runtime, &handle)?;
    let exchange = registration_client.exchange_token(AgentRegistrationExchangeRequest {
        token: RegistrationToken::new(payload.args.registration_token.clone())?,
        agent_kind: AgentKind::Runtime,
        controller_did: controller_did.to_string(),
        handle: handle.clone(),
        did_document: identity.did_document.clone(),
        endpoint_url: identity.endpoint_url.clone(),
        key_algorithm: identity.key_algorithm.clone(),
        public_key: identity.public_key.clone(),
    })?;
    verify_exchange_result(&exchange, AgentKind::Runtime, controller_did, &handle)?;
    if exchange.did != identity.did {
        bail!("registration token exchange returned a different DID");
    }
    state.store_agent_identity(&identity.into_record(handle.clone(), AgentKind::Runtime))?;

    let profile = RuntimeAgentProfile {
        agent_did: exchange.did.clone(),
        controller_did: exchange.controller_did.clone(),
        runtime_profile_id: profile_id.clone(),
        runtime_plugin_id: plugin_id.clone(),
        display_name: Some(exchange.handle.clone()),
        workspace_id: workspace_id.clone(),
        workspace_root,
        workspace_mode: workspace_id.as_ref().map(|_| WorkspaceMode::SharedRoot),
    };
    state.upsert_runtime_agent_profile_with_handle(&profile, &exchange.handle)?;
    state.insert_audit_event_json(
        "agent.registration.exchange",
        Some(&exchange.did),
        Some(&profile_id),
        None,
        Some(&exchange.token_id),
        json!({
            "agent_kind": AgentKind::Runtime.as_str(),
            "controller_did": exchange.controller_did,
            "handle": exchange.handle,
            "daemon_agent_did": daemon_agent.agent_did,
            "command_id": payload.command_id,
        }),
    )?;

    if profile.runtime_plugin_id == HERMES_RUNTIME_PLUGIN_ID {
        match initialize_hermes_profile(config, state, &profile, &exchange.handle) {
            Ok(install) => {
                state.insert_audit_event_json(
                    "hermes.profile.initialize",
                    Some(&profile.agent_did),
                    Some(&profile.runtime_profile_id),
                    None,
                    None,
                    json!({
                        "status": install.record.status,
                        "hermes_profile": install.record.hermes_profile,
                        "hermes_home": install.record.hermes_home,
                        "awiki_skills_version": install.record.awiki_skills_version,
                    }),
                )?;
            }
            Err(error) => {
                mark_hermes_profile_failed(config, state, &profile, &exchange.handle)?;
                state.insert_audit_event_json(
                    "hermes.profile.initialize",
                    Some(&profile.agent_did),
                    Some(&profile.runtime_profile_id),
                    None,
                    None,
                    json!({
                        "status": "failed",
                        "reason": error.to_string(),
                    }),
                )?;
                return Err(error).context("initialize Hermes profile");
            }
        }
    }

    Ok(RuntimeAgentCreateOutcome {
        command_id: payload.command_id.clone(),
        agent_did: exchange.did,
        handle: exchange.handle,
        runtime_profile_id: profile_id,
        runtime_plugin_id: plugin_id,
        workspace_id,
        registration_token_id: exchange.token_id,
    })
}

fn validate_runtime_create_args_contract(args: &RuntimeAgentCreateArgs) -> Result<()> {
    resolve_runtime(&args.runtime, args.driver_id.as_deref())?;
    validate_optional_object(args.driver_config.as_ref(), "driver_config")?;
    validate_optional_object(args.recipient_policy.as_ref(), "recipient_policy")?;
    Ok(())
}

fn validate_optional_object(value: Option<&Value>, field_name: &str) -> Result<()> {
    if value.is_some_and(|value| !value.is_object()) {
        bail!("{field_name} must be a JSON object when present");
    }
    Ok(())
}

fn validate_application_json_payload(message: &IncomingAgentPayloadMessage) -> Result<()> {
    if message.content_type != "application/json" {
        bail!("agent payload command must use application/json");
    }
    if !message.payload.is_object() {
        bail!("agent payload command body.payload must be a JSON object");
    }
    if message.sender_did.trim().is_empty() {
        bail!("sender_did must not be empty");
    }
    if message.target_agent_did.trim().is_empty() {
        bail!("target_agent_did must not be empty");
    }
    Ok(())
}

fn verify_exchange_result(
    exchange: &AgentRegistrationExchangeResult,
    expected_kind: AgentKind,
    expected_controller_did: &str,
    expected_handle: &str,
) -> Result<()> {
    if exchange.agent_kind != expected_kind {
        bail!("registration token exchange returned wrong agent kind");
    }
    if exchange.controller_did != expected_controller_did {
        bail!("registration token exchange returned wrong controller_did");
    }
    if normalize_handle(&exchange.handle)? != expected_handle {
        bail!("registration token exchange returned wrong handle");
    }
    if exchange.did.trim().is_empty() {
        bail!("registration token exchange returned empty DID");
    }
    if exchange.status != "registered" {
        bail!("registration token exchange did not register agent");
    }
    Ok(())
}

fn send_command_status<O>(
    outbox: &O,
    daemon_agent: &AgentDefinition,
    message: &IncomingAgentPayloadMessage,
    command_id: &str,
    state: &str,
    text: Option<String>,
    result: Value,
) -> Result<()>
where
    O: AgentManagementOutbox,
{
    let payload = json!({
        "schema": AGENT_STATUS_SCHEMA,
        "command_id": command_id,
        "state": state,
        "message": text,
        "result": result,
    });
    outbox.send_agent_status(&AgentStatusResponse {
        conversation_id: message.conversation_id.clone(),
        agent_did: daemon_agent.agent_did.clone(),
        recipient_did: message.sender_did.clone(),
        payload,
    })
}

fn default_true() -> bool {
    true
}
