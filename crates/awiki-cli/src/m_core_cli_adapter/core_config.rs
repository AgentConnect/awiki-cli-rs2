use im_core::ids::Did;
use im_core::{ClientVersionInfo, ImCoreConfig, MessageTransportPolicy, ServiceEndpoint};

use crate::cli_output::ExitError;

pub fn build_im_core_config(
    resolved: &crate::workspace_config::Resolved,
) -> Result<ImCoreConfig, ExitError> {
    build_im_core_config_from_parts(
        &resolved.service_base_url,
        &resolved.did_domain,
        Some(&resolved.user_service_endpoint),
        Some(&resolved.message_service_endpoint),
        Some(&resolved.mail_service_url),
        Some(&resolved.anp_service_endpoint),
        Some(&resolved.anp_service_did),
        Some(&resolved.ca_bundle),
        &resolved.runtime_mode,
    )
}

pub(crate) fn build_im_core_config_from_parts(
    service_base_url: &str,
    did_domain: &str,
    user_service_endpoint: Option<&str>,
    message_service_endpoint: Option<&str>,
    mail_service_endpoint: Option<&str>,
    anp_service_endpoint: Option<&str>,
    anp_service_did: Option<&str>,
    ca_bundle: Option<&str>,
    runtime_mode: &str,
) -> Result<ImCoreConfig, ExitError> {
    let service_base_url = parse_endpoint("service_base_url", service_base_url)?;
    let did_domain = did_domain.trim();
    if did_domain.is_empty() {
        return Err(ExitError::new(
            "invalid_config",
            2,
            "did_host is required to build ImCoreConfig.",
            "Create or switch to a tenant with a DID host.",
        ));
    }
    Ok(ImCoreConfig {
        service_base_url,
        did_domain: did_domain.to_string(),
        client_version_info: Some(
            ClientVersionInfo::new(
                crate::build_info::PRODUCT,
                crate::build_info::RELEASE,
                crate::build_info::VERSION,
                None,
            )
            .map_err(|err| {
                ExitError::new(
                    "invalid_build_info",
                    2,
                    format!("invalid awiki-cli client version metadata: {err}"),
                    "Build awiki-cli with a valid release and version.",
                )
            })?,
        ),
        user_service_endpoint: optional_endpoint("user_service_endpoint", user_service_endpoint)?,
        message_service_endpoint: optional_endpoint(
            "message_service_endpoint",
            message_service_endpoint,
        )?,
        mail_service_endpoint: optional_endpoint("mail_service_endpoint", mail_service_endpoint)?,
        anp_service_endpoint: optional_endpoint("anp_service_endpoint", anp_service_endpoint)?,
        anp_service_did: optional_did("anp_service_did", anp_service_did)?,
        ca_bundle: ca_bundle
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        transport_policy: transport_policy_from_runtime_mode(runtime_mode),
    })
}

pub(crate) fn transport_policy_from_runtime_mode(runtime_mode: &str) -> MessageTransportPolicy {
    match runtime_mode.trim().to_ascii_lowercase().as_str() {
        "websocket" => MessageTransportPolicy::RealtimePreferred,
        "http" | "" => MessageTransportPolicy::HttpOnly,
        _ => MessageTransportPolicy::Auto,
    }
}

fn optional_endpoint(
    field: &'static str,
    value: Option<&str>,
) -> Result<Option<ServiceEndpoint>, ExitError> {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => parse_endpoint(field, value).map(Some),
        None => Ok(None),
    }
}

fn parse_endpoint(field: &'static str, value: &str) -> Result<ServiceEndpoint, ExitError> {
    ServiceEndpoint::parse(value).map_err(|err| {
        ExitError::new(
            "invalid_config",
            2,
            format!("invalid {field}: {err}"),
            "Use an http:// or https:// service endpoint.",
        )
    })
}

fn optional_did(field: &'static str, value: Option<&str>) -> Result<Option<Did>, ExitError> {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => Did::parse(value).map(Some).map_err(|err| {
            ExitError::new(
                "invalid_config",
                2,
                format!("invalid {field}: {err}"),
                "Use a non-empty DID value such as did:wba:example.test.",
            )
        }),
        None => Ok(None),
    }
}
