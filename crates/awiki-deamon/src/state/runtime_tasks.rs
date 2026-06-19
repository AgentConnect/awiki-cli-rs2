use super::row_mappers::*;
use super::*;
use crate::runtime::RuntimeTaskTriggerKind;
use rand::{rngs::OsRng, RngCore};

const CLI_ROUTE_HASH_SALT_METADATA_KEY: &str = "generic_cli.route_hash_salt.v2";

impl DaemonState {
    pub fn ensure_cli_route_hash_salt(&self) -> Result<String> {
        let connection = self.connection()?;
        if let Some(existing) = connection
            .query_row(
                "SELECT value FROM daemon_state_metadata WHERE key = ?1",
                [CLI_ROUTE_HASH_SALT_METADATA_KEY],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .context("load generic-cli route hash salt")?
        {
            decode_cli_route_hash_salt_hex(&existing)?;
            return Ok(existing);
        }

        let mut salt = [0u8; 32];
        OsRng.fill_bytes(&mut salt);
        let salt = encode_cli_route_hash_salt_hex(&salt);
        connection.execute(
            r#"
INSERT INTO daemon_state_metadata (key, value, updated_at_ms)
VALUES (?1, ?2, ?3)
ON CONFLICT(key) DO NOTHING
"#,
            rusqlite::params![
                CLI_ROUTE_HASH_SALT_METADATA_KEY,
                salt,
                current_time_millis()?
            ],
        )?;
        let persisted = connection.query_row(
            "SELECT value FROM daemon_state_metadata WHERE key = ?1",
            [CLI_ROUTE_HASH_SALT_METADATA_KEY],
            |row| row.get::<_, String>(0),
        )?;
        decode_cli_route_hash_salt_hex(&persisted)?;
        Ok(persisted)
    }

    pub fn generic_cli_route_hash_salt_present(&self) -> bool {
        let Ok(connection) = self.connection() else {
            return false;
        };
        let Ok(value) = connection
            .query_row(
                "SELECT value FROM daemon_state_metadata WHERE key = ?1",
                [CLI_ROUTE_HASH_SALT_METADATA_KEY],
                |row| row.get::<_, String>(0),
            )
            .optional()
        else {
            return false;
        };
        value
            .as_deref()
            .is_some_and(|value| decode_cli_route_hash_salt_hex(value).is_ok())
    }

    pub fn cli_route_key_hash(&self, route_key: &str) -> Result<String> {
        let salt = self.ensure_cli_route_hash_salt()?;
        cli_route_key_hash_with_salt(route_key, &salt)
    }

    pub fn insert_runtime_task(&self, task: &RuntimeTask) -> Result<()> {
        task.validate()?;
        let connection = self.connection()?;
        let now = current_time_millis()?;
        connection.execute(
            r#"
INSERT INTO runtime_task (
    task_id,
    agent_did,
    controller_user_id,
    controller_full_handle,
    controller_scope_key,
    controller_did,
    sender_did,
    requester_did,
    requester_full_handle,
    trigger_kind,
    reply_recipient_did,
    conversation_id,
    task_text,
    status,
    created_at_ms,
    updated_at_ms
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, 'created', ?14, ?14)
ON CONFLICT(task_id) DO UPDATE SET
    task_text = excluded.task_text,
    sender_did = excluded.sender_did,
    requester_did = excluded.requester_did,
    requester_full_handle = excluded.requester_full_handle,
    trigger_kind = excluded.trigger_kind,
    reply_recipient_did = excluded.reply_recipient_did,
    conversation_id = excluded.conversation_id,
    updated_at_ms = excluded.updated_at_ms
"#,
            rusqlite::params![
                task.task_id,
                task.agent_did,
                task.controller_user_id,
                task.controller_full_handle,
                task.controller_scope_key,
                task.controller_did,
                task.sender_did,
                task.requester_did,
                task.requester_full_handle,
                task.trigger_kind.as_str(),
                task.reply_recipient_did,
                task.conversation_id,
                task.text,
                now,
            ],
        )?;
        Ok(())
    }

    pub fn try_insert_runtime_run(&self, run: &RuntimeRun) -> Result<bool> {
        let connection = self.connection()?;
        let now = current_time_millis()?;
        let inserted = connection.execute(
            r#"
INSERT OR IGNORE INTO runtime_run (
    run_id,
    task_id,
    agent_did,
    runtime_profile_id,
    runtime_plugin_id,
    workspace_id,
    status,
    started_at,
    updated_at,
    started_at_ms,
    updated_at_ms
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, ?9, ?9)
"#,
            rusqlite::params![
                run.run_id,
                run.task_id,
                run.agent_did,
                run.runtime_profile_id,
                run.runtime_plugin_id,
                run.workspace_id,
                run.status.as_str(),
                now.to_string(),
                now,
            ],
        )?;
        Ok(inserted > 0)
    }

    pub fn insert_runtime_run(&self, run: &RuntimeRun) -> Result<()> {
        if !self.try_insert_runtime_run(run)? {
            bail!("runtime run already exists: {}", run.run_id);
        }
        Ok(())
    }

    pub fn insert_runtime_retry_request(
        &self,
        original_run: &RuntimeRun,
        command_id: &str,
    ) -> Result<RuntimeRetryQueueRecord> {
        self.insert_runtime_retry_request_due_at(original_run, command_id, current_time_millis()?)
    }

    pub fn insert_runtime_retry_request_due_at(
        &self,
        original_run: &RuntimeRun,
        command_id: &str,
        next_attempt_at_ms: i64,
    ) -> Result<RuntimeRetryQueueRecord> {
        if original_run.status != RuntimeRunStatus::Failed {
            bail!("only failed runs can be retried");
        }
        if next_attempt_at_ms < 0 {
            bail!("next_attempt_at_ms must not be negative");
        }
        let command_id = command_id.trim();
        if command_id.is_empty() {
            bail!("command_id must not be empty");
        }
        let now = current_time_millis()?;
        let retry_id = format!("retry_{}_{}", now, stable_id_suffix(&original_run.run_id));
        let record = RuntimeRetryQueueRecord {
            retry_id,
            original_run_id: original_run.run_id.clone(),
            task_id: original_run.task_id.clone(),
            agent_did: original_run.agent_did.clone(),
            runtime_profile_id: original_run.runtime_profile_id.clone(),
            runtime_plugin_id: original_run.runtime_plugin_id.clone(),
            workspace_id: original_run.workspace_id.clone(),
            status: "queued".to_string(),
            requested_by_command_id: command_id.to_string(),
            attempts: 0,
            next_attempt_at_ms,
            created_at_ms: now,
            updated_at_ms: now,
        };
        record.validate()?;
        let connection = self.connection()?;
        connection.execute(
            r#"
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
    next_attempt_at_ms,
    created_at_ms,
    updated_at_ms
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'queued', ?8, 0, ?9, ?10, ?10)
"#,
            rusqlite::params![
                record.retry_id,
                record.original_run_id,
                record.task_id,
                record.agent_did,
                record.runtime_profile_id,
                record.runtime_plugin_id,
                record.workspace_id,
                record.requested_by_command_id,
                record.next_attempt_at_ms,
                now,
            ],
        )?;
        Ok(record)
    }

    pub fn load_runtime_retry_request(&self, retry_id: &str) -> Result<RuntimeRetryQueueRecord> {
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
SELECT
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
    next_attempt_at_ms,
    created_at_ms,
    updated_at_ms
FROM runtime_retry_queue
WHERE retry_id = ?1
"#,
                [retry_id],
                runtime_retry_queue_record_from_row,
            )
            .context("load runtime retry request")
    }

    pub fn list_runtime_retry_requests_for_original_run(
        &self,
        original_run_id: &str,
    ) -> Result<Vec<RuntimeRetryQueueRecord>> {
        if original_run_id.trim().is_empty() {
            bail!("original_run_id must not be empty");
        }
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            r#"
SELECT
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
    next_attempt_at_ms,
    created_at_ms,
    updated_at_ms
FROM runtime_retry_queue
WHERE original_run_id = ?1
ORDER BY created_at_ms ASC, retry_id ASC
"#,
        )?;
        let rows = statement.query_map([original_run_id], runtime_retry_queue_record_from_row)?;
        let mut retries = Vec::new();
        for row in rows {
            retries.push(row?);
        }
        Ok(retries)
    }

    pub fn list_queued_runtime_retries_due(
        &self,
        now_ms: i64,
        limit: usize,
    ) -> Result<Vec<RuntimeRetryQueueRecord>> {
        if now_ms < 0 {
            bail!("now_ms must not be negative");
        }
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            r#"
SELECT
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
    next_attempt_at_ms,
    created_at_ms,
    updated_at_ms
FROM runtime_retry_queue
WHERE status = 'queued'
  AND next_attempt_at_ms <= ?1
ORDER BY next_attempt_at_ms ASC, created_at_ms ASC, retry_id ASC
LIMIT ?2
"#,
        )?;
        let rows = statement.query_map(
            rusqlite::params![now_ms, limit.max(1) as i64],
            runtime_retry_queue_record_from_row,
        )?;
        let mut retries = Vec::new();
        for row in rows {
            retries.push(row?);
        }
        Ok(retries)
    }

    pub fn list_queued_runtime_retries(
        &self,
        limit: usize,
    ) -> Result<Vec<RuntimeRetryQueueRecord>> {
        self.list_queued_runtime_retries_due(current_time_millis()?, limit)
    }

    pub fn mark_runtime_retry_status(&self, retry_id: &str, status: &str) -> Result<()> {
        if retry_id.trim().is_empty() {
            bail!("retry_id must not be empty");
        }
        if status.trim().is_empty() {
            bail!("retry status must not be empty");
        }
        let connection = self.connection()?;
        let updated = connection.execute(
            r#"
UPDATE runtime_retry_queue
SET status = ?1,
    attempts = attempts + CASE WHEN ?1 = 'running' THEN 1 ELSE 0 END,
    updated_at_ms = ?2
WHERE retry_id = ?3
"#,
            rusqlite::params![status, current_time_millis()?, retry_id],
        )?;
        if updated == 0 {
            bail!("runtime retry request does not exist: {retry_id}");
        }
        Ok(())
    }

    pub fn reschedule_runtime_retry_request(
        &self,
        retry_id: &str,
        next_attempt_at_ms: i64,
    ) -> Result<()> {
        if retry_id.trim().is_empty() {
            bail!("retry_id must not be empty");
        }
        if next_attempt_at_ms < 0 {
            bail!("next_attempt_at_ms must not be negative");
        }
        let connection = self.connection()?;
        let updated = connection.execute(
            r#"
UPDATE runtime_retry_queue
SET status = 'queued',
    next_attempt_at_ms = ?1,
    updated_at_ms = ?2
WHERE retry_id = ?3
"#,
            rusqlite::params![next_attempt_at_ms, current_time_millis()?, retry_id],
        )?;
        if updated == 0 {
            bail!("runtime retry request does not exist: {retry_id}");
        }
        Ok(())
    }

    pub fn upsert_runtime_final_outbox_pending(
        &self,
        record: &RuntimeFinalOutboxRecord,
    ) -> Result<()> {
        record.validate()?;
        if record.status != "pending" {
            bail!("runtime final outbox upsert requires pending status");
        }
        let now = current_time_millis()?;
        let connection = self.connection()?;
        connection.execute(
            r#"
INSERT INTO runtime_final_outbox (
    idempotency_key,
    run_id,
    agent_did,
    runtime_profile_id,
    controller_scope_key,
    controller_did,
    recipient_did,
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
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'pending', 0, ?11, NULL, NULL, NULL, ?12, ?12, NULL)
ON CONFLICT(idempotency_key) DO UPDATE SET
    final_text = CASE WHEN runtime_final_outbox.status = 'sent' THEN runtime_final_outbox.final_text ELSE excluded.final_text END,
    security = CASE WHEN runtime_final_outbox.status = 'sent' THEN runtime_final_outbox.security ELSE excluded.security END,
    recipient_did = CASE WHEN runtime_final_outbox.status = 'sent' THEN runtime_final_outbox.recipient_did ELSE excluded.recipient_did END,
    conversation_id = CASE WHEN runtime_final_outbox.status = 'sent' THEN runtime_final_outbox.conversation_id ELSE excluded.conversation_id END,
    status = CASE WHEN runtime_final_outbox.status = 'sent' THEN runtime_final_outbox.status ELSE 'pending' END,
    next_attempt_at_ms = CASE WHEN runtime_final_outbox.status = 'sent' THEN runtime_final_outbox.next_attempt_at_ms ELSE excluded.next_attempt_at_ms END,
    last_error_code = CASE WHEN runtime_final_outbox.status = 'sent' THEN runtime_final_outbox.last_error_code ELSE NULL END,
    last_error_summary = CASE WHEN runtime_final_outbox.status = 'sent' THEN runtime_final_outbox.last_error_summary ELSE NULL END,
    updated_at_ms = excluded.updated_at_ms
"#,
            rusqlite::params![
                record.idempotency_key,
                record.run_id,
                record.agent_did,
                record.runtime_profile_id,
                record.controller_scope_key,
                record.controller_did,
                record.recipient_did,
                record.conversation_id,
                record.final_text,
                record.security,
                record.next_attempt_at_ms,
                now,
            ],
        )?;
        Ok(())
    }

    pub fn load_runtime_final_outbox_by_run(
        &self,
        run_id: &str,
    ) -> Result<Option<RuntimeFinalOutboxRecord>> {
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
SELECT
    idempotency_key,
    run_id,
    agent_did,
    runtime_profile_id,
    controller_scope_key,
    controller_did,
    recipient_did,
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
FROM runtime_final_outbox
WHERE run_id = ?1
"#,
                [run_id],
                runtime_final_outbox_record_from_row,
            )
            .optional()
            .context("load runtime final outbox by run")
    }

    pub fn list_due_runtime_final_outbox(
        &self,
        now_ms: i64,
        limit: usize,
    ) -> Result<Vec<RuntimeFinalOutboxRecord>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            r#"
SELECT
    idempotency_key,
    run_id,
    agent_did,
    runtime_profile_id,
    controller_scope_key,
    controller_did,
    recipient_did,
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
FROM runtime_final_outbox
WHERE status = 'pending'
  AND next_attempt_at_ms <= ?1
ORDER BY created_at_ms ASC, idempotency_key ASC
LIMIT ?2
"#,
        )?;
        let rows = statement.query_map(
            rusqlite::params![now_ms, limit.max(1) as i64],
            runtime_final_outbox_record_from_row,
        )?;
        let mut records = Vec::new();
        for row in rows {
            records.push(row?);
        }
        Ok(records)
    }

    pub fn mark_runtime_final_outbox_sending(&self, idempotency_key: &str) -> Result<bool> {
        let connection = self.connection()?;
        let updated = connection.execute(
            r#"
UPDATE runtime_final_outbox
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

    pub fn mark_runtime_final_outbox_sent(
        &self,
        idempotency_key: &str,
        message_id: Option<&str>,
    ) -> Result<()> {
        let now = current_time_millis()?;
        let connection = self.connection()?;
        let updated = connection.execute(
            r#"
UPDATE runtime_final_outbox
SET status = 'sent',
    message_id = ?1,
    sent_at_ms = ?2,
    updated_at_ms = ?2,
    last_error_code = NULL,
    last_error_summary = NULL
WHERE idempotency_key = ?3
"#,
            rusqlite::params![message_id, now, idempotency_key],
        )?;
        if updated == 0 {
            bail!("runtime final outbox does not exist: {idempotency_key}");
        }
        Ok(())
    }

    pub fn mark_runtime_final_outbox_retry(
        &self,
        idempotency_key: &str,
        next_attempt_at_ms: i64,
        error_code: &str,
        error_summary: &str,
    ) -> Result<()> {
        let connection = self.connection()?;
        let updated = connection.execute(
            r#"
UPDATE runtime_final_outbox
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
            bail!("runtime final outbox is not sending: {idempotency_key}");
        }
        Ok(())
    }

    pub fn recover_stale_runtime_final_outbox_sending(
        &self,
        stale_before_ms: i64,
        next_attempt_at_ms: i64,
    ) -> Result<usize> {
        let connection = self.connection()?;
        let updated = connection.execute(
            r#"
UPDATE runtime_final_outbox
SET status = 'pending',
    next_attempt_at_ms = ?1,
    last_error_code = COALESCE(last_error_code, 'final_delivery_recovered'),
    last_error_summary = COALESCE(last_error_summary, 'Recovered stale final delivery attempt'),
    updated_at_ms = ?2
WHERE status = 'sending'
  AND updated_at_ms <= ?3
"#,
            rusqlite::params![next_attempt_at_ms, current_time_millis()?, stale_before_ms],
        )?;
        Ok(updated)
    }

    pub fn mark_runtime_final_outbox_failed_terminal(
        &self,
        idempotency_key: &str,
        error_code: &str,
        error_summary: &str,
    ) -> Result<()> {
        let connection = self.connection()?;
        let updated = connection.execute(
            r#"
UPDATE runtime_final_outbox
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
            bail!("runtime final outbox does not exist: {idempotency_key}");
        }
        Ok(())
    }

    pub fn update_runtime_run_status(&self, run_id: &str, status: RuntimeRunStatus) -> Result<()> {
        let connection = self.connection()?;
        let now = current_time_millis()?;
        let completed_at = match status {
            RuntimeRunStatus::Finished | RuntimeRunStatus::Failed => Some(now.to_string()),
            RuntimeRunStatus::Pending | RuntimeRunStatus::Running => None,
        };
        let updated = connection.execute(
            r#"
UPDATE runtime_run
SET status = ?1,
    completed_at = COALESCE(?2, completed_at),
    updated_at = ?3,
    completed_at_ms = COALESCE(?4, completed_at_ms),
    updated_at_ms = ?5
WHERE run_id = ?6
"#,
            rusqlite::params![
                status.as_str(),
                completed_at,
                now.to_string(),
                match status {
                    RuntimeRunStatus::Finished | RuntimeRunStatus::Failed => Some(now),
                    RuntimeRunStatus::Pending | RuntimeRunStatus::Running => None,
                },
                now,
                run_id,
            ],
        )?;
        if updated == 0 {
            bail!("runtime run does not exist: {run_id}");
        }
        Ok(())
    }

    pub fn load_runtime_run(&self, run_id: &str) -> Result<RuntimeRun> {
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
SELECT run_id, task_id, agent_did, runtime_profile_id, runtime_plugin_id, workspace_id, status
FROM runtime_run
WHERE run_id = ?1
"#,
                [run_id],
                |row| {
                    let status: String = row.get(6)?;
                    let status = RuntimeRunStatus::parse(&status).map_err(|err| {
                        rusqlite::Error::FromSqlConversionFailure(
                            status.len(),
                            rusqlite::types::Type::Text,
                            Box::new(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                err.to_string(),
                            )),
                        )
                    })?;
                    Ok(RuntimeRun {
                        run_id: row.get(0)?,
                        task_id: row.get(1)?,
                        agent_did: row.get(2)?,
                        runtime_profile_id: row.get(3)?,
                        runtime_plugin_id: row.get(4)?,
                        workspace_id: row.get(5)?,
                        status,
                    })
                },
            )
            .context("load runtime run")
    }

    pub fn load_runtime_task(&self, task_id: &str) -> Result<RuntimeTask> {
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
SELECT
    task_id,
    agent_did,
    controller_user_id,
    controller_full_handle,
    controller_scope_key,
    controller_did,
    sender_did,
    requester_did,
    requester_full_handle,
    trigger_kind,
    reply_recipient_did,
    conversation_id,
    task_text
FROM runtime_task
WHERE task_id = ?1
"#,
                [task_id],
                |row| {
                    let trigger_raw: String = row.get(9)?;
                    let trigger_kind =
                        RuntimeTaskTriggerKind::parse(&trigger_raw).map_err(|err| {
                            rusqlite::Error::FromSqlConversionFailure(
                                trigger_raw.len(),
                                rusqlite::types::Type::Text,
                                Box::new(std::io::Error::new(
                                    std::io::ErrorKind::InvalidData,
                                    err.to_string(),
                                )),
                            )
                        })?;
                    Ok(RuntimeTask {
                        task_id: row.get(0)?,
                        agent_did: row.get(1)?,
                        controller_user_id: row.get(2)?,
                        controller_full_handle: row.get(3)?,
                        controller_scope_key: row.get(4)?,
                        controller_did: row.get(5)?,
                        sender_did: row.get(6)?,
                        requester_did: row.get(7)?,
                        requester_full_handle: row.get(8)?,
                        trigger_kind,
                        reply_recipient_did: row.get(10)?,
                        conversation_id: row.get(11)?,
                        text: row.get(12)?,
                    })
                },
            )
            .context("load runtime task")
    }

    pub fn load_runtime_task_for_run(&self, run_id: &str) -> Result<RuntimeTask> {
        let run = self.load_runtime_run(run_id)?;
        self.load_runtime_task(&run.task_id)
    }

    pub fn upsert_cli_driver_run(&self, record: &CliDriverRunRecord) -> Result<()> {
        record.validate()?;
        let connection = self.connection()?;
        let now = current_time_millis()?;
        connection.execute(
            r#"
INSERT INTO cli_driver_run (
    run_id,
    agent_did,
    runtime_profile_id,
    driver_id,
    controller_user_id,
    controller_full_handle,
    controller_scope_key,
    controller_did,
    conversation_id,
    route_key,
    workspace_id,
    workspace_root,
    workspace_instance_path,
    workspace_mode,
    is_security_boundary,
    command_json,
    output_json,
    final_output_path,
    native_session_id,
    synthetic_session_id,
    status,
    fallback_final_source,
    created_at_ms,
    updated_at_ms
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?23)
ON CONFLICT(run_id) DO UPDATE SET
    agent_did = excluded.agent_did,
    runtime_profile_id = excluded.runtime_profile_id,
    driver_id = excluded.driver_id,
    controller_user_id = excluded.controller_user_id,
    controller_full_handle = excluded.controller_full_handle,
    controller_scope_key = excluded.controller_scope_key,
    controller_did = excluded.controller_did,
    conversation_id = excluded.conversation_id,
    route_key = excluded.route_key,
    workspace_id = excluded.workspace_id,
    workspace_root = excluded.workspace_root,
    workspace_instance_path = excluded.workspace_instance_path,
    workspace_mode = excluded.workspace_mode,
    is_security_boundary = excluded.is_security_boundary,
    command_json = excluded.command_json,
    output_json = excluded.output_json,
    final_output_path = excluded.final_output_path,
    native_session_id = excluded.native_session_id,
    synthetic_session_id = excluded.synthetic_session_id,
    status = excluded.status,
    fallback_final_source = excluded.fallback_final_source,
    updated_at_ms = excluded.updated_at_ms
"#,
            rusqlite::params![
                record.run_id,
                record.agent_did,
                record.runtime_profile_id,
                record.driver_id,
                record.controller_user_id,
                record.controller_full_handle,
                record.controller_scope_key,
                record.controller_did,
                record.conversation_id,
                record.route_key,
                record.workspace_id,
                record.workspace_root.as_ref().map(|path| path.display().to_string()),
                record
                    .workspace_instance_path
                    .as_ref()
                    .map(|path| path.display().to_string()),
                record.workspace_mode.map(WorkspaceMode::as_str),
                if record.is_security_boundary { 1 } else { 0 },
                record.command_json.to_string(),
                record.output_json.to_string(),
                record
                    .final_output_path
                    .as_ref()
                    .map(|path| path.display().to_string()),
                record.native_session_id,
                record.synthetic_session_id,
                record.status,
                record.fallback_final_source,
                now,
            ],
        )?;
        Ok(())
    }

    pub fn load_cli_driver_run(&self, run_id: &str) -> Result<CliDriverRunRecord> {
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
SELECT
    run_id,
    agent_did,
    runtime_profile_id,
    driver_id,
    controller_user_id,
    controller_full_handle,
    controller_scope_key,
    controller_did,
    conversation_id,
    route_key,
    workspace_id,
    workspace_root,
    workspace_instance_path,
    workspace_mode,
    is_security_boundary,
    command_json,
    output_json,
    final_output_path,
    native_session_id,
    synthetic_session_id,
    status,
    fallback_final_source
FROM cli_driver_run
WHERE run_id = ?1
"#,
                [run_id],
                cli_driver_run_from_row,
            )
            .context("load cli driver run")
    }

    pub fn get_or_create_cli_route_session(
        &self,
        create: CreateCliRouteSession,
    ) -> Result<CliRouteSessionRecord> {
        let route_key = create.route_key()?;
        if let Some(existing) = self.load_cli_route_session(&route_key)? {
            validate_cli_route_session_fields(
                &create.agent_did,
                &create.runtime_profile_id,
                &create.driver_id,
                &create.controller_user_id,
                &create.controller_full_handle,
                &create.controller_scope_key,
                &create.controller_did,
                &canonical_cli_conversation_id(&create.conversation_id)?,
                &create.workspace_path,
                &create.session_dir,
                existing.status.as_str(),
            )?;
            if existing.runtime_profile_id != create.runtime_profile_id
                || existing.agent_did != create.agent_did
                || existing.driver_id != create.driver_id
                || existing.controller_user_id != create.controller_user_id
                || existing.controller_full_handle != create.controller_full_handle
                || existing.controller_scope_key != create.controller_scope_key
                || existing.conversation_id
                    != canonical_cli_conversation_id(&create.conversation_id)?
                || existing.workspace_path != create.workspace_path
                || existing.session_dir != create.session_dir
            {
                bail!("cli route session binding conflict for route {route_key}");
            }
            if existing.status != "reset" {
                return Ok(existing);
            }
            let connection = self.connection()?;
            connection.execute(
                r#"
UPDATE cli_route_sessions
SET status = 'active',
    native_session_id = NULL,
    native_session_source = NULL,
    synthetic_session_id = ?1,
    last_message_id = NULL,
    last_error_code = NULL,
    last_error_summary = NULL,
    version = version + 1,
    updated_at_ms = ?2
WHERE route_key = ?3
  AND status = 'reset'
"#,
                rusqlite::params![route_key, current_time_millis()?, route_key],
            )?;
            return self
                .load_cli_route_session(&route_key)?
                .with_context(|| format!("load cli route session {route_key}"));
        }

        let route_key_hash = self.cli_route_key_hash(&route_key)?;
        let record = create.into_record(route_key_hash)?;
        let connection = self.connection()?;
        let inserted = connection.execute(
            r#"
INSERT OR IGNORE INTO cli_route_sessions (
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
    native_session_id,
    native_session_source,
    synthetic_session_id,
    status,
    last_run_id,
    last_message_id,
    lock_run_id,
    lock_owner,
    lock_expires_at_ms,
    last_error_code,
    last_error_summary,
    version,
    created_at_ms,
    updated_at_ms
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, NULL, NULL, ?13, 'active', NULL, NULL, NULL, NULL, NULL, NULL, NULL, 0, ?14, ?15)
"#,
            rusqlite::params![
                record.route_key,
                record.route_key_hash,
                record.agent_did,
                record.runtime_profile_id,
                record.driver_id,
                record.controller_user_id,
                record.controller_full_handle,
                record.controller_scope_key,
                record.controller_did,
                record.conversation_id,
                record.workspace_path.display().to_string(),
                record.session_dir.display().to_string(),
                record.route_key,
                record.created_at_ms,
                record.updated_at_ms,
            ],
        )?;
        if inserted == 0 {
            if let Some(by_hash) = self.load_cli_route_session_by_profile_hash(
                &record.runtime_profile_id,
                &record.route_key_hash,
            )? {
                if by_hash.route_key != record.route_key {
                    bail!(
                        "cli route session hash conflict for profile {} hash {}",
                        record.runtime_profile_id,
                        record.route_key_hash
                    );
                }
            }
        }
        let mut loaded = self
            .load_cli_route_session(&record.route_key)?
            .with_context(|| format!("load cli route session {}", record.route_key))?;
        if loaded.runtime_profile_id != record.runtime_profile_id
            || loaded.route_key_hash != record.route_key_hash
            || loaded.agent_did != record.agent_did
            || loaded.driver_id != record.driver_id
            || loaded.controller_user_id != record.controller_user_id
            || loaded.controller_full_handle != record.controller_full_handle
            || loaded.controller_scope_key != record.controller_scope_key
            || loaded.conversation_id != record.conversation_id
            || loaded.workspace_path != record.workspace_path
            || loaded.session_dir != record.session_dir
        {
            bail!(
                "cli route session binding conflict for route {}",
                record.route_key
            );
        }
        if loaded.status == "reset" {
            connection.execute(
                r#"
UPDATE cli_route_sessions
SET status = 'active',
    native_session_id = NULL,
    native_session_source = NULL,
    synthetic_session_id = ?1,
    last_message_id = NULL,
    last_error_code = NULL,
    last_error_summary = NULL,
    version = version + 1,
    updated_at_ms = ?2
WHERE route_key = ?3
  AND status = 'reset'
"#,
                rusqlite::params![record.route_key, current_time_millis()?, record.route_key],
            )?;
            loaded = self
                .load_cli_route_session(&record.route_key)?
                .with_context(|| format!("load cli route session {}", record.route_key))?;
        }
        Ok(loaded)
    }

    pub fn load_cli_route_session(&self, route_key: &str) -> Result<Option<CliRouteSessionRecord>> {
        let connection = self.connection()?;
        connection
            .query_row(
                cli_route_session_select_sql("WHERE route_key = ?1").as_str(),
                [route_key],
                cli_route_session_from_row,
            )
            .optional()
            .context("load cli route session")
    }

    pub fn load_cli_route_session_by_profile_hash(
        &self,
        runtime_profile_id: &str,
        route_key_hash: &str,
    ) -> Result<Option<CliRouteSessionRecord>> {
        let connection = self.connection()?;
        connection
            .query_row(
                cli_route_session_select_sql(
                    "WHERE runtime_profile_id = ?1 AND route_key_hash = ?2",
                )
                .as_str(),
                rusqlite::params![runtime_profile_id, route_key_hash],
                cli_route_session_from_row,
            )
            .optional()
            .context("load cli route session by profile hash")
    }

    pub fn list_cli_route_sessions_for_runtime_profile(
        &self,
        agent_did: &str,
        runtime_profile_id: &str,
        controller_scope_key: &str,
        status: Option<&str>,
        conversation_id: Option<&str>,
        route_key_hash: Option<&str>,
        limit: usize,
    ) -> Result<Vec<CliRouteSessionRecord>> {
        if agent_did.trim().is_empty() {
            bail!("agent_did must not be empty");
        }
        if runtime_profile_id.trim().is_empty() {
            bail!("runtime_profile_id must not be empty");
        }
        if controller_scope_key.trim().is_empty() {
            bail!("controller_scope_key must not be empty");
        }
        if limit == 0 {
            bail!("limit must be greater than zero");
        }
        let conversation_id = conversation_id
            .map(canonical_cli_conversation_id)
            .transpose()?;
        if let Some(route_key_hash) = route_key_hash {
            validate_cli_route_key_hash(route_key_hash)?;
        }

        let mut where_clause =
            "WHERE agent_did = ? AND runtime_profile_id = ? AND controller_scope_key = ?"
                .to_string();
        let mut params: Vec<String> = vec![
            agent_did.to_string(),
            runtime_profile_id.to_string(),
            controller_scope_key.to_string(),
        ];
        if let Some(status) = status {
            if status.trim().is_empty() {
                bail!("status must not be empty when present");
            }
            where_clause.push_str(" AND status = ?");
            params.push(status.trim().to_string());
        }
        if let Some(conversation_id) = conversation_id {
            where_clause.push_str(" AND conversation_id = ?");
            params.push(conversation_id);
        }
        if let Some(route_key_hash) = route_key_hash {
            where_clause.push_str(" AND route_key_hash = ?");
            params.push(route_key_hash.trim().to_string());
        }
        where_clause.push_str(&format!(
            " ORDER BY updated_at_ms DESC, created_at_ms DESC LIMIT {limit}"
        ));

        let sql = cli_route_session_select_sql(&where_clause);
        let connection = self.connection()?;
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map(
            rusqlite::params_from_iter(params.iter().map(String::as_str)),
            cli_route_session_from_row,
        )?;
        let mut sessions = Vec::new();
        for row in rows {
            sessions.push(row?);
        }
        Ok(sessions)
    }

    pub fn count_cli_route_sessions_for_runtime_profile(
        &self,
        runtime_profile_id: &str,
        controller_scope_key: &str,
        status: Option<&str>,
    ) -> Result<usize> {
        if runtime_profile_id.trim().is_empty() {
            bail!("runtime_profile_id must not be empty");
        }
        if controller_scope_key.trim().is_empty() {
            bail!("controller_scope_key must not be empty");
        }
        let connection = self.connection()?;
        let count: i64 = if let Some(status) = status {
            connection.query_row(
                r#"
SELECT COUNT(*)
FROM cli_route_sessions
WHERE runtime_profile_id = ?1
  AND controller_scope_key = ?2
  AND status = ?3
"#,
                rusqlite::params![runtime_profile_id, controller_scope_key, status],
                |row| row.get(0),
            )?
        } else {
            connection.query_row(
                r#"
SELECT COUNT(*)
FROM cli_route_sessions
WHERE runtime_profile_id = ?1
  AND controller_scope_key = ?2
"#,
                rusqlite::params![runtime_profile_id, controller_scope_key],
                |row| row.get(0),
            )?
        };
        Ok(count as usize)
    }

    pub fn try_acquire_cli_route_session_lease(
        &self,
        route_key: &str,
        run_id: &str,
        lock_owner: &str,
        lock_expires_at_ms: i64,
    ) -> Result<bool> {
        for (field_name, value) in [
            ("route_key", route_key),
            ("run_id", run_id),
            ("lock_owner", lock_owner),
        ] {
            if value.trim().is_empty() {
                bail!("{field_name} must not be empty");
            }
        }
        let now = current_time_millis()?;
        let connection = self.connection()?;
        let updated = connection.execute(
            r#"
UPDATE cli_route_sessions
SET status = 'running',
    lock_run_id = ?1,
    lock_owner = ?2,
    lock_expires_at_ms = ?3,
    last_run_id = ?1,
    version = version + 1,
    updated_at_ms = ?4
WHERE route_key = ?5
  AND status IN ('active', 'failed', 'queued')
  AND (
      lock_run_id IS NULL
      OR lock_expires_at_ms IS NULL
      OR lock_expires_at_ms <= ?4
  )
"#,
            rusqlite::params![run_id, lock_owner, lock_expires_at_ms, now, route_key],
        )?;
        Ok(updated > 0)
    }

    pub fn release_cli_route_session_lease(
        &self,
        route_key: &str,
        run_id: &str,
        next_status: &str,
        last_message_id: Option<&str>,
        last_error_code: Option<&str>,
        last_error_summary: Option<&str>,
    ) -> Result<()> {
        if route_key.trim().is_empty() {
            bail!("route_key must not be empty");
        }
        if run_id.trim().is_empty() {
            bail!("run_id must not be empty");
        }
        if !matches!(
            next_status,
            "active" | "failed" | "queued" | "reset" | "archived"
        ) {
            bail!("unsupported cli route session status: {next_status}");
        }
        let connection = self.connection()?;
        let updated = connection.execute(
            r#"
UPDATE cli_route_sessions
SET status = ?1,
    lock_run_id = NULL,
    lock_owner = NULL,
    lock_expires_at_ms = NULL,
    last_message_id = COALESCE(?2, last_message_id),
    last_error_code = ?3,
    last_error_summary = ?4,
    version = version + 1,
    updated_at_ms = ?5
WHERE route_key = ?6
  AND lock_run_id = ?7
"#,
            rusqlite::params![
                next_status,
                last_message_id,
                last_error_code,
                last_error_summary,
                current_time_millis()?,
                route_key,
                run_id,
            ],
        )?;
        if updated == 0 {
            bail!("cli route session lease is not held by run {run_id}");
        }
        Ok(())
    }

    pub fn mark_cli_route_session_deferred(
        &self,
        route_key: &str,
        run_id: Option<&str>,
        last_error_code: &str,
        last_error_summary: &str,
    ) -> Result<()> {
        if route_key.trim().is_empty() {
            bail!("route_key must not be empty");
        }
        if last_error_code.trim().is_empty() {
            bail!("last_error_code must not be empty");
        }
        let connection = self.connection()?;
        let updated = connection.execute(
            r#"
UPDATE cli_route_sessions
SET status = 'queued',
    lock_run_id = NULL,
    lock_owner = NULL,
    lock_expires_at_ms = NULL,
    last_run_id = COALESCE(?1, last_run_id),
    last_error_code = ?2,
    last_error_summary = ?3,
    version = version + 1,
    updated_at_ms = ?4
WHERE route_key = ?5
"#,
            rusqlite::params![
                run_id,
                last_error_code,
                last_error_summary,
                current_time_millis()?,
                route_key,
            ],
        )?;
        if updated == 0 {
            bail!("cli route session does not exist: {route_key}");
        }
        Ok(())
    }

    pub fn update_cli_route_session_native_id(
        &self,
        route_key: &str,
        native_session_id: Option<&str>,
        native_session_source: Option<&str>,
        synthetic_session_id: Option<&str>,
    ) -> Result<()> {
        if route_key.trim().is_empty() {
            bail!("route_key must not be empty");
        }
        let connection = self.connection()?;
        let updated = connection.execute(
            r#"
UPDATE cli_route_sessions
SET native_session_id = ?1,
    native_session_source = ?2,
    synthetic_session_id = ?3,
    version = version + 1,
    updated_at_ms = ?4
WHERE route_key = ?5
"#,
            rusqlite::params![
                native_session_id,
                native_session_source,
                synthetic_session_id,
                current_time_millis()?,
                route_key,
            ],
        )?;
        if updated == 0 {
            bail!("cli route session does not exist: {route_key}");
        }
        Ok(())
    }

    pub fn update_cli_route_session_native_id_if_locked(
        &self,
        route_key: &str,
        run_id: &str,
        native_session_id: Option<&str>,
        native_session_source: Option<&str>,
        synthetic_session_id: Option<&str>,
    ) -> Result<()> {
        if route_key.trim().is_empty() {
            bail!("route_key must not be empty");
        }
        if run_id.trim().is_empty() {
            bail!("run_id must not be empty");
        }
        let connection = self.connection()?;
        let updated = connection.execute(
            r#"
UPDATE cli_route_sessions
SET native_session_id = ?1,
    native_session_source = ?2,
    synthetic_session_id = ?3,
    version = version + 1,
    updated_at_ms = ?4
WHERE route_key = ?5
  AND lock_run_id = ?6
  AND status = 'running'
"#,
            rusqlite::params![
                native_session_id,
                native_session_source,
                synthetic_session_id,
                current_time_millis()?,
                route_key,
                run_id,
            ],
        )?;
        if updated == 0 {
            bail!("cli route session lease is not held by run {run_id}");
        }
        Ok(())
    }

    pub fn mark_cli_route_session_failed(
        &self,
        route_key: &str,
        last_run_id: Option<&str>,
        last_error_code: &str,
        last_error_summary: &str,
    ) -> Result<()> {
        for (field_name, value) in [
            ("route_key", route_key),
            ("last_error_code", last_error_code),
            ("last_error_summary", last_error_summary),
        ] {
            if value.trim().is_empty() {
                bail!("{field_name} must not be empty");
            }
        }
        let connection = self.connection()?;
        let updated = connection.execute(
            r#"
UPDATE cli_route_sessions
SET status = 'failed',
    last_run_id = COALESCE(?1, last_run_id),
    lock_run_id = NULL,
    lock_owner = NULL,
    lock_expires_at_ms = NULL,
    last_error_code = ?2,
    last_error_summary = ?3,
    version = version + 1,
    updated_at_ms = ?4
WHERE route_key = ?5
"#,
            rusqlite::params![
                last_run_id,
                last_error_code,
                last_error_summary,
                current_time_millis()?,
                route_key,
            ],
        )?;
        if updated == 0 {
            bail!("cli route session does not exist: {route_key}");
        }
        Ok(())
    }

    pub fn reset_cli_route_session_by_route(&self, route_key: &str) -> Result<usize> {
        if route_key.trim().is_empty() {
            bail!("route_key must not be empty");
        }
        let connection = self.connection()?;
        let updated = connection.execute(
            r#"
UPDATE cli_route_sessions
SET status = 'reset',
    native_session_id = NULL,
    native_session_source = NULL,
    lock_run_id = NULL,
    lock_owner = NULL,
    lock_expires_at_ms = NULL,
    version = version + 1,
    updated_at_ms = ?1
WHERE route_key = ?2
  AND status IN ('active', 'running', 'failed')
"#,
            rusqlite::params![current_time_millis()?, route_key],
        )?;
        Ok(updated)
    }

    pub fn reset_cli_route_sessions_for_runtime_controller_scope(
        &self,
        agent_did: &str,
        runtime_profile_id: &str,
        controller_scope_key: &str,
    ) -> Result<usize> {
        for (field_name, value) in [
            ("agent_did", agent_did),
            ("runtime_profile_id", runtime_profile_id),
            ("controller_scope_key", controller_scope_key),
        ] {
            if value.trim().is_empty() {
                bail!("{field_name} must not be empty");
            }
        }
        let connection = self.connection()?;
        let updated = connection.execute(
            r#"
UPDATE cli_route_sessions
SET status = 'reset',
    native_session_id = NULL,
    native_session_source = NULL,
    lock_run_id = NULL,
    lock_owner = NULL,
    lock_expires_at_ms = NULL,
    version = version + 1,
    updated_at_ms = ?1
WHERE agent_did = ?2
  AND runtime_profile_id = ?3
  AND controller_scope_key = ?4
  AND status IN ('active', 'running', 'failed')
"#,
            rusqlite::params![
                current_time_millis()?,
                agent_did,
                runtime_profile_id,
                controller_scope_key,
            ],
        )?;
        Ok(updated)
    }

    pub fn try_acquire_cli_runtime_profile_lock(
        &self,
        runtime_profile_id: &str,
        driver_id: &str,
        run_id: &str,
        lock_owner: &str,
        lock_expires_at_ms: i64,
    ) -> Result<bool> {
        let lock_key = cli_runtime_profile_lock_key(runtime_profile_id)?;
        self.try_acquire_cli_runtime_lock(
            &lock_key,
            "profile",
            Some(runtime_profile_id),
            Some(driver_id),
            run_id,
            lock_owner,
            lock_expires_at_ms,
        )
    }

    pub fn try_acquire_cli_host_home_lock(
        &self,
        driver_id: &str,
        run_id: &str,
        lock_owner: &str,
        lock_expires_at_ms: i64,
    ) -> Result<bool> {
        let lock_key = cli_host_home_lock_key(driver_id)?;
        self.try_acquire_cli_runtime_lock(
            &lock_key,
            "host-home",
            None,
            Some(driver_id),
            run_id,
            lock_owner,
            lock_expires_at_ms,
        )
    }

    fn try_acquire_cli_runtime_lock(
        &self,
        lock_key: &str,
        lock_kind: &str,
        runtime_profile_id: Option<&str>,
        driver_id: Option<&str>,
        run_id: &str,
        lock_owner: &str,
        lock_expires_at_ms: i64,
    ) -> Result<bool> {
        for (field_name, value) in [
            ("lock_key", lock_key),
            ("lock_kind", lock_kind),
            ("run_id", run_id),
            ("lock_owner", lock_owner),
        ] {
            if value.trim().is_empty() {
                bail!("{field_name} must not be empty");
            }
        }
        if runtime_profile_id.is_some_and(|value| value.trim().is_empty()) {
            bail!("runtime_profile_id must not be empty when present");
        }
        if driver_id.is_some_and(|value| value.trim().is_empty()) {
            bail!("driver_id must not be empty when present");
        }
        let now = current_time_millis()?;
        let connection = self.connection()?;
        let updated = connection.execute(
            r#"
INSERT INTO cli_runtime_locks (
    lock_key,
    lock_kind,
    runtime_profile_id,
    driver_id,
    run_id,
    lock_owner,
    lock_expires_at_ms,
    created_at_ms,
    updated_at_ms
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
ON CONFLICT(lock_key) DO UPDATE SET
    lock_kind = excluded.lock_kind,
    runtime_profile_id = excluded.runtime_profile_id,
    driver_id = excluded.driver_id,
    run_id = excluded.run_id,
    lock_owner = excluded.lock_owner,
    lock_expires_at_ms = excluded.lock_expires_at_ms,
    updated_at_ms = excluded.updated_at_ms
WHERE cli_runtime_locks.lock_expires_at_ms <= excluded.updated_at_ms
   OR cli_runtime_locks.run_id = excluded.run_id
"#,
            rusqlite::params![
                lock_key,
                lock_kind,
                runtime_profile_id,
                driver_id,
                run_id,
                lock_owner,
                lock_expires_at_ms,
                now,
            ],
        )?;
        Ok(updated > 0)
    }

    pub fn release_cli_runtime_profile_lock(
        &self,
        runtime_profile_id: &str,
        run_id: &str,
    ) -> Result<bool> {
        let lock_key = cli_runtime_profile_lock_key(runtime_profile_id)?;
        self.release_cli_runtime_lock(&lock_key, run_id)
    }

    pub fn release_cli_host_home_lock(&self, driver_id: &str, run_id: &str) -> Result<bool> {
        let lock_key = cli_host_home_lock_key(driver_id)?;
        self.release_cli_runtime_lock(&lock_key, run_id)
    }

    fn release_cli_runtime_lock(&self, lock_key: &str, run_id: &str) -> Result<bool> {
        if lock_key.trim().is_empty() {
            bail!("lock_key must not be empty");
        }
        if run_id.trim().is_empty() {
            bail!("run_id must not be empty");
        }
        let connection = self.connection()?;
        let updated = connection.execute(
            r#"
DELETE FROM cli_runtime_locks
WHERE lock_key = ?1
  AND run_id = ?2
"#,
            rusqlite::params![lock_key, run_id],
        )?;
        Ok(updated > 0)
    }

    pub fn count_cli_runtime_locks(
        &self,
        lock_kind: Option<&str>,
        runtime_profile_id: Option<&str>,
        driver_id: Option<&str>,
        include_expired: bool,
    ) -> Result<usize> {
        let now = current_time_millis()?;
        let connection = self.connection()?;
        let mut where_clause = "WHERE 1 = 1".to_string();
        let mut params: Vec<String> = Vec::new();
        if let Some(lock_kind) = lock_kind {
            if lock_kind.trim().is_empty() {
                bail!("lock_kind must not be empty when present");
            }
            where_clause.push_str(" AND lock_kind = ?");
            params.push(lock_kind.trim().to_string());
        }
        if let Some(runtime_profile_id) = runtime_profile_id {
            if runtime_profile_id.trim().is_empty() {
                bail!("runtime_profile_id must not be empty when present");
            }
            where_clause.push_str(" AND runtime_profile_id = ?");
            params.push(runtime_profile_id.trim().to_string());
        }
        if let Some(driver_id) = driver_id {
            if driver_id.trim().is_empty() {
                bail!("driver_id must not be empty when present");
            }
            where_clause.push_str(" AND driver_id = ?");
            params.push(driver_id.trim().to_string());
        }
        if !include_expired {
            where_clause.push_str(" AND lock_expires_at_ms > ?");
            params.push(now.to_string());
        }
        let sql = format!("SELECT COUNT(*) FROM cli_runtime_locks {where_clause}");
        let count: i64 = connection.query_row(
            &sql,
            rusqlite::params_from_iter(params.iter().map(String::as_str)),
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    pub fn load_cli_runtime_lock(&self, lock_key: &str) -> Result<Option<CliRuntimeLockRecord>> {
        if lock_key.trim().is_empty() {
            bail!("lock_key must not be empty");
        }
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
SELECT
    lock_key,
    lock_kind,
    runtime_profile_id,
    driver_id,
    run_id,
    lock_owner,
    lock_expires_at_ms,
    created_at_ms,
    updated_at_ms
FROM cli_runtime_locks
WHERE lock_key = ?1
"#,
                [lock_key],
                cli_runtime_lock_from_row,
            )
            .optional()
            .context("load cli runtime lock")
    }
}

pub fn cli_runtime_profile_lock_key(runtime_profile_id: &str) -> Result<String> {
    if runtime_profile_id.trim().is_empty() {
        bail!("runtime_profile_id must not be empty");
    }
    Ok(format!("profile:{}", runtime_profile_id.trim()))
}

pub fn cli_host_home_lock_key(driver_id: &str) -> Result<String> {
    if driver_id.trim().is_empty() {
        bail!("driver_id must not be empty");
    }
    Ok(format!("host-home:{}:default", driver_id.trim()))
}

fn cli_route_session_select_sql(where_clause: &str) -> String {
    format!(
        r#"
SELECT
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
    native_session_id,
    native_session_source,
    synthetic_session_id,
    status,
    last_run_id,
    last_message_id,
    lock_run_id,
    lock_owner,
    lock_expires_at_ms,
    last_error_code,
    last_error_summary,
    version,
    created_at_ms,
    updated_at_ms
FROM cli_route_sessions
{where_clause}
"#
    )
}
