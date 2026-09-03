use serde_json::Value;

#[cfg(feature = "sqlite")]
use rusqlite::OptionalExtension;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AttachmentManifestCacheRecord {
    pub(crate) owner_identity_id: String,
    pub(crate) owner_did: String,
    pub(crate) thread_kind: String,
    pub(crate) thread_id: String,
    pub(crate) message_id: String,
    pub(crate) wire_message_id: String,
    pub(crate) sender_did: String,
    pub(crate) message_security_profile: String,
    pub(crate) content: String,
    pub(crate) stored_at: String,
}

pub(crate) fn transport_direct_cache_record(
    message: &super::messages::MessageRecord,
) -> Option<AttachmentManifestCacheRecord> {
    if message.is_e2ee
        || message.content_type != crate::attachments::manifest::attachment_manifest_content_type()
        || message.wire_thread_kind != "direct"
        || message.owner_identity_id.trim().is_empty()
        || message.wire_thread_ref.trim().is_empty()
    {
        return None;
    }
    let content: Value = serde_json::from_str(&message.content).ok()?;
    if content
        .get("attachments")
        .and_then(Value::as_array)
        .is_none_or(Vec::is_empty)
    {
        return None;
    }
    let wire_message_id = serde_json::from_str::<Value>(&message.metadata)
        .ok()
        .and_then(|metadata| {
            metadata
                .get("raw_message_id")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .unwrap_or_default();
    Some(AttachmentManifestCacheRecord {
        owner_identity_id: message.owner_identity_id.clone(),
        owner_did: message.owner_did.clone(),
        thread_kind: "direct".to_owned(),
        thread_id: message.wire_thread_ref.clone(),
        message_id: message.msg_id.clone(),
        wire_message_id,
        sender_did: message.sender_did.clone(),
        message_security_profile: "transport-protected".to_owned(),
        content: message.content.clone(),
        stored_at: message.stored_at.clone(),
    })
}

#[cfg(feature = "sqlite")]
pub(crate) fn upsert_attachment_manifest_cache(
    connection: &rusqlite::Connection,
    record: &AttachmentManifestCacheRecord,
) -> crate::ImResult<()> {
    crate::internal::local_state::schema::ensure_schema(connection)?;
    let owner_identity_id = required("owner_identity_id", &record.owner_identity_id)?;
    let message_id = required("message_id", &record.message_id)?;
    let thread_kind = required("thread_kind", &record.thread_kind)?;
    let thread_id = required("thread_id", &record.thread_id)?;
    let content = required("content", &record.content)?;
    let stored_at = default_string(&record.stored_at, &now_utc_like());
    connection
        .execute(
            r#"
INSERT INTO attachment_manifest_cache
    (owner_identity_id, owner_did, thread_kind, thread_id, message_id, wire_message_id,
     sender_did, message_security_profile, content, stored_at)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
ON CONFLICT(owner_identity_id, thread_kind, thread_id, message_id) DO UPDATE SET
    owner_did = excluded.owner_did,
    wire_message_id = excluded.wire_message_id,
    sender_did = excluded.sender_did,
    message_security_profile = excluded.message_security_profile,
    content = excluded.content,
    stored_at = excluded.stored_at"#,
            rusqlite::params![
                owner_identity_id,
                record.owner_did.trim(),
                thread_kind,
                thread_id,
                message_id,
                record.wire_message_id.trim(),
                record.sender_did.trim(),
                default_string(&record.message_security_profile, "transport-protected"),
                content,
                stored_at,
            ],
        )
        .map_err(super::local_state_unavailable)?;
    Ok(())
}

#[cfg(feature = "sqlite")]
pub(crate) fn get_attachment_manifest_cache_message(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    thread_kind: &str,
    thread_id: &str,
    message_id: &str,
) -> crate::ImResult<Option<Value>> {
    crate::internal::local_state::schema::ensure_schema(connection)?;
    let owner_identity_id = required("owner_identity_id", owner_identity_id)?;
    let thread_kind = required("thread_kind", thread_kind)?;
    let thread_id = required("thread_id", thread_id)?;
    let message_id = required("message_id", message_id)?;
    let row = connection
        .query_row(
            r#"
SELECT message_id, wire_message_id, owner_did, sender_did, message_security_profile, content
FROM attachment_manifest_cache
WHERE owner_identity_id = ?1
  AND thread_kind = ?2
  AND thread_id = ?3
  AND message_id = ?4"#,
            rusqlite::params![owner_identity_id, thread_kind, thread_id, message_id],
            |row| {
                Ok((
                    row.get::<_, String>("message_id")?,
                    row.get::<_, Option<String>>("wire_message_id")?
                        .unwrap_or_default(),
                    row.get::<_, String>("owner_did")?,
                    row.get::<_, Option<String>>("sender_did")?
                        .unwrap_or_default(),
                    row.get::<_, Option<String>>("message_security_profile")?
                        .unwrap_or_default(),
                    row.get::<_, String>("content")?,
                ))
            },
        )
        .optional()
        .map_err(super::local_state_unavailable)?;
    let Some((message_id, wire_message_id, owner_did, sender_did, security_profile, content)) = row
    else {
        return Ok(None);
    };
    let content: Value =
        serde_json::from_str(&content).map_err(|err| crate::ImError::Serialization {
            detail: err.to_string(),
        })?;
    let receiver_did = if thread_kind == "direct" && sender_did == owner_did {
        thread_id.to_owned()
    } else {
        owner_did
    };
    let mut message = serde_json::json!({
        "id": message_id,
        "message_id": message_id,
        "sender_did": sender_did,
        "receiver_did": receiver_did,
        "message_security_profile": default_string(&security_profile, "transport-protected"),
        "security": default_string(&security_profile, "transport-protected"),
        "content_type": crate::attachments::manifest::attachment_manifest_content_type(),
        "type": "attachment_manifest",
        "secure": security_profile.trim().ends_with("e2ee"),
        "decrypted": true,
        "decryption_state": "decrypted",
        "content": content
    });
    if !wire_message_id.trim().is_empty() {
        message["raw_message_id"] = Value::String(wire_message_id);
    }
    Ok(Some(message))
}

#[cfg(feature = "sqlite")]
fn required(field: &str, value: &str) -> crate::ImResult<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some(field.to_owned()),
            format!("{field} must not be empty"),
        ));
    }
    Ok(value.to_owned())
}

#[cfg(feature = "sqlite")]
fn default_string(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        fallback.to_owned()
    } else {
        value.to_owned()
    }
}

#[cfg(feature = "sqlite")]
fn now_utc_like() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
}

#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use super::*;
    use rusqlite::OptionalExtension;

    #[test]
    fn attachment_manifest_cache_stores_internal_full_manifest() {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        let manifest = serde_json::json!({
            "attachments": [{
                "attachment_id": "att-group-secure",
                "size": "27",
                "digest": {
                    "alg": "sha-256",
                    "value_b64u": "digest"
                },
                "access_info": {
                    "object_uri": "https://objects.example/secure"
                },
                "encryption_info": {
                    "mode": "object-e2ee",
                    "object_cipher": "chacha20-poly1305",
                    "object_key_b64u": "OBJECT-KEY-SECRET",
                    "nonce_b64u": "NONCE-SECRET",
                    "plaintext_size": "11"
                }
            }]
        });

        upsert_attachment_manifest_cache(
            &db,
            &AttachmentManifestCacheRecord {
                owner_identity_id: "bob-id".to_owned(),
                owner_did: "did:example:bob".to_owned(),
                thread_kind: "group".to_owned(),
                thread_id: "did:example:group".to_owned(),
                message_id: "logical-group-message-7".to_owned(),
                wire_message_id: "wire-group-message-7".to_owned(),
                sender_did: "did:example:alice".to_owned(),
                message_security_profile: "group-e2ee".to_owned(),
                content: serde_json::to_string(&manifest).unwrap(),
                stored_at: "2026-06-02T00:00:00Z".to_owned(),
            },
        )
        .unwrap();

        let message = get_attachment_manifest_cache_message(
            &db,
            "bob-id",
            "group",
            "did:example:group",
            "logical-group-message-7",
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            message["content"]["attachments"][0]["encryption_info"]["object_key_b64u"],
            "OBJECT-KEY-SECRET"
        );
        assert_eq!(message["message_security_profile"], "group-e2ee");
        assert_eq!(message["id"], "logical-group-message-7");
        assert_eq!(message["message_id"], "logical-group-message-7");
        assert_eq!(message["raw_message_id"], "wire-group-message-7");
        assert!(message.get("wire_message_id").is_none());

        let stored = db
            .query_row(
                "SELECT content FROM attachment_manifest_cache WHERE owner_identity_id = 'bob-id'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .unwrap()
            .unwrap();
        assert!(stored.contains("OBJECT-KEY-SECRET"));
    }

    #[test]
    fn attachment_manifest_cache_adds_wire_message_id_to_existing_table() {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        db.execute_batch(
            r#"
CREATE TABLE attachment_manifest_cache (
    owner_identity_id TEXT NOT NULL,
    owner_did TEXT NOT NULL DEFAULT '',
    thread_kind TEXT NOT NULL,
    thread_id TEXT NOT NULL,
    message_id TEXT NOT NULL,
    sender_did TEXT,
    message_security_profile TEXT NOT NULL DEFAULT 'transport-protected',
    content TEXT NOT NULL,
    stored_at TEXT NOT NULL,
    PRIMARY KEY (owner_identity_id, thread_kind, thread_id, message_id)
);
"#,
        )
        .unwrap();

        upsert_attachment_manifest_cache(
            &db,
            &AttachmentManifestCacheRecord {
                owner_identity_id: "owner-id".to_owned(),
                owner_did: "did:example:owner".to_owned(),
                thread_kind: "direct".to_owned(),
                thread_id: "did:example:peer".to_owned(),
                message_id: "logical-message".to_owned(),
                wire_message_id: "wire-message".to_owned(),
                sender_did: "did:example:peer".to_owned(),
                message_security_profile: "direct-e2ee".to_owned(),
                content: r#"{"attachments":[{"attachment_id":"att-1","access_info":{"object_uri":"https://objects.example/att-1"}}]}"#.to_owned(),
                stored_at: "2026-07-21T00:00:00Z".to_owned(),
            },
        )
        .unwrap();

        let wire_message_id = db
            .query_row(
                "SELECT wire_message_id FROM attachment_manifest_cache WHERE message_id = 'logical-message'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap();
        assert_eq!(wire_message_id, "wire-message");
    }

    #[test]
    fn attachment_manifest_cache_reconstructs_historical_direct_receiver() {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        upsert_attachment_manifest_cache(
            &db,
            &AttachmentManifestCacheRecord {
                owner_identity_id: "stable-owner".to_owned(),
                owner_did: "did:example:old-recipient".to_owned(),
                thread_kind: "direct".to_owned(),
                thread_id: "did:example:sender".to_owned(),
                message_id: "logical-message".to_owned(),
                wire_message_id: "wire-message".to_owned(),
                sender_did: "did:example:sender".to_owned(),
                message_security_profile: "transport-protected".to_owned(),
                content: r#"{"attachments":[{"attachment_id":"att-1","access_info":{"object_uri":"https://objects.example/att-1"}}]}"#.to_owned(),
                stored_at: "2026-09-04T00:00:00Z".to_owned(),
            },
        )
        .unwrap();

        let message = get_attachment_manifest_cache_message(
            &db,
            "stable-owner",
            "direct",
            "did:example:sender",
            "logical-message",
        )
        .unwrap()
        .unwrap();
        assert_eq!(message["sender_did"], "did:example:sender");
        assert_eq!(message["receiver_did"], "did:example:old-recipient");
        assert_eq!(message["message_security_profile"], "transport-protected");
    }

    #[test]
    fn ordinary_message_upsert_populates_transport_attachment_cache() {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        super::super::messages::upsert_messages(
            &db,
            &[super::super::messages::MessageRecord {
                msg_id: "logical-message".to_owned(),
                owner_identity_id: "stable-owner".to_owned(),
                owner_did: "did:example:old-recipient".to_owned(),
                wire_thread_kind: "direct".to_owned(),
                wire_thread_ref: "did:example:sender".to_owned(),
                sender_did: "did:example:sender".to_owned(),
                receiver_did: "did:example:old-recipient".to_owned(),
                content_type: crate::attachments::manifest::attachment_manifest_content_type()
                    .to_owned(),
                content: r#"{"attachments":[{"attachment_id":"att-1","access_info":{"object_uri":"https://objects.example/att-1"}}]}"#.to_owned(),
                stored_at: "2026-09-04T00:00:00Z".to_owned(),
                metadata: r#"{"raw_message_id":"wire-message"}"#.to_owned(),
                ..super::super::messages::MessageRecord::default()
            }],
        )
        .unwrap();

        let message = get_attachment_manifest_cache_message(
            &db,
            "stable-owner",
            "direct",
            "did:example:sender",
            "logical-message",
        )
        .unwrap()
        .unwrap();
        assert_eq!(message["raw_message_id"], "wire-message");
        assert_eq!(message["receiver_did"], "did:example:old-recipient");
    }
}
