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
    pub alias_mappings: Vec<LocalStateConversationAliasMapping>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalStateConversationAliasMapping {
    pub owner_identity_id: String,
    pub owner_did: String,
    pub legacy_conversation_id: String,
    pub canonical_conversation_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalStateRestoreResult {
    pub restored_schema_version: i64,
    pub target_safety_copy_available: bool,
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
            alias_mappings: Vec::new(),
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
            alias_mappings: read_alias_mappings(&paths.sqlite_path)?,
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
            alias_mappings: read_alias_mappings(&paths.sqlite_path)?,
        }),
    }
}

/// Restores the verified release/0710 backup after a completed canonical cutover.
///
/// The current target database is retained as a private safety copy. Callers
/// must dispose Core before invoking this entry point and must not reopen the
/// target-version Core afterwards unless they intend to run the upgrade again.
pub fn restore_local_state_backup(paths: &LocalStatePaths) -> ImResult<LocalStateRestoreResult> {
    let upgrade_dir = upgrade_directory(paths)?;
    let report = crate::internal::local_state::canonical_upgrade::restore_verified_backup(
        &paths.sqlite_path,
        &upgrade_dir,
    )?;
    Ok(LocalStateRestoreResult {
        restored_schema_version: report.restored_schema_version,
        target_safety_copy_available: report.target_safety_copy.is_file(),
    })
}

fn read_alias_mappings(
    sqlite_path: &std::path::Path,
) -> ImResult<Vec<LocalStateConversationAliasMapping>> {
    crate::internal::local_state::canonical_upgrade::list_alias_mappings(sqlite_path).map(|items| {
        items
            .into_iter()
            .map(|item| LocalStateConversationAliasMapping {
                owner_identity_id: item.owner_identity_id,
                owner_did: item.owner_did,
                legacy_conversation_id: item.legacy_conversation_id,
                canonical_conversation_id: item.canonical_conversation_id,
            })
            .collect()
    })
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

    #[test]
    fn post_canonical_schema_migrations_remain_reachable_at_bootstrap() {
        for source_schema_version in [28, 29, 30] {
            let directory = tempfile::tempdir().unwrap();
            let paths = LocalStatePaths {
                sqlite_path: directory.path().join("im.sqlite"),
            };
            let connection =
                crate::internal::local_state::open_writable(&paths.sqlite_path).unwrap();
            connection
                .pragma_update(None, "user_version", source_schema_version)
                .unwrap();
            drop(connection);

            let inspection = inspect_local_state_upgrade(&paths).unwrap();
            assert_eq!(
                inspection.eligibility,
                LocalStateUpgradeEligibility::NotRequired
            );
            assert_eq!(inspection.source_schema_version, source_schema_version);
            assert_eq!(
                inspection.target_schema_version,
                crate::internal::local_state::schema::SCHEMA_VERSION
            );

            let result = upgrade_local_state(&paths).unwrap();
            assert_eq!(result.status, LocalStateUpgradeStatus::NotRequired);
            assert_eq!(result.source_schema_version, source_schema_version);

            let migrated = crate::internal::local_state::open_writable(&paths.sqlite_path).unwrap();
            assert_eq!(
                crate::internal::local_state::schema::current_schema_version(&migrated).unwrap(),
                crate::internal::local_state::schema::SCHEMA_VERSION
            );
        }
    }

    #[test]
    fn completed_target_returns_alias_mapping_for_overlay_resume() {
        let directory = tempfile::tempdir().unwrap();
        let paths = LocalStatePaths {
            sqlite_path: directory.path().join("im.sqlite"),
        };
        let connection = crate::internal::local_state::open_writable(&paths.sqlite_path).unwrap();
        crate::internal::local_state::conversation_registry::ensure(
            &connection,
            &crate::internal::local_state::conversation_registry::ConversationRegistryRecord {
                owner_identity_id: "owner-a".to_owned(),
                owner_did: "did:wba:example.com:alice".to_owned(),
                conversation_id: "group:did:wba:example.com:groups:canonical".to_owned(),
                thread_kind: "group".to_owned(),
                thread_id: "did:wba:example.com:groups:canonical".to_owned(),
                activity_at: "2026-07-14T00:00:00Z".to_owned(),
            },
        )
        .unwrap();
        crate::internal::local_state::conversation_aliases::insert(
            &connection,
            &crate::internal::local_state::conversation_aliases::ConversationAliasRecord {
                owner_identity_id: "owner-a".to_owned(),
                alias_kind: "legacy_conversation_id".to_owned(),
                alias_conversation_id: "group:legacy".to_owned(),
                canonical_conversation_id: "group:did:wba:example.com:groups:canonical".to_owned(),
                source: "test".to_owned(),
                verified_at: "2026-07-14T00:00:00Z".to_owned(),
            },
        )
        .unwrap();
        drop(connection);

        let result = upgrade_local_state(&paths).unwrap();
        assert_eq!(result.status, LocalStateUpgradeStatus::NotRequired);
        assert_eq!(
            result.alias_mappings,
            vec![LocalStateConversationAliasMapping {
                owner_identity_id: "owner-a".to_owned(),
                owner_did: "did:wba:example.com:alice".to_owned(),
                legacy_conversation_id: "group:legacy".to_owned(),
                canonical_conversation_id: "group:did:wba:example.com:groups:canonical".to_owned(),
            }]
        );
    }

    #[test]
    fn completed_upgrade_can_restore_verified_release_0710_backup() {
        let directory = tempfile::tempdir().unwrap();
        let paths = LocalStatePaths {
            sqlite_path: directory.path().join("im.sqlite"),
        };
        crate::internal::local_state::canonical_upgrade::copy_release_0710_fixture(
            &paths.sqlite_path,
        );
        let upgraded = upgrade_local_state(&paths).unwrap();
        assert_eq!(upgraded.status, LocalStateUpgradeStatus::Completed);

        let restored = restore_local_state_backup(&paths).unwrap();
        assert_eq!(restored.restored_schema_version, 27);
        assert!(restored.target_safety_copy_available);
        assert_eq!(
            rusqlite::Connection::open(&paths.sqlite_path)
                .unwrap()
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            27
        );

        let repeated = restore_local_state_backup(&paths).unwrap();
        assert_eq!(repeated, restored);
    }
}
