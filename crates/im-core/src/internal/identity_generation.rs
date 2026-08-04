use anp::authentication::{
    build_anp_message_service, create_did_wba_document_with_creation_options,
    AnpMessageServiceOptions, DidDocumentCreationOptions, DidDocumentOptions, DidProfile,
    VM_KEY_AUTH, VM_KEY_E2EE_AGREEMENT, VM_KEY_E2EE_SIGNING,
};
use rand::RngCore;
use serde_json::{json, Value};

#[cfg(test)]
mod handle_recovery_tests;

const DEFAULT_ANP_SERVICE_PATH: &str = "/anp-im/rpc";
const AGENT_MESSAGE_SERVICE_PROFILES: &[&str] = &[
    "anp.core.binding.v1",
    "anp.direct.base.v1",
    "anp.group.base.v1",
    "anp.attachment.v1",
];
const AGENT_MESSAGE_SERVICE_SECURITY_PROFILES: &[&str] = &["transport-protected"];
const VNEXT_DEVICE_PROFILES: &[&str] = &[
    anp::authentication::PROFILE_CORE_BINDING_V1,
    anp::authentication::PROFILE_IDENTITY_DISCOVERY_V1,
    anp::authentication::PROFILE_DIRECT_BASE_V1,
    anp::authentication::PROFILE_DIRECT_E2EE_V2,
    anp::authentication::PROFILE_GROUP_BASE_V1,
    anp::authentication::PROFILE_GROUP_E2EE_V2,
];
const VNEXT_SERVICE_PROFILES: &[&str] = &[
    anp::authentication::PROFILE_CORE_BINDING_V1,
    anp::authentication::PROFILE_IDENTITY_DISCOVERY_V1,
    anp::authentication::PROFILE_DIRECT_BASE_V1,
    anp::authentication::PROFILE_DIRECT_E2EE_V2,
    anp::authentication::PROFILE_GROUP_BASE_V1,
    anp::authentication::PROFILE_GROUP_E2EE_V2,
    "anp.attachment.v1",
    "anp.federation.relay.v1",
];
const VNEXT_SERVICE_SECURITY_PROFILES: &[&str] =
    &["transport-protected", "direct-e2ee", "group-e2ee"];

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GeneratedIdentity {
    pub(crate) did: crate::ids::Did,
    pub(crate) unique_id: String,
    pub(crate) did_document: Value,
    pub(crate) key1_private_pem: String,
    pub(crate) key1_public_pem: String,
    pub(crate) e2ee_signing_private_pem: String,
    pub(crate) e2ee_agreement_private_pem: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GeneratedIdentityWithDaemonSubkey {
    pub(crate) identity: GeneratedIdentity,
    pub(crate) daemon_subkey_package: crate::identity::DaemonSubkeyPrivatePackage,
}

#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct GeneratedVNextIdentityWithDaemonSubkey {
    pub(crate) did: crate::ids::Did,
    pub(crate) unique_id: String,
    pub(crate) did_document: Value,
    pub(crate) protocol_device_id: crate::ids::ProtocolDeviceId,
    pub(crate) root_key_id: String,
    pub(crate) root_private_pem: String,
    pub(crate) root_public_pem: String,
    pub(crate) device_signing_key_id: String,
    pub(crate) device_signing_private_pem: String,
    pub(crate) device_signing_public_pem: String,
    pub(crate) device_e2ee_key_id: String,
    pub(crate) device_e2ee_private_pem: String,
    pub(crate) device_e2ee_public_pem: String,
    pub(crate) daemon_subkey_package: crate::identity::DaemonSubkeyPrivatePackage,
}

#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct GeneratedHandleRecoveryIdentity {
    pub(crate) did: crate::ids::Did,
    pub(crate) unique_id: String,
    pub(crate) did_document: Value,
    pub(crate) protocol_device_id: crate::ids::ProtocolDeviceId,
    pub(crate) root_key_id: String,
    pub(crate) root_private_pem: String,
    pub(crate) root_public_pem: String,
    pub(crate) device_signing_key_id: String,
    pub(crate) device_signing_private_pem: String,
    pub(crate) device_signing_public_pem: String,
    pub(crate) device_e2ee_key_id: String,
    pub(crate) device_e2ee_private_pem: String,
    pub(crate) device_e2ee_public_pem: String,
}

impl std::fmt::Debug for GeneratedHandleRecoveryIdentity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GeneratedHandleRecoveryIdentity")
            .field("did", &self.did)
            .field("unique_id", &self.unique_id)
            .field("did_document", &self.did_document)
            .field("protocol_device_id", &self.protocol_device_id)
            .field("root_key_id", &self.root_key_id)
            .field("root_private_pem", &"<redacted-private-key>")
            .field("root_public_pem", &self.root_public_pem)
            .field("device_signing_key_id", &self.device_signing_key_id)
            .field("device_signing_private_pem", &"<redacted-private-key>")
            .field("device_signing_public_pem", &self.device_signing_public_pem)
            .field("device_e2ee_key_id", &self.device_e2ee_key_id)
            .field("device_e2ee_private_pem", &"<redacted-private-key>")
            .field("device_e2ee_public_pem", &self.device_e2ee_public_pem)
            .finish()
    }
}

impl std::fmt::Debug for GeneratedVNextIdentityWithDaemonSubkey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GeneratedVNextIdentityWithDaemonSubkey")
            .field("did", &self.did)
            .field("unique_id", &self.unique_id)
            .field("did_document", &self.did_document)
            .field("protocol_device_id", &self.protocol_device_id)
            .field("root_key_id", &self.root_key_id)
            .field("root_private_pem", &"<redacted-private-key>")
            .field("root_public_pem", &self.root_public_pem)
            .field("device_signing_key_id", &self.device_signing_key_id)
            .field("device_signing_private_pem", &"<redacted-private-key>")
            .field("device_signing_public_pem", &self.device_signing_public_pem)
            .field("device_e2ee_key_id", &self.device_e2ee_key_id)
            .field("device_e2ee_private_pem", &"<redacted-private-key>")
            .field("device_e2ee_public_pem", &self.device_e2ee_public_pem)
            .field("daemon_subkey_package", &"<redacted-private-package>")
            .finish()
    }
}

/// Generates a root-signed vNext DID Document for the bootstrap device.
///
/// The caller submits this document through the single `register` RPC and must
/// commit the returned Registry authorization before exposing the identity.
pub(crate) fn generate_vnext_handle_identity_with_default_daemon_subkey(
    hostname: &str,
    local_part: &str,
    service_endpoint: Option<&crate::config::ServiceEndpoint>,
    service_did: Option<&crate::ids::Did>,
) -> crate::ImResult<GeneratedVNextIdentityWithDaemonSubkey> {
    let local_part = canonical_handle_local_part(local_part)?;
    let mut generated = generate_vnext_identity_with_path_segments(
        hostname,
        ["user", local_part.as_str()],
        service_endpoint,
        service_did,
    )?;
    add_handle_service_and_resign(&mut generated, hostname, &local_part)?;
    Ok(generated)
}

/// Generates the exact three-method Manifest document required by Handle
/// Recovery: root, bootstrap signing and bootstrap E2EE. Delegated Daemon
/// authority is deliberately removed and its generated private material is
/// discarded before this value can be persisted.
pub(crate) fn generate_handle_recovery_identity(
    hostname: &str,
    local_part: &str,
    service_endpoint: Option<&crate::config::ServiceEndpoint>,
    service_did: Option<&crate::ids::Did>,
) -> crate::ImResult<GeneratedHandleRecoveryIdentity> {
    let mut generated = generate_vnext_handle_identity_with_default_daemon_subkey(
        hostname,
        local_part,
        service_endpoint,
        service_did,
    )?;
    let daemon_method =
        crate::internal::identity_daemon_subkey::expected_verification_method(&generated.did);
    let object =
        generated
            .did_document
            .as_object_mut()
            .ok_or_else(|| crate::ImError::Serialization {
                detail: "generated Handle Recovery DID document must be an object".to_owned(),
            })?;
    for field in ["verificationMethod", "authentication"] {
        let entries = object
            .get_mut(field)
            .and_then(Value::as_array_mut)
            .ok_or_else(|| crate::ImError::Serialization {
                detail: format!("generated Handle Recovery document is missing {field}"),
            })?;
        entries.retain(|entry| match entry {
            Value::String(reference) => reference != &daemon_method,
            Value::Object(method) => {
                method.get("id").and_then(Value::as_str) != Some(&daemon_method)
            }
            _ => true,
        });
    }
    crate::internal::identity_daemon_subkey::resign_did_document_with_key1(
        &mut generated.did_document,
        &generated.did,
        &generated.root_private_pem,
    )?;
    Ok(GeneratedHandleRecoveryIdentity {
        did: generated.did,
        unique_id: generated.unique_id,
        did_document: generated.did_document,
        protocol_device_id: generated.protocol_device_id,
        root_key_id: generated.root_key_id,
        root_private_pem: generated.root_private_pem,
        root_public_pem: generated.root_public_pem,
        device_signing_key_id: generated.device_signing_key_id,
        device_signing_private_pem: generated.device_signing_private_pem,
        device_signing_public_pem: generated.device_signing_public_pem,
        device_e2ee_key_id: generated.device_e2ee_key_id,
        device_e2ee_private_pem: generated.device_e2ee_private_pem,
        device_e2ee_public_pem: generated.device_e2ee_public_pem,
    })
}

/// Shared vNext builder for independent Agent accounts.
///
/// All consumers use the same root/device/Manifest algorithm; only the
/// identity-kind path segment differs from ordinary App identities.
pub(crate) fn generate_vnext_agent_handle_identity(
    hostname: &str,
    kind: crate::identity::AgentIdentityKind,
    local_part: &str,
    service_endpoint: Option<&crate::config::ServiceEndpoint>,
    service_did: Option<&crate::ids::Did>,
) -> crate::ImResult<GeneratedVNextIdentityWithDaemonSubkey> {
    let local_part = canonical_handle_local_part(local_part)?;
    let mut generated = generate_vnext_identity_with_path_segments(
        hostname,
        ["agent", kind.as_str(), local_part.as_str()],
        service_endpoint,
        service_did,
    )?;
    add_handle_service_and_resign(&mut generated, hostname, &local_part)?;
    Ok(generated)
}

fn canonical_handle_local_part(local_part: &str) -> crate::ImResult<String> {
    let local_part = local_part.trim().to_ascii_lowercase();
    if !anp::wns::validate_local_part(&local_part) {
        return Err(crate::ImError::invalid_input(
            Some("handle".to_owned()),
            "Handle local-part must be a canonical WNS local-part",
        ));
    }
    Ok(local_part)
}

fn add_handle_service_and_resign(
    generated: &mut GeneratedVNextIdentityWithDaemonSubkey,
    hostname: &str,
    local_part: &str,
) -> crate::ImResult<()> {
    let handle_service =
        anp::wns::build_handle_service_entry(generated.did.as_str(), local_part, hostname.trim());
    let services = generated
        .did_document
        .as_object_mut()
        .and_then(|document| document.get_mut("service"))
        .and_then(Value::as_array_mut)
        .ok_or_else(|| crate::ImError::Internal {
            message: "generated vNext DID document is missing service entries".to_owned(),
        })?;
    services.push(handle_service);
    crate::internal::identity_daemon_subkey::resign_did_document_with_key1(
        &mut generated.did_document,
        &generated.did,
        &generated.root_private_pem,
    )?;
    crate::internal::identity_daemon_subkey::validate_package_against_did_document(
        &generated.daemon_subkey_package,
        &generated.did_document,
    )?;
    Ok(())
}

fn generate_vnext_identity_with_path_segments<I, S>(
    hostname: &str,
    path_segments: I,
    service_endpoint: Option<&crate::config::ServiceEndpoint>,
    service_did: Option<&crate::ids::Did>,
) -> crate::ImResult<GeneratedVNextIdentityWithDaemonSubkey>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let hostname = hostname.trim();
    if hostname.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("hostname".to_owned()),
            "hostname is required",
        ));
    }
    let path_segments = path_segments
        .into_iter()
        .map(|segment| segment.as_ref().trim().to_owned())
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    if path_segments.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("path_segments".to_owned()),
            "DID path prefix is required",
        ));
    }
    let endpoint = service_endpoint.map_or_else(
        || default_anp_service_endpoint(hostname),
        |endpoint| endpoint.as_str().to_owned(),
    );
    let service_did = service_did.map_or_else(
        || default_anp_service_did(hostname),
        |did| did.as_str().to_owned(),
    );
    validate_anp_service_did(&service_did)?;

    // Reuse the DID Method generator for the root-bound e1 identifier and root
    // proof material, but deliberately disable its legacy E2EE keys. Device
    // keys are generated separately below and bound by the vNext SDK builder.
    let root_bundle = create_did_wba_document_with_creation_options(
        hostname,
        DidDocumentCreationOptions::from(DidDocumentOptions {
            path_segments,
            domain: Some(hostname.to_owned()),
            challenge: Some(random_hex(16)),
            services: vec![build_vnext_agent_anp_message_service(
                &endpoint,
                &service_did,
            )],
            enable_e2ee: false,
            did_profile: DidProfile::E1,
            ..DidDocumentOptions::default()
        }),
    )
    .map_err(|err| crate::ImError::Internal {
        message: format!("generate vNext DID root: {err}"),
    })?;
    let did_value = root_bundle
        .did()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| crate::ImError::Internal {
            message: "generated vNext DID document is missing id".to_owned(),
        })?;
    let did = crate::ids::Did::parse(did_value)?;
    let root_key_id = format!("{}#{VM_KEY_AUTH}", did.as_str());
    let root_verification_method = required_verification_method(
        &root_bundle.did_document,
        &root_key_id,
        "root verification method",
    )?;
    let root_private_pem = required_private_key(&root_bundle, VM_KEY_AUTH)?;
    let root_public_pem = required_public_key(&root_bundle, VM_KEY_AUTH)?;

    let protocol_device_id = crate::ids::ProtocolDeviceId::generate()?;
    let device_signing_key_id = format!("{}#{}-sign", did.as_str(), protocol_device_id.as_str());
    let device_e2ee_key_id = format!("{}#{}-e2ee", did.as_str(), protocol_device_id.as_str());
    let device_signing_private = anp::PrivateKeyMaterial::Ed25519(
        ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng),
    );
    let device_signing_public = device_signing_private.public_key();
    let device_e2ee_private = anp::PrivateKeyMaterial::X25519(
        x25519_dalek::StaticSecret::random_from_rng(rand::rngs::OsRng),
    );
    let device_e2ee_public = device_e2ee_private.public_key();
    let device_signing_verification_method = json!({
        "id": device_signing_key_id,
        "type": "Multikey",
        "controller": did.as_str(),
        "publicKeyMultibase": public_key_multibase(&device_signing_public)?,
    });
    let device_e2ee_verification_method = json!({
        "id": device_e2ee_key_id,
        "type": "X25519KeyAgreementKey2019",
        "controller": did.as_str(),
        "publicKeyMultibase": public_key_multibase(&device_e2ee_public)?,
    });
    let device_entry = anp::authentication::DeviceManifestEntry {
        device_id: protocol_device_id.as_str().to_owned(),
        signing_key_id: device_signing_key_id.clone(),
        e2ee_key_id: device_e2ee_key_id.clone(),
        profiles: VNEXT_DEVICE_PROFILES
            .iter()
            .map(|profile| (*profile).to_owned())
            .collect(),
    };

    let mut base_document = root_bundle.did_document.clone();
    let base_object = base_document
        .as_object_mut()
        .ok_or_else(|| crate::ImError::Internal {
            message: "generated vNext DID base is not an object".to_owned(),
        })?;
    let root_proof = base_object
        .remove("proof")
        .filter(|proof| proof.get("domain").and_then(Value::as_str) == Some(hostname))
        .ok_or_else(|| crate::ImError::Internal {
            message: "generated vNext DID root proof is missing its authority domain".to_owned(),
        })?;
    for field in [
        "verificationMethod",
        "authentication",
        "assertionMethod",
        "keyAgreement",
        "deviceManifest",
    ] {
        base_object.remove(field);
    }
    let mut did_document = anp::authentication::build_vnext_did_document(
        &base_document,
        &root_key_id,
        &root_verification_method,
        &device_entry,
        &device_signing_verification_method,
        &device_e2ee_verification_method,
    )
    .map_err(|err| crate::ImError::Internal {
        message: format!("build vNext DID document: {err}"),
    })?;

    let daemon_subkey = crate::internal::identity_daemon_subkey::generate_for_did(&did);
    crate::internal::identity_daemon_subkey::apply_to_did_document(
        &mut did_document,
        &did,
        &daemon_subkey,
    )?;
    did_document
        .as_object_mut()
        .ok_or_else(|| crate::ImError::Internal {
            message: "generated vNext DID document is not an object".to_owned(),
        })?
        .insert("proof".to_owned(), root_proof);
    crate::internal::identity_daemon_subkey::resign_did_document_with_key1(
        &mut did_document,
        &did,
        &root_private_pem,
    )?;
    let manifest = anp::authentication::validate_device_manifest(&did_document).map_err(|err| {
        crate::ImError::Internal {
            message: format!("validate generated vNext DID document: {err}"),
        }
    })?;
    if manifest
        .as_ref()
        .is_none_or(|manifest| manifest.devices.len() != 1)
    {
        return Err(crate::ImError::Internal {
            message: "generated vNext DID document must contain one bootstrap device".to_owned(),
        });
    }

    Ok(GeneratedVNextIdentityWithDaemonSubkey {
        did: did.clone(),
        unique_id: did_suffix(did.as_str()),
        did_document,
        protocol_device_id,
        root_key_id,
        root_private_pem,
        root_public_pem,
        device_signing_key_id,
        device_signing_private_pem: device_signing_private.to_pem(),
        device_signing_public_pem: device_signing_public.to_pem(),
        device_e2ee_key_id,
        device_e2ee_private_pem: device_e2ee_private.to_pem(),
        device_e2ee_public_pem: device_e2ee_public.to_pem(),
        daemon_subkey_package: crate::internal::identity_daemon_subkey::package_from_parts(
            did,
            daemon_subkey.verification_method,
            daemon_subkey.public_key_multibase,
            daemon_subkey.private_key_pem,
        ),
    })
}

pub(crate) fn generate_identity_with_default_daemon_subkey<I, S>(
    hostname: &str,
    path_segments: I,
    service_endpoint: Option<&crate::config::ServiceEndpoint>,
    service_did: Option<&crate::ids::Did>,
) -> crate::ImResult<GeneratedIdentityWithDaemonSubkey>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let daemon_subkey = crate::internal::identity_daemon_subkey::generate_material();
    let generated = generate_identity_with_path_segments_and_daemon_subkey(
        hostname,
        path_segments,
        service_endpoint,
        service_did,
        &daemon_subkey,
    )?;
    let daemon_subkey_package = crate::internal::identity_daemon_subkey::package_from_material(
        generated.did.clone(),
        daemon_subkey,
    );
    crate::internal::identity_daemon_subkey::validate_package_against_did_document(
        &daemon_subkey_package,
        &generated.did_document,
    )?;
    Ok(GeneratedIdentityWithDaemonSubkey {
        identity: generated,
        daemon_subkey_package,
    })
}

pub(crate) fn generate_handle_identity_with_default_daemon_subkey(
    hostname: &str,
    local_part: &str,
    service_endpoint: Option<&crate::config::ServiceEndpoint>,
    service_did: Option<&crate::ids::Did>,
) -> crate::ImResult<GeneratedIdentityWithDaemonSubkey> {
    let local_part = local_part.trim().to_ascii_lowercase();
    if local_part.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("handle".to_string()),
            "Handle local-part is required",
        ));
    }
    let mut generated = generate_identity_with_default_daemon_subkey(
        hostname,
        [local_part.as_str()],
        service_endpoint,
        service_did,
    )?;
    let handle_service = anp::wns::build_handle_service_entry(
        generated.identity.did.as_str(),
        &local_part,
        hostname.trim(),
    );
    let services = generated
        .identity
        .did_document
        .as_object_mut()
        .and_then(|document| document.get_mut("service"))
        .and_then(Value::as_array_mut)
        .ok_or_else(|| crate::ImError::Internal {
            message: "generated DID document is missing service entries".to_string(),
        })?;
    services.push(handle_service);
    crate::internal::identity_daemon_subkey::resign_did_document_with_key1(
        &mut generated.identity.did_document,
        &generated.identity.did,
        &generated.identity.key1_private_pem,
    )?;
    crate::internal::identity_daemon_subkey::validate_package_against_did_document(
        &generated.daemon_subkey_package,
        &generated.identity.did_document,
    )?;
    Ok(generated)
}

pub(crate) fn generate_skill_handle_identity(
    hostname: &str,
    local_part: &str,
    service_endpoint: Option<&crate::config::ServiceEndpoint>,
    service_did: Option<&crate::ids::Did>,
) -> crate::ImResult<GeneratedIdentity> {
    let local_part = local_part.trim().to_ascii_lowercase();
    if local_part.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("handle".to_string()),
            "Handle local-part is required",
        ));
    }
    let mut generated = generate_identity_with_path_segments(
        hostname,
        ["agent", "skill", local_part.as_str()],
        service_endpoint,
        service_did,
    )?;
    let handle_service =
        anp::wns::build_handle_service_entry(generated.did.as_str(), &local_part, hostname.trim());
    let services = generated
        .did_document
        .as_object_mut()
        .and_then(|document| document.get_mut("service"))
        .and_then(Value::as_array_mut)
        .ok_or_else(|| crate::ImError::Internal {
            message: "generated DID document is missing service entries".to_string(),
        })?;
    services.push(handle_service);
    crate::internal::identity_daemon_subkey::resign_did_document_with_key1(
        &mut generated.did_document,
        &generated.did,
        &generated.key1_private_pem,
    )?;
    Ok(generated)
}

pub(crate) fn generate_identity_with_path_segments<I, S>(
    hostname: &str,
    path_segments: I,
    service_endpoint: Option<&crate::config::ServiceEndpoint>,
    service_did: Option<&crate::ids::Did>,
) -> crate::ImResult<GeneratedIdentity>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    generate_identity_with_path_segments_internal(
        hostname,
        path_segments,
        service_endpoint,
        service_did,
        None,
    )
}

fn generate_identity_with_path_segments_and_daemon_subkey<I, S>(
    hostname: &str,
    path_segments: I,
    service_endpoint: Option<&crate::config::ServiceEndpoint>,
    service_did: Option<&crate::ids::Did>,
    daemon_subkey: &crate::internal::identity_daemon_subkey::GeneratedDaemonSubkeyMaterial,
) -> crate::ImResult<GeneratedIdentity>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    generate_identity_with_path_segments_internal(
        hostname,
        path_segments,
        service_endpoint,
        service_did,
        Some(daemon_subkey),
    )
}

fn generate_identity_with_path_segments_internal<I, S>(
    hostname: &str,
    path_segments: I,
    service_endpoint: Option<&crate::config::ServiceEndpoint>,
    service_did: Option<&crate::ids::Did>,
    daemon_subkey: Option<&crate::internal::identity_daemon_subkey::GeneratedDaemonSubkeyMaterial>,
) -> crate::ImResult<GeneratedIdentity>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let hostname = hostname.trim();
    if hostname.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("hostname".to_string()),
            "hostname is required",
        ));
    }
    let endpoint = service_endpoint.map_or_else(
        || default_anp_service_endpoint(hostname),
        |endpoint| endpoint.as_str().to_string(),
    );
    let service_did = service_did.map_or_else(
        || default_anp_service_did(hostname),
        |did| did.as_str().to_string(),
    );
    validate_anp_service_did(&service_did)?;
    let path_segments = path_segments
        .into_iter()
        .map(|segment| segment.as_ref().trim().to_string())
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    if path_segments.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("path_segments".to_string()),
            "DID path prefix is required",
        ));
    }
    let service = build_agent_anp_message_service(&endpoint, &service_did)?;
    let options = DidDocumentOptions {
        path_segments,
        domain: Some(hostname.to_string()),
        challenge: Some(random_hex(16)),
        services: vec![service],
        did_profile: DidProfile::E1,
        ..DidDocumentOptions::default()
    };
    let creation_options = daemon_subkey.map_or_else(
        || DidDocumentCreationOptions::from(options.clone()),
        |subkey| {
            DidDocumentCreationOptions::new(options.clone())
                .with_additional_verification_method(
                    crate::internal::identity_daemon_subkey::creation_verification_method(subkey),
                )
                .with_additional_authentication(
                    crate::internal::identity_daemon_subkey::creation_authentication_reference(),
                )
        },
    );
    let bundle = create_did_wba_document_with_creation_options(hostname, creation_options)
        .map_err(|err| crate::ImError::Internal {
            message: format!("generate did document: {err}"),
        })?;
    let did = bundle
        .did()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| crate::ImError::Internal {
            message: "generated did document is missing id".to_string(),
        })?;
    let key1_private_pem = required_private_key(&bundle, VM_KEY_AUTH)?;
    let key1_public_pem = required_public_key(&bundle, VM_KEY_AUTH)?;
    let e2ee_signing_private_pem = bundle
        .private_key_pem(VM_KEY_E2EE_SIGNING)
        .unwrap_or_default()
        .to_string();
    let e2ee_agreement_private_pem = bundle
        .private_key_pem(VM_KEY_E2EE_AGREEMENT)
        .unwrap_or_default()
        .to_string();
    Ok(GeneratedIdentity {
        did: crate::ids::Did::parse(did)?,
        unique_id: did_suffix(did),
        did_document: bundle.did_document,
        key1_private_pem,
        key1_public_pem,
        e2ee_signing_private_pem,
        e2ee_agreement_private_pem,
    })
}

fn default_anp_service_endpoint(hostname: &str) -> String {
    format!("https://{}{}", hostname.trim(), DEFAULT_ANP_SERVICE_PATH)
}

fn default_anp_service_did(hostname: &str) -> String {
    format!("did:wba:{}", hostname.trim())
}

fn build_agent_anp_message_service(
    service_endpoint: &str,
    service_did: &str,
) -> crate::ImResult<Value> {
    Ok(build_anp_message_service(
        "#message",
        service_endpoint.trim().to_string(),
        AnpMessageServiceOptions::default()
            .with_service_did(service_did.trim().to_string())
            .with_profiles(AGENT_MESSAGE_SERVICE_PROFILES.iter().copied())
            .with_security_profiles(AGENT_MESSAGE_SERVICE_SECURITY_PROFILES.iter().copied()),
    ))
}

fn build_vnext_agent_anp_message_service(service_endpoint: &str, service_did: &str) -> Value {
    build_anp_message_service(
        "#message",
        service_endpoint.trim().to_owned(),
        AnpMessageServiceOptions::default()
            .with_service_did(service_did.trim().to_owned())
            .with_profiles(VNEXT_SERVICE_PROFILES.iter().copied())
            .with_security_profiles(VNEXT_SERVICE_SECURITY_PROFILES.iter().copied()),
    )
}

fn validate_anp_service_did(service_did: &str) -> crate::ImResult<()> {
    let trimmed = service_did.trim();
    let Some(remainder) = trimmed.strip_prefix("did:wba:") else {
        return Err(crate::ImError::invalid_input(
            Some("anp_service_did".to_string()),
            "ANP service DID must use did:wba",
        ));
    };
    if trimmed.contains('#') || remainder.is_empty() || remainder.contains([':', '/', '?']) {
        return Err(crate::ImError::invalid_input(
            Some("anp_service_did".to_string()),
            "ANP service DID must be a bare-domain did:wba DID",
        ));
    }
    Ok(())
}

fn did_suffix(did: &str) -> String {
    did.rsplit(':')
        .next()
        .filter(|value| !value.is_empty())
        .unwrap_or(did)
        .to_string()
}

fn required_private_key(
    bundle: &anp::authentication::DidDocumentBundle,
    name: &str,
) -> crate::ImResult<String> {
    bundle
        .private_key_pem(name)
        .map(ToString::to_string)
        .ok_or_else(|| crate::ImError::Internal {
            message: format!("generated did document is missing {name}"),
        })
}

fn required_public_key(
    bundle: &anp::authentication::DidDocumentBundle,
    name: &str,
) -> crate::ImResult<String> {
    bundle
        .public_key_pem(name)
        .map(ToString::to_string)
        .ok_or_else(|| crate::ImError::Internal {
            message: format!("generated did document is missing {name}"),
        })
}

fn required_verification_method(
    did_document: &Value,
    key_id: &str,
    description: &str,
) -> crate::ImResult<Value> {
    did_document
        .get("verificationMethod")
        .and_then(Value::as_array)
        .and_then(|methods| {
            methods
                .iter()
                .find(|method| method.get("id").and_then(Value::as_str) == Some(key_id))
        })
        .cloned()
        .ok_or_else(|| crate::ImError::Internal {
            message: format!("generated DID document is missing {description}"),
        })
}

pub(crate) fn public_key_multibase(public_key: &anp::PublicKeyMaterial) -> crate::ImResult<String> {
    let (codec, bytes): ([u8; 2], Vec<u8>) = match public_key {
        anp::PublicKeyMaterial::Ed25519(key) => ([0xed, 0x01], key.to_bytes().to_vec()),
        anp::PublicKeyMaterial::X25519(key) => ([0xec, 0x01], key.to_vec()),
        _ => {
            return Err(crate::ImError::Internal {
                message: "vNext device keys must use Ed25519 or X25519".to_owned(),
            })
        }
    };
    let mut encoded = Vec::with_capacity(codec.len() + bytes.len());
    encoded.extend_from_slice(&codec);
    encoded.extend_from_slice(&bytes);
    Ok(format!("z{}", bs58::encode(encoded).into_string()))
}

fn random_hex(num_bytes: usize) -> String {
    let mut buffer = vec![0_u8; num_bytes];
    rand::thread_rng().fill_bytes(&mut buffer);
    buffer.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests;
