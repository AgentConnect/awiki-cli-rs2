// Temporary migration-only legacy bridge exception.
// Delete in PR C7 when group E2EE diagnostic handlers no longer share the
// default message cutover result mapper.

use std::fmt;

use serde_json::Value;

const ERR_TRANSPORT_UNAVAILABLE_TEXT: &str = "message transport is unavailable";

#[derive(Debug, Clone)]
pub struct CommandResult {
    pub data: Value,
    pub summary: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ServiceError {
    pub status_code: u16,
    pub rpc_code: i64,
    pub message: String,
    pub data: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentityErrorKind {
    InvalidInput,
    NotFound,
    Conflict,
    AuthRequired,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityError {
    pub kind: IdentityErrorKind,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MessageAdapterError {
    TargetRequired,
    GroupRequired,
    MemberRequired,
    GroupOwnerCannotLeave,
    TextRequired,
    FilePathRequired,
    MimeTypeWithoutFile,
    MessageIdRequired,
    OutputPathRequired,
    DownloadTargetNeeded,
    DownloadTargetConflict,
    AttachmentNotFound,
    AttachmentIdRequired,
    AttachmentMessageInvalid,
    AttachmentSenderRequired,
    TransportUnavailable(String),
    SecureNotSupported,
    AttachmentNotSupported,
    GroupNotSupported,
    GroupE2eeSelfLeaveUnsupported,
    MessageNotFound,
    IdentityRequired(String),
    Service(ServiceError),
    Identity(IdentityError),
    Internal(String),
    InvalidAttachmentServiceEndpoint(String),
    MissingMessageServiceDid,
    MissingAttachmentServiceDid,
    Json(String),
}

impl MessageAdapterError {
    pub fn transport_unavailable(detail: impl Into<String>) -> Self {
        Self::TransportUnavailable(detail.into())
    }
}

impl fmt::Display for ServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.rpc_code, self.status_code) {
            (code, _) if code != 0 => {
                write!(formatter, "service rpc error {code}: {}", self.message)
            }
            (_, status_code) if status_code != 0 => {
                write!(
                    formatter,
                    "service http error {status_code}: {}",
                    self.message
                )
            }
            _ => formatter.write_str(&self.message),
        }
    }
}

impl fmt::Display for IdentityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl fmt::Display for MessageAdapterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TargetRequired => formatter.write_str("direct message target is required"),
            Self::GroupRequired => formatter.write_str("group target is required"),
            Self::MemberRequired => formatter.write_str("group member target is required"),
            Self::GroupOwnerCannotLeave => {
                formatter.write_str("group owner cannot leave the group")
            }
            Self::TextRequired => formatter.write_str("message text is required"),
            Self::FilePathRequired => formatter.write_str("attachment file path is required"),
            Self::MimeTypeWithoutFile => {
                formatter.write_str("mime_type requires an attachment file")
            }
            Self::MessageIdRequired => formatter.write_str("attachment message id is required"),
            Self::OutputPathRequired => formatter.write_str("attachment output path is required"),
            Self::DownloadTargetNeeded => {
                formatter.write_str("attachment download requires either --with or --group")
            }
            Self::DownloadTargetConflict => formatter
                .write_str("attachment download accepts either --with or --group, but not both"),
            Self::AttachmentNotFound => {
                formatter.write_str("attachment not found in message content")
            }
            Self::AttachmentIdRequired => formatter
                .write_str("attachment_id is required for messages with multiple attachments"),
            Self::AttachmentMessageInvalid => {
                formatter.write_str("message is not an attachment manifest")
            }
            Self::AttachmentSenderRequired => {
                formatter.write_str("attachment message sender_did is required")
            }
            Self::TransportUnavailable(detail) => {
                if detail.trim().is_empty() {
                    formatter.write_str(ERR_TRANSPORT_UNAVAILABLE_TEXT)
                } else {
                    write!(
                        formatter,
                        "{}: {}",
                        ERR_TRANSPORT_UNAVAILABLE_TEXT,
                        detail.trim()
                    )
                }
            }
            Self::SecureNotSupported => {
                formatter.write_str("secure messaging is not supported for this command yet")
            }
            Self::AttachmentNotSupported => {
                formatter.write_str("attachment messaging is not supported for this command yet")
            }
            Self::GroupNotSupported => {
                formatter.write_str("group messaging is not supported for this command yet")
            }
            Self::GroupE2eeSelfLeaveUnsupported => {
                formatter.write_str("group E2EE self-leave is not cryptographically supported yet")
            }
            Self::MessageNotFound => formatter.write_str("message not found"),
            Self::IdentityRequired(message) | Self::Internal(message) => {
                formatter.write_str(message)
            }
            Self::Service(error) => write!(formatter, "{error}"),
            Self::Identity(error) => write!(formatter, "{error}"),
            Self::InvalidAttachmentServiceEndpoint(message) => {
                write!(
                    formatter,
                    "attachment service endpoint is invalid: {message}"
                )
            }
            Self::MissingMessageServiceDid => {
                formatter.write_str("message service did is required")
            }
            Self::MissingAttachmentServiceDid => {
                formatter.write_str("attachment service did is required")
            }
            Self::Json(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for ServiceError {}
impl std::error::Error for IdentityError {}
impl std::error::Error for MessageAdapterError {}

impl From<crate::identity::wire::ServiceError> for ServiceError {
    fn from(value: crate::identity::wire::ServiceError) -> Self {
        Self {
            status_code: value.status_code,
            rpc_code: value.rpc_code,
            message: value.message,
            data: value.data,
        }
    }
}

impl From<crate::identity::IdentityError> for IdentityError {
    fn from(value: crate::identity::IdentityError) -> Self {
        match value {
            crate::identity::IdentityError::InvalidInput(message) => Self {
                kind: IdentityErrorKind::InvalidInput,
                message,
            },
            crate::identity::IdentityError::NotFound(message)
            | crate::identity::IdentityError::LegacyNotFound(message)
            | crate::identity::IdentityError::NoDefaultIdentity(message) => Self {
                kind: IdentityErrorKind::NotFound,
                message,
            },
            crate::identity::IdentityError::Conflict(message) => Self {
                kind: IdentityErrorKind::Conflict,
                message,
            },
            crate::identity::IdentityError::AuthRequired(message) => Self {
                kind: IdentityErrorKind::AuthRequired,
                message,
            },
            crate::identity::IdentityError::Service(error) => {
                let message = error.to_string();
                Self {
                    kind: identity_service_error_kind(&error),
                    message,
                }
            }
            crate::identity::IdentityError::Io(error) => Self {
                kind: IdentityErrorKind::Internal,
                message: error.to_string(),
            },
            crate::identity::IdentityError::Json(error) => Self {
                kind: IdentityErrorKind::Internal,
                message: error.to_string(),
            },
            crate::identity::IdentityError::Internal(message) => Self {
                kind: IdentityErrorKind::Internal,
                message,
            },
        }
    }
}

fn identity_service_error_kind(error: &crate::identity::wire::ServiceError) -> IdentityErrorKind {
    match (error.status_code, error.rpc_code) {
        (400, _) | (_, -32602) => IdentityErrorKind::InvalidInput,
        (401, _) | (_, -32000) => IdentityErrorKind::AuthRequired,
        (404, _) | (_, -32002) => IdentityErrorKind::NotFound,
        (409, _) | (_, -32003 | -32004) => IdentityErrorKind::Conflict,
        _ => IdentityErrorKind::Internal,
    }
}
