use super::*;

#[derive(Debug, Clone)]
pub(super) struct UdsTestRuntimePlugin {
    socket_path: PathBuf,
}

impl UdsTestRuntimePlugin {
    pub(super) fn new(socket_path: PathBuf) -> Self {
        Self { socket_path }
    }
}

impl RuntimePlugin for UdsTestRuntimePlugin {
    fn plugin_id(&self) -> &str {
        "test-runtime-uds"
    }

    fn check_install_status(&self) -> Result<RuntimeInstallStatus> {
        Ok(RuntimeInstallStatus {
            installed: true,
            detail: Some("test runtime uses daemon UDS local RPC".to_string()),
        })
    }

    fn launch_run(&self, context: RuntimeLaunchContext) -> Result<RuntimeLaunchOutcome> {
        let token = context.runtime_rpc_token.as_str().to_string();
        ensure_runtime_rpc_success(call_uds_once(
            &self.socket_path,
            &CliWrapperRequest::task_status(
                token.clone(),
                context.task.task_id.clone(),
                "running",
                "runtime started",
            )
            .into_rpc_request(),
        )?)?;
        ensure_runtime_rpc_success(call_uds_once(
            &self.socket_path,
            &CliWrapperRequest::task_finish(token, context.task.task_id, "runtime finished")
                .into_rpc_request(),
        )?)?;
        Ok(RuntimeLaunchOutcome {
            run_id: context.run.run_id,
            status: RuntimeRunStatus::Finished,
            exit_code: Some(0),
            callbacks: Vec::new(),
            metadata: serde_json::json!({}),
        })
    }
}

fn ensure_runtime_rpc_success(response: crate::local_rpc::RuntimeRpcResponse) -> Result<()> {
    if response.ok {
        return Ok(());
    }
    let message = response
        .error
        .map(|error| format!("{}: {}", error.code, error.message))
        .unwrap_or_else(|| "runtime RPC returned ok=false".to_string());
    bail!(message)
}
