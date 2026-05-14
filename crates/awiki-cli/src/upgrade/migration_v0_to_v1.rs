use crate::config;
use std::fmt;

#[derive(Debug)]
pub enum RefreshResolvedConfigError {
    Required,
    ReadConfig(String),
}

impl fmt::Display for RefreshResolvedConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Required => f.write_str("resolved config is required"),
            Self::ReadConfig(err) => f.write_str(err),
        }
    }
}

impl std::error::Error for RefreshResolvedConfigError {}

pub fn refresh_resolved_config(
    current: &config::Resolved,
) -> Result<config::Resolved, RefreshResolvedConfigError> {
    refresh_resolved_config_optional(Some(current))
}

pub fn refresh_resolved_config_optional(
    current: Option<&config::Resolved>,
) -> Result<config::Resolved, RefreshResolvedConfigError> {
    let current = current.ok_or(RefreshResolvedConfigError::Required)?;
    let mut refreshed = current.clone();
    let (file_config, exists, error) = config::read_file_config(&current.paths.config_file);
    if !error.is_empty() {
        return Err(RefreshResolvedConfigError::ReadConfig(error));
    }
    refreshed.config_exists = exists;
    refreshed.config_schema_version = file_config.schema_version;
    if !file_config.runtime.mode.trim().is_empty() {
        refreshed.runtime_mode = file_config.runtime.mode.trim().to_string();
    }
    if !file_config.runtime.socket_path.trim().is_empty() {
        refreshed.runtime_socket_path = file_config.runtime.socket_path.trim().to_string();
    }
    if !file_config.output.format.trim().is_empty() {
        refreshed.output_format = file_config.output.format.trim().to_string();
    }
    if let Some(no_color) = file_config.output.no_color {
        refreshed.no_color = no_color;
    }
    if !file_config.services.service_base_url.trim().is_empty() {
        refreshed.service_base_url =
            config::normalize_base_url(file_config.services.service_base_url.trim());
    }
    if !file_config.services.did_domain.trim().is_empty() {
        refreshed.did_domain = file_config.services.did_domain.trim().to_string();
    }
    if !file_config.services.anp_service_endpoint.trim().is_empty() {
        refreshed.anp_service_endpoint =
            file_config.services.anp_service_endpoint.trim().to_string();
    } else if refreshed.anp_service_endpoint.trim().is_empty() {
        refreshed.anp_service_endpoint =
            config::derive_anp_service_endpoint(&refreshed.service_base_url);
    }
    if !file_config.services.anp_service_did.trim().is_empty() {
        refreshed.anp_service_did = file_config.services.anp_service_did.trim().to_string();
    } else if refreshed.anp_service_did.trim().is_empty() {
        refreshed.anp_service_did = config::derive_anp_service_did(&refreshed.service_base_url);
    }
    if !file_config.services.mail_service_url.trim().is_empty() {
        refreshed.mail_service_url =
            config::normalize_base_url(file_config.services.mail_service_url.trim());
    } else if refreshed.mail_service_url.trim().is_empty() {
        refreshed.mail_service_url = refreshed.service_base_url.clone();
    }
    if !file_config.services.ca_bundle.trim().is_empty() {
        refreshed.ca_bundle = file_config.services.ca_bundle.trim().to_string();
    }
    Ok(refreshed)
}
