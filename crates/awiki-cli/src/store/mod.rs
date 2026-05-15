mod contacts;
mod groups;
mod helpers;
mod import;
mod messages;
mod open;
mod query;
mod rebind;
mod recover_merge;
mod schema;
mod types;

pub use contacts::{
    get_contact_by_did, get_current_contact_by_handle, list_dids_by_handle,
    resolve_contact_handle_by_did, upsert_contact, ContactRecord,
};
pub use groups::{
    get_group_snapshot, list_cached_group_members, list_group_messages, mark_group_left,
    replace_group_members, touch_group_after_message, upsert_group, upsert_group_member,
    GroupMemberRecord, GroupRecord,
};
pub use helpers::{make_thread_id, now_utc};
pub use import::{import_legacy_database, scan_legacy_database, LegacyOwnerLookup};
pub use messages::{
    list_direct_messages_by_peer_dids, list_inbox_messages, list_messages_by_ids,
    list_thread_messages, mark_messages_read, store_message, store_messages_batch, MessageRecord,
};
pub use open::{open, open_read_only};
pub use query::{execute_sql, list_notifications};
pub use rebind::{
    clear_owner_e2ee_data, rebind_local_identity_state, rebind_local_identity_state_with_partial,
    rebind_owner_did, RebindLocalIdentityStateError, RebindLocalIdentityStateOutcome,
};
pub use recover_merge::merge_recovered_handle_local_state;
pub use schema::{current_schema_version, ensure_schema};
pub use types::{ImportReport, LegacyScan, StoreError, StoreResult, SCHEMA_VERSION};
