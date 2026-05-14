mod backup;
mod detect;
mod fsutil;
mod journal;
mod lock;
mod meta;
mod settings;
mod types;
mod upgrader;

pub use backup::{backup_sqlite_database, create_backup, BackupError};
pub use detect::{detect, inspect, resolve_paths, InspectError};
pub use journal::{clear_journal, load_journal, save_journal, JournalError};
pub use lock::{acquire_file_lock, LockError, UpgradeLockGuard};
pub use meta::{load_meta, save_meta, MetaError};
pub use settings::{
    load_legacy_settings, parse_legacy_settings, LegacySettingsError, NormalizedLegacySettings,
};
pub use types::{Detection, Inspection, Journal, Meta, Paths, LATEST_WORKSPACE_SCHEMA_VERSION};
pub use upgrader::{
    new_context, new_default_upgrader, Context, Migration, MigrationError, Upgrader, UpgraderError,
};
