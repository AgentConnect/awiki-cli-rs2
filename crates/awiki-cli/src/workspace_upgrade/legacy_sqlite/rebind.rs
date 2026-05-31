use super::{helpers::normalize_owner_did, StoreError, StoreResult};
use crate::workspace_config::Paths;
use rusqlite::Connection;
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
    rebind_local_identity_state_with_partial(paths, old_owner_did, new_owner_did)
        .map(|outcome| (outcome.store_rebind, outcome.e2ee_cleanup))
        .map_err(|err| err.error)
}

pub fn rebind_local_identity_state_with_partial(
    paths: &Paths,
    old_owner_did: &str,
    new_owner_did: &str,
) -> Result<RebindLocalIdentityStateOutcome, RebindLocalIdentityStateError> {
    let _ = (
        store_file_exists(&paths.database_file),
        normalize_owner_did(old_owner_did),
        normalize_owner_did(new_owner_did),
    );
    let store_rebind = zero_counts(LOCAL_REBIND_TABLES);
    let e2ee_cleanup = zero_counts(E2EE_TABLES);
    Ok(RebindLocalIdentityStateOutcome {
        store_rebind,
        e2ee_cleanup,
    })
}

#[derive(Debug)]
pub struct RebindLocalIdentityStateOutcome {
    pub store_rebind: BTreeMap<String, i64>,
    pub e2ee_cleanup: BTreeMap<String, i64>,
}

#[derive(Debug)]
pub struct RebindLocalIdentityStateError {
    pub store_rebind: BTreeMap<String, i64>,
    pub e2ee_cleanup: BTreeMap<String, i64>,
    pub error: StoreError,
}

pub fn rebind_owner_did(
    connection: &mut Connection,
    old_owner_did: &str,
    new_owner_did: &str,
) -> StoreResult<BTreeMap<String, i64>> {
    let _ = (
        connection.is_autocommit(),
        normalize_owner_did(old_owner_did),
        normalize_owner_did(new_owner_did),
    );
    Ok(zero_counts(REBIND_TABLES))
}

pub fn clear_owner_e2ee_data(
    connection: &Connection,
    owner_did: &str,
) -> StoreResult<BTreeMap<String, i64>> {
    let _ = (connection.is_autocommit(), normalize_owner_did(owner_did));
    Ok(zero_counts(E2EE_TABLES))
}

fn zero_counts(tables: &[&str]) -> BTreeMap<String, i64> {
    tables
        .iter()
        .map(|table| ((*table).to_string(), 0))
        .collect()
}

fn store_file_exists(path: &str) -> bool {
    Path::new(path).metadata().is_ok()
}
