mod dto;
mod mention;
mod service;

pub use self::dto::{
    direct_peer_scope_thread_id, Conversation, ConversationQuery, DelegatedSigningOptions,
    DeliveryState, HistoryQuery, InboxAuth, InboxHistoryOptions, InboxQuery, InboxScope,
    MarkReadResult, MarkThreadReadRequest, MarkThreadReadResult, Message, MessageBody,
    MessageBodyView, MessageDeliveryOptions, MessageDirection, MessageKind, MessageMetadata,
    MessageMetadataAttribute, MessagePage, MessageRetryAction, MessageRetryPlan,
    MessageSecurityMode, MessageSecurityPolicy, MessageSendState, MessageSendStateKind,
    MessageTarget, ScopedInboxToken, SendMessageRequest, SendMessageResult, ThreadRef,
};

pub use self::mention::{
    is_message_mention_payload, parse_message_mention_payload, validate_message_mention_payload,
    MessageMention, MessageMentionPayload, MessageMentionRange, MessageMentionRangeUnit,
    MessageMentionRole, MessageMentionSelector, MessageMentionTarget,
};
pub use self::service::MessageService;
pub use crate::attachments::AttachmentInput;
