#[cfg(feature = "sqlite")]
use serde_json::{json, Value};

#[cfg(feature = "sqlite")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RealtimeMessageLocalProjectionContext {
    pub owner_identity_id: String,
    pub owner_did: String,
    pub credential_name: String,
}

#[cfg(feature = "sqlite")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RealtimeMessageLocalProjection {
    record: crate::internal::local_state::messages::MessageRecord,
}

#[cfg(feature = "sqlite")]
impl RealtimeMessageLocalProjection {
    pub(crate) fn into_record(self) -> crate::internal::local_state::messages::MessageRecord {
        self.record
    }

    pub fn msg_id(&self) -> &str {
        &self.record.msg_id
    }

    pub fn sender_did(&self) -> &str {
        &self.record.sender_did
    }

    pub fn group_did(&self) -> &str {
        &self.record.group_did
    }
}

#[cfg(feature = "sqlite")]
pub fn plan_realtime_message_local_projection(
    context: &RealtimeMessageLocalProjectionContext,
    message: &crate::messages::Message,
    attachment_summary: Option<&crate::realtime::AttachmentMessageSummary>,
    download_action: Option<&crate::realtime::AttachmentDownloadAction>,
    warnings: &[String],
) -> Option<RealtimeMessageLocalProjection> {
    let owner_did = owner_did_for_message(message, context);
    if owner_did.is_empty() || message.id.as_str().trim().is_empty() {
        return None;
    }
    let sender_did = message.sender.as_str().trim().to_string();
    let receiver_did = message
        .receiver
        .as_ref()
        .map(|peer| peer.as_str().trim().to_string())
        .unwrap_or_default();
    let group_did = group_did_for_message(message);
    let peer_did = if sender_did == owner_did {
        receiver_did.as_str()
    } else {
        sender_did.as_str()
    };
    let has_thread_peer = !peer_did.trim().is_empty();
    let conversation_id = conversation_id_for_message(message);
    if conversation_id.trim().is_empty() || (group_did.trim().is_empty() && !has_thread_peer) {
        return None;
    }

    Some(RealtimeMessageLocalProjection {
        record: crate::internal::local_state::messages::MessageRecord {
            msg_id: message.id.as_str().to_string(),
            owner_identity_id: context.owner_identity_id.trim().to_string(),
            owner_did,
            conversation_id: conversation_id.clone(),
            thread_id: conversation_id,
            direction: direction_value(&message.direction),
            sender_did,
            receiver_did,
            group_id: group_did.clone(),
            group_did,
            content_type: message_content_type(message),
            content: message_content(message),
            server_seq: message.metadata.server_sequence,
            sent_at: message
                .sent_at
                .clone()
                .or_else(|| message.received_at.clone())
                .unwrap_or_else(now_utc_like),
            is_read: matches!(
                message.direction,
                crate::messages::MessageDirection::Outgoing
            ),
            metadata: message_metadata_value(
                message,
                attachment_summary,
                download_action,
                warnings,
            ),
            credential_name: context.credential_name.trim().to_string(),
            ..crate::internal::local_state::messages::MessageRecord::default()
        },
    })
}

#[cfg(feature = "sqlite")]
pub fn apply_realtime_message_local_projection(
    connection: &rusqlite::Connection,
    projection: RealtimeMessageLocalProjection,
) -> crate::ImResult<()> {
    crate::internal::local_state::messages::upsert_message(connection, &projection.record)
}

#[cfg(feature = "sqlite")]
fn owner_did_for_message(
    message: &crate::messages::Message,
    context: &RealtimeMessageLocalProjectionContext,
) -> String {
    if let Some(receiver) = message.receiver.as_ref() {
        let receiver = receiver.as_str().trim();
        if !receiver.is_empty() {
            return receiver.to_string();
        }
    }
    context.owner_did.trim().to_string()
}

#[cfg(feature = "sqlite")]
fn conversation_id_for_message(message: &crate::messages::Message) -> String {
    match &message.thread {
        crate::messages::ThreadRef::Group(group) => {
            crate::internal::message_runtime::local_projection::group_conversation_id(
                group.as_str(),
            )
        }
        crate::messages::ThreadRef::Direct(peer) => {
            crate::internal::message_runtime::local_projection::direct_conversation_id(
                peer.as_str(),
            )
        }
        crate::messages::ThreadRef::Thread(thread) => thread.as_str().to_string(),
    }
}

#[cfg(feature = "sqlite")]
fn group_did_for_message(message: &crate::messages::Message) -> String {
    message
        .group
        .as_ref()
        .map(|group| group.as_str().trim().to_string())
        .or_else(|| match &message.thread {
            crate::messages::ThreadRef::Group(group) => Some(group.as_str().trim().to_string()),
            _ => None,
        })
        .unwrap_or_default()
}

#[cfg(feature = "sqlite")]
fn direction_value(direction: &crate::messages::MessageDirection) -> i64 {
    match direction {
        crate::messages::MessageDirection::Outgoing => 1,
        crate::messages::MessageDirection::Incoming
        | crate::messages::MessageDirection::Unknown => 0,
    }
}

#[cfg(feature = "sqlite")]
fn message_content_type(message: &crate::messages::Message) -> String {
    message
        .metadata
        .content_type
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| match &message.body {
            crate::messages::MessageBodyView::Text { .. } => "text/plain".to_string(),
            crate::messages::MessageBodyView::Payload { .. } => "application/json".to_string(),
            crate::messages::MessageBodyView::Unsupported { content_type } => content_type
                .as_ref()
                .map(|value| value.trim())
                .filter(|value| !value.is_empty())
                .unwrap_or("application/octet-stream")
                .to_string(),
        })
}

#[cfg(feature = "sqlite")]
fn message_content(message: &crate::messages::Message) -> String {
    match &message.body {
        crate::messages::MessageBodyView::Text { text, .. } => text.clone(),
        crate::messages::MessageBodyView::Payload { payload } => {
            serde_json::to_string(payload).unwrap_or_default()
        }
        crate::messages::MessageBodyView::Unsupported { content_type } => json!({
            "unsupported": true,
            "content_type": content_type,
        })
        .to_string(),
    }
}

#[cfg(feature = "sqlite")]
fn message_metadata_value(
    message: &crate::messages::Message,
    attachment_summary: Option<&crate::realtime::AttachmentMessageSummary>,
    download_action: Option<&crate::realtime::AttachmentDownloadAction>,
    warnings: &[String],
) -> String {
    let mut value = serde_json::to_value(&message.metadata).unwrap_or_else(|_| json!({}));
    if let Some(object) = value.as_object_mut() {
        if let Some(summary) = attachment_summary_value(attachment_summary) {
            object.insert("attachment_summary".to_string(), summary);
            object.insert("has_attachments".to_string(), Value::Bool(true));
        }
        if let Some(action) = attachment_download_action_value(download_action) {
            object.insert("attachment_download_action".to_string(), action);
        }
        if !warnings.is_empty() {
            object.insert(
                "attachment_warnings".to_string(),
                Value::Array(warnings.iter().cloned().map(Value::String).collect()),
            );
        }
    }
    serde_json::to_string(&value).unwrap_or_default()
}

#[cfg(feature = "sqlite")]
fn attachment_summary_value(
    summary: Option<&crate::realtime::AttachmentMessageSummary>,
) -> Option<Value> {
    let summary = summary?;
    Some(json!({
        "attachment_id": summary.attachment_id.as_deref(),
        "filename": summary.filename.as_deref(),
        "mime_type": summary.mime_type.as_deref(),
        "size_bytes": summary.size_bytes,
        "content_type": summary.content_type.as_deref(),
    }))
}

#[cfg(feature = "sqlite")]
fn attachment_download_action_value(
    action: Option<&crate::realtime::AttachmentDownloadAction>,
) -> Option<Value> {
    let action = action?;
    let mut value = json!({
        "command": "msg.attachment.download",
        "message_id": action.message_id.as_str(),
        "attachment_id": action.attachment_id.as_deref(),
    });
    if let Some(object) = value.as_object_mut() {
        match &action.thread {
            crate::messages::ThreadRef::Direct(peer) => {
                object.insert("with".to_string(), Value::String(peer.as_str().to_string()));
            }
            crate::messages::ThreadRef::Group(group) => {
                object.insert(
                    "group".to_string(),
                    Value::String(group.as_str().to_string()),
                );
            }
            crate::messages::ThreadRef::Thread(thread) => {
                object.insert(
                    "thread".to_string(),
                    Value::String(thread.as_str().to_string()),
                );
            }
        }
    }
    Some(value)
}

#[cfg(feature = "sqlite")]
fn now_utc_like() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}
