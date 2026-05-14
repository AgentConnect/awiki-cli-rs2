mod records;
mod sql;

use super::{ensure_schema, open, StoreResult};
use crate::config::Paths;
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

const STORE_MERGE_TABLES: &[&str] = &[
    "messages",
    "contacts",
    "contact_handle_bindings",
    "relationship_events",
    "groups",
    "group_members",
];
const E2EE_CLEANUP_TABLES: &[&str] = &["e2ee_outbox", "e2ee_sessions"];

pub fn merge_recovered_handle_local_state<S: AsRef<str>>(
    paths: &Paths,
    old_owner_dids: &[S],
    new_owner_did: &str,
    final_credential_name: &str,
) -> StoreResult<(BTreeMap<String, i64>, BTreeMap<String, i64>)> {
    let mut store_merge = zero_counts(STORE_MERGE_TABLES);
    let mut e2ee_cleanup = zero_counts(E2EE_CLEANUP_TABLES);
    if !store_file_exists(&paths.database_file) {
        return Ok((store_merge, e2ee_cleanup));
    }
    let owners = normalize_recover_owner_dids(old_owner_dids, new_owner_did);
    if owners.is_empty() {
        return Ok((store_merge, e2ee_cleanup));
    }

    let mut connection = open(paths)?;
    ensure_schema(&connection)?;
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
