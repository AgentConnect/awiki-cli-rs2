use serde_json::{Map, Value};

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

#[cfg(feature = "group-e2ee")]
pub(crate) fn group_thread_id(group_did: &str) -> String {
    format!("group:{}", group_did.trim())
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

fn content_type_for_kind(kind: &crate::messages::MessageKind) -> &'static str {
    match kind {
        crate::messages::MessageKind::Markdown => "text/markdown",
        crate::messages::MessageKind::Text => "text/plain",
    }
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
