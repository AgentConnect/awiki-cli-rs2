use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;

pub const INDEX_SCHEMA_VERSION: i64 = 3;
pub const INDEX_FILE_NAME: &str = "index.json";
pub const IDENTITY_FILE_NAME: &str = "identity.json";
pub const AUTH_FILE_NAME: &str = "auth.json";
pub const DID_DOCUMENT_FILE_NAME: &str = "did_document.json";
pub const KEY1_PRIVATE_FILE_NAME: &str = "key-1-private.pem";
pub const KEY1_PUBLIC_FILE_NAME: &str = "key-1-public.pem";
pub const E2EE_SIGNING_PRIVATE_FILE_NAME: &str = "e2ee-signing-private.pem";
pub const E2EE_AGREEMENT_PRIVATE_FILE_NAME: &str = "e2ee-agreement-private.pem";
pub const E2EE_STATE_FILE_NAME: &str = "e2ee-state.json";
pub const LEGACY_BACKUP_DIR_NAME: &str = ".legacy-backup";
pub const LEGACY_E2EE_PREFIX: &str = "e2ee_";
pub const LEGACY_LAYOUT_HINT: &str =
    "Legacy credential layout detected. Import it before relying on the v2 identity store.";

#[derive(Debug)]
pub enum IdentityError {
    InvalidInput(String),
    NotFound(String),
    NoDefaultIdentity(String),
    Conflict(String),
    LegacyNotFound(String),
    AuthRequired(String),
    Service(super::wire::ServiceError),
    Io(std::io::Error),
    Json(serde_json::Error),
    Internal(String),
}

impl fmt::Display for IdentityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(message)
            | Self::NotFound(message)
            | Self::NoDefaultIdentity(message)
            | Self::Conflict(message)
            | Self::LegacyNotFound(message)
            | Self::AuthRequired(message)
            | Self::Internal(message) => f.write_str(message),
            Self::Service(err) => write!(f, "{err}"),
            Self::Io(err) => write!(f, "{err}"),
            Self::Json(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for IdentityError {}

impl From<std::io::Error> for IdentityError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for IdentityError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

impl From<super::wire::ServiceError> for IdentityError {
    fn from(value: super::wire::ServiceError) -> Self {
        Self::Service(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IndexEntry {
    #[serde(default, deserialize_with = "deserialize_string_lossy")]
    pub credential_name: String,
    #[serde(default, deserialize_with = "deserialize_string_lossy")]
    pub dir_name: String,
    #[serde(default, deserialize_with = "deserialize_string_lossy")]
    pub did: String,
    #[serde(default, deserialize_with = "deserialize_string_lossy")]
    pub unique_id: String,
    #[serde(
        default,
        deserialize_with = "deserialize_string_lossy",
        skip_serializing_if = "String::is_empty"
    )]
    pub user_id: String,
    #[serde(
        default,
        deserialize_with = "deserialize_string_lossy",
        skip_serializing_if = "String::is_empty"
    )]
    pub name: String,
    #[serde(
        default,
        deserialize_with = "deserialize_string_lossy",
        skip_serializing_if = "String::is_empty"
    )]
    pub handle: String,
    #[serde(
        default,
        deserialize_with = "deserialize_string_lossy",
        skip_serializing_if = "String::is_empty"
    )]
    pub full_handle: String,
    #[serde(
        default,
        deserialize_with = "deserialize_string_lossy",
        skip_serializing_if = "String::is_empty"
    )]
    pub created_at: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexPayload {
    pub schema_version: i64,
    #[serde(
        default,
        deserialize_with = "deserialize_string_lossy",
        skip_serializing_if = "String::is_empty"
    )]
    pub default_credential_name: String,
    #[serde(default)]
    pub credentials: BTreeMap<String, IndexEntry>,
}

impl Default for IndexPayload {
    fn default() -> Self {
        Self {
            schema_version: INDEX_SCHEMA_VERSION,
            default_credential_name: String::new(),
            credentials: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Paths {
    pub root_dir: String,
    pub dir_name: String,
    pub identity_dir: String,
    pub identity_path: String,
    pub auth_path: String,
    pub did_document_path: String,
    pub key1_private_path: String,
    pub key1_public_path: String,
    pub e2ee_signing_private_path: String,
    pub e2ee_agreement_private_path: String,
    pub e2ee_state_path: String,
}

#[derive(Debug, Clone, Default)]
pub struct StoredIdentity {
    pub identity_name: String,
    pub dir_name: String,
    pub did: String,
    pub unique_id: String,
    pub user_id: String,
    pub display_name: String,
    pub handle: String,
    pub full_handle: String,
    pub created_at: String,
    pub jwt_token: String,
    pub did_document: Option<Value>,
    pub key1_private_pem: String,
    pub key1_public_pem: String,
    pub e2ee_signing_private_pem: String,
    pub e2ee_agreement_private_pem: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct IdentitySummary {
    pub identity_name: String,
    pub did: String,
    pub unique_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub display_name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub handle: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub full_handle: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub created_at: String,
    pub dir_name: String,
    pub is_default: bool,
    pub has_jwt: bool,
    pub has_did_document: bool,
    pub has_key1_private: bool,
    pub has_key1_public: bool,
    pub has_e2ee_signing_private: bool,
    pub has_e2ee_agreement_private: bool,
    pub user_state: UserState,
    #[serde(skip)]
    pub user_id: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct UserState {
    pub registration_state: String,
    pub ready_for_messaging: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub missing: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LegacyFlatIdentity {
    pub credential_name: String,
    pub path: String,
    pub did: String,
    pub unique_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub handle: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LegacyScan {
    pub root_dir: String,
    pub indexed_layout: bool,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub indexed_entries: BTreeMap<String, IndexEntry>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub legacy_credentials: Vec<LegacyFlatIdentity>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub invalid_json_files: Vec<BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub orphan_e2ee_files: Vec<BTreeMap<String, String>>,
    pub has_legacy: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub hint: String,
}

#[derive(Debug, Clone, Default)]
pub struct SaveInput {
    pub identity_name: String,
    pub did: String,
    pub unique_id: String,
    pub user_id: String,
    pub display_name: String,
    pub handle: String,
    pub full_handle: String,
    pub jwt_token: String,
    pub did_document: Option<Value>,
    pub key1_private_pem: String,
    pub key1_public_pem: String,
    pub e2ee_signing_private_pem: String,
    pub e2ee_agreement_private_pem: String,
    pub replace_existing: bool,
}

#[derive(Debug, Clone, Default)]
pub struct RegisterParams {
    pub identity_name: String,
    pub handle: String,
    pub phone: String,
    pub email: String,
    pub otp: String,
    pub invite_code: String,
    pub wait: bool,
}

#[derive(Debug, Clone, Default)]
pub struct BindParams {
    pub phone: String,
    pub email: String,
    pub otp: String,
    pub wait: bool,
    pub verification_timeout: i64,
    pub poll_interval_seconds: f64,
}

#[derive(Debug, Clone, Default)]
pub struct RecoverParams {
    pub identity_name: String,
    pub handle: String,
    pub phone: String,
    pub otp: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ImportResult {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub imported: Vec<IdentitySummary>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub skipped: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct GeneratedIdentity {
    pub did: String,
    pub unique_id: String,
    pub did_document: Value,
    pub key1_private_pem: String,
    pub key1_public_pem: String,
    pub e2ee_signing_private_pem: String,
    pub e2ee_agreement_private_pem: String,
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn deserialize_string_lossy<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    Ok(value
        .and_then(|value| value.as_str().map(ToString::to_string))
        .unwrap_or_default())
}
