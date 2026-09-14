//! Current-device confirmation for a Web Join, before local activation.
//!
//! The enrollment signer exists only for this fixed read sequence. A consumed
//! Join is historical evidence; current Registry membership and key bytes must
//! independently authorize activation. Failed observations retain the enrollment.

use super::*;
use crate::internal::identity_device_join_runtime::{
    DeviceJoinAccessResult, DeviceJoinHandleBinding, DeviceJoinRemoteAuthorization,
    DeviceJoinRemoteNewDeviceStatus, DeviceJoinRemoteRegistry, DeviceJoinRemoteState,
};
use crate::internal::identity_join_activation_pending::{
    PendingJoinActivation, PendingJoinActivationStore,
};
use crate::internal::identity_provider::*;
use crate::internal::transport::AsyncAuthenticatedRpcTransport;
use std::sync::Arc;

pub(crate) struct WebJoinObservation {
    status: DeviceJoinRemoteNewDeviceStatus,
    registry: DeviceJoinRemoteRegistry,
    document: Value,
    access: DeviceJoinAccessResult,
}

fn snapshot(core: &crate::core::ImCore, session_id: &str) -> crate::ImResult<StoredJoinSession> {
    let _guard = lock_join_state(core)?;
    let stored = JoinStateStore::new(core)
        .load(session_id, DeviceJoinSide::NewDevice)?
        .ok_or(crate::ImError::PermissionDenied)?;
    if !stored.join_request.did.starts_with("did:web:")
        || stored.phase != DeviceJoinLocalPhase::ResponsePrepared
        || stored.join_custody.is_none()
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(stored)
}

/// No arbitrary URL, RPC method or payload can be supplied by the product.
pub(crate) async fn observe(
    core: &crate::core::ImCore,
    session_id: &str,
    document: Value,
) -> crate::ImResult<WebJoinObservation> {
    let stored = snapshot(core, session_id)?;
    validate_authorized_document(&stored.join_request, &document)?;
    let did = crate::ids::Did::parse(&stored.join_request.did)?;
    let config = core.inner().sdk_config();
    let origin = config
        .user_service_endpoint
        .as_ref()
        .unwrap_or(&config.service_base_url);
    let url = reqwest::Url::parse(origin.as_str()).map_err(|_| crate::ImError::PermissionDenied)?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || crate::internal::identity_join_activation_pending::service_domain_from_did(&did)?
            != config.did_domain.trim().to_ascii_lowercase()
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let custody = stored
        .join_custody
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    let reference = ProviderIdentityRef {
        store_id: custody.store_id.clone(),
        identity_id: custody.identity_id.clone(),
        did: did.as_str().to_owned(),
    };
    let provider = crate::internal::identity_custody::controller_custody_provider(core).await?;
    let identity = provider
        .open_identity(&reference)
        .await
        .map_err(map_provider_error)?;
    let public = identity
        .public_identity()
        .await
        .map_err(map_provider_error)?;
    if public.reference != reference {
        return Err(crate::ImError::PermissionDenied);
    }
    let signing_id = method_id(&stored.join_request.signing_public_key, "signing key")?.to_owned();
    let agreement_id = method_id(&stored.join_request.e2ee_public_key, "agreement key")?.to_owned();
    let signer: Arc<dyn crate::internal::key_provider::IdentitySigner> = match public.state {
        ProviderIdentityState::Enrolling => {
            let session = provider
                .resume_enrollment(&reference)
                .await
                .map_err(map_provider_error)?
                .ok_or(crate::ImError::PermissionDenied)?;
            let proposal = session.proposal().await.map_err(map_provider_error)?;
            if proposal.identity != reference || proposal.enrollment_id != custody.enrollment_id {
                return Err(crate::ImError::PermissionDenied);
            }
            validate_proposal(&proposal, &stored.join_request)?;
            Arc::new(
                crate::internal::key_provider::ProviderEnrollmentIdentitySigner::new(
                    &proposal,
                    session,
                    document.clone(),
                    signing_id.clone(),
                    agreement_id.clone(),
                )?,
            )
        }
        ProviderIdentityState::Active => {
            // Custody can already be active after a crash; its keys must still
            // match this enrollment, and no business identity is opened here.
            validate_authorized_document(&stored.join_request, &public.document)?;
            Arc::new(
                crate::internal::key_provider::ProviderIdentitySigner::new_ephemeral(
                    public, identity,
                )?,
            )
        }
        _ => return Err(crate::ImError::PermissionDenied),
    };
    let client = core.client_with_pending_signer(
        did.clone(),
        signer.clone(),
        None,
        did.as_str(),
        &crate::ids::ProtocolDeviceId::parse(&stored.join_request.device_id)?,
    )?;
    let mut transport = crate::internal::transport::CoreHttpTransport::new_pending_device(
        &client,
        signer,
        crate::internal::transport::ExpectedDeviceAccessOwned {
            did: did.as_str().to_owned(),
            user_id: String::new(),
            device_id: stored.join_request.device_id.clone(),
            key_id: signing_id,
            auth_generation: 1,
            role: crate::internal::identity_device_state::DeviceAuthorizationRole::Member,
            management_ready: false,
        },
    );
    let access_token = transport.refresh_jwt_async().await?;
    let call =
        crate::internal::identity_wire::device_join::build_current_device_status_call(session_id)?;
    let raw = transport
        .authenticated_rpc(call.endpoint, call.method, call.params)
        .await?;
    let status =
        crate::internal::identity_wire::device_join::parse_new_status_result(raw, session_id)?;
    let call = crate::internal::identity_wire::device_join::build_registry_call(&did, false);
    let raw = transport
        .authenticated_rpc(call.endpoint, call.method, call.params)
        .await?;
    let registry =
        crate::internal::identity_wire::device_join::parse_registry_result(raw, &did, false)?;
    let handle = declared_handle(&document, &did)?;
    let lookup = crate::internal::handle_discovery::resolve_authoritative_handle_binding_async(
        &client,
        handle.as_str(),
    )
    .await?;
    if lookup.did != did {
        return Err(crate::ImError::PermissionDenied);
    }
    let binding = DeviceJoinHandleBinding {
        full_handle: handle.as_str().to_owned(),
        binding_generation: lookup
            .binding_generation
            .ok_or(crate::ImError::PermissionDenied)?,
    };
    let observation = WebJoinObservation {
        status,
        registry,
        document,
        access: DeviceJoinAccessResult {
            user_id: transport.pending_device_user_id()?,
            access_token,
            handle_binding: Some(binding),
        },
    };
    observation.validate(&stored)?;
    Ok(observation)
}

fn validate_proposal(
    proposal: &ProviderEnrollmentProposal,
    request: &DeviceJoinRequest,
) -> crate::ImResult<()> {
    let ProviderEnrollmentProposalKind::Device {
        device_id,
        signing_key,
        agreement_key,
        profiles,
    } = &proposal.kind
    else {
        return Err(crate::ImError::PermissionDenied);
    };
    for (key, expected) in [
        (signing_key, &request.signing_public_key),
        (agreement_key, &request.e2ee_public_key),
    ] {
        let method = json!({"id": key.kid, "controller": request.did, "type": "Multikey",
            "publicKeyMultibase": key.public_key_multibase});
        if key.kid != method_id(expected, "enrollment key")?
            || public_key_bytes(&extract_identity_public_key(&method)?)?
                != public_key_bytes(&extract_identity_public_key(expected)?)?
        {
            return Err(crate::ImError::PermissionDenied);
        }
    }
    if device_id != &request.device_id || profiles != &request.profiles {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

pub(crate) fn declared_handle(
    document: &Value,
    did: &crate::ids::Did,
) -> crate::ImResult<crate::ids::Handle> {
    let services = anp::wns::extract_handle_service_from_did_document(document);
    let endpoint = services
        .first()
        .and_then(|s| s.get("serviceEndpoint"))
        .and_then(Value::as_str)
        .ok_or(crate::ImError::PermissionDenied)?;
    let url = reqwest::Url::parse(endpoint).map_err(|_| crate::ImError::PermissionDenied)?;
    let local = url
        .path()
        .strip_prefix("/.well-known/handle/")
        .ok_or(crate::ImError::PermissionDenied)?;
    let host = url.host_str().ok_or(crate::ImError::PermissionDenied)?;
    let handle = crate::ids::Handle::parse(format!("{local}.{host}"), "")?;
    crate::core::validate_handle_service_for_did(document, did, &handle)?;
    Ok(handle)
}

fn advances(
    old: &crate::internal::identity_device_state::IdentityInternalCheckpoint,
    current: &crate::internal::identity_device_state::IdentityInternalCheckpoint,
) -> bool {
    current.document_version >= old.document_version
        && current.registry_version >= old.registry_version
        && (current.document_version != old.document_version
            || current.document_hash == old.document_hash)
}

impl WebJoinObservation {
    #[cfg(test)]
    pub(super) fn from_test_parts(
        status: DeviceJoinRemoteNewDeviceStatus,
        registry: DeviceJoinRemoteRegistry,
        document: Value,
        access: DeviceJoinAccessResult,
    ) -> Self {
        Self {
            status,
            registry,
            document,
            access,
        }
    }

    fn validate(
        &self,
        stored: &StoredJoinSession,
    ) -> crate::ImResult<DeviceJoinRemoteAuthorization> {
        let historical = self
            .status
            .authorization
            .as_ref()
            .ok_or(crate::ImError::PermissionDenied)?;
        if self.status.state != DeviceJoinRemoteState::Consumed
            || self.status.join_session_id != stored.join_request.join_session_id
            || self.status.expires_at != stored.join_request.expires_at
            || self.registry.did.as_str() != stored.join_request.did
            || !advances(&historical.checkpoint, &self.registry.checkpoint)
            || self
                .registry
                .devices
                .iter()
                .filter(|device| {
                    device.device_id == historical.device.device_id
                        || device.signing_key_id == historical.device.signing_key_id
                        || device.e2ee_key_id == historical.device.e2ee_key_id
                })
                .count()
                != 1
            || !self
                .registry
                .devices
                .iter()
                .any(|device| device == &historical.device)
        {
            return Err(crate::ImError::PermissionDenied);
        }
        let current = DeviceJoinRemoteAuthorization {
            checkpoint: self.registry.checkpoint.clone(),
            device: historical.device.clone(),
        };
        validate_remote_authorization(stored, &current, &self.document)?;
        let did = crate::ids::Did::parse(&stored.join_request.did)?;
        let binding = self
            .access
            .handle_binding
            .as_ref()
            .ok_or(crate::ImError::PermissionDenied)?;
        if binding.binding_generation.trim().is_empty()
            || declared_handle(&self.document, &did)?.as_str() != binding.full_handle
        {
            return Err(crate::ImError::PermissionDenied);
        }
        crate::internal::access_token::validate_device_access_token(
            &self.access.access_token,
            &crate::internal::access_token::ExpectedDeviceAccess {
                did: did.as_str(),
                user_id: &self.access.user_id,
                device_id: &current.device.device_id,
                key_id: &current.device.signing_key_id,
                auth_generation: current.device.auth_generation,
                role: current.device.role,
                management_ready: false,
            },
        )?;
        Ok(current)
    }
}

pub(crate) async fn prepare(
    core: &crate::core::ImCore,
    session_id: &str,
    observation: WebJoinObservation,
) -> crate::ImResult<PendingJoinActivation> {
    let before = snapshot(core, session_id)?;
    let authorization = observation.validate(&before)?;
    let did = crate::ids::Did::parse(&before.join_request.did)?;
    let custody = before
        .join_custody
        .clone()
        .ok_or(crate::ImError::PermissionDenied)?;
    let store = PendingJoinActivationStore::from_core(core)?;
    let previous = store.load(session_id, &did)?.map(|(_, pending)| pending);
    if previous.as_ref().is_some_and(|old| {
        old.custody != custody
            || old.authorization.device != authorization.device
            || !advances(&old.authorization.checkpoint, &authorization.checkpoint)
    }) {
        return Err(crate::ImError::PermissionDenied);
    }
    let mut pending = PendingJoinActivation::new(
        session_id.to_owned(),
        did.clone(),
        observation.document,
        authorization,
        custody.clone(),
    )?;
    pending.access_result = Some(observation.access);
    pending.validate()?;
    crate::internal::identity_custody::adopt_join_identity_async(
        core,
        &did,
        &custody,
        &pending.resolved_document,
        &pending.authorization.checkpoint,
    )
    .await?;
    #[cfg(test)]
    if FAIL_AFTER_CUSTODY_ADOPTION.swap(false, std::sync::atomic::Ordering::SeqCst) {
        return Err(crate::ImError::Internal {
            message: "injected crash after Web Join custody adoption".to_owned(),
        });
    }
    let _guard = lock_join_state(core)?;
    let state_store = JoinStateStore::new(core);
    let mut current = state_store
        .load(session_id, DeviceJoinSide::NewDevice)?
        .ok_or(crate::ImError::PermissionDenied)?;
    if current != before || store.load(session_id, &did)?.map(|(_, pending)| pending) != previous {
        return Err(invalid_state(
            "Web Join changed during current-device confirmation",
        ));
    }
    store.save(&pending)?;
    current.activation_pending = true;
    state_store.save(&current)?;
    Ok(pending)
}

#[cfg(test)]
pub(super) static FAIL_AFTER_CUSTODY_ADOPTION: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
