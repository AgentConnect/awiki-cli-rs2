//! Non-secret, restart-safe index for Handle Recovery v4 operations.
//!
//! Private keys and grants stay in SecretVault. This SQLite index is the
//! authoritative guard for destructive cleanup: once `commit_attempted` is
//! true, an operation can only be reconciled or moved forward.

use rusqlite::OptionalExtension as _;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub(crate) const HANDLE_RECOVERY_OPERATION_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS handle_recovery_operations_v4 (
    operation_id TEXT PRIMARY KEY,
    owner_identity_id TEXT NOT NULL,
    account_user_id TEXT,
    full_handle TEXT NOT NULL,
    lifecycle_class TEXT NOT NULL CHECK(lifecycle_class IN (
        'pre_commit',
        'remote_unresolved',
        'remote_committed',
        'local_transition_pending',
        'applied',
        'discarded_pre_attempt',
        'quarantined_key_unavailable',
        'superseded_by_state_change',
        'failed_terminal'
    )),
    commit_attempted INTEGER NOT NULL CHECK(commit_attempted IN (0,1)),
    key_state TEXT NOT NULL CHECK(key_state IN (
        'available',
        'temporarily_locked',
        'permanently_unavailable',
        'destroyed_pre_attempt'
    )),
    intent_hash TEXT,
    vault_key_id TEXT NOT NULL,
    state_root_fingerprint TEXT,
    superseded_by_operation_id TEXT,
    last_error_code TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_handle_recovery_operations_owner_lifecycle
ON handle_recovery_operations_v4(owner_identity_id, lifecycle_class, updated_at);

CREATE INDEX IF NOT EXISTS idx_handle_recovery_operations_handle_lifecycle
ON handle_recovery_operations_v4(full_handle, lifecycle_class, updated_at);

CREATE INDEX IF NOT EXISTS idx_handle_recovery_operations_account_lifecycle
ON handle_recovery_operations_v4(account_user_id, lifecycle_class, updated_at);

CREATE INDEX IF NOT EXISTS idx_handle_recovery_operations_superseded_by
ON handle_recovery_operations_v4(superseded_by_operation_id);

CREATE UNIQUE INDEX IF NOT EXISTS idx_handle_recovery_operations_active_owner
ON handle_recovery_operations_v4(owner_identity_id)
WHERE lifecycle_class IN (
    'pre_commit', 'remote_unresolved', 'remote_committed', 'local_transition_pending'
);
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RecoveryLifecycleClass {
    PreCommit,
    RemoteUnresolved,
    RemoteCommitted,
    LocalTransitionPending,
    Applied,
    DiscardedPreAttempt,
    QuarantinedKeyUnavailable,
    SupersededByStateChange,
    FailedTerminal,
}

impl RecoveryLifecycleClass {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::PreCommit => "pre_commit",
            Self::RemoteUnresolved => "remote_unresolved",
            Self::RemoteCommitted => "remote_committed",
            Self::LocalTransitionPending => "local_transition_pending",
            Self::Applied => "applied",
            Self::DiscardedPreAttempt => "discarded_pre_attempt",
            Self::QuarantinedKeyUnavailable => "quarantined_key_unavailable",
            Self::SupersededByStateChange => "superseded_by_state_change",
            Self::FailedTerminal => "failed_terminal",
        }
    }

    fn parse(value: &str) -> rusqlite::Result<Self> {
        match value {
            "pre_commit" => Ok(Self::PreCommit),
            "remote_unresolved" => Ok(Self::RemoteUnresolved),
            "remote_committed" => Ok(Self::RemoteCommitted),
            "local_transition_pending" => Ok(Self::LocalTransitionPending),
            "applied" => Ok(Self::Applied),
            "discarded_pre_attempt" => Ok(Self::DiscardedPreAttempt),
            "quarantined_key_unavailable" => Ok(Self::QuarantinedKeyUnavailable),
            "superseded_by_state_change" => Ok(Self::SupersededByStateChange),
            "failed_terminal" => Ok(Self::FailedTerminal),
            _ => Err(rusqlite::Error::InvalidQuery),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RecoveryKeyState {
    Available,
    TemporarilyLocked,
    PermanentlyUnavailable,
    DestroyedPreAttempt,
}

impl RecoveryKeyState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::TemporarilyLocked => "temporarily_locked",
            Self::PermanentlyUnavailable => "permanently_unavailable",
            Self::DestroyedPreAttempt => "destroyed_pre_attempt",
        }
    }

    fn parse(value: &str) -> rusqlite::Result<Self> {
        match value {
            "available" => Ok(Self::Available),
            "temporarily_locked" => Ok(Self::TemporarilyLocked),
            "permanently_unavailable" => Ok(Self::PermanentlyUnavailable),
            "destroyed_pre_attempt" => Ok(Self::DestroyedPreAttempt),
            _ => Err(rusqlite::Error::InvalidQuery),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RecoveryOperationRecord {
    pub(crate) operation_id: String,
    pub(crate) owner_identity_id: String,
    pub(crate) account_user_id: Option<String>,
    pub(crate) full_handle: String,
    pub(crate) lifecycle_class: RecoveryLifecycleClass,
    pub(crate) commit_attempted: bool,
    pub(crate) key_state: RecoveryKeyState,
    pub(crate) intent_hash: Option<String>,
    pub(crate) vault_key_id: String,
    pub(crate) state_root_fingerprint: Option<String>,
    pub(crate) superseded_by_operation_id: Option<String>,
    pub(crate) last_error_code: Option<String>,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}

impl RecoveryOperationRecord {
    pub(crate) fn pre_commit(
        operation_id: String,
        owner_identity_id: String,
        full_handle: String,
        vault_key_id: String,
        now: String,
    ) -> crate::ImResult<Self> {
        crate::internal::identity_wire::handle_recovery::validate_operation_id(&operation_id)?;
        crate::internal::identity_wire::handle_recovery::canonical_handle(&full_handle)?;
        if owner_identity_id.trim().is_empty() || vault_key_id.trim().is_empty() {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(Self {
            operation_id,
            owner_identity_id,
            account_user_id: None,
            full_handle,
            lifecycle_class: RecoveryLifecycleClass::PreCommit,
            commit_attempted: false,
            key_state: RecoveryKeyState::Available,
            intent_hash: None,
            vault_key_id,
            state_root_fingerprint: None,
            superseded_by_operation_id: None,
            last_error_code: None,
            created_at: now.clone(),
            updated_at: now,
        })
    }
}

pub(crate) fn insert(sqlite_path: &Path, record: &RecoveryOperationRecord) -> crate::ImResult<()> {
    let connection = crate::internal::local_state::open_writable(sqlite_path)?;
    connection
        .execute(
            r#"INSERT INTO handle_recovery_operations_v4
(operation_id,owner_identity_id,account_user_id,full_handle,lifecycle_class,commit_attempted,
 key_state,intent_hash,vault_key_id,state_root_fingerprint,superseded_by_operation_id,
 last_error_code,created_at,updated_at)
VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)"#,
            rusqlite::params![
                record.operation_id,
                record.owner_identity_id,
                record.account_user_id,
                record.full_handle,
                record.lifecycle_class.as_str(),
                i64::from(record.commit_attempted),
                record.key_state.as_str(),
                record.intent_hash,
                record.vault_key_id,
                record.state_root_fingerprint,
                record.superseded_by_operation_id,
                record.last_error_code,
                record.created_at,
                record.updated_at,
            ],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    Ok(())
}

pub(crate) fn load(
    sqlite_path: &Path,
    operation_id: &str,
) -> crate::ImResult<Option<RecoveryOperationRecord>> {
    let connection = crate::internal::local_state::open_writable(sqlite_path)?;
    connection
        .query_row(
            r#"SELECT operation_id,owner_identity_id,account_user_id,full_handle,lifecycle_class,
commit_attempted,key_state,intent_hash,vault_key_id,state_root_fingerprint,
superseded_by_operation_id,last_error_code,created_at,updated_at
FROM handle_recovery_operations_v4 WHERE operation_id=?1"#,
            [operation_id],
            row_to_record,
        )
        .optional()
        .map_err(crate::internal::local_state::local_state_unavailable)
}

pub(crate) fn list_owner(
    sqlite_path: &Path,
    owner_identity_id: &str,
) -> crate::ImResult<Vec<RecoveryOperationRecord>> {
    let connection = crate::internal::local_state::open_writable(sqlite_path)?;
    let mut statement = connection
        .prepare(
            r#"SELECT operation_id,owner_identity_id,account_user_id,full_handle,lifecycle_class,
commit_attempted,key_state,intent_hash,vault_key_id,state_root_fingerprint,
superseded_by_operation_id,last_error_code,created_at,updated_at
FROM handle_recovery_operations_v4 WHERE owner_identity_id=?1 ORDER BY updated_at DESC,operation_id DESC"#,
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let rows = statement
        .query_map([owner_identity_id], row_to_record)
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(crate::internal::local_state::local_state_unavailable)
}

pub(crate) fn mark_commit_attempted(
    sqlite_path: &Path,
    operation_id: &str,
    now: &str,
) -> crate::ImResult<()> {
    let connection = crate::internal::local_state::open_writable(sqlite_path)?;
    let changed = connection
        .execute(
            "UPDATE handle_recovery_operations_v4 SET commit_attempted=1,lifecycle_class='remote_unresolved',updated_at=?2 WHERE operation_id=?1 AND lifecycle_class IN ('pre_commit','remote_unresolved')",
            rusqlite::params![operation_id, now],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    if changed == 0 {
        let record = load(sqlite_path, operation_id)?.ok_or(crate::ImError::PermissionDenied)?;
        if !record.commit_attempted {
            return Err(crate::ImError::PermissionDenied);
        }
    }
    Ok(())
}

pub(crate) fn record_frozen_intent(
    sqlite_path: &Path,
    operation_id: &str,
    account_user_id: &str,
    intent_hash: &str,
    now: &str,
) -> crate::ImResult<()> {
    if account_user_id.trim().is_empty() || !valid_sha256_digest(intent_hash) {
        return Err(crate::ImError::PermissionDenied);
    }
    let connection = crate::internal::local_state::open_writable(sqlite_path)?;
    let changed = connection
        .execute(
            "UPDATE handle_recovery_operations_v4 SET account_user_id=?2,intent_hash=?3,updated_at=?4 WHERE operation_id=?1 AND lifecycle_class IN ('pre_commit','remote_unresolved') AND (account_user_id IS NULL OR account_user_id=?2) AND (intent_hash IS NULL OR intent_hash=?3)",
            rusqlite::params![operation_id, account_user_id, intent_hash, now],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    if changed != 1 {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

pub(crate) fn update_lifecycle(
    sqlite_path: &Path,
    operation_id: &str,
    expected: RecoveryLifecycleClass,
    next: RecoveryLifecycleClass,
    state_root_fingerprint: Option<&str>,
    last_error_code: Option<&str>,
    now: &str,
) -> crate::ImResult<()> {
    let connection = crate::internal::local_state::open_writable(sqlite_path)?;
    let changed = connection
        .execute(
            "UPDATE handle_recovery_operations_v4 SET lifecycle_class=?2,state_root_fingerprint=COALESCE(?3,state_root_fingerprint),last_error_code=?4,updated_at=?5 WHERE operation_id=?1 AND lifecycle_class=?6",
            rusqlite::params![
                operation_id,
                next.as_str(),
                state_root_fingerprint,
                last_error_code,
                now,
                expected.as_str(),
            ],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    if changed != 1 {
        let current = load(sqlite_path, operation_id)?.ok_or(crate::ImError::PermissionDenied)?;
        if current.lifecycle_class != next {
            return Err(crate::ImError::PermissionDenied);
        }
    }
    Ok(())
}

pub(crate) fn record_nonterminal_error(
    sqlite_path: &Path,
    operation_id: &str,
    last_error_code: Option<&str>,
    now: &str,
) -> crate::ImResult<()> {
    let connection = crate::internal::local_state::open_writable(sqlite_path)?;
    let changed = connection
        .execute(
            "UPDATE handle_recovery_operations_v4 SET last_error_code=?2,updated_at=?3 WHERE operation_id=?1 AND lifecycle_class IN ('pre_commit','remote_unresolved')",
            rusqlite::params![operation_id, last_error_code, now],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    if changed != 1 {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

pub(crate) fn discard_pre_attempt(
    sqlite_path: &Path,
    operation_id: &str,
    now: &str,
) -> crate::ImResult<()> {
    let connection = crate::internal::local_state::open_writable(sqlite_path)?;
    let changed = connection
        .execute(
            "UPDATE handle_recovery_operations_v4 SET lifecycle_class='discarded_pre_attempt',key_state='destroyed_pre_attempt',updated_at=?2 WHERE operation_id=?1 AND lifecycle_class='pre_commit' AND commit_attempted=0",
            rusqlite::params![operation_id, now],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    if changed != 1 {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

pub(crate) fn quarantine_key_unavailable(
    sqlite_path: &Path,
    operation_id: &str,
    now: &str,
) -> crate::ImResult<()> {
    let connection = crate::internal::local_state::open_writable(sqlite_path)?;
    let changed = connection
        .execute(
            "UPDATE handle_recovery_operations_v4 SET lifecycle_class='quarantined_key_unavailable',key_state='permanently_unavailable',last_error_code='handle_recovery_key_unavailable',updated_at=?2 WHERE operation_id=?1 AND lifecycle_class<>'applied'",
            rusqlite::params![operation_id, now],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    if changed != 1 {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

fn row_to_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<RecoveryOperationRecord> {
    Ok(RecoveryOperationRecord {
        operation_id: row.get(0)?,
        owner_identity_id: row.get(1)?,
        account_user_id: row.get(2)?,
        full_handle: row.get(3)?,
        lifecycle_class: RecoveryLifecycleClass::parse(&row.get::<_, String>(4)?)?,
        commit_attempted: row.get::<_, i64>(5)? != 0,
        key_state: RecoveryKeyState::parse(&row.get::<_, String>(6)?)?,
        intent_hash: row.get(7)?,
        vault_key_id: row.get(8)?,
        state_root_fingerprint: row.get(9)?,
        superseded_by_operation_id: row.get(10)?,
        last_error_code: row.get(11)?,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
    })
}

fn valid_sha256_digest(value: &str) -> bool {
    use base64::Engine as _;
    value
        .strip_prefix("sha256:")
        .and_then(|encoded| {
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(encoded)
                .ok()
        })
        .is_some_and(|bytes| bytes.len() == 32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(operation_id: &str, owner: &str) -> RecoveryOperationRecord {
        RecoveryOperationRecord::pre_commit(
            operation_id.to_owned(),
            owner.to_owned(),
            "alice.example.invalid".to_owned(),
            format!("vault-{operation_id}"),
            "2026-08-07T00:00:00Z".to_owned(),
        )
        .unwrap()
    }

    #[test]
    fn one_actionable_operation_per_owner_but_history_is_retained() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("im.sqlite");
        insert(&path, &record("op_alpha_12345678", "owner-1")).unwrap();
        assert!(insert(&path, &record("op_bravo_12345678", "owner-1")).is_err());

        discard_pre_attempt(&path, "op_alpha_12345678", "2026-08-07T00:01:00Z").unwrap();
        insert(&path, &record("op_bravo_12345678", "owner-1")).unwrap();
        assert_eq!(list_owner(&path, "owner-1").unwrap().len(), 2);
    }

    #[test]
    fn commit_attempt_is_monotonic_and_prevents_discard() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("im.sqlite");
        insert(&path, &record("op_commit_12345678", "owner-1")).unwrap();
        mark_commit_attempted(&path, "op_commit_12345678", "2026-08-07T00:01:00Z").unwrap();
        mark_commit_attempted(&path, "op_commit_12345678", "2026-08-07T00:02:00Z").unwrap();
        assert!(discard_pre_attempt(&path, "op_commit_12345678", "2026-08-07T00:03:00Z").is_err());
        let stored = load(&path, "op_commit_12345678").unwrap().unwrap();
        assert!(stored.commit_attempted);
        assert_eq!(
            stored.lifecycle_class,
            RecoveryLifecycleClass::RemoteUnresolved
        );
    }

    #[test]
    fn frozen_intent_hash_accepts_one_prefix_and_rejects_double_prefix() {
        let standard = "sha256:SlQnFpLKCK0OFEKnA2492wGZ8WsD_w35-l_wTccWbUA";
        assert!(valid_sha256_digest(standard));
        assert!(!valid_sha256_digest(&format!("sha256:{standard}")));
    }
}
