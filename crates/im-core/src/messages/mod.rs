mod dto;
mod mention;
mod service;
mod v2_product;

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
