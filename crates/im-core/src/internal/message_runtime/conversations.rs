use crate::messages::{
    ConversationSnapshotItem, ConversationSnapshotMessage, ConversationSnapshotMessageBody,
};

pub(crate) struct MessageConversationRuntime<'a> {
    client: &'a crate::core::ImClient,
}

impl<'a> MessageConversationRuntime<'a> {
    pub(crate) fn new(client: &'a crate::core::ImClient) -> Self {
        Self { client }
    }

    pub(crate) fn conversations(
        self,
        query: crate::messages::ConversationQuery,
    ) -> crate::ImResult<crate::ids::Page<crate::messages::Conversation>> {
        let requested_limit = page_limit(query.limit, 50);
        let mut records = list_conversation_records(self.client, &query)?;
        let has_more = records.len() > requested_limit;
        records.truncate(requested_limit);
        let next_cursor = if has_more {
            records.last().and_then(conversation_cursor_from_record)
        } else {
            None
        };
        let items = records
            .into_iter()
            .map(|record| conversation_from_record(self.client.did().as_str(), record))
            .collect::<crate::ImResult<Vec<_>>>()?;
        save_snapshot_best_effort(self.client, &query, &items);
        Ok(crate::ids::Page {
            items,
            next_cursor,
            has_more,
        })
    }

    pub(crate) async fn conversations_async(
        self,
        query: crate::messages::ConversationQuery,
    ) -> crate::ImResult<crate::ids::Page<crate::messages::Conversation>> {
        let requested_limit = page_limit(query.limit, 50);
        let mut records = list_conversation_records_async(self.client, &query).await?;
        let has_more = records.len() > requested_limit;
        records.truncate(requested_limit);
        let next_cursor = if has_more {
            records.last().and_then(conversation_cursor_from_record)
        } else {
            None
        };
        let items = records
            .into_iter()
            .map(|record| conversation_from_record(self.client.did().as_str(), record))
            .collect::<crate::ImResult<Vec<_>>>()?;
        save_snapshot_best_effort(self.client, &query, &items);
        Ok(crate::ids::Page {
            items,
            next_cursor,
            has_more,
        })
    }
}

#[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
fn list_conversation_records(
    client: &crate::core::ImClient,
    query: &crate::messages::ConversationQuery,
) -> crate::ImResult<Vec<crate::internal::local_state::conversations::ConversationRecord>> {
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    crate::internal::local_state::conversations::list_conversations_for_owner_identity(
        &connection,
        client.current_identity().id.as_str(),
        client.did().as_str(),
        query,
    )
}

fn save_snapshot_best_effort(
    client: &crate::core::ImClient,
    query: &crate::messages::ConversationQuery,
    conversations: &[crate::messages::Conversation],
) {
    if !should_update_conversation_snapshot(query) {
        return;
    }
    let items = conversations
        .iter()
        .map(snapshot_item_from_conversation)
        .collect::<Vec<_>>();
    let _ = crate::internal::snapshot::conversation_snapshot::save_for_client(client, items);
}

fn should_update_conversation_snapshot(query: &crate::messages::ConversationQuery) -> bool {
    query.cursor.is_none() && query.include_groups && query.include_direct && !query.unread_only
}

pub(crate) fn snapshot_item_from_conversation(
    conversation: &crate::messages::Conversation,
) -> ConversationSnapshotItem {
    let (thread_kind, thread_id) = thread_ref_parts(&conversation.thread);
    let conversation_identity = conversation
        .conversation_identity
        .clone()
        .unwrap_or_else(|| {
            crate::messages::ConversationIdentity::from_thread_ref(&conversation.thread)
        });
    ConversationSnapshotItem {
        conversation_id: conversation.conversation_id.clone(),
        peer_persona_id: conversation.peer_persona_id.clone(),
        canonical_group_did: conversation.canonical_group_did.clone(),
        resolution_state: conversation.resolution_state,
        thread_kind,
        thread_id,
        title: conversation.title.clone(),
        conversation_identity: Some(conversation_identity),
        participants: conversation
            .participants
            .iter()
            .map(|peer| peer.as_str().to_owned())
            .collect(),
        last_message: conversation.last_message.as_ref().map(snapshot_message),
        unread_count: conversation.unread_count,
        unread_mention_count: conversation.unread_mention_count,
        first_unread_mention_message_id: conversation
            .first_unread_mention_message_id
            .as_ref()
            .map(|message_id| message_id.as_str().to_owned()),
        message_count: conversation.message_count,
        last_message_at: conversation.last_message_at.clone(),
        activity_at: conversation.activity_at.clone(),
    }
}

fn snapshot_message(message: &crate::messages::Message) -> ConversationSnapshotMessage {
    let (thread_kind, thread_id) = thread_ref_parts(&message.thread);
    let conversation_identity = message
        .metadata
        .conversation_identity
        .clone()
        .unwrap_or_else(|| crate::messages::ConversationIdentity::from_thread_ref(&message.thread));
    ConversationSnapshotMessage {
        id: message.id.as_str().to_owned(),
        thread_kind,
        thread_id,
        conversation_identity: Some(conversation_identity),
        direction: message_direction_name(&message.direction).to_owned(),
        sender: message.sender.as_str().to_owned(),
        receiver: message
            .receiver
            .as_ref()
            .map(|receiver| receiver.as_str().to_owned()),
        group: message
            .group
            .as_ref()
            .map(|group| group.as_str().to_owned()),
        body: snapshot_body(&message.body),
        sent_at: message.sent_at.clone(),
        received_at: message.received_at.clone(),
        server_sequence: message.metadata.server_sequence,
        content_type: message.metadata.content_type.clone(),
        attributes: snapshot_attributes(&message.metadata.attributes),
    }
}

fn snapshot_body(body: &crate::messages::MessageBodyView) -> ConversationSnapshotMessageBody {
    match body {
        crate::messages::MessageBodyView::Text { text, kind } => ConversationSnapshotMessageBody {
            text: Some(text.clone()),
            kind: Some(message_kind_to_string(kind).to_owned()),
            payload_json: None,
            unsupported_content_type: None,
        },
        crate::messages::MessageBodyView::Payload { payload } => ConversationSnapshotMessageBody {
            text: None,
            kind: Some("payload".to_owned()),
            payload_json: Some(payload.to_string()),
            unsupported_content_type: None,
        },
        crate::messages::MessageBodyView::Unsupported { content_type } => {
            ConversationSnapshotMessageBody {
                text: None,
                kind: None,
                payload_json: None,
                unsupported_content_type: content_type.clone(),
            }
        }
    }
}

fn message_kind_to_string(kind: &crate::messages::MessageKind) -> &'static str {
    match kind {
        crate::messages::MessageKind::Text => "text",
        crate::messages::MessageKind::Markdown => "markdown",
    }
}

fn snapshot_attributes(
    attributes: &[crate::messages::MessageMetadataAttribute],
) -> Vec<crate::messages::MessageMetadataAttribute> {
    attributes
        .iter()
        .filter(|attribute| is_snapshot_attribute(attribute.key.as_str()))
        .cloned()
        .collect()
}

fn is_snapshot_attribute(key: &str) -> bool {
    matches!(
        key,
        "peer_user_id"
            | "peer_full_handle"
            | "peer_current_did"
            | "resolved_target_did"
            | "target_handle"
    )
}

fn thread_ref_parts(thread: &crate::messages::ThreadRef) -> (String, String) {
    match thread {
        crate::messages::ThreadRef::Direct(peer) => ("direct".to_owned(), peer.as_str().to_owned()),
        crate::messages::ThreadRef::Group(group) => ("group".to_owned(), group.as_str().to_owned()),
        crate::messages::ThreadRef::Thread(thread_id) => {
            ("thread".to_owned(), thread_id.as_str().to_owned())
        }
    }
}

fn message_direction_name(direction: &crate::messages::MessageDirection) -> &'static str {
    match direction {
        crate::messages::MessageDirection::Outgoing => "outgoing",
        crate::messages::MessageDirection::Incoming => "incoming",
        crate::messages::MessageDirection::Unknown => "unknown",
    }
}

#[cfg(all(feature = "sqlite", not(any(feature = "blocking", test))))]
fn list_conversation_records(
    _client: &crate::core::ImClient,
    _query: &crate::messages::ConversationQuery,
) -> crate::ImResult<Vec<crate::internal::local_state::conversations::ConversationRecord>> {
    Err(crate::ImError::unsupported("sync-message-conversations"))
}

#[cfg(feature = "sqlite")]
async fn list_conversation_records_async(
    client: &crate::core::ImClient,
    query: &crate::messages::ConversationQuery,
) -> crate::ImResult<Vec<crate::internal::local_state::conversations::ConversationRecord>> {
    client
        .core_inner()
        .local_state_db()
        .await?
        .list_conversations(
            client.current_identity().id.as_str(),
            client.did().as_str(),
            query.clone(),
        )
        .await
}

#[cfg(not(feature = "sqlite"))]
async fn list_conversation_records_async(
    _client: &crate::core::ImClient,
    _query: &crate::messages::ConversationQuery,
) -> crate::ImResult<Vec<NoSqliteConversationRecord>> {
    Err(crate::ImError::unsupported("message-conversations"))
}

#[cfg(not(feature = "sqlite"))]
fn list_conversation_records(
    _client: &crate::core::ImClient,
    _query: &crate::messages::ConversationQuery,
) -> crate::ImResult<Vec<NoSqliteConversationRecord>> {
    Err(crate::ImError::unsupported("message-conversations"))
}

fn conversation_from_record(
    owner_did: &str,
    record: ConversationRecordLike,
) -> crate::ImResult<crate::messages::Conversation> {
    let last_message = record.last_message().map(message_from_record).transpose()?;
    let thread = conversation_thread(owner_did, &record, last_message.as_ref())?;
    let identity_thread =
        crate::messages::ThreadRef::Thread(crate::ids::ThreadId::parse(record.conversation_id())?);
    let conversation_identity = crate::messages::ConversationIdentity::from_thread_ref_for_owner(
        &identity_thread,
        owner_did,
    );
    let participants = conversation_participants(owner_did, &thread, last_message.as_ref())?;
    let resolution_state = conversation_resolution_state(record.resolution_state())?;
    let peer_persona_id = non_empty_string(record.peer_persona_id());
    let canonical_group_did = non_empty_string(record.canonical_group_did());
    if resolution_state == crate::messages::ConversationResolutionState::Resolved {
        match record.thread_kind() {
            "direct" if peer_persona_id.is_none() => {
                return Err(crate::ImError::LocalProjectionUnavailable {
                    detail: "resolved Direct conversation is missing peer_persona_id".to_owned(),
                });
            }
            "group" if canonical_group_did.is_none() => {
                return Err(crate::ImError::CanonicalGroupIdentityMissing {
                    group: record.conversation_id().to_owned(),
                });
            }
            _ => {}
        }
    }
    Ok(crate::messages::Conversation {
        conversation_id: record.conversation_id().to_owned(),
        peer_persona_id,
        canonical_group_did,
        resolution_state,
        thread,
        conversation_identity: Some(conversation_identity),
        title: non_empty_string(record.title()),
        participants,
        last_message,
        unread_count: u32_count(record.unread_count()),
        unread_mention_count: u32_count(record.unread_mention_count()),
        first_unread_mention_message_id: record
            .first_unread_mention_message_id()
            .map(crate::ids::MessageId::parse)
            .transpose()?,
        message_count: u32_count(record.message_count()),
        last_message_at: non_empty_string(record.last_message_at()),
        activity_at: non_empty_string(record.activity_at()),
    })
}

fn conversation_cursor_from_record(record: &ConversationRecordLike) -> Option<crate::ids::Cursor> {
    encode_conversation_cursor(record).and_then(|cursor| crate::ids::Cursor::parse(cursor).ok())
}

#[cfg(feature = "sqlite")]
type ConversationRecordLike = crate::internal::local_state::conversations::ConversationRecord;

#[cfg(not(feature = "sqlite"))]
type ConversationRecordLike = NoSqliteConversationRecord;

#[cfg(not(feature = "sqlite"))]
struct NoSqliteConversationRecord;

#[cfg(feature = "sqlite")]
impl ConversationRecordExt for crate::internal::local_state::conversations::ConversationRecord {
    fn thread_id(&self) -> &str {
        &self.thread_id
    }

    fn conversation_id(&self) -> &str {
        &self.conversation_id
    }

    fn peer_persona_id(&self) -> &str {
        &self.peer_persona_id
    }

    fn canonical_group_did(&self) -> &str {
        &self.canonical_group_did
    }

    fn resolution_state(&self) -> &str {
        &self.resolution_state
    }

    fn thread_kind(&self) -> &str {
        &self.thread_kind
    }

    fn direct_peer_did(&self) -> &str {
        &self.direct_peer_did
    }

    fn title(&self) -> &str {
        &self.title
    }

    fn activity_at(&self) -> &str {
        &self.activity_at
    }

    fn message_count(&self) -> i64 {
        self.message_count
    }

    fn unread_count(&self) -> i64 {
        self.unread_count
    }

    fn unread_mention_count(&self) -> i64 {
        self.unread_mention_count
    }

    fn first_unread_mention_message_id(&self) -> Option<&str> {
        self.first_unread_mention_message_id.as_deref()
    }

    fn last_message_at(&self) -> &str {
        &self.last_message_at
    }

    fn last_message(&self) -> Option<&crate::internal::local_state::messages::MessageRecord> {
        self.last_message.as_ref()
    }
}

#[cfg(not(feature = "sqlite"))]
impl ConversationRecordExt for NoSqliteConversationRecord {
    fn thread_id(&self) -> &str {
        ""
    }

    fn conversation_id(&self) -> &str {
        ""
    }

    fn peer_persona_id(&self) -> &str {
        ""
    }

    fn canonical_group_did(&self) -> &str {
        ""
    }

    fn resolution_state(&self) -> &str {
        "legacy_unresolved"
    }

    fn thread_kind(&self) -> &str {
        ""
    }
    fn direct_peer_did(&self) -> &str {
        ""
    }
    fn title(&self) -> &str {
        ""
    }
    fn activity_at(&self) -> &str {
        ""
    }

    fn message_count(&self) -> i64 {
        0
    }

    fn unread_count(&self) -> i64 {
        0
    }

    fn unread_mention_count(&self) -> i64 {
        0
    }

    fn first_unread_mention_message_id(&self) -> Option<&str> {
        None
    }

    fn last_message_at(&self) -> &str {
        ""
    }

    fn last_message(&self) -> Option<&crate::internal::local_state::messages::MessageRecord> {
        None
    }
}

trait ConversationRecordExt {
    fn thread_id(&self) -> &str;
    fn conversation_id(&self) -> &str;
    fn peer_persona_id(&self) -> &str;
    fn canonical_group_did(&self) -> &str;
    fn resolution_state(&self) -> &str;
    fn thread_kind(&self) -> &str;
    fn direct_peer_did(&self) -> &str;
    fn title(&self) -> &str;
    fn activity_at(&self) -> &str;
    fn message_count(&self) -> i64;
    fn unread_count(&self) -> i64;
    fn unread_mention_count(&self) -> i64;
    fn first_unread_mention_message_id(&self) -> Option<&str>;
    fn last_message_at(&self) -> &str;
    fn last_message(&self) -> Option<&crate::internal::local_state::messages::MessageRecord>;
}

fn conversation_resolution_state(
    value: &str,
) -> crate::ImResult<crate::messages::ConversationResolutionState> {
    match value.trim() {
        "resolved" => Ok(crate::messages::ConversationResolutionState::Resolved),
        "legacy_unresolved" => Ok(crate::messages::ConversationResolutionState::LegacyUnresolved),
        "blocked_conflict" => Ok(crate::messages::ConversationResolutionState::BlockedConflict),
        other => Err(crate::ImError::LocalProjectionUnavailable {
            detail: format!("unknown conversation resolution state: {other}"),
        }),
    }
}

fn encode_conversation_cursor(record: &impl ConversationRecordExt) -> Option<String> {
    let conversation_id = non_empty_string(record.conversation_id())?;
    Some(format!(
        "conversation-list:v2:{}:{}",
        base64_url_encode(record.activity_at()),
        base64_url_encode(&conversation_id)
    ))
}

fn conversation_thread(
    _owner_did: &str,
    record: &impl ConversationRecordExt,
    last_message: Option<&crate::messages::Message>,
) -> crate::ImResult<crate::messages::ThreadRef> {
    if let Some(message) = last_message {
        if let Some(group) = &message.group {
            return Ok(crate::messages::ThreadRef::Group(group.clone()));
        }
        if let Some(thread) =
            crate::internal::message_runtime::local_projection::scoped_direct_thread_ref_from_metadata(
                &message.metadata,
            )
        {
            return Ok(thread);
        }
        if let crate::messages::ThreadRef::Direct(peer) = &message.thread {
            return Ok(crate::messages::ThreadRef::Direct(peer.clone()));
        }
    }
    let thread_id = record.thread_id().trim();
    if record.thread_kind() == "group" {
        let group_ref = thread_id.strip_prefix("group:").unwrap_or(thread_id);
        return Ok(crate::messages::ThreadRef::Group(
            crate::ids::GroupRef::parse(group_ref)?,
        ));
    }
    if record.thread_kind() == "direct" && !record.direct_peer_did().trim().is_empty() {
        return Ok(crate::messages::ThreadRef::Direct(
            crate::ids::PeerRef::parse(record.direct_peer_did(), "")?,
        ));
    }
    Ok(crate::messages::ThreadRef::Thread(
        crate::ids::ThreadId::parse(thread_id)?,
    ))
}

fn conversation_participants(
    owner_did: &str,
    thread: &crate::messages::ThreadRef,
    last_message: Option<&crate::messages::Message>,
) -> crate::ImResult<Vec<crate::ids::PeerRef>> {
    match thread {
        crate::messages::ThreadRef::Direct(peer) => Ok(vec![peer.clone()]),
        crate::messages::ThreadRef::Group(_) | crate::messages::ThreadRef::Thread(_) => {
            let Some(message) = last_message else {
                return Ok(Vec::new());
            };
            let mut participants = Vec::new();
            for candidate in [
                direct_peer_from_message(owner_did, message).as_ref(),
                Some(&message.sender),
                message.receiver.as_ref(),
            ]
            .into_iter()
            .flatten()
            {
                if candidate.as_str() != owner_did
                    && !participants
                        .iter()
                        .any(|known: &crate::ids::PeerRef| known == candidate)
                {
                    participants.push(candidate.clone());
                }
            }
            Ok(participants)
        }
    }
}

fn direct_peer_from_message(
    owner_did: &str,
    message: &crate::messages::Message,
) -> Option<crate::ids::PeerRef> {
    if let Some(peer) = metadata_attribute(&message.metadata, "peer_full_handle") {
        return crate::ids::PeerRef::parse(peer, "").ok();
    }
    if message.sender.as_str() != owner_did {
        return Some(message.sender.clone());
    }
    message.receiver.clone()
}

pub(crate) fn message_from_record(
    record: &crate::internal::local_state::messages::MessageRecord,
) -> crate::ImResult<crate::messages::Message> {
    let thread = message_thread(record)?;
    let group = group_ref_from_record(record)?;
    let conversation_identity = crate::messages::ConversationIdentity::from_thread_ref_for_owner(
        &thread,
        &record.owner_did,
    );
    let content_type = effective_content_type(record);
    let retry_target = retry_target_from_record(record);
    let send_state = crate::internal::message_runtime::state::send_state_from_metadata(
        &record.metadata,
        &record.msg_id,
    );
    let retry_plan = crate::internal::message_runtime::state::retry_plan_from_metadata(
        &record.metadata,
        send_state.as_ref(),
        retry_target,
    );
    let mut attributes = metadata_attributes(&record.metadata);
    attributes.retain(|attribute| attribute.key != "is_read");
    attributes.push(crate::messages::MessageMetadataAttribute {
        key: "is_read".to_owned(),
        value: record.is_read.to_string(),
    });
    if record.is_e2ee && metadata_attribute_value(&attributes, "security").is_none() {
        attributes.push(crate::messages::MessageMetadataAttribute {
            key: "security".to_owned(),
            value: if group.is_some() {
                "group-e2ee".to_owned()
            } else {
                "direct-e2ee".to_owned()
            },
        });
    }
    Ok(crate::messages::Message {
        id: crate::ids::MessageId::parse(&record.msg_id)?,
        thread,
        direction: message_direction(record.direction),
        sender: crate::ids::PeerRef::parse(
            non_empty_or(&record.sender_did, "did:unknown:sender"),
            "",
        )?,
        receiver: non_empty_string(&record.receiver_did)
            .map(|value| crate::ids::PeerRef::parse(value, ""))
            .transpose()?,
        group,
        body: message_body(record, content_type.as_deref()),
        sent_at: non_empty_string(&record.sent_at),
        received_at: None,
        metadata: crate::messages::MessageMetadata {
            conversation_identity: Some(conversation_identity),
            operation_id: metadata_string(&record.metadata, "operation_id"),
            delivery_state: metadata_string(&record.metadata, "delivery_state"),
            send_state,
            retry_plan,
            server_sequence: record.server_seq,
            content_type,
            attributes,
        },
    })
}

fn retry_target_from_record(
    record: &crate::internal::local_state::messages::MessageRecord,
) -> Option<crate::internal::message_runtime::state::MessageRetryTarget> {
    if record.direction != 1 {
        return None;
    }
    if !record.group_did.trim().is_empty()
        || !record.group_id.trim().is_empty()
        || record.thread_id.trim().starts_with("group:")
    {
        return Some(crate::internal::message_runtime::state::MessageRetryTarget::GroupText);
    }
    Some(crate::internal::message_runtime::state::MessageRetryTarget::DirectText)
}

fn message_thread(
    record: &crate::internal::local_state::messages::MessageRecord,
) -> crate::ImResult<crate::messages::ThreadRef> {
    if let Some(group) = group_ref_from_record(record)? {
        return Ok(crate::messages::ThreadRef::Group(group));
    }
    if let Some(thread_id) = peer_scope_thread_id_from_record(record) {
        return Ok(crate::messages::ThreadRef::Thread(
            crate::ids::ThreadId::parse(thread_id)?,
        ));
    }
    let metadata = crate::messages::MessageMetadata {
        attributes: metadata_attributes(&record.metadata),
        ..crate::messages::MessageMetadata::default()
    };
    if let Some(thread) =
        crate::internal::message_runtime::local_projection::scoped_direct_thread_ref_from_metadata(
            &metadata,
        )
    {
        return Ok(thread);
    }
    let peer = if record.sender_did.trim() != record.owner_did.trim() {
        record.sender_did.as_str()
    } else {
        record.receiver_did.as_str()
    };
    if !peer.trim().is_empty() {
        return Ok(crate::messages::ThreadRef::Direct(
            crate::ids::PeerRef::parse(peer, "")?,
        ));
    }
    Ok(crate::messages::ThreadRef::Thread(
        crate::ids::ThreadId::parse(&record.thread_id)?,
    ))
}

fn peer_scope_thread_id_from_record(
    record: &crate::internal::local_state::messages::MessageRecord,
) -> Option<&str> {
    [record.conversation_id.as_str(), record.thread_id.as_str()]
        .into_iter()
        .map(str::trim)
        .find(|value| value.starts_with("dm:peer-scope:"))
}

fn group_ref_from_record(
    record: &crate::internal::local_state::messages::MessageRecord,
) -> crate::ImResult<Option<crate::ids::GroupRef>> {
    if !record.group_did.trim().is_empty() {
        return crate::ids::GroupRef::parse(&record.group_did).map(Some);
    }
    if !record.group_id.trim().is_empty() {
        return crate::ids::GroupRef::parse(&record.group_id).map(Some);
    }
    if let Some(group) = record.thread_id.trim().strip_prefix("group:") {
        return crate::ids::GroupRef::parse(group).map(Some);
    }
    Ok(None)
}

fn message_direction(direction: i64) -> crate::messages::MessageDirection {
    match direction {
        1 => crate::messages::MessageDirection::Outgoing,
        0 => crate::messages::MessageDirection::Incoming,
        _ => crate::messages::MessageDirection::Unknown,
    }
}

fn message_body(
    record: &crate::internal::local_state::messages::MessageRecord,
    content_type: Option<&str>,
) -> crate::messages::MessageBodyView {
    let content_type = content_type.map(str::to_owned);
    if content_type.as_deref() == Some("application/json") {
        return serde_json::from_str::<serde_json::Value>(&record.content)
            .ok()
            .filter(serde_json::Value::is_object)
            .map(crate::messages::MessageBodyView::from_json_payload)
            .unwrap_or(crate::messages::MessageBodyView::Unsupported { content_type });
    }
    if content_type.as_deref()
        == Some(crate::attachments::manifest::attachment_manifest_content_type())
    {
        return serde_json::from_str::<serde_json::Value>(&record.content)
            .ok()
            .filter(serde_json::Value::is_object)
            .map(|payload| crate::messages::MessageBodyView::Payload { payload })
            .unwrap_or(crate::messages::MessageBodyView::Unsupported { content_type });
    }
    let kind = match content_type.as_deref() {
        Some("text/markdown") => crate::messages::MessageKind::Markdown,
        Some("text/plain") | Some("text") | None => crate::messages::MessageKind::Text,
        _ => return crate::messages::MessageBodyView::Unsupported { content_type },
    };
    crate::messages::MessageBodyView::Text {
        text: record.content.clone(),
        kind,
    }
}

fn effective_content_type(
    record: &crate::internal::local_state::messages::MessageRecord,
) -> Option<String> {
    let stored = non_empty_string(&record.content_type);
    let metadata = metadata_string(&record.metadata, "content_type");
    if stored.as_deref() == Some("application/json")
        && metadata.as_deref()
            == Some(crate::attachments::manifest::attachment_manifest_content_type())
    {
        return metadata;
    }
    stored.or(metadata)
}

fn metadata_string(metadata: &str, key: &str) -> Option<String> {
    if metadata.trim().is_empty() {
        return None;
    }
    serde_json::from_str::<serde_json::Value>(metadata)
        .ok()
        .and_then(|value| {
            value
                .get(key)
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        })
        .filter(|value| !value.trim().is_empty())
}

fn metadata_attributes(metadata: &str) -> Vec<crate::messages::MessageMetadataAttribute> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(metadata) else {
        return Vec::new();
    };
    let Some(object) = value.as_object() else {
        return Vec::new();
    };
    [
        "raw_message_id",
        "group_event_seq",
        "peer_user_id",
        "peer_full_handle",
        "peer_current_did",
        "resolved_target_did",
        "target_handle",
        "is_read",
        "senderName",
        "sender_name",
        "sender_peer_persona_id",
        "security",
        "decryption_state",
        "secure_wire_content_type",
    ]
    .into_iter()
    .filter_map(|key| {
        object
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| crate::messages::MessageMetadataAttribute {
                key: key.to_owned(),
                value: value.to_owned(),
            })
    })
    .collect()
}

fn metadata_attribute_value<'a>(
    attributes: &'a [crate::messages::MessageMetadataAttribute],
    key: &str,
) -> Option<&'a str> {
    attributes
        .iter()
        .find(|attribute| attribute.key == key)
        .map(|attribute| attribute.value.trim())
        .filter(|value| !value.is_empty())
}

fn metadata_attribute<'a>(
    metadata: &'a crate::messages::MessageMetadata,
    key: &str,
) -> Option<&'a str> {
    metadata
        .attributes
        .iter()
        .find(|attribute| attribute.key == key)
        .map(|attribute| attribute.value.trim())
        .filter(|value| !value.is_empty())
}

fn page_limit(limit: crate::ids::PageLimit, fallback: usize) -> usize {
    if limit.0 == 0 {
        fallback
    } else {
        usize::try_from(limit.0).unwrap_or(fallback)
    }
}

fn u32_count(value: i64) -> u32 {
    u32::try_from(value.max(0)).unwrap_or(u32::MAX)
}

fn non_empty_string(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn base64_url_encode(value: &str) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

    URL_SAFE_NO_PAD.encode(value.as_bytes())
}

fn non_empty_or<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    if value.trim().is_empty() {
        fallback
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn secure_message_record_restores_public_security_metadata() {
        let message = message_from_record(
            &crate::internal::local_state::messages::MessageRecord {
                msg_id: "msg-secure".to_owned(),
                owner_identity_id: "alice-id".to_owned(),
                owner_did: "did:example:alice".to_owned(),
                conversation_id: "dm:did:example:bob".to_owned(),
                thread_id: "dm:did:example:bob".to_owned(),
                sender_did: "did:example:bob".to_owned(),
                receiver_did: "did:example:alice".to_owned(),
                content_type: "text/plain".to_owned(),
                content: "decrypted text".to_owned(),
                is_e2ee: true,
                metadata: r#"{"decryption_state":"decrypted","secure_wire_content_type":"application/anp-direct-init+json"}"#.to_owned(),
                ..crate::internal::local_state::messages::MessageRecord::default()
            },
        )
        .unwrap();

        assert_eq!(
            metadata_attribute(&message.metadata, "security"),
            Some("direct-e2ee")
        );
        assert_eq!(
            metadata_attribute(&message.metadata, "decryption_state"),
            Some("decrypted")
        );
        assert_eq!(
            metadata_attribute(&message.metadata, "secure_wire_content_type"),
            Some("application/anp-direct-init+json")
        );
    }

    #[test]
    fn message_record_publishes_canonical_read_state_metadata() {
        for (stored_read, stale_metadata, expected) in [
            (true, r#"{"is_read":"false"}"#, "true"),
            (false, r#"{"is_read":"true"}"#, "false"),
        ] {
            let message =
                message_from_record(&crate::internal::local_state::messages::MessageRecord {
                    msg_id: format!("msg-read-{expected}"),
                    owner_identity_id: "alice-id".to_owned(),
                    owner_did: "did:example:alice".to_owned(),
                    conversation_id: "dm:did:example:bob".to_owned(),
                    thread_id: "dm:did:example:bob".to_owned(),
                    direction: 0,
                    sender_did: "did:example:bob".to_owned(),
                    receiver_did: "did:example:alice".to_owned(),
                    content_type: "text/plain".to_owned(),
                    content: "read projection".to_owned(),
                    is_read: stored_read,
                    metadata: stale_metadata.to_owned(),
                    ..crate::internal::local_state::messages::MessageRecord::default()
                })
                .unwrap();
            let read_attributes = message
                .metadata
                .attributes
                .iter()
                .filter(|attribute| attribute.key == "is_read")
                .collect::<Vec<_>>();
            assert_eq!(read_attributes.len(), 1);
            assert_eq!(read_attributes[0].value, expected);
        }
    }

    #[test]
    fn empty_peer_scope_conversation_keeps_canonical_identity() {
        let conversation_id =
            crate::messages::direct_peer_scope_thread_id("user-bob", "bob.awiki.info")
                .unwrap()
                .as_str()
                .to_string();
        let conversation = conversation_from_record(
            "did:example:alice",
            crate::internal::local_state::conversations::ConversationRecord {
                owner_identity_id: "alice-id".into(),
                owner_did: "did:example:alice".into(),
                conversation_id: conversation_id.clone(),
                peer_persona_id: "persona:v1:bob".into(),
                resolution_state: "resolved".into(),
                thread_kind: "direct".into(),
                thread_id: conversation_id.clone(),
                direct_peer_did: "did:example:bob".into(),
                activity_at: "2026-07-13T00:00:00Z".into(),
                ..crate::internal::local_state::conversations::ConversationRecord::default()
            },
        )
        .unwrap();

        assert!(matches!(
            conversation.thread,
            crate::messages::ThreadRef::Direct(_)
        ));
        assert_eq!(
            conversation.peer_persona_id.as_deref(),
            Some("persona:v1:bob")
        );
        assert_eq!(
            conversation
                .conversation_identity
                .as_ref()
                .map(|identity| identity.conversation_id.as_str()),
            Some(conversation_id.as_str())
        );
        assert!(conversation.last_message.is_none());
        assert_eq!(conversation.message_count, 0);
    }

    #[test]
    fn empty_group_conversation_keeps_local_group_title() {
        let group_did = "did:wba:awiki.info:groups:group-1:e1_group";
        let conversation = conversation_from_record(
            "did:example:alice",
            crate::internal::local_state::conversations::ConversationRecord {
                owner_identity_id: "alice-id".into(),
                owner_did: "did:example:alice".into(),
                conversation_id: format!("group:{group_did}"),
                canonical_group_did: group_did.into(),
                resolution_state: "resolved".into(),
                thread_kind: "group".into(),
                thread_id: group_did.into(),
                title: "Project Group".into(),
                activity_at: "2026-07-13T00:00:00Z".into(),
                ..crate::internal::local_state::conversations::ConversationRecord::default()
            },
        )
        .unwrap();

        assert_eq!(conversation.title.as_deref(), Some("Project Group"));
        assert!(conversation.last_message.is_none());
        assert_eq!(
            snapshot_item_from_conversation(&conversation).title,
            conversation.title
        );
    }

    #[test]
    fn committed_group_message_preserves_wire_identity_metadata() {
        let group_did = "did:example:group";
        let message = message_from_record(&crate::internal::local_state::messages::MessageRecord {
            msg_id: format!("{group_did}:9"),
            owner_identity_id: "alice-id".into(),
            owner_did: "did:example:alice".into(),
            conversation_id: format!("group:{group_did}"),
            thread_id: format!("group:{group_did}"),
            direction: 0,
            sender_did: "did:example:bob".into(),
            group_id: group_did.into(),
            group_did: group_did.into(),
            content_type: "text/plain".into(),
            content: "hello group".into(),
            server_seq: Some(9),
            metadata: r#"{"raw_message_id":"msg-group-history-1","group_event_seq":"9"}"#.into(),
            ..crate::internal::local_state::messages::MessageRecord::default()
        })
        .unwrap();

        for (key, value) in [
            ("raw_message_id", "msg-group-history-1"),
            ("group_event_seq", "9"),
        ] {
            assert!(message
                .metadata
                .attributes
                .iter()
                .any(|attribute| attribute.key == key && attribute.value == value));
        }
    }

    #[test]
    fn message_conversation_runtime_projects_direct_and_group_conversations() {
        let fixture = Fixture::new();
        let client = fixture.client();
        fixture.seed_message(
            &client,
            "direct-old",
            "",
            0,
            "old",
            "2026-05-21T00:00:01Z",
            1,
        );
        fixture.seed_message(
            &client,
            "direct-new",
            "",
            0,
            "new",
            "2026-05-21T00:00:03Z",
            0,
        );
        fixture.seed_message(
            &client,
            "group-new",
            "did:example:group-1",
            0,
            "group",
            "2026-05-21T00:00:04Z",
            0,
        );

        let page = MessageConversationRuntime::new(&client)
            .conversations(crate::messages::ConversationQuery {
                limit: crate::ids::PageLimit(10),
                cursor: None,
                include_groups: true,
                include_direct: true,
                unread_only: false,
            })
            .unwrap();

        assert!(!page.has_more);
        assert_eq!(page.items.len(), 2);
        assert!(matches!(
            page.items[0].thread,
            crate::messages::ThreadRef::Group(_)
        ));
        assert_eq!(page.items[0].message_count, 1);
        assert_eq!(page.items[0].unread_count, 1);
        assert_eq!(
            page.items[0].last_message.as_ref().unwrap().id.as_str(),
            "group-new"
        );
        assert!(matches!(
            page.items[1].thread,
            crate::messages::ThreadRef::Direct(_)
        ));
        assert_eq!(page.items[1].message_count, 2);
        assert_eq!(page.items[1].unread_count, 1);
        assert_eq!(
            page.items[1].last_message_at.as_deref(),
            Some("2026-05-21T00:00:03Z")
        );
        assert_eq!(
            page.items[1].last_message.as_ref().unwrap().id.as_str(),
            "direct-new"
        );
        assert_eq!(page.items[1].participants[0].as_str(), "did:example:bob");
    }

    #[test]
    fn message_conversation_runtime_filters_unread_and_sets_has_more() {
        let fixture = Fixture::new();
        let client = fixture.client();
        fixture.seed_message(
            &client,
            "direct-read",
            "",
            0,
            "read",
            "2026-05-21T00:00:01Z",
            1,
        );
        fixture.seed_message(
            &client,
            "group-unread-1",
            "did:example:group-1",
            0,
            "group 1",
            "2026-05-21T00:00:02Z",
            0,
        );
        fixture.seed_message(
            &client,
            "group-unread-2",
            "did:example:group-2",
            0,
            "group 2",
            "2026-05-21T00:00:03Z",
            0,
        );

        let page = MessageConversationRuntime::new(&client)
            .conversations(crate::messages::ConversationQuery {
                limit: crate::ids::PageLimit(1),
                cursor: None,
                include_groups: true,
                include_direct: false,
                unread_only: true,
            })
            .unwrap();

        assert!(page.has_more);
        assert_eq!(page.items.len(), 1);
        assert!(matches!(
            page.items[0].thread,
            crate::messages::ThreadRef::Group(_)
        ));
        assert_eq!(
            page.items[0].last_message.as_ref().unwrap().id.as_str(),
            "group-unread-2"
        );
    }

    #[test]
    fn message_conversation_runtime_pages_with_next_cursor() {
        let fixture = Fixture::new();
        let client = fixture.client();
        fixture.seed_direct_message(
            &client,
            "direct-a",
            "dm:did:example:a",
            "did:example:a",
            "a",
            "2026-05-21T00:00:03Z",
            0,
        );
        fixture.seed_direct_message(
            &client,
            "direct-b",
            "dm:did:example:b",
            "did:example:b",
            "b",
            "2026-05-21T00:00:03Z",
            0,
        );
        fixture.seed_direct_message(
            &client,
            "direct-c",
            "dm:did:example:c",
            "did:example:c",
            "c",
            "2026-05-21T00:00:02Z",
            0,
        );

        let first_page = MessageConversationRuntime::new(&client)
            .conversations(crate::messages::ConversationQuery {
                limit: crate::ids::PageLimit(1),
                cursor: None,
                include_groups: true,
                include_direct: true,
                unread_only: false,
            })
            .unwrap();
        assert!(first_page.has_more);
        assert_eq!(first_page.items.len(), 1);
        assert_eq!(
            first_page.items[0].participants[0].as_str(),
            "did:example:b"
        );
        assert!(first_page.next_cursor.is_some());

        let snapshot = crate::internal::snapshot::conversation_snapshot::load_for_client(&client)
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.items.len(), 1);

        let second_page = MessageConversationRuntime::new(&client)
            .conversations(crate::messages::ConversationQuery {
                limit: crate::ids::PageLimit(2),
                cursor: first_page.next_cursor,
                include_groups: true,
                include_direct: true,
                unread_only: false,
            })
            .unwrap();

        assert!(!second_page.has_more);
        assert_eq!(second_page.items.len(), 2);
        assert_eq!(
            second_page.items[0].participants[0].as_str(),
            "did:example:a"
        );
        assert_eq!(
            second_page.items[1].participants[0].as_str(),
            "did:example:c"
        );
        assert!(second_page.next_cursor.is_none());

        let snapshot = crate::internal::snapshot::conversation_snapshot::load_for_client(&client)
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.items.len(), 1);
    }

    #[test]
    fn message_state_conversation_projection_reads_local_metadata_retry_plan() {
        let fixture = Fixture::new();
        let client = fixture.client();
        fixture.seed_message_with_metadata(
            &client,
            "direct-failed",
            1,
            "failed outgoing",
            "2026-05-21T00:00:05Z",
            true,
            r#"{"delivery_state":"failed","operation_id":"op-failed","failure_reason":"timeout"}"#,
        );

        let page = MessageConversationRuntime::new(&client)
            .conversations(crate::messages::ConversationQuery {
                limit: crate::ids::PageLimit(10),
                cursor: None,
                include_groups: false,
                include_direct: true,
                unread_only: false,
            })
            .unwrap();

        let message = page.items[0].last_message.as_ref().unwrap();
        let send_state = message.metadata.send_state.as_ref().unwrap();
        assert_eq!(
            send_state.state,
            crate::messages::MessageSendStateKind::Failed
        );
        assert_eq!(send_state.operation_id.as_deref(), Some("op-failed"));
        assert_eq!(send_state.reason.as_deref(), Some("timeout"));
        let retry_plan = message.metadata.retry_plan.as_ref().unwrap();
        assert!(retry_plan.retryable);
        assert_eq!(
            retry_plan.action,
            crate::messages::MessageRetryAction::RetryDirectText
        );
    }

    #[test]
    fn message_conversation_runtime_reads_outgoing_sdk_message_projection() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let message = crate::messages::Message {
            id: crate::ids::MessageId::parse("msg-outgoing-projected").unwrap(),
            thread: crate::messages::ThreadRef::Direct(
                crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
            ),
            direction: crate::messages::MessageDirection::Outgoing,
            sender: crate::ids::PeerRef::parse(client.did().as_str(), "").unwrap(),
            receiver: Some(crate::ids::PeerRef::parse("did:example:bob", "").unwrap()),
            group: None,
            body: crate::messages::MessageBodyView::Text {
                text: "sent after login".to_owned(),
                kind: crate::messages::MessageKind::Text,
            },
            sent_at: Some("2026-05-21T00:00:06Z".to_owned()),
            received_at: None,
            metadata: crate::messages::MessageMetadata::default(),
        };
        crate::internal::message_runtime::local_projection::persist_messages(
            &client,
            std::slice::from_ref(&message),
        )
        .unwrap();

        let page = MessageConversationRuntime::new(&client)
            .conversations(crate::messages::ConversationQuery {
                limit: crate::ids::PageLimit(10),
                cursor: None,
                include_groups: true,
                include_direct: true,
                unread_only: false,
            })
            .unwrap();

        assert_eq!(page.items.len(), 1);
        let conversation = &page.items[0];
        assert_eq!(conversation.unread_count, 0);
        assert_eq!(
            conversation.last_message.as_ref().unwrap().id.as_str(),
            "msg-outgoing-projected"
        );
        assert!(matches!(
            conversation.thread,
            crate::messages::ThreadRef::Direct(_)
        ));
    }

    #[test]
    fn direct_conversation_uses_peer_scope_across_did_rotation() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let scope = crate::internal::local_state::owner_scope::DirectPeerScope::new(
            "user-bob",
            "bob.anpclaw.com",
        )
        .unwrap();
        let conversation_id =
            crate::internal::local_state::owner_scope::direct_conversation_id_for_peer_scope(
                &scope,
            );
        fixture.seed_legacy_direct_message(
            &client,
            "msg-old-did",
            "did:wba:anpclaw.com:bob:e1_old",
            "old did",
            "2026-05-21T00:00:01Z",
        );
        fixture.seed_verified_peer_identity(
            &client,
            &[
                "did:wba:anpclaw.com:bob:e1_old",
                "did:wba:anpclaw.com:bob:e1_new",
            ],
        );
        fixture.seed_scoped_direct_message(
            &client,
            "msg-new-did",
            &conversation_id,
            "did:wba:anpclaw.com:bob:e1_new",
            "new did",
            "2026-05-21T00:00:02Z",
        );

        let page = MessageConversationRuntime::new(&client)
            .conversations(crate::messages::ConversationQuery {
                limit: crate::ids::PageLimit(10),
                cursor: None,
                include_groups: true,
                include_direct: true,
                unread_only: false,
            })
            .unwrap();

        assert_eq!(page.items.len(), 1);
        let conversation = &page.items[0];
        assert_eq!(conversation.message_count, 2);
        assert!(matches!(
            &conversation.thread,
            crate::messages::ThreadRef::Thread(thread) if thread.as_str() == conversation_id
        ));
        assert_eq!(conversation.participants[0].as_str(), "bob.anpclaw.com");
        assert_eq!(
            conversation
                .last_message
                .as_ref()
                .unwrap()
                .receiver
                .as_ref()
                .unwrap()
                .as_str(),
            client.did().as_str()
        );
    }

    #[test]
    fn conversation_query_saves_core_only_snapshot_from_committed_projection() {
        let fixture = Fixture::new();
        let client = fixture.client();
        fixture.seed_message_with_metadata(
            &client,
            "msg-snapshot",
            0,
            "snapshot hello",
            "2026-05-21T00:00:03Z",
            false,
            r#"{
              "peer_user_id":"user-bob",
              "peer_full_handle":"bob.awiki.test",
              "displayName":"Bob Product",
              "avatarUri":"https://example.test/avatar.png",
              "pinned":"true",
              "muted":"true",
              "hidden":"true",
              "peerLifecycleState":"deletedAgent"
            }"#,
        );

        let page = MessageConversationRuntime::new(&client)
            .conversations(crate::messages::ConversationQuery {
                limit: crate::ids::PageLimit(10),
                cursor: None,
                include_groups: true,
                include_direct: true,
                unread_only: false,
            })
            .unwrap();
        assert_eq!(page.items.len(), 1);

        let snapshot = crate::internal::snapshot::conversation_snapshot::load_for_client(&client)
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.owner_identity_id, "alice-id");
        assert_eq!(snapshot.owner_did, client.did().as_str());
        assert_eq!(snapshot.items.len(), 1);
        let item = &snapshot.items[0];
        assert_eq!(item.thread_kind, "thread");
        assert!(item.thread_id.starts_with("dm:peer-scope:v1:"));
        assert_eq!(item.unread_count, 1);
        assert_eq!(
            item.last_message.as_ref().unwrap().body.text.as_deref(),
            Some("snapshot hello")
        );
        assert_eq!(
            item.last_message.as_ref().unwrap().body.kind.as_deref(),
            Some("text")
        );
        let attributes = &item.last_message.as_ref().unwrap().attributes;
        assert!(attributes
            .iter()
            .any(|attribute| attribute.key == "peer_full_handle"));
        assert!(!attributes
            .iter()
            .any(|attribute| attribute.key == "displayName"));
        assert!(!attributes
            .iter()
            .any(|attribute| attribute.key == "avatarUri"));
        assert!(!attributes.iter().any(|attribute| attribute.key == "pinned"));
        assert!(!attributes.iter().any(|attribute| attribute.key == "muted"));
        assert!(!attributes.iter().any(|attribute| attribute.key == "hidden"));
        assert!(!attributes
            .iter()
            .any(|attribute| attribute.key == "peerLifecycleState"));
        let json = serde_json::to_string(&snapshot).unwrap();
        assert!(!json.contains("displayName"));
        assert!(!json.contains("avatarUri"));
        assert!(!json.contains("pinned"));
        assert!(!json.contains("muted"));
        assert!(!json.contains("hidden"));
        assert!(!json.contains("peerLifecycleState"));
    }

    #[test]
    fn filtered_conversation_query_does_not_overwrite_full_snapshot() {
        let fixture = Fixture::new();
        let client = fixture.client();
        fixture.seed_message(
            &client,
            "direct-snapshot",
            "",
            0,
            "direct",
            "2026-05-21T00:00:03Z",
            0,
        );
        fixture.seed_message(
            &client,
            "group-snapshot",
            "did:example:group-1",
            0,
            "group",
            "2026-05-21T00:00:04Z",
            0,
        );

        let full_page = MessageConversationRuntime::new(&client)
            .conversations(crate::messages::ConversationQuery {
                limit: crate::ids::PageLimit(10),
                cursor: None,
                include_groups: true,
                include_direct: true,
                unread_only: false,
            })
            .unwrap();
        assert_eq!(full_page.items.len(), 2);

        let snapshot = crate::internal::snapshot::conversation_snapshot::load_for_client(&client)
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.items.len(), 2);

        let unread_groups_page = MessageConversationRuntime::new(&client)
            .conversations(crate::messages::ConversationQuery {
                limit: crate::ids::PageLimit(10),
                cursor: None,
                include_groups: true,
                include_direct: false,
                unread_only: true,
            })
            .unwrap();
        assert_eq!(unread_groups_page.items.len(), 1);

        let snapshot = crate::internal::snapshot::conversation_snapshot::load_for_client(&client)
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.items.len(), 2);
        assert!(snapshot
            .items
            .iter()
            .any(|item| item.thread_kind == "direct"));
        assert!(snapshot
            .items
            .iter()
            .any(|item| item.thread_kind == "group"));
    }

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let root = unique_temp_root();
            let identities = root.join("identities");
            fs::create_dir_all(identities.join("alice")).unwrap();
            fs::write(identities.join("default"), "alice\n").unwrap();
            fs::write(
                identities.join("registry.json"),
                r#"{
                  "default_identity": "alice",
                  "identities": [{
                    "id": "alice-id",
                    "did": "did:example:alice",
                    "local_alias": "alice",
                    "ready_for_auth": true,
                    "ready_for_messaging": true,
                    "missing": []
                  }]
                }"#,
            )
            .unwrap();
            Self { root }
        }

        fn client(&self) -> crate::core::ImClient {
            crate::core::ImCore::new(
                crate::ImCoreConfig {
                    service_base_url: crate::ServiceEndpoint::parse("https://example.test")
                        .unwrap(),
                    did_domain: "awiki.test".to_string(),
                    client_version_info: None,
                    user_service_endpoint: None,
                    message_service_endpoint: None,
                    mail_service_endpoint: None,
                    anp_service_endpoint: None,
                    anp_service_did: None,
                    ca_bundle: None,
                    transport_policy: crate::MessageTransportPolicy::HttpOnly,
                },
                crate::ImCorePaths {
                    identities: crate::paths::IdentityRegistryPaths {
                        identity_root_dir: self.root.join("identities"),
                        registry_path: self.root.join("identities").join("registry.json"),
                        default_identity_path: Some(self.root.join("identities").join("default")),
                    },
                    local_state: crate::paths::LocalStatePaths {
                        sqlite_path: self.root.join("local").join("im.sqlite"),
                    },
                    runtime: crate::paths::RuntimePaths {
                        cache_dir: self.root.join("cache"),
                        temp_dir: self.root.join("tmp"),
                    },
                },
            )
            .unwrap()
            .client(crate::identity::IdentitySelector::LocalAlias(
                "alice".to_string(),
            ))
            .unwrap()
        }

        fn seed_message(
            &self,
            client: &crate::core::ImClient,
            message_id: &str,
            group_did: &str,
            direction: i64,
            content: &str,
            sent_at: &str,
            is_read: i64,
        ) {
            let connection = crate::internal::local_state::open_writable(
                &client.core_inner().sdk_paths().local_state.sqlite_path,
            )
            .unwrap();
            let conversation_id = if group_did.trim().is_empty() {
                "dm:did:example:bob".to_string()
            } else {
                format!("group:{group_did}")
            };
            let (sender_did, receiver_did) = if direction == 0 {
                ("did:example:bob", client.did().as_str())
            } else {
                (client.did().as_str(), "did:example:bob")
            };
            crate::internal::local_state::messages::upsert_message(
                &connection,
                &crate::internal::local_state::messages::MessageRecord {
                    msg_id: message_id.to_owned(),
                    owner_identity_id: client.current_identity().id.as_str().to_owned(),
                    owner_did: client.did().as_str().to_owned(),
                    conversation_id,
                    thread_id: if group_did.trim().is_empty() {
                        "dm:did:example:bob".to_owned()
                    } else {
                        format!("group:{group_did}")
                    },
                    direction,
                    sender_did: sender_did.to_owned(),
                    receiver_did: receiver_did.to_owned(),
                    group_id: group_did.to_owned(),
                    group_did: group_did.to_owned(),
                    content_type: "text/plain".to_owned(),
                    content: content.to_owned(),
                    sent_at: sent_at.to_owned(),
                    stored_at: sent_at.to_owned(),
                    is_read: is_read != 0,
                    ..crate::internal::local_state::messages::MessageRecord::default()
                },
            )
            .unwrap();
        }

        fn seed_direct_message(
            &self,
            client: &crate::core::ImClient,
            message_id: &str,
            conversation_id: &str,
            peer_did: &str,
            content: &str,
            sent_at: &str,
            is_read: i64,
        ) {
            let connection = crate::internal::local_state::open_writable(
                &client.core_inner().sdk_paths().local_state.sqlite_path,
            )
            .unwrap();
            crate::internal::local_state::messages::upsert_message(
                &connection,
                &crate::internal::local_state::messages::MessageRecord {
                    msg_id: message_id.to_owned(),
                    owner_identity_id: client.current_identity().id.as_str().to_owned(),
                    owner_did: client.did().as_str().to_owned(),
                    conversation_id: conversation_id.to_owned(),
                    thread_id: conversation_id.to_owned(),
                    direction: 0,
                    sender_did: peer_did.to_owned(),
                    receiver_did: client.did().as_str().to_owned(),
                    content_type: "text/plain".to_owned(),
                    content: content.to_owned(),
                    sent_at: sent_at.to_owned(),
                    stored_at: sent_at.to_owned(),
                    is_read: is_read != 0,
                    ..crate::internal::local_state::messages::MessageRecord::default()
                },
            )
            .unwrap();
        }

        fn seed_message_with_metadata(
            &self,
            client: &crate::core::ImClient,
            message_id: &str,
            direction: i64,
            content: &str,
            sent_at: &str,
            is_read: bool,
            metadata: &str,
        ) {
            let connection = crate::internal::local_state::open_writable(
                &client.core_inner().sdk_paths().local_state.sqlite_path,
            )
            .unwrap();
            let (sender_did, receiver_did) = if direction == 0 {
                ("did:example:bob", client.did().as_str())
            } else {
                (client.did().as_str(), "did:example:bob")
            };
            crate::internal::local_state::messages::upsert_message(
                &connection,
                &crate::internal::local_state::messages::MessageRecord {
                    msg_id: message_id.to_owned(),
                    owner_identity_id: client.current_identity().id.as_str().to_owned(),
                    owner_did: client.did().as_str().to_owned(),
                    conversation_id: "dm:did:example:bob".to_owned(),
                    thread_id: "dm:did:example:bob".to_owned(),
                    direction,
                    sender_did: sender_did.to_owned(),
                    receiver_did: receiver_did.to_owned(),
                    content_type: "text/plain".to_owned(),
                    content: content.to_owned(),
                    sent_at: sent_at.to_owned(),
                    stored_at: sent_at.to_owned(),
                    is_read,
                    metadata: metadata.to_owned(),
                    ..crate::internal::local_state::messages::MessageRecord::default()
                },
            )
            .unwrap();
        }

        fn seed_scoped_direct_message(
            &self,
            client: &crate::core::ImClient,
            message_id: &str,
            conversation_id: &str,
            sender_did: &str,
            content: &str,
            sent_at: &str,
        ) {
            let connection = crate::internal::local_state::open_writable(
                &client.core_inner().sdk_paths().local_state.sqlite_path,
            )
            .unwrap();
            crate::internal::local_state::messages::upsert_message(
                &connection,
                &crate::internal::local_state::messages::MessageRecord {
                    msg_id: message_id.to_owned(),
                    owner_identity_id: client.current_identity().id.as_str().to_owned(),
                    owner_did: client.did().as_str().to_owned(),
                    conversation_id: conversation_id.to_owned(),
                    thread_id: conversation_id.to_owned(),
                    sender_did: sender_did.to_owned(),
                    receiver_did: client.did().as_str().to_owned(),
                    content_type: "text/plain".to_owned(),
                    content: content.to_owned(),
                    sent_at: sent_at.to_owned(),
                    stored_at: sent_at.to_owned(),
                    metadata: r#"{"peer_user_id":"user-bob","peer_full_handle":"bob.anpclaw.com"}"#
                        .to_owned(),
                    ..crate::internal::local_state::messages::MessageRecord::default()
                },
            )
            .unwrap();
        }

        fn seed_verified_peer_identity(&self, client: &crate::core::ImClient, peer_dids: &[&str]) {
            let connection = crate::internal::local_state::open_writable(
                &client.core_inner().sdk_paths().local_state.sqlite_path,
            )
            .unwrap();
            let persona = crate::internal::canonical_identity::PeerPersona::from_verified_handle(
                "anpclaw.com",
                "user-bob",
                "bob.anpclaw.com",
                Some("active"),
            )
            .unwrap();
            crate::internal::local_state::peer_personas::upsert(
                &connection,
                &crate::internal::local_state::peer_personas::PeerPersonaRecord {
                    owner_identity_id: client.current_identity().id.as_str().to_owned(),
                    persona: persona.clone(),
                    binding_generation: Some("2".to_owned()),
                    subject_type: "human".to_owned(),
                    source: "test_authority".to_owned(),
                    authority_revision: None,
                    verified_at: "2026-07-14T00:00:00Z".to_owned(),
                },
            )
            .unwrap();
            for (index, did) in peer_dids.iter().enumerate() {
                crate::internal::local_state::peer_identifiers::bind(
                    &connection,
                    &crate::internal::local_state::peer_identifiers::PeerIdentifierRecord {
                        owner_identity_id: client.current_identity().id.as_str().to_owned(),
                        peer_persona_id: persona.peer_persona_id.clone(),
                        identifier_kind: "did".to_owned(),
                        identifier_value: (*did).to_owned(),
                        is_current: index + 1 == peer_dids.len(),
                        binding_generation: Some((index + 1).to_string()),
                        source: "test_authority".to_owned(),
                        verified_at: "2026-07-14T00:00:00Z".to_owned(),
                    },
                )
                .unwrap();
            }
            let route = crate::internal::local_state::direct_peer_routes::DirectPeerRouteRecord::from_verified_persona(
                client.current_identity().id.as_str(),
                &persona,
                peer_dids.last().copied().unwrap(),
            )
            .unwrap();
            crate::internal::local_state::direct_peer_routes::upsert(&connection, &route).unwrap();
            crate::internal::local_state::conversation_registry::ensure(
                &connection,
                &crate::internal::local_state::conversation_registry::ConversationRegistryRecord {
                    owner_identity_id: client.current_identity().id.as_str().to_owned(),
                    owner_did: client.did().as_str().to_owned(),
                    conversation_id: persona.direct_conversation_id(),
                    thread_kind: "direct".to_owned(),
                    thread_id: persona.direct_conversation_id(),
                    activity_at: "2026-07-14T00:00:00Z".to_owned(),
                },
            )
            .unwrap();
        }

        fn seed_legacy_direct_message(
            &self,
            client: &crate::core::ImClient,
            message_id: &str,
            sender_did: &str,
            content: &str,
            sent_at: &str,
        ) {
            let connection = crate::internal::local_state::open_writable(
                &client.core_inner().sdk_paths().local_state.sqlite_path,
            )
            .unwrap();
            let conversation_id =
                crate::internal::local_state::owner_scope::direct_conversation_id(sender_did);
            crate::internal::local_state::messages::upsert_message(
                &connection,
                &crate::internal::local_state::messages::MessageRecord {
                    msg_id: message_id.to_owned(),
                    owner_identity_id: client.current_identity().id.as_str().to_owned(),
                    owner_did: client.did().as_str().to_owned(),
                    conversation_id: conversation_id.clone(),
                    thread_id: conversation_id,
                    direction: 0,
                    sender_did: sender_did.to_owned(),
                    receiver_did: client.did().as_str().to_owned(),
                    content_type: "text/plain".to_owned(),
                    content: content.to_owned(),
                    sent_at: sent_at.to_owned(),
                    stored_at: sent_at.to_owned(),
                    ..crate::internal::local_state::messages::MessageRecord::default()
                },
            )
            .unwrap();
        }
    }

    fn unique_temp_root() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "im-core-conversations-runtime-{}-{nanos}",
            std::process::id()
        ))
    }
}
