//! One-device Legacy → Manifest orchestration.

use std::sync::Arc;

use crate::internal::transport::AsyncAuthenticatedRpcTransport;

pub(crate) async fn upgrade(
    core: &crate::core::ImCore,
    selector: crate::identity::IdentitySelector,
) -> crate::ImResult<crate::identity::LegacyUpgradeStatus> {
    let identity = core.identities().resolve_async(selector.clone()).await?;
    let result = upgrade_inner(core, selector).await;
    if let Err(error) = &result {
        if let Some(alias) = identity.local_alias.as_deref() {
            if let Ok(store) =
                crate::internal::identity_legacy_upgrade_pending::PendingLegacyUpgradeStore::from_core(
                    core,
                )
            {
                if let Ok(Some((_, mut pending))) = store.load(alias) {
                    pending.mark_retry_required(legacy_upgrade_error_code(error));
                    let _ = store.save(&pending);
                }
            }
        }
    }
    result
}

async fn upgrade_inner(
    core: &crate::core::ImCore,
    selector: crate::identity::IdentitySelector,
) -> crate::ImResult<crate::identity::LegacyUpgradeStatus> {
    let paths = &core.inner().sdk_paths().identities;
    let store = crate::internal::identity_store::IdentityStore::new(paths);
    let summary = core.identities().resolve_async(selector.clone()).await?;
    let alias = summary
        .local_alias
        .clone()
        .ok_or(crate::ImError::PermissionDenied)?;
    let mut index = store.load_index()?;
    let entry = index
        .credentials
        .get(&alias)
        .ok_or(crate::ImError::PermissionDenied)?;
    if entry.device_state.as_ref().is_some_and(|state| {
        state.mode == crate::internal::identity_device_state::IdentityDeviceMode::VNext
    }) {
        let pending_store =
            crate::internal::identity_legacy_upgrade_pending::PendingLegacyUpgradeStore::from_core(
                core,
            )?;
        if let Some((pending_ref, pending)) = pending_store.load(&alias)? {
            if entry.identity_custody_backend.as_deref() != Some("anp_identity")
                || entry.anp_identity_store_id.as_deref()
                    != Some(pending.identity.custody.store_id.as_str())
                || entry.anp_identity_id.as_deref()
                    != Some(pending.identity.custody.identity_id.as_str())
            {
                return Err(crate::ImError::PermissionDenied);
            }
            store.save_did_document(&entry.dir_name, &pending.identity.target_document)?;
            pending_store.delete(&pending_ref)?;
        }
        let client = core.client(selector)?;
        let mut resolver_transport = crate::internal::transport::CoreHttpTransport::new(&client);
        let remote_document = crate::internal::discovery::did_document::resolve_did_document_async(
            &mut resolver_transport,
            entry.did.as_str(),
        )
        .await?;
        if !crate::internal::identity_legacy_upgrade::vnext_profile_discovery_requires_convergence(
            &remote_document,
        )? {
            store.save_did_document(&entry.dir_name, &remote_document)?;
            return Ok(crate::identity::LegacyUpgradeStatus::Completed);
        }
        let target_document =
            crate::internal::identity_legacy_upgrade::converge_vnext_profile_discovery_with_signer(
                &remote_document,
                client.runtime().key_provider.as_ref(),
            )?
            .ok_or(crate::ImError::PermissionDenied)?;
        let call = crate::internal::identity_wire::update_document::build_update_document_rpc_call(
            crate::internal::identity_wire::UpdateDocumentRpcParams {
                did_document: target_document.clone(),
                is_public: None,
                is_agent: None,
                role: None,
                endpoint_url: None,
            },
        );
        let mut transport = crate::internal::transport::CoreHttpTransport::new(&client);
        transport
            .authenticated_rpc(call.endpoint, call.method, call.params)
            .await?;
        store.save_did_document(&entry.dir_name, &target_document)?;
        return Ok(crate::identity::LegacyUpgradeStatus::Completed);
    }
    let client = core.client(selector)?;
    let context =
        core.inner()
            .identity_vault()
            .ok_or_else(|| crate::ImError::LocalStateUnavailable {
                detail: "Legacy upgrade requires Vault storage".to_owned(),
            })?;
    if entry.vault_migration.is_none() {
        store.migrate_identity_to_vault(
            &alias,
            context.workspace_id(),
            context.vault_context_device_id().as_str(),
            context.vault().as_ref(),
        )?;
        index = store.load_index()?;
    }
    let entry = index.credentials.get(&alias).unwrap();
    let metadata = entry
        .vault_migration
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    let root_ref = metadata.refs.default_signing_private.clone();
    let legacy_document = client.runtime().key_provider.did_document()?;
    let pending_store =
        crate::internal::identity_legacy_upgrade_pending::PendingLegacyUpgradeStore::from_core(
            core,
        )?;
    let (pending_ref, mut pending, resumed_pending) = match pending_store.load(&alias)? {
        Some((secret_ref, pending)) => (secret_ref, pending, true),
        None => {
            let did = crate::ids::Did::parse(&entry.did)?;
            let (custody, enrollment) = crate::internal::identity_custody::prepare_join_enrollment(
                core,
                &did,
                &legacy_document,
            )?;
            let identity =
                crate::internal::identity_legacy_upgrade::build_custodied_legacy_upgrade(
                    &legacy_document,
                    custody,
                    &enrollment,
                    client.runtime().key_provider.as_ref(),
                )?;
            let pending =
                crate::internal::identity_legacy_upgrade_pending::PendingLegacyUpgrade::new(
                    alias.clone(),
                    crate::internal::identity_wire::document::document_hash(&legacy_document)?,
                    root_ref,
                    identity,
                )?;
            let secret_ref = pending_store.save(&pending)?;
            (secret_ref, pending, false)
        }
    };
    let mut remote_already_committed = false;
    if resumed_pending
        && pending.phase
            == crate::internal::identity_legacy_upgrade_pending::PendingLegacyUpgradePhase::Prepared
    {
        let mut resolver_transport = crate::internal::transport::CoreHttpTransport::new(&client);
        let remote_document = crate::internal::discovery::did_document::resolve_did_document_async(
            &mut resolver_transport,
            pending.identity.did.as_str(),
        )
        .await?;
        match pending.reconcile_remote_document(
            &remote_document,
            client.runtime().key_provider.as_ref(),
        )? {
            crate::internal::identity_legacy_upgrade_pending::PendingLegacyUpgradeRemoteState::TargetCommitted => {
                // The update committed and only its response/local commit was
                // lost. Keep the exact document for server idempotence.
                remote_already_committed = true;
            }
            crate::internal::identity_legacy_upgrade_pending::PendingLegacyUpgradeRemoteState::LegacyRebuilt => {
                // The public source of truth is still Legacy. The proof and
                // source extensions are fresh, but device keys stay stable.
                pending_store.save(&pending)?;
            }
        }
    }
    pending.mark_running();
    pending_store.save(&pending)?;
    if pending.phase
        == crate::internal::identity_legacy_upgrade_pending::PendingLegacyUpgradePhase::Prepared
    {
        let update_error = if remote_already_committed {
            None
        } else {
            let call =
                crate::internal::identity_wire::update_document::build_update_document_rpc_call(
                    crate::internal::identity_wire::UpdateDocumentRpcParams {
                        did_document: pending.identity.target_document.clone(),
                        is_public: None,
                        is_agent: None,
                        role: None,
                        endpoint_url: None,
                    },
                );
            let mut legacy_transport = crate::internal::transport::CoreHttpTransport::new(&client);
            legacy_transport
                .authenticated_rpc(call.endpoint, call.method, call.params)
                .await
                .err()
        };

        let managed = crate::internal::identity_custody::pending_join_identity(
            core,
            &pending.identity.did,
            &pending.identity.custody,
        )?;
        let provider: Arc<dyn crate::internal::key_provider::IdentitySigner> =
            if pending.identity.custody.enrollment_id
                == crate::internal::identity_custody::LEGACY_IMPORTED_ACTIVE_ENROLLMENT_ID
            {
                Arc::new(crate::internal::key_provider::AnpIdentitySigner::new_ephemeral(managed))
            } else {
                Arc::new(
                    crate::internal::key_provider::PendingAnpEnrollmentSigner::new(
                        managed,
                        pending.identity.custody.enrollment_id.clone(),
                        pending.identity.target_document.clone(),
                        pending.identity.signing_key_id.clone(),
                        pending.identity.e2ee_key_id.clone(),
                    )?,
                )
            };
        let mut device_transport =
            crate::internal::transport::CoreHttpTransport::new_pending_device(
                &client,
                provider,
                crate::internal::transport::ExpectedDeviceAccessOwned {
                    did: entry.did.clone(),
                    user_id: entry.user_id.clone(),
                    device_id: pending.identity.protocol_device_id.as_str().to_owned(),
                    key_id: pending.identity.signing_key_id.clone(),
                    auth_generation: 1,
                    role: crate::internal::identity_device_state::DeviceAuthorizationRole::Admin,
                    management_ready: true,
                },
            );
        let access_token = match device_transport.refresh_jwt_async().await {
            Ok(token) => token,
            Err(probe_error) => {
                return Err(update_error.unwrap_or(probe_error));
            }
        };
        let registry_call = crate::internal::identity_wire::device_join::build_registry_call(
            &pending.identity.did,
            false,
        );
        let raw = device_transport
            .authenticated_rpc(
                registry_call.endpoint,
                registry_call.method,
                registry_call.params,
            )
            .await?;
        let registry = crate::internal::identity_wire::device_join::parse_registry_result(
            raw,
            &pending.identity.did,
            false,
        )?;
        let device = registry
            .devices
            .iter()
            .find(|device| device.device_id == pending.identity.protocol_device_id.as_str())
            .filter(|device| {
                device.signing_key_id == pending.identity.signing_key_id
                    && device.e2ee_key_id == pending.identity.e2ee_key_id
                    && device.role
                        == crate::internal::identity_device_state::DeviceAuthorizationRole::Admin
                    && device.management_ready
                    && device.auth_generation == 1
            })
            .ok_or(crate::ImError::PermissionDenied)?;
        let _ = device;
        crate::internal::identity_custody::adopt_join_identity(
            core,
            &pending.identity.did,
            &pending.identity.custody,
            &pending.identity.target_document,
            &registry.checkpoint,
        )?;
        pending.phase =
            crate::internal::identity_legacy_upgrade_pending::PendingLegacyUpgradePhase::RemoteCommitted;
        pending.checkpoint = Some(registry.checkpoint);
        pending.access_token = Some(access_token);
        pending.root_imported_at = Some(
            time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .map_err(|_| crate::ImError::PermissionDenied)?,
        );
        pending_store.save(&pending)?;
    }
    let checkpoint = pending
        .checkpoint
        .clone()
        .ok_or(crate::ImError::PermissionDenied)?;
    let access_token = pending
        .access_token
        .clone()
        .ok_or(crate::ImError::PermissionDenied)?;
    let root_imported_at = pending
        .root_imported_at
        .clone()
        .ok_or(crate::ImError::PermissionDenied)?;
    let root_secret = context.vault().open(&pending.root_ref)?;
    let root_pem = zeroize::Zeroizing::new(
        String::from_utf8(root_secret.expose_secret().to_vec())
            .map_err(|_| crate::ImError::PermissionDenied)?,
    );
    let root_der = zeroize::Zeroizing::new(
        crate::internal::identity_root_transfer_runtime::canonical_ed25519_pkcs8_der(&root_pem)?,
    );
    crate::internal::identity_custody::promote_legacy_upgrade_root(
        core,
        &pending.identity,
        &checkpoint,
        &format!("legacy-upgrade:{}", pending.local_alias),
        &root_imported_at,
        root_der,
    )?;
    let projection_storage =
        crate::internal::identity_store::AnpIdentityProjectionStorage::from_core(
            core,
            pending.identity.custody.store_id.clone(),
            pending.identity.custody.identity_id.clone(),
        )?;
    let protocol_device_id = pending.identity.protocol_device_id.clone();
    let signing_key_id = pending.identity.signing_key_id.clone();
    let e2ee_key_id = pending.identity.e2ee_key_id.clone();
    store.save_anp_identity_transition_projection(
        crate::internal::identity_store::SaveIdentityInput {
            local_alias: alias.clone(),
            did: pending.identity.did.clone(),
            unique_id: entry.unique_id.clone(),
            user_id: entry.user_id.clone(),
            display_name: entry.name.clone(),
            handle: entry.handle.clone(),
            full_handle: entry.full_handle.clone(),
            binding_generation: entry.binding_generation.clone(),
            jwt_token: access_token,
            did_document: Some(pending.identity.target_document.clone()),
            key_mode: crate::internal::identity_store::SaveIdentityKeyMode::VNext {
                root_key_id: format!("{}#key-1", pending.identity.did.as_str()),
                device_signing_key_id: signing_key_id.clone(),
                device_e2ee_key_id: e2ee_key_id.clone(),
            },
            device_state: Some(crate::internal::identity_device_state::IdentityDeviceState {
                schema_version:
                    crate::internal::identity_device_state::IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
                mode: crate::internal::identity_device_state::IdentityDeviceMode::VNext,
                authorization: Some(
                    crate::internal::identity_device_state::DeviceAuthorizationProjection {
                        protocol_device_id,
                        signing_key_id,
                        e2ee_key_id,
                        status: crate::internal::identity_device_state::DeviceAuthorizationStatus::Active,
                        role: crate::internal::identity_device_state::DeviceAuthorizationRole::Admin,
                        management_ready: true,
                        auth_generation: 1,
                    },
                ),
                checkpoint: Some(checkpoint),
            }),
            key1_private_pem: String::new(),
            key1_public_pem: String::new(),
            e2ee_signing_private_pem: String::new(),
            e2ee_agreement_private_pem: String::new(),
            daemon_subkey_package: None,
            make_default: entry.is_default,
        },
        projection_storage,
        crate::internal::identity_store::AnpIdentityProjectionReplacement {
            expected_did: &entry.did,
            expected_unique_id: &entry.unique_id,
        },
    )?;
    pending_store.delete(&pending_ref)?;
    Ok(crate::identity::LegacyUpgradeStatus::Completed)
}

pub(crate) fn legacy_upgrade_error_code(error: &crate::ImError) -> &'static str {
    match error {
        crate::ImError::TransportUnavailable { .. } => "transport_unavailable",
        crate::ImError::Service { .. } => "service_error",
        crate::ImError::PermissionDenied => "permission_denied",
        crate::ImError::AuthRequired | crate::ImError::SessionExpired => "auth_required",
        crate::ImError::LocalStateUnavailable { .. } => "local_state_unavailable",
        _ => "legacy_upgrade_failed",
    }
}

#[cfg(test)]
mod tests {
    use super::legacy_upgrade_error_code;

    #[test]
    fn failure_codes_preserve_safe_root_cause_categories() {
        assert_eq!(
            legacy_upgrade_error_code(&crate::ImError::TransportUnavailable {
                detail: "redacted".to_owned(),
            }),
            "transport_unavailable"
        );
        assert_eq!(
            legacy_upgrade_error_code(&crate::ImError::AuthRequired),
            "auth_required"
        );
        assert_eq!(
            legacy_upgrade_error_code(&crate::ImError::Serialization {
                detail: "redacted".to_owned(),
            }),
            "legacy_upgrade_failed"
        );
    }
}
