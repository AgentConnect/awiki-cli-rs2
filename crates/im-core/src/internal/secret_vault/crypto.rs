use super::record::{
    SecretMetadata, VaultCipher, VaultKdf, VaultSecretRecord, VAULT_RECORD_SCHEMA_VERSION,
};
use crate::internal::platform_secret::{DeviceVaultRootKey, SecretBytes};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use hkdf::Hkdf;
use rand::rngs::OsRng;
use rand::RngCore;
use sha2::Sha256;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

const VAULT_NONCE_LEN: usize = 12;
const VAULT_RECORD_HKDF_SALT: &[u8] = b"awiki:vault:hkdf-salt:v1";

pub(crate) fn seal_record(
    root_key: &DeviceVaultRootKey,
    metadata: SecretMetadata,
    plaintext: &SecretBytes,
) -> crate::ImResult<VaultSecretRecord> {
    metadata.policy.validate_no_prompt()?;
    validate_metadata(&metadata)?;
    let now = now_rfc3339();
    let aad = aad_for_metadata(
        VAULT_RECORD_SCHEMA_VERSION,
        &metadata,
        &VaultCipher::ChaCha20Poly1305,
        &VaultKdf::HkdfSha256,
    );
    let record_key = derive_record_key(root_key, &aad)?;
    let mut nonce = [0_u8; VAULT_NONCE_LEN];
    OsRng.fill_bytes(&mut nonce);
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&record_key));
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: plaintext.expose_secret(),
                aad: &aad,
            },
        )
        .map_err(|_| crate::ImError::Internal {
            message: "secret vault encryption failed".to_owned(),
        })?;
    Ok(VaultSecretRecord {
        schema_version: VAULT_RECORD_SCHEMA_VERSION,
        workspace_id: metadata.workspace_id,
        device_id: metadata.device_id,
        identity_id: metadata.identity_id,
        did: metadata.did,
        kind: metadata.kind,
        key_id: metadata.key_id,
        key_version: metadata.key_version,
        cipher: VaultCipher::ChaCha20Poly1305,
        kdf: VaultKdf::HkdfSha256,
        nonce_b64u: URL_SAFE_NO_PAD.encode(nonce),
        aad_b64u: URL_SAFE_NO_PAD.encode(aad),
        ciphertext_b64u: URL_SAFE_NO_PAD.encode(ciphertext),
        created_at: now.clone(),
        updated_at: now,
        policy: metadata.policy,
    })
}

pub(crate) fn open_record(
    root_key: &DeviceVaultRootKey,
    record: &VaultSecretRecord,
) -> crate::ImResult<SecretBytes> {
    if record.schema_version != VAULT_RECORD_SCHEMA_VERSION {
        return Err(crate::ImError::Serialization {
            detail: "unsupported secret vault record schema version".to_owned(),
        });
    }
    if record.cipher != VaultCipher::ChaCha20Poly1305 {
        return Err(crate::ImError::unsupported(format!(
            "unsupported vault cipher {}",
            record.cipher.as_str()
        )));
    }
    if record.kdf != VaultKdf::HkdfSha256 {
        return Err(crate::ImError::unsupported(format!(
            "unsupported vault kdf {}",
            record.kdf.as_str()
        )));
    }
    record.policy.validate_no_prompt()?;
    let metadata = record.metadata();
    validate_metadata(&metadata)?;
    let aad = aad_for_metadata(
        record.schema_version,
        &metadata,
        &record.cipher,
        &record.kdf,
    );
    let stored_aad = decode_b64u("vault_record_aad", &record.aad_b64u)?;
    if stored_aad != aad {
        return Err(vault_integrity_error());
    }
    let nonce = decode_fixed_b64u("vault_record_nonce", &record.nonce_b64u, VAULT_NONCE_LEN)?;
    let ciphertext = decode_b64u("vault_record_ciphertext", &record.ciphertext_b64u)?;
    let record_key = derive_record_key(root_key, &aad)?;
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&record_key));
    let plaintext = cipher
        .decrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: &ciphertext,
                aad: &aad,
            },
        )
        .map_err(|_| vault_integrity_error())?;
    Ok(SecretBytes::from_vec(plaintext))
}

pub(crate) fn aad_for_record(record: &VaultSecretRecord) -> Vec<u8> {
    aad_for_metadata(
        record.schema_version,
        &record.metadata(),
        &record.cipher,
        &record.kdf,
    )
}

fn aad_for_metadata(
    schema_version: u32,
    metadata: &SecretMetadata,
    cipher: &VaultCipher,
    kdf: &VaultKdf,
) -> Vec<u8> {
    let mut aad = Vec::new();
    push_field(&mut aad, "domain", "awiki:vault:v1");
    push_field(&mut aad, "schema_version", &schema_version.to_string());
    push_field(&mut aad, "workspace_id", &metadata.workspace_id);
    push_field(&mut aad, "device_id", &metadata.device_id);
    push_optional_field(&mut aad, "identity_id", metadata.identity_id.as_deref());
    push_optional_field(&mut aad, "did", metadata.did.as_deref());
    push_field(&mut aad, "kind", metadata.kind.as_str());
    push_field(&mut aad, "key_id", &metadata.key_id);
    push_field(&mut aad, "key_version", &metadata.key_version.to_string());
    push_field(&mut aad, "cipher", cipher.as_str());
    push_field(&mut aad, "kdf", kdf.as_str());
    push_field(
        &mut aad,
        "policy_no_prompt",
        if metadata.policy.no_prompt {
            "true"
        } else {
            "false"
        },
    );
    push_field(
        &mut aad,
        "policy_user_presence_required",
        if metadata.policy.user_presence_required {
            "true"
        } else {
            "false"
        },
    );
    push_field(
        &mut aad,
        "policy_exportable",
        if metadata.policy.exportable {
            "true"
        } else {
            "false"
        },
    );
    push_optional_field(
        &mut aad,
        "policy_cache_ttl_seconds",
        metadata
            .policy
            .cache_ttl_seconds
            .map(|ttl| ttl.to_string())
            .as_deref(),
    );
    aad
}

fn push_optional_field(out: &mut Vec<u8>, name: &str, value: Option<&str>) {
    match value {
        Some(value) => push_field(out, name, value),
        None => push_field(out, name, "<none>"),
    }
}

fn push_field(out: &mut Vec<u8>, name: &str, value: &str) {
    out.extend_from_slice(name.as_bytes());
    out.push(0);
    out.extend_from_slice(value.len().to_string().as_bytes());
    out.push(0);
    out.extend_from_slice(value.as_bytes());
    out.push(0xff);
}

fn derive_record_key(root_key: &DeviceVaultRootKey, aad: &[u8]) -> crate::ImResult<[u8; 32]> {
    let hkdf = Hkdf::<Sha256>::new(Some(VAULT_RECORD_HKDF_SALT), root_key.expose_secret());
    let mut key = [0_u8; 32];
    hkdf.expand(aad, &mut key)
        .map_err(|_| crate::ImError::Internal {
            message: "derive secret vault record key failed".to_owned(),
        })?;
    Ok(key)
}

fn validate_metadata(metadata: &SecretMetadata) -> crate::ImResult<()> {
    validate_required("workspace_id", &metadata.workspace_id)?;
    validate_required("device_id", &metadata.device_id)?;
    validate_required("key_id", &metadata.key_id)?;
    if metadata.key_version == 0 {
        return Err(crate::ImError::invalid_input(
            Some("key_version".to_owned()),
            "key_version must be greater than zero",
        ));
    }
    Ok(())
}

fn validate_required(field: &str, value: &str) -> crate::ImResult<()> {
    if value.trim().is_empty() {
        return Err(crate::ImError::invalid_input(
            Some(field.to_owned()),
            format!("{field} is required"),
        ));
    }
    Ok(())
}

fn decode_b64u(field: &str, value: &str) -> crate::ImResult<Vec<u8>> {
    URL_SAFE_NO_PAD
        .decode(value.trim())
        .map_err(|_| crate::ImError::Serialization {
            detail: format!("{field} must be base64url without padding"),
        })
}

fn decode_fixed_b64u(field: &str, value: &str, expected_len: usize) -> crate::ImResult<Vec<u8>> {
    let decoded = decode_b64u(field, value)?;
    if decoded.len() != expected_len {
        return Err(crate::ImError::Serialization {
            detail: format!("{field} must decode to {expected_len} bytes"),
        });
    }
    Ok(decoded)
}

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
}

fn vault_integrity_error() -> crate::ImError {
    crate::ImError::PermissionDenied
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::internal::secret_vault::policy::SecretAccessPolicy;
    use crate::internal::secret_vault::record::SecretKind;

    #[test]
    fn secret_vault_crypto_roundtrips_and_uses_bound_aad() {
        let root_key = DeviceVaultRootKey::from_bytes([1_u8; 32]);
        let metadata = test_metadata("workspace-a", "device-a");
        let plaintext = SecretBytes::from_vec(b"identity-private-key".to_vec());

        let record = seal_record(&root_key, metadata, &plaintext).unwrap();
        let opened = open_record(&root_key, &record).unwrap();

        assert_eq!(opened.expose_secret(), b"identity-private-key");
        assert_eq!(record.schema_version, VAULT_RECORD_SCHEMA_VERSION);
        assert_ne!(record.ciphertext_b64u, "identity-private-key");
        assert_eq!(
            record.aad_b64u,
            URL_SAFE_NO_PAD.encode(aad_for_record(&record))
        );
    }

    #[test]
    fn secret_vault_crypto_rejects_aad_tamper() {
        let root_key = DeviceVaultRootKey::from_bytes([1_u8; 32]);
        let metadata = test_metadata("workspace-a", "device-a");
        let plaintext = SecretBytes::from_vec(b"identity-private-key".to_vec());
        let mut record = seal_record(&root_key, metadata, &plaintext).unwrap();

        record.workspace_id = "workspace-b".to_owned();
        let err = open_record(&root_key, &record).unwrap_err();

        assert_eq!(err, crate::ImError::PermissionDenied);
    }

    #[test]
    fn secret_vault_crypto_rejects_wrong_root_key() {
        let root_key = DeviceVaultRootKey::from_bytes([1_u8; 32]);
        let wrong_root_key = DeviceVaultRootKey::from_bytes([2_u8; 32]);
        let metadata = test_metadata("workspace-a", "device-a");
        let plaintext = SecretBytes::from_vec(b"identity-private-key".to_vec());
        let record = seal_record(&root_key, metadata, &plaintext).unwrap();

        let err = open_record(&wrong_root_key, &record).unwrap_err();

        assert_eq!(err, crate::ImError::PermissionDenied);
    }

    #[test]
    fn secret_vault_crypto_rejects_user_presence_policy() {
        let root_key = DeviceVaultRootKey::from_bytes([1_u8; 32]);
        let mut metadata = test_metadata("workspace-a", "device-a");
        metadata.policy.user_presence_required = true;
        let plaintext = SecretBytes::from_vec(b"identity-private-key".to_vec());

        let err = seal_record(&root_key, metadata, &plaintext).unwrap_err();

        assert!(matches!(err, crate::ImError::UnsupportedCapability { .. }));
    }

    fn test_metadata(workspace_id: &str, device_id: &str) -> SecretMetadata {
        SecretMetadata {
            workspace_id: workspace_id.to_owned(),
            device_id: device_id.to_owned(),
            identity_id: Some("identity-a".to_owned()),
            did: Some("did:wba:alice@example.com".to_owned()),
            kind: SecretKind::IdentityRootPrivate,
            key_id: "key-1".to_owned(),
            key_version: 1,
            policy: SecretAccessPolicy::no_prompt_local_secret(),
        }
    }
}
