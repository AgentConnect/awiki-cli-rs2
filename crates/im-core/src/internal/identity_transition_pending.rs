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
    current_device_id TEXT,
    device_auth_generation TEXT,
    registry_version TEXT,
    applied_at TEXT,
    metadata_json TEXT NOT NULL DEFAULT '{}',
    phase TEXT NOT NULL CHECK(phase IN ('pending','identity_switched','completed')),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_identity_transition_source
ON identity_transition_pending(source_kind, source_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_identity_transition_active_owner
ON identity_transition_pending(owner_identity_id)
WHERE phase IN ('pending','identity_switched');
CREATE INDEX IF NOT EXISTS idx_identity_transition_owner_phase
ON identity_transition_pending(owner_identity_id, phase, updated_at);
CREATE INDEX IF NOT EXISTS idx_identity_transition_account_generation
ON identity_transition_pending(account_user_id, binding_generation);
CREATE INDEX IF NOT EXISTS idx_identity_transition_handle_epoch
ON identity_transition_pending(handle, current_did, binding_generation);
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
    pub(crate) current_device_id: Option<String>,
    pub(crate) device_auth_generation: Option<String>,
    pub(crate) registry_version: Option<String>,
    pub(crate) applied_at: Option<String>,
    pub(crate) metadata_json: String,
    pub(crate) phase: TransitionPhase,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}

impl IdentityTransitionMarker {
    pub(crate) fn initiator_v4(
        sqlite_path: &Path,
        pending: &crate::internal::identity_handle_recovery_pending::PendingHandleRecoveryV4,
        result: &crate::internal::identity_handle_recovery_pending::RecoveryRemoteResultV4,
    ) -> crate::ImResult<Self> {
        let intent = pending
            .intent
            .as_ref()
            .ok_or(crate::ImError::PermissionDenied)?;
        let binding = pending
            .authoritative_binding
            .as_ref()
            .ok_or(crate::ImError::PermissionDenied)?;
        if result.operation_id != pending.operation_id
            || result.intent_hash != pending.intent_hash.as_deref().unwrap_or("")
            || result.account_user_id != binding.account_user_id
            || result.full_handle != pending.full_handle
            || result.previous_did != binding.current_did
            || result.current_did != intent.new_did
        {
            return Err(crate::ImError::PermissionDenied);
        }
        let now = now()?;
        let marker = Self {
            schema_version: 1,
            contract_version:
                crate::internal::identity_handle_recovery_pending::V4_CONTRACT_VERSION.to_owned(),
            contract_hash: crate::internal::identity_handle_recovery_pending::V4_CONTRACT_HASH
                .to_owned(),
            recovery_id: pending.operation_id.clone(),
            source_kind: TransitionSourceKind::Initiator,
            source_id: pending.operation_id.clone(),
            state_root_fingerprint: state_root_fingerprint(sqlite_path),
            account_user_id: result.account_user_id.clone(),
            owner_identity_id: pending.owner_identity_id.clone(),
            handle: result.full_handle.clone(),
            previous_did: pending.local_previous_did.clone(),
            current_did: result.current_did.clone(),
            binding_generation: result.binding_generation.clone(),
            current_device_id: Some(result.bootstrap_device.device_id.clone()),
            device_auth_generation: Some(result.bootstrap_device.auth_generation.to_string()),
            registry_version: Some(result.checkpoint.registry_version.to_string()),
            applied_at: None,
            metadata_json: "{}".to_owned(),
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
            contract_version:
                crate::internal::identity_handle_recovery_pending::V4_CONTRACT_VERSION.to_owned(),
            contract_hash: crate::internal::identity_handle_recovery_pending::V4_CONTRACT_HASH
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
            current_device_id: None,
            device_auth_generation: None,
            registry_version: None,
            applied_at: None,
            metadata_json: "{}".to_owned(),
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
                != crate::internal::identity_handle_recovery_pending::V4_CONTRACT_VERSION
            || self.contract_hash
                != crate::internal::identity_handle_recovery_pending::V4_CONTRACT_HASH
            || self.recovery_id.trim().is_empty()
            || self.source_id.trim().is_empty()
            || self.account_user_id.trim().is_empty()
            || self.owner_identity_id.trim().is_empty()
            || self.previous_did == self.current_did
            || !valid_state_root_fingerprint(&self.state_root_fingerprint)
            || crate::internal::identity_wire::handle_recovery::canonical_handle(&self.handle)
                .is_err()
            || serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(
                &self.metadata_json,
            )
            .is_err()
            || self.metadata_json != "{}"
        {
            return Err(crate::ImError::PermissionDenied);
        }
        for value in [
            self.device_auth_generation.as_deref(),
            self.registry_version.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            if !canonical_generation(value) {
                return Err(crate::ImError::PermissionDenied);
            }
        }
        if self.phase == TransitionPhase::Completed
            && (self.current_device_id.as_deref().unwrap_or("").is_empty()
                || self.device_auth_generation.is_none()
                || self.registry_version.is_none()
                || self.applied_at.is_none())
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
            "SELECT schema_version,contract_version,contract_hash,recovery_id,state_root_fingerprint,account_user_id,owner_identity_id,handle,previous_did,current_did,binding_generation,phase,created_at,updated_at,current_device_id,device_auth_generation,registry_version,applied_at,metadata_json FROM identity_transition_pending WHERE source_kind='joined_device' AND source_id=?1",
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
                    current_device_id: row.get(14)?,
                    device_auth_generation: row.get(15)?,
                    registry_version: row.get(16)?,
                    applied_at: row.get(17)?,
                    metadata_json: row.get(18)?,
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
            "SELECT schema_version,contract_version,contract_hash,source_kind,source_id,state_root_fingerprint,account_user_id,owner_identity_id,handle,previous_did,current_did,binding_generation,phase,created_at,updated_at,current_device_id,device_auth_generation,registry_version,applied_at,metadata_json FROM identity_transition_pending WHERE recovery_id=?1",
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
                    current_device_id: row.get(15)?,
                    device_auth_generation: row.get(16)?,
                    registry_version: row.get(17)?,
                    applied_at: row.get(18)?,
                    metadata_json: row.get(19)?,
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

/// Returns transitions that crossed the durable identity-switch checkpoint but
/// did not finish their local post-switch work. These markers are the only
/// authority allowed to repair a stale active identity-index entry at startup.
pub(crate) fn load_identity_switched(
    sqlite_path: &Path,
) -> crate::ImResult<Vec<IdentityTransitionMarker>> {
    let connection = crate::internal::local_state::open_writable(sqlite_path)?;
    crate::internal::local_state::schema::ensure_schema(&connection)?;
    let recovery_ids = {
        let mut statement = connection
            .prepare(
                "SELECT recovery_id FROM identity_transition_pending WHERE phase='identity_switched' ORDER BY updated_at,recovery_id",
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        let recovery_ids = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(crate::internal::local_state::local_state_unavailable)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        recovery_ids
    };
    drop(connection);

    recovery_ids
        .into_iter()
        .map(|recovery_id| load(sqlite_path, &recovery_id)?.ok_or(crate::ImError::PermissionDenied))
        .collect()
}

pub(crate) fn load_latest_applied_for_owner(
    sqlite_path: &Path,
    owner_identity_id: &str,
) -> crate::ImResult<Option<IdentityTransitionMarker>> {
    let connection = crate::internal::local_state::open_writable(sqlite_path)?;
    crate::internal::local_state::schema::ensure_schema(&connection)?;
    let recovery_id = connection
        .query_row(
            "SELECT recovery_id FROM identity_transition_pending WHERE owner_identity_id=?1 AND phase='completed' ORDER BY applied_at DESC,updated_at DESC,recovery_id DESC LIMIT 1",
            [owner_identity_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    recovery_id
        .map(|recovery_id| load(sqlite_path, &recovery_id))
        .transpose()
        .map(Option::flatten)
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
    let other_active_transition: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM identity_transition_pending WHERE owner_identity_id=?1 AND recovery_id<>?2 AND phase IN ('pending','identity_switched')",
            rusqlite::params![marker.owner_identity_id, marker.recovery_id],
            |row| row.get(0),
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    if other_active_transition != 0 {
        let code = crate::identity::HandleRecoveryErrorCode::LocalTransitionPending.as_str();
        return Err(crate::ImError::Service {
            status_code: None,
            code: Some(code.to_owned()),
            message: code.to_owned(),
            data: None,
        });
    }
    connection
        .execute(
            r#"
INSERT INTO identity_transition_pending
    (recovery_id,schema_version,contract_version,contract_hash,source_kind,source_id,
     state_root_fingerprint,account_user_id,owner_identity_id,handle,previous_did,current_did,
     binding_generation,current_device_id,device_auth_generation,registry_version,applied_at,
     metadata_json,phase,created_at,updated_at)
VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21)
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
  AND identity_transition_pending.binding_generation=excluded.binding_generation
  AND identity_transition_pending.current_device_id IS excluded.current_device_id
  AND identity_transition_pending.device_auth_generation IS excluded.device_auth_generation
  AND identity_transition_pending.registry_version IS excluded.registry_version
  AND identity_transition_pending.applied_at IS excluded.applied_at
  AND identity_transition_pending.metadata_json=excluded.metadata_json"#,
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
                marker.current_device_id,
                marker.device_auth_generation,
                marker.registry_version,
                marker.applied_at,
                marker.metadata_json,
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

#[allow(clippy::too_many_arguments)]
pub(crate) fn mark_applied(
    sqlite_path: &Path,
    recovery_id: &str,
    expected: TransitionPhase,
    current_device_id: &str,
    device_auth_generation: &str,
    registry_version: &str,
    metadata_json: &str,
) -> crate::ImResult<()> {
    if current_device_id.trim().is_empty()
        || !canonical_generation(device_auth_generation)
        || !canonical_generation(registry_version)
        || serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(metadata_json)
            .is_err()
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let connection = crate::internal::local_state::open_writable(sqlite_path)?;
    let applied_at = now()?;
    let changed = connection
        .execute(
            "UPDATE identity_transition_pending SET phase='completed',current_device_id=?1,device_auth_generation=?2,registry_version=?3,applied_at=?4,metadata_json=?5,updated_at=?4 WHERE recovery_id=?6 AND phase=?7 AND state_root_fingerprint=?8",
            rusqlite::params![
                current_device_id,
                device_auth_generation,
                registry_version,
                applied_at,
                metadata_json,
                recovery_id,
                expected.as_str(),
                state_root_fingerprint(sqlite_path),
            ],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    if changed != 1 {
        let current = load(sqlite_path, recovery_id)?;
        if current.as_ref().map(|marker| marker.phase) != Some(TransitionPhase::Completed) {
            return Err(crate::ImError::PermissionDenied);
        }
    }
    Ok(())
}

/// Applies the recovery-authorized state epoch reset while preserving business
/// history. The marker tuple is rechecked inside the same transaction before
/// superseded write-capable state is retired.
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
        false,
    )
}

/// Applies the explicitly confirmed V4.0 fresh-state break-glass path. The
/// local binding may be older than the authoritative Recovery predecessor,
/// but it must still belong to the same stable owner/account/Handle and be
/// strictly older than the committed epoch. This does not perform V4.1
/// transparent history adoption; superseded write-capable state is retired by
/// the same transaction as an ordinary Recovery transition.
pub(crate) fn migrate_initiator_fresh_local_state(
    sqlite_path: &Path,
    marker: &IdentityTransitionMarker,
    bootstrap_device_id: &str,
    auth_generation: u64,
) -> crate::ImResult<()> {
    if marker.source_kind != TransitionSourceKind::Initiator
        || marker.contract_version
            != crate::internal::identity_handle_recovery_pending::V4_CONTRACT_VERSION
        || marker.contract_hash
            != crate::internal::identity_handle_recovery_pending::V4_CONTRACT_HASH
    {
        return Err(crate::ImError::PermissionDenied);
    }
    migrate_local_state_inner(
        sqlite_path,
        marker,
        bootstrap_device_id,
        auth_generation,
        false,
        true,
    )
}

/// Installs a recovered identity on a machine that has never held the Handle.
/// The authoritative previous DID is retained in the receipt, while the new
/// stable owner starts with no prior local account binding or history rows.
pub(crate) fn migrate_initiator_new_local_state(
    sqlite_path: &Path,
    marker: &IdentityTransitionMarker,
    bootstrap_device_id: &str,
    auth_generation: u64,
) -> crate::ImResult<()> {
    if marker.source_kind != TransitionSourceKind::Initiator
        || marker.contract_version
            != crate::internal::identity_handle_recovery_pending::V4_CONTRACT_VERSION
        || marker.contract_hash
            != crate::internal::identity_handle_recovery_pending::V4_CONTRACT_HASH
    {
        return Err(crate::ImError::PermissionDenied);
    }
    migrate_local_state_inner(
        sqlite_path,
        marker,
        bootstrap_device_id,
        auth_generation,
        true,
        false,
    )
}

fn migrate_local_state_inner(
    sqlite_path: &Path,
    marker: &IdentityTransitionMarker,
    bootstrap_device_id: &str,
    auth_generation: u64,
    allow_missing_previous_binding: bool,
    allow_stale_previous_binding: bool,
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
    let previous_generation =
        crate::internal::identity_handle_recovery_pending::previous_canonical_generation(
            &marker.binding_generation,
        )
        .ok_or(crate::ImError::PermissionDenied)?;
    let expected_previous_binding = (
        marker.account_user_id.clone(),
        Some(marker.handle.clone()),
        marker.previous_did.clone(),
        previous_generation,
    );
    let stale_previous_binding =
        binding
            .as_ref()
            .is_some_and(|(account_id, handle, current_did, binding_generation)| {
                account_id == &marker.account_user_id
                    && handle.as_deref() == Some(marker.handle.as_str())
                    && current_did == &marker.previous_did
                    && canonical_generation_is_less_than(
                        binding_generation,
                        &marker.binding_generation,
                    )
            });
    if binding.as_ref() != Some(&expected_previous_binding)
        && !(allow_missing_previous_binding && binding.is_none())
        && !(allow_stale_previous_binding && stale_previous_binding)
    {
        return Err(crate::ImError::PermissionDenied);
    }

    let now_unix = time::OffsetDateTime::now_utc().unix_timestamp();
    if let Some((account_id, handle_scope, current_did, identity_generation)) = binding.as_ref() {
        let changed = transaction
            .execute(
                r#"UPDATE identity_account_bindings
                   SET current_did=?2,device_id=?3,identity_generation=?4,
                       device_auth_generation=?5,updated_at=?6
                   WHERE owner_identity_id=?1 AND account_id=?7
                     AND handle_scope IS ?8 AND current_did=?9
                     AND identity_generation=?10"#,
                rusqlite::params![
                    marker.owner_identity_id,
                    marker.current_did,
                    bootstrap_device_id,
                    marker.binding_generation,
                    auth_generation.to_string(),
                    now_unix,
                    account_id,
                    handle_scope,
                    current_did,
                    identity_generation,
                ],
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        if changed != 1 {
            return Err(crate::ImError::PermissionDenied);
        }
    } else {
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
    }

    let now = now()?;
    retire_superseded_write_state(&transaction, marker, &now)?;

    for table in [
        "identity_root_import_completion_v1",
        "identity_root_transfer_sender_v1",
        "system_notification_join_state",
    ] {
        delete_owner_rows(&transaction, table, &marker.owner_identity_id)?;
    }

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
                    "protocol": "manifest_handle_recovery_v4",
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

fn retire_superseded_write_state(
    transaction: &rusqlite::Transaction<'_>,
    marker: &IdentityTransitionMarker,
    now: &str,
) -> crate::ImResult<()> {
    update_existing_table(
        transaction,
        "direct_e2ee_signed_prekeys",
        "UPDATE direct_e2ee_signed_prekeys SET status='retired',updated_at=?3 WHERE owner_identity_id=?1 AND owner_did=?2 AND status='active'",
        rusqlite::params![marker.owner_identity_id, marker.previous_did, now],
    )?;
    update_existing_table(
        transaction,
        "direct_e2ee_one_time_prekeys",
        "UPDATE direct_e2ee_one_time_prekeys SET status='consumed',consumed_at=COALESCE(consumed_at,?3) WHERE owner_identity_id=?1 AND owner_did=?2 AND status IN ('available','reserved')",
        rusqlite::params![marker.owner_identity_id, marker.previous_did, now],
    )?;
    update_existing_table(
        transaction,
        "direct_e2ee_v2_sessions",
        "UPDATE direct_e2ee_v2_sessions SET disabled=1,updated_at=?3 WHERE owner_identity_id=?1 AND owner_did=?2 AND disabled=0",
        rusqlite::params![marker.owner_identity_id, marker.previous_did, now],
    )?;
    update_existing_table(
        transaction,
        "direct_e2ee_v2_prekey_bundles",
        "UPDATE direct_e2ee_v2_prekey_bundles SET status='retired',updated_at=?3 WHERE owner_identity_id=?1 AND owner_did=?2 AND status<>'retired'",
        rusqlite::params![marker.owner_identity_id, marker.previous_did, now],
    )?;
    update_existing_table(
        transaction,
        "direct_e2ee_v2_one_time_prekeys",
        "UPDATE direct_e2ee_v2_one_time_prekeys SET status='consumed',consumed_at=COALESCE(consumed_at,?3) WHERE owner_identity_id=?1 AND owner_did=?2 AND status IN ('available','reserved')",
        rusqlite::params![marker.owner_identity_id, marker.previous_did, now],
    )?;
    update_existing_table(
        transaction,
        "e2ee_outbox",
        "UPDATE e2ee_outbox SET local_status='dropped',last_error_code='did_transition_superseded_old_did',retry_hint=NULL,updated_at=?3 WHERE owner_identity_id=?1 AND owner_did=?2 AND local_status IN ('queued','failed')",
        rusqlite::params![marker.owner_identity_id, marker.previous_did, now],
    )?;
    update_existing_table(
        transaction,
        "group_rebind_outbox",
        "UPDATE group_rebind_outbox SET phase='blocked',lease_expires_at=NULL,next_attempt_at=NULL,last_error_code='did_transition_replaced_legacy_rebind',last_error_detail=NULL,updated_at=?2 WHERE owner_identity_id=?1 AND phase NOT IN ('complete','blocked')",
        rusqlite::params![marker.owner_identity_id, now],
    )?;
    update_existing_table(
        transaction,
        "group_rebind_p6_jobs",
        "UPDATE group_rebind_p6_jobs SET phase='blocked',lease_expires_at=NULL,next_attempt_at=NULL,last_error_code='did_transition_replaced_legacy_rebind',last_error_detail=NULL,updated_at=?2 WHERE owner_identity_id=?1 AND phase NOT IN ('complete','blocked')",
        rusqlite::params![marker.owner_identity_id, now],
    )?;
    Ok(())
}

fn update_existing_table<P: rusqlite::Params>(
    transaction: &rusqlite::Transaction<'_>,
    table: &str,
    sql: &str,
    params: P,
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
            .execute(sql, params)
            .map_err(crate::internal::local_state::local_state_unavailable)?;
    }
    Ok(())
}

fn canonical_generation_is_less_than(left: &str, right: &str) -> bool {
    left.len() <= crate::internal::identity_wire::handle_recovery::MAX_BINDING_GENERATION_DIGITS
        && right.len()
            <= crate::internal::identity_wire::handle_recovery::MAX_BINDING_GENERATION_DIGITS
        && crate::internal::local_state::sync_v2::compare_decimal(left, right)
            .is_ok_and(|ordering| ordering == std::cmp::Ordering::Less)
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
        .replace_nanosecond(0)
        .map_err(|_| crate::ImError::PermissionDenied)?
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|error| crate::ImError::Serialization {
            detail: error.to_string(),
        })
}

fn canonical_generation(value: &str) -> bool {
    !value.is_empty()
        && value.len()
            <= crate::internal::identity_wire::handle_recovery::MAX_BINDING_GENERATION_DIGITS
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value.as_bytes()[0] != b'0'
}

fn valid_state_root_fingerprint(value: &str) -> bool {
    value.len() == 71
        && value.strip_prefix("sha256:").is_some_and(|digest| {
            digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
}

use rusqlite::OptionalExtension as _;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transition_timestamp_uses_contract_second_precision() {
        let value = now().unwrap();
        let parsed =
            time::OffsetDateTime::parse(&value, &time::format_description::well_known::Rfc3339)
                .unwrap();
        assert_eq!(parsed.nanosecond(), 0);
        assert!(value.ends_with('Z'));
        assert!(!value.contains('.'));
    }

    fn test_marker(path: &Path) -> IdentityTransitionMarker {
        IdentityTransitionMarker {
            schema_version: 1,
            contract_version:
                crate::internal::identity_handle_recovery_pending::V4_CONTRACT_VERSION.to_owned(),
            contract_hash: crate::internal::identity_handle_recovery_pending::V4_CONTRACT_HASH
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
            current_device_id: Some("device-new".to_owned()),
            device_auth_generation: Some("1".to_owned()),
            registry_version: Some("1".to_owned()),
            applied_at: None,
            metadata_json: "{}".to_owned(),
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
        for field in [
            "current_device_id",
            "device_auth_generation",
            "registry_version",
            "applied_at",
            "metadata_json",
        ] {
            assert!(IDENTITY_TRANSITION_SQL.contains(field), "missing {field}");
        }
    }

    #[test]
    fn state_root_fingerprint_is_exact_lowercase_hex() {
        assert!(valid_state_root_fingerprint(&format!(
            "sha256:{}",
            "a".repeat(64)
        )));
        assert!(!valid_state_root_fingerprint(&format!(
            "sha256:{}",
            "A".repeat(64)
        )));
        assert!(!valid_state_root_fingerprint(
            "sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
        ));
        assert!(!valid_state_root_fingerprint(&format!(
            "sha256:{}",
            "a".repeat(63)
        )));
    }

    #[test]
    fn marker_accepts_only_the_exact_v4_contract_pair() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("contract-pair.sqlite");
        let mut marker = test_marker(&path);
        assert!(marker.validate().is_ok());

        marker.contract_version = "unsupported-handle-recovery-contract".to_owned();
        assert_eq!(
            marker.validate().unwrap_err(),
            crate::ImError::PermissionDenied
        );

        marker.contract_version =
            crate::internal::identity_handle_recovery_pending::V4_CONTRACT_VERSION.to_owned();
        marker.contract_hash = "0".repeat(64);
        assert_eq!(
            marker.validate().unwrap_err(),
            crate::ImError::PermissionDenied
        );
    }

    #[test]
    fn startup_scan_returns_only_valid_identity_switched_markers() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("startup-reconcile.sqlite");
        let mut completed = test_marker(&path);
        completed.recovery_id = "recovery-completed".to_owned();
        completed.source_id = "recover-completed".to_owned();
        completed.phase = TransitionPhase::Completed;
        completed.applied_at = Some("2026-08-03T00:02:00Z".to_owned());
        persist(&path, &completed).unwrap();

        let mut switched = test_marker(&path);
        switched.recovery_id = "recovery-switched".to_owned();
        switched.source_id = "recover-switched".to_owned();
        switched.owner_identity_id = "owner-2".to_owned();
        switched.phase = TransitionPhase::IdentitySwitched;
        persist(&path, &switched).unwrap();

        let markers = load_identity_switched(&path).unwrap();
        assert_eq!(markers, vec![switched]);
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
    fn completed_receipts_are_history_while_only_one_transition_is_active() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("receipt-history.sqlite");
        let first = test_marker(&path);
        persist(&path, &first).unwrap();

        let mut second = first.clone();
        second.recovery_id = "recovery-2".to_owned();
        second.source_id = "recover-002".to_owned();
        second.previous_did = first.current_did.clone();
        second.current_did = "did:wba:example.invalid:users:alice-newer".to_owned();
        second.binding_generation = "9".to_owned();
        assert!(persist(&path, &second).is_err());

        mark_applied(
            &path,
            &first.recovery_id,
            TransitionPhase::Pending,
            "device-new",
            "1",
            "1",
            "{}",
        )
        .unwrap();
        persist(&path, &second).unwrap();

        let connection = crate::internal::local_state::open_writable(&path).unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM identity_transition_pending WHERE owner_identity_id='owner-1'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            2
        );
        let applied = load(&path, &first.recovery_id).unwrap().unwrap();
        assert_eq!(applied.phase, TransitionPhase::Completed);
        assert_eq!(applied.current_device_id.as_deref(), Some("device-new"));
        assert!(applied.applied_at.is_some());
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
    fn generation_predecessor_is_arbitrary_precision_and_canonical() {
        let previous =
            crate::internal::identity_handle_recovery_pending::previous_canonical_generation;
        assert_eq!(previous("2").as_deref(), Some("1"));
        assert_eq!(previous("1000").as_deref(), Some("999"));
        assert_eq!(
            previous("100000000000000000000000000000000000000").as_deref(),
            Some("99999999999999999999999999999999999999")
        );
        for invalid in ["", "0", "1", "01", "+2", "2 "] {
            assert_eq!(previous(invalid), None);
        }
    }

    #[test]
    fn confirmed_v4_fresh_migration_accepts_only_same_owner_stale_epoch() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("fresh.sqlite");
        insert_old_binding(&path);
        let mut marker = test_marker(&path);
        marker.contract_version =
            crate::internal::identity_handle_recovery_pending::V4_CONTRACT_VERSION.to_owned();
        marker.contract_hash =
            crate::internal::identity_handle_recovery_pending::V4_CONTRACT_HASH.to_owned();
        marker.binding_generation = "100000000000000000000000000000000000000".to_owned();
        persist(&path, &marker).unwrap();

        assert!(matches!(
            migrate_local_state(&path, &marker, "device-new", 1),
            Err(crate::ImError::PermissionDenied)
        ));
        migrate_initiator_fresh_local_state(&path, &marker, "device-new", 1).unwrap();
        let connection = crate::internal::local_state::open_writable(&path).unwrap();
        let binding: (String, String, String, String) = connection
            .query_row(
                "SELECT account_id,current_did,identity_generation,device_id FROM identity_account_bindings WHERE owner_identity_id='owner-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            binding,
            (
                "user-1".to_owned(),
                "did:wba:example.invalid:users:alice-new".to_owned(),
                marker.binding_generation,
                "device-new".to_owned(),
            )
        );
    }

    #[test]
    fn new_machine_recovery_installs_first_local_binding() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("new-machine.sqlite");
        let marker = test_marker(&path);
        persist(&path, &marker).unwrap();

        assert!(matches!(
            migrate_local_state(&path, &marker, "device-new", 1),
            Err(crate::ImError::PermissionDenied)
        ));
        migrate_initiator_new_local_state(&path, &marker, "device-new", 1).unwrap();

        let connection = crate::internal::local_state::open_writable(&path).unwrap();
        let binding: (String, String, String, String) = connection
            .query_row(
                "SELECT account_id,current_did,identity_generation,device_id FROM identity_account_bindings WHERE owner_identity_id='owner-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            binding,
            (
                "user-1".to_owned(),
                "did:wba:example.invalid:users:alice-new".to_owned(),
                "8".to_owned(),
                "device-new".to_owned(),
            )
        );
    }

    #[test]
    fn authorized_epoch_reset_preserves_old_crypto_and_retires_old_writers() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.sqlite");
        let connection = crate::internal::local_state::open_writable(&path).unwrap();
        crate::internal::local_state::schema::ensure_schema(&connection).unwrap();
        crate::internal::secure_direct::v2_store::ensure_v2_schema(&connection).unwrap();
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
        connection.execute(
            "INSERT INTO direct_e2ee_v2_owner_scopes(owner_identity_id,owner_did,local_device_id,created_at) VALUES ('owner-1','did:wba:example.invalid:users:alice-old','old-device','2026-08-03T00:00:00Z')",
            [],
        ).unwrap();
        connection.execute(
            "INSERT INTO direct_e2ee_v2_sessions(owner_identity_id,owner_did,local_device_id,peer_did,peer_device_id,session_id,state_blob,revision,disabled,created_at,updated_at) VALUES ('owner-1','did:wba:example.invalid:users:alice-old','old-device','did:wba:example.invalid:users:bob','bob-device','session-v2-old',X'01',0,0,'2026-08-03T00:00:00Z','2026-08-03T00:00:00Z')",
            [],
        ).unwrap();
        connection.execute(
            "INSERT INTO direct_e2ee_v2_pending(owner_identity_id,owner_did,local_device_id,peer_did,peer_device_id,operation_id,message_id,session_id,session_revision,pending_blob,created_at,updated_at) VALUES ('owner-1','did:wba:example.invalid:users:alice-old','old-device','did:wba:example.invalid:users:bob','bob-device','old-operation','old-message','session-v2-old',0,X'01','2026-08-03T00:00:00Z','2026-08-03T00:00:00Z')",
            [],
        ).unwrap();
        drop(connection);

        let marker = IdentityTransitionMarker {
            schema_version: 1,
            contract_version:
                crate::internal::identity_handle_recovery_pending::V4_CONTRACT_VERSION.to_owned(),
            contract_hash: crate::internal::identity_handle_recovery_pending::V4_CONTRACT_HASH
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
            current_device_id: Some("device-new".to_owned()),
            device_auth_generation: Some("1".to_owned()),
            registry_version: Some("1".to_owned()),
            applied_at: None,
            metadata_json: "{}".to_owned(),
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
            1
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT disabled FROM direct_e2ee_v2_sessions WHERE owner_identity_id='owner-1' AND session_id='session-v2-old'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM direct_e2ee_v2_pending WHERE owner_identity_id='owner-1' AND operation_id='old-operation'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
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
        let metadata: String = connection
            .query_row(
                "SELECT metadata FROM identity_did_history WHERE owner_identity_id='owner-1' AND did='did:wba:example.invalid:users:alice-new'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&metadata).unwrap()["protocol"],
            "manifest_handle_recovery_v4"
        );
    }

    #[test]
    fn phase4_0714_recovery_preserves_history_and_isolates_legacy_writers() {
        let Ok(fixture_dir) = std::env::var("AWIKI_0714_E2EE_FIXTURE_DIR") else {
            return;
        };
        let source = Path::new(&fixture_dir).join("core-schema-36.sqlite");
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("phase4-0714-recovery.sqlite");
        std::fs::copy(source, &path).unwrap();

        let connection = crate::internal::local_state::open_writable(&path).unwrap();
        crate::internal::local_state::schema::ensure_schema(&connection).unwrap();
        drop(connection);

        let marker = IdentityTransitionMarker {
            schema_version: 1,
            contract_version:
                crate::internal::identity_handle_recovery_pending::V4_CONTRACT_VERSION.to_owned(),
            contract_hash: crate::internal::identity_handle_recovery_pending::V4_CONTRACT_HASH
                .to_owned(),
            recovery_id: "fixture-recovery-0714".to_owned(),
            source_kind: TransitionSourceKind::Initiator,
            source_id: "fixture-recovery-0714".to_owned(),
            state_root_fingerprint: state_root_fingerprint(&path),
            account_user_id: "fixture-account-0714".to_owned(),
            owner_identity_id: "fixture-owner-0714".to_owned(),
            handle: "fixture.invalid".to_owned(),
            previous_did: "did:wba:alice.fixture.invalid:agents:primary:e1_fixture_alice"
                .to_owned(),
            current_did: "did:wba:alice.fixture.invalid:agents:primary:e1_fixture_alice_next"
                .to_owned(),
            binding_generation: "2".to_owned(),
            current_device_id: Some("fixture-device-0714-next".to_owned()),
            device_auth_generation: Some("1".to_owned()),
            registry_version: Some("2".to_owned()),
            applied_at: None,
            metadata_json: "{}".to_owned(),
            phase: TransitionPhase::Pending,
            created_at: "2026-08-26T00:00:00Z".to_owned(),
            updated_at: "2026-08-26T00:00:00Z".to_owned(),
        };
        persist(&path, &marker).unwrap();
        migrate_local_state(&path, &marker, "fixture-device-0714-next", 1).unwrap();

        let connection = crate::internal::local_state::open_writable(&path).unwrap();
        let binding: (String, String, String, String, String, String, i64) = connection
            .query_row(
                "SELECT owner_identity_id,account_id,current_did,device_id,identity_generation,device_auth_generation,created_at FROM identity_account_bindings",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(
            binding,
            (
                "fixture-owner-0714".to_owned(),
                "fixture-account-0714".to_owned(),
                marker.current_did.clone(),
                "fixture-device-0714-next".to_owned(),
                "2".to_owned(),
                "1".to_owned(),
                1_710_400_000,
            )
        );

        for (table, expected) in [
            ("messages", 2_i64),
            ("conversation_registry", 1),
            ("conversation_summaries", 1),
            ("direct_peer_routes", 1),
            ("attachment_manifest_cache", 1),
            ("sync_state", 1),
            ("e2ee_outbox", 1),
            ("group_rebind_outbox", 1),
        ] {
            assert_eq!(
                connection
                    .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                        row.get::<_, i64>(0)
                    })
                    .unwrap(),
                expected,
                "Recovery changed the locked 0714 {table} row count"
            );
        }
        assert_eq!(
            connection
                .query_row(
                    "SELECT event_seq FROM sync_state WHERE owner_identity_id='fixture-owner-0714' AND sync_subject_id='fixture-account-0714'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "2"
        );
        let outbox: (String, String, Option<String>) = connection
            .query_row(
                "SELECT owner_did,local_status,last_error_code FROM e2ee_outbox WHERE outbox_id='fixture-outbox-0714'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            outbox,
            (
                marker.previous_did.clone(),
                "dropped".to_owned(),
                Some("did_transition_superseded_old_did".to_owned()),
            )
        );
        let legacy_rebind: (String, Option<String>, Option<String>, Option<String>) = connection
            .query_row(
                "SELECT phase,next_attempt_at,lease_expires_at,last_error_code FROM group_rebind_outbox WHERE job_id='fixture-rebind-job-0714'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            legacy_rebind,
            (
                "blocked".to_owned(),
                None,
                None,
                Some("did_transition_replaced_legacy_rebind".to_owned()),
            )
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
                    crate::internal::identity_handle_recovery_pending::V4_CONTRACT_VERSION,
                    crate::internal::identity_handle_recovery_pending::V4_CONTRACT_HASH,
                    state_root_fingerprint(&path),
                    phase,
                ],
            ).unwrap();
            assert!(authority().is_none(), "{phase} marker must fail closed");
        }
    }
}
