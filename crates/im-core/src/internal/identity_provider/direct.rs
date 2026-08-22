use async_trait::async_trait;
use std::sync::{Arc, Mutex};

use anp_identity::host::{
    HttpRequestSigningPort, IdentityStatusPort, KeyAgreementPort, KeyAgreementRequest,
};

use super::{
    IdentityCustody, IdentityProviderError, IdentityProviderErrorCode, IdentitySession,
    ProviderExactHttpRequest, ProviderHttpHeader, ProviderIdentityDescriptor, ProviderIdentityRef,
    ProviderIdentityState, ProviderKeyAgreementRequest, ProviderKeyAlgorithm, ProviderKeyPurpose,
    ProviderKeySelector, ProviderOriginProofRequest, ProviderPreparedHttpSignature,
    ProviderPublicIdentity, ProviderPublicKey, ProviderResult, ProviderSharedSecret,
    ProviderSignRequest, ProviderSignature, ProviderSignedOriginProof, ProviderSigningPurpose,
    ProviderStoreInfo,
};

pub(crate) struct DirectAnpIdentityCustody {
    manager: Arc<Mutex<anp_identity::IdentityManager>>,
}

pub(crate) struct DirectAnpIdentitySession {
    identity: Arc<anp_identity::ManagedIdentity>,
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
            identity: Arc::new(identity),
        }
    }

    pub(crate) fn from_shared(identity: Arc<anp_identity::ManagedIdentity>) -> Self {
        Self { identity }
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
            identity
                .public_identity()
                .map(Into::into)
                .map_err(map_identity_error)
        })
        .await
    }

    async fn sign(&self, request: ProviderSignRequest) -> ProviderResult<ProviderSignature> {
        let identity = self.identity.clone();
        run_blocking(move || {
            identity
                .sign(request.into())
                .map(Into::into)
                .map_err(map_identity_error)
        })
        .await
    }

    async fn sign_origin_proof(
        &self,
        request: ProviderOriginProofRequest,
    ) -> ProviderResult<ProviderSignedOriginProof> {
        let identity = self.identity.clone();
        run_blocking(move || {
            identity
                .sign_origin_proof(request.into())
                .map(Into::into)
                .map_err(map_identity_error)
        })
        .await
    }

    async fn prepare_http_signature(
        &self,
        request: ProviderExactHttpRequest,
    ) -> ProviderResult<ProviderPreparedHttpSignature> {
        let identity = self.identity.clone();
        run_blocking(move || {
            identity
                .prepare_http_signature(request.into())
                .map(Into::into)
                .map_err(map_identity_error)
        })
        .await
    }

    async fn derive_shared_secret(
        &self,
        request: ProviderKeyAgreementRequest,
    ) -> ProviderResult<ProviderSharedSecret> {
        let identity = self.identity.clone();
        run_blocking(move || {
            identity
                .derive_shared_secret(KeyAgreementRequest {
                    key: request.key.into(),
                    peer_public: request.peer_public,
                })
                .map(|secret| ProviderSharedSecret::new(*secret.as_bytes()))
                .map_err(map_identity_error)
        })
        .await
    }

    async fn recover(&self) -> ProviderResult<()> {
        let identity = self.identity.clone();
        run_blocking(move || identity.recover_identity().map_err(map_identity_error)).await
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
        Source::PendingDocumentChange
        | Source::DocumentChangeNotFound
        | Source::InvalidDocumentChangeState => (Target::Conflict, false),
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
