//! Migration-only origin proof helpers for `awiki-cli` wrappers.

pub use anp::proof::Rfc9421OriginProof;

use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct OriginProofIdentity {
    pub identity_name: String,
    pub did_document: Option<Value>,
    pub key1_private_pem: String,
    pub verification_method: Option<String>,
}

#[doc(hidden)]
pub const ORIGIN_PROOF_SCHEME: &str = crate::internal::proof::origin::ORIGIN_PROOF_SCHEME;

#[doc(hidden)]
pub fn verification_method_id_from_document(did_document: &Value) -> Option<String> {
    crate::internal::proof::origin::verification_method_id_from_document(did_document)
}

#[doc(hidden)]
pub fn build_origin_proof(
    identity: &OriginProofIdentity,
    payload: &crate::realtime::wire::DirectPayload,
) -> crate::ImResult<Rfc9421OriginProof> {
    crate::internal::proof::origin::build_origin_proof(
        &crate::internal::proof::origin::OriginProofIdentity {
            identity_name: identity.identity_name.clone(),
            did_document: identity.did_document.clone(),
            signer: crate::internal::proof::origin::OriginProofSigner::PrivateKeyPem(
                identity.key1_private_pem.clone(),
            ),
            verification_method: identity.verification_method.clone(),
        },
        payload,
    )
}

#[doc(hidden)]
pub fn origin_auth_value(origin_proof: &Rfc9421OriginProof) -> Value {
    crate::internal::proof::origin::origin_auth_value(origin_proof)
}
