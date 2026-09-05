//! Restricted host-side identity provider contract.
//!
//! This module is available only with the `provider-traits` feature. It is an
//! integration SPI for trusted hosts such as the Node External Provider
//! adapter, not the ordinary application-facing identity API.

pub use crate::internal::identity_provider::{
    IdentityCustody, IdentityProviderError, IdentityProviderErrorCode, IdentitySession,
    ProviderCapabilities, ProviderCreateIdentityRequest, ProviderDeviceEnrollmentRequest,
    ProviderDeviceManifestEntry, ProviderDidProfile, ProviderDocumentChangeOutcome,
    ProviderDocumentChangePhase, ProviderDocumentChangeSession, ProviderDocumentCheckpoint,
    ProviderDocumentProofOptions, ProviderDocumentProofRequest, ProviderEnrollmentCapabilities,
    ProviderEnrollmentProposal, ProviderEnrollmentProposalKind, ProviderEnrollmentPublicKey,
    ProviderEnrollmentSession, ProviderExactHttpRequest, ProviderExportedRoot, ProviderHostStatus,
    ProviderHttpHeader, ProviderHttpSigningOptions, ProviderIdentityDescriptor,
    ProviderIdentityExtension, ProviderIdentityMaterialImportRequest, ProviderIdentityMaterialKey,
    ProviderIdentityRef, ProviderIdentityService, ProviderIdentityState,
    ProviderIdentityTransitionOutcome, ProviderIdentityTransitionPublicationAttempt,
    ProviderIdentityTransitionPublicationEvidence, ProviderIdentityTransitionPublicationResult,
    ProviderIdentityTransitionRemoteObservation, ProviderIdentityTransitionRequest,
    ProviderIdentityTransitionSession, ProviderKeyAgreementRequest, ProviderKeyAlgorithm,
    ProviderKeyPurpose, ProviderKeySelector, ProviderLegacyRootExportRequest,
    ProviderLegacyRootImportEvidence, ProviderLegacyRootImportOutcome,
    ProviderLegacyRootImportRequest, ProviderManagedKeyRole, ProviderManagedKeySpec,
    ProviderObjectProofRequest, ProviderOriginProofOptions, ProviderOriginProofRequest,
    ProviderPreparedDocumentChange, ProviderPreparedHttpSignature,
    ProviderPreparedIdentityTransition, ProviderPrivateKeyEncoding, ProviderPublicIdentity,
    ProviderPublicKey, ProviderPublicationAttempt, ProviderPublicationEvidence,
    ProviderPublicationResult, ProviderRequestSigningEnrollmentRequest, ProviderResult,
    ProviderRootCapability, ProviderRootTransferContext, ProviderSharedSecret, ProviderSignRequest,
    ProviderSignature, ProviderSignedOriginProof, ProviderSigningPurpose, ProviderStoreInfo,
    ProviderTransitionAssurance, ProviderVerifiedRemoteDocument, ProviderWrappedRootEnvelope,
    CAP_HTTP_SIGN, CAP_IDENTITY_SIGN, CAP_KEY_AGREEMENT, CAP_ORIGIN_PROOF, CAP_STORE_READ,
    IDENTITY_PROVIDER_PROTOCOL,
};
