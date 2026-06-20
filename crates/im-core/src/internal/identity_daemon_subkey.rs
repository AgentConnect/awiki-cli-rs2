use anp::proof::{
    generate_w3c_proof, ProofGenerationOptions, CRYPTOSUITE_EDDSA_JCS_2022,
    PROOF_TYPE_DATA_INTEGRITY,
};
use serde_json::{json, Value};

pub(crate) const DAEMON_SUBKEY_FRAGMENT: &str = "daemon-key-1";
const KEY_TYPE: &str = "Multikey/Ed25519";
const KEY_ALGORITHM: &str = "Ed25519";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GeneratedDaemonSubkeyMaterial {
    pub(crate) public_key_multibase: String,
    pub(crate) private_key_pem: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GeneratedDaemonSubkey {
    pub(crate) verification_method: String,
    pub(crate) public_key_multibase: String,
    pub(crate) private_key_pem: String,
}

pub(crate) fn attach_to_generated_identity(
    generated: &mut crate::internal::identity_generation::GeneratedIdentity,
) -> crate::ImResult<crate::identity::DaemonSubkeyPrivatePackage> {
    let daemon_subkey = generate_for_did(&generated.did);
    apply_to_did_document(&mut generated.did_document, &generated.did, &daemon_subkey)?;
    resign_did_document_with_key1(
        &mut generated.did_document,
        &generated.did,
        &generated.key1_private_pem,
    )?;
    Ok(package_from_parts(
        generated.did.clone(),
        daemon_subkey.verification_method,
        daemon_subkey.public_key_multibase,
        daemon_subkey.private_key_pem,
    ))
}

pub(crate) fn generate_material() -> GeneratedDaemonSubkeyMaterial {
    let private_key = anp::PrivateKeyMaterial::Ed25519(ed25519_dalek::SigningKey::generate(
        &mut rand::rngs::OsRng,
    ));
    let public_key = private_key.public_key();
    let public_key_multibase = match public_key {
        anp::PublicKeyMaterial::Ed25519(key) => ed25519_public_key_to_multibase(&key),
        _ => unreachable!("generated daemon subkey must be Ed25519"),
    };
    GeneratedDaemonSubkeyMaterial {
        public_key_multibase,
        private_key_pem: private_key.to_pem(),
    }
}

pub(crate) fn generate_for_did(did: &crate::ids::Did) -> GeneratedDaemonSubkey {
    let material = generate_material();
    GeneratedDaemonSubkey {
        verification_method: format!("{}#{}", did.as_str(), DAEMON_SUBKEY_FRAGMENT),
        public_key_multibase: material.public_key_multibase,
        private_key_pem: material.private_key_pem,
    }
}

pub(crate) fn expected_verification_method(did: &crate::ids::Did) -> String {
    format!("{}#{}", did.as_str(), DAEMON_SUBKEY_FRAGMENT)
}

pub(crate) fn creation_verification_method(material: &GeneratedDaemonSubkeyMaterial) -> Value {
    json!({
        "id": format!("#{DAEMON_SUBKEY_FRAGMENT}"),
        "type": "Multikey",
        "publicKeyMultibase": material.public_key_multibase,
    })
}

pub(crate) fn creation_authentication_reference() -> String {
    format!("#{DAEMON_SUBKEY_FRAGMENT}")
}

pub(crate) fn package_from_material(
    user_did: crate::ids::Did,
    material: GeneratedDaemonSubkeyMaterial,
) -> crate::identity::DaemonSubkeyPrivatePackage {
    package_from_parts(
        user_did.clone(),
        expected_verification_method(&user_did),
        material.public_key_multibase,
        material.private_key_pem,
    )
}

pub(crate) fn apply_to_did_document(
    did_document: &mut Value,
    did: &crate::ids::Did,
    subkey: &GeneratedDaemonSubkey,
) -> crate::ImResult<()> {
    if subkey.verification_method != expected_verification_method(did) {
        return Err(crate::ImError::invalid_input(
            Some("daemon_subkey.verification_method".to_owned()),
            "daemon subkey verification method must be user_did#daemon-key-1",
        ));
    }
    let Some(object) = did_document.as_object_mut() else {
        return Err(crate::ImError::Serialization {
            detail: "DID Document must be a JSON object".to_owned(),
        });
    };
    let verification_method = json!({
        "id": subkey.verification_method,
        "type": "Multikey",
        "controller": did.as_str(),
        "publicKeyMultibase": subkey.public_key_multibase,
    });
    let methods = object
        .entry("verificationMethod".to_owned())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| crate::ImError::Serialization {
            detail: "DID Document verificationMethod must be an array".to_owned(),
        })?;
    if let Some(existing) = methods.iter_mut().find(|item| {
        item.get("id")
            .and_then(Value::as_str)
            .is_some_and(|id| id == subkey.verification_method)
    }) {
        *existing = verification_method;
    } else {
        methods.push(verification_method);
    }

    let auth = object
        .entry("authentication".to_owned())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| crate::ImError::Serialization {
            detail: "DID Document authentication must be an array".to_owned(),
        })?;
    if !auth.iter().any(|item| {
        item.as_str()
            .is_some_and(|value| value == subkey.verification_method)
    }) {
        auth.push(Value::String(subkey.verification_method.clone()));
    }
    Ok(())
}

pub(crate) fn apply_package_to_did_document(
    did_document: &mut Value,
    package: &crate::identity::DaemonSubkeyPrivatePackage,
) -> crate::ImResult<()> {
    validate_package_private_matches_public(package)?;
    let subkey = GeneratedDaemonSubkey {
        verification_method: package.verification_method.clone(),
        public_key_multibase: package.public_key_multibase.clone(),
        private_key_pem: package.private_key_material().to_string(),
    };
    apply_to_did_document(did_document, &package.user_did, &subkey)
}

pub(crate) fn remove_from_did_document(
    did_document: &mut Value,
    did: &crate::ids::Did,
) -> crate::ImResult<bool> {
    let expected = expected_verification_method(did);
    let Some(object) = did_document.as_object_mut() else {
        return Err(crate::ImError::Serialization {
            detail: "DID Document must be a JSON object".to_owned(),
        });
    };

    let mut removed = false;
    if let Some(methods) = object.get_mut("verificationMethod") {
        let methods = methods
            .as_array_mut()
            .ok_or_else(|| crate::ImError::Serialization {
                detail: "DID Document verificationMethod must be an array".to_owned(),
            })?;
        let before = methods.len();
        methods.retain(|item| {
            item.get("id")
                .and_then(Value::as_str)
                .is_none_or(|id| id != expected)
        });
        removed |= methods.len() != before;
    }

    for relationship in [
        "authentication",
        "assertionMethod",
        "capabilityDelegation",
        "capabilityInvocation",
    ] {
        if let Some(items) = object.get_mut(relationship) {
            let items = items
                .as_array_mut()
                .ok_or_else(|| crate::ImError::Serialization {
                    detail: format!("DID Document {relationship} must be an array"),
                })?;
            let before = items.len();
            items.retain(|item| match item {
                Value::String(value) => value != &expected,
                Value::Object(object) => object
                    .get("id")
                    .and_then(Value::as_str)
                    .is_none_or(|id| id != expected),
                _ => true,
            });
            removed |= items.len() != before;
        }
    }

    Ok(removed)
}

pub(crate) fn resign_did_document_with_key1(
    did_document: &mut Value,
    did: &crate::ids::Did,
    key1_private_pem: &str,
) -> crate::ImResult<()> {
    let private_key = anp::PrivateKeyMaterial::from_pem(key1_private_pem).map_err(|err| {
        crate::ImError::Serialization {
            detail: format!("load DID Document signing key: {err}"),
        }
    })?;
    let options = proof_generation_options_from_document(did_document);
    let signed = generate_w3c_proof(
        did_document,
        &private_key,
        &format!("{}#key-1", did.as_str()),
        options,
    )
    .map_err(|err| crate::ImError::Serialization {
        detail: format!("resign DID Document proof after daemon subkey registration: {err}"),
    })?;
    *did_document = signed;
    Ok(())
}

pub(crate) fn package_from_parts(
    user_did: crate::ids::Did,
    verification_method: String,
    public_key_multibase: String,
    private_key_pem: String,
) -> crate::identity::DaemonSubkeyPrivatePackage {
    crate::identity::DaemonSubkeyPrivatePackage::new_v2_pem(
        user_did,
        verification_method,
        KEY_TYPE.to_owned(),
        Some(KEY_ALGORITHM.to_owned()),
        public_key_multibase,
        private_key_pem,
    )
}

pub(crate) fn package_from_private_pem_and_document(
    did: crate::ids::Did,
    private_key_pem: String,
    did_document: &Value,
) -> crate::ImResult<crate::identity::DaemonSubkeyPrivatePackage> {
    let verification_method = expected_verification_method(&did);
    let public_key_multibase =
        daemon_public_key_multibase(did_document, &did)?.ok_or_else(|| {
            crate::ImError::IdentityNotReady {
                identity: did.as_str().to_string(),
                missing: vec!["daemon_subkey_authentication".to_string()],
            }
        })?;
    let derived_public_key_multibase = public_key_multibase_from_private_pem(&private_key_pem)?;
    if derived_public_key_multibase != public_key_multibase {
        return Err(crate::ImError::IdentityNotReady {
            identity: did.as_str().to_string(),
            missing: vec!["daemon_subkey_private_mismatch".to_string()],
        });
    }
    Ok(package_from_parts(
        did,
        verification_method,
        public_key_multibase,
        private_key_pem,
    ))
}

pub(crate) fn validate_package_against_did_document(
    package: &crate::identity::DaemonSubkeyPrivatePackage,
    did_document: &Value,
) -> crate::ImResult<()> {
    if package.verification_method != expected_verification_method(&package.user_did) {
        return Err(crate::ImError::invalid_input(
            Some("daemon_subkey_package.verification_method".to_owned()),
            "daemon subkey verification method must be user_did#daemon-key-1",
        ));
    }
    validate_package_schema_and_encoding(package)?;
    validate_package_private_matches_public(package)?;
    let document_public = daemon_public_key_multibase(did_document, &package.user_did)?
        .ok_or_else(|| crate::ImError::IdentityNotReady {
            identity: package.user_did.as_str().to_string(),
            missing: vec!["daemon_subkey_authentication".to_string()],
        })?;
    if document_public != package.public_key_multibase {
        return Err(crate::ImError::IdentityNotReady {
            identity: package.user_did.as_str().to_string(),
            missing: vec!["daemon_subkey_public_mismatch".to_string()],
        });
    }
    Ok(())
}

pub(crate) fn validate_package_private_matches_public(
    package: &crate::identity::DaemonSubkeyPrivatePackage,
) -> crate::ImResult<()> {
    let derived_public_key_multibase =
        public_key_multibase_from_private_pem(package.private_key_material())?;
    if derived_public_key_multibase != package.public_key_multibase {
        return Err(crate::ImError::IdentityNotReady {
            identity: package.user_did.as_str().to_string(),
            missing: vec!["daemon_subkey_private_mismatch".to_string()],
        });
    }
    Ok(())
}

fn validate_package_schema_and_encoding(
    package: &crate::identity::DaemonSubkeyPrivatePackage,
) -> crate::ImResult<()> {
    match package.schema.as_str() {
        crate::identity::DAEMON_SUBKEY_PACKAGE_SCHEMA_V1
        | crate::identity::DAEMON_SUBKEY_PACKAGE_SCHEMA_V2 => {}
        _ => {
            return Err(crate::ImError::invalid_input(
                Some("daemon_subkey_package.schema".to_owned()),
                "unsupported daemon subkey package schema",
            ));
        }
    }
    if package.schema == crate::identity::DAEMON_SUBKEY_PACKAGE_SCHEMA_V2
        && package.private_key_encoding != crate::identity::DAEMON_SUBKEY_PRIVATE_KEY_ENCODING_PEM
    {
        return Err(crate::ImError::invalid_input(
            Some("daemon_subkey_package.private_key_encoding".to_owned()),
            "daemon subkey package v2 private_key_encoding must be pem",
        ));
    }
    Ok(())
}

pub(crate) fn daemon_public_key_multibase(
    did_document: &Value,
    did: &crate::ids::Did,
) -> crate::ImResult<Option<String>> {
    let expected = expected_verification_method(did);
    let auth_contains = did_document
        .get("authentication")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items
                .iter()
                .any(|item| item.as_str() == Some(expected.as_str()))
        });
    if !auth_contains {
        return Ok(None);
    }
    let Some(methods) = did_document
        .get("verificationMethod")
        .and_then(Value::as_array)
    else {
        return Err(crate::ImError::Serialization {
            detail: "DID Document verificationMethod must be an array".to_owned(),
        });
    };
    let Some(method) = methods
        .iter()
        .find(|item| item.get("id").and_then(Value::as_str) == Some(expected.as_str()))
    else {
        return Err(crate::ImError::IdentityNotReady {
            identity: did.as_str().to_string(),
            missing: vec!["daemon_subkey_verification_method".to_string()],
        });
    };
    let public_key = method
        .get("publicKeyMultibase")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| crate::ImError::IdentityNotReady {
            identity: did.as_str().to_string(),
            missing: vec!["daemon_subkey_public_key".to_string()],
        })?;
    Ok(Some(public_key.to_string()))
}

pub(crate) fn did_document_references_daemon_subkey(
    did_document: &Value,
    did: &crate::ids::Did,
) -> bool {
    let expected = expected_verification_method(did);
    did_document
        .get("authentication")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items
                .iter()
                .any(|item| item.as_str() == Some(expected.as_str()))
        })
        || did_document
            .get("verificationMethod")
            .and_then(Value::as_array)
            .is_some_and(|items| {
                items
                    .iter()
                    .any(|item| item.get("id").and_then(Value::as_str) == Some(expected.as_str()))
            })
}

pub(crate) fn public_key_multibase_from_private_pem(
    private_key_pem: &str,
) -> crate::ImResult<String> {
    let private_key = anp::PrivateKeyMaterial::from_pem(private_key_pem).map_err(|err| {
        crate::ImError::Serialization {
            detail: format!("load daemon subkey private key: {err}"),
        }
    })?;
    match private_key.public_key() {
        anp::PublicKeyMaterial::Ed25519(key) => Ok(ed25519_public_key_to_multibase(&key)),
        _ => Err(crate::ImError::invalid_input(
            Some("daemon_subkey_package.key_type".to_owned()),
            "daemon subkey private key must be Ed25519",
        )),
    }
}

fn ed25519_public_key_to_multibase(key: &ed25519_dalek::VerifyingKey) -> String {
    let mut bytes = vec![0xed, 0x01];
    bytes.extend_from_slice(&key.to_bytes());
    format!("z{}", bs58::encode(bytes).into_string())
}

fn proof_generation_options_from_document(did_document: &Value) -> ProofGenerationOptions {
    let proof = did_document.get("proof");
    ProofGenerationOptions {
        proof_purpose: proof
            .and_then(|value| value.get("proofPurpose"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .or_else(|| Some("assertionMethod".to_owned())),
        proof_type: proof
            .and_then(|value| value.get("type"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .or_else(|| Some(PROOF_TYPE_DATA_INTEGRITY.to_owned())),
        cryptosuite: proof
            .and_then(|value| value.get("cryptosuite"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .or_else(|| Some(CRYPTOSUITE_EDDSA_JCS_2022.to_owned())),
        created: proof
            .and_then(|value| value.get("created"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        domain: proof
            .and_then(|value| value.get("domain"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        challenge: proof
            .and_then(|value| value.get("challenge"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anp::proof::{verify_w3c_proof, ProofVerificationOptions};
    use serde_json::json;

    #[test]
    fn apply_to_did_document_adds_daemon_key_to_authentication() {
        let did = crate::ids::Did::parse("did:example:alice").unwrap();
        let subkey = generate_for_did(&did);
        let mut document = json!({
            "id": did.as_str(),
            "verificationMethod": [],
            "authentication": []
        });

        apply_to_did_document(&mut document, &did, &subkey).unwrap();

        assert_eq!(
            document["verificationMethod"][0]["id"].as_str(),
            Some("did:example:alice#daemon-key-1")
        );
        assert_eq!(
            document["verificationMethod"][0]["type"].as_str(),
            Some("Multikey")
        );
        assert_eq!(
            document["authentication"][0].as_str(),
            Some("did:example:alice#daemon-key-1")
        );
        assert!(document["verificationMethod"][0]["publicKeyMultibase"]
            .as_str()
            .unwrap()
            .starts_with('z'));
    }

    #[test]
    fn remove_from_did_document_removes_exact_daemon_key_only() {
        let did = crate::ids::Did::parse("did:example:alice").unwrap();
        let daemon_method = expected_verification_method(&did);
        let mut document = json!({
            "id": did.as_str(),
            "verificationMethod": [
                {
                    "id": "did:example:alice#key-1",
                    "type": "Multikey",
                    "controller": did.as_str(),
                    "publicKeyMultibase": "zmain"
                },
                {
                    "id": daemon_method,
                    "type": "Multikey",
                    "controller": did.as_str(),
                    "publicKeyMultibase": "zdaemon"
                },
                {
                    "id": "did:example:alice#daemon-key-10",
                    "type": "Multikey",
                    "controller": did.as_str(),
                    "publicKeyMultibase": "zother"
                }
            ],
            "authentication": [
                "did:example:alice#key-1",
                daemon_method,
                "did:example:alice#daemon-key-10",
                {"id": daemon_method, "type": "Multikey"}
            ],
            "assertionMethod": [daemon_method, "did:example:alice#key-1"],
            "capabilityDelegation": [{"id": daemon_method}],
            "capabilityInvocation": ["did:example:alice#key-1"],
            "service": [{"id": "did:example:alice#messages"}]
        });

        assert!(remove_from_did_document(&mut document, &did).unwrap());

        let serialized = serde_json::to_string(&document).unwrap();
        assert!(!serialized.contains("did:example:alice#daemon-key-1\""));
        assert!(serialized.contains("did:example:alice#daemon-key-10"));
        assert!(serialized.contains("did:example:alice#key-1"));
        assert_eq!(document["service"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn remove_from_did_document_is_idempotent_when_daemon_key_absent() {
        let did = crate::ids::Did::parse("did:example:alice").unwrap();
        let mut document = json!({
            "id": did.as_str(),
            "verificationMethod": [{
                "id": "did:example:alice#key-1",
                "type": "Multikey",
                "controller": did.as_str(),
                "publicKeyMultibase": "zmain"
            }],
            "authentication": ["did:example:alice#key-1"]
        });

        assert!(!remove_from_did_document(&mut document, &did).unwrap());
        assert_eq!(document["verificationMethod"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn package_validation_requires_private_public_and_auth_match() {
        let did = crate::ids::Did::parse("did:example:alice").unwrap();
        let subkey = generate_for_did(&did);
        let mut document = json!({
            "id": did.as_str(),
            "verificationMethod": [],
            "authentication": []
        });
        apply_to_did_document(&mut document, &did, &subkey).unwrap();
        let package = package_from_parts(
            did.clone(),
            subkey.verification_method.clone(),
            subkey.public_key_multibase.clone(),
            subkey.private_key_pem.clone(),
        );

        validate_package_against_did_document(&package, &document).unwrap();

        let mut bad_package = package.clone();
        bad_package.public_key_multibase = "zbad".to_string();
        assert!(validate_package_against_did_document(&bad_package, &document).is_err());

        let mut missing_auth = document.clone();
        missing_auth["authentication"] = Value::Array(Vec::new());
        assert!(validate_package_against_did_document(&package, &missing_auth).is_err());
    }

    #[test]
    fn resign_did_document_after_daemon_key_keeps_w3c_proof_valid() {
        let generated = crate::internal::identity_generation::generate_identity_with_path_segments(
            "awiki.test",
            ["alice"],
            None,
            None,
        )
        .unwrap();
        let mut document = generated.did_document.clone();
        let original_proof = document["proof"].clone();
        let signing_key = anp::PrivateKeyMaterial::from_pem(&generated.key1_private_pem).unwrap();
        assert!(verify_w3c_proof(
            &document,
            &signing_key.public_key(),
            ProofVerificationOptions::default()
        ));

        let subkey = generate_for_did(&generated.did);
        apply_to_did_document(&mut document, &generated.did, &subkey).unwrap();
        assert!(!verify_w3c_proof(
            &document,
            &signing_key.public_key(),
            ProofVerificationOptions::default()
        ));

        resign_did_document_with_key1(&mut document, &generated.did, &generated.key1_private_pem)
            .unwrap();

        assert_eq!(document["proof"]["created"], original_proof["created"]);
        assert_eq!(document["proof"]["domain"], original_proof["domain"]);
        assert_eq!(document["proof"]["challenge"], original_proof["challenge"]);
        assert!(verify_w3c_proof(
            &document,
            &signing_key.public_key(),
            ProofVerificationOptions::default()
        ));
        assert!(document["authentication"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item.as_str() == Some(subkey.verification_method.as_str())));
    }
}
