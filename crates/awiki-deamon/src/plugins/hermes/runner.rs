use anyhow::{bail, Context, Result};

use crate::runtime::{
    RuntimeInstallStatus, RuntimeLaunchContext, RuntimeLaunchOutcome, RuntimePlugin,
    RuntimeRunStatus,
};
use crate::state::HermesProfileRecord;

use super::gateway::{
    HermesGateway, HermesPromptOutcome, HermesPromptSubmitRequest, HermesRunnerRef,
    HermesSessionCreateRequest, HermesSessionRef,
};
use super::HERMES_RUNTIME_PLUGIN_ID;

#[derive(Debug, Clone)]
pub struct HermesRuntimePlugin<G> {
    gateway: G,
    profile: HermesProfileRecord,
}

#[derive(Debug, Clone)]
pub struct HermesRunner<G> {
    gateway: G,
    runner: HermesRunnerRef,
}

impl<G> HermesRuntimePlugin<G> {
    pub fn new(gateway: G, profile: HermesProfileRecord) -> Self {
        Self { gateway, profile }
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
        {
            bail!("Hermes launch context does not match profile binding");
        }
        let runner = HermesRunner::start(self.gateway.clone(), &self.profile)
            .context("start Hermes runner")?;
        let session = runner
            .create_session(HermesSessionCreateRequest {
                route_key: context
                    .task
                    .conversation_id
                    .clone()
                    .unwrap_or_else(|| context.task.task_id.clone()),
                conversation_id: context.task.conversation_id.clone(),
            })
            .context("create Hermes session")?;
        runner
            .submit_prompt(
                &session,
                HermesPromptSubmitRequest {
                    run_id: context.run.run_id.clone(),
                    message_id: context.task.task_id,
                    prompt: context.task.text,
                },
            )
            .context("submit Hermes prompt")?;

        Ok(RuntimeLaunchOutcome {
            run_id: context.run.run_id,
            status: RuntimeRunStatus::Running,
            exit_code: None,
            callbacks: Vec::new(),
        })
    }
}
