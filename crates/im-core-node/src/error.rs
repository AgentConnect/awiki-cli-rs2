use napi::{Error, Status};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SafeError {
    pub(crate) code: String,
    pub(crate) safe_message: String,
    pub(crate) retryable: bool,
}

impl SafeError {
    pub(crate) fn new(
        code: impl Into<String>,
        safe_message: impl Into<String>,
        retryable: bool,
    ) -> Self {
        Self {
            code: code.into(),
            safe_message: safe_message.into(),
            retryable,
        }
    }

    pub(crate) fn closed() -> Self {
        Self::new("client_closed", "The IM client is closed.", false)
    }

    pub(crate) fn cancelled() -> Self {
        Self::new("cancelled", "The IM operation was cancelled.", true)
    }

    pub(crate) fn timeout() -> Self {
        Self::new("timeout", "The IM operation timed out.", true)
    }

    pub(crate) fn state_in_use() -> Self {
        Self::new(
            "state_in_use",
            "The IM state root is already open in another client.",
            false,
        )
    }

    pub(crate) fn internal() -> Self {
        Self::new("internal", "The IM operation failed internally.", false)
    }

    pub(crate) fn sync_outcome(status: im_core::messages::MessageSyncStatus) -> Self {
        match status {
            im_core::messages::MessageSyncStatus::AuthRevoked => Self::new(
                "auth_revoked",
                "The IM authorization is no longer valid.",
                false,
            ),
            im_core::messages::MessageSyncStatus::RecoveryRequired => Self::new(
                "sync_recovery_required",
                "The IM state requires synchronization recovery.",
                true,
            ),
            im_core::messages::MessageSyncStatus::RetryableFailure => Self::new(
                "sync_failed",
                "The IM synchronization could not be completed.",
                true,
            ),
            im_core::messages::MessageSyncStatus::Idle
            | im_core::messages::MessageSyncStatus::Changed => Self::internal(),
        }
    }

    pub(crate) fn from_im(error: im_core::ImError) -> Self {
        use im_core::ImError;

        match error {
            ImError::InvalidInput { .. } => {
                Self::new("invalid_input", "The IM request is invalid.", false)
            }
            ImError::IdentityRequired | ImError::DefaultIdentityMissing => Self::new(
                "identity_required",
                "A registered IM identity is required.",
                false,
            ),
            ImError::IdentityNotFound { .. } => Self::new(
                "identity_not_found",
                "The IM identity was not found.",
                false,
            ),
            ImError::IdentityNotReady { .. } => {
                Self::new("identity_not_ready", "The IM identity is not ready.", false)
            }
            ImError::AuthRequired => {
                Self::new("auth_required", "IM authentication is required.", false)
            }
            ImError::SessionExpired => Self::new(
                "session_expired",
                "The IM authentication session expired.",
                true,
            ),
            ImError::PermissionDenied => Self::new(
                "permission_denied",
                "The IM operation is not permitted.",
                false,
            ),
            ImError::PeerNotFound { .. }
            | ImError::GroupNotFound { .. }
            | ImError::MessageNotFound { .. } => {
                Self::new("not_found", "The IM resource was not found.", false)
            }
            ImError::CursorInvalid | ImError::CursorStale => {
                Self::new("invalid_cursor", "The IM page cursor is invalid.", false)
            }
            ImError::TransportUnavailable { .. } => Self::new(
                "transport_unavailable",
                "The IM service could not be reached.",
                true,
            ),
            ImError::AttachmentTransfer {
                failure, retryable, ..
            } => Self::new(
                failure.code(),
                "The attachment transfer could not be completed.",
                retryable,
            ),
            ImError::UnsupportedCapability { .. } => Self::new(
                "unsupported_capability",
                "The IM capability is not supported.",
                false,
            ),
            ImError::Service {
                status_code, code, ..
            } => service_error(status_code, code.as_deref()),
            ImError::IdentityUnresolved { .. } => Self::new(
                "identity_unresolved",
                "The IM peer identity could not be resolved.",
                false,
            ),
            ImError::IdentityBindingConflict { .. }
            | ImError::ConversationAliasConflict { .. }
            | ImError::MessageWireIdentityConflict { .. }
            | ImError::CanonicalGroupIdentityMissing { .. } => Self::new(
                "conflict",
                "The IM operation conflicts with the current state.",
                false,
            ),
            ImError::SkillOnboarding {
                code, retryable, ..
            } => Self::new(
                code,
                "The Skill Agent identity operation could not be completed.",
                retryable,
            ),
            ImError::IdentityVault { .. }
            | ImError::LocalStateUnavailable { .. }
            | ImError::LocalStateUpgradeRequired { .. }
            | ImError::LocalStateUpgradeInProgress
            | ImError::LocalStateUpgradeFailed { .. }
            | ImError::LocalProjectionUnavailable { .. }
            | ImError::PathUnavailable { .. }
            | ImError::CredentialFileUnreadable { .. }
            | ImError::Serialization { .. }
            | ImError::Io { .. }
            | ImError::Internal { .. }
            | ImError::InventoryIncomplete
            | ImError::InventoryTooLarge
            | ImError::DeviceRevokeOutcome { .. } => Self::internal(),
        }
    }

    pub(crate) fn into_napi(self) -> Error {
        let reason = serde_json::to_string(&self).unwrap_or_else(|_| {
            r#"{"code":"internal","safeMessage":"The IM operation failed internally.","retryable":false}"#.to_owned()
        });
        Error::new(Status::GenericFailure, reason)
    }
}

fn service_error(status: Option<u16>, code: Option<&str>) -> SafeError {
    let code = code.unwrap_or_default().trim().to_ascii_lowercase();
    if matches!(
        code.as_str(),
        "invalid_otp" | "otp_invalid" | "identity.registration_verification_invalid"
    ) {
        return SafeError::new("invalid_otp", "The registration OTP is invalid.", false);
    }
    if matches!(code.as_str(), "otp_expired" | "challenge_expired") {
        return SafeError::new(
            "challenge_expired",
            "The registration challenge expired.",
            false,
        );
    }
    if matches!(
        code.as_str(),
        "handle_unavailable" | "handle_exists" | "user_path_conflict" | "did_conflict"
    ) {
        return SafeError::new(
            "handle_unavailable",
            "The requested Handle is unavailable.",
            false,
        );
    }
    if matches!(code.as_str(), "otp_rate_limited" | "-32005") || status == Some(429) {
        return SafeError::new(
            "rate_limited",
            "The IM service rate-limited the request.",
            true,
        );
    }
    if status == Some(403) || code == "anp.forbidden" {
        return SafeError::new(
            "permission_denied",
            "The IM operation is not permitted.",
            false,
        );
    }
    if status == Some(404) || matches!(code.as_str(), "anp.target_not_found" | "target_not_found") {
        return SafeError::new("not_found", "The IM resource was not found.", false);
    }
    if status == Some(409)
        || matches!(
            code.as_str(),
            "anp.idempotency_conflict" | "idempotency_conflict"
        )
    {
        return SafeError::new(
            "conflict",
            "The IM operation conflicts with the current state.",
            false,
        );
    }
    SafeError::new(
        "service_error",
        "The IM service rejected the operation.",
        status.is_some_and(|status| status >= 500),
    )
}

pub(crate) type SafeResult<T> = Result<T, SafeError>;

pub(crate) fn napi_result<T>(result: SafeResult<T>) -> napi::Result<T> {
    result.map_err(SafeError::into_napi)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sensitive_core_sources_never_reach_the_safe_payload() {
        let error = SafeError::from_im(im_core::ImError::TransportUnavailable {
            detail: "Bearer secret-token at /private/state".to_owned(),
        });
        let payload = serde_json::to_string(&error).unwrap();
        assert_eq!(error.code, "transport_unavailable");
        assert!(!payload.contains("secret-token"));
        assert!(!payload.contains("/private/state"));
    }

    #[test]
    fn service_classification_uses_only_stable_code_and_status() {
        let error = SafeError::from_im(im_core::ImError::Service {
            status_code: None,
            code: Some("invalid_otp".to_owned()),
            message: "token=secret path=/private".to_owned(),
            data: Some(serde_json::json!({"private": "secret"})),
        });
        assert_eq!(error.code, "invalid_otp");
        assert_eq!(error.safe_message, "The registration OTP is invalid.");
    }

    #[test]
    fn numeric_registration_rate_limit_is_actionable() {
        let error = SafeError::from_im(im_core::ImError::Service {
            status_code: None,
            code: Some("-32005".to_owned()),
            message: "localized message must not be parsed".to_owned(),
            data: Some(serde_json::json!({"code": "otp_rate_limited"})),
        });
        assert_eq!(error.code, "rate_limited");
        assert!(error.retryable);
    }

    #[test]
    fn registration_verification_code_maps_without_parsing_service_text() {
        let error = SafeError::from_im(im_core::ImError::Service {
            status_code: Some(400),
            code: Some("identity.registration_verification_invalid".to_owned()),
            message: "localized message must not be parsed".to_owned(),
            data: Some(serde_json::json!({"retryable": false})),
        });
        assert_eq!(error.code, "invalid_otp");
        assert!(!error.retryable);
    }

    #[test]
    fn skill_provisioning_preserves_only_stable_code_and_retryability() {
        for (code, retryable) in [
            ("skill_onboarding_rate_limited", true),
            ("skill_onboarding_provision_cleanup_failed", true),
        ] {
            let error = SafeError::from_im(im_core::ImError::SkillOnboarding {
                code: code.to_owned(),
                phase: "private-phase".to_owned(),
                retryable,
            });
            let payload = serde_json::to_string(&error).unwrap();
            assert_eq!(error.code, code);
            assert_eq!(error.retryable, retryable);
            assert!(!payload.contains("private-phase"));
        }
    }

    #[test]
    fn upload_and_download_failures_keep_only_stable_transfer_metadata() {
        for (failure, expected_code, retryable) in [
            (
                im_core::AttachmentTransferFailure::Network,
                "attachment_transfer_network",
                true,
            ),
            (
                im_core::AttachmentTransferFailure::Incomplete,
                "attachment_transfer_incomplete",
                false,
            ),
        ] {
            let error = SafeError::from_im(im_core::ImError::AttachmentTransfer {
                failure,
                received_bytes: 128,
                expected_bytes: Some(256),
                retryable,
                detail: "Bearer secret-token at /private/attachment".to_owned(),
            });
            let payload = serde_json::to_string(&error).unwrap();
            assert_eq!(error.code, expected_code);
            assert_eq!(error.retryable, retryable);
            assert!(!payload.contains("secret-token"));
            assert!(!payload.contains("/private/attachment"));
            assert!(!payload.contains("256"));
        }
    }
}
