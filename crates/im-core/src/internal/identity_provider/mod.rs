mod direct;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;
use std::sync::Arc;
use zeroize::Zeroizing;

pub(crate) use direct::DirectAnpIdentitySession;

pub const IDENTITY_PROVIDER_PROTOCOL: &str = "anp-identity-provider-ts/1";
pub const CAP_STORE_READ: &str = "store.read";
pub const CAP_IDENTITY_SIGN: &str = "identity.sign";
pub const CAP_ORIGIN_PROOF: &str = "identity.origin-proof";
pub const CAP_HTTP_SIGN: &str = "host.http-sign";
pub const CAP_KEY_AGREEMENT: &str = "host.key-agreement";

pub type ProviderResult<T> = Result<T, IdentityProviderError>;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IdentityProviderErrorCode {
    InvalidRequest,
    StoreNotFound,
    ProviderUnavailable,
    ProviderIncompatible,
    ProviderDisposed,
    RootKeyMismatch,
    CorruptState,
    IdentityNotFound,
    IdentityAlreadyExists,
    KeyNotFound,
    KeyUnavailable,
    KeyPurposeViolation,
    AmbiguousKey,
    VerificationFailed,
    Conflict,
    CapabilityUnavailable,
    RequestCancelled,
    RequestTimeout,
    Storage,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityProviderError {
    pub code: IdentityProviderErrorCode,
    pub retryable: bool,
}

impl IdentityProviderError {
    pub fn new(code: IdentityProviderErrorCode, retryable: bool) -> Self {
        Self { code, retryable }
    }
}

impl fmt::Display for IdentityProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "identity provider failed: {:?}", self.code)
    }
}

impl std::error::Error for IdentityProviderError {}

pub fn map_provider_error(error: IdentityProviderError) -> crate::ImError {
    use IdentityProviderErrorCode as Code;
    match error.code {
        Code::InvalidRequest => crate::ImError::invalid_input(
            Some("identity_provider".to_owned()),
            "identity provider rejected the request",
        ),
        Code::IdentityNotFound | Code::KeyNotFound => crate::ImError::IdentityUnresolved {
            detail: "identity provider reference was not found".to_owned(),
        },
        Code::KeyUnavailable
        | Code::KeyPurposeViolation
        | Code::AmbiguousKey
        | Code::VerificationFailed
        | Code::CapabilityUnavailable
        | Code::RootKeyMismatch => crate::ImError::PermissionDenied,
        Code::ProviderUnavailable
        | Code::ProviderDisposed
        | Code::RequestTimeout
        | Code::Storage => crate::ImError::LocalStateUnavailable {
            detail: "identity provider is temporarily unavailable".to_owned(),
        },
        Code::ProviderIncompatible | Code::CorruptState => crate::ImError::LocalStateUnavailable {
            detail: "identity provider state is incompatible".to_owned(),
        },
        Code::Conflict => crate::ImError::IdentityBindingConflict {
            detail: "identity provider changed concurrently".to_owned(),
        },
        Code::RequestCancelled => crate::ImError::LocalStateUnavailable {
            detail: "identity provider request was cancelled".to_owned(),
        },
        Code::StoreNotFound | Code::IdentityAlreadyExists | Code::Internal => {
            crate::ImError::LocalStateUnavailable {
                detail: "identity provider operation failed".to_owned(),
            }
        }
    }
}

pub async fn derive_shared_secret_or_fallback(
    session: Option<&Arc<dyn IdentitySession>>,
    fallback: &Arc<dyn crate::internal::key_provider::IdentitySigner>,
    kid: &str,
    peer_public: [u8; 32],
) -> crate::ImResult<Zeroizing<[u8; 32]>> {
    if let Some(session) = session {
        let shared = session
            .derive_shared_secret(ProviderKeyAgreementRequest {
                key: ProviderKeySelector::Kid(kid.to_owned()),
                peer_public,
            })
            .await
            .map_err(map_provider_error)?;
        return Ok(Zeroizing::new(*shared.as_bytes()));
    }
    fallback.ecdh(kid, &peer_public)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct ProviderIdentityRef {
    pub store_id: String,
    pub identity_id: String,
    pub did: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderIdentityState {
    Enrolling,
    Active,
    Revoked,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKeyAlgorithm {
    Ed25519,
    X25519,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKeyPurpose {
    RootControl,
    Authentication,
    DeviceAssertion,
    ApplicationAssertion,
    KeyAgreement,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProviderPublicKey {
    pub kid: String,
    pub algorithm: ProviderKeyAlgorithm,
    pub purposes: Vec<ProviderKeyPurpose>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProviderPublicIdentity {
    pub reference: ProviderIdentityRef,
    pub state: ProviderIdentityState,
    pub revision: u64,
    pub document: Value,
    pub active_keys: Vec<ProviderPublicKey>,
    pub did_wba: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProviderStoreInfo {
    pub store_id: String,
    pub schema_compatible: bool,
    pub identity_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProviderIdentityDescriptor {
    pub reference: ProviderIdentityRef,
    pub state: ProviderIdentityState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKeySelector {
    Default,
    Kid(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "purpose", rename_all = "snake_case")]
pub enum ProviderSigningPurpose {
    Authentication,
    DeviceAssertion,
    ApplicationAssertion { domain: String },
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProviderSignRequest {
    pub purpose: ProviderSigningPurpose,
    pub key: ProviderKeySelector,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProviderSignature {
    pub kid: String,
    pub algorithm: ProviderKeyAlgorithm,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProviderOriginProofOptions {
    pub created: Option<i64>,
    pub expires: Option<i64>,
    pub nonce: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProviderOriginProofRequest {
    pub method: String,
    pub meta: Value,
    pub body: Value,
    pub key: ProviderKeySelector,
    #[serde(default)]
    pub options: ProviderOriginProofOptions,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderSignedOriginProof {
    pub content_digest: String,
    pub signature_input: String,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProviderHttpHeader {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProviderHttpSigningOptions {
    pub nonce: Option<String>,
    pub created: Option<i64>,
    pub expires: Option<i64>,
    pub covered_components: Option<Vec<String>>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProviderExactHttpRequest {
    pub key: ProviderKeySelector,
    pub url: String,
    pub method: String,
    pub headers: Vec<ProviderHttpHeader>,
    pub body: Option<Vec<u8>>,
    #[serde(default)]
    pub options: ProviderHttpSigningOptions,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProviderPreparedHttpSignature {
    pub binding_digest: String,
    pub kid: String,
    pub header_patch: Vec<ProviderHttpHeader>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ProviderKeyAgreementRequest {
    pub key: ProviderKeySelector,
    pub peer_public: [u8; 32],
}

pub struct ProviderSharedSecret {
    bytes: Zeroizing<[u8; 32]>,
}

impl ProviderSharedSecret {
    pub fn new(bytes: [u8; 32]) -> Self {
        Self {
            bytes: Zeroizing::new(bytes),
        }
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.bytes
    }
}

#[async_trait]
pub trait IdentityCustody: Send + Sync {
    async fn store_info(&self) -> ProviderResult<ProviderStoreInfo>;

    async fn list_identities(&self) -> ProviderResult<Vec<ProviderIdentityDescriptor>>;

    async fn open_identity(
        &self,
        identity: &ProviderIdentityRef,
    ) -> ProviderResult<Arc<dyn IdentitySession>>;

    async fn recover(&self) -> ProviderResult<()>;
}

#[async_trait]
pub trait IdentitySession: Send + Sync {
    async fn public_identity(&self) -> ProviderResult<ProviderPublicIdentity>;

    async fn sign(&self, request: ProviderSignRequest) -> ProviderResult<ProviderSignature>;

    async fn sign_origin_proof(
        &self,
        request: ProviderOriginProofRequest,
    ) -> ProviderResult<ProviderSignedOriginProof>;

    async fn prepare_http_signature(
        &self,
        request: ProviderExactHttpRequest,
    ) -> ProviderResult<ProviderPreparedHttpSignature>;

    async fn derive_shared_secret(
        &self,
        request: ProviderKeyAgreementRequest,
    ) -> ProviderResult<ProviderSharedSecret>;

    async fn recover(&self) -> ProviderResult<()>;
}

#[cfg(test)]
mod tests;
