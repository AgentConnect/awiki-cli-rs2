use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::agent::{
    agent_data_paths, generate_agent_identity, normalize_handle, resolve_runtime,
    runtime_profile_id, workspace_id, workspace_path, AgentDefinition, AgentKind,
    CODEX_CLI_DRIVER_ID, GENERIC_CLI_RUNTIME_PLUGIN_ID,
};
use crate::controller_scope::{verify_daemon_controller_sender, VerifiedControllerSender};
use crate::outbox::{AgentManagementOutbox, AgentStatusResponse};
use crate::plugins::hermes::{
    initialize_hermes_profile, mark_hermes_profile_failed, HERMES_RUNTIME_PLUGIN_ID,
};
use crate::public_error::{sanitize_public_error, sanitize_public_error_chain};
use crate::registration::{
    AgentInventoryClient, AgentRegistrationClient, AgentRegistrationExchangeRequest,
    AgentRegistrationExchangeResult, DidAuthMaterial, RegistrationToken,
};
use crate::runtime::{RuntimeAgentProfile, RuntimeRunStatus};
use crate::runtime_inbox::{
    clamp_limit, query_runtime_inbox, query_runtime_inbox_thread, RuntimeInboxQuery,
    RuntimeInboxScope, RuntimeInboxThreadKind, RuntimeInboxThreadQuery,
};
use crate::state::{
    controller_scope_key, CliRuntimeProfileRecord, DaemonState, HermesSessionRoute,
    RuntimeAgentCreateRequestRecord,
};
use crate::upgrade::{
    upgrade_daemon_with_progress_and_cancel, DaemonUpgradeCancelToken, DaemonUpgradeProgress,
    DaemonUpgradeRequest, DAEMON_UPGRADE_CANCELLED_ERROR,
};
use crate::workspace::WorkspaceMode;
use crate::DaemonConfig;

const AGENT_COMMAND_SCHEMA: &str = "awiki.agent.command.v1";
const AGENT_STATUS_SCHEMA: &str = "awiki.agent.status.v1";
const RUNTIME_AGENT_CREATE: &str = "runtime.agent.create";
const AGENT_STATUS_QUERY: &str = "agent.status.query";
const RUNTIME_SESSION_RESET: &str = "runtime.session.reset";
const RUNTIME_RUN_RETRY: &str = "runtime.run.retry";
const DAEMON_UPGRADE: &str = "daemon.upgrade";
const DAEMON_UPGRADE_CANCEL: &str = "daemon.upgrade.cancel";
const RUNTIME_AGENT_REBUILD: &str = "runtime.agent.rebuild";
const DAEMON_DELETE: &str = "daemon.delete";
const RUNTIME_AGENT_DELETE: &str = "runtime.agent.delete";
const RUNTIME_INBOX_QUERY: &str = "runtime.inbox.query";
const RUNTIME_INBOX_THREAD_QUERY: &str = "runtime.inbox.thread.query";
const STATUS_QUERY_MIN_INTERVAL_MS: i64 = 10_000;

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
    pub driver_id: Option<String>,
    pub defaulted_driver_id: bool,
    pub workspace_id: Option<String>,
    pub registration_token_id: String,
    pub runtime_alias: String,
    pub display_name: String,
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeAgentCreateRequest {
    pub command_id: String,
    pub handle: Option<String>,
    pub runtime: String,
    pub display_name: Option<String>,
    pub driver_id: Option<String>,
    pub driver_config: Option<Value>,
    pub recipient_policy: Option<Value>,
    pub workspace: Option<String>,
    pub controller_did: String,
    pub registration_token: String,
    pub client_request_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentCommandOutcome {
    RuntimeAgentCreated(RuntimeAgentCreateOutcome),
    StatusReported { command_id: String },
}

impl std::fmt::Debug for RuntimeAgentCreateRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeAgentCreateRequest")
            .field("command_id", &self.command_id)
            .field("handle", &self.handle)
            .field("runtime", &self.runtime)
            .field("display_name", &self.display_name)
            .field("driver_id", &self.driver_id)
            .field("driver_config", &self.driver_config)
            .field("recipient_policy", &self.recipient_policy)
            .field("workspace", &self.workspace)
            .field("controller_did", &self.controller_did)
            .field("registration_token", &"<redacted-registration-token>")
            .field("client_request_id", &self.client_request_id)
            .finish()
    }
}

#[derive(Deserialize)]
struct AgentCommandEnvelope {
    schema: String,
    command_id: String,
    command: String,
    #[serde(default)]
    target_agent_kind: Option<String>,
    #[serde(default)]
    args: Value,
    #[serde(default)]
    reply_policy: Option<ReplyPolicy>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct DaemonUpgradeTaskKey {
    daemon_agent_did: String,
    controller_scope_key: String,
    command_id: String,
}

impl DaemonUpgradeTaskKey {
    fn new(daemon_agent_did: &str, controller_scope_key: &str, command_id: &str) -> Self {
        Self {
            daemon_agent_did: daemon_agent_did.to_string(),
            controller_scope_key: controller_scope_key.to_string(),
            command_id: command_id.to_string(),
        }
    }
}

static DAEMON_UPGRADE_CANCEL_TOKENS: OnceLock<
    Mutex<HashMap<DaemonUpgradeTaskKey, DaemonUpgradeCancelToken>>,
> = OnceLock::new();

fn daemon_upgrade_cancel_tokens(
) -> &'static Mutex<HashMap<DaemonUpgradeTaskKey, DaemonUpgradeCancelToken>> {
    DAEMON_UPGRADE_CANCEL_TOKENS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn register_daemon_upgrade_task(key: DaemonUpgradeTaskKey, token: DaemonUpgradeCancelToken) {
    daemon_upgrade_cancel_tokens()
        .lock()
        .expect("daemon upgrade cancel registry poisoned")
        .insert(key, token);
}

fn unregister_daemon_upgrade_task(key: &DaemonUpgradeTaskKey) {
    daemon_upgrade_cancel_tokens()
        .lock()
        .expect("daemon upgrade cancel registry poisoned")
        .remove(key);
}

fn cancel_daemon_upgrade_task(key: &DaemonUpgradeTaskKey) -> bool {
    let token = daemon_upgrade_cancel_tokens()
        .lock()
        .expect("daemon upgrade cancel registry poisoned")
        .get(key)
        .cloned();
    if let Some(token) = token {
        token.cancel();
        true
    } else {
        false
    }
}

#[derive(Deserialize)]
struct RuntimeAgentCreatePayload {
    command_id: String,
    args: RuntimeAgentCreateArgs,
    #[serde(default)]
    reply_policy: Option<ReplyPolicy>,
}

#[derive(Deserialize)]
struct RuntimeAgentCreateArgs {
    #[serde(default)]
    handle: Option<String>,
    runtime: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    driver_id: Option<String>,
    #[serde(default)]
    driver_config: Option<Value>,
    #[serde(default)]
    recipient_policy: Option<Value>,
    #[serde(default)]
    workspace: Option<String>,
    #[serde(default)]
    workspace_mode: Option<String>,
    #[serde(default)]
    workspace_strategy: Option<String>,
    #[serde(default)]
    default_sandbox: Option<String>,
    #[serde(default)]
    default_model: Option<String>,
    controller_did: String,
    registration_token: String,
    #[serde(default)]
    client_request_id: Option<String>,
}

#[derive(Clone, Deserialize)]
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
    C: AgentRegistrationClient + AgentInventoryClient,
    O: AgentManagementOutbox + Clone + Send + 'static,
{
    validate_application_json_payload(&message)?;
    let daemon_agent = state
        .load_active_agent_definition(&message.target_agent_did)
        .context("load target daemon agent")?;
    if daemon_agent.agent_kind != AgentKind::Daemon {
        bail!("target agent is not a daemon agent");
    }
    let verified_sender = verify_daemon_controller_sender(
        config,
        state,
        registration_client,
        &daemon_agent,
        &message.sender_did,
    )
    .context("verify daemon command sender scope")?;
    let daemon_agent = state
        .load_active_agent_definition(&message.target_agent_did)
        .context("reload target daemon agent after controller sender verification")?;

    let envelope: AgentCommandEnvelope =
        serde_json::from_value(message.payload.clone()).context("parse agent command payload")?;
    if envelope.schema != AGENT_COMMAND_SCHEMA {
        bail!("unsupported agent command schema: {}", envelope.schema);
    }
    match envelope.command.as_str() {
        RUNTIME_AGENT_CREATE => {
            if envelope
                .target_agent_kind
                .as_deref()
                .is_some_and(|kind| kind != AgentKind::Runtime.as_str())
            {
                bail!("runtime.agent.create target_agent_kind must be runtime");
            }
            let payload = RuntimeAgentCreatePayload {
                command_id: envelope.command_id.clone(),
                args: serde_json::from_value(envelope.args.clone())
                    .context("parse runtime.agent.create args")?,
                reply_policy: envelope.reply_policy.clone(),
            };
            let outcome = match create_runtime_agent(
                config,
                state,
                registration_client,
                &daemon_agent,
                &verified_sender,
                &payload,
            ) {
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
                        "runtime_agent_did": outcome.agent_did,
                        "daemon_agent_did": daemon_agent.agent_did,
                        "runtime": outcome.runtime_alias,
                        "handle": outcome.handle,
                        "display_name": outcome.display_name,
                        "runtime_profile_id": outcome.runtime_profile_id,
                        "runtime_plugin_id": outcome.runtime_plugin_id,
                        "driver_id": outcome.driver_id,
                        "defaulted_driver_id": outcome.defaulted_driver_id,
                        "workspace_id": outcome.workspace_id,
                        "registration_token_id": outcome.registration_token_id,
                    }),
                )?;
            }

            Ok(AgentCommandOutcome::RuntimeAgentCreated(outcome))
        }
        AGENT_STATUS_QUERY => {
            send_snapshot_status(
                config,
                outbox,
                state,
                &daemon_agent,
                &message,
                &envelope.command_id,
            )?;
            Ok(AgentCommandOutcome::StatusReported {
                command_id: envelope.command_id,
            })
        }
        RUNTIME_SESSION_RESET => {
            handle_runtime_session_reset(outbox, state, &daemon_agent, &message, &envelope)?;
            Ok(AgentCommandOutcome::StatusReported {
                command_id: envelope.command_id,
            })
        }
        RUNTIME_RUN_RETRY => {
            handle_runtime_run_retry(outbox, state, &daemon_agent, &message, &envelope)?;
            Ok(AgentCommandOutcome::StatusReported {
                command_id: envelope.command_id,
            })
        }
        RUNTIME_INBOX_QUERY => {
            handle_runtime_inbox_query(config, outbox, state, &daemon_agent, &message, &envelope)?;
            Ok(AgentCommandOutcome::StatusReported {
                command_id: envelope.command_id,
            })
        }
        RUNTIME_INBOX_THREAD_QUERY => {
            handle_runtime_inbox_thread_query(
                config,
                outbox,
                state,
                &daemon_agent,
                &message,
                &envelope,
            )?;
            Ok(AgentCommandOutcome::StatusReported {
                command_id: envelope.command_id,
            })
        }
        DAEMON_UPGRADE => {
            handle_daemon_upgrade(config, outbox, state, &daemon_agent, &message, &envelope)?;
            Ok(AgentCommandOutcome::StatusReported {
                command_id: envelope.command_id,
            })
        }
        DAEMON_UPGRADE_CANCEL => {
            handle_daemon_upgrade_cancel(
                config,
                state,
                outbox,
                &daemon_agent,
                &message,
                &envelope,
            )?;
            Ok(AgentCommandOutcome::StatusReported {
                command_id: envelope.command_id,
            })
        }
        RUNTIME_AGENT_REBUILD => {
            send_command_status(
                outbox,
                &daemon_agent,
                &message,
                &envelope.command_id,
                "failed",
                Some("runtime.agent.rebuild is unsupported in v1".to_string()),
                json!({
                    "command": RUNTIME_AGENT_REBUILD,
                    "error_code": "unsupported_command",
                }),
            )?;
            Ok(AgentCommandOutcome::StatusReported {
                command_id: envelope.command_id,
            })
        }
        DAEMON_DELETE => {
            handle_daemon_delete(
                config,
                outbox,
                state,
                registration_client,
                &daemon_agent,
                &message,
                &envelope,
            )?;
            Ok(AgentCommandOutcome::StatusReported {
                command_id: envelope.command_id,
            })
        }
        RUNTIME_AGENT_DELETE => {
            handle_runtime_agent_delete(
                config,
                outbox,
                state,
                registration_client,
                &daemon_agent,
                &message,
                &envelope,
            )?;
            Ok(AgentCommandOutcome::StatusReported {
                command_id: envelope.command_id,
            })
        }
        other => {
            send_command_status(
                outbox,
                &daemon_agent,
                &message,
                &envelope.command_id,
                "failed",
                Some("unsupported command".to_string()),
                json!({
                    "command": other,
                    "error_code": "unsupported_command",
                }),
            )?;
            Ok(AgentCommandOutcome::StatusReported {
                command_id: envelope.command_id,
            })
        }
    }
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
        name: None,
        did_document: identity.did_document.clone(),
        endpoint_url: identity.endpoint_url.clone(),
        key_algorithm: identity.key_algorithm.clone(),
        public_key: identity.public_key.clone(),
        allow_existing_agent_did: false,
    })?;
    verify_exchange_result(&exchange, AgentKind::Daemon, controller_did, &handle)?;
    if exchange.did != identity.did {
        bail!("registration token exchange returned a different DID");
    }
    let exchange_scope_key = controller_scope_key(
        &exchange.controller_user_id,
        &exchange.controller_full_handle,
    )?;
    state.store_agent_identity(&identity.into_record(handle.clone(), AgentKind::Daemon))?;
    let (local_agent_db_path, message_db_path) = agent_data_paths(&exchange.did)?;
    let definition = AgentDefinition {
        agent_did: exchange.did,
        handle: exchange.handle,
        agent_kind: AgentKind::Daemon,
        controller_user_id: exchange.controller_user_id,
        controller_full_handle: exchange.controller_full_handle,
        controller_scope_key: exchange_scope_key,
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

pub fn create_runtime_agent_from_request<C>(
    config: &DaemonConfig,
    state: &DaemonState,
    registration_client: &C,
    daemon_agent: &AgentDefinition,
    request: RuntimeAgentCreateRequest,
) -> Result<RuntimeAgentCreateOutcome>
where
    C: AgentRegistrationClient,
{
    let verified_sender = VerifiedControllerSender {
        controller_user_id: daemon_agent.controller_user_id.clone(),
        controller_full_handle: daemon_agent.controller_full_handle.clone(),
        controller_scope_key: daemon_agent.controller_scope_key.clone(),
        controller_did: daemon_agent.controller_did.clone(),
        sender_did: daemon_agent.controller_did.clone(),
    };
    create_runtime_agent(
        config,
        state,
        registration_client,
        daemon_agent,
        &verified_sender,
        &RuntimeAgentCreatePayload {
            command_id: request.command_id,
            args: RuntimeAgentCreateArgs {
                handle: request.handle,
                runtime: request.runtime,
                display_name: request.display_name,
                driver_id: request.driver_id,
                driver_config: request.driver_config,
                recipient_policy: request.recipient_policy,
                workspace: request.workspace,
                workspace_mode: None,
                workspace_strategy: None,
                default_sandbox: None,
                default_model: None,
                controller_did: request.controller_did,
                registration_token: request.registration_token,
                client_request_id: request.client_request_id,
            },
            reply_policy: None,
        },
    )
}

fn create_runtime_agent<C>(
    config: &DaemonConfig,
    state: &DaemonState,
    registration_client: &C,
    daemon_agent: &AgentDefinition,
    verified_sender: &VerifiedControllerSender,
    payload: &RuntimeAgentCreatePayload,
) -> Result<RuntimeAgentCreateOutcome>
where
    C: AgentRegistrationClient,
{
    let handle = runtime_handle_from_args(&payload.args)?;
    let controller_did = payload.args.controller_did.trim();
    if controller_did.is_empty() {
        bail!("controller_did must not be empty");
    }
    if controller_did != verified_sender.sender_did {
        bail!("runtime agent controller_did must match verified sender_did");
    }
    let client_request_id = payload
        .args
        .client_request_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(client_request_id) = client_request_id {
        if let Some(existing) = state.load_runtime_agent_create_request(
            &daemon_agent.agent_did,
            &daemon_agent.controller_scope_key,
            client_request_id,
        )? {
            let mut outcome: RuntimeAgentCreateOutcome =
                serde_json::from_value(existing.outcome_json)
                    .context("parse cached runtime.agent.create outcome")?;
            outcome.command_id = payload.command_id.clone();
            return Ok(outcome);
        }
    }
    let resolution = validate_runtime_create_args_contract(&payload.args)?;
    let plugin_id = resolution.runtime_plugin_id.clone();
    let profile_id = runtime_profile_id(&payload.args.runtime, &handle)?;
    let is_generic_cli = plugin_id == GENERIC_CLI_RUNTIME_PLUGIN_ID;
    let workspace_mode = runtime_create_workspace_mode(&payload.args, is_generic_cli)?;
    let workspace_root = if is_generic_cli {
        Some(
            workspace_path(payload.args.workspace.as_deref())?
                .unwrap_or_else(|| generic_cli_workspace_root(config, &profile_id)),
        )
    } else {
        workspace_path(payload.args.workspace.as_deref())?
    };
    let workspace_id = workspace_root
        .as_ref()
        .map(|_| {
            if is_generic_cli {
                generic_cli_workspace_id(
                    resolution.driver_id.as_deref().unwrap_or("generic-cli"),
                    &handle,
                )
            } else {
                workspace_id(&handle)
            }
        })
        .transpose()?;
    let display_name = payload
        .args
        .display_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .context("runtime.agent.create display_name is required")?
        .to_string();
    let identity = generate_agent_identity(config, AgentKind::Runtime, &handle)?;
    let exchange = registration_client.exchange_token(AgentRegistrationExchangeRequest {
        token: RegistrationToken::new(payload.args.registration_token.clone())?,
        agent_kind: AgentKind::Runtime,
        controller_did: controller_did.to_string(),
        handle: handle.clone(),
        name: Some(display_name.clone()),
        did_document: identity.did_document.clone(),
        endpoint_url: identity.endpoint_url.clone(),
        key_algorithm: identity.key_algorithm.clone(),
        public_key: identity.public_key.clone(),
        allow_existing_agent_did: false,
    })?;
    verify_exchange_result(&exchange, AgentKind::Runtime, controller_did, &handle)?;
    if exchange.controller_user_id != daemon_agent.controller_user_id
        || exchange.controller_full_handle != daemon_agent.controller_full_handle
    {
        bail!("registration token exchange returned wrong controller scope");
    }
    if exchange.did != identity.did {
        bail!("registration token exchange returned a different DID");
    }
    state.store_agent_identity(&identity.into_record(handle.clone(), AgentKind::Runtime))?;

    let profile = RuntimeAgentProfile {
        agent_did: exchange.did.clone(),
        agent_handle: exchange.handle.clone(),
        controller_user_id: verified_sender.controller_user_id.clone(),
        controller_full_handle: verified_sender.controller_full_handle.clone(),
        controller_scope_key: verified_sender.controller_scope_key.clone(),
        controller_did: exchange.controller_did.clone(),
        runtime_profile_id: profile_id.clone(),
        runtime_plugin_id: plugin_id.clone(),
        display_name: Some(display_name.clone()),
        workspace_id: workspace_id.clone(),
        workspace_root,
        workspace_mode: workspace_id.as_ref().map(|_| workspace_mode),
    };
    state.upsert_runtime_agent_profile_with_handle(&profile, &exchange.handle)?;
    state.upsert_runtime_daemon_binding(
        &profile.agent_did,
        &daemon_agent.agent_did,
        &verified_sender.controller_user_id,
        &verified_sender.controller_full_handle,
        &verified_sender.controller_scope_key,
        &verified_sender.controller_did,
    )?;
    if profile.runtime_plugin_id == GENERIC_CLI_RUNTIME_PLUGIN_ID {
        let driver_id = resolution
            .driver_id
            .clone()
            .context("generic-cli runtime must have driver_id")?;
        let mut cli_profile = CliRuntimeProfileRecord::for_driver(&profile_id, driver_id)?;
        if cli_profile.driver_id == CODEX_CLI_DRIVER_ID {
            let profile_dir = codex_profile_dir(config, &profile_id);
            create_private_dir_all(&profile_dir)?;
            let config_home = profile_dir.join("codex-home");
            create_private_dir_all(&config_home)?;
            seed_codex_profile_home_from_host(&config_home)?;
            cli_profile.config_home = Some(config_home);
        }
        if let Some(recipient_policy) = payload.args.recipient_policy.clone() {
            cli_profile.recipient_policy_json = recipient_policy;
        }
        cli_profile.default_workspace_mode = workspace_mode;
        if let Some(default_sandbox) =
            normalize_optional_arg(payload.args.default_sandbox.as_deref())
        {
            cli_profile.default_sandbox = Some(default_sandbox);
        }
        if let Some(default_model) = normalize_optional_arg(payload.args.default_model.as_deref()) {
            cli_profile.default_model = Some(default_model);
        }
        if let Some(driver_config) = payload.args.driver_config.clone() {
            cli_profile.driver_config_json = driver_config;
        }
        state.upsert_cli_runtime_profile(&cli_profile)?;
    }
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
            "runtime_alias": payload.args.runtime.clone(),
            "runtime_plugin_id": profile.runtime_plugin_id.clone(),
            "driver_id": resolution.driver_id.clone(),
            "defaulted_driver_id": resolution.defaulted_driver_id,
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

    let outcome = RuntimeAgentCreateOutcome {
        command_id: payload.command_id.clone(),
        agent_did: exchange.did,
        handle: exchange.handle,
        runtime_profile_id: profile_id,
        runtime_plugin_id: plugin_id,
        driver_id: resolution.driver_id.clone(),
        defaulted_driver_id: resolution.defaulted_driver_id,
        workspace_id,
        registration_token_id: exchange.token_id,
        runtime_alias: payload.args.runtime.clone(),
        display_name,
    };
    if let Some(client_request_id) = client_request_id {
        let now = crate::security::runtime_token::current_time_millis()?;
        state.store_runtime_agent_create_request(&RuntimeAgentCreateRequestRecord {
            daemon_agent_did: daemon_agent.agent_did.clone(),
            controller_scope_key: verified_sender.controller_scope_key.clone(),
            controller_did: verified_sender.controller_did.clone(),
            client_request_id: client_request_id.to_string(),
            runtime_agent_did: outcome.agent_did.clone(),
            command_id: payload.command_id.clone(),
            outcome_json: serde_json::to_value(&outcome)?,
            created_at_ms: now,
            updated_at_ms: now,
        })?;
    }
    Ok(outcome)
}

fn validate_runtime_create_args_contract(
    args: &RuntimeAgentCreateArgs,
) -> Result<crate::agent::RuntimeResolution> {
    let resolution = resolve_runtime(&args.runtime, args.driver_id.as_deref())?;
    validate_optional_object(args.driver_config.as_ref(), "driver_config")?;
    validate_optional_object(args.recipient_policy.as_ref(), "recipient_policy")?;
    if let Some(workspace_mode) = args.workspace_mode.as_deref() {
        WorkspaceMode::parse(workspace_mode)?;
    }
    if let Some(workspace_strategy) = args.workspace_strategy.as_deref() {
        WorkspaceMode::parse(workspace_strategy)?;
    }
    if let (Some(workspace_mode), Some(workspace_strategy)) = (
        args.workspace_mode.as_deref(),
        args.workspace_strategy.as_deref(),
    ) {
        let workspace_mode = WorkspaceMode::parse(workspace_mode)?;
        let workspace_strategy = WorkspaceMode::parse(workspace_strategy)?;
        if workspace_mode != workspace_strategy {
            bail!("workspace_mode and workspace_strategy must resolve to the same value");
        }
    }
    Ok(resolution)
}

fn runtime_create_workspace_mode(
    args: &RuntimeAgentCreateArgs,
    is_generic_cli: bool,
) -> Result<WorkspaceMode> {
    if let Some(workspace_mode) = args.workspace_mode.as_deref() {
        return WorkspaceMode::parse(workspace_mode);
    }
    if let Some(workspace_strategy) = args.workspace_strategy.as_deref() {
        return WorkspaceMode::parse(workspace_strategy);
    }
    if is_generic_cli {
        Ok(WorkspaceMode::RouteRoot)
    } else {
        Ok(WorkspaceMode::SharedRoot)
    }
}

fn generic_cli_workspace_root(config: &DaemonConfig, runtime_profile_id: &str) -> PathBuf {
    config
        .runtime_cache_dir
        .join("generic-cli")
        .join("workspaces")
        .join(runtime_profile_id)
}

fn generic_cli_workspace_id(driver_id: &str, handle: &str) -> Result<String> {
    workspace_id(&format!("{driver_id}-{handle}"))
}

fn codex_profile_dir(config: &DaemonConfig, runtime_profile_id: &str) -> PathBuf {
    config
        .runtime_cache_dir
        .join("generic-cli")
        .join("profiles")
        .join(runtime_profile_id)
}

fn normalize_optional_arg(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

const CODEX_PROFILE_SEED_FILES: &[&str] = &["config.toml", "auth.json"];

fn seed_codex_profile_home_from_host(config_home: &Path) -> Result<()> {
    let Some(source_home) = host_codex_home_for_profile_seed(config_home) else {
        return Ok(());
    };
    seed_codex_profile_home_from_source(config_home, &source_home)
}

fn host_codex_home_for_profile_seed(config_home: &Path) -> Option<PathBuf> {
    let candidates = [
        std::env::var_os("CODEX_HOME").map(PathBuf::from),
        std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".codex")),
    ];
    candidates
        .into_iter()
        .flatten()
        .find(|candidate| candidate.is_dir() && !same_path(candidate, config_home))
}

fn seed_codex_profile_home_from_source(config_home: &Path, source_home: &Path) -> Result<()> {
    create_private_dir_all(config_home)?;
    for file_name in CODEX_PROFILE_SEED_FILES {
        copy_codex_seed_file_if_missing(source_home, config_home, file_name)?;
    }
    Ok(())
}

fn copy_codex_seed_file_if_missing(
    source_home: &Path,
    config_home: &Path,
    file_name: &str,
) -> Result<()> {
    let source = source_home.join(file_name);
    let target = config_home.join(file_name);
    let source_metadata = match std::fs::symlink_metadata(&source) {
        Ok(metadata) if metadata.file_type().is_file() => metadata,
        Ok(_) => return Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).with_context(|| format!("inspect {}", source.display())),
    };
    if target.exists() {
        return Ok(());
    }
    if source_metadata.len() > 2 * 1024 * 1024 {
        return Ok(());
    }
    std::fs::copy(&source, &target)
        .with_context(|| format!("seed Codex profile file {}", target.display()))?;
    set_owner_private_file_permissions(&target)?;
    Ok(())
}

fn create_private_dir_all(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path).with_context(|| format!("create {}", path.display()))?;
    set_owner_private_dir_permissions(path)?;
    Ok(())
}

#[cfg(unix)]
fn set_owner_private_dir_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = std::fs::metadata(path)?.permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_owner_private_dir_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn set_owner_private_file_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = std::fs::metadata(path)?.permissions();
    permissions.set_mode(0o600);
    std::fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_owner_private_file_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

fn same_path(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn runtime_handle_from_args(args: &RuntimeAgentCreateArgs) -> Result<String> {
    if let Some(handle) = args.handle.as_deref() {
        return normalize_handle(handle);
    }
    bail!("runtime.agent.create handle is required")
}

fn validate_optional_object(value: Option<&Value>, field_name: &str) -> Result<()> {
    if value.is_some_and(|value| !value.is_object()) {
        bail!("{field_name} must be a JSON object when present");
    }
    Ok(())
}

fn send_snapshot_status<O>(
    config: &DaemonConfig,
    outbox: &O,
    state: &DaemonState,
    daemon_agent: &AgentDefinition,
    message: &IncomingAgentPayloadMessage,
    command_id: &str,
) -> Result<()>
where
    O: AgentManagementOutbox + Clone + Send + 'static,
{
    if !state.should_emit_agent_status_query_snapshot(
        &daemon_agent.agent_did,
        &daemon_agent.controller_did,
        STATUS_QUERY_MIN_INTERVAL_MS,
    )? {
        return send_command_status(
            outbox,
            daemon_agent,
            message,
            command_id,
            "ready",
            Some("daemon status query throttled".to_string()),
            json!({
                "command": AGENT_STATUS_QUERY,
                "daemon_agent_did": daemon_agent.agent_did,
                "throttled": true,
                "retry_after_seconds": 10,
            }),
        );
    }

    let snapshot = crate::agent_status::daemon_snapshot_payload(config, state, daemon_agent)?;
    send_command_status(
        outbox,
        daemon_agent,
        message,
        command_id,
        "ready",
        Some("daemon status snapshot".to_string()),
        snapshot,
    )
}

fn handle_runtime_session_reset<O>(
    outbox: &O,
    state: &DaemonState,
    daemon_agent: &AgentDefinition,
    message: &IncomingAgentPayloadMessage,
    payload: &AgentCommandEnvelope,
) -> Result<()>
where
    O: AgentManagementOutbox + Clone + Send + 'static,
{
    let runtime_agent_did = required_arg_string(&payload.args, "runtime_agent_did")?;
    let conversation_id = optional_arg_string(&payload.args, "conversation_id");
    let runtime_agent = match load_owned_runtime_agent(state, daemon_agent, &runtime_agent_did) {
        Ok(runtime_agent) => runtime_agent,
        Err(_) => {
            return send_command_status(
                outbox,
                daemon_agent,
                message,
                &payload.command_id,
                "failed",
                Some("runtime agent does not belong to this daemon".to_string()),
                json!({
                    "command": RUNTIME_SESSION_RESET,
                    "runtime_agent_did": runtime_agent_did,
                    "error_code": "runtime_not_owned",
                }),
            );
        }
    };
    let Some(runtime_profile_id) = runtime_agent.runtime_profile_id.as_deref() else {
        return send_command_status(
            outbox,
            daemon_agent,
            message,
            &payload.command_id,
            "failed",
            Some("runtime agent profile is missing".to_string()),
            json!({
                "command": RUNTIME_SESSION_RESET,
                "runtime_agent_did": runtime_agent_did,
                "error_code": "runtime_profile_missing",
            }),
        );
    };
    let reset_count = if let Some(conversation_id) = conversation_id.as_deref() {
        let route = HermesSessionRoute::new(
            runtime_agent.agent_did.clone(),
            runtime_agent.handle.clone(),
            runtime_profile_id.to_string(),
            daemon_agent.controller_scope_key.clone(),
            if conversation_id.starts_with("group:") {
                "group_visible"
            } else {
                "controller_private"
            },
            if let Some(group_key) = conversation_id.strip_prefix("group:") {
                format!("group:{group_key}")
            } else {
                format!("controller:{}", daemon_agent.controller_scope_key)
            },
            Some(conversation_id.to_string()),
            "conversation",
        );
        state.reset_active_hermes_session_by_route(&route)?
    } else {
        state.reset_active_hermes_sessions_for_runtime_controller_scope(
            &runtime_agent.agent_did,
            runtime_profile_id,
            &daemon_agent.controller_scope_key,
        )?
    };
    state.insert_audit_event_json(
        "runtime.session.reset",
        Some(&runtime_agent.agent_did),
        Some(runtime_profile_id),
        None,
        None,
        json!({
            "command_id": payload.command_id,
            "conversation_id_present": conversation_id.is_some(),
            "reset_count": reset_count,
        }),
    )?;
    send_command_status(
        outbox,
        daemon_agent,
        message,
        &payload.command_id,
        "ready",
        Some("runtime session mapping reset".to_string()),
        json!({
            "command": RUNTIME_SESSION_RESET,
            "runtime_agent_did": runtime_agent.agent_did,
            "daemon_agent_did": daemon_agent.agent_did,
            "reset_count": reset_count,
        }),
    )
}

fn handle_runtime_run_retry<O>(
    outbox: &O,
    state: &DaemonState,
    daemon_agent: &AgentDefinition,
    message: &IncomingAgentPayloadMessage,
    payload: &AgentCommandEnvelope,
) -> Result<()>
where
    O: AgentManagementOutbox + Clone + Send + 'static,
{
    let runtime_agent_did = required_arg_string(&payload.args, "runtime_agent_did")?;
    let run_id = required_arg_string(&payload.args, "run_id")?;
    if load_owned_runtime_agent(state, daemon_agent, &runtime_agent_did).is_err() {
        return send_command_status(
            outbox,
            daemon_agent,
            message,
            &payload.command_id,
            "failed",
            Some("runtime agent does not belong to this daemon".to_string()),
            json!({
                "command": RUNTIME_RUN_RETRY,
                "runtime_agent_did": runtime_agent_did,
                "run_id": run_id,
                "error_code": "runtime_not_owned",
            }),
        );
    }
    let run = match state.load_runtime_run(&run_id) {
        Ok(run) => run,
        Err(_) => {
            return send_command_status(
                outbox,
                daemon_agent,
                message,
                &payload.command_id,
                "failed",
                Some("run_id is not in the local recent run cache".to_string()),
                json!({
                    "command": RUNTIME_RUN_RETRY,
                    "runtime_agent_did": runtime_agent_did,
                    "run_id": run_id,
                    "error_code": "run_not_found",
                }),
            );
        }
    };
    if run.agent_did != runtime_agent_did {
        return send_command_status(
            outbox,
            daemon_agent,
            message,
            &payload.command_id,
            "failed",
            Some("run does not belong to runtime agent".to_string()),
            json!({
                "command": RUNTIME_RUN_RETRY,
                "runtime_agent_did": runtime_agent_did,
                "run_id": run_id,
                "error_code": "run_runtime_mismatch",
            }),
        );
    }
    if run.status != RuntimeRunStatus::Failed {
        return send_command_status(
            outbox,
            daemon_agent,
            message,
            &payload.command_id,
            "failed",
            Some("only failed runs can be retried".to_string()),
            json!({
                "command": RUNTIME_RUN_RETRY,
                "runtime_agent_did": runtime_agent_did,
                "run_id": run_id,
                "error_code": "invalid_run_state",
                "run_state": run.status.as_str(),
            }),
        );
    }
    let retry = state.insert_runtime_retry_request(&run, &payload.command_id)?;
    let task = state.load_runtime_task(&run.task_id)?;
    let retry_run_id = format!("run_{}", retry.retry_id);
    state.insert_audit_event_json(
        "runtime.run.retry.requested",
        Some(&run.agent_did),
        Some(&run.runtime_profile_id),
        Some(&run.run_id),
        None,
        json!({
            "command_id": payload.command_id,
            "task_id": run.task_id,
            "retry_id": retry.retry_id,
        }),
    )?;
    send_command_status(
        outbox,
        daemon_agent,
        message,
        &payload.command_id,
        "queued",
        Some("runtime run retry queued".to_string()),
        json!({
            "command": RUNTIME_RUN_RETRY,
            "runtime_agent_did": runtime_agent_did,
            "daemon_agent_did": daemon_agent.agent_did,
            "run_id": run_id,
            "retry_run_id": retry_run_id,
            "message_id": task.task_id,
            "conversation_id": task.conversation_id,
            "retry_id": retry.retry_id,
            "retry_status": retry.status,
            "updated_at": rfc3339_from_millis(retry.updated_at_ms),
        }),
    )
}

fn handle_runtime_inbox_query<O>(
    config: &DaemonConfig,
    outbox: &O,
    state: &DaemonState,
    daemon_agent: &AgentDefinition,
    message: &IncomingAgentPayloadMessage,
    payload: &AgentCommandEnvelope,
) -> Result<()>
where
    O: AgentManagementOutbox,
{
    if payload
        .target_agent_kind
        .as_deref()
        .is_some_and(|kind| kind != AgentKind::Daemon.as_str())
    {
        return send_runtime_inbox_failure(
            outbox,
            daemon_agent,
            message,
            payload,
            RUNTIME_INBOX_QUERY,
            "runtime_inbox_invalid_target",
            "runtime.inbox.query must target daemon",
            "runtime_inbox",
        );
    }
    let runtime_agent_did = required_arg_string(&payload.args, "runtime_agent_did")?;
    let scope =
        match RuntimeInboxScope::parse(optional_arg_string(&payload.args, "scope").as_deref()) {
            Ok(scope) => scope,
            Err(error) => {
                return send_runtime_inbox_failure(
                    outbox,
                    daemon_agent,
                    message,
                    payload,
                    RUNTIME_INBOX_QUERY,
                    "runtime_inbox_invalid_scope",
                    &error.to_string(),
                    "runtime_inbox",
                );
            }
        };
    let limit = match clamp_limit(optional_arg_u64(&payload.args, "limit"), 30) {
        Ok(limit) => limit,
        Err(error) => {
            return send_runtime_inbox_failure(
                outbox,
                daemon_agent,
                message,
                payload,
                RUNTIME_INBOX_QUERY,
                "runtime_inbox_invalid_limit",
                &error.to_string(),
                "runtime_inbox",
            );
        }
    };
    let cursor = optional_arg_string(&payload.args, "cursor");
    let runtime_agent = match load_owned_runtime_agent(state, daemon_agent, &runtime_agent_did) {
        Ok(runtime_agent) => runtime_agent,
        Err(_) => {
            return send_runtime_inbox_failure_with_runtime(
                outbox,
                daemon_agent,
                message,
                payload,
                RUNTIME_INBOX_QUERY,
                &runtime_agent_did,
                "runtime_not_owned",
                "runtime agent does not belong to this daemon",
                "runtime_inbox",
            );
        }
    };
    let result = match query_runtime_inbox(
        config,
        state,
        &runtime_agent,
        RuntimeInboxQuery {
            runtime_agent_did: runtime_agent.agent_did.clone(),
            scope,
            limit,
            cursor,
        },
    ) {
        Ok(result) => result,
        Err(error) => {
            return send_runtime_inbox_failure_with_runtime(
                outbox,
                daemon_agent,
                message,
                payload,
                RUNTIME_INBOX_QUERY,
                &runtime_agent_did,
                "runtime_inbox_unavailable",
                &sanitize_public_error(&error.to_string()),
                "runtime_inbox",
            );
        }
    };
    send_scoped_command_status(
        outbox,
        daemon_agent,
        message,
        payload,
        "succeeded",
        Some("runtime inbox loaded".to_string()),
        "runtime_inbox",
        json!({
            "command": RUNTIME_INBOX_QUERY,
            "daemon_agent_did": daemon_agent.agent_did,
            "runtime_agent_did": runtime_agent.agent_did,
            "scope": scope_name(scope),
            "result": result,
        }),
    )
}

fn handle_runtime_inbox_thread_query<O>(
    config: &DaemonConfig,
    outbox: &O,
    state: &DaemonState,
    daemon_agent: &AgentDefinition,
    message: &IncomingAgentPayloadMessage,
    payload: &AgentCommandEnvelope,
) -> Result<()>
where
    O: AgentManagementOutbox,
{
    if payload
        .target_agent_kind
        .as_deref()
        .is_some_and(|kind| kind != AgentKind::Daemon.as_str())
    {
        return send_runtime_inbox_failure(
            outbox,
            daemon_agent,
            message,
            payload,
            RUNTIME_INBOX_THREAD_QUERY,
            "runtime_inbox_invalid_target",
            "runtime.inbox.thread.query must target daemon",
            "runtime_inbox_thread",
        );
    }
    let runtime_agent_did = required_arg_string(&payload.args, "runtime_agent_did")?;
    let thread_id = required_arg_string(&payload.args, "thread_id")?;
    let kind = match RuntimeInboxThreadKind::parse(
        optional_arg_string(&payload.args, "kind").as_deref(),
        &thread_id,
    ) {
        Ok(kind) => kind,
        Err(error) => {
            return send_runtime_inbox_failure_with_runtime(
                outbox,
                daemon_agent,
                message,
                payload,
                RUNTIME_INBOX_THREAD_QUERY,
                &runtime_agent_did,
                "runtime_inbox_invalid_kind",
                &error.to_string(),
                "runtime_inbox_thread",
            );
        }
    };
    let limit = match clamp_limit(optional_arg_u64(&payload.args, "limit"), 50) {
        Ok(limit) => limit,
        Err(error) => {
            return send_runtime_inbox_failure_with_runtime(
                outbox,
                daemon_agent,
                message,
                payload,
                RUNTIME_INBOX_THREAD_QUERY,
                &runtime_agent_did,
                "runtime_inbox_invalid_limit",
                &error.to_string(),
                "runtime_inbox_thread",
            );
        }
    };
    let runtime_agent = match load_owned_runtime_agent(state, daemon_agent, &runtime_agent_did) {
        Ok(runtime_agent) => runtime_agent,
        Err(_) => {
            return send_runtime_inbox_failure_with_runtime(
                outbox,
                daemon_agent,
                message,
                payload,
                RUNTIME_INBOX_THREAD_QUERY,
                &runtime_agent_did,
                "runtime_not_owned",
                "runtime agent does not belong to this daemon",
                "runtime_inbox_thread",
            );
        }
    };
    let result = match query_runtime_inbox_thread(
        config,
        state,
        &runtime_agent,
        RuntimeInboxThreadQuery {
            runtime_agent_did: runtime_agent.agent_did.clone(),
            thread_id: thread_id.clone(),
            kind,
            peer_did: optional_arg_string(&payload.args, "peer_did"),
            peer_handle: optional_arg_string(&payload.args, "peer_handle"),
            group_did: optional_arg_string(&payload.args, "group_did"),
            limit,
            cursor: optional_arg_string(&payload.args, "cursor"),
        },
    ) {
        Ok(result) => result,
        Err(error) => {
            return send_runtime_inbox_failure_with_runtime(
                outbox,
                daemon_agent,
                message,
                payload,
                RUNTIME_INBOX_THREAD_QUERY,
                &runtime_agent_did,
                "runtime_inbox_thread_unavailable",
                &sanitize_public_error(&error.to_string()),
                "runtime_inbox_thread",
            );
        }
    };
    send_scoped_command_status(
        outbox,
        daemon_agent,
        message,
        payload,
        "succeeded",
        Some("runtime inbox thread loaded".to_string()),
        "runtime_inbox_thread",
        json!({
            "command": RUNTIME_INBOX_THREAD_QUERY,
            "daemon_agent_did": daemon_agent.agent_did,
            "runtime_agent_did": runtime_agent.agent_did,
            "thread_id": thread_id,
            "kind": kind_name(kind),
            "result": result,
        }),
    )
}

fn handle_daemon_upgrade<O>(
    config: &DaemonConfig,
    outbox: &O,
    state: &DaemonState,
    daemon_agent: &AgentDefinition,
    message: &IncomingAgentPayloadMessage,
    payload: &AgentCommandEnvelope,
) -> Result<()>
where
    O: AgentManagementOutbox + Clone + Send + 'static,
{
    let target_daemon = optional_arg_string(&payload.args, "daemon_agent_did")
        .or_else(|| optional_arg_string(&payload.args, "target_daemon_agent_did"));
    if target_daemon
        .as_deref()
        .is_some_and(|target| target != daemon_agent.agent_did)
    {
        return send_command_status(
            outbox,
            daemon_agent,
            message,
            &payload.command_id,
            "failed",
            Some("daemon.upgrade can only target this daemon".to_string()),
            json!({
                "command": DAEMON_UPGRADE,
                "daemon_agent_did": daemon_agent.agent_did,
                "error_code": "daemon_target_mismatch",
            }),
        );
    }
    let target_version = optional_arg_string(&payload.args, "target_version")
        .unwrap_or_else(|| "latest".to_string());
    crate::agent_status::reconcile_daemon_upgrade_state_from_release_status(
        config,
        state,
        daemon_agent,
    )?;
    if let Some(existing) = state.load_latest_control_command_state(
        &daemon_agent.agent_did,
        &daemon_agent.controller_scope_key,
        DAEMON_UPGRADE,
        &["in_progress", "restart_scheduled"],
    )? {
        if existing.command_id != payload.command_id {
            return send_command_status(
                outbox,
                daemon_agent,
                message,
                &payload.command_id,
                duplicate_upgrade_public_state(&existing.status),
                Some(duplicate_upgrade_status_message(&existing.status)),
                json!({
                    "command": DAEMON_UPGRADE,
                    "daemon_agent_did": daemon_agent.agent_did,
                    "target_version": existing.target_version.unwrap_or(target_version),
                    "status": existing.status,
                    "duplicate": true,
                    "original_command_id": existing.command_id,
                }),
            );
        }
    }
    if let Some(existing) = state.try_begin_control_command(
        &daemon_agent.agent_did,
        &daemon_agent.controller_scope_key,
        &payload.command_id,
        DAEMON_UPGRADE,
        &message.message_id,
        Some(&target_version),
    )? {
        return send_command_status(
            outbox,
            daemon_agent,
            message,
            &payload.command_id,
            duplicate_upgrade_public_state(&existing.status),
            Some(duplicate_upgrade_status_message(&existing.status)),
            if existing.result_json.is_object() {
                existing.result_json
            } else {
                json!({
                    "command": DAEMON_UPGRADE,
                    "daemon_agent_did": daemon_agent.agent_did,
                    "target_version": existing.target_version.unwrap_or(target_version),
                    "duplicate": true,
                })
            },
        );
    }
    send_command_status(
        outbox,
        daemon_agent,
        message,
        &payload.command_id,
        "upgrading",
        Some("daemon upgrade started".to_string()),
        json!({
            "command": DAEMON_UPGRADE,
            "daemon_agent_did": daemon_agent.agent_did,
            "target_version": target_version.clone(),
        }),
    )?;
    let request = match DaemonUpgradeRequest::from_env(config, target_version.clone()) {
        Ok(request) => request,
        Err(error) => {
            let summary = sanitize_public_error_chain(error.chain());
            let result = json!({
                "command": DAEMON_UPGRADE,
                "daemon_agent_did": daemon_agent.agent_did,
                "target_version": target_version,
                "error_code": "upgrade_failed",
                "last_error_summary": summary,
            });
            state.mark_control_command_state(
                &daemon_agent.agent_did,
                &daemon_agent.controller_scope_key,
                &payload.command_id,
                "failed",
                result.clone(),
                Some(&summary),
            )?;
            return send_command_status(
                outbox,
                daemon_agent,
                message,
                &payload.command_id,
                "failed",
                Some("daemon upgrade failed".to_string()),
                result,
            );
        }
    };
    let task_config = config.clone();
    let task_state = state.clone();
    let task_outbox = (*outbox).clone();
    let task_daemon_agent = daemon_agent.clone();
    let task_message = message.clone();
    let task_command_id = payload.command_id.clone();
    let task_target_version = target_version.clone();
    let cancel_token = DaemonUpgradeCancelToken::default();
    let task_key = DaemonUpgradeTaskKey::new(
        &task_daemon_agent.agent_did,
        &task_daemon_agent.controller_scope_key,
        &task_command_id,
    );
    register_daemon_upgrade_task(task_key.clone(), cancel_token.clone());
    let task_key_for_thread = task_key.clone();
    let spawn_result = std::thread::Builder::new()
        .name("awiki-daemon-upgrade-control".to_string())
        .spawn(move || {
            let mut restart_scheduled = false;
            let result = upgrade_daemon_with_progress_and_cancel(
                &task_config,
                request,
                cancel_token,
                |progress| {
                    if progress.stage == "restarting" && !restart_scheduled {
                        restart_scheduled = true;
                        let restart_target_version = task_target_version.clone();
                        let _ = task_state.mark_control_command_state_if_status_in(
                            &task_daemon_agent.agent_did,
                            &task_daemon_agent.controller_scope_key,
                            &task_command_id,
                            &["in_progress"],
                            "restart_scheduled",
                            json!({
                                "command": DAEMON_UPGRADE,
                                "daemon_agent_did": task_daemon_agent.agent_did,
                                "target_version": restart_target_version,
                                "status": "restart_scheduled",
                            }),
                            None,
                        );
                    }
                    let _ = send_daemon_upgrade_progress(
                        &task_outbox,
                        &task_daemon_agent,
                        &task_message,
                        &task_command_id,
                        &task_target_version,
                        &progress,
                    );
                },
            );
            unregister_daemon_upgrade_task(&task_key_for_thread);
            finish_daemon_upgrade_task(
                &task_state,
                &task_outbox,
                &task_daemon_agent,
                &task_message,
                &task_command_id,
                &task_target_version,
                result,
            );
        });
    if let Err(error) = spawn_result {
        let error = anyhow::Error::new(error).context("spawn daemon upgrade control task");
        finish_daemon_upgrade_spawn_failure(
            state,
            outbox,
            daemon_agent,
            message,
            &payload.command_id,
            &target_version,
            &task_key,
            &error,
        )?;
        return Err(error);
    }
    Ok(())
}

fn finish_daemon_upgrade_spawn_failure<O>(
    state: &DaemonState,
    outbox: &O,
    daemon_agent: &AgentDefinition,
    message: &IncomingAgentPayloadMessage,
    command_id: &str,
    target_version: &str,
    task_key: &DaemonUpgradeTaskKey,
    error: &anyhow::Error,
) -> Result<()>
where
    O: AgentManagementOutbox,
{
    unregister_daemon_upgrade_task(task_key);
    let summary = sanitize_public_error_chain(error.chain());
    let result = json!({
        "command": DAEMON_UPGRADE,
        "daemon_agent_did": daemon_agent.agent_did,
        "target_version": target_version,
        "status": "failed",
        "error_code": "upgrade_task_start_failed",
        "last_error_summary": summary,
    });
    state.mark_control_command_state(
        &daemon_agent.agent_did,
        &daemon_agent.controller_scope_key,
        command_id,
        "failed",
        result.clone(),
        Some(&summary),
    )?;
    send_command_status(
        outbox,
        daemon_agent,
        message,
        command_id,
        "failed",
        Some("daemon upgrade failed".to_string()),
        result,
    )
}

fn finish_daemon_upgrade_task<O>(
    state: &DaemonState,
    outbox: &O,
    daemon_agent: &AgentDefinition,
    message: &IncomingAgentPayloadMessage,
    command_id: &str,
    target_version: &str,
    result: Result<crate::upgrade::DaemonUpgradeReport>,
) where
    O: AgentManagementOutbox,
{
    match result {
        Ok(report) => {
            let result = json!({
                "command": DAEMON_UPGRADE,
                "daemon_agent_did": daemon_agent.agent_did,
                "status": "ready",
                "previous_version": report.previous_version,
                "version": report.target_version,
                "min_supported_version": report.min_supported_version,
                "package_sha256": report.package_sha256,
                "manifest_url": report.manifest_url,
                "download_base_url": report.download_base_url,
                "download_route": report.download_route,
                "service": service_label(report.service.platform),
                "service_running": report.service.running,
                "restarted": report.restarted,
            });
            state
                .mark_control_command_state(
                    &daemon_agent.agent_did,
                    &daemon_agent.controller_scope_key,
                    command_id,
                    "succeeded",
                    result.clone(),
                    None,
                )
                .ok();
            let _ = send_command_status(
                outbox,
                daemon_agent,
                message,
                command_id,
                "ready",
                Some("daemon upgrade completed".to_string()),
                result,
            );
        }
        Err(error) => {
            let summary = sanitize_public_error_chain(error.chain());
            let was_cancelled = summary.contains(DAEMON_UPGRADE_CANCELLED_ERROR);
            let result = json!({
                "command": DAEMON_UPGRADE,
                "daemon_agent_did": daemon_agent.agent_did,
                "target_version": target_version,
                "status": if was_cancelled { "cancelled" } else { "failed" },
                "error_code": if was_cancelled { "upgrade_cancelled" } else { "upgrade_failed" },
                "last_error_summary": summary,
            });
            state
                .mark_control_command_state(
                    &daemon_agent.agent_did,
                    &daemon_agent.controller_scope_key,
                    command_id,
                    if was_cancelled { "cancelled" } else { "failed" },
                    result.clone(),
                    if was_cancelled { None } else { Some(&summary) },
                )
                .ok();
            let _ = send_command_status(
                outbox,
                daemon_agent,
                message,
                command_id,
                if was_cancelled { "cancelled" } else { "failed" },
                Some(if was_cancelled {
                    "daemon upgrade cancelled".to_string()
                } else {
                    "daemon upgrade failed".to_string()
                }),
                result,
            );
        }
    }
}

fn handle_daemon_upgrade_cancel<O>(
    config: &DaemonConfig,
    state: &DaemonState,
    outbox: &O,
    daemon_agent: &AgentDefinition,
    message: &IncomingAgentPayloadMessage,
    payload: &AgentCommandEnvelope,
) -> Result<()>
where
    O: AgentManagementOutbox,
{
    let target_daemon = optional_arg_string(&payload.args, "daemon_agent_did")
        .or_else(|| optional_arg_string(&payload.args, "target_daemon_agent_did"));
    if target_daemon
        .as_deref()
        .is_some_and(|target| target != daemon_agent.agent_did)
    {
        return send_command_status(
            outbox,
            daemon_agent,
            message,
            &payload.command_id,
            "failed",
            Some("daemon.upgrade.cancel can only target this daemon".to_string()),
            json!({
                "command": DAEMON_UPGRADE_CANCEL,
                "daemon_agent_did": daemon_agent.agent_did,
                "error_code": "daemon_target_mismatch",
            }),
        );
    }

    let upgrade_command_id = optional_arg_string(&payload.args, "upgrade_command_id")
        .or_else(|| optional_arg_string(&payload.args, "daemon_upgrade_command_id"))
        .or_else(|| optional_arg_string(&payload.args, "target_command_id"));
    crate::agent_status::reconcile_daemon_upgrade_state_from_release_status(
        config,
        state,
        daemon_agent,
    )?;
    let upgrade = if let Some(command_id) = upgrade_command_id.as_deref() {
        match state.load_control_command_state(
            &daemon_agent.agent_did,
            &daemon_agent.controller_scope_key,
            command_id,
        )? {
            Some(record) if record.command == DAEMON_UPGRADE => Some(record),
            _ => None,
        }
    } else {
        state.load_latest_control_command_state(
            &daemon_agent.agent_did,
            &daemon_agent.controller_scope_key,
            DAEMON_UPGRADE,
            &["in_progress", "restart_scheduled"],
        )?
    };
    let Some(upgrade) = upgrade else {
        return send_command_status(
            outbox,
            daemon_agent,
            message,
            &payload.command_id,
            "ready",
            Some("no daemon upgrade is running".to_string()),
            json!({
                "command": DAEMON_UPGRADE_CANCEL,
                "daemon_agent_did": daemon_agent.agent_did,
                "status": "not_running",
                "upgrade_command_id": upgrade_command_id,
            }),
        );
    };
    if upgrade.status == "restart_scheduled" {
        return send_command_status(
            outbox,
            daemon_agent,
            message,
            &payload.command_id,
            "failed",
            Some("daemon upgrade is already restarting and cannot be cancelled".to_string()),
            json!({
                "command": DAEMON_UPGRADE_CANCEL,
                "daemon_agent_did": daemon_agent.agent_did,
                "status": "not_cancellable",
                "error_code": "upgrade_not_cancellable",
                "upgrade_command_id": upgrade.command_id,
            }),
        );
    }

    let task_key = DaemonUpgradeTaskKey::new(
        &daemon_agent.agent_did,
        &daemon_agent.controller_scope_key,
        &upgrade.command_id,
    );
    if !cancel_daemon_upgrade_task(&task_key) {
        return send_command_status(
            outbox,
            daemon_agent,
            message,
            &payload.command_id,
            "failed",
            Some("daemon upgrade is not cancellable in this process".to_string()),
            json!({
                "command": DAEMON_UPGRADE_CANCEL,
                "daemon_agent_did": daemon_agent.agent_did,
                "status": "not_cancellable",
                "error_code": "upgrade_cancel_unavailable",
                "upgrade_command_id": upgrade.command_id,
            }),
        );
    }

    send_command_status(
        outbox,
        daemon_agent,
        message,
        &payload.command_id,
        "cancel_requested",
        Some("daemon upgrade cancellation requested".to_string()),
        json!({
            "command": DAEMON_UPGRADE_CANCEL,
            "daemon_agent_did": daemon_agent.agent_did,
            "status": "cancel_requested",
            "upgrade_command_id": upgrade.command_id,
        }),
    )
}

fn send_daemon_upgrade_progress<O>(
    outbox: &O,
    daemon_agent: &AgentDefinition,
    message: &IncomingAgentPayloadMessage,
    command_id: &str,
    target_version: &str,
    progress: &DaemonUpgradeProgress,
) -> Result<()>
where
    O: AgentManagementOutbox,
{
    send_command_status(
        outbox,
        daemon_agent,
        message,
        command_id,
        "upgrading",
        Some(progress.message.clone()),
        json!({
            "command": DAEMON_UPGRADE,
            "daemon_agent_did": daemon_agent.agent_did,
            "target_version": progress.target_version.as_deref().unwrap_or(target_version),
            "status": "in_progress",
            "progress": progress,
        }),
    )
}

fn duplicate_upgrade_status_message(status: &str) -> String {
    match status {
        "in_progress" => "daemon upgrade is already in progress",
        "restart_scheduled" => "daemon upgrade is waiting for service restart confirmation",
        "succeeded" => "daemon upgrade already completed",
        "failed" => "daemon upgrade already failed",
        _ => "daemon upgrade command already processed",
    }
    .to_string()
}

fn duplicate_upgrade_public_state(status: &str) -> &'static str {
    match status {
        "in_progress" | "restart_scheduled" => "upgrading",
        "failed" => "failed",
        _ => "ready",
    }
}

fn handle_runtime_agent_delete<C, O>(
    config: &DaemonConfig,
    outbox: &O,
    state: &DaemonState,
    registration_client: &C,
    daemon_agent: &AgentDefinition,
    message: &IncomingAgentPayloadMessage,
    payload: &AgentCommandEnvelope,
) -> Result<()>
where
    C: AgentInventoryClient,
    O: AgentManagementOutbox,
{
    let runtime_agent_did = required_arg_string(&payload.args, "runtime_agent_did")?;
    let runtime_agent = match load_owned_runtime_agent(state, daemon_agent, &runtime_agent_did) {
        Ok(runtime_agent) => runtime_agent,
        Err(_) => {
            return send_command_status(
                outbox,
                daemon_agent,
                message,
                &payload.command_id,
                "failed",
                Some("runtime agent does not belong to this daemon".to_string()),
                json!({
                    "command": RUNTIME_AGENT_DELETE,
                    "runtime_agent_did": runtime_agent_did,
                    "daemon_agent_did": daemon_agent.agent_did,
                    "error_code": "runtime_not_owned",
                }),
            );
        }
    };
    send_command_status(
        outbox,
        daemon_agent,
        message,
        &payload.command_id,
        "archiving",
        Some("runtime agent archive started".to_string()),
        json!({
            "command": RUNTIME_AGENT_DELETE,
            "runtime_agent_did": runtime_agent.agent_did,
            "daemon_agent_did": daemon_agent.agent_did,
        }),
    )?;
    let result =
        crate::archive::archive_runtime_agent(config, state, &runtime_agent).and_then(|report| {
            archive_agent_remote(
                registration_client,
                config,
                state,
                daemon_agent,
                &runtime_agent.agent_did,
            )
            .map(|_| report)
        });
    match result {
        Ok(report) => send_command_status(
            outbox,
            daemon_agent,
            message,
            &payload.command_id,
            "archived",
            Some("runtime agent archived".to_string()),
            json!({
                "command": RUNTIME_AGENT_DELETE,
                "runtime_agent_did": runtime_agent.agent_did,
                "daemon_agent_did": daemon_agent.agent_did,
                "archive_id": report.archive_id,
                "moved_path_count": report.moved_paths.len(),
                "skipped_path_count": report.skipped_paths.len(),
            }),
        ),
        Err(error) => send_command_status(
            outbox,
            daemon_agent,
            message,
            &payload.command_id,
            "failed",
            Some("runtime agent archive failed".to_string()),
            json!({
                "command": RUNTIME_AGENT_DELETE,
                "runtime_agent_did": runtime_agent.agent_did,
                "daemon_agent_did": daemon_agent.agent_did,
                "error_code": "archive_failed",
                "last_error_summary": sanitize_public_error(&error.to_string()),
            }),
        ),
    }
}

fn handle_daemon_delete<C, O>(
    config: &DaemonConfig,
    outbox: &O,
    state: &DaemonState,
    registration_client: &C,
    daemon_agent: &AgentDefinition,
    message: &IncomingAgentPayloadMessage,
    payload: &AgentCommandEnvelope,
) -> Result<()>
where
    C: AgentInventoryClient,
    O: AgentManagementOutbox,
{
    let target_daemon = optional_arg_string(&payload.args, "daemon_agent_did")
        .or_else(|| optional_arg_string(&payload.args, "target_daemon_agent_did"));
    if target_daemon
        .as_deref()
        .is_some_and(|target| target != daemon_agent.agent_did)
    {
        return send_command_status(
            outbox,
            daemon_agent,
            message,
            &payload.command_id,
            "failed",
            Some("daemon.delete can only target this daemon".to_string()),
            json!({
                "command": DAEMON_DELETE,
                "daemon_agent_did": daemon_agent.agent_did,
                "error_code": "daemon_target_mismatch",
            }),
        );
    }
    send_command_status(
        outbox,
        daemon_agent,
        message,
        &payload.command_id,
        "archiving",
        Some("daemon archive started".to_string()),
        json!({
            "command": DAEMON_DELETE,
            "daemon_agent_did": daemon_agent.agent_did,
        }),
    )?;
    let runtimes = state.list_runtime_agent_definitions_for_daemon(&daemon_agent.agent_did)?;
    let result = crate::archive::prepare_daemon_archive(config, state, daemon_agent, &runtimes)
        .and_then(|report| {
            archive_agent_remote(
                registration_client,
                config,
                state,
                daemon_agent,
                &daemon_agent.agent_did,
            )
            .map(|_| report)
        });
    match result {
        Ok(mut report) => {
            send_command_status(
                outbox,
                daemon_agent,
                message,
                &payload.command_id,
                "archived",
                Some("daemon archived".to_string()),
                json!({
                    "command": DAEMON_DELETE,
                    "daemon_agent_did": daemon_agent.agent_did,
                    "archive_id": report.archive_id,
                    "runtime_agent_count": runtimes.len(),
                }),
            )?;
            crate::archive::schedule_daemon_archive_finalizer(
                config.clone(),
                report.archive_id.clone(),
                std::time::Duration::from_millis(1500),
            )?;
            report.finalizer_scheduled = true;
            Ok(())
        }
        Err(error) => send_command_status(
            outbox,
            daemon_agent,
            message,
            &payload.command_id,
            "failed",
            Some("daemon archive failed".to_string()),
            json!({
                "command": DAEMON_DELETE,
                "daemon_agent_did": daemon_agent.agent_did,
                "error_code": "archive_failed",
                "last_error_summary": sanitize_public_error(&error.to_string()),
            }),
        ),
    }
}

fn archive_agent_remote<C>(
    registration_client: &C,
    config: &DaemonConfig,
    state: &DaemonState,
    daemon_agent: &AgentDefinition,
    agent_did: &str,
) -> Result<Value>
where
    C: AgentInventoryClient,
{
    let auth_paths =
        crate::im_core_adapter::agent_identity_auth_paths(config, &daemon_agent.agent_did);
    let auth = DidAuthMaterial {
        did_document_path: auth_paths.0,
        private_key_path: auth_paths.1,
        bearer_token: state.load_agent_auth_token(&daemon_agent.agent_did)?,
    };
    registration_client.archive_agent(&daemon_agent.agent_did, agent_did, &auth)
}

fn service_label(platform: crate::service::ServicePlatform) -> &'static str {
    match platform {
        crate::service::ServicePlatform::LaunchAgent => "launch_agent",
        crate::service::ServicePlatform::SystemdUser => "systemd_user",
        crate::service::ServicePlatform::Foreground => "foreground",
        crate::service::ServicePlatform::Unsupported => "unsupported",
    }
}

fn load_owned_runtime_agent(
    state: &DaemonState,
    daemon_agent: &AgentDefinition,
    runtime_agent_did: &str,
) -> Result<AgentDefinition> {
    let runtime_agent = state.load_active_agent_definition(runtime_agent_did)?;
    if runtime_agent.agent_kind != AgentKind::Runtime
        || runtime_agent.controller_scope_key != daemon_agent.controller_scope_key
        || !state.runtime_agent_belongs_to_daemon_scope(
            &runtime_agent.agent_did,
            &daemon_agent.agent_did,
            &daemon_agent.controller_scope_key,
        )?
    {
        bail!("runtime agent does not belong to this daemon");
    }
    Ok(runtime_agent)
}

fn required_arg_string(args: &Value, field: &str) -> Result<String> {
    args.get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .with_context(|| format!("agent command args.{field} is required"))
}

fn optional_arg_string(args: &Value, field: &str) -> Option<String> {
    args.get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn optional_arg_u64(args: &Value, field: &str) -> Option<u64> {
    args.get(field).and_then(|value| {
        value.as_u64().or_else(|| {
            value
                .as_str()
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .and_then(|text| text.parse::<u64>().ok())
        })
    })
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
    let runs = top_level_runs_for_result(&result);
    let payload = json!({
        "schema": AGENT_STATUS_SCHEMA,
        "event_id": format!("evt_{}", crate::security::runtime_token::current_time_millis().unwrap_or(0)),
        "sent_at": rfc3339_now(),
        "daemon_agent_did": daemon_agent.agent_did,
        "status_scope": status_scope_for_result(&result),
        "command_id": command_id,
        "state": state,
        "message": text,
        "daemon": result.get("daemon").cloned().unwrap_or_else(|| json!({})),
        "runtimes": result.get("runtimes").cloned().unwrap_or_else(|| json!([])),
        "runs": runs,
        "details": result.clone(),
        "result": result,
    });
    outbox.send_agent_status(&AgentStatusResponse {
        conversation_id: message.conversation_id.clone(),
        agent_did: daemon_agent.agent_did.clone(),
        recipient_did: message.sender_did.clone(),
        payload,
    })
}

fn send_scoped_command_status<O>(
    outbox: &O,
    daemon_agent: &AgentDefinition,
    message: &IncomingAgentPayloadMessage,
    envelope: &AgentCommandEnvelope,
    state: &str,
    text: Option<String>,
    status_scope: &str,
    details: Value,
) -> Result<()>
where
    O: AgentManagementOutbox,
{
    let result = details
        .get("result")
        .cloned()
        .unwrap_or_else(|| details.clone());
    let payload = json!({
        "schema": AGENT_STATUS_SCHEMA,
        "event_id": format!("evt_{}", crate::security::runtime_token::current_time_millis().unwrap_or(0)),
        "sent_at": rfc3339_now(),
        "daemon_agent_did": daemon_agent.agent_did,
        "runtime_agent_did": details.get("runtime_agent_did").cloned().unwrap_or_else(|| json!(null)),
        "status_scope": status_scope,
        "command": envelope.command,
        "command_id": envelope.command_id,
        "request_id": envelope.command_id,
        "state": state,
        "message": text,
        "details": details,
        "result": result,
    });
    outbox.send_agent_status(&AgentStatusResponse {
        conversation_id: message.conversation_id.clone(),
        agent_did: daemon_agent.agent_did.clone(),
        recipient_did: message.sender_did.clone(),
        payload,
    })
}

fn send_runtime_inbox_failure<O>(
    outbox: &O,
    daemon_agent: &AgentDefinition,
    message: &IncomingAgentPayloadMessage,
    envelope: &AgentCommandEnvelope,
    command: &str,
    error_code: &str,
    text: &str,
    status_scope: &str,
) -> Result<()>
where
    O: AgentManagementOutbox,
{
    send_scoped_command_status(
        outbox,
        daemon_agent,
        message,
        envelope,
        "failed",
        Some(text.to_string()),
        status_scope,
        json!({
            "command": command,
            "error_code": error_code,
        }),
    )
}

fn send_runtime_inbox_failure_with_runtime<O>(
    outbox: &O,
    daemon_agent: &AgentDefinition,
    message: &IncomingAgentPayloadMessage,
    envelope: &AgentCommandEnvelope,
    command: &str,
    runtime_agent_did: &str,
    error_code: &str,
    text: &str,
    status_scope: &str,
) -> Result<()>
where
    O: AgentManagementOutbox,
{
    send_scoped_command_status(
        outbox,
        daemon_agent,
        message,
        envelope,
        "failed",
        Some(text.to_string()),
        status_scope,
        json!({
            "command": command,
            "runtime_agent_did": runtime_agent_did,
            "error_code": error_code,
        }),
    )
}

fn scope_name(scope: RuntimeInboxScope) -> &'static str {
    match scope {
        RuntimeInboxScope::All => "all",
        RuntimeInboxScope::Direct => "direct",
        RuntimeInboxScope::Group => "group",
    }
}

fn kind_name(kind: RuntimeInboxThreadKind) -> &'static str {
    match kind {
        RuntimeInboxThreadKind::Direct => "direct",
        RuntimeInboxThreadKind::Group => "group",
    }
}

fn top_level_runs_for_result(result: &Value) -> Value {
    if let Some(runs) = result.get("runs") {
        return runs.clone();
    }
    if result.get("run_id").is_none()
        || result.get("message_id").is_none()
        || result.get("runtime_agent_did").is_none()
        || (result.get("retry_status").is_none() && result.get("status").is_none())
    {
        return json!([]);
    }
    let status = result
        .get("retry_status")
        .or_else(|| result.get("status"))
        .and_then(Value::as_str)
        .unwrap_or("running");
    let run_id = result
        .get("retry_run_id")
        .or_else(|| result.get("run_id"))
        .cloned()
        .unwrap_or_else(|| json!(""));
    json!([{
        "run_id": run_id,
        "message_id": result.get("message_id").cloned().unwrap_or_else(|| json!("")),
        "runtime_agent_did": result.get("runtime_agent_did").cloned().unwrap_or_else(|| json!("")),
        "conversation_id": result.get("conversation_id").cloned().unwrap_or_else(|| json!(null)),
        "status": normalize_run_status(status),
        "started_at": result.get("started_at").cloned().unwrap_or_else(|| json!(null)),
        "updated_at": result.get("updated_at").cloned().unwrap_or_else(|| json!(null)),
        "last_error_code": result.get("last_error_code").cloned().unwrap_or_else(|| json!(null)),
        "last_error_summary": result.get("last_error_summary").cloned().unwrap_or_else(|| json!(null)),
    }])
}

fn normalize_run_status(status: &str) -> &'static str {
    match status {
        "queued" | "pending" => "queued",
        "running" => "running",
        "succeeded" | "finished" => "succeeded",
        "failed" => "failed",
        _ => "running",
    }
}

fn status_scope_for_result(result: &Value) -> &'static str {
    if result.get("runtimes").is_some() {
        return "snapshot";
    }
    if result.get("run_id").is_some() {
        return "run";
    }
    if result.get("runtime_agent_did").is_some() {
        return "runtime";
    }
    "daemon"
}

fn rfc3339_now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

fn rfc3339_from_millis(ms: i64) -> String {
    let seconds = ms.div_euclid(1000);
    let nanos = (ms.rem_euclid(1000) * 1_000_000) as i32;
    let Ok(value) = time::OffsetDateTime::from_unix_timestamp(seconds) else {
        return rfc3339_now();
    };
    let Ok(value) = value.replace_nanosecond(nanos as u32) else {
        return rfc3339_now();
    };
    value
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| rfc3339_now())
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::outbox::MemoryRuntimeOutbox;

    fn daemon_definition_fixture() -> AgentDefinition {
        AgentDefinition {
            agent_did: "did:agent:daemon".to_string(),
            handle: "alice-daemon".to_string(),
            agent_kind: AgentKind::Daemon,
            controller_user_id: "user-alice".to_string(),
            controller_full_handle: "alice.anpclaw.com".to_string(),
            controller_scope_key: "controller-scope:v1:user-alice:alice-anpclaw-com".to_string(),
            controller_did: "did:human:alice".to_string(),
            runtime_plugin_id: None,
            runtime_profile_id: None,
            workspace_id: None,
            policy_id: "default".to_string(),
            local_agent_db_path: "agent-local.sqlite".to_string(),
            message_db_path: "agent-message.sqlite".to_string(),
            status: "active".to_string(),
        }
    }

    #[test]
    fn daemon_upgrade_spawn_failure_marks_command_failed_and_clears_cancel_token() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        let state = DaemonState::open(&config).unwrap();
        state.initialize().unwrap();
        let daemon = daemon_definition_fixture();
        state.upsert_agent_definition(&daemon).unwrap();
        state
            .try_begin_control_command(
                &daemon.agent_did,
                &daemon.controller_scope_key,
                "cmd_upgrade_spawn_failed",
                DAEMON_UPGRADE,
                "msg_upgrade_spawn_failed",
                Some("latest"),
            )
            .unwrap();
        let task_key = DaemonUpgradeTaskKey::new(
            &daemon.agent_did,
            &daemon.controller_scope_key,
            "cmd_upgrade_spawn_failed",
        );
        register_daemon_upgrade_task(task_key.clone(), DaemonUpgradeCancelToken::default());
        let outbox = MemoryRuntimeOutbox::default();
        let message = IncomingAgentPayloadMessage {
            message_id: "msg_upgrade_spawn_failed".to_string(),
            conversation_id: Some("conv_upgrade".to_string()),
            sender_did: daemon.controller_did.clone(),
            target_agent_did: daemon.agent_did.clone(),
            content_type: "application/json".to_string(),
            payload: json!({}),
        };
        let error =
            anyhow::anyhow!("thread limit reached").context("spawn daemon upgrade control task");

        finish_daemon_upgrade_spawn_failure(
            &state,
            &outbox,
            &daemon,
            &message,
            "cmd_upgrade_spawn_failed",
            "latest",
            &task_key,
            &error,
        )
        .unwrap();

        let stored = state
            .load_control_command_state(
                &daemon.agent_did,
                &daemon.controller_scope_key,
                "cmd_upgrade_spawn_failed",
            )
            .unwrap()
            .unwrap();
        assert_eq!(stored.status, "failed");
        assert_eq!(
            stored.result_json["error_code"],
            "upgrade_task_start_failed"
        );
        assert!(stored
            .error_summary
            .as_deref()
            .is_some_and(|summary| summary.contains("spawn daemon upgrade control task")));
        assert!(!cancel_daemon_upgrade_task(&task_key));

        let statuses = outbox.agent_statuses();
        assert_eq!(statuses.len(), 1);
        let payload = &statuses[0].payload;
        assert_eq!(payload["command_id"], "cmd_upgrade_spawn_failed");
        assert_eq!(payload["state"], "failed");
        assert_eq!(
            payload["details"]["error_code"],
            "upgrade_task_start_failed"
        );
        assert_eq!(statuses[0].recipient_did, daemon.controller_did);
    }

    #[test]
    fn public_error_chain_keeps_upgrade_download_root_cause() {
        let error = anyhow::anyhow!("request timed out")
            .context("download daemon package https://anpclaw.com/daemon/releases/0.1.39/awiki-deamon-darwin-arm64.tar.gz");

        let summary = sanitize_public_error_chain(error.chain());

        assert!(summary.contains("download daemon package"));
        assert!(summary.contains("request timed out"));
    }
}
