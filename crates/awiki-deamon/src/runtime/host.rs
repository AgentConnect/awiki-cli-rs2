use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::Value;

use crate::inbox::{route_controller_text_task, ControllerTextMessage};
use crate::local_rpc::execute_runtime_rpc_request_with_outbox;
use crate::outbox::RuntimeOutbox;
use crate::runtime::{
    RuntimeAgentProfile, RuntimeLaunchContext, RuntimeLaunchOutcome, RuntimePlugin, RuntimeRun,
    RuntimeRunStatus,
};
use crate::security::runtime_token::{issue_runtime_token, RpcMethod, RuntimeTokenScope};
use crate::state::DaemonState;

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

    let launch_context = RuntimeLaunchContext {
        run: run.clone(),
        task,
        workspace_root: profile.workspace_root.clone(),
        runtime_rpc_token: issued.token.clone(),
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

    if launch_outcome.status == RuntimeRunStatus::Failed {
        state.update_runtime_run_status(&run.run_id, RuntimeRunStatus::Failed)?;
    }

    Ok(RuntimeTaskRunResult {
        run: state.load_runtime_run(&run.run_id)?,
        launch_outcome,
        token_id: issued.token_id,
    })
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
