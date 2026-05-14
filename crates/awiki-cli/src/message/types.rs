use std::fmt;

pub const MESSAGE_RPC_ENDPOINT: &str = "/im/rpc";
pub const MESSAGE_WS_ENDPOINT: &str = "/im/ws";

pub const ERR_TRANSPORT_UNAVAILABLE_TEXT: &str = "message transport is unavailable";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageError {
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
    GroupE2eeSelfLeaveUnsupported,
    MessageNotFound,
    InvalidAttachmentServiceEndpoint(String),
    MissingMessageServiceDid,
    MissingAttachmentServiceDid,
    Json(String),
}

impl MessageError {
    pub fn transport_unavailable(detail: impl Into<String>) -> Self {
        Self::TransportUnavailable(detail.into())
    }
}

impl fmt::Display for MessageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TargetRequired => f.write_str("direct message target is required"),
            Self::GroupRequired => f.write_str("group target is required"),
            Self::MemberRequired => f.write_str("group member target is required"),
            Self::GroupOwnerCannotLeave => f.write_str("group owner cannot leave the group"),
            Self::TextRequired => f.write_str("message text is required"),
            Self::FilePathRequired => f.write_str("attachment file path is required"),
            Self::MimeTypeWithoutFile => f.write_str("mime_type requires an attachment file"),
            Self::MessageIdRequired => f.write_str("attachment message id is required"),
            Self::OutputPathRequired => f.write_str("attachment output path is required"),
            Self::DownloadTargetNeeded => {
                f.write_str("attachment download requires either --with or --group")
            }
            Self::DownloadTargetConflict => {
                f.write_str("attachment download accepts either --with or --group, but not both")
            }
            Self::AttachmentNotFound => f.write_str("attachment not found in message content"),
            Self::AttachmentIdRequired => {
                f.write_str("attachment_id is required for messages with multiple attachments")
            }
            Self::AttachmentMessageInvalid => f.write_str("message is not an attachment manifest"),
            Self::AttachmentSenderRequired => {
                f.write_str("attachment message sender_did is required")
            }
            Self::TransportUnavailable(detail) => {
                if detail.trim().is_empty() {
                    f.write_str(ERR_TRANSPORT_UNAVAILABLE_TEXT)
                } else {
                    write!(f, "{ERR_TRANSPORT_UNAVAILABLE_TEXT}: {}", detail.trim())
                }
            }
            Self::SecureNotSupported => {
                f.write_str("secure messaging is not supported for this command yet")
            }
            Self::GroupE2eeSelfLeaveUnsupported => {
                f.write_str("group E2EE self-leave is not cryptographically supported yet")
            }
            Self::MessageNotFound => f.write_str("message not found"),
            Self::InvalidAttachmentServiceEndpoint(message) => {
                write!(f, "attachment service endpoint is invalid: {message}")
            }
            Self::MissingMessageServiceDid => f.write_str("message service did is required"),
            Self::MissingAttachmentServiceDid => f.write_str("attachment service did is required"),
            Self::Json(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for MessageError {}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SendRequest {
    pub identity_name: String,
    pub target: String,
    pub group: String,
    pub text: String,
    pub message_type: String,
    pub secure_mode: String,
    pub file_path: String,
    pub mime_type: String,
}

impl SendRequest {
    pub fn has_attachment(&self) -> bool {
        !self.file_path.trim().is_empty()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InboxRequest {
    pub identity_name: String,
    pub scope: String,
    pub with: String,
    pub group: String,
    pub limit: i64,
    pub unread_only: bool,
    pub mark_read: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HistoryRequest {
    pub identity_name: String,
    pub with: String,
    pub limit: i64,
    pub cursor: String,
    pub skip: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MarkReadRequest {
    pub identity_name: String,
    pub message_ids: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AttachmentDownloadRequest {
    pub identity_name: String,
    pub with: String,
    pub group: String,
    pub message_id: String,
    pub attachment_id: String,
    pub output_path: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SecureStatusRequest {
    pub identity_name: String,
    pub with: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SecurePeerRequest {
    pub identity_name: String,
    pub with: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SecureOutboxActionRequest {
    pub identity_name: String,
    pub outbox_id: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupCreateRequest {
    pub identity_name: String,
    pub name: String,
    pub description: String,
    pub discoverability: String,
    pub admission_mode: String,
    pub message_security_profile: String,
    pub e2ee: bool,
    pub slug: String,
    pub goal: String,
    pub rules: String,
    pub message_prompt: String,
    pub doc_url: String,
    pub attachments_allowed: Option<bool>,
    pub max_members: String,
    pub member_max_messages: Option<i64>,
    pub member_max_total_chars: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupGetRequest {
    pub identity_name: String,
    pub group: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupInfoRequest {
    pub identity_name: String,
    pub group: String,
    pub include_policy: bool,
    pub include_member_list: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupJoinRequest {
    pub identity_name: String,
    pub group: String,
    pub reason_text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupMemberRequest {
    pub identity_name: String,
    pub group: String,
    pub member: String,
    pub role: String,
    pub reason_text: String,
    pub e2ee: bool,
    pub leave_request_id: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupLeaveRequest {
    pub identity_name: String,
    pub group: String,
    pub reason_text: String,
    pub e2ee: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupE2eeProcessLeaveRequest {
    pub identity_name: String,
    pub group: String,
    pub member: String,
    pub leave_request_id: String,
    pub reason_text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupE2eeRecoverMemberRequest {
    pub identity_name: String,
    pub group: String,
    pub member: String,
    pub device_id: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupE2eeUpdateKeyRequest {
    pub identity_name: String,
    pub group: String,
    pub member: String,
    pub device_id: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupUpdateRequest {
    pub identity_name: String,
    pub group: String,
    pub name: String,
    pub description: String,
    pub discoverability: String,
    pub admission_mode: String,
    pub slug: String,
    pub goal: String,
    pub rules: String,
    pub message_prompt: String,
    pub doc_url: String,
    pub attachments_allowed: Option<bool>,
    pub max_members: String,
    pub member_max_messages: Option<i64>,
    pub member_max_total_chars: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupListRequest {
    pub identity_name: String,
    pub limit: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupMembersRequest {
    pub identity_name: String,
    pub group: String,
    pub limit: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupMessagesRequest {
    pub identity_name: String,
    pub group: String,
    pub limit: i64,
    pub cursor: String,
    pub skip: i64,
}
