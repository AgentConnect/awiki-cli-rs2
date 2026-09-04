use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::DaemonConfig;

const POLICY_PRODUCT: &str = "awiki-daemon";
const POLICY_CHANNEL: &str = "stable";
const POLICY_RESPONSE_MAX_BYTES: usize = 1024 * 1024;
const POLICY_HTTP_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const POLICY_HTTP_TIMEOUT: Duration = Duration::from_secs(8);
const POLICY_CACHE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DaemonUpdatePolicySource {
    Network,
    Cache,
}

impl DaemonUpdatePolicySource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Network => "network",
            Self::Cache => "cache",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonUpdatePolicy {
    pub enabled: bool,
    pub policy_origin: String,
    pub policy_revision: u64,
    pub published_at: String,
    pub recommended_version: Option<String>,
    pub minimum_supported_version: Option<String>,
    pub upgrade_url: Option<String>,
    pub artifact_manifest_url: Option<String>,
    pub policy_url: String,
    pub source: DaemonUpdatePolicySource,
    /// A cache-backed policy remains usable when refresh fails. This field keeps
    /// that failure visible to diagnostics without turning the policy into an error.
    pub refresh_error: Option<String>,
}

impl DaemonUpdatePolicy {
    pub fn uses_cache(&self) -> bool {
        self.source == DaemonUpdatePolicySource::Cache
    }

    pub fn recommends_newer_than(&self, current: &str) -> bool {
        let Some(recommended) = self.recommended_version.as_deref() else {
            return false;
        };
        match (Version::parse(recommended), Version::parse(current)) {
            (Ok(recommended), Ok(current)) => recommended > current,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CachedDaemonUpdatePolicy {
    schema_version: u32,
    product: String,
    channel: String,
    policy_origin: String,
    policy_revision: u64,
    published_at: String,
    enabled: bool,
    recommended_version: Option<String>,
    minimum_supported_version: Option<String>,
    upgrade_url: Option<String>,
    artifact_manifest_url: Option<String>,
    retrieved_at: String,
}

#[derive(Debug, Deserialize)]
struct ServerInfoResponse {
    #[serde(default)]
    schema_version: u64,
    #[serde(default)]
    client_versions: Option<ServerClientVersions>,
}

#[derive(Debug, Deserialize)]
struct ServerClientVersions {
    #[serde(default)]
    schema_version: u64,
    #[serde(default)]
    channel: String,
    #[serde(default)]
    policy_origin: String,
    #[serde(default)]
    policy_revision: u64,
    #[serde(default)]
    published_at: String,
    products: ServerProducts,
}

#[derive(Debug, Deserialize)]
struct ServerProducts {
    daemon: ServerDaemonPolicy,
}

#[derive(Debug, Default, Deserialize)]
struct ServerDaemonPolicy {
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    recommended_version: Option<String>,
    #[serde(default)]
    minimum_supported_version: Option<String>,
    #[serde(default)]
    upgrade_url: Option<String>,
    #[serde(default)]
    artifact_manifest_url: Option<String>,
}

pub fn load_daemon_update_policy(config: &DaemonConfig) -> Result<DaemonUpdatePolicy> {
    let expected_origin = tenant_origin(&config.user_service_base_url)?;
    let policy_url = daemon_update_policy_url(config)?;
    let cache_path = policy_cache_path(config, &expected_origin);
    let cached = read_cached_policy(&cache_path, &expected_origin, &policy_url)
        .ok()
        .flatten();

    match fetch_policy_blocking(config, &policy_url, &expected_origin) {
        Ok(network) => {
            if let Some(cached) = cached.as_ref() {
                if network.policy_revision < cached.policy_revision {
                    return Ok(cache_after_refresh_failure(
                        cached.clone(),
                        format!(
                            "daemon update policy revision rolled back from {} to {}",
                            cached.policy_revision, network.policy_revision
                        ),
                    ));
                }
                if network.policy_revision == cached.policy_revision
                    && policy_identity(&network) != policy_identity(cached)
                {
                    return Ok(cache_after_refresh_failure(
                        cached.clone(),
                        "daemon update policy changed without increasing policy_revision"
                            .to_string(),
                    ));
                }
            }
            let mut network = network;
            if let Err(error) = write_cached_policy(&cache_path, &network) {
                network.refresh_error = Some(format!(
                    "tenant daemon update policy cache write failed: {error}"
                ));
            }
            Ok(network)
        }
        Err(error) => {
            if let Some(cached) = cached {
                Ok(cache_after_refresh_failure(cached, error.to_string()))
            } else {
                Err(error)
            }
        }
    }
}

pub fn daemon_update_policy_url(config: &DaemonConfig) -> Result<String> {
    let origin = tenant_origin(&config.user_service_base_url)?;
    Ok(format!(
        "{}/user-service/v1/server-info?client_platform=daemon",
        origin.trim_end_matches('/')
    ))
}

fn cache_after_refresh_failure(
    mut cached: DaemonUpdatePolicy,
    error: String,
) -> DaemonUpdatePolicy {
    cached.source = DaemonUpdatePolicySource::Cache;
    cached.refresh_error = Some(error);
    cached
}

fn policy_identity(
    policy: &DaemonUpdatePolicy,
) -> (
    &str,
    u64,
    bool,
    Option<&str>,
    Option<&str>,
    Option<&str>,
    Option<&str>,
    &str,
) {
    (
        policy.policy_origin.as_str(),
        policy.policy_revision,
        policy.enabled,
        policy.recommended_version.as_deref(),
        policy.minimum_supported_version.as_deref(),
        policy.upgrade_url.as_deref(),
        policy.artifact_manifest_url.as_deref(),
        policy.published_at.as_str(),
    )
}

fn fetch_policy_blocking(
    config: &DaemonConfig,
    policy_url: &str,
    expected_origin: &str,
) -> Result<DaemonUpdatePolicy> {
    if tokio::runtime::Handle::try_current().is_ok() {
        let config = config.clone();
        let policy_url = policy_url.to_string();
        let expected_origin = expected_origin.to_string();
        return std::thread::Builder::new()
            .name("awiki-daemon-update-policy".to_string())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .context("create daemon update policy runtime")?;
                runtime.block_on(fetch_policy(&config, &policy_url, &expected_origin))
            })
            .context("spawn daemon update policy runtime thread")?
            .join()
            .map_err(|_| anyhow::anyhow!("daemon update policy runtime thread panicked"))?;
    }

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("create daemon update policy runtime")?;
    runtime.block_on(fetch_policy(config, policy_url, expected_origin))
}

async fn fetch_policy(
    config: &DaemonConfig,
    policy_url: &str,
    expected_origin: &str,
) -> Result<DaemonUpdatePolicy> {
    let client = reqwest::Client::builder()
        .connect_timeout(POLICY_HTTP_CONNECT_TIMEOUT)
        .timeout(POLICY_HTTP_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .context("create daemon update policy HTTP client")?;
    let mut response = client
        .get(policy_url)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .context("request tenant daemon update policy")?;
    if response.status().is_redirection() {
        bail!("tenant daemon update policy must not redirect");
    }
    if !response.status().is_success() {
        bail!(
            "tenant daemon update policy returned HTTP status {}",
            response.status()
        );
    }
    if response
        .content_length()
        .is_some_and(|length| length > POLICY_RESPONSE_MAX_BYTES as u64)
    {
        bail!("tenant daemon update policy response exceeds 1 MiB");
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .context("read tenant daemon update policy response")?
    {
        if bytes.len().saturating_add(chunk.len()) > POLICY_RESPONSE_MAX_BYTES {
            bail!("tenant daemon update policy response exceeds 1 MiB");
        }
        bytes.extend_from_slice(&chunk);
    }
    let body: ServerInfoResponse =
        serde_json::from_slice(&bytes).context("parse tenant daemon update policy")?;
    policy_from_server_info(config, body, policy_url, expected_origin)
}

fn policy_from_server_info(
    config: &DaemonConfig,
    body: ServerInfoResponse,
    policy_url: &str,
    expected_origin: &str,
) -> Result<DaemonUpdatePolicy> {
    if body.schema_version != 1 {
        bail!("server-info has an unsupported schema version");
    }
    let releases = body
        .client_versions
        .context("selected tenant does not publish client version policy")?;
    if releases.schema_version != 1 || releases.channel != POLICY_CHANNEL {
        bail!("server-info daemon policy has an unsupported schema or channel");
    }
    if releases.policy_revision == 0 {
        bail!("server-info daemon policy revision must be positive");
    }
    let policy_origin = policy_origin(&releases.policy_origin)?;
    if policy_origin != expected_origin {
        bail!("server-info daemon policy origin does not match the registered tenant");
    }
    OffsetDateTime::parse(releases.published_at.trim(), &Rfc3339)
        .context("server-info daemon policy published_at is invalid")?;

    let daemon = releases.products.daemon;
    if !daemon.enabled {
        return Ok(DaemonUpdatePolicy {
            enabled: false,
            policy_origin,
            policy_revision: releases.policy_revision,
            published_at: releases.published_at,
            recommended_version: None,
            minimum_supported_version: None,
            upgrade_url: None,
            artifact_manifest_url: None,
            policy_url: policy_url.to_string(),
            source: DaemonUpdatePolicySource::Network,
            refresh_error: None,
        });
    }

    let recommended_version = required(daemon.recommended_version, "recommended_version")?;
    let minimum_supported_version = required(
        daemon.minimum_supported_version,
        "minimum_supported_version",
    )?;
    let recommended = Version::parse(&recommended_version)
        .context("server-info daemon recommended_version is invalid")?;
    let minimum = Version::parse(&minimum_supported_version)
        .context("server-info daemon minimum_supported_version is invalid")?;
    if minimum > recommended {
        bail!("server-info daemon minimum version exceeds recommended version");
    }
    let upgrade_url = required(daemon.upgrade_url, "upgrade_url")?;
    validate_https_or_debug_loopback_url(&upgrade_url, false)?;

    // Revision 3 briefly shipped before this machine-readable field existed.
    // The registered tenant's persisted download base is the only safe bridge;
    // it is never replaced with another official tenant's source.
    let artifact_manifest_url = match daemon
        .artifact_manifest_url
        .filter(|value| !value.trim().is_empty())
    {
        Some(value) => value,
        None if releases.policy_revision <= 3 => {
            manifest_url_from_download_base(&config.download_base_url)
        }
        None => bail!("server-info daemon policy is missing artifact_manifest_url"),
    };
    validate_https_or_debug_loopback_url(&artifact_manifest_url, true)?;
    if tenant_origin(&artifact_manifest_url)? != expected_origin {
        bail!("server-info daemon artifact manifest must use the registered tenant origin");
    }

    Ok(DaemonUpdatePolicy {
        enabled: true,
        policy_origin,
        policy_revision: releases.policy_revision,
        published_at: releases.published_at,
        recommended_version: Some(recommended_version),
        minimum_supported_version: Some(minimum_supported_version),
        upgrade_url: Some(upgrade_url),
        artifact_manifest_url: Some(artifact_manifest_url),
        policy_url: policy_url.to_string(),
        source: DaemonUpdatePolicySource::Network,
        refresh_error: None,
    })
}

fn required(value: Option<String>, field: &str) -> Result<String> {
    let value = value.unwrap_or_default().trim().to_string();
    if value.is_empty() {
        bail!("server-info daemon policy is missing {field}");
    }
    Ok(value)
}

fn tenant_origin(raw: &str) -> Result<String> {
    let url = reqwest::Url::parse(raw.trim()).context("parse tenant service URL")?;
    validate_url_transport(&url)?;
    if !url.username().is_empty() || url.password().is_some() || url.host_str().is_none() {
        bail!("tenant service URL has invalid authority");
    }
    let origin = url.origin().ascii_serialization();
    if origin == "null" {
        bail!("tenant service URL has no network origin");
    }
    Ok(origin)
}

fn policy_origin(raw: &str) -> Result<String> {
    let url = reqwest::Url::parse(raw.trim()).context("parse daemon policy origin")?;
    validate_url_transport(&url)?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.host_str().is_none()
        || (url.path() != "/" && !url.path().is_empty())
        || url.query().is_some()
        || url.fragment().is_some()
    {
        bail!("server-info daemon policy_origin must be an HTTPS origin");
    }
    Ok(url.origin().ascii_serialization())
}

fn validate_https_or_debug_loopback_url(raw: &str, require_json_path: bool) -> Result<()> {
    let url = reqwest::Url::parse(raw.trim()).context("parse daemon update URL")?;
    validate_url_transport(&url)?;
    if !url.username().is_empty() || url.password().is_some() || url.host_str().is_none() {
        bail!("daemon update URL has invalid authority");
    }
    if require_json_path
        && (!url.path().ends_with("/releases/manifest.json")
            || url.query().is_some()
            || url.fragment().is_some())
    {
        bail!("daemon artifact manifest URL must end with /releases/manifest.json");
    }
    Ok(())
}

fn validate_url_transport(url: &reqwest::Url) -> Result<()> {
    if url.scheme() == "https" {
        return Ok(());
    }
    if cfg!(debug_assertions)
        && url.scheme() == "http"
        && url.host_str().is_some_and(|host| {
            host.eq_ignore_ascii_case("localhost")
                || host
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|address| address.is_loopback())
        })
    {
        return Ok(());
    }
    bail!("daemon update policy must use HTTPS");
}

fn manifest_url_from_download_base(base: &str) -> String {
    let base = base.trim();
    if base.ends_with(".json") {
        base.to_string()
    } else {
        format!("{}/releases/manifest.json", base.trim_end_matches('/'))
    }
}

fn policy_cache_path(config: &DaemonConfig, policy_origin: &str) -> PathBuf {
    let key = format!("{}|{}|{}", policy_origin, POLICY_PRODUCT, POLICY_CHANNEL);
    let digest = Sha256::digest(key.as_bytes());
    let name = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    config
        .state_root
        .join("update-policy")
        .join(format!("{name}.json"))
}

fn read_cached_policy(
    path: &Path,
    expected_origin: &str,
    policy_url: &str,
) -> Result<Option<DaemonUpdatePolicy>> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
    };
    if bytes.len() > POLICY_RESPONSE_MAX_BYTES {
        bail!("daemon update policy cache exceeds 1 MiB");
    }
    let cached: CachedDaemonUpdatePolicy =
        serde_json::from_slice(&bytes).context("parse daemon update policy cache")?;
    if cached.schema_version != POLICY_CACHE_SCHEMA_VERSION
        || cached.product != POLICY_PRODUCT
        || cached.channel != POLICY_CHANNEL
        || cached.policy_origin != expected_origin
        || cached.policy_revision == 0
    {
        bail!("daemon update policy cache scope is invalid");
    }
    OffsetDateTime::parse(cached.published_at.trim(), &Rfc3339)
        .context("daemon update policy cache published_at is invalid")?;
    validate_cached_product(&cached, expected_origin)?;
    Ok(Some(DaemonUpdatePolicy {
        enabled: cached.enabled,
        policy_origin: cached.policy_origin,
        policy_revision: cached.policy_revision,
        published_at: cached.published_at,
        recommended_version: cached.recommended_version,
        minimum_supported_version: cached.minimum_supported_version,
        upgrade_url: cached.upgrade_url,
        artifact_manifest_url: cached.artifact_manifest_url,
        policy_url: policy_url.to_string(),
        source: DaemonUpdatePolicySource::Cache,
        refresh_error: None,
    }))
}

fn validate_cached_product(cached: &CachedDaemonUpdatePolicy, expected_origin: &str) -> Result<()> {
    if !cached.enabled {
        return Ok(());
    }
    let recommended = cached
        .recommended_version
        .as_deref()
        .context("daemon update policy cache is missing recommended_version")?;
    let minimum = cached
        .minimum_supported_version
        .as_deref()
        .context("daemon update policy cache is missing minimum_supported_version")?;
    if Version::parse(minimum)? > Version::parse(recommended)? {
        bail!("daemon update policy cache minimum exceeds recommended version");
    }
    let upgrade_url = cached
        .upgrade_url
        .as_deref()
        .context("daemon update policy cache is missing upgrade_url")?;
    validate_https_or_debug_loopback_url(upgrade_url, false)?;
    let manifest_url = cached
        .artifact_manifest_url
        .as_deref()
        .context("daemon update policy cache is missing artifact_manifest_url")?;
    validate_https_or_debug_loopback_url(manifest_url, true)?;
    if tenant_origin(manifest_url)? != expected_origin {
        bail!("daemon update policy cache artifact origin is invalid");
    }
    Ok(())
}

fn write_cached_policy(path: &Path, policy: &DaemonUpdatePolicy) -> Result<()> {
    let parent = path
        .parent()
        .context("daemon update policy cache has no parent")?;
    fs::create_dir_all(parent)
        .with_context(|| format!("create daemon update policy cache {}", parent.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
            .with_context(|| format!("secure daemon update policy cache {}", parent.display()))?;
    }
    let cached = CachedDaemonUpdatePolicy {
        schema_version: POLICY_CACHE_SCHEMA_VERSION,
        product: POLICY_PRODUCT.to_string(),
        channel: POLICY_CHANNEL.to_string(),
        policy_origin: policy.policy_origin.clone(),
        policy_revision: policy.policy_revision,
        published_at: policy.published_at.clone(),
        enabled: policy.enabled,
        recommended_version: policy.recommended_version.clone(),
        minimum_supported_version: policy.minimum_supported_version.clone(),
        upgrade_url: policy.upgrade_url.clone(),
        artifact_manifest_url: policy.artifact_manifest_url.clone(),
        retrieved_at: OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .context("format daemon update policy retrieval time")?,
    };
    let bytes =
        serde_json::to_vec_pretty(&cached).context("serialize daemon update policy cache")?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = parent.join(format!(
        ".{}.{}.{nonce}.tmp",
        path.file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("policy"),
        std::process::id()
    ));
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let write_result = (|| -> Result<()> {
        let mut file = options
            .open(&temporary)
            .with_context(|| format!("create {}", temporary.display()))?;
        file.write_all(&bytes)
            .with_context(|| format!("write {}", temporary.display()))?;
        file.sync_all()
            .with_context(|| format!("sync {}", temporary.display()))?;
        atomic_replace(&temporary, path)
            .with_context(|| format!("commit daemon update policy cache {}", path.display()))?;
        if let Ok(directory) = fs::File::open(parent) {
            let _ = directory.sync_all();
        }
        Ok(())
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    write_result
}

#[cfg(not(windows))]
fn atomic_replace(temporary: &Path, target: &Path) -> std::io::Result<()> {
    fs::rename(temporary, target)
}

#[cfg(windows)]
fn atomic_replace(temporary: &Path, target: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    fn wide(path: &Path) -> std::io::Result<Vec<u16>> {
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::path::absolute(path)?
        };
        Ok(absolute
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect())
    }

    let temporary = wide(temporary)?;
    let target = wide(target)?;
    // SAFETY: both vectors are live, NUL-terminated UTF-16 paths for the call.
    let moved = unsafe {
        MoveFileExW(
            temporary.as_ptr(),
            target.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if moved == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
