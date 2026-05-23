use serde_json::Value;

use crate::message::types::MessageError;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiscoveredAttachmentService {
    pub sender_did: String,
    pub service_did: String,
    pub rpc_endpoint: String,
}

pub fn select_attachment_rpc_service_from_document(
    sender_did: &str,
    document: &Value,
) -> Result<DiscoveredAttachmentService, MessageError> {
    im_core::compat::attachments::select_attachment_rpc_service_from_document(sender_did, document)
        .map(|service| DiscoveredAttachmentService {
            sender_did: service.sender_did,
            service_did: service.service_did,
            rpc_endpoint: service.rpc_endpoint,
        })
        .map_err(attachment_discovery_error)
}

fn attachment_discovery_error(err: im_core::ImError) -> MessageError {
    match err {
        im_core::ImError::InvalidInput { field, message }
            if field.as_deref() == Some("service_endpoint")
                && message.contains("missing protocol scheme") =>
        {
            MessageError::InvalidAttachmentServiceEndpoint("missing protocol scheme".to_string())
        }
        im_core::ImError::InvalidInput { message, .. } => MessageError::Json(message),
        err => MessageError::Json(err.to_string()),
    }
}
