use serde_json::{json, Value};

pub(crate) const DAEMON_SUBKEY_FRAGMENT: &str = "daemon-key-1";
const USER_SUBKEY_PACKAGE_SCHEMA: &str = "awiki.daemon.user_subkey_package.v1";
const KEY_TYPE: &str = "Multikey/Ed25519";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GeneratedDaemonSubkey {
    pub(crate) verification_method: String,
    pub(crate) public_key_multibase: String,
    pub(crate) private_key_pem: String,
}

pub(crate) fn generate_for_did(did: &crate::ids::Did) -> GeneratedDaemonSubkey {
    let private_key = anp::PrivateKeyMaterial::Ed25519(ed25519_dalek::SigningKey::generate(
        &mut rand::rngs::OsRng,
    ));
    let public_key = private_key.public_key();
    let public_key_multibase = match public_key {
        anp::PublicKeyMaterial::Ed25519(key) => ed25519_public_key_to_multibase(&key),
        _ => unreachable!("generated daemon subkey must be Ed25519"),
    };
    GeneratedDaemonSubkey {
        verification_method: format!("{}#{}", did.as_str(), DAEMON_SUBKEY_FRAGMENT),
        public_key_multibase,
        private_key_pem: private_key.to_pem(),
    }
}

pub(crate) fn apply_to_did_document(
    did_document: &mut Value,
    did: &crate::ids::Did,
    subkey: &GeneratedDaemonSubkey,
) -> crate::ImResult<()> {
    if subkey.verification_method != format!("{}#{}", did.as_str(), DAEMON_SUBKEY_FRAGMENT) {
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

pub(crate) fn package_from_parts(
    user_did: crate::ids::Did,
    verification_method: String,
    public_key_multibase: String,
    private_key_pem: String,
) -> crate::identity::DaemonSubkeyPrivatePackage {
    crate::identity::DaemonSubkeyPrivatePackage {
        schema: USER_SUBKEY_PACKAGE_SCHEMA.to_owned(),
        user_did,
        verification_method,
        key_type: KEY_TYPE.to_owned(),
        public_key_multibase,
        private_key_multibase: private_key_pem,
    }
}

fn ed25519_public_key_to_multibase(key: &ed25519_dalek::VerifyingKey) -> String {
    let mut bytes = vec![0xed, 0x01];
    bytes.extend_from_slice(&key.to_bytes());
    format!("z{}", bs58::encode(bytes).into_string())
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
