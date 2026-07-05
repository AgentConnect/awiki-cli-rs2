use anyhow::Result;
use rusqlite::Connection;

use crate::agent::GENERIC_CLI_RUNTIME_PLUGIN_ID;

use super::records::DEFAULT_CLI_RECIPIENT_POLICY_JSON;

pub(super) const DAEMON_SCHEMA_VERSION: i64 = 33;

pub fn current_schema_version(connection: &Connection) -> Result<i64> {
    let version = connection.query_row(
        "SELECT version FROM schema_migrations ORDER BY version DESC LIMIT 1",
        [],
        |row| row.get(0),
    )?;
    Ok(version)
}

pub(super) fn initialize_schema(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        r#"
        PRAGMA foreign_keys = ON;
        PRAGMA secure_delete = ON;

        CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS daemon_state_metadata (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS agent_definition (
            agent_did TEXT PRIMARY KEY,
            controller_user_id TEXT NOT NULL DEFAULT '',
            controller_full_handle TEXT NOT NULL DEFAULT '',
            controller_scope_key TEXT NOT NULL DEFAULT '',
            controller_did TEXT NOT NULL,
            runtime_profile_id TEXT,
            status TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS runtime_profile (
            runtime_profile_id TEXT PRIMARY KEY,
            agent_did TEXT,
            runtime_plugin_id TEXT NOT NULL,
            display_name TEXT,
            preferred_language TEXT NOT NULL DEFAULT 'zh-Hans',
            status TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS workspace_binding (
            workspace_id TEXT PRIMARY KEY,
            agent_did TEXT,
            runtime_profile_id TEXT,
            workspace_root TEXT NOT NULL,
            workspace_mode TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS cli_runtime_profile (
            runtime_profile_id TEXT PRIMARY KEY,
            driver_id TEXT NOT NULL,
            binary_path TEXT,
            config_home TEXT,
            auth_mode TEXT,
            default_model TEXT,
            default_sandbox TEXT,
            default_workspace_mode TEXT NOT NULL,
            recipient_policy_json TEXT NOT NULL,
            driver_config_json TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS cli_driver_run (
            run_id TEXT PRIMARY KEY,
            agent_did TEXT NOT NULL,
            runtime_profile_id TEXT NOT NULL,
            driver_id TEXT NOT NULL,
            controller_user_id TEXT NOT NULL DEFAULT '',
            controller_full_handle TEXT NOT NULL DEFAULT '',
            controller_scope_key TEXT NOT NULL DEFAULT '',
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

        CREATE TABLE IF NOT EXISTS cli_route_sessions (
            route_key TEXT PRIMARY KEY,
            route_key_hash TEXT NOT NULL,
            agent_did TEXT NOT NULL,
            runtime_profile_id TEXT NOT NULL,
            driver_id TEXT NOT NULL,
            controller_user_id TEXT NOT NULL DEFAULT '',
            controller_full_handle TEXT NOT NULL DEFAULT '',
            controller_scope_key TEXT NOT NULL DEFAULT '',
            controller_did TEXT NOT NULL,
            conversation_id TEXT NOT NULL,
            workspace_path TEXT NOT NULL,
            session_dir TEXT NOT NULL,
            native_session_id TEXT,
            native_session_source TEXT,
            synthetic_session_id TEXT,
            status TEXT NOT NULL,
            last_run_id TEXT,
            last_message_id TEXT,
            lock_run_id TEXT,
            lock_owner TEXT,
            lock_expires_at_ms INTEGER,
            last_error_code TEXT,
            last_error_summary TEXT,
            version INTEGER NOT NULL DEFAULT 0,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            UNIQUE(runtime_profile_id, route_key_hash)
        );

        CREATE INDEX IF NOT EXISTS idx_cli_route_sessions_profile_status
        ON cli_route_sessions(runtime_profile_id, controller_scope_key, status);

        CREATE INDEX IF NOT EXISTS idx_cli_route_sessions_lock
        ON cli_route_sessions(status, lock_expires_at_ms);

        CREATE TABLE IF NOT EXISTS cli_runtime_locks (
            lock_key TEXT PRIMARY KEY,
            lock_kind TEXT NOT NULL,
            runtime_profile_id TEXT,
            driver_id TEXT,
            run_id TEXT NOT NULL,
            lock_owner TEXT NOT NULL,
            lock_expires_at_ms INTEGER NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_cli_runtime_locks_kind
        ON cli_runtime_locks(lock_kind, runtime_profile_id, driver_id);

        CREATE INDEX IF NOT EXISTS idx_cli_runtime_locks_expiry
        ON cli_runtime_locks(lock_expires_at_ms);

        CREATE TABLE IF NOT EXISTS runtime_run (
            run_id TEXT PRIMARY KEY,
            task_id TEXT DEFAULT '',
            agent_did TEXT NOT NULL,
            runtime_profile_id TEXT NOT NULL,
            runtime_plugin_id TEXT NOT NULL,
            workspace_id TEXT,
            status TEXT NOT NULL,
            started_at TEXT NOT NULL,
            completed_at TEXT,
            updated_at TEXT NOT NULL,
            started_at_ms INTEGER NOT NULL DEFAULT 0,
            completed_at_ms INTEGER,
            updated_at_ms INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS runtime_task (
            task_id TEXT PRIMARY KEY,
            agent_did TEXT NOT NULL,
            agent_handle TEXT NOT NULL DEFAULT '',
            controller_user_id TEXT NOT NULL DEFAULT '',
            controller_full_handle TEXT NOT NULL DEFAULT '',
            controller_scope_key TEXT NOT NULL DEFAULT '',
            controller_did TEXT NOT NULL,
            sender_did TEXT NOT NULL,
            requester_did TEXT NOT NULL DEFAULT '',
            requester_user_id TEXT,
            requester_full_handle TEXT,
            trigger_kind TEXT NOT NULL DEFAULT 'controller_direct',
            conversation_scope_kind TEXT NOT NULL DEFAULT 'controller_private',
            conversation_scope_key TEXT NOT NULL DEFAULT '',
            invocation_authority TEXT NOT NULL DEFAULT 'controller',
            reply_recipient_did TEXT NOT NULL DEFAULT '',
            conversation_id TEXT,
            task_text TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS runtime_rpc_tokens (
            token_id TEXT PRIMARY KEY,
            token_secret_hash TEXT NOT NULL,
            agent_did TEXT NOT NULL,
            runtime_profile_id TEXT NOT NULL,
            run_id TEXT NOT NULL,
            allowed_methods_json TEXT NOT NULL,
            allowed_recipients_json TEXT,
            allowed_message_security_json TEXT,
            expires_at TEXT NOT NULL DEFAULT '',
            expires_at_ms INTEGER NOT NULL,
            single_use INTEGER NOT NULL DEFAULT 0,
            revoked_at TEXT,
            revoked_at_ms INTEGER,
            used_at TEXT,
            used_at_ms INTEGER,
            created_at TEXT NOT NULL DEFAULT '',
            created_at_ms INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS audit_log (
            audit_id TEXT PRIMARY KEY,
            event_type TEXT NOT NULL,
            agent_did TEXT,
            runtime_profile_id TEXT,
            run_id TEXT,
            token_id TEXT,
            detail_json TEXT,
            created_at TEXT NOT NULL DEFAULT '',
            created_at_ms INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS agent_identity (
            agent_did TEXT PRIMARY KEY,
            handle TEXT NOT NULL,
            agent_kind TEXT NOT NULL,
            did_document_json TEXT NOT NULL,
            endpoint_url TEXT,
            key_algorithm TEXT NOT NULL,
            public_key TEXT NOT NULL,
            auth_private_key_pem TEXT NOT NULL,
            e2ee_signing_private_key_pem TEXT,
            e2ee_agreement_private_key_pem TEXT,
            auth_private_key_ref_json TEXT,
            e2ee_signing_private_key_ref_json TEXT,
            e2ee_agreement_private_key_ref_json TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS agent_auth_state (
            agent_did TEXT PRIMARY KEY,
            jwt_token TEXT NOT NULL,
            jwt_token_ref_json TEXT,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS hermes_profiles (
            agent_did TEXT PRIMARY KEY,
            runtime_profile_id TEXT NOT NULL,
            hermes_profile TEXT NOT NULL,
            hermes_home TEXT NOT NULL,
            hermes_version TEXT,
            awiki_skills_version TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS hermes_native_sessions (
            id TEXT PRIMARY KEY,
            runtime_session_id TEXT NOT NULL,
            agent_did TEXT NOT NULL,
            agent_handle TEXT NOT NULL DEFAULT '',
            runtime_profile_id TEXT NOT NULL,
            controller_scope_key TEXT NOT NULL DEFAULT '',
            controller_did TEXT NOT NULL,
            session_actor_did TEXT NOT NULL DEFAULT '',
            scope_kind TEXT NOT NULL DEFAULT 'controller_private',
            scope_key TEXT NOT NULL DEFAULT '',
            conversation_id TEXT,
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

        CREATE UNIQUE INDEX IF NOT EXISTS idx_hermes_native_sessions_active_route
        ON hermes_native_sessions(route_key)
        WHERE status = 'active';

        CREATE TABLE IF NOT EXISTS runtime_daemon_binding (
            runtime_agent_did TEXT PRIMARY KEY,
            daemon_agent_did TEXT NOT NULL,
            controller_user_id TEXT NOT NULL DEFAULT '',
            controller_full_handle TEXT NOT NULL DEFAULT '',
            controller_scope_key TEXT NOT NULL DEFAULT '',
            controller_did TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS agent_status_query_throttle (
            daemon_agent_did TEXT NOT NULL,
            controller_scope_key TEXT NOT NULL DEFAULT '',
            controller_did TEXT NOT NULL,
            last_snapshot_at_ms INTEGER NOT NULL,
            PRIMARY KEY (daemon_agent_did, controller_did)
        );

        CREATE TABLE IF NOT EXISTS runtime_retry_queue (
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
            next_attempt_at_ms INTEGER NOT NULL DEFAULT 0,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_runtime_retry_queue_status
        ON runtime_retry_queue(status, created_at_ms);


        CREATE TABLE IF NOT EXISTS cli_route_message_queue (
            queue_id TEXT PRIMARY KEY,
            agent_did TEXT NOT NULL,
            runtime_profile_id TEXT NOT NULL,
            driver_id TEXT NOT NULL,
            controller_user_id TEXT NOT NULL DEFAULT '',
            controller_full_handle TEXT NOT NULL DEFAULT '',
            controller_scope_key TEXT NOT NULL DEFAULT '',
            controller_did TEXT NOT NULL,
            conversation_id TEXT NOT NULL,
            route_key TEXT NOT NULL,
            route_key_hash TEXT NOT NULL,
            source_message_id TEXT NOT NULL,
            task_id TEXT,
            run_id TEXT,
            status TEXT NOT NULL,
            enqueue_reason TEXT NOT NULL,
            attempts INTEGER NOT NULL DEFAULT 0,
            next_attempt_at_ms INTEGER NOT NULL DEFAULT 0,
            route_sequence INTEGER NOT NULL,
            last_error_code TEXT,
            last_error_summary TEXT,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            UNIQUE(runtime_profile_id, route_key, source_message_id)
        );

        CREATE INDEX IF NOT EXISTS idx_cli_route_message_queue_due
        ON cli_route_message_queue(status, next_attempt_at_ms, created_at_ms, queue_id);

        CREATE INDEX IF NOT EXISTS idx_cli_route_message_queue_route_order
        ON cli_route_message_queue(runtime_profile_id, route_key, route_sequence);

        CREATE TABLE IF NOT EXISTS runtime_agent_create_request (
            daemon_agent_did TEXT NOT NULL,
            controller_scope_key TEXT NOT NULL DEFAULT '',
            controller_did TEXT NOT NULL,
            client_request_id TEXT NOT NULL,
            runtime_agent_did TEXT NOT NULL,
            command_id TEXT NOT NULL,
            outcome_json TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            PRIMARY KEY (daemon_agent_did, controller_scope_key, client_request_id)
        );

        CREATE INDEX IF NOT EXISTS idx_runtime_agent_create_request_runtime
        ON runtime_agent_create_request(runtime_agent_did);

        CREATE TABLE IF NOT EXISTS runtime_final_outbox (
            idempotency_key TEXT PRIMARY KEY,
            run_id TEXT NOT NULL UNIQUE,
            agent_did TEXT NOT NULL,
            runtime_profile_id TEXT NOT NULL,
            controller_scope_key TEXT NOT NULL DEFAULT '',
            controller_did TEXT NOT NULL,
            recipient_did TEXT NOT NULL DEFAULT '',
            conversation_id TEXT,
            final_text TEXT NOT NULL,
            final_source TEXT NOT NULL DEFAULT 'unknown_legacy',
            final_body_hash TEXT NOT NULL DEFAULT '',
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

        CREATE INDEX IF NOT EXISTS idx_runtime_final_outbox_due
        ON runtime_final_outbox(status, next_attempt_at_ms, created_at_ms);

        CREATE TABLE IF NOT EXISTS user_delegated_identity (
            verification_method TEXT PRIMARY KEY,
            user_did TEXT NOT NULL,
            app_instance_id TEXT NOT NULL,
            controller_did TEXT NOT NULL,
            daemon_agent_did TEXT NOT NULL,
            public_key_multibase TEXT NOT NULL,
            private_key_material TEXT NOT NULL,
            private_key_ref_json TEXT,
            allowed_scopes_json TEXT NOT NULL,
            status TEXT NOT NULL,
            expires_at TEXT,
            bootstrap_id TEXT NOT NULL,
            idempotency_key TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_user_delegated_identity_user
        ON user_delegated_identity(user_did, app_instance_id, status);

        CREATE TABLE IF NOT EXISTS bootstrap_replay (
            bootstrap_id TEXT PRIMARY KEY,
            idempotency_key TEXT NOT NULL UNIQUE,
            payload_hash TEXT NOT NULL,
            user_did TEXT NOT NULL,
            verification_method TEXT NOT NULL,
            app_instance_id TEXT NOT NULL,
            daemon_agent_did TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_bootstrap_replay_identity
        ON bootstrap_replay(user_did, verification_method, app_instance_id);

        CREATE TABLE IF NOT EXISTS secure_bootstrap_replay (
            operation_id TEXT PRIMARY KEY,
            nonce TEXT NOT NULL UNIQUE,
            envelope_hash TEXT NOT NULL,
            recipient_daemon_did TEXT NOT NULL,
            recipient_key_id TEXT NOT NULL,
            sender_human_did TEXT NOT NULL,
            bootstrap_id TEXT NOT NULL,
            idempotency_key TEXT NOT NULL,
            payload_sha256 TEXT,
            expires_at TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_secure_bootstrap_replay_sender
        ON secure_bootstrap_replay(sender_human_did, recipient_daemon_did, status);

        CREATE TABLE IF NOT EXISTS app_message_agent_binding (
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

        CREATE INDEX IF NOT EXISTS idx_app_message_agent_binding_active
        ON app_message_agent_binding(user_did, app_instance_id, role, status, revoked_at_ms);

        CREATE UNIQUE INDEX IF NOT EXISTS ux_app_message_agent_binding_active_role
        ON app_message_agent_binding(user_did, app_instance_id, role)
        WHERE revoked_at_ms IS NULL
          AND status IN ('message_agent_ready', 'message_agent_active', 'message_agent_ensuring');

        CREATE TABLE IF NOT EXISTS inbox_cursor (
            owner_did TEXT NOT NULL,
            inbox_scope TEXT NOT NULL,
            cursor TEXT,
            updated_at_ms INTEGER NOT NULL,
            PRIMARY KEY (owner_did, inbox_scope)
        );

        CREATE TABLE IF NOT EXISTS processed_message (
            owner_did TEXT NOT NULL,
            message_id TEXT NOT NULL,
            schema TEXT NOT NULL,
            processed_at_ms INTEGER NOT NULL,
            status TEXT NOT NULL,
            PRIMARY KEY (owner_did, message_id)
        );

        CREATE INDEX IF NOT EXISTS idx_processed_message_status
        ON processed_message(owner_did, status, processed_at_ms);

        CREATE TABLE IF NOT EXISTS message_event (
            event_id TEXT PRIMARY KEY,
            owner_did TEXT NOT NULL,
            conversation_id TEXT,
            message_id TEXT NOT NULL,
            message_kind TEXT NOT NULL,
            sender_did TEXT NOT NULL,
            received_at TEXT,
            plain_text_ref_or_excerpt TEXT,
            content_hash TEXT NOT NULL,
            schema TEXT NOT NULL,
            processing_status TEXT NOT NULL,
            retention_class TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_message_event_owner_message
        ON message_event(owner_did, message_id);

        CREATE INDEX IF NOT EXISTS idx_message_event_processing
        ON message_event(owner_did, processing_status, created_at_ms);

        CREATE TABLE IF NOT EXISTS message_sync_outbox (
            idempotency_key TEXT PRIMARY KEY,
            owner_did TEXT NOT NULL,
            app_instance_id TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            status TEXT NOT NULL,
            attempt_count INTEGER NOT NULL DEFAULT 0,
            next_attempt_at_ms INTEGER NOT NULL DEFAULT 0,
            last_error_code TEXT,
            last_error_summary TEXT,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            sent_at_ms INTEGER
        );

        CREATE INDEX IF NOT EXISTS idx_message_sync_outbox_due
        ON message_sync_outbox(status, next_attempt_at_ms, created_at_ms);

        CREATE TABLE IF NOT EXISTS control_command_state (
            daemon_agent_did TEXT NOT NULL,
            controller_scope_key TEXT NOT NULL,
            command_id TEXT NOT NULL,
            command TEXT NOT NULL,
            message_id TEXT NOT NULL,
            status TEXT NOT NULL,
            target_version TEXT,
            result_json TEXT NOT NULL DEFAULT '{}',
            error_summary TEXT,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            PRIMARY KEY (daemon_agent_did, controller_scope_key, command_id)
        );

        CREATE INDEX IF NOT EXISTS idx_control_command_state_message
        ON control_command_state(daemon_agent_did, message_id);

        INSERT OR IGNORE INTO schema_migrations (version, applied_at)
        VALUES (1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
        "#,
    )?;
    migrate_runtime_rpc_tokens_v2(connection)?;
    migrate_audit_log_v2(connection)?;
    migrate_runtime_run_v3(connection)?;
    migrate_runtime_task_v3(connection)?;
    migrate_agent_definition_v4(connection)?;
    migrate_agent_auth_state_v5(connection)?;
    migrate_hermes_profiles_v6(connection)?;
    migrate_hermes_native_sessions_v7(connection)?;
    migrate_cli_runtime_profile_v8(connection)?;
    migrate_runtime_rpc_tokens_v9(connection)?;
    migrate_cli_driver_run_v10(connection)?;
    migrate_agent_management_v11(connection)?;
    migrate_runtime_retry_queue_v12(connection)?;
    migrate_runtime_agent_create_request_v13(connection)?;
    migrate_runtime_final_outbox_v14(connection)?;
    migrate_runtime_final_plain_delivery_v15(connection)?;
    migrate_user_delegated_identity_v16(connection)?;
    migrate_app_message_agent_binding_v17(connection)?;
    migrate_user_delegated_inbox_sync_v18(connection)?;
    migrate_controller_scope_v19(connection)?;
    migrate_control_command_state_v20(connection)?;
    migrate_runtime_requester_contract_v21(connection)?;
    migrate_secure_bootstrap_replay_v22(connection)?;
    migrate_cli_route_sessions_v23(connection)?;
    migrate_cli_runtime_locks_v24(connection)?;
    migrate_daemon_state_metadata_v25(connection)?;
    migrate_runtime_retry_queue_due_v26(connection)?;
    migrate_cli_route_message_queue_v27(connection)?;
    migrate_runtime_final_outbox_provenance_v28(connection)?;
    migrate_runtime_scope_authority_v29(connection)?;
    migrate_hermes_native_session_stored_ids_v30(connection)?;
    migrate_runtime_profile_preferred_language_v31(connection)?;
    migrate_agent_identity_vault_refs_v32(connection)?;
    migrate_user_delegated_identity_vault_refs_v33(connection)?;
    connection.execute(
        "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
        [],
    )?;
    connection.execute(
        "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
        [DAEMON_SCHEMA_VERSION],
    )?;
    Ok(())
}

fn migrate_agent_identity_vault_refs_v32(connection: &Connection) -> Result<()> {
    add_column_if_missing(
        connection,
        "agent_identity",
        "auth_private_key_ref_json",
        "TEXT",
    )?;
    add_column_if_missing(
        connection,
        "agent_identity",
        "e2ee_signing_private_key_ref_json",
        "TEXT",
    )?;
    add_column_if_missing(
        connection,
        "agent_identity",
        "e2ee_agreement_private_key_ref_json",
        "TEXT",
    )?;
    Ok(())
}

fn migrate_user_delegated_identity_vault_refs_v33(connection: &Connection) -> Result<()> {
    add_column_if_missing(
        connection,
        "user_delegated_identity",
        "private_key_ref_json",
        "TEXT",
    )
}

fn migrate_runtime_profile_preferred_language_v31(connection: &Connection) -> Result<()> {
    add_column_if_missing(
        connection,
        "runtime_profile",
        "preferred_language",
        "TEXT NOT NULL DEFAULT 'zh-Hans'",
    )?;
    connection.execute_batch(
        r#"
        UPDATE runtime_profile
        SET preferred_language = 'zh-Hans'
        WHERE preferred_language IS NULL OR preferred_language = '';
        "#,
    )?;
    Ok(())
}

fn migrate_daemon_state_metadata_v25(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS daemon_state_metadata (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );
        "#,
    )?;
    Ok(())
}

fn migrate_cli_runtime_locks_v24(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS cli_runtime_locks (
            lock_key TEXT PRIMARY KEY,
            lock_kind TEXT NOT NULL,
            runtime_profile_id TEXT,
            driver_id TEXT,
            run_id TEXT NOT NULL,
            lock_owner TEXT NOT NULL,
            lock_expires_at_ms INTEGER NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_cli_runtime_locks_kind
        ON cli_runtime_locks(lock_kind, runtime_profile_id, driver_id);

        CREATE INDEX IF NOT EXISTS idx_cli_runtime_locks_expiry
        ON cli_runtime_locks(lock_expires_at_ms);
        "#,
    )?;
    Ok(())
}

fn migrate_cli_route_sessions_v23(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS cli_route_sessions (
            route_key TEXT PRIMARY KEY,
            route_key_hash TEXT NOT NULL,
            agent_did TEXT NOT NULL,
            runtime_profile_id TEXT NOT NULL,
            driver_id TEXT NOT NULL,
            controller_user_id TEXT NOT NULL DEFAULT '',
            controller_full_handle TEXT NOT NULL DEFAULT '',
            controller_scope_key TEXT NOT NULL DEFAULT '',
            controller_did TEXT NOT NULL,
            conversation_id TEXT NOT NULL,
            workspace_path TEXT NOT NULL,
            session_dir TEXT NOT NULL,
            native_session_id TEXT,
            native_session_source TEXT,
            synthetic_session_id TEXT,
            status TEXT NOT NULL,
            last_run_id TEXT,
            last_message_id TEXT,
            lock_run_id TEXT,
            lock_owner TEXT,
            lock_expires_at_ms INTEGER,
            last_error_code TEXT,
            last_error_summary TEXT,
            version INTEGER NOT NULL DEFAULT 0,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            UNIQUE(runtime_profile_id, route_key_hash)
        );

        CREATE INDEX IF NOT EXISTS idx_cli_route_sessions_profile_status
        ON cli_route_sessions(runtime_profile_id, controller_scope_key, status);

        CREATE INDEX IF NOT EXISTS idx_cli_route_sessions_lock
        ON cli_route_sessions(status, lock_expires_at_ms);
        "#,
    )?;
    Ok(())
}

fn migrate_user_delegated_identity_v16(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS user_delegated_identity (
            verification_method TEXT PRIMARY KEY,
            user_did TEXT NOT NULL,
            app_instance_id TEXT NOT NULL,
            controller_did TEXT NOT NULL,
            daemon_agent_did TEXT NOT NULL,
            public_key_multibase TEXT NOT NULL,
            private_key_material TEXT NOT NULL,
            allowed_scopes_json TEXT NOT NULL,
            status TEXT NOT NULL,
            expires_at TEXT,
            bootstrap_id TEXT NOT NULL,
            idempotency_key TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_user_delegated_identity_user
        ON user_delegated_identity(user_did, app_instance_id, status);

        CREATE TABLE IF NOT EXISTS bootstrap_replay (
            bootstrap_id TEXT PRIMARY KEY,
            idempotency_key TEXT NOT NULL UNIQUE,
            payload_hash TEXT NOT NULL,
            user_did TEXT NOT NULL,
            verification_method TEXT NOT NULL,
            app_instance_id TEXT NOT NULL,
            daemon_agent_did TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_bootstrap_replay_identity
        ON bootstrap_replay(user_did, verification_method, app_instance_id);
        "#,
    )?;
    Ok(())
}

fn migrate_app_message_agent_binding_v17(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS app_message_agent_binding (
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

        CREATE INDEX IF NOT EXISTS idx_app_message_agent_binding_active
        ON app_message_agent_binding(user_did, app_instance_id, role, status, revoked_at_ms);

        CREATE UNIQUE INDEX IF NOT EXISTS ux_app_message_agent_binding_active_role
        ON app_message_agent_binding(user_did, app_instance_id, role)
        WHERE revoked_at_ms IS NULL
          AND status IN ('message_agent_ready', 'message_agent_active', 'message_agent_ensuring');
        "#,
    )?;
    Ok(())
}

fn migrate_secure_bootstrap_replay_v22(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS secure_bootstrap_replay (
            operation_id TEXT PRIMARY KEY,
            nonce TEXT NOT NULL UNIQUE,
            envelope_hash TEXT NOT NULL,
            recipient_daemon_did TEXT NOT NULL,
            recipient_key_id TEXT NOT NULL,
            sender_human_did TEXT NOT NULL,
            bootstrap_id TEXT NOT NULL,
            idempotency_key TEXT NOT NULL,
            payload_sha256 TEXT,
            expires_at TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_secure_bootstrap_replay_sender
        ON secure_bootstrap_replay(sender_human_did, recipient_daemon_did, status);
        "#,
    )?;
    Ok(())
}

fn migrate_user_delegated_inbox_sync_v18(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS inbox_cursor (
            owner_did TEXT NOT NULL,
            inbox_scope TEXT NOT NULL,
            cursor TEXT,
            updated_at_ms INTEGER NOT NULL,
            PRIMARY KEY (owner_did, inbox_scope)
        );

        CREATE TABLE IF NOT EXISTS processed_message (
            owner_did TEXT NOT NULL,
            message_id TEXT NOT NULL,
            schema TEXT NOT NULL,
            processed_at_ms INTEGER NOT NULL,
            status TEXT NOT NULL,
            PRIMARY KEY (owner_did, message_id)
        );

        CREATE INDEX IF NOT EXISTS idx_processed_message_status
        ON processed_message(owner_did, status, processed_at_ms);

        CREATE TABLE IF NOT EXISTS message_event (
            event_id TEXT PRIMARY KEY,
            owner_did TEXT NOT NULL,
            conversation_id TEXT,
            message_id TEXT NOT NULL,
            message_kind TEXT NOT NULL,
            sender_did TEXT NOT NULL,
            received_at TEXT,
            plain_text_ref_or_excerpt TEXT,
            content_hash TEXT NOT NULL,
            schema TEXT NOT NULL,
            processing_status TEXT NOT NULL,
            retention_class TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_message_event_owner_message
        ON message_event(owner_did, message_id);

        CREATE INDEX IF NOT EXISTS idx_message_event_processing
        ON message_event(owner_did, processing_status, created_at_ms);

        CREATE TABLE IF NOT EXISTS message_sync_outbox (
            idempotency_key TEXT PRIMARY KEY,
            owner_did TEXT NOT NULL,
            app_instance_id TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            status TEXT NOT NULL,
            attempt_count INTEGER NOT NULL DEFAULT 0,
            next_attempt_at_ms INTEGER NOT NULL DEFAULT 0,
            last_error_code TEXT,
            last_error_summary TEXT,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            sent_at_ms INTEGER
        );

        CREATE INDEX IF NOT EXISTS idx_message_sync_outbox_due
        ON message_sync_outbox(status, next_attempt_at_ms, created_at_ms);
        "#,
    )?;
    Ok(())
}

fn migrate_control_command_state_v20(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS control_command_state (
            daemon_agent_did TEXT NOT NULL,
            controller_scope_key TEXT NOT NULL,
            command_id TEXT NOT NULL,
            command TEXT NOT NULL,
            message_id TEXT NOT NULL,
            status TEXT NOT NULL,
            target_version TEXT,
            result_json TEXT NOT NULL DEFAULT '{}',
            error_summary TEXT,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            PRIMARY KEY (daemon_agent_did, controller_scope_key, command_id)
        );

        CREATE INDEX IF NOT EXISTS idx_control_command_state_message
        ON control_command_state(daemon_agent_did, message_id);
        "#,
    )?;
    Ok(())
}

fn migrate_runtime_requester_contract_v21(connection: &Connection) -> Result<()> {
    for (table, column, definition) in [
        ("runtime_task", "requester_did", "TEXT NOT NULL DEFAULT ''"),
        ("runtime_task", "requester_full_handle", "TEXT"),
        (
            "runtime_task",
            "trigger_kind",
            "TEXT NOT NULL DEFAULT 'controller_direct'",
        ),
        (
            "runtime_task",
            "reply_recipient_did",
            "TEXT NOT NULL DEFAULT ''",
        ),
        (
            "runtime_final_outbox",
            "recipient_did",
            "TEXT NOT NULL DEFAULT ''",
        ),
        (
            "hermes_native_sessions",
            "session_actor_did",
            "TEXT NOT NULL DEFAULT ''",
        ),
    ] {
        add_column_if_missing(connection, table, column, definition)?;
    }

    connection.execute_batch(
        r#"
        UPDATE runtime_task
        SET requester_did = CASE WHEN requester_did = '' THEN sender_did ELSE requester_did END,
            reply_recipient_did = CASE WHEN reply_recipient_did = '' THEN sender_did ELSE reply_recipient_did END,
            trigger_kind = CASE
                WHEN trigger_kind = '' THEN
                    CASE
                        WHEN conversation_id LIKE 'group:%' THEN 'group_mention'
                        WHEN sender_did = controller_did THEN 'controller_direct'
                        ELSE 'external_direct'
                    END
                ELSE trigger_kind
            END
        WHERE sender_did <> '';

        UPDATE runtime_final_outbox
        SET recipient_did = controller_did
        WHERE recipient_did = ''
          AND controller_did <> '';

        UPDATE hermes_native_sessions
        SET session_actor_did = controller_did
        WHERE session_actor_did = ''
          AND controller_did <> '';
        "#,
    )?;
    Ok(())
}

fn migrate_runtime_retry_queue_due_v26(connection: &Connection) -> Result<()> {
    add_column_if_missing(
        connection,
        "runtime_retry_queue",
        "next_attempt_at_ms",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    connection.execute_batch(
        r#"
        CREATE INDEX IF NOT EXISTS idx_runtime_retry_queue_due
        ON runtime_retry_queue(status, next_attempt_at_ms, created_at_ms, retry_id);
        "#,
    )?;
    Ok(())
}

fn migrate_cli_route_message_queue_v27(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS cli_route_message_queue (
            queue_id TEXT PRIMARY KEY,
            agent_did TEXT NOT NULL,
            runtime_profile_id TEXT NOT NULL,
            driver_id TEXT NOT NULL,
            controller_user_id TEXT NOT NULL DEFAULT '',
            controller_full_handle TEXT NOT NULL DEFAULT '',
            controller_scope_key TEXT NOT NULL DEFAULT '',
            controller_did TEXT NOT NULL,
            conversation_id TEXT NOT NULL,
            route_key TEXT NOT NULL,
            route_key_hash TEXT NOT NULL,
            source_message_id TEXT NOT NULL,
            task_id TEXT,
            run_id TEXT,
            status TEXT NOT NULL,
            enqueue_reason TEXT NOT NULL,
            attempts INTEGER NOT NULL DEFAULT 0,
            next_attempt_at_ms INTEGER NOT NULL DEFAULT 0,
            route_sequence INTEGER NOT NULL,
            last_error_code TEXT,
            last_error_summary TEXT,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            UNIQUE(runtime_profile_id, route_key, source_message_id)
        );

        CREATE INDEX IF NOT EXISTS idx_cli_route_message_queue_due
        ON cli_route_message_queue(status, next_attempt_at_ms, created_at_ms, queue_id);

        CREATE INDEX IF NOT EXISTS idx_cli_route_message_queue_route_order
        ON cli_route_message_queue(runtime_profile_id, route_key, route_sequence);
        "#,
    )?;
    Ok(())
}

fn migrate_runtime_scope_authority_v29(connection: &Connection) -> Result<()> {
    for (table, column, definition) in [
        ("runtime_task", "agent_handle", "TEXT NOT NULL DEFAULT ''"),
        ("runtime_task", "requester_user_id", "TEXT"),
        (
            "runtime_task",
            "conversation_scope_kind",
            "TEXT NOT NULL DEFAULT 'controller_private'",
        ),
        (
            "runtime_task",
            "conversation_scope_key",
            "TEXT NOT NULL DEFAULT ''",
        ),
        (
            "runtime_task",
            "invocation_authority",
            "TEXT NOT NULL DEFAULT 'controller'",
        ),
        (
            "hermes_native_sessions",
            "agent_handle",
            "TEXT NOT NULL DEFAULT ''",
        ),
        (
            "hermes_native_sessions",
            "scope_kind",
            "TEXT NOT NULL DEFAULT 'controller_private'",
        ),
        (
            "hermes_native_sessions",
            "scope_key",
            "TEXT NOT NULL DEFAULT ''",
        ),
    ] {
        add_column_if_missing(connection, table, column, definition)?;
    }

    connection.execute_batch(
        r#"
        UPDATE runtime_task
        SET agent_handle = COALESCE(
            (SELECT handle FROM agent_definition WHERE agent_definition.agent_did = runtime_task.agent_did),
            agent_did
        )
        WHERE agent_handle = '';

        UPDATE runtime_task
        SET requester_user_id = controller_user_id,
            requester_full_handle = CASE
                WHEN requester_full_handle IS NULL OR requester_full_handle = '' THEN controller_full_handle
                ELSE requester_full_handle
            END
        WHERE trigger_kind = 'controller_direct'
          AND (requester_user_id IS NULL OR requester_user_id = '');

        UPDATE runtime_task
        SET conversation_scope_kind = CASE
                WHEN trigger_kind = 'group_mention' THEN 'group_visible'
                WHEN trigger_kind = 'controller_direct' THEN 'controller_private'
                ELSE 'direct'
            END,
            conversation_scope_key = CASE
                WHEN trigger_kind = 'group_mention'
                    THEN 'group:' || COALESCE(NULLIF(substr(conversation_id, length('group:') + 1), ''), conversation_id, 'unknown')
                WHEN trigger_kind = 'controller_direct'
                    THEN 'controller:' || controller_scope_key
                ELSE 'user:' || COALESCE(NULLIF(requester_user_id, ''), 'unknown') || ':handle:' || COALESCE(NULLIF(requester_full_handle, ''), 'unknown')
            END,
            invocation_authority = CASE
                WHEN trigger_kind = 'controller_direct' THEN 'controller'
                ELSE 'requester'
            END
        WHERE conversation_scope_key = '';

        UPDATE hermes_native_sessions
        SET agent_handle = COALESCE(
            (SELECT handle FROM agent_definition WHERE agent_definition.agent_did = hermes_native_sessions.agent_did),
            agent_did
        )
        WHERE agent_handle = '';

        UPDATE hermes_native_sessions
        SET scope_kind = CASE
                WHEN conversation_id LIKE 'group:%' THEN 'group_visible'
                ELSE 'controller_private'
            END,
            scope_key = CASE
                WHEN conversation_id LIKE 'group:%' THEN 'group:' || substr(conversation_id, length('group:') + 1)
                ELSE 'controller:' || controller_scope_key
            END
        WHERE scope_key = '';
        "#,
    )?;
    Ok(())
}

fn migrate_hermes_native_session_stored_ids_v30(connection: &Connection) -> Result<()> {
    add_column_if_missing(
        connection,
        "hermes_native_sessions",
        "stored_session_id",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    add_column_if_missing(
        connection,
        "hermes_native_sessions",
        "last_live_session_id",
        "TEXT",
    )?;
    connection.execute_batch(
        r#"
        UPDATE hermes_native_sessions
        SET stored_session_id = hermes_session_id
        WHERE stored_session_id = ''
          AND hermes_session_id <> '';
        "#,
    )?;
    Ok(())
}

fn migrate_runtime_retry_queue_v12(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS runtime_retry_queue (
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
            next_attempt_at_ms INTEGER NOT NULL DEFAULT 0,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_runtime_retry_queue_status
        ON runtime_retry_queue(status, created_at_ms);

        "#,
    )?;
    Ok(())
}

fn migrate_runtime_agent_create_request_v13(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS runtime_agent_create_request (
            daemon_agent_did TEXT NOT NULL,
            controller_did TEXT NOT NULL,
            client_request_id TEXT NOT NULL,
            runtime_agent_did TEXT NOT NULL,
            command_id TEXT NOT NULL,
            outcome_json TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            PRIMARY KEY (daemon_agent_did, controller_did, client_request_id)
        );

        CREATE INDEX IF NOT EXISTS idx_runtime_agent_create_request_runtime
        ON runtime_agent_create_request(runtime_agent_did);
        "#,
    )?;
    Ok(())
}

fn migrate_runtime_final_outbox_v14(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS runtime_final_outbox (
            idempotency_key TEXT PRIMARY KEY,
            run_id TEXT NOT NULL UNIQUE,
            agent_did TEXT NOT NULL,
            runtime_profile_id TEXT NOT NULL,
            controller_did TEXT NOT NULL,
            conversation_id TEXT,
            final_text TEXT NOT NULL,
            final_source TEXT NOT NULL DEFAULT 'unknown_legacy',
            final_body_hash TEXT NOT NULL DEFAULT '',
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

        CREATE INDEX IF NOT EXISTS idx_runtime_final_outbox_due
        ON runtime_final_outbox(status, next_attempt_at_ms, created_at_ms);
        "#,
    )?;
    Ok(())
}

fn migrate_runtime_final_outbox_provenance_v28(connection: &Connection) -> Result<()> {
    add_column_if_missing(
        connection,
        "runtime_final_outbox",
        "final_source",
        "TEXT NOT NULL DEFAULT 'unknown_legacy'",
    )?;
    add_column_if_missing(
        connection,
        "runtime_final_outbox",
        "final_body_hash",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    Ok(())
}

fn migrate_runtime_final_plain_delivery_v15(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        r#"
        UPDATE runtime_final_outbox
        SET security = 'default_plain',
            status = CASE WHEN status = 'sent' THEN status ELSE 'pending' END,
            attempt_count = CASE WHEN status = 'sent' THEN attempt_count ELSE 0 END,
            next_attempt_at_ms = CASE WHEN status = 'sent' THEN next_attempt_at_ms ELSE 0 END,
            last_error_code = CASE WHEN status = 'sent' THEN last_error_code ELSE NULL END,
            last_error_summary = CASE WHEN status = 'sent' THEN last_error_summary ELSE NULL END,
            updated_at_ms = strftime('%s','now') * 1000
        WHERE security = 'direct_e2ee'
          AND status != 'sent';
        "#,
    )?;
    Ok(())
}

fn migrate_controller_scope_v19(connection: &Connection) -> Result<()> {
    for (table, column, definition) in [
        (
            "agent_definition",
            "controller_user_id",
            "TEXT NOT NULL DEFAULT ''",
        ),
        (
            "agent_definition",
            "controller_full_handle",
            "TEXT NOT NULL DEFAULT ''",
        ),
        (
            "agent_definition",
            "controller_scope_key",
            "TEXT NOT NULL DEFAULT ''",
        ),
        (
            "runtime_task",
            "controller_user_id",
            "TEXT NOT NULL DEFAULT ''",
        ),
        (
            "runtime_task",
            "controller_full_handle",
            "TEXT NOT NULL DEFAULT ''",
        ),
        (
            "runtime_task",
            "controller_scope_key",
            "TEXT NOT NULL DEFAULT ''",
        ),
        (
            "hermes_native_sessions",
            "controller_scope_key",
            "TEXT NOT NULL DEFAULT ''",
        ),
        (
            "runtime_daemon_binding",
            "controller_user_id",
            "TEXT NOT NULL DEFAULT ''",
        ),
        (
            "runtime_daemon_binding",
            "controller_full_handle",
            "TEXT NOT NULL DEFAULT ''",
        ),
        (
            "runtime_daemon_binding",
            "controller_scope_key",
            "TEXT NOT NULL DEFAULT ''",
        ),
        (
            "runtime_final_outbox",
            "controller_scope_key",
            "TEXT NOT NULL DEFAULT ''",
        ),
        (
            "cli_driver_run",
            "controller_user_id",
            "TEXT NOT NULL DEFAULT ''",
        ),
        (
            "cli_driver_run",
            "controller_full_handle",
            "TEXT NOT NULL DEFAULT ''",
        ),
        (
            "cli_driver_run",
            "controller_scope_key",
            "TEXT NOT NULL DEFAULT ''",
        ),
    ] {
        add_column_if_missing(connection, table, column, definition)?;
    }

    connection.execute_batch(
        r#"
        UPDATE runtime_task
        SET
            controller_user_id = CASE WHEN controller_user_id = '' THEN COALESCE((SELECT controller_user_id FROM agent_definition WHERE agent_definition.agent_did = runtime_task.agent_did), '') ELSE controller_user_id END,
            controller_full_handle = CASE WHEN controller_full_handle = '' THEN COALESCE((SELECT controller_full_handle FROM agent_definition WHERE agent_definition.agent_did = runtime_task.agent_did), '') ELSE controller_full_handle END,
            controller_scope_key = CASE WHEN controller_scope_key = '' THEN COALESCE((SELECT controller_scope_key FROM agent_definition WHERE agent_definition.agent_did = runtime_task.agent_did), '') ELSE controller_scope_key END;

        UPDATE runtime_daemon_binding
        SET
            controller_user_id = CASE WHEN controller_user_id = '' THEN COALESCE((SELECT controller_user_id FROM agent_definition WHERE agent_definition.agent_did = runtime_daemon_binding.daemon_agent_did), '') ELSE controller_user_id END,
            controller_full_handle = CASE WHEN controller_full_handle = '' THEN COALESCE((SELECT controller_full_handle FROM agent_definition WHERE agent_definition.agent_did = runtime_daemon_binding.daemon_agent_did), '') ELSE controller_full_handle END,
            controller_scope_key = CASE WHEN controller_scope_key = '' THEN COALESCE((SELECT controller_scope_key FROM agent_definition WHERE agent_definition.agent_did = runtime_daemon_binding.daemon_agent_did), '') ELSE controller_scope_key END;

        UPDATE hermes_native_sessions
        SET controller_scope_key = COALESCE(
            (SELECT controller_scope_key FROM agent_definition WHERE agent_definition.agent_did = hermes_native_sessions.agent_did),
            ''
        )
        WHERE controller_scope_key = '';

        UPDATE runtime_final_outbox
        SET controller_scope_key = COALESCE(
            (SELECT controller_scope_key FROM agent_definition WHERE agent_definition.agent_did = runtime_final_outbox.agent_did),
            ''
        )
        WHERE controller_scope_key = '';

        UPDATE cli_driver_run
        SET
            controller_user_id = CASE WHEN controller_user_id = '' THEN COALESCE((SELECT controller_user_id FROM agent_definition WHERE agent_definition.agent_did = cli_driver_run.agent_did), '') ELSE controller_user_id END,
            controller_full_handle = CASE WHEN controller_full_handle = '' THEN COALESCE((SELECT controller_full_handle FROM agent_definition WHERE agent_definition.agent_did = cli_driver_run.agent_did), '') ELSE controller_full_handle END,
            controller_scope_key = CASE WHEN controller_scope_key = '' THEN COALESCE((SELECT controller_scope_key FROM agent_definition WHERE agent_definition.agent_did = cli_driver_run.agent_did), '') ELSE controller_scope_key END;
        "#,
    )?;

    rebuild_runtime_agent_create_request_for_scope(connection)?;
    connection.execute_batch(
        r#"
        CREATE INDEX IF NOT EXISTS idx_runtime_daemon_binding_daemon_scope
        ON runtime_daemon_binding(daemon_agent_did, controller_scope_key);
        "#,
    )?;
    Ok(())
}

fn rebuild_runtime_agent_create_request_for_scope(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS runtime_agent_create_request_v19 (
            daemon_agent_did TEXT NOT NULL,
            controller_scope_key TEXT NOT NULL DEFAULT '',
            controller_did TEXT NOT NULL,
            client_request_id TEXT NOT NULL,
            runtime_agent_did TEXT NOT NULL,
            command_id TEXT NOT NULL,
            outcome_json TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            PRIMARY KEY (daemon_agent_did, controller_scope_key, client_request_id)
        );

        INSERT OR IGNORE INTO runtime_agent_create_request_v19 (
            daemon_agent_did,
            controller_scope_key,
            controller_did,
            client_request_id,
            runtime_agent_did,
            command_id,
            outcome_json,
            created_at_ms,
            updated_at_ms
        )
        SELECT
            daemon_agent_did,
            COALESCE(
                (SELECT controller_scope_key FROM agent_definition WHERE agent_definition.agent_did = runtime_agent_create_request.daemon_agent_did),
                ''
            ),
            controller_did,
            client_request_id,
            runtime_agent_did,
            command_id,
            outcome_json,
            created_at_ms,
            updated_at_ms
        FROM runtime_agent_create_request;

        DROP TABLE runtime_agent_create_request;
        ALTER TABLE runtime_agent_create_request_v19 RENAME TO runtime_agent_create_request;

        CREATE INDEX IF NOT EXISTS idx_runtime_agent_create_request_runtime
        ON runtime_agent_create_request(runtime_agent_did);
        "#,
    )?;
    Ok(())
}

fn migrate_agent_management_v11(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS runtime_daemon_binding (
            runtime_agent_did TEXT PRIMARY KEY,
            daemon_agent_did TEXT NOT NULL,
            controller_did TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_runtime_daemon_binding_daemon
        ON runtime_daemon_binding(daemon_agent_did, controller_did);

        CREATE TABLE IF NOT EXISTS agent_status_query_throttle (
            daemon_agent_did TEXT NOT NULL,
            controller_did TEXT NOT NULL,
            last_snapshot_at_ms INTEGER NOT NULL,
            PRIMARY KEY (daemon_agent_did, controller_did)
        );

        INSERT OR IGNORE INTO runtime_daemon_binding (
            runtime_agent_did,
            daemon_agent_did,
            controller_did,
            created_at_ms,
            updated_at_ms
        )
        SELECT
            runtime.agent_did,
            daemon.agent_did,
            runtime.controller_did,
            0,
            0
        FROM agent_definition AS runtime
        INNER JOIN agent_definition AS daemon
            ON daemon.agent_kind = 'daemon'
           AND daemon.controller_did = runtime.controller_did
        WHERE runtime.agent_kind = 'runtime'
          AND (
              SELECT COUNT(*)
              FROM agent_definition AS daemon_count
              WHERE daemon_count.agent_kind = 'daemon'
                AND daemon_count.controller_did = runtime.controller_did
          ) = 1;
        "#,
    )?;
    Ok(())
}

fn migrate_cli_driver_run_v10(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS cli_driver_run (
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
        "#,
    )?;
    for (column, definition) in [
        ("agent_did", "TEXT NOT NULL DEFAULT ''"),
        ("runtime_profile_id", "TEXT NOT NULL DEFAULT ''"),
        ("driver_id", "TEXT NOT NULL DEFAULT ''"),
        ("controller_did", "TEXT NOT NULL DEFAULT ''"),
        ("conversation_id", "TEXT"),
        ("route_key", "TEXT NOT NULL DEFAULT ''"),
        ("workspace_id", "TEXT"),
        ("workspace_root", "TEXT"),
        ("workspace_instance_path", "TEXT"),
        ("workspace_mode", "TEXT"),
        ("is_security_boundary", "INTEGER NOT NULL DEFAULT 0"),
        ("command_json", "TEXT NOT NULL DEFAULT '{}'"),
        ("output_json", "TEXT NOT NULL DEFAULT '{}'"),
        ("final_output_path", "TEXT"),
        ("native_session_id", "TEXT"),
        ("synthetic_session_id", "TEXT"),
        ("status", "TEXT NOT NULL DEFAULT 'created'"),
        ("fallback_final_source", "TEXT"),
        ("created_at_ms", "INTEGER NOT NULL DEFAULT 0"),
        ("updated_at_ms", "INTEGER NOT NULL DEFAULT 0"),
    ] {
        add_column_if_missing(connection, "cli_driver_run", column, definition)?;
    }
    Ok(())
}

fn migrate_runtime_rpc_tokens_v9(connection: &Connection) -> Result<()> {
    add_column_if_missing(
        connection,
        "runtime_rpc_tokens",
        "allowed_message_security_json",
        "TEXT",
    )?;
    Ok(())
}

fn migrate_cli_runtime_profile_v8(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS cli_runtime_profile (
            runtime_profile_id TEXT PRIMARY KEY,
            driver_id TEXT NOT NULL,
            binary_path TEXT,
            config_home TEXT,
            auth_mode TEXT,
            default_model TEXT,
            default_sandbox TEXT,
            default_workspace_mode TEXT NOT NULL,
            recipient_policy_json TEXT NOT NULL,
            driver_config_json TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );
        "#,
    )?;
    for (column, definition) in [
        ("driver_id", "TEXT NOT NULL DEFAULT ''"),
        ("binary_path", "TEXT"),
        ("config_home", "TEXT"),
        ("auth_mode", "TEXT"),
        ("default_model", "TEXT"),
        ("default_sandbox", "TEXT"),
        (
            "default_workspace_mode",
            "TEXT NOT NULL DEFAULT 'shared-root'",
        ),
        (
            "recipient_policy_json",
            "TEXT NOT NULL DEFAULT '{\"mode\":\"controller-only\"}'",
        ),
        ("driver_config_json", "TEXT NOT NULL DEFAULT '{}'"),
        ("status", "TEXT NOT NULL DEFAULT 'active'"),
        ("created_at_ms", "INTEGER NOT NULL DEFAULT 0"),
        ("updated_at_ms", "INTEGER NOT NULL DEFAULT 0"),
    ] {
        add_column_if_missing(connection, "cli_runtime_profile", column, definition)?;
    }
    migrate_legacy_cli_runtime_profiles(connection)?;
    Ok(())
}

fn migrate_legacy_cli_runtime_profiles(connection: &Connection) -> Result<()> {
    for (legacy_plugin_id, driver_id) in [
        ("runtime.cli.codex", "codex"),
        ("runtime.cli.claude-code", "claude-code"),
        ("runtime.cli.gemini-cli", "gemini"),
    ] {
        connection.execute(
            r#"
INSERT INTO cli_runtime_profile (
    runtime_profile_id,
    driver_id,
    default_workspace_mode,
    recipient_policy_json,
    driver_config_json,
    status,
    created_at_ms,
    updated_at_ms
)
SELECT
    runtime_profile_id,
    ?2,
    'shared-root',
    ?3,
    '{}',
    status,
    0,
    0
FROM runtime_profile
WHERE runtime_plugin_id = ?1
  AND COALESCE(runtime_profile_id, '') <> ''
ON CONFLICT(runtime_profile_id) DO UPDATE SET
    driver_id = excluded.driver_id,
    default_workspace_mode = excluded.default_workspace_mode,
    recipient_policy_json = excluded.recipient_policy_json,
    driver_config_json = excluded.driver_config_json,
    status = excluded.status,
    updated_at_ms = excluded.updated_at_ms
"#,
            rusqlite::params![
                legacy_plugin_id,
                driver_id,
                DEFAULT_CLI_RECIPIENT_POLICY_JSON,
            ],
        )?;
        connection.execute(
            "UPDATE runtime_profile SET runtime_plugin_id = ?1 WHERE runtime_plugin_id = ?2",
            rusqlite::params![GENERIC_CLI_RUNTIME_PLUGIN_ID, legacy_plugin_id],
        )?;
        connection.execute(
            "UPDATE agent_definition SET runtime_plugin_id = ?1 WHERE runtime_plugin_id = ?2",
            rusqlite::params![GENERIC_CLI_RUNTIME_PLUGIN_ID, legacy_plugin_id],
        )?;
    }
    Ok(())
}

fn migrate_agent_auth_state_v5(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS agent_auth_state (
            agent_did TEXT PRIMARY KEY,
            jwt_token TEXT NOT NULL,
            jwt_token_ref_json TEXT,
            updated_at_ms INTEGER NOT NULL
        );
        "#,
    )?;
    add_column_if_missing(connection, "agent_auth_state", "jwt_token_ref_json", "TEXT")?;
    Ok(())
}

fn migrate_hermes_profiles_v6(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS hermes_profiles (
            agent_did TEXT PRIMARY KEY,
            runtime_profile_id TEXT NOT NULL,
            hermes_profile TEXT NOT NULL,
            hermes_home TEXT NOT NULL,
            hermes_version TEXT,
            awiki_skills_version TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );
        "#,
    )?;
    for (column, definition) in [
        ("runtime_profile_id", "TEXT NOT NULL DEFAULT ''"),
        ("hermes_profile", "TEXT NOT NULL DEFAULT ''"),
        ("hermes_home", "TEXT NOT NULL DEFAULT ''"),
        ("hermes_version", "TEXT"),
        ("awiki_skills_version", "TEXT NOT NULL DEFAULT ''"),
        ("status", "TEXT NOT NULL DEFAULT 'unknown'"),
        ("created_at_ms", "INTEGER NOT NULL DEFAULT 0"),
        ("updated_at_ms", "INTEGER NOT NULL DEFAULT 0"),
    ] {
        add_column_if_missing(connection, "hermes_profiles", column, definition)?;
    }
    Ok(())
}

fn migrate_hermes_native_sessions_v7(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS hermes_native_sessions (
            id TEXT PRIMARY KEY,
            runtime_session_id TEXT NOT NULL,
            agent_did TEXT NOT NULL,
            runtime_profile_id TEXT NOT NULL,
            controller_did TEXT NOT NULL,
            conversation_id TEXT,
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

        CREATE UNIQUE INDEX IF NOT EXISTS idx_hermes_native_sessions_active_route
        ON hermes_native_sessions(route_key)
        WHERE status = 'active';
        "#,
    )?;
    for (column, definition) in [
        ("runtime_session_id", "TEXT NOT NULL DEFAULT ''"),
        ("agent_did", "TEXT NOT NULL DEFAULT ''"),
        ("runtime_profile_id", "TEXT NOT NULL DEFAULT ''"),
        ("controller_did", "TEXT NOT NULL DEFAULT ''"),
        ("conversation_id", "TEXT"),
        ("route_key", "TEXT NOT NULL DEFAULT ''"),
        ("hermes_profile", "TEXT NOT NULL DEFAULT ''"),
        ("hermes_session_id", "TEXT NOT NULL DEFAULT ''"),
        ("stored_session_id", "TEXT NOT NULL DEFAULT ''"),
        ("last_live_session_id", "TEXT"),
        ("session_kind", "TEXT NOT NULL DEFAULT 'conversation'"),
        ("status", "TEXT NOT NULL DEFAULT 'active'"),
        ("created_at_ms", "INTEGER NOT NULL DEFAULT 0"),
        ("updated_at_ms", "INTEGER NOT NULL DEFAULT 0"),
    ] {
        add_column_if_missing(connection, "hermes_native_sessions", column, definition)?;
    }
    connection.execute_batch(
        r#"
        UPDATE hermes_native_sessions
        SET stored_session_id = hermes_session_id
        WHERE stored_session_id = ''
          AND hermes_session_id <> '';
        "#,
    )?;
    Ok(())
}

fn migrate_agent_definition_v4(connection: &Connection) -> Result<()> {
    for (column, definition) in [
        ("handle", "TEXT NOT NULL DEFAULT ''"),
        ("agent_kind", "TEXT NOT NULL DEFAULT 'runtime'"),
        ("runtime_plugin_id", "TEXT"),
        ("workspace_id", "TEXT"),
        ("policy_id", "TEXT NOT NULL DEFAULT 'default'"),
        ("local_agent_db_path", "TEXT NOT NULL DEFAULT ''"),
        ("message_db_path", "TEXT NOT NULL DEFAULT ''"),
    ] {
        add_column_if_missing(connection, "agent_definition", column, definition)?;
    }
    connection.execute_batch(
        r#"
        UPDATE agent_definition
        SET handle = agent_did
        WHERE handle = '';

        UPDATE agent_definition
        SET agent_kind = 'runtime'
        WHERE agent_kind = '';

        UPDATE agent_definition
        SET policy_id = 'default'
        WHERE policy_id = '';

        UPDATE agent_definition
        SET runtime_plugin_id = (
            SELECT runtime_profile.runtime_plugin_id
            FROM runtime_profile
            WHERE runtime_profile.runtime_profile_id = agent_definition.runtime_profile_id
            LIMIT 1
        )
        WHERE runtime_plugin_id IS NULL
          AND runtime_profile_id IS NOT NULL;

        UPDATE agent_definition
        SET local_agent_db_path = 'agents/' || replace(replace(replace(agent_did, ':', '_'), '/', '_'), '#', '_') || '/agent.db'
        WHERE local_agent_db_path = '';

        UPDATE agent_definition
        SET message_db_path = 'agents/' || replace(replace(replace(agent_did, ':', '_'), '/', '_'), '#', '_') || '/messages.db'
        WHERE message_db_path = '';
        "#,
    )?;
    Ok(())
}

fn migrate_runtime_run_v3(connection: &Connection) -> Result<()> {
    for (column, definition) in [
        ("task_id", "TEXT DEFAULT ''"),
        ("started_at_ms", "INTEGER NOT NULL DEFAULT 0"),
        ("completed_at_ms", "INTEGER"),
        ("updated_at_ms", "INTEGER NOT NULL DEFAULT 0"),
    ] {
        add_column_if_missing(connection, "runtime_run", column, definition)?;
    }
    Ok(())
}

fn migrate_runtime_task_v3(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS runtime_task (
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
        "#,
    )?;
    Ok(())
}

fn migrate_runtime_rpc_tokens_v2(connection: &Connection) -> Result<()> {
    for (column, definition) in [
        ("token_secret_hash", "TEXT NOT NULL DEFAULT ''"),
        ("expires_at_ms", "INTEGER NOT NULL DEFAULT 0"),
        ("single_use", "INTEGER NOT NULL DEFAULT 0"),
        ("revoked_at_ms", "INTEGER"),
        ("used_at_ms", "INTEGER"),
        ("created_at_ms", "INTEGER NOT NULL DEFAULT 0"),
    ] {
        add_column_if_missing(connection, "runtime_rpc_tokens", column, definition)?;
    }
    Ok(())
}

fn migrate_audit_log_v2(connection: &Connection) -> Result<()> {
    add_column_if_missing(
        connection,
        "audit_log",
        "created_at_ms",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    Ok(())
}

fn add_column_if_missing(
    connection: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<()> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
    for existing in columns {
        if existing? == column {
            return Ok(());
        }
    }
    connection.execute(
        &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
        [],
    )?;
    Ok(())
}
