use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::vault::{
    DeviceVaultRootKey, FileSecretVault, FileSecretVaultStore, SealSecretRequest,
    SecretAccessPolicy, SecretBytes, SecretKind, SecretMetadata, SecretRef, SecretVault,
};

const DIRECT_SECRET_ENVELOPE_PREFIX: &[u8] = b"awiki-direct-secret-envelope-v1\n";
const DIRECT_SECRET_ENVELOPE_SCHEMA_VERSION: u32 = 1;
#[cfg(test)]
use crate::vault::DEVICE_VAULT_ROOT_KEY_LEN;
pub(crate) use crate::vault::IM_CORE_VAULT_ROOT_KEY_ENV;

pub(crate) type DirectSecretVault = Arc<dyn SecretVault + Send + Sync>;

pub(crate) struct DirectSecretSealInput<'a> {
    pub(crate) owner_identity_id: &'a str,
    pub(crate) owner_did: &'a str,
    pub(crate) kind: SecretKind,
    pub(crate) key_id: String,
    pub(crate) plaintext: &'a [u8],
    pub(crate) field: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct DirectSecretEnvelopeV1 {
    schema_version: u32,
    secret_ref: SecretRef,
}

pub(crate) fn direct_secret_vault_from_env(
    vault_dir: PathBuf,
) -> crate::ImResult<Option<DirectSecretVault>> {
    if std::env::var_os(IM_CORE_VAULT_ROOT_KEY_ENV).is_none() {
        return Ok(None);
    }
    let root_key = im_core_vault_root_key_from_env()?;
    Ok(Some(Arc::new(FileSecretVault::new(
        root_key,
        FileSecretVaultStore::new(vault_dir),
    ))))
}

pub(crate) fn default_direct_secret_vault(
    vault_dir: PathBuf,
) -> crate::ImResult<Option<DirectSecretVault>> {
    let secret_vault = direct_secret_vault_from_env(vault_dir)?;
    #[cfg(test)]
    let secret_vault = secret_vault.or_else(|| Some(test_direct_secret_vault()));
    Ok(secret_vault)
}

pub(crate) fn seal_direct_secret_blob(
    vault: Option<&DirectSecretVault>,
    input: DirectSecretSealInput<'_>,
) -> crate::ImResult<Vec<u8>> {
    if is_direct_secret_envelope(input.plaintext) {
        return Err(crate::ImError::Serialization {
            detail: format!("{} is already a direct secret envelope", input.field),
        });
    }
    let vault = vault.ok_or_else(|| crate::ImError::LocalStateUnavailable {
        detail: format!(
            "{field} requires {env}; refusing plaintext fallback",
            field = input.field,
            env = IM_CORE_VAULT_ROOT_KEY_ENV
        ),
    })?;
    let secret_ref = vault.seal(SealSecretRequest {
        metadata: SecretMetadata {
            workspace_id: "awiki-im-core".to_owned(),
            device_id: "local-device".to_owned(),
            identity_id: non_empty_owned(input.owner_identity_id),
            did: non_empty_owned(input.owner_did),
            kind: input.kind,
            key_id: input.key_id,
            key_version: 1,
            policy: SecretAccessPolicy::no_prompt_local_secret(),
        },
        plaintext: SecretBytes::from_vec(input.plaintext.to_vec()),
    })?;
    let opened = vault.open(&secret_ref)?;
    if opened.expose_secret() != input.plaintext {
        return Err(crate::ImError::Internal {
            message: format!("{} secret vault verification failed", input.field),
        });
    }
    direct_secret_envelope_to_blob(&DirectSecretEnvelopeV1 {
        schema_version: DIRECT_SECRET_ENVELOPE_SCHEMA_VERSION,
        secret_ref,
    })
}

pub(crate) fn open_direct_secret_blob(
    vault: Option<&DirectSecretVault>,
    blob: Vec<u8>,
    field: &str,
) -> crate::ImResult<Vec<u8>> {
    let Some(envelope) = direct_secret_envelope_from_blob(&blob)? else {
        return Ok(blob);
    };
    let vault = vault.ok_or_else(|| crate::ImError::LocalStateUnavailable {
        detail: format!(
            "{field} requires {IM_CORE_VAULT_ROOT_KEY_ENV} to open direct secret envelope"
        ),
    })?;
    Ok(vault.open(&envelope.secret_ref)?.expose_secret().to_vec())
}

fn direct_secret_envelope_to_blob(envelope: &DirectSecretEnvelopeV1) -> crate::ImResult<Vec<u8>> {
    let mut blob = DIRECT_SECRET_ENVELOPE_PREFIX.to_vec();
    let body = serde_json::to_vec(envelope).map_err(|err| crate::ImError::Serialization {
        detail: format!("serialize direct secret envelope: {err}"),
    })?;
    blob.extend_from_slice(&body);
    Ok(blob)
}

fn direct_secret_envelope_from_blob(
    blob: &[u8],
) -> crate::ImResult<Option<DirectSecretEnvelopeV1>> {
    if !is_direct_secret_envelope(blob) {
        return Ok(None);
    }
    let body = &blob[DIRECT_SECRET_ENVELOPE_PREFIX.len()..];
    let envelope: DirectSecretEnvelopeV1 =
        serde_json::from_slice(body).map_err(|err| crate::ImError::Serialization {
            detail: format!("parse direct secret envelope: {err}"),
        })?;
    if envelope.schema_version != DIRECT_SECRET_ENVELOPE_SCHEMA_VERSION {
        return Err(crate::ImError::Serialization {
            detail: "unsupported direct secret envelope schema version".to_owned(),
        });
    }
    Ok(Some(envelope))
}

pub(crate) fn is_direct_secret_envelope(blob: &[u8]) -> bool {
    blob.starts_with(DIRECT_SECRET_ENVELOPE_PREFIX)
}

pub(crate) fn direct_secret_key_id(
    owner_identity_id: &str,
    category: &str,
    id: &str,
    suffix: &str,
) -> String {
    format!(
        "direct-e2ee/{}/{}/{}/{}/{}",
        sanitize_secret_key_part(owner_identity_id),
        sanitize_secret_key_part(category),
        sanitize_secret_key_part(id),
        sanitize_secret_key_part(suffix),
        vault_secret_nonce_hex()
    )
}

pub(crate) fn im_core_vault_root_key_from_env() -> crate::ImResult<DeviceVaultRootKey> {
    crate::vault::im_core_vault_root_key_from_env()
}

fn vault_secret_nonce_hex() -> String {
    use rand::RngCore;

    let mut bytes = [0_u8; 8];
    rand::thread_rng().fill_bytes(&mut bytes);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let random = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("{nanos:x}-{random}")
}

fn sanitize_secret_key_part(value: &str) -> String {
    value
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | ':') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn non_empty_owned(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_owned())
    }
}

#[cfg(test)]
pub(crate) fn test_direct_secret_vault() -> DirectSecretVault {
    Arc::new(FileSecretVault::new(
        DeviceVaultRootKey::from_bytes([17_u8; DEVICE_VAULT_ROOT_KEY_LEN]),
        FileSecretVaultStore::new(test_direct_secret_vault_dir()),
    ))
}

#[cfg(test)]
fn test_direct_secret_vault_dir() -> PathBuf {
    static TEST_DIRECT_SECRET_VAULT_DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

    TEST_DIRECT_SECRET_VAULT_DIR
        .get_or_init(|| {
            std::env::temp_dir().join(format!(
                "awiki-im-core-direct-vault-test-{}",
                std::process::id()
            ))
        })
        .clone()
}
