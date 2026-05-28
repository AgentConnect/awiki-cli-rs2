use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub struct CoreBootstrap<'a> {
    core: &'a super::ImCore,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathValidationReport {
    pub checked: Vec<PathCheck>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathCheck {
    pub kind: String,
    pub path: String,
    pub exists: bool,
    pub readable: bool,
    pub writable: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalStateStatus {
    pub sqlite_path: String,
    pub initialized: bool,
    pub schema_version: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationReport {
    pub sqlite_path: String,
    pub from_version: Option<u32>,
    pub to_version: u32,
    pub applied: Vec<String>,
}

impl<'a> CoreBootstrap<'a> {
    pub(crate) fn new(core: &'a super::ImCore) -> Self {
        Self { core }
    }

    pub fn validate_paths(&self) -> crate::ImResult<PathValidationReport> {
        let sdk_paths = self.core.inner().sdk_paths();
        let checked = vec![
            check_path(
                "identity_root_dir",
                &sdk_paths.identities.identity_root_dir,
                Some(true),
            ),
            check_path("registry_path", &sdk_paths.identities.registry_path, None),
            check_optional_path(
                "default_identity_path",
                sdk_paths.identities.default_identity_path.as_deref(),
            ),
            check_path("sqlite_path", &sdk_paths.local_state.sqlite_path, None),
            check_path("cache_dir", &sdk_paths.runtime.cache_dir, Some(true)),
            check_path("temp_dir", &sdk_paths.runtime.temp_dir, Some(true)),
        ];
        Ok(PathValidationReport {
            checked,
            warnings: Vec::new(),
        })
    }

    pub async fn validate_paths_async(&self) -> crate::ImResult<PathValidationReport> {
        let sdk_paths = self.core.inner().sdk_paths();
        let checked = vec![
            check_path_async(
                "identity_root_dir",
                sdk_paths.identities.identity_root_dir.clone(),
                Some(true),
            )
            .await,
            check_path_async(
                "registry_path",
                sdk_paths.identities.registry_path.clone(),
                None,
            )
            .await,
            check_optional_path_async(
                "default_identity_path",
                sdk_paths.identities.default_identity_path.clone(),
            )
            .await,
            check_path_async(
                "sqlite_path",
                sdk_paths.local_state.sqlite_path.clone(),
                None,
            )
            .await,
            check_path_async("cache_dir", sdk_paths.runtime.cache_dir.clone(), Some(true)).await,
            check_path_async("temp_dir", sdk_paths.runtime.temp_dir.clone(), Some(true)).await,
        ];
        Ok(PathValidationReport {
            checked,
            warnings: Vec::new(),
        })
    }

    pub fn initialize_local_state(&self) -> crate::ImResult<LocalStateStatus> {
        let sqlite_path = &self.core.inner().sdk_paths().local_state.sqlite_path;
        if let Some(parent) = sqlite_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let schema_version = initialize_local_state_schema(sqlite_path)?;
        Ok(LocalStateStatus {
            sqlite_path: sqlite_path.display().to_string(),
            initialized: true,
            schema_version,
        })
    }

    pub async fn initialize_local_state_async(&self) -> crate::ImResult<LocalStateStatus> {
        let sqlite_path = self
            .core
            .inner()
            .sdk_paths()
            .local_state
            .sqlite_path
            .clone();
        if let Some(parent) = sqlite_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let schema_version = initialize_local_state_schema_async(self.core).await?;
        Ok(LocalStateStatus {
            sqlite_path: sqlite_path.display().to_string(),
            initialized: true,
            schema_version,
        })
    }

    pub fn migrate_local_state(&self) -> crate::ImResult<MigrationReport> {
        let status = self.initialize_local_state()?;
        #[cfg(feature = "sqlite")]
        let applied = {
            let identities = self
                .core
                .identities()
                .list()?
                .into_iter()
                .map(|identity| {
                    let mut credential_names = Vec::new();
                    credential_names.push(identity.id.as_str().to_string());
                    if let Some(alias) = identity.local_alias.as_deref() {
                        if !credential_names.iter().any(|known| known == alias) {
                            credential_names.push(alias.to_string());
                        }
                    }
                    crate::internal::local_state::schema::OwnerIdentityBackfill {
                        identity_id: identity.id.as_str().to_string(),
                        owner_did: identity.did.as_str().to_string(),
                        credential_names,
                    }
                })
                .collect::<Vec<_>>();
            let connection = crate::internal::local_state::open_writable(
                &self.core.inner().sdk_paths().local_state.sqlite_path,
            )?;
            let updated = crate::internal::local_state::schema::backfill_owner_identity_ids(
                &connection,
                &identities,
            )?;
            if updated == 0 {
                Vec::new()
            } else {
                vec![format!("owner_identity_id_backfill:{updated}")]
            }
        };
        #[cfg(not(feature = "sqlite"))]
        let applied = Vec::new();
        Ok(MigrationReport {
            sqlite_path: status.sqlite_path,
            from_version: status.schema_version,
            to_version: status.schema_version.unwrap_or_default(),
            applied,
        })
    }

    pub async fn migrate_local_state_async(&self) -> crate::ImResult<MigrationReport> {
        let status = self.initialize_local_state_async().await?;
        #[cfg(feature = "sqlite")]
        let applied = {
            let identities = self
                .core
                .identities()
                .list_async()
                .await?
                .into_iter()
                .map(|identity| {
                    let mut credential_names = Vec::new();
                    credential_names.push(identity.id.as_str().to_string());
                    if let Some(alias) = identity.local_alias.as_deref() {
                        if !credential_names.iter().any(|known| known == alias) {
                            credential_names.push(alias.to_string());
                        }
                    }
                    crate::internal::local_state::schema::OwnerIdentityBackfill {
                        identity_id: identity.id.as_str().to_string(),
                        owner_did: identity.did.as_str().to_string(),
                        credential_names,
                    }
                })
                .collect::<Vec<_>>();
            let db = self.core.inner().local_state_db().await?;
            let updated = db.backfill_owner_identity_ids(identities).await?;
            if updated == 0 {
                Vec::new()
            } else {
                vec![format!("owner_identity_id_backfill:{updated}")]
            }
        };
        #[cfg(not(feature = "sqlite"))]
        let applied = Vec::new();
        Ok(MigrationReport {
            sqlite_path: status.sqlite_path,
            from_version: status.schema_version,
            to_version: status.schema_version.unwrap_or_default(),
            applied,
        })
    }
}

fn check_optional_path(kind: &str, path: Option<&Path>) -> PathCheck {
    match path {
        Some(path) => check_path(kind, path, None),
        None => PathCheck {
            kind: kind.to_string(),
            path: String::new(),
            exists: false,
            readable: false,
            writable: None,
        },
    }
}

fn check_path(kind: &str, path: &Path, writable: Option<bool>) -> PathCheck {
    let exists = path.exists();
    let readable = fs::metadata(path).is_ok();
    PathCheck {
        kind: kind.to_string(),
        path: path.display().to_string(),
        exists,
        readable,
        writable,
    }
}

async fn check_optional_path_async(kind: &str, path: Option<std::path::PathBuf>) -> PathCheck {
    match path {
        Some(path) => check_path_async(kind, path, None).await,
        None => PathCheck {
            kind: kind.to_string(),
            path: String::new(),
            exists: false,
            readable: false,
            writable: None,
        },
    }
}

async fn check_path_async(
    kind: &str,
    path: std::path::PathBuf,
    writable: Option<bool>,
) -> PathCheck {
    let exists = tokio::fs::try_exists(&path).await.unwrap_or(false);
    let readable = tokio::fs::metadata(&path).await.is_ok();
    PathCheck {
        kind: kind.to_string(),
        path: path.display().to_string(),
        exists,
        readable,
        writable,
    }
}

#[cfg(feature = "sqlite")]
fn initialize_local_state_schema(sqlite_path: &Path) -> crate::ImResult<Option<u32>> {
    let connection = crate::internal::local_state::open_writable(sqlite_path)?;
    let schema_version = crate::internal::local_state::schema::current_schema_version(&connection)?;
    Ok(Some(schema_version as u32))
}

#[cfg(feature = "sqlite")]
async fn initialize_local_state_schema_async(core: &super::ImCore) -> crate::ImResult<Option<u32>> {
    let db = core.inner().local_state_db().await?;
    let schema_version = db.current_schema_version().await?;
    Ok(Some(schema_version as u32))
}

#[cfg(not(feature = "sqlite"))]
fn initialize_local_state_schema(_sqlite_path: &Path) -> crate::ImResult<Option<u32>> {
    Ok(None)
}

#[cfg(not(feature = "sqlite"))]
async fn initialize_local_state_schema_async(
    _core: &super::ImCore,
) -> crate::ImResult<Option<u32>> {
    Ok(None)
}
