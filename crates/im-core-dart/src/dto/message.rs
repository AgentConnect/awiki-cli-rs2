#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DartMessageTarget {
    Direct { peer: String },
    Group { group: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DartThreadRef {
    Direct { peer: String },
    Group { group: String },
    Thread { thread_id: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DartMessageSecurityMode {
    DefaultPlain,
    Plain,
    E2eeRequired,
    SecureDirect,
    GroupE2ee,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartSendTextRequest {
    pub target: DartMessageTarget,
    pub text: String,
    pub markdown: bool,
    pub security: DartMessageSecurityMode,
    pub client_message_id: Option<String>,
    pub idempotency_key: Option<String>,
    pub wait_for_final_acceptance: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DartMessageDirection {
    Outgoing,
    Incoming,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartMessageBodyView {
    pub text: Option<String>,
    pub kind: Option<String>,
    pub unsupported_content_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartMessageMetadata {
    pub operation_id: Option<String>,
    pub delivery_state: Option<String>,
    pub send_state: Option<String>,
    pub retryable: Option<bool>,
    pub retry_action: Option<String>,
    pub server_sequence: Option<i64>,
    pub content_type: Option<String>,
    pub attributes: Vec<DartMessageMetadataAttribute>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartMessageMetadataAttribute {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartMessage {
    pub id: String,
    pub thread_kind: String,
    pub thread_id: String,
    pub direction: DartMessageDirection,
    pub sender: String,
    pub receiver: Option<String>,
    pub group: Option<String>,
    pub body: DartMessageBodyView,
    pub sent_at: Option<String>,
    pub received_at: Option<String>,
    pub metadata: DartMessageMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartMessagePage {
    pub items: Vec<DartMessage>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartConversation {
    pub thread_kind: String,
    pub thread_id: String,
    pub title: Option<String>,
    pub participants: Vec<String>,
    pub last_message: Option<DartMessage>,
    pub unread_count: u32,
    pub message_count: u32,
    pub last_message_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartConversationPage {
    pub items: Vec<DartConversation>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartSendMessageResult {
    pub message: DartMessage,
    pub delivery_state: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartMarkReadResult {
    pub updated_count: u32,
    pub message_ids: Vec<String>,
    pub warnings: Vec<String>,
}
