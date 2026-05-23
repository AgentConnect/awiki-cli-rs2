#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartImCoreConfig {
    pub service_base_url: String,
    pub did_domain: String,
    pub user_service_endpoint: Option<String>,
    pub message_service_endpoint: Option<String>,
    pub anp_service_endpoint: Option<String>,
    pub anp_service_did: Option<String>,
    pub transport_policy: DartMessageTransportPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DartMessageTransportPolicy {
    Auto,
    HttpOnly,
    RealtimePreferred,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartImCorePaths {
    pub identity_root_dir: String,
    pub registry_path: String,
    pub default_identity_path: Option<String>,
    pub sqlite_path: String,
    pub cache_dir: String,
    pub temp_dir: String,
}
