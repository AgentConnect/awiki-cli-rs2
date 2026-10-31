use super::*;
pub(super) async fn activate_local(
    core: &crate::ImCore,
    store: &Store,
    s: &Session,
    o: &mut Operation,
) -> crate::ImResult<()> {
    let outcome = activate_local_inner(core, store, s, o).await;
    if let Err(error) = &outcome {
        let code = error_code(error);
        o.last_error = Some(PinRecoveryError {
            code: code.into(),
            retryable: !matches!(
                code,
                "device_revoked"
                    | "device_authorization_changed"
                    | "root_key_mismatch"
                    | "binding_changed"
            ),
            retry_after_seconds: None,
        });
        if matches!(code, "device_revoked" | "device_authorization_changed") {
            o.phase = PinRecoveryPhase::Failed;
        }
        store.save_operation(o)?;
    }
    outcome
}
async fn activate_local_inner(
    core: &crate::ImCore,
    store: &Store,
    s: &Session,
    o: &mut Operation,
) -> crate::ImResult<()> {
    let result = o
        .remote
        .as_ref()
        .and_then(|r| r.device_result.clone())
        .ok_or_else(|| failure("commit_unknown", true))?;
    o.phase = PinRecoveryPhase::LocalActivationPending;
    store.save_operation(o)?;
    fault::cut("local_activation_pending_saved", o)?;
    let completed = custody::complete(core, s, o, &result).await?;
    let public = completed.public;
    let current_checkpoint = completed.checkpoint;
    let authorization = completed.authorization;
    use crate::internal::identity_device_state::*;
    use crate::internal::identity_store::{
        AnpIdentityProjectionStorage, IdentityStore, SaveIdentityInput, SaveIdentityKeyMode,
    };
    let binding = custody::binding(s, o)?;
    let input = SaveIdentityInput {
        local_alias: format!("pin-{}", o.id),
        did: crate::ids::Did::parse(&result.did)?,
        unique_id: o.id.clone(),
        user_id: binding.account_user_id,
        display_name: o.device_label.clone(),
        handle: o
            .target
            .full_handle
            .split('.')
            .next()
            .unwrap_or_default()
            .into(),
        full_handle: o.target.full_handle.clone(),
        binding_generation: Some(o.target.binding_generation.clone()),
        jwt_token: String::new(),
        did_document: Some(public.document.clone()),
        key_mode: SaveIdentityKeyMode::VNext {
            root_key_id: Some(result.root_kid.clone()),
            device_signing_key_id: result.signing_kid.clone(),
            device_e2ee_key_id: result.agreement_kid.clone(),
        },
        device_state: Some(IdentityDeviceState {
            schema_version: 1,
            mode: IdentityDeviceMode::VNext,
            authorization: Some(DeviceAuthorizationProjection {
                protocol_device_id: crate::ids::ProtocolDeviceId::parse(&result.device_id)?,
                signing_key_id: result.signing_kid.clone(),
                e2ee_key_id: result.agreement_kid.clone(),
                status: DeviceAuthorizationStatus::Active,
                role: DeviceAuthorizationRole::Admin,
                management_ready: authorization.management_ready,
                auth_generation: authorization.auth_generation,
            }),
            checkpoint: Some(IdentityInternalCheckpoint {
                document_version: current_checkpoint.document_version,
                registry_version: current_checkpoint.registry_version,
                document_hash: format!("sha256:{}", current_checkpoint.document_digest),
            }),
        }),
        key1_private_pem: String::new(),
        key1_public_pem: String::new(),
        e2ee_signing_private_pem: String::new(),
        e2ee_agreement_private_pem: String::new(),
        daemon_subkey_package: None,
        make_default: true,
    };
    let storage = AnpIdentityProjectionStorage::from_core_pending_auth(
        core,
        public.reference.store_id,
        public.reference.identity_id,
    )?;
    let saved = IdentityStore::new(&core.inner().sdk_paths().identities)
        .save_anp_identity_projection(input, storage)?;
    let selector =
        crate::identity::IdentitySelector::Id(crate::ids::IdentityId::parse(&saved.unique_id)?);
    o.recovered_identity = Some(selector.clone());
    o.phase = PinRecoveryPhase::ActiveSecurePending;
    o.secure_readiness = PinRecoverySecureReadiness::PrekeysPending;
    store.save_operation(o)?;
    let client = core.client_async(selector).await?;
    crate::internal::transport::CoreHttpTransport::new_signature_only(&client)
        .refresh_jwt_async()
        .await?;
    #[cfg(feature = "sqlite")]
    if core.inner().direct_e2ee_v2_enabled() {
        crate::internal::secure_direct::v2_prekey_runtime::ensure_local_prekey_published_from_authorized_document(core,&client).await?;
    }
    #[cfg(feature = "group-e2ee")]
    if core.inner().group_e2ee_v2_enabled() {
        crate::internal::group_e2ee::v2_lifecycle::publish_stable_key_package(
            &client,
            &result.device_id,
            &format!("pin:{}:key-package", o.id),
            &format!("pin:{}:key-package", o.id),
        )
        .await?;
    }
    o.secure_readiness = if core.inner().direct_e2ee_v2_enabled() {
        PinRecoverySecureReadiness::Ready
    } else {
        PinRecoverySecureReadiness::SecurePending
    };
    clear_ephemeral(o);
    o.phase = PinRecoveryPhase::Completed;
    store.save_operation(o)
}
