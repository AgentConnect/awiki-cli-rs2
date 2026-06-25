use super::schema::DAEMON_SCHEMA_VERSION;
use super::*;
use crate::runtime::RuntimeTaskTriggerKind;
use crate::runtime::{RuntimeConversationScope, RuntimeInvocationAuthority};

#[test]
fn initialize_creates_required_tables() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let summary = DaemonState::open(&config).unwrap().initialize().unwrap();
    assert_eq!(summary.schema_version, DAEMON_SCHEMA_VERSION);

    let connection = Connection::open(&config.daemon_db_path).unwrap();
    for table in [
        "schema_migrations",
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
            ..sync.clone()
        })
        .unwrap();
    let still_sent = state
        .load_message_sync_outbox("message-sync:did:human:alice:msg_1")
        .unwrap()
        .unwrap();
    assert_eq!(still_sent.status, "sent");
    assert_eq!(still_sent.payload_json["message_id"], "msg_1");

    let failed_sync = MessageSyncOutboxRecord {
        idempotency_key: "message-sync:did:human:alice:msg_failed".to_string(),
        payload_json: serde_json::json!({
            "schema": "awiki.message.sync.v1",
            "message_id": "msg_failed"
        }),
        ..sync.clone()
    };
    state.upsert_message_sync_outbox(&failed_sync).unwrap();
    assert!(state
        .mark_message_sync_outbox_sending(&failed_sync.idempotency_key)
        .unwrap());
    state
        .mark_message_sync_outbox_failed_terminal(
            &failed_sync.idempotency_key,
            "message_sync_delivery_failed",
            "terminal delivery failure",
        )
        .unwrap();
    state
        .upsert_message_sync_outbox(&MessageSyncOutboxRecord {
            status: "pending".to_string(),
            payload_json: serde_json::json!({
                "schema": "awiki.message.sync.v1",
                "message_id": "msg_failed_changed"
            }),
            ..failed_sync.clone()
        })
        .unwrap();
    let still_failed = state
        .load_message_sync_outbox(&failed_sync.idempotency_key)
        .unwrap()
        .unwrap();
    assert_eq!(still_failed.status, "failed_terminal");
    assert_eq!(still_failed.payload_json["message_id"], "msg_failed");
    assert_eq!(
        still_failed.last_error_code.as_deref(),
        Some("message_sync_delivery_failed")
    );
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
fn control_command_conditional_transition_preserves_late_terminal_result() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();

    state
        .try_begin_control_command(
            "did:agent:daemon",
            "controller-scope:v1:test-alice",
            "cmd_upgrade_race",
            "daemon.upgrade",
            "msg_upgrade_race",
            Some("latest"),
        )
        .unwrap();
    state
        .mark_control_command_state(
            "did:agent:daemon",
            "controller-scope:v1:test-alice",
            "cmd_upgrade_race",
            "succeeded",
            serde_json::json!({
                "command": "daemon.upgrade",
                "status": "ready",
                "version": "0.2.0",
                "source": "upgrade_task",
            }),
            None,
        )
        .unwrap();

    let updated = state
        .mark_control_command_state_if_status_in(
            "did:agent:daemon",
            "controller-scope:v1:test-alice",
            "cmd_upgrade_race",
            &["in_progress", "restart_scheduled"],
            "failed",
            serde_json::json!({
                "command": "daemon.upgrade",
                "status": "failed",
                "source": "reconcile",
            }),
            Some("daemon upgrade did not reach the requested version"),
        )
        .unwrap();
    assert!(!updated);

    let stored = state
        .load_control_command_state(
            "did:agent:daemon",
            "controller-scope:v1:test-alice",
            "cmd_upgrade_race",
        )
        .unwrap()
        .unwrap();
    assert_eq!(stored.status, "succeeded");
    assert_eq!(stored.result_json["source"], "upgrade_task");
    assert!(stored.error_summary.is_none());
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

    state
        .upsert_runtime_final_outbox_pending(&RuntimeFinalOutboxRecord {
            idempotency_key:
                "runtime-final:did:agent:hermes:run_failed:controller-scope:v1:test-alice"
                    .to_string(),
            run_id: "run_failed".to_string(),
            final_text: "failed final text".to_string(),
            ..record.clone()
        })
        .unwrap();
    assert!(state
        .mark_runtime_final_outbox_sending(
            "runtime-final:did:agent:hermes:run_failed:controller-scope:v1:test-alice"
        )
        .unwrap());
    state
        .mark_runtime_final_outbox_failed_terminal(
            "runtime-final:did:agent:hermes:run_failed:controller-scope:v1:test-alice",
            "final_delivery_failed",
            "terminal delivery failure",
        )
        .unwrap();
    state
        .upsert_runtime_final_outbox_pending(&RuntimeFinalOutboxRecord {
            idempotency_key:
                "runtime-final:did:agent:hermes:run_failed:controller-scope:v1:test-alice"
                    .to_string(),
            run_id: "run_failed".to_string(),
            final_text: "changed final text".to_string(),
            next_attempt_at_ms: now + 60_000,
            ..record.clone()
        })
        .unwrap();
    let failed = state
        .load_runtime_final_outbox_by_run("run_failed")
        .unwrap()
        .unwrap();
    assert_eq!(failed.status, "failed_terminal");
    assert_eq!(failed.final_text, "failed final text");
    assert_eq!(
        failed.last_error_code.as_deref(),
        Some("final_delivery_failed")
    );
}

#[test]
fn runtime_final_plain_delivery_migration_preserves_terminal_failure() {
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
            VALUES (14, 'legacy-fixture');

            CREATE TABLE runtime_final_outbox (
                idempotency_key TEXT PRIMARY KEY,
                run_id TEXT NOT NULL UNIQUE,
                agent_did TEXT NOT NULL,
                runtime_profile_id TEXT NOT NULL,
                controller_did TEXT NOT NULL,
                conversation_id TEXT,
                final_text TEXT NOT NULL,
                security TEXT NOT NULL DEFAULT 'default_plain',
                status TEXT NOT NULL,
                attempt_count INTEGER NOT NULL DEFAULT 0,
                next_attempt_at_ms INTEGER NOT NULL DEFAULT 0,
                last_error_code TEXT,
                last_error_summary TEXT,
                message_id TEXT,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                sent_at_ms INTEGER
            );

            INSERT INTO runtime_final_outbox (
                idempotency_key,
                run_id,
                agent_did,
                runtime_profile_id,
                controller_did,
                conversation_id,
                final_text,
                security,
                status,
                attempt_count,
                next_attempt_at_ms,
                last_error_code,
                last_error_summary,
                message_id,
                created_at_ms,
                updated_at_ms,
                sent_at_ms
            ) VALUES
            (
                'runtime-final:pending',
                'run_pending',
                'did:agent:hermes',
                'profile_hermes',
                'did:human:alice',
                'direct:did:human:alice',
                'pending final',
                'direct_e2ee',
                'pending',
                3,
                12345,
                'final_delivery_retry',
                'retry later',
                NULL,
                1,
                1,
                NULL
            ),
            (
                'runtime-final:failed',
                'run_failed',
                'did:agent:hermes',
                'profile_hermes',
                'did:human:alice',
                'direct:did:human:alice',
                'failed final',
                'direct_e2ee',
                'failed_terminal',
                5,
                12345,
                'final_delivery_failed',
                'terminal failure',
                NULL,
                1,
                1,
                NULL
            );
            "#,
        )
        .unwrap();
    drop(connection);

    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    DaemonState::open(&config).unwrap().initialize().unwrap();

    let connection = Connection::open(db_path).unwrap();
    let pending: (String, i64, i64, Option<String>, Option<String>) = connection
        .query_row(
            r#"
SELECT status, attempt_count, next_attempt_at_ms, last_error_code, last_error_summary
FROM runtime_final_outbox
WHERE idempotency_key = 'runtime-final:pending'
"#,
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(pending, ("pending".to_string(), 0, 0, None, None));

    let failed: (String, i64, i64, Option<String>, Option<String>) = connection
        .query_row(
            r#"
SELECT status, attempt_count, next_attempt_at_ms, last_error_code, last_error_summary
FROM runtime_final_outbox
WHERE idempotency_key = 'runtime-final:failed'
"#,
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(
        failed,
        (
            "failed_terminal".to_string(),
            5,
            12345,
            Some("final_delivery_failed".to_string()),
            Some("terminal failure".to_string())
        )
    );
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
    assert_eq!(loaded.default_workspace_mode, WorkspaceMode::RouteRoot);
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
