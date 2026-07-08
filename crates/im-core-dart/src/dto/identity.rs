#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DartIdentitySelector {
    Default,
    Id { id: String },
    Did { did: String },
    Handle { handle: String },
    LocalAlias { alias: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartIdentitySummary {
    pub id: String,
    pub did: String,
    pub handle: Option<String>,
    pub display_name: Option<String>,
    pub local_alias: Option<String>,
    pub device_id: Option<String>,
    pub is_default: bool,
    pub ready_for_auth: bool,
    pub ready_for_messaging: bool,
    pub missing: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DartIdentitySecretStorageBackend {
    FileCompat,
    Vault,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartIdentityVaultStatus {
    pub identity: DartIdentitySummary,
    pub storage_policy: crate::dto::config::DartIdentitySecretStoragePolicy,
    pub selected_backend: DartIdentitySecretStorageBackend,
    pub vault_available: bool,
    pub vault_metadata_present: bool,
    pub vault_metadata_verified: bool,
    pub workspace_id: Option<String>,
    pub device_id: Option<String>,
    pub plaintext_compat_retained: Option<bool>,
    pub missing: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartIdentityVaultMigrationReport {
    pub identity: DartIdentitySummary,
    pub status: DartIdentityVaultStatus,
    pub migrated: bool,
    pub verified: bool,
    pub plaintext_compat_retained: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartIdentityVaultVerificationReport {
    pub identity: DartIdentitySummary,
    pub status: DartIdentityVaultStatus,
    pub verified: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartInitialProfile {
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartDaemonSubkeyPrivatePackage {
    pub schema: String,
    pub user_did: String,
    pub verification_method: String,
    pub key_type: String,
    pub key_algorithm: Option<String>,
    pub public_key_multibase: String,
    pub private_key_encoding: String,
    pub private_key_pem: String,
    pub private_key_multibase: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartDaemonSubkeyAuthorizationRevokeResult {
    pub user_did: String,
    pub verification_method: String,
    pub updated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartDefaultIdentityChange {
    pub previous: Option<DartIdentitySummary>,
    pub next: DartIdentitySummary,
    pub requires_default_identity_write: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartDeleteLocalIdentityResult {
    pub deleted: DartIdentitySummary,
    pub was_default: bool,
    pub next_default: Option<DartIdentitySummary>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartHandleRegistrationResult {
    pub identity: Option<DartIdentitySummary>,
    pub handle: String,
    pub method: String,
    pub state: String,
    pub default_identity_change: Option<DartDefaultIdentityChange>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartRecoverHandleResult {
    pub handle: String,
    pub phone: String,
    pub state: String,
    pub recovered_identity: Option<DartIdentitySummary>,
    pub user_id: Option<String>,
    pub access_token_present: bool,
    pub warnings: Vec<String>,
}
