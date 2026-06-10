use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;
use std::path::PathBuf;

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

pub const DAEMON_SUBKEY_PACKAGE_SCHEMA_V1: &str = "awiki.daemon.user_subkey_package.v1";
pub const DAEMON_SUBKEY_PACKAGE_SCHEMA_V2: &str = "awiki.daemon.user_subkey_package.v2";
pub const DAEMON_SUBKEY_PRIVATE_KEY_ENCODING_PEM: &str = "pem";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonSubkeyPrivatePackage {
    pub schema: String,
    pub user_did: crate::ids::Did,
    pub verification_method: String,
    pub key_type: String,
    pub key_algorithm: Option<String>,
    pub public_key_multibase: String,
    pub private_key_encoding: String,
    pub private_key_pem: String,
    /// Legacy compatibility field. New JSON serialization writes `private_key_pem`
    /// instead of this v1 field, but older Rust/Dart callers may still read it.
    pub private_key_multibase: String,
}

impl DaemonSubkeyPrivatePackage {
    pub fn new_v2_pem(
        user_did: crate::ids::Did,
        verification_method: String,
        key_type: String,
        key_algorithm: Option<String>,
        public_key_multibase: String,
        private_key_pem: String,
    ) -> Self {
        Self {
            schema: DAEMON_SUBKEY_PACKAGE_SCHEMA_V2.to_owned(),
            user_did,
            verification_method,
            key_type,
            key_algorithm,
            public_key_multibase,
            private_key_encoding: DAEMON_SUBKEY_PRIVATE_KEY_ENCODING_PEM.to_owned(),
            private_key_multibase: private_key_pem.clone(),
            private_key_pem,
        }
    }

    pub fn private_key_material(&self) -> &str {
        if !self.private_key_pem.trim().is_empty() {
            &self.private_key_pem
        } else {
            &self.private_key_multibase
        }
    }

    pub fn is_v2_pem(&self) -> bool {
        self.schema == DAEMON_SUBKEY_PACKAGE_SCHEMA_V2
            && self.private_key_encoding == DAEMON_SUBKEY_PRIVATE_KEY_ENCODING_PEM
            && !self.private_key_pem.trim().is_empty()
    }
}

impl Serialize for DaemonSubkeyPrivatePackage {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        #[derive(Serialize)]
        struct Wire<'a> {
            schema: &'a str,
            user_did: &'a crate::ids::Did,
            verification_method: &'a str,
            key_type: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            key_algorithm: Option<&'a str>,
            public_key_multibase: &'a str,
            private_key_encoding: &'a str,
            private_key_pem: &'a str,
        }

        let private_key_pem = self.private_key_material();
        let private_key_encoding = if self.private_key_encoding.trim().is_empty() {
            DAEMON_SUBKEY_PRIVATE_KEY_ENCODING_PEM
        } else {
            self.private_key_encoding.trim()
        };
        Wire {
            schema: DAEMON_SUBKEY_PACKAGE_SCHEMA_V2,
            user_did: &self.user_did,
            verification_method: &self.verification_method,
            key_type: &self.key_type,
            key_algorithm: self.key_algorithm.as_deref(),
            public_key_multibase: &self.public_key_multibase,
            private_key_encoding,
            private_key_pem,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for DaemonSubkeyPrivatePackage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Wire {
            schema: String,
            user_did: crate::ids::Did,
            verification_method: String,
            key_type: String,
            #[serde(default)]
            key_algorithm: Option<String>,
            public_key_multibase: String,
            #[serde(default)]
            private_key_encoding: Option<String>,
            #[serde(default)]
            private_key_pem: Option<String>,
            #[serde(default)]
            private_key_multibase: Option<String>,
        }

        let wire = Wire::deserialize(deserializer)?;
        let private_key_pem = wire
            .private_key_pem
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| {
                wire.private_key_multibase
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned)
            })
            .ok_or_else(|| serde::de::Error::missing_field("private_key_pem"))?;
        let private_key_encoding = wire
            .private_key_encoding
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(DAEMON_SUBKEY_PRIVATE_KEY_ENCODING_PEM)
            .to_string();
        Ok(Self {
            schema: wire.schema,
            user_did: wire.user_did,
            verification_method: wire.verification_method,
            key_type: wire.key_type,
            key_algorithm: wire.key_algorithm,
            public_key_multibase: wire.public_key_multibase,
            private_key_encoding,
            private_key_multibase: private_key_pem.clone(),
            private_key_pem,
        })
    }
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
pub struct DeleteLocalIdentityResult {
    pub deleted: IdentitySummary,
    pub was_default: bool,
    pub next_default: Option<IdentitySummary>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub subject: crate::ids::Did,
    pub handle: Option<crate::ids::Handle>,
    pub display_name: Option<String>,
    pub bio: Option<String>,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub markdown: Option<String>,
    pub avatar_uri: Option<String>,
    pub avatar_url: Option<String>,
    pub profile_uri: Option<String>,
    pub subject_type: Option<String>,
    pub updated_at: Option<String>,
    #[serde(
        default,
        rename = "versionId",
        alias = "version_id",
        skip_serializing_if = "Option::is_none"
    )]
    pub version_id: Option<String>,
    pub ttl: Option<u64>,
    pub proof: Option<serde_json::Value>,
    pub metadata: Vec<ProfileAttribute>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileAttribute {
    pub key: String,
    pub value: String,
}

impl Profile {
    pub fn new(subject: crate::ids::Did) -> Self {
        Self {
            subject,
            handle: None,
            display_name: None,
            bio: None,
            description: None,
            tags: Vec::new(),
            markdown: None,
            avatar_uri: None,
            avatar_url: None,
            profile_uri: None,
            subject_type: None,
            updated_at: None,
            version_id: None,
            ttl: None,
            proof: None,
            metadata: Vec::new(),
        }
    }

    pub fn effective_description(&self) -> Option<&String> {
        self.description.as_ref().or(self.bio.as_ref())
    }

    pub fn effective_avatar_uri(&self) -> Option<&String> {
        self.avatar_uri.as_ref().or(self.avatar_url.as_ref())
    }

    pub fn to_wire_profile_value(&self) -> serde_json::Value {
        let mut value = serde_json::Map::new();
        value.insert(
            "did".to_string(),
            serde_json::Value::String(self.subject.as_str().to_string()),
        );
        value.insert(
            "subject_did".to_string(),
            serde_json::Value::String(self.subject.as_str().to_string()),
        );
        if let Some(handle) = self.handle.as_ref() {
            value.insert(
                "handle".to_string(),
                serde_json::Value::String(handle.as_str().to_string()),
            );
        }
        if let Some(display_name) = self.display_name.as_ref() {
            value.insert(
                "display_name".to_string(),
                serde_json::Value::String(display_name.clone()),
            );
            value.insert(
                "nick_name".to_string(),
                serde_json::Value::String(display_name.clone()),
            );
        }
        if let Some(description) = self.effective_description() {
            value.insert(
                "description".to_string(),
                serde_json::Value::String(description.clone()),
            );
        }
        if let Some(bio) = self.bio.as_ref().or(self.description.as_ref()) {
            value.insert("bio".to_string(), serde_json::Value::String(bio.clone()));
        }
        if !self.tags.is_empty() {
            value.insert("tags".to_string(), serde_json::json!(self.tags));
        }
        if let Some(markdown) = self.markdown.as_ref() {
            value.insert(
                "profile_md".to_string(),
                serde_json::Value::String(markdown.clone()),
            );
        }
        if let Some(avatar_uri) = self.effective_avatar_uri() {
            value.insert(
                "avatar_uri".to_string(),
                serde_json::Value::String(avatar_uri.clone()),
            );
        }
        if let Some(avatar_url) = self.avatar_url.as_ref().or(self.avatar_uri.as_ref()) {
            value.insert(
                "avatar_url".to_string(),
                serde_json::Value::String(avatar_url.clone()),
            );
        }
        if let Some(profile_uri) = self.profile_uri.as_ref() {
            value.insert(
                "profile_uri".to_string(),
                serde_json::Value::String(profile_uri.clone()),
            );
        }
        if let Some(subject_type) = self.subject_type.as_ref() {
            value.insert(
                "subject_type".to_string(),
                serde_json::Value::String(subject_type.clone()),
            );
        }
        if let Some(updated_at) = self.updated_at.as_ref() {
            value.insert(
                "updated_at".to_string(),
                serde_json::Value::String(updated_at.clone()),
            );
            value.insert(
                "updated".to_string(),
                serde_json::Value::String(updated_at.clone()),
            );
        }
        if let Some(version_id) = self.version_id.as_ref() {
            value.insert(
                "versionId".to_string(),
                serde_json::Value::String(version_id.clone()),
            );
        }
        if let Some(ttl) = self.ttl {
            value.insert("ttl".to_string(), serde_json::json!(ttl));
        }
        if let Some(proof) = self.proof.as_ref() {
            value.insert("proof".to_string(), proof.clone());
        }
        if !self.metadata.is_empty() {
            value.insert(
                "metadata".to_string(),
                serde_json::Value::Object(
                    self.metadata
                        .iter()
                        .map(|attribute| {
                            (
                                attribute.key.clone(),
                                serde_json::Value::String(attribute.value.clone()),
                            )
                        })
                        .collect(),
                ),
            );
        }
        serde_json::Value::Object(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProfilePatch {
    pub display_name: Option<String>,
    pub bio: Option<String>,
    pub tags: Option<Vec<String>>,
    pub markdown: Option<String>,
    pub avatar_uri: Option<String>,
    pub avatar_url: Option<String>,
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
    #[serde(skip)]
    raw_response: Option<serde_json::Value>,
    pub warnings: Vec<String>,
}

impl ContactBindingResult {
    pub(crate) fn with_raw_response(
        method: ContactBindingMethodKind,
        target: String,
        state: ContactBindingState,
        raw_response: Option<serde_json::Value>,
        warnings: Vec<String>,
    ) -> Self {
        Self {
            method,
            target,
            state,
            raw_response,
            warnings,
        }
    }

    pub fn response_json(&self) -> Option<&serde_json::Value> {
        self.raw_response.as_ref()
    }
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_handle: Option<String>,
    pub phone: String,
    pub otp: Option<String>,
    pub generated_identity: Option<RecoverGeneratedIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_finalize: Option<RecoverHandleLocalFinalizeRequest>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecoverGeneratedIdentity {
    pub did: crate::ids::Did,
    pub unique_id: String,
    pub did_document: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RecoverHandleLocalFinalizeRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_handle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_identity_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_file_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoverHandlePlanRequest {
    pub handle: crate::ids::Handle,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_handle: Option<String>,
    pub phone: String,
    pub otp: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecoverHandlePlan {
    pub action: String,
    pub target_handle: String,
    pub identity_name: String,
    pub final_identity_name: String,
    pub temp_identity_name: String,
    pub same_handle_candidates: Vec<RecoverLocalIdentitySummary>,
    pub excluded_identities: Vec<RecoverLocalIdentitySummary>,
    pub backup_path: String,
    pub phone: String,
    pub remote_calls: Vec<String>,
    pub local_writes: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecoverHandleResult {
    pub handle: crate::ids::Handle,
    pub phone: String,
    pub state: RecoverHandleState,
    pub recovered_identity: Option<RecoveredIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_recovery: Option<RecoverHandleLocalResult>,
    #[serde(skip)]
    raw_response: Option<serde_json::Value>,
    pub warnings: Vec<String>,
}

impl RecoverHandleResult {
    pub(crate) fn with_raw_response(
        handle: crate::ids::Handle,
        phone: String,
        state: RecoverHandleState,
        recovered_identity: Option<RecoveredIdentity>,
        local_recovery: Option<RecoverHandleLocalResult>,
        raw_response: Option<serde_json::Value>,
        warnings: Vec<String>,
    ) -> Self {
        Self {
            handle,
            phone,
            state,
            recovered_identity,
            local_recovery,
            raw_response,
            warnings,
        }
    }

    pub fn response_json(&self) -> Option<&serde_json::Value> {
        self.raw_response.as_ref()
    }
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
pub struct RecoverHandleLocalResult {
    pub identity: RecoverLocalIdentitySummary,
    pub backup_path: String,
    pub archived_identities: Vec<String>,
    pub archived_dids: Vec<String>,
    pub full_handle: String,
    pub final_identity_name: String,
    pub store_merge_counts: BTreeMap<String, i64>,
    pub e2ee_cleanup_counts: BTreeMap<String, i64>,
    pub default_updated: bool,
    pub active_config_updated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RecoverLocalIdentitySummary {
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
    pub user_state: RecoverLocalUserState,
    #[serde(skip)]
    pub user_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RecoverLocalUserState {
    pub registration_state: String,
    pub ready_for_messaging: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub missing: Vec<String>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn binding_and_recover_results_keep_raw_response_internal_only() {
        let binding = ContactBindingResult::with_raw_response(
            ContactBindingMethodKind::Email,
            "alice@example.test".to_string(),
            ContactBindingState::EmailSent,
            Some(json!({ "provider_state": "sent" })),
            vec!["queued".to_string()],
        );
        let binding_json = serde_json::to_value(&binding).expect("serialize binding result");
        assert_eq!(
            binding
                .response_json()
                .and_then(|raw| raw.get("provider_state")),
            Some(&json!("sent"))
        );
        assert!(binding_json.get("raw_response").is_none());
        assert!(binding_json.get("raw_response").is_none());
        assert!(binding_json.get("raw").is_none());

        let recover = RecoverHandleResult::with_raw_response(
            crate::ids::Handle::parse("alice", "example.test").expect("handle"),
            "+15551234567".to_string(),
            RecoverHandleState::OtpSent,
            None,
            None,
            Some(json!({ "sent": true })),
            Vec::new(),
        );
        let recover_json = serde_json::to_value(&recover).expect("serialize recover result");
        assert_eq!(
            recover.response_json().and_then(|raw| raw.get("sent")),
            Some(&json!(true))
        );
        assert!(recover_json.get("raw_response").is_none());
        assert!(recover_json.get("raw_response").is_none());
        assert!(recover_json.get("raw").is_none());
    }

    #[test]
    fn daemon_subkey_package_writes_v2_pem_without_legacy_private_field() {
        let package = DaemonSubkeyPrivatePackage::new_v2_pem(
            crate::ids::Did::parse("did:example:alice").unwrap(),
            "did:example:alice#daemon-key-1".to_string(),
            "Multikey/Ed25519".to_string(),
            Some("Ed25519".to_string()),
            "zPublic".to_string(),
            "-----BEGIN PRIVATE KEY-----\nsecret\n-----END PRIVATE KEY-----".to_string(),
        );

        let value = serde_json::to_value(&package).unwrap();

        assert_eq!(value["schema"], DAEMON_SUBKEY_PACKAGE_SCHEMA_V2);
        assert_eq!(
            value["private_key_encoding"],
            DAEMON_SUBKEY_PRIVATE_KEY_ENCODING_PEM
        );
        assert_eq!(value["private_key_pem"], package.private_key_pem);
        assert!(value.get("private_key_multibase").is_none());
    }

    #[test]
    fn daemon_subkey_package_reads_legacy_v1_private_key_multibase() {
        let package: DaemonSubkeyPrivatePackage = serde_json::from_value(json!({
            "schema": DAEMON_SUBKEY_PACKAGE_SCHEMA_V1,
            "user_did": "did:example:alice",
            "verification_method": "did:example:alice#daemon-key-1",
            "key_type": "Multikey/Ed25519",
            "public_key_multibase": "zPublic",
            "private_key_multibase": "-----BEGIN PRIVATE KEY-----\nsecret\n-----END PRIVATE KEY-----"
        }))
        .unwrap();

        assert_eq!(package.schema, DAEMON_SUBKEY_PACKAGE_SCHEMA_V1);
        assert_eq!(
            package.private_key_encoding,
            DAEMON_SUBKEY_PRIVATE_KEY_ENCODING_PEM
        );
        assert_eq!(package.private_key_pem, package.private_key_multibase);
        assert!(package
            .private_key_material()
            .starts_with("-----BEGIN PRIVATE KEY-----"));
    }
}
