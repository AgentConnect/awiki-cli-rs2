use serde::{Deserialize, Serialize};

/// Public DID service data. Management remains subject to current Registry
/// authority and the service's existing protected-field policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DidDocumentService {
    pub id: String,
    #[serde(rename = "type")]
    pub service_type: String,
    #[serde(rename = "serviceEndpoint")]
    pub service_endpoint: String,
    #[serde(
        rename = "serviceDid",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub service_did: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub profiles: Vec<String>,
    #[serde(
        rename = "securityProfiles",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub security_profiles: Vec<String>,
}
