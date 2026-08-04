//! Pure material/document builder for the one-device Legacy upgrade.
//!
//! This module never writes identity state. In particular, it does not reuse
//! registration persistence or any identity cleanup path.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const VNEXT_DEVICE_PROFILES: &[&str] = &[
    anp::authentication::PROFILE_CORE_BINDING_V1,
    anp::authentication::PROFILE_IDENTITY_DISCOVERY_V1,
    anp::authentication::PROFILE_DIRECT_BASE_V1,
    anp::authentication::PROFILE_DIRECT_E2EE_V2,
    anp::authentication::PROFILE_GROUP_BASE_V1,
    anp::authentication::PROFILE_GROUP_E2EE_V2,
];

const MANAGED_DOCUMENT_FIELDS: &[&str] = &[
    "verificationMethod",
    "authentication",
    "assertionMethod",
    "keyAgreement",
    "deviceManifest",
    "proof",
];

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
    let protocol_device_id = crate::ids::ProtocolDeviceId::generate()?;
    let signing_key_id = format!("{}#{}-sign", did.as_str(), protocol_device_id.as_str());
    let e2ee_key_id = format!("{}#{}-e2ee", did.as_str(), protocol_device_id.as_str());
    let signing_private = anp::PrivateKeyMaterial::Ed25519(ed25519_dalek::SigningKey::generate(
        &mut rand::rngs::OsRng,
    ));
    let e2ee_private = anp::PrivateKeyMaterial::X25519(
        x25519_dalek::StaticSecret::random_from_rng(rand::rngs::OsRng),
    );
    let mut generated = GeneratedLegacyUpgrade {
        did,
        protocol_device_id,
        signing_key_id,
        signing_private_pem: signing_private.to_pem(),
        e2ee_key_id,
        e2ee_private_pem: e2ee_private.to_pem(),
        target_document: Value::Null,
        target_document_hash: String::new(),
    };
    rebuild_legacy_upgrade_target(&mut generated, legacy_document, root_private_pem)?;
    Ok(generated)
}

/// Rebuild the canonical target from a proven-current Legacy document while
/// preserving the exact pending device identity and private keys.
///
/// This is the only safe stale-proof recovery path: callers must first prove
/// that the remote DID is still Legacy. A remotely committed Manifest keeps
/// the original target document so exact-document idempotence remains intact.
pub(crate) fn rebuild_legacy_upgrade_target(
    generated: &mut GeneratedLegacyUpgrade,
    legacy_document: &Value,
    root_private_pem: &str,
) -> crate::ImResult<()> {
    if legacy_document.get("deviceManifest").is_some() {
        return Err(crate::ImError::PermissionDenied);
    }
    let did = crate::ids::Did::parse(
        legacy_document
            .get("id")
            .and_then(Value::as_str)
            .ok_or(crate::ImError::PermissionDenied)?,
    )?;
    if did != generated.did {
        return Err(crate::ImError::PermissionDenied);
    }
    let expected_signing_key_id = format!(
        "{}#{}-sign",
        did.as_str(),
        generated.protocol_device_id.as_str()
    );
    let expected_e2ee_key_id = format!(
        "{}#{}-e2ee",
        did.as_str(),
        generated.protocol_device_id.as_str()
    );
    if generated.signing_key_id != expected_signing_key_id
        || generated.e2ee_key_id != expected_e2ee_key_id
    {
        return Err(crate::ImError::PermissionDenied);
    }

    let root_key_id = format!("{}#key-1", did.as_str());
    let root_method = unique_verification_method(legacy_document, &root_key_id)?;
    let root_private = anp::PrivateKeyMaterial::from_pem(root_private_pem)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    let document_root_public =
        crate::internal::identity_wire::document::extract_identity_public_key(root_method)?;
    if root_private.public_key().to_pem() != document_root_public.to_pem() {
        return Err(crate::ImError::PermissionDenied);
    }
    let signing_private = anp::PrivateKeyMaterial::from_pem(&generated.signing_private_pem)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    let e2ee_private = anp::PrivateKeyMaterial::from_pem(&generated.e2ee_private_pem)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    if !matches!(&signing_private, anp::PrivateKeyMaterial::Ed25519(_))
        || !matches!(&e2ee_private, anp::PrivateKeyMaterial::X25519(_))
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let daemon_method = validated_legacy_daemon_method(legacy_document, &did)?;
    let signing_method = json!({
        "id": generated.signing_key_id.clone(),
        "type": "Multikey",
        "controller": did.as_str(),
        "publicKeyMultibase": crate::internal::identity_generation::public_key_multibase(
            &signing_private.public_key()
        )?,
    });
    let e2ee_method = json!({
        "id": generated.e2ee_key_id.clone(),
        "type": "X25519KeyAgreementKey2019",
        "controller": did.as_str(),
        "publicKeyMultibase": crate::internal::identity_generation::public_key_multibase(
            &e2ee_private.public_key()
        )?,
    });
    let device = anp::authentication::DeviceManifestEntry {
        device_id: generated.protocol_device_id.as_str().to_owned(),
        signing_key_id: generated.signing_key_id.clone(),
        e2ee_key_id: generated.e2ee_key_id.clone(),
        profiles: VNEXT_DEVICE_PROFILES
            .iter()
            .map(|profile| (*profile).to_owned())
            .collect(),
    };
    let base_document = legacy_base_document(legacy_document)?;
    let mut target_document = anp::authentication::build_vnext_did_document(
        &base_document,
        &root_key_id,
        root_method,
        &device,
        &signing_method,
        &e2ee_method,
    )
    .map_err(|_| crate::ImError::PermissionDenied)?;
    if let Some(method) = daemon_method {
        append_daemon_method(&mut target_document, &did, method)?;
    }
    crate::internal::identity_daemon_subkey::resign_did_document_with_fresh_key1_proof(
        &mut target_document,
        &did,
        root_private_pem,
    )?;
    anp::authentication::validate_device_manifest(&target_document)
        .map_err(|_| crate::ImError::PermissionDenied)?
        .filter(|manifest| manifest.devices.len() == 1)
        .ok_or(crate::ImError::PermissionDenied)?;
    generated.target_document_hash =
        crate::internal::identity_wire::document::document_hash(&target_document)?;
    generated.target_document = target_document;
    Ok(())
}

fn unique_verification_method<'a>(document: &'a Value, key_id: &str) -> crate::ImResult<&'a Value> {
    let methods = document
        .get("verificationMethod")
        .and_then(Value::as_array)
        .ok_or(crate::ImError::PermissionDenied)?;
    let mut matches = methods
        .iter()
        .filter(|method| method.get("id").and_then(Value::as_str) == Some(key_id));
    let method = matches.next().ok_or(crate::ImError::PermissionDenied)?;
    if matches.next().is_some() {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(method)
}

fn legacy_base_document(document: &Value) -> crate::ImResult<Value> {
    let mut base = document
        .as_object()
        .cloned()
        .ok_or(crate::ImError::PermissionDenied)?;
    for field in MANAGED_DOCUMENT_FIELDS {
        base.remove(*field);
    }
    Ok(Value::Object(base))
}

fn validated_legacy_daemon_method<'a>(
    document: &'a Value,
    did: &crate::ids::Did,
) -> crate::ImResult<Option<&'a Value>> {
    let daemon_key_id = crate::internal::identity_daemon_subkey::expected_verification_method(did);
    let methods = document
        .get("verificationMethod")
        .and_then(Value::as_array)
        .ok_or(crate::ImError::PermissionDenied)?;
    let matching = methods
        .iter()
        .filter(|method| method.get("id").and_then(Value::as_str) == Some(&daemon_key_id))
        .collect::<Vec<_>>();
    let referenced = relationship_count(document, "authentication", &daemon_key_id);
    if matching.is_empty() && referenced == 0 {
        return Ok(None);
    }
    if matching.len() != 1 || referenced != 1 {
        return Err(crate::ImError::PermissionDenied);
    }
    let method = matching[0];
    let object = method.as_object().ok_or(crate::ImError::PermissionDenied)?;
    if object.len() != 4
        || object.get("id").and_then(Value::as_str) != Some(daemon_key_id.as_str())
        || object.get("type").and_then(Value::as_str) != Some("Multikey")
        || object.get("controller").and_then(Value::as_str) != Some(did.as_str())
        || object
            .get("publicKeyMultibase")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        || !matches!(
            crate::internal::identity_wire::document::extract_identity_public_key(method)?,
            anp::PublicKeyMaterial::Ed25519(_)
        )
    {
        return Err(crate::ImError::PermissionDenied);
    }
    for relationship in [
        "assertionMethod",
        "keyAgreement",
        "capabilityInvocation",
        "capabilityDelegation",
    ] {
        if relationship_count(document, relationship, &daemon_key_id) != 0 {
            return Err(crate::ImError::PermissionDenied);
        }
    }
    Ok(Some(method))
}

fn relationship_count(document: &Value, field: &str, key_id: &str) -> usize {
    document
        .get(field)
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter(|value| match value {
                    Value::String(value) => value == key_id,
                    Value::Object(object) => {
                        object.get("id").and_then(Value::as_str) == Some(key_id)
                    }
                    _ => false,
                })
                .count()
        })
        .unwrap_or_default()
}

fn append_daemon_method(
    document: &mut Value,
    did: &crate::ids::Did,
    method: &Value,
) -> crate::ImResult<()> {
    let object = document
        .as_object_mut()
        .ok_or(crate::ImError::PermissionDenied)?;
    object
        .get_mut("verificationMethod")
        .and_then(Value::as_array_mut)
        .ok_or(crate::ImError::PermissionDenied)?
        .push(method.clone());
    object
        .get_mut("authentication")
        .and_then(Value::as_array_mut)
        .ok_or(crate::ImError::PermissionDenied)?
        .push(Value::String(
            crate::internal::identity_daemon_subkey::expected_verification_method(did),
        ));
    Ok(())
}

#[cfg(test)]
mod tests;
