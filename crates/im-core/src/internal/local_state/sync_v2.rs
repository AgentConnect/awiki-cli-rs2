use rand::RngCore;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::BTreeSet;

pub(crate) const APPLIED_EVENT_MIN_RECEIPTS_PER_OWNER: i64 = 10_000;
pub(crate) const APPLIED_EVENT_SAFETY_WINDOW: u32 = 1_000;
pub(crate) const SYNC_CLEANUP_BATCH_SIZE: u32 = 256;
pub(crate) const TERMINAL_SYNC_STATE_RETENTION_SECONDS: i64 = 7 * 24 * 60 * 60;

pub(crate) const SYNC_INSTALLATION_SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS sync_installation_state (
    owner_identity_id   TEXT PRIMARY KEY,
    client_instance_id  TEXT NOT NULL UNIQUE,
    created_at          INTEGER NOT NULL,
    CHECK (length(trim(owner_identity_id)) > 0),
    CHECK (length(trim(client_instance_id)) > 0)
);
"#;

pub(crate) const READ_RECOVERY_SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS sync_thread_bindings (
    owner_identity_id       TEXT NOT NULL,
    remote_thread_key       TEXT NOT NULL,
    thread_kind             TEXT NOT NULL,
    conversation_id         TEXT NOT NULL,
    updated_at              INTEGER NOT NULL,
    PRIMARY KEY (owner_identity_id, remote_thread_key),
    CHECK (thread_kind IN ('direct', 'group')),
    CHECK (length(trim(remote_thread_key)) > 0),
    CHECK (length(trim(conversation_id)) > 0),
    FOREIGN KEY (owner_identity_id)
        REFERENCES identity_account_bindings(owner_identity_id)
        ON DELETE CASCADE
);

CREATE UNIQUE INDEX IF NOT EXISTS sync_thread_bindings_conversation_idx
ON sync_thread_bindings(owner_identity_id, conversation_id);

CREATE TABLE IF NOT EXISTS sync_remote_read_states (
    owner_identity_id          TEXT NOT NULL,
    remote_thread_key          TEXT NOT NULL,
    thread_kind                TEXT NOT NULL,
    read_watermark_seq         TEXT NOT NULL,
    read_watermark_message_id  TEXT,
    state_version              TEXT NOT NULL,
    occurred_at                TEXT NOT NULL,
    PRIMARY KEY (owner_identity_id, remote_thread_key),
    CHECK (thread_kind IN ('direct', 'group')),
    FOREIGN KEY (owner_identity_id)
        REFERENCES identity_account_bindings(owner_identity_id)
        ON DELETE CASCADE
);
"#;

pub(crate) const SYNC_V2_SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS identity_account_bindings (
    owner_identity_id       TEXT PRIMARY KEY,
    account_id              TEXT NOT NULL,
    handle_scope            TEXT,
    current_did             TEXT NOT NULL,
    device_id               TEXT NOT NULL,
    identity_generation     TEXT NOT NULL,
    device_auth_generation  TEXT NOT NULL,
    created_at              INTEGER NOT NULL,
    updated_at              INTEGER NOT NULL,
    CHECK (length(trim(owner_identity_id)) > 0),
    CHECK (length(trim(account_id)) > 0),
    CHECK (length(trim(current_did)) > 0),
    CHECK (length(trim(device_id)) > 0),
    CHECK (
        identity_generation <> ''
        AND identity_generation NOT GLOB '*[^0-9]*'
        AND substr(identity_generation, 1, 1) <> '0'
    ),
    CHECK (
        device_auth_generation <> ''
        AND device_auth_generation NOT GLOB '*[^0-9]*'
        AND substr(device_auth_generation, 1, 1) <> '0'
    )
);

CREATE UNIQUE INDEX IF NOT EXISTS identity_account_device_idx
ON identity_account_bindings(account_id, device_id);

CREATE TABLE IF NOT EXISTS message_sync_state (
    owner_identity_id       TEXT PRIMARY KEY,
    account_id              TEXT NOT NULL,
    device_id               TEXT NOT NULL,
    device_auth_generation  TEXT NOT NULL,
    stream_epoch            TEXT NOT NULL,
    scan_seq                TEXT NOT NULL,
    bootstrap_state         TEXT NOT NULL,
    last_server_time        TEXT,
    last_success_at         INTEGER,
    last_error_code         TEXT,
    metadata_json           TEXT,
    updated_at              INTEGER NOT NULL,
    CHECK (length(trim(owner_identity_id)) > 0),
    CHECK (length(trim(account_id)) > 0),
    CHECK (length(trim(device_id)) > 0),
    CHECK (
        device_auth_generation <> ''
        AND device_auth_generation NOT GLOB '*[^0-9]*'
        AND substr(device_auth_generation, 1, 1) <> '0'
    ),
    CHECK (
        stream_epoch <> ''
        AND stream_epoch NOT GLOB '*[^0-9]*'
        AND substr(stream_epoch, 1, 1) <> '0'
    ),
    CHECK (
        scan_seq <> ''
        AND scan_seq NOT GLOB '*[^0-9]*'
        AND (scan_seq = '0' OR substr(scan_seq, 1, 1) <> '0')
    ),
    CHECK (bootstrap_state IN (
        'uninitialized', 'tail_bootstrapped', 'active', 'recovering', 'blocked'
    )),
    FOREIGN KEY (owner_identity_id)
        REFERENCES identity_account_bindings(owner_identity_id)
        ON DELETE CASCADE
);

CREATE UNIQUE INDEX IF NOT EXISTS message_sync_state_account_device_idx
ON message_sync_state(account_id, device_id);

CREATE TABLE IF NOT EXISTS sync_applied_events (
    owner_identity_id  TEXT NOT NULL,
    event_id           TEXT NOT NULL,
    stream_epoch       TEXT NOT NULL,
    event_seq          TEXT NOT NULL,
    applied_at         INTEGER NOT NULL,
    PRIMARY KEY (owner_identity_id, event_id),
    CHECK (length(trim(owner_identity_id)) > 0),
    CHECK (length(trim(event_id)) > 0),
    CHECK (
        stream_epoch <> ''
        AND stream_epoch NOT GLOB '*[^0-9]*'
        AND substr(stream_epoch, 1, 1) <> '0'
    ),
    CHECK (
        event_seq <> ''
        AND event_seq NOT GLOB '*[^0-9]*'
        AND substr(event_seq, 1, 1) <> '0'
    ),
    FOREIGN KEY (owner_identity_id)
        REFERENCES identity_account_bindings(owner_identity_id)
        ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS sync_applied_events_prune_idx
ON sync_applied_events(
    owner_identity_id,
    length(stream_epoch),
    stream_epoch,
    length(event_seq),
    event_seq
);

CREATE INDEX IF NOT EXISTS sync_applied_events_applied_at_idx
ON sync_applied_events(owner_identity_id, applied_at DESC);

CREATE TABLE IF NOT EXISTS sync_recovery_state (
    owner_identity_id     TEXT PRIMARY KEY,
    mode                  TEXT NOT NULL,
    requested_from_epoch  TEXT NOT NULL,
    requested_from_seq    TEXT NOT NULL,
    recovery_id_hash      TEXT,
    snapshot_scan_seq     TEXT,
    status                TEXT NOT NULL,
    retry_count           INTEGER NOT NULL DEFAULT 0,
    last_error_code       TEXT,
    started_at            INTEGER NOT NULL,
    updated_at            INTEGER NOT NULL,
    CHECK (mode = 'compact_recovery'),
    CHECK (status IN (
        'recovering', 'downloading', 'applying', 'retryable',
        'completed', 'permanent_failure'
    )),
    CHECK (retry_count >= 0),
    CHECK (
        requested_from_epoch <> ''
        AND requested_from_epoch NOT GLOB '*[^0-9]*'
        AND substr(requested_from_epoch, 1, 1) <> '0'
    ),
    CHECK (
        requested_from_seq <> ''
        AND requested_from_seq NOT GLOB '*[^0-9]*'
        AND (
            requested_from_seq = '0'
            OR substr(requested_from_seq, 1, 1) <> '0'
        )
    ),
    CHECK (
        snapshot_scan_seq IS NULL
        OR (
            snapshot_scan_seq <> ''
            AND snapshot_scan_seq NOT GLOB '*[^0-9]*'
            AND (
                snapshot_scan_seq = '0'
                OR substr(snapshot_scan_seq, 1, 1) <> '0'
            )
        )
    ),
    FOREIGN KEY (owner_identity_id)
        REFERENCES identity_account_bindings(owner_identity_id)
        ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS sync_recovery_state_status_idx
ON sync_recovery_state(status, updated_at);

CREATE TABLE IF NOT EXISTS local_mutation_outbox (
    owner_identity_id  TEXT NOT NULL,
    mutation_id        TEXT NOT NULL,
    operation_id       TEXT NOT NULL,
    mutation_type      TEXT NOT NULL,
    aggregate_id       TEXT NOT NULL,
    payload_json       TEXT NOT NULL,
    status             TEXT NOT NULL,
    attempt_count      INTEGER NOT NULL DEFAULT 0,
    retry_at           INTEGER,
    in_flight_since    INTEGER,
    last_error_code    TEXT,
    created_at         INTEGER NOT NULL,
    updated_at         INTEGER NOT NULL,
    PRIMARY KEY (owner_identity_id, mutation_id),
    UNIQUE (owner_identity_id, operation_id),
    CHECK (length(trim(mutation_id)) > 0),
    CHECK (length(trim(operation_id)) > 0),
    CHECK (length(trim(aggregate_id)) > 0),
    CHECK (mutation_type = 'read_state_mark_read'),
    CHECK (status IN (
        'pending', 'in_flight', 'retryable', 'committed', 'permanent_failure'
    )),
    CHECK (attempt_count >= 0),
    FOREIGN KEY (owner_identity_id)
        REFERENCES identity_account_bindings(owner_identity_id)
        ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS local_mutation_outbox_drain_idx
ON local_mutation_outbox(owner_identity_id, status, retry_at, created_at);
"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IdentityAccountBinding {
    pub(crate) owner_identity_id: String,
    pub(crate) account_id: String,
    pub(crate) handle_scope: Option<String>,
    pub(crate) current_did: String,
    pub(crate) protocol_device_id: String,
    pub(crate) identity_generation: String,
    pub(crate) device_auth_generation: String,
    pub(crate) created_at: i64,
    pub(crate) updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MessageSyncState {
    pub(crate) owner_identity_id: String,
    pub(crate) account_id: String,
    pub(crate) protocol_device_id: String,
    pub(crate) device_auth_generation: String,
    pub(crate) stream_epoch: String,
    pub(crate) scan_seq: String,
    pub(crate) bootstrap_state: String,
    pub(crate) last_server_time: Option<String>,
    pub(crate) last_success_at: Option<i64>,
    pub(crate) last_error_code: Option<String>,
    pub(crate) metadata_json: Option<String>,
    pub(crate) updated_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MessageSyncBootstrapReason {
    MissingState,
    DeviceAuthGenerationChanged,
    StreamEpochChanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MessageSyncBootstrapFence {
    pub(crate) owner_identity_id: String,
    pub(crate) account_id: String,
    pub(crate) protocol_device_id: String,
    pub(crate) active_device_auth_generation: String,
    pub(crate) stored_device_auth_generation: Option<String>,
    pub(crate) stored_stream_epoch: Option<String>,
    pub(crate) requested_stream_epoch: Option<String>,
    pub(crate) reason: MessageSyncBootstrapReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MessageSyncStateAccess {
    Ready(MessageSyncState),
    BootstrapRequired(MessageSyncBootstrapFence),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AppliedEventReceipt {
    pub(crate) owner_identity_id: String,
    pub(crate) event_id: String,
    pub(crate) stream_epoch: String,
    pub(crate) event_seq: String,
    pub(crate) applied_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RecoveryState {
    pub(crate) owner_identity_id: String,
    pub(crate) mode: String,
    pub(crate) requested_from_epoch: String,
    pub(crate) requested_from_seq: String,
    pub(crate) recovery_id_hash: Option<String>,
    pub(crate) snapshot_scan_seq: Option<String>,
    pub(crate) status: String,
    pub(crate) retry_count: i64,
    pub(crate) last_error_code: Option<String>,
    pub(crate) started_at: i64,
    pub(crate) updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocalMutationRecord {
    pub(crate) owner_identity_id: String,
    pub(crate) mutation_id: String,
    pub(crate) operation_id: String,
    pub(crate) mutation_type: String,
    pub(crate) aggregate_id: String,
    pub(crate) payload_json: String,
    pub(crate) status: String,
    pub(crate) attempt_count: i64,
    pub(crate) retry_at: Option<i64>,
    pub(crate) in_flight_since: Option<i64>,
    pub(crate) last_error_code: Option<String>,
    pub(crate) created_at: i64,
    pub(crate) updated_at: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SyncDiagnosticsState {
    pub(crate) last_success_at: Option<i64>,
    pub(crate) bootstrap_state: Option<String>,
    pub(crate) recovery_status: Option<String>,
    pub(crate) pending_mutation_count: u32,
    pub(crate) pending_count: u32,
    pub(crate) in_flight_count: u32,
    pub(crate) retryable_count: u32,
    pub(crate) permanent_failure_count: u32,
    pub(crate) next_retry_at: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SyncCleanupOutcome {
    pub(crate) applied_events_deleted: usize,
    pub(crate) terminal_mutations_deleted: usize,
    pub(crate) terminal_recovery_deleted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BootstrapApplyInputV2 {
    pub(crate) binding: IdentityAccountBinding,
    pub(crate) state: MessageSyncState,
    pub(crate) groups: Vec<super::groups::GroupRecord>,
    pub(crate) read_states: Vec<ReadStateApplyV2>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct DeltaApplyEventV2 {
    pub(crate) event_id: String,
    pub(crate) event_seq: String,
    pub(crate) event_type: String,
    pub(crate) messages: Vec<super::messages::MessageRecord>,
    pub(crate) groups: Vec<super::groups::GroupRecord>,
    pub(crate) thread_bindings: Vec<SyncThreadBinding>,
    pub(crate) read_states: Vec<ReadStateApplyV2>,
    pub(crate) system_notification:
        Option<crate::internal::system_notification::store::SystemNotificationApplyInput>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SyncThreadBinding {
    pub(crate) owner_identity_id: String,
    pub(crate) remote_thread_key: String,
    pub(crate) thread_kind: String,
    pub(crate) conversation_id: String,
    pub(crate) updated_at: i64,
}

pub(crate) fn load_sync_thread_binding_for_conversation(
    connection: &Connection,
    owner_identity_id: &str,
    conversation_id: &str,
    thread_kind: &str,
) -> crate::ImResult<Option<SyncThreadBinding>> {
    validate_required("owner_identity_id", owner_identity_id)?;
    validate_required("conversation_id", conversation_id)?;
    if !matches!(thread_kind, "direct" | "group") {
        return Err(sync_error(
            "SYNC_THREAD_KIND_INVALID",
            "thread_kind must be direct or group",
        ));
    }
    connection
        .query_row(
            "SELECT owner_identity_id, remote_thread_key, thread_kind,
                    conversation_id, updated_at
             FROM sync_thread_bindings
             WHERE owner_identity_id = ?1 AND conversation_id = ?2
               AND thread_kind = ?3",
            params![owner_identity_id, conversation_id, thread_kind],
            |row| {
                Ok(SyncThreadBinding {
                    owner_identity_id: row.get(0)?,
                    remote_thread_key: row.get(1)?,
                    thread_kind: row.get(2)?,
                    conversation_id: row.get(3)?,
                    updated_at: row.get(4)?,
                })
            },
        )
        .optional()
        .map_err(super::local_state_unavailable)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReadStateApplyV2 {
    pub(crate) remote_thread_key: String,
    pub(crate) thread_kind: String,
    pub(crate) read_watermark_seq: String,
    pub(crate) read_watermark_message_id: Option<String>,
    pub(crate) state_version: String,
    pub(crate) occurred_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DeltaApplyInputV2 {
    pub(crate) owner_identity_id: String,
    pub(crate) owner_did: String,
    pub(crate) account_id: String,
    pub(crate) protocol_device_id: String,
    pub(crate) device_auth_generation: String,
    pub(crate) stream_epoch: String,
    pub(crate) next_scan_seq: String,
    pub(crate) server_time: String,
    pub(crate) events: Vec<DeltaApplyEventV2>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DeltaApplyOutcomeV2 {
    pub(crate) applied_event_ids: Vec<String>,
    pub(crate) projected_message_event_ids: Vec<String>,
    pub(crate) duplicate_events: usize,
    pub(crate) backlogged_messages: usize,
    pub(crate) committed_system_notifications:
        Vec<crate::system_notifications::SystemNotificationSnapshot>,
    pub(crate) invalidation: super::sync_state::SyncDeltaInvalidation,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SnapshotApplyInputV2 {
    pub(crate) owner_identity_id: String,
    pub(crate) owner_did: String,
    pub(crate) account_id: String,
    pub(crate) protocol_device_id: String,
    pub(crate) device_auth_generation: String,
    pub(crate) expected_stream_epoch: String,
    pub(crate) expected_scan_seq: String,
    pub(crate) allow_missing_previous: bool,
    pub(crate) recovery_id_hash: String,
    pub(crate) stream_epoch: String,
    pub(crate) snapshot_scan_seq: String,
    pub(crate) server_time: String,
    pub(crate) events: Vec<DeltaApplyEventV2>,
    pub(crate) groups: Vec<super::groups::GroupRecord>,
    pub(crate) read_states: Vec<ReadStateApplyV2>,
}

pub(crate) fn create_schema(connection: &Connection) -> crate::ImResult<()> {
    connection
        .execute_batch(SYNC_V2_SCHEMA_SQL)
        .map_err(super::local_state_unavailable)?;
    connection
        .execute_batch(READ_RECOVERY_SCHEMA_SQL)
        .map_err(super::local_state_unavailable)?;
    create_installation_schema(connection)
}

pub(crate) fn create_installation_schema(connection: &Connection) -> crate::ImResult<()> {
    connection
        .execute_batch(SYNC_INSTALLATION_SCHEMA_SQL)
        .map_err(super::local_state_unavailable)
}

pub(crate) fn load_or_create_sync_client_instance_id(
    connection: &Connection,
    owner_identity_id: &str,
) -> crate::ImResult<String> {
    validate_required("owner_identity_id", owner_identity_id)?;
    if let Some(client_instance_id) = connection
        .query_row(
            "SELECT client_instance_id
             FROM sync_installation_state
             WHERE owner_identity_id = ?1",
            [owner_identity_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(super::local_state_unavailable)?
    {
        validate_required("client_instance_id", &client_instance_id)?;
        return Ok(client_instance_id);
    }
    use base64::Engine as _;
    use rand::RngCore as _;
    let mut random = [0_u8; 24];
    rand::rngs::OsRng
        .try_fill_bytes(&mut random)
        .map_err(|error| crate::ImError::Internal {
            message: format!("generate sync client installation id: {error}"),
        })?;
    let client_instance_id = format!(
        "core-installation-{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(random)
    );
    connection
        .execute(
            "INSERT INTO sync_installation_state
                 (owner_identity_id, client_instance_id, created_at)
             VALUES (?1, ?2, ?3)",
            params![owner_identity_id, client_instance_id, unix_time_i64()],
        )
        .map_err(super::local_state_unavailable)?;
    Ok(client_instance_id)
}

pub(crate) fn upsert_identity_account_binding(
    connection: &Connection,
    binding: &IdentityAccountBinding,
) -> crate::ImResult<()> {
    validate_binding(binding)?;
    if let Some((
        account_id,
        protocol_device_id,
        current_did,
        identity_generation,
        device_auth_generation,
    )) = connection
        .query_row(
            "SELECT account_id, device_id, current_did, identity_generation,
                    device_auth_generation
             FROM identity_account_bindings
             WHERE owner_identity_id = ?1",
            [&binding.owner_identity_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )
        .optional()
        .map_err(super::local_state_unavailable)?
    {
        if account_id != binding.account_id {
            return Err(crate::ImError::IdentityBindingConflict {
                detail: "owner identity is already bound to a different account".to_owned(),
            });
        }
        if protocol_device_id != binding.protocol_device_id {
            return Err(crate::ImError::IdentityBindingConflict {
                detail: "owner identity is already bound to a different protocol device".to_owned(),
            });
        }
        match compare_decimal(&binding.identity_generation, &identity_generation)? {
            std::cmp::Ordering::Less => {
                return Err(crate::ImError::IdentityBindingConflict {
                    detail: "identity generation cannot move backwards".to_owned(),
                });
            }
            std::cmp::Ordering::Equal if current_did != binding.current_did => {
                return Err(crate::ImError::IdentityBindingConflict {
                    detail: "current DID cannot change without a newer identity generation"
                        .to_owned(),
                });
            }
            _ => {}
        }
        if compare_decimal(&binding.device_auth_generation, &device_auth_generation)?
            == std::cmp::Ordering::Less
        {
            return Err(crate::ImError::IdentityBindingConflict {
                detail: "device authorization generation cannot move backwards".to_owned(),
            });
        }
    }
    if let Some(owner_identity_id) = connection
        .query_row(
            "SELECT owner_identity_id
             FROM identity_account_bindings
             WHERE account_id = ?1 AND device_id = ?2",
            params![binding.account_id, binding.protocol_device_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(super::local_state_unavailable)?
    {
        if owner_identity_id != binding.owner_identity_id {
            return Err(crate::ImError::IdentityBindingConflict {
                detail: "account device is already bound to a different local owner".to_owned(),
            });
        }
    }
    connection
        .execute(
            r#"
INSERT INTO identity_account_bindings
    (owner_identity_id, account_id, handle_scope, current_did, device_id,
     identity_generation, device_auth_generation, created_at, updated_at)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
ON CONFLICT(owner_identity_id)
DO UPDATE SET
    handle_scope = excluded.handle_scope,
    current_did = excluded.current_did,
    identity_generation = excluded.identity_generation,
    device_auth_generation = excluded.device_auth_generation,
    updated_at = excluded.updated_at"#,
            params![
                binding.owner_identity_id,
                binding.account_id,
                binding.handle_scope,
                binding.current_did,
                binding.protocol_device_id,
                binding.identity_generation,
                binding.device_auth_generation,
                binding.created_at,
                binding.updated_at,
            ],
        )
        .map_err(super::local_state_unavailable)?;
    Ok(())
}

pub(crate) fn load_identity_account_binding(
    connection: &Connection,
    owner_identity_id: &str,
) -> crate::ImResult<Option<IdentityAccountBinding>> {
    connection
        .query_row(
            r#"
SELECT owner_identity_id, account_id, handle_scope, current_did, device_id,
       identity_generation, device_auth_generation, created_at, updated_at
FROM identity_account_bindings
WHERE owner_identity_id = ?1"#,
            [owner_identity_id],
            |row| {
                Ok(IdentityAccountBinding {
                    owner_identity_id: row.get(0)?,
                    account_id: row.get(1)?,
                    handle_scope: row.get(2)?,
                    current_did: row.get(3)?,
                    protocol_device_id: row.get(4)?,
                    identity_generation: row.get(5)?,
                    device_auth_generation: row.get(6)?,
                    created_at: row.get(7)?,
                    updated_at: row.get(8)?,
                })
            },
        )
        .optional()
        .map_err(super::local_state_unavailable)
}

/// Replaces the v2 cursor only at an explicit, server-authorized bootstrap
/// boundary. Normal delta application must use `advance_message_sync_state`.
pub(crate) fn bootstrap_message_sync_state(
    connection: &Connection,
    state: &MessageSyncState,
) -> crate::ImResult<()> {
    validate_message_sync_state(state)?;
    let binding =
        load_identity_account_binding(connection, &state.owner_identity_id)?.ok_or_else(|| {
            crate::ImError::IdentityBindingConflict {
                detail: "message sync state requires an active account binding".to_owned(),
            }
        })?;
    if binding.account_id != state.account_id
        || binding.protocol_device_id != state.protocol_device_id
        || binding.device_auth_generation != state.device_auth_generation
    {
        return Err(crate::ImError::IdentityBindingConflict {
            detail: "message sync state does not match the active account binding".to_owned(),
        });
    }
    let transaction = connection
        .unchecked_transaction()
        .map_err(super::local_state_unavailable)?;
    upsert_bootstrap_state(&transaction, state)?;
    complete_active_recovery(&transaction, state)?;
    transaction.commit().map_err(super::local_state_unavailable)
}

fn upsert_bootstrap_state(
    connection: &Connection,
    state: &MessageSyncState,
) -> crate::ImResult<()> {
    connection
        .execute(
            r#"
INSERT INTO message_sync_state
    (owner_identity_id, account_id, device_id, device_auth_generation,
     stream_epoch, scan_seq, bootstrap_state, last_server_time, last_success_at,
     last_error_code, metadata_json, updated_at)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
ON CONFLICT(owner_identity_id)
DO UPDATE SET
    account_id = excluded.account_id,
    device_id = excluded.device_id,
    device_auth_generation = excluded.device_auth_generation,
    stream_epoch = excluded.stream_epoch,
    scan_seq = excluded.scan_seq,
    bootstrap_state = excluded.bootstrap_state,
    last_server_time = excluded.last_server_time,
    last_success_at = excluded.last_success_at,
    last_error_code = excluded.last_error_code,
    metadata_json = excluded.metadata_json,
    updated_at = excluded.updated_at"#,
            params![
                state.owner_identity_id,
                state.account_id,
                state.protocol_device_id,
                state.device_auth_generation,
                state.stream_epoch,
                state.scan_seq,
                state.bootstrap_state,
                state.last_server_time,
                state.last_success_at,
                state.last_error_code,
                state.metadata_json,
                state.updated_at,
            ],
        )
        .map_err(super::local_state_unavailable)?;
    Ok(())
}

fn complete_active_recovery(
    connection: &Connection,
    state: &MessageSyncState,
) -> crate::ImResult<()> {
    // An explicit server-authorized bootstrap supersedes any interrupted
    // owner-scoped recovery, including one anchored in an older epoch or
    // device authorization generation. Keep the terminal diagnostic row, but
    // ensure pruning and restart recovery can no longer treat its old anchor
    // as active.
    connection
        .execute(
            r#"
UPDATE sync_recovery_state
SET status = 'completed',
    snapshot_scan_seq = ?2,
    last_error_code = NULL,
    updated_at = ?3
WHERE owner_identity_id = ?1
  AND status IN ('recovering', 'downloading', 'applying', 'retryable')"#,
            params![state.owner_identity_id, state.scan_seq, state.updated_at],
        )
        .map_err(super::local_state_unavailable)?;
    Ok(())
}

pub(crate) fn apply_bootstrap_v2(
    connection: &Connection,
    input: BootstrapApplyInputV2,
) -> crate::ImResult<()> {
    validate_binding(&input.binding)?;
    validate_message_sync_state(&input.state)?;
    if input.binding.owner_identity_id != input.state.owner_identity_id
        || input.binding.account_id != input.state.account_id
        || input.binding.protocol_device_id != input.state.protocol_device_id
        || input.binding.device_auth_generation != input.state.device_auth_generation
    {
        return Err(crate::ImError::IdentityBindingConflict {
            detail: "sync bootstrap state does not match its active account binding".to_owned(),
        });
    }
    let transaction = connection
        .unchecked_transaction()
        .map_err(super::local_state_unavailable)?;
    upsert_identity_account_binding(&transaction, &input.binding)?;
    for group in input.groups {
        validate_group_owner(&group, &input.binding.owner_identity_id)?;
        super::groups::upsert_group(&transaction, group)?;
    }
    for read_state in input.read_states {
        apply_remote_read_state(
            &transaction,
            &input.binding.owner_identity_id,
            &input.binding.current_did,
            &read_state,
        )?;
    }
    upsert_bootstrap_state(&transaction, &input.state)?;
    complete_active_recovery(&transaction, &input.state)?;
    transaction.commit().map_err(super::local_state_unavailable)
}

pub(crate) fn apply_delta_v2(
    connection: &Connection,
    input: DeltaApplyInputV2,
) -> crate::ImResult<DeltaApplyOutcomeV2> {
    validate_required("owner_identity_id", &input.owner_identity_id)?;
    validate_required("owner_did", &input.owner_did)?;
    validate_required("account_id", &input.account_id)?;
    validate_required("protocol_device_id", &input.protocol_device_id)?;
    validate_positive_decimal("device_auth_generation", &input.device_auth_generation)?;
    validate_positive_decimal("stream_epoch", &input.stream_epoch)?;
    validate_decimal("next_scan_seq", &input.next_scan_seq)?;
    validate_required("server_time", &input.server_time)?;

    let mut event_ids = BTreeSet::new();
    let mut event_seqs = BTreeSet::new();
    for event in &input.events {
        validate_required("event_id", &event.event_id)?;
        validate_positive_decimal("event_seq", &event.event_seq)?;
        validate_required("event_type", &event.event_type)?;
        if !event_ids.insert(event.event_id.as_str()) {
            return Err(sync_error(
                "SYNC_INVALID_PAGE",
                "delta apply contains a duplicate event_id",
            ));
        }
        if !event_seqs.insert(event.event_seq.as_str()) {
            return Err(sync_error(
                "SYNC_INVALID_PAGE",
                "delta apply contains a duplicate event_seq",
            ));
        }
        if compare_decimal(&event.event_seq, &input.next_scan_seq)? == std::cmp::Ordering::Greater {
            return Err(sync_error(
                "SYNC_INVALID_PAGE",
                "visible event is ahead of next scan cursor",
            ));
        }
    }

    let transaction = connection
        .unchecked_transaction()
        .map_err(super::local_state_unavailable)?;
    let binding = load_identity_account_binding(&transaction, &input.owner_identity_id)?
        .ok_or_else(|| crate::ImError::IdentityBindingConflict {
            detail: "v2 delta requires an active account binding".to_owned(),
        })?;
    if binding.account_id != input.account_id
        || binding.protocol_device_id != input.protocol_device_id
        || binding.device_auth_generation != input.device_auth_generation
    {
        return Err(crate::ImError::IdentityBindingConflict {
            detail: "v2 delta binding does not match the active account device".to_owned(),
        });
    }
    let current = match load_message_sync_state(&transaction, &input.owner_identity_id)? {
        MessageSyncStateAccess::Ready(state) => state,
        MessageSyncStateAccess::BootstrapRequired(_) => {
            return Err(sync_error(
                "SYNC_BOOTSTRAP_REQUIRED",
                "v2 delta cannot apply before bootstrap",
            ));
        }
    };
    if current.stream_epoch != input.stream_epoch {
        return Err(sync_error(
            "SYNC_CURSOR_EPOCH_MISMATCH",
            "v2 delta stream epoch does not match the local cursor",
        ));
    }
    if compare_decimal(&input.next_scan_seq, &current.scan_seq)? == std::cmp::Ordering::Less {
        return Err(sync_error(
            "SYNC_CURSOR_REGRESSION",
            "v2 delta next cursor is behind the local cursor",
        ));
    }

    let mut events = input.events;
    events.sort_by(|left, right| {
        decimal_order(&left.event_seq, &right.event_seq)
            .then_with(|| left.event_id.cmp(&right.event_id))
    });
    let now = unix_time_i64();
    let mut applied_event_ids = Vec::new();
    let mut projected_message_event_ids = Vec::new();
    let mut duplicate_events = 0usize;
    let mut messages = Vec::new();
    let mut groups = Vec::new();
    let mut thread_bindings = Vec::new();
    let mut read_states = Vec::new();
    let mut backlogged_messages = 0usize;
    let mut committed_system_notifications = Vec::new();
    for mut event in events {
        let event_id = event.event_id.clone();
        let inserted = record_applied_event(
            &transaction,
            &AppliedEventReceipt {
                owner_identity_id: input.owner_identity_id.clone(),
                event_id: event.event_id.clone(),
                stream_epoch: input.stream_epoch.clone(),
                event_seq: event.event_seq.clone(),
                applied_at: now,
            },
        )?;
        if !inserted {
            duplicate_events = duplicate_events.saturating_add(1);
            continue;
        }
        if let Some(notification) = event.system_notification.take() {
            if notification.owner_identity_id != input.owner_identity_id
                || notification.owner_did != input.owner_did
                || notification.protocol_device_id != input.protocol_device_id
            {
                return Err(crate::ImError::IdentityBindingConflict {
                    detail:
                        "system notification projection does not match the active account device"
                            .to_owned(),
                });
            }
            if let crate::internal::system_notification::store::SystemNotificationApplyOutcome::Applied(
                snapshot,
            ) = crate::internal::system_notification::store::apply_transaction(
                &transaction,
                &notification,
            )? {
                committed_system_notifications.push(snapshot);
            }
        }
        let mut projected_message = false;
        let mut canonical_conversation_ids = BTreeSet::new();
        let had_messages = !event.messages.is_empty();
        let message_thread_binding =
            message_event_thread_binding(&event, &input.owner_identity_id)?;
        for message in event.messages {
            validate_message_owner(&message, &input.owner_identity_id)?;
            match super::inbound_resolution_backlog::canonicalize_inbound_message(
                &transaction,
                message.clone(),
            ) {
                Ok(message) => {
                    canonical_conversation_ids.insert(message.conversation_id.clone());
                    messages.push(message);
                    projected_message = true;
                }
                Err(error) if super::inbound_resolution_backlog::is_resolution_error(&error) => {
                    super::inbound_resolution_backlog::store_with_thread_binding(
                        &transaction,
                        super::inbound_resolution_backlog::BacklogSource {
                            event_id: &event.event_id,
                            event_seq: &event.event_seq,
                            event_type: &event.event_type,
                        },
                        &message,
                        &error,
                        message_thread_binding.as_ref(),
                    )?;
                    backlogged_messages = backlogged_messages.saturating_add(1);
                }
                Err(error) => return Err(error),
            }
        }
        canonicalize_message_event_thread_bindings(
            &mut event.thread_bindings,
            &event.event_type,
            &input.owner_identity_id,
            &canonical_conversation_ids,
        )?;
        for group in event.groups {
            validate_group_owner(&group, &input.owner_identity_id)?;
            if group_state_is_stale(&transaction, &group)? {
                continue;
            }
            groups.push(group);
        }
        if projected_message || !had_messages {
            thread_bindings.extend(event.thread_bindings);
        }
        read_states.extend(event.read_states);
        applied_event_ids.push(event_id.clone());
        if projected_message {
            projected_message_event_ids.push(event_id);
        }
    }

    for binding in thread_bindings {
        upsert_sync_thread_binding(&transaction, &binding)?;
    }
    let mut invalidation = v2_invalidation(
        &transaction,
        &input.owner_identity_id,
        &input.owner_did,
        &input.next_scan_seq,
        &messages,
        &groups,
        &read_states,
    )?;
    if !messages.is_empty() {
        let touched = super::messages::upsert_messages_with_touched(&transaction, &messages)?;
        let mut conversation_ids = invalidation
            .conversation_ids
            .into_iter()
            .collect::<BTreeSet<_>>();
        let mut thread_ids = invalidation.thread_ids.into_iter().collect::<BTreeSet<_>>();
        for (_, conversation_id) in touched {
            if !conversation_id.trim().is_empty() {
                conversation_ids.insert(conversation_id.clone());
                thread_ids.insert(conversation_id);
            }
        }
        invalidation.conversation_ids = conversation_ids.into_iter().collect();
        invalidation.thread_ids = thread_ids.into_iter().collect();
    }
    for group in groups {
        super::groups::upsert_group(&transaction, group)?;
    }
    for read_state in read_states {
        apply_remote_read_state(
            &transaction,
            &input.owner_identity_id,
            &input.owner_did,
            &read_state,
        )?;
    }

    let next_state = MessageSyncState {
        owner_identity_id: input.owner_identity_id.clone(),
        account_id: input.account_id,
        protocol_device_id: input.protocol_device_id,
        device_auth_generation: input.device_auth_generation,
        stream_epoch: input.stream_epoch,
        scan_seq: input.next_scan_seq,
        bootstrap_state: "active".to_owned(),
        last_server_time: Some(input.server_time),
        last_success_at: Some(now),
        last_error_code: None,
        metadata_json: None,
        updated_at: now,
    };
    match advance_message_sync_state(&transaction, &next_state)? {
        MessageSyncStateAccess::Ready(_) => {}
        MessageSyncStateAccess::BootstrapRequired(_) => {
            return Err(sync_error(
                "SYNC_BOOTSTRAP_REQUIRED",
                "v2 cursor was fenced while applying the page",
            ));
        }
    }
    transaction
        .commit()
        .map_err(super::local_state_unavailable)?;
    Ok(DeltaApplyOutcomeV2 {
        applied_event_ids,
        projected_message_event_ids,
        duplicate_events,
        backlogged_messages,
        committed_system_notifications,
        invalidation,
    })
}

pub(crate) fn apply_snapshot_v2(
    connection: &Connection,
    input: SnapshotApplyInputV2,
) -> crate::ImResult<DeltaApplyOutcomeV2> {
    validate_required("owner_identity_id", &input.owner_identity_id)?;
    validate_required("owner_did", &input.owner_did)?;
    validate_required("account_id", &input.account_id)?;
    validate_required("protocol_device_id", &input.protocol_device_id)?;
    validate_positive_decimal("device_auth_generation", &input.device_auth_generation)?;
    validate_positive_decimal("expected_stream_epoch", &input.expected_stream_epoch)?;
    validate_decimal("expected_scan_seq", &input.expected_scan_seq)?;
    validate_required("recovery_id_hash", &input.recovery_id_hash)?;
    validate_positive_decimal("stream_epoch", &input.stream_epoch)?;
    validate_decimal("snapshot_scan_seq", &input.snapshot_scan_seq)?;
    validate_required("server_time", &input.server_time)?;

    let transaction = connection
        .unchecked_transaction()
        .map_err(super::local_state_unavailable)?;
    let binding = load_identity_account_binding(&transaction, &input.owner_identity_id)?
        .ok_or_else(|| crate::ImError::IdentityBindingConflict {
            detail: "snapshot requires an active account binding".to_owned(),
        })?;
    if binding.account_id != input.account_id
        || binding.protocol_device_id != input.protocol_device_id
        || binding.device_auth_generation != input.device_auth_generation
    {
        return Err(crate::ImError::IdentityBindingConflict {
            detail: "snapshot binding does not match the active account device".to_owned(),
        });
    }
    match load_message_sync_state_row(&transaction, &input.owner_identity_id)? {
        Some(current)
            if current.stream_epoch == input.expected_stream_epoch
                && current.scan_seq == input.expected_scan_seq => {}
        None if input.allow_missing_previous && input.expected_scan_seq == "0" => {}
        _ => {
            return Err(sync_error(
                "SYNC_SNAPSHOT_CAS_FAILED",
                "snapshot cursor changed while recovery was in flight",
            ))
        }
    }
    let recovery = load_recovery_state(&transaction, &input.owner_identity_id)?
        .ok_or_else(|| sync_error("SYNC_SNAPSHOT_CAS_FAILED", "snapshot recovery is missing"))?;
    if recovery.recovery_id_hash.as_deref() != Some(input.recovery_id_hash.as_str())
        || recovery.snapshot_scan_seq.as_deref() != Some(input.snapshot_scan_seq.as_str())
        || recovery.requested_from_epoch != input.expected_stream_epoch
        || recovery.requested_from_seq != input.expected_scan_seq
        || recovery.status != "applying"
    {
        return Err(sync_error(
            "SYNC_SNAPSHOT_CAS_FAILED",
            "snapshot recovery authorization changed before commit",
        ));
    }

    let now = unix_time_i64();
    let mut events = input.events;
    events.sort_by(|left, right| {
        decimal_order(&left.event_seq, &right.event_seq)
            .then_with(|| left.event_id.cmp(&right.event_id))
    });
    let mut applied_event_ids = Vec::new();
    let mut projected_message_event_ids = Vec::new();
    let mut duplicate_events = 0usize;
    let mut messages = Vec::new();
    let mut groups = input.groups;
    let mut thread_bindings = Vec::new();
    let mut read_states = input.read_states;
    let mut backlogged_messages = 0usize;
    let mut committed_system_notifications = Vec::new();
    for mut event in events {
        let event_id = event.event_id.clone();
        if compare_decimal(&event.event_seq, &input.snapshot_scan_seq)?
            == std::cmp::Ordering::Greater
        {
            return Err(sync_error(
                "SYNC_INVALID_SNAPSHOT",
                "snapshot event is ahead of its anchor",
            ));
        }
        let inserted = record_applied_event(
            &transaction,
            &AppliedEventReceipt {
                owner_identity_id: input.owner_identity_id.clone(),
                event_id: event.event_id.clone(),
                stream_epoch: input.stream_epoch.clone(),
                event_seq: event.event_seq.clone(),
                applied_at: now,
            },
        )?;
        if !inserted {
            duplicate_events = duplicate_events.saturating_add(1);
            continue;
        }
        if let Some(notification) = event.system_notification.take() {
            if notification.owner_identity_id != input.owner_identity_id
                || notification.owner_did != input.owner_did
                || notification.protocol_device_id != input.protocol_device_id
            {
                return Err(crate::ImError::IdentityBindingConflict {
                    detail:
                        "system notification projection does not match the active account device"
                            .to_owned(),
                });
            }
            if let crate::internal::system_notification::store::SystemNotificationApplyOutcome::Applied(
                snapshot,
            ) = crate::internal::system_notification::store::apply_transaction(
                &transaction,
                &notification,
            )? {
                committed_system_notifications.push(snapshot);
            }
        }
        let mut canonical_conversation_ids = BTreeSet::new();
        let had_messages = !event.messages.is_empty();
        let message_thread_binding =
            message_event_thread_binding(&event, &input.owner_identity_id)?;
        let mut projected_message = false;
        for message in event.messages {
            validate_message_owner(&message, &input.owner_identity_id)?;
            match super::inbound_resolution_backlog::canonicalize_inbound_message(
                &transaction,
                message.clone(),
            ) {
                Ok(message) => {
                    canonical_conversation_ids.insert(message.conversation_id.clone());
                    messages.push(message);
                    projected_message = true;
                }
                Err(error) if super::inbound_resolution_backlog::is_resolution_error(&error) => {
                    super::inbound_resolution_backlog::store_with_thread_binding(
                        &transaction,
                        super::inbound_resolution_backlog::BacklogSource {
                            event_id: &event.event_id,
                            event_seq: &event.event_seq,
                            event_type: &event.event_type,
                        },
                        &message,
                        &error,
                        message_thread_binding.as_ref(),
                    )?;
                    backlogged_messages = backlogged_messages.saturating_add(1);
                }
                Err(error) => return Err(error),
            }
        }
        canonicalize_message_event_thread_bindings(
            &mut event.thread_bindings,
            &event.event_type,
            &input.owner_identity_id,
            &canonical_conversation_ids,
        )?;
        groups.extend(event.groups);
        if projected_message || !had_messages {
            thread_bindings.extend(event.thread_bindings);
        }
        read_states.extend(event.read_states);
        applied_event_ids.push(event_id.clone());
        if projected_message {
            projected_message_event_ids.push(event_id);
        }
    }

    for binding in thread_bindings {
        upsert_sync_thread_binding(&transaction, &binding)?;
    }
    let mut invalidation = v2_invalidation(
        &transaction,
        &input.owner_identity_id,
        &input.owner_did,
        &input.snapshot_scan_seq,
        &messages,
        &groups,
        &read_states,
    )?;
    if !messages.is_empty() {
        let touched = super::messages::upsert_messages_with_touched(&transaction, &messages)?;
        let mut conversation_ids = invalidation
            .conversation_ids
            .into_iter()
            .collect::<BTreeSet<_>>();
        let mut thread_ids = invalidation.thread_ids.into_iter().collect::<BTreeSet<_>>();
        for (_, conversation_id) in touched {
            if !conversation_id.trim().is_empty() {
                conversation_ids.insert(conversation_id.clone());
                thread_ids.insert(conversation_id);
            }
        }
        invalidation.conversation_ids = conversation_ids.into_iter().collect();
        invalidation.thread_ids = thread_ids.into_iter().collect();
    }
    for group in groups {
        validate_group_owner(&group, &input.owner_identity_id)?;
        if !group_state_is_stale(&transaction, &group)? {
            super::groups::upsert_group(&transaction, group)?;
        }
    }
    for read_state in read_states {
        apply_remote_read_state(
            &transaction,
            &input.owner_identity_id,
            &input.owner_did,
            &read_state,
        )?;
    }

    let next_state = MessageSyncState {
        owner_identity_id: input.owner_identity_id,
        account_id: input.account_id,
        protocol_device_id: input.protocol_device_id,
        device_auth_generation: input.device_auth_generation,
        stream_epoch: input.stream_epoch,
        scan_seq: input.snapshot_scan_seq,
        bootstrap_state: "active".to_owned(),
        last_server_time: Some(input.server_time),
        last_success_at: Some(now),
        last_error_code: None,
        metadata_json: Some("{\"mode\":\"compact_recovery\"}".to_owned()),
        updated_at: now,
    };
    // Snapshot is the one server-authorized boundary allowed to replace an
    // epoch/cursor while preserving existing local message rows.
    upsert_bootstrap_state(&transaction, &next_state)?;
    complete_active_recovery(&transaction, &next_state)?;
    transaction
        .commit()
        .map_err(super::local_state_unavailable)?;
    Ok(DeltaApplyOutcomeV2 {
        applied_event_ids,
        projected_message_event_ids,
        duplicate_events,
        backlogged_messages,
        committed_system_notifications,
        invalidation,
    })
}

pub(crate) fn load_message_sync_state(
    connection: &Connection,
    owner_identity_id: &str,
) -> crate::ImResult<MessageSyncStateAccess> {
    let binding =
        load_identity_account_binding(connection, owner_identity_id)?.ok_or_else(|| {
            crate::ImError::IdentityBindingConflict {
                detail: "message sync state requires an active account binding".to_owned(),
            }
        })?;
    let Some(state) = load_message_sync_state_row(connection, owner_identity_id)? else {
        return Ok(MessageSyncStateAccess::BootstrapRequired(
            message_sync_bootstrap_fence(
                &binding,
                None,
                None,
                MessageSyncBootstrapReason::MissingState,
            ),
        ));
    };
    if state.account_id != binding.account_id
        || state.protocol_device_id != binding.protocol_device_id
    {
        return Err(crate::ImError::IdentityBindingConflict {
            detail: "stored message sync state belongs to a different account device".to_owned(),
        });
    }
    if state.device_auth_generation != binding.device_auth_generation {
        return Ok(MessageSyncStateAccess::BootstrapRequired(
            message_sync_bootstrap_fence(
                &binding,
                Some(&state),
                None,
                MessageSyncBootstrapReason::DeviceAuthGenerationChanged,
            ),
        ));
    }
    Ok(MessageSyncStateAccess::Ready(state))
}

fn load_message_sync_state_row(
    connection: &Connection,
    owner_identity_id: &str,
) -> crate::ImResult<Option<MessageSyncState>> {
    connection
        .query_row(
            r#"
SELECT owner_identity_id, account_id, device_id, device_auth_generation,
       stream_epoch, scan_seq, bootstrap_state, last_server_time, last_success_at,
       last_error_code, metadata_json, updated_at
FROM message_sync_state
WHERE owner_identity_id = ?1"#,
            [owner_identity_id],
            |row| {
                Ok(MessageSyncState {
                    owner_identity_id: row.get(0)?,
                    account_id: row.get(1)?,
                    protocol_device_id: row.get(2)?,
                    device_auth_generation: row.get(3)?,
                    stream_epoch: row.get(4)?,
                    scan_seq: row.get(5)?,
                    bootstrap_state: row.get(6)?,
                    last_server_time: row.get(7)?,
                    last_success_at: row.get(8)?,
                    last_error_code: row.get(9)?,
                    metadata_json: row.get(10)?,
                    updated_at: row.get(11)?,
                })
            },
        )
        .optional()
        .map_err(super::local_state_unavailable)
}

/// Advances an already bootstrapped cursor without permitting a generation or
/// epoch boundary to be crossed implicitly.
pub(crate) fn advance_message_sync_state(
    connection: &Connection,
    next: &MessageSyncState,
) -> crate::ImResult<MessageSyncStateAccess> {
    validate_message_sync_state(next)?;
    let binding =
        load_identity_account_binding(connection, &next.owner_identity_id)?.ok_or_else(|| {
            crate::ImError::IdentityBindingConflict {
                detail: "message sync state requires an active account binding".to_owned(),
            }
        })?;
    if binding.account_id != next.account_id
        || binding.protocol_device_id != next.protocol_device_id
    {
        return Err(crate::ImError::IdentityBindingConflict {
            detail: "message sync advance targets a different account device".to_owned(),
        });
    }
    let current = match load_message_sync_state(connection, &next.owner_identity_id)? {
        MessageSyncStateAccess::Ready(current) => current,
        fence @ MessageSyncStateAccess::BootstrapRequired(_) => return Ok(fence),
    };
    if binding.device_auth_generation != next.device_auth_generation {
        return Err(crate::ImError::IdentityBindingConflict {
            detail: "message sync advance uses a non-active device authorization generation"
                .to_owned(),
        });
    }
    if current.stream_epoch != next.stream_epoch {
        return Ok(MessageSyncStateAccess::BootstrapRequired(
            message_sync_bootstrap_fence(
                &binding,
                Some(&current),
                Some(&next.stream_epoch),
                MessageSyncBootstrapReason::StreamEpochChanged,
            ),
        ));
    }
    if compare_decimal(&next.scan_seq, &current.scan_seq)? == std::cmp::Ordering::Less {
        return Err(crate::ImError::IdentityBindingConflict {
            detail: "message sync scan sequence cannot move backwards".to_owned(),
        });
    }
    let updated = connection
        .execute(
            r#"
UPDATE message_sync_state
SET scan_seq = ?2,
    bootstrap_state = ?3,
    last_server_time = ?4,
    last_success_at = ?5,
    last_error_code = ?6,
    metadata_json = ?7,
    updated_at = ?8
WHERE owner_identity_id = ?1
  AND account_id = ?9
  AND device_id = ?10
  AND device_auth_generation = ?11
  AND stream_epoch = ?12"#,
            params![
                next.owner_identity_id,
                next.scan_seq,
                next.bootstrap_state,
                next.last_server_time,
                next.last_success_at,
                next.last_error_code,
                next.metadata_json,
                next.updated_at,
                next.account_id,
                next.protocol_device_id,
                next.device_auth_generation,
                next.stream_epoch,
            ],
        )
        .map_err(super::local_state_unavailable)?;
    if updated != 1 {
        return Err(crate::ImError::IdentityBindingConflict {
            detail: "message sync cursor changed while it was being advanced".to_owned(),
        });
    }
    Ok(MessageSyncStateAccess::Ready(next.clone()))
}

fn message_sync_bootstrap_fence(
    binding: &IdentityAccountBinding,
    stored: Option<&MessageSyncState>,
    requested_stream_epoch: Option<&str>,
    reason: MessageSyncBootstrapReason,
) -> MessageSyncBootstrapFence {
    MessageSyncBootstrapFence {
        owner_identity_id: binding.owner_identity_id.clone(),
        account_id: binding.account_id.clone(),
        protocol_device_id: binding.protocol_device_id.clone(),
        active_device_auth_generation: binding.device_auth_generation.clone(),
        stored_device_auth_generation: stored.map(|state| state.device_auth_generation.clone()),
        stored_stream_epoch: stored.map(|state| state.stream_epoch.clone()),
        requested_stream_epoch: requested_stream_epoch.map(str::to_owned),
        reason,
    }
}

pub(crate) fn record_applied_event(
    connection: &Connection,
    receipt: &AppliedEventReceipt,
) -> crate::ImResult<bool> {
    validate_required("owner_identity_id", &receipt.owner_identity_id)?;
    validate_required("event_id", &receipt.event_id)?;
    validate_positive_decimal("stream_epoch", &receipt.stream_epoch)?;
    validate_positive_decimal("event_seq", &receipt.event_seq)?;
    if let Some((stream_epoch, event_seq)) = connection
        .query_row(
            "SELECT stream_epoch, event_seq
             FROM sync_applied_events
             WHERE owner_identity_id = ?1 AND event_id = ?2",
            params![receipt.owner_identity_id, receipt.event_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(super::local_state_unavailable)?
    {
        if stream_epoch == receipt.stream_epoch && event_seq == receipt.event_seq {
            return Ok(false);
        }
        return Err(crate::ImError::IdentityBindingConflict {
            detail: "sync event id was already applied at a different stream position".to_owned(),
        });
    }
    connection
        .execute(
            "INSERT INTO sync_applied_events
                 (owner_identity_id, event_id, stream_epoch, event_seq, applied_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                receipt.owner_identity_id,
                receipt.event_id,
                receipt.stream_epoch,
                receipt.event_seq,
                receipt.applied_at,
            ],
        )
        .map_err(super::local_state_unavailable)?;
    Ok(true)
}

pub(crate) fn prune_applied_events(
    connection: &Connection,
    owner_identity_id: &str,
    current_stream_epoch: &str,
    current_scan_seq: &str,
    max_delete: u32,
) -> crate::ImResult<usize> {
    validate_required("owner_identity_id", owner_identity_id)?;
    validate_positive_decimal("stream_epoch", current_stream_epoch)?;
    validate_decimal("scan_seq", current_scan_seq)?;
    if max_delete == 0 {
        return Ok(0);
    }
    let count = connection
        .query_row(
            "SELECT COUNT(*) FROM sync_applied_events WHERE owner_identity_id = ?1",
            [owner_identity_id],
            |row| row.get::<_, i64>(0),
        )
        .map_err(super::local_state_unavailable)?;
    if count <= APPLIED_EVENT_MIN_RECEIPTS_PER_OWNER {
        return Ok(0);
    }
    let Some(mut safe_seq) = subtract_small_decimal(current_scan_seq, APPLIED_EVENT_SAFETY_WINDOW)?
    else {
        return Ok(0);
    };
    let mut safe_epoch = current_stream_epoch.to_owned();
    if let Some(recovery) = load_recovery_state(connection, owner_identity_id)? {
        if !matches!(recovery.status.as_str(), "completed" | "permanent_failure")
            && compare_stream_position(
                &recovery.requested_from_epoch,
                &recovery.requested_from_seq,
                &safe_epoch,
                &safe_seq,
            )? == std::cmp::Ordering::Less
        {
            safe_epoch = recovery.requested_from_epoch;
            safe_seq = recovery.requested_from_seq;
        }
    }
    let delete_limit = i64::from(max_delete).min(count - APPLIED_EVENT_MIN_RECEIPTS_PER_OWNER);
    let mut statement = connection
        .prepare(
            r#"
SELECT event_id
FROM sync_applied_events
WHERE owner_identity_id = ?1
  AND (
      length(stream_epoch) < length(?2)
      OR (length(stream_epoch) = length(?2) AND stream_epoch < ?2)
      OR (
          stream_epoch = ?2
          AND (
              length(event_seq) < length(?3)
              OR (length(event_seq) = length(?3) AND event_seq < ?3)
          )
      )
  )
  AND event_id NOT IN (
      SELECT event_id
      FROM sync_applied_events
      WHERE owner_identity_id = ?1
      ORDER BY length(stream_epoch) DESC, stream_epoch DESC,
               length(event_seq) DESC, event_seq DESC
      LIMIT 10000
  )
ORDER BY length(stream_epoch), stream_epoch, length(event_seq), event_seq
LIMIT ?4"#,
        )
        .map_err(super::local_state_unavailable)?;
    let rows = statement
        .query_map(
            params![owner_identity_id, safe_epoch, safe_seq, delete_limit],
            |row| row.get::<_, String>(0),
        )
        .map_err(super::local_state_unavailable)?;
    let mut event_ids = Vec::new();
    for row in rows {
        event_ids.push(row.map_err(super::local_state_unavailable)?);
    }
    drop(statement);
    for event_id in &event_ids {
        connection
            .execute(
                "DELETE FROM sync_applied_events
                 WHERE owner_identity_id = ?1 AND event_id = ?2",
                params![owner_identity_id, event_id],
            )
            .map_err(super::local_state_unavailable)?;
    }
    Ok(event_ids.len())
}

pub(crate) fn load_sync_diagnostics(
    connection: &Connection,
    owner_identity_id: &str,
) -> crate::ImResult<SyncDiagnosticsState> {
    validate_required("owner_identity_id", owner_identity_id)?;
    let sync_state = connection
        .query_row(
            "SELECT last_success_at, bootstrap_state
             FROM message_sync_state
             WHERE owner_identity_id = ?1",
            [owner_identity_id],
            |row| Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(super::local_state_unavailable)?;
    let recovery_status = connection
        .query_row(
            "SELECT status FROM sync_recovery_state WHERE owner_identity_id = ?1",
            [owner_identity_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(super::local_state_unavailable)?;
    let (pending_count, in_flight_count, retryable_count, permanent_failure_count, next_retry_at) =
        connection
            .query_row(
                "SELECT
                    SUM(CASE WHEN status = 'pending' THEN 1 ELSE 0 END),
                    SUM(CASE WHEN status = 'in_flight' THEN 1 ELSE 0 END),
                    SUM(CASE WHEN status = 'retryable' THEN 1 ELSE 0 END),
                    SUM(CASE WHEN status = 'permanent_failure' THEN 1 ELSE 0 END),
                    MIN(CASE WHEN status = 'retryable' THEN retry_at ELSE NULL END)
                 FROM local_mutation_outbox
                 WHERE owner_identity_id = ?1",
                [owner_identity_id],
                |row| {
                    Ok((
                        row.get::<_, Option<i64>>(0)?.unwrap_or(0),
                        row.get::<_, Option<i64>>(1)?.unwrap_or(0),
                        row.get::<_, Option<i64>>(2)?.unwrap_or(0),
                        row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                        row.get::<_, Option<i64>>(4)?,
                    ))
                },
            )
            .map_err(super::local_state_unavailable)?;
    let pending_mutation_count = pending_count
        .saturating_add(in_flight_count)
        .saturating_add(retryable_count);
    Ok(SyncDiagnosticsState {
        last_success_at: sync_state.as_ref().and_then(|state| state.0),
        bootstrap_state: sync_state.map(|state| state.1),
        recovery_status,
        pending_mutation_count: u32::try_from(pending_mutation_count).unwrap_or(u32::MAX),
        pending_count: u32::try_from(pending_count).unwrap_or(u32::MAX),
        in_flight_count: u32::try_from(in_flight_count).unwrap_or(u32::MAX),
        retryable_count: u32::try_from(retryable_count).unwrap_or(u32::MAX),
        permanent_failure_count: u32::try_from(permanent_failure_count).unwrap_or(u32::MAX),
        next_retry_at,
    })
}

pub(crate) fn cleanup_terminal_sync_state(
    connection: &Connection,
    owner_identity_id: &str,
    current_stream_epoch: &str,
    current_scan_seq: &str,
    max_delete: u32,
    now: i64,
) -> crate::ImResult<SyncCleanupOutcome> {
    validate_required("owner_identity_id", owner_identity_id)?;
    validate_positive_decimal("stream_epoch", current_stream_epoch)?;
    validate_decimal("scan_seq", current_scan_seq)?;
    if max_delete == 0 {
        return Ok(SyncCleanupOutcome::default());
    }
    let retention_cutoff = now.saturating_sub(TERMINAL_SYNC_STATE_RETENTION_SECONDS);
    let transaction = connection
        .unchecked_transaction()
        .map_err(super::local_state_unavailable)?;
    let applied_events_deleted = prune_applied_events(
        &transaction,
        owner_identity_id,
        current_stream_epoch,
        current_scan_seq,
        max_delete,
    )?;
    let remaining_after_receipts =
        max_delete.saturating_sub(u32::try_from(applied_events_deleted).unwrap_or(u32::MAX));
    let terminal_mutations_deleted = transaction
        .execute(
            "DELETE FROM local_mutation_outbox
             WHERE rowid IN (
                 SELECT rowid
                 FROM local_mutation_outbox
                 WHERE owner_identity_id = ?1
                   AND status IN ('committed', 'permanent_failure')
                   AND updated_at <= ?2
                 ORDER BY updated_at, mutation_id
                 LIMIT ?3
             )",
            params![
                owner_identity_id,
                retention_cutoff,
                i64::from(remaining_after_receipts)
            ],
        )
        .map_err(super::local_state_unavailable)?;
    let has_recovery_budget = usize::try_from(max_delete)
        .map(|limit| applied_events_deleted.saturating_add(terminal_mutations_deleted) < limit)
        .unwrap_or(false);
    let terminal_recovery_deleted =
        if let Some(recovery) = load_recovery_state(&transaction, owner_identity_id)? {
            let anchor_is_covered =
                matches!(recovery.status.as_str(), "completed" | "permanent_failure")
                    && recovery.updated_at <= retention_cutoff
                    && compare_stream_position(
                        &recovery.requested_from_epoch,
                        recovery
                            .snapshot_scan_seq
                            .as_deref()
                            .unwrap_or(&recovery.requested_from_seq),
                        current_stream_epoch,
                        current_scan_seq,
                    )? != std::cmp::Ordering::Greater;
            if has_recovery_budget && anchor_is_covered {
                transaction
                    .execute(
                        "DELETE FROM sync_recovery_state
                         WHERE owner_identity_id = ?1
                           AND status IN ('completed', 'permanent_failure')",
                        [owner_identity_id],
                    )
                    .map_err(super::local_state_unavailable)?
                    == 1
            } else {
                false
            }
        } else {
            false
        };
    transaction
        .commit()
        .map_err(super::local_state_unavailable)?;
    Ok(SyncCleanupOutcome {
        applied_events_deleted,
        terminal_mutations_deleted,
        terminal_recovery_deleted,
    })
}

pub(crate) fn upsert_recovery_state(
    connection: &Connection,
    state: &RecoveryState,
) -> crate::ImResult<()> {
    validate_required("owner_identity_id", &state.owner_identity_id)?;
    validate_recovery_mode(&state.mode)?;
    validate_recovery_status(&state.status)?;
    validate_positive_decimal("requested_from_epoch", &state.requested_from_epoch)?;
    validate_decimal("requested_from_seq", &state.requested_from_seq)?;
    if let Some(snapshot_scan_seq) = state.snapshot_scan_seq.as_deref() {
        validate_decimal("snapshot_scan_seq", snapshot_scan_seq)?;
    }
    if state.retry_count < 0 {
        return Err(crate::ImError::invalid_input(
            Some("retry_count".to_owned()),
            "retry_count must not be negative",
        ));
    }
    connection
        .execute(
            r#"
INSERT INTO sync_recovery_state
    (owner_identity_id, mode, requested_from_epoch, requested_from_seq,
     recovery_id_hash, snapshot_scan_seq, status, retry_count, last_error_code,
     started_at, updated_at)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
ON CONFLICT(owner_identity_id)
DO UPDATE SET
    mode = excluded.mode,
    requested_from_epoch = excluded.requested_from_epoch,
    requested_from_seq = excluded.requested_from_seq,
    recovery_id_hash = excluded.recovery_id_hash,
    snapshot_scan_seq = excluded.snapshot_scan_seq,
    status = excluded.status,
    retry_count = excluded.retry_count,
    last_error_code = excluded.last_error_code,
    started_at = excluded.started_at,
    updated_at = excluded.updated_at"#,
            params![
                state.owner_identity_id,
                state.mode,
                state.requested_from_epoch,
                state.requested_from_seq,
                state.recovery_id_hash,
                state.snapshot_scan_seq,
                state.status,
                state.retry_count,
                state.last_error_code,
                state.started_at,
                state.updated_at,
            ],
        )
        .map_err(super::local_state_unavailable)?;
    Ok(())
}

pub(crate) fn load_recovery_state(
    connection: &Connection,
    owner_identity_id: &str,
) -> crate::ImResult<Option<RecoveryState>> {
    connection
        .query_row(
            r#"
SELECT owner_identity_id, mode, requested_from_epoch, requested_from_seq,
       recovery_id_hash, snapshot_scan_seq, status, retry_count, last_error_code,
       started_at, updated_at
FROM sync_recovery_state
WHERE owner_identity_id = ?1"#,
            [owner_identity_id],
            |row| {
                Ok(RecoveryState {
                    owner_identity_id: row.get(0)?,
                    mode: row.get(1)?,
                    requested_from_epoch: row.get(2)?,
                    requested_from_seq: row.get(3)?,
                    recovery_id_hash: row.get(4)?,
                    snapshot_scan_seq: row.get(5)?,
                    status: row.get(6)?,
                    retry_count: row.get(7)?,
                    last_error_code: row.get(8)?,
                    started_at: row.get(9)?,
                    updated_at: row.get(10)?,
                })
            },
        )
        .optional()
        .map_err(super::local_state_unavailable)
}

pub(crate) fn mark_thread_read_and_update_outbox(
    connection: &Connection,
    owner_identity_id: &str,
    owner_did: &str,
    input: super::messages::MarkThreadReadWatermarkInput,
) -> crate::ImResult<super::messages::MarkThreadReadWatermarkResult> {
    let transaction = connection
        .unchecked_transaction()
        .map_err(super::local_state_unavailable)?;
    let pending_remote_ack = input.pending_remote_ack;
    let acknowledged_remote_seq = (!pending_remote_ack)
        .then(|| input.read_watermark_seq.clone())
        .flatten();
    let mut result = super::messages::mark_thread_read_watermark_for_owner_identity(
        &transaction,
        owner_identity_id,
        owner_did,
        input,
    )?;
    let Some(seq) = result.read_watermark_seq.as_deref() else {
        transaction
            .commit()
            .map_err(super::local_state_unavailable)?;
        return Ok(result);
    };
    let remote_thread_key = transaction
        .query_row(
            "SELECT remote_thread_key
             FROM sync_thread_bindings
             WHERE owner_identity_id = ?1 AND conversation_id = ?2
             ORDER BY updated_at DESC, remote_thread_key DESC
             LIMIT 1",
            params![owner_identity_id, result.conversation_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(super::local_state_unavailable)?
        .or_else(|| {
            (result.thread_scope == "group")
                .then(|| remote_group_key_from_local_thread_id(&result.thread_id))
        });
    result.remote_thread_key = remote_thread_key.clone();
    let has_sync_binding =
        load_identity_account_binding(&transaction, owner_identity_id)?.is_some();
    if pending_remote_ack && has_sync_binding && remote_thread_key.is_some() {
        let remote_thread_key = remote_thread_key
            .as_deref()
            .expect("read outbox branch requires a remote thread key");
        let Some((remote_seq, remote_message_id, target_remote_thread_key)) =
            ordinary_read_outbox_target(
                &transaction,
                owner_identity_id,
                &result.thread_scope,
                &result.conversation_id,
                seq,
                result.read_watermark_message_id.as_deref(),
            )?
        else {
            if result.thread_scope == "group" {
                transaction
                    .execute(
                        "UPDATE thread_read_state
                         SET pending_remote_ack = 0, remote_ack_at = NULL
                         WHERE owner_identity_id = ?1 AND conversation_id = ?2",
                        params![owner_identity_id, result.conversation_id],
                    )
                    .map_err(super::local_state_unavailable)?;
                result.remote_ack_applicable = false;
                transaction
                    .commit()
                    .map_err(super::local_state_unavailable)?;
                return Ok(result);
            }
            return Err(sync_error(
                "SYNC_LOCAL_OUTBOX_CORRUPT",
                "ordinary Direct read watermark has no remote target",
            ));
        };
        let remote_thread_key = target_remote_thread_key
            .as_deref()
            .unwrap_or(remote_thread_key);
        result.remote_thread_key = Some(remote_thread_key.to_owned());
        result.outbox_operation_id = Some(upsert_read_outbox(
            &transaction,
            owner_identity_id,
            &result.thread_scope,
            &result.thread_id,
            remote_thread_key,
            &remote_seq,
            remote_message_id.as_deref(),
            result.read_watermark_at.as_deref(),
        )?);
    } else if !pending_remote_ack && has_sync_binding {
        let Some(remote_thread_key) = remote_thread_key.as_deref() else {
            transaction
                .commit()
                .map_err(super::local_state_unavailable)?;
            return Ok(result);
        };
        acknowledge_read_outbox(
            &transaction,
            owner_identity_id,
            remote_thread_key,
            Some(&result.conversation_id),
            acknowledged_remote_seq.as_deref().unwrap_or(seq),
            unix_time_i64(),
        )?;
        let still_pending = transaction
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM local_mutation_outbox
                    WHERE owner_identity_id = ?1
                      AND status NOT IN ('committed', 'permanent_failure')
                      AND (
                          aggregate_id = ?2
                          OR (
                              json_valid(payload_json)
                              AND json_extract(payload_json, '$.thread_id') = ?3
                          )
                      )
                 )",
                params![owner_identity_id, remote_thread_key, result.conversation_id],
                |row| row.get::<_, bool>(0),
            )
            .map_err(super::local_state_unavailable)?;
        if still_pending {
            transaction
                .execute(
                    "UPDATE thread_read_state
                     SET pending_remote_ack = 1, remote_ack_at = NULL
                     WHERE owner_identity_id = ?1 AND conversation_id = ?2",
                    params![owner_identity_id, result.conversation_id],
                )
                .map_err(super::local_state_unavailable)?;
        }
    }
    transaction
        .commit()
        .map_err(super::local_state_unavailable)?;
    Ok(result)
}

fn ordinary_read_outbox_target(
    connection: &Connection,
    owner_identity_id: &str,
    thread_kind: &str,
    conversation_id: &str,
    read_watermark_seq: &str,
    read_watermark_message_id: Option<&str>,
) -> crate::ImResult<Option<(String, Option<String>, Option<String>)>> {
    if thread_kind == "group" {
        return ordinary_group_read_target(
            connection,
            owner_identity_id,
            conversation_id,
            read_watermark_seq,
        )
        .map(|target| target.map(|(seq, message_id)| (seq, Some(message_id), None)));
    }
    validate_decimal("read_watermark_seq", read_watermark_seq)?;
    let target = connection
        .query_row(
            r#"
            SELECT CAST(server_seq AS TEXT), msg_id,
                   CASE
                       WHEN json_valid(COALESCE(metadata, ''))
                       THEN NULLIF(
                           TRIM(json_extract(metadata, '$.remote_thread_key')),
                           ''
                       )
                       ELSE NULL
                   END
            FROM messages
            WHERE owner_identity_id = ?1
              AND COALESCE(NULLIF(conversation_id, ''), thread_id) = ?2
              AND wire_thread_kind = 'direct'
              AND hydration_state = 'hydrated'
              AND COALESCE(is_e2ee, 0) = 0
              AND server_seq IS NOT NULL
              AND server_seq <= CAST(?3 AS INTEGER)
            ORDER BY server_seq DESC
            LIMIT 1
            "#,
            params![owner_identity_id, conversation_id, read_watermark_seq],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    Some(row.get::<_, String>(1)?),
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()
        .map_err(super::local_state_unavailable)?;
    Ok(Some(target.unwrap_or_else(|| {
        (
            read_watermark_seq.to_owned(),
            read_watermark_message_id.map(str::to_owned),
            None,
        )
    })))
}

fn ordinary_group_read_target(
    connection: &Connection,
    owner_identity_id: &str,
    conversation_id: &str,
    read_watermark_seq: &str,
) -> crate::ImResult<Option<(String, String)>> {
    validate_decimal("read_watermark_seq", read_watermark_seq)?;
    connection
        .query_row(
            r#"
            SELECT server_seq, raw_message_id
            FROM (
                SELECT server_seq,
                       CASE
                           WHEN json_valid(COALESCE(metadata, ''))
                           THEN COALESCE(
                               NULLIF(
                                   TRIM(json_extract(metadata, '$.raw_message_id')),
                                   ''
                               ),
                               NULLIF(
                                   TRIM(json_extract(metadata, '$.operation_id')),
                                   ''
                               ),
                               CASE
                                   WHEN COALESCE(
                                       TRIM(json_extract(metadata, '$.message_role')),
                                       ''
                                   ) <> 'group_system_event'
                                    AND msg_id = COALESCE(
                                        NULLIF(TRIM(group_did), ''),
                                        NULLIF(TRIM(group_id), ''),
                                        NULLIF(TRIM(wire_thread_ref), '')
                                    ) || ':' || CAST(server_seq AS TEXT)
                                   THEN msg_id
                                   ELSE NULL
                               END
                           )
                           ELSE NULL
                       END AS raw_message_id
                FROM messages
                WHERE owner_identity_id = ?1
                  AND COALESCE(NULLIF(conversation_id, ''), thread_id) = ?2
                  AND wire_thread_kind = 'group'
                  AND hydration_state = 'hydrated'
                  AND COALESCE(is_e2ee, 0) = 0
                  AND server_seq IS NOT NULL
                  AND server_seq <= CAST(?3 AS INTEGER)
            ) AS candidate
            WHERE raw_message_id IS NOT NULL
            ORDER BY server_seq DESC
            LIMIT 1
            "#,
            params![owner_identity_id, conversation_id, read_watermark_seq],
            |row| Ok((row.get::<_, i64>(0)?.to_string(), row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(super::local_state_unavailable)
}

fn upsert_read_outbox(
    connection: &Connection,
    owner_identity_id: &str,
    thread_kind: &str,
    thread_id: &str,
    remote_thread_key: &str,
    read_watermark_seq: &str,
    read_watermark_message_id: Option<&str>,
    read_watermark_at: Option<&str>,
) -> crate::ImResult<String> {
    validate_decimal("read_watermark_seq", read_watermark_seq)?;
    let existing = connection
        .query_row(
            "SELECT mutation_id, operation_id, payload_json
             FROM local_mutation_outbox
             WHERE owner_identity_id = ?1 AND aggregate_id = ?2
               AND status = 'pending' AND attempt_count = 0
             ORDER BY created_at DESC, mutation_id DESC LIMIT 1",
            params![owner_identity_id, remote_thread_key],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()
        .map_err(super::local_state_unavailable)?;
    let now = unix_time_i64();
    let payload = serde_json::json!({
        "thread_kind": thread_kind,
        "thread_id": thread_id,
        "remote_thread_key": remote_thread_key,
        "read_watermark_seq": read_watermark_seq,
        "read_watermark_message_id": read_watermark_message_id,
        "read_watermark_at": read_watermark_at,
    });
    if let Some((mutation_id, operation_id, current_payload)) = existing {
        let current: serde_json::Value =
            serde_json::from_str(&current_payload).map_err(|error| {
                sync_error(
                    "SYNC_LOCAL_OUTBOX_CORRUPT",
                    format!("read outbox payload is invalid: {error}"),
                )
            })?;
        let current_seq = current
            .get("read_watermark_seq")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("0");
        if compare_decimal(read_watermark_seq, current_seq)? == std::cmp::Ordering::Greater {
            connection
                .execute(
                    "UPDATE local_mutation_outbox
                     SET payload_json = ?1, updated_at = ?2
                     WHERE owner_identity_id = ?3 AND mutation_id = ?4",
                    params![payload.to_string(), now, owner_identity_id, mutation_id],
                )
                .map_err(super::local_state_unavailable)?;
        }
        return Ok(operation_id);
    }

    let operation_id = random_local_id("op-read");
    enqueue_local_mutation(
        connection,
        &LocalMutationRecord {
            owner_identity_id: owner_identity_id.to_owned(),
            mutation_id: random_local_id("read"),
            operation_id: operation_id.clone(),
            mutation_type: "read_state_mark_read".to_owned(),
            aggregate_id: remote_thread_key.to_owned(),
            payload_json: payload.to_string(),
            status: "pending".to_owned(),
            attempt_count: 0,
            retry_at: None,
            in_flight_since: None,
            last_error_code: None,
            created_at: now,
            updated_at: now,
        },
    )?;
    Ok(operation_id)
}

fn random_local_id(prefix: &str) -> String {
    let mut bytes = [0_u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let suffix = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("{prefix}-{suffix}")
}

pub(crate) fn upsert_sync_thread_binding(
    connection: &Connection,
    binding: &SyncThreadBinding,
) -> crate::ImResult<()> {
    validate_required("owner_identity_id", &binding.owner_identity_id)?;
    validate_required("remote_thread_key", &binding.remote_thread_key)?;
    validate_required("conversation_id", &binding.conversation_id)?;
    if !matches!(binding.thread_kind.as_str(), "direct" | "group") {
        return Err(sync_error(
            "SYNC_INVALID_PAGE",
            "sync thread binding kind must be direct or group",
        ));
    }
    let conflict = connection
        .query_row(
            "SELECT thread_kind, conversation_id
             FROM sync_thread_bindings
             WHERE owner_identity_id = ?1 AND remote_thread_key = ?2",
            params![binding.owner_identity_id, binding.remote_thread_key],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(super::local_state_unavailable)?;
    if let Some(current) = conflict.as_ref() {
        let next = (binding.thread_kind.clone(), binding.conversation_id.clone());
        if current != &next && !is_direct_binding_canonical_upgrade(current, &next) {
            return Err(sync_error(
                "SYNC_THREAD_BINDING_CONFLICT",
                "remote thread key cannot be rebound to another canonical conversation",
            ));
        }
    }
    let rotated_direct_binding = if conflict.is_none() {
        let existing = connection
            .query_row(
                "SELECT remote_thread_key, thread_kind
                 FROM sync_thread_bindings
                 WHERE owner_identity_id = ?1 AND conversation_id = ?2",
                params![binding.owner_identity_id, binding.conversation_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(super::local_state_unavailable)?;
        if let Some((previous_remote_thread_key, previous_thread_kind)) = existing {
            if previous_thread_kind != "direct" || binding.thread_kind != "direct" {
                return Err(sync_error(
                    "SYNC_THREAD_BINDING_CONFLICT",
                    "canonical conversation cannot be rebound to another remote thread",
                ));
            }
            connection
                .execute(
                    "UPDATE sync_thread_bindings
                     SET remote_thread_key = ?1, updated_at = ?2
                     WHERE owner_identity_id = ?3
                       AND remote_thread_key = ?4
                       AND conversation_id = ?5
                       AND thread_kind = 'direct'",
                    params![
                        binding.remote_thread_key,
                        binding.updated_at,
                        binding.owner_identity_id,
                        previous_remote_thread_key,
                        binding.conversation_id,
                    ],
                )
                .map_err(super::local_state_unavailable)?;
            true
        } else {
            false
        }
    } else {
        false
    };
    if !rotated_direct_binding {
        connection
            .execute(
                "INSERT INTO sync_thread_bindings
                (owner_identity_id, remote_thread_key, thread_kind, conversation_id, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(owner_identity_id, remote_thread_key) DO UPDATE SET
                conversation_id = excluded.conversation_id,
                updated_at = excluded.updated_at",
                params![
                    binding.owner_identity_id,
                    binding.remote_thread_key,
                    binding.thread_kind,
                    binding.conversation_id,
                    binding.updated_at,
                ],
            )
            .map_err(super::local_state_unavailable)?;
    }
    if let Some(state) = load_remote_read_state(
        connection,
        &binding.owner_identity_id,
        &binding.remote_thread_key,
    )? {
        let owner_did = connection
            .query_row(
                "SELECT current_did FROM identity_account_bindings
                 WHERE owner_identity_id = ?1",
                [&binding.owner_identity_id],
                |row| row.get::<_, String>(0),
            )
            .map_err(super::local_state_unavailable)?;
        project_remote_read_state(
            connection,
            &binding.owner_identity_id,
            &owner_did,
            &state,
            &(binding.thread_kind.clone(), binding.conversation_id.clone()),
        )?;
    }
    let pending_local = connection
        .query_row(
            "SELECT thread_scope, thread_id, read_watermark_seq,
                    read_watermark_message_id, read_watermark_at
             FROM thread_read_state
             WHERE owner_identity_id = ?1
               AND conversation_id = ?2
               AND thread_scope = ?3
               AND pending_remote_ack = 1
               AND read_watermark_seq IS NOT NULL
             ORDER BY updated_at DESC
             LIMIT 1",
            params![
                binding.owner_identity_id,
                binding.conversation_id,
                binding.thread_kind
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            },
        )
        .optional()
        .map_err(super::local_state_unavailable)?;
    if let Some((thread_kind, thread_id, seq, message_id, read_at)) = pending_local {
        if let Some((remote_seq, remote_message_id, target_remote_thread_key)) =
            ordinary_read_outbox_target(
                connection,
                &binding.owner_identity_id,
                &thread_kind,
                &binding.conversation_id,
                &seq,
                message_id.as_deref(),
            )?
        {
            upsert_read_outbox(
                connection,
                &binding.owner_identity_id,
                &thread_kind,
                &thread_id,
                target_remote_thread_key
                    .as_deref()
                    .unwrap_or(&binding.remote_thread_key),
                &remote_seq,
                remote_message_id.as_deref(),
                read_at.as_deref(),
            )?;
        } else if thread_kind == "group" {
            connection
                .execute(
                    "UPDATE thread_read_state
                     SET pending_remote_ack = 0, remote_ack_at = NULL
                     WHERE owner_identity_id = ?1 AND conversation_id = ?2",
                    params![binding.owner_identity_id, binding.conversation_id],
                )
                .map_err(super::local_state_unavailable)?;
        }
    }
    Ok(())
}

fn is_direct_binding_canonical_upgrade(
    current: &(String, String),
    next: &(String, String),
) -> bool {
    current.0 == "direct"
        && next.0 == "direct"
        && current.1.starts_with("dm:did:")
        && next.1.starts_with("dm:peer-scope:v1:")
}

fn apply_remote_read_state(
    connection: &Connection,
    owner_identity_id: &str,
    owner_did: &str,
    state: &ReadStateApplyV2,
) -> crate::ImResult<()> {
    validate_required("remote_thread_key", &state.remote_thread_key)?;
    validate_decimal("read_watermark_seq", &state.read_watermark_seq)?;
    validate_positive_decimal("state_version", &state.state_version)?;
    let current_remote =
        load_remote_read_state(connection, owner_identity_id, &state.remote_thread_key)?;
    if let Some(current) = current_remote.as_ref() {
        match compare_decimal(&state.state_version, &current.state_version)? {
            std::cmp::Ordering::Less => return Ok(()),
            std::cmp::Ordering::Equal => {
                if current != state {
                    return Err(sync_error(
                        "SYNC_READ_STATE_CONFLICT",
                        "equal read state versions carry conflicting content",
                    ));
                }
                return Ok(());
            }
            std::cmp::Ordering::Greater => {}
        }
    }
    connection
        .execute(
            "INSERT INTO sync_remote_read_states
                (owner_identity_id, remote_thread_key, thread_kind,
                 read_watermark_seq, read_watermark_message_id, state_version, occurred_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(owner_identity_id, remote_thread_key) DO UPDATE SET
                thread_kind = excluded.thread_kind,
                read_watermark_seq = excluded.read_watermark_seq,
                read_watermark_message_id = excluded.read_watermark_message_id,
                state_version = excluded.state_version,
                occurred_at = excluded.occurred_at",
            params![
                owner_identity_id,
                state.remote_thread_key,
                state.thread_kind,
                state.read_watermark_seq,
                state.read_watermark_message_id,
                state.state_version,
                state.occurred_at,
            ],
        )
        .map_err(super::local_state_unavailable)?;
    let binding = connection
        .query_row(
            "SELECT thread_kind, conversation_id
             FROM sync_thread_bindings
             WHERE owner_identity_id = ?1 AND remote_thread_key = ?2",
            params![owner_identity_id, state.remote_thread_key],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(super::local_state_unavailable)?
        .or_else(|| {
            (state.thread_kind == "group").then(|| {
                (
                    "group".to_owned(),
                    super::owner_scope::group_conversation_id(&state.remote_thread_key),
                )
            })
        });
    let Some(binding) = binding else {
        return Ok(());
    };
    project_remote_read_state(connection, owner_identity_id, owner_did, state, &binding)
}

fn remote_group_key_from_local_thread_id(thread_id: &str) -> String {
    thread_id
        .strip_prefix("group:")
        .unwrap_or(thread_id)
        .to_owned()
}

fn project_remote_read_state(
    connection: &Connection,
    owner_identity_id: &str,
    owner_did: &str,
    state: &ReadStateApplyV2,
    binding: &(String, String),
) -> crate::ImResult<()> {
    if binding.0 != state.thread_kind {
        return Err(sync_error(
            "SYNC_THREAD_BINDING_CONFLICT",
            "read state thread kind conflicts with the local binding",
        ));
    }
    let current = super::read_state::get_thread_read_state(
        connection,
        owner_identity_id,
        &binding.0,
        &binding.1,
    )?;
    // A Direct conversation key can rotate after Handle recovery. Its replacement has an
    // independent state-version sequence, while the Direct message sequence stays monotonic.
    if binding.0 != "direct" {
        if let Some(current_version) = current
            .as_ref()
            .and_then(|record| record.remote_state_version.as_deref())
        {
            match compare_decimal(&state.state_version, current_version)? {
                std::cmp::Ordering::Less => {
                    connection
                        .execute(
                            "DELETE FROM sync_remote_read_states
                         WHERE owner_identity_id = ?1 AND remote_thread_key = ?2",
                            params![owner_identity_id, state.remote_thread_key],
                        )
                        .map_err(super::local_state_unavailable)?;
                    return Ok(());
                }
                std::cmp::Ordering::Equal => {
                    let current_seq = current
                        .as_ref()
                        .and_then(|record| record.read_watermark_seq.as_deref())
                        .unwrap_or("0");
                    if compare_decimal(&state.read_watermark_seq, current_seq)?
                        == std::cmp::Ordering::Greater
                    {
                        return Err(sync_error(
                            "SYNC_READ_STATE_CONFLICT",
                            "equal remote read versions carry a higher conflicting watermark",
                        ));
                    }
                    connection
                        .execute(
                            "DELETE FROM sync_remote_read_states
                         WHERE owner_identity_id = ?1 AND remote_thread_key = ?2",
                            params![owner_identity_id, state.remote_thread_key],
                        )
                        .map_err(super::local_state_unavailable)?;
                    return Ok(());
                }
                std::cmp::Ordering::Greater => {}
            }
        }
    }
    let local_seq = current
        .as_ref()
        .and_then(|record| record.read_watermark_seq.as_deref())
        .unwrap_or("0");
    if binding.0 == "direct"
        && compare_decimal(&state.read_watermark_seq, local_seq)? == std::cmp::Ordering::Less
    {
        acknowledge_read_outbox(
            connection,
            owner_identity_id,
            &state.remote_thread_key,
            Some(&binding.1),
            &state.read_watermark_seq,
            unix_time_i64(),
        )?;
        connection
            .execute(
                "DELETE FROM sync_remote_read_states
                 WHERE owner_identity_id = ?1 AND remote_thread_key = ?2",
                params![owner_identity_id, state.remote_thread_key],
            )
            .map_err(super::local_state_unavailable)?;
        return Ok(());
    }
    let pending_remote_ack =
        compare_decimal(local_seq, &state.read_watermark_seq)? == std::cmp::Ordering::Greater;
    let thread = if binding.0 == "group" {
        crate::messages::ThreadRef::Group(crate::ids::GroupRef::parse(binding.1.clone())?)
    } else {
        crate::messages::ThreadRef::Thread(crate::ids::ThreadId::parse(binding.1.clone())?)
    };
    let _ = super::messages::mark_thread_read_watermark_for_owner_identity(
        connection,
        owner_identity_id,
        owner_did,
        super::messages::MarkThreadReadWatermarkInput {
            thread,
            read_watermark_message_id: state.read_watermark_message_id.clone(),
            read_watermark_seq: Some(state.read_watermark_seq.clone()),
            read_watermark_at: Some(state.occurred_at.clone()),
            pending_remote_ack,
        },
    )?;
    let remote_advances_local =
        compare_decimal(&state.read_watermark_seq, local_seq)? == std::cmp::Ordering::Greater;
    let effective_seq = if remote_advances_local {
        state.read_watermark_seq.clone()
    } else {
        local_seq.to_owned()
    };
    let effective_message_id = if remote_advances_local {
        state.read_watermark_message_id.clone()
    } else {
        current
            .as_ref()
            .and_then(|record| record.read_watermark_message_id.clone())
    };
    super::read_state::replace_thread_read_state(
        connection,
        &super::read_state::ThreadReadStateRecord {
            owner_identity_id: owner_identity_id.to_owned(),
            owner_did: owner_did.to_owned(),
            thread_scope: binding.0.clone(),
            thread_id: binding.1.clone(),
            conversation_id: binding.1.clone(),
            read_watermark_message_id: effective_message_id,
            read_watermark_seq: Some(effective_seq),
            read_watermark_at: Some(state.occurred_at.clone()),
            pending_remote_ack,
            remote_ack_at: (!pending_remote_ack).then(|| state.occurred_at.clone()),
            remote_state_version: Some(state.state_version.clone()),
            updated_at: state.occurred_at.clone(),
        },
    )?;
    acknowledge_read_outbox(
        connection,
        owner_identity_id,
        &state.remote_thread_key,
        Some(&binding.1),
        &state.read_watermark_seq,
        unix_time_i64(),
    )?;
    connection
        .execute(
            "DELETE FROM sync_remote_read_states
             WHERE owner_identity_id = ?1 AND remote_thread_key = ?2",
            params![owner_identity_id, state.remote_thread_key],
        )
        .map_err(super::local_state_unavailable)?;
    Ok(())
}

fn load_remote_read_state(
    connection: &Connection,
    owner_identity_id: &str,
    remote_thread_key: &str,
) -> crate::ImResult<Option<ReadStateApplyV2>> {
    connection
        .query_row(
            "SELECT remote_thread_key, thread_kind, read_watermark_seq,
                    read_watermark_message_id, state_version, occurred_at
             FROM sync_remote_read_states
             WHERE owner_identity_id = ?1 AND remote_thread_key = ?2",
            params![owner_identity_id, remote_thread_key],
            |row| {
                Ok(ReadStateApplyV2 {
                    remote_thread_key: row.get(0)?,
                    thread_kind: row.get(1)?,
                    read_watermark_seq: row.get(2)?,
                    read_watermark_message_id: row.get(3)?,
                    state_version: row.get(4)?,
                    occurred_at: row.get(5)?,
                })
            },
        )
        .optional()
        .map_err(super::local_state_unavailable)
}

fn acknowledge_read_outbox(
    connection: &Connection,
    owner_identity_id: &str,
    aggregate_id: &str,
    conversation_id: Option<&str>,
    acknowledged_seq: &str,
    now: i64,
) -> crate::ImResult<()> {
    let mut statement = connection
        .prepare(
            "SELECT mutation_id, payload_json
             FROM local_mutation_outbox
             WHERE owner_identity_id = ?1
               AND status NOT IN ('committed', 'permanent_failure')
               AND (
                   aggregate_id = ?2
                   OR (
                       ?3 IS NOT NULL
                       AND json_valid(payload_json)
                       AND json_extract(payload_json, '$.thread_id') = ?3
                   )
               )",
        )
        .map_err(super::local_state_unavailable)?;
    let rows = statement
        .query_map(
            params![owner_identity_id, aggregate_id, conversation_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .map_err(super::local_state_unavailable)?;
    let mut committed = Vec::new();
    for row in rows {
        let (mutation_id, payload) = row.map_err(super::local_state_unavailable)?;
        let payload: serde_json::Value = serde_json::from_str(&payload).map_err(|error| {
            sync_error(
                "SYNC_LOCAL_OUTBOX_CORRUPT",
                format!("read outbox payload is invalid: {error}"),
            )
        })?;
        let seq = payload
            .get("read_watermark_seq")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                sync_error(
                    "SYNC_LOCAL_OUTBOX_CORRUPT",
                    "read outbox payload has no watermark",
                )
            })?;
        if compare_decimal(seq, acknowledged_seq)? != std::cmp::Ordering::Greater {
            committed.push(mutation_id);
        }
    }
    drop(statement);
    for mutation_id in committed {
        connection
            .execute(
                "UPDATE local_mutation_outbox
                 SET status = 'committed', in_flight_since = NULL,
                     retry_at = NULL, last_error_code = NULL, updated_at = ?1
                 WHERE owner_identity_id = ?2 AND mutation_id = ?3",
                params![now, owner_identity_id, mutation_id],
            )
            .map_err(super::local_state_unavailable)?;
    }
    Ok(())
}

pub(crate) fn enqueue_local_mutation(
    connection: &Connection,
    record: &LocalMutationRecord,
) -> crate::ImResult<()> {
    validate_required("owner_identity_id", &record.owner_identity_id)?;
    validate_required("mutation_id", &record.mutation_id)?;
    validate_required("operation_id", &record.operation_id)?;
    validate_required("aggregate_id", &record.aggregate_id)?;
    validate_local_mutation_status(&record.status)?;
    if record.attempt_count < 0 {
        return Err(crate::ImError::invalid_input(
            Some("attempt_count".to_owned()),
            "attempt_count must not be negative",
        ));
    }
    if record.mutation_type != "read_state_mark_read" {
        return Err(crate::ImError::invalid_input(
            Some("mutation_type".to_owned()),
            "only read_state_mark_read is supported by the v2 local mutation outbox",
        ));
    }
    connection
        .execute(
            r#"
INSERT INTO local_mutation_outbox
    (owner_identity_id, mutation_id, operation_id, mutation_type, aggregate_id,
     payload_json, status, attempt_count, retry_at, in_flight_since,
     last_error_code, created_at, updated_at)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)"#,
            params![
                record.owner_identity_id,
                record.mutation_id,
                record.operation_id,
                record.mutation_type,
                record.aggregate_id,
                record.payload_json,
                record.status,
                record.attempt_count,
                record.retry_at,
                record.in_flight_since,
                record.last_error_code,
                record.created_at,
                record.updated_at,
            ],
        )
        .map_err(super::local_state_unavailable)?;
    Ok(())
}

pub(crate) fn load_local_mutation(
    connection: &Connection,
    owner_identity_id: &str,
    mutation_id: &str,
) -> crate::ImResult<Option<LocalMutationRecord>> {
    validate_required("owner_identity_id", owner_identity_id)?;
    validate_required("mutation_id", mutation_id)?;
    connection
        .query_row(
            r#"
SELECT owner_identity_id, mutation_id, operation_id, mutation_type, aggregate_id,
       payload_json, status, attempt_count, retry_at, in_flight_since,
       last_error_code, created_at, updated_at
FROM local_mutation_outbox
WHERE owner_identity_id = ?1 AND mutation_id = ?2"#,
            params![owner_identity_id, mutation_id],
            |row| {
                Ok(LocalMutationRecord {
                    owner_identity_id: row.get(0)?,
                    mutation_id: row.get(1)?,
                    operation_id: row.get(2)?,
                    mutation_type: row.get(3)?,
                    aggregate_id: row.get(4)?,
                    payload_json: row.get(5)?,
                    status: row.get(6)?,
                    attempt_count: row.get(7)?,
                    retry_at: row.get(8)?,
                    in_flight_since: row.get(9)?,
                    last_error_code: row.get(10)?,
                    created_at: row.get(11)?,
                    updated_at: row.get(12)?,
                })
            },
        )
        .optional()
        .map_err(super::local_state_unavailable)
}

pub(crate) fn claim_next_read_mutation(
    connection: &Connection,
    owner_identity_id: &str,
    now: i64,
) -> crate::ImResult<Option<LocalMutationRecord>> {
    let transaction = connection
        .unchecked_transaction()
        .map_err(super::local_state_unavailable)?;
    let next = transaction
        .query_row(
            "SELECT mutation_id
             FROM local_mutation_outbox
             WHERE owner_identity_id = ?1
               AND status IN ('pending', 'retryable')
               AND (retry_at IS NULL OR retry_at <= ?2)
               AND NOT EXISTS (
                   SELECT 1 FROM local_mutation_outbox predecessor
                   WHERE predecessor.owner_identity_id = local_mutation_outbox.owner_identity_id
                     AND predecessor.aggregate_id = local_mutation_outbox.aggregate_id
                     AND predecessor.status = 'in_flight'
               )
             ORDER BY created_at, mutation_id
             LIMIT 1",
            params![owner_identity_id, now],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(super::local_state_unavailable)?;
    let Some(mutation_id) = next else {
        transaction
            .commit()
            .map_err(super::local_state_unavailable)?;
        return Ok(None);
    };
    transaction
        .execute(
            "UPDATE local_mutation_outbox
             SET status = 'in_flight', attempt_count = attempt_count + 1,
                 in_flight_since = ?1, updated_at = ?1
             WHERE owner_identity_id = ?2 AND mutation_id = ?3",
            params![now, owner_identity_id, mutation_id],
        )
        .map_err(super::local_state_unavailable)?;
    let record = load_local_mutation(&transaction, owner_identity_id, &mutation_id)?;
    transaction
        .commit()
        .map_err(super::local_state_unavailable)?;
    Ok(record)
}

pub(crate) fn claim_read_mutation_by_operation_id(
    connection: &Connection,
    owner_identity_id: &str,
    operation_id: &str,
    now: i64,
) -> crate::ImResult<Option<LocalMutationRecord>> {
    validate_required("owner_identity_id", owner_identity_id)?;
    validate_required("operation_id", operation_id)?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(super::local_state_unavailable)?;
    let mutation_id = transaction
        .query_row(
            "SELECT candidate.mutation_id
             FROM local_mutation_outbox candidate
             WHERE candidate.owner_identity_id = ?1
               AND candidate.operation_id = ?2
               AND candidate.status IN ('pending', 'retryable')
               AND (candidate.retry_at IS NULL OR candidate.retry_at <= ?3)
               AND NOT EXISTS (
                   SELECT 1 FROM local_mutation_outbox predecessor
                   WHERE predecessor.owner_identity_id = candidate.owner_identity_id
                     AND predecessor.aggregate_id = candidate.aggregate_id
                     AND predecessor.status = 'in_flight'
                     AND predecessor.mutation_id <> candidate.mutation_id
               )
             LIMIT 1",
            params![owner_identity_id, operation_id, now],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(super::local_state_unavailable)?;
    let Some(mutation_id) = mutation_id else {
        transaction
            .commit()
            .map_err(super::local_state_unavailable)?;
        return Ok(None);
    };
    let updated = transaction
        .execute(
            "UPDATE local_mutation_outbox
             SET status = 'in_flight', attempt_count = attempt_count + 1,
                 retry_at = NULL, in_flight_since = ?1, updated_at = ?1
             WHERE owner_identity_id = ?2 AND mutation_id = ?3
               AND status IN ('pending', 'retryable')",
            params![now, owner_identity_id, mutation_id],
        )
        .map_err(super::local_state_unavailable)?;
    if updated != 1 {
        return Err(sync_error(
            "SYNC_LOCAL_OUTBOX_CONFLICT",
            "read outbox claim lost its operation",
        ));
    }
    let record = load_local_mutation(&transaction, owner_identity_id, &mutation_id)?;
    transaction
        .commit()
        .map_err(super::local_state_unavailable)?;
    Ok(record)
}

pub(crate) fn retry_local_mutation(
    connection: &Connection,
    owner_identity_id: &str,
    mutation_id: &str,
    error_code: &str,
    retry_at: i64,
) -> crate::ImResult<()> {
    connection
        .execute(
            "UPDATE local_mutation_outbox
             SET status = 'retryable', in_flight_since = NULL,
                 retry_at = ?1, last_error_code = ?2, updated_at = ?1
             WHERE owner_identity_id = ?3 AND mutation_id = ?4
               AND status = 'in_flight'",
            params![retry_at, error_code, owner_identity_id, mutation_id],
        )
        .map_err(super::local_state_unavailable)?;
    Ok(())
}

pub(crate) fn recover_interrupted_work(
    connection: &Connection,
    recovered_at: i64,
) -> crate::ImResult<()> {
    connection
        .execute(
            "UPDATE sync_recovery_state
             SET status = 'retryable', updated_at = ?1
             WHERE status IN ('recovering', 'downloading', 'applying')",
            [recovered_at],
        )
        .map_err(super::local_state_unavailable)?;
    connection
        .execute(
            "UPDATE local_mutation_outbox
             SET status = 'retryable', in_flight_since = NULL, updated_at = ?1
             WHERE status = 'in_flight'",
            [recovered_at],
        )
        .map_err(super::local_state_unavailable)?;
    Ok(())
}

fn validate_binding(binding: &IdentityAccountBinding) -> crate::ImResult<()> {
    validate_required("owner_identity_id", &binding.owner_identity_id)?;
    validate_required("account_id", &binding.account_id)?;
    validate_required("current_did", &binding.current_did)?;
    validate_required("protocol_device_id", &binding.protocol_device_id)?;
    validate_positive_decimal("identity_generation", &binding.identity_generation)?;
    validate_positive_decimal("device_auth_generation", &binding.device_auth_generation)?;
    if let Some(handle_scope) = binding.handle_scope.as_deref() {
        validate_required("handle_scope", handle_scope)?;
    }
    Ok(())
}

fn validate_message_sync_state(state: &MessageSyncState) -> crate::ImResult<()> {
    validate_required("owner_identity_id", &state.owner_identity_id)?;
    validate_required("account_id", &state.account_id)?;
    validate_required("protocol_device_id", &state.protocol_device_id)?;
    validate_positive_decimal("device_auth_generation", &state.device_auth_generation)?;
    validate_positive_decimal("stream_epoch", &state.stream_epoch)?;
    validate_decimal("scan_seq", &state.scan_seq)?;
    if !matches!(
        state.bootstrap_state.as_str(),
        "uninitialized" | "tail_bootstrapped" | "active" | "recovering" | "blocked"
    ) {
        return Err(crate::ImError::invalid_input(
            Some("bootstrap_state".to_owned()),
            "bootstrap_state is not supported",
        ));
    }
    Ok(())
}

fn validate_recovery_mode(value: &str) -> crate::ImResult<()> {
    if value == "compact_recovery" {
        return Ok(());
    }
    Err(crate::ImError::invalid_input(
        Some("mode".to_owned()),
        "recovery mode is not supported",
    ))
}

fn validate_recovery_status(value: &str) -> crate::ImResult<()> {
    if matches!(
        value,
        "recovering" | "downloading" | "applying" | "retryable" | "completed" | "permanent_failure"
    ) {
        return Ok(());
    }
    Err(crate::ImError::invalid_input(
        Some("status".to_owned()),
        "recovery status is not supported",
    ))
}

fn validate_local_mutation_status(value: &str) -> crate::ImResult<()> {
    if matches!(
        value,
        "pending" | "in_flight" | "retryable" | "committed" | "permanent_failure"
    ) {
        return Ok(());
    }
    Err(crate::ImError::invalid_input(
        Some("status".to_owned()),
        "local mutation status is not supported",
    ))
}

fn validate_group_owner(
    group: &super::groups::GroupRecord,
    owner_identity_id: &str,
) -> crate::ImResult<()> {
    if group.owner_identity_id == owner_identity_id {
        Ok(())
    } else {
        Err(crate::ImError::IdentityBindingConflict {
            detail: "group sync fact belongs to a different local owner".to_owned(),
        })
    }
}

fn validate_message_owner(
    message: &super::messages::MessageRecord,
    owner_identity_id: &str,
) -> crate::ImResult<()> {
    if message.owner_identity_id == owner_identity_id {
        Ok(())
    } else {
        Err(crate::ImError::IdentityBindingConflict {
            detail: "message sync fact belongs to a different local owner".to_owned(),
        })
    }
}

fn message_event_thread_binding(
    event: &DeltaApplyEventV2,
    owner_identity_id: &str,
) -> crate::ImResult<Option<SyncThreadBinding>> {
    for binding in &event.thread_bindings {
        if binding.owner_identity_id != owner_identity_id {
            return Err(crate::ImError::IdentityBindingConflict {
                detail: "sync thread binding belongs to a different local owner".to_owned(),
            });
        }
    }
    if event.messages.is_empty() {
        return Ok(None);
    }
    match event.event_type.as_str() {
        "message.created" => {
            if event.messages.len() != 1 || event.thread_bindings.len() != 1 {
                return Err(sync_error(
                    "SYNC_INVALID_PAGE",
                    "hydrated message.created requires exactly one message and one thread binding",
                ));
            }
            let binding = &event.thread_bindings[0];
            let message = &event.messages[0];
            if binding.thread_kind != message.wire_thread_kind {
                return Err(sync_error(
                    "SYNC_INVALID_PAGE",
                    "hydrated message thread binding kind conflicts with its wire identity",
                ));
            }
            Ok(Some(binding.clone()))
        }
        "group.member_changed" | "group.profile_updated" => {
            if event.messages.len() != 1 || !event.thread_bindings.is_empty() {
                return Err(sync_error(
                    "SYNC_INVALID_PAGE",
                    "Group state event requires exactly one local system message and no thread binding",
                ));
            }
            validate_group_system_message_projection(event, &event.messages[0])?;
            Ok(None)
        }
        _ => Err(sync_error(
            "SYNC_INVALID_PAGE",
            "sync event type is not allowed to carry a message projection",
        )),
    }
}

fn validate_group_system_message_projection(
    event: &DeltaApplyEventV2,
    message: &super::messages::MessageRecord,
) -> crate::ImResult<()> {
    let payload = serde_json::from_str::<serde_json::Value>(&message.content)
        .ok()
        .and_then(|value| value.as_object().cloned());
    let metadata = serde_json::from_str::<serde_json::Value>(&message.metadata)
        .ok()
        .and_then(|value| value.as_object().cloned());
    let group_did = payload
        .as_ref()
        .and_then(|value| value.get("group_did"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    let group_event_seq = payload
        .as_ref()
        .and_then(|value| value.get("group_event_seq"))
        .and_then(serde_json::Value::as_str)
        .and_then(|value| value.parse::<i64>().ok());
    let sync_event_type = payload
        .as_ref()
        .and_then(|value| value.get("sync_event_type"))
        .and_then(serde_json::Value::as_str);
    let valid = message.content_type == "application/json"
        && message.is_read
        && payload
            .as_ref()
            .and_then(|value| value.get("schema"))
            .and_then(serde_json::Value::as_str)
            == Some(crate::internal::group_system_events::GROUP_SYSTEM_EVENT_SCHEMA)
        && !group_did.is_empty()
        && (message.group_did.trim() == group_did || message.group_id.trim() == group_did)
        && group_event_seq.is_some_and(|sequence| {
            message.server_seq == Some(sequence)
                && message.msg_id == format!("{group_did}:{sequence}")
        })
        && sync_event_type == Some(event.event_type.as_str())
        && metadata
            .as_ref()
            .and_then(|value| value.get("message_role"))
            .and_then(serde_json::Value::as_str)
            == Some("group_system_event");
    if valid {
        return Ok(());
    }
    Err(sync_error(
        "SYNC_INVALID_PAGE",
        "Group state event contains an invalid local system message projection",
    ))
}

fn canonicalize_message_event_thread_bindings(
    bindings: &mut [SyncThreadBinding],
    event_type: &str,
    owner_identity_id: &str,
    canonical_conversation_ids: &BTreeSet<String>,
) -> crate::ImResult<()> {
    for binding in bindings.iter() {
        if binding.owner_identity_id != owner_identity_id {
            return Err(crate::ImError::IdentityBindingConflict {
                detail: "sync thread binding belongs to a different local owner".to_owned(),
            });
        }
    }
    if event_type != "message.created"
        || bindings.is_empty()
        || canonical_conversation_ids.is_empty()
    {
        return Ok(());
    }
    if canonical_conversation_ids.len() != 1 {
        return Err(sync_error(
            "SYNC_INVALID_PAGE",
            "hydrated message.created thread binding requires one canonical conversation",
        ));
    }
    let conversation_id = canonical_conversation_ids
        .iter()
        .next()
        .expect("one canonical conversation was validated");
    for binding in bindings {
        binding.conversation_id.clone_from(conversation_id);
    }
    Ok(())
}

fn group_state_is_stale(
    connection: &Connection,
    group: &super::groups::GroupRecord,
) -> crate::ImResult<bool> {
    let next_version = metadata_decimal(&group.metadata, "group_state_version")?;
    let Some(next_version) = next_version else {
        return Ok(false);
    };
    let existing_metadata = connection
        .query_row(
            "SELECT metadata FROM groups
             WHERE owner_identity_id = ?1 AND group_id = ?2",
            params![group.owner_identity_id, group.group_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map_err(super::local_state_unavailable)?
        .flatten();
    let Some(existing_metadata) = existing_metadata else {
        return Ok(false);
    };
    let Some(existing_version) = metadata_decimal(&existing_metadata, "group_state_version")?
    else {
        return Ok(false);
    };
    Ok(compare_decimal(&next_version, &existing_version)? != std::cmp::Ordering::Greater)
}

fn metadata_decimal(raw: &str, key: &str) -> crate::ImResult<Option<String>> {
    let Some(value) = serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .and_then(|value| value.get(key).cloned())
    else {
        return Ok(None);
    };
    let Some(value) = value.as_str() else {
        return Err(sync_error(
            "SYNC_INVALID_PAGE",
            format!("group metadata {key} must be a decimal string"),
        ));
    };
    validate_decimal(key, value).map_err(|_| {
        sync_error(
            "SYNC_INVALID_PAGE",
            format!("group metadata {key} must be a canonical decimal string"),
        )
    })?;
    Ok(Some(value.to_owned()))
}

fn v2_invalidation(
    connection: &Connection,
    owner_identity_id: &str,
    owner_did: &str,
    scan_seq: &str,
    messages: &[super::messages::MessageRecord],
    groups: &[super::groups::GroupRecord],
    read_states: &[ReadStateApplyV2],
) -> crate::ImResult<super::sync_state::SyncDeltaInvalidation> {
    let mut conversation_ids = BTreeSet::new();
    let mut thread_ids = BTreeSet::new();
    let mut group_ids = BTreeSet::new();
    let mut group_dids = BTreeSet::new();
    for message in messages {
        if !message.conversation_id.trim().is_empty() {
            conversation_ids.insert(message.conversation_id.clone());
        }
        if !message.thread_id.trim().is_empty() {
            thread_ids.insert(message.thread_id.clone());
        }
    }
    for group in groups {
        if !group.group_id.trim().is_empty() {
            group_ids.insert(group.group_id.clone());
        }
        if !group.group_did.trim().is_empty() {
            group_dids.insert(group.group_did.clone());
            let conversation_id =
                super::owner_scope::group_conversation_id(group.group_did.as_str());
            conversation_ids.insert(conversation_id.clone());
            thread_ids.insert(conversation_id);
        }
    }
    for read_state in read_states {
        let conversation_id = connection
            .query_row(
                "SELECT conversation_id
                 FROM sync_thread_bindings
                 WHERE owner_identity_id = ?1 AND remote_thread_key = ?2",
                params![owner_identity_id, read_state.remote_thread_key],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(super::local_state_unavailable)?
            .or_else(|| {
                (read_state.thread_kind == "group").then(|| {
                    super::owner_scope::group_conversation_id(&read_state.remote_thread_key)
                })
            });
        if let Some(conversation_id) = conversation_id.filter(|value| !value.trim().is_empty()) {
            conversation_ids.insert(conversation_id.clone());
            thread_ids.insert(conversation_id);
        }
    }
    Ok(super::sync_state::SyncDeltaInvalidation {
        owner_identity_id: owner_identity_id.to_owned(),
        owner_did: owner_did.to_owned(),
        reason: "sync_v2_delta".to_owned(),
        checkpoint_event_seq: scan_seq.to_owned(),
        conversation_ids: conversation_ids.into_iter().collect(),
        thread_ids: thread_ids.into_iter().collect(),
        group_ids: group_ids.into_iter().collect(),
        group_dids: group_dids.into_iter().collect(),
    })
}

fn decimal_order(left: &str, right: &str) -> std::cmp::Ordering {
    left.len()
        .cmp(&right.len())
        .then_with(|| left.as_bytes().cmp(right.as_bytes()))
}

fn unix_time_i64() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .try_into()
        .unwrap_or(i64::MAX)
}

fn sync_error(code: &str, message: impl Into<String>) -> crate::ImError {
    crate::ImError::Service {
        status_code: None,
        code: Some(code.to_owned()),
        message: message.into(),
        data: None,
    }
}

fn validate_required(field: &str, value: &str) -> crate::ImResult<()> {
    if value.trim().is_empty() || value.trim() != value {
        return Err(crate::ImError::invalid_input(
            Some(field.to_owned()),
            format!("{field} must be a non-empty canonical string"),
        ));
    }
    Ok(())
}

pub(crate) fn validate_decimal(field: &str, value: &str) -> crate::ImResult<()> {
    if value.is_empty()
        || !value.bytes().all(|byte| byte.is_ascii_digit())
        || (value.len() > 1 && value.starts_with('0'))
    {
        return Err(crate::ImError::invalid_input(
            Some(field.to_owned()),
            format!("{field} must be a canonical non-negative decimal string"),
        ));
    }
    Ok(())
}

pub(crate) fn validate_positive_decimal(field: &str, value: &str) -> crate::ImResult<()> {
    validate_decimal(field, value)?;
    if value == "0" {
        return Err(crate::ImError::invalid_input(
            Some(field.to_owned()),
            format!("{field} must be a canonical positive decimal string"),
        ));
    }
    Ok(())
}

pub(crate) fn compare_decimal(left: &str, right: &str) -> crate::ImResult<std::cmp::Ordering> {
    validate_decimal("left_decimal", left)?;
    validate_decimal("right_decimal", right)?;
    Ok(left
        .len()
        .cmp(&right.len())
        .then_with(|| left.as_bytes().cmp(right.as_bytes())))
}

fn compare_stream_position(
    left_epoch: &str,
    left_seq: &str,
    right_epoch: &str,
    right_seq: &str,
) -> crate::ImResult<std::cmp::Ordering> {
    let epoch_order = compare_decimal(left_epoch, right_epoch)?;
    if epoch_order == std::cmp::Ordering::Equal {
        compare_decimal(left_seq, right_seq)
    } else {
        Ok(epoch_order)
    }
}

fn subtract_small_decimal(value: &str, amount: u32) -> crate::ImResult<Option<String>> {
    validate_decimal("decimal", value)?;
    let amount = amount.to_string();
    if compare_decimal(value, &amount)? != std::cmp::Ordering::Greater {
        return Ok(None);
    }
    let mut digits = value.bytes().map(|byte| byte - b'0').collect::<Vec<_>>();
    let mut remaining = amount
        .bytes()
        .rev()
        .map(|byte| byte - b'0')
        .chain(std::iter::repeat(0));
    let mut borrow = 0i16;
    for digit in digits.iter_mut().rev() {
        let subtrahend = i16::from(remaining.next().unwrap_or_default()) + borrow;
        let current = i16::from(*digit);
        if current < subtrahend {
            *digit = u8::try_from(current + 10 - subtrahend).unwrap_or_default();
            borrow = 1;
        } else {
            *digit = u8::try_from(current - subtrahend).unwrap_or_default();
            borrow = 0;
        }
    }
    let first_non_zero = digits
        .iter()
        .position(|digit| *digit != 0)
        .unwrap_or(digits.len() - 1);
    Ok(Some(
        digits[first_non_zero..]
            .iter()
            .map(|digit| char::from(b'0' + *digit))
            .collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding() -> IdentityAccountBinding {
        IdentityAccountBinding {
            owner_identity_id: "owner-1".to_owned(),
            account_id: "account-1".to_owned(),
            handle_scope: Some("alice.awiki.info".to_owned()),
            current_did: "did:wba:awiki.info:user:alice".to_owned(),
            protocol_device_id: "device-1".to_owned(),
            identity_generation: "100000000000000000000000000000000000001".to_owned(),
            device_auth_generation: "2".to_owned(),
            created_at: 1,
            updated_at: 1,
        }
    }

    #[test]
    fn group_state_message_projection_is_closed_to_one_unbound_system_record() {
        let system_message = super::super::messages::MessageRecord {
            msg_id: "did:example:group:7".to_owned(),
            owner_identity_id: "owner-1".to_owned(),
            group_id: "did:example:group".to_owned(),
            group_did: "did:example:group".to_owned(),
            content_type: "application/json".to_owned(),
            content: serde_json::json!({
                "schema": "awiki.group.system_event.v1",
                "type": "member_added",
                "group_did": "did:example:group",
                "group_event_seq": "7",
                "sync_event_type": "group.member_changed"
            })
            .to_string(),
            server_seq: Some(7),
            is_read: true,
            metadata: serde_json::json!({"message_role": "group_system_event"}).to_string(),
            wire_thread_kind: "group".to_owned(),
            wire_thread_ref: "did:example:group".to_owned(),
            ..Default::default()
        };
        for event_type in ["group.member_changed", "group.profile_updated"] {
            let mut system_message = system_message.clone();
            let mut payload =
                serde_json::from_str::<serde_json::Value>(&system_message.content).unwrap();
            payload["sync_event_type"] = serde_json::Value::String(event_type.to_owned());
            system_message.content = payload.to_string();
            let event = DeltaApplyEventV2 {
                event_type: event_type.to_owned(),
                messages: vec![system_message],
                ..Default::default()
            };
            assert_eq!(
                message_event_thread_binding(&event, "owner-1").unwrap(),
                None
            );

            let mut unread = event.clone();
            unread.messages[0].is_read = false;
            assert!(matches!(
                message_event_thread_binding(&unread, "owner-1"),
                Err(crate::ImError::Service {
                    code: Some(code),
                    ..
                }) if code == "SYNC_INVALID_PAGE"
            ));

            let mut with_binding = event.clone();
            with_binding.thread_bindings.push(SyncThreadBinding {
                owner_identity_id: "owner-1".to_owned(),
                remote_thread_key: "did:example:group".to_owned(),
                thread_kind: "group".to_owned(),
                conversation_id: "group:did:example:group".to_owned(),
                updated_at: 1,
            });
            assert!(matches!(
                message_event_thread_binding(&with_binding, "owner-1"),
                Err(crate::ImError::Service {
                    code: Some(code),
                    ..
                }) if code == "SYNC_INVALID_PAGE"
            ));
        }

        let unrelated = DeltaApplyEventV2 {
            event_type: "message.read_state_updated".to_owned(),
            messages: vec![system_message],
            ..Default::default()
        };
        assert!(matches!(
            message_event_thread_binding(&unrelated, "owner-1"),
            Err(crate::ImError::Service {
                code: Some(code),
                ..
            }) if code == "SYNC_INVALID_PAGE"
        ));
    }

    #[test]
    fn v2_repository_preserves_decimal_strings_and_has_no_implicit_cursor() {
        let db = Connection::open_in_memory().unwrap();
        db.pragma_update(None, "foreign_keys", "ON").unwrap();
        create_schema(&db).unwrap();
        let binding = binding();
        upsert_identity_account_binding(&db, &binding).unwrap();

        assert!(matches!(
            load_message_sync_state(&db, &binding.owner_identity_id).unwrap(),
            MessageSyncStateAccess::BootstrapRequired(MessageSyncBootstrapFence {
                reason: MessageSyncBootstrapReason::MissingState,
                stored_device_auth_generation: None,
                ..
            })
        ));
        let state = MessageSyncState {
            owner_identity_id: binding.owner_identity_id.clone(),
            account_id: binding.account_id.clone(),
            protocol_device_id: binding.protocol_device_id.clone(),
            device_auth_generation: binding.device_auth_generation.clone(),
            stream_epoch: "99999999999999999999999999999999999999".to_owned(),
            scan_seq: "123456789012345678901234567890123456789".to_owned(),
            bootstrap_state: "tail_bootstrapped".to_owned(),
            last_server_time: None,
            last_success_at: None,
            last_error_code: None,
            metadata_json: None,
            updated_at: 2,
        };
        bootstrap_message_sync_state(&db, &state).unwrap();
        assert_eq!(
            load_message_sync_state(&db, &binding.owner_identity_id).unwrap(),
            MessageSyncStateAccess::Ready(state)
        );
    }

    #[test]
    fn system_notification_delta_is_atomic_idempotent_and_projects_no_chat() {
        let db = Connection::open_in_memory().unwrap();
        db.pragma_update(None, "foreign_keys", "ON").unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let binding = binding();
        upsert_identity_account_binding(&db, &binding).unwrap();
        bootstrap_message_sync_state(
            &db,
            &MessageSyncState {
                owner_identity_id: binding.owner_identity_id.clone(),
                account_id: binding.account_id.clone(),
                protocol_device_id: binding.protocol_device_id.clone(),
                device_auth_generation: binding.device_auth_generation.clone(),
                stream_epoch: "1".to_owned(),
                scan_seq: "0".to_owned(),
                bootstrap_state: "active".to_owned(),
                last_server_time: None,
                last_success_at: Some(1),
                last_error_code: None,
                metadata_json: None,
                updated_at: 1,
            },
        )
        .unwrap();

        let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/multi_device_v1/system-notification-v1.json");
        let fixture: serde_json::Value =
            serde_json::from_slice(&std::fs::read(fixture_path).unwrap()).unwrap();
        let mut request = fixture["p3_vector"]["request"].clone();
        request["method"] = serde_json::Value::String("direct.incoming".to_owned());
        let envelope =
            crate::internal::system_notification::wire::parse_envelope(&request).unwrap();
        let received_at = chrono::DateTime::parse_from_rfc3339("2026-07-23T02:00:01Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let input = |proof_hash: &str| {
            crate::internal::system_notification::store::SystemNotificationApplyInput {
                owner_identity_id: binding.owner_identity_id.clone(),
                owner_did: binding.current_did.clone(),
                protocol_device_id: binding.protocol_device_id.clone(),
                verified:
                    crate::internal::system_notification::verify::VerifiedSystemNotification {
                        envelope: envelope.clone(),
                        payload_hash: "sha256:payload".to_owned(),
                        proof_hash: proof_hash.to_owned(),
                    },
                received_at,
            }
        };
        let delta =
            |sync_event_id: &str, next_scan_seq: &str, proof_hash: &str| DeltaApplyInputV2 {
                owner_identity_id: binding.owner_identity_id.clone(),
                owner_did: binding.current_did.clone(),
                account_id: binding.account_id.clone(),
                protocol_device_id: binding.protocol_device_id.clone(),
                device_auth_generation: binding.device_auth_generation.clone(),
                stream_epoch: "1".to_owned(),
                next_scan_seq: next_scan_seq.to_owned(),
                server_time: "2026-07-23T02:00:01Z".to_owned(),
                events: vec![DeltaApplyEventV2 {
                    event_id: sync_event_id.to_owned(),
                    event_seq: next_scan_seq.to_owned(),
                    event_type: "system.notification".to_owned(),
                    system_notification: Some(input(proof_hash)),
                    ..DeltaApplyEventV2::default()
                }],
            };

        let outcome = apply_delta_v2(&db, delta("sync-event-1", "1", "sha256:proof")).unwrap();
        assert_eq!(outcome.applied_event_ids, ["sync-event-1"]);
        assert_eq!(outcome.committed_system_notifications.len(), 1);
        assert!(outcome.projected_message_event_ids.is_empty());
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM messages", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM system_notification_receipts",
                [],
                |row| { row.get::<_, i64>(0) }
            )
            .unwrap(),
            1
        );
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM system_notification_join_state",
                [],
                |row| { row.get::<_, i64>(0) }
            )
            .unwrap(),
            1
        );

        let duplicate = apply_delta_v2(&db, delta("sync-event-1", "1", "sha256:proof")).unwrap();
        assert_eq!(duplicate.duplicate_events, 1);
        assert!(duplicate.committed_system_notifications.is_empty());

        let error = apply_delta_v2(&db, delta("sync-event-2", "2", "sha256:conflict")).unwrap_err();
        assert!(matches!(error, crate::ImError::Service { .. }));
        let MessageSyncStateAccess::Ready(state) =
            load_message_sync_state(&db, &binding.owner_identity_id).unwrap()
        else {
            panic!("failed delta must preserve the ready cursor")
        };
        assert_eq!(state.scan_seq, "1");
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM sync_applied_events", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
            1
        );
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM system_notification_receipts",
                [],
                |row| { row.get::<_, i64>(0) }
            )
            .unwrap(),
            1
        );
    }

    #[test]
    fn message_delta_binds_remote_thread_to_resolved_persona_conversation() {
        let mut db = Connection::open_in_memory().unwrap();
        db.pragma_update(None, "foreign_keys", "ON").unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let binding = binding();
        upsert_identity_account_binding(&db, &binding).unwrap();
        bootstrap_message_sync_state(
            &db,
            &MessageSyncState {
                owner_identity_id: binding.owner_identity_id.clone(),
                account_id: binding.account_id.clone(),
                protocol_device_id: binding.protocol_device_id.clone(),
                device_auth_generation: binding.device_auth_generation.clone(),
                stream_epoch: "1".to_owned(),
                scan_seq: "0".to_owned(),
                bootstrap_state: "active".to_owned(),
                last_server_time: None,
                last_success_at: Some(1),
                last_error_code: None,
                metadata_json: None,
                updated_at: 1,
            },
        )
        .unwrap();
        let peer_did = "did:wba:awiki.info:user:bob";
        let canonical_conversation_id = super::super::peer_personas::project_verified_handle(
            &mut db,
            &binding.owner_identity_id,
            &binding.current_did,
            &crate::directory::HandleLookupResult {
                handle: crate::ids::Handle::parse("bob.awiki.info", "").unwrap(),
                did: crate::ids::Did::parse(peer_did).unwrap(),
                user_id: "user-bob".to_owned(),
                domain: Some("awiki.info".to_owned()),
                status: Some("active".to_owned()),
                binding_generation: Some("1".to_owned()),
                profile: None,
                warnings: Vec::new(),
            },
        )
        .unwrap();
        let provisional_conversation_id =
            super::super::owner_scope::direct_conversation_id(peer_did);
        assert_ne!(canonical_conversation_id, provisional_conversation_id);
        upsert_sync_thread_binding(
            &db,
            &SyncThreadBinding {
                owner_identity_id: binding.owner_identity_id.clone(),
                remote_thread_key: "remote-thread-1".to_owned(),
                thread_kind: "direct".to_owned(),
                conversation_id: provisional_conversation_id.clone(),
                updated_at: 1,
            },
        )
        .unwrap();

        apply_delta_v2(
            &db,
            DeltaApplyInputV2 {
                owner_identity_id: binding.owner_identity_id.clone(),
                owner_did: binding.current_did.clone(),
                account_id: binding.account_id.clone(),
                protocol_device_id: binding.protocol_device_id.clone(),
                device_auth_generation: binding.device_auth_generation.clone(),
                stream_epoch: "1".to_owned(),
                next_scan_seq: "1".to_owned(),
                server_time: "2026-07-31T00:00:00Z".to_owned(),
                events: vec![DeltaApplyEventV2 {
                    event_id: "event-1".to_owned(),
                    event_seq: "1".to_owned(),
                    event_type: "message.created".to_owned(),
                    messages: vec![super::super::messages::MessageRecord {
                        msg_id: "message-1".to_owned(),
                        owner_identity_id: binding.owner_identity_id.clone(),
                        owner_did: binding.current_did.clone(),
                        conversation_id: provisional_conversation_id.clone(),
                        thread_id: provisional_conversation_id.clone(),
                        direction: 0,
                        sender_did: peer_did.to_owned(),
                        receiver_did: binding.current_did.clone(),
                        content_type: "text/plain".to_owned(),
                        content: "hello".to_owned(),
                        server_seq: Some(1),
                        sent_at: "2026-07-31T00:00:00Z".to_owned(),
                        stored_at: "2026-07-31T00:00:00Z".to_owned(),
                        credential_name: binding.owner_identity_id.clone(),
                        ..super::super::messages::MessageRecord::default()
                    }
                    .with_resolved_wire_thread("direct", peer_did)],
                    thread_bindings: vec![SyncThreadBinding {
                        owner_identity_id: binding.owner_identity_id.clone(),
                        remote_thread_key: "remote-thread-1".to_owned(),
                        thread_kind: "direct".to_owned(),
                        conversation_id: provisional_conversation_id,
                        updated_at: 2,
                    }],
                    ..DeltaApplyEventV2::default()
                }],
            },
        )
        .unwrap();

        let stored_message_conversation = db
            .query_row(
                "SELECT conversation_id FROM messages
                 WHERE owner_identity_id = ?1 AND msg_id = 'message-1'",
                [&binding.owner_identity_id],
                |row| row.get::<_, String>(0),
            )
            .unwrap();
        let stored_binding_conversation = db
            .query_row(
                "SELECT conversation_id FROM sync_thread_bindings
                 WHERE owner_identity_id = ?1
                   AND remote_thread_key = 'remote-thread-1'",
                [&binding.owner_identity_id],
                |row| row.get::<_, String>(0),
            )
            .unwrap();
        assert_eq!(stored_message_conversation, canonical_conversation_id);
        assert_eq!(stored_binding_conversation, canonical_conversation_id);
        assert!(load_sync_thread_binding_for_conversation(
            &db,
            &binding.owner_identity_id,
            &canonical_conversation_id,
            "direct",
        )
        .unwrap()
        .is_some());
        let conflicting = upsert_sync_thread_binding(
            &db,
            &SyncThreadBinding {
                owner_identity_id: binding.owner_identity_id.clone(),
                remote_thread_key: "remote-thread-1".to_owned(),
                thread_kind: "direct".to_owned(),
                conversation_id: "dm:peer-scope:v1:other".to_owned(),
                updated_at: 3,
            },
        )
        .unwrap_err();
        assert!(matches!(
            conflicting,
            crate::ImError::Service {
                code: Some(code),
                ..
            } if code == "SYNC_THREAD_BINDING_CONFLICT"
        ));
    }

    #[test]
    fn direct_thread_binding_rotation_replaces_remote_key_without_rewriting_history() {
        let db = Connection::open_in_memory().unwrap();
        db.pragma_update(None, "foreign_keys", "ON").unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let account = binding();
        upsert_identity_account_binding(&db, &account).unwrap();
        let conversation_id = "dm:peer-scope:v1:stable-controller";
        upsert_sync_thread_binding(
            &db,
            &SyncThreadBinding {
                owner_identity_id: account.owner_identity_id.clone(),
                remote_thread_key: "remote-thread-d1".to_owned(),
                thread_kind: "direct".to_owned(),
                conversation_id: conversation_id.to_owned(),
                updated_at: 1,
            },
        )
        .unwrap();
        db.execute(
            "INSERT INTO messages
                (msg_id, owner_identity_id, owner_did, thread_id, conversation_id,
                 wire_thread_kind, wire_thread_ref, wire_identity_resolution_state,
                 content_type, stored_at)
             VALUES ('history-d1', ?1, ?2, ?3, ?3, 'direct',
                     'did:example:controller-d1', 'resolved', 'text/plain', '1')",
            params![
                account.owner_identity_id,
                account.current_did,
                conversation_id
            ],
        )
        .unwrap();

        upsert_sync_thread_binding(
            &db,
            &SyncThreadBinding {
                owner_identity_id: account.owner_identity_id.clone(),
                remote_thread_key: "remote-thread-d2".to_owned(),
                thread_kind: "direct".to_owned(),
                conversation_id: conversation_id.to_owned(),
                updated_at: 2,
            },
        )
        .unwrap();

        assert_eq!(
            db.query_row(
                "SELECT remote_thread_key || '|' || updated_at
                 FROM sync_thread_bindings
                 WHERE owner_identity_id = ?1 AND conversation_id = ?2",
                params![account.owner_identity_id, conversation_id],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
            "remote-thread-d2|2"
        );
        assert_eq!(
            db.query_row(
                "SELECT wire_thread_ref FROM messages
                 WHERE owner_identity_id = ?1 AND msg_id = 'history-d1'",
                [&account.owner_identity_id],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
            "did:example:controller-d1"
        );
    }

    #[test]
    fn sync_client_instance_id_reuses_failed_retry_and_changes_with_new_database() {
        let first_file = tempfile::NamedTempFile::new().unwrap();
        let (first, retry) = {
            let db = Connection::open(first_file.path()).unwrap();
            create_schema(&db).unwrap();
            (
                load_or_create_sync_client_instance_id(&db, "owner-1").unwrap(),
                load_or_create_sync_client_instance_id(&db, "owner-1").unwrap(),
            )
        };
        assert_eq!(
            retry, first,
            "a failed bootstrap retry must reuse its anchor"
        );
        let reopened = {
            let db = Connection::open(first_file.path()).unwrap();
            create_schema(&db).unwrap();
            load_or_create_sync_client_instance_id(&db, "owner-1").unwrap()
        };
        assert_eq!(reopened, first);
        assert!(first.starts_with("core-installation-"));
        for forbidden in ["owner-1", "account-1", "device-1"] {
            assert!(!first.contains(forbidden));
        }
        let other_owner = {
            let db = Connection::open(first_file.path()).unwrap();
            create_schema(&db).unwrap();
            load_or_create_sync_client_instance_id(&db, "owner-2").unwrap()
        };
        assert_ne!(other_owner, first);

        let second_file = tempfile::NamedTempFile::new().unwrap();
        let second = {
            let db = Connection::open(second_file.path()).unwrap();
            create_schema(&db).unwrap();
            load_or_create_sync_client_instance_id(&db, "owner-1").unwrap()
        };
        assert_ne!(second, first);
    }

    #[test]
    fn v2_unresolved_message_commits_cursor_and_replays_canonical_thread_binding() {
        let mut db = Connection::open_in_memory().unwrap();
        db.pragma_update(None, "foreign_keys", "ON").unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let binding = binding();
        upsert_identity_account_binding(&db, &binding).unwrap();
        let state = MessageSyncState {
            owner_identity_id: binding.owner_identity_id.clone(),
            account_id: binding.account_id.clone(),
            protocol_device_id: binding.protocol_device_id.clone(),
            device_auth_generation: binding.device_auth_generation.clone(),
            stream_epoch: "1".to_owned(),
            scan_seq: "10".to_owned(),
            bootstrap_state: "active".to_owned(),
            last_server_time: None,
            last_success_at: Some(1),
            last_error_code: None,
            metadata_json: None,
            updated_at: 1,
        };
        bootstrap_message_sync_state(&db, &state).unwrap();
        let unresolved = super::super::messages::MessageRecord {
            msg_id: "message-11".to_owned(),
            owner_identity_id: binding.owner_identity_id.clone(),
            owner_did: binding.current_did.clone(),
            conversation_id: "dm:did:example:unknown".to_owned(),
            thread_id: "dm:did:example:unknown".to_owned(),
            direction: 0,
            sender_did: "did:example:unknown".to_owned(),
            receiver_did: binding.current_did.clone(),
            content_type: "text/plain".to_owned(),
            content: "hello".to_owned(),
            sent_at: "2026-07-28T10:00:00Z".to_owned(),
            stored_at: "2026-07-28T10:00:00Z".to_owned(),
            credential_name: binding.owner_identity_id.clone(),
            ..super::super::messages::MessageRecord::default()
        }
        .with_resolved_wire_thread("direct", "did:example:unknown");

        let outcome = apply_delta_v2(
            &db,
            DeltaApplyInputV2 {
                owner_identity_id: binding.owner_identity_id.clone(),
                owner_did: binding.current_did.clone(),
                account_id: binding.account_id.clone(),
                protocol_device_id: binding.protocol_device_id.clone(),
                device_auth_generation: binding.device_auth_generation.clone(),
                stream_epoch: "1".to_owned(),
                next_scan_seq: "11".to_owned(),
                server_time: "2026-07-28T10:00:00Z".to_owned(),
                events: vec![DeltaApplyEventV2 {
                    event_id: "event-11".to_owned(),
                    event_seq: "11".to_owned(),
                    event_type: "message.created".to_owned(),
                    messages: vec![unresolved],
                    groups: Vec::new(),
                    thread_bindings: vec![SyncThreadBinding {
                        owner_identity_id: binding.owner_identity_id.clone(),
                        remote_thread_key: "remote-thread-unknown".to_owned(),
                        thread_kind: "direct".to_owned(),
                        conversation_id: "dm:did:example:unknown".to_owned(),
                        updated_at: 11,
                    }],
                    read_states: Vec::new(),
                    system_notification: None,
                }],
            },
        )
        .unwrap();
        assert_eq!(outcome.applied_event_ids, ["event-11"]);
        assert!(outcome.projected_message_event_ids.is_empty());
        assert_eq!(outcome.backlogged_messages, 1);
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM sync_applied_events
                 WHERE owner_identity_id = ?1",
                [&binding.owner_identity_id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            1
        );
        let MessageSyncStateAccess::Ready(applied_state) =
            load_message_sync_state(&db, &binding.owner_identity_id).unwrap()
        else {
            panic!("expected ready v2 sync state");
        };
        assert_eq!(applied_state.scan_seq, "11");
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM messages", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM sync_thread_bindings", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
            0,
            "a provisional DID conversation binding must not become durable"
        );
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM inbound_resolution_thread_bindings",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            1
        );

        let canonical_conversation_id = super::super::peer_personas::project_verified_handle(
            &mut db,
            &binding.owner_identity_id,
            &binding.current_did,
            &crate::directory::HandleLookupResult {
                handle: crate::ids::Handle::parse("unknown.awiki.info", "").unwrap(),
                did: crate::ids::Did::parse("did:example:unknown").unwrap(),
                user_id: "user-unknown".to_owned(),
                domain: Some("awiki.info".to_owned()),
                status: Some("active".to_owned()),
                binding_generation: Some("1".to_owned()),
                profile: None,
                warnings: Vec::new(),
            },
        )
        .unwrap();
        let replayed: (String, String) = db
            .query_row(
                "SELECT m.conversation_id, b.conversation_id
                 FROM messages AS m
                 JOIN sync_thread_bindings AS b
                   ON b.owner_identity_id = m.owner_identity_id
                  AND b.remote_thread_key = 'remote-thread-unknown'
                 WHERE m.owner_identity_id = ?1 AND m.msg_id = 'message-11'",
                [&binding.owner_identity_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            replayed,
            (canonical_conversation_id.clone(), canonical_conversation_id)
        );
        assert_eq!(
            super::super::inbound_resolution_backlog::pending_count(
                &db,
                &binding.owner_identity_id,
            )
            .unwrap(),
            0
        );
    }

    #[test]
    fn v2_empty_sparse_page_advances_scan_cursor_atomically() {
        let db = Connection::open_in_memory().unwrap();
        db.pragma_update(None, "foreign_keys", "ON").unwrap();
        create_schema(&db).unwrap();
        let binding = binding();
        upsert_identity_account_binding(&db, &binding).unwrap();
        let state = MessageSyncState {
            owner_identity_id: binding.owner_identity_id.clone(),
            account_id: binding.account_id.clone(),
            protocol_device_id: binding.protocol_device_id.clone(),
            device_auth_generation: binding.device_auth_generation.clone(),
            stream_epoch: "1".to_owned(),
            scan_seq: "10".to_owned(),
            bootstrap_state: "active".to_owned(),
            last_server_time: None,
            last_success_at: Some(1),
            last_error_code: None,
            metadata_json: None,
            updated_at: 1,
        };
        bootstrap_message_sync_state(&db, &state).unwrap();

        let outcome = apply_delta_v2(
            &db,
            DeltaApplyInputV2 {
                owner_identity_id: binding.owner_identity_id.clone(),
                owner_did: binding.current_did.clone(),
                account_id: binding.account_id.clone(),
                protocol_device_id: binding.protocol_device_id.clone(),
                device_auth_generation: binding.device_auth_generation.clone(),
                stream_epoch: "1".to_owned(),
                next_scan_seq: "25".to_owned(),
                server_time: "2026-07-28T10:00:00Z".to_owned(),
                events: Vec::new(),
            },
        )
        .unwrap();
        assert!(outcome.applied_event_ids.is_empty());
        let MessageSyncStateAccess::Ready(advanced) =
            load_message_sync_state(&db, &binding.owner_identity_id).unwrap()
        else {
            panic!("empty sparse page must leave the v2 cursor ready");
        };
        assert_eq!(advanced.scan_seq, "25");
    }

    #[test]
    fn v2_bootstrap_failure_rolls_back_binding_group_and_cursor() {
        let db = Connection::open_in_memory().unwrap();
        db.pragma_update(None, "foreign_keys", "ON").unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let binding = binding();
        let state = MessageSyncState {
            owner_identity_id: binding.owner_identity_id.clone(),
            account_id: binding.account_id.clone(),
            protocol_device_id: binding.protocol_device_id.clone(),
            device_auth_generation: binding.device_auth_generation.clone(),
            stream_epoch: "1".to_owned(),
            scan_seq: "10".to_owned(),
            bootstrap_state: "tail_bootstrapped".to_owned(),
            last_server_time: Some("2026-07-28T10:00:00Z".to_owned()),
            last_success_at: Some(1),
            last_error_code: None,
            metadata_json: None,
            updated_at: 1,
        };
        let invalid_group = super::super::groups::GroupRecord {
            owner_identity_id: binding.owner_identity_id.clone(),
            group_id: "did:example:group".to_owned(),
            group_did: "did:example:group".to_owned(),
            owner_did: String::new(),
            ..super::super::groups::GroupRecord::default()
        };

        assert!(apply_bootstrap_v2(
            &db,
            BootstrapApplyInputV2 {
                binding: binding.clone(),
                state,
                groups: vec![invalid_group],
                read_states: Vec::new(),
            }
        )
        .is_err());
        assert_eq!(
            load_identity_account_binding(&db, &binding.owner_identity_id).unwrap(),
            None
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM groups", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM message_sync_state", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
            0
        );
    }

    #[test]
    fn auth_generation_change_fences_old_cursor_until_explicit_bootstrap() {
        let db = Connection::open_in_memory().unwrap();
        db.pragma_update(None, "foreign_keys", "ON").unwrap();
        create_schema(&db).unwrap();
        let mut active_binding = binding();
        upsert_identity_account_binding(&db, &active_binding).unwrap();
        let old_state = MessageSyncState {
            owner_identity_id: active_binding.owner_identity_id.clone(),
            account_id: active_binding.account_id.clone(),
            protocol_device_id: active_binding.protocol_device_id.clone(),
            device_auth_generation: active_binding.device_auth_generation.clone(),
            stream_epoch: "3".to_owned(),
            scan_seq: "400".to_owned(),
            bootstrap_state: "active".to_owned(),
            last_server_time: None,
            last_success_at: Some(1),
            last_error_code: None,
            metadata_json: None,
            updated_at: 1,
        };
        bootstrap_message_sync_state(&db, &old_state).unwrap();
        upsert_recovery_state(
            &db,
            &RecoveryState {
                owner_identity_id: active_binding.owner_identity_id.clone(),
                mode: "compact_recovery".to_owned(),
                requested_from_epoch: "1".to_owned(),
                requested_from_seq: "25".to_owned(),
                recovery_id_hash: Some("sha256:old-generation".to_owned()),
                snapshot_scan_seq: None,
                status: "applying".to_owned(),
                retry_count: 0,
                last_error_code: Some("interrupted".to_owned()),
                started_at: 1,
                updated_at: 1,
            },
        )
        .unwrap();

        active_binding.device_auth_generation = "3".to_owned();
        active_binding.updated_at = 2;
        upsert_identity_account_binding(&db, &active_binding).unwrap();

        let fence = load_message_sync_state(&db, &active_binding.owner_identity_id).unwrap();
        assert_eq!(
            fence,
            MessageSyncStateAccess::BootstrapRequired(MessageSyncBootstrapFence {
                owner_identity_id: active_binding.owner_identity_id.clone(),
                account_id: active_binding.account_id.clone(),
                protocol_device_id: active_binding.protocol_device_id.clone(),
                active_device_auth_generation: "3".to_owned(),
                stored_device_auth_generation: Some("2".to_owned()),
                stored_stream_epoch: Some("3".to_owned()),
                requested_stream_epoch: None,
                reason: MessageSyncBootstrapReason::DeviceAuthGenerationChanged,
            })
        );

        let mut attempted_advance = old_state.clone();
        attempted_advance.scan_seq = "401".to_owned();
        assert!(matches!(
            advance_message_sync_state(&db, &attempted_advance).unwrap(),
            MessageSyncStateAccess::BootstrapRequired(MessageSyncBootstrapFence {
                reason: MessageSyncBootstrapReason::DeviceAuthGenerationChanged,
                ..
            })
        ));
        assert_eq!(
            load_message_sync_state_row(&db, &active_binding.owner_identity_id)
                .unwrap()
                .unwrap()
                .scan_seq,
            "400",
            "a stale generation must not advance the stored cursor"
        );

        let new_state = MessageSyncState {
            device_auth_generation: active_binding.device_auth_generation.clone(),
            stream_epoch: "4".to_owned(),
            scan_seq: "900".to_owned(),
            bootstrap_state: "tail_bootstrapped".to_owned(),
            updated_at: 3,
            ..old_state
        };
        bootstrap_message_sync_state(&db, &new_state).unwrap();
        assert_eq!(
            load_message_sync_state(&db, &active_binding.owner_identity_id).unwrap(),
            MessageSyncStateAccess::Ready(new_state.clone())
        );
        let superseded_recovery = load_recovery_state(&db, &active_binding.owner_identity_id)
            .unwrap()
            .unwrap();
        assert_eq!(superseded_recovery.status, "completed");
        assert_eq!(
            superseded_recovery.snapshot_scan_seq.as_deref(),
            Some("900")
        );
        assert_eq!(superseded_recovery.last_error_code, None);

        let mut advanced = new_state;
        advanced.scan_seq = "901".to_owned();
        advanced.bootstrap_state = "active".to_owned();
        advanced.updated_at = 4;
        assert_eq!(
            advance_message_sync_state(&db, &advanced).unwrap(),
            MessageSyncStateAccess::Ready(advanced)
        );
    }

    #[test]
    fn advance_rejects_account_device_or_unrelated_generation_mismatch() {
        let db = Connection::open_in_memory().unwrap();
        db.pragma_update(None, "foreign_keys", "ON").unwrap();
        create_schema(&db).unwrap();
        let active_binding = binding();
        upsert_identity_account_binding(&db, &active_binding).unwrap();
        let current = MessageSyncState {
            owner_identity_id: active_binding.owner_identity_id.clone(),
            account_id: active_binding.account_id.clone(),
            protocol_device_id: active_binding.protocol_device_id.clone(),
            device_auth_generation: active_binding.device_auth_generation.clone(),
            stream_epoch: "3".to_owned(),
            scan_seq: "400".to_owned(),
            bootstrap_state: "active".to_owned(),
            last_server_time: None,
            last_success_at: Some(1),
            last_error_code: None,
            metadata_json: None,
            updated_at: 1,
        };
        bootstrap_message_sync_state(&db, &current).unwrap();

        let mut wrong_account = current.clone();
        wrong_account.account_id = "account-other".to_owned();
        wrong_account.scan_seq = "401".to_owned();
        assert!(matches!(
            advance_message_sync_state(&db, &wrong_account),
            Err(crate::ImError::IdentityBindingConflict { .. })
        ));

        let mut wrong_device = current.clone();
        wrong_device.protocol_device_id = "device-other".to_owned();
        wrong_device.scan_seq = "401".to_owned();
        assert!(matches!(
            advance_message_sync_state(&db, &wrong_device),
            Err(crate::ImError::IdentityBindingConflict { .. })
        ));

        let mut unrelated_generation = current.clone();
        unrelated_generation.device_auth_generation = "9".to_owned();
        unrelated_generation.scan_seq = "401".to_owned();
        assert!(matches!(
            advance_message_sync_state(&db, &unrelated_generation),
            Err(crate::ImError::IdentityBindingConflict { .. })
        ));
        assert_eq!(
            load_message_sync_state(&db, &active_binding.owner_identity_id).unwrap(),
            MessageSyncStateAccess::Ready(current)
        );
    }

    #[test]
    fn account_binding_is_conflict_visible_and_generation_is_canonical() {
        let db = Connection::open_in_memory().unwrap();
        create_schema(&db).unwrap();
        let binding = binding();
        upsert_identity_account_binding(&db, &binding).unwrap();

        let mut conflict = binding.clone();
        conflict.account_id = "account-2".to_owned();
        assert!(matches!(
            upsert_identity_account_binding(&db, &conflict),
            Err(crate::ImError::IdentityBindingConflict { .. })
        ));

        let mut invalid = binding;
        invalid.identity_generation = "01".to_owned();
        assert!(matches!(
            upsert_identity_account_binding(&db, &invalid),
            Err(crate::ImError::InvalidInput { .. })
        ));
        invalid.identity_generation = "0".to_owned();
        assert!(matches!(
            upsert_identity_account_binding(&db, &invalid),
            Err(crate::ImError::InvalidInput { .. })
        ));
    }

    #[test]
    fn account_binding_generations_are_monotonic_without_integer_narrowing() {
        let db = Connection::open_in_memory().unwrap();
        create_schema(&db).unwrap();
        let mut current = binding();
        current.identity_generation = "184467440737095516160000000000000000002".to_owned();
        current.device_auth_generation = "184467440737095516160000000000000000004".to_owned();
        upsert_identity_account_binding(&db, &current).unwrap();

        let mut stale_identity = current.clone();
        stale_identity.identity_generation = "184467440737095516160000000000000000001".to_owned();
        assert!(matches!(
            upsert_identity_account_binding(&db, &stale_identity),
            Err(crate::ImError::IdentityBindingConflict { .. })
        ));

        let mut stale_device_auth = current.clone();
        stale_device_auth.device_auth_generation =
            "184467440737095516160000000000000000003".to_owned();
        assert!(matches!(
            upsert_identity_account_binding(&db, &stale_device_auth),
            Err(crate::ImError::IdentityBindingConflict { .. })
        ));

        let mut changed_did_without_generation = current.clone();
        changed_did_without_generation.current_did =
            "did:wba:awiki.info:user:alice:recovered".to_owned();
        assert!(matches!(
            upsert_identity_account_binding(&db, &changed_did_without_generation),
            Err(crate::ImError::IdentityBindingConflict { .. })
        ));

        let mut advanced = changed_did_without_generation;
        advanced.identity_generation = "184467440737095516160000000000000000003".to_owned();
        advanced.device_auth_generation = "184467440737095516160000000000000000005".to_owned();
        upsert_identity_account_binding(&db, &advanced).unwrap();
        assert_eq!(
            load_identity_account_binding(&db, &advanced.owner_identity_id)
                .unwrap()
                .unwrap(),
            advanced
        );
    }

    #[test]
    fn applied_event_receipt_is_idempotent_and_conflict_visible() {
        let db = Connection::open_in_memory().unwrap();
        db.pragma_update(None, "foreign_keys", "ON").unwrap();
        create_schema(&db).unwrap();
        let binding = binding();
        upsert_identity_account_binding(&db, &binding).unwrap();
        let receipt = AppliedEventReceipt {
            owner_identity_id: binding.owner_identity_id,
            event_id: "event-1".to_owned(),
            stream_epoch: "1".to_owned(),
            event_seq: "7".to_owned(),
            applied_at: 1,
        };

        assert!(record_applied_event(&db, &receipt).unwrap());
        assert!(!record_applied_event(&db, &receipt).unwrap());
        let mut conflicting = receipt;
        conflicting.event_seq = "8".to_owned();
        assert!(matches!(
            record_applied_event(&db, &conflicting),
            Err(crate::ImError::IdentityBindingConflict { .. })
        ));
    }

    #[test]
    fn cleanup_keeps_recent_terminal_and_all_live_sync_work() {
        let db = Connection::open_in_memory().unwrap();
        db.pragma_update(None, "foreign_keys", "ON").unwrap();
        create_schema(&db).unwrap();
        let binding = binding();
        upsert_identity_account_binding(&db, &binding).unwrap();
        let now = TERMINAL_SYNC_STATE_RETENTION_SECONDS + 10_000;
        let records = [
            ("old-committed", "committed", 1),
            ("old-permanent", "permanent_failure", 1),
            ("recent-committed", "committed", now),
            ("live-pending", "pending", 1),
            ("live-in-flight", "in_flight", 1),
            ("live-retryable", "retryable", 1),
        ];
        for (index, (id, status, updated_at)) in records.into_iter().enumerate() {
            enqueue_local_mutation(
                &db,
                &LocalMutationRecord {
                    owner_identity_id: binding.owner_identity_id.clone(),
                    mutation_id: id.to_owned(),
                    operation_id: format!("operation-{index}"),
                    mutation_type: "read_state_mark_read".to_owned(),
                    aggregate_id: format!("dm:{index}"),
                    payload_json: "{}".to_owned(),
                    status: status.to_owned(),
                    attempt_count: 0,
                    retry_at: (status == "retryable").then_some(2),
                    in_flight_since: (status == "in_flight").then_some(2),
                    last_error_code: None,
                    created_at: updated_at,
                    updated_at,
                },
            )
            .unwrap();
        }
        upsert_recovery_state(
            &db,
            &RecoveryState {
                owner_identity_id: binding.owner_identity_id.clone(),
                mode: "compact_recovery".to_owned(),
                requested_from_epoch: "1".to_owned(),
                requested_from_seq: "10".to_owned(),
                recovery_id_hash: Some("hash".to_owned()),
                snapshot_scan_seq: Some("20".to_owned()),
                status: "completed".to_owned(),
                retry_count: 0,
                last_error_code: None,
                started_at: 1,
                updated_at: now,
            },
        )
        .unwrap();

        let diagnostics = load_sync_diagnostics(&db, &binding.owner_identity_id).unwrap();
        assert_eq!(diagnostics.pending_mutation_count, 3);
        assert_eq!(diagnostics.pending_count, 1);
        assert_eq!(diagnostics.in_flight_count, 1);
        assert_eq!(diagnostics.retryable_count, 1);
        assert_eq!(diagnostics.permanent_failure_count, 1);
        assert_eq!(diagnostics.next_retry_at, Some(2));
        assert_eq!(diagnostics.recovery_status.as_deref(), Some("completed"));

        let first =
            cleanup_terminal_sync_state(&db, &binding.owner_identity_id, "1", "5000", 100, now)
                .unwrap();
        assert_eq!(first.terminal_mutations_deleted, 2);
        assert!(!first.terminal_recovery_deleted);
        let remaining = db
            .prepare(
                "SELECT mutation_id FROM local_mutation_outbox
                 WHERE owner_identity_id = ?1 ORDER BY mutation_id",
            )
            .unwrap()
            .query_map([&binding.owner_identity_id], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            remaining,
            vec![
                "live-in-flight",
                "live-pending",
                "live-retryable",
                "recent-committed"
            ]
        );

        db.execute(
            "UPDATE sync_recovery_state SET updated_at = 1
             WHERE owner_identity_id = ?1",
            [&binding.owner_identity_id],
        )
        .unwrap();
        let second =
            cleanup_terminal_sync_state(&db, &binding.owner_identity_id, "1", "5000", 100, now)
                .unwrap();
        assert!(second.terminal_recovery_deleted);
        assert!(load_recovery_state(&db, &binding.owner_identity_id)
            .unwrap()
            .is_none());
    }

    #[test]
    fn cleanup_total_batch_never_exceeds_the_production_limit() {
        let db = Connection::open_in_memory().unwrap();
        db.pragma_update(None, "foreign_keys", "ON").unwrap();
        create_schema(&db).unwrap();
        let binding = binding();
        upsert_identity_account_binding(&db, &binding).unwrap();
        let transaction = db.unchecked_transaction().unwrap();
        {
            let mut insert = transaction
                .prepare(
                    "INSERT INTO sync_applied_events
                         (owner_identity_id, event_id, stream_epoch, event_seq, applied_at)
                     VALUES (?1, ?2, '1', ?3, 1)",
                )
                .unwrap();
            for seq in 1..=10_100_i64 {
                insert
                    .execute(params![
                        binding.owner_identity_id,
                        format!("event-{seq}"),
                        seq.to_string()
                    ])
                    .unwrap();
            }
        }
        transaction.commit().unwrap();
        for index in 0..=SYNC_CLEANUP_BATCH_SIZE {
            enqueue_local_mutation(
                &db,
                &LocalMutationRecord {
                    owner_identity_id: binding.owner_identity_id.clone(),
                    mutation_id: format!("old-terminal-{index:03}"),
                    operation_id: format!("operation-{index:03}"),
                    mutation_type: "read_state_mark_read".to_owned(),
                    aggregate_id: format!("dm:{index}"),
                    payload_json: "{}".to_owned(),
                    status: "committed".to_owned(),
                    attempt_count: 0,
                    retry_at: None,
                    in_flight_since: None,
                    last_error_code: None,
                    created_at: 1,
                    updated_at: 1,
                },
            )
            .unwrap();
        }
        upsert_recovery_state(
            &db,
            &RecoveryState {
                owner_identity_id: binding.owner_identity_id.clone(),
                mode: "compact_recovery".to_owned(),
                requested_from_epoch: "1".to_owned(),
                requested_from_seq: "1".to_owned(),
                recovery_id_hash: Some("hash".to_owned()),
                snapshot_scan_seq: Some("1".to_owned()),
                status: "completed".to_owned(),
                retry_count: 0,
                last_error_code: None,
                started_at: 1,
                updated_at: 1,
            },
        )
        .unwrap();

        let outcome = cleanup_terminal_sync_state(
            &db,
            &binding.owner_identity_id,
            "1",
            "20000",
            SYNC_CLEANUP_BATCH_SIZE,
            TERMINAL_SYNC_STATE_RETENTION_SECONDS + 10,
        )
        .unwrap();
        let total_deleted = outcome
            .applied_events_deleted
            .saturating_add(outcome.terminal_mutations_deleted)
            .saturating_add(usize::from(outcome.terminal_recovery_deleted));

        assert_eq!(total_deleted, SYNC_CLEANUP_BATCH_SIZE as usize);
        assert!(total_deleted <= SYNC_CLEANUP_BATCH_SIZE as usize);
        assert_eq!(outcome.applied_events_deleted, 100);
        assert_eq!(outcome.terminal_mutations_deleted, 156);
        assert!(!outcome.terminal_recovery_deleted);
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM local_mutation_outbox
                 WHERE owner_identity_id = ?1",
                [&binding.owner_identity_id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            101
        );
        assert!(load_recovery_state(&db, &binding.owner_identity_id)
            .unwrap()
            .is_some());
    }

    #[test]
    fn cleanup_seven_day_boundary_is_inclusive_and_keeps_newer_rows() {
        let db = Connection::open_in_memory().unwrap();
        db.pragma_update(None, "foreign_keys", "ON").unwrap();
        create_schema(&db).unwrap();
        let binding = binding();
        upsert_identity_account_binding(&db, &binding).unwrap();
        let retention_cutoff = 10_000;
        let now = retention_cutoff + TERMINAL_SYNC_STATE_RETENTION_SECONDS;
        for (mutation_id, updated_at) in [
            ("exactly-seven-days-old", retention_cutoff),
            ("inside-seven-days", retention_cutoff + 1),
        ] {
            enqueue_local_mutation(
                &db,
                &LocalMutationRecord {
                    owner_identity_id: binding.owner_identity_id.clone(),
                    mutation_id: mutation_id.to_owned(),
                    operation_id: format!("operation-{mutation_id}"),
                    mutation_type: "read_state_mark_read".to_owned(),
                    aggregate_id: format!("dm:{mutation_id}"),
                    payload_json: "{}".to_owned(),
                    status: "committed".to_owned(),
                    attempt_count: 0,
                    retry_at: None,
                    in_flight_since: None,
                    last_error_code: None,
                    created_at: updated_at,
                    updated_at,
                },
            )
            .unwrap();
        }

        let outcome =
            cleanup_terminal_sync_state(&db, &binding.owner_identity_id, "1", "5000", 10, now)
                .unwrap();

        assert_eq!(outcome.terminal_mutations_deleted, 1);
        assert_eq!(
            db.query_row(
                "SELECT mutation_id FROM local_mutation_outbox
                 WHERE owner_identity_id = ?1",
                [&binding.owner_identity_id],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
            "inside-seven-days"
        );
    }

    #[test]
    fn cleanup_keeps_recovery_anchor_ahead_of_current_cursor() {
        let db = Connection::open_in_memory().unwrap();
        db.pragma_update(None, "foreign_keys", "ON").unwrap();
        create_schema(&db).unwrap();
        let binding = binding();
        upsert_identity_account_binding(&db, &binding).unwrap();
        upsert_recovery_state(
            &db,
            &RecoveryState {
                owner_identity_id: binding.owner_identity_id.clone(),
                mode: "compact_recovery".to_owned(),
                requested_from_epoch: "2".to_owned(),
                requested_from_seq: "1".to_owned(),
                recovery_id_hash: Some("hash".to_owned()),
                snapshot_scan_seq: None,
                status: "completed".to_owned(),
                retry_count: 0,
                last_error_code: None,
                started_at: 1,
                updated_at: 1,
            },
        )
        .unwrap();

        let outcome = cleanup_terminal_sync_state(
            &db,
            &binding.owner_identity_id,
            "1",
            "999999",
            100,
            TERMINAL_SYNC_STATE_RETENTION_SECONDS + 10,
        )
        .unwrap();

        assert!(!outcome.terminal_recovery_deleted);
        assert!(load_recovery_state(&db, &binding.owner_identity_id)
            .unwrap()
            .is_some());
    }

    #[test]
    fn outbox_rejects_non_read_mutations_and_restart_recovers_transient_rows() {
        let db = Connection::open_in_memory().unwrap();
        db.pragma_update(None, "foreign_keys", "ON").unwrap();
        create_schema(&db).unwrap();
        let binding = binding();
        upsert_identity_account_binding(&db, &binding).unwrap();
        let mut mutation = LocalMutationRecord {
            owner_identity_id: binding.owner_identity_id.clone(),
            mutation_id: "mutation-1".to_owned(),
            operation_id: "operation-1".to_owned(),
            mutation_type: "plain_message_send".to_owned(),
            aggregate_id: "dm:1".to_owned(),
            payload_json: "{}".to_owned(),
            status: "pending".to_owned(),
            attempt_count: 0,
            retry_at: None,
            in_flight_since: None,
            last_error_code: None,
            created_at: 1,
            updated_at: 1,
        };
        assert!(matches!(
            enqueue_local_mutation(&db, &mutation),
            Err(crate::ImError::InvalidInput { .. })
        ));

        mutation.mutation_type = "read_state_mark_read".to_owned();
        mutation.status = "in_flight".to_owned();
        mutation.in_flight_since = Some(1);
        enqueue_local_mutation(&db, &mutation).unwrap();
        upsert_recovery_state(
            &db,
            &RecoveryState {
                owner_identity_id: binding.owner_identity_id.clone(),
                mode: "compact_recovery".to_owned(),
                requested_from_epoch: "1".to_owned(),
                requested_from_seq: "10".to_owned(),
                recovery_id_hash: Some("sha256:test".to_owned()),
                snapshot_scan_seq: None,
                status: "applying".to_owned(),
                retry_count: 0,
                last_error_code: None,
                started_at: 1,
                updated_at: 1,
            },
        )
        .unwrap();

        recover_interrupted_work(&db, 2).unwrap();

        let mutation_state = db
            .query_row(
                "SELECT status, in_flight_since FROM local_mutation_outbox
                 WHERE owner_identity_id = ?1",
                [&binding.owner_identity_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<i64>>(1)?)),
            )
            .unwrap();
        assert_eq!(mutation_state, ("retryable".to_owned(), None));
        assert_eq!(
            load_local_mutation(&db, &binding.owner_identity_id, "mutation-1")
                .unwrap()
                .unwrap()
                .status,
            "retryable"
        );
        assert_eq!(
            load_recovery_state(&db, &binding.owner_identity_id)
                .unwrap()
                .unwrap()
                .status,
            "retryable"
        );
    }

    #[test]
    fn decimal_subtraction_does_not_narrow_large_values() {
        assert_eq!(
            subtract_small_decimal("1000000000000000000000000", 1000).unwrap(),
            Some("999999999999999999999000".to_owned())
        );
        assert_eq!(subtract_small_decimal("1000", 1000).unwrap(), None);
    }

    #[test]
    fn recovery_state_rejects_open_ended_modes_and_statuses() {
        let db = Connection::open_in_memory().unwrap();
        db.pragma_update(None, "foreign_keys", "ON").unwrap();
        create_schema(&db).unwrap();
        let binding = binding();
        upsert_identity_account_binding(&db, &binding).unwrap();
        let mut recovery = RecoveryState {
            owner_identity_id: binding.owner_identity_id,
            mode: "arbitrary_mode".to_owned(),
            requested_from_epoch: "1".to_owned(),
            requested_from_seq: "0".to_owned(),
            recovery_id_hash: None,
            snapshot_scan_seq: None,
            status: "recovering".to_owned(),
            retry_count: 0,
            last_error_code: None,
            started_at: 1,
            updated_at: 1,
        };
        assert!(matches!(
            upsert_recovery_state(&db, &recovery),
            Err(crate::ImError::InvalidInput { .. })
        ));
        recovery.mode = "compact_recovery".to_owned();
        recovery.status = "arbitrary_status".to_owned();
        assert!(matches!(
            upsert_recovery_state(&db, &recovery),
            Err(crate::ImError::InvalidInput { .. })
        ));
    }

    #[test]
    fn pruning_is_bounded_and_preserves_an_active_old_epoch_anchor() {
        let db = Connection::open_in_memory().unwrap();
        db.pragma_update(None, "foreign_keys", "ON").unwrap();
        create_schema(&db).unwrap();
        let binding = binding();
        upsert_identity_account_binding(&db, &binding).unwrap();
        let transaction = db.unchecked_transaction().unwrap();
        {
            let mut insert = transaction
                .prepare(
                    "INSERT INTO sync_applied_events
                         (owner_identity_id, event_id, stream_epoch, event_seq, applied_at)
                     VALUES (?1, ?2, '1', ?3, ?4)",
                )
                .unwrap();
            for seq in 1..=10_050_i64 {
                insert
                    .execute(params![
                        binding.owner_identity_id,
                        format!("event-{seq}"),
                        seq.to_string(),
                        seq
                    ])
                    .unwrap();
            }
        }
        transaction.commit().unwrap();
        upsert_recovery_state(
            &db,
            &RecoveryState {
                owner_identity_id: binding.owner_identity_id.clone(),
                mode: "compact_recovery".to_owned(),
                requested_from_epoch: "1".to_owned(),
                requested_from_seq: "25".to_owned(),
                recovery_id_hash: Some("sha256:old-epoch-anchor".to_owned()),
                snapshot_scan_seq: None,
                status: "applying".to_owned(),
                retry_count: 0,
                last_error_code: None,
                started_at: 1,
                updated_at: 1,
            },
        )
        .unwrap();

        assert_eq!(
            prune_applied_events(&db, &binding.owner_identity_id, "2", "5000", 100).unwrap(),
            24,
            "only receipts strictly before the old-epoch anchor may be deleted"
        );
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM sync_applied_events
                 WHERE owner_identity_id = ?1 AND event_id = 'event-25'",
                [&binding.owner_identity_id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            1
        );
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM sync_applied_events
                 WHERE owner_identity_id = ?1",
                [&binding.owner_identity_id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            10_026
        );

        let mut completed = load_recovery_state(&db, &binding.owner_identity_id)
            .unwrap()
            .unwrap();
        completed.status = "completed".to_owned();
        completed.updated_at = 2;
        upsert_recovery_state(&db, &completed).unwrap();
        assert_eq!(
            prune_applied_events(&db, &binding.owner_identity_id, "2", "5000", 100).unwrap(),
            26
        );
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM sync_applied_events
                 WHERE owner_identity_id = ?1",
                [&binding.owner_identity_id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            APPLIED_EVENT_MIN_RECEIPTS_PER_OWNER
        );
    }

    #[test]
    fn snapshot_unbound_direct_read_state_commits_and_replays_after_message_binding() {
        let db = Connection::open_in_memory().unwrap();
        db.pragma_update(None, "foreign_keys", "ON").unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let binding = binding();
        upsert_identity_account_binding(&db, &binding).unwrap();
        bootstrap_message_sync_state(
            &db,
            &MessageSyncState {
                owner_identity_id: binding.owner_identity_id.clone(),
                account_id: binding.account_id.clone(),
                protocol_device_id: binding.protocol_device_id.clone(),
                device_auth_generation: binding.device_auth_generation.clone(),
                stream_epoch: "1".to_owned(),
                scan_seq: "10".to_owned(),
                bootstrap_state: "active".to_owned(),
                last_server_time: None,
                last_success_at: Some(1),
                last_error_code: None,
                metadata_json: None,
                updated_at: 1,
            },
        )
        .unwrap();
        upsert_recovery_state(
            &db,
            &RecoveryState {
                owner_identity_id: binding.owner_identity_id.clone(),
                mode: "compact_recovery".to_owned(),
                requested_from_epoch: "1".to_owned(),
                requested_from_seq: "10".to_owned(),
                recovery_id_hash: Some("recovery-hash".to_owned()),
                snapshot_scan_seq: Some("50".to_owned()),
                status: "applying".to_owned(),
                retry_count: 0,
                last_error_code: None,
                started_at: 1,
                updated_at: 1,
            },
        )
        .unwrap();
        let remote_read = ReadStateApplyV2 {
            remote_thread_key: "dconv-old-outside-window".to_owned(),
            thread_kind: "direct".to_owned(),
            read_watermark_seq: "991".to_owned(),
            read_watermark_message_id: Some("msg-991".to_owned()),
            state_version: "38".to_owned(),
            occurred_at: "2026-07-28T10:00:00Z".to_owned(),
        };
        apply_snapshot_v2(
            &db,
            SnapshotApplyInputV2 {
                owner_identity_id: binding.owner_identity_id.clone(),
                owner_did: binding.current_did.clone(),
                account_id: binding.account_id.clone(),
                protocol_device_id: binding.protocol_device_id.clone(),
                device_auth_generation: binding.device_auth_generation.clone(),
                expected_stream_epoch: "1".to_owned(),
                expected_scan_seq: "10".to_owned(),
                allow_missing_previous: false,
                recovery_id_hash: "recovery-hash".to_owned(),
                stream_epoch: "1".to_owned(),
                snapshot_scan_seq: "50".to_owned(),
                server_time: "2026-07-28T10:00:01Z".to_owned(),
                events: Vec::new(),
                groups: Vec::new(),
                read_states: vec![remote_read],
            },
        )
        .unwrap();
        let MessageSyncStateAccess::Ready(snapshot_state) =
            load_message_sync_state(&db, &binding.owner_identity_id).unwrap()
        else {
            panic!("snapshot must commit its anchor");
        };
        assert_eq!(snapshot_state.scan_seq, "50");
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM sync_remote_read_states
                 WHERE owner_identity_id = ?1",
                [&binding.owner_identity_id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            1,
            "a current read state outside the 48h/500 message window must be backlogged"
        );

        apply_delta_v2(
            &db,
            DeltaApplyInputV2 {
                owner_identity_id: binding.owner_identity_id.clone(),
                owner_did: binding.current_did.clone(),
                account_id: binding.account_id.clone(),
                protocol_device_id: binding.protocol_device_id.clone(),
                device_auth_generation: binding.device_auth_generation.clone(),
                stream_epoch: "1".to_owned(),
                next_scan_seq: "51".to_owned(),
                server_time: "2026-07-28T10:00:02Z".to_owned(),
                events: vec![DeltaApplyEventV2 {
                    event_id: "event-51".to_owned(),
                    event_seq: "51".to_owned(),
                    event_type: "message.created".to_owned(),
                    thread_bindings: vec![SyncThreadBinding {
                        owner_identity_id: binding.owner_identity_id.clone(),
                        remote_thread_key: "dconv-old-outside-window".to_owned(),
                        thread_kind: "direct".to_owned(),
                        conversation_id: "dm:peer-scope:v1:alice:bob".to_owned(),
                        updated_at: 2,
                    }],
                    ..DeltaApplyEventV2::default()
                }],
            },
        )
        .unwrap();
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM sync_remote_read_states
                 WHERE owner_identity_id = ?1",
                [&binding.owner_identity_id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            0,
            "binding establishment must replay and remove the unresolved read backlog"
        );
        assert_eq!(
            db.query_row(
                "SELECT read_watermark_seq, remote_state_version
                 FROM thread_read_state
                 WHERE owner_identity_id = ?1",
                [&binding.owner_identity_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .unwrap(),
            ("991".to_owned(), "38".to_owned())
        );
    }

    #[test]
    fn group_read_outbox_uses_canonical_id_for_legacy_hydration() {
        let db = Connection::open_in_memory().unwrap();
        db.pragma_update(None, "foreign_keys", "ON").unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let binding = binding();
        upsert_identity_account_binding(&db, &binding).unwrap();
        let group_did = "did:wba:awiki.info:groups:group-read-outbox";
        let conversation_id = format!("group:{group_did}");
        db.execute(
            "INSERT INTO messages
                (msg_id, owner_identity_id, owner_did, thread_id, conversation_id,
                 wire_thread_kind, wire_thread_ref, wire_identity_resolution_state,
                 server_seq, hydration_state, is_e2ee, metadata, stored_at)
             VALUES (?1, ?2, ?3, ?4, ?4, 'group', ?5, 'resolved',
                     30, 'hydrated', 0, ?6, '2026-07-28T10:00:00Z')",
            params![
                format!("{group_did}:30"),
                binding.owner_identity_id,
                binding.current_did,
                conversation_id,
                group_did,
                serde_json::json!({}).to_string(),
            ],
        )
        .unwrap();
        db.execute(
            "INSERT INTO messages
                (msg_id, owner_identity_id, owner_did, thread_id, conversation_id,
                 wire_thread_kind, wire_thread_ref, wire_identity_resolution_state,
                 content_type, content, server_seq, hydration_state, is_e2ee, metadata, stored_at)
             VALUES (?1, ?2, ?3, ?4, ?4, 'group', ?5, 'resolved',
                     'application/json', ?6, 31, 'hydrated', 0, ?7,
                     '2026-07-28T10:00:01Z')",
            params![
                format!("{group_did}:31"),
                binding.owner_identity_id,
                binding.current_did,
                conversation_id,
                group_did,
                serde_json::json!({"schema": "awiki.group.system_event.v1"}).to_string(),
                serde_json::json!({
                    "message_role": "group_system_event",
                    "group_event_seq": "31"
                })
                .to_string(),
            ],
        )
        .unwrap();

        let result = mark_thread_read_and_update_outbox(
            &db,
            &binding.owner_identity_id,
            &binding.current_did,
            super::super::messages::MarkThreadReadWatermarkInput {
                thread: crate::messages::ThreadRef::Group(
                    crate::ids::GroupRef::parse(group_did).unwrap(),
                ),
                read_watermark_message_id: None,
                read_watermark_seq: Some("31".to_owned()),
                read_watermark_at: Some("2026-07-28T10:00:00Z".to_owned()),
                pending_remote_ack: true,
            },
        )
        .unwrap();

        assert_eq!(result.conversation_id, conversation_id);
        assert_eq!(result.remote_thread_key.as_deref(), Some(group_did));
        assert!(result.remote_ack_applicable);
        upsert_sync_thread_binding(
            &db,
            &SyncThreadBinding {
                owner_identity_id: binding.owner_identity_id.clone(),
                remote_thread_key: group_did.to_owned(),
                thread_kind: "group".to_owned(),
                conversation_id: conversation_id.clone(),
                updated_at: 1,
            },
        )
        .unwrap();
        assert_eq!(
            db.query_row(
                "SELECT aggregate_id || '|' ||
                        json_extract(payload_json, '$.remote_thread_key') || '|' ||
                        json_extract(payload_json, '$.read_watermark_seq') || '|' ||
                        json_extract(payload_json, '$.read_watermark_message_id')
                 FROM local_mutation_outbox
                 WHERE owner_identity_id = ?1",
                [&binding.owner_identity_id],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
            format!("{group_did}|{group_did}|30|{group_did}:30")
        );
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM local_mutation_outbox
                 WHERE owner_identity_id = ?1",
                [&binding.owner_identity_id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            1,
            "binding recovery must not create an unnormalized successor"
        );
    }

    #[test]
    fn group_control_event_read_does_not_enqueue_ordinary_remote_watermark() {
        let db = Connection::open_in_memory().unwrap();
        db.pragma_update(None, "foreign_keys", "ON").unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let binding = binding();
        upsert_identity_account_binding(&db, &binding).unwrap();
        let group_did = "did:wba:awiki.info:groups:group-control-read";
        let conversation_id = format!("group:{group_did}");
        db.execute(
            "INSERT INTO messages
                (msg_id, owner_identity_id, owner_did, thread_id, conversation_id,
                 wire_thread_kind, wire_thread_ref, wire_identity_resolution_state,
                 content_type, content, server_seq, hydration_state, is_e2ee, metadata, stored_at)
             VALUES (?1, ?2, ?3, ?4, ?4, 'group', ?5, 'resolved',
                     'application/json', ?6, 2, 'hydrated', 0, ?7,
                     '2026-07-28T10:00:00Z')",
            params![
                format!("{group_did}:2"),
                binding.owner_identity_id,
                binding.current_did,
                conversation_id,
                group_did,
                serde_json::json!({"schema": "awiki.group.system_event.v1"}).to_string(),
                serde_json::json!({
                    "message_role": "group_system_event",
                    "group_event_seq": "2"
                })
                .to_string(),
            ],
        )
        .unwrap();

        let result = mark_thread_read_and_update_outbox(
            &db,
            &binding.owner_identity_id,
            &binding.current_did,
            super::super::messages::MarkThreadReadWatermarkInput {
                thread: crate::messages::ThreadRef::Group(
                    crate::ids::GroupRef::parse(group_did).unwrap(),
                ),
                read_watermark_message_id: None,
                read_watermark_seq: Some("2".to_owned()),
                read_watermark_at: Some("2026-07-28T10:00:00Z".to_owned()),
                pending_remote_ack: true,
            },
        )
        .unwrap();

        assert!(!result.remote_ack_applicable);
        assert_eq!(result.outbox_operation_id, None);
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM local_mutation_outbox
                 WHERE owner_identity_id = ?1",
                [&binding.owner_identity_id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            0
        );
        assert_eq!(
            db.query_row(
                "SELECT pending_remote_ack
                 FROM thread_read_state
                 WHERE owner_identity_id = ?1 AND conversation_id = ?2",
                params![binding.owner_identity_id, conversation_id],
                |row| row.get::<_, bool>(0),
            )
            .unwrap(),
            false
        );
    }

    #[test]
    fn unbound_remote_group_read_state_projects_to_local_group_conversation() {
        let db = Connection::open_in_memory().unwrap();
        db.pragma_update(None, "foreign_keys", "ON").unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let binding = binding();
        upsert_identity_account_binding(&db, &binding).unwrap();
        let group_did = "did:wba:awiki.info:groups:group-read-projection";

        mark_thread_read_and_update_outbox(
            &db,
            &binding.owner_identity_id,
            &binding.current_did,
            super::super::messages::MarkThreadReadWatermarkInput {
                thread: crate::messages::ThreadRef::Group(
                    crate::ids::GroupRef::parse(group_did).unwrap(),
                ),
                read_watermark_message_id: None,
                read_watermark_seq: Some("10".to_owned()),
                read_watermark_at: Some("2026-07-28T10:00:00Z".to_owned()),
                pending_remote_ack: false,
            },
        )
        .unwrap();
        apply_remote_read_state(
            &db,
            &binding.owner_identity_id,
            &binding.current_did,
            &ReadStateApplyV2 {
                remote_thread_key: group_did.to_owned(),
                thread_kind: "group".to_owned(),
                read_watermark_seq: "20".to_owned(),
                read_watermark_message_id: None,
                state_version: "1".to_owned(),
                occurred_at: "2026-07-28T10:00:01Z".to_owned(),
            },
        )
        .unwrap();
        assert_eq!(
            db.query_row(
                "SELECT conversation_id || '|' || read_watermark_seq || '|' ||
                        remote_state_version
                 FROM thread_read_state
                 WHERE owner_identity_id = ?1",
                [&binding.owner_identity_id],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
            format!("group:{group_did}|20|1")
        );
    }

    #[test]
    fn direct_read_uses_message_binding_and_new_alias_acknowledges_old_outbox() {
        let db = Connection::open_in_memory().unwrap();
        db.pragma_update(None, "foreign_keys", "ON").unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let binding = binding();
        upsert_identity_account_binding(&db, &binding).unwrap();
        let conversation_id = "dm:peer-scope:v1:alice:rebound-bob";
        let old_remote_thread_key = "dconv-old-bob";
        let new_remote_thread_key = "dconv-new-bob";
        for (remote_thread_key, updated_at) in
            [(old_remote_thread_key, 1), (new_remote_thread_key, 2)]
        {
            upsert_sync_thread_binding(
                &db,
                &SyncThreadBinding {
                    owner_identity_id: binding.owner_identity_id.clone(),
                    remote_thread_key: remote_thread_key.to_owned(),
                    thread_kind: "direct".to_owned(),
                    conversation_id: conversation_id.to_owned(),
                    updated_at,
                },
            )
            .unwrap();
        }
        db.execute(
            "INSERT INTO messages
                (msg_id, owner_identity_id, owner_did, thread_id, conversation_id,
                 wire_thread_kind, wire_thread_ref, wire_identity_resolution_state,
                 content_type, server_seq, hydration_state, is_e2ee, metadata, stored_at)
             VALUES ('msg-old-30', ?1, ?2, ?3, ?3, 'direct', 'did:example:bob',
                     'resolved', 'text/plain', 30, 'hydrated', 0, ?4,
                     '2026-07-28T10:00:00Z')",
            params![
                binding.owner_identity_id,
                binding.current_did,
                conversation_id,
                serde_json::json!({"remote_thread_key": old_remote_thread_key}).to_string(),
            ],
        )
        .unwrap();

        let local = mark_thread_read_and_update_outbox(
            &db,
            &binding.owner_identity_id,
            &binding.current_did,
            super::super::messages::MarkThreadReadWatermarkInput {
                thread: crate::messages::ThreadRef::Thread(
                    crate::ids::ThreadId::parse(conversation_id).unwrap(),
                ),
                read_watermark_message_id: Some("msg-old-30".to_owned()),
                read_watermark_seq: Some("30".to_owned()),
                read_watermark_at: Some("2026-07-28T10:00:01Z".to_owned()),
                pending_remote_ack: true,
            },
        )
        .unwrap();
        assert_eq!(
            local.remote_thread_key.as_deref(),
            Some(old_remote_thread_key)
        );
        assert_eq!(
            db.query_row(
                "SELECT aggregate_id || '|' ||
                        json_extract(payload_json, '$.read_watermark_message_id')
                 FROM local_mutation_outbox",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
            "dconv-old-bob|msg-old-30"
        );

        apply_remote_read_state(
            &db,
            &binding.owner_identity_id,
            &binding.current_did,
            &ReadStateApplyV2 {
                remote_thread_key: old_remote_thread_key.to_owned(),
                thread_kind: "direct".to_owned(),
                read_watermark_seq: "20".to_owned(),
                read_watermark_message_id: Some("msg-old-20".to_owned()),
                state_version: "1".to_owned(),
                occurred_at: "2026-07-28T10:00:02Z".to_owned(),
            },
        )
        .unwrap();
        apply_remote_read_state(
            &db,
            &binding.owner_identity_id,
            &binding.current_did,
            &ReadStateApplyV2 {
                remote_thread_key: new_remote_thread_key.to_owned(),
                thread_kind: "direct".to_owned(),
                read_watermark_seq: "40".to_owned(),
                read_watermark_message_id: Some("msg-new-40".to_owned()),
                state_version: "1".to_owned(),
                occurred_at: "2026-07-28T10:00:03Z".to_owned(),
            },
        )
        .unwrap();
        apply_remote_read_state(
            &db,
            &binding.owner_identity_id,
            &binding.current_did,
            &ReadStateApplyV2 {
                remote_thread_key: old_remote_thread_key.to_owned(),
                thread_kind: "direct".to_owned(),
                read_watermark_seq: "25".to_owned(),
                read_watermark_message_id: Some("msg-old-25".to_owned()),
                state_version: "2".to_owned(),
                occurred_at: "2026-07-28T10:00:04Z".to_owned(),
            },
        )
        .unwrap();

        assert_eq!(
            db.query_row(
                "SELECT read_watermark_seq || '|' || pending_remote_ack
                 FROM thread_read_state
                 WHERE owner_identity_id = ?1 AND conversation_id = ?2",
                params![binding.owner_identity_id, conversation_id],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
            "40|0"
        );
        assert_eq!(
            db.query_row("SELECT status FROM local_mutation_outbox", [], |row| {
                row.get::<_, String>(0)
            })
            .unwrap(),
            "committed"
        );
    }

    #[test]
    fn read_outbox_coalesces_unsent_watermark_and_preserves_in_flight_successor() {
        let db = Connection::open_in_memory().unwrap();
        db.pragma_update(None, "foreign_keys", "ON").unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let binding = binding();
        upsert_identity_account_binding(&db, &binding).unwrap();
        upsert_sync_thread_binding(
            &db,
            &SyncThreadBinding {
                owner_identity_id: binding.owner_identity_id.clone(),
                remote_thread_key: "dconv-outbox-bob".to_owned(),
                thread_kind: "direct".to_owned(),
                conversation_id: "dm:peer-scope:v1:alice:bob".to_owned(),
                updated_at: 1,
            },
        )
        .unwrap();
        db.execute(
            "INSERT INTO messages
                (msg_id, owner_identity_id, owner_did, thread_id, conversation_id,
                 wire_thread_kind, wire_thread_ref, wire_identity_resolution_state,
                 server_seq, stored_at)
             VALUES ('msg-30', ?1, ?2, 'dm:peer-scope:v1:alice:bob',
                     'dm:peer-scope:v1:alice:bob', 'direct', 'did:example:bob',
                     'resolved', 30, '2026-07-28T10:00:00Z')",
            params![binding.owner_identity_id, binding.current_did],
        )
        .unwrap();
        let mark = |seq: &str| {
            mark_thread_read_and_update_outbox(
                &db,
                &binding.owner_identity_id,
                &binding.current_did,
                super::super::messages::MarkThreadReadWatermarkInput {
                    thread: crate::messages::ThreadRef::Thread(
                        crate::ids::ThreadId::parse("dm:peer-scope:v1:alice:bob").unwrap(),
                    ),
                    read_watermark_message_id: None,
                    read_watermark_seq: Some(seq.to_owned()),
                    read_watermark_at: Some("2026-07-28T10:00:00Z".to_owned()),
                    pending_remote_ack: true,
                },
            )
            .unwrap()
        };
        let first = mark("10");
        let second = mark("20");
        assert_eq!(first.outbox_operation_id, second.outbox_operation_id);
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM local_mutation_outbox
                 WHERE owner_identity_id = ?1",
                [&binding.owner_identity_id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            1
        );
        assert_eq!(
            db.query_row(
                "SELECT json_extract(payload_json, '$.read_watermark_seq')
                 FROM local_mutation_outbox",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
            "20"
        );
        let claimed = claim_read_mutation_by_operation_id(
            &db,
            &binding.owner_identity_id,
            first.outbox_operation_id.as_deref().unwrap(),
            2,
        )
        .unwrap()
        .unwrap();
        assert!(claimed
            .payload_json
            .contains("\"read_watermark_seq\":\"20\""));
        let successor = mark("30");
        assert_ne!(successor.outbox_operation_id, first.outbox_operation_id);
        let rows = db
            .prepare(
                "SELECT status, json_extract(payload_json, '$.read_watermark_seq')
                 FROM local_mutation_outbox ORDER BY created_at, mutation_id",
            )
            .unwrap()
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.contains(&("in_flight".to_owned(), "20".to_owned())));
        assert!(rows.contains(&("pending".to_owned(), "30".to_owned())));
        assert!(
            claim_read_mutation_by_operation_id(
                &db,
                &binding.owner_identity_id,
                successor.outbox_operation_id.as_deref().unwrap(),
                2,
            )
            .unwrap()
            .is_none(),
            "successor must wait until its in-flight predecessor is acknowledged"
        );

        recover_interrupted_work(&db, 3).unwrap();
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM local_mutation_outbox
                 WHERE status = 'retryable' AND in_flight_since IS NULL",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            1
        );
    }

    #[test]
    fn read_only_delta_invalidates_its_bound_conversation() {
        let db = Connection::open_in_memory().unwrap();
        db.pragma_update(None, "foreign_keys", "ON").unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let binding = binding();
        upsert_identity_account_binding(&db, &binding).unwrap();
        bootstrap_message_sync_state(
            &db,
            &MessageSyncState {
                owner_identity_id: binding.owner_identity_id.clone(),
                account_id: binding.account_id.clone(),
                protocol_device_id: binding.protocol_device_id.clone(),
                device_auth_generation: binding.device_auth_generation.clone(),
                stream_epoch: "1".to_owned(),
                scan_seq: "10".to_owned(),
                bootstrap_state: "active".to_owned(),
                last_server_time: None,
                last_success_at: Some(1),
                last_error_code: None,
                metadata_json: None,
                updated_at: 1,
            },
        )
        .unwrap();
        let conversation_id = "dm:peer-scope:v1:alice:read-only";
        let outcome = apply_delta_v2(
            &db,
            DeltaApplyInputV2 {
                owner_identity_id: binding.owner_identity_id.clone(),
                owner_did: binding.current_did.clone(),
                account_id: binding.account_id.clone(),
                protocol_device_id: binding.protocol_device_id.clone(),
                device_auth_generation: binding.device_auth_generation.clone(),
                stream_epoch: "1".to_owned(),
                next_scan_seq: "11".to_owned(),
                server_time: "2026-07-28T10:00:01Z".to_owned(),
                events: vec![DeltaApplyEventV2 {
                    event_id: "event-read-only-11".to_owned(),
                    event_seq: "11".to_owned(),
                    event_type: "message.read_state_updated".to_owned(),
                    thread_bindings: vec![SyncThreadBinding {
                        owner_identity_id: binding.owner_identity_id.clone(),
                        remote_thread_key: "dconv-read-only".to_owned(),
                        thread_kind: "direct".to_owned(),
                        conversation_id: conversation_id.to_owned(),
                        updated_at: 1,
                    }],
                    read_states: vec![ReadStateApplyV2 {
                        remote_thread_key: "dconv-read-only".to_owned(),
                        thread_kind: "direct".to_owned(),
                        read_watermark_seq: "9".to_owned(),
                        read_watermark_message_id: None,
                        state_version: "2".to_owned(),
                        occurred_at: "2026-07-28T10:00:00Z".to_owned(),
                    }],
                    ..DeltaApplyEventV2::default()
                }],
            },
        )
        .unwrap();
        assert_eq!(
            outcome.invalidation.conversation_ids,
            vec![conversation_id.to_owned()]
        );
        assert_eq!(
            outcome.invalidation.thread_ids,
            vec![conversation_id.to_owned()]
        );
    }

    #[test]
    fn unbound_group_read_delta_invalidates_canonical_conversation() {
        let db = Connection::open_in_memory().unwrap();
        db.pragma_update(None, "foreign_keys", "ON").unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let binding = binding();
        upsert_identity_account_binding(&db, &binding).unwrap();
        bootstrap_message_sync_state(
            &db,
            &MessageSyncState {
                owner_identity_id: binding.owner_identity_id.clone(),
                account_id: binding.account_id.clone(),
                protocol_device_id: binding.protocol_device_id.clone(),
                device_auth_generation: binding.device_auth_generation.clone(),
                stream_epoch: "1".to_owned(),
                scan_seq: "10".to_owned(),
                bootstrap_state: "active".to_owned(),
                last_server_time: None,
                last_success_at: Some(1),
                last_error_code: None,
                metadata_json: None,
                updated_at: 1,
            },
        )
        .unwrap();
        let group_did = "did:wba:awiki.info:groups:unbound-read-only";
        let conversation_id =
            crate::internal::local_state::owner_scope::group_conversation_id(group_did);
        let outcome = apply_delta_v2(
            &db,
            DeltaApplyInputV2 {
                owner_identity_id: binding.owner_identity_id.clone(),
                owner_did: binding.current_did.clone(),
                account_id: binding.account_id.clone(),
                protocol_device_id: binding.protocol_device_id.clone(),
                device_auth_generation: binding.device_auth_generation.clone(),
                stream_epoch: "1".to_owned(),
                next_scan_seq: "11".to_owned(),
                server_time: "2026-07-28T10:00:01Z".to_owned(),
                events: vec![DeltaApplyEventV2 {
                    event_id: "event-unbound-group-read-11".to_owned(),
                    event_seq: "11".to_owned(),
                    event_type: "message.read_state_updated".to_owned(),
                    read_states: vec![ReadStateApplyV2 {
                        remote_thread_key: group_did.to_owned(),
                        thread_kind: "group".to_owned(),
                        read_watermark_seq: "9".to_owned(),
                        read_watermark_message_id: None,
                        state_version: "2".to_owned(),
                        occurred_at: "2026-07-28T10:00:00Z".to_owned(),
                    }],
                    ..DeltaApplyEventV2::default()
                }],
            },
        )
        .unwrap();

        assert_eq!(
            outcome.invalidation.conversation_ids,
            vec![conversation_id.clone()]
        );
        assert_eq!(outcome.invalidation.thread_ids, vec![conversation_id]);
    }

    #[test]
    fn v2_snapshot_backlogs_unresolved_message_without_losing_its_thread_binding() {
        let db = Connection::open_in_memory().unwrap();
        db.pragma_update(None, "foreign_keys", "ON").unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let binding = binding();
        upsert_identity_account_binding(&db, &binding).unwrap();
        bootstrap_message_sync_state(
            &db,
            &MessageSyncState {
                owner_identity_id: binding.owner_identity_id.clone(),
                account_id: binding.account_id.clone(),
                protocol_device_id: binding.protocol_device_id.clone(),
                device_auth_generation: binding.device_auth_generation.clone(),
                stream_epoch: "1".to_owned(),
                scan_seq: "10".to_owned(),
                bootstrap_state: "active".to_owned(),
                last_server_time: None,
                last_success_at: Some(1),
                last_error_code: None,
                metadata_json: None,
                updated_at: 1,
            },
        )
        .unwrap();
        upsert_recovery_state(
            &db,
            &RecoveryState {
                owner_identity_id: binding.owner_identity_id.clone(),
                mode: "compact_recovery".to_owned(),
                requested_from_epoch: "1".to_owned(),
                requested_from_seq: "10".to_owned(),
                recovery_id_hash: Some("recovery-hash-unresolved".to_owned()),
                snapshot_scan_seq: Some("20".to_owned()),
                status: "applying".to_owned(),
                retry_count: 0,
                last_error_code: None,
                started_at: 1,
                updated_at: 1,
            },
        )
        .unwrap();
        let provisional_conversation_id = "dm:did:example:snapshot-peer";
        let outcome = apply_snapshot_v2(
            &db,
            SnapshotApplyInputV2 {
                owner_identity_id: binding.owner_identity_id.clone(),
                owner_did: binding.current_did.clone(),
                account_id: binding.account_id.clone(),
                protocol_device_id: binding.protocol_device_id.clone(),
                device_auth_generation: binding.device_auth_generation.clone(),
                expected_stream_epoch: "1".to_owned(),
                expected_scan_seq: "10".to_owned(),
                allow_missing_previous: false,
                recovery_id_hash: "recovery-hash-unresolved".to_owned(),
                stream_epoch: "2".to_owned(),
                snapshot_scan_seq: "20".to_owned(),
                server_time: "2026-07-31T10:00:02Z".to_owned(),
                events: vec![DeltaApplyEventV2 {
                    event_id: "snapshot-event-20".to_owned(),
                    event_seq: "20".to_owned(),
                    event_type: "message.created".to_owned(),
                    messages: vec![super::super::messages::MessageRecord {
                        msg_id: "snapshot-message-20".to_owned(),
                        owner_identity_id: binding.owner_identity_id.clone(),
                        owner_did: binding.current_did.clone(),
                        conversation_id: provisional_conversation_id.to_owned(),
                        thread_id: provisional_conversation_id.to_owned(),
                        direction: 0,
                        sender_did: "did:example:snapshot-peer".to_owned(),
                        receiver_did: binding.current_did.clone(),
                        content_type: "text/plain".to_owned(),
                        content: "snapshot body".to_owned(),
                        sent_at: "2026-07-31T10:00:00Z".to_owned(),
                        stored_at: "2026-07-31T10:00:00Z".to_owned(),
                        credential_name: binding.owner_identity_id.clone(),
                        ..Default::default()
                    }
                    .with_resolved_wire_thread("direct", "did:example:snapshot-peer")],
                    thread_bindings: vec![SyncThreadBinding {
                        owner_identity_id: binding.owner_identity_id.clone(),
                        remote_thread_key: "snapshot-remote-thread".to_owned(),
                        thread_kind: "direct".to_owned(),
                        conversation_id: provisional_conversation_id.to_owned(),
                        updated_at: 20,
                    }],
                    ..Default::default()
                }],
                groups: Vec::new(),
                read_states: Vec::new(),
            },
        )
        .unwrap();

        assert_eq!(outcome.backlogged_messages, 1);
        assert!(outcome.projected_message_event_ids.is_empty());
        let MessageSyncStateAccess::Ready(state) =
            load_message_sync_state(&db, &binding.owner_identity_id).unwrap()
        else {
            panic!("snapshot must commit its cursor");
        };
        assert_eq!(
            (state.stream_epoch.as_str(), state.scan_seq.as_str()),
            ("2", "20")
        );
        assert_eq!(
            db.query_row(
                "SELECT remote_thread_key FROM inbound_resolution_thread_bindings
                 WHERE owner_identity_id = ?1 AND message_id = 'snapshot-message-20'",
                [&binding.owner_identity_id],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
            "snapshot-remote-thread"
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM sync_thread_bindings", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
            0
        );
    }

    #[test]
    fn snapshot_cas_rejects_a_concurrent_cursor_advance() {
        let db = Connection::open_in_memory().unwrap();
        db.pragma_update(None, "foreign_keys", "ON").unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let binding = binding();
        upsert_identity_account_binding(&db, &binding).unwrap();
        let state = MessageSyncState {
            owner_identity_id: binding.owner_identity_id.clone(),
            account_id: binding.account_id.clone(),
            protocol_device_id: binding.protocol_device_id.clone(),
            device_auth_generation: binding.device_auth_generation.clone(),
            stream_epoch: "1".to_owned(),
            scan_seq: "10".to_owned(),
            bootstrap_state: "active".to_owned(),
            last_server_time: None,
            last_success_at: Some(1),
            last_error_code: None,
            metadata_json: None,
            updated_at: 1,
        };
        bootstrap_message_sync_state(&db, &state).unwrap();
        upsert_recovery_state(
            &db,
            &RecoveryState {
                owner_identity_id: binding.owner_identity_id.clone(),
                mode: "compact_recovery".to_owned(),
                requested_from_epoch: "1".to_owned(),
                requested_from_seq: "10".to_owned(),
                recovery_id_hash: Some("recovery-hash-cas".to_owned()),
                snapshot_scan_seq: Some("20".to_owned()),
                status: "applying".to_owned(),
                retry_count: 0,
                last_error_code: None,
                started_at: 1,
                updated_at: 1,
            },
        )
        .unwrap();
        let mut concurrently_advanced = state;
        concurrently_advanced.scan_seq = "11".to_owned();
        concurrently_advanced.updated_at = 2;
        assert!(matches!(
            advance_message_sync_state(&db, &concurrently_advanced).unwrap(),
            MessageSyncStateAccess::Ready(_)
        ));
        let error = apply_snapshot_v2(
            &db,
            SnapshotApplyInputV2 {
                owner_identity_id: binding.owner_identity_id.clone(),
                owner_did: binding.current_did.clone(),
                account_id: binding.account_id.clone(),
                protocol_device_id: binding.protocol_device_id.clone(),
                device_auth_generation: binding.device_auth_generation.clone(),
                expected_stream_epoch: "1".to_owned(),
                expected_scan_seq: "10".to_owned(),
                allow_missing_previous: false,
                recovery_id_hash: "recovery-hash-cas".to_owned(),
                stream_epoch: "1".to_owned(),
                snapshot_scan_seq: "20".to_owned(),
                server_time: "2026-07-28T10:00:02Z".to_owned(),
                events: Vec::new(),
                groups: Vec::new(),
                read_states: Vec::new(),
            },
        )
        .unwrap_err();
        assert!(matches!(
            error,
            crate::ImError::Service {
                code: Some(code),
                ..
            } if code == "SYNC_SNAPSHOT_CAS_FAILED"
        ));
        let MessageSyncStateAccess::Ready(current) =
            load_message_sync_state(&db, &binding.owner_identity_id).unwrap()
        else {
            panic!("concurrent cursor must remain ready");
        };
        assert_eq!(current.scan_seq, "11");
    }

    #[test]
    fn schema_two_snapshot_rolls_back_on_second_notification_then_commits_mixed_state() {
        let mut db = Connection::open_in_memory().unwrap();
        db.pragma_update(None, "foreign_keys", "ON").unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let binding = binding();
        upsert_identity_account_binding(&db, &binding).unwrap();
        bootstrap_message_sync_state(
            &db,
            &MessageSyncState {
                owner_identity_id: binding.owner_identity_id.clone(),
                account_id: binding.account_id.clone(),
                protocol_device_id: binding.protocol_device_id.clone(),
                device_auth_generation: binding.device_auth_generation.clone(),
                stream_epoch: "1".to_owned(),
                scan_seq: "10".to_owned(),
                bootstrap_state: "active".to_owned(),
                last_server_time: None,
                last_success_at: Some(1),
                last_error_code: None,
                metadata_json: None,
                updated_at: 1,
            },
        )
        .unwrap();
        upsert_recovery_state(
            &db,
            &RecoveryState {
                owner_identity_id: binding.owner_identity_id.clone(),
                mode: "compact_recovery".to_owned(),
                requested_from_epoch: "1".to_owned(),
                requested_from_seq: "10".to_owned(),
                recovery_id_hash: Some("schema-two-recovery".to_owned()),
                snapshot_scan_seq: Some("20".to_owned()),
                status: "applying".to_owned(),
                retry_count: 0,
                last_error_code: None,
                started_at: 1,
                updated_at: 1,
            },
        )
        .unwrap();
        let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/multi_device_v1/system-notification-v1.json");
        let fixture: serde_json::Value =
            serde_json::from_slice(&std::fs::read(fixture_path).unwrap()).unwrap();
        let mut request = fixture["p3_vector"]["request"].clone();
        request["method"] = serde_json::Value::String("direct.incoming".to_owned());
        let envelope =
            crate::internal::system_notification::wire::parse_envelope(&request).unwrap();
        let received_at = chrono::DateTime::parse_from_rfc3339("2026-07-23T02:00:01Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let notification = |proof_hash: &str| {
            crate::internal::system_notification::store::SystemNotificationApplyInput {
                owner_identity_id: binding.owner_identity_id.clone(),
                owner_did: binding.current_did.clone(),
                protocol_device_id: binding.protocol_device_id.clone(),
                verified:
                    crate::internal::system_notification::verify::VerifiedSystemNotification {
                        envelope: envelope.clone(),
                        payload_hash: "sha256:snapshot-payload".to_owned(),
                        proof_hash: proof_hash.to_owned(),
                    },
                received_at,
            }
        };
        let snapshot_input = |events| SnapshotApplyInputV2 {
            owner_identity_id: binding.owner_identity_id.clone(),
            owner_did: binding.current_did.clone(),
            account_id: binding.account_id.clone(),
            protocol_device_id: binding.protocol_device_id.clone(),
            device_auth_generation: binding.device_auth_generation.clone(),
            expected_stream_epoch: "1".to_owned(),
            expected_scan_seq: "10".to_owned(),
            allow_missing_previous: false,
            recovery_id_hash: "schema-two-recovery".to_owned(),
            stream_epoch: "2".to_owned(),
            snapshot_scan_seq: "20".to_owned(),
            server_time: "2026-07-23T02:00:01Z".to_owned(),
            events,
            groups: Vec::new(),
            read_states: Vec::new(),
        };
        let system_event = |event_id: &str, event_seq: &str, proof_hash: &str| DeltaApplyEventV2 {
            event_id: event_id.to_owned(),
            event_seq: event_seq.to_owned(),
            event_type: "system.notification".to_owned(),
            system_notification: Some(notification(proof_hash)),
            ..DeltaApplyEventV2::default()
        };

        assert!(apply_snapshot_v2(
            &db,
            snapshot_input(vec![
                system_event("snapshot-system-18", "18", "sha256:proof-a"),
                system_event("snapshot-system-19", "19", "sha256:proof-conflict"),
            ]),
        )
        .is_err());
        let MessageSyncStateAccess::Ready(state) =
            load_message_sync_state(&db, &binding.owner_identity_id).unwrap()
        else {
            panic!("failed snapshot must retain its prior cursor")
        };
        assert_eq!(
            (state.stream_epoch.as_str(), state.scan_seq.as_str()),
            ("1", "10")
        );
        for table in [
            "sync_applied_events",
            "system_notification_receipts",
            "system_notification_join_state",
        ] {
            assert_eq!(
                db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
                0,
                "{table} must roll back"
            );
        }

        let peer_did = "did:wba:awiki.info:user:snapshot-two-peer";
        let conversation_id = super::super::peer_personas::project_verified_handle(
            &mut db,
            &binding.owner_identity_id,
            &binding.current_did,
            &crate::directory::HandleLookupResult {
                handle: crate::ids::Handle::parse("snapshot-two-peer.awiki.info", "").unwrap(),
                did: crate::ids::Did::parse(peer_did).unwrap(),
                user_id: "snapshot-two-peer".to_owned(),
                domain: Some("awiki.info".to_owned()),
                status: Some("active".to_owned()),
                binding_generation: Some("1".to_owned()),
                profile: None,
                warnings: Vec::new(),
            },
        )
        .unwrap();
        let ordinary = DeltaApplyEventV2 {
            event_id: "snapshot-plain-19".to_owned(),
            event_seq: "19".to_owned(),
            event_type: "message.created".to_owned(),
            messages: vec![super::super::messages::MessageRecord {
                msg_id: "snapshot-message-19".to_owned(),
                owner_identity_id: binding.owner_identity_id.clone(),
                owner_did: binding.current_did.clone(),
                conversation_id: conversation_id.clone(),
                thread_id: conversation_id.clone(),
                direction: 0,
                sender_did: peer_did.to_owned(),
                receiver_did: binding.current_did.clone(),
                content_type: "text/plain".to_owned(),
                content: "mixed snapshot message".to_owned(),
                server_seq: Some(19),
                sent_at: "2026-07-23T02:00:00Z".to_owned(),
                stored_at: "2026-07-23T02:00:01Z".to_owned(),
                credential_name: binding.owner_identity_id.clone(),
                ..Default::default()
            }
            .with_resolved_wire_thread("direct", peer_did)],
            thread_bindings: vec![SyncThreadBinding {
                owner_identity_id: binding.owner_identity_id.clone(),
                remote_thread_key: "snapshot-two-remote-thread".to_owned(),
                thread_kind: "direct".to_owned(),
                conversation_id,
                updated_at: 19,
            }],
            ..Default::default()
        };
        let outcome = apply_snapshot_v2(
            &db,
            snapshot_input(vec![
                system_event("snapshot-system-18", "18", "sha256:proof-a"),
                ordinary,
            ]),
        )
        .unwrap();
        assert_eq!(outcome.committed_system_notifications.len(), 1);
        assert_eq!(outcome.projected_message_event_ids, ["snapshot-plain-19"]);
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM system_notification_receipts",
                [],
                |row| { row.get::<_, i64>(0) }
            )
            .unwrap(),
            1
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM messages", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        let MessageSyncStateAccess::Ready(state) =
            load_message_sync_state(&db, &binding.owner_identity_id).unwrap()
        else {
            panic!("valid mixed snapshot must commit its cursor")
        };
        assert_eq!(
            (state.stream_epoch.as_str(), state.scan_seq.as_str()),
            ("2", "20")
        );
        assert_eq!(
            load_recovery_state(&db, &binding.owner_identity_id)
                .unwrap()
                .unwrap()
                .status,
            "completed"
        );
    }
}
