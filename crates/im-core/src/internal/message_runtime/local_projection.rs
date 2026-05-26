use serde_json::{Map, Value};

#[cfg(feature = "sqlite")]
pub(crate) fn persist_messages(
    client: &crate::core::ImClient,
    messages: &[crate::messages::Message],
) -> crate::ImResult<()> {
    if messages.is_empty() {
        return Ok(());
    }
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    let records = messages
        .iter()
        .map(|message| message_record_from_message(client, message))
        .collect::<crate::ImResult<Vec<_>>>()?;
    crate::internal::local_state::messages::upsert_messages(&connection, &records)
}

#[cfg(not(feature = "sqlite"))]
pub(crate) fn persist_messages(
    _client: &crate::core::ImClient,
    _messages: &[crate::messages::Message],
) -> crate::ImResult<()> {
    Ok(())
}

#[cfg(feature = "sqlite")]
pub(crate) fn persist_direct_outgoing(
    client: &crate::core::ImClient,
    target_did: &str,
    target_handle: Option<&str>,
    text: &str,
    kind: &crate::messages::MessageKind,
    sdk_result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    crate::internal::local_state::messages::upsert_message(
        &connection,
        &crate::internal::local_state::messages::MessageRecord {
            msg_id: sdk_result.message.id.as_str().to_owned(),
            owner_identity_id: client.current_identity().id.as_str().to_owned(),
            owner_did: client.did().as_str().to_owned(),
            thread_id: direct_thread_id(client.did().as_str(), target_did),
            direction: 1,
            sender_did: client.did().as_str().to_owned(),
            receiver_did: target_did.trim().to_owned(),
            content_type: content_type_for_kind(kind).to_owned(),
            content: text.to_owned(),
            server_seq: sdk_result.message.metadata.server_sequence,
            sent_at: sdk_result.message.sent_at.clone().unwrap_or_default(),
            is_e2ee: false,
            is_read: true,
            metadata: delivery_metadata_json(
                &sdk_result.message.metadata,
                [("target_handle", target_handle.unwrap_or_default())],
            ),
            credential_name: credential_name(client),
            ..crate::internal::local_state::messages::MessageRecord::default()
        },
    )
}

#[cfg(feature = "sqlite")]
pub(crate) fn persist_group_outgoing(
    client: &crate::core::ImClient,
    group_did: &str,
    text: &str,
    kind: &crate::messages::MessageKind,
    sdk_result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    crate::internal::local_state::messages::upsert_message(
        &connection,
        &crate::internal::local_state::messages::MessageRecord {
            msg_id: sdk_result.message.id.as_str().to_owned(),
            owner_identity_id: client.current_identity().id.as_str().to_owned(),
            owner_did: client.did().as_str().to_owned(),
            thread_id: group_thread_id(group_did),
            direction: 1,
            sender_did: client.did().as_str().to_owned(),
            group_id: group_storage_key(group_did),
            group_did: group_did.trim().to_owned(),
            content_type: content_type_for_kind(kind).to_owned(),
            content: text.to_owned(),
            server_seq: sdk_result.message.metadata.server_sequence,
            sent_at: sdk_result.message.sent_at.clone().unwrap_or_default(),
            is_e2ee: false,
            is_read: true,
            metadata: delivery_metadata_json(&sdk_result.message.metadata, []),
            credential_name: credential_name(client),
            ..crate::internal::local_state::messages::MessageRecord::default()
        },
    )?;
    touch_group_after_outgoing(&connection, client, group_did, sdk_result)
}

#[cfg(feature = "sqlite")]
pub(crate) fn persist_direct_attachment_outgoing(
    client: &crate::core::ImClient,
    target_did: &str,
    target_handle: Option<&str>,
    manifest: &Value,
    sdk_result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    crate::internal::local_state::messages::upsert_message(
        &connection,
        &crate::internal::local_state::messages::MessageRecord {
            msg_id: sdk_result.message.id.as_str().to_owned(),
            owner_identity_id: client.current_identity().id.as_str().to_owned(),
            owner_did: client.did().as_str().to_owned(),
            thread_id: direct_thread_id(client.did().as_str(), target_did),
            direction: 1,
            sender_did: client.did().as_str().to_owned(),
            receiver_did: target_did.trim().to_owned(),
            content_type: crate::attachments::manifest::attachment_manifest_content_type()
                .to_owned(),
            content: crate::attachments::manifest::manifest_content_string(manifest),
            server_seq: sdk_result.message.metadata.server_sequence,
            sent_at: sdk_result.message.sent_at.clone().unwrap_or_default(),
            is_e2ee: false,
            is_read: true,
            metadata: delivery_metadata_json(
                &sdk_result.message.metadata,
                attachment_metadata_extras(manifest, target_handle),
            ),
            credential_name: credential_name(client),
            ..crate::internal::local_state::messages::MessageRecord::default()
        },
    )
}

#[cfg(feature = "sqlite")]
pub(crate) fn persist_group_attachment_outgoing(
    client: &crate::core::ImClient,
    group_did: &str,
    manifest: &Value,
    sdk_result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    crate::internal::local_state::messages::upsert_message(
        &connection,
        &crate::internal::local_state::messages::MessageRecord {
            msg_id: sdk_result.message.id.as_str().to_owned(),
            owner_identity_id: client.current_identity().id.as_str().to_owned(),
            owner_did: client.did().as_str().to_owned(),
            thread_id: group_thread_id(group_did),
            direction: 1,
            sender_did: client.did().as_str().to_owned(),
            group_id: group_storage_key(group_did),
            group_did: group_did.trim().to_owned(),
            content_type: crate::attachments::manifest::attachment_manifest_content_type()
                .to_owned(),
            content: crate::attachments::manifest::manifest_content_string(manifest),
            server_seq: sdk_result.message.metadata.server_sequence,
            sent_at: sdk_result.message.sent_at.clone().unwrap_or_default(),
            is_e2ee: false,
            is_read: true,
            metadata: delivery_metadata_json(
                &sdk_result.message.metadata,
                attachment_metadata_extras(manifest, None),
            ),
            credential_name: credential_name(client),
            ..crate::internal::local_state::messages::MessageRecord::default()
        },
    )?;
    touch_group_after_outgoing(&connection, client, group_did, sdk_result)
}

#[cfg(feature = "sqlite")]
pub(crate) fn peer_dids_for_handle(
    client: &crate::core::ImClient,
    handle: &str,
    current_did: &str,
) -> crate::ImResult<Vec<String>> {
    let connection = crate::internal::contact_store::open_writable(client)?;
    let dids = crate::internal::contact_store::records::list_dids_by_handle_for_owner_identity(
        &connection,
        client.current_identity().id.as_str(),
        client.did().as_str(),
        &normalize_handle_value(handle),
    )?;
    Ok(merge_peer_dids(current_did, &dids))
}

#[cfg(feature = "sqlite")]
pub(crate) fn persist_direct_e2ee_outgoing(
    connection: &rusqlite::Connection,
    client: &crate::core::ImClient,
    target_did: &str,
    text: &str,
    kind: &crate::messages::MessageKind,
    sdk_result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    crate::internal::local_state::messages::upsert_message(
        connection,
        &crate::internal::local_state::messages::MessageRecord {
            msg_id: sdk_result.message.id.as_str().to_owned(),
            owner_identity_id: client.current_identity().id.as_str().to_owned(),
            owner_did: client.did().as_str().to_owned(),
            thread_id: direct_thread_id(client.did().as_str(), target_did),
            direction: 1,
            sender_did: client.did().as_str().to_owned(),
            receiver_did: target_did.trim().to_owned(),
            content_type: content_type_for_kind(kind).to_owned(),
            content: text.to_owned(),
            server_seq: sdk_result.message.metadata.server_sequence,
            sent_at: sdk_result.message.sent_at.clone().unwrap_or_default(),
            is_e2ee: true,
            is_read: true,
            metadata: secure_metadata_json("direct-e2ee", &sdk_result.message.metadata),
            credential_name: credential_name(client),
            ..crate::internal::local_state::messages::MessageRecord::default()
        },
    )
}

#[cfg(feature = "group-e2ee")]
pub(crate) fn persist_group_e2ee_outgoing(
    client: &crate::core::ImClient,
    group_did: &str,
    text: &str,
    kind: &crate::messages::MessageKind,
    sdk_result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    crate::internal::local_state::messages::upsert_message(
        &connection,
        &crate::internal::local_state::messages::MessageRecord {
            msg_id: sdk_result.message.id.as_str().to_owned(),
            owner_identity_id: client.current_identity().id.as_str().to_owned(),
            owner_did: client.did().as_str().to_owned(),
            thread_id: group_thread_id(group_did),
            direction: 1,
            sender_did: client.did().as_str().to_owned(),
            group_id: group_did.trim().to_owned(),
            group_did: group_did.trim().to_owned(),
            content_type: content_type_for_kind(kind).to_owned(),
            content: text.to_owned(),
            server_seq: sdk_result.message.metadata.server_sequence,
            sent_at: sdk_result.message.sent_at.clone().unwrap_or_default(),
            is_e2ee: true,
            is_read: true,
            metadata: secure_metadata_json("group-e2ee", &sdk_result.message.metadata),
            credential_name: credential_name(client),
            ..crate::internal::local_state::messages::MessageRecord::default()
        },
    )
}

pub(crate) fn direct_thread_id(owner_did: &str, peer_did: &str) -> String {
    let owner_did = owner_did.trim();
    let peer_did = peer_did.trim();
    if peer_did.is_empty() {
        return format!("dm:{owner_did}:unknown");
    }
    let mut pair = [owner_did.to_owned(), peer_did.to_owned()];
    pair.sort();
    format!("dm:{}:{}", pair[0], pair[1])
}

pub(crate) fn group_thread_id(group_did: &str) -> String {
    format!("group:{}", group_did.trim())
}

fn group_storage_key(group_did: &str) -> String {
    group_did.trim().to_owned()
}

#[cfg(feature = "sqlite")]
fn touch_group_after_outgoing(
    connection: &rusqlite::Connection,
    client: &crate::core::ImClient,
    group_did: &str,
    sdk_result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    crate::internal::local_state::groups::upsert_group(
        connection,
        crate::internal::local_state::groups::GroupRecord {
            owner_identity_id: client.current_identity().id.as_str().to_owned(),
            owner_did: client.did().as_str().to_owned(),
            group_id: group_storage_key(group_did),
            group_did: group_did.trim().to_owned(),
            membership_status: "active".to_owned(),
            last_synced_seq: sdk_result.message.metadata.server_sequence,
            last_message_at: sdk_result.message.sent_at.clone().unwrap_or_default(),
            metadata: delivery_metadata_json(&sdk_result.message.metadata, []),
            credential_name: credential_name(client),
            ..crate::internal::local_state::groups::GroupRecord::default()
        },
    )
}

fn delivery_metadata_json<'a, I>(metadata: &crate::messages::MessageMetadata, extras: I) -> String
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    let mut object = Map::new();
    insert_string(
        &mut object,
        "operation_id",
        metadata.operation_id.as_deref(),
    );
    insert_string(
        &mut object,
        "delivery_state",
        metadata.delivery_state.as_deref(),
    );
    insert_string(
        &mut object,
        "content_type",
        metadata.content_type.as_deref(),
    );
    if let Some(server_sequence) = metadata.server_sequence {
        object.insert(
            "server_sequence".to_owned(),
            Value::Number(server_sequence.into()),
        );
        object.insert(
            "group_event_seq".to_owned(),
            Value::String(server_sequence.to_string()),
        );
    }
    if let Some(send_state) = metadata.send_state.as_ref() {
        object.insert(
            "send_state".to_owned(),
            serde_json::to_value(send_state).unwrap_or(Value::Null),
        );
    }
    if let Some(retry_plan) = metadata.retry_plan.as_ref() {
        object.insert(
            "retry_plan".to_owned(),
            serde_json::to_value(retry_plan).unwrap_or(Value::Null),
        );
    }
    for attribute in &metadata.attributes {
        match attribute.key.as_str() {
            "raw_message_id"
            | "group_event_seq"
            | "group_state_version"
            | "attachment_id"
            | "object_uri"
                if !attribute.value.trim().is_empty() =>
            {
                object.insert(
                    attribute.key.clone(),
                    Value::String(attribute.value.clone()),
                );
            }
            "attachment_manifest" => {
                insert_attachment_manifest_fields(&mut object, &attribute.value);
            }
            _ => {}
        }
    }
    for (key, value) in extras {
        insert_string(&mut object, key, Some(value));
    }
    Value::Object(object).to_string()
}

fn attachment_metadata_extras<'a>(
    manifest: &'a Value,
    target_handle: Option<&'a str>,
) -> Vec<(&'a str, &'a str)> {
    let mut extras = Vec::new();
    if let Some(target_handle) = target_handle {
        extras.push(("target_handle", target_handle));
    }
    if let Some(caption) = manifest
        .get("caption")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
    {
        extras.push(("caption", caption));
    }
    if let Some(attachment) = manifest
        .get("attachments")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
    {
        if let Some(attachment_id) = attachment
            .get("attachment_id")
            .or_else(|| attachment.get("id"))
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
        {
            extras.push(("attachment_id", attachment_id));
        }
        if let Some(object_uri) = attachment
            .get("object_uri")
            .or_else(|| {
                attachment
                    .get("access_info")
                    .and_then(|value| value.get("object_uri"))
            })
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
        {
            extras.push(("object_uri", object_uri));
        }
    }
    extras
}

fn insert_attachment_manifest_fields(object: &mut Map<String, Value>, manifest: &str) {
    let Ok(value) = serde_json::from_str::<Value>(manifest) else {
        return;
    };
    if let Some(caption) = value
        .get("caption")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
    {
        object.insert("caption".to_owned(), Value::String(caption.to_owned()));
    }
    let Some(attachment) = value
        .get("attachments")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
    else {
        return;
    };
    if let Some(attachment_id) = attachment
        .get("attachment_id")
        .or_else(|| attachment.get("id"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
    {
        object.insert(
            "attachment_id".to_owned(),
            Value::String(attachment_id.to_owned()),
        );
    }
    if let Some(object_uri) = attachment
        .get("object_uri")
        .or_else(|| {
            attachment
                .get("access_info")
                .and_then(|value| value.get("object_uri"))
        })
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
    {
        object.insert(
            "object_uri".to_owned(),
            Value::String(object_uri.to_owned()),
        );
    }
}

fn secure_metadata_json(security: &str, metadata: &crate::messages::MessageMetadata) -> String {
    let mut object = Map::new();
    object.insert("security".to_owned(), Value::String(security.to_owned()));
    object.insert(
        "redaction_version".to_owned(),
        Value::String("v1".to_owned()),
    );
    object.insert("contains_sensitive".to_owned(), Value::Bool(false));
    insert_string(
        &mut object,
        "operation_id",
        metadata.operation_id.as_deref(),
    );
    insert_string(
        &mut object,
        "delivery_state",
        metadata.delivery_state.as_deref(),
    );
    insert_string(
        &mut object,
        "content_type",
        metadata.content_type.as_deref(),
    );
    if let Some(server_sequence) = metadata.server_sequence {
        object.insert(
            "server_sequence".to_owned(),
            Value::Number(server_sequence.into()),
        );
    }
    if let Some(send_state) = metadata.send_state.as_ref() {
        object.insert(
            "send_state".to_owned(),
            serde_json::to_value(send_state).unwrap_or(Value::Null),
        );
    }
    if let Some(retry_plan) = metadata.retry_plan.as_ref() {
        object.insert(
            "retry_plan".to_owned(),
            serde_json::to_value(retry_plan).unwrap_or(Value::Null),
        );
    }
    for attribute in &metadata.attributes {
        match attribute.key.as_str() {
            "raw_message_id" | "group_event_seq" | "group_state_version" | "secure_outbox_id"
                if !attribute.value.trim().is_empty() =>
            {
                object.insert(
                    attribute.key.clone(),
                    Value::String(attribute.value.clone()),
                );
            }
            "security" => {}
            _ => {}
        }
    }
    Value::Object(object).to_string()
}

fn insert_string(object: &mut Map<String, Value>, key: &str, value: Option<&str>) {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    object.insert(key.to_owned(), Value::String(value.to_owned()));
}

fn credential_name(client: &crate::core::ImClient) -> String {
    client.current_identity().id.as_str().to_owned()
}

#[cfg(feature = "sqlite")]
fn message_record_from_message(
    client: &crate::core::ImClient,
    message: &crate::messages::Message,
) -> crate::ImResult<crate::internal::local_state::messages::MessageRecord> {
    let (content_type, content) = body_projection(&message.body);
    let direction = direction_value_for_message(client.did().as_str(), message);
    Ok(crate::internal::local_state::messages::MessageRecord {
        msg_id: message.id.as_str().to_owned(),
        owner_identity_id: client.current_identity().id.as_str().to_owned(),
        owner_did: client.did().as_str().to_owned(),
        thread_id: thread_id_for_message(client.did().as_str(), message),
        direction,
        sender_did: message.sender.as_str().to_owned(),
        receiver_did: message
            .receiver
            .as_ref()
            .map(|receiver| receiver.as_str().to_owned())
            .unwrap_or_default(),
        group_id: group_ref_for_message(message).unwrap_or_default(),
        group_did: group_ref_for_message(message).unwrap_or_default(),
        content_type,
        content,
        server_seq: message.metadata.server_sequence,
        sent_at: message.sent_at.clone().unwrap_or_default(),
        stored_at: message
            .received_at
            .clone()
            .or_else(|| message.sent_at.clone())
            .unwrap_or_default(),
        is_e2ee: is_e2ee_message(message),
        is_read: read_state_for_message(direction, message),
        metadata: read_metadata_json(&message.metadata),
        credential_name: credential_name(client),
        ..crate::internal::local_state::messages::MessageRecord::default()
    })
}

#[cfg(feature = "sqlite")]
fn thread_id_for_message(owner_did: &str, message: &crate::messages::Message) -> String {
    if let Some(group) = group_ref_for_message(message) {
        return group_thread_id(&group);
    }
    let peer = direct_peer_for_message(owner_did, message);
    if !peer.trim().is_empty() {
        return direct_thread_id(owner_did, &peer);
    }
    match &message.thread {
        crate::messages::ThreadRef::Thread(thread) => thread.as_str().to_owned(),
        crate::messages::ThreadRef::Direct(peer) => direct_thread_id(owner_did, peer.as_str()),
        crate::messages::ThreadRef::Group(group) => group_thread_id(group.as_str()),
    }
}

#[cfg(feature = "sqlite")]
fn direct_peer_for_message(owner_did: &str, message: &crate::messages::Message) -> String {
    if message.sender.as_str() != owner_did {
        return message.sender.as_str().to_owned();
    }
    if let Some(receiver) = message.receiver.as_ref() {
        return receiver.as_str().to_owned();
    }
    match &message.thread {
        crate::messages::ThreadRef::Direct(peer) => peer.as_str().to_owned(),
        _ => String::new(),
    }
}

#[cfg(feature = "sqlite")]
fn group_ref_for_message(message: &crate::messages::Message) -> Option<String> {
    message
        .group
        .as_ref()
        .map(|group| group.as_str().to_owned())
        .or_else(|| match &message.thread {
            crate::messages::ThreadRef::Group(group) => Some(group.as_str().to_owned()),
            _ => None,
        })
        .filter(|group| !group.trim().is_empty())
}

#[cfg(feature = "sqlite")]
fn direction_value_for_message(owner_did: &str, message: &crate::messages::Message) -> i64 {
    match message.direction {
        crate::messages::MessageDirection::Incoming => 0,
        crate::messages::MessageDirection::Outgoing => 1,
        crate::messages::MessageDirection::Unknown => {
            if message.sender.as_str() == owner_did {
                1
            } else if !message.sender.as_str().trim().is_empty() {
                0
            } else {
                -1
            }
        }
    }
}

#[cfg(feature = "sqlite")]
fn read_state_for_message(direction: i64, message: &crate::messages::Message) -> bool {
    match metadata_bool_attribute(&message.metadata, "is_read") {
        Some(value) => value,
        None => direction != 0,
    }
}

#[cfg(feature = "sqlite")]
fn metadata_bool_attribute(metadata: &crate::messages::MessageMetadata, key: &str) -> Option<bool> {
    metadata
        .attributes
        .iter()
        .find(|attribute| attribute.key == key)
        .and_then(|attribute| match attribute.value.trim() {
            "true" | "1" => Some(true),
            "false" | "0" => Some(false),
            _ => None,
        })
}

#[cfg(feature = "sqlite")]
fn body_projection(body: &crate::messages::MessageBodyView) -> (String, String) {
    match body {
        crate::messages::MessageBodyView::Text { text, kind } => {
            (content_type_for_kind(kind).to_owned(), text.to_owned())
        }
        crate::messages::MessageBodyView::Unsupported { content_type } => {
            (content_type.clone().unwrap_or_default(), String::new())
        }
    }
}

#[cfg(feature = "sqlite")]
fn is_e2ee_message(message: &crate::messages::Message) -> bool {
    message
        .metadata
        .content_type
        .as_deref()
        .map(|content_type| content_type.contains("cipher") || content_type.contains("e2ee"))
        .unwrap_or(false)
        || message
            .metadata
            .attributes
            .iter()
            .any(|attribute| attribute.key == "security" && attribute.value.contains("e2ee"))
}

#[cfg(feature = "sqlite")]
fn read_metadata_json(metadata: &crate::messages::MessageMetadata) -> String {
    let mut object = Map::new();
    insert_string(
        &mut object,
        "operation_id",
        metadata.operation_id.as_deref(),
    );
    insert_string(
        &mut object,
        "delivery_state",
        metadata.delivery_state.as_deref(),
    );
    insert_string(
        &mut object,
        "content_type",
        metadata.content_type.as_deref(),
    );
    if let Some(server_sequence) = metadata.server_sequence {
        object.insert(
            "server_sequence".to_owned(),
            Value::Number(server_sequence.into()),
        );
    }
    if let Some(send_state) = metadata.send_state.as_ref() {
        object.insert(
            "send_state".to_owned(),
            serde_json::to_value(send_state).unwrap_or(Value::Null),
        );
    }
    if let Some(retry_plan) = metadata.retry_plan.as_ref() {
        object.insert(
            "retry_plan".to_owned(),
            serde_json::to_value(retry_plan).unwrap_or(Value::Null),
        );
    }
    for attribute in &metadata.attributes {
        match attribute.key.as_str() {
            "raw_message_id" | "group_event_seq" | "is_read" | "senderName" | "sender_name"
                if !attribute.value.trim().is_empty() =>
            {
                object.insert(
                    attribute.key.clone(),
                    Value::String(attribute.value.clone()),
                );
            }
            _ => {}
        }
    }
    Value::Object(object).to_string()
}

fn content_type_for_kind(kind: &crate::messages::MessageKind) -> &'static str {
    match kind {
        crate::messages::MessageKind::Markdown => "text/markdown",
        crate::messages::MessageKind::Text => "text/plain",
    }
}

fn normalize_handle_value(value: &str) -> String {
    let value = value.trim().to_ascii_lowercase();
    if value.is_empty() {
        return String::new();
    }
    let value = value.trim_start_matches("wba://");
    match value.find('.') {
        Some(index) if index > 0 => value[..index].to_owned(),
        _ => value.to_owned(),
    }
}

fn merge_peer_dids(current: &str, historical: &[String]) -> Vec<String> {
    let mut seen = Vec::with_capacity(historical.len() + 1);
    let mut result = Vec::with_capacity(historical.len() + 1);
    let current = current.trim();
    if !current.is_empty() {
        seen.push(current.to_owned());
        result.push(current.to_owned());
    }
    for did in historical {
        let did = did.trim();
        if did.is_empty() || seen.iter().any(|known| known == did) {
            continue;
        }
        seen.push(did.to_owned());
        result.push(did.to_owned());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_thread_id_is_stable_for_peer_order() {
        assert_eq!(
            direct_thread_id("did:example:b", "did:example:a"),
            "dm:did:example:a:did:example:b"
        );
    }

    #[test]
    fn secure_metadata_keeps_only_redacted_delivery_fields() {
        let metadata = crate::messages::MessageMetadata {
            operation_id: Some("op-1".to_owned()),
            delivery_state: Some("accepted".to_owned()),
            send_state: Some(crate::messages::MessageSendState {
                state: crate::messages::MessageSendStateKind::Accepted,
                operation_id: Some("op-1".to_owned()),
                message_id: Some(crate::ids::MessageId::parse("msg-1").unwrap()),
                reason: None,
                updated_at: Some("2026-05-24T00:00:00Z".to_owned()),
            }),
            server_sequence: Some(7),
            content_type: Some("text/plain".to_owned()),
            attributes: vec![
                crate::messages::MessageMetadataAttribute {
                    key: "group_event_seq".to_owned(),
                    value: "7".to_owned(),
                },
                crate::messages::MessageMetadataAttribute {
                    key: "private_message_b64u".to_owned(),
                    value: "cipher".to_owned(),
                },
            ],
            ..Default::default()
        };

        let encoded = secure_metadata_json("group-e2ee", &metadata);
        let value: Value = serde_json::from_str(&encoded).unwrap();
        assert_eq!(value["security"], "group-e2ee");
        assert_eq!(value["contains_sensitive"], false);
        assert_eq!(value["group_event_seq"], "7");
        assert!(value.get("private_message_b64u").is_none());
        assert!(!encoded.contains("cipher"));
    }
}
