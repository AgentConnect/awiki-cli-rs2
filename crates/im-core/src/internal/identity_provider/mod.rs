#[cfg(feature = "identity-native-anp")]
mod direct;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;
use std::sync::Arc;
use zeroize::Zeroizing;

#[cfg(feature = "identity-native-anp")]
pub(crate) use direct::DirectAnpIdentityCustody;
#[cfg(feature = "identity-native-anp")]
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
    InvalidState,
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
    PendingDocumentChange,
    DocumentChangeNotFound,
    InvalidDocumentChangeState,
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
        Code::InvalidState
        | Code::KeyUnavailable
        | Code::KeyPurposeViolation
        | Code::AmbiguousKey
        | Code::VerificationFailed
        | Code::CapabilityUnavailable
        | Code::RootKeyMismatch => crate::ImError::PermissionDenied,
        Code::PendingDocumentChange
        | Code::DocumentChangeNotFound
        | Code::InvalidDocumentChangeState => crate::ImError::IdentityBindingConflict {
            detail: "identity provider document workflow state changed".to_owned(),
        },
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
#[serde(deny_unknown_fields, rename_all = "camelCase")]
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
pub enum ProviderManagedKeyRole {
    RootControl,
    DeviceSigning,
    RequestSigning,
    E2eeSigning,
    E2eeAgreement,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderManagedKeySpec {
    pub fragment: String,
    pub role: ProviderManagedKeyRole,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderIdentityService {
    pub id: String,
    pub service_type: String,
    pub service_endpoint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_did: Option<String>,
    #[serde(default)]
    pub profiles: Vec<String>,
    #[serde(default)]
    pub security_profiles: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderDeviceManifestEntry {
    pub device_id: String,
    pub signing_key_id: String,
    pub e2ee_key_id: String,
    #[serde(default)]
    pub profiles: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum ProviderIdentityExtension {
    DeviceManifest {
        devices: Vec<ProviderDeviceManifestEntry>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderCreateIdentityRequest {
    pub profile: ProviderDidProfile,
    pub domain: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    pub path_segments: Vec<String>,
    pub capabilities: ProviderCapabilities,
    pub managed_keys: Vec<ProviderManagedKeySpec>,
    #[serde(default)]
    pub services: Vec<ProviderIdentityService>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_description_url: Option<String>,
    #[serde(default)]
    pub extensions: Vec<ProviderIdentityExtension>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderDidProfile {
    E1,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderCapabilities {
    pub did_wba: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderPublicationEvidence {
    pub document_version: u64,
    pub registry_version: u64,
    pub document_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderVerifiedRemoteDocument {
    pub document: Value,
    pub evidence: ProviderPublicationEvidence,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderPreparedDocumentChange {
    pub operation_id: String,
    pub candidate_document: Value,
    pub candidate_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderPublicationAttempt {
    pub operation_id: String,
    pub candidate_digest: String,
    pub publication_generation: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum ProviderPublicationResult {
    Confirmed {
        evidence: ProviderPublicationEvidence,
    },
    RejectedBeforeAcceptance,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ProviderDocumentChangeOutcome {
    ReadyForPublication,
    PublicationUncertain,
    Committed { identity: ProviderPublicIdentity },
    Aborted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderIdentityTransitionRequest {
    pub expected_current_did: String,
    pub operation_id: String,
    pub successor: ProviderIdentityRef,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transition_document: Option<Value>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderTransitionAssurance {
    Verified,
    RecoveryVerified,
    ProviderAsserted,
    Unverified,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderPreparedIdentityTransition {
    pub operation_id: String,
    pub expected_current_did: String,
    pub successor_did: String,
    pub predecessor_document: Value,
    pub successor_document: Value,
    pub predecessor_digest: String,
    pub successor_digest: String,
    pub assurance: ProviderTransitionAssurance,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderIdentityTransitionPublicationAttempt {
    pub operation_id: String,
    pub predecessor_digest: String,
    pub successor_digest: String,
    pub publication_generation: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderIdentityTransitionPublicationEvidence {
    pub predecessor_digest: String,
    pub successor_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum ProviderIdentityTransitionPublicationResult {
    Confirmed {
        evidence: ProviderIdentityTransitionPublicationEvidence,
    },
    RejectedBeforeAcceptance,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(
    tag = "observation",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ProviderIdentityTransitionRemoteObservation {
    RemoteOld {
        current_document: Value,
    },
    Published {
        predecessor_document: Value,
        successor_document: Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "outcome",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ProviderIdentityTransitionOutcome {
    ReadyForPublication,
    PublicationUncertain,
    Committed { current_did: String },
    Aborted,
}

#[async_trait]
pub trait ProviderIdentityTransitionSession: Send + Sync {
    async fn candidate(&self) -> ProviderResult<ProviderPreparedIdentityTransition>;

    async fn begin_publication(
        &self,
    ) -> ProviderResult<ProviderIdentityTransitionPublicationAttempt>;

    async fn complete(
        &self,
        attempt: ProviderIdentityTransitionPublicationAttempt,
        result: ProviderIdentityTransitionPublicationResult,
    ) -> ProviderResult<ProviderIdentityTransitionOutcome>;

    async fn reconcile(
        &self,
        observation: ProviderIdentityTransitionRemoteObservation,
    ) -> ProviderResult<ProviderIdentityTransitionOutcome>;
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderDocumentChangePhase {
    Prepared,
    PublicationInFlight,
    PublicationUncertain,
    Published,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderRootCapability {
    Absent,
    Pending,
    Active,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderDocumentCheckpoint {
    pub document_version: u64,
    pub registry_version: u64,
    pub document_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderHostStatus {
    pub root_capability: ProviderRootCapability,
    pub root_key_fingerprint: String,
    pub checkpoint: Option<ProviderDocumentCheckpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderEnrollmentCapabilities {
    pub did_wba: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderDeviceEnrollmentRequest {
    pub remote: ProviderVerifiedRemoteDocument,
    pub device_id: String,
    pub device_signing_fragment: String,
    pub device_agreement_fragment: String,
    #[serde(default)]
    pub profiles: Vec<String>,
    #[serde(default)]
    pub capabilities: ProviderEnrollmentCapabilities,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderRequestSigningEnrollmentRequest {
    pub remote: ProviderVerifiedRemoteDocument,
    pub fragment: String,
    #[serde(default)]
    pub capabilities: ProviderEnrollmentCapabilities,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderEnrollmentPublicKey {
    pub kid: String,
    pub public_key_multibase: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ProviderEnrollmentProposalKind {
    Device {
        device_id: String,
        signing_key: ProviderEnrollmentPublicKey,
        agreement_key: ProviderEnrollmentPublicKey,
        profiles: Vec<String>,
    },
    RequestSigning {
        signing_key: ProviderEnrollmentPublicKey,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderEnrollmentProposal {
    pub enrollment_id: String,
    pub identity: ProviderIdentityRef,
    pub kind: ProviderEnrollmentProposalKind,
    pub root_key_fingerprint: String,
    pub checkpoint: ProviderDocumentCheckpoint,
}

#[async_trait]
pub trait ProviderEnrollmentSession: Send + Sync {
    async fn proposal(&self) -> ProviderResult<ProviderEnrollmentProposal>;

    async fn sign_device_assertion(&self, payload: Vec<u8>) -> ProviderResult<Vec<u8>>;

    async fn derive_device_shared_secret(
        &self,
        peer_public: [u8; 32],
    ) -> ProviderResult<ProviderSharedSecret>;

    async fn activate(&self, remote: ProviderVerifiedRemoteDocument) -> ProviderResult<()>;

    async fn cancel(&self) -> ProviderResult<()>;
}

#[async_trait]
pub trait ProviderDocumentChangeSession: Send + Sync {
    async fn candidate(&self) -> ProviderResult<ProviderPreparedDocumentChange>;

    async fn host_phase(&self) -> ProviderResult<ProviderDocumentChangePhase>;

    async fn begin_publication(&self) -> ProviderResult<ProviderPublicationAttempt>;

    async fn complete(
        &self,
        attempt: ProviderPublicationAttempt,
        result: ProviderPublicationResult,
    ) -> ProviderResult<ProviderDocumentChangeOutcome>;

    async fn reconcile(
        &self,
        observation: ProviderVerifiedRemoteDocument,
    ) -> ProviderResult<ProviderDocumentChangeOutcome>;
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

#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderObjectProofRequest {
    pub key: ProviderKeySelector,
    pub document: Value,
    pub issuer_did: String,
    pub created: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProviderDocumentProofOptions {
    pub proof_purpose: Option<String>,
    pub proof_type: Option<String>,
    pub cryptosuite: Option<String>,
    pub created: Option<String>,
    pub domain: Option<String>,
    pub challenge: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProviderDocumentProofRequest {
    pub key: ProviderKeySelector,
    pub document: Value,
    #[serde(default)]
    pub options: ProviderDocumentProofOptions,
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

pub struct ProviderLegacyRootExportRequest {
    pub key: ProviderKeySelector,
    pub user_presence_confirmed: bool,
}

pub struct ProviderExportedRoot {
    pkcs8_der: zeroize::Zeroizing<Vec<u8>>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderPrivateKeyEncoding {
    Raw32,
    Pkcs8Der,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderLegacyRootImportEvidence {
    pub transfer_id: String,
    pub source_did: String,
    pub target_did: String,
    pub sender_device_id: String,
    pub recipient_device_id: String,
    pub recipient_agreement_kid: String,
    pub root_kid: String,
    pub checkpoint: ProviderDocumentCheckpoint,
    pub accepted_at: String,
}

pub struct ProviderLegacyRootImportRequest {
    pub identity: ProviderIdentityRef,
    pub evidence: ProviderLegacyRootImportEvidence,
    pub encoding: ProviderPrivateKeyEncoding,
    pub root_key: Zeroizing<Vec<u8>>,
}

pub struct ProviderIdentityMaterialKey {
    pub kid: String,
    pub purpose: ProviderKeyPurpose,
    pub encoding: ProviderPrivateKeyEncoding,
    pub secret: Zeroizing<Vec<u8>>,
}

pub struct ProviderIdentityMaterialImportRequest {
    pub remote: ProviderVerifiedRemoteDocument,
    pub did_wba: bool,
    pub keys: Vec<ProviderIdentityMaterialKey>,
    pub request_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderLegacyRootImportOutcome {
    Pending,
    Active,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderRootTransferContext {
    pub source_did: String,
    pub target_did: String,
    pub sender_device_id: String,
    pub recipient_device_id: String,
    pub recipient_agreement_kid: String,
    pub root_kid: String,
    pub checkpoint: ProviderDocumentCheckpoint,
    pub created_at: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderWrappedRootEnvelope {
    #[serde(rename = "type")]
    pub envelope_type: String,
    pub version: u32,
    pub context: ProviderRootTransferContext,
    pub ephemeral_public_b64u: String,
    pub nonce_b64u: String,
    pub ciphertext_b64u: String,
    pub signature_b64u: String,
}

impl ProviderExportedRoot {
    pub fn new(pkcs8_der: Vec<u8>) -> Self {
        Self {
            pkcs8_der: zeroize::Zeroizing::new(pkcs8_der),
        }
    }

    pub fn as_pkcs8_der(&self) -> &[u8] {
        &self.pkcs8_der
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

    async fn create_identity(
        &self,
        request: ProviderCreateIdentityRequest,
    ) -> ProviderResult<Arc<dyn IdentitySession>>;

    async fn delete_identity(&self, identity: &ProviderIdentityRef) -> ProviderResult<()>;

    async fn prepare_identity_transition(
        &self,
        _request: ProviderIdentityTransitionRequest,
    ) -> ProviderResult<Arc<dyn ProviderIdentityTransitionSession>> {
        Err(IdentityProviderError::new(
            IdentityProviderErrorCode::CapabilityUnavailable,
            false,
        ))
    }

    async fn resume_identity_transition(
        &self,
        _expected_current_did: &str,
    ) -> ProviderResult<Option<Arc<dyn ProviderIdentityTransitionSession>>> {
        Err(IdentityProviderError::new(
            IdentityProviderErrorCode::CapabilityUnavailable,
            false,
        ))
    }

    async fn begin_device_enrollment(
        &self,
        request: ProviderDeviceEnrollmentRequest,
    ) -> ProviderResult<Arc<dyn ProviderEnrollmentSession>>;

    async fn begin_request_signing_enrollment(
        &self,
        request: ProviderRequestSigningEnrollmentRequest,
    ) -> ProviderResult<Arc<dyn ProviderEnrollmentSession>>;

    async fn resume_enrollment(
        &self,
        identity: &ProviderIdentityRef,
    ) -> ProviderResult<Option<Arc<dyn ProviderEnrollmentSession>>>;

    async fn confirm_root_promotion(
        &self,
        identity: &ProviderIdentityRef,
        remote: ProviderVerifiedRemoteDocument,
    ) -> ProviderResult<()>;

    async fn sign_pending_root_object_proof(
        &self,
        identity: &ProviderIdentityRef,
        request: ProviderObjectProofRequest,
    ) -> ProviderResult<Value>;

    async fn sign_document_proof(
        &self,
        _identity: &ProviderIdentityRef,
        _request: ProviderDocumentProofRequest,
    ) -> ProviderResult<Value> {
        Err(IdentityProviderError::new(
            IdentityProviderErrorCode::CapabilityUnavailable,
            false,
        ))
    }

    async fn import_legacy_root(
        &self,
        _request: ProviderLegacyRootImportRequest,
    ) -> ProviderResult<ProviderLegacyRootImportOutcome> {
        Err(IdentityProviderError::new(
            IdentityProviderErrorCode::CapabilityUnavailable,
            false,
        ))
    }

    async fn import_wrapped_root(
        &self,
        _identity: &ProviderIdentityRef,
        _envelope: ProviderWrappedRootEnvelope,
    ) -> ProviderResult<ProviderLegacyRootImportOutcome> {
        Err(IdentityProviderError::new(
            IdentityProviderErrorCode::CapabilityUnavailable,
            false,
        ))
    }

    async fn import_identity_material(
        &self,
        _request: ProviderIdentityMaterialImportRequest,
    ) -> ProviderResult<Arc<dyn IdentitySession>> {
        Err(IdentityProviderError::new(
            IdentityProviderErrorCode::CapabilityUnavailable,
            false,
        ))
    }

    async fn recover(&self) -> ProviderResult<()>;
}

#[async_trait]
pub trait IdentitySession: Send + Sync {
    async fn public_identity(&self) -> ProviderResult<ProviderPublicIdentity>;

    async fn host_status(&self) -> ProviderResult<ProviderHostStatus>;

    async fn sign(&self, request: ProviderSignRequest) -> ProviderResult<ProviderSignature>;

    async fn sign_origin_proof(
        &self,
        request: ProviderOriginProofRequest,
    ) -> ProviderResult<ProviderSignedOriginProof>;

    async fn prepare_http_signature(
        &self,
        request: ProviderExactHttpRequest,
    ) -> ProviderResult<ProviderPreparedHttpSignature>;

    async fn prepare_document_change(
        &self,
        request: Value,
    ) -> ProviderResult<Arc<dyn ProviderDocumentChangeSession>>;

    async fn resume_document_change(
        &self,
    ) -> ProviderResult<Option<Arc<dyn ProviderDocumentChangeSession>>>;

    async fn adopt_verified_document(
        &self,
        remote: ProviderVerifiedRemoteDocument,
    ) -> ProviderResult<ProviderPublicIdentity>;

    async fn derive_shared_secret(
        &self,
        request: ProviderKeyAgreementRequest,
    ) -> ProviderResult<ProviderSharedSecret>;

    async fn export_root_for_legacy_envelope(
        &self,
        _request: ProviderLegacyRootExportRequest,
    ) -> ProviderResult<ProviderExportedRoot> {
        Err(IdentityProviderError::new(
            IdentityProviderErrorCode::CapabilityUnavailable,
            false,
        ))
    }

    async fn recover(&self) -> ProviderResult<()>;
}

#[cfg(test)]
mod tests;
