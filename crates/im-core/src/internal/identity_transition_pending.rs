use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::path::Path;

pub(crate) const IDENTITY_TRANSITION_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS identity_transition_pending (
    recovery_id TEXT PRIMARY KEY,
    schema_version INTEGER NOT NULL,
    contract_version TEXT NOT NULL,
    contract_hash TEXT NOT NULL,
    source_kind TEXT NOT NULL CHECK(source_kind IN ('initiator','joined_device')),
    source_id TEXT NOT NULL,
    state_root_fingerprint TEXT NOT NULL,
    account_user_id TEXT NOT NULL,
    owner_identity_id TEXT NOT NULL,
    handle TEXT NOT NULL,
    previous_did TEXT NOT NULL,
    current_did TEXT NOT NULL,
    binding_generation TEXT NOT NULL,
    phase TEXT NOT NULL CHECK(phase IN ('pending','identity_switched','completed')),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_identity_transition_source
ON identity_transition_pending(source_kind, source_id);
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TransitionSourceKind {
    Initiator,
    JoinedDevice,
}

impl TransitionSourceKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Initiator => "initiator",
            Self::JoinedDevice => "joined_device",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TransitionPhase {
    Pending,
    IdentitySwitched,
    Completed,
}

impl TransitionPhase {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::IdentitySwitched => "identity_switched",
            Self::Completed => "completed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct IdentityTransitionMarker {
    pub(crate) schema_version: u32,
    pub(crate) contract_version: String,
    pub(crate) contract_hash: String,
    pub(crate) recovery_id: String,
    pub(crate) source_kind: TransitionSourceKind,
    pub(crate) source_id: String,
    pub(crate) state_root_fingerprint: String,
    pub(crate) account_user_id: String,
    pub(crate) owner_identity_id: String,
    pub(crate) handle: String,
    pub(crate) previous_did: String,
    pub(crate) current_did: String,
    pub(crate) binding_generation: String,
    pub(crate) phase: TransitionPhase,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}

impl IdentityTransitionMarker {
    pub(crate) fn initiator(
        sqlite_path: &Path,
        pending: &crate::internal::identity_handle_recovery_pending::PendingHandleRecovery,
        result: &crate::internal::identity_handle_recovery_pending::RecoveryRemoteResult,
    ) -> crate::ImResult<Self> {
        let now = now()?;
        let marker = Self {
            schema_version: 1,
            contract_version: crate::internal::identity_handle_recovery_pending::CONTRACT_VERSION
                .to_owned(),
            contract_hash: crate::internal::identity_handle_recovery_pending::CONTRACT_HASH
                .to_owned(),
            recovery_id: pending.recovery_id.clone(),
            source_kind: TransitionSourceKind::Initiator,
            source_id: pending.operation_id.clone(),
            state_root_fingerprint: state_root_fingerprint(sqlite_path),
            account_user_id: result.account_user_id.clone(),
            owner_identity_id: pending.owner_identity_id.clone(),
            handle: result.handle.clone(),
            previous_did: result.previous_did.clone(),
            current_did: result.did.clone(),
            binding_generation: result.binding_generation.clone(),
            phase: TransitionPhase::Pending,
            created_at: now.clone(),
            updated_at: now,
        };
        marker.validate()?;
        Ok(marker)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn joined_device(
        sqlite_path: &Path,
        join_session_id: &str,
        account_user_id: &str,
        owner_identity_id: &str,
        handle: &str,
        previous_did: &str,
        current_did: &str,
        binding_generation: &str,
    ) -> crate::ImResult<Self> {
        let now = now()?;
        let mut digest = Sha256::new();
        digest.update(b"awiki.identity.handle-recovery.joined-marker.v1\0");
        digest.update(join_session_id.as_bytes());
        let marker = Self {
            schema_version: 1,
            contract_version: crate::internal::identity_handle_recovery_pending::CONTRACT_VERSION
                .to_owned(),
            contract_hash: crate::internal::identity_handle_recovery_pending::CONTRACT_HASH
                .to_owned(),
            recovery_id: format!("joined_{:x}", digest.finalize()),
            source_kind: TransitionSourceKind::JoinedDevice,
            source_id: join_session_id.to_owned(),
            state_root_fingerprint: state_root_fingerprint(sqlite_path),
            account_user_id: account_user_id.to_owned(),
            owner_identity_id: owner_identity_id.to_owned(),
            handle: handle.to_owned(),
            previous_did: previous_did.to_owned(),
            current_did: current_did.to_owned(),
            binding_generation: binding_generation.to_owned(),
            phase: TransitionPhase::Pending,
            created_at: now.clone(),
            updated_at: now,
        };
        marker.validate()?;
        Ok(marker)
    }

    pub(crate) fn validate(&self) -> crate::ImResult<()> {
        if self.schema_version != 1
            || self.contract_version
                != crate::internal::identity_handle_recovery_pending::CONTRACT_VERSION
            || self.contract_hash
                != crate::internal::identity_handle_recovery_pending::CONTRACT_HASH
            || self.recovery_id.trim().is_empty()
            || self.source_id.trim().is_empty()
            || self.account_user_id.trim().is_empty()
            || self.owner_identity_id.trim().is_empty()
            || self.previous_did == self.current_did
            || self.state_root_fingerprint.len() != 71
            || !self.state_root_fingerprint.starts_with("sha256:")
            || crate::internal::identity_wire::handle_recovery::canonical_handle(&self.handle)
                .is_err()
        {
            return Err(crate::ImError::PermissionDenied);
        }
        match self.source_kind {
            TransitionSourceKind::Initiator
                if self.source_id
                    != crate::internal::identity_wire::handle_recovery::validate_operation_id(
                        &self.source_id,
                    )? => {}
            TransitionSourceKind::JoinedDevice if self.source_id.trim().is_empty() => {
                return Err(crate::ImError::PermissionDenied)
            }
            _ => {}
        }
        Ok(())
    }
}

pub(crate) fn load_joined_device(
    sqlite_path: &Path,
    join_session_id: &str,
) -> crate::ImResult<Option<IdentityTransitionMarker>> {
    let connection = crate::internal::local_state::open_writable(sqlite_path)?;
    crate::internal::local_state::schema::ensure_schema(&connection)?;
    connection
        .query_row(
            "SELECT schema_version,contract_version,contract_hash,recovery_id,state_root_fingerprint,account_user_id,owner_identity_id,handle,previous_did,current_did,binding_generation,phase,created_at,updated_at FROM identity_transition_pending WHERE source_kind='joined_device' AND source_id=?1",
            [join_session_id],
            |row| {
                let phase = match row.get::<_, String>(11)?.as_str() {
                    "pending" => TransitionPhase::Pending,
                    "identity_switched" => TransitionPhase::IdentitySwitched,
                    "completed" => TransitionPhase::Completed,
                    _ => return Err(rusqlite::Error::InvalidQuery),
                };
                Ok(IdentityTransitionMarker {
                    schema_version: row.get(0)?,
                    contract_version: row.get(1)?,
                    contract_hash: row.get(2)?,
                    recovery_id: row.get(3)?,
                    source_kind: TransitionSourceKind::JoinedDevice,
                    source_id: join_session_id.to_owned(),
                    state_root_fingerprint: row.get(4)?,
                    account_user_id: row.get(5)?,
                    owner_identity_id: row.get(6)?,
                    handle: row.get(7)?,
                    previous_did: row.get(8)?,
                    current_did: row.get(9)?,
                    binding_generation: row.get(10)?,
                    phase,
                    created_at: row.get(12)?,
                    updated_at: row.get(13)?,
                })
            },
        )
        .optional()
        .map_err(crate::internal::local_state::local_state_unavailable)?
        .map(|marker| {
            marker.validate()?;
            marker.validate_state_root(sqlite_path)?;
            Ok(marker)
        })
        .transpose()
}

pub(crate) fn load(
    sqlite_path: &Path,
    recovery_id: &str,
) -> crate::ImResult<Option<IdentityTransitionMarker>> {
    let connection = crate::internal::local_state::open_writable(sqlite_path)?;
    crate::internal::local_state::schema::ensure_schema(&connection)?;
    connection
        .query_row(
            "SELECT schema_version,contract_version,contract_hash,source_kind,source_id,state_root_fingerprint,account_user_id,owner_identity_id,handle,previous_did,current_did,binding_generation,phase,created_at,updated_at FROM identity_transition_pending WHERE recovery_id=?1",
            [recovery_id],
            |row| {
                let source_kind = match row.get::<_, String>(3)?.as_str() {
                    "initiator" => TransitionSourceKind::Initiator,
                    "joined_device" => TransitionSourceKind::JoinedDevice,
                    _ => return Err(rusqlite::Error::InvalidQuery),
                };
                let phase = match row.get::<_, String>(12)?.as_str() {
                    "pending" => TransitionPhase::Pending,
                    "identity_switched" => TransitionPhase::IdentitySwitched,
                    "completed" => TransitionPhase::Completed,
                    _ => return Err(rusqlite::Error::InvalidQuery),
                };
                Ok(IdentityTransitionMarker {
                    schema_version: row.get(0)?,
                    contract_version: row.get(1)?,
                    contract_hash: row.get(2)?,
                    recovery_id: recovery_id.to_owned(),
                    source_kind,
                    source_id: row.get(4)?,
                    state_root_fingerprint: row.get(5)?,
                    account_user_id: row.get(6)?,
                    owner_identity_id: row.get(7)?,
                    handle: row.get(8)?,
                    previous_did: row.get(9)?,
                    current_did: row.get(10)?,
                    binding_generation: row.get(11)?,
                    phase,
                    created_at: row.get(13)?,
                    updated_at: row.get(14)?,
                })
            },
        )
        .optional()
        .map_err(crate::internal::local_state::local_state_unavailable)?
        .map(|marker| {
            marker.validate()?;
            marker.validate_state_root(sqlite_path)?;
            Ok(marker)
        })
        .transpose()
}

pub(crate) fn has_transition_for_owner(
    sqlite_path: &Path,
    owner_identity_id: &str,
) -> crate::ImResult<bool> {
    let connection = crate::internal::local_state::open_writable(sqlite_path)?;
    crate::internal::local_state::schema::ensure_schema(&connection)?;
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM identity_transition_pending WHERE owner_identity_id=?1 AND state_root_fingerprint=?2",
            rusqlite::params![owner_identity_id, state_root_fingerprint(sqlite_path)],
            |row| row.get(0),
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    Ok(count != 0)
}

pub(crate) fn persist(
    sqlite_path: &Path,
    marker: &IdentityTransitionMarker,
) -> crate::ImResult<()> {
    marker.validate()?;
    marker.validate_state_root(sqlite_path)?;
    let connection = crate::internal::local_state::open_writable(sqlite_path)?;
    crate::internal::local_state::schema::ensure_schema(&connection)?;
    let other_transition: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM identity_transition_pending WHERE owner_identity_id=?1 AND recovery_id<>?2",
            rusqlite::params![marker.owner_identity_id, marker.recovery_id],
            |row| row.get(0),
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    if other_transition != 0 {
        return Err(crate::ImError::Service {
            status_code: None,
            code: Some("handle_recovery_transition_chain_unsupported".to_owned()),
            message: "handle_recovery_transition_chain_unsupported".to_owned(),
            data: None,
        });
    }
    connection
        .execute(
            r#"
INSERT INTO identity_transition_pending
    (recovery_id,schema_version,contract_version,contract_hash,source_kind,source_id,
     state_root_fingerprint,account_user_id,owner_identity_id,handle,previous_did,current_did,
     binding_generation,phase,created_at,updated_at)
VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)
ON CONFLICT(recovery_id) DO UPDATE SET updated_at=excluded.updated_at
WHERE identity_transition_pending.schema_version=excluded.schema_version
  AND identity_transition_pending.contract_version=excluded.contract_version
  AND identity_transition_pending.contract_hash=excluded.contract_hash
  AND identity_transition_pending.source_kind=excluded.source_kind
  AND identity_transition_pending.source_id=excluded.source_id
  AND identity_transition_pending.state_root_fingerprint=excluded.state_root_fingerprint
  AND identity_transition_pending.account_user_id=excluded.account_user_id
  AND identity_transition_pending.owner_identity_id=excluded.owner_identity_id
  AND identity_transition_pending.handle=excluded.handle
  AND identity_transition_pending.previous_did=excluded.previous_did
  AND identity_transition_pending.current_did=excluded.current_did
  AND identity_transition_pending.binding_generation=excluded.binding_generation"#,
            rusqlite::params![
                marker.recovery_id,
                marker.schema_version,
                marker.contract_version,
                marker.contract_hash,
                marker.source_kind.as_str(),
                marker.source_id,
                marker.state_root_fingerprint,
                marker.account_user_id,
                marker.owner_identity_id,
                marker.handle,
                marker.previous_did,
                marker.current_did,
                marker.binding_generation,
                marker.phase.as_str(),
                marker.created_at,
                marker.updated_at,
            ],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM identity_transition_pending WHERE recovery_id=?1 AND source_kind=?2 AND source_id=?3 AND owner_identity_id=?4 AND previous_did=?5 AND current_did=?6 AND binding_generation=?7",
            rusqlite::params![
                marker.recovery_id,
                marker.source_kind.as_str(),
                marker.source_id,
                marker.owner_identity_id,
                marker.previous_did,
                marker.current_did,
                marker.binding_generation,
            ],
            |row| row.get(0),
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    if count != 1 {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

pub(crate) fn update_phase(
    sqlite_path: &Path,
    recovery_id: &str,
    expected: TransitionPhase,
    next: TransitionPhase,
) -> crate::ImResult<()> {
    let connection = crate::internal::local_state::open_writable(sqlite_path)?;
    crate::internal::local_state::schema::ensure_schema(&connection)?;
    let changed = connection
        .execute(
            "UPDATE identity_transition_pending SET phase=?1,updated_at=?2 WHERE recovery_id=?3 AND phase=?4 AND state_root_fingerprint=?5",
            rusqlite::params![next.as_str(), now()?, recovery_id, expected.as_str(), state_root_fingerprint(sqlite_path)],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    if changed == 0 {
        let current: Option<(String, String)> = connection
            .query_row(
                "SELECT phase,state_root_fingerprint FROM identity_transition_pending WHERE recovery_id=?1",
                [recovery_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        if current
            .as_ref()
            .map(|value| (value.0.as_str(), value.1.as_str()))
            != Some((next.as_str(), state_root_fingerprint(sqlite_path).as_str()))
        {
            return Err(crate::ImError::PermissionDenied);
        }
    }
    Ok(())
}

/// Applies the recovery-authorized state epoch reset while preserving business
/// history. The marker tuple is rechecked inside the same transaction before
/// any credential-scoped state is retired.
pub(crate) fn migrate_local_state(
    sqlite_path: &Path,
    marker: &IdentityTransitionMarker,
    bootstrap_device_id: &str,
    auth_generation: u64,
) -> crate::ImResult<()> {
    migrate_local_state_inner(
        sqlite_path,
        marker,
        bootstrap_device_id,
        auth_generation,
        false,
    )
}

pub(crate) fn migrate_joined_device_local_state_without_historical_binding(
    sqlite_path: &Path,
    marker: &IdentityTransitionMarker,
    bootstrap_device_id: &str,
    auth_generation: u64,
) -> crate::ImResult<()> {
    if marker.source_kind != TransitionSourceKind::JoinedDevice {
        return Err(crate::ImError::PermissionDenied);
    }
    migrate_local_state_inner(
        sqlite_path,
        marker,
        bootstrap_device_id,
        auth_generation,
        true,
    )
}

pub(crate) fn migrate_initiator_without_local_identity(
    sqlite_path: &Path,
    marker: &IdentityTransitionMarker,
    bootstrap_device_id: &str,
    auth_generation: u64,
) -> crate::ImResult<()> {
    if marker.source_kind != TransitionSourceKind::Initiator {
        return Err(crate::ImError::PermissionDenied);
    }
    migrate_local_state_inner(
        sqlite_path,
        marker,
        bootstrap_device_id,
        auth_generation,
        true,
    )
}

fn migrate_local_state_inner(
    sqlite_path: &Path,
    marker: &IdentityTransitionMarker,
    bootstrap_device_id: &str,
    auth_generation: u64,
    allow_missing_previous_binding: bool,
) -> crate::ImResult<()> {
    marker.validate()?;
    marker.validate_state_root(sqlite_path)?;
    if marker.phase != TransitionPhase::Pending
        || bootstrap_device_id.trim().is_empty()
        || auth_generation == 0
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let mut connection = crate::internal::local_state::open_writable(sqlite_path)?;
    crate::internal::local_state::schema::ensure_schema(&connection)?;
    let transaction = connection
        .transaction()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let authorized: i64 = transaction
        .query_row(
            "SELECT COUNT(*) FROM identity_transition_pending WHERE recovery_id=?1 AND source_kind=?2 AND source_id=?3 AND state_root_fingerprint=?4 AND account_user_id=?5 AND owner_identity_id=?6 AND handle=?7 AND previous_did=?8 AND current_did=?9 AND binding_generation=?10 AND phase='pending'",
            rusqlite::params![
                marker.recovery_id,
                marker.source_kind.as_str(),
                marker.source_id,
                marker.state_root_fingerprint,
                marker.account_user_id,
                marker.owner_identity_id,
                marker.handle,
                marker.previous_did,
                marker.current_did,
                marker.binding_generation,
            ],
            |row| row.get(0),
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    if authorized != 1 {
        return Err(crate::ImError::PermissionDenied);
    }

    let binding: Option<(String, Option<String>, String, String)> = transaction
        .query_row(
            "SELECT account_id,handle_scope,current_did,identity_generation FROM identity_account_bindings WHERE owner_identity_id=?1",
            [&marker.owner_identity_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    if binding.as_ref()
        == Some(&(
            marker.account_user_id.clone(),
            Some(marker.handle.clone()),
            marker.current_did.clone(),
            marker.binding_generation.clone(),
        ))
    {
        return Ok(());
    }
    let previous_generation = marker
        .binding_generation
        .parse::<u128>()
        .ok()
        .and_then(|value| value.checked_sub(1))
        .filter(|value| *value > 0)
        .map(|value| value.to_string())
        .ok_or(crate::ImError::PermissionDenied)?;
    let expected_previous_binding = (
        marker.account_user_id.clone(),
        Some(marker.handle.clone()),
        marker.previous_did.clone(),
        previous_generation,
    );
    if binding.as_ref() != Some(&expected_previous_binding)
        && !(allow_missing_previous_binding && binding.is_none())
    {
        return Err(crate::ImError::PermissionDenied);
    }

    // Cascades clear Account State cursor/recovery/read/outbox projections.
    transaction
        .execute(
            "DELETE FROM identity_account_bindings WHERE owner_identity_id=?1",
            [&marker.owner_identity_id],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let now_unix = time::OffsetDateTime::now_utc().unix_timestamp();
    transaction
        .execute(
            r#"INSERT INTO identity_account_bindings
                (owner_identity_id,account_id,handle_scope,current_did,device_id,
                 identity_generation,device_auth_generation,created_at,updated_at)
               VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?8)"#,
            rusqlite::params![
                marker.owner_identity_id,
                marker.account_user_id,
                marker.handle,
                marker.current_did,
                bootstrap_device_id,
                marker.binding_generation,
                auth_generation.to_string(),
                now_unix,
            ],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;

    // Subject checkpoints are epoch-scoped. Lazy sync restarts the new DID at
    // zero only after the reset receipt/marker is durable.
    transaction
        .execute(
            "DELETE FROM sync_state WHERE owner_identity_id=?1",
            [&marker.owner_identity_id],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;

    for table in [
        "direct_e2ee_sessions",
        "direct_e2ee_signed_prekeys",
        "direct_e2ee_one_time_prekeys",
        "e2ee_outbox",
        "direct_e2ee_v2_owner_scopes",
        "direct_e2ee_v2_attachment_intents",
        "direct_e2ee_v2_delivery_ledger",
        "direct_e2ee_v2_one_time_prekeys",
        "direct_e2ee_v2_pending",
        "direct_e2ee_v2_prekey_bundles",
        "direct_e2ee_v2_replay",
        "direct_e2ee_v2_session_reply_ledger",
        "direct_e2ee_v2_sessions",
        "group_rebind_outbox",
        "group_rebind_p6_jobs",
        "identity_root_import_completion_v1",
        "identity_root_transfer_sender_v1",
        "system_notification_join_state",
    ] {
        delete_owner_rows(&transaction, table, &marker.owner_identity_id)?;
    }
    delete_rows_by_column(
        &transaction,
        "e2ee_sessions",
        "owner_did",
        &marker.previous_did,
    )?;

    let now = now()?;
    transaction
        .execute(
            "UPDATE identity_did_history SET status='previous',last_seen_at=?1 WHERE owner_identity_id=?2 AND did<>?3",
            rusqlite::params![now, marker.owner_identity_id, marker.current_did],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    transaction
        .execute(
            r#"INSERT INTO identity_did_history
                (owner_identity_id,did,status,first_seen_at,last_seen_at,metadata)
               VALUES (?1,?2,'current',?3,?3,?4)
               ON CONFLICT(owner_identity_id,did) DO UPDATE SET
                 status='current',last_seen_at=excluded.last_seen_at,metadata=excluded.metadata"#,
            rusqlite::params![
                marker.owner_identity_id,
                marker.current_did,
                now,
                serde_json::json!({
                    "protocol": "manifest_handle_recovery_v1",
                    "operation_id": marker.source_id,
                    "binding_generation": marker.binding_generation,
                })
                .to_string(),
            ],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    transaction
        .commit()
        .map_err(crate::internal::local_state::local_state_unavailable)
}

fn delete_owner_rows(
    transaction: &rusqlite::Transaction<'_>,
    table: &str,
    owner_identity_id: &str,
) -> crate::ImResult<()> {
    let exists: i64 = transaction
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
            [table],
            |row| row.get(0),
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    if exists == 0 {
        return Ok(());
    }
    let has_owner = transaction
        .prepare(&format!("PRAGMA table_info({table})"))
        .and_then(|mut statement| {
            let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
            rows.collect::<Result<Vec<_>, _>>()
        })
        .map_err(crate::internal::local_state::local_state_unavailable)?
        .into_iter()
        .any(|column| column == "owner_identity_id");
    if has_owner {
        transaction
            .execute(
                &format!("DELETE FROM {table} WHERE owner_identity_id=?1"),
                [owner_identity_id],
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
    }
    Ok(())
}

fn delete_rows_by_column(
    transaction: &rusqlite::Transaction<'_>,
    table: &str,
    column: &str,
    value: &str,
) -> crate::ImResult<()> {
    let exists: i64 = transaction
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
            [table],
            |row| row.get(0),
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    if exists != 0 {
        transaction
            .execute(&format!("DELETE FROM {table} WHERE {column}=?1"), [value])
            .map_err(crate::internal::local_state::local_state_unavailable)?;
    }
    Ok(())
}

pub(crate) fn state_root_fingerprint(path: &Path) -> String {
    let mut digest = Sha256::new();
    digest.update(b"awiki.identity.handle-recovery.state-root.v1\0");
    digest.update(path.as_os_str().as_encoded_bytes());
    format!("sha256:{:x}", digest.finalize())
}

impl IdentityTransitionMarker {
    fn validate_state_root(&self, sqlite_path: &Path) -> crate::ImResult<()> {
        if self.state_root_fingerprint != state_root_fingerprint(sqlite_path) {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(())
    }
}

#[derive(Debug)]
pub(crate) struct LegacyAuthorityInput<'a> {
    pub(crate) owner_identity_id: &'a str,
    pub(crate) account_user_id: &'a str,
    pub(crate) current_did: &'a str,
    pub(crate) binding_generation: &'a str,
    pub(crate) protocol_device_id: &'a str,
    pub(crate) device_auth_generation: u64,
    pub(crate) document_version: u64,
    pub(crate) document_hash: &'a str,
    pub(crate) registry_version: u64,
}

pub(crate) fn legacy_registry_epoch_adoption_authority(
    sqlite_path: &Path,
    input: LegacyAuthorityInput<'_>,
) -> crate::ImResult<Option<crate::identity::LegacyRegistryEpochAdoptionAuthority>> {
    if input.owner_identity_id.trim().is_empty()
        || input.account_user_id.trim().is_empty()
        || input.device_auth_generation == 0
        || input.document_version == 0
        || input.registry_version == 0
        || !input.document_hash.starts_with("sha256:")
    {
        return Ok(None);
    }
    let connection = crate::internal::local_state::open_writable(sqlite_path)?;
    crate::internal::local_state::schema::ensure_schema(&connection)?;
    let transition_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM identity_transition_pending WHERE owner_identity_id=?1 OR current_did=?2",
            rusqlite::params![input.owner_identity_id, input.current_did],
            |row| row.get(0),
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    if transition_count != 0 {
        return Ok(None);
    }
    let exact_binding: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM identity_account_bindings WHERE owner_identity_id=?1 AND account_id=?2 AND current_did=?3 AND identity_generation=?4 AND device_id=?5 AND device_auth_generation=?6",
            rusqlite::params![
                input.owner_identity_id,
                input.account_user_id,
                input.current_did,
                input.binding_generation,
                input.protocol_device_id,
                input.device_auth_generation.to_string(),
            ],
            |row| row.get(0),
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    if exact_binding != 1 {
        return Ok(None);
    }
    let mut digest = Sha256::new();
    digest.update(b"awiki.identity.legacy-registry-epoch-adoption.v1\0");
    for value in [
        state_root_fingerprint(sqlite_path),
        input.owner_identity_id.to_owned(),
        input.account_user_id.to_owned(),
        input.current_did.to_owned(),
        input.binding_generation.to_owned(),
        input.protocol_device_id.to_owned(),
        input.device_auth_generation.to_string(),
        input.document_version.to_string(),
        input.document_hash.to_owned(),
        input.registry_version.to_string(),
    ] {
        digest.update((value.len() as u64).to_be_bytes());
        digest.update(value.as_bytes());
    }
    Ok(Some(
        crate::identity::LegacyRegistryEpochAdoptionAuthority {
            owner_identity_id: crate::ids::IdentityId::parse(input.owner_identity_id)?,
            account_user_id: input.account_user_id.to_owned(),
            current_did: crate::ids::Did::parse(input.current_did)?,
            binding_generation: input.binding_generation.to_owned(),
            protocol_device_id: crate::ids::ProtocolDeviceId::parse(input.protocol_device_id)?,
            device_auth_generation: input.device_auth_generation.to_string(),
            provenance_id: format!("sha256:{:x}", digest.finalize()),
        },
    ))
}

fn now() -> crate::ImResult<String> {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|error| crate::ImError::Serialization {
            detail: error.to_string(),
        })
}

use rusqlite::OptionalExtension as _;

#[cfg(test)]
mod tests {
    use super::*;

    fn test_marker(path: &Path) -> IdentityTransitionMarker {
        IdentityTransitionMarker {
            schema_version: 1,
            contract_version: crate::internal::identity_handle_recovery_pending::CONTRACT_VERSION
                .to_owned(),
            contract_hash: crate::internal::identity_handle_recovery_pending::CONTRACT_HASH
                .to_owned(),
            recovery_id: "recovery-1".to_owned(),
            source_kind: TransitionSourceKind::Initiator,
            source_id: "recover-001".to_owned(),
            state_root_fingerprint: state_root_fingerprint(path),
            account_user_id: "user-1".to_owned(),
            owner_identity_id: "owner-1".to_owned(),
            handle: "alice.example.invalid".to_owned(),
            previous_did: "did:wba:example.invalid:users:alice-old".to_owned(),
            current_did: "did:wba:example.invalid:users:alice-new".to_owned(),
            binding_generation: "8".to_owned(),
            phase: TransitionPhase::Pending,
            created_at: "2026-08-03T00:01:00Z".to_owned(),
            updated_at: "2026-08-03T00:01:00Z".to_owned(),
        }
    }

    fn insert_old_binding(path: &Path) {
        let connection = crate::internal::local_state::open_writable(path).unwrap();
        crate::internal::local_state::schema::ensure_schema(&connection).unwrap();
        connection.execute(
            "INSERT INTO identity_account_bindings(owner_identity_id,account_id,handle_scope,current_did,device_id,identity_generation,device_auth_generation,created_at,updated_at) VALUES ('owner-1','user-1','alice.example.invalid','did:wba:example.invalid:users:alice-old','old-device','7','3',1,1)",
            [],
        ).unwrap();
    }

    #[test]
    fn transition_table_is_secret_free_and_source_bound() {
        assert!(!IDENTITY_TRANSITION_SQL.contains("grant"));
        assert!(!IDENTITY_TRANSITION_SQL.contains("secret_ref"));
        assert!(IDENTITY_TRANSITION_SQL.contains("source_kind"));
        assert!(IDENTITY_TRANSITION_SQL.contains("source_id"));
    }

    #[test]
    fn joined_device_marker_is_persisted_and_bound_to_the_exact_join_session() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("joined-marker.sqlite");
        let marker = IdentityTransitionMarker::joined_device(
            &path,
            "join-session-1",
            "user-1",
            "owner-1",
            "alice.awiki.info",
            "did:wba:awiki.info:users:alice-old",
            "did:wba:awiki.info:users:alice-new",
            "8",
        )
        .unwrap();
        persist(&path, &marker).unwrap();

        assert_eq!(
            load_joined_device(&path, "join-session-1").unwrap(),
            Some(marker)
        );
        assert_eq!(load_joined_device(&path, "join-session-2").unwrap(), None);
    }

    #[test]
    fn copied_transition_database_cannot_authorize_another_state_root() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.sqlite");
        let copied = directory.path().join("copied.sqlite");
        let marker = test_marker(&source);
        persist(&source, &marker).unwrap();
        let db = crate::internal::local_state::open_writable(&source).unwrap();
        db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)").unwrap();
        drop(db);
        std::fs::copy(&source, &copied).unwrap();
        assert!(matches!(
            load(&copied, &marker.recovery_id),
            Err(crate::ImError::PermissionDenied)
        ));
    }

    #[test]
    fn migration_cas_rejects_changed_binding_and_is_idempotent_after_commit() {
        let directory = tempfile::tempdir().unwrap();
        let stale_path = directory.path().join("stale.sqlite");
        insert_old_binding(&stale_path);
        let stale_marker = test_marker(&stale_path);
        persist(&stale_path, &stale_marker).unwrap();
        let db = crate::internal::local_state::open_writable(&stale_path).unwrap();
        db.execute(
            "UPDATE identity_account_bindings SET current_did='did:wba:example.invalid:users:other' WHERE owner_identity_id='owner-1'",
            [],
        )
        .unwrap();
        drop(db);
        assert!(matches!(
            migrate_local_state(&stale_path, &stale_marker, "device-new", 1),
            Err(crate::ImError::PermissionDenied)
        ));

        let idempotent_path = directory.path().join("idempotent.sqlite");
        insert_old_binding(&idempotent_path);
        let marker = test_marker(&idempotent_path);
        persist(&idempotent_path, &marker).unwrap();
        migrate_local_state(&idempotent_path, &marker, "device-new", 1).unwrap();
        migrate_local_state(&idempotent_path, &marker, "device-new", 1).unwrap();
        update_phase(
            &idempotent_path,
            &marker.recovery_id,
            TransitionPhase::Pending,
            TransitionPhase::IdentitySwitched,
        )
        .unwrap();
        assert_eq!(
            load(&idempotent_path, &marker.recovery_id)
                .unwrap()
                .unwrap()
                .phase,
            TransitionPhase::IdentitySwitched
        );
    }

    #[test]
    fn fresh_initiator_migration_accepts_only_a_missing_owner_binding() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("fresh.sqlite");
        let marker = test_marker(&path);
        persist(&path, &marker).unwrap();

        migrate_initiator_without_local_identity(&path, &marker, "device-new", 1).unwrap();

        let connection = crate::internal::local_state::open_writable(&path).unwrap();
        let binding: (String, String, String) = connection
            .query_row(
                "SELECT account_id,current_did,identity_generation FROM identity_account_bindings WHERE owner_identity_id='owner-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            binding,
            (
                "user-1".to_owned(),
                "did:wba:example.invalid:users:alice-new".to_owned(),
                "8".to_owned(),
            )
        );
    }

    #[test]
    fn authorized_epoch_reset_preserves_history_and_retires_old_crypto() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.sqlite");
        let connection = crate::internal::local_state::open_writable(&path).unwrap();
        crate::internal::local_state::schema::ensure_schema(&connection).unwrap();
        connection.execute(
            "INSERT INTO identity_account_bindings(owner_identity_id,account_id,handle_scope,current_did,device_id,identity_generation,device_auth_generation,created_at,updated_at) VALUES ('owner-1','user-1','alice.example.invalid','did:wba:example.invalid:users:alice-old','old-device','7','3',1,1)",
            [],
        ).unwrap();
        connection.execute(
            "INSERT INTO identity_did_history(owner_identity_id,did,status,first_seen_at,last_seen_at,metadata) VALUES ('owner-1','did:wba:example.invalid:users:alice-old','current','2026-08-03T00:00:00Z','2026-08-03T00:00:00Z',NULL)",
            [],
        ).unwrap();
        connection.execute(
            "INSERT INTO direct_e2ee_sessions(owner_identity_id,owner_did,peer_did,session_id,state_blob,created_at,updated_at) VALUES ('owner-1','did:wba:example.invalid:users:alice-old','did:wba:example.invalid:users:bob','session-old',X'01','2026-08-03T00:00:00Z','2026-08-03T00:00:00Z')",
            [],
        ).unwrap();
        drop(connection);

        let marker = IdentityTransitionMarker {
            schema_version: 1,
            contract_version: crate::internal::identity_handle_recovery_pending::CONTRACT_VERSION
                .to_owned(),
            contract_hash: crate::internal::identity_handle_recovery_pending::CONTRACT_HASH
                .to_owned(),
            recovery_id: "recovery-1".to_owned(),
            source_kind: TransitionSourceKind::Initiator,
            source_id: "recover-001".to_owned(),
            state_root_fingerprint: state_root_fingerprint(&path),
            account_user_id: "user-1".to_owned(),
            owner_identity_id: "owner-1".to_owned(),
            handle: "alice.example.invalid".to_owned(),
            previous_did: "did:wba:example.invalid:users:alice-old".to_owned(),
            current_did: "did:wba:example.invalid:users:alice-new".to_owned(),
            binding_generation: "8".to_owned(),
            phase: TransitionPhase::Pending,
            created_at: "2026-08-03T00:01:00Z".to_owned(),
            updated_at: "2026-08-03T00:01:00Z".to_owned(),
        };
        persist(&path, &marker).unwrap();
        migrate_local_state(&path, &marker, "device-new", 1).unwrap();

        let connection = crate::internal::local_state::open_writable(&path).unwrap();
        let binding: (String, String, String, String) = connection
            .query_row(
                "SELECT owner_identity_id,current_did,device_id,identity_generation FROM identity_account_bindings WHERE owner_identity_id='owner-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            binding,
            (
                "owner-1".to_owned(),
                "did:wba:example.invalid:users:alice-new".to_owned(),
                "device-new".to_owned(),
                "8".to_owned(),
            )
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM direct_e2ee_sessions WHERE owner_identity_id='owner-1'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM identity_did_history WHERE owner_identity_id='owner-1'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            2
        );
    }

    #[test]
    fn legacy_epoch_authority_is_exact_marker_free_and_restart_deterministic() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("authority.sqlite");
        let connection = crate::internal::local_state::open_writable(&path).unwrap();
        crate::internal::local_state::schema::ensure_schema(&connection).unwrap();
        connection.execute(
            "INSERT INTO identity_account_bindings(owner_identity_id,account_id,handle_scope,current_did,device_id,identity_generation,device_auth_generation,created_at,updated_at) VALUES ('e1_owner','user-1','alice.example.invalid','did:wba:example.invalid:user:alice:e1_owner','device-1','7','3',1,1)",
            [],
        ).unwrap();
        drop(connection);

        let authority = || {
            legacy_registry_epoch_adoption_authority(
                &path,
                LegacyAuthorityInput {
                    owner_identity_id: "e1_owner",
                    account_user_id: "user-1",
                    current_did: "did:wba:example.invalid:user:alice:e1_owner",
                    binding_generation: "7",
                    protocol_device_id: "device-1",
                    device_auth_generation: 3,
                    document_version: 4,
                    document_hash: "sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
                    registry_version: 9,
                },
            )
            .unwrap()
        };
        let first = authority().expect("ordinary legacy epoch authority");
        let restarted = authority().expect("restart authority");
        assert_eq!(first, restarted);
        assert_eq!(first.owner_identity_id.as_str(), "e1_owner");

        for mismatch in ["did", "generation", "owner"] {
            let result = legacy_registry_epoch_adoption_authority(
                &path,
                LegacyAuthorityInput {
                    owner_identity_id: if mismatch == "owner" {
                        "e1_other"
                    } else {
                        "e1_owner"
                    },
                    account_user_id: "user-1",
                    current_did: if mismatch == "did" {
                        "did:wba:example.invalid:user:alice:e1_other"
                    } else {
                        "did:wba:example.invalid:user:alice:e1_owner"
                    },
                    binding_generation: if mismatch == "generation" { "8" } else { "7" },
                    protocol_device_id: "device-1",
                    device_auth_generation: 3,
                    document_version: 4,
                    document_hash: "sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
                    registry_version: 9,
                },
            )
            .unwrap();
            assert!(result.is_none(), "{mismatch} mismatch must fail closed");
        }

        let connection = crate::internal::local_state::open_writable(&path).unwrap();
        for phase in ["pending", "identity_switched", "completed"] {
            connection
                .execute("DELETE FROM identity_transition_pending", [])
                .unwrap();
            connection.execute(
                "INSERT INTO identity_transition_pending(recovery_id,schema_version,contract_version,contract_hash,source_kind,source_id,state_root_fingerprint,account_user_id,owner_identity_id,handle,previous_did,current_did,binding_generation,phase,created_at,updated_at) VALUES (?1,1,?2,?3,'initiator','recover-1',?4,'user-1','e1_owner','alice.example.invalid','did:wba:example.invalid:user:alice:e1_old','did:wba:example.invalid:user:alice:e1_owner','7',?5,'2026-08-03T00:00:00Z','2026-08-03T00:00:00Z')",
                rusqlite::params![
                    format!("recovery-{phase}"),
                    crate::internal::identity_handle_recovery_pending::CONTRACT_VERSION,
                    crate::internal::identity_handle_recovery_pending::CONTRACT_HASH,
                    state_root_fingerprint(&path),
                    phase,
                ],
            ).unwrap();
            assert!(authority().is_none(), "{phase} marker must fail closed");
        }
    }
}
