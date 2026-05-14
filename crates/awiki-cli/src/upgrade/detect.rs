use super::journal::{load_journal, JournalError};
use super::meta::{load_meta, MetaError};
use super::types::{Detection, Inspection, Meta, Paths, LATEST_WORKSPACE_SCHEMA_VERSION};
use crate::config::Resolved;
use crate::identity::{self, Manager};
use crate::store;
use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum InspectError {
    Meta(MetaError),
    Journal(JournalError),
}

impl fmt::Display for InspectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Meta(err) => write!(f, "{err}"),
            Self::Journal(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for InspectError {}

impl From<MetaError> for InspectError {
    fn from(value: MetaError) -> Self {
        Self::Meta(value)
    }
}

impl From<JournalError> for InspectError {
    fn from(value: JournalError) -> Self {
        Self::Journal(value)
    }
}

pub fn inspect(resolved: &Resolved, _app_version: &str) -> Result<Inspection, InspectError> {
    let paths = resolve_paths(resolved);
    let meta = load_meta(&paths.meta_path)?;
    let journal = load_journal(&paths.journal_path)?;
    let detection = detect(resolved, meta.as_ref());
    Ok(Inspection {
        paths,
        meta,
        journal,
        detection,
    })
}

pub fn resolve_paths(resolved: &Resolved) -> Paths {
    let workspace_home_dir = if resolved.paths.workspace_home_dir.trim().is_empty() {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join(".awiki-cli"))
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default()
    } else {
        resolved.paths.workspace_home_dir.clone()
    };
    let upgrade_dir = Path::new(&workspace_home_dir).join("upgrade");
    Paths {
        config_file: resolved.paths.config_file.clone(),
        legacy_config_file: Path::new(&resolved.paths.config_dir)
            .join("config.json")
            .to_string_lossy()
            .into_owned(),
        identity_dir: resolved.paths.identity_dir.clone(),
        database_file: resolved.paths.database_file.clone(),
        legacy_credentials_dir: resolved.paths.legacy_credentials_dir.clone(),
        legacy_data_dir: resolved.paths.legacy_data_dir.clone(),
        legacy_settings_path: Path::new(&resolved.paths.legacy_data_dir)
            .join("config")
            .join("settings.json")
            .to_string_lossy()
            .into_owned(),
        meta_path: upgrade_dir.join("meta.json").to_string_lossy().into_owned(),
        journal_path: upgrade_dir
            .join("upgrade_journal.json")
            .to_string_lossy()
            .into_owned(),
        lock_path: upgrade_dir
            .join("upgrade.lock")
            .to_string_lossy()
            .into_owned(),
        backup_root: upgrade_dir.join("backups").to_string_lossy().into_owned(),
    }
}

pub fn detect(resolved: &Resolved, meta: Option<&Meta>) -> Detection {
    let paths = resolve_paths(resolved);
    let mut detection = Detection {
        latest_version: LATEST_WORKSPACE_SCHEMA_VERSION,
        ..Detection::default()
    };

    detection.config_exists = super::fsutil::file_exists(&paths.config_file);
    if detection.config_exists {
        detection.config_schema_version = resolved.config_schema_version;
        if !resolved.config_error.trim().is_empty() {
            detection.config_error = resolved.config_error.clone();
        }
    }
    detection.legacy_config_exists = super::fsutil::file_exists(&paths.legacy_config_file);

    let identity_index_path = Path::new(&paths.identity_dir).join(identity::types::INDEX_FILE_NAME);
    detection.identity_index_exists = identity_index_path.is_file();
    if detection.identity_index_exists {
        let manager = Manager::new(resolved.paths.clone());
        match manager.load_index() {
            Ok(index) => detection.identity_index_schema_version = index.schema_version,
            Err(err) => detection.identity_index_error = err.to_string(),
        }
    }

    detection.database_exists = super::fsutil::file_exists(&paths.database_file);
    if detection.database_exists {
        match store::open_read_only(&paths.database_file) {
            Ok(db) => match store::current_schema_version(&db) {
                Ok(version) => detection.database_schema_version = version,
                Err(err) => detection.database_error = err.to_string(),
            },
            Err(err) => detection.database_error = err.to_string(),
        }
    }

    let manager = Manager::new(resolved.paths.clone());
    match manager.scan_legacy() {
        Ok(scan) => detection.legacy_identity_exists = scan.has_legacy,
        Err(err) => detection.legacy_identity_error = err.to_string(),
    }

    match store::scan_legacy_database(&resolved.paths) {
        Ok(scan) => detection.legacy_database_exists = scan.exists,
        Err(err) => detection.legacy_database_error = err.to_string(),
    }

    detection.legacy_settings_exists = super::fsutil::file_exists(&paths.legacy_settings_path);
    detection.has_workspace = detection.config_exists
        || detection.legacy_config_exists
        || detection.identity_index_exists
        || detection.database_exists;
    detection.has_legacy = detection.legacy_identity_exists
        || detection.legacy_database_exists
        || detection.legacy_settings_exists;
    detection.empty = meta.is_none() && !detection.has_workspace && !detection.has_legacy;

    if let Some(meta) = meta {
        detection.current_version = meta.workspace_schema_version;
        detection.current_version_source = "meta".to_string();
    } else if detection.empty {
        detection.current_version = LATEST_WORKSPACE_SCHEMA_VERSION;
        detection.current_version_source = "default_empty".to_string();
    } else {
        detection.current_version = 0;
        detection.current_version_source = "legacy_detector".to_string();
    }

    detection
}
