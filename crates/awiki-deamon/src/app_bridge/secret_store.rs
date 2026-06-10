use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};

const ED25519_PKCS8_PREFIX: &[u8] = &[
    0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04, 0x20,
];

#[derive(Clone, PartialEq, Eq)]
pub struct SecretString(String);

impl SecretString {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for SecretString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted-secret>")
    }
}

impl std::fmt::Display for SecretString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted-secret>")
    }
}

pub fn secret_from_private_key_multibase(value: &str) -> SecretString {
    SecretString::new(value.trim().to_string())
}

pub(crate) fn normalize_delegated_private_key_pem(material: &str) -> Result<String> {
    let material = material.trim();
    if material.is_empty() {
        bail!("delegated private key material must not be empty");
    }
    if material.starts_with("-----BEGIN ") {
        let private_key = anp::PrivateKeyMaterial::from_pem(material)
            .or_else(|_| anp::PrivateKeyMaterial::from_compatible_private_pem(material))
            .context("delegated private key PEM is not supported")?;
        return Ok(private_key.to_pem());
    }
    ed25519_private_key_pem_from_multibase(material)
}

pub(crate) fn public_key_multibase_from_private_material(material: &str) -> Result<String> {
    let pem = normalize_delegated_private_key_pem(material)?;
    let private_key = anp::PrivateKeyMaterial::from_pem(&pem)
        .context("load delegated private key for public derivation")?;
    match private_key.public_key() {
        anp::PublicKeyMaterial::Ed25519(key) => {
            let mut bytes = vec![0xed, 0x01];
            bytes.extend_from_slice(&key.to_bytes());
            Ok(format!("z{}", bs58::encode(bytes).into_string()))
        }
        _ => bail!("delegated private key must be Ed25519"),
    }
}

fn ed25519_private_key_pem_from_multibase(material: &str) -> Result<String> {
    let Some(encoded) = material.strip_prefix('z') else {
        bail!("delegated private key material must be PEM or base58btc multibase");
    };
    let bytes = bs58::decode(encoded)
        .into_vec()
        .context("decode delegated private key multibase")?;
    let key_bytes = match bytes.as_slice() {
        [0x80, 0x26, rest @ ..] if rest.len() == 32 => rest,
        [0x13, 0x00, rest @ ..] if rest.len() == 32 => rest,
        rest if rest.len() == 32 => rest,
        _ => bail!("delegated private key multibase must contain an Ed25519 private key"),
    };
    let key_bytes: [u8; 32] = key_bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("delegated private key length is invalid"))?;
    let pem = ed25519_private_key_pem(&key_bytes);
    anp::PrivateKeyMaterial::from_pem(&pem).context("normalize delegated Ed25519 key")?;
    Ok(pem)
}

fn ed25519_private_key_pem(key_bytes: &[u8; 32]) -> String {
    let mut der = Vec::with_capacity(ED25519_PKCS8_PREFIX.len() + key_bytes.len());
    der.extend_from_slice(ED25519_PKCS8_PREFIX);
    der.extend_from_slice(key_bytes);
    encode_pem("PRIVATE KEY", &der)
}

fn encode_pem(label: &str, contents: &[u8]) -> String {
    let encoded = STANDARD.encode(contents);
    let mut wrapped = String::new();
    for chunk in encoded.as_bytes().chunks(64) {
        wrapped.push_str(std::str::from_utf8(chunk).unwrap_or_default());
        wrapped.push('\n');
    }
    format!("-----BEGIN {label}-----\n{wrapped}-----END {label}-----\n")
}

#[cfg(test)]
pub(crate) fn ed25519_private_key_pem_for_test(key_bytes: &[u8; 32]) -> String {
    ed25519_private_key_pem(key_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_debug_and_display_redact_value() {
        let secret = SecretString::new("z-private-key-material");
        assert_eq!(secret.expose_secret(), "z-private-key-material");
        assert!(!format!("{secret:?}").contains("private-key-material"));
        assert!(!format!("{secret}").contains("private-key-material"));
    }

    #[test]
    fn delegated_private_key_multibase_is_written_as_parseable_pem() {
        let mut key_bytes = [0_u8; 32];
        key_bytes[0] = 1;
        let mut prefixed = vec![0x80, 0x26];
        prefixed.extend_from_slice(&key_bytes);
        let private_multibase = format!("z{}", bs58::encode(prefixed).into_string());

        let pem = normalize_delegated_private_key_pem(&private_multibase).unwrap();

        assert!(pem.starts_with("-----BEGIN PRIVATE KEY-----"));
        anp::PrivateKeyMaterial::from_pem(&pem).unwrap();
    }

    #[test]
    fn delegated_private_key_pem_is_normalized_for_key_ref() {
        let mut key_bytes = [0_u8; 32];
        key_bytes[0] = 2;
        let pem = ed25519_private_key_pem(&key_bytes);

        let normalized = normalize_delegated_private_key_pem(&pem).unwrap();

        assert!(normalized.starts_with("-----BEGIN PRIVATE KEY-----"));
        anp::PrivateKeyMaterial::from_pem(&normalized).unwrap();
    }

    #[test]
    fn delegated_private_key_derives_ed25519_public_multibase() {
        let mut key_bytes = [0_u8; 32];
        key_bytes[0] = 3;
        let pem = ed25519_private_key_pem(&key_bytes);

        let public = public_key_multibase_from_private_material(&pem).unwrap();

        assert!(public.starts_with('z'));
    }
}
