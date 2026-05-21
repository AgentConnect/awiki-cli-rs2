use crate::identity::types::StoredIdentity;
use crate::message::types::MessageError;
use crate::message::DirectPayload;
use anp::proof::Rfc9421OriginProof;
use anp::PrivateKeyMaterial;
use serde_json::Value;

pub const ORIGIN_PROOF_SCHEME: &str = im_core::compat::proof::ORIGIN_PROOF_SCHEME;

pub fn load_private_key_material(pem_text: &str) -> Result<PrivateKeyMaterial, MessageError> {
    PrivateKeyMaterial::from_pem(pem_text).map_err(|err| {
        MessageError::Json(format!("load private key material for origin proof: {err}"))
    })
}

pub fn verification_method_id_from_document(did_document: &Value) -> Option<String> {
    im_core::compat::proof::verification_method_id_from_document(did_document)
}

pub fn build_origin_proof(
    record: &StoredIdentity,
    payload: &DirectPayload,
) -> Result<Rfc9421OriginProof, MessageError> {
    let compat_payload = im_core::compat::wire::DirectPayload {
        method: payload.method.clone(),
        meta: payload.meta.clone(),
        body: payload.body.clone(),
    };
    im_core::compat::proof::build_origin_proof(
        &im_core::compat::proof::OriginProofIdentity {
            identity_name: record.identity_name.clone(),
            did_document: record.did_document.clone(),
            key1_private_pem: record.key1_private_pem.clone(),
        },
        &compat_payload,
    )
    .map_err(origin_proof_error)
}

pub fn origin_auth_value(origin_proof: &Rfc9421OriginProof) -> Value {
    im_core::compat::proof::origin_auth_value(origin_proof)
}

fn origin_proof_error(err: im_core::ImError) -> MessageError {
    match err {
        im_core::ImError::Serialization { detail } => MessageError::Json(detail),
        im_core::ImError::InvalidInput { message, .. } => MessageError::Json(message),
        err => MessageError::Json(err.to_string()),
    }
}
