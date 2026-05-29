mod records;
mod sql;

use crate::internal::identity_recover_local_state::helpers::StoreResult;
use records::{
    merge_recovered_contact, merge_recovered_contact_handle_binding, merge_recovered_group,
    merge_recovered_group_member, merge_recovered_message, merge_recovered_relationship_event,
    normalize_recover_owner_dids, normalize_recovered_contact_handle_binding_row,
    normalize_recovered_contact_row, normalize_recovered_group_member_row,
    normalize_recovered_group_row, normalize_recovered_message_row,
    normalize_recovered_relationship_event_row,
};
use rusqlite::Transaction;
use sql::{
    count_rows_for_owners, delete_rows_for_owners, normalize_recovered_current_handles,
    query_maps_with_param, query_one_map1, query_one_map2, query_one_map3, store_file_exists,
    upsert_recovered_contact, upsert_recovered_contact_handle_binding, upsert_recovered_group,
    upsert_recovered_group_member, upsert_recovered_message, upsert_recovered_relationship_event,
    zero_counts,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const STORE_MERGE_TABLES: &[&str] = &[
    "messages",
    "contacts",
    "contact_handle_bindings",
    "relationship_events",
    "groups",
    "group_members",
];
const E2EE_CLEANUP_TABLES: &[&str] = &["e2ee_outbox", "e2ee_sessions"];

pub(crate) fn merge_recovered_handle_local_state<S: AsRef<str>>(
    sqlite_path: &Path,
    old_owner_dids: &[S],
    new_owner_did: &str,
    final_credential_name: &str,
) -> StoreResult<(BTreeMap<String, i64>, BTreeMap<String, i64>)> {
    if !store_file_exists(sqlite_path) {
        return Ok((
            zero_counts(STORE_MERGE_TABLES),
            zero_counts(E2EE_CLEANUP_TABLES),
        ));
    }

    let mut connection = rusqlite::Connection::open(sqlite_path)
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    crate::internal::local_state::configure(&connection)?;
    crate::internal::local_state::schema::ensure_schema(&connection)?;
    merge_recovered_handle_local_state_for_connection(
        &mut connection,
        old_owner_dids,
        new_owner_did,
        final_credential_name,
    )
}

pub(crate) fn merge_recovered_handle_local_state_for_connection<S: AsRef<str>>(
    connection: &mut rusqlite::Connection,
    old_owner_dids: &[S],
    new_owner_did: &str,
    final_credential_name: &str,
) -> StoreResult<(BTreeMap<String, i64>, BTreeMap<String, i64>)> {
    let mut store_merge = zero_counts(STORE_MERGE_TABLES);
    let mut e2ee_cleanup = zero_counts(E2EE_CLEANUP_TABLES);
    let owners = normalize_recover_owner_dids(old_owner_dids, new_owner_did);
    if owners.is_empty() {
        return Ok((store_merge, e2ee_cleanup));
    }

    let transaction = connection.transaction()?;
    let owner_set = owners.iter().cloned().collect::<BTreeSet<_>>();
    let mut affected_handles = BTreeSet::new();

    store_merge.insert(
        "messages".to_string(),
        merge_recovered_messages(
            &transaction,
            &owners,
            &owner_set,
            new_owner_did,
            final_credential_name,
        )?,
    );
    store_merge.insert(
        "contacts".to_string(),
        merge_recovered_contacts(
            &transaction,
            &owners,
            new_owner_did,
            final_credential_name,
            &mut affected_handles,
        )?,
    );
    store_merge.insert(
        "contact_handle_bindings".to_string(),
        merge_recovered_contact_handle_bindings(
            &transaction,
            &owners,
            new_owner_did,
            final_credential_name,
            &mut affected_handles,
        )?,
    );
    normalize_recovered_current_handles(&transaction, new_owner_did, &affected_handles)?;
    store_merge.insert(
        "relationship_events".to_string(),
        merge_recovered_relationship_events(
            &transaction,
            &owners,
            new_owner_did,
            final_credential_name,
        )?,
    );
    store_merge.insert(
        "groups".to_string(),
        merge_recovered_groups(
            &transaction,
            &owners,
            &owner_set,
            new_owner_did,
            final_credential_name,
        )?,
    );
    store_merge.insert(
        "group_members".to_string(),
        merge_recovered_group_members(
            &transaction,
            &owners,
            &owner_set,
            new_owner_did,
            final_credential_name,
        )?,
    );
    e2ee_cleanup = clear_recovered_owner_e2ee_data(&transaction, &owners)?;
    transaction.commit()?;
    Ok((store_merge, e2ee_cleanup))
}

fn merge_recovered_messages(
    transaction: &Transaction<'_>,
    old_owner_dids: &[String],
    old_owner_set: &BTreeSet<String>,
    new_owner_did: &str,
    final_credential_name: &str,
) -> StoreResult<i64> {
    let mut count = 0;
    for old_owner in old_owner_dids {
        let rows = query_maps_with_param(
            transaction,
            "SELECT * FROM messages WHERE owner_did = ?1 ORDER BY COALESCE(sent_at, stored_at) ASC, msg_id ASC",
            old_owner,
        )?;
        count += rows.len() as i64;
        for row in rows {
            let mut record = normalize_recovered_message_row(
                &row,
                old_owner_set,
                new_owner_did,
                final_credential_name,
            );
            if let Some(existing) = query_one_map2(
                transaction,
                "SELECT * FROM messages WHERE owner_did = ?1 AND msg_id = ?2",
                new_owner_did,
                &record.msg_id,
            )? {
                record = merge_recovered_message(&existing, record);
            }
            upsert_recovered_message(transaction, &record)?;
        }
    }
    delete_rows_for_owners(transaction, "messages", old_owner_dids)?;
    Ok(count)
}

fn merge_recovered_contacts(
    transaction: &Transaction<'_>,
    old_owner_dids: &[String],
    new_owner_did: &str,
    final_credential_name: &str,
    affected_handles: &mut BTreeSet<String>,
) -> StoreResult<i64> {
    let mut count = 0;
    for old_owner in old_owner_dids {
        let rows = query_maps_with_param(
            transaction,
            "SELECT * FROM contacts WHERE owner_did = ?1 ORDER BY COALESCE(last_seen_at, first_seen_at, connected_at) ASC, did ASC",
            old_owner,
        )?;
        count += rows.len() as i64;
        for row in rows {
            let mut record =
                normalize_recovered_contact_row(&row, new_owner_did, final_credential_name);
            if !record.handle.trim().is_empty() {
                affected_handles.insert(record.handle.trim().to_string());
            }
            if let Some(existing) = query_one_map2(
                transaction,
                "SELECT * FROM contacts WHERE owner_did = ?1 AND did = ?2",
                new_owner_did,
                &record.did,
            )? {
                record = merge_recovered_contact(&existing, record);
            }
            upsert_recovered_contact(transaction, &record)?;
        }
    }
    delete_rows_for_owners(transaction, "contacts", old_owner_dids)?;
    Ok(count)
}

fn merge_recovered_contact_handle_bindings(
    transaction: &Transaction<'_>,
    old_owner_dids: &[String],
    new_owner_did: &str,
    final_credential_name: &str,
    affected_handles: &mut BTreeSet<String>,
) -> StoreResult<i64> {
    let mut count = 0;
    for old_owner in old_owner_dids {
        let rows = query_maps_with_param(
            transaction,
            "SELECT * FROM contact_handle_bindings WHERE owner_did = ?1 ORDER BY COALESCE(last_seen_at, first_seen_at) ASC, handle ASC, did ASC",
            old_owner,
        )?;
        count += rows.len() as i64;
        for row in rows {
            let mut record = normalize_recovered_contact_handle_binding_row(
                &row,
                new_owner_did,
                final_credential_name,
            );
            if !record.handle.trim().is_empty() {
                affected_handles.insert(record.handle.trim().to_string());
            }
            if let Some(existing) = query_one_map3(
                transaction,
                "SELECT * FROM contact_handle_bindings WHERE owner_did = ?1 AND handle = ?2 AND did = ?3",
                new_owner_did,
                &record.handle,
                &record.did,
            )? {
                record = merge_recovered_contact_handle_binding(&existing, record);
            }
            upsert_recovered_contact_handle_binding(transaction, &record)?;
        }
    }
    delete_rows_for_owners(transaction, "contact_handle_bindings", old_owner_dids)?;
    Ok(count)
}

fn merge_recovered_relationship_events(
    transaction: &Transaction<'_>,
    old_owner_dids: &[String],
    new_owner_did: &str,
    final_credential_name: &str,
) -> StoreResult<i64> {
    let mut count = 0;
    for old_owner in old_owner_dids {
        let rows = query_maps_with_param(
            transaction,
            "SELECT * FROM relationship_events WHERE owner_did = ?1 ORDER BY COALESCE(updated_at, created_at) ASC, event_id ASC",
            old_owner,
        )?;
        count += rows.len() as i64;
        for row in rows {
            let mut record = normalize_recovered_relationship_event_row(
                &row,
                new_owner_did,
                final_credential_name,
            );
            if let Some(existing) = query_one_map1(
                transaction,
                "SELECT * FROM relationship_events WHERE event_id = ?1",
                &record.event_id,
            )? {
                record = merge_recovered_relationship_event(&existing, record);
            }
            upsert_recovered_relationship_event(transaction, &record)?;
        }
    }
    delete_rows_for_owners(transaction, "relationship_events", old_owner_dids)?;
    Ok(count)
}

fn merge_recovered_groups(
    transaction: &Transaction<'_>,
    old_owner_dids: &[String],
    old_owner_set: &BTreeSet<String>,
    new_owner_did: &str,
    final_credential_name: &str,
) -> StoreResult<i64> {
    let mut count = 0;
    for old_owner in old_owner_dids {
        let rows = query_maps_with_param(
            transaction,
            "SELECT * FROM groups WHERE owner_did = ?1 ORDER BY COALESCE(remote_updated_at, last_message_at, stored_at) ASC, group_id ASC",
            old_owner,
        )?;
        count += rows.len() as i64;
        for row in rows {
            let mut record = normalize_recovered_group_row(
                &row,
                old_owner_set,
                new_owner_did,
                final_credential_name,
            );
            if let Some(existing) = query_one_map2(
                transaction,
                "SELECT * FROM groups WHERE owner_did = ?1 AND group_id = ?2",
                new_owner_did,
                &record.group_id,
            )? {
                record = merge_recovered_group(&existing, record);
            }
            upsert_recovered_group(transaction, &record)?;
        }
    }
    delete_rows_for_owners(transaction, "groups", old_owner_dids)?;
    Ok(count)
}

fn merge_recovered_group_members(
    transaction: &Transaction<'_>,
    old_owner_dids: &[String],
    old_owner_set: &BTreeSet<String>,
    new_owner_did: &str,
    final_credential_name: &str,
) -> StoreResult<i64> {
    let mut count = 0;
    for old_owner in old_owner_dids {
        let rows = query_maps_with_param(
            transaction,
            "SELECT * FROM group_members WHERE owner_did = ?1 ORDER BY group_id ASC, user_id ASC",
            old_owner,
        )?;
        count += rows.len() as i64;
        for row in rows {
            let mut record = normalize_recovered_group_member_row(
                &row,
                old_owner_set,
                new_owner_did,
                final_credential_name,
            );
            if let Some(existing) = query_one_map3(
                transaction,
                "SELECT * FROM group_members WHERE owner_did = ?1 AND group_id = ?2 AND user_id = ?3",
                new_owner_did,
                &record.group_id,
                &record.user_id,
            )? {
                record = merge_recovered_group_member(&existing, record);
            }
            upsert_recovered_group_member(transaction, &record)?;
        }
    }
    delete_rows_for_owners(transaction, "group_members", old_owner_dids)?;
    Ok(count)
}

fn clear_recovered_owner_e2ee_data(
    transaction: &Transaction<'_>,
    old_owner_dids: &[String],
) -> StoreResult<BTreeMap<String, i64>> {
    let mut result = zero_counts(E2EE_CLEANUP_TABLES);
    for table in E2EE_CLEANUP_TABLES {
        let total = count_rows_for_owners(transaction, table, old_owner_dids)?;
        result.insert((*table).to_string(), total);
        delete_rows_for_owners(transaction, table, old_owner_dids)?;
    }
    Ok(result)
}

mod helpers {
    use std::time::{SystemTime, UNIX_EPOCH};
    use time::OffsetDateTime;

    pub(super) type StoreResult<T> = crate::ImResult<T>;

    pub(super) fn now_utc() -> String {
        let value = OffsetDateTime::now_utc();
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
            value.year(),
            u8::from(value.month()),
            value.day(),
            value.hour(),
            value.minute(),
            value.second()
        )
    }

    pub(super) fn make_thread_id(my_did: &str, peer_did: &str, group_id: &str) -> String {
        let group_id = group_id.trim();
        if !group_id.is_empty() {
            return format!("group:{group_id}");
        }
        let peer_did = peer_did.trim();
        let my_did = my_did.trim();
        if !peer_did.is_empty() {
            let mut pair = [my_did.to_string(), peer_did.to_string()];
            pair.sort();
            return format!("dm:{}:{}", pair[0], pair[1]);
        }
        format!("dm:{my_did}:unknown")
    }

    pub(super) fn normalize_owner_did(value: &str) -> String {
        value.trim().to_string()
    }

    pub(super) fn normalize_credential_name(value: &str) -> String {
        value.trim().to_string()
    }

    pub(super) fn normalize_optional_string(value: &str) -> Option<String> {
        let value = value.trim();
        if value.is_empty() {
            None
        } else {
            Some(value.to_string())
        }
    }

    pub(super) fn normalize_optional_bool(value: Option<bool>) -> Option<i64> {
        value.map(bool_to_int)
    }

    pub(super) fn normalize_optional_int64(value: Option<i64>) -> Option<i64> {
        value
    }

    pub(super) fn normalize_optional_float64(value: Option<f64>) -> Option<f64> {
        value
    }

    pub(super) fn normalize_metadata(value: &str) -> Option<String> {
        normalize_optional_string(value)
    }

    pub(super) fn default_bool_value(value: Option<bool>) -> i64 {
        value.map(bool_to_int).unwrap_or(0)
    }

    pub(super) fn bool_to_int(value: bool) -> i64 {
        if value {
            1
        } else {
            0
        }
    }

    pub(super) fn default_string(value: String, fallback: &str) -> String {
        if value.trim().is_empty() {
            fallback.to_string()
        } else {
            value
        }
    }

    #[allow(dead_code)]
    pub(super) fn generate_id() -> String {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        format!("local-{nanos}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::collections::BTreeMap;
    use std::path::Path;

    const STORE_KEYS: &[&str] = &[
        "messages",
        "contacts",
        "contact_handle_bindings",
        "relationship_events",
        "groups",
        "group_members",
    ];

    const E2EE_KEYS: &[&str] = &["e2ee_outbox", "e2ee_sessions"];

    #[test]
    fn recover_merge_missing_database_returns_zero_counts_and_does_not_create_db() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let sqlite_path = sqlite_path(temp.path());

        let (store_merge, e2ee_cleanup) = merge_recovered_handle_local_state(
            &sqlite_path,
            &["did:old"],
            "did:new",
            "final-credential",
        )
        .expect("missing database is a soft no-op");

        assert_zero_counts(&store_merge, STORE_KEYS);
        assert_zero_counts(&e2ee_cleanup, E2EE_KEYS);
        assert!(
            !sqlite_path.exists(),
            "missing-DB no-op must not create the SQLite file"
        );
    }

    #[test]
    fn recover_merge_existing_empty_database_ensures_schema_and_returns_zero_counts() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let sqlite_path = sqlite_path(temp.path());
        std::fs::create_dir_all(sqlite_path.parent().unwrap()).expect("data dir");
        drop(Connection::open(&sqlite_path).expect("create empty sqlite"));

        let (store_merge, e2ee_cleanup) = merge_recovered_handle_local_state(
            &sqlite_path,
            &["did:old"],
            "did:new",
            "final-credential",
        )
        .expect("existing empty database should be migrated before merge");

        assert_zero_counts(&store_merge, STORE_KEYS);
        assert_zero_counts(&e2ee_cleanup, E2EE_KEYS);

        let verify = Connection::open(&sqlite_path).expect("open database");
        assert_eq!(
            crate::internal::local_state::schema::current_schema_version(&verify)
                .expect("schema version"),
            crate::internal::local_state::schema::SCHEMA_VERSION
        );
        for table in STORE_KEYS.iter().chain(E2EE_KEYS.iter()) {
            assert_table_exists(&verify, table);
        }
    }

    #[test]
    fn recover_merge_moves_only_target_owners_and_cleans_only_old_owner_e2ee() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let sqlite_path = sqlite_path(temp.path());
        let connection = create_database(&sqlite_path);
        seed_recover_main_rows(&connection).expect("seed rows");
        drop(connection);

        let (store_merge, e2ee_cleanup) = merge_recovered_handle_local_state(
            &sqlite_path,
            &[
                " did:owner:old-1 ",
                "",
                "did:owner:new",
                "did:owner:old-2",
                "did:owner:old-1",
            ],
            "did:owner:new",
            "final-credential",
        )
        .expect("merge recovered local state");

        assert_eq!(store_merge["messages"], 2);
        assert_eq!(store_merge["contacts"], 2);
        assert_eq!(store_merge["contact_handle_bindings"], 2);
        assert_eq!(store_merge["relationship_events"], 1);
        assert_eq!(store_merge["groups"], 1);
        assert_eq!(store_merge["group_members"], 1);
        assert_eq!(e2ee_cleanup["e2ee_outbox"], 1);
        assert_eq!(e2ee_cleanup["e2ee_sessions"], 1);

        let verify = Connection::open(&sqlite_path).expect("open database");
        for table in STORE_KEYS.iter().chain(E2EE_KEYS.iter()) {
            assert_owner_count(&verify, table, "did:owner:old-1", 0);
            assert_owner_count(&verify, table, "did:owner:old-2", 0);
        }
        assert_owner_count(&verify, "messages", "did:owner:new", 2);
        assert_owner_count(&verify, "contacts", "did:owner:new", 2);
        assert_owner_count(&verify, "contact_handle_bindings", "did:owner:new", 2);
        assert_owner_count(&verify, "relationship_events", "did:owner:new", 1);
        assert_owner_count(&verify, "groups", "did:owner:new", 1);
        assert_owner_count(&verify, "group_members", "did:owner:new", 1);
        assert_owner_count(&verify, "messages", "did:owner:other", 1);
        assert_owner_count(&verify, "contacts", "did:owner:other", 1);
        assert_owner_count(&verify, "e2ee_outbox", "did:owner:new", 1);
        assert_owner_count(&verify, "e2ee_sessions", "did:owner:new", 1);
        assert_owner_count(&verify, "e2ee_outbox", "did:owner:other", 1);

        assert_eq!(
            scalar_string(
                &verify,
                "SELECT receiver_did FROM messages WHERE owner_did = 'did:owner:new' AND msg_id = 'msg-old-1'",
            ),
            "did:owner:new"
        );
        assert_eq!(
            scalar_string(
                &verify,
                "SELECT thread_id FROM messages WHERE owner_did = 'did:owner:new' AND msg_id = 'msg-old-1'",
            ),
            expected_thread_id("did:owner:new", "did:peer:bob", "")
        );
        assert_eq!(
            scalar_string(
                &verify,
                "SELECT sender_did FROM messages WHERE owner_did = 'did:owner:new' AND msg_id = 'msg-old-2'",
            ),
            "did:owner:new"
        );
        assert_eq!(
            scalar_string(
                &verify,
                "SELECT credential_name FROM messages WHERE owner_did = 'did:owner:new' AND msg_id = 'msg-old-2'",
            ),
            "final-credential"
        );

        assert_eq!(
            scalar_string(
                &verify,
                "SELECT did FROM contact_handle_bindings WHERE owner_did = 'did:owner:new' AND handle = 'alice' AND is_current = 1",
            ),
            "did:peer:new"
        );
        assert_eq!(
            scalar_opt_string(
                &verify,
                "SELECT handle FROM contacts WHERE owner_did = 'did:owner:new' AND did = 'did:peer:old'",
            ),
            None
        );
        assert_eq!(
            scalar_opt_string(
                &verify,
                "SELECT handle FROM contacts WHERE owner_did = 'did:owner:new' AND did = 'did:peer:new'",
            ),
            Some("alice".to_string())
        );

        assert_eq!(
            scalar_string(
                &verify,
                "SELECT group_owner_did FROM groups WHERE owner_did = 'did:owner:new' AND group_id = 'group:one'",
            ),
            "did:owner:new"
        );
        assert_eq!(
            scalar_string(
                &verify,
                "SELECT member_did FROM group_members WHERE owner_did = 'did:owner:new' AND group_id = 'group:one' AND user_id = 'user-z'",
            ),
            "did:owner:new"
        );
    }

    #[test]
    fn recover_merge_counts_source_rows_before_conflicts_and_applies_merge_algebra() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let sqlite_path = sqlite_path(temp.path());
        let connection = create_database(&sqlite_path);
        seed_recover_conflict_rows(&connection).expect("seed rows");
        drop(connection);

        let (store_merge, e2ee_cleanup) = merge_recovered_handle_local_state(
            &sqlite_path,
            &["did:owner:old"],
            "did:owner:new",
            "final-credential",
        )
        .expect("merge recovered conflicts");

        for key in STORE_KEYS {
            assert_eq!(store_merge[*key], 1, "source count for {key}");
        }
        assert_zero_counts(&e2ee_cleanup, E2EE_KEYS);

        let verify = Connection::open(&sqlite_path).expect("open database");
        for table in STORE_KEYS {
            assert_owner_count(&verify, table, "did:owner:old", 0);
        }

        assert_eq!(
            scalar_string(&verify, "SELECT content FROM messages WHERE owner_did = 'did:owner:new' AND msg_id = 'msg-conflict'"),
            "incoming content"
        );
        assert_eq!(
            scalar_i64(&verify, "SELECT server_seq FROM messages WHERE owner_did = 'did:owner:new' AND msg_id = 'msg-conflict'"),
            9
        );
        assert_eq!(
            scalar_string(&verify, "SELECT sent_at FROM messages WHERE owner_did = 'did:owner:new' AND msg_id = 'msg-conflict'"),
            "2026-01-03T00:00:00Z"
        );
        assert_eq!(
            scalar_i64(&verify, "SELECT is_e2ee FROM messages WHERE owner_did = 'did:owner:new' AND msg_id = 'msg-conflict'"),
            1
        );
        assert_eq!(
            scalar_i64(&verify, "SELECT is_read FROM messages WHERE owner_did = 'did:owner:new' AND msg_id = 'msg-conflict'"),
            1
        );

        assert_eq!(
            scalar_string(&verify, "SELECT name FROM contacts WHERE owner_did = 'did:owner:new' AND did = 'did:peer:conflict'"),
            "Incoming Alice"
        );
        assert_eq!(
            scalar_string(&verify, "SELECT first_seen_at FROM contacts WHERE owner_did = 'did:owner:new' AND did = 'did:peer:conflict'"),
            "2026-01-01T00:00:00Z"
        );
        assert_eq!(
            scalar_string(&verify, "SELECT last_seen_at FROM contacts WHERE owner_did = 'did:owner:new' AND did = 'did:peer:conflict'"),
            "2026-01-10T00:00:00Z"
        );
        assert_eq!(
            scalar_i64(&verify, "SELECT followed FROM contacts WHERE owner_did = 'did:owner:new' AND did = 'did:peer:conflict'"),
            1
        );
        assert_eq!(
            scalar_i64(&verify, "SELECT messaged FROM contacts WHERE owner_did = 'did:owner:new' AND did = 'did:peer:conflict'"),
            1
        );

        assert_eq!(
            scalar_string(&verify, "SELECT first_seen_at FROM contact_handle_bindings WHERE owner_did = 'did:owner:new' AND handle = 'alice' AND did = 'did:peer:conflict'"),
            "2026-01-01T00:00:00Z"
        );
        assert_eq!(
            scalar_string(&verify, "SELECT last_seen_at FROM contact_handle_bindings WHERE owner_did = 'did:owner:new' AND handle = 'alice' AND did = 'did:peer:conflict'"),
            "2026-01-10T00:00:00Z"
        );

        assert_eq!(
            scalar_string(&verify, "SELECT name FROM groups WHERE owner_did = 'did:owner:new' AND group_id = 'group-conflict'"),
            "Incoming Group"
        );
        assert_eq!(
            scalar_string(&verify, "SELECT group_owner_did FROM groups WHERE owner_did = 'did:owner:new' AND group_id = 'group-conflict'"),
            "did:owner:new"
        );
        assert_eq!(
            scalar_i64(&verify, "SELECT member_count FROM groups WHERE owner_did = 'did:owner:new' AND group_id = 'group-conflict'"),
            7
        );
        assert_eq!(
            scalar_string(&verify, "SELECT remote_created_at FROM groups WHERE owner_did = 'did:owner:new' AND group_id = 'group-conflict'"),
            "2026-01-01T00:00:00Z"
        );
        assert_eq!(
            scalar_string(&verify, "SELECT remote_updated_at FROM groups WHERE owner_did = 'did:owner:new' AND group_id = 'group-conflict'"),
            "2026-01-10T00:00:00Z"
        );

        assert_eq!(
            scalar_string(&verify, "SELECT member_did FROM group_members WHERE owner_did = 'did:owner:new' AND group_id = 'group-conflict' AND user_id = 'user-conflict'"),
            "did:owner:new"
        );
        assert_eq!(
            scalar_i64(&verify, "SELECT sent_message_count FROM group_members WHERE owner_did = 'did:owner:new' AND group_id = 'group-conflict' AND user_id = 'user-conflict'"),
            8
        );
        assert_eq!(
            scalar_string(&verify, "SELECT joined_at FROM group_members WHERE owner_did = 'did:owner:new' AND group_id = 'group-conflict' AND user_id = 'user-conflict'"),
            "2026-01-01T00:00:00Z"
        );
        assert_eq!(
            scalar_string(&verify, "SELECT last_synced_at FROM group_members WHERE owner_did = 'did:owner:new' AND group_id = 'group-conflict' AND user_id = 'user-conflict'"),
            "2026-01-10T00:00:00Z"
        );
    }

    #[test]
    fn recover_merge_current_contact_handle_uses_latest_binding_then_did_desc_tiebreak() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let sqlite_path = sqlite_path(temp.path());
        let connection = create_database(&sqlite_path);
        seed_contact_with_binding(
            &connection,
            "did:owner:old-a",
            "did:peer:a",
            "same",
            "2026-02-01T00:00:00Z",
            "2026-02-10T00:00:00Z",
        )
        .expect("seed a");
        seed_contact_with_binding(
            &connection,
            "did:owner:old-z",
            "did:peer:z",
            "same",
            "2026-02-01T00:00:00Z",
            "2026-02-10T00:00:00Z",
        )
        .expect("seed z");
        seed_contact_with_binding(
            &connection,
            "did:owner:old-old",
            "did:peer:old",
            "same",
            "2026-02-01T00:00:00Z",
            "2026-02-09T00:00:00Z",
        )
        .expect("seed old");
        drop(connection);

        merge_recovered_handle_local_state(
            &sqlite_path,
            &["did:owner:old-a", "did:owner:old-z", "did:owner:old-old"],
            "did:owner:new",
            "final-credential",
        )
        .expect("merge contacts");

        let verify = Connection::open(&sqlite_path).expect("open database");
        assert_eq!(
            scalar_string(
                &verify,
                "SELECT did FROM contact_handle_bindings WHERE owner_did = 'did:owner:new' AND handle = 'same' AND is_current = 1",
            ),
            "did:peer:z"
        );
        assert_eq!(
            scalar_opt_string(
                &verify,
                "SELECT handle FROM contacts WHERE owner_did = 'did:owner:new' AND did = 'did:peer:z'",
            ),
            Some("same".to_string())
        );
        for did in ["did:peer:a", "did:peer:old"] {
            assert_eq!(
                scalar_opt_string(
                    &verify,
                    &format!(
                        "SELECT handle FROM contacts WHERE owner_did = 'did:owner:new' AND did = '{did}'"
                    ),
                ),
                None,
                "{did} should no longer own the current handle"
            );
        }
    }

    #[test]
    fn recover_merge_relationship_events_move_by_global_event_id_not_owner_scope() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let sqlite_path = sqlite_path(temp.path());
        let connection = create_database(&sqlite_path);
        connection
            .execute(
                r#"
INSERT INTO relationship_events (
    event_id, owner_did, target_did, target_handle, event_type, source_type,
    reason, score, status, created_at, updated_at, metadata, credential_name
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
"#,
                rusqlite::params![
                    "evt-global",
                    "did:owner:old",
                    "did:peer:target",
                    "target",
                    "follow",
                    "contacts",
                    "because",
                    0.42_f64,
                    "accepted",
                    "2026-03-01T00:00:00Z",
                    "2026-03-02T00:00:00Z",
                    "{\"old\":true}",
                    "old-credential",
                ],
            )
            .expect("insert relationship event");
        drop(connection);

        let (store_merge, e2ee_cleanup) = merge_recovered_handle_local_state(
            &sqlite_path,
            &["did:owner:old"],
            "did:owner:new",
            "final-credential",
        )
        .expect("merge relationship event");

        assert_eq!(store_merge["relationship_events"], 1);
        assert_zero_counts(&e2ee_cleanup, E2EE_KEYS);

        let verify = Connection::open(&sqlite_path).expect("open database");
        assert_eq!(
            scalar_i64(
                &verify,
                "SELECT COUNT(*) FROM relationship_events WHERE event_id = 'evt-global'",
            ),
            1
        );
        assert_eq!(
            scalar_string(
                &verify,
                "SELECT owner_did FROM relationship_events WHERE event_id = 'evt-global'",
            ),
            "did:owner:new"
        );
        assert_eq!(
            scalar_string(
                &verify,
                "SELECT credential_name FROM relationship_events WHERE event_id = 'evt-global'",
            ),
            "final-credential"
        );
        assert_eq!(
            scalar_opt_f64(
                &verify,
                "SELECT score FROM relationship_events WHERE event_id = 'evt-global'",
            ),
            Some(0.42)
        );
    }

    fn seed_recover_main_rows(connection: &Connection) -> rusqlite::Result<()> {
        connection.execute(
            r#"
INSERT INTO messages (
    msg_id, owner_did, thread_id, direction, sender_did, receiver_did,
    content_type, content, sent_at, stored_at, credential_name
) VALUES (?1, ?2, ?3, 0, ?4, ?5, 'text', ?6, ?7, ?7, ?8)
"#,
            rusqlite::params![
                "msg-old-1",
                "did:owner:old-1",
                expected_thread_id("did:owner:old-1", "did:peer:bob", ""),
                "did:peer:bob",
                "did:owner:old-1",
                "hello old one",
                "2026-04-20T10:00:00Z",
                "old-credential-1",
            ],
        )?;
        connection.execute(
            r#"
INSERT INTO messages (
    msg_id, owner_did, thread_id, direction, sender_did, receiver_did,
    content_type, content, sent_at, stored_at, credential_name
) VALUES (?1, ?2, ?3, 1, ?4, ?5, 'text', ?6, ?7, ?7, ?8)
"#,
            rusqlite::params![
                "msg-old-2",
                "did:owner:old-2",
                expected_thread_id("did:owner:old-2", "did:peer:bob", ""),
                "did:owner:old-2",
                "did:peer:bob",
                "hello old two",
                "2026-04-20T11:00:00Z",
                "old-credential-2",
            ],
        )?;
        connection.execute(
            r#"
INSERT INTO messages (
    msg_id, owner_did, thread_id, direction, sender_did, receiver_did,
    content_type, content, sent_at, stored_at, credential_name
) VALUES (?1, ?2, ?3, 0, ?4, ?5, 'text', ?6, ?7, ?7, ?8)
"#,
            rusqlite::params![
                "msg-other",
                "did:owner:other",
                expected_thread_id("did:owner:other", "did:peer:bob", ""),
                "did:peer:bob",
                "did:owner:other",
                "hello other",
                "2026-04-20T12:00:00Z",
                "other-credential",
            ],
        )?;

        seed_contact_with_binding(
            connection,
            "did:owner:old-1",
            "did:peer:old",
            "alice",
            "2026-04-20T10:00:00Z",
            "2026-04-20T10:00:00Z",
        )?;
        seed_contact_with_binding(
            connection,
            "did:owner:old-2",
            "did:peer:new",
            "alice",
            "2026-04-20T11:00:00Z",
            "2026-04-20T11:00:00Z",
        )?;
        seed_contact_with_binding(
            connection,
            "did:owner:other",
            "did:peer:else",
            "else",
            "2026-04-20T09:00:00Z",
            "2026-04-20T09:00:00Z",
        )?;

        connection.execute(
            r#"
INSERT INTO relationship_events (
    event_id, owner_did, target_did, target_handle, event_type, status,
    created_at, updated_at, credential_name
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, ?8)
"#,
            rusqlite::params![
                "evt-1",
                "did:owner:old-1",
                "did:peer:old",
                "alice",
                "follow",
                "pending",
                "2026-04-20T10:00:00Z",
                "old-credential-1",
            ],
        )?;
        connection.execute(
            r#"
INSERT INTO groups (
    owner_did, group_id, group_did, name, group_mode, group_owner_did,
    membership_status, last_message_at, stored_at, credential_name
) VALUES (?1, ?2, ?3, ?4, 'general', ?5, 'active', ?6, ?6, ?7)
"#,
            rusqlite::params![
                "did:owner:old-2",
                "group:one",
                "did:group:one",
                "Group One",
                "did:owner:old-2",
                "2026-04-20T11:30:00Z",
                "old-credential-2",
            ],
        )?;
        connection.execute(
            r#"
INSERT INTO group_members (
    owner_did, group_id, user_id, member_did, member_handle, status,
    sent_message_count, last_synced_at, credential_name
) VALUES (?1, ?2, ?3, ?4, ?5, 'active', 3, ?6, ?7)
"#,
            rusqlite::params![
                "did:owner:old-2",
                "group:one",
                "user-z",
                "did:owner:old-2",
                "zhuocheng",
                "2026-04-20T11:30:00Z",
                "old-credential-2",
            ],
        )?;
        seed_e2ee_outbox(
            connection,
            "out-old",
            "did:owner:old-1",
            "did:peer:bob",
            "old secret",
        )?;
        seed_e2ee_session(connection, "did:owner:old-2", "did:peer:bob", "session-old")?;
        seed_e2ee_outbox(
            connection,
            "out-new",
            "did:owner:new",
            "did:peer:bob",
            "new secret",
        )?;
        seed_e2ee_session(connection, "did:owner:new", "did:peer:bob", "session-new")?;
        seed_e2ee_outbox(
            connection,
            "out-other",
            "did:owner:other",
            "did:peer:bob",
            "other secret",
        )?;
        Ok(())
    }

    fn seed_recover_conflict_rows(connection: &Connection) -> rusqlite::Result<()> {
        seed_message_conflict(
            connection,
            "did:owner:new",
            "did:peer:conflict",
            "did:owner:new",
            "existing content",
            4,
            "2026-01-02T00:00:00Z",
            false,
            true,
            "existing-credential",
        )?;
        seed_message_conflict(
            connection,
            "did:owner:old",
            "did:owner:old",
            "did:peer:conflict",
            "incoming content",
            9,
            "2026-01-03T00:00:00Z",
            true,
            false,
            "old-credential",
        )?;
        seed_contact_conflict(
            connection,
            "did:owner:new",
            "Existing Alice",
            "2026-01-05T00:00:00Z",
            "2026-01-06T00:00:00Z",
            true,
            false,
        )?;
        seed_contact_conflict(
            connection,
            "did:owner:old",
            "Incoming Alice",
            "2026-01-01T00:00:00Z",
            "2026-01-10T00:00:00Z",
            false,
            true,
        )?;
        seed_binding_conflict(
            connection,
            "did:owner:new",
            "2026-01-05T00:00:00Z",
            "2026-01-06T00:00:00Z",
            "existing-binding",
        )?;
        seed_binding_conflict(
            connection,
            "did:owner:old",
            "2026-01-01T00:00:00Z",
            "2026-01-10T00:00:00Z",
            "incoming-binding",
        )?;
        connection.execute(
            r#"
INSERT INTO relationship_events (
    event_id, owner_did, target_did, target_handle, event_type, source_type,
    reason, score, status, created_at, updated_at, credential_name
) VALUES ('evt-conflict', 'did:owner:old', 'did:peer:conflict', 'alice', 'follow', 'contacts',
          'old reason', 2.5, 'pending', '2026-01-02T00:00:00Z', '2026-01-03T00:00:00Z', 'old-credential')
"#,
            [],
        )?;
        seed_group_conflict(
            connection,
            "did:owner:new",
            "Existing Group",
            "did:owner:someone",
            2,
            "2026-01-05T00:00:00Z",
            "2026-01-06T00:00:00Z",
        )?;
        seed_group_conflict(
            connection,
            "did:owner:old",
            "Incoming Group",
            "did:owner:old",
            7,
            "2026-01-01T00:00:00Z",
            "2026-01-10T00:00:00Z",
        )?;
        seed_group_member_conflict(
            connection,
            "did:owner:new",
            "did:owner:someone",
            3,
            "2026-01-05T00:00:00Z",
            "2026-01-06T00:00:00Z",
        )?;
        seed_group_member_conflict(
            connection,
            "did:owner:old",
            "did:owner:old",
            8,
            "2026-01-01T00:00:00Z",
            "2026-01-10T00:00:00Z",
        )?;
        Ok(())
    }

    fn seed_message_conflict(
        connection: &Connection,
        owner_did: &str,
        sender_did: &str,
        receiver_did: &str,
        content: &str,
        server_seq: i64,
        at: &str,
        is_e2ee: bool,
        is_read: bool,
        credential_name: &str,
    ) -> rusqlite::Result<()> {
        connection.execute(
            r#"
INSERT INTO messages (
    msg_id, owner_did, thread_id, direction, sender_did, receiver_did,
    content_type, content, title, server_seq, sent_at, stored_at, is_e2ee,
    is_read, sender_name, metadata, credential_name
) VALUES (?1, ?2, ?3, 0, ?4, ?5, 'text', ?6, 'incoming title', ?7, ?8, ?8, ?9, ?10, 'sender', '{"kind":"msg"}', ?11)
"#,
            rusqlite::params![
                "msg-conflict",
                owner_did,
                expected_thread_id(owner_did, "did:peer:conflict", ""),
                sender_did,
                receiver_did,
                content,
                server_seq,
                at,
                if is_e2ee { 1 } else { 0 },
                if is_read { 1 } else { 0 },
                credential_name,
            ],
        )?;
        Ok(())
    }

    fn seed_contact_conflict(
        connection: &Connection,
        owner_did: &str,
        name: &str,
        first_seen_at: &str,
        last_seen_at: &str,
        followed: bool,
        messaged: bool,
    ) -> rusqlite::Result<()> {
        connection.execute(
            r#"
INSERT INTO contacts (
    owner_did, did, name, handle, followed, messaged, first_seen_at, last_seen_at, metadata
) VALUES (?1, 'did:peer:conflict', ?2, 'alice', ?3, ?4, ?5, ?6, '{"kind":"contact"}')
"#,
            rusqlite::params![
                owner_did,
                name,
                if followed { 1 } else { 0 },
                if messaged { 1 } else { 0 },
                first_seen_at,
                last_seen_at,
            ],
        )?;
        Ok(())
    }

    fn seed_binding_conflict(
        connection: &Connection,
        owner_did: &str,
        first_seen_at: &str,
        last_seen_at: &str,
        metadata: &str,
    ) -> rusqlite::Result<()> {
        connection.execute(
            r#"
INSERT INTO contact_handle_bindings (
    owner_did, handle, did, is_current, first_seen_at, last_seen_at, metadata, credential_name
) VALUES (?1, 'alice', 'did:peer:conflict', 1, ?2, ?3, ?4, 'old-credential')
"#,
            rusqlite::params![owner_did, first_seen_at, last_seen_at, metadata],
        )?;
        Ok(())
    }

    fn seed_group_conflict(
        connection: &Connection,
        owner_did: &str,
        name: &str,
        group_owner_did: &str,
        member_count: i64,
        remote_created_at: &str,
        remote_updated_at: &str,
    ) -> rusqlite::Result<()> {
        connection.execute(
            r#"
INSERT INTO groups (
    owner_did, group_id, group_did, name, group_mode, group_owner_did,
    membership_status, member_count, remote_created_at, remote_updated_at,
    stored_at, credential_name
) VALUES (?1, 'group-conflict', 'did:group:conflict', ?2, 'general', ?3, 'active', ?4, ?5, ?6, ?6, 'old-credential')
"#,
            rusqlite::params![
                owner_did,
                name,
                group_owner_did,
                member_count,
                remote_created_at,
                remote_updated_at,
            ],
        )?;
        Ok(())
    }

    fn seed_group_member_conflict(
        connection: &Connection,
        owner_did: &str,
        member_did: &str,
        sent_message_count: i64,
        joined_at: &str,
        last_synced_at: &str,
    ) -> rusqlite::Result<()> {
        connection.execute(
            r#"
INSERT INTO group_members (
    owner_did, group_id, user_id, member_did, member_handle, status,
    joined_at, sent_message_count, last_synced_at, credential_name
) VALUES (?1, 'group-conflict', 'user-conflict', ?2, 'member', 'active', ?3, ?4, ?5, 'old-credential')
"#,
            rusqlite::params![
                owner_did,
                member_did,
                joined_at,
                sent_message_count,
                last_synced_at,
            ],
        )?;
        Ok(())
    }

    fn seed_contact_with_binding(
        connection: &Connection,
        owner_did: &str,
        did: &str,
        handle: &str,
        first_seen_at: &str,
        last_seen_at: &str,
    ) -> rusqlite::Result<()> {
        connection.execute(
            r#"
INSERT INTO contacts (
    owner_did, did, handle, first_seen_at, last_seen_at
) VALUES (?1, ?2, ?3, ?4, ?5)
"#,
            rusqlite::params![owner_did, did, handle, first_seen_at, last_seen_at],
        )?;
        connection.execute(
            r#"
INSERT INTO contact_handle_bindings (
    owner_did, handle, did, is_current, first_seen_at, last_seen_at, credential_name
) VALUES (?1, ?2, ?3, 1, ?4, ?5, 'old-credential')
"#,
            rusqlite::params![owner_did, handle, did, first_seen_at, last_seen_at],
        )?;
        Ok(())
    }

    fn seed_e2ee_outbox(
        connection: &Connection,
        outbox_id: &str,
        owner_did: &str,
        peer_did: &str,
        plaintext: &str,
    ) -> rusqlite::Result<()> {
        connection.execute(
            r#"
INSERT INTO e2ee_outbox (
    outbox_id, owner_did, peer_did, plaintext, created_at, updated_at, credential_name
) VALUES (?1, ?2, ?3, ?4, '2026-04-20T10:00:00Z', '2026-04-20T10:00:00Z', 'old-credential')
"#,
            rusqlite::params![outbox_id, owner_did, peer_did, plaintext],
        )?;
        Ok(())
    }

    fn seed_e2ee_session(
        connection: &Connection,
        owner_did: &str,
        peer_did: &str,
        session_id: &str,
    ) -> rusqlite::Result<()> {
        connection.execute(
            r#"
INSERT INTO e2ee_sessions (
    owner_did, peer_did, session_id, is_initiator, send_chain_key, recv_chain_key,
    send_seq, recv_seq, expires_at, created_at, active_at, peer_confirmed,
    credential_name, updated_at
) VALUES (?1, ?2, ?3, 1, 'send-key', 'recv-key', 0, 0, NULL, '2026-04-20T10:00:00Z', NULL, 0, 'old-credential', '2026-04-20T10:00:00Z')
"#,
            rusqlite::params![owner_did, peer_did, session_id],
        )?;
        Ok(())
    }

    fn create_database(sqlite_path: &Path) -> Connection {
        crate::internal::local_state::open_writable(sqlite_path).expect("open database")
    }

    fn assert_zero_counts(counts: &BTreeMap<String, i64>, keys: &[&str]) {
        assert_eq!(counts.len(), keys.len(), "count keys: {counts:?}");
        for key in keys {
            assert_eq!(counts[*key], 0, "zero count for {key}");
        }
    }

    fn assert_table_exists(connection: &Connection, table: &str) {
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                rusqlite::params![table],
                |row| row.get(0),
            )
            .expect("table lookup");
        assert_eq!(count, 1, "{table} should exist");
    }

    fn assert_owner_count(connection: &Connection, table: &str, owner_did: &str, want: i64) {
        let got = scalar_i64(
            connection,
            &format!("SELECT COUNT(*) FROM {table} WHERE owner_did = '{owner_did}'"),
        );
        assert_eq!(got, want, "owner count for {table}/{owner_did}");
    }

    fn scalar_string(connection: &Connection, sql: &str) -> String {
        connection
            .query_row(sql, [], |row| row.get::<_, String>(0))
            .expect(sql)
    }

    fn scalar_opt_string(connection: &Connection, sql: &str) -> Option<String> {
        connection
            .query_row(sql, [], |row| row.get::<_, Option<String>>(0))
            .expect(sql)
    }

    fn scalar_i64(connection: &Connection, sql: &str) -> i64 {
        connection
            .query_row(sql, [], |row| row.get::<_, i64>(0))
            .expect(sql)
    }

    fn scalar_opt_f64(connection: &Connection, sql: &str) -> Option<f64> {
        connection
            .query_row(sql, [], |row| row.get::<_, Option<f64>>(0))
            .expect(sql)
    }

    fn sqlite_path(root: &Path) -> std::path::PathBuf {
        root.join("local").join("im.sqlite")
    }

    fn expected_thread_id(my_did: &str, peer_did: &str, group_id: &str) -> String {
        helpers::make_thread_id(my_did, peer_did, group_id)
    }
}
