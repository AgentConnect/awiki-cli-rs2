use std::path::{Component, Path, PathBuf};

use anyhow::{bail, Context, Result};
use im_core::{
    IdentityRegistryPaths, ImCoreConfig, ImCorePaths, LocalStatePaths, MessageTransportPolicy,
    RuntimePaths, ServiceEndpoint,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonConfig {
    pub state_root: PathBuf,
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
    pub did_domain: String,
    pub identity_selector: IdentitySelectorConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonConfigFile {
    pub state_root: PathBuf,
    pub service_base_url: Option<String>,
    pub did_domain: Option<String>,
    pub identity_selector: Option<IdentitySelectorConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value")]
pub enum IdentitySelectorConfig {
    Default,
    LocalAlias(String),
    Did(String),
}

impl DaemonConfig {
    pub fn for_state_root(state_root: impl Into<PathBuf>) -> Result<Self> {
        let state_root = normalize_state_root(state_root.into())?;
        Ok(Self {
            daemon_db_path: state_root.join("daemon.db"),
            im_core_sqlite_path: state_root.join("im-core").join("local-state.sqlite"),
            identity_root_dir: state_root.join("identity"),
            identity_registry_path: state_root.join("identity").join("registry.json"),
            default_identity_path: Some(state_root.join("identity").join("default")),
            runtime_cache_dir: state_root.join("runtime").join("cache"),
            runtime_temp_dir: state_root.join("runtime").join("tmp"),
            local_socket_path: state_root.join("rpc").join("awiki-deamon.sock"),
            audit_log_path: state_root.join("audit").join("audit.log"),
            service_base_url: "https://example.invalid".to_string(),
            did_domain: "awiki.local".to_string(),
            identity_selector: IdentitySelectorConfig::Default,
            state_root,
        })
    }

    pub fn from_config_file(file: DaemonConfigFile) -> Result<Self> {
        let mut config = Self::for_state_root(file.state_root)?;
        if let Some(service_base_url) = file.service_base_url {
            config.service_base_url = service_base_url;
        }
        if let Some(did_domain) = file.did_domain {
            config.did_domain = did_domain;
        }
        if let Some(identity_selector) = file.identity_selector {
            config.identity_selector = identity_selector;
        }
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        validate_state_root(&self.state_root)?;
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
        if self.did_domain.trim().is_empty() {
            bail!("did_domain must not be empty");
        }
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

    #[test]
    fn default_config_uses_state_root_layout() {
        let root = std::env::temp_dir().join("awiki-deamon-config-test");
        let config = DaemonConfig::for_state_root(&root).unwrap();

        assert_eq!(config.daemon_db_path, root.join("daemon.db"));
        assert_eq!(
            config.local_socket_path,
            root.join("rpc").join("awiki-deamon.sock")
        );
        assert_eq!(
            config.identity_registry_path,
            root.join("identity").join("registry.json")
        );
        config.validate().unwrap();
    }

    #[test]
    fn state_root_rejects_parent_components() {
        let error = DaemonConfig::for_state_root("../state").unwrap_err();
        assert!(error.to_string().contains("parent directory"));
    }

    #[test]
    fn child_paths_must_stay_under_state_root() {
        let root = std::env::temp_dir().join("awiki-deamon-config-test");
        let mut config = DaemonConfig::for_state_root(&root).unwrap();
        config.daemon_db_path = std::env::temp_dir().join("outside.db");

        let error = config.validate().unwrap_err();
        assert!(error.to_string().contains("daemon_db_path"));
    }

    #[test]
    fn ensure_state_layout_creates_expected_directories() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();

        config.ensure_state_layout().unwrap();

        assert!(config.identity_root_dir.is_dir());
        assert!(config.runtime_cache_dir.is_dir());
        assert!(config.runtime_temp_dir.is_dir());
        assert!(config.local_socket_path.parent().unwrap().is_dir());
        assert!(config.audit_log_path.parent().unwrap().is_dir());
    }
}
