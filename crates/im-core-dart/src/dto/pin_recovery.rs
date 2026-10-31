//! Secret-free PIN recovery boundary DTOs. PIN/OTP are input arguments only.
use crate::dto::identity::DartIdentitySelector;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartPinRecoveryTenantConfig {
    pub tenant_id: String,
    pub deployment_id: String,
    pub control_base_url: String,
    pub config_signing_keys: std::collections::HashMap<String, String>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartPhoneChallengeSummary {
    pub challenge_handle: String,
    pub expires_at: String,
    pub resend_after_seconds: u32,
    pub masked_phone: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartDiscoverySessionSummary {
    pub discovery_handle: String,
    pub tenant_id: String,
    pub expires_at: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartPhoneSessionSummary {
    pub session_handle: String,
    pub tenant_id: String,
    pub target: DartPinRecoveryTarget,
    pub expires_at: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartPinRecoveryTarget {
    pub target_id: String,
    pub full_handle: String,
    pub did: String,
    pub binding_generation: String,
    pub active_epoch: Option<String>,
    pub can_enable: bool,
    pub can_recover: bool,
    pub unavailable_reason: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DartPinRecoveryKind {
    Enable,
    Recover,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DartPinRecoveryPhase {
    AwaitingPin,
    Authorizing,
    Registering,
    VerifyingBackup,
    RecoveringKey,
    AwaitingDeviceConfirmation,
    Committing,
    CommitUnknown,
    LocalActivationPending,
    ActiveSecurePending,
    CleanupPending,
    Completed,
    Failed,
    Cancelled,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DartPinRecoveryAction {
    VerifyPhone,
    SubmitPin,
    ConfirmDevice,
    Resume,
    Cancel,
    RetryLocalActivation,
    Inspect,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DartPinRecoverySecureReadiness {
    NotStarted,
    PrekeysPending,
    SecurePending,
    Ready,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartRealmProgress {
    pub realm_id: String,
    pub phase: String,
    pub error_code: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartPinRecoveryError {
    pub code: String,
    pub retryable: bool,
    pub retry_after_seconds: Option<u32>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartPinRecoveryProgress {
    pub operation_id: String,
    pub kind: DartPinRecoveryKind,
    pub phase: DartPinRecoveryPhase,
    pub allowed_actions: Vec<DartPinRecoveryAction>,
    pub target: DartPinRecoveryTarget,
    pub node_progress: Vec<DartRealmProgress>,
    pub attempts_used: u16,
    pub attempts_limit: u16,
    pub needs_phone_verification: bool,
    pub needs_user_confirmation: bool,
    pub recovered_identity: Option<DartIdentitySelector>,
    pub new_device_id: Option<String>,
    pub device_label: Option<String>,
    pub secure_readiness: DartPinRecoverySecureReadiness,
    pub cleanup_remaining: u8,
    pub error: Option<DartPinRecoveryError>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DartPinRecoveryBackupState {
    Unknown,
    NotEnabled,
    Pending,
    Active,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartPinRecoveryBackupStatus {
    pub did: String,
    pub full_handle: Option<String>,
    pub binding_generation: Option<String>,
    pub state: DartPinRecoveryBackupState,
    pub active_epoch: Option<String>,
    pub error: Option<DartPinRecoveryError>,
}
