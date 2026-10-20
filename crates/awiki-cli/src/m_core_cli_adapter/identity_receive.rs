//! Safe diagnostics after registration has committed. This layer never retries
//! registration, changes the selected identity, or relaxes receive readiness.

use super::{registration_receive_pending, ExitError};
use im_core::messages::{MessageSyncOutcome, MessageSyncStatus};
use im_core::ImError;
use serde_json::json;

pub(super) fn missing_identity() -> ExitError {
    let mut error = registration_receive_pending();
    error.detail.details = json!({
        "registration_committed": true,
        "phase": "identity_selection",
        "cause_code": "registered_identity_missing",
    });
    error
}

pub(super) fn core_failure(phase: &'static str, cause: &ImError) -> ExitError {
    // Error Display, service bodies, paths and identity selectors can contain
    // secrets. Project only categories chosen by this client and HTTP status.
    let code = match cause {
        ImError::TransportUnavailable { .. } => "transport_unavailable",
        ImError::Service { .. } => "service_error",
        ImError::AuthRequired | ImError::SessionExpired => "auth_required",
        ImError::PermissionDenied => "permission_denied",
        ImError::IdentityRequired | ImError::DefaultIdentityMissing => "identity_required",
        ImError::IdentityNotFound { .. } => "identity_not_found",
        ImError::IdentityNotReady { .. } => "identity_not_ready",
        ImError::IdentityVault { failure } => failure.code(),
        ImError::LocalIdentityRecoveryRequired => "local_identity_recovery_required",
        ImError::LocalStateUnavailable { .. } => "local_state_unavailable",
        ImError::LocalStateUpgradeRequired { .. } => "local_state_upgrade_required",
        ImError::LocalStateUpgradeInProgress => "local_state_upgrade_in_progress",
        ImError::LocalStateUpgradeFailed { .. } => "local_state_upgrade_failed",
        ImError::LocalProjectionUnavailable { .. } => "local_projection_unavailable",
        ImError::Serialization { .. } => "serialization_error",
        ImError::Io { .. } => "io_error",
        ImError::Internal { .. } => "internal_error",
        _ => "core_error",
    };
    let http_status = match cause {
        ImError::Service { status_code, .. } => *status_code,
        _ => None,
    };
    let mut error = registration_receive_pending();
    error.detail.details = json!({
        "registration_committed": true, "phase": phase,
        "cause_code": code, "http_status": http_status,
    });
    error
}

pub(super) fn require_ready(outcome: &MessageSyncOutcome) -> Result<(), ExitError> {
    super::super::messages::require_foreground_message_sync(outcome).map_err(|_| {
        let status = match outcome.status {
            MessageSyncStatus::Idle => "idle",
            MessageSyncStatus::Changed => "changed",
            MessageSyncStatus::RetryableFailure => "retryable_failure",
            MessageSyncStatus::RecoveryRequired => "recovery_required",
            MessageSyncStatus::Blocked => "blocked",
            MessageSyncStatus::AuthRevoked => "auth_revoked",
        };
        let safe_code = outcome.error_code.as_deref().filter(|code| {
            matches!(
                *code,
                "SYNC_RETRYABLE_FAILURE"
                    | "SYNC_RECOVERY_RETRY"
                    | "SYNC_RECOVERY_FAILED"
                    | "SYNC_RECEIVE_BUSY"
                    | "SYNC_RUN_SUPERSEDED"
                    | "SYNC_INVALID_SNAPSHOT"
                    | "SYNC_ACCOUNT_BINDING_MISMATCH"
                    | "SYNC_INVALID_PAGE"
                    | "SYNC_INPUT_INVALID"
                    | "SYNC_P6_NONCONFORMANT"
                    | "SYNC_P6_APPLICATION_INCOMPLETE"
                    | "SYNC_HYDRATION_INCOMPLETE"
                    | "SYNC_LANE_BOOTSTRAP_REQUIRED"
                    | "SYNC_LOCAL_OUTBOX_CORRUPT"
                    | "AUTH_REVOKED"
                    | "SYNC_AUTH_GENERATION_MISMATCH"
                    | "1401"
                    | "anp.device_not_eligible"
                    | "anp.device_state_changed"
                    | "sync.device_binding_mismatch"
                    | "message_wire_identity_conflict"
                    | "device_reprovision_required"
                    | "server_repair_required"
                    | "sync.snapshot_item_too_large"
                    | "sync.snapshot_required_state_too_large"
                    | "sync.client_upgrade_required"
                    | "sync.invalid_request"
                    | "sync.invalid_cursor"
                    | "SYNC_CAPACITY_EXCEEDED"
                    | "SYNC_BLOCKED"
                    | "SYNC_RECOVERY_REQUIRED"
            )
        });
        let warnings: Vec<_> = outcome
            .warnings
            .iter()
            .filter(|warning| {
                matches!(
                    warning.as_str(),
                    "sync.budget_exhausted"
                        | "sync.blocked.action_required"
                        | "sync.retry.transport_unavailable"
                        | "sync.retry.service_unavailable"
                        | "sync.retry.local_state_unavailable"
                        | "sync.retry.local_state.actor_closed"
                        | "sync.retry.local_state.database_busy"
                        | "sync.retry.local_state.constraint_failed"
                        | "sync.retry.local_state.schema_unavailable"
                        | "sync.retry.local_state.storage_unavailable"
                        | "sync.retry.local_state.codec_unavailable"
                        | "sync.retry.local_state.other"
                )
            })
            .collect();
        let mut error = registration_receive_pending();
        // retryable remains false: rerunning the registration write is not the
        // recovery action. The existing hint directs a same-identity inbox read.
        error.detail.details = json!({
            "registration_committed": true, "phase": "sync_readiness",
            "sync_status": status, "sync_error_code": safe_code,
            "sync_error_code_redacted": outcome.error_code.is_some() && safe_code.is_none(),
            "sync_warnings": warnings,
            "sync_warning_count": outcome.warnings.len(),
            "sync_warnings_redacted": outcome.warnings.len() - warnings.len(),
            "pages_fetched": outcome.pages_fetched,
            "events_applied": outcome.events_applied,
        });
        error
    })
}
