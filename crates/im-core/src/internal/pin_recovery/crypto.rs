use super::{failure, wire};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chacha20poly1305::{
    aead::{Aead, Payload},
    KeyInit, XChaCha20Poly1305, XNonce,
};
use ed25519_dalek::{pkcs8::DecodePrivateKey, Verifier};
use rand::RngCore;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

pub(crate) fn b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}
pub(crate) fn decode(text: &str) -> crate::ImResult<Zeroizing<Vec<u8>>> {
    let decoded = Zeroizing::new(
        URL_SAFE_NO_PAD
            .decode(text)
            .map_err(|_| failure("invalid_encoding", false))?,
    );
    if b64(&decoded) != text {
        return Err(failure("invalid_encoding", false));
    }
    Ok(decoded)
}
pub(crate) fn canonical<T: Serialize>(value: &T) -> crate::ImResult<Vec<u8>> {
    serde_json_canonicalizer::to_vec(value).map_err(|_| failure("invalid_wire", false))
}
pub(crate) fn hash(bytes: &[u8]) -> String {
    b64(&Sha256::digest(bytes))
}
pub(crate) fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
pub(crate) fn random32() -> Zeroizing<[u8; 32]> {
    let mut b = Zeroizing::new([0u8; 32]);
    rand::rngs::OsRng.fill_bytes(b.as_mut());
    b
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JwsHeader {
    typ: String,
    alg: String,
    kid: String,
}
pub(crate) fn verify_jws<T: DeserializeOwned>(
    jws: &str,
    typ: &str,
    keys: &std::collections::BTreeMap<String, String>,
) -> crate::ImResult<T> {
    if jws.len() > 32768 {
        return Err(failure("invalid_signature", false));
    }
    let parts = jws.split('.').collect::<Vec<_>>();
    if parts.len() != 3 {
        return Err(failure("invalid_signature", false));
    }
    let header: JwsHeader = serde_json::from_slice(&decode(parts[0])?)
        .map_err(|_| failure("invalid_signature", false))?;
    if header.typ != typ || header.alg != "EdDSA" {
        return Err(failure("invalid_signature", false));
    }
    let key = decode(
        keys.get(&header.kid)
            .ok_or_else(|| failure("untrusted_signing_key", false))?,
    )?;
    let key: [u8; 32] = key
        .as_slice()
        .try_into()
        .map_err(|_| failure("invalid_signature", false))?;
    let signature = ed25519_dalek::Signature::from_slice(&decode(parts[2])?)
        .map_err(|_| failure("invalid_signature", false))?;
    ed25519_dalek::VerifyingKey::from_bytes(&key)
        .map_err(|_| failure("invalid_signature", false))?
        .verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &signature)
        .map_err(|_| failure("invalid_signature", false))?;
    serde_json::from_slice(&decode(parts[1])?).map_err(|_| failure("invalid_wire", false))
}
pub(crate) fn validate_url(raw: &str, loopback: bool) -> crate::ImResult<reqwest::Url> {
    let url = reqwest::Url::parse(raw).map_err(|_| failure("untrusted_endpoint", false))?;
    let local = url.host_str().is_some_and(|h| {
        h == "localhost"
            || h.parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    });
    if url.query().is_some()
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || (url.scheme() != "https" && !(loopback && local && url.scheme() == "http"))
    {
        return Err(failure("untrusted_endpoint", false));
    }
    Ok(url)
}
pub(crate) fn config_hash(config: &wire::ConfigurationPayload) -> crate::ImResult<String> {
    let realms = config
        .realms
        .iter()
        .map(|r| serde_json::json!({"id":r.id,"address":r.address}))
        .collect::<Vec<_>>();
    Ok(hash(&canonical(
        &serde_json::json!({"protocol":config.protocol,"deployment_id":config.deployment_id,"tenant_id":config.tenant_id,"realm_tenant_name":config.realm_tenant_name,"realms":realms,"register_threshold":config.register_threshold,"recover_threshold":config.recover_threshold,"pin_hashing_mode":config.pin_hashing_mode,"num_guesses":config.num_guesses}),
    )?))
}
pub(crate) fn verify_config(
    response: wire::ConfigResponse,
    trusted: &crate::pin_recovery::PinRecoveryTenantConfig,
    loopback: bool,
) -> crate::ImResult<(wire::ConfigurationPayload, String)> {
    let config: wire::ConfigurationPayload = verify_jws(
        &response.config_jws,
        "awiki-recovery-config+jwt",
        &trusted.config_signing_keys,
    )?;
    let now = now();
    if config.protocol != wire::PROTOCOL
        || config.tenant_id != trusted.tenant_id
        || config.deployment_id != trusted.deployment_id
        || config.control_base_url != trusted.control_base_url
        || config.iat > now + 5
        || config.exp <= config.iat
        || config.exp <= now
        || config.exp.saturating_sub(config.iat) > 86400
        || config.configuration_version == 0
        || config.realms.len() != 3
        || config.register_threshold != 3
        || config.recover_threshold != 2
        || config.pin_hashing_mode != "Standard2019"
        || config.pin_format != "ascii-4-digits"
        || config.num_guesses != 10
        || !config.capability_enabled
        || config_hash(&config)? != response.config_hash
    {
        return Err(failure("invalid_configuration", false));
    }
    validate_url(&config.control_base_url, loopback)?;
    let mut ids = std::collections::BTreeSet::new();
    for realm in &config.realms {
        if realm.id.len() != 32
            || !realm
                .id
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || !ids.insert(&realm.id)
            || realm.receipt_keys.is_empty()
            || realm.receipt_keys.len() > 3
        {
            return Err(failure("invalid_configuration", false));
        }
        let url = validate_url(&realm.address, loopback)?;
        let _ = url;
        for key in &realm.receipt_keys {
            validate_ed25519_jwk(&key.public_jwk)?;
        }
    }
    Ok((config, response.config_hash))
}
pub(crate) fn validate_ed25519_jwk(jwk: &wire::Ed25519PublicJwk) -> crate::ImResult<[u8; 32]> {
    if jwk.kty != "OKP" || jwk.crv != "Ed25519" {
        return Err(failure("root_key_mismatch", false));
    }
    decode(&jwk.x)?
        .as_slice()
        .try_into()
        .map_err(|_| failure("root_key_mismatch", false))
}
pub(crate) fn root_multibase(jwk: &wire::Ed25519PublicJwk) -> crate::ImResult<String> {
    let mut bytes = vec![0xed, 0x01];
    bytes.extend_from_slice(&validate_ed25519_jwk(jwk)?);
    Ok(format!("z{}", bs58::encode(bytes).into_string()))
}
pub(crate) fn jwk_thumbprint(jwk: &wire::Ed25519PublicJwk) -> crate::ImResult<String> {
    validate_ed25519_jwk(jwk)?;
    Ok(hash(&canonical(jwk)?))
}
pub(crate) fn verify_root(root: &[u8], target: &wire::Target) -> crate::ImResult<()> {
    let signing = ed25519_dalek::SigningKey::from_pkcs8_der(root)
        .map_err(|_| failure("root_key_mismatch", false))?;
    if signing.verifying_key().to_bytes() != validate_ed25519_jwk(&target.root_public_jwk)? {
        return Err(failure("root_key_mismatch", false));
    }
    Ok(())
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RootSecret {
    format: String,
    did: String,
    root_kid: String,
    root_algorithm: String,
    private_key_encoding: String,
    private_key_b64u: String,
}
impl Drop for RootSecret {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.private_key_b64u.zeroize();
    }
}
pub(crate) fn encrypt_root(
    header: wire::BackupHeader,
    root: &[u8],
    key: &[u8],
) -> crate::ImResult<wire::BackupResponse> {
    let plain = RootSecret {
        format: "awiki.root-secret.v1".into(),
        did: header.did.clone(),
        root_kid: header.root_kid.clone(),
        root_algorithm: "Ed25519".into(),
        private_key_encoding: "pkcs8_der".into(),
        private_key_b64u: b64(root),
    };
    let plaintext = Zeroizing::new(canonical(&plain)?);
    let mut nonce = [0u8; 24];
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    let cipher =
        XChaCha20Poly1305::new_from_slice(key).map_err(|_| failure("invalid_backup", false))?;
    let ciphertext = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: &plaintext,
                aad: &canonical(&header)?,
            },
        )
        .map_err(|_| failure("invalid_backup", false))?;
    let envelope = wire::BackupEnvelope {
        header,
        algorithm: "XChaCha20-Poly1305".into(),
        nonce_b64u: b64(&nonce),
        ciphertext_b64u: b64(&ciphertext),
    };
    Ok(wire::BackupResponse {
        backup_hash: hash(&canonical(&envelope)?),
        envelope,
    })
}
pub(crate) fn validate_envelope(
    backup: &wire::BackupResponse,
    operation: &wire::OperationResponse,
    target: &wire::Target,
    config: &wire::ConfigurationPayload,
) -> crate::ImResult<()> {
    let h = &backup.envelope.header;
    if backup.backup_hash != hash(&canonical(&backup.envelope)?)
        || operation.backup_hash.as_ref() != Some(&backup.backup_hash)
        || h.format != "awiki.root-backup.v1"
        || h.tenant_id != config.tenant_id
        || h.deployment_id != config.deployment_id
        || h.did != target.did
        || h.root_kid != target.root_kid
        || h.root_public_jwk != target.root_public_jwk
        || h.recovery_id != operation.recovery_id
        || h.backup_epoch != operation.backup_epoch
        || h.slot_id != operation.slot_id
        || h.config_hash != operation.config_hash
        || backup.envelope.algorithm != "XChaCha20-Poly1305"
        || decode(&backup.envelope.nonce_b64u)?.len() != 24
    {
        return Err(failure("invalid_backup", false));
    }
    Ok(())
}
pub(crate) fn decrypt_root(
    backup: &wire::BackupResponse,
    key: &[u8],
    target: &wire::Target,
) -> crate::ImResult<Zeroizing<Vec<u8>>> {
    let env = &backup.envelope;
    let nonce = decode(&env.nonce_b64u)?;
    if nonce.len() != 24 {
        return Err(failure("invalid_backup", false));
    }
    let cipher =
        XChaCha20Poly1305::new_from_slice(key).map_err(|_| failure("invalid_backup", false))?;
    let raw = Zeroizing::new(
        cipher
            .decrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: &decode(&env.ciphertext_b64u)?,
                    aad: &canonical(&env.header)?,
                },
            )
            .map_err(|_| failure("invalid_backup", false))?,
    );
    let secret: RootSecret =
        serde_json::from_slice(&raw).map_err(|_| failure("invalid_backup", false))?;
    if secret.format != "awiki.root-secret.v1"
        || secret.did != target.did
        || secret.root_kid != target.root_kid
        || secret.root_algorithm != "Ed25519"
        || secret.private_key_encoding != "pkcs8_der"
    {
        return Err(failure("invalid_backup", false));
    }
    let root = decode(&secret.private_key_b64u)?;
    verify_root(&root, target)?;
    Ok(root)
}
pub(crate) fn user_info(
    session: &super::pending::Session,
    op: &wire::OperationResponse,
) -> crate::ImResult<Vec<u8>> {
    Ok(Sha256::digest(canonical(&serde_json::json!({"domain":wire::PROTOCOL,"deployment_id":session.config.deployment_id,"tenant_id":session.config.tenant_id,"recovery_id":op.recovery_id,"slot_id":op.slot_id}))?).to_vec())
}
