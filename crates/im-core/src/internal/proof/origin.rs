use anp::proof::{
    generate_rfc9421_origin_proof, Rfc9421OriginProof, Rfc9421OriginProofGenerationOptions,
};
use anp::PrivateKeyMaterial;
use serde_json::{json, Value};

use crate::internal::wire::direct::DirectPayload;

pub(crate) const ORIGIN_PROOF_SCHEME: &str = "anp-rfc9421-origin-proof-v1";

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct OriginProofIdentity {
    pub identity_name: String,
    pub did_document: Option<Value>,
    pub key1_private_pem: String,
    pub verification_method: Option<String>,
}

pub(crate) fn load_private_key_material(pem_text: &str) -> crate::ImResult<PrivateKeyMaterial> {
    PrivateKeyMaterial::from_pem(pem_text).map_err(|err| crate::ImError::Serialization {
        detail: format!("load private key material for origin proof: {err}"),
    })
}

pub(crate) fn verification_method_id_from_document(did_document: &Value) -> Option<String> {
    did_document
        .get("authentication")
        .and_then(Value::as_array)
        .and_then(|methods| methods.first())
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .or_else(|| {
            did_document
                .get("verificationMethod")
                .and_then(Value::as_array)
                .and_then(|methods| methods.first())
                .and_then(|method| method.get("id"))
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
}

pub(crate) fn build_origin_proof(
    identity: &OriginProofIdentity,
    payload: &DirectPayload,
) -> crate::ImResult<Rfc9421OriginProof> {
    let did_document = identity
        .did_document
        .as_ref()
        .ok_or_else(|| missing_verification_method_error(identity))?;
    let key_id = match identity
        .verification_method
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(method) => {
            validate_verification_method_in_document(did_document, method, identity)?;
            method.to_string()
        }
        None => verification_method_id_from_document(did_document)
            .ok_or_else(|| missing_verification_method_error(identity))?,
    };
    if key_id.is_empty() {
        return Err(missing_verification_method_error(identity));
    }
    let private_key = load_private_key_material(&identity.key1_private_pem)?;
    generate_rfc9421_origin_proof(
        &payload.method,
        &payload.meta,
        &payload.body,
        &private_key,
        &key_id,
        Rfc9421OriginProofGenerationOptions::default(),
    )
    .map_err(|err| crate::ImError::Serialization {
        detail: format!("generate origin proof: {err}"),
    })
}

pub(crate) fn validate_verification_method_in_document(
    did_document: &Value,
    verification_method: &str,
    identity: &OriginProofIdentity,
) -> crate::ImResult<()> {
    let method = verification_method.trim();
    if method.is_empty() {
        return Err(missing_verification_method_error(identity));
    }
    if authentication_contains_method(did_document, method)
        || verification_methods_contains_method(did_document, method)
    {
        return Ok(());
    }
    Err(crate::ImError::invalid_input(
        Some("verification_method".to_string()),
        format!(
            "verification method {method} is not present in DID Document for identity {}",
            identity.identity_name
        ),
    ))
}

fn authentication_contains_method(did_document: &Value, method: &str) -> bool {
    did_document
        .get("authentication")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items.iter().any(|item| {
                item.as_str()
                    .is_some_and(|candidate| candidate.trim() == method)
                    || item
                        .get("id")
                        .and_then(Value::as_str)
                        .is_some_and(|candidate| candidate.trim() == method)
            })
        })
}

fn verification_methods_contains_method(did_document: &Value, method: &str) -> bool {
    did_document
        .get("verificationMethod")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items.iter().any(|item| {
                item.get("id")
                    .and_then(Value::as_str)
                    .is_some_and(|candidate| candidate.trim() == method)
            })
        })
}

pub(crate) fn origin_auth_value(origin_proof: &Rfc9421OriginProof) -> Value {
    json!({
        "scheme": ORIGIN_PROOF_SCHEME,
        "origin_proof": origin_proof,
    })
}

fn missing_verification_method_error(identity: &OriginProofIdentity) -> crate::ImError {
    crate::ImError::Serialization {
        detail: format!(
            "identity {} is missing an authentication verification method",
            identity.identity_name
        ),
    }
}
