use super::*;
use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use im_core::vault::{
    SealIfAbsentResult, SealSecretRequest, SecretAccessPolicy, SecretBytes, SecretKind,
    SecretMetadata, SecretRef,
};
use rand::{rngs::OsRng, RngCore};
use rusqlite::{OptionalExtension, Transaction};

const STAGED_AGENT_DEVICE_SECRET_KEY_PREFIX: &str = "awiki-daemon-staged-agent-device:v1:";
const STAGED_AGENT_LEGACY_PENDING_KEY_PREFIX: &str = "awiki-daemon-staged-agent-legacy-pending:v1:";
const STAGED_AGENT_REGISTRATION_PENDING_KEY_PREFIX: &str =
    "awiki-daemon-staged-agent-registration-pending:v1:";

#[derive(Debug, Clone)]
struct AgentDeviceSecretRefs {
    root_private_key: SecretRef,
    device_signing_private_key: SecretRef,
    device_e2ee_private_key: SecretRef,
    daemon_subkey_package: Option<SecretRef>,
    access_token: SecretRef,
}

impl AgentDeviceSecretRefs {
    fn all(&self) -> Vec<SecretRef> {
        let mut refs = vec![
            self.root_private_key.clone(),
            self.device_signing_private_key.clone(),
            self.device_e2ee_private_key.clone(),
            self.access_token.clone(),
        ];
        if let Some(secret_ref) = &self.daemon_subkey_package {
            refs.push(secret_ref.clone());
        }
        refs
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AgentDeviceSecretStoreFault {
    FailSealAt(usize),
    FailBeforeCommit,
    SkipPostCommitCleanup,
}

#[cfg(test)]
thread_local! {
    static AGENT_DEVICE_SECRET_STORE_FAULT: std::cell::Cell<Option<AgentDeviceSecretStoreFault>> =
        const { std::cell::Cell::new(None) };
}

impl DaemonState {
    pub fn store_pending_agent_legacy_upgrade(
        &self,
        pending: &PendingAgentLegacyUpgradeRecord,
    ) -> Result<()> {
        pending.validate()?;
        let secret_ref = self.stage_pending_secret(
            &pending.protocol_device_id,
            &pending.agent_did,
            SecretKind::IdentityLegacyUpgradePending,
            STAGED_AGENT_LEGACY_PENDING_KEY_PREFIX,
            "legacy-upgrade",
            serde_json::to_vec(&pending.secret_payload_json)?,
            "Agent Legacy upgrade pending payload",
        )?;
        let secret_ref_json = serde_json::to_string(&secret_ref)?;
        let store_result = (|| -> Result<bool> {
            let mut connection = self.connection()?;
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let existing = transaction
                .query_row(
                    r#"
SELECT agent_kind, protocol_device_id, target_document_hash
FROM agent_legacy_upgrade_pending
WHERE agent_did = ?1
"#,
                    [&pending.agent_did],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    },
                )
                .optional()?;
            if let Some(existing) = existing {
                if existing.0 != pending.agent_kind.as_str()
                    || existing.1 != pending.protocol_device_id
                    || existing.2 != pending.target_document_hash
                {
                    bail!("Agent Legacy upgrade pending replay conflict");
                }
                return Ok(false);
            }
            transaction.execute(
                r#"
INSERT INTO agent_legacy_upgrade_pending (
    agent_did, agent_kind, protocol_device_id, target_document_hash,
    secret_ref_json, status, attempt_count, last_error_code, updated_at_ms
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
"#,
                rusqlite::params![
                    pending.agent_did,
                    pending.agent_kind.as_str(),
                    pending.protocol_device_id,
                    pending.target_document_hash,
                    secret_ref_json,
                    pending.status,
                    i64::from(pending.attempt_count),
                    pending.last_error_code,
                    pending.updated_at_ms,
                ],
            )?;
            inject_agent_device_store_failure_before_commit()?;
            transaction.commit()?;
            Ok(true)
        })();
        match store_result {
            Ok(true) => Ok(()),
            Ok(false) => {
                self.delete_staged_secret_best_effort(&secret_ref);
                Ok(())
            }
            Err(error) => {
                self.delete_staged_secret_best_effort(&secret_ref);
                Err(error)
            }
        }
    }

    pub fn load_pending_agent_legacy_upgrade(
        &self,
        agent_did: &str,
    ) -> Result<Option<PendingAgentLegacyUpgradeRecord>> {
        let stored = self
            .connection()?
            .query_row(
                r#"
SELECT agent_kind, protocol_device_id, target_document_hash, secret_ref_json,
       status, attempt_count, last_error_code, updated_at_ms
FROM agent_legacy_upgrade_pending
WHERE agent_did = ?1
"#,
                [agent_did],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, i64>(5)?,
                        row.get::<_, Option<String>>(6)?,
                        row.get::<_, i64>(7)?,
                    ))
                },
            )
            .optional()?;
        let Some((kind, device_id, hash, secret_ref_json, status, attempts, error, updated)) =
            stored
        else {
            return Ok(None);
        };
        let secret_payload_json = self.open_secret_json(
            &secret_ref_json,
            SecretKind::IdentityLegacyUpgradePending,
            "Agent Legacy upgrade pending payload",
        )?;
        let record = PendingAgentLegacyUpgradeRecord {
            agent_did: agent_did.to_owned(),
            agent_kind: crate::agent::AgentKind::parse(&kind)?,
            protocol_device_id: device_id,
            target_document_hash: hash,
            secret_payload_json,
            status,
            attempt_count: u32::try_from(attempts)
                .context("Agent Legacy upgrade attempt count is invalid")?,
            last_error_code: error,
            updated_at_ms: updated,
        };
        record.validate()?;
        Ok(Some(record))
    }

    pub fn mark_pending_agent_legacy_upgrade_attempt(
        &self,
        agent_did: &str,
        status: &str,
        error_code: Option<&str>,
    ) -> Result<()> {
        if !matches!(status, "prepared" | "retryable" | "blocked") {
            bail!("unsupported Agent Legacy upgrade pending attempt status");
        }
        if error_code.is_some_and(|value| value.trim().is_empty()) {
            bail!("Agent Legacy upgrade error code must not be empty");
        }
        let updated = self.connection()?.execute(
            r#"
UPDATE agent_legacy_upgrade_pending
SET status = ?2,
    attempt_count = attempt_count + 1,
    last_error_code = ?3,
    updated_at_ms = ?4
WHERE agent_did = ?1 AND status != 'completed'
"#,
            rusqlite::params![agent_did, status, error_code, current_time_millis()?],
        )?;
        if updated != 1 {
            bail!("Agent Legacy upgrade pending is missing or completed");
        }
        Ok(())
    }

    /// Atomically replaces the encrypted retry material after a proven remote
    /// Legacy document is rebuilt with a fresh proof. The bootstrap device
    /// binding must remain unchanged; only the source/target document and its
    /// digest may advance.
    pub fn replace_pending_agent_legacy_upgrade_payload(
        &self,
        agent_did: &str,
        protocol_device_id: &str,
        target_document_hash: &str,
        secret_payload_json: &Value,
    ) -> Result<()> {
        if agent_did.trim().is_empty() || protocol_device_id.trim().is_empty() {
            bail!("Agent Legacy upgrade refreshed binding must not be empty");
        }
        if !target_document_hash.starts_with("sha256:") || !secret_payload_json.is_object() {
            bail!("Agent Legacy upgrade refreshed payload is invalid");
        }
        let staged = self.stage_pending_secret(
            protocol_device_id,
            agent_did,
            SecretKind::IdentityLegacyUpgradePending,
            STAGED_AGENT_LEGACY_PENDING_KEY_PREFIX,
            "legacy-upgrade-refresh",
            serde_json::to_vec(secret_payload_json)?,
            "refreshed Agent Legacy upgrade pending payload",
        )?;
        let switch_result = (|| -> Result<SecretRef> {
            let mut connection = self.connection()?;
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let (stored_device_id, status, old_ref_json) = transaction
                .query_row(
                    r#"
SELECT protocol_device_id, status, secret_ref_json
FROM agent_legacy_upgrade_pending
WHERE agent_did = ?1
"#,
                    [agent_did],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    },
                )
                .optional()?
                .context("Agent Legacy upgrade pending is missing")?;
            if stored_device_id != protocol_device_id
                || status == "completed"
                || status == "blocked"
            {
                bail!("Agent Legacy upgrade refreshed payload cannot change the pending binding");
            }
            let old_ref: SecretRef = serde_json::from_str(&old_ref_json)
                .context("parse superseded Agent Legacy upgrade pending ref")?;
            transaction.execute(
                r#"
UPDATE agent_legacy_upgrade_pending
SET target_document_hash = ?2,
    secret_ref_json = ?3,
    updated_at_ms = ?4
WHERE agent_did = ?1
"#,
                rusqlite::params![
                    agent_did,
                    target_document_hash,
                    serde_json::to_string(&staged)?,
                    current_time_millis()?,
                ],
            )?;
            inject_agent_device_store_failure_before_commit()?;
            transaction.commit()?;
            Ok(old_ref)
        })();
        match switch_result {
            Ok(old_ref) => {
                self.delete_staged_secret_best_effort(&old_ref);
                Ok(())
            }
            Err(error) => {
                self.delete_staged_secret_best_effort(&staged);
                Err(error)
            }
        }
    }

    pub fn promote_pending_agent_legacy_upgrade(
        &self,
        identity: &AgentDeviceIdentityRecord,
    ) -> Result<()> {
        identity.validate()?;
        if identity.legacy_migration_state != "completed" {
            bail!("Legacy upgrade promotion requires completed migration state");
        }
        let refs = self.stage_agent_device_identity(identity)?;
        let switch_result = (|| -> Result<Option<AgentDeviceSecretRefs>> {
            let mut connection = self.connection()?;
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let old_refs = load_agent_device_secret_refs(&transaction, &identity.agent_did)?;
            let binding = transaction
                .query_row(
                    r#"
SELECT protocol_device_id, target_document_hash, status
FROM agent_legacy_upgrade_pending WHERE agent_did = ?1
"#,
                    [&identity.agent_did],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    },
                )
                .optional()?
                .context("Agent Legacy upgrade pending not found")?;
            if binding.0 != identity.protocol_device_id || binding.1 != identity.document_hash {
                bail!("Agent Legacy upgrade pending does not match active device identity");
            }
            if binding.2 == "blocked" {
                bail!("blocked Agent Legacy upgrade cannot be promoted");
            }
            reset_agent_sync_probe_if_binding_changed(&transaction, identity)?;
            upsert_agent_device_identity(&transaction, identity, &refs)?;
            transaction.execute(
                r#"
UPDATE agent_legacy_upgrade_pending
SET status = 'completed', last_error_code = NULL, updated_at_ms = ?2
WHERE agent_did = ?1
"#,
                rusqlite::params![identity.agent_did, current_time_millis()?],
            )?;
            transaction.execute(
                "DELETE FROM agent_identity_migration_state WHERE agent_did = ?1",
                [&identity.agent_did],
            )?;
            inject_agent_device_store_failure_before_commit()?;
            transaction.commit()?;
            Ok(old_refs)
        })();
        self.finish_agent_device_secret_switch(refs, switch_result)
    }

    pub fn scrub_completed_agent_legacy_upgrade(&self, agent_did: &str) -> Result<bool> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let stored_ref = transaction
            .query_row(
                r#"
SELECT pending.secret_ref_json
FROM agent_legacy_upgrade_pending pending
JOIN agent_device_identity active ON active.agent_did = pending.agent_did
WHERE pending.agent_did = ?1
  AND pending.status = 'completed'
  AND active.identity_status = 'active'
  AND active.legacy_migration_state = 'completed'
"#,
                [agent_did],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let Some(stored_ref) = stored_ref else {
            return Ok(false);
        };
        let secret_ref: SecretRef = serde_json::from_str(&stored_ref)?;
        if secret_ref.kind != SecretKind::IdentityLegacyUpgradePending {
            bail!("Agent Legacy upgrade pending ref has an unexpected secret kind");
        }
        transaction.execute(
            "DELETE FROM agent_legacy_upgrade_pending WHERE agent_did = ?1 AND status = 'completed'",
            [agent_did],
        )?;
        transaction.commit()?;
        self.delete_staged_secret_best_effort(&secret_ref);
        Ok(true)
    }

    pub fn record_agent_identity_migration_required(
        &self,
        agent_did: &str,
        error_code: &str,
    ) -> Result<()> {
        if agent_did.trim().is_empty() || error_code.trim().is_empty() {
            bail!("migration-required Agent DID and error code must not be empty");
        }
        self.connection()?.execute(
            r#"
INSERT INTO agent_identity_migration_state (agent_did, status, last_error_code, updated_at_ms)
VALUES (?1, 'migration_required', ?2, ?3)
ON CONFLICT(agent_did) DO UPDATE SET
    status = excluded.status,
    last_error_code = excluded.last_error_code,
    updated_at_ms = excluded.updated_at_ms
"#,
            rusqlite::params![agent_did, error_code, current_time_millis()?],
        )?;
        Ok(())
    }

    pub fn store_pending_agent_registration(
        &self,
        pending: &PendingAgentRegistrationRecord,
    ) -> Result<PendingAgentRegistrationStoreOutcome> {
        pending.validate()?;
        let secret_ref = self.stage_pending_secret(
            &pending.protocol_device_id,
            &pending.agent_did,
            SecretKind::IdentityRegistrationPending,
            STAGED_AGENT_REGISTRATION_PENDING_KEY_PREFIX,
            &pending.registration_id,
            serde_json::to_vec(&pending.secret_payload_json)
                .context("serialize pending agent registration secret payload")?,
            "pending agent registration secret payload",
        )?;
        let secret_ref_json = serde_json::to_string(&secret_ref)?;
        let store_result = (|| -> Result<PendingAgentRegistrationStoreOutcome> {
            let mut connection = self.connection()?;
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let existing = transaction
                .query_row(
                    r#"
SELECT registration_id, dedupe_key, agent_did, protocol_device_id, document_digest, request_digest
FROM agent_registration_pending
WHERE registration_id = ?1 OR dedupe_key = ?2
"#,
                    rusqlite::params![pending.registration_id, pending.dedupe_key],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, String>(5)?,
                        ))
                    },
                )
                .optional()?;
            if let Some(existing) = existing {
                let expected = (
                    pending.registration_id.as_str(),
                    pending.dedupe_key.as_str(),
                    pending.agent_did.as_str(),
                    pending.protocol_device_id.as_str(),
                    pending.document_digest.as_str(),
                    pending.request_digest.as_str(),
                );
                let actual = (
                    existing.0.as_str(),
                    existing.1.as_str(),
                    existing.2.as_str(),
                    existing.3.as_str(),
                    existing.4.as_str(),
                    existing.5.as_str(),
                );
                if actual != expected {
                    bail!("pending agent registration replay conflict");
                }
                return Ok(PendingAgentRegistrationStoreOutcome::Duplicate);
            }
            transaction.execute(
                r#"
INSERT INTO agent_registration_pending (
    registration_id,
    dedupe_key,
    agent_kind,
    controller_did,
    handle,
    display_name,
    agent_did,
    protocol_device_id,
    document_digest,
    request_digest,
    secret_ref_json,
    status,
    attempt_count,
    last_error_code,
    last_error_summary,
    created_at_ms,
    updated_at_ms
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
"#,
                rusqlite::params![
                    pending.registration_id,
                    pending.dedupe_key,
                    pending.agent_kind.as_str(),
                    pending.controller_did,
                    pending.handle,
                    pending.display_name,
                    pending.agent_did,
                    pending.protocol_device_id,
                    pending.document_digest,
                    pending.request_digest,
                    secret_ref_json,
                    pending.status,
                    i64::from(pending.attempt_count),
                    pending.last_error_code,
                    pending.last_error_summary,
                    pending.created_at_ms,
                    pending.updated_at_ms,
                ],
            )?;
            inject_agent_device_store_failure_before_commit()?;
            transaction.commit()?;
            Ok(PendingAgentRegistrationStoreOutcome::Inserted)
        })();
        match store_result {
            Ok(PendingAgentRegistrationStoreOutcome::Inserted) => {
                Ok(PendingAgentRegistrationStoreOutcome::Inserted)
            }
            Ok(PendingAgentRegistrationStoreOutcome::Duplicate) => {
                self.delete_staged_secret_best_effort(&secret_ref);
                Ok(PendingAgentRegistrationStoreOutcome::Duplicate)
            }
            Err(error) => {
                self.delete_staged_secret_best_effort(&secret_ref);
                Err(error)
            }
        }
    }

    #[cfg(any(test, feature = "system-test-probe"))]
    pub fn replace_pending_agent_registration_payload_for_system_test(
        &self,
        registration_id: &str,
        agent_did: &str,
        protocol_device_id: &str,
        secret_payload_json: &Value,
    ) -> Result<()> {
        if registration_id.trim().is_empty()
            || agent_did.trim().is_empty()
            || protocol_device_id.trim().is_empty()
            || !secret_payload_json.is_object()
        {
            bail!("pending Agent registration replacement binding is invalid");
        }
        let staged = self.stage_pending_secret(
            protocol_device_id,
            agent_did,
            SecretKind::IdentityRegistrationPending,
            STAGED_AGENT_REGISTRATION_PENDING_KEY_PREFIX,
            "system-test-token-bind",
            serde_json::to_vec(secret_payload_json)?,
            "system-test pending Agent registration payload",
        )?;
        let switch_result = (|| -> Result<SecretRef> {
            let mut connection = self.connection()?;
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let (stored_agent_did, stored_device_id, status, old_ref_json) = transaction
                .query_row(
                    r#"
SELECT agent_did, protocol_device_id, status, secret_ref_json
FROM agent_registration_pending
WHERE registration_id = ?1
"#,
                    [registration_id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                        ))
                    },
                )
                .optional()?
                .context("pending Agent registration is missing")?;
            if stored_agent_did != agent_did
                || stored_device_id != protocol_device_id
                || status != "pending"
            {
                bail!("pending Agent registration replacement cannot change its binding");
            }
            let old_ref: SecretRef = serde_json::from_str(&old_ref_json)
                .context("parse superseded pending Agent registration ref")?;
            transaction.execute(
                r#"
UPDATE agent_registration_pending
SET secret_ref_json = ?2,
    updated_at_ms = ?3
WHERE registration_id = ?1 AND status = 'pending'
"#,
                rusqlite::params![
                    registration_id,
                    serde_json::to_string(&staged)?,
                    current_time_millis()?,
                ],
            )?;
            inject_agent_device_store_failure_before_commit()?;
            transaction.commit()?;
            Ok(old_ref)
        })();
        match switch_result {
            Ok(old_ref) => {
                self.delete_staged_secret_best_effort(&old_ref);
                Ok(())
            }
            Err(error) => {
                self.delete_staged_secret_best_effort(&staged);
                Err(error)
            }
        }
    }

    pub fn load_pending_agent_registration_by_dedupe_key(
        &self,
        dedupe_key: &str,
    ) -> Result<Option<PendingAgentRegistrationRecord>> {
        self.load_pending_agent_registration_by_column("dedupe_key", dedupe_key)
    }

    pub fn load_pending_agent_registration(
        &self,
        registration_id: &str,
    ) -> Result<Option<PendingAgentRegistrationRecord>> {
        self.load_pending_agent_registration_by_column("registration_id", registration_id)
    }

    pub fn list_resumable_agent_registrations(
        &self,
    ) -> Result<Vec<PendingAgentRegistrationRecord>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(&format!(
            "{} WHERE status IN ('pending', 'retryable') ORDER BY updated_at_ms ASC",
            pending_registration_select_sql()
        ))?;
        let rows = statement.query_map([], pending_registration_from_row)?;
        let stored = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        stored
            .into_iter()
            .map(|stored| self.open_pending_registration(stored))
            .collect()
    }

    pub fn mark_pending_agent_registration_attempt(
        &self,
        registration_id: &str,
        status: &str,
        last_error_code: Option<&str>,
        last_error_summary: Option<&str>,
    ) -> Result<()> {
        if !matches!(status, "pending" | "retryable" | "blocked") {
            bail!("unsupported pending registration attempt status: {status}");
        }
        let now = current_time_millis()?;
        let updated = self.connection()?.execute(
            r#"
UPDATE agent_registration_pending
SET status = ?2,
    attempt_count = attempt_count + 1,
    last_error_code = ?3,
    last_error_summary = ?4,
    updated_at_ms = ?5
WHERE registration_id = ?1 AND status != 'completed'
"#,
            rusqlite::params![
                registration_id,
                status,
                last_error_code,
                last_error_summary,
                now
            ],
        )?;
        if updated != 1 {
            bail!("pending agent registration is missing or already completed");
        }
        Ok(())
    }

    pub fn store_agent_device_identity(&self, identity: &AgentDeviceIdentityRecord) -> Result<()> {
        identity.validate()?;
        let refs = self.stage_agent_device_identity(identity)?;
        let switch_result = (|| -> Result<Option<AgentDeviceSecretRefs>> {
            let mut connection = self.connection()?;
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let old_refs = load_agent_device_secret_refs(&transaction, &identity.agent_did)?;
            reset_agent_sync_probe_if_binding_changed(&transaction, identity)?;
            upsert_agent_device_identity(&transaction, identity, &refs)?;
            inject_agent_device_store_failure_before_commit()?;
            transaction.commit()?;
            Ok(old_refs)
        })();
        self.finish_agent_device_secret_switch(refs, switch_result)
    }

    pub fn promote_pending_agent_device_identity(
        &self,
        registration_id: &str,
        identity: &AgentDeviceIdentityRecord,
    ) -> Result<()> {
        identity.validate()?;
        let refs = self.stage_agent_device_identity(identity)?;
        let switch_result = (|| -> Result<Option<AgentDeviceSecretRefs>> {
            let mut connection = self.connection()?;
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let old_refs = load_agent_device_secret_refs(&transaction, &identity.agent_did)?;
            let pending_binding = transaction
                .query_row(
                    r#"
SELECT agent_did, protocol_device_id, status
FROM agent_registration_pending
WHERE registration_id = ?1
"#,
                    [registration_id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    },
                )
                .optional()?;
            let Some((agent_did, protocol_device_id, pending_status)) = pending_binding else {
                bail!("pending agent registration not found");
            };
            if agent_did != identity.agent_did || protocol_device_id != identity.protocol_device_id
            {
                bail!("pending registration does not match active device identity");
            }
            if pending_status == "blocked" {
                bail!("blocked pending registration cannot be promoted");
            }
            reset_agent_sync_probe_if_binding_changed(&transaction, identity)?;
            upsert_agent_device_identity(&transaction, identity, &refs)?;
            transaction.execute(
                r#"
UPDATE agent_registration_pending
SET status = 'completed',
    last_error_code = NULL,
    last_error_summary = NULL,
    updated_at_ms = ?2
WHERE registration_id = ?1
"#,
                rusqlite::params![registration_id, current_time_millis()?],
            )?;
            inject_agent_device_store_failure_before_commit()?;
            transaction.commit()?;
            Ok(old_refs)
        })();
        self.finish_agent_device_secret_switch(refs, switch_result)
    }

    pub fn complete_pending_agent_registration(&self, registration_id: &str) -> Result<()> {
        let connection = self.connection()?;
        let updated = connection.execute(
            r#"
UPDATE agent_registration_pending
SET status = 'completed',
    last_error_code = NULL,
    last_error_summary = NULL,
    updated_at_ms = ?2
WHERE registration_id = ?1
  AND status != 'blocked'
  AND EXISTS (
      SELECT 1
      FROM agent_device_identity active
      WHERE active.agent_did = agent_registration_pending.agent_did
        AND active.protocol_device_id = agent_registration_pending.protocol_device_id
        AND active.identity_status = 'active'
  )
"#,
            rusqlite::params![registration_id, current_time_millis()?],
        )?;
        if updated != 1 {
            bail!("pending registration is blocked, missing, or has no active device identity");
        }
        Ok(())
    }

    pub fn scrub_completed_agent_registration(&self, agent_did: &str) -> Result<bool> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let pending = transaction
            .query_row(
                r#"
SELECT pending.registration_id, pending.secret_ref_json
FROM agent_registration_pending pending
JOIN agent_device_identity active ON active.agent_did = pending.agent_did
JOIN agent_definition definition ON definition.agent_did = pending.agent_did
WHERE pending.agent_did = ?1
  AND pending.status = 'completed'
  AND active.identity_status = 'active'
LIMIT 1
"#,
                [agent_did],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;
        let Some((registration_id, secret_ref_json)) = pending else {
            return Ok(false);
        };
        let secret_ref: SecretRef =
            serde_json::from_str(&secret_ref_json).context("parse completed registration ref")?;
        if secret_ref.kind != SecretKind::IdentityRegistrationPending {
            bail!("completed registration ref has an unexpected secret kind");
        }
        transaction.execute(
            "DELETE FROM agent_registration_pending WHERE registration_id = ?1 AND status = 'completed'",
            [registration_id],
        )?;
        transaction.commit()?;
        self.delete_staged_secret_best_effort(&secret_ref);
        Ok(true)
    }

    pub fn load_agent_device_identity(
        &self,
        agent_did: &str,
    ) -> Result<Option<AgentDeviceIdentityRecord>> {
        let connection = self.connection()?;
        let stored = connection
            .query_row(
                agent_device_identity_select_sql(),
                [agent_did],
                agent_device_identity_from_row,
            )
            .optional()?;
        stored
            .map(|stored| self.open_agent_device_identity(stored))
            .transpose()
    }

    pub fn mark_legacy_agent_identity_upgrade_blocked(
        &self,
        agent_did: &str,
        error_code: &str,
    ) -> Result<()> {
        if error_code.trim().is_empty() {
            bail!("error_code must not be empty");
        }
        let updated = self.connection()?.execute(
            r#"
UPDATE agent_device_identity
SET identity_status = 'blocked',
    legacy_migration_state = 'blocked',
    last_error_code = ?2,
    updated_at_ms = ?3
WHERE agent_did = ?1
"#,
            rusqlite::params![agent_did, error_code, current_time_millis()?],
        )?;
        if updated != 1 {
            bail!("agent device identity not found");
        }
        Ok(())
    }

    pub fn mark_agent_device_auth_revoked(&self, agent_did: &str) -> Result<()> {
        let agent_did = agent_did.trim();
        if agent_did.is_empty() {
            bail!("agent_did must not be empty");
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let updated = transaction.execute(
            r#"
UPDATE agent_device_identity
SET authorization_status = 'revoked',
    management_ready = 0,
    identity_status = 'revoked',
    last_error_code = 'auth_revoked',
    updated_at_ms = ?2
WHERE agent_did = ?1
  AND identity_status = 'active'
"#,
            rusqlite::params![agent_did, current_time_millis()?],
        )?;
        if updated == 0 {
            let already_revoked = transaction.query_row(
                r#"
SELECT EXISTS(
    SELECT 1 FROM agent_device_identity
    WHERE agent_did = ?1
      AND identity_status = 'revoked'
      AND authorization_status = 'revoked'
)
"#,
                [agent_did],
                |row| row.get::<_, i64>(0),
            )? != 0;
            if !already_revoked {
                bail!("active Agent device identity not found for AuthRevoked fencing");
            }
        }
        transaction.execute(
            "DELETE FROM agent_sync_probe WHERE agent_did = ?1",
            [agent_did],
        )?;
        transaction.commit()?;
        let fenced = self
            .load_agent_device_identity(agent_did)?
            .context("AuthRevoked fencing lost Agent device identity")?;
        fenced.validate()?;
        if fenced.identity_status != "revoked" || fenced.authorization_status != "revoked" {
            bail!("AuthRevoked fencing did not persist the closed revoked state");
        }
        Ok(())
    }

    fn load_pending_agent_registration_by_column(
        &self,
        column: &str,
        value: &str,
    ) -> Result<Option<PendingAgentRegistrationRecord>> {
        let predicate = match column {
            "registration_id" => "registration_id = ?1",
            "dedupe_key" => "dedupe_key = ?1",
            _ => bail!("unsupported pending registration lookup"),
        };
        let connection = self.connection()?;
        let stored = connection
            .query_row(
                &format!("{} WHERE {predicate}", pending_registration_select_sql()),
                [value],
                pending_registration_from_row,
            )
            .optional()?;
        stored
            .map(|stored| self.open_pending_registration(stored))
            .transpose()
    }

    fn stage_pending_secret(
        &self,
        protocol_device_id: &str,
        agent_did: &str,
        kind: SecretKind,
        key_prefix: &str,
        logical_key_id: &str,
        plaintext: Vec<u8>,
        context: &str,
    ) -> Result<SecretRef> {
        let vault = self
            .secret_vault()
            .with_context(|| format!("{context} requires daemon secret vault root key"))?;
        let stage_id = random_agent_device_secret_stage_id();
        let result = vault
            .seal_if_absent(SealSecretRequest {
                metadata: SecretMetadata {
                    workspace_id: "awiki-daemon".to_owned(),
                    device_id: protocol_device_id.to_owned(),
                    identity_id: Some(agent_did.to_owned()),
                    did: Some(agent_did.to_owned()),
                    kind,
                    key_id: format!("{key_prefix}{stage_id}:{logical_key_id}"),
                    key_version: 1,
                    policy: SecretAccessPolicy::no_prompt_local_secret(),
                },
                plaintext: SecretBytes::from_vec(plaintext),
            })
            .map_err(anyhow::Error::from)
            .with_context(|| format!("seal staged {context} without replacement"))?;
        match result {
            SealIfAbsentResult::Sealed(secret_ref) => Ok(secret_ref),
            SealIfAbsentResult::AlreadyExists(_) => {
                bail!("staged {context} reference collision")
            }
        }
    }

    fn delete_staged_secret_best_effort(&self, secret_ref: &SecretRef) {
        if skip_agent_device_post_commit_cleanup() {
            return;
        }
        if let Some(vault) = self.secret_vault() {
            let _ = vault.delete(secret_ref);
        }
    }

    fn open_pending_registration(
        &self,
        stored: StoredPendingAgentRegistration,
    ) -> Result<PendingAgentRegistrationRecord> {
        let payload = self.open_secret_json(
            &stored.secret_ref_json,
            SecretKind::IdentityRegistrationPending,
            "pending agent registration",
        )?;
        let record = PendingAgentRegistrationRecord {
            registration_id: stored.registration_id,
            dedupe_key: stored.dedupe_key,
            agent_kind: stored.agent_kind,
            controller_did: stored.controller_did,
            handle: stored.handle,
            display_name: stored.display_name,
            agent_did: stored.agent_did,
            protocol_device_id: stored.protocol_device_id,
            document_digest: stored.document_digest,
            request_digest: stored.request_digest,
            secret_payload_json: payload,
            status: stored.status,
            attempt_count: stored.attempt_count,
            last_error_code: stored.last_error_code,
            last_error_summary: stored.last_error_summary,
            created_at_ms: stored.created_at_ms,
            updated_at_ms: stored.updated_at_ms,
        };
        record.validate()?;
        Ok(record)
    }

    fn stage_agent_device_identity(
        &self,
        identity: &AgentDeviceIdentityRecord,
    ) -> Result<AgentDeviceSecretRefs> {
        let vault = self
            .secret_vault()
            .context("agent device identity requires daemon secret vault root key")?;
        let stage_id = random_agent_device_secret_stage_id();
        let mut sealed = Vec::new();
        let staged = (|| -> Result<AgentDeviceSecretRefs> {
            let root_private_key = seal_staged_device_secret(
                vault,
                identity,
                &stage_id,
                SecretKind::IdentityRootPrivate,
                &identity.root_key_id,
                &identity.root_private_key_pem,
                &mut sealed,
            )?;
            let device_signing_private_key = seal_staged_device_secret(
                vault,
                identity,
                &stage_id,
                SecretKind::IdentityDeviceSigningPrivate,
                &identity.device_signing_key_id,
                &identity.device_signing_private_key_pem,
                &mut sealed,
            )?;
            let device_e2ee_private_key = seal_staged_device_secret(
                vault,
                identity,
                &stage_id,
                SecretKind::IdentityE2eeAgreementPrivate,
                &identity.device_e2ee_key_id,
                &identity.device_e2ee_private_key_pem,
                &mut sealed,
            )?;
            let daemon_subkey_package = identity
                .daemon_subkey_package_json
                .as_ref()
                .map(|package| {
                    seal_staged_device_secret(
                        vault,
                        identity,
                        &stage_id,
                        SecretKind::IdentityDaemonPrivate,
                        &format!("{}#daemon-subkey-package", identity.agent_did),
                        &package.to_string(),
                        &mut sealed,
                    )
                })
                .transpose()?;
            let access_token = seal_staged_device_secret(
                vault,
                identity,
                &stage_id,
                SecretKind::AuthJwt,
                &format!("{}#device-access", identity.agent_did),
                &identity.access_token,
                &mut sealed,
            )?;
            Ok(AgentDeviceSecretRefs {
                root_private_key,
                device_signing_private_key,
                device_e2ee_private_key,
                daemon_subkey_package,
                access_token,
            })
        })();
        if staged.is_err() {
            delete_secret_refs_best_effort(vault, &sealed);
        }
        staged
    }

    fn finish_agent_device_secret_switch(
        &self,
        staged: AgentDeviceSecretRefs,
        switch_result: Result<Option<AgentDeviceSecretRefs>>,
    ) -> Result<()> {
        match switch_result {
            Ok(old_refs) => {
                if !skip_agent_device_post_commit_cleanup() {
                    if let Some(old_refs) = old_refs {
                        let _ = self.delete_device_secret_refs_if_unreferenced(&old_refs.all());
                    }
                }
                Ok(())
            }
            Err(error) => {
                if let Some(vault) = self.secret_vault() {
                    delete_secret_refs_best_effort(vault, &staged.all());
                }
                Err(error)
            }
        }
    }

    pub fn recover_unreferenced_staged_agent_secrets(&self) -> Result<usize> {
        let Some(vault) = self.secret_vault() else {
            return Ok(0);
        };
        let referenced = load_all_agent_secret_refs(&self.connection()?)?;
        let mut removed = 0usize;
        for secret_ref in vault
            .list()
            .map_err(anyhow::Error::from)
            .context("list staged Agent device secrets during startup recovery")?
        {
            if !is_recoverable_staged_agent_secret(&secret_ref)
                || referenced.iter().any(|active| active == &secret_ref)
            {
                continue;
            }
            vault
                .delete(&secret_ref)
                .map_err(anyhow::Error::from)
                .context("delete unreferenced staged Agent secret")?;
            removed += 1;
        }
        Ok(removed)
    }

    fn delete_device_secret_refs_if_unreferenced(&self, candidates: &[SecretRef]) -> Result<()> {
        let Some(vault) = self.secret_vault() else {
            return Ok(());
        };
        let referenced = load_all_agent_secret_refs(&self.connection()?)?;
        for secret_ref in candidates {
            if referenced.iter().any(|active| active == secret_ref) {
                continue;
            }
            vault
                .delete(secret_ref)
                .map_err(anyhow::Error::from)
                .context("delete superseded Agent device secret")?;
        }
        Ok(())
    }

    fn open_agent_device_identity(
        &self,
        stored: StoredAgentDeviceIdentity,
    ) -> Result<AgentDeviceIdentityRecord> {
        let record = AgentDeviceIdentityRecord {
            identity_id: stored.identity_id,
            agent_did: stored.agent_did,
            handle: stored.handle,
            display_name: stored.display_name,
            agent_kind: stored.agent_kind,
            account_id: stored.account_id,
            full_handle: stored.full_handle,
            binding_generation: stored.binding_generation,
            did_document: stored.did_document,
            protocol_device_id: stored.protocol_device_id,
            root_key_id: stored.root_key_id,
            root_private_key_pem: self.open_secret_string(
                &stored.root_private_key_ref_json,
                SecretKind::IdentityRootPrivate,
                "agent root private key",
            )?,
            device_signing_key_id: stored.device_signing_key_id,
            device_signing_private_key_pem: self.open_secret_string(
                &stored.device_signing_private_key_ref_json,
                SecretKind::IdentityDeviceSigningPrivate,
                "agent device signing private key",
            )?,
            device_e2ee_key_id: stored.device_e2ee_key_id,
            device_e2ee_private_key_pem: self.open_secret_string(
                &stored.device_e2ee_private_key_ref_json,
                SecretKind::IdentityE2eeAgreementPrivate,
                "agent device E2EE private key",
            )?,
            daemon_subkey_package_json: stored
                .daemon_subkey_package_ref_json
                .as_deref()
                .map(|secret_ref| {
                    self.open_secret_json(
                        secret_ref,
                        SecretKind::IdentityDaemonPrivate,
                        "daemon subkey package",
                    )
                })
                .transpose()?,
            authorization_status: stored.authorization_status,
            role: stored.role,
            management_ready: stored.management_ready,
            auth_generation: stored.auth_generation,
            access_token: self.open_secret_string(
                &stored.access_token_ref_json,
                SecretKind::AuthJwt,
                "agent Device Access token",
            )?,
            document_version: stored.document_version,
            document_hash: stored.document_hash,
            registry_version: stored.registry_version,
            identity_status: stored.identity_status,
            legacy_migration_state: stored.legacy_migration_state,
            last_error_code: stored.last_error_code,
        };
        record.validate()?;
        Ok(record)
    }

    fn open_secret_json(
        &self,
        secret_ref_json: &str,
        expected_kind: SecretKind,
        context: &str,
    ) -> Result<Value> {
        let bytes = self.open_secret_bytes(secret_ref_json, expected_kind, context)?;
        serde_json::from_slice(&bytes).with_context(|| format!("parse {context} JSON"))
    }

    fn open_secret_string(
        &self,
        secret_ref_json: &str,
        expected_kind: SecretKind,
        context: &str,
    ) -> Result<String> {
        let bytes = self.open_secret_bytes(secret_ref_json, expected_kind, context)?;
        String::from_utf8(bytes).with_context(|| format!("{context} is not utf-8"))
    }

    fn open_secret_bytes(
        &self,
        secret_ref_json: &str,
        expected_kind: SecretKind,
        context: &str,
    ) -> Result<Vec<u8>> {
        let secret_ref: SecretRef = serde_json::from_str(secret_ref_json)
            .with_context(|| format!("parse {context} ref"))?;
        if secret_ref.kind != expected_kind {
            bail!("{context} ref has an unexpected secret kind");
        }
        let vault = self
            .secret_vault()
            .with_context(|| format!("{context} requires daemon secret vault root key"))?;
        let secret = vault
            .open(&secret_ref)
            .map_err(anyhow::Error::from)
            .with_context(|| format!("open {context}"))?;
        Ok(secret.expose_secret().to_vec())
    }
}

fn seal_staged_device_secret(
    vault: &crate::secret_vault::DaemonSecretVault,
    identity: &AgentDeviceIdentityRecord,
    stage_id: &str,
    kind: SecretKind,
    logical_key_id: &str,
    plaintext: &str,
    sealed: &mut Vec<SecretRef>,
) -> Result<SecretRef> {
    inject_agent_device_seal_failure(sealed.len())?;
    let key_id = format!("{STAGED_AGENT_DEVICE_SECRET_KEY_PREFIX}{stage_id}:{logical_key_id}");
    let result = vault
        .seal_if_absent(SealSecretRequest {
            metadata: SecretMetadata {
                workspace_id: "awiki-daemon".to_owned(),
                device_id: identity.protocol_device_id.clone(),
                identity_id: Some(identity.agent_did.clone()),
                did: Some(identity.agent_did.clone()),
                kind,
                key_id,
                key_version: 1,
                policy: SecretAccessPolicy::no_prompt_local_secret(),
            },
            plaintext: SecretBytes::from_vec(plaintext.as_bytes().to_vec()),
        })
        .map_err(anyhow::Error::from)
        .context("seal staged Agent device secret without replacement")?;
    let SecretIfAbsentOutcome::Sealed(secret_ref) = SecretIfAbsentOutcome::from(result) else {
        bail!("staged Agent device secret reference collision");
    };
    sealed.push(secret_ref.clone());
    Ok(secret_ref)
}

enum SecretIfAbsentOutcome {
    Sealed(SecretRef),
    AlreadyExists,
}

impl From<SealIfAbsentResult> for SecretIfAbsentOutcome {
    fn from(value: SealIfAbsentResult) -> Self {
        match value {
            SealIfAbsentResult::Sealed(secret_ref) => Self::Sealed(secret_ref),
            SealIfAbsentResult::AlreadyExists(_) => Self::AlreadyExists,
        }
    }
}

fn random_agent_device_secret_stage_id() -> String {
    let mut bytes = [0_u8; 16];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn is_recoverable_staged_agent_secret(secret_ref: &SecretRef) -> bool {
    let staged_device = secret_ref
        .key_id
        .starts_with(STAGED_AGENT_DEVICE_SECRET_KEY_PREFIX)
        && matches!(
            secret_ref.kind,
            SecretKind::IdentityRootPrivate
                | SecretKind::IdentityDeviceSigningPrivate
                | SecretKind::IdentityE2eeAgreementPrivate
                | SecretKind::IdentityDaemonPrivate
                | SecretKind::AuthJwt
        );
    let legacy_pending = secret_ref.kind == SecretKind::IdentityLegacyUpgradePending
        && (secret_ref
            .key_id
            .starts_with(STAGED_AGENT_LEGACY_PENDING_KEY_PREFIX)
            // Recover orphan records written by the pre-staging implementation.
            || secret_ref.key_id.ends_with("#legacy-upgrade"));
    let registration_pending = secret_ref.kind == SecretKind::IdentityRegistrationPending
        && (secret_ref
            .key_id
            .starts_with(STAGED_AGENT_REGISTRATION_PENDING_KEY_PREFIX)
            // Recover orphan records written by the pre-staging implementation.
            || secret_ref.key_id.starts_with("pending-registration:"));
    staged_device || legacy_pending || registration_pending
}

fn delete_secret_refs_best_effort(
    vault: &crate::secret_vault::DaemonSecretVault,
    secret_refs: &[SecretRef],
) {
    for secret_ref in secret_refs {
        let _ = vault.delete(secret_ref);
    }
}

fn inject_agent_device_seal_failure(seal_index: usize) -> Result<()> {
    #[cfg(test)]
    AGENT_DEVICE_SECRET_STORE_FAULT.with(|fault| {
        if fault.get() == Some(AgentDeviceSecretStoreFault::FailSealAt(seal_index)) {
            bail!("injected staged Agent device secret seal failure");
        }
        Ok(())
    })?;
    #[cfg(not(test))]
    let _ = seal_index;
    Ok(())
}

fn inject_agent_device_store_failure_before_commit() -> Result<()> {
    #[cfg(test)]
    AGENT_DEVICE_SECRET_STORE_FAULT.with(|fault| {
        if fault.get() == Some(AgentDeviceSecretStoreFault::FailBeforeCommit) {
            bail!("injected Agent device database failure before commit");
        }
        Ok(())
    })?;
    Ok(())
}

fn skip_agent_device_post_commit_cleanup() -> bool {
    #[cfg(test)]
    {
        return AGENT_DEVICE_SECRET_STORE_FAULT
            .with(|fault| fault.get() == Some(AgentDeviceSecretStoreFault::SkipPostCommitCleanup));
    }
    #[cfg(not(test))]
    false
}

#[cfg(test)]
pub(super) fn with_agent_device_secret_store_fault<T>(
    fault: AgentDeviceSecretStoreFault,
    operation: impl FnOnce() -> T,
) -> T {
    struct ResetFault(Option<AgentDeviceSecretStoreFault>);

    impl Drop for ResetFault {
        fn drop(&mut self) {
            AGENT_DEVICE_SECRET_STORE_FAULT.with(|slot| slot.set(self.0));
        }
    }

    let previous = AGENT_DEVICE_SECRET_STORE_FAULT.with(|slot| slot.replace(Some(fault)));
    let _reset = ResetFault(previous);
    operation()
}

fn load_agent_device_secret_refs(
    connection: &rusqlite::Connection,
    agent_did: &str,
) -> Result<Option<AgentDeviceSecretRefs>> {
    let stored = connection
        .query_row(
            r#"
SELECT root_private_key_ref_json,
       device_signing_private_key_ref_json,
       device_e2ee_private_key_ref_json,
       daemon_subkey_package_ref_json,
       access_token_ref_json
FROM agent_device_identity
WHERE agent_did = ?1
"#,
            [agent_did],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )
        .optional()?;
    stored.map(parse_agent_device_secret_refs).transpose()
}

fn load_all_agent_secret_refs(connection: &rusqlite::Connection) -> Result<Vec<SecretRef>> {
    let mut statement = connection.prepare(
        r#"
SELECT root_private_key_ref_json,
       device_signing_private_key_ref_json,
       device_e2ee_private_key_ref_json,
       daemon_subkey_package_ref_json,
       access_token_ref_json
FROM agent_device_identity
"#,
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, String>(4)?,
        ))
    })?;
    let mut refs = Vec::new();
    for stored in rows {
        refs.extend(parse_agent_device_secret_refs(stored?)?.all());
    }
    for table in ["agent_legacy_upgrade_pending", "agent_registration_pending"] {
        let mut statement = connection.prepare(&format!("SELECT secret_ref_json FROM {table}"))?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        for secret_ref_json in rows {
            refs.push(
                serde_json::from_str(&secret_ref_json?)
                    .with_context(|| format!("parse {table} secret ref"))?,
            );
        }
    }
    Ok(refs)
}

fn parse_agent_device_secret_refs(
    stored: (String, String, String, Option<String>, String),
) -> Result<AgentDeviceSecretRefs> {
    Ok(AgentDeviceSecretRefs {
        root_private_key: serde_json::from_str(&stored.0)
            .context("parse Agent root private key ref")?,
        device_signing_private_key: serde_json::from_str(&stored.1)
            .context("parse Agent device signing private key ref")?,
        device_e2ee_private_key: serde_json::from_str(&stored.2)
            .context("parse Agent device E2EE private key ref")?,
        daemon_subkey_package: stored
            .3
            .map(|value| {
                serde_json::from_str(&value).context("parse Agent daemon subkey package ref")
            })
            .transpose()?,
        access_token: serde_json::from_str(&stored.4)
            .context("parse Agent Device Access token ref")?,
    })
}

fn upsert_agent_device_identity(
    transaction: &Transaction<'_>,
    identity: &AgentDeviceIdentityRecord,
    refs: &AgentDeviceSecretRefs,
) -> Result<()> {
    let now = current_time_millis()?;
    let auth_generation = i64::try_from(identity.auth_generation)
        .context("auth_generation exceeds SQLite integer range")?;
    let document_version = i64::try_from(identity.document_version)
        .context("document_version exceeds SQLite integer range")?;
    let registry_version = i64::try_from(identity.registry_version)
        .context("registry_version exceeds SQLite integer range")?;
    let root_private_key_ref_json = serde_json::to_string(&refs.root_private_key)?;
    let device_signing_private_key_ref_json =
        serde_json::to_string(&refs.device_signing_private_key)?;
    let device_e2ee_private_key_ref_json = serde_json::to_string(&refs.device_e2ee_private_key)?;
    let daemon_subkey_package_ref_json = refs
        .daemon_subkey_package
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    let access_token_ref_json = serde_json::to_string(&refs.access_token)?;
    transaction.execute(
        r#"
INSERT INTO agent_device_identity (
    agent_did, handle, display_name, agent_kind, account_id, full_handle,
    binding_generation, did_document_json, protocol_device_id, root_key_id,
    root_private_key_ref_json, device_signing_key_id,
    device_signing_private_key_ref_json, device_e2ee_key_id,
    device_e2ee_private_key_ref_json, daemon_subkey_package_ref_json,
    authorization_status, role,
    management_ready, auth_generation, access_token_ref_json, document_version,
    document_hash, registry_version, identity_status, legacy_migration_state,
    last_error_code, identity_id, created_at_ms, updated_at_ms
) VALUES (
    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
    ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?29
)
ON CONFLICT(agent_did) DO UPDATE SET
    handle = excluded.handle,
    display_name = excluded.display_name,
    agent_kind = excluded.agent_kind,
    account_id = excluded.account_id,
    full_handle = excluded.full_handle,
    binding_generation = excluded.binding_generation,
    did_document_json = excluded.did_document_json,
    protocol_device_id = excluded.protocol_device_id,
    root_key_id = excluded.root_key_id,
    root_private_key_ref_json = excluded.root_private_key_ref_json,
    device_signing_key_id = excluded.device_signing_key_id,
    device_signing_private_key_ref_json = excluded.device_signing_private_key_ref_json,
    device_e2ee_key_id = excluded.device_e2ee_key_id,
    device_e2ee_private_key_ref_json = excluded.device_e2ee_private_key_ref_json,
    daemon_subkey_package_ref_json = excluded.daemon_subkey_package_ref_json,
    authorization_status = excluded.authorization_status,
    role = excluded.role,
    management_ready = excluded.management_ready,
    auth_generation = excluded.auth_generation,
    access_token_ref_json = excluded.access_token_ref_json,
    document_version = excluded.document_version,
    document_hash = excluded.document_hash,
    registry_version = excluded.registry_version,
    identity_status = excluded.identity_status,
    legacy_migration_state = excluded.legacy_migration_state,
    last_error_code = excluded.last_error_code,
    identity_id = excluded.identity_id,
    updated_at_ms = excluded.updated_at_ms
"#,
        rusqlite::params![
            identity.agent_did,
            identity.handle,
            identity.display_name,
            identity.agent_kind.as_str(),
            identity.account_id,
            identity.full_handle,
            identity.binding_generation,
            identity.did_document.to_string(),
            identity.protocol_device_id,
            identity.root_key_id,
            root_private_key_ref_json,
            identity.device_signing_key_id,
            device_signing_private_key_ref_json,
            identity.device_e2ee_key_id,
            device_e2ee_private_key_ref_json,
            daemon_subkey_package_ref_json,
            identity.authorization_status,
            identity.role,
            if identity.management_ready {
                1_i64
            } else {
                0_i64
            },
            auth_generation,
            access_token_ref_json,
            document_version,
            identity.document_hash,
            registry_version,
            identity.identity_status,
            identity.legacy_migration_state,
            identity.last_error_code,
            identity.identity_id,
            now,
        ],
    )?;
    Ok(())
}

fn reset_agent_sync_probe_if_binding_changed(
    transaction: &Transaction<'_>,
    identity: &AgentDeviceIdentityRecord,
) -> Result<()> {
    let previous = transaction
        .query_row(
            r#"
SELECT protocol_device_id, auth_generation, binding_generation, document_hash
FROM agent_device_identity WHERE agent_did = ?1
"#,
            [&identity.agent_did],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?;
    let auth_generation = i64::try_from(identity.auth_generation)
        .context("auth_generation exceeds SQLite integer range")?;
    if previous.is_some_and(|previous| {
        previous.0 != identity.protocol_device_id
            || previous.1 != auth_generation
            || previous.2 != identity.binding_generation
            || previous.3 != identity.document_hash
    }) {
        transaction.execute(
            "DELETE FROM agent_sync_probe WHERE agent_did = ?1",
            [&identity.agent_did],
        )?;
    }
    Ok(())
}

struct StoredPendingAgentRegistration {
    registration_id: String,
    dedupe_key: String,
    agent_kind: crate::agent::AgentKind,
    controller_did: String,
    handle: String,
    display_name: String,
    agent_did: String,
    protocol_device_id: String,
    document_digest: String,
    request_digest: String,
    secret_ref_json: String,
    status: String,
    attempt_count: u32,
    last_error_code: Option<String>,
    last_error_summary: Option<String>,
    created_at_ms: i64,
    updated_at_ms: i64,
}

fn pending_registration_select_sql() -> &'static str {
    r#"
SELECT registration_id, dedupe_key, agent_kind, controller_did, handle,
       display_name, agent_did, protocol_device_id, document_digest,
       request_digest, secret_ref_json, status, attempt_count, last_error_code,
       last_error_summary, created_at_ms, updated_at_ms
FROM agent_registration_pending
"#
}

fn pending_registration_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<StoredPendingAgentRegistration> {
    let kind_raw: String = row.get(2)?;
    let agent_kind = crate::agent::AgentKind::parse(&kind_raw)
        .map_err(|error| to_sql_conversion_error(std::io::Error::other(error.to_string())))?;
    let attempt_count = u32::try_from(row.get::<_, i64>(12)?).map_err(to_sql_conversion_error)?;
    Ok(StoredPendingAgentRegistration {
        registration_id: row.get(0)?,
        dedupe_key: row.get(1)?,
        agent_kind,
        controller_did: row.get(3)?,
        handle: row.get(4)?,
        display_name: row.get(5)?,
        agent_did: row.get(6)?,
        protocol_device_id: row.get(7)?,
        document_digest: row.get(8)?,
        request_digest: row.get(9)?,
        secret_ref_json: row.get(10)?,
        status: row.get(11)?,
        attempt_count,
        last_error_code: row.get(13)?,
        last_error_summary: row.get(14)?,
        created_at_ms: row.get(15)?,
        updated_at_ms: row.get(16)?,
    })
}

struct StoredAgentDeviceIdentity {
    identity_id: String,
    agent_did: String,
    handle: String,
    display_name: String,
    agent_kind: crate::agent::AgentKind,
    account_id: String,
    full_handle: String,
    binding_generation: String,
    did_document: Value,
    protocol_device_id: String,
    root_key_id: String,
    root_private_key_ref_json: String,
    device_signing_key_id: String,
    device_signing_private_key_ref_json: String,
    device_e2ee_key_id: String,
    device_e2ee_private_key_ref_json: String,
    daemon_subkey_package_ref_json: Option<String>,
    authorization_status: String,
    role: String,
    management_ready: bool,
    auth_generation: u64,
    access_token_ref_json: String,
    document_version: u64,
    document_hash: String,
    registry_version: u64,
    identity_status: String,
    legacy_migration_state: String,
    last_error_code: Option<String>,
}

fn agent_device_identity_select_sql() -> &'static str {
    r#"
SELECT agent_did, handle, display_name, agent_kind, account_id, full_handle,
       binding_generation, did_document_json, protocol_device_id, root_key_id,
       root_private_key_ref_json, device_signing_key_id,
       device_signing_private_key_ref_json, device_e2ee_key_id,
       device_e2ee_private_key_ref_json, daemon_subkey_package_ref_json,
       authorization_status, role, management_ready, auth_generation,
       access_token_ref_json, document_version, document_hash, registry_version,
       identity_status, legacy_migration_state, last_error_code, identity_id
FROM agent_device_identity
WHERE agent_did = ?1
"#
}

fn agent_device_identity_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<StoredAgentDeviceIdentity> {
    let kind_raw: String = row.get(3)?;
    let agent_kind = crate::agent::AgentKind::parse(&kind_raw)
        .map_err(|error| to_sql_conversion_error(std::io::Error::other(error.to_string())))?;
    let document_json: String = row.get(7)?;
    let did_document = serde_json::from_str(&document_json).map_err(to_sql_conversion_error)?;
    let auth_generation = u64::try_from(row.get::<_, i64>(19)?).map_err(to_sql_conversion_error)?;
    let document_version =
        u64::try_from(row.get::<_, i64>(21)?).map_err(to_sql_conversion_error)?;
    let registry_version =
        u64::try_from(row.get::<_, i64>(23)?).map_err(to_sql_conversion_error)?;
    Ok(StoredAgentDeviceIdentity {
        identity_id: row.get(27)?,
        agent_did: row.get(0)?,
        handle: row.get(1)?,
        display_name: row.get(2)?,
        agent_kind,
        account_id: row.get(4)?,
        full_handle: row.get(5)?,
        binding_generation: row.get(6)?,
        did_document,
        protocol_device_id: row.get(8)?,
        root_key_id: row.get(9)?,
        root_private_key_ref_json: row.get(10)?,
        device_signing_key_id: row.get(11)?,
        device_signing_private_key_ref_json: row.get(12)?,
        device_e2ee_key_id: row.get(13)?,
        device_e2ee_private_key_ref_json: row.get(14)?,
        daemon_subkey_package_ref_json: row.get(15)?,
        authorization_status: row.get(16)?,
        role: row.get(17)?,
        management_ready: row.get::<_, i64>(18)? != 0,
        auth_generation,
        access_token_ref_json: row.get(20)?,
        document_version,
        document_hash: row.get(22)?,
        registry_version,
        identity_status: row.get(24)?,
        legacy_migration_state: row.get(25)?,
        last_error_code: row.get(26)?,
    })
}

fn to_sql_conversion_error(
    error: impl std::error::Error + Send + Sync + 'static,
) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}
