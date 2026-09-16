use super::*;
use crate::acp::{self, store};

pub(super) fn handle(
    config: &DaemonConfig,
    state: &DaemonState,
    im_core: &ImCoreAdapter,
    registration: &UserServiceAgentRegistrationClient,
    target: &str,
    sender: &str,
    payload: &Value,
) -> Result<()> {
    let profile = state.load_runtime_agent_profile(target)?;
    if profile.runtime_plugin_id != acp::PLUGIN_ID {
        bail!("not_acp_runtime");
    }
    let args = &payload["args"];
    let command = payload["command_id"]
        .as_str()
        .filter(|v| !v.is_empty() && v.len() <= 128)
        .context("command_id_required")?;
    let action = args["action"].as_str().context("action_required")?;
    if let Some(binding) = state.load_runtime_daemon_binding(target)? {
        if crate::agent_status::controller_identity_change_observed(
            state,
            &binding.daemon_agent_did,
        )? {
            bail!("controller_identity_changed")
        }
    }
    // Answers have the narrower per-task requester authority, including groups.
    if action != "answer" {
        verify_runtime_controller_sender(config, state, registration, target, sender)?;
    }
    let status_sender = runtime_status_sender_for_agent(config, state, im_core, target)?;
    let outcome: Result<(Value, Option<store::Work>)> = (|| {
        if action == "query" {
            let db = state.connection()?;
            let sessions = db
                .prepare("SELECT data FROM acp_sessions WHERE agent_did=?1")?
                .query_map([target], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let snapshots = sessions
                .into_iter()
                .map(|raw| serde_json::from_str::<store::Session>(&raw))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .filter(|s| s.controller_scope_key == profile.controller_scope_key)
                .map(|s| s.snapshot())
                .collect::<Vec<_>>();
            let cli = state.load_cli_runtime_profile(&profile.runtime_profile_id)?;
            return Ok((
                json!({"sessions":snapshots,"driver_id":cli.driver_id}),
                None,
            ));
        }
        let (snapshot, work) = store::control(state, target, sender, command, args)?;
        Ok((json!({"sessions":[snapshot]}), work))
    })();
    let (data, work, code) = match outcome {
        Ok((data, work)) => (data, work, None),
        Err(error) => (
            json!({}),
            None,
            Some(sanitize_error_message(&error.to_string())),
        ),
    };
    let response = json!({
        "schema":"awiki.agent.status.v1","event_id":format!("acp-command:{target}:{command}"),
        "daemon_agent_did":status_sender.daemon_agent_did,"status_scope":"acp",
        "state":if code.is_some(){"failed"}else{"succeeded"},"command_id":command,
        "runtime_agent_did":target,"acp_result":data,"error_code":code,
        "sent_at":time::OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339)?,
        "runtimes":[],"runs":[]
    });
    let delivery = status_sender.sender.send_payload(sender, response.clone());
    let mut direct_response = response;
    direct_response["schema"] = json!("awiki.acp.command-result.v1");
    let direct_delivery = runtime_message_sender_for_agent(config, state, im_core, target)
        .and_then(|direct_sender| direct_sender.send_payload(sender, direct_response));
    if let Some(work) = work {
        let key = args["session_key"]
            .as_str()
            .context("session_key_required")?;
        let outbox =
            RuntimeCallbackOutbox::new(config.clone(), state.clone(), im_core.clone(), false);
        acp::host::execute(
            state,
            &profile,
            &outbox,
            key,
            work,
            Some(&config.local_socket_path),
        )?;
    }
    delivery.and(direct_delivery).map(|_| ())
}
