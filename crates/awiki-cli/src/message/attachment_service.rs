use super::group_service::{
    compact_warnings, group_send_message_id, group_storage_key, i64_option, values_from_array,
    GroupSendResult,
};
use super::service::{
    auth_session, metadata_string, require_active_identity, resolve_target, string_value,
    CommandResult,
};
use super::types::{
    AttachmentDownloadRequest, GroupMessagesRequest, HistoryRequest, MessageError, SendRequest,
    MESSAGE_RPC_ENDPOINT,
};
use super::{
    build_attachment_commit_object_rpc_params, build_attachment_create_slot_rpc_params,
    build_attachment_download_ticket_rpc_params, build_attachment_manifest,
    build_direct_attachment_send_rpc_params, build_group_attachment_send_rpc_params,
    find_attachment_selection_with_paging, load_attachment_file, manifest_content_string,
    select_attachment_rpc_service_from_document, AttachmentCreateSlotResult,
    AttachmentDownloadTicketResult, AttachmentSelection, Client, PreparedAttachment,
};
use crate::authsdk::http_status_error;
use crate::config::Resolved;
use crate::identity::types::StoredIdentity;
use crate::identity::Manager;
use crate::store::{self, MessageRecord};
use crate::transportcfg::{new_http_client, HttpRequest, Profile};
use serde_json::{json, Map, Value};
use std::path::Path;

const ATTACHMENT_DOWNLOAD_LOOKUP_PAGE_SIZE: i64 = 100;
const ATTACHMENT_MESSAGE_TYPE: &str = "attachment_manifest";

#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize)]
struct DirectAttachmentSendResult {
    #[serde(default)]
    accepted: bool,
    #[serde(default)]
    message_id: String,
    #[serde(default)]
    operation_id: String,
    #[serde(default)]
    target_did: String,
    #[serde(default)]
    accepted_at: String,
    #[serde(default)]
    final_acceptance: bool,
    #[serde(default)]
    delivery_state: String,
}

pub fn send_direct_attachment(
    resolved: &Resolved,
    manager: &Manager,
    request: SendRequest,
) -> Result<CommandResult, MessageError> {
    if request.target.trim().is_empty() {
        return Err(MessageError::TargetRequired);
    }
    if request.secure_mode.trim().eq_ignore_ascii_case("on") {
        return Err(MessageError::SecureNotSupported);
    }
    let record = require_active_identity(resolved, manager, &request.identity_name)?;
    let target = resolve_target(resolved, &request.target)?;
    let mut auth = auth_session(resolved, manager, &record)?;
    let client = Client::new(resolved)?;
    let mut warnings = attachment_transport_warnings(resolved, false);
    let (prepared, slot, manifest) =
        prepare_attachment_upload(resolved, &record, &mut auth, &request, "agent", &target.did)?;
    let params = build_direct_attachment_send_rpc_params(&record, &target.did, manifest.clone())?;
    let meta = params.get("meta").cloned().unwrap_or(Value::Null);
    let mut result: DirectAttachmentSendResult = client.authenticated_rpc_call_profile(
        Profile::RpcDefault,
        MESSAGE_RPC_ENDPOINT,
        "direct.send",
        params,
        &mut auth,
    )?;
    fill_direct_attachment_send_result(&mut result, &meta, &target.did);
    persist_direct_attachment_send_result(
        resolved,
        &record,
        &target.did,
        &target.handle,
        &request.text,
        &prepared,
        &slot,
        &manifest,
        &result,
        &mut warnings,
    )
}

pub fn send_group_attachment(
    resolved: &Resolved,
    manager: &Manager,
    request: SendRequest,
) -> Result<CommandResult, MessageError> {
    if request.group.trim().is_empty() {
        return Err(MessageError::GroupRequired);
    }
    if request.secure_mode.trim().eq_ignore_ascii_case("on") {
        return Err(MessageError::SecureNotSupported);
    }
    let record = require_active_identity(resolved, manager, &request.identity_name)?;
    let mut auth = auth_session(resolved, manager, &record)?;
    let client = Client::new(resolved)?;
    let mut warnings = attachment_transport_warnings(resolved, false);
    let (prepared, slot, manifest) = prepare_attachment_upload(
        resolved,
        &record,
        &mut auth,
        &request,
        "group",
        &request.group,
    )?;
    let params = build_group_attachment_send_rpc_params(&record, &request.group, manifest.clone())?;
    let mut result: GroupSendResult = client.authenticated_rpc_call_profile(
        Profile::RpcDefault,
        MESSAGE_RPC_ENDPOINT,
        "group.send",
        params,
        &mut auth,
    )?;
    if result.group_did.trim().is_empty() {
        result.group_did = request.group.clone();
    }
    persist_group_attachment_send_result(
        resolved,
        &record,
        &request.group,
        &request.text,
        &prepared,
        &slot,
        &manifest,
        &result,
        &mut warnings,
    )
}

pub fn download_attachment(
    resolved: &Resolved,
    manager: &Manager,
    request: AttachmentDownloadRequest,
) -> Result<CommandResult, MessageError> {
    if request.message_id.trim().is_empty() {
        return Err(MessageError::MessageIdRequired);
    }
    if request.output_path.trim().is_empty() {
        return Err(MessageError::OutputPathRequired);
    }
    if request.with.trim().is_empty() && request.group.trim().is_empty() {
        return Err(MessageError::DownloadTargetNeeded);
    }
    if !request.with.trim().is_empty() && !request.group.trim().is_empty() {
        return Err(MessageError::DownloadTargetConflict);
    }
    let record = require_active_identity(resolved, manager, &request.identity_name)?;
    let mut auth = auth_session(resolved, manager, &record)?;
    let client = Client::new(resolved)?;
    let mut warnings = attachment_transport_warnings(resolved, true);
    let (selection, message_peer) = if request.group.trim().is_empty() {
        let peer = resolve_target(resolved, &request.with)?;
        let selection = find_attachment_selection_with_paging(
            |skip| fetch_direct_attachment_page(&client, &mut auth, &record, &peer.did, skip),
            &request.message_id,
            &request.attachment_id,
        )?;
        (selection, peer.did)
    } else {
        let selection = find_attachment_selection_with_paging(
            |skip| fetch_group_attachment_page(&client, &mut auth, &record, &request.group, skip),
            &request.message_id,
            &request.attachment_id,
        )?;
        (selection, String::new())
    };
    let attachment_service =
        resolve_attachment_rpc_service(resolved, manager, &selection.sender_did)?;
    let ticket_params = build_attachment_download_ticket_rpc_params(
        &record,
        &attachment_service.service_did,
        &selection.sender_did,
        &selection.message_id,
        &request.group,
        &selection,
    )?;
    let ticket: AttachmentDownloadTicketResult = authenticated_rpc_call_url(
        &resolved.ca_bundle,
        &resolved.service_base_url,
        &attachment_service.rpc_endpoint,
        "attachment.get_download_ticket",
        ticket_params,
        &mut auth,
    )?;
    let (payload, content_type) = download_attachment_object(
        &resolved.ca_bundle,
        &selection.object_uri,
        &ticket.download_ticket_b64u,
    )?;
    let output_path = clean_output_path(&request.output_path);
    write_private_file(&output_path, &payload)?;
    Ok(CommandResult {
        data: json!({
            "action": "download_attachment",
            "message_id": selection.message_id,
            "target": {
                "kind": group_or_direct_kind(&request.group),
                "did": group_or_direct_target(&request.group, &message_peer),
            },
            "attachment": attachment_value(&selection),
            "output": {
                "path": output_path,
                "size_bytes": payload.len(),
                "content_type": content_type,
            },
        }),
        summary: format!("Downloaded attachment to {output_path}"),
        warnings: compact_warnings(&mut warnings),
    })
}

fn prepare_attachment_upload(
    resolved: &Resolved,
    record: &StoredIdentity,
    auth: &mut crate::authsdk::Session,
    request: &SendRequest,
    target_kind: &str,
    target_did: &str,
) -> Result<(PreparedAttachment, AttachmentCreateSlotResult, Value), MessageError> {
    let prepared = load_attachment_file(&request.file_path, &request.mime_type)?;
    let slot = create_attachment_slot(resolved, record, auth, target_kind, target_did, &prepared)?;
    upload_attachment_object(
        &resolved.ca_bundle,
        &slot.upload_uri,
        &slot.upload_headers,
        &prepared.payload,
    )?;
    commit_attachment_object(resolved, record, auth, &prepared, &slot)?;
    let manifest = build_attachment_manifest(&prepared, &slot, &request.text);
    Ok((prepared, slot, manifest))
}

fn create_attachment_slot(
    resolved: &Resolved,
    record: &StoredIdentity,
    auth: &mut crate::authsdk::Session,
    target_kind: &str,
    target_did: &str,
    prepared: &PreparedAttachment,
) -> Result<AttachmentCreateSlotResult, MessageError> {
    let service_did = message_service_did(resolved)?;
    let params = build_attachment_create_slot_rpc_params(
        record,
        &service_did,
        target_kind,
        target_did,
        prepared,
    )?;
    let mut slot: AttachmentCreateSlotResult = authenticated_rpc_call_url(
        &resolved.ca_bundle,
        &resolved.service_base_url,
        &crate::config::join_base_url(&resolved.service_base_url, MESSAGE_RPC_ENDPOINT),
        "attachment.create_slot",
        params,
        auth,
    )?;
    slot.request_service_did = service_did;
    Ok(slot)
}

fn commit_attachment_object(
    resolved: &Resolved,
    record: &StoredIdentity,
    auth: &mut crate::authsdk::Session,
    prepared: &PreparedAttachment,
    slot: &AttachmentCreateSlotResult,
) -> Result<(), MessageError> {
    let service_did = if slot.request_service_did.trim().is_empty() {
        message_service_did(resolved)?
    } else {
        slot.request_service_did.clone()
    };
    let params = build_attachment_commit_object_rpc_params(record, &service_did, prepared, slot)?;
    let _: Value = authenticated_rpc_call_url(
        &resolved.ca_bundle,
        &resolved.service_base_url,
        &crate::config::join_base_url(&resolved.service_base_url, MESSAGE_RPC_ENDPOINT),
        "attachment.commit_object",
        params,
        auth,
    )?;
    Ok(())
}

fn authenticated_rpc_call_url<T>(
    ca_bundle: &str,
    base_url: &str,
    request_url: &str,
    rpc_method: &str,
    params: Value,
    auth: &mut crate::authsdk::Session,
) -> Result<T, MessageError>
where
    T: serde::de::DeserializeOwned,
{
    let http_client =
        new_http_client(ca_bundle).map_err(|err| MessageError::Internal(err.to_string()))?;
    super::client::authenticated_rpc_call_url(
        &http_client,
        base_url,
        request_url,
        rpc_method,
        params,
        auth,
    )
}

fn upload_attachment_object(
    ca_bundle: &str,
    upload_uri: &str,
    headers: &Map<String, Value>,
    payload: &[u8],
) -> Result<(), MessageError> {
    let response = execute_raw_http(
        ca_bundle,
        HttpRequest::new("PUT", upload_uri).body(payload.to_vec()),
        headers,
    )?;
    if let Some(err) = http_status_error(response.status_code, &response.body) {
        return Err(MessageError::Service(
            crate::identity::wire::ServiceError::from(err),
        ));
    }
    Ok(())
}

fn download_attachment_object(
    ca_bundle: &str,
    object_uri: &str,
    download_ticket: &str,
) -> Result<(Vec<u8>, String), MessageError> {
    let request = HttpRequest::new("GET", object_uri).header(
        "Authorization",
        format!("Bearer {}", download_ticket.trim()),
    );
    let response = execute_raw_http(ca_bundle, request, &Map::new())?;
    if let Some(err) = http_status_error(response.status_code, &response.body) {
        return Err(MessageError::Service(
            crate::identity::wire::ServiceError::from(err),
        ));
    }
    let content_type = response_header_value(&response.headers, "Content-Type");
    Ok((response.body, content_type))
}

fn execute_raw_http(
    ca_bundle: &str,
    mut request: HttpRequest,
    headers: &Map<String, Value>,
) -> Result<crate::transportcfg::HttpResponse, MessageError> {
    for (key, value) in headers {
        if let Some(value) = value.as_str() {
            request.headers.push((key.clone(), value.to_string()));
        }
    }
    let client =
        new_http_client(ca_bundle).map_err(|err| MessageError::Internal(err.to_string()))?;
    client
        .execute(request)
        .map_err(|err| MessageError::Internal(err.to_string()))
}

fn fetch_direct_attachment_page(
    client: &Client,
    auth: &mut crate::authsdk::Session,
    record: &StoredIdentity,
    peer_did: &str,
    skip: i64,
) -> Result<(Vec<Value>, bool), MessageError> {
    let params = super::build_history_rpc_params(
        record,
        HistoryRequest {
            with: peer_did.to_string(),
            limit: ATTACHMENT_DOWNLOAD_LOOKUP_PAGE_SIZE,
            skip,
            ..HistoryRequest::default()
        },
    )?;
    let raw: Value = client.authenticated_rpc_call_profile(
        Profile::RpcReadHeavy,
        MESSAGE_RPC_ENDPOINT,
        "direct.get_history",
        params,
        auth,
    )?;
    Ok((
        values_from_array(raw.get("messages")),
        bool_from_value(raw.get("has_more")),
    ))
}

fn fetch_group_attachment_page(
    client: &Client,
    auth: &mut crate::authsdk::Session,
    record: &StoredIdentity,
    group_did: &str,
    skip: i64,
) -> Result<(Vec<Value>, bool), MessageError> {
    let params = super::build_group_messages_rpc_params(
        record,
        GroupMessagesRequest {
            group: group_did.to_string(),
            limit: ATTACHMENT_DOWNLOAD_LOOKUP_PAGE_SIZE,
            skip,
            ..GroupMessagesRequest::default()
        },
    )?;
    let raw: Value = client.authenticated_rpc_call_profile(
        Profile::RpcReadHeavy,
        MESSAGE_RPC_ENDPOINT,
        "group.list_messages",
        params,
        auth,
    )?;
    Ok((
        values_from_array(raw.get("messages")),
        bool_from_value(raw.get("has_more")),
    ))
}

fn resolve_attachment_rpc_service(
    resolved: &Resolved,
    manager: &Manager,
    sender_did: &str,
) -> Result<super::DiscoveredAttachmentService, MessageError> {
    let sender_did = sender_did.trim();
    if sender_did.is_empty() {
        return Err(MessageError::AttachmentSenderRequired);
    }
    let document = match crate::anpsdk::resolve_did_document_sync(sender_did, true)
        .or_else(|_| resolve_did_document_via_rustls(resolved, sender_did))
    {
        Ok(document) => document,
        Err(err) => local_identity_document(manager, sender_did)?.ok_or_else(|| {
            MessageError::Json(format!("resolve attachment sender DID document: {err}"))
        })?,
    };
    select_attachment_rpc_service_from_document(sender_did, &document)
}

fn local_identity_document(
    manager: &Manager,
    sender_did: &str,
) -> Result<Option<Value>, MessageError> {
    let index = manager.load_index()?;
    for name in index.credentials.keys() {
        let record = manager.load(name)?;
        if record.did == sender_did {
            return Ok(record.did_document);
        }
    }
    Ok(None)
}

fn resolve_did_document_via_rustls(
    resolved: &Resolved,
    did: &str,
) -> Result<Value, crate::anpsdk::AuthenticationError> {
    let url = did_document_url(did)?;
    let client = new_http_client(&resolved.ca_bundle)
        .map_err(|_| crate::anpsdk::AuthenticationError::NetworkFailure)?;
    let response = client
        .execute(HttpRequest::new("GET", url).header("Accept", "application/json"))
        .map_err(|_| crate::anpsdk::AuthenticationError::NetworkFailure)?;
    if response.status_code >= 400 {
        return Err(crate::anpsdk::AuthenticationError::NetworkFailure);
    }
    let document: Value = serde_json::from_slice(&response.body)
        .map_err(|_| crate::anpsdk::AuthenticationError::JsonFailure)?;
    if document.get("id").and_then(Value::as_str) != Some(did) {
        return Err(crate::anpsdk::AuthenticationError::InvalidDidDocument);
    }
    if did.starts_with("did:wba:") && !crate::anpsdk::validate_did_document_binding(&document, true)
    {
        return Err(crate::anpsdk::AuthenticationError::InvalidDidBinding);
    }
    Ok(document)
}

fn did_document_url(did: &str) -> Result<String, crate::anpsdk::AuthenticationError> {
    let (scheme, rest) = did
        .split_once(':')
        .ok_or(crate::anpsdk::AuthenticationError::InvalidDid)?;
    if scheme != "did" {
        return Err(crate::anpsdk::AuthenticationError::InvalidDid);
    }
    let (method, suffix) = rest
        .split_once(':')
        .ok_or(crate::anpsdk::AuthenticationError::InvalidDid)?;
    let mut parts = suffix.split(':');
    let domain = parts
        .next()
        .filter(|value| !value.is_empty())
        .ok_or(crate::anpsdk::AuthenticationError::InvalidDid)?;
    let domain = percent_decode_lossy(domain);
    let path_segments = parts.map(percent_decode_lossy).collect::<Vec<String>>();
    match method {
        "wba" | "web" => {
            if path_segments.is_empty() {
                Ok(format!("https://{domain}/.well-known/did.json"))
            } else {
                Ok(format!(
                    "https://{}/{}/did.json",
                    domain,
                    path_segments.join("/")
                ))
            }
        }
        _ => Err(crate::anpsdk::AuthenticationError::InvalidDid),
    }
}

fn percent_decode_lossy(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Some(byte) = hex_pair(bytes[index + 1], bytes[index + 2]) {
                decoded.push(byte);
                index += 3;
                continue;
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn hex_pair(high: u8, low: u8) -> Option<u8> {
    Some(hex_value(high)? * 16 + hex_value(low)?)
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn persist_direct_attachment_send_result(
    resolved: &Resolved,
    record: &StoredIdentity,
    target_did: &str,
    target_handle: &str,
    caption: &str,
    prepared: &PreparedAttachment,
    slot: &AttachmentCreateSlotResult,
    manifest: &Value,
    result: &DirectAttachmentSendResult,
    warnings: &mut Vec<String>,
) -> Result<CommandResult, MessageError> {
    let connection =
        store::open(&resolved.paths).map_err(|err| MessageError::Internal(err.to_string()))?;
    store::ensure_schema(&connection).map_err(|err| MessageError::Internal(err.to_string()))?;
    if let Err(err) = store::store_message(
        &connection,
        MessageRecord {
            msg_id: result.message_id.clone(),
            owner_did: record.did.clone(),
            thread_id: store::make_thread_id(&record.did, target_did, ""),
            direction: 1,
            sender_did: record.did.clone(),
            receiver_did: target_did.to_string(),
            content_type: super::attachment_manifest_content_type().to_string(),
            content: manifest_content_string(manifest),
            sent_at: result.accepted_at.clone(),
            is_read: true,
            metadata: metadata_string(json!({
                "delivery_state": result.delivery_state,
                "operation_id": result.operation_id,
                "target_handle": target_handle,
                "attachment_id": slot.attachment_id,
                "object_uri": slot.object_uri,
                "caption": caption,
            })),
            credential_name: record.identity_name.clone(),
            ..MessageRecord::default()
        },
    ) {
        warnings.push(format!("Failed to persist local message: {err}"));
    }
    Ok(CommandResult {
        data: json!({
            "action": "send_attachment",
            "target": {
                "did": target_did,
                "handle": target_handle,
                "kind": "direct",
            },
            "message": attachment_message_value(&result.message_id, &result.accepted_at, caption),
            "attachment": prepared_attachment_value(prepared, slot),
            "delivery": result,
        }),
        summary: "Sent a direct attachment message".to_string(),
        warnings: compact_warnings(warnings),
    })
}

fn persist_group_attachment_send_result(
    resolved: &Resolved,
    record: &StoredIdentity,
    group_did: &str,
    caption: &str,
    prepared: &PreparedAttachment,
    slot: &AttachmentCreateSlotResult,
    manifest: &Value,
    result: &GroupSendResult,
    warnings: &mut Vec<String>,
) -> Result<CommandResult, MessageError> {
    let message_id = group_send_message_id(group_did, result);
    let connection =
        store::open(&resolved.paths).map_err(|err| MessageError::Internal(err.to_string()))?;
    store::ensure_schema(&connection).map_err(|err| MessageError::Internal(err.to_string()))?;
    if let Err(err) = store::store_message(
        &connection,
        MessageRecord {
            msg_id: message_id.clone(),
            owner_did: record.did.clone(),
            thread_id: store::make_thread_id(&record.did, "", &group_storage_key(group_did)),
            direction: 1,
            sender_did: record.did.clone(),
            group_id: group_storage_key(group_did),
            group_did: group_did.to_string(),
            content_type: super::attachment_manifest_content_type().to_string(),
            content: manifest_content_string(manifest),
            sent_at: result.accepted_at.clone(),
            is_read: true,
            metadata: metadata_string(json!({
                "group_event_seq": result.group_event_seq,
                "group_state_version": result.group_state_version,
                "operation_id": result.operation_id,
                "attachment_id": slot.attachment_id,
                "object_uri": slot.object_uri,
                "caption": caption,
            })),
            credential_name: record.identity_name.clone(),
            ..MessageRecord::default()
        },
    ) {
        warnings.push(format!("Failed to persist local group message: {err}"));
    }
    if let Err(err) = store::touch_group_after_message(
        &connection,
        &record.did,
        &group_storage_key(group_did),
        group_did,
        &result.accepted_at,
        i64_option(Some(&Value::String(result.group_event_seq.clone()))),
        &record.identity_name,
        &metadata_string(json!({ "group_state_version": result.group_state_version })),
    ) {
        warnings.push(format!("Failed to update group cache: {err}"));
    }
    Ok(CommandResult {
        data: json!({
            "action": "send_attachment",
            "target": {
                "kind": "group",
                "did": group_did,
            },
            "message": attachment_message_value(&message_id, &result.accepted_at, caption),
            "attachment": prepared_attachment_value(prepared, slot),
            "delivery": result,
        }),
        summary: "Sent a group attachment message".to_string(),
        warnings: compact_warnings(warnings),
    })
}

fn fill_direct_attachment_send_result(
    result: &mut DirectAttachmentSendResult,
    meta: &Value,
    target_did: &str,
) {
    if result.message_id.trim().is_empty() {
        result.message_id = string_value(meta.get("message_id"));
    }
    if result.operation_id.trim().is_empty() {
        result.operation_id = string_value(meta.get("operation_id"));
    }
    if result.target_did.trim().is_empty() {
        result.target_did = target_did.to_string();
    }
}

fn attachment_message_value(message_id: &str, sent_at: &str, caption: &str) -> Value {
    json!({
        "id": message_id,
        "type": ATTACHMENT_MESSAGE_TYPE,
        "content_type": super::attachment_manifest_content_type(),
        "caption": caption,
        "secure": false,
        "sent_at": sent_at,
    })
}

fn prepared_attachment_value(
    prepared: &PreparedAttachment,
    slot: &AttachmentCreateSlotResult,
) -> Value {
    json!({
        "attachment_id": slot.attachment_id,
        "filename": prepared.filename,
        "mime_type": prepared.mime_type,
        "size": prepared.size_string,
        "digest": {
            "alg": "sha-256",
            "value_b64u": prepared.digest_b64u,
        },
        "object_uri": slot.object_uri,
    })
}

fn attachment_value(selection: &AttachmentSelection) -> Value {
    json!({
        "attachment_id": selection.attachment_id,
        "filename": selection.filename,
        "mime_type": selection.mime_type,
        "size": selection.size,
        "digest": {
            "alg": "sha-256",
            "value_b64u": selection.digest_b64u,
        },
        "object_uri": selection.object_uri,
        "sender_did": selection.sender_did,
        "caption": selection.caption,
    })
}

fn message_service_did(resolved: &Resolved) -> Result<String, MessageError> {
    let service_did = resolved.anp_service_did.trim();
    if service_did.is_empty() {
        Err(MessageError::MissingMessageServiceDid)
    } else {
        Ok(service_did.to_string())
    }
}

fn attachment_transport_warnings(resolved: &Resolved, download: bool) -> Vec<String> {
    if resolved.runtime_mode.trim() != "websocket" {
        return Vec::new();
    }
    if download {
        vec![
            "Attachment downloads use HTTP transport even when runtime.mode is websocket."
                .to_string(),
        ]
    } else {
        vec![
            "Attachment messages use HTTP transport even when runtime.mode is websocket."
                .to_string(),
        ]
    }
}

fn bool_from_value(value: Option<&Value>) -> bool {
    matches!(value, Some(Value::Bool(true)))
}

fn response_header_value(headers: &[(String, String)], name: &str) -> String {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.clone())
        .unwrap_or_default()
}

fn clean_output_path(output_path: &str) -> String {
    Path::new(output_path.trim()).to_string_lossy().into_owned()
}

fn write_private_file(output_path: &str, payload: &[u8]) -> Result<(), MessageError> {
    let path = Path::new(output_path);
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent).map_err(|err| MessageError::Internal(err.to_string()))?;
        set_private_dir_mode(parent)?;
    }
    std::fs::write(path, payload).map_err(|err| MessageError::Internal(err.to_string()))?;
    set_private_file_mode(path)?;
    Ok(())
}

#[cfg(unix)]
fn set_private_dir_mode(path: &Path) -> Result<(), MessageError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
        .map_err(|err| MessageError::Internal(err.to_string()))
}

#[cfg(not(unix))]
fn set_private_dir_mode(_path: &Path) -> Result<(), MessageError> {
    Ok(())
}

#[cfg(unix)]
fn set_private_file_mode(path: &Path) -> Result<(), MessageError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|err| MessageError::Internal(err.to_string()))
}

#[cfg(not(unix))]
fn set_private_file_mode(_path: &Path) -> Result<(), MessageError> {
    Ok(())
}

fn group_or_direct_kind(group_did: &str) -> &'static str {
    if group_did.trim().is_empty() {
        "direct"
    } else {
        "group"
    }
}

fn group_or_direct_target(group_did: &str, peer_did: &str) -> String {
    if group_did.trim().is_empty() {
        peer_did.to_string()
    } else {
        group_did.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::did_document_url;

    #[test]
    fn did_document_url_matches_anp_wba_and_web_resolution_paths() {
        assert_eq!(
            did_document_url("did:wba:awiki.info").unwrap(),
            "https://awiki.info/.well-known/did.json"
        );
        assert_eq!(
            did_document_url("did:wba:awiki.info:alice:e1_abc").unwrap(),
            "https://awiki.info/alice/e1_abc/did.json"
        );
        assert_eq!(
            did_document_url("did:web:example.com:user:alice").unwrap(),
            "https://example.com/user/alice/did.json"
        );
        assert_eq!(
            did_document_url("did:wba:example.com:user%20name:e1_abc").unwrap(),
            "https://example.com/user name/e1_abc/did.json"
        );
        assert!(did_document_url("did:key:z6mk").is_err());
    }
}
