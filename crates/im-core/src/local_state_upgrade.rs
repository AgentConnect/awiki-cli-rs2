//! Explicit local-state upgrade entry points for application bootstrap.
//!
//! Call these functions before opening [`crate::ImCore`]. Ordinary Core open
//! intentionally refuses release/0710 schema 27 so that backup and validation
//! cannot be bypassed.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{ImError, ImResult, LocalStatePaths};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalStateUpgradeEligibility {
    NotRequired,
    Required,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalStateUpgradeInspection {
    pub eligibility: LocalStateUpgradeEligibility,
    pub source_schema_version: i64,
    pub target_schema_version: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalStateUpgradeStatus {
    NotRequired,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalStateUpgradeResult {
    pub status: LocalStateUpgradeStatus,
    pub source_schema_version: i64,
    pub target_schema_version: i64,
    pub migrated_personas: u64,
    pub migrated_conversations: u64,
    pub unresolved_messages: u64,
    pub alias_count: u64,
    pub backup_available: bool,
}

/// Inspects whether the Storage Scope SQLite file needs the canonical upgrade.
///
/// A missing file is a fresh install and therefore needs no migration. Existing
/// files are opened read-only by the detector.
pub fn inspect_local_state_upgrade(
    paths: &LocalStatePaths,
) -> ImResult<LocalStateUpgradeInspection> {
    if !paths.sqlite_path.exists() {
        return Ok(LocalStateUpgradeInspection {
            eligibility: LocalStateUpgradeEligibility::NotRequired,
            source_schema_version: 0,
            target_schema_version: crate::internal::local_state::schema::SCHEMA_VERSION,
        });
    }
    let detection = crate::internal::local_state::canonical_upgrade::detect(&paths.sqlite_path)?;
    Ok(LocalStateUpgradeInspection {
        eligibility: match detection.eligibility {
            crate::internal::local_state::canonical_upgrade::CanonicalUpgradeEligibility::NotRequired => {
                LocalStateUpgradeEligibility::NotRequired
            }
            crate::internal::local_state::canonical_upgrade::CanonicalUpgradeEligibility::Eligible => {
                LocalStateUpgradeEligibility::Required
            }
        },
        source_schema_version: detection.source_schema_version,
        target_schema_version: detection.target_schema_version,
    })
}

/// Runs the complete backup, shadow migration, validation, and cutover flow.
///
/// This function performs blocking filesystem and SQLite work. UI runtimes
/// should invoke it on a blocking worker before opening Core.
pub fn upgrade_local_state(paths: &LocalStatePaths) -> ImResult<LocalStateUpgradeResult> {
    if !paths.sqlite_path.exists() {
        return Ok(LocalStateUpgradeResult {
            status: LocalStateUpgradeStatus::NotRequired,
            source_schema_version: 0,
            target_schema_version: crate::internal::local_state::schema::SCHEMA_VERSION,
            migrated_personas: 0,
            migrated_conversations: 0,
            unresolved_messages: 0,
            alias_count: 0,
            backup_available: false,
        });
    }
    let upgrade_dir = upgrade_directory(paths)?;
    match crate::internal::local_state::canonical_upgrade::run(&paths.sqlite_path, &upgrade_dir)? {
        crate::internal::local_state::canonical_upgrade::CanonicalUpgradeOutcome::NotRequired(
            detection,
        ) => Ok(LocalStateUpgradeResult {
            status: LocalStateUpgradeStatus::NotRequired,
            source_schema_version: detection.source_schema_version,
            target_schema_version: detection.target_schema_version,
            migrated_personas: 0,
            migrated_conversations: 0,
            unresolved_messages: 0,
            alias_count: 0,
            backup_available: completed_backup_exists(&upgrade_dir),
        }),
        crate::internal::local_state::canonical_upgrade::CanonicalUpgradeOutcome::Completed(
            report,
        ) => Ok(LocalStateUpgradeResult {
            status: LocalStateUpgradeStatus::Completed,
            source_schema_version: report.source_schema_version,
            target_schema_version: report.target_schema_version,
            migrated_personas: report.migrated_personas,
            migrated_conversations: report.migrated_conversations,
            unresolved_messages: report.unresolved_messages,
            alias_count: report.alias_count,
            backup_available: report.backup_path.exists(),
        }),
    }
}

fn upgrade_directory(paths: &LocalStatePaths) -> ImResult<PathBuf> {
    let parent = paths
        .sqlite_path
        .parent()
        .ok_or_else(|| ImError::PathUnavailable {
            path_kind: "local_state_upgrade".to_owned(),
            detail: "SQLite path has no parent directory".to_owned(),
        })?;
    Ok(parent.join("canonical-conversation-upgrade"))
}

fn completed_backup_exists(upgrade_dir: &std::path::Path) -> bool {
    crate::internal::local_state::canonical_upgrade::load_journal(upgrade_dir)
        .ok()
        .flatten()
        .is_some_and(|journal| upgrade_dir.join(journal.backup_file).is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_database_is_fresh_and_does_not_create_files() {
        let directory = tempfile::tempdir().unwrap();
        let paths = LocalStatePaths {
            sqlite_path: directory.path().join("im.sqlite"),
        };

        let inspection = inspect_local_state_upgrade(&paths).unwrap();
        assert_eq!(
            inspection.eligibility,
            LocalStateUpgradeEligibility::NotRequired
        );
        assert_eq!(inspection.source_schema_version, 0);

        let result = upgrade_local_state(&paths).unwrap();
        assert_eq!(result.status, LocalStateUpgradeStatus::NotRequired);
        assert!(!result.backup_available);
        assert!(!paths.sqlite_path.exists());
    }
}
