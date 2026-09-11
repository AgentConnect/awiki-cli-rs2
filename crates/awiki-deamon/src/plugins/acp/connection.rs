use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;

use super::protocol::{
    initialize_request, new_session_request, parse_inbound, permission_response, prompt_request,
    AcpInbound, AcpPermissionPolicy, AcpTextAccumulator, ACP_PROTOCOL_VERSION,
};

#[derive(Clone, PartialEq, Eq)]
pub struct AcpProcessSpec {
    pub runner_id: String,
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
    pub permission_policy: AcpPermissionPolicy,
}

impl std::fmt::Debug for AcpProcessSpec {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AcpProcessSpec")
            .field("runner_id", &self.runner_id)
            .field("program", &self.program)
            .field("args", &self.args)
            .field("cwd", &self.cwd)
            .field("env_names", &self.env.keys().collect::<Vec<_>>())
            .field("permission_policy", &self.permission_policy)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AcpConnectionTimeouts {
    pub initialize: Duration,
    pub session_new: Duration,
    pub prompt_first_update: Duration,
    pub prompt_total: Duration,
}

impl Default for AcpConnectionTimeouts {
    fn default() -> Self {
        Self {
            initialize: Duration::from_secs(10),
            session_new: Duration::from_secs(120),
            prompt_first_update: Duration::from_secs(60),
            prompt_total: Duration::from_secs(30 * 60),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpRunnerRef {
    pub runner_id: String,
    pub connection_epoch: u64,
    pub protocol_version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpSessionRef {
    pub runner_id: String,
    pub connection_epoch: u64,
    pub session_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpPromptOutcome {
    pub session: AcpSessionRef,
    pub stop_reason: String,
    pub final_text: Option<String>,
    pub permission_requests: usize,
}

#[derive(Debug, Clone)]
pub struct AcpProcessPool {
    inner: Arc<Mutex<AcpPoolState>>,
    timeouts: AcpConnectionTimeouts,
}

#[derive(Debug, Default)]
struct AcpPoolState {
    processes: BTreeMap<String, AcpProcess>,
    busy_processes: BTreeMap<String, AcpBusyProcess>,
    epochs: BTreeMap<String, u64>,
}

#[derive(Debug, Clone)]
struct AcpBusyProcess {
    runner: AcpRunnerRef,
    stdin: Arc<Mutex<ChildStdin>>,
}

impl AcpProcessPool {
    pub fn new(timeouts: AcpConnectionTimeouts) -> Self {
        Self {
            inner: Arc::new(Mutex::new(AcpPoolState::default())),
            timeouts,
        }
    }

    pub fn ensure_process(&self, spec: &AcpProcessSpec) -> Result<AcpRunnerRef> {
        self.ensure_process_after(spec, 0)
    }

    pub fn ensure_process_after(
        &self,
        spec: &AcpProcessSpec,
        previous_epoch: u64,
    ) -> Result<AcpRunnerRef> {
        validate_process_spec(spec)?;
        let mut state = self.inner.lock().expect("ACP process pool lock poisoned");
        if let Some(process) = state.busy_processes.get(&spec.runner_id) {
            return Ok(process.runner.clone());
        }
        if let Some(process) = state.processes.get_mut(&spec.runner_id) {
            if process.is_running() {
                return Ok(process.runner.clone());
            }
        }
        if let Some(mut process) = state.processes.remove(&spec.runner_id) {
            process.terminate();
        }

        let next_epoch = state
            .epochs
            .get(&spec.runner_id)
            .copied()
            .unwrap_or_default()
            .max(previous_epoch)
            .checked_add(1)
            .context("ACP connection epoch overflow")?;
        let mut process = AcpProcess::spawn(spec, next_epoch)?;
        process.initialize(self.timeouts.initialize)?;
        let runner = process.runner.clone();
        state.epochs.insert(spec.runner_id.clone(), next_epoch);
        state.processes.insert(spec.runner_id.clone(), process);
        Ok(runner)
    }

    pub fn create_session(
        &self,
        runner: &AcpRunnerRef,
        cwd: &std::path::Path,
    ) -> Result<AcpSessionRef> {
        let mut state = self.inner.lock().expect("ACP process pool lock poisoned");
        let result = {
            let process = active_process(&mut state, runner)?;
            let request_id = process.next_request_id();
            let request = new_session_request(request_id, cwd)?;
            process.call(
                request_id,
                request,
                self.timeouts.session_new,
                "session/new",
            )
        };
        let result = match result {
            Ok(result) => result,
            Err(error) => {
                terminate_failed_process(&mut state, runner);
                return Err(error);
            }
        };
        let session_id = result
            .get("sessionId")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .context("ACP session/new response did not include sessionId")?
            .to_string();
        Ok(AcpSessionRef {
            runner_id: runner.runner_id.clone(),
            connection_epoch: runner.connection_epoch,
            session_id,
        })
    }

    pub fn submit_prompt(
        &self,
        runner: &AcpRunnerRef,
        session: &AcpSessionRef,
        prompt: &str,
    ) -> Result<AcpPromptOutcome> {
        validate_session_ref(runner, session)?;
        let mut process = {
            let mut state = self.inner.lock().expect("ACP process pool lock poisoned");
            if state.busy_processes.contains_key(&runner.runner_id) {
                bail!("ACP runner already has an in-flight prompt");
            }
            let process = take_active_process(&mut state, runner)?;
            state.busy_processes.insert(
                runner.runner_id.clone(),
                AcpBusyProcess {
                    runner: runner.clone(),
                    stdin: Arc::clone(&process.stdin),
                },
            );
            process
        };

        let result = process.submit_prompt(
            session,
            prompt,
            self.timeouts.prompt_first_update,
            self.timeouts.prompt_total,
        );
        let mut state = self.inner.lock().expect("ACP process pool lock poisoned");
        state.busy_processes.remove(&runner.runner_id);
        if result.is_ok() {
            state.processes.insert(runner.runner_id.clone(), process);
        } else {
            process.terminate();
        }
        result
    }

    pub fn cancel(&self, runner: &AcpRunnerRef, session: &AcpSessionRef) -> Result<()> {
        validate_session_ref(runner, session)?;
        let mut state = self.inner.lock().expect("ACP process pool lock poisoned");
        if let Some(process) = state.busy_processes.get(&runner.runner_id) {
            if process.runner.connection_epoch != runner.connection_epoch {
                bail!("ACP runner connection epoch does not match active process");
            }
            let stdin = Arc::clone(&process.stdin);
            drop(state);
            return write_value(
                &stdin,
                &super::protocol::cancel_notification(&session.session_id)?,
            );
        }
        let process = active_process(&mut state, runner)?;
        process.write_value(&super::protocol::cancel_notification(&session.session_id)?)
    }

    pub fn terminate(&self, runner: &AcpRunnerRef) -> Result<()> {
        let mut state = self.inner.lock().expect("ACP process pool lock poisoned");
        if state.busy_processes.contains_key(&runner.runner_id) {
            bail!("ACP runner has an in-flight prompt; cancel it before terminating");
        }
        let Some(process) = state.processes.get(&runner.runner_id) else {
            return Ok(());
        };
        if process.runner.connection_epoch != runner.connection_epoch {
            bail!("ACP runner connection epoch does not match active process");
        }
        if let Some(mut process) = state.processes.remove(&runner.runner_id) {
            process.terminate();
        }
        Ok(())
    }
}

impl Default for AcpProcessPool {
    fn default() -> Self {
        Self::new(AcpConnectionTimeouts::default())
    }
}

fn active_process<'a>(
    state: &'a mut AcpPoolState,
    runner: &AcpRunnerRef,
) -> Result<&'a mut AcpProcess> {
    if state.busy_processes.contains_key(&runner.runner_id) {
        bail!("ACP runner already has an in-flight prompt");
    }
    let process = state
        .processes
        .get_mut(&runner.runner_id)
        .context("ACP process is not running")?;
    if process.runner.connection_epoch != runner.connection_epoch {
        bail!("ACP runner connection epoch does not match active process");
    }
    if !process.is_running() {
        bail!("ACP process exited: {}", process.stderr_summary());
    }
    Ok(process)
}

fn take_active_process(state: &mut AcpPoolState, runner: &AcpRunnerRef) -> Result<AcpProcess> {
    active_process(state, runner)?;
    state
        .processes
        .remove(&runner.runner_id)
        .context("ACP process is not running")
}

fn terminate_failed_process(state: &mut AcpPoolState, runner: &AcpRunnerRef) {
    if state
        .processes
        .get(&runner.runner_id)
        .is_some_and(|process| process.runner.connection_epoch == runner.connection_epoch)
    {
        state.processes.remove(&runner.runner_id);
    }
}

fn validate_session_ref(runner: &AcpRunnerRef, session: &AcpSessionRef) -> Result<()> {
    if session.runner_id != runner.runner_id {
        bail!("ACP session runner does not match active runner");
    }
    if session.connection_epoch != runner.connection_epoch {
        bail!("ACP session connection epoch does not match active runner");
    }
    Ok(())
}

fn validate_process_spec(spec: &AcpProcessSpec) -> Result<()> {
    if spec.runner_id.trim().is_empty() {
        bail!("ACP runner_id must not be empty");
    }
    if spec.program.as_os_str().is_empty() {
        bail!("ACP program must not be empty");
    }
    if !spec.cwd.is_absolute() {
        bail!("ACP process cwd must be an absolute path");
    }
    Ok(())
}

#[derive(Debug)]
struct AcpProcess {
    runner: AcpRunnerRef,
    child: Child,
    stdin: Arc<Mutex<ChildStdin>>,
    stdout_rx: mpsc::Receiver<String>,
    stderr_lines: Arc<Mutex<Vec<String>>>,
    next_id: u64,
    permission_policy: AcpPermissionPolicy,
    process_group_isolated: bool,
}

impl AcpProcess {
    fn spawn(spec: &AcpProcessSpec, connection_epoch: u64) -> Result<Self> {
        let mut command = Command::new(&spec.program);
        command
            .args(&spec.args)
            .current_dir(&spec.cwd)
            .env_clear()
            .envs(&spec.env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let process_group_isolated = configure_process_group(&mut command);
        let mut child = command.spawn().with_context(|| {
            format!(
                "spawn ACP program {}",
                sanitize_program_name(&spec.program.display().to_string())
            )
        })?;
        let stdin = Arc::new(Mutex::new(child.stdin.take().context("open ACP stdin")?));
        let stdout = child.stdout.take().context("open ACP stdout")?;
        let stderr = child.stderr.take().context("open ACP stderr")?;
        let (stdout_tx, stdout_rx) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if stdout_tx.send(line).is_err() {
                    break;
                }
            }
        });
        let stderr_lines = Arc::new(Mutex::new(Vec::new()));
        let stderr_for_thread = Arc::clone(&stderr_lines);
        thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if let Ok(mut lines) = stderr_for_thread.lock() {
                    lines.push(redact_line(&line));
                    let overflow = lines.len().saturating_sub(20);
                    if overflow > 0 {
                        lines.drain(0..overflow);
                    }
                }
            }
        });
        Ok(Self {
            runner: AcpRunnerRef {
                runner_id: spec.runner_id.clone(),
                connection_epoch,
                protocol_version: ACP_PROTOCOL_VERSION,
            },
            child,
            stdin,
            stdout_rx,
            stderr_lines,
            next_id: 1,
            permission_policy: spec.permission_policy,
            process_group_isolated,
        })
    }

    fn initialize(&mut self, timeout: Duration) -> Result<()> {
        let request_id = self.next_request_id();
        let result = self.call(
            request_id,
            initialize_request(request_id),
            timeout,
            "initialize",
        )?;
        let protocol_version = result
            .get("protocolVersion")
            .and_then(Value::as_u64)
            .context("ACP initialize response did not include protocolVersion")?;
        if protocol_version != ACP_PROTOCOL_VERSION {
            bail!("ACP initialize negotiated unsupported protocol version {protocol_version}");
        }
        self.runner.protocol_version = protocol_version;
        Ok(())
    }

    fn call(
        &mut self,
        request_id: u64,
        request: Value,
        timeout: Duration,
        stage: &str,
    ) -> Result<Value> {
        self.write_value(&request)?;
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = remaining_timeout(deadline, &format!("ACP {stage} timed out"))?;
            let message = self.read_message(remaining, &format!("ACP {stage} timed out"))?;
            match message {
                AcpInbound::Response { id, result, error } if id == request_id.to_string() => {
                    return response_result(stage, result, error);
                }
                AcpInbound::PermissionRequest { .. } => {
                    self.respond_permission(&message)?;
                }
                AcpInbound::Request { method, .. } => {
                    bail!("unsupported ACP reverse request: {}", redact_line(&method));
                }
                _ => {}
            }
        }
    }

    fn submit_prompt(
        &mut self,
        session: &AcpSessionRef,
        prompt: &str,
        first_update_timeout: Duration,
        total_timeout: Duration,
    ) -> Result<AcpPromptOutcome> {
        let request_id = self.next_request_id();
        self.write_value(&prompt_request(request_id, &session.session_id, prompt)?)?;
        let started_at = Instant::now();
        let first_update_deadline = started_at + first_update_timeout;
        let total_deadline = started_at + total_timeout;
        let mut saw_update = false;
        let mut permission_requests = 0;
        let mut text = AcpTextAccumulator::new(&session.session_id);
        loop {
            let now = Instant::now();
            let (deadline, timeout_error) = if saw_update {
                (total_deadline, "ACP prompt total timed out")
            } else if first_update_deadline <= total_deadline {
                (first_update_deadline, "ACP prompt first update timed out")
            } else {
                (total_deadline, "ACP prompt total timed out")
            };
            let remaining = deadline
                .checked_duration_since(now)
                .filter(|remaining| !remaining.is_zero())
                .with_context(|| timeout_error.to_string())?;
            let message = self.read_message(remaining, timeout_error)?;
            match message {
                AcpInbound::Response { ref id, .. } if id == &request_id.to_string() => {
                    let AcpInbound::Response { result, error, .. } = message else {
                        unreachable!()
                    };
                    let result = response_result("session/prompt", result, error)?;
                    let stop_reason = result
                        .get("stopReason")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .context("ACP session/prompt response did not include stopReason")?
                        .to_string();
                    return Ok(AcpPromptOutcome {
                        session: session.clone(),
                        stop_reason,
                        final_text: text.final_text(),
                        permission_requests,
                    });
                }
                AcpInbound::SessionUpdate { ref session_id, .. }
                    if session_id == &session.session_id =>
                {
                    saw_update = true;
                    text.observe(&message);
                }
                AcpInbound::PermissionRequest { ref session_id, .. } => {
                    if session_id == &session.session_id {
                        permission_requests += 1;
                    }
                    self.respond_permission(&message)?;
                }
                AcpInbound::Request { method, .. } => {
                    bail!("unsupported ACP reverse request: {}", redact_line(&method));
                }
                _ => {}
            }
        }
    }

    fn respond_permission(&mut self, request: &AcpInbound) -> Result<()> {
        let response = permission_response(request, self.permission_policy)?;
        self.write_value(&response)
    }

    fn write_value(&self, value: &Value) -> Result<()> {
        write_value(&self.stdin, value)
    }

    fn read_message(&mut self, timeout: Duration, timeout_error: &str) -> Result<AcpInbound> {
        let line = match self.stdout_rx.recv_timeout(timeout) {
            Ok(line) => line,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                bail!("{timeout_error}: {}", self.stderr_summary())
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                bail!("ACP process exited: {}", self.stderr_summary())
            }
        };
        parse_inbound(&line)
            .with_context(|| format!("parse ACP stdout frame: {}", redact_line(&line)))
    }

    fn next_request_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    fn terminate(&mut self) {
        kill_child_process_tree_best_effort(&mut self.child, self.process_group_isolated);
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
            .take(20)
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

impl Drop for AcpProcess {
    fn drop(&mut self) {
        self.terminate();
    }
}

fn write_value(stdin: &Arc<Mutex<ChildStdin>>, value: &Value) -> Result<()> {
    let mut stdin = stdin
        .lock()
        .map_err(|_| anyhow!("ACP stdin lock poisoned"))?;
    writeln!(stdin, "{value}").context("write ACP JSON-RPC frame")?;
    stdin.flush().context("flush ACP JSON-RPC frame")
}

fn response_result(stage: &str, result: Option<Value>, error: Option<Value>) -> Result<Value> {
    if let Some(error) = error {
        let summary = error
            .get("message")
            .and_then(Value::as_str)
            .map(redact_line)
            .unwrap_or_else(|| redact_line(&error.to_string()));
        bail!("ACP {stage} failed: {summary}");
    }
    result.with_context(|| format!("ACP {stage} response did not include result"))
}

fn remaining_timeout(deadline: Instant, timeout_error: &str) -> Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|remaining| !remaining.is_zero())
        .with_context(|| timeout_error.to_string())
}

fn sanitize_program_name(value: &str) -> String {
    if contains_sensitive_marker(value) {
        "<redacted>".to_string()
    } else {
        value.to_string()
    }
}

pub fn redact_line(line: &str) -> String {
    if contains_sensitive_marker(line) {
        return "<redacted>".to_string();
    }
    let mut sanitized = line.replace(['\r', '\n'], " ");
    const MAX_SUMMARY_CHARS: usize = 500;
    if sanitized.chars().count() > MAX_SUMMARY_CHARS {
        sanitized = sanitized.chars().take(MAX_SUMMARY_CHARS).collect();
        sanitized.push('…');
    }
    sanitized
}

fn contains_sensitive_marker(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    [
        "api_key",
        "apikey",
        "authorization",
        "bearer ",
        "password",
        "secret",
        "token",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
        || contains_token_like_value(value)
}

fn contains_token_like_value(value: &str) -> bool {
    let bytes = value.as_bytes();
    for index in 0..bytes.len().saturating_sub(2) {
        if !bytes[index].eq_ignore_ascii_case(&b's')
            || !bytes[index + 1].eq_ignore_ascii_case(&b'k')
            || bytes[index + 2] != b'-'
            || index > 0 && (bytes[index - 1].is_ascii_alphanumeric() || bytes[index - 1] == b'_')
        {
            continue;
        }
        let token_length = bytes[index + 3..]
            .iter()
            .take_while(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            .count();
        if token_length >= 12 {
            return true;
        }
    }
    false
}

#[cfg(unix)]
fn configure_process_group(command: &mut Command) -> bool {
    use std::os::unix::process::CommandExt;

    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        });
    }
    true
}

#[cfg(not(unix))]
fn configure_process_group(_command: &mut Command) -> bool {
    false
}

#[cfg(unix)]
fn kill_child_process_tree_best_effort(child: &mut Child, process_group_isolated: bool) {
    if let Ok(None) = child.try_wait() {
        if process_group_isolated {
            unsafe {
                libc::killpg(child.id() as libc::pid_t, libc::SIGTERM);
            }
            thread::sleep(Duration::from_millis(50));
            if let Ok(None) = child.try_wait() {
                unsafe {
                    libc::killpg(child.id() as libc::pid_t, libc::SIGKILL);
                }
            }
        } else {
            let _ = child.kill();
        }
        let _ = child.wait();
    }
}

#[cfg(not(unix))]
fn kill_child_process_tree_best_effort(child: &mut Child, _process_group_isolated: bool) {
    if let Ok(None) = child.try_wait() {
        let _ = child.kill();
        let _ = child.wait();
    }
}
