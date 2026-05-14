mod helpers;
mod import;
mod open;
mod query;
mod rebind;
mod schema;
mod types;

pub use helpers::{make_thread_id, now_utc};
pub use import::{import_legacy_database, scan_legacy_database, LegacyOwnerLookup};
pub use open::{open, open_read_only};
pub use query::{execute_sql, list_notifications};
pub use rebind::{clear_owner_e2ee_data, rebind_local_identity_state, rebind_owner_did};
pub use schema::{current_schema_version, ensure_schema};
pub use types::{ImportReport, LegacyScan, StoreError, StoreResult, SCHEMA_VERSION};
