use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::env;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::durable_fs;
use awiki_user_dirs::{expand_tilde, requires_home_expansion, try_home_dir, HomeDirUnavailable};

mod write;
pub(crate) use write::write_file_config_raw;
pub use write::{
    clear_hermes_secret, clear_openclaw_token, configure_hermes_host_notify,
    ensure_config_schema_version, read_openclaw_token, set_hermes_secret, set_openclaw_token,
    update_active_identity, update_hermes_settings, update_host_notify_enabled,
    update_host_notify_sink, update_openclaw_settings, update_runtime_listener_settings,
    update_runtime_settings, write_file_config,
};

const APP_NAME: &str = "awiki-cli";
const CONFIG_FILE_NAME: &str = "config.yaml";
const GLOBAL_CONFIG_FILE_NAME: &str = "global.json";
const TENANT_REGISTRY_FILE_NAME: &str = "registry.json";
const TENANTS_DIR_NAME: &str = "tenants";
const LEGACY_ARCHIVE_DIR_NAME: &str = "legacy-archive";
const LEGACY_ARCHIVE_LOCK_DIR_NAME: &str = ".legacy-archive.lock";
const DEFAULT_TENANT_ALIAS: &str = "default";
const CHINA_TENANT_NAME: &str = "china";
const CHINA_TENANT_DISPLAY_NAME: &str = "AWiki China (Shanghai)";
const CHINA_SERVICE_BASE_URL: &str = "https://awiki.me";
const CHINA_DID_DOMAIN: &str = "awiki.me";
const GLOBAL_TENANT_NAME: &str = "global";
const GLOBAL_TENANT_DISPLAY_NAME: &str = "AWiki Global (Silicon Valley)";
const GLOBAL_SERVICE_BASE_URL: &str = "https://awiki.ai";
const GLOBAL_DID_DOMAIN: &str = "awiki.ai";
const TENANT_REGISTRY_SCHEMA_VERSION: i64 = 2;
const OFFICIAL_TENANT_CATALOG_VERSION: i64 = 1;
const DEFAULT_SERVICE_BASE_URL_ENV: &str = "AWIKI_CLI_DEFAULT_BACKEND_BASE_URL";
const DEFAULT_DID_DOMAIN_ENV: &str = "AWIKI_CLI_DEFAULT_DID_HOST";
const DEFAULT_ANP_PATH: &str = "/anp-im/rpc";
const DEFAULT_RUNTIME_MODE: &str = "websocket";
const DEFAULT_OUTPUT_FORMAT: &str = "json";
const DEFAULT_LISTENER_ENABLED: bool = true;
const DEFAULT_LISTENER_AUTO_INSTALL: bool = true;
const DEFAULT_LISTENER_AUTO_START: bool = true;
const DEFAULT_HOST_NOTIFY_ENABLED: bool = true;
const DEFAULT_HOST_NOTIFY_SINK: &str = "log";
const DEFAULT_HOST_NOTIFY_FILE: &str = "host-notify.events.jsonl";
const DEFAULT_OPENCLAW_HOOK_URL: &str = "http://127.0.0.1:18789/hooks/agent";
const DEFAULT_OPENCLAW_AGENT_ID: &str = "main";
const DEFAULT_OPENCLAW_HOOK_NAME: &str = "AWiki";
const DEFAULT_HERMES_NOTIFY_URL: &str = "http://127.0.0.1:8765/notify/host-event";
const DEFAULT_HERMES_DELIVER_TARGET: &str = "feishu";
const DEFAULT_IDENTITY_SECRET_STORAGE_MODE: &str = "vault_required";
pub const CONFIG_SCHEMA_VERSION: i64 = 1;

#[derive(Debug, Clone, Default)]
pub struct Overrides {
    pub identity: String,
    pub identity_changed: bool,
    pub tenant: String,
    pub tenant_changed: bool,
    pub format: String,
    pub format_changed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Paths {
    pub workspace_home_dir: String,
    pub root_dir: String,
    pub config_dir: String,
    pub data_dir: String,
    pub state_dir: String,
    pub cache_dir: String,
    pub logs_dir: String,
    pub config_file: String,
    pub identity_dir: String,
    pub database_file: String,
    pub legacy_credentials_dir: String,
    pub legacy_data_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TenantKind {
    BuiltIn,
    Custom,
}

impl Default for TenantKind {
    fn default() -> Self {
        Self::Custom
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TenantProfile {
    pub name: String,
    pub display_name: String,
    pub backend_base_url: String,
    pub did_host: String,
    pub dir_name: String,
    #[serde(default)]
    pub kind: TenantKind,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TenantContext {
    pub active: String,
    pub active_source: String,
    pub profile: TenantProfile,
    pub registry_file: String,
    pub global_config_file: String,
    pub tenants_dir: String,
    pub tenant_dir: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TenantSetupResult {
    pub action: String,
    pub tenant: TenantContext,
}

/// Minimal tenant-scoped update inputs resolved without creating or migrating files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdatePolicyContext {
    pub tenant_alias: String,
    pub service_base_url: String,
    pub cache_dir: String,
    pub disable_strict_version: bool,
    pub metadata_cache_ttl_seconds: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceConfigErrorKind {
    InvalidArgument,
    InvalidConfig,
    NotFound,
    Conflict,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceConfigError {
    kind: WorkspaceConfigErrorKind,
    message: String,
    hint: String,
}

impl WorkspaceConfigError {
    fn new(
        kind: WorkspaceConfigErrorKind,
        message: impl Into<String>,
        hint: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            message: message.into(),
            hint: hint.into(),
        }
    }

    pub fn invalid_argument(message: impl Into<String>, hint: impl Into<String>) -> Self {
        Self::new(WorkspaceConfigErrorKind::InvalidArgument, message, hint)
    }

    pub fn invalid_config(message: impl Into<String>, hint: impl Into<String>) -> Self {
        Self::new(WorkspaceConfigErrorKind::InvalidConfig, message, hint)
    }

    pub fn not_found(message: impl Into<String>, hint: impl Into<String>) -> Self {
        Self::new(WorkspaceConfigErrorKind::NotFound, message, hint)
    }

    pub fn conflict(message: impl Into<String>, hint: impl Into<String>) -> Self {
        Self::new(WorkspaceConfigErrorKind::Conflict, message, hint)
    }

    pub fn kind(&self) -> WorkspaceConfigErrorKind {
        self.kind
    }

    pub fn hint(&self) -> &str {
        &self.hint
    }
}

impl fmt::Display for WorkspaceConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for WorkspaceConfigError {}

#[derive(Debug, Serialize, Deserialize, Default)]
struct GlobalConfig {
    #[serde(default)]
    schema_version: i64,
    #[serde(default)]
    active_tenant: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
struct TenantRegistry {
    #[serde(default)]
    schema_version: i64,
    #[serde(default)]
    official_catalog_version: i64,
    #[serde(default)]
    aliases: BTreeMap<String, String>,
    #[serde(default)]
    tenants: Vec<TenantProfile>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EnvHit {
    pub key: String,
    pub value: String,
    pub tier: String,
    pub target: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValueSource {
    pub source: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub key: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub value: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Resolved {
    pub paths: Paths,
    pub config_schema_version: i64,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub active_identity: String,
    pub runtime_mode: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub runtime_socket_path: String,
    pub runtime_listener_enabled: bool,
    pub runtime_listener_auto_install: bool,
    pub runtime_listener_auto_start: bool,
    pub host_notify_enabled: bool,
    pub host_notify_sink: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub host_notify_file_path: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub host_notify_openclaw_hook_url: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub host_notify_openclaw_agent_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub host_notify_openclaw_hook_name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub host_notify_hermes_notify_url: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub host_notify_hermes_deliver: String,
    pub output_format: String,
    pub no_color: bool,
    pub service_base_url: String,
    pub user_service_endpoint: String,
    pub message_service_endpoint: String,
    pub did_domain: String,
    pub anp_service_endpoint: String,
    pub anp_service_did: String,
    pub mail_service_url: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub ca_bundle: String,
    pub update_disable_strict_version: bool,
    pub update_metadata_cache_ttl_seconds: i64,
    pub config_exists: bool,
    pub config_error: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub env_hits: Vec<EnvHit>,
    pub sources: BTreeMap<String, ValueSource>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResolvedSecretStorage {
    pub mode: String,
    pub vault_dir: String,
    pub workspace_id: String,
    pub device_id: String,
    pub root_key_env: String,
    pub root_key_available: bool,
    pub root_key_source: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct FileConfig {
    #[serde(default)]
    pub schema_version: i64,
    #[serde(default)]
    pub identity: IdentityConfig,
    #[serde(default)]
    pub runtime: RuntimeConfig,
    #[serde(default)]
    pub output: OutputConfig,
    #[serde(default)]
    pub secret_storage: SecretStorageConfig,
    #[serde(default)]
    pub services: ServicesConfig,
    #[serde(default)]
    pub update: UpdateConfig,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct SecretStorageConfig {
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub vault_dir: String,
    #[serde(default)]
    pub workspace_id: String,
    #[serde(default)]
    pub device_id: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct IdentityConfig {
    #[serde(default)]
    pub active: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct RuntimeConfig {
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub socket_path: String,
    #[serde(default)]
    pub listener: ListenerConfig,
    #[serde(default)]
    pub host_notify: HostNotifyConfig,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ListenerConfig {
    pub enabled: Option<bool>,
    pub auto_install: Option<bool>,
    pub auto_start: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct HostNotifyConfig {
    pub enabled: Option<bool>,
    #[serde(default)]
    pub sink: String,
    #[serde(default)]
    pub file_path: String,
    #[serde(default)]
    pub openclaw: OpenClawConfig,
    #[serde(default)]
    pub hermes: HermesConfig,
    #[serde(default)]
    pub webhook: LegacyWebhookConfig,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct OpenClawConfig {
    #[serde(default)]
    pub hook_url: String,
    #[serde(default)]
    pub agent_id: String,
    #[serde(default)]
    pub hook_name: String,
    #[serde(default)]
    pub token: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct HermesConfig {
    #[serde(default)]
    pub notify_url: String,
    #[serde(default)]
    pub deliver: String,
    #[serde(default)]
    pub secret: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct LegacyWebhookConfig {
    #[serde(default)]
    pub notify_url: String,
    #[serde(default)]
    pub secret: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct OutputConfig {
    #[serde(default)]
    pub format: String,
    pub no_color: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ServicesConfig {
    #[serde(default)]
    pub service_base_url: String,
    #[serde(default)]
    pub user_service_endpoint: String,
    #[serde(default)]
    pub message_service_endpoint: String,
    #[serde(default)]
    pub did_domain: String,
    #[serde(default)]
    pub anp_service_endpoint: String,
    #[serde(default)]
    pub anp_service_did: String,
    #[serde(default)]
    pub ca_bundle: String,
    #[serde(default)]
    pub mail_service_url: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct UpdateConfig {
    #[serde(default)]
    pub disable_strict_version: bool,
    #[serde(default)]
    pub metadata_cache_ttl_seconds: i64,
}

pub fn resolve(overrides: Overrides) -> anyhow::Result<Resolved> {
    let home = try_home_dir();
    let (product_home_dir, product_source) = resolve_workspace_home(home.as_deref())?;
    archive_legacy_product_root(&product_home_dir)?;
    let tenant = resolve_active_tenant(&product_home_dir, &overrides)?;
    let tenant_dir = tenant_dir(&product_home_dir, &tenant.profile);
    let paths = build_paths(home.as_deref(), &tenant_dir);
    validate_deprecated_config_fields(&paths.config_file)?;
    let (file_config, config_exists, config_error) = read_file_config(&paths.config_file);
    let mut sources = BTreeMap::new();
    sources.insert("product_home_dir".to_string(), product_source.clone());
    sources.insert(
        "active_tenant".to_string(),
        ValueSource {
            source: tenant.active_source.clone(),
            key: String::new(),
            value: tenant.active.clone(),
        },
    );
    sources.insert(
        "tenant_dir".to_string(),
        ValueSource {
            source: "tenant_registry".to_string(),
            key: "active_tenant".to_string(),
            value: tenant.tenant_dir.clone(),
        },
    );
    sources.insert(
        "workspace_home_dir".to_string(),
        ValueSource {
            source: "tenant_registry".to_string(),
            key: "active_tenant".to_string(),
            value: paths.workspace_home_dir.clone(),
        },
    );
    sources.insert(
        "root_dir".to_string(),
        ValueSource {
            source: "tenant_registry".to_string(),
            key: "active_tenant".to_string(),
            value: paths.root_dir.clone(),
        },
    );
    for (key, value) in [
        ("config_dir", &paths.config_dir),
        ("data_dir", &paths.data_dir),
        ("state_dir", &paths.state_dir),
        ("cache_dir", &paths.cache_dir),
        ("logs_dir", &paths.logs_dir),
    ] {
        sources.insert(
            key.to_string(),
            ValueSource {
                source: "derived".to_string(),
                key: "active_tenant".to_string(),
                value: value.clone(),
            },
        );
    }

    let (active_identity, active_source) = choose_value(
        &overrides.identity,
        overrides.identity_changed,
        &file_config.identity.active,
        "",
    );
    sources.insert("active_identity".to_string(), active_source);
    let (runtime_mode, runtime_source) =
        choose_value("", false, &file_config.runtime.mode, DEFAULT_RUNTIME_MODE);
    sources.insert("runtime_mode".to_string(), runtime_source);
    let default_socket = default_runtime_bridge_path(&paths);
    let (runtime_socket_path, socket_source) =
        choose_value("", false, &file_config.runtime.socket_path, &default_socket);
    sources.insert("runtime_socket_path".to_string(), socket_source);
    let (output_format, output_source) = choose_value(
        &overrides.format,
        overrides.format_changed,
        &file_config.output.format,
        DEFAULT_OUTPUT_FORMAT,
    );
    sources.insert("output_format".to_string(), output_source);
    resolve_secret_storage_from_config(&paths, &file_config.secret_storage, &mut sources)?;
    let service_base_url = normalize_base_url(&tenant.profile.backend_base_url);
    sources.insert(
        "service_base_url".to_string(),
        ValueSource {
            source: "tenant_registry".to_string(),
            key: "backend_base_url".to_string(),
            value: service_base_url.clone(),
        },
    );
    let user_service_endpoint = service_base_url.clone();
    sources.insert(
        "user_service_endpoint".to_string(),
        ValueSource {
            source: "tenant_registry".to_string(),
            key: "backend_base_url".to_string(),
            value: user_service_endpoint.clone(),
        },
    );
    let message_service_endpoint = service_base_url.clone();
    sources.insert(
        "message_service_endpoint".to_string(),
        ValueSource {
            source: "tenant_registry".to_string(),
            key: "backend_base_url".to_string(),
            value: message_service_endpoint.clone(),
        },
    );
    let did_domain = tenant.profile.did_host.clone();
    sources.insert(
        "did_domain".to_string(),
        ValueSource {
            source: "tenant_registry".to_string(),
            key: "did_host".to_string(),
            value: did_domain.clone(),
        },
    );
    let (mut anp_endpoint, anp_endpoint_source) =
        choose_value("", false, &file_config.services.anp_service_endpoint, "");
    if anp_endpoint.trim().is_empty() {
        anp_endpoint = derive_anp_service_endpoint(&service_base_url);
        sources.insert(
            "anp_service_endpoint".to_string(),
            ValueSource {
                source: "derived_default".to_string(),
                key: "backend_base_url".to_string(),
                value: anp_endpoint.clone(),
            },
        );
    } else {
        sources.insert("anp_service_endpoint".to_string(), anp_endpoint_source);
    }
    let (mut anp_did, anp_did_source) =
        choose_value("", false, &file_config.services.anp_service_did, "");
    if anp_did.trim().is_empty() {
        anp_did = derive_anp_service_did(&service_base_url);
        sources.insert(
            "anp_service_did".to_string(),
            ValueSource {
                source: "derived_default".to_string(),
                key: "backend_base_url".to_string(),
                value: anp_did.clone(),
            },
        );
    } else {
        sources.insert("anp_service_did".to_string(), anp_did_source);
    }
    let (mail_service_url, mail_source) = if file_config.services.mail_service_url.trim().is_empty()
    {
        (
            service_base_url.clone(),
            ValueSource {
                source: "derived_default".to_string(),
                key: "backend_base_url".to_string(),
                value: service_base_url.clone(),
            },
        )
    } else {
        let value = normalize_base_url(&file_config.services.mail_service_url);
        (
            value.clone(),
            ValueSource {
                source: "config_file".to_string(),
                key: String::new(),
                value,
            },
        )
    };
    sources.insert("mail_service_url".to_string(), mail_source);

    let host_notify_sink = normalize_host_notify_sink(&file_config.runtime.host_notify.sink);
    validate_host_notify_sink(&host_notify_sink)?;
    let host_notify_file_path =
        host_notify_file_path(&paths, &file_config.runtime.host_notify, &host_notify_sink);
    let (openclaw_hook_url, openclaw_hook_source, openclaw_agent_id, openclaw_hook_name) =
        resolve_openclaw_fields(&file_config.runtime.host_notify, &host_notify_sink);
    sources.insert(
        "host_notify_openclaw_hook_url".to_string(),
        openclaw_hook_source,
    );
    let (hermes_notify_url, hermes_notify_source, hermes_deliver, hermes_deliver_source) =
        resolve_hermes_fields(&file_config.runtime.host_notify, &host_notify_sink);
    sources.insert(
        "host_notify_hermes_notify_url".to_string(),
        hermes_notify_source,
    );
    sources.insert(
        "host_notify_hermes_deliver".to_string(),
        hermes_deliver_source,
    );

    Ok(Resolved {
        paths,
        config_schema_version: normalized_config_schema_version(file_config.schema_version),
        active_identity,
        runtime_mode,
        runtime_socket_path,
        runtime_listener_enabled: file_config
            .runtime
            .listener
            .enabled
            .unwrap_or(DEFAULT_LISTENER_ENABLED),
        runtime_listener_auto_install: file_config
            .runtime
            .listener
            .auto_install
            .unwrap_or(DEFAULT_LISTENER_AUTO_INSTALL),
        runtime_listener_auto_start: file_config
            .runtime
            .listener
            .auto_start
            .unwrap_or(DEFAULT_LISTENER_AUTO_START),
        host_notify_enabled: file_config
            .runtime
            .host_notify
            .enabled
            .unwrap_or(DEFAULT_HOST_NOTIFY_ENABLED),
        host_notify_sink,
        host_notify_file_path,
        host_notify_openclaw_hook_url: openclaw_hook_url,
        host_notify_openclaw_agent_id: openclaw_agent_id,
        host_notify_openclaw_hook_name: openclaw_hook_name,
        host_notify_hermes_notify_url: hermes_notify_url,
        host_notify_hermes_deliver: hermes_deliver,
        output_format,
        no_color: file_config.output.no_color.unwrap_or(false),
        service_base_url,
        user_service_endpoint,
        message_service_endpoint,
        did_domain,
        anp_service_endpoint: anp_endpoint,
        anp_service_did: anp_did,
        mail_service_url: normalize_base_url(&mail_service_url),
        ca_bundle: file_config.services.ca_bundle,
        update_disable_strict_version: file_config.update.disable_strict_version,
        update_metadata_cache_ttl_seconds: file_config.update.metadata_cache_ttl_seconds,
        config_exists,
        config_error,
        env_hits: collect_env_hits(),
        sources,
    })
}

/// Resolve the selected tenant for root update preflight without mutating the workspace.
pub fn resolve_update_policy_context(
    tenant_override: &str,
    tenant_changed: bool,
) -> anyhow::Result<UpdatePolicyContext> {
    let home = try_home_dir();
    let (product_home_dir, _) = resolve_workspace_home(home.as_deref())?;
    let registry_path = tenant_registry_path(&product_home_dir);
    let (registry, default_active) = if registry_path.exists() {
        let raw = fs::read_to_string(&registry_path).map_err(|error| {
            anyhow::anyhow!("read tenant registry {}: {error}", registry_path.display())
        })?;
        let registry: TenantRegistry = serde_json::from_str(&raw).map_err(|error| {
            anyhow::anyhow!("parse tenant registry {}: {error}", registry_path.display())
        })?;
        if registry.schema_version != 1 && registry.schema_version != TENANT_REGISTRY_SCHEMA_VERSION
        {
            anyhow::bail!(
                "unsupported tenant registry schema_version {}",
                registry.schema_version
            );
        }
        let active = if global_config_path(&product_home_dir).exists() {
            load_global_config(&product_home_dir)?.active_tenant
        } else {
            CHINA_TENANT_NAME.to_string()
        };
        (registry, active)
    } else {
        let (tenants, aliases, active) = default_tenant_profiles()?;
        (
            TenantRegistry {
                schema_version: TENANT_REGISTRY_SCHEMA_VERSION,
                official_catalog_version: OFFICIAL_TENANT_CATALOG_VERSION,
                aliases,
                tenants,
            },
            active,
        )
    };
    let requested = if tenant_changed {
        normalize_tenant_name(tenant_override)?
    } else if default_active.trim().is_empty() {
        CHINA_TENANT_NAME.to_string()
    } else {
        normalize_tenant_name(&default_active)?
    };
    let selected = resolve_tenant_alias(&registry, &requested);
    let profile = registry
        .tenants
        .iter()
        .find(|tenant| tenant.name == selected)
        .ok_or_else(|| tenant_not_found_error("active tenant", &selected))?;
    let paths = build_paths(home.as_deref(), &tenant_dir(&product_home_dir, profile));
    let (config, _, _) = read_file_config(&paths.config_file);
    Ok(UpdatePolicyContext {
        tenant_alias: selected,
        service_base_url: normalize_base_url(&profile.backend_base_url),
        cache_dir: paths.cache_dir,
        disable_strict_version: config.update.disable_strict_version,
        metadata_cache_ttl_seconds: config.update.metadata_cache_ttl_seconds,
    })
}

/// Return the product-level workspace root used to resolve tenant state.
///
/// `Resolved.paths` is intentionally tenant-scoped. Long-running child
/// processes must receive the product root through
/// `AWIKI_CLI_WORKSPACE_HOME_DIR`, otherwise they would treat the active
/// tenant directory as a second product root and resolve a nested tenant.
pub fn product_home_dir(resolved: &Resolved) -> &str {
    resolved
        .sources
        .get("product_home_dir")
        .map(|source| source.value.as_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(resolved.paths.workspace_home_dir.as_str())
}

pub fn snapshot(resolved: &Resolved) -> Value {
    let mut value = serde_json::to_value(resolved).unwrap_or_else(|_| json!({}));
    if let Some(object) = value.as_object_mut() {
        if let Ok(tenant) = tenant_context_for_resolved(resolved) {
            object.insert(
                "tenant".to_string(),
                serde_json::to_value(tenant).unwrap_or_else(|_| json!({})),
            );
        }
        if let Ok(secret_storage) = resolve_secret_storage(resolved) {
            object.insert(
                "secret_storage".to_string(),
                serde_json::to_value(secret_storage).unwrap_or_else(|_| json!({})),
            );
        }
    }
    value
}

pub(crate) fn read_file_config(path: &str) -> (FileConfig, bool, String) {
    match fs::read_to_string(path) {
        Ok(raw) => match parse_file_config(&raw) {
            Ok(config) => (config, true, String::new()),
            Err(err) => (FileConfig::default(), true, err),
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            (FileConfig::default(), false, String::new())
        }
        Err(err) => (FileConfig::default(), false, err.to_string()),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantCreateInput {
    pub name: String,
    pub display_name: Option<String>,
    pub backend_base_url: String,
    pub did_host: String,
}

fn tenant_name_hint() -> &'static str {
    "Use a tenant name like `default`, `acme`, or `team-1`. Use --display-name for spaces, uppercase presentation, or non-ASCII labels."
}

fn did_host_hint() -> &'static str {
    "Use a bare DID host like `awiki.ai` or `tenant.example`, not a URL."
}

fn backend_base_url_hint() -> &'static str {
    "Use an absolute http(s) backend base URL like `https://awiki.ai`."
}

fn tenant_not_found_error(label: &str, name: &str) -> WorkspaceConfigError {
    WorkspaceConfigError::not_found(
        format!("{label} {name:?} does not exist"),
        "Run `awiki-cli tenant list` to inspect tenants, or create one with `awiki-cli tenant create <name> --backend-base-url <url> --did-host <domain>`.",
    )
}

fn duplicate_tenant_endpoint_error(prefix: &str) -> WorkspaceConfigError {
    WorkspaceConfigError::conflict(
        format!("{prefix}tenant with the same backend_base_url and did_host already exists"),
        "Use the existing tenant for that backend/DID host pair, or choose a different backend_base_url + did_host combination.",
    )
}

pub fn product_cache_dir() -> anyhow::Result<String> {
    let (product_home_dir, _) = current_workspace_home()?;
    Ok(path_string(&product_home_dir.join("cache")))
}

pub fn list_tenants() -> anyhow::Result<Vec<TenantProfile>> {
    let (product_home_dir, _) = current_workspace_home()?;
    archive_legacy_product_root(&product_home_dir)?;
    ensure_tenant_state(&product_home_dir)?;
    Ok(load_tenant_registry(&product_home_dir)?.tenants)
}

pub fn current_tenant_context() -> anyhow::Result<TenantContext> {
    let (product_home_dir, _) = current_workspace_home()?;
    archive_legacy_product_root(&product_home_dir)?;
    resolve_active_tenant(&product_home_dir, &Overrides::default())
}

pub fn tenant_context_for_resolved(resolved: &Resolved) -> anyhow::Result<TenantContext> {
    let (product_home_dir, _) = current_workspace_home()?;
    archive_legacy_product_root(&product_home_dir)?;
    ensure_tenant_state(&product_home_dir)?;
    let active_source = resolved
        .sources
        .get("active_tenant")
        .cloned()
        .unwrap_or(ValueSource {
            source: "default".to_string(),
            key: String::new(),
            value: CHINA_TENANT_NAME.to_string(),
        });
    let registry = load_tenant_registry(&product_home_dir)?;
    let requested_name = normalize_tenant_name(&active_source.value)?;
    let name = resolve_tenant_alias(&registry, &requested_name);
    let profile = registry
        .tenants
        .iter()
        .find(|tenant| tenant.name == name)
        .cloned()
        .ok_or_else(|| tenant_not_found_error("active tenant", &name))?;
    Ok(tenant_context(
        &product_home_dir,
        profile,
        active_source.source,
    ))
}

pub fn create_tenant(input: TenantCreateInput) -> anyhow::Result<TenantContext> {
    let (product_home_dir, mut registry, profile) = prepare_tenant_create(input)?;
    write_tenant_config(&product_home_dir, &profile)?;
    registry.tenants.push(profile.clone());
    registry
        .tenants
        .sort_by(|left, right| left.name.cmp(&right.name));
    write_tenant_registry(&product_home_dir, &registry)?;
    Ok(tenant_context(
        &product_home_dir,
        profile,
        "created".to_string(),
    ))
}

pub fn setup_tenant(input: TenantCreateInput) -> anyhow::Result<TenantSetupResult> {
    let (product_home_dir, mut registry, profile, action) = prepare_tenant_setup(input)?;
    if action == "created" {
        write_tenant_config(&product_home_dir, &profile)?;
        registry.tenants.push(profile.clone());
        registry
            .tenants
            .sort_by(|left, right| left.name.cmp(&right.name));
        write_tenant_registry(&product_home_dir, &registry)?;
    }
    activate_tenant(&product_home_dir, &profile.name)?;
    Ok(TenantSetupResult {
        action,
        tenant: tenant_context(&product_home_dir, profile, "global_config".to_string()),
    })
}

pub fn preview_setup_tenant(input: TenantCreateInput) -> anyhow::Result<TenantSetupResult> {
    let (product_home_dir, _registry, profile, action) = prepare_tenant_setup(input)?;
    Ok(TenantSetupResult {
        action,
        tenant: tenant_context(&product_home_dir, profile, "planned".to_string()),
    })
}

pub fn preview_create_tenant(input: TenantCreateInput) -> anyhow::Result<TenantContext> {
    let (product_home_dir, _registry, profile) = prepare_tenant_create(input)?;
    Ok(tenant_context(
        &product_home_dir,
        profile,
        "planned".to_string(),
    ))
}

pub fn preview_use_tenant(name: &str) -> anyhow::Result<TenantContext> {
    let (product_home_dir, _) = current_workspace_home()?;
    archive_legacy_product_root(&product_home_dir)?;
    ensure_tenant_state(&product_home_dir)?;
    let registry = load_tenant_registry(&product_home_dir)?;
    let requested_name = normalize_tenant_name(name)?;
    let name = resolve_tenant_alias(&registry, &requested_name);
    let profile = registry
        .tenants
        .iter()
        .find(|tenant| tenant.name == name)
        .cloned()
        .ok_or_else(|| tenant_not_found_error("tenant", &name))?;
    Ok(tenant_context(
        &product_home_dir,
        profile,
        "planned".to_string(),
    ))
}

fn prepare_tenant_create(
    input: TenantCreateInput,
) -> anyhow::Result<(PathBuf, TenantRegistry, TenantProfile)> {
    let (product_home_dir, _) = current_workspace_home()?;
    archive_legacy_product_root(&product_home_dir)?;
    ensure_tenant_state(&product_home_dir)?;
    let name = normalize_tenant_name(&input.name)?;
    let backend_base_url = normalize_base_url(&input.backend_base_url);
    validate_service_base_url(&backend_base_url)?;
    let did_host = normalize_did_domain(&input.did_host)?;
    let registry = load_tenant_registry(&product_home_dir)?;
    if registry.tenants.iter().any(|tenant| tenant.name == name)
        || registry.aliases.contains_key(&name)
    {
        return Err(WorkspaceConfigError::conflict(
            format!("tenant {name:?} already exists"),
            "Run `awiki-cli tenant list` to inspect existing tenants, or choose a different tenant name.",
        )
        .into());
    }
    if registry
        .tenants
        .iter()
        .any(|tenant| tenant.backend_base_url == backend_base_url && tenant.did_host == did_host)
    {
        return Err(duplicate_tenant_endpoint_error("a ").into());
    }
    let now = now_compact();
    let profile = TenantProfile {
        name: name.clone(),
        display_name: input
            .display_name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(&name)
            .to_string(),
        backend_base_url,
        did_host,
        dir_name: name.clone(),
        kind: TenantKind::Custom,
        created_at: now.clone(),
        updated_at: now,
    };
    Ok((product_home_dir, registry, profile))
}

fn prepare_tenant_setup(
    input: TenantCreateInput,
) -> anyhow::Result<(PathBuf, TenantRegistry, TenantProfile, String)> {
    let (product_home_dir, _) = current_workspace_home()?;
    archive_legacy_product_root(&product_home_dir)?;
    ensure_tenant_state(&product_home_dir)?;
    let name = normalize_tenant_name(&input.name)?;
    let backend_base_url = normalize_base_url(&input.backend_base_url);
    validate_service_base_url(&backend_base_url)?;
    let did_host = normalize_did_domain(&input.did_host)?;
    let registry = load_tenant_registry(&product_home_dir)?;

    if registry.aliases.contains_key(&name) {
        let canonical = resolve_tenant_alias(&registry, &name);
        let existing = registry
            .tenants
            .iter()
            .find(|tenant| tenant.name == canonical)
            .expect("validated tenant alias")
            .clone();
        if existing.backend_base_url == backend_base_url && existing.did_host == did_host {
            return Ok((product_home_dir, registry, existing, "reused".to_string()));
        }
        return Err(WorkspaceConfigError::conflict(
            format!("tenant alias {name:?} already points to tenant {canonical:?}"),
            "Use the canonical tenant name shown by `awiki-cli tenant list`.",
        )
        .into());
    }
    if let Some(existing) = registry.tenants.iter().find(|tenant| tenant.name == name) {
        if existing.backend_base_url == backend_base_url && existing.did_host == did_host {
            let existing = existing.clone();
            return Ok((product_home_dir, registry, existing, "reused".to_string()));
        }
        return Err(WorkspaceConfigError::conflict(
            format!(
                "tenant {name:?} already exists with a different backend_base_url or did_host"
            ),
            "Use the existing tenant endpoints, choose a different tenant name, or inspect the tenant with `awiki-cli tenant list`. Existing tenant data is never reconfigured by `tenant setup`.",
        )
        .into());
    }
    if let Some(existing) = registry
        .tenants
        .iter()
        .find(|tenant| tenant.backend_base_url == backend_base_url && tenant.did_host == did_host)
    {
        let existing = existing.clone();
        return Ok((product_home_dir, registry, existing, "reused".to_string()));
    }

    let now = now_compact();
    let profile = TenantProfile {
        name: name.clone(),
        display_name: input
            .display_name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(&name)
            .to_string(),
        backend_base_url,
        did_host,
        dir_name: name,
        kind: TenantKind::Custom,
        created_at: now.clone(),
        updated_at: now,
    };
    Ok((product_home_dir, registry, profile, "created".to_string()))
}

pub fn use_tenant(name: &str) -> anyhow::Result<TenantContext> {
    let (product_home_dir, _) = current_workspace_home()?;
    archive_legacy_product_root(&product_home_dir)?;
    ensure_tenant_state(&product_home_dir)?;
    let registry = load_tenant_registry(&product_home_dir)?;
    let requested_name = normalize_tenant_name(name)?;
    let name = resolve_tenant_alias(&registry, &requested_name);
    let profile = registry
        .tenants
        .iter()
        .find(|tenant| tenant.name == name)
        .cloned()
        .ok_or_else(|| tenant_not_found_error("tenant", &name))?;
    activate_tenant(&product_home_dir, &name)?;
    Ok(tenant_context(
        &product_home_dir,
        profile,
        "global_config".to_string(),
    ))
}

fn activate_tenant(product_home_dir: &Path, name: &str) -> anyhow::Result<()> {
    let registry = load_tenant_registry(product_home_dir)?;
    let name = resolve_tenant_alias(&registry, name);
    let mut global = load_global_config(product_home_dir)?;
    global.schema_version = 1;
    global.active_tenant = name;
    write_global_config(product_home_dir, &global)
}

pub fn reconfigure_tenant(
    name: &str,
    backend_base_url: &str,
    did_host: &str,
) -> anyhow::Result<TenantContext> {
    let (product_home_dir, mut registry, index, profile) =
        prepare_tenant_reconfigure(name, backend_base_url, did_host)?;
    registry.tenants[index] = profile.clone();
    write_tenant_config(&product_home_dir, &profile)?;
    write_tenant_registry(&product_home_dir, &registry)?;
    Ok(tenant_context(
        &product_home_dir,
        profile,
        "tenant_registry".to_string(),
    ))
}

pub fn preview_reconfigure_tenant(
    name: &str,
    backend_base_url: &str,
    did_host: &str,
) -> anyhow::Result<TenantContext> {
    let (product_home_dir, _registry, _index, profile) =
        prepare_tenant_reconfigure(name, backend_base_url, did_host)?;
    Ok(tenant_context(
        &product_home_dir,
        profile,
        "planned".to_string(),
    ))
}

fn prepare_tenant_reconfigure(
    name: &str,
    backend_base_url: &str,
    did_host: &str,
) -> anyhow::Result<(PathBuf, TenantRegistry, usize, TenantProfile)> {
    let (product_home_dir, _) = current_workspace_home()?;
    archive_legacy_product_root(&product_home_dir)?;
    ensure_tenant_state(&product_home_dir)?;
    let requested_name = normalize_tenant_name(name)?;
    let backend_base_url = normalize_base_url(backend_base_url);
    validate_service_base_url(&backend_base_url)?;
    let did_host = normalize_did_domain(did_host)?;
    let registry = load_tenant_registry(&product_home_dir)?;
    let name = resolve_tenant_alias(&registry, &requested_name);
    let Some(index) = registry
        .tenants
        .iter()
        .position(|tenant| tenant.name == name)
    else {
        return Err(tenant_not_found_error("tenant", &name).into());
    };
    if registry.tenants[index].kind == TenantKind::BuiltIn {
        return Err(WorkspaceConfigError::conflict(
            format!("official tenant {name:?} cannot be reconfigured"),
            "Create a custom tenant for different backend or DID endpoints.",
        )
        .into());
    }
    if tenant_has_data(&product_home_dir, &registry.tenants[index])? {
        return Err(WorkspaceConfigError::conflict(
            format!(
                "tenant {name:?} already has local data; create a new tenant instead of changing its backend_base_url or did_host"
            ),
            "Create a new tenant for the new backend/DID host. Existing tenant-local identity and database data are immutable.",
        )
        .into());
    }
    if registry
        .tenants
        .iter()
        .enumerate()
        .any(|(other_index, tenant)| {
            other_index != index
                && tenant.backend_base_url == backend_base_url
                && tenant.did_host == did_host
        })
    {
        return Err(duplicate_tenant_endpoint_error("another ").into());
    }
    let mut profile = registry.tenants[index].clone();
    profile.backend_base_url = backend_base_url;
    profile.did_host = did_host;
    profile.updated_at = now_compact();
    Ok((product_home_dir, registry, index, profile))
}

fn resolve_active_tenant(
    product_home_dir: &Path,
    overrides: &Overrides,
) -> anyhow::Result<TenantContext> {
    ensure_tenant_state(product_home_dir)?;
    let registry = load_tenant_registry(product_home_dir)?;
    let global = load_global_config(product_home_dir)?;
    let (requested_active, active_source) = if overrides.tenant_changed {
        (
            normalize_tenant_name(&overrides.tenant)?,
            "flag".to_string(),
        )
    } else if !global.active_tenant.trim().is_empty() {
        (
            normalize_tenant_name(&global.active_tenant)?,
            "global_config".to_string(),
        )
    } else {
        (CHINA_TENANT_NAME.to_string(), "default".to_string())
    };
    let active = resolve_tenant_alias(&registry, &requested_active);
    let profile = registry
        .tenants
        .iter()
        .find(|tenant| tenant.name == active)
        .cloned()
        .ok_or_else(|| tenant_not_found_error("active tenant", &active))?;
    Ok(tenant_context(product_home_dir, profile, active_source))
}

fn tenant_context(
    product_home_dir: &Path,
    profile: TenantProfile,
    active_source: String,
) -> TenantContext {
    let tenant_dir = tenant_dir(product_home_dir, &profile);
    TenantContext {
        active: profile.name.clone(),
        active_source,
        profile,
        registry_file: path_string(&tenant_registry_path(product_home_dir)),
        global_config_file: path_string(&global_config_path(product_home_dir)),
        tenants_dir: path_string(&product_home_dir.join(TENANTS_DIR_NAME)),
        tenant_dir: path_string(&tenant_dir),
    }
}

fn ensure_tenant_state(product_home_dir: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(product_home_dir)
        .map_err(|err| anyhow::anyhow!("create awiki-cli home: {err}"))?;
    fs::create_dir_all(product_home_dir.join(TENANTS_DIR_NAME))
        .map_err(|err| anyhow::anyhow!("create tenant directory: {err}"))?;
    if !tenant_registry_path(product_home_dir).exists() {
        let (profiles, aliases, active_tenant) = default_tenant_profiles()?;
        for profile in &profiles {
            write_tenant_config(product_home_dir, profile)?;
        }
        write_tenant_registry(
            product_home_dir,
            &TenantRegistry {
                schema_version: TENANT_REGISTRY_SCHEMA_VERSION,
                official_catalog_version: OFFICIAL_TENANT_CATALOG_VERSION,
                aliases,
                tenants: profiles,
            },
        )?;
        write_global_config(
            product_home_dir,
            &GlobalConfig {
                schema_version: 1,
                active_tenant,
            },
        )?;
    } else if !global_config_path(product_home_dir).exists() {
        write_global_config(
            product_home_dir,
            &GlobalConfig {
                schema_version: 1,
                active_tenant: CHINA_TENANT_NAME.to_string(),
            },
        )?;
    }
    let _ = load_tenant_registry(product_home_dir)?;
    Ok(())
}

fn default_tenant_profiles(
) -> anyhow::Result<(Vec<TenantProfile>, BTreeMap<String, String>, String)> {
    let configured_base_url = env::var(DEFAULT_SERVICE_BASE_URL_ENV).unwrap_or_default();
    let configured_did_host = env::var(DEFAULT_DID_DOMAIN_ENV).unwrap_or_default();
    let has_base_url = !configured_base_url.trim().is_empty();
    let has_did_host = !configured_did_host.trim().is_empty();
    if has_base_url != has_did_host {
        anyhow::bail!(
            "{DEFAULT_SERVICE_BASE_URL_ENV} and {DEFAULT_DID_DOMAIN_ENV} must be configured together"
        );
    }
    let configured_endpoint = if has_base_url {
        let backend_base_url = normalize_base_url(&configured_base_url);
        validate_service_base_url(&backend_base_url)?;
        let did_host = normalize_did_domain(&configured_did_host)?;
        Some((backend_base_url, did_host))
    } else {
        None
    };
    let now = now_compact();
    let mut china = official_tenant_profile(CHINA_TENANT_NAME, &now);
    let mut global = official_tenant_profile(GLOBAL_TENANT_NAME, &now);
    let mut aliases = BTreeMap::new();
    let (profiles, active) = match configured_endpoint {
        None => {
            aliases.insert(
                DEFAULT_TENANT_ALIAS.to_string(),
                CHINA_TENANT_NAME.to_string(),
            );
            (vec![china, global], CHINA_TENANT_NAME.to_string())
        }
        Some((backend_base_url, did_host)) if is_endpoint(&china, &backend_base_url, &did_host) => {
            china.dir_name = DEFAULT_TENANT_ALIAS.to_string();
            aliases.insert(
                DEFAULT_TENANT_ALIAS.to_string(),
                CHINA_TENANT_NAME.to_string(),
            );
            (vec![china, global], CHINA_TENANT_NAME.to_string())
        }
        Some((backend_base_url, did_host))
            if is_endpoint(&global, &backend_base_url, &did_host) =>
        {
            global.dir_name = DEFAULT_TENANT_ALIAS.to_string();
            aliases.insert(
                DEFAULT_TENANT_ALIAS.to_string(),
                GLOBAL_TENANT_NAME.to_string(),
            );
            (vec![china, global], GLOBAL_TENANT_NAME.to_string())
        }
        Some((backend_base_url, did_host)) => {
            let custom = TenantProfile {
                name: DEFAULT_TENANT_ALIAS.to_string(),
                display_name: "AWiki".to_string(),
                backend_base_url,
                did_host,
                dir_name: DEFAULT_TENANT_ALIAS.to_string(),
                kind: TenantKind::Custom,
                created_at: now.clone(),
                updated_at: now.clone(),
            };
            (
                vec![china, custom, global],
                DEFAULT_TENANT_ALIAS.to_string(),
            )
        }
    };
    Ok((profiles, aliases, active))
}

fn load_tenant_registry(product_home_dir: &Path) -> anyhow::Result<TenantRegistry> {
    let path = tenant_registry_path(product_home_dir);
    let raw = fs::read_to_string(&path)
        .map_err(|err| anyhow::anyhow!("read tenant registry {}: {err}", path.display()))?;
    let registry: TenantRegistry = serde_json::from_str(&raw)
        .map_err(|err| anyhow::anyhow!("parse tenant registry {}: {err}", path.display()))?;
    if registry.schema_version != 1 && registry.schema_version != TENANT_REGISTRY_SCHEMA_VERSION {
        anyhow::bail!(
            "unsupported tenant registry schema_version {}",
            registry.schema_version
        );
    }
    let global = load_global_config(product_home_dir)?;
    let original_registry = registry.clone();
    let (mut upgraded, canonical_active, changed) =
        reconcile_tenant_registry(registry, &global.active_tenant)?;
    sort_tenant_profiles(&mut upgraded.tenants);
    if changed {
        if original_registry.schema_version == 1 {
            if let Err(error) = backup_v1_control_files(product_home_dir) {
                eprintln!("warning: tenant registry v2 migration skipped: {error}");
                return Ok(original_registry);
            }
        }
        if let Err(error) = write_tenant_registry(product_home_dir, &upgraded) {
            eprintln!("warning: tenant registry v2 migration skipped: {error}");
            return Ok(original_registry);
        }
    }
    if !canonical_active.is_empty() && global.active_tenant != canonical_active {
        let canonical_global = GlobalConfig {
            schema_version: 1,
            active_tenant: canonical_active,
        };
        if let Err(error) = write_global_config(product_home_dir, &canonical_global) {
            eprintln!("warning: active tenant migration will retry: {error}");
        }
    }
    Ok(upgraded)
}

fn official_tenant_profile(name: &str, timestamp: &str) -> TenantProfile {
    let (display_name, backend_base_url, did_host) = match name {
        CHINA_TENANT_NAME => (
            CHINA_TENANT_DISPLAY_NAME,
            CHINA_SERVICE_BASE_URL,
            CHINA_DID_DOMAIN,
        ),
        GLOBAL_TENANT_NAME => (
            GLOBAL_TENANT_DISPLAY_NAME,
            GLOBAL_SERVICE_BASE_URL,
            GLOBAL_DID_DOMAIN,
        ),
        _ => unreachable!("official tenant name"),
    };
    TenantProfile {
        name: name.to_string(),
        display_name: display_name.to_string(),
        backend_base_url: backend_base_url.to_string(),
        did_host: did_host.to_string(),
        dir_name: name.to_string(),
        kind: TenantKind::BuiltIn,
        created_at: timestamp.to_string(),
        updated_at: timestamp.to_string(),
    }
}

fn is_endpoint(profile: &TenantProfile, backend_base_url: &str, did_host: &str) -> bool {
    profile.backend_base_url == backend_base_url && profile.did_host == did_host
}

fn reconcile_tenant_registry(
    mut registry: TenantRegistry,
    active_tenant: &str,
) -> anyhow::Result<(TenantRegistry, String, bool)> {
    let original = registry.clone();
    let now = now_compact();
    for canonical_name in [CHINA_TENANT_NAME, GLOBAL_TENANT_NAME] {
        let expected = official_tenant_profile(canonical_name, &now);
        let matches = registry
            .tenants
            .iter()
            .enumerate()
            .filter_map(|(index, tenant)| {
                is_endpoint(tenant, &expected.backend_base_url, &expected.did_host).then_some(index)
            })
            .collect::<Vec<_>>();
        if matches.len() > 1 {
            anyhow::bail!(
                "multiple tenant profiles use official endpoint {} / {}",
                expected.backend_base_url,
                expected.did_host
            );
        }
        if let Some(index) = matches.first().copied() {
            let old_name = registry.tenants[index].name.clone();
            if old_name != canonical_name {
                if registry
                    .tenants
                    .iter()
                    .enumerate()
                    .any(|(other, tenant)| other != index && tenant.name == canonical_name)
                {
                    anyhow::bail!(
                        "tenant name {canonical_name:?} conflicts with the official catalog"
                    );
                }
                registry.tenants[index].name = canonical_name.to_string();
                registry
                    .aliases
                    .insert(old_name, canonical_name.to_string());
            }
            let profile = &mut registry.tenants[index];
            profile.kind = TenantKind::BuiltIn;
            profile.display_name = expected.display_name;
            profile.backend_base_url = expected.backend_base_url;
            profile.did_host = expected.did_host;
            if profile.dir_name.trim().is_empty() {
                profile.dir_name = canonical_name.to_string();
            }
            if profile.created_at.trim().is_empty() {
                profile.created_at = now.clone();
            }
            if profile.updated_at.trim().is_empty() || original.schema_version == 1 {
                profile.updated_at = now.clone();
            }
        } else {
            registry.tenants.push(expected);
        }
    }
    registry.schema_version = TENANT_REGISTRY_SCHEMA_VERSION;
    registry.official_catalog_version = OFFICIAL_TENANT_CATALOG_VERSION;
    registry.aliases.retain(|alias, target| alias != target);
    let requested_active = if active_tenant.trim().is_empty() {
        CHINA_TENANT_NAME
    } else {
        active_tenant.trim()
    };
    let canonical_active = resolve_tenant_alias(&registry, requested_active);
    if !registry
        .tenants
        .iter()
        .any(|tenant| tenant.name == canonical_active)
    {
        anyhow::bail!("active tenant {requested_active:?} does not exist");
    }
    for (alias, target) in &registry.aliases {
        if !registry.tenants.iter().any(|tenant| tenant.name == *target) {
            anyhow::bail!("tenant alias {alias:?} points to missing tenant {target:?}");
        }
    }
    sort_tenant_profiles(&mut registry.tenants);
    let changed = registry != original;
    Ok((registry, canonical_active, changed))
}

fn sort_tenant_profiles(tenants: &mut [TenantProfile]) {
    tenants.sort_by(|left, right| {
        let rank = |name: &str| match name {
            CHINA_TENANT_NAME => 0,
            GLOBAL_TENANT_NAME => 1,
            _ => 2,
        };
        rank(&left.name)
            .cmp(&rank(&right.name))
            .then_with(|| left.name.cmp(&right.name))
    });
}

fn resolve_tenant_alias(registry: &TenantRegistry, name: &str) -> String {
    registry
        .aliases
        .get(name)
        .cloned()
        .unwrap_or_else(|| name.to_string())
}

fn backup_v1_control_files(product_home_dir: &Path) -> anyhow::Result<()> {
    for source in [
        tenant_registry_path(product_home_dir),
        global_config_path(product_home_dir),
    ] {
        let backup = source.with_extension("json.v1.bak");
        if backup.exists() {
            continue;
        }
        fs::copy(&source, &backup)
            .map_err(|error| anyhow::anyhow!("back up {}: {error}", source.display()))?;
        if let Some(parent) = backup.parent() {
            durable_fs::sync_directory(parent)
                .map_err(|error| anyhow::anyhow!("sync backup directory: {error}"))?;
        }
    }
    Ok(())
}

fn write_tenant_registry(product_home_dir: &Path, registry: &TenantRegistry) -> anyhow::Result<()> {
    write_json_file(&tenant_registry_path(product_home_dir), registry)
}

fn load_global_config(product_home_dir: &Path) -> anyhow::Result<GlobalConfig> {
    let path = global_config_path(product_home_dir);
    let raw = fs::read_to_string(&path)
        .map_err(|err| anyhow::anyhow!("read global config {}: {err}", path.display()))?;
    let mut config: GlobalConfig = serde_json::from_str(&raw)
        .map_err(|err| anyhow::anyhow!("parse global config {}: {err}", path.display()))?;
    config.schema_version = 1;
    Ok(config)
}

fn write_global_config(product_home_dir: &Path, config: &GlobalConfig) -> anyhow::Result<()> {
    write_json_file(&global_config_path(product_home_dir), config)
}

fn write_tenant_config(product_home_dir: &Path, profile: &TenantProfile) -> anyhow::Result<()> {
    let tenant_dir = tenant_dir(product_home_dir, profile);
    fs::create_dir_all(&tenant_dir)
        .map_err(|err| anyhow::anyhow!("create tenant directory: {err}"))?;
    let home = try_home_dir();
    let paths = build_paths(home.as_deref(), &tenant_dir);
    let (mut config, exists, error) = read_file_config(&paths.config_file);
    if exists && error.is_empty() {
        config.schema_version = CONFIG_SCHEMA_VERSION;
        rewrite_tenant_services_config(&mut config.services, profile);
        return write_file_config_raw(&paths.config_file, config);
    }
    let resolved = Resolved {
        paths: paths.clone(),
        config_schema_version: CONFIG_SCHEMA_VERSION,
        active_identity: String::new(),
        runtime_mode: DEFAULT_RUNTIME_MODE.to_string(),
        runtime_socket_path: default_runtime_bridge_path(&paths),
        runtime_listener_enabled: DEFAULT_LISTENER_ENABLED,
        runtime_listener_auto_install: DEFAULT_LISTENER_AUTO_INSTALL,
        runtime_listener_auto_start: DEFAULT_LISTENER_AUTO_START,
        host_notify_enabled: DEFAULT_HOST_NOTIFY_ENABLED,
        host_notify_sink: DEFAULT_HOST_NOTIFY_SINK.to_string(),
        host_notify_file_path: path_string(&tenant_dir.join("logs").join(DEFAULT_HOST_NOTIFY_FILE)),
        host_notify_openclaw_hook_url: DEFAULT_OPENCLAW_HOOK_URL.to_string(),
        host_notify_openclaw_agent_id: DEFAULT_OPENCLAW_AGENT_ID.to_string(),
        host_notify_openclaw_hook_name: DEFAULT_OPENCLAW_HOOK_NAME.to_string(),
        host_notify_hermes_notify_url: DEFAULT_HERMES_NOTIFY_URL.to_string(),
        host_notify_hermes_deliver: DEFAULT_HERMES_DELIVER_TARGET.to_string(),
        output_format: DEFAULT_OUTPUT_FORMAT.to_string(),
        no_color: false,
        service_base_url: profile.backend_base_url.clone(),
        user_service_endpoint: profile.backend_base_url.clone(),
        message_service_endpoint: profile.backend_base_url.clone(),
        did_domain: profile.did_host.clone(),
        anp_service_endpoint: derive_anp_service_endpoint(&profile.backend_base_url),
        anp_service_did: derive_anp_service_did(&profile.backend_base_url),
        mail_service_url: profile.backend_base_url.clone(),
        ca_bundle: String::new(),
        update_disable_strict_version: false,
        update_metadata_cache_ttl_seconds: 0,
        config_exists: false,
        config_error: String::new(),
        env_hits: Vec::new(),
        sources: BTreeMap::new(),
    };
    write_file_config(&paths.config_file, &resolved)
}

fn rewrite_tenant_services_config(config: &mut ServicesConfig, profile: &TenantProfile) {
    let _ = profile;
    config.service_base_url.clear();
    config.user_service_endpoint.clear();
    config.message_service_endpoint.clear();
    config.did_domain.clear();
}

fn write_json_file<T: Serialize>(path: &Path, value: &T) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| anyhow::anyhow!("create config directory: {err}"))?;
    }
    let raw = serde_json::to_vec_pretty(value)?;
    let temp_path = path.with_extension(format!("tmp-{}-{}", std::process::id(), now_compact()));
    fs::write(&temp_path, raw).map_err(|err| anyhow::anyhow!("write temp config: {err}"))?;
    fs::rename(&temp_path, path).map_err(|err| anyhow::anyhow!("replace config: {err}"))?;
    if let Some(parent) = path.parent() {
        durable_fs::sync_directory(parent)
            .map_err(|err| anyhow::anyhow!("sync config dir: {err}"))?;
    }
    Ok(())
}

fn archive_legacy_product_root(product_home_dir: &Path) -> anyhow::Result<()> {
    if !legacy_root_has_active_state(product_home_dir) {
        return Ok(());
    }
    fs::create_dir_all(product_home_dir)
        .map_err(|err| anyhow::anyhow!("create awiki-cli home: {err}"))?;
    let lock_dir = product_home_dir.join(LEGACY_ARCHIVE_LOCK_DIR_NAME);
    let owns_lock = acquire_legacy_archive_lock(&lock_dir)?;
    if !owns_lock {
        return Ok(());
    }
    let _guard = LegacyArchiveLockGuard { path: lock_dir };
    if !legacy_root_has_active_state(product_home_dir) {
        return Ok(());
    }
    let archive_root = product_home_dir.join(LEGACY_ARCHIVE_DIR_NAME);
    fs::create_dir_all(&archive_root)
        .map_err(|err| anyhow::anyhow!("create legacy archive directory: {err}"))?;
    let archive_dir = unique_archive_dir(&archive_root);
    fs::create_dir_all(&archive_dir)
        .map_err(|err| anyhow::anyhow!("create legacy archive target: {err}"))?;
    for name in [
        CONFIG_FILE_NAME,
        "config.json",
        "identities",
        "data",
        "runtime",
        "cache",
        "logs",
    ] {
        let source = product_home_dir.join(name);
        match fs::rename(&source, archive_dir.join(name)) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => anyhow::bail!("archive legacy awiki-cli state {}: {err}", source.display()),
        }
    }
    durable_fs::sync_directory(product_home_dir)
        .map_err(|err| anyhow::anyhow!("sync awiki-cli home after legacy archive: {err}"))?;
    Ok(())
}

struct LegacyArchiveLockGuard {
    path: PathBuf,
}

impl Drop for LegacyArchiveLockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn acquire_legacy_archive_lock(lock_dir: &Path) -> anyhow::Result<bool> {
    for _ in 0..200 {
        match fs::create_dir(lock_dir) {
            Ok(()) => return Ok(true),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                std::thread::sleep(std::time::Duration::from_millis(10));
                if !legacy_root_has_active_state(lock_dir.parent().unwrap_or(lock_dir)) {
                    return Ok(false);
                }
            }
            Err(err) => anyhow::bail!("create legacy archive lock {}: {err}", lock_dir.display()),
        }
    }
    anyhow::bail!("timed out waiting for legacy awiki-cli state archive lock")
}

fn legacy_root_has_active_state(product_home_dir: &Path) -> bool {
    if !product_home_dir.exists() {
        return false;
    }
    if product_home_dir.join(TENANTS_DIR_NAME).exists() {
        return false;
    }
    [
        CONFIG_FILE_NAME,
        "config.json",
        "identities",
        "data",
        "runtime",
    ]
    .iter()
    .any(|name| product_home_dir.join(name).exists())
}

fn unique_archive_dir(archive_root: &Path) -> PathBuf {
    for attempt in 0..1000 {
        let suffix = if attempt == 0 {
            String::new()
        } else {
            format!("-{attempt}")
        };
        let candidate = archive_root.join(format!("{}{}", now_compact(), suffix));
        if !candidate.exists() {
            return candidate;
        }
    }
    archive_root.join(format!("{}-{}", now_compact(), std::process::id()))
}

fn tenant_has_data(product_home_dir: &Path, profile: &TenantProfile) -> anyhow::Result<bool> {
    let tenant_dir = tenant_dir(product_home_dir, profile);
    for relative in [Path::new("identities"), Path::new("data")] {
        let path = tenant_dir.join(relative);
        if path_has_data(&path)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn path_has_data(path: &Path) -> anyhow::Result<bool> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => anyhow::bail!("inspect tenant data {}: {err}", path.display()),
    };
    if metadata.is_file() {
        return Ok(metadata.len() > 0);
    }
    if !metadata.is_dir() {
        return Ok(false);
    }
    for entry in fs::read_dir(path)
        .map_err(|err| anyhow::anyhow!("inspect tenant data {}: {err}", path.display()))?
    {
        let entry = entry.map_err(|err| {
            anyhow::anyhow!("inspect tenant data entry {}: {err}", path.display())
        })?;
        if path_has_data(&entry.path())? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn tenant_dir(product_home_dir: &Path, profile: &TenantProfile) -> PathBuf {
    product_home_dir
        .join(TENANTS_DIR_NAME)
        .join(profile.dir_name.trim())
}

fn tenant_registry_path(product_home_dir: &Path) -> PathBuf {
    product_home_dir
        .join(TENANTS_DIR_NAME)
        .join(TENANT_REGISTRY_FILE_NAME)
}

fn global_config_path(product_home_dir: &Path) -> PathBuf {
    product_home_dir.join(GLOBAL_CONFIG_FILE_NAME)
}

fn normalize_tenant_name(raw: &str) -> Result<String, WorkspaceConfigError> {
    let value = raw.trim().to_ascii_lowercase();
    if value.is_empty() {
        return Err(WorkspaceConfigError::invalid_argument(
            "tenant name is required",
            tenant_name_hint(),
        ));
    }
    if value.len() > 64 {
        return Err(WorkspaceConfigError::invalid_argument(
            "tenant name must be at most 64 characters",
            tenant_name_hint(),
        ));
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
    {
        return Err(WorkspaceConfigError::invalid_argument(
            "tenant name may only contain ASCII letters, numbers, and '-'",
            tenant_name_hint(),
        ));
    }
    if value.starts_with('-') || value.ends_with('-') || value.contains("--") {
        return Err(WorkspaceConfigError::invalid_argument(
            "tenant name must not start or end with '-' or contain '--'",
            tenant_name_hint(),
        ));
    }
    Ok(value)
}

fn validate_service_base_url(value: &str) -> Result<(), WorkspaceConfigError> {
    im_core::ServiceEndpoint::parse(value.to_string())
        .map(|_| ())
        .map_err(|err| {
            WorkspaceConfigError::invalid_argument(
                format!("backend_base_url is invalid: {err}"),
                backend_base_url_hint(),
            )
        })
}

fn now_compact() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .to_string()
}

fn parse_file_config(raw: &str) -> Result<FileConfig, String> {
    let trimmed = raw.trim_start();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return serde_json::from_str::<FileConfig>(raw).map_err(|err| err.to_string());
    }
    let mut config = FileConfig::default();
    let mut stack: Vec<(usize, String)> = Vec::new();
    for line in raw.lines() {
        let without_comment = strip_yaml_inline_comment(line).trim_end();
        if without_comment.trim().is_empty() {
            continue;
        }
        let indent = without_comment.chars().take_while(|ch| *ch == ' ').count();
        while stack.last().is_some_and(|(level, _)| *level >= indent) {
            stack.pop();
        }
        let trimmed = without_comment.trim_start();
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            return Err(format!("yaml: missing mapping key in line {line:?}"));
        }
        let raw_value = value.trim();
        if raw_value.is_empty() {
            stack.push((indent, key.to_string()));
            continue;
        }
        let value = strip_yaml_scalar(raw_value)?;
        let mut path: Vec<String> = stack.iter().map(|(_, key)| key.clone()).collect();
        path.push(key.to_string());
        set_config_value(&mut config, &path, &value);
    }
    Ok(config)
}

fn validate_deprecated_config_fields(path: &str) -> anyhow::Result<()> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => anyhow::bail!("read config yaml for policy validation: {err}"),
    };
    let deprecated_fields = collect_deprecated_config_fields(&raw);
    if deprecated_fields.is_empty() {
        return Ok(());
    }
    Err(WorkspaceConfigError::invalid_config(
        format!(
            "deprecated config.yaml fields are no longer supported: {}",
            deprecated_fields.join(", ")
        ),
        "Remove deprecated services.* backend/DID fields from tenant config.yaml. Manage backend_base_url and did_host with `awiki-cli tenant create` or `awiki-cli tenant reconfigure`.",
    )
    .into())
}

fn collect_deprecated_config_fields(raw: &str) -> Vec<String> {
    let trimmed = raw.trim_start();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return collect_deprecated_config_fields_json(raw);
    }
    collect_deprecated_config_fields_yaml(raw)
}

fn collect_deprecated_config_fields_json(raw: &str) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<Value>(raw) else {
        return Vec::new();
    };
    let Some(services) = value.get("services").and_then(Value::as_object) else {
        return Vec::new();
    };
    deprecated_service_keys()
        .iter()
        .filter(|key| services.contains_key(**key))
        .map(|key| format!("services.{key}"))
        .collect()
}

fn collect_deprecated_config_fields_yaml(raw: &str) -> Vec<String> {
    let mut stack: Vec<(usize, String)> = Vec::new();
    let mut deprecated = Vec::new();
    for line in raw.lines() {
        let without_comment = strip_yaml_inline_comment(line).trim_end();
        if without_comment.trim().is_empty() {
            continue;
        }
        let indent = without_comment.chars().take_while(|ch| *ch == ' ').count();
        while stack.last().is_some_and(|(level, _)| *level >= indent) {
            stack.pop();
        }
        let trimmed = without_comment.trim_start();
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        if stack.len() == 1 && stack[0].1 == "services" && deprecated_service_keys().contains(&key)
        {
            deprecated.push(format!("services.{key}"));
        }
        if value.trim().is_empty() {
            stack.push((indent, key.to_string()));
            continue;
        }
    }
    deprecated
}

fn strip_yaml_inline_comment(line: &str) -> &str {
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;
    for (idx, ch) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if in_double && ch == '\\' {
            escaped = true;
            continue;
        }
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '#' if !in_single && !in_double => return &line[..idx],
            _ => {}
        }
    }
    line
}

fn deprecated_service_keys() -> [&'static str; 7] {
    [
        "service_base_url",
        "user_service_endpoint",
        "message_service_endpoint",
        "did_domain",
        "user_service_url",
        "message_service_url",
        "message_service_ws_url",
    ]
}

fn strip_yaml_scalar(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() || value == "~" || value.eq_ignore_ascii_case("null") {
        return Ok(String::new());
    }
    if let Some(inner) = value.strip_prefix('"') {
        let Some(inner) = inner.strip_suffix('"') else {
            return Err(
                "yaml: found unexpected end of stream while scanning a quoted scalar".into(),
            );
        };
        return decode_yaml_double_quoted(inner);
    }
    if let Some(inner) = value.strip_prefix('\'') {
        let Some(inner) = inner.strip_suffix('\'') else {
            return Err(
                "yaml: found unexpected end of stream while scanning a quoted scalar".into(),
            );
        };
        return Ok(inner.replace("''", "'"));
    }
    Ok(value.to_string())
}

fn decode_yaml_double_quoted(value: &str) -> Result<String, String> {
    let mut output = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            output.push(ch);
            continue;
        }
        let Some(escaped) = chars.next() else {
            return Err("yaml: found unknown escape character".into());
        };
        match escaped {
            '0' => output.push('\0'),
            'a' => output.push('\u{0007}'),
            'b' => output.push('\u{0008}'),
            't' | '\t' => output.push('\t'),
            'n' => output.push('\n'),
            'v' => output.push('\u{000b}'),
            'f' => output.push('\u{000c}'),
            'r' => output.push('\r'),
            'e' => output.push('\u{001b}'),
            '"' => output.push('"'),
            '/' => output.push('/'),
            '\\' => output.push('\\'),
            'N' => output.push('\u{0085}'),
            '_' => output.push('\u{00a0}'),
            'L' => output.push('\u{2028}'),
            'P' => output.push('\u{2029}'),
            _ => return Err(format!("yaml: found unknown escape character {escaped:?}")),
        }
    }
    Ok(output)
}

fn set_config_value(config: &mut FileConfig, path: &[String], value: &str) {
    let path = path.iter().map(String::as_str).collect::<Vec<_>>();
    match path.as_slice() {
        ["schema_version"] => config.schema_version = value.parse().unwrap_or_default(),
        ["identity", "active"] => config.identity.active = value.to_string(),
        ["runtime", "mode"] => config.runtime.mode = value.to_string(),
        ["runtime", "socket_path"] => config.runtime.socket_path = value.to_string(),
        ["runtime", "listener", "enabled"] => config.runtime.listener.enabled = parse_bool(value),
        ["runtime", "listener", "auto_install"] => {
            config.runtime.listener.auto_install = parse_bool(value)
        }
        ["runtime", "listener", "auto_start"] => {
            config.runtime.listener.auto_start = parse_bool(value)
        }
        ["runtime", "host_notify", "enabled"] => {
            config.runtime.host_notify.enabled = parse_bool(value)
        }
        ["runtime", "host_notify", "sink"] => config.runtime.host_notify.sink = value.to_string(),
        ["runtime", "host_notify", "file_path"] => {
            config.runtime.host_notify.file_path = value.to_string()
        }
        ["runtime", "host_notify", "openclaw", "hook_url"] => {
            config.runtime.host_notify.openclaw.hook_url = value.to_string()
        }
        ["runtime", "host_notify", "openclaw", "agent_id"] => {
            config.runtime.host_notify.openclaw.agent_id = value.to_string()
        }
        ["runtime", "host_notify", "openclaw", "hook_name"] => {
            config.runtime.host_notify.openclaw.hook_name = value.to_string()
        }
        ["runtime", "host_notify", "openclaw", "token"] => {
            config.runtime.host_notify.openclaw.token = value.to_string()
        }
        ["runtime", "host_notify", "hermes", "notify_url"] => {
            config.runtime.host_notify.hermes.notify_url = value.to_string()
        }
        ["runtime", "host_notify", "hermes", "deliver"] => {
            config.runtime.host_notify.hermes.deliver = value.to_string()
        }
        ["runtime", "host_notify", "hermes", "secret"] => {
            config.runtime.host_notify.hermes.secret = value.to_string()
        }
        ["runtime", "host_notify", "webhook", "notify_url"] => {
            config.runtime.host_notify.webhook.notify_url = value.to_string()
        }
        ["runtime", "host_notify", "webhook", "secret"] => {
            config.runtime.host_notify.webhook.secret = value.to_string()
        }
        ["output", "format"] => config.output.format = value.to_string(),
        ["output", "no_color"] => config.output.no_color = parse_bool(value),
        ["secret_storage", "mode"] => config.secret_storage.mode = value.to_string(),
        ["secret_storage", "vault_dir"] => config.secret_storage.vault_dir = value.to_string(),
        ["secret_storage", "workspace_id"] => {
            config.secret_storage.workspace_id = value.to_string()
        }
        ["secret_storage", "device_id"] => config.secret_storage.device_id = value.to_string(),
        ["services", "service_base_url"] => config.services.service_base_url = value.to_string(),
        ["services", "user_service_endpoint"] => {
            config.services.user_service_endpoint = value.to_string()
        }
        ["services", "message_service_endpoint"] => {
            config.services.message_service_endpoint = value.to_string()
        }
        ["services", "did_domain"] => config.services.did_domain = value.to_string(),
        ["services", "anp_service_endpoint"] => {
            config.services.anp_service_endpoint = value.to_string()
        }
        ["services", "anp_service_did"] => config.services.anp_service_did = value.to_string(),
        ["services", "ca_bundle"] => config.services.ca_bundle = value.to_string(),
        ["services", "mail_service_url"] => config.services.mail_service_url = value.to_string(),
        ["update", "disable_strict_version"] => {
            config.update.disable_strict_version = parse_bool(value).unwrap_or(false)
        }
        ["update", "metadata_cache_ttl_seconds"] => {
            config.update.metadata_cache_ttl_seconds = value.parse().unwrap_or_default()
        }
        _ => {}
    }
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn current_workspace_home() -> anyhow::Result<(PathBuf, ValueSource)> {
    let home = try_home_dir();
    resolve_workspace_home(home.as_deref())
}

fn resolve_workspace_home(home: Option<&Path>) -> anyhow::Result<(PathBuf, ValueSource)> {
    let configured = env::var("AWIKI_CLI_WORKSPACE_HOME_DIR").ok();
    resolve_workspace_home_from(home, configured.as_deref())
}

fn resolve_workspace_home_from(
    home: Option<&Path>,
    configured: Option<&str>,
) -> anyhow::Result<(PathBuf, ValueSource)> {
    if let Some(value) = configured {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            let expanded = match home {
                Some(home) => expand_tilde(home, trimmed),
                None if requires_home_expansion(trimmed) => {
                    return Err(HomeDirUnavailable.into());
                }
                None => PathBuf::from(trimmed),
            };
            return Ok((
                expanded.clone(),
                ValueSource {
                    source: "canonical_env".to_string(),
                    key: "AWIKI_CLI_WORKSPACE_HOME_DIR".to_string(),
                    value: path_string(&expanded),
                },
            ));
        }
    }
    let home = home.ok_or(HomeDirUnavailable)?;
    let default = home.join(format!(".{APP_NAME}"));
    Ok((
        default.clone(),
        ValueSource {
            source: "default".to_string(),
            key: String::new(),
            value: path_string(&default),
        },
    ))
}

fn build_paths(home: Option<&Path>, workspace_home_dir: &Path) -> Paths {
    let data_dir = workspace_home_dir.join("data");
    let state_dir = workspace_home_dir.join("runtime");
    let cache_dir = workspace_home_dir.join("cache");
    let logs_dir = workspace_home_dir.join("logs");
    Paths {
        workspace_home_dir: path_string(workspace_home_dir),
        root_dir: path_string(workspace_home_dir),
        config_dir: path_string(workspace_home_dir),
        data_dir: path_string(&data_dir),
        state_dir: path_string(&state_dir),
        cache_dir: path_string(&cache_dir),
        logs_dir: path_string(&logs_dir),
        config_file: path_string(&workspace_home_dir.join(CONFIG_FILE_NAME)),
        identity_dir: path_string(&workspace_home_dir.join("identities")),
        database_file: path_string(&data_dir.join(format!("{APP_NAME}.db"))),
        legacy_credentials_dir: home
            .map(|home| {
                home.join(".openclaw")
                    .join("credentials")
                    .join("awiki-agent-id-message")
            })
            .as_deref()
            .map(path_string)
            .unwrap_or_default(),
        legacy_data_dir: home
            .map(|home| {
                home.join(".openclaw")
                    .join("workspace")
                    .join("data")
                    .join("awiki-agent-id-message")
            })
            .as_deref()
            .map(path_string)
            .unwrap_or_default(),
    }
}

fn choose_value(
    flag_value: &str,
    flag_changed: bool,
    file_value: &str,
    default_value: &str,
) -> (String, ValueSource) {
    if flag_changed && !flag_value.trim().is_empty() {
        let value = flag_value.trim().to_string();
        return (
            value.clone(),
            ValueSource {
                source: "flag".to_string(),
                key: String::new(),
                value,
            },
        );
    }
    if !file_value.trim().is_empty() {
        let value = file_value.trim().to_string();
        return (
            value.clone(),
            ValueSource {
                source: "config_file".to_string(),
                key: String::new(),
                value,
            },
        );
    }
    (
        default_value.to_string(),
        ValueSource {
            source: "default".to_string(),
            key: String::new(),
            value: default_value.to_string(),
        },
    )
}

fn normalized_config_schema_version(version: i64) -> i64 {
    version.max(0)
}

fn collect_env_hits() -> Vec<EnvHit> {
    let mut hits = env::var("AWIKI_CLI_WORKSPACE_HOME_DIR")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(|value| {
            vec![EnvHit {
                key: "AWIKI_CLI_WORKSPACE_HOME_DIR".to_string(),
                value,
                tier: "canonical_env".to_string(),
                target: "workspace_home_dir".to_string(),
            }]
        })
        .unwrap_or_default();
    if env::var(im_core::vault::IM_CORE_VAULT_ROOT_KEY_ENV)
        .ok()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
    {
        hits.push(EnvHit {
            key: im_core::vault::IM_CORE_VAULT_ROOT_KEY_ENV.to_string(),
            value: "[redacted]".to_string(),
            tier: "canonical_env".to_string(),
            target: "secret_storage.root_key".to_string(),
        });
    }
    hits
}

pub fn resolve_secret_storage(resolved: &Resolved) -> anyhow::Result<ResolvedSecretStorage> {
    let (config, _, error) = read_file_config(&resolved.paths.config_file);
    if !error.is_empty() {
        anyhow::bail!(error);
    }
    let mut sources = BTreeMap::new();
    resolve_secret_storage_from_config(&resolved.paths, &config.secret_storage, &mut sources)
}

fn resolve_secret_storage_from_config(
    paths: &Paths,
    config: &SecretStorageConfig,
    sources: &mut BTreeMap<String, ValueSource>,
) -> anyhow::Result<ResolvedSecretStorage> {
    let mode = normalize_secret_storage_mode(&config.mode)?;
    sources.insert(
        "secret_storage.mode".to_string(),
        ValueSource {
            source: if config.mode.trim().is_empty() {
                "default".to_string()
            } else {
                "config_file".to_string()
            },
            key: String::new(),
            value: mode.clone(),
        },
    );
    let vault_dir = if config.vault_dir.trim().is_empty() {
        Path::new(&paths.data_dir)
            .join("identity-vault")
            .to_string_lossy()
            .into_owned()
    } else {
        config.vault_dir.trim().to_string()
    };
    sources.insert(
        "secret_storage.vault_dir".to_string(),
        ValueSource {
            source: if config.vault_dir.trim().is_empty() {
                "derived_default".to_string()
            } else {
                "config_file".to_string()
            },
            key: if config.vault_dir.trim().is_empty() {
                "data_dir".to_string()
            } else {
                String::new()
            },
            value: vault_dir.clone(),
        },
    );
    let workspace_id = if config.workspace_id.trim().is_empty() {
        workspace_id_from_path(&paths.workspace_home_dir)
    } else {
        config.workspace_id.trim().to_string()
    };
    sources.insert(
        "secret_storage.workspace_id".to_string(),
        ValueSource {
            source: if config.workspace_id.trim().is_empty() {
                "derived_default".to_string()
            } else {
                "config_file".to_string()
            },
            key: if config.workspace_id.trim().is_empty() {
                "workspace_home_dir".to_string()
            } else {
                String::new()
            },
            value: workspace_id.clone(),
        },
    );
    let device_id = if config.device_id.trim().is_empty() {
        "cli-local-device".to_string()
    } else {
        config.device_id.trim().to_string()
    };
    sources.insert(
        "secret_storage.device_id".to_string(),
        ValueSource {
            source: if config.device_id.trim().is_empty() {
                "default".to_string()
            } else {
                "config_file".to_string()
            },
            key: String::new(),
            value: device_id.clone(),
        },
    );
    let root_key_env = im_core::vault::IM_CORE_VAULT_ROOT_KEY_ENV.to_string();
    let root_key_available = env::var(&root_key_env)
        .ok()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);
    Ok(ResolvedSecretStorage {
        mode,
        vault_dir,
        workspace_id,
        device_id,
        root_key_env: root_key_env.clone(),
        root_key_available,
        root_key_source: if root_key_available {
            root_key_env
        } else {
            "unset".to_string()
        },
    })
}

pub fn normalize_secret_storage_mode(raw: &str) -> anyhow::Result<String> {
    let mode = if raw.trim().is_empty() {
        DEFAULT_IDENTITY_SECRET_STORAGE_MODE.to_string()
    } else {
        raw.trim().to_ascii_lowercase().replace('-', "_")
    };
    match mode.as_str() {
        "file_compat" | "vault_preferred" | "vault_required" => Ok(mode),
        _ => anyhow::bail!("unsupported secret_storage.mode"),
    }
}

fn workspace_id_from_path(path: &str) -> String {
    let digest = Sha256::digest(path.as_bytes());
    let hex = format!("{digest:x}");
    format!("cli-workspace-{}", &hex[..16])
}

fn host_notify_file_path(paths: &Paths, config: &HostNotifyConfig, sink: &str) -> String {
    if sink != "file" {
        return String::new();
    }
    if !config.file_path.trim().is_empty() {
        return config.file_path.trim().to_string();
    }
    Path::new(&paths.state_dir)
        .join(DEFAULT_HOST_NOTIFY_FILE)
        .to_string_lossy()
        .into_owned()
}

fn resolve_openclaw_fields(
    config: &HostNotifyConfig,
    sink: &str,
) -> (String, ValueSource, String, String) {
    if sink != "openclaw" {
        return (
            String::new(),
            ValueSource {
                source: "unset".to_string(),
                key: String::new(),
                value: String::new(),
            },
            String::new(),
            String::new(),
        );
    }
    let (hook_url, hook_source) = choose_value(
        "",
        false,
        &config.openclaw.hook_url,
        DEFAULT_OPENCLAW_HOOK_URL,
    );
    (
        hook_url,
        hook_source,
        default_trimmed_string(&config.openclaw.agent_id, DEFAULT_OPENCLAW_AGENT_ID),
        default_trimmed_string(&config.openclaw.hook_name, DEFAULT_OPENCLAW_HOOK_NAME),
    )
}

fn resolve_hermes_fields(
    config: &HostNotifyConfig,
    sink: &str,
) -> (String, ValueSource, String, ValueSource) {
    let (deliver, deliver_source) = resolve_hermes_deliver_field(config);
    if sink != "hermes" {
        return (
            String::new(),
            ValueSource {
                source: "unset".to_string(),
                key: String::new(),
                value: String::new(),
            },
            deliver,
            deliver_source,
        );
    }
    let (notify_url, notify_source) = if !config.hermes.notify_url.trim().is_empty() {
        (
            config.hermes.notify_url.trim().to_string(),
            ValueSource {
                source: "config_file".to_string(),
                key: "runtime.host_notify.hermes.notify_url".to_string(),
                value: config.hermes.notify_url.trim().to_string(),
            },
        )
    } else if !config.webhook.notify_url.trim().is_empty() {
        (
            config.webhook.notify_url.trim().to_string(),
            ValueSource {
                source: "config_file".to_string(),
                key: "runtime.host_notify.webhook.notify_url".to_string(),
                value: config.webhook.notify_url.trim().to_string(),
            },
        )
    } else {
        (
            DEFAULT_HERMES_NOTIFY_URL.to_string(),
            ValueSource {
                source: "default".to_string(),
                key: String::new(),
                value: DEFAULT_HERMES_NOTIFY_URL.to_string(),
            },
        )
    };
    (notify_url, notify_source, deliver, deliver_source)
}

fn resolve_hermes_deliver_field(config: &HostNotifyConfig) -> (String, ValueSource) {
    let value = config.hermes.deliver.trim();
    if !value.is_empty() {
        let value = value.to_ascii_lowercase();
        return (
            value.clone(),
            ValueSource {
                source: "config_file".to_string(),
                key: "runtime.host_notify.hermes.deliver".to_string(),
                value,
            },
        );
    }
    (
        DEFAULT_HERMES_DELIVER_TARGET.to_string(),
        ValueSource {
            source: "default".to_string(),
            key: String::new(),
            value: DEFAULT_HERMES_DELIVER_TARGET.to_string(),
        },
    )
}

fn normalize_host_notify_sink(value: &str) -> String {
    let normalized = default_string(value, DEFAULT_HOST_NOTIFY_SINK);
    if normalized == "webhook" {
        "hermes".to_string()
    } else {
        normalized
    }
}

pub fn normalize_host_notify_sink_for_write(value: &str) -> anyhow::Result<String> {
    let normalized = normalize_host_notify_sink(value);
    validate_host_notify_sink(&normalized)?;
    Ok(normalized)
}

fn validate_host_notify_sink(value: &str) -> anyhow::Result<()> {
    match value {
        "noop" | "log" | "file" | "openclaw" | "hermes" => Ok(()),
        _ => anyhow::bail!("unsupported host notify sink"),
    }
}

pub fn derive_anp_service_endpoint(service_base_url: &str) -> String {
    join_base_url(service_base_url, DEFAULT_ANP_PATH)
}

pub fn derive_anp_service_did(service_base_url: &str) -> String {
    format!("did:wba:{}", service_host_from_base_url(service_base_url))
}

pub fn normalize_did_domain(raw: &str) -> Result<String, WorkspaceConfigError> {
    let normalized = raw.trim().to_ascii_lowercase();
    let normalized = normalized.trim_end_matches('.').to_string();
    if normalized.is_empty() {
        return Err(WorkspaceConfigError::invalid_argument(
            "did_host is required",
            did_host_hint(),
        ));
    }
    if normalized.contains("://") {
        return Err(WorkspaceConfigError::invalid_argument(
            "did_host must be a bare domain without a URL scheme",
            did_host_hint(),
        ));
    }
    if normalized.contains(['/', '?', '#']) {
        return Err(WorkspaceConfigError::invalid_argument(
            "did_host must not include a path, query, or fragment",
            did_host_hint(),
        ));
    }
    if normalized.contains(':') {
        return Err(WorkspaceConfigError::invalid_argument(
            "did_host must not include a port",
            did_host_hint(),
        ));
    }
    if normalized.chars().any(char::is_whitespace) {
        return Err(WorkspaceConfigError::invalid_argument(
            "did_host must not contain whitespace",
            did_host_hint(),
        ));
    }
    if normalized.contains('@') || normalized.contains('%') {
        return Err(WorkspaceConfigError::invalid_argument(
            "did_host must be a bare domain",
            did_host_hint(),
        ));
    }
    Ok(normalized)
}

pub fn normalize_base_url(base_url: &str) -> String {
    base_url.trim().trim_end_matches('/').to_string()
}

pub fn join_base_url(base_url: &str, path: &str) -> String {
    let base = normalize_base_url(base_url);
    if base.is_empty() {
        return path.trim().to_string();
    }
    let mut path = path.trim().to_string();
    if path.is_empty() {
        return base;
    }
    if !path.starts_with('/') {
        path.insert(0, '/');
    }
    format!("{base}{path}")
}

pub fn derive_websocket_url(base_url: &str, path: &str) -> String {
    let http_url = join_base_url(base_url, path);
    let trimmed = http_url.trim();
    if let Some(rest) = trimmed.strip_prefix("https://") {
        return format!("wss://{rest}");
    }
    if let Some(rest) = trimmed.strip_prefix("http://") {
        return format!("ws://{rest}");
    }
    trimmed.to_string()
}

fn service_host_from_base_url(service_base_url: &str) -> String {
    let normalized = normalize_base_url(service_base_url);
    normalized
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or(CHINA_DID_DOMAIN)
        .split(':')
        .next()
        .unwrap_or(CHINA_DID_DOMAIN)
        .to_ascii_lowercase()
}

fn default_runtime_bridge_path(paths: &Paths) -> String {
    if cfg!(windows) {
        let digest = Sha256::digest(paths.workspace_home_dir.as_bytes());
        let hex = format!("{digest:x}");
        return format!(r"\\.\pipe\awiki-cli-{}", &hex[..16]);
    }
    Path::new(&paths.state_dir)
        .join("message-daemon.sock")
        .to_string_lossy()
        .into_owned()
}

fn default_string(value: &str, default: &str) -> String {
    if value.trim().is_empty() {
        default.to_string()
    } else {
        value.trim().to_ascii_lowercase()
    }
}

fn default_trimmed_string(value: &str, default: &str) -> String {
    if value.trim().is_empty() {
        default.to_string()
    } else {
        value.trim().to_string()
    }
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_config_still_accepts_legacy_service_endpoint_fields_for_config_preservation() {
        let raw = r#"
schema_version: 1
services:
  service_base_url: https://base.example.test/
  user_service_endpoint: https://users.example.test/
  message_service_endpoint: https://messages.example.test/
  did_domain: example.test
"#;

        let config = parse_file_config(raw).expect("parse config");

        assert_eq!(
            config.services.service_base_url,
            "https://base.example.test/"
        );
        assert_eq!(
            config.services.user_service_endpoint,
            "https://users.example.test/"
        );
        assert_eq!(
            config.services.message_service_endpoint,
            "https://messages.example.test/"
        );
    }

    #[test]
    fn absolute_workspace_override_does_not_require_a_user_home() {
        let (workspace, source) =
            resolve_workspace_home_from(None, Some("/srv/awiki/workspace")).unwrap();

        assert_eq!(workspace, PathBuf::from("/srv/awiki/workspace"));
        assert_eq!(source.source, "canonical_env");
        assert_eq!(source.key, "AWIKI_CLI_WORKSPACE_HOME_DIR");
    }

    #[test]
    fn default_and_tilde_workspace_paths_require_a_user_home() {
        assert!(resolve_workspace_home_from(None, None)
            .unwrap_err()
            .downcast_ref::<HomeDirUnavailable>()
            .is_some());
        assert!(resolve_workspace_home_from(None, Some("~/workspace"))
            .unwrap_err()
            .downcast_ref::<HomeDirUnavailable>()
            .is_some());
    }

    #[test]
    fn paths_without_a_user_home_keep_legacy_discovery_disabled() {
        let workspace = Path::new("/srv/awiki/workspace");
        let paths = build_paths(None, workspace);

        assert_eq!(paths.workspace_home_dir, path_string(workspace));
        assert!(paths.legacy_credentials_dir.is_empty());
        assert!(paths.legacy_data_dir.is_empty());
    }
}
