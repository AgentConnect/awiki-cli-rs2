mod attachment;
mod attachment_service;
mod client;
mod contact_sync;
mod group_e2ee_wire;
mod group_service;
mod group_wire;
mod proof;
mod secure_client;
mod secure_commands;
mod secure_control;
mod secure_outbox_flush;
mod service;
mod service_discovery;
mod types;
mod warnings;
mod wire;
mod ws_proxy;

pub use attachment::{
    attachment_manifest_content_type, build_attachment_commit_object_rpc_params,
    build_attachment_create_slot_rpc_params, build_attachment_download_ticket_rpc_params,
    build_attachment_manifest, build_direct_attachment_send_rpc_params,
    build_group_attachment_send_rpc_params, find_attachment_selection, manifest_content_string,
    AttachmentCommitObjectResult, AttachmentCreateSlotResult, AttachmentDownloadTicketResult,
    AttachmentSelection, PreparedAttachment,
};
pub(crate) use attachment::{find_attachment_selection_with_paging, load_attachment_file};
pub use attachment_service::download_attachment;
pub use client::Client;
pub use group_e2ee_wire::{
    build_group_e2ee_add_rpc_params, build_group_e2ee_create_rpc_params,
    build_group_e2ee_get_key_package_rpc_params,
    build_group_e2ee_get_recovery_key_package_rpc_params,
    build_group_e2ee_get_update_key_package_rpc_params, build_group_e2ee_head_rpc_params,
    build_group_e2ee_leave_request_rpc_params, build_group_e2ee_leave_rpc_params,
    build_group_e2ee_notice_rpc_params, build_group_e2ee_publish_key_package_rpc_params,
    build_group_e2ee_recover_member_rpc_params, build_group_e2ee_remove_rpc_params,
    build_group_e2ee_send_rpc_params, build_group_e2ee_update_member_rpc_params,
    GROUP_E2EE_CIPHER_CONTENT_TYPE,
};
pub use group_service::{
    add_group_member, create_group, get_group, group_members, group_messages, join_group,
    leave_group, list_groups, remove_group_member, update_group,
};
pub use group_wire::{
    build_group_add_rpc_params, build_group_create_rpc_params, build_group_get_info_rpc_params,
    build_group_get_rpc_params, build_group_join_rpc_params, build_group_leave_rpc_params,
    build_group_list_rpc_params, build_group_members_rpc_params, build_group_messages_rpc_params,
    build_group_remove_rpc_params, build_group_send_rpc_params,
    build_group_update_policy_rpc_params, build_group_update_profile_rpc_params,
    GROUP_E2EE_PROFILE, GROUP_E2EE_SECURITY_PROFILE, GROUP_E2EE_TRANSPORT_PROFILE,
};
pub use proof::{
    build_origin_proof, load_private_key_material, origin_auth_value,
    verification_method_id_from_document, ORIGIN_PROOF_SCHEME,
};
pub use secure_client::{
    local_did_document, prepare_secure_e2ee_client_for_record, resolve_secure_e2ee_local_document,
    PreparedSecureE2EEClient,
};
pub use secure_commands::{secure_drop, secure_failed, secure_retry_with_sender, secure_status};
pub use secure_control::{
    build_secure_ack_payload, build_secure_init_payload, current_secure_session_id,
    is_pending_confirmation_error, is_secure_ack_plaintext, is_secure_init_plaintext,
    queue_secure_outbox_record, secure_ack_session_id, SECURE_ACK_SYSTEM_TYPE,
    SECURE_INIT_SYSTEM_TYPE,
};
pub use secure_outbox_flush::{
    compact_warnings, flush_queued_secure_outbox_rows_plan, flush_queued_secure_outbox_with_sender,
    MarkSentOutcome, QueuedSecureOutboxRow, SecureOutboxFlushAction, SecureOutboxFlushPlan,
    SecureOutboxFlushRowOutcome, SecureOutboxSendOutcome, SecureOutboxSendRequest,
    StoreMessageOutcome,
};
pub use service::{history, inbox, mark_read, send, CommandResult};
pub use service_discovery::{
    select_attachment_rpc_service_from_document, DiscoveredAttachmentService,
};
pub use types::{
    AttachmentDownloadRequest, GroupCreateRequest, GroupE2eeProcessLeaveRequest,
    GroupE2eeRecoverMemberRequest, GroupE2eeUpdateKeyRequest, GroupGetRequest, GroupInfoRequest,
    GroupJoinRequest, GroupLeaveRequest, GroupListRequest, GroupMemberRequest, GroupMembersRequest,
    GroupMessagesRequest, GroupUpdateRequest, HistoryRequest, InboxRequest, MarkReadRequest,
    MessageError, SecureOutboxActionRequest, SecurePeerRequest, SecureStatusRequest, SendRequest,
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
pub use ws_proxy::{DirectSendResult, GroupSendResult, WSProxyTransport};
