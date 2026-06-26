use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendMessageRequest {
    pub target: MessageTarget,
    pub body: MessageBody,
    pub security: MessageSecurityMode,
    pub client_message_id: Option<crate::ids::MessageId>,
    pub delivery: MessageDeliveryOptions,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delegated_signing: Option<DelegatedSigningOptions>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DelegatedSigningOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logical_sender_did: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signing_verification_method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signing_key_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_agent_did: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageTarget {
    Direct(crate::ids::PeerRef),
    Group(crate::ids::GroupRef),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageBody {
    Text {
        text: String,
        kind: MessageKind,
    },
    Payload {
        payload: serde_json::Value,
    },
    Attachment {
        input: crate::attachments::AttachmentInput,
        caption: Option<String>,
        mime_type: Option<String>,
        filename: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageKind {
    Text,
    Markdown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageSecurityMode {
    DefaultPlain,
    Plain,
    E2eeRequired,
    SecureDirect,
    GroupE2ee,
}

pub type MessageSecurityPolicy = MessageSecurityMode;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageDeliveryOptions {
    pub idempotency_key: Option<String>,
    pub wait_for_final_acceptance: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendMessageResult {
    pub message: Message,
    pub delivery: DeliveryState,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeliveryState {
    Accepted,
    Sent,
    StoredLocally,
    Failed { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub id: crate::ids::MessageId,
    pub thread: ThreadRef,
    pub direction: MessageDirection,
    pub sender: crate::ids::PeerRef,
    pub receiver: Option<crate::ids::PeerRef>,
    pub group: Option<crate::ids::GroupRef>,
    pub body: MessageBodyView,
    pub sent_at: Option<String>,
    pub received_at: Option<String>,
    pub metadata: MessageMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessagePage {
    pub items: Vec<Message>,
    pub next_cursor: Option<crate::ids::Cursor>,
    pub has_more: bool,
    pub source: Option<String>,
    pub resolved_dids: Vec<crate::ids::Did>,
    pub warnings: Vec<String>,
}

impl MessagePage {
    pub fn into_page(self) -> crate::ids::Page<Message> {
        crate::ids::Page {
            items: self.items,
            next_cursor: self.next_cursor,
            has_more: self.has_more,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageDirection {
    Outgoing,
    Incoming,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageBodyView {
    Text { text: String, kind: MessageKind },
    Payload { payload: serde_json::Value },
    Unsupported { content_type: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MessageMetadata {
    #[serde(default)]
    pub operation_id: Option<String>,
    #[serde(default)]
    pub delivery_state: Option<String>,
    #[serde(default)]
    pub send_state: Option<MessageSendState>,
    #[serde(default)]
    pub retry_plan: Option<MessageRetryPlan>,
    #[serde(default)]
    pub server_sequence: Option<i64>,
    #[serde(default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub attributes: Vec<MessageMetadataAttribute>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageSendState {
    pub state: MessageSendStateKind,
    pub operation_id: Option<String>,
    pub message_id: Option<crate::ids::MessageId>,
    pub reason: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageSendStateKind {
    Accepted,
    Sent,
    StoredLocally,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageRetryPlan {
    pub retryable: bool,
    pub action: MessageRetryAction,
    pub operation_id: Option<String>,
    pub message_id: Option<crate::ids::MessageId>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRetryAction {
    None,
    RetryDirectText,
    RetryGroupText,
    RetryDirectPayload,
    RetryGroupPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageMetadataAttribute {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThreadRef {
    Direct(crate::ids::PeerRef),
    Group(crate::ids::GroupRef),
    Thread(crate::ids::ThreadId),
}

pub fn direct_peer_scope_thread_id(
    user_id: impl AsRef<str>,
    full_handle: impl AsRef<str>,
) -> crate::ImResult<crate::ids::ThreadId> {
    let scope = crate::internal::local_state::owner_scope::DirectPeerScope::new(
        user_id.as_ref().to_owned(),
        full_handle.as_ref().to_owned(),
    )?;
    crate::ids::ThreadId::parse(
        crate::internal::local_state::owner_scope::direct_conversation_id_for_peer_scope(&scope),
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboxQuery {
    pub scope: InboxScope,
    pub limit: crate::ids::PageLimit,
    pub cursor: Option<crate::ids::Cursor>,
    pub unread_only: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inbox_history_options: Option<InboxHistoryOptions>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InboxScope {
    All,
    DirectOnly,
    GroupOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryQuery {
    pub limit: crate::ids::PageLimit,
    pub cursor: Option<crate::ids::Cursor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inbox_history_options: Option<InboxHistoryOptions>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalHistoryQuery {
    pub limit: crate::ids::PageLimit,
    pub cursor: Option<crate::ids::Cursor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboxHistoryOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inbox_owner_did: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inbox_auth_verification_method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inbox_auth_key_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inbox_auth: Option<InboxAuth>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InboxAuth {
    ScopedInboxToken { token: ScopedInboxToken },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopedInboxToken {
    pub token: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkReadResult {
    pub updated_count: u32,
    pub message_ids: Vec<crate::ids::MessageId>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkThreadReadRequest {
    pub thread: ThreadRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_message_ids: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkThreadReadResult {
    pub updated_count: u32,
    pub message_ids: Vec<crate::ids::MessageId>,
    pub local_candidate_count: u32,
    pub local_updated_count: u32,
    pub remote_updated_count: u32,
    pub remote_acknowledged: bool,
    pub partial: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conversation {
    pub thread: ThreadRef,
    pub title: Option<String>,
    pub participants: Vec<crate::ids::PeerRef>,
    pub last_message: Option<Message>,
    pub unread_count: u32,
    #[serde(default)]
    pub unread_mention_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_unread_mention_message_id: Option<crate::ids::MessageId>,
    pub message_count: u32,
    pub last_message_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationQuery {
    pub limit: crate::ids::PageLimit,
    pub include_groups: bool,
    pub include_direct: bool,
    pub unread_only: bool,
}
