mod attachment;
mod proof;
mod service_discovery;
mod types;
mod warnings;
mod wire;

pub use attachment::{
    attachment_manifest_content_type, build_attachment_commit_object_rpc_params,
    build_attachment_create_slot_rpc_params, build_attachment_download_ticket_rpc_params,
    build_attachment_manifest, build_direct_attachment_send_rpc_params,
    build_group_attachment_send_rpc_params, find_attachment_selection, manifest_content_string,
    AttachmentCommitObjectResult, AttachmentCreateSlotResult, AttachmentSelection,
    PreparedAttachment,
};
pub use proof::{
    build_origin_proof, load_private_key_material, origin_auth_value,
    verification_method_id_from_document, ORIGIN_PROOF_SCHEME,
};
pub use service_discovery::{
    select_attachment_rpc_service_from_document, DiscoveredAttachmentService,
};
pub use types::{
    AttachmentDownloadRequest, HistoryRequest, InboxRequest, MarkReadRequest, MessageError,
    SecureOutboxActionRequest, SecurePeerRequest, SecureStatusRequest, SendRequest,
    MESSAGE_RPC_ENDPOINT, MESSAGE_WS_ENDPOINT,
};
pub use warnings::{
    websocket_cache_fallback_warning, websocket_http_fallback_warning, ERR_TRANSPORT_UNAVAILABLE,
};
pub use wire::{
    build_direct_send_rpc_params, build_direct_text_payload, build_history_rpc_params,
    build_inbox_rpc_params, build_mark_read_rpc_params, content_type_for_message_type,
    DirectPayload,
};
