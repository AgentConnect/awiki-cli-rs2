use super::row_mappers::*;
use super::*;
use crate::runtime::{
    RuntimeConversationScope, RuntimeInvocationAuthority, RuntimeTaskTriggerKind,
};

impl DaemonState {
    pub fn insert_runtime_task(&self, task: &RuntimeTask) -> Result<()> {
        task.validate()?;
        let connection = self.connection()?;
        let now = current_time_millis()?;
        connection.execute(
            r#"
INSERT INTO runtime_task (
    task_id,
    agent_did,
    agent_handle,
    controller_user_id,
    controller_full_handle,
    controller_scope_key,
    controller_did,
    sender_did,
    requester_did,
    requester_user_id,
    requester_full_handle,
    trigger_kind,
    conversation_scope_kind,
    conversation_scope_key,
    invocation_authority,
    reply_recipient_did,
    conversation_id,
    task_text,
    status,
    created_at_ms,
    updated_at_ms
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, 'created', ?19, ?19)
ON CONFLICT(task_id) DO UPDATE SET
    task_text = excluded.task_text,
    agent_handle = excluded.agent_handle,
    sender_did = excluded.sender_did,
    requester_did = excluded.requester_did,
    requester_user_id = excluded.requester_user_id,
    requester_full_handle = excluded.requester_full_handle,
    trigger_kind = excluded.trigger_kind,
    conversation_scope_kind = excluded.conversation_scope_kind,
    conversation_scope_key = excluded.conversation_scope_key,
    invocation_authority = excluded.invocation_authority,
    reply_recipient_did = excluded.reply_recipient_did,
    conversation_id = excluded.conversation_id,
    updated_at_ms = excluded.updated_at_ms
"#,
            rusqlite::params![
                task.task_id,
                task.agent_did,
                task.agent_handle,
                task.controller_user_id,
                task.controller_full_handle,
                task.controller_scope_key,
                task.controller_did,
                task.sender_did,
                task.requester_did,
                task.requester_user_id,
                task.requester_full_handle,
                task.trigger_kind.as_str(),
                task.conversation_scope.kind_str(),
                task.conversation_scope.scope_key(),
                task.invocation_authority.as_str(),
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
        if original_run.status != RuntimeRunStatus::Failed {
            bail!("only failed runs can be retried");
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
    created_at_ms,
    updated_at_ms
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'queued', ?8, 0, ?9, ?9)
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

    pub fn list_queued_runtime_retries(
        &self,
        limit: usize,
    ) -> Result<Vec<RuntimeRetryQueueRecord>> {
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
    created_at_ms,
    updated_at_ms
FROM runtime_retry_queue
WHERE status = 'queued'
ORDER BY created_at_ms ASC, retry_id ASC
LIMIT ?1
"#,
        )?;
        let rows =
            statement.query_map([limit.max(1) as i64], runtime_retry_queue_record_from_row)?;
        let mut retries = Vec::new();
        for row in rows {
            retries.push(row?);
        }
        Ok(retries)
    }

    pub fn mark_runtime_retry_status(&self, retry_id: &str, status: &str) -> Result<()> {
        if retry_id.trim().is_empty() {
            bail!("retry_id must not be empty");
        }
        validate_runtime_retry_status(status)?;
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

    pub fn mark_runtime_retry_status_if_status_in(
        &self,
        retry_id: &str,
        current_statuses: &[&str],
        status: &str,
    ) -> Result<bool> {
        if retry_id.trim().is_empty() {
            bail!("retry_id must not be empty");
        }
        if current_statuses.is_empty() {
            bail!("current_statuses must not be empty");
        }
        for current_status in current_statuses {
            validate_runtime_retry_status(current_status)?;
        }
        validate_runtime_retry_status(status)?;

        let mut sql = r#"
UPDATE runtime_retry_queue
SET status = ?1,
    attempts = attempts + CASE WHEN ?1 = 'running' THEN 1 ELSE 0 END,
    updated_at_ms = ?2
WHERE retry_id = ?3
  AND status IN (
"#
        .to_string();
        sql.push_str(
            &std::iter::repeat("?")
                .take(current_statuses.len())
                .collect::<Vec<_>>()
                .join(", "),
        );
        sql.push_str(")\n");

        let updated_at_ms = current_time_millis()?;
        let connection = self.connection()?;
        let mut values: Vec<&dyn rusqlite::ToSql> = vec![&status, &updated_at_ms, &retry_id];
        for current_status in current_statuses {
            values.push(current_status);
        }
        let updated = connection.execute(&sql, values.as_slice())?;
        Ok(updated > 0)
    }

    pub fn recover_stale_runtime_retries_running(&self, stale_before_ms: i64) -> Result<usize> {
        let connection = self.connection()?;
        let updated = connection.execute(
            r#"
UPDATE runtime_retry_queue
SET status = 'queued',
    updated_at_ms = ?1
WHERE status = 'running'
  AND updated_at_ms <= ?2
"#,
            rusqlite::params![current_time_millis()?, stale_before_ms],
        )?;
        Ok(updated)
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
    final_text = CASE WHEN runtime_final_outbox.status IN ('sent', 'failed_terminal') THEN runtime_final_outbox.final_text ELSE excluded.final_text END,
    security = CASE WHEN runtime_final_outbox.status IN ('sent', 'failed_terminal') THEN runtime_final_outbox.security ELSE excluded.security END,
    recipient_did = CASE WHEN runtime_final_outbox.status IN ('sent', 'failed_terminal') THEN runtime_final_outbox.recipient_did ELSE excluded.recipient_did END,
    conversation_id = CASE WHEN runtime_final_outbox.status IN ('sent', 'failed_terminal') THEN runtime_final_outbox.conversation_id ELSE excluded.conversation_id END,
    status = CASE WHEN runtime_final_outbox.status IN ('sent', 'failed_terminal') THEN runtime_final_outbox.status ELSE 'pending' END,
    next_attempt_at_ms = CASE WHEN runtime_final_outbox.status IN ('sent', 'failed_terminal') THEN runtime_final_outbox.next_attempt_at_ms ELSE excluded.next_attempt_at_ms END,
    last_error_code = CASE WHEN runtime_final_outbox.status IN ('sent', 'failed_terminal') THEN runtime_final_outbox.last_error_code ELSE NULL END,
    last_error_summary = CASE WHEN runtime_final_outbox.status IN ('sent', 'failed_terminal') THEN runtime_final_outbox.last_error_summary ELSE NULL END,
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

    pub fn update_runtime_run_status_if_status_in(
        &self,
        run_id: &str,
        current_statuses: &[RuntimeRunStatus],
        status: RuntimeRunStatus,
    ) -> Result<bool> {
        if run_id.trim().is_empty() {
            bail!("run_id must not be empty");
        }
        if current_statuses.is_empty() {
            bail!("current_statuses must not be empty");
        }

        let now = current_time_millis()?;
        let completed_at = match status {
            RuntimeRunStatus::Finished | RuntimeRunStatus::Failed => Some(now.to_string()),
            RuntimeRunStatus::Pending | RuntimeRunStatus::Running => None,
        };
        let completed_at_ms = match status {
            RuntimeRunStatus::Finished | RuntimeRunStatus::Failed => Some(now),
            RuntimeRunStatus::Pending | RuntimeRunStatus::Running => None,
        };
        let mut sql = r#"
UPDATE runtime_run
SET status = ?1,
    completed_at = COALESCE(?2, completed_at),
    updated_at = ?3,
    completed_at_ms = COALESCE(?4, completed_at_ms),
    updated_at_ms = ?5
WHERE run_id = ?6
  AND status IN (
"#
        .to_string();
        sql.push_str(
            &std::iter::repeat("?")
                .take(current_statuses.len())
                .collect::<Vec<_>>()
                .join(", "),
        );
        sql.push_str(")\n");

        let status_value = status.as_str();
        let updated_at = now.to_string();
        let current_status_values = current_statuses
            .iter()
            .map(RuntimeRunStatus::as_str)
            .collect::<Vec<_>>();
        let connection = self.connection()?;
        let mut values: Vec<&dyn rusqlite::ToSql> = vec![
            &status_value,
            &completed_at,
            &updated_at,
            &completed_at_ms,
            &now,
            &run_id,
        ];
        for current_status in current_status_values.iter() {
            values.push(current_status);
        }
        let updated = connection.execute(&sql, values.as_slice())?;
        if updated == 0 {
            let exists: bool = connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM runtime_run WHERE run_id = ?1)",
                [run_id],
                |row| row.get(0),
            )?;
            if !exists {
                bail!("runtime run does not exist: {run_id}");
            }
        }
        Ok(updated > 0)
    }

    pub fn recover_stale_active_runtime_runs(&self, stale_before_ms: i64) -> Result<usize> {
        let connection = self.connection()?;
        let now = current_time_millis()?;
        let updated = connection.execute(
            r#"
UPDATE runtime_run
SET status = 'failed',
    completed_at = COALESCE(completed_at, ?1),
    updated_at = ?1,
    completed_at_ms = COALESCE(completed_at_ms, ?2),
    updated_at_ms = ?2
WHERE status IN ('pending', 'running')
  AND updated_at_ms <= ?3
"#,
            rusqlite::params![now.to_string(), now, stale_before_ms],
        )?;
        Ok(updated)
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
    agent_handle,
    controller_user_id,
    controller_full_handle,
    controller_scope_key,
    controller_did,
    sender_did,
    requester_did,
    requester_user_id,
    requester_full_handle,
    trigger_kind,
    conversation_scope_kind,
    conversation_scope_key,
    invocation_authority,
    reply_recipient_did,
    conversation_id,
    task_text
FROM runtime_task
WHERE task_id = ?1
"#,
                [task_id],
                |row| {
                    let trigger_raw: String = row.get(11)?;
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
                    let scope_kind_raw: String = row.get(12)?;
                    let scope_key: String = row.get(13)?;
                    let conversation_scope =
                        conversation_scope_from_storage(&scope_kind_raw, &scope_key).map_err(
                            |err| {
                                rusqlite::Error::FromSqlConversionFailure(
                                    scope_kind_raw.len() + scope_key.len(),
                                    rusqlite::types::Type::Text,
                                    Box::new(std::io::Error::new(
                                        std::io::ErrorKind::InvalidData,
                                        err.to_string(),
                                    )),
                                )
                            },
                        )?;
                    let authority_raw: String = row.get(14)?;
                    let invocation_authority = RuntimeInvocationAuthority::parse(&authority_raw)
                        .map_err(|err| {
                            rusqlite::Error::FromSqlConversionFailure(
                                authority_raw.len(),
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
                        agent_handle: row.get(2)?,
                        controller_user_id: row.get(3)?,
                        controller_full_handle: row.get(4)?,
                        controller_scope_key: row.get(5)?,
                        controller_did: row.get(6)?,
                        sender_did: row.get(7)?,
                        requester_did: row.get(8)?,
                        requester_user_id: row.get(9)?,
                        requester_full_handle: row.get(10)?,
                        trigger_kind,
                        conversation_scope,
                        invocation_authority,
                        reply_recipient_did: row.get(15)?,
                        conversation_id: row.get(16)?,
                        text: row.get(17)?,
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

    pub fn load_latest_cli_driver_run_by_route(
        &self,
        route_key: &str,
    ) -> Result<Option<CliDriverRunRecord>> {
        if route_key.trim().is_empty() {
            bail!("route_key must not be empty");
        }
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
WHERE route_key = ?1
  AND status = 'finished'
ORDER BY updated_at_ms DESC, created_at_ms DESC
LIMIT 1
"#,
                [route_key],
                cli_driver_run_from_row,
            )
            .optional()
            .context("load latest cli driver run by route")
    }
}

fn conversation_scope_from_storage(kind: &str, key: &str) -> Result<RuntimeConversationScope> {
    match kind {
        "controller_private" => {
            let controller_scope_key = key
                .strip_prefix("controller:")
                .unwrap_or(key)
                .trim()
                .to_string();
            Ok(RuntimeConversationScope::controller_private(
                controller_scope_key,
            ))
        }
        "direct" => {
            let Some(rest) = key.strip_prefix("user:") else {
                bail!("direct runtime conversation scope key must start with user:");
            };
            let Some((requester_user_id, requester_full_handle)) = rest.split_once(":handle:")
            else {
                bail!("direct runtime conversation scope key must include handle");
            };
            RuntimeConversationScope::direct(
                requester_user_id.to_string(),
                requester_full_handle.to_string(),
            )
        }
        "group_visible" => {
            let group_key = key.strip_prefix("group:").unwrap_or(key).trim().to_string();
            Ok(RuntimeConversationScope::group_visible(group_key))
        }
        other => bail!("unsupported runtime conversation scope kind: {other}"),
    }
}
