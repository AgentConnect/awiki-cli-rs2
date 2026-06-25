use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde_json::Value;

use crate::runtime::{RuntimeInstallStatus, RuntimeRunStatus};
use crate::state::CliRuntimeProfileRecord;

use super::{
    build_generic_cli_prompt_envelope, generic_cli_run_paths, json_bool_field,
    json_duration_ms_field, json_path_field, json_string_field, optional_path_field,
    output_metadata, output_sanitizer_metadata,
    process::{
        ManagedChild, ManagedChildTimeoutError, DEFAULT_GENERIC_CLI_PROBE_TIMEOUT,
        DEFAULT_GENERIC_CLI_RUN_TIMEOUT,
    },
    route_session_metadata, sanitize_cli_output_file, validate_native_session_id,
    workspace_metadata, write_sanitized_cli_output, GenericCliDriver, GenericCliExit,
    GenericCliInvocation, GenericCliPromptEnvelope, GenericCliRunPaths, CODEX_CLI_DRIVER_ID,
};

const DEFAULT_CODEX_BINARY: &str = "codex";
const DEFAULT_SANDBOX: &str = "read-only";
const DEFAULT_CLI_WRAPPER: &str = "library:awiki_deamon::cli_wrapper";
const NATIVE_SESSION_SOURCE_JSON_EVENT: &str = "json_event";
const NATIVE_SESSION_SOURCE_RESUME_ID: &str = "resume_id";
const NATIVE_SESSION_SOURCE_RESUME_LAST: &str = "resume_last";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexDriverConfig {
    pub binary_path: PathBuf,
    pub config_home: PathBuf,
    pub profile: Option<String>,
    pub model: Option<String>,
    pub sandbox: String,
    pub ignore_user_config: bool,
    pub ignore_rules: bool,
    pub ephemeral: bool,
    pub output_dir: Option<PathBuf>,
    pub cli_wrapper: String,
    pub run_timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct CodexDriver {
    config: CodexDriverConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodexResumeMode {
    Fresh,
    ResumeId(String),
    ResumeLast,
}

impl CodexResumeMode {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Fresh => "fresh",
            Self::ResumeId(_) => "resume_id",
            Self::ResumeLast => "resume_last",
        }
    }

    fn native_session_id(&self) -> Option<&str> {
        match self {
            Self::Fresh => None,
            Self::ResumeId(id) => Some(id.as_str()),
            Self::ResumeLast => None,
        }
    }
}

impl CodexDriver {
    pub fn new(config: CodexDriverConfig) -> Result<Self> {
        config.validate()?;
        Ok(Self { config })
    }

    pub fn from_profile(profile: &CliRuntimeProfileRecord) -> Result<Self> {
        Self::new(CodexDriverConfig::from_profile(profile)?)
    }

    pub fn config(&self) -> &CodexDriverConfig {
        &self.config
    }
}

impl CodexDriverConfig {
    pub fn from_profile(profile: &CliRuntimeProfileRecord) -> Result<Self> {
        let config = &profile.driver_config_json;
        let binary_path = optional_path_field(&profile.binary_path)
            .or_else(|| json_path_field(config, "binary_path"))
            .unwrap_or_else(default_codex_binary_path);
        let sandbox = json_string_field(config, "sandbox")
            .or_else(|| profile.default_sandbox.clone())
            .unwrap_or_else(|| DEFAULT_SANDBOX.to_string());
        let output_dir = json_path_field(config, "output_dir");
        let config_home = optional_path_field(&profile.config_home)
            .or_else(|| json_path_field(config, "config_home"))
            .context("Codex generic-cli profile requires config_home for CODEX_HOME")?;
        let record = Self {
            binary_path,
            config_home,
            profile: json_string_field(config, "profile"),
            model: profile
                .default_model
                .clone()
                .or_else(|| json_string_field(config, "model")),
            sandbox,
            ignore_user_config: json_bool_field(config, "ignore_user_config", false),
            ignore_rules: json_bool_field(config, "ignore_rules", false),
            ephemeral: json_bool_field(config, "ephemeral", false),
            output_dir,
            cli_wrapper: json_string_field(config, "cli_wrapper")
                .unwrap_or_else(|| DEFAULT_CLI_WRAPPER.to_string()),
            run_timeout: json_duration_ms_field(
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
            bail!("codex binary_path must not be empty");
        }
        if self.config_home.as_os_str().is_empty() {
            bail!("codex config_home must not be empty");
        }
        if !matches!(self.sandbox.as_str(), "read-only" | "workspace-write") {
            bail!("codex sandbox must be read-only or workspace-write");
        }
        if self.cli_wrapper.trim().is_empty() {
            bail!("codex cli_wrapper must not be empty");
        }
        Ok(())
    }
}

impl GenericCliDriver for CodexDriver {
    fn check_install_status(&self) -> Result<RuntimeInstallStatus> {
        let mut command = Command::new(&self.config.binary_path);
        command
            .arg("--version")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        apply_codex_probe_env(&mut command, &self.config);
        let output =
            ManagedChild::spawn(&mut command, "spawn codex --version").and_then(|managed| {
                managed.wait_timeout(
                    "wait for codex --version",
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
            .context("Codex driver requires workspace_root")?;
        let workspace_binding_root = invocation
            .workspace_root
            .clone()
            .context("Codex driver requires workspace_root")?;
        let socket_path = invocation
            .local_socket_path
            .clone()
            .context("Codex driver requires daemon local RPC socket")?;
        std::fs::create_dir_all(&workspace_root)
            .with_context(|| format!("create Codex workspace {}", workspace_root.display()))?;
        if !self.config.config_home.is_dir() {
            bail!("Codex CODEX_HOME is not configured or does not exist");
        }
        if !codex_profile_auth_ready(&self.config.config_home) {
            return Ok(GenericCliExit {
                exit_code: 78,
                status: RuntimeRunStatus::Failed,
                callbacks: Vec::new(),
                metadata: serde_json::json!({
                    "driver_id": CODEX_CLI_DRIVER_ID,
                    "config_home": "configured",
                    "auth_status": "missing",
                    "setup_ready": false,
                    "error_code": "generic_cli_auth_missing",
                    "error_summary": "Codex profile CODEX_HOME is missing auth.json; seed it from an authenticated Codex setup before sending messages.",
                    "next_action": "manual_review_required",
                    "process": {
                        "spawned": false,
                        "reason": "auth_missing"
                    },
                }),
            });
        }
        let paths = self.run_paths(&invocation)?;
        std::fs::create_dir_all(&paths.output_dir)
            .with_context(|| format!("create Codex output dir {}", paths.output_dir.display()))?;
        if let Some(parent) = paths.final_output_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create Codex final output dir {}", parent.display()))?;
        }
        let prompt = build_prompt_envelope(&invocation, &workspace_root, &self.config);
        ensure_prompt_does_not_contain_token(&prompt, &invocation.runtime_rpc_token)?;

        let resume_mode = codex_resume_mode(&invocation);
        let args = self.command_args_for_mode(
            &workspace_root,
            &paths.final_output_path,
            resume_mode.clone(),
        );
        let mut command = Command::new(&self.config.binary_path);
        command
            .args(&args)
            .current_dir(&workspace_root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        apply_codex_env(&mut command, &invocation, &socket_path, &self.config);

        let managed = ManagedChild::spawn(&mut command, "spawn codex exec")?;
        let managed_output = match managed.write_stdin_and_wait_timeout(
            prompt.as_bytes(),
            "write codex prompt envelope to stdin",
            "wait for codex exec",
            self.config.run_timeout,
        ) {
            Ok(output) => output,
            Err(error) => {
                if let Some(timeout) = error.downcast_ref::<ManagedChildTimeoutError>() {
                    return Ok(GenericCliExit::timeout(
                        CODEX_CLI_DRIVER_ID,
                        "codex_cli_timeout",
                        timeout,
                        [("config_home", Value::String("configured".to_string()))],
                    ));
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
        let final_output_sanitizer =
            sanitize_cli_output_file(&paths.final_output_path, &invocation.runtime_rpc_token)?;
        let parsed_native_session_id = codex_native_session_id_from_stdout_jsonl(&output.stdout);
        let native_session_id = parsed_native_session_id
            .clone()
            .or_else(|| resume_mode.native_session_id().map(str::to_string));
        let native_session_source = if parsed_native_session_id.is_some() {
            Some(NATIVE_SESSION_SOURCE_JSON_EVENT)
        } else if resume_mode.native_session_id().is_some() && output.status.success() {
            Some(NATIVE_SESSION_SOURCE_RESUME_ID)
        } else if resume_mode == CodexResumeMode::ResumeLast && output.status.success() {
            Some(NATIVE_SESSION_SOURCE_RESUME_LAST)
        } else {
            None
        };
        std::fs::write(
            &paths.jsonl_path,
            serde_json::json!({
                "kind": "codex.exec.observation",
                "run_id": invocation.run_id,
                "stdout_path": paths.stdout_path,
                "stderr_path": paths.stderr_path,
                "final_output_path": paths.final_output_path,
                "resume_mode": resume_mode.as_str(),
                "native_session_id_present": native_session_id.is_some(),
            })
            .to_string(),
        )
        .with_context(|| format!("write {}", paths.jsonl_path.display()))?;
        let exit_code = output.status.code().unwrap_or(1);
        Ok(GenericCliExit {
            exit_code,
            status: if exit_code == 0 {
                RuntimeRunStatus::Finished
            } else {
                RuntimeRunStatus::Failed
            },
            callbacks: Vec::new(),
            metadata: serde_json::json!({
                "driver_id": CODEX_CLI_DRIVER_ID,
                "config_home": "configured",
                "error_code": if exit_code == 0 { serde_json::Value::Null } else { serde_json::Value::String("codex_cli_failed".to_string()) },
                "error_summary": if exit_code == 0 { serde_json::Value::Null } else { serde_json::Value::String(format!("Codex CLI exited with status {exit_code}")) },
                "next_action": if exit_code == 0 { serde_json::Value::Null } else { serde_json::Value::String("manual_review_required".to_string()) },
                "process": process_metadata,
                "native_session_id": native_session_id,
                "native_session_source": native_session_source,
                "resume": {
                    "mode": resume_mode.as_str(),
                    "native_session_id_present": resume_mode.native_session_id().is_some(),
                },
                "route_session": route_session_metadata(invocation.route_session.as_ref()),
                "command": {
                    "program": self.config.binary_path,
                    "args": args,
                },
                "workspace": workspace_metadata(
                    &workspace_binding_root,
                    &workspace_root,
                    invocation.workspace_instance.as_ref(),
                ),
                "output": output_metadata(
                    &paths.output_dir,
                    &paths.stdout_path,
                    &paths.stderr_path,
                    &paths.jsonl_path,
                    &paths.final_output_path,
                    Some(output_sanitizer_metadata(
                        &stdout_sanitizer,
                        &stderr_sanitizer,
                        final_output_sanitizer.as_ref().map(|sanitizer| sanitizer.metadata_json()),
                    )),
                ),
                "final_output_path": paths.final_output_path,
            }),
        })
    }
}

impl CodexDriver {
    pub fn command_args(
        &self,
        workspace_root: &std::path::Path,
        final_output_path: &std::path::Path,
    ) -> Vec<String> {
        self.command_args_for_mode(workspace_root, final_output_path, CodexResumeMode::Fresh)
    }

    pub fn command_args_for_mode(
        &self,
        workspace_root: &std::path::Path,
        final_output_path: &std::path::Path,
        resume_mode: CodexResumeMode,
    ) -> Vec<String> {
        let mut args = vec!["exec".to_string()];
        args.extend([
            "--cd".to_string(),
            workspace_root.display().to_string(),
            "--sandbox".to_string(),
            self.config.sandbox.clone(),
            "--skip-git-repo-check".to_string(),
        ]);
        if let Some(model) = self.config.model.as_ref() {
            args.push("--model".to_string());
            args.push(model.clone());
        }
        if let Some(profile) = self.config.profile.as_ref() {
            args.push("--profile".to_string());
            args.push(profile.clone());
        }
        if self.config.ignore_user_config {
            args.push("--ignore-user-config".to_string());
        }
        if self.config.ignore_rules {
            args.push("--ignore-rules".to_string());
        }
        if self.config.ephemeral {
            args.push("--ephemeral".to_string());
        }
        args.extend([
            "--json".to_string(),
            "--output-last-message".to_string(),
            final_output_path.display().to_string(),
        ]);
        match resume_mode {
            CodexResumeMode::Fresh => args.push("-".to_string()),
            CodexResumeMode::ResumeId(native_session_id) => {
                args.push("resume".to_string());
                args.push(native_session_id);
                args.push("-".to_string());
            }
            CodexResumeMode::ResumeLast => {
                args.push("resume".to_string());
                args.push("--last".to_string());
                args.push("-".to_string());
            }
        }
        args
    }

    pub fn run_paths(&self, invocation: &GenericCliInvocation) -> Result<GenericCliRunPaths> {
        Ok(generic_cli_run_paths(
            invocation,
            self.config.output_dir.as_deref(),
            CODEX_CLI_DRIVER_ID,
            "codex-stdout.jsonl",
            "codex-stderr.log",
            "codex-observation.jsonl",
        ))
    }
}

fn codex_resume_mode(invocation: &GenericCliInvocation) -> CodexResumeMode {
    let Some(session) = invocation.route_session.as_ref() else {
        return CodexResumeMode::Fresh;
    };
    if let Some(id) = session
        .native_session_id
        .as_deref()
        .filter(|value| validate_native_session_id(CODEX_CLI_DRIVER_ID, value))
    {
        return CodexResumeMode::ResumeId(id.to_string());
    }
    if session.last_message_id.is_some() {
        return CodexResumeMode::ResumeLast;
    }
    CodexResumeMode::Fresh
}

pub(crate) fn codex_profile_auth_ready(config_home: &Path) -> bool {
    let auth_path = config_home.join("auth.json");
    auth_path
        .metadata()
        .map(|metadata| metadata.is_file() && metadata.len() > 0)
        .unwrap_or(false)
}

fn apply_codex_env(
    command: &mut Command,
    invocation: &GenericCliInvocation,
    socket_path: &std::path::Path,
    config: &CodexDriverConfig,
) {
    command.env_clear();
    apply_minimal_process_env(command);
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
        .env("CODEX_HOME", &config.config_home)
        .env(
            "AWIKI_DAEMON_RUNTIME_RPC_TOKEN",
            &invocation.runtime_rpc_token,
        );
}

fn apply_codex_probe_env(command: &mut Command, config: &CodexDriverConfig) {
    command.env_clear();
    apply_minimal_process_env(command);
    command.env("CODEX_HOME", &config.config_home);
}

fn apply_minimal_process_env(command: &mut Command) {
    if let Some(path) = codex_child_path() {
        command.env("PATH", path);
    } else if let Some(value) = std::env::var_os("PATH") {
        command.env("PATH", value);
    }
    for key in ["LANG", "LC_ALL", "LC_CTYPE", "LC_MESSAGES", "TERM"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
}

fn default_codex_binary_path() -> PathBuf {
    codex_child_path()
        .and_then(|path| find_executable_on_path(DEFAULT_CODEX_BINARY, &path))
        .unwrap_or_else(|| PathBuf::from(DEFAULT_CODEX_BINARY))
}

fn codex_child_path() -> Option<OsString> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    build_codex_child_path(home.as_deref(), std::env::var_os("PATH").as_deref())
}

fn build_codex_child_path(home: Option<&Path>, current_path: Option<&OsStr>) -> Option<OsString> {
    let mut paths = Vec::<PathBuf>::new();
    if let Some(home) = home {
        push_existing_dir(&mut paths, home.join(".local").join("bin"));
        push_existing_dir(&mut paths, home.join(".npm-global").join("bin"));
        push_existing_dir(&mut paths, home.join(".nvm").join("current").join("bin"));
        push_nvm_node_bins(&mut paths, home);
    }
    push_existing_dir(&mut paths, PathBuf::from("/opt/homebrew/bin"));
    push_existing_dir(&mut paths, PathBuf::from("/usr/local/bin"));
    if let Some(current_path) = current_path {
        for path in std::env::split_paths(current_path) {
            push_path(&mut paths, path);
        }
    }
    if paths.is_empty() {
        return None;
    }
    std::env::join_paths(paths).ok()
}

fn push_nvm_node_bins(paths: &mut Vec<PathBuf>, home: &Path) {
    let versions_dir = home.join(".nvm").join("versions").join("node");
    let Ok(entries) = std::fs::read_dir(versions_dir) else {
        return;
    };
    let mut bins = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("bin"))
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    bins.sort();
    bins.reverse();
    for bin in bins {
        push_path(paths, bin);
    }
}

fn push_existing_dir(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if path.is_dir() {
        push_path(paths, path);
    }
}

fn push_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if path.as_os_str().is_empty() || paths.iter().any(|existing| existing == &path) {
        return;
    }
    paths.push(path);
}

fn find_executable_on_path(name: &str, path: &OsStr) -> Option<PathBuf> {
    std::env::split_paths(path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_child_path_adds_common_user_cli_bins_before_service_path() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let v18 = home.join(".nvm/versions/node/v18.1.0/bin");
        let v24 = home.join(".nvm/versions/node/v24.12.0/bin");
        let local_bin = home.join(".local/bin");
        std::fs::create_dir_all(&v18).unwrap();
        std::fs::create_dir_all(&v24).unwrap();
        std::fs::create_dir_all(&local_bin).unwrap();

        let path = build_codex_child_path(Some(&home), Some(OsStr::new("/usr/bin:/bin")))
            .expect("path should be built");
        let entries = std::env::split_paths(&path).collect::<Vec<_>>();

        assert_eq!(entries[0], local_bin);
        assert_eq!(entries[1], v24);
        assert_eq!(entries[2], v18);
        assert!(entries.contains(&PathBuf::from("/usr/bin")));
        assert!(entries.contains(&PathBuf::from("/bin")));
    }
}

pub fn codex_native_session_id_from_stdout_jsonl(stdout: &[u8]) -> Option<String> {
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
            .filter(|id| validate_native_session_id(CODEX_CLI_DRIVER_ID, id))
        {
            return Some(id);
        }
    }
    None
}

fn native_session_id_from_json_value(value: &Value) -> Option<String> {
    for field in ["session_id", "thread_id"] {
        if let Some(id) = json_string_field(value, field) {
            return Some(id);
        }
    }
    for field in ["session", "thread", "rollout"] {
        if let Some(id) = value.get(field).and_then(|nested| {
            json_string_field(nested, "id").or_else(|| json_string_field(nested, "session_id"))
        }) {
            return Some(id);
        }
    }
    value
        .get("msg")
        .and_then(native_session_id_from_json_value)
        .or_else(|| {
            value
                .get("event")
                .and_then(native_session_id_from_json_value)
        })
}

pub fn build_prompt_envelope(
    invocation: &GenericCliInvocation,
    workspace_root: &std::path::Path,
    config: &CodexDriverConfig,
) -> String {
    build_generic_cli_prompt_envelope(GenericCliPromptEnvelope {
        invocation,
        workspace_root,
        driver_id: CODEX_CLI_DRIVER_ID,
        sandbox: &config.sandbox,
        driver_runtime_context: &[],
    })
}

fn ensure_prompt_does_not_contain_token(prompt: &str, token: &str) -> Result<()> {
    if prompt.contains(token) {
        bail!("Codex prompt envelope must not contain runtime RPC token");
    }
    Ok(())
}
