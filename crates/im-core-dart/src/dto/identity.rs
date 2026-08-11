#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DartIdentitySelector {
    Default,
    Id { id: String },
    Did { did: String },
    Handle { handle: String },
    LocalAlias { alias: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartIdentitySummary {
    pub id: String,
    pub did: String,
    pub handle: Option<String>,
    pub display_name: Option<String>,
    pub local_alias: Option<String>,
    pub device_id: Option<String>,
    pub is_default: bool,
    pub ready_for_auth: bool,
    pub ready_for_messaging: bool,
    pub missing: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartActiveSyncAccountBinding {
    pub owner_identity_id: String,
    pub account_id: String,
    pub current_did: String,
    pub protocol_device_id: String,
    pub identity_generation: String,
    pub device_auth_generation: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartLegacyRegistryEpochAdoptionAuthority {
    pub owner_identity_id: String,
    pub account_user_id: String,
    pub current_did: String,
    pub binding_generation: String,
    pub protocol_device_id: String,
    pub device_auth_generation: String,
    pub provenance_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DartHandleRecoveryPhase {
    AwaitingFactor,
    ReadyToCommit,
    RemoteOutcomeUnknown,
    RemoteCommitted,
    IdentityTransitionPending,
    Applied,
    QuarantinedKeyUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DartHandleRecoveryErrorCode {
    FactorRetryRequired,
    ResultAbsent,
    OutcomeUnknown,
    LocalKeyUnavailable,
    LocalTransitionPending,
    LocalMigrationUnsupported,
    UnknownEpoch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DartHandleRecoveryTransitionSourceKind {
    Initiator,
    JoinedDevice,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartHandleRecoveryImpact {
    pub local_ordinary_data_will_migrate: bool,
    pub other_devices_must_rejoin: bool,
    pub unsupported_e2ee_group_count: u32,
    pub unsupported_did_only_group_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartHandleRecoveryResetReference {
    pub account_user_id: String,
    pub owner_identity_id: String,
    pub previous_did: String,
    pub current_did: String,
    pub binding_generation: String,
    pub handle: String,
    pub source_kind: DartHandleRecoveryTransitionSourceKind,
    pub source_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartHandleRecoveryProgress {
    pub operation_id: String,
    pub owner_identity_id: String,
    pub account_user_id: Option<String>,
    pub full_handle: String,
    pub local_previous_did: Option<String>,
    pub current_did: String,
    pub binding_generation: Option<String>,
    pub state_root_fingerprint: Option<String>,
    pub phase: DartHandleRecoveryPhase,
    pub impact: DartHandleRecoveryImpact,
    pub reset_reference: Option<DartHandleRecoveryResetReference>,
    pub failure_code: Option<DartHandleRecoveryErrorCode>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DartAuthorizedJoinActivationProgress {
    pub join: DartDeviceJoinProgress,
    pub reset_reference: Option<DartHandleRecoveryResetReference>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartHandleRecoveryOtpResult {
    pub owner_identity_id: String,
    pub full_handle: String,
    pub operation_id: String,
    pub accepted: bool,
    pub retry_after_seconds: u32,
    pub retry_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DartHandleRecoveryOperationLifecycle {
    PreCommit,
    RemoteUnresolved,
    RemoteCommitted,
    LocalTransitionPending,
    Applied,
    DiscardedPreAttempt,
    QuarantinedKeyUnavailable,
    SupersededByStateChange,
    FailedTerminal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DartHandleRecoveryKeyState {
    Available,
    TemporarilyLocked,
    PermanentlyUnavailable,
    DestroyedPreAttempt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartHandleRecoveryOperationSummary {
    pub operation_id: String,
    pub owner_identity_id: String,
    pub account_user_id: Option<String>,
    pub full_handle: String,
    pub lifecycle_class: DartHandleRecoveryOperationLifecycle,
    pub commit_attempted: bool,
    pub key_state: DartHandleRecoveryKeyState,
    pub intent_hash: Option<String>,
    pub state_root_fingerprint: Option<String>,
    pub superseded_by_operation_id: Option<String>,
    pub last_error_code: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartHandleRecoveryAccountEpochReceipt {
    pub receipt_schema_version: String,
    pub source_kind: DartHandleRecoveryTransitionSourceKind,
    pub source_id: String,
    pub account_user_id: String,
    pub owner_identity_id: String,
    pub full_handle: String,
    pub local_previous_did: String,
    pub current_did: String,
    pub binding_generation: String,
    pub current_device_id: String,
    pub device_auth_generation: u64,
    pub registry_version: u64,
    pub state_root_fingerprint: String,
    pub applied_at: String,
    pub metadata_json: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DartIdentityDeviceMode {
    Legacy,
    VNext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DartIdentityDeviceRole {
    Member,
    Admin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DartIdentityDeviceReadiness {
    Legacy,
    MemberReady,
    AdminAwaitingRoot,
    AdminReady,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartIdentityDeviceSummary {
    pub identity: DartIdentitySummary,
    pub mode: DartIdentityDeviceMode,
    pub protocol_device_id: Option<String>,
    pub role: Option<DartIdentityDeviceRole>,
    pub signing_key_id: Option<String>,
    pub e2ee_key_id: Option<String>,
    pub readiness: DartIdentityDeviceReadiness,
    pub blocked_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DartDeviceJoinSide {
    NewDevice,
    Admin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DartDeviceJoinPhase {
    Pending,
    ChallengePrepared,
    ResponsePrepared,
    ResponseVerified,
    ApprovalPrepared,
    Authorized,
    Cancelled,
    Expired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DartDeviceJoinRole {
    Member,
    Admin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DartDeviceJoinAuthorizationStatus {
    Active,
    Revoked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DartDeviceJoinRemoteState {
    Pending,
    ChallengeSent,
    ResponseVerified,
    Consumed,
    Cancelled,
    Rejected,
    Expired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartDeviceJoinSessionSummary {
    pub join_session_id: String,
    pub did: String,
    pub protocol_device_id: String,
    pub side: DartDeviceJoinSide,
    pub phase: DartDeviceJoinPhase,
    pub expires_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartDeviceJoinAuthorizedDeviceSummary {
    pub protocol_device_id: String,
    pub signing_key_id: String,
    pub e2ee_key_id: String,
    pub status: DartDeviceJoinAuthorizationStatus,
    pub role: DartDeviceJoinRole,
    pub management_ready: bool,
    pub is_current: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartDeviceRegistryAuthorizedDeviceSummary {
    pub protocol_device_id: String,
    pub signing_key_id: String,
    pub e2ee_key_id: String,
    pub status: DartDeviceJoinAuthorizationStatus,
    pub role: DartDeviceJoinRole,
    pub management_ready: bool,
    pub is_current: bool,
    pub auth_generation: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartDeviceJoinRequestNotice {
    pub event_id: String,
    pub join_session_id: String,
    pub did: String,
    pub protocol_device_id: String,
    pub candidate_key_fingerprint: String,
    pub issued_at: String,
    pub expires_at: String,
    pub state: DartDeviceJoinRemoteState,
    pub claimed_by_current_device: bool,
    pub can_start_verification: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartDeviceJoinRegistrySnapshot {
    pub did: String,
    pub registry_version: String,
    pub devices: Vec<DartDeviceRegistryAuthorizedDeviceSummary>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DartDeviceJoinRejectReason {
    UserRejected,
    SasMismatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DartDeviceRevokeStatus {
    Revoked,
}

/// Safe host projection. Internal checkpoints, documents, proofs and
/// generations remain inside Core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartDeviceRevokeResult {
    pub did: String,
    pub target_device_id: String,
    pub status: DartDeviceRevokeStatus,
}

/// Opaque, short-lived authorization returned by root-transfer preparation.
///
/// Its debug projection must never reveal the handle.
#[derive(Clone, PartialEq, Eq)]
pub struct DartRootKeyTransferPreparation {
    pub authorization_handle: String,
    pub recipient: DartRootKeyTransferRecipientSummary,
    pub expires_at: String,
}

impl std::fmt::Debug for DartRootKeyTransferPreparation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DartRootKeyTransferPreparation")
            .field("authorization_handle", &"<redacted-authorization-handle>")
            .field("recipient", &self.recipient)
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// Secret-free exact-device summary verified by Core during preparation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartRootKeyTransferRecipientSummary {
    pub did: String,
    pub device_id: String,
    pub signing_key_id: String,
    pub e2ee_key_id: String,
    pub registry_version: u64,
}

/// Safe host projection for one accepted root-key control delivery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartRootKeyTransferSendResult {
    pub did: String,
    pub sender_device_id: String,
    pub recipient_device_id: String,
    pub message_id: String,
    pub accepted_at: String,
}

/// Closed, secret-free public error returned by root-transfer host APIs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartRootKeyTransferError {
    pub code: String,
    pub retryable: bool,
}

#[derive(Clone, PartialEq, Eq)]
pub struct DartDeviceJoinProgress {
    pub session: DartDeviceJoinSessionSummary,
    pub remote_state: DartDeviceJoinRemoteState,
    pub sas: Option<String>,
    pub authorized_device: Option<DartDeviceJoinAuthorizedDeviceSummary>,
}

impl std::fmt::Debug for DartDeviceJoinProgress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DartDeviceJoinProgress")
            .field("session", &self.session)
            .field("remote_state", &self.remote_state)
            .field("sas", &self.sas.as_ref().map(|_| "<redacted-sas>"))
            .field("authorized_device", &self.authorized_device)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct DartDeviceJoinApprovalPrompt {
    pub approval_handle: String,
    pub join_session_id: String,
    pub sas: String,
    pub expires_at: String,
}

impl std::fmt::Debug for DartDeviceJoinApprovalPrompt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DartDeviceJoinApprovalPrompt")
            .field("approval_handle", &"<redacted-approval-handle>")
            .field("join_session_id", &self.join_session_id)
            .field("sas", &"<redacted-sas>")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DartIdentitySecretStorageBackend {
    FileCompat,
    Vault,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartIdentityVaultStatus {
    pub identity: DartIdentitySummary,
    pub storage_policy: crate::dto::config::DartIdentitySecretStoragePolicy,
    pub selected_backend: DartIdentitySecretStorageBackend,
    pub vault_available: bool,
    pub vault_metadata_present: bool,
    pub vault_metadata_verified: bool,
    pub workspace_id: Option<String>,
    pub device_id: Option<String>,
    pub plaintext_compat_retained: Option<bool>,
    pub missing: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartIdentityVaultMigrationReport {
    pub identity: DartIdentitySummary,
    pub status: DartIdentityVaultStatus,
    pub migrated: bool,
    pub verified: bool,
    pub plaintext_compat_retained: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartIdentityVaultVerificationReport {
    pub identity: DartIdentitySummary,
    pub status: DartIdentityVaultStatus,
    pub verified: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DartLegacyUpgradeStatus {
    Idle,
    Running,
    RetryRequired { identity_id: String, code: String },
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartInitialProfile {
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartDaemonSubkeyPrivatePackage {
    pub schema: String,
    pub user_did: String,
    pub verification_method: String,
    pub key_type: String,
    pub key_algorithm: Option<String>,
    pub public_key_multibase: String,
    pub private_key_encoding: String,
    pub private_key_pem: String,
    pub private_key_multibase: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartDaemonSubkeyAuthorizationRevokeResult {
    pub user_did: String,
    pub verification_method: String,
    pub updated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartDefaultIdentityChange {
    pub previous: Option<DartIdentitySummary>,
    pub next: DartIdentitySummary,
    pub requires_default_identity_write: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartDeleteLocalIdentityResult {
    pub deleted: DartIdentitySummary,
    pub was_default: bool,
    pub next_default: Option<DartIdentitySummary>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartHandleRegistrationResult {
    pub identity: Option<DartIdentitySummary>,
    pub account_id: Option<String>,
    pub handle: String,
    pub method: String,
    pub state: String,
    pub join_required: Option<DartHandleRegistrationJoinRequiredPreparation>,
    pub default_identity_change: Option<DartDefaultIdentityChange>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DartHandleRegistrationJoinMode {
    Ordinary,
    HandleRecoveryRebind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartHandleRegistrationJoinRequiredPreparation {
    pub preparation_id: String,
    pub mode: DartHandleRegistrationJoinMode,
    pub requires_user_presence: bool,
    pub expected_did: String,
    pub full_handle: String,
}
