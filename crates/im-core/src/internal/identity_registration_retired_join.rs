//! Durable, secret-free convergence journal for ordinary registration Join
//! after one exact local identity retirement.

use rusqlite::OptionalExtension as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use std::path::Path;

pub(crate) const RETIRED_JOIN_ROLLOVER_TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS registration_retired_join_rollovers (
    join_session_id              TEXT PRIMARY KEY,
    schema_version               INTEGER NOT NULL,
    account_user_id              TEXT NOT NULL,
    owner_identity_id            TEXT NOT NULL,
    handle                       TEXT NOT NULL,
    retired_did                  TEXT NOT NULL,
    retired_device_id            TEXT NOT NULL,
    retired_binding_generation   TEXT NOT NULL,
    current_did                  TEXT NOT NULL,
    current_binding_generation   TEXT NOT NULL,
    new_device_id                TEXT NOT NULL,
    join_expires_at              TEXT NOT NULL,
    completed_auth_generation    TEXT,
    phase                        TEXT NOT NULL,
    created_at                   TEXT NOT NULL,
    updated_at                   TEXT NOT NULL,
    completed_at                 TEXT,
    CHECK (schema_version = 1),
    CHECK (phase IN ('prepared', 'completed')),
    CHECK (
        (phase = 'prepared' AND completed_auth_generation IS NULL AND completed_at IS NULL)
        OR
        (phase = 'completed' AND completed_auth_generation IS NOT NULL AND completed_at IS NOT NULL)
    )
);
"#;

pub(crate) const RETIRED_JOIN_ROLLOVER_INDEX_SQL: &str = r#"
CREATE INDEX IF NOT EXISTS registration_retired_join_rollovers_owner_phase_idx
ON registration_retired_join_rollovers(owner_identity_id, phase, updated_at);
"#;

const JOURNAL_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RetiredJoinRolloverPhase {
    Prepared,
    Completed,
}

impl RetiredJoinRolloverPhase {
    fn parse(value: &str) -> rusqlite::Result<Self> {
        match value {
            "prepared" => Ok(Self::Prepared),
            "completed" => Ok(Self::Completed),
            _ => Err(rusqlite::Error::InvalidQuery),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RetiredJoinRollover {
    pub(crate) join_session_id: String,
    pub(crate) schema_version: u32,
    pub(crate) account_user_id: String,
    pub(crate) owner_identity_id: String,
    pub(crate) handle: String,
    pub(crate) retired_did: String,
    pub(crate) retired_device_id: String,
    pub(crate) retired_binding_generation: String,
    pub(crate) current_did: String,
    pub(crate) current_binding_generation: String,
    pub(crate) new_device_id: String,
    pub(crate) join_expires_at: String,
    pub(crate) completed_auth_generation: Option<String>,
    pub(crate) phase: RetiredJoinRolloverPhase,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    pub(crate) completed_at: Option<String>,
}

impl RetiredJoinRollover {
    pub(crate) fn prepared(
        join_session_id: &str,
        account_user_id: &str,
        handle: &str,
        transition: &crate::internal::identity_registration_join_preparation::RegistrationJoinTransition,
        evidence: &crate::internal::identity_local_owner_matcher::RetiredOwnerEvidence,
        new_device_id: &str,
        join_expires_at: &str,
    ) -> crate::ImResult<Self> {
        let evidence_matches_transition = match evidence.epoch_relation {
            crate::internal::identity_local_owner_matcher::RetiredOwnerEpochRelation::Current => {
                evidence.retired_did == transition.current_did
                    && evidence.retired_binding_generation == transition.binding_generation
            }
            crate::internal::identity_local_owner_matcher::RetiredOwnerEpochRelation::DirectPrevious => {
                evidence.retired_did == transition.previous_did
                    && crate::internal::identity_handle_recovery_pending::increment_canonical_generation(
                        &evidence.retired_binding_generation,
                    )
                    .as_deref()
                        == Some(transition.binding_generation.as_str())
            }
        };
        if account_user_id != transition.account_user_id || !evidence_matches_transition {
            return Err(crate::ImError::PermissionDenied);
        }
        let now = now()?;
        let record = Self {
            join_session_id: join_session_id.to_owned(),
            schema_version: JOURNAL_SCHEMA_VERSION,
            account_user_id: account_user_id.to_owned(),
            owner_identity_id: evidence.owner_identity_id.clone(),
            handle: handle.to_owned(),
            retired_did: evidence.retired_did.clone(),
            retired_device_id: evidence.retired_protocol_device_id.clone(),
            retired_binding_generation: evidence.retired_binding_generation.clone(),
            current_did: transition.current_did.clone(),
            current_binding_generation: transition.binding_generation.clone(),
            new_device_id: new_device_id.to_owned(),
            join_expires_at: join_expires_at.to_owned(),
            completed_auth_generation: None,
            phase: RetiredJoinRolloverPhase::Prepared,
            created_at: now.clone(),
            updated_at: now,
            completed_at: None,
        };
        record.validate()?;
        Ok(record)
    }

    pub(crate) fn validate(&self) -> crate::ImResult<()> {
        if self.schema_version != JOURNAL_SCHEMA_VERSION
            || self.join_session_id.trim().is_empty()
            || self.account_user_id.trim().is_empty()
            || self.owner_identity_id.trim().is_empty()
            || self.created_at.trim().is_empty()
            || self.updated_at.trim().is_empty()
            || (self.retired_did == self.current_did
                && self.retired_binding_generation != self.current_binding_generation)
            || self.retired_device_id == self.new_device_id
            || (self.phase == RetiredJoinRolloverPhase::Prepared
                && (self.completed_auth_generation.is_some() || self.completed_at.is_some()))
            || (self.phase == RetiredJoinRolloverPhase::Completed
                && (self.completed_auth_generation.is_none() || self.completed_at.is_none()))
        {
            return Err(crate::ImError::PermissionDenied);
        }
        crate::internal::identity_wire::handle_recovery::canonical_handle(&self.handle)?;
        crate::ids::Did::parse(&self.retired_did)?;
        crate::ids::Did::parse(&self.current_did)?;
        crate::ids::ProtocolDeviceId::parse(&self.retired_device_id)?;
        crate::ids::ProtocolDeviceId::parse(&self.new_device_id)?;
        anp::wns::BindingGeneration::new(self.retired_binding_generation.clone())
            .map_err(|_| crate::ImError::PermissionDenied)?;
        anp::wns::BindingGeneration::new(self.current_binding_generation.clone())
            .map_err(|_| crate::ImError::PermissionDenied)?;
        time::OffsetDateTime::parse(
            &self.join_expires_at,
            &time::format_description::well_known::Rfc3339,
        )
        .map_err(|_| crate::ImError::PermissionDenied)?;
        for timestamp in [
            Some(self.created_at.as_str()),
            Some(self.updated_at.as_str()),
            self.completed_at.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            time::OffsetDateTime::parse(timestamp, &time::format_description::well_known::Rfc3339)
                .map_err(|_| crate::ImError::PermissionDenied)?;
        }
        if self
            .completed_auth_generation
            .as_deref()
            .is_some_and(|value| anp::wns::BindingGeneration::new(value.to_owned()).is_err())
        {
            return Err(crate::ImError::PermissionDenied);
        }
        let epoch_is_current = self.retired_did == self.current_did
            && self.retired_binding_generation == self.current_binding_generation;
        let epoch_is_direct_previous = self.retired_did != self.current_did
            && crate::internal::identity_handle_recovery_pending::increment_canonical_generation(
                &self.retired_binding_generation,
            )
            .as_deref()
                == Some(self.current_binding_generation.as_str());
        if !epoch_is_current && !epoch_is_direct_previous {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(())
    }
}

pub(crate) fn insert_prepared(
    sqlite_path: &Path,
    record: &RetiredJoinRollover,
) -> crate::ImResult<()> {
    record.validate()?;
    if record.phase != RetiredJoinRolloverPhase::Prepared {
        return Err(crate::ImError::PermissionDenied);
    }
    let mut connection = crate::internal::local_state::open_writable(sqlite_path)?;
    let transaction = connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    crate::internal::identity_local_deletion::ensure_no_active_deletion(
        &transaction,
        &record.owner_identity_id,
        Some(&record.handle),
    )?;
    let changed = transaction
        .execute(
            r#"INSERT OR IGNORE INTO registration_retired_join_rollovers
                (join_session_id,schema_version,account_user_id,owner_identity_id,handle,
                 retired_did,retired_device_id,retired_binding_generation,current_did,
                 current_binding_generation,new_device_id,join_expires_at,
                 completed_auth_generation,phase,created_at,updated_at,completed_at)
               VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,NULL,'prepared',?13,?13,NULL)"#,
            rusqlite::params![
                record.join_session_id,
                record.schema_version,
                record.account_user_id,
                record.owner_identity_id,
                record.handle,
                record.retired_did,
                record.retired_device_id,
                record.retired_binding_generation,
                record.current_did,
                record.current_binding_generation,
                record.new_device_id,
                record.join_expires_at,
                record.created_at,
            ],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    if changed != 1
        && load_from_connection(&transaction, &record.join_session_id)?.as_ref() != Some(record)
    {
        return Err(crate::ImError::PermissionDenied);
    }
    transaction
        .commit()
        .map_err(crate::internal::local_state::local_state_unavailable)
}

pub(crate) fn load(
    sqlite_path: &Path,
    join_session_id: &str,
) -> crate::ImResult<Option<RetiredJoinRollover>> {
    let connection = crate::internal::local_state::open_writable(sqlite_path)?;
    load_from_connection(&connection, join_session_id)
}

fn load_from_connection(
    connection: &rusqlite::Connection,
    join_session_id: &str,
) -> crate::ImResult<Option<RetiredJoinRollover>> {
    connection
        .query_row(
            r#"SELECT join_session_id,schema_version,account_user_id,owner_identity_id,handle,
retired_did,retired_device_id,retired_binding_generation,current_did,
current_binding_generation,new_device_id,join_expires_at,completed_auth_generation,
phase,created_at,updated_at,completed_at
FROM registration_retired_join_rollovers WHERE join_session_id=?1"#,
            [join_session_id],
            row_to_record,
        )
        .optional()
        .map_err(crate::internal::local_state::local_state_unavailable)?
        .map(|record| {
            record.validate()?;
            Ok(record)
        })
        .transpose()
}

pub(crate) fn converge_after_registry_save(
    sqlite_path: &Path,
    expected: &RetiredJoinRollover,
    auth_generation: u64,
) -> crate::ImResult<()> {
    expected.validate()?;
    if auth_generation == 0 {
        return Err(crate::ImError::PermissionDenied);
    }
    let mut connection = crate::internal::local_state::open_writable(sqlite_path)?;
    let transaction = connection
        .transaction()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let current = load_from_connection(&transaction, &expected.join_session_id)?
        .ok_or(crate::ImError::PermissionDenied)?;
    if &current != expected {
        return Err(crate::ImError::PermissionDenied);
    }

    let binding: Option<(String, Option<String>, String, String, String, String)> = transaction
        .query_row(
            r#"SELECT account_id,handle_scope,current_did,device_id,
identity_generation,device_auth_generation
FROM identity_account_bindings WHERE owner_identity_id=?1"#,
            [&expected.owner_identity_id],
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
        .optional()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let auth_generation = auth_generation.to_string();
    let final_binding = (
        expected.account_user_id.clone(),
        Some(expected.handle.clone()),
        expected.current_did.clone(),
        expected.new_device_id.clone(),
        expected.current_binding_generation.clone(),
        auth_generation.clone(),
    );
    if current.phase == RetiredJoinRolloverPhase::Completed {
        if binding == Some(final_binding)
            && current.completed_auth_generation.as_deref() == Some(auth_generation.as_str())
        {
            return Ok(());
        }
        return Err(crate::ImError::PermissionDenied);
    }
    let retired_binding = (
        expected.account_user_id.clone(),
        Some(expected.handle.clone()),
        expected.retired_did.clone(),
        expected.retired_device_id.clone(),
        expected.retired_binding_generation.clone(),
    );
    if binding.as_ref().map(|value| {
        (
            value.0.clone(),
            value.1.clone(),
            value.2.clone(),
            value.3.clone(),
            value.4.clone(),
        )
    }) != Some(retired_binding)
    {
        return Err(crate::ImError::PermissionDenied);
    }

    let now_unix = time::OffsetDateTime::now_utc().unix_timestamp();
    let changed = transaction
        .execute(
            r#"UPDATE identity_account_bindings
SET current_did=?2,device_id=?3,identity_generation=?4,
    device_auth_generation=?5,updated_at=?6
WHERE owner_identity_id=?1 AND account_id=?7 AND handle_scope=?8
  AND current_did=?9 AND device_id=?10 AND identity_generation=?11"#,
            rusqlite::params![
                expected.owner_identity_id,
                expected.current_did,
                expected.new_device_id,
                expected.current_binding_generation,
                auth_generation,
                now_unix,
                expected.account_user_id,
                expected.handle,
                expected.retired_did,
                expected.retired_device_id,
                expected.retired_binding_generation,
            ],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    if changed != 1 {
        return Err(crate::ImError::PermissionDenied);
    }

    let now = now()?;
    crate::internal::identity_transition_pending::retire_owner_write_state(
        &transaction,
        &expected.owner_identity_id,
        &expected.retired_did,
        &now,
    )?;
    crate::internal::identity_transition_pending::clear_owner_join_control_state(
        &transaction,
        &expected.owner_identity_id,
    )?;

    transaction
        .execute(
            "UPDATE identity_did_history SET status='previous',last_seen_at=?1 WHERE owner_identity_id=?2 AND did<>?3",
            rusqlite::params![now, expected.owner_identity_id, expected.current_did],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let relation_digest = rollover_relation_digest(expected);
    transaction
        .execute(
            r#"INSERT INTO identity_did_history
                (owner_identity_id,did,status,first_seen_at,last_seen_at,metadata)
               VALUES (?1,?2,'current',?3,?3,?4)
               ON CONFLICT(owner_identity_id,did) DO UPDATE SET
                 status='current',last_seen_at=excluded.last_seen_at,metadata=excluded.metadata"#,
            rusqlite::params![
                expected.owner_identity_id,
                expected.current_did,
                now,
                serde_json::json!({
                    "protocol": "registration_retired_join_v1",
                    "relation_digest": relation_digest,
                    "binding_generation": expected.current_binding_generation,
                })
                .to_string(),
            ],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let completed = transaction
        .execute(
            r#"UPDATE registration_retired_join_rollovers
SET phase='completed',completed_auth_generation=?1,completed_at=?2,updated_at=?2
WHERE join_session_id=?3 AND phase='prepared' AND owner_identity_id=?4
  AND retired_did=?5 AND retired_device_id=?6 AND new_device_id=?7"#,
            rusqlite::params![
                auth_generation,
                now,
                expected.join_session_id,
                expected.owner_identity_id,
                expected.retired_did,
                expected.retired_device_id,
                expected.new_device_id,
            ],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    if completed != 1 {
        return Err(crate::ImError::PermissionDenied);
    }
    transaction
        .commit()
        .map_err(crate::internal::local_state::local_state_unavailable)
}

pub(crate) fn recover_all(core: &crate::core::ImCore) -> crate::ImResult<()> {
    let sqlite_path = &core.inner().sdk_paths().local_state.sqlite_path;
    let records = list(sqlite_path)?;
    if records.is_empty() {
        return Ok(());
    }
    let index =
        crate::internal::identity_store::IdentityStore::new(&core.inner().sdk_paths().identities)
            .load_index()?;
    let mut by_owner = BTreeMap::<String, Vec<RetiredJoinRollover>>::new();
    for record in records {
        if record.phase == RetiredJoinRolloverPhase::Prepared
            && !crate::internal::identity_retirement::matches_completed_binding(
                &core.inner().sdk_paths().identities.identity_root_dir,
                &record.owner_identity_id,
                &record.retired_did,
                &record.retired_device_id,
            )?
        {
            return Err(crate::ImError::PermissionDenied);
        }
        by_owner
            .entry(record.owner_identity_id.clone())
            .or_default()
            .push(record);
    }

    for owner_records in by_owner.values() {
        let sample = owner_records
            .first()
            .ok_or(crate::ImError::PermissionDenied)?;
        if owner_records.iter().any(|record| {
            record.account_user_id != sample.account_user_id || record.handle != sample.handle
        }) {
            return Err(crate::ImError::PermissionDenied);
        }
        let related_entries = index
            .credentials
            .iter()
            .filter(|(_, entry)| {
                entry.unique_id == sample.owner_identity_id
                    || entry.user_id == sample.account_user_id
                    || entry.full_handle == sample.handle
                    || owner_records
                        .iter()
                        .any(|record| entry.did == record.current_did)
            })
            .collect::<Vec<_>>();
        if related_entries.is_empty() {
            cleanup_prepared_orphans(core, owner_records, None)?;
            continue;
        }
        if related_entries.len() != 1 {
            return Err(crate::ImError::PermissionDenied);
        }
        let (_, entry) = related_entries[0];
        let (registry_device_id, auth_generation) = registry_device_authority(entry)?;
        let exact_records = owner_records
            .iter()
            .filter(|record| registry_entry_matches(record, entry, registry_device_id))
            .collect::<Vec<_>>();
        let completed = exact_records
            .iter()
            .filter(|record| record.phase == RetiredJoinRolloverPhase::Completed)
            .copied()
            .collect::<Vec<_>>();
        let winner = if completed.len() == 1 {
            completed[0]
        } else if completed.is_empty() {
            let prepared = exact_records
                .iter()
                .filter(|record| record.phase == RetiredJoinRolloverPhase::Prepared)
                .copied()
                .collect::<Vec<_>>();
            let has_any_prepared = owner_records
                .iter()
                .any(|record| record.phase == RetiredJoinRolloverPhase::Prepared);
            if prepared.is_empty() && !has_any_prepared {
                // Completed journals are permanent audit rows. A later exact
                // retirement or Recovery may legitimately leave them with no
                // current Registry match; they must not become a new blocker.
                continue;
            }
            if prepared.len() != 1 {
                return Err(crate::ImError::PermissionDenied);
            }
            prepared[0]
        } else {
            return Err(crate::ImError::PermissionDenied);
        };
        converge_after_registry_save(sqlite_path, winner, auth_generation)?;
        cleanup_prepared_orphans(core, owner_records, Some(&winner.join_session_id))?;
    }
    Ok(())
}

pub(crate) fn completed_rollover_supersedes_retirement(
    core: &crate::core::ImCore,
    owner_identity_id: &str,
    retired_did: &str,
    retired_device_id: &str,
) -> crate::ImResult<bool> {
    let sqlite_path = &core.inner().sdk_paths().local_state.sqlite_path;
    let matching = list(sqlite_path)?
        .into_iter()
        .filter(|record| {
            record.phase == RetiredJoinRolloverPhase::Completed
                && record.owner_identity_id == owner_identity_id
                && record.retired_did == retired_did
                && record.retired_device_id == retired_device_id
        })
        .collect::<Vec<_>>();
    if matching.len() != 1 {
        return Ok(false);
    }
    let record = &matching[0];
    let index =
        crate::internal::identity_store::IdentityStore::new(&core.inner().sdk_paths().identities)
            .load_index()?;
    let related = index
        .credentials
        .values()
        .filter(|entry| {
            entry.unique_id == record.owner_identity_id
                || entry.user_id == record.account_user_id
                || entry.full_handle == record.handle
                || entry.did == record.current_did
        })
        .collect::<Vec<_>>();
    if related.len() != 1 {
        return Ok(false);
    }
    let entry = related[0];
    let (device_id, auth_generation) = registry_device_authority(entry)?;
    let auth_generation_text = auth_generation.to_string();
    if !registry_entry_matches(record, entry, device_id)
        || record.completed_auth_generation.as_deref() != Some(auth_generation_text.as_str())
    {
        return Ok(false);
    }
    let connection = crate::internal::local_state::open_writable(sqlite_path)?;
    let binding_count: i64 = connection
        .query_row(
            r#"SELECT COUNT(*) FROM identity_account_bindings
WHERE owner_identity_id=?1 AND account_id=?2 AND handle_scope=?3
  AND current_did=?4 AND device_id=?5 AND identity_generation=?6
  AND device_auth_generation=?7"#,
            rusqlite::params![
                record.owner_identity_id,
                record.account_user_id,
                record.handle,
                record.current_did,
                record.new_device_id,
                record.current_binding_generation,
                auth_generation_text,
            ],
            |row| row.get(0),
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    Ok(binding_count == 1)
}

fn list(sqlite_path: &Path) -> crate::ImResult<Vec<RetiredJoinRollover>> {
    if !sqlite_path.is_file() {
        return Ok(Vec::new());
    }
    let connection = crate::internal::local_state::open_writable(sqlite_path)?;
    let mut statement = connection
        .prepare(
            r#"SELECT join_session_id,schema_version,account_user_id,owner_identity_id,handle,
retired_did,retired_device_id,retired_binding_generation,current_did,
current_binding_generation,new_device_id,join_expires_at,completed_auth_generation,
phase,created_at,updated_at,completed_at
FROM registration_retired_join_rollovers
ORDER BY owner_identity_id,created_at,join_session_id"#,
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let records = statement
        .query_map([], row_to_record)
        .map_err(crate::internal::local_state::local_state_unavailable)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    records
        .into_iter()
        .map(|record| {
            record.validate()?;
            Ok(record)
        })
        .collect()
}

fn cleanup_prepared_orphans(
    core: &crate::core::ImCore,
    records: &[RetiredJoinRollover],
    excluded_winner: Option<&str>,
) -> crate::ImResult<()> {
    for record in records.iter().filter(|record| {
        record.phase == RetiredJoinRolloverPhase::Prepared
            && excluded_winner != Some(record.join_session_id.as_str())
    }) {
        let should_delete = match core.device_join().session(
            &record.join_session_id,
            crate::identity::DeviceJoinSide::NewDevice,
        ) {
            Ok(session) => matches!(
                session.phase,
                crate::identity::DeviceJoinLocalPhase::Cancelled
                    | crate::identity::DeviceJoinLocalPhase::Expired
            ),
            Err(crate::ImError::IdentityNotFound { .. }) => {
                time::OffsetDateTime::parse(
                    &record.join_expires_at,
                    &time::format_description::well_known::Rfc3339,
                )
                .map_err(|_| crate::ImError::PermissionDenied)?
                    <= time::OffsetDateTime::now_utc()
            }
            Err(error) => return Err(error),
        };
        if should_delete {
            let connection = crate::internal::local_state::open_writable(
                &core.inner().sdk_paths().local_state.sqlite_path,
            )?;
            let changed = connection
                .execute(
                    "DELETE FROM registration_retired_join_rollovers WHERE join_session_id=?1 AND phase='prepared' AND new_device_id=?2",
                    rusqlite::params![record.join_session_id, record.new_device_id],
                )
                .map_err(crate::internal::local_state::local_state_unavailable)?;
            if changed != 1 {
                return Err(crate::ImError::PermissionDenied);
            }
        }
    }
    Ok(())
}

fn registry_device_authority(
    entry: &crate::internal::identity_store::IndexEntry,
) -> crate::ImResult<(&str, u64)> {
    let authorization = entry
        .device_state
        .as_ref()
        .and_then(|state| state.authorization.as_ref())
        .ok_or(crate::ImError::PermissionDenied)?;
    if authorization.auth_generation == 0 {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok((
        authorization.protocol_device_id.as_str(),
        authorization.auth_generation,
    ))
}

fn registry_entry_matches(
    record: &RetiredJoinRollover,
    entry: &crate::internal::identity_store::IndexEntry,
    device_id: &str,
) -> bool {
    entry.unique_id == record.owner_identity_id
        && entry.user_id == record.account_user_id
        && entry.full_handle == record.handle
        && entry.did == record.current_did
        && entry.binding_generation.as_deref() == Some(record.current_binding_generation.as_str())
        && device_id == record.new_device_id
}

fn rollover_relation_digest(record: &RetiredJoinRollover) -> String {
    let mut digest = Sha256::new();
    digest.update(b"awiki.registration-retired-join.relation.v1\0");
    for value in [
        &record.owner_identity_id,
        &record.retired_did,
        &record.retired_device_id,
        &record.retired_binding_generation,
        &record.current_did,
        &record.current_binding_generation,
        &record.new_device_id,
    ] {
        digest.update(value.as_bytes());
        digest.update(b"\0");
    }
    format!("sha256:{:x}", digest.finalize())
}

fn row_to_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<RetiredJoinRollover> {
    Ok(RetiredJoinRollover {
        join_session_id: row.get(0)?,
        schema_version: row.get(1)?,
        account_user_id: row.get(2)?,
        owner_identity_id: row.get(3)?,
        handle: row.get(4)?,
        retired_did: row.get(5)?,
        retired_device_id: row.get(6)?,
        retired_binding_generation: row.get(7)?,
        current_did: row.get(8)?,
        current_binding_generation: row.get(9)?,
        new_device_id: row.get(10)?,
        join_expires_at: row.get(11)?,
        completed_auth_generation: row.get(12)?,
        phase: RetiredJoinRolloverPhase::parse(&row.get::<_, String>(13)?)?,
        created_at: row.get(14)?,
        updated_at: row.get(15)?,
        completed_at: row.get(16)?,
    })
}

fn now() -> crate::ImResult<String> {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|error| crate::ImError::Serialization {
            detail: error.to_string(),
        })
}

#[cfg(test)]
mod tests;
