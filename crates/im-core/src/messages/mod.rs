mod dto;
mod service;

pub use self::dto::{
    Conversation, ConversationQuery, DeliveryState, HistoryQuery, InboxQuery, InboxScope,
    MarkReadResult, Message, MessageBody, MessageBodyView, MessageDeliveryOptions,
    MessageDirection, MessageKind, MessageMetadata, MessageMetadataAttribute, MessageRetryAction,
    MessageRetryPlan, MessageSecurityMode, MessageSecurityPolicy, MessageSendState,
    MessageSendStateKind, MessageTarget, SendMessageRequest, SendMessageResult, ThreadRef,
};
pub use self::service::MessageService;
pub use crate::attachments::AttachmentInput;
