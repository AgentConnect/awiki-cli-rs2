use rusqlite::{params, Connection, OptionalExtension};
use std::collections::BTreeSet;

pub(crate) const APPLIED_EVENT_MIN_RECEIPTS_PER_OWNER: i64 = 10_000;
pub(crate) const APPLIED_EVENT_SAFETY_WINDOW: u32 = 1_000;

pub(crate) const SYNC_INSTALLATION_SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS sync_installation_state (
    owner_identity_id   TEXT PRIMARY KEY,
    client_instance_id  TEXT NOT NULL UNIQUE,
    created_at          INTEGER NOT NULL,
    CHECK (length(trim(owner_identity_id)) > 0),
    CHECK (length(trim(client_instance_id)) > 0)
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BootstrapApplyInputV2 {
    pub(crate) binding: IdentityAccountBinding,
    pub(crate) state: MessageSyncState,
    pub(crate) groups: Vec<super::groups::GroupRecord>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DeltaApplyEventV2 {
    pub(crate) event_id: String,
    pub(crate) event_seq: String,
    pub(crate) event_type: String,
    pub(crate) messages: Vec<super::messages::MessageRecord>,
    pub(crate) groups: Vec<super::groups::GroupRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
    pub(crate) invalidation: super::sync_state::SyncDeltaInvalidation,
}

pub(crate) fn create_schema(connection: &Connection) -> crate::ImResult<()> {
    connection
        .execute_batch(SYNC_V2_SCHEMA_SQL)
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
    for event in events {
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
        let mut projected_message = false;
        for message in event.messages {
            let message = super::inbound_resolution_backlog::canonicalize_inbound_message(
                &transaction,
                message,
            )?;
            messages.push(message);
            projected_message = true;
        }
        for group in event.groups {
            validate_group_owner(&group, &input.owner_identity_id)?;
            if group_state_is_stale(&transaction, &group)? {
                continue;
            }
            groups.push(group);
        }
        applied_event_ids.push(event_id.clone());
        if projected_message {
            projected_message_event_ids.push(event_id);
        }
    }

    let mut invalidation = v2_invalidation(
        &input.owner_identity_id,
        &input.owner_did,
        &input.next_scan_seq,
        &messages,
        &groups,
    );
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
        backlogged_messages: 0,
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
    owner_identity_id: &str,
    owner_did: &str,
    scan_seq: &str,
    messages: &[super::messages::MessageRecord],
    groups: &[super::groups::GroupRecord],
) -> super::sync_state::SyncDeltaInvalidation {
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
    super::sync_state::SyncDeltaInvalidation {
        owner_identity_id: owner_identity_id.to_owned(),
        owner_did: owner_did.to_owned(),
        reason: "sync_v2_delta".to_owned(),
        checkpoint_event_seq: scan_seq.to_owned(),
        conversation_ids: conversation_ids.into_iter().collect(),
        thread_ids: thread_ids.into_iter().collect(),
        group_ids: group_ids.into_iter().collect(),
        group_dids: group_dids.into_iter().collect(),
    }
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
    fn v2_required_resolution_failure_rolls_back_receipt_and_cursor() {
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

        assert!(matches!(
            apply_delta_v2(
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
                    }],
                }
            ),
            Err(crate::ImError::IdentityUnresolved { .. })
        ));
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM sync_applied_events
                 WHERE owner_identity_id = ?1",
                [&binding.owner_identity_id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            0
        );
        assert_eq!(
            load_message_sync_state(&db, &binding.owner_identity_id).unwrap(),
            MessageSyncStateAccess::Ready(state)
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
}
