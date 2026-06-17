use super::*;

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

pub(super) fn store_agent_token_for_configured_agents(
    state: &DaemonState,
    token: &str,
) -> Result<()> {
    for agent in state.list_agent_definitions()? {
        state.store_agent_auth_token(&agent.agent_did, token)?;
    }
    Ok(())
}

pub(super) fn sync_configured_agent_identities(
    config: &DaemonConfig,
    state: &DaemonState,
    im_core: &ImCoreAdapter,
) -> Result<()> {
    for agent in state.list_agent_definitions()? {
        let identity = match state.load_agent_identity(&agent.agent_did) {
            Ok(identity) => identity,
            Err(_) => continue,
        };
        let jwt_token = state.load_agent_auth_token(&agent.agent_did)?;
        let _client = im_core.client_for_agent_identity(config, &identity, jwt_token.as_deref())?;
    }
    Ok(())
}

pub(super) async fn ensure_agent_messaging_session(
    client: &im_core::ImClient,
    agent_did: &str,
) -> Result<()> {
    match client
        .auth()
        .ensure_session_async(im_core::auth::AuthScope::Messaging)
        .await
    {
        Ok(_) => Ok(()),
        Err(_) => {
            client
                .auth()
                .refresh_session_async()
                .await
                .with_context(|| format!("refresh DID WBA session for agent {agent_did}"))?;
            client
                .auth()
                .ensure_session_async(im_core::auth::AuthScope::Messaging)
                .await
                .with_context(|| format!("ensure messaging session for agent {agent_did}"))?;
            Ok(())
        }
    }
}

#[cfg(unix)]
pub(super) fn start_runtime_rpc_worker(
    socket_path: PathBuf,
    state: DaemonState,
    outbox: Arc<Mutex<RuntimeCallbackOutbox>>,
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
                            let _ = handle_uds_stream_with_outbox(&state, &*outbox, stream);
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
        }))?,
    )?;
    Ok(())
}
