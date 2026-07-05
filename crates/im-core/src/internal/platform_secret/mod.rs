use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
#[cfg(test)]
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
#[cfg(test)]
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::fmt;
use zeroize::Zeroize;

pub const DEVICE_VAULT_ROOT_KEY_LEN: usize = 32;
const PLATFORM_PROTECTED_SECRET_SCHEMA_VERSION: u32 = 1;
const MEMORY_PROTECTOR_NONCE_LEN: usize = 12;

pub struct SecretBytes {
    inner: Vec<u8>,
}

impl SecretBytes {
    pub fn from_vec(inner: Vec<u8>) -> Self {
        Self { inner }
    }

    pub fn expose_secret(&self) -> &[u8] {
        &self.inner
    }
}

impl Drop for SecretBytes {
    fn drop(&mut self) {
        self.inner.zeroize();
    }
}

impl fmt::Debug for SecretBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecretBytes")
            .field("len", &self.inner.len())
            .field("value", &"[REDACTED]")
            .finish()
    }
}

pub struct DeviceVaultRootKey {
    bytes: [u8; DEVICE_VAULT_ROOT_KEY_LEN],
}

impl DeviceVaultRootKey {
    pub fn generate() -> Self {
        let mut bytes = [0_u8; DEVICE_VAULT_ROOT_KEY_LEN];
        OsRng.fill_bytes(&mut bytes);
        Self { bytes }
    }

    pub fn from_bytes(bytes: [u8; DEVICE_VAULT_ROOT_KEY_LEN]) -> Self {
        Self { bytes }
    }

    pub fn try_from_secret(secret: &SecretBytes) -> crate::ImResult<Self> {
        let bytes = secret.expose_secret();
        if bytes.len() != DEVICE_VAULT_ROOT_KEY_LEN {
            return Err(crate::ImError::Serialization {
                detail: format!("device vault root key must be {DEVICE_VAULT_ROOT_KEY_LEN} bytes"),
            });
        }
        let mut out = [0_u8; DEVICE_VAULT_ROOT_KEY_LEN];
        out.copy_from_slice(bytes);
        Ok(Self { bytes: out })
    }

    pub(crate) fn expose_secret(&self) -> &[u8; DEVICE_VAULT_ROOT_KEY_LEN] {
        &self.bytes
    }
}

impl Drop for DeviceVaultRootKey {
    fn drop(&mut self) {
        self.bytes.zeroize();
    }
}

impl fmt::Debug for DeviceVaultRootKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeviceVaultRootKey")
            .field("len", &DEVICE_VAULT_ROOT_KEY_LEN)
            .field("value", &"[REDACTED]")
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PlatformSecretLabel {
    pub(crate) workspace_id: String,
    pub(crate) device_id: String,
    pub(crate) version: u32,
}

impl PlatformSecretLabel {
    pub(crate) fn vault_root_key(
        workspace_id: impl Into<String>,
        device_id: impl Into<String>,
    ) -> Self {
        Self {
            workspace_id: workspace_id.into(),
            device_id: device_id.into(),
            version: 1,
        }
    }

    pub(crate) fn item_name(&self) -> String {
        format!(
            "awiki.vault.{}.{}.v{}",
            self.workspace_id, self.device_id, self.version
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum PlatformProtectionScope {
    CurrentUser,
    CurrentLoginSession,
    OperatorProvided,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PlatformProtectionPolicy {
    pub(crate) no_prompt: bool,
    pub(crate) user_presence_required: bool,
    pub(crate) scope: PlatformProtectionScope,
}

impl PlatformProtectionPolicy {
    pub(crate) fn no_prompt_current_user() -> Self {
        Self {
            no_prompt: true,
            user_presence_required: false,
            scope: PlatformProtectionScope::CurrentUser,
        }
    }

    pub(crate) fn validate_no_prompt(&self) -> crate::ImResult<()> {
        if !self.no_prompt || self.user_presence_required {
            return Err(crate::ImError::unsupported(
                "platform secret protector requires user presence; awiki vault unlock must be no-prompt",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ProtectedSecret {
    pub(crate) schema_version: u32,
    pub(crate) label: String,
    pub(crate) protector_kind: String,
    pub(crate) policy: PlatformProtectionPolicy,
    pub(crate) protected_blob_b64u: String,
}

impl ProtectedSecret {
    fn new(
        label: String,
        protector_kind: impl Into<String>,
        policy: PlatformProtectionPolicy,
        protected_blob: &[u8],
    ) -> Self {
        Self {
            schema_version: PLATFORM_PROTECTED_SECRET_SCHEMA_VERSION,
            label,
            protector_kind: protector_kind.into(),
            policy,
            protected_blob_b64u: URL_SAFE_NO_PAD.encode(protected_blob),
        }
    }

    fn protected_blob(&self) -> crate::ImResult<Vec<u8>> {
        URL_SAFE_NO_PAD
            .decode(self.protected_blob_b64u.trim())
            .map_err(|_| crate::ImError::Serialization {
                detail: "protected platform secret blob must be base64url without padding"
                    .to_owned(),
            })
    }
}

impl fmt::Debug for ProtectedSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProtectedSecret")
            .field("schema_version", &self.schema_version)
            .field("label", &self.label)
            .field("protector_kind", &self.protector_kind)
            .field("policy", &self.policy)
            .field("protected_blob", &"[REDACTED]")
            .finish()
    }
}

pub(crate) trait PlatformProtector: Send + Sync {
    fn protector_kind(&self) -> &'static str;

    fn policy(&self) -> PlatformProtectionPolicy;

    fn protect(
        &self,
        label: &PlatformSecretLabel,
        plaintext: &SecretBytes,
    ) -> crate::ImResult<ProtectedSecret>;

    fn unprotect(
        &self,
        label: &PlatformSecretLabel,
        protected: &ProtectedSecret,
    ) -> crate::ImResult<SecretBytes>;

    fn delete(&self, label: &PlatformSecretLabel) -> crate::ImResult<()>;
}

#[derive(Debug, Clone)]
pub(crate) struct UnavailablePlatformProtector {
    reason: String,
}

impl UnavailablePlatformProtector {
    pub(crate) fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }

    fn unavailable(&self) -> crate::ImError {
        crate::ImError::unsupported(format!(
            "no-prompt platform secret backend unavailable: {}",
            self.reason
        ))
    }
}

impl PlatformProtector for UnavailablePlatformProtector {
    fn protector_kind(&self) -> &'static str {
        "unavailable"
    }

    fn policy(&self) -> PlatformProtectionPolicy {
        PlatformProtectionPolicy::no_prompt_current_user()
    }

    fn protect(
        &self,
        _label: &PlatformSecretLabel,
        _plaintext: &SecretBytes,
    ) -> crate::ImResult<ProtectedSecret> {
        Err(self.unavailable())
    }

    fn unprotect(
        &self,
        _label: &PlatformSecretLabel,
        _protected: &ProtectedSecret,
    ) -> crate::ImResult<SecretBytes> {
        Err(self.unavailable())
    }

    fn delete(&self, _label: &PlatformSecretLabel) -> crate::ImResult<()> {
        Err(self.unavailable())
    }
}

pub(crate) fn default_platform_protector() -> UnavailablePlatformProtector {
    UnavailablePlatformProtector::new(
        "platform-specific Keychain/Keystore/DPAPI/Secret Service backend is not wired yet",
    )
}

pub(crate) fn protect_device_vault_root_key(
    protector: &dyn PlatformProtector,
    label: &PlatformSecretLabel,
    root_key: &DeviceVaultRootKey,
) -> crate::ImResult<ProtectedSecret> {
    let secret = SecretBytes::from_vec(root_key.expose_secret().to_vec());
    protector.protect(label, &secret)
}

pub(crate) fn unprotect_device_vault_root_key(
    protector: &dyn PlatformProtector,
    label: &PlatformSecretLabel,
    protected: &ProtectedSecret,
) -> crate::ImResult<DeviceVaultRootKey> {
    let secret = protector.unprotect(label, protected)?;
    DeviceVaultRootKey::try_from_secret(&secret)
}

#[cfg(test)]
pub(crate) struct InMemoryPlatformProtector {
    wrapping_key: [u8; DEVICE_VAULT_ROOT_KEY_LEN],
    policy: PlatformProtectionPolicy,
}

#[cfg(test)]
impl InMemoryPlatformProtector {
    pub(crate) fn new_for_tests(wrapping_key: [u8; DEVICE_VAULT_ROOT_KEY_LEN]) -> Self {
        Self {
            wrapping_key,
            policy: PlatformProtectionPolicy::no_prompt_current_user(),
        }
    }

    fn aad(label: &PlatformSecretLabel) -> Vec<u8> {
        format!("awiki:platform-secret:v1:{}", label.item_name()).into_bytes()
    }
}

#[cfg(test)]
impl PlatformProtector for InMemoryPlatformProtector {
    fn protector_kind(&self) -> &'static str {
        "test-memory-aead"
    }

    fn policy(&self) -> PlatformProtectionPolicy {
        self.policy.clone()
    }

    fn protect(
        &self,
        label: &PlatformSecretLabel,
        plaintext: &SecretBytes,
    ) -> crate::ImResult<ProtectedSecret> {
        self.policy.validate_no_prompt()?;
        let mut nonce = [0_u8; MEMORY_PROTECTOR_NONCE_LEN];
        OsRng.fill_bytes(&mut nonce);
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&self.wrapping_key));
        let mut blob = nonce.to_vec();
        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: plaintext.expose_secret(),
                    aad: &Self::aad(label),
                },
            )
            .map_err(|_| crate::ImError::Internal {
                message: "platform secret protection failed".to_owned(),
            })?;
        blob.extend_from_slice(&ciphertext);
        Ok(ProtectedSecret::new(
            label.item_name(),
            self.protector_kind(),
            self.policy.clone(),
            &blob,
        ))
    }

    fn unprotect(
        &self,
        label: &PlatformSecretLabel,
        protected: &ProtectedSecret,
    ) -> crate::ImResult<SecretBytes> {
        self.policy.validate_no_prompt()?;
        if protected.schema_version != PLATFORM_PROTECTED_SECRET_SCHEMA_VERSION {
            return Err(crate::ImError::Serialization {
                detail: "unsupported platform protected secret schema version".to_owned(),
            });
        }
        if protected.label != label.item_name() {
            return Err(crate::ImError::PermissionDenied);
        }
        if protected.protector_kind != self.protector_kind() {
            return Err(crate::ImError::unsupported(format!(
                "unsupported platform secret protector {}",
                protected.protector_kind
            )));
        }
        protected.policy.validate_no_prompt()?;
        let blob = protected.protected_blob()?;
        if blob.len() <= MEMORY_PROTECTOR_NONCE_LEN {
            return Err(crate::ImError::Serialization {
                detail: "platform protected secret blob is too short".to_owned(),
            });
        }
        let (nonce, ciphertext) = blob.split_at(MEMORY_PROTECTOR_NONCE_LEN);
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&self.wrapping_key));
        let plaintext = cipher
            .decrypt(
                Nonce::from_slice(nonce),
                Payload {
                    msg: ciphertext,
                    aad: &Self::aad(label),
                },
            )
            .map_err(|_| crate::ImError::PermissionDenied)?;
        Ok(SecretBytes::from_vec(plaintext))
    }

    fn delete(&self, _label: &PlatformSecretLabel) -> crate::ImResult<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_platform_protector_fails_without_plaintext_fallback() {
        let protector = default_platform_protector();
        let label = PlatformSecretLabel::vault_root_key("workspace", "device");
        let secret = SecretBytes::from_vec(b"leaked-root-key".to_vec());

        let err = protector.protect(&label, &secret).unwrap_err();

        assert!(matches!(err, crate::ImError::UnsupportedCapability { .. }));
        assert!(!format!("{err}").contains("leaked-root-key"));
    }

    #[test]
    fn memory_platform_protector_roundtrips_no_prompt_root_key() {
        let protector = InMemoryPlatformProtector::new_for_tests([7_u8; 32]);
        let label = PlatformSecretLabel::vault_root_key("workspace", "device");
        let root_key = DeviceVaultRootKey::from_bytes([9_u8; 32]);

        let protected = protect_device_vault_root_key(&protector, &label, &root_key).unwrap();
        let opened = unprotect_device_vault_root_key(&protector, &label, &protected).unwrap();

        assert_eq!(opened.expose_secret(), root_key.expose_secret());
        assert!(protected.policy.no_prompt);
        assert!(!protected.policy.user_presence_required);
    }

    #[test]
    fn memory_platform_protector_rejects_wrong_label() {
        let protector = InMemoryPlatformProtector::new_for_tests([7_u8; 32]);
        let label = PlatformSecretLabel::vault_root_key("workspace", "device-a");
        let other_label = PlatformSecretLabel::vault_root_key("workspace", "device-b");
        let root_key = DeviceVaultRootKey::from_bytes([9_u8; 32]);
        let protected = protect_device_vault_root_key(&protector, &label, &root_key).unwrap();

        let err =
            unprotect_device_vault_root_key(&protector, &other_label, &protected).unwrap_err();

        assert_eq!(err, crate::ImError::PermissionDenied);
    }

    #[test]
    fn platform_secret_debug_redacts_secret_material() {
        let protector = InMemoryPlatformProtector::new_for_tests([7_u8; 32]);
        let label = PlatformSecretLabel::vault_root_key("workspace", "device");
        let secret = SecretBytes::from_vec(b"debug-secret-value".to_vec());
        let protected = protector.protect(&label, &secret).unwrap();

        assert!(!format!("{secret:?}").contains("debug-secret-value"));
        assert!(!format!("{protected:?}").contains(&protected.protected_blob_b64u));
        assert!(format!("{protected:?}").contains("[REDACTED]"));
    }
}
