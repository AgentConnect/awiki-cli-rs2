//! Exact, durable cleanup of the unpublished registration candidate superseded by Recovery.

use serde::{Deserialize, Serialize};

use crate::internal::identity_handle_recovery_pending::{
    PendingHandleRecoveryStore, PendingHandleRecoveryV4, PendingRecoveryPhaseV4,
};
use crate::internal::identity_registration_pending::{
    PendingRegistration, PendingRegistrationIdentity, PendingRegistrationPhase,
    PendingRegistrationStore,
};

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RegistrationCandidateCleanup {
    pub(crate) identity: PendingRegistrationIdentity,
    #[serde(default)]
    pub(crate) retry_required: bool,
}

fn unpublished(registration: &PendingRegistration) -> bool {
    registration.phase == PendingRegistrationPhase::Prepared
        && !registration.remote_attempted
        && registration.remote_result.is_none()
}

/// Called while holding the canonical Handle lock, before persisting a new operation.
pub(crate) fn capture(
    core: &crate::ImCore,
    recovery: &PendingHandleRecoveryV4,
) -> crate::ImResult<Option<RegistrationCandidateCleanup>> {
    let handle =
        crate::internal::identity_wire::handle_recovery::canonical_handle(&recovery.full_handle)?;
    let store = PendingRegistrationStore::from_core(core)?;
    let registration = match store.load(&handle.local_part, &handle.domain) {
        Ok(registration) => registration,
        Err(crate::ImError::PermissionDenied | crate::ImError::InvalidInput { .. }) => {
            return Ok(None)
        }
        Err(error) => return Err(error),
    };
    let Some((_, registration)) = registration else {
        return Ok(None);
    };
    if !unpublished(&registration)
        || !unowned(core, recovery, &registration.identity).unwrap_or(false)
    {
        return Ok(None);
    }
    Ok(Some(RegistrationCandidateCleanup {
        identity: registration.identity,
        retry_required: false,
    }))
}

fn unowned(
    core: &crate::ImCore,
    recovery: &PendingHandleRecoveryV4,
    candidate: &PendingRegistrationIdentity,
) -> crate::ImResult<bool> {
    if candidate.did == recovery.identity.did
        || candidate.did.as_str() == recovery.local_previous_did
        || (candidate.controller_store_id == recovery.identity.store_id
            && candidate.controller_identity_id == recovery.identity.identity_id)
        || recovery.remote_result.as_ref().is_some_and(|result| {
            candidate.did.as_str() == result.previous_did
                || candidate.did.as_str() == result.current_did
        })
    {
        return Ok(false);
    }
    let index =
        crate::internal::identity_store::IdentityStore::new(&core.inner().sdk_paths().identities)
            .load_index()?;
    if index.credentials.values().any(|entry| {
        entry.did == candidate.did.as_str()
            || entry.anp_identity_id.as_deref() == Some(candidate.controller_identity_id.as_str())
    }) {
        return Ok(false);
    }
    Ok(
        !crate::internal::identity_custody::historical_handle_dids(core, &recovery.full_handle)?
            .contains(candidate.did.as_str()),
    )
}

/// Called under the same Handle -> owner locks as recovery advance/resume.
pub(crate) async fn finish(
    core: &crate::ImCore,
    store: &PendingHandleRecoveryStore,
    recovery: &mut PendingHandleRecoveryV4,
) -> crate::ImResult<()> {
    if recovery.phase != PendingRecoveryPhaseV4::Applied
        || recovery.registration_candidate_cleanup.is_none()
    {
        return Ok(());
    }
    let result = cleanup(core, recovery).await;
    let revision = recovery.revision;
    if result.is_err()
        && recovery
            .registration_candidate_cleanup
            .as_ref()
            .is_some_and(|cleanup| cleanup.retry_required)
    {
        return result;
    }
    let next_revision = revision
        .checked_add(1)
        .ok_or(crate::ImError::PermissionDenied)?;
    let previous = recovery.registration_candidate_cleanup.take();
    if result.is_err() {
        let mut retry = previous.clone().ok_or(crate::ImError::PermissionDenied)?;
        retry.retry_required = true;
        recovery.registration_candidate_cleanup = Some(retry);
    }
    recovery.revision = next_revision;
    if let Err(error) = store.save_v4_cas(recovery, revision) {
        recovery.registration_candidate_cleanup = previous;
        recovery.revision = revision;
        return Err(error);
    }
    result
}

async fn cleanup(core: &crate::ImCore, recovery: &PendingHandleRecoveryV4) -> crate::ImResult<()> {
    let candidate = &recovery
        .registration_candidate_cleanup
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?
        .identity;
    let result = recovery
        .remote_result
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    let marker = crate::internal::identity_transition_pending::load(
        &core.inner().sdk_paths().local_state.sqlite_path,
        &recovery.operation_id,
    )?
    .ok_or(crate::ImError::PermissionDenied)?;
    if marker.phase != crate::internal::identity_transition_pending::TransitionPhase::Completed
        || marker.owner_identity_id != recovery.owner_identity_id
        || marker.handle != recovery.full_handle
        || marker.current_did != result.current_did
        || marker.previous_did != result.previous_did
        || marker.binding_generation != result.binding_generation
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let handle =
        crate::internal::identity_wire::handle_recovery::canonical_handle(&recovery.full_handle)?;
    let store = PendingRegistrationStore::from_core(core)?;
    let Some((reference, registration)) = store.load(&handle.local_part, &handle.domain)? else {
        // Custody is always deleted first; an absent pending record completes a retry.
        return Ok(());
    };
    if !unpublished(&registration)
        || registration.identity != *candidate
        || !unowned(core, recovery, candidate)?
    {
        return Err(crate::ImError::PermissionDenied);
    }
    // Never delete a candidate which another recovery has adopted since capture.
    for (_, other) in
        PendingHandleRecoveryStore::from_core(core)?.list_v4_for_handle(&recovery.full_handle)?
    {
        if other.operation_id != recovery.operation_id
            && (other.identity.did == candidate.did
                || other.local_previous_did == candidate.did.as_str()
                || other.previous_custody.as_ref().is_some_and(|previous| {
                    previous.did == candidate.did.as_str()
                        || (previous.store_id == candidate.controller_store_id
                            && previous.identity_id == candidate.controller_identity_id)
                }))
        {
            return Err(crate::ImError::PermissionDenied);
        }
    }
    crate::internal::identity_custody::discard_unpublished_registration_async(core, candidate)
        .await?;
    store.delete(&reference)
}
