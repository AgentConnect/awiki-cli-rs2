use std::collections::BTreeMap;

use rusqlite::{types::ValueRef, Connection};
use sha2::{Digest as _, Sha256};

use super::upgrade_failed;

const CONSERVED_TABLES: &[&str] = &[
    "contacts",
    "groups",
    "group_members",
    "e2ee_outbox",
    "thread_read_state",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SourceConservationSnapshot {
    table_counts: BTreeMap<String, i64>,
    message_count: i64,
    message_facts_hash: String,
    outbox_facts_hash: String,
    read_state_facts_hash: String,
}

impl SourceConservationSnapshot {
    pub(super) fn capture(connection: &Connection) -> crate::ImResult<Self> {
        let mut table_counts = BTreeMap::new();
        for table in CONSERVED_TABLES {
            table_counts.insert((*table).to_owned(), count(connection, table)?);
        }
        Ok(Self {
            table_counts,
            message_count: count(connection, "messages")?,
            message_facts_hash: hash_rows(
                connection,
                r#"SELECT msg_id, owner_identity_id, owner_did, direction,
       sender_did, receiver_did, group_id, group_did, content_type, content,
       title, server_seq, sent_at, stored_at, is_e2ee, is_read, sender_name,
       metadata, credential_name
FROM messages ORDER BY owner_identity_id, msg_id"#,
            )?,
            outbox_facts_hash: hash_rows(
                connection,
                r#"SELECT outbox_id, owner_identity_id, owner_did, peer_did, session_id,
       original_type, plaintext, local_status, attempt_count, sent_msg_id,
       sent_server_seq, last_error_code, retry_hint, failed_msg_id,
       failed_server_seq, metadata, last_attempt_at, created_at, updated_at,
       credential_name
FROM e2ee_outbox ORDER BY owner_identity_id, outbox_id"#,
            )?,
            read_state_facts_hash: hash_rows(
                connection,
                r#"SELECT owner_identity_id, owner_did, thread_scope, thread_id,
       read_watermark_message_id, read_watermark_seq, read_watermark_at,
       pending_remote_ack, remote_ack_at, updated_at
FROM thread_read_state ORDER BY owner_identity_id, thread_scope, thread_id"#,
            )?,
        })
    }
}

pub(super) fn validate_migrated_shadow(
    connection: &Connection,
    source: &SourceConservationSnapshot,
) -> crate::ImResult<()> {
    let target = SourceConservationSnapshot::capture(connection)?;
    if source.table_counts != target.table_counts {
        return Err(upgrade_failed(
            "validation",
            "table_count_conservation_failed",
        ));
    }
    if source.message_count != target.message_count {
        return Err(upgrade_failed(
            "validation",
            "message_count_conservation_failed",
        ));
    }
    if source.message_facts_hash != target.message_facts_hash {
        return Err(upgrade_failed("validation", "message_facts_changed"));
    }
    if source.outbox_facts_hash != target.outbox_facts_hash {
        return Err(upgrade_failed("validation", "outbox_facts_changed"));
    }
    if source.read_state_facts_hash != target.read_state_facts_hash {
        return Err(upgrade_failed("validation", "read_state_facts_changed"));
    }
    let incomplete_wire: i64 = connection
        .query_row(
            r#"SELECT COUNT(*) FROM messages
WHERE wire_identity_resolution_state NOT IN ('resolved', 'legacy_unresolved')
   OR (wire_identity_resolution_state = 'resolved'
       AND (TRIM(wire_thread_kind) = '' OR TRIM(wire_thread_ref) = ''))"#,
            [],
            |row| row.get(0),
        )
        .map_err(super::super::local_state_unavailable)?;
    if incomplete_wire != 0 {
        return Err(upgrade_failed("validation", "wire_identity_incomplete"));
    }
    let owner_ids = distinct_owner_ids(connection)?;
    for owner_identity_id in owner_ids {
        let violations = super::super::canonical_invariants::check(connection, &owner_identity_id)?;
        if !violations.is_empty() {
            return Err(upgrade_failed("validation", "canonical_invariant_failed"));
        }
    }
    let integrity: String = connection
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .map_err(|_| upgrade_failed("validation", "shadow_integrity_check_failed"))?;
    if integrity != "ok" {
        return Err(upgrade_failed(
            "validation",
            "shadow_integrity_check_failed",
        ));
    }
    Ok(())
}

fn count(connection: &Connection, table: &str) -> crate::ImResult<i64> {
    connection
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .map_err(|_| upgrade_failed("validation", "conservation_query_failed"))
}

fn hash_rows(connection: &Connection, sql: &str) -> crate::ImResult<String> {
    let mut statement = connection
        .prepare(sql)
        .map_err(|_| upgrade_failed("validation", "conservation_query_failed"))?;
    let column_count = statement.column_count();
    let mut rows = statement
        .query([])
        .map_err(|_| upgrade_failed("validation", "conservation_query_failed"))?;
    let mut digest = Sha256::new();
    while let Some(row) = rows
        .next()
        .map_err(|_| upgrade_failed("validation", "conservation_query_failed"))?
    {
        for index in 0..column_count {
            match row
                .get_ref(index)
                .map_err(|_| upgrade_failed("validation", "conservation_query_failed"))?
            {
                ValueRef::Null => digest.update([0]),
                ValueRef::Integer(value) => {
                    digest.update([1]);
                    digest.update(value.to_be_bytes());
                }
                ValueRef::Real(value) => {
                    digest.update([2]);
                    digest.update(value.to_bits().to_be_bytes());
                }
                ValueRef::Text(value) => {
                    digest.update([3]);
                    digest.update((value.len() as u64).to_be_bytes());
                    digest.update(value);
                }
                ValueRef::Blob(value) => {
                    digest.update([4]);
                    digest.update((value.len() as u64).to_be_bytes());
                    digest.update(value);
                }
            }
        }
        digest.update([b'\n']);
    }
    Ok(format!("sha256:{:x}", digest.finalize()))
}

fn distinct_owner_ids(connection: &Connection) -> crate::ImResult<Vec<String>> {
    let mut statement = connection
        .prepare(
            r#"SELECT owner_identity_id FROM (
SELECT owner_identity_id FROM messages
UNION SELECT owner_identity_id FROM conversation_registry
UNION SELECT owner_identity_id FROM direct_peer_routes
) WHERE TRIM(owner_identity_id) <> '' ORDER BY owner_identity_id"#,
        )
        .map_err(super::super::local_state_unavailable)?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(super::super::local_state_unavailable)?;
    let mut owners = Vec::new();
    for row in rows {
        owners.push(row.map_err(super::super::local_state_unavailable)?);
    }
    Ok(owners)
}
