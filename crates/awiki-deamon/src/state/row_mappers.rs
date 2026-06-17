use super::*;

pub(super) fn cli_runtime_profile_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<CliRuntimeProfileRecord> {
    let default_workspace_mode_raw: String = row.get(7)?;
    let default_workspace_mode =
        WorkspaceMode::parse(&default_workspace_mode_raw).map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(
                default_workspace_mode_raw.len(),
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    err.to_string(),
                )),
            )
        })?;
    let recipient_policy_raw: String = row.get(8)?;
    let recipient_policy_json = serde_json::from_str(&recipient_policy_raw).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            recipient_policy_raw.len(),
            rusqlite::types::Type::Text,
            Box::new(err),
        )
    })?;
    let driver_config_raw: String = row.get(9)?;
    let driver_config_json = serde_json::from_str(&driver_config_raw).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            driver_config_raw.len(),
            rusqlite::types::Type::Text,
            Box::new(err),
        )
    })?;
    Ok(CliRuntimeProfileRecord {
        runtime_profile_id: row.get(0)?,
        driver_id: row.get(1)?,
        binary_path: row.get::<_, Option<String>>(2)?.map(PathBuf::from),
        config_home: row.get::<_, Option<String>>(3)?.map(PathBuf::from),
        auth_mode: row.get(4)?,
        default_model: row.get(5)?,
        default_sandbox: row.get(6)?,
        default_workspace_mode,
        recipient_policy_json,
        driver_config_json,
        status: row.get(10)?,
    })
}

pub(super) fn cli_driver_run_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<CliDriverRunRecord> {
    let workspace_mode_raw: Option<String> = row.get(13)?;
    let workspace_mode = match workspace_mode_raw {
        Some(raw) => Some(WorkspaceMode::parse(&raw).map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(
                raw.len(),
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    err.to_string(),
                )),
            )
        })?),
        None => None,
    };
    let command_raw: String = row.get(15)?;
    let command_json = serde_json::from_str(&command_raw).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            command_raw.len(),
            rusqlite::types::Type::Text,
            Box::new(err),
        )
    })?;
    let output_raw: String = row.get(16)?;
    let output_json = serde_json::from_str(&output_raw).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            output_raw.len(),
            rusqlite::types::Type::Text,
            Box::new(err),
        )
    })?;
    Ok(CliDriverRunRecord {
        run_id: row.get(0)?,
        agent_did: row.get(1)?,
        runtime_profile_id: row.get(2)?,
        driver_id: row.get(3)?,
        controller_user_id: row.get(4)?,
        controller_full_handle: row.get(5)?,
        controller_scope_key: row.get(6)?,
        controller_did: row.get(7)?,
        conversation_id: row.get(8)?,
        route_key: row.get(9)?,
        workspace_id: row.get(10)?,
        workspace_root: row.get::<_, Option<String>>(11)?.map(PathBuf::from),
        workspace_instance_path: row.get::<_, Option<String>>(12)?.map(PathBuf::from),
        workspace_mode,
        is_security_boundary: row.get::<_, i64>(14)? != 0,
        command_json,
        output_json,
        final_output_path: row.get::<_, Option<String>>(17)?.map(PathBuf::from),
        native_session_id: row.get(18)?,
        synthetic_session_id: row.get(19)?,
        status: row.get(20)?,
        fallback_final_source: row.get(21)?,
    })
}

pub(super) fn agent_definition_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<AgentDefinition> {
    let kind_raw: String = row.get(2)?;
    let agent_kind = AgentKind::parse(&kind_raw).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            kind_raw.len(),
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                err.to_string(),
            )),
        )
    })?;
    Ok(AgentDefinition {
        agent_did: row.get(0)?,
        handle: row.get(1)?,
        agent_kind,
        controller_user_id: row.get(3)?,
        controller_full_handle: row.get(4)?,
        controller_scope_key: row.get(5)?,
        controller_did: row.get(6)?,
        runtime_plugin_id: row.get(7)?,
        runtime_profile_id: row.get(8)?,
        workspace_id: row.get(9)?,
        policy_id: row.get(10)?,
        local_agent_db_path: row.get(11)?,
        message_db_path: row.get(12)?,
        status: row.get(13)?,
    })
}

pub(super) fn hermes_native_session_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<HermesNativeSessionRecord> {
    Ok(HermesNativeSessionRecord {
        id: row.get(0)?,
        runtime_session_id: row.get(1)?,
        agent_did: row.get(2)?,
        runtime_profile_id: row.get(3)?,
        controller_scope_key: row.get(4)?,
        controller_did: row.get(5)?,
        conversation_id: row.get(6)?,
        route_key: row.get(7)?,
        hermes_profile: row.get(8)?,
        hermes_session_id: row.get(9)?,
        session_kind: row.get(10)?,
        status: row.get(11)?,
        created_at_ms: row.get(12)?,
        updated_at_ms: row.get(13)?,
    })
}

pub(super) fn runtime_retry_queue_record_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<RuntimeRetryQueueRecord> {
    Ok(RuntimeRetryQueueRecord {
        retry_id: row.get(0)?,
        original_run_id: row.get(1)?,
        task_id: row.get(2)?,
        agent_did: row.get(3)?,
        runtime_profile_id: row.get(4)?,
        runtime_plugin_id: row.get(5)?,
        workspace_id: row.get(6)?,
        status: row.get(7)?,
        requested_by_command_id: row.get(8)?,
        attempts: row.get(9)?,
        created_at_ms: row.get(10)?,
        updated_at_ms: row.get(11)?,
    })
}

pub(super) fn runtime_final_outbox_record_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<RuntimeFinalOutboxRecord> {
    Ok(RuntimeFinalOutboxRecord {
        idempotency_key: row.get(0)?,
        run_id: row.get(1)?,
        agent_did: row.get(2)?,
        runtime_profile_id: row.get(3)?,
        controller_scope_key: row.get(4)?,
        controller_did: row.get(5)?,
        conversation_id: row.get(6)?,
        final_text: row.get(7)?,
        security: row.get(8)?,
        status: row.get(9)?,
        attempt_count: row.get(10)?,
        next_attempt_at_ms: row.get(11)?,
        last_error_code: row.get(12)?,
        last_error_summary: row.get(13)?,
        message_id: row.get(14)?,
        created_at_ms: row.get(15)?,
        updated_at_ms: row.get(16)?,
        sent_at_ms: row.get(17)?,
    })
}

pub(super) fn runtime_agent_create_request_record_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<RuntimeAgentCreateRequestRecord> {
    let outcome_json: String = row.get(6)?;
    let outcome_json = serde_json::from_str(&outcome_json).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            outcome_json.len(),
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                err.to_string(),
            )),
        )
    })?;
    Ok(RuntimeAgentCreateRequestRecord {
        daemon_agent_did: row.get(0)?,
        controller_scope_key: row.get(1)?,
        controller_did: row.get(2)?,
        client_request_id: row.get(3)?,
        runtime_agent_did: row.get(4)?,
        command_id: row.get(5)?,
        outcome_json,
        created_at_ms: row.get(7)?,
        updated_at_ms: row.get(8)?,
    })
}

pub(super) fn control_command_state_record_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<ControlCommandStateRecord> {
    let result_json_raw: String = row.get(7)?;
    let result_json = serde_json::from_str(&result_json_raw).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            result_json_raw.len(),
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                err.to_string(),
            )),
        )
    })?;
    Ok(ControlCommandStateRecord {
        daemon_agent_did: row.get(0)?,
        controller_scope_key: row.get(1)?,
        command_id: row.get(2)?,
        command: row.get(3)?,
        message_id: row.get(4)?,
        status: row.get(5)?,
        target_version: row.get(6)?,
        result_json,
        error_summary: row.get(8)?,
        created_at_ms: row.get(9)?,
        updated_at_ms: row.get(10)?,
    })
}

pub(super) fn agent_identity_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<AgentIdentityRecord> {
    let kind_raw: String = row.get(2)?;
    let agent_kind = AgentKind::parse(&kind_raw).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            kind_raw.len(),
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                err.to_string(),
            )),
        )
    })?;
    let did_document_json: String = row.get(3)?;
    let did_document = serde_json::from_str(&did_document_json).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            did_document_json.len(),
            rusqlite::types::Type::Text,
            Box::new(err),
        )
    })?;
    Ok(AgentIdentityRecord {
        agent_did: row.get(0)?,
        handle: row.get(1)?,
        agent_kind,
        did_document,
        endpoint_url: row.get(4)?,
        key_algorithm: row.get(5)?,
        public_key: row.get(6)?,
        auth_private_key_pem: row.get(7)?,
        e2ee_signing_private_key_pem: row.get(8)?,
        e2ee_agreement_private_key_pem: row.get(9)?,
    })
}

pub(super) fn user_delegated_identity_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<UserDelegatedIdentityRecord> {
    let allowed_scopes_json_raw: String = row.get(7)?;
    let allowed_scopes_json = serde_json::from_str(&allowed_scopes_json_raw).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            allowed_scopes_json_raw.len(),
            rusqlite::types::Type::Text,
            Box::new(err),
        )
    })?;
    Ok(UserDelegatedIdentityRecord {
        user_did: row.get(0)?,
        verification_method: row.get(1)?,
        app_instance_id: row.get(2)?,
        controller_did: row.get(3)?,
        daemon_agent_did: row.get(4)?,
        public_key_multibase: row.get(5)?,
        private_key_material: row.get(6)?,
        allowed_scopes_json,
        status: row.get(8)?,
        expires_at: row.get(9)?,
        bootstrap_id: row.get(10)?,
        idempotency_key: row.get(11)?,
        created_at_ms: row.get(12)?,
        updated_at_ms: row.get(13)?,
    })
}

pub(super) fn bootstrap_replay_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<BootstrapReplayRecord> {
    Ok(BootstrapReplayRecord {
        bootstrap_id: row.get(0)?,
        idempotency_key: row.get(1)?,
        payload_hash: row.get(2)?,
        user_did: row.get(3)?,
        verification_method: row.get(4)?,
        app_instance_id: row.get(5)?,
        daemon_agent_did: row.get(6)?,
        status: row.get(7)?,
        created_at_ms: row.get(8)?,
        updated_at_ms: row.get(9)?,
    })
}

pub(super) fn app_message_agent_binding_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<AppMessageAgentBindingRecord> {
    let desired_agent_json_raw: String = row.get(10)?;
    let desired_agent_json = serde_json::from_str(&desired_agent_json_raw).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            desired_agent_json_raw.len(),
            rusqlite::types::Type::Text,
            Box::new(err),
        )
    })?;
    let capability_policy_json_raw: String = row.get(11)?;
    let capability_policy_json =
        serde_json::from_str(&capability_policy_json_raw).map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(
                capability_policy_json_raw.len(),
                rusqlite::types::Type::Text,
                Box::new(err),
            )
        })?;
    Ok(AppMessageAgentBindingRecord {
        binding_id: row.get(0)?,
        user_did: row.get(1)?,
        inbox_auth_verification_method: row.get(2)?,
        app_instance_id: row.get(3)?,
        bootstrap_id: row.get(4)?,
        idempotency_key: row.get(5)?,
        daemon_agent_did: row.get(6)?,
        runtime_agent_did: row.get(7)?,
        runtime_profile_id: row.get(8)?,
        role: row.get(9)?,
        desired_agent_json,
        capability_policy_json,
        status: row.get(12)?,
        created_at_ms: row.get(13)?,
        updated_at_ms: row.get(14)?,
        revoked_at_ms: row.get(15)?,
    })
}

pub(super) fn inbox_cursor_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<InboxCursorRecord> {
    Ok(InboxCursorRecord {
        owner_did: row.get(0)?,
        inbox_scope: row.get(1)?,
        cursor: row.get(2)?,
        updated_at_ms: row.get(3)?,
    })
}

pub(super) fn processed_message_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<ProcessedMessageRecord> {
    Ok(ProcessedMessageRecord {
        owner_did: row.get(0)?,
        message_id: row.get(1)?,
        schema: row.get(2)?,
        processed_at_ms: row.get(3)?,
        status: row.get(4)?,
    })
}

pub(super) fn message_event_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<MessageEventRecord> {
    Ok(MessageEventRecord {
        event_id: row.get(0)?,
        owner_did: row.get(1)?,
        conversation_id: row.get(2)?,
        message_id: row.get(3)?,
        message_kind: row.get(4)?,
        sender_did: row.get(5)?,
        received_at: row.get(6)?,
        plain_text_ref_or_excerpt: row.get(7)?,
        content_hash: row.get(8)?,
        schema: row.get(9)?,
        processing_status: row.get(10)?,
        retention_class: row.get(11)?,
        created_at_ms: row.get(12)?,
        updated_at_ms: row.get(13)?,
    })
}

pub(super) fn message_sync_outbox_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<MessageSyncOutboxRecord> {
    let payload_json_raw: String = row.get(3)?;
    let payload_json = serde_json::from_str(&payload_json_raw).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            payload_json_raw.len(),
            rusqlite::types::Type::Text,
            Box::new(err),
        )
    })?;
    Ok(MessageSyncOutboxRecord {
        idempotency_key: row.get(0)?,
        owner_did: row.get(1)?,
        app_instance_id: row.get(2)?,
        payload_json,
        status: row.get(4)?,
        attempt_count: row.get(5)?,
        next_attempt_at_ms: row.get(6)?,
        last_error_code: row.get(7)?,
        last_error_summary: row.get(8)?,
        created_at_ms: row.get(9)?,
        updated_at_ms: row.get(10)?,
        sent_at_ms: row.get(11)?,
    })
}

pub(super) fn load_bootstrap_replay_by_id_or_key(
    connection: &Connection,
    bootstrap_id: &str,
    idempotency_key: &str,
) -> Result<Option<BootstrapReplayRecord>> {
    connection
        .query_row(
            r#"
SELECT
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
FROM bootstrap_replay
WHERE bootstrap_id = ?1 OR idempotency_key = ?2
ORDER BY created_at_ms ASC
LIMIT 1
"#,
            rusqlite::params![bootstrap_id, idempotency_key],
            bootstrap_replay_from_row,
        )
        .optional()
        .context("load bootstrap replay by id or idempotency key")
}
