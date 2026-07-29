use super::row_mappers::*;
use super::*;
use im_core::vault::{
    SealSecretRequest, SecretAccessPolicy, SecretBytes, SecretKind, SecretMetadata, SecretRef,
};

const VAULT_PRIVATE_KEY_SENTINEL: &str = "<awiki-secret-vault-ref>";

impl DaemonState {
    pub fn store_agent_identity(&self, identity: &AgentIdentityRecord) -> Result<()> {
        if identity.agent_did.trim().is_empty() {
            bail!("agent_did must not be empty");
        }
        if identity.handle.trim().is_empty() {
            bail!("handle must not be empty");
        }
        let refs = self.seal_agent_identity_private_keys(identity)?;
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
    auth_private_key_ref_json,
    e2ee_signing_private_key_ref_json,
    e2ee_agreement_private_key_ref_json,
    created_at,
    updated_at
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?14)
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
    auth_private_key_ref_json = excluded.auth_private_key_ref_json,
    e2ee_signing_private_key_ref_json = excluded.e2ee_signing_private_key_ref_json,
    e2ee_agreement_private_key_ref_json = excluded.e2ee_agreement_private_key_ref_json,
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
                VAULT_PRIVATE_KEY_SENTINEL,
                if refs.e2ee_signing_private_key_ref_json.is_some() {
                    VAULT_PRIVATE_KEY_SENTINEL
                } else {
                    ""
                },
                if refs.e2ee_agreement_private_key_ref_json.is_some() {
                    VAULT_PRIVATE_KEY_SENTINEL
                } else {
                    ""
                },
                refs.auth_private_key_ref_json,
                refs.e2ee_signing_private_key_ref_json,
                refs.e2ee_agreement_private_key_ref_json,
                now,
            ],
        )?;
        Ok(())
    }

    pub fn load_agent_identity(&self, agent_did: &str) -> Result<AgentIdentityRecord> {
        let connection = self.connection()?;
        let row = connection
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
    e2ee_agreement_private_key_pem,
    auth_private_key_ref_json,
    e2ee_signing_private_key_ref_json,
    e2ee_agreement_private_key_ref_json
FROM agent_identity
WHERE agent_did = ?1
"#,
                [agent_did],
                agent_identity_storage_row_from_row,
            )
            .with_context(|| format!("load agent identity {agent_did}"))?;
        let identity = self.agent_identity_from_storage_row(row)?;
        Ok(identity)
    }

    pub fn store_bootstrap_state(
        &self,
        identity: &UserDelegatedIdentityRecord,
        replay: &BootstrapReplayRecord,
    ) -> Result<BootstrapStoreOutcome> {
        self.store_bootstrap_state_with_payload_hash_alias(identity, replay, None)
    }

    pub(crate) fn store_bootstrap_state_with_legacy_payload_hash(
        &self,
        identity: &UserDelegatedIdentityRecord,
        replay: &BootstrapReplayRecord,
        legacy_payload_hash: &str,
    ) -> Result<BootstrapStoreOutcome> {
        self.store_bootstrap_state_with_payload_hash_alias(
            identity,
            replay,
            Some(legacy_payload_hash),
        )
    }

    fn store_bootstrap_state_with_payload_hash_alias(
        &self,
        identity: &UserDelegatedIdentityRecord,
        replay: &BootstrapReplayRecord,
        legacy_payload_hash: Option<&str>,
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
            let payload_hash_matches = existing.payload_hash == replay.payload_hash
                || legacy_payload_hash
                    .is_some_and(|legacy_hash| existing.payload_hash == legacy_hash);
            if !payload_hash_matches {
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
        let private_key_ref_json = self.seal_user_delegated_private_key(identity)?;
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
    private_key_ref_json,
    allowed_scopes_json,
    status,
    expires_at,
    bootstrap_id,
    idempotency_key,
    created_at_ms,
    updated_at_ms
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?14)
ON CONFLICT(verification_method) DO UPDATE SET
    user_did = excluded.user_did,
    app_instance_id = excluded.app_instance_id,
    controller_did = excluded.controller_did,
    daemon_agent_did = excluded.daemon_agent_did,
    public_key_multibase = excluded.public_key_multibase,
    private_key_material = excluded.private_key_material,
    private_key_ref_json = excluded.private_key_ref_json,
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
                VAULT_PRIVATE_KEY_SENTINEL,
                private_key_ref_json,
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
        let stored = connection
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
    private_key_ref_json,
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
            .context("load user delegated identity")?;
        let Some(stored) = stored else {
            return Ok(None);
        };
        let identity = self.user_delegated_identity_from_storage_record(stored)?;
        Ok(Some(identity))
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

    pub fn store_secure_bootstrap_replay(
        &self,
        replay: &SecureBootstrapReplayRecord,
    ) -> Result<BootstrapStoreOutcome> {
        replay.validate()?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(existing) = load_secure_bootstrap_replay_by_operation_or_nonce(
            &transaction,
            &replay.operation_id,
            &replay.nonce,
        )? {
            if existing.envelope_hash != replay.envelope_hash
                || existing.operation_id != replay.operation_id
                || existing.nonce != replay.nonce
                || existing.recipient_daemon_did != replay.recipient_daemon_did
                || existing.recipient_key_id != replay.recipient_key_id
                || existing.sender_human_did != replay.sender_human_did
                || existing.bootstrap_id != replay.bootstrap_id
                || existing.idempotency_key != replay.idempotency_key
            {
                bail!("secure daemon bootstrap replay conflict");
            }
            return Ok(BootstrapStoreOutcome::Duplicate);
        }
        let now = current_time_millis()?;
        transaction.execute(
            r#"
INSERT INTO secure_bootstrap_replay (
    operation_id,
    nonce,
    envelope_hash,
    recipient_daemon_did,
    recipient_key_id,
    sender_human_did,
    bootstrap_id,
    idempotency_key,
    payload_sha256,
    expires_at,
    status,
    created_at_ms,
    updated_at_ms
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12)
"#,
            rusqlite::params![
                &replay.operation_id,
                &replay.nonce,
                &replay.envelope_hash,
                &replay.recipient_daemon_did,
                &replay.recipient_key_id,
                &replay.sender_human_did,
                &replay.bootstrap_id,
                &replay.idempotency_key,
                &replay.payload_sha256,
                &replay.expires_at,
                &replay.status,
                now,
            ],
        )?;
        transaction.commit()?;
        Ok(BootstrapStoreOutcome::Inserted)
    }

    pub fn load_secure_bootstrap_replay(
        &self,
        operation_id: &str,
    ) -> Result<Option<SecureBootstrapReplayRecord>> {
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
SELECT
    operation_id,
    nonce,
    envelope_hash,
    recipient_daemon_did,
    recipient_key_id,
    sender_human_did,
    bootstrap_id,
    idempotency_key,
    payload_sha256,
    expires_at,
    status,
    created_at_ms,
    updated_at_ms
FROM secure_bootstrap_replay
WHERE operation_id = ?1
"#,
                [operation_id],
                secure_bootstrap_replay_from_row,
            )
            .optional()
            .context("load secure bootstrap replay")
    }

    pub fn upsert_app_personal_agent_binding(
        &self,
        record: &AppPersonalAgentBindingRecord,
    ) -> Result<()> {
        record.validate()?;
        let connection = self.connection()?;
        let now = current_time_millis()?;
        connection.execute(
            r#"
INSERT INTO app_personal_agent_binding (
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

    pub fn revoke_other_active_app_personal_agent_bindings(
        &self,
        user_did: &str,
        role: &str,
        keep_binding_id: &str,
    ) -> Result<usize> {
        let connection = self.connection()?;
        let now = current_time_millis()?;
        let affected = connection.execute(
            r#"
UPDATE app_personal_agent_binding
SET revoked_at_ms = ?1,
    updated_at_ms = ?1
WHERE user_did = ?2
  AND role = ?3
  AND binding_id <> ?4
  AND revoked_at_ms IS NULL
  AND status IN ('personal_agent_ready', 'personal_agent_active', 'personal_agent_ensuring')
"#,
            rusqlite::params![now, user_did, role, keep_binding_id],
        )?;
        Ok(affected)
    }

    pub fn load_active_app_personal_agent_binding(
        &self,
        user_did: &str,
        app_instance_id: &str,
        role: &str,
    ) -> Result<Option<AppPersonalAgentBindingRecord>> {
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
FROM app_personal_agent_binding
WHERE user_did = ?1
  AND app_instance_id = ?2
  AND role = ?3
  AND revoked_at_ms IS NULL
  AND status IN ('personal_agent_ready', 'personal_agent_active', 'personal_agent_ensuring')
ORDER BY updated_at_ms DESC
LIMIT 1
"#,
                rusqlite::params![user_did, app_instance_id, role],
                app_personal_agent_binding_from_row,
            )
            .optional()
            .context("load active app personal agent binding")
    }

    pub fn load_app_personal_agent_binding(
        &self,
        binding_id: &str,
    ) -> Result<Option<AppPersonalAgentBindingRecord>> {
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
FROM app_personal_agent_binding
WHERE binding_id = ?1
"#,
                [binding_id],
                app_personal_agent_binding_from_row,
            )
            .optional()
            .context("load app personal agent binding")
    }

    pub fn update_app_personal_agent_binding_status_by_runtime(
        &self,
        runtime_agent_did: &str,
        status: &str,
        revoked: bool,
    ) -> Result<Option<AppPersonalAgentBindingRecord>> {
        let runtime_agent_did = runtime_agent_did.trim();
        if runtime_agent_did.is_empty() {
            anyhow::bail!("runtime_agent_did must not be empty");
        }
        let status = status.trim();
        if status.is_empty() {
            anyhow::bail!("status must not be empty");
        }
        let connection = self.connection()?;
        let now = current_time_millis()?;
        let affected = connection.execute(
            r#"
UPDATE app_personal_agent_binding
SET status = ?1,
    updated_at_ms = ?2,
    revoked_at_ms = CASE WHEN ?3 THEN COALESCE(revoked_at_ms, ?2) ELSE revoked_at_ms END
WHERE runtime_agent_did = ?4
  AND revoked_at_ms IS NULL
  AND status IN ('personal_agent_ready', 'personal_agent_active', 'personal_agent_ensuring')
"#,
            rusqlite::params![status, now, revoked, runtime_agent_did],
        )?;
        if affected == 0 {
            return Ok(None);
        }
        self.load_active_or_inactive_app_personal_agent_binding_by_runtime(runtime_agent_did)
    }

    pub fn load_active_or_inactive_app_personal_agent_binding_by_runtime(
        &self,
        runtime_agent_did: &str,
    ) -> Result<Option<AppPersonalAgentBindingRecord>> {
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
FROM app_personal_agent_binding
WHERE runtime_agent_did = ?1
ORDER BY updated_at_ms DESC
LIMIT 1
"#,
                [runtime_agent_did],
                app_personal_agent_binding_from_row,
            )
            .optional()
            .context("load app personal agent binding by runtime")
    }

    pub fn load_active_app_personal_agent_binding_by_runtime(
        &self,
        runtime_agent_did: &str,
    ) -> Result<Option<AppPersonalAgentBindingRecord>> {
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
FROM app_personal_agent_binding
WHERE runtime_agent_did = ?1
  AND revoked_at_ms IS NULL
  AND status IN ('personal_agent_ready', 'personal_agent_active', 'personal_agent_ensuring')
ORDER BY updated_at_ms DESC
LIMIT 1
"#,
                [runtime_agent_did],
                app_personal_agent_binding_from_row,
            )
            .optional()
            .context("load active app personal agent binding by runtime")
    }

    pub fn list_active_app_personal_agent_bindings(
        &self,
    ) -> Result<Vec<AppPersonalAgentBindingRecord>> {
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
FROM app_personal_agent_binding
WHERE revoked_at_ms IS NULL
  AND status IN ('personal_agent_ready', 'personal_agent_active', 'personal_agent_ensuring')
ORDER BY updated_at_ms ASC
"#,
        )?;
        let rows = statement.query_map([], app_personal_agent_binding_from_row)?;
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

    pub fn next_pending_message_sync_outbox_due_ms(&self) -> Result<Option<i64>> {
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
SELECT MIN(next_attempt_at_ms)
FROM message_sync_outbox
WHERE status = 'pending'
"#,
                [],
                |row| row.get(0),
            )
            .context("load next pending message sync outbox due time")
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
        let jwt_token_ref_json = self.seal_agent_auth_token(agent_did, jwt_token)?;
        let connection = self.connection()?;
        let now = current_time_millis()?;
        connection.execute(
            r#"
INSERT INTO agent_auth_state (
    agent_did,
    jwt_token,
    jwt_token_ref_json,
    updated_at_ms
) VALUES (?1, ?2, ?3, ?4)
ON CONFLICT(agent_did) DO UPDATE SET
    jwt_token = excluded.jwt_token,
    jwt_token_ref_json = excluded.jwt_token_ref_json,
    updated_at_ms = excluded.updated_at_ms
"#,
            rusqlite::params![
                agent_did,
                VAULT_PRIVATE_KEY_SENTINEL,
                jwt_token_ref_json,
                now
            ],
        )?;
        Ok(())
    }

    pub fn load_agent_auth_token(&self, agent_did: &str) -> Result<Option<String>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT jwt_token, jwt_token_ref_json FROM agent_auth_state WHERE agent_did = ?1",
        )?;
        let mut rows = statement.query([agent_did])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        let jwt_token: String = row.get(0)?;
        let jwt_token_ref_json: Option<String> = row.get(1)?;
        Ok(Some(self.open_agent_auth_token(
            jwt_token_ref_json.as_deref(),
            &jwt_token,
        )?))
    }

    pub fn list_agent_auth_tokens(&self) -> Result<Vec<(String, String)>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            r#"
SELECT agent_did, jwt_token, jwt_token_ref_json
FROM agent_auth_state
ORDER BY agent_did ASC
"#,
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })?;
        let mut tokens = Vec::new();
        for row in rows {
            let (agent_did, jwt_token, jwt_token_ref_json) = row?;
            tokens.push((
                agent_did,
                self.open_agent_auth_token(jwt_token_ref_json.as_deref(), &jwt_token)?,
            ));
        }
        Ok(tokens)
    }
}

struct AgentIdentitySecretRefsJson {
    auth_private_key_ref_json: String,
    e2ee_signing_private_key_ref_json: Option<String>,
    e2ee_agreement_private_key_ref_json: Option<String>,
}

impl DaemonState {
    fn seal_agent_identity_private_keys(
        &self,
        identity: &AgentIdentityRecord,
    ) -> Result<AgentIdentitySecretRefsJson> {
        let vault = self.secret_vault().context(
            "daemon secret vault root key is required to store agent identity private keys; refusing plaintext fallback",
        )?;
        let auth_private_key_ref_json = seal_agent_identity_secret(
            vault,
            identity,
            SecretKind::IdentityDaemonPrivate,
            "auth",
            &identity.auth_private_key_pem,
            true,
        )?
        .context("auth private key ref missing after seal")?;
        let e2ee_signing_private_key_ref_json = seal_agent_identity_secret(
            vault,
            identity,
            SecretKind::IdentityE2eeSigningPrivate,
            "e2ee-signing",
            &identity.e2ee_signing_private_key_pem,
            false,
        )?;
        let e2ee_agreement_private_key_ref_json = seal_agent_identity_secret(
            vault,
            identity,
            SecretKind::IdentityE2eeAgreementPrivate,
            "e2ee-agreement",
            &identity.e2ee_agreement_private_key_pem,
            false,
        )?;
        Ok(AgentIdentitySecretRefsJson {
            auth_private_key_ref_json,
            e2ee_signing_private_key_ref_json,
            e2ee_agreement_private_key_ref_json,
        })
    }

    fn agent_identity_from_storage_row(
        &self,
        row: AgentIdentityStorageRow,
    ) -> Result<AgentIdentityRecord> {
        let auth_opened = self.open_agent_identity_secret(
            row.auth_private_key_ref_json.as_deref(),
            &row.auth_private_key_pem,
            "auth_private_key_ref_json",
        )?;
        let signing_legacy = row.e2ee_signing_private_key_pem.unwrap_or_default();
        let signing_opened = self.open_agent_identity_secret(
            row.e2ee_signing_private_key_ref_json.as_deref(),
            &signing_legacy,
            "e2ee_signing_private_key_ref_json",
        )?;
        let agreement_legacy = row.e2ee_agreement_private_key_pem.unwrap_or_default();
        let agreement_opened = self.open_agent_identity_secret(
            row.e2ee_agreement_private_key_ref_json.as_deref(),
            &agreement_legacy,
            "e2ee_agreement_private_key_ref_json",
        )?;
        Ok(AgentIdentityRecord {
            agent_did: row.agent_did,
            handle: row.handle,
            agent_kind: row.agent_kind,
            did_document: row.did_document,
            endpoint_url: row.endpoint_url,
            key_algorithm: row.key_algorithm,
            public_key: row.public_key,
            auth_private_key_pem: auth_opened.private_key_pem,
            e2ee_signing_private_key_pem: signing_opened.private_key_pem,
            e2ee_agreement_private_key_pem: agreement_opened.private_key_pem,
        })
    }

    fn open_agent_identity_secret(
        &self,
        secret_ref_json: Option<&str>,
        legacy_private_key_pem: &str,
        field: &str,
    ) -> Result<OpenedAgentIdentitySecret> {
        if let Some(secret_ref_json) = non_empty(secret_ref_json) {
            let vault = self
                .secret_vault()
                .with_context(|| format!("{field} requires daemon secret vault root key"))?;
            let secret_ref: SecretRef =
                serde_json::from_str(secret_ref_json).with_context(|| format!("parse {field}"))?;
            let secret = vault
                .open(&secret_ref)
                .map_err(anyhow::Error::from)
                .with_context(|| format!("open {field}"))?;
            let private_key_pem = String::from_utf8(secret.expose_secret().to_vec())
                .with_context(|| format!("{field} secret is not utf-8"))?;
            return Ok(OpenedAgentIdentitySecret { private_key_pem });
        }
        let _ = legacy_private_key_pem;
        bail!("{field} is missing a daemon secret vault ref")
    }

    fn seal_user_delegated_private_key(
        &self,
        identity: &UserDelegatedIdentityRecord,
    ) -> Result<String> {
        let vault = self.secret_vault().context(
            "daemon secret vault root key is required to store user delegated private keys; refusing plaintext fallback",
        )?;
        if identity.private_key_material == VAULT_PRIVATE_KEY_SENTINEL {
            let secret_ref_json = non_empty(identity.private_key_ref_json.as_deref())
                .context("user delegated private key is missing a daemon secret vault ref")?;
            let secret_ref: SecretRef =
                serde_json::from_str(secret_ref_json).context("parse private_key_ref_json")?;
            vault
                .open(&secret_ref)
                .map_err(anyhow::Error::from)
                .context("open private_key_ref_json")?;
            return Ok(secret_ref_json.to_owned());
        }
        if identity.private_key_material.trim().is_empty() {
            bail!("user delegated private key must not be empty");
        }
        let secret_ref = vault
            .seal(SealSecretRequest {
                metadata: SecretMetadata {
                    workspace_id: "awiki-daemon".to_owned(),
                    device_id: "local-daemon".to_owned(),
                    identity_id: Some(identity.daemon_agent_did.clone()),
                    did: Some(identity.user_did.clone()),
                    kind: SecretKind::IdentityDaemonPrivate,
                    key_id: identity.verification_method.clone(),
                    key_version: 1,
                    policy: SecretAccessPolicy::no_prompt_local_secret(),
                },
                plaintext: SecretBytes::from_vec(identity.private_key_material.as_bytes().to_vec()),
            })
            .map_err(anyhow::Error::from)
            .context("seal user delegated private key")?;
        Ok(serde_json::to_string(&secret_ref)?)
    }

    fn user_delegated_identity_from_storage_record(
        &self,
        mut record: UserDelegatedIdentityRecord,
    ) -> Result<UserDelegatedIdentityRecord> {
        let opened = self.open_user_delegated_private_key(
            record.private_key_ref_json.as_deref(),
            &record.private_key_material,
        )?;
        record.private_key_material = opened.private_key_pem;
        Ok(record)
    }

    fn open_user_delegated_private_key(
        &self,
        secret_ref_json: Option<&str>,
        legacy_private_key_pem: &str,
    ) -> Result<OpenedAgentIdentitySecret> {
        self.open_agent_identity_secret(
            secret_ref_json,
            legacy_private_key_pem,
            "private_key_ref_json",
        )
    }

    fn seal_agent_auth_token(&self, agent_did: &str, jwt_token: &str) -> Result<String> {
        let vault = self.secret_vault().context(
            "daemon secret vault root key is required to store agent auth tokens; refusing plaintext fallback",
        )?;
        let secret_ref = vault
            .seal(SealSecretRequest {
                metadata: SecretMetadata {
                    workspace_id: "awiki-daemon".to_owned(),
                    device_id: "local-daemon".to_owned(),
                    identity_id: Some(agent_did.to_owned()),
                    did: Some(agent_did.to_owned()),
                    kind: SecretKind::AuthJwt,
                    key_id: "agent-auth-token".to_owned(),
                    key_version: 1,
                    policy: SecretAccessPolicy::no_prompt_local_secret(),
                },
                plaintext: SecretBytes::from_vec(jwt_token.as_bytes().to_vec()),
            })
            .map_err(anyhow::Error::from)
            .context("seal agent auth token")?;
        Ok(serde_json::to_string(&secret_ref)?)
    }

    fn open_agent_auth_token(
        &self,
        jwt_token_ref_json: Option<&str>,
        legacy_jwt_token: &str,
    ) -> Result<String> {
        let Some(jwt_token_ref_json) = non_empty(jwt_token_ref_json) else {
            let _ = legacy_jwt_token;
            bail!("jwt_token_ref_json is missing a daemon secret vault ref");
        };
        let vault = self
            .secret_vault()
            .context("jwt_token_ref_json requires daemon secret vault root key")?;
        let secret_ref: SecretRef =
            serde_json::from_str(jwt_token_ref_json).context("parse jwt_token_ref_json")?;
        let secret = vault
            .open(&secret_ref)
            .map_err(anyhow::Error::from)
            .context("open jwt_token_ref_json")?;
        String::from_utf8(secret.expose_secret().to_vec())
            .context("jwt_token_ref_json secret is not utf-8")
    }
}

struct OpenedAgentIdentitySecret {
    private_key_pem: String,
}

fn seal_agent_identity_secret(
    vault: &crate::secret_vault::DaemonSecretVault,
    identity: &AgentIdentityRecord,
    kind: SecretKind,
    key_id_suffix: &str,
    private_key_pem: &str,
    required: bool,
) -> Result<Option<String>> {
    if private_key_pem.trim().is_empty() {
        if required {
            bail!("{key_id_suffix} private key must not be empty");
        }
        return Ok(None);
    }
    let secret_ref = vault
        .seal(SealSecretRequest {
            metadata: SecretMetadata {
                workspace_id: "awiki-daemon".to_owned(),
                device_id: "local-daemon".to_owned(),
                identity_id: Some(identity.agent_did.clone()),
                did: Some(identity.agent_did.clone()),
                kind,
                key_id: format!("{}#{key_id_suffix}", identity.agent_did),
                key_version: 1,
                policy: SecretAccessPolicy::no_prompt_local_secret(),
            },
            plaintext: SecretBytes::from_vec(private_key_pem.as_bytes().to_vec()),
        })
        .map_err(anyhow::Error::from)
        .with_context(|| format!("seal {key_id_suffix} private key"))?;
    Ok(Some(serde_json::to_string(&secret_ref)?))
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}
