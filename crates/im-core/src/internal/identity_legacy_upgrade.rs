//! Pure material/document builder for the one-device Legacy upgrade.
//!
//! This module never writes identity state. In particular, it does not reuse
//! registration persistence or any identity cleanup path.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct GeneratedLegacyUpgrade {
    pub(crate) did: crate::ids::Did,
    pub(crate) protocol_device_id: crate::ids::ProtocolDeviceId,
    pub(crate) signing_key_id: String,
    pub(crate) signing_private_pem: String,
    pub(crate) e2ee_key_id: String,
    pub(crate) e2ee_private_pem: String,
    pub(crate) target_document: Value,
    pub(crate) target_document_hash: String,
}

impl std::fmt::Debug for GeneratedLegacyUpgrade {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GeneratedLegacyUpgrade")
            .field("did", &self.did)
            .field("protocol_device_id", &self.protocol_device_id)
            .field("signing_key_id", &self.signing_key_id)
            .field("signing_private_pem", &"<redacted>")
            .field("e2ee_key_id", &self.e2ee_key_id)
            .field("e2ee_private_pem", &"<redacted>")
            .field("target_document_hash", &self.target_document_hash)
            .finish()
    }
}

pub(crate) fn build_legacy_upgrade(
    legacy_document: &Value,
    root_private_pem: &str,
) -> crate::ImResult<GeneratedLegacyUpgrade> {
    if legacy_document.get("deviceManifest").is_some() {
        return Err(crate::ImError::PermissionDenied);
    }
    let did = crate::ids::Did::parse(
        legacy_document
            .get("id")
            .and_then(Value::as_str)
            .ok_or(crate::ImError::PermissionDenied)?,
    )?;
    let root_key_id = format!("{}#key-1", did.as_str());
    let root_private = anp::PrivateKeyMaterial::from_pem(root_private_pem)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    let root_public = root_private.public_key();
    let root_public_multibase =
        crate::internal::identity_generation::public_key_multibase(&root_public)?;
    let root_matches = legacy_document
        .get("verificationMethod")
        .and_then(Value::as_array)
        .and_then(|methods| {
            methods
                .iter()
                .find(|method| method.get("id").and_then(Value::as_str) == Some(&root_key_id))
        })
        .and_then(|method| method.get("publicKeyMultibase"))
        .and_then(Value::as_str)
        == Some(root_public_multibase.as_str());
    if !root_matches {
        return Err(crate::ImError::PermissionDenied);
    }

    let protocol_device_id = crate::ids::ProtocolDeviceId::generate()?;
    let signing_key_id = format!("{}#{}-sign", did.as_str(), protocol_device_id.as_str());
    let e2ee_key_id = format!("{}#{}-e2ee", did.as_str(), protocol_device_id.as_str());
    let signing_private = anp::PrivateKeyMaterial::Ed25519(ed25519_dalek::SigningKey::generate(
        &mut rand::rngs::OsRng,
    ));
    let e2ee_private = anp::PrivateKeyMaterial::X25519(
        x25519_dalek::StaticSecret::random_from_rng(rand::rngs::OsRng),
    );
    let mut target_document = legacy_document.clone();
    let object = target_document
        .as_object_mut()
        .ok_or(crate::ImError::PermissionDenied)?;
    object
        .entry("verificationMethod")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or(crate::ImError::PermissionDenied)?
        .extend([
            json!({
                "id": signing_key_id.clone(),
                "type": "Multikey",
                "controller": did.as_str(),
                "publicKeyMultibase": crate::internal::identity_generation::public_key_multibase(
                    &signing_private.public_key()
                )?,
            }),
            json!({
                "id": e2ee_key_id.clone(),
                "type": "X25519KeyAgreementKey2019",
                "controller": did.as_str(),
                "publicKeyMultibase": crate::internal::identity_generation::public_key_multibase(
                    &e2ee_private.public_key()
                )?,
            }),
        ]);
    append_relationship(object, "authentication", &signing_key_id)?;
    append_relationship(object, "assertionMethod", &signing_key_id)?;
    append_relationship(object, "keyAgreement", &e2ee_key_id)?;
    object.insert(
        "deviceManifest".to_owned(),
        json!({
            "type": "ANPDeviceManifest",
            "devices": [{
                "device_id": protocol_device_id.as_str(),
                "signing_key_id": signing_key_id.clone(),
                "e2ee_key_id": e2ee_key_id.clone(),
                "profiles": [
                    anp::authentication::PROFILE_CORE_BINDING_V2,
                    anp::authentication::PROFILE_IDENTITY_DISCOVERY_V2,
                    anp::authentication::PROFILE_DIRECT_BASE_V2,
                    anp::authentication::PROFILE_DIRECT_E2EE_V2,
                    anp::authentication::PROFILE_GROUP_BASE_V2,
                    anp::authentication::PROFILE_GROUP_E2EE_V2
                ]
            }]
        }),
    );
    crate::internal::identity_daemon_subkey::resign_did_document_with_key1(
        &mut target_document,
        &did,
        root_private_pem,
    )?;
    anp::authentication::validate_device_manifest(&target_document)
        .map_err(|_| crate::ImError::PermissionDenied)?
        .filter(|manifest| manifest.devices.len() == 1)
        .ok_or(crate::ImError::PermissionDenied)?;
    let target_document_hash =
        crate::internal::identity_wire::document::document_hash(&target_document)?;
    Ok(GeneratedLegacyUpgrade {
        did,
        protocol_device_id,
        signing_key_id,
        signing_private_pem: signing_private.to_pem(),
        e2ee_key_id,
        e2ee_private_pem: e2ee_private.to_pem(),
        target_document,
        target_document_hash,
    })
}

fn append_relationship(
    object: &mut serde_json::Map<String, Value>,
    field: &str,
    key_id: &str,
) -> crate::ImResult<()> {
    let values = object
        .entry(field)
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or(crate::ImError::PermissionDenied)?;
    values.push(Value::String(key_id.to_owned()));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_preserves_legacy_services_delegation_and_key_relationships() {
        let generated =
            crate::internal::identity_generation::generate_handle_identity_with_default_daemon_subkey(
                "example.test",
                "alice",
                None,
                None,
            )
            .unwrap();
        let legacy = generated.identity;
        let service = legacy.did_document.get("service").cloned();
        let old_methods = legacy.did_document["verificationMethod"]
            .as_array()
            .unwrap()
            .clone();
        let old_authentication = legacy.did_document["authentication"]
            .as_array()
            .unwrap()
            .clone();
        let old_assertion = legacy.did_document["assertionMethod"]
            .as_array()
            .unwrap()
            .clone();
        let old_agreement = legacy.did_document["keyAgreement"]
            .as_array()
            .unwrap()
            .clone();

        let upgrade = build_legacy_upgrade(&legacy.did_document, &legacy.key1_private_pem).unwrap();

        assert_eq!(upgrade.target_document.get("service").cloned(), service);
        for method in old_methods {
            assert!(upgrade.target_document["verificationMethod"]
                .as_array()
                .unwrap()
                .contains(&method));
        }
        for (field, old) in [
            ("authentication", old_authentication),
            ("assertionMethod", old_assertion),
            ("keyAgreement", old_agreement),
        ] {
            for relationship in old {
                assert!(upgrade.target_document[field]
                    .as_array()
                    .unwrap()
                    .contains(&relationship));
            }
        }
        assert_eq!(
            anp::authentication::validate_device_manifest(&upgrade.target_document)
                .unwrap()
                .unwrap()
                .devices
                .len(),
            1
        );
    }
}
