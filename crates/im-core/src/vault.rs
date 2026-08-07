//! No-prompt local secret vault API.
//!
//! This module exposes the narrow vault surface that other AWiki crates need to
//! seal and open local key material. The encrypted record format and crypto
//! implementation remain internal to `im-core`.

use base64::{engine::general_purpose, Engine as _};

pub use crate::internal::platform_secret::{
    DeviceVaultRootKey, SecretBytes, DEVICE_VAULT_ROOT_KEY_LEN,
};
pub use crate::internal::secret_vault::{
    FileSecretVault, FileSecretVaultStore, SealIfAbsentResult, SealSecretRequest,
    SecretAccessPolicy, SecretKind, SecretMetadata, SecretRef, SecretVault,
};

pub fn encode_delegated_key_ref(secret_ref: &SecretRef) -> crate::ImResult<String> {
    crate::internal::delegated_identity::encode_vault_key_ref_public(secret_ref)
}

pub const IM_CORE_VAULT_ROOT_KEY_ENV: &str = "AWIKI_IM_CORE_VAULT_ROOT_KEY_B64";

pub fn im_core_vault_root_key_from_env() -> crate::ImResult<DeviceVaultRootKey> {
    device_vault_root_key_from_env(IM_CORE_VAULT_ROOT_KEY_ENV)
}

pub fn device_vault_root_key_from_env(source_name: &str) -> crate::ImResult<DeviceVaultRootKey> {
    let raw = std::env::var(source_name).map_err(|_| crate::ImError::LocalStateUnavailable {
        detail: format!("{source_name} is required for im-core secret vault"),
    })?;
    parse_device_vault_root_key_b64(&raw, source_name)
}

pub fn parse_device_vault_root_key_b64(
    raw: &str,
    source_name: &str,
) -> crate::ImResult<DeviceVaultRootKey> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(crate::ImError::LocalStateUnavailable {
            detail: format!("{source_name} must not be empty"),
        });
    }
    let bytes = general_purpose::URL_SAFE_NO_PAD
        .decode(trimmed)
        .or_else(|_| general_purpose::STANDARD.decode(trimmed))
        .map_err(|_| crate::ImError::Serialization {
            detail: format!(
                "{source_name} must be base64url/base64 encoded {DEVICE_VAULT_ROOT_KEY_LEN}-byte key"
            ),
        })?;
    let bytes: [u8; DEVICE_VAULT_ROOT_KEY_LEN] =
        bytes
            .try_into()
            .map_err(|_| crate::ImError::Serialization {
                detail: format!(
                    "{source_name} must decode to exactly {DEVICE_VAULT_ROOT_KEY_LEN} bytes"
                ),
            })?;
    Ok(DeviceVaultRootKey::from_bytes(bytes))
}
