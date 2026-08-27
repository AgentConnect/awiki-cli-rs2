use super::types::{GeneratedIdentity, IdentityError};
use anp::authentication::{
    build_anp_message_service, create_did_wba_document, AnpMessageServiceOptions,
    DidDocumentOptions, DidProfile, VM_KEY_AUTH, VM_KEY_E2EE_AGREEMENT, VM_KEY_E2EE_SIGNING,
};
use rand::RngCore;
use serde_json::Value;
use std::net::IpAddr;

const DEFAULT_ANP_SERVICE_PATH: &str = "/anp-im/rpc";
const AGENT_MESSAGE_SERVICE_PROFILES: &[&str] = &[
    "anp.core.binding.v1",
    "anp.direct.base.v1",
    "anp.group.base.v2",
    "anp.attachment.v1",
];
const AGENT_MESSAGE_SERVICE_SECURITY_PROFILES: &[&str] = &["transport-protected"];

pub fn default_anp_service_endpoint(hostname: &str) -> String {
    format!("https://{}{}", hostname.trim(), DEFAULT_ANP_SERVICE_PATH)
}

pub fn default_anp_service_did(hostname: &str) -> String {
    format!("did:wba:{}", hostname.trim())
}

pub fn validate_anp_service_endpoint(service_endpoint: &str) -> Result<(), IdentityError> {
    let trimmed = service_endpoint.trim();
    if trimmed.is_empty() {
        return Err(invalid_input("anp_service_endpoint is required"));
    }
    let Some((scheme, rest)) = trimmed.split_once("://") else {
        return Err(invalid_input("anp_service_endpoint is invalid"));
    };
    if scheme != "http" && scheme != "https" {
        return Err(invalid_input("anp_service_endpoint must use http or https"));
    }
    let Some(hostname) = endpoint_hostname(rest) else {
        return Err(invalid_input(
            "anp_service_endpoint must include a hostname",
        ));
    };
    let hostname = hostname.trim().to_ascii_lowercase();
    if hostname.is_empty() {
        return Err(invalid_input(
            "anp_service_endpoint must include a hostname",
        ));
    }
    if hostname == "localhost" {
        return Err(invalid_input("anp_service_endpoint must not use localhost"));
    }
    if hostname
        .parse::<IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(false)
    {
        return Err(invalid_input(
            "anp_service_endpoint must not use a loopback address",
        ));
    }
    Ok(())
}

pub fn validate_anp_service_did(service_did: &str) -> Result<(), IdentityError> {
    let trimmed = service_did.trim();
    if trimmed.is_empty() {
        return Err(invalid_input("anp_service_did is required"));
    }
    let Some(remainder) = trimmed.strip_prefix("did:wba:") else {
        return Err(invalid_input("anp_service_did must use did:wba"));
    };
    if trimmed.contains('#') {
        return Err(invalid_input("anp_service_did must not include a fragment"));
    }
    if remainder.is_empty() {
        return Err(invalid_input("anp_service_did must include a domain"));
    }
    if remainder.contains([':', '/', '?']) {
        return Err(invalid_input(
            "anp_service_did must be a bare-domain did:wba DID",
        ));
    }
    Ok(())
}

pub fn build_agent_anp_message_service(
    service_endpoint: &str,
    service_did: &str,
) -> Result<Value, IdentityError> {
    validate_anp_service_endpoint(service_endpoint)?;
    validate_anp_service_did(service_did)?;
    Ok(build_anp_message_service(
        "#message",
        service_endpoint.trim().to_string(),
        AnpMessageServiceOptions::default()
            .with_service_did(service_did.trim().to_string())
            .with_profiles(AGENT_MESSAGE_SERVICE_PROFILES.iter().copied())
            .with_security_profiles(AGENT_MESSAGE_SERVICE_SECURITY_PROFILES.iter().copied()),
    ))
}

pub fn generate_identity(
    hostname: &str,
    service_endpoint: &str,
    service_did: &str,
) -> Result<GeneratedIdentity, IdentityError> {
    generate_identity_with_path_segments(hostname, ["user"], service_endpoint, service_did)
}

pub fn generate_identity_with_path_segments<I, S>(
    hostname: &str,
    path_segments: I,
    service_endpoint: &str,
    service_did: &str,
) -> Result<GeneratedIdentity, IdentityError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let hostname = hostname.trim();
    if hostname.is_empty() {
        return Err(IdentityError::InvalidInput(
            "invalid input: hostname is required".to_string(),
        ));
    }
    let endpoint = if service_endpoint.trim().is_empty() {
        default_anp_service_endpoint(hostname)
    } else {
        service_endpoint.trim().to_string()
    };
    let service_did = if service_did.trim().is_empty() {
        default_anp_service_did(hostname)
    } else {
        service_did.trim().to_string()
    };
    let path_segments = path_segments
        .into_iter()
        .map(|segment| segment.as_ref().trim().to_string())
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    if path_segments.is_empty() {
        return Err(IdentityError::InvalidInput(
            "invalid input: did path prefix is required".to_string(),
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
    let bundle = create_did_wba_document(hostname, options)
        .map_err(|err| IdentityError::Internal(format!("generate did document: {err}")))?;
    let did = bundle
        .did()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| IdentityError::Internal("generated did document is missing id".to_string()))?
        .to_string();
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
        unique_id: did_suffix(&did),
        did,
        did_document: bundle.did_document,
        key1_private_pem,
        key1_public_pem,
        e2ee_signing_private_pem,
        e2ee_agreement_private_pem,
    })
}

pub fn did_suffix(did: &str) -> String {
    did.rsplit(':')
        .next()
        .filter(|value| !value.is_empty())
        .unwrap_or(did)
        .to_string()
}

fn required_private_key(
    bundle: &anp::authentication::DidDocumentBundle,
    name: &str,
) -> Result<String, IdentityError> {
    bundle
        .private_key_pem(name)
        .map(ToString::to_string)
        .ok_or_else(|| IdentityError::Internal(format!("generated did document is missing {name}")))
}

fn required_public_key(
    bundle: &anp::authentication::DidDocumentBundle,
    name: &str,
) -> Result<String, IdentityError> {
    bundle
        .public_key_pem(name)
        .map(ToString::to_string)
        .ok_or_else(|| IdentityError::Internal(format!("generated did document is missing {name}")))
}

fn random_hex(num_bytes: usize) -> String {
    let mut buffer = vec![0_u8; num_bytes];
    rand::thread_rng().fill_bytes(&mut buffer);
    buffer.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn endpoint_hostname(rest: &str) -> Option<&str> {
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host_port = authority
        .rsplit_once('@')
        .map(|(_, host)| host)
        .unwrap_or(authority);
    if let Some(stripped) = host_port.strip_prefix('[') {
        return stripped.split_once(']').map(|(host, _)| host);
    }
    host_port
        .split_once(':')
        .map(|(host, _)| host)
        .or(Some(host_port))
}

fn invalid_input(message: &str) -> IdentityError {
    IdentityError::InvalidInput(format!("invalid input: {message}"))
}
