use crate::dto::pin_recovery::*;
use im_core::pin_recovery as core;
impl From<core::PhoneChallengeSummary> for DartPhoneChallengeSummary {
    fn from(value: core::PhoneChallengeSummary) -> Self {
        Self {
            challenge_handle: value.challenge_handle.0,
            expires_at: value.expires_at,
            resend_after_seconds: value.resend_after_seconds,
            masked_phone: value.masked_phone,
        }
    }
}
impl From<core::DiscoverySessionSummary> for DartDiscoverySessionSummary {
    fn from(value: core::DiscoverySessionSummary) -> Self {
        Self {
            discovery_handle: value.discovery_handle.0,
            tenant_id: value.tenant_id,
            expires_at: value.expires_at,
        }
    }
}
impl From<core::PhoneSessionSummary> for DartPhoneSessionSummary {
    fn from(value: core::PhoneSessionSummary) -> Self {
        Self {
            session_handle: value.session_handle.0,
            tenant_id: value.tenant_id,
            target: value.target.into(),
            expires_at: value.expires_at,
        }
    }
}
impl From<core::PinRecoveryTarget> for DartPinRecoveryTarget {
    fn from(value: core::PinRecoveryTarget) -> Self {
        Self {
            target_id: value.target_id.0,
            full_handle: value.full_handle,
            did: value.did,
            binding_generation: value.binding_generation,
            active_epoch: value.active_epoch.map(|v| v.to_string()),
            can_enable: value.can_enable,
            can_recover: value.can_recover,
            unavailable_reason: value.unavailable_reason,
        }
    }
}
impl From<core::PinRecoveryKind> for DartPinRecoveryKind {
    fn from(value: core::PinRecoveryKind) -> Self {
        match value {
            core::PinRecoveryKind::Enable => Self::Enable,
            core::PinRecoveryKind::Recover => Self::Recover,
        }
    }
}
impl From<core::PinRecoveryPhase> for DartPinRecoveryPhase {
    fn from(value: core::PinRecoveryPhase) -> Self {
        match value {
            core::PinRecoveryPhase::AwaitingPin => Self::AwaitingPin,
            core::PinRecoveryPhase::Authorizing => Self::Authorizing,
            core::PinRecoveryPhase::Registering => Self::Registering,
            core::PinRecoveryPhase::VerifyingBackup => Self::VerifyingBackup,
            core::PinRecoveryPhase::RecoveringKey => Self::RecoveringKey,
            core::PinRecoveryPhase::AwaitingDeviceConfirmation => Self::AwaitingDeviceConfirmation,
            core::PinRecoveryPhase::Committing => Self::Committing,
            core::PinRecoveryPhase::CommitUnknown => Self::CommitUnknown,
            core::PinRecoveryPhase::LocalActivationPending => Self::LocalActivationPending,
            core::PinRecoveryPhase::ActiveSecurePending => Self::ActiveSecurePending,
            core::PinRecoveryPhase::CleanupPending => Self::CleanupPending,
            core::PinRecoveryPhase::Completed => Self::Completed,
            core::PinRecoveryPhase::Failed => Self::Failed,
            core::PinRecoveryPhase::Cancelled => Self::Cancelled,
        }
    }
}
impl From<core::PinRecoveryAction> for DartPinRecoveryAction {
    fn from(value: core::PinRecoveryAction) -> Self {
        match value {
            core::PinRecoveryAction::VerifyPhone => Self::VerifyPhone,
            core::PinRecoveryAction::SubmitPin => Self::SubmitPin,
            core::PinRecoveryAction::ConfirmDevice => Self::ConfirmDevice,
            core::PinRecoveryAction::Resume => Self::Resume,
            core::PinRecoveryAction::Cancel => Self::Cancel,
            core::PinRecoveryAction::RetryLocalActivation => Self::RetryLocalActivation,
            core::PinRecoveryAction::Inspect => Self::Inspect,
        }
    }
}
impl From<core::PinRecoverySecureReadiness> for DartPinRecoverySecureReadiness {
    fn from(value: core::PinRecoverySecureReadiness) -> Self {
        match value {
            core::PinRecoverySecureReadiness::NotStarted => Self::NotStarted,
            core::PinRecoverySecureReadiness::PrekeysPending => Self::PrekeysPending,
            core::PinRecoverySecureReadiness::SecurePending => Self::SecurePending,
            core::PinRecoverySecureReadiness::Ready => Self::Ready,
        }
    }
}
impl From<core::RealmProgress> for DartRealmProgress {
    fn from(value: core::RealmProgress) -> Self {
        Self {
            realm_id: value.realm_id,
            phase: value.phase,
            error_code: value.error_code,
        }
    }
}
impl From<core::PinRecoveryError> for DartPinRecoveryError {
    fn from(value: core::PinRecoveryError) -> Self {
        Self {
            code: value.code,
            retryable: value.retryable,
            retry_after_seconds: value.retry_after_seconds,
        }
    }
}
impl From<core::PinRecoveryProgress> for DartPinRecoveryProgress {
    fn from(value: core::PinRecoveryProgress) -> Self {
        Self {
            operation_id: value.operation_id.0,
            kind: value.kind.into(),
            phase: value.phase.into(),
            allowed_actions: value.allowed_actions.into_iter().map(Into::into).collect(),
            target: value.target.into(),
            node_progress: value.node_progress.into_iter().map(Into::into).collect(),
            attempts_used: value.attempts_used,
            attempts_limit: value.attempts_limit,
            needs_phone_verification: value.needs_phone_verification,
            needs_user_confirmation: value.needs_user_confirmation,
            recovered_identity: value.recovered_identity.map(Into::into),
            new_device_id: value.new_device_id,
            device_label: value.device_label,
            secure_readiness: value.secure_readiness.into(),
            cleanup_remaining: value.cleanup_remaining,
            error: value.error.map(Into::into),
        }
    }
}
impl From<DartPinRecoveryTenantConfig> for core::PinRecoveryTenantConfig {
    fn from(v: DartPinRecoveryTenantConfig) -> Self {
        Self {
            tenant_id: v.tenant_id,
            deployment_id: v.deployment_id,
            control_base_url: v.control_base_url,
            config_signing_keys: v.config_signing_keys.into_iter().collect(),
        }
    }
}
impl From<im_core::identity::IdentitySelector> for crate::dto::identity::DartIdentitySelector {
    fn from(value: im_core::identity::IdentitySelector) -> Self {
        use im_core::identity::IdentitySelector as C;
        match value {
            C::Default => Self::Default,
            C::Id(v) => Self::Id {
                id: v.as_str().to_owned(),
            },
            C::Did(v) => Self::Did {
                did: v.as_str().to_owned(),
            },
            C::Handle(v) => Self::Handle {
                handle: v.as_str().to_owned(),
            },
            C::LocalAlias(alias) => Self::LocalAlias { alias },
        }
    }
}

impl From<core::PinRecoveryBackupState> for DartPinRecoveryBackupState {
    fn from(value: core::PinRecoveryBackupState) -> Self {
        match value {
            core::PinRecoveryBackupState::Unknown => Self::Unknown,
            core::PinRecoveryBackupState::NotEnabled => Self::NotEnabled,
            core::PinRecoveryBackupState::Pending => Self::Pending,
            core::PinRecoveryBackupState::Active => Self::Active,
        }
    }
}
impl From<core::PinRecoveryBackupStatus> for DartPinRecoveryBackupStatus {
    fn from(value: core::PinRecoveryBackupStatus) -> Self {
        Self {
            did: value.did,
            full_handle: value.full_handle,
            binding_generation: value.binding_generation,
            state: value.state.into(),
            active_epoch: value.active_epoch.map(|v| v.to_string()),
            error: value.error.map(Into::into),
        }
    }
}
