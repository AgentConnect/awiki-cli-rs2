//! Converge a sibling-published document before beginning a new Join approval.
//! This authority cannot complete or replace a pending local publication.
use super::*;

pub(crate) fn needs_refresh(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
    checkpoint: &crate::internal::identity_device_state::IdentityInternalCheckpoint,
) -> crate::ImResult<bool> {
    let store =
        crate::internal::identity_store::IdentityStore::new(&core.inner().sdk_paths().identities);
    let alias = client
        .current_identity()
        .local_alias
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    let index = store.load_index()?;
    let state = index
        .credentials
        .get(alias)
        .and_then(|entry| entry.device_state.as_ref())
        .ok_or(crate::ImError::PermissionDenied)?;
    Ok(state.checkpoint.as_ref() != Some(checkpoint)
        || canonical_hash(&client.runtime().key_provider.did_document()?)?
            != checkpoint.document_hash)
}

pub(crate) async fn refresh_admin_document(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
    document: &Value,
    registry: &crate::internal::identity_device_join_runtime::DeviceJoinRemoteRegistry,
) -> crate::ImResult<()> {
    use crate::internal::identity_device_state::{
        DeviceAuthorizationRole, DeviceAuthorizationStatus,
    };
    if registry.did != *client.did() {
        return Err(crate::ImError::PermissionDenied);
    }
    validate_current_document(
        document,
        client.did().as_str(),
        &registry.checkpoint.document_hash,
    )?;
    let selector = crate::identity::IdentitySelector::Id(client.current_identity().id.clone());
    let store =
        crate::internal::identity_store::IdentityStore::new(&core.inner().sdk_paths().identities);
    let alias = client
        .current_identity()
        .local_alias
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    let expected = store
        .load_index()?
        .credentials
        .get(alias)
        .cloned()
        .ok_or(crate::ImError::PermissionDenied)?;
    let prepared =
        prepare_admin_projection_context_async(core, &selector, &registry.checkpoint).await?;
    let mut expected_state = expected
        .device_state
        .clone()
        .ok_or(crate::ImError::PermissionDenied)?;
    expected_state.checkpoint = Some(registry.checkpoint.clone());
    if expected_state != prepared.state
        || expected.did != prepared.did.as_str()
        || expected.anp_identity_store_id.as_deref()
            != prepared.binding.as_ref().map(|b| b.store_id.as_str())
        || expected.anp_identity_id.as_deref()
            != prepared.binding.as_ref().map(|b| b.identity_id.as_str())
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let auth = prepared
        .state
        .authorization
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    let sources: Vec<_> = registry
        .devices
        .iter()
        .filter(|device| device.device_id == auth.protocol_device_id.as_str())
        .collect();
    if sources.len() != 1 {
        return Err(crate::ImError::PermissionDenied);
    }
    let source = sources[0];
    if source.status != DeviceAuthorizationStatus::Active
        || source.role != DeviceAuthorizationRole::Admin
        || !source.management_ready
        || source.auth_generation != auth.auth_generation
        || source.signing_key_id != auth.signing_key_id
        || source.e2ee_key_id != auth.e2ee_key_id
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let local_document: Value = serde_json::from_slice(&fs::read(&prepared.document_path)?)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    // Check eligibility in BOTH documents, and compare actual public key bytes.
    for (before, after) in [(&local_document, document), (document, &local_document)] {
        crate::internal::identity_root_transfer_runtime::validate_join_key_binding(
            before,
            after,
            client.did(),
            source,
            source,
        )
        .map_err(|_| crate::ImError::PermissionDenied)?;
    }
    // Only provider-backed controller custody can enforce the pinned root and
    // atomic no-pending-publication gate. Legacy custody remains fail closed.
    let binding = prepared
        .binding
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    crate::internal::identity_custody::adopt_sibling_controller_document_async(
        core,
        &prepared.did,
        &binding.store_id,
        &binding.identity_id,
        document,
        &registry.checkpoint,
    )
    .await?;
    let _guard = lock_join_state(core)?;
    store.commit_converged_admin_document(
        &prepared.local_alias,
        &expected,
        prepared.state,
        document,
    )
}
