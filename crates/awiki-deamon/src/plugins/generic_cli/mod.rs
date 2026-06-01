use std::process::Command;

use anyhow::{bail, Context, Result};
use serde_json::Value;

pub mod codex;

use crate::cli_wrapper::CliWrapperRequest;
use crate::local_rpc::RuntimeRpcRequest;
use crate::runtime::{
    RuntimeInstallStatus, RuntimeLaunchContext, RuntimeLaunchOutcome, RuntimePlugin,
    RuntimeRunStatus,
};
use crate::state::CliRuntimeProfileRecord;

pub const GENERIC_CLI_RUNTIME_PLUGIN_ID: &str = crate::agent::GENERIC_CLI_RUNTIME_PLUGIN_ID;

pub trait GenericCliDriver {
    fn check_install_status(&self) -> Result<RuntimeInstallStatus>;
    fn run(&self, invocation: GenericCliInvocation) -> Result<GenericCliExit>;
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
            "command" => command_driver_from_profile(&self.cli_profile)?.check_install_status(),
            "codex" => codex::CodexDriver::from_profile(&self.cli_profile)?.check_install_status(),
            "claude-code" | "gemini" => Ok(RuntimeInstallStatus {
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
            "command" => {
                GenericCliRuntimePlugin::new(command_driver_from_profile(&self.cli_profile)?)
                    .launch_run(context)
            }
            "codex" => {
                GenericCliRuntimePlugin::new(codex::CodexDriver::from_profile(&self.cli_profile)?)
                    .launch_run(context)
            }
            "claude-code" | "gemini" => {
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
}

impl CommandGenericCliDriver {
    pub fn new(program: impl Into<std::path::PathBuf>, args: Vec<String>) -> Self {
        Self {
            program: program.into(),
            args,
            cli_wrapper: "library:awiki_deamon::cli_wrapper".to_string(),
        }
    }

    pub fn with_cli_wrapper(mut self, cli_wrapper: impl Into<String>) -> Self {
        self.cli_wrapper = cli_wrapper.into();
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
        let status = command.status().context("run generic CLI runtime")?;
        Ok(GenericCliExit::from_exit_code(status.code().unwrap_or(1)))
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
            })
        } else {
            Ok(GenericCliExit {
                exit_code: self.exit_code,
                status: RuntimeRunStatus::Failed,
                callbacks: vec![CliWrapperRequest::task_status(
                    invocation.runtime_rpc_token,
                    invocation.task_id,
                    "failed",
                    "runtime failed",
                )
                .into_rpc_request()],
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
    Ok(CommandGenericCliDriver::new(program, args).with_cli_wrapper(cli_wrapper))
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
