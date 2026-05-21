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

    pub fn migrate_local_state(&self) -> crate::ImResult<MigrationReport> {
        let status = self.initialize_local_state()?;
        Ok(MigrationReport {
            sqlite_path: status.sqlite_path,
            from_version: status.schema_version,
            to_version: status.schema_version.unwrap_or_default(),
            applied: Vec::new(),
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

#[cfg(feature = "sqlite")]
fn initialize_local_state_schema(sqlite_path: &Path) -> crate::ImResult<Option<u32>> {
    let connection = crate::internal::local_state::open_writable(sqlite_path)?;
    let schema_version = crate::internal::local_state::schema::current_schema_version(&connection)?;
    Ok(Some(schema_version as u32))
}

#[cfg(not(feature = "sqlite"))]
fn initialize_local_state_schema(_sqlite_path: &Path) -> crate::ImResult<Option<u32>> {
    Ok(None)
}
