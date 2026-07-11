use std::fmt;

pub type ImResult<T> = Result<T, ImError>;

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
    PeerNotFound {
        peer: String,
    },
    GroupNotFound {
        group: String,
    },
    MessageNotFound {
        message_id: String,
    },
    TransportUnavailable {
        detail: String,
    },
    UnsupportedCapability {
        capability: String,
    },
    LocalStateUnavailable {
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
            Self::PeerNotFound { peer } => write!(f, "peer not found: {peer}"),
            Self::GroupNotFound { group } => write!(f, "group not found: {group}"),
            Self::MessageNotFound { message_id } => write!(f, "message not found: {message_id}"),
            Self::TransportUnavailable { detail } => write!(f, "transport unavailable: {detail}"),
            Self::UnsupportedCapability { capability } => {
                write!(f, "unsupported capability: {capability}")
            }
            Self::LocalStateUnavailable { detail } => {
                write!(f, "local state unavailable: {detail}")
            }
            Self::PathUnavailable { path_kind, detail } => {
                write!(f, "{path_kind} path unavailable: {detail}")
            }
            Self::CredentialFileUnreadable { path_kind, detail } => {
                write!(f, "{path_kind} credential file unreadable: {detail}")
            }
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
