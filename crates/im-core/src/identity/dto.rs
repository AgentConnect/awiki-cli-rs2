use serde::{Deserialize, Deserializer, Serialize, Serializer};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityDeviceMode {
    Legacy,
    VNext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityDeviceRole {
    Member,
    Admin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityDeviceAuthorizationStatus {
    Active,
    Revoked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentIdentityKind {
    Skill,
    Daemon,
    Runtime,
}

impl AgentIdentityKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Skill => "skill",
            Self::Daemon => "daemon",
            Self::Runtime => "runtime",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityDeviceReadiness {
    Legacy,
    MemberReady,
    AdminAwaitingRoot,
    AdminReady,
    Blocked,
}

/// Safe local device projection. It intentionally excludes Vault references,
/// private-key presence flags and internal document/Registry checkpoints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityDeviceSummary {
    pub identity: IdentitySummary,
    pub mode: IdentityDeviceMode,
    pub protocol_device_id: Option<crate::ids::ProtocolDeviceId>,
    pub role: Option<IdentityDeviceRole>,
    pub signing_key_id: Option<String>,
    pub e2ee_key_id: Option<String>,
    pub readiness: IdentityDeviceReadiness,
    pub blocked_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityCustodyBackend {
    AnpIdentity,
    LegacyFileCompat,
    LegacyVault,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityCustodyState {
    Creating,
    Active,
    Enrolling,
    Revoked,
    Legacy,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityCustodyStatus {
    pub identity: IdentitySummary,
    pub backend: IdentityCustodyBackend,
    pub state: IdentityCustodyState,
    pub ready: bool,
    pub root_control_available: bool,
    pub pending_operation: bool,
    pub store_id: Option<String>,
    pub custody_identity_id: Option<String>,
    pub missing: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentitySecretStorageBackend {
    FileCompat,
    Vault,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityVaultStatus {
    pub identity: IdentitySummary,
    pub storage_policy: crate::core::IdentitySecretStoragePolicy,
    pub selected_backend: IdentitySecretStorageBackend,
    pub vault_available: bool,
    pub vault_metadata_present: bool,
    pub vault_metadata_verified: bool,
    pub workspace_id: Option<String>,
    pub device_id: Option<String>,
    pub plaintext_compat_retained: Option<bool>,
    pub missing: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityVaultMigrationReport {
    pub identity: IdentitySummary,
    pub status: IdentityVaultStatus,
    pub migrated: bool,
    pub verified: bool,
    pub plaintext_compat_retained: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityVaultVerificationReport {
    pub identity: IdentitySummary,
    pub status: IdentityVaultStatus,
    pub verified: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityCustodyMigrationPhase {
    NotRequired,
    Blocked,
    Eligible,
    Copied,
    Verified,
    Cutover,
    Cleaned,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityCustodyMigrationIdentityReport {
    pub identity_name: String,
    pub did: String,
    pub eligible: bool,
    pub already_managed: bool,
    pub root_capability_present: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityCustodyMigrationReport {
    pub dry_run: bool,
    pub phase: IdentityCustodyMigrationPhase,
    pub store_id: Option<String>,
    pub marker_written: bool,
    pub cleanup_complete: bool,
    pub copied_count: usize,
    pub verified_count: usize,
    pub identities: Vec<IdentityCustodyMigrationIdentityReport>,
    pub blockers: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct HostedIdentityMaterial {
    pub identity_id: String,
    pub did: String,
    pub handle: Option<String>,
    pub display_name: Option<String>,
    pub did_document: serde_json::Value,
    pub default_signing_private_key_pem: String,
    pub e2ee_agreement_private_key_pem: Option<String>,
    pub auth_token: Option<String>,
}

impl std::fmt::Debug for HostedIdentityMaterial {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostedIdentityMaterial")
            .field("identity_id", &self.identity_id)
            .field("did", &self.did)
            .field("handle", &self.handle)
            .field("display_name", &self.display_name)
            .field("did_document", &"<redacted-hosted-did-document>")
            .field("default_signing_private_key_pem", &"<redacted-private-key>")
            .field("e2ee_agreement_private_key_pem", &"<redacted-private-key>")
            .field(
                "auth_token",
                &self.auth_token.as_ref().map(|_| "<redacted-token>"),
            )
            .finish()
    }
}

/// Fresh vNext Agent identity material produced for a trusted in-process host.
///
/// This secret-bearing value intentionally does not implement Serde. Hosts
/// must move it directly into their SecretVault-backed pending record and must
/// never log it or persist it as ordinary application data.
#[derive(Clone, PartialEq)]
pub struct VNextAgentBootstrapMaterial {
    pub kind: AgentIdentityKind,
    pub handle_local_part: String,
    pub identity_id: String,
    pub did: crate::ids::Did,
    pub did_document: serde_json::Value,
    pub document_hash: String,
    pub protocol_device_id: crate::ids::ProtocolDeviceId,
    pub root_key_id: String,
    pub root_private_key_pem: String,
    pub root_public_key_pem: String,
    pub device_signing_key_id: String,
    pub device_signing_private_key_pem: String,
    pub device_signing_public_key_pem: String,
    pub device_e2ee_key_id: String,
    pub device_e2ee_private_key_pem: String,
    pub device_e2ee_public_key_pem: String,
}

impl std::fmt::Debug for VNextAgentBootstrapMaterial {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VNextAgentBootstrapMaterial")
            .field("kind", &self.kind)
            .field("handle_local_part", &self.handle_local_part)
            .field("identity_id", &self.identity_id)
            .field("did", &self.did)
            .field("did_document", &"<redacted-did-document>")
            .field("document_hash", &self.document_hash)
            .field("protocol_device_id", &self.protocol_device_id)
            .field("root_key_material", &"<redacted-key-material>")
            .field("device_signing_key_material", &"<redacted-key-material>")
            .field("device_e2ee_key_material", &"<redacted-key-material>")
            .finish()
    }
}

/// Crash-recovery classification for a same-DID Legacy Agent upgrade.
///
/// `TargetCommitted` means the remote document is byte-for-byte the prepared
/// target and callers must not issue `update_document` again. `LegacyRebuilt`
/// preserves the exact pending device identity and keys while refreshing the
/// target proof and source-owned document extensions from the remote Legacy
/// document.
#[derive(Clone, PartialEq)]
pub enum VNextAgentLegacyUpgradeReconciliation {
    TargetCommitted,
    LegacyRebuilt { target: VNextAgentBootstrapMaterial },
}

impl std::fmt::Debug for VNextAgentLegacyUpgradeReconciliation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TargetCommitted => f.write_str("TargetCommitted"),
            Self::LegacyRebuilt { target } => f
                .debug_struct("LegacyRebuilt")
                .field("target", target)
                .finish(),
        }
    }
}

/// Exact committed bootstrap-device session recovered after a lost Legacy
/// upgrade response. The access token is deliberately redacted from `Debug`.
#[derive(Clone, PartialEq, Eq)]
pub struct VNextAgentLegacyUpgradeSession {
    pub did: crate::ids::Did,
    pub user_id: String,
    pub binding_generation: String,
    pub access_token: String,
}

impl std::fmt::Debug for VNextAgentLegacyUpgradeSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VNextAgentLegacyUpgradeSession")
            .field("did", &self.did)
            .field("user_id", &self.user_id)
            .field("binding_generation", &self.binding_generation)
            .field("access_token", &"<redacted-token>")
            .finish()
    }
}

/// Exact vNext Device Identity material owned by a trusted in-process host.
///
/// Unlike [`HostedIdentityMaterial`], this type carries a validated account,
/// device and credential-generation binding. It intentionally does not
/// implement Serde; the host remains responsible for encrypted-at-rest secret
/// storage and should construct this value only at the im-core call boundary.
#[derive(Clone, PartialEq)]
pub struct HostBackedDeviceIdentityMaterial {
    pub identity_id: String,
    pub did: String,
    pub handle: Option<String>,
    pub display_name: Option<String>,
    pub account_id: String,
    pub binding_generation: String,
    pub did_document: serde_json::Value,
    pub protocol_device_id: crate::ids::ProtocolDeviceId,
    pub device_signing_key_id: String,
    pub device_signing_private_key_pem: String,
    pub device_e2ee_key_id: String,
    pub device_e2ee_private_key_pem: String,
    pub root_key_id: String,
    pub root_private_key_pem: String,
    pub authorization_status: IdentityDeviceAuthorizationStatus,
    pub role: IdentityDeviceRole,
    pub management_ready: bool,
    pub auth_generation: String,
    pub access_token: String,
}

/// Optional trusted-host persistence used after im-core has validated a newly
/// issued exact-device access token. Implementations must commit atomically and
/// must never log or expose the token.
pub trait HostBackedAuthTokenPersistence: Send + Sync {
    fn persist_auth_token(&self, token: &str) -> crate::ImResult<()>;
}

impl std::fmt::Debug for HostBackedDeviceIdentityMaterial {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostBackedDeviceIdentityMaterial")
            .field("identity_id", &self.identity_id)
            .field("did", &self.did)
            .field("handle", &self.handle)
            .field("display_name", &self.display_name)
            .field("account_id", &self.account_id)
            .field("binding_generation", &self.binding_generation)
            .field("did_document", &"<redacted-did-document>")
            .field("protocol_device_id", &self.protocol_device_id)
            .field("device_signing_key_material", &"<redacted-key-material>")
            .field("device_e2ee_key_material", &"<redacted-key-material>")
            .field("root_key_material", &"<redacted-key-material>")
            .field("authorization_status", &self.authorization_status)
            .field("role", &self.role)
            .field("management_ready", &self.management_ready)
            .field("auth_generation", &self.auth_generation)
            .field("access_token", &"<redacted-token>")
            .finish()
    }
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

/// Registration input reserved for trusted Rust backend services.
///
/// The operation id is sent to User Service as the durable idempotency key. It
/// deliberately stays out of the Node, Dart and CLI bindings.
#[cfg(feature = "service-trusted-registration")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedServiceRegisterHandleRequest {
    pub registration: RegisterHandleRequest,
    pub provision_operation_id: String,
}

/// Durable local preparation result for a trusted backend registration.
///
/// The digest covers the canonical RPC params that will be submitted by
/// `register_handle_with_trusted_service_async`. Preparing does not perform any
/// network request.
#[cfg(feature = "service-trusted-registration")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedServiceRegistrationPreparation {
    pub canonical_request_sha256: [u8; 32],
}

pub(crate) const DAEMON_SUBKEY_PACKAGE_SCHEMA_V1: &str = "awiki.daemon.user_subkey_package.v1";
pub(crate) const DAEMON_SUBKEY_PACKAGE_SCHEMA_V2: &str = "awiki.daemon.user_subkey_package.v2";
pub(crate) const DAEMON_SUBKEY_PRIVATE_KEY_ENCODING_PEM: &str = "pem";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DaemonSubkeyPrivatePackage {
    pub(crate) schema: String,
    pub(crate) user_did: crate::ids::Did,
    pub(crate) verification_method: String,
    pub(crate) key_type: String,
    pub(crate) key_algorithm: Option<String>,
    pub(crate) public_key_multibase: String,
    pub(crate) private_key_encoding: String,
    pub(crate) private_key_pem: String,
    /// Legacy compatibility field. New JSON serialization writes `private_key_pem`
    /// instead of this v1 field, but older Rust/Dart callers may still read it.
    pub(crate) private_key_multibase: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonSubkeyAuthorizationRevokeResult {
    pub user_did: crate::ids::Did,
    pub verification_method: String,
    pub updated: bool,
}

pub const DAEMON_SUBKEY_PUBLIC_PACKAGE_SCHEMA_V3: &str = "awiki.daemon.user_subkey_package.v3";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DaemonSubkeyPublicProposal {
    pub user_did: crate::ids::Did,
    pub verification_method: String,
    pub public_key_multibase: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DaemonSubkeyPublicPackage {
    pub schema: String,
    pub user_did: crate::ids::Did,
    pub verification_method: String,
    pub key_type: String,
    pub key_algorithm: String,
    pub public_key_multibase: String,
}

impl DaemonSubkeyPrivatePackage {
    pub(crate) fn new_v2_pem(
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

    pub(crate) fn private_key_material(&self) -> &str {
        if !self.private_key_pem.trim().is_empty() {
            &self.private_key_pem
        } else {
            &self.private_key_multibase
        }
    }

    pub(crate) fn is_v2_pem(&self) -> bool {
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
    pub account_id: Option<String>,
    pub handle: crate::ids::Handle,
    pub method: RegistrationMethod,
    pub state: HandleRegistrationState,
    pub join_required: Option<HandleRegistrationJoinRequiredPreparation>,
    pub default_identity_change: Option<DefaultIdentityChange>,
    /// Retry guidance returned by the registration OTP endpoint.
    /// Present only when `state` is `OtpSent`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_seconds: Option<u32>,
    /// RFC 3339 retry timestamp returned by the registration OTP endpoint.
    /// Present only when `state` is `OtpSent`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_at: Option<String>,
    pub warnings: Vec<String>,
}

/// Read-only, secret-free binding used to scope account synchronization.
///
/// Every field is derived and validated by Core. Hosts cannot construct or
/// override the active binding used by sync.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveSyncAccountBinding {
    pub owner_identity_id: String,
    pub account_id: String,
    pub current_did: String,
    pub protocol_device_id: String,
    pub identity_generation: String,
    pub device_auth_generation: String,
}

/// Secret-free authority for adopting one pre-Recovery Registry epoch into a
/// product-local store. `provenance_id` is an opaque Core digest; hosts must
/// never reconstruct or broaden this decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyRegistryEpochAdoptionAuthority {
    pub owner_identity_id: crate::ids::IdentityId,
    pub account_user_id: String,
    pub current_did: crate::ids::Did,
    pub binding_generation: String,
    pub protocol_device_id: crate::ids::ProtocolDeviceId,
    pub device_auth_generation: String,
    pub provenance_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandleRegistrationJoinMode {
    Ordinary,
    HandleRecoveryRebind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandleRegistrationJoinRequiredPreparation {
    pub preparation_id: String,
    pub mode: HandleRegistrationJoinMode,
    pub requires_user_presence: bool,
    pub expected_did: crate::ids::Did,
    pub full_handle: crate::ids::Handle,
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
    JoinRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum LegacyUpgradeStatus {
    Idle,
    Running,
    RetryRequired { identity_id: String, code: String },
    Completed,
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

/// Opaque, secret-free authority for one committed local identity data
/// deletion. Hosts use it only to coordinate their own product store before
/// asking Core to continue the same deletion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalIdentityDeletionTicket {
    pub deletion_id: String,
    pub owner_identity_id: crate::ids::IdentityId,
    pub current_did: crate::ids::Did,
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
    pub agent_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub agent_capabilities: Vec<String>,
    pub updated_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_version: Option<String>,
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
            agent_kind: None,
            agent_capabilities: Vec::new(),
            updated_at: None,
            profile_version: None,
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
        if let Some(agent_kind) = self.agent_kind.as_ref() {
            value.insert(
                "agent_kind".to_string(),
                serde_json::Value::String(agent_kind.clone()),
            );
        }
        if !self.agent_capabilities.is_empty() {
            value.insert(
                "agent_capabilities".to_string(),
                serde_json::json!(self.agent_capabilities),
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
        if let Some(profile_version) = self.profile_version.as_ref() {
            value.insert(
                "profile_version".to_string(),
                serde_json::Value::String(profile_version.clone()),
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
