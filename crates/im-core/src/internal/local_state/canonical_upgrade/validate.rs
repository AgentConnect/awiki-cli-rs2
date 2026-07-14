use std::collections::BTreeMap;

use rusqlite::{types::ValueRef, Connection};
use sha2::{Digest as _, Sha256};

use super::upgrade_failed;

const CONSERVED_TABLES: &[&str] = &[
    "contact_handle_bindings",
    "contacts",
    "groups",
    "group_members",
    "e2ee_outbox",
    "group_rebind_outbox",
    "group_rebind_p6_jobs",
    "identity_did_history",
    "relationship_events",
    "sync_state",
    "thread_read_state",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SourceConservationSnapshot {
    table_counts: BTreeMap<String, i64>,
    empty_group_dids: Vec<(String, String)>,
    message_count: i64,
    message_facts_hash: String,
    expected_wire_facts_hash: String,
    outbox_facts_hash: String,
    read_state_facts_hash: String,
    contact_facts_hash: String,
    contact_binding_facts_hash: String,
    group_facts_hash: String,
    group_member_facts_hash: String,
    group_rebind_facts_hash: String,
    group_rebind_p6_facts_hash: String,
    identity_history_facts_hash: String,
    relationship_facts_hash: String,
    sync_facts_hash: String,
}

impl SourceConservationSnapshot {
    pub(super) fn capture(connection: &Connection) -> crate::ImResult<Self> {
        let mut table_counts = BTreeMap::new();
        for table in CONSERVED_TABLES {
            table_counts.insert((*table).to_owned(), count(connection, table)?);
        }
        Ok(Self {
            table_counts,
            empty_group_dids: empty_group_dids(connection)?,
            message_count: count(connection, "messages")?,
            message_facts_hash: hash_rows(
                connection,
                r#"SELECT msg_id, owner_identity_id, owner_did, direction,
       sender_did, receiver_did, group_id, group_did, content_type, content,
       title, server_seq, sent_at, stored_at, is_e2ee, is_read, sender_name,
       metadata, credential_name
FROM messages ORDER BY owner_identity_id, msg_id"#,
            )?,
            expected_wire_facts_hash: hash_rows(
                connection,
                r#"SELECT message.msg_id, message.owner_identity_id,
       CASE
         WHEN TRIM(COALESCE(message.group_did, '')) <> ''
           OR TRIM(COALESCE(message.group_id, '')) <> '' THEN 'group'
         WHEN TRIM(COALESCE(message.sender_did, '')) <> ''
           OR TRIM(COALESCE(message.receiver_did, '')) <> '' THEN 'direct'
         WHEN TRIM(message.thread_id) LIKE 'mail:%' THEN 'mail'
         ELSE ''
       END,
       CASE
         WHEN TRIM(COALESCE(message.group_did, '')) <> ''
           THEN TRIM(message.group_did)
         WHEN TRIM(COALESCE(message.group_id, '')) <> ''
           THEN COALESCE(
             (SELECT TRIM(groups.group_did) FROM groups
              WHERE groups.owner_identity_id = message.owner_identity_id
                AND groups.group_id = message.group_id
                AND TRIM(COALESCE(groups.group_did, '')) <> ''
              LIMIT 1),
             TRIM(message.group_id))
         WHEN TRIM(COALESCE(message.sender_did, '')) <> ''
           AND TRIM(message.sender_did) <> TRIM(message.owner_did)
           THEN TRIM(message.sender_did)
         WHEN TRIM(COALESCE(message.receiver_did, '')) <> ''
           THEN TRIM(message.receiver_did)
         WHEN TRIM(message.thread_id) LIKE 'mail:%' THEN TRIM(message.thread_id)
         ELSE ''
       END,
       message.sender_did, message.receiver_did, message.group_id,
       message.group_did, message.server_seq, message.is_e2ee, message.metadata
FROM messages AS message ORDER BY message.owner_identity_id, message.msg_id"#,
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
            contact_facts_hash: hash_rows(
                connection,
                r#"SELECT owner_identity_id, owner_did, did, name, handle,
       nick_name, bio, profile_md, tags, relationship, source_type,
       source_name, source_group_id, connected_at, recommended_reason,
       followed, messaged, note, first_seen_at, last_seen_at, metadata,
       credential_name
FROM contacts ORDER BY owner_identity_id, did"#,
            )?,
            contact_binding_facts_hash: hash_rows(
                connection,
                r#"SELECT owner_identity_id, owner_did, handle, did, is_current,
       first_seen_at, last_seen_at, source_type, source_group_id, metadata,
       credential_name
FROM contact_handle_bindings ORDER BY owner_identity_id, handle, did"#,
            )?,
            group_facts_hash: hash_rows(
                connection,
                r#"SELECT owner_identity_id, owner_did, group_id, group_did,
       name, group_mode, slug, description, goal, rules, message_prompt,
       doc_url, group_owner_did, group_owner_handle, my_role,
       membership_status, join_enabled, join_code, join_code_expires_at,
       member_count, last_synced_seq, last_read_seq, last_message_at,
       remote_created_at, remote_updated_at, stored_at, metadata,
       credential_name
FROM groups ORDER BY owner_identity_id, group_id"#,
            )?,
            group_member_facts_hash: hash_rows(
                connection,
                r#"SELECT owner_identity_id, owner_did, group_id, user_id,
       member_did, member_handle, anchor_kind, anchor_value,
       handle_binding_generation, profile_url, role, status, joined_at,
       sent_message_count, last_synced_at, metadata, credential_name
FROM group_members ORDER BY owner_identity_id, group_id, user_id"#,
            )?,
            group_rebind_facts_hash: hash_rows(
                connection,
                r#"SELECT job_id, owner_identity_id, group_did, member_handle,
       previous_member_did, new_member_did, binding_generation, phase,
       group_state_ref_json, attempt_count, lease_expires_at, next_attempt_at,
       last_error_code, last_error_detail, created_at, updated_at
FROM group_rebind_outbox ORDER BY job_id"#,
            )?,
            group_rebind_p6_facts_hash: hash_rows(
                connection,
                r#"SELECT job_id, owner_identity_id, group_did, event_id,
       member_handle, previous_member_did, new_member_did,
       binding_generation, group_state_ref_json, phase, attempt_count,
       lease_expires_at, next_attempt_at, last_error_code,
       last_error_detail, created_at, updated_at
FROM group_rebind_p6_jobs ORDER BY job_id"#,
            )?,
            identity_history_facts_hash: hash_rows(
                connection,
                r#"SELECT owner_identity_id, did, status, first_seen_at,
       last_seen_at, metadata
FROM identity_did_history ORDER BY owner_identity_id, did"#,
            )?,
            relationship_facts_hash: hash_rows(
                connection,
                r#"SELECT event_id, owner_identity_id, owner_did, target_did,
       target_handle, event_type, source_type, source_name, source_group_id,
       reason, score, status, created_at, updated_at, metadata,
       credential_name
FROM relationship_events ORDER BY owner_identity_id, event_id"#,
            )?,
            sync_facts_hash: hash_rows(
                connection,
                r#"SELECT owner_identity_id, owner_did, scope, checkpoint_kind,
       event_seq, updated_at, metadata_json
FROM sync_state ORDER BY owner_identity_id, scope, checkpoint_kind"#,
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
    let actual_wire_facts_hash = hash_rows(
        connection,
        r#"SELECT msg_id, owner_identity_id, wire_thread_kind, wire_thread_ref,
       sender_did, receiver_did, group_id, group_did, server_seq, is_e2ee,
       metadata
FROM messages ORDER BY owner_identity_id, msg_id"#,
    )?;
    if source.expected_wire_facts_hash != actual_wire_facts_hash {
        return Err(upgrade_failed("validation", "wire_identity_facts_changed"));
    }
    if source.outbox_facts_hash != target.outbox_facts_hash {
        return Err(upgrade_failed("validation", "outbox_facts_changed"));
    }
    if source.read_state_facts_hash != target.read_state_facts_hash {
        return Err(upgrade_failed("validation", "read_state_facts_changed"));
    }
    for (source_hash, target_hash, code) in [
        (
            &source.contact_facts_hash,
            &target.contact_facts_hash,
            "contact_facts_changed",
        ),
        (
            &source.contact_binding_facts_hash,
            &target.contact_binding_facts_hash,
            "contact_binding_facts_changed",
        ),
        (
            &source.group_facts_hash,
            &target.group_facts_hash,
            "group_facts_changed",
        ),
        (
            &source.group_member_facts_hash,
            &target.group_member_facts_hash,
            "group_member_facts_changed",
        ),
        (
            &source.group_rebind_facts_hash,
            &target.group_rebind_facts_hash,
            "group_rebind_facts_changed",
        ),
        (
            &source.group_rebind_p6_facts_hash,
            &target.group_rebind_p6_facts_hash,
            "group_rebind_p6_facts_changed",
        ),
        (
            &source.identity_history_facts_hash,
            &target.identity_history_facts_hash,
            "identity_history_facts_changed",
        ),
        (
            &source.relationship_facts_hash,
            &target.relationship_facts_hash,
            "relationship_facts_changed",
        ),
        (
            &source.sync_facts_hash,
            &target.sync_facts_hash,
            "sync_facts_changed",
        ),
    ] {
        if source_hash != target_hash {
            return Err(upgrade_failed("validation", code));
        }
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
    for (owner_identity_id, group_did) in &source.empty_group_dids {
        let count: i64 = connection
            .query_row(
                r#"SELECT COUNT(*) FROM conversation_registry
WHERE owner_identity_id = ?1 AND canonical_group_did = ?2
  AND lifecycle_state = 'active' AND resolution_state = 'resolved'"#,
                (owner_identity_id, group_did),
                |row| row.get(0),
            )
            .map_err(super::super::local_state_unavailable)?;
        if count != 1 {
            return Err(upgrade_failed(
                "validation",
                "empty_group_conversation_not_preserved",
            ));
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

fn empty_group_dids(connection: &Connection) -> crate::ImResult<Vec<(String, String)>> {
    let mut statement = connection
        .prepare(
            r#"SELECT groups.owner_identity_id, TRIM(groups.group_did)
FROM groups
JOIN conversation_registry registry
  ON registry.owner_identity_id = groups.owner_identity_id
 AND registry.is_active = 1
 AND (registry.thread_id = groups.group_id
      OR registry.conversation_id = 'group:' || groups.group_id)
LEFT JOIN messages
  ON messages.owner_identity_id = groups.owner_identity_id
 AND (messages.group_id = groups.group_id OR messages.group_did = groups.group_did)
WHERE TRIM(COALESCE(groups.group_did, '')) <> ''
GROUP BY groups.owner_identity_id, groups.group_did
HAVING COUNT(messages.msg_id) = 0
ORDER BY groups.owner_identity_id, groups.group_did"#,
        )
        .map_err(|_| upgrade_failed("validation", "conservation_query_failed"))?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|_| upgrade_failed("validation", "conservation_query_failed"))?;
    let mut values = Vec::new();
    for row in rows {
        values.push(row.map_err(|_| upgrade_failed("validation", "conservation_query_failed"))?);
    }
    Ok(values)
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
