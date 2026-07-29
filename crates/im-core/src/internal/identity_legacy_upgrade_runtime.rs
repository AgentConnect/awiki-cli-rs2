//! One-device Legacy → Manifest orchestration.

use serde_json::Value;
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
            store.save_did_document(&entry.dir_name, &pending.generated.target_document)?;
            pending_store.delete(&pending_ref)?;
        }
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
    let root_private = context.vault().open(&root_ref)?;
    let root_private_pem = String::from_utf8(root_private.expose_secret().to_vec())
        .map_err(|_| crate::ImError::PermissionDenied)?;
    let legacy_document = client.runtime().key_provider.did_document()?;
    let pending_store =
        crate::internal::identity_legacy_upgrade_pending::PendingLegacyUpgradeStore::from_core(
            core,
        )?;
    let (pending_ref, mut pending) = match pending_store.load(&alias)? {
        Some(value) => value,
        None => {
            let generated = crate::internal::identity_legacy_upgrade::build_legacy_upgrade(
                &legacy_document,
                &root_private_pem,
            )?;
            let pending =
                crate::internal::identity_legacy_upgrade_pending::PendingLegacyUpgrade::new(
                    alias.clone(),
                    crate::internal::identity_wire::document::document_hash(&legacy_document)?,
                    root_ref,
                    generated,
                )?;
            let secret_ref = pending_store.save(&pending)?;
            (secret_ref, pending)
        }
    };
    pending.mark_running();
    pending_store.save(&pending)?;
    if pending.phase
        == crate::internal::identity_legacy_upgrade_pending::PendingLegacyUpgradePhase::Prepared
    {
        let call = crate::internal::identity_wire::update_document::build_update_document_rpc_call(
            crate::internal::identity_wire::UpdateDocumentRpcParams {
                did_document: pending.generated.target_document.clone(),
                is_public: None,
                is_agent: None,
                role: None,
                endpoint_url: None,
            },
        );
        let mut legacy_transport = crate::internal::transport::CoreHttpTransport::new(&client);
        let update_result = legacy_transport
            .authenticated_rpc(call.endpoint, call.method, call.params)
            .await;

        let provider: Arc<dyn crate::internal::key_provider::KeyMaterialProvider> =
            Arc::new(PendingDeviceProvider {
                document: pending.generated.target_document.clone(),
                signing_key_id: pending.generated.signing_key_id.clone(),
                signing_private_pem: pending.generated.signing_private_pem.clone(),
                e2ee_private_pem: pending.generated.e2ee_private_pem.clone(),
            });
        let mut device_transport =
            crate::internal::transport::CoreHttpTransport::new_pending_device(
                &client,
                provider,
                crate::internal::transport::ExpectedDeviceAccessOwned {
                    did: entry.did.clone(),
                    user_id: entry.user_id.clone(),
                    device_id: pending.generated.protocol_device_id.as_str().to_owned(),
                    key_id: pending.generated.signing_key_id.clone(),
                    auth_generation: 1,
                    role: crate::internal::identity_device_state::DeviceAuthorizationRole::Admin,
                    management_ready: true,
                },
            );
        let access_token = match device_transport.refresh_jwt_async().await {
            Ok(token) => token,
            Err(probe_error) => {
                return Err(update_result.err().unwrap_or(probe_error));
            }
        };
        let registry_call = crate::internal::identity_wire::device_join::build_registry_call(
            &pending.generated.did,
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
            &pending.generated.did,
            false,
        )?;
        let device = registry
            .devices
            .iter()
            .find(|device| device.device_id == pending.generated.protocol_device_id.as_str())
            .filter(|device| {
                device.signing_key_id == pending.generated.signing_key_id
                    && device.e2ee_key_id == pending.generated.e2ee_key_id
                    && device.role
                        == crate::internal::identity_device_state::DeviceAuthorizationRole::Admin
                    && device.management_ready
                    && device.auth_generation == 1
            })
            .ok_or(crate::ImError::PermissionDenied)?;
        let _ = device;
        pending.phase =
            crate::internal::identity_legacy_upgrade_pending::PendingLegacyUpgradePhase::RemoteCommitted;
        pending.checkpoint = Some(registry.checkpoint);
        pending.access_token = Some(access_token);
        pending_store.save(&pending)?;
    }
    store.promote_legacy_identity_to_vnext(
        crate::internal::identity_store::PromoteLegacyIdentityInput {
            local_alias: alias,
            generated: pending.generated,
            checkpoint: pending.checkpoint.ok_or(crate::ImError::PermissionDenied)?,
            access_token: pending
                .access_token
                .ok_or(crate::ImError::PermissionDenied)?,
            workspace_id: context.workspace_id().to_owned(),
            local_vault_device_id: context.vault_context_device_id().as_str().to_owned(),
            vault: context.vault(),
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

struct PendingDeviceProvider {
    document: Value,
    signing_key_id: String,
    signing_private_pem: String,
    e2ee_private_pem: String,
}

impl crate::internal::key_provider::KeyMaterialProvider for PendingDeviceProvider {
    fn did_document(&self) -> crate::ImResult<Value> {
        Ok(self.document.clone())
    }
    fn optional_did_document(&self) -> crate::ImResult<Option<Value>> {
        Ok(Some(self.document.clone()))
    }
    fn device_request_signing_private_pem(&self) -> crate::ImResult<String> {
        Ok(self.signing_private_pem.clone())
    }
    fn device_request_signing_material(
        &self,
    ) -> crate::ImResult<crate::internal::key_provider::DeviceRequestSigningMaterial> {
        Ok(
            crate::internal::key_provider::DeviceRequestSigningMaterial {
                key_id: self.signing_key_id.clone(),
                private_key_pem: self.signing_private_pem.clone(),
            },
        )
    }
    fn did_document_root_private_pem(&self) -> crate::ImResult<String> {
        Err(crate::ImError::PermissionDenied)
    }
    fn e2ee_agreement_private_pem(&self) -> crate::ImResult<String> {
        Ok(self.e2ee_private_pem.clone())
    }
    fn auth_state(&self) -> crate::ImResult<crate::internal::auth::state::AuthStateSnapshot> {
        Ok(Default::default())
    }
    fn valid_auth_token(&self) -> crate::ImResult<Option<String>> {
        Ok(None)
    }
    fn persist_auth_token(&self, _token: &str) -> crate::ImResult<()> {
        Ok(())
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
