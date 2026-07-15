#[cfg(feature = "sqlite")]
use rusqlite::{params, Connection, Transaction};
#[cfg(feature = "sqlite")]
use std::collections::BTreeSet;

#[cfg(feature = "sqlite")]
const GLOBAL_SCOPE: &str = "global";
#[cfg(feature = "sqlite")]
const EVENT_SEQ_CHECKPOINT: &str = "event_seq";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SyncDeltaApplyInput {
    pub(crate) owner_identity_id: String,
    pub(crate) owner_did: String,
    pub(crate) events: Vec<SyncDeltaApplyEvent>,
    pub(crate) next_event_seq: String,
    pub(crate) metadata_json: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SyncDeltaApplyEvent {
    pub(crate) event_id: String,
    pub(crate) event_seq: String,
    pub(crate) event_type: String,
    pub(crate) messages: Vec<super::messages::MessageRecord>,
    pub(crate) groups: Vec<super::groups::GroupRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SyncDeltaApplyOutcome {
    pub(crate) applied_events: usize,
    pub(crate) backlogged_messages: usize,
    pub(crate) last_applied_event_seq: String,
    pub(crate) invalidation: SyncDeltaInvalidation,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SyncDeltaInvalidation {
    pub(crate) owner_identity_id: String,
    pub(crate) owner_did: String,
    pub(crate) reason: String,
    pub(crate) checkpoint_event_seq: String,
    pub(crate) conversation_ids: Vec<String>,
    pub(crate) thread_ids: Vec<String>,
    pub(crate) group_ids: Vec<String>,
    pub(crate) group_dids: Vec<String>,
}

impl SyncDeltaInvalidation {
    pub(crate) fn has_changes(&self) -> bool {
        !self.conversation_ids.is_empty()
            || !self.thread_ids.is_empty()
            || !self.group_ids.is_empty()
            || !self.group_dids.is_empty()
    }
}

#[cfg(feature = "sqlite")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GlobalCheckpoint {
    pub(crate) event_seq: String,
    pub(crate) owner_did: String,
    pub(crate) updated_at: String,
    pub(crate) metadata_json: Option<String>,
}

#[cfg(feature = "sqlite")]
pub(crate) fn load_global_checkpoint(
    connection: &Connection,
    owner_identity_id: &str,
) -> crate::ImResult<Option<GlobalCheckpoint>> {
    crate::internal::local_state::schema::ensure_schema(connection)?;
    let owner_identity_id = required("owner_identity_id", owner_identity_id)?;
    let mut statement = connection
        .prepare(
            r#"
SELECT owner_did, event_seq, updated_at, metadata_json
FROM sync_state
WHERE owner_identity_id = ?1
  AND scope = ?2
  AND checkpoint_kind = ?3"#,
        )
        .map_err(super::local_state_unavailable)?;
    let mut rows = statement
        .query(params![
            owner_identity_id,
            GLOBAL_SCOPE,
            EVENT_SEQ_CHECKPOINT
        ])
        .map_err(super::local_state_unavailable)?;
    let Some(row) = rows.next().map_err(super::local_state_unavailable)? else {
        return Ok(None);
    };
    let event_seq = row
        .get::<_, String>("event_seq")
        .map_err(super::local_state_unavailable)?;
    parse_decimal_seq(&event_seq)?;
    Ok(Some(GlobalCheckpoint {
        owner_did: row
            .get::<_, String>("owner_did")
            .map_err(super::local_state_unavailable)?,
        event_seq,
        updated_at: row
            .get::<_, String>("updated_at")
            .map_err(super::local_state_unavailable)?,
        metadata_json: row
            .get::<_, Option<String>>("metadata_json")
            .map_err(super::local_state_unavailable)?,
    }))
}

#[cfg(feature = "sqlite")]
pub(crate) fn store_global_checkpoint_tx(
    transaction: &Transaction<'_>,
    owner_identity_id: &str,
    owner_did: &str,
    event_seq: &str,
    metadata_json: Option<&str>,
) -> crate::ImResult<()> {
    let owner_identity_id = required("owner_identity_id", owner_identity_id)?;
    let owner_did = required("owner_did", owner_did)?;
    let event_seq = normalize_decimal_seq(event_seq)?;
    let updated_at = now_utc_like();
    transaction
        .execute(
            r#"
INSERT INTO sync_state
    (owner_identity_id, owner_did, scope, checkpoint_kind, event_seq, updated_at, metadata_json)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
ON CONFLICT(owner_identity_id, scope, checkpoint_kind)
DO UPDATE SET
    owner_did = excluded.owner_did,
    event_seq = excluded.event_seq,
    updated_at = excluded.updated_at,
    metadata_json = excluded.metadata_json"#,
            params![
                owner_identity_id,
                owner_did,
                GLOBAL_SCOPE,
                EVENT_SEQ_CHECKPOINT,
                event_seq,
                updated_at,
                metadata_json,
            ],
        )
        .map_err(super::local_state_unavailable)?;
    Ok(())
}

#[cfg(feature = "sqlite")]
pub(crate) fn apply_sync_delta_tx(
    transaction: &Transaction<'_>,
    input: SyncDeltaApplyInput,
) -> crate::ImResult<SyncDeltaApplyOutcome> {
    crate::internal::local_state::schema::ensure_schema(transaction)?;
    let owner_identity_id = required("owner_identity_id", &input.owner_identity_id)?;
    let owner_did = required("owner_did", &input.owner_did)?;
    let current_checkpoint = load_global_checkpoint(transaction, &owner_identity_id)?
        .map(|checkpoint| checkpoint.event_seq)
        .unwrap_or_else(|| "0".to_owned());
    let current_seq = parse_decimal_seq(&current_checkpoint)?;
    let next_event_seq = normalize_decimal_seq(&input.next_event_seq)
        .map_err(|_| invalid_page("next_event_seq must be a decimal string"))?;
    let next_seq = parse_decimal_seq(&next_event_seq)?;
    if next_seq < current_seq {
        return Err(invalid_page("next_event_seq is behind local checkpoint"));
    }

    let mut new_events = Vec::new();
    for event in input.events {
        let event_seq = normalize_decimal_seq(&event.event_seq)
            .map_err(|_| invalid_page("event_seq must be a decimal string"))?;
        let event_seq_num = parse_decimal_seq(&event_seq)?;
        if event_seq_num <= current_seq {
            continue;
        }
        new_events.push((event_seq_num, event_seq, event));
    }
    new_events.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.2.event_id.cmp(&right.2.event_id))
    });

    let mut expected = current_seq.saturating_add(1);
    let mut previous_seq = None;
    for (seq_num, _, _) in &new_events {
        if previous_seq == Some(*seq_num) {
            return Err(invalid_page("sync.delta page contains duplicate event_seq"));
        }
        if *seq_num != expected {
            return Err(invalid_page("sync.delta page has an event_seq gap"));
        }
        previous_seq = Some(*seq_num);
        expected = expected.saturating_add(1);
    }

    let last_new_seq = new_events
        .last()
        .map(|(seq_num, _, _)| *seq_num)
        .unwrap_or(current_seq);
    if next_seq != last_new_seq {
        return Err(invalid_page(
            "next_event_seq must equal the last applied event_seq",
        ));
    }

    let mut messages = Vec::new();
    let mut groups = Vec::new();
    let mut backlogged_messages = 0usize;
    for (_, event_seq, event) in new_events {
        for message in event.messages {
            match super::inbound_resolution_backlog::canonicalize_inbound_message(
                transaction,
                message.clone(),
            ) {
                Ok(message) => messages.push(message),
                Err(error) if super::inbound_resolution_backlog::is_resolution_error(&error) => {
                    super::inbound_resolution_backlog::store(
                        transaction,
                        super::inbound_resolution_backlog::BacklogSource {
                            event_id: &event.event_id,
                            event_seq: &event_seq,
                            event_type: &event.event_type,
                        },
                        &message,
                        &error,
                    )?;
                    backlogged_messages = backlogged_messages.saturating_add(1);
                }
                Err(error) => return Err(error),
            }
        }
        groups.extend(event.groups);
    }
    let invalidation = sync_delta_invalidation(
        &owner_identity_id,
        &owner_did,
        &next_event_seq,
        &messages,
        &groups,
    );
    if !messages.is_empty() {
        let touched = super::messages::upsert_messages_with_touched(transaction, &messages)?;
        // Use touched conversations from the committed local-state write path so
        // legacy direct folds and canonical conversation normalization are not
        // lost when downstream stores consume this invalidation after commit.
        let mut conversation_ids = invalidation
            .conversation_ids
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        let mut thread_ids = invalidation
            .thread_ids
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        for (_, conversation_id) in touched {
            if !conversation_id.trim().is_empty() {
                conversation_ids.insert(conversation_id.clone());
                thread_ids.insert(conversation_id);
            }
        }
        let invalidation = SyncDeltaInvalidation {
            conversation_ids: conversation_ids.into_iter().collect(),
            thread_ids: thread_ids.into_iter().collect(),
            ..invalidation
        };
        return finish_sync_delta_apply(
            transaction,
            owner_identity_id,
            owner_did,
            current_seq,
            next_seq,
            next_event_seq,
            input.metadata_json,
            groups,
            invalidation,
            backlogged_messages,
        );
    }

    finish_sync_delta_apply(
        transaction,
        owner_identity_id,
        owner_did,
        current_seq,
        next_seq,
        next_event_seq,
        input.metadata_json,
        groups,
        invalidation,
        backlogged_messages,
    )
}

#[cfg(feature = "sqlite")]
#[allow(clippy::too_many_arguments)]
fn finish_sync_delta_apply(
    transaction: &Transaction<'_>,
    owner_identity_id: String,
    owner_did: String,
    current_seq: u64,
    next_seq: u64,
    next_event_seq: String,
    metadata_json: Option<String>,
    groups: Vec<super::groups::GroupRecord>,
    invalidation: SyncDeltaInvalidation,
    backlogged_messages: usize,
) -> crate::ImResult<SyncDeltaApplyOutcome> {
    for group in groups {
        super::groups::upsert_group(transaction, group)?;
    }

    if next_seq > current_seq {
        store_global_checkpoint_tx(
            transaction,
            &owner_identity_id,
            &owner_did,
            &next_event_seq,
            metadata_json.as_deref(),
        )?;
    }

    Ok(SyncDeltaApplyOutcome {
        applied_events: usize::try_from(next_seq - current_seq).unwrap_or(usize::MAX),
        backlogged_messages,
        last_applied_event_seq: next_event_seq,
        invalidation,
    })
}

#[cfg(feature = "sqlite")]
fn sync_delta_invalidation(
    owner_identity_id: &str,
    owner_did: &str,
    checkpoint_event_seq: &str,
    messages: &[super::messages::MessageRecord],
    groups: &[super::groups::GroupRecord],
) -> SyncDeltaInvalidation {
    let mut conversation_ids = BTreeSet::new();
    let mut thread_ids = BTreeSet::new();
    let mut group_ids = BTreeSet::new();
    let mut group_dids = BTreeSet::new();

    for message in messages {
        let conversation_id = message.conversation_id.trim();
        if !conversation_id.is_empty() {
            conversation_ids.insert(conversation_id.to_owned());
        }
        let thread_id = message.thread_id.trim();
        if !thread_id.is_empty() {
            thread_ids.insert(thread_id.to_owned());
        } else if !conversation_id.is_empty() {
            thread_ids.insert(conversation_id.to_owned());
        }
    }

    for group in groups {
        let group_id = group.group_id.trim();
        if !group_id.is_empty() {
            group_ids.insert(group_id.to_owned());
            let conversation_id =
                crate::internal::local_state::owner_scope::group_conversation_id(group_id);
            conversation_ids.insert(conversation_id.clone());
            thread_ids.insert(conversation_id);
        }
        let group_did = group.group_did.trim();
        if !group_did.is_empty() {
            group_dids.insert(group_did.to_owned());
            let conversation_id =
                crate::internal::local_state::owner_scope::group_conversation_id(group_did);
            conversation_ids.insert(conversation_id.clone());
            thread_ids.insert(conversation_id);
        }
    }

    SyncDeltaInvalidation {
        owner_identity_id: owner_identity_id.to_owned(),
        owner_did: owner_did.to_owned(),
        reason: "sync_delta".to_owned(),
        checkpoint_event_seq: checkpoint_event_seq.to_owned(),
        conversation_ids: conversation_ids.into_iter().collect(),
        thread_ids: thread_ids.into_iter().collect(),
        group_ids: group_ids.into_iter().collect(),
        group_dids: group_dids.into_iter().collect(),
    }
}

pub(crate) fn parse_decimal_seq(value: &str) -> crate::ImResult<u64> {
    let value = value.trim();
    if value.is_empty() {
        return Err(decimal_seq_error(value));
    }
    if value.len() > 1 && value.starts_with('0') {
        return Err(decimal_seq_error(value));
    }
    if !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(decimal_seq_error(value));
    }
    value.parse::<u64>().map_err(|_| decimal_seq_error(value))
}

pub(crate) fn normalize_decimal_seq(value: &str) -> crate::ImResult<String> {
    Ok(parse_decimal_seq(value)?.to_string())
}

pub(crate) fn decimal_seq_gt(left: &str, right: &str) -> crate::ImResult<bool> {
    Ok(parse_decimal_seq(left)? > parse_decimal_seq(right)?)
}

fn decimal_seq_error(value: &str) -> crate::ImError {
    crate::ImError::invalid_input(
        Some("event_seq".to_owned()),
        format!("sequence must be a non-negative decimal string: {value:?}"),
    )
}

#[cfg(feature = "sqlite")]
fn invalid_page(message: impl Into<String>) -> crate::ImError {
    crate::ImError::Service {
        status_code: None,
        code: Some("sync.invalid_page".to_owned()),
        message: message.into(),
        data: None,
    }
}

#[cfg(feature = "sqlite")]
fn required(field: &'static str, value: &str) -> crate::ImResult<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some(field.to_owned()),
            format!("{field} is required"),
        ));
    }
    Ok(value.to_owned())
}

#[cfg(feature = "sqlite")]
fn now_utc_like() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    format!("{secs}")
}

#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use super::*;

    #[test]
    fn sync_state_load_store_and_rollback_are_transactional() {
        let mut db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();

        {
            let tx = db.transaction().unwrap();
            store_global_checkpoint_tx(
                &tx,
                "alice-id",
                "did:example:alice",
                "41",
                Some(r#"{"reason":"test"}"#),
            )
            .unwrap();
            tx.commit().unwrap();
        }

        let stored = load_global_checkpoint(&db, "alice-id").unwrap().unwrap();
        assert_eq!(stored.event_seq, "41");
        assert_eq!(stored.owner_did, "did:example:alice");
        assert_eq!(
            stored.metadata_json.as_deref(),
            Some(r#"{"reason":"test"}"#)
        );

        {
            let tx = db.transaction().unwrap();
            store_global_checkpoint_tx(&tx, "alice-id", "did:example:alice", "42", None).unwrap();
            tx.rollback().unwrap();
        }

        let stored = load_global_checkpoint(&db, "alice-id").unwrap().unwrap();
        assert_eq!(stored.event_seq, "41");
        assert_eq!(
            stored.metadata_json.as_deref(),
            Some(r#"{"reason":"test"}"#)
        );
        assert!(load_global_checkpoint(&db, "bob-id").unwrap().is_none());
    }

    #[test]
    fn sync_state_numeric_seq_parse_rejects_invalid_values() {
        assert_eq!(parse_decimal_seq("0").unwrap(), 0);
        assert_eq!(parse_decimal_seq("42").unwrap(), 42);
        assert!(decimal_seq_gt("100", "99").unwrap());
        assert!(!decimal_seq_gt("99", "100").unwrap());

        for invalid in ["", "  ", "-1", "1.0", "01", "abc"] {
            assert!(parse_decimal_seq(invalid).is_err(), "{invalid:?}");
        }
    }

    #[test]
    fn unresolved_inbound_is_backlogged_in_same_transaction_as_checkpoint() {
        let mut db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let message = super::super::messages::MessageRecord {
            msg_id: "msg-unresolved".to_owned(),
            owner_identity_id: "alice-id".to_owned(),
            owner_did: "did:example:alice".to_owned(),
            conversation_id: "dm:did:example:bob".to_owned(),
            thread_id: "dm:did:example:bob".to_owned(),
            direction: 0,
            sender_did: "did:example:bob".to_owned(),
            receiver_did: "did:example:alice".to_owned(),
            content_type: "text/plain".to_owned(),
            content: "hello".to_owned(),
            stored_at: "2026-07-14T00:00:00Z".to_owned(),
            credential_name: "alice-id".to_owned(),
            ..super::super::messages::MessageRecord::default()
        }
        .with_resolved_wire_thread("direct", "did:example:bob");
        let input = SyncDeltaApplyInput {
            owner_identity_id: "alice-id".to_owned(),
            owner_did: "did:example:alice".to_owned(),
            events: vec![SyncDeltaApplyEvent {
                event_id: "event-1".to_owned(),
                event_seq: "1".to_owned(),
                event_type: "message.created".to_owned(),
                messages: vec![message],
                groups: Vec::new(),
            }],
            next_event_seq: "1".to_owned(),
            metadata_json: None,
        };

        let tx = db.transaction().unwrap();
        let outcome = apply_sync_delta_tx(&tx, input).unwrap();
        assert_eq!(outcome.backlogged_messages, 1);
        tx.commit().unwrap();

        assert_eq!(
            load_global_checkpoint(&db, "alice-id")
                .unwrap()
                .unwrap()
                .event_seq,
            "1"
        );
        assert_eq!(
            super::super::inbound_resolution_backlog::pending_count(&db, "alice-id").unwrap(),
            1
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM messages", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
}
