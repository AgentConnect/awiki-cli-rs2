use super::{
    normalize_did_domain, read_file_config, FileConfig, HermesConfig, HostNotifyConfig,
    IdentityConfig, LegacyWebhookConfig, ListenerConfig, OpenClawConfig, OutputConfig, Paths,
    Resolved, RuntimeConfig, ServicesConfig, UpdateConfig, CONFIG_SCHEMA_VERSION,
    DEFAULT_HOST_NOTIFY_ENABLED, DEFAULT_LISTENER_AUTO_INSTALL, DEFAULT_LISTENER_AUTO_START,
    DEFAULT_LISTENER_ENABLED,
};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn write_file_config(path: &str, resolved: &Resolved) -> anyhow::Result<()> {
    let config = FileConfig {
        schema_version: CONFIG_SCHEMA_VERSION,
        identity: IdentityConfig {
            active: resolved.active_identity.clone(),
        },
        runtime: RuntimeConfig {
            mode: resolved.runtime_mode.clone(),
            socket_path: resolved.runtime_socket_path.clone(),
            listener: ListenerConfig {
                enabled: Some(resolved.runtime_listener_enabled),
                auto_install: Some(resolved.runtime_listener_auto_install),
                auto_start: Some(resolved.runtime_listener_auto_start),
            },
            host_notify: HostNotifyConfig {
                enabled: Some(resolved.host_notify_enabled),
                sink: resolved.host_notify_sink.clone(),
                file_path: resolved.host_notify_file_path.clone(),
                openclaw: OpenClawConfig {
                    hook_url: resolved.host_notify_openclaw_hook_url.clone(),
                    agent_id: resolved.host_notify_openclaw_agent_id.clone(),
                    hook_name: resolved.host_notify_openclaw_hook_name.clone(),
                    token: String::new(),
                },
                hermes: HermesConfig {
                    notify_url: resolved.host_notify_hermes_notify_url.clone(),
                    deliver: resolved.host_notify_hermes_deliver.clone(),
                    secret: String::new(),
                },
                webhook: LegacyWebhookConfig::default(),
            },
        },
        output: OutputConfig {
            format: resolved.output_format.clone(),
            no_color: Some(resolved.no_color),
        },
        services: ServicesConfig {
            service_base_url: resolved.service_base_url.clone(),
            did_domain: resolved.did_domain.clone(),
            anp_service_endpoint: resolved.anp_service_endpoint.clone(),
            anp_service_did: resolved.anp_service_did.clone(),
            ca_bundle: resolved.ca_bundle.clone(),
            mail_service_url: resolved.mail_service_url.clone(),
        },
        update: UpdateConfig::default(),
    };
    write_raw_file_config(path, &config)
}

pub fn ensure_config_schema_version(path: &str) -> anyhow::Result<()> {
    let (mut config, exists, error) = read_file_config(path);
    if !error.is_empty() {
        anyhow::bail!(error);
    }
    if !exists {
        return Ok(());
    }
    config.schema_version = CONFIG_SCHEMA_VERSION;
    write_raw_file_config(path, &config)
}

pub fn update_runtime_settings(paths: &Paths, mode: &str, socket_path: &str) -> anyhow::Result<()> {
    update_file_config(&paths.config_file, |config| {
        config.runtime.mode = mode.to_string();
        if !socket_path.is_empty() {
            config.runtime.socket_path = socket_path.to_string();
        }
        Ok(())
    })
}

pub fn update_active_identity(paths: &Paths, identity_name: &str) -> anyhow::Result<()> {
    update_file_config(&paths.config_file, |config| {
        config.identity.active = identity_name.trim().to_string();
        Ok(())
    })
}

pub fn update_did_domain(paths: &Paths, value: &str) -> anyhow::Result<String> {
    let normalized = normalize_did_domain(value)?;
    update_file_config(&paths.config_file, |config| {
        config.services.did_domain = normalized.clone();
        Ok(())
    })?;
    Ok(normalized)
}

pub fn update_runtime_listener_settings(
    paths: &Paths,
    enabled: Option<bool>,
    auto_install: Option<bool>,
    auto_start: Option<bool>,
) -> anyhow::Result<()> {
    update_file_config(&paths.config_file, |config| {
        if let Some(value) = enabled {
            config.runtime.listener.enabled = Some(value);
        }
        if let Some(value) = auto_install {
            config.runtime.listener.auto_install = Some(value);
        }
        if let Some(value) = auto_start {
            config.runtime.listener.auto_start = Some(value);
        }
        Ok(())
    })
}

pub fn update_host_notify_sink(paths: &Paths, sink: &str) -> anyhow::Result<()> {
    let mut normalized = sink.trim().to_ascii_lowercase();
    if normalized == "webhook" {
        normalized = "hermes".to_string();
    }
    update_file_config(&paths.config_file, |config| {
        config.runtime.host_notify.sink = normalized;
        config.runtime.host_notify.enabled = Some(true);
        Ok(())
    })
}

pub fn update_host_notify_enabled(paths: &Paths, enabled: bool) -> anyhow::Result<()> {
    update_file_config(&paths.config_file, |config| {
        config.runtime.host_notify.enabled = Some(enabled);
        Ok(())
    })
}

pub fn update_openclaw_settings(paths: &Paths, hook_url: Option<&str>) -> anyhow::Result<()> {
    update_file_config(&paths.config_file, |config| {
        if let Some(value) = hook_url {
            config.runtime.host_notify.openclaw.hook_url = value.trim().to_string();
        }
        Ok(())
    })
}

pub fn update_hermes_settings(
    paths: &Paths,
    notify_url: Option<&str>,
    deliver: Option<&str>,
) -> anyhow::Result<()> {
    update_file_config(&paths.config_file, |config| {
        if let Some(value) = notify_url {
            let value = value.trim().to_string();
            config.runtime.host_notify.hermes.notify_url = value.clone();
            config.runtime.host_notify.webhook.notify_url = value;
        }
        if let Some(value) = deliver {
            config.runtime.host_notify.hermes.deliver = value.trim().to_ascii_lowercase();
        }
        Ok(())
    })
}

pub fn configure_hermes_host_notify(
    paths: &Paths,
    notify_url: &str,
    secret: Option<&str>,
    deliver: &str,
    enabled: bool,
) -> anyhow::Result<()> {
    update_file_config(&paths.config_file, |config| {
        let notify_url = notify_url.trim().to_string();
        config.runtime.host_notify.enabled = Some(enabled);
        config.runtime.host_notify.sink = "hermes".to_string();
        config.runtime.host_notify.hermes.notify_url = notify_url.clone();
        config.runtime.host_notify.hermes.deliver = deliver.trim().to_ascii_lowercase();
        config.runtime.host_notify.webhook.notify_url = notify_url;
        if let Some(value) = secret {
            let value = value.trim().to_string();
            config.runtime.host_notify.hermes.secret = value.clone();
            config.runtime.host_notify.webhook.secret = value;
        }
        Ok(())
    })
}

pub fn set_openclaw_token(paths: &Paths, token: &str) -> anyhow::Result<()> {
    update_file_config(&paths.config_file, |config| {
        config.runtime.host_notify.openclaw.token = token.to_string();
        Ok(())
    })
}

pub fn clear_openclaw_token(paths: &Paths) -> anyhow::Result<()> {
    update_file_config(&paths.config_file, |config| {
        config.runtime.host_notify.openclaw.token.clear();
        Ok(())
    })
}

pub fn set_hermes_secret(paths: &Paths, secret: &str) -> anyhow::Result<()> {
    update_file_config(&paths.config_file, |config| {
        config.runtime.host_notify.hermes.secret = secret.to_string();
        config.runtime.host_notify.webhook.secret = secret.to_string();
        Ok(())
    })
}

pub fn clear_hermes_secret(paths: &Paths) -> anyhow::Result<()> {
    update_file_config(&paths.config_file, |config| {
        config.runtime.host_notify.hermes.secret.clear();
        config.runtime.host_notify.webhook.secret.clear();
        Ok(())
    })
}

pub fn read_openclaw_token(paths: &Paths) -> (String, String) {
    if paths.config_file.trim().is_empty() {
        return (String::new(), "unset".to_string());
    }
    let (config, _, error) = read_file_config(&paths.config_file);
    if error.is_empty() {
        let token = config.runtime.host_notify.openclaw.token.trim();
        if !token.is_empty() {
            return (token.to_string(), "config_file".to_string());
        }
    }
    (String::new(), "unset".to_string())
}

fn update_file_config(
    path: &str,
    mutate: impl FnOnce(&mut FileConfig) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    if let Some(parent) = Path::new(path).parent() {
        create_config_dir(parent)?;
    }
    let (mut config, _, error) = read_file_config(path);
    if !error.is_empty() {
        anyhow::bail!(error);
    }
    mutate(&mut config)?;
    config.schema_version = CONFIG_SCHEMA_VERSION;
    write_raw_file_config(path, &config)
}

fn write_raw_file_config(path: &str, config: &FileConfig) -> anyhow::Result<()> {
    write_atomic_file(
        Path::new(path),
        render_file_config(config).as_bytes(),
        0o600,
    )
}

fn write_atomic_file(path: &Path, content: &[u8], mode: u32) -> anyhow::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    create_config_dir(parent)?;
    let (mut temp_file, temp_path) = create_temp_config_file(parent)?;
    let mut cleanup = TempCleanup::new(temp_path.clone());

    temp_file
        .write_all(content)
        .map_err(|err| anyhow::anyhow!("write temp config file: {err}"))?;
    temp_file
        .sync_all()
        .map_err(|err| anyhow::anyhow!("sync temp config file: {err}"))?;
    drop(temp_file);
    set_file_mode(&temp_path, mode)?;
    fs::rename(&temp_path, path).map_err(|err| anyhow::anyhow!("replace config file: {err}"))?;
    cleanup.keep();
    sync_directory(parent)?;
    Ok(())
}

fn create_config_dir(path: &Path) -> anyhow::Result<()> {
    if path.exists() {
        return Ok(());
    }
    fs::create_dir_all(path).map_err(|err| anyhow::anyhow!("create config dir: {err}"))?;
    set_dir_mode(path, 0o700)
}

fn create_temp_config_file(parent: &Path) -> anyhow::Result<(File, PathBuf)> {
    for attempt in 0..100 {
        let path = parent.join(temp_config_name(attempt));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((file, path)),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(anyhow::anyhow!("create temp config file: {err}")),
        }
    }
    anyhow::bail!("create temp config file: too many temporary name collisions")
}

fn temp_config_name(attempt: u32) -> String {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!(".config-{}-{}-{attempt}.tmp", std::process::id(), nonce)
}

#[cfg(unix)]
fn set_dir_mode(path: &Path, mode: u32) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|err| anyhow::anyhow!("chmod config dir: {err}"))
}

#[cfg(not(unix))]
fn set_dir_mode(_path: &Path, _mode: u32) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn set_file_mode(path: &Path, mode: u32) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|err| anyhow::anyhow!("chmod temp config file: {err}"))
}

#[cfg(not(unix))]
fn set_file_mode(_path: &Path, _mode: u32) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(windows)]
fn sync_directory(_path: &Path) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(not(windows))]
fn sync_directory(path: &Path) -> anyhow::Result<()> {
    File::open(path)
        .and_then(|dir| dir.sync_all())
        .map_err(|err| anyhow::anyhow!("sync config dir: {err}"))
}

struct TempCleanup {
    path: PathBuf,
    cleanup: bool,
}

impl TempCleanup {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            cleanup: true,
        }
    }

    fn keep(&mut self) {
        self.cleanup = false;
    }
}

impl Drop for TempCleanup {
    fn drop(&mut self) {
        if self.cleanup {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn render_file_config(config: &FileConfig) -> String {
    format!(
        concat!(
            "schema_version: {}\n",
            "identity:\n",
            "  active: {}\n",
            "runtime:\n",
            "  mode: {}\n",
            "  socket_path: {}\n",
            "  listener:\n",
            "    enabled: {}\n",
            "    auto_install: {}\n",
            "    auto_start: {}\n",
            "  host_notify:\n",
            "    enabled: {}\n",
            "    sink: {}\n",
            "    file_path: {}\n",
            "    openclaw:\n",
            "      hook_url: {}\n",
            "      agent_id: {}\n",
            "      hook_name: {}\n",
            "      token: {}\n",
            "    hermes:\n",
            "      notify_url: {}\n",
            "      deliver: {}\n",
            "      secret: {}\n",
            "    webhook:\n",
            "      notify_url: {}\n",
            "      secret: {}\n",
            "output:\n",
            "  format: {}\n",
            "  no_color: {}\n",
            "services:\n",
            "  service_base_url: {}\n",
            "  did_domain: {}\n",
            "  anp_service_endpoint: {}\n",
            "  anp_service_did: {}\n",
            "  ca_bundle: {}\n",
            "  mail_service_url: {}\n",
            "update:\n",
            "  disable_strict_version: {}\n",
            "  metadata_cache_ttl_seconds: {}\n"
        ),
        CONFIG_SCHEMA_VERSION,
        config.identity.active,
        config.runtime.mode,
        config.runtime.socket_path,
        config
            .runtime
            .listener
            .enabled
            .unwrap_or(DEFAULT_LISTENER_ENABLED),
        config
            .runtime
            .listener
            .auto_install
            .unwrap_or(DEFAULT_LISTENER_AUTO_INSTALL),
        config
            .runtime
            .listener
            .auto_start
            .unwrap_or(DEFAULT_LISTENER_AUTO_START),
        config
            .runtime
            .host_notify
            .enabled
            .unwrap_or(DEFAULT_HOST_NOTIFY_ENABLED),
        config.runtime.host_notify.sink,
        config.runtime.host_notify.file_path,
        config.runtime.host_notify.openclaw.hook_url,
        config.runtime.host_notify.openclaw.agent_id,
        config.runtime.host_notify.openclaw.hook_name,
        config.runtime.host_notify.openclaw.token,
        config.runtime.host_notify.hermes.notify_url,
        config.runtime.host_notify.hermes.deliver,
        config.runtime.host_notify.hermes.secret,
        config.runtime.host_notify.webhook.notify_url,
        config.runtime.host_notify.webhook.secret,
        config.output.format,
        config.output.no_color.unwrap_or(false),
        config.services.service_base_url,
        config.services.did_domain,
        config.services.anp_service_endpoint,
        config.services.anp_service_did,
        config.services.ca_bundle,
        config.services.mail_service_url,
        config.update.disable_strict_version,
        config.update.metadata_cache_ttl_seconds,
    )
}
