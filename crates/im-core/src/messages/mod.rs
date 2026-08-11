mod agent_message;
#[cfg(test)]
mod agent_message_tests;
mod dto;
mod mention;
mod service;
mod v2_product;

pub(crate) use self::agent_message::{
    classify_message_payload_for_projection, sanitize_projected_json_payload,
};
pub use self::agent_message::{
    project_agent_message_payload, project_agent_message_payload_for_scope,
    validate_agent_message_send_request, AgentMessageAction, AgentMessageKind,
    AgentMessageProjection, AgentMessageProjectionScope, AgentMessageRequestedLevel,
    AgentMessageV1, AGENT_MESSAGE_SCHEMA_V1, AGENT_MESSAGE_V1_MAX_COMPACT_BYTES,
    AGENT_MESSAGE_V1_MAX_DETAIL_CHARS, AGENT_MESSAGE_V1_MAX_SUMMARY_CHARS,
    AGENT_MESSAGE_V1_MAX_TASK_NAME_CHARS,
};
pub(crate) use self::dto::thread_ref_parts;
pub use self::dto::{
    direct_peer_scope_thread_id, CommittedIncomingMessage, Conversation, ConversationAlias,
    ConversationAliasSource, ConversationIdentity, ConversationIdentityScope,
    ConversationListSnapshot, ConversationMigrationState, ConversationPatchSession,
    ConversationQuery, ConversationReadRef, ConversationResolutionState, ConversationSnapshotItem,
    ConversationSnapshotMessage, ConversationSnapshotMessageBody, ConversationStorageThreadRef,
    ConversationStorePatch, DelegatedSigningOptions, DeliveryState, HistoryQuery, InboxAuth,
    InboxHistoryOptions, InboxQuery, InboxScope, IncomingMessageRecoveryItem,
    IncomingMessageRecoveryPage, IncomingMessageRecoveryPageToken, IncomingMessageRecoveryQuery,
    LocalHistoryQuery, MarkConversationReadRequest, MarkReadResult, MarkThreadReadRequest,
    MarkThreadReadResult, Message, MessageBody, MessageBodyView, MessageDeliveryOptions,
    MessageDirection, MessageKind, MessageMetadata, MessageMetadataAttribute, MessagePage,
    MessageRetryAction, MessageRetryPlan, MessageSecurityMode, MessageSecurityPolicy,
    MessageSendState, MessageSendStateKind, MessageSyncDiagnostics, MessageSyncDirtyDomain,
    MessageSyncMode, MessageSyncOutcome, MessageSyncRequest, MessageSyncRetryState,
    MessageSyncStatus, MessageTarget, ReadWatermark, ScopedInboxToken,
    SendConversationPayloadRequest, SendConversationTextRequest, SendMessageRequest,
    SendMessageResult, SyncConversationAfterRequest, SyncDeltaRequest, SyncDeltaResult,
    SyncThreadAfterRequest, SyncThreadAfterResult, ThreadMessagePatchSession,
    ThreadMessageStorePatch, ThreadRef,
};
pub(crate) use self::service::{
    normalize_direct_send_result_for_peer_scope, resolve_conversation_send_target,
};

pub use self::mention::{
    is_message_mention_payload, parse_message_mention_payload, validate_message_mention_payload,
    MessageMention, MessageMentionPayload, MessageMentionRange, MessageMentionRangeUnit,
    MessageMentionRole, MessageMentionSelector, MessageMentionTarget,
};
pub use self::service::{MessageService, LOCAL_INCOMING_RECOVERY_LIMIT_MAX};
pub use crate::attachments::AttachmentInput;
