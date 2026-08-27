use super::backup::{create_backup, BackupError};
use super::detect::{inspect, InspectError};
use super::journal::{clear_journal, load_journal, save_journal, JournalError};
use super::legacy_identity as identity;
use super::legacy_sqlite as store;
use super::lock::{acquire_file_lock, LockError};
use super::meta::{load_meta, save_meta, MetaError};
use super::migration_v0_to_v1;
use super::migration_v1_to_v2;
use super::migration_v3_to_v4;
use super::resolve_paths;
use super::types::{Inspection, Meta, Paths, LATEST_WORKSPACE_SCHEMA_VERSION};
use crate::workspace_config;
use std::collections::BTreeMap;
use std::fmt;
use time::{OffsetDateTime, UtcOffset};

#[derive(Debug, Clone)]
pub struct Context {
    pub resolved: workspace_config::Resolved,
    pub paths: Paths,
    pub app_version: String,
    pub inspection: Option<Inspection>,
    pub backup_dir: String,
    pub current_meta: Option<Meta>,
    pub warnings: Vec<String>,
}

pub fn new_context(resolved: &workspace_config::Resolved, app_version: &str) -> Context {
    Context {
        resolved: resolved.clone(),
        paths: resolve_paths(resolved),
        app_version: app_version.to_string(),
        inspection: None,
        backup_dir: String::new(),
        current_meta: None,
        warnings: Vec::new(),
    }
}

pub fn upgrade_if_needed(
    resolved: &workspace_config::Resolved,
    app_version: &str,
) -> Result<(), UpgradeError> {
    let mut context = new_context(resolved, app_version);
    new_default_upgrader().upgrade_if_needed(&mut context)
}

pub trait Migration: fmt::Debug {
    fn from(&self) -> i64;
    fn to(&self) -> i64;
    fn name(&self) -> &'static str;
    fn is_done(&self, context: &Context) -> Result<bool, MigrationError>;
    fn apply(&self, context: &mut Context) -> Result<(), MigrationError>;
    fn validate(&self, context: &Context) -> Result<(), MigrationError>;
}

#[derive(Debug)]
pub enum UpgraderError {
    NewerThanTarget {
        from_version: i64,
        to_version: i64,
    },
    MissingMigration {
        from_version: i64,
        to_version: i64,
    },
    UnexpectedTarget {
        from_version: i64,
        actual_target: i64,
    },
}

impl fmt::Display for UpgraderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NewerThanTarget {
                from_version,
                to_version,
            } => write!(
                f,
                "workspace schema version {from_version} is newer than target {to_version}"
            ),
            Self::MissingMigration {
                from_version,
                to_version,
            } => write!(
                f,
                "missing workspace migration {from_version} -> {to_version}"
            ),
            Self::UnexpectedTarget {
                from_version,
                actual_target,
            } => write!(
                f,
                "workspace migration {from_version} has unexpected target {actual_target}"
            ),
        }
    }
}

impl std::error::Error for UpgraderError {}

#[derive(Debug)]
pub enum UpgradeError {
    ContextRequired,
    Inspect(InspectError),
    Backup(BackupError),
    Journal(JournalError),
    Lock(LockError),
    Meta(MetaError),
    Migration(MigrationError),
    Plan(UpgraderError),
    NewerThanSupported {
        current_version: i64,
        latest_version: i64,
    },
    ExecutionDeferred {
        current_version: i64,
        latest_version: i64,
    },
}

impl fmt::Display for UpgradeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ContextRequired => f.write_str("upgrade context is required"),
            Self::Inspect(err) => write!(f, "{err}"),
            Self::Backup(err) => write!(f, "{err}"),
            Self::Journal(err) => write!(f, "{err}"),
            Self::Lock(err) => write!(f, "{err}"),
            Self::Meta(err) => write!(f, "{err}"),
            Self::Migration(err) => write!(f, "{err}"),
            Self::Plan(err) => write!(f, "{err}"),
            Self::NewerThanSupported {
                current_version,
                latest_version,
            } => write!(
                f,
                "workspace schema version {current_version} is newer than supported {latest_version}"
            ),
            Self::ExecutionDeferred {
                current_version,
                latest_version,
            } => write!(
                f,
                "workspace migration execution is not implemented from {current_version} to {latest_version}"
            ),
        }
    }
}

impl std::error::Error for UpgradeError {}

impl From<InspectError> for UpgradeError {
    fn from(value: InspectError) -> Self {
        Self::Inspect(value)
    }
}

impl From<JournalError> for UpgradeError {
    fn from(value: JournalError) -> Self {
        Self::Journal(value)
    }
}

impl From<BackupError> for UpgradeError {
    fn from(value: BackupError) -> Self {
        Self::Backup(value)
    }
}

impl From<LockError> for UpgradeError {
    fn from(value: LockError) -> Self {
        Self::Lock(value)
    }
}

impl From<MetaError> for UpgradeError {
    fn from(value: MetaError) -> Self {
        Self::Meta(value)
    }
}

impl From<MigrationError> for UpgradeError {
    fn from(value: MigrationError) -> Self {
        Self::Migration(value)
    }
}

impl From<UpgraderError> for UpgradeError {
    fn from(value: UpgraderError) -> Self {
        Self::Plan(value)
    }
}

#[derive(Debug)]
pub enum MigrationError {
    Meta(MetaError),
    Store(store::StoreError),
    SQLiteHealth(migration_v0_to_v1::SQLiteHealthError),
    Identity(identity::IdentityError),
    Message(String),
    ExecutionDeferred { name: &'static str },
}

impl fmt::Display for MigrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Meta(err) => write!(f, "{err}"),
            Self::Store(err) => write!(f, "{err}"),
            Self::SQLiteHealth(err) => write!(f, "{err}"),
            Self::Identity(err) => write!(f, "{err}"),
            Self::Message(message) => f.write_str(message),
            Self::ExecutionDeferred { name } => {
                write!(
                    f,
                    "workspace migration execution is not implemented: {name}"
                )
            }
        }
    }
}

impl std::error::Error for MigrationError {}

impl From<MetaError> for MigrationError {
    fn from(value: MetaError) -> Self {
        Self::Meta(value)
    }
}

impl From<store::StoreError> for MigrationError {
    fn from(value: store::StoreError) -> Self {
        Self::Store(value)
    }
}

impl From<migration_v0_to_v1::SQLiteHealthError> for MigrationError {
    fn from(value: migration_v0_to_v1::SQLiteHealthError) -> Self {
        Self::SQLiteHealth(value)
    }
}

impl From<identity::IdentityError> for MigrationError {
    fn from(value: identity::IdentityError) -> Self {
        Self::Identity(value)
    }
}

pub struct Upgrader {
    latest_version: i64,
    migrations: BTreeMap<i64, Box<dyn Migration>>,
}

impl Upgrader {
    pub fn latest_version(&self) -> i64 {
        self.latest_version
    }

    pub fn upgrade_context_if_needed(
        &self,
        context: Option<&mut Context>,
    ) -> Result<(), UpgradeError> {
        let context = context.ok_or(UpgradeError::ContextRequired)?;
        self.upgrade_if_needed(context)
    }

    pub fn upgrade_if_needed(&self, context: &mut Context) -> Result<(), UpgradeError> {
        let inspection = inspect(&context.resolved, &context.app_version)?;
        let (current_version, empty, has_journal) = capture_inspection(context, inspection);

        if current_version > self.latest_version {
            return Err(UpgradeError::NewerThanSupported {
                current_version,
                latest_version: self.latest_version,
            });
        }

        if empty || current_version == self.latest_version {
            if has_journal {
                clear_journal(&context.paths.journal_path)?;
            }
            return Ok(());
        }

        let _lock = acquire_file_lock(&context.paths.lock_path, &context.app_version)?;

        let inspection = inspect(&context.resolved, &context.app_version)?;
        let (current_version, empty, has_journal) = capture_inspection(context, inspection);

        if empty || current_version == self.latest_version {
            if has_journal {
                clear_journal(&context.paths.journal_path)?;
            }
            return Ok(());
        }

        let journal = load_journal(&context.paths.journal_path)?;
        let mut backup_dir = journal
            .as_ref()
            .map(|journal| journal.backup_dir.clone())
            .unwrap_or_default();
        if backup_dir.is_empty() {
            backup_dir = create_backup(&context.paths, "")?;
        }
        context.backup_dir = backup_dir;

        let plan = self.plan(current_version, self.latest_version)?;
        if plan.is_empty() {
            clear_journal(&context.paths.journal_path)?;
            return Ok(());
        }

        let upgrade_id = journal
            .as_ref()
            .filter(|journal| !journal.upgrade_id.is_empty())
            .map(|journal| journal.upgrade_id.clone())
            .unwrap_or_else(|| format_time_layout(now_utc()));
        for migration in plan {
            let mut journal = super::types::Journal {
                upgrade_id: upgrade_id.clone(),
                from_version: migration.from(),
                to_version: migration.to(),
                current_step: migration.name().to_string(),
                phase: "checking".to_string(),
                backup_dir: context.backup_dir.clone(),
                started_at: format_rfc3339_seconds(now_utc()),
                app_version: context.app_version.clone(),
            };
            save_journal(&context.paths.journal_path, &journal)?;

            let done = migration.is_done(context)?;
            if !done {
                journal.phase = "applying".to_string();
                save_journal(&context.paths.journal_path, &journal)?;
                migration.apply(context)?;
            }

            journal.phase = "validating".to_string();
            save_journal(&context.paths.journal_path, &journal)?;
            migration.validate(context)?;

            let meta = Meta {
                workspace_schema_version: migration.to(),
                app_version: context.app_version.clone(),
                updated_at: format_rfc3339_seconds(now_utc()),
                last_upgrade_id: upgrade_id.clone(),
                last_backup_dir: context.backup_dir.clone(),
                warnings: context.warnings.clone(),
            };
            save_meta(&context.paths.meta_path, &meta)?;
            context.current_meta = Some(meta);
        }

        clear_journal(&context.paths.journal_path)?;
        Ok(())
    }

    pub fn plan(
        &self,
        from_version: i64,
        to_version: i64,
    ) -> Result<Vec<&dyn Migration>, UpgraderError> {
        if from_version > to_version {
            return Err(UpgraderError::NewerThanTarget {
                from_version,
                to_version,
            });
        }
        let mut plan = Vec::with_capacity((to_version - from_version).max(0) as usize);
        for version in from_version..to_version {
            let migration =
                self.migrations
                    .get(&version)
                    .ok_or(UpgraderError::MissingMigration {
                        from_version: version,
                        to_version: version + 1,
                    })?;
            if migration.to() != version + 1 {
                return Err(UpgraderError::UnexpectedTarget {
                    from_version: version,
                    actual_target: migration.to(),
                });
            }
            plan.push(migration.as_ref());
        }
        Ok(plan)
    }
}

fn capture_inspection(context: &mut Context, inspection: Inspection) -> (i64, bool, bool) {
    let current_version = inspection.detection.current_version;
    let empty = inspection.detection.empty;
    let has_journal = inspection.journal.is_some();
    context.current_meta = inspection.meta.clone();
    context.inspection = Some(inspection);
    (current_version, empty, has_journal)
}

fn now_utc() -> OffsetDateTime {
    OffsetDateTime::now_utc().to_offset(UtcOffset::UTC)
}

fn format_time_layout(value: OffsetDateTime) -> String {
    let value = value.to_offset(UtcOffset::UTC);
    let month: u8 = value.month().into();
    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        value.year(),
        month,
        value.day(),
        value.hour(),
        value.minute(),
        value.second()
    )
}

fn format_rfc3339_seconds(value: OffsetDateTime) -> String {
    let value = value.to_offset(UtcOffset::UTC);
    let month: u8 = value.month().into();
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        value.year(),
        month,
        value.day(),
        value.hour(),
        value.minute(),
        value.second()
    )
}

pub fn new_default_upgrader() -> Upgrader {
    let migrations: Vec<Box<dyn Migration>> = vec![
        Box::new(workspace_v0_to_v1_migration()),
        Box::new(workspace_v1_to_v2_migration()),
        Box::new(workspace_v2_to_v3_migration()),
        Box::new(workspace_v3_to_v4_migration()),
    ];
    Upgrader {
        latest_version: LATEST_WORKSPACE_SCHEMA_VERSION,
        migrations: migrations
            .into_iter()
            .map(|migration| (migration.from(), migration))
            .collect(),
    }
}

#[derive(Debug, Clone, Copy)]
struct WorkspaceMigration {
    from: i64,
    to: i64,
    name: &'static str,
}

fn workspace_v0_to_v1_migration() -> WorkspaceMigration {
    WorkspaceMigration {
        from: 0,
        to: 1,
        name: "workspace_0_to_1_bootstrap_local_state_upgrade",
    }
}

fn workspace_v1_to_v2_migration() -> WorkspaceMigration {
    WorkspaceMigration {
        from: 1,
        to: 2,
        name: "workspace_1_to_2_remove_legacy_skill_and_listener",
    }
}

fn workspace_v2_to_v3_migration() -> WorkspaceMigration {
    WorkspaceMigration {
        from: 2,
        to: 3,
        name: "workspace_2_to_3_retire_k1_online_replacement",
    }
}

fn workspace_v3_to_v4_migration() -> WorkspaceMigration {
    WorkspaceMigration {
        from: 3,
        to: 4,
        name: "workspace_3_to_4_owner_identity_local_state",
    }
}

impl Migration for WorkspaceMigration {
    fn from(&self) -> i64 {
        self.from
    }

    fn to(&self) -> i64 {
        self.to
    }

    fn name(&self) -> &'static str {
        self.name
    }

    fn is_done(&self, context: &Context) -> Result<bool, MigrationError> {
        let meta = load_meta(&context.paths.meta_path)?;
        Ok(meta
            .map(|meta| meta.workspace_schema_version >= self.to)
            .unwrap_or(false))
    }

    fn apply(&self, context: &mut Context) -> Result<(), MigrationError> {
        if self.from == 0 && self.to == 1 {
            migration_v0_to_v1::apply_workspace_v0_to_v1_config(context)?;
            let refreshed = migration_v0_to_v1::refresh_resolved_config(&context.resolved)
                .map_err(|err| MigrationError::Message(err.to_string()))?;
            context.resolved = refreshed;
            context.paths = resolve_paths(&context.resolved);
            let imported = migration_v0_to_v1::import_legacy_identities(context)?;
            let imported_any = !imported.imported.is_empty();
            let historical_owner_dids = imported
                .imported
                .iter()
                .map(|summary| (summary.identity_name.clone(), summary.did.clone()))
                .collect::<Vec<_>>();
            migration_v0_to_v1::import_legacy_sqlite_with_historical_dids(
                context,
                historical_owner_dids,
            )?;
            if std::path::Path::new(&context.paths.database_file).is_file() {
                migration_v0_to_v1::ensure_target_store_schema(&context.resolved.paths)?;
            }
            if imported_any {
                let refreshed = migration_v0_to_v1::refresh_resolved_config(&context.resolved)
                    .map_err(|err| MigrationError::Message(err.to_string()))?;
                context.resolved = refreshed;
                context.paths = resolve_paths(&context.resolved);
            }
            return Ok(());
        }
        if self.from == 1 && self.to == 2 {
            return migration_v1_to_v2::apply_workspace_v1_to_v2_cleanup(context);
        }
        if self.from == 2 && self.to == 3 {
            return Ok(());
        }
        if self.from == 3 && self.to == 4 {
            return migration_v3_to_v4::apply_workspace_v3_to_v4_owner_identity_local_state(
                context,
            );
        }
        Err(MigrationError::ExecutionDeferred { name: self.name })
    }

    fn validate(&self, context: &Context) -> Result<(), MigrationError> {
        if self.from == 0 && self.to == 1 {
            return migration_v0_to_v1::validate_workspace_v0_to_v1(context);
        }
        if self.from == 1 && self.to == 2 {
            return Ok(());
        }
        if self.from == 2 && self.to == 3 {
            return Ok(());
        }
        if self.from == 3 && self.to == 4 {
            return migration_v3_to_v4::validate_workspace_v3_to_v4_owner_identity_local_state(
                context,
            );
        }
        Err(MigrationError::ExecutionDeferred { name: self.name })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_rejects_newer_source_version_like_go() {
        let upgrader = new_default_upgrader();
        let err = upgrader.plan(5, 4).expect_err("newer source should fail");
        assert_eq!(
            err.to_string(),
            "workspace schema version 5 is newer than target 4"
        );
    }

    #[test]
    fn plan_reports_missing_migration_like_go() {
        let upgrader = Upgrader {
            latest_version: 3,
            migrations: BTreeMap::new(),
        };
        let err = upgrader
            .plan(0, 1)
            .expect_err("missing migration should fail");
        assert_eq!(err.to_string(), "missing workspace migration 0 -> 1");
    }

    #[test]
    fn plan_reports_unexpected_target_like_go() {
        let mut migrations: BTreeMap<i64, Box<dyn Migration>> = BTreeMap::new();
        migrations.insert(
            0,
            Box::new(WorkspaceMigration {
                from: 0,
                to: 2,
                name: "bad",
            }),
        );
        let upgrader = Upgrader {
            latest_version: 2,
            migrations,
        };
        let err = upgrader
            .plan(0, 1)
            .expect_err("unexpected target should fail");
        assert_eq!(
            err.to_string(),
            "workspace migration 0 has unexpected target 2"
        );
    }

    #[test]
    fn upgrade_context_if_needed_requires_context_like_go() {
        let upgrader = new_default_upgrader();
        let err = upgrader
            .upgrade_context_if_needed(None)
            .expect_err("missing context should fail");
        assert_eq!(err.to_string(), "upgrade context is required");
    }
}
