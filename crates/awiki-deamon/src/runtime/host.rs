use std::time::Duration;

use anyhow::{Context, Result};

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

    let scope = RuntimeTokenScope::new(
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
        Some(vec![profile.controller_did.clone()]),
        Duration::from_secs(5 * 60),
    )?;
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
