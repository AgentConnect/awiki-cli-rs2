//! Reliable holding area for inbound events whose canonical identity is not yet known.

use std::collections::BTreeSet;

use rusqlite::{Connection, OptionalExtension};

const TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS inbound_resolution_backlog (
    owner_identity_id TEXT NOT NULL,
    owner_did         TEXT NOT NULL DEFAULT '',
    event_id          TEXT NOT NULL,
    event_seq         TEXT NOT NULL,
    event_type        TEXT NOT NULL,
    message_id        TEXT NOT NULL,
    peer_did          TEXT NOT NULL DEFAULT '',
    message_record_json TEXT NOT NULL,
    resolution_state  TEXT NOT NULL DEFAULT 'pending',
    error_code        TEXT NOT NULL,
    error_detail      TEXT NOT NULL DEFAULT '',
    attempt_count     INTEGER NOT NULL DEFAULT 0,
    first_seen_at     TEXT NOT NULL,
    last_attempt_at   TEXT NOT NULL,
    PRIMARY KEY (owner_identity_id, event_id, message_id)
);
CREATE INDEX IF NOT EXISTS idx_inbound_resolution_backlog_pending_peer
ON inbound_resolution_backlog(owner_identity_id, resolution_state, peer_did, event_seq);
"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BacklogSource<'a> {
    pub(crate) event_id: &'a str,
    pub(crate) event_seq: &'a str,
    pub(crate) event_type: &'a str,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct RemoteMessageIngestOutcome {
    pub(crate) stored_messages: usize,
    pub(crate) backlogged_messages: usize,
    pub(crate) hydration_probes_resolved: usize,
}

pub(crate) fn create_schema(connection: &Connection) -> crate::ImResult<()> {
    connection
        .execute_batch(TABLE_SQL)
        .map_err(super::local_state_unavailable)
}

pub(crate) fn canonicalize_inbound_message(
    connection: &Connection,
    mut record: super::messages::MessageRecord,
) -> crate::ImResult<super::messages::MessageRecord> {
    if is_group(&record) {
        let group_did = record.group_did.trim();
        crate::ids::Did::parse(group_did).map_err(|_| {
            crate::ImError::CanonicalGroupIdentityMissing {
                group: if group_did.is_empty() {
                    record.group_id.clone()
                } else {
                    group_did.to_owned()
                },
            }
        })?;
        let conversation_id = super::owner_scope::group_conversation_id(group_did);
        record.conversation_id = conversation_id.clone();
        record.thread_id = conversation_id;
        return Ok(record);
    }
    if is_mail(&record) {
        return Ok(record);
    }
    let peer_did = direct_peer_did(&record).ok_or_else(|| crate::ImError::IdentityUnresolved {
        detail: "inbound Direct message has no peer DID snapshot".to_owned(),
    })?;
    let resolved =
        super::peer_personas::resolve_by_did(connection, &record.owner_identity_id, &peer_did)?
            .ok_or_else(|| crate::ImError::IdentityUnresolved {
                detail: "inbound Direct peer DID is not bound to a verified Persona".to_owned(),
            })?;
    if record.sender_did.trim() != record.owner_did.trim() {
        set_metadata_string(
            &mut record.metadata,
            "sender_peer_persona_id",
            &resolved.peer_persona_id,
        );
    }
    record.conversation_id = resolved.conversation_id.clone();
    record.thread_id = resolved.conversation_id;
    Ok(record)
}

pub(crate) fn ingest_remote_messages(
    connection: &Connection,
    records: &[super::messages::MessageRecord],
    source_event_type: &str,
) -> crate::ImResult<RemoteMessageIngestOutcome> {
    let source_event_type = source_event_type.trim();
    let mut resolved = Vec::new();
    let mut backlogged_messages = 0usize;
    for record in records {
        match canonicalize_inbound_message(connection, record.clone()) {
            Ok(record) => resolved.push(record),
            Err(error) if is_resolution_error(&error) => {
                let event_id = format!("{}:{}", source_event_type, record.msg_id.trim());
                let event_seq = record.server_seq.unwrap_or_default().to_string();
                store(
                    connection,
                    BacklogSource {
                        event_id: &event_id,
                        event_seq: &event_seq,
                        event_type: source_event_type,
                    },
                    record,
                    &error,
                )?;
                backlogged_messages = backlogged_messages.saturating_add(1);
            }
            Err(error) => return Err(error),
        }
    }
    super::messages::upsert_messages(connection, &resolved)?;
    Ok(RemoteMessageIngestOutcome {
        stored_messages: resolved.len(),
        backlogged_messages,
        hydration_probes_resolved: 0,
    })
}

pub(crate) fn ingest_catch_up_remote_messages(
    connection: &Connection,
    records: &[super::messages::MessageRecord],
    source_event_type: &str,
    proof: &super::messages::CatchUpHydrationProof,
) -> crate::ImResult<RemoteMessageIngestOutcome> {
    let mut outcome = ingest_remote_messages(connection, records, source_event_type)?;
    // A backlogged message was present in the scanned range but was not
    // committed to the canonical timeline. Do not let range coverage promote
    // an older empty probe for that message into a visible hydrated row.
    if outcome.backlogged_messages == 0 {
        outcome.hydration_probes_resolved =
            super::messages::resolve_legacy_hydration_probes_for_thread_ref(
                connection,
                &proof.owner_identity_id,
                &proof.owner_did,
                &proof.thread,
                proof.after_server_seq,
                proof.through_server_seq,
                proof.exhausted,
            )?;
    }
    Ok(outcome)
}

pub(crate) fn is_resolution_error(error: &crate::ImError) -> bool {
    matches!(
        error,
        crate::ImError::IdentityUnresolved { .. }
            | crate::ImError::IdentityBindingConflict { .. }
            | crate::ImError::ConversationAliasConflict { .. }
            | crate::ImError::CanonicalGroupIdentityMissing { .. }
    )
}

fn set_metadata_string(metadata: &mut String, key: &str, value: &str) {
    let mut object = if metadata.trim().is_empty() {
        serde_json::Map::new()
    } else {
        let Ok(existing) = serde_json::from_str::<serde_json::Value>(metadata) else {
            return;
        };
        let Some(object) = existing.as_object() else {
            return;
        };
        object.clone()
    };
    object.insert(key.to_owned(), serde_json::Value::String(value.to_owned()));
    *metadata = serde_json::Value::Object(object).to_string();
}

pub(crate) fn store(
    connection: &Connection,
    source: BacklogSource<'_>,
    record: &super::messages::MessageRecord,
    error: &crate::ImError,
) -> crate::ImResult<()> {
    create_schema(connection)?;
    let state = if matches!(
        error,
        crate::ImError::IdentityBindingConflict { .. }
            | crate::ImError::ConversationAliasConflict { .. }
    ) {
        "blocked_conflict"
    } else {
        "pending"
    };
    let error_code = match error {
        crate::ImError::IdentityBindingConflict { .. } => "identity_binding_conflict",
        crate::ImError::ConversationAliasConflict { .. } => "conversation_alias_conflict",
        crate::ImError::CanonicalGroupIdentityMissing { .. } => "canonical_group_identity_missing",
        _ => "identity_unresolved",
    };
    let peer_did = direct_peer_did(record).unwrap_or_default();
    let payload =
        serde_json::to_string(record).map_err(|err| crate::ImError::LocalStateUnavailable {
            detail: format!("failed to encode unresolved inbound message: {err}"),
        })?;
    let now = now();
    connection
        .execute(
            r#"INSERT INTO inbound_resolution_backlog
    (owner_identity_id, owner_did, event_id, event_seq, event_type, message_id,
     peer_did, message_record_json, resolution_state, error_code, error_detail,
     attempt_count, first_seen_at, last_attempt_at)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 1, ?12, ?12)
ON CONFLICT(owner_identity_id, event_id, message_id) DO UPDATE SET
    resolution_state = excluded.resolution_state,
    error_code = excluded.error_code,
    error_detail = excluded.error_detail,
    attempt_count = inbound_resolution_backlog.attempt_count + 1,
    last_attempt_at = excluded.last_attempt_at"#,
            rusqlite::params![
                record.owner_identity_id.trim(),
                record.owner_did.trim(),
                source.event_id.trim(),
                source.event_seq.trim(),
                source.event_type.trim(),
                record.msg_id.trim(),
                peer_did,
                payload,
                state,
                error_code,
                redacted_detail(error),
                now,
            ],
        )
        .map_err(super::local_state_unavailable)?;
    Ok(())
}

pub(crate) fn replay_for_persona(
    connection: &Connection,
    owner_identity_id: &str,
    peer_persona_id: &str,
) -> crate::ImResult<usize> {
    create_schema(connection)?;
    let dids =
        super::peer_identifiers::dids_for_persona(connection, owner_identity_id, peer_persona_id)?;
    if dids.is_empty() {
        return Ok(0);
    }
    let mut rows = Vec::new();
    for did in dids {
        let mut statement = connection
            .prepare(
                r#"SELECT event_id, event_type, message_id, message_record_json
FROM inbound_resolution_backlog
WHERE owner_identity_id = ?1 AND resolution_state = 'pending' AND peer_did = ?2
ORDER BY LENGTH(event_seq), event_seq, event_id, message_id"#,
            )
            .map_err(super::local_state_unavailable)?;
        let found = statement
            .query_map((owner_identity_id.trim(), did.trim()), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .map_err(super::local_state_unavailable)?;
        for row in found {
            rows.push(row.map_err(super::local_state_unavailable)?);
        }
    }
    let mut replayed = 0usize;
    for (event_id, event_type, message_id, payload) in rows {
        let record = decode_message_record(&event_type, &payload)?;
        let record = canonicalize_inbound_message(connection, record)?;
        super::messages::upsert_message(connection, &record)?;
        connection
            .execute(
                r#"DELETE FROM inbound_resolution_backlog
WHERE owner_identity_id = ?1 AND event_id = ?2 AND message_id = ?3"#,
                (owner_identity_id.trim(), event_id, message_id),
            )
            .map_err(super::local_state_unavailable)?;
        replayed = replayed.saturating_add(1);
    }
    Ok(replayed)
}

pub(crate) fn pending_count(
    connection: &Connection,
    owner_identity_id: &str,
) -> crate::ImResult<u64> {
    create_schema(connection)?;
    let count = connection
        .query_row(
            r#"SELECT COUNT(*) FROM inbound_resolution_backlog
WHERE owner_identity_id = ?1 AND resolution_state = 'pending'"#,
            [owner_identity_id.trim()],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(super::local_state_unavailable)?
        .unwrap_or_default();
    Ok(u64::try_from(count).unwrap_or_default())
}

pub(crate) fn list_decrypted_secure_messages_for_owner_identity(
    connection: &Connection,
    owner_identity_id: &str,
    message_ids: &[String],
) -> crate::ImResult<Vec<super::messages::MessageRecord>> {
    create_schema(connection)?;
    let owner_identity_id = owner_identity_id.trim();
    if owner_identity_id.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("owner_identity_id".to_owned()),
            "owner identity id is required",
        ));
    }
    let message_ids = message_ids
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>();
    if message_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = vec!["?"; message_ids.len()].join(",");
    let query = format!(
        r#"SELECT event_type, message_record_json
FROM inbound_resolution_backlog
WHERE owner_identity_id = ?
  AND message_id IN ({placeholders})"#
    );
    let mut params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(message_ids.len() + 1);
    params.push(&owner_identity_id);
    for message_id in &message_ids {
        params.push(message_id);
    }
    let mut statement = connection
        .prepare(&query)
        .map_err(super::local_state_unavailable)?;
    let rows = statement
        .query_map(params.as_slice(), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(super::local_state_unavailable)?;
    let mut records = Vec::new();
    for row in rows {
        let (event_type, payload) = row.map_err(super::local_state_unavailable)?;
        let record = decode_message_record(&event_type, &payload)?;
        let decrypted = serde_json::from_str::<serde_json::Value>(&record.metadata)
            .ok()
            .and_then(|metadata| {
                metadata
                    .get("decryption_state")
                    .and_then(serde_json::Value::as_str)
                    .map(|state| state == "decrypted")
            })
            .unwrap_or(false);
        if record.owner_identity_id == owner_identity_id
            && message_ids.contains(record.msg_id.trim())
            && record.is_e2ee
            && decrypted
        {
            records.push(record);
        }
    }
    Ok(records)
}

fn decode_message_record(
    event_type: &str,
    payload: &str,
) -> crate::ImResult<super::messages::MessageRecord> {
    let value = serde_json::from_str::<serde_json::Value>(payload).map_err(|err| {
        crate::ImError::LocalStateUnavailable {
            detail: format!("failed to decode unresolved inbound message: {err}"),
        }
    })?;
    let has_explicit_hydration_state = value
        .as_object()
        .is_some_and(|object| object.contains_key("hydration_state"));
    let mut record =
        serde_json::from_value::<super::messages::MessageRecord>(value).map_err(|err| {
            crate::ImError::LocalStateUnavailable {
                detail: format!("failed to decode unresolved inbound message: {err}"),
            }
        })?;
    if !has_explicit_hydration_state {
        record.hydration_state = legacy_hydration_state(event_type, &record);
    }
    Ok(record)
}

fn legacy_hydration_state(
    event_type: &str,
    record: &super::messages::MessageRecord,
) -> super::messages::MessageHydrationState {
    use super::messages::MessageHydrationState;

    if !record.content.trim().is_empty() {
        return MessageHydrationState::Hydrated;
    }
    match event_type.trim() {
        // Before schema 29, sync.delta records did not retain whether `content`
        // was absent or explicitly empty. A sequence makes metadata provenance
        // reliable; catch-up will either hydrate it or confirm a legitimate
        // empty body. Without a sequence, keep the row as a conservative probe
        // and force thread catch-up from the beginning.
        "message.created" | "conversation.updated" if record.server_seq.is_some() => {
            MessageHydrationState::Discovered
        }
        "message.created" | "conversation.updated" => MessageHydrationState::LegacyProbe,
        // These sources carry complete message projections, where an empty body
        // can be valid and must not be reclassified as metadata-only.
        "remote_history" | "realtime_message" | "thread_catch_up" => {
            MessageHydrationState::Hydrated
        }
        _ => MessageHydrationState::LegacyProbe,
    }
}

fn direct_peer_did(record: &super::messages::MessageRecord) -> Option<String> {
    if is_group(record) {
        return None;
    }
    let peer = if record.sender_did.trim() != record.owner_did.trim() {
        record.sender_did.trim()
    } else {
        record.receiver_did.trim()
    };
    (peer.starts_with("did:")).then(|| peer.to_owned())
}

fn is_group(record: &super::messages::MessageRecord) -> bool {
    !record.group_id.trim().is_empty()
        || !record.group_did.trim().is_empty()
        || record.wire_thread_kind.trim() == "group"
}

fn is_mail(record: &super::messages::MessageRecord) -> bool {
    record.wire_thread_kind.trim() == "mail" || record.thread_id.trim().starts_with("mail:")
}

fn redacted_detail(error: &crate::ImError) -> String {
    match error {
        crate::ImError::IdentityBindingConflict { .. } => {
            "verified identity binding conflict".to_owned()
        }
        crate::ImError::ConversationAliasConflict { .. } => {
            "canonical conversation alias conflict".to_owned()
        }
        crate::ImError::CanonicalGroupIdentityMissing { .. } => {
            "canonical Group DID is missing".to_owned()
        }
        _ => "canonical identity is not resolved".to_owned(),
    }
}

fn now() -> String {
    time::OffsetDateTime::now_utc().unix_timestamp().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn legacy_payload(mut record: super::super::messages::MessageRecord) -> String {
        record.hydration_state = super::super::messages::MessageHydrationState::Hydrated;
        let mut value = serde_json::to_value(record).unwrap();
        value.as_object_mut().unwrap().remove("hydration_state");
        value.to_string()
    }

    #[test]
    fn legacy_backlog_hydration_uses_source_contract_and_fails_closed() {
        let base = super::super::messages::MessageRecord {
            msg_id: "legacy-message".to_owned(),
            owner_identity_id: "owner-a".to_owned(),
            owner_did: "did:example:owner".to_owned(),
            conversation_id: "dm:did:example:peer".to_owned(),
            thread_id: "dm:did:example:peer".to_owned(),
            direction: 0,
            sender_did: "did:example:peer".to_owned(),
            receiver_did: "did:example:owner".to_owned(),
            content_type: "text/plain".to_owned(),
            server_seq: Some(8),
            stored_at: "2026-07-24T00:00:00Z".to_owned(),
            ..super::super::messages::MessageRecord::default()
        };
        let sync_metadata =
            decode_message_record("message.created", &legacy_payload(base.clone())).unwrap();
        assert_eq!(
            sync_metadata.hydration_state,
            super::super::messages::MessageHydrationState::Discovered
        );

        let mut unsequenced = base.clone();
        unsequenced.server_seq = None;
        assert_eq!(
            decode_message_record("conversation.updated", &legacy_payload(unsequenced))
                .unwrap()
                .hydration_state,
            super::super::messages::MessageHydrationState::LegacyProbe
        );
        assert_eq!(
            decode_message_record("remote_history", &legacy_payload(base.clone()))
                .unwrap()
                .hydration_state,
            super::super::messages::MessageHydrationState::Hydrated
        );
        assert_eq!(
            decode_message_record("unknown_source", &legacy_payload(base.clone()))
                .unwrap()
                .hydration_state,
            super::super::messages::MessageHydrationState::LegacyProbe
        );

        let mut complete = base.clone();
        complete.content = "complete body".to_owned();
        assert_eq!(
            decode_message_record("message.created", &legacy_payload(complete))
                .unwrap()
                .hydration_state,
            super::super::messages::MessageHydrationState::Hydrated
        );

        let explicit_discovered = super::super::messages::MessageRecord {
            hydration_state: super::super::messages::MessageHydrationState::Discovered,
            ..base
        };
        assert_eq!(
            decode_message_record(
                "remote_history",
                &serde_json::to_string(&explicit_discovered).unwrap(),
            )
            .unwrap()
            .hydration_state,
            super::super::messages::MessageHydrationState::Discovered
        );
    }

    #[test]
    fn unresolved_direct_is_durable_and_replays_after_verified_persona_projection() {
        let mut db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let record = super::super::messages::MessageRecord {
            msg_id: "msg-unresolved-1".to_owned(),
            owner_identity_id: "owner-a".to_owned(),
            owner_did: "did:example:owner".to_owned(),
            conversation_id: "dm:did:example:peer".to_owned(),
            thread_id: "dm:did:example:peer".to_owned(),
            direction: 0,
            sender_did: "did:example:peer".to_owned(),
            receiver_did: "did:example:owner".to_owned(),
            content_type: "text/plain".to_owned(),
            content: "hello".to_owned(),
            stored_at: "2026-07-14T00:00:00Z".to_owned(),
            credential_name: "owner-a".to_owned(),
            ..super::super::messages::MessageRecord::default()
        }
        .with_resolved_wire_thread("direct", "did:example:peer");
        let first =
            ingest_remote_messages(&db, std::slice::from_ref(&record), "remote_history").unwrap();
        let repeated =
            ingest_remote_messages(&db, std::slice::from_ref(&record), "remote_history").unwrap();
        assert_eq!(first.stored_messages, 0);
        assert_eq!(first.backlogged_messages, 1);
        assert_eq!(repeated, first);
        assert_eq!(pending_count(&db, "owner-a").unwrap(), 1);
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM messages", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            0
        );

        let lookup = crate::directory::HandleLookupResult {
            handle: crate::ids::Handle::parse("peer.awiki.info", "").unwrap(),
            did: crate::ids::Did::parse("did:example:peer").unwrap(),
            user_id: "user-peer".to_owned(),
            domain: Some("awiki.info".to_owned()),
            status: Some("active".to_owned()),
            binding_generation: Some("1".to_owned()),
            profile: None,
            warnings: Vec::new(),
        };
        let conversation_id = super::super::peer_personas::project_verified_handle(
            &mut db,
            "owner-a",
            "did:example:owner",
            &lookup,
        )
        .unwrap();

        assert_eq!(pending_count(&db, "owner-a").unwrap(), 0);
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM messages", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        let stored: (String, String, String, String) = db
            .query_row(
                r#"SELECT conversation_id, wire_thread_kind, wire_thread_ref, metadata
FROM messages WHERE owner_identity_id = 'owner-a' AND msg_id = 'msg-unresolved-1'"#,
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(stored.0, conversation_id);
        assert_ne!(stored.0, "dm:did:example:peer");
        assert_eq!(stored.1, "direct");
        assert_eq!(stored.2, "did:example:peer");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&stored.3).unwrap()["sender_peer_persona_id"],
            serde_json::Value::String(
                crate::internal::canonical_identity::PeerPersona::from_verified_handle(
                    "awiki.info",
                    "user-peer",
                    "peer.awiki.info",
                    Some("verified"),
                )
                .unwrap()
                .peer_persona_id
            )
        );
        assert!(
            crate::internal::local_state::canonical_invariants::check(&db, "owner-a")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn decrypted_secure_backlog_lookup_is_owner_scoped_and_filters_plaintext_messages() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let secure = super::super::messages::MessageRecord {
            msg_id: "msg-secure".to_owned(),
            owner_identity_id: "owner-a".to_owned(),
            owner_did: "did:example:owner-a".to_owned(),
            sender_did: "did:example:peer".to_owned(),
            receiver_did: "did:example:owner-a".to_owned(),
            content_type: "text/plain".to_owned(),
            content: "decrypted".to_owned(),
            is_e2ee: true,
            metadata: serde_json::json!({"decryption_state": "decrypted"}).to_string(),
            ..super::super::messages::MessageRecord::default()
        }
        .with_resolved_wire_thread("direct", "did:example:peer");
        store(
            &db,
            BacklogSource {
                event_id: "remote_history:msg-secure",
                event_seq: "1",
                event_type: "remote_history",
            },
            &secure,
            &crate::ImError::IdentityUnresolved {
                detail: "unresolved".to_owned(),
            },
        )
        .unwrap();
        let mut plaintext = secure.clone();
        plaintext.msg_id = "msg-plaintext".to_owned();
        plaintext.is_e2ee = false;
        store(
            &db,
            BacklogSource {
                event_id: "remote_history:msg-plaintext",
                event_seq: "2",
                event_type: "remote_history",
            },
            &plaintext,
            &crate::ImError::IdentityUnresolved {
                detail: "unresolved".to_owned(),
            },
        )
        .unwrap();

        let ids = vec!["msg-secure".to_owned(), "msg-plaintext".to_owned()];
        let found =
            list_decrypted_secure_messages_for_owner_identity(&db, "owner-a", &ids).unwrap();
        assert_eq!(found, vec![secure]);
        assert!(
            list_decrypted_secure_messages_for_owner_identity(&db, "owner-b", &ids)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn verified_direct_remote_history_is_stored_only_under_canonical_conversation() {
        let mut db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let lookup = crate::directory::HandleLookupResult {
            handle: crate::ids::Handle::parse("peer.awiki.info", "").unwrap(),
            did: crate::ids::Did::parse("did:example:peer").unwrap(),
            user_id: "user-peer".to_owned(),
            domain: Some("awiki.info".to_owned()),
            status: Some("active".to_owned()),
            binding_generation: Some("1".to_owned()),
            profile: None,
            warnings: Vec::new(),
        };
        let conversation_id = super::super::peer_personas::project_verified_handle(
            &mut db,
            "owner-a",
            "did:example:owner",
            &lookup,
        )
        .unwrap();
        let record = super::super::messages::MessageRecord {
            msg_id: "msg-resolved-1".to_owned(),
            owner_identity_id: "owner-a".to_owned(),
            owner_did: "did:example:owner".to_owned(),
            conversation_id: "dm:did:example:peer".to_owned(),
            thread_id: "dm:did:example:peer".to_owned(),
            direction: 0,
            sender_did: "did:example:peer".to_owned(),
            receiver_did: "did:example:owner".to_owned(),
            content_type: "application/json".to_owned(),
            content: r#"{"type":"system.control"}"#.to_owned(),
            stored_at: "2026-07-15T00:00:00Z".to_owned(),
            credential_name: "owner-a".to_owned(),
            ..super::super::messages::MessageRecord::default()
        }
        .with_resolved_wire_thread("direct", "did:example:peer");

        let outcome = ingest_remote_messages(&db, &[record], "remote_history").unwrap();
        assert_eq!(outcome.stored_messages, 1);
        assert_eq!(outcome.backlogged_messages, 0);
        assert_eq!(pending_count(&db, "owner-a").unwrap(), 0);
        let stored_conversation_id: String = db
            .query_row(
                "SELECT conversation_id FROM messages WHERE msg_id = 'msg-resolved-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored_conversation_id, conversation_id);
        assert!(
            crate::internal::local_state::canonical_invariants::check(&db, "owner-a")
                .unwrap()
                .is_empty()
        );
    }
}
