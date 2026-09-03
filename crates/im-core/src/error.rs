use std::fmt;

pub type ImResult<T> = Result<T, ImError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceRevokeOutcomeCategory {
    CancelledBeforeSubmit,
    RejectedBeforeCommit,
    OutcomeUnknown,
}

/// Stable, redacted reasons why an identity vault cannot be opened or verified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityVaultFailure {
    Unavailable,
    MetadataMissing,
    MetadataUnverified,
    WorkspaceMismatch,
    DeviceMismatch,
    RecordOpenFailed,
    VerificationFailed,
}

/// Stable attachment transfer failures exposed to every SDK surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentTransferFailure {
    Network,
    Stalled,
    Incomplete,
    RangeRejected,
    Cancelled,
}

impl AttachmentTransferFailure {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Network => "attachment_transfer_network",
            Self::Stalled => "attachment_transfer_stalled",
            Self::Incomplete => "attachment_transfer_incomplete",
            Self::RangeRejected => "attachment_transfer_range_rejected",
            Self::Cancelled => "attachment_transfer_cancelled",
        }
    }
}

impl IdentityVaultFailure {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Unavailable => "identity_vault_unavailable",
            Self::MetadataMissing => "identity_vault_metadata_missing",
            Self::MetadataUnverified => "identity_vault_metadata_unverified",
            Self::WorkspaceMismatch => "identity_vault_workspace_mismatch",
            Self::DeviceMismatch => "identity_vault_device_mismatch",
            Self::RecordOpenFailed => "identity_vault_record_open_failed",
            Self::VerificationFailed => "identity_vault_verification_failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImError {
    InvalidInput {
        field: Option<String>,
        message: String,
    },
    IdentityRequired,
    IdentityNotFound {
        selector: String,
    },
    DefaultIdentityMissing,
    IdentityNotReady {
        identity: String,
        missing: Vec<String>,
    },
    IdentityVault {
        failure: IdentityVaultFailure,
    },
    AuthRequired,
    SessionExpired,
    PermissionDenied,
    LocalIdentityRecoveryRequired,
    PeerNotFound {
        peer: String,
    },
    GroupNotFound {
        group: String,
    },
    CursorInvalid,
    CursorStale,
    InventoryIncomplete,
    InventoryTooLarge,
    DeviceRevokeOutcome {
        category: DeviceRevokeOutcomeCategory,
    },
    MessageNotFound {
        message_id: String,
    },
    TransportUnavailable {
        detail: String,
    },
    AttachmentTransfer {
        failure: AttachmentTransferFailure,
        received_bytes: u64,
        expected_bytes: Option<u64>,
        retryable: bool,
        detail: String,
    },
    UnsupportedCapability {
        capability: String,
    },
    LocalStateUnavailable {
        detail: String,
    },
    LocalStateUpgradeRequired {
        from_version: i64,
        target_version: i64,
    },
    LocalStateUpgradeInProgress,
    LocalStateUpgradeFailed {
        phase: String,
        code: String,
    },
    IdentityUnresolved {
        detail: String,
    },
    IdentityBindingConflict {
        detail: String,
    },
    ConversationAliasConflict {
        alias: String,
        existing_target: String,
        requested_target: String,
    },
    MessageWireIdentityConflict {
        message_id: String,
    },
    CanonicalGroupIdentityMissing {
        group: String,
    },
    LocalProjectionUnavailable {
        detail: String,
    },
    PathUnavailable {
        path_kind: String,
        detail: String,
    },
    CredentialFileUnreadable {
        path_kind: String,
        detail: String,
    },
    SkillOnboarding {
        code: String,
        phase: String,
        retryable: bool,
    },
    Service {
        status_code: Option<u16>,
        code: Option<String>,
        message: String,
        data: Option<serde_json::Value>,
    },
    Serialization {
        detail: String,
    },
    Io {
        detail: String,
    },
    Internal {
        message: String,
    },
}

impl ImError {
    pub fn invalid_input(field: impl Into<Option<String>>, message: impl Into<String>) -> Self {
        Self::InvalidInput {
            field: field.into(),
            message: message.into(),
        }
    }

    pub fn unsupported(capability: impl Into<String>) -> Self {
        Self::UnsupportedCapability {
            capability: capability.into(),
        }
    }
}

impl fmt::Display for ImError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput {
                field: Some(field),
                message,
            } => write!(f, "invalid input for {field}: {message}"),
            Self::InvalidInput {
                field: None,
                message,
            } => write!(f, "invalid input: {message}"),
            Self::IdentityRequired => f.write_str("identity is required"),
            Self::IdentityNotFound { selector } => {
                write!(f, "identity not found for selector {selector}")
            }
            Self::DefaultIdentityMissing => f.write_str("default identity is missing"),
            Self::IdentityNotReady { identity, missing } => {
                write!(
                    f,
                    "identity {identity} is not ready: {}",
                    missing.join(", ")
                )
            }
            Self::IdentityVault { failure } => {
                write!(f, "identity vault failure: {}", failure.code())
            }
            Self::AuthRequired => f.write_str("authentication is required"),
            Self::SessionExpired => f.write_str("session expired"),
            Self::PermissionDenied => f.write_str("permission denied"),
            Self::LocalIdentityRecoveryRequired => {
                f.write_str("local identity recovery is required")
            }
            Self::PeerNotFound { peer } => write!(f, "peer not found: {peer}"),
            Self::GroupNotFound { group } => write!(f, "group not found: {group}"),
            Self::CursorInvalid => f.write_str("group page cursor is invalid"),
            Self::CursorStale => f.write_str("group member inventory changed during pagination"),
            Self::InventoryIncomplete => f.write_str("group member inventory is incomplete"),
            Self::InventoryTooLarge => f.write_str("group member inventory exceeds its limit"),
            Self::DeviceRevokeOutcome { category } => match category {
                DeviceRevokeOutcomeCategory::CancelledBeforeSubmit => {
                    f.write_str("device revoke was cancelled before submission")
                }
                DeviceRevokeOutcomeCategory::RejectedBeforeCommit => {
                    f.write_str("device revoke was rejected before commit")
                }
                DeviceRevokeOutcomeCategory::OutcomeUnknown => {
                    f.write_str("device revoke outcome is unknown")
                }
            },
            Self::MessageNotFound { message_id } => write!(f, "message not found: {message_id}"),
            Self::TransportUnavailable { detail } => write!(f, "transport unavailable: {detail}"),
            Self::AttachmentTransfer {
                failure,
                received_bytes,
                expected_bytes,
                retryable,
                detail,
            } => write!(
                f,
                "attachment transfer failed ({}): received {received_bytes} of {} bytes (retryable={retryable}): {detail}",
                failure.code(),
                expected_bytes
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "unknown".to_owned()),
            ),
            Self::UnsupportedCapability { capability } => {
                write!(f, "unsupported capability: {capability}")
            }
            Self::LocalStateUnavailable { detail } => {
                write!(f, "local state unavailable: {detail}")
            }
            Self::LocalStateUpgradeRequired {
                from_version,
                target_version,
            } => write!(
                f,
                "local state upgrade required: schema {from_version} -> {target_version}"
            ),
            Self::LocalStateUpgradeInProgress => f.write_str("local state upgrade in progress"),
            Self::LocalStateUpgradeFailed { phase, code } => {
                write!(f, "local state upgrade failed during {phase}: {code}")
            }
            Self::IdentityUnresolved { detail } => write!(f, "identity unresolved: {detail}"),
            Self::IdentityBindingConflict { detail } => {
                write!(f, "identity binding conflict: {detail}")
            }
            Self::ConversationAliasConflict {
                alias,
                existing_target,
                requested_target,
            } => write!(
                f,
                "conversation alias conflict for {alias}: existing target {existing_target}, requested target {requested_target}"
            ),
            Self::MessageWireIdentityConflict { message_id } => {
                write!(f, "message wire identity conflict: {message_id}")
            }
            Self::CanonicalGroupIdentityMissing { group } => {
                write!(f, "canonical group identity missing: {group}")
            }
            Self::LocalProjectionUnavailable { detail } => {
                write!(f, "local projection unavailable: {detail}")
            }
            Self::PathUnavailable { path_kind, detail } => {
                write!(f, "{path_kind} path unavailable: {detail}")
            }
            Self::CredentialFileUnreadable { path_kind, detail } => {
                write!(f, "{path_kind} credential file unreadable: {detail}")
            }
            Self::SkillOnboarding {
                code,
                phase,
                retryable,
            } => write!(
                f,
                "Skill onboarding failed during {phase}: {code} (retryable={retryable})"
            ),
            Self::Service {
                status_code,
                code,
                message,
                data: _,
            } => match (status_code, code) {
                (Some(status), Some(code)) => {
                    write!(f, "service error {status} ({code}): {message}")
                }
                (Some(status), None) => write!(f, "service error {status}: {message}"),
                (None, Some(code)) => write!(f, "service error ({code}): {message}"),
                (None, None) => write!(f, "service error: {message}"),
            },
            Self::Serialization { detail } => write!(f, "serialization error: {detail}"),
            Self::Io { detail } => write!(f, "io error: {detail}"),
            Self::Internal { message } => write!(f, "internal error: {message}"),
        }
    }
}

impl std::error::Error for ImError {}

impl From<std::io::Error> for ImError {
    fn from(value: std::io::Error) -> Self {
        Self::Io {
            detail: value.to_string(),
        }
    }
}

#[cfg(feature = "sqlite")]
impl From<rusqlite::Error> for ImError {
    fn from(value: rusqlite::Error) -> Self {
        Self::LocalStateUnavailable {
            detail: value.to_string(),
        }
    }
}
