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
    pub delegated_signing: Option<DartDelegatedSigningOptions>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartSendPayloadRequest {
    pub target: DartMessageTarget,
    pub payload_json: String,
    pub security: DartMessageSecurityMode,
    pub client_message_id: Option<String>,
    pub idempotency_key: Option<String>,
    pub wait_for_final_acceptance: bool,
    pub delegated_signing: Option<DartDelegatedSigningOptions>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartSendConversationTextRequest {
    pub conversation: DartConversationReadRef,
    pub text: String,
    pub markdown: bool,
    pub security: DartMessageSecurityMode,
    pub client_message_id: Option<String>,
    pub idempotency_key: Option<String>,
    pub wait_for_final_acceptance: bool,
    pub delegated_signing: Option<DartDelegatedSigningOptions>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartSendConversationPayloadRequest {
    pub conversation: DartConversationReadRef,
    pub payload_json: String,
    pub security: DartMessageSecurityMode,
    pub client_message_id: Option<String>,
    pub idempotency_key: Option<String>,
    pub wait_for_final_acceptance: bool,
    pub delegated_signing: Option<DartDelegatedSigningOptions>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartDelegatedSigningOptions {
    pub logical_sender_did: Option<String>,
    pub signing_verification_method: Option<String>,
    pub signing_key_ref: Option<String>,
    pub actor_agent_did: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartInboxHistoryOptions {
    pub inbox_owner_did: Option<String>,
    pub inbox_auth_verification_method: Option<String>,
    pub inbox_auth_key_ref: Option<String>,
    pub inbox_auth: Option<DartInboxAuth>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DartInboxAuth {
    ScopedInboxToken { token: DartScopedInboxToken },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartScopedInboxToken {
    pub token: String,
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
    pub payload_json: Option<String>,
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
    pub conversation_identity: Option<DartConversationIdentity>,
    pub attributes: Vec<DartMessageMetadataAttribute>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartConversationIdentity {
    pub conversation_id: String,
    pub canonical_thread_kind: String,
    pub canonical_thread_id: String,
    pub storage_thread_ref: DartConversationStorageThreadRef,
    pub aliases: Vec<DartConversationAlias>,
    pub identity_scope: DartConversationIdentityScope,
    pub migration_state: DartConversationMigrationState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartConversationStorageThreadRef {
    pub kind: String,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartConversationAlias {
    pub kind: String,
    pub id: String,
    pub source: DartConversationAliasSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DartConversationAliasSource {
    LegacyDirectDid,
    OldFlutterSortedDirect,
    PeerScopeStorage,
    GroupStorage,
    ThreadStorage,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DartConversationIdentityScope {
    Direct,
    Group,
    Thread,
    Mail,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DartConversationMigrationState {
    Canonical,
    AliasResolved,
    LegacyInput,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartMessageMetadataAttribute {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartMessage {
    pub id: String,
    pub conversation_id: String,
    pub sender_peer_persona_id: Option<String>,
    pub sender_did_snapshot: String,
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
    pub conversation_id: String,
    pub peer_persona_id: Option<String>,
    pub canonical_group_did: Option<String>,
    pub resolution_state: DartConversationResolutionState,
    pub thread_kind: String,
    pub thread_id: String,
    pub conversation_identity: Option<DartConversationIdentity>,
    pub title: Option<String>,
    pub participants: Vec<String>,
    pub last_message: Option<DartMessage>,
    pub unread_count: u32,
    pub unread_mention_count: u32,
    pub first_unread_mention_message_id: Option<String>,
    pub message_count: u32,
    pub last_message_at: Option<String>,
    pub activity_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DartConversationResolutionState {
    Resolved,
    LegacyUnresolved,
    BlockedConflict,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartReadWatermark {
    pub last_read_message_id: Option<String>,
    pub last_read_thread_seq: Option<String>,
    pub read_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartMarkThreadReadResult {
    pub updated_count: u32,
    pub remote_acknowledged: bool,
    pub partial: bool,
    pub fallback_used: bool,
    pub pending_remote_ack: bool,
    pub effective_watermark: Option<DartReadWatermark>,
    pub legacy_message_ids: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartSyncDeltaRequest {
    pub limit: Option<u32>,
    pub device_id: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartSyncDeltaResult {
    pub events_applied: u32,
    pub pages_fetched: u32,
    pub last_applied_event_seq: Option<String>,
    pub has_more: bool,
    pub snapshot_required: bool,
    pub retention_floor_event_seq: Option<String>,
    pub hydration_required_conversation_ids: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartConversationListSnapshot {
    pub format_version: u32,
    pub im_schema_version: i64,
    pub owner_identity_id: String,
    pub owner_did: String,
    pub generated_at_ms: i64,
    pub summary_version: Option<String>,
    pub unread_total: u32,
    pub items: Vec<DartConversationSnapshotItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DartConversationStorePatch {
    Reset {
        owner_identity_id: String,
        owner_did: String,
        version: u64,
        unread_total: u32,
        items: Vec<DartConversationSnapshotItem>,
    },
    Upsert {
        owner_identity_id: String,
        owner_did: String,
        version: u64,
        unread_total: u32,
        item: DartConversationSnapshotItem,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DartThreadMessageStorePatch {
    Reset {
        owner_identity_id: String,
        owner_did: String,
        version: u64,
        thread_kind: String,
        thread_id: String,
        conversation_identity: Option<DartConversationIdentity>,
        items: Vec<DartMessage>,
    },
    Upsert {
        owner_identity_id: String,
        owner_did: String,
        version: u64,
        thread_kind: String,
        thread_id: String,
        conversation_identity: Option<DartConversationIdentity>,
        message: DartMessage,
        index: u32,
    },
    Remove {
        owner_identity_id: String,
        owner_did: String,
        version: u64,
        thread_kind: String,
        thread_id: String,
        conversation_identity: Option<DartConversationIdentity>,
        message_id: String,
    },
    RepairRequired {
        owner_identity_id: String,
        owner_did: String,
        version: u64,
        thread_kind: String,
        thread_id: String,
        conversation_identity: Option<DartConversationIdentity>,
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartConversationSnapshotItem {
    pub conversation_id: String,
    pub peer_persona_id: Option<String>,
    pub canonical_group_did: Option<String>,
    pub resolution_state: DartConversationResolutionState,
    pub thread_kind: String,
    pub thread_id: String,
    pub title: Option<String>,
    pub conversation_identity: Option<DartConversationIdentity>,
    pub participants: Vec<String>,
    pub last_message: Option<DartConversationSnapshotMessage>,
    pub unread_count: u32,
    pub unread_mention_count: u32,
    pub first_unread_mention_message_id: Option<String>,
    pub message_count: u32,
    pub last_message_at: Option<String>,
    pub activity_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartConversationSnapshotMessage {
    pub id: String,
    pub thread_kind: String,
    pub thread_id: String,
    pub conversation_identity: Option<DartConversationIdentity>,
    pub direction: String,
    pub sender: String,
    pub receiver: Option<String>,
    pub group: Option<String>,
    pub body: DartConversationSnapshotMessageBody,
    pub sent_at: Option<String>,
    pub received_at: Option<String>,
    pub server_sequence: Option<i64>,
    pub content_type: Option<String>,
    pub attributes: Vec<DartMessageMetadataAttribute>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartConversationSnapshotMessageBody {
    pub text: Option<String>,
    pub kind: Option<String>,
    pub payload_json: Option<String>,
    pub unsupported_content_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartConversationReadRef {
    pub conversation_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartMarkConversationReadRequest {
    pub conversation: DartConversationReadRef,
    pub watermark: Option<DartReadWatermark>,
    pub fallback_max_message_ids: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartSyncThreadAfterRequest {
    pub thread: DartThreadRef,
    pub after_server_seq: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartSyncConversationAfterRequest {
    pub conversation: DartConversationReadRef,
    pub after_server_seq: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartSyncThreadAfterResult {
    pub messages: Vec<DartMessage>,
    pub next_after_server_seq: Option<String>,
    pub has_more: bool,
    pub warnings: Vec<String>,
}
