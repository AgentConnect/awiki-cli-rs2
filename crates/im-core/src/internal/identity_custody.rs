#[cfg(test)]
use std::path::Path;

pub(crate) const LEGACY_IMPORTED_ACTIVE_ENROLLMENT_ID: &str = "legacy-imported-active-v1";

#[cfg(test)]
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

pub(crate) fn open_controller_manager(
    core: &crate::core::ImCore,
) -> crate::ImResult<anp_identity::IdentityManager> {
    let root = core
        .inner()
        .sdk_paths()
        .identities
        .identity_root_dir
        .join(".anp-identity");
    if let Some(context) = core.inner().identity_vault() {
        let key_id = format!("awiki-workspace-vault:{}", context.workspace_id());
        let config = || anp_identity::IdentityManagerConfig {
            state_root: root.clone(),
            root_key: anp_identity::RootKeySource::Injected(anp_identity::InjectedStoreKey::new(
                key_id.clone(),
                context.anp_identity_root_key(),
            )),
        };
        open_or_initialize_manager(config)
    } else {
        let config = || anp_identity::IdentityManagerConfig {
            state_root: root.clone(),
            root_key: anp_identity::RootKeySource::LocalPrivateFile,
        };
        open_or_initialize_manager(config)
    }
}

fn open_or_initialize_manager(
    config: impl Fn() -> anp_identity::IdentityManagerConfig,
) -> crate::ImResult<anp_identity::IdentityManager> {
    match anp_identity::IdentityManager::open(config()) {
        Ok(manager) => Ok(manager),
        Err(anp_identity::IdentityError::StoreNotFound) => {
            match anp_identity::IdentityManager::initialize(config()) {
                Ok(manager) => Ok(manager),
                Err(anp_identity::IdentityError::Conflict)
                | Err(anp_identity::IdentityError::IdentityAlreadyExists) => {
                    anp_identity::IdentityManager::open(config()).map_err(map_facade_error)
                }
                Err(error) => Err(map_facade_error(error)),
            }
        }
        Err(error) => Err(map_facade_error(error)),
    }
}

pub(crate) fn provision_registration_identity(
    core: &crate::core::ImCore,
    domain: &str,
    local_part: &str,
) -> crate::ImResult<crate::internal::identity_registration_pending::PendingRegistrationIdentity> {
    let mut manager = open_controller_manager(core)?;
    let controller =
        match find_unprojected_registration_identity(core, &manager, domain, local_part)? {
            Some(identity) => identity,
            None => {
                let create =
                    crate::internal::identity_generation::vnext_handle_anp_identity_create_spec(
                        domain,
                        local_part,
                        core.inner().sdk_config().anp_service_endpoint.as_ref(),
                        core.inner().sdk_config().anp_service_did.as_ref(),
                    )?;
                manager
                    .create(native_create_spec(create.spec))
                    .map_err(map_facade_error)?
            }
        };
    let public = controller.public_identity().map_err(map_facade_error)?;
    let manifest = anp::authentication::validate_device_manifest(public.document.as_value())
        .map_err(|_| crate::ImError::PermissionDenied)?
        .ok_or(crate::ImError::PermissionDenied)?;
    if manifest.devices.len() != 1 {
        return Err(crate::ImError::PermissionDenied);
    }
    let device = &manifest.devices[0];
    let protocol_device_id = crate::ids::ProtocolDeviceId::parse(&device.device_id)?;
    let root_key_id = public
        .active_keys
        .iter()
        .find(|key| {
            key.purposes
                .contains(&anp_identity::KeyPurpose::RootControl)
        })
        .map(|key| key.kid.clone())
        .ok_or(crate::ImError::PermissionDenied)?;
    let did = crate::ids::Did::parse(&public.reference.did)?;
    let identity = crate::internal::identity_registration_pending::PendingRegistrationIdentity {
        controller_store_id: public.reference.store_id,
        controller_identity_id: public.reference.identity_id,
        did,
        did_document: public.document.into_value(),
        protocol_device_id,
        root_key_id,
        device_signing_key_id: device.signing_key_id.clone(),
        device_e2ee_key_id: device.e2ee_key_id.clone(),
        legacy_daemon_authorization: false,
        controller_revision_id: None,
    };
    identity.validate()?;
    Ok(identity)
}

pub(crate) async fn provision_registration_identity_async(
    core: &crate::core::ImCore,
    domain: &str,
    local_part: &str,
) -> crate::ImResult<crate::internal::identity_registration_pending::PendingRegistrationIdentity> {
    #[cfg(feature = "provider-traits")]
    if let Some(custody) = core.inner().identity_custody_provider() {
        let paths = core.inner().sdk_paths().identities.clone();
        let projected = crate::internal::runtime::worker::run_blocking(move || {
            crate::internal::identity_store::IdentityStore::new(&paths)
                .load_index()
                .map(|index| {
                    index
                        .credentials
                        .into_values()
                        .filter_map(|entry| entry.anp_identity_id)
                        .collect::<std::collections::BTreeSet<_>>()
                })
        })
        .await
        .map_err(|error| crate::ImError::Internal {
            message: error.to_string(),
        })??;
        let did_prefix = format!("did:wba:{domain}:user:{local_part}:e1_");
        let endpoint = format!("https://{domain}/.well-known/handle/{local_part}");
        let mut matches = Vec::new();
        for descriptor in custody
            .list_identities()
            .await
            .map_err(crate::internal::identity_provider::map_provider_error)?
        {
            if descriptor.state != crate::internal::identity_provider::ProviderIdentityState::Active
                || projected.contains(&descriptor.reference.identity_id)
                || !descriptor.reference.did.starts_with(&did_prefix)
            {
                continue;
            }
            let session = custody
                .open_identity(&descriptor.reference)
                .await
                .map_err(crate::internal::identity_provider::map_provider_error)?;
            let public = session
                .public_identity()
                .await
                .map_err(crate::internal::identity_provider::map_provider_error)?;
            let handle_matches = public
                .document
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
            if handle_matches
                && session
                    .resume_document_change()
                    .await
                    .map_err(crate::internal::identity_provider::map_provider_error)?
                    .is_none()
            {
                matches.push(session);
            }
        }
        if matches.len() > 1 {
            return Err(crate::ImError::PermissionDenied);
        }
        let session = match matches.pop() {
            Some(session) => session,
            None => {
                let create =
                    crate::internal::identity_generation::vnext_handle_anp_identity_create_spec(
                        domain,
                        local_part,
                        core.inner().sdk_config().anp_service_endpoint.as_ref(),
                        core.inner().sdk_config().anp_service_did.as_ref(),
                    )?;
                custody
                    .create_identity(create.spec)
                    .await
                    .map_err(crate::internal::identity_provider::map_provider_error)?
            }
        };
        let public = session
            .public_identity()
            .await
            .map_err(crate::internal::identity_provider::map_provider_error)?;
        return pending_registration_from_provider(public);
    }

    #[cfg(feature = "identity-native-anp")]
    {
        let core = core.clone();
        let domain = domain.to_owned();
        let local_part = local_part.to_owned();
        return crate::internal::runtime::worker::run_blocking(move || {
            provision_registration_identity(&core, &domain, &local_part)
        })
        .await
        .map_err(|error| crate::ImError::Internal {
            message: error.to_string(),
        })?;
    }

    #[cfg(not(feature = "identity-native-anp"))]
    Err(crate::ImError::IdentityNotReady {
        identity: format!("did:wba:{domain}:user:{local_part}"),
        missing: vec!["external_identity_provider".to_owned()],
    })
}

#[cfg(feature = "provider-traits")]
fn pending_registration_from_provider(
    public: crate::internal::identity_provider::ProviderPublicIdentity,
) -> crate::ImResult<crate::internal::identity_registration_pending::PendingRegistrationIdentity> {
    if public.state != crate::internal::identity_provider::ProviderIdentityState::Active {
        return Err(crate::ImError::PermissionDenied);
    }
    let manifest = anp::authentication::validate_device_manifest(&public.document)
        .map_err(|_| crate::ImError::PermissionDenied)?
        .ok_or(crate::ImError::PermissionDenied)?;
    if manifest.devices.len() != 1 {
        return Err(crate::ImError::PermissionDenied);
    }
    let device = &manifest.devices[0];
    let root_key_id = public
        .active_keys
        .iter()
        .find(|key| {
            key.purposes
                .contains(&crate::internal::identity_provider::ProviderKeyPurpose::RootControl)
        })
        .map(|key| key.kid.clone())
        .ok_or(crate::ImError::PermissionDenied)?;
    let identity = crate::internal::identity_registration_pending::PendingRegistrationIdentity {
        controller_store_id: public.reference.store_id,
        controller_identity_id: public.reference.identity_id,
        did: crate::ids::Did::parse(&public.reference.did)?,
        did_document: public.document,
        protocol_device_id: crate::ids::ProtocolDeviceId::parse(&device.device_id)?,
        root_key_id,
        device_signing_key_id: device.signing_key_id.clone(),
        device_e2ee_key_id: device.e2ee_key_id.clone(),
        legacy_daemon_authorization: false,
        controller_revision_id: None,
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
    anp_identity::host::EnrollmentProposal,
)> {
    if resolved_document
        .get("id")
        .and_then(serde_json::Value::as_str)
        != Some(did.as_str())
        || !anp::authentication::validate_did_document_binding(resolved_document, true)
    {
        return Err(crate::ImError::PermissionDenied);
    }
    use anp_identity::host::EnrollmentWorkflow;

    let mut manager = open_controller_manager(core)?;
    let existing = manager
        .list()
        .map_err(map_facade_error)?
        .into_iter()
        .find(|item| item.reference.did == did.as_str());
    let proposal = match existing {
        Some(descriptor) => {
            let identity = manager
                .get(&descriptor.reference)
                .map_err(map_facade_error)?;
            let public = identity.public_identity().map_err(map_facade_error)?;
            if public.state != anp_identity::PublicIdentityState::Enrolling
                || anp_identity::canonical_document_digest(public.document.as_value())
                    .map_err(map_error)?
                    != anp_identity::canonical_document_digest(resolved_document)
                        .map_err(map_error)?
            {
                return Err(crate::ImError::PermissionDenied);
            }
            manager
                .resume_enrollment(&descriptor.reference)
                .map_err(map_facade_error)?
                .ok_or(crate::ImError::PermissionDenied)?
                .proposal()
                .clone()
        }
        None => {
            let device_id = crate::ids::ProtocolDeviceId::generate()?;
            let signing_fragment = format!("{}-sign", device_id.as_str());
            let e2ee_fragment = format!("{}-e2ee", device_id.as_str());
            manager
                .begin_device_enrollment(anp_identity::host::DeviceEnrollmentRequest {
                    remote: anp_identity::VerifiedRemoteDocument {
                        document: anp_identity::DidDocument::from_value(resolved_document.clone()),
                        evidence: anp_identity::VerifiedPublicationEvidence {
                            document_version: 1,
                            registry_version: 1,
                            document_digest:
                                crate::internal::identity_wire::document::document_hash(
                                    resolved_document,
                                )?,
                        },
                    },
                    device_id: device_id.as_str().to_owned(),
                    device_signing_fragment: signing_fragment,
                    device_agreement_fragment: e2ee_fragment,
                    profiles: crate::internal::identity_generation::vnext_device_profiles(),
                    capabilities: anp_identity::host::EnrollmentCapabilities { did_wba: true },
                })
                .map_err(map_facade_error)?
                .proposal()
                .clone()
        }
    };
    if proposal.identity.did != did.as_str() {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok((
        crate::internal::identity_join_activation_pending::JoinEnrollmentRef {
            store_id: proposal.identity.store_id.clone(),
            identity_id: proposal.identity.identity_id.clone(),
            enrollment_id: proposal.enrollment_id.clone(),
        },
        proposal,
    ))
}

pub(crate) fn provision_handle_recovery_identity(
    core: &crate::core::ImCore,
    domain: &str,
    local_part: &str,
) -> crate::ImResult<crate::internal::identity_handle_recovery_pending::HandleRecoveryIdentityRef> {
    let mut manager = open_controller_manager(core)?;
    let mut matches = find_unprojected_handle_identities(core, &manager, domain, local_part)?;
    let mut identity = if let Some(identity) = matches.pop() {
        identity
    } else {
        let create = crate::internal::identity_generation::vnext_handle_anp_identity_create_spec(
            domain,
            local_part,
            core.inner().sdk_config().anp_service_endpoint.as_ref(),
            core.inner().sdk_config().anp_service_did.as_ref(),
        )?;
        manager
            .create(native_create_spec(create.spec))
            .map_err(map_facade_error)?
    };
    let public = identity.public_identity().map_err(map_facade_error)?;
    if public.state != anp_identity::PublicIdentityState::Active
        || identity
            .resume_document_change()
            .map_err(map_facade_error)?
            .is_some()
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let manifest = anp::authentication::validate_device_manifest(public.document.as_value())
        .map_err(|_| crate::ImError::PermissionDenied)?
        .ok_or(crate::ImError::PermissionDenied)?;
    if manifest.devices.len() != 1 {
        return Err(crate::ImError::PermissionDenied);
    }
    let device = &manifest.devices[0];
    let root_key_id = public
        .active_keys
        .iter()
        .find(|key| {
            key.purposes
                .contains(&anp_identity::KeyPurpose::RootControl)
        })
        .map(|key| key.kid.clone())
        .ok_or(crate::ImError::PermissionDenied)?;
    let reference = crate::internal::identity_handle_recovery_pending::HandleRecoveryIdentityRef {
        store_id: public.reference.store_id.clone(),
        identity_id: public.reference.identity_id.clone(),
        did: crate::ids::Did::parse(&public.reference.did)?,
        did_document: public.document.into_value(),
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
) -> crate::ImResult<anp_identity::ManagedIdentity> {
    let identity = open_managed_identity(
        core,
        &expected.store_id,
        &expected.identity_id,
        expected.did.as_str(),
    )?;
    let public = identity.public_identity().map_err(map_facade_error)?;
    if public.state != anp_identity::PublicIdentityState::Active
        || anp_identity::canonical_document_digest(public.document.as_value()).map_err(map_error)?
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
    let mut manager = open_controller_manager(core)?;
    let reference = anp_identity::IdentityRef {
        store_id: expected.store_id.clone(),
        identity_id: expected.identity_id.clone(),
        did: expected.did.as_str().to_owned(),
    };
    match manager.get(&reference) {
        Ok(mut identity) => {
            if identity.public_identity().map_err(map_facade_error)?.state
                != anp_identity::PublicIdentityState::Active
                || identity
                    .resume_document_change()
                    .map_err(map_facade_error)?
                    .is_some()
            {
                return Err(crate::ImError::PermissionDenied);
            }
            manager
                .delete(&reference, anp_identity::DeleteIdentityRequest::default())
                .map_err(map_facade_error)
        }
        Err(anp_identity::IdentityError::IdentityNotFound) => Ok(()),
        Err(error) => Err(map_facade_error(error)),
    }
}

fn find_unprojected_handle_identities(
    core: &crate::core::ImCore,
    manager: &anp_identity::IdentityManager,
    domain: &str,
    local_part: &str,
) -> crate::ImResult<Vec<anp_identity::ManagedIdentity>> {
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
    for descriptor in manager.list().map_err(map_facade_error)? {
        if descriptor.state != anp_identity::PublicIdentityState::Active
            || projected.contains(descriptor.reference.identity_id.as_str())
            || !descriptor.reference.did.starts_with(&did_prefix)
        {
            continue;
        }
        let mut identity = manager
            .get(&descriptor.reference)
            .map_err(map_facade_error)?;
        let public = identity.public_identity().map_err(map_facade_error)?;
        let handle_matches = public
            .document
            .as_value()
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
        let has_daemon = public.document.as_value()["authentication"]
            .as_array()
            .is_some_and(|entries| {
                entries.iter().any(|entry| {
                    entry
                        .as_str()
                        .is_some_and(|kid| kid.ends_with("#daemon-key-1"))
                })
            });
        if handle_matches
            && !has_daemon
            && identity
                .resume_document_change()
                .map_err(map_facade_error)?
                .is_none()
        {
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
    if custody.enrollment_id == LEGACY_IMPORTED_ACTIVE_ENROLLMENT_ID {
        return open_managed_identity(core, &custody.store_id, &custody.identity_id, did.as_str())?
            .sign(anp_identity::SignRequest {
                purpose: anp_identity::SigningPurpose::DeviceAssertion,
                key: anp_identity::KeySelector::Kid(kid.to_owned()),
                payload: message.to_vec(),
            })
            .map(|signature| signature.bytes)
            .map_err(map_facade_error);
    }
    let session = pending_join_enrollment_session(core, did, custody)?;
    let anp_identity::host::EnrollmentProposalKind::Device { signing_key, .. } =
        &session.proposal().kind
    else {
        return Err(crate::ImError::PermissionDenied);
    };
    if signing_key.kid != kid {
        return Err(crate::ImError::PermissionDenied);
    }
    session
        .sign_device_assertion(message)
        .map_err(map_facade_error)
}

pub(crate) fn ecdh_join_enrollment(
    core: &crate::core::ImCore,
    did: &crate::ids::Did,
    custody: &crate::internal::identity_join_activation_pending::JoinEnrollmentRef,
    kid: &str,
    peer_public: &[u8],
) -> crate::ImResult<zeroize::Zeroizing<[u8; 32]>> {
    let peer_public: [u8; 32] = peer_public
        .try_into()
        .map_err(|_| crate::ImError::PermissionDenied)?;
    let shared = if custody.enrollment_id == LEGACY_IMPORTED_ACTIVE_ENROLLMENT_ID {
        use anp_identity::host::KeyAgreementPort;
        open_managed_identity(core, &custody.store_id, &custody.identity_id, did.as_str())?
            .derive_shared_secret(anp_identity::host::KeyAgreementRequest {
                key: anp_identity::KeySelector::Kid(kid.to_owned()),
                peer_public,
            })
            .map_err(map_facade_error)?
    } else {
        let session = pending_join_enrollment_session(core, did, custody)?;
        let anp_identity::host::EnrollmentProposalKind::Device { agreement_key, .. } =
            &session.proposal().kind
        else {
            return Err(crate::ImError::PermissionDenied);
        };
        if agreement_key.kid != kid {
            return Err(crate::ImError::PermissionDenied);
        }
        session
            .derive_device_shared_secret(peer_public)
            .map_err(map_facade_error)?
    };
    Ok(zeroize::Zeroizing::new(*shared.as_bytes()))
}

pub(crate) fn adopt_join_identity(
    core: &crate::core::ImCore,
    did: &crate::ids::Did,
    custody: &crate::internal::identity_join_activation_pending::JoinEnrollmentRef,
    document: &serde_json::Value,
    checkpoint: &crate::internal::identity_device_state::IdentityInternalCheckpoint,
) -> crate::ImResult<()> {
    if custody.enrollment_id == LEGACY_IMPORTED_ACTIVE_ENROLLMENT_ID {
        let identity =
            open_managed_identity(core, &custody.store_id, &custody.identity_id, did.as_str())?;
        let public = identity.public_identity().map_err(map_facade_error)?;
        if public.state != anp_identity::PublicIdentityState::Active
            || anp_identity::canonical_document_digest(public.document.as_value())
                .map_err(map_error)?
                != anp_identity::canonical_document_digest(document).map_err(map_error)?
        {
            return Err(crate::ImError::PermissionDenied);
        }
        return Ok(());
    }
    let mut session = pending_join_enrollment_session(core, did, custody)?;
    let outcome = session
        .activate(anp_identity::VerifiedRemoteDocument {
            document: anp_identity::DidDocument::from_value(document.clone()),
            evidence: anp_identity::VerifiedPublicationEvidence {
                document_version: checkpoint.document_version,
                registry_version: checkpoint.registry_version,
                document_digest: checkpoint.document_hash.clone(),
            },
        })
        .map_err(map_facade_error)?;
    if outcome != anp_identity::host::ConvergenceOutcome::Activated {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

pub(crate) fn adopt_controller_document(
    core: &crate::core::ImCore,
    did: &crate::ids::Did,
    store_id: &str,
    identity_id: &str,
    document: &serde_json::Value,
    checkpoint: &crate::internal::identity_device_state::IdentityInternalCheckpoint,
) -> crate::ImResult<()> {
    if crate::internal::identity_wire::document::document_hash(document)?
        != checkpoint.document_hash
    {
        return Err(crate::ImError::PermissionDenied);
    }
    use anp_identity::host::ConvergenceWorkflow;
    let mut identity = open_managed_identity(core, store_id, identity_id, did.as_str())?;
    if identity.public_identity().map_err(map_facade_error)?.state
        != anp_identity::PublicIdentityState::Active
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let outcome = identity
        .adopt_verified_document(anp_identity::VerifiedRemoteDocument {
            document: anp_identity::DidDocument::from_value(document.clone()),
            evidence: anp_identity::VerifiedPublicationEvidence {
                document_version: checkpoint.document_version,
                registry_version: checkpoint.registry_version,
                document_digest: checkpoint.document_hash.clone(),
            },
        })
        .map_err(map_facade_error)?;
    if !matches!(
        outcome,
        anp_identity::host::ConvergenceOutcome::Updated
            | anp_identity::host::ConvergenceOutcome::Unchanged
    ) || identity.public_identity().map_err(map_facade_error)?.state
        != anp_identity::PublicIdentityState::Active
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
) -> crate::ImResult<anp_identity::ManagedIdentity> {
    active_join_managed_identity(core, did, custody, signing_kid, e2ee_kid)
}

pub(crate) fn active_join_managed_identity(
    core: &crate::core::ImCore,
    did: &crate::ids::Did,
    custody: &crate::internal::identity_join_activation_pending::JoinEnrollmentRef,
    signing_kid: &str,
    e2ee_kid: &str,
) -> crate::ImResult<anp_identity::ManagedIdentity> {
    let identity =
        open_managed_identity(core, &custody.store_id, &custody.identity_id, did.as_str())?;
    let public = identity.public_identity().map_err(map_facade_error)?;
    if public.state != anp_identity::PublicIdentityState::Active
        || !public.active_keys.iter().any(|key| {
            key.kid == signing_kid
                && key
                    .purposes
                    .contains(&anp_identity::KeyPurpose::DeviceAssertion)
        })
        || !public.active_keys.iter().any(|key| {
            key.kid == e2ee_kid
                && key
                    .purposes
                    .contains(&anp_identity::KeyPurpose::KeyAgreement)
        })
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(identity)
}

pub(crate) fn pending_join_identity(
    core: &crate::core::ImCore,
    did: &crate::ids::Did,
    custody: &crate::internal::identity_join_activation_pending::JoinEnrollmentRef,
) -> crate::ImResult<anp_identity::ManagedIdentity> {
    let identity =
        open_managed_identity(core, &custody.store_id, &custody.identity_id, did.as_str())?;
    if custody.enrollment_id == LEGACY_IMPORTED_ACTIVE_ENROLLMENT_ID {
        return (identity.public_identity().map_err(map_facade_error)?.state
            == anp_identity::PublicIdentityState::Active)
            .then_some(identity)
            .ok_or(crate::ImError::PermissionDenied);
    }
    if identity.public_identity().map_err(map_facade_error)?.state
        != anp_identity::PublicIdentityState::Enrolling
        || pending_join_enrollment_session(core, did, custody)?
            .proposal()
            .enrollment_id
            != custody.enrollment_id
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(identity)
}

pub(crate) fn pending_join_enrollment_session(
    core: &crate::core::ImCore,
    did: &crate::ids::Did,
    custody: &crate::internal::identity_join_activation_pending::JoinEnrollmentRef,
) -> crate::ImResult<anp_identity::host::EnrollmentSession> {
    if custody.enrollment_id == LEGACY_IMPORTED_ACTIVE_ENROLLMENT_ID {
        return Err(crate::ImError::PermissionDenied);
    }
    let manager = open_controller_manager(core)?;
    let reference = anp_identity::IdentityRef {
        store_id: custody.store_id.clone(),
        identity_id: custody.identity_id.clone(),
        did: did.as_str().to_owned(),
    };
    let session = anp_identity::host::EnrollmentWorkflow::resume_enrollment(&manager, &reference)
        .map_err(map_facade_error)?
        .ok_or(crate::ImError::PermissionDenied)?;
    if session.proposal().enrollment_id != custody.enrollment_id {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(session)
}

pub(crate) fn imported_active_join_managed_identity(
    core: &crate::core::ImCore,
    did: &crate::ids::Did,
    custody: &crate::internal::identity_join_activation_pending::JoinEnrollmentRef,
) -> crate::ImResult<anp_identity::ManagedIdentity> {
    if custody.enrollment_id != LEGACY_IMPORTED_ACTIVE_ENROLLMENT_ID {
        return Err(crate::ImError::PermissionDenied);
    }
    let identity =
        open_managed_identity(core, &custody.store_id, &custody.identity_id, did.as_str())?;
    if identity.public_identity().map_err(map_facade_error)?.state
        != anp_identity::PublicIdentityState::Active
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(identity)
}

pub(crate) fn discard_join_enrollment(
    core: &crate::core::ImCore,
    did: &crate::ids::Did,
    custody: &crate::internal::identity_join_activation_pending::JoinEnrollmentRef,
) -> crate::ImResult<()> {
    use anp_identity::host::EnrollmentWorkflow;
    let mut manager = open_controller_manager(core)?;
    let reference = anp_identity::IdentityRef {
        store_id: custody.store_id.clone(),
        identity_id: custody.identity_id.clone(),
        did: did.as_str().to_owned(),
    };
    match manager.resume_enrollment(&reference) {
        Ok(Some(session)) if session.proposal().enrollment_id == custody.enrollment_id => {
            session.cancel(&mut manager).map_err(map_facade_error)
        }
        Ok(Some(_)) => Err(crate::ImError::PermissionDenied),
        Ok(None) | Err(anp_identity::IdentityError::IdentityNotFound) => Ok(()),
        Err(error) => Err(map_facade_error(error)),
    }
}

pub(crate) fn promote_legacy_upgrade_root(
    core: &crate::core::ImCore,
    identity: &crate::internal::identity_legacy_upgrade_pending::LegacyUpgradeIdentityRef,
    checkpoint: &crate::internal::identity_device_state::IdentityInternalCheckpoint,
    transfer_id: &str,
    accepted_at: &str,
    root_key: zeroize::Zeroizing<Vec<u8>>,
) -> crate::ImResult<()> {
    use anp_identity::host::{IdentityStatusPort, RootImportPort, RootPromotionPort};
    let mut managed = open_managed_identity(
        core,
        &identity.custody.store_id,
        &identity.custody.identity_id,
        identity.did.as_str(),
    )?;
    let evidence = anp_identity::host::LegacyRootImportEvidence {
        transfer_id: transfer_id.to_owned(),
        source_did: identity.did.as_str().to_owned(),
        target_did: identity.did.as_str().to_owned(),
        sender_device_id: "legacy-upgrade".to_owned(),
        recipient_device_id: identity.protocol_device_id.as_str().to_owned(),
        recipient_agreement_kid: identity.e2ee_key_id.clone(),
        root_kid: format!("{}#key-1", identity.did.as_str()),
        checkpoint: anp_identity::host::HostDocumentCheckpoint {
            document_version: checkpoint.document_version,
            registry_version: checkpoint.registry_version,
            document_digest: checkpoint.document_hash.clone(),
        },
        accepted_at: accepted_at.to_owned(),
    };
    let outcome = managed
        .import_legacy_root(anp_identity::host::LegacyRootImportRequest {
            evidence,
            encoding: anp_identity::host::RootPrivateKeyEncoding::Pkcs8Der,
            root_key,
        })
        .map_err(map_facade_error)?;
    if !matches!(
        outcome,
        anp_identity::host::LegacyRootImportOutcome::Pending
            | anp_identity::host::LegacyRootImportOutcome::Active
    ) {
        return Err(crate::ImError::PermissionDenied);
    }
    managed
        .confirm_root_promotion(anp_identity::host::RootPromotionRequest {
            remote: anp_identity::VerifiedRemoteDocument {
                document: anp_identity::DidDocument::from_value(identity.target_document.clone()),
                evidence: anp_identity::VerifiedPublicationEvidence {
                    document_version: checkpoint.document_version,
                    registry_version: checkpoint.registry_version,
                    document_digest: checkpoint.document_hash.clone(),
                },
            },
        })
        .map_err(map_facade_error)?;
    if managed
        .host_status()
        .map_err(map_facade_error)?
        .root_capability
        != anp_identity::host::HostRootCapability::Active
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

pub(crate) fn import_legacy_completion_root(
    core: &crate::core::ImCore,
    reference: &crate::internal::identity_root_import_completion::RootImportCustodyRef,
    evidence: anp_identity::LegacyRootTransferEvidence,
    root_key: zeroize::Zeroizing<Vec<u8>>,
) -> crate::ImResult<()> {
    use anp_identity::host::RootImportPort;
    let mut identity = open_root_import_identity(core, reference)?;
    let outcome = identity
        .import_legacy_root(anp_identity::host::LegacyRootImportRequest {
            evidence: anp_identity::host::LegacyRootImportEvidence {
                transfer_id: evidence.transfer_id,
                source_did: evidence.source_did,
                target_did: evidence.target_did,
                sender_device_id: evidence.sender_device_id,
                recipient_device_id: evidence.recipient_device_id,
                recipient_agreement_kid: evidence.recipient_agreement_kid,
                root_kid: evidence.root_kid,
                checkpoint: anp_identity::host::HostDocumentCheckpoint {
                    document_version: evidence.checkpoint.document_version,
                    registry_version: evidence.checkpoint.registry_version,
                    document_digest: evidence.checkpoint.document_digest,
                },
                accepted_at: evidence.accepted_at,
            },
            encoding: anp_identity::host::RootPrivateKeyEncoding::Pkcs8Der,
            root_key,
        })
        .map_err(map_facade_error)?;
    if !matches!(
        outcome,
        anp_identity::host::LegacyRootImportOutcome::Pending
            | anp_identity::host::LegacyRootImportOutcome::Active
    ) {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

pub(crate) fn sign_pending_completion_root_proof(
    core: &crate::core::ImCore,
    reference: &crate::internal::identity_root_import_completion::RootImportCustodyRef,
    root_key_id: &str,
    statement: &serde_json::Value,
    created: Option<String>,
) -> crate::ImResult<serde_json::Value> {
    use anp_identity::host::RootPromotionPort;
    open_root_import_identity(core, reference)?
        .sign_pending_root_object_proof(anp_identity::host::PendingRootObjectProofRequest {
            key: anp_identity::KeySelector::Kid(root_key_id.to_owned()),
            document: statement.clone(),
            issuer_did: reference.did.clone(),
            created,
        })
        .map_err(map_facade_error)
}

pub(crate) fn confirm_completion_root(
    core: &crate::core::ImCore,
    reference: &crate::internal::identity_root_import_completion::RootImportCustodyRef,
    document: &serde_json::Value,
    checkpoint: &crate::internal::identity_device_state::IdentityInternalCheckpoint,
) -> crate::ImResult<()> {
    use anp_identity::host::{IdentityStatusPort, RootPromotionPort};
    let mut identity = open_root_import_identity(core, reference)?;
    identity
        .confirm_root_promotion(anp_identity::host::RootPromotionRequest {
            remote: anp_identity::VerifiedRemoteDocument {
                document: anp_identity::DidDocument::from_value(document.clone()),
                evidence: anp_identity::VerifiedPublicationEvidence {
                    document_version: checkpoint.document_version,
                    registry_version: checkpoint.registry_version,
                    document_digest: checkpoint.document_hash.clone(),
                },
            },
        })
        .map_err(map_facade_error)?;
    if identity
        .host_status()
        .map_err(map_facade_error)?
        .root_capability
        != anp_identity::host::HostRootCapability::Active
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

fn open_root_import_identity(
    core: &crate::core::ImCore,
    reference: &crate::internal::identity_root_import_completion::RootImportCustodyRef,
) -> crate::ImResult<anp_identity::ManagedIdentity> {
    open_managed_identity(
        core,
        &reference.store_id,
        &reference.identity_id,
        &reference.did,
    )
}

pub(crate) fn begin_registration_publication(
    core: &crate::core::ImCore,
    identity: &crate::internal::identity_registration_pending::PendingRegistrationIdentity,
) -> crate::ImResult<()> {
    let Some(revision_id) = identity.controller_revision_id.as_deref() else {
        return Ok(());
    };
    let mut session =
        registration_change_session(core, identity)?.ok_or(crate::ImError::PermissionDenied)?;
    let candidate = session.candidate().clone();
    if candidate.operation_id != revision_id {
        return Err(crate::ImError::PermissionDenied);
    }
    session
        .begin_publication()
        .map(|_| ())
        .map_err(map_facade_error)
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
    let current = controller.public_identity().map_err(map_facade_error)?;
    let Some(mut session) = controller
        .resume_document_change()
        .map_err(map_facade_error)?
    else {
        return ensure_document_matches(current.document.as_value(), &identity.did_document);
    };
    let candidate = session.candidate().clone();
    if candidate.operation_id != revision_id {
        return Err(crate::ImError::PermissionDenied);
    }
    let observed_document = if remote_committed {
        anp_identity::DidDocument::from_value(identity.did_document.clone())
    } else {
        current.document
    };
    let observation = verified_remote(observed_document)?;
    let outcome = match session.reconcile(observation.clone()) {
        Ok(outcome) => outcome,
        Err(anp_identity::IdentityError::InvalidDocumentChangeState) => {
            let attempt = session.begin_publication().map_err(map_facade_error)?;
            if remote_committed {
                session
                    .complete(
                        attempt,
                        anp_identity::PublicationResult::Confirmed {
                            evidence: anp_identity::VerifiedPublicationEvidence {
                                document_digest: candidate.candidate_digest,
                                ..observation.evidence
                            },
                        },
                    )
                    .map_err(map_facade_error)?
            } else {
                session
                    .complete(attempt, anp_identity::PublicationResult::Unknown)
                    .map_err(map_facade_error)?;
                session.reconcile(observation).map_err(map_facade_error)?
            }
        }
        Err(error) => return Err(map_facade_error(error)),
    };
    match (remote_committed, outcome) {
        (true, anp_identity::DocumentChangeOutcome::Committed { .. })
        | (false, anp_identity::DocumentChangeOutcome::ReadyForPublication) => Ok(()),
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
    let Some(mut session) = registration_change_session(core, identity)? else {
        return ensure_controller_document(core, identity);
    };
    let candidate = session.candidate().clone();
    if candidate.operation_id != revision_id {
        return Err(crate::ImError::PermissionDenied);
    }
    let remote = verified_remote(anp_identity::DidDocument::from_value(
        identity.did_document.clone(),
    ))?;
    let outcome = match session.reconcile(remote.clone()) {
        Ok(outcome) => outcome,
        Err(anp_identity::IdentityError::InvalidDocumentChangeState) => {
            let attempt = session.begin_publication().map_err(map_facade_error)?;
            session
                .complete(
                    attempt,
                    anp_identity::PublicationResult::Confirmed {
                        evidence: anp_identity::VerifiedPublicationEvidence {
                            document_digest: candidate.candidate_digest,
                            ..remote.evidence
                        },
                    },
                )
                .map_err(map_facade_error)?
        }
        Err(error) => return Err(map_facade_error(error)),
    };
    if !matches!(
        outcome,
        anp_identity::DocumentChangeOutcome::Committed { .. }
    ) {
        return Err(crate::ImError::PermissionDenied);
    }
    ensure_controller_document(core, identity)
}

pub(crate) fn refresh_registration_document(
    core: &crate::core::ImCore,
    identity: &crate::internal::identity_registration_pending::PendingRegistrationIdentity,
) -> crate::ImResult<(serde_json::Value, String)> {
    let mut controller = open_registration_controller(core, identity)?;
    if let Some(revision_id) = identity.controller_revision_id.as_deref() {
        let mut session = controller
            .resume_document_change()
            .map_err(map_facade_error)?
            .ok_or(crate::ImError::PermissionDenied)?;
        if session.candidate().operation_id != revision_id {
            return Err(crate::ImError::PermissionDenied);
        }
        let attempt = session.begin_publication().map_err(map_facade_error)?;
        if session
            .complete(
                attempt,
                anp_identity::PublicationResult::RejectedBeforeAcceptance,
            )
            .map_err(map_facade_error)?
            != anp_identity::DocumentChangeOutcome::Aborted
        {
            return Err(crate::ImError::PermissionDenied);
        }
    }
    let public = controller.public_identity().map_err(map_facade_error)?;
    let services = identity_services(public.document.as_value())?;
    let prepared = controller
        .prepare_document_change(anp_identity::DocumentChangeRequest {
            changes: vec![anp_identity::DocumentChange::ReplaceServices { services }],
        })
        .map_err(map_facade_error)?;
    Ok((
        prepared.candidate().candidate_document.clone().into_value(),
        prepared.candidate().operation_id.clone(),
    ))
}

pub(crate) fn discard_unpublished_registration(
    core: &crate::core::ImCore,
    identity: &crate::internal::identity_registration_pending::PendingRegistrationIdentity,
) -> crate::ImResult<()> {
    let mut manager = open_controller_manager(core)?;
    let reference = anp_identity::IdentityRef {
        store_id: identity.controller_store_id.clone(),
        identity_id: identity.controller_identity_id.clone(),
        did: identity.did.as_str().to_owned(),
    };
    let mut controller = match manager.get(&reference) {
        Ok(controller) => controller,
        Err(anp_identity::IdentityError::IdentityNotFound) => return Ok(()),
        Err(error) => return Err(map_facade_error(error)),
    };
    if let Some(mut session) = controller
        .resume_document_change()
        .map_err(map_facade_error)?
    {
        if Some(session.candidate().operation_id.as_str())
            != identity.controller_revision_id.as_deref()
        {
            return Err(crate::ImError::PermissionDenied);
        }
        let attempt = session.begin_publication().map_err(map_facade_error)?;
        if session
            .complete(
                attempt,
                anp_identity::PublicationResult::RejectedBeforeAcceptance,
            )
            .map_err(map_facade_error)?
            != anp_identity::DocumentChangeOutcome::Aborted
        {
            return Err(crate::ImError::PermissionDenied);
        }
    }
    manager
        .delete(&reference, anp_identity::DeleteIdentityRequest::default())
        .map_err(map_facade_error)
}

#[cfg(feature = "provider-traits")]
async fn provider_registration_session(
    core: &crate::core::ImCore,
    identity: &crate::internal::identity_registration_pending::PendingRegistrationIdentity,
) -> crate::ImResult<std::sync::Arc<dyn crate::internal::identity_provider::IdentitySession>> {
    let custody = core.inner().identity_custody_provider().ok_or_else(|| {
        crate::ImError::IdentityNotReady {
            identity: identity.did.as_str().to_owned(),
            missing: vec!["external_identity_provider".to_owned()],
        }
    })?;
    let info = custody
        .store_info()
        .await
        .map_err(crate::internal::identity_provider::map_provider_error)?;
    if info.store_id != identity.controller_store_id {
        return Err(crate::ImError::IdentityBindingConflict {
            detail: "external identity provider Store binding changed".to_owned(),
        });
    }
    custody
        .open_identity(&crate::internal::identity_provider::ProviderIdentityRef {
            store_id: identity.controller_store_id.clone(),
            identity_id: identity.controller_identity_id.clone(),
            did: identity.did.as_str().to_owned(),
        })
        .await
        .map_err(crate::internal::identity_provider::map_provider_error)
}

#[cfg(feature = "provider-traits")]
fn provider_verified_remote(
    document: serde_json::Value,
) -> crate::ImResult<crate::internal::identity_provider::ProviderVerifiedRemoteDocument> {
    let document_digest = crate::internal::identity_wire::document::document_hash(&document)?;
    Ok(
        crate::internal::identity_provider::ProviderVerifiedRemoteDocument {
            document,
            evidence: crate::internal::identity_provider::ProviderPublicationEvidence {
                document_version: 1,
                registry_version: 1,
                document_digest,
            },
        },
    )
}

#[cfg(feature = "provider-traits")]
fn provider_identity_services(
    document: &serde_json::Value,
) -> crate::ImResult<Vec<crate::internal::identity_provider::ProviderIdentityService>> {
    document
        .get("service")
        .and_then(serde_json::Value::as_array)
        .ok_or(crate::ImError::PermissionDenied)?
        .iter()
        .filter(|service| {
            service.get("type").and_then(serde_json::Value::as_str) != Some("AgentDescription")
        })
        .map(|service| {
            let id = service
                .get("id")
                .and_then(serde_json::Value::as_str)
                .map(|id| id.rsplit_once('#').map_or(id, |(_, fragment)| fragment))
                .ok_or(crate::ImError::PermissionDenied)?;
            let service_type = service
                .get("type")
                .and_then(serde_json::Value::as_str)
                .ok_or(crate::ImError::PermissionDenied)?;
            let service_endpoint = service
                .get("serviceEndpoint")
                .and_then(serde_json::Value::as_str)
                .ok_or(crate::ImError::PermissionDenied)?;
            let strings = |field: &str| {
                service
                    .get(field)
                    .and_then(serde_json::Value::as_array)
                    .into_iter()
                    .flatten()
                    .map(|value| {
                        value
                            .as_str()
                            .map(ToOwned::to_owned)
                            .ok_or(crate::ImError::PermissionDenied)
                    })
                    .collect::<crate::ImResult<Vec<_>>>()
            };
            Ok(
                crate::internal::identity_provider::ProviderIdentityService {
                    id: id.to_owned(),
                    service_type: service_type.to_owned(),
                    service_endpoint: service_endpoint.to_owned(),
                    service_did: service
                        .get("serviceDid")
                        .and_then(serde_json::Value::as_str)
                        .map(ToOwned::to_owned),
                    profiles: strings("profiles")?,
                    security_profiles: strings("securityProfiles")?,
                },
            )
        })
        .collect()
}

pub(crate) async fn begin_registration_publication_async(
    core: &crate::core::ImCore,
    identity: &crate::internal::identity_registration_pending::PendingRegistrationIdentity,
) -> crate::ImResult<()> {
    #[cfg(feature = "provider-traits")]
    if core.inner().identity_custody_provider().is_some() {
        let Some(revision_id) = identity.controller_revision_id.as_deref() else {
            return Ok(());
        };
        let session = provider_registration_session(core, identity).await?;
        let change = session
            .resume_document_change()
            .await
            .map_err(crate::internal::identity_provider::map_provider_error)?
            .ok_or(crate::ImError::PermissionDenied)?;
        let candidate = change
            .candidate()
            .await
            .map_err(crate::internal::identity_provider::map_provider_error)?;
        if candidate.operation_id != revision_id {
            return Err(crate::ImError::PermissionDenied);
        }
        change
            .begin_publication()
            .await
            .map(|_| ())
            .map_err(crate::internal::identity_provider::map_provider_error)
    } else {
        run_native_registration_operation(core, identity, begin_registration_publication).await
    }
    #[cfg(not(feature = "provider-traits"))]
    run_native_registration_operation(core, identity, begin_registration_publication).await
}

pub(crate) async fn reconcile_registration_publication_async(
    core: &crate::core::ImCore,
    identity: &crate::internal::identity_registration_pending::PendingRegistrationIdentity,
    remote_committed: bool,
) -> crate::ImResult<()> {
    #[cfg(feature = "provider-traits")]
    if core.inner().identity_custody_provider().is_some() {
        let Some(revision_id) = identity.controller_revision_id.as_deref() else {
            let public = provider_registration_session(core, identity)
                .await?
                .public_identity()
                .await
                .map_err(crate::internal::identity_provider::map_provider_error)?;
            return ensure_document_matches(&public.document, &identity.did_document);
        };
        let session = provider_registration_session(core, identity).await?;
        let current = session
            .public_identity()
            .await
            .map_err(crate::internal::identity_provider::map_provider_error)?;
        let Some(change) = session
            .resume_document_change()
            .await
            .map_err(crate::internal::identity_provider::map_provider_error)?
        else {
            return ensure_document_matches(&current.document, &identity.did_document);
        };
        let candidate = change
            .candidate()
            .await
            .map_err(crate::internal::identity_provider::map_provider_error)?;
        if candidate.operation_id != revision_id {
            return Err(crate::ImError::PermissionDenied);
        }
        let observation = provider_verified_remote(if remote_committed {
            identity.did_document.clone()
        } else {
            current.document
        })?;
        let outcome = match change.reconcile(observation.clone()).await {
            Ok(outcome) => outcome,
            Err(error)
                if error.code
                    == crate::internal::identity_provider::IdentityProviderErrorCode::InvalidDocumentChangeState =>
            {
                let attempt = change
                    .begin_publication()
                    .await
                    .map_err(crate::internal::identity_provider::map_provider_error)?;
                if remote_committed {
                    change
                        .complete(
                            attempt,
                            crate::internal::identity_provider::ProviderPublicationResult::Confirmed {
                                evidence: crate::internal::identity_provider::ProviderPublicationEvidence {
                                    document_digest: candidate.candidate_digest,
                                    ..observation.evidence
                                },
                            },
                        )
                        .await
                        .map_err(crate::internal::identity_provider::map_provider_error)?
                } else {
                    change
                        .complete(
                            attempt,
                            crate::internal::identity_provider::ProviderPublicationResult::Unknown,
                        )
                        .await
                        .map_err(crate::internal::identity_provider::map_provider_error)?;
                    change
                        .reconcile(observation)
                        .await
                        .map_err(crate::internal::identity_provider::map_provider_error)?
                }
            }
            Err(error) => {
                return Err(crate::internal::identity_provider::map_provider_error(error));
            }
        };
        return match (remote_committed, outcome) {
            (
                true,
                crate::internal::identity_provider::ProviderDocumentChangeOutcome::Committed {
                    ..
                },
            )
            | (
                false,
                crate::internal::identity_provider::ProviderDocumentChangeOutcome::ReadyForPublication,
            ) => Ok(()),
            _ => Err(crate::ImError::PermissionDenied),
        };
    }
    run_native_registration_reconcile(core, identity, remote_committed).await
}

pub(crate) async fn commit_registration_publication_async(
    core: &crate::core::ImCore,
    identity: &crate::internal::identity_registration_pending::PendingRegistrationIdentity,
) -> crate::ImResult<()> {
    #[cfg(feature = "provider-traits")]
    if core.inner().identity_custody_provider().is_some() {
        let session = provider_registration_session(core, identity).await?;
        let Some(revision_id) = identity.controller_revision_id.as_deref() else {
            let public = session
                .public_identity()
                .await
                .map_err(crate::internal::identity_provider::map_provider_error)?;
            return ensure_document_matches(&public.document, &identity.did_document);
        };
        let Some(change) = session
            .resume_document_change()
            .await
            .map_err(crate::internal::identity_provider::map_provider_error)?
        else {
            let public = session
                .public_identity()
                .await
                .map_err(crate::internal::identity_provider::map_provider_error)?;
            return ensure_document_matches(&public.document, &identity.did_document);
        };
        let candidate = change
            .candidate()
            .await
            .map_err(crate::internal::identity_provider::map_provider_error)?;
        if candidate.operation_id != revision_id {
            return Err(crate::ImError::PermissionDenied);
        }
        let remote = provider_verified_remote(identity.did_document.clone())?;
        let outcome = match change.reconcile(remote.clone()).await {
            Ok(outcome) => outcome,
            Err(error)
                if error.code
                    == crate::internal::identity_provider::IdentityProviderErrorCode::InvalidDocumentChangeState =>
            {
                let attempt = change
                    .begin_publication()
                    .await
                    .map_err(crate::internal::identity_provider::map_provider_error)?;
                change
                    .complete(
                        attempt,
                        crate::internal::identity_provider::ProviderPublicationResult::Confirmed {
                            evidence: crate::internal::identity_provider::ProviderPublicationEvidence {
                                document_digest: candidate.candidate_digest,
                                ..remote.evidence
                            },
                        },
                    )
                    .await
                    .map_err(crate::internal::identity_provider::map_provider_error)?
            }
            Err(error) => {
                return Err(crate::internal::identity_provider::map_provider_error(error));
            }
        };
        if !matches!(
            outcome,
            crate::internal::identity_provider::ProviderDocumentChangeOutcome::Committed { .. }
        ) {
            return Err(crate::ImError::PermissionDenied);
        }
        let public = session
            .public_identity()
            .await
            .map_err(crate::internal::identity_provider::map_provider_error)?;
        return ensure_document_matches(&public.document, &identity.did_document);
    }
    run_native_registration_operation(core, identity, commit_registration_publication).await
}

pub(crate) async fn refresh_registration_document_async(
    core: &crate::core::ImCore,
    identity: &crate::internal::identity_registration_pending::PendingRegistrationIdentity,
) -> crate::ImResult<(serde_json::Value, String)> {
    #[cfg(feature = "provider-traits")]
    if core.inner().identity_custody_provider().is_some() {
        let session = provider_registration_session(core, identity).await?;
        if let Some(revision_id) = identity.controller_revision_id.as_deref() {
            let change = session
                .resume_document_change()
                .await
                .map_err(crate::internal::identity_provider::map_provider_error)?
                .ok_or(crate::ImError::PermissionDenied)?;
            if change
                .candidate()
                .await
                .map_err(crate::internal::identity_provider::map_provider_error)?
                .operation_id
                != revision_id
            {
                return Err(crate::ImError::PermissionDenied);
            }
            let attempt = change
                .begin_publication()
                .await
                .map_err(crate::internal::identity_provider::map_provider_error)?;
            if change
                .complete(
                    attempt,
                    crate::internal::identity_provider::ProviderPublicationResult::RejectedBeforeAcceptance,
                )
                .await
                .map_err(crate::internal::identity_provider::map_provider_error)?
                != crate::internal::identity_provider::ProviderDocumentChangeOutcome::Aborted
            {
                return Err(crate::ImError::PermissionDenied);
            }
        }
        let public = session
            .public_identity()
            .await
            .map_err(crate::internal::identity_provider::map_provider_error)?;
        let services = provider_identity_services(&public.document)?;
        let change = session
            .prepare_document_change(serde_json::json!({
                "changes": [{"change": "replace_services", "services": services}]
            }))
            .await
            .map_err(crate::internal::identity_provider::map_provider_error)?;
        let candidate = change
            .candidate()
            .await
            .map_err(crate::internal::identity_provider::map_provider_error)?;
        return Ok((candidate.candidate_document, candidate.operation_id));
    }
    run_native_registration_refresh(core, identity).await
}

pub(crate) async fn discard_unpublished_registration_async(
    core: &crate::core::ImCore,
    identity: &crate::internal::identity_registration_pending::PendingRegistrationIdentity,
) -> crate::ImResult<()> {
    #[cfg(feature = "provider-traits")]
    if let Some(custody) = core.inner().identity_custody_provider() {
        let reference = crate::internal::identity_provider::ProviderIdentityRef {
            store_id: identity.controller_store_id.clone(),
            identity_id: identity.controller_identity_id.clone(),
            did: identity.did.as_str().to_owned(),
        };
        let session = match custody.open_identity(&reference).await {
            Ok(session) => session,
            Err(error)
                if error.code
                    == crate::internal::identity_provider::IdentityProviderErrorCode::IdentityNotFound =>
            {
                return Ok(());
            }
            Err(error) => {
                return Err(crate::internal::identity_provider::map_provider_error(error));
            }
        };
        if let Some(change) = session
            .resume_document_change()
            .await
            .map_err(crate::internal::identity_provider::map_provider_error)?
        {
            if Some(
                change
                    .candidate()
                    .await
                    .map_err(crate::internal::identity_provider::map_provider_error)?
                    .operation_id
                    .as_str(),
            ) != identity.controller_revision_id.as_deref()
            {
                return Err(crate::ImError::PermissionDenied);
            }
            let attempt = change
                .begin_publication()
                .await
                .map_err(crate::internal::identity_provider::map_provider_error)?;
            if change
                .complete(
                    attempt,
                    crate::internal::identity_provider::ProviderPublicationResult::RejectedBeforeAcceptance,
                )
                .await
                .map_err(crate::internal::identity_provider::map_provider_error)?
                != crate::internal::identity_provider::ProviderDocumentChangeOutcome::Aborted
            {
                return Err(crate::ImError::PermissionDenied);
            }
        }
        return custody
            .delete_identity(&reference)
            .await
            .map_err(crate::internal::identity_provider::map_provider_error);
    }
    run_native_registration_operation(core, identity, discard_unpublished_registration).await
}

async fn run_native_registration_operation(
    core: &crate::core::ImCore,
    identity: &crate::internal::identity_registration_pending::PendingRegistrationIdentity,
    operation: fn(
        &crate::core::ImCore,
        &crate::internal::identity_registration_pending::PendingRegistrationIdentity,
    ) -> crate::ImResult<()>,
) -> crate::ImResult<()> {
    #[cfg(feature = "identity-native-anp")]
    {
        let core = core.clone();
        let identity = identity.clone();
        return crate::internal::runtime::worker::run_blocking(move || operation(&core, &identity))
            .await
            .map_err(|error| crate::ImError::Internal {
                message: error.to_string(),
            })?;
    }
    #[cfg(not(feature = "identity-native-anp"))]
    {
        let _ = (core, identity, operation);
        Err(crate::ImError::IdentityNotReady {
            identity: identity.did.as_str().to_owned(),
            missing: vec!["external_identity_provider".to_owned()],
        })
    }
}

async fn run_native_registration_reconcile(
    core: &crate::core::ImCore,
    identity: &crate::internal::identity_registration_pending::PendingRegistrationIdentity,
    remote_committed: bool,
) -> crate::ImResult<()> {
    #[cfg(feature = "identity-native-anp")]
    {
        let core = core.clone();
        let identity = identity.clone();
        return crate::internal::runtime::worker::run_blocking(move || {
            reconcile_registration_publication(&core, &identity, remote_committed)
        })
        .await
        .map_err(|error| crate::ImError::Internal {
            message: error.to_string(),
        })?;
    }
    #[cfg(not(feature = "identity-native-anp"))]
    {
        let _ = (core, identity, remote_committed);
        Err(crate::ImError::IdentityNotReady {
            identity: identity.did.as_str().to_owned(),
            missing: vec!["external_identity_provider".to_owned()],
        })
    }
}

async fn run_native_registration_refresh(
    core: &crate::core::ImCore,
    identity: &crate::internal::identity_registration_pending::PendingRegistrationIdentity,
) -> crate::ImResult<(serde_json::Value, String)> {
    #[cfg(feature = "identity-native-anp")]
    {
        let core = core.clone();
        let identity = identity.clone();
        return crate::internal::runtime::worker::run_blocking(move || {
            refresh_registration_document(&core, &identity)
        })
        .await
        .map_err(|error| crate::ImError::Internal {
            message: error.to_string(),
        })?;
    }
    #[cfg(not(feature = "identity-native-anp"))]
    {
        let _ = (core, identity);
        Err(crate::ImError::IdentityNotReady {
            identity: identity.did.as_str().to_owned(),
            missing: vec!["external_identity_provider".to_owned()],
        })
    }
}

pub(crate) fn registration_controller_signing_managed_identity(
    core: &crate::core::ImCore,
    identity: &crate::internal::identity_registration_pending::PendingRegistrationIdentity,
) -> crate::ImResult<anp_identity::ManagedIdentity> {
    let controller = open_managed_identity(
        core,
        &identity.controller_store_id,
        &identity.controller_identity_id,
        identity.did.as_str(),
    )?;
    let public = controller.public_identity().map_err(map_facade_error)?;
    for (kid, purpose) in [
        (&identity.root_key_id, anp_identity::KeyPurpose::RootControl),
        (
            &identity.device_signing_key_id,
            anp_identity::KeyPurpose::DeviceAssertion,
        ),
        (
            &identity.device_e2ee_key_id,
            anp_identity::KeyPurpose::KeyAgreement,
        ),
    ] {
        if !public
            .active_keys
            .iter()
            .any(|key| key.kid == *kid && key.purposes.contains(&purpose))
        {
            return Err(crate::ImError::PermissionDenied);
        }
    }
    Ok(controller)
}

fn open_managed_identity(
    core: &crate::core::ImCore,
    store_id: &str,
    identity_id: &str,
    did: &str,
) -> crate::ImResult<anp_identity::ManagedIdentity> {
    let manager = open_controller_manager(core)?;
    manager
        .get(&anp_identity::IdentityRef {
            store_id: store_id.to_owned(),
            identity_id: identity_id.to_owned(),
            did: did.to_owned(),
        })
        .map_err(map_facade_error)
}

fn find_unprojected_registration_identity(
    core: &crate::core::ImCore,
    manager: &anp_identity::IdentityManager,
    domain: &str,
    local_part: &str,
) -> crate::ImResult<Option<anp_identity::ManagedIdentity>> {
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
    for descriptor in manager.list().map_err(map_facade_error)? {
        if descriptor.state != anp_identity::PublicIdentityState::Active
            || projected.contains(descriptor.reference.identity_id.as_str())
            || !descriptor.reference.did.starts_with(&did_prefix)
        {
            continue;
        }
        let mut identity = manager
            .get(&descriptor.reference)
            .map_err(map_facade_error)?;
        let public = identity.public_identity().map_err(map_facade_error)?;
        let handle_matches = public
            .document
            .as_value()
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
        if handle_matches
            && identity
                .resume_document_change()
                .map_err(map_facade_error)?
                .is_none()
        {
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
) -> crate::ImResult<anp_identity::ManagedIdentity> {
    open_managed_identity(
        core,
        &expected.controller_store_id,
        &expected.controller_identity_id,
        expected.did.as_str(),
    )
}

fn registration_change_session(
    core: &crate::core::ImCore,
    expected: &crate::internal::identity_registration_pending::PendingRegistrationIdentity,
) -> crate::ImResult<Option<anp_identity::DocumentChangeSession>> {
    let mut controller = open_registration_controller(core, expected)?;
    controller
        .resume_document_change()
        .map_err(map_facade_error)
}

fn verified_remote(
    document: anp_identity::DidDocument,
) -> crate::ImResult<anp_identity::VerifiedRemoteDocument> {
    let document_digest =
        crate::internal::identity_wire::document::document_hash(document.as_value())?;
    Ok(anp_identity::VerifiedRemoteDocument {
        document,
        evidence: anp_identity::VerifiedPublicationEvidence {
            document_version: 1,
            registry_version: 1,
            document_digest,
        },
    })
}

fn ensure_controller_document(
    core: &crate::core::ImCore,
    identity: &crate::internal::identity_registration_pending::PendingRegistrationIdentity,
) -> crate::ImResult<()> {
    let controller = open_registration_controller(core, identity)?;
    ensure_document_matches(
        controller
            .public_identity()
            .map_err(map_facade_error)?
            .document
            .as_value(),
        &identity.did_document,
    )
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

fn identity_services(
    document: &serde_json::Value,
) -> crate::ImResult<Vec<anp_identity::IdentityService>> {
    document
        .get("service")
        .and_then(serde_json::Value::as_array)
        .ok_or(crate::ImError::PermissionDenied)?
        .iter()
        .filter(|service| {
            service.get("type").and_then(serde_json::Value::as_str) != Some("AgentDescription")
        })
        .map(|service| {
            let id = service
                .get("id")
                .and_then(serde_json::Value::as_str)
                .map(|id| id.rsplit_once('#').map_or(id, |(_, fragment)| fragment))
                .ok_or(crate::ImError::PermissionDenied)?;
            let strings = |field: &str| {
                service
                    .get(field)
                    .and_then(serde_json::Value::as_array)
                    .into_iter()
                    .flatten()
                    .map(|value| {
                        value
                            .as_str()
                            .map(ToOwned::to_owned)
                            .ok_or(crate::ImError::PermissionDenied)
                    })
                    .collect::<crate::ImResult<Vec<_>>>()
            };
            Ok(anp_identity::IdentityService {
                id: id.to_owned(),
                service_type: service
                    .get("type")
                    .and_then(serde_json::Value::as_str)
                    .ok_or(crate::ImError::PermissionDenied)?
                    .to_owned(),
                service_endpoint: service
                    .get("serviceEndpoint")
                    .and_then(serde_json::Value::as_str)
                    .ok_or(crate::ImError::PermissionDenied)?
                    .to_owned(),
                service_did: service
                    .get("serviceDid")
                    .and_then(serde_json::Value::as_str)
                    .map(ToOwned::to_owned),
                profiles: strings("profiles")?,
                security_profiles: strings("securityProfiles")?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct RootSigner {
        document: serde_json::Value,
        kid: String,
        private_pem: String,
    }

    impl crate::internal::key_provider::IdentitySigner for RootSigner {
        fn did_document(&self) -> crate::ImResult<serde_json::Value> {
            Ok(self.document.clone())
        }

        fn optional_did_document(&self) -> crate::ImResult<Option<serde_json::Value>> {
            Ok(Some(self.document.clone()))
        }

        fn request_signing_key_id(&self) -> crate::ImResult<String> {
            Ok(self.kid.clone())
        }

        fn sign(&self, kid: &str, message: &[u8]) -> crate::ImResult<Vec<u8>> {
            self.sign_root(kid, message)
        }

        fn sign_root(&self, kid: &str, message: &[u8]) -> crate::ImResult<Vec<u8>> {
            if kid != self.kid {
                return Err(crate::ImError::PermissionDenied);
            }
            crate::internal::key_provider::sign_private_pem(&self.private_pem, message, "test root")
        }

        fn ecdh(
            &self,
            _kid: &str,
            _peer_public: &[u8],
        ) -> crate::ImResult<zeroize::Zeroizing<[u8; 32]>> {
            Err(crate::ImError::PermissionDenied)
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

    #[test]
    fn registration_provisioning_recovers_one_controller_namespace() {
        let root = tempfile::tempdir().unwrap();
        let core = crate::ImCore::new(test_config(), test_paths(root.path())).unwrap();

        let first = provision_registration_identity(&core, "example.test", "alice").unwrap();
        let recovered = provision_registration_identity(&core, "example.test", "alice").unwrap();

        assert_eq!(recovered, first);
        let controller = open_controller_store(&core).unwrap();
        assert_eq!(controller.list_identities().unwrap().len(), 1);
        assert!(!first.did_document["authentication"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry
                .as_str()
                .is_some_and(|kid| kid.ends_with("#daemon-key-1"))));
        let encoded = serde_json::to_string(&first).unwrap();
        assert!(!encoded.contains("PRIVATE KEY"));
        assert!(!encoded.contains("private_pem"));
    }

    #[cfg(feature = "provider-traits")]
    #[tokio::test]
    async fn external_registration_provider_converges_document_publication() {
        let root = tempfile::tempdir().unwrap();
        let provider_root = tempfile::tempdir().unwrap();
        let manager =
            anp_identity::IdentityManager::initialize(anp_identity::IdentityManagerConfig {
                state_root: provider_root.path().to_path_buf(),
                root_key: anp_identity::RootKeySource::Injected(
                    anp_identity::InjectedStoreKey::new("external-registration", [0x72; 32]),
                ),
            })
            .unwrap();
        let provider = std::sync::Arc::new(
            crate::internal::identity_provider::DirectAnpIdentityCustody::new(manager),
        );
        let core = crate::ImCore::new_with_options(
            test_config(),
            test_paths(root.path()),
            crate::ImCoreOpenOptions::default().with_identity_custody_provider(provider),
        )
        .unwrap();

        let mut identity = provision_registration_identity_async(&core, "example.test", "external")
            .await
            .unwrap();
        let (candidate, revision_id) = refresh_registration_document_async(&core, &identity)
            .await
            .unwrap();
        identity.did_document = candidate;
        identity.controller_revision_id = Some(revision_id);

        begin_registration_publication_async(&core, &identity)
            .await
            .unwrap();
        reconcile_registration_publication_async(&core, &identity, false)
            .await
            .unwrap();
        begin_registration_publication_async(&core, &identity)
            .await
            .unwrap();
        commit_registration_publication_async(&core, &identity)
            .await
            .unwrap();

        let public = provider_registration_session(&core, &identity)
            .await
            .unwrap()
            .public_identity()
            .await
            .unwrap();
        ensure_document_matches(&public.document, &identity.did_document).unwrap();
    }

    #[test]
    fn unpublished_registration_cleanup_removes_controller_namespace_and_is_idempotent() {
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
            .sign(anp_identity::SignRequest {
                purpose: anp_identity::SigningPurpose::DeviceAssertion,
                key: anp_identity::KeySelector::Kid(first.device_signing_key_id.clone()),
                payload: b"recovery proof".to_vec(),
            })
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

    #[test]
    fn legacy_upgrade_activates_device_then_imports_the_existing_root() {
        let root = tempfile::tempdir().unwrap();
        let core = crate::ImCore::new(test_config(), test_paths(root.path())).unwrap();
        let legacy = crate::internal::identity_generation::generate_handle_identity_with_default_daemon_subkey(
            "example.test",
            "legacy",
            None,
            None,
        )
        .unwrap()
        .identity;
        let signer = RootSigner {
            document: legacy.did_document.clone(),
            kid: format!("{}#key-1", legacy.did.as_str()),
            private_pem: legacy.key1_private_pem.clone(),
        };
        let (custody, enrollment) =
            prepare_join_enrollment(&core, &legacy.did, &legacy.did_document).unwrap();
        let identity = crate::internal::identity_legacy_upgrade::build_custodied_legacy_upgrade(
            &legacy.did_document,
            custody,
            &enrollment,
            &signer,
        )
        .unwrap();
        let checkpoint = crate::internal::identity_device_state::IdentityInternalCheckpoint {
            document_version: 2,
            document_hash: crate::internal::identity_wire::document::document_hash(
                &identity.target_document,
            )
            .unwrap(),
            registry_version: 2,
        };
        adopt_join_identity(
            &core,
            &identity.did,
            &identity.custody,
            &identity.target_document,
            &checkpoint,
        )
        .unwrap();
        let root_der = zeroize::Zeroizing::new(
            crate::internal::identity_root_transfer_runtime::canonical_ed25519_pkcs8_der(
                &legacy.key1_private_pem,
            )
            .unwrap(),
        );
        promote_legacy_upgrade_root(
            &core,
            &identity,
            &checkpoint,
            "legacy-upgrade-test",
            "2026-08-21T00:00:00Z",
            root_der,
        )
        .unwrap();

        let managed = open_controller_store(&core)
            .unwrap()
            .open_identity(identity.did.as_str())
            .unwrap();
        assert_eq!(
            managed.root_capability(),
            anp_identity::RootCapabilityState::Active
        );
        managed
            .sign_device_assertion(&identity.signing_key_id, b"device")
            .unwrap();
    }

    #[test]
    fn completion_root_import_keeps_proof_and_promotion_inside_custody() {
        let root = tempfile::tempdir().unwrap();
        let core = crate::ImCore::new(test_config(), test_paths(root.path())).unwrap();
        let legacy = crate::internal::identity_generation::generate_handle_identity_with_default_daemon_subkey(
            "example.test",
            "completion",
            None,
            None,
        )
        .unwrap()
        .identity;
        let signer = RootSigner {
            document: legacy.did_document.clone(),
            kid: format!("{}#key-1", legacy.did.as_str()),
            private_pem: legacy.key1_private_pem.clone(),
        };
        let (custody, enrollment) =
            prepare_join_enrollment(&core, &legacy.did, &legacy.did_document).unwrap();
        let target = crate::internal::identity_legacy_upgrade::build_custodied_legacy_upgrade(
            &legacy.did_document,
            custody.clone(),
            &enrollment,
            &signer,
        )
        .unwrap();
        let checkpoint = crate::internal::identity_device_state::IdentityInternalCheckpoint {
            document_version: 2,
            document_hash: crate::internal::identity_wire::document::document_hash(
                &target.target_document,
            )
            .unwrap(),
            registry_version: 2,
        };
        adopt_join_identity(
            &core,
            &target.did,
            &custody,
            &target.target_document,
            &checkpoint,
        )
        .unwrap();
        let reference = crate::internal::identity_root_import_completion::RootImportCustodyRef {
            store_id: custody.store_id,
            identity_id: custody.identity_id,
            did: target.did.as_str().to_owned(),
        };
        let root_der = zeroize::Zeroizing::new(
            crate::internal::identity_root_transfer_runtime::canonical_ed25519_pkcs8_der(
                &legacy.key1_private_pem,
            )
            .unwrap(),
        );
        import_legacy_completion_root(
            &core,
            &reference,
            anp_identity::LegacyRootTransferEvidence {
                transfer_id: "completion-message".to_owned(),
                source_did: target.did.as_str().to_owned(),
                target_did: target.did.as_str().to_owned(),
                sender_device_id: "sender-device".to_owned(),
                recipient_device_id: target.protocol_device_id.as_str().to_owned(),
                recipient_agreement_kid: target.e2ee_key_id.clone(),
                root_kid: format!("{}#key-1", target.did.as_str()),
                checkpoint: anp_identity::DocumentCheckpoint {
                    document_version: checkpoint.document_version,
                    registry_version: checkpoint.registry_version,
                    document_digest: checkpoint.document_hash.clone(),
                },
                accepted_at: "2026-08-21T00:00:00Z".to_owned(),
            },
            root_der,
        )
        .unwrap();
        let statement = serde_json::json!({"type": "root-possession"});
        let proof = sign_pending_completion_root_proof(
            &core,
            &reference,
            &format!("{}#key-1", target.did.as_str()),
            &statement,
            Some("2026-08-21T00:00:00Z".to_owned()),
        )
        .unwrap();
        anp::proof::verify_object_proof(&proof, target.did.as_str(), &target.target_document)
            .unwrap();
        confirm_completion_root(&core, &reference, &target.target_document, &checkpoint).unwrap();
        assert_eq!(
            open_controller_store(&core)
                .unwrap()
                .open_identity(target.did.as_str())
                .unwrap()
                .root_capability(),
            anp_identity::RootCapabilityState::Active
        );
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

#[cfg(test)]
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

pub(crate) fn map_facade_error(error: anp_identity::IdentityError) -> crate::ImError {
    match error {
        anp_identity::IdentityError::IdentityNotFound => crate::ImError::IdentityNotFound {
            selector: "anp-identity".to_owned(),
        },
        anp_identity::IdentityError::Conflict => crate::ImError::LocalStateUnavailable {
            detail: "anp identity store generation changed; recovery is required".to_owned(),
        },
        anp_identity::IdentityError::RootKeyMismatch
        | anp_identity::IdentityError::ProviderUnavailable => crate::ImError::PermissionDenied,
        error => crate::ImError::LocalStateUnavailable {
            detail: format!("anp identity facade operation failed: {error}"),
        },
    }
}

pub(crate) fn native_create_spec(
    value: crate::internal::identity_provider::ProviderCreateIdentityRequest,
) -> anp_identity::DidCreateSpec {
    use crate::internal::identity_provider::{
        ProviderDidProfile, ProviderIdentityExtension, ProviderManagedKeyRole,
    };
    anp_identity::DidCreateSpec {
        profile: match value.profile {
            ProviderDidProfile::E1 => anp_identity::DidProfile::E1,
        },
        domain: value.domain,
        port: value.port,
        path_segments: value.path_segments,
        capabilities: anp_identity::Capabilities {
            did_wba: value.capabilities.did_wba,
        },
        managed_keys: value
            .managed_keys
            .into_iter()
            .map(|key| anp_identity::ManagedKeySpec {
                fragment: key.fragment,
                role: match key.role {
                    ProviderManagedKeyRole::RootControl => anp_identity::KeyRole::RootControl,
                    ProviderManagedKeyRole::DeviceSigning => anp_identity::KeyRole::DeviceSigning,
                    ProviderManagedKeyRole::RequestSigning => anp_identity::KeyRole::RequestSigning,
                    ProviderManagedKeyRole::E2eeSigning => anp_identity::KeyRole::E2eeSigning,
                    ProviderManagedKeyRole::E2eeAgreement => anp_identity::KeyRole::E2eeAgreement,
                },
            })
            .collect(),
        external_keys: Vec::new(),
        services: value
            .services
            .into_iter()
            .map(|service| anp_identity::ServiceSpec {
                id: service.id,
                service_type: service.service_type,
                service_endpoint: service.service_endpoint,
                service_did: service.service_did,
                profiles: service.profiles,
                security_profiles: service.security_profiles,
            })
            .collect(),
        agent_description_url: value.agent_description_url,
        extensions: value
            .extensions
            .into_iter()
            .map(|extension| match extension {
                ProviderIdentityExtension::DeviceManifest { devices } => {
                    anp_identity::DidExtensionSpec::DeviceManifest(
                        anp_identity::DeviceManifestSpec {
                            devices: devices
                                .into_iter()
                                .map(|device| anp_identity::DeviceManifestEntrySpec {
                                    device_id: device.device_id,
                                    signing_key_id: device.signing_key_id,
                                    e2ee_key_id: device.e2ee_key_id,
                                    profiles: device.profiles,
                                })
                                .collect(),
                        },
                    )
                }
            })
            .collect(),
    }
}
