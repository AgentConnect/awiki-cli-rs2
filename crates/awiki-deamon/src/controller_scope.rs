use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{bail, Context, Result};
use serde_json::json;

use crate::agent::AgentDefinition;
use crate::registration::{AgentInventoryClient, ControllerSenderScope, DidAuthMaterial};
use crate::state::{controller_scope_key, DaemonState};
use crate::DaemonConfig;

type ControllerReconcileLockMap = BTreeMap<String, Arc<Mutex<()>>>;

static CONTROLLER_RECONCILE_LOCKS: OnceLock<Mutex<ControllerReconcileLockMap>> = OnceLock::new();

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
    with_controller_reconcile_singleflight(daemon_agent, || {
        let daemon_agent = state.load_agent_definition(&daemon_agent.agent_did)?;
        let auth = daemon_auth_material(config, state, &daemon_agent)?;
        let scope =
            client.verify_controller_sender(&daemon_agent.agent_did, sender_did.trim(), &auth)?;
        let verified = VerifiedControllerSender::from_scope(scope)?;
        reconcile_authoritative_controller_scope(
            state,
            &daemon_agent,
            Some(&verified.controller_user_id),
            Some(&verified.controller_full_handle),
            Some(&verified.controller_scope_key),
            &verified.controller_did,
            "verified_controller_sender",
            "daemon.controller_sender_scope_mismatch",
        )?;
        Ok(verified)
    })
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
    with_controller_reconcile_singleflight(daemon_agent, || {
        let daemon_agent = state.load_agent_definition(&daemon_agent.agent_did)?;
        let auth = daemon_auth_material(config, state, &daemon_agent)?;
        let response = client.sync_controller_scope(&daemon_agent.agent_did, &auth)?;
        crate::agent_status::sync_controller_scope_from_response(
            state,
            &daemon_agent.agent_did,
            &response,
        )
    })
}

pub(crate) fn with_controller_reconcile_singleflight<T>(
    daemon_agent: &AgentDefinition,
    operation: impl FnOnce() -> Result<T>,
) -> Result<T> {
    let key = format!(
        "{}\u{1f}{}",
        daemon_agent.agent_did, daemon_agent.controller_scope_key
    );
    let lock = {
        let mut locks = CONTROLLER_RECONCILE_LOCKS
            .get_or_init(|| Mutex::new(BTreeMap::new()))
            .lock()
            .map_err(|_| anyhow::anyhow!("controller reconcile lock map is poisoned"))?;
        Arc::clone(locks.entry(key).or_insert_with(|| Arc::new(Mutex::new(()))))
    };
    let _guard = lock
        .lock()
        .map_err(|_| anyhow::anyhow!("controller reconcile lock is poisoned"))?;
    operation()
}

pub fn daemon_auth_material(
    _config: &DaemonConfig,
    state: &DaemonState,
    daemon_agent: &AgentDefinition,
) -> Result<DidAuthMaterial> {
    let identity = state
        .load_agent_device_identity(&daemon_agent.agent_did)?
        .context("agent_identity_migration_required: exact daemon device identity is missing")?;
    identity.validate()?;
    if identity.identity_status != "active" || identity.authorization_status != "active" {
        bail!("agent_device_identity_unavailable: daemon device identity is not active");
    }
    Ok(DidAuthMaterial {
        did_document: identity.did_document,
        private_key_pem: identity.device_signing_private_key_pem,
        bearer_token: Some(identity.access_token),
    })
}

pub(crate) fn reconcile_authoritative_controller_scope(
    state: &DaemonState,
    daemon_agent: &AgentDefinition,
    authoritative_controller_user_id: Option<&str>,
    authoritative_controller_full_handle: Option<&str>,
    authoritative_controller_scope_key: Option<&str>,
    authoritative_controller_did: &str,
    source: &'static str,
    mismatch_event_type: &'static str,
) -> Result<()> {
    let did_changed = authoritative_controller_did != daemon_agent.controller_did;
    let stable_scope_missing = did_changed
        && (authoritative_controller_user_id.is_none()
            || authoritative_controller_full_handle.is_none());
    if stable_scope_missing
        || authoritative_controller_user_id
            .is_some_and(|value| value != daemon_agent.controller_user_id)
        || authoritative_controller_full_handle
            .is_some_and(|value| value != daemon_agent.controller_full_handle)
        || authoritative_controller_scope_key
            .is_some_and(|value| value != daemon_agent.controller_scope_key)
    {
        state.insert_audit_event_json(
            mismatch_event_type,
            Some(&daemon_agent.agent_did),
            None,
            None,
            None,
            json!({
                "local_controller_user_id": daemon_agent.controller_user_id,
                "local_controller_full_handle": daemon_agent.controller_full_handle,
                "remote_controller_user_id": authoritative_controller_user_id,
                "remote_controller_full_handle": authoritative_controller_full_handle,
            }),
        )?;
        bail!("controller_scope_mismatch");
    }
    state.rebind_controller_did_for_agent_family(
        &daemon_agent.agent_did,
        &daemon_agent.controller_user_id,
        &daemon_agent.controller_full_handle,
        &daemon_agent.controller_scope_key,
        authoritative_controller_did,
        source,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;
    use crate::agent::AgentKind;

    fn controller_rebind_daemon() -> AgentDefinition {
        AgentDefinition {
            agent_did: "did:agent:daemon-controller-singleflight".to_owned(),
            handle: "alice-daemon".to_owned(),
            agent_kind: AgentKind::Daemon,
            controller_user_id: "user-alice".to_owned(),
            controller_full_handle: "alice.awiki.info".to_owned(),
            controller_scope_key: "controller-scope:v1:user-alice:alice.awiki.info".to_owned(),
            controller_did: "did:human:alice-old".to_owned(),
            runtime_plugin_id: None,
            runtime_profile_id: None,
            workspace_id: None,
            policy_id: "default".to_owned(),
            local_agent_db_path: "agents/daemon/agent.db".to_owned(),
            message_db_path: "agents/daemon/messages.db".to_owned(),
            status: "active".to_owned(),
        }
    }

    #[test]
    fn controller_rebind_singleflight_covers_authoritative_query_through_apply() {
        let daemon = controller_rebind_daemon();
        let first_daemon = daemon.clone();
        let (first_started_tx, first_started_rx) = mpsc::channel();
        let (release_first_tx, release_first_rx) = mpsc::channel();
        let first = std::thread::spawn(move || {
            with_controller_reconcile_singleflight(&first_daemon, || {
                first_started_tx.send(()).unwrap();
                release_first_rx.recv().unwrap();
                Ok(())
            })
            .unwrap();
        });
        first_started_rx.recv().unwrap();

        let second_daemon = daemon;
        let (second_started_tx, second_started_rx) = mpsc::channel();
        let second = std::thread::spawn(move || {
            with_controller_reconcile_singleflight(&second_daemon, || {
                second_started_tx.send(()).unwrap();
                Ok(())
            })
            .unwrap();
        });
        assert!(second_started_rx
            .recv_timeout(Duration::from_millis(100))
            .is_err());

        release_first_tx.send(()).unwrap();
        first.join().unwrap();
        second_started_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        second.join().unwrap();
    }
}
