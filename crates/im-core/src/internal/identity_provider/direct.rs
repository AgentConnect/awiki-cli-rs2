use async_trait::async_trait;
use std::sync::{Arc, Mutex};

use anp_identity::host::{
    ConvergenceWorkflow, HttpRequestSigningPort, IdentityStatusPort, KeyAgreementPort,
    KeyAgreementRequest, RootExportPort, RootPromotionPort, UserConfirmedRootExportRequest,
};

use super::{
    IdentityCustody, IdentityProviderError, IdentityProviderErrorCode, IdentitySession,
    ProviderCreateIdentityRequest, ProviderDeviceEnrollmentRequest, ProviderDocumentChangeOutcome,
    ProviderDocumentChangePhase, ProviderDocumentChangeSession, ProviderDocumentCheckpoint,
    ProviderDocumentProofRequest, ProviderEnrollmentProposal, ProviderEnrollmentProposalKind,
    ProviderEnrollmentPublicKey, ProviderEnrollmentSession, ProviderExactHttpRequest,
    ProviderExportedRoot, ProviderHostStatus, ProviderHttpHeader, ProviderIdentityDescriptor,
    ProviderIdentityMaterialImportRequest, ProviderIdentityMaterialKey, ProviderIdentityRef,
    ProviderIdentityState, ProviderIdentityTransitionOutcome,
    ProviderIdentityTransitionPublicationAttempt, ProviderIdentityTransitionPublicationResult,
    ProviderIdentityTransitionRemoteObservation, ProviderIdentityTransitionRequest,
    ProviderIdentityTransitionSession, ProviderKeyAgreementRequest, ProviderKeyAlgorithm,
    ProviderKeyPurpose, ProviderKeySelector, ProviderLegacyRootExportRequest,
    ProviderLegacyRootImportOutcome, ProviderLegacyRootImportRequest, ProviderObjectProofRequest,
    ProviderOriginProofRequest, ProviderPreparedDocumentChange, ProviderPreparedHttpSignature,
    ProviderPreparedIdentityTransition, ProviderPrivateKeyEncoding, ProviderPublicIdentity,
    ProviderPublicKey, ProviderPublicationAttempt, ProviderPublicationEvidence,
    ProviderPublicationResult, ProviderRequestSigningEnrollmentRequest, ProviderResult,
    ProviderRootCapability, ProviderSharedSecret, ProviderSignRequest, ProviderSignature,
    ProviderSignedOriginProof, ProviderSigningPurpose, ProviderStoreInfo,
    ProviderVerifiedRemoteDocument, ProviderWrappedRootEnvelope,
};

pub(crate) struct DirectAnpIdentityCustody {
    manager: Arc<Mutex<anp_identity::IdentityManager>>,
}

pub(crate) struct DirectAnpIdentitySession {
    identity: DirectIdentityHandle,
}

#[derive(Clone)]
enum DirectIdentityHandle {
    Owned(Arc<Mutex<anp_identity::ManagedIdentity>>),
    Shared(Arc<anp_identity::ManagedIdentity>),
}

struct DirectDocumentChangeSession {
    session: Arc<Mutex<anp_identity::DocumentChangeSession>>,
}

struct DirectIdentityTransitionSession {
    session: Arc<Mutex<anp_identity::IdentityTransitionSession>>,
}

struct DirectEnrollmentSession {
    session: Arc<Mutex<Option<anp_identity::host::EnrollmentSession>>>,
    manager: Arc<Mutex<anp_identity::IdentityManager>>,
}

impl DirectAnpIdentityCustody {
    pub(crate) fn new(manager: anp_identity::IdentityManager) -> Self {
        Self {
            manager: Arc::new(Mutex::new(manager)),
        }
    }
}

impl DirectAnpIdentitySession {
    pub(crate) fn new(identity: anp_identity::ManagedIdentity) -> Self {
        Self {
            identity: DirectIdentityHandle::Owned(Arc::new(Mutex::new(identity))),
        }
    }

    pub(crate) fn from_shared(identity: Arc<anp_identity::ManagedIdentity>) -> Self {
        Self {
            identity: DirectIdentityHandle::Shared(identity),
        }
    }
}

#[async_trait]
impl IdentityCustody for DirectAnpIdentityCustody {
    async fn store_info(&self) -> ProviderResult<ProviderStoreInfo> {
        let manager = self.manager.clone();
        run_blocking(move || {
            let info = manager
                .lock()
                .map_err(|_| internal())?
                .info()
                .map_err(map_identity_error)?;
            Ok(ProviderStoreInfo {
                store_id: info.store_id,
                schema_compatible: info.schema_compatible,
                identity_count: info.identity_count,
            })
        })
        .await
    }

    async fn list_identities(&self) -> ProviderResult<Vec<ProviderIdentityDescriptor>> {
        let manager = self.manager.clone();
        run_blocking(move || {
            manager
                .lock()
                .map_err(|_| internal())?
                .list()
                .map_err(map_identity_error)?
                .into_iter()
                .map(|item| {
                    Ok(ProviderIdentityDescriptor {
                        reference: item.reference.into(),
                        state: item.state.into(),
                    })
                })
                .collect()
        })
        .await
    }

    async fn open_identity(
        &self,
        identity: &ProviderIdentityRef,
    ) -> ProviderResult<Arc<dyn IdentitySession>> {
        let manager = self.manager.clone();
        let identity = identity.clone();
        run_blocking(move || {
            let managed = manager
                .lock()
                .map_err(|_| internal())?
                .get(&identity.into())
                .map_err(map_identity_error)?;
            Ok(Arc::new(DirectAnpIdentitySession::new(managed)) as Arc<dyn IdentitySession>)
        })
        .await
    }

    async fn create_identity(
        &self,
        request: ProviderCreateIdentityRequest,
    ) -> ProviderResult<Arc<dyn IdentitySession>> {
        let manager = self.manager.clone();
        run_blocking(move || {
            let request = crate::internal::identity_custody::native_create_spec(request);
            let managed = manager
                .lock()
                .map_err(|_| internal())?
                .create(request)
                .map_err(map_identity_error)?;
            Ok(Arc::new(DirectAnpIdentitySession::new(managed)) as Arc<dyn IdentitySession>)
        })
        .await
    }

    async fn delete_identity(&self, identity: &ProviderIdentityRef) -> ProviderResult<()> {
        let manager = self.manager.clone();
        let identity = identity.clone();
        run_blocking(move || {
            manager
                .lock()
                .map_err(|_| internal())?
                .delete(
                    &identity.into(),
                    anp_identity::DeleteIdentityRequest::default(),
                )
                .map_err(map_identity_error)
        })
        .await
    }

    async fn prepare_identity_transition(
        &self,
        request: ProviderIdentityTransitionRequest,
    ) -> ProviderResult<Arc<dyn ProviderIdentityTransitionSession>> {
        let manager = self.manager.clone();
        run_blocking(move || {
            let session = manager
                .lock()
                .map_err(|_| internal())?
                .prepare_identity_transition(anp_identity::IdentityTransitionRequest {
                    expected_current_did: request.expected_current_did,
                    operation_id: request.operation_id,
                    successor: request.successor.into(),
                    transition_document: request
                        .transition_document
                        .map(anp_identity::DidDocument::from_value),
                    provider_document: request
                        .provider_document
                        .map(anp_identity::DidDocument::from_value),
                })
                .map_err(map_identity_error)?;
            Ok(Arc::new(DirectIdentityTransitionSession {
                session: Arc::new(Mutex::new(session)),
            }) as Arc<dyn ProviderIdentityTransitionSession>)
        })
        .await
    }

    async fn resume_identity_transition(
        &self,
        expected_current_did: &str,
    ) -> ProviderResult<Option<Arc<dyn ProviderIdentityTransitionSession>>> {
        let manager = self.manager.clone();
        let expected_current_did = expected_current_did.to_owned();
        run_blocking(move || {
            manager
                .lock()
                .map_err(|_| internal())?
                .resume_identity_transition(&expected_current_did)
                .map_err(map_identity_error)
                .map(|session| {
                    session.map(|session| {
                        Arc::new(DirectIdentityTransitionSession {
                            session: Arc::new(Mutex::new(session)),
                        }) as Arc<dyn ProviderIdentityTransitionSession>
                    })
                })
        })
        .await
    }

    async fn begin_device_enrollment(
        &self,
        request: ProviderDeviceEnrollmentRequest,
    ) -> ProviderResult<Arc<dyn ProviderEnrollmentSession>> {
        use anp_identity::host::EnrollmentWorkflow;
        let manager = self.manager.clone();
        run_blocking(move || {
            let session = manager
                .lock()
                .map_err(|_| internal())?
                .begin_device_enrollment(anp_identity::host::DeviceEnrollmentRequest {
                    remote: request.remote.into(),
                    device_id: request.device_id,
                    device_signing_fragment: request.device_signing_fragment,
                    device_agreement_fragment: request.device_agreement_fragment,
                    profiles: request.profiles,
                    capabilities: anp_identity::host::EnrollmentCapabilities {
                        did_wba: request.capabilities.did_wba,
                    },
                })
                .map_err(map_identity_error)?;
            Ok(Arc::new(DirectEnrollmentSession {
                session: Arc::new(Mutex::new(Some(session))),
                manager: manager.clone(),
            }) as Arc<dyn ProviderEnrollmentSession>)
        })
        .await
    }

    async fn begin_request_signing_enrollment(
        &self,
        request: ProviderRequestSigningEnrollmentRequest,
    ) -> ProviderResult<Arc<dyn ProviderEnrollmentSession>> {
        use anp_identity::host::EnrollmentWorkflow;
        let manager = self.manager.clone();
        run_blocking(move || {
            let session = manager
                .lock()
                .map_err(|_| internal())?
                .begin_request_signing_enrollment(
                    anp_identity::host::RequestSigningEnrollmentRequest {
                        remote: request.remote.into(),
                        fragment: request.fragment,
                        capabilities: anp_identity::host::EnrollmentCapabilities {
                            did_wba: request.capabilities.did_wba,
                        },
                    },
                )
                .map_err(map_identity_error)?;
            Ok(Arc::new(DirectEnrollmentSession {
                session: Arc::new(Mutex::new(Some(session))),
                manager: manager.clone(),
            }) as Arc<dyn ProviderEnrollmentSession>)
        })
        .await
    }

    async fn resume_enrollment(
        &self,
        identity: &ProviderIdentityRef,
    ) -> ProviderResult<Option<Arc<dyn ProviderEnrollmentSession>>> {
        use anp_identity::host::EnrollmentWorkflow;
        let manager = self.manager.clone();
        let identity = identity.clone();
        run_blocking(move || {
            manager
                .lock()
                .map_err(|_| internal())?
                .resume_enrollment(&identity.into())
                .map_err(map_identity_error)
                .map(|session| {
                    session.map(|session| {
                        Arc::new(DirectEnrollmentSession {
                            session: Arc::new(Mutex::new(Some(session))),
                            manager: manager.clone(),
                        }) as Arc<dyn ProviderEnrollmentSession>
                    })
                })
        })
        .await
    }

    async fn confirm_root_promotion(
        &self,
        identity: &ProviderIdentityRef,
        remote: ProviderVerifiedRemoteDocument,
    ) -> ProviderResult<()> {
        let manager = self.manager.clone();
        let identity = identity.clone();
        run_blocking(move || {
            let mut managed = manager
                .lock()
                .map_err(|_| internal())?
                .get(&identity.into())
                .map_err(map_identity_error)?;
            managed
                .confirm_root_promotion(anp_identity::host::RootPromotionRequest {
                    remote: remote.into(),
                })
                .map_err(map_identity_error)?;
            if managed
                .host_status()
                .map_err(map_identity_error)?
                .root_capability
                != anp_identity::host::HostRootCapability::Active
            {
                return Err(IdentityProviderError::new(
                    IdentityProviderErrorCode::InvalidState,
                    false,
                ));
            }
            Ok(())
        })
        .await
    }

    async fn sign_pending_root_object_proof(
        &self,
        identity: &ProviderIdentityRef,
        request: ProviderObjectProofRequest,
    ) -> ProviderResult<serde_json::Value> {
        let manager = self.manager.clone();
        let identity = identity.clone();
        run_blocking(move || {
            let managed = manager
                .lock()
                .map_err(|_| internal())?
                .get(&identity.into())
                .map_err(map_identity_error)?;
            managed
                .sign_pending_root_object_proof(anp_identity::host::PendingRootObjectProofRequest {
                    key: request.key.into(),
                    document: request.document,
                    issuer_did: request.issuer_did,
                    created: request.created,
                })
                .map_err(map_identity_error)
        })
        .await
    }

    async fn sign_document_proof(
        &self,
        identity: &ProviderIdentityRef,
        request: ProviderDocumentProofRequest,
    ) -> ProviderResult<serde_json::Value> {
        use anp_identity::host::TypedProofPort;

        let manager = self.manager.clone();
        let identity = identity.clone();
        run_blocking(move || {
            let managed = manager
                .lock()
                .map_err(|_| internal())?
                .get(&identity.into())
                .map_err(map_identity_error)?;
            managed
                .sign_document_proof(anp_identity::host::DocumentProofRequest {
                    key: request.key.into(),
                    document: request.document,
                    options: anp_identity::host::DocumentProofOptions {
                        proof_purpose: request.options.proof_purpose,
                        proof_type: request.options.proof_type,
                        cryptosuite: request.options.cryptosuite,
                        created: request.options.created,
                        domain: request.options.domain,
                        challenge: request.options.challenge,
                    },
                })
                .map_err(map_identity_error)
        })
        .await
    }

    async fn import_legacy_root(
        &self,
        request: ProviderLegacyRootImportRequest,
    ) -> ProviderResult<ProviderLegacyRootImportOutcome> {
        use anp_identity::host::RootImportPort;
        let manager = self.manager.clone();
        run_blocking(move || {
            let mut managed = manager
                .lock()
                .map_err(|_| internal())?
                .get(&request.identity.into())
                .map_err(map_identity_error)?;
            managed
                .import_legacy_root(anp_identity::host::LegacyRootImportRequest {
                    evidence: anp_identity::host::LegacyRootImportEvidence {
                        transfer_id: request.evidence.transfer_id,
                        source_did: request.evidence.source_did,
                        target_did: request.evidence.target_did,
                        sender_device_id: request.evidence.sender_device_id,
                        recipient_device_id: request.evidence.recipient_device_id,
                        recipient_agreement_kid: request.evidence.recipient_agreement_kid,
                        root_kid: request.evidence.root_kid,
                        checkpoint: anp_identity::host::HostDocumentCheckpoint {
                            document_version: request.evidence.checkpoint.document_version,
                            registry_version: request.evidence.checkpoint.registry_version,
                            document_digest: request.evidence.checkpoint.document_digest,
                        },
                        accepted_at: request.evidence.accepted_at,
                    },
                    encoding: match request.encoding {
                        ProviderPrivateKeyEncoding::Raw32 => {
                            anp_identity::host::RootPrivateKeyEncoding::Raw32
                        }
                        ProviderPrivateKeyEncoding::Pkcs8Der => {
                            anp_identity::host::RootPrivateKeyEncoding::Pkcs8Der
                        }
                    },
                    root_key: request.root_key,
                })
                .map(|outcome| match outcome {
                    anp_identity::host::LegacyRootImportOutcome::Pending => {
                        ProviderLegacyRootImportOutcome::Pending
                    }
                    anp_identity::host::LegacyRootImportOutcome::Active => {
                        ProviderLegacyRootImportOutcome::Active
                    }
                })
                .map_err(map_identity_error)
        })
        .await
    }

    async fn import_wrapped_root(
        &self,
        identity: &ProviderIdentityRef,
        envelope: ProviderWrappedRootEnvelope,
    ) -> ProviderResult<ProviderLegacyRootImportOutcome> {
        use anp_identity::host::WrappedRootImportPort;
        let manager = self.manager.clone();
        let identity = identity.clone();
        run_blocking(move || {
            let mut managed = manager
                .lock()
                .map_err(|_| internal())?
                .get(&identity.into())
                .map_err(map_identity_error)?;
            managed
                .import_wrapped_root_envelope(&anp_identity::host::WrappedRootEnvelope {
                    envelope_type: envelope.envelope_type,
                    version: envelope.version,
                    context: anp_identity::host::RootTransferContext {
                        source_did: envelope.context.source_did,
                        target_did: envelope.context.target_did,
                        sender_device_id: envelope.context.sender_device_id,
                        recipient_device_id: envelope.context.recipient_device_id,
                        recipient_agreement_kid: envelope.context.recipient_agreement_kid,
                        root_kid: envelope.context.root_kid,
                        checkpoint: anp_identity::host::DocumentCheckpoint {
                            document_version: envelope.context.checkpoint.document_version,
                            registry_version: envelope.context.checkpoint.registry_version,
                            document_digest: envelope.context.checkpoint.document_digest,
                        },
                        created_at: envelope.context.created_at,
                        expires_at: envelope.context.expires_at,
                    },
                    ephemeral_public_b64u: envelope.ephemeral_public_b64u,
                    nonce_b64u: envelope.nonce_b64u,
                    ciphertext_b64u: envelope.ciphertext_b64u,
                    signature_b64u: envelope.signature_b64u,
                })
                .map(|outcome| match outcome {
                    anp_identity::host::WrappedRootImportOutcome::Pending => {
                        ProviderLegacyRootImportOutcome::Pending
                    }
                    anp_identity::host::WrappedRootImportOutcome::Active => {
                        ProviderLegacyRootImportOutcome::Active
                    }
                })
                .map_err(map_identity_error)
        })
        .await
    }

    async fn import_identity_material(
        &self,
        request: ProviderIdentityMaterialImportRequest,
    ) -> ProviderResult<Arc<dyn IdentitySession>> {
        use anp_identity::host::MigrationPort;
        let manager = self.manager.clone();
        run_blocking(move || {
            let mut manager = manager.lock().map_err(|_| internal())?;
            let has_root = request
                .keys
                .iter()
                .any(|key| key.purpose == ProviderKeyPurpose::RootControl);
            let managed = if has_root {
                manager.import_full_identity(anp_identity::host::FullIdentityImportRequest {
                    remote: request.remote.into(),
                    did_wba: request.did_wba,
                    private_keys: request.keys.into_iter().map(native_migration_key).collect(),
                })
            } else {
                let mut signing = None;
                let mut agreement = None;
                for key in request.keys {
                    match key.purpose {
                        ProviderKeyPurpose::DeviceAssertion if signing.is_none() => {
                            signing = Some(native_migration_key(key));
                        }
                        ProviderKeyPurpose::KeyAgreement if agreement.is_none() => {
                            agreement = Some(native_migration_key(key));
                        }
                        _ => return Err(invalid_request()),
                    }
                }
                manager.import_device_identity(anp_identity::host::DeviceIdentityImportRequest {
                    remote: request.remote.into(),
                    did_wba: request.did_wba,
                    signing_key: signing.ok_or_else(invalid_request)?,
                    agreement_key: agreement.ok_or_else(invalid_request)?,
                })
            }
            .map_err(map_identity_error)?;
            Ok(Arc::new(DirectAnpIdentitySession::new(managed)) as Arc<dyn IdentitySession>)
        })
        .await
    }

    async fn recover(&self) -> ProviderResult<()> {
        let manager = self.manager.clone();
        run_blocking(move || {
            manager
                .lock()
                .map_err(|_| internal())?
                .recover()
                .map(|_| ())
                .map_err(map_identity_error)
        })
        .await
    }
}

fn native_migration_key(
    key: ProviderIdentityMaterialKey,
) -> anp_identity::host::MigrationPrivateKey {
    anp_identity::host::MigrationPrivateKey {
        kid: key.kid,
        purpose: match key.purpose {
            ProviderKeyPurpose::RootControl => anp_identity::host::MigrationKeyPurpose::RootControl,
            ProviderKeyPurpose::Authentication => {
                anp_identity::host::MigrationKeyPurpose::Authentication
            }
            ProviderKeyPurpose::DeviceAssertion => {
                anp_identity::host::MigrationKeyPurpose::DeviceAssertion
            }
            ProviderKeyPurpose::ApplicationAssertion => {
                anp_identity::host::MigrationKeyPurpose::ApplicationAssertion
            }
            ProviderKeyPurpose::KeyAgreement => {
                anp_identity::host::MigrationKeyPurpose::KeyAgreement
            }
        },
        encoding: match key.encoding {
            ProviderPrivateKeyEncoding::Raw32 => {
                anp_identity::host::MigrationPrivateKeyEncoding::Raw32
            }
            ProviderPrivateKeyEncoding::Pkcs8Der => {
                anp_identity::host::MigrationPrivateKeyEncoding::Pkcs8Der
            }
        },
        secret: key.secret,
    }
}

#[async_trait]
impl IdentitySession for DirectAnpIdentitySession {
    async fn public_identity(&self) -> ProviderResult<ProviderPublicIdentity> {
        let identity = self.identity.clone();
        run_blocking(move || {
            with_identity(&identity, |identity| identity.public_identity()).map(Into::into)
        })
        .await
    }

    async fn host_status(&self) -> ProviderResult<ProviderHostStatus> {
        let identity = self.identity.clone();
        run_blocking(move || {
            with_identity(&identity, |identity| identity.host_status()).map(|status| {
                ProviderHostStatus {
                    root_capability: match status.root_capability {
                        anp_identity::host::HostRootCapability::Absent => {
                            ProviderRootCapability::Absent
                        }
                        anp_identity::host::HostRootCapability::Pending => {
                            ProviderRootCapability::Pending
                        }
                        anp_identity::host::HostRootCapability::Active => {
                            ProviderRootCapability::Active
                        }
                    },
                    root_key_fingerprint: status.root_key_fingerprint,
                    checkpoint: status
                        .checkpoint
                        .map(|checkpoint| ProviderDocumentCheckpoint {
                            document_version: checkpoint.document_version,
                            registry_version: checkpoint.registry_version,
                            document_digest: checkpoint.document_digest,
                        }),
                }
            })
        })
        .await
    }

    async fn sign(&self, request: ProviderSignRequest) -> ProviderResult<ProviderSignature> {
        let identity = self.identity.clone();
        run_blocking(move || {
            with_identity(&identity, |identity| identity.sign(request.clone().into()))
                .map(Into::into)
        })
        .await
    }

    async fn sign_origin_proof(
        &self,
        request: ProviderOriginProofRequest,
    ) -> ProviderResult<ProviderSignedOriginProof> {
        let identity = self.identity.clone();
        run_blocking(move || {
            with_identity(&identity, |identity| {
                identity.sign_origin_proof(request.clone().into())
            })
            .map(Into::into)
        })
        .await
    }

    async fn prepare_http_signature(
        &self,
        request: ProviderExactHttpRequest,
    ) -> ProviderResult<ProviderPreparedHttpSignature> {
        let identity = self.identity.clone();
        run_blocking(move || {
            with_identity(&identity, |identity| {
                identity.prepare_http_signature(request.clone().into())
            })
            .map(Into::into)
        })
        .await
    }

    async fn prepare_document_change(
        &self,
        request: serde_json::Value,
    ) -> ProviderResult<Arc<dyn ProviderDocumentChangeSession>> {
        let identity = self.identity.clone();
        run_blocking(move || {
            let request = serde_json::from_value(snake_case_json(request)).map_err(|_| {
                IdentityProviderError::new(IdentityProviderErrorCode::InvalidRequest, false)
            })?;
            let session = with_owned_identity(&identity, |identity| {
                identity.prepare_document_change(request)
            })?;
            Ok(Arc::new(DirectDocumentChangeSession {
                session: Arc::new(Mutex::new(session)),
            }) as Arc<dyn ProviderDocumentChangeSession>)
        })
        .await
    }

    async fn resume_document_change(
        &self,
    ) -> ProviderResult<Option<Arc<dyn ProviderDocumentChangeSession>>> {
        let identity = self.identity.clone();
        run_blocking(move || {
            with_owned_identity(&identity, |identity| identity.resume_document_change()).map(
                |session| {
                    session.map(|session| {
                        Arc::new(DirectDocumentChangeSession {
                            session: Arc::new(Mutex::new(session)),
                        }) as Arc<dyn ProviderDocumentChangeSession>
                    })
                },
            )
        })
        .await
    }

    async fn adopt_verified_document(
        &self,
        remote: ProviderVerifiedRemoteDocument,
    ) -> ProviderResult<ProviderPublicIdentity> {
        let identity = self.identity.clone();
        run_blocking(move || {
            with_owned_identity(&identity, |identity| {
                identity.adopt_verified_document(remote.clone().into())?;
                identity.public_identity()
            })
            .map(Into::into)
        })
        .await
    }

    async fn derive_shared_secret(
        &self,
        request: ProviderKeyAgreementRequest,
    ) -> ProviderResult<ProviderSharedSecret> {
        let identity = self.identity.clone();
        run_blocking(move || {
            with_identity(&identity, |identity| {
                identity.derive_shared_secret(KeyAgreementRequest {
                    key: request.key.into(),
                    peer_public: request.peer_public,
                })
            })
            .map(|secret| ProviderSharedSecret::new(*secret.as_bytes()))
        })
        .await
    }

    async fn export_root_for_legacy_envelope(
        &self,
        request: ProviderLegacyRootExportRequest,
    ) -> ProviderResult<ProviderExportedRoot> {
        let identity = self.identity.clone();
        run_blocking(move || {
            with_identity(&identity, |identity| {
                identity.export_root_for_legacy_envelope(UserConfirmedRootExportRequest {
                    key: request.key.into(),
                    user_presence_confirmed: request.user_presence_confirmed,
                })
            })
            .map(|root| ProviderExportedRoot::new(root.as_pkcs8_der().to_vec()))
        })
        .await
    }

    async fn recover(&self) -> ProviderResult<()> {
        let identity = self.identity.clone();
        run_blocking(move || with_identity(&identity, |identity| identity.recover_identity())).await
    }
}

fn snake_case_json(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.into_iter().map(snake_case_json).collect())
        }
        serde_json::Value::Object(values) => serde_json::Value::Object(
            values
                .into_iter()
                .map(|(key, value)| (camel_to_snake(&key), snake_case_json(value)))
                .collect(),
        ),
        value => value,
    }
}

fn camel_to_snake(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for character in value.chars() {
        if character.is_ascii_uppercase() {
            output.push('_');
            output.push(character.to_ascii_lowercase());
        } else {
            output.push(character);
        }
    }
    output
}

fn provider_transition_candidate(
    candidate: &anp_identity::PreparedIdentityTransition,
) -> ProviderResult<ProviderPreparedIdentityTransition> {
    let assurance = match candidate.assurance.as_str() {
        "verified" => super::ProviderTransitionAssurance::Verified,
        "recovery_verified" => super::ProviderTransitionAssurance::RecoveryVerified,
        "provider_asserted" => super::ProviderTransitionAssurance::ProviderAsserted,
        "unverified" => super::ProviderTransitionAssurance::Unverified,
        _ => return Err(internal()),
    };
    Ok(ProviderPreparedIdentityTransition {
        operation_id: candidate.operation_id.clone(),
        expected_current_did: candidate.expected_current_did.clone(),
        successor_did: candidate.successor_did.clone(),
        predecessor_document: candidate.predecessor_document.as_value().clone(),
        successor_document: candidate.successor_document.as_value().clone(),
        predecessor_digest: candidate.predecessor_digest.clone(),
        successor_digest: candidate.successor_digest.clone(),
        assurance,
    })
}

fn provider_transition_attempt(
    attempt: anp_identity::IdentityTransitionPublicationAttempt,
) -> ProviderResult<ProviderIdentityTransitionPublicationAttempt> {
    let value = serde_json::to_value(attempt).map_err(|_| internal())?;
    Ok(ProviderIdentityTransitionPublicationAttempt {
        operation_id: value
            .get("operation_id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(internal)?
            .to_owned(),
        predecessor_digest: value
            .get("predecessor_digest")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(internal)?
            .to_owned(),
        successor_digest: value
            .get("successor_digest")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(internal)?
            .to_owned(),
        publication_generation: value
            .get("publication_generation")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(internal)?,
    })
}

fn native_transition_attempt(
    attempt: ProviderIdentityTransitionPublicationAttempt,
) -> ProviderResult<anp_identity::IdentityTransitionPublicationAttempt> {
    serde_json::from_value(serde_json::json!({
        "operation_id": attempt.operation_id,
        "predecessor_digest": attempt.predecessor_digest,
        "successor_digest": attempt.successor_digest,
        "publication_generation": attempt.publication_generation,
    }))
    .map_err(|_| internal())
}

fn native_transition_result(
    result: ProviderIdentityTransitionPublicationResult,
) -> anp_identity::IdentityTransitionPublicationResult {
    match result {
        ProviderIdentityTransitionPublicationResult::Confirmed { evidence } => {
            anp_identity::IdentityTransitionPublicationResult::Confirmed {
                evidence: anp_identity::IdentityTransitionPublicationEvidence {
                    predecessor_digest: evidence.predecessor_digest,
                    successor_digest: evidence.successor_digest,
                },
            }
        }
        ProviderIdentityTransitionPublicationResult::RejectedBeforeAcceptance => {
            anp_identity::IdentityTransitionPublicationResult::RejectedBeforeAcceptance
        }
        ProviderIdentityTransitionPublicationResult::Unknown => {
            anp_identity::IdentityTransitionPublicationResult::Unknown
        }
    }
}

fn native_transition_observation(
    observation: ProviderIdentityTransitionRemoteObservation,
) -> anp_identity::IdentityTransitionRemoteObservation {
    match observation {
        ProviderIdentityTransitionRemoteObservation::RemoteOld { current_document } => {
            anp_identity::IdentityTransitionRemoteObservation::RemoteOld {
                current_document: anp_identity::DidDocument::from_value(current_document),
            }
        }
        ProviderIdentityTransitionRemoteObservation::Published {
            predecessor_document,
            successor_document,
        } => anp_identity::IdentityTransitionRemoteObservation::Published {
            predecessor_document: anp_identity::DidDocument::from_value(predecessor_document),
            successor_document: anp_identity::DidDocument::from_value(successor_document),
        },
    }
}

fn provider_transition_outcome(
    outcome: anp_identity::IdentityTransitionOutcome,
) -> ProviderIdentityTransitionOutcome {
    match outcome {
        anp_identity::IdentityTransitionOutcome::ReadyForPublication => {
            ProviderIdentityTransitionOutcome::ReadyForPublication
        }
        anp_identity::IdentityTransitionOutcome::PublicationUncertain => {
            ProviderIdentityTransitionOutcome::PublicationUncertain
        }
        anp_identity::IdentityTransitionOutcome::Committed { current_did } => {
            ProviderIdentityTransitionOutcome::Committed { current_did }
        }
        anp_identity::IdentityTransitionOutcome::Aborted => {
            ProviderIdentityTransitionOutcome::Aborted
        }
    }
}

fn with_identity<T>(
    handle: &DirectIdentityHandle,
    operation: impl FnOnce(&anp_identity::ManagedIdentity) -> anp_identity::IdentityResult<T>,
) -> ProviderResult<T> {
    match handle {
        DirectIdentityHandle::Owned(identity) => {
            let identity = identity.lock().map_err(|_| internal())?;
            operation(&identity)
        }
        DirectIdentityHandle::Shared(identity) => operation(identity),
    }
    .map_err(map_identity_error)
}

fn with_owned_identity<T>(
    handle: &DirectIdentityHandle,
    operation: impl FnOnce(&mut anp_identity::ManagedIdentity) -> anp_identity::IdentityResult<T>,
) -> ProviderResult<T> {
    let DirectIdentityHandle::Owned(identity) = handle else {
        return Err(IdentityProviderError::new(
            IdentityProviderErrorCode::CapabilityUnavailable,
            false,
        ));
    };
    let mut identity = identity.lock().map_err(|_| internal())?;
    operation(&mut identity).map_err(map_identity_error)
}

#[async_trait]
impl ProviderDocumentChangeSession for DirectDocumentChangeSession {
    async fn candidate(&self) -> ProviderResult<ProviderPreparedDocumentChange> {
        let session = self.session.clone();
        run_blocking(move || {
            let session = session.lock().map_err(|_| internal())?;
            Ok(session.candidate().clone().into())
        })
        .await
    }

    async fn host_phase(&self) -> ProviderResult<ProviderDocumentChangePhase> {
        use anp_identity::host::DocumentChangeRecoveryPort;
        let session = self.session.clone();
        run_blocking(move || {
            let phase = session
                .lock()
                .map_err(|_| internal())?
                .host_phase()
                .map_err(map_identity_error)?;
            Ok(match phase {
                anp_identity::host::HostDocumentChangePhase::Prepared => {
                    ProviderDocumentChangePhase::Prepared
                }
                anp_identity::host::HostDocumentChangePhase::PublicationInFlight => {
                    ProviderDocumentChangePhase::PublicationInFlight
                }
                anp_identity::host::HostDocumentChangePhase::PublicationUncertain => {
                    ProviderDocumentChangePhase::PublicationUncertain
                }
                anp_identity::host::HostDocumentChangePhase::Published => {
                    ProviderDocumentChangePhase::Published
                }
            })
        })
        .await
    }

    async fn begin_publication(&self) -> ProviderResult<ProviderPublicationAttempt> {
        let session = self.session.clone();
        run_blocking(move || {
            let attempt = session
                .lock()
                .map_err(|_| internal())?
                .begin_publication()
                .map_err(map_identity_error)?;
            let publication_generation = serde_json::to_value(&attempt)
                .ok()
                .and_then(|value| {
                    value
                        .get("publication_generation")
                        .and_then(|value| value.as_u64())
                })
                .ok_or_else(internal)?;
            Ok(ProviderPublicationAttempt {
                operation_id: attempt.operation_id().to_owned(),
                candidate_digest: attempt.candidate_digest().to_owned(),
                publication_generation,
            })
        })
        .await
    }

    async fn complete(
        &self,
        attempt: ProviderPublicationAttempt,
        result: ProviderPublicationResult,
    ) -> ProviderResult<ProviderDocumentChangeOutcome> {
        let session = self.session.clone();
        run_blocking(move || {
            session
                .lock()
                .map_err(|_| internal())?
                .complete(
                    serde_json::from_value(serde_json::json!({
                        "operation_id": attempt.operation_id,
                        "candidate_digest": attempt.candidate_digest,
                        "publication_generation": attempt.publication_generation,
                    }))
                    .map_err(|_| internal())?,
                    result.into(),
                )
                .map(Into::into)
                .map_err(map_identity_error)
        })
        .await
    }

    async fn reconcile(
        &self,
        observation: ProviderVerifiedRemoteDocument,
    ) -> ProviderResult<ProviderDocumentChangeOutcome> {
        let session = self.session.clone();
        run_blocking(move || {
            session
                .lock()
                .map_err(|_| internal())?
                .reconcile(observation.into())
                .map(Into::into)
                .map_err(map_identity_error)
        })
        .await
    }
}

#[async_trait]
impl ProviderIdentityTransitionSession for DirectIdentityTransitionSession {
    async fn candidate(&self) -> ProviderResult<ProviderPreparedIdentityTransition> {
        let session = self.session.clone();
        run_blocking(move || {
            let session = session.lock().map_err(|_| internal())?;
            provider_transition_candidate(session.candidate())
        })
        .await
    }

    async fn begin_publication(
        &self,
    ) -> ProviderResult<ProviderIdentityTransitionPublicationAttempt> {
        let session = self.session.clone();
        run_blocking(move || {
            let attempt = session
                .lock()
                .map_err(|_| internal())?
                .begin_publication()
                .map_err(map_identity_error)?;
            provider_transition_attempt(attempt)
        })
        .await
    }

    async fn complete(
        &self,
        attempt: ProviderIdentityTransitionPublicationAttempt,
        result: ProviderIdentityTransitionPublicationResult,
    ) -> ProviderResult<ProviderIdentityTransitionOutcome> {
        let session = self.session.clone();
        run_blocking(move || {
            let outcome = session
                .lock()
                .map_err(|_| internal())?
                .complete(
                    native_transition_attempt(attempt)?,
                    native_transition_result(result),
                )
                .map_err(map_identity_error)?;
            Ok(provider_transition_outcome(outcome))
        })
        .await
    }

    async fn reconcile(
        &self,
        observation: ProviderIdentityTransitionRemoteObservation,
    ) -> ProviderResult<ProviderIdentityTransitionOutcome> {
        let session = self.session.clone();
        run_blocking(move || {
            let outcome = session
                .lock()
                .map_err(|_| internal())?
                .reconcile(native_transition_observation(observation))
                .map_err(map_identity_error)?;
            Ok(provider_transition_outcome(outcome))
        })
        .await
    }
}

#[async_trait]
impl ProviderEnrollmentSession for DirectEnrollmentSession {
    async fn proposal(&self) -> ProviderResult<ProviderEnrollmentProposal> {
        let session = self.session.clone();
        run_blocking(move || {
            let session = session.lock().map_err(|_| internal())?;
            let session = session.as_ref().ok_or_else(invalid_state)?;
            Ok(session.proposal().clone().into())
        })
        .await
    }

    async fn sign_device_assertion(&self, payload: Vec<u8>) -> ProviderResult<Vec<u8>> {
        let session = self.session.clone();
        run_blocking(move || {
            session
                .lock()
                .map_err(|_| internal())?
                .as_ref()
                .ok_or_else(invalid_state)?
                .sign_device_assertion(&payload)
                .map_err(map_identity_error)
        })
        .await
    }

    async fn derive_device_shared_secret(
        &self,
        peer_public: [u8; 32],
    ) -> ProviderResult<ProviderSharedSecret> {
        let session = self.session.clone();
        run_blocking(move || {
            session
                .lock()
                .map_err(|_| internal())?
                .as_ref()
                .ok_or_else(invalid_state)?
                .derive_device_shared_secret(peer_public)
                .map(|shared| ProviderSharedSecret::new(*shared.as_bytes()))
                .map_err(map_identity_error)
        })
        .await
    }

    async fn activate(&self, remote: ProviderVerifiedRemoteDocument) -> ProviderResult<()> {
        let session = self.session.clone();
        run_blocking(move || {
            let outcome = session
                .lock()
                .map_err(|_| internal())?
                .as_mut()
                .ok_or_else(invalid_state)?
                .activate(remote.into())
                .map_err(map_identity_error)?;
            if outcome != anp_identity::host::ConvergenceOutcome::Activated {
                return Err(invalid_state());
            }
            Ok(())
        })
        .await
    }

    async fn cancel(&self) -> ProviderResult<()> {
        let session = self.session.clone();
        let manager = self.manager.clone();
        run_blocking(move || {
            let session = session
                .lock()
                .map_err(|_| internal())?
                .take()
                .ok_or_else(invalid_state)?;
            let mut manager = manager.lock().map_err(|_| internal())?;
            session.cancel(&mut manager).map_err(map_identity_error)
        })
        .await
    }
}

async fn run_blocking<T>(
    operation: impl FnOnce() -> ProviderResult<T> + Send + 'static,
) -> ProviderResult<T>
where
    T: Send + 'static,
{
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|_| internal())?
}

fn internal() -> IdentityProviderError {
    IdentityProviderError::new(IdentityProviderErrorCode::Internal, false)
}

fn invalid_request() -> IdentityProviderError {
    IdentityProviderError::new(IdentityProviderErrorCode::InvalidRequest, false)
}

fn invalid_state() -> IdentityProviderError {
    IdentityProviderError::new(IdentityProviderErrorCode::InvalidState, false)
}

fn map_identity_error(error: anp_identity::IdentityError) -> IdentityProviderError {
    use anp_identity::IdentityError as Source;
    use IdentityProviderErrorCode as Target;
    let (code, retryable) = match error {
        Source::InvalidRequest => (Target::InvalidRequest, false),
        Source::StoreNotFound => (Target::StoreNotFound, false),
        Source::ProviderUnavailable => (Target::ProviderUnavailable, true),
        Source::RootKeyMismatch => (Target::RootKeyMismatch, false),
        Source::CorruptState => (Target::CorruptState, false),
        Source::IdentityNotFound => (Target::IdentityNotFound, false),
        Source::IdentityAlreadyExists => (Target::IdentityAlreadyExists, false),
        Source::KeyNotFound => (Target::KeyNotFound, false),
        Source::KeyUnavailable => (Target::KeyUnavailable, false),
        Source::KeyPurposeViolation => (Target::KeyPurposeViolation, false),
        Source::AmbiguousKey => (Target::AmbiguousKey, false),
        Source::VerificationFailed => (Target::VerificationFailed, false),
        Source::Conflict => (Target::Conflict, true),
        Source::CapabilityUnavailable | Source::Unsupported => {
            (Target::CapabilityUnavailable, false)
        }
        Source::PendingDocumentChange => (Target::PendingDocumentChange, false),
        Source::DocumentChangeNotFound => (Target::DocumentChangeNotFound, false),
        Source::InvalidDocumentChangeState => (Target::InvalidDocumentChangeState, false),
        Source::Storage => (Target::Storage, true),
        Source::Internal => (Target::Internal, false),
        _ => (Target::Internal, false),
    };
    IdentityProviderError::new(code, retryable)
}

impl From<ProviderIdentityRef> for anp_identity::IdentityRef {
    fn from(value: ProviderIdentityRef) -> Self {
        Self {
            store_id: value.store_id,
            identity_id: value.identity_id,
            did: value.did,
        }
    }
}

impl From<anp_identity::IdentityRef> for ProviderIdentityRef {
    fn from(value: anp_identity::IdentityRef) -> Self {
        Self {
            store_id: value.store_id,
            identity_id: value.identity_id,
            did: value.did,
        }
    }
}

impl From<anp_identity::PublicIdentityState> for ProviderIdentityState {
    fn from(value: anp_identity::PublicIdentityState) -> Self {
        match value {
            anp_identity::PublicIdentityState::Enrolling => Self::Enrolling,
            anp_identity::PublicIdentityState::Active => Self::Active,
            anp_identity::PublicIdentityState::Revoked => Self::Revoked,
        }
    }
}

impl From<anp_identity::KeyAlgorithm> for ProviderKeyAlgorithm {
    fn from(value: anp_identity::KeyAlgorithm) -> Self {
        match value {
            anp_identity::KeyAlgorithm::Ed25519 => Self::Ed25519,
            anp_identity::KeyAlgorithm::X25519 => Self::X25519,
        }
    }
}

impl From<anp_identity::KeyPurpose> for ProviderKeyPurpose {
    fn from(value: anp_identity::KeyPurpose) -> Self {
        match value {
            anp_identity::KeyPurpose::RootControl => Self::RootControl,
            anp_identity::KeyPurpose::Authentication => Self::Authentication,
            anp_identity::KeyPurpose::DeviceAssertion => Self::DeviceAssertion,
            anp_identity::KeyPurpose::ApplicationAssertion => Self::ApplicationAssertion,
            anp_identity::KeyPurpose::KeyAgreement => Self::KeyAgreement,
        }
    }
}

impl From<anp_identity::PublicIdentity> for ProviderPublicIdentity {
    fn from(value: anp_identity::PublicIdentity) -> Self {
        Self {
            reference: value.reference.into(),
            state: value.state.into(),
            revision: value.revision,
            document: value.document.into_value(),
            active_keys: value
                .active_keys
                .into_iter()
                .map(|key| ProviderPublicKey {
                    kid: key.kid,
                    algorithm: key.algorithm.into(),
                    purposes: key.purposes.into_iter().map(Into::into).collect(),
                })
                .collect(),
            did_wba: value.capabilities.did_wba,
        }
    }
}

impl From<ProviderKeySelector> for anp_identity::KeySelector {
    fn from(value: ProviderKeySelector) -> Self {
        match value {
            ProviderKeySelector::Default => Self::Default,
            ProviderKeySelector::Kid(kid) => Self::Kid(kid),
        }
    }
}

impl From<ProviderSigningPurpose> for anp_identity::SigningPurpose {
    fn from(value: ProviderSigningPurpose) -> Self {
        match value {
            ProviderSigningPurpose::Authentication => Self::Authentication,
            ProviderSigningPurpose::DeviceAssertion => Self::DeviceAssertion,
            ProviderSigningPurpose::ApplicationAssertion { domain } => {
                Self::ApplicationAssertion { domain }
            }
        }
    }
}

impl From<ProviderSignRequest> for anp_identity::SignRequest {
    fn from(value: ProviderSignRequest) -> Self {
        Self {
            purpose: value.purpose.into(),
            key: value.key.into(),
            payload: value.payload,
        }
    }
}

impl From<anp_identity::Signature> for ProviderSignature {
    fn from(value: anp_identity::Signature) -> Self {
        Self {
            kid: value.kid,
            algorithm: value.algorithm.into(),
            bytes: value.bytes,
        }
    }
}

impl From<ProviderOriginProofRequest> for anp_identity::OriginProofRequest {
    fn from(value: ProviderOriginProofRequest) -> Self {
        Self {
            method: value.method,
            meta: value.meta,
            body: value.body,
            key: value.key.into(),
            options: anp_identity::OriginProofOptions {
                created: value.options.created,
                expires: value.options.expires,
                nonce: value.options.nonce,
            },
        }
    }
}

impl From<anp_identity::SignedOriginProof> for ProviderSignedOriginProof {
    fn from(value: anp_identity::SignedOriginProof) -> Self {
        Self {
            content_digest: value.content_digest,
            signature_input: value.signature_input,
            signature: value.signature,
        }
    }
}

impl From<ProviderExactHttpRequest> for anp_identity::host::ExactHttpRequest {
    fn from(value: ProviderExactHttpRequest) -> Self {
        Self {
            key: value.key.into(),
            url: value.url,
            method: value.method,
            headers: value
                .headers
                .into_iter()
                .map(|header| anp_identity::host::HttpHeader {
                    name: header.name,
                    value: header.value,
                })
                .collect(),
            body: value.body,
            options: anp_identity::host::HttpRequestSigningOptions {
                nonce: value.options.nonce,
                created: value.options.created,
                expires: value.options.expires,
                covered_components: value.options.covered_components,
            },
        }
    }
}

impl From<anp_identity::host::PreparedHttpSignatureAttempt> for ProviderPreparedHttpSignature {
    fn from(value: anp_identity::host::PreparedHttpSignatureAttempt) -> Self {
        Self {
            binding_digest: value.binding_digest,
            kid: value.kid,
            header_patch: value
                .header_patch
                .into_iter()
                .map(|header| ProviderHttpHeader {
                    name: header.name,
                    value: header.value,
                })
                .collect(),
        }
    }
}

impl From<ProviderPublicationEvidence> for anp_identity::VerifiedPublicationEvidence {
    fn from(value: ProviderPublicationEvidence) -> Self {
        Self {
            document_version: value.document_version,
            registry_version: value.registry_version,
            document_digest: value.document_digest,
        }
    }
}

impl From<ProviderVerifiedRemoteDocument> for anp_identity::VerifiedRemoteDocument {
    fn from(value: ProviderVerifiedRemoteDocument) -> Self {
        Self {
            document: anp_identity::DidDocument::from_value(value.document),
            evidence: value.evidence.into(),
        }
    }
}

impl From<anp_identity::PreparedDocumentChange> for ProviderPreparedDocumentChange {
    fn from(value: anp_identity::PreparedDocumentChange) -> Self {
        Self {
            operation_id: value.operation_id,
            candidate_document: value.candidate_document.into_value(),
            candidate_digest: value.candidate_digest,
        }
    }
}

impl From<ProviderPublicationResult> for anp_identity::PublicationResult {
    fn from(value: ProviderPublicationResult) -> Self {
        match value {
            ProviderPublicationResult::Confirmed { evidence } => Self::Confirmed {
                evidence: evidence.into(),
            },
            ProviderPublicationResult::RejectedBeforeAcceptance => Self::RejectedBeforeAcceptance,
            ProviderPublicationResult::Unknown => Self::Unknown,
        }
    }
}

impl From<anp_identity::DocumentChangeOutcome> for ProviderDocumentChangeOutcome {
    fn from(value: anp_identity::DocumentChangeOutcome) -> Self {
        match value {
            anp_identity::DocumentChangeOutcome::ReadyForPublication => Self::ReadyForPublication,
            anp_identity::DocumentChangeOutcome::PublicationUncertain => Self::PublicationUncertain,
            anp_identity::DocumentChangeOutcome::Committed { identity } => Self::Committed {
                identity: identity.into(),
            },
            anp_identity::DocumentChangeOutcome::Aborted => Self::Aborted,
        }
    }
}

impl From<anp_identity::host::EnrollmentProposal> for ProviderEnrollmentProposal {
    fn from(value: anp_identity::host::EnrollmentProposal) -> Self {
        Self {
            enrollment_id: value.enrollment_id,
            identity: value.identity.into(),
            kind: match value.kind {
                anp_identity::host::EnrollmentProposalKind::Device {
                    device_id,
                    signing_key,
                    agreement_key,
                    profiles,
                } => ProviderEnrollmentProposalKind::Device {
                    device_id,
                    signing_key: ProviderEnrollmentPublicKey {
                        kid: signing_key.kid,
                        public_key_multibase: signing_key.public_key_multibase,
                    },
                    agreement_key: ProviderEnrollmentPublicKey {
                        kid: agreement_key.kid,
                        public_key_multibase: agreement_key.public_key_multibase,
                    },
                    profiles,
                },
                anp_identity::host::EnrollmentProposalKind::RequestSigning { signing_key } => {
                    ProviderEnrollmentProposalKind::RequestSigning {
                        signing_key: ProviderEnrollmentPublicKey {
                            kid: signing_key.kid,
                            public_key_multibase: signing_key.public_key_multibase,
                        },
                    }
                }
            },
            root_key_fingerprint: value.root_key_fingerprint,
            checkpoint: ProviderDocumentCheckpoint {
                document_version: value.checkpoint.document_version,
                registry_version: value.checkpoint.registry_version,
                document_digest: value.checkpoint.document_digest,
            },
        }
    }
}
