use im_core::{ImCoreConfig, MessageTransportPolicy, ServiceEndpoint};

use crate::output::ExitError;

pub fn build_im_core_config(resolved: &crate::config::Resolved) -> Result<ImCoreConfig, ExitError> {
    build_im_core_config_from_parts(
        &resolved.service_base_url,
        &resolved.did_domain,
        Some(&resolved.service_base_url),
        Some(&resolved.service_base_url),
        &resolved.runtime_mode,
    )
}

pub(crate) fn build_im_core_config_from_parts(
    service_base_url: &str,
    did_domain: &str,
    user_service_endpoint: Option<&str>,
    message_service_endpoint: Option<&str>,
    runtime_mode: &str,
) -> Result<ImCoreConfig, ExitError> {
    let service_base_url = parse_endpoint("service_base_url", service_base_url)?;
    let did_domain = did_domain.trim();
    if did_domain.is_empty() {
        return Err(ExitError::new(
            "invalid_config",
            2,
            "did_domain is required to build ImCoreConfig.",
            "Set services.did_domain in the awiki-cli config.",
        ));
    }
    Ok(ImCoreConfig {
        service_base_url,
        did_domain: did_domain.to_string(),
        user_service_endpoint: optional_endpoint("user_service_endpoint", user_service_endpoint)?,
        message_service_endpoint: optional_endpoint(
            "message_service_endpoint",
            message_service_endpoint,
        )?,
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
