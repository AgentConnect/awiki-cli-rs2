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
    /// Encrypted, local-only crash recovery state for an in-flight Genesis.
    /// This label is never serialized into ANP or first-party wire requests.
    IdentityGenesisPending,
    IdentityDaemonPrivate,
    AuthJwt,
    DirectE2eeSignedPrekeyPrivate,
    DirectE2eeOneTimePrekeyPrivate,
    DirectE2eeSessionState,
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
            Self::IdentityGenesisPending => "identity.genesis.pending",
            Self::IdentityDaemonPrivate => "identity.daemon.private",
            Self::AuthJwt => "auth.jwt",
            Self::DirectE2eeSignedPrekeyPrivate => "direct_e2ee.signed_prekey.private",
            Self::DirectE2eeOneTimePrekeyPrivate => "direct_e2ee.one_time_prekey.private",
            Self::DirectE2eeSessionState => "direct_e2ee.session_state",
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
