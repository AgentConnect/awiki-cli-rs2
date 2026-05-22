use anp::authentication::{
    build_anp_message_service, create_did_wba_document, AnpMessageServiceOptions,
    DidDocumentOptions, DidProfile, VM_KEY_AUTH, VM_KEY_E2EE_AGREEMENT, VM_KEY_E2EE_SIGNING,
};
use rand::RngCore;
use serde_json::Value;

const DEFAULT_ANP_SERVICE_PATH: &str = "/anp-im/rpc";
const AGENT_MESSAGE_SERVICE_PROFILES: &[&str] = &[
    "anp.core.binding.v1",
    "anp.direct.base.v1",
    "anp.attachment.v1",
];
const AGENT_MESSAGE_SERVICE_SECURITY_PROFILES: &[&str] = &["transport-protected"];

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
    let bundle =
        create_did_wba_document(hostname, options).map_err(|err| crate::ImError::Internal {
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

fn random_hex(num_bytes: usize) -> String {
    let mut buffer = vec![0_u8; num_bytes];
    rand::thread_rng().fill_bytes(&mut buffer);
    buffer.iter().map(|byte| format!("{byte:02x}")).collect()
}
