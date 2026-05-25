mod contacts;
mod helpers;
mod import;
mod open;
mod query;
mod rebind;
mod schema;
mod types;

#[cfg(test)]
mod import_tests;
#[cfg(test)]
mod rebind_tests;

pub(crate) use contacts::list_contact_handle_history;
pub(crate) use import::{import_legacy_database, scan_legacy_database, LegacyOwnerLookup};
pub(crate) use open::{open, open_read_only};
pub(crate) use rebind::rebind_local_identity_state;
pub(crate) use schema::{current_schema_version, ensure_schema};
pub(crate) use types::{LegacyScan, StoreError, StoreResult, SCHEMA_VERSION};

#[cfg(test)]
pub(crate) use rebind::{clear_owner_e2ee_data, rebind_owner_did};
