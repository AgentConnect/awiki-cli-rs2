use super::schema::DAEMON_SCHEMA_VERSION;
use super::*;
use crate::runtime::{
    RuntimeConversationScope, RuntimeInvocationAuthority, RuntimeTaskTriggerKind,
};
use sha2::{Digest, Sha256};

type StoredIdentitySecretColumns = (
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

#[test]
fn controller_did_cutover_primitive_is_test_only_and_crate_private() {
    let source = include_str!("runtime_profiles.rs");
    assert!(
        source.contains("#[cfg(test)]\n    pub(crate) fn update_controller_did_for_agent_family(")
    );
    assert!(!source.contains("pub fn update_controller_did_for_agent_family("));
}

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
        "agent_device_identity",
        "agent_registration_pending",
        "agent_legacy_upgrade_pending",
        "agent_identity_migration_state",
        "agent_sync_probe",
        "cli_runtime_profile",
        "cli_driver_run",
        "cli_route_sessions",
        "cli_runtime_locks",
        "cli_route_message_queue",
        "hermes_profiles",
        "hermes_native_sessions",
        "runtime_daemon_binding",
        "agent_status_query_throttle",
        "runtime_retry_queue",
        "runtime_agent_create_request",
        "runtime_final_outbox",
        "user_delegated_identity",
        "bootstrap_replay",
        "secure_bootstrap_replay",
        "app_personal_agent_binding",
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

#[test]
fn schema_v34_migrates_legacy_message_agent_binding_without_changing_opaque_ids() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let connection = Connection::open(&config.daemon_db_path).unwrap();
    connection
        .execute_batch(
            r#"
            CREATE TABLE schema_migrations (
                version INTEGER PRIMARY KEY,
                applied_at TEXT NOT NULL
            );
            INSERT INTO schema_migrations(version, applied_at) VALUES (33, 'legacy');

            CREATE TABLE app_message_agent_binding (
                binding_id TEXT PRIMARY KEY,
                user_did TEXT NOT NULL,
                inbox_auth_verification_method TEXT NOT NULL,
                app_instance_id TEXT NOT NULL,
                bootstrap_id TEXT NOT NULL,
                idempotency_key TEXT NOT NULL,
                daemon_agent_did TEXT NOT NULL,
                runtime_agent_did TEXT NOT NULL,
                runtime_profile_id TEXT NOT NULL,
                role TEXT NOT NULL,
                desired_agent_json TEXT NOT NULL,
                capability_policy_json TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                revoked_at_ms INTEGER
            );
            INSERT INTO app_message_agent_binding (
                binding_id,
                user_did,
                inbox_auth_verification_method,
                app_instance_id,
                bootstrap_id,
                idempotency_key,
                daemon_agent_did,
                runtime_agent_did,
                runtime_profile_id,
                role,
                desired_agent_json,
                capability_policy_json,
                status,
                created_at_ms,
                updated_at_ms,
                revoked_at_ms
            ) VALUES (
                'app-message-agent:did:human:alice:app_1',
                'did:human:alice',
                'did:human:alice#daemon-key-1',
                'app_1',
                'boot_legacy',
                'message-agent-bootstrap:did:human:alice:app_1',
                'did:agent:daemon',
                'did:agent:existing-runtime',
                'profile_existing_runtime',
                'app_message_handler',
                '{"role":"app_message_handler","runtime_profile":"message_agent"}',
                '{"allowed_actions":[]}',
                'message_agent_ready',
                10,
                20,
                NULL
            );
            "#,
        )
        .unwrap();
    drop(connection);

    let state = DaemonState::open(&config).unwrap();
    let summary = state.initialize().unwrap();
    assert_eq!(summary.schema_version, DAEMON_SCHEMA_VERSION);
    state.initialize().unwrap();

    let binding = state
        .load_active_app_personal_agent_binding("did:human:alice", "app_1", "app_message_handler")
        .unwrap()
        .unwrap();
    assert_eq!(
        binding.binding_id,
        "app-message-agent:did:human:alice:app_1"
    );
    assert_eq!(
        binding.idempotency_key,
        "message-agent-bootstrap:did:human:alice:app_1"
    );
    assert_eq!(binding.runtime_agent_did, "did:agent:existing-runtime");
    assert_eq!(binding.runtime_profile_id, "profile_existing_runtime");
    assert_eq!(binding.status, "personal_agent_ready");

    let connection = state.connection().unwrap();
    let canonical_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM app_personal_agent_binding",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let legacy_table_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'app_message_agent_binding'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(canonical_count, 1);
    assert_eq!(legacy_table_count, 0);
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

fn cli_route_queue_reference(
    route: &CliRouteSessionRecord,
    source_message_id: &str,
    task_id: Option<&str>,
    run_id: Option<&str>,
    next_attempt_at_ms: i64,
) -> CreateCliRouteMessageQueueReference {
    CreateCliRouteMessageQueueReference {
        agent_did: route.agent_did.clone(),
        runtime_profile_id: route.runtime_profile_id.clone(),
        driver_id: route.driver_id.clone(),
        controller_user_id: route.controller_user_id.clone(),
        controller_full_handle: route.controller_full_handle.clone(),
        controller_scope_key: route.controller_scope_key.clone(),
        controller_did: route.controller_did.clone(),
        conversation_id: route.conversation_id.clone(),
        source_message_id: source_message_id.to_string(),
        task_id: task_id.map(str::to_string),
        run_id: run_id.map(str::to_string),
        enqueue_reason: "profile_busy".to_string(),
        next_attempt_at_ms,
        last_error_code: Some("profile_busy".to_string()),
        last_error_summary: Some("profile busy sanitized".to_string()),
    }
}

#[test]
fn cli_route_message_queue_enqueues_minimal_reference_and_is_idempotent() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();

    let route = state
        .get_or_create_cli_route_session(cli_route_create(
            root.path().to_path_buf(),
            "direct:did:human:bob",
        ))
        .unwrap();
    let now = current_time_millis().unwrap();
    let first = state
        .enqueue_cli_route_message_reference(cli_route_queue_reference(
            &route,
            "msg_queue_1",
            Some("task_msg_queue_1"),
            Some("run_task_msg_queue_1"),
            now,
        ))
        .unwrap();
    let duplicate = state
        .enqueue_cli_route_message_reference(cli_route_queue_reference(
            &route,
            "msg_queue_1",
            Some("task_msg_queue_1_changed"),
            Some("run_task_msg_queue_1_changed"),
            now + 60_000,
        ))
        .unwrap();

    assert_eq!(first.queue_id, duplicate.queue_id);
    assert_eq!(first.route_sequence, 1);
    assert_eq!(duplicate.route_sequence, 1);
    assert_eq!(first.source_message_id, "msg_queue_1");
    assert_eq!(first.task_id.as_deref(), Some("task_msg_queue_1"));
    assert_eq!(first.run_id.as_deref(), Some("run_task_msg_queue_1"));
    assert_eq!(first.route_key, route.route_key);
    assert_eq!(first.route_key_hash, route.route_key_hash);
    assert_eq!(
        state.next_queued_cli_route_message_queue_due_ms().unwrap(),
        Some(now)
    );
    assert_eq!(
        state
            .list_cli_route_message_queue_for_route(&route.runtime_profile_id, &route.route_key)
            .unwrap()
            .len(),
        1
    );
    let dump = format!("{first:?}");
    assert!(!dump.contains("secret prompt"));
    assert!(!dump.contains(root.path().to_string_lossy().as_ref()));
}

#[test]
fn cli_route_message_queue_orders_due_items_by_route_sequence() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();

    let route = state
        .get_or_create_cli_route_session(cli_route_create(
            root.path().to_path_buf(),
            "direct:did:human:bob",
        ))
        .unwrap();
    let now = current_time_millis().unwrap();
    let first = state
        .enqueue_cli_route_message_reference(cli_route_queue_reference(
            &route,
            "msg_queue_first",
            Some("task_msg_queue_first"),
            Some("run_task_msg_queue_first"),
            now,
        ))
        .unwrap();
    let second = state
        .enqueue_cli_route_message_reference(cli_route_queue_reference(
            &route,
            "msg_queue_second",
            Some("task_msg_queue_second"),
            Some("run_task_msg_queue_second"),
            now,
        ))
        .unwrap();

    assert_eq!(first.route_sequence, 1);
    assert_eq!(second.route_sequence, 2);
    let due = state.list_due_cli_route_message_queue(now, 10).unwrap();
    assert_eq!(
        due.iter()
            .map(|record| record.source_message_id.as_str())
            .collect::<Vec<_>>(),
        vec!["msg_queue_first", "msg_queue_second"]
    );
    assert_eq!(
        state.next_queued_cli_route_message_queue_due_ms().unwrap(),
        Some(now)
    );
    state
        .mark_cli_route_message_queue_running(&first.queue_id, "run_replay_first")
        .unwrap();
    let running = state
        .load_cli_route_message_queue_item(&first.queue_id)
        .unwrap()
        .unwrap();
    assert_eq!(running.status, "running");
    assert_eq!(running.attempts, 1);
    assert_eq!(running.run_id.as_deref(), Some("run_replay_first"));
    state
        .mark_cli_route_message_queue_succeeded(&first.queue_id, "run_replay_first")
        .unwrap();
    let succeeded = state
        .load_cli_route_message_queue_item(&first.queue_id)
        .unwrap()
        .unwrap();
    assert_eq!(succeeded.status, "succeeded");
    assert_eq!(
        state.next_queued_cli_route_message_queue_due_ms().unwrap(),
        Some(now)
    );
}

#[test]
fn cli_route_message_queue_fair_due_returns_route_heads_only() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();

    let bob = state
        .get_or_create_cli_route_session(cli_route_create(
            root.path().to_path_buf(),
            "direct:did:human:bob",
        ))
        .unwrap();
    let charlie = state
        .get_or_create_cli_route_session(cli_route_create(
            root.path().to_path_buf(),
            "direct:did:human:charlie",
        ))
        .unwrap();
    let now = current_time_millis().unwrap();
    let bob_first = state
        .enqueue_cli_route_message_reference(cli_route_queue_reference(
            &bob,
            "msg_bob_first",
            Some("task_msg_bob_first"),
            Some("run_task_msg_bob_first"),
            now,
        ))
        .unwrap();
    let bob_second = state
        .enqueue_cli_route_message_reference(cli_route_queue_reference(
            &bob,
            "msg_bob_second",
            Some("task_msg_bob_second"),
            Some("run_task_msg_bob_second"),
            now,
        ))
        .unwrap();
    let charlie_first = state
        .enqueue_cli_route_message_reference(cli_route_queue_reference(
            &charlie,
            "msg_charlie_first",
            Some("task_msg_charlie_first"),
            Some("run_task_msg_charlie_first"),
            now,
        ))
        .unwrap();

    let plain_due = state.list_due_cli_route_message_queue(now, 10).unwrap();
    assert_eq!(
        plain_due
            .iter()
            .map(|record| record.source_message_id.as_str())
            .collect::<Vec<_>>(),
        vec!["msg_bob_first", "msg_bob_second", "msg_charlie_first"]
    );

    let fair_due = state
        .list_due_cli_route_message_queue_fair(now, 10)
        .unwrap();
    let fair_ids = fair_due
        .iter()
        .map(|record| record.source_message_id.as_str())
        .collect::<Vec<_>>();
    assert!(fair_ids.contains(&"msg_bob_first"));
    assert!(fair_ids.contains(&"msg_charlie_first"));
    assert!(!fair_ids.contains(&"msg_bob_second"));
    assert_eq!(fair_due.len(), 2);

    assert_eq!(bob_first.route_sequence, 1);
    assert_eq!(bob_second.route_sequence, 2);
    assert_eq!(charlie_first.route_sequence, 1);

    let connection = Connection::open(&config.daemon_db_path).unwrap();
    connection
        .execute(
            r#"
UPDATE cli_route_message_queue
SET route_sequence = ?1,
    created_at_ms = ?2
WHERE queue_id = ?3
"#,
            rusqlite::params![
                bob_first.route_sequence,
                bob_first.created_at_ms,
                bob_second.queue_id
            ],
        )
        .unwrap();
    let fair_due_with_duplicate_sequence = state
        .list_due_cli_route_message_queue_fair(now, 10)
        .unwrap();
    let bob_due_count = fair_due_with_duplicate_sequence
        .iter()
        .filter(|record| record.route_key == bob.route_key)
        .count();
    assert_eq!(bob_due_count, 1);
}

#[test]
fn cli_route_message_queue_claim_retry_and_dead_letter_are_state_only() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();

    let route = state
        .get_or_create_cli_route_session(cli_route_create(
            root.path().to_path_buf(),
            "direct:did:human:bob",
        ))
        .unwrap();
    let now = current_time_millis().unwrap();
    let item = state
        .enqueue_cli_route_message_reference(cli_route_queue_reference(
            &route,
            "msg_claim_retry",
            Some("task_msg_claim_retry"),
            Some("run_task_msg_claim_retry"),
            now,
        ))
        .unwrap();

    let claimed = state
        .claim_cli_route_message_queue_item(&item.queue_id, "run_replay_claim_1")
        .unwrap()
        .unwrap();
    assert_eq!(claimed.status, "running");
    assert_eq!(claimed.run_id.as_deref(), Some("run_replay_claim_1"));
    assert_eq!(claimed.attempts, 1);
    assert!(state
        .claim_cli_route_message_queue_item(&item.queue_id, "run_replay_claim_2")
        .unwrap()
        .is_none());

    let retry = state
        .retry_or_dead_letter_cli_route_message_queue_item(
            &item.queue_id,
            2,
            now + 30_000,
            "provider_unavailable",
            "token: abc /tmp/secret-path should be redacted",
        )
        .unwrap();
    assert_eq!(retry.status, "queued");
    assert_eq!(retry.attempts, 1);
    assert_eq!(retry.next_attempt_at_ms, now + 30_000);
    let retry_summary = retry.last_error_summary.unwrap();
    assert!(retry_summary.contains("<redacted>"));
    assert!(retry_summary.contains("<path>"));
    assert!(!retry_summary.contains("abc"));
    assert!(!retry_summary.contains("/tmp/secret-path"));

    let claimed_again = state
        .claim_cli_route_message_queue_item(&item.queue_id, "run_replay_claim_2")
        .unwrap()
        .unwrap();
    assert_eq!(claimed_again.attempts, 2);
    let dead_letter = state
        .retry_or_dead_letter_cli_route_message_queue_item(
            &item.queue_id,
            2,
            now + 60_000,
            "provider_unavailable",
            "final retry failed without user payload",
        )
        .unwrap();
    assert_eq!(dead_letter.status, "dead_letter");
    assert_eq!(dead_letter.attempts, 2);
    assert_eq!(dead_letter.next_attempt_at_ms, now + 30_000);

    let route_after = state
        .load_cli_route_session(&route.route_key)
        .unwrap()
        .unwrap();
    assert_eq!(route_after.last_message_id, None);
}

#[test]
fn cli_route_message_queue_running_recovers_to_due_queued() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();

    let route = state
        .get_or_create_cli_route_session(cli_route_create(
            root.path().to_path_buf(),
            "direct:did:human:bob",
        ))
        .unwrap();
    let now = current_time_millis().unwrap();
    let item = state
        .enqueue_cli_route_message_reference(cli_route_queue_reference(
            &route,
            "msg_recover_running_queue",
            Some("task_msg_recover_running_queue"),
            Some("run_task_msg_recover_running_queue"),
            now + 60_000,
        ))
        .unwrap();
    state
        .claim_cli_route_message_queue_item(&item.queue_id, "run_replay_stale")
        .unwrap()
        .unwrap();

    assert_eq!(
        state
            .recover_stale_cli_route_message_queue_running(i64::MAX)
            .unwrap(),
        1
    );
    let recovered = state
        .load_cli_route_message_queue_item(&item.queue_id)
        .unwrap()
        .unwrap();
    assert_eq!(recovered.status, "queued");
    assert_eq!(recovered.run_id.as_deref(), Some("run_replay_stale"));
    assert_eq!(recovered.attempts, 1);
    assert!(recovered.next_attempt_at_ms <= current_time_millis().unwrap());
    assert_eq!(
        recovered.last_error_code.as_deref(),
        Some("recovered_stale_running")
    );
}

#[test]
fn cli_route_session_running_recovers_lock_to_active_or_queued() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();

    let active_route = state
        .get_or_create_cli_route_session(cli_route_create(
            root.path().to_path_buf(),
            "direct:did:human:bob",
        ))
        .unwrap();
    assert!(state
        .try_acquire_cli_route_session_lease(
            &active_route.route_key,
            "run_stale_active",
            "test",
            current_time_millis().unwrap() + 60_000,
        )
        .unwrap());

    let queued_route = state
        .get_or_create_cli_route_session(cli_route_create(
            root.path().to_path_buf(),
            "direct:did:human:carol",
        ))
        .unwrap();
    let item = state
        .enqueue_cli_route_message_reference(cli_route_queue_reference(
            &queued_route,
            "msg_recover_session_queue",
            Some("task_msg_recover_session_queue"),
            Some("run_task_msg_recover_session_queue"),
            current_time_millis().unwrap(),
        ))
        .unwrap();
    assert!(state
        .try_acquire_cli_route_session_lease(
            &queued_route.route_key,
            "run_stale_queued",
            "test",
            current_time_millis().unwrap() + 60_000,
        )
        .unwrap());

    assert_eq!(
        state
            .recover_stale_cli_route_sessions_running(i64::MAX)
            .unwrap(),
        2
    );
    let active_recovered = state
        .load_cli_route_session(&active_route.route_key)
        .unwrap()
        .unwrap();
    assert_eq!(active_recovered.status, "active");
    assert_eq!(active_recovered.lock_run_id, None);
    assert_eq!(
        active_recovered.last_error_code.as_deref(),
        Some("recovered_stale_route_session")
    );

    let queued_recovered = state
        .load_cli_route_session(&queued_route.route_key)
        .unwrap()
        .unwrap();
    assert_eq!(queued_recovered.status, "queued");
    assert_eq!(queued_recovered.lock_run_id, None);
    assert_eq!(
        state
            .load_cli_route_message_queue_item(&item.queue_id)
            .unwrap()
            .unwrap()
            .status,
        "queued"
    );
}

#[test]
fn cli_route_message_queue_cancel_helpers_cancel_pending_items() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();

    let route = state
        .get_or_create_cli_route_session(cli_route_create(
            root.path().to_path_buf(),
            "direct:did:human:bob",
        ))
        .unwrap();
    let now = current_time_millis().unwrap();
    let first = state
        .enqueue_cli_route_message_reference(cli_route_queue_reference(
            &route,
            "msg_queue_cancel_route",
            Some("task_msg_queue_cancel_route"),
            Some("run_task_msg_queue_cancel_route"),
            now,
        ))
        .unwrap();
    assert_eq!(
        state
            .cancel_cli_route_message_queue_for_route(
                &route.runtime_profile_id,
                &route.route_key,
                "route_reset",
            )
            .unwrap(),
        1
    );
    assert_eq!(
        state
            .load_cli_route_message_queue_item(&first.queue_id)
            .unwrap()
            .unwrap()
            .status,
        "cancelled"
    );

    let second = state
        .enqueue_cli_route_message_reference(cli_route_queue_reference(
            &route,
            "msg_queue_cancel_scope",
            Some("task_msg_queue_cancel_scope"),
            Some("run_task_msg_queue_cancel_scope"),
            now,
        ))
        .unwrap();
    assert_eq!(
        state
            .cancel_cli_route_message_queue_for_runtime_controller_scope(
                &route.agent_did,
                &route.runtime_profile_id,
                &route.controller_scope_key,
                "runtime_scope_reset",
            )
            .unwrap(),
        1
    );
    assert_eq!(
        state
            .load_cli_route_message_queue_item(&second.queue_id)
            .unwrap()
            .unwrap()
            .last_error_code
            .as_deref(),
        Some("runtime_scope_reset")
    );
}

#[test]
fn cli_route_message_queue_route_session_reset_cancels_queued_items() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();

    let route = state
        .get_or_create_cli_route_session(cli_route_create(
            root.path().to_path_buf(),
            "direct:did:human:bob",
        ))
        .unwrap();
    state
        .mark_cli_route_session_deferred(
            &route.route_key,
            Some("run_task_msg_deferred"),
            "profile_busy",
            "profile busy sanitized",
        )
        .unwrap();
    let item = state
        .enqueue_cli_route_message_reference(cli_route_queue_reference(
            &route,
            "msg_deferred",
            Some("task_msg_deferred"),
            Some("run_task_msg_deferred"),
            current_time_millis().unwrap(),
        ))
        .unwrap();

    assert_eq!(
        state
            .reset_cli_route_session_by_route(&route.route_key)
            .unwrap(),
        1
    );
    let reset = state
        .load_cli_route_session(&route.route_key)
        .unwrap()
        .unwrap();
    assert_eq!(reset.status, "reset");
    assert_eq!(reset.last_message_id, None);
    let cancelled = state
        .load_cli_route_message_queue_item(&item.queue_id)
        .unwrap()
        .unwrap();
    assert_eq!(cancelled.status, "cancelled");
    assert_eq!(cancelled.last_error_code.as_deref(), Some("route_reset"));
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
fn cli_route_session_adopts_native_pointer_from_legacy_alias() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();

    let legacy = state
        .get_or_create_cli_route_session(cli_route_create(
            root.path().to_path_buf(),
            "direct:peer-scope:v1:test-alice",
        ))
        .unwrap();
    state
        .update_cli_route_session_native_id(
            &legacy.route_key,
            Some("019f0333-4385-7ec3-bf0e-61b0b7d15f48"),
            Some("json_event"),
            Some(&legacy.route_key),
        )
        .unwrap();

    let canonical = state
        .get_or_create_cli_route_session(cli_route_create(
            root.path().to_path_buf(),
            "direct:controller:controller-scope:v1:test-alice",
        ))
        .unwrap();
    assert_ne!(canonical.route_key, legacy.route_key);
    assert_eq!(canonical.native_session_id, None);

    assert!(state
        .adopt_cli_route_session_native_pointer_from_aliases(
            &canonical.route_key,
            &[legacy.route_key.clone()]
        )
        .unwrap());
    let adopted = state
        .load_cli_route_session(&canonical.route_key)
        .unwrap()
        .unwrap();
    assert_eq!(
        adopted.native_session_id.as_deref(),
        Some("019f0333-4385-7ec3-bf0e-61b0b7d15f48")
    );
    assert_eq!(adopted.native_session_source.as_deref(), Some("json_event"));
    assert!(!state
        .adopt_cli_route_session_native_pointer_from_aliases(
            &canonical.route_key,
            &[legacy.route_key.clone()]
        )
        .unwrap());
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
        private_key_ref_json: None,
        allowed_scopes_json: serde_json::json!([
            "message.inbox.read.plain",
            "message.history.read.plain",
            "message.summarize_plain"
        ]),
        status: "paired_key_received".to_string(),
        expires_at: Some("2026-09-09T00:00:00Z".to_string()),
        bootstrap_id: "boot_1".to_string(),
        idempotency_key: "personal-agent-bootstrap:did:wba:example.com:user:alice:e1_user:app_1"
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

fn secure_bootstrap_replay_fixture() -> SecureBootstrapReplayRecord {
    SecureBootstrapReplayRecord {
        operation_id: "personal-agent-bootstrap:did:human:alice:app_1".to_string(),
        nonce: "AQEBAQEBAQEBAQEB".to_string(),
        envelope_hash: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            .to_string(),
        recipient_daemon_did: "did:agent:daemon".to_string(),
        recipient_key_id: "did:agent:daemon#key-3".to_string(),
        sender_human_did: "did:human:alice".to_string(),
        bootstrap_id: "boot_1".to_string(),
        idempotency_key: "personal-agent-bootstrap:did:human:alice:app_1".to_string(),
        payload_sha256: Some(
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
        ),
        expires_at: "2026-09-09T00:00:00Z".to_string(),
        status: "paired_key_received".to_string(),
        created_at_ms: 0,
        updated_at_ms: 0,
    }
}

#[test]
fn secure_bootstrap_replay_roundtrips_and_rejects_conflicts() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let replay = secure_bootstrap_replay_fixture();

    assert_eq!(
        state.store_secure_bootstrap_replay(&replay).unwrap(),
        BootstrapStoreOutcome::Inserted
    );
    assert_eq!(
        state.store_secure_bootstrap_replay(&replay).unwrap(),
        BootstrapStoreOutcome::Duplicate
    );

    let loaded = state
        .load_secure_bootstrap_replay(&replay.operation_id)
        .unwrap()
        .unwrap();
    assert_eq!(loaded.nonce, replay.nonce);
    assert_eq!(loaded.payload_sha256, replay.payload_sha256);

    let mut operation_conflict = replay.clone();
    operation_conflict.nonce = "AgICAgICAgICAgIC".to_string();
    operation_conflict.envelope_hash =
        "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".to_string();
    let error = state
        .store_secure_bootstrap_replay(&operation_conflict)
        .unwrap_err()
        .to_string();
    assert!(error.contains("secure daemon bootstrap replay conflict"));

    let mut nonce_conflict = replay.clone();
    nonce_conflict.operation_id = "personal-agent-bootstrap:did:human:alice:app_2".to_string();
    nonce_conflict.idempotency_key = nonce_conflict.operation_id.clone();
    nonce_conflict.bootstrap_id = "boot_2".to_string();
    nonce_conflict.envelope_hash =
        "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd".to_string();
    let error = state
        .store_secure_bootstrap_replay(&nonce_conflict)
        .unwrap_err()
        .to_string();
    assert!(error.contains("secure daemon bootstrap replay conflict"));
}

#[test]
fn secure_bootstrap_replay_exists_for_scope_is_exact_and_rejects_empty_scope() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let replay = secure_bootstrap_replay_fixture();

    assert!(!state
        .secure_bootstrap_replay_exists_for_scope(
            &replay.sender_human_did,
            &replay.recipient_daemon_did,
        )
        .unwrap());
    state.store_secure_bootstrap_replay(&replay).unwrap();
    assert!(state
        .secure_bootstrap_replay_exists_for_scope(
            &replay.sender_human_did,
            &replay.recipient_daemon_did,
        )
        .unwrap());
    assert!(!state
        .secure_bootstrap_replay_exists_for_scope(
            "did:human:different",
            &replay.recipient_daemon_did,
        )
        .unwrap());
    assert!(!state
        .secure_bootstrap_replay_exists_for_scope(&replay.sender_human_did, "did:agent:different",)
        .unwrap());
    assert!(state
        .secure_bootstrap_replay_exists_for_scope("", &replay.recipient_daemon_did)
        .is_err());
    assert!(state
        .secure_bootstrap_replay_exists_for_scope(&replay.sender_human_did, " ")
        .is_err());
}

#[test]
fn user_delegated_identity_roundtrips_and_replays_idempotently() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open_with_root_key_bytes(&config, [21_u8; 32]);
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

    let connection = Connection::open(&config.daemon_db_path).unwrap();
    let (stored_private_key_material, private_key_ref_json): (String, Option<String>) = connection
        .query_row(
            r#"
SELECT private_key_material, private_key_ref_json
FROM user_delegated_identity
WHERE verification_method = ?1
"#,
            [&identity.verification_method],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(stored_private_key_material, "<awiki-secret-vault-ref>");
    assert!(private_key_ref_json.is_some());
    let raw_db = std::fs::read(&config.daemon_db_path).unwrap();
    assert!(!String::from_utf8_lossy(&raw_db).contains("z-private-secret"));

    let replay_loaded = state.load_bootstrap_replay("boot_1").unwrap().unwrap();
    assert_eq!(replay_loaded.payload_hash, "payload-hash-1");

    let reopened = DaemonState::open_with_root_key_bytes(&config, [21_u8; 32]);
    let recovered = reopened
        .load_user_delegated_identity(&identity.verification_method)
        .unwrap()
        .unwrap();
    assert_eq!(recovered.status, "paired_key_received");
}

#[test]
fn bootstrap_replay_accepts_explicit_legacy_hash_alias_without_rewriting_stored_hash() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open_with_root_key_bytes(&config, [23_u8; 32]);
    state.initialize().unwrap();
    let (identity, mut legacy_replay) = delegated_identity_fixture();
    legacy_replay.payload_hash = "legacy-message-agent-payload-hash".to_string();
    state
        .store_bootstrap_state(&identity, &legacy_replay)
        .unwrap();

    let mut canonical_replay = legacy_replay.clone();
    canonical_replay.payload_hash = "canonical-personal-agent-payload-hash".to_string();
    let outcome = state
        .store_bootstrap_state_with_legacy_payload_hash(
            &identity,
            &canonical_replay,
            &legacy_replay.payload_hash,
        )
        .unwrap();

    assert_eq!(outcome, BootstrapStoreOutcome::Duplicate);
    assert_eq!(
        state
            .load_bootstrap_replay(&legacy_replay.bootstrap_id)
            .unwrap()
            .unwrap()
            .payload_hash,
        legacy_replay.payload_hash
    );
}

#[test]
fn user_delegated_identity_rejects_conflicting_replay() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open_with_root_key_bytes(&config, [22_u8; 32]);
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
fn user_delegated_identity_store_requires_secret_vault_root_key() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open_without_secret_vault_for_legacy(&config);
    state.initialize().unwrap();
    let (identity, replay) = delegated_identity_fixture();

    let error = state.store_bootstrap_state(&identity, &replay).unwrap_err();

    assert!(error.to_string().contains("refusing plaintext fallback"));
}

#[test]
fn user_delegated_identity_plaintext_row_without_vault_ref_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let legacy_state = DaemonState::open_without_secret_vault_for_legacy(&config);
    legacy_state.initialize().unwrap();
    let (identity, replay) = delegated_identity_fixture();
    let connection = Connection::open(&config.daemon_db_path).unwrap();
    connection
        .execute(
            r#"
INSERT INTO bootstrap_replay (
    bootstrap_id,
    idempotency_key,
    payload_hash,
    user_did,
    verification_method,
    app_instance_id,
    daemon_agent_did,
    status,
    created_at_ms,
    updated_at_ms
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)
"#,
            rusqlite::params![
                &replay.bootstrap_id,
                &replay.idempotency_key,
                &replay.payload_hash,
                &replay.user_did,
                &replay.verification_method,
                &replay.app_instance_id,
                &replay.daemon_agent_did,
                &replay.status,
                1700000000000_i64,
            ],
        )
        .unwrap();
    connection
        .execute(
            r#"
INSERT INTO user_delegated_identity (
    user_did,
    verification_method,
    app_instance_id,
    controller_did,
    daemon_agent_did,
    public_key_multibase,
    private_key_material,
    allowed_scopes_json,
    status,
    expires_at,
    bootstrap_id,
    idempotency_key,
    created_at_ms,
    updated_at_ms
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13)
"#,
            rusqlite::params![
                &identity.user_did,
                &identity.verification_method,
                &identity.app_instance_id,
                &identity.controller_did,
                &identity.daemon_agent_did,
                &identity.public_key_multibase,
                "legacy-delegated-private-secret",
                identity.allowed_scopes_json.to_string(),
                &identity.status,
                &identity.expires_at,
                &identity.bootstrap_id,
                &identity.idempotency_key,
                1700000000000_i64,
            ],
        )
        .unwrap();

    let state = DaemonState::open_with_root_key_bytes(&config, [23_u8; 32]);
    let error = state
        .load_user_delegated_identity(&identity.verification_method)
        .unwrap_err()
        .to_string();

    assert!(error.contains("private_key_ref_json is missing a daemon secret vault ref"));
    let (stored_private_key_material, private_key_ref_json): (String, Option<String>) = connection
        .query_row(
            r#"
SELECT private_key_material, private_key_ref_json
FROM user_delegated_identity
WHERE verification_method = ?1
"#,
            [&identity.verification_method],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        stored_private_key_material,
        "legacy-delegated-private-secret"
    );
    assert!(private_key_ref_json.is_none());
}

#[test]
fn app_personal_agent_binding_roundtrips_and_restores_active_record() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let record = AppPersonalAgentBindingRecord {
        binding_id: "app-personal-agent:did:human:alice:app_1".to_string(),
        user_did: "did:human:alice".to_string(),
        inbox_auth_verification_method: "did:human:alice#daemon-key-1".to_string(),
        app_instance_id: "app_1".to_string(),
        bootstrap_id: "boot_1".to_string(),
        idempotency_key: "personal-agent-bootstrap:did:human:alice:app_1".to_string(),
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
        status: "personal_agent_ready".to_string(),
        created_at_ms: 0,
        updated_at_ms: 0,
        revoked_at_ms: None,
    };

    state.upsert_app_personal_agent_binding(&record).unwrap();
    let loaded = state
        .load_active_app_personal_agent_binding("did:human:alice", "app_1", "app_message_handler")
        .unwrap()
        .unwrap();
    assert_eq!(loaded.binding_id, record.binding_id);
    assert_eq!(loaded.runtime_agent_did, "did:agent:runtime-hermes");

    let reopened = DaemonState::open(&config).unwrap();
    let restored = reopened
        .load_app_personal_agent_binding(&record.binding_id)
        .unwrap()
        .unwrap();
    assert_eq!(restored.status, "personal_agent_ready");
}

#[test]
fn app_personal_agent_binding_disable_removes_record_from_active_queries() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let record = AppPersonalAgentBindingRecord {
        binding_id: "app-personal-agent:did:human:alice:app_1".to_string(),
        user_did: "did:human:alice".to_string(),
        inbox_auth_verification_method: "did:human:alice#daemon-key-1".to_string(),
        app_instance_id: "app_1".to_string(),
        bootstrap_id: "boot_1".to_string(),
        idempotency_key: "personal-agent-bootstrap:did:human:alice:app_1".to_string(),
        daemon_agent_did: "did:agent:daemon".to_string(),
        runtime_agent_did: "did:agent:runtime-hermes".to_string(),
        runtime_profile_id: "profile_hermes_app_message".to_string(),
        role: "app_message_handler".to_string(),
        desired_agent_json: serde_json::json!({"role": "app_message_handler"}),
        capability_policy_json: serde_json::json!({"allowed_actions": []}),
        status: "personal_agent_ready".to_string(),
        created_at_ms: 0,
        updated_at_ms: 0,
        revoked_at_ms: None,
    };

    state.upsert_app_personal_agent_binding(&record).unwrap();
    let updated = state
        .update_app_personal_agent_binding_status_by_runtime(
            "did:agent:runtime-hermes",
            "personal_agent_disabled",
            false,
        )
        .unwrap()
        .unwrap();

    assert_eq!(updated.status, "personal_agent_disabled");
    assert!(updated.revoked_at_ms.is_none());
    assert!(state
        .load_active_app_personal_agent_binding_by_runtime("did:agent:runtime-hermes")
        .unwrap()
        .is_none());
    assert_eq!(
        state
            .list_active_app_personal_agent_bindings()
            .unwrap()
            .len(),
        0
    );
}

#[test]
fn app_personal_agent_binding_revokes_superseded_records_for_same_user_role() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let mut first = AppPersonalAgentBindingRecord {
        binding_id: "app-personal-agent:did:human:alice:app_1".to_string(),
        user_did: "did:human:alice".to_string(),
        inbox_auth_verification_method: "did:human:alice#daemon-key-1".to_string(),
        app_instance_id: "app_1".to_string(),
        bootstrap_id: "boot_1".to_string(),
        idempotency_key: "personal-agent-bootstrap:did:human:alice:app_1".to_string(),
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
        status: "personal_agent_ready".to_string(),
        created_at_ms: 0,
        updated_at_ms: 0,
        revoked_at_ms: None,
    };
    let mut second = first.clone();
    second.binding_id = "app-personal-agent:did:human:alice:app_2".to_string();
    second.app_instance_id = "app_2".to_string();
    second.bootstrap_id = "boot_2".to_string();
    second.idempotency_key = "personal-agent-bootstrap:did:human:alice:app_2".to_string();
    second.runtime_agent_did = "did:agent:runtime-hermes-2".to_string();
    second.runtime_profile_id = "profile_hermes_app_message_2".to_string();
    let mut other_user = first.clone();
    other_user.binding_id = "app-personal-agent:did:human:bob:app_1".to_string();
    other_user.user_did = "did:human:bob".to_string();
    other_user.inbox_auth_verification_method = "did:human:bob#daemon-key-1".to_string();
    other_user.runtime_agent_did = "did:agent:runtime-hermes-bob".to_string();
    other_user.runtime_profile_id = "profile_hermes_bob".to_string();

    state.upsert_app_personal_agent_binding(&first).unwrap();
    state.upsert_app_personal_agent_binding(&second).unwrap();
    state
        .upsert_app_personal_agent_binding(&other_user)
        .unwrap();

    let revoked = state
        .revoke_other_active_app_personal_agent_bindings(
            "did:human:alice",
            "app_message_handler",
            &second.binding_id,
        )
        .unwrap();
    assert_eq!(revoked, 1);
    assert!(state
        .load_active_app_personal_agent_binding("did:human:alice", "app_1", "app_message_handler",)
        .unwrap()
        .is_none());
    assert_eq!(
        state
            .load_active_app_personal_agent_binding(
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
        .load_active_app_personal_agent_binding("did:human:bob", "app_1", "app_message_handler",)
        .unwrap()
        .is_some());
    first.revoked_at_ms = state
        .load_app_personal_agent_binding(&first.binding_id)
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
    assert_eq!(
        state.next_pending_message_sync_outbox_due_ms().unwrap(),
        Some(0)
    );
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
    assert_eq!(
        state.next_pending_message_sync_outbox_due_ms().unwrap(),
        Some(i64::MAX - 1)
    );
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
fn control_command_state_supports_cancelled_and_latest_lookup() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();

    state
        .try_begin_control_command(
            "did:agent:daemon",
            "controller-scope:v1:test-alice",
            "cmd_upgrade_cancelled",
            "daemon.upgrade",
            "msg_upgrade_cancelled",
            Some("latest"),
        )
        .unwrap();
    state
        .mark_control_command_state(
            "did:agent:daemon",
            "controller-scope:v1:test-alice",
            "cmd_upgrade_cancelled",
            "cancelled",
            serde_json::json!({
                "command": "daemon.upgrade",
                "status": "cancelled",
                "error_code": "upgrade_cancelled",
            }),
            None,
        )
        .unwrap();
    state
        .try_begin_control_command(
            "did:agent:daemon",
            "controller-scope:v1:test-alice",
            "cmd_upgrade_running",
            "daemon.upgrade",
            "msg_upgrade_running",
            Some("0.2.0"),
        )
        .unwrap();

    let cancelled = state
        .load_control_command_state(
            "did:agent:daemon",
            "controller-scope:v1:test-alice",
            "cmd_upgrade_cancelled",
        )
        .unwrap()
        .unwrap();
    assert_eq!(cancelled.status, "cancelled");
    assert!(cancelled.error_summary.is_none());

    let latest = state
        .load_latest_control_command_state(
            "did:agent:daemon",
            "controller-scope:v1:test-alice",
            "daemon.upgrade",
            &["in_progress", "restart_scheduled"],
        )
        .unwrap()
        .unwrap();
    assert_eq!(latest.command_id, "cmd_upgrade_running");
    assert_eq!(latest.status, "in_progress");
}

#[test]
fn daemon_upgrade_command_reconciliation_finishes_pending_state() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();

    state
        .try_begin_control_command(
            "did:agent:daemon",
            "controller-scope:v1:test-alice",
            "cmd_upgrade_pending",
            "daemon.upgrade",
            "msg_upgrade_pending",
            Some("latest"),
        )
        .unwrap();

    state
        .reconcile_daemon_upgrade_commands(
            "did:agent:daemon",
            "controller-scope:v1:test-alice",
            "0.2.0",
            Some("0.2.0"),
            false,
        )
        .unwrap();

    let stored = state
        .load_control_command_state(
            "did:agent:daemon",
            "controller-scope:v1:test-alice",
            "cmd_upgrade_pending",
        )
        .unwrap()
        .unwrap();
    assert_eq!(stored.status, "succeeded");
    assert_eq!(stored.result_json["status"], "ready");
    assert_eq!(stored.result_json["version"], "0.2.0");
    assert_eq!(stored.result_json["reconciled"], true);
    assert!(stored.error_summary.is_none());
}

#[test]
fn daemon_upgrade_command_reconciliation_keeps_recent_pending_old_version() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();

    state
        .try_begin_control_command(
            "did:agent:daemon",
            "controller-scope:v1:test-alice",
            "cmd_upgrade_stale",
            "daemon.upgrade",
            "msg_upgrade_stale",
            Some("latest"),
        )
        .unwrap();
    state
        .mark_control_command_state(
            "did:agent:daemon",
            "controller-scope:v1:test-alice",
            "cmd_upgrade_stale",
            "restart_scheduled",
            serde_json::json!({
                "command": "daemon.upgrade",
                "status": "restart_scheduled",
            }),
            None,
        )
        .unwrap();

    state
        .reconcile_daemon_upgrade_commands(
            "did:agent:daemon",
            "controller-scope:v1:test-alice",
            "0.1.31",
            Some("0.1.34"),
            true,
        )
        .unwrap();

    let stored = state
        .load_control_command_state(
            "did:agent:daemon",
            "controller-scope:v1:test-alice",
            "cmd_upgrade_stale",
        )
        .unwrap()
        .unwrap();
    assert_eq!(stored.status, "restart_scheduled");
    assert_eq!(stored.result_json["status"], "restart_scheduled");
    assert!(stored.error_summary.is_none());
}

#[test]
fn daemon_upgrade_command_reconciliation_fails_stale_old_version() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();

    state
        .try_begin_control_command(
            "did:agent:daemon",
            "controller-scope:v1:test-alice",
            "cmd_upgrade_stale",
            "daemon.upgrade",
            "msg_upgrade_stale",
            Some("latest"),
        )
        .unwrap();
    state
        .mark_control_command_state(
            "did:agent:daemon",
            "controller-scope:v1:test-alice",
            "cmd_upgrade_stale",
            "restart_scheduled",
            serde_json::json!({
                "command": "daemon.upgrade",
                "status": "restart_scheduled",
            }),
            None,
        )
        .unwrap();

    {
        let connection = state.connection().unwrap();
        connection
            .execute(
                r#"
UPDATE control_command_state
SET updated_at_ms = updated_at_ms - 180000
WHERE command_id = ?1
"#,
                rusqlite::params!["cmd_upgrade_stale"],
            )
            .unwrap();
    }

    state
        .reconcile_daemon_upgrade_commands(
            "did:agent:daemon",
            "controller-scope:v1:test-alice",
            "0.1.31",
            Some("0.1.34"),
            true,
        )
        .unwrap();

    let stored = state
        .load_control_command_state(
            "did:agent:daemon",
            "controller-scope:v1:test-alice",
            "cmd_upgrade_stale",
        )
        .unwrap()
        .unwrap();
    assert_eq!(stored.status, "failed");
    assert_eq!(stored.result_json["status"], "failed");
    assert_eq!(stored.result_json["error_code"], "upgrade_not_applied");
    assert_eq!(stored.result_json["version"], "0.1.31");
    assert!(stored.error_summary.is_some());

    let latest_pending = state
        .load_latest_control_command_state(
            "did:agent:daemon",
            "controller-scope:v1:test-alice",
            "daemon.upgrade",
            &["in_progress", "restart_scheduled"],
        )
        .unwrap();
    assert!(latest_pending.is_none());
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
        agent_handle: "alice-hermes".to_string(),
        controller_user_id: "user-alice".to_string(),
        controller_full_handle: "alice.anpclaw.com".to_string(),
        controller_scope_key: "controller-scope:v1:test-alice".to_string(),
        controller_did: "did:human:alice".to_string(),
        sender_did: "did:human:bob".to_string(),
        requester_did: "did:human:bob".to_string(),
        requester_user_id: Some("user-bob".to_string()),
        requester_full_handle: Some("bob.example.com".to_string()),
        trigger_kind: RuntimeTaskTriggerKind::ExternalDirect,
        conversation_scope: RuntimeConversationScope::direct("user-bob", "bob.example.com")
            .unwrap(),
        invocation_authority: RuntimeInvocationAuthority::Requester,
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

fn runtime_final_outbox_record(
    run_id: &str,
    idempotency_key: &str,
    final_text: &str,
    now: i64,
) -> RuntimeFinalOutboxRecord {
    RuntimeFinalOutboxRecord {
        idempotency_key: idempotency_key.to_string(),
        run_id: run_id.to_string(),
        agent_did: "did:agent:hermes".to_string(),
        runtime_profile_id: "profile_hermes".to_string(),
        controller_scope_key: "controller-scope:v1:test-alice".to_string(),
        controller_did: "did:human:alice".to_string(),
        recipient_did: "did:human:alice".to_string(),
        conversation_id: Some("direct:did:human:alice".to_string()),
        final_text: final_text.to_string(),
        final_source: "hermes_final_text".to_string(),
        final_body_hash: test_final_body_hash(final_text),
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
    }
}

#[test]
fn runtime_run_active_transitions_are_conditional() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();

    let pending = RuntimeRun {
        run_id: "run_active_pending".to_string(),
        task_id: "task_active_pending".to_string(),
        agent_did: "did:agent:hermes".to_string(),
        runtime_profile_id: "profile_hermes".to_string(),
        runtime_plugin_id: "hermes".to_string(),
        workspace_id: None,
        status: RuntimeRunStatus::Pending,
    };
    state.insert_runtime_run(&pending).unwrap();
    assert!(state
        .finish_active_runtime_run("run_active_pending")
        .unwrap());
    assert_eq!(
        state.load_runtime_run("run_active_pending").unwrap().status,
        RuntimeRunStatus::Finished
    );
    assert!(!state.fail_active_runtime_run("run_active_pending").unwrap());
    assert_eq!(
        state.load_runtime_run("run_active_pending").unwrap().status,
        RuntimeRunStatus::Finished
    );

    let running = RuntimeRun {
        run_id: "run_active_running".to_string(),
        task_id: "task_active_running".to_string(),
        status: RuntimeRunStatus::Running,
        ..pending
    };
    state.insert_runtime_run(&running).unwrap();
    assert!(state.fail_active_runtime_run("run_active_running").unwrap());
    assert_eq!(
        state.load_runtime_run("run_active_running").unwrap().status,
        RuntimeRunStatus::Failed
    );
    assert!(!state
        .finish_active_runtime_run("run_active_running")
        .unwrap());
}

#[test]
fn runtime_run_recovery_marks_only_stale_active_runs_failed() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();

    let pending = RuntimeRun {
        run_id: "run_recover_pending".to_string(),
        task_id: "task_recover_pending".to_string(),
        agent_did: "did:agent:hermes".to_string(),
        runtime_profile_id: "profile_hermes".to_string(),
        runtime_plugin_id: "hermes".to_string(),
        workspace_id: None,
        status: RuntimeRunStatus::Pending,
    };
    let running = RuntimeRun {
        run_id: "run_recover_running".to_string(),
        task_id: "task_recover_running".to_string(),
        status: RuntimeRunStatus::Running,
        ..pending.clone()
    };
    let finished = RuntimeRun {
        run_id: "run_recover_finished".to_string(),
        task_id: "task_recover_finished".to_string(),
        status: RuntimeRunStatus::Finished,
        ..pending.clone()
    };
    state.insert_runtime_run(&pending).unwrap();
    state.insert_runtime_run(&running).unwrap();
    state.insert_runtime_run(&finished).unwrap();

    assert_eq!(
        state.recover_stale_active_runtime_runs(i64::MAX).unwrap(),
        2
    );
    assert_eq!(
        state
            .load_runtime_run("run_recover_pending")
            .unwrap()
            .status,
        RuntimeRunStatus::Failed
    );
    assert_eq!(
        state
            .load_runtime_run("run_recover_running")
            .unwrap()
            .status,
        RuntimeRunStatus::Failed
    );
    assert_eq!(
        state
            .load_runtime_run("run_recover_finished")
            .unwrap()
            .status,
        RuntimeRunStatus::Finished
    );
}

#[test]
fn runtime_retry_queue_v26_migrates_legacy_table_without_due_column() {
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
            VALUES (25, 'legacy-fixture');

            CREATE TABLE runtime_retry_queue (
                retry_id TEXT PRIMARY KEY,
                original_run_id TEXT NOT NULL,
                task_id TEXT NOT NULL,
                agent_did TEXT NOT NULL,
                runtime_profile_id TEXT NOT NULL,
                runtime_plugin_id TEXT NOT NULL,
                workspace_id TEXT,
                status TEXT NOT NULL,
                requested_by_command_id TEXT NOT NULL,
                attempts INTEGER NOT NULL DEFAULT 0,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL
            );
            INSERT INTO runtime_retry_queue (
                retry_id,
                original_run_id,
                task_id,
                agent_did,
                runtime_profile_id,
                runtime_plugin_id,
                workspace_id,
                status,
                requested_by_command_id,
                attempts,
                created_at_ms,
                updated_at_ms
            ) VALUES (
                'retry_legacy',
                'run_legacy',
                'task_legacy',
                'did:agent:legacy',
                'profile_legacy',
                'runtime.hermes',
                NULL,
                'queued',
                'cmd_legacy',
                0,
                100,
                100
            );
            "#,
        )
        .unwrap();
    drop(connection);

    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();

    let migrated = state
        .list_queued_runtime_retries_due(0, 10)
        .unwrap()
        .into_iter()
        .map(|retry| retry.retry_id)
        .collect::<Vec<_>>();
    assert_eq!(migrated, vec!["retry_legacy"]);
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
    assert_eq!(state.next_queued_runtime_retry_due_ms().unwrap(), Some(now));

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
fn runtime_retry_queue_lists_retries_for_original_run_without_payload() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();

    let now = current_time_millis().unwrap();
    let first_run = failed_runtime_run("run_retry_origin", "task_retry_origin");
    let other_run = failed_runtime_run("run_retry_other", "task_retry_other");
    state.insert_runtime_run(&first_run).unwrap();
    state.insert_runtime_run(&other_run).unwrap();

    let first = state
        .insert_runtime_retry_request_due_at(&first_run, "runtime.busy.auto-deferred", now + 10_000)
        .unwrap();
    let second = state
        .insert_runtime_retry_request_due_at(&first_run, "cmd_retry_manual", now + 20_000)
        .unwrap();
    state
        .insert_runtime_retry_request_due_at(&other_run, "cmd_retry_other", now)
        .unwrap();

    let retries = state
        .list_runtime_retry_requests_for_original_run("run_retry_origin")
        .unwrap();
    assert_eq!(
        retries
            .iter()
            .map(|retry| retry.retry_id.as_str())
            .collect::<Vec<_>>(),
        vec![first.retry_id.as_str(), second.retry_id.as_str()]
    );
    let dump = format!("{retries:?}");
    assert!(!dump.contains("secret prompt"));
}

#[test]
fn runtime_retry_queue_state_transitions_are_conditional() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();

    let now = current_time_millis().unwrap();
    let run = failed_runtime_run("run_retry_transition", "task_retry_transition");
    state.insert_runtime_run(&run).unwrap();
    let retry = state
        .insert_runtime_retry_request_due_at(&run, "cmd_retry_transition", now)
        .unwrap();

    assert!(!state
        .succeed_running_runtime_retry(&retry.retry_id)
        .unwrap());
    let stored = state.load_runtime_retry_request(&retry.retry_id).unwrap();
    assert_eq!(stored.status, "queued");
    assert_eq!(stored.attempts, 0);

    assert!(state.start_queued_runtime_retry(&retry.retry_id).unwrap());
    let stored = state.load_runtime_retry_request(&retry.retry_id).unwrap();
    assert_eq!(stored.status, "running");
    assert_eq!(stored.attempts, 1);
    assert!(!state.start_queued_runtime_retry(&retry.retry_id).unwrap());

    assert!(state
        .succeed_running_runtime_retry(&retry.retry_id)
        .unwrap());
    let stored = state.load_runtime_retry_request(&retry.retry_id).unwrap();
    assert_eq!(stored.status, "succeeded");
    assert_eq!(stored.attempts, 1);
    assert!(!state.fail_running_runtime_retry(&retry.retry_id).unwrap());
    state
        .reschedule_runtime_retry_request(&retry.retry_id, now + 60_000)
        .unwrap();
    assert_eq!(
        state
            .load_runtime_retry_request(&retry.retry_id)
            .unwrap()
            .status,
        "succeeded"
    );
}

#[test]
fn runtime_retry_queue_recovery_requeues_only_running_retries() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();

    let now = current_time_millis().unwrap();
    let queued_run = failed_runtime_run("run_retry_recover_queued", "task_retry_recover_queued");
    let running_run = failed_runtime_run("run_retry_recover_running", "task_retry_recover_running");
    state.insert_runtime_run(&queued_run).unwrap();
    state.insert_runtime_run(&running_run).unwrap();
    let queued = state
        .insert_runtime_retry_request_due_at(&queued_run, "cmd_retry_recover_queued", now)
        .unwrap();
    let running = state
        .insert_runtime_retry_request_due_at(&running_run, "cmd_retry_recover_running", now)
        .unwrap();
    assert!(state.start_queued_runtime_retry(&running.retry_id).unwrap());

    assert_eq!(
        state
            .recover_stale_runtime_retries_running(i64::MAX)
            .unwrap(),
        1
    );
    assert_eq!(
        state
            .load_runtime_retry_request(&queued.retry_id)
            .unwrap()
            .status,
        "queued"
    );
    let recovered = state.load_runtime_retry_request(&running.retry_id).unwrap();
    assert_eq!(recovered.status, "queued");
    assert_eq!(recovered.attempts, 1);
}

#[test]
fn runtime_final_outbox_roundtrips_retry_and_sent_state() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();

    let now = current_time_millis().unwrap();
    let record = runtime_final_outbox_record(
        "run_1",
        "runtime-final:did:agent:hermes:run_1:controller-scope:v1:test-alice",
        "final text",
        now,
    );

    state.upsert_runtime_final_outbox_pending(&record).unwrap();
    let due = state.list_due_runtime_final_outbox(now, 10).unwrap();
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].final_text, "final text");
    assert_eq!(due[0].final_source, "hermes_final_text");
    assert_eq!(due[0].final_body_hash, test_final_body_hash("final text"));
    assert_eq!(
        state.next_pending_runtime_final_outbox_due_ms().unwrap(),
        Some(now)
    );
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
    assert_eq!(
        state.next_pending_runtime_final_outbox_due_ms().unwrap(),
        Some(now + 10_000)
    );

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
    assert!(state
        .mark_runtime_final_outbox_sent(&record.idempotency_key, Some("msg_final_1"))
        .unwrap());
    let stored = state
        .load_runtime_final_outbox_by_run("run_1")
        .unwrap()
        .unwrap();
    assert_eq!(stored.status, "sent");
    assert_eq!(stored.attempt_count, 3);
    assert_eq!(stored.message_id.as_deref(), Some("msg_final_1"));
    assert!(stored.sent_at_ms.is_some());

    let mut duplicate = record.clone();
    duplicate.final_text = "different final".to_string();
    duplicate.final_source = "stdout_fallback".to_string();
    duplicate.final_body_hash = test_final_body_hash("different final");
    state
        .upsert_runtime_final_outbox_pending(&duplicate)
        .unwrap();
    let stored = state
        .load_runtime_final_outbox_by_run("run_1")
        .unwrap()
        .unwrap();
    assert_eq!(stored.status, "sent");
    assert_eq!(stored.final_text, "final text");
    assert_eq!(stored.final_source, "hermes_final_text");
    assert_eq!(stored.final_body_hash, test_final_body_hash("final text"));

    assert!(state
        .list_due_runtime_final_outbox(now + 60_000, 10)
        .unwrap()
        .is_empty());
}

#[test]
fn runtime_final_outbox_failed_terminal_is_not_overwritten() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();

    let now = current_time_millis().unwrap();
    let record = runtime_final_outbox_record(
        "run_failed_terminal",
        "runtime-final:did:agent:hermes:run_failed_terminal:controller-scope:v1:test-alice",
        "first final text",
        now,
    );
    state.upsert_runtime_final_outbox_pending(&record).unwrap();
    assert!(state
        .mark_runtime_final_outbox_sending(&record.idempotency_key)
        .unwrap());
    assert!(state
        .mark_runtime_final_outbox_failed_terminal(
            &record.idempotency_key,
            "final_delivery_failed",
            "network unavailable",
        )
        .unwrap());

    let mut duplicate = runtime_final_outbox_record(
        "run_failed_terminal",
        &record.idempotency_key,
        "replacement final text",
        now,
    );
    duplicate.final_source = "stdout_fallback".to_string();
    state
        .upsert_runtime_final_outbox_pending(&duplicate)
        .unwrap();

    let stored = state
        .load_runtime_final_outbox_by_run("run_failed_terminal")
        .unwrap()
        .unwrap();
    assert_eq!(stored.status, "failed_terminal");
    assert_eq!(stored.final_text, "first final text");
    assert_eq!(stored.final_source, "hermes_final_text");
    assert_eq!(
        stored.last_error_code.as_deref(),
        Some("final_delivery_failed")
    );
    assert!(!state
        .mark_runtime_final_outbox_sent(&record.idempotency_key, Some("msg_late"))
        .unwrap());
    assert!(!state
        .mark_runtime_final_outbox_failed_terminal(
            &record.idempotency_key,
            "late_error",
            "late failure",
        )
        .unwrap());
}

#[test]
fn runtime_final_outbox_requires_lowercase_hash_for_new_pending_rows() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();

    let now = current_time_millis().unwrap();
    let record = RuntimeFinalOutboxRecord {
        idempotency_key: "runtime-final:did:agent:hermes:run_hash:controller-scope:v1:test-alice"
            .to_string(),
        run_id: "run_hash".to_string(),
        agent_did: "did:agent:hermes".to_string(),
        runtime_profile_id: "profile_hermes".to_string(),
        controller_scope_key: "controller-scope:v1:test-alice".to_string(),
        controller_did: "did:human:alice".to_string(),
        recipient_did: "did:human:alice".to_string(),
        conversation_id: Some("direct:did:human:alice".to_string()),
        final_text: "final text".to_string(),
        final_source: "hermes_final_text".to_string(),
        final_body_hash: test_final_body_hash("final text"),
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

    record.validate().unwrap();

    let mut legacy_empty_hash = record.clone();
    legacy_empty_hash.final_body_hash.clear();
    legacy_empty_hash.validate().unwrap();
    assert!(state
        .upsert_runtime_final_outbox_pending(&legacy_empty_hash)
        .unwrap_err()
        .to_string()
        .contains("requires final_body_hash"));

    let mut uppercase_hash = record.clone();
    uppercase_hash.final_body_hash = record.final_body_hash.to_ascii_uppercase();
    assert!(uppercase_hash
        .validate()
        .unwrap_err()
        .to_string()
        .contains("sha256:<64 lowercase hex>"));
}

fn test_final_body_hash(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    format!(
        "sha256:{}",
        digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
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

    let agent_plugins: Vec<(String, String)> = {
        let mut statement = connection
            .prepare(
                r#"
SELECT agent_did, runtime_plugin_id
FROM agent_definition
ORDER BY agent_did
"#,
            )
            .unwrap();
        statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    };
    assert_eq!(
        agent_plugins,
        vec![
            ("did:agent:codex".to_string(), "generic-cli".to_string()),
            ("did:agent:hermes".to_string(), "runtime.hermes".to_string()),
        ]
    );
}

#[test]
fn controller_scope_v19_does_not_fabricate_legacy_controller_identity() {
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
            VALUES (18, 'legacy-fixture');

            CREATE TABLE agent_definition (
                agent_did TEXT PRIMARY KEY,
                handle TEXT NOT NULL,
                agent_kind TEXT NOT NULL,
                controller_did TEXT NOT NULL,
                runtime_plugin_id TEXT,
                runtime_profile_id TEXT,
                workspace_id TEXT,
                policy_id TEXT NOT NULL DEFAULT 'default',
                local_agent_db_path TEXT NOT NULL,
                message_db_path TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            INSERT INTO agent_definition (
                agent_did,
                handle,
                agent_kind,
                controller_did,
                policy_id,
                local_agent_db_path,
                message_db_path,
                status,
                created_at,
                updated_at
            ) VALUES (
                'did:agent:daemon',
                'alice-daemon',
                'daemon',
                'did:human:alice',
                'default',
                'agents/daemon/agent.db',
                'agents/daemon/messages.db',
                'active',
                '0',
                '0'
            );

            CREATE TABLE runtime_task (
                task_id TEXT PRIMARY KEY,
                agent_did TEXT NOT NULL,
                controller_did TEXT NOT NULL,
                sender_did TEXT NOT NULL,
                conversation_id TEXT,
                task_text TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL
            );
            INSERT INTO runtime_task (
                task_id,
                agent_did,
                controller_did,
                sender_did,
                conversation_id,
                task_text,
                status,
                created_at_ms,
                updated_at_ms
            ) VALUES (
                'task_1',
                'did:agent:daemon',
                'did:human:alice',
                'did:human:alice',
                NULL,
                'hello',
                'pending',
                0,
                0
            );

            CREATE TABLE runtime_daemon_binding (
                runtime_agent_did TEXT PRIMARY KEY,
                daemon_agent_did TEXT NOT NULL,
                controller_did TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL
            );
            INSERT INTO runtime_daemon_binding (
                runtime_agent_did,
                daemon_agent_did,
                controller_did,
                status,
                created_at_ms,
                updated_at_ms
            ) VALUES (
                'did:agent:runtime',
                'did:agent:daemon',
                'did:human:alice',
                'active',
                0,
                0
            );

            CREATE TABLE hermes_native_sessions (
                id TEXT PRIMARY KEY,
                runtime_session_id TEXT NOT NULL,
                agent_did TEXT NOT NULL,
                agent_handle TEXT NOT NULL,
                runtime_profile_id TEXT NOT NULL,
                controller_did TEXT NOT NULL,
                session_actor_did TEXT NOT NULL,
                conversation_id TEXT,
                scope_kind TEXT NOT NULL,
                scope_key TEXT NOT NULL,
                route_key TEXT NOT NULL,
                hermes_profile TEXT NOT NULL,
                hermes_session_id TEXT NOT NULL,
                stored_session_id TEXT NOT NULL DEFAULT '',
                last_live_session_id TEXT,
                session_kind TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL
            );

            CREATE TABLE runtime_final_outbox (
                idempotency_key TEXT PRIMARY KEY,
                agent_did TEXT NOT NULL,
                controller_did TEXT NOT NULL,
                run_id TEXT NOT NULL,
                message_text TEXT NOT NULL,
                security TEXT NOT NULL,
                status TEXT NOT NULL,
                attempt_count INTEGER NOT NULL,
                next_attempt_at_ms INTEGER NOT NULL,
                last_error_code TEXT,
                last_error_summary TEXT,
                message_id TEXT,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                sent_at_ms INTEGER
            );

            CREATE TABLE cli_driver_run (
                run_id TEXT PRIMARY KEY,
                agent_did TEXT NOT NULL,
                runtime_profile_id TEXT NOT NULL,
                driver_id TEXT NOT NULL,
                controller_did TEXT NOT NULL,
                conversation_id TEXT,
                route_key TEXT NOT NULL,
                workspace_id TEXT,
                workspace_root TEXT,
                workspace_instance_path TEXT,
                workspace_mode TEXT,
                is_security_boundary INTEGER NOT NULL DEFAULT 0,
                command_json TEXT NOT NULL DEFAULT '{}',
                output_json TEXT NOT NULL DEFAULT '{}',
                final_output_path TEXT,
                native_session_id TEXT,
                synthetic_session_id TEXT,
                status TEXT NOT NULL,
                fallback_final_source TEXT,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL
            );

            CREATE TABLE runtime_agent_create_request (
                daemon_agent_did TEXT NOT NULL,
                controller_did TEXT NOT NULL,
                client_request_id TEXT NOT NULL,
                runtime_agent_did TEXT NOT NULL,
                command_id TEXT NOT NULL,
                outcome_json TEXT NOT NULL,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                PRIMARY KEY (daemon_agent_did, client_request_id)
            );
            INSERT INTO runtime_agent_create_request (
                daemon_agent_did,
                controller_did,
                client_request_id,
                runtime_agent_did,
                command_id,
                outcome_json,
                created_at_ms,
                updated_at_ms
            ) VALUES (
                'did:agent:daemon',
                'did:human:alice',
                'request_1',
                'did:agent:runtime',
                'command_1',
                '{}',
                0,
                0
            );
            "#,
        )
        .unwrap();
    drop(connection);

    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();

    let connection = Connection::open(db_path).unwrap();
    let controller_identity: (String, String, String) = connection
        .query_row(
            r#"
SELECT controller_user_id, controller_full_handle, controller_scope_key
FROM agent_definition
WHERE agent_did = 'did:agent:daemon'
"#,
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        controller_identity,
        ("".to_string(), "".to_string(), "".to_string())
    );
    let task_scope: (String, String, String) = connection
        .query_row(
            r#"
SELECT controller_user_id, controller_full_handle, controller_scope_key
FROM runtime_task
WHERE task_id = 'task_1'
"#,
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(task_scope, ("".to_string(), "".to_string(), "".to_string()));
    let create_request_scope: String = connection
        .query_row(
            r#"
SELECT controller_scope_key
FROM runtime_agent_create_request
WHERE daemon_agent_did = 'did:agent:daemon'
  AND client_request_id = 'request_1'
"#,
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(create_request_scope, "");
    let error = state.load_agent_definition("did:agent:daemon").unwrap_err();
    let chain = error
        .chain()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(chain.contains("controller_user_id must not be empty"));
}

#[test]
fn agent_identity_record_roundtrips_without_debug_leaking_private_key() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open_with_root_key_bytes(&config, [11_u8; 32]);
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
    assert_eq!(loaded.e2ee_signing_private_key_pem, "signing-secret");
    assert_eq!(loaded.e2ee_agreement_private_key_pem, "agreement-secret");

    let connection = Connection::open(&config.daemon_db_path).unwrap();
    let (
        auth_private_key_pem,
        e2ee_signing_private_key_pem,
        e2ee_agreement_private_key_pem,
        auth_private_key_ref_json,
        e2ee_signing_private_key_ref_json,
        e2ee_agreement_private_key_ref_json,
    ): StoredIdentitySecretColumns = connection
        .query_row(
            r#"
SELECT
    auth_private_key_pem,
    e2ee_signing_private_key_pem,
    e2ee_agreement_private_key_pem,
    auth_private_key_ref_json,
    e2ee_signing_private_key_ref_json,
    e2ee_agreement_private_key_ref_json
FROM agent_identity
WHERE agent_did = ?1
"#,
            ["did:agent:daemon"],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(auth_private_key_pem, "<awiki-secret-vault-ref>");
    assert_eq!(
        e2ee_signing_private_key_pem.as_deref(),
        Some("<awiki-secret-vault-ref>")
    );
    assert_eq!(
        e2ee_agreement_private_key_pem.as_deref(),
        Some("<awiki-secret-vault-ref>")
    );
    assert!(auth_private_key_ref_json.is_some());
    assert!(e2ee_signing_private_key_ref_json.is_some());
    assert!(e2ee_agreement_private_key_ref_json.is_some());
    let raw_db = std::fs::read(&config.daemon_db_path).unwrap();
    let raw_db_text = String::from_utf8_lossy(&raw_db);
    assert!(!raw_db_text.contains("private-secret"));
    assert!(!raw_db_text.contains("signing-secret"));
    assert!(!raw_db_text.contains("agreement-secret"));

    let debug = format!("{loaded:?}");
    assert!(!debug.contains("private-secret"));
    assert!(!debug.contains("signing-secret"));
    assert!(!debug.contains("agreement-secret"));
}

#[test]
fn agent_identity_store_requires_secret_vault_root_key() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open_without_secret_vault_for_legacy(&config);
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

    let error = state.store_agent_identity(&identity).unwrap_err();

    assert!(error.to_string().contains("refusing plaintext fallback"));
}

#[test]
fn agent_identity_plaintext_row_without_vault_ref_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let legacy_state = DaemonState::open_without_secret_vault_for_legacy(&config);
    legacy_state.initialize().unwrap();
    let connection = Connection::open(&config.daemon_db_path).unwrap();
    connection
        .execute(
            r#"
INSERT INTO agent_identity (
    agent_did,
    handle,
    agent_kind,
    did_document_json,
    endpoint_url,
    key_algorithm,
    public_key,
    auth_private_key_pem,
    e2ee_signing_private_key_pem,
    e2ee_agreement_private_key_pem,
    created_at,
    updated_at
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)
"#,
            rusqlite::params![
                "did:agent:daemon",
                "alice-daemon",
                AgentKind::Daemon.as_str(),
                serde_json::json!({ "id": "did:agent:daemon" }).to_string(),
                "https://example.test/anp-im/rpc",
                "JsonWebKey2020",
                "public",
                "legacy-private-secret",
                "legacy-signing-secret",
                "legacy-agreement-secret",
                "1700000000000",
            ],
        )
        .unwrap();

    let state = DaemonState::open_with_root_key_bytes(&config, [12_u8; 32]);
    let error = state
        .load_agent_identity("did:agent:daemon")
        .unwrap_err()
        .to_string();

    assert!(error.contains("auth_private_key_ref_json is missing a daemon secret vault ref"));
    let stored_auth_pem: String = connection
        .query_row(
            "SELECT auth_private_key_pem FROM agent_identity WHERE agent_did = ?1",
            ["did:agent:daemon"],
            |row| row.get(0),
        )
        .unwrap();
    let migrated_auth_ref: Option<String> = connection
        .query_row(
            "SELECT auth_private_key_ref_json FROM agent_identity WHERE agent_did = ?1",
            ["did:agent:daemon"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(stored_auth_pem, "legacy-private-secret");
    assert!(migrated_auth_ref.is_none());
}

#[test]
fn agent_auth_token_roundtrips_from_vault_without_plaintext_storage() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open_with_root_key_bytes(&config, [31_u8; 32]);
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
    let (stored_jwt_token, jwt_token_ref_json): (String, Option<String>) = state
        .connection()
        .unwrap()
        .query_row(
            "SELECT jwt_token, jwt_token_ref_json FROM agent_auth_state WHERE agent_did = ?1",
            ["did:agent:daemon"],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(stored_jwt_token, "<awiki-secret-vault-ref>");
    assert!(jwt_token_ref_json.is_some());
    let raw_db = std::fs::read(&config.daemon_db_path).unwrap();
    assert!(!String::from_utf8_lossy(&raw_db).contains("jwt-secret-value"));

    let audit_count: i64 = state
        .connection()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM audit_log", [], |row| row.get(0))
        .unwrap();
    assert_eq!(audit_count, 0);
}

#[test]
fn agent_auth_token_store_requires_secret_vault_root_key() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open_without_secret_vault_for_legacy(&config);
    state.initialize().unwrap();

    let error = state
        .store_agent_auth_token("did:agent:daemon", "jwt-secret-value")
        .unwrap_err()
        .to_string();

    assert!(error.contains("refusing plaintext fallback"));
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
        "alice-hermes",
        "profile_hermes_alice",
        "controller-scope:v1:test-alice",
        "controller_private",
        "controller:controller-scope:v1:test-alice",
        Some("direct:did:human:alice".to_string()),
        "conversation",
    );
    assert_eq!(
        route.route_key(),
        "hermes:alice-hermes:controller-scope:v1:test-alice:controller_private:controller:controller-scope:v1:test-alice:conversation"
    );
    assert!(!route.route_key().contains("did:human:alice"));
    let session = HermesNativeSessionRecord::active(
        &route,
        "did:human:alice",
        "awiki_alice_hermes",
        "stored-session-1",
        Some("live-session-1".to_string()),
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
        "stored-session-2",
        Some("live-session-2".to_string()),
    )
    .unwrap();
    state.store_hermes_native_session(&replacement).unwrap();
    assert_eq!(
        state
            .load_active_hermes_session_by_route(&route)
            .unwrap()
            .unwrap()
            .stored_session_id,
        "stored-session-2"
    );

    let reopened = DaemonState::open(&config).unwrap();
    assert_eq!(
        reopened
            .load_active_hermes_session_by_route(&route)
            .unwrap()
            .unwrap()
            .last_live_session_id
            .as_deref()
            .map(str::to_string),
        Some("live-session-2".to_string())
    );
}

#[test]
fn hermes_route_key_uses_stable_scope_not_requester_did() {
    let route_before_rotation = HermesSessionRoute::new(
        "did:agent:hermes:e1_old",
        "alice-hermes",
        "profile_hermes_alice",
        "controller-scope:v1:test-alice",
        "direct",
        "user:user-bob:handle:bob.example.com",
        Some("direct:did:human:bob-old".to_string()),
        "conversation",
    );
    let route_after_rotation = HermesSessionRoute::new(
        "did:agent:hermes:e1_new",
        "alice-hermes",
        "profile_hermes_alice",
        "controller-scope:v1:test-alice",
        "direct",
        "user:user-bob:handle:bob.example.com",
        Some("direct:did:human:bob-new".to_string()),
        "conversation",
    );

    assert_eq!(
        route_before_rotation.route_key(),
        route_after_rotation.route_key()
    );
    assert!(!route_before_rotation.route_key().contains("bob-old"));
    assert!(!route_after_rotation.route_key().contains("bob-new"));
}

fn device_identity_fixture(
    suffix: &str,
    kind: crate::agent::AgentKind,
) -> AgentDeviceIdentityRecord {
    let kind_segment = kind.as_str();
    let did = format!("did:wba:awiki.info:agent:{kind_segment}:{suffix}");
    AgentDeviceIdentityRecord {
        identity_id: suffix.to_owned(),
        agent_did: did.clone(),
        handle: format!("{kind_segment}-{suffix}.awiki.info"),
        display_name: format!("{kind_segment}-{suffix}"),
        agent_kind: kind,
        account_id: format!("account-{suffix}"),
        full_handle: format!("{kind_segment}-{suffix}.awiki.info"),
        binding_generation: "1".to_owned(),
        did_document: serde_json::json!({"id": did}),
        protocol_device_id: format!("device-{suffix}"),
        root_key_id: format!("{did}#key-1"),
        root_private_key_pem: format!("root-private-{suffix}"),
        device_signing_key_id: format!("{did}#device-{suffix}-sign"),
        device_signing_private_key_pem: format!("sign-private-{suffix}"),
        device_e2ee_key_id: format!("{did}#device-{suffix}-e2ee"),
        device_e2ee_private_key_pem: format!("e2ee-private-{suffix}"),
        daemon_subkey_package_json: None,
        authorization_status: "active".to_owned(),
        role: "admin".to_owned(),
        management_ready: true,
        auth_generation: 1,
        access_token: format!("device-access-{suffix}"),
        document_version: 1,
        document_hash: "sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_owned(),
        registry_version: 1,
        identity_status: "active".to_owned(),
        legacy_migration_state: "not_required".to_owned(),
        last_error_code: None,
    }
}

fn device_agent_definition(identity: &AgentDeviceIdentityRecord) -> AgentDefinition {
    let is_runtime = identity.agent_kind == crate::agent::AgentKind::Runtime;
    AgentDefinition {
        agent_did: identity.agent_did.clone(),
        handle: identity.handle.clone(),
        agent_kind: identity.agent_kind,
        controller_user_id: format!("controller-{}", identity.identity_id),
        controller_full_handle: "controller.awiki.info".to_owned(),
        controller_scope_key: format!("controller-scope:v1:{}", identity.identity_id),
        controller_did: "did:wba:awiki.info:alice".to_owned(),
        runtime_plugin_id: is_runtime.then(|| "generic-cli".to_owned()),
        runtime_profile_id: is_runtime.then(|| format!("profile-{}", identity.identity_id)),
        workspace_id: None,
        policy_id: "default".to_owned(),
        local_agent_db_path: format!("agents/{}/agent.db", identity.identity_id),
        message_db_path: format!("agents/{}/messages.db", identity.identity_id),
        status: "active".to_owned(),
    }
}

#[test]
fn agent_device_identity_validation_is_closed_before_persistence() {
    let valid = device_identity_fixture("e1", crate::agent::AgentKind::Daemon);
    valid.validate().unwrap();

    let mut invalid = valid.clone();
    invalid.authorization_status = "pending".to_owned();
    assert!(invalid
        .validate()
        .unwrap_err()
        .to_string()
        .contains("authorization_status"));

    let mut invalid = valid.clone();
    invalid.role = "admin".to_owned();
    invalid.management_ready = false;
    assert!(invalid
        .validate()
        .unwrap_err()
        .to_string()
        .contains("ready-admin"));

    let mut invalid = valid.clone();
    invalid.document_hash = "plain-hash".to_owned();
    assert!(invalid
        .validate()
        .unwrap_err()
        .to_string()
        .contains("sha256"));

    let mut invalid = valid.clone();
    invalid.identity_id = "wrong".to_owned();
    assert!(invalid
        .validate()
        .unwrap_err()
        .to_string()
        .contains("final agent DID segment"));

    let mut invalid = valid;
    invalid.identity_status = "blocked".to_owned();
    invalid.legacy_migration_state = "not_required".to_owned();
    invalid.last_error_code = None;
    assert!(invalid
        .validate()
        .unwrap_err()
        .to_string()
        .contains("blocked identity"));
}

#[test]
fn fresh_device_identity_is_vault_only_and_does_not_write_legacy_identity_rows() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open_with_root_key_bytes(&config, [81_u8; 32]);
    state.initialize().unwrap();
    let identity = device_identity_fixture("e1", crate::agent::AgentKind::Daemon);
    state.store_agent_device_identity(&identity).unwrap();

    let loaded = state
        .load_agent_device_identity(&identity.agent_did)
        .unwrap()
        .unwrap();
    assert_eq!(loaded.access_token, identity.access_token);
    assert!(!format!("{loaded:?}").contains(&identity.access_token));
    assert!(!format!("{loaded:?}").contains(&identity.root_private_key_pem));

    let connection = Connection::open(&config.daemon_db_path).unwrap();
    for table in ["agent_identity", "agent_auth_state"] {
        let count: i64 = connection
            .query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE agent_did = ?1"),
                [&identity.agent_did],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0, "fresh vNext identity leaked into {table}");
    }
    let raw_db = std::fs::read(&config.daemon_db_path).unwrap();
    let raw_db = String::from_utf8_lossy(&raw_db);
    assert!(!raw_db.contains(&identity.access_token));
    assert!(!raw_db.contains(&identity.root_private_key_pem));
    assert!(!raw_db.contains(&identity.device_signing_private_key_pem));
    assert!(!raw_db.contains(&identity.device_e2ee_private_key_pem));
}

#[test]
fn device_access_token_replacement_is_exact_atomic_and_vault_only() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open_with_root_key_bytes(&config, [82_u8; 32]);
    state.initialize().unwrap();
    let identity = device_identity_fixture("e2-token", crate::agent::AgentKind::Runtime);
    state.store_agent_device_identity(&identity).unwrap();
    let connection = Connection::open(&config.daemon_db_path).unwrap();
    let refs_before: (String, String, String, String) = connection
        .query_row(
            r#"
SELECT root_private_key_ref_json, device_signing_private_key_ref_json,
       device_e2ee_private_key_ref_json, access_token_ref_json
FROM agent_device_identity WHERE agent_did = ?1
"#,
            [&identity.agent_did],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();

    state
        .replace_agent_device_access_token(
            &identity.agent_did,
            &identity.protocol_device_id,
            &identity.device_signing_key_id,
            identity.auth_generation,
            "refreshed-device-access-secret",
        )
        .unwrap();

    let loaded = state
        .load_agent_device_identity(&identity.agent_did)
        .unwrap()
        .unwrap();
    assert_eq!(loaded.access_token, "refreshed-device-access-secret");
    assert_eq!(loaded.root_private_key_pem, identity.root_private_key_pem);
    let refs_after: (String, String, String, String) = connection
        .query_row(
            r#"
SELECT root_private_key_ref_json, device_signing_private_key_ref_json,
       device_e2ee_private_key_ref_json, access_token_ref_json
FROM agent_device_identity WHERE agent_did = ?1
"#,
            [&identity.agent_did],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(refs_after.0, refs_before.0);
    assert_eq!(refs_after.1, refs_before.1);
    assert_eq!(refs_after.2, refs_before.2);
    assert_ne!(refs_after.3, refs_before.3);
    assert!(
        !String::from_utf8_lossy(&std::fs::read(&config.daemon_db_path).unwrap())
            .contains("refreshed-device-access-secret")
    );

    let error = state
        .replace_agent_device_access_token(
            &identity.agent_did,
            "wrong-device",
            &identity.device_signing_key_id,
            identity.auth_generation,
            "must-not-commit",
        )
        .unwrap_err();
    assert!(error.to_string().contains("binding is stale"));
    assert_eq!(
        state
            .load_agent_device_identity(&identity.agent_did)
            .unwrap()
            .unwrap()
            .access_token,
        "refreshed-device-access-secret"
    );
}

#[test]
fn promoted_registration_survives_definition_crash_window_then_scrubs_pending_secret() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open_with_root_key_bytes(&config, [83_u8; 32]);
    state.initialize().unwrap();
    let identity = device_identity_fixture("e3", crate::agent::AgentKind::Daemon);
    let pending = PendingAgentRegistrationRecord {
        registration_id: "agentreg-e3".to_owned(),
        dedupe_key: "dedupe-e3".to_owned(),
        agent_kind: identity.agent_kind,
        controller_did: "did:wba:awiki.info:controller".to_owned(),
        handle: identity.handle.clone(),
        display_name: identity.display_name.clone(),
        agent_did: identity.agent_did.clone(),
        protocol_device_id: identity.protocol_device_id.clone(),
        document_digest: identity.document_hash.clone(),
        request_digest: "request-digest-e3".to_owned(),
        secret_payload_json: serde_json::json!({
            "registration_token": "pending-secret-token",
            "root_private_key_pem": "pending-secret-root"
        }),
        status: "pending".to_owned(),
        attempt_count: 0,
        last_error_code: None,
        last_error_summary: None,
        created_at_ms: 1,
        updated_at_ms: 1,
    };
    state.store_pending_agent_registration(&pending).unwrap();
    state
        .promote_pending_agent_device_identity(&pending.registration_id, &identity)
        .unwrap();

    assert!(!state
        .scrub_completed_agent_registration(&identity.agent_did)
        .unwrap());
    let crash_window = state
        .load_pending_agent_registration(&pending.registration_id)
        .unwrap()
        .unwrap();
    assert_eq!(crash_window.status, "completed");
    state
        .upsert_agent_definition(&device_agent_definition(&identity))
        .unwrap();
    assert!(state
        .scrub_completed_agent_registration(&identity.agent_did)
        .unwrap());
    assert!(state
        .load_pending_agent_registration(&pending.registration_id)
        .unwrap()
        .is_none());
    assert!(state
        .secret_vault()
        .unwrap()
        .list()
        .unwrap()
        .iter()
        .all(|secret_ref| {
            secret_ref.kind != im_core::vault::SecretKind::IdentityRegistrationPending
        }));
}

#[test]
fn daemon_sync_probe_requires_every_active_agent_and_revocation_clears_readiness() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open_with_root_key_bytes(&config, [82_u8; 32]);
    state.initialize().unwrap();
    let first = device_identity_fixture("e1", crate::agent::AgentKind::Daemon);
    let second = device_identity_fixture("e2", crate::agent::AgentKind::Runtime);
    for identity in [&first, &second] {
        state.store_agent_device_identity(identity).unwrap();
        state
            .upsert_agent_definition(&device_agent_definition(identity))
            .unwrap();
    }

    state
        .mark_v2_subprotocol_negotiated(&first.agent_did)
        .unwrap();
    state
        .mark_sync_v2_reconcile_completed(&first.agent_did)
        .unwrap();
    let one_of_two = state.load_sync_probe().unwrap();
    assert!(!one_of_two.v2_subprotocol_negotiated);
    assert!(!one_of_two.v2_bootstrap_completed);
    assert_eq!(one_of_two.last_reconcile_protocol, None);

    state
        .mark_v2_subprotocol_negotiated(&second.agent_did)
        .unwrap();
    state
        .mark_sync_v2_reconcile_completed(&second.agent_did)
        .unwrap();
    let both = state.load_sync_probe().unwrap();
    assert!(both.v2_subprotocol_negotiated);
    assert!(both.v2_bootstrap_completed);
    assert_eq!(both.last_reconcile_protocol.as_deref(), Some("sync_v2"));

    state
        .mark_agent_device_auth_revoked(&second.agent_did)
        .unwrap();
    let revoked = state.load_sync_probe().unwrap();
    assert!(!revoked.v2_subprotocol_negotiated);
    assert!(!revoked.v2_bootstrap_completed);
    assert_eq!(revoked.last_reconcile_protocol, None);
    let reopened = DaemonState::open_with_root_key_bytes(&config, [82_u8; 32]);
    let fenced = reopened
        .load_agent_device_identity(&second.agent_did)
        .unwrap()
        .unwrap();
    assert_eq!(fenced.identity_status, "revoked");
    assert_eq!(fenced.authorization_status, "revoked");
}

#[test]
fn daemon_sync_probe_negotiation_is_current_boot_but_reconcile_is_durable() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open_with_root_key_bytes(&config, [84_u8; 32]);
    state.initialize().unwrap();
    let first = device_identity_fixture("boot-e1", crate::agent::AgentKind::Daemon);
    let second = device_identity_fixture("boot-e2", crate::agent::AgentKind::Runtime);
    for identity in [&first, &second] {
        state.store_agent_device_identity(identity).unwrap();
        state
            .upsert_agent_definition(&device_agent_definition(identity))
            .unwrap();
        state
            .mark_v2_subprotocol_negotiated(&identity.agent_did)
            .unwrap();
        state
            .mark_sync_v2_reconcile_completed(&identity.agent_did)
            .unwrap();
    }
    assert!(state.load_sync_probe().unwrap().v2_subprotocol_negotiated);

    assert_eq!(
        state.reset_v2_subprotocol_negotiation_for_boot().unwrap(),
        2
    );
    let after_restart = state.load_sync_probe().unwrap();
    assert!(!after_restart.v2_subprotocol_negotiated);
    assert!(after_restart.v2_bootstrap_completed);
    assert_eq!(
        after_restart.last_reconcile_protocol.as_deref(),
        Some("sync_v2")
    );

    state
        .mark_v2_subprotocol_negotiated(&first.agent_did)
        .unwrap();
    assert!(!state.load_sync_probe().unwrap().v2_subprotocol_negotiated);
    state
        .mark_v2_subprotocol_negotiated(&second.agent_did)
        .unwrap();
    assert!(state.load_sync_probe().unwrap().v2_subprotocol_negotiated);

    state
        .clear_v2_subprotocol_negotiated(&first.agent_did)
        .unwrap();
    let after_disconnect = state.load_sync_probe().unwrap();
    assert!(!after_disconnect.v2_subprotocol_negotiated);
    assert!(after_disconnect.v2_bootstrap_completed);
}

fn vault_ref_fingerprints(state: &DaemonState) -> Vec<String> {
    let mut refs = state
        .secret_vault()
        .unwrap()
        .list()
        .unwrap()
        .into_iter()
        .map(|secret_ref| serde_json::to_string(&secret_ref).unwrap())
        .collect::<Vec<_>>();
    refs.sort();
    refs
}

fn rotated_device_identity(identity: &AgentDeviceIdentityRecord) -> AgentDeviceIdentityRecord {
    let mut rotated = identity.clone();
    rotated.root_private_key_pem.push_str("-rotated");
    rotated.device_signing_private_key_pem.push_str("-rotated");
    rotated.device_e2ee_private_key_pem.push_str("-rotated");
    rotated.access_token.push_str("-rotated");
    rotated
}

fn pending_registration_fixture(
    identity: &AgentDeviceIdentityRecord,
) -> PendingAgentRegistrationRecord {
    PendingAgentRegistrationRecord {
        registration_id: format!("agentreg-{}", identity.identity_id),
        dedupe_key: format!("dedupe-{}", identity.identity_id),
        agent_kind: identity.agent_kind,
        controller_did: "did:wba:awiki.info:controller".to_owned(),
        handle: identity.handle.clone(),
        display_name: identity.display_name.clone(),
        agent_did: identity.agent_did.clone(),
        protocol_device_id: identity.protocol_device_id.clone(),
        document_digest: identity.document_hash.clone(),
        request_digest: format!("request-digest-{}", identity.identity_id),
        secret_payload_json: serde_json::json!({
            "registration_token": format!("pending-token-{}", identity.identity_id),
            "root_private_key_pem": format!("pending-root-{}", identity.identity_id),
        }),
        status: "pending".to_owned(),
        attempt_count: 0,
        last_error_code: None,
        last_error_summary: None,
        created_at_ms: 1,
        updated_at_ms: 1,
    }
}

fn pending_legacy_fixture(identity: &AgentDeviceIdentityRecord) -> PendingAgentLegacyUpgradeRecord {
    PendingAgentLegacyUpgradeRecord {
        agent_did: identity.agent_did.clone(),
        agent_kind: identity.agent_kind,
        protocol_device_id: identity.protocol_device_id.clone(),
        target_document_hash: identity.document_hash.clone(),
        secret_payload_json: serde_json::json!({
            "target_did_document": identity.did_document.clone(),
            "root_private_key_pem": identity.root_private_key_pem.clone(),
        }),
        status: "prepared".to_owned(),
        attempt_count: 0,
        last_error_code: None,
        updated_at_ms: 1,
    }
}

#[test]
fn agent_device_secret_staging_rolls_back_all_refs_on_partial_seal_failure() {
    use super::device_identity::{
        with_agent_device_secret_store_fault, AgentDeviceSecretStoreFault,
    };

    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open_with_root_key_bytes(&config, [85_u8; 32]);
    state.initialize().unwrap();
    let original = device_identity_fixture("seal-fail", crate::agent::AgentKind::Daemon);
    state.store_agent_device_identity(&original).unwrap();
    let before_refs = vault_ref_fingerprints(&state);

    let rotated = rotated_device_identity(&original);
    let error =
        with_agent_device_secret_store_fault(AgentDeviceSecretStoreFault::FailSealAt(2), || {
            state.store_agent_device_identity(&rotated)
        })
        .unwrap_err();
    assert!(error.to_string().contains("seal failure"));
    assert_eq!(vault_ref_fingerprints(&state), before_refs);
    assert_eq!(
        state
            .load_agent_device_identity(&original.agent_did)
            .unwrap()
            .unwrap()
            .access_token,
        original.access_token
    );
}

#[test]
fn pending_agent_secret_staging_rolls_back_refs_when_database_insert_fails() {
    use super::device_identity::{
        with_agent_device_secret_store_fault, AgentDeviceSecretStoreFault,
    };

    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open_with_root_key_bytes(&config, [88_u8; 32]);
    state.initialize().unwrap();
    let identity = device_identity_fixture("pending-db-fail", crate::agent::AgentKind::Daemon);
    let registration = pending_registration_fixture(&identity);
    let legacy = pending_legacy_fixture(&identity);
    let before_refs = vault_ref_fingerprints(&state);

    let registration_error =
        with_agent_device_secret_store_fault(AgentDeviceSecretStoreFault::FailBeforeCommit, || {
            state.store_pending_agent_registration(&registration)
        })
        .unwrap_err();
    assert!(registration_error.to_string().contains("database failure"));
    assert_eq!(vault_ref_fingerprints(&state), before_refs);
    assert!(state
        .load_pending_agent_registration(&registration.registration_id)
        .unwrap()
        .is_none());

    let legacy_error =
        with_agent_device_secret_store_fault(AgentDeviceSecretStoreFault::FailBeforeCommit, || {
            state.store_pending_agent_legacy_upgrade(&legacy)
        })
        .unwrap_err();
    assert!(legacy_error.to_string().contains("database failure"));
    assert_eq!(vault_ref_fingerprints(&state), before_refs);
    assert!(state
        .load_pending_agent_legacy_upgrade(&legacy.agent_did)
        .unwrap()
        .is_none());
}

#[test]
fn startup_recovery_preserves_referenced_pending_agent_secrets() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open_with_root_key_bytes(&config, [89_u8; 32]);
    state.initialize().unwrap();
    let registration_identity =
        device_identity_fixture("pending-registration", crate::agent::AgentKind::Daemon);
    let legacy_identity =
        device_identity_fixture("pending-legacy", crate::agent::AgentKind::Runtime);
    let registration = pending_registration_fixture(&registration_identity);
    let legacy = pending_legacy_fixture(&legacy_identity);
    state
        .store_pending_agent_registration(&registration)
        .unwrap();
    state.store_pending_agent_legacy_upgrade(&legacy).unwrap();
    let before_refs = vault_ref_fingerprints(&state);

    assert_eq!(
        state.recover_unreferenced_staged_agent_secrets().unwrap(),
        0
    );
    assert_eq!(vault_ref_fingerprints(&state), before_refs);
    assert_eq!(
        state
            .load_pending_agent_registration(&registration.registration_id)
            .unwrap()
            .unwrap()
            .secret_payload_json,
        registration.secret_payload_json
    );
    assert_eq!(
        state
            .load_pending_agent_legacy_upgrade(&legacy.agent_did)
            .unwrap()
            .unwrap()
            .secret_payload_json,
        legacy.secret_payload_json
    );
}

#[test]
fn refreshed_legacy_pending_payload_switch_is_atomic_and_crash_recoverable() {
    use super::device_identity::{
        with_agent_device_secret_store_fault, AgentDeviceSecretStoreFault,
    };

    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open_with_root_key_bytes(&config, [96_u8; 32]);
    state.initialize().unwrap();
    let identity = device_identity_fixture("legacy-refresh", crate::agent::AgentKind::Daemon);
    let pending = pending_legacy_fixture(&identity);
    state.store_pending_agent_legacy_upgrade(&pending).unwrap();
    let before_refs = vault_ref_fingerprints(&state);
    let refreshed_hash = "sha256:BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB";
    let refreshed_payload = serde_json::json!({
        "target_did_document": identity.did_document,
        "root_private_key_pem": "fresh-proof-same-device",
    });

    let error =
        with_agent_device_secret_store_fault(AgentDeviceSecretStoreFault::FailBeforeCommit, || {
            state.replace_pending_agent_legacy_upgrade_payload(
                &pending.agent_did,
                &pending.protocol_device_id,
                refreshed_hash,
                &refreshed_payload,
            )
        })
        .unwrap_err();
    assert!(error.to_string().contains("database failure"));
    assert_eq!(vault_ref_fingerprints(&state), before_refs);
    let unchanged = state
        .load_pending_agent_legacy_upgrade(&pending.agent_did)
        .unwrap()
        .unwrap();
    assert_eq!(unchanged.target_document_hash, pending.target_document_hash);
    assert_eq!(unchanged.secret_payload_json, pending.secret_payload_json);

    with_agent_device_secret_store_fault(
        AgentDeviceSecretStoreFault::SkipPostCommitCleanup,
        || {
            state.replace_pending_agent_legacy_upgrade_payload(
                &pending.agent_did,
                &pending.protocol_device_id,
                refreshed_hash,
                &refreshed_payload,
            )
        },
    )
    .unwrap();
    assert_eq!(vault_ref_fingerprints(&state).len(), before_refs.len() + 1);
    let refreshed = state
        .load_pending_agent_legacy_upgrade(&pending.agent_did)
        .unwrap()
        .unwrap();
    assert_eq!(refreshed.target_document_hash, refreshed_hash);
    assert_eq!(refreshed.secret_payload_json, refreshed_payload);
    assert_eq!(
        state.recover_unreferenced_staged_agent_secrets().unwrap(),
        1
    );
    assert_eq!(vault_ref_fingerprints(&state).len(), before_refs.len());
    assert_eq!(
        state
            .load_pending_agent_legacy_upgrade(&pending.agent_did)
            .unwrap()
            .unwrap()
            .secret_payload_json,
        refreshed_payload
    );
}

#[test]
fn startup_recovery_cleans_pending_secrets_left_after_post_commit_crash() {
    use super::device_identity::{
        with_agent_device_secret_store_fault, AgentDeviceSecretStoreFault,
    };

    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open_with_root_key_bytes(&config, [90_u8; 32]);
    state.initialize().unwrap();

    let registration_identity =
        device_identity_fixture("registration-scrub", crate::agent::AgentKind::Daemon);
    let registration = pending_registration_fixture(&registration_identity);
    state
        .store_pending_agent_registration(&registration)
        .unwrap();
    state
        .promote_pending_agent_device_identity(
            &registration.registration_id,
            &registration_identity,
        )
        .unwrap();
    state
        .upsert_agent_definition(&device_agent_definition(&registration_identity))
        .unwrap();

    let mut legacy_identity =
        device_identity_fixture("legacy-scrub", crate::agent::AgentKind::Runtime);
    legacy_identity.legacy_migration_state = "completed".to_owned();
    let mut legacy = pending_legacy_fixture(&legacy_identity);
    legacy.status = "completed".to_owned();
    state.store_pending_agent_legacy_upgrade(&legacy).unwrap();
    state.store_agent_device_identity(&legacy_identity).unwrap();

    let before_scrub = vault_ref_fingerprints(&state);
    with_agent_device_secret_store_fault(
        AgentDeviceSecretStoreFault::SkipPostCommitCleanup,
        || {
            assert!(state
                .scrub_completed_agent_registration(&registration_identity.agent_did)
                .unwrap());
            assert!(state
                .scrub_completed_agent_legacy_upgrade(&legacy_identity.agent_did)
                .unwrap());
        },
    );
    assert_eq!(vault_ref_fingerprints(&state), before_scrub);
    assert!(state
        .load_pending_agent_registration(&registration.registration_id)
        .unwrap()
        .is_none());
    assert!(state
        .load_pending_agent_legacy_upgrade(&legacy.agent_did)
        .unwrap()
        .is_none());

    assert_eq!(
        state.recover_unreferenced_staged_agent_secrets().unwrap(),
        2
    );
    assert_eq!(vault_ref_fingerprints(&state).len(), before_scrub.len() - 2);
}

#[test]
fn agent_device_secret_staging_rolls_back_all_refs_on_database_failure() {
    use super::device_identity::{
        with_agent_device_secret_store_fault, AgentDeviceSecretStoreFault,
    };

    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open_with_root_key_bytes(&config, [86_u8; 32]);
    state.initialize().unwrap();
    let original = device_identity_fixture("db-fail", crate::agent::AgentKind::Daemon);
    state.store_agent_device_identity(&original).unwrap();
    let before_refs = vault_ref_fingerprints(&state);

    let rotated = rotated_device_identity(&original);
    let error =
        with_agent_device_secret_store_fault(AgentDeviceSecretStoreFault::FailBeforeCommit, || {
            state.store_agent_device_identity(&rotated)
        })
        .unwrap_err();
    assert!(error.to_string().contains("database failure"));
    assert_eq!(vault_ref_fingerprints(&state), before_refs);
    assert_eq!(
        state
            .load_agent_device_identity(&original.agent_did)
            .unwrap()
            .unwrap()
            .access_token,
        original.access_token
    );
}

#[test]
fn startup_recovery_only_removes_unreferenced_post_commit_staged_refs() {
    use super::device_identity::{
        with_agent_device_secret_store_fault, AgentDeviceSecretStoreFault,
    };

    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open_with_root_key_bytes(&config, [87_u8; 32]);
    state.initialize().unwrap();
    let original = device_identity_fixture("crash-cleanup", crate::agent::AgentKind::Daemon);
    state.store_agent_device_identity(&original).unwrap();
    let original_ref_count = vault_ref_fingerprints(&state).len();

    let rotated = rotated_device_identity(&original);
    with_agent_device_secret_store_fault(
        AgentDeviceSecretStoreFault::SkipPostCommitCleanup,
        || state.store_agent_device_identity(&rotated),
    )
    .unwrap();
    assert_eq!(vault_ref_fingerprints(&state).len(), original_ref_count + 4);
    assert_eq!(
        state
            .load_agent_device_identity(&original.agent_did)
            .unwrap()
            .unwrap()
            .access_token,
        rotated.access_token
    );

    let reopened = DaemonState::open_with_root_key_bytes(&config, [87_u8; 32]);
    assert_eq!(
        reopened
            .recover_unreferenced_staged_agent_secrets()
            .unwrap(),
        4
    );
    assert_eq!(vault_ref_fingerprints(&reopened).len(), original_ref_count);
    assert_eq!(
        reopened
            .load_agent_device_identity(&original.agent_did)
            .unwrap()
            .unwrap()
            .access_token,
        rotated.access_token
    );
    assert_eq!(
        reopened
            .recover_unreferenced_staged_agent_secrets()
            .unwrap(),
        0
    );
}

#[test]
fn daemon_sync_probe_public_shape_is_closed_and_identity_free() {
    let value = serde_json::to_value(DaemonSyncProbe {
        v2_subprotocol_negotiated: true,
        v2_bootstrap_completed: true,
        last_reconcile_protocol: Some("sync_v2".to_owned()),
        legacy_sync_used: false,
    })
    .unwrap();
    let object = value.as_object().unwrap();
    assert_eq!(object.len(), 4);
    assert!(object.contains_key("v2_subprotocol_negotiated"));
    assert!(object.contains_key("v2_bootstrap_completed"));
    assert!(object.contains_key("last_reconcile_protocol"));
    assert!(object.contains_key("legacy_sync_used"));

    let serialized = serde_json::to_string(&value).unwrap();
    for forbidden in [
        "agent_did",
        "account_id",
        "device_id",
        "access_token",
        "cursor",
        "raw_frame",
    ] {
        assert!(!serialized.contains(forbidden));
    }
}
