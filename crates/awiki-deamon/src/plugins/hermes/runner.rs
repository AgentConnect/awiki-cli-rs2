use anyhow::{bail, Context, Result};

use crate::runtime::{
    RuntimeInstallStatus, RuntimeLaunchContext, RuntimeLaunchOutcome, RuntimePlugin,
    RuntimeRunStatus,
};
use crate::state::{
    DaemonState, HermesNativeSessionRecord, HermesProfileRecord, HermesSessionRoute,
};

use super::gateway::{
    HermesGateway, HermesGatewayLaunchContext, HermesPromptOutcome, HermesPromptSubmitRequest,
    HermesRunnerRef, HermesSessionCreateRequest, HermesSessionRef,
};
use super::prompt::HermesPromptWrapper;
use super::HERMES_RUNTIME_PLUGIN_ID;

#[derive(Debug, Clone)]
pub struct HermesRuntimePlugin<G> {
    gateway: G,
    profile: HermesProfileRecord,
    state: Option<DaemonState>,
}

#[derive(Debug, Clone)]
pub struct HermesRunner<G> {
    gateway: G,
    runner: HermesRunnerRef,
}

impl<G> HermesRuntimePlugin<G> {
    pub fn new(gateway: G, profile: HermesProfileRecord) -> Self {
        Self {
            gateway,
            profile,
            state: None,
        }
    }

    pub fn with_state(gateway: G, profile: HermesProfileRecord, state: DaemonState) -> Self {
        Self {
            gateway,
            profile,
            state: Some(state),
        }
    }
}

impl<G> HermesRunner<G>
where
    G: HermesGateway,
{
    pub fn start(gateway: G, profile: &HermesProfileRecord) -> Result<Self> {
        let runner = gateway.start(profile)?;
        Ok(Self { gateway, runner })
    }

    pub fn start_for_launch(
        gateway: G,
        profile: &HermesProfileRecord,
        context: &HermesGatewayLaunchContext,
    ) -> Result<Self> {
        let runner = gateway.start_for_launch(profile, context)?;
        Ok(Self { gateway, runner })
    }

    pub fn runner_ref(&self) -> &HermesRunnerRef {
        &self.runner
    }

    pub fn create_session(&self, request: HermesSessionCreateRequest) -> Result<HermesSessionRef> {
        self.gateway.create_session(&self.runner, request)
    }

    pub fn submit_prompt(
        &self,
        session: &HermesSessionRef,
        request: HermesPromptSubmitRequest,
    ) -> Result<HermesPromptOutcome> {
        self.gateway.submit_prompt(session, request)
    }
}

impl<G> RuntimePlugin for HermesRuntimePlugin<G>
where
    G: HermesGateway + Clone,
{
    fn plugin_id(&self) -> &str {
        HERMES_RUNTIME_PLUGIN_ID
    }

    fn check_install_status(&self) -> Result<RuntimeInstallStatus> {
        self.gateway.check_installation()
    }

    fn launch_run(&self, context: RuntimeLaunchContext) -> Result<RuntimeLaunchOutcome> {
        if context.run.agent_did != self.profile.agent_did
            || context.run.runtime_profile_id != self.profile.runtime_profile_id
            || context.run.runtime_plugin_id != HERMES_RUNTIME_PLUGIN_ID
            || context.run.task_id != context.task.task_id
            || context.task.agent_did != self.profile.agent_did
        {
            bail!("Hermes launch context does not match profile binding");
        }
        let runner = if let Some(local_socket_path) = context.local_socket_path.as_ref() {
            let launch_context = HermesGatewayLaunchContext::new(
                context.run.run_id.clone(),
                local_socket_path.clone(),
                context.runtime_rpc_token.as_str().to_string(),
            );
            HermesRunner::start_for_launch(self.gateway.clone(), &self.profile, &launch_context)
        } else {
            HermesRunner::start(self.gateway.clone(), &self.profile)
        }
        .context("start Hermes runner")?;
        let route = HermesSessionRoute::new(
            self.profile.agent_did.clone(),
            self.profile.runtime_profile_id.clone(),
            context.task.controller_scope_key.clone(),
            context.task.requester_did.clone(),
            context.task.conversation_id.clone(),
            "conversation",
        );
        let session = if let Some(state) = self.state.as_ref() {
            load_or_create_persisted_session(
                state,
                &runner,
                &self.profile,
                &route,
                &context.task.controller_did,
            )?
        } else {
            runner
                .create_session(HermesSessionCreateRequest {
                    route_key: route.route_key(),
                    conversation_id: context.task.conversation_id.clone(),
                })
                .context("create Hermes session")?
        };
        let prompt = HermesPromptWrapper::new(&self.profile, &context.run, &context.task);
        let request = HermesPromptSubmitRequest {
            run_id: context.run.run_id.clone(),
            message_id: context.task.task_id.clone(),
            prompt: prompt.to_prompt_text(),
        };
        let outcome = match runner.submit_prompt(&session, request.clone()) {
            Ok(outcome) => outcome,
            Err(error)
                if self.state.is_some() && is_missing_hermes_session_error(&error.to_string()) =>
            {
                let state = self
                    .state
                    .as_ref()
                    .expect("state checked above for Hermes session recovery");
                let session = reset_and_create_persisted_session(
                    state,
                    &runner,
                    &self.profile,
                    &route,
                    &context.task.controller_did,
                )
                .context("recover missing Hermes session")?;
                runner
                    .submit_prompt(&session, request)
                    .context("submit Hermes prompt after session recovery")?
            }
            Err(error) => return Err(error).context("submit Hermes prompt"),
        };
        let callbacks = outcome
            .callbacks
            .into_iter()
            .map(|mut callback| {
                callback.runtime_rpc_token = context.runtime_rpc_token.as_str().to_string();
                callback
            })
            .collect();

        Ok(RuntimeLaunchOutcome {
            run_id: context.run.run_id,
            status: RuntimeRunStatus::Running,
            exit_code: None,
            callbacks,
            metadata: serde_json::json!({
                "events": outcome.events,
                "final_text": outcome.final_text,
                "error": outcome.error,
            }),
        })
    }
}

fn load_or_create_persisted_session<G>(
    state: &DaemonState,
    runner: &HermesRunner<G>,
    profile: &HermesProfileRecord,
    route: &HermesSessionRoute,
    controller_did: &str,
) -> Result<super::gateway::HermesSessionRef>
where
    G: HermesGateway,
{
    if let Some(record) = state.load_active_hermes_session_by_route(route)? {
        return Ok(super::gateway::HermesSessionRef {
            runner_id: runner.runner_ref().runner_id.clone(),
            hermes_session_id: record.hermes_session_id,
            route_key: record.route_key,
        });
    }
    create_and_store_persisted_session(state, runner, profile, route, controller_did)
}

fn reset_and_create_persisted_session<G>(
    state: &DaemonState,
    runner: &HermesRunner<G>,
    profile: &HermesProfileRecord,
    route: &HermesSessionRoute,
    controller_did: &str,
) -> Result<super::gateway::HermesSessionRef>
where
    G: HermesGateway,
{
    state.reset_active_hermes_session_by_route(route)?;
    create_and_store_persisted_session(state, runner, profile, route, controller_did)
}

fn create_and_store_persisted_session<G>(
    state: &DaemonState,
    runner: &HermesRunner<G>,
    profile: &HermesProfileRecord,
    route: &HermesSessionRoute,
    controller_did: &str,
) -> Result<super::gateway::HermesSessionRef>
where
    G: HermesGateway,
{
    let session = runner
        .create_session(HermesSessionCreateRequest {
            route_key: route.route_key(),
            conversation_id: route.conversation_id.clone(),
        })
        .context("create Hermes session")?;
    let record = HermesNativeSessionRecord::active(
        route,
        controller_did.to_string(),
        profile.hermes_profile.clone(),
        session.hermes_session_id.clone(),
    )?;
    state.store_hermes_native_session(&record)?;
    Ok(session)
}

fn is_missing_hermes_session_error(message: &str) -> bool {
    message.to_ascii_lowercase().contains("session not found")
}

pub fn reset_hermes_session_by_route(
    state: &DaemonState,
    route: &HermesSessionRoute,
) -> Result<usize> {
    state.reset_active_hermes_session_by_route(route)
}
