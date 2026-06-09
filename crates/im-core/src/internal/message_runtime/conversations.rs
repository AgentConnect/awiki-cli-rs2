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
        if records.is_empty() && should_refresh_projection(&query) {
            refresh_conversation_projection(self.client, &query, requested_limit)?;
            records = list_conversation_records(self.client, &query)?;
        }
        let has_more = records.len() > requested_limit;
        records.truncate(requested_limit);
        let items = records
            .into_iter()
            .map(|record| conversation_from_record(self.client.did().as_str(), record))
            .collect::<crate::ImResult<Vec<_>>>()?;
        Ok(crate::ids::Page {
            items,
            next_cursor: None,
            has_more,
        })
    }

    pub(crate) async fn conversations_async(
        self,
        query: crate::messages::ConversationQuery,
    ) -> crate::ImResult<crate::ids::Page<crate::messages::Conversation>> {
        let requested_limit = page_limit(query.limit, 50);
        let mut records = list_conversation_records_async(self.client, &query).await?;
        if records.is_empty() && should_refresh_projection(&query) {
            refresh_conversation_projection_async(self.client, &query, requested_limit).await?;
            records = list_conversation_records_async(self.client, &query).await?;
        }
        let has_more = records.len() > requested_limit;
        records.truncate(requested_limit);
        let items = records
            .into_iter()
            .map(|record| conversation_from_record(self.client.did().as_str(), record))
            .collect::<crate::ImResult<Vec<_>>>()?;
        Ok(crate::ids::Page {
            items,
            next_cursor: None,
            has_more,
        })
    }
}

fn should_refresh_projection(query: &crate::messages::ConversationQuery) -> bool {
    !query.unread_only && (query.include_direct || query.include_groups)
}

#[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
fn refresh_conversation_projection(
    client: &crate::core::ImClient,
    query: &crate::messages::ConversationQuery,
    requested_limit: usize,
) -> crate::ImResult<()> {
    refresh_projection_from_inbox(client, query, requested_limit)?;
    if query.include_direct {
        refresh_projection_from_contact_history(client, requested_limit)?;
    }
    if query.include_groups {
        refresh_projection_from_group_history(client, requested_limit)?;
    }
    Ok(())
}

#[cfg(all(feature = "sqlite", not(any(feature = "blocking", test))))]
fn refresh_conversation_projection(
    _client: &crate::core::ImClient,
    _query: &crate::messages::ConversationQuery,
    _requested_limit: usize,
) -> crate::ImResult<()> {
    Err(crate::ImError::unsupported("sync-message-conversations"))
}

#[cfg(feature = "sqlite")]
async fn refresh_conversation_projection_async(
    client: &crate::core::ImClient,
    query: &crate::messages::ConversationQuery,
    requested_limit: usize,
) -> crate::ImResult<()> {
    refresh_projection_from_inbox_async(client, query, requested_limit).await?;
    if query.include_direct {
        refresh_projection_from_contact_history_async(client, requested_limit).await?;
    }
    if query.include_groups {
        refresh_projection_from_group_history_async(client, requested_limit).await?;
    }
    Ok(())
}

#[cfg(not(feature = "sqlite"))]
async fn refresh_conversation_projection_async(
    _client: &crate::core::ImClient,
    _query: &crate::messages::ConversationQuery,
    _requested_limit: usize,
) -> crate::ImResult<()> {
    Ok(())
}

#[cfg(not(feature = "sqlite"))]
fn refresh_conversation_projection(
    _client: &crate::core::ImClient,
    _query: &crate::messages::ConversationQuery,
    _requested_limit: usize,
) -> crate::ImResult<()> {
    Ok(())
}

#[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
fn refresh_projection_from_inbox(
    client: &crate::core::ImClient,
    query: &crate::messages::ConversationQuery,
    requested_limit: usize,
) -> crate::ImResult<()> {
    let scope = match (query.include_direct, query.include_groups) {
        (true, true) => crate::messages::InboxScope::All,
        (true, false) => crate::messages::InboxScope::DirectOnly,
        (false, true) => crate::messages::InboxScope::GroupOnly,
        (false, false) => return Ok(()),
    };
    let limit = u32::try_from(requested_limit.max(50))
        .unwrap_or(u32::MAX)
        .min(100);
    client.messages().inbox(crate::messages::InboxQuery {
        scope,
        limit: crate::ids::PageLimit(limit),
        cursor: None,
        unread_only: false,
        inbox_history_options: None,
    })?;
    Ok(())
}

#[cfg(all(feature = "sqlite", not(any(feature = "blocking", test))))]
fn refresh_projection_from_inbox(
    _client: &crate::core::ImClient,
    _query: &crate::messages::ConversationQuery,
    _requested_limit: usize,
) -> crate::ImResult<()> {
    Err(crate::ImError::unsupported("sync-message-conversations"))
}

#[cfg(feature = "sqlite")]
async fn refresh_projection_from_inbox_async(
    client: &crate::core::ImClient,
    query: &crate::messages::ConversationQuery,
    requested_limit: usize,
) -> crate::ImResult<()> {
    let scope = match (query.include_direct, query.include_groups) {
        (true, true) => crate::messages::InboxScope::All,
        (true, false) => crate::messages::InboxScope::DirectOnly,
        (false, true) => crate::messages::InboxScope::GroupOnly,
        (false, false) => return Ok(()),
    };
    let limit = u32::try_from(requested_limit.max(50))
        .unwrap_or(u32::MAX)
        .min(100);
    client
        .messages()
        .inbox_async(crate::messages::InboxQuery {
            scope,
            limit: crate::ids::PageLimit(limit),
            cursor: None,
            unread_only: false,
            inbox_history_options: None,
        })
        .await?;
    Ok(())
}

#[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
fn refresh_projection_from_contact_history(
    client: &crate::core::ImClient,
    requested_limit: usize,
) -> crate::ImResult<()> {
    let candidates = list_direct_history_candidates(client, requested_limit)?;
    for peer in candidates {
        let Ok(peer) = crate::ids::PeerRef::parse(&peer, "") else {
            continue;
        };
        let _ = client.messages().history(
            crate::messages::ThreadRef::Direct(peer),
            crate::messages::HistoryQuery {
                limit: crate::ids::PageLimit(1),
                cursor: None,
                inbox_history_options: None,
            },
        );
    }
    Ok(())
}

#[cfg(all(feature = "sqlite", not(any(feature = "blocking", test))))]
fn refresh_projection_from_contact_history(
    _client: &crate::core::ImClient,
    _requested_limit: usize,
) -> crate::ImResult<()> {
    Err(crate::ImError::unsupported("sync-message-conversations"))
}

#[cfg(feature = "sqlite")]
async fn refresh_projection_from_contact_history_async(
    client: &crate::core::ImClient,
    requested_limit: usize,
) -> crate::ImResult<()> {
    let candidates = list_direct_history_candidates_async(client, requested_limit).await?;
    for peer in candidates {
        let Ok(peer) = crate::ids::PeerRef::parse(&peer, "") else {
            continue;
        };
        let _ = client
            .messages()
            .history_async(
                crate::messages::ThreadRef::Direct(peer),
                crate::messages::HistoryQuery {
                    limit: crate::ids::PageLimit(1),
                    cursor: None,
                    inbox_history_options: None,
                },
            )
            .await;
    }
    Ok(())
}

#[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
fn refresh_projection_from_group_history(
    client: &crate::core::ImClient,
    requested_limit: usize,
) -> crate::ImResult<()> {
    refresh_group_list_best_effort(client, requested_limit);
    let candidates = list_group_history_candidates(client, requested_limit)?;
    for group in candidates {
        let Ok(group) = crate::ids::GroupRef::parse(&group) else {
            continue;
        };
        let _ = client.messages().history(
            crate::messages::ThreadRef::Group(group),
            crate::messages::HistoryQuery {
                limit: crate::ids::PageLimit(1),
                cursor: None,
                inbox_history_options: None,
            },
        );
    }
    Ok(())
}

#[cfg(all(feature = "sqlite", not(any(feature = "blocking", test))))]
fn refresh_projection_from_group_history(
    _client: &crate::core::ImClient,
    _requested_limit: usize,
) -> crate::ImResult<()> {
    Err(crate::ImError::unsupported("sync-message-conversations"))
}

#[cfg(feature = "sqlite")]
async fn refresh_projection_from_group_history_async(
    client: &crate::core::ImClient,
    requested_limit: usize,
) -> crate::ImResult<()> {
    refresh_group_list_best_effort_async(client, requested_limit).await;
    let candidates = list_group_history_candidates_async(client, requested_limit).await?;
    for group in candidates {
        let Ok(group) = crate::ids::GroupRef::parse(&group) else {
            continue;
        };
        let _ = client
            .messages()
            .history_async(
                crate::messages::ThreadRef::Group(group),
                crate::messages::HistoryQuery {
                    limit: crate::ids::PageLimit(1),
                    cursor: None,
                    inbox_history_options: None,
                },
            )
            .await;
    }
    Ok(())
}

#[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
fn refresh_group_list_best_effort(client: &crate::core::ImClient, requested_limit: usize) {
    let limit = u32::try_from(requested_limit.max(50))
        .unwrap_or(u32::MAX)
        .min(100);
    let _ = client.groups().list(crate::groups::GroupListRequest {
        limit: crate::ids::PageLimit(limit),
    });
}

#[cfg(all(feature = "sqlite", not(any(feature = "blocking", test))))]
fn refresh_group_list_best_effort(_client: &crate::core::ImClient, _requested_limit: usize) {}

#[cfg(feature = "sqlite")]
async fn refresh_group_list_best_effort_async(
    client: &crate::core::ImClient,
    requested_limit: usize,
) {
    let limit = u32::try_from(requested_limit.max(50))
        .unwrap_or(u32::MAX)
        .min(100);
    let _ = client
        .groups()
        .list_async(crate::groups::GroupListRequest {
            limit: crate::ids::PageLimit(limit),
        })
        .await;
}

#[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
fn list_direct_history_candidates(
    client: &crate::core::ImClient,
    requested_limit: usize,
) -> crate::ImResult<Vec<String>> {
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    let limit = i64::try_from(requested_limit.max(50))
        .unwrap_or(i64::MAX)
        .min(100);
    crate::internal::contact_store::records::list_contact_dids_for_message_history_recovery(
        &connection,
        client.current_identity().id.as_str(),
        client.did().as_str(),
        limit,
    )
}

#[cfg(all(feature = "sqlite", not(any(feature = "blocking", test))))]
fn list_direct_history_candidates(
    _client: &crate::core::ImClient,
    _requested_limit: usize,
) -> crate::ImResult<Vec<String>> {
    Err(crate::ImError::unsupported("sync-message-conversations"))
}

#[cfg(feature = "sqlite")]
async fn list_direct_history_candidates_async(
    client: &crate::core::ImClient,
    requested_limit: usize,
) -> crate::ImResult<Vec<String>> {
    let limit = i64::try_from(requested_limit.max(50))
        .unwrap_or(i64::MAX)
        .min(100);
    client
        .core_inner()
        .local_state_db()
        .await?
        .list_contact_dids_for_message_history_recovery(
            client.current_identity().id.as_str(),
            client.did().as_str(),
            limit,
        )
        .await
}

#[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
fn list_group_history_candidates(
    client: &crate::core::ImClient,
    requested_limit: usize,
) -> crate::ImResult<Vec<String>> {
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    let limit = i64::try_from(requested_limit.max(50))
        .unwrap_or(i64::MAX)
        .min(100);
    crate::internal::local_state::groups::list_active_group_refs_for_owner_identity(
        &connection,
        client.current_identity().id.as_str(),
        client.did().as_str(),
        limit,
    )
}

#[cfg(all(feature = "sqlite", not(any(feature = "blocking", test))))]
fn list_group_history_candidates(
    _client: &crate::core::ImClient,
    _requested_limit: usize,
) -> crate::ImResult<Vec<String>> {
    Err(crate::ImError::unsupported("sync-message-conversations"))
}

#[cfg(feature = "sqlite")]
async fn list_group_history_candidates_async(
    client: &crate::core::ImClient,
    requested_limit: usize,
) -> crate::ImResult<Vec<String>> {
    let limit = i64::try_from(requested_limit.max(50))
        .unwrap_or(i64::MAX)
        .min(100);
    client
        .core_inner()
        .local_state_db()
        .await?
        .list_active_group_refs(
            client.current_identity().id.as_str(),
            client.did().as_str(),
            limit,
        )
        .await
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
    let participants = conversation_participants(owner_did, &thread, last_message.as_ref())?;
    Ok(crate::messages::Conversation {
        thread,
        title: None,
        participants,
        last_message,
        unread_count: u32_count(record.unread_count()),
        message_count: u32_count(record.message_count()),
        last_message_at: non_empty_string(record.last_message_at()),
    })
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

    fn message_count(&self) -> i64 {
        self.message_count
    }

    fn unread_count(&self) -> i64 {
        self.unread_count
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

    fn message_count(&self) -> i64 {
        0
    }

    fn unread_count(&self) -> i64 {
        0
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
    fn message_count(&self) -> i64;
    fn unread_count(&self) -> i64;
    fn last_message_at(&self) -> &str;
    fn last_message(&self) -> Option<&crate::internal::local_state::messages::MessageRecord>;
}

fn conversation_thread(
    owner_did: &str,
    record: &impl ConversationRecordExt,
    last_message: Option<&crate::messages::Message>,
) -> crate::ImResult<crate::messages::ThreadRef> {
    if let Some(message) = last_message {
        if let Some(group) = &message.group {
            return Ok(crate::messages::ThreadRef::Group(group.clone()));
        }
        if let Some(peer) = direct_peer_from_message(owner_did, message) {
            return Ok(crate::messages::ThreadRef::Direct(peer));
        }
    }
    let thread_id = record.thread_id().trim();
    if let Some(group) = thread_id.strip_prefix("group:") {
        return Ok(crate::messages::ThreadRef::Group(
            crate::ids::GroupRef::parse(group)?,
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
                Some(&message.sender),
                message.receiver.as_ref(),
                direct_peer_from_message(owner_did, message).as_ref(),
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
    if message.sender.as_str() != owner_did {
        return Some(message.sender.clone());
    }
    message.receiver.clone()
}

fn message_from_record(
    record: &crate::internal::local_state::messages::MessageRecord,
) -> crate::ImResult<crate::messages::Message> {
    let thread = message_thread(record)?;
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
        group: group_ref_from_record(record)?,
        body: message_body(record),
        sent_at: non_empty_string(&record.sent_at),
        received_at: None,
        metadata: crate::messages::MessageMetadata {
            operation_id: metadata_string(&record.metadata, "operation_id"),
            delivery_state: metadata_string(&record.metadata, "delivery_state"),
            send_state,
            retry_plan,
            server_sequence: record.server_seq,
            content_type: non_empty_string(&record.content_type),
            attributes: Vec::new(),
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
) -> crate::messages::MessageBodyView {
    let content_type = non_empty_string(&record.content_type);
    if content_type.as_deref() == Some("application/json") {
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
    fn message_state_conversation_projection_reads_local_metadata_retry_plan() {
        let fixture = Fixture::new();
        let client = fixture.client();
        fixture.seed_message_with_metadata(
            &client,
            "direct-failed",
            1,
            "failed outgoing",
            "2026-05-21T00:00:05Z",
            r#"{"delivery_state":"failed","operation_id":"op-failed","failure_reason":"timeout"}"#,
        );

        let page = MessageConversationRuntime::new(&client)
            .conversations(crate::messages::ConversationQuery {
                limit: crate::ids::PageLimit(10),
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
            connection
                .execute(
                    r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, conversation_id, thread_id, direction, sender_did, receiver_did, group_id, group_did,
     content_type, content, sent_at, stored_at, is_read)
VALUES (?1, ?2, ?3, ?4, ?4, ?5, ?6, ?7, ?8, ?8, 'text/plain', ?9, ?10, ?10, ?11)"#,
                    (
                        message_id,
                        client.current_identity().id.as_str(),
                        client.did().as_str(),
                        conversation_id,
                        direction,
                        sender_did,
                        receiver_did,
                        group_did,
                        content,
                        sent_at,
                        is_read,
                    ),
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
            connection
                .execute(
                    r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, conversation_id, thread_id, direction, sender_did, receiver_did,
     content_type, content, sent_at, stored_at, is_read, metadata)
VALUES (?1, ?2, ?3, 'dm:did:example:bob', 'dm:did:example:bob', ?4, ?5, ?6,
        'text/plain', ?7, ?8, ?8, 1, ?9)"#,
                    (
                        message_id,
                        client.current_identity().id.as_str(),
                        client.did().as_str(),
                        direction,
                        sender_did,
                        receiver_did,
                        content,
                        sent_at,
                        metadata,
                    ),
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
