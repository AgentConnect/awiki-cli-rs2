use std::borrow::Cow;
use std::process::Command;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};

pub mod claude_code;
pub mod codex;
mod process;
pub(crate) mod status;

use crate::cli_wrapper::CliWrapperRequest;
use crate::local_rpc::RuntimeRpcRequest;
use crate::runtime::{
    GenericCliRouteSession, RuntimeInstallStatus, RuntimeLaunchContext, RuntimeLaunchOutcome,
    RuntimePlugin, RuntimeRunStatus,
};
use crate::state::CliRuntimeProfileRecord;

use self::process::{ManagedChild, DEFAULT_GENERIC_CLI_RUN_TIMEOUT};

pub const GENERIC_CLI_RUNTIME_PLUGIN_ID: &str = crate::agent::GENERIC_CLI_RUNTIME_PLUGIN_ID;
pub const CODEX_CLI_DRIVER_ID: &str = crate::agent::CODEX_CLI_DRIVER_ID;
pub const CLAUDE_CODE_CLI_DRIVER_ID: &str = crate::agent::CLAUDE_CODE_CLI_DRIVER_ID;
pub const GEMINI_CLI_DRIVER_ID: &str = crate::agent::GEMINI_CLI_DRIVER_ID;
pub const COMMAND_CLI_DRIVER_ID: &str = crate::agent::COMMAND_CLI_DRIVER_ID;
const MAX_NATIVE_SESSION_ID_LEN: usize = 128;
pub const OUTPUT_SANITIZER_VERSION: &str = "generic-cli-output-sanitizer-v1";
pub const DEFAULT_SANITIZED_OUTPUT_MAX_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SanitizedCliOutput {
    pub text: String,
    pub raw_bytes: usize,
    pub text_bytes: usize,
    pub output_sanitized: bool,
    pub output_truncated: bool,
    pub non_utf8_replaced: bool,
    pub token_redacted: bool,
    pub max_text_bytes: usize,
}

impl SanitizedCliOutput {
    pub fn metadata_json(&self) -> Value {
        json!({
            "sanitizer_version": OUTPUT_SANITIZER_VERSION,
            "raw_bytes": self.raw_bytes,
            "text_bytes": self.text_bytes,
            "output_sanitized": self.output_sanitized,
            "output_truncated": self.output_truncated,
            "non_utf8_replaced": self.non_utf8_replaced,
            "token_redacted": self.token_redacted,
            "max_text_bytes": self.max_text_bytes,
        })
    }
}

pub fn sanitize_cli_output_text(
    text: &str,
    runtime_rpc_token: &str,
    max_text_bytes: usize,
) -> SanitizedCliOutput {
    sanitize_cli_output_bytes(text.as_bytes(), runtime_rpc_token, max_text_bytes)
}

pub fn sanitize_cli_output_bytes(
    bytes: &[u8],
    runtime_rpc_token: &str,
    max_text_bytes: usize,
) -> SanitizedCliOutput {
    let raw_bytes = bytes.len();
    let decoded = String::from_utf8_lossy(bytes);
    let non_utf8_replaced = matches!(&decoded, Cow::Owned(_));
    let (mut text, removed_controls) = strip_ansi_and_controls(&decoded);
    let mut token_redacted = false;
    if !runtime_rpc_token.is_empty() && text.contains(runtime_rpc_token) {
        text = text.replace(runtime_rpc_token, "<redacted-runtime-rpc-token>");
        token_redacted = true;
    }
    let (text, output_truncated) = truncate_utf8_to_bytes(text, max_text_bytes);
    let text_bytes = text.len();
    SanitizedCliOutput {
        text,
        raw_bytes,
        text_bytes,
        output_sanitized: token_redacted || non_utf8_replaced || removed_controls,
        output_truncated,
        non_utf8_replaced,
        token_redacted,
        max_text_bytes,
    }
}

pub fn write_sanitized_cli_output(
    path: &std::path::Path,
    bytes: &[u8],
    runtime_rpc_token: &str,
) -> Result<SanitizedCliOutput> {
    let sanitized =
        sanitize_cli_output_bytes(bytes, runtime_rpc_token, DEFAULT_SANITIZED_OUTPUT_MAX_BYTES);
    std::fs::write(path, sanitized.text.as_bytes())
        .with_context(|| format!("write sanitized CLI output {}", path.display()))?;
    Ok(sanitized)
}

pub fn sanitize_cli_output_file(
    path: &std::path::Path,
    runtime_rpc_token: &str,
) -> Result<Option<SanitizedCliOutput>> {
    if !path.exists() {
        return Ok(None);
    }
    let bytes =
        std::fs::read(path).with_context(|| format!("read CLI output file {}", path.display()))?;
    let sanitized = sanitize_cli_output_bytes(
        &bytes,
        runtime_rpc_token,
        DEFAULT_SANITIZED_OUTPUT_MAX_BYTES,
    );
    std::fs::write(path, sanitized.text.as_bytes())
        .with_context(|| format!("write sanitized CLI output {}", path.display()))?;
    Ok(Some(sanitized))
}

fn truncate_utf8_to_bytes(text: String, max_text_bytes: usize) -> (String, bool) {
    if text.len() <= max_text_bytes {
        return (text, false);
    }
    let mut end = max_text_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].to_string(), true)
}

fn strip_ansi_and_controls(input: &str) -> (String, bool) {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum State {
        Normal,
        Escape,
        Csi,
        Osc,
        OscEscape,
        StringControl,
        StringControlEscape,
    }

    let mut output = String::with_capacity(input.len());
    let mut state = State::Normal;
    let mut sanitized = false;
    for ch in input.chars() {
        match state {
            State::Normal => match ch {
                '\u{1b}' => {
                    sanitized = true;
                    state = State::Escape;
                }
                '\n' | '\t' => output.push(ch),
                '\r' => {
                    sanitized = true;
                    output.push('\n');
                }
                ch if ch.is_control() => {
                    sanitized = true;
                }
                _ => output.push(ch),
            },
            State::Escape => {
                sanitized = true;
                state = match ch {
                    '[' => State::Csi,
                    ']' => State::Osc,
                    'P' | '_' | '^' | 'X' => State::StringControl,
                    _ => State::Normal,
                };
            }
            State::Csi => {
                sanitized = true;
                let code = ch as u32;
                if (0x40..=0x7e).contains(&code) {
                    state = State::Normal;
                }
            }
            State::Osc => {
                sanitized = true;
                match ch {
                    '\u{7}' => state = State::Normal,
                    '\u{1b}' => state = State::OscEscape,
                    _ => {}
                }
            }
            State::OscEscape => {
                sanitized = true;
                state = if ch == '\\' {
                    State::Normal
                } else if ch == '\u{1b}' {
                    State::OscEscape
                } else {
                    State::Osc
                };
            }
            State::StringControl => {
                sanitized = true;
                match ch {
                    '\u{7}' => state = State::Normal,
                    '\u{1b}' => state = State::StringControlEscape,
                    _ => {}
                }
            }
            State::StringControlEscape => {
                sanitized = true;
                state = if ch == '\\' {
                    State::Normal
                } else if ch == '\u{1b}' {
                    State::StringControlEscape
                } else {
                    State::StringControl
                };
            }
        }
    }
    (output, sanitized)
}

pub trait GenericCliDriver {
    fn check_install_status(&self) -> Result<RuntimeInstallStatus>;
    fn run(&self, invocation: GenericCliInvocation) -> Result<GenericCliExit>;
}

pub fn validate_native_session_id(driver_id: &str, native_session_id: &str) -> bool {
    let id = native_session_id.trim();
    if id.is_empty() || id.len() > MAX_NATIVE_SESSION_ID_LEN || id != native_session_id {
        return false;
    }
    if matches!(id, "." | "..") || id.contains("..") {
        return false;
    }
    if id
        .bytes()
        .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
    {
        return false;
    }
    let allowed: fn(u8) -> bool = match driver_id {
        CODEX_CLI_DRIVER_ID => {
            |byte: u8| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')
        }
        CLAUDE_CODE_CLI_DRIVER_ID => {
            |byte: u8| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':')
        }
        _ => return false,
    };
    id.bytes().all(allowed)
}

pub fn validate_native_session_source(driver_id: &str, native_session_source: &str) -> bool {
    matches!(
        (driver_id, native_session_source),
        (CODEX_CLI_DRIVER_ID, "json_event")
            | (CODEX_CLI_DRIVER_ID, "resume_id")
            | (CODEX_CLI_DRIVER_ID, "resume_last")
            | (CLAUDE_CODE_CLI_DRIVER_ID, "stream_json")
            | (CLAUDE_CODE_CLI_DRIVER_ID, "generated_session_id")
            | (CLAUDE_CODE_CLI_DRIVER_ID, "resume_id")
    )
}

#[derive(Clone, PartialEq, Eq)]
pub struct GenericCliInvocation {
    pub run_id: String,
    pub task_id: String,
    pub message_id: String,
    pub conversation_id: Option<String>,
    pub task_text: String,
    pub agent_did: String,
    pub runtime_profile_id: String,
    pub workspace_root: Option<std::path::PathBuf>,
    pub workspace_instance: Option<crate::workspace::WorkspaceInstance>,
    pub route_session: Option<GenericCliRouteSession>,
    pub runtime_temp_dir: Option<std::path::PathBuf>,
    pub runtime_rpc_token: String,
    pub local_socket_path: Option<std::path::PathBuf>,
    pub callbacks: Vec<RuntimeRpcRequest>,
}

impl std::fmt::Debug for GenericCliInvocation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GenericCliInvocation")
            .field("run_id", &self.run_id)
            .field("task_id", &self.task_id)
            .field("message_id", &self.message_id)
            .field("conversation_id", &self.conversation_id)
            .field("task_text", &"<redacted-task-text>")
            .field("agent_did", &self.agent_did)
            .field("runtime_profile_id", &self.runtime_profile_id)
            .field("workspace_root", &self.workspace_root)
            .field("workspace_instance", &self.workspace_instance)
            .field(
                "route_session",
                &self.route_session.as_ref().map(|session| {
                    serde_json::json!({
                        "route_key_hash": session.route_key_hash,
                        "status": session.status,
                        "native_session_id_present": session.native_session_id.is_some(),
                    })
                }),
            )
            .field("runtime_temp_dir", &self.runtime_temp_dir)
            .field("runtime_rpc_token", &"<redacted>")
            .field("local_socket_path", &self.local_socket_path)
            .field("callbacks", &self.callbacks)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenericCliExit {
    pub exit_code: i32,
    pub status: RuntimeRunStatus,
    pub callbacks: Vec<RuntimeRpcRequest>,
    pub metadata: Value,
}

impl GenericCliExit {
    pub fn from_exit_code(exit_code: i32) -> Self {
        let status = if exit_code == 0 {
            RuntimeRunStatus::Finished
        } else {
            RuntimeRunStatus::Failed
        };
        Self {
            exit_code,
            status,
            callbacks: Vec::new(),
            metadata: serde_json::json!({}),
        }
    }
}

#[derive(Debug, Clone)]
pub struct GenericCliRuntimePlugin<D> {
    driver: D,
}

impl<D> GenericCliRuntimePlugin<D> {
    pub fn new(driver: D) -> Self {
        Self { driver }
    }
}

impl<D> RuntimePlugin for GenericCliRuntimePlugin<D>
where
    D: GenericCliDriver,
{
    fn plugin_id(&self) -> &str {
        "generic-cli"
    }

    fn check_install_status(&self) -> Result<RuntimeInstallStatus> {
        self.driver.check_install_status()
    }

    fn launch_run(&self, context: RuntimeLaunchContext) -> Result<RuntimeLaunchOutcome> {
        let token = context.runtime_rpc_token.as_str().to_string();
        let callbacks = vec![
            CliWrapperRequest::task_status(
                token.clone(),
                context.task.task_id.clone(),
                "running",
                "runtime started",
            )
            .into_rpc_request(),
            CliWrapperRequest::task_finish(token, context.task.task_id.clone(), "runtime finished")
                .into_rpc_request(),
        ];
        let exit = self.driver.run(GenericCliInvocation {
            run_id: context.run.run_id.clone(),
            task_id: context.task.task_id.clone(),
            message_id: context
                .task
                .task_id
                .strip_prefix("task_")
                .unwrap_or(&context.task.task_id)
                .to_string(),
            conversation_id: context.task.conversation_id.clone(),
            task_text: context.task.text.clone(),
            workspace_root: context.workspace_root.clone(),
            workspace_instance: context.workspace_instance.clone(),
            route_session: context.cli_route_session.clone(),
            runtime_temp_dir: context.runtime_temp_dir.clone(),
            agent_did: context.run.agent_did.clone(),
            runtime_profile_id: context.run.runtime_profile_id.clone(),
            runtime_rpc_token: context.runtime_rpc_token.as_str().to_string(),
            local_socket_path: context.local_socket_path.clone(),
            callbacks: callbacks.clone(),
        })?;
        Ok(RuntimeLaunchOutcome {
            run_id: context.run.run_id,
            status: exit.status,
            exit_code: Some(exit.exit_code),
            callbacks: exit.callbacks,
            metadata: exit.metadata,
        })
    }
}

#[derive(Debug, Clone)]
pub struct GenericCliDriverRegistry {
    cli_profile: CliRuntimeProfileRecord,
}

impl GenericCliDriverRegistry {
    pub fn new(cli_profile: CliRuntimeProfileRecord) -> Self {
        Self { cli_profile }
    }

    pub fn driver_id(&self) -> &str {
        &self.cli_profile.driver_id
    }
}

impl RuntimePlugin for GenericCliDriverRegistry {
    fn plugin_id(&self) -> &str {
        GENERIC_CLI_RUNTIME_PLUGIN_ID
    }

    fn check_install_status(&self) -> Result<RuntimeInstallStatus> {
        match self.cli_profile.driver_id.as_str() {
            COMMAND_CLI_DRIVER_ID => {
                command_driver_from_profile(&self.cli_profile)?.check_install_status()
            }
            CLAUDE_CODE_CLI_DRIVER_ID => {
                claude_code::ClaudeCodeDriver::from_profile(&self.cli_profile)?
                    .check_install_status()
            }
            CODEX_CLI_DRIVER_ID => {
                codex::CodexDriver::from_profile(&self.cli_profile)?.check_install_status()
            }
            GEMINI_CLI_DRIVER_ID => Ok(RuntimeInstallStatus {
                installed: false,
                detail: Some(format!(
                    "generic-cli driver {} is not implemented yet",
                    self.cli_profile.driver_id
                )),
            }),
            other => bail!("unsupported generic-cli driver_id: {other}"),
        }
    }

    fn launch_run(&self, context: RuntimeLaunchContext) -> Result<RuntimeLaunchOutcome> {
        match self.cli_profile.driver_id.as_str() {
            COMMAND_CLI_DRIVER_ID => {
                GenericCliRuntimePlugin::new(command_driver_from_profile(&self.cli_profile)?)
                    .launch_run(context)
            }
            CODEX_CLI_DRIVER_ID => {
                GenericCliRuntimePlugin::new(codex::CodexDriver::from_profile(&self.cli_profile)?)
                    .launch_run(context)
            }
            CLAUDE_CODE_CLI_DRIVER_ID => GenericCliRuntimePlugin::new(
                claude_code::ClaudeCodeDriver::from_profile(&self.cli_profile)?,
            )
            .launch_run(context),
            GEMINI_CLI_DRIVER_ID => {
                bail!(
                    "generic-cli driver {} is not implemented yet",
                    self.cli_profile.driver_id
                )
            }
            other => bail!("unsupported generic-cli driver_id: {other}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CommandGenericCliDriver {
    program: std::path::PathBuf,
    args: Vec<String>,
    cli_wrapper: String,
    run_timeout: Duration,
}

impl CommandGenericCliDriver {
    pub fn new(program: impl Into<std::path::PathBuf>, args: Vec<String>) -> Self {
        Self {
            program: program.into(),
            args,
            cli_wrapper: "library:awiki_deamon::cli_wrapper".to_string(),
            run_timeout: DEFAULT_GENERIC_CLI_RUN_TIMEOUT,
        }
    }

    pub fn with_cli_wrapper(mut self, cli_wrapper: impl Into<String>) -> Self {
        self.cli_wrapper = cli_wrapper.into();
        self
    }

    pub fn with_run_timeout(mut self, run_timeout: Duration) -> Self {
        self.run_timeout = run_timeout;
        self
    }
}

impl GenericCliDriver for CommandGenericCliDriver {
    fn check_install_status(&self) -> Result<RuntimeInstallStatus> {
        Ok(RuntimeInstallStatus {
            installed: self.program.exists(),
            detail: Some(self.program.display().to_string()),
        })
    }

    fn run(&self, invocation: GenericCliInvocation) -> Result<GenericCliExit> {
        if !self.program.exists() {
            bail!("generic CLI driver program does not exist");
        }
        let mut command = Command::new(&self.program);
        command.args(&self.args);
        if let Some(workspace_root) = invocation.workspace_root.as_ref() {
            command.current_dir(workspace_root);
        }
        let Some(socket_path) = invocation.local_socket_path.as_ref() else {
            bail!("generic CLI command driver requires daemon local RPC socket");
        };
        command
            .env("AWIKI_DAEMON_RUN_ID", &invocation.run_id)
            .env("AWIKI_DAEMON_TASK_ID", &invocation.task_id)
            .env("AWIKI_DAEMON_AGENT_DID", &invocation.agent_did)
            .env(
                "AWIKI_DAEMON_RUNTIME_PROFILE_ID",
                &invocation.runtime_profile_id,
            )
            .env("AWIKI_DAEMON_SOCKET", socket_path)
            .env("AWIKI_DAEMON_CLI_WRAPPER", &self.cli_wrapper)
            .env(
                "AWIKI_DAEMON_RUNTIME_RPC_TOKEN",
                &invocation.runtime_rpc_token,
            )
            .env_remove("AWIKI_DAEMON_TASK_TEXT");
        let managed = ManagedChild::spawn(&mut command, "spawn generic CLI runtime")?;
        let output = match managed.wait_timeout("wait for generic CLI runtime", self.run_timeout) {
            Ok(output) => output,
            Err(error) => {
                if let Some(timeout) = error.downcast_ref::<process::ManagedChildTimeoutError>() {
                    return Ok(GenericCliExit {
                        exit_code: 124,
                        status: RuntimeRunStatus::Failed,
                        callbacks: Vec::new(),
                        metadata: serde_json::json!({
                            "driver_id": "generic-cli",
                            "error_code": "generic_cli_timeout",
                            "error_summary": timeout.to_string(),
                            "next_action": "manual_review_required",
                            "process": {
                                "timed_out": true,
                                "timeout_ms": timeout.timeout_ms(),
                                "management": timeout.metadata_json(),
                            },
                        }),
                    });
                }
                return Err(error);
            }
        };
        let mut exit = GenericCliExit::from_exit_code(output.output.status.code().unwrap_or(1));
        exit.metadata = serde_json::json!({
            "process": output.metadata_json(),
        });
        Ok(exit)
    }
}

#[derive(Debug, Clone, Default)]
pub struct TestGenericCliDriver {
    pub exit_code: i32,
}

impl GenericCliDriver for TestGenericCliDriver {
    fn check_install_status(&self) -> Result<RuntimeInstallStatus> {
        Ok(RuntimeInstallStatus {
            installed: true,
            detail: Some("test generic CLI driver".to_string()),
        })
    }

    fn run(&self, invocation: GenericCliInvocation) -> Result<GenericCliExit> {
        if self.exit_code == 0 {
            Ok(GenericCliExit {
                exit_code: self.exit_code,
                status: RuntimeRunStatus::Finished,
                callbacks: invocation.callbacks,
                metadata: serde_json::json!({}),
            })
        } else {
            Ok(GenericCliExit {
                exit_code: self.exit_code,
                status: RuntimeRunStatus::Failed,
                callbacks: Vec::new(),
                metadata: serde_json::json!({
                    "driver_id": "generic-cli",
                    "error_code": "generic_cli_failed",
                    "error_summary": format!("generic CLI test driver exited with status {}", self.exit_code),
                    "next_action": "manual_review_required",
                }),
            })
        }
    }
}

fn command_driver_from_profile(
    profile: &CliRuntimeProfileRecord,
) -> Result<CommandGenericCliDriver> {
    let program = profile
        .binary_path
        .clone()
        .or_else(|| {
            profile
                .driver_config_json
                .get("program")
                .and_then(Value::as_str)
                .map(std::path::PathBuf::from)
        })
        .context("command generic-cli driver requires binary_path or driver_config.program")?;
    let args = string_array(profile.driver_config_json.get("args"))?;
    let cli_wrapper = profile
        .driver_config_json
        .get("cli_wrapper")
        .and_then(Value::as_str)
        .unwrap_or("library:awiki_deamon::cli_wrapper");
    let run_timeout = duration_ms_field(
        &profile.driver_config_json,
        "run_timeout_ms",
        DEFAULT_GENERIC_CLI_RUN_TIMEOUT,
    );
    Ok(CommandGenericCliDriver::new(program, args)
        .with_cli_wrapper(cli_wrapper)
        .with_run_timeout(run_timeout))
}

fn string_array(value: Option<&Value>) -> Result<Vec<String>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let Some(items) = value.as_array() else {
        bail!("generic-cli command args must be an array");
    };
    items
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_string)
                .context("generic-cli command args must be strings")
        })
        .collect()
}

fn duration_ms_field(value: &Value, field: &str, default: Duration) -> Duration {
    value
        .get(field)
        .and_then(Value::as_u64)
        .filter(|millis| *millis > 0)
        .map(Duration::from_millis)
        .unwrap_or(default)
}
