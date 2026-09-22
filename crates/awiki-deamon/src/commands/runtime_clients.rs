use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

static PENDING: AtomicUsize = AtomicUsize::new(0);
struct Pending;
impl Drop for Pending {
    fn drop(&mut self) {
        PENDING.fetch_sub(1, Ordering::SeqCst);
    }
}

pub(super) fn handle<O: AgentManagementOutbox + Clone + Send + 'static>(
    config: &DaemonConfig,
    state: &DaemonState,
    outbox: &O,
    daemon: &AgentDefinition,
    message: &IncomingAgentPayloadMessage,
    payload: &AgentCommandEnvelope,
) -> Result<()> {
    if payload
        .target_agent_kind
        .as_deref()
        .is_some_and(|k| k != "daemon")
    {
        bail!("runtime.clients.inspect requires daemon target");
    }
    let refresh = match payload.args.get("refresh") {
        None => false,
        Some(Value::Bool(value)) => *value,
        _ => bail!("runtime.clients.inspect refresh must be boolean"),
    };
    if PENDING.fetch_add(1, Ordering::SeqCst) >= 32 {
        PENDING.fetch_sub(1, Ordering::SeqCst);
        return send_command_status(
            outbox,
            daemon,
            message,
            &payload.command_id,
            "failed",
            None,
            json!({"command":"runtime.clients.inspect","error_code":"inspection_busy"}),
        );
    }
    let pending = Pending;
    let (config, state, outbox, daemon, message, id) = (
        config.clone(),
        state.clone(),
        outbox.clone(),
        daemon.clone(),
        message.clone(),
        payload.command_id.clone(),
    );
    std::thread::Builder::new()
        .name("awiki-client-inspection".into())
        .spawn(move || {
            let _pending = pending;
            let report = crate::runtime_clients::inspect(&config, refresh);
            // A late result cannot cross an authoritative controller change.
            let Ok(current) = state.load_agent_definition(&daemon.agent_did) else {
                return;
            };
            if current.controller_scope_key != daemon.controller_scope_key
                || current.status == "archived"
            {
                return;
            }
            let _ = send_command_status(
                &outbox,
                &daemon,
                &message,
                &id,
                "ready",
                None,
                json!({"command":"runtime.clients.inspect","installation":report}),
            );
        })
        .context("start client inspection worker")?;
    Ok(())
}

// Only stable codes reach UI; arbitrary child/process output stays private.
pub(super) fn creation_error_code(error: &anyhow::Error) -> String {
    let message = error.to_string();
    let code = message.split(':').next().unwrap_or("");
    if matches!(
        code,
        "runtime_client_not_found"
            | "runtime_client_not_executable"
            | "runtime_client_launch_failed"
            | "runtime_client_version_failed"
            | "runtime_client_gateway_module_missing"
            | "runtime_client_custom_launcher"
            | "runtime_client_timeout"
            | "runtime_client_node_missing"
            | "runtime_client_node_incompatible"
            | "runtime_client_node_unavailable"
            | "runtime_client_node_timeout"
            | "acp_setup_required"
            | "acp_version_timeout"
            | "acp_probe_timeout"
            | "acp_question_tool_unsupported"
            | "acp_version_unavailable"
    ) {
        code.to_owned()
    } else {
        "creation_failed".into()
    }
}
