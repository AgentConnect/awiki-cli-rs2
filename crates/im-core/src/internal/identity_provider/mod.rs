mod direct;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;
use std::sync::Arc;
use zeroize::Zeroizing;

pub(crate) use direct::DirectAnpIdentitySession;

pub(crate) const IDENTITY_PROVIDER_PROTOCOL: &str = "anp-identity-provider-ts/1";
pub(crate) const CAP_STORE_READ: &str = "store.read";
pub(crate) const CAP_IDENTITY_SIGN: &str = "identity.sign";
pub(crate) const CAP_ORIGIN_PROOF: &str = "identity.origin-proof";
pub(crate) const CAP_HTTP_SIGN: &str = "host.http-sign";
pub(crate) const CAP_KEY_AGREEMENT: &str = "host.key-agreement";

pub(crate) type ProviderResult<T> = Result<T, IdentityProviderError>;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum IdentityProviderErrorCode {
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
pub(crate) struct IdentityProviderError {
    pub(crate) code: IdentityProviderErrorCode,
    pub(crate) retryable: bool,
}

impl IdentityProviderError {
    pub(crate) fn new(code: IdentityProviderErrorCode, retryable: bool) -> Self {
        Self { code, retryable }
    }
}

impl fmt::Display for IdentityProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "identity provider failed: {:?}", self.code)
    }
}

impl std::error::Error for IdentityProviderError {}

pub(crate) fn map_provider_error(error: IdentityProviderError) -> crate::ImError {
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProviderIdentityRef {
    pub(crate) store_id: String,
    pub(crate) identity_id: String,
    pub(crate) did: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProviderIdentityState {
    Enrolling,
    Active,
    Revoked,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProviderKeyAlgorithm {
    Ed25519,
    X25519,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProviderKeyPurpose {
    RootControl,
    Authentication,
    DeviceAssertion,
    ApplicationAssertion,
    KeyAgreement,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProviderPublicKey {
    pub(crate) kid: String,
    pub(crate) algorithm: ProviderKeyAlgorithm,
    pub(crate) purposes: Vec<ProviderKeyPurpose>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProviderPublicIdentity {
    pub(crate) reference: ProviderIdentityRef,
    pub(crate) state: ProviderIdentityState,
    pub(crate) revision: u64,
    pub(crate) document: Value,
    pub(crate) active_keys: Vec<ProviderPublicKey>,
    pub(crate) did_wba: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProviderStoreInfo {
    pub(crate) store_id: String,
    pub(crate) schema_compatible: bool,
    pub(crate) identity_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProviderIdentityDescriptor {
    pub(crate) reference: ProviderIdentityRef,
    pub(crate) state: ProviderIdentityState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProviderKeySelector {
    Default,
    Kid(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "purpose", rename_all = "snake_case")]
pub(crate) enum ProviderSigningPurpose {
    Authentication,
    DeviceAssertion,
    ApplicationAssertion { domain: String },
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProviderSignRequest {
    pub(crate) purpose: ProviderSigningPurpose,
    pub(crate) key: ProviderKeySelector,
    pub(crate) payload: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProviderSignature {
    pub(crate) kid: String,
    pub(crate) algorithm: ProviderKeyAlgorithm,
    pub(crate) bytes: Vec<u8>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProviderOriginProofOptions {
    pub(crate) created: Option<i64>,
    pub(crate) expires: Option<i64>,
    pub(crate) nonce: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProviderOriginProofRequest {
    pub(crate) method: String,
    pub(crate) meta: Value,
    pub(crate) body: Value,
    pub(crate) key: ProviderKeySelector,
    #[serde(default)]
    pub(crate) options: ProviderOriginProofOptions,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct ProviderSignedOriginProof {
    pub(crate) content_digest: String,
    pub(crate) signature_input: String,
    pub(crate) signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProviderHttpHeader {
    pub(crate) name: String,
    pub(crate) value: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProviderHttpSigningOptions {
    pub(crate) nonce: Option<String>,
    pub(crate) created: Option<i64>,
    pub(crate) expires: Option<i64>,
    pub(crate) covered_components: Option<Vec<String>>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProviderExactHttpRequest {
    pub(crate) key: ProviderKeySelector,
    pub(crate) url: String,
    pub(crate) method: String,
    pub(crate) headers: Vec<ProviderHttpHeader>,
    pub(crate) body: Option<Vec<u8>>,
    #[serde(default)]
    pub(crate) options: ProviderHttpSigningOptions,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProviderPreparedHttpSignature {
    pub(crate) binding_digest: String,
    pub(crate) kid: String,
    pub(crate) header_patch: Vec<ProviderHttpHeader>,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ProviderKeyAgreementRequest {
    pub(crate) key: ProviderKeySelector,
    pub(crate) peer_public: [u8; 32],
}

pub(crate) struct ProviderSharedSecret {
    bytes: Zeroizing<[u8; 32]>,
}

impl ProviderSharedSecret {
    pub(crate) fn new(bytes: [u8; 32]) -> Self {
        Self {
            bytes: Zeroizing::new(bytes),
        }
    }

    pub(crate) fn as_bytes(&self) -> &[u8; 32] {
        &self.bytes
    }
}

#[async_trait]
pub(crate) trait IdentityCustody: Send + Sync {
    async fn store_info(&self) -> ProviderResult<ProviderStoreInfo>;

    async fn list_identities(&self) -> ProviderResult<Vec<ProviderIdentityDescriptor>>;

    async fn open_identity(
        &self,
        identity: &ProviderIdentityRef,
    ) -> ProviderResult<Arc<dyn IdentitySession>>;

    async fn recover(&self) -> ProviderResult<()>;
}

#[async_trait]
pub(crate) trait IdentitySession: Send + Sync {
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
