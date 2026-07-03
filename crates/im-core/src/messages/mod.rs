mod dto;
mod mention;
mod service;

pub(crate) use self::dto::thread_ref_parts;
pub use self::dto::{
    direct_peer_scope_thread_id, Conversation, ConversationListSnapshot, ConversationPatchSession,
    ConversationQuery, ConversationSnapshotItem, ConversationSnapshotMessage,
    ConversationSnapshotMessageBody, ConversationStorePatch, DelegatedSigningOptions,
    DeliveryState, HistoryQuery, InboxAuth, InboxHistoryOptions, InboxQuery, InboxScope,
    LocalHistoryQuery, MarkReadResult, MarkThreadReadRequest, MarkThreadReadResult, Message,
    MessageBody, MessageBodyView, MessageDeliveryOptions, MessageDirection, MessageKind,
    MessageMetadata, MessageMetadataAttribute, MessagePage, MessageRetryAction, MessageRetryPlan,
    MessageSecurityMode, MessageSecurityPolicy, MessageSendState, MessageSendStateKind,
    MessageTarget, ReadWatermark, ScopedInboxToken, SendMessageRequest, SendMessageResult,
    SyncDeltaRequest, SyncDeltaResult, SyncThreadAfterRequest, SyncThreadAfterResult,
    ThreadMessagePatchSession, ThreadMessageStorePatch, ThreadRef,
};
pub(crate) use self::service::normalize_direct_send_result_for_peer_scope;

pub use self::mention::{
    is_message_mention_payload, parse_message_mention_payload, validate_message_mention_payload,
    MessageMention, MessageMentionPayload, MessageMentionRange, MessageMentionRangeUnit,
    MessageMentionRole, MessageMentionSelector, MessageMentionTarget,
};
pub use self::service::MessageService;
pub use crate::attachments::AttachmentInput;
