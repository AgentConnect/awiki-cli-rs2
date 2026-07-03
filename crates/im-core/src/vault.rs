//! No-prompt local secret vault API.
//!
//! This module exposes the narrow vault surface that other AWiki crates need to
//! seal and open local key material. The encrypted record format and crypto
//! implementation remain internal to `im-core`.

pub use crate::internal::platform_secret::{
    DeviceVaultRootKey, SecretBytes, DEVICE_VAULT_ROOT_KEY_LEN,
};
pub use crate::internal::secret_vault::{
    FileSecretVault, FileSecretVaultStore, SealSecretRequest, SecretAccessPolicy, SecretKind,
    SecretMetadata, SecretRef, SecretVault,
};
