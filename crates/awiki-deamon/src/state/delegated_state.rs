use super::row_mappers::*;
use super::*;

impl DaemonState {
    pub fn store_agent_identity(&self, identity: &AgentIdentityRecord) -> Result<()> {
        if identity.agent_did.trim().is_empty() {
            bail!("agent_did must not be empty");
        }
        if identity.handle.trim().is_empty() {
            bail!("handle must not be empty");
        }
        let connection = self.connection()?;
        let now = current_time_millis()?.to_string();
        connection.execute(
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
ON CONFLICT(agent_did) DO UPDATE SET
    handle = excluded.handle,
    agent_kind = excluded.agent_kind,
    did_document_json = excluded.did_document_json,
    endpoint_url = excluded.endpoint_url,
    key_algorithm = excluded.key_algorithm,
    public_key = excluded.public_key,
    auth_private_key_pem = excluded.auth_private_key_pem,
    e2ee_signing_private_key_pem = excluded.e2ee_signing_private_key_pem,
    e2ee_agreement_private_key_pem = excluded.e2ee_agreement_private_key_pem,
    updated_at = excluded.updated_at
"#,
            rusqlite::params![
                identity.agent_did,
                identity.handle,
                identity.agent_kind.as_str(),
                identity.did_document.to_string(),
                identity.endpoint_url,
                identity.key_algorithm,
                identity.public_key,
                identity.auth_private_key_pem,
                identity.e2ee_signing_private_key_pem,
                identity.e2ee_agreement_private_key_pem,
                now,
            ],
        )?;
        Ok(())
    }

    pub fn load_agent_identity(&self, agent_did: &str) -> Result<AgentIdentityRecord> {
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
SELECT
    agent_did,
    handle,
    agent_kind,
    did_document_json,
    endpoint_url,
    key_algorithm,
    public_key,
    auth_private_key_pem,
    e2ee_signing_private_key_pem,
    e2ee_agreement_private_key_pem
FROM agent_identity
WHERE agent_did = ?1
"#,
                [agent_did],
                agent_identity_from_row,
            )
            .with_context(|| format!("load agent identity {agent_did}"))
    }

    pub fn store_bootstrap_state(
        &self,
        identity: &UserDelegatedIdentityRecord,
        replay: &BootstrapReplayRecord,
    ) -> Result<BootstrapStoreOutcome> {
        identity.validate()?;
        replay.validate()?;
        if identity.user_did != replay.user_did
            || identity.verification_method != replay.verification_method
            || identity.app_instance_id != replay.app_instance_id
            || identity.daemon_agent_did != replay.daemon_agent_did
            || identity.bootstrap_id != replay.bootstrap_id
            || identity.idempotency_key != replay.idempotency_key
            || identity.status != replay.status
        {
            bail!("bootstrap replay and delegated identity records do not match");
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(existing) = load_bootstrap_replay_by_id_or_key(
            &transaction,
            &replay.bootstrap_id,
            &replay.idempotency_key,
        )? {
            if existing.payload_hash != replay.payload_hash {
                bail!("daemon bootstrap replay conflict");
            }
            if existing.bootstrap_id != replay.bootstrap_id
                || existing.idempotency_key != replay.idempotency_key
                || existing.user_did != replay.user_did
                || existing.verification_method != replay.verification_method
                || existing.app_instance_id != replay.app_instance_id
                || existing.daemon_agent_did != replay.daemon_agent_did
            {
                bail!("daemon bootstrap replay identity conflict");
            }
            return Ok(BootstrapStoreOutcome::Duplicate);
        }
        let now = current_time_millis()?;
        transaction.execute(
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
                now,
            ],
        )?;
        transaction.execute(
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
ON CONFLICT(verification_method) DO UPDATE SET
    user_did = excluded.user_did,
    app_instance_id = excluded.app_instance_id,
    controller_did = excluded.controller_did,
    daemon_agent_did = excluded.daemon_agent_did,
    public_key_multibase = excluded.public_key_multibase,
    private_key_material = excluded.private_key_material,
    allowed_scopes_json = excluded.allowed_scopes_json,
    status = excluded.status,
    expires_at = excluded.expires_at,
    bootstrap_id = excluded.bootstrap_id,
    idempotency_key = excluded.idempotency_key,
    updated_at_ms = excluded.updated_at_ms
"#,
            rusqlite::params![
                &identity.user_did,
                &identity.verification_method,
                &identity.app_instance_id,
                &identity.controller_did,
                &identity.daemon_agent_did,
                &identity.public_key_multibase,
                &identity.private_key_material,
                identity.allowed_scopes_json.to_string(),
                &identity.status,
                &identity.expires_at,
                &identity.bootstrap_id,
                &identity.idempotency_key,
                now,
            ],
        )?;
        transaction.commit()?;
        Ok(BootstrapStoreOutcome::Inserted)
    }

    pub fn load_user_delegated_identity(
        &self,
        verification_method: &str,
    ) -> Result<Option<UserDelegatedIdentityRecord>> {
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
SELECT
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
FROM user_delegated_identity
WHERE verification_method = ?1
"#,
                [verification_method],
                user_delegated_identity_from_row,
            )
            .optional()
            .context("load user delegated identity")
    }

    pub fn load_bootstrap_replay(
        &self,
        bootstrap_id: &str,
    ) -> Result<Option<BootstrapReplayRecord>> {
        let connection = self.connection()?;
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
WHERE bootstrap_id = ?1
"#,
                [bootstrap_id],
                bootstrap_replay_from_row,
            )
            .optional()
            .context("load bootstrap replay")
    }

    pub fn upsert_app_message_agent_binding(
        &self,
        record: &AppMessageAgentBindingRecord,
    ) -> Result<()> {
        record.validate()?;
        let connection = self.connection()?;
        let now = current_time_millis()?;
        connection.execute(
            r#"
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
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
ON CONFLICT(binding_id) DO UPDATE SET
    user_did = excluded.user_did,
    inbox_auth_verification_method = excluded.inbox_auth_verification_method,
    app_instance_id = excluded.app_instance_id,
    bootstrap_id = excluded.bootstrap_id,
    idempotency_key = excluded.idempotency_key,
    daemon_agent_did = excluded.daemon_agent_did,
    runtime_agent_did = excluded.runtime_agent_did,
    runtime_profile_id = excluded.runtime_profile_id,
    role = excluded.role,
    desired_agent_json = excluded.desired_agent_json,
    capability_policy_json = excluded.capability_policy_json,
    status = excluded.status,
    updated_at_ms = excluded.updated_at_ms,
    revoked_at_ms = excluded.revoked_at_ms
"#,
            rusqlite::params![
                &record.binding_id,
                &record.user_did,
                &record.inbox_auth_verification_method,
                &record.app_instance_id,
                &record.bootstrap_id,
                &record.idempotency_key,
                &record.daemon_agent_did,
                &record.runtime_agent_did,
                &record.runtime_profile_id,
                &record.role,
                record.desired_agent_json.to_string(),
                record.capability_policy_json.to_string(),
                &record.status,
                if record.created_at_ms > 0 {
                    record.created_at_ms
                } else {
                    now
                },
                now,
                record.revoked_at_ms,
            ],
        )?;
        Ok(())
    }

    pub fn revoke_other_active_app_message_agent_bindings(
        &self,
        user_did: &str,
        role: &str,
        keep_binding_id: &str,
    ) -> Result<usize> {
        let connection = self.connection()?;
        let now = current_time_millis()?;
        let affected = connection.execute(
            r#"
UPDATE app_message_agent_binding
SET revoked_at_ms = ?1,
    updated_at_ms = ?1
WHERE user_did = ?2
  AND role = ?3
  AND binding_id <> ?4
  AND revoked_at_ms IS NULL
  AND status IN ('message_agent_ready', 'message_agent_active', 'message_agent_ensuring')
"#,
            rusqlite::params![now, user_did, role, keep_binding_id],
        )?;
        Ok(affected)
    }

    pub fn load_active_app_message_agent_binding(
        &self,
        user_did: &str,
        app_instance_id: &str,
        role: &str,
    ) -> Result<Option<AppMessageAgentBindingRecord>> {
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
SELECT
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
FROM app_message_agent_binding
WHERE user_did = ?1
  AND app_instance_id = ?2
  AND role = ?3
  AND revoked_at_ms IS NULL
  AND status IN ('message_agent_ready', 'message_agent_active', 'message_agent_ensuring')
ORDER BY updated_at_ms DESC
LIMIT 1
"#,
                rusqlite::params![user_did, app_instance_id, role],
                app_message_agent_binding_from_row,
            )
            .optional()
            .context("load active app message agent binding")
    }

    pub fn load_app_message_agent_binding(
        &self,
        binding_id: &str,
    ) -> Result<Option<AppMessageAgentBindingRecord>> {
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
SELECT
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
FROM app_message_agent_binding
WHERE binding_id = ?1
"#,
                [binding_id],
                app_message_agent_binding_from_row,
            )
            .optional()
            .context("load app message agent binding")
    }

    pub fn load_active_app_message_agent_binding_by_runtime(
        &self,
        runtime_agent_did: &str,
    ) -> Result<Option<AppMessageAgentBindingRecord>> {
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
SELECT
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
FROM app_message_agent_binding
WHERE runtime_agent_did = ?1
  AND revoked_at_ms IS NULL
  AND status IN ('message_agent_ready', 'message_agent_active', 'message_agent_ensuring')
ORDER BY updated_at_ms DESC
LIMIT 1
"#,
                [runtime_agent_did],
                app_message_agent_binding_from_row,
            )
            .optional()
            .context("load active app message agent binding by runtime")
    }

    pub fn list_active_app_message_agent_bindings(
        &self,
    ) -> Result<Vec<AppMessageAgentBindingRecord>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            r#"
SELECT
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
FROM app_message_agent_binding
WHERE revoked_at_ms IS NULL
  AND status IN ('message_agent_ready', 'message_agent_active', 'message_agent_ensuring')
ORDER BY updated_at_ms ASC
"#,
        )?;
        let rows = statement.query_map([], app_message_agent_binding_from_row)?;
        let mut bindings = Vec::new();
        for row in rows {
            bindings.push(row?);
        }
        Ok(bindings)
    }

    pub fn load_inbox_cursor(
        &self,
        owner_did: &str,
        inbox_scope: &str,
    ) -> Result<Option<InboxCursorRecord>> {
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
SELECT owner_did, inbox_scope, cursor, updated_at_ms
FROM inbox_cursor
WHERE owner_did = ?1 AND inbox_scope = ?2
"#,
                rusqlite::params![owner_did, inbox_scope],
                inbox_cursor_from_row,
            )
            .optional()
            .context("load inbox cursor")
    }

    pub fn upsert_inbox_cursor(&self, record: &InboxCursorRecord) -> Result<()> {
        record.validate()?;
        let connection = self.connection()?;
        let now = current_time_millis()?;
        connection.execute(
            r#"
INSERT INTO inbox_cursor (
    owner_did,
    inbox_scope,
    cursor,
    updated_at_ms
) VALUES (?1, ?2, ?3, ?4)
ON CONFLICT(owner_did, inbox_scope) DO UPDATE SET
    cursor = excluded.cursor,
    updated_at_ms = excluded.updated_at_ms
"#,
            rusqlite::params![
                &record.owner_did,
                &record.inbox_scope,
                &record.cursor,
                if record.updated_at_ms > 0 {
                    record.updated_at_ms
                } else {
                    now
                },
            ],
        )?;
        Ok(())
    }

    pub fn load_processed_message(
        &self,
        owner_did: &str,
        message_id: &str,
    ) -> Result<Option<ProcessedMessageRecord>> {
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
SELECT owner_did, message_id, schema, processed_at_ms, status
FROM processed_message
WHERE owner_did = ?1 AND message_id = ?2
"#,
                rusqlite::params![owner_did, message_id],
                processed_message_from_row,
            )
            .optional()
            .context("load processed message")
    }

    pub fn try_insert_processed_message(&self, record: &ProcessedMessageRecord) -> Result<bool> {
        record.validate()?;
        let connection = self.connection()?;
        let now = current_time_millis()?;
        let inserted = connection.execute(
            r#"
INSERT OR IGNORE INTO processed_message (
    owner_did,
    message_id,
    schema,
    processed_at_ms,
    status
) VALUES (?1, ?2, ?3, ?4, ?5)
"#,
            rusqlite::params![
                &record.owner_did,
                &record.message_id,
                &record.schema,
                if record.processed_at_ms > 0 {
                    record.processed_at_ms
                } else {
                    now
                },
                &record.status,
            ],
        )?;
        Ok(inserted > 0)
    }

    pub fn mark_processed_message_status(
        &self,
        owner_did: &str,
        message_id: &str,
        status: &str,
    ) -> Result<()> {
        if status.trim().is_empty() {
            bail!("processed message status must not be empty");
        }
        let connection = self.connection()?;
        connection.execute(
            r#"
UPDATE processed_message
SET status = ?3,
    processed_at_ms = ?4
WHERE owner_did = ?1 AND message_id = ?2
"#,
            rusqlite::params![owner_did, message_id, status, current_time_millis()?],
        )?;
        Ok(())
    }

    pub fn upsert_message_event(&self, record: &MessageEventRecord) -> Result<()> {
        record.validate()?;
        let connection = self.connection()?;
        let now = current_time_millis()?;
        connection.execute(
            r#"
INSERT INTO message_event (
    event_id,
    owner_did,
    conversation_id,
    message_id,
    message_kind,
    sender_did,
    received_at,
    plain_text_ref_or_excerpt,
    content_hash,
    schema,
    processing_status,
    retention_class,
    created_at_ms,
    updated_at_ms
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
ON CONFLICT(event_id) DO UPDATE SET
    conversation_id = excluded.conversation_id,
    message_kind = excluded.message_kind,
    sender_did = excluded.sender_did,
    received_at = excluded.received_at,
    plain_text_ref_or_excerpt = excluded.plain_text_ref_or_excerpt,
    content_hash = excluded.content_hash,
    schema = excluded.schema,
    processing_status = excluded.processing_status,
    retention_class = excluded.retention_class,
    updated_at_ms = excluded.updated_at_ms
"#,
            rusqlite::params![
                &record.event_id,
                &record.owner_did,
                &record.conversation_id,
                &record.message_id,
                &record.message_kind,
                &record.sender_did,
                &record.received_at,
                &record.plain_text_ref_or_excerpt,
                &record.content_hash,
                &record.schema,
                &record.processing_status,
                &record.retention_class,
                if record.created_at_ms > 0 {
                    record.created_at_ms
                } else {
                    now
                },
                if record.updated_at_ms > 0 {
                    record.updated_at_ms
                } else {
                    now
                },
            ],
        )?;
        Ok(())
    }

    pub fn load_message_event(&self, event_id: &str) -> Result<Option<MessageEventRecord>> {
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
SELECT
    event_id,
    owner_did,
    conversation_id,
    message_id,
    message_kind,
    sender_did,
    received_at,
    plain_text_ref_or_excerpt,
    content_hash,
    schema,
    processing_status,
    retention_class,
    created_at_ms,
    updated_at_ms
FROM message_event
WHERE event_id = ?1
"#,
                [event_id],
                message_event_from_row,
            )
            .optional()
            .context("load message event")
    }

    pub fn upsert_message_sync_outbox(&self, record: &MessageSyncOutboxRecord) -> Result<()> {
        record.validate()?;
        let connection = self.connection()?;
        let now = current_time_millis()?;
        connection.execute(
            r#"
INSERT INTO message_sync_outbox (
    idempotency_key,
    owner_did,
    app_instance_id,
    payload_json,
    status,
    attempt_count,
    next_attempt_at_ms,
    last_error_code,
    last_error_summary,
    created_at_ms,
    updated_at_ms,
    sent_at_ms
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
ON CONFLICT(idempotency_key) DO UPDATE SET
    payload_json = CASE WHEN message_sync_outbox.status = 'sent' THEN message_sync_outbox.payload_json ELSE excluded.payload_json END,
    status = CASE WHEN message_sync_outbox.status = 'sent' THEN message_sync_outbox.status ELSE excluded.status END,
    next_attempt_at_ms = CASE WHEN message_sync_outbox.status = 'sent' THEN message_sync_outbox.next_attempt_at_ms ELSE excluded.next_attempt_at_ms END,
    last_error_code = CASE WHEN message_sync_outbox.status = 'sent' THEN message_sync_outbox.last_error_code ELSE excluded.last_error_code END,
    last_error_summary = CASE WHEN message_sync_outbox.status = 'sent' THEN message_sync_outbox.last_error_summary ELSE excluded.last_error_summary END,
    updated_at_ms = excluded.updated_at_ms,
    sent_at_ms = CASE WHEN message_sync_outbox.status = 'sent' THEN message_sync_outbox.sent_at_ms ELSE excluded.sent_at_ms END
"#,
            rusqlite::params![
                &record.idempotency_key,
                &record.owner_did,
                &record.app_instance_id,
                record.payload_json.to_string(),
                &record.status,
                record.attempt_count,
                record.next_attempt_at_ms,
                &record.last_error_code,
                &record.last_error_summary,
                if record.created_at_ms > 0 {
                    record.created_at_ms
                } else {
                    now
                },
                if record.updated_at_ms > 0 {
                    record.updated_at_ms
                } else {
                    now
                },
                record.sent_at_ms,
            ],
        )?;
        Ok(())
    }

    pub fn load_message_sync_outbox(
        &self,
        idempotency_key: &str,
    ) -> Result<Option<MessageSyncOutboxRecord>> {
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
SELECT
    idempotency_key,
    owner_did,
    app_instance_id,
    payload_json,
    status,
    attempt_count,
    next_attempt_at_ms,
    last_error_code,
    last_error_summary,
    created_at_ms,
    updated_at_ms,
    sent_at_ms
FROM message_sync_outbox
WHERE idempotency_key = ?1
"#,
                [idempotency_key],
                message_sync_outbox_from_row,
            )
            .optional()
            .context("load message sync outbox")
    }

    pub fn list_due_message_sync_outbox(
        &self,
        now_ms: i64,
        limit: usize,
    ) -> Result<Vec<MessageSyncOutboxRecord>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            r#"
SELECT
    idempotency_key,
    owner_did,
    app_instance_id,
    payload_json,
    status,
    attempt_count,
    next_attempt_at_ms,
    last_error_code,
    last_error_summary,
    created_at_ms,
    updated_at_ms,
    sent_at_ms
FROM message_sync_outbox
WHERE status = 'pending'
  AND next_attempt_at_ms <= ?1
ORDER BY created_at_ms ASC, idempotency_key ASC
LIMIT ?2
"#,
        )?;
        let rows = statement.query_map(
            rusqlite::params![now_ms, limit.max(1) as i64],
            message_sync_outbox_from_row,
        )?;
        let mut records = Vec::new();
        for row in rows {
            records.push(row?);
        }
        Ok(records)
    }

    pub fn mark_message_sync_outbox_sending(&self, idempotency_key: &str) -> Result<bool> {
        let connection = self.connection()?;
        let updated = connection.execute(
            r#"
UPDATE message_sync_outbox
SET status = 'sending',
    attempt_count = attempt_count + 1,
    updated_at_ms = ?1
WHERE idempotency_key = ?2
  AND status = 'pending'
"#,
            rusqlite::params![current_time_millis()?, idempotency_key],
        )?;
        Ok(updated > 0)
    }

    pub fn mark_message_sync_outbox_sent(&self, idempotency_key: &str) -> Result<()> {
        let now = current_time_millis()?;
        let connection = self.connection()?;
        let updated = connection.execute(
            r#"
UPDATE message_sync_outbox
SET status = 'sent',
    sent_at_ms = ?1,
    updated_at_ms = ?1,
    last_error_code = NULL,
    last_error_summary = NULL
WHERE idempotency_key = ?2
"#,
            rusqlite::params![now, idempotency_key],
        )?;
        if updated == 0 {
            bail!("message sync outbox does not exist: {idempotency_key}");
        }
        Ok(())
    }

    pub fn mark_message_sync_outbox_retry(
        &self,
        idempotency_key: &str,
        next_attempt_at_ms: i64,
        error_code: &str,
        error_summary: &str,
    ) -> Result<()> {
        let connection = self.connection()?;
        let updated = connection.execute(
            r#"
UPDATE message_sync_outbox
SET status = 'pending',
    next_attempt_at_ms = ?1,
    last_error_code = ?2,
    last_error_summary = ?3,
    updated_at_ms = ?4
WHERE idempotency_key = ?5
  AND status = 'sending'
"#,
            rusqlite::params![
                next_attempt_at_ms,
                error_code,
                error_summary,
                current_time_millis()?,
                idempotency_key,
            ],
        )?;
        if updated == 0 {
            bail!("message sync outbox is not sending: {idempotency_key}");
        }
        Ok(())
    }

    pub fn recover_stale_message_sync_outbox_sending(
        &self,
        stale_before_ms: i64,
        next_attempt_at_ms: i64,
    ) -> Result<usize> {
        let connection = self.connection()?;
        let updated = connection.execute(
            r#"
UPDATE message_sync_outbox
SET status = 'pending',
    next_attempt_at_ms = ?1,
    last_error_code = COALESCE(last_error_code, 'message_sync_delivery_recovered'),
    last_error_summary = COALESCE(last_error_summary, 'Recovered stale message sync delivery attempt'),
    updated_at_ms = ?2
WHERE status = 'sending'
  AND updated_at_ms <= ?3
"#,
            rusqlite::params![next_attempt_at_ms, current_time_millis()?, stale_before_ms],
        )?;
        Ok(updated)
    }

    pub fn mark_message_sync_outbox_failed_terminal(
        &self,
        idempotency_key: &str,
        error_code: &str,
        error_summary: &str,
    ) -> Result<()> {
        let connection = self.connection()?;
        let updated = connection.execute(
            r#"
UPDATE message_sync_outbox
SET status = 'failed_terminal',
    last_error_code = ?1,
    last_error_summary = ?2,
    updated_at_ms = ?3
WHERE idempotency_key = ?4
"#,
            rusqlite::params![
                error_code,
                error_summary,
                current_time_millis()?,
                idempotency_key,
            ],
        )?;
        if updated == 0 {
            bail!("message sync outbox does not exist: {idempotency_key}");
        }
        Ok(())
    }

    pub fn store_agent_auth_token(&self, agent_did: &str, jwt_token: &str) -> Result<()> {
        if agent_did.trim().is_empty() {
            bail!("agent_did must not be empty");
        }
        let jwt_token = jwt_token.trim();
        if jwt_token.is_empty() {
            bail!("agent auth token must not be empty");
        }
        let connection = self.connection()?;
        let now = current_time_millis()?;
        connection.execute(
            r#"
INSERT INTO agent_auth_state (
    agent_did,
    jwt_token,
    updated_at_ms
) VALUES (?1, ?2, ?3)
ON CONFLICT(agent_did) DO UPDATE SET
    jwt_token = excluded.jwt_token,
    updated_at_ms = excluded.updated_at_ms
"#,
            rusqlite::params![agent_did, jwt_token, now],
        )?;
        Ok(())
    }

    pub fn load_agent_auth_token(&self, agent_did: &str) -> Result<Option<String>> {
        let connection = self.connection()?;
        let mut statement =
            connection.prepare("SELECT jwt_token FROM agent_auth_state WHERE agent_did = ?1")?;
        let mut rows = statement.query([agent_did])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        Ok(Some(row.get(0)?))
    }

    pub fn list_agent_auth_tokens(&self) -> Result<Vec<(String, String)>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            r#"
SELECT agent_did, jwt_token
FROM agent_auth_state
ORDER BY agent_did ASC
"#,
        )?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        let mut tokens = Vec::new();
        for row in rows {
            tokens.push(row?);
        }
        Ok(tokens)
    }
}
