use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceEndpoint(String);

impl ServiceEndpoint {
    pub fn parse(input: impl Into<String>) -> crate::ImResult<Self> {
        let input = input.into();
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(crate::ImError::invalid_input(
                Some("service_base_url".to_string()),
                "endpoint must not be empty",
            ));
        }
        if !(trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
            return Err(crate::ImError::invalid_input(
                Some("service_base_url".to_string()),
                "endpoint must start with http:// or https://",
            ));
        }
        Ok(Self(trimmed.trim_end_matches('/').to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImCoreConfig {
    pub service_base_url: ServiceEndpoint,
    pub did_domain: String,
    pub user_service_endpoint: Option<ServiceEndpoint>,
    pub message_service_endpoint: Option<ServiceEndpoint>,
    pub anp_service_endpoint: Option<ServiceEndpoint>,
    pub anp_service_did: Option<crate::ids::Did>,
    pub transport_policy: MessageTransportPolicy,
}

impl ImCoreConfig {
    pub fn new(
        service_base_url: ServiceEndpoint,
        did_domain: impl Into<String>,
    ) -> crate::ImResult<Self> {
        let did_domain = did_domain.into();
        if did_domain.trim().is_empty() {
            return Err(crate::ImError::invalid_input(
                Some("did_domain".to_string()),
                "DID domain must not be empty",
            ));
        }
        Ok(Self {
            service_base_url,
            did_domain,
            user_service_endpoint: None,
            message_service_endpoint: None,
            anp_service_endpoint: None,
            anp_service_did: None,
            transport_policy: MessageTransportPolicy::Auto,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageTransportPolicy {
    Auto,
    HttpOnly,
    RealtimePreferred,
}
