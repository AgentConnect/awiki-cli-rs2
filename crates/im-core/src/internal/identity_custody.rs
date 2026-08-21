use std::path::Path;

use anp_identity::{
    AdoptVerifiedDocumentSpec, DocumentUpdateSpec, IdentityState, KeyOrigin, KeyRole, KeyState,
    PublicationState, RequestSigningEnrollmentSpec, RequestSigningMutationSpec,
    RequestSigningPublicKeySpec, VerifiedDocumentEvidence,
};

pub(crate) fn open_controller_store(
    core: &crate::core::ImCore,
) -> crate::ImResult<anp_identity::DidStore> {
    let root = core
        .inner()
        .sdk_paths()
        .identities
        .identity_root_dir
        .join(".anp-identity");
    if let Some(context) = core.inner().identity_vault() {
        let key_id = format!("awiki-workspace-vault:{}", context.workspace_id());
        let open = || {
            anp_identity::DidStore::open_injected(
                &root,
                key_id.clone(),
                context.anp_identity_root_key(),
            )
        };
        match open() {
            Ok(store) => Ok(store),
            Err(anp_identity::DidError::StoreNotFound) => {
                match anp_identity::DidStore::initialize_injected(
                    &root,
                    key_id.clone(),
                    context.anp_identity_root_key(),
                ) {
                    Ok(store) => Ok(store),
                    Err(anp_identity::DidError::Conflict) => open().map_err(map_error),
                    Err(error) => Err(map_error(error)),
                }
            }
            Err(error) => Err(map_error(error)),
        }
    } else {
        open_or_initialize_local_file(&root)
    }
}

/// Opens the daemon-only custody domain used for rootless `daemon-key-1`
/// enrollments. It deliberately has a different store root and root-key file
/// from the controller store.
pub(crate) fn open_daemon_store(
    core: &crate::core::ImCore,
) -> crate::ImResult<anp_identity::DidStore> {
    let root = core
        .inner()
        .sdk_paths()
        .identities
        .identity_root_dir
        .join(".anp-identity-daemon");
    open_or_initialize_local_file(&root)
}

pub(crate) fn provision_registration_identity(
    core: &crate::core::ImCore,
    domain: &str,
    local_part: &str,
) -> crate::ImResult<crate::internal::identity_registration_pending::PendingRegistrationIdentity> {
    let mut controller_store = open_controller_store(core)?;
    let mut controller = match find_unprojected_registration_identity(
        core,
        &controller_store,
        domain,
        local_part,
    )? {
        Some(identity) => identity,
        None => {
            let create =
                crate::internal::identity_generation::vnext_handle_anp_identity_create_spec(
                    domain,
                    local_part,
                    core.inner().sdk_config().anp_service_endpoint.as_ref(),
                    core.inner().sdk_config().anp_service_did.as_ref(),
                )?;
            controller_store
                .create_identity(create.spec)
                .map_err(map_error)?
        }
    };
    let manifest = anp::authentication::validate_device_manifest(controller.document())
        .map_err(|_| crate::ImError::PermissionDenied)?
        .ok_or(crate::ImError::PermissionDenied)?;
    if manifest.devices.len() != 1 {
        return Err(crate::ImError::PermissionDenied);
    }
    let device = &manifest.devices[0];
    let protocol_device_id = crate::ids::ProtocolDeviceId::parse(&device.device_id)?;
    let root_key_id = controller
        .keys()
        .iter()
        .find(|key| key.role == KeyRole::RootControl && key.origin == KeyOrigin::Managed)
        .map(|key| key.kid.clone())
        .ok_or(crate::ImError::PermissionDenied)?;
    let checkpoint = controller
        .checkpoint()
        .cloned()
        .ok_or(crate::ImError::PermissionDenied)?;

    let mut daemon_store = open_daemon_store(core)?;
    let daemon = match daemon_store.open_identity(controller.did()) {
        Ok(identity) => identity,
        Err(anp_identity::DidError::IdentityNotFound) => daemon_store
            .prepare_request_signing_enrollment(RequestSigningEnrollmentSpec {
                verified_document: controller.document().clone(),
                evidence: VerifiedDocumentEvidence {
                    document_version: checkpoint.document_version,
                    registry_version: checkpoint.registry_version,
                    document_digest: checkpoint.document_digest,
                },
                fragment: "daemon-key-1".to_owned(),
                capabilities: anp_identity::Capabilities { did_wba: true },
            })
            .map(|(identity, _)| identity)
            .map_err(map_error)?,
        Err(error) => return Err(map_error(error)),
    };
    let daemon_key = daemon
        .keys()
        .iter()
        .find(|key| {
            key.role == KeyRole::RequestSigning
                && key.origin == KeyOrigin::Managed
                && matches!(key.state, KeyState::Pending | KeyState::Active)
        })
        .ok_or(crate::ImError::PermissionDenied)?;
    let daemon_key_id = format!("{}#daemon-key-1", controller.did());
    if daemon_key.kid != daemon_key_id {
        return Err(crate::ImError::PermissionDenied);
    }

    let (did_document, controller_revision_id) = match controller.pending_revision() {
        Some(pending) => {
            let method = anp::authentication::find_verification_method(
                &pending.candidate_document,
                &daemon_key_id,
            )
            .ok_or(crate::ImError::PermissionDenied)?;
            if method
                .get("publicKeyMultibase")
                .and_then(serde_json::Value::as_str)
                != Some(daemon_key.public_key_multibase.as_str())
                || pending.state == PublicationState::PublicationUncertain
            {
                return Err(crate::ImError::PermissionDenied);
            }
            (pending.candidate_document, Some(pending.revision_id))
        }
        None if request_signing_method_matches(
            controller.document(),
            &daemon_key_id,
            &daemon_key.public_key_multibase,
        ) =>
        {
            (controller.document().clone(), None)
        }
        None => {
            let prepared = controller
                .prepare_update(DocumentUpdateSpec {
                    request_signing_rotation: None,
                    request_signing_mutations: vec![RequestSigningMutationSpec::Add {
                        key: RequestSigningPublicKeySpec {
                            kid: daemon_key_id.clone(),
                            public_key_multibase: daemon_key.public_key_multibase.clone(),
                        },
                    }],
                    device_mutations: Vec::new(),
                    services: None,
                })
                .map_err(map_error)?;
            (prepared.candidate_document, Some(prepared.revision_id))
        }
    };
    let did = crate::ids::Did::parse(controller.did())?;
    let identity = crate::internal::identity_registration_pending::PendingRegistrationIdentity {
        controller_store_id: controller_store.manifest().store_id.clone(),
        controller_identity_id: controller.identity_id().to_owned(),
        daemon_store_id: daemon_store.manifest().store_id.clone(),
        daemon_identity_id: daemon.identity_id().to_owned(),
        did,
        did_document,
        protocol_device_id,
        root_key_id,
        device_signing_key_id: device.signing_key_id.clone(),
        device_e2ee_key_id: device.e2ee_key_id.clone(),
        daemon_key_id,
        controller_revision_id,
    };
    identity.validate()?;
    Ok(identity)
}

pub(crate) fn prepare_join_enrollment(
    core: &crate::core::ImCore,
    did: &crate::ids::Did,
    resolved_document: &serde_json::Value,
) -> crate::ImResult<(
    crate::internal::identity_join_activation_pending::JoinEnrollmentRef,
    anp_identity::PreparedEnrollment,
)> {
    if resolved_document
        .get("id")
        .and_then(serde_json::Value::as_str)
        != Some(did.as_str())
        || !anp::authentication::validate_did_document_binding(resolved_document, true)
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let mut store = open_controller_store(core)?;
    let (identity, prepared) = match store.open_identity(did.as_str()) {
        Ok(identity) => {
            if identity.state() != IdentityState::Enrolling
                || anp_identity::canonical_document_digest(identity.document())
                    .map_err(map_error)?
                    != anp_identity::canonical_document_digest(resolved_document)
                        .map_err(map_error)?
            {
                return Err(crate::ImError::PermissionDenied);
            }
            let prepared = identity
                .pending_enrollment()
                .ok_or(crate::ImError::PermissionDenied)?;
            (identity, prepared)
        }
        Err(anp_identity::DidError::IdentityNotFound) => {
            let device_id = crate::ids::ProtocolDeviceId::generate()?;
            let signing_fragment = format!("{}-sign", device_id.as_str());
            let e2ee_fragment = format!("{}-e2ee", device_id.as_str());
            store
                .prepare_enrollment(anp_identity::EnrollmentSpec {
                    verified_document: resolved_document.clone(),
                    evidence: VerifiedDocumentEvidence {
                        document_version: 1,
                        registry_version: 1,
                        document_digest: crate::internal::identity_wire::document::document_hash(
                            resolved_document,
                        )?,
                    },
                    device_id: device_id.as_str().to_owned(),
                    device_signing_fragment: signing_fragment,
                    device_e2ee_fragment: e2ee_fragment,
                    profiles: crate::internal::identity_generation::vnext_device_profiles(),
                    capabilities: anp_identity::Capabilities { did_wba: true },
                })
                .map_err(map_error)?
        }
        Err(error) => return Err(map_error(error)),
    };
    if prepared.did != did.as_str() || prepared.identity_id != identity.identity_id() {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok((
        crate::internal::identity_join_activation_pending::JoinEnrollmentRef {
            store_id: store.manifest().store_id.clone(),
            identity_id: prepared.identity_id.clone(),
            enrollment_id: prepared.enrollment_id.clone(),
        },
        prepared,
    ))
}

pub(crate) fn provision_handle_recovery_identity(
    core: &crate::core::ImCore,
    domain: &str,
    local_part: &str,
) -> crate::ImResult<crate::internal::identity_handle_recovery_pending::HandleRecoveryIdentityRef> {
    let mut store = open_controller_store(core)?;
    let mut matches = find_unprojected_handle_identities(core, &store, domain, local_part)?;
    let identity = if let Some(identity) = matches.pop() {
        identity
    } else {
        let create = crate::internal::identity_generation::vnext_handle_anp_identity_create_spec(
            domain,
            local_part,
            core.inner().sdk_config().anp_service_endpoint.as_ref(),
            core.inner().sdk_config().anp_service_did.as_ref(),
        )?;
        store.create_identity(create.spec).map_err(map_error)?
    };
    if identity.state() != IdentityState::Active || identity.pending_revision().is_some() {
        return Err(crate::ImError::PermissionDenied);
    }
    let manifest = anp::authentication::validate_device_manifest(identity.document())
        .map_err(|_| crate::ImError::PermissionDenied)?
        .ok_or(crate::ImError::PermissionDenied)?;
    if manifest.devices.len() != 1 {
        return Err(crate::ImError::PermissionDenied);
    }
    let device = &manifest.devices[0];
    let root_key_id = identity
        .keys()
        .iter()
        .find(|key| {
            key.role == KeyRole::RootControl
                && key.origin == KeyOrigin::Managed
                && key.state == KeyState::Active
        })
        .map(|key| key.kid.clone())
        .ok_or(crate::ImError::PermissionDenied)?;
    let reference = crate::internal::identity_handle_recovery_pending::HandleRecoveryIdentityRef {
        store_id: store.manifest().store_id.clone(),
        identity_id: identity.identity_id().to_owned(),
        did: crate::ids::Did::parse(identity.did())?,
        did_document: identity.document().clone(),
        protocol_device_id: crate::ids::ProtocolDeviceId::parse(&device.device_id)?,
        root_key_id,
        device_signing_key_id: device.signing_key_id.clone(),
        device_e2ee_key_id: device.e2ee_key_id.clone(),
    };
    reference.validate()?;
    Ok(reference)
}

pub(crate) fn handle_recovery_identity(
    core: &crate::core::ImCore,
    expected: &crate::internal::identity_handle_recovery_pending::HandleRecoveryIdentityRef,
) -> crate::ImResult<anp_identity::DidIdentity> {
    let store = open_controller_store(core)?;
    if store.manifest().store_id != expected.store_id {
        return Err(crate::ImError::PermissionDenied);
    }
    let identity = store
        .open_identity(expected.did.as_str())
        .map_err(map_error)?;
    if identity.identity_id() != expected.identity_id
        || identity.state() != IdentityState::Active
        || anp_identity::canonical_document_digest(identity.document()).map_err(map_error)?
            != anp_identity::canonical_document_digest(&expected.did_document).map_err(map_error)?
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(identity)
}

pub(crate) fn discard_unpublished_handle_recovery(
    core: &crate::core::ImCore,
    expected: &crate::internal::identity_handle_recovery_pending::HandleRecoveryIdentityRef,
) -> crate::ImResult<()> {
    let mut store = open_controller_store(core)?;
    if store.manifest().store_id != expected.store_id {
        return Err(crate::ImError::PermissionDenied);
    }
    match store.open_identity(expected.did.as_str()) {
        Ok(identity) => {
            if identity.identity_id() != expected.identity_id
                || identity.state() != IdentityState::Active
                || identity.pending_revision().is_some()
            {
                return Err(crate::ImError::PermissionDenied);
            }
            store
                .delete_identity_namespace(expected.did.as_str(), store.generation())
                .map_err(map_error)
        }
        Err(anp_identity::DidError::IdentityNotFound) => Ok(()),
        Err(error) => Err(map_error(error)),
    }
}

fn find_unprojected_handle_identities(
    core: &crate::core::ImCore,
    store: &anp_identity::DidStore,
    domain: &str,
    local_part: &str,
) -> crate::ImResult<Vec<anp_identity::DidIdentity>> {
    let index =
        crate::internal::identity_store::IdentityStore::new(&core.inner().sdk_paths().identities)
            .load_index()?;
    let projected = index
        .credentials
        .values()
        .filter_map(|entry| entry.anp_identity_id.as_deref())
        .collect::<std::collections::BTreeSet<_>>();
    let did_prefix = format!("did:wba:{domain}:user:{local_part}:e1_");
    let endpoint = format!("https://{domain}/.well-known/handle/{local_part}");
    let mut matches = Vec::new();
    for summary in store.list_identities().map_err(map_error)? {
        if summary.state != IdentityState::Active
            || projected.contains(summary.identity_id.as_str())
            || !summary.did.starts_with(&did_prefix)
        {
            continue;
        }
        let identity = store.open_identity(&summary.did).map_err(map_error)?;
        let handle_matches = identity
            .document()
            .get("service")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|services| {
                services.iter().any(|service| {
                    service.get("type").and_then(serde_json::Value::as_str)
                        == Some("ANPHandleService")
                        && service
                            .get("serviceEndpoint")
                            .and_then(serde_json::Value::as_str)
                            == Some(endpoint.as_str())
                })
            });
        let has_daemon = identity.document()["authentication"]
            .as_array()
            .is_some_and(|entries| {
                entries.iter().any(|entry| {
                    entry
                        .as_str()
                        .is_some_and(|kid| kid.ends_with("#daemon-key-1"))
                })
            });
        if handle_matches && !has_daemon && identity.pending_revision().is_none() {
            matches.push(identity);
        }
    }
    if matches.len() > 1 {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(matches)
}

pub(crate) fn sign_join_enrollment(
    core: &crate::core::ImCore,
    did: &crate::ids::Did,
    custody: &crate::internal::identity_join_activation_pending::JoinEnrollmentRef,
    kid: &str,
    message: &[u8],
) -> crate::ImResult<Vec<u8>> {
    open_join_identity(core, did, custody)?
        .sign_pending_enrollment(&custody.enrollment_id, kid, message)
        .map_err(map_error)
}

pub(crate) fn ecdh_join_enrollment(
    core: &crate::core::ImCore,
    did: &crate::ids::Did,
    custody: &crate::internal::identity_join_activation_pending::JoinEnrollmentRef,
    kid: &str,
    peer_public: &[u8],
) -> crate::ImResult<zeroize::Zeroizing<[u8; 32]>> {
    open_join_identity(core, did, custody)?
        .ecdh_pending_enrollment(&custody.enrollment_id, kid, peer_public)
        .map(|shared| zeroize::Zeroizing::new(*shared.as_bytes()))
        .map_err(map_error)
}

pub(crate) fn adopt_join_identity(
    core: &crate::core::ImCore,
    did: &crate::ids::Did,
    custody: &crate::internal::identity_join_activation_pending::JoinEnrollmentRef,
    document: &serde_json::Value,
    checkpoint: &crate::internal::identity_device_state::IdentityInternalCheckpoint,
) -> crate::ImResult<()> {
    let mut identity = open_join_identity(core, did, custody)?;
    let outcome = identity
        .adopt_verified_document(AdoptVerifiedDocumentSpec {
            document: document.clone(),
            evidence: VerifiedDocumentEvidence {
                document_version: checkpoint.document_version,
                registry_version: checkpoint.registry_version,
                document_digest: checkpoint.document_hash.clone(),
            },
        })
        .map_err(map_error)?;
    if !matches!(
        outcome,
        anp_identity::AdoptDocumentOutcome::Activated
            | anp_identity::AdoptDocumentOutcome::Unchanged
    ) || identity.state() != IdentityState::Active
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

pub(crate) fn active_join_identity(
    core: &crate::core::ImCore,
    did: &crate::ids::Did,
    custody: &crate::internal::identity_join_activation_pending::JoinEnrollmentRef,
    signing_kid: &str,
    e2ee_kid: &str,
) -> crate::ImResult<anp_identity::DidIdentity> {
    let identity = open_join_identity(core, did, custody)?;
    if identity.state() != IdentityState::Active {
        return Err(crate::ImError::PermissionDenied);
    }
    for (kid, role) in [
        (signing_kid, KeyRole::DeviceSigning),
        (e2ee_kid, KeyRole::E2eeAgreement),
    ] {
        let key = identity.key_metadata(kid).map_err(map_error)?;
        if key.role != role
            || key.origin != KeyOrigin::Managed
            || key.state != KeyState::Active
            || key.material_erased
        {
            return Err(crate::ImError::PermissionDenied);
        }
    }
    Ok(identity)
}

pub(crate) fn discard_join_enrollment(
    core: &crate::core::ImCore,
    did: &crate::ids::Did,
    custody: &crate::internal::identity_join_activation_pending::JoinEnrollmentRef,
) -> crate::ImResult<()> {
    let mut store = open_controller_store(core)?;
    if store.manifest().store_id != custody.store_id {
        return Err(crate::ImError::PermissionDenied);
    }
    match store.open_identity(did.as_str()) {
        Ok(identity) => {
            if identity.identity_id() != custody.identity_id
                || identity.state() != IdentityState::Enrolling
            {
                return Err(crate::ImError::PermissionDenied);
            }
            store
                .discard_unpublished_enrollment(
                    did.as_str(),
                    &custody.identity_id,
                    store.generation(),
                )
                .map_err(map_error)
        }
        Err(anp_identity::DidError::IdentityNotFound) => Ok(()),
        Err(error) => Err(map_error(error)),
    }
}

fn open_join_identity(
    core: &crate::core::ImCore,
    did: &crate::ids::Did,
    custody: &crate::internal::identity_join_activation_pending::JoinEnrollmentRef,
) -> crate::ImResult<anp_identity::DidIdentity> {
    let store = open_controller_store(core)?;
    if store.manifest().store_id != custody.store_id {
        return Err(crate::ImError::PermissionDenied);
    }
    let identity = store.open_identity(did.as_str()).map_err(map_error)?;
    if identity.identity_id() != custody.identity_id {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(identity)
}

pub(crate) fn begin_registration_publication(
    core: &crate::core::ImCore,
    identity: &crate::internal::identity_registration_pending::PendingRegistrationIdentity,
) -> crate::ImResult<()> {
    let Some(revision_id) = identity.controller_revision_id.as_deref() else {
        return Ok(());
    };
    let mut controller = open_registration_controller(core, identity)?;
    let pending = controller
        .pending_revision()
        .ok_or(crate::ImError::PermissionDenied)?;
    if pending.revision_id != revision_id || pending.state != PublicationState::Prepared {
        return Err(crate::ImError::PermissionDenied);
    }
    controller.begin_publication(revision_id).map_err(map_error)
}

pub(crate) fn reconcile_registration_publication(
    core: &crate::core::ImCore,
    identity: &crate::internal::identity_registration_pending::PendingRegistrationIdentity,
    remote_committed: bool,
) -> crate::ImResult<()> {
    let Some(revision_id) = identity.controller_revision_id.as_deref() else {
        return ensure_controller_document(core, identity);
    };
    let mut controller = open_registration_controller(core, identity)?;
    let Some(pending) = controller.pending_revision() else {
        return ensure_document_matches(controller.document(), &identity.did_document);
    };
    if pending.revision_id != revision_id {
        return Err(crate::ImError::PermissionDenied);
    }
    match pending.state {
        PublicationState::PublicationInFlight => controller
            .mark_publication_uncertain(revision_id)
            .map_err(map_error)?,
        PublicationState::PublicationUncertain => {}
        PublicationState::Published if remote_committed => {
            controller.commit_update(revision_id).map_err(map_error)?;
            return ensure_document_matches(controller.document(), &identity.did_document);
        }
        PublicationState::Prepared if !remote_committed => return Ok(()),
        _ => return Err(crate::ImError::PermissionDenied),
    }
    let observed = if remote_committed {
        identity.did_document.clone()
    } else {
        controller.document().clone()
    };
    let outcome = controller
        .reconcile_update(revision_id, &observed)
        .map_err(map_error)?;
    match (remote_committed, outcome) {
        (true, anp_identity::ReconcileOutcome::Committed)
        | (false, anp_identity::ReconcileOutcome::RemoteOld) => Ok(()),
        _ => Err(crate::ImError::PermissionDenied),
    }
}

pub(crate) fn commit_registration_publication(
    core: &crate::core::ImCore,
    identity: &crate::internal::identity_registration_pending::PendingRegistrationIdentity,
) -> crate::ImResult<()> {
    let Some(revision_id) = identity.controller_revision_id.as_deref() else {
        return ensure_controller_document(core, identity);
    };
    let mut controller = open_registration_controller(core, identity)?;
    let Some(pending) = controller.pending_revision() else {
        return ensure_document_matches(controller.document(), &identity.did_document);
    };
    if pending.revision_id != revision_id {
        return Err(crate::ImError::PermissionDenied);
    }
    match pending.state {
        PublicationState::PublicationInFlight => {
            controller.mark_published(revision_id).map_err(map_error)?;
            controller.commit_update(revision_id).map_err(map_error)?;
        }
        PublicationState::Published => controller.commit_update(revision_id).map_err(map_error)?,
        PublicationState::PublicationUncertain => {
            let outcome = controller
                .reconcile_update(revision_id, &identity.did_document)
                .map_err(map_error)?;
            if outcome != anp_identity::ReconcileOutcome::Committed {
                return Err(crate::ImError::PermissionDenied);
            }
        }
        PublicationState::Prepared => return Err(crate::ImError::PermissionDenied),
    }
    ensure_document_matches(controller.document(), &identity.did_document)
}

pub(crate) fn refresh_registration_document(
    core: &crate::core::ImCore,
    identity: &crate::internal::identity_registration_pending::PendingRegistrationIdentity,
) -> crate::ImResult<(serde_json::Value, String)> {
    let revision_id = identity
        .controller_revision_id
        .as_deref()
        .ok_or(crate::ImError::PermissionDenied)?;
    let mut controller = open_registration_controller(core, identity)?;
    let pending = controller
        .pending_revision()
        .ok_or(crate::ImError::PermissionDenied)?;
    if pending.revision_id != revision_id || pending.state != PublicationState::Prepared {
        return Err(crate::ImError::PermissionDenied);
    }
    controller.abort_update(revision_id).map_err(map_error)?;
    let daemon = open_registration_daemon(core, identity)?;
    let daemon_key = daemon
        .key_metadata(&identity.daemon_key_id)
        .map_err(map_error)?;
    let prepared = controller
        .prepare_update(DocumentUpdateSpec {
            request_signing_rotation: None,
            request_signing_mutations: vec![RequestSigningMutationSpec::Add {
                key: RequestSigningPublicKeySpec {
                    kid: identity.daemon_key_id.clone(),
                    public_key_multibase: daemon_key.public_key_multibase.clone(),
                },
            }],
            device_mutations: Vec::new(),
            services: None,
        })
        .map_err(map_error)?;
    Ok((prepared.candidate_document, prepared.revision_id))
}

pub(crate) fn activate_registration_daemon(
    core: &crate::core::ImCore,
    identity: &crate::internal::identity_registration_pending::PendingRegistrationIdentity,
) -> crate::ImResult<()> {
    let controller = open_registration_controller(core, identity)?;
    ensure_document_matches(controller.document(), &identity.did_document)?;
    let checkpoint = controller
        .checkpoint()
        .cloned()
        .ok_or(crate::ImError::PermissionDenied)?;
    let mut daemon = open_registration_daemon(core, identity)?;
    let outcome = daemon
        .adopt_verified_document(AdoptVerifiedDocumentSpec {
            document: identity.did_document.clone(),
            evidence: VerifiedDocumentEvidence {
                document_version: checkpoint.document_version,
                registry_version: checkpoint.registry_version,
                document_digest: checkpoint.document_digest,
            },
        })
        .map_err(map_error)?;
    if !matches!(
        outcome,
        anp_identity::AdoptDocumentOutcome::Activated
            | anp_identity::AdoptDocumentOutcome::Unchanged
    ) || daemon.state() != IdentityState::Active
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

pub(crate) fn discard_unpublished_registration(
    core: &crate::core::ImCore,
    identity: &crate::internal::identity_registration_pending::PendingRegistrationIdentity,
) -> crate::ImResult<()> {
    let mut daemon_store = open_daemon_store(core)?;
    if daemon_store.manifest().store_id != identity.daemon_store_id {
        return Err(crate::ImError::PermissionDenied);
    }
    match daemon_store.open_identity(identity.did.as_str()) {
        Ok(daemon) => {
            if daemon.identity_id() != identity.daemon_identity_id
                || daemon.state() != IdentityState::Enrolling
            {
                return Err(crate::ImError::PermissionDenied);
            }
            daemon_store
                .discard_unpublished_enrollment(
                    identity.did.as_str(),
                    &identity.daemon_identity_id,
                    daemon_store.generation(),
                )
                .map_err(map_error)?;
        }
        Err(anp_identity::DidError::IdentityNotFound) => {}
        Err(error) => return Err(map_error(error)),
    }

    let mut controller_store = open_controller_store(core)?;
    if controller_store.manifest().store_id != identity.controller_store_id {
        return Err(crate::ImError::PermissionDenied);
    }
    match controller_store.open_identity(identity.did.as_str()) {
        Ok(mut controller) => {
            if controller.identity_id() != identity.controller_identity_id {
                return Err(crate::ImError::PermissionDenied);
            }
            if let Some(pending) = controller.pending_revision() {
                if Some(pending.revision_id.as_str()) != identity.controller_revision_id.as_deref()
                    || pending.state != PublicationState::Prepared
                {
                    return Err(crate::ImError::PermissionDenied);
                }
                controller
                    .abort_update(&pending.revision_id)
                    .map_err(map_error)?;
            }
            controller_store
                .delete_identity_namespace(identity.did.as_str(), controller_store.generation())
                .map_err(map_error)?;
        }
        Err(anp_identity::DidError::IdentityNotFound) => {}
        Err(error) => return Err(map_error(error)),
    }
    Ok(())
}

pub(crate) fn registration_controller_identity(
    core: &crate::core::ImCore,
    identity: &crate::internal::identity_registration_pending::PendingRegistrationIdentity,
) -> crate::ImResult<anp_identity::DidIdentity> {
    let controller = open_registration_controller(core, identity)?;
    ensure_document_matches(controller.document(), &identity.did_document)?;
    Ok(controller)
}

pub(crate) fn registration_controller_signing_identity(
    core: &crate::core::ImCore,
    identity: &crate::internal::identity_registration_pending::PendingRegistrationIdentity,
) -> crate::ImResult<anp_identity::DidIdentity> {
    let controller = open_registration_controller(core, identity)?;
    for (kid, role) in [
        (&identity.root_key_id, KeyRole::RootControl),
        (&identity.device_signing_key_id, KeyRole::DeviceSigning),
        (&identity.device_e2ee_key_id, KeyRole::E2eeAgreement),
    ] {
        let metadata = controller.key_metadata(kid).map_err(map_error)?;
        if metadata.role != role
            || metadata.origin != KeyOrigin::Managed
            || metadata.state != KeyState::Active
            || metadata.material_erased
        {
            return Err(crate::ImError::PermissionDenied);
        }
    }
    Ok(controller)
}

fn find_unprojected_registration_identity(
    core: &crate::core::ImCore,
    store: &anp_identity::DidStore,
    domain: &str,
    local_part: &str,
) -> crate::ImResult<Option<anp_identity::DidIdentity>> {
    let index =
        crate::internal::identity_store::IdentityStore::new(&core.inner().sdk_paths().identities)
            .load_index()?;
    let projected = index
        .credentials
        .values()
        .filter_map(|entry| entry.anp_identity_id.as_deref())
        .collect::<std::collections::BTreeSet<_>>();
    let did_prefix = format!("did:wba:{domain}:user:{local_part}:e1_");
    let endpoint = format!("https://{domain}/.well-known/handle/{local_part}");
    let mut matches = Vec::new();
    for summary in store.list_identities().map_err(map_error)? {
        if summary.state != IdentityState::Active
            || projected.contains(summary.identity_id.as_str())
            || !summary.did.starts_with(&did_prefix)
        {
            continue;
        }
        let identity = store.open_identity(&summary.did).map_err(map_error)?;
        let handle_matches = identity
            .document()
            .get("service")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|services| {
                services.iter().any(|service| {
                    service.get("type").and_then(serde_json::Value::as_str)
                        == Some("ANPHandleService")
                        && service
                            .get("serviceEndpoint")
                            .and_then(serde_json::Value::as_str)
                            == Some(endpoint.as_str())
                })
            });
        if handle_matches {
            matches.push(identity);
        }
    }
    if matches.len() > 1 {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(matches.pop())
}

fn open_registration_controller(
    core: &crate::core::ImCore,
    expected: &crate::internal::identity_registration_pending::PendingRegistrationIdentity,
) -> crate::ImResult<anp_identity::DidIdentity> {
    let store = open_controller_store(core)?;
    if store.manifest().store_id != expected.controller_store_id {
        return Err(crate::ImError::PermissionDenied);
    }
    let identity = store
        .open_identity(expected.did.as_str())
        .map_err(map_error)?;
    if identity.identity_id() != expected.controller_identity_id {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(identity)
}

fn open_registration_daemon(
    core: &crate::core::ImCore,
    expected: &crate::internal::identity_registration_pending::PendingRegistrationIdentity,
) -> crate::ImResult<anp_identity::DidIdentity> {
    let store = open_daemon_store(core)?;
    if store.manifest().store_id != expected.daemon_store_id {
        return Err(crate::ImError::PermissionDenied);
    }
    let identity = store
        .open_identity(expected.did.as_str())
        .map_err(map_error)?;
    if identity.identity_id() != expected.daemon_identity_id {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(identity)
}

fn ensure_controller_document(
    core: &crate::core::ImCore,
    identity: &crate::internal::identity_registration_pending::PendingRegistrationIdentity,
) -> crate::ImResult<()> {
    let controller = open_registration_controller(core, identity)?;
    ensure_document_matches(controller.document(), &identity.did_document)
}

fn ensure_document_matches(
    actual: &serde_json::Value,
    expected: &serde_json::Value,
) -> crate::ImResult<()> {
    if crate::internal::identity_wire::document::document_hash(actual)?
        != crate::internal::identity_wire::document::document_hash(expected)?
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

fn request_signing_method_matches(
    document: &serde_json::Value,
    kid: &str,
    public_key_multibase: &str,
) -> bool {
    anp::authentication::find_verification_method(document, kid).is_some_and(|method| {
        method
            .get("publicKeyMultibase")
            .and_then(serde_json::Value::as_str)
            == Some(public_key_multibase)
            && anp::authentication::is_authentication_authorized(document, kid)
            && !anp::authentication::is_assertion_method_authorized(document, kid)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registration_provisioning_recovers_the_same_controller_and_daemon_namespaces() {
        let root = tempfile::tempdir().unwrap();
        let core = crate::ImCore::new(test_config(), test_paths(root.path())).unwrap();

        let first = provision_registration_identity(&core, "example.test", "alice").unwrap();
        let recovered = provision_registration_identity(&core, "example.test", "alice").unwrap();

        assert_eq!(recovered, first);
        let controller = open_controller_store(&core).unwrap();
        let daemon = open_daemon_store(&core).unwrap();
        assert_eq!(controller.list_identities().unwrap().len(), 1);
        assert_eq!(daemon.list_identities().unwrap().len(), 1);
        assert_ne!(first.controller_store_id, first.daemon_store_id);
        let controller_root =
            std::fs::read(root.path().join("identities/.anp-identity/root-key.b64u")).unwrap();
        let daemon_root = std::fs::read(
            root.path()
                .join("identities/.anp-identity-daemon/root-key.b64u"),
        )
        .unwrap();
        assert_ne!(controller_root, daemon_root);
        let encoded = serde_json::to_string(&first).unwrap();
        assert!(!encoded.contains("PRIVATE KEY"));
        assert!(!encoded.contains("private_pem"));
    }

    #[test]
    fn unpublished_registration_cleanup_removes_both_namespaces_and_is_idempotent() {
        let root = tempfile::tempdir().unwrap();
        let core = crate::ImCore::new(test_config(), test_paths(root.path())).unwrap();
        let identity = provision_registration_identity(&core, "example.test", "alice").unwrap();

        discard_unpublished_registration(&core, &identity).unwrap();
        discard_unpublished_registration(&core, &identity).unwrap();

        assert!(open_controller_store(&core)
            .unwrap()
            .list_identities()
            .unwrap()
            .is_empty());
        assert!(open_daemon_store(&core)
            .unwrap()
            .list_identities()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn join_enrollment_reuses_the_same_pending_device_after_restart() {
        let root = tempfile::tempdir().unwrap();
        let core = crate::ImCore::new(test_config(), test_paths(root.path())).unwrap();
        let generated = crate::internal::identity_generation::generate_vnext_handle_identity_with_default_daemon_subkey(
            "example.test",
            "joined",
            None,
            None,
        )
        .unwrap();

        let first =
            prepare_join_enrollment(&core, &generated.did, &generated.did_document).unwrap();
        let recovered =
            prepare_join_enrollment(&core, &generated.did, &generated.did_document).unwrap();

        assert_eq!(recovered, first);
        assert_eq!(
            open_controller_store(&core)
                .unwrap()
                .list_identities()
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn handle_recovery_provisioning_reuses_and_discards_one_unpublished_identity() {
        let root = tempfile::tempdir().unwrap();
        let core = crate::ImCore::new(test_config(), test_paths(root.path())).unwrap();

        let first = provision_handle_recovery_identity(&core, "example.test", "recovered").unwrap();
        let recovered =
            provision_handle_recovery_identity(&core, "example.test", "recovered").unwrap();

        assert_eq!(recovered, first);
        handle_recovery_identity(&core, &first)
            .unwrap()
            .sign_device_assertion(&first.device_signing_key_id, b"recovery proof")
            .unwrap();
        let encoded = serde_json::to_string(&first).unwrap();
        assert!(!encoded.contains("PRIVATE KEY"));
        assert!(!encoded.contains("private_pem"));
        discard_unpublished_handle_recovery(&core, &first).unwrap();
        discard_unpublished_handle_recovery(&core, &first).unwrap();
        assert!(open_controller_store(&core)
            .unwrap()
            .list_identities()
            .unwrap()
            .is_empty());
    }

    fn test_config() -> crate::ImCoreConfig {
        crate::ImCoreConfig {
            service_base_url: crate::ServiceEndpoint::parse("https://example.test").unwrap(),
            did_domain: "example.test".to_owned(),
            client_version_info: None,
            user_service_endpoint: None,
            message_service_endpoint: None,
            mail_service_endpoint: None,
            anp_service_endpoint: None,
            anp_service_did: None,
            ca_bundle: None,
            transport_policy: crate::MessageTransportPolicy::HttpOnly,
        }
    }

    fn test_paths(root: &std::path::Path) -> crate::ImCorePaths {
        crate::ImCorePaths {
            identities: crate::IdentityRegistryPaths {
                identity_root_dir: root.join("identities"),
                registry_path: root.join("identities").join("registry.json"),
                default_identity_path: Some(root.join("identities").join("default")),
            },
            local_state: crate::LocalStatePaths {
                sqlite_path: root.join("local").join("im.sqlite"),
            },
            runtime: crate::RuntimePaths {
                cache_dir: root.join("cache"),
                temp_dir: root.join("tmp"),
            },
        }
    }
}

fn open_or_initialize_local_file(root: &Path) -> crate::ImResult<anp_identity::DidStore> {
    match anp_identity::DidStore::open_local_file(root) {
        Ok(store) => Ok(store),
        Err(anp_identity::DidError::StoreNotFound) => {
            match anp_identity::DidStore::initialize_local_file(root) {
                Ok(store) => Ok(store),
                Err(anp_identity::DidError::Conflict) => {
                    anp_identity::DidStore::open_local_file(root).map_err(map_error)
                }
                Err(error) => Err(map_error(error)),
            }
        }
        Err(error) => Err(map_error(error)),
    }
}

pub(crate) fn map_error(error: anp_identity::DidError) -> crate::ImError {
    match error {
        anp_identity::DidError::IdentityNotFound => crate::ImError::IdentityNotFound {
            selector: "anp-identity".to_owned(),
        },
        anp_identity::DidError::Conflict => crate::ImError::LocalStateUnavailable {
            detail: "anp identity store generation changed; reload is required".to_owned(),
        },
        anp_identity::DidError::RootKeyMismatch | anp_identity::DidError::ProviderUnavailable => {
            crate::ImError::PermissionDenied
        }
        error => crate::ImError::LocalStateUnavailable {
            detail: format!("anp identity store operation failed: {error}"),
        },
    }
}
