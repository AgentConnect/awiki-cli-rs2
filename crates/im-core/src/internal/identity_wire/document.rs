use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use sha2::{Digest, Sha256};

pub(crate) fn document_hash(document: &serde_json::Value) -> crate::ImResult<String> {
    let canonical = serde_json_canonicalizer::to_vec(document).map_err(|err| {
        crate::ImError::Serialization {
            detail: err.to_string(),
        }
    })?;
    Ok(format!(
        "sha256:{}",
        URL_SAFE_NO_PAD.encode(Sha256::digest(canonical))
    ))
}

/// Decode canonical AWiki vNext identity-document verification methods.
///
/// The ANP generic verification-method decoder does not dispatch Ed25519 or
/// X25519 OKP material from `JsonWebKey2020`, while vNext identity documents
/// intentionally use that representation. Keep the lossless normalization at
/// this boundary so authentication, activation, Root and messaging consumers
/// apply the same key semantics.
pub(crate) fn extract_identity_public_key(
    method: &serde_json::Value,
) -> crate::ImResult<anp::PublicKeyMaterial> {
    if method.get("type").and_then(serde_json::Value::as_str) == Some("JsonWebKey2020")
        && method
            .pointer("/publicKeyJwk/kty")
            .and_then(serde_json::Value::as_str)
            == Some("OKP")
        && method
            .pointer("/publicKeyJwk/crv")
            .and_then(serde_json::Value::as_str)
            == Some("X25519")
    {
        let bytes = URL_SAFE_NO_PAD
            .decode(
                method
                    .pointer("/publicKeyJwk/x")
                    .and_then(serde_json::Value::as_str)
                    .ok_or(crate::ImError::PermissionDenied)?,
            )
            .map_err(|_| crate::ImError::PermissionDenied)?;
        let bytes: [u8; 32] = bytes
            .try_into()
            .map_err(|_| crate::ImError::PermissionDenied)?;
        return Ok(anp::PublicKeyMaterial::X25519(bytes));
    }
    let normalized = normalize_identity_okp_method(method)?;
    anp::authentication::extract_public_key(&normalized)
        .map_err(|_| crate::ImError::PermissionDenied)
}

pub(crate) fn normalize_identity_okp_method(
    method: &serde_json::Value,
) -> crate::ImResult<serde_json::Value> {
    let mut normalized = method.clone();
    let object = normalized
        .as_object_mut()
        .ok_or(crate::ImError::PermissionDenied)?;
    if object.get("type").and_then(serde_json::Value::as_str) != Some("JsonWebKey2020") {
        return Ok(normalized);
    }
    let method_type = match object
        .get("publicKeyJwk")
        .and_then(|jwk| jwk.get("crv"))
        .and_then(serde_json::Value::as_str)
    {
        Some("Ed25519") => "Ed25519VerificationKey2020",
        _ => return Ok(normalized),
    };
    object.insert(
        "type".to_owned(),
        serde_json::Value::String(method_type.to_owned()),
    );
    Ok(normalized)
}
