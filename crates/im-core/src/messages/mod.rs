mod dto;
mod service;

pub use self::dto::{
    AttachmentInput, DeliveryState, HistoryQuery, InboxQuery, InboxScope, Message, MessageBody,
    MessageBodyView, MessageDeliveryOptions, MessageDirection, MessageKind, MessageMetadata,
    MessageMetadataAttribute, MessageSecurityMode, MessageTarget, SendMessageRequest,
    SendMessageResult, ThreadRef,
};
pub use self::service::MessageService;
