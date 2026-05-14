use super::types::{GeneratedIdentity, IdentityError};
use anp::authentication::{
    build_anp_message_service, create_did_wba_document, AnpMessageServiceOptions,
    DidDocumentOptions, DidProfile, VM_KEY_AUTH, VM_KEY_E2EE_AGREEMENT, VM_KEY_E2EE_SIGNING,
};
use rand::RngCore;
use serde_json::Value;

const DEFAULT_ANP_SERVICE_PATH: &str = "/anp-im/rpc";

pub fn default_anp_service_endpoint(hostname: &str) -> String {
    format!("https://{}{}", hostname.trim(), DEFAULT_ANP_SERVICE_PATH)
}

pub fn default_anp_service_did(hostname: &str) -> String {
    format!("did:wba:{}", hostname.trim())
}

pub fn generate_identity(
    hostname: &str,
    service_endpoint: &str,
    service_did: &str,
) -> Result<GeneratedIdentity, IdentityError> {
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
    let service = build_anp_message_service(
        "#message",
        endpoint,
        AnpMessageServiceOptions::default()
            .with_service_did(service_did)
            .with_profiles([
                "anp.core.binding.v1",
                "anp.direct.base.v1",
                "anp.attachment.v1",
            ])
            .with_security_profiles(["transport-protected"]),
    );
    let options = DidDocumentOptions {
        path_segments: vec!["user".to_string()],
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

pub fn stored_handle_fields(handle: &str, full_handle: &str, did: &str) -> (String, String) {
    let mut local_part = handle.trim().to_ascii_lowercase();
    if let Some(stripped) = local_part.strip_prefix("wba://") {
        local_part = stripped.to_string();
    }
    if let Some(index) = local_part.find('.') {
        local_part.truncate(index);
    }
    if let Some((full_local, full)) = normalize_full_handle(full_handle, did) {
        if local_part.is_empty() {
            local_part = full_local;
        }
        return (local_part, full);
    }
    if local_part.is_empty() {
        return (String::new(), String::new());
    }
    let full = derive_full_handle_from_did(&local_part, did);
    (local_part, full)
}

fn derive_full_handle_from_did(handle: &str, did: &str) -> String {
    let Some(domain) = handle_domain_from_did(did) else {
        return String::new();
    };
    format!("{}.{}", handle.trim().to_ascii_lowercase(), domain)
}

fn normalize_full_handle(full_handle: &str, did: &str) -> Option<(String, String)> {
    let trimmed = full_handle.trim().trim_start_matches("wba://");
    if trimmed.is_empty() || trimmed.starts_with("did:") {
        return None;
    }
    if let Some(index) = trimmed.find('.') {
        let local = trimmed[..index].trim().to_ascii_lowercase();
        let domain = trimmed[index + 1..]
            .trim()
            .trim_end_matches('.')
            .to_ascii_lowercase();
        if !local.is_empty() && !domain.is_empty() {
            return Some((local.clone(), format!("{local}.{domain}")));
        }
    }
    let domain = handle_domain_from_did(did)?;
    let local = trimmed.to_ascii_lowercase();
    Some((local.clone(), format!("{local}.{domain}")))
}

fn handle_domain_from_did(did: &str) -> Option<String> {
    let mut parts = did.trim().split(':');
    if parts.next()? != "did" || parts.next()? != "wba" {
        return None;
    }
    let domain = parts.next()?.trim().to_ascii_lowercase();
    if domain.is_empty() {
        return None;
    }
    Some(domain)
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

pub fn object_or_null(value: Option<Value>) -> Value {
    value.unwrap_or(Value::Null)
}
