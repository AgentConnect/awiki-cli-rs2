use std::borrow::Cow;
use std::path::{Path, PathBuf};
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
use crate::security::runtime_token::current_time_millis;
use crate::state::CliRuntimeProfileRecord;
use crate::workspace::WorkspaceInstance;

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

pub(crate) fn route_session_metadata(session: Option<&GenericCliRouteSession>) -> Option<Value> {
    session.map(|session| {
        json!({
            "route_key_hash": session.route_key_hash,
            "status": session.status,
            "last_message_id_present": session.last_message_id.is_some(),
            "last_run_id_present": session.last_run_id.is_some(),
            "synthetic_session_id_present": session.synthetic_session_id.is_some(),
            "native_session_id_present": session.native_session_id.is_some(),
        })
    })
}

pub(crate) fn workspace_metadata(
    workspace_binding_root: &Path,
    workspace_instance_path: &Path,
    workspace_instance: Option<&WorkspaceInstance>,
) -> Value {
    json!({
        "workspace_root": workspace_binding_root,
        "workspace_instance_path": workspace_instance_path,
        "workspace_mode": workspace_instance.map(|instance| instance.workspace_mode.as_str()),
        "is_security_boundary": workspace_instance.map(|instance| instance.is_security_boundary),
        "isolation_note": workspace_instance.map(|instance| instance.isolation_note.as_str()),
        "cleanup_policy": workspace_instance.map(|instance| instance.cleanup_policy),
        "base_ref": workspace_instance.and_then(|instance| instance.base_ref.as_deref()),
        "branch_name": workspace_instance.and_then(|instance| instance.branch_name.as_deref()),
    })
}

pub(crate) fn output_sanitizer_metadata(
    stdout: &SanitizedCliOutput,
    stderr: &SanitizedCliOutput,
    final_output: Option<Value>,
) -> Value {
    json!({
        "stdout": stdout.metadata_json(),
        "stderr": stderr.metadata_json(),
        "final_output": final_output,
    })
}

pub(crate) fn output_metadata(
    output_dir: &Path,
    stdout_path: &Path,
    stderr_path: &Path,
    jsonl_path: &Path,
    final_output_path: &Path,
    sanitizer: Option<Value>,
) -> Value {
    let mut metadata = serde_json::Map::new();
    metadata.insert("output_dir".to_string(), json!(output_dir));
    metadata.insert("stdout_path".to_string(), json!(stdout_path));
    metadata.insert("stderr_path".to_string(), json!(stderr_path));
    metadata.insert("jsonl_path".to_string(), json!(jsonl_path));
    metadata.insert("final_output_path".to_string(), json!(final_output_path));
    if let Some(sanitizer) = sanitizer {
        metadata.insert("sanitizer".to_string(), sanitizer);
    }
    Value::Object(metadata)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenericCliRunPaths {
    pub output_dir: PathBuf,
    pub final_output_path: PathBuf,
    pub stdout_path: PathBuf,
    pub stderr_path: PathBuf,
    pub jsonl_path: PathBuf,
}

pub(crate) fn generic_cli_run_paths(
    invocation: &GenericCliInvocation,
    configured_output_dir: Option<&Path>,
    driver_dir_name: &str,
    stdout_filename: &str,
    stderr_filename: &str,
    observation_filename: &str,
) -> GenericCliRunPaths {
    let output_dir = configured_output_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(|| {
            if let Some(route_session) = invocation.route_session.as_ref() {
                return route_session
                    .session_dir
                    .join("runs")
                    .join(sanitize_path_component(&invocation.run_id));
            }
            invocation
                .runtime_temp_dir
                .clone()
                .unwrap_or_else(std::env::temp_dir)
                .join("awiki-deamon")
                .join("generic-cli")
                .join(driver_dir_name)
                .join(sanitize_path_component(&invocation.run_id))
        });
    let final_output_path = if configured_output_dir.is_some() {
        output_dir.join("final-output.txt")
    } else {
        invocation
            .route_session
            .as_ref()
            .map(|route_session| route_session.session_dir.join("last-output.md"))
            .unwrap_or_else(|| output_dir.join("final-output.txt"))
    };
    GenericCliRunPaths {
        final_output_path,
        stdout_path: output_dir.join(stdout_filename),
        stderr_path: output_dir.join(stderr_filename),
        jsonl_path: output_dir.join(observation_filename),
        output_dir,
    }
}

pub(crate) struct GenericCliPromptEnvelope<'a> {
    pub(crate) invocation: &'a GenericCliInvocation,
    pub(crate) workspace_root: &'a Path,
    pub(crate) driver_id: &'a str,
    pub(crate) sandbox: &'a str,
    pub(crate) driver_runtime_context: &'a [(&'a str, &'a str)],
}

pub(crate) fn build_generic_cli_prompt_envelope(input: GenericCliPromptEnvelope<'_>) -> String {
    let mut runtime_context = String::new();
    runtime_context.push_str("[Awiki Runtime Context]\n");
    runtime_context.push_str(&format!("agent_did: {}\n", input.invocation.agent_did));
    runtime_context.push_str("runtime_plugin_id: generic-cli\n");
    runtime_context.push_str(&format!("driver_id: {}\n", input.driver_id));
    runtime_context.push_str(&format!(
        "runtime_profile_id: {}\n",
        input.invocation.runtime_profile_id
    ));
    runtime_context.push_str(&format!(
        "workspace_instance_path: {}\n",
        input.workspace_root.display()
    ));
    runtime_context.push_str(&format!("sandbox: {}\n", input.sandbox));
    for (key, value) in input.driver_runtime_context {
        runtime_context.push_str(&format!("{key}: {value}\n"));
    }

    format!(
        r#"{runtime_context}
[Controller]
controller_verified: true

[Message Run]
message_id: {message_id}
task_id: {task_id}
run_id: {run_id}
conversation_id: {conversation_id}
user_message:
{task_text}

[Awiki Callback Rules]
- Use the daemon CLI wrapper for status, final replies, outgoing messages, and artifacts.
- Do not connect to message-service directly.
- Do not read or use DID private keys.
- If a wrapper call fails, report the failure instead of claiming success.

[Safety]
- Do not read secrets, private keys, .env files, or credential stores.
- Do not run destructive shell commands.
- Do not use unauthorized network access.
- Request controller approval before higher-risk actions.
"#,
        runtime_context = runtime_context,
        message_id = input.invocation.message_id,
        task_id = input.invocation.task_id,
        run_id = input.invocation.run_id,
        conversation_id = input.invocation.conversation_id.as_deref().unwrap_or(""),
        task_text = input.invocation.task_text,
    )
}

fn sanitize_path_component(input: &str) -> String {
    let sanitized = input
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if sanitized.is_empty() {
        format!("run_{}", current_time_millis().unwrap_or_default())
    } else {
        sanitized
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
            json_string_field(&profile.driver_config_json, "program").map(std::path::PathBuf::from)
        })
        .context("command generic-cli driver requires binary_path or driver_config.program")?;
    let args = string_array(profile.driver_config_json.get("args"))?;
    let cli_wrapper = json_string_field(&profile.driver_config_json, "cli_wrapper")
        .unwrap_or_else(|| "library:awiki_deamon::cli_wrapper".to_string());
    let run_timeout = json_duration_ms_field(
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

pub(crate) fn json_string_field(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub(crate) fn json_bool_field(value: &Value, field: &str, default: bool) -> bool {
    value.get(field).and_then(Value::as_bool).unwrap_or(default)
}

pub(crate) fn json_duration_ms_field(value: &Value, field: &str, default: Duration) -> Duration {
    value
        .get(field)
        .and_then(Value::as_u64)
        .filter(|millis| *millis > 0)
        .map(Duration::from_millis)
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::GenericCliRouteSession;
    use crate::workspace::{WorkspaceCleanupPolicy, WorkspaceInstance, WorkspaceMode};

    #[test]
    fn shared_metadata_helpers_keep_stable_runtime_shape() {
        let session = GenericCliRouteSession {
            route_key: "controller:alice:conversation".to_string(),
            route_key_hash: "hash-1".to_string(),
            session_dir: std::path::PathBuf::from("/tmp/session"),
            last_run_id: Some("run-1".to_string()),
            last_message_id: None,
            native_session_id: Some("native-1".to_string()),
            synthetic_session_id: Some("synthetic-1".to_string()),
            status: "active".to_string(),
        };
        let workspace_instance = WorkspaceInstance {
            workspace_id: "workspace-1".to_string(),
            workspace_root: std::path::PathBuf::from("/tmp/workspace"),
            workspace_instance_path: std::path::PathBuf::from("/tmp/workspace/conversations/a"),
            workspace_mode: WorkspaceMode::RouteRoot,
            is_security_boundary: false,
            isolation_note: "route scoped".to_string(),
            cleanup_policy: WorkspaceCleanupPolicy::Preserve,
            base_ref: Some("main".to_string()),
            branch_name: Some("awiki/task".to_string()),
        };
        let output_dir = std::path::PathBuf::from("/tmp/output");

        let route = route_session_metadata(Some(&session)).unwrap();
        assert_eq!(route["route_key_hash"], "hash-1");
        assert_eq!(route["last_run_id_present"], true);
        assert_eq!(route["last_message_id_present"], false);
        assert_eq!(route["native_session_id_present"], true);

        let workspace = workspace_metadata(
            &workspace_instance.workspace_root,
            &workspace_instance.workspace_instance_path,
            Some(&workspace_instance),
        );
        assert_eq!(workspace["workspace_mode"], "route-root");
        assert_eq!(workspace["cleanup_policy"], "preserve");
        assert_eq!(workspace["base_ref"], "main");
        assert_eq!(workspace["branch_name"], "awiki/task");

        let output = output_metadata(
            &output_dir,
            &output_dir.join("stdout.log"),
            &output_dir.join("stderr.log"),
            &output_dir.join("observation.jsonl"),
            &output_dir.join("final-output.txt"),
            None,
        );
        assert_eq!(output["output_dir"], "/tmp/output");
        assert!(output.get("sanitizer").is_none());
    }

    #[test]
    fn shared_run_paths_preserve_route_session_and_configured_output_rules() {
        let route_session = GenericCliRouteSession {
            route_key: "route-key".to_string(),
            route_key_hash: "route-hash".to_string(),
            session_dir: std::path::PathBuf::from("/tmp/awiki-session"),
            last_run_id: None,
            last_message_id: None,
            native_session_id: None,
            synthetic_session_id: Some("route-hash".to_string()),
            status: "new".to_string(),
        };
        let base_invocation = GenericCliInvocation {
            run_id: "run:one".to_string(),
            task_id: "task-1".to_string(),
            message_id: "msg-1".to_string(),
            conversation_id: Some("conv-1".to_string()),
            task_text: "hello".to_string(),
            agent_did: "did:agent:one".to_string(),
            runtime_profile_id: "profile-1".to_string(),
            workspace_root: None,
            workspace_instance: None,
            route_session: Some(route_session),
            runtime_temp_dir: Some(std::path::PathBuf::from("/tmp/runtime")),
            runtime_rpc_token: "rtok_1".to_string(),
            local_socket_path: None,
            callbacks: Vec::new(),
        };

        let route_paths = generic_cli_run_paths(
            &base_invocation,
            None,
            "codex",
            "stdout.jsonl",
            "stderr.log",
            "observation.jsonl",
        );
        assert_eq!(
            route_paths.output_dir,
            std::path::PathBuf::from("/tmp/awiki-session/runs/run_one")
        );
        assert_eq!(
            route_paths.final_output_path,
            std::path::PathBuf::from("/tmp/awiki-session/last-output.md")
        );

        let configured_output = std::path::PathBuf::from("/tmp/configured-output");
        let configured_paths = generic_cli_run_paths(
            &base_invocation,
            Some(&configured_output),
            "codex",
            "stdout.jsonl",
            "stderr.log",
            "observation.jsonl",
        );
        assert_eq!(configured_paths.output_dir, configured_output);
        assert_eq!(
            configured_paths.final_output_path,
            std::path::PathBuf::from("/tmp/configured-output/final-output.txt")
        );

        let mut no_route_invocation = base_invocation.clone();
        no_route_invocation.route_session = None;
        let no_route_paths = generic_cli_run_paths(
            &no_route_invocation,
            None,
            "claude-code",
            "stdout.jsonl",
            "stderr.log",
            "observation.jsonl",
        );
        assert_eq!(
            no_route_paths.output_dir,
            std::path::PathBuf::from("/tmp/runtime/awiki-deamon/generic-cli/claude-code/run_one")
        );
        assert_eq!(
            no_route_paths.final_output_path,
            no_route_paths.output_dir.join("final-output.txt")
        );
    }

    #[test]
    fn shared_prompt_envelope_keeps_driver_specific_context_inside_common_rules() {
        let invocation = GenericCliInvocation {
            run_id: "run-1".to_string(),
            task_id: "task-1".to_string(),
            message_id: "msg-1".to_string(),
            conversation_id: Some("conv-1".to_string()),
            task_text: "please help".to_string(),
            agent_did: "did:agent:one".to_string(),
            runtime_profile_id: "profile-1".to_string(),
            workspace_root: None,
            workspace_instance: None,
            route_session: None,
            runtime_temp_dir: None,
            runtime_rpc_token: "rtok_prompt_secret".to_string(),
            local_socket_path: None,
            callbacks: Vec::new(),
        };

        let prompt = build_generic_cli_prompt_envelope(GenericCliPromptEnvelope {
            invocation: &invocation,
            workspace_root: Path::new("/tmp/workspace"),
            driver_id: "claude-code",
            sandbox: "read-only",
            driver_runtime_context: &[("permission_mode", "plan")],
        });

        assert!(prompt.contains("[Awiki Runtime Context]"));
        assert!(prompt.contains("runtime_plugin_id: generic-cli"));
        assert!(prompt.contains("driver_id: claude-code"));
        assert!(prompt.contains("workspace_instance_path: /tmp/workspace"));
        assert!(prompt.contains("sandbox: read-only"));
        assert!(prompt.contains("permission_mode: plan"));
        assert!(prompt.contains("[Awiki Callback Rules]"));
        assert!(prompt.contains("Do not connect to message-service directly."));
        assert!(prompt
            .contains("If a wrapper call fails, report the failure instead of claiming success."));
        assert!(prompt.contains("[Safety]"));
        assert!(prompt.contains("Request controller approval before higher-risk actions."));
        assert!(prompt.contains("message_id: msg-1"));
        assert!(prompt.contains("conversation_id: conv-1"));
        assert!(prompt.contains("user_message:\nplease help"));
        assert!(!prompt.contains(&invocation.runtime_rpc_token));
    }

    #[test]
    fn shared_json_field_helpers_keep_profile_config_semantics() {
        let config = json!({
            "name": "  codex  ",
            "empty": "   ",
            "enabled": true,
            "timeout": 2500,
            "zero_timeout": 0,
        });

        assert_eq!(json_string_field(&config, "name").as_deref(), Some("codex"));
        assert_eq!(json_string_field(&config, "empty"), None);
        assert_eq!(json_string_field(&config, "missing"), None);
        assert!(json_bool_field(&config, "enabled", false));
        assert!(json_bool_field(&config, "missing_bool", true));
        assert_eq!(
            json_duration_ms_field(&config, "timeout", Duration::from_millis(10)),
            Duration::from_millis(2500)
        );
        assert_eq!(
            json_duration_ms_field(&config, "zero_timeout", Duration::from_millis(10)),
            Duration::from_millis(10)
        );
    }
}
