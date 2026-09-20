//! Access the automatic task through the existing locked, atomic Join store.
use super::*;
use crate::internal::identity_join_management::ManagementTask;

#[derive(Clone)]
pub(crate) struct AuthorizedManagementJoin {
    pub join_session_id: String,
    pub recipient_device_id: String,
    pub authorizing_device_id: String,
    pub approved_document: Value,
    pub task: ManagementTask,
    pub join_authorized: bool,
}

pub(crate) fn tasks(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
) -> crate::ImResult<Vec<AuthorizedManagementJoin>> {
    let _guard = lock_join_state(core)?;
    let mut result = Vec::new();
    for stored in JoinStateStore::new(core).list()? {
        if stored.side != DeviceJoinSide::Admin || !owns_admin_join(client, &stored)? {
            continue;
        }
        let Some(approval) = stored.approval.as_ref() else {
            continue;
        };
        let Some(mut task) = approval.management_task.clone() else {
            continue;
        };
        if task.attempts > task.max_attempts {
            return Err(crate::ImError::PermissionDenied);
        }
        if matches!(
            stored.phase,
            DeviceJoinLocalPhase::Cancelled | DeviceJoinLocalPhase::Expired
        ) {
            task.phase = crate::internal::identity_join_management::ManagementPhase::Failed;
            task.failure_code = Some(
                if stored.phase == DeviceJoinLocalPhase::Cancelled {
                    "join_cancelled"
                } else {
                    "join_expired"
                }
                .to_owned(),
            );
        }
        result.push(AuthorizedManagementJoin {
            join_session_id: stored.join_request.join_session_id,
            recipient_device_id: stored.join_request.device_id,
            authorizing_device_id: approval.authorizing_device_id.clone(),
            approved_document: approval.new_document.clone(),
            task,
            join_authorized: stored.phase == DeviceJoinLocalPhase::Authorized
                && approval.confirmed_authorization.is_some(),
        });
    }
    Ok(result)
}

pub(crate) fn save_task(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
    join: &AuthorizedManagementJoin,
) -> crate::ImResult<()> {
    let _guard = lock_join_state(core)?;
    let store = JoinStateStore::new(core);
    let mut stored = store
        .load(&join.join_session_id, DeviceJoinSide::Admin)?
        .ok_or(crate::ImError::PermissionDenied)?;
    if !owns_admin_join(client, &stored)?
        || stored.join_request.device_id != join.recipient_device_id
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let approval = stored
        .approval
        .as_mut()
        .ok_or(crate::ImError::PermissionDenied)?;
    if approval.management_task.is_none()
        || approval.new_document != join.approved_document
        || approval.authorizing_device_id != join.authorizing_device_id
    {
        return Err(crate::ImError::PermissionDenied);
    }
    approval.management_task = Some(join.task.clone());
    store.save(&stored)
}

pub(super) fn authorization_payload(
    stored: &StoredJoinSession,
    approval: &StoredAdminApproval,
) -> crate::ImResult<Value> {
    Ok(json!({
        "type": "awiki.local.join-management-authorization.v1",
        "owner_identity_id": stored.pairing_private_ref.identity_id.as_deref().ok_or(crate::ImError::PermissionDenied)?,
        "did": stored.join_request.did,
        "join_session_id": stored.join_request.join_session_id,
        "join_request_hash": stored.join_request_hash,
        "recipient_device_id": stored.join_request.device_id,
        "source_device_id": approval.authorizing_device_id,
        "operation_id": approval.operation_id,
        "expected_checkpoint": approval.expected_checkpoint,
        "approved_document_hash": canonical_hash(&approval.new_document)?,
        "pairing_confirmation": approval.pairing_confirmation,
        "maximum_attempts_per_round": approval.management_task.as_ref().ok_or(crate::ImError::PermissionDenied)?.max_attempts,
    }))
}

pub(super) fn validate_authority(stored: &StoredJoinSession) -> crate::ImResult<()> {
    let Some(approval) = stored.approval.as_ref() else {
        return Ok(());
    };
    if approval.management_task.is_none() {
        return if approval.management_proof.is_none() {
            Ok(())
        } else {
            Err(crate::ImError::PermissionDenied)
        };
    }
    let task = approval
        .management_task
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    if !matches!(task.max_attempts, 3 | 4) || task.attempts > task.max_attempts {
        return Err(crate::ImError::PermissionDenied);
    }
    let proof = approval
        .management_proof
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    let method = admin_signing_method(
        &approval.new_document,
        &approval.authorizing_device_id,
        &proof.verification_method,
    )?;
    verify_object_proof(
        proof,
        &authorization_payload(stored, approval)?,
        &stored.join_request.did,
        &approval.new_document,
        &method,
    )
}

/// Reuse the existing confirmed-Join custody repair for a Registry-only advance.
/// This does not relax the sender's checkpoint equality or adopt another document.
#[cfg(feature = "sqlite")]
pub(crate) async fn refresh_confirmed_registry_checkpoint(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
    join: &AuthorizedManagementJoin,
    document: &Value,
    registry: &crate::internal::identity_device_join_runtime::DeviceJoinRemoteRegistry,
) -> crate::ImResult<()> {
    let snapshot = {
        let _guard = lock_join_state(core)?;
        let stored = JoinStateStore::new(core)
            .load(&join.join_session_id, DeviceJoinSide::Admin)?
            .ok_or(crate::ImError::PermissionDenied)?;
        if !owns_admin_join(client, &stored)? || stored.phase != DeviceJoinLocalPhase::Authorized {
            return Err(crate::ImError::PermissionDenied);
        }
        stored
    };
    let approval = snapshot
        .approval
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    let confirmed = approval
        .confirmed_authorization
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    if approval.management_task.is_none()
        || *document != approval.new_document
        || registry.did != *client.did()
        || registry.checkpoint.document_version != confirmed.checkpoint.document_version
        || registry.checkpoint.document_hash != confirmed.checkpoint.document_hash
        || registry.checkpoint.registry_version < confirmed.checkpoint.registry_version
    {
        return Err(crate::ImError::PermissionDenied);
    }
    use crate::internal::identity_device_state::{
        DeviceAuthorizationRole, DeviceAuthorizationStatus,
    };
    let source_matches: Vec<_> = registry
        .devices
        .iter()
        .filter(|d| d.device_id == join.authorizing_device_id)
        .collect();
    let target_matches: Vec<_> = registry
        .devices
        .iter()
        .filter(|d| d.device_id == join.recipient_device_id)
        .collect();
    if source_matches.len() != 1 || target_matches.len() != 1 {
        return Err(crate::ImError::PermissionDenied);
    }
    let (source, target) = (source_matches[0], target_matches[0]);
    if source.status != DeviceAuthorizationStatus::Active
        || source.role != DeviceAuthorizationRole::Admin
        || !source.management_ready
        || target.status != DeviceAuthorizationStatus::Active
    {
        return Err(crate::ImError::PermissionDenied);
    }
    crate::internal::identity_root_transfer_runtime::validate_join_key_binding(
        &approval.new_document,
        document,
        client.did(),
        source,
        target,
    )
    .map_err(|_| crate::ImError::PermissionDenied)?;
    let selector = crate::identity::IdentitySelector::Id(client.current_identity().id.clone());
    let prepared =
        prepare_admin_projection_context_async(core, &selector, &registry.checkpoint).await?;
    let local_auth = prepared
        .state
        .authorization
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    if source.signing_key_id != local_auth.signing_key_id
        || source.e2ee_key_id != local_auth.e2ee_key_id
        || source.auth_generation != local_auth.auth_generation
    {
        return Err(crate::ImError::PermissionDenied);
    }
    // Existing local document content must already be confirmed. Do not create,
    // replace, or infer document authority from a newer Registry alone.
    let local_document: Value = serde_json::from_slice(&fs::read(&prepared.document_path)?)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    if local_document != *document {
        return Err(crate::ImError::PermissionDenied);
    }
    let Some(binding) = prepared.binding.as_ref() else {
        return Ok(());
    };
    let pending = approval.provider_document_change_operation_id.as_deref()
        .map(crate::internal::identity_custody::DeviceJoinPendingDocumentChange::ExactOperation)
        .unwrap_or(crate::internal::identity_custody::DeviceJoinPendingDocumentChange::LegacyDocumentDigestCheckpoint);
    crate::internal::identity_custody::adopt_controller_document_async(
        core,
        crate::internal::identity_custody::ControllerDocumentAdoption::DeviceJoin { pending },
        &prepared.did,
        &binding.store_id,
        &binding.identity_id,
        document,
        &registry.checkpoint,
    )
    .await?;
    let current =
        prepare_admin_projection_context_async(core, &selector, &registry.checkpoint).await?;
    if current.binding != prepared.binding || current.did != prepared.did {
        return Err(crate::ImError::PermissionDenied);
    }
    let _guard = lock_join_state(core)?;
    let stored = JoinStateStore::new(core)
        .load(&join.join_session_id, DeviceJoinSide::Admin)?
        .ok_or(crate::ImError::PermissionDenied)?;
    if !owns_admin_join(client, &stored)?
        || stored.approval.as_ref().map(|a| &a.new_document) != Some(document)
    {
        return Err(crate::ImError::PermissionDenied);
    }
    commit_admin_projection_local(core, current, document)
}
