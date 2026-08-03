//! Authenticated metadata and kind separation for SecretVault records.
//!
//! Secret kinds are local storage labels. They are never protocol capabilities
//! or wire fields; Join session tokens use a dedicated kind so cancellation and
//! expiry can delete them without confusing them with long-term device keys.

use super::policy::SecretAccessPolicy;
use serde::{Deserialize, Serialize};
use std::fmt;

pub(crate) const VAULT_RECORD_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretKind {
    IdentityRootPrivate,
    IdentityDeviceSigningPrivate,
    IdentityE2eeSigningPrivate,
    IdentityE2eeAgreementPrivate,
    IdentityJoinPairingPrivate,
    IdentityJoinSessionToken,
    /// Encrypted, local-only crash recovery state for promotion after Join.
    /// This label is never serialized into ANP or first-party wire requests.
    IdentityJoinActivationPending,
    /// Encrypted, local-only crash recovery state for an in-flight register.
    /// This label is never serialized into ANP or first-party wire requests.
    #[serde(alias = "identity_genesis_pending")]
    IdentityRegistrationPending,
    /// Encrypted exact-retry state for Manifest Handle Recovery. The record
    /// may contain a short-lived Recovery grant and newly generated keys.
    IdentityHandleRecoveryPending,
    /// Encrypted exact-retry state for the one-device Legacy promotion.
    IdentityLegacyUpgradePending,
    /// Canonical 48-byte root PKCS#8 DER awaiting remote completion and exact
    /// Registry confirmation. Active key providers must never resolve it.
    IdentityRootImportPending,
    /// Encrypted access token awaiting local-only auth-state convergence.
    IdentityAuthCommitPending,
    /// Encrypted exact-retry intent for one permanent device revocation.
    /// This is AWiki-local state and never appears in ANP or DID Documents.
    IdentityDeviceRevokePending,
    IdentityDaemonPrivate,
    AuthJwt,
    DirectE2eeSignedPrekeyPrivate,
    DirectE2eeOneTimePrekeyPrivate,
    DirectE2eeSessionState,
    /// P5 v2 state is intentionally not compatible with the legacy P5 store.
    DirectE2eeV2SignedPrekeyPrivate,
    DirectE2eeV2OneTimePrekeyPrivate,
    DirectE2eeV2SessionState,
    DirectE2eeV2PendingOutbound,
    /// Vault-only full attachment Manifest retained for exact P5 fan-out retry.
    /// The SQLite delivery ledger stores only this record's opaque reference.
    DirectE2eeV2AttachmentManifest,
    GroupMlsState,
    RuntimeSecret,
}

impl SecretKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::IdentityRootPrivate => "identity.root.private",
            Self::IdentityDeviceSigningPrivate => "identity.device.signing.private",
            Self::IdentityE2eeSigningPrivate => "identity.e2ee.signing.private",
            Self::IdentityE2eeAgreementPrivate => "identity.e2ee.agreement.private",
            Self::IdentityJoinPairingPrivate => "identity.join.pairing.private",
            Self::IdentityJoinSessionToken => "identity.join.session.token",
            Self::IdentityJoinActivationPending => "identity.join.activation.pending",
            Self::IdentityRegistrationPending => "identity.registration.pending",
            Self::IdentityHandleRecoveryPending => "identity.handle_recovery.pending",
            Self::IdentityLegacyUpgradePending => "identity.legacy_upgrade.pending",
            Self::IdentityRootImportPending => "identity.root_import.pending",
            Self::IdentityAuthCommitPending => "identity.auth_commit.pending",
            Self::IdentityDeviceRevokePending => "identity.device.revoke.pending",
            Self::IdentityDaemonPrivate => "identity.daemon.private",
            Self::AuthJwt => "auth.jwt",
            Self::DirectE2eeSignedPrekeyPrivate => "direct_e2ee.signed_prekey.private",
            Self::DirectE2eeOneTimePrekeyPrivate => "direct_e2ee.one_time_prekey.private",
            Self::DirectE2eeSessionState => "direct_e2ee.session_state",
            Self::DirectE2eeV2SignedPrekeyPrivate => "direct_e2ee.v2.signed_prekey.private",
            Self::DirectE2eeV2OneTimePrekeyPrivate => "direct_e2ee.v2.one_time_prekey.private",
            Self::DirectE2eeV2SessionState => "direct_e2ee.v2.session_state",
            Self::DirectE2eeV2PendingOutbound => "direct_e2ee.v2.pending_outbound",
            Self::DirectE2eeV2AttachmentManifest => "direct_e2ee.v2.attachment_manifest",
            Self::GroupMlsState => "group_mls.state",
            Self::RuntimeSecret => "runtime.secret",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum VaultCipher {
    ChaCha20Poly1305,
}

impl VaultCipher {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::ChaCha20Poly1305 => "chacha20-poly1305",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum VaultKdf {
    HkdfSha256,
}

impl VaultKdf {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::HkdfSha256 => "hkdf-sha256",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretMetadata {
    pub workspace_id: String,
    pub device_id: String,
    pub identity_id: Option<String>,
    pub did: Option<String>,
    pub kind: SecretKind,
    pub key_id: String,
    pub key_version: u32,
    pub policy: SecretAccessPolicy,
}

impl SecretMetadata {
    pub fn secret_ref(&self) -> SecretRef {
        SecretRef {
            workspace_id: self.workspace_id.clone(),
            device_id: self.device_id.clone(),
            identity_id: self.identity_id.clone(),
            did: self.did.clone(),
            kind: self.kind.clone(),
            key_id: self.key_id.clone(),
            key_version: self.key_version,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretRef {
    pub workspace_id: String,
    pub device_id: String,
    pub identity_id: Option<String>,
    pub did: Option<String>,
    pub kind: SecretKind,
    pub key_id: String,
    pub key_version: u32,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct VaultSecretRecord {
    pub(crate) schema_version: u32,
    pub(crate) workspace_id: String,
    pub(crate) device_id: String,
    pub(crate) identity_id: Option<String>,
    pub(crate) did: Option<String>,
    pub(crate) kind: SecretKind,
    pub(crate) key_id: String,
    pub(crate) key_version: u32,
    pub(crate) cipher: VaultCipher,
    pub(crate) kdf: VaultKdf,
    pub(crate) nonce_b64u: String,
    pub(crate) aad_b64u: String,
    pub(crate) ciphertext_b64u: String,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    pub(crate) policy: SecretAccessPolicy,
}

impl VaultSecretRecord {
    pub(crate) fn metadata(&self) -> SecretMetadata {
        SecretMetadata {
            workspace_id: self.workspace_id.clone(),
            device_id: self.device_id.clone(),
            identity_id: self.identity_id.clone(),
            did: self.did.clone(),
            kind: self.kind.clone(),
            key_id: self.key_id.clone(),
            key_version: self.key_version,
            policy: self.policy.clone(),
        }
    }

    pub(crate) fn secret_ref(&self) -> SecretRef {
        self.metadata().secret_ref()
    }
}

impl fmt::Debug for VaultSecretRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VaultSecretRecord")
            .field("schema_version", &self.schema_version)
            .field("workspace_id", &self.workspace_id)
            .field("device_id", &self.device_id)
            .field("identity_id", &self.identity_id)
            .field("did", &self.did)
            .field("kind", &self.kind)
            .field("key_id", &self.key_id)
            .field("key_version", &self.key_version)
            .field("cipher", &self.cipher)
            .field("kdf", &self.kdf)
            .field("nonce", &"[REDACTED]")
            .field("aad", &"[REDACTED]")
            .field("ciphertext", &"[REDACTED]")
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .field("policy", &self.policy)
            .finish()
    }
}
