use super::{
    client,
    store::{self, Session, Work},
};
use crate::runtime::host::{
    flush_runtime_final_outbox, runtime_final_outbox_record, RuntimeTaskRunResult,
};
use crate::security::runtime_token::RpcMethod;
use crate::{
    outbox::RuntimeOutbox,
    runtime::{
        RuntimeAgentProfile, RuntimeLaunchOutcome, RuntimeRun, RuntimeRunStatus, RuntimeTask,
    },
    state::AuthorizedRuntimeContext,
    DaemonState,
};
use agent_client_protocol::schema::v1::{ContentBlock, TextContent};
use anyhow::{Context, Result};
use serde_json::{json, Value};

pub fn run(
    state: &DaemonState,
    profile: &RuntimeAgentProfile,
    outbox: &impl RuntimeOutbox,
    task: RuntimeTask,
    run_id: String,
    socket: Option<&std::path::Path>,
) -> Result<RuntimeTaskRunResult> {
    task.validate()?;
    profile.validate()?;
    let run = RuntimeRun {
        run_id: run_id.clone(),
        task_id: task.task_id.clone(),
        agent_did: task.agent_did.clone(),
        runtime_profile_id: profile.runtime_profile_id.clone(),
        runtime_plugin_id: super::PLUGIN_ID.into(),
        workspace_id: profile.workspace_id.clone(),
        status: RuntimeRunStatus::Pending,
    };
    state.insert_runtime_task(&task)?;
    if !state.try_insert_runtime_run(&run)? {
        return Ok(result(run));
    }
    let work = Work { task, run_id };
    let session = Session::new(&work.task);
    let key = session.key.clone();
    match store::mutate(state, &key, Some(session), |s| s.submit(work.clone())) {
        Ok(true) => {
            execute(state, profile, outbox, &key, work, socket)?;
        }
        Ok(false) => {}
        Err(error) => {
            state.fail_active_runtime_run(&work.run_id)?;
            let rejection = json!({"schema":"awiki.acp.rejection.v1","agent_did":profile.agent_did,"conversation_id":work.task.conversation_id,"source_message_id":work.task.correlation().source_message_id,"reason":error.to_string()});
            state.connection()?.execute("INSERT OR IGNORE INTO acp_events(event_id,session_key,run_id,snapshot) VALUES(?1,?2,?3,?4)",rusqlite::params![format!("acp-rejected:{}",work.run_id),format!("rejected:{}",work.run_id),work.run_id,serde_json::to_string(&rejection)?])?;
        }
    }
    Ok(result(state.load_runtime_run(&run.run_id)?))
}

fn result(run: RuntimeRun) -> RuntimeTaskRunResult {
    RuntimeTaskRunResult {
        launch_outcome: RuntimeLaunchOutcome {
            run_id: run.run_id.clone(),
            status: run.status.clone(),
            exit_code: None,
            callbacks: vec![],
            metadata: json!({"protocol":"acp"}),
        },
        run,
        token_id: "acp-host".into(),
    }
}

pub fn execute(
    state: &DaemonState,
    profile: &RuntimeAgentProfile,
    outbox: &impl RuntimeOutbox,
    key: &str,
    mut work: Work,
    socket: Option<&std::path::Path>,
) -> Result<()> {
    loop {
        let output = execute_turn(state, profile, key, &work, socket);
        let mut completed = None;
        let outcome = match output {
            Ok(output) if output.cancelled => {
                state.fail_active_runtime_run(&work.run_id)?;
                "cancelled"
            }
            Ok(output) if !output.text.trim().is_empty() => {
                let persisted = (|| {
                    let run = state.load_runtime_run(&work.run_id)?;
                    let record = runtime_final_outbox_record(
                        profile,
                        &work.task.controller_did,
                        &work.task.reply_recipient_did,
                        &run,
                        work.task.conversation_id.as_deref(),
                        &output.text,
                        "acp",
                    )?;
                    store::finish_with_final(state, key, &record)
                })();
                match persisted {
                    Ok((cancelled, next)) => {
                        completed = Some(next);
                        if cancelled {
                            state.fail_active_runtime_run(&work.run_id)?;
                            "cancelled"
                        } else {
                            let _ = flush_runtime_final_outbox(state, outbox, 20);
                            "finished"
                        }
                    }
                    Err(_) => {
                        store::mutate(state, key, None, |s| {
                            s.interaction_error = Some("final_persistence_failed".into());
                            Ok(())
                        })?;
                        state.fail_active_runtime_run(&work.run_id)?;
                        "failed"
                    }
                }
            }
            failed => {
                let code = failed.err().map(|e| e.to_string()).unwrap_or_default();
                let code = match code.as_str() {
                    "attachment_download_failed"
                    | "attachment_changed"
                    | "attachment_unavailable"
                    | "image_too_large" => code.as_str(),
                    _ => "acp_turn_failed",
                };
                store::mutate(state, key, None, |s| {
                    if s.interaction_error.is_none() {
                        s.interaction_error = Some(code.into());
                    }
                    Ok(())
                })?;
                state.fail_active_runtime_run(&work.run_id)?;
                "failed"
            }
        };
        let next = match completed {
            Some(next) => next,
            None => store::mutate(state, key, None, |s| s.complete(&work.run_id, outcome))?,
        };
        match next {
            Some(next) => work = next,
            None => break,
        }
    }
    Ok(())
}

struct TokenGuard {
    state: DaemonState,
    id: String,
}

pub(super) fn file_wrapper_command(brand: super::Brand) -> &'static str {
    if brand == super::Brand::DeepseekHarness {
        // DSH's official shellEnv contribution survives its ambient-secret
        // scrub. Restore only this task-scoped RPC credential for the wrapper.
        r#"AWIKI_RUNTIME_RPC_TOKEN="$DSH_AWIKI_RUNTIME_RPC_TOKEN" "$AWIKI_DAEMON_EXECUTABLE""#
    } else {
        r#""$AWIKI_DAEMON_EXECUTABLE""#
    }
}
impl Drop for TokenGuard {
    fn drop(&mut self) {
        let _ = self.state.revoke_runtime_token(&self.id);
    }
}

fn execute_turn(
    state: &DaemonState,
    profile: &RuntimeAgentProfile,
    key: &str,
    work: &Work,
    socket: Option<&std::path::Path>,
) -> Result<client::TurnResult> {
    if store::load(state, key)?.stopping {
        return Ok(client::TurnResult {
            text: String::new(),
            cancelled: true,
        });
    }
    let cli = state.load_cli_runtime_profile(&profile.runtime_profile_id)?;
    let base = profile
        .workspace_root
        .as_ref()
        .context("acp_workspace_required")?;
    let cwd = base.join("acp").join(key);
    std::fs::create_dir_all(&cwd)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&cwd, std::fs::Permissions::from_mode(0o700))?;
    }
    state.update_runtime_run_status(&work.run_id, RuntimeRunStatus::Running)?;
    let issued =
        crate::runtime::host::issue_acp_runtime_token(state, profile, &work.task, &work.run_id)?;
    let _token = TokenGuard {
        state: state.clone(),
        id: issued.token_id.clone(),
    };
    let invocation = crate::plugins::generic_cli::GenericCliInvocation {
        run_id: work.run_id.clone(),
        task_id: work.task.task_id.clone(),
        message_id: work.task.correlation().source_message_id,
        conversation_id: work.task.conversation_id.clone(),
        preferred_language: profile.preferred_language.clone(),
        context: crate::plugins::generic_cli::GenericCliInvocationContext::from_task(&work.task),
        task_text: work.task.text.clone(),
        agent_did: profile.agent_did.clone(),
        runtime_profile_id: profile.runtime_profile_id.clone(),
        workspace_root: Some(cwd.clone()),
        workspace_instance: None,
        route_session: None,
        runtime_temp_dir: None,
        runtime_rpc_token: String::new(),
        local_socket_path: socket.map(std::path::Path::to_path_buf),
        callbacks: vec![],
    };
    let mut text = crate::plugins::generic_cli::render_invocation_context_prompt(&invocation);
    text.push_str(&format!("\n[AWiki file delivery]\nWhen authorized to send a file, create the output within the current working directory and use the existing wrapper: {} __runtime-wrapper send-attachment --file <absolute-path> --display-filename <name> --caption <text>. Authentication comes from the process environment. Never print credentials or put them in command arguments. Report a wrapper failure accurately. When you need user input, call awiki_questions request_user_input (or native elicitation) and wait for the actual response. Never choose an answer on their behalf. Treat decline/cancel as final and do not repeat the same question through another tool.\n", file_wrapper_command(super::Brand::parse(&cli.driver_id)?)));
    let mut prompt = vec![ContentBlock::Text(TextContent::new(text))];
    prompt.extend(super::attachments::prompt_blocks(state, &work.task)?);
    prompt.push(ContentBlock::Text(TextContent::new(format!(
        "[User request]\n{}",
        work.task.text
    ))));
    let mut environment = vec![
        ("AWIKI_DAEMON_RUN_ID".into(), work.run_id.clone()),
        ("AWIKI_DAEMON_TASK_ID".into(), work.task.task_id.clone()),
        ("AWIKI_DAEMON_AGENT_DID".into(), profile.agent_did.clone()),
        (
            "AWIKI_DAEMON_RUNTIME_PROFILE_ID".into(),
            profile.runtime_profile_id.clone(),
        ),
        (
            "AWIKI_RUNTIME_RPC_TOKEN".into(),
            issued.token.as_str().to_owned(),
        ),
        (
            "AWIKI_DAEMON_EXECUTABLE".into(),
            std::env::current_exe()?.to_string_lossy().into_owned(),
        ),
    ];
    if let Some(socket) = socket {
        environment.push((
            "AWIKI_DAEMON_RPC_SOCKET".into(),
            socket.to_string_lossy().into_owned(),
        ));
    }
    let turn = client::Turn {
        state: state.clone(),
        key: key.to_string(),
        run_id: work.run_id.clone(),
        profile: cli,
        cwd,
        prompt,
        environment,
    };
    // Foreground calls us from Handle::block_on on its blocking worker.
    // Keep the ACP executor on its own thread: nesting Tokio block_on would
    // panic, and synchronous reliable delivery must not stall protocol I/O.
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        let output = (|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?
                .block_on(client::run(turn))
        })();
        let _ = sender.send(output);
    });
    let mut control_error = None;
    let output = loop {
        match receiver.recv_timeout(std::time::Duration::from_millis(250)) {
            Ok(output) => break output,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                break Err(anyhow::anyhow!("acp_executor_exited"));
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                let checked: Result<()> = (|| {
                    if let Some(binding) = state.load_runtime_daemon_binding(&profile.agent_did)? {
                        if crate::agent_status::controller_identity_change_observed(
                            state,
                            &binding.daemon_agent_did,
                        )? {
                            store::mutate(state, key, None, |s| {
                                s.stopping = true;
                                s.execute_after_stop = false;
                                Ok(())
                            })?;
                        }
                    }
                    Ok(())
                })();
                if let Err(error) = checked {
                    control_error = Some(error);
                    let _ = store::mutate(state, key, None, |s| {
                        s.stopping = true;
                        s.execute_after_stop = false;
                        Ok(())
                    });
                }
            }
        }
    };
    worker
        .join()
        .map_err(|_| anyhow::anyhow!("acp_executor_panicked"))?;
    if let Some(error) = control_error {
        return Err(error);
    }
    output
}

fn context(profile: &RuntimeAgentProfile, run: &str) -> AuthorizedRuntimeContext {
    AuthorizedRuntimeContext {
        token_id: "acp-host".into(),
        agent_did: profile.agent_did.clone(),
        runtime_profile_id: profile.runtime_profile_id.clone(),
        run_id: run.into(),
        method: RpcMethod::TaskStatus,
    }
}

pub fn flush_events(
    state: &DaemonState,
    outbox: &impl RuntimeOutbox,
    limit: usize,
) -> Result<usize> {
    let db = state.connection()?;
    let rows = db
        .prepare(
            "SELECT event_id,run_id,snapshot FROM acp_events WHERE sent=0 ORDER BY rowid LIMIT ?1",
        )?
        .query_map([limit as i64], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut sent = 0;
    let mut first_error = None;
    for (event, run_id, raw) in rows {
        let delivered: Result<bool> = (|| {
            let snapshot: Value = serde_json::from_str(&raw)?;
            let run = state.load_runtime_run(&run_id)?;
            let profile = state.load_runtime_agent_profile(&run.agent_did)?;
            let task = state.load_runtime_task(&run.task_id)?;
            if let Some(binding) = state.load_runtime_daemon_binding(&run.agent_did)? {
                if crate::agent_status::controller_identity_change_observed(
                    state,
                    &binding.daemon_agent_did,
                )? {
                    return Ok(false);
                }
            }
            let phase = if snapshot["stopping"] == true {
                "stopping"
            } else if snapshot["active"].is_object() {
                "running"
            } else {
                snapshot["last_task"]["state"]
                    .as_str()
                    .unwrap_or("finished")
            };
            let rejected = snapshot["schema"] == "awiki.acp.rejection.v1";
            let field = if rejected { "acp_rejection" } else { "acp" };
            let metadata = json!({field:snapshot,"acp_event_id":event});
            outbox.send_status_with_metadata(
                &context(&profile, &run_id),
                if rejected { "failed" } else { phase },
                None,
                None,
                None,
                Some(&metadata),
            )?;
            // Publish in the actual conversation as a hidden control message too:
            // group members and authorized direct requesters need the same durable
            // state even when they do not own the daemon's controller channel.
            let target = if task.conversation_scope.kind()
                == crate::runtime::RuntimeConversationScopeKind::GroupVisible
            {
                let group = task
                    .conversation_id
                    .as_deref()
                    .and_then(crate::runtime::reply_payload::group_did_from_conversation_id)
                    .context("acp_group_identity_missing")?;
                crate::outbox::RuntimeMessageTarget::Group {
                    group: group.to_owned(),
                }
            } else {
                crate::outbox::RuntimeMessageTarget::Direct {
                    recipient: task.reply_recipient_did.clone(),
                    raw_recipient: task.reply_recipient_did.clone(),
                    resolved_did: Some(task.reply_recipient_did.clone()),
                }
            };
            outbox.send_message(
                &context(&profile, &run_id),
                &crate::outbox::RuntimeMessageSend {
                    target,
                    text: String::new(),
                    payload: Some(
                        json!({"schema":"awiki.acp.status.v1","event_id":event,field:snapshot}),
                    ),
                    file_path: None,
                    display_filename: None,
                    mime_type: None,
                    idempotency_key: Some(event.clone()),
                    security: crate::outbox::RuntimeMessageSecurity::DefaultPlain,
                },
            )?;
            db.execute("UPDATE acp_events SET sent=1 WHERE event_id=?1", [event])?;
            Ok(true)
        })();
        match delivered {
            Ok(true) => sent += 1,
            Ok(false) => {}
            Err(error) => {
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
    }
    if let Some(error) = first_error {
        return Err(error);
    }
    Ok(sent)
}

pub fn inspect_sync(profile: &crate::state::CliRuntimeProfileRecord) -> Result<Value> {
    // Creation may be called from a Tokio foreground worker.
    let profile = profile.clone();
    std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(client::inspect(&profile))
    })
    .join()
    .map_err(|_| anyhow::anyhow!("acp_probe_failed"))?
}
