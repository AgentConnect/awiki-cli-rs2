//! Environment-level, phone-bound PIN backup and original-root recovery.
//! Secret inputs and network credentials never appear in public progress DTOs.
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

macro_rules! handle {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);
    };
}
handle!(PhoneChallengeHandle);
handle!(DiscoverySessionHandle);
handle!(PhoneSessionHandle);
handle!(PinRecoveryOperationId);
handle!(RecoveryTargetId);

/// Exactly four ASCII bytes, including leading zeroes. Never serialized or logged.
pub struct SecretPin(Zeroizing<Vec<u8>>);
pub struct SecretOtp(Zeroizing<Vec<u8>>);
impl SecretPin {
    pub fn new(value: impl Into<Vec<u8>>) -> crate::ImResult<Self> {
        let value = Zeroizing::new(value.into());
        if value.len() != 4 || !value.iter().all(u8::is_ascii_digit) {
            return Err(crate::ImError::invalid_input(
                Some("pin".into()),
                "PIN must contain exactly four ASCII digits",
            ));
        }
        Ok(Self(value))
    }
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.0
    }
}
impl SecretOtp {
    pub fn new(value: impl Into<Vec<u8>>) -> crate::ImResult<Self> {
        let value = Zeroizing::new(value.into());
        if value.len() != 6 || !value.iter().all(u8::is_ascii_digit) {
            return Err(crate::ImError::invalid_input(
                Some("code".into()),
                "OTP must contain exactly six ASCII digits",
            ));
        }
        Ok(Self(value))
    }
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.0
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserPresence {
    confirmed: bool,
}
impl UserPresence {
    pub fn confirmed() -> Self {
        Self { confirmed: true }
    }
    pub(crate) fn is_confirmed(self) -> bool {
        self.confirmed
    }
}

/// Installed by the trusted Host. Keys are public Ed25519 raw bytes, keyed by kid.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PinRecoveryTenantConfig {
    pub tenant_id: String,
    pub deployment_id: String,
    pub control_base_url: String,
    pub config_signing_keys: std::collections::BTreeMap<String, String>,
}
pub struct PhoneChallengeRequest {
    pub tenant_id: String,
    pub phone: Zeroizing<String>,
    pub resume_operation_id: Option<PinRecoveryOperationId>,
}
pub struct PhoneVerifyRequest {
    pub challenge: PhoneChallengeHandle,
    pub code: SecretOtp,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhoneChallengeSummary {
    pub challenge_handle: PhoneChallengeHandle,
    pub expires_at: String,
    pub resend_after_seconds: u32,
    pub masked_phone: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoverySessionSummary {
    pub discovery_handle: DiscoverySessionHandle,
    pub tenant_id: String,
    pub expires_at: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhoneSessionSummary {
    pub session_handle: PhoneSessionHandle,
    pub tenant_id: String,
    pub target: PinRecoveryTarget,
    pub expires_at: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinRecoveryTarget {
    pub target_id: RecoveryTargetId,
    pub full_handle: String,
    pub did: String,
    pub binding_generation: String,
    pub active_epoch: Option<u64>,
    pub can_enable: bool,
    pub can_recover: bool,
    pub unavailable_reason: Option<String>,
}
#[derive(Debug, Clone)]
pub struct EnablePinRecoveryRequest {
    pub session: PhoneSessionHandle,
    pub identity: crate::identity::IdentitySelector,
    pub user_presence: UserPresence,
}
#[derive(Debug, Clone)]
pub struct BeginPinRecoveryRequest {
    pub session: PhoneSessionHandle,
    pub device_label: String,
}
#[derive(Debug, Clone)]
pub struct ConfirmRecoveredDeviceRequest {
    pub operation_id: PinRecoveryOperationId,
    pub user_presence: UserPresence,
}
#[derive(Debug, Clone)]
pub struct ResumePinRecoveryRequest {
    pub operation_id: PinRecoveryOperationId,
    pub session: Option<PhoneSessionHandle>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PinRecoveryKind {
    Enable,
    Recover,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PinRecoveryPhase {
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PinRecoveryAction {
    VerifyPhone,
    SubmitPin,
    ConfirmDevice,
    Resume,
    Cancel,
    RetryLocalActivation,
    Inspect,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PinRecoverySecureReadiness {
    NotStarted,
    PrekeysPending,
    SecurePending,
    Ready,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RealmProgress {
    pub realm_id: String,
    pub phase: String,
    pub error_code: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinRecoveryError {
    pub code: String,
    pub retryable: bool,
    pub retry_after_seconds: Option<u32>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinRecoveryProgress {
    pub operation_id: PinRecoveryOperationId,
    pub kind: PinRecoveryKind,
    pub phase: PinRecoveryPhase,
    pub allowed_actions: Vec<PinRecoveryAction>,
    pub target: PinRecoveryTarget,
    pub node_progress: Vec<RealmProgress>,
    pub attempts_used: u16,
    pub attempts_limit: u16,
    pub needs_phone_verification: bool,
    pub needs_user_confirmation: bool,
    pub recovered_identity: Option<crate::identity::IdentitySelector>,
    pub new_device_id: Option<String>,
    pub device_label: Option<String>,
    pub secure_readiness: PinRecoverySecureReadiness,
    pub cleanup_remaining: u8,
    pub error: Option<PinRecoveryError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PinRecoveryBackupState {
    Unknown,
    NotEnabled,
    Pending,
    Active,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinRecoveryBackupStatus {
    pub did: String,
    pub full_handle: Option<String>,
    pub binding_generation: Option<String>,
    pub state: PinRecoveryBackupState,
    pub active_epoch: Option<u64>,
    pub error: Option<PinRecoveryError>,
}

pub struct PinRecoveryService<'a> {
    pub(crate) core: &'a crate::ImCore,
}
impl PinRecoveryService<'_> {
    pub async fn backup_status(
        &self,
        identity: crate::identity::IdentitySelector,
    ) -> crate::ImResult<PinRecoveryBackupStatus> {
        crate::internal::pin_recovery::backup_status::read(self.core, identity).await
    }

    pub async fn request_phone_code(
        &self,
        request: PhoneChallengeRequest,
    ) -> crate::ImResult<PhoneChallengeSummary> {
        crate::internal::pin_recovery::request_phone_code(self.core, request).await
    }
    pub async fn verify_phone_code(
        &self,
        request: PhoneVerifyRequest,
    ) -> crate::ImResult<DiscoverySessionSummary> {
        crate::internal::pin_recovery::verify_phone_code(self.core, request).await
    }
    pub async fn list_targets(
        &self,
        discovery: DiscoverySessionHandle,
    ) -> crate::ImResult<Vec<PinRecoveryTarget>> {
        crate::internal::pin_recovery::list_targets(self.core, discovery).await
    }
    pub async fn select_target(
        &self,
        discovery: DiscoverySessionHandle,
        target: RecoveryTargetId,
    ) -> crate::ImResult<PhoneSessionSummary> {
        crate::internal::pin_recovery::select_target(self.core, discovery, target).await
    }
    pub async fn begin_enable(
        &self,
        request: EnablePinRecoveryRequest,
    ) -> crate::ImResult<PinRecoveryProgress> {
        crate::internal::pin_recovery::begin_enable(self.core, request).await
    }
    pub async fn begin_recover(
        &self,
        request: BeginPinRecoveryRequest,
    ) -> crate::ImResult<PinRecoveryProgress> {
        crate::internal::pin_recovery::begin_recover(self.core, request).await
    }
    pub async fn submit_pin(
        &self,
        operation_id: PinRecoveryOperationId,
        pin: SecretPin,
    ) -> crate::ImResult<PinRecoveryProgress> {
        crate::internal::pin_recovery::submit_pin(self.core, operation_id, pin).await
    }
    pub async fn confirm_device(
        &self,
        request: ConfirmRecoveredDeviceRequest,
    ) -> crate::ImResult<PinRecoveryProgress> {
        crate::internal::pin_recovery::confirm_device(self.core, request).await
    }
    pub async fn list_pending(&self) -> crate::ImResult<Vec<PinRecoveryProgress>> {
        crate::internal::pin_recovery::list_pending(self.core).await
    }
    pub async fn status(
        &self,
        operation_id: PinRecoveryOperationId,
    ) -> crate::ImResult<PinRecoveryProgress> {
        crate::internal::pin_recovery::status(self.core, operation_id).await
    }
    pub async fn resume(
        &self,
        request: ResumePinRecoveryRequest,
    ) -> crate::ImResult<PinRecoveryProgress> {
        crate::internal::pin_recovery::resume(self.core, request).await
    }
    pub async fn cancel(
        &self,
        operation_id: PinRecoveryOperationId,
    ) -> crate::ImResult<PinRecoveryProgress> {
        crate::internal::pin_recovery::cancel(self.core, operation_id).await
    }
}
