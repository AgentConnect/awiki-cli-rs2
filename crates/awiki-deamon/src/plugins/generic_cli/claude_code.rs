use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use rand::RngCore;
use serde_json::Value;

use crate::runtime::{RuntimeInstallStatus, RuntimeRunStatus};
use crate::security::runtime_token::current_time_millis;
use crate::state::CliRuntimeProfileRecord;

use super::{
    process::{
        ManagedChild, ManagedChildTimeoutError, DEFAULT_GENERIC_CLI_PROBE_TIMEOUT,
        DEFAULT_GENERIC_CLI_RUN_TIMEOUT,
    },
    sanitize_cli_output_text, validate_native_session_id, write_sanitized_cli_output,
    GenericCliDriver, GenericCliExit, GenericCliInvocation,
};

const DEFAULT_CLAUDE_CODE_BINARY: &str = "claude";
const DEFAULT_SANDBOX: &str = "read-only";
const DEFAULT_CLI_WRAPPER: &str = "library:awiki_deamon::cli_wrapper";
const NATIVE_SESSION_SOURCE_STREAM_JSON: &str = "stream_json";
const NATIVE_SESSION_SOURCE_GENERATED_SESSION_ID: &str = "generated_session_id";
const NATIVE_SESSION_SOURCE_RESUME_ID: &str = "resume_id";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeCodeDriverConfig {
    pub binary_path: PathBuf,
    pub model: Option<String>,
    pub sandbox: String,
    pub permission_mode: String,
    pub setting_sources: Option<String>,
    pub strict_mcp_config: bool,
    pub bare: bool,
    pub no_session_persistence: bool,
    pub output_dir: Option<PathBuf>,
    pub cli_wrapper: String,
    pub run_timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct ClaudeCodeDriver {
    config: ClaudeCodeDriverConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeCodeRunPaths {
    pub output_dir: PathBuf,
    pub final_output_path: PathBuf,
    pub stdout_path: PathBuf,
    pub stderr_path: PathBuf,
    pub jsonl_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaudeCodeSessionMode {
    New { session_id: String },
    ResumeId(String),
}

impl ClaudeCodeSessionMode {
    fn as_str(&self) -> &'static str {
        match self {
            Self::New { .. } => "new",
            Self::ResumeId(_) => "resume_id",
        }
    }

    fn native_session_id(&self) -> &str {
        match self {
            Self::New { session_id } => session_id.as_str(),
            Self::ResumeId(id) => id.as_str(),
        }
    }
}

impl ClaudeCodeDriver {
    pub fn new(config: ClaudeCodeDriverConfig) -> Result<Self> {
        config.validate()?;
        Ok(Self { config })
    }

    pub fn from_profile(profile: &CliRuntimeProfileRecord) -> Result<Self> {
        Self::new(ClaudeCodeDriverConfig::from_profile(profile)?)
    }
}

impl ClaudeCodeDriverConfig {
    pub fn from_profile(profile: &CliRuntimeProfileRecord) -> Result<Self> {
        let config = &profile.driver_config_json;
        let binary_path = profile
            .binary_path
            .clone()
            .or_else(|| string_field(config, "binary_path").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from(DEFAULT_CLAUDE_CODE_BINARY));
        let sandbox = string_field(config, "sandbox")
            .or_else(|| profile.default_sandbox.clone())
            .unwrap_or_else(|| DEFAULT_SANDBOX.to_string());
        let permission_mode = string_field(config, "permission_mode")
            .unwrap_or_else(|| permission_mode_for_sandbox(&sandbox).to_string());
        let output_dir = string_field(config, "output_dir").map(PathBuf::from);
        let setting_sources = string_field(config, "setting_sources").or_else(|| {
            if bool_field(config, "load_project_settings", false) {
                Some("user,project,local".to_string())
            } else {
                Some("user".to_string())
            }
        });
        let record = Self {
            binary_path,
            model: profile
                .default_model
                .clone()
                .or_else(|| string_field(config, "model")),
            sandbox,
            permission_mode,
            setting_sources,
            strict_mcp_config: bool_field(config, "strict_mcp_config", true),
            bare: bool_field(config, "bare", false),
            no_session_persistence: bool_field(config, "no_session_persistence", false),
            output_dir,
            cli_wrapper: string_field(config, "cli_wrapper")
                .unwrap_or_else(|| DEFAULT_CLI_WRAPPER.to_string()),
            run_timeout: duration_ms_field(
                config,
                "run_timeout_ms",
                DEFAULT_GENERIC_CLI_RUN_TIMEOUT,
            ),
        };
        record.validate()?;
        Ok(record)
    }

    pub fn validate(&self) -> Result<()> {
        if self.binary_path.as_os_str().is_empty() {
            bail!("claude-code binary_path must not be empty");
        }
        if !matches!(self.sandbox.as_str(), "read-only" | "workspace-write") {
            bail!("claude-code sandbox must be read-only or workspace-write");
        }
        if !matches!(
            self.permission_mode.as_str(),
            "plan" | "default" | "acceptEdits" | "dontAsk" | "auto"
        ) {
            bail!("claude-code permission_mode is not supported");
        }
        if self.sandbox == "read-only" && self.permission_mode != "plan" {
            bail!("claude-code read-only sandbox requires permission_mode=plan");
        }
        if let Some(setting_sources) = self.setting_sources.as_deref() {
            validate_setting_sources(setting_sources)?;
        }
        if self.cli_wrapper.trim().is_empty() {
            bail!("claude-code cli_wrapper must not be empty");
        }
        Ok(())
    }
}

impl GenericCliDriver for ClaudeCodeDriver {
    fn check_install_status(&self) -> Result<RuntimeInstallStatus> {
        let mut command = Command::new(&self.config.binary_path);
        command
            .arg("--version")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        apply_claude_code_probe_env(&mut command);
        let output =
            ManagedChild::spawn(&mut command, "spawn claude-code --version").and_then(|managed| {
                managed.wait_timeout(
                    "wait for claude-code --version",
                    DEFAULT_GENERIC_CLI_PROBE_TIMEOUT,
                )
            });
        match output {
            Ok(output) if output.output.status.success() => {
                let version = String::from_utf8_lossy(&output.output.stdout)
                    .trim()
                    .to_string();
                let detail = if version.is_empty() {
                    self.config.binary_path.display().to_string()
                } else {
                    format!("{} ({version})", self.config.binary_path.display())
                };
                Ok(RuntimeInstallStatus {
                    installed: true,
                    detail: Some(detail),
                })
            }
            Ok(output) => Ok(RuntimeInstallStatus {
                installed: false,
                detail: Some(format!(
                    "{} --version exited with {}",
                    self.config.binary_path.display(),
                    output.output.status.code().unwrap_or(1)
                )),
            }),
            Err(error) => Ok(RuntimeInstallStatus {
                installed: false,
                detail: Some(format!("{}: {error}", self.config.binary_path.display())),
            }),
        }
    }

    fn run(&self, invocation: GenericCliInvocation) -> Result<GenericCliExit> {
        let workspace_root = invocation
            .workspace_instance
            .as_ref()
            .map(|instance| instance.workspace_instance_path.clone())
            .or_else(|| invocation.workspace_root.clone())
            .context("Claude Code driver requires workspace_root")?;
        let workspace_binding_root = invocation
            .workspace_root
            .clone()
            .context("Claude Code driver requires workspace_root")?;
        let socket_path = invocation
            .local_socket_path
            .clone()
            .context("Claude Code driver requires daemon local RPC socket")?;
        std::fs::create_dir_all(&workspace_root).with_context(|| {
            format!("create Claude Code workspace {}", workspace_root.display())
        })?;
        let paths = self.run_paths(&invocation)?;
        std::fs::create_dir_all(&paths.output_dir).with_context(|| {
            format!(
                "create Claude Code output dir {}",
                paths.output_dir.display()
            )
        })?;
        if let Some(parent) = paths.final_output_path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("create Claude Code final output dir {}", parent.display())
            })?;
        }
        let prompt = build_prompt_envelope(&invocation, &workspace_root, &self.config);
        ensure_prompt_does_not_contain_token(&prompt, &invocation.runtime_rpc_token)?;

        let session_mode = claude_code_session_mode(&invocation);
        if self.config.no_session_persistence && invocation.route_session.is_some() {
            return Ok(GenericCliExit {
                exit_code: 2,
                status: RuntimeRunStatus::Failed,
                callbacks: Vec::new(),
                metadata: serde_json::json!({
                    "driver_id": "claude-code",
                    "error_code": "session_persistence_disabled",
                    "error_summary": "claude-code no_session_persistence cannot be used with route sessions",
                    "next_action": "manual_review_required",
                    "home_isolation": home_isolation(),
                    "native_session_id": null,
                    "native_session_source": null,
                    "session": {
                        "mode": session_mode.as_str(),
                        "native_session_id_present": false,
                    },
                    "route_session": invocation.route_session.as_ref().map(|session| serde_json::json!({
                        "route_key_hash": session.route_key_hash,
                        "status": session.status,
                        "last_message_id_present": session.last_message_id.is_some(),
                        "last_run_id_present": session.last_run_id.is_some(),
                        "synthetic_session_id_present": session.synthetic_session_id.is_some(),
                        "native_session_id_present": session.native_session_id.is_some(),
                    })),
                    "command": {
                        "program": self.config.binary_path,
                        "args": self.command_args_for_mode(session_mode),
                    },
                    "workspace": {
                        "workspace_root": workspace_binding_root,
                        "workspace_instance_path": workspace_root,
                        "workspace_mode": invocation.workspace_instance.as_ref().map(|instance| instance.workspace_mode.as_str()),
                        "is_security_boundary": invocation.workspace_instance.as_ref().map(|instance| instance.is_security_boundary),
                        "isolation_note": invocation.workspace_instance.as_ref().map(|instance| instance.isolation_note.as_str()),
                        "cleanup_policy": invocation.workspace_instance.as_ref().map(|instance| instance.cleanup_policy),
                        "base_ref": invocation.workspace_instance.as_ref().and_then(|instance| instance.base_ref.as_deref()),
                        "branch_name": invocation.workspace_instance.as_ref().and_then(|instance| instance.branch_name.as_deref()),
                    },
                    "output": {
                        "output_dir": paths.output_dir,
                        "stdout_path": paths.stdout_path,
                        "stderr_path": paths.stderr_path,
                        "jsonl_path": paths.jsonl_path,
                        "final_output_path": paths.final_output_path.clone(),
                    },
                    "final_output_path": paths.final_output_path,
                }),
            });
        }
        let args = self.command_args_for_mode(session_mode.clone());
        let mut command = Command::new(&self.config.binary_path);
        command
            .args(&args)
            .current_dir(&workspace_root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        apply_claude_code_env(&mut command, &invocation, &socket_path, &self.config);

        let managed = ManagedChild::spawn(&mut command, "spawn claude-code print")?;
        let managed_output = match managed.write_stdin_and_wait_timeout(
            prompt.as_bytes(),
            "write Claude Code prompt envelope to stdin",
            "wait for claude-code",
            self.config.run_timeout,
        ) {
            Ok(output) => output,
            Err(error) => {
                if let Some(timeout) = error.downcast_ref::<ManagedChildTimeoutError>() {
                    return Ok(GenericCliExit {
                        exit_code: 124,
                        status: RuntimeRunStatus::Failed,
                        callbacks: Vec::new(),
                        metadata: serde_json::json!({
                            "driver_id": "claude-code",
                            "home_isolation": home_isolation(),
                            "error_code": "claude_code_cli_timeout",
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
        let process_metadata = managed_output.process_metadata();
        let output = managed_output.output;
        let stdout_sanitizer = write_sanitized_cli_output(
            &paths.stdout_path,
            &output.stdout,
            &invocation.runtime_rpc_token,
        )?;
        let stderr_sanitizer = write_sanitized_cli_output(
            &paths.stderr_path,
            &output.stderr,
            &invocation.runtime_rpc_token,
        )?;
        let parsed_native_session_id =
            claude_code_native_session_id_from_stream_json(&output.stdout);
        let exit_code = output.status.code().unwrap_or(1);
        let success = exit_code == 0;
        let native_session_id = parsed_native_session_id.clone().or_else(|| {
            if success {
                Some(session_mode.native_session_id().to_string())
            } else if matches!(session_mode, ClaudeCodeSessionMode::ResumeId(_)) {
                Some(session_mode.native_session_id().to_string())
            } else {
                None
            }
        });
        let native_session_source = if parsed_native_session_id.is_some() {
            Some(NATIVE_SESSION_SOURCE_STREAM_JSON)
        } else if success {
            match session_mode {
                ClaudeCodeSessionMode::New { .. } => {
                    Some(NATIVE_SESSION_SOURCE_GENERATED_SESSION_ID)
                }
                ClaudeCodeSessionMode::ResumeId(_) => Some(NATIVE_SESSION_SOURCE_RESUME_ID),
            }
        } else {
            None
        };
        let final_text = claude_code_final_text_from_stream_json(&output.stdout);
        let final_output_sanitizer = sanitize_cli_output_text(
            final_text.as_deref().unwrap_or_default(),
            &invocation.runtime_rpc_token,
            super::DEFAULT_SANITIZED_OUTPUT_MAX_BYTES,
        );
        std::fs::write(
            &paths.final_output_path,
            final_output_sanitizer.text.as_bytes(),
        )
        .with_context(|| format!("write {}", paths.final_output_path.display()))?;
        std::fs::write(
            &paths.jsonl_path,
            serde_json::json!({
                "kind": "claude-code.print.observation",
                "run_id": invocation.run_id,
                "stdout_path": paths.stdout_path,
                "stderr_path": paths.stderr_path,
                "final_output_path": paths.final_output_path,
                "session_mode": session_mode.as_str(),
                "native_session_id_present": native_session_id.is_some(),
                "final_text_present": final_text.is_some(),
            })
            .to_string(),
        )
        .with_context(|| format!("write {}", paths.jsonl_path.display()))?;
        Ok(GenericCliExit {
            exit_code,
            status: if success {
                RuntimeRunStatus::Finished
            } else {
                RuntimeRunStatus::Failed
            },
            callbacks: Vec::new(),
            metadata: serde_json::json!({
                "driver_id": "claude-code",
                "home_isolation": home_isolation(),
                "error_code": if success { serde_json::Value::Null } else { serde_json::Value::String("claude_code_cli_failed".to_string()) },
                "error_summary": if success { serde_json::Value::Null } else { serde_json::Value::String(format!("Claude Code CLI exited with status {exit_code}")) },
                "next_action": if success { serde_json::Value::Null } else { serde_json::Value::String("manual_review_required".to_string()) },
                "process": process_metadata,
                "native_session_id": native_session_id,
                "native_session_source": native_session_source,
                "session": {
                    "mode": session_mode.as_str(),
                    "native_session_id_present": native_session_id.is_some(),
                },
                "route_session": invocation.route_session.as_ref().map(|session| serde_json::json!({
                    "route_key_hash": session.route_key_hash,
                    "status": session.status,
                    "last_message_id_present": session.last_message_id.is_some(),
                    "last_run_id_present": session.last_run_id.is_some(),
                    "synthetic_session_id_present": session.synthetic_session_id.is_some(),
                    "native_session_id_present": session.native_session_id.is_some(),
                })),
                "command": {
                    "program": self.config.binary_path,
                    "args": args,
                },
                "workspace": {
                    "workspace_root": workspace_binding_root,
                    "workspace_instance_path": workspace_root,
                    "workspace_mode": invocation.workspace_instance.as_ref().map(|instance| instance.workspace_mode.as_str()),
                    "is_security_boundary": invocation.workspace_instance.as_ref().map(|instance| instance.is_security_boundary),
                    "isolation_note": invocation.workspace_instance.as_ref().map(|instance| instance.isolation_note.as_str()),
                    "cleanup_policy": invocation.workspace_instance.as_ref().map(|instance| instance.cleanup_policy),
                    "base_ref": invocation.workspace_instance.as_ref().and_then(|instance| instance.base_ref.as_deref()),
                    "branch_name": invocation.workspace_instance.as_ref().and_then(|instance| instance.branch_name.as_deref()),
                },
                "output": {
                    "output_dir": paths.output_dir,
                    "stdout_path": paths.stdout_path,
                    "stderr_path": paths.stderr_path,
                    "jsonl_path": paths.jsonl_path,
                    "final_output_path": paths.final_output_path.clone(),
                    "sanitizer": {
                        "stdout": stdout_sanitizer.metadata_json(),
                        "stderr": stderr_sanitizer.metadata_json(),
                        "final_output": final_output_sanitizer.metadata_json(),
                    },
                },
                "final_output_path": paths.final_output_path,
            }),
        })
    }
}

impl ClaudeCodeDriver {
    pub fn command_args_for_mode(&self, session_mode: ClaudeCodeSessionMode) -> Vec<String> {
        let mut args = vec![
            "-p".to_string(),
            "--verbose".to_string(),
            "--output-format".to_string(),
            "stream-json".to_string(),
            "--permission-mode".to_string(),
            self.config.permission_mode.clone(),
        ];
        if let Some(model) = self.config.model.as_ref() {
            args.push("--model".to_string());
            args.push(model.clone());
        }
        if let Some(setting_sources) = self.config.setting_sources.as_ref() {
            args.push("--setting-sources".to_string());
            args.push(setting_sources.clone());
        }
        if self.config.strict_mcp_config {
            args.push("--strict-mcp-config".to_string());
        }
        if self.config.bare {
            args.push("--bare".to_string());
        }
        if self.config.no_session_persistence {
            args.push("--no-session-persistence".to_string());
        }
        match session_mode {
            ClaudeCodeSessionMode::New { session_id } => {
                args.push("--session-id".to_string());
                args.push(session_id);
            }
            ClaudeCodeSessionMode::ResumeId(id) => {
                args.push("--resume".to_string());
                args.push(id);
            }
        }
        args
    }

    pub fn run_paths(&self, invocation: &GenericCliInvocation) -> Result<ClaudeCodeRunPaths> {
        let output_dir = self.config.output_dir.clone().unwrap_or_else(|| {
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
                .join("claude-code")
                .join(sanitize_path_component(&invocation.run_id))
        });
        let final_output_path = self.config.output_dir.as_ref().map_or_else(
            || {
                invocation
                    .route_session
                    .as_ref()
                    .map(|route_session| route_session.session_dir.join("last-output.md"))
                    .unwrap_or_else(|| output_dir.join("final-output.txt"))
            },
            |_| output_dir.join("final-output.txt"),
        );
        Ok(ClaudeCodeRunPaths {
            final_output_path,
            stdout_path: output_dir.join("claude-stdout.jsonl"),
            stderr_path: output_dir.join("claude-stderr.log"),
            jsonl_path: output_dir.join("claude-observation.jsonl"),
            output_dir,
        })
    }
}

fn claude_code_session_mode(invocation: &GenericCliInvocation) -> ClaudeCodeSessionMode {
    if let Some(id) = invocation
        .route_session
        .as_ref()
        .and_then(|session| session.native_session_id.as_deref())
        .filter(|value| validate_native_session_id("claude-code", value))
    {
        return ClaudeCodeSessionMode::ResumeId(id.to_string());
    }
    ClaudeCodeSessionMode::New {
        session_id: generate_uuid_v4(),
    }
}

fn apply_claude_code_env(
    command: &mut Command,
    invocation: &GenericCliInvocation,
    socket_path: &std::path::Path,
    config: &ClaudeCodeDriverConfig,
) {
    command.env_clear();
    apply_claude_code_base_env(command);
    command
        .env("AWIKI_DAEMON_RUN_ID", &invocation.run_id)
        .env("AWIKI_DAEMON_TASK_ID", &invocation.task_id)
        .env("AWIKI_DAEMON_AGENT_DID", &invocation.agent_did)
        .env(
            "AWIKI_DAEMON_RUNTIME_PROFILE_ID",
            &invocation.runtime_profile_id,
        )
        .env("AWIKI_DAEMON_SOCKET", socket_path)
        .env("AWIKI_DAEMON_CLI_WRAPPER", &config.cli_wrapper)
        .env(
            "AWIKI_DAEMON_RUNTIME_RPC_TOKEN",
            &invocation.runtime_rpc_token,
        );
}

fn apply_claude_code_probe_env(command: &mut Command) {
    command.env_clear();
    apply_claude_code_base_env(command);
}

fn apply_claude_code_base_env(command: &mut Command) {
    for key in [
        "PATH",
        "LANG",
        "LC_ALL",
        "LC_CTYPE",
        "LC_MESSAGES",
        "TERM",
        "HOME",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
}

pub fn claude_code_native_session_id_from_stream_json(stdout: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(stdout);
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };
        if let Some(id) = native_session_id_from_json_value(&value)
            .filter(|id| validate_native_session_id("claude-code", id))
        {
            return Some(id);
        }
    }
    None
}

pub fn claude_code_final_text_from_stream_json(stdout: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(stdout);
    let mut latest = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };
        if let Some(text) = final_text_from_json_value(&value) {
            latest = Some(text);
        }
    }
    latest
}

fn native_session_id_from_json_value(value: &Value) -> Option<String> {
    for field in ["session_id", "sessionId"] {
        if let Some(id) = string_field(value, field) {
            return Some(id);
        }
    }
    for field in ["session", "conversation"] {
        if let Some(id) = value.get(field).and_then(|nested| {
            string_field(nested, "id")
                .or_else(|| string_field(nested, "session_id"))
                .or_else(|| string_field(nested, "sessionId"))
        }) {
            return Some(id);
        }
    }
    value
        .get("message")
        .and_then(native_session_id_from_json_value)
        .or_else(|| value.get("msg").and_then(native_session_id_from_json_value))
        .or_else(|| {
            value
                .get("event")
                .and_then(native_session_id_from_json_value)
        })
}

fn final_text_from_json_value(value: &Value) -> Option<String> {
    for field in ["result", "text"] {
        if let Some(text) = string_field(value, field) {
            return Some(text);
        }
    }
    if let Some(text) = value
        .get("delta")
        .and_then(|delta| string_field(delta, "text"))
    {
        return Some(text);
    }
    if let Some(text) = value.get("message").and_then(final_text_from_json_value) {
        return Some(text);
    }
    for field in ["content", "result"] {
        if let Some(text) = content_text(value.get(field)?) {
            return Some(text);
        }
    }
    None
}

fn content_text(value: &Value) -> Option<String> {
    if let Some(text) = value.as_str().filter(|text| !text.is_empty()) {
        return Some(text.to_string());
    }
    let array = value.as_array()?;
    let parts = array
        .iter()
        .filter_map(|item| {
            item.get("text")
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
                .map(str::to_string)
        })
        .collect::<Vec<_>>();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(""))
    }
}

pub fn build_prompt_envelope(
    invocation: &GenericCliInvocation,
    workspace_root: &std::path::Path,
    config: &ClaudeCodeDriverConfig,
) -> String {
    format!(
        r#"[Awiki Runtime Context]
agent_did: {agent_did}
runtime_plugin_id: generic-cli
driver_id: claude-code
runtime_profile_id: {runtime_profile_id}
workspace_instance_path: {workspace_root}
sandbox: {sandbox}
permission_mode: {permission_mode}

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
        agent_did = invocation.agent_did,
        runtime_profile_id = invocation.runtime_profile_id,
        workspace_root = workspace_root.display(),
        sandbox = config.sandbox,
        permission_mode = config.permission_mode,
        message_id = invocation.message_id,
        task_id = invocation.task_id,
        run_id = invocation.run_id,
        conversation_id = invocation.conversation_id.as_deref().unwrap_or(""),
        task_text = invocation.task_text,
    )
}

fn ensure_prompt_does_not_contain_token(prompt: &str, token: &str) -> Result<()> {
    if prompt.contains(token) {
        bail!("Claude Code prompt envelope must not contain runtime RPC token");
    }
    Ok(())
}

fn string_field(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn bool_field(value: &Value, field: &str, default: bool) -> bool {
    value.get(field).and_then(Value::as_bool).unwrap_or(default)
}

fn duration_ms_field(value: &Value, field: &str, default: Duration) -> Duration {
    value
        .get(field)
        .and_then(Value::as_u64)
        .filter(|millis| *millis > 0)
        .map(Duration::from_millis)
        .unwrap_or(default)
}

fn permission_mode_for_sandbox(sandbox: &str) -> &'static str {
    match sandbox {
        "workspace-write" => "default",
        _ => "plan",
    }
}

fn validate_setting_sources(setting_sources: &str) -> Result<()> {
    for source in setting_sources.split(',').map(str::trim) {
        if !matches!(source, "user") {
            bail!("claude-code setting_sources only supports user by default");
        }
    }
    Ok(())
}

fn home_isolation() -> &'static str {
    if std::env::var_os("HOME").is_some() {
        "host_default"
    } else {
        "unknown"
    }
}

fn generate_uuid_v4() -> String {
    let mut bytes = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15],
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
