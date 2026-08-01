use super::*;
use crate::agent::AgentDefinition;

pub(super) fn runtime_callback_outbox(
    config: &DaemonConfig,
    state: &DaemonState,
    im_core: &ImCoreAdapter,
    mock_status_outbox: bool,
) -> Result<Arc<Mutex<RuntimeCallbackOutbox>>> {
    if state.list_agent_definitions()?.is_empty() {
        bail!("foreground requires at least one configured agent identity");
    }
    Ok(Arc::new(Mutex::new(RuntimeCallbackOutbox::new(
        config.clone(),
        state.clone(),
        im_core.clone(),
        mock_status_outbox,
    ))))
}

pub(super) fn sync_configured_agent_identities(
    config: &DaemonConfig,
    state: &DaemonState,
    im_core: &ImCoreAdapter,
) -> Result<ConfiguredAgentIdentitySyncSummary> {
    let migration_client = crate::registration::UserServiceAgentRegistrationClient::new(
        &config.user_service_base_url,
    )?;
    let mut summary = ConfiguredAgentIdentitySyncSummary::default();
    for agent in state.list_agent_definitions()? {
        match sync_one_configured_agent_identity(config, state, im_core, &migration_client, &agent)
        {
            Ok(ConfiguredAgentIdentitySyncOutcome::Active) => {
                summary.active += 1;
            }
            Ok(ConfiguredAgentIdentitySyncOutcome::Revoked) => {
                summary.revoked += 1;
                state.insert_audit_event_json(
                    "agent.identity.revoked_skipped",
                    Some(&agent.agent_did),
                    None,
                    None,
                    None,
                    json!({ "reason": "auth_revoked_fence" }),
                )?;
            }
            Err(error) => {
                summary.failed += 1;
                state.insert_audit_event_json(
                    "agent.identity.sync.failed",
                    Some(&agent.agent_did),
                    None,
                    None,
                    None,
                    json!({
                        "error": sanitize_error_message(&format!("{error:#}")),
                    }),
                )?;
            }
        }
    }
    Ok(summary)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct ConfiguredAgentIdentitySyncSummary {
    pub(super) active: usize,
    pub(super) revoked: usize,
    pub(super) failed: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConfiguredAgentIdentitySyncOutcome {
    Active,
    Revoked,
}

fn sync_one_configured_agent_identity(
    config: &DaemonConfig,
    state: &DaemonState,
    im_core: &ImCoreAdapter,
    migration_client: &crate::registration::UserServiceAgentRegistrationClient,
    agent: &AgentDefinition,
) -> Result<ConfiguredAgentIdentitySyncOutcome> {
    if let Some(identity) = state.load_agent_device_identity(&agent.agent_did)? {
        if identity.identity_status == "revoked" && identity.authorization_status == "revoked" {
            return Ok(ConfiguredAgentIdentitySyncOutcome::Revoked);
        }
        if identity.identity_status != "active" || identity.authorization_status != "active" {
            bail!(
                "agent_device_identity_unavailable: closed identity state is {}/{}",
                identity.identity_status,
                identity.authorization_status
            );
        }
    } else {
        crate::legacy_migration::migrate_legacy_agent_identity(
            config,
            state,
            im_core,
            migration_client,
            agent,
        )
        .with_context(|| {
            format!(
                "migrate exact device identity for Agent {}",
                agent.agent_did
            )
        })?;
    }
    let _client = im_core
        .client_for_agent(config, state, &agent.agent_did)
        .with_context(|| {
            format!(
                "validate exact device identity for Agent {}",
                agent.agent_did
            )
        })?;
    Ok(ConfiguredAgentIdentitySyncOutcome::Active)
}

#[cfg(test)]
mod identity_sync_tests {
    use super::*;

    fn definition(identity: &crate::state::AgentDeviceIdentityRecord) -> AgentDefinition {
        let (local_agent_db_path, message_db_path) =
            crate::agent::agent_data_paths(&identity.agent_did).unwrap();
        let is_runtime = identity.agent_kind == crate::agent::AgentKind::Runtime;
        AgentDefinition {
            agent_did: identity.agent_did.clone(),
            handle: identity.handle.clone(),
            agent_kind: identity.agent_kind,
            controller_user_id: format!("controller-{}", identity.identity_id),
            controller_full_handle: "controller.awiki.info".to_owned(),
            controller_scope_key: format!("controller-scope:v1:{}", identity.identity_id),
            controller_did: "did:wba:awiki.info:controller".to_owned(),
            runtime_plugin_id: is_runtime.then(|| "generic-cli".to_owned()),
            runtime_profile_id: is_runtime.then(|| format!("profile-{}", identity.identity_id)),
            workspace_id: None,
            policy_id: "default".to_owned(),
            local_agent_db_path,
            message_db_path,
            status: "active".to_owned(),
        }
    }

    #[test]
    fn revoked_or_corrupt_agent_does_not_block_healthy_sibling_restart_sync() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        let state = DaemonState::open_with_root_key_bytes(&config, [92_u8; 32]);
        state.initialize().unwrap();
        let im_core = ImCoreAdapter::open(&config).unwrap();
        let healthy = crate::registration::store_mock_vnext_device_identity(
            &config,
            &state,
            crate::agent::AgentKind::Daemon,
            "healthy-sibling",
        )
        .unwrap();
        let revoked = crate::registration::store_mock_vnext_device_identity(
            &config,
            &state,
            crate::agent::AgentKind::Runtime,
            "revoked-sibling",
        )
        .unwrap();
        for identity in [&healthy, &revoked] {
            state
                .upsert_agent_definition(&definition(identity))
                .unwrap();
        }
        let mut corrupt = definition(&healthy);
        corrupt.agent_did = "did:wba:awiki.info:agent:daemon:missing:e1_missing".to_owned();
        corrupt.handle = "missing.awiki.info".to_owned();
        corrupt.local_agent_db_path = "agents/missing/agent.db".to_owned();
        corrupt.message_db_path = "agents/missing/messages.db".to_owned();
        state.upsert_agent_definition(&corrupt).unwrap();
        state
            .mark_agent_device_auth_revoked(&revoked.agent_did)
            .unwrap();
        state
            .mark_v2_subprotocol_negotiated(&healthy.agent_did)
            .unwrap();
        state
            .mark_sync_v2_reconcile_completed(&healthy.agent_did)
            .unwrap();

        let summary = sync_configured_agent_identities(&config, &state, &im_core).unwrap();

        assert_eq!(
            summary,
            ConfiguredAgentIdentitySyncSummary {
                active: 1,
                revoked: 1,
                failed: 1,
            }
        );
        im_core
            .client_for_agent(&config, &state, &healthy.agent_did)
            .unwrap();
        assert!(im_core
            .client_for_agent(&config, &state, &revoked.agent_did)
            .is_err());
        let probe = state.load_sync_probe().unwrap();
        assert!(!probe.v2_subprotocol_negotiated);
        assert!(!probe.v2_bootstrap_completed);

        let connection = state.connection().unwrap();
        let revoked_audits: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM audit_log WHERE event_type = 'agent.identity.revoked_skipped' AND agent_did = ?1",
                [&revoked.agent_did],
                |row| row.get(0),
            )
            .unwrap();
        let corrupt_audits: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM audit_log WHERE event_type = 'agent.identity.sync.failed' AND agent_did = ?1",
                [&corrupt.agent_did],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(revoked_audits, 1);
        assert_eq!(corrupt_audits, 1);
    }
}

#[cfg(unix)]
pub(super) fn start_runtime_rpc_worker(
    socket_path: PathBuf,
    state: DaemonState,
    outbox: Arc<Mutex<RuntimeCallbackOutbox>>,
    queue_notifier: QueueSchedulerNotifier,
) -> Result<RuntimeRpcWorker> {
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let listener = bind_uds_listener(&socket_path)?;
    verify_socket_permissions(&socket_path)?;
    let worker_stop = Arc::clone(&stop);
    let handle = thread::Builder::new()
        .name("awiki-daemon-local-rpc".to_string())
        .spawn(move || {
            while !worker_stop.load(std::sync::atomic::Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        if let Ok(outbox) = outbox.lock() {
                            if handle_uds_stream_with_outbox(&state, &*outbox, stream).is_ok() {
                                queue_notifier.notify_all();
                            }
                        }
                    }
                    Err(error) if error.kind() == ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        })
        .context("spawn daemon local RPC worker")?;
    Ok(RuntimeRpcWorker {
        stop,
        handle: Some(handle),
    })
}

#[cfg(not(unix))]
pub(super) fn start_runtime_rpc_worker(
    _socket_path: PathBuf,
    _state: DaemonState,
    _outbox: Arc<Mutex<RuntimeCallbackOutbox>>,
    _queue_notifier: QueueSchedulerNotifier,
) -> Result<RuntimeRpcWorker> {
    bail!("daemon long-running local RPC requires Unix domain sockets")
}

pub(super) struct RuntimeRpcWorker {
    stop: Arc<std::sync::atomic::AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl RuntimeRpcWorker {
    pub(super) fn stop(mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

pub(super) fn write_ready_file(path: &Path, status: &crate::DaemonStatus) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(
        path,
        serde_json::to_vec_pretty(&json!({
            "ready": true,
            "state_root": status.state_root,
            "local_socket_path": status.local_socket_path,
            "daemon_schema_version": status.daemon_schema_version,
            "im_core_schema_version": status.im_core_schema_version,
            "sync_probe": status.sync_probe,
        }))?,
    )?;
    Ok(())
}
