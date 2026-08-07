use serde::{Deserialize, Serialize};

pub const CLIENT_VERSION_HEADER: &str = "X-AWiki-Client-Version";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientVersionInfo {
    product: String,
    release: String,
    version: String,
    build: Option<u64>,
}

impl ClientVersionInfo {
    pub fn new(
        product: impl Into<String>,
        release: impl Into<String>,
        version: impl Into<String>,
        build: Option<u64>,
    ) -> crate::ImResult<Self> {
        let value = Self {
            product: product.into(),
            release: release.into(),
            version: version.into(),
            build,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn product(&self) -> &str {
        &self.product
    }

    pub fn release(&self) -> &str {
        &self.release
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn build(&self) -> Option<u64> {
        self.build
    }

    pub fn header_value(&self) -> String {
        let value = format!("{}/{}/{}", self.product, self.release, self.version);
        match self.build {
            Some(build) => format!("{value}+{build}"),
            None => value,
        }
    }

    fn validate(&self) -> crate::ImResult<()> {
        if !matches!(
            self.product.as_str(),
            "awiki-me" | "awiki-cli" | "awiki-daemon"
        ) {
            return Err(crate::ImError::invalid_input(
                Some("client_version_info.product".to_owned()),
                "client product must be awiki-me, awiki-cli, or awiki-daemon",
            ));
        }
        if self.release.len() != 4 || !self.release.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(crate::ImError::invalid_input(
                Some("client_version_info.release".to_owned()),
                "client release must contain exactly four ASCII digits",
            ));
        }
        if !is_numeric_dotted_version(&self.version) {
            return Err(crate::ImError::invalid_input(
                Some("client_version_info.version".to_owned()),
                "client version must contain one to four canonical decimal components",
            ));
        }
        Ok(())
    }
}

fn is_numeric_dotted_version(version: &str) -> bool {
    let mut count = 0;
    for component in version.split('.') {
        count += 1;
        if count > 4 || !is_canonical_decimal(component) {
            return false;
        }
    }
    count != 0
}

fn is_canonical_decimal(value: &str) -> bool {
    match value.as_bytes() {
        [b'0'] => true,
        [first, rest @ ..] => {
            first.is_ascii_digit()
                && *first != b'0'
                && rest.iter().all(|byte| byte.is_ascii_digit())
        }
        [] => false,
    }
}

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_version_info: Option<ClientVersionInfo>,
    pub user_service_endpoint: Option<ServiceEndpoint>,
    pub message_service_endpoint: Option<ServiceEndpoint>,
    pub mail_service_endpoint: Option<ServiceEndpoint>,
    pub anp_service_endpoint: Option<ServiceEndpoint>,
    pub anp_service_did: Option<crate::ids::Did>,
    pub ca_bundle: Option<String>,
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
            client_version_info: None,
            user_service_endpoint: None,
            message_service_endpoint: None,
            mail_service_endpoint: None,
            anp_service_endpoint: None,
            anp_service_did: None,
            ca_bundle: None,
            transport_policy: MessageTransportPolicy::Auto,
        })
    }

    pub(crate) fn ca_bundle_path(&self) -> Option<&str> {
        self.ca_bundle
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::ClientVersionInfo;

    #[test]
    fn client_version_info_encodes_the_only_wire_format() {
        assert_eq!(
            ClientVersionInfo::new("awiki-me", "0714", "1.0.31", Some(214))
                .unwrap()
                .header_value(),
            "awiki-me/0714/1.0.31+214"
        );
        assert_eq!(
            ClientVersionInfo::new("awiki-cli", "0714", "1.0.29", None)
                .unwrap()
                .header_value(),
            "awiki-cli/0714/1.0.29"
        );
    }

    #[test]
    fn client_version_info_rejects_unknown_products_and_ambiguous_segments() {
        for result in [
            ClientVersionInfo::new("custom", "0714", "1.0.0", None),
            ClientVersionInfo::new("awiki-cli", "714", "1.0.0", None),
            ClientVersionInfo::new("awiki-cli", "0714", "1.0.0+other", None),
            ClientVersionInfo::new("awiki-cli", "0714", "dev", None),
            ClientVersionInfo::new("awiki-cli", "0714", "01.0.0", None),
            ClientVersionInfo::new("awiki-cli", "0714", "1.0.0.0.1", None),
            ClientVersionInfo::new("awiki-cli", "0714", "1.0.", None),
        ] {
            assert!(result.is_err());
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageTransportPolicy {
    Auto,
    HttpOnly,
    RealtimePreferred,
}
