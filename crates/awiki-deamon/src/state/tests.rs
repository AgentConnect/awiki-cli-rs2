use super::schema::DAEMON_SCHEMA_VERSION;
use super::*;
use crate::runtime::RuntimeTaskTriggerKind;

#[test]
fn initialize_creates_required_tables() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let summary = DaemonState::open(&config).unwrap().initialize().unwrap();
    assert_eq!(summary.schema_version, DAEMON_SCHEMA_VERSION);

    let connection = Connection::open(&config.daemon_db_path).unwrap();
    for table in [
        "schema_migrations",
        "daemon_state_metadata",
        "agent_definition",
        "runtime_profile",
        "workspace_binding",
        "runtime_task",
        "runtime_run",
        "runtime_rpc_tokens",
        "audit_log",
        "agent_identity",
        "agent_auth_state",
        "cli_runtime_profile",
        "cli_driver_run",
        "cli_route_sessions",
        "cli_runtime_locks",
        "hermes_profiles",
        "hermes_native_sessions",
        "runtime_daemon_binding",
        "agent_status_query_throttle",
        "runtime_retry_queue",
        "runtime_agent_create_request",
        "runtime_final_outbox",
        "user_delegated_identity",
        "bootstrap_replay",
        "app_message_agent_binding",
        "inbox_cursor",
        "processed_message",
        "message_event",
        "message_sync_outbox",
        "control_command_state",
    ] {
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "missing table {table}");
    }
    assert!(DaemonState::open(&config)
        .unwrap()
        .generic_cli_route_hash_salt_present());
    let salt_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM daemon_state_metadata WHERE key = 'generic_cli.route_hash_salt.v2'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(salt_count, 1);
}

fn cli_route_create(workspace: PathBuf, conversation_id: &str) -> CreateCliRouteSession {
    CreateCliRouteSession {
        agent_did: "did:agent:codex-alice".to_string(),
        runtime_profile_id: "profile_codex_alice".to_string(),
        driver_id: "codex".to_string(),
        controller_user_id: "user-alice".to_string(),
        controller_full_handle: "alice.awiki.info".to_string(),
        controller_scope_key: "controller-scope:v1:test-alice".to_string(),
        controller_did: "did:human:alice".to_string(),
        conversation_id: conversation_id.to_string(),
        workspace_path: workspace.join("workspaces").join("route"),
        session_dir: workspace.join("sessions").join("route"),
    }
}

#[test]
fn cli_route_session_canonicalizes_direct_ids_and_roundtrips() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();

    let direct = state
        .get_or_create_cli_route_session(cli_route_create(
            root.path().to_path_buf(),
            "dm:did:human:bob",
        ))
        .unwrap();
    assert_eq!(direct.conversation_id, "direct:did:human:bob");
    assert!(direct.route_key.contains(":direct:did:human:bob:"));
    assert!(direct.route_key_hash.starts_with("route_"));
    assert_eq!(direct.route_key_hash.len(), "route_".len() + 24);
    assert!(format!("{direct:?}").contains("route_"));
    assert_ne!(
        direct.route_key_hash,
        cli_route_key_hash(&direct.route_key).unwrap()
    );

    let same = state
        .get_or_create_cli_route_session(cli_route_create(
            root.path().to_path_buf(),
            "direct:did:human:bob",
        ))
        .unwrap();
    assert_eq!(same.route_key, direct.route_key);
    assert_eq!(same.route_key_hash, direct.route_key_hash);

    let group = state
        .get_or_create_cli_route_session(cli_route_create(
            root.path().to_path_buf(),
            "group:did:group:writers",
        ))
        .unwrap();
    assert_ne!(group.route_key_hash, direct.route_key_hash);
    assert_eq!(
        state
            .count_cli_route_sessions_for_runtime_profile(
                "profile_codex_alice",
                "controller-scope:v1:test-alice",
                Some("active"),
            )
            .unwrap(),
        2
    );
}

#[test]
fn cli_route_hash_is_keyed_per_daemon_state_root() {
    let first_root = tempfile::tempdir().unwrap();
    let first_config = DaemonConfig::for_state_root(first_root.path()).unwrap();
    let first_state = DaemonState::open(&first_config).unwrap();
    first_state.initialize().unwrap();

    let first = first_state
        .get_or_create_cli_route_session(cli_route_create(
            first_root.path().to_path_buf(),
            "direct:did:human:bob",
        ))
        .unwrap();
    let same = first_state
        .get_or_create_cli_route_session(cli_route_create(
            first_root.path().to_path_buf(),
            "dm:did:human:bob",
        ))
        .unwrap();
    assert_eq!(same.route_key_hash, first.route_key_hash);
    assert!(first_state.generic_cli_route_hash_salt_present());

    let second_root = tempfile::tempdir().unwrap();
    let second_config = DaemonConfig::for_state_root(second_root.path()).unwrap();
    let second_state = DaemonState::open(&second_config).unwrap();
    second_state.initialize().unwrap();
    let second = second_state
        .get_or_create_cli_route_session(cli_route_create(
            second_root.path().to_path_buf(),
            "direct:did:human:bob",
        ))
        .unwrap();

    assert_eq!(second.route_key, first.route_key);
    assert_ne!(second.route_key_hash, first.route_key_hash);
}

#[test]
fn cli_route_existing_plain_hash_record_keeps_legacy_path_binding() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();

    let create = cli_route_create(root.path().to_path_buf(), "direct:did:human:bob");
    let route_key = create.route_key().unwrap();
    let legacy_hash = cli_route_key_hash(&route_key).unwrap();
    let legacy_workspace = root
        .path()
        .join("runtime/workspaces/profile_codex_alice/conversations")
        .join(&legacy_hash);
    let legacy_session_dir = root
        .path()
        .join("runtime/sessions/profile_codex_alice")
        .join(&legacy_hash);
    Connection::open(&config.daemon_db_path)
        .unwrap()
        .execute(
            r#"
INSERT INTO cli_route_sessions (
    route_key,
    route_key_hash,
    agent_did,
    runtime_profile_id,
    driver_id,
    controller_user_id,
    controller_full_handle,
    controller_scope_key,
    controller_did,
    conversation_id,
    workspace_path,
    session_dir,
    synthetic_session_id,
    status,
    version,
    created_at_ms,
    updated_at_ms
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?1, 'active', 0, 1, 1)
"#,
            rusqlite::params![
                route_key,
                legacy_hash,
                create.agent_did,
                create.runtime_profile_id,
                create.driver_id,
                create.controller_user_id,
                create.controller_full_handle,
                create.controller_scope_key,
                create.controller_did,
                canonical_cli_conversation_id(&create.conversation_id).unwrap(),
                legacy_workspace.display().to_string(),
                legacy_session_dir.display().to_string(),
            ],
        )
        .unwrap();

    let loaded = state
        .get_or_create_cli_route_session(CreateCliRouteSession {
            workspace_path: legacy_workspace.clone(),
            session_dir: legacy_session_dir.clone(),
            ..cli_route_create(root.path().to_path_buf(), "dm:did:human:bob")
        })
        .unwrap();

    assert_eq!(loaded.route_key_hash, legacy_hash);
    assert_eq!(loaded.workspace_path, legacy_workspace);
    assert_eq!(loaded.session_dir, legacy_session_dir);
    assert_ne!(
        loaded.route_key_hash,
        state.cli_route_key_hash(&loaded.route_key).unwrap()
    );
}

#[test]
fn cli_route_session_rejects_no_conversation_and_lease_is_exclusive() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();

    let error = cli_route_create(root.path().to_path_buf(), "no-conversation")
        .route_key()
        .unwrap_err();
    assert!(error.to_string().contains("no-conversation"));

    let session = state
        .get_or_create_cli_route_session(cli_route_create(
            root.path().to_path_buf(),
            "direct:did:human:bob",
        ))
        .unwrap();
    assert!(state
        .try_acquire_cli_route_session_lease(
            &session.route_key,
            "run_1",
            "test",
            current_time_millis().unwrap() + 60_000,
        )
        .unwrap());
    assert!(!state
        .try_acquire_cli_route_session_lease(
            &session.route_key,
            "run_2",
            "test",
            current_time_millis().unwrap() + 60_000,
        )
        .unwrap());
    state
        .release_cli_route_session_lease(
            &session.route_key,
            "run_1",
            "active",
            Some("msg_1"),
            None,
            None,
        )
        .unwrap();
    assert!(state
        .try_acquire_cli_route_session_lease(
            &session.route_key,
            "run_2",
            "test",
            current_time_millis().unwrap() + 60_000,
        )
        .unwrap());
    assert_eq!(
        state
            .reset_cli_route_session_by_route(&session.route_key)
            .unwrap(),
        1
    );
    let reset = state
        .load_cli_route_session(&session.route_key)
        .unwrap()
        .unwrap();
    assert_eq!(reset.status, "reset");
    assert_eq!(reset.lock_run_id, None);
}

#[test]
fn cli_route_session_reset_reactivates_same_route_without_native_pointer() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();

    let session = state
        .get_or_create_cli_route_session(cli_route_create(
            root.path().to_path_buf(),
            "direct:did:human:bob",
        ))
        .unwrap();
    state
        .update_cli_route_session_native_id(
            &session.route_key,
            Some("native_old"),
            Some("json_event"),
            Some("synthetic_old"),
        )
        .unwrap();
    assert_eq!(
        state
            .reset_cli_route_session_by_route(&session.route_key)
            .unwrap(),
        1
    );

    let reactivated = state
        .get_or_create_cli_route_session(cli_route_create(
            root.path().to_path_buf(),
            "dm:did:human:bob",
        ))
        .unwrap();

    assert_eq!(reactivated.route_key, session.route_key);
    assert_eq!(reactivated.status, "active");
    assert_eq!(reactivated.native_session_id, None);
    assert_eq!(reactivated.native_session_source, None);
    assert_eq!(
        reactivated.synthetic_session_id.as_deref(),
        Some(session.route_key.as_str())
    );
    assert!(state
        .try_acquire_cli_route_session_lease(
            &reactivated.route_key,
            "run_after_reset",
            "test",
            current_time_millis().unwrap() + 60_000,
        )
        .unwrap());
}

#[test]
fn cli_runtime_profile_lock_is_exclusive_and_stale_recoverable() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();

    let now = current_time_millis().unwrap();
    assert!(state
        .try_acquire_cli_runtime_profile_lock(
            "profile_codex_alice",
            "codex",
            "run_1",
            "test",
            now + 60_000,
        )
        .unwrap());
    assert!(!state
        .try_acquire_cli_runtime_profile_lock(
            "profile_codex_alice",
            "codex",
            "run_2",
            "test",
            now + 60_000,
        )
        .unwrap());
    assert_eq!(
        state
            .count_cli_runtime_locks(
                Some("profile"),
                Some("profile_codex_alice"),
                Some("codex"),
                false,
            )
            .unwrap(),
        1
    );
    assert!(state
        .release_cli_runtime_profile_lock("profile_codex_alice", "run_1")
        .unwrap());
    assert!(state
        .try_acquire_cli_runtime_profile_lock(
            "profile_codex_alice",
            "codex",
            "run_expired",
            "test",
            now - 1,
        )
        .unwrap());
    assert!(state
        .try_acquire_cli_runtime_profile_lock(
            "profile_codex_alice",
            "codex",
            "run_2",
            "test",
            now + 60_000,
        )
        .unwrap());
    assert!(!state
        .release_cli_runtime_profile_lock("profile_codex_alice", "run_expired")
        .unwrap());
    assert!(state
        .release_cli_runtime_profile_lock("profile_codex_alice", "run_2")
        .unwrap());
    assert_eq!(
        state
            .count_cli_runtime_locks(
                Some("profile"),
                Some("profile_codex_alice"),
                Some("codex"),
                true,
            )
            .unwrap(),
        0
    );
}

#[test]
fn cli_host_home_lock_is_driver_scoped() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();

    let now = current_time_millis().unwrap();
    assert!(state
        .try_acquire_cli_host_home_lock("claude-code", "run_1", "test", now + 60_000)
        .unwrap());
    assert!(!state
        .try_acquire_cli_host_home_lock("claude-code", "run_2", "test", now + 60_000)
        .unwrap());
    assert!(state
        .try_acquire_cli_host_home_lock("codex", "run_3", "test", now + 60_000)
        .unwrap());
    assert_eq!(
        state
            .count_cli_runtime_locks(Some("host-home"), None, Some("claude-code"), false)
            .unwrap(),
        1
    );
    assert!(state
        .release_cli_host_home_lock("claude-code", "run_1")
        .unwrap());
    assert!(state.release_cli_host_home_lock("codex", "run_3").unwrap());
}

#[test]
fn cli_route_session_rejects_existing_route_binding_drift() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();

    let session = state
        .get_or_create_cli_route_session(cli_route_create(
            root.path().to_path_buf(),
            "direct:did:human:bob",
        ))
        .unwrap();
    let mut drifted = cli_route_create(root.path().to_path_buf(), "dm:did:human:bob");
    drifted.workspace_path = root.path().join("workspaces").join("different-route");

    let error = state.get_or_create_cli_route_session(drifted).unwrap_err();
    assert!(error.to_string().contains("binding conflict"));
    let unchanged = state
        .load_cli_route_session(&session.route_key)
        .unwrap()
        .unwrap();
    assert_eq!(unchanged.workspace_path, session.workspace_path);
}

#[test]
fn cli_route_session_allows_controller_did_rotation_for_same_scope() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();

    let session = state
        .get_or_create_cli_route_session(cli_route_create(
            root.path().to_path_buf(),
            "direct:did:human:bob",
        ))
        .unwrap();
    let mut rotated = cli_route_create(root.path().to_path_buf(), "dm:did:human:bob");
    rotated.controller_did = "did:human:alice#rotated".to_string();

    let loaded = state.get_or_create_cli_route_session(rotated).unwrap();
    assert_eq!(loaded.route_key, session.route_key);
    assert_eq!(loaded.controller_did, "did:human:alice");
}

fn delegated_identity_fixture() -> (UserDelegatedIdentityRecord, BootstrapReplayRecord) {
    let identity = UserDelegatedIdentityRecord {
        user_did: "did:wba:example.com:user:alice:e1_user".to_string(),
        verification_method: "did:wba:example.com:user:alice:e1_user#daemon-key-1".to_string(),
        app_instance_id: "app_1".to_string(),
        controller_did: "did:wba:example.com:user:alice:e1_user".to_string(),
        daemon_agent_did: "did:agent:daemon".to_string(),
        public_key_multibase: "z-public".to_string(),
        private_key_material: "z-private-secret".to_string(),
        allowed_scopes_json: serde_json::json!([
            "message.inbox.read.plain",
            "message.history.read.plain",
            "message.send.plain"
        ]),
        status: "paired_key_received".to_string(),
        expires_at: Some("2026-09-09T00:00:00Z".to_string()),
        bootstrap_id: "boot_1".to_string(),
        idempotency_key: "message-agent-bootstrap:did:wba:example.com:user:alice:e1_user:app_1"
            .to_string(),
        created_at_ms: 0,
        updated_at_ms: 0,
    };
    let replay = BootstrapReplayRecord {
        bootstrap_id: identity.bootstrap_id.clone(),
        idempotency_key: identity.idempotency_key.clone(),
        payload_hash: "payload-hash-1".to_string(),
        user_did: identity.user_did.clone(),
        verification_method: identity.verification_method.clone(),
        app_instance_id: identity.app_instance_id.clone(),
        daemon_agent_did: identity.daemon_agent_did.clone(),
        status: identity.status.clone(),
        created_at_ms: 0,
        updated_at_ms: 0,
    };
    (identity, replay)
}

#[test]
fn user_delegated_identity_roundtrips_and_replays_idempotently() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let (identity, replay) = delegated_identity_fixture();

    assert_eq!(
        state.store_bootstrap_state(&identity, &replay).unwrap(),
        BootstrapStoreOutcome::Inserted
    );
    assert_eq!(
        state.store_bootstrap_state(&identity, &replay).unwrap(),
        BootstrapStoreOutcome::Duplicate
    );

    let loaded = state
        .load_user_delegated_identity(&identity.verification_method)
        .unwrap()
        .unwrap();
    assert_eq!(loaded.user_did, identity.user_did);
    assert_eq!(loaded.private_key_material, "z-private-secret");
    assert_eq!(loaded.status, "paired_key_received");
    assert!(!format!("{loaded:?}").contains("z-private-secret"));

    let replay_loaded = state.load_bootstrap_replay("boot_1").unwrap().unwrap();
    assert_eq!(replay_loaded.payload_hash, "payload-hash-1");

    let reopened = DaemonState::open(&config).unwrap();
    let recovered = reopened
        .load_user_delegated_identity(&identity.verification_method)
        .unwrap()
        .unwrap();
    assert_eq!(recovered.status, "paired_key_received");
}

#[test]
fn user_delegated_identity_rejects_conflicting_replay() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let (identity, mut replay) = delegated_identity_fixture();
    state.store_bootstrap_state(&identity, &replay).unwrap();
    replay.payload_hash = "payload-hash-2".to_string();

    let error = state
        .store_bootstrap_state(&identity, &replay)
        .unwrap_err()
        .to_string();
    assert!(error.contains("replay conflict"));
}

#[test]
fn app_message_agent_binding_roundtrips_and_restores_active_record() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let record = AppMessageAgentBindingRecord {
        binding_id: "app-message-agent:did:human:alice:app_1".to_string(),
        user_did: "did:human:alice".to_string(),
        inbox_auth_verification_method: "did:human:alice#daemon-key-1".to_string(),
        app_instance_id: "app_1".to_string(),
        bootstrap_id: "boot_1".to_string(),
        idempotency_key: "message-agent-bootstrap:did:human:alice:app_1".to_string(),
        daemon_agent_did: "did:agent:daemon".to_string(),
        runtime_agent_did: "did:agent:runtime-hermes".to_string(),
        runtime_profile_id: "profile_hermes_app_message".to_string(),
        role: "app_message_handler".to_string(),
        desired_agent_json: serde_json::json!({
            "role": "app_message_handler",
            "runtime": "hermes"
        }),
        capability_policy_json: serde_json::json!({
            "allowed_actions": ["message.summarize_plain"]
        }),
        status: "message_agent_ready".to_string(),
        created_at_ms: 0,
        updated_at_ms: 0,
        revoked_at_ms: None,
    };

    state.upsert_app_message_agent_binding(&record).unwrap();
    let loaded = state
        .load_active_app_message_agent_binding("did:human:alice", "app_1", "app_message_handler")
        .unwrap()
        .unwrap();
    assert_eq!(loaded.binding_id, record.binding_id);
    assert_eq!(loaded.runtime_agent_did, "did:agent:runtime-hermes");

    let reopened = DaemonState::open(&config).unwrap();
    let restored = reopened
        .load_app_message_agent_binding(&record.binding_id)
        .unwrap()
        .unwrap();
    assert_eq!(restored.status, "message_agent_ready");
}

#[test]
fn app_message_agent_binding_revokes_superseded_records_for_same_user_role() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let mut first = AppMessageAgentBindingRecord {
        binding_id: "app-message-agent:did:human:alice:app_1".to_string(),
        user_did: "did:human:alice".to_string(),
        inbox_auth_verification_method: "did:human:alice#daemon-key-1".to_string(),
        app_instance_id: "app_1".to_string(),
        bootstrap_id: "boot_1".to_string(),
        idempotency_key: "message-agent-bootstrap:did:human:alice:app_1".to_string(),
        daemon_agent_did: "did:agent:daemon".to_string(),
        runtime_agent_did: "did:agent:runtime-hermes-1".to_string(),
        runtime_profile_id: "profile_hermes_app_message_1".to_string(),
        role: "app_message_handler".to_string(),
        desired_agent_json: serde_json::json!({
            "role": "app_message_handler",
            "runtime": "hermes"
        }),
        capability_policy_json: serde_json::json!({
            "allowed_actions": ["message.summarize_plain"]
        }),
        status: "message_agent_ready".to_string(),
        created_at_ms: 0,
        updated_at_ms: 0,
        revoked_at_ms: None,
    };
    let mut second = first.clone();
    second.binding_id = "app-message-agent:did:human:alice:app_2".to_string();
    second.app_instance_id = "app_2".to_string();
    second.bootstrap_id = "boot_2".to_string();
    second.idempotency_key = "message-agent-bootstrap:did:human:alice:app_2".to_string();
    second.runtime_agent_did = "did:agent:runtime-hermes-2".to_string();
    second.runtime_profile_id = "profile_hermes_app_message_2".to_string();
    let mut other_user = first.clone();
    other_user.binding_id = "app-message-agent:did:human:bob:app_1".to_string();
    other_user.user_did = "did:human:bob".to_string();
    other_user.inbox_auth_verification_method = "did:human:bob#daemon-key-1".to_string();
    other_user.runtime_agent_did = "did:agent:runtime-hermes-bob".to_string();
    other_user.runtime_profile_id = "profile_hermes_bob".to_string();

    state.upsert_app_message_agent_binding(&first).unwrap();
    state.upsert_app_message_agent_binding(&second).unwrap();
    state.upsert_app_message_agent_binding(&other_user).unwrap();

    let revoked = state
        .revoke_other_active_app_message_agent_bindings(
            "did:human:alice",
            "app_message_handler",
            &second.binding_id,
        )
        .unwrap();
    assert_eq!(revoked, 1);
    assert!(state
        .load_active_app_message_agent_binding("did:human:alice", "app_1", "app_message_handler",)
        .unwrap()
        .is_none());
    assert_eq!(
        state
            .load_active_app_message_agent_binding(
                "did:human:alice",
                "app_2",
                "app_message_handler",
            )
            .unwrap()
            .unwrap()
            .binding_id,
        second.binding_id
    );
    assert!(state
        .load_active_app_message_agent_binding("did:human:bob", "app_1", "app_message_handler",)
        .unwrap()
        .is_some());
    first.revoked_at_ms = state
        .load_app_message_agent_binding(&first.binding_id)
        .unwrap()
        .unwrap()
        .revoked_at_ms;
    assert!(first.revoked_at_ms.is_some());
}

#[test]
fn delegated_inbox_sync_state_roundtrips_and_deduplicates_messages() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();

    let cursor = InboxCursorRecord {
        owner_did: "did:human:alice".to_string(),
        inbox_scope: "default_plain".to_string(),
        cursor: Some("cursor_10".to_string()),
        updated_at_ms: 0,
    };
    state.upsert_inbox_cursor(&cursor).unwrap();
    let loaded_cursor = state
        .load_inbox_cursor("did:human:alice", "default_plain")
        .unwrap()
        .unwrap();
    assert_eq!(loaded_cursor.cursor.as_deref(), Some("cursor_10"));

    let processed = ProcessedMessageRecord {
        owner_did: "did:human:alice".to_string(),
        message_id: "msg_1".to_string(),
        schema: "awiki.user_message.default_plain.v1".to_string(),
        processed_at_ms: 0,
        status: "dispatched".to_string(),
    };
    assert!(state.try_insert_processed_message(&processed).unwrap());
    assert!(!state.try_insert_processed_message(&processed).unwrap());
    state
        .mark_processed_message_status("did:human:alice", "msg_1", "done")
        .unwrap();
    let loaded_processed = state
        .load_processed_message("did:human:alice", "msg_1")
        .unwrap()
        .unwrap();
    assert_eq!(loaded_processed.status, "done");

    let event = MessageEventRecord {
        event_id: "evt_msg_1".to_string(),
        owner_did: "did:human:alice".to_string(),
        conversation_id: Some("direct:did:human:bob".to_string()),
        message_id: "msg_1".to_string(),
        message_kind: "text".to_string(),
        sender_did: "did:human:bob".to_string(),
        received_at: Some("2026-06-09T00:00:00Z".to_string()),
        plain_text_ref_or_excerpt: Some("hello".to_string()),
        content_hash: "hash_1".to_string(),
        schema: "awiki.user_message.default_plain.v1".to_string(),
        processing_status: "agent_dispatched".to_string(),
        retention_class: "short_excerpt".to_string(),
        created_at_ms: 0,
        updated_at_ms: 0,
    };
    state.upsert_message_event(&event).unwrap();
    let loaded_event = state.load_message_event("evt_msg_1").unwrap().unwrap();
    assert_eq!(
        loaded_event.plain_text_ref_or_excerpt.as_deref(),
        Some("hello")
    );

    let sync = MessageSyncOutboxRecord {
        idempotency_key: "message-sync:did:human:alice:msg_1".to_string(),
        owner_did: "did:human:alice".to_string(),
        app_instance_id: "app_1".to_string(),
        payload_json: serde_json::json!({
            "schema": "awiki.message.sync.v1",
            "message_id": "msg_1"
        }),
        status: "pending".to_string(),
        attempt_count: 0,
        next_attempt_at_ms: 0,
        last_error_code: None,
        last_error_summary: None,
        created_at_ms: 0,
        updated_at_ms: 0,
        sent_at_ms: None,
    };
    state.upsert_message_sync_outbox(&sync).unwrap();
    let loaded_sync = state
        .load_message_sync_outbox(&sync.idempotency_key)
        .unwrap()
        .unwrap();
    assert_eq!(loaded_sync.payload_json["message_id"], "msg_1");
    let due = state.list_due_message_sync_outbox(i64::MAX, 10).unwrap();
    assert_eq!(due.len(), 1);
    assert!(state
        .mark_message_sync_outbox_sending(&sync.idempotency_key)
        .unwrap());
    state
        .mark_message_sync_outbox_retry(&sync.idempotency_key, i64::MAX - 1, "retry", "retry later")
        .unwrap();
    assert!(state
        .list_due_message_sync_outbox(0, 10)
        .unwrap()
        .is_empty());
    state
        .recover_stale_message_sync_outbox_sending(i64::MAX, 0)
        .unwrap();
    state
        .mark_message_sync_outbox_sending(&sync.idempotency_key)
        .unwrap();
    state
        .mark_message_sync_outbox_sent(&sync.idempotency_key)
        .unwrap();
    let sent_sync = state
        .load_message_sync_outbox(&sync.idempotency_key)
        .unwrap()
        .unwrap();
    assert_eq!(sent_sync.status, "sent");
    state
        .upsert_message_sync_outbox(&MessageSyncOutboxRecord {
            status: "pending".to_string(),
            payload_json: serde_json::json!({
                "schema": "awiki.message.sync.v1",
                "message_id": "msg_changed"
            }),
            ..sync
        })
        .unwrap();
    let still_sent = state
        .load_message_sync_outbox("message-sync:did:human:alice:msg_1")
        .unwrap()
        .unwrap();
    assert_eq!(still_sent.status, "sent");
    assert_eq!(still_sent.payload_json["message_id"], "msg_1");
}

#[test]
fn control_command_state_roundtrips_and_deduplicates() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();

    let first = state
        .try_begin_control_command(
            "did:agent:daemon",
            "controller-scope:v1:test-alice",
            "cmd_upgrade_1",
            "daemon.upgrade",
            "msg_upgrade_1",
            Some("latest"),
        )
        .unwrap();
    assert!(first.is_none());

    let duplicate = state
        .try_begin_control_command(
            "did:agent:daemon",
            "controller-scope:v1:test-alice",
            "cmd_upgrade_1",
            "daemon.upgrade",
            "msg_upgrade_1",
            Some("latest"),
        )
        .unwrap()
        .unwrap();
    assert_eq!(duplicate.status, "in_progress");
    assert_eq!(duplicate.target_version.as_deref(), Some("latest"));

    state
        .mark_control_command_state(
            "did:agent:daemon",
            "controller-scope:v1:test-alice",
            "cmd_upgrade_1",
            "restart_scheduled",
            serde_json::json!({
                "command": "daemon.upgrade",
                "status": "ready",
                "version": "0.2.0",
                "restarted": true,
            }),
            None,
        )
        .unwrap();

    let stored = state
        .load_control_command_state(
            "did:agent:daemon",
            "controller-scope:v1:test-alice",
            "cmd_upgrade_1",
        )
        .unwrap()
        .unwrap();
    assert_eq!(stored.status, "restart_scheduled");
    assert_eq!(stored.result_json["version"], "0.2.0");
}

#[test]
fn runtime_task_for_run_roundtrips_requester_and_trigger_fields() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();

    let task = RuntimeTask {
        task_id: "task_state_roundtrip".to_string(),
        agent_did: "did:agent:hermes".to_string(),
        controller_user_id: "user-alice".to_string(),
        controller_full_handle: "alice.anpclaw.com".to_string(),
        controller_scope_key: "controller-scope:v1:test-alice".to_string(),
        controller_did: "did:human:alice".to_string(),
        sender_did: "did:human:bob".to_string(),
        requester_did: "did:human:bob".to_string(),
        requester_full_handle: Some("bob.example.com".to_string()),
        trigger_kind: RuntimeTaskTriggerKind::ExternalDirect,
        reply_recipient_did: "did:human:bob".to_string(),
        conversation_id: Some("direct:did:human:bob".to_string()),
        text: serde_json::json!({
            "schema": "awiki.runtime.user_message_task.v1",
            "source_message_id": "msg_state_roundtrip",
            "content_text": "hello"
        })
        .to_string(),
    };
    state.insert_runtime_task(&task).unwrap();
    state
        .insert_runtime_run(&RuntimeRun {
            run_id: "run_state_roundtrip".to_string(),
            task_id: task.task_id.clone(),
            agent_did: task.agent_did.clone(),
            runtime_profile_id: "profile_hermes".to_string(),
            runtime_plugin_id: "hermes".to_string(),
            workspace_id: None,
            status: RuntimeRunStatus::Running,
        })
        .unwrap();

    let loaded = state
        .load_runtime_task_for_run("run_state_roundtrip")
        .unwrap();
    assert_eq!(loaded.trigger_kind, RuntimeTaskTriggerKind::ExternalDirect);
    assert_eq!(loaded.requester_did, "did:human:bob");
    assert_eq!(
        loaded.requester_full_handle.as_deref(),
        Some("bob.example.com")
    );
    assert_eq!(loaded.reply_recipient_did, "did:human:bob");
    assert_eq!(loaded.text, task.text);
}

fn failed_runtime_run(run_id: &str, task_id: &str) -> RuntimeRun {
    RuntimeRun {
        run_id: run_id.to_string(),
        task_id: task_id.to_string(),
        agent_did: "did:agent:hermes".to_string(),
        runtime_profile_id: "profile_hermes".to_string(),
        runtime_plugin_id: "hermes".to_string(),
        workspace_id: None,
        status: RuntimeRunStatus::Failed,
    }
}

#[test]
fn runtime_retry_queue_records_due_time_and_filters_future_retries() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();

    let now = current_time_millis().unwrap();
    let due_run = failed_runtime_run("run_retry_due", "task_retry_due");
    let future_run = failed_runtime_run("run_retry_future", "task_retry_future");
    state.insert_runtime_run(&due_run).unwrap();
    state.insert_runtime_run(&future_run).unwrap();

    let due = state
        .insert_runtime_retry_request_due_at(&due_run, "cmd_retry_due", now)
        .unwrap();
    let future = state
        .insert_runtime_retry_request_due_at(&future_run, "cmd_retry_future", now + 60_000)
        .unwrap();

    assert_eq!(due.next_attempt_at_ms, now);
    assert_eq!(future.next_attempt_at_ms, now + 60_000);
    assert_eq!(
        state
            .load_runtime_retry_request(&future.retry_id)
            .unwrap()
            .next_attempt_at_ms,
        now + 60_000
    );

    let queued = state.list_queued_runtime_retries_due(now, 10).unwrap();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].retry_id, due.retry_id);

    let queued = state
        .list_queued_runtime_retries_due(now + 60_000, 10)
        .unwrap();
    assert_eq!(
        queued
            .iter()
            .map(|retry| retry.retry_id.as_str())
            .collect::<Vec<_>>(),
        vec![due.retry_id.as_str(), future.retry_id.as_str()]
    );
}

#[test]
fn runtime_retry_queue_lists_due_retries_by_due_time_then_fifo() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();

    let now = current_time_millis().unwrap();
    let late_run = failed_runtime_run("run_retry_late", "task_retry_late");
    let first_run = failed_runtime_run("run_retry_first", "task_retry_first");
    let second_run = failed_runtime_run("run_retry_second", "task_retry_second");
    state.insert_runtime_run(&late_run).unwrap();
    state.insert_runtime_run(&first_run).unwrap();
    state.insert_runtime_run(&second_run).unwrap();

    let late = state
        .insert_runtime_retry_request_due_at(&late_run, "cmd_retry_late", now + 50)
        .unwrap();
    let first = state
        .insert_runtime_retry_request_due_at(&first_run, "cmd_retry_first", now)
        .unwrap();
    let second = state
        .insert_runtime_retry_request_due_at(&second_run, "cmd_retry_second", now)
        .unwrap();

    let queued = state.list_queued_runtime_retries_due(now + 50, 10).unwrap();
    assert_eq!(
        queued
            .iter()
            .map(|retry| retry.retry_id.as_str())
            .collect::<Vec<_>>(),
        vec![
            first.retry_id.as_str(),
            second.retry_id.as_str(),
            late.retry_id.as_str()
        ]
    );

    assert_eq!(
        state
            .list_queued_runtime_retries_due(now + 50, 2)
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn runtime_retry_manual_request_is_immediately_due() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();

    let before = current_time_millis().unwrap();
    let run = failed_runtime_run("run_retry_manual", "task_retry_manual");
    state.insert_runtime_run(&run).unwrap();
    let retry = state
        .insert_runtime_retry_request(&run, "cmd_retry_manual")
        .unwrap();
    let after = current_time_millis().unwrap();

    assert!(retry.next_attempt_at_ms >= before);
    assert!(retry.next_attempt_at_ms <= after);
    let due = state.list_queued_runtime_retries_due(after, 10).unwrap();
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].retry_id, retry.retry_id);
}

#[test]
fn runtime_final_outbox_roundtrips_retry_and_sent_state() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();

    let now = current_time_millis().unwrap();
    let record = RuntimeFinalOutboxRecord {
        idempotency_key: "runtime-final:did:agent:hermes:run_1:controller-scope:v1:test-alice"
            .to_string(),
        run_id: "run_1".to_string(),
        agent_did: "did:agent:hermes".to_string(),
        runtime_profile_id: "profile_hermes".to_string(),
        controller_scope_key: "controller-scope:v1:test-alice".to_string(),
        controller_did: "did:human:alice".to_string(),
        recipient_did: "did:human:alice".to_string(),
        conversation_id: Some("direct:did:human:alice".to_string()),
        final_text: "final text".to_string(),
        security: "default_plain".to_string(),
        status: "pending".to_string(),
        attempt_count: 0,
        next_attempt_at_ms: now,
        last_error_code: None,
        last_error_summary: None,
        message_id: None,
        created_at_ms: now,
        updated_at_ms: now,
        sent_at_ms: None,
    };

    state.upsert_runtime_final_outbox_pending(&record).unwrap();
    let due = state.list_due_runtime_final_outbox(now, 10).unwrap();
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].final_text, "final text");
    assert!(state
        .mark_runtime_final_outbox_sending(&record.idempotency_key)
        .unwrap());
    state
        .mark_runtime_final_outbox_retry(
            &record.idempotency_key,
            now + 10_000,
            "final_delivery_retry",
            "temporary unavailable",
        )
        .unwrap();
    let stored = state
        .load_runtime_final_outbox_by_run("run_1")
        .unwrap()
        .unwrap();
    assert_eq!(stored.status, "pending");
    assert_eq!(stored.attempt_count, 1);
    assert_eq!(
        stored.last_error_code.as_deref(),
        Some("final_delivery_retry")
    );
    assert!(state
        .list_due_runtime_final_outbox(now + 9_999, 10)
        .unwrap()
        .is_empty());

    assert!(state
        .mark_runtime_final_outbox_sending(&record.idempotency_key)
        .unwrap());
    let recovered = state
        .recover_stale_runtime_final_outbox_sending(now + 60_000, now)
        .unwrap();
    assert_eq!(recovered, 1);
    let due = state.list_due_runtime_final_outbox(now, 10).unwrap();
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].status, "pending");
    assert_eq!(due[0].attempt_count, 2);

    assert!(state
        .mark_runtime_final_outbox_sending(&record.idempotency_key)
        .unwrap());
    state
        .mark_runtime_final_outbox_sent(&record.idempotency_key, Some("msg_final_1"))
        .unwrap();
    let stored = state
        .load_runtime_final_outbox_by_run("run_1")
        .unwrap()
        .unwrap();
    assert_eq!(stored.status, "sent");
    assert_eq!(stored.attempt_count, 3);
    assert_eq!(stored.message_id.as_deref(), Some("msg_final_1"));
    assert!(stored.sent_at_ms.is_some());
    assert!(state
        .list_due_runtime_final_outbox(now + 60_000, 10)
        .unwrap()
        .is_empty());
}

#[test]
fn agent_definition_v4_roundtrips_daemon_and_runtime_agents() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();

    let definition = AgentDefinition {
        agent_did: "did:agent:daemon".to_string(),
        handle: "alice-daemon".to_string(),
        agent_kind: AgentKind::Daemon,
        controller_user_id: "user-alice".to_string(),
        controller_full_handle: "alice.anpclaw.com".to_string(),
        controller_scope_key: "controller-scope:v1:test-alice".to_string(),
        controller_did: "did:human:alice".to_string(),
        runtime_plugin_id: None,
        runtime_profile_id: None,
        workspace_id: None,
        policy_id: "default".to_string(),
        local_agent_db_path: "agents/daemon/agent.db".to_string(),
        message_db_path: "agents/daemon/messages.db".to_string(),
        status: "active".to_string(),
    };
    state.upsert_agent_definition(&definition).unwrap();

    assert_eq!(
        state.load_agent_definition("did:agent:daemon").unwrap(),
        definition
    );
    assert_eq!(state.list_agent_definitions().unwrap().len(), 1);
    assert_eq!(state.list_runtime_agent_definitions().unwrap().len(), 0);
}

#[test]
fn cli_runtime_profile_roundtrips_with_controller_only_default_policy() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();

    let mut profile =
        CliRuntimeProfileRecord::for_driver("profile_generic_cli_alice", "codex-cli").unwrap();
    profile.binary_path = Some(PathBuf::from("/usr/local/bin/codex"));
    profile.config_home = Some(PathBuf::from("/tmp/codex-config"));
    profile.auth_mode = Some("user-local".to_string());
    profile.default_model = Some("gpt-5-codex".to_string());
    profile.driver_config_json = serde_json::json!({
        "api_key": "driver-secret-value"
    });
    state.upsert_cli_runtime_profile(&profile).unwrap();

    let loaded = state
        .load_cli_runtime_profile("profile_generic_cli_alice")
        .unwrap();
    assert_eq!(loaded.runtime_profile_id, "profile_generic_cli_alice");
    assert_eq!(loaded.driver_id, "codex");
    assert_eq!(
        loaded.recipient_policy_json,
        serde_json::json!({ "mode": "controller-only" })
    );
    assert_eq!(loaded.default_workspace_mode, WorkspaceMode::SharedRoot);
    assert_eq!(
        state.list_cli_runtime_profiles().unwrap(),
        vec![loaded.clone()]
    );
    assert!(!format!("{loaded:?}").contains("driver-secret-value"));
}

#[test]
fn cli_runtime_profile_rejects_invalid_policy_and_driver() {
    let mut profile =
        CliRuntimeProfileRecord::for_driver("profile_generic_cli_alice", "codex").unwrap();
    profile.recipient_policy_json = serde_json::json!(["did:human:alice"]);
    assert!(profile
        .validate()
        .unwrap_err()
        .to_string()
        .contains("policy"));

    let error = CliRuntimeProfileRecord::for_driver("profile_generic_cli_alice", " ");
    assert!(error.unwrap_err().to_string().contains("driver_id"));
}

#[test]
fn cli_runtime_profile_v8_migrates_legacy_cli_plugin_types() {
    let root = tempfile::tempdir().unwrap();
    let db_path = root.path().join("daemon.db");
    let connection = Connection::open(&db_path).unwrap();
    connection
        .execute_batch(
            r#"
            CREATE TABLE schema_migrations (
                version INTEGER PRIMARY KEY,
                applied_at TEXT NOT NULL
            );
            INSERT INTO schema_migrations (version, applied_at)
            VALUES (7, 'legacy-fixture');

            CREATE TABLE runtime_profile (
                runtime_profile_id TEXT PRIMARY KEY,
                agent_did TEXT,
                runtime_plugin_id TEXT NOT NULL,
                display_name TEXT,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            INSERT INTO runtime_profile (
                runtime_profile_id,
                agent_did,
                runtime_plugin_id,
                display_name,
                status,
                created_at,
                updated_at
            ) VALUES
                ('profile_codex', 'did:agent:codex', 'runtime.cli.codex', 'Codex', 'active', '0', '0'),
                ('profile_claude', 'did:agent:claude', 'runtime.cli.claude-code', 'Claude', 'active', '0', '0'),
                ('profile_gemini', 'did:agent:gemini', 'runtime.cli.gemini-cli', 'Gemini', 'active', '0', '0'),
                ('profile_hermes', 'did:agent:hermes', 'runtime.hermes', 'Hermes', 'active', '0', '0');

            CREATE TABLE agent_definition (
                agent_did TEXT PRIMARY KEY,
                controller_did TEXT NOT NULL,
                runtime_profile_id TEXT,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                handle TEXT NOT NULL DEFAULT '',
                agent_kind TEXT NOT NULL DEFAULT 'runtime',
                runtime_plugin_id TEXT,
                workspace_id TEXT,
                policy_id TEXT NOT NULL DEFAULT 'default',
                local_agent_db_path TEXT NOT NULL DEFAULT '',
                message_db_path TEXT NOT NULL DEFAULT ''
            );
            INSERT INTO agent_definition (
                agent_did,
                controller_did,
                runtime_profile_id,
                status,
                created_at,
                updated_at,
                handle,
                agent_kind,
                runtime_plugin_id,
                policy_id,
                local_agent_db_path,
                message_db_path
            ) VALUES
                ('did:agent:codex', 'did:human:alice', 'profile_codex', 'active', '0', '0', 'codex', 'runtime', 'runtime.cli.codex', 'default', 'agents/codex/agent.db', 'agents/codex/messages.db'),
                ('did:agent:hermes', 'did:human:alice', 'profile_hermes', 'active', '0', '0', 'hermes', 'runtime', 'runtime.hermes', 'default', 'agents/hermes/agent.db', 'agents/hermes/messages.db');
            "#,
        )
        .unwrap();
    drop(connection);

    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();

    let connection = Connection::open(db_path).unwrap();
    let runtime_plugins: Vec<(String, String)> = {
        let mut statement = connection
            .prepare(
                "SELECT runtime_profile_id, runtime_plugin_id FROM runtime_profile ORDER BY runtime_profile_id",
            )
            .unwrap();
        statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    };
    assert_eq!(
        runtime_plugins,
        vec![
            ("profile_claude".to_string(), "generic-cli".to_string()),
            ("profile_codex".to_string(), "generic-cli".to_string()),
            ("profile_gemini".to_string(), "generic-cli".to_string()),
            ("profile_hermes".to_string(), "runtime.hermes".to_string()),
        ]
    );

    let cli_profiles = state.list_cli_runtime_profiles().unwrap();
    let mapped: Vec<(String, String, serde_json::Value)> = cli_profiles
        .into_iter()
        .map(|profile| {
            (
                profile.runtime_profile_id,
                profile.driver_id,
                profile.recipient_policy_json,
            )
        })
        .collect();
    assert_eq!(
        mapped,
        vec![
            (
                "profile_claude".to_string(),
                "claude-code".to_string(),
                serde_json::json!({ "mode": "controller-only" })
            ),
            (
                "profile_codex".to_string(),
                "codex".to_string(),
                serde_json::json!({ "mode": "controller-only" })
            ),
            (
                "profile_gemini".to_string(),
                "gemini".to_string(),
                serde_json::json!({ "mode": "controller-only" })
            ),
        ]
    );

    let migrated_agent = state.load_agent_definition("did:agent:codex").unwrap();
    assert_eq!(
        migrated_agent.runtime_plugin_id.as_deref(),
        Some("generic-cli")
    );
    let hermes_agent = state.load_agent_definition("did:agent:hermes").unwrap();
    assert_eq!(
        hermes_agent.runtime_plugin_id.as_deref(),
        Some("runtime.hermes")
    );
}

#[test]
fn agent_identity_record_roundtrips_without_debug_leaking_private_key() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let identity = AgentIdentityRecord {
        agent_did: "did:agent:daemon".to_string(),
        handle: "alice-daemon".to_string(),
        agent_kind: AgentKind::Daemon,
        did_document: serde_json::json!({ "id": "did:agent:daemon" }),
        endpoint_url: Some("https://example.test/anp-im/rpc".to_string()),
        key_algorithm: "JsonWebKey2020".to_string(),
        public_key: "public".to_string(),
        auth_private_key_pem: "private-secret".to_string(),
        e2ee_signing_private_key_pem: "signing-secret".to_string(),
        e2ee_agreement_private_key_pem: "agreement-secret".to_string(),
    };
    state.store_agent_identity(&identity).unwrap();

    let loaded = state.load_agent_identity("did:agent:daemon").unwrap();
    assert_eq!(loaded.agent_did, identity.agent_did);
    assert_eq!(loaded.auth_private_key_pem, "private-secret");
    let debug = format!("{loaded:?}");
    assert!(!debug.contains("private-secret"));
    assert!(!debug.contains("signing-secret"));
    assert!(!debug.contains("agreement-secret"));
}

#[test]
fn agent_auth_token_roundtrips_without_audit_log_side_effects() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();

    state
        .store_agent_auth_token("did:agent:daemon", "jwt-secret-value")
        .unwrap();

    assert_eq!(
        state
            .load_agent_auth_token("did:agent:daemon")
            .unwrap()
            .as_deref(),
        Some("jwt-secret-value")
    );
    assert_eq!(
        state.list_agent_auth_tokens().unwrap(),
        vec![(
            "did:agent:daemon".to_string(),
            "jwt-secret-value".to_string()
        )]
    );

    let audit_count: i64 = state
        .connection()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM audit_log", [], |row| row.get(0))
        .unwrap();
    assert_eq!(audit_count, 0);
}

#[test]
fn runtime_daemon_binding_and_status_query_throttle_roundtrip() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();

    state
        .upsert_runtime_daemon_binding(
            "did:agent:runtime",
            "did:agent:daemon",
            "user-alice",
            "alice.anpclaw.com",
            "controller-scope:v1:test-alice",
            "did:human:alice",
        )
        .unwrap();

    assert!(state
        .runtime_agent_belongs_to_daemon_scope(
            "did:agent:runtime",
            "did:agent:daemon",
            "controller-scope:v1:test-alice",
        )
        .unwrap());
    assert!(!state
        .runtime_agent_belongs_to_daemon_scope(
            "did:agent:runtime",
            "did:agent:other-daemon",
            "controller-scope:v1:test-alice",
        )
        .unwrap());

    assert!(state
        .should_emit_agent_status_query_snapshot("did:agent:daemon", "did:human:alice", 10_000,)
        .unwrap());
    assert!(!state
        .should_emit_agent_status_query_snapshot("did:agent:daemon", "did:human:alice", 10_000,)
        .unwrap());
    assert!(state
        .should_emit_agent_status_query_snapshot("did:agent:daemon", "did:human:alice", 0,)
        .unwrap());
}

#[test]
fn hermes_native_session_roundtrips_and_resets_active_route() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let route = HermesSessionRoute::new(
        "did:agent:hermes",
        "profile_hermes_alice",
        "controller-scope:v1:test-alice",
        "did:human:alice",
        Some("direct:did:human:alice".to_string()),
        "conversation",
    );
    let session = HermesNativeSessionRecord::active(
        &route,
        "did:human:alice",
        "awiki_alice_hermes",
        "hermes-session-1",
    )
    .unwrap();

    state.store_hermes_native_session(&session).unwrap();
    assert_eq!(
        state
            .load_active_hermes_session_by_route(&route)
            .unwrap()
            .unwrap(),
        session
    );

    let connection = state.connection().unwrap();
    let unique_index_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_hermes_native_sessions_active_route'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(unique_index_count, 1);
    drop(connection);

    assert_eq!(
        state.reset_active_hermes_session_by_route(&route).unwrap(),
        1
    );
    assert!(state
        .load_active_hermes_session_by_route(&route)
        .unwrap()
        .is_none());

    let replacement = HermesNativeSessionRecord::active(
        &route,
        "did:human:alice",
        "awiki_alice_hermes",
        "hermes-session-2",
    )
    .unwrap();
    state.store_hermes_native_session(&replacement).unwrap();
    assert_eq!(
        state
            .load_active_hermes_session_by_route(&route)
            .unwrap()
            .unwrap()
            .hermes_session_id,
        "hermes-session-2"
    );

    let reopened = DaemonState::open(&config).unwrap();
    assert_eq!(
        reopened
            .load_active_hermes_session_by_route(&route)
            .unwrap()
            .unwrap()
            .hermes_session_id,
        "hermes-session-2"
    );
}
