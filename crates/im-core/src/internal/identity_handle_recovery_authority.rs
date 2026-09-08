//! Bounded, exact-operation public authority checks for committed Recovery.

use crate::identity::HandleRecoveryErrorCode as Code;
use crate::internal::identity_handle_recovery_context::error;
use crate::internal::identity_handle_recovery_pending::PendingHandleRecoveryV4;
use crate::internal::identity_transition_pending::{
    self as transitions, IdentityTransitionMarker, SupersededTransitionEvidence as Evidence,
    TransitionPhase,
};

pub(crate) async fn require_current_binding(
    core: &crate::ImCore,
    pending: &PendingHandleRecoveryV4,
    marker: &IdentityTransitionMarker,
) -> crate::ImResult<()> {
    let result = pending
        .remote_result
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    let binding = crate::internal::handle_discovery::resolve_authoritative_recovery_binding_async(
        core,
        &pending.full_handle,
    )
    .await?;
    let generation = binding
        .binding_generation
        .ok_or(crate::ImError::PermissionDenied)?;
    if binding.did.as_str() != result.current_did {
        return close(
            core,
            pending,
            marker,
            Evidence::HandleBinding {
                observed_did: binding.did.as_str().to_owned(),
                observed_binding_generation: generation,
            },
        );
    }
    if generation != result.binding_generation {
        return Err(error(Code::UnknownEpoch));
    }
    Ok(())
}

/// One exact Handle read and, only if it still names this DID, one verified DID
/// Document read. A 401/403 or an exhausted retry is never terminal evidence.
pub(crate) async fn reconcile_authorization_rejection(
    core: &crate::ImCore,
    pending: &PendingHandleRecoveryV4,
    failure: &crate::ImError,
) -> crate::ImResult<()> {
    if !is_authorization_rejection(failure) {
        return Ok(());
    }
    let marker = transitions::load(
        &core.inner().sdk_paths().local_state.sqlite_path,
        &pending.operation_id,
    )?
    .ok_or(crate::ImError::PermissionDenied)?;
    if marker.phase != TransitionPhase::IdentitySwitched {
        return Ok(());
    }
    match require_current_binding(core, pending, &marker).await {
        Ok(()) => {}
        Err(failure) if matches!(&failure, crate::ImError::Service { code: Some(code), .. } if code == Code::LocalTransitionSuperseded.as_str()) => {
            return Err(failure)
        }
        Err(_) => return Ok(()), // No trustworthy new observation: preserve pending.
    }
    let mut transport = crate::internal::transport::CorePlainTransport::new(core);
    let document = match crate::internal::discovery::did_document::resolve_did_document_async(
        &mut transport,
        pending.identity.did.as_str(),
    )
    .await
    {
        Ok(document) => document,
        Err(_) => return Ok(()),
    };
    if let Some(evidence) = removed_bootstrap_authority(pending, &document)? {
        return close(core, pending, &marker, evidence);
    }
    Ok(())
}

pub(crate) fn removed_bootstrap_authority(
    pending: &PendingHandleRecoveryV4,
    current_document: &serde_json::Value,
) -> crate::ImResult<Option<Evidence>> {
    let initial = &pending.identity.did_document;
    let did = pending.identity.did.as_str();
    let kid = &pending.identity.device_signing_key_id;
    // This DID was first published by this Recovery with this bootstrap key.
    // A currently served, root-verified document removing it is independent
    // authority, unlike a service rejection or a stale cached device status.
    if anp::authentication::verify_active_e1_document(did, initial).is_err()
        || !anp::authentication::is_authentication_authorized(initial, kid)
        || anp::authentication::verify_active_e1_document(did, current_document).is_err()
    {
        return Ok(None);
    }
    if anp::authentication::is_authentication_authorized(current_document, kid) {
        return Ok(None);
    }
    let hash = crate::internal::identity_wire::document::document_hash(current_document)?;
    if pending
        .remote_result
        .as_ref()
        .is_none_or(|result| result.checkpoint.document_hash == hash)
    {
        return Ok(None);
    }
    Ok(Some(Evidence::DeviceAuthorizationRemoved {
        observed_document_hash: hash,
        device_id: pending.identity.protocol_device_id.as_str().to_owned(),
        signing_key_id: kid.clone(),
    }))
}

fn close(
    core: &crate::ImCore,
    pending: &PendingHandleRecoveryV4,
    marker: &IdentityTransitionMarker,
    evidence: Evidence,
) -> crate::ImResult<()> {
    transitions::mark_superseded(
        &core.inner().sdk_paths().local_state.sqlite_path,
        marker,
        &evidence,
        pending
            .intent_hash
            .as_deref()
            .ok_or(crate::ImError::PermissionDenied)?,
        &crate::internal::identity_handle_recovery_runtime::now_second_z()?,
    )?;
    Err(error(Code::LocalTransitionSuperseded))
}

fn is_authorization_rejection(error: &crate::ImError) -> bool {
    matches!(
        error,
        crate::ImError::AuthRequired
            | crate::ImError::SessionExpired
            | crate::ImError::Service {
                status_code: Some(401 | 403),
                ..
            }
    ) || matches!(error, crate::ImError::Service { code: Some(code), .. }
            if matches!(code.as_str(), "device.inactive" | "anp.device_not_eligible" | "anp.device_state_changed" | "1401"))
}
