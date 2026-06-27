#[cfg(feature = "sqlite")]
use rusqlite::{params, Connection, Transaction};

#[cfg(feature = "sqlite")]
const GLOBAL_SCOPE: &str = "global";
#[cfg(feature = "sqlite")]
const EVENT_SEQ_CHECKPOINT: &str = "event_seq";

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
}
