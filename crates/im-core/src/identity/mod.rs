mod device_revoke;
mod dto;
mod handle_recovery;
mod join;
mod profile;
mod registry;
mod root_key_transfer;
mod service;

pub use self::device_revoke::{
    DeviceRevokeRequest, DeviceRevokeResult, DeviceRevokeService, DeviceRevokeStatus,
};
pub use self::dto::{
    ActiveSyncAccountBinding, AgentIdentityKind, ContactBindingMethod, ContactBindingMethodKind,
    ContactBindingRequest, ContactBindingResult, ContactBindingState,
    DaemonSubkeyAuthorizationRevokeResult, DaemonSubkeyPublicPackage, DaemonSubkeyPublicProposal,
    DefaultIdentityChange, DeleteLocalIdentityResult, HandleRegistrationJoinMode,
    HandleRegistrationJoinRequiredPreparation, HandleRegistrationResult, HandleRegistrationState,
    HostBackedAuthTokenPersistence, HostBackedDeviceIdentityMaterial, HostedIdentityMaterial,
    IdentityCustodyBackend, IdentityCustodyMigrationIdentityReport, IdentityCustodyMigrationPhase,
    IdentityCustodyMigrationReport, IdentityCustodyState, IdentityCustodyStatus,
    IdentityDeviceAuthorizationStatus, IdentityDeviceMode, IdentityDeviceReadiness,
    IdentityDeviceRole, IdentityDeviceSummary, IdentityMissingItem, IdentityReadiness,
    IdentitySecretStorageBackend, IdentitySelector, IdentitySummary, IdentityVaultMigrationReport,
    IdentityVaultStatus, IdentityVaultVerificationReport, InitialProfile,
    LegacyRegistryEpochAdoptionAuthority, LegacyUpgradeStatus, LocalIdentityDeletionTicket,
    Profile, ProfileAttribute, ProfilePatch, RegisterHandleRequest, RegistrationMethod,
    VNextAgentBootstrapMaterial, VNextAgentLegacyUpgradeReconciliation,
    VNextAgentLegacyUpgradeSession, VerificationInput, DAEMON_SUBKEY_PUBLIC_PACKAGE_SCHEMA_V3,
};
pub(crate) use self::dto::{
    DaemonSubkeyPrivatePackage, DAEMON_SUBKEY_PACKAGE_SCHEMA_V1, DAEMON_SUBKEY_PACKAGE_SCHEMA_V2,
    DAEMON_SUBKEY_PRIVATE_KEY_ENCODING_PEM,
};
#[cfg(feature = "service-trusted-registration")]
pub use self::dto::{TrustedServiceRegisterHandleRequest, TrustedServiceRegistrationPreparation};
pub use self::handle_recovery::{
    AuthorizedJoinActivationProgress, AuthorizedJoinActivationRequest,
    BeginPreparedRegistrationDeviceJoinRequest, HandleRecoveryAccountEpochReceipt,
    HandleRecoveryActivateRequest, HandleRecoveryAttestation, HandleRecoveryAttestationRequest,
    HandleRecoveryDiscardRequest, HandleRecoveryErrorCode, HandleRecoveryImpact,
    HandleRecoveryKeyState, HandleRecoveryMetricsSnapshot, HandleRecoveryOperationLifecycle,
    HandleRecoveryOperationSummary, HandleRecoveryOtpRequest, HandleRecoveryOtpResult,
    HandleRecoveryPhase, HandleRecoveryPrepareRequest, HandleRecoveryProgress,
    HandleRecoveryQuarantineRequest, HandleRecoveryResetReference, HandleRecoveryResumeRequest,
    HandleRecoveryService, HandleRecoveryTransitionSourceKind,
};
pub use self::join::{
    DeviceJoinAccountVerificationGrant, DeviceJoinApprovalPrompt, DeviceJoinAuthorizationStatus,
    DeviceJoinAuthorizedDeviceSummary, DeviceJoinBeginRequest, DeviceJoinConfirmApprovalRequest,
    DeviceJoinLocalPhase, DeviceJoinProgress, DeviceJoinRegistrySnapshot, DeviceJoinRejectReason,
    DeviceJoinRemoteState, DeviceJoinRequestNotice, DeviceJoinRole, DeviceJoinService,
    DeviceJoinSessionView, DeviceJoinSide, DeviceRegistryAuthorizedDeviceSummary,
};
pub(crate) use self::join::{
    DeviceJoinAdminPrepareRequest, DeviceJoinAdminPrepareResult, DeviceJoinAdminVerifyRequest,
    DeviceJoinAdminVerifyResult, DeviceJoinChallenge, DeviceJoinChallengeResponse,
    DeviceJoinNewDeviceRespondRequest, DeviceJoinNewDeviceRespondResult, DeviceJoinObjectProof,
    DeviceJoinRequest, DeviceJoinRequestProof, DeviceJoinSessionSummary, DeviceJoinStartRequest,
    DeviceJoinStartResult, DeviceProof, EncryptedJoinChallenge, DEVICE_JOIN_CHALLENGE_ALGORITHM,
    DEVICE_JOIN_LEGACY_DRAFT_PROFILES, DEVICE_JOIN_MAX_CHALLENGE_TTL_SECONDS,
    DEVICE_JOIN_MAX_TTL_SECONDS, DEVICE_JOIN_REQUEST_PROOF_INPUT_TYPE,
    DEVICE_JOIN_REQUEST_PROOF_TYPE, DEVICE_JOIN_REQUEST_TYPE,
    DEVICE_JOIN_RESPONSE_SIGNATURE_INPUT_TYPE, DEVICE_JOIN_VNEXT_PROFILES, DEVICE_PROOF_TYPE,
};
pub use self::registry::IdentityRegistry;
pub use self::root_key_transfer::{
    RootKeyTransferAuthorizationHandle, RootKeyTransferError, RootKeyTransferErrorCode,
    RootKeyTransferPreparation, RootKeyTransferPrepareRequest, RootKeyTransferRecipientSummary,
    RootKeyTransferResult, RootKeyTransferSendRequest, RootKeyTransferSendResult,
    RootKeyTransferService,
};
pub use self::service::IdentityService;
pub use crate::error::DeviceRevokeOutcomeCategory;
