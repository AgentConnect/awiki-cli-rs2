use super::layout::write_secure_text;
use super::types::{IdentityError, Paths};
use crate::anpsdk::PrivateKeyMaterial;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::fs;
use std::io::ErrorKind;

const ANP_SECP256K1_PRIVATE_KEY_LABEL: &str = "ANP SECP256K1 PRIVATE KEY";
const ANP_SECP256R1_PRIVATE_KEY_LABEL: &str = "ANP SECP256R1 PRIVATE KEY";
const ANP_ED25519_PRIVATE_KEY_LABEL: &str = "ANP ED25519 PRIVATE KEY";
const ANP_X25519_PRIVATE_KEY_LABEL: &str = "ANP X25519 PRIVATE KEY";

struct PemBlock {
    label: String,
}

pub fn ensure_key1_private_pem_compatible(path: &str) -> Result<(), IdentityError> {
    ensure_private_key_pem_compatible(path, "key-1 private key")
}

pub(crate) fn ensure_identity_private_keys_compatible(paths: &Paths) -> Result<(), IdentityError> {
    for (path, name) in [
        (&paths.key1_private_path, "key-1 private key"),
        (&paths.e2ee_signing_private_path, "e2ee signing private key"),
        (
            &paths.e2ee_agreement_private_path,
            "e2ee agreement private key",
        ),
    ] {
        ensure_private_key_pem_compatible(path, name)?;
    }
    Ok(())
}

fn ensure_private_key_pem_compatible(path: &str, name: &str) -> Result<(), IdentityError> {
    let raw = match fs::read(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(IdentityError::Io(err)),
    };
    let Some(normalized) = normalize_private_key_pem_to_pkcs8(&raw, name)? else {
        return Ok(());
    };
    write_secure_text(path, &normalized).map_err(|err| {
        IdentityError::Internal(format!("rewrite {name} as standard PKCS#8 PEM: {err}"))
    })?;
    Ok(())
}

fn normalize_private_key_pem_to_pkcs8(
    raw: &[u8],
    name: &str,
) -> Result<Option<String>, IdentityError> {
    let trimmed = trim_ascii_whitespace(raw);
    if trimmed.is_empty() {
        return Err(auth_required(format!("{name} is empty")));
    }
    let text = std::str::from_utf8(trimmed)
        .map_err(|_| auth_required(format!("invalid {name} PEM structure")))?;
    let block = decode_single_pem(text, name)?;

    let private_key = match block.label.as_str() {
        "PRIVATE KEY" => PrivateKeyMaterial::from_compatible_private_pem(text)
            .map_err(|err| auth_required(format!("unsupported {name} format: {err}")))?,
        "EC PRIVATE KEY" => {
            PrivateKeyMaterial::from_compatible_private_pem(text).map_err(|err| {
                auth_required(format!(
                    "unsupported {name} format ({}): {err}",
                    block.label
                ))
            })?
        }
        ANP_ED25519_PRIVATE_KEY_LABEL
        | ANP_X25519_PRIVATE_KEY_LABEL
        | ANP_SECP256R1_PRIVATE_KEY_LABEL
        | ANP_SECP256K1_PRIVATE_KEY_LABEL => PrivateKeyMaterial::from_compatible_private_pem(text)
            .map_err(|err| {
                auth_required(format!(
                    "unsupported {name} format ({}): {err}",
                    block.label
                ))
            })?,
        label => {
            return Err(auth_required(format!(
                "unsupported {name} PEM label {label:?}"
            )))
        }
    };

    let normalized = private_key.to_pem();
    if normalized.trim().is_empty() {
        return Err(IdentityError::Internal(
            "private key cannot be encoded as standard PKCS#8 PEM".to_string(),
        ));
    }
    if trim_trailing_lf(raw) == trim_trailing_lf(normalized.as_bytes()) {
        return Ok(None);
    }
    Ok(Some(normalized))
}

fn decode_single_pem(input: &str, name: &str) -> Result<PemBlock, IdentityError> {
    let mut lines = input.lines();
    let begin = lines
        .next()
        .ok_or_else(|| auth_required(format!("invalid {name} PEM structure")))?;
    if !begin.starts_with("-----BEGIN ") || !begin.ends_with("-----") {
        return Err(auth_required(format!("invalid {name} PEM structure")));
    }
    let label = begin
        .trim_start_matches("-----BEGIN ")
        .trim_end_matches("-----")
        .to_string();
    if label.is_empty() {
        return Err(auth_required(format!("invalid {name} PEM structure")));
    }
    let end_marker = format!("-----END {label}-----");
    let mut body = String::new();
    let mut found_end = false;
    let mut reading_headers = true;
    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if trimmed == end_marker {
            found_end = true;
            break;
        }
        if trimmed.is_empty() {
            reading_headers = false;
            continue;
        }
        if reading_headers && trimmed.contains(':') {
            continue;
        }
        reading_headers = false;
        body.push_str(trimmed);
    }
    if !found_end || body.is_empty() {
        return Err(auth_required(format!("invalid {name} PEM structure")));
    }
    if lines.any(|line| !line.trim().is_empty()) {
        return Err(auth_required(format!("invalid {name} PEM structure")));
    }
    STANDARD
        .decode(body.as_bytes())
        .map_err(|_| auth_required(format!("invalid {name} PEM structure")))?;
    Ok(PemBlock { label })
}

fn auth_required(message: String) -> IdentityError {
    IdentityError::AuthRequired(format!("authentication required: {message}"))
}

fn trim_ascii_whitespace(bytes: &[u8]) -> &[u8] {
    let mut start = 0;
    let mut end = bytes.len();
    while start < end && bytes[start].is_ascii_whitespace() {
        start += 1;
    }
    while end > start && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    &bytes[start..end]
}

fn trim_trailing_lf(bytes: &[u8]) -> &[u8] {
    let mut end = bytes.len();
    while end > 0 && bytes[end - 1] == b'\n' {
        end -= 1;
    }
    &bytes[..end]
}
