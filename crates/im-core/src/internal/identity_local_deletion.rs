//! Crash-safe local identity deletion admission and journal coordination.

use rusqlite::{OptionalExtension as _, TransactionBehavior};
use std::path::Path;

pub(crate) const LOCAL_IDENTITY_DELETION_TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS local_identity_deletions (
    deletion_id             TEXT PRIMARY KEY,
    schema_version          INTEGER NOT NULL,
    mode                    TEXT NOT NULL,
    owner_identity_id       TEXT NOT NULL,
    current_did             TEXT NOT NULL,
    full_handle             TEXT,
    local_alias             TEXT NOT NULL,
    identity_dir_name       TEXT,
    next_default_alias      TEXT,
    protocol_device_id      TEXT,
    phase                   TEXT NOT NULL,
    created_at              TEXT NOT NULL,
    updated_at              TEXT NOT NULL,
    completed_at            TEXT,
    CHECK (schema_version = 1),
    CHECK (mode IN ('credential_only', 'full_data_core', 'full_data_app')),
    CHECK (phase IN ('prepared', 'retirement_ready', 'completed')),
    CHECK (
        (phase <> 'completed' AND completed_at IS NULL)
        OR (phase = 'completed' AND completed_at IS NOT NULL)
    )
);
"#;

pub(crate) const LOCAL_IDENTITY_DELETION_INDEX_SQL: &str = r#"
CREATE UNIQUE INDEX IF NOT EXISTS local_identity_deletions_active_owner_idx
ON local_identity_deletions(owner_identity_id)
WHERE phase <> 'completed';

CREATE INDEX IF NOT EXISTS local_identity_deletions_handle_phase_idx
ON local_identity_deletions(full_handle, phase, updated_at);
"#;

const JOURNAL_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LocalIdentityDeletionMode {
    CredentialOnly,
    FullDataCore,
    FullDataApp,
}

impl LocalIdentityDeletionMode {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::CredentialOnly => "credential_only",
            Self::FullDataCore => "full_data_core",
            Self::FullDataApp => "full_data_app",
        }
    }

    pub(crate) fn parse(value: &str) -> crate::ImResult<Self> {
        match value {
            "credential_only" => Ok(Self::CredentialOnly),
            "full_data_core" => Ok(Self::FullDataCore),
            "full_data_app" => Ok(Self::FullDataApp),
            _ => Err(crate::ImError::PermissionDenied),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LocalIdentityDeletionPhase {
    Prepared,
    RetirementReady,
    Completed,
}

impl LocalIdentityDeletionPhase {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::RetirementReady => "retirement_ready",
            Self::Completed => "completed",
        }
    }

    fn parse(value: &str) -> crate::ImResult<Self> {
        match value {
            "prepared" => Ok(Self::Prepared),
            "retirement_ready" => Ok(Self::RetirementReady),
            "completed" => Ok(Self::Completed),
            _ => Err(crate::ImError::PermissionDenied),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocalIdentityDeletionSnapshot {
    pub(crate) owner_identity_id: String,
    pub(crate) current_did: String,
    pub(crate) full_handle: Option<String>,
    pub(crate) local_alias: String,
    pub(crate) identity_dir_name: Option<String>,
    pub(crate) next_default_alias: Option<String>,
    pub(crate) protocol_device_id: Option<String>,
}

impl LocalIdentityDeletionSnapshot {
    fn validate(&self) -> crate::ImResult<()> {
        if self.owner_identity_id.trim().is_empty() || self.local_alias.trim().is_empty() {
            return Err(crate::ImError::PermissionDenied);
        }
        crate::ids::IdentityId::parse(&self.owner_identity_id)?;
        crate::ids::Did::parse(&self.current_did)?;
        if let Some(handle) = self.full_handle.as_deref() {
            crate::internal::identity_wire::handle_recovery::canonical_handle(handle)?;
        }
        if let Some(device_id) = self.protocol_device_id.as_deref() {
            crate::ids::ProtocolDeviceId::parse(device_id)?;
        }
        for value in [
            self.identity_dir_name.as_deref(),
            self.next_default_alias.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            if value.trim().is_empty() {
                return Err(crate::ImError::PermissionDenied);
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocalIdentityDeletionRecord {
    pub(crate) deletion_id: String,
    pub(crate) schema_version: u32,
    pub(crate) mode: LocalIdentityDeletionMode,
    pub(crate) owner_identity_id: String,
    pub(crate) current_did: String,
    pub(crate) full_handle: Option<String>,
    pub(crate) local_alias: String,
    pub(crate) identity_dir_name: Option<String>,
    pub(crate) next_default_alias: Option<String>,
    pub(crate) protocol_device_id: Option<String>,
    pub(crate) phase: LocalIdentityDeletionPhase,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    pub(crate) completed_at: Option<String>,
}

impl LocalIdentityDeletionRecord {
    pub(crate) fn snapshot(&self) -> LocalIdentityDeletionSnapshot {
        LocalIdentityDeletionSnapshot {
            owner_identity_id: self.owner_identity_id.clone(),
            current_did: self.current_did.clone(),
            full_handle: self.full_handle.clone(),
            local_alias: self.local_alias.clone(),
            identity_dir_name: self.identity_dir_name.clone(),
            next_default_alias: self.next_default_alias.clone(),
            protocol_device_id: self.protocol_device_id.clone(),
        }
    }

    fn validate(&self) -> crate::ImResult<()> {
        self.snapshot().validate()?;
        if self.schema_version != JOURNAL_SCHEMA_VERSION
            || self.deletion_id.trim().is_empty()
            || self.created_at.trim().is_empty()
            || self.updated_at.trim().is_empty()
            || (self.phase == LocalIdentityDeletionPhase::Completed) != self.completed_at.is_some()
        {
            return Err(crate::ImError::PermissionDenied);
        }
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
        Ok(())
    }
}

pub(crate) fn prepare(
    sqlite_path: &Path,
    snapshot: &LocalIdentityDeletionSnapshot,
    mode: LocalIdentityDeletionMode,
) -> crate::ImResult<LocalIdentityDeletionRecord> {
    let mut random = [0_u8; 24];
    rand::RngCore::try_fill_bytes(&mut rand::rngs::OsRng, &mut random).map_err(|error| {
        crate::ImError::Internal {
            message: error.to_string(),
        }
    })?;
    let deletion_id = format!(
        "delete_{}",
        base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, random)
    );
    prepare_with_id(sqlite_path, snapshot, mode, &deletion_id, &now_second_z()?)
}

pub(crate) fn prepare_with_id(
    sqlite_path: &Path,
    snapshot: &LocalIdentityDeletionSnapshot,
    mode: LocalIdentityDeletionMode,
    deletion_id: &str,
    now: &str,
) -> crate::ImResult<LocalIdentityDeletionRecord> {
    snapshot.validate()?;
    if deletion_id.trim().is_empty() {
        return Err(crate::ImError::PermissionDenied);
    }
    time::OffsetDateTime::parse(now, &time::format_description::well_known::Rfc3339)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    let mut connection = crate::internal::local_state::open_writable(sqlite_path)?;
    crate::internal::local_state::schema::ensure_schema(&connection)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(crate::internal::local_state::local_state_unavailable)?;

    if let Some(existing) =
        load_active_owner_from_connection(&transaction, &snapshot.owner_identity_id)?
    {
        if existing.mode == mode && existing.snapshot() == *snapshot {
            transaction
                .commit()
                .map_err(crate::internal::local_state::local_state_unavailable)?;
            return Ok(existing);
        }
        return Err(deletion_error("identity.local_data_deletion_pending"));
    }
    if let Some(full_handle) = snapshot.full_handle.as_deref() {
        if load_active_handle_from_connection(&transaction, full_handle)?.is_some() {
            return Err(deletion_error("identity.local_deletion_conflict"));
        }
    }
    if let Some(code) = admission_blocker(&transaction, snapshot)? {
        return Err(deletion_error(code));
    }

    let record = LocalIdentityDeletionRecord {
        deletion_id: deletion_id.to_owned(),
        schema_version: JOURNAL_SCHEMA_VERSION,
        mode,
        owner_identity_id: snapshot.owner_identity_id.clone(),
        current_did: snapshot.current_did.clone(),
        full_handle: snapshot.full_handle.clone(),
        local_alias: snapshot.local_alias.clone(),
        identity_dir_name: snapshot.identity_dir_name.clone(),
        next_default_alias: snapshot.next_default_alias.clone(),
        protocol_device_id: snapshot.protocol_device_id.clone(),
        phase: LocalIdentityDeletionPhase::Prepared,
        created_at: now.to_owned(),
        updated_at: now.to_owned(),
        completed_at: None,
    };
    record.validate()?;
    transaction
        .execute(
            r#"INSERT INTO local_identity_deletions
(deletion_id,schema_version,mode,owner_identity_id,current_did,full_handle,local_alias,
 identity_dir_name,next_default_alias,protocol_device_id,phase,created_at,updated_at,completed_at)
VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'prepared',?11,?11,NULL)"#,
            rusqlite::params![
                record.deletion_id,
                record.schema_version,
                record.mode.as_str(),
                record.owner_identity_id,
                record.current_did,
                record.full_handle,
                record.local_alias,
                record.identity_dir_name,
                record.next_default_alias,
                record.protocol_device_id,
                record.created_at,
            ],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    transaction
        .commit()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    Ok(record)
}

pub(crate) fn load_active_owner(
    sqlite_path: &Path,
    owner_identity_id: &str,
) -> crate::ImResult<Option<LocalIdentityDeletionRecord>> {
    let connection = crate::internal::local_state::open_writable(sqlite_path)?;
    crate::internal::local_state::schema::ensure_schema(&connection)?;
    load_active_owner_from_connection(&connection, owner_identity_id)
}

pub(crate) fn load(
    sqlite_path: &Path,
    deletion_id: &str,
) -> crate::ImResult<Option<LocalIdentityDeletionRecord>> {
    let connection = crate::internal::local_state::open_writable(sqlite_path)?;
    crate::internal::local_state::schema::ensure_schema(&connection)?;
    load_from_connection(&connection, deletion_id)
}

fn load_from_connection(
    connection: &rusqlite::Connection,
    deletion_id: &str,
) -> crate::ImResult<Option<LocalIdentityDeletionRecord>> {
    connection
        .query_row(
            r#"SELECT deletion_id,schema_version,mode,owner_identity_id,current_did,full_handle,
local_alias,identity_dir_name,next_default_alias,protocol_device_id,phase,created_at,updated_at,
completed_at FROM local_identity_deletions WHERE deletion_id=?1"#,
            [deletion_id],
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

pub(crate) fn advance_sqlite_phase(
    sqlite_path: &Path,
    deletion_id: &str,
    allow_full_data_app: bool,
) -> crate::ImResult<LocalIdentityDeletionRecord> {
    let mut connection = crate::internal::local_state::open_writable(sqlite_path)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let mut record = load_from_connection(&transaction, deletion_id)?
        .ok_or_else(|| deletion_error("identity.local_deletion_conflict"))?;
    if record.phase != LocalIdentityDeletionPhase::Prepared {
        transaction
            .commit()
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        return Ok(record);
    }
    if record.mode == LocalIdentityDeletionMode::FullDataApp && !allow_full_data_app {
        transaction
            .commit()
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        return Ok(record);
    }
    if matches!(
        record.mode,
        LocalIdentityDeletionMode::FullDataCore | LocalIdentityDeletionMode::FullDataApp
    ) {
        crate::internal::local_state::owner_scope::delete_owner_data_in_transaction(
            &transaction,
            &record.owner_identity_id,
            &record.current_did,
        )?;
        #[cfg(test)]
        if FAIL_AFTER_BUSINESS_DELETE.with(|fail| fail.replace(false)) {
            return Err(crate::ImError::LocalStateUnavailable {
                detail: "injected identity deletion business transaction failure".to_owned(),
            });
        }
    }
    let now = now_second_z()?;
    let changed = transaction
        .execute(
            "UPDATE local_identity_deletions SET phase='retirement_ready',updated_at=?2 WHERE deletion_id=?1 AND phase='prepared'",
            rusqlite::params![deletion_id, now],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    if changed != 1 {
        return Err(deletion_error("identity.local_deletion_conflict"));
    }
    transaction
        .commit()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    record.phase = LocalIdentityDeletionPhase::RetirementReady;
    record.updated_at = now;
    Ok(record)
}

pub(crate) fn complete(
    core: &crate::core::ImCore,
    deletion_id: &str,
    allow_full_data_app: bool,
) -> crate::ImResult<(LocalIdentityDeletionRecord, Vec<String>)> {
    let sqlite_path = &core.inner().sdk_paths().local_state.sqlite_path;
    let mut record = advance_sqlite_phase(sqlite_path, deletion_id, allow_full_data_app)?;
    if record.phase == LocalIdentityDeletionPhase::Prepared {
        return Err(deletion_error("identity.local_data_deletion_pending"));
    }
    if record.phase == LocalIdentityDeletionPhase::Completed {
        return Ok((record, Vec::new()));
    }
    let retirement = retirement_input(&record);
    let outcome = crate::internal::identity_retirement::retire(core, retirement.clone())?;
    if !crate::internal::identity_retirement::is_completed(core, &retirement)? {
        return Err(deletion_error("identity.local_data_deletion_pending"));
    }
    record = mark_completed(sqlite_path, deletion_id)?;
    Ok((record, outcome.warnings))
}

pub(crate) fn recover_before_retirement(core: &crate::core::ImCore) -> crate::ImResult<()> {
    let sqlite_path = &core.inner().sdk_paths().local_state.sqlite_path;
    for record in list_incomplete(sqlite_path)? {
        let record = match (record.phase, record.mode) {
            (LocalIdentityDeletionPhase::Prepared, LocalIdentityDeletionMode::FullDataApp) => {
                continue
            }
            (LocalIdentityDeletionPhase::Prepared, _) => {
                advance_sqlite_phase(sqlite_path, &record.deletion_id, false)?
            }
            _ => record,
        };
        if record.phase == LocalIdentityDeletionPhase::RetirementReady {
            crate::internal::identity_retirement::ensure_prepared(
                core,
                &retirement_input(&record),
            )?;
        }
    }
    Ok(())
}

pub(crate) fn recover_after_retirement(core: &crate::core::ImCore) -> crate::ImResult<()> {
    let sqlite_path = &core.inner().sdk_paths().local_state.sqlite_path;
    for record in list_incomplete(sqlite_path)? {
        if record.phase == LocalIdentityDeletionPhase::RetirementReady
            && crate::internal::identity_retirement::is_completed(core, &retirement_input(&record))?
        {
            mark_completed(sqlite_path, &record.deletion_id)?;
        }
    }
    Ok(())
}

pub(crate) fn list_incomplete(
    sqlite_path: &Path,
) -> crate::ImResult<Vec<LocalIdentityDeletionRecord>> {
    let connection = crate::internal::local_state::open_writable(sqlite_path)?;
    let mut statement = connection
        .prepare(
            r#"SELECT deletion_id,schema_version,mode,owner_identity_id,current_did,full_handle,
local_alias,identity_dir_name,next_default_alias,protocol_device_id,phase,created_at,updated_at,
completed_at FROM local_identity_deletions WHERE phase<>'completed' ORDER BY created_at,deletion_id"#,
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let records = statement
        .query_map([], row_to_record)
        .map_err(crate::internal::local_state::local_state_unavailable)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    for record in &records {
        record.validate()?;
    }
    Ok(records)
}

pub(crate) fn pending_full_data_app(
    sqlite_path: &Path,
) -> crate::ImResult<Vec<crate::identity::LocalIdentityDeletionTicket>> {
    list_incomplete(sqlite_path)?
        .into_iter()
        .filter(|record| record.mode == LocalIdentityDeletionMode::FullDataApp)
        .map(|record| ticket(&record))
        .collect()
}

pub(crate) fn ticket(
    record: &LocalIdentityDeletionRecord,
) -> crate::ImResult<crate::identity::LocalIdentityDeletionTicket> {
    Ok(crate::identity::LocalIdentityDeletionTicket {
        deletion_id: record.deletion_id.clone(),
        owner_identity_id: crate::ids::IdentityId::parse(&record.owner_identity_id)?,
        current_did: crate::ids::Did::parse(&record.current_did)?,
    })
}

fn mark_completed(
    sqlite_path: &Path,
    deletion_id: &str,
) -> crate::ImResult<LocalIdentityDeletionRecord> {
    let mut connection = crate::internal::local_state::open_writable(sqlite_path)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let mut record = load_from_connection(&transaction, deletion_id)?
        .ok_or_else(|| deletion_error("identity.local_deletion_conflict"))?;
    if record.phase == LocalIdentityDeletionPhase::Completed {
        transaction
            .commit()
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        return Ok(record);
    }
    if record.phase != LocalIdentityDeletionPhase::RetirementReady {
        return Err(deletion_error("identity.local_deletion_conflict"));
    }
    let now = now_second_z()?;
    let changed = transaction
        .execute(
            "UPDATE local_identity_deletions SET phase='completed',updated_at=?2,completed_at=?2 WHERE deletion_id=?1 AND phase='retirement_ready'",
            rusqlite::params![deletion_id, now],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    if changed != 1 {
        return Err(deletion_error("identity.local_deletion_conflict"));
    }
    transaction
        .commit()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    record.phase = LocalIdentityDeletionPhase::Completed;
    record.updated_at = now.clone();
    record.completed_at = Some(now);
    Ok(record)
}

fn retirement_input(
    record: &LocalIdentityDeletionRecord,
) -> crate::internal::identity_retirement::IdentityRetirementInput {
    crate::internal::identity_retirement::IdentityRetirementInput {
        identity_id: record.owner_identity_id.clone(),
        did: record.current_did.clone(),
        local_alias: record.local_alias.clone(),
        identity_dir_name: record.identity_dir_name.clone(),
        next_default_alias: record.next_default_alias.clone(),
        protocol_device_id: record.protocol_device_id.clone(),
    }
}

fn load_active_owner_from_connection(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
) -> crate::ImResult<Option<LocalIdentityDeletionRecord>> {
    connection
        .query_row(
            r#"SELECT deletion_id,schema_version,mode,owner_identity_id,current_did,full_handle,
local_alias,identity_dir_name,next_default_alias,protocol_device_id,phase,created_at,updated_at,
completed_at FROM local_identity_deletions
WHERE owner_identity_id=?1 AND phase<>'completed'"#,
            [owner_identity_id],
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

fn load_active_handle_from_connection(
    connection: &rusqlite::Connection,
    full_handle: &str,
) -> crate::ImResult<Option<LocalIdentityDeletionRecord>> {
    connection
        .query_row(
            r#"SELECT deletion_id,schema_version,mode,owner_identity_id,current_did,full_handle,
local_alias,identity_dir_name,next_default_alias,protocol_device_id,phase,created_at,updated_at,
completed_at FROM local_identity_deletions
WHERE full_handle=?1 AND phase<>'completed' ORDER BY created_at,deletion_id LIMIT 1"#,
            [full_handle],
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

pub(crate) fn ensure_no_active_deletion(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    full_handle: Option<&str>,
) -> crate::ImResult<()> {
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM local_identity_deletions WHERE phase<>'completed' AND (owner_identity_id=?1 OR (?2 IS NOT NULL AND full_handle=?2))",
            rusqlite::params![owner_identity_id, full_handle],
            |row| row.get(0),
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    if count == 0 {
        Ok(())
    } else {
        Err(deletion_error("identity.local_deletion_conflict"))
    }
}

pub(crate) fn ensure_no_active_deletion_at_path(
    sqlite_path: &Path,
    owner_identity_id: &str,
    full_handle: Option<&str>,
) -> crate::ImResult<()> {
    let connection = crate::internal::local_state::open_writable(sqlite_path)?;
    crate::internal::local_state::schema::ensure_schema(&connection)?;
    ensure_no_active_deletion(&connection, owner_identity_id, full_handle)
}

fn admission_blocker(
    connection: &rusqlite::Connection,
    snapshot: &LocalIdentityDeletionSnapshot,
) -> crate::ImResult<Option<&'static str>> {
    let mut blockers = Vec::new();
    let mut operations = connection
        .prepare(
            "SELECT operation_id,owner_identity_id,lifecycle_class,commit_attempted,key_state,superseded_by_operation_id FROM handle_recovery_operations_v4 WHERE owner_identity_id=?1 OR (?2 IS NOT NULL AND full_handle=?2) ORDER BY operation_id",
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let rows = operations
        .query_map(
            rusqlite::params![
                snapshot.owner_identity_id.as_str(),
                snapshot.full_handle.as_deref(),
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)? != 0,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            },
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    for (
        _operation_id,
        operation_owner_identity_id,
        lifecycle,
        commit_attempted,
        key_state,
        replacement,
    ) in rows
    {
        match lifecycle.as_str() {
            "pre_commit" if !commit_attempted => {
                blockers.push("handle_recovery.precommit_discard_required")
            }
            "pre_commit" | "remote_unresolved" => {
                blockers.push("handle_recovery.operation_must_resume")
            }
            "remote_committed" | "local_transition_pending" => {
                blockers.push("handle_recovery.transition_must_complete")
            }
            "quarantined_key_unavailable" => {
                if key_state != "permanently_unavailable" {
                    blockers.push("identity.local_deletion_conflict");
                }
                if let Some(replacement) = replacement {
                    let replacement_lifecycle = connection
                        .query_row(
                            "SELECT lifecycle_class FROM handle_recovery_operations_v4 WHERE operation_id=?1 AND owner_identity_id=?2",
                            rusqlite::params![replacement, operation_owner_identity_id],
                            |row| row.get::<_, String>(0),
                        )
                        .optional()
                        .map_err(crate::internal::local_state::local_state_unavailable)?;
                    match replacement_lifecycle.as_deref() {
                        Some("pre_commit") | Some("remote_unresolved") => {
                            blockers.push("handle_recovery.operation_must_resume")
                        }
                        Some("remote_committed") | Some("local_transition_pending") => {
                            blockers.push("handle_recovery.transition_must_complete")
                        }
                        Some("applied")
                        | Some("discarded_pre_attempt")
                        | Some("superseded_by_state_change")
                        | Some("failed_terminal")
                        | Some("quarantined_key_unavailable") => {}
                        _ => blockers.push("identity.local_deletion_conflict"),
                    }
                }
            }
            "applied"
            | "discarded_pre_attempt"
            | "superseded_by_state_change"
            | "failed_terminal" => {}
            _ => blockers.push("identity.local_deletion_conflict"),
        }
    }

    let mut transitions = connection
        .prepare(
            "SELECT source_kind,phase FROM identity_transition_pending WHERE (owner_identity_id=?1 OR (?2 IS NOT NULL AND handle=?2)) AND phase IN ('pending','identity_switched') ORDER BY recovery_id",
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let transition_rows = transitions
        .query_map(
            rusqlite::params![
                snapshot.owner_identity_id.as_str(),
                snapshot.full_handle.as_deref(),
            ],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    for (source_kind, phase) in transition_rows {
        blockers.push(if source_kind == "joined_device" && phase == "pending" {
            "handle_recovery.join_must_complete"
        } else {
            "handle_recovery.transition_must_complete"
        });
    }

    let rollover_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM registration_retired_join_rollovers WHERE (owner_identity_id=?1 OR (?2 IS NOT NULL AND handle=?2)) AND phase='prepared'",
            rusqlite::params![
                snapshot.owner_identity_id.as_str(),
                snapshot.full_handle.as_deref(),
            ],
            |row| row.get(0),
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    if rollover_count != 0 {
        blockers.push("handle_recovery.join_must_complete");
    }
    blockers.sort_unstable();
    blockers.dedup();
    Ok(match blockers.as_slice() {
        [] => None,
        [only] => Some(*only),
        _ => Some("identity.local_deletion_conflict"),
    })
}

fn row_to_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<LocalIdentityDeletionRecord> {
    let mode = LocalIdentityDeletionMode::parse(&row.get::<_, String>(2)?)
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    let phase = LocalIdentityDeletionPhase::parse(&row.get::<_, String>(10)?)
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    Ok(LocalIdentityDeletionRecord {
        deletion_id: row.get(0)?,
        schema_version: row.get(1)?,
        mode,
        owner_identity_id: row.get(3)?,
        current_did: row.get(4)?,
        full_handle: row.get(5)?,
        local_alias: row.get(6)?,
        identity_dir_name: row.get(7)?,
        next_default_alias: row.get(8)?,
        protocol_device_id: row.get(9)?,
        phase,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
        completed_at: row.get(13)?,
    })
}

fn deletion_error(code: &'static str) -> crate::ImError {
    crate::ImError::Service {
        status_code: None,
        code: Some(code.to_owned()),
        message: code.to_owned(),
        data: None,
    }
}

fn now_second_z() -> crate::ImResult<String> {
    let now = time::OffsetDateTime::now_utc()
        .replace_nanosecond(0)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    now.format(&time::format_description::well_known::Rfc3339)
        .map_err(|error| crate::ImError::Serialization {
            detail: error.to_string(),
        })
}

#[cfg(test)]
std::thread_local! {
    static FAIL_AFTER_BUSINESS_DELETE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
mod tests;
