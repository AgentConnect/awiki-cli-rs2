use anyhow::{bail, Result};
use serde_json::json;

use crate::agent::AgentDefinition;
use crate::registration::{AgentInventoryClient, ControllerSenderScope, DidAuthMaterial};
use crate::state::{controller_scope_key, DaemonState};
use crate::DaemonConfig;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedControllerSender {
    pub controller_user_id: String,
    pub controller_full_handle: String,
    pub controller_scope_key: String,
    pub controller_did: String,
    pub sender_did: String,
}

impl VerifiedControllerSender {
    pub fn from_scope(scope: ControllerSenderScope) -> Result<Self> {
        let controller_scope_key =
            controller_scope_key(&scope.controller_user_id, &scope.controller_full_handle)?;
        Ok(Self {
            controller_user_id: scope.controller_user_id,
            controller_full_handle: scope.controller_full_handle,
            controller_scope_key,
            controller_did: scope.controller_did,
            sender_did: scope.sender_did,
        })
    }
}

pub fn verify_daemon_controller_sender<C>(
    config: &DaemonConfig,
    state: &DaemonState,
    client: &C,
    daemon_agent: &AgentDefinition,
    sender_did: &str,
) -> Result<VerifiedControllerSender>
where
    C: AgentInventoryClient,
{
    let auth = daemon_auth_material(config, state, daemon_agent)?;
    let scope =
        client.verify_controller_sender(&daemon_agent.agent_did, sender_did.trim(), &auth)?;
    let verified = VerifiedControllerSender::from_scope(scope)?;
    ensure_scope_matches_daemon(state, daemon_agent, &verified)?;
    Ok(verified)
}

pub fn sync_daemon_controller_scope<C>(
    config: &DaemonConfig,
    state: &DaemonState,
    client: &C,
    daemon_agent: &AgentDefinition,
) -> Result<()>
where
    C: AgentInventoryClient,
{
    let auth = daemon_auth_material(config, state, daemon_agent)?;
    let response = client.sync_controller_scope(&daemon_agent.agent_did, &auth)?;
    crate::agent_status::sync_controller_scope_from_response(
        state,
        &daemon_agent.agent_did,
        &response,
    )
}

pub fn daemon_auth_material(
    _config: &DaemonConfig,
    state: &DaemonState,
    daemon_agent: &AgentDefinition,
) -> Result<DidAuthMaterial> {
    let identity = state.load_agent_identity(&daemon_agent.agent_did)?;
    Ok(DidAuthMaterial {
        did_document: identity.did_document,
        private_key_pem: identity.auth_private_key_pem,
        bearer_token: state.load_agent_auth_token(&daemon_agent.agent_did)?,
    })
}

fn ensure_scope_matches_daemon(
    state: &DaemonState,
    daemon_agent: &AgentDefinition,
    verified: &VerifiedControllerSender,
) -> Result<()> {
    if daemon_agent.controller_user_id != verified.controller_user_id
        || daemon_agent.controller_full_handle != verified.controller_full_handle
        || daemon_agent.controller_scope_key != verified.controller_scope_key
    {
        state.insert_audit_event_json(
            "daemon.controller_sender_scope_mismatch",
            Some(&daemon_agent.agent_did),
            None,
            None,
            None,
            json!({
                "local_controller_user_id": daemon_agent.controller_user_id,
                "local_controller_full_handle": daemon_agent.controller_full_handle,
                "remote_controller_user_id": verified.controller_user_id,
                "remote_controller_full_handle": verified.controller_full_handle,
            }),
        )?;
        bail!("controller_scope_mismatch");
    }
    if daemon_agent.controller_did != verified.controller_did {
        state.update_controller_did_for_agent_family(
            &daemon_agent.agent_did,
            &verified.controller_did,
        )?;
        state.insert_audit_event_json(
            "daemon.controller_did.synced",
            Some(&daemon_agent.agent_did),
            None,
            None,
            None,
            json!({
                "old_controller_did": daemon_agent.controller_did,
                "new_controller_did": verified.controller_did,
                "source": "verify_controller_sender",
            }),
        )?;
    }
    Ok(())
}
