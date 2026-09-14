//! Web revocation recovery separates exact operation results from current state.

use super::*;
use crate::internal::identity_provider::{
    map_provider_error, ProviderPublicationEvidence, ProviderVerifiedRemoteDocument,
};

pub(super) async fn recover<R: DeviceRevokeRemote, D: DeviceRevokeDocumentResolver>(
    core: &crate::ImCore,
    client: &crate::core::ImClient,
    store: &PendingDeviceRevokeStore,
    remote: &mut R,
    resolver: &mut D,
) -> crate::ImResult<usize> {
    let pending_records = store.list_for_identity(client.did())?;
    let mut completed = 0;
    for (reference, mut pending) in pending_records {
        pending.validate()?;
        validate_local_authorizer(client, &pending)?;
        // The current document alone cannot identify the operation that removed
        // the target. An unknown outcome must retry the original private RPC.
        if pending.remote_result.is_none() {
            let prepared = prepare_revoke_async(
                client,
                pending.operation_id.clone(),
                pending.target_device_id.clone(),
                pending.expected_checkpoint.clone(),
                pending.new_document.clone(),
                pending.authorizing_device.device_id.clone(),
                &pending.authorizing_device.signing_key_id,
                OffsetDateTime::now_utc(),
            )
            .await?;
            let checkpoint = pending.expected_result_checkpoint()?;
            let generation = pending
                .target_auth_generation
                .checked_add(1)
                .ok_or(crate::ImError::PermissionDenied)?;
            let result = remote
                .revoke(&prepared, generation, &checkpoint)
                .await
                .map_err(redact_remote_error)?;
            pending.remote_result = Some(result);
            pending.validate()?;
            if store.save(&pending)? != reference {
                return Err(crate::ImError::PermissionDenied);
            }
        }
        let result = pending
            .remote_result
            .as_ref()
            .ok_or(crate::ImError::PermissionDenied)?;
        converge_current(core, client, &pending, result, remote, resolver).await?;
        store.delete(&reference)?;
        completed += 1;
    }
    Ok(completed)
}

pub(super) async fn converge_current<R: DeviceRevokeRemote, D: DeviceRevokeDocumentResolver>(
    core: &crate::ImCore,
    client: &crate::core::ImClient,
    pending: &PendingDeviceRevoke,
    result: &DeviceRevokeRemoteResult,
    remote: &mut R,
    resolver: &mut D,
) -> crate::ImResult<()> {
    pending.validate()?;
    validate_local_authorizer(client, pending)?;
    if pending.did != *client.did()
        || !pending.did.as_str().starts_with("did:web:")
        || result.checkpoint != pending.expected_result_checkpoint()?
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let registry = remote.registry(client.did()).await?;
    let document = resolver.resolve(client.did()).await?;
    validate_current(pending, result, &registry, &document)?;
    let identity = client
        .runtime()
        .identity_session
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    if identity
        .resume_document_change()
        .await
        .map_err(map_provider_error)?
        .is_some()
    {
        crate::internal::identity_device_join::complete_provider_document_change(
            client,
            &pending.new_document,
            &result.checkpoint,
        )
        .await?;
    }
    // The original candidate is confirmed only at its original checkpoint.
    // Then the latest verified state is adopted monotonically by custody.
    identity
        .adopt_verified_document(ProviderVerifiedRemoteDocument {
            document: document.clone(),
            evidence: ProviderPublicationEvidence {
                document_version: registry.checkpoint.document_version,
                registry_version: registry.checkpoint.registry_version,
                document_digest: registry.checkpoint.document_hash.clone(),
            },
        })
        .await
        .map_err(map_provider_error)?;
    let adopted = identity
        .public_identity()
        .await
        .map_err(map_provider_error)?;
    if adopted.state != crate::internal::identity_provider::ProviderIdentityState::Active
        || adopted.document != document
    {
        return Err(crate::ImError::PermissionDenied);
    }
    write_document_atomic(&client.runtime().did_document_path, &document)?;
    let alias = client
        .current_identity()
        .local_alias
        .as_deref()
        .ok_or(crate::ImError::PermissionDenied)?;
    crate::internal::identity_store::IdentityStore::new(&core.inner().sdk_paths().identities)
        .save_device_state(
            alias,
            IdentityDeviceState {
                schema_version: IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
                mode: IdentityDeviceMode::VNext,
                authorization: Some(DeviceAuthorizationProjection {
                    protocol_device_id: crate::ids::ProtocolDeviceId::parse(
                        &pending.authorizing_device.device_id,
                    )?,
                    signing_key_id: pending.authorizing_device.signing_key_id.clone(),
                    e2ee_key_id: pending.authorizing_device.e2ee_key_id.clone(),
                    status: DeviceAuthorizationStatus::Active,
                    role: DeviceAuthorizationRole::Admin,
                    management_ready: true,
                    auth_generation: pending.authorizing_device.auth_generation,
                }),
                checkpoint: Some(registry.checkpoint),
            },
        )
}

fn validate_current(
    pending: &PendingDeviceRevoke,
    result: &DeviceRevokeRemoteResult,
    registry: &DeviceJoinRemoteRegistry,
    document: &Value,
) -> crate::ImResult<()> {
    if registry.did != pending.did
        || document.get("id").and_then(Value::as_str) != Some(pending.did.as_str())
        || !crate::internal::identity_wire::document::validate_control_document_method(document)
        || registry.checkpoint.document_hash
            != crate::internal::identity_wire::document::document_hash(document)?
        || registry.checkpoint.document_version < result.checkpoint.document_version
        || registry.checkpoint.registry_version < result.checkpoint.registry_version
        || (registry.checkpoint.document_version == result.checkpoint.document_version
            && registry.checkpoint.document_hash != result.checkpoint.document_hash)
        || registry
            .devices
            .iter()
            .filter(|device| {
                device.device_id == pending.authorizing_device.device_id
                    || device.signing_key_id == pending.authorizing_device.signing_key_id
                    || device.e2ee_key_id == pending.authorizing_device.e2ee_key_id
            })
            .count()
            != 1
        || !registry
            .devices
            .iter()
            .any(|device| device == &pending.authorizing_device)
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let targets = registry
        .devices
        .iter()
        .filter(|device| device.device_id == pending.target_device_id)
        .collect::<Vec<_>>();
    if targets.len() != 1
        || targets[0].status != DeviceAuthorizationStatus::Revoked
        || targets[0].auth_generation < result.auth_generation
    {
        return Err(crate::ImError::PermissionDenied);
    }
    validate_manifest_device(document, &pending.authorizing_device)?;
    let manifest = anp::authentication::validate_device_manifest(document)
        .map_err(|_| crate::ImError::PermissionDenied)?
        .ok_or(crate::ImError::PermissionDenied)?;
    if manifest
        .devices
        .iter()
        .any(|device| device.device_id == pending.target_device_id)
    {
        return Err(crate::ImError::PermissionDenied);
    }
    for key in [
        &pending.authorizing_device.signing_key_id,
        &pending.authorizing_device.e2ee_key_id,
    ] {
        if crate::internal::identity_device_join::document_public_key_bytes(document, key)?
            != crate::internal::identity_device_join::document_public_key_bytes(
                &pending.new_document,
                key,
            )?
        {
            return Err(crate::ImError::PermissionDenied);
        }
    }
    Ok(())
}

fn validate_local_authorizer(
    client: &crate::core::ImClient,
    pending: &PendingDeviceRevoke,
) -> crate::ImResult<()> {
    if pending.did != *client.did()
        || client.runtime().key_provider.request_signing_key_id()?
            != pending.authorizing_device.signing_key_id
        || client.runtime().key_provider.agreement_key_id()?
            != pending.authorizing_device.e2ee_key_id
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}
