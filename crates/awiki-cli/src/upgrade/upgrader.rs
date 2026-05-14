use super::meta::{load_meta, MetaError};
use super::resolve_paths;
use super::types::{Inspection, Meta, Paths, LATEST_WORKSPACE_SCHEMA_VERSION};
use crate::config;
use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone)]
pub struct Context {
    pub resolved: config::Resolved,
    pub paths: Paths,
    pub app_version: String,
    pub inspection: Option<Inspection>,
    pub backup_dir: String,
    pub current_meta: Option<Meta>,
    pub warnings: Vec<String>,
}

pub fn new_context(resolved: &config::Resolved, app_version: &str) -> Context {
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
pub enum MigrationError {
    Meta(MetaError),
    ExecutionDeferred { name: &'static str },
}

impl fmt::Display for MigrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Meta(err) => write!(f, "{err}"),
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

pub struct Upgrader {
    latest_version: i64,
    migrations: BTreeMap<i64, Box<dyn Migration>>,
}

impl Upgrader {
    pub fn latest_version(&self) -> i64 {
        self.latest_version
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

pub fn new_default_upgrader() -> Upgrader {
    let migrations: Vec<Box<dyn Migration>> = vec![
        Box::new(workspace_v0_to_v1_migration()),
        Box::new(workspace_v1_to_v2_migration()),
        Box::new(workspace_v2_to_v3_migration()),
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
        name: "workspace_2_to_3_replace_existing_k1_handle_dids",
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

    fn apply(&self, _context: &mut Context) -> Result<(), MigrationError> {
        Err(MigrationError::ExecutionDeferred { name: self.name })
    }

    fn validate(&self, _context: &Context) -> Result<(), MigrationError> {
        Err(MigrationError::ExecutionDeferred { name: self.name })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_rejects_newer_source_version_like_go() {
        let upgrader = new_default_upgrader();
        let err = upgrader.plan(4, 3).expect_err("newer source should fail");
        assert_eq!(
            err.to_string(),
            "workspace schema version 4 is newer than target 3"
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
}
