use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::cli_wrapper::CliWrapperRequest;
use crate::local_rpc::RuntimeRpcRequest;
use crate::runtime::{
    RuntimeInstallStatus, RuntimeLaunchContext, RuntimeLaunchOutcome, RuntimePlugin,
    RuntimeRunStatus,
};

pub trait GenericCliDriver {
    fn check_install_status(&self) -> Result<RuntimeInstallStatus>;
    fn run(&self, invocation: GenericCliInvocation) -> Result<GenericCliExit>;
}

#[derive(Clone, PartialEq, Eq)]
pub struct GenericCliInvocation {
    pub run_id: String,
    pub task_id: String,
    pub task_text: String,
    pub workspace_root: Option<std::path::PathBuf>,
    pub runtime_rpc_token: String,
    pub callbacks: Vec<RuntimeRpcRequest>,
}

impl std::fmt::Debug for GenericCliInvocation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GenericCliInvocation")
            .field("run_id", &self.run_id)
            .field("task_id", &self.task_id)
            .field("task_text", &self.task_text)
            .field("workspace_root", &self.workspace_root)
            .field("runtime_rpc_token", &"<redacted>")
            .field("callbacks", &self.callbacks)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenericCliExit {
    pub exit_code: i32,
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
            task_text: context.task.text.clone(),
            workspace_root: context.workspace_root.clone(),
            runtime_rpc_token: context.runtime_rpc_token.as_str().to_string(),
            callbacks: callbacks.clone(),
        })?;
        let status = if exit.exit_code == 0 {
            RuntimeRunStatus::Finished
        } else {
            RuntimeRunStatus::Failed
        };
        let callbacks = if exit.exit_code == 0 {
            callbacks
        } else {
            vec![CliWrapperRequest::task_status(
                context.runtime_rpc_token.as_str().to_string(),
                context.task.task_id.clone(),
                "failed",
                "runtime failed",
            )
            .into_rpc_request()]
        };
        Ok(RuntimeLaunchOutcome {
            run_id: context.run.run_id,
            status,
            exit_code: Some(exit.exit_code),
            callbacks,
        })
    }
}

#[derive(Debug, Clone)]
pub struct CommandGenericCliDriver {
    program: std::path::PathBuf,
    args: Vec<String>,
}

impl CommandGenericCliDriver {
    pub fn new(program: impl Into<std::path::PathBuf>, args: Vec<String>) -> Self {
        Self {
            program: program.into(),
            args,
        }
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
        command
            .env("AWIKI_DAEMON_RUN_ID", &invocation.run_id)
            .env("AWIKI_DAEMON_TASK_ID", &invocation.task_id)
            .env("AWIKI_DAEMON_TASK_TEXT", &invocation.task_text)
            .env(
                "AWIKI_DAEMON_RUNTIME_RPC_TOKEN",
                &invocation.runtime_rpc_token,
            );
        let status = command.status().context("run generic CLI runtime")?;
        Ok(GenericCliExit {
            exit_code: status.code().unwrap_or(1),
        })
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

    fn run(&self, _invocation: GenericCliInvocation) -> Result<GenericCliExit> {
        Ok(GenericCliExit {
            exit_code: self.exit_code,
        })
    }
}
