//! Terminal CAS rejection reconciliation shared by Web management operations.
use super::*;

pub(crate) fn checkpoint_conflict(error: &crate::ImError) -> bool {
    matches!(error, crate::ImError::Service { code: Some(code), .. }
        if matches!(code.as_str(), "device.document_version_conflict"
            | "device.document_hash_conflict" | "device.registry_version_conflict"))
}

/// The caller must durably record the original RPC's terminal CAS rejection
/// before calling. Current state alone never proves a pending operation failed.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn reconcile_rejected(
    core: &crate::ImCore,
    client: &crate::core::ImClient,
    base: &IdentityInternalCheckpoint,
    authorizer: &DeviceJoinRemoteDeviceSummary,
    candidate: &Value,
    operation_id: Option<&str>,
    registry: &DeviceJoinRemoteRegistry,
    current: &Value,
) -> crate::ImResult<()> {
    if !client.did().as_str().starts_with("did:web:")
        || registry.checkpoint.document_version == base.document_version
            && registry.checkpoint.registry_version == base.registry_version
        || validate_current(client, registry, current, candidate, Some(base))? != *authorizer
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let identity = client
        .runtime()
        .identity_session
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    if let Some(change) = identity
        .resume_document_change()
        .await
        .map_err(map_provider_error)?
    {
        let prepared = change.candidate().await.map_err(map_provider_error)?;
        if prepared.candidate_document != *candidate
            || operation_id.is_some_and(|id| id != prepared.operation_id)
        {
            return Err(crate::ImError::PermissionDenied);
        }
        if change.host_phase().await.map_err(map_provider_error)?
            != ProviderDocumentChangePhase::PublicationUncertain
        {
            let attempt = change
                .begin_publication()
                .await
                .map_err(map_provider_error)?;
            change
                .complete(attempt, ProviderPublicationResult::Unknown)
                .await
                .map_err(map_provider_error)?;
        }
        let outcome = change
            .reconcile_rejected(ProviderVerifiedRemoteDocument {
                document: current.clone(),
                evidence: ProviderPublicationEvidence {
                    document_version: registry.checkpoint.document_version,
                    registry_version: registry.checkpoint.registry_version,
                    document_digest: registry.checkpoint.document_hash.clone(),
                },
            })
            .await
            .map_err(map_provider_error)?;
        if !matches!(outcome, ProviderDocumentChangeOutcome::Aborted) {
            return Err(crate::ImError::PermissionDenied);
        }
    }
    adopt(identity.as_ref(), registry, current).await?;
    persist_current(core, client, registry, current, authorizer.clone())
}
