//! Crash-repairable finalization for a verified root-key import.
//!
//! This module joins the anp-identity root-capability transition, local
//! identity projection, sync-account authorization generation and secret-free
//! SQLite coordinator transition. It does not interpret wire input.

use crate::internal::identity_device_state::IdentityInternalCheckpoint;
use crate::internal::identity_store::IdentityStore;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RootImportPromotionRequest {
    pub(crate) completed_message_id: String,
    pub(crate) auth_generation: u64,
    pub(crate) checkpoint: IdentityInternalCheckpoint,
    pub(crate) pending_root_ref:
        crate::internal::identity_root_import_completion::RootImportCustodyRef,
    pub(crate) root_key_id: String,
    pub(crate) root_public_key_fingerprint: String,
}

/// Replays the same exact transition after a process crash. It repairs:
///
/// - active root capability present while the index is still a member projection;
/// - index promoted while the coordinator remains `registry_confirmed`;
/// - coordinator promoted while a long-lived client still has stale custody state.
pub(crate) fn repair_root_import_promotion(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
    request: RootImportPromotionRequest,
) -> crate::ImResult<()> {
    converge_root_import_promotion(core, client, request)
}

fn converge_root_import_promotion(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
    request: RootImportPromotionRequest,
) -> crate::ImResult<()> {
    if request.completed_message_id.trim().is_empty()
        || request.auth_generation == 0
        || request.pending_root_ref.store_id.trim().is_empty()
        || request.pending_root_ref.identity_id.trim().is_empty()
        || request.pending_root_ref.did != client.did().as_str()
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let owner_identity_id = client.current_identity().id.as_str();
    let local_device_id = client.exact_protocol_device_id()?;
    let pending_ref_json =
        serde_json::to_string(&request.pending_root_ref).map_err(redacted_serialization)?;
    let connection = crate::internal::local_state::open_writable(
        &core.inner().sdk_paths().local_state.sqlite_path,
    )?;
    require_exact_registry_confirmed_coordinator(
        &connection,
        owner_identity_id,
        &local_device_id,
        &request,
        &pending_ref_json,
    )?;

    let local_alias = client
        .current_identity()
        .local_alias
        .as_deref()
        .unwrap_or(owner_identity_id)
        .to_owned();
    let store = IdentityStore::new(&core.inner().sdk_paths().identities);
    let index = store.load_index()?;
    let entry = index
        .credentials
        .get(&local_alias)
        .ok_or(crate::ImError::PermissionDenied)?;
    if entry.identity_custody_backend.as_deref() != Some("anp_identity")
        || entry.anp_identity_store_id.as_deref()
            != Some(request.pending_root_ref.store_id.as_str())
        || entry.anp_identity_id.as_deref() != Some(request.pending_root_ref.identity_id.as_str())
        || entry.did != request.pending_root_ref.did
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let document = client.runtime().key_provider.did_document()?;
    if crate::internal::identity_wire::document::document_hash(&document)?
        != request.checkpoint.document_hash
    {
        return Err(crate::ImError::PermissionDenied);
    }
    crate::internal::identity_custody::confirm_completion_root(
        core,
        &request.pending_root_ref,
        &document,
        &request.checkpoint,
    )?;
    let mut state = entry
        .device_state
        .clone()
        .ok_or(crate::ImError::PermissionDenied)?;
    let authorization = state
        .authorization
        .as_mut()
        .ok_or(crate::ImError::PermissionDenied)?;
    authorization.role = crate::internal::identity_device_state::DeviceAuthorizationRole::Admin;
    authorization.management_ready = true;
    authorization.auth_generation = request.auth_generation;
    state.checkpoint = Some(request.checkpoint.clone());
    state.validate_for_did(client.did())?;
    store.save_device_state(&local_alias, state)?;
    client.runtime().key_provider.reload_custody()?;

    mark_coordinator_promoted(
        &connection,
        owner_identity_id,
        &local_device_id,
        &request.completed_message_id,
        &pending_ref_json,
        request.auth_generation,
    )?;

    Ok(())
}

fn require_exact_registry_confirmed_coordinator(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    local_device_id: &str,
    request: &RootImportPromotionRequest,
    pending_ref_json: &str,
) -> crate::ImResult<()> {
    let count: i64 = connection
        .query_row(
            r#"
SELECT COUNT(*) FROM identity_root_import_completion_v1
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND message_id = ?3
  AND owner_did = ?4
  AND recipient_device_id = ?2
  AND pending_root_ref_json = ?5
  AND root_key_id = ?6
  AND root_fingerprint = ?7
  AND document_version = ?8
  AND document_hash = ?9
  AND registry_version + 1 = ?10
  AND phase IN ('registry_confirmed', 'promoted')"#,
            rusqlite::params![
                owner_identity_id,
                local_device_id,
                request.completed_message_id,
                request.pending_root_ref.did,
                pending_ref_json,
                request.root_key_id,
                request.root_public_key_fingerprint,
                request.checkpoint.document_version,
                request.checkpoint.document_hash,
                request.checkpoint.registry_version,
            ],
            |row| row.get(0),
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    if count != 1 {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

fn mark_coordinator_promoted(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    local_device_id: &str,
    completed_message_id: &str,
    pending_ref_json: &str,
    auth_generation: u64,
) -> crate::ImResult<()> {
    let transaction = connection
        .unchecked_transaction()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    advance_local_account_binding_generation(
        &transaction,
        owner_identity_id,
        local_device_id,
        auth_generation,
    )?;
    let changed = transaction
        .execute(
            r#"
UPDATE identity_root_import_completion_v1
SET phase = 'promoted', updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
    last_error_code = NULL
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND message_id = ?3
  AND pending_root_ref_json = ?4 AND phase = 'registry_confirmed'"#,
            rusqlite::params![
                owner_identity_id,
                local_device_id,
                completed_message_id,
                pending_ref_json,
            ],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    if changed != 1 {
        let promoted: i64 = transaction
            .query_row(
                r#"
SELECT COUNT(*) FROM identity_root_import_completion_v1
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND message_id = ?3
  AND pending_root_ref_json = ?4 AND phase = 'promoted'"#,
                rusqlite::params![
                    owner_identity_id,
                    local_device_id,
                    completed_message_id,
                    pending_ref_json,
                ],
                |row| row.get(0),
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        if promoted != 1 {
            return Err(crate::ImError::PermissionDenied);
        }
    }
    transaction
        .commit()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    Ok(())
}

fn advance_local_account_binding_generation(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    local_device_id: &str,
    auth_generation: u64,
) -> crate::ImResult<()> {
    use rusqlite::OptionalExtension as _;

    let Some((stored_device_id, stored_generation)) = connection
        .query_row(
            "SELECT device_id, device_auth_generation
             FROM identity_account_bindings
             WHERE owner_identity_id = ?1",
            [owner_identity_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(crate::internal::local_state::local_state_unavailable)?
    else {
        // A fresh client will create the binding from the promoted identity
        // index. Root import itself must not invent missing account metadata.
        return Ok(());
    };
    if stored_device_id != local_device_id {
        return Err(crate::ImError::IdentityBindingConflict {
            detail: "root import local account binding targets another device".to_owned(),
        });
    }
    let next_generation = auth_generation.to_string();
    match crate::internal::local_state::sync_v2::compare_decimal(
        &next_generation,
        &stored_generation,
    )? {
        std::cmp::Ordering::Less => {
            return Err(crate::ImError::IdentityBindingConflict {
                detail: "root import device authorization generation cannot move backwards"
                    .to_owned(),
            });
        }
        std::cmp::Ordering::Equal => return Ok(()),
        std::cmp::Ordering::Greater => {}
    }
    let changed = connection
        .execute(
            "UPDATE identity_account_bindings
             SET device_auth_generation = ?1,
                 updated_at = CAST(strftime('%s', 'now') AS INTEGER)
             WHERE owner_identity_id = ?2 AND device_id = ?3
               AND device_auth_generation = ?4",
            rusqlite::params![
                next_generation,
                owner_identity_id,
                local_device_id,
                stored_generation,
            ],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    if changed != 1 {
        return Err(crate::ImError::IdentityBindingConflict {
            detail: "root import local account binding changed concurrently".to_owned(),
        });
    }
    Ok(())
}

fn redacted_serialization(_error: impl std::fmt::Display) -> crate::ImError {
    crate::ImError::Serialization {
        detail: "root import promotion state is invalid".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coordinator(connection: &rusqlite::Connection, phase: &str) {
        connection
            .execute_batch(
                r#"
CREATE TABLE identity_root_import_completion_v1 (
    owner_identity_id TEXT NOT NULL,
    local_device_id TEXT NOT NULL,
    message_id TEXT NOT NULL,
    pending_root_ref_json TEXT NOT NULL,
    phase TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    last_error_code TEXT,
    PRIMARY KEY (owner_identity_id, local_device_id, message_id)
);
CREATE TABLE identity_account_bindings (
    owner_identity_id TEXT PRIMARY KEY,
    device_id TEXT NOT NULL,
    device_auth_generation TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);
"#,
            )
            .unwrap();
        connection
            .execute(
                r#"
INSERT INTO identity_root_import_completion_v1 (
  owner_identity_id, local_device_id, message_id, pending_root_ref_json,
  phase, updated_at
) VALUES ('owner-a', 'device-a', 'message-a', 'pending-ref', ?1, 'now')"#,
                [phase],
            )
            .unwrap();
    }

    #[test]
    fn coordinator_promotion_is_restart_idempotent() {
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        coordinator(&connection, "registry_confirmed");

        mark_coordinator_promoted(
            &connection,
            "owner-a",
            "device-a",
            "message-a",
            "pending-ref",
            2,
        )
        .unwrap();
        mark_coordinator_promoted(
            &connection,
            "owner-a",
            "device-a",
            "message-a",
            "pending-ref",
            2,
        )
        .unwrap();

        let phase: String = connection
            .query_row(
                "SELECT phase FROM identity_root_import_completion_v1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(phase, "promoted");
    }

    #[test]
    fn coordinator_promotion_rejects_mismatched_pending_ref_or_phase() {
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        coordinator(&connection, "completion_accepted");

        assert!(mark_coordinator_promoted(
            &connection,
            "owner-a",
            "device-a",
            "message-a",
            "pending-ref",
            2,
        )
        .is_err());
        assert!(mark_coordinator_promoted(
            &connection,
            "owner-a",
            "device-a",
            "message-a",
            "different-ref",
            2,
        )
        .is_err());
    }

    #[test]
    fn coordinator_promotion_advances_binding_but_leaves_sync_epoch_for_bootstrap() {
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        coordinator(&connection, "registry_confirmed");
        connection
            .execute_batch(
                r#"
INSERT INTO identity_account_bindings (
  owner_identity_id, device_id, device_auth_generation, updated_at
) VALUES ('owner-a', 'device-a', '1', 1);
CREATE TABLE message_sync_state (
  owner_identity_id TEXT PRIMARY KEY,
  device_auth_generation TEXT NOT NULL
);
INSERT INTO message_sync_state (owner_identity_id, device_auth_generation)
VALUES ('owner-a', '1');
"#,
            )
            .unwrap();

        mark_coordinator_promoted(
            &connection,
            "owner-a",
            "device-a",
            "message-a",
            "pending-ref",
            2,
        )
        .unwrap();

        let binding_generation: String = connection
            .query_row(
                "SELECT device_auth_generation FROM identity_account_bindings",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let sync_generation: String = connection
            .query_row(
                "SELECT device_auth_generation FROM message_sync_state",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(binding_generation, "2");
        assert_eq!(sync_generation, "1");
    }

    #[test]
    fn coordinator_promotion_replay_repairs_stale_binding_generation() {
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        coordinator(&connection, "promoted");
        connection
            .execute(
                r#"
INSERT INTO identity_account_bindings (
  owner_identity_id, device_id, device_auth_generation, updated_at
) VALUES ('owner-a', 'device-a', '1', 1)"#,
                [],
            )
            .unwrap();

        mark_coordinator_promoted(
            &connection,
            "owner-a",
            "device-a",
            "message-a",
            "pending-ref",
            2,
        )
        .unwrap();

        let binding_generation: String = connection
            .query_row(
                "SELECT device_auth_generation FROM identity_account_bindings",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(binding_generation, "2");
    }
}
