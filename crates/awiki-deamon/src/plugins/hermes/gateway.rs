use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::runtime::RuntimeInstallStatus;
use crate::state::HermesProfileRecord;

pub trait HermesGateway {
    fn check_installation(&self) -> Result<RuntimeInstallStatus>;
    fn start(&self, profile: &HermesProfileRecord) -> Result<HermesRunnerRef>;
    fn create_session(
        &self,
        runner: &HermesRunnerRef,
        request: HermesSessionCreateRequest,
    ) -> Result<HermesSessionRef>;
    fn submit_prompt(
        &self,
        session: &HermesSessionRef,
        request: HermesPromptSubmitRequest,
    ) -> Result<HermesPromptOutcome>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HermesRunnerRef {
    pub runner_id: String,
    pub agent_did: String,
    pub runtime_profile_id: String,
    pub hermes_profile: String,
    pub hermes_home: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HermesSessionCreateRequest {
    pub route_key: String,
    pub conversation_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HermesSessionRef {
    pub runner_id: String,
    pub hermes_session_id: String,
    pub route_key: String,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HermesPromptSubmitRequest {
    pub run_id: String,
    pub message_id: String,
    pub prompt: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HermesPromptOutcome {
    pub session: HermesSessionRef,
    pub events: Vec<HermesRuntimeEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HermesRuntimeEvent {
    pub kind: HermesRuntimeEventKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HermesRuntimeEventKind {
    RunnerReady,
    SessionCreated,
    PromptSubmitted,
    MessageDelta,
    MessageComplete,
    ToolCallObserved,
    Error,
    RunnerExited,
}

impl std::fmt::Debug for HermesPromptSubmitRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HermesPromptSubmitRequest")
            .field("run_id", &self.run_id)
            .field("message_id", &self.message_id)
            .field("prompt", &"<redacted>")
            .finish()
    }
}

impl HermesRuntimeEvent {
    pub fn new(kind: HermesRuntimeEventKind) -> Self {
        Self {
            kind,
            session_id: None,
            run_id: None,
            text: None,
            detail: None,
        }
    }

    pub fn with_session(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    pub fn with_run(mut self, run_id: impl Into<String>) -> Self {
        self.run_id = Some(run_id.into());
        self
    }

    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }
}

#[derive(Debug, Clone)]
pub struct StdioHermesGateway {
    hermes_bin: Option<PathBuf>,
}

impl StdioHermesGateway {
    pub fn from_env() -> Self {
        Self {
            hermes_bin: std::env::var_os("AWIKI_HERMES_BIN").map(PathBuf::from),
        }
    }

    pub fn new(hermes_bin: impl Into<PathBuf>) -> Self {
        Self {
            hermes_bin: Some(hermes_bin.into()),
        }
    }
}

impl Default for StdioHermesGateway {
    fn default() -> Self {
        Self::from_env()
    }
}

impl HermesGateway for StdioHermesGateway {
    fn check_installation(&self) -> Result<RuntimeInstallStatus> {
        let Some(path) = self.hermes_bin.as_ref() else {
            return Ok(RuntimeInstallStatus {
                installed: false,
                detail: Some("AWIKI_HERMES_BIN is not set".to_string()),
            });
        };
        Ok(RuntimeInstallStatus {
            installed: path.exists(),
            detail: Some(path.display().to_string()),
        })
    }

    fn start(&self, profile: &HermesProfileRecord) -> Result<HermesRunnerRef> {
        let status = self.check_installation()?;
        if !status.installed {
            bail!(
                "Hermes binary is not installed: {}",
                status.detail.unwrap_or_else(|| "unknown".to_string())
            );
        }
        Ok(HermesRunnerRef {
            runner_id: format!("stdio:{}", profile.hermes_profile),
            agent_did: profile.agent_did.clone(),
            runtime_profile_id: profile.runtime_profile_id.clone(),
            hermes_profile: profile.hermes_profile.clone(),
            hermes_home: profile.hermes_home.clone(),
        })
    }

    fn create_session(
        &self,
        _runner: &HermesRunnerRef,
        _request: HermesSessionCreateRequest,
    ) -> Result<HermesSessionRef> {
        bail!("real Hermes TUI Gateway session.create is not wired in Step 03 skeleton")
    }

    fn submit_prompt(
        &self,
        _session: &HermesSessionRef,
        _request: HermesPromptSubmitRequest,
    ) -> Result<HermesPromptOutcome> {
        bail!("real Hermes TUI Gateway prompt.submit is not wired in Step 03 skeleton")
    }
}

#[derive(Debug, Clone, Default)]
pub struct FakeHermesGateway {
    observed_events: Arc<Mutex<Vec<HermesRuntimeEvent>>>,
}

impl FakeHermesGateway {
    pub fn observed_events(&self) -> Vec<HermesRuntimeEvent> {
        self.observed_events
            .lock()
            .expect("fake Hermes gateway events lock poisoned")
            .clone()
    }

    fn push_event(&self, event: HermesRuntimeEvent) {
        self.observed_events
            .lock()
            .expect("fake Hermes gateway events lock poisoned")
            .push(event);
    }
}

impl HermesGateway for FakeHermesGateway {
    fn check_installation(&self) -> Result<RuntimeInstallStatus> {
        Ok(RuntimeInstallStatus {
            installed: true,
            detail: Some("fake Hermes TUI Gateway".to_string()),
        })
    }

    fn start(&self, profile: &HermesProfileRecord) -> Result<HermesRunnerRef> {
        let runner = HermesRunnerRef {
            runner_id: format!("fake:{}", profile.hermes_profile),
            agent_did: profile.agent_did.clone(),
            runtime_profile_id: profile.runtime_profile_id.clone(),
            hermes_profile: profile.hermes_profile.clone(),
            hermes_home: profile.hermes_home.clone(),
        };
        self.push_event(
            HermesRuntimeEvent::new(HermesRuntimeEventKind::RunnerReady)
                .with_text(runner.runner_id.clone()),
        );
        Ok(runner)
    }

    fn create_session(
        &self,
        runner: &HermesRunnerRef,
        request: HermesSessionCreateRequest,
    ) -> Result<HermesSessionRef> {
        let session = HermesSessionRef {
            runner_id: runner.runner_id.clone(),
            hermes_session_id: format!("fake-session-{}", request.route_key),
            route_key: request.route_key,
        };
        self.push_event(
            HermesRuntimeEvent::new(HermesRuntimeEventKind::SessionCreated)
                .with_session(session.hermes_session_id.clone()),
        );
        Ok(session)
    }

    fn submit_prompt(
        &self,
        session: &HermesSessionRef,
        request: HermesPromptSubmitRequest,
    ) -> Result<HermesPromptOutcome> {
        let events = vec![
            HermesRuntimeEvent::new(HermesRuntimeEventKind::PromptSubmitted)
                .with_session(session.hermes_session_id.clone())
                .with_run(request.run_id.clone()),
            HermesRuntimeEvent::new(HermesRuntimeEventKind::MessageDelta)
                .with_session(session.hermes_session_id.clone())
                .with_run(request.run_id.clone())
                .with_text("fake delta"),
            HermesRuntimeEvent::new(HermesRuntimeEventKind::MessageComplete)
                .with_session(session.hermes_session_id.clone())
                .with_run(request.run_id)
                .with_text("fake complete"),
        ];
        for event in events.iter().cloned() {
            self.push_event(event);
        }
        Ok(HermesPromptOutcome {
            session: session.clone(),
            events,
        })
    }
}
