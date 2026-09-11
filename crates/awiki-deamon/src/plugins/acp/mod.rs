use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::runtime::RuntimeAgentProfile;
use crate::state::{AcpRuntimeProfileRecord, DaemonState};
use crate::DaemonConfig;

pub mod catalog;
pub mod connection;
pub mod install;
pub mod protocol;
pub mod runner;

pub const ACP_RUNTIME_PLUGIN_ID: &str = "runtime.acp";

#[derive(Clone, Copy)]
pub struct AcpProfileInitRequest<'a> {
    pub acp_agent_id: &'a str,
    pub driver_config: Option<&'a Value>,
    pub secrets: Option<&'a Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpProfileInstallResult {
    pub record: AcpRuntimeProfileRecord,
    pub profile_dir: PathBuf,
    pub dotenv_path: PathBuf,
    pub installed_packages: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpCatalogInstallRequest {
    pub acp_agent_id: String,
    pub install_mode: String,
    pub local_checkout: Option<PathBuf>,
    pub package_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpCatalogInstallResult {
    pub acp_agent_id: String,
    pub install_mode: String,
    pub installed_version: String,
    pub installed_packages: Vec<String>,
}

type ResolvedCredentials = (
    Vec<String>,
    BTreeMap<String, String>,
    BTreeMap<String, String>,
);

pub fn install_acp_catalog_agent(
    config: &DaemonConfig,
    request: &AcpCatalogInstallRequest,
) -> Result<AcpCatalogInstallResult> {
    let catalog = catalog::entry(&request.acp_agent_id)?;
    let package_version = request
        .package_version
        .as_deref()
        .unwrap_or(catalog.default_npm_version);
    let installed = match request.install_mode.as_str() {
        "local" => {
            let checkout = request
                .local_checkout
                .as_deref()
                .context("ACP local install requires local_checkout")?;
            install::prepare_local(catalog, checkout)?
        }
        "npm" => install::install_npm(
            config,
            catalog,
            request
                .package_version
                .as_deref()
                .unwrap_or(catalog.default_npm_version),
        )?,
        other => bail!("unsupported ACP install_mode: {other}"),
    };
    Ok(AcpCatalogInstallResult {
        acp_agent_id: catalog.agent_id.to_string(),
        install_mode: request.install_mode.clone(),
        installed_version: installed.version,
        installed_packages: if request.install_mode == "npm" {
            (catalog.npm_packages)(package_version)
        } else {
            Vec::new()
        },
    })
}

pub fn initialize_acp_profile(
    config: &DaemonConfig,
    state: &DaemonState,
    profile: &RuntimeAgentProfile,
    request: AcpProfileInitRequest<'_>,
) -> Result<AcpProfileInstallResult> {
    let result = initialize_acp_profile_inner(config, state, profile, request);
    if result.is_err() {
        let _ = mark_acp_profile_failed(config, state, profile, request);
    }
    result
}

fn initialize_acp_profile_inner(
    config: &DaemonConfig,
    state: &DaemonState,
    profile: &RuntimeAgentProfile,
    request: AcpProfileInitRequest<'_>,
) -> Result<AcpProfileInstallResult> {
    if profile.runtime_plugin_id != ACP_RUNTIME_PLUGIN_ID {
        bail!("ACP profile initialization requires runtime.acp");
    }
    profile.validate()?;
    let catalog = catalog::entry(request.acp_agent_id)?;
    let driver_config = optional_object(request.driver_config, "ACP driver_config")?;
    let install_mode = driver_config
        .get("install_mode")
        .and_then(Value::as_str)
        .unwrap_or("npm");
    let permission_policy = driver_config
        .get("permission_policy")
        .and_then(Value::as_str)
        .unwrap_or(catalog.default_permission_policy);
    if !matches!(permission_policy, "allow-once" | "reject-once") {
        bail!("ACP permission_policy must be allow-once or reject-once");
    }

    let profile_dir = config
        .state_root
        .join("runtime")
        .join("acp")
        .join("profiles")
        .join(&profile.runtime_profile_id);
    create_private_dir_all(&profile_dir)?;
    let cwd_root = profile_dir.join("workspace");
    let persistence_root = profile_dir.join("sessions");
    create_private_dir_all(&cwd_root)?;
    create_private_dir_all(&persistence_root)?;

    let (installed, installed_packages) = match install_mode {
        "local" => {
            let checkout = driver_config
                .get("local_checkout")
                .and_then(Value::as_str)
                .map(PathBuf::from)
                .context("ACP local install requires driver_config.local_checkout")?;
            (install::prepare_local(catalog, &checkout)?, Vec::new())
        }
        "npm" => {
            let version = driver_config
                .get("package_version")
                .and_then(Value::as_str)
                .unwrap_or(catalog.default_npm_version);
            (
                install::install_npm(config, catalog, version)?,
                (catalog.npm_packages)(version),
            )
        }
        other => bail!("unsupported ACP install_mode: {other}"),
    };
    link_profile_node_modules(&profile_dir, &installed.install_root, install_mode, catalog)?;

    let config_path = profile_dir.join("cordis.yml");
    let config_text = catalog.render_config(&cwd_root, &persistence_root)?;
    write_private_file(&config_path, config_text.as_bytes())?;

    let supplied_secrets = optional_object(request.secrets, "ACP secrets")?;
    let (credential_env_names, process_env, dotenv_values) =
        resolve_credentials(catalog, supplied_secrets)?;
    let dotenv_path = profile_dir.join(".env");
    write_private_file(&dotenv_path, render_dotenv(&dotenv_values)?.as_bytes())?;

    let launch_args = catalog.launch_args(&installed.entrypoint, &config_path);
    let launch_cwd = catalog.launch_cwd(&installed.install_root, &profile_dir, &cwd_root);
    let record = AcpRuntimeProfileRecord {
        runtime_profile_id: profile.runtime_profile_id.clone(),
        agent_did: profile.agent_did.clone(),
        acp_agent_id: catalog.agent_id.to_string(),
        install_mode: install_mode.to_string(),
        install_root: installed.install_root,
        entry_command_json: serde_json::json!({
            "program": installed.program,
            "args": launch_args,
            "cwd": launch_cwd,
        }),
        config_path,
        cwd_root,
        credential_env_names,
        permission_policy: permission_policy.to_string(),
        installed_version: Some(installed.version),
        status: "ready".to_string(),
    };
    state.upsert_acp_runtime_profile(&record)?;
    let plugin = runner::AcpRuntimePlugin::with_state(
        connection::AcpProcessPool::default(),
        record.clone(),
        state.clone(),
    );
    if let Err(error) = plugin.smoke_check_with_env(process_env) {
        let mut failed = record.clone();
        failed.status = "failed".to_string();
        state.upsert_acp_runtime_profile(&failed)?;
        return Err(error).context("ACP initialize handshake smoke check");
    }
    Ok(AcpProfileInstallResult {
        record,
        profile_dir,
        dotenv_path,
        installed_packages,
    })
}

pub fn mark_acp_profile_failed(
    config: &DaemonConfig,
    state: &DaemonState,
    profile: &RuntimeAgentProfile,
    request: AcpProfileInitRequest<'_>,
) -> Result<()> {
    let catalog = catalog::entry(request.acp_agent_id)?;
    let driver_config = optional_object(request.driver_config, "ACP driver_config")?;
    let install_mode = driver_config
        .get("install_mode")
        .and_then(Value::as_str)
        .unwrap_or("npm");
    let default_install_root = config
        .state_root
        .join("runtime")
        .join("acp")
        .join(catalog.agent_id);
    let install_root = driver_config
        .get("local_checkout")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or(default_install_root);
    let profile_dir = config
        .state_root
        .join("runtime")
        .join("acp")
        .join("profiles")
        .join(&profile.runtime_profile_id);
    let permission_policy = driver_config
        .get("permission_policy")
        .and_then(Value::as_str)
        .filter(|value| matches!(*value, "allow-once" | "reject-once"))
        .unwrap_or(catalog.default_permission_policy);
    state.upsert_acp_runtime_profile(&AcpRuntimeProfileRecord {
        runtime_profile_id: profile.runtime_profile_id.clone(),
        agent_did: profile.agent_did.clone(),
        acp_agent_id: catalog.agent_id.to_string(),
        install_mode: if matches!(install_mode, "npm" | "local") {
            install_mode.to_string()
        } else {
            "npm".to_string()
        },
        install_root,
        entry_command_json: serde_json::json!({"program": catalog.program_name, "args": []}),
        config_path: profile_dir.join("cordis.yml"),
        cwd_root: profile_dir.join("workspace"),
        credential_env_names: catalog
            .credential_env_names
            .iter()
            .map(|name| (*name).to_string())
            .collect(),
        permission_policy: permission_policy.to_string(),
        installed_version: None,
        status: "failed".to_string(),
    })
}

fn optional_object<'a>(
    value: Option<&'a Value>,
    field: &str,
) -> Result<&'a serde_json::Map<String, Value>> {
    match value {
        Some(Value::Object(object)) => Ok(object),
        Some(_) => bail!("{field} must be an object"),
        None => Ok(empty_object()),
    }
}

fn empty_object() -> &'static serde_json::Map<String, Value> {
    static EMPTY: std::sync::OnceLock<serde_json::Map<String, Value>> = std::sync::OnceLock::new();
    EMPTY.get_or_init(serde_json::Map::new)
}

fn resolve_credentials(
    catalog: &catalog::AcpAgentCatalogEntry,
    supplied: &serde_json::Map<String, Value>,
) -> Result<ResolvedCredentials> {
    for key in supplied.keys() {
        if !catalog.credential_env_names.contains(&key.as_str()) {
            bail!("unsupported ACP secret variable name: {key}");
        }
    }
    let mut names = Vec::new();
    let mut process_env = BTreeMap::new();
    let mut dotenv_values = BTreeMap::new();
    for name in catalog.credential_env_names {
        let supplied_value = supplied
            .get(*name)
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_string)
                    .context("ACP secret values must be strings")
            })
            .transpose()?;
        let value = supplied_value
            .clone()
            .or_else(|| std::env::var(name).ok())
            .filter(|value| !value.is_empty());
        if catalog.required_credential_env_names.contains(name) && value.is_none() {
            bail!("ACP credential {name} is required");
        }
        if let Some(value) = value {
            names.push((*name).to_string());
            process_env.insert((*name).to_string(), value.clone());
            if supplied_value.is_some() {
                dotenv_values.insert((*name).to_string(), value);
            }
        }
    }
    Ok((names, process_env, dotenv_values))
}

fn render_dotenv(values: &BTreeMap<String, String>) -> Result<String> {
    let mut lines = vec!["# Managed by awiki-deamon. Contains ACP credentials.".to_string()];
    for (name, value) in values {
        if value.contains(['\r', '\n']) {
            bail!("ACP secret values must not contain line breaks");
        }
        lines.push(format!("{name}={}", serde_json::to_string(value)?));
    }
    lines.push(String::new());
    Ok(lines.join("\n"))
}

fn create_private_dir_all(path: &Path) -> Result<()> {
    if std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        bail!("refusing to use ACP private directory symlink");
    }
    std::fs::create_dir_all(path).with_context(|| format!("create {}", path.display()))?;
    let metadata = std::fs::symlink_metadata(path)
        .with_context(|| format!("inspect ACP private directory {}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("ACP private directory must be a real directory");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn write_private_file(path: &Path, content: &[u8]) -> Result<()> {
    use std::io::Write;
    if std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        bail!("refusing to write ACP private file symlink");
    }
    let mut options = std::fs::OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options
        .open(path)
        .with_context(|| format!("open {}", path.display()))?;
    file.write_all(content)
        .with_context(|| format!("write {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn link_profile_node_modules(
    profile_dir: &Path,
    install_root: &Path,
    install_mode: &str,
    catalog: &catalog::AcpAgentCatalogEntry,
) -> Result<()> {
    if install_mode == "local" {
        let node_modules_dir = profile_dir.join("node_modules");
        create_private_dir_all(&node_modules_dir)?;
        for (link_path, checkout_path) in catalog.local_package_paths {
            let link = node_modules_dir.join(link_path);
            let parent = link
                .parent()
                .context("ACP local package link needs a parent")?;
            create_private_dir_all(parent)?;
            link_directory(&link, &install_root.join(checkout_path))?;
        }
        return Ok(());
    }
    link_directory(
        &profile_dir.join("node_modules"),
        &install_root.join("node_modules"),
    )
}

fn link_directory(link: &Path, target: &Path) -> Result<()> {
    match std::fs::symlink_metadata(link) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            if std::fs::read_link(link)? == target {
                return Ok(());
            }
            bail!("ACP profile node_modules link points to an unexpected target");
        }
        Ok(_) => bail!("ACP profile node_modules path must be a managed symlink"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("inspect ACP profile node_modules link"),
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link)
        .with_context(|| format!("link ACP profile node_modules {}", link.display()))?;
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(target, link)
        .with_context(|| format!("link ACP profile node_modules {}", link.display()))?;
    #[cfg(not(any(unix, windows)))]
    bail!("ACP profile node_modules links are unsupported on this platform");
    Ok(())
}
