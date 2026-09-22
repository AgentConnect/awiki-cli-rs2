//! Read client-advertised choices without changing the conversation model.
use super::{client, operations, store};
use crate::{
    runtime::{RuntimeAgentProfile, RuntimeConversationScope},
    DaemonState,
};
use anyhow::{bail, Result};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;

fn response(session: &store::Session, refreshed: bool, retry: Option<i64>) -> Value {
    let mut refresh = json!({"state":if refreshed {"refreshed"} else {"deferred"}});
    if let Some(retry) = retry {
        refresh["retry_after_ms"] = json!(retry);
    }
    json!({"prepared_session_key":session.key,"sessions":[session.snapshot()],"model_refresh":refresh})
}

pub fn control(
    state: &DaemonState,
    profile: &RuntimeAgentProfile,
    sender: &str,
    conversation_id: Option<String>,
    command: &str,
    args: &Value,
) -> Result<Value> {
    if conversation_id.as_deref().is_none_or(str::is_empty) {
        bail!("conversation_mismatch");
    }
    let scope = RuntimeConversationScope::ControllerPrivate {
        controller_scope_key: profile.controller_scope_key.clone(),
    };
    let seed = store::Session::for_scope(
        &profile.agent_did,
        &profile.controller_scope_key,
        scope.scope_key(),
        conversation_id,
        false,
    );
    if args["action"] != "refresh_models" || args["session_key"].as_str() != Some(&seed.key) {
        bail!("conversation_mismatch");
    }
    let gate = operations::session_gate(state, &seed.key);
    let mut guard = match gate.refresh() {
        Ok(guard) => guard,
        Err(error) if error.to_string() == "model_refresh_deferred" => {
            return Ok(response(&store::load(state, &seed.key)?, false, None))
        }
        Err(error) => return Err(error),
    };
    let fingerprint = serde_json::to_string(&(sender, args))?;
    let db = state.connection()?;
    if let Some((request, result)) = db
        .query_row(
            "SELECT request,result FROM acp_commands WHERE agent_did=?1 AND command_id=?2",
            params![profile.agent_did, command],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .optional()?
    {
        if request != fingerprint {
            bail!("command_id_conflict");
        }
        return Ok(serde_json::from_str(&result)?);
    }
    drop(db);
    let mut session = store::load(state, &seed.key)?;
    if session.agent_did != profile.agent_did
        || session.controller_scope_key != profile.controller_scope_key
        || session.conversation_id != seed.conversation_id
        || session.group
    {
        bail!("conversation_mismatch");
    }
    if session.context_lost {
        bail!("context_reset_required");
    }
    if session.active.is_some() || session.waiting.is_some() || guard.cancel.load(Ordering::SeqCst)
    {
        return Ok(response(&session, false, None));
    }
    if guard.coalesced {
        if !guard.prior_succeeded() {
            return Ok(response(&session, false, None));
        }
        let result = response(&session, true, None);
        state.connection()?.execute(
            "INSERT INTO acp_commands(command_id,agent_did,request,result) VALUES(?1,?2,?3,?4)",
            params![
                command,
                profile.agent_did,
                fingerprint,
                serde_json::to_string(&result)?
            ],
        )?;
        return Ok(result);
    }
    let cli = state.load_cli_runtime_profile(&profile.runtime_profile_id)?;
    let cli = super::hermes_profile::for_session(state, &cli, &session.key)?;
    if cli.driver_id == "gemini" && session.native_session_id.is_some() {
        if let Some(created) = session.native_created_at_ms {
            let remaining = (created / 60_000 + 1) * 60_000 + 1_000
                - crate::security::runtime_token::current_time_millis()?;
            if remaining > 0 {
                return Ok(response(&session, false, Some(remaining)));
            }
        }
    }
    let cwd = super::host::workspace(profile, &session.key)?;
    let native = session.native_session_id.clone();
    let cancel = guard.cancel.clone();
    let probe = std::thread::spawn(move || -> Result<client::PreparedConfiguration> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(client::prepare_configuration_cancellable(
                cli, cwd, native, None, cancel,
            ))
    })
    .join()
    .map_err(|_| anyhow::anyhow!("acp_configuration_interrupted"))?;
    // Connection and its process group have ended before admission can resume.
    if guard.cancel.load(Ordering::SeqCst) {
        return Ok(response(&session, false, None));
    }
    let prepared = probe?;
    let mut db = state.connection()?;
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let raw: String = tx.query_row(
        "SELECT data FROM acp_sessions WHERE session_key=?1",
        [&session.key],
        |r| r.get(0),
    )?;
    let current: store::Session = serde_json::from_str(&raw)?;
    if current.revision != session.revision || guard.cancel.load(Ordering::SeqCst) {
        return Ok(response(&current, false, None));
    }
    // A restored client's startup default is not a confirmed model switch.
    // Only its choices and capabilities belong to this metadata operation.
    session.capabilities = prepared.capabilities;
    session.update_catalog(prepared.options);
    store::save(&tx, &mut session)?;
    let result = response(&session, true, None);
    tx.execute(
        "INSERT INTO acp_commands(command_id,agent_did,request,result) VALUES(?1,?2,?3,?4)",
        params![
            command,
            profile.agent_did,
            fingerprint,
            serde_json::to_string(&result)?
        ],
    )?;
    tx.commit()?;
    guard.mark_success();
    Ok(result)
}
