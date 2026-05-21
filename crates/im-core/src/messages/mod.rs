mod dto;
mod service;

pub use self::dto::{
    AttachmentInput, Conversation, ConversationQuery, DeliveryState, HistoryQuery, InboxQuery,
    InboxScope, MarkReadResult, Message, MessageBody, MessageBodyView, MessageDeliveryOptions,
    MessageDirection, MessageKind, MessageMetadata, MessageMetadataAttribute, MessageSecurityMode,
    MessageTarget, SendMessageRequest, SendMessageResult, ThreadRef,
};
pub use self::service::MessageService;
