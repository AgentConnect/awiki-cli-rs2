//! Crash-repairable finalization for a verified root-key import.
//!
//! This module joins the encrypted Vault transition, atomic identity-index
//! transition and secret-free SQLite coordinator transition. It does not
//! interpret wire input and never exposes the pending Vault kind through a key
//! provider.

use crate::internal::identity_device_state::IdentityInternalCheckpoint;
use crate::internal::identity_store::{
    IdentityStore, PromoteVerifiedRootImportInput, RootImportPromotionResult,
    SaveIdentitySecretStorage,
};
use crate::internal::secret_vault::record::SecretRef;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RootImportPromotionRequest {
    pub(crate) completed_message_id: String,
    pub(crate) auth_generation: u64,
    pub(crate) checkpoint: IdentityInternalCheckpoint,
    pub(crate) pending_root_ref: SecretRef,
    pub(crate) root_key_id: String,
    pub(crate) root_public_key_fingerprint: String,
}

/// Replays the same exact transition after a process crash. It repairs:
///
/// - active Vault present while the index is still a member projection;
/// - index promoted while the coordinator remains `registry_confirmed`;
/// - coordinator promoted while the pending Vault record remains.
pub(crate) fn repair_root_import_promotion(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
    request: RootImportPromotionRequest,
) -> crate::ImResult<RootImportPromotionResult> {
    converge_root_import_promotion(core, client, request)
}

fn converge_root_import_promotion(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
    request: RootImportPromotionRequest,
) -> crate::ImResult<RootImportPromotionResult> {
    if request.completed_message_id.trim().is_empty()
        || request.auth_generation == 0
        || request.pending_root_ref.kind
            != crate::internal::secret_vault::record::SecretKind::IdentityRootImportPending
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
    let secret_storage = SaveIdentitySecretStorage::from_core(core)?;
    let mut result = IdentityStore::new(&core.inner().sdk_paths().identities)
        .promote_verified_root_import(PromoteVerifiedRootImportInput {
            local_alias,
            completed_message_id: request.completed_message_id.clone(),
            pending_root_ref: request.pending_root_ref.clone(),
            root_key_id: request.root_key_id.clone(),
            root_public_key_fingerprint: request.root_public_key_fingerprint.clone(),
            auth_generation: request.auth_generation,
            checkpoint: request.checkpoint.clone(),
            secret_storage: secret_storage.clone(),
        })?;
    client
        .runtime()
        .key_provider
        .advance_vault_root_ref(&result.active_root_ref)?;

    mark_coordinator_promoted(
        &connection,
        owner_identity_id,
        &local_device_id,
        &request.completed_message_id,
        &pending_ref_json,
    )?;

    let SaveIdentitySecretStorage::Vault { vault, .. } = secret_storage else {
        return Err(crate::ImError::PermissionDenied);
    };
    if vault.delete(&request.pending_root_ref).is_ok() {
        result.pending_cleanup_required = false;
    }
    Ok(result)
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
                request
                    .pending_root_ref
                    .did
                    .as_deref()
                    .ok_or(crate::ImError::PermissionDenied)?,
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
) -> crate::ImResult<()> {
    let changed = connection
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
    if changed == 1 {
        return Ok(());
    }
    let promoted: i64 = connection
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
        )
        .unwrap();
        mark_coordinator_promoted(
            &connection,
            "owner-a",
            "device-a",
            "message-a",
            "pending-ref",
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
        )
        .is_err());
        assert!(mark_coordinator_promoted(
            &connection,
            "owner-a",
            "device-a",
            "message-a",
            "different-ref",
        )
        .is_err());
    }
}
