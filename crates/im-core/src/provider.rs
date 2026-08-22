//! Restricted host-side identity provider contract.
//!
//! This module is available only with the `provider-traits` feature. It is an
//! integration SPI for trusted hosts such as the Node External Provider
//! adapter, not the ordinary application-facing identity API.

pub use crate::internal::identity_provider::{
    IdentityCustody, IdentityProviderError, IdentityProviderErrorCode, IdentitySession,
    ProviderCapabilities, ProviderCreateIdentityRequest, ProviderDeviceManifestEntry,
    ProviderDidProfile, ProviderDocumentChangeOutcome, ProviderDocumentChangePhase,
    ProviderDocumentChangeSession, ProviderDocumentCheckpoint, ProviderExactHttpRequest,
    ProviderHostStatus, ProviderHttpHeader, ProviderHttpSigningOptions, ProviderIdentityDescriptor,
    ProviderIdentityExtension, ProviderIdentityRef, ProviderIdentityService, ProviderIdentityState,
    ProviderKeyAgreementRequest, ProviderKeyAlgorithm, ProviderKeyPurpose, ProviderKeySelector,
    ProviderManagedKeyRole, ProviderManagedKeySpec, ProviderOriginProofOptions,
    ProviderOriginProofRequest, ProviderPreparedDocumentChange, ProviderPreparedHttpSignature,
    ProviderPublicIdentity, ProviderPublicKey, ProviderPublicationAttempt,
    ProviderPublicationEvidence, ProviderPublicationResult, ProviderResult, ProviderRootCapability,
    ProviderSharedSecret, ProviderSignRequest, ProviderSignature, ProviderSignedOriginProof,
    ProviderSigningPurpose, ProviderStoreInfo, ProviderVerifiedRemoteDocument, CAP_HTTP_SIGN,
    CAP_IDENTITY_SIGN, CAP_KEY_AGREEMENT, CAP_ORIGIN_PROOF, CAP_STORE_READ,
    IDENTITY_PROVIDER_PROTOCOL,
};
