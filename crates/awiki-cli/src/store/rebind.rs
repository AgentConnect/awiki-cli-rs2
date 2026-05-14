use super::{open, StoreResult};
use crate::config::Paths;
use rusqlite::{params, Connection};
use std::collections::BTreeMap;
use std::path::Path;

const REBIND_TABLES: &[&str] = &[
    "messages",
    "contacts",
    "contact_handle_bindings",
    "relationship_events",
    "groups",
    "group_members",
];

const LOCAL_REBIND_TABLES: &[&str] = &[
    "messages",
    "contacts",
    "relationship_events",
    "groups",
    "group_members",
];

const E2EE_TABLES: &[&str] = &["e2ee_outbox", "e2ee_sessions"];

pub fn rebind_local_identity_state(
    paths: &Paths,
    old_owner_did: &str,
    new_owner_did: &str,
) -> StoreResult<(BTreeMap<String, i64>, BTreeMap<String, i64>)> {
    let mut store_rebind = zero_counts(LOCAL_REBIND_TABLES);
    let mut e2ee_cleanup = zero_counts(E2EE_TABLES);
    if !store_file_exists(&paths.database_file) {
        return Ok((store_rebind, e2ee_cleanup));
    }

    let mut connection = open(paths)?;
    store_rebind = rebind_owner_did(&mut connection, old_owner_did, new_owner_did)?;
    e2ee_cleanup = clear_owner_e2ee_data(&connection, old_owner_did)?;
    Ok((store_rebind, e2ee_cleanup))
}

pub fn rebind_owner_did(
    connection: &mut Connection,
    old_owner_did: &str,
    new_owner_did: &str,
) -> StoreResult<BTreeMap<String, i64>> {
    let old_owner_did = normalize_owner_did(old_owner_did);
    let new_owner_did = normalize_owner_did(new_owner_did);
    let mut result = zero_counts(REBIND_TABLES);
    if old_owner_did.is_empty() || new_owner_did.is_empty() || old_owner_did == new_owner_did {
        return Ok(result);
    }

    let transaction = connection.transaction()?;
    for table in REBIND_TABLES {
        let count: i64 = transaction.query_row(
            &format!("SELECT COUNT(*) FROM {table} WHERE owner_did = ?1"),
            params![old_owner_did],
            |row| row.get(0),
        )?;
        result.insert((*table).to_string(), count);
        transaction.execute(
            &format!("UPDATE OR IGNORE {table} SET owner_did = ?1 WHERE owner_did = ?2"),
            params![new_owner_did, old_owner_did],
        )?;
    }
    transaction.commit()?;
    Ok(result)
}

pub fn clear_owner_e2ee_data(
    connection: &Connection,
    owner_did: &str,
) -> StoreResult<BTreeMap<String, i64>> {
    let owner_did = normalize_owner_did(owner_did);
    let mut result = zero_counts(E2EE_TABLES);
    if owner_did.is_empty() {
        return Ok(result);
    }

    for table in E2EE_TABLES {
        let count: i64 = connection.query_row(
            &format!("SELECT COUNT(*) FROM {table} WHERE owner_did = ?1"),
            params![owner_did],
            |row| row.get(0),
        )?;
        result.insert((*table).to_string(), count);
        connection.execute(
            &format!("DELETE FROM {table} WHERE owner_did = ?1"),
            params![owner_did],
        )?;
    }
    Ok(result)
}

fn zero_counts(tables: &[&str]) -> BTreeMap<String, i64> {
    tables
        .iter()
        .map(|table| ((*table).to_string(), 0))
        .collect()
}

fn normalize_owner_did(value: &str) -> String {
    value.trim().to_string()
}

fn store_file_exists(path: &str) -> bool {
    Path::new(path).metadata().is_ok()
}
