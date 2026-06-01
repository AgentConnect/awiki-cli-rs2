use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::Value;

use crate::cli_wrapper::CliWrapperRequest;
use crate::inbox::{route_controller_text_task, ControllerTextMessage};
use crate::local_rpc::execute_runtime_rpc_request_with_outbox;
use crate::outbox::RuntimeOutbox;
use crate::runtime::{
    RuntimeAgentProfile, RuntimeLaunchContext, RuntimeLaunchOutcome, RuntimePlugin, RuntimeRun,
    RuntimeRunStatus,
};
use crate::security::runtime_token::{issue_runtime_token, RpcMethod, RuntimeTokenScope};
use crate::state::{CliDriverRunRecord, DaemonState};
use crate::workspace::{prepare_workspace_instance, WorkspaceBindingConfig, WorkspaceInstance};
use crate::DaemonConfig;

#[derive(Debug, Clone, PartialEq, Eq)]
struct RecipientPolicy {
    allowed_recipients: Vec<String>,
    allowed_message_security: Vec<String>,
}

impl RecipientPolicy {
    fn controller_only(controller_did: &str) -> Self {
        Self {
            allowed_recipients: vec![controller_did.to_string()],
            allowed_message_security: vec!["default_plain".to_string(), "direct_e2ee".to_string()],
        }
    }

    fn from_json(value: &Value, controller_did: &str) -> Result<Self> {
        let Some(object) = value.as_object() else {
            anyhow::bail!("recipient_policy_json must be a JSON object");
        };
        let allow_controller = object
            .get("allow_controller")
            .and_then(Value::as_bool)
            .or_else(|| {
                object
                    .get("mode")
                    .and_then(Value::as_str)
                    .map(|mode| mode == "controller-only")
            })
            .unwrap_or(false);
        let mut allowed_recipients = Vec::new();
        if allow_controller {
            allowed_recipients.push(controller_did.to_string());
        }
        collect_string_array(object.get("allowed_dids"), &mut allowed_recipients)?;
        collect_string_array(object.get("allowed_handles"), &mut allowed_recipients)?;
        collect_string_array(object.get("allow"), &mut allowed_recipients)?;
        let mut allowed_message_security = Vec::new();
        collect_string_array(
            object.get("allowed_security"),
            &mut allowed_message_security,
        )?;
        if allowed_message_security.is_empty() {
            allowed_message_security.push("default_plain".to_string());
            allowed_message_security.push("direct_e2ee".to_string());
        }
        if allowed_recipients.is_empty() {
            anyhow::bail!("recipient policy must allow at least one recipient");
        }
        Ok(Self {
            allowed_recipients,
            allowed_message_security,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeTaskRunResult {
    pub run: RuntimeRun,
    pub launch_outcome: RuntimeLaunchOutcome,
    pub token_id: String,
}

pub fn run_controller_text_task<P, O>(
    state: &DaemonState,
    profile: &RuntimeAgentProfile,
    plugin: &P,
    outbox: &O,
    message: ControllerTextMessage,
) -> Result<RuntimeTaskRunResult>
where
    P: RuntimePlugin,
    O: RuntimeOutbox,
{
    run_controller_text_task_with_socket(state, profile, plugin, outbox, message, None, None)
}

pub fn run_controller_text_task_with_config<P, O>(
    config: &DaemonConfig,
    state: &DaemonState,
    profile: &RuntimeAgentProfile,
    plugin: &P,
    outbox: &O,
    message: ControllerTextMessage,
) -> Result<RuntimeTaskRunResult>
where
    P: RuntimePlugin,
    O: RuntimeOutbox,
{
    run_controller_text_task_with_socket(
        state,
        profile,
        plugin,
        outbox,
        message,
        Some(config.local_socket_path.clone()),
        Some(config.runtime_temp_dir.clone()),
    )
}

fn run_controller_text_task_with_socket<P, O>(
    state: &DaemonState,
    profile: &RuntimeAgentProfile,
    plugin: &P,
    outbox: &O,
    message: ControllerTextMessage,
    local_socket_path: Option<std::path::PathBuf>,
    runtime_temp_dir: Option<std::path::PathBuf>,
) -> Result<RuntimeTaskRunResult>
where
    P: RuntimePlugin,
    O: RuntimeOutbox,
{
    profile.validate()?;
    let task = route_controller_text_task(profile, message)?;
    state.upsert_runtime_agent_profile(profile)?;
    state.insert_runtime_task(&task)?;

    let run_id = format!("run_{}", task.task_id);
    let run = RuntimeRun {
        run_id,
        task_id: task.task_id.clone(),
        agent_did: profile.agent_did.clone(),
        runtime_profile_id: profile.runtime_profile_id.clone(),
        runtime_plugin_id: profile.runtime_plugin_id.clone(),
        workspace_id: profile.workspace_id.clone(),
        status: RuntimeRunStatus::Pending,
    };
    state.insert_runtime_run(&run)?;

    let recipient_policy = runtime_recipient_policy(state, profile)?;
    let mut scope = RuntimeTokenScope::new(
        profile.agent_did.clone(),
        profile.runtime_profile_id.clone(),
        run.run_id.clone(),
        vec![
            RpcMethod::RpcPing,
            RpcMethod::TaskStatus,
            RpcMethod::TaskFinish,
            RpcMethod::MsgSend,
            RpcMethod::ArtifactCreated,
        ],
        Some(recipient_policy.allowed_recipients),
        Duration::from_secs(5 * 60),
    )?;
    scope.allowed_message_security = Some(recipient_policy.allowed_message_security);
    let issued = issue_runtime_token(scope)?;
    state.store_runtime_token(&issued)?;

    let install_status = plugin.check_install_status()?;
    if !install_status.installed {
        state.update_runtime_run_status(&run.run_id, RuntimeRunStatus::Failed)?;
        anyhow::bail!("runtime plugin {} is not installed", plugin.plugin_id());
    }
    let workspace_instance = match (
        profile.workspace_id.as_ref(),
        profile.workspace_root.as_ref(),
        profile.workspace_mode,
        local_socket_path.is_some(),
        plugin.plugin_id() == crate::agent::GENERIC_CLI_RUNTIME_PLUGIN_ID,
    ) {
        (Some(workspace_id), Some(workspace_root), Some(workspace_mode), true, true) => Some(
            prepare_workspace_instance(
                runtime_temp_dir
                    .as_ref()
                    .context("runtime temp dir is unavailable without daemon config")?,
                &WorkspaceBindingConfig {
                    workspace_id: workspace_id.clone(),
                    workspace_root: workspace_root.clone(),
                    workspace_mode,
                },
                &run.run_id,
            )
            .context("prepare runtime workspace instance")?,
        ),
        _ => None,
    };

    let launch_context = RuntimeLaunchContext {
        run: run.clone(),
        task,
        workspace_root: profile.workspace_root.clone(),
        workspace_instance: workspace_instance.clone(),
        runtime_temp_dir,
        runtime_rpc_token: issued.token.clone(),
        local_socket_path,
    };
    let launch_outcome = match plugin.launch_run(launch_context) {
        Ok(outcome) => outcome,
        Err(error) => {
            state.update_runtime_run_status(&run.run_id, RuntimeRunStatus::Failed)?;
            return Err(error).context("launch runtime run");
        }
    };

    for callback in launch_outcome.callbacks.iter().cloned() {
        execute_runtime_rpc_request_with_outbox(state, outbox, callback)
            .context("apply runtime callback")?;
    }

    let mut fallback_final_source = None;
    if plugin.plugin_id() == crate::agent::GENERIC_CLI_RUNTIME_PLUGIN_ID
        && launch_outcome.status == RuntimeRunStatus::Finished
        && state.load_runtime_run(&run.run_id)?.status != RuntimeRunStatus::Finished
    {
        if let Some(final_text) = fallback_final_text(&launch_outcome.metadata)? {
            let response = execute_runtime_rpc_request_with_outbox(
                state,
                outbox,
                CliWrapperRequest::task_finish(
                    issued.token.as_str().to_string(),
                    run.task_id.clone(),
                    final_text,
                )
                .into_rpc_request(),
            )
            .context("apply fallback final from CLI driver output")?;
            if response.ok {
                fallback_final_source = Some("codex_output_last_message".to_string());
            }
        }
    }

    if launch_outcome.status == RuntimeRunStatus::Failed {
        state.update_runtime_run_status(&run.run_id, RuntimeRunStatus::Failed)?;
    }
    if plugin.plugin_id() == crate::agent::GENERIC_CLI_RUNTIME_PLUGIN_ID {
        persist_cli_driver_run(
            state,
            profile,
            &run,
            &launch_outcome,
            workspace_instance.as_ref(),
            fallback_final_source.as_deref(),
        )?;
    }

    Ok(RuntimeTaskRunResult {
        run: state.load_runtime_run(&run.run_id)?,
        launch_outcome,
        token_id: issued.token_id,
    })
}

fn persist_cli_driver_run(
    state: &DaemonState,
    profile: &RuntimeAgentProfile,
    run: &RuntimeRun,
    launch_outcome: &RuntimeLaunchOutcome,
    workspace_instance: Option<&WorkspaceInstance>,
    fallback_final_source: Option<&str>,
) -> Result<()> {
    let task = state.load_runtime_task(&run.task_id)?;
    let Some(driver_id) = state
        .load_cli_runtime_profile(&profile.runtime_profile_id)
        .map(|profile| profile.driver_id)
        .or_else(|_| {
            launch_outcome
                .metadata
                .get("driver_id")
                .and_then(Value::as_str)
                .map(str::to_string)
                .context("generic-cli run metadata does not include driver_id")
        })
        .ok()
    else {
        return Ok(());
    };
    let output_json = launch_outcome
        .metadata
        .get("output")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let command_json = launch_outcome
        .metadata
        .get("command")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    state.upsert_cli_driver_run(&CliDriverRunRecord {
        run_id: run.run_id.clone(),
        agent_did: run.agent_did.clone(),
        runtime_profile_id: run.runtime_profile_id.clone(),
        driver_id,
        controller_did: profile.controller_did.clone(),
        conversation_id: task.conversation_id.clone(),
        route_key: generic_cli_route_key(
            &run.agent_did,
            &profile.controller_did,
            task.conversation_id.as_deref(),
        ),
        workspace_id: profile.workspace_id.clone(),
        workspace_root: workspace_instance
            .map(|instance| instance.workspace_root.clone())
            .or_else(|| canonicalize_optional_path(profile.workspace_root.as_ref())),
        workspace_instance_path: workspace_instance
            .map(|instance| instance.workspace_instance_path.clone())
            .or_else(|| canonicalize_optional_path(profile.workspace_root.as_ref())),
        workspace_mode: workspace_instance
            .map(|instance| instance.workspace_mode)
            .or(profile.workspace_mode),
        is_security_boundary: workspace_instance
            .map(|instance| instance.is_security_boundary)
            .unwrap_or(false),
        command_json,
        output_json,
        final_output_path: launch_outcome
            .metadata
            .get("final_output_path")
            .and_then(Value::as_str)
            .map(std::path::PathBuf::from),
        native_session_id: None,
        synthetic_session_id: Some(generic_cli_route_key(
            &run.agent_did,
            &profile.controller_did,
            task.conversation_id.as_deref(),
        )),
        status: launch_outcome.status.as_str().to_string(),
        fallback_final_source: fallback_final_source.map(str::to_string),
    })?;
    Ok(())
}

fn canonicalize_optional_path(path: Option<&std::path::PathBuf>) -> Option<std::path::PathBuf> {
    path.map(|path| path.canonicalize().unwrap_or_else(|_| path.clone()))
}

fn generic_cli_route_key(
    agent_did: &str,
    controller_did: &str,
    conversation_id: Option<&str>,
) -> String {
    format!(
        "cli:{agent_did}:{controller_did}:{}:message-run",
        conversation_id.unwrap_or("no-conversation")
    )
}

fn fallback_final_text(metadata: &Value) -> Result<Option<String>> {
    let Some(path) = metadata
        .get("final_output_path")
        .and_then(Value::as_str)
        .map(std::path::PathBuf::from)
    else {
        return Ok(None);
    };
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("read fallback final output {}", path.display()))?;
    let text = text.trim();
    if text.is_empty() {
        Ok(None)
    } else {
        Ok(Some(text.to_string()))
    }
}

fn runtime_recipient_policy(
    state: &DaemonState,
    profile: &RuntimeAgentProfile,
) -> Result<RecipientPolicy> {
    match state.load_cli_runtime_profile(&profile.runtime_profile_id) {
        Ok(cli_profile) => {
            RecipientPolicy::from_json(&cli_profile.recipient_policy_json, &profile.controller_did)
        }
        Err(_) => Ok(RecipientPolicy::controller_only(&profile.controller_did)),
    }
}

fn collect_string_array(value: Option<&Value>, output: &mut Vec<String>) -> Result<()> {
    let Some(value) = value else {
        return Ok(());
    };
    let Some(items) = value.as_array() else {
        anyhow::bail!("recipient policy entries must be arrays");
    };
    for item in items {
        let item = item
            .as_str()
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .context("recipient policy entries must be non-empty strings")?;
        output.push(item.to_string());
    }
    Ok(())
}
