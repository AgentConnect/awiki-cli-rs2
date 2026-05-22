use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IdentitySelector {
    Default,
    Id(crate::ids::IdentityId),
    Did(crate::ids::Did),
    Handle(crate::ids::Handle),
    LocalAlias(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentitySummary {
    pub id: crate::ids::IdentityId,
    pub did: crate::ids::Did,
    pub handle: Option<crate::ids::Handle>,
    pub display_name: Option<String>,
    pub local_alias: Option<String>,
    pub device_id: Option<String>,
    pub is_default: bool,
    pub readiness: IdentityReadiness,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityReadiness {
    pub ready_for_auth: bool,
    pub ready_for_messaging: bool,
    pub missing: Vec<IdentityMissingItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IdentityMissingItem {
    DidDocument,
    PrivateKey,
    AuthState,
    Handle,
    MessageEndpoint,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterHandleRequest {
    pub local_alias: Option<String>,
    pub requested_handle: crate::ids::Handle,
    pub verification: VerificationInput,
    pub invite_code: Option<String>,
    pub profile: InitialProfile,
    pub make_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerificationInput {
    Otp {
        code: String,
    },
    Phone {
        phone: String,
        otp: Option<String>,
    },
    Email {
        email: String,
        wait_for_verification: bool,
    },
    AlreadyVerified,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitialProfile {
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandleRegistrationResult {
    pub identity: Option<IdentitySummary>,
    pub handle: crate::ids::Handle,
    pub method: RegistrationMethod,
    pub state: HandleRegistrationState,
    pub default_identity_change: Option<DefaultIdentityChange>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RegistrationMethod {
    Phone,
    Email,
    AlreadyVerified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HandleRegistrationState {
    OtpSent,
    EmailSent,
    EmailPending,
    Registered,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DefaultIdentityChange {
    pub previous: Option<IdentitySummary>,
    pub next: IdentitySummary,
    pub requires_default_identity_write: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub subject: crate::ids::Did,
    pub handle: Option<crate::ids::Handle>,
    pub display_name: Option<String>,
    pub bio: Option<String>,
    pub tags: Vec<String>,
    pub markdown: Option<String>,
    pub avatar_url: Option<String>,
    pub updated_at: Option<String>,
    pub metadata: Vec<ProfileAttribute>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileAttribute {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProfilePatch {
    pub display_name: Option<String>,
    pub bio: Option<String>,
    pub tags: Option<Vec<String>>,
    pub markdown: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContactBindingRequest {
    pub method: ContactBindingMethod,
    pub wait_for_email_verification: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContactBindingMethod {
    Phone { phone: String, otp: Option<String> },
    Email { email: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContactBindingResult {
    pub method: ContactBindingMethodKind,
    pub target: String,
    pub state: ContactBindingState,
    pub raw: Option<serde_json::Value>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContactBindingMethodKind {
    Phone,
    Email,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContactBindingState {
    OtpSent,
    EmailSent,
    Pending,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecoverHandleRequest {
    pub handle: crate::ids::Handle,
    pub phone: String,
    pub otp: Option<String>,
    pub generated_identity: Option<RecoverGeneratedIdentity>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecoverGeneratedIdentity {
    pub did: crate::ids::Did,
    pub unique_id: String,
    pub did_document: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecoverHandleResult {
    pub handle: crate::ids::Handle,
    pub phone: String,
    pub state: RecoverHandleState,
    pub recovered_identity: Option<RecoveredIdentity>,
    pub raw: Option<serde_json::Value>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecoverHandleState {
    OtpSent,
    Recovered,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecoveredIdentity {
    pub identity: IdentitySummary,
    pub user_id: Option<String>,
    pub access_token_present: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplaceDidPlanRequest {
    pub identity: IdentitySummary,
    pub linked_identity_names: Vec<String>,
    pub planned_new_did: crate::ids::Did,
    pub backup_path_preview: String,
    pub old_dir_name: String,
    pub is_public: Option<bool>,
    pub is_agent: Option<bool>,
    pub role: Option<String>,
    pub endpoint_url: Option<String>,
    pub affected_local_state: ReplaceDidAffectedLocalState,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplaceDidPlan {
    pub action: String,
    pub identity: IdentitySummary,
    pub dangerous: bool,
    pub risk_summary: Vec<String>,
    pub backup_plan: ReplaceDidBackupPlan,
    pub local_rebind_plan: ReplaceDidLocalRebindPlan,
    pub affected_local_state: ReplaceDidAffectedLocalState,
    pub remote_replace_did_call_preview: ReplaceDidRemoteCallPreview,
    pub rollback_notes: Vec<String>,
    pub local_writes: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplaceDidBackupPlan {
    pub required: bool,
    pub backup_path_preview: String,
    pub manifest_preview: ReplaceDidBackupManifestPreview,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplaceDidBackupManifestPreview {
    pub reason: String,
    pub identity_name: String,
    pub linked_identity_names: Vec<String>,
    pub old_did: crate::ids::Did,
    pub old_dir_name: String,
    pub planned_new_did: crate::ids::Did,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplaceDidLocalRebindPlan {
    pub required: bool,
    pub old_owner_did: crate::ids::Did,
    pub new_owner_did: crate::ids::Did,
    pub destructive: bool,
    pub dry_run_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ReplaceDidAffectedLocalState {
    pub store_rebind_counts: BTreeMap<String, i64>,
    pub e2ee_cleanup_counts: BTreeMap<String, i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplaceDidRemoteCallPreview {
    pub endpoint: String,
    pub method: String,
    pub params: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplaceDidGeneratedIdentity {
    pub did: crate::ids::Did,
    pub unique_id: String,
    pub did_document: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplaceDidExecutionRequest {
    pub plan: ReplaceDidPlan,
    pub generated_identity: ReplaceDidGeneratedIdentity,
    pub is_public: Option<bool>,
    pub is_agent: Option<bool>,
    pub role: Option<String>,
    pub endpoint_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplaceDidExecutionResult {
    pub identity: IdentitySummary,
    pub old_did: crate::ids::Did,
    pub new_did: crate::ids::Did,
    pub backup_path: String,
    pub backup_manifest: ReplaceDidBackupManifestPreview,
    pub affected_local_state: ReplaceDidAffectedLocalState,
    pub remote_result: serde_json::Value,
    pub warnings: Vec<String>,
    pub recovery_notes: Vec<String>,
}
