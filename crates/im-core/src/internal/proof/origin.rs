use anp::proof::{
    generate_rfc9421_origin_proof, Rfc9421OriginProof, Rfc9421OriginProofGenerationOptions,
};
use anp::PrivateKeyMaterial;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::internal::wire::direct::DirectPayload;

pub(crate) const ORIGIN_PROOF_SCHEME: &str = "anp-rfc9421-origin-proof-v1";

#[derive(Clone)]
pub(crate) enum OriginProofSigner {
    Identity(Arc<dyn crate::internal::key_provider::IdentitySigner>),
    Provider(Arc<dyn crate::internal::identity_provider::IdentitySession>),
    PrivateKeyPem(String),
}

impl std::fmt::Debug for OriginProofSigner {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Identity(_) => formatter.write_str("OriginProofSigner::Identity(..)"),
            Self::Provider(_) => formatter.write_str("OriginProofSigner::Provider(..)"),
            Self::PrivateKeyPem(_) => formatter.write_str("OriginProofSigner::PrivateKeyPem(..)"),
        }
    }
}

impl PartialEq for OriginProofSigner {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Identity(left), Self::Identity(right)) => Arc::ptr_eq(left, right),
            (Self::Provider(left), Self::Provider(right)) => Arc::ptr_eq(left, right),
            (Self::PrivateKeyPem(left), Self::PrivateKeyPem(right)) => left == right,
            _ => false,
        }
    }
}

impl OriginProofSigner {
    pub(crate) fn sign_origin_proof(
        &self,
        method: &str,
        meta: &Value,
        body: &Value,
        key_id: &str,
        options: Rfc9421OriginProofGenerationOptions,
    ) -> crate::ImResult<Rfc9421OriginProof> {
        match self {
            Self::Identity(signer) => signer.sign_origin_proof(method, meta, body, key_id, options),
            Self::Provider(_) => Err(crate::ImError::LocalStateUnavailable {
                detail: "asynchronous identity provider requires an async send path".to_owned(),
            }),
            Self::PrivateKeyPem(private_key_pem) => {
                let private_key = load_private_key_material(private_key_pem)?;
                generate_rfc9421_origin_proof(method, meta, body, &private_key, key_id, options)
                    .map_err(|err| crate::ImError::Serialization {
                        detail: format!("generate origin proof: {err}"),
                    })
            }
        }
    }

    pub(crate) async fn sign_origin_proof_async(
        &self,
        method: &str,
        meta: &Value,
        body: &Value,
        key_id: &str,
        options: Rfc9421OriginProofGenerationOptions,
    ) -> crate::ImResult<Rfc9421OriginProof> {
        match self {
            Self::Provider(session) => {
                let proof = session
                    .sign_origin_proof(
                        crate::internal::identity_provider::ProviderOriginProofRequest {
                            method: method.to_owned(),
                            meta: meta.clone(),
                            body: body.clone(),
                            key: crate::internal::identity_provider::ProviderKeySelector::Kid(
                                key_id.to_owned(),
                            ),
                            options:
                                crate::internal::identity_provider::ProviderOriginProofOptions {
                                    created: options.created,
                                    expires: options.expires,
                                    nonce: options.nonce,
                                },
                        },
                    )
                    .await
                    .map_err(crate::internal::identity_provider::map_provider_error)?;
                Ok(Rfc9421OriginProof {
                    content_digest: proof.content_digest,
                    signature_input: proof.signature_input,
                    signature: proof.signature,
                })
            }
            _ => self.sign_origin_proof(method, meta, body, key_id, options),
        }
    }

    pub(crate) fn sign_object_proof(
        &self,
        document: &Value,
        key_id: &str,
        issuer_did: &str,
        created: Option<String>,
    ) -> crate::ImResult<Value> {
        match self {
            Self::Identity(signer) => {
                signer.sign_object_proof(key_id, document, issuer_did, created)
            }
            Self::Provider(_) => Err(crate::ImError::LocalStateUnavailable {
                detail: "asynchronous identity provider requires an async proof path".to_owned(),
            }),
            Self::PrivateKeyPem(private_key_pem) => {
                let private_key = load_private_key_material(private_key_pem)?;
                anp::proof::generate_object_proof(
                    document,
                    &private_key,
                    key_id,
                    issuer_did,
                    created,
                )
                .map_err(|err| crate::ImError::Serialization {
                    detail: format!("generate object proof: {err}"),
                })
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct OriginProofIdentity {
    pub identity_name: String,
    pub did_document: Option<Value>,
    pub signer: OriginProofSigner,
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
    identity.signer.sign_origin_proof(
        &payload.method,
        &payload.meta,
        &payload.body,
        &key_id,
        Rfc9421OriginProofGenerationOptions::default(),
    )
}

pub(crate) async fn build_origin_proof_async(
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
    identity
        .signer
        .sign_origin_proof_async(
            &payload.method,
            &payload.meta,
            &payload.body,
            &key_id,
            Rfc9421OriginProofGenerationOptions::default(),
        )
        .await
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
