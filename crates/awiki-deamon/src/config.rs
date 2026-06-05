use std::path::{Component, Path, PathBuf};

use anyhow::{bail, Context, Result};
use im_core::{
    IdentityRegistryPaths, ImCoreConfig, ImCorePaths, LocalStatePaths, MessageTransportPolicy,
    RuntimePaths, ServiceEndpoint,
};
use serde::{Deserialize, Serialize};

const DEFAULT_BASE_URL: &str = "https://awiki.ai";
const PERSISTENT_CONFIG_FILE_NAME: &str = "config.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonConfig {
    pub state_root: PathBuf,
    pub config_file_path: PathBuf,
    pub daemon_db_path: PathBuf,
    pub im_core_sqlite_path: PathBuf,
    pub identity_root_dir: PathBuf,
    pub identity_registry_path: PathBuf,
    pub default_identity_path: Option<PathBuf>,
    pub runtime_cache_dir: PathBuf,
    pub runtime_temp_dir: PathBuf,
    pub local_socket_path: PathBuf,
    pub audit_log_path: PathBuf,
    pub service_base_url: String,
    pub user_service_base_url: String,
    pub message_service_base_url: String,
    pub mail_service_base_url: String,
    pub download_base_url: String,
    pub did_domain: String,
    pub anp_service_endpoint: String,
    pub anp_service_did: String,
    pub hermes_gateway_cmd: Option<String>,
    pub identity_selector: IdentitySelectorConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonConfigFile {
    pub state_root: PathBuf,
    pub base_url: Option<String>,
    pub service_base_url: Option<String>,
    pub user_service_base_url: Option<String>,
    pub message_service_base_url: Option<String>,
    pub mail_service_base_url: Option<String>,
    pub download_base_url: Option<String>,
    pub did_domain: Option<String>,
    pub anp_service_endpoint: Option<String>,
    pub anp_service_did: Option<String>,
    pub hermes_gateway_cmd: Option<String>,
    pub identity_selector: Option<IdentitySelectorConfig>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonPersistentConfig {
    #[serde(default)]
    pub schema_version: u32,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub user_service_base_url: Option<String>,
    #[serde(default)]
    pub message_service_base_url: Option<String>,
    #[serde(default)]
    pub mail_service_base_url: Option<String>,
    #[serde(default)]
    pub download_base_url: Option<String>,
    #[serde(default)]
    pub did_domain: Option<String>,
    #[serde(default)]
    pub anp_service_endpoint: Option<String>,
    #[serde(default)]
    pub anp_service_did: Option<String>,
    #[serde(default)]
    pub hermes_gateway_cmd: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value")]
pub enum IdentitySelectorConfig {
    Default,
    LocalAlias(String),
    Did(String),
}

impl DaemonConfig {
    pub fn default_product_state_root() -> Result<PathBuf> {
        if let Some(home) = std::env::var_os("HOME").filter(|value| !value.is_empty()) {
            return normalize_state_root(
                PathBuf::from(home)
                    .join(".awiki-daemon")
                    .join("deamon")
                    .join("state"),
            );
        }
        normalize_state_root(PathBuf::from(".awiki-daemon").join("deamon").join("state"))
    }

    pub fn for_state_root(state_root: impl Into<PathBuf>) -> Result<Self> {
        let state_root = normalize_state_root(state_root.into())?;
        let config_file_path = state_root.join(PERSISTENT_CONFIG_FILE_NAME);
        let persisted = DaemonPersistentConfig::read_optional(&config_file_path)?;
        let env_base_url = env_value("AWIKI_DAEMON_BASE_URL");
        let env_service_base_url = env_value("AWIKI_DAEMON_SERVICE_BASE_URL");
        let base_url = normalize_base_url(&first_non_empty([
            env_base_url.clone(),
            env_service_base_url.clone(),
            persisted.base_url.clone(),
            Some(DEFAULT_BASE_URL.to_string()),
        ])?);
        let service_base_url = normalize_base_url(&first_non_empty([
            env_service_base_url,
            env_base_url,
            Some(base_url.clone()),
        ])?);
        let user_service_base_url = normalize_base_url(&first_non_empty([
            env_value("AWIKI_DAEMON_USER_SERVICE_BASE_URL"),
            persisted.user_service_base_url.clone(),
            Some(service_base_url.clone()),
        ])?);
        let message_service_base_url = normalize_base_url(&first_non_empty([
            env_value("AWIKI_DAEMON_MESSAGE_SERVICE_BASE_URL"),
            persisted.message_service_base_url.clone(),
            Some(service_base_url.clone()),
        ])?);
        let mail_service_base_url = normalize_base_url(&first_non_empty([
            env_value("AWIKI_DAEMON_MAIL_SERVICE_BASE_URL"),
            persisted.mail_service_base_url.clone(),
            Some(service_base_url.clone()),
        ])?);
        let download_base_url = normalize_base_url(&first_non_empty([
            env_value("AWIKI_DAEMON_DOWNLOAD_BASE_URL"),
            persisted.download_base_url.clone(),
            Some(join_base_url(&service_base_url, "/daemon")),
        ])?);
        let did_domain = first_non_empty([
            env_value("AWIKI_DAEMON_DID_DOMAIN"),
            persisted.did_domain.clone(),
            Some(host_from_base_url(&service_base_url)),
        ])?;
        let anp_service_endpoint = normalize_base_url(&first_non_empty([
            env_value("AWIKI_DAEMON_ANP_SERVICE_URL"),
            persisted.anp_service_endpoint.clone(),
            Some(join_base_url(&service_base_url, "/anp-im/rpc")),
        ])?);
        let anp_service_did = first_non_empty([
            env_value("AWIKI_DAEMON_ANP_SERVICE_DID"),
            persisted.anp_service_did.clone(),
            Some(format!("did:wba:{did_domain}")),
        ])?;
        let hermes_gateway_cmd = first_optional_non_empty([
            env_value("AWIKI_HERMES_GATEWAY_CMD"),
            persisted.hermes_gateway_cmd.clone(),
        ]);
        Ok(Self {
            config_file_path,
            daemon_db_path: state_root.join("daemon.db"),
            im_core_sqlite_path: state_root.join("im-core").join("local-state.sqlite"),
            identity_root_dir: state_root.join("identity"),
            identity_registry_path: state_root.join("identity").join("registry.json"),
            default_identity_path: Some(state_root.join("identity").join("default")),
            runtime_cache_dir: state_root.join("runtime").join("cache"),
            runtime_temp_dir: state_root.join("runtime").join("tmp"),
            local_socket_path: state_root.join("rpc").join("awiki-deamon.sock"),
            audit_log_path: state_root.join("audit").join("audit.log"),
            service_base_url,
            user_service_base_url,
            message_service_base_url,
            mail_service_base_url,
            download_base_url,
            did_domain,
            anp_service_endpoint,
            anp_service_did,
            hermes_gateway_cmd,
            identity_selector: IdentitySelectorConfig::Default,
            state_root,
        })
    }

    pub fn from_config_file(file: DaemonConfigFile) -> Result<Self> {
        let mut config = Self::for_state_root(file.state_root)?;
        let service_base_url = file.service_base_url.or(file.base_url);
        if let Some(service_base_url) = service_base_url {
            config.service_base_url = normalize_base_url(&service_base_url);
            if file.user_service_base_url.is_none() {
                config.user_service_base_url = config.service_base_url.clone();
            }
            if file.message_service_base_url.is_none() {
                config.message_service_base_url = config.service_base_url.clone();
            }
            if file.mail_service_base_url.is_none() {
                config.mail_service_base_url = config.service_base_url.clone();
            }
            if file.download_base_url.is_none() {
                config.download_base_url = join_base_url(&config.service_base_url, "/daemon");
            }
            if file.did_domain.is_none() {
                config.did_domain = host_from_base_url(&config.service_base_url);
            }
            if file.anp_service_endpoint.is_none() {
                config.anp_service_endpoint =
                    join_base_url(&config.service_base_url, "/anp-im/rpc");
            }
            if file.anp_service_did.is_none() {
                config.anp_service_did = format!("did:wba:{}", config.did_domain);
            }
        }
        if let Some(user_service_base_url) = file.user_service_base_url {
            config.user_service_base_url = normalize_base_url(&user_service_base_url);
        }
        if let Some(message_service_base_url) = file.message_service_base_url {
            config.message_service_base_url = normalize_base_url(&message_service_base_url);
        }
        if let Some(mail_service_base_url) = file.mail_service_base_url {
            config.mail_service_base_url = normalize_base_url(&mail_service_base_url);
        }
        if let Some(download_base_url) = file.download_base_url {
            config.download_base_url = normalize_base_url(&download_base_url);
        }
        if let Some(did_domain) = file.did_domain {
            config.did_domain = did_domain.trim().to_string();
        }
        if let Some(anp_service_endpoint) = file.anp_service_endpoint {
            config.anp_service_endpoint = normalize_base_url(&anp_service_endpoint);
        }
        if let Some(anp_service_did) = file.anp_service_did {
            config.anp_service_did = anp_service_did.trim().to_string();
        }
        if let Some(hermes_gateway_cmd) = file.hermes_gateway_cmd {
            config.hermes_gateway_cmd = normalize_optional_string(Some(hermes_gateway_cmd));
        }
        if let Some(identity_selector) = file.identity_selector {
            config.identity_selector = identity_selector;
        }
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        validate_state_root(&self.state_root)?;
        validate_child_path("config_file_path", &self.config_file_path, &self.state_root)?;
        validate_child_path("daemon_db_path", &self.daemon_db_path, &self.state_root)?;
        validate_child_path(
            "im_core_sqlite_path",
            &self.im_core_sqlite_path,
            &self.state_root,
        )?;
        validate_child_path(
            "identity_root_dir",
            &self.identity_root_dir,
            &self.state_root,
        )?;
        validate_child_path(
            "identity_registry_path",
            &self.identity_registry_path,
            &self.state_root,
        )?;
        if let Some(path) = self.default_identity_path.as_ref() {
            validate_child_path("default_identity_path", path, &self.state_root)?;
        }
        validate_child_path(
            "runtime_cache_dir",
            &self.runtime_cache_dir,
            &self.state_root,
        )?;
        validate_child_path("runtime_temp_dir", &self.runtime_temp_dir, &self.state_root)?;
        validate_child_path(
            "local_socket_path",
            &self.local_socket_path,
            &self.state_root,
        )?;
        validate_child_path("audit_log_path", &self.audit_log_path, &self.state_root)?;
        ServiceEndpoint::parse(&self.service_base_url)
            .with_context(|| format!("invalid service_base_url {}", self.service_base_url))?;
        ServiceEndpoint::parse(&self.user_service_base_url).with_context(|| {
            format!(
                "invalid user_service_base_url {}",
                self.user_service_base_url
            )
        })?;
        ServiceEndpoint::parse(&self.message_service_base_url).with_context(|| {
            format!(
                "invalid message_service_base_url {}",
                self.message_service_base_url
            )
        })?;
        ServiceEndpoint::parse(&self.mail_service_base_url).with_context(|| {
            format!(
                "invalid mail_service_base_url {}",
                self.mail_service_base_url
            )
        })?;
        validate_download_base_url(&self.download_base_url)?;
        ServiceEndpoint::parse(&self.anp_service_endpoint).with_context(|| {
            format!("invalid anp_service_endpoint {}", self.anp_service_endpoint)
        })?;
        if self.did_domain.trim().is_empty() {
            bail!("did_domain must not be empty");
        }
        im_core::ids::Did::parse(&self.anp_service_did).context("invalid anp_service_did")?;
        match &self.identity_selector {
            IdentitySelectorConfig::Default => {}
            IdentitySelectorConfig::LocalAlias(alias) if alias.trim().is_empty() => {
                bail!("identity local alias must not be empty");
            }
            IdentitySelectorConfig::LocalAlias(_) => {}
            IdentitySelectorConfig::Did(did) => {
                im_core::ids::Did::parse(did).context("invalid identity selector DID")?;
            }
        }
        Ok(())
    }

    pub fn write_persistent_config(&self) -> Result<()> {
        self.write_persistent_config_value(&DaemonPersistentConfig::from_resolved(self))
    }

    pub fn write_persistent_hermes_gateway_cmd(&self, gateway_cmd: Option<String>) -> Result<()> {
        let mut persisted = DaemonPersistentConfig::read_optional(&self.config_file_path)?;
        if persisted.schema_version == 0 {
            persisted.schema_version = 1;
        }
        persisted.hermes_gateway_cmd = normalize_optional_string(gateway_cmd);
        self.write_persistent_config_value(&persisted)
    }

    pub fn read_persistent_hermes_gateway_cmd(&self) -> Result<Option<String>> {
        let persisted = DaemonPersistentConfig::read_optional(&self.config_file_path)?;
        Ok(normalize_optional_string(persisted.hermes_gateway_cmd))
    }

    fn write_persistent_config_value(&self, value: &DaemonPersistentConfig) -> Result<()> {
        if let Some(parent) = self.config_file_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create daemon config directory {}", parent.display()))?;
        }
        let text = serde_json::to_string_pretty(value)?;
        std::fs::write(&self.config_file_path, format!("{text}\n"))
            .with_context(|| format!("write daemon config {}", self.config_file_path.display()))
    }

    pub fn ensure_state_layout(&self) -> Result<()> {
        for dir in [
            &self.state_root,
            &self.identity_root_dir,
            &self.runtime_cache_dir,
            &self.runtime_temp_dir,
        ] {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("create daemon state directory {}", dir.display()))?;
        }
        for path in [
            &self.config_file_path,
            &self.daemon_db_path,
            &self.im_core_sqlite_path,
            &self.identity_registry_path,
            &self.local_socket_path,
            &self.audit_log_path,
        ] {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).with_context(|| {
                    format!("create daemon state directory {}", parent.display())
                })?;
            }
        }
        Ok(())
    }

    pub fn im_core_config(&self) -> Result<ImCoreConfig> {
        let mut config = ImCoreConfig::new(
            ServiceEndpoint::parse(&self.service_base_url)?,
            self.did_domain.clone(),
        )?;
        config.user_service_endpoint = Some(ServiceEndpoint::parse(&self.user_service_base_url)?);
        config.message_service_endpoint =
            Some(ServiceEndpoint::parse(&self.message_service_base_url)?);
        config.mail_service_endpoint = Some(ServiceEndpoint::parse(&self.mail_service_base_url)?);
        config.anp_service_endpoint = Some(ServiceEndpoint::parse(&self.anp_service_endpoint)?);
        config.anp_service_did = Some(im_core::ids::Did::parse(&self.anp_service_did)?);
        config.transport_policy = MessageTransportPolicy::HttpOnly;
        Ok(config)
    }

    pub fn im_core_paths(&self) -> ImCorePaths {
        ImCorePaths {
            identities: IdentityRegistryPaths {
                identity_root_dir: self.identity_root_dir.clone(),
                registry_path: self.identity_registry_path.clone(),
                default_identity_path: self.default_identity_path.clone(),
            },
            local_state: LocalStatePaths {
                sqlite_path: self.im_core_sqlite_path.clone(),
            },
            runtime: RuntimePaths {
                cache_dir: self.runtime_cache_dir.clone(),
                temp_dir: self.runtime_temp_dir.clone(),
            },
        }
    }
}

impl DaemonPersistentConfig {
    fn read_optional(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("read daemon config {}", path.display()))?;
        let mut value: Self = serde_json::from_str(&text)
            .with_context(|| format!("parse daemon config {}", path.display()))?;
        if value.schema_version == 0 {
            value.schema_version = 1;
        }
        Ok(value)
    }

    fn from_resolved(config: &DaemonConfig) -> Self {
        Self {
            schema_version: 1,
            base_url: Some(config.service_base_url.clone()),
            user_service_base_url: Some(config.user_service_base_url.clone()),
            message_service_base_url: Some(config.message_service_base_url.clone()),
            mail_service_base_url: Some(config.mail_service_base_url.clone()),
            download_base_url: Some(config.download_base_url.clone()),
            did_domain: Some(config.did_domain.clone()),
            anp_service_endpoint: Some(config.anp_service_endpoint.clone()),
            anp_service_did: Some(config.anp_service_did.clone()),
            hermes_gateway_cmd: config.hermes_gateway_cmd.clone(),
        }
    }
}

fn env_value(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn first_non_empty(values: impl IntoIterator<Item = Option<String>>) -> Result<String> {
    values
        .into_iter()
        .flatten()
        .map(|value| value.trim().to_string())
        .find(|value| !value.is_empty())
        .context("daemon base configuration is empty")
}

fn first_optional_non_empty(values: impl IntoIterator<Item = Option<String>>) -> Option<String> {
    values
        .into_iter()
        .flatten()
        .map(|value| value.trim().to_string())
        .find(|value| !value.is_empty())
}

fn normalize_base_url(value: &str) -> String {
    value.trim().trim_end_matches('/').to_string()
}

fn join_base_url(base_url: &str, path: &str) -> String {
    let base = normalize_base_url(base_url);
    let path = path.trim();
    if path.starts_with('/') {
        format!("{base}{path}")
    } else {
        format!("{base}/{path}")
    }
}

fn validate_download_base_url(value: &str) -> Result<()> {
    let value = value.trim();
    if value.is_empty() {
        bail!("download_base_url must not be empty");
    }
    if value.starts_with("http://")
        || value.starts_with("https://")
        || value.starts_with("file://")
        || value.starts_with('/')
        || value.starts_with("./")
        || value.starts_with("../")
    {
        return Ok(());
    }
    bail!("download_base_url must be http, https, file URL, or local path")
}

fn host_from_base_url(base_url: &str) -> String {
    let trimmed = base_url.trim();
    let without_scheme = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .unwrap_or(trimmed);
    let authority = without_scheme
        .split('/')
        .next()
        .unwrap_or_default()
        .split('@')
        .next_back()
        .unwrap_or_default();
    authority
        .split(':')
        .next()
        .unwrap_or("awiki.ai")
        .trim()
        .to_ascii_lowercase()
}

impl IdentitySelectorConfig {
    pub fn to_im_core_selector(&self) -> Result<im_core::IdentitySelector> {
        match self {
            Self::Default => Ok(im_core::IdentitySelector::Default),
            Self::LocalAlias(alias) => Ok(im_core::IdentitySelector::LocalAlias(alias.clone())),
            Self::Did(did) => Ok(im_core::IdentitySelector::Did(im_core::ids::Did::parse(
                did,
            )?)),
        }
    }
}

fn normalize_state_root(path: PathBuf) -> Result<PathBuf> {
    if path.as_os_str().is_empty() {
        bail!("state_root must not be empty");
    }
    if path
        .components()
        .any(|component| component == Component::ParentDir)
    {
        bail!("state_root must not contain parent directory components");
    }
    let path = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()?.join(path)
    };
    Ok(path)
}

fn validate_state_root(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty() {
        bail!("state_root must not be empty");
    }
    if path
        .components()
        .any(|component| component == Component::ParentDir)
    {
        bail!("state_root must not contain parent directory components");
    }
    Ok(())
}

fn validate_child_path(name: &str, path: &Path, state_root: &Path) -> Result<()> {
    if path.as_os_str().is_empty() {
        bail!("{name} must not be empty");
    }
    if path
        .components()
        .any(|component| component == Component::ParentDir)
    {
        bail!("{name} must not contain parent directory components");
    }
    if !path.starts_with(state_root) {
        bail!("{name} must be under state_root");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    const CONFIG_ENV_KEYS: &[&str] = &[
        "AWIKI_DAEMON_BASE_URL",
        "AWIKI_DAEMON_SERVICE_BASE_URL",
        "AWIKI_DAEMON_USER_SERVICE_BASE_URL",
        "AWIKI_DAEMON_MESSAGE_SERVICE_BASE_URL",
        "AWIKI_DAEMON_MAIL_SERVICE_BASE_URL",
        "AWIKI_DAEMON_DOWNLOAD_BASE_URL",
        "AWIKI_DAEMON_DID_DOMAIN",
        "AWIKI_DAEMON_ANP_SERVICE_URL",
        "AWIKI_DAEMON_ANP_SERVICE_DID",
        "AWIKI_HERMES_GATEWAY_CMD",
    ];

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvGuard {
        _lock: MutexGuard<'static, ()>,
        values: Vec<(&'static str, Option<String>)>,
    }

    impl EnvGuard {
        fn clear() -> Self {
            let lock = ENV_LOCK.lock().unwrap();
            let values = CONFIG_ENV_KEYS
                .iter()
                .map(|key| {
                    let value = std::env::var(key).ok();
                    std::env::remove_var(key);
                    (*key, value)
                })
                .collect();
            Self {
                _lock: lock,
                values,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (key, value) in &self.values {
                if let Some(value) = value {
                    std::env::set_var(key, value);
                } else {
                    std::env::remove_var(key);
                }
            }
        }
    }

    #[test]
    fn default_config_uses_state_root_layout() {
        let _env = EnvGuard::clear();
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("state");
        let config = DaemonConfig::for_state_root(&root).unwrap();

        assert_eq!(config.state_root, root);
        assert_eq!(config.config_file_path, root.join("config.json"));
        assert_eq!(config.daemon_db_path, root.join("daemon.db"));
        assert_eq!(
            config.local_socket_path,
            root.join("rpc").join("awiki-deamon.sock")
        );
        assert_eq!(
            config.identity_registry_path,
            root.join("identity").join("registry.json")
        );
        assert_eq!(config.service_base_url, "https://awiki.ai");
        assert_eq!(config.user_service_base_url, "https://awiki.ai");
        assert_eq!(config.message_service_base_url, "https://awiki.ai");
        assert_eq!(config.mail_service_base_url, "https://awiki.ai");
        assert_eq!(config.download_base_url, "https://awiki.ai/daemon");
        assert_eq!(config.did_domain, "awiki.ai");
        assert_eq!(config.anp_service_endpoint, "https://awiki.ai/anp-im/rpc");
        assert_eq!(config.anp_service_did, "did:wba:awiki.ai");
        config.validate().unwrap();
    }

    #[test]
    fn default_product_state_root_uses_daemon_home_layout() {
        let root = DaemonConfig::default_product_state_root().unwrap();

        if let Some(home) = std::env::var_os("HOME").filter(|value| !value.is_empty()) {
            assert_eq!(
                root,
                PathBuf::from(home)
                    .join(".awiki-daemon")
                    .join("deamon")
                    .join("state")
            );
        } else {
            assert!(root.ends_with(Path::new(".awiki-daemon").join("deamon").join("state")));
        }
    }

    #[test]
    fn base_url_drives_default_service_fields() {
        let _env = EnvGuard::clear();
        std::env::set_var("AWIKI_DAEMON_BASE_URL", "https://anpclaw.com/");
        let root = tempfile::tempdir().unwrap();

        let config = DaemonConfig::for_state_root(root.path()).unwrap();

        assert_eq!(config.service_base_url, "https://anpclaw.com");
        assert_eq!(config.user_service_base_url, "https://anpclaw.com");
        assert_eq!(config.message_service_base_url, "https://anpclaw.com");
        assert_eq!(config.mail_service_base_url, "https://anpclaw.com");
        assert_eq!(config.download_base_url, "https://anpclaw.com/daemon");
        assert_eq!(config.did_domain, "anpclaw.com");
        assert_eq!(
            config.anp_service_endpoint,
            "https://anpclaw.com/anp-im/rpc"
        );
        assert_eq!(config.anp_service_did, "did:wba:anpclaw.com");
    }

    #[test]
    fn explicit_service_overrides_win_over_base_defaults() {
        let _env = EnvGuard::clear();
        std::env::set_var("AWIKI_DAEMON_BASE_URL", "https://anpclaw.com");
        std::env::set_var(
            "AWIKI_DAEMON_SERVICE_BASE_URL",
            "https://service.example.test/",
        );
        std::env::set_var(
            "AWIKI_DAEMON_MESSAGE_SERVICE_BASE_URL",
            "https://message.example.test/",
        );
        let root = tempfile::tempdir().unwrap();

        let config = DaemonConfig::for_state_root(root.path()).unwrap();

        assert_eq!(config.service_base_url, "https://service.example.test");
        assert_eq!(config.user_service_base_url, "https://service.example.test");
        assert_eq!(
            config.message_service_base_url,
            "https://message.example.test"
        );
        assert_eq!(
            config.download_base_url,
            "https://service.example.test/daemon"
        );
        assert_eq!(config.did_domain, "service.example.test");
    }

    #[test]
    fn persistent_config_round_trips_and_derives_missing_fields() {
        let _env = EnvGuard::clear();
        let root = tempfile::tempdir().unwrap();
        let mut config = DaemonConfig::for_state_root(root.path()).unwrap();
        config.service_base_url = "https://anpclaw.com".to_string();
        config.user_service_base_url = "https://users.anpclaw.test".to_string();
        config.message_service_base_url = "https://messages.anpclaw.test".to_string();
        config.mail_service_base_url = "https://mail.anpclaw.test".to_string();
        config.download_base_url = "file:///tmp/awiki-daemon".to_string();
        config.did_domain = "anpclaw.com".to_string();
        config.anp_service_endpoint = "https://anpclaw.com/anp-im/rpc".to_string();
        config.anp_service_did = "did:wba:anpclaw.com".to_string();
        config.hermes_gateway_cmd = Some("python -m tui_gateway.entry".to_string());
        config.write_persistent_config().unwrap();

        let loaded = DaemonConfig::for_state_root(root.path()).unwrap();

        assert_eq!(loaded.config_file_path, root.path().join("config.json"));
        assert_eq!(loaded.service_base_url, "https://anpclaw.com");
        assert_eq!(loaded.user_service_base_url, "https://users.anpclaw.test");
        assert_eq!(
            loaded.message_service_base_url,
            "https://messages.anpclaw.test"
        );
        assert_eq!(loaded.mail_service_base_url, "https://mail.anpclaw.test");
        assert_eq!(loaded.download_base_url, "file:///tmp/awiki-daemon");
        assert_eq!(loaded.did_domain, "anpclaw.com");
        assert_eq!(loaded.anp_service_did, "did:wba:anpclaw.com");
        assert_eq!(
            loaded.hermes_gateway_cmd.as_deref(),
            Some("python -m tui_gateway.entry")
        );
    }

    #[test]
    fn hermes_gateway_env_overrides_persistent_config() {
        let _env = EnvGuard::clear();
        let root = tempfile::tempdir().unwrap();
        let mut config = DaemonConfig::for_state_root(root.path()).unwrap();
        config.hermes_gateway_cmd = Some("persisted-python -m tui_gateway.entry".to_string());
        config.write_persistent_config().unwrap();

        std::env::set_var(
            "AWIKI_HERMES_GATEWAY_CMD",
            "env-python -m tui_gateway.entry",
        );
        let loaded = DaemonConfig::for_state_root(root.path()).unwrap();

        assert_eq!(
            loaded.hermes_gateway_cmd.as_deref(),
            Some("env-python -m tui_gateway.entry")
        );
    }

    #[test]
    fn im_core_config_includes_all_service_endpoints() {
        let _env = EnvGuard::clear();
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();

        let im_core_config = config.im_core_config().unwrap();

        assert_eq!(im_core_config.service_base_url.as_str(), "https://awiki.ai");
        assert_eq!(
            im_core_config.user_service_endpoint.unwrap().as_str(),
            "https://awiki.ai"
        );
        assert_eq!(
            im_core_config.message_service_endpoint.unwrap().as_str(),
            "https://awiki.ai"
        );
        assert_eq!(
            im_core_config.mail_service_endpoint.unwrap().as_str(),
            "https://awiki.ai"
        );
        assert_eq!(
            im_core_config.anp_service_endpoint.unwrap().as_str(),
            "https://awiki.ai/anp-im/rpc"
        );
        assert_eq!(
            im_core_config.anp_service_did.unwrap().as_str(),
            "did:wba:awiki.ai"
        );
    }

    #[test]
    fn state_root_rejects_parent_components() {
        let _env = EnvGuard::clear();
        let error = DaemonConfig::for_state_root("../state").unwrap_err();
        assert!(error.to_string().contains("parent directory"));
    }

    #[test]
    fn child_paths_must_stay_under_state_root() {
        let _env = EnvGuard::clear();
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("state");
        let mut config = DaemonConfig::for_state_root(&root).unwrap();
        config.daemon_db_path = std::env::temp_dir().join("outside.db");

        let error = config.validate().unwrap_err();
        assert!(error.to_string().contains("daemon_db_path"));
    }

    #[test]
    fn ensure_state_layout_creates_expected_directories() {
        let _env = EnvGuard::clear();
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();

        config.ensure_state_layout().unwrap();

        assert!(config.config_file_path.parent().unwrap().is_dir());
        assert!(config.identity_root_dir.is_dir());
        assert!(config.runtime_cache_dir.is_dir());
        assert!(config.runtime_temp_dir.is_dir());
        assert!(config.local_socket_path.parent().unwrap().is_dir());
        assert!(config.audit_log_path.parent().unwrap().is_dir());
    }
}
