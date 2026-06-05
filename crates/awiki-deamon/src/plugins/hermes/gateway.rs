use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::cli_wrapper::CliWrapperRequest;
use crate::config::DaemonConfig;
use crate::local_rpc::RuntimeRpcRequest;
use crate::plugins::hermes::ensure_runtime_model_config;
use crate::runtime::RuntimeInstallStatus;
use crate::state::HermesProfileRecord;

const HERMES_GATEWAY_CMD_ENV: &str = "AWIKI_HERMES_GATEWAY_CMD";
const HERMES_BIN_ENV: &str = "AWIKI_HERMES_BIN";
const HERMES_GATEWAY_DETECTION_READY_TIMEOUT: Duration = Duration::from_secs(4);

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<HermesGatewayErrorSummary>,
    #[serde(default)]
    pub callbacks: Vec<RuntimeRpcRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HermesGatewayErrorSummary {
    pub code: String,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HermesRuntimeEvent {
    pub kind: HermesRuntimeEventKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
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
            code: None,
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

    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }
}

#[derive(Debug, Clone)]
pub struct StdioHermesGateway {
    gateway_cmd: Option<String>,
    hermes_bin: Option<PathBuf>,
    processes: Arc<Mutex<BTreeMap<String, StdioGatewayProcess>>>,
    timeouts: HermesGatewayTimeouts,
}

impl StdioHermesGateway {
    pub fn from_env() -> Self {
        Self {
            gateway_cmd: std::env::var(HERMES_GATEWAY_CMD_ENV)
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            hermes_bin: std::env::var_os(HERMES_BIN_ENV).map(PathBuf::from),
            processes: Arc::new(Mutex::new(BTreeMap::new())),
            timeouts: HermesGatewayTimeouts::default(),
        }
    }

    pub fn from_config(config: &DaemonConfig) -> Self {
        let gateway_cmd = configured_gateway_command(config).or_else(|| {
            detect_hermes_gateway_command(config).map(|detected| {
                let _ = config.write_persistent_hermes_gateway_cmd(Some(detected.command.clone()));
                detected.command
            })
        });
        Self {
            gateway_cmd,
            hermes_bin: std::env::var_os(HERMES_BIN_ENV).map(PathBuf::from),
            processes: Arc::new(Mutex::new(BTreeMap::new())),
            timeouts: HermesGatewayTimeouts::default(),
        }
    }

    pub fn from_config_without_detection(config: &DaemonConfig) -> Self {
        Self {
            gateway_cmd: configured_gateway_command(config),
            hermes_bin: std::env::var_os(HERMES_BIN_ENV).map(PathBuf::from),
            processes: Arc::new(Mutex::new(BTreeMap::new())),
            timeouts: HermesGatewayTimeouts::default(),
        }
    }

    pub fn new(hermes_bin: impl Into<PathBuf>) -> Self {
        Self {
            gateway_cmd: None,
            hermes_bin: Some(hermes_bin.into()),
            processes: Arc::new(Mutex::new(BTreeMap::new())),
            timeouts: HermesGatewayTimeouts::default(),
        }
    }

    pub fn with_gateway_cmd(gateway_cmd: impl Into<String>) -> Self {
        Self {
            gateway_cmd: Some(gateway_cmd.into()),
            hermes_bin: None,
            processes: Arc::new(Mutex::new(BTreeMap::new())),
            timeouts: HermesGatewayTimeouts::default(),
        }
    }

    pub fn with_timeouts(mut self, timeouts: HermesGatewayTimeouts) -> Self {
        self.timeouts = timeouts;
        self
    }

    pub fn gateway_command_status(&self) -> HermesGatewayCommandStatus {
        let Some(command) = self.gateway_cmd.as_deref() else {
            return HermesGatewayCommandStatus::Missing;
        };
        match split_gateway_command(command) {
            Ok(parts) => parts
                .first()
                .map(String::as_str)
                .filter(|executable| executable_is_available(executable))
                .map(|_| HermesGatewayCommandStatus::Configured)
                .unwrap_or(HermesGatewayCommandStatus::Unavailable),
            Err(_) => HermesGatewayCommandStatus::Unavailable,
        }
    }

    pub fn gateway_cmd(&self) -> Option<&str> {
        self.gateway_cmd.as_deref()
    }

    pub fn ensure_detected_config(
        config: &DaemonConfig,
    ) -> Result<Option<DetectedHermesGatewayCommand>> {
        if configured_gateway_command(config).is_some() {
            return Ok(None);
        }
        let Some(detected) = detect_hermes_gateway_command(config) else {
            return Ok(None);
        };
        config.write_persistent_hermes_gateway_cmd(Some(detected.command.clone()))?;
        Ok(Some(detected))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectedHermesGatewayCommand {
    pub command: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HermesGatewayCommandCandidate {
    parts: Vec<String>,
    source: String,
}

fn configured_gateway_command(config: &DaemonConfig) -> Option<String> {
    normalize_gateway_command(std::env::var(HERMES_GATEWAY_CMD_ENV).ok())
        .or_else(|| normalize_gateway_command(config.hermes_gateway_cmd.clone()))
        .or_else(|| config.read_persistent_hermes_gateway_cmd().ok().flatten())
}

fn normalize_gateway_command(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn detect_hermes_gateway_command(config: &DaemonConfig) -> Option<DetectedHermesGatewayCommand> {
    hermes_gateway_command_candidates()
        .into_iter()
        .filter(|candidate| candidate_executable_is_available(candidate))
        .find_map(|candidate| {
            let command = gateway_command_from_parts(&candidate.parts);
            smoke_test_gateway_command(&command, config).ok().map(|_| {
                DetectedHermesGatewayCommand {
                    command,
                    source: candidate.source,
                }
            })
        })
}

fn hermes_gateway_command_candidates() -> Vec<HermesGatewayCommandCandidate> {
    let mut candidates = Vec::new();
    if let Some(home) = std::env::var_os("HOME").filter(|value| !value.is_empty()) {
        let home = PathBuf::from(home);
        for relative in [
            ".hermes/hermes-agent/venv/bin/python",
            ".hermes/hermes-agent/.venv/bin/python",
        ] {
            candidates.push(python_gateway_candidate(
                home.join(relative),
                format!("home:{relative}"),
            ));
        }
    }
    candidates.push(python_gateway_candidate("python3", "path:python3"));
    candidates.push(python_gateway_candidate("python", "path:python"));
    dedupe_gateway_candidates(candidates)
}

fn python_gateway_candidate(
    python: impl Into<PathBuf>,
    source: impl Into<String>,
) -> HermesGatewayCommandCandidate {
    HermesGatewayCommandCandidate {
        parts: vec![
            python.into().display().to_string(),
            "-m".to_string(),
            "tui_gateway.entry".to_string(),
        ],
        source: source.into(),
    }
}

fn dedupe_gateway_candidates(
    candidates: Vec<HermesGatewayCommandCandidate>,
) -> Vec<HermesGatewayCommandCandidate> {
    let mut seen = std::collections::BTreeSet::new();
    candidates
        .into_iter()
        .filter(|candidate| seen.insert(candidate.parts.clone()))
        .collect()
}

fn candidate_executable_is_available(candidate: &HermesGatewayCommandCandidate) -> bool {
    candidate
        .parts
        .first()
        .map(String::as_str)
        .is_some_and(executable_is_available)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HermesGatewayTimeouts {
    pub gateway_ready: Duration,
    pub session_create: Duration,
    pub prompt_first_event: Duration,
    pub prompt_total: Duration,
}

impl Default for HermesGatewayTimeouts {
    fn default() -> Self {
        Self {
            gateway_ready: Duration::from_secs(10),
            session_create: Duration::from_secs(30),
            prompt_first_event: Duration::from_secs(60),
            prompt_total: Duration::from_secs(30 * 60),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HermesGatewayCommandStatus {
    Configured,
    Missing,
    Unavailable,
}

impl HermesGatewayCommandStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Configured => "configured",
            Self::Missing => "missing",
            Self::Unavailable => "unavailable",
        }
    }

    pub fn needs_config(self) -> bool {
        self != Self::Configured
    }

    pub fn error_code(self) -> Option<&'static str> {
        match self {
            Self::Configured => None,
            Self::Missing => Some("gateway_command_missing"),
            Self::Unavailable => Some("gateway_command_unavailable"),
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
        if let Some(command) = self.gateway_cmd.as_deref() {
            let parts = split_gateway_command(command)?;
            let executable = parts
                .first()
                .map(String::as_str)
                .context("AWIKI_HERMES_GATEWAY_CMD is empty")?;
            return Ok(RuntimeInstallStatus {
                installed: executable_is_available(executable),
                detail: Some(sanitize_gateway_command(command)),
            });
        }
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
        ensure_runtime_model_config(&profile.hermes_home)?;
        let runner = HermesRunnerRef {
            runner_id: format!("stdio:{}", profile.hermes_profile),
            agent_did: profile.agent_did.clone(),
            runtime_profile_id: profile.runtime_profile_id.clone(),
            hermes_profile: profile.hermes_profile.clone(),
            hermes_home: profile.hermes_home.clone(),
        };
        if self.gateway_cmd.is_none() {
            let status = self.check_installation()?;
            if !status.installed {
                bail!(
                    "Hermes gateway command is not configured: {}",
                    status.detail.unwrap_or_else(|| "unknown".to_string())
                );
            }
            bail!("AWIKI_HERMES_GATEWAY_CMD is required for real Hermes TUI Gateway runs");
        }
        let mut processes = self
            .processes
            .lock()
            .expect("Hermes gateway process lock poisoned");
        if let Some(process) = processes.get_mut(&runner.runner_id) {
            if process.is_running() {
                return Ok(runner);
            }
        }
        let mut process =
            spawn_gateway_process(self.gateway_cmd.as_deref().unwrap_or_default(), profile)?;
        process.wait_for_ready(self.timeouts.gateway_ready)?;
        processes.insert(runner.runner_id.clone(), process);
        Ok(runner)
    }

    fn create_session(
        &self,
        runner: &HermesRunnerRef,
        request: HermesSessionCreateRequest,
    ) -> Result<HermesSessionRef> {
        let mut processes = self
            .processes
            .lock()
            .expect("Hermes gateway process lock poisoned");
        let process = processes
            .get_mut(&runner.runner_id)
            .context("Hermes gateway process is not running")?;
        let response = process.call(
            "session.create",
            json!({}),
            self.timeouts.session_create,
            &mut Vec::new(),
        )?;
        let session_id = extract_session_id(&response)
            .context("Hermes session.create response did not include session_id")?;
        Ok(HermesSessionRef {
            runner_id: runner.runner_id.clone(),
            hermes_session_id: session_id,
            route_key: request.route_key,
        })
    }

    fn submit_prompt(
        &self,
        session: &HermesSessionRef,
        request: HermesPromptSubmitRequest,
    ) -> Result<HermesPromptOutcome> {
        let mut processes = self
            .processes
            .lock()
            .expect("Hermes gateway process lock poisoned");
        let process = processes
            .get_mut(&session.runner_id)
            .context("Hermes gateway process is not running")?;
        let mut events = Vec::new();
        let response = process.call(
            "prompt.submit",
            json!({
                "session_id": session.hermes_session_id,
                "text": request.prompt,
            }),
            self.timeouts.prompt_first_event,
            &mut events,
        )?;
        append_events_from_response(&response, session, &request, &mut events);
        if prompt_response_is_streaming(&response) && !has_terminal_prompt_event(&events) {
            process.collect_until_prompt_terminal_after_first_event(
                self.timeouts.prompt_first_event,
                self.timeouts.prompt_total,
                &mut events,
            )?;
        }
        let error = error_summary_from_events(&events);
        let final_text = final_text_from_events(&events);
        Ok(HermesPromptOutcome {
            session: session.clone(),
            events,
            final_text,
            error,
            callbacks: Vec::new(),
        })
    }
}

#[derive(Debug)]
struct StdioGatewayProcess {
    child: Child,
    stdin: ChildStdin,
    stdout_rx: mpsc::Receiver<String>,
    stderr_lines: Arc<Mutex<Vec<String>>>,
    next_id: u64,
}

impl StdioGatewayProcess {
    fn is_running(&mut self) -> bool {
        self.child.try_wait().ok().flatten().is_none()
    }

    fn terminate(&mut self) {
        if self.child.try_wait().ok().flatten().is_some() {
            return;
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    fn wait_for_ready(&mut self, timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;
        loop {
            let line = self
                .read_line(remaining_timeout(
                    deadline,
                    "Hermes gateway did not emit gateway.ready before timeout",
                )?)
                .context("Hermes gateway did not emit gateway.ready before timeout")?;
            let value: Value = serde_json::from_str(&line)
                .with_context(|| format!("parse Hermes gateway line: {}", redact_line(&line)))?;
            if is_gateway_ready_event(&value) {
                return Ok(());
            }
            if let Some(error) = response_error(&value) {
                bail!("Hermes gateway ready failed: {error}");
            }
        }
    }

    fn call(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
        events: &mut Vec<HermesRuntimeEvent>,
    ) -> Result<Value> {
        let id = self.next_id.to_string();
        self.next_id += 1;
        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        writeln!(self.stdin, "{request}")?;
        self.stdin.flush()?;
        let deadline = Instant::now() + timeout;
        loop {
            let timeout = remaining_timeout(deadline, "Hermes gateway response timed out")
                .with_context(|| format!("Hermes gateway {method} response timed out"))?;
            let line = self
                .read_line(timeout)
                .with_context(|| format!("Hermes gateway {method} response timed out"))?;
            let value: Value = serde_json::from_str(&line)
                .with_context(|| format!("parse Hermes gateway line: {}", redact_line(&line)))?;
            if let Some(event) = event_from_gateway_line(&value) {
                events.push(event);
            }
            if value.get("id").and_then(json_id_as_string).as_deref() == Some(id.as_str()) {
                if let Some(error) = response_error(&value) {
                    bail!("Hermes gateway {method} failed: {error}");
                }
                return Ok(value.get("result").cloned().unwrap_or(Value::Null));
            }
        }
    }

    fn collect_until_prompt_terminal_after_first_event(
        &mut self,
        first_event_timeout: Duration,
        total_timeout: Duration,
        events: &mut Vec<HermesRuntimeEvent>,
    ) -> Result<()> {
        let mut saw_prompt_event = false;
        let deadline = Instant::now() + total_timeout;
        loop {
            let timeout = if !saw_prompt_event {
                first_event_timeout.min(remaining_timeout(
                    deadline,
                    "Hermes gateway prompt stream exceeded total timeout",
                )?)
            } else {
                remaining_timeout(
                    deadline,
                    "Hermes gateway prompt stream exceeded total timeout",
                )?
            };
            let line = self.read_line(timeout).with_context(|| {
                if saw_prompt_event {
                    "Hermes gateway prompt stream exceeded total timeout"
                } else {
                    "Hermes gateway prompt stream timed out before first event"
                }
            })?;
            let value: Value = serde_json::from_str(&line)
                .with_context(|| format!("parse Hermes gateway line: {}", redact_line(&line)))?;
            if let Some(error) = response_error(&value) {
                bail!("Hermes gateway prompt stream failed: {error}");
            }
            if let Some(event) = event_from_gateway_line(&value) {
                saw_prompt_event = true;
                let terminal = is_terminal_prompt_event(&event);
                events.push(event);
                if terminal {
                    return Ok(());
                }
            }
        }
    }

    fn read_line(&mut self, timeout: Duration) -> Result<String> {
        match self.stdout_rx.recv_timeout(timeout) {
            Ok(line) => Ok(line),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                bail!("Hermes gateway timed out: {}", self.stderr_summary())
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                bail!("Hermes gateway exited: {}", self.stderr_summary())
            }
        }
    }

    fn stderr_summary(&self) -> String {
        let lines = self
            .stderr_lines
            .lock()
            .map(|lines| lines.clone())
            .unwrap_or_default();
        let summary = lines
            .into_iter()
            .rev()
            .take(3)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join(" | ");
        if summary.trim().is_empty() {
            "no stderr".to_string()
        } else {
            redact_line(&summary)
        }
    }
}

fn remaining_timeout(deadline: Instant, timeout_error: &'static str) -> Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|remaining| !remaining.is_zero())
        .with_context(|| timeout_error)
}

fn spawn_gateway_process(
    gateway_cmd: &str,
    profile: &HermesProfileRecord,
) -> Result<StdioGatewayProcess> {
    let parts = split_gateway_command(gateway_cmd)?;
    let executable = parts.first().context("AWIKI_HERMES_GATEWAY_CMD is empty")?;
    let mut command = Command::new(executable);
    command
        .args(parts.iter().skip(1))
        .current_dir(&profile.hermes_home)
        .env("HERMES_PROFILE", &profile.hermes_profile)
        .env("HERMES_HOME", &profile.hermes_home)
        .env("AWIKI_HERMES_PROFILE", &profile.hermes_profile)
        .env("AWIKI_HERMES_HOME", &profile.hermes_home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().with_context(|| {
        format!(
            "spawn AWIKI_HERMES_GATEWAY_CMD {}",
            sanitize_gateway_command(gateway_cmd)
        )
    })?;
    let stdin = child.stdin.take().context("open Hermes gateway stdin")?;
    let stdout = child.stdout.take().context("open Hermes gateway stdout")?;
    let stderr = child.stderr.take().context("open Hermes gateway stderr")?;
    let (stdout_tx, stdout_rx) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if stdout_tx.send(line).is_err() {
                break;
            }
        }
    });
    let stderr_lines = Arc::new(Mutex::new(Vec::new()));
    let stderr_lines_for_thread = Arc::clone(&stderr_lines);
    thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if let Ok(mut lines) = stderr_lines_for_thread.lock() {
                lines.push(line);
                let overflow = lines.len().saturating_sub(20);
                if overflow > 0 {
                    lines.drain(0..overflow);
                }
            }
        }
    });
    Ok(StdioGatewayProcess {
        child,
        stdin,
        stdout_rx,
        stderr_lines,
        next_id: 1,
    })
}

fn smoke_test_gateway_command(gateway_cmd: &str, config: &DaemonConfig) -> Result<()> {
    let probe_home = gateway_detection_probe_home(config)?;
    if let Some(parent) = probe_home.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!("create Hermes gateway probe directory {}", parent.display())
        })?;
    }
    std::fs::create_dir_all(&probe_home)
        .with_context(|| format!("create Hermes gateway probe home {}", probe_home.display()))?;
    let profile = HermesProfileRecord {
        agent_did: "did:awiki:hermes-gateway-detect".to_string(),
        runtime_profile_id: "hermes_gateway_detect".to_string(),
        hermes_profile: "awiki_gateway_detect".to_string(),
        hermes_home: probe_home.clone(),
        hermes_version: None,
        awiki_skills_version: "detect".to_string(),
        status: "probe".to_string(),
    };
    ensure_runtime_model_config(&profile.hermes_home)?;
    let mut process = spawn_gateway_process(gateway_cmd, &profile)?;
    let ready = process.wait_for_ready(HERMES_GATEWAY_DETECTION_READY_TIMEOUT);
    process.terminate();
    let _ = std::fs::remove_dir_all(&probe_home);
    ready
}

fn gateway_detection_probe_home(config: &DaemonConfig) -> Result<PathBuf> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    Ok(config
        .runtime_temp_dir
        .join("hermes-gateway-detect")
        .join(format!("probe-{now}-{}", std::process::id())))
}

fn gateway_command_from_parts(parts: &[String]) -> String {
    parts
        .iter()
        .map(|part| shell_quote_arg(part))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_quote_arg(value: &str) -> String {
    if !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-' | ':' | '='))
    {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn split_gateway_command(command: &str) -> Result<Vec<String>> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for ch in command.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' && quote != Some('\'') {
            escaped = true;
            continue;
        }
        if matches!(quote, Some(q) if q == ch) {
            quote = None;
            continue;
        }
        if quote.is_none() && (ch == '\'' || ch == '"') {
            quote = Some(ch);
            continue;
        }
        if quote.is_none() && ch.is_whitespace() {
            if !current.is_empty() {
                parts.push(std::mem::take(&mut current));
            }
            continue;
        }
        current.push(ch);
    }
    if escaped {
        current.push('\\');
    }
    if quote.is_some() {
        bail!("AWIKI_HERMES_GATEWAY_CMD has an unclosed quote");
    }
    if !current.is_empty() {
        parts.push(current);
    }
    if parts.is_empty() {
        bail!("AWIKI_HERMES_GATEWAY_CMD must not be empty");
    }
    Ok(parts)
}

fn executable_is_available(executable: &str) -> bool {
    let path = Path::new(executable);
    if path.components().count() > 1 {
        return path.exists();
    }
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths).any(|dir| {
                let candidate = dir.join(executable);
                candidate.is_file()
            })
        })
        .unwrap_or(false)
}

fn sanitize_gateway_command(command: &str) -> String {
    split_gateway_command(command)
        .map(|parts| {
            parts
                .into_iter()
                .map(|part| {
                    if part.to_ascii_lowercase().contains("token")
                        || part.to_ascii_lowercase().contains("secret")
                    {
                        "<redacted>".to_string()
                    } else {
                        part
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_else(|_| "<invalid gateway command>".to_string())
}

fn is_gateway_ready_event(value: &Value) -> bool {
    event_name(value)
        .as_deref()
        .is_some_and(|name| name == "gateway.ready")
}

fn event_from_gateway_line(value: &Value) -> Option<HermesRuntimeEvent> {
    let name = event_name(value)?;
    let raw_params = value
        .get("params")
        .or_else(|| value.get("data"))
        .or_else(|| value.get("result"))
        .unwrap_or(value);
    let params = raw_params.get("payload").unwrap_or(raw_params);
    let mut event = match name.as_str() {
        "gateway.ready" => HermesRuntimeEvent::new(HermesRuntimeEventKind::RunnerReady),
        "session.created" | "session.create" | "session.info" => {
            HermesRuntimeEvent::new(HermesRuntimeEventKind::SessionCreated)
        }
        "message.start" | "prompt.submitted" | "prompt.submit" => {
            HermesRuntimeEvent::new(HermesRuntimeEventKind::PromptSubmitted)
        }
        "message.delta" | "thinking.delta" | "reasoning.delta" => {
            HermesRuntimeEvent::new(HermesRuntimeEventKind::MessageDelta)
        }
        "message.complete" => HermesRuntimeEvent::new(HermesRuntimeEventKind::MessageComplete),
        "tool.start" | "tool.generating" | "tool.complete" | "tool.progress" => {
            HermesRuntimeEvent::new(HermesRuntimeEventKind::ToolCallObserved)
        }
        "error" | "gateway.error" => {
            HermesRuntimeEvent::new(HermesRuntimeEventKind::Error).with_code("gateway_error")
        }
        "runner.exited" => HermesRuntimeEvent::new(HermesRuntimeEventKind::RunnerExited),
        other if other.ends_with(".request") => HermesRuntimeEvent {
            kind: HermesRuntimeEventKind::Error,
            code: Some(unsupported_request_error_code(other).to_string()),
            session_id: None,
            run_id: None,
            text: Some(format!(
                "{}: unsupported Hermes interaction {other}",
                unsupported_request_error_code(other)
            )),
            detail: Some(json!({ "event": other })),
        },
        _ => return None,
    };
    if let Some(session_id) = extract_string(params, &["session_id", "sessionId", "session"])
        .or_else(|| extract_string(raw_params, &["session_id", "sessionId", "session"]))
    {
        event.session_id = Some(session_id);
    }
    if let Some(run_id) = extract_string(params, &["run_id", "runId"])
        .or_else(|| extract_string(raw_params, &["run_id", "runId"]))
    {
        event.run_id = Some(run_id);
    }
    if let Some(text) = extract_string(params, &["text", "content", "message", "delta"]) {
        event.text = Some(text);
    }
    if name == "message.complete"
        && extract_string(params, &["status"])
            .or_else(|| extract_string(raw_params, &["status"]))
            .as_deref()
            == Some("error")
    {
        event.kind = HermesRuntimeEventKind::Error;
        event.code = Some(
            extract_string(params, &["error_code", "errorCode", "code"])
                .or_else(|| extract_string(raw_params, &["error_code", "errorCode", "code"]))
                .unwrap_or_else(|| "message_complete_error".to_string()),
        );
    }
    if raw_params.is_object() {
        event.detail = Some(raw_params.clone());
    }
    Some(event)
}

fn unsupported_request_error_code(event_name: &str) -> &'static str {
    match event_name {
        "approval.request" => "approval_not_supported",
        "clarify.request" => "clarify_not_supported",
        "sudo.request" => "sudo_not_supported",
        "secret.request" => "secret_not_supported",
        _ => "unsupported_interaction",
    }
}

fn event_name(value: &Value) -> Option<String> {
    if value.get("method").and_then(Value::as_str) == Some("event") {
        if let Some(params) = value.get("params") {
            if let Some(name) = extract_string(params, &["type", "event", "kind", "name"]) {
                return Some(name);
            }
        }
    }
    extract_string(value, &["method", "event", "type", "kind", "name"])
}

fn response_error(value: &Value) -> Option<String> {
    let error = value.get("error")?;
    if error.is_null() {
        return None;
    }
    if let Some(message) = error.get("message").and_then(Value::as_str) {
        return Some(redact_line(message));
    }
    Some(redact_line(&error.to_string()))
}

fn prompt_response_is_streaming(response: &Value) -> bool {
    extract_string(response, &["status"]).as_deref() == Some("streaming")
}

fn has_terminal_prompt_event(events: &[HermesRuntimeEvent]) -> bool {
    events.iter().any(is_terminal_prompt_event)
}

fn is_terminal_prompt_event(event: &HermesRuntimeEvent) -> bool {
    matches!(
        event.kind,
        HermesRuntimeEventKind::MessageComplete
            | HermesRuntimeEventKind::Error
            | HermesRuntimeEventKind::RunnerExited
    )
}

fn json_id_as_string(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_string)
        .or_else(|| value.as_i64().map(|id| id.to_string()))
        .or_else(|| value.as_u64().map(|id| id.to_string()))
}

fn extract_session_id(value: &Value) -> Option<String> {
    value.as_str().map(str::to_string).or_else(|| {
        extract_string(
            value,
            &[
                "session_id",
                "sessionId",
                "hermes_session_id",
                "id",
                "session",
            ],
        )
    })
}

fn append_events_from_response(
    response: &Value,
    session: &HermesSessionRef,
    request: &HermesPromptSubmitRequest,
    events: &mut Vec<HermesRuntimeEvent>,
) {
    if let Some(items) = response.get("events").and_then(Value::as_array) {
        events.extend(items.iter().filter_map(event_from_gateway_line));
    }
    if let Some(text) = extract_string(response, &["final_text", "text", "content"]) {
        events.push(
            HermesRuntimeEvent::new(HermesRuntimeEventKind::MessageComplete)
                .with_session(session.hermes_session_id.clone())
                .with_run(request.run_id.clone())
                .with_text(text),
        );
    }
}

fn final_text_from_events(events: &[HermesRuntimeEvent]) -> Option<String> {
    events.iter().rev().find_map(|event| {
        if event.kind != HermesRuntimeEventKind::MessageComplete {
            return None;
        }
        event
            .text
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_string)
    })
}

fn error_summary_from_events(events: &[HermesRuntimeEvent]) -> Option<HermesGatewayErrorSummary> {
    events.iter().rev().find_map(|event| {
        if event.kind != HermesRuntimeEventKind::Error {
            return None;
        }
        let summary = event
            .text
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(redact_line)
            .unwrap_or_else(|| "Hermes gateway error".to_string());
        Some(HermesGatewayErrorSummary {
            code: event
                .code
                .as_deref()
                .filter(|code| !code.trim().is_empty())
                .unwrap_or("gateway_error")
                .to_string(),
            summary,
        })
    })
}

fn extract_string(value: &Value, fields: &[&str]) -> Option<String> {
    fields.iter().find_map(|field| {
        value
            .get(*field)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

fn redact_line(line: &str) -> String {
    let mut redacted = line.to_string();
    for marker in [
        "rtok_",
        "registration_token",
        "jwt",
        "private_key",
        "bearer ",
        "api_key",
        "api key",
        "secret",
        "sk-",
        "token prefix",
    ] {
        redacted = redact_after_marker(&redacted, marker);
    }
    if redacted.chars().count() > 512 {
        redacted = redacted.chars().take(512).collect::<String>() + "...";
    }
    redacted
}

fn redact_after_marker(input: &str, marker: &str) -> String {
    let lower = input.to_ascii_lowercase();
    let Some(index) = lower.find(marker) else {
        return input.to_string();
    };
    let end = input[index..]
        .find(char::is_whitespace)
        .map(|offset| index + offset)
        .unwrap_or(input.len());
    format!("{}<redacted>{}", &input[..index], &input[end..])
}

#[derive(Debug, Clone, Default)]
pub struct FakeHermesGateway {
    observed_events: Arc<Mutex<Vec<HermesRuntimeEvent>>>,
    submitted_prompts: Arc<Mutex<Vec<HermesPromptSubmitRequest>>>,
    created_sessions: Arc<Mutex<Vec<HermesSessionCreateRequest>>>,
    prompt_attempts: Arc<Mutex<usize>>,
    behavior: FakeHermesBehavior,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FakeHermesBehavior {
    #[default]
    ObserveOnly,
    FinishSuccessfully,
    CompleteWithoutText,
    FailWithStatus,
    SendMessage,
    SendHandleMessage,
    ApprovalRequest,
    FailOnceWithMissingSession,
}

impl FakeHermesGateway {
    pub fn with_behavior(behavior: FakeHermesBehavior) -> Self {
        Self {
            observed_events: Arc::new(Mutex::new(Vec::new())),
            submitted_prompts: Arc::new(Mutex::new(Vec::new())),
            created_sessions: Arc::new(Mutex::new(Vec::new())),
            prompt_attempts: Arc::new(Mutex::new(0)),
            behavior,
        }
    }

    pub fn submitted_prompts(&self) -> Vec<HermesPromptSubmitRequest> {
        self.submitted_prompts
            .lock()
            .expect("fake Hermes gateway prompts lock poisoned")
            .clone()
    }

    pub fn created_sessions(&self) -> Vec<HermesSessionCreateRequest> {
        self.created_sessions
            .lock()
            .expect("fake Hermes gateway sessions lock poisoned")
            .clone()
    }
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
        let create_count = {
            let mut sessions = self
                .created_sessions
                .lock()
                .expect("fake Hermes gateway sessions lock poisoned");
            sessions.push(request.clone());
            sessions.len()
        };
        let suffix = if create_count == 1 {
            String::new()
        } else {
            format!("-{create_count}")
        };
        let session = HermesSessionRef {
            runner_id: runner.runner_id.clone(),
            hermes_session_id: format!("fake-session-{}{}", request.route_key, suffix),
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
        let prompt_attempt = {
            let mut attempts = self
                .prompt_attempts
                .lock()
                .expect("fake Hermes gateway prompt attempts lock poisoned");
            *attempts += 1;
            *attempts
        };
        self.submitted_prompts
            .lock()
            .expect("fake Hermes gateway prompts lock poisoned")
            .push(request.clone());
        if self.behavior == FakeHermesBehavior::FailOnceWithMissingSession && prompt_attempt == 1 {
            anyhow::bail!("Hermes gateway prompt.submit failed: session not found");
        }
        let run_id = request.run_id.clone();
        let message_id = request.message_id.clone();
        let fake_token = "rtok_fake_hermes_runtime_token_placeholder_123456789".to_string();
        let mut events = vec![
            HermesRuntimeEvent::new(HermesRuntimeEventKind::PromptSubmitted)
                .with_session(session.hermes_session_id.clone())
                .with_run(request.run_id.clone()),
            HermesRuntimeEvent::new(HermesRuntimeEventKind::MessageDelta)
                .with_session(session.hermes_session_id.clone())
                .with_run(request.run_id.clone())
                .with_text("fake delta"),
        ];
        if self.behavior == FakeHermesBehavior::FailWithStatus {
            events.push(
                HermesRuntimeEvent::new(HermesRuntimeEventKind::Error)
                    .with_session(session.hermes_session_id.clone())
                    .with_run(request.run_id)
                    .with_text("fake failure"),
            );
        } else if self.behavior == FakeHermesBehavior::ApprovalRequest {
            events.push(
                HermesRuntimeEvent::new(HermesRuntimeEventKind::Error)
                    .with_code("approval_not_supported")
                    .with_session(session.hermes_session_id.clone())
                    .with_run(request.run_id)
                    .with_text(
                        "approval_not_supported: unsupported Hermes interaction approval.request",
                    ),
            );
        } else if self.behavior == FakeHermesBehavior::CompleteWithoutText {
            events.push(
                HermesRuntimeEvent::new(HermesRuntimeEventKind::RunnerExited)
                    .with_session(session.hermes_session_id.clone())
                    .with_run(request.run_id),
            );
        } else {
            events.push(
                HermesRuntimeEvent::new(HermesRuntimeEventKind::MessageComplete)
                    .with_session(session.hermes_session_id.clone())
                    .with_run(request.run_id)
                    .with_text("fake complete"),
            );
        }
        for event in events.iter().cloned() {
            self.push_event(event);
        }
        let error = error_summary_from_events(&events);
        let final_text = final_text_from_events(&events);
        Ok(HermesPromptOutcome {
            session: session.clone(),
            events,
            final_text,
            error,
            callbacks: fake_callbacks(self.behavior, fake_token, message_id, run_id),
        })
    }
}

fn fake_callbacks(
    behavior: FakeHermesBehavior,
    runtime_rpc_token: String,
    task_id: String,
    _run_id: String,
) -> Vec<RuntimeRpcRequest> {
    match behavior {
        FakeHermesBehavior::FinishSuccessfully => vec![
            CliWrapperRequest::task_status(
                runtime_rpc_token.clone(),
                task_id.clone(),
                "running",
                "Hermes runtime started",
            )
            .into_rpc_request(),
            CliWrapperRequest::task_finish(runtime_rpc_token, task_id, "Hermes runtime finished")
                .into_rpc_request(),
        ],
        FakeHermesBehavior::FailWithStatus => vec![CliWrapperRequest::task_status(
            runtime_rpc_token,
            task_id,
            "failed",
            "Hermes runtime failed",
        )
        .into_rpc_request()],
        FakeHermesBehavior::ObserveOnly => Vec::new(),
        FakeHermesBehavior::CompleteWithoutText => Vec::new(),
        FakeHermesBehavior::ApprovalRequest => Vec::new(),
        FakeHermesBehavior::FailOnceWithMissingSession => Vec::new(),
        FakeHermesBehavior::SendMessage => vec![CliWrapperRequest::msg_send(
            runtime_rpc_token,
            "did:human:alice",
            "Hermes says hello",
        )
        .into_rpc_request()],
        FakeHermesBehavior::SendHandleMessage => vec![CliWrapperRequest::msg_send_with_security(
            runtime_rpc_token,
            "bob",
            "Hermes says hello Bob",
            Some("direct_e2ee"),
        )
        .into_rpc_request()],
    }
}
