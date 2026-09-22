//! Serial, idempotent model preparation on the existing ACP control channel.
use super::{client, operations, store};
use crate::{
    runtime::{RuntimeAgentProfile, RuntimeConversationScope},
    DaemonState,
};
use anyhow::{bail, Context, Result};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde_json::Value;

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
    let action = args["action"].as_str().context("action_required")?;
    if action != "prepare_session" && action != "set_model" {
        bail!("unknown_acp_command");
    }
    if action == "set_model" && args["session_key"].as_str() != Some(seed.key.as_str()) {
        bail!("conversation_mismatch");
    }
    let gate = operations::session_gate(state, &seed.key);
    let _guard = gate
        .lock()
        .map_err(|_| anyhow::anyhow!("acp_configuration_interrupted"))?;
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
    let raw = db
        .query_row(
            "SELECT data FROM acp_sessions WHERE session_key=?1",
            [&seed.key],
            |r| r.get::<_, String>(0),
        )
        .optional()?;
    let mut session: store::Session = raw
        .map(|raw| serde_json::from_str(&raw))
        .transpose()?
        .unwrap_or(seed);
    if session.agent_did != profile.agent_did
        || session.controller_scope_key != profile.controller_scope_key
        || session.group
    {
        bail!("conversation_mismatch");
    }
    if session.context_lost {
        bail!("context_reset_required");
    }
    // A query during an active turn returns its confirmed configuration. It
    // must not open a second client over the same native session.
    if action == "prepare_session" && (session.active.is_some() || session.waiting.is_some()) {
        return Ok(session.snapshot());
    }
    if action == "set_model" {
        if args["revision"].as_u64() != Some(session.revision) {
            bail!("stale_revision");
        }
        session.command(
            action,
            args,
            sender,
            crate::security::runtime_token::current_time_millis()?,
        )?;
    }
    let cli = state.load_cli_runtime_profile(&profile.runtime_profile_id)?;
    let cli = super::hermes_profile::for_session(state, &cli, &session.key)?;
    let cwd = super::host::workspace(profile, &session.key)?;
    let native = session.native_session_id.clone();
    // Reapply the last confirmed model too: a failed/uncertain native switch
    // must not silently change the model used for the next prompt.
    let desired = session
        .model_selection()
        .or(cli.default_model.clone())
        .or(session.model.clone());
    let created = session.native_created_at_ms;
    drop(db);
    // Foreground's blocking worker may already have entered Tokio.
    let probe = std::thread::spawn(move || -> Result<client::PreparedConfiguration> {
        if cli.driver_id == "gemini" && native.is_some() {
            // Same checkpoint boundary as real turns; never overwrite native
            // history just to populate a model menu.
            if let Some(created) = created {
                let remaining = (created / 60_000 + 1) * 60_000 + 1_000
                    - crate::security::runtime_token::current_time_millis()?;
                if remaining > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(remaining as u64));
                }
            }
        }
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(client::prepare_configuration(cli, cwd, native, desired))
    })
    .join()
    .map_err(|_| anyhow::anyhow!("acp_configuration_interrupted"))?;
    let prepared = match probe {
        Ok(prepared) => prepared,
        Err(error) => {
            if error.to_string() == "context_reset_required" {
                store::mutate(state, &session.key, None, |s| {
                    s.context_lost = true;
                    Ok(())
                })?;
            }
            return Err(error);
        }
    };
    let mut db = state.connection()?;
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let current = tx
        .query_row(
            "SELECT data FROM acp_sessions WHERE session_key=?1",
            [&session.key],
            |r| r.get::<_, String>(0),
        )
        .optional()?;
    let current = current
        .map(|raw| serde_json::from_str::<store::Session>(&raw))
        .transpose()?;
    if current.as_ref().map_or(0, |s| s.revision) != session.revision {
        bail!("stale_revision");
    }
    session.capabilities = prepared.capabilities;
    session.update_configuration(prepared.options);
    store::save(&tx, &mut session)?;
    let result = session.snapshot();
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
    Ok(result)
}
