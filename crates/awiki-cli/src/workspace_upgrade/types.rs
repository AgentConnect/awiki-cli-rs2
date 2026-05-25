use serde::{Deserialize, Serialize};

pub const LATEST_WORKSPACE_SCHEMA_VERSION: i64 = 3;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Paths {
    pub config_file: String,
    pub legacy_config_file: String,
    pub identity_dir: String,
    pub database_file: String,
    pub legacy_credentials_dir: String,
    pub legacy_data_dir: String,
    pub legacy_settings_path: String,
    pub meta_path: String,
    pub journal_path: String,
    pub lock_path: String,
    pub backup_root: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Inspection {
    pub paths: Paths,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<Meta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub journal: Option<Journal>,
    pub detection: Detection,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Detection {
    pub current_version: i64,
    pub latest_version: i64,
    pub current_version_source: String,
    pub empty: bool,
    pub has_workspace: bool,
    pub has_legacy: bool,
    pub config_exists: bool,
    pub legacy_config_exists: bool,
    pub config_schema_version: i64,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub config_error: String,
    pub identity_index_exists: bool,
    pub identity_index_schema_version: i64,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub identity_index_error: String,
    pub database_exists: bool,
    pub database_schema_version: i64,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub database_error: String,
    pub legacy_identity_exists: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub legacy_identity_error: String,
    pub legacy_database_exists: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub legacy_database_error: String,
    pub legacy_settings_exists: bool,
}

impl Default for Detection {
    fn default() -> Self {
        Self {
            current_version: 0,
            latest_version: LATEST_WORKSPACE_SCHEMA_VERSION,
            current_version_source: String::new(),
            empty: false,
            has_workspace: false,
            has_legacy: false,
            config_exists: false,
            legacy_config_exists: false,
            config_schema_version: 0,
            config_error: String::new(),
            identity_index_exists: false,
            identity_index_schema_version: 0,
            identity_index_error: String::new(),
            database_exists: false,
            database_schema_version: 0,
            database_error: String::new(),
            legacy_identity_exists: false,
            legacy_identity_error: String::new(),
            legacy_database_exists: false,
            legacy_database_error: String::new(),
            legacy_settings_exists: false,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Meta {
    pub workspace_schema_version: i64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub app_version: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub last_upgrade_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub last_backup_dir: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Journal {
    pub upgrade_id: String,
    pub from_version: i64,
    pub to_version: i64,
    pub current_step: String,
    pub phase: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub backup_dir: String,
    pub started_at: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub app_version: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub(crate) struct LockMetadata {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub lock_scheme: String,
    pub pid: i64,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub app_version: String,
    pub started_at: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub hostname: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub executable: String,
}
