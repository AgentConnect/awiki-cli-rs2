mod attachment;
mod attachment_service;
mod client;
mod contact_sync;
mod group_create;
mod group_e2ee_add;
mod group_e2ee_create;
mod group_e2ee_decrypt;
mod group_e2ee_provider;
mod group_e2ee_publish;
mod group_e2ee_recover;
mod group_e2ee_remove;
mod group_e2ee_repair;
mod group_e2ee_send;
mod group_e2ee_status;
mod group_e2ee_transport;
mod group_e2ee_update;
mod group_e2ee_wire;
mod group_service;
mod group_wire;
mod group_ws;
mod history;
mod inbox;
mod mark_read;
mod proof;
mod secure_client;
mod secure_commands;
mod secure_control;
mod secure_incoming;
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
pub(crate) use contact_sync::peer_dids_for_handle_from_store;
pub use group_create::create_group;
pub(crate) use group_e2ee_decrypt::maybe_decrypt_group_messages;
pub use group_e2ee_provider::{default_mls_data_dir, ANP_MLS_BINARY_ENV};
pub use group_e2ee_publish::{publish_group_e2ee_key_package, GroupE2eePublishKeyPackageRequest};
pub use group_e2ee_recover::recover_group_e2ee_member;
pub use group_e2ee_remove::process_group_e2ee_leave_request;
pub use group_e2ee_repair::repair_group_e2ee_notices;
pub use group_e2ee_status::{
    inspect_group_e2ee_status, pull_group_e2ee_notices, GroupE2eePendingRequest,
    GroupE2eeStatusRequest,
};
pub use group_e2ee_update::update_group_e2ee_key;
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
    add_group_member, get_group, group_members, group_messages, join_group, leave_group,
    list_groups, remove_group_member, update_group,
};
pub(crate) use group_service::{
    cached_group_members, cached_group_messages, cached_group_snapshot, group_control_source,
    group_control_warnings, group_did_from_result, is_active_group_owner, mark_cached_group_left,
    normalize_group_snapshot, persist_group_members, persist_group_messages,
    persist_group_snapshot, sync_group_state, values_from_array,
};
pub use group_wire::{
    build_group_add_rpc_params, build_group_create_rpc_params, build_group_get_info_rpc_params,
    build_group_get_rpc_params, build_group_join_rpc_params, build_group_leave_rpc_params,
    build_group_list_rpc_params, build_group_members_rpc_params, build_group_messages_rpc_params,
    build_group_remove_rpc_params, build_group_send_rpc_params,
    build_group_update_policy_rpc_params, build_group_update_profile_rpc_params,
    GROUP_E2EE_PROFILE, GROUP_E2EE_SECURITY_PROFILE, GROUP_E2EE_TRANSPORT_PROFILE,
};
pub use history::history;
pub use inbox::inbox;
pub use mark_read::mark_read;
pub use proof::{
    build_origin_proof, load_private_key_material, origin_auth_value,
    verification_method_id_from_document, ORIGIN_PROOF_SCHEME,
};
pub use secure_client::{
    local_did_document, new_secure_e2ee_client_for_record, prepare_secure_e2ee_client_for_record,
    resolve_secure_e2ee_local_document, MessageServiceE2EEClient, PreparedSecureE2EEClient,
    SecureE2EERpc, SecureE2EERpcResult,
};
pub use secure_commands::{
    secure_drop, secure_failed, secure_init, secure_repair, secure_retry, secure_retry_with_sender,
    secure_status,
};
pub use secure_control::{
    build_secure_ack_payload, build_secure_init_payload, current_secure_session_id,
    is_pending_confirmation_error, is_secure_ack_plaintext, is_secure_init_plaintext,
    queue_secure_outbox_record, secure_ack_session_id, SECURE_ACK_SYSTEM_TYPE,
    SECURE_INIT_SYSTEM_TYPE,
};
pub use secure_incoming::{
    apply_direct_e2ee_processing_result, direct_e2ee_notification_from_message_view,
    direct_init_session_id_from_message, filter_displayable_direct_e2ee_messages,
    is_direct_e2ee_control_or_undisplayable, is_direct_e2ee_wire_content_type,
    maybe_decrypt_direct_e2ee_messages_with_processor,
    maybe_decrypt_direct_e2ee_messages_with_processor_and_side_effects,
    polling_direct_init_ack_request, polling_secure_ack_flush_peer, PollingDirectInitAckRequest,
    SecureIncomingProcessor, SecureIncomingRpc, SecureIncomingRpcResult,
};
pub use secure_outbox_flush::{
    compact_warnings, flush_queued_secure_outbox_rows_plan, flush_queued_secure_outbox_with_sender,
    MarkSentOutcome, QueuedSecureOutboxRow, SecureOutboxFlushAction, SecureOutboxFlushPlan,
    SecureOutboxFlushRowOutcome, SecureOutboxSendOutcome, SecureOutboxSendRequest,
    StoreMessageOutcome,
};
pub(crate) use service::{
    apply_inbox_filters, auth_session, bool_value, int_value, maybe_publish_secure_prekeys,
    merge_handle_history_messages, persist_history_messages, persist_inbox_messages,
    require_active_identity, resolved_dids_value, TargetResolution,
};
pub use service::{
    send, send_secure_direct_with_sender, CommandResult, SecureDirectSendOutcome,
    SecureDirectSendRequest,
};
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
