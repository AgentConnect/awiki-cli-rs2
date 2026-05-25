mod backup;
mod detect;
mod fsutil;
mod journal;
pub(crate) mod legacy_identity;
pub(crate) mod legacy_sqlite;
mod lock;
mod meta;
mod migration_v0_to_v1;
mod migration_v1_to_v2;
mod migration_v2_to_v3;
mod settings;
mod types;
mod upgrader;

pub use backup::{backup_sqlite_database, create_backup, BackupError};
pub use detect::{detect, inspect, resolve_paths, InspectError};
pub use journal::{clear_journal, load_journal, save_journal, JournalError};
pub use lock::{acquire_file_lock, LockError, UpgradeLockGuard};
pub use meta::{load_meta, save_meta, MetaError};
pub use migration_v0_to_v1::{
    apply_workspace_v0_to_v1_config, apply_workspace_v0_to_v1_config_optional,
    apply_workspace_v0_to_v1_legacy_imports, apply_workspace_v0_to_v1_legacy_imports_optional,
    apply_workspace_v0_to_v1_local_state, apply_workspace_v0_to_v1_local_state_optional,
    ensure_target_store_schema, refresh_resolved_config, refresh_resolved_config_optional,
    validate_sqlite_health, RefreshResolvedConfigError, SQLiteHealthError,
};
pub use migration_v1_to_v2::{
    apply_workspace_v1_to_v2_cleanup, apply_workspace_v1_to_v2_cleanup_optional,
};
pub use settings::{
    load_legacy_settings, parse_legacy_settings, LegacySettingsError, NormalizedLegacySettings,
};
pub use types::{Detection, Inspection, Journal, Meta, Paths, LATEST_WORKSPACE_SCHEMA_VERSION};
pub use upgrader::{
    new_context, new_default_upgrader, upgrade_if_needed, Context, Migration, MigrationError,
    UpgradeError, Upgrader, UpgraderError,
};
