use async_trait::async_trait;
use std::sync::{Arc, Mutex};

use anp_identity::host::{
    ConvergenceWorkflow, HttpRequestSigningPort, IdentityStatusPort, KeyAgreementPort,
    KeyAgreementRequest,
};

use super::{
    IdentityCustody, IdentityProviderError, IdentityProviderErrorCode, IdentitySession,
    ProviderCreateIdentityRequest, ProviderDocumentChangeOutcome, ProviderDocumentChangePhase,
    ProviderDocumentChangeSession, ProviderDocumentCheckpoint, ProviderExactHttpRequest,
    ProviderHostStatus, ProviderHttpHeader, ProviderIdentityDescriptor, ProviderIdentityRef,
    ProviderIdentityState, ProviderKeyAgreementRequest, ProviderKeyAlgorithm, ProviderKeyPurpose,
    ProviderKeySelector, ProviderOriginProofRequest, ProviderPreparedDocumentChange,
    ProviderPreparedHttpSignature, ProviderPublicIdentity, ProviderPublicKey,
    ProviderPublicationAttempt, ProviderPublicationEvidence, ProviderPublicationResult,
    ProviderResult, ProviderRootCapability, ProviderSharedSecret, ProviderSignRequest,
    ProviderSignature, ProviderSignedOriginProof, ProviderSigningPurpose, ProviderStoreInfo,
    ProviderVerifiedRemoteDocument,
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
