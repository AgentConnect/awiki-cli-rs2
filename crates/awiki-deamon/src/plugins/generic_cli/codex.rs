use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};
use serde_json::Value;

use crate::runtime::{RuntimeInstallStatus, RuntimeRunStatus};
use crate::security::runtime_token::current_time_millis;
use crate::state::CliRuntimeProfileRecord;

use super::{GenericCliDriver, GenericCliExit, GenericCliInvocation};

const DEFAULT_CODEX_BINARY: &str = "codex";
const DEFAULT_SANDBOX: &str = "read-only";
const DEFAULT_CLI_WRAPPER: &str = "library:awiki_deamon::cli_wrapper";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexDriverConfig {
    pub binary_path: PathBuf,
    pub profile: Option<String>,
    pub model: Option<String>,
    pub sandbox: String,
    pub ignore_user_config: bool,
    pub ignore_rules: bool,
    pub ephemeral: bool,
    pub output_dir: Option<PathBuf>,
    pub cli_wrapper: String,
}

#[derive(Debug, Clone)]
pub struct CodexDriver {
    config: CodexDriverConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexRunPaths {
    pub output_dir: PathBuf,
    pub final_output_path: PathBuf,
    pub stdout_path: PathBuf,
    pub stderr_path: PathBuf,
    pub jsonl_path: PathBuf,
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
        let binary_path = profile
            .binary_path
            .clone()
            .or_else(|| string_field(config, "binary_path").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from(DEFAULT_CODEX_BINARY));
        let sandbox = string_field(config, "sandbox")
            .or_else(|| profile.default_sandbox.clone())
            .unwrap_or_else(|| DEFAULT_SANDBOX.to_string());
        let output_dir = string_field(config, "output_dir").map(PathBuf::from);
        let record = Self {
            binary_path,
            profile: string_field(config, "profile"),
            model: profile
                .default_model
                .clone()
                .or_else(|| string_field(config, "model")),
            sandbox,
            ignore_user_config: bool_field(config, "ignore_user_config", false),
            ignore_rules: bool_field(config, "ignore_rules", false),
            ephemeral: bool_field(config, "ephemeral", true),
            output_dir,
            cli_wrapper: string_field(config, "cli_wrapper")
                .unwrap_or_else(|| DEFAULT_CLI_WRAPPER.to_string()),
        };
        record.validate()?;
        Ok(record)
    }

    pub fn validate(&self) -> Result<()> {
        if self.binary_path.as_os_str().is_empty() {
            bail!("codex binary_path must not be empty");
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
        match Command::new(&self.config.binary_path)
            .arg("--version")
            .output()
        {
            Ok(output) if output.status.success() => {
                let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
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
                    output.status.code().unwrap_or(1)
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
            .workspace_root
            .clone()
            .context("Codex driver requires workspace_root")?;
        let socket_path = invocation
            .local_socket_path
            .clone()
            .context("Codex driver requires daemon local RPC socket")?;
        std::fs::create_dir_all(&workspace_root)
            .with_context(|| format!("create Codex workspace {}", workspace_root.display()))?;
        let paths = self.run_paths(&invocation)?;
        std::fs::create_dir_all(&paths.output_dir)
            .with_context(|| format!("create Codex output dir {}", paths.output_dir.display()))?;
        let prompt = build_prompt_envelope(&invocation, &workspace_root, &self.config);
        ensure_prompt_does_not_contain_token(&prompt, &invocation.runtime_rpc_token)?;

        let args = self.command_args(&workspace_root, &paths.final_output_path);
        let mut command = Command::new(&self.config.binary_path);
        command
            .args(&args)
            .current_dir(&workspace_root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("AWIKI_DAEMON_RUN_ID", &invocation.run_id)
            .env("AWIKI_DAEMON_TASK_ID", &invocation.task_id)
            .env("AWIKI_DAEMON_AGENT_DID", &invocation.agent_did)
            .env(
                "AWIKI_DAEMON_RUNTIME_PROFILE_ID",
                &invocation.runtime_profile_id,
            )
            .env("AWIKI_DAEMON_SOCKET", &socket_path)
            .env("AWIKI_DAEMON_CLI_WRAPPER", &self.config.cli_wrapper)
            .env(
                "AWIKI_DAEMON_RUNTIME_RPC_TOKEN",
                &invocation.runtime_rpc_token,
            )
            .env_remove("AWIKI_DAEMON_TASK_TEXT");

        let mut child = command.spawn().context("spawn codex exec")?;
        child
            .stdin
            .as_mut()
            .context("open codex stdin")?
            .write_all(prompt.as_bytes())
            .context("write codex prompt envelope to stdin")?;
        drop(child.stdin.take());
        let output = child.wait_with_output().context("wait for codex exec")?;
        std::fs::write(
            &paths.stdout_path,
            redact_token_bytes(&output.stdout, &invocation.runtime_rpc_token),
        )
        .with_context(|| format!("write {}", paths.stdout_path.display()))?;
        std::fs::write(
            &paths.stderr_path,
            redact_token_bytes(&output.stderr, &invocation.runtime_rpc_token),
        )
        .with_context(|| format!("write {}", paths.stderr_path.display()))?;
        redact_token_file(&paths.final_output_path, &invocation.runtime_rpc_token)?;
        std::fs::write(
            &paths.jsonl_path,
            serde_json::json!({
                "kind": "codex.exec.observation",
                "run_id": invocation.run_id,
                "stdout_path": paths.stdout_path,
                "stderr_path": paths.stderr_path,
                "final_output_path": paths.final_output_path,
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
        })
    }
}

impl CodexDriver {
    pub fn command_args(
        &self,
        workspace_root: &std::path::Path,
        final_output_path: &std::path::Path,
    ) -> Vec<String> {
        let mut args = vec![
            "exec".to_string(),
            "--cd".to_string(),
            workspace_root.display().to_string(),
            "--sandbox".to_string(),
            self.config.sandbox.clone(),
        ];
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
            "-".to_string(),
        ]);
        args
    }

    pub fn run_paths(&self, invocation: &GenericCliInvocation) -> Result<CodexRunPaths> {
        let output_dir = self.config.output_dir.clone().unwrap_or_else(|| {
            std::env::temp_dir()
                .join("awiki-deamon")
                .join("generic-cli")
                .join("codex")
                .join(sanitize_path_component(&invocation.run_id))
        });
        Ok(CodexRunPaths {
            final_output_path: output_dir.join("final-output.txt"),
            stdout_path: output_dir.join("codex-stdout.jsonl"),
            stderr_path: output_dir.join("codex-stderr.log"),
            jsonl_path: output_dir.join("codex-observation.jsonl"),
            output_dir,
        })
    }
}

pub fn build_prompt_envelope(
    invocation: &GenericCliInvocation,
    workspace_root: &std::path::Path,
    config: &CodexDriverConfig,
) -> String {
    format!(
        r#"[Awiki Runtime Context]
agent_did: {agent_did}
runtime_plugin_id: generic-cli
driver_id: codex
runtime_profile_id: {runtime_profile_id}
workspace_instance_path: {workspace_root}
sandbox: {sandbox}

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
        message_id = invocation.message_id,
        task_id = invocation.task_id,
        run_id = invocation.run_id,
        conversation_id = invocation.conversation_id.as_deref().unwrap_or(""),
        task_text = invocation.task_text,
    )
}

fn ensure_prompt_does_not_contain_token(prompt: &str, token: &str) -> Result<()> {
    if prompt.contains(token) {
        bail!("Codex prompt envelope must not contain runtime RPC token");
    }
    Ok(())
}

fn redact_token_bytes(bytes: &[u8], token: &str) -> Vec<u8> {
    let text = String::from_utf8_lossy(bytes);
    text.replace(token, "<redacted-runtime-rpc-token>")
        .into_bytes()
}

fn redact_token_file(path: &std::path::Path, token: &str) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let content =
        std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    if content.contains(token) {
        std::fs::write(path, content.replace(token, "<redacted-runtime-rpc-token>"))
            .with_context(|| format!("redact {}", path.display()))?;
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
