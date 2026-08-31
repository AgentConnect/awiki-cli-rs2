use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;

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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mention_payload: Option<serde_json::Value>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_identity: Option<ConversationIdentity>,
    #[serde(default)]
    pub attributes: Vec<MessageMetadataAttribute>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationIdentity {
    pub conversation_id: String,
    pub canonical_thread_kind: String,
    pub canonical_thread_id: String,
    pub storage_thread_ref: ConversationStorageThreadRef,
    #[serde(default)]
    pub aliases: Vec<ConversationAlias>,
    pub identity_scope: ConversationIdentityScope,
    pub migration_state: ConversationMigrationState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationStorageThreadRef {
    pub kind: String,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationAlias {
    pub kind: String,
    pub id: String,
    pub source: ConversationAliasSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationAliasSource {
    LegacyDirectDid,
    OldFlutterSortedDirect,
    PeerScopeStorage,
    GroupStorage,
    ThreadStorage,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationIdentityScope {
    Direct,
    Group,
    Thread,
    Mail,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationMigrationState {
    Canonical,
    AliasResolved,
    LegacyInput,
    Unknown,
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
    Pending,
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

impl ConversationIdentity {
    pub fn from_thread_ref(thread: &ThreadRef) -> Self {
        let (kind, id) = thread_ref_parts(thread);
        Self::from_storage_parts(kind, id)
    }

    pub fn from_thread_ref_for_owner(thread: &ThreadRef, owner_did: &str) -> Self {
        let (kind, id) = thread_ref_parts(thread);
        Self::from_storage_parts_for_owner(kind, id, owner_did)
    }

    pub fn from_storage_parts(kind: impl Into<String>, id: impl Into<String>) -> Self {
        Self::from_storage_parts_with_owner(kind, id, None)
    }

    pub fn from_storage_parts_for_owner(
        kind: impl Into<String>,
        id: impl Into<String>,
        owner_did: &str,
    ) -> Self {
        Self::from_storage_parts_with_owner(kind, id, Some(owner_did))
    }

    fn from_storage_parts_with_owner(
        kind: impl Into<String>,
        id: impl Into<String>,
        owner_did: Option<&str>,
    ) -> Self {
        let kind = kind.into();
        let id = id.into();
        let conversation_id = canonical_conversation_id_for_storage_parts(&kind, &id, owner_did);
        let identity_scope = identity_scope_for_kind_id(&kind, &id);
        let migration_state =
            migration_state_for_storage_parts(&kind, &id, &conversation_id, &identity_scope);
        let aliases = aliases_for_storage_parts(&kind, &id, &conversation_id, &identity_scope);
        Self {
            conversation_id: conversation_id.clone(),
            canonical_thread_kind: canonical_thread_kind(&identity_scope).to_owned(),
            canonical_thread_id: conversation_id,
            storage_thread_ref: ConversationStorageThreadRef { kind, id },
            aliases,
            identity_scope,
            migration_state,
        }
    }
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
    pub watermark: Option<ReadWatermark>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_max_message_ids: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkConversationReadRequest {
    pub conversation: ConversationReadRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watermark: Option<ReadWatermark>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_max_message_ids: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadWatermark {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_read_message_id: Option<crate::ids::MessageId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_read_thread_seq: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkThreadReadResult {
    pub updated_count: u32,
    pub remote_acknowledged: bool,
    pub partial: bool,
    pub fallback_used: bool,
    pub pending_remote_ack: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_watermark: Option<ReadWatermark>,
    pub legacy_message_ids: Vec<crate::ids::MessageId>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationReadRef {
    pub conversation_id: String,
}

impl ConversationReadRef {
    pub fn new(conversation_id: impl Into<String>) -> crate::ImResult<Self> {
        let conversation_id = conversation_id.into();
        let conversation_id = conversation_id.trim();
        if conversation_id.is_empty() {
            return Err(crate::ImError::invalid_input(
                Some("conversation_id".to_owned()),
                "conversation_id must not be empty",
            ));
        }
        Ok(Self {
            conversation_id: conversation_id.to_owned(),
        })
    }

    pub fn as_thread_ref(&self) -> crate::ImResult<ThreadRef> {
        crate::ids::ThreadId::parse(&self.conversation_id).map(ThreadRef::Thread)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendConversationTextRequest {
    pub conversation: ConversationReadRef,
    pub text: String,
    pub markdown: bool,
    pub security: MessageSecurityMode,
    pub client_message_id: Option<crate::ids::MessageId>,
    pub idempotency_key: Option<String>,
    pub wait_for_final_acceptance: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delegated_signing: Option<DelegatedSigningOptions>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendConversationPayloadRequest {
    pub conversation: ConversationReadRef,
    pub payload: serde_json::Value,
    pub security: MessageSecurityMode,
    pub client_message_id: Option<crate::ids::MessageId>,
    pub idempotency_key: Option<String>,
    pub wait_for_final_acceptance: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delegated_signing: Option<DelegatedSigningOptions>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncThreadAfterRequest {
    pub thread: ThreadRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_server_seq: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncConversationAfterRequest {
    pub conversation: ConversationReadRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_server_seq: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncThreadAfterResult {
    pub messages: Vec<Message>,
    pub next_after_server_seq: Option<String>,
    pub has_more: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncDeltaRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncDeltaResult {
    pub events_applied: u32,
    pub pages_fetched: u32,
    pub last_applied_event_seq: Option<String>,
    pub has_more: bool,
    pub snapshot_required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retention_floor_event_seq: Option<String>,
    /// Product-safe diagnostic warnings emitted while applying or hydrating
    /// this delta. Values are warning codes/details, not conversation IDs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageSyncRequest {
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageSyncStatus {
    Idle,
    Changed,
    RecoveryRequired,
    RetryableFailure,
    AuthRevoked,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommittedIncomingMessage {
    pub event_id: String,
    pub logical_message_id: String,
    pub source: String,
    pub direction: MessageDirection,
    pub message: Message,
}

/// One hydrated incoming message recovered from the active identity's local
/// projection. Ownership and account binding are derived by Core.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncomingMessageRecoveryItem {
    pub logical_message_id: String,
    pub message: Message,
}

/// Core-issued continuation token for local incoming recovery scans.
///
/// The token is intentionally opaque to hosts and is bound to the exact
/// account/device identity that created it.
#[derive(Clone, PartialEq, Eq)]
pub struct IncomingMessageRecoveryPageToken {
    pub(crate) owner_identity_id: String,
    pub(crate) account_id: String,
    pub(crate) current_did: String,
    pub(crate) protocol_device_id: String,
    pub(crate) identity_generation: String,
    pub(crate) device_auth_generation: String,
    pub(crate) timestamp: String,
    pub(crate) server_sequence_key: i64,
    pub(crate) logical_message_id: String,
}

impl std::fmt::Debug for IncomingMessageRecoveryPageToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("IncomingMessageRecoveryPageToken")
            .field(&"<opaque>")
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncomingMessageRecoveryQuery {
    pub limit: u32,
    pub page_token: Option<IncomingMessageRecoveryPageToken>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncomingMessageRecoveryPage {
    pub items: Vec<IncomingMessageRecoveryItem>,
    pub next_page_token: Option<IncomingMessageRecoveryPageToken>,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageSyncOutcome {
    pub status: MessageSyncStatus,
    pub events_applied: u32,
    pub pages_fetched: u32,
    pub messages_hydrated: u32,
    pub duplicates_skipped: u32,
    /// True when Compact Recovery intentionally selected the newest complete
    /// ordinary-history suffix within the server policy budget.
    #[serde(default)]
    pub older_history_excluded: bool,
    pub changed_conversation_ids: Vec<String>,
    pub committed_incoming_messages: Vec<CommittedIncomingMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageSyncMode {
    Uninitialized,
    Idle,
    Recovering,
    Retryable,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageSyncDirtyDomain {
    Messages,
    ReadState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageSyncRetryState {
    None,
    Pending,
    InFlight,
    Scheduled,
    PermanentFailure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageSyncLane {
    P5Device,
    P6Group,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageSyncLaneState {
    pub lane: MessageSyncLane,
    pub committed_cursor: String,
    pub pending: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_transport_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageSyncDomainStatus {
    Pending,
    Processing,
    Applied,
    Terminal,
    RejoinRequired,
    RepairRequired,
    UpgradeRequired,
    ActionRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageSyncDomainState {
    pub lane: MessageSyncLane,
    pub scope: String,
    pub retryable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_ref: Option<String>,
    pub status: MessageSyncDomainStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageSyncDiagnostics {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_success_at: Option<String>,
    pub mode: MessageSyncMode,
    pub pending_mutation_count: u32,
    pub dirty_domains: Vec<MessageSyncDirtyDomain>,
    pub retry_state: MessageSyncRetryState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_retry_at: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lanes: Vec<MessageSyncLaneState>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub domain_states: Vec<MessageSyncDomainState>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationListSnapshot {
    pub format_version: u32,
    pub im_schema_version: i64,
    pub owner_identity_id: String,
    pub owner_did: String,
    pub generated_at_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_version: Option<String>,
    pub unread_total: u32,
    pub items: Vec<ConversationSnapshotItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConversationStorePatch {
    Reset {
        owner_identity_id: String,
        owner_did: String,
        version: u64,
        unread_total: u32,
        items: Vec<ConversationSnapshotItem>,
    },
    Upsert {
        owner_identity_id: String,
        owner_did: String,
        version: u64,
        unread_total: u32,
        item: ConversationSnapshotItem,
        index: u32,
    },
    Remove {
        owner_identity_id: String,
        owner_did: String,
        version: u64,
        unread_total: u32,
        conversation_id: String,
    },
    Reorder {
        owner_identity_id: String,
        owner_did: String,
        version: u64,
        unread_total: u32,
        conversation_id: String,
        index: u32,
    },
    RepairRequired {
        owner_identity_id: String,
        owner_did: String,
        version: u64,
        unread_total: u32,
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ThreadMessageStorePatch {
    Reset {
        owner_identity_id: String,
        owner_did: String,
        version: u64,
        thread_kind: String,
        thread_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        conversation_identity: Option<ConversationIdentity>,
        items: Vec<Message>,
    },
    Upsert {
        owner_identity_id: String,
        owner_did: String,
        version: u64,
        thread_kind: String,
        thread_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        conversation_identity: Option<ConversationIdentity>,
        message: Message,
        index: u32,
    },
    Remove {
        owner_identity_id: String,
        owner_did: String,
        version: u64,
        thread_kind: String,
        thread_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        conversation_identity: Option<ConversationIdentity>,
        message_id: String,
    },
    RepairRequired {
        owner_identity_id: String,
        owner_did: String,
        version: u64,
        thread_kind: String,
        thread_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        conversation_identity: Option<ConversationIdentity>,
        reason: String,
    },
}

pub struct ConversationPatchSession {
    pub(crate) store: Arc<crate::internal::runtime_store::conversation_store::ConversationStore>,
    pub(crate) receiver: tokio::sync::broadcast::Receiver<ConversationStorePatch>,
    pub(crate) initial: VecDeque<ConversationStorePatch>,
    last_delivered_version: u64,
    closed: bool,
}

impl ConversationPatchSession {
    pub(crate) fn new(
        store: Arc<crate::internal::runtime_store::conversation_store::ConversationStore>,
        receiver: tokio::sync::broadcast::Receiver<ConversationStorePatch>,
        initial: Vec<ConversationStorePatch>,
    ) -> Self {
        Self {
            store,
            receiver,
            initial: initial.into(),
            last_delivered_version: 0,
            closed: false,
        }
    }

    pub async fn next_patch(&mut self) -> Option<ConversationStorePatch> {
        if self.closed {
            return None;
        }
        while let Some(patch) = self.initial.pop_front() {
            let version = conversation_patch_version(&patch);
            if version > self.last_delivered_version {
                self.last_delivered_version = version;
                return Some(patch);
            }
        }
        loop {
            match self.receiver.recv().await {
                Ok(patch) => {
                    let version = conversation_patch_version(&patch);
                    if version <= self.last_delivered_version {
                        continue;
                    }
                    self.last_delivered_version = version;
                    return Some(patch);
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    let patch = self.store.repair_required_patch("subscriber_lag");
                    self.last_delivered_version = conversation_patch_version(&patch);
                    return Some(patch);
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
            }
        }
    }

    pub async fn stop(&mut self) -> crate::ImResult<()> {
        self.closed = true;
        self.receiver.resubscribe();
        Ok(())
    }
}

pub struct ThreadMessagePatchSession {
    pub(crate) store: Arc<crate::internal::runtime_store::message_store::MessageStore>,
    pub(crate) receiver: tokio::sync::broadcast::Receiver<ThreadMessageStorePatch>,
    pub(crate) initial: VecDeque<ThreadMessageStorePatch>,
    pub(crate) thread: ThreadRef,
    pub(crate) limit: u32,
    last_delivered_version: u64,
    closed: bool,
}

impl ThreadMessagePatchSession {
    pub(crate) fn new(
        store: Arc<crate::internal::runtime_store::message_store::MessageStore>,
        receiver: tokio::sync::broadcast::Receiver<ThreadMessageStorePatch>,
        initial: Vec<ThreadMessageStorePatch>,
        thread: ThreadRef,
        limit: u32,
    ) -> Self {
        Self {
            store,
            receiver,
            initial: initial.into(),
            thread,
            limit,
            last_delivered_version: 0,
            closed: false,
        }
    }

    pub async fn next_patch(&mut self) -> Option<ThreadMessageStorePatch> {
        if self.closed {
            return None;
        }
        while let Some(patch) = self.initial.pop_front() {
            let version = thread_message_patch_version(&patch);
            if version > self.last_delivered_version {
                self.last_delivered_version = version;
                return Some(patch);
            }
        }
        loop {
            match self.receiver.recv().await {
                Ok(patch) if thread_patch_matches(&patch, &self.thread) => {
                    let version = thread_message_patch_version(&patch);
                    if version <= self.last_delivered_version {
                        continue;
                    }
                    self.last_delivered_version = version;
                    return Some(patch);
                }
                Ok(_) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    let patch = self.store.repair_required_patch(
                        &self.thread,
                        self.limit,
                        "subscriber_lag",
                    );
                    self.last_delivered_version = thread_message_patch_version(&patch);
                    return Some(patch);
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
            }
        }
    }

    pub async fn stop(&mut self) -> crate::ImResult<()> {
        self.closed = true;
        self.receiver.resubscribe();
        Ok(())
    }
}

fn conversation_patch_version(patch: &ConversationStorePatch) -> u64 {
    match patch {
        ConversationStorePatch::Reset { version, .. }
        | ConversationStorePatch::Upsert { version, .. }
        | ConversationStorePatch::Remove { version, .. }
        | ConversationStorePatch::Reorder { version, .. }
        | ConversationStorePatch::RepairRequired { version, .. } => *version,
    }
}

fn thread_message_patch_version(patch: &ThreadMessageStorePatch) -> u64 {
    match patch {
        ThreadMessageStorePatch::Reset { version, .. }
        | ThreadMessageStorePatch::Upsert { version, .. }
        | ThreadMessageStorePatch::Remove { version, .. }
        | ThreadMessageStorePatch::RepairRequired { version, .. } => *version,
    }
}

fn thread_patch_matches(patch: &ThreadMessageStorePatch, thread: &ThreadRef) -> bool {
    let (expected_kind, expected_id) = thread_ref_parts(thread);
    match patch {
        ThreadMessageStorePatch::Reset {
            thread_kind,
            thread_id,
            ..
        }
        | ThreadMessageStorePatch::Upsert {
            thread_kind,
            thread_id,
            ..
        }
        | ThreadMessageStorePatch::Remove {
            thread_kind,
            thread_id,
            ..
        }
        | ThreadMessageStorePatch::RepairRequired {
            thread_kind,
            thread_id,
            ..
        } => thread_kind == &expected_kind && thread_id == &expected_id,
    }
}

pub(crate) fn thread_ref_parts(thread: &ThreadRef) -> (String, String) {
    match thread {
        ThreadRef::Direct(peer) => ("direct".to_owned(), peer.as_str().to_owned()),
        ThreadRef::Group(group) => ("group".to_owned(), group.as_str().to_owned()),
        ThreadRef::Thread(thread) => ("thread".to_owned(), thread.as_str().to_owned()),
    }
}

fn canonical_conversation_id_for_storage_parts(
    kind: &str,
    id: &str,
    owner_did: Option<&str>,
) -> String {
    match kind.trim() {
        "direct" => crate::internal::local_state::owner_scope::direct_conversation_id(id),
        "group" => crate::internal::local_state::owner_scope::group_conversation_id(id),
        "thread" if id.trim().starts_with("dm:peer-scope:") => id.trim().to_owned(),
        "thread" if id.trim().starts_with("dm:") => owner_did
            .and_then(|owner_did| {
                crate::internal::local_state::owner_scope::direct_conversation_id_from_thread_alias(
                    id, owner_did,
                )
            })
            .unwrap_or_else(|| id.trim().to_owned()),
        "thread" if id.trim().starts_with("group:") => id.trim().to_owned(),
        "thread" if id.trim().starts_with("mail:") => id.trim().to_owned(),
        "thread" => id.trim().to_owned(),
        _ if !id.trim().is_empty() => id.trim().to_owned(),
        _ => "unknown".to_owned(),
    }
}

fn identity_scope_for_kind_id(kind: &str, id: &str) -> ConversationIdentityScope {
    match kind.trim() {
        "direct" => ConversationIdentityScope::Direct,
        "group" => ConversationIdentityScope::Group,
        "thread" if id.trim().starts_with("dm:") => ConversationIdentityScope::Direct,
        "thread" if id.trim().starts_with("group:") => ConversationIdentityScope::Group,
        "thread" if id.trim().starts_with("mail:") => ConversationIdentityScope::Mail,
        "thread" => ConversationIdentityScope::Thread,
        _ => ConversationIdentityScope::Unknown,
    }
}

fn canonical_thread_kind(scope: &ConversationIdentityScope) -> &'static str {
    match scope {
        ConversationIdentityScope::Direct => "direct",
        ConversationIdentityScope::Group => "group",
        ConversationIdentityScope::Mail => "mail",
        ConversationIdentityScope::Thread => "thread",
        ConversationIdentityScope::Unknown => "unknown",
    }
}

fn migration_state_for_storage_parts(
    kind: &str,
    id: &str,
    conversation_id: &str,
    scope: &ConversationIdentityScope,
) -> ConversationMigrationState {
    match (kind.trim(), scope) {
        ("direct", ConversationIdentityScope::Direct) => ConversationMigrationState::LegacyInput,
        ("group", ConversationIdentityScope::Group) => ConversationMigrationState::LegacyInput,
        ("thread", ConversationIdentityScope::Direct)
            if id.trim().starts_with("dm:") && !id.trim().starts_with("dm:peer-scope:") =>
        {
            ConversationMigrationState::LegacyInput
        }
        ("thread", _) if id.trim() == conversation_id => ConversationMigrationState::Canonical,
        ("thread", _) => ConversationMigrationState::AliasResolved,
        _ => ConversationMigrationState::Unknown,
    }
}

fn aliases_for_storage_parts(
    kind: &str,
    id: &str,
    conversation_id: &str,
    scope: &ConversationIdentityScope,
) -> Vec<ConversationAlias> {
    let source = match (kind.trim(), scope) {
        ("direct", ConversationIdentityScope::Direct) => ConversationAliasSource::LegacyDirectDid,
        ("group", ConversationIdentityScope::Group) => ConversationAliasSource::GroupStorage,
        ("thread", ConversationIdentityScope::Direct)
            if id.trim().starts_with("dm:peer-scope:") =>
        {
            ConversationAliasSource::PeerScopeStorage
        }
        ("thread", ConversationIdentityScope::Direct) if is_old_flutter_direct_alias(id) => {
            ConversationAliasSource::OldFlutterSortedDirect
        }
        ("thread", ConversationIdentityScope::Direct) if id.trim().starts_with("dm:") => {
            ConversationAliasSource::LegacyDirectDid
        }
        ("thread", _) => ConversationAliasSource::ThreadStorage,
        _ => ConversationAliasSource::Unknown,
    };
    if id.trim() == conversation_id.trim() && source == ConversationAliasSource::PeerScopeStorage {
        return Vec::new();
    }
    vec![ConversationAlias {
        kind: kind.trim().to_owned(),
        id: id.trim().to_owned(),
        source,
    }]
}

fn is_old_flutter_direct_alias(id: &str) -> bool {
    id.trim()
        .strip_prefix("dm:")
        .map(|value| value.contains(":did:"))
        .unwrap_or(false)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationSnapshotItem {
    pub conversation_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer_persona_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_group_did: Option<String>,
    pub resolution_state: ConversationResolutionState,
    pub thread_kind: String,
    pub thread_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_identity: Option<ConversationIdentity>,
    pub participants: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_message: Option<ConversationSnapshotMessage>,
    pub unread_count: u32,
    #[serde(default)]
    pub unread_mention_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_unread_mention_message_id: Option<String>,
    pub message_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_message_at: Option<String>,
    /// Durable list ordering time. Unlike `last_message_at`, this is present for
    /// conversations that have not sent their first message yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationSnapshotMessage {
    pub id: String,
    pub thread_kind: String,
    pub thread_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_identity: Option<ConversationIdentity>,
    pub direction: String,
    pub sender: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receiver: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    pub body: ConversationSnapshotMessageBody,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sent_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub received_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_sequence: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    #[serde(default)]
    pub attributes: Vec<MessageMetadataAttribute>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationSnapshotMessageBody {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_json: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unsupported_content_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conversation {
    pub conversation_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer_persona_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_group_did: Option<String>,
    pub resolution_state: ConversationResolutionState,
    pub thread: ThreadRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_identity: Option<ConversationIdentity>,
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
    /// Durable list ordering time, independent from message aggregation.
    pub activity_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationResolutionState {
    Resolved,
    LegacyUnresolved,
    BlockedConflict,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationQuery {
    pub limit: crate::ids::PageLimit,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<crate::ids::Cursor>,
    pub include_groups: bool,
    pub include_direct: bool,
    pub unread_only: bool,
}

#[cfg(test)]
mod conversation_identity_tests {
    use super::*;

    #[test]
    fn conversation_identity_direct_ref_uses_ownerless_conversation_id() {
        let peer = crate::ids::PeerRef::parse("did:example:bob", "").unwrap();
        let identity = ConversationIdentity::from_thread_ref(&ThreadRef::Direct(peer));

        assert_eq!(identity.conversation_id, "dm:did:example:bob");
        assert_eq!(identity.canonical_thread_kind, "direct");
        assert_eq!(identity.canonical_thread_id, "dm:did:example:bob");
        assert_eq!(identity.storage_thread_ref.kind, "direct");
        assert_eq!(identity.storage_thread_ref.id, "did:example:bob");
        assert_eq!(identity.identity_scope, ConversationIdentityScope::Direct);
        assert_eq!(
            identity.migration_state,
            ConversationMigrationState::LegacyInput
        );
        assert_eq!(identity.aliases.len(), 1);
        assert_eq!(
            identity.aliases[0].source,
            ConversationAliasSource::LegacyDirectDid
        );
    }

    #[test]
    fn conversation_identity_peer_scope_thread_is_canonical_direct_storage() {
        let thread_id =
            crate::messages::direct_peer_scope_thread_id("user-1", "alice.anpclaw.com").unwrap();
        let identity = ConversationIdentity::from_thread_ref(&ThreadRef::Thread(thread_id));

        assert!(identity.conversation_id.starts_with("dm:peer-scope:v1:"));
        assert_eq!(identity.canonical_thread_kind, "direct");
        assert_eq!(identity.canonical_thread_id, identity.conversation_id);
        assert_eq!(identity.storage_thread_ref.kind, "thread");
        assert_eq!(identity.storage_thread_ref.id, identity.conversation_id);
        assert_eq!(identity.identity_scope, ConversationIdentityScope::Direct);
        assert_eq!(
            identity.migration_state,
            ConversationMigrationState::Canonical
        );
        assert!(identity.aliases.is_empty());
    }

    #[test]
    fn conversation_identity_old_flutter_sorted_direct_is_alias_input() {
        let identity = ConversationIdentity::from_storage_parts(
            "thread",
            "dm:did:example:alice:did:example:bob",
        );

        assert_eq!(
            identity.conversation_id,
            "dm:did:example:alice:did:example:bob"
        );
        assert_eq!(identity.canonical_thread_kind, "direct");
        assert_eq!(identity.identity_scope, ConversationIdentityScope::Direct);
        assert_eq!(
            identity.migration_state,
            ConversationMigrationState::LegacyInput
        );
        assert_eq!(identity.aliases.len(), 1);
        assert_eq!(
            identity.aliases[0].source,
            ConversationAliasSource::OldFlutterSortedDirect
        );
    }

    #[test]
    fn conversation_identity_old_flutter_sorted_direct_resolves_with_owner() {
        let identity = ConversationIdentity::from_storage_parts_for_owner(
            "thread",
            "dm:did:example:alice:did:example:bob",
            "did:example:alice",
        );

        assert_eq!(identity.conversation_id, "dm:did:example:bob");
        assert_eq!(identity.canonical_thread_kind, "direct");
        assert_eq!(identity.canonical_thread_id, "dm:did:example:bob");
        assert_eq!(identity.storage_thread_ref.kind, "thread");
        assert_eq!(
            identity.storage_thread_ref.id,
            "dm:did:example:alice:did:example:bob"
        );
        assert_eq!(identity.identity_scope, ConversationIdentityScope::Direct);
        assert_eq!(
            identity.migration_state,
            ConversationMigrationState::LegacyInput
        );
        assert_eq!(identity.aliases.len(), 1);
        assert_eq!(
            identity.aliases[0].source,
            ConversationAliasSource::OldFlutterSortedDirect
        );
    }

    #[test]
    fn conversation_identity_group_ref_uses_group_conversation_id() {
        let group = crate::ids::GroupRef::parse("did:example:group").unwrap();
        let identity = ConversationIdentity::from_thread_ref(&ThreadRef::Group(group));

        assert_eq!(identity.conversation_id, "group:did:example:group");
        assert_eq!(identity.canonical_thread_kind, "group");
        assert_eq!(identity.identity_scope, ConversationIdentityScope::Group);
        assert_eq!(
            identity.migration_state,
            ConversationMigrationState::LegacyInput
        );
    }
}
