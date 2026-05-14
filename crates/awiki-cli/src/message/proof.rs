use crate::identity::types::StoredIdentity;
use crate::message::types::MessageError;
use crate::message::DirectPayload;
use anp::proof::{
    generate_rfc9421_origin_proof, Rfc9421OriginProof, Rfc9421OriginProofGenerationOptions,
};
use anp::PrivateKeyMaterial;
use serde_json::{json, Value};

pub const ORIGIN_PROOF_SCHEME: &str = "anp-rfc9421-origin-proof-v1";

pub fn load_private_key_material(pem_text: &str) -> Result<PrivateKeyMaterial, MessageError> {
    PrivateKeyMaterial::from_pem(pem_text).map_err(|err| {
        MessageError::Json(format!("load private key material for origin proof: {err}"))
    })
}

pub fn verification_method_id_from_document(did_document: &Value) -> Option<String> {
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

pub fn build_origin_proof(
    record: &StoredIdentity,
    payload: &DirectPayload,
) -> Result<Rfc9421OriginProof, MessageError> {
    let did_document = record
        .did_document
        .as_ref()
        .ok_or_else(|| missing_verification_method_error(record))?;
    let key_id = verification_method_id_from_document(did_document)
        .ok_or_else(|| missing_verification_method_error(record))?;
    if key_id.is_empty() {
        return Err(missing_verification_method_error(record));
    }
    let private_key = load_private_key_material(&record.key1_private_pem)?;
    generate_rfc9421_origin_proof(
        &payload.method,
        &payload.meta,
        &payload.body,
        &private_key,
        &key_id,
        Rfc9421OriginProofGenerationOptions::default(),
    )
    .map_err(|err| MessageError::Json(format!("generate origin proof: {err}")))
}

pub fn origin_auth_value(origin_proof: &Rfc9421OriginProof) -> Value {
    json!({
        "scheme": ORIGIN_PROOF_SCHEME,
        "origin_proof": origin_proof,
    })
}

fn missing_verification_method_error(record: &StoredIdentity) -> MessageError {
    MessageError::Json(format!(
        "identity {} is missing an authentication verification method",
        record.identity_name
    ))
}
