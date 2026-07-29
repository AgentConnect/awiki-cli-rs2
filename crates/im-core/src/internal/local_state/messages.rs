use std::collections::{BTreeMap, BTreeSet};

#[cfg(feature = "sqlite")]
use rusqlite::OptionalExtension;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MessageHydrationState {
    #[default]
    Hydrated,
    Discovered,
    LegacyProbe,
}

impl MessageHydrationState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Hydrated => "hydrated",
            Self::Discovered => "discovered",
            Self::LegacyProbe => "legacy_probe",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CatchUpHydrationProof {
    pub(crate) owner_identity_id: String,
    pub(crate) owner_did: String,
    pub(crate) thread: crate::messages::ThreadRef,
    pub(crate) after_server_seq: i64,
    pub(crate) through_server_seq: i64,
    pub(crate) exhausted: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct CatchUpCursor {
    pub(crate) default_after_server_seq: Option<i64>,
    pub(crate) hydration_gap_after_server_seq: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct MessageRecord {
    pub(crate) msg_id: String,
    pub(crate) owner_identity_id: String,
    pub(crate) owner_did: String,
    pub(crate) conversation_id: String,
    pub(crate) wire_thread_kind: String,
    pub(crate) wire_thread_ref: String,
    pub(crate) wire_identity_resolution_state: String,
    pub(crate) thread_id: String,
    pub(crate) direction: i64,
    pub(crate) sender_did: String,
    pub(crate) receiver_did: String,
    pub(crate) group_id: String,
    pub(crate) group_did: String,
    pub(crate) content_type: String,
    pub(crate) content: String,
    pub(crate) title: String,
    pub(crate) server_seq: Option<i64>,
    #[serde(default)]
    pub(crate) hydration_state: MessageHydrationState,
    pub(crate) sent_at: String,
    pub(crate) stored_at: String,
    pub(crate) is_e2ee: bool,
    pub(crate) is_read: bool,
    pub(crate) sender_name: String,
    pub(crate) metadata: String,
    pub(crate) mentions_current_user: bool,
    pub(crate) credential_name: String,
}

impl MessageRecord {
    pub(crate) fn with_resolved_wire_thread(
        mut self,
        kind: impl Into<String>,
        reference: impl Into<String>,
    ) -> Self {
        self.wire_thread_kind = kind.into();
        self.wire_thread_ref = reference.into();
        self.wire_identity_resolution_state = "resolved".to_owned();
        self
    }

    pub(crate) fn with_wire_identity_from_message(
        self,
        owner_did: &str,
        message: &crate::messages::Message,
    ) -> Self {
        let (kind, reference) = wire_thread_parts_from_message(owner_did, message);
        self.with_resolved_wire_thread(kind, reference)
    }
}

fn wire_thread_parts_from_message(
    owner_did: &str,
    message: &crate::messages::Message,
) -> (&'static str, String) {
    match &message.thread {
        crate::messages::ThreadRef::Direct(peer) => ("direct", peer.as_str().to_owned()),
        crate::messages::ThreadRef::Group(group) => ("group", group.as_str().to_owned()),
        crate::messages::ThreadRef::Thread(thread) => {
            if let Some(group) = message.group.as_ref() {
                return ("group", group.as_str().to_owned());
            }
            if thread.as_str().starts_with("dm:peer-scope:v1:") {
                if let Some(peer_did) = direct_wire_peer_did(owner_did, message) {
                    return ("direct", peer_did);
                }
            }
            ("thread", thread.as_str().to_owned())
        }
    }
}

fn direct_wire_peer_did(owner_did: &str, message: &crate::messages::Message) -> Option<String> {
    let owner_did = owner_did.trim();
    [
        Some(message.sender.as_str()),
        message.receiver.as_ref().map(crate::ids::PeerRef::as_str),
    ]
    .into_iter()
    .flatten()
    .map(str::trim)
    .find(|candidate| candidate.starts_with("did:") && *candidate != owner_did)
    .map(str::to_owned)
}

#[cfg(feature = "sqlite")]
#[derive(Debug, Clone, PartialEq, Eq)]
struct WireThreadIdentity {
    kind: String,
    reference: String,
    resolution_state: String,
}

#[cfg(feature = "sqlite")]
fn wire_identity_for_record(record: &MessageRecord) -> crate::ImResult<WireThreadIdentity> {
    let explicit_kind = record.wire_thread_kind.trim();
    let explicit_ref = record.wire_thread_ref.trim();
    if explicit_kind.is_empty() != explicit_ref.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("wire_thread_ref".to_owned()),
            "wire thread kind and reference must be provided together",
        ));
    }
    if !explicit_kind.is_empty() {
        return validated_wire_identity(
            explicit_kind,
            explicit_ref,
            record.wire_identity_resolution_state.as_str(),
        );
    }

    let group_ref =
        non_empty(record.group_did.as_str()).or_else(|| non_empty(record.group_id.as_str()));
    if let Some(group_ref) = group_ref {
        return validated_wire_identity("group", group_ref, "resolved");
    }
    let owner_did = record.owner_did.trim();
    let direct_ref = [record.sender_did.as_str(), record.receiver_did.as_str()]
        .into_iter()
        .map(str::trim)
        .find(|did| did.starts_with("did:") && *did != owner_did);
    if let Some(direct_ref) = direct_ref {
        return validated_wire_identity("direct", direct_ref, "resolved");
    }

    let compatibility_thread = record.thread_id.trim();
    if compatibility_thread.is_empty() {
        return Ok(WireThreadIdentity {
            kind: String::new(),
            reference: String::new(),
            resolution_state: "legacy_unresolved".to_owned(),
        });
    }
    if let Some(group_ref) = compatibility_thread.strip_prefix("group:") {
        return validated_wire_identity("group", group_ref, "legacy_unresolved");
    }
    if let Some(mail_ref) = compatibility_thread.strip_prefix("mail:") {
        return validated_wire_identity("mail", mail_ref, "resolved");
    }
    if let Some(direct_ref) = compatibility_thread.strip_prefix("dm:") {
        let resolution_state = if direct_ref.starts_with("peer-scope:") {
            "legacy_unresolved"
        } else {
            "resolved"
        };
        return validated_wire_identity("direct", direct_ref, resolution_state);
    }
    validated_wire_identity("thread", compatibility_thread, "resolved")
}

#[cfg(feature = "sqlite")]
fn validated_wire_identity(
    kind: &str,
    reference: &str,
    resolution_state: &str,
) -> crate::ImResult<WireThreadIdentity> {
    let kind = kind.trim().to_ascii_lowercase();
    if !matches!(kind.as_str(), "direct" | "group" | "thread" | "mail") {
        return Err(crate::ImError::invalid_input(
            Some("wire_thread_kind".to_owned()),
            "wire thread kind must be direct, group, thread, or mail",
        ));
    }
    let reference = reference.trim();
    if reference.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("wire_thread_ref".to_owned()),
            "wire thread reference must not be empty",
        ));
    }
    let resolution_state = match resolution_state.trim() {
        "" | "resolved" => "resolved",
        "legacy_unresolved" => "legacy_unresolved",
        _ => {
            return Err(crate::ImError::invalid_input(
                Some("wire_identity_resolution_state".to_owned()),
                "wire identity resolution state must be resolved or legacy_unresolved",
            ));
        }
    };
    Ok(WireThreadIdentity {
        kind,
        reference: reference.to_owned(),
        resolution_state: resolution_state.to_owned(),
    })
}

#[cfg(feature = "sqlite")]
fn assert_existing_wire_identity_compatible(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    message_id: &str,
    record: &MessageRecord,
    wire_identity: &WireThreadIdentity,
) -> crate::ImResult<()> {
    let existing = connection
        .query_row(
            r#"SELECT wire_thread_kind, wire_thread_ref, sender_did, receiver_did,
                      group_id, group_did, server_seq
FROM messages
WHERE owner_identity_id = ?1 AND msg_id = ?2"#,
            (owner_identity_id, message_id),
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                    row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                    row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                    row.get::<_, Option<i64>>(6)?,
                ))
            },
        )
        .optional()
        .map_err(super::local_state_unavailable)?;
    let Some((kind, reference, sender, receiver, group_id, group_did, server_seq)) = existing
    else {
        return Ok(());
    };
    let text_conflict = [
        (kind.as_str(), wire_identity.kind.as_str()),
        (reference.as_str(), wire_identity.reference.as_str()),
        (sender.as_str(), record.sender_did.as_str()),
        (receiver.as_str(), record.receiver_did.as_str()),
        (group_id.as_str(), record.group_id.as_str()),
        (group_did.as_str(), record.group_did.as_str()),
    ]
    .into_iter()
    .any(|(existing, incoming)| {
        !existing.trim().is_empty()
            && !incoming.trim().is_empty()
            && existing.trim() != incoming.trim()
    });
    let sequence_conflict = server_seq
        .zip(record.server_seq)
        .is_some_and(|(existing, incoming)| existing != incoming);
    if text_conflict || sequence_conflict {
        return Err(crate::ImError::MessageWireIdentityConflict {
            message_id: message_id.to_owned(),
        });
    }
    Ok(())
}

#[cfg(feature = "sqlite")]
#[derive(Debug)]
struct LegacyCanonicalDirectWireCandidate {
    owner_identity_id: String,
    message_id: String,
    owner_did: String,
    conversation_id: String,
    sender_did: String,
    receiver_did: String,
}

#[cfg(feature = "sqlite")]
pub(crate) fn repair_legacy_canonical_direct_wire_identities(
    connection: &rusqlite::Connection,
) -> crate::ImResult<usize> {
    let candidates = {
        let mut statement = connection
            .prepare(
                r#"SELECT owner_identity_id, msg_id, owner_did, conversation_id,
                          sender_did, receiver_did
FROM messages
WHERE conversation_id LIKE 'dm:peer-scope:v1:%'
  AND wire_thread_kind = 'thread'
  AND wire_thread_ref = conversation_id
  AND wire_identity_resolution_state = 'resolved'
  AND TRIM(COALESCE(group_id, '')) = ''
  AND TRIM(COALESCE(group_did, '')) = ''"#,
            )
            .map_err(super::local_state_unavailable)?;
        let rows = statement
            .query_map([], |row| {
                Ok(LegacyCanonicalDirectWireCandidate {
                    owner_identity_id: row.get(0)?,
                    message_id: row.get(1)?,
                    owner_did: row.get(2)?,
                    conversation_id: row.get(3)?,
                    sender_did: row.get(4)?,
                    receiver_did: row.get(5)?,
                })
            })
            .map_err(super::local_state_unavailable)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(super::local_state_unavailable)?
    };

    let mut repaired = 0usize;
    for candidate in candidates {
        let owner_did = candidate.owner_did.trim();
        let sender_did = candidate.sender_did.trim();
        let receiver_did = candidate.receiver_did.trim();
        let peer_did = match (sender_did == owner_did, receiver_did == owner_did) {
            (true, false) => receiver_did,
            (false, true) => sender_did,
            _ => continue,
        };
        if crate::ids::Did::parse(owner_did).is_err() || crate::ids::Did::parse(peer_did).is_err() {
            continue;
        }

        let registry_persona_id = connection
            .query_row(
                r#"SELECT peer_persona_id
FROM conversation_registry
WHERE owner_identity_id = ?1
  AND conversation_id = ?2
  AND thread_kind = 'direct'
  AND resolution_state = 'resolved'
  AND TRIM(COALESCE(peer_persona_id, '')) <> ''"#,
                (
                    candidate.owner_identity_id.as_str(),
                    candidate.conversation_id.as_str(),
                ),
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(super::local_state_unavailable)?;
        let Some(registry_persona_id) = registry_persona_id else {
            continue;
        };
        let Ok(Some(route)) = super::direct_peer_routes::get(
            connection,
            &candidate.owner_identity_id,
            &candidate.conversation_id,
        ) else {
            continue;
        };
        if route.peer_persona_id.as_deref().map(str::trim) != Some(registry_persona_id.trim()) {
            continue;
        }
        let Ok(Some(resolved_peer)) = super::peer_personas::resolve_by_did(
            connection,
            &candidate.owner_identity_id,
            peer_did,
        ) else {
            continue;
        };
        if resolved_peer.peer_persona_id != registry_persona_id
            || resolved_peer.conversation_id != candidate.conversation_id
        {
            continue;
        }

        repaired += connection
            .execute(
                r#"UPDATE messages
SET wire_thread_kind = 'direct',
    wire_thread_ref = ?1,
    wire_identity_resolution_state = 'resolved'
WHERE owner_identity_id = ?2
  AND msg_id = ?3
  AND conversation_id = ?4
  AND wire_thread_kind = 'thread'
  AND wire_thread_ref = conversation_id
  AND wire_identity_resolution_state = 'resolved'"#,
                rusqlite::params![
                    peer_did,
                    candidate.owner_identity_id,
                    candidate.message_id,
                    candidate.conversation_id,
                ],
            )
            .map_err(super::local_state_unavailable)?;
    }
    Ok(repaired)
}

#[cfg(feature = "sqlite")]
pub(crate) fn upsert_message(
    connection: &rusqlite::Connection,
    record: &MessageRecord,
) -> crate::ImResult<()> {
    crate::internal::local_state::schema::ensure_schema(connection)?;
    // A savepoint is atomic both on a standalone connection and inside the
    // local-state actor's wider transaction.
    connection
        .execute_batch("SAVEPOINT awiki_message_upsert")
        .map_err(super::local_state_unavailable)?;
    let result = (|| {
        let _ = upsert_message_record(connection, record)?;
        let _ = crate::internal::group_rebind_recovery::project_rebind_event(connection, record)?;
        Ok(())
    })();
    match result {
        Ok(()) => connection
            .execute_batch("RELEASE SAVEPOINT awiki_message_upsert")
            .map_err(super::local_state_unavailable),
        Err(error) => {
            let rollback = connection.execute_batch(
                "ROLLBACK TO SAVEPOINT awiki_message_upsert; RELEASE SAVEPOINT awiki_message_upsert",
            );
            match rollback {
                Ok(()) => Err(error),
                Err(rollback_error) => Err(super::local_state_unavailable(rollback_error)),
            }
        }
    }
}

#[cfg(feature = "sqlite")]
fn upsert_message_record(
    connection: &rusqlite::Connection,
    record: &MessageRecord,
) -> crate::ImResult<BTreeSet<(String, String)>> {
    let input_msg_id = required("msg_id", &record.msg_id)?;
    let owner_identity_id = required("owner_identity_id", &record.owner_identity_id)?;
    let owner_did = required("owner_did", &record.owner_did)?;
    let stable_conversation_id_value =
        required("conversation_id", &stable_conversation_id(record))?;
    // Once a rotated DID direct thread has been folded into a peer-scope thread,
    // late legacy projections should use the same canonical conversation without
    // re-running the expensive legacy scan/update path.
    let conversation_id = if record.group_id.trim().is_empty() && record.group_did.trim().is_empty()
    {
        cached_peer_scope_conversation_id_for_legacy_direct(
            connection,
            &owner_identity_id,
            &stable_conversation_id_value,
        )?
        .unwrap_or(stable_conversation_id_value)
    } else {
        stable_conversation_id_value
    };
    let thread_id = conversation_id.clone();
    let canonical_group_msg_id = canonical_group_message_id(record, &conversation_id);
    let msg_id = canonical_group_msg_id.clone().unwrap_or(input_msg_id);
    let wire_identity = wire_identity_for_record(record)?;
    assert_existing_wire_identity_compatible(
        connection,
        &owner_identity_id,
        &msg_id,
        record,
        &wire_identity,
    )?;
    let group_aliases = group_message_aliases(record, &msg_id);
    let group_duplicate_rows = existing_group_duplicate_rows(
        connection,
        &owner_identity_id,
        &conversation_id,
        &msg_id,
        record.server_seq,
        &group_aliases,
    )?;
    let mut touched = BTreeSet::new();
    let previous_projection = super::conversation_summaries::message_projection_for_id(
        connection,
        &owner_identity_id,
        &msg_id,
    )?;
    if let Some(previous_projection) = previous_projection.as_ref() {
        touched.insert((
            owner_identity_id.clone(),
            previous_projection.conversation_id.clone(),
        ));
    }
    for duplicate in &group_duplicate_rows {
        touched.insert((
            duplicate.owner_identity_id.clone(),
            stable_conversation_id(duplicate),
        ));
    }
    let sent_at =
        merged_group_projection_value(record.sent_at.as_str(), &group_duplicate_rows, |row| {
            row.sent_at.as_str()
        });
    let stored_at = default_string(
        merged_group_projection_value(record.stored_at.as_str(), &group_duplicate_rows, |row| {
            row.stored_at.as_str()
        })
        .as_str(),
        &now_utc_like(),
    );
    let content =
        merged_group_projection_value(record.content.as_str(), &group_duplicate_rows, |row| {
            row.content.as_str()
        });
    let title =
        merged_group_projection_value(record.title.as_str(), &group_duplicate_rows, |row| {
            row.title.as_str()
        });
    let content_type = default_string(
        merged_group_projection_value(record.content_type.as_str(), &group_duplicate_rows, |row| {
            row.content_type.as_str()
        })
        .as_str(),
        "text/plain",
    );
    let sender_name =
        merged_group_projection_value(record.sender_name.as_str(), &group_duplicate_rows, |row| {
            row.sender_name.as_str()
        });
    let metadata = merged_group_metadata(record.metadata.as_str(), &group_duplicate_rows);
    let hydration_state = if record.hydration_state == MessageHydrationState::Hydrated
        || group_duplicate_rows
            .iter()
            .any(|row| row.hydration_state == MessageHydrationState::Hydrated)
    {
        MessageHydrationState::Hydrated
    } else if record.hydration_state == MessageHydrationState::Discovered
        || group_duplicate_rows
            .iter()
            .any(|row| row.hydration_state == MessageHydrationState::Discovered)
    {
        MessageHydrationState::Discovered
    } else {
        MessageHydrationState::LegacyProbe
    };
    let is_e2ee = record.is_e2ee || group_duplicate_rows.iter().any(|row| row.is_e2ee);
    let is_control_payload = hydration_state == MessageHydrationState::Hydrated
        && is_control_payload_for_projection(&content_type, &content, &record.sender_did);
    let is_read = record.is_read;
    let is_read = is_read || is_control_payload;
    let mentions_current_user = mentions_current_user_for_projection(
        &owner_did,
        record.direction,
        &record.thread_id,
        &record.group_id,
        &record.group_did,
        &content_type,
        &content,
    ) || group_duplicate_rows
        .iter()
        .any(|row| row.mentions_current_user);
    remove_group_duplicate_rows(
        connection,
        &owner_identity_id,
        &msg_id,
        &group_duplicate_rows,
    )?;
    let group_identity_aliases = canonical_group_msg_id
        .as_ref()
        .map(|_| merged_group_message_aliases(record, &group_duplicate_rows, &msg_id))
        .unwrap_or_default();
    connection
        .execute(
            r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, conversation_id,
     wire_thread_kind, wire_thread_ref, wire_identity_resolution_state,
     thread_id, direction, sender_did, receiver_did,
     group_id, group_did, content_type, content, title, server_seq, hydration_state,
     sent_at, stored_at,
     is_e2ee, is_read, sender_name, metadata, mentions_current_user, credential_name)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
        ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26)
ON CONFLICT(owner_identity_id, msg_id) DO UPDATE SET
    owner_did = excluded.owner_did,
    conversation_id = CASE
        WHEN messages.hydration_state = 'hydrated' AND excluded.hydration_state <> 'hydrated'
        THEN messages.conversation_id
        ELSE excluded.conversation_id
    END,
    wire_thread_kind = CASE
        WHEN TRIM(COALESCE(messages.wire_thread_kind, '')) = '' THEN excluded.wire_thread_kind
        ELSE messages.wire_thread_kind
    END,
    wire_thread_ref = CASE
        WHEN TRIM(COALESCE(messages.wire_thread_ref, '')) = '' THEN excluded.wire_thread_ref
        ELSE messages.wire_thread_ref
    END,
    wire_identity_resolution_state = CASE
        WHEN messages.wire_identity_resolution_state = 'resolved' THEN 'resolved'
        ELSE excluded.wire_identity_resolution_state
    END,
    direction = CASE
        WHEN messages.hydration_state = 'hydrated' AND excluded.hydration_state <> 'hydrated'
             AND messages.direction <> -1
        THEN messages.direction
        ELSE excluded.direction
    END,
    sender_did = COALESCE(NULLIF(messages.sender_did, ''), excluded.sender_did),
    receiver_did = COALESCE(NULLIF(messages.receiver_did, ''), excluded.receiver_did),
    group_id = COALESCE(NULLIF(messages.group_id, ''), excluded.group_id),
    group_did = COALESCE(NULLIF(messages.group_did, ''), excluded.group_did),
    content_type = CASE
        WHEN messages.hydration_state = 'hydrated' AND excluded.hydration_state <> 'hydrated'
        THEN messages.content_type
        WHEN excluded.content IS NULL THEN messages.content_type
        ELSE excluded.content_type
    END,
    content = CASE
        WHEN messages.hydration_state = 'hydrated' AND excluded.hydration_state <> 'hydrated'
        THEN messages.content
        ELSE COALESCE(excluded.content, messages.content)
    END,
    title = CASE
        WHEN messages.hydration_state = 'hydrated' AND excluded.hydration_state <> 'hydrated'
        THEN messages.title
        ELSE COALESCE(excluded.title, messages.title)
    END,
    server_seq = COALESCE(messages.server_seq, excluded.server_seq),
    hydration_state = CASE
        WHEN messages.hydration_state = 'hydrated' OR excluded.hydration_state = 'hydrated'
        THEN 'hydrated'
        WHEN messages.hydration_state = 'discovered' OR excluded.hydration_state = 'discovered'
        THEN 'discovered'
        ELSE 'legacy_probe'
    END,
    sent_at = CASE
        WHEN messages.hydration_state = 'hydrated' AND excluded.hydration_state <> 'hydrated'
        THEN COALESCE(NULLIF(messages.sent_at, ''), excluded.sent_at)
        ELSE excluded.sent_at
    END,
    stored_at = CASE
        WHEN messages.hydration_state = 'hydrated' AND excluded.hydration_state <> 'hydrated'
        THEN messages.stored_at
        ELSE excluded.stored_at
    END,
    is_e2ee = CASE WHEN excluded.is_e2ee = 1 OR messages.is_e2ee = 1 THEN 1 ELSE 0 END,
    is_read = CASE
        WHEN messages.hydration_state = 'hydrated' AND excluded.hydration_state <> 'hydrated'
        THEN messages.is_read
        WHEN excluded.is_read = 1 OR messages.is_read = 1 THEN 1
        ELSE 0
    END,
    sender_name = CASE
        WHEN messages.hydration_state = 'hydrated' AND excluded.hydration_state <> 'hydrated'
        THEN COALESCE(NULLIF(messages.sender_name, ''), excluded.sender_name)
        ELSE excluded.sender_name
    END,
    metadata = CASE
        WHEN messages.hydration_state = 'hydrated' AND excluded.hydration_state <> 'hydrated'
        THEN COALESCE(NULLIF(messages.metadata, ''), excluded.metadata)
        WHEN excluded.content IS NULL
         AND messages.is_e2ee = 1
         AND json_valid(COALESCE(NULLIF(messages.metadata, ''), '{}')) = 1
         AND json_extract(
               COALESCE(NULLIF(messages.metadata, ''), '{}'),
               '$.decryption_state'
             ) = 'decrypted'
         AND json_valid(COALESCE(NULLIF(excluded.metadata, ''), '{}')) = 1
         AND json_extract(
               COALESCE(NULLIF(excluded.metadata, ''), '{}'),
               '$.decryption_state'
             ) = 'failed'
        THEN messages.metadata
        ELSE excluded.metadata
    END,
    mentions_current_user = CASE
        WHEN messages.hydration_state = 'hydrated' AND excluded.hydration_state <> 'hydrated'
        THEN messages.mentions_current_user
        WHEN excluded.content IS NULL THEN messages.mentions_current_user
        ELSE excluded.mentions_current_user
    END,
    credential_name = CASE
        WHEN messages.hydration_state = 'hydrated' AND excluded.hydration_state <> 'hydrated'
        THEN COALESCE(NULLIF(messages.credential_name, ''), excluded.credential_name)
        ELSE excluded.credential_name
    END"#,
            rusqlite::params![
                msg_id,
                owner_identity_id,
                owner_did,
                conversation_id,
                wire_identity.kind,
                wire_identity.reference,
                wire_identity.resolution_state,
                thread_id,
                record.direction,
                nullable_text(&record.sender_did),
                nullable_text(&record.receiver_did),
                nullable_text(&record.group_id),
                nullable_text(&record.group_did),
                content_type,
                nullable_text(&content),
                nullable_text(&title),
                record.server_seq,
                hydration_state.as_str(),
                nullable_text(&sent_at),
                stored_at,
                is_e2ee,
                is_read,
                nullable_text(&sender_name),
                nullable_text(&metadata),
                mentions_current_user,
                record.credential_name.trim(),
            ],
        )
        .map_err(super::local_state_unavailable)?;
    upsert_message_identity_aliases(
        connection,
        &owner_identity_id,
        &msg_id,
        &group_identity_aliases,
        "group_message",
        &stored_at,
    )?;
    let merged_legacy_ids = merge_legacy_direct_did_conversation(
        connection,
        &owner_identity_id,
        &conversation_id,
        record,
    )?;
    for legacy_id in &merged_legacy_ids {
        touched.insert((owner_identity_id.clone(), legacy_id.clone()));
    }
    let next_projection = super::conversation_summaries::message_projection_for_id(
        connection,
        &owner_identity_id,
        &msg_id,
    )?;
    if let Some(next_projection) = next_projection.as_ref() {
        touched.insert((
            owner_identity_id.clone(),
            next_projection.conversation_id.clone(),
        ));
    }
    if !merged_legacy_ids.is_empty() || !group_duplicate_rows.is_empty() {
        super::conversation_summaries::rebuild_touched(connection, &touched)?;
    } else if let Some(next_projection) = next_projection.as_ref() {
        super::conversation_summaries::apply_message_delta_or_rebuild(
            connection,
            previous_projection.as_ref(),
            next_projection,
        )?;
    } else {
        super::conversation_summaries::rebuild_touched(connection, &touched)?;
    }
    let committed_conversation_id = next_projection
        .as_ref()
        .map(|projection| projection.conversation_id.as_str())
        .unwrap_or(conversation_id.as_str());
    super::conversation_registry::ensure_from_summary(
        connection,
        &owner_identity_id,
        committed_conversation_id,
    )?;
    touched.insert((owner_identity_id, committed_conversation_id.to_owned()));
    Ok(touched)
}

#[cfg(feature = "sqlite")]
pub(crate) fn repair_group_message_identity_projection(
    connection: &rusqlite::Connection,
) -> crate::ImResult<usize> {
    let records = {
        let mut statement = connection
            .prepare(
                r#"
SELECT msg_id,
       owner_identity_id,
       owner_did,
       conversation_id,
       thread_id,
       direction,
       sender_did,
       receiver_did,
       group_id,
       group_did,
       content_type,
       content,
       title,
       server_seq,
       hydration_state,
       sent_at,
       stored_at,
       is_e2ee,
       is_read,
       sender_name,
       metadata,
       mentions_current_user,
       credential_name
FROM messages
WHERE server_seq IS NOT NULL
  AND (
      TRIM(COALESCE(group_id, '')) <> ''
      OR TRIM(COALESCE(group_did, '')) <> ''
      OR COALESCE(NULLIF(conversation_id, ''), thread_id) LIKE 'group:%'
  )
ORDER BY owner_identity_id,
         COALESCE(NULLIF(conversation_id, ''), thread_id),
         server_seq,
         CASE WHEN msg_id = group_did || ':' || server_seq THEN 0 ELSE 1 END,
         msg_id"#,
            )
            .map_err(super::local_state_unavailable)?;
        let rows = statement
            .query_map([], message_record_from_row)
            .map_err(super::local_state_unavailable)?;
        let mut records = Vec::new();
        for row in rows {
            records.push(row.map_err(super::local_state_unavailable)?);
        }
        records
    };

    let mut repaired = 0usize;
    for record in records {
        if !message_exists(connection, &record.owner_identity_id, &record.msg_id)? {
            continue;
        }
        let conversation_id = stable_conversation_id(&record);
        let Some(canonical_id) = canonical_group_message_id(&record, &conversation_id) else {
            continue;
        };
        let aliases = group_message_aliases(&record, &canonical_id);
        let duplicates = existing_group_duplicate_rows(
            connection,
            &record.owner_identity_id,
            &conversation_id,
            &canonical_id,
            record.server_seq,
            &aliases,
        )?;
        if !aliases.is_empty() {
            upsert_message_identity_aliases(
                connection,
                &record.owner_identity_id,
                &canonical_id,
                &aliases,
                "group_message_repair",
                &default_string(&record.stored_at, &now_utc_like()),
            )?;
        }
        if record.msg_id.trim() != canonical_id || !duplicates.is_empty() {
            upsert_message_record(connection, &record)?;
            repaired = repaired.saturating_add(1);
        }
    }
    Ok(repaired)
}

#[cfg(feature = "sqlite")]
fn canonical_group_message_id(record: &MessageRecord, conversation_id: &str) -> Option<String> {
    let server_seq = record.server_seq?;
    if server_seq < 0 {
        return None;
    }
    let group = canonical_group_ref(record, conversation_id)?;
    Some(format!("{group}:{server_seq}"))
}

#[cfg(feature = "sqlite")]
fn canonical_group_ref(record: &MessageRecord, conversation_id: &str) -> Option<String> {
    if let Some(group) = normalize_group_ref(record.group_did.as_str())
        .or_else(|| normalize_group_ref(&record.group_id))
    {
        return Some(group);
    }
    if conversation_id.trim().starts_with("group:") {
        return normalize_group_ref(conversation_id);
    }
    if record.thread_id.trim().starts_with("group:") {
        return normalize_group_ref(&record.thread_id);
    }
    None
}

#[cfg(feature = "sqlite")]
fn normalize_group_ref(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let value = value.strip_prefix("group:").unwrap_or(value).trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_owned())
    }
}

#[cfg(feature = "sqlite")]
fn group_message_aliases(record: &MessageRecord, canonical_id: &str) -> Vec<String> {
    let mut aliases = Vec::new();
    push_unique(&mut aliases, record.msg_id.trim().to_owned());
    let metadata = parse_metadata(&record.metadata);
    for key in ["raw_message_id", "client_message_id", "operation_id"] {
        if let Some(value) = metadata
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            push_unique(&mut aliases, value.to_owned());
        }
    }
    aliases.retain(|alias| alias != canonical_id);
    aliases
}

#[cfg(feature = "sqlite")]
fn merged_group_message_aliases(
    record: &MessageRecord,
    duplicate_rows: &[MessageRecord],
    canonical_id: &str,
) -> Vec<String> {
    let mut aliases = group_message_aliases(record, canonical_id);
    for duplicate in duplicate_rows {
        for alias in group_message_aliases(duplicate, canonical_id) {
            push_unique(&mut aliases, alias);
        }
    }
    aliases.retain(|alias| alias.trim() != canonical_id);
    aliases
}

#[cfg(feature = "sqlite")]
fn upsert_message_identity_aliases(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    canonical_msg_id: &str,
    aliases: &[String],
    source: &str,
    stored_at: &str,
) -> crate::ImResult<usize> {
    let owner_identity_id = required("owner_identity_id", owner_identity_id)?;
    let canonical_msg_id = required("canonical_msg_id", canonical_msg_id)?;
    let stored_at = default_string(stored_at, &now_utc_like());
    let mut updated = 0usize;
    for alias in aliases {
        let alias = alias.trim();
        if alias.is_empty() || alias == canonical_msg_id {
            continue;
        }
        updated += connection
            .execute(
                r#"
INSERT INTO message_identity_aliases
    (owner_identity_id, alias_msg_id, canonical_msg_id, source, stored_at)
VALUES (?1, ?2, ?3, ?4, ?5)
ON CONFLICT(owner_identity_id, alias_msg_id) DO UPDATE SET
    canonical_msg_id = excluded.canonical_msg_id,
    source = excluded.source,
    stored_at = excluded.stored_at
WHERE message_identity_aliases.canonical_msg_id <> excluded.canonical_msg_id
   OR message_identity_aliases.source <> excluded.source
   OR message_identity_aliases.stored_at <> excluded.stored_at"#,
                rusqlite::params![
                    owner_identity_id,
                    alias,
                    canonical_msg_id,
                    source.trim(),
                    stored_at,
                ],
            )
            .map_err(super::local_state_unavailable)?;
    }
    Ok(updated)
}

#[cfg(feature = "sqlite")]
fn existing_group_duplicate_rows(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    conversation_id: &str,
    canonical_msg_id: &str,
    server_seq: Option<i64>,
    aliases: &[String],
) -> crate::ImResult<Vec<MessageRecord>> {
    if server_seq.is_none() && aliases.is_empty() {
        return Ok(Vec::new());
    }
    let alias_placeholders = vec!["?"; aliases.len()].join(",");
    let mut statement = String::from(
        r#"
SELECT msg_id,
       owner_identity_id,
       owner_did,
       conversation_id,
       thread_id,
       direction,
       sender_did,
       receiver_did,
       group_id,
       group_did,
       content_type,
       content,
       title,
       server_seq,
       hydration_state,
       sent_at,
       stored_at,
       is_e2ee,
       is_read,
       sender_name,
       metadata,
       mentions_current_user,
       credential_name
FROM messages
WHERE owner_identity_id = ?
  AND msg_id <> ?
  AND ("#,
    );
    if server_seq.is_some() {
        statement.push_str(
            r#"
      (
        server_seq = ?
        AND COALESCE(NULLIF(conversation_id, ''), thread_id) = ?
        AND (
            TRIM(COALESCE(group_id, '')) <> ''
            OR TRIM(COALESCE(group_did, '')) <> ''
            OR COALESCE(NULLIF(conversation_id, ''), thread_id) LIKE 'group:%'
        )
      )"#,
        );
    }
    if !aliases.is_empty() {
        if server_seq.is_some() {
            statement.push_str(" OR ");
        }
        statement.push_str(
            r#"
      (
        msg_id IN ("#,
        );
        statement.push_str(&alias_placeholders);
        statement.push_str(
            r#")
        AND COALESCE(NULLIF(conversation_id, ''), thread_id) = ?
        AND (
            TRIM(COALESCE(group_id, '')) <> ''
            OR TRIM(COALESCE(group_did, '')) <> ''
            OR COALESCE(NULLIF(conversation_id, ''), thread_id) LIKE 'group:%'
        )
      )"#,
        );
    }
    statement.push_str(")\nORDER BY CASE WHEN msg_id = ? THEN 0 ELSE 1 END, msg_id");

    let mut params: Vec<&dyn rusqlite::ToSql> = Vec::new();
    params.push(&owner_identity_id);
    params.push(&canonical_msg_id);
    if let Some(server_seq) = &server_seq {
        params.push(server_seq);
        params.push(&conversation_id);
    }
    for alias in aliases {
        params.push(alias);
    }
    if !aliases.is_empty() {
        params.push(&conversation_id);
    }
    params.push(&canonical_msg_id);

    let mut statement = connection
        .prepare(&statement)
        .map_err(super::local_state_unavailable)?;
    let rows = statement
        .query_map(params.as_slice(), message_record_from_row)
        .map_err(super::local_state_unavailable)?;
    let mut records = Vec::new();
    for row in rows {
        records.push(row.map_err(super::local_state_unavailable)?);
    }
    Ok(records)
}

#[cfg(feature = "sqlite")]
fn message_exists(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    msg_id: &str,
) -> crate::ImResult<bool> {
    let exists = connection
        .query_row(
            r#"
SELECT EXISTS(
    SELECT 1
    FROM messages
    WHERE owner_identity_id = ?1 AND msg_id = ?2
)"#,
            rusqlite::params![owner_identity_id, msg_id],
            |row| row.get::<_, i64>(0),
        )
        .map_err(super::local_state_unavailable)?;
    Ok(exists != 0)
}

#[cfg(feature = "sqlite")]
pub(crate) fn message_server_seq(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    msg_id: &str,
) -> crate::ImResult<Option<i64>> {
    connection
        .query_row(
            r#"
SELECT server_seq
FROM messages
WHERE owner_identity_id = ?1 AND msg_id = ?2
"#,
            rusqlite::params![owner_identity_id, msg_id],
            |row| row.get::<_, Option<i64>>(0),
        )
        .optional()
        .map(|value| value.flatten())
        .map_err(super::local_state_unavailable)
}

#[cfg(feature = "sqlite")]
fn remove_group_duplicate_rows(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    canonical_msg_id: &str,
    duplicate_rows: &[MessageRecord],
) -> crate::ImResult<()> {
    for duplicate in duplicate_rows {
        let duplicate_id = duplicate.msg_id.trim();
        if duplicate_id.is_empty() || duplicate_id == canonical_msg_id {
            continue;
        }
        connection
            .execute(
                "DELETE FROM messages WHERE owner_identity_id = ?1 AND msg_id = ?2",
                rusqlite::params![owner_identity_id, duplicate_id],
            )
            .map_err(super::local_state_unavailable)?;
    }
    Ok(())
}

#[cfg(feature = "sqlite")]
fn merged_group_projection_value<F>(
    incoming: &str,
    duplicate_rows: &[MessageRecord],
    get_value: F,
) -> String
where
    F: Fn(&MessageRecord) -> &str,
{
    if let Some(value) = non_empty(incoming) {
        return value.to_owned();
    }
    duplicate_rows
        .iter()
        .filter_map(|row| non_empty(get_value(row)))
        .next()
        .unwrap_or("")
        .to_owned()
}

#[cfg(feature = "sqlite")]
fn merged_group_metadata(incoming: &str, duplicate_rows: &[MessageRecord]) -> String {
    let mut merged = serde_json::Map::new();
    for value in duplicate_rows
        .iter()
        .map(|row| row.metadata.as_str())
        .chain(std::iter::once(incoming))
    {
        let Ok(serde_json::Value::Object(object)) =
            serde_json::from_str::<serde_json::Value>(value)
        else {
            continue;
        };
        for (key, value) in object {
            merged.insert(key, value);
        }
    }
    if !merged.is_empty() {
        return serde_json::Value::Object(merged).to_string();
    }
    merged_group_projection_value(incoming, duplicate_rows, |row| row.metadata.as_str())
}

#[cfg(feature = "sqlite")]
pub(crate) fn mentions_current_user_for_projection(
    owner_did: &str,
    direction: i64,
    thread_id: &str,
    group_id: &str,
    group_did: &str,
    content_type: &str,
    content: &str,
) -> bool {
    if direction != 0 || !is_group_projection(thread_id, group_id, group_did) {
        return false;
    }
    let content_type = content_type.trim();
    if content_type != "application/json"
        && content_type != crate::attachments::attachment_manifest_content_type()
    {
        return false;
    }
    let owner_did = owner_did.trim();
    if owner_did.is_empty() {
        return false;
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(content) else {
        return false;
    };
    let Ok(payload) = crate::messages::parse_message_mention_payload(&value) else {
        return false;
    };
    payload
        .mentions
        .iter()
        .any(|mention| match &mention.target {
            crate::messages::MessageMentionTarget::Human { did, .. } => did.trim() == owner_did,
            crate::messages::MessageMentionTarget::GroupSelector { selector } => matches!(
                selector,
                crate::messages::MessageMentionSelector::All
                    | crate::messages::MessageMentionSelector::Humans
            ),
            crate::messages::MessageMentionTarget::Agent { .. } => false,
        })
}

#[cfg(feature = "sqlite")]
pub(crate) fn is_control_payload_for_projection(
    content_type: &str,
    content: &str,
    sender_did: &str,
) -> bool {
    if !content_type.trim().eq_ignore_ascii_case("application/json") {
        return false;
    }
    let daemon_sender = is_daemon_control_sender(sender_did);
    let content = content.trim();
    if content.is_empty() {
        return true;
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(content) else {
        return false;
    };
    is_control_payload_value(&value) || (daemon_sender && is_daemon_control_payload_value(&value))
}

#[cfg(feature = "sqlite")]
fn is_control_payload_value(value: &serde_json::Value) -> bool {
    value
        .as_object()
        .and_then(|object| object.get("schema"))
        .and_then(serde_json::Value::as_str)
        .map(|schema| schema.trim().starts_with("awiki."))
        .unwrap_or(false)
}

#[cfg(feature = "sqlite")]
fn is_daemon_control_sender(sender_did: &str) -> bool {
    sender_did.trim().contains(":agent:daemon:")
}

#[cfg(feature = "sqlite")]
fn is_daemon_control_payload_value(value: &serde_json::Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    object.contains_key("daemon")
        || object.contains_key("runtimes")
        || object.contains_key("command_id")
        || object.contains_key("events")
        || object
            .get("command")
            .and_then(serde_json::Value::as_str)
            .map(|command| command.trim().starts_with("agent."))
            .unwrap_or(false)
}

#[cfg(feature = "sqlite")]
pub(crate) fn repair_control_payload_read_projection(
    connection: &rusqlite::Connection,
) -> crate::ImResult<usize> {
    let candidates = {
        let mut statement = connection
            .prepare(
                r#"
SELECT owner_identity_id,
       msg_id,
       COALESCE(NULLIF(conversation_id, ''), thread_id) AS conversation_id,
       content_type,
       content,
       sender_did
FROM messages
WHERE direction = 0
  AND is_read = 0
  AND hydration_state = 'hydrated'
  AND LOWER(TRIM(COALESCE(content_type, ''))) = 'application/json'"#,
            )
            .map_err(super::local_state_unavailable)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, Option<String>>("owner_identity_id")?
                        .unwrap_or_default(),
                    row.get::<_, Option<String>>("msg_id")?.unwrap_or_default(),
                    row.get::<_, Option<String>>("conversation_id")?
                        .unwrap_or_default(),
                    row.get::<_, Option<String>>("content_type")?
                        .unwrap_or_default(),
                    row.get::<_, Option<String>>("content")?.unwrap_or_default(),
                    row.get::<_, Option<String>>("sender_did")?
                        .unwrap_or_default(),
                ))
            })
            .map_err(super::local_state_unavailable)?;
        let mut candidates = Vec::new();
        for row in rows {
            let (owner_identity_id, msg_id, conversation_id, content_type, content, sender_did) =
                row.map_err(super::local_state_unavailable)?;
            if owner_identity_id.trim().is_empty()
                || msg_id.trim().is_empty()
                || conversation_id.trim().is_empty()
                || !is_control_payload_for_projection(&content_type, &content, &sender_did)
            {
                continue;
            }
            candidates.push((owner_identity_id, msg_id, conversation_id));
        }
        candidates
    };

    let mut touched = BTreeSet::new();
    let mut updated = 0usize;
    for (owner_identity_id, msg_id, conversation_id) in candidates {
        let rows = connection
            .execute(
                r#"
UPDATE messages
SET is_read = 1
WHERE owner_identity_id = ?1 AND msg_id = ?2 AND is_read = 0"#,
                rusqlite::params![owner_identity_id, msg_id],
            )
            .map_err(super::local_state_unavailable)?;
        if rows > 0 {
            updated += rows;
            touched.insert((owner_identity_id, conversation_id));
        }
    }
    if !touched.is_empty() {
        super::conversation_summaries::rebuild_touched(connection, &touched)?;
    }
    Ok(updated)
}

#[cfg(feature = "sqlite")]
pub(crate) fn repair_thread_read_state_alias_projection(
    connection: &rusqlite::Connection,
) -> crate::ImResult<usize> {
    let states = {
        let mut statement = connection
            .prepare(
                r#"
SELECT owner_identity_id,
       owner_did,
       thread_id,
       read_watermark_seq,
       read_watermark_at
FROM thread_read_state
WHERE thread_scope = 'direct'
  AND NULLIF(TRIM(COALESCE(thread_id, '')), '') IS NOT NULL
  AND NULLIF(TRIM(COALESCE(read_watermark_seq, '')), '') IS NOT NULL
  AND EXISTS (
      SELECT 1
      FROM messages
      WHERE messages.owner_identity_id = thread_read_state.owner_identity_id
        AND messages.direction = 0
        AND messages.is_read = 0
        AND COALESCE(NULLIF(messages.conversation_id, ''), messages.thread_id) LIKE 'dm:peer-scope:%'
      LIMIT 1
  )"#,
            )
            .map_err(super::local_state_unavailable)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, Option<String>>("owner_identity_id")?
                        .unwrap_or_default(),
                    row.get::<_, Option<String>>("owner_did")?
                        .unwrap_or_default(),
                    row.get::<_, Option<String>>("thread_id")?
                        .unwrap_or_default(),
                    row.get::<_, Option<String>>("read_watermark_seq")?
                        .unwrap_or_default(),
                    row.get::<_, Option<String>>("read_watermark_at")?
                        .unwrap_or_default(),
                ))
            })
            .map_err(super::local_state_unavailable)?;
        let mut states = Vec::new();
        for row in rows {
            states.push(row.map_err(super::local_state_unavailable)?);
        }
        states
    };

    let mut updated = 0usize;
    for (owner_identity_id, owner_did, thread_id, seq, read_at) in states {
        let owner_identity_id = owner_identity_id.trim();
        let owner_did = owner_did.trim();
        let thread_id = thread_id.trim();
        if thread_id.starts_with("dm:peer-scope:") {
            continue;
        }
        let peer = thread_id.strip_prefix("dm:").unwrap_or(thread_id).trim();
        if owner_identity_id.is_empty() || peer.is_empty() || seq.trim().is_empty() {
            continue;
        }
        let conversation_ids =
            conversation_ids_for_direct_peer(connection, owner_identity_id, owner_did, peer)?;
        if conversation_ids.is_empty() {
            continue;
        }
        let rows = mark_thread_messages_read_up_to_seq(
            connection,
            owner_identity_id,
            &conversation_ids,
            &seq,
        )?;
        let rows = usize::try_from(rows).unwrap_or(usize::MAX);
        updated = updated.saturating_add(rows);
        let _ = read_at;
    }
    Ok(updated)
}

#[cfg(feature = "sqlite")]
fn is_group_projection(thread_id: &str, group_id: &str, group_did: &str) -> bool {
    !group_id.trim().is_empty()
        || !group_did.trim().is_empty()
        || thread_id.trim().starts_with("group:")
}

#[cfg(feature = "sqlite")]
pub(crate) fn upsert_messages(
    connection: &rusqlite::Connection,
    records: &[MessageRecord],
) -> crate::ImResult<()> {
    let _ = upsert_messages_with_touched(connection, records)?;
    Ok(())
}

#[cfg(feature = "sqlite")]
pub(crate) fn upsert_messages_with_touched(
    connection: &rusqlite::Connection,
    records: &[MessageRecord],
) -> crate::ImResult<BTreeSet<(String, String)>> {
    crate::internal::local_state::schema::ensure_schema(connection)?;
    let mut touched = BTreeSet::new();
    for record in records {
        touched.extend(upsert_message_record(connection, record)?);
        let _ = crate::internal::group_rebind_recovery::project_rebind_event(connection, record)?;
    }
    Ok(touched)
}

#[cfg(feature = "sqlite")]
pub(crate) fn list_direct_messages_for_owner_identity(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    conversation_ids: &[String],
    limit: i64,
) -> crate::ImResult<Vec<MessageRecord>> {
    let owner_identity_id = required("owner_identity_id", owner_identity_id)?;
    let conversation_ids = normalized_conversation_ids(conversation_ids);
    if conversation_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = vec!["?"; conversation_ids.len()].join(",");
    let statement = format!(
        r#"
SELECT msg_id,
       owner_identity_id,
       owner_did,
       conversation_id,
       thread_id,
       direction,
       sender_did,
       receiver_did,
       group_id,
       group_did,
       content_type,
       content,
       title,
       server_seq,
       hydration_state,
       sent_at,
       stored_at,
       is_e2ee,
       is_read,
       sender_name,
       metadata,
       mentions_current_user,
       credential_name
FROM messages
WHERE owner_identity_id = ?
  AND hydration_state = 'hydrated'
  AND NULLIF(TRIM(COALESCE(group_id, '')), '') IS NULL
  AND NULLIF(TRIM(COALESCE(group_did, '')), '') IS NULL
  AND COALESCE(NULLIF(conversation_id, ''), thread_id) IN ({placeholders})
ORDER BY COALESCE(NULLIF(sent_at, ''), stored_at) DESC, msg_id DESC
LIMIT ?"#
    );
    let mut params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(conversation_ids.len() + 2);
    params.push(&owner_identity_id);
    for conversation_id in &conversation_ids {
        params.push(conversation_id);
    }
    params.push(&limit);

    let mut statement = connection
        .prepare(&statement)
        .map_err(super::local_state_unavailable)?;
    let rows = statement
        .query_map(params.as_slice(), message_record_from_row)
        .map_err(super::local_state_unavailable)?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row.map_err(super::local_state_unavailable)?);
    }
    Ok(result)
}

#[cfg(feature = "sqlite")]
pub(crate) fn list_decrypted_secure_messages_for_owner_identity(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    message_ids: &[String],
) -> crate::ImResult<Vec<MessageRecord>> {
    let owner_identity_id = required("owner_identity_id", owner_identity_id)?;
    let message_ids = message_ids
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>();
    if message_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = vec!["?"; message_ids.len()].join(",");
    let statement = format!(
        r#"
SELECT msg_id,
       owner_identity_id,
       owner_did,
       conversation_id,
       wire_thread_kind,
       wire_thread_ref,
       wire_identity_resolution_state,
       thread_id,
       direction,
       sender_did,
       receiver_did,
       group_id,
       group_did,
       content_type,
       content,
       title,
       server_seq,
       hydration_state,
       sent_at,
       stored_at,
       is_e2ee,
       is_read,
       sender_name,
       metadata,
       mentions_current_user,
       credential_name
FROM messages
WHERE owner_identity_id = ?
  AND (
        msg_id IN ({placeholders})
        OR CASE
             WHEN json_valid(COALESCE(NULLIF(metadata, ''), '{{}}')) = 1
             THEN json_extract(
                    COALESCE(NULLIF(metadata, ''), '{{}}'),
                    '$.raw_message_id'
                  ) IN ({placeholders})
             ELSE 0
           END
      )
  AND is_e2ee = 1
  AND CASE
        WHEN json_valid(COALESCE(NULLIF(metadata, ''), '{{}}')) = 1
        THEN json_extract(COALESCE(NULLIF(metadata, ''), '{{}}'), '$.decryption_state')
        ELSE NULL
      END = 'decrypted'"#
    );
    let mut params: Vec<&dyn rusqlite::ToSql> =
        Vec::with_capacity(message_ids.len().saturating_mul(2) + 1);
    params.push(&owner_identity_id);
    for message_id in &message_ids {
        params.push(message_id);
    }
    for message_id in &message_ids {
        params.push(message_id);
    }
    let mut statement = connection
        .prepare(&statement)
        .map_err(super::local_state_unavailable)?;
    let rows = statement
        .query_map(params.as_slice(), message_record_from_row)
        .map_err(super::local_state_unavailable)?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row.map_err(super::local_state_unavailable)?);
    }
    let committed_ids = result
        .iter()
        .map(|record| record.msg_id.clone())
        .collect::<BTreeSet<_>>();
    let message_ids = message_ids
        .iter()
        .map(|value| (*value).to_owned())
        .collect::<Vec<_>>();
    let backlogged =
        super::inbound_resolution_backlog::list_decrypted_secure_messages_for_owner_identity(
            connection,
            &owner_identity_id,
            &message_ids,
        )?;
    result.extend(
        backlogged
            .into_iter()
            .filter(|record| !committed_ids.contains(&record.msg_id)),
    );
    Ok(result)
}

#[cfg(feature = "sqlite")]
pub(crate) fn reconcile_peer_scope_direct_conversations(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
) -> crate::ImResult<()> {
    let owner_identity_id = required("owner_identity_id", owner_identity_id)?;
    clear_legacy_direct_merge_memo_for_owner(connection, &owner_identity_id)?;
    let mut touched = BTreeSet::new();
    let mut merged_legacy_ids = BTreeSet::new();
    for candidate in peer_scope_direct_candidates(connection, &owner_identity_id)? {
        let record = MessageRecord {
            owner_identity_id: owner_identity_id.clone(),
            owner_did: candidate.owner_did,
            conversation_id: candidate.conversation_id.clone(),
            thread_id: candidate.conversation_id.clone(),
            sender_did: candidate.sender_did,
            receiver_did: candidate.receiver_did,
            metadata: candidate.metadata,
            ..MessageRecord::default()
        };
        touched.insert((owner_identity_id.clone(), candidate.conversation_id.clone()));
        for legacy_id in merge_legacy_direct_did_conversation(
            connection,
            &owner_identity_id,
            &candidate.conversation_id,
            &record,
        )? {
            touched.insert((owner_identity_id.clone(), legacy_id.clone()));
            merged_legacy_ids.insert(legacy_id);
        }
    }
    super::conversation_summaries::rebuild_touched(connection, &touched)?;
    let _ = merged_legacy_ids;
    Ok(())
}

#[cfg(feature = "sqlite")]
fn normalized_conversation_ids(conversation_ids: &[String]) -> Vec<String> {
    let mut ids = Vec::new();
    for conversation_id in conversation_ids {
        push_unique(&mut ids, conversation_id.trim().to_owned());
    }
    ids
}

#[cfg(feature = "sqlite")]
fn message_record_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MessageRecord> {
    Ok(MessageRecord {
        msg_id: row.get::<_, Option<String>>("msg_id")?.unwrap_or_default(),
        owner_identity_id: row
            .get::<_, Option<String>>("owner_identity_id")?
            .unwrap_or_default(),
        owner_did: row
            .get::<_, Option<String>>("owner_did")?
            .unwrap_or_default(),
        conversation_id: row
            .get::<_, Option<String>>("conversation_id")?
            .unwrap_or_default(),
        wire_thread_kind: optional_string_column(row, "wire_thread_kind")?,
        wire_thread_ref: optional_string_column(row, "wire_thread_ref")?,
        wire_identity_resolution_state: optional_string_column(
            row,
            "wire_identity_resolution_state",
        )?,
        thread_id: row
            .get::<_, Option<String>>("thread_id")?
            .unwrap_or_default(),
        direction: row.get::<_, Option<i64>>("direction")?.unwrap_or_default(),
        sender_did: row
            .get::<_, Option<String>>("sender_did")?
            .unwrap_or_default(),
        receiver_did: row
            .get::<_, Option<String>>("receiver_did")?
            .unwrap_or_default(),
        group_id: row
            .get::<_, Option<String>>("group_id")?
            .unwrap_or_default(),
        group_did: row
            .get::<_, Option<String>>("group_did")?
            .unwrap_or_default(),
        content_type: row
            .get::<_, Option<String>>("content_type")?
            .unwrap_or_default(),
        content: row.get::<_, Option<String>>("content")?.unwrap_or_default(),
        title: row.get::<_, Option<String>>("title")?.unwrap_or_default(),
        server_seq: row.get::<_, Option<i64>>("server_seq")?,
        hydration_state: hydration_state_from_row(row)?,
        sent_at: row.get::<_, Option<String>>("sent_at")?.unwrap_or_default(),
        stored_at: row
            .get::<_, Option<String>>("stored_at")?
            .unwrap_or_default(),
        is_e2ee: row.get::<_, Option<bool>>("is_e2ee")?.unwrap_or(false),
        is_read: row.get::<_, Option<bool>>("is_read")?.unwrap_or(false),
        sender_name: row
            .get::<_, Option<String>>("sender_name")?
            .unwrap_or_default(),
        metadata: row
            .get::<_, Option<String>>("metadata")?
            .unwrap_or_default(),
        mentions_current_user: row
            .get::<_, Option<bool>>("mentions_current_user")?
            .unwrap_or(false),
        credential_name: row
            .get::<_, Option<String>>("credential_name")?
            .unwrap_or_default(),
    })
}

#[cfg(feature = "sqlite")]
fn optional_string_column(row: &rusqlite::Row<'_>, column: &str) -> rusqlite::Result<String> {
    match row.get::<_, Option<String>>(column) {
        Ok(value) => Ok(value.unwrap_or_default()),
        Err(rusqlite::Error::InvalidColumnName(_)) => Ok(String::new()),
        Err(error) => Err(error),
    }
}

#[cfg(feature = "sqlite")]
fn hydration_state_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MessageHydrationState> {
    match row.get::<_, Option<String>>("hydration_state") {
        Ok(Some(value)) if value.trim() == "hydrated" => Ok(MessageHydrationState::Hydrated),
        Ok(Some(value)) if value.trim() == "discovered" => Ok(MessageHydrationState::Discovered),
        Ok(Some(value)) if value.trim() == "legacy_probe" => Ok(MessageHydrationState::LegacyProbe),
        // Invalid persisted state is a gap; catch-up must not advance past it.
        Ok(_) => Ok(MessageHydrationState::Discovered),
        // Older compatibility SELECTs do not always request the schema-32-only column.
        Err(rusqlite::Error::InvalidColumnName(_)) => Ok(MessageHydrationState::Hydrated),
        Err(error) => Err(error),
    }
}

#[cfg(feature = "sqlite")]
fn peer_scope_projection_id_for_thread_ref(thread: &crate::messages::ThreadRef) -> Option<String> {
    match thread {
        crate::messages::ThreadRef::Thread(thread) => {
            let raw = thread.as_str().trim();
            raw.starts_with("dm:peer-scope:").then(|| raw.to_owned())
        }
        _ => None,
    }
}

#[cfg(feature = "sqlite")]
fn project_direct_records_to_thread(records: &mut [MessageRecord], thread_id: &str) {
    let thread_id = thread_id.trim();
    if thread_id.is_empty() {
        return;
    }
    for record in records {
        if !record.group_id.trim().is_empty() || !record.group_did.trim().is_empty() {
            continue;
        }
        record.conversation_id = thread_id.to_owned();
        record.thread_id = thread_id.to_owned();
    }
}

#[cfg(feature = "sqlite")]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct MarkReadClassification {
    pub(crate) direct_ids: Vec<String>,
    pub(crate) remote_direct_ids: Vec<String>,
    pub(crate) group_ids: Vec<String>,
    pub(crate) local_only_ids: Vec<String>,
}

#[cfg(feature = "sqlite")]
impl MarkReadClassification {
    pub(crate) fn local_ids(&self) -> Vec<String> {
        let mut ids = self.direct_ids.clone();
        ids.extend(self.group_ids.iter().cloned());
        ids.extend(self.local_only_ids.iter().cloned());
        ids
    }
}

#[cfg(feature = "sqlite")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ThreadUnreadMessageIds {
    pub(crate) message_ids: Vec<String>,
    pub(crate) truncated: bool,
}

#[cfg(feature = "sqlite")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MarkThreadReadWatermarkInput {
    pub(crate) thread: crate::messages::ThreadRef,
    pub(crate) read_watermark_message_id: Option<String>,
    pub(crate) read_watermark_seq: Option<String>,
    pub(crate) read_watermark_at: Option<String>,
    pub(crate) pending_remote_ack: bool,
}

#[cfg(feature = "sqlite")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MarkThreadReadWatermarkResult {
    pub(crate) thread_scope: String,
    pub(crate) thread_id: String,
    pub(crate) conversation_id: String,
    pub(crate) updated_count: i64,
    pub(crate) previous_read_watermark_seq: Option<String>,
    pub(crate) read_watermark_seq: Option<String>,
    pub(crate) read_watermark_message_id: Option<String>,
    pub(crate) read_watermark_at: Option<String>,
    pub(crate) advanced: bool,
    pub(crate) outbox_operation_id: Option<String>,
    pub(crate) remote_thread_key: Option<String>,
    pub(crate) remote_ack_applicable: bool,
}

#[cfg(feature = "sqlite")]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct EffectiveReadWatermark {
    message_id: Option<String>,
    seq: Option<String>,
    invalid_message_id: bool,
}

#[cfg(feature = "sqlite")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ThreadLocalHistoryRecords {
    pub(crate) records: Vec<MessageRecord>,
    pub(crate) next_cursor: Option<String>,
    pub(crate) has_more: bool,
}

#[cfg(feature = "sqlite")]
pub(crate) fn list_messages_for_thread_ref_for_owner_identity(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    owner_did: &str,
    thread: &crate::messages::ThreadRef,
    limit: i64,
    cursor: Option<&str>,
) -> crate::ImResult<ThreadLocalHistoryRecords> {
    let owner_identity_id = required("owner_identity_id", owner_identity_id)?;
    let limit = normalize_local_history_limit(limit);
    let cursor = decode_local_history_cursor(cursor)?;
    let conversation_ids =
        conversation_ids_for_thread_ref(connection, &owner_identity_id, owner_did, thread)?;
    let peer_scope_projection_id = peer_scope_projection_id_for_thread_ref(thread);
    if conversation_ids.is_empty() {
        return Ok(ThreadLocalHistoryRecords {
            records: Vec::new(),
            next_cursor: None,
            has_more: false,
        });
    }
    let placeholders = vec!["?"; conversation_ids.len()].join(",");
    let mut statement = format!(
        r#"
SELECT msg_id,
       owner_identity_id,
       owner_did,
       conversation_id,
       thread_id,
       direction,
       sender_did,
       receiver_did,
       group_id,
       group_did,
       content_type,
       content,
       title,
       server_seq,
       hydration_state,
       sent_at,
       stored_at,
       is_e2ee,
       is_read,
       sender_name,
       metadata,
       mentions_current_user,
       credential_name
FROM messages
WHERE owner_identity_id = ?
  AND hydration_state = 'hydrated'
  AND COALESCE(NULLIF(conversation_id, ''), thread_id) IN ({placeholders})"#
    );
    if cursor.is_some() {
        statement.push_str(
            r#"
  AND (
        COALESCE(NULLIF(sent_at, ''), stored_at) < ?
     OR (COALESCE(NULLIF(sent_at, ''), stored_at) = ? AND msg_id < ?)
  )"#,
        );
    }
    statement.push_str(
        r#"
ORDER BY COALESCE(NULLIF(sent_at, ''), stored_at) DESC, msg_id DESC
LIMIT ?"#,
    );
    let fetch_limit = limit.saturating_add(1);
    let mut params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(conversation_ids.len() + 5);
    params.push(&owner_identity_id);
    for conversation_id in &conversation_ids {
        params.push(conversation_id);
    }
    if let Some((timestamp, msg_id)) = cursor.as_ref() {
        params.push(timestamp);
        params.push(timestamp);
        params.push(msg_id);
    }
    params.push(&fetch_limit);
    let mut statement = connection
        .prepare(&statement)
        .map_err(super::local_state_unavailable)?;
    let rows = statement
        .query_map(params.as_slice(), message_record_from_row)
        .map_err(super::local_state_unavailable)?;
    let mut records = Vec::new();
    for row in rows {
        records.push(row.map_err(super::local_state_unavailable)?);
    }
    if let Some(peer_scope_projection_id) = peer_scope_projection_id.as_deref() {
        project_direct_records_to_thread(&mut records, peer_scope_projection_id);
    }
    let has_more = i64::try_from(records.len()).unwrap_or(i64::MAX) > limit;
    if has_more {
        records.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
    }
    let next_cursor = if has_more {
        records.last().and_then(encode_local_history_cursor)
    } else {
        None
    };
    Ok(ThreadLocalHistoryRecords {
        records,
        next_cursor,
        has_more,
    })
}

#[cfg(feature = "sqlite")]
pub(crate) fn max_server_seq_for_thread_ref_for_owner_identity(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    owner_did: &str,
    thread: &crate::messages::ThreadRef,
) -> crate::ImResult<Option<i64>> {
    let owner_identity_id = required("owner_identity_id", owner_identity_id)?;
    let conversation_ids =
        conversation_ids_for_thread_ref(connection, &owner_identity_id, owner_did, thread)?;
    if conversation_ids.is_empty() {
        return Ok(None);
    }
    let placeholders = vec!["?"; conversation_ids.len()].join(",");
    let statement = format!(
        r#"
SELECT MAX(server_seq)
FROM messages
WHERE owner_identity_id = ?
  AND hydration_state = 'hydrated'
  AND server_seq IS NOT NULL
  AND COALESCE(NULLIF(conversation_id, ''), thread_id) IN ({placeholders})"#
    );
    let mut params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(conversation_ids.len() + 1);
    params.push(&owner_identity_id);
    for conversation_id in &conversation_ids {
        params.push(conversation_id);
    }
    connection
        .query_row(&statement, params.as_slice(), |row| {
            row.get::<_, Option<i64>>(0)
        })
        .map_err(super::local_state_unavailable)
}

#[cfg(feature = "sqlite")]
pub(crate) fn catch_up_server_seq_for_thread_ref_for_owner_identity(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    owner_did: &str,
    thread: &crate::messages::ThreadRef,
) -> crate::ImResult<CatchUpCursor> {
    let owner_identity_id = required("owner_identity_id", owner_identity_id)?;
    let conversation_ids =
        conversation_ids_for_thread_ref(connection, &owner_identity_id, owner_did, thread)?;
    if conversation_ids.is_empty() {
        return Ok(CatchUpCursor::default());
    }
    let placeholders = vec!["?"; conversation_ids.len()].join(",");
    let statement = format!(
        r#"
SELECT MIN(CASE
               WHEN hydration_state <> 'hydrated' THEN COALESCE(server_seq, 0)
           END),
       MAX(server_seq)
FROM messages
WHERE owner_identity_id = ?
  AND (server_seq IS NOT NULL OR hydration_state <> 'hydrated')
  AND COALESCE(NULLIF(conversation_id, ''), thread_id) IN ({placeholders})"#
    );
    let mut params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(conversation_ids.len() + 1);
    params.push(&owner_identity_id);
    for conversation_id in &conversation_ids {
        params.push(conversation_id);
    }
    let (earliest_gap, max_server_seq) = connection
        .query_row(&statement, params.as_slice(), |row| {
            Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, Option<i64>>(1)?))
        })
        .map_err(super::local_state_unavailable)?;
    let hydration_gap_after_server_seq =
        earliest_gap.map(|server_seq| server_seq.saturating_sub(1).max(0));
    Ok(CatchUpCursor {
        default_after_server_seq: hydration_gap_after_server_seq.or(max_server_seq),
        hydration_gap_after_server_seq,
    })
}

#[cfg(feature = "sqlite")]
pub(crate) fn resolve_legacy_hydration_probes_for_thread_ref(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    owner_did: &str,
    thread: &crate::messages::ThreadRef,
    after_server_seq: i64,
    through_server_seq: i64,
    exhausted: bool,
) -> crate::ImResult<usize> {
    let owner_identity_id = required("owner_identity_id", owner_identity_id)?;
    if !exhausted && through_server_seq <= after_server_seq {
        return Ok(0);
    }
    let conversation_ids =
        conversation_ids_for_thread_ref(connection, &owner_identity_id, owner_did, thread)?;
    if conversation_ids.is_empty() {
        return Ok(0);
    }
    let placeholders = vec!["?"; conversation_ids.len()].join(",");
    let upper_bound = if exhausted {
        String::new()
    } else {
        "\n  AND server_seq <= ?".to_owned()
    };
    let statement = format!(
        r#"
UPDATE messages
SET hydration_state = 'hydrated'
WHERE owner_identity_id = ?
  AND hydration_state = 'legacy_probe'
  AND server_seq IS NOT NULL
  AND server_seq > ?
  AND COALESCE(NULLIF(conversation_id, ''), thread_id) IN ({placeholders}){upper_bound}"#
    );
    let mut params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(conversation_ids.len() + 3);
    params.push(&owner_identity_id);
    params.push(&after_server_seq);
    for conversation_id in &conversation_ids {
        params.push(conversation_id);
    }
    if !exhausted {
        params.push(&through_server_seq);
    }
    connection
        .execute(&statement, params.as_slice())
        .map_err(super::local_state_unavailable)
}

#[cfg(feature = "sqlite")]
pub(crate) fn list_unread_incoming_message_ids_for_owner_identity(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    owner_did: &str,
    thread: &crate::messages::ThreadRef,
    limit: i64,
) -> crate::ImResult<ThreadUnreadMessageIds> {
    let owner_identity_id = required("owner_identity_id", owner_identity_id)?;
    let limit = normalize_thread_mark_read_limit(limit);
    let conversation_ids =
        conversation_ids_for_thread_ref(connection, &owner_identity_id, owner_did, thread)?;
    if conversation_ids.is_empty() {
        return Ok(ThreadUnreadMessageIds {
            message_ids: Vec::new(),
            truncated: false,
        });
    }
    let placeholders = vec!["?"; conversation_ids.len()].join(",");
    let statement = format!(
        r#"
SELECT msg_id
FROM messages
WHERE owner_identity_id = ?
  AND direction = 0
  AND is_read = 0
  AND NULLIF(TRIM(COALESCE(msg_id, '')), '') IS NOT NULL
  AND COALESCE(NULLIF(conversation_id, ''), thread_id) IN ({placeholders})
ORDER BY COALESCE(NULLIF(sent_at, ''), stored_at) DESC, msg_id DESC
LIMIT ?"#
    );
    let fetch_limit = limit.saturating_add(1);
    let mut params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(conversation_ids.len() + 2);
    params.push(&owner_identity_id);
    for conversation_id in &conversation_ids {
        params.push(conversation_id);
    }
    params.push(&fetch_limit);
    let mut statement = connection
        .prepare(&statement)
        .map_err(super::local_state_unavailable)?;
    let rows = statement
        .query_map(params.as_slice(), |row| {
            row.get::<_, Option<String>>("msg_id")
                .map(|value| value.unwrap_or_default())
        })
        .map_err(super::local_state_unavailable)?;
    let mut message_ids = Vec::new();
    for row in rows {
        let message_id = row.map_err(super::local_state_unavailable)?;
        push_unique(&mut message_ids, message_id.trim().to_owned());
    }
    let truncated = i64::try_from(message_ids.len()).unwrap_or(i64::MAX) > limit;
    if truncated {
        message_ids.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
    }
    Ok(ThreadUnreadMessageIds {
        message_ids,
        truncated,
    })
}

#[cfg(feature = "sqlite")]
pub(crate) fn list_incoming_message_ids_up_to_watermark_for_owner_identity(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    owner_did: &str,
    thread: &crate::messages::ThreadRef,
    read_watermark_message_id: Option<&str>,
    read_watermark_seq: Option<&str>,
    limit: i64,
) -> crate::ImResult<ThreadUnreadMessageIds> {
    let owner_identity_id = required("owner_identity_id", owner_identity_id)?;
    let limit = normalize_thread_mark_read_limit(limit);
    let conversation_ids =
        conversation_ids_for_thread_ref(connection, &owner_identity_id, owner_did, thread)?;
    if conversation_ids.is_empty() {
        return Ok(ThreadUnreadMessageIds {
            message_ids: Vec::new(),
            truncated: false,
        });
    }
    if let Some(seq) = read_watermark_seq
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return list_incoming_message_ids_up_to_seq(
            connection,
            &owner_identity_id,
            &conversation_ids,
            seq,
            limit,
        );
    }
    if let Some(message_id) = read_watermark_message_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return list_incoming_message_ids_up_to_message(
            connection,
            &owner_identity_id,
            &conversation_ids,
            message_id,
            limit,
        );
    }
    Ok(ThreadUnreadMessageIds {
        message_ids: Vec::new(),
        truncated: false,
    })
}

#[cfg(feature = "sqlite")]
pub(crate) fn classify_mark_read_ids(
    connection: &rusqlite::Connection,
    owner_did: &str,
    message_ids: &[String],
) -> crate::ImResult<MarkReadClassification> {
    let _ = (connection, owner_did, message_ids);
    Err(crate::ImError::invalid_input(
        Some("owner_identity_id".to_owned()),
        "owner_identity_id is required",
    ))
}

#[cfg(feature = "sqlite")]
pub(crate) fn classify_mark_read_ids_for_owner_identity(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    _owner_did: &str,
    message_ids: &[String],
) -> crate::ImResult<MarkReadClassification> {
    let rows = list_message_classification_rows(connection, owner_identity_id, message_ids)?;
    classify_mark_read_ids_from_rows(message_ids, rows)
}

#[cfg(feature = "sqlite")]
fn classify_mark_read_ids_from_rows(
    message_ids: &[String],
    rows: Vec<MessageClassificationRow>,
) -> crate::ImResult<MarkReadClassification> {
    let rows = rows
        .into_iter()
        .map(|row| (row.requested_msg_id.clone(), row))
        .collect::<BTreeMap<_, _>>();
    let mut result = MarkReadClassification::default();
    for id in message_ids {
        let requested_msg_id = id.trim();
        let Some(row) = rows.get(requested_msg_id) else {
            result.direct_ids.push(id.clone());
            result.remote_direct_ids.push(id.clone());
            continue;
        };
        if row.is_local_mail_notification() {
            result.local_only_ids.push(id.clone());
        } else if row.is_group_message() {
            result.group_ids.push(id.clone());
        } else {
            result.direct_ids.push(id.clone());
            result.remote_direct_ids.push(row.remote_mark_read_id());
        }
    }
    Ok(result)
}

#[cfg(feature = "sqlite")]
pub(crate) fn mark_messages_read(
    connection: &rusqlite::Connection,
    owner_did: &str,
    message_ids: &[String],
) -> crate::ImResult<i64> {
    let _ = (connection, owner_did, message_ids);
    Err(crate::ImError::invalid_input(
        Some("owner_identity_id".to_owned()),
        "owner_identity_id is required",
    ))
}

#[cfg(feature = "sqlite")]
pub(crate) fn mark_messages_read_for_owner_identity(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    _owner_did: &str,
    message_ids: &[String],
) -> crate::ImResult<i64> {
    mark_messages_read_for_owner(connection, owner_identity_id, message_ids)
}

#[cfg(feature = "sqlite")]
pub(crate) fn mark_thread_read_watermark_for_owner_identity(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    owner_did: &str,
    input: MarkThreadReadWatermarkInput,
) -> crate::ImResult<MarkThreadReadWatermarkResult> {
    crate::internal::local_state::schema::ensure_schema(connection)?;
    let owner_identity_id = required("owner_identity_id", owner_identity_id)?;
    let owner_did = owner_did.trim().to_owned();
    let thread_key = thread_read_state_key(&input.thread)?;
    let conversation_ids =
        conversation_ids_for_thread_ref(connection, &owner_identity_id, &owner_did, &input.thread)?;
    let conversation_id = conversation_ids.first().cloned().unwrap_or_default();
    let current = super::read_state::get_thread_read_state(
        connection,
        &owner_identity_id,
        &thread_key.thread_scope,
        &thread_key.thread_id,
    )?;
    let previous_watermark = effective_read_watermark_for_thread(
        connection,
        &owner_identity_id,
        &conversation_ids,
        current.as_ref().and_then(|record| {
            record
                .read_watermark_message_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
        }),
        current.as_ref().and_then(|record| {
            record
                .read_watermark_seq
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
        }),
    )?;
    let requested_watermark = effective_read_watermark_for_thread(
        connection,
        &owner_identity_id,
        &conversation_ids,
        input
            .read_watermark_message_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty()),
        input
            .read_watermark_seq
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty()),
    )?;
    let previous_was_invalid = previous_watermark.invalid_message_id;
    let requested_was_invalid = requested_watermark.invalid_message_id;
    let previous_seq = previous_watermark.seq.clone();
    let effective_watermark =
        max_effective_read_watermark(&previous_watermark, &requested_watermark);
    let effective_seq = effective_watermark.seq.clone();
    let requested_message_id = effective_watermark.message_id.clone();
    let state_write_seq = if requested_was_invalid {
        None
    } else {
        requested_watermark.seq.clone()
    };
    let state_write_message_id = if requested_was_invalid {
        None
    } else {
        requested_watermark.message_id.clone()
    };
    let read_at = input
        .read_watermark_at
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(now_utc_like);
    let advanced = watermark_seq_gt(effective_seq.as_deref(), previous_seq.as_deref());
    let updated_count = if let Some(seq) = effective_seq.as_deref() {
        mark_thread_messages_read_up_to_seq(connection, &owner_identity_id, &conversation_ids, seq)?
    } else if let Some(message_id) = requested_message_id.as_deref() {
        mark_thread_messages_read_up_to_message(
            connection,
            &owner_identity_id,
            &conversation_ids,
            message_id,
        )?
    } else {
        0
    };
    let read_state = super::read_state::ThreadReadStateRecord {
        owner_identity_id: owner_identity_id.clone(),
        owner_did,
        thread_scope: thread_key.thread_scope.clone(),
        thread_id: thread_key.thread_id.clone(),
        conversation_id: conversation_id.clone(),
        read_watermark_message_id: state_write_message_id.clone(),
        read_watermark_seq: state_write_seq.clone(),
        read_watermark_at: Some(read_at.clone()),
        pending_remote_ack: input.pending_remote_ack,
        remote_ack_at: (!input.pending_remote_ack).then(|| read_at.clone()),
        remote_state_version: current.and_then(|record| record.remote_state_version),
        updated_at: read_at.clone(),
    };
    if previous_was_invalid {
        super::read_state::replace_thread_read_state(connection, &read_state)?;
    } else {
        super::read_state::upsert_thread_read_state(connection, &read_state)?;
    }

    Ok(MarkThreadReadWatermarkResult {
        thread_scope: thread_key.thread_scope,
        thread_id: thread_key.thread_id,
        conversation_id,
        updated_count,
        previous_read_watermark_seq: previous_seq,
        read_watermark_seq: effective_seq,
        read_watermark_message_id: requested_message_id,
        read_watermark_at: Some(read_at),
        advanced,
        outbox_operation_id: None,
        remote_thread_key: None,
        remote_ack_applicable: true,
    })
}

#[cfg(feature = "sqlite")]
fn mark_messages_read_for_owner(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    message_ids: &[String],
) -> crate::ImResult<i64> {
    let ids = message_ids
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if ids.is_empty() {
        return Ok(0);
    }
    let owner_identity_id = normalize_owner_identity_id(owner_identity_id);
    required("owner_identity_id", &owner_identity_id)?;
    let identity_rows = resolve_message_identity_rows(connection, &owner_identity_id, &ids)?;
    let canonical_ids = canonical_message_ids_for_requests(&ids, &identity_rows);
    if canonical_ids.is_empty() {
        return Ok(0);
    }
    let placeholders = vec!["?"; canonical_ids.len()].join(",");
    let statement = format!(
        "UPDATE messages SET is_read = 1 WHERE {} AND msg_id IN ({placeholders})",
        owner_predicate()
    );
    let summary_rows = super::conversation_summaries::mark_read_summary_rows_for_message_ids(
        connection,
        &owner_identity_id,
        &canonical_ids.iter().map(String::as_str).collect::<Vec<_>>(),
    )?;
    let mut params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(canonical_ids.len() + 1);
    params.push(&owner_identity_id);
    for id in &canonical_ids {
        params.push(id);
    }
    let rows = connection
        .execute(&statement, params.as_slice())
        .map_err(super::local_state_unavailable)?;
    super::conversation_summaries::apply_mark_read_delta_or_rebuild(connection, &summary_rows)?;
    Ok(i64::try_from(rows).unwrap_or(i64::MAX))
}

#[cfg(feature = "sqlite")]
fn resolve_message_identity_rows(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    requested_ids: &[&str],
) -> crate::ImResult<BTreeMap<String, MessageIdentityRow>> {
    let requested_ids = requested_ids
        .iter()
        .filter_map(|value| non_empty(value).map(str::to_owned))
        .fold(Vec::new(), |mut values, value| {
            push_unique(&mut values, value);
            values
        });
    if requested_ids.is_empty() {
        return Ok(BTreeMap::new());
    }
    let owner_identity_id = required("owner_identity_id", owner_identity_id)?;
    let placeholders = vec!["?"; requested_ids.len()].join(",");
    let exact_statement = format!(
        r#"
SELECT msg_id
FROM messages
WHERE owner_identity_id = ?
  AND msg_id IN ({placeholders})"#
    );
    let mut params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(requested_ids.len() + 1);
    params.push(&owner_identity_id);
    for requested_id in &requested_ids {
        params.push(requested_id);
    }
    let mut result = BTreeMap::new();
    {
        let mut statement = connection
            .prepare(&exact_statement)
            .map_err(super::local_state_unavailable)?;
        let rows = statement
            .query_map(params.as_slice(), |row| {
                row.get::<_, Option<String>>("msg_id")
                    .map(|value| value.unwrap_or_default())
            })
            .map_err(super::local_state_unavailable)?;
        for row in rows {
            let msg_id = row.map_err(super::local_state_unavailable)?;
            let Some(msg_id) = non_empty(&msg_id).map(str::to_owned) else {
                continue;
            };
            result.insert(
                msg_id.clone(),
                MessageIdentityRow {
                    requested_msg_id: msg_id.clone(),
                    canonical_msg_id: msg_id,
                },
            );
        }
    }

    let alias_statement = format!(
        r#"
SELECT aliases.alias_msg_id,
       aliases.canonical_msg_id
FROM message_identity_aliases aliases
JOIN messages canonical
  ON canonical.owner_identity_id = aliases.owner_identity_id
 AND canonical.msg_id = aliases.canonical_msg_id
WHERE aliases.owner_identity_id = ?
  AND aliases.alias_msg_id IN ({placeholders})"#
    );
    let mut statement = connection
        .prepare(&alias_statement)
        .map_err(super::local_state_unavailable)?;
    let rows = statement
        .query_map(params.as_slice(), |row| {
            Ok(MessageIdentityRow {
                requested_msg_id: row
                    .get::<_, Option<String>>("alias_msg_id")?
                    .unwrap_or_default(),
                canonical_msg_id: row
                    .get::<_, Option<String>>("canonical_msg_id")?
                    .unwrap_or_default(),
            })
        })
        .map_err(super::local_state_unavailable)?;
    for row in rows {
        let row = row.map_err(super::local_state_unavailable)?;
        let Some(requested_msg_id) = non_empty(&row.requested_msg_id).map(str::to_owned) else {
            continue;
        };
        let Some(canonical_msg_id) = non_empty(&row.canonical_msg_id).map(str::to_owned) else {
            continue;
        };
        result
            .entry(requested_msg_id.clone())
            .or_insert(MessageIdentityRow {
                requested_msg_id,
                canonical_msg_id,
            });
    }
    Ok(result)
}

#[cfg(feature = "sqlite")]
fn canonical_message_ids_for_requests(
    requested_ids: &[&str],
    identity_rows: &BTreeMap<String, MessageIdentityRow>,
) -> Vec<String> {
    let mut canonical_ids = Vec::new();
    for requested_id in requested_ids {
        let Some(row) = identity_rows.get(*requested_id) else {
            continue;
        };
        push_unique(&mut canonical_ids, row.canonical_msg_id.clone());
    }
    canonical_ids
}

#[cfg(feature = "sqlite")]
fn canonical_message_id_for_request(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    requested_id: &str,
) -> crate::ImResult<String> {
    let identity_rows =
        resolve_message_identity_rows(connection, owner_identity_id, &[requested_id])?;
    Ok(identity_rows
        .get(requested_id)
        .map(|row| row.canonical_msg_id.clone())
        .unwrap_or_else(|| requested_id.trim().to_owned()))
}

#[cfg(feature = "sqlite")]
fn list_message_classification_rows(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    message_ids: &[String],
) -> crate::ImResult<Vec<MessageClassificationRow>> {
    let ids = message_ids
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let owner_identity_id = normalize_owner_identity_id(owner_identity_id);
    required("owner_identity_id", &owner_identity_id)?;
    let identity_rows = resolve_message_identity_rows(connection, &owner_identity_id, &ids)?;
    let canonical_ids = canonical_message_ids_for_requests(&ids, &identity_rows);
    if canonical_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = vec!["?"; canonical_ids.len()].join(",");
    let statement = format!(
        r#"
SELECT msg_id, group_id, group_did, content_type, metadata
FROM messages
WHERE {} AND msg_id IN ({placeholders})"#,
        owner_predicate()
    );
    let mut params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(canonical_ids.len() + 1);
    params.push(&owner_identity_id);
    for id in &canonical_ids {
        params.push(id);
    }
    let mut statement = connection
        .prepare(&statement)
        .map_err(super::local_state_unavailable)?;
    let rows = statement
        .query_map(params.as_slice(), |row| {
            Ok(MessageClassificationRow {
                requested_msg_id: String::new(),
                canonical_msg_id: row.get::<_, Option<String>>("msg_id")?.unwrap_or_default(),
                group_id: row
                    .get::<_, Option<String>>("group_id")?
                    .unwrap_or_default(),
                group_did: row
                    .get::<_, Option<String>>("group_did")?
                    .unwrap_or_default(),
                content_type: row
                    .get::<_, Option<String>>("content_type")?
                    .unwrap_or_default(),
                metadata: row
                    .get::<_, Option<String>>("metadata")?
                    .unwrap_or_default(),
            })
        })
        .map_err(super::local_state_unavailable)?;
    let mut by_canonical_id = BTreeMap::new();
    for row in rows {
        let row = row.map_err(super::local_state_unavailable)?;
        by_canonical_id.insert(row.canonical_msg_id.clone(), row);
    }
    let mut result = Vec::new();
    for requested_msg_id in &ids {
        let Some(identity_row) = identity_rows.get(*requested_msg_id) else {
            continue;
        };
        let Some(row) = by_canonical_id.get(&identity_row.canonical_msg_id) else {
            continue;
        };
        let mut row = row.clone();
        row.requested_msg_id = (*requested_msg_id).to_owned();
        result.push(row);
    }
    Ok(result)
}

#[cfg(feature = "sqlite")]
fn mark_thread_messages_read_up_to_seq(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    conversation_ids: &[String],
    seq: &str,
) -> crate::ImResult<i64> {
    let seq = normalize_read_watermark_seq(seq)?;
    if conversation_ids.is_empty() {
        return Ok(0);
    }
    let placeholders = vec!["?"; conversation_ids.len()].join(",");
    let statement = format!(
        r#"
UPDATE messages
SET is_read = 1
WHERE owner_identity_id = ?
  AND direction = 0
  AND is_read = 0
  AND hydration_state = 'hydrated'
  AND server_seq IS NOT NULL
  AND server_seq <= CAST(? AS INTEGER)
  AND COALESCE(NULLIF(conversation_id, ''), thread_id) IN ({placeholders})"#
    );
    let mut params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(conversation_ids.len() + 2);
    params.push(&owner_identity_id);
    params.push(&seq);
    for conversation_id in conversation_ids {
        params.push(conversation_id);
    }
    let rows = connection
        .execute(&statement, params.as_slice())
        .map_err(super::local_state_unavailable)?;
    if rows > 0 {
        rebuild_thread_summaries(connection, owner_identity_id, conversation_ids)?;
    }
    Ok(i64::try_from(rows).unwrap_or(i64::MAX))
}

#[cfg(feature = "sqlite")]
fn mark_thread_messages_read_up_to_message(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    conversation_ids: &[String],
    message_id: &str,
) -> crate::ImResult<i64> {
    let message_id = required("read_watermark_message_id", message_id)?;
    if conversation_ids.is_empty() {
        return Ok(0);
    }
    let message_id = canonical_message_id_for_request(connection, owner_identity_id, &message_id)?;
    let placeholders = vec!["?"; conversation_ids.len()].join(",");
    let lookup = format!(
        r#"
SELECT server_seq
FROM messages
WHERE owner_identity_id = ?
  AND msg_id = ?
  AND hydration_state = 'hydrated'
  AND server_seq IS NOT NULL
  AND COALESCE(NULLIF(conversation_id, ''), thread_id) IN ({placeholders})
LIMIT 1"#
    );
    let mut params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(conversation_ids.len() + 2);
    params.push(&owner_identity_id);
    params.push(&message_id);
    for conversation_id in conversation_ids {
        params.push(conversation_id);
    }
    let seq = connection
        .query_row(&lookup, params.as_slice(), |row| row.get::<_, i64>(0))
        .optional()
        .map_err(super::local_state_unavailable)?;
    if let Some(seq) = seq {
        return mark_thread_messages_read_up_to_seq(
            connection,
            owner_identity_id,
            conversation_ids,
            &seq.to_string(),
        );
    }
    let statement = format!(
        r#"
UPDATE messages
SET is_read = 1
WHERE owner_identity_id = ?
  AND direction = 0
  AND is_read = 0
  AND hydration_state = 'hydrated'
  AND msg_id = ?
  AND COALESCE(NULLIF(conversation_id, ''), thread_id) IN ({placeholders})"#
    );
    let mut params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(conversation_ids.len() + 2);
    params.push(&owner_identity_id);
    params.push(&message_id);
    for conversation_id in conversation_ids {
        params.push(conversation_id);
    }
    let rows = connection
        .execute(&statement, params.as_slice())
        .map_err(super::local_state_unavailable)?;
    if rows > 0 {
        rebuild_thread_summaries(connection, owner_identity_id, conversation_ids)?;
    }
    Ok(i64::try_from(rows).unwrap_or(i64::MAX))
}

#[cfg(feature = "sqlite")]
fn effective_read_watermark_for_thread(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    conversation_ids: &[String],
    message_id: Option<&str>,
    seq: Option<&str>,
) -> crate::ImResult<EffectiveReadWatermark> {
    let message_id = message_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let seq = seq.map(normalize_read_watermark_seq).transpose()?;
    if conversation_ids.is_empty() {
        return Ok(EffectiveReadWatermark::default());
    }
    if let Some(message_id) = message_id.as_deref() {
        if let Some(message_watermark) = read_watermark_for_message_id(
            connection,
            owner_identity_id,
            conversation_ids,
            message_id,
        )? {
            let _ = seq;
            return Ok(message_watermark);
        }
        return Ok(EffectiveReadWatermark {
            invalid_message_id: true,
            ..EffectiveReadWatermark::default()
        });
    }
    if let Some(seq) = seq {
        let message_id = read_watermark_message_id_for_seq(
            connection,
            owner_identity_id,
            conversation_ids,
            Some(&seq),
        )?;
        return Ok(EffectiveReadWatermark {
            message_id,
            seq: Some(seq),
            invalid_message_id: false,
        });
    }
    Ok(EffectiveReadWatermark::default())
}

#[cfg(feature = "sqlite")]
fn read_watermark_for_message_id(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    conversation_ids: &[String],
    message_id: &str,
) -> crate::ImResult<Option<EffectiveReadWatermark>> {
    let message_id = required("read_watermark_message_id", message_id)?;
    if conversation_ids.is_empty() {
        return Ok(None);
    }
    let message_id = canonical_message_id_for_request(connection, owner_identity_id, &message_id)?;
    let placeholders = vec!["?"; conversation_ids.len()].join(",");
    let statement = format!(
        r#"
SELECT msg_id, server_seq
FROM messages
WHERE owner_identity_id = ?
  AND msg_id = ?
  AND hydration_state = 'hydrated'
  AND COALESCE(NULLIF(conversation_id, ''), thread_id) IN ({placeholders})
LIMIT 1"#
    );
    let mut params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(conversation_ids.len() + 2);
    params.push(&owner_identity_id);
    params.push(&message_id);
    for conversation_id in conversation_ids {
        params.push(conversation_id);
    }
    connection
        .query_row(&statement, params.as_slice(), |row| {
            let msg_id = row.get::<_, Option<String>>("msg_id")?.unwrap_or_default();
            let seq = row
                .get::<_, Option<i64>>("server_seq")?
                .map(|value| value.to_string());
            Ok(EffectiveReadWatermark {
                message_id: non_empty(&msg_id).map(str::to_owned),
                seq,
                invalid_message_id: false,
            })
        })
        .optional()
        .map_err(super::local_state_unavailable)
}

#[cfg(feature = "sqlite")]
fn read_watermark_message_id_for_seq(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    conversation_ids: &[String],
    seq: Option<&str>,
) -> crate::ImResult<Option<String>> {
    let Some(seq) = seq else {
        return Ok(None);
    };
    let seq = normalize_read_watermark_seq(seq)?;
    if conversation_ids.is_empty() {
        return Ok(None);
    }
    let placeholders = vec!["?"; conversation_ids.len()].join(",");
    let statement = format!(
        r#"
SELECT msg_id
FROM messages
WHERE owner_identity_id = ?
  AND hydration_state = 'hydrated'
  AND server_seq IS NOT NULL
  AND server_seq <= CAST(? AS INTEGER)
  AND COALESCE(NULLIF(conversation_id, ''), thread_id) IN ({placeholders})
ORDER BY server_seq DESC, COALESCE(NULLIF(sent_at, ''), stored_at) DESC, msg_id DESC
LIMIT 1"#
    );
    let mut params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(conversation_ids.len() + 2);
    params.push(&owner_identity_id);
    params.push(&seq);
    for conversation_id in conversation_ids {
        params.push(conversation_id);
    }
    connection
        .query_row(&statement, params.as_slice(), |row| {
            row.get::<_, Option<String>>("msg_id")
        })
        .optional()
        .map(|value| {
            value
                .flatten()
                .and_then(|id| non_empty(&id).map(str::to_owned))
        })
        .map_err(super::local_state_unavailable)
}

#[cfg(feature = "sqlite")]
fn max_effective_read_watermark(
    left: &EffectiveReadWatermark,
    right: &EffectiveReadWatermark,
) -> EffectiveReadWatermark {
    if watermark_seq_gt(right.seq.as_deref(), left.seq.as_deref()) {
        return right.clone();
    }
    if watermark_seq_gt(left.seq.as_deref(), right.seq.as_deref()) {
        return left.clone();
    }
    if right.message_id.is_some() {
        return right.clone();
    }
    left.clone()
}

#[cfg(feature = "sqlite")]
fn mark_thread_messages_read_up_to_read_at(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    conversation_ids: &[String],
    read_at: &str,
) -> crate::ImResult<i64> {
    let read_at = required("read_watermark_at", read_at)?;
    if conversation_ids.is_empty() {
        return Ok(0);
    }
    let placeholders = vec!["?"; conversation_ids.len()].join(",");
    let statement = format!(
        r#"
UPDATE messages
SET is_read = 1
WHERE owner_identity_id = ?
  AND direction = 0
  AND is_read = 0
  AND hydration_state = 'hydrated'
  AND NULLIF(TRIM(COALESCE(sent_at, stored_at, '')), '') IS NOT NULL
  AND julianday(COALESCE(NULLIF(sent_at, ''), stored_at)) <= julianday(?)
  AND COALESCE(NULLIF(conversation_id, ''), thread_id) IN ({placeholders})"#
    );
    let mut params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(conversation_ids.len() + 2);
    params.push(&owner_identity_id);
    params.push(&read_at);
    for conversation_id in conversation_ids {
        params.push(conversation_id);
    }
    let rows = connection
        .execute(&statement, params.as_slice())
        .map_err(super::local_state_unavailable)?;
    if rows > 0 {
        rebuild_thread_summaries(connection, owner_identity_id, conversation_ids)?;
    }
    Ok(i64::try_from(rows).unwrap_or(i64::MAX))
}

#[cfg(feature = "sqlite")]
fn list_incoming_message_ids_up_to_seq(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    conversation_ids: &[String],
    seq: &str,
    limit: i64,
) -> crate::ImResult<ThreadUnreadMessageIds> {
    let seq = normalize_read_watermark_seq(seq)?;
    if conversation_ids.is_empty() {
        return Ok(ThreadUnreadMessageIds {
            message_ids: Vec::new(),
            truncated: false,
        });
    }
    let placeholders = vec!["?"; conversation_ids.len()].join(",");
    let statement = format!(
        r#"
SELECT msg_id
FROM messages
WHERE owner_identity_id = ?
  AND direction = 0
  AND hydration_state = 'hydrated'
  AND server_seq IS NOT NULL
  AND server_seq <= CAST(? AS INTEGER)
  AND NULLIF(TRIM(COALESCE(msg_id, '')), '') IS NOT NULL
  AND COALESCE(NULLIF(conversation_id, ''), thread_id) IN ({placeholders})
ORDER BY server_seq DESC, COALESCE(NULLIF(sent_at, ''), stored_at) DESC, msg_id DESC
LIMIT ?"#
    );
    let fetch_limit = limit.saturating_add(1);
    let mut params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(conversation_ids.len() + 3);
    params.push(&owner_identity_id);
    params.push(&seq);
    for conversation_id in conversation_ids {
        params.push(conversation_id);
    }
    params.push(&fetch_limit);
    collect_message_ids_with_truncation(connection, &statement, params.as_slice(), limit)
}

#[cfg(feature = "sqlite")]
fn list_incoming_message_ids_up_to_message(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    conversation_ids: &[String],
    message_id: &str,
    limit: i64,
) -> crate::ImResult<ThreadUnreadMessageIds> {
    let message_id = required("read_watermark_message_id", message_id)?;
    if conversation_ids.is_empty() {
        return Ok(ThreadUnreadMessageIds {
            message_ids: Vec::new(),
            truncated: false,
        });
    }
    let message_id = canonical_message_id_for_request(connection, owner_identity_id, &message_id)?;
    let placeholders = vec!["?"; conversation_ids.len()].join(",");
    let lookup = format!(
        r#"
SELECT server_seq
FROM messages
WHERE owner_identity_id = ?
  AND msg_id = ?
  AND hydration_state = 'hydrated'
  AND server_seq IS NOT NULL
  AND COALESCE(NULLIF(conversation_id, ''), thread_id) IN ({placeholders})
LIMIT 1"#
    );
    let mut params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(conversation_ids.len() + 2);
    params.push(&owner_identity_id);
    params.push(&message_id);
    for conversation_id in conversation_ids {
        params.push(conversation_id);
    }
    let seq = connection
        .query_row(&lookup, params.as_slice(), |row| row.get::<_, i64>(0))
        .optional()
        .map_err(super::local_state_unavailable)?;
    if let Some(seq) = seq {
        return list_incoming_message_ids_up_to_seq(
            connection,
            owner_identity_id,
            conversation_ids,
            &seq.to_string(),
            limit,
        );
    }
    let statement = format!(
        r#"
SELECT msg_id
FROM messages
WHERE owner_identity_id = ?
  AND direction = 0
  AND hydration_state = 'hydrated'
  AND msg_id = ?
  AND COALESCE(NULLIF(conversation_id, ''), thread_id) IN ({placeholders})
LIMIT ?"#
    );
    let fetch_limit = limit.saturating_add(1);
    let mut params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(conversation_ids.len() + 3);
    params.push(&owner_identity_id);
    params.push(&message_id);
    for conversation_id in conversation_ids {
        params.push(conversation_id);
    }
    params.push(&fetch_limit);
    collect_message_ids_with_truncation(connection, &statement, params.as_slice(), limit)
}

#[cfg(feature = "sqlite")]
fn collect_message_ids_with_truncation(
    connection: &rusqlite::Connection,
    statement: &str,
    params: &[&dyn rusqlite::ToSql],
    limit: i64,
) -> crate::ImResult<ThreadUnreadMessageIds> {
    let mut statement = connection
        .prepare(statement)
        .map_err(super::local_state_unavailable)?;
    let rows = statement
        .query_map(params, |row| {
            row.get::<_, Option<String>>("msg_id")
                .map(|value| value.unwrap_or_default())
        })
        .map_err(super::local_state_unavailable)?;
    let mut message_ids = Vec::new();
    for row in rows {
        let message_id = row.map_err(super::local_state_unavailable)?;
        push_unique(&mut message_ids, message_id.trim().to_owned());
    }
    let truncated = i64::try_from(message_ids.len()).unwrap_or(i64::MAX) > limit;
    if truncated {
        message_ids.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
    }
    Ok(ThreadUnreadMessageIds {
        message_ids,
        truncated,
    })
}

#[cfg(feature = "sqlite")]
fn rebuild_thread_summaries(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    conversation_ids: &[String],
) -> crate::ImResult<usize> {
    let mut touched = BTreeSet::new();
    for conversation_id in conversation_ids {
        if !conversation_id.trim().is_empty() {
            touched.insert((
                owner_identity_id.to_owned(),
                conversation_id.trim().to_owned(),
            ));
        }
    }
    super::conversation_summaries::rebuild_touched(connection, &touched)
}

#[cfg(feature = "sqlite")]
fn normalize_thread_mark_read_limit(limit: i64) -> i64 {
    limit.clamp(1, 500)
}

#[cfg(feature = "sqlite")]
fn normalize_local_history_limit(limit: i64) -> i64 {
    limit.clamp(1, 100)
}

#[cfg(feature = "sqlite")]
fn encode_local_history_cursor(record: &MessageRecord) -> Option<String> {
    let msg_id = non_empty(&record.msg_id)?;
    let timestamp = non_empty(&record.sent_at)
        .or_else(|| non_empty(&record.stored_at))
        .unwrap_or_default();
    Some(format!(
        "local-history:v1:{}:{}",
        base64_url_encode(timestamp),
        base64_url_encode(msg_id)
    ))
}

#[cfg(feature = "sqlite")]
fn decode_local_history_cursor(cursor: Option<&str>) -> crate::ImResult<Option<(String, String)>> {
    let Some(cursor) = cursor.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let Some(rest) = cursor.strip_prefix("local-history:v1:") else {
        return Err(crate::ImError::invalid_input(
            Some("cursor".to_owned()),
            "local history cursor must be produced by local_history",
        ));
    };
    let Some((timestamp, msg_id)) = rest.split_once(':') else {
        return Err(crate::ImError::invalid_input(
            Some("cursor".to_owned()),
            "local history cursor is malformed",
        ));
    };
    let timestamp = base64_url_decode(timestamp, "cursor")?;
    let msg_id = base64_url_decode(msg_id, "cursor")?;
    required("cursor.message_id", &msg_id)?;
    Ok(Some((timestamp, msg_id)))
}

#[cfg(feature = "sqlite")]
fn base64_url_encode(value: &str) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

    URL_SAFE_NO_PAD.encode(value.as_bytes())
}

#[cfg(feature = "sqlite")]
fn base64_url_decode(value: &str, field: &'static str) -> crate::ImResult<String> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

    let bytes = URL_SAFE_NO_PAD
        .decode(value.as_bytes())
        .map_err(|err| crate::ImError::invalid_input(Some(field.to_owned()), err.to_string()))?;
    String::from_utf8(bytes)
        .map_err(|err| crate::ImError::invalid_input(Some(field.to_owned()), err.to_string()))
}

#[cfg(feature = "sqlite")]
fn conversation_ids_for_thread_ref(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    owner_did: &str,
    thread: &crate::messages::ThreadRef,
) -> crate::ImResult<Vec<String>> {
    let mut ids = Vec::new();
    match thread {
        crate::messages::ThreadRef::Direct(peer) => {
            let peer = peer.as_str().trim();
            if !peer.is_empty() {
                for id in conversation_ids_for_direct_peer(
                    connection,
                    owner_identity_id,
                    owner_did,
                    peer,
                )? {
                    push_unique(&mut ids, id);
                }
            }
        }
        crate::messages::ThreadRef::Group(group) => {
            let group = group.as_str().trim();
            if !group.is_empty() {
                push_unique(&mut ids, group_conversation_id_for_ref(group));
            }
        }
        crate::messages::ThreadRef::Thread(thread) => {
            let raw = thread.as_str().trim();
            if !raw.is_empty() {
                push_unique(&mut ids, raw.to_owned());
                if raw.starts_with("dm:peer-scope:") {
                    for id in legacy_direct_conversation_ids_for_peer_scope_thread(
                        connection,
                        owner_identity_id,
                        owner_did,
                        raw,
                    )? {
                        push_unique(&mut ids, id);
                    }
                } else if raw.starts_with("dm:") {
                    let peer = raw.strip_prefix("dm:").unwrap_or(raw);
                    for id in conversation_ids_for_direct_peer(
                        connection,
                        owner_identity_id,
                        owner_did,
                        peer,
                    )? {
                        push_unique(&mut ids, id);
                    }
                }
                if let Some(alias) =
                    crate::internal::local_state::owner_scope::direct_conversation_id_from_thread_alias(
                        raw,
                        owner_did,
                    )
                {
                    push_unique(&mut ids, alias);
                }
                if let Some(group) = raw.strip_prefix("group:") {
                    push_unique(&mut ids, group_conversation_id_for_ref(group));
                }
            }
        }
    }
    Ok(ids)
}

#[cfg(feature = "sqlite")]
fn legacy_direct_conversation_ids_for_peer_scope_thread(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    owner_did: &str,
    peer_scope_conversation_id: &str,
) -> crate::ImResult<Vec<String>> {
    let mut ids = Vec::new();
    for candidate in peer_scope_direct_candidates(connection, owner_identity_id)? {
        if candidate.conversation_id.trim() != peer_scope_conversation_id.trim() {
            continue;
        }
        for legacy_id in legacy_direct_conversation_ids_for_peer_scope_candidate(
            connection,
            owner_identity_id,
            owner_did,
            &candidate,
        )? {
            push_unique(&mut ids, legacy_id);
        }
    }
    Ok(ids)
}

#[cfg(feature = "sqlite")]
fn conversation_ids_for_direct_peer(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    owner_did: &str,
    peer: &str,
) -> crate::ImResult<Vec<String>> {
    let peer = peer
        .trim()
        .strip_prefix("dm:")
        .unwrap_or_else(|| peer.trim())
        .trim();
    let mut ids = Vec::new();
    if peer.is_empty() {
        return Ok(ids);
    }
    push_unique(&mut ids, direct_conversation_id_for_peer_ref(peer));
    for id in peer_scope_direct_conversation_ids_matching_peer(
        connection,
        owner_identity_id,
        owner_did,
        peer,
    )? {
        push_unique(&mut ids, id);
    }
    if peer.starts_with("did:") {
        if let Some(full_handle) = did_full_handle(peer) {
            for id in legacy_direct_conversation_ids_matching_handle(
                connection,
                owner_identity_id,
                &full_handle,
            )? {
                push_unique(&mut ids, id);
            }
        }
    } else {
        for id in legacy_direct_conversation_ids_matching_handle(
            connection,
            owner_identity_id,
            &normalize_full_handle(peer),
        )? {
            push_unique(&mut ids, id);
        }
    }
    Ok(ids)
}

#[cfg(feature = "sqlite")]
struct ThreadReadStateKey {
    thread_scope: String,
    thread_id: String,
}

#[cfg(feature = "sqlite")]
fn thread_read_state_key(
    thread: &crate::messages::ThreadRef,
) -> crate::ImResult<ThreadReadStateKey> {
    match thread {
        crate::messages::ThreadRef::Direct(peer) => {
            let peer = required("thread.peer", peer.as_str())?;
            Ok(ThreadReadStateKey {
                thread_scope: "direct".to_owned(),
                thread_id: direct_conversation_id_for_peer_ref(&peer),
            })
        }
        crate::messages::ThreadRef::Group(group) => {
            let group = required("thread.group", group.as_str())?;
            Ok(ThreadReadStateKey {
                thread_scope: "group".to_owned(),
                thread_id: group_conversation_id_for_ref(&group),
            })
        }
        crate::messages::ThreadRef::Thread(thread) => {
            let thread = required("thread.thread_id", thread.as_str())?;
            let (scope, id) = if thread.starts_with("group:") {
                ("group", group_conversation_id_for_ref(&thread))
            } else if thread.starts_with("dm:") {
                ("direct", thread)
            } else {
                ("thread", thread)
            };
            Ok(ThreadReadStateKey {
                thread_scope: scope.to_owned(),
                thread_id: id,
            })
        }
    }
}

#[cfg(feature = "sqlite")]
fn direct_conversation_id_for_peer_ref(peer: &str) -> String {
    let peer = peer.trim();
    if let Some(conversation_id) = peer.strip_prefix("dm:") {
        return crate::internal::local_state::owner_scope::direct_conversation_id(conversation_id);
    }
    crate::internal::local_state::owner_scope::direct_conversation_id(peer)
}

#[cfg(feature = "sqlite")]
fn group_conversation_id_for_ref(group: &str) -> String {
    let group = group.trim();
    if group.starts_with("group:") {
        group.to_owned()
    } else {
        crate::internal::local_state::owner_scope::group_conversation_id(group)
    }
}

#[cfg(feature = "sqlite")]
#[derive(Debug, Clone, PartialEq, Eq)]
struct MessageIdentityRow {
    requested_msg_id: String,
    canonical_msg_id: String,
}

#[cfg(feature = "sqlite")]
#[derive(Debug, Clone, PartialEq, Eq)]
struct MessageClassificationRow {
    requested_msg_id: String,
    canonical_msg_id: String,
    group_id: String,
    group_did: String,
    content_type: String,
    metadata: String,
}

#[cfg(feature = "sqlite")]
impl MessageClassificationRow {
    fn is_group_message(&self) -> bool {
        !self.group_did.trim().is_empty() || !self.group_id.trim().is_empty()
    }

    fn is_local_mail_notification(&self) -> bool {
        if self.content_type.trim() == "mail.notification" {
            return true;
        }
        parse_metadata(&self.metadata)
            .get("source_kind")
            .and_then(serde_json::Value::as_str)
            .map(|value| value.trim() == "mail")
            .unwrap_or(false)
    }

    fn remote_mark_read_id(&self) -> String {
        let metadata = parse_metadata(&self.metadata);
        let is_authenticated_p5_v2_projection = metadata
            .get("p5_cache_profile")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            == Some(anp::direct_e2ee::DIRECT_E2EE_PROFILE_V2)
            && [
                "p5_cache_sender_did",
                "p5_cache_sender_device_id",
                "p5_cache_recipient_did",
                "p5_cache_recipient_device_id",
                "p5_cache_binding_digest",
            ]
            .into_iter()
            .all(|key| {
                metadata
                    .get(key)
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .is_some_and(|value| !value.is_empty())
            });
        if is_authenticated_p5_v2_projection {
            if let Some(raw_message_id) = metadata
                .get("raw_message_id")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                return raw_message_id.to_owned();
            }
        }
        self.requested_msg_id.clone()
    }
}

#[cfg(feature = "sqlite")]
fn parse_metadata(value: &str) -> serde_json::Map<String, serde_json::Value> {
    if value.trim().is_empty() {
        return serde_json::Map::new();
    }
    serde_json::from_str(value).unwrap_or_default()
}

#[cfg(feature = "sqlite")]
fn stable_conversation_id(record: &MessageRecord) -> String {
    if let Some(value) = non_empty(record.conversation_id.as_str()) {
        return value.to_owned();
    }
    if let Some(group) =
        non_empty(record.group_id.as_str()).or_else(|| non_empty(&record.group_did))
    {
        return crate::internal::local_state::owner_scope::group_conversation_id(group);
    }
    if is_mail_record(record) {
        if let Some(source) = record
            .thread_id
            .trim()
            .strip_prefix("mail:")
            .filter(|value| !value.trim().is_empty())
        {
            return crate::internal::local_state::owner_scope::mail_conversation_id(source);
        }
        return crate::internal::local_state::owner_scope::mail_conversation_id("inbox");
    }
    let owner_did = record.owner_did.trim();
    let peer = if record.sender_did.trim() != owner_did {
        record.sender_did.trim()
    } else {
        record.receiver_did.trim()
    };
    if !peer.is_empty() {
        return crate::internal::local_state::owner_scope::direct_conversation_id(peer);
    }
    if let Some(conversation_id) =
        crate::internal::local_state::owner_scope::direct_conversation_id_from_thread_alias(
            &record.thread_id,
            &record.owner_did,
        )
    {
        return conversation_id;
    }
    default_string(&record.thread_id, "")
}

#[cfg(feature = "sqlite")]
#[derive(Debug, Clone, PartialEq, Eq)]
struct PeerScopeDirectCandidate {
    conversation_id: String,
    owner_did: String,
    sender_did: String,
    receiver_did: String,
    metadata: String,
}

#[cfg(feature = "sqlite")]
fn peer_scope_direct_candidates(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
) -> crate::ImResult<Vec<PeerScopeDirectCandidate>> {
    let mut statement = connection
        .prepare(
            r#"
SELECT DISTINCT
    COALESCE(NULLIF(conversation_id, ''), thread_id) AS conversation_id,
    owner_did,
    sender_did,
    receiver_did,
    metadata
FROM messages
WHERE owner_identity_id = ?1
  AND COALESCE(NULLIF(conversation_id, ''), thread_id) LIKE 'dm:peer-scope:%'
  AND NULLIF(TRIM(COALESCE(group_id, '')), '') IS NULL
  AND NULLIF(TRIM(COALESCE(group_did, '')), '') IS NULL"#,
        )
        .map_err(super::local_state_unavailable)?;
    let rows = statement
        .query_map([owner_identity_id], |row| {
            Ok(PeerScopeDirectCandidate {
                conversation_id: row
                    .get::<_, Option<String>>("conversation_id")?
                    .unwrap_or_default(),
                owner_did: row
                    .get::<_, Option<String>>("owner_did")?
                    .unwrap_or_default(),
                sender_did: row
                    .get::<_, Option<String>>("sender_did")?
                    .unwrap_or_default(),
                receiver_did: row
                    .get::<_, Option<String>>("receiver_did")?
                    .unwrap_or_default(),
                metadata: row
                    .get::<_, Option<String>>("metadata")?
                    .unwrap_or_default(),
            })
        })
        .map_err(super::local_state_unavailable)?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row.map_err(super::local_state_unavailable)?);
    }
    Ok(result)
}

#[cfg(feature = "sqlite")]
fn peer_scope_direct_conversation_ids_matching_peer(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    owner_did: &str,
    peer: &str,
) -> crate::ImResult<Vec<String>> {
    let peer = peer.trim();
    if peer.is_empty() {
        return Ok(Vec::new());
    }
    let peer_did = peer.starts_with("did:").then(|| peer.to_owned());
    let normalized_handle = if peer.starts_with("did:") {
        did_full_handle(peer).unwrap_or_default()
    } else {
        normalize_full_handle(peer)
    };
    let mut ids = Vec::new();
    for candidate in peer_scope_direct_candidates(connection, owner_identity_id)? {
        if peer_scope_direct_candidate_matches_peer(
            &candidate,
            owner_did,
            peer_did.as_deref(),
            &normalized_handle,
        ) {
            push_unique(&mut ids, candidate.conversation_id);
        }
    }
    Ok(ids)
}

#[cfg(feature = "sqlite")]
fn peer_scope_direct_candidate_matches_peer(
    candidate: &PeerScopeDirectCandidate,
    owner_did: &str,
    peer_did: Option<&str>,
    normalized_handle: &str,
) -> bool {
    let metadata = parse_metadata(&candidate.metadata);
    if let Some(peer_did) = peer_did.map(str::trim).filter(|value| !value.is_empty()) {
        if peer_scope_candidate_dids(candidate, owner_did, &metadata)
            .iter()
            .any(|did| did == peer_did)
        {
            return true;
        }
    }

    let normalized_handle = normalized_handle.trim();
    if normalized_handle.is_empty() {
        return false;
    }
    if metadata
        .get("peer_full_handle")
        .and_then(serde_json::Value::as_str)
        .map(normalize_full_handle)
        .as_deref()
        == Some(normalized_handle)
    {
        return true;
    }
    peer_scope_candidate_dids(candidate, owner_did, &metadata)
        .iter()
        .any(|did| did_full_handle(did).as_deref() == Some(normalized_handle))
}

#[cfg(feature = "sqlite")]
fn peer_scope_candidate_dids(
    candidate: &PeerScopeDirectCandidate,
    owner_did: &str,
    metadata: &serde_json::Map<String, serde_json::Value>,
) -> Vec<String> {
    let mut dids = Vec::new();
    let effective_owner_did = owner_did.trim();
    let effective_owner_did = if effective_owner_did.is_empty() {
        candidate.owner_did.trim()
    } else {
        effective_owner_did
    };
    for did in [
        peer_current_did_from_metadata(metadata)
            .as_deref()
            .unwrap_or(""),
        candidate.sender_did.as_str(),
        candidate.receiver_did.as_str(),
    ] {
        let did = did.trim();
        if did.starts_with("did:") && did != effective_owner_did {
            push_unique(&mut dids, did.to_owned());
        }
    }
    dids
}

#[cfg(feature = "sqlite")]
fn legacy_direct_conversation_ids_for_peer_scope_candidate(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    owner_did: &str,
    candidate: &PeerScopeDirectCandidate,
) -> crate::ImResult<Vec<String>> {
    let metadata = parse_metadata(&candidate.metadata);
    let mut ids = Vec::new();
    for did in peer_scope_candidate_dids(candidate, owner_did, &metadata) {
        push_unique(
            &mut ids,
            crate::internal::local_state::owner_scope::direct_conversation_id(&did),
        );
    }
    if let Some(full_handle) = metadata
        .get("peer_full_handle")
        .and_then(serde_json::Value::as_str)
        .map(normalize_full_handle)
        .filter(|value| !value.is_empty())
    {
        for conversation_id in legacy_direct_conversation_ids_matching_handle(
            connection,
            owner_identity_id,
            &full_handle,
        )? {
            push_unique(&mut ids, conversation_id);
        }
    }
    Ok(ids)
}

#[cfg(feature = "sqlite")]
fn merge_legacy_direct_did_conversation(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    conversation_id: &str,
    record: &MessageRecord,
) -> crate::ImResult<Vec<String>> {
    let owner_identity_id = required("owner_identity_id", owner_identity_id)?;
    let conversation_id = required("conversation_id", conversation_id)?;
    if !conversation_id.starts_with("dm:peer-scope:")
        || !record.group_id.trim().is_empty()
        || !record.group_did.trim().is_empty()
    {
        return Ok(Vec::new());
    }
    if legacy_direct_merge_memo_contains(connection, &owner_identity_id, &conversation_id)? {
        return Ok(Vec::new());
    }
    let Some(route) =
        super::direct_peer_routes::get(connection, &owner_identity_id, &conversation_id)?
    else {
        return Ok(Vec::new());
    };
    let Some(peer_persona_id) = route
        .peer_persona_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(Vec::new());
    };
    super::conversation_registry::ensure(
        connection,
        &super::conversation_registry::ConversationRegistryRecord {
            owner_identity_id: owner_identity_id.clone(),
            owner_did: owner_did_for_record(record),
            conversation_id: conversation_id.clone(),
            thread_kind: "direct".to_owned(),
            thread_id: conversation_id.clone(),
            activity_at: default_string(&record.stored_at, &now_utc_like()),
        },
    )?;
    let legacy_conversation_ids = verified_legacy_direct_conversation_ids(
        connection,
        &owner_identity_id,
        peer_persona_id,
        &conversation_id,
    )?;
    if legacy_conversation_ids.is_empty() {
        mark_legacy_direct_merge_memo(connection, &owner_identity_id, &conversation_id, &[], 0)?;
        return Ok(Vec::new());
    }
    let verified_at = now_utc_like();
    for legacy_id in &legacy_conversation_ids {
        super::conversation_aliases::insert(
            connection,
            &super::conversation_aliases::ConversationAliasRecord {
                owner_identity_id: owner_identity_id.clone(),
                alias_kind: "legacy_direct_did".to_owned(),
                alias_conversation_id: legacy_id.clone(),
                canonical_conversation_id: conversation_id.clone(),
                source: "verified_peer_persona".to_owned(),
                verified_at: verified_at.clone(),
            },
        )?;
    }
    let placeholders = vec!["?"; legacy_conversation_ids.len()].join(",");
    let statement = format!(
        r#"
UPDATE messages
SET conversation_id = ?
WHERE owner_identity_id = ?
  AND NULLIF(TRIM(COALESCE(group_id, '')), '') IS NULL
  AND NULLIF(TRIM(COALESCE(group_did, '')), '') IS NULL
  AND COALESCE(NULLIF(conversation_id, ''), thread_id) IN ({placeholders})"#
    );
    let mut params: Vec<&dyn rusqlite::ToSql> =
        Vec::with_capacity(legacy_conversation_ids.len() + 2);
    params.push(&conversation_id);
    params.push(&owner_identity_id);
    for legacy_id in &legacy_conversation_ids {
        params.push(legacy_id);
    }
    let merged_rows = connection
        .execute(&statement, params.as_slice())
        .map_err(super::local_state_unavailable)?;
    mark_legacy_direct_merge_memo(
        connection,
        &owner_identity_id,
        &conversation_id,
        &legacy_conversation_ids,
        merged_rows as i64,
    )?;
    for legacy_id in &legacy_conversation_ids {
        super::conversation_registry::mark_merged(
            connection,
            &owner_identity_id,
            legacy_id,
            &conversation_id,
        )?;
    }
    Ok(legacy_conversation_ids)
}

#[cfg(feature = "sqlite")]
fn verified_legacy_direct_conversation_ids(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    peer_persona_id: &str,
    canonical_conversation_id: &str,
) -> crate::ImResult<Vec<String>> {
    let mut ids = Vec::new();
    for did in
        super::peer_identifiers::dids_for_persona(connection, owner_identity_id, peer_persona_id)?
    {
        let candidate = crate::internal::local_state::owner_scope::direct_conversation_id(&did);
        if candidate == canonical_conversation_id {
            continue;
        }
        let exists = connection
            .query_row(
                r#"SELECT 1
FROM (
    SELECT conversation_id FROM conversation_registry
    WHERE owner_identity_id = ?1
    UNION ALL
    SELECT COALESCE(NULLIF(conversation_id, ''), thread_id) FROM messages
    WHERE owner_identity_id = ?1
)
WHERE conversation_id = ?2 LIMIT 1"#,
                (owner_identity_id, candidate.as_str()),
                |_| Ok(()),
            )
            .optional()
            .map_err(super::local_state_unavailable)?
            .is_some();
        if exists {
            push_unique(&mut ids, candidate);
        }
    }
    Ok(ids)
}

#[cfg(feature = "sqlite")]
fn owner_did_for_record(record: &MessageRecord) -> String {
    record.owner_did.trim().to_owned()
}

#[cfg(feature = "sqlite")]
fn legacy_direct_merge_memo_contains(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    conversation_id: &str,
) -> crate::ImResult<bool> {
    ensure_legacy_direct_merge_memo(connection)?;
    let exists = connection
        .query_row(
            r#"
SELECT 1
FROM temp.legacy_direct_merge_memo
WHERE owner_identity_id = ?1 AND peer_scope_conversation_id = ?2
LIMIT 1"#,
            (owner_identity_id, conversation_id),
            |_| Ok(true),
        )
        .or_else(|err| match err {
            rusqlite::Error::QueryReturnedNoRows => Ok(false),
            other => Err(other),
        })
        .map_err(super::local_state_unavailable)?;
    Ok(exists)
}

#[cfg(feature = "sqlite")]
fn mark_legacy_direct_merge_memo(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    conversation_id: &str,
    legacy_conversation_ids: &[String],
    merged_rows: i64,
) -> crate::ImResult<()> {
    ensure_legacy_direct_merge_memo(connection)?;
    let updated_at = now_utc_like();
    connection
        .execute(
            r#"
INSERT INTO temp.legacy_direct_merge_memo
    (owner_identity_id, peer_scope_conversation_id, scan_attempts, merged_rows, updated_at)
VALUES (?1, ?2, 1, ?3, ?4)
ON CONFLICT(owner_identity_id, peer_scope_conversation_id) DO UPDATE SET
    scan_attempts = scan_attempts + 1,
    merged_rows = merged_rows + excluded.merged_rows,
    updated_at = excluded.updated_at"#,
            (owner_identity_id, conversation_id, merged_rows, updated_at),
        )
        .map_err(super::local_state_unavailable)?;
    for legacy_conversation_id in legacy_conversation_ids {
        connection
            .execute(
                r#"
INSERT OR IGNORE INTO temp.legacy_direct_merge_memo_ids
    (owner_identity_id, peer_scope_conversation_id, legacy_conversation_id)
VALUES (?1, ?2, ?3)"#,
                (owner_identity_id, conversation_id, legacy_conversation_id),
            )
            .map_err(super::local_state_unavailable)?;
    }
    Ok(())
}

#[cfg(feature = "sqlite")]
fn clear_legacy_direct_merge_memo_for_owner(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
) -> crate::ImResult<()> {
    ensure_legacy_direct_merge_memo(connection)?;
    connection
        .execute(
            "DELETE FROM temp.legacy_direct_merge_memo_ids WHERE owner_identity_id = ?1",
            [owner_identity_id],
        )
        .map_err(super::local_state_unavailable)?;
    connection
        .execute(
            "DELETE FROM temp.legacy_direct_merge_memo WHERE owner_identity_id = ?1",
            [owner_identity_id],
        )
        .map_err(super::local_state_unavailable)?;
    Ok(())
}

#[cfg(feature = "sqlite")]
fn ensure_legacy_direct_merge_memo(connection: &rusqlite::Connection) -> crate::ImResult<()> {
    // TEMP tables keep the memo scoped to the current local-state DB handle.
    // That avoids global state and schema migrations while removing repeated
    // legacy scans from the hot message upsert path.
    connection
        .execute_batch(
            r#"
CREATE TEMP TABLE IF NOT EXISTS legacy_direct_merge_memo (
    owner_identity_id TEXT NOT NULL,
    peer_scope_conversation_id TEXT NOT NULL,
    scan_attempts INTEGER NOT NULL DEFAULT 0,
    merged_rows INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (owner_identity_id, peer_scope_conversation_id)
);
CREATE TEMP TABLE IF NOT EXISTS legacy_direct_merge_memo_ids (
    owner_identity_id TEXT NOT NULL,
    peer_scope_conversation_id TEXT NOT NULL,
    legacy_conversation_id TEXT NOT NULL,
    PRIMARY KEY (owner_identity_id, peer_scope_conversation_id, legacy_conversation_id)
);
CREATE INDEX IF NOT EXISTS temp.idx_legacy_direct_merge_memo_ids_lookup
ON legacy_direct_merge_memo_ids(owner_identity_id, legacy_conversation_id);"#,
        )
        .map_err(super::local_state_unavailable)
}

#[cfg(feature = "sqlite")]
fn cached_peer_scope_conversation_id_for_legacy_direct(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    conversation_id: &str,
) -> crate::ImResult<Option<String>> {
    if !conversation_id.trim().starts_with("dm:did:wba:") {
        return Ok(None);
    }
    for alias_kind in ["legacy_direct_did", "verified_did"] {
        if let Some(canonical) = super::conversation_aliases::resolve(
            connection,
            owner_identity_id,
            alias_kind,
            conversation_id,
        )? {
            return Ok(Some(canonical));
        }
    }
    ensure_legacy_direct_merge_memo(connection)?;
    let cached_by_id = connection
        .query_row(
            r#"
SELECT peer_scope_conversation_id
FROM temp.legacy_direct_merge_memo_ids
WHERE owner_identity_id = ?1 AND legacy_conversation_id = ?2
LIMIT 1"#,
            (owner_identity_id, conversation_id),
            |row| row.get::<_, String>(0),
        )
        .map(Some)
        .or_else(|err| match err {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })
        .map_err(super::local_state_unavailable)?;
    Ok(cached_by_id)
}

#[cfg(feature = "sqlite")]
fn legacy_direct_peer_full_handle(record: &MessageRecord) -> Option<String> {
    parse_metadata(&record.metadata)
        .get("peer_full_handle")
        .and_then(serde_json::Value::as_str)
        .map(normalize_full_handle)
        .filter(|value| !value.is_empty())
}

#[cfg(feature = "sqlite")]
fn peer_scope_user_id_from_record(record: &MessageRecord) -> Option<String> {
    parse_metadata(&record.metadata)
        .get("peer_user_id")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

#[cfg(feature = "sqlite")]
fn legacy_direct_conversation_ids_for_peer(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    record: &MessageRecord,
) -> crate::ImResult<Vec<String>> {
    let metadata = parse_metadata(&record.metadata);
    let mut ids = Vec::new();
    for did in [
        peer_current_did_from_metadata(&metadata),
        direct_peer_did(record),
    ]
    .into_iter()
    .flatten()
    {
        push_unique(
            &mut ids,
            crate::internal::local_state::owner_scope::direct_conversation_id(&did),
        );
    }
    if let Some(full_handle) = metadata
        .get("peer_full_handle")
        .and_then(serde_json::Value::as_str)
        .map(normalize_full_handle)
        .filter(|value| !value.is_empty())
    {
        for did in peer_dids_for_handle(record, &full_handle) {
            push_unique(
                &mut ids,
                crate::internal::local_state::owner_scope::direct_conversation_id(&did),
            );
        }
        for conversation_id in legacy_direct_conversation_ids_matching_handle(
            connection,
            owner_identity_id,
            &full_handle,
        )? {
            push_unique(&mut ids, conversation_id);
        }
    }
    Ok(ids)
}

#[cfg(feature = "sqlite")]
fn peer_current_did_from_metadata(
    metadata: &serde_json::Map<String, serde_json::Value>,
) -> Option<String> {
    ["peer_current_did", "resolved_target_did"]
        .into_iter()
        .find_map(|key| {
            metadata
                .get(key)
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|value| value.starts_with("did:"))
                .map(str::to_string)
        })
}

#[cfg(feature = "sqlite")]
fn peer_current_did(record: &MessageRecord) -> Option<String> {
    let metadata = parse_metadata(&record.metadata);
    peer_current_did_from_metadata(&metadata)
}

#[cfg(feature = "sqlite")]
fn direct_peer_did(record: &MessageRecord) -> Option<String> {
    if !record.group_id.trim().is_empty() || !record.group_did.trim().is_empty() {
        return None;
    }
    let owner_did = record.owner_did.trim();
    let peer = if record.sender_did.trim() != owner_did {
        record.sender_did.trim()
    } else {
        record.receiver_did.trim()
    };
    (!peer.is_empty() && peer.starts_with("did:")).then(|| peer.to_string())
}

#[cfg(feature = "sqlite")]
fn peer_dids_for_handle(record: &MessageRecord, full_handle: &str) -> Vec<String> {
    let mut dids = Vec::new();
    let current_did = peer_current_did(record).unwrap_or_default();
    for did in [
        record.sender_did.as_str(),
        record.receiver_did.as_str(),
        current_did.as_str(),
    ] {
        let did = did.trim();
        if did_full_handle(did).as_deref() == Some(full_handle) {
            push_unique(&mut dids, did.to_string());
        }
    }
    dids
}

#[cfg(feature = "sqlite")]
fn legacy_direct_conversation_ids_matching_handle(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    full_handle: &str,
) -> crate::ImResult<Vec<String>> {
    let mut statement = connection
        .prepare(
            r#"
SELECT DISTINCT
    COALESCE(NULLIF(conversation_id, ''), thread_id) AS conversation_id,
    sender_did,
    receiver_did
FROM messages
WHERE owner_identity_id = ?1
  AND NULLIF(TRIM(COALESCE(group_id, '')), '') IS NULL
  AND NULLIF(TRIM(COALESCE(group_did, '')), '') IS NULL
  AND COALESCE(NULLIF(conversation_id, ''), thread_id) LIKE 'dm:did:wba:%'"#,
        )
        .map_err(super::local_state_unavailable)?;
    let rows = statement
        .query_map([owner_identity_id], |row| {
            Ok((
                row.get::<_, Option<String>>("conversation_id")?
                    .unwrap_or_default(),
                row.get::<_, Option<String>>("sender_did")?
                    .unwrap_or_default(),
                row.get::<_, Option<String>>("receiver_did")?
                    .unwrap_or_default(),
            ))
        })
        .map_err(super::local_state_unavailable)?;
    let mut ids = Vec::new();
    for row in rows {
        let (conversation_id, sender_did, receiver_did) =
            row.map_err(super::local_state_unavailable)?;
        let conversation_did = did_from_direct_conversation_id(&conversation_id);
        if [
            conversation_did.as_deref(),
            Some(sender_did.as_str()),
            Some(receiver_did.as_str()),
        ]
        .into_iter()
        .flatten()
        .any(|did| did_full_handle(did).as_deref() == Some(full_handle))
        {
            push_unique(&mut ids, conversation_id);
        }
    }
    Ok(ids)
}

#[cfg(feature = "sqlite")]
fn did_from_direct_conversation_id(conversation_id: &str) -> Option<String> {
    conversation_id
        .trim()
        .strip_prefix("dm:")
        .map(str::trim)
        .filter(|value| value.starts_with("did:"))
        .map(str::to_string)
}

#[cfg(feature = "sqlite")]
fn did_full_handle(did: &str) -> Option<String> {
    let mut parts = did.trim().strip_prefix("did:wba:")?.split(':');
    let domain = parts.next()?.trim();
    if domain.is_empty() {
        return None;
    }
    let path_parts = parts
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .take_while(|part| !part.starts_with("e1"))
        .collect::<Vec<_>>();
    let first_path = path_parts.first().copied()?;
    let local = match first_path {
        "user" | "users" => path_parts.get(1).copied()?,
        "agent" | "agents" => path_parts.last().copied()?,
        "group" | "groups" => return None,
        other => other,
    };
    if local.is_empty() || local.starts_with("e1") {
        return None;
    }
    Some(normalize_full_handle(&format!("{local}.{domain}")))
}

#[cfg(feature = "sqlite")]
fn normalize_full_handle(value: &str) -> String {
    value.trim().trim_end_matches('.').to_ascii_lowercase()
}

#[cfg(feature = "sqlite")]
fn push_unique(values: &mut Vec<String>, value: String) {
    if !value.trim().is_empty() && !values.iter().any(|known| known == &value) {
        values.push(value);
    }
}

#[cfg(feature = "sqlite")]
fn is_mail_record(record: &MessageRecord) -> bool {
    if record.content_type.trim() == "mail.notification" {
        return true;
    }
    parse_metadata(&record.metadata)
        .get("source_kind")
        .and_then(serde_json::Value::as_str)
        .map(|value| value.trim() == "mail")
        .unwrap_or(false)
}

#[cfg(feature = "sqlite")]
fn non_empty(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

#[cfg(feature = "sqlite")]
fn normalize_read_watermark_seq(value: &str) -> crate::ImResult<String> {
    crate::internal::local_state::sync_state::normalize_decimal_seq(value).map_err(|_| {
        crate::ImError::invalid_input(
            Some("read_watermark_seq".to_owned()),
            format!("read_watermark_seq must be a non-negative decimal string, got {value:?}"),
        )
    })
}

#[cfg(feature = "sqlite")]
fn watermark_seq_gt(left: Option<&str>, right: Option<&str>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => {
            parse_read_watermark_seq(left).unwrap_or(0)
                > parse_read_watermark_seq(right).unwrap_or(0)
        }
        (Some(_), None) => true,
        _ => false,
    }
}

#[cfg(feature = "sqlite")]
fn max_watermark_seq(left: Option<&str>, right: Option<&str>) -> Option<String> {
    match (left, right) {
        (Some(left), Some(right)) => {
            if parse_read_watermark_seq(left).unwrap_or(0)
                >= parse_read_watermark_seq(right).unwrap_or(0)
            {
                Some(left.to_owned())
            } else {
                Some(right.to_owned())
            }
        }
        (Some(value), None) | (None, Some(value)) => Some(value.to_owned()),
        (None, None) => None,
    }
}

#[cfg(feature = "sqlite")]
fn parse_read_watermark_seq(value: &str) -> crate::ImResult<i64> {
    let normalized = normalize_read_watermark_seq(value)?;
    normalized.parse::<i64>().map_err(|err| {
        crate::ImError::invalid_input(Some("read_watermark_seq".to_owned()), err.to_string())
    })
}

#[cfg(feature = "sqlite")]
fn normalize_owner_identity_id(value: &str) -> String {
    value.trim().to_string()
}

#[cfg(feature = "sqlite")]
fn owner_predicate() -> &'static str {
    "owner_identity_id = ?"
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
fn nullable_text(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
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

    #[test]
    fn decrypted_secure_lookup_uses_unresolved_durable_projection() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let record = MessageRecord {
            msg_id: "msg-secure-unresolved".to_owned(),
            owner_identity_id: "owner-id".to_owned(),
            owner_did: "did:example:owner".to_owned(),
            conversation_id: "dm:did:example:peer".to_owned(),
            thread_id: "dm:did:example:peer".to_owned(),
            direction: 0,
            sender_did: "did:example:peer".to_owned(),
            receiver_did: "did:example:owner".to_owned(),
            content_type: "text/plain".to_owned(),
            content: "decrypted text".to_owned(),
            is_e2ee: true,
            metadata: r#"{"decryption_state":"decrypted"}"#.to_owned(),
            ..MessageRecord::default()
        };
        let outcome = super::super::inbound_resolution_backlog::ingest_remote_messages(
            &db,
            std::slice::from_ref(&record),
            "remote_history",
        )
        .unwrap();
        assert_eq!(outcome.stored_messages, 0);
        assert_eq!(outcome.backlogged_messages, 1);

        let cached = list_decrypted_secure_messages_for_owner_identity(
            &db,
            "owner-id",
            &[record.msg_id.clone()],
        )
        .unwrap();

        assert_eq!(cached, vec![record]);
    }

    fn wire_identity_record() -> MessageRecord {
        MessageRecord {
            msg_id: "wire-message-1".to_owned(),
            owner_identity_id: "owner-id".to_owned(),
            owner_did: "did:example:owner".to_owned(),
            conversation_id: "dm:peer-scope:v1:first".to_owned(),
            thread_id: "dm:peer-scope:v1:first".to_owned(),
            direction: 1,
            sender_did: "did:example:owner".to_owned(),
            receiver_did: "did:example:peer".to_owned(),
            content_type: "text/plain".to_owned(),
            content: "hello".to_owned(),
            server_seq: Some(7),
            stored_at: "2026-07-14T00:00:00Z".to_owned(),
            ..MessageRecord::default()
        }
        .with_resolved_wire_thread("direct", "did:example:peer")
    }

    fn wire_identity_message(
        thread: crate::messages::ThreadRef,
        sender_did: &str,
        receiver_did: Option<&str>,
        group_did: Option<&str>,
    ) -> crate::messages::Message {
        crate::messages::Message {
            id: crate::ids::MessageId::parse("wire-message-dto-1").unwrap(),
            thread,
            direction: crate::messages::MessageDirection::Incoming,
            sender: crate::ids::PeerRef::parse(sender_did, "").unwrap(),
            receiver: receiver_did.map(|did| crate::ids::PeerRef::parse(did, "").unwrap()),
            group: group_did.map(|did| crate::ids::GroupRef::parse(did).unwrap()),
            body: crate::messages::MessageBodyView::Payload {
                payload: serde_json::json!({"type": "wire-identity-test"}),
            },
            sent_at: None,
            received_at: None,
            metadata: crate::messages::MessageMetadata::default(),
        }
    }

    #[test]
    fn canonical_direct_projection_keeps_peer_did_as_wire_identity() {
        let canonical_thread = crate::messages::ThreadRef::Thread(
            crate::ids::ThreadId::parse("dm:peer-scope:v1:canonical").unwrap(),
        );
        for (sender_did, receiver_did) in [
            ("did:example:peer", "did:example:owner"),
            ("did:example:owner", "did:example:peer"),
        ] {
            let message = wire_identity_message(
                canonical_thread.clone(),
                sender_did,
                Some(receiver_did),
                None,
            );
            let record = MessageRecord::default()
                .with_wire_identity_from_message("did:example:owner", &message);

            assert_eq!(record.wire_thread_kind, "direct");
            assert_eq!(record.wire_thread_ref, "did:example:peer");
            assert_eq!(record.wire_identity_resolution_state, "resolved");
        }
    }

    #[test]
    fn raw_thread_and_group_projection_keep_their_protocol_identity() {
        let raw_thread = wire_identity_message(
            crate::messages::ThreadRef::Thread(
                crate::ids::ThreadId::parse("compatibility-thread-1").unwrap(),
            ),
            "did:example:peer",
            Some("did:example:owner"),
            None,
        );
        let raw_record = MessageRecord::default()
            .with_wire_identity_from_message("did:example:owner", &raw_thread);
        assert_eq!(raw_record.wire_thread_kind, "thread");
        assert_eq!(raw_record.wire_thread_ref, "compatibility-thread-1");

        let group = wire_identity_message(
            crate::messages::ThreadRef::Thread(
                crate::ids::ThreadId::parse("canonical-group-projection").unwrap(),
            ),
            "did:example:peer",
            None,
            Some("did:example:group"),
        );
        let group_record =
            MessageRecord::default().with_wire_identity_from_message("did:example:owner", &group);
        assert_eq!(group_record.wire_thread_kind, "group");
        assert_eq!(group_record.wire_thread_ref, "did:example:group");
    }

    #[test]
    fn canonical_reprojection_never_rewrites_wire_identity() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let original = wire_identity_record();
        upsert_message(&db, &original).unwrap();

        let mut reprojected = original;
        reprojected.conversation_id = "dm:peer-scope:v1:canonical".to_owned();
        reprojected.thread_id = "dm:peer-scope:v1:canonical".to_owned();
        upsert_message(&db, &reprojected).unwrap();

        let stored: (String, String, String, String, String, i64) = db
            .query_row(
                r#"SELECT conversation_id, thread_id, wire_thread_kind, wire_thread_ref,
                          wire_identity_resolution_state, server_seq
FROM messages WHERE owner_identity_id = 'owner-id' AND msg_id = 'wire-message-1'"#,
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(stored.0, "dm:peer-scope:v1:canonical");
        assert_eq!(stored.1, "dm:peer-scope:v1:first");
        assert_eq!(stored.2, "direct");
        assert_eq!(stored.3, "did:example:peer");
        assert_eq!(stored.4, "resolved");
        assert_eq!(stored.5, 7);
    }

    #[test]
    fn same_message_id_with_different_wire_facts_fails_closed() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let original = wire_identity_record();
        upsert_message(&db, &original).unwrap();

        let mut conflicting_thread = original.clone();
        conflicting_thread.wire_thread_ref = "did:example:other".to_owned();
        assert!(matches!(
            upsert_message(&db, &conflicting_thread).unwrap_err(),
            crate::ImError::MessageWireIdentityConflict { .. }
        ));

        let mut conflicting_sequence = original;
        conflicting_sequence.server_seq = Some(8);
        assert!(matches!(
            upsert_message(&db, &conflicting_sequence).unwrap_err(),
            crate::ImError::MessageWireIdentityConflict { .. }
        ));
    }

    #[test]
    fn local_state_message_upsert_rolls_back_all_projections_on_post_write_failure() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        crate::internal::local_state::groups::upsert_group(
            &db,
            crate::internal::local_state::groups::GroupRecord {
                owner_identity_id: "owner-id".to_owned(),
                owner_did: "did:example:owner".to_owned(),
                group_id: "group-1".to_owned(),
                group_did: "did:example:group-1".to_owned(),
                my_role: "owner".to_owned(),
                membership_status: "active".to_owned(),
                ..crate::internal::local_state::groups::GroupRecord::default()
            },
        )
        .unwrap();
        let baseline_counts = [
            "messages",
            "conversation_summaries",
            "conversation_registry",
        ]
        .map(|table| {
            db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap()
        });
        let error = upsert_message(
            &db,
            &MessageRecord {
                msg_id: "bad-rebind".to_owned(),
                owner_identity_id: "owner-id".to_owned(),
                owner_did: "did:example:owner".to_owned(),
                conversation_id: "group:group-1".to_owned(),
                thread_id: "group:group-1".to_owned(),
                sender_did: "did:example:owner".to_owned(),
                group_id: "group-1".to_owned(),
                group_did: "did:example:group-1".to_owned(),
                content_type: "application/json".to_owned(),
                content: serde_json::json!({
                    "type": "member_credential_rebound",
                    "group_did": "did:example:group-1",
                    "subject_handle": "peer.awiki.test",
                    "previous_subject_did": "did:example:peer-old",
                    "subject_did": "did:example:peer-new",
                    "handle_binding_generation": "invalid",
                    "group_state_version": "1"
                })
                .to_string(),
                stored_at: "2026-07-14T00:00:00Z".to_owned(),
                ..MessageRecord::default()
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("generation"));
        for (table, baseline) in [
            "messages",
            "conversation_summaries",
            "conversation_registry",
        ]
        .into_iter()
        .zip(baseline_counts)
        {
            let count: i64 = db
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(
                count, baseline,
                "{table} must roll back with the failed upsert"
            );
        }
    }
    use rusqlite::Connection;

    #[test]
    fn local_state_messages_classifies_mark_read_ids_by_owner_identity() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        db.execute(
            r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, conversation_id, thread_id, direction, sender_did, receiver_did, content_type, content, stored_at)
VALUES (?1, ?2, ?3, ?4, ?4, 0, ?5, ?3, 'text/plain', 'direct', '2026-05-21T00:00:00Z')"#,
            (
                "direct-1",
                "alice-id",
                "did:example:alice",
                "dm:did:example:bob",
                "did:example:bob",
            ),
        )
        .unwrap();
        db.execute(
            r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, conversation_id, thread_id, direction, group_id, group_did, content_type, content, stored_at)
VALUES (?1, ?2, ?3, ?4, ?4, 0, ?5, ?5, 'text/plain', 'group', '2026-05-21T00:00:00Z')"#,
            (
                "group-1",
                "alice-id",
                "did:example:alice",
                "group:one",
                "did:example:group",
            ),
        )
        .unwrap();
        db.execute(
            r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, conversation_id, thread_id, direction, content_type, content, stored_at, metadata)
VALUES (?1, ?2, ?3, ?4, ?4, 0, 'mail.notification', 'mail', '2026-05-21T00:00:00Z', ?5)"#,
            (
                "mail-1",
                "alice-id",
                "did:example:alice",
                "mail:inbox",
                r#"{"source_kind":"mail"}"#,
            ),
        )
        .unwrap();

        let classified = classify_mark_read_ids_for_owner_identity(
            &db,
            "alice-id",
            "did:example:alice",
            &[
                "direct-1".to_string(),
                "group-1".to_string(),
                "mail-1".to_string(),
                "missing-1".to_string(),
            ],
        )
        .unwrap();

        assert_eq!(classified.direct_ids, vec!["direct-1", "missing-1"]);
        assert_eq!(classified.remote_direct_ids, vec!["direct-1", "missing-1"]);
        assert_eq!(classified.group_ids, vec!["group-1"]);
        assert_eq!(classified.local_only_ids, vec!["mail-1"]);
    }

    #[test]
    fn local_state_messages_mark_read_respects_owner() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        for (identity, owner) in [("owner-1-id", "did:owner-1"), ("owner-2-id", "did:owner-2")] {
            db.execute(
                r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, thread_id, direction, content_type, content, stored_at, is_read)
VALUES (?1, ?2, ?3, 'thread', 0, 'text/plain', 'hello', '2026-05-21T00:00:00Z', 0)"#,
                ("shared", identity, owner),
            )
            .unwrap();
        }

        super::super::conversation_summaries::rebuild_owner(&db, "owner-1-id").unwrap();
        super::super::conversation_summaries::rebuild_owner(&db, "owner-2-id").unwrap();

        let updated = mark_messages_read_for_owner_identity(
            &db,
            "owner-1-id",
            "did:owner-1",
            &["shared".to_string()],
        )
        .unwrap();

        assert_eq!(updated, 1);
        assert_eq!(is_read(&db, "owner-1-id"), 1);
        assert_eq!(is_read(&db, "owner-2-id"), 0);
        assert_eq!(summary_unread(&db, "owner-1-id", "thread"), 0);
        assert_eq!(summary_unread(&db, "owner-2-id", "thread"), 1);
    }

    #[test]
    fn local_state_owner_mark_read_uses_identity_without_legacy_did_fallback() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        db.execute(
            r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, thread_id, direction, content_type, content, stored_at, is_read)
VALUES ('stable', 'alice-id', 'did:alice-old', 'thread', 0, 'text/plain', 'hello', '2026-05-21T00:00:00Z', 0)"#,
            [],
        )
        .unwrap();
        db.execute(
            r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, thread_id, direction, content_type, content, stored_at, is_read)
VALUES ('same-did-other-id', 'mallory-id', 'did:alice-new', 'thread', 0, 'text/plain', 'hello', '2026-05-21T00:00:00Z', 0)"#,
            [],
        )
        .unwrap();
        db.execute(
            r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, thread_id, direction, content_type, content, stored_at, is_read)
VALUES ('other', 'bob-id', 'did:alice-new', 'thread', 0, 'text/plain', 'hello', '2026-05-21T00:00:00Z', 0)"#,
            [],
        )
        .unwrap();

        let updated = mark_messages_read_for_owner_identity(
            &db,
            "alice-id",
            "did:alice-new",
            &[
                "stable".to_string(),
                "same-did-other-id".to_string(),
                "other".to_string(),
            ],
        )
        .unwrap();

        assert_eq!(updated, 1);
        assert_eq!(read_by_msg_id(&db, "stable"), 1);
        assert_eq!(read_by_msg_id(&db, "same-did-other-id"), 0);
        assert_eq!(read_by_msg_id(&db, "other"), 0);
    }

    #[test]
    fn local_state_lists_unread_incoming_direct_ids_for_thread() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        seed_message_row(
            &db,
            "direct-old",
            "alice-id",
            "did:example:alice",
            "dm:did:example:bob",
            0,
            "did:example:bob",
            "did:example:alice",
            "",
            0,
            "2026-05-21T00:00:00Z",
        );
        seed_message_row(
            &db,
            "direct-new",
            "alice-id",
            "did:example:alice",
            "dm:did:example:bob",
            0,
            "did:example:bob",
            "did:example:alice",
            "",
            0,
            "2026-05-22T00:00:00Z",
        );
        seed_message_row(
            &db,
            "direct-outgoing",
            "alice-id",
            "did:example:alice",
            "dm:did:example:bob",
            1,
            "did:example:alice",
            "did:example:bob",
            "",
            0,
            "2026-05-23T00:00:00Z",
        );
        seed_message_row(
            &db,
            "direct-read",
            "alice-id",
            "did:example:alice",
            "dm:did:example:bob",
            0,
            "did:example:bob",
            "did:example:alice",
            "",
            1,
            "2026-05-24T00:00:00Z",
        );
        seed_message_row(
            &db,
            "other-owner",
            "mallory-id",
            "did:example:mallory",
            "dm:did:example:bob",
            0,
            "did:example:bob",
            "did:example:mallory",
            "",
            0,
            "2026-05-25T00:00:00Z",
        );

        let result = list_unread_incoming_message_ids_for_owner_identity(
            &db,
            "alice-id",
            "did:example:alice",
            &crate::messages::ThreadRef::Direct(
                crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
            ),
            10,
        )
        .unwrap();

        assert_eq!(result.message_ids, vec!["direct-new", "direct-old"]);
        assert!(!result.truncated);
    }

    #[test]
    fn local_state_lists_watermark_fallback_ids_even_after_local_read() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let owner_identity_id = "alice-id";
        let owner_did = "did:example:alice";
        let conversation_id = "dm:did:example:bob";
        for (msg_id, seq) in [("m1", 1), ("m2", 2), ("m3", 3)] {
            upsert_message(
                &db,
                &MessageRecord {
                    msg_id: msg_id.to_owned(),
                    owner_identity_id: owner_identity_id.to_owned(),
                    owner_did: owner_did.to_owned(),
                    conversation_id: conversation_id.to_owned(),
                    thread_id: conversation_id.to_owned(),
                    direction: 0,
                    sender_did: "did:example:bob".to_owned(),
                    receiver_did: owner_did.to_owned(),
                    content_type: "text/plain".to_owned(),
                    content: msg_id.to_owned(),
                    server_seq: Some(seq),
                    stored_at: format!("2026-06-27T00:00:0{seq}Z"),
                    ..MessageRecord::default()
                },
            )
            .unwrap();
        }
        mark_thread_read_watermark_for_owner_identity(
            &db,
            owner_identity_id,
            owner_did,
            MarkThreadReadWatermarkInput {
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                ),
                read_watermark_message_id: Some("m2".to_owned()),
                read_watermark_seq: Some("2".to_owned()),
                read_watermark_at: Some("2026-06-27T00:01:00Z".to_owned()),
                pending_remote_ack: true,
            },
        )
        .unwrap();

        let result = list_incoming_message_ids_up_to_watermark_for_owner_identity(
            &db,
            owner_identity_id,
            owner_did,
            &crate::messages::ThreadRef::Direct(
                crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
            ),
            Some("m2"),
            Some("2"),
            1,
        )
        .unwrap();

        assert_eq!(read_by_msg_id(&db, "m1"), 1);
        assert_eq!(read_by_msg_id(&db, "m2"), 1);
        assert_eq!(read_by_msg_id(&db, "m3"), 0);
        assert_eq!(result.message_ids, vec!["m2"]);
        assert!(result.truncated);
    }

    #[test]
    fn local_state_lists_group_unread_ids_and_reports_truncation() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        seed_message_row(
            &db,
            "group-old",
            "alice-id",
            "did:example:alice",
            "group:did:example:group",
            0,
            "did:example:bob",
            "",
            "did:example:group",
            0,
            "2026-05-21T00:00:00Z",
        );
        seed_message_row(
            &db,
            "group-new",
            "alice-id",
            "did:example:alice",
            "group:did:example:group",
            0,
            "did:example:carol",
            "",
            "did:example:group",
            0,
            "2026-05-22T00:00:00Z",
        );

        let result = list_unread_incoming_message_ids_for_owner_identity(
            &db,
            "alice-id",
            "did:example:alice",
            &crate::messages::ThreadRef::Group(
                crate::ids::GroupRef::parse("did:example:group").unwrap(),
            ),
            1,
        )
        .unwrap();

        assert_eq!(result.message_ids, vec!["group-new"]);
        assert!(result.truncated);
    }

    #[test]
    fn local_state_thread_after_max_server_seq_is_owner_and_thread_scoped() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        for (msg_id, owner_identity_id, conversation_id, group_did, server_seq) in [
            ("direct-old", "alice-id", "dm:did:example:bob", "", Some(7)),
            ("direct-new", "alice-id", "dm:did:example:bob", "", Some(42)),
            (
                "direct-other-thread",
                "alice-id",
                "dm:did:example:carol",
                "",
                Some(100),
            ),
            (
                "direct-other-owner",
                "mallory-id",
                "dm:did:example:bob",
                "",
                Some(99),
            ),
            (
                "group-new",
                "alice-id",
                "group:did:example:group",
                "did:example:group",
                Some(55),
            ),
        ] {
            upsert_message(
                &db,
                &MessageRecord {
                    msg_id: msg_id.to_owned(),
                    owner_identity_id: owner_identity_id.to_owned(),
                    owner_did: "did:example:alice".to_owned(),
                    conversation_id: conversation_id.to_owned(),
                    thread_id: conversation_id.to_owned(),
                    direction: 0,
                    sender_did: "did:example:bob".to_owned(),
                    receiver_did: "did:example:alice".to_owned(),
                    group_id: group_did.to_owned(),
                    group_did: group_did.to_owned(),
                    content_type: "text/plain".to_owned(),
                    content: "hello".to_owned(),
                    server_seq,
                    stored_at: "2026-06-27T00:00:00Z".to_owned(),
                    ..MessageRecord::default()
                },
            )
            .unwrap();
        }

        let direct = max_server_seq_for_thread_ref_for_owner_identity(
            &db,
            "alice-id",
            "did:example:alice",
            &crate::messages::ThreadRef::Direct(
                crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
            ),
        )
        .unwrap();
        let group = max_server_seq_for_thread_ref_for_owner_identity(
            &db,
            "alice-id",
            "did:example:alice",
            &crate::messages::ThreadRef::Group(
                crate::ids::GroupRef::parse("did:example:group").unwrap(),
            ),
        )
        .unwrap();
        let missing = max_server_seq_for_thread_ref_for_owner_identity(
            &db,
            "alice-id",
            "did:example:alice",
            &crate::messages::ThreadRef::Direct(
                crate::ids::PeerRef::parse("did:example:missing", "").unwrap(),
            ),
        )
        .unwrap();

        assert_eq!(direct, Some(42));
        assert_eq!(group, Some(55));
        assert_eq!(missing, None);
    }

    #[test]
    fn local_state_catch_up_cursor_stops_before_earliest_hydration_gap() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let thread = crate::messages::ThreadRef::Direct(
            crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
        );
        for (msg_id, owner_identity_id, server_seq, hydration_state) in [
            ("complete-1", "alice-id", 1, MessageHydrationState::Hydrated),
            ("gap-2", "alice-id", 2, MessageHydrationState::Discovered),
            ("complete-3", "alice-id", 3, MessageHydrationState::Hydrated),
            (
                "other-owner-gap",
                "mallory-id",
                1,
                MessageHydrationState::Discovered,
            ),
        ] {
            upsert_message(
                &db,
                &MessageRecord {
                    msg_id: msg_id.to_owned(),
                    owner_identity_id: owner_identity_id.to_owned(),
                    owner_did: "did:example:alice".to_owned(),
                    conversation_id: "dm:did:example:bob".to_owned(),
                    thread_id: "dm:did:example:bob".to_owned(),
                    direction: 0,
                    sender_did: "did:example:bob".to_owned(),
                    receiver_did: "did:example:alice".to_owned(),
                    content_type: "text/plain".to_owned(),
                    content: (hydration_state == MessageHydrationState::Hydrated)
                        .then_some("complete")
                        .unwrap_or_default()
                        .to_owned(),
                    server_seq: Some(server_seq),
                    hydration_state,
                    stored_at: format!("2026-07-24T00:00:0{server_seq}Z"),
                    ..MessageRecord::default()
                },
            )
            .unwrap();
        }

        let cursor = catch_up_server_seq_for_thread_ref_for_owner_identity(
            &db,
            "alice-id",
            "did:example:alice",
            &thread,
        )
        .unwrap();
        assert_eq!(cursor.default_after_server_seq, Some(1));
        assert_eq!(cursor.hydration_gap_after_server_seq, Some(1));
        let local = list_messages_for_thread_ref_for_owner_identity(
            &db,
            "alice-id",
            "did:example:alice",
            &thread,
            10,
            None,
        )
        .unwrap();
        assert_eq!(
            local
                .records
                .iter()
                .map(|record| record.msg_id.as_str())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["complete-1", "complete-3"])
        );

        upsert_message(
            &db,
            &MessageRecord {
                msg_id: "gap-2".to_owned(),
                owner_identity_id: "alice-id".to_owned(),
                owner_did: "did:example:alice".to_owned(),
                conversation_id: "dm:did:example:bob".to_owned(),
                thread_id: "dm:did:example:bob".to_owned(),
                direction: 0,
                sender_did: "did:example:bob".to_owned(),
                receiver_did: "did:example:alice".to_owned(),
                content_type: "text/plain".to_owned(),
                content: "hydrated two".to_owned(),
                server_seq: Some(2),
                hydration_state: MessageHydrationState::Hydrated,
                stored_at: "2026-07-24T00:00:02Z".to_owned(),
                ..MessageRecord::default()
            },
        )
        .unwrap();
        let cursor = catch_up_server_seq_for_thread_ref_for_owner_identity(
            &db,
            "alice-id",
            "did:example:alice",
            &thread,
        )
        .unwrap();
        assert_eq!(cursor.default_after_server_seq, Some(3));
        assert_eq!(cursor.hydration_gap_after_server_seq, None);
    }

    #[test]
    fn local_state_catch_up_cursor_restarts_for_unsequenced_legacy_probe() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let thread = crate::messages::ThreadRef::Direct(
            crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
        );
        upsert_message(
            &db,
            &MessageRecord {
                msg_id: "unsequenced-legacy-probe".to_owned(),
                owner_identity_id: "alice-id".to_owned(),
                owner_did: "did:example:alice".to_owned(),
                conversation_id: "dm:did:example:bob".to_owned(),
                thread_id: "dm:did:example:bob".to_owned(),
                direction: 0,
                sender_did: "did:example:bob".to_owned(),
                receiver_did: "did:example:alice".to_owned(),
                content_type: "text/plain".to_owned(),
                hydration_state: MessageHydrationState::LegacyProbe,
                stored_at: "2026-07-24T00:00:00Z".to_owned(),
                ..MessageRecord::default()
            },
        )
        .unwrap();

        let cursor = catch_up_server_seq_for_thread_ref_for_owner_identity(
            &db,
            "alice-id",
            "did:example:alice",
            &thread,
        )
        .unwrap();
        assert_eq!(cursor.default_after_server_seq, Some(0));
        assert_eq!(cursor.hydration_gap_after_server_seq, Some(0));
    }

    #[test]
    fn local_state_legacy_probes_require_trusted_catch_up_coverage() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let thread = crate::messages::ThreadRef::Direct(
            crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
        );
        for (msg_id, server_seq, content_type, hydration_state) in [
            (
                "valid-empty-text",
                10,
                "text/plain",
                MessageHydrationState::LegacyProbe,
            ),
            (
                "valid-unsupported",
                11,
                "application/x-awiki-future",
                MessageHydrationState::LegacyProbe,
            ),
            (
                "confirmed-discovery",
                12,
                "text/plain",
                MessageHydrationState::Discovered,
            ),
        ] {
            upsert_message(
                &db,
                &MessageRecord {
                    msg_id: msg_id.to_owned(),
                    owner_identity_id: "alice-id".to_owned(),
                    owner_did: "did:example:alice".to_owned(),
                    conversation_id: "dm:did:example:bob".to_owned(),
                    thread_id: "dm:did:example:bob".to_owned(),
                    direction: 0,
                    sender_did: "did:example:bob".to_owned(),
                    receiver_did: "did:example:alice".to_owned(),
                    content_type: content_type.to_owned(),
                    server_seq: Some(server_seq),
                    hydration_state,
                    stored_at: format!("2026-07-24T00:00:{server_seq}Z"),
                    ..MessageRecord::default()
                },
            )
            .unwrap();
        }

        assert_eq!(
            resolve_legacy_hydration_probes_for_thread_ref(
                &db,
                "alice-id",
                "did:example:alice",
                &thread,
                9,
                9,
                false,
            )
            .unwrap(),
            0
        );
        assert_eq!(
            resolve_legacy_hydration_probes_for_thread_ref(
                &db,
                "alice-id",
                "did:example:alice",
                &thread,
                9,
                10,
                false,
            )
            .unwrap(),
            1
        );
        assert_eq!(
            resolve_legacy_hydration_probes_for_thread_ref(
                &db,
                "alice-id",
                "did:example:alice",
                &thread,
                10,
                10,
                true,
            )
            .unwrap(),
            1
        );
        let states = db
            .prepare(
                "SELECT msg_id, hydration_state FROM messages WHERE owner_identity_id = 'alice-id' ORDER BY msg_id",
            )
            .unwrap()
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(
            states,
            vec![
                ("confirmed-discovery".to_owned(), "discovered".to_owned()),
                ("valid-empty-text".to_owned(), "hydrated".to_owned()),
                ("valid-unsupported".to_owned(), "hydrated".to_owned()),
            ]
        );
    }

    #[test]
    fn local_state_hydration_is_content_type_agnostic_and_complete_projection_wins() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let content_types = [
            ("text", "text/plain", "plain body"),
            ("payload", "application/json", r#"{"kind":"payload"}"#),
            (
                "attachment",
                crate::attachments::manifest::attachment_manifest_content_type(),
                r#"{"attachments":[{"attachment_id":"att-1"}]}"#,
            ),
            (
                "e2ee",
                "application/anp-e2ee+json",
                r#"{"ciphertext":"redacted-test-value"}"#,
            ),
        ];
        for (index, (msg_id, content_type, complete_content)) in
            content_types.into_iter().enumerate()
        {
            let server_seq = i64::try_from(index + 1).unwrap();
            let base = MessageRecord {
                msg_id: msg_id.to_owned(),
                owner_identity_id: "alice-id".to_owned(),
                owner_did: "did:example:alice".to_owned(),
                conversation_id: "dm:did:example:bob".to_owned(),
                thread_id: "dm:did:example:bob".to_owned(),
                direction: 0,
                sender_did: "did:example:bob".to_owned(),
                receiver_did: "did:example:alice".to_owned(),
                content_type: content_type.to_owned(),
                server_seq: Some(server_seq),
                stored_at: format!("2026-07-24T00:00:0{server_seq}Z"),
                ..MessageRecord::default()
            };
            upsert_message(
                &db,
                &MessageRecord {
                    hydration_state: MessageHydrationState::Discovered,
                    ..base.clone()
                },
            )
            .unwrap();
            upsert_message(
                &db,
                &MessageRecord {
                    content: complete_content.to_owned(),
                    hydration_state: MessageHydrationState::Hydrated,
                    ..base.clone()
                },
            )
            .unwrap();
            // A later duplicate metadata event must not erase or downgrade the
            // complete projection.
            upsert_message(
                &db,
                &MessageRecord {
                    hydration_state: MessageHydrationState::Discovered,
                    ..base
                },
            )
            .unwrap();

            let persisted = db
                .query_row(
                    "SELECT content, hydration_state FROM messages WHERE owner_identity_id = 'alice-id' AND msg_id = ?1",
                    [msg_id],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )
                .unwrap();
            assert_eq!(
                persisted,
                (complete_content.to_owned(), "hydrated".to_owned())
            );
        }
    }

    #[test]
    fn metadata_only_json_discovery_stays_unread_for_direct_and_group() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        for (kind, seq, conversation_id, group_did) in [
            ("direct", 20, "dm:did:example:bob", ""),
            ("group", 21, "group:did:example:group", "did:example:group"),
        ] {
            let input_id = format!("{kind}-payload");
            let canonical_id = if group_did.is_empty() {
                input_id.clone()
            } else {
                format!("{group_did}:{seq}")
            };
            let base = MessageRecord {
                msg_id: input_id,
                owner_identity_id: "alice-id".to_owned(),
                owner_did: "did:example:alice".to_owned(),
                conversation_id: conversation_id.to_owned(),
                thread_id: conversation_id.to_owned(),
                direction: 0,
                sender_did: "did:example:bob".to_owned(),
                receiver_did: "did:example:alice".to_owned(),
                group_id: group_did.to_owned(),
                group_did: group_did.to_owned(),
                content_type: "application/json".to_owned(),
                server_seq: Some(seq),
                stored_at: format!("2026-07-24T00:00:{seq}Z"),
                ..MessageRecord::default()
            };
            upsert_message(
                &db,
                &MessageRecord {
                    hydration_state: MessageHydrationState::Discovered,
                    ..base.clone()
                },
            )
            .unwrap();
            assert_eq!(read_by_msg_id(&db, &canonical_id), 0);

            upsert_message(
                &db,
                &MessageRecord {
                    content: r#"{"text":"normal payload"}"#.to_owned(),
                    hydration_state: MessageHydrationState::Hydrated,
                    ..base.clone()
                },
            )
            .unwrap();
            assert_eq!(read_by_msg_id(&db, &canonical_id), 0);

            let control_seq = seq + 100;
            let control_input_id = format!("{kind}-control");
            let control_id = if group_did.is_empty() {
                control_input_id.clone()
            } else {
                format!("{group_did}:{control_seq}")
            };
            upsert_message(
                &db,
                &MessageRecord {
                    msg_id: control_input_id,
                    server_seq: Some(control_seq),
                    content: r#"{"schema":"awiki.control.v1","command":"test"}"#.to_owned(),
                    hydration_state: MessageHydrationState::Hydrated,
                    ..base
                },
            )
            .unwrap();
            assert_eq!(read_by_msg_id(&db, &control_id), 1);
        }
    }

    #[test]
    fn metadata_only_replay_preserves_hydrated_rich_projection_and_canonical_route() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let canonical_conversation = "dm:peer-scope:v1:canonical-test";
        let rich_metadata = r#"{"decryption_state":"decrypted","security":"direct-e2ee","peer_user_id":"peer-user","peer_full_handle":"bob.example.test"}"#;
        upsert_message(
            &db,
            &MessageRecord {
                msg_id: "rich-message".to_owned(),
                owner_identity_id: "alice-id".to_owned(),
                owner_did: "did:example:alice".to_owned(),
                conversation_id: canonical_conversation.to_owned(),
                thread_id: canonical_conversation.to_owned(),
                direction: 0,
                sender_did: "did:example:bob".to_owned(),
                receiver_did: "did:example:alice".to_owned(),
                content_type: crate::attachments::manifest::attachment_manifest_content_type()
                    .to_owned(),
                content: r#"{"attachments":[{"attachment_id":"att-rich"}]}"#.to_owned(),
                title: "rich title".to_owned(),
                server_seq: Some(30),
                hydration_state: MessageHydrationState::Hydrated,
                sent_at: "2026-07-24T00:00:30Z".to_owned(),
                stored_at: "2026-07-24T00:00:31Z".to_owned(),
                is_e2ee: true,
                is_read: false,
                sender_name: "Bob".to_owned(),
                metadata: rich_metadata.to_owned(),
                credential_name: "original-credential".to_owned(),
                ..MessageRecord::default()
            },
        )
        .unwrap();
        db.execute(
            "UPDATE messages SET mentions_current_user = 1 WHERE owner_identity_id = 'alice-id' AND msg_id = 'rich-message'",
            [],
        )
        .unwrap();

        upsert_message(
            &db,
            &MessageRecord {
                msg_id: "rich-message".to_owned(),
                owner_identity_id: "alice-id".to_owned(),
                owner_did: "did:example:alice".to_owned(),
                conversation_id: "dm:did:example:bob".to_owned(),
                thread_id: "dm:did:example:bob".to_owned(),
                direction: 1,
                sender_did: "did:example:bob".to_owned(),
                receiver_did: "did:example:alice".to_owned(),
                content_type: "text/plain".to_owned(),
                content: "metadata pseudo body".to_owned(),
                title: "metadata title".to_owned(),
                server_seq: Some(30),
                hydration_state: MessageHydrationState::Discovered,
                is_read: true,
                sent_at: "2026-07-24T01:00:30Z".to_owned(),
                stored_at: "2026-07-24T01:00:31Z".to_owned(),
                metadata: r#"{"server_sequence":30}"#.to_owned(),
                credential_name: "metadata-credential".to_owned(),
                ..MessageRecord::default()
            },
        )
        .unwrap();

        let persisted = db
            .query_row(
                r#"SELECT conversation_id, direction, content_type, content, title,
                          sent_at, stored_at, is_e2ee, is_read, sender_name,
                          metadata, mentions_current_user, credential_name,
                          hydration_state
FROM messages
WHERE owner_identity_id = 'alice-id' AND msg_id = 'rich-message'"#,
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, i64>(7)?,
                        row.get::<_, i64>(8)?,
                        row.get::<_, String>(9)?,
                        row.get::<_, String>(10)?,
                        row.get::<_, i64>(11)?,
                        row.get::<_, String>(12)?,
                        row.get::<_, String>(13)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(persisted.0, canonical_conversation);
        assert_eq!(persisted.1, 0);
        assert_eq!(
            persisted.2,
            crate::attachments::manifest::attachment_manifest_content_type()
        );
        assert!(persisted.3.contains("att-rich"));
        assert_eq!(persisted.4, "rich title");
        assert_eq!(persisted.5, "2026-07-24T00:00:30Z");
        assert_eq!(persisted.6, "2026-07-24T00:00:31Z");
        assert_eq!(persisted.7, 1);
        assert_eq!(persisted.8, 0);
        assert_eq!(persisted.9, "Bob");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&persisted.10).unwrap(),
            serde_json::from_str::<serde_json::Value>(rich_metadata).unwrap()
        );
        assert_eq!(persisted.11, 1);
        assert_eq!(persisted.12, "original-credential");
        assert_eq!(persisted.13, "hydrated");
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM conversation_registry WHERE owner_identity_id = 'alice-id' AND conversation_id = 'dm:did:example:bob'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            0
        );
    }

    #[test]
    fn local_state_mark_thread_read_watermark_marks_up_to_seq_and_rebuilds_summary() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let owner_identity_id = "alice-id";
        let owner_did = "did:example:alice";
        let conversation_id = "dm:did:example:bob";
        for (msg_id, seq) in [("m1", 1), ("m2", 2), ("m3", 3)] {
            upsert_message(
                &db,
                &MessageRecord {
                    msg_id: msg_id.to_owned(),
                    owner_identity_id: owner_identity_id.to_owned(),
                    owner_did: owner_did.to_owned(),
                    conversation_id: conversation_id.to_owned(),
                    thread_id: conversation_id.to_owned(),
                    direction: 0,
                    sender_did: "did:example:bob".to_owned(),
                    receiver_did: owner_did.to_owned(),
                    content_type: "text/plain".to_owned(),
                    content: msg_id.to_owned(),
                    server_seq: Some(seq),
                    stored_at: format!("2026-06-27T00:00:0{seq}Z"),
                    ..MessageRecord::default()
                },
            )
            .unwrap();
        }
        assert_eq!(summary_unread(&db, owner_identity_id, conversation_id), 3);

        let result = mark_thread_read_watermark_for_owner_identity(
            &db,
            owner_identity_id,
            owner_did,
            MarkThreadReadWatermarkInput {
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                ),
                read_watermark_message_id: Some("m2".to_owned()),
                read_watermark_seq: Some("2".to_owned()),
                read_watermark_at: Some("2026-06-27T00:01:00Z".to_owned()),
                pending_remote_ack: true,
            },
        )
        .unwrap();

        assert_eq!(result.updated_count, 2);
        assert_eq!(result.read_watermark_seq.as_deref(), Some("2"));
        assert!(result.advanced);
        assert_eq!(read_by_msg_id(&db, "m1"), 1);
        assert_eq!(read_by_msg_id(&db, "m2"), 1);
        assert_eq!(read_by_msg_id(&db, "m3"), 0);
        assert_eq!(summary_unread(&db, owner_identity_id, conversation_id), 1);
        let pending: bool = db
            .query_row(
                "SELECT pending_remote_ack FROM thread_read_state WHERE owner_identity_id = ?1",
                [owner_identity_id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(pending);
    }

    #[test]
    fn local_state_direct_did_thread_ref_includes_peer_scope_messages() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let owner_identity_id = "alice-id";
        let owner_did = "did:example:alice";
        let peer_did = "did:wba:example.com:agent:runtime:bob:e1_peer";
        let peer_handle = "bob.example.com";
        let conversation_id = "dm:peer-scope:v1:bob";
        for (msg_id, seq) in [("m1", 1), ("m2", 2), ("m3", 3)] {
            upsert_message(
                &db,
                &peer_scope_message(
                    msg_id,
                    owner_identity_id,
                    owner_did,
                    conversation_id,
                    peer_did,
                    peer_handle,
                    seq,
                ),
            )
            .unwrap();
        }
        upsert_message(
            &db,
            &peer_scope_message(
                "other",
                owner_identity_id,
                owner_did,
                "dm:peer-scope:v1:other",
                "did:wba:example.com:agent:runtime:other:e1_peer",
                "other.example.com",
                2,
            ),
        )
        .unwrap();

        let did_records = list_messages_for_thread_ref_for_owner_identity(
            &db,
            owner_identity_id,
            owner_did,
            &crate::messages::ThreadRef::Direct(crate::ids::PeerRef::parse(peer_did, "").unwrap()),
            10,
            None,
        )
        .unwrap();
        assert_eq!(
            did_records
                .records
                .iter()
                .map(|record| record.msg_id.as_str())
                .collect::<Vec<_>>(),
            vec!["m3", "m2", "m1"]
        );

        let handle_records = list_messages_for_thread_ref_for_owner_identity(
            &db,
            owner_identity_id,
            owner_did,
            &crate::messages::ThreadRef::Direct(
                crate::ids::PeerRef::parse(peer_handle, "").unwrap(),
            ),
            10,
            None,
        )
        .unwrap();
        assert_eq!(handle_records.records.len(), 3);

        let result = mark_thread_read_watermark_for_owner_identity(
            &db,
            owner_identity_id,
            owner_did,
            MarkThreadReadWatermarkInput {
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse(peer_did, "").unwrap(),
                ),
                read_watermark_message_id: Some("m2".to_owned()),
                read_watermark_seq: Some("2".to_owned()),
                read_watermark_at: Some("2026-06-27T00:02:00Z".to_owned()),
                pending_remote_ack: false,
            },
        )
        .unwrap();

        assert_eq!(result.updated_count, 2);
        assert_eq!(read_by_msg_id(&db, "m1"), 1);
        assert_eq!(read_by_msg_id(&db, "m2"), 1);
        assert_eq!(read_by_msg_id(&db, "m3"), 0);
        assert_eq!(read_by_msg_id(&db, "other"), 0);
        assert_eq!(
            summary_snapshot(&db, owner_identity_id, conversation_id).unread_count,
            1
        );
    }

    #[test]
    fn local_state_peer_scope_thread_ref_includes_legacy_direct_messages() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let owner_identity_id = "alice-id";
        let owner_did = "did:wba:example.com:alice:e1_owner";
        let legacy_peer_did = "did:wba:example.com:agent:runtime:bob:e1_old";
        let current_peer_did = "did:wba:example.com:agent:runtime:bob:e1_current";
        let peer_handle = "bob.example.com";
        let legacy_conversation_id =
            crate::internal::local_state::owner_scope::direct_conversation_id(legacy_peer_did);
        let scoped_conversation_id = "dm:peer-scope:v1:bob";

        db.execute(
            r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, conversation_id, thread_id, direction,
     sender_did, receiver_did, content_type, content, server_seq, sent_at, stored_at, is_read)
VALUES (?1, ?2, ?3, ?4, ?4, 0, ?5, ?3, 'text/plain', 'legacy reply', 1,
        '2026-06-27T00:00:01Z', '2026-06-27T00:00:01Z', 0)"#,
            (
                "legacy-reply",
                owner_identity_id,
                owner_did,
                &legacy_conversation_id,
                legacy_peer_did,
            ),
        )
        .unwrap();
        upsert_message(
            &db,
            &peer_scope_message(
                "scoped-reply",
                owner_identity_id,
                owner_did,
                scoped_conversation_id,
                current_peer_did,
                peer_handle,
                2,
            ),
        )
        .unwrap();

        let records = list_messages_for_thread_ref_for_owner_identity(
            &db,
            owner_identity_id,
            owner_did,
            &crate::messages::ThreadRef::Thread(
                crate::ids::ThreadId::parse(scoped_conversation_id).unwrap(),
            ),
            10,
            None,
        )
        .unwrap();
        assert_eq!(
            records
                .records
                .iter()
                .map(|record| record.msg_id.as_str())
                .collect::<Vec<_>>(),
            vec!["scoped-reply", "legacy-reply"]
        );
        assert!(
            records
                .records
                .iter()
                .all(|record| record.conversation_id == scoped_conversation_id
                    && record.thread_id == scoped_conversation_id),
            "peer-scope local history must project alias rows to the requested storage thread"
        );

        let result = mark_thread_read_watermark_for_owner_identity(
            &db,
            owner_identity_id,
            owner_did,
            MarkThreadReadWatermarkInput {
                thread: crate::messages::ThreadRef::Thread(
                    crate::ids::ThreadId::parse(scoped_conversation_id).unwrap(),
                ),
                read_watermark_message_id: Some("scoped-reply".to_owned()),
                read_watermark_seq: Some("2".to_owned()),
                read_watermark_at: Some("2026-06-27T00:02:00Z".to_owned()),
                pending_remote_ack: false,
            },
        )
        .unwrap();

        assert_eq!(result.updated_count, 2);
        assert_eq!(read_by_msg_id(&db, "legacy-reply"), 1);
        assert_eq!(read_by_msg_id(&db, "scoped-reply"), 1);
    }

    #[test]
    fn local_state_direct_owner_did_does_not_include_peer_scope_messages_addressed_to_owner() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let owner_identity_id = "alice-id";
        let owner_did = "did:example:alice";
        let direct_self_conversation_id =
            crate::internal::local_state::owner_scope::direct_conversation_id(owner_did);
        let agent_conversation_id = "dm:peer-scope:v1:agent";
        upsert_message(
            &db,
            &MessageRecord {
                msg_id: "self".to_owned(),
                owner_identity_id: owner_identity_id.to_owned(),
                owner_did: owner_did.to_owned(),
                conversation_id: direct_self_conversation_id.clone(),
                thread_id: direct_self_conversation_id.clone(),
                direction: 1,
                sender_did: owner_did.to_owned(),
                receiver_did: owner_did.to_owned(),
                content_type: "text/plain".to_owned(),
                content: "self direct".to_owned(),
                server_seq: Some(10),
                sent_at: "2026-06-27T00:00:10Z".to_owned(),
                stored_at: "2026-06-27T00:00:10Z".to_owned(),
                ..MessageRecord::default()
            },
        )
        .unwrap();
        upsert_message(
            &db,
            &peer_scope_message(
                "agent",
                owner_identity_id,
                owner_did,
                agent_conversation_id,
                "did:wba:example.com:agent:runtime:bot:e1_peer",
                "bot.example.com",
                20,
            ),
        )
        .unwrap();

        let records = list_messages_for_thread_ref_for_owner_identity(
            &db,
            owner_identity_id,
            owner_did,
            &crate::messages::ThreadRef::Direct(crate::ids::PeerRef::parse(owner_did, "").unwrap()),
            10,
            None,
        )
        .unwrap();

        assert_eq!(
            records
                .records
                .iter()
                .map(|record| record.msg_id.as_str())
                .collect::<Vec<_>>(),
            vec!["self"]
        );
    }

    #[test]
    fn local_state_mark_thread_read_watermark_rejects_message_from_other_thread() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let owner_identity_id = "alice-id";
        let owner_did = "did:example:alice";
        let direct_self_conversation_id =
            crate::internal::local_state::owner_scope::direct_conversation_id(owner_did);
        let agent_conversation_id = "dm:peer-scope:v1:agent";
        upsert_message(
            &db,
            &MessageRecord {
                msg_id: "self".to_owned(),
                owner_identity_id: owner_identity_id.to_owned(),
                owner_did: owner_did.to_owned(),
                conversation_id: direct_self_conversation_id.clone(),
                thread_id: direct_self_conversation_id.clone(),
                direction: 0,
                sender_did: owner_did.to_owned(),
                receiver_did: owner_did.to_owned(),
                content_type: "text/plain".to_owned(),
                content: "self direct".to_owned(),
                server_seq: Some(10),
                sent_at: "2026-06-27T00:00:10Z".to_owned(),
                stored_at: "2026-06-27T00:00:10Z".to_owned(),
                ..MessageRecord::default()
            },
        )
        .unwrap();
        upsert_message(
            &db,
            &peer_scope_message(
                "agent",
                owner_identity_id,
                owner_did,
                agent_conversation_id,
                "did:wba:example.com:agent:runtime:bot:e1_peer",
                "bot.example.com",
                20,
            ),
        )
        .unwrap();

        let result = mark_thread_read_watermark_for_owner_identity(
            &db,
            owner_identity_id,
            owner_did,
            MarkThreadReadWatermarkInput {
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse(owner_did, "").unwrap(),
                ),
                read_watermark_message_id: Some("agent".to_owned()),
                read_watermark_seq: Some("20".to_owned()),
                read_watermark_at: Some("2026-06-27T00:02:00Z".to_owned()),
                pending_remote_ack: true,
            },
        )
        .unwrap();

        assert_eq!(result.updated_count, 0);
        assert_eq!(result.read_watermark_message_id, None);
        assert_eq!(result.read_watermark_seq, None);
        assert!(!result.advanced);
        assert_eq!(read_by_msg_id(&db, "self"), 0);
        assert_eq!(read_by_msg_id(&db, "agent"), 0);
        let stored: (Option<String>, Option<String>) = db
            .query_row(
                r#"
SELECT read_watermark_message_id, read_watermark_seq
FROM thread_read_state
WHERE owner_identity_id = ?1 AND thread_id = ?2"#,
                (owner_identity_id, direct_self_conversation_id),
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(stored, (None, None));
    }

    #[test]
    fn local_state_mark_thread_read_watermark_replaces_existing_foreign_watermark() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let owner_identity_id = "alice-id";
        let owner_did = "did:example:alice";
        let human_peer_did = "did:example:bob";
        let direct_self_conversation_id =
            crate::internal::local_state::owner_scope::direct_conversation_id(human_peer_did);
        let agent_conversation_id = "dm:peer-scope:v1:agent";
        upsert_message(
            &db,
            &MessageRecord {
                msg_id: "self".to_owned(),
                owner_identity_id: owner_identity_id.to_owned(),
                owner_did: owner_did.to_owned(),
                conversation_id: direct_self_conversation_id.clone(),
                thread_id: direct_self_conversation_id.clone(),
                direction: 0,
                sender_did: human_peer_did.to_owned(),
                receiver_did: owner_did.to_owned(),
                content_type: "text/plain".to_owned(),
                content: "self direct".to_owned(),
                server_seq: Some(10),
                sent_at: "2026-06-27T00:00:10Z".to_owned(),
                stored_at: "2026-06-27T00:00:10Z".to_owned(),
                ..MessageRecord::default()
            },
        )
        .unwrap();
        upsert_message(
            &db,
            &peer_scope_message(
                "agent",
                owner_identity_id,
                owner_did,
                agent_conversation_id,
                "did:wba:example.com:agent:runtime:bot:e1_peer",
                "bot.example.com",
                20,
            ),
        )
        .unwrap();
        db.execute(
            r#"
INSERT INTO thread_read_state
    (owner_identity_id, owner_did, thread_scope, thread_id, conversation_id,
     read_watermark_message_id, read_watermark_seq, read_watermark_at,
     pending_remote_ack, remote_ack_at, updated_at)
VALUES (?1, ?2, 'direct', ?3, ?3, 'agent', '20',
        '2026-06-27T00:00:20Z', 1, NULL, '2026-06-27T00:00:20Z')"#,
            (
                owner_identity_id,
                owner_did,
                direct_self_conversation_id.as_str(),
            ),
        )
        .unwrap();

        let result = mark_thread_read_watermark_for_owner_identity(
            &db,
            owner_identity_id,
            owner_did,
            MarkThreadReadWatermarkInput {
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse(human_peer_did, "").unwrap(),
                ),
                read_watermark_message_id: Some("self".to_owned()),
                read_watermark_seq: Some("10".to_owned()),
                read_watermark_at: Some("2026-06-27T00:01:00Z".to_owned()),
                pending_remote_ack: false,
            },
        )
        .unwrap();

        assert_eq!(result.updated_count, 0);
        assert_eq!(result.read_watermark_message_id.as_deref(), Some("self"));
        assert_eq!(result.read_watermark_seq.as_deref(), Some("10"));
        assert!(result.advanced);
        assert_eq!(read_by_msg_id(&db, "self"), 1);
        assert_eq!(read_by_msg_id(&db, "agent"), 0);
        let stored: (Option<String>, Option<String>, bool) = db
            .query_row(
                r#"
SELECT read_watermark_message_id, read_watermark_seq, pending_remote_ack
FROM thread_read_state
WHERE owner_identity_id = ?1 AND thread_id = ?2"#,
                (owner_identity_id, direct_self_conversation_id),
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            stored,
            (Some("self".to_owned()), Some("10".to_owned()), false)
        );
    }

    #[test]
    fn local_state_repair_replays_direct_read_state_to_peer_scope_alias() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let owner_identity_id = "alice-id";
        let owner_did = "did:example:alice";
        let peer_did = "did:wba:example.com:agent:runtime:bob:e1_peer";
        let peer_handle = "bob.example.com";
        let conversation_id = "dm:peer-scope:v1:bob";
        for (msg_id, seq) in [("m1", 1), ("m2", 2), ("m3", 3)] {
            upsert_message(
                &db,
                &peer_scope_message(
                    msg_id,
                    owner_identity_id,
                    owner_did,
                    conversation_id,
                    peer_did,
                    peer_handle,
                    seq,
                ),
            )
            .unwrap();
        }
        db.execute(
            r#"
INSERT INTO thread_read_state
    (owner_identity_id, owner_did, thread_scope, thread_id, conversation_id,
     read_watermark_message_id, read_watermark_seq, read_watermark_at,
     pending_remote_ack, remote_ack_at, updated_at)
VALUES (?1, ?2, 'direct', ?3, ?3, 'm2', '2', '2026-06-27T00:00:02Z',
        0, '2026-06-27T00:00:02Z', '2026-06-27T00:00:02Z')"#,
            (
                owner_identity_id,
                owner_did,
                direct_conversation_id_for_peer_ref(peer_did),
            ),
        )
        .unwrap();
        assert_eq!(
            summary_snapshot(&db, owner_identity_id, conversation_id).unread_count,
            3
        );

        let repaired = repair_thread_read_state_alias_projection(&db).unwrap();

        assert_eq!(repaired, 2);
        assert_eq!(read_by_msg_id(&db, "m1"), 1);
        assert_eq!(read_by_msg_id(&db, "m2"), 1);
        assert_eq!(read_by_msg_id(&db, "m3"), 0);
        assert_eq!(
            summary_snapshot(&db, owner_identity_id, conversation_id).unread_count,
            1
        );
    }

    #[test]
    fn local_state_repair_ignores_direct_read_state_time_for_peer_scope_alias() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let owner_identity_id = "alice-id";
        let owner_did = "did:example:alice";
        let peer_did = "did:wba:example.com:agent:runtime:bob:e1_peer";
        let peer_handle = "bob.example.com";
        let conversation_id = "dm:peer-scope:v1:bob";
        for (msg_id, seq) in [("m1", 1), ("m2", 2), ("m3", 3), ("m4", 4)] {
            upsert_message(
                &db,
                &peer_scope_message(
                    msg_id,
                    owner_identity_id,
                    owner_did,
                    conversation_id,
                    peer_did,
                    peer_handle,
                    seq,
                ),
            )
            .unwrap();
        }
        db.execute(
            r#"
INSERT INTO thread_read_state
    (owner_identity_id, owner_did, thread_scope, thread_id, conversation_id,
     read_watermark_message_id, read_watermark_seq, read_watermark_at,
     pending_remote_ack, remote_ack_at, updated_at)
VALUES (?1, ?2, 'direct', ?3, ?3, 'm2', '2', '2026-06-27T00:00:03Z',
        0, '2026-06-27T00:00:03Z', '2026-06-27T00:00:03Z')"#,
            (
                owner_identity_id,
                owner_did,
                direct_conversation_id_for_peer_ref(peer_did),
            ),
        )
        .unwrap();

        let repaired = repair_thread_read_state_alias_projection(&db).unwrap();

        assert_eq!(repaired, 2);
        assert_eq!(read_by_msg_id(&db, "m1"), 1);
        assert_eq!(read_by_msg_id(&db, "m2"), 1);
        assert_eq!(read_by_msg_id(&db, "m3"), 0);
        assert_eq!(read_by_msg_id(&db, "m4"), 0);
        assert_eq!(
            summary_snapshot(&db, owner_identity_id, conversation_id).unread_count,
            2
        );
    }

    #[test]
    fn local_state_repair_does_not_replay_peer_scope_read_state_by_time() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let owner_identity_id = "alice-id";
        let owner_did = "did:example:alice";
        let peer_did = "did:wba:example.com:agent:runtime:bob:e1_peer";
        let peer_handle = "bob.example.com";
        let conversation_id = "dm:peer-scope:v1:bob";
        for (msg_id, seq) in [("m1", 1), ("m2", 2), ("m3", 3)] {
            upsert_message(
                &db,
                &peer_scope_message(
                    msg_id,
                    owner_identity_id,
                    owner_did,
                    conversation_id,
                    peer_did,
                    peer_handle,
                    seq,
                ),
            )
            .unwrap();
        }
        db.execute(
            "UPDATE messages SET is_read = 1 WHERE msg_id IN ('m1', 'm2')",
            [],
        )
        .unwrap();
        crate::internal::local_state::conversation_summaries::rebuild_all(&db).unwrap();
        db.execute(
            r#"
INSERT INTO thread_read_state
    (owner_identity_id, owner_did, thread_scope, thread_id, conversation_id,
     read_watermark_message_id, read_watermark_seq, read_watermark_at,
     pending_remote_ack, remote_ack_at, updated_at)
VALUES (?1, ?2, 'direct', ?3, ?3, 'm2', '2', '2026-06-27T00:00:03Z',
        0, '2026-06-27T00:00:03Z', '2026-06-27T00:00:03Z')"#,
            (owner_identity_id, owner_did, conversation_id),
        )
        .unwrap();

        let repaired = repair_thread_read_state_alias_projection(&db).unwrap();

        assert_eq!(repaired, 0);
        assert_eq!(read_by_msg_id(&db, "m1"), 1);
        assert_eq!(read_by_msg_id(&db, "m2"), 1);
        assert_eq!(read_by_msg_id(&db, "m3"), 0);
        assert_eq!(
            summary_snapshot(&db, owner_identity_id, conversation_id).unread_count,
            1
        );
    }

    #[test]
    fn local_state_mark_thread_read_watermark_is_monotonic_and_owner_scoped() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        for owner_identity_id in ["alice-id", "mallory-id"] {
            for (msg_id, seq) in [("m1", 1), ("m2", 2), ("m3", 3)] {
                upsert_message(
                    &db,
                    &MessageRecord {
                        msg_id: format!("{owner_identity_id}-{msg_id}"),
                        owner_identity_id: owner_identity_id.to_owned(),
                        owner_did: "did:example:alice".to_owned(),
                        conversation_id: "group:did:example:group".to_owned(),
                        thread_id: "group:did:example:group".to_owned(),
                        direction: 0,
                        sender_did: "did:example:bob".to_owned(),
                        group_id: "did:example:group".to_owned(),
                        group_did: "did:example:group".to_owned(),
                        content_type: "text/plain".to_owned(),
                        content: msg_id.to_owned(),
                        server_seq: Some(seq),
                        stored_at: format!("2026-06-27T00:00:0{seq}Z"),
                        ..MessageRecord::default()
                    },
                )
                .unwrap();
            }
        }

        let thread = crate::messages::ThreadRef::Group(
            crate::ids::GroupRef::parse("did:example:group").unwrap(),
        );
        mark_thread_read_watermark_for_owner_identity(
            &db,
            "alice-id",
            "did:example:alice",
            MarkThreadReadWatermarkInput {
                thread: thread.clone(),
                read_watermark_message_id: None,
                read_watermark_seq: Some("3".to_owned()),
                read_watermark_at: None,
                pending_remote_ack: false,
            },
        )
        .unwrap();
        let low = mark_thread_read_watermark_for_owner_identity(
            &db,
            "alice-id",
            "did:example:alice",
            MarkThreadReadWatermarkInput {
                thread,
                read_watermark_message_id: None,
                read_watermark_seq: Some("1".to_owned()),
                read_watermark_at: None,
                pending_remote_ack: true,
            },
        )
        .unwrap();

        assert_eq!(low.previous_read_watermark_seq.as_deref(), Some("3"));
        assert_eq!(low.read_watermark_seq.as_deref(), Some("3"));
        assert!(!low.advanced);
        let pending: bool = db
            .query_row(
                "SELECT pending_remote_ack FROM thread_read_state WHERE owner_identity_id = ?1",
                ["alice-id"],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!pending);
        assert_eq!(
            summary_unread(&db, "alice-id", "group:did:example:group"),
            0
        );
        assert_eq!(
            summary_unread(&db, "mallory-id", "group:did:example:group"),
            3
        );
    }

    #[test]
    fn local_state_low_watermark_ack_does_not_clear_newer_pending_state() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let owner_identity_id = "alice-id";
        let owner_did = "did:example:alice";
        let conversation_id = "dm:did:example:bob";
        for (msg_id, seq) in [("m1", 1), ("m2", 2), ("m3", 3)] {
            upsert_message(
                &db,
                &MessageRecord {
                    msg_id: msg_id.to_owned(),
                    owner_identity_id: owner_identity_id.to_owned(),
                    owner_did: owner_did.to_owned(),
                    conversation_id: conversation_id.to_owned(),
                    thread_id: conversation_id.to_owned(),
                    direction: 0,
                    sender_did: "did:example:bob".to_owned(),
                    receiver_did: owner_did.to_owned(),
                    content_type: "text/plain".to_owned(),
                    content: msg_id.to_owned(),
                    server_seq: Some(seq),
                    stored_at: format!("2026-06-27T00:00:0{seq}Z"),
                    ..MessageRecord::default()
                },
            )
            .unwrap();
        }
        let thread = crate::messages::ThreadRef::Direct(
            crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
        );
        mark_thread_read_watermark_for_owner_identity(
            &db,
            owner_identity_id,
            owner_did,
            MarkThreadReadWatermarkInput {
                thread: thread.clone(),
                read_watermark_message_id: None,
                read_watermark_seq: Some("3".to_owned()),
                read_watermark_at: Some("2026-06-27T00:03:00Z".to_owned()),
                pending_remote_ack: true,
            },
        )
        .unwrap();

        let low_ack = mark_thread_read_watermark_for_owner_identity(
            &db,
            owner_identity_id,
            owner_did,
            MarkThreadReadWatermarkInput {
                thread,
                read_watermark_message_id: None,
                read_watermark_seq: Some("1".to_owned()),
                read_watermark_at: Some("2026-06-27T00:01:00Z".to_owned()),
                pending_remote_ack: false,
            },
        )
        .unwrap();

        assert_eq!(low_ack.read_watermark_seq.as_deref(), Some("3"));
        assert!(!low_ack.advanced);
        let state = db
            .query_row(
                "SELECT read_watermark_seq, pending_remote_ack, remote_ack_at, updated_at FROM thread_read_state WHERE owner_identity_id = ?1",
                [owner_identity_id],
                |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, bool>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(state.0.as_deref(), Some("3"));
        assert!(state.1);
        assert!(state.2.is_none());
        assert_eq!(state.3, "2026-06-27T00:03:00Z");
    }

    #[test]
    fn local_state_messages_upsert_stores_owner_identity_and_replaces_existing() {
        let db = Connection::open_in_memory().unwrap();
        upsert_message(
            &db,
            &MessageRecord {
                msg_id: "msg-1".to_owned(),
                owner_identity_id: "alice-id".to_owned(),
                owner_did: "did:example:alice".to_owned(),
                conversation_id: "dm:did:example:bob".to_owned(),
                thread_id: "dm:did:example:bob".to_owned(),
                direction: 1,
                sender_did: "did:example:alice".to_owned(),
                receiver_did: "did:example:bob".to_owned(),
                content_type: "text/plain".to_owned(),
                content: "first".to_owned(),
                stored_at: "2026-05-24T00:00:00Z".to_owned(),
                is_e2ee: true,
                is_read: true,
                credential_name: "alice".to_owned(),
                ..MessageRecord::default()
            },
        )
        .unwrap();
        upsert_message(
            &db,
            &MessageRecord {
                msg_id: "msg-1".to_owned(),
                owner_identity_id: "alice-id".to_owned(),
                owner_did: "did:example:alice".to_owned(),
                conversation_id: "dm:did:example:bob".to_owned(),
                thread_id: "dm:did:example:bob".to_owned(),
                direction: 1,
                sender_did: "did:example:alice".to_owned(),
                receiver_did: "did:example:bob".to_owned(),
                content_type: "text/plain".to_owned(),
                content: "second".to_owned(),
                stored_at: "2026-05-24T00:00:01Z".to_owned(),
                is_e2ee: true,
                is_read: true,
                credential_name: "alice".to_owned(),
                ..MessageRecord::default()
            },
        )
        .unwrap();

        let row = db
            .query_row(
                "SELECT owner_identity_id, conversation_id, thread_id, content, stored_at, is_e2ee FROM messages WHERE msg_id = 'msg-1'",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, i64>(5)?,
                    ))
                },
            )
            .unwrap();

        assert_eq!(row.0, "alice-id");
        assert_eq!(row.1, "dm:did:example:bob");
        assert_eq!(row.2, row.1);
        assert_eq!(row.3, "second");
        assert_eq!(row.4, "2026-05-24T00:00:01Z");
        assert_eq!(row.5, 1);

        let summary = db
            .query_row(
                "SELECT message_count, last_message_id, last_content, last_message_at FROM conversation_summaries WHERE owner_identity_id = 'alice-id' AND conversation_id = 'dm:did:example:bob'",
                [],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?)),
            )
            .unwrap();
        assert_eq!(
            summary,
            (
                1,
                "msg-1".to_owned(),
                "second".to_owned(),
                "2026-05-24T00:00:01Z".to_owned()
            )
        );
    }

    #[test]
    fn local_state_messages_upsert_does_not_revert_read_or_e2ee_flags() {
        let db = Connection::open_in_memory().unwrap();
        upsert_message(
            &db,
            &MessageRecord {
                msg_id: "msg-flags".to_owned(),
                owner_identity_id: "owner-id".to_owned(),
                owner_did: "did:owner".to_owned(),
                conversation_id: "dm:did:peer".to_owned(),
                thread_id: "dm:did:peer".to_owned(),
                direction: 0,
                content: "read secure".to_owned(),
                stored_at: "2026-01-01T00:00:00Z".to_owned(),
                is_e2ee: true,
                is_read: true,
                ..MessageRecord::default()
            },
        )
        .unwrap();
        upsert_message(
            &db,
            &MessageRecord {
                msg_id: "msg-flags".to_owned(),
                owner_identity_id: "owner-id".to_owned(),
                owner_did: "did:owner".to_owned(),
                conversation_id: "dm:did:peer".to_owned(),
                thread_id: "dm:did:peer".to_owned(),
                direction: 0,
                content: "later projection".to_owned(),
                stored_at: "2026-01-02T00:00:00Z".to_owned(),
                is_e2ee: false,
                is_read: false,
                ..MessageRecord::default()
            },
        )
        .unwrap();

        let (is_e2ee, is_read): (i64, i64) = db
            .query_row(
                "SELECT is_e2ee, is_read FROM messages WHERE msg_id = 'msg-flags'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(is_e2ee, 1);
        assert_eq!(is_read, 1);
    }

    #[test]
    fn local_state_messages_upsert_empty_projection_preserves_existing_content() {
        let db = Connection::open_in_memory().unwrap();
        let owner_identity_id = "owner-id";
        let owner_did = "did:owner";
        let conversation_id = "dm:did:peer";
        let mut full = summary_test_message(
            "msg-body",
            owner_identity_id,
            owner_did,
            conversation_id,
            0,
            "history body",
            "2026-06-27T00:00:01Z",
            false,
        );
        full.server_seq = Some(10);
        upsert_message(&db, &full).unwrap();

        let mut metadata_only = summary_test_message(
            "msg-body",
            owner_identity_id,
            owner_did,
            conversation_id,
            0,
            "",
            "2026-06-27T00:00:02Z",
            false,
        );
        metadata_only.server_seq = Some(10);
        upsert_message(&db, &metadata_only).unwrap();

        let content = db
            .query_row(
                "SELECT content FROM messages WHERE owner_identity_id = ?1 AND msg_id = 'msg-body'",
                [owner_identity_id],
                |row| row.get::<_, String>(0),
            )
            .unwrap();
        assert_eq!(content, "history body");
        let summary = summary_snapshot(&db, owner_identity_id, conversation_id);
        assert_eq!(summary.last_message_id, "msg-body");
        assert_eq!(summary.last_content, "history body");
        assert_summary_matches_rebuild(&db, owner_identity_id, conversation_id);
    }

    #[test]
    fn local_state_messages_incremental_summary_matches_rebuild_for_insert_update() {
        let db = Connection::open_in_memory().unwrap();
        let owner_identity_id = "owner-id";
        let owner_did = "did:owner";
        let conversation_id = "dm:did:peer";

        upsert_message(
            &db,
            &summary_test_message(
                "msg-unread",
                owner_identity_id,
                owner_did,
                conversation_id,
                0,
                "first unread",
                "2026-06-27T00:00:01Z",
                false,
            ),
        )
        .unwrap();
        assert_eq!(
            summary_snapshot(&db, owner_identity_id, conversation_id).unread_count,
            1
        );
        assert_summary_matches_rebuild(&db, owner_identity_id, conversation_id);

        upsert_message(
            &db,
            &summary_test_message(
                "msg-older",
                owner_identity_id,
                owner_did,
                conversation_id,
                0,
                "older read",
                "2026-06-27T00:00:00Z",
                true,
            ),
        )
        .unwrap();
        let summary = summary_snapshot(&db, owner_identity_id, conversation_id);
        assert_eq!(summary.message_count, 2);
        assert_eq!(summary.unread_count, 1);
        assert_eq!(summary.last_message_id, "msg-unread");
        assert_summary_matches_rebuild(&db, owner_identity_id, conversation_id);

        upsert_message(
            &db,
            &summary_test_message(
                "msg-newer",
                owner_identity_id,
                owner_did,
                conversation_id,
                1,
                "newer outgoing",
                "2026-06-27T00:00:02Z",
                true,
            ),
        )
        .unwrap();
        let summary = summary_snapshot(&db, owner_identity_id, conversation_id);
        assert_eq!(summary.message_count, 3);
        assert_eq!(summary.unread_count, 1);
        assert_eq!(summary.last_message_id, "msg-newer");
        assert_eq!(summary.last_content, "newer outgoing");
        assert_summary_matches_rebuild(&db, owner_identity_id, conversation_id);

        upsert_message(
            &db,
            &summary_test_message(
                "msg-unread",
                owner_identity_id,
                owner_did,
                conversation_id,
                0,
                "first now read",
                "2026-06-27T00:00:01Z",
                true,
            ),
        )
        .unwrap();
        let summary = summary_snapshot(&db, owner_identity_id, conversation_id);
        assert_eq!(summary.message_count, 3);
        assert_eq!(summary.unread_count, 0);
        assert_eq!(summary.last_message_id, "msg-newer");
        assert_summary_matches_rebuild(&db, owner_identity_id, conversation_id);
    }

    #[test]
    fn local_state_messages_summary_uses_server_seq_for_same_second_last_message() {
        let db = Connection::open_in_memory().unwrap();
        let owner_identity_id = "owner-id";
        let owner_did = "did:owner";
        let conversation_id = "dm:did:peer";
        let timestamp = "2026-06-27T19:58:58Z";
        let mut outgoing = summary_test_message(
            "msg-z-outgoing",
            owner_identity_id,
            owner_did,
            conversation_id,
            1,
            "app to cli",
            timestamp,
            true,
        );
        outgoing.server_seq = Some(14233);
        upsert_message(&db, &outgoing).unwrap();
        assert_eq!(
            summary_snapshot(&db, owner_identity_id, conversation_id).last_message_id,
            "msg-z-outgoing"
        );

        let mut incoming = summary_test_message(
            "msg-a-incoming",
            owner_identity_id,
            owner_did,
            conversation_id,
            0,
            "cli to app",
            timestamp,
            false,
        );
        incoming.server_seq = Some(14234);
        upsert_message(&db, &incoming).unwrap();

        let summary = summary_snapshot(&db, owner_identity_id, conversation_id);
        assert_eq!(summary.message_count, 2);
        assert_eq!(summary.unread_count, 1);
        assert_eq!(summary.last_message_id, "msg-a-incoming");
        assert_eq!(summary.last_content, "cli to app");
        assert_summary_matches_rebuild(&db, owner_identity_id, conversation_id);
    }

    #[test]
    fn local_state_group_send_success_replaces_local_echo_with_canonical_id() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let owner_identity_id = "owner-id";
        let owner_did = "did:owner";
        let group_did = "did:example:groups:demo";
        let conversation_id =
            crate::internal::local_state::owner_scope::group_conversation_id(group_did);
        let local_id = "msg-awiki-me-local-1";
        let canonical_id = "did:example:groups:demo:6";

        upsert_message(
            &db,
            &MessageRecord {
                msg_id: local_id.to_owned(),
                owner_identity_id: owner_identity_id.to_owned(),
                owner_did: owner_did.to_owned(),
                conversation_id: conversation_id.clone(),
                thread_id: conversation_id.clone(),
                direction: 1,
                sender_did: owner_did.to_owned(),
                group_id: group_did.to_owned(),
                group_did: group_did.to_owned(),
                content_type: "application/json".to_owned(),
                content: serde_json::json!({
                    "text": "@agent 今天是几号了？"
                })
                .to_string(),
                stored_at: "2026-07-06T00:00:00Z".to_owned(),
                is_read: true,
                metadata: serde_json::json!({
                    "operation_id": format!("op-{local_id}"),
                    "send_state": {
                        "state": "pending",
                        "message_id": local_id
                    }
                })
                .to_string(),
                ..MessageRecord::default()
            },
        )
        .unwrap();

        upsert_message(
            &db,
            &MessageRecord {
                msg_id: canonical_id.to_owned(),
                owner_identity_id: owner_identity_id.to_owned(),
                owner_did: owner_did.to_owned(),
                conversation_id: conversation_id.clone(),
                thread_id: conversation_id.clone(),
                direction: 1,
                sender_did: owner_did.to_owned(),
                group_id: group_did.to_owned(),
                group_did: group_did.to_owned(),
                content_type: "application/json".to_owned(),
                content: serde_json::json!({
                    "text": "@agent 今天是几号了？"
                })
                .to_string(),
                server_seq: Some(6),
                sent_at: "2026-07-06T00:00:01Z".to_owned(),
                stored_at: "2026-07-06T00:00:01Z".to_owned(),
                is_read: true,
                metadata: serde_json::json!({
                    "operation_id": format!("op-{local_id}"),
                    "raw_message_id": local_id,
                    "group_event_seq": "6",
                    "delivery_state": "accepted"
                })
                .to_string(),
                ..MessageRecord::default()
            },
        )
        .unwrap();

        assert_eq!(message_ids(&db, owner_identity_id), vec![canonical_id]);
        let summary = summary_snapshot(&db, owner_identity_id, &conversation_id);
        assert_eq!(summary.message_count, 1);
        assert_eq!(summary.last_message_id, canonical_id);
        assert!(summary.last_content.contains("@agent 今天是几号了？"));
        assert_summary_matches_rebuild(&db, owner_identity_id, &conversation_id);
    }

    #[test]
    fn local_state_group_runtime_final_projection_preserves_canonical_content() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let owner_identity_id = "owner-id";
        let owner_did = "did:owner";
        let group_did = "did:example:groups:demo";
        let conversation_id =
            crate::internal::local_state::owner_scope::group_conversation_id(group_did);
        let canonical_id = "did:example:groups:demo:7";

        upsert_message(
            &db,
            &MessageRecord {
                msg_id: canonical_id.to_owned(),
                owner_identity_id: owner_identity_id.to_owned(),
                owner_did: owner_did.to_owned(),
                conversation_id: conversation_id.clone(),
                thread_id: conversation_id.clone(),
                direction: 0,
                sender_did: "did:example:agent".to_owned(),
                receiver_did: owner_did.to_owned(),
                group_id: group_did.to_owned(),
                group_did: group_did.to_owned(),
                content_type: "application/json".to_owned(),
                content: serde_json::json!({
                    "text": "@owner 今天是 2026-07-06。"
                })
                .to_string(),
                server_seq: Some(7),
                sent_at: "2026-07-06T00:00:02Z".to_owned(),
                stored_at: "2026-07-06T00:00:02Z".to_owned(),
                metadata: serde_json::json!({
                    "group_event_seq": "7"
                })
                .to_string(),
                ..MessageRecord::default()
            },
        )
        .unwrap();

        upsert_message(
            &db,
            &MessageRecord {
                msg_id: "runtime-final:agent:run-1".to_owned(),
                owner_identity_id: owner_identity_id.to_owned(),
                owner_did: owner_did.to_owned(),
                conversation_id: conversation_id.clone(),
                thread_id: conversation_id.clone(),
                direction: 0,
                sender_did: "did:example:agent".to_owned(),
                receiver_did: owner_did.to_owned(),
                group_id: group_did.to_owned(),
                group_did: group_did.to_owned(),
                content_type: "application/json".to_owned(),
                server_seq: Some(7),
                sent_at: "2026-07-06T00:00:02Z".to_owned(),
                stored_at: "2026-07-06T00:00:03Z".to_owned(),
                metadata: serde_json::json!({
                    "raw_message_id": "runtime-final:agent:run-1",
                    "group_event_seq": "7"
                })
                .to_string(),
                ..MessageRecord::default()
            },
        )
        .unwrap();

        assert_eq!(message_ids(&db, owner_identity_id), vec![canonical_id]);
        let summary = summary_snapshot(&db, owner_identity_id, &conversation_id);
        assert_eq!(summary.message_count, 1);
        assert_eq!(summary.last_message_id, canonical_id);
        assert!(summary.last_content.contains("2026-07-06"));
        assert_summary_matches_rebuild(&db, owner_identity_id, &conversation_id);
    }

    #[test]
    fn local_state_group_redaction_does_not_replace_decrypted_projection_metadata() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let group_did = "did:example:groups:secure";
        let canonical_id = "did:example:groups:secure:11";
        let base = MessageRecord {
            msg_id: canonical_id.to_owned(),
            owner_identity_id: "owner-id".to_owned(),
            owner_did: "did:owner".to_owned(),
            conversation_id: crate::internal::local_state::owner_scope::group_conversation_id(
                group_did,
            ),
            thread_id: crate::internal::local_state::owner_scope::group_conversation_id(group_did),
            direction: 0,
            sender_did: "did:example:sender".to_owned(),
            group_id: group_did.to_owned(),
            group_did: group_did.to_owned(),
            server_seq: Some(11),
            is_e2ee: true,
            ..MessageRecord::default()
        };

        upsert_message(
            &db,
            &MessageRecord {
                content_type: "text/plain".to_owned(),
                content: "decrypted".to_owned(),
                metadata: serde_json::json!({
                    "decryption_state": "decrypted",
                    "group_event_seq": "11"
                })
                .to_string(),
                ..base.clone()
            },
        )
        .unwrap();
        upsert_message(
            &db,
            &MessageRecord {
                content_type: "application/x-awiki-group-e2ee-redacted".to_owned(),
                metadata: serde_json::json!({
                    "decryption_state": "failed",
                    "group_event_seq": "11"
                })
                .to_string(),
                ..base
            },
        )
        .unwrap();

        let (content_type, content, metadata): (String, String, String) = db
            .query_row(
                "SELECT content_type, content, metadata FROM messages WHERE msg_id = ?1",
                [canonical_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(content_type, "text/plain");
        assert_eq!(content, "decrypted");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&metadata).unwrap()["decryption_state"],
            "decrypted"
        );
    }

    #[test]
    fn local_state_group_identity_repair_collapses_existing_echo_duplicates() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let owner_identity_id = "owner-id";
        let owner_did = "did:owner";
        let group_did = "did:example:groups:demo";
        let conversation_id =
            crate::internal::local_state::owner_scope::group_conversation_id(group_did);
        let local_id = "msg-awiki-me-local-2";
        let canonical_id = "did:example:groups:demo:8";

        raw_insert_message(
            &db,
            &MessageRecord {
                msg_id: local_id.to_owned(),
                owner_identity_id: owner_identity_id.to_owned(),
                owner_did: owner_did.to_owned(),
                conversation_id: conversation_id.clone(),
                thread_id: conversation_id.clone(),
                direction: 1,
                sender_did: owner_did.to_owned(),
                group_id: group_did.to_owned(),
                group_did: group_did.to_owned(),
                content_type: "text/plain".to_owned(),
                content: "hello group".to_owned(),
                stored_at: "2026-07-06T00:00:04Z".to_owned(),
                is_read: true,
                metadata: serde_json::json!({
                    "operation_id": format!("op-{local_id}")
                })
                .to_string(),
                ..MessageRecord::default()
            },
        );
        raw_insert_message(
            &db,
            &MessageRecord {
                msg_id: canonical_id.to_owned(),
                owner_identity_id: owner_identity_id.to_owned(),
                owner_did: owner_did.to_owned(),
                conversation_id: conversation_id.clone(),
                thread_id: conversation_id.clone(),
                direction: 1,
                sender_did: owner_did.to_owned(),
                group_id: group_did.to_owned(),
                group_did: group_did.to_owned(),
                content_type: "text/plain".to_owned(),
                content: "hello group".to_owned(),
                server_seq: Some(8),
                sent_at: "2026-07-06T00:00:05Z".to_owned(),
                stored_at: "2026-07-06T00:00:05Z".to_owned(),
                is_read: true,
                metadata: serde_json::json!({
                    "raw_message_id": local_id,
                    "group_event_seq": "8"
                })
                .to_string(),
                ..MessageRecord::default()
            },
        );
        crate::internal::local_state::conversation_summaries::rebuild_all(&db).unwrap();
        assert_eq!(
            summary_message_count(&db, owner_identity_id, &conversation_id),
            2
        );

        let repaired = repair_group_message_identity_projection(&db).unwrap();

        assert_eq!(repaired, 1);
        assert_eq!(message_ids(&db, owner_identity_id), vec![canonical_id]);
        let summary = summary_snapshot(&db, owner_identity_id, &conversation_id);
        assert_eq!(summary.message_count, 1);
        assert_eq!(summary.last_message_id, canonical_id);
        assert_summary_matches_rebuild(&db, owner_identity_id, &conversation_id);
    }

    #[test]
    fn local_state_group_identity_alias_marks_canonical_message_read() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let owner_identity_id = "owner-id";
        let owner_did = "did:owner";
        let group_did = "did:example:groups:demo";
        let conversation_id =
            crate::internal::local_state::owner_scope::group_conversation_id(group_did);
        let local_id = "msg-awiki-me-local-read";
        let canonical_id = "did:example:groups:demo:9";

        upsert_message(
            &db,
            &MessageRecord {
                msg_id: canonical_id.to_owned(),
                owner_identity_id: owner_identity_id.to_owned(),
                owner_did: owner_did.to_owned(),
                conversation_id: conversation_id.clone(),
                thread_id: conversation_id.clone(),
                direction: 0,
                sender_did: "did:example:agent".to_owned(),
                receiver_did: owner_did.to_owned(),
                group_id: group_did.to_owned(),
                group_did: group_did.to_owned(),
                content_type: "text/plain".to_owned(),
                content: "reply".to_owned(),
                server_seq: Some(9),
                sent_at: "2026-07-06T00:00:09Z".to_owned(),
                stored_at: "2026-07-06T00:00:09Z".to_owned(),
                metadata: serde_json::json!({
                    "raw_message_id": local_id,
                    "group_event_seq": "9"
                })
                .to_string(),
                ..MessageRecord::default()
            },
        )
        .unwrap();
        assert_eq!(read_by_msg_id(&db, canonical_id), 0);
        assert_eq!(
            summary_snapshot(&db, owner_identity_id, &conversation_id).unread_count,
            1
        );

        let classification = classify_mark_read_ids_for_owner_identity(
            &db,
            owner_identity_id,
            owner_did,
            &[local_id.to_owned()],
        )
        .unwrap();
        assert_eq!(classification.group_ids, vec![local_id]);
        assert_eq!(classification.local_ids(), vec![local_id]);
        let rows = mark_messages_read_for_owner_identity(
            &db,
            owner_identity_id,
            owner_did,
            &classification.local_ids(),
        )
        .unwrap();

        assert_eq!(rows, 1);
        assert_eq!(read_by_msg_id(&db, canonical_id), 1);
        assert_eq!(
            summary_snapshot(&db, owner_identity_id, &conversation_id).unread_count,
            0
        );
        assert_summary_matches_rebuild(&db, owner_identity_id, &conversation_id);
    }

    #[test]
    fn local_state_group_watermark_accepts_alias_message_id() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let owner_identity_id = "owner-id";
        let owner_did = "did:owner";
        let group_did = "did:example:groups:demo";
        let conversation_id =
            crate::internal::local_state::owner_scope::group_conversation_id(group_did);
        let local_id = "msg-awiki-me-local-watermark";
        let canonical_id = "did:example:groups:demo:10";

        upsert_message(
            &db,
            &MessageRecord {
                msg_id: canonical_id.to_owned(),
                owner_identity_id: owner_identity_id.to_owned(),
                owner_did: owner_did.to_owned(),
                conversation_id: conversation_id.clone(),
                thread_id: conversation_id.clone(),
                direction: 0,
                sender_did: "did:example:agent".to_owned(),
                receiver_did: owner_did.to_owned(),
                group_id: group_did.to_owned(),
                group_did: group_did.to_owned(),
                content_type: "text/plain".to_owned(),
                content: "reply".to_owned(),
                server_seq: Some(10),
                sent_at: "2026-07-06T00:00:10Z".to_owned(),
                stored_at: "2026-07-06T00:00:10Z".to_owned(),
                metadata: serde_json::json!({
                    "raw_message_id": local_id,
                    "group_event_seq": "10"
                })
                .to_string(),
                ..MessageRecord::default()
            },
        )
        .unwrap();

        let result = mark_thread_read_watermark_for_owner_identity(
            &db,
            owner_identity_id,
            owner_did,
            MarkThreadReadWatermarkInput {
                thread: crate::messages::ThreadRef::Group(
                    crate::ids::GroupRef::parse(group_did).unwrap(),
                ),
                read_watermark_message_id: Some(local_id.to_owned()),
                read_watermark_seq: None,
                read_watermark_at: Some("2026-07-06T00:01:00Z".to_owned()),
                pending_remote_ack: false,
            },
        )
        .unwrap();

        assert_eq!(result.updated_count, 1);
        assert_eq!(result.read_watermark_seq.as_deref(), Some("10"));
        assert_eq!(
            result.read_watermark_message_id.as_deref(),
            Some(canonical_id)
        );
        assert_eq!(read_by_msg_id(&db, canonical_id), 1);
        assert_summary_matches_rebuild(&db, owner_identity_id, &conversation_id);
    }

    #[test]
    fn local_state_control_payloads_do_not_materialize_conversation_summaries() {
        let db = Connection::open_in_memory().unwrap();
        let owner_identity_id = "owner-id";
        let owner_did = "did:owner";
        let conversation_id = "dm:did:daemon";

        let mut control = summary_test_message(
            "msg-control",
            owner_identity_id,
            owner_did,
            conversation_id,
            0,
            &serde_json::json!({
                "schema": "awiki.agent.status.v1",
                "message": "daemon heartbeat"
            })
            .to_string(),
            "2026-06-30T00:00:00Z",
            false,
        );
        control.content_type = "application/json".to_owned();
        upsert_message(&db, &control).unwrap();

        assert_eq!(read_by_msg_id(&db, "msg-control"), 1);
        assert!(!summary_exists(&db, owner_identity_id, conversation_id));
        super::super::conversation_summaries::rebuild_owner(&db, owner_identity_id).unwrap();
        assert!(!summary_exists(&db, owner_identity_id, conversation_id));
    }

    #[test]
    fn local_state_plain_json_payloads_still_count_unread() {
        let db = Connection::open_in_memory().unwrap();
        let owner_identity_id = "owner-id";
        let owner_did = "did:owner";
        let conversation_id = "dm:did:peer";

        let mut plain_json = summary_test_message(
            "msg-json",
            owner_identity_id,
            owner_did,
            conversation_id,
            0,
            &serde_json::json!({
                "text": "普通 JSON 消息",
                "schema": "example.chat.message.v1"
            })
            .to_string(),
            "2026-06-30T00:00:00Z",
            false,
        );
        plain_json.content_type = "application/json".to_owned();
        upsert_message(&db, &plain_json).unwrap();

        assert_eq!(read_by_msg_id(&db, "msg-json"), 0);
        let summary = summary_snapshot(&db, owner_identity_id, conversation_id);
        assert_eq!(summary.message_count, 1);
        assert_eq!(summary.unread_count, 1);
        assert_summary_matches_rebuild(&db, owner_identity_id, conversation_id);
    }

    #[test]
    fn local_state_daemon_empty_json_payloads_do_not_count_unread() {
        let db = Connection::open_in_memory().unwrap();
        let owner_identity_id = "owner-id";
        let owner_did = "did:owner";
        let conversation_id = "dm:peer-scope:v1:daemon";

        let mut control = summary_test_message(
            "msg-empty-daemon-control",
            owner_identity_id,
            owner_did,
            conversation_id,
            0,
            "",
            "2026-06-30T00:00:00Z",
            false,
        );
        control.content_type = "application/json".to_owned();
        control.sender_did = "did:wba:awiki.info:agent:daemon:edgehost_1:e1_owner".to_owned();
        upsert_message(&db, &control).unwrap();

        assert_eq!(read_by_msg_id(&db, "msg-empty-daemon-control"), 1);
        assert!(!summary_exists(&db, owner_identity_id, conversation_id));
        super::super::conversation_summaries::rebuild_owner(&db, owner_identity_id).unwrap();
        assert!(!summary_exists(&db, owner_identity_id, conversation_id));
    }

    #[test]
    fn local_state_empty_json_payloads_do_not_count_unread() {
        let db = Connection::open_in_memory().unwrap();
        let owner_identity_id = "owner-id";
        let owner_did = "did:owner";
        let conversation_id = "dm:did:peer";

        let mut plain_json = summary_test_message(
            "msg-empty-json",
            owner_identity_id,
            owner_did,
            conversation_id,
            0,
            "",
            "2026-06-30T00:00:00Z",
            false,
        );
        plain_json.content_type = "application/json".to_owned();
        plain_json.sender_did = "did:wba:awiki.info:agent:runtime:cgw001:e1_peer".to_owned();
        upsert_message(&db, &plain_json).unwrap();

        assert_eq!(read_by_msg_id(&db, "msg-empty-json"), 1);
        assert!(!summary_exists(&db, owner_identity_id, conversation_id));
        super::super::conversation_summaries::rebuild_owner(&db, owner_identity_id).unwrap();
        assert!(!summary_exists(&db, owner_identity_id, conversation_id));
    }

    #[test]
    fn local_state_repairs_existing_control_payload_unread() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let owner_identity_id = "owner-id";
        let owner_did = "did:owner";
        let conversation_id = "dm:did:daemon";
        let content = serde_json::json!({
            "schema": "awiki.agent.status.v1",
            "message": "daemon heartbeat"
        })
        .to_string();
        db.execute(
            r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, conversation_id, thread_id, direction,
     sender_did, receiver_did, content_type, content, sent_at, stored_at, is_read)
VALUES ('msg-legacy-control', ?1, ?2, ?3, ?3, 0,
        'did:daemon', ?2, 'application/json', ?4,
        '2026-06-30T00:00:00Z', '2026-06-30T00:00:00Z', 0)"#,
            rusqlite::params![owner_identity_id, owner_did, conversation_id, content],
        )
        .unwrap();
        super::super::conversation_summaries::rebuild_owner(&db, owner_identity_id).unwrap();
        assert!(!summary_exists(&db, owner_identity_id, conversation_id));
        assert_eq!(read_by_msg_id(&db, "msg-legacy-control"), 0);

        let repaired = repair_control_payload_read_projection(&db).unwrap();
        assert_eq!(repaired, 1);
        assert_eq!(read_by_msg_id(&db, "msg-legacy-control"), 1);
        assert!(!summary_exists(&db, owner_identity_id, conversation_id));
    }

    #[test]
    fn local_state_repairs_existing_daemon_empty_json_unread() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let owner_identity_id = "owner-id";
        let owner_did = "did:owner";
        let conversation_id = "dm:peer-scope:v1:daemon";
        db.execute(
            r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, conversation_id, thread_id, direction,
     sender_did, receiver_did, content_type, content, sent_at, stored_at, is_read)
VALUES ('msg-legacy-empty-daemon', ?1, ?2, ?3, ?3, 0,
        'did:wba:awiki.info:agent:daemon:edgehost_1:e1_owner', ?2, 'application/json', '',
        '2026-06-30T00:00:00Z', '2026-06-30T00:00:00Z', 0)"#,
            rusqlite::params![owner_identity_id, owner_did, conversation_id],
        )
        .unwrap();
        super::super::conversation_summaries::rebuild_owner(&db, owner_identity_id).unwrap();
        assert!(!summary_exists(&db, owner_identity_id, conversation_id));
        assert_eq!(read_by_msg_id(&db, "msg-legacy-empty-daemon"), 0);

        let repaired = repair_control_payload_read_projection(&db).unwrap();
        assert_eq!(repaired, 1);
        assert_eq!(read_by_msg_id(&db, "msg-legacy-empty-daemon"), 1);
        assert!(!summary_exists(&db, owner_identity_id, conversation_id));
    }

    #[test]
    fn local_state_repairs_existing_empty_json_unread() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let owner_identity_id = "owner-id";
        let owner_did = "did:owner";
        let conversation_id = "dm:did:peer";
        db.execute(
            r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, conversation_id, thread_id, direction,
     sender_did, receiver_did, content_type, content, sent_at, stored_at, is_read)
VALUES ('msg-legacy-empty-json', ?1, ?2, ?3, ?3, 0,
        'did:wba:awiki.info:agent:runtime:cgw001:e1_peer', ?2, 'application/json', '',
        '2026-06-30T00:00:00Z', '2026-06-30T00:00:00Z', 0)"#,
            rusqlite::params![owner_identity_id, owner_did, conversation_id],
        )
        .unwrap();
        super::super::conversation_summaries::rebuild_owner(&db, owner_identity_id).unwrap();
        assert!(!summary_exists(&db, owner_identity_id, conversation_id));
        assert_eq!(read_by_msg_id(&db, "msg-legacy-empty-json"), 0);

        let repaired = repair_control_payload_read_projection(&db).unwrap();
        assert_eq!(repaired, 1);
        assert_eq!(read_by_msg_id(&db, "msg-legacy-empty-json"), 1);
        assert!(!summary_exists(&db, owner_identity_id, conversation_id));
    }

    #[test]
    fn local_state_reopen_does_not_repair_discovered_json_as_control() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("im.sqlite");
        {
            let db = Connection::open(&path).unwrap();
            upsert_message(
                &db,
                &MessageRecord {
                    msg_id: "msg-discovered-json".to_owned(),
                    owner_identity_id: "owner-id".to_owned(),
                    owner_did: "did:example:owner".to_owned(),
                    conversation_id: "dm:did:example:peer".to_owned(),
                    thread_id: "dm:did:example:peer".to_owned(),
                    direction: 0,
                    sender_did: "did:example:peer".to_owned(),
                    receiver_did: "did:example:owner".to_owned(),
                    content_type: "application/json".to_owned(),
                    server_seq: Some(72),
                    hydration_state: MessageHydrationState::Discovered,
                    stored_at: "2026-07-24T00:00:00Z".to_owned(),
                    ..MessageRecord::default()
                },
            )
            .unwrap();
            assert_eq!(read_by_msg_id(&db, "msg-discovered-json"), 0);
        }

        let reopened = Connection::open(&path).unwrap();
        crate::internal::local_state::schema::ensure_schema(&reopened).unwrap();
        assert_eq!(
            repair_control_payload_read_projection(&reopened).unwrap(),
            0
        );
        assert_eq!(read_by_msg_id(&reopened, "msg-discovered-json"), 0);
        assert_eq!(
            reopened
                .query_row(
                    "SELECT hydration_state FROM messages WHERE msg_id = 'msg-discovered-json'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "discovered"
        );
    }

    #[test]
    fn local_state_messages_mark_read_delta_matches_rebuild() {
        let db = Connection::open_in_memory().unwrap();
        let owner_identity_id = "owner-id";
        let owner_did = "did:owner";
        let conversation_id = "dm:did:peer";
        for (msg_id, sent_at) in [
            ("msg-unread-1", "2026-06-27T00:00:01Z"),
            ("msg-unread-2", "2026-06-27T00:00:02Z"),
        ] {
            upsert_message(
                &db,
                &summary_test_message(
                    msg_id,
                    owner_identity_id,
                    owner_did,
                    conversation_id,
                    0,
                    msg_id,
                    sent_at,
                    false,
                ),
            )
            .unwrap();
        }
        assert_eq!(
            summary_snapshot(&db, owner_identity_id, conversation_id).unread_count,
            2
        );

        let rows = mark_messages_read_for_owner_identity(
            &db,
            owner_identity_id,
            owner_did,
            &["msg-unread-1".to_owned()],
        )
        .unwrap();
        assert_eq!(rows, 1);
        assert_eq!(
            summary_snapshot(&db, owner_identity_id, conversation_id).unread_count,
            1
        );
        assert_summary_matches_rebuild(&db, owner_identity_id, conversation_id);

        let rows = mark_messages_read_for_owner_identity(
            &db,
            owner_identity_id,
            owner_did,
            &["msg-unread-2".to_owned()],
        )
        .unwrap();
        assert_eq!(rows, 1);
        assert_eq!(
            summary_snapshot(&db, owner_identity_id, conversation_id).unread_count,
            0
        );
        assert_summary_matches_rebuild(&db, owner_identity_id, conversation_id);
    }

    #[test]
    fn local_state_messages_unread_mention_fallback_preserves_first_mention() {
        let db = Connection::open_in_memory().unwrap();
        let owner_identity_id = "owner-id";
        let owner_did = "did:owner";
        let conversation_id = "group:mentions";

        upsert_message(
            &db,
            &summary_test_message(
                "plain-newer",
                owner_identity_id,
                owner_did,
                conversation_id,
                0,
                "plain unread",
                "2026-06-27T00:00:03Z",
                false,
            ),
        )
        .unwrap();
        upsert_message(
            &db,
            &mention_test_message(
                "mention-later",
                owner_identity_id,
                owner_did,
                conversation_id,
                "2026-06-27T00:00:02Z",
            ),
        )
        .unwrap();
        let summary = summary_snapshot(&db, owner_identity_id, conversation_id);
        assert_eq!(summary.unread_count, 2);
        assert_eq!(summary.unread_mention_count, 1);
        assert_eq!(summary.first_unread_mention_message_id, "mention-later");
        assert_summary_matches_rebuild(&db, owner_identity_id, conversation_id);

        upsert_message(
            &db,
            &mention_test_message(
                "mention-earlier",
                owner_identity_id,
                owner_did,
                conversation_id,
                "2026-06-27T00:00:01Z",
            ),
        )
        .unwrap();
        let summary = summary_snapshot(&db, owner_identity_id, conversation_id);
        assert_eq!(summary.message_count, 3);
        assert_eq!(summary.unread_mention_count, 2);
        assert_eq!(summary.first_unread_mention_message_id, "mention-earlier");
        assert_summary_matches_rebuild(&db, owner_identity_id, conversation_id);

        mark_messages_read_for_owner_identity(
            &db,
            owner_identity_id,
            owner_did,
            &["mention-earlier".to_owned()],
        )
        .unwrap();
        let summary = summary_snapshot(&db, owner_identity_id, conversation_id);
        assert_eq!(summary.unread_mention_count, 1);
        assert_eq!(summary.first_unread_mention_message_id, "mention-later");
        assert_summary_matches_rebuild(&db, owner_identity_id, conversation_id);
    }

    #[test]
    fn local_state_messages_upsert_normalizes_legacy_direct_thread_alias() {
        let db = Connection::open_in_memory().unwrap();
        upsert_message(
            &db,
            &MessageRecord {
                msg_id: "msg-legacy-thread".to_owned(),
                owner_identity_id: "owner-id".to_owned(),
                owner_did: "did:owner".to_owned(),
                thread_id: "dm:did:owner:did:peer".to_owned(),
                direction: 0,
                content: "legacy alias".to_owned(),
                stored_at: "2026-01-01T00:00:00Z".to_owned(),
                ..MessageRecord::default()
            },
        )
        .unwrap();

        let (conversation_id, thread_id): (String, String) = db
            .query_row(
                "SELECT conversation_id, thread_id FROM messages WHERE msg_id = 'msg-legacy-thread'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();

        assert_eq!(conversation_id, "dm:did:peer");
        assert_eq!(thread_id, conversation_id);
    }

    #[test]
    fn local_state_messages_memoizes_legacy_direct_merge_and_rewrites_late_legacy_rows() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let owner_identity_id = "owner-id";
        let owner_did = "did:wba:anpclaw.com:zhuocheng:e1_owner";
        let peer_old_did = "did:wba:anpclaw.com:zhuochengtest:e1_old";
        let peer_new_did = "did:wba:anpclaw.com:zhuochengtest:e1_new";
        let legacy_conversation_id =
            crate::internal::local_state::owner_scope::direct_conversation_id(peer_old_did);
        let scoped_conversation_id = scoped_zhuochengtest_conversation_id();
        seed_verified_peer_identity(
            &db,
            owner_identity_id,
            owner_did,
            &[peer_old_did, peer_new_did],
        );

        seed_message_row(
            &db,
            "msg-old",
            owner_identity_id,
            owner_did,
            &legacy_conversation_id,
            0,
            peer_old_did,
            owner_did,
            "",
            1,
            "2026-06-10T00:00:00Z",
        );

        upsert_message(
            &db,
            &peer_scope_message_record(
                "msg-new-1",
                owner_identity_id,
                owner_did,
                &scoped_conversation_id,
                peer_new_did,
                "2026-06-10T00:00:01Z",
            ),
        )
        .unwrap();

        assert_eq!(
            legacy_merge_memo_stats(&db, owner_identity_id, &scoped_conversation_id),
            Some((1, 1))
        );
        assert_eq!(
            legacy_memo_target_for_legacy_id(&db, owner_identity_id, &legacy_conversation_id),
            Some(scoped_conversation_id.clone())
        );

        for index in 2..=100 {
            let minute = index / 60;
            let second = index % 60;
            upsert_message(
                &db,
                &peer_scope_message_record(
                    &format!("msg-new-{index}"),
                    owner_identity_id,
                    owner_did,
                    &scoped_conversation_id,
                    peer_new_did,
                    &format!("2026-06-10T00:{minute:02}:{second:02}Z"),
                ),
            )
            .unwrap();
        }

        assert_eq!(
            legacy_merge_memo_stats(&db, owner_identity_id, &scoped_conversation_id),
            Some((1, 1))
        );

        upsert_message(
            &db,
            &MessageRecord {
                msg_id: "msg-late-legacy".to_owned(),
                owner_identity_id: owner_identity_id.to_owned(),
                owner_did: owner_did.to_owned(),
                conversation_id: legacy_conversation_id.clone(),
                thread_id: legacy_conversation_id.clone(),
                direction: 0,
                sender_did: peer_old_did.to_owned(),
                receiver_did: owner_did.to_owned(),
                content_type: "text/plain".to_owned(),
                content: "late legacy projection".to_owned(),
                stored_at: "2026-06-10T00:02:00Z".to_owned(),
                is_read: true,
                ..MessageRecord::default()
            },
        )
        .unwrap();

        assert_eq!(
            legacy_merge_memo_stats(&db, owner_identity_id, &scoped_conversation_id),
            Some((1, 1))
        );
        assert_eq!(
            conversation_ids_for_owner(&db, owner_identity_id),
            vec![scoped_conversation_id.clone()]
        );
        assert_eq!(
            summary_message_count(&db, owner_identity_id, &scoped_conversation_id),
            102
        );
    }

    #[test]
    fn local_state_messages_legacy_direct_merge_memo_is_owner_scoped() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let scoped_conversation_id = scoped_zhuochengtest_conversation_id();
        let peer_old_did = "did:wba:anpclaw.com:zhuochengtest:e1_old";
        let peer_new_did = "did:wba:anpclaw.com:zhuochengtest:e1_new";
        let legacy_conversation_id =
            crate::internal::local_state::owner_scope::direct_conversation_id(peer_old_did);

        for (owner_identity_id, owner_did, old_msg, new_msg) in [
            (
                "owner-1-id",
                "did:wba:anpclaw.com:ownerone:e1_owner",
                "owner-1-old",
                "owner-1-new",
            ),
            (
                "owner-2-id",
                "did:wba:anpclaw.com:ownertwo:e1_owner",
                "owner-2-old",
                "owner-2-new",
            ),
        ] {
            seed_verified_peer_identity(
                &db,
                owner_identity_id,
                owner_did,
                &[peer_old_did, peer_new_did],
            );
            seed_message_row(
                &db,
                old_msg,
                owner_identity_id,
                owner_did,
                &legacy_conversation_id,
                0,
                peer_old_did,
                owner_did,
                "",
                1,
                "2026-06-10T00:00:00Z",
            );
            upsert_message(
                &db,
                &peer_scope_message_record(
                    new_msg,
                    owner_identity_id,
                    owner_did,
                    &scoped_conversation_id,
                    peer_new_did,
                    "2026-06-10T00:00:01Z",
                ),
            )
            .unwrap();
        }

        for owner_identity_id in ["owner-1-id", "owner-2-id"] {
            assert_eq!(
                legacy_merge_memo_stats(&db, owner_identity_id, &scoped_conversation_id),
                Some((1, 1))
            );
            assert_eq!(
                conversation_ids_for_owner(&db, owner_identity_id),
                vec![scoped_conversation_id.clone()]
            );
            assert_eq!(
                summary_message_count(&db, owner_identity_id, &scoped_conversation_id),
                2
            );
        }
    }

    #[test]
    fn local_state_messages_legacy_direct_merge_memo_skips_group_rows() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let owner_identity_id = "owner-id";
        let owner_did = "did:wba:anpclaw.com:zhuocheng:e1_owner";
        let peer_old_did = "did:wba:anpclaw.com:zhuochengtest:e1_old";
        let legacy_conversation_id =
            crate::internal::local_state::owner_scope::direct_conversation_id(peer_old_did);
        let scoped_conversation_id = scoped_zhuochengtest_conversation_id();

        seed_message_row(
            &db,
            "msg-old",
            owner_identity_id,
            owner_did,
            &legacy_conversation_id,
            0,
            peer_old_did,
            owner_did,
            "",
            1,
            "2026-06-10T00:00:00Z",
        );

        upsert_message(
            &db,
            &MessageRecord {
                msg_id: "msg-group".to_owned(),
                owner_identity_id: owner_identity_id.to_owned(),
                owner_did: owner_did.to_owned(),
                conversation_id: scoped_conversation_id.clone(),
                thread_id: scoped_conversation_id.clone(),
                direction: 0,
                sender_did: "did:wba:anpclaw.com:someone:e1_sender".to_owned(),
                receiver_did: owner_did.to_owned(),
                group_id: "group-1".to_owned(),
                group_did: "did:wba:anpclaw.com:groups:e1_group".to_owned(),
                content_type: "text/plain".to_owned(),
                content: "group projection".to_owned(),
                stored_at: "2026-06-10T00:00:01Z".to_owned(),
                metadata: r#"{"peer_full_handle":"zhuochengtest.anpclaw.com"}"#.to_owned(),
                ..MessageRecord::default()
            },
        )
        .unwrap();

        assert_eq!(
            legacy_merge_memo_stats(&db, owner_identity_id, &scoped_conversation_id),
            None
        );
        assert_eq!(
            conversation_ids_for_owner(&db, owner_identity_id),
            vec![legacy_conversation_id, scoped_conversation_id]
        );
    }

    #[test]
    fn local_state_messages_merge_rotated_did_direct_rows_into_peer_scope() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let owner_did = "did:wba:anpclaw.com:zhuocheng:e1_owner";
        let old_peer_did = "did:wba:anpclaw.com:zhuochengtest:e1_old";
        let legacy_conversation_id =
            crate::internal::local_state::owner_scope::direct_conversation_id(old_peer_did);
        upsert_message(
            &db,
            &MessageRecord {
                msg_id: "msg-old".to_owned(),
                owner_identity_id: "owner-id".to_owned(),
                owner_did: owner_did.to_owned(),
                conversation_id: legacy_conversation_id.clone(),
                thread_id: legacy_conversation_id,
                direction: 0,
                sender_did: old_peer_did.to_owned(),
                receiver_did: owner_did.to_owned(),
                content_type: "text/plain".to_owned(),
                content: "old did message".to_owned(),
                sent_at: "2026-06-10T00:00:00Z".to_owned(),
                stored_at: "2026-06-10T00:00:00Z".to_owned(),
                is_read: true,
                ..MessageRecord::default()
            },
        )
        .unwrap();
        let new_peer_did = "did:wba:anpclaw.com:zhuochengtest:e1_new";
        seed_verified_peer_identity(&db, "owner-id", owner_did, &[old_peer_did, new_peer_did]);
        let scoped_conversation_id = scoped_zhuochengtest_conversation_id();

        upsert_message(
            &db,
            &MessageRecord {
                msg_id: "msg-new".to_owned(),
                owner_identity_id: "owner-id".to_owned(),
                owner_did: "did:wba:anpclaw.com:zhuocheng:e1_owner".to_owned(),
                conversation_id: scoped_conversation_id.clone(),
                thread_id: scoped_conversation_id.clone(),
                direction: 1,
                sender_did: "did:wba:anpclaw.com:zhuocheng:e1_owner".to_owned(),
                receiver_did: new_peer_did.to_owned(),
                content_type: "text/plain".to_owned(),
                content: "new scoped message".to_owned(),
                stored_at: "2026-06-10T00:00:01Z".to_owned(),
                metadata: r#"{"peer_user_id":"peer-user-id","peer_full_handle":"zhuochengtest.anpclaw.com","peer_current_did":"did:wba:anpclaw.com:zhuochengtest:e1_new"}"#.to_owned(),
                ..MessageRecord::default()
            },
        )
        .unwrap();

        let conversations = db
            .prepare(
                "SELECT DISTINCT conversation_id FROM messages WHERE owner_identity_id = 'owner-id' ORDER BY conversation_id",
            )
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(conversations, vec![scoped_conversation_id.clone()]);
        let (conversation_id, message_count): (String, i64) = db
            .query_row(
                "SELECT conversation_id, message_count FROM threads WHERE owner_identity_id = 'owner-id'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(conversation_id, scoped_conversation_id);
        assert_eq!(message_count, 2);
        let (summary_conversation_id, summary_count): (String, i64) = db
            .query_row(
                "SELECT conversation_id, message_count FROM conversation_summaries WHERE owner_identity_id = 'owner-id'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(summary_conversation_id, scoped_conversation_id);
        assert_eq!(summary_count, 2);
        let old_wire: (String, String, String, String) = db
            .query_row(
                r#"SELECT conversation_id, thread_id, wire_thread_kind, wire_thread_ref
FROM messages WHERE owner_identity_id = 'owner-id' AND msg_id = 'msg-old'"#,
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(old_wire.0, scoped_conversation_id);
        assert_eq!(old_wire.1, "dm:did:wba:anpclaw.com:zhuochengtest:e1_old");
        assert_eq!(old_wire.2, "direct");
        assert_eq!(old_wire.3, old_peer_did);
        let registry_rows = db
            .prepare(
                "SELECT conversation_id, is_active, lifecycle_state, resolution_state, COALESCE(merged_into_conversation_id, '') FROM conversation_registry WHERE owner_identity_id = 'owner-id' ORDER BY conversation_id",
            )
            .unwrap()
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            registry_rows,
            vec![
                (
                    "dm:did:wba:anpclaw.com:zhuochengtest:e1_old".to_owned(),
                    0,
                    "merged".to_owned(),
                    "resolved".to_owned(),
                    scoped_conversation_id.clone(),
                ),
                (
                    summary_conversation_id,
                    1,
                    "active".to_owned(),
                    "resolved".to_owned(),
                    String::new(),
                ),
            ]
        );
        assert_eq!(
            crate::internal::local_state::conversation_aliases::resolve(
                &db,
                "owner-id",
                "legacy_direct_did",
                "dm:did:wba:anpclaw.com:zhuochengtest:e1_old",
            )
            .unwrap(),
            Some(scoped_conversation_id.clone())
        );

        upsert_message(
            &db,
            &MessageRecord {
                msg_id: "msg-late-legacy".to_owned(),
                owner_identity_id: "owner-id".to_owned(),
                owner_did: owner_did.to_owned(),
                conversation_id: crate::internal::local_state::owner_scope::direct_conversation_id(
                    old_peer_did,
                ),
                thread_id: crate::internal::local_state::owner_scope::direct_conversation_id(
                    old_peer_did,
                ),
                direction: 0,
                sender_did: old_peer_did.to_owned(),
                receiver_did: owner_did.to_owned(),
                content_type: "text/plain".to_owned(),
                content: "late old did message".to_owned(),
                sent_at: "2026-06-10T00:00:02Z".to_owned(),
                stored_at: "2026-06-10T00:00:02Z".to_owned(),
                ..MessageRecord::default()
            },
        )
        .unwrap();
        let active_ids = db
            .prepare(
                "SELECT conversation_id FROM conversation_registry WHERE owner_identity_id = 'owner-id' AND is_active = 1 ORDER BY conversation_id",
            )
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(active_ids, vec![scoped_conversation_id]);
    }

    #[test]
    fn local_state_messages_do_not_merge_legacy_rows_when_peer_user_id_is_did_fallback() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let owner_identity_id = "owner-id";
        let owner_did = "did:wba:anpclaw.com:zhuocheng:e1_owner";
        let peer_old_did = "did:wba:anpclaw.com:zhuochengtest:e1_old";
        let peer_current_did = "did:wba:anpclaw.com:zhuochengtest:e1_new";
        let full_handle = "zhuochengtest.anpclaw.com";
        let legacy_conversation_id =
            crate::internal::local_state::owner_scope::direct_conversation_id(peer_old_did);
        db.execute(
            r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, conversation_id, thread_id, direction, sender_did, receiver_did,
     content_type, content, sent_at, stored_at, is_read)
VALUES (?1, ?2, ?3, ?4, ?4, 0, ?5, ?3,
        'text/plain', 'old did message', '2026-06-10T00:00:00Z', '2026-06-10T00:00:00Z', 1)"#,
            (
                "msg-old",
                owner_identity_id,
                owner_did,
                &legacy_conversation_id,
                peer_old_did,
            ),
        )
        .unwrap();
        let fallback_scope = crate::internal::local_state::owner_scope::DirectPeerScope::new(
            peer_current_did,
            full_handle,
        )
        .unwrap();
        let scoped_conversation_id =
            crate::internal::local_state::owner_scope::direct_conversation_id_for_peer_scope(
                &fallback_scope,
            );

        upsert_message(
            &db,
            &MessageRecord {
                msg_id: "msg-new".to_owned(),
                owner_identity_id: owner_identity_id.to_owned(),
                owner_did: owner_did.to_owned(),
                conversation_id: scoped_conversation_id.clone(),
                thread_id: scoped_conversation_id.clone(),
                direction: 1,
                sender_did: owner_did.to_owned(),
                receiver_did: peer_current_did.to_owned(),
                content_type: "text/plain".to_owned(),
                content: "new scoped message".to_owned(),
                stored_at: "2026-06-10T00:00:01Z".to_owned(),
                metadata: format!(
                    r#"{{"peer_user_id":"{peer_current_did}","peer_full_handle":"{full_handle}","peer_current_did":"{peer_current_did}"}}"#
                ),
                ..MessageRecord::default()
            },
        )
        .unwrap();

        assert_eq!(
            conversation_ids_for_owner(&db, owner_identity_id),
            vec![legacy_conversation_id, scoped_conversation_id]
        );
    }

    fn scoped_zhuochengtest_conversation_id() -> String {
        let scope = crate::internal::local_state::owner_scope::DirectPeerScope::new(
            "peer-user-id",
            "zhuochengtest.anpclaw.com",
        )
        .unwrap();
        crate::internal::local_state::owner_scope::direct_conversation_id_for_peer_scope(&scope)
    }

    fn seed_verified_peer_identity(
        db: &Connection,
        owner_identity_id: &str,
        owner_did: &str,
        peer_dids: &[&str],
    ) {
        let persona = crate::internal::canonical_identity::PeerPersona::from_verified_handle(
            "anpclaw.com",
            "peer-user-id",
            "zhuochengtest.anpclaw.com",
            Some("active"),
        )
        .unwrap();
        crate::internal::local_state::peer_personas::upsert(
            db,
            &crate::internal::local_state::peer_personas::PeerPersonaRecord {
                owner_identity_id: owner_identity_id.to_owned(),
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
                db,
                &crate::internal::local_state::peer_identifiers::PeerIdentifierRecord {
                    owner_identity_id: owner_identity_id.to_owned(),
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
            owner_identity_id,
            &persona,
            peer_dids.last().copied().unwrap(),
        )
        .unwrap();
        crate::internal::local_state::direct_peer_routes::upsert(db, &route).unwrap();
        crate::internal::local_state::conversation_registry::ensure(
            db,
            &crate::internal::local_state::conversation_registry::ConversationRegistryRecord {
                owner_identity_id: owner_identity_id.to_owned(),
                owner_did: owner_did.to_owned(),
                conversation_id: persona.direct_conversation_id(),
                thread_kind: "direct".to_owned(),
                thread_id: persona.direct_conversation_id(),
                activity_at: "2026-07-14T00:00:00Z".to_owned(),
            },
        )
        .unwrap();
    }

    fn peer_scope_message_record(
        msg_id: &str,
        owner_identity_id: &str,
        owner_did: &str,
        scoped_conversation_id: &str,
        peer_current_did: &str,
        stored_at: &str,
    ) -> MessageRecord {
        MessageRecord {
            msg_id: msg_id.to_owned(),
            owner_identity_id: owner_identity_id.to_owned(),
            owner_did: owner_did.to_owned(),
            conversation_id: scoped_conversation_id.to_owned(),
            thread_id: scoped_conversation_id.to_owned(),
            direction: 1,
            sender_did: owner_did.to_owned(),
            receiver_did: peer_current_did.to_owned(),
            content_type: "text/plain".to_owned(),
            content: format!("scoped message {msg_id}"),
            stored_at: stored_at.to_owned(),
            metadata: format!(
                r#"{{"peer_user_id":"peer-user-id","peer_full_handle":"zhuochengtest.anpclaw.com","peer_current_did":"{peer_current_did}"}}"#
            ),
            ..MessageRecord::default()
        }
    }

    fn conversation_ids_for_owner(db: &Connection, owner_identity_id: &str) -> Vec<String> {
        db.prepare(
            "SELECT DISTINCT conversation_id FROM messages WHERE owner_identity_id = ?1 ORDER BY conversation_id",
        )
        .unwrap()
        .query_map([owner_identity_id], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
    }

    fn summary_message_count(
        db: &Connection,
        owner_identity_id: &str,
        conversation_id: &str,
    ) -> i64 {
        db.query_row(
            "SELECT message_count FROM conversation_summaries WHERE owner_identity_id = ?1 AND conversation_id = ?2",
            (owner_identity_id, conversation_id),
            |row| row.get(0),
        )
        .unwrap()
    }

    fn summary_exists(db: &Connection, owner_identity_id: &str, conversation_id: &str) -> bool {
        db.query_row(
            "SELECT EXISTS(SELECT 1 FROM conversation_summaries WHERE owner_identity_id = ?1 AND conversation_id = ?2)",
            (owner_identity_id, conversation_id),
            |row| row.get::<_, bool>(0),
        )
        .unwrap()
    }

    fn message_ids(db: &Connection, owner_identity_id: &str) -> Vec<String> {
        db.prepare("SELECT msg_id FROM messages WHERE owner_identity_id = ?1 ORDER BY msg_id")
            .unwrap()
            .query_map([owner_identity_id], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    }

    fn raw_insert_message(db: &Connection, record: &MessageRecord) {
        db.execute(
            r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, conversation_id, thread_id, direction,
     sender_did, receiver_did, group_id, group_did, content_type, content, title,
     server_seq, sent_at, stored_at, is_e2ee, is_read, sender_name, metadata,
     mentions_current_user, credential_name)
VALUES (?1, ?2, ?3, ?4, ?5, ?6,
        ?7, ?8, ?9, ?10, ?11, ?12, ?13,
        ?14, ?15, ?16, ?17, ?18, ?19, ?20,
        ?21, ?22)"#,
            rusqlite::params![
                record.msg_id,
                record.owner_identity_id,
                record.owner_did,
                record.conversation_id,
                record.thread_id,
                record.direction,
                record.sender_did,
                record.receiver_did,
                record.group_id,
                record.group_did,
                record.content_type,
                record.content,
                record.title,
                record.server_seq,
                record.sent_at,
                record.stored_at,
                record.is_e2ee,
                record.is_read,
                record.sender_name,
                record.metadata,
                record.mentions_current_user,
                record.credential_name,
            ],
        )
        .unwrap();
    }

    fn legacy_merge_memo_stats(
        db: &Connection,
        owner_identity_id: &str,
        conversation_id: &str,
    ) -> Option<(i64, i64)> {
        db.query_row(
            r#"
SELECT scan_attempts, merged_rows
FROM temp.legacy_direct_merge_memo
WHERE owner_identity_id = ?1 AND peer_scope_conversation_id = ?2"#,
            (owner_identity_id, conversation_id),
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .ok()
    }

    fn legacy_memo_target_for_legacy_id(
        db: &Connection,
        owner_identity_id: &str,
        legacy_conversation_id: &str,
    ) -> Option<String> {
        db.query_row(
            r#"
SELECT peer_scope_conversation_id
FROM temp.legacy_direct_merge_memo_ids
WHERE owner_identity_id = ?1 AND legacy_conversation_id = ?2"#,
            (owner_identity_id, legacy_conversation_id),
            |row| row.get(0),
        )
        .ok()
    }

    fn summary_unread(db: &Connection, owner_identity_id: &str, conversation_id: &str) -> i64 {
        db.query_row(
            "SELECT unread_count FROM conversation_summaries WHERE owner_identity_id = ?1 AND conversation_id = ?2",
            (owner_identity_id, conversation_id),
            |row| row.get(0),
        )
        .unwrap()
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct SummarySnapshot {
        message_count: i64,
        unread_count: i64,
        unread_mention_count: i64,
        first_unread_mention_message_id: String,
        last_message_id: String,
        last_content: String,
        last_message_at: String,
    }

    fn summary_snapshot(
        db: &Connection,
        owner_identity_id: &str,
        conversation_id: &str,
    ) -> SummarySnapshot {
        db.query_row(
            r#"
SELECT message_count,
       unread_count,
       unread_mention_count,
       COALESCE(first_unread_mention_message_id, ''),
       COALESCE(last_message_id, ''),
       COALESCE(last_content, ''),
       last_message_at
FROM conversation_summaries
WHERE owner_identity_id = ?1 AND conversation_id = ?2"#,
            (owner_identity_id, conversation_id),
            |row| {
                Ok(SummarySnapshot {
                    message_count: row.get(0)?,
                    unread_count: row.get(1)?,
                    unread_mention_count: row.get(2)?,
                    first_unread_mention_message_id: row.get(3)?,
                    last_message_id: row.get(4)?,
                    last_content: row.get(5)?,
                    last_message_at: row.get(6)?,
                })
            },
        )
        .unwrap()
    }

    fn assert_summary_matches_rebuild(
        db: &Connection,
        owner_identity_id: &str,
        conversation_id: &str,
    ) {
        let incremental = summary_snapshot(db, owner_identity_id, conversation_id);
        super::super::conversation_summaries::rebuild_owner(db, owner_identity_id).unwrap();
        let rebuilt = summary_snapshot(db, owner_identity_id, conversation_id);
        assert_eq!(incremental, rebuilt);
    }

    fn summary_test_message(
        msg_id: &str,
        owner_identity_id: &str,
        owner_did: &str,
        conversation_id: &str,
        direction: i64,
        content: &str,
        stored_at: &str,
        is_read: bool,
    ) -> MessageRecord {
        let peer_did = "did:peer";
        MessageRecord {
            msg_id: msg_id.to_owned(),
            owner_identity_id: owner_identity_id.to_owned(),
            owner_did: owner_did.to_owned(),
            conversation_id: conversation_id.to_owned(),
            thread_id: conversation_id.to_owned(),
            direction,
            sender_did: if direction == 0 {
                peer_did.to_owned()
            } else {
                owner_did.to_owned()
            },
            receiver_did: if direction == 0 {
                owner_did.to_owned()
            } else {
                peer_did.to_owned()
            },
            group_id: if conversation_id.starts_with("group:") {
                conversation_id.to_owned()
            } else {
                String::new()
            },
            group_did: if conversation_id.starts_with("group:") {
                "did:group".to_owned()
            } else {
                String::new()
            },
            content_type: "text/plain".to_owned(),
            content: content.to_owned(),
            stored_at: stored_at.to_owned(),
            is_read,
            ..MessageRecord::default()
        }
    }

    fn mention_test_message(
        msg_id: &str,
        owner_identity_id: &str,
        owner_did: &str,
        conversation_id: &str,
        stored_at: &str,
    ) -> MessageRecord {
        MessageRecord {
            content_type: "application/json".to_owned(),
            content: serde_json::json!({
                "text": "@owner 请看",
                "mentions": [{
                    "id": format!("mention-{msg_id}"),
                    "range": {
                        "start": 0,
                        "end": 6,
                        "unit": "unicode_code_point"
                    },
                    "target": {
                        "kind": "human",
                        "did": owner_did,
                        "display_name": "Owner"
                    },
                    "mention_role": "addressee"
                }]
            })
            .to_string(),
            ..summary_test_message(
                msg_id,
                owner_identity_id,
                owner_did,
                conversation_id,
                0,
                "",
                stored_at,
                false,
            )
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn peer_scope_message(
        msg_id: &str,
        owner_identity_id: &str,
        owner_did: &str,
        conversation_id: &str,
        peer_did: &str,
        peer_handle: &str,
        server_seq: i64,
    ) -> MessageRecord {
        MessageRecord {
            msg_id: msg_id.to_owned(),
            owner_identity_id: owner_identity_id.to_owned(),
            owner_did: owner_did.to_owned(),
            conversation_id: conversation_id.to_owned(),
            thread_id: conversation_id.to_owned(),
            direction: 0,
            sender_did: peer_did.to_owned(),
            receiver_did: owner_did.to_owned(),
            content_type: "text/plain".to_owned(),
            content: msg_id.to_owned(),
            server_seq: Some(server_seq),
            sent_at: format!("2026-06-27T00:00:0{server_seq}Z"),
            stored_at: format!("2026-06-27T00:00:0{server_seq}Z"),
            metadata: serde_json::json!({
                "peer_user_id": "peer-user-id",
                "peer_full_handle": peer_handle,
                "peer_current_did": peer_did,
                "resolved_target_did": peer_did
            })
            .to_string(),
            ..MessageRecord::default()
        }
    }

    fn is_read(db: &Connection, owner_identity_id: &str) -> i64 {
        db.query_row(
            "SELECT is_read FROM messages WHERE owner_identity_id = ?1 AND msg_id = 'shared'",
            [owner_identity_id],
            |row| row.get(0),
        )
        .unwrap()
    }

    fn read_by_msg_id(db: &Connection, msg_id: &str) -> i64 {
        db.query_row(
            "SELECT is_read FROM messages WHERE msg_id = ?1",
            [msg_id],
            |row| row.get(0),
        )
        .unwrap()
    }

    #[allow(clippy::too_many_arguments)]
    fn seed_message_row(
        db: &Connection,
        msg_id: &str,
        owner_identity_id: &str,
        owner_did: &str,
        conversation_id: &str,
        direction: i64,
        sender_did: &str,
        receiver_did: &str,
        group_did: &str,
        is_read: i64,
        sent_at: &str,
    ) {
        db.execute(
            r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, conversation_id, thread_id, direction,
     sender_did, receiver_did, group_id, group_did, content_type, content,
     sent_at, stored_at, is_read)
VALUES (?1, ?2, ?3, ?4, ?4, ?5, ?6, ?7, ?8, ?8, 'text/plain', 'hello', ?10, ?10, ?9)"#,
            (
                msg_id,
                owner_identity_id,
                owner_did,
                conversation_id,
                direction,
                sender_did,
                receiver_did,
                group_did,
                is_read,
                sent_at,
            ),
        )
        .unwrap();
    }
}
