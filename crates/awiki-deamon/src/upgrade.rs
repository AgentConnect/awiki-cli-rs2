use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::service::{manage_service, ServiceAction, ServicePlatform, ServiceStatus};
use crate::DaemonConfig;

pub const CURRENT_DAEMON_VERSION: &str = env!("CARGO_PKG_VERSION");
const RELEASE_HTTP_TIMEOUT: Duration = Duration::from_secs(3);
const DAEMON_VERSION_RETENTION_EXTRA: usize = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonUpgradeRequest {
    pub target_version: String,
    pub download_base_url: String,
    pub bin_root: PathBuf,
    pub restart_service: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonUpgradeReport {
    pub previous_version: Option<String>,
    pub target_version: String,
    pub min_supported_version: Option<String>,
    pub package_sha256: String,
    pub manifest_url: String,
    pub restarted: bool,
    pub service: ServiceStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonReleaseStatus {
    pub current_version: String,
    pub latest_version: Option<String>,
    pub needs_upgrade: bool,
    pub manifest_url: String,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DaemonReleaseManifest {
    latest: String,
    #[serde(default)]
    min_supported: Option<String>,
    packages: Vec<DaemonReleasePackage>,
}

#[derive(Debug, Clone, Deserialize)]
struct DaemonReleasePackage {
    version: String,
    os: String,
    arch: String,
    path: String,
    sha256: String,
}

struct CurrentLinks {
    daemon: Option<PathBuf>,
    runtime: Option<PathBuf>,
}

impl DaemonUpgradeRequest {
    pub fn from_env(config: &DaemonConfig, target_version: impl Into<String>) -> Result<Self> {
        Ok(Self {
            target_version: normalize_target_version(&target_version.into())?,
            download_base_url: std::env::var("AWIKI_DAEMON_DOWNLOAD_BASE_URL")
                .ok()
                .map(|value| value.trim().trim_end_matches('/').to_string())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| config.download_base_url.clone()),
            bin_root: std::env::var_os("AWIKI_DAEMON_BIN_ROOT")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| default_bin_root()),
            restart_service: std::env::var("AWIKI_DAEMON_UPGRADE_SKIP_SERVICE_RESTART")
                .map(|value| value.trim() != "1")
                .unwrap_or(true)
                && config.state_root == DaemonConfig::default_product_state_root()?,
        })
    }
}

pub fn check_release_status(config: &DaemonConfig) -> DaemonReleaseStatus {
    let current_version = CURRENT_DAEMON_VERSION.to_string();
    let manifest_url = manifest_url(&config.download_base_url);
    match read_release_manifest(&manifest_url) {
        Ok(manifest) => {
            let latest = manifest.latest.trim().to_string();
            let latest_version = (!latest.is_empty()).then_some(latest);
            let needs_upgrade = latest_version
                .as_deref()
                .map(|latest| version_is_newer(latest, &current_version))
                .unwrap_or(false);
            DaemonReleaseStatus {
                current_version,
                latest_version,
                needs_upgrade,
                manifest_url: diagnostic_url(&manifest_url),
                error: None,
            }
        }
        Err(error) => DaemonReleaseStatus {
            current_version,
            latest_version: None,
            needs_upgrade: false,
            manifest_url: diagnostic_url(&manifest_url),
            error: Some(sanitize_error(&error.to_string())),
        },
    }
}

pub fn upgrade_daemon(
    config: &DaemonConfig,
    request: DaemonUpgradeRequest,
) -> Result<DaemonUpgradeReport> {
    if tokio::runtime::Handle::try_current().is_ok() {
        let config = config.clone();
        let join = std::thread::Builder::new()
            .name("awiki-daemon-upgrade".to_string())
            .spawn(move || upgrade_daemon_in_new_runtime(&config, request))
            .context("spawn daemon upgrade runtime thread")?;
        return join
            .join()
            .map_err(|_| anyhow::anyhow!("daemon upgrade runtime thread panicked"))?;
    }
    upgrade_daemon_in_new_runtime(config, request)
}

fn upgrade_daemon_in_new_runtime(
    config: &DaemonConfig,
    request: DaemonUpgradeRequest,
) -> Result<DaemonUpgradeReport> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("create daemon upgrade runtime")?;
    runtime.block_on(upgrade_daemon_async(config, request))
}

async fn upgrade_daemon_async(
    config: &DaemonConfig,
    request: DaemonUpgradeRequest,
) -> Result<DaemonUpgradeReport> {
    if release_os().is_none() || release_arch().is_none() {
        bail!("current platform is not supported for awiki daemon upgrade");
    }
    cleanup_daemon_bin_root(&request.bin_root, &[CURRENT_DAEMON_VERSION.to_string()])?;
    let target_version = normalize_target_version(&request.target_version)?;
    let manifest_url = manifest_url(&request.download_base_url);
    let manifest = read_release_manifest_async(&manifest_url)
        .await
        .with_context(|| format!("download daemon manifest {}", public_url(&manifest_url)))?;
    if manifest.latest.trim().is_empty() {
        bail!("daemon release manifest latest version is empty");
    }
    let version = if target_version == "latest" {
        manifest.latest.clone()
    } else {
        target_version
    };
    let package = select_package(&manifest, &version)?;
    if !version_is_newer(&package.version, CURRENT_DAEMON_VERSION) {
        let current_dir = request.bin_root.join("current");
        let previous_version = current_daemon_link_version(&request.bin_root)
            .or_else(|| Some(CURRENT_DAEMON_VERSION.to_string()));
        let service = if request.restart_service {
            manage_service(
                config,
                &current_dir.join("awiki-deamon"),
                ServiceAction::Status,
            )
            .unwrap_or_else(|error| ServiceStatus {
                platform: ServicePlatform::Foreground,
                installed: false,
                running: false,
                unit_path: None,
                detail: Some(format!(
                    "service status unavailable during no-op upgrade: {}",
                    sanitize_error(&error.to_string())
                )),
            })
        } else {
            ServiceStatus {
                platform: ServicePlatform::Foreground,
                installed: false,
                running: false,
                unit_path: None,
                detail: Some("service restart skipped for daemon upgrade".to_string()),
            }
        };
        return Ok(DaemonUpgradeReport {
            previous_version,
            target_version: package.version,
            min_supported_version: manifest.min_supported,
            package_sha256: package.sha256,
            manifest_url: public_url(&manifest_url),
            restarted: false,
            service,
        });
    }
    let package_url = package_url(&request.download_base_url, &package.path)?;
    let archive_bytes = read_url_bytes(&package_url)
        .await
        .with_context(|| format!("download daemon package {}", public_url(&package_url)))?;
    let actual_sha = sha256_hex(&archive_bytes);
    if !actual_sha.eq_ignore_ascii_case(package.sha256.trim()) {
        bail!("daemon package sha256 mismatch");
    }

    std::fs::create_dir_all(&request.bin_root)
        .with_context(|| format!("create daemon bin root {}", request.bin_root.display()))?;
    cleanup_daemon_bin_root(
        &request.bin_root,
        &[CURRENT_DAEMON_VERSION.to_string(), version.clone()],
    )?;
    let temp_root = request.bin_root.join(format!(
        ".upgrade-{}-{}",
        sanitize_version_segment(&package.version)?,
        std::process::id()
    ));
    if temp_root.exists() {
        std::fs::remove_dir_all(&temp_root)
            .with_context(|| format!("remove stale upgrade staging {}", temp_root.display()))?;
    }
    std::fs::create_dir_all(&temp_root)
        .with_context(|| format!("create daemon upgrade staging {}", temp_root.display()))?;
    let temp_guard = UpgradeTempRootGuard::new(temp_root.clone());
    let archive_path = temp_root.join("package.tar.gz");
    std::fs::write(&archive_path, &archive_bytes)
        .with_context(|| format!("write daemon package {}", archive_path.display()))?;
    let stage_dir = temp_root.join("stage");
    std::fs::create_dir_all(&stage_dir)
        .with_context(|| format!("create daemon archive stage {}", stage_dir.display()))?;
    extract_archive(&archive_path, &stage_dir)?;
    validate_extracted_package(&stage_dir)?;

    let install_dir = request
        .bin_root
        .join(sanitize_version_segment(&package.version)?);
    if !install_dir.exists() {
        std::fs::rename(&stage_dir, &install_dir).with_context(|| {
            format!(
                "install daemon package into version directory {}",
                install_dir.display()
            )
        })?;
    } else {
        validate_extracted_package(&install_dir)?;
    }
    set_executable_mode(&install_dir.join("awiki-deamon"))?;
    let runtime_binary = install_dir.join("awiki-deamon-runtime");
    if runtime_binary.exists() {
        set_executable_mode(&runtime_binary)?;
    }
    verify_candidate_binary(&install_dir.join("awiki-deamon"), &package.version)?;

    let current_dir = request.bin_root.join("current");
    std::fs::create_dir_all(&current_dir).with_context(|| {
        format!(
            "create current daemon bin directory {}",
            current_dir.display()
        )
    })?;
    let backup = read_current_links(&current_dir)?;
    let previous_version = backup
        .daemon
        .as_ref()
        .and_then(|target| version_from_current_target(target))
        .or_else(|| Some(env!("CARGO_PKG_VERSION").to_string()));

    swap_current_links(&current_dir, &package.version, &install_dir)?;
    let service = if request.restart_service {
        match manage_service(
            config,
            &current_dir.join("awiki-deamon"),
            ServiceAction::Install,
        ) {
            Ok(status) => status,
            Err(error) => {
                let _ = restore_current_links(&current_dir, &backup);
                return Err(error).context("restart daemon service after upgrade");
            }
        }
    } else {
        ServiceStatus {
            platform: ServicePlatform::Foreground,
            installed: false,
            running: false,
            unit_path: None,
            detail: Some("service restart skipped for daemon upgrade".to_string()),
        }
    };
    temp_guard.cleanup_now();
    cleanup_daemon_bin_root(
        &request.bin_root,
        &[
            package.version.clone(),
            previous_version.clone().unwrap_or_default(),
            CURRENT_DAEMON_VERSION.to_string(),
        ],
    )?;
    Ok(DaemonUpgradeReport {
        previous_version,
        target_version: package.version,
        min_supported_version: manifest.min_supported,
        package_sha256: actual_sha,
        manifest_url: public_url(&manifest_url),
        restarted: request.restart_service,
        service,
    })
}

struct UpgradeTempRootGuard {
    path: Option<PathBuf>,
}

impl UpgradeTempRootGuard {
    fn new(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    fn cleanup_now(mut self) {
        if let Some(path) = self.path.take() {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}

impl Drop for UpgradeTempRootGuard {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}

fn cleanup_daemon_bin_root(bin_root: &Path, keep_versions: &[String]) -> Result<()> {
    let Ok(entries) = std::fs::read_dir(bin_root) else {
        return Ok(());
    };
    let mut keep = keep_versions
        .iter()
        .map(|version| version.trim().trim_start_matches('v').to_string())
        .filter(|version| !version.is_empty())
        .collect::<std::collections::BTreeSet<_>>();
    if let Some(current_version) = current_daemon_link_version(bin_root) {
        keep.insert(current_version);
    }

    let mut removable_versions = Vec::new();
    for entry in entries {
        let entry =
            entry.with_context(|| format!("read daemon bin root {}", bin_root.display()))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "current" {
            continue;
        }
        if name.starts_with(".upgrade-") || is_backup_install_dir_name(&name) {
            remove_bin_root_entry(&path)?;
            continue;
        }
        if !is_release_version_dir_name(&name) {
            continue;
        }
        if keep.contains(&name) {
            continue;
        }
        let modified_at = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        removable_versions.push((name, path, modified_at));
    }

    removable_versions.sort_by(|left, right| {
        compare_versions(&right.0, &left.0)
            .then_with(|| right.2.cmp(&left.2))
            .then_with(|| left.0.cmp(&right.0))
    });
    for (index, (_name, path, _modified_at)) in removable_versions.into_iter().enumerate() {
        if index < DAEMON_VERSION_RETENTION_EXTRA {
            continue;
        }
        remove_bin_root_entry(&path)?;
    }
    Ok(())
}

fn remove_bin_root_entry(path: &Path) -> Result<()> {
    let metadata =
        std::fs::symlink_metadata(path).with_context(|| format!("stat {}", path.display()))?;
    if metadata.file_type().is_symlink() || metadata.is_file() {
        std::fs::remove_file(path).with_context(|| format!("remove {}", path.display()))?;
    } else if metadata.is_dir() {
        std::fs::remove_dir_all(path)
            .with_context(|| format!("remove directory {}", path.display()))?;
    }
    Ok(())
}

fn is_release_version_dir_name(name: &str) -> bool {
    if name.is_empty() || name.starts_with('.') || name.contains("..") {
        return false;
    }
    let has_dot = name.contains('.');
    has_dot
        && name
            .chars()
            .all(|ch| ch.is_ascii_digit() || matches!(ch, '.' | '-' | '_'))
}

fn is_backup_install_dir_name(name: &str) -> bool {
    name.contains(".backup.")
        && name
            .split(".backup.")
            .next()
            .is_some_and(is_release_version_dir_name)
}

fn normalize_target_version(value: &str) -> Result<String> {
    let value = value.trim().trim_start_matches('v');
    if value.is_empty() {
        bail!("target_version must not be empty");
    }
    if value == "latest" {
        return Ok(value.to_string());
    }
    sanitize_version_segment(value)
}

fn sanitize_version_segment(value: &str) -> Result<String> {
    let value = value.trim().trim_start_matches('v');
    if value.is_empty() {
        bail!("daemon version must not be empty");
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'))
    {
        bail!("daemon version contains unsupported characters");
    }
    Ok(value.to_string())
}

fn version_is_newer(candidate: &str, current: &str) -> bool {
    compare_versions(candidate, current).is_gt()
}

pub(crate) fn version_is_at_least(current: &str, target: &str) -> bool {
    compare_versions(current, target).is_ge()
}

fn compare_versions(left: &str, right: &str) -> std::cmp::Ordering {
    let left = version_components(left);
    let right = version_components(right);
    let max_len = left.len().max(right.len());
    for index in 0..max_len {
        let a = left.get(index).copied().unwrap_or(0);
        let b = right.get(index).copied().unwrap_or(0);
        match a.cmp(&b) {
            std::cmp::Ordering::Equal => {}
            ordering => return ordering,
        }
    }
    std::cmp::Ordering::Equal
}

fn version_components(value: &str) -> Vec<u64> {
    value
        .trim()
        .trim_start_matches('v')
        .split(['.', '-', '_'])
        .map(|part| {
            part.chars()
                .take_while(|ch| ch.is_ascii_digit())
                .collect::<String>()
        })
        .take_while(|part| !part.is_empty())
        .filter_map(|part| part.parse::<u64>().ok())
        .collect()
}

fn default_bin_root() -> PathBuf {
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".awiki-daemon")
        .join("deamon")
        .join("bin")
}

fn manifest_url(base: &str) -> String {
    let base = base.trim();
    if base.ends_with(".json") {
        base.to_string()
    } else {
        format!("{}/releases/manifest.json", base.trim_end_matches('/'))
    }
}

fn package_url(base: &str, package_path: &str) -> Result<String> {
    let base = base.trim();
    let package_path = sanitize_manifest_path(package_path)?;
    if base.ends_with(".json") {
        let parent = base.rsplit_once('/').map(|(parent, _)| parent).unwrap_or("");
        if parent.is_empty() {
            bail!("download_base_url cannot join package path from manifest file URL");
        }
        return Ok(format!(
            "{}/{}",
            parent.trim_end_matches('/'),
            package_path.trim_start_matches('/')
        ));
    }
    Ok(format!(
        "{}/{}",
        base.trim_end_matches('/'),
        package_path.trim_start_matches('/')
    ))
}

fn sanitize_manifest_path(value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        bail!("daemon package path must not be empty");
    }
    if value.starts_with('/')
        || value.starts_with("./")
        || value.starts_with("../")
        || value.contains("/../")
        || value.ends_with("/..")
        || value.contains('\\')
    {
        bail!("daemon package path is unsafe");
    }
    Ok(value.to_string())
}

fn read_release_manifest(url: &str) -> Result<DaemonReleaseManifest> {
    if tokio::runtime::Handle::try_current().is_ok() {
        let url = url.to_string();
        let join = std::thread::Builder::new()
            .name("awiki-daemon-release-status".to_string())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .context("create daemon release status runtime")?;
                runtime.block_on(read_release_manifest_async(&url))
            })
            .context("spawn daemon release status runtime thread")?;
        return join
            .join()
            .map_err(|_| anyhow::anyhow!("daemon release status runtime thread panicked"))?;
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("create daemon release status runtime")?;
    runtime.block_on(read_release_manifest_async(url))
}

async fn read_release_manifest_async(url: &str) -> Result<DaemonReleaseManifest> {
    let manifest_bytes = read_url_bytes(url).await?;
    serde_json::from_slice(&manifest_bytes).context("parse daemon release manifest")
}

async fn read_url_bytes(url: &str) -> Result<Vec<u8>> {
    if let Some(path) = file_url_path(url) {
        return std::fs::read(&path).with_context(|| format!("read {}", path.display()));
    }
    if url.starts_with('/') || url.starts_with("./") || url.starts_with("../") {
        return std::fs::read(url).with_context(|| format!("read {url}"));
    }
    let response = reqwest::Client::builder()
        .timeout(RELEASE_HTTP_TIMEOUT)
        .build()
        .context("create HTTP client")?
        .get(url)
        .send()
        .await
        .context("send HTTP request")?
        .error_for_status()
        .context("HTTP error")?;
    Ok(response.bytes().await.context("read HTTP body")?.to_vec())
}

fn file_url_path(url: &str) -> Option<PathBuf> {
    let path = url.strip_prefix("file://")?;
    Some(PathBuf::from(path))
}

fn select_package(manifest: &DaemonReleaseManifest, version: &str) -> Result<DaemonReleasePackage> {
    let os = release_os().context("unsupported daemon release OS")?;
    let arch = release_arch().context("unsupported daemon release arch")?;
    manifest
        .packages
        .iter()
        .find(|package| package.version == version && package.os == os && package.arch == arch)
        .cloned()
        .with_context(|| format!("no daemon package for {os}-{arch} version {version}"))
}

fn release_os() -> Option<&'static str> {
    match std::env::consts::OS {
        "macos" => Some("darwin"),
        "linux" => Some("linux"),
        _ => None,
    }
}

fn release_arch() -> Option<&'static str> {
    match std::env::consts::ARCH {
        "x86_64" => Some("amd64"),
        "aarch64" => Some("arm64"),
        _ => None,
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn extract_archive(archive_path: &Path, stage_dir: &Path) -> Result<()> {
    let output = std::process::Command::new("tar")
        .arg("-C")
        .arg(stage_dir)
        .arg("-xzf")
        .arg(archive_path)
        .output()
        .context("run tar to extract daemon package")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("extract daemon package failed: {}", sanitize_error(&stderr));
    }
    Ok(())
}

fn validate_extracted_package(dir: &Path) -> Result<()> {
    let binary = dir.join("awiki-deamon");
    if !binary.is_file() {
        bail!("daemon package does not contain awiki-deamon");
    }
    Ok(())
}

fn verify_candidate_binary(binary: &Path, expected_version: &str) -> Result<()> {
    let output = std::process::Command::new(binary)
        .arg("__self-check")
        .arg("--expected-version")
        .arg(expected_version)
        .output()
        .with_context(|| {
            format!(
                "run daemon candidate self-check {}",
                binary
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("awiki-deamon")
            )
        })?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let detail = if !stderr.trim().is_empty() {
        stderr.trim()
    } else {
        stdout.trim()
    };
    bail!(
        "daemon candidate self-check failed: {}",
        sanitize_error(detail)
    )
}

fn set_executable_mode(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(path)?.permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions)?;
    }
    Ok(())
}

fn read_current_links(current_dir: &Path) -> Result<CurrentLinks> {
    Ok(CurrentLinks {
        daemon: read_optional_symlink(&current_dir.join("awiki-deamon"))?,
        runtime: read_optional_symlink(&current_dir.join("awiki-deamon-runtime"))?,
    })
}

fn read_optional_symlink(path: &Path) -> Result<Option<PathBuf>> {
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return Ok(None);
    };
    if !metadata.file_type().is_symlink() {
        bail!("current daemon binary path is not a symlink");
    }
    Ok(Some(std::fs::read_link(path)?))
}

fn swap_current_links(current_dir: &Path, version: &str, install_dir: &Path) -> Result<()> {
    let daemon_target = PathBuf::from("..").join(version).join("awiki-deamon");
    let runtime_target_name = if install_dir.join("awiki-deamon-runtime").exists() {
        "awiki-deamon-runtime"
    } else {
        "awiki-deamon"
    };
    let runtime_target = PathBuf::from("..").join(version).join(runtime_target_name);
    replace_symlink(&daemon_target, &current_dir.join("awiki-deamon"))?;
    replace_symlink(&runtime_target, &current_dir.join("awiki-deamon-runtime"))?;
    Ok(())
}

fn version_from_current_target(target: &Path) -> Option<String> {
    let parts = target
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .collect::<Vec<_>>();
    parts
        .windows(2)
        .find(|window| window[1] == "awiki-deamon")
        .and_then(|window| {
            let version = window[0].trim();
            (!version.is_empty() && version != "..").then(|| version.to_string())
        })
}

fn current_daemon_link_version(bin_root: &Path) -> Option<String> {
    let link = bin_root.join("current").join("awiki-deamon");
    let target = std::fs::read_link(link).ok()?;
    version_from_current_target(&target)
}

fn restore_current_links(current_dir: &Path, backup: &CurrentLinks) -> Result<()> {
    restore_one_link(&current_dir.join("awiki-deamon"), backup.daemon.as_ref())?;
    restore_one_link(
        &current_dir.join("awiki-deamon-runtime"),
        backup.runtime.as_ref(),
    )?;
    Ok(())
}

fn restore_one_link(path: &Path, target: Option<&PathBuf>) -> Result<()> {
    if let Some(target) = target {
        replace_symlink(target, path)
    } else {
        let _ = std::fs::remove_file(path);
        Ok(())
    }
}

#[cfg(unix)]
fn replace_symlink(target: &Path, link: &Path) -> Result<()> {
    use std::os::unix::fs::symlink;
    let tmp = link.with_extension("new");
    let _ = std::fs::remove_file(&tmp);
    symlink(target, &tmp).with_context(|| format!("create symlink {}", tmp.display()))?;
    std::fs::rename(&tmp, link).with_context(|| format!("replace symlink {}", link.display()))?;
    Ok(())
}

#[cfg(not(unix))]
fn replace_symlink(_target: &Path, _link: &Path) -> Result<()> {
    bail!("daemon upgrade symlink swap requires Unix")
}

fn public_url(url: &str) -> String {
    let value = url.trim();
    if value.chars().count() > 512 {
        value.chars().take(512).collect::<String>() + "..."
    } else {
        value.to_string()
    }
}

fn diagnostic_url(url: &str) -> String {
    let value = url.trim();
    if value.starts_with("http://") || value.starts_with("https://") {
        public_url(value)
    } else {
        "<local-release-manifest>".to_string()
    }
}

fn sanitize_error(value: &str) -> String {
    let mut redacted = value
        .split_whitespace()
        .map(|part| {
            let lower = part.to_ascii_lowercase();
            if lower.contains("token")
                || lower.contains("secret")
                || lower.contains("jwt")
                || lower.contains("key")
            {
                "<redacted>"
            } else if part.starts_with('/') || part.starts_with("file://") {
                "<path>"
            } else {
                part
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    if redacted.chars().count() > 240 {
        redacted = redacted.chars().take(240).collect();
    }
    redacted
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, DaemonConfig) {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path().join("state")).unwrap();
        config.ensure_state_layout().unwrap();
        (root, config)
    }

    fn create_package(root: &Path, version: &str) -> (PathBuf, String) {
        let stage = root.join(format!("stage-{version}"));
        std::fs::create_dir_all(&stage).unwrap();
        std::fs::write(
            stage.join("awiki-deamon"),
            format!(
                "#!/bin/sh\nif [ \"${{1-}}\" = \"__self-check\" ]; then exit 0; fi\nprintf '%s\\n' 'daemon {version}'\n"
            ),
        )
        .unwrap();
        set_executable_mode(&stage.join("awiki-deamon")).unwrap();
        std::fs::write(
            stage.join("awiki-deamon-runtime"),
            format!("runtime {version}"),
        )
        .unwrap();
        let archive = root.join(format!("awiki-deamon-{version}.tar.gz"));
        let output = std::process::Command::new("tar")
            .arg("-C")
            .arg(&stage)
            .arg("-czf")
            .arg(&archive)
            .arg("awiki-deamon")
            .arg("awiki-deamon-runtime")
            .output()
            .unwrap();
        assert!(output.status.success());
        let bytes = std::fs::read(&archive).unwrap();
        let sha = sha256_hex(&bytes);
        (archive, sha)
    }

    fn write_manifest(root: &Path, version: &str, archive: &Path, sha: &str) -> PathBuf {
        let releases = root.join("releases");
        std::fs::create_dir_all(&releases).unwrap();
        let manifest = releases.join("manifest.json");
        let package_path = archive
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        std::fs::write(
            &manifest,
            serde_json::to_vec_pretty(&serde_json::json!({
                "latest": version,
                "min_supported": "0.1.0",
                "packages": [{
                    "version": version,
                    "os": release_os().unwrap(),
                    "arch": release_arch().unwrap(),
                    "path": package_path,
                    "sha256": sha,
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        manifest
    }

    fn write_status_manifest(root: &Path, latest: &str) -> PathBuf {
        let releases = root.join("releases");
        std::fs::create_dir_all(&releases).unwrap();
        let manifest = releases.join("manifest.json");
        std::fs::write(
            &manifest,
            serde_json::to_vec_pretty(&serde_json::json!({
                "latest": latest,
                "packages": []
            }))
            .unwrap(),
        )
        .unwrap();
        manifest
    }

    #[test]
    fn version_comparison_uses_numeric_segments() {
        assert!(version_is_newer("0.10.0", "0.2.0"));
        assert!(version_is_newer("v1.2.1", "1.2.0"));
        assert!(!version_is_newer("1.2.0", "1.2.0"));
        assert!(!version_is_newer("1.1.9", "1.2.0"));
    }

    #[test]
    fn release_status_is_latest_version_driven() {
        let (root, mut config) = fixture();
        write_status_manifest(root.path(), "0.10.0");
        config.download_base_url = format!("file://{}", root.path().display());

        let status = check_release_status(&config);

        assert_eq!(status.current_version, CURRENT_DAEMON_VERSION);
        assert_eq!(status.latest_version.as_deref(), Some("0.10.0"));
        assert!(status.needs_upgrade);
        assert!(status.error.is_none());
        assert_eq!(status.manifest_url, "<local-release-manifest>");
    }

    #[test]
    fn release_status_is_unavailable_without_forcing_upgrade() {
        let (_root, mut config) = fixture();
        config.download_base_url = "file:///missing-awiki-daemon-download-root".to_string();

        let status = check_release_status(&config);

        assert_eq!(status.current_version, CURRENT_DAEMON_VERSION);
        assert!(status.latest_version.is_none());
        assert!(!status.needs_upgrade);
        assert!(status.error.is_some());
    }

    #[cfg(unix)]
    #[test]
    fn upgrade_is_noop_when_latest_is_not_newer_than_running_version() {
        let (root, config) = fixture();
        let bin_root = root.path().join("bin");
        let old_dir = bin_root.join(CURRENT_DAEMON_VERSION);
        let current_dir = bin_root.join("current");
        std::fs::create_dir_all(&old_dir).unwrap();
        std::fs::create_dir_all(&current_dir).unwrap();
        std::fs::write(old_dir.join("awiki-deamon"), "current").unwrap();
        std::os::unix::fs::symlink(
            format!("../{CURRENT_DAEMON_VERSION}/awiki-deamon"),
            current_dir.join("awiki-deamon"),
        )
        .unwrap();
        let (archive, sha) = create_package(root.path(), CURRENT_DAEMON_VERSION);
        write_manifest(root.path(), CURRENT_DAEMON_VERSION, &archive, &sha);

        let report = upgrade_daemon(
            &config,
            DaemonUpgradeRequest {
                target_version: "latest".to_string(),
                download_base_url: format!("file://{}", root.path().display()),
                bin_root: bin_root.clone(),
                restart_service: false,
            },
        )
        .unwrap();

        assert_eq!(report.target_version, CURRENT_DAEMON_VERSION);
        assert!(!report.restarted);
        assert_eq!(
            std::fs::read_link(current_dir.join("awiki-deamon")).unwrap(),
            PathBuf::from(format!("../{CURRENT_DAEMON_VERSION}/awiki-deamon"))
        );
        assert!(!bin_root.read_dir().unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".upgrade-")));
    }

    #[cfg(unix)]
    #[test]
    fn upgrade_downloads_verifies_extracts_and_swaps_current_symlinks() {
        let (root, config) = fixture();
        let bin_root = root.path().join("bin");
        let old_dir = bin_root.join("0.1.0");
        let older_dir = bin_root.join("0.0.9");
        let oldest_dir = bin_root.join("0.0.7");
        let old_backup_dir = bin_root.join("0.0.8.backup.20260101000000");
        let stale_upgrade_dir = bin_root.join(".upgrade-0.0.7-123");
        let current_dir = bin_root.join("current");
        std::fs::create_dir_all(&old_dir).unwrap();
        std::fs::create_dir_all(&older_dir).unwrap();
        std::fs::create_dir_all(&oldest_dir).unwrap();
        std::fs::create_dir_all(&old_backup_dir).unwrap();
        std::fs::create_dir_all(&stale_upgrade_dir).unwrap();
        std::fs::create_dir_all(&current_dir).unwrap();
        std::fs::write(old_dir.join("awiki-deamon"), "old").unwrap();
        std::fs::write(older_dir.join("awiki-deamon"), "older").unwrap();
        std::fs::write(oldest_dir.join("awiki-deamon"), "oldest").unwrap();
        std::fs::write(old_backup_dir.join("awiki-deamon"), "backup").unwrap();
        std::fs::write(stale_upgrade_dir.join("package.tar.gz"), "stale").unwrap();
        std::os::unix::fs::symlink("../0.1.0/awiki-deamon", current_dir.join("awiki-deamon"))
            .unwrap();
        std::os::unix::fs::symlink(
            "../0.1.0/awiki-deamon",
            current_dir.join("awiki-deamon-runtime"),
        )
        .unwrap();
        let (archive, sha) = create_package(root.path(), "0.2.0");
        write_manifest(root.path(), "0.2.0", &archive, &sha);

        let report = upgrade_daemon(
            &config,
            DaemonUpgradeRequest {
                target_version: "latest".to_string(),
                download_base_url: format!("file://{}", root.path().display()),
                bin_root: bin_root.clone(),
                restart_service: false,
            },
        )
        .unwrap();

        assert_eq!(report.target_version, "0.2.0");
        assert_eq!(report.min_supported_version.as_deref(), Some("0.1.0"));
        assert_eq!(report.package_sha256, sha);
        assert_eq!(
            std::fs::read_link(current_dir.join("awiki-deamon")).unwrap(),
            PathBuf::from("../0.2.0/awiki-deamon")
        );
        assert_eq!(
            std::fs::read_link(current_dir.join("awiki-deamon-runtime")).unwrap(),
            PathBuf::from("../0.2.0/awiki-deamon-runtime")
        );
        let self_check = std::process::Command::new(bin_root.join("0.2.0").join("awiki-deamon"))
            .arg("__self-check")
            .arg("--expected-version")
            .arg("0.2.0")
            .output()
            .unwrap();
        assert!(self_check.status.success());
        assert!(bin_root.join("0.2.0").exists());
        assert!(bin_root.join("0.1.0").exists());
        assert!(bin_root.join("0.0.9").exists());
        assert!(!bin_root.join("0.0.7").exists());
        assert!(!bin_root.join("0.0.8.backup.20260101000000").exists());
        assert!(!bin_root.join(".upgrade-0.0.7-123").exists());
        assert!(!bin_root.read_dir().unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".upgrade-")));
    }

    #[cfg(unix)]
    #[test]
    fn upgrade_preserves_current_symlink_when_sha256_mismatches() {
        let (root, config) = fixture();
        let bin_root = root.path().join("bin");
        let old_dir = bin_root.join("0.1.0");
        let current_dir = bin_root.join("current");
        std::fs::create_dir_all(&old_dir).unwrap();
        std::fs::create_dir_all(&current_dir).unwrap();
        std::fs::write(old_dir.join("awiki-deamon"), "old").unwrap();
        std::os::unix::fs::symlink("../0.1.0/awiki-deamon", current_dir.join("awiki-deamon"))
            .unwrap();
        let (archive, _sha) = create_package(root.path(), "0.2.0");
        write_manifest(root.path(), "0.2.0", &archive, "deadbeef");

        let error = upgrade_daemon(
            &config,
            DaemonUpgradeRequest {
                target_version: "latest".to_string(),
                download_base_url: format!("file://{}", root.path().display()),
                bin_root: bin_root.clone(),
                restart_service: false,
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("sha256"));
        assert_eq!(
            std::fs::read_link(current_dir.join("awiki-deamon")).unwrap(),
            PathBuf::from("../0.1.0/awiki-deamon")
        );
        assert!(!bin_root.read_dir().unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".upgrade-")));
    }

    #[cfg(unix)]
    #[test]
    fn upgrade_preserves_current_symlink_when_candidate_self_check_fails() {
        let (root, config) = fixture();
        let bin_root = root.path().join("bin");
        let old_dir = bin_root.join("0.1.0");
        let current_dir = bin_root.join("current");
        std::fs::create_dir_all(&old_dir).unwrap();
        std::fs::create_dir_all(&current_dir).unwrap();
        std::fs::write(old_dir.join("awiki-deamon"), "old").unwrap();
        std::os::unix::fs::symlink("../0.1.0/awiki-deamon", current_dir.join("awiki-deamon"))
            .unwrap();

        let bad_stage = root.path().join("bad-stage");
        std::fs::create_dir_all(&bad_stage).unwrap();
        std::fs::write(bad_stage.join("awiki-deamon"), "#!/bin/sh\nexit 42\n").unwrap();
        set_executable_mode(&bad_stage.join("awiki-deamon")).unwrap();
        let bad_archive = root.path().join("bad-package.tar.gz");
        let output = std::process::Command::new("tar")
            .arg("-C")
            .arg(&bad_stage)
            .arg("-czf")
            .arg(&bad_archive)
            .arg("awiki-deamon")
            .output()
            .unwrap();
        assert!(output.status.success());
        let sha = sha256_hex(&std::fs::read(&bad_archive).unwrap());
        write_manifest(root.path(), "0.2.0", &bad_archive, &sha);

        let error = upgrade_daemon(
            &config,
            DaemonUpgradeRequest {
                target_version: "latest".to_string(),
                download_base_url: format!("file://{}", root.path().display()),
                bin_root: bin_root.clone(),
                restart_service: false,
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("self-check"));
        assert_eq!(
            std::fs::read_link(current_dir.join("awiki-deamon")).unwrap(),
            PathBuf::from("../0.1.0/awiki-deamon")
        );
    }
}
