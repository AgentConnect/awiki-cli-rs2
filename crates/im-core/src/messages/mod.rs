mod dto;
mod service;

pub use self::dto::{
    direct_peer_scope_thread_id, Conversation, ConversationQuery, DeliveryState, HistoryQuery,
    InboxQuery, InboxScope, MarkReadResult, Message, MessageBody, MessageBodyView,
    MessageDeliveryOptions, MessageDirection, MessageKind, MessageMetadata,
    MessageMetadataAttribute, MessagePage, MessageRetryAction, MessageRetryPlan,
    MessageSecurityMode, MessageSecurityPolicy, MessageSendState, MessageSendStateKind,
    MessageTarget, SendMessageRequest, SendMessageResult, ThreadRef,
};
pub use self::service::MessageService;
pub use crate::attachments::AttachmentInput;
