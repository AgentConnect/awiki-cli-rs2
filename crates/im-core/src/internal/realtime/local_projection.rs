#[cfg(feature = "sqlite")]
use serde_json::{json, Value};

#[cfg(feature = "sqlite")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RealtimeMessageLocalProjectionContext {
    pub owner_identity_id: String,
    pub owner_did: String,
    pub credential_name: String,
    pub(crate) peer_scope: Option<crate::internal::local_state::owner_scope::DirectPeerScope>,
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
    let owner_did = context.owner_did.trim().to_string();
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
    let conversation_id = conversation_id_for_message(message, context.peer_scope.as_ref());
    if conversation_id.trim().is_empty() || (group_did.trim().is_empty() && !has_thread_peer) {
        return None;
    }
    let received_at = message.received_at.clone().unwrap_or_else(now_utc_like);

    Some(RealtimeMessageLocalProjection {
        record: crate::internal::local_state::messages::MessageRecord {
            msg_id: message.id.as_str().to_string(),
            owner_identity_id: context.owner_identity_id.trim().to_string(),
            owner_did: owner_did.clone(),
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
            sent_at: local_projection_sent_at(message, &received_at),
            stored_at: received_at,
            is_e2ee: message
                .metadata
                .attributes
                .iter()
                .any(|attribute| attribute.key == "security" && attribute.value.contains("e2ee")),
            is_read: matches!(
                message.direction,
                crate::messages::MessageDirection::Outgoing
            ),
            metadata: message_metadata_value(
                message,
                attachment_summary,
                download_action,
                warnings,
                context.peer_scope.as_ref(),
            ),
            credential_name: context.credential_name.trim().to_string(),
            ..crate::internal::local_state::messages::MessageRecord::default()
        }
        .with_wire_identity_from_message(&owner_did, message),
    })
}

#[cfg(feature = "sqlite")]
fn local_projection_sent_at(message: &crate::messages::Message, received_at: &str) -> String {
    if !matches!(
        message.direction,
        crate::messages::MessageDirection::Outgoing
    ) && message.metadata.server_sequence.is_none()
    {
        return received_at.to_owned();
    }
    message
        .sent_at
        .clone()
        .or_else(|| message.received_at.clone())
        .unwrap_or_else(|| received_at.to_owned())
}

#[cfg(feature = "sqlite")]
pub fn apply_realtime_message_local_projection(
    connection: &mut rusqlite::Connection,
    projection: RealtimeMessageLocalProjection,
) -> crate::ImResult<bool> {
    let transaction = connection
        .transaction()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let outcome = crate::internal::local_state::inbound_resolution_backlog::ingest_remote_messages(
        &transaction,
        &[projection.record],
        "realtime_message",
    )?;
    transaction
        .commit()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    Ok(outcome.stored_messages > 0)
}

#[cfg(feature = "sqlite")]
fn conversation_id_for_message(
    message: &crate::messages::Message,
    context_peer_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
) -> String {
    if let Some(scope) =
        crate::internal::message_runtime::local_projection::peer_scope_from_metadata(
            &message.metadata,
        )
    {
        return crate::internal::message_runtime::local_projection::direct_conversation_id_for_peer_scope(
            &scope,
        );
    }
    if let Some(scope) = context_peer_scope {
        return crate::internal::message_runtime::local_projection::direct_conversation_id_for_peer_scope(
            scope,
        );
    }
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
    context_peer_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
) -> String {
    let mut value = serde_json::to_value(&message.metadata).unwrap_or_else(|_| json!({}));
    if let Some(object) = value.as_object_mut() {
        if let Some(scope) =
            crate::internal::message_runtime::local_projection::peer_scope_from_metadata(
                &message.metadata,
            )
            .or_else(|| context_peer_scope.cloned())
        {
            object.insert("peer_user_id".to_string(), Value::String(scope.user_id));
            object.insert(
                "peer_full_handle".to_string(),
                Value::String(scope.full_handle),
            );
        }
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
        for attribute in &message.metadata.attributes {
            if matches!(
                attribute.key.as_str(),
                "raw_message_id"
                    | "group_event_seq"
                    | "security"
                    | "decryption_state"
                    | "secure_wire_content_type"
            ) && !attribute.value.trim().is_empty()
            {
                object.insert(
                    attribute.key.clone(),
                    Value::String(attribute.value.clone()),
                );
            }
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

#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use super::*;

    #[test]
    fn unsequenced_incoming_realtime_uses_recipient_timestamp_for_local_order() {
        let message = crate::messages::Message {
            id: crate::ids::MessageId::parse("realtime-order-1").unwrap(),
            thread: crate::messages::ThreadRef::Direct(
                crate::ids::PeerRef::parse("did:example:peer", "").unwrap(),
            ),
            direction: crate::messages::MessageDirection::Incoming,
            sender: crate::ids::PeerRef::parse("did:example:peer", "").unwrap(),
            receiver: Some(crate::ids::PeerRef::parse("did:example:owner", "").unwrap()),
            group: None,
            body: crate::messages::MessageBodyView::Text {
                text: "same body".to_owned(),
                kind: crate::messages::MessageKind::Text,
            },
            sent_at: Some("2026-07-17T05:20:31Z".to_owned()),
            received_at: Some("2026-07-17T05:20:32.207005Z".to_owned()),
            metadata: crate::messages::MessageMetadata {
                content_type: Some("text/plain".to_owned()),
                ..crate::messages::MessageMetadata::default()
            },
        };

        let projection = plan_realtime_message_local_projection(
            &RealtimeMessageLocalProjectionContext {
                owner_identity_id: "owner-a".to_owned(),
                owner_did: "did:example:owner".to_owned(),
                credential_name: "owner-a".to_owned(),
                peer_scope: None,
            },
            &message,
            None,
            None,
            &[],
        )
        .unwrap()
        .into_record();

        assert_eq!(projection.sent_at, "2026-07-17T05:20:32.207005Z");
        assert_eq!(projection.stored_at, "2026-07-17T05:20:32.207005Z");
    }

    #[test]
    fn sequenced_incoming_realtime_keeps_authoritative_sent_timestamp() {
        let message = crate::messages::Message {
            id: crate::ids::MessageId::parse("realtime-order-2").unwrap(),
            thread: crate::messages::ThreadRef::Direct(
                crate::ids::PeerRef::parse("did:example:peer", "").unwrap(),
            ),
            direction: crate::messages::MessageDirection::Incoming,
            sender: crate::ids::PeerRef::parse("did:example:peer", "").unwrap(),
            receiver: Some(crate::ids::PeerRef::parse("did:example:owner", "").unwrap()),
            group: None,
            body: crate::messages::MessageBodyView::Text {
                text: "same body".to_owned(),
                kind: crate::messages::MessageKind::Text,
            },
            sent_at: Some("2026-07-17T05:20:31.558059Z".to_owned()),
            received_at: Some("2026-07-17T05:20:32.207005Z".to_owned()),
            metadata: crate::messages::MessageMetadata {
                server_sequence: Some(21_263),
                content_type: Some("text/plain".to_owned()),
                ..crate::messages::MessageMetadata::default()
            },
        };

        let projection = plan_realtime_message_local_projection(
            &RealtimeMessageLocalProjectionContext {
                owner_identity_id: "owner-a".to_owned(),
                owner_did: "did:example:owner".to_owned(),
                credential_name: "owner-a".to_owned(),
                peer_scope: None,
            },
            &message,
            None,
            None,
            &[],
        )
        .unwrap()
        .into_record();

        assert_eq!(projection.sent_at, "2026-07-17T05:20:31.558059Z");
        assert_eq!(projection.stored_at, "2026-07-17T05:20:32.207005Z");
    }

    #[test]
    fn decrypted_group_realtime_projection_is_reusable_secure_cache() {
        let message = crate::messages::Message {
            id: crate::ids::MessageId::parse("did:example:group:12").unwrap(),
            thread: crate::messages::ThreadRef::Group(
                crate::ids::GroupRef::parse("did:example:group").unwrap(),
            ),
            direction: crate::messages::MessageDirection::Incoming,
            sender: crate::ids::PeerRef::parse("did:example:sender", "").unwrap(),
            receiver: Some(crate::ids::PeerRef::parse("did:example:owner", "").unwrap()),
            group: Some(crate::ids::GroupRef::parse("did:example:group").unwrap()),
            body: crate::messages::MessageBodyView::Text {
                text: "decrypted".to_owned(),
                kind: crate::messages::MessageKind::Text,
            },
            sent_at: None,
            received_at: None,
            metadata: crate::messages::MessageMetadata {
                server_sequence: Some(12),
                content_type: Some("text/plain".to_owned()),
                attributes: vec![
                    crate::messages::MessageMetadataAttribute {
                        key: "security".to_owned(),
                        value: "group-e2ee".to_owned(),
                    },
                    crate::messages::MessageMetadataAttribute {
                        key: "decryption_state".to_owned(),
                        value: "decrypted".to_owned(),
                    },
                    crate::messages::MessageMetadataAttribute {
                        key: "raw_message_id".to_owned(),
                        value: "logical-message-12".to_owned(),
                    },
                ],
                ..crate::messages::MessageMetadata::default()
            },
        };

        let record = plan_realtime_message_local_projection(
            &RealtimeMessageLocalProjectionContext {
                owner_identity_id: "owner-a".to_owned(),
                owner_did: "did:example:owner".to_owned(),
                credential_name: "owner-a".to_owned(),
                peer_scope: None,
            },
            &message,
            None,
            None,
            &[],
        )
        .unwrap()
        .into_record();
        let metadata: Value = serde_json::from_str(&record.metadata).unwrap();

        assert!(record.is_e2ee);
        assert_eq!(metadata["decryption_state"], "decrypted");
        assert_eq!(metadata["raw_message_id"], "logical-message-12");
    }

    #[test]
    fn own_device_sync_keeps_local_owner_and_external_wire_peer() {
        let message = crate::messages::Message {
            id: crate::ids::MessageId::parse("logical-own-sync-1").unwrap(),
            thread: crate::messages::ThreadRef::Direct(
                crate::ids::PeerRef::parse("did:example:peer", "").unwrap(),
            ),
            direction: crate::messages::MessageDirection::Outgoing,
            sender: crate::ids::PeerRef::parse("did:example:owner", "").unwrap(),
            receiver: Some(crate::ids::PeerRef::parse("did:example:peer", "").unwrap()),
            group: None,
            body: crate::messages::MessageBodyView::Text {
                text: "synced outbound".to_owned(),
                kind: crate::messages::MessageKind::Text,
            },
            sent_at: Some("2026-07-26T20:00:00Z".to_owned()),
            received_at: Some("2026-07-26T20:00:01Z".to_owned()),
            metadata: crate::messages::MessageMetadata {
                content_type: Some("text/plain".to_owned()),
                attributes: vec![
                    crate::messages::MessageMetadataAttribute {
                        key: "security".to_owned(),
                        value: "direct-e2ee".to_owned(),
                    },
                    crate::messages::MessageMetadataAttribute {
                        key: "decryption_state".to_owned(),
                        value: "decrypted".to_owned(),
                    },
                ],
                ..crate::messages::MessageMetadata::default()
            },
        };
        let peer_scope = crate::internal::local_state::owner_scope::DirectPeerScope::new(
            "peer-user",
            "peer.example",
        )
        .unwrap();

        let record = plan_realtime_message_local_projection(
            &RealtimeMessageLocalProjectionContext {
                owner_identity_id: "owner-id".to_owned(),
                owner_did: "did:example:owner".to_owned(),
                credential_name: "owner".to_owned(),
                peer_scope: Some(peer_scope.clone()),
            },
            &message,
            None,
            None,
            &[],
        )
        .unwrap()
        .into_record();

        assert_eq!(record.owner_did, "did:example:owner");
        assert_eq!(record.sender_did, "did:example:owner");
        assert_eq!(record.receiver_did, "did:example:peer");
        assert_eq!(record.wire_thread_kind, "direct");
        assert_eq!(record.wire_thread_ref, "did:example:peer");
        assert_eq!(
            record.conversation_id,
            crate::internal::local_state::owner_scope::direct_conversation_id_for_peer_scope(
                &peer_scope
            )
        );
        assert_eq!(record.content, "synced outbound");
        assert!(record.is_e2ee);
        assert!(record.is_read);
    }

    #[test]
    fn unresolved_realtime_direct_is_backlogged_without_creating_legacy_message() {
        let mut db = rusqlite::Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let projection = RealtimeMessageLocalProjection {
            record: crate::internal::local_state::messages::MessageRecord {
                msg_id: "realtime-unresolved-1".to_owned(),
                owner_identity_id: "owner-a".to_owned(),
                owner_did: "did:example:owner".to_owned(),
                conversation_id: "dm:did:example:peer".to_owned(),
                thread_id: "dm:did:example:peer".to_owned(),
                sender_did: "did:example:peer".to_owned(),
                receiver_did: "did:example:owner".to_owned(),
                content_type: "text/plain".to_owned(),
                content: "hello".to_owned(),
                stored_at: "2026-07-15T00:00:00Z".to_owned(),
                credential_name: "owner-a".to_owned(),
                ..crate::internal::local_state::messages::MessageRecord::default()
            }
            .with_resolved_wire_thread("direct", "did:example:peer"),
        };

        let stored = apply_realtime_message_local_projection(&mut db, projection).unwrap();

        assert!(!stored);
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM messages", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            crate::internal::local_state::inbound_resolution_backlog::pending_count(&db, "owner-a")
                .unwrap(),
            1
        );
        assert!(
            crate::internal::local_state::canonical_invariants::check(&db, "owner-a")
                .unwrap()
                .is_empty()
        );
    }
}
