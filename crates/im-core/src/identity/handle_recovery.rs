use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct HandleRecoveryOtpRequest {
    pub identity: super::IdentitySelector,
    pub phone: String,
}

impl std::fmt::Debug for HandleRecoveryOtpRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HandleRecoveryOtpRequest")
            .field("identity", &self.identity)
            .field("phone", &"<redacted>")
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandleRecoveryOtpResult {
    pub full_handle: String,
    pub operation_id: String,
    pub accepted: bool,
    pub retry_after_seconds: u32,
    pub retry_at: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct HandleRecoveryPrepareRequest {
    pub operation_id: String,
    pub phone: String,
    pub code: String,
}

impl std::fmt::Debug for HandleRecoveryPrepareRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HandleRecoveryPrepareRequest")
            .field("operation_id", &self.operation_id)
            .field("phone", &"<redacted>")
            .field("code", &"<redacted>")
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandleRecoveryActivateRequest {
    pub operation_id: String,
    pub user_presence_confirmed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandleRecoveryResumeRequest {
    pub operation_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandleRecoveryDiscardRequest {
    pub operation_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandleRecoveryQuarantineRequest {
    pub operation_id: String,
    pub user_presence_confirmed: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct AuthorizedJoinActivationRequest {
    pub identity: super::IdentitySelector,
    pub phone: String,
    pub code: String,
    pub handle: String,
    pub did: crate::ids::Did,
    pub operation_id: String,
    pub ttl_seconds: Option<u64>,
    pub user_presence_confirmed: bool,
}

impl std::fmt::Debug for AuthorizedJoinActivationRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AuthorizedJoinActivationRequest")
            .field("identity", &self.identity)
            .field("phone", &"<redacted>")
            .field("code", &"<redacted>")
            .field("handle", &self.handle)
            .field("did", &self.did)
            .field("operation_id", &self.operation_id)
            .field("ttl_seconds", &self.ttl_seconds)
            .field("user_presence_confirmed", &self.user_presence_confirmed)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandleRecoveryPhase {
    AwaitingFactor,
    ReadyToCommit,
    RemoteOutcomeUnknown,
    Prepared,
    RemoteCommitPending,
    RemoteCommitted,
    IdentityTransitionPending,
    IdentitySwitched,
    Completed,
    Applied,
    QuarantinedKeyUnavailable,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandleRecoveryErrorCode {
    FactorRetryRequired,
    ResultAbsent,
    OutcomeUnknown,
    LocalKeyUnavailable,
    LocalTransitionPending,
    LocalMigrationUnsupported,
    UnknownEpoch,
    HandleRecoveryNotPrepared,
    HandleRecoveryUserPresenceRequired,
    HandleRecoveryTransitionMismatch,
    HandleRecoveryTransitionChainUnsupported,
    HandleRecoveryRemoteStateChanged,
    HandleRecoveryOutcomeUnknown,
    HandleRecoveryLocalStateUnavailable,
    HandleRecoveryBlocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandleRecoveryTransitionSourceKind {
    Initiator,
    JoinedDevice,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandleRecoveryImpact {
    pub local_ordinary_data_will_migrate: bool,
    pub other_devices_must_rejoin: bool,
    pub unsupported_e2ee_group_count: u32,
    pub unsupported_did_only_group_count: u32,
}

impl HandleRecoveryErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FactorRetryRequired => "factor_retry_required",
            Self::ResultAbsent => "result_absent",
            Self::OutcomeUnknown => "outcome_unknown",
            Self::LocalKeyUnavailable => "local_key_unavailable",
            Self::LocalTransitionPending => "local_transition_pending",
            Self::LocalMigrationUnsupported => "local_migration_unsupported",
            Self::UnknownEpoch => "unknown_epoch",
            Self::HandleRecoveryNotPrepared => "handle_recovery_not_prepared",
            Self::HandleRecoveryUserPresenceRequired => "handle_recovery_user_presence_required",
            Self::HandleRecoveryTransitionMismatch => "handle_recovery_transition_mismatch",
            Self::HandleRecoveryTransitionChainUnsupported => {
                "handle_recovery_transition_chain_unsupported"
            }
            Self::HandleRecoveryRemoteStateChanged => "handle_recovery_remote_state_changed",
            Self::HandleRecoveryOutcomeUnknown => "handle_recovery_outcome_unknown",
            Self::HandleRecoveryLocalStateUnavailable => "handle_recovery_local_state_unavailable",
            Self::HandleRecoveryBlocked => "handle_recovery_blocked",
        }
    }

    /// Closed V4.0 retryability projection consumed by every public facade.
    pub const fn retryable(self) -> bool {
        matches!(
            self,
            Self::FactorRetryRequired
                | Self::ResultAbsent
                | Self::OutcomeUnknown
                | Self::LocalTransitionPending
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandleRecoveryResetReference {
    pub account_user_id: String,
    pub owner_identity_id: String,
    pub previous_did: crate::ids::Did,
    pub current_did: crate::ids::Did,
    pub binding_generation: String,
    pub handle: String,
    pub source_kind: HandleRecoveryTransitionSourceKind,
    pub source_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandleRecoveryProgress {
    pub operation_id: String,
    pub owner_identity_id: crate::ids::IdentityId,
    pub account_user_id: Option<String>,
    pub full_handle: String,
    pub local_previous_did: Option<crate::ids::Did>,
    pub current_did: crate::ids::Did,
    pub binding_generation: Option<String>,
    pub state_root_fingerprint: Option<String>,
    pub phase: HandleRecoveryPhase,
    pub impact: HandleRecoveryImpact,
    pub reset_reference: Option<HandleRecoveryResetReference>,
    pub blocked_code: Option<HandleRecoveryErrorCode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandleRecoveryOperationLifecycle {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandleRecoveryKeyState {
    Available,
    TemporarilyLocked,
    PermanentlyUnavailable,
    DestroyedPreAttempt,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandleRecoveryOperationSummary {
    pub operation_id: String,
    pub owner_identity_id: crate::ids::IdentityId,
    pub account_user_id: Option<String>,
    pub full_handle: String,
    pub lifecycle_class: HandleRecoveryOperationLifecycle,
    pub commit_attempted: bool,
    pub key_state: HandleRecoveryKeyState,
    pub intent_hash: Option<String>,
    pub state_root_fingerprint: Option<String>,
    pub superseded_by_operation_id: Option<String>,
    pub last_error_code: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandleRecoveryAccountEpochReceipt {
    pub receipt_schema_version: String,
    pub source_kind: HandleRecoveryTransitionSourceKind,
    pub source_id: String,
    pub account_user_id: String,
    pub owner_identity_id: crate::ids::IdentityId,
    pub full_handle: String,
    pub local_previous_did: crate::ids::Did,
    pub current_did: crate::ids::Did,
    pub binding_generation: String,
    pub current_device_id: crate::ids::ProtocolDeviceId,
    pub device_auth_generation: u64,
    pub registry_version: u64,
    pub state_root_fingerprint: String,
    pub applied_at: String,
    pub metadata_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorizedJoinActivationProgress {
    pub join: super::DeviceJoinProgress,
    pub reset_reference: Option<HandleRecoveryResetReference>,
}

pub struct HandleRecoveryService<'a> {
    core: &'a crate::core::ImCore,
}

impl<'a> HandleRecoveryService<'a> {
    pub(crate) fn new(core: &'a crate::core::ImCore) -> Self {
        Self { core }
    }

    pub async fn request_handle_recovery_otp(
        &self,
        request: HandleRecoveryOtpRequest,
    ) -> crate::ImResult<HandleRecoveryOtpResult> {
        crate::internal::identity_handle_recovery_runtime::request_otp(self.core, request).await
    }

    pub async fn prepare_handle_recovery(
        &self,
        request: HandleRecoveryPrepareRequest,
    ) -> crate::ImResult<HandleRecoveryProgress> {
        crate::internal::identity_handle_recovery_runtime::prepare(self.core, request).await
    }

    pub async fn activate_handle_recovery(
        &self,
        request: HandleRecoveryActivateRequest,
    ) -> crate::ImResult<HandleRecoveryProgress> {
        crate::internal::identity_handle_recovery_runtime::activate(self.core, request).await
    }

    pub async fn resume_handle_recovery(
        &self,
        request: HandleRecoveryResumeRequest,
    ) -> crate::ImResult<HandleRecoveryProgress> {
        crate::internal::identity_handle_recovery_runtime::resume(self.core, request).await
    }

    pub fn handle_recovery_status(
        &self,
        operation_id: &str,
    ) -> crate::ImResult<HandleRecoveryProgress> {
        crate::internal::identity_handle_recovery_runtime::status(self.core, operation_id)
    }

    pub async fn list_handle_recovery_operations(
        &self,
        identity: super::IdentitySelector,
    ) -> crate::ImResult<Vec<HandleRecoveryOperationSummary>> {
        crate::internal::identity_handle_recovery_runtime::list_operations(self.core, identity)
            .await
    }

    pub fn discard_handle_recovery_pre_attempt(
        &self,
        request: HandleRecoveryDiscardRequest,
    ) -> crate::ImResult<HandleRecoveryOperationSummary> {
        crate::internal::identity_handle_recovery_runtime::discard_pre_attempt(self.core, request)
    }

    pub fn quarantine_handle_recovery_key_unavailable(
        &self,
        request: HandleRecoveryQuarantineRequest,
    ) -> crate::ImResult<HandleRecoveryOperationSummary> {
        crate::internal::identity_handle_recovery_runtime::quarantine_key_unavailable(
            self.core, request,
        )
    }

    pub async fn authorized_handle_recovery_receipt(
        &self,
        identity: super::IdentitySelector,
    ) -> crate::ImResult<Option<HandleRecoveryAccountEpochReceipt>> {
        crate::internal::identity_handle_recovery_runtime::authorized_receipt(self.core, identity)
            .await
    }

    pub async fn activate_authorized_join(
        &self,
        request: AuthorizedJoinActivationRequest,
    ) -> crate::ImResult<AuthorizedJoinActivationProgress> {
        crate::internal::identity_handle_recovery_runtime::activate_authorized_join(
            self.core, request,
        )
        .await
    }

    pub async fn resume_authorized_join_activation(
        &self,
        join_session_id: &str,
    ) -> crate::ImResult<AuthorizedJoinActivationProgress> {
        crate::internal::identity_handle_recovery_runtime::resume_authorized_join_activation(
            self.core,
            join_session_id,
        )
        .await
    }
}
