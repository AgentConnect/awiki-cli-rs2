use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::service::{manage_service, ServiceAction, ServicePlatform, ServiceStatus};
use crate::DaemonConfig;

pub const CURRENT_DAEMON_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const DAEMON_UPGRADE_CANCELLED_ERROR: &str = "daemon upgrade cancelled";
const RELEASE_HTTP_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const RELEASE_MANIFEST_HTTP_TIMEOUT: Duration = Duration::from_secs(5);
const RELEASE_PACKAGE_READ_TIMEOUT: Duration = Duration::from_secs(90);
const RELEASE_PACKAGE_PROBE_TIMEOUT: Duration = Duration::from_secs(8);
const RELEASE_PACKAGE_PROBE_BYTES: u64 = 256 * 1024;
const RELEASE_HTTP_MAX_ATTEMPTS: usize = 3;
const RELEASE_HTTP_RETRY_BASE_DELAY: Duration = Duration::from_millis(500);
const RELEASE_PROGRESS_MIN_INTERVAL: Duration = Duration::from_millis(800);
const DAEMON_VERSION_RETENTION_EXTRA: usize = 1;
const COMMON_LOCAL_HTTP_PROXY_PORTS: &[u16] = &[7897, 7890, 7891, 7892, 1080, 8080];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonUpgradeRequest {
    pub target_version: String,
    pub download_base_url: String,
    pub bin_root: PathBuf,
    pub restart_service: bool,
}

#[derive(Debug, Clone, Default)]
pub struct DaemonUpgradeCancelToken {
    cancelled: Arc<AtomicBool>,
}

impl DaemonUpgradeCancelToken {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    fn check(&self) -> Result<()> {
        if self.is_cancelled() {
            bail!(DAEMON_UPGRADE_CANCELLED_ERROR);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonUpgradeReport {
    pub previous_version: Option<String>,
    pub target_version: String,
    pub min_supported_version: Option<String>,
    pub package_sha256: String,
    pub manifest_url: String,
    pub download_base_url: String,
    pub download_route: Option<String>,
    pub restarted: bool,
    pub service: ServiceStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DaemonUpgradeProgress {
    pub stage: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub route: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub downloaded_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed_bytes_per_sec: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonReleaseStatus {
    pub current_version: String,
    pub latest_version: Option<String>,
    pub needs_upgrade: bool,
    pub manifest_url: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct DaemonReleaseManifest {
    latest: String,
    #[serde(default)]
    min_supported: Option<String>,
    #[serde(default)]
    download_base_urls: Vec<String>,
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

#[derive(Debug, Clone)]
struct ReleaseManifestSelection {
    manifest: DaemonReleaseManifest,
    manifest_url: String,
    download_base_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ReleaseHttpRoute {
    label: String,
    proxy_url: Option<String>,
}

#[derive(Debug, Clone)]
struct ReleaseDownloadCandidate {
    base_url: String,
    package_url: String,
    route: Option<ReleaseHttpRoute>,
    score_bytes_per_sec: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ReleaseHttpTimeoutPolicy {
    total_timeout: Option<Duration>,
    read_timeout: Option<Duration>,
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
    let request_sources = DaemonUpgradeRequest {
        target_version: "latest".to_string(),
        download_base_url: config.download_base_url.clone(),
        bin_root: default_bin_root(),
        restart_service: false,
    };
    let sources = configured_download_base_urls(config, &request_sources);
    match read_release_manifest_status_from_sources(&sources) {
        Ok(selection) => {
            let manifest = selection.manifest;
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
                manifest_url: diagnostic_url(&selection.manifest_url),
                error: None,
            }
        }
        Err(error) => DaemonReleaseStatus {
            current_version,
            latest_version: None,
            needs_upgrade: false,
            manifest_url: sources
                .first()
                .map(|source| diagnostic_url(&manifest_url(source)))
                .unwrap_or_default(),
            error: Some(sanitize_error(&error.to_string())),
        },
    }
}

pub fn upgrade_daemon(
    config: &DaemonConfig,
    request: DaemonUpgradeRequest,
) -> Result<DaemonUpgradeReport> {
    upgrade_daemon_with_progress(config, request, |_| {})
}

pub fn upgrade_daemon_with_progress<F>(
    config: &DaemonConfig,
    request: DaemonUpgradeRequest,
    progress: F,
) -> Result<DaemonUpgradeReport>
where
    F: FnMut(DaemonUpgradeProgress),
{
    upgrade_daemon_with_progress_and_cancel(
        config,
        request,
        DaemonUpgradeCancelToken::default(),
        progress,
    )
}

pub fn upgrade_daemon_with_progress_and_cancel<F>(
    config: &DaemonConfig,
    request: DaemonUpgradeRequest,
    cancel_token: DaemonUpgradeCancelToken,
    mut progress: F,
) -> Result<DaemonUpgradeReport>
where
    F: FnMut(DaemonUpgradeProgress),
{
    if tokio::runtime::Handle::try_current().is_ok() {
        enum UpgradeThreadEvent {
            Progress(DaemonUpgradeProgress),
            Done(Result<DaemonUpgradeReport>),
        }
        let config = config.clone();
        let cancel_token = cancel_token.clone();
        let (tx, rx) = mpsc::channel::<UpgradeThreadEvent>();
        let join = std::thread::Builder::new()
            .name("awiki-daemon-upgrade".to_string())
            .spawn(move || {
                let result =
                    upgrade_daemon_in_new_runtime(&config, request, cancel_token, |event| {
                        let _ = tx.send(UpgradeThreadEvent::Progress(event));
                    });
                let _ = tx.send(UpgradeThreadEvent::Done(result));
            })
            .context("spawn daemon upgrade runtime thread")?;
        let mut result = None;
        while let Ok(event) = rx.recv() {
            match event {
                UpgradeThreadEvent::Progress(event) => progress(event),
                UpgradeThreadEvent::Done(done) => {
                    result = Some(done);
                    break;
                }
            }
        }
        join.join()
            .map_err(|_| anyhow::anyhow!("daemon upgrade runtime thread panicked"))?;
        return result
            .unwrap_or_else(|| bail!("daemon upgrade runtime thread ended without result"));
    }
    upgrade_daemon_in_new_runtime(config, request, cancel_token, progress)
}

fn upgrade_daemon_in_new_runtime<F>(
    config: &DaemonConfig,
    request: DaemonUpgradeRequest,
    cancel_token: DaemonUpgradeCancelToken,
    progress: F,
) -> Result<DaemonUpgradeReport>
where
    F: FnMut(DaemonUpgradeProgress),
{
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("create daemon upgrade runtime")?;
    runtime.block_on(upgrade_daemon_async(
        config,
        request,
        cancel_token,
        progress,
    ))
}

async fn upgrade_daemon_async<F>(
    config: &DaemonConfig,
    request: DaemonUpgradeRequest,
    cancel_token: DaemonUpgradeCancelToken,
    mut progress: F,
) -> Result<DaemonUpgradeReport>
where
    F: FnMut(DaemonUpgradeProgress),
{
    cancel_token.check()?;
    if release_os().is_none() || release_arch().is_none() {
        bail!("current platform is not supported for awiki daemon upgrade");
    }
    cleanup_daemon_bin_root(&request.bin_root, &[CURRENT_DAEMON_VERSION.to_string()])?;
    let target_version = normalize_target_version(&request.target_version)?;
    emit_upgrade_progress(
        &mut progress,
        "manifest",
        "正在获取版本信息",
        Some(target_version.clone()),
        None,
    );
    let initial_sources = configured_download_base_urls(config, &request);
    let manifest_selection = read_release_manifest_from_sources(&initial_sources, &mut progress)
        .await
        .context("download daemon manifest")?;
    cancel_token.check()?;
    let manifest = manifest_selection.manifest;
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
            manifest_url: public_url(&manifest_selection.manifest_url),
            download_base_url: public_url(&manifest_selection.download_base_url),
            download_route: None,
            restarted: false,
            service,
        });
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
    let download_sources = package_download_base_urls(
        &initial_sources,
        &manifest.download_base_urls,
        &manifest_selection.download_base_url,
    );
    emit_upgrade_progress(
        &mut progress,
        "selecting_source",
        "正在选择下载线路",
        Some(package.version.clone()),
        None,
    );
    let candidates =
        rank_package_download_candidates(&download_sources, &package.path, &mut progress).await?;
    cancel_token.check()?;
    let selected_download = download_package_with_candidates(
        &candidates,
        &archive_path,
        &package.sha256,
        &package.version,
        &cancel_token,
        &mut progress,
    )
    .await
    .with_context(|| {
        format!(
            "download daemon package {}",
            public_url(
                &candidates
                    .first()
                    .map(|candidate| candidate.package_url.clone())
                    .unwrap_or_else(|| package.path.clone())
            )
        )
    })?;
    let actual_sha = selected_download.0;
    emit_upgrade_progress(
        &mut progress,
        "verifying",
        "正在校验安装包",
        Some(package.version.clone()),
        Some((&selected_download.1, selected_download.2.as_deref())),
    );
    cancel_token.check()?;
    let stage_dir = temp_root.join("stage");
    std::fs::create_dir_all(&stage_dir)
        .with_context(|| format!("create daemon archive stage {}", stage_dir.display()))?;
    emit_upgrade_progress(
        &mut progress,
        "extracting",
        "正在解压安装包",
        Some(package.version.clone()),
        Some((&selected_download.1, selected_download.2.as_deref())),
    );
    cancel_token.check()?;
    extract_archive(&archive_path, &stage_dir)?;
    validate_extracted_package(&stage_dir)?;
    cancel_token.check()?;

    emit_upgrade_progress(
        &mut progress,
        "installing",
        "正在安装新版本",
        Some(package.version.clone()),
        Some((&selected_download.1, selected_download.2.as_deref())),
    );
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
        emit_upgrade_progress(
            &mut progress,
            "restarting",
            "正在重启代理服务",
            Some(package.version.clone()),
            Some((&selected_download.1, selected_download.2.as_deref())),
        );
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
        manifest_url: public_url(&manifest_selection.manifest_url),
        download_base_url: public_url(&selected_download.1),
        download_route: selected_download.2,
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

fn configured_download_base_urls(
    config: &DaemonConfig,
    request: &DaemonUpgradeRequest,
) -> Vec<String> {
    let mut sources = Vec::new();
    push_download_base_values(&mut sources, &request.download_base_url);
    if let Ok(value) = std::env::var("AWIKI_DAEMON_DOWNLOAD_BASE_URLS") {
        push_download_base_values(&mut sources, &value);
    }
    if let Ok(value) = std::env::var("AWIKI_DAEMON_DOWNLOAD_BASE_URL") {
        push_download_base_values(&mut sources, &value);
    }
    push_download_base_values(&mut sources, &config.download_base_url);
    dedupe_download_sources(sources)
}

fn package_download_base_urls(
    configured: &[String],
    manifest_sources: &[String],
    manifest_source: &str,
) -> Vec<String> {
    let mut sources = Vec::new();
    push_download_base_values(&mut sources, manifest_source);
    for value in manifest_sources {
        push_download_base_values(&mut sources, value);
    }
    for value in configured {
        push_download_base_values(&mut sources, value);
    }
    dedupe_download_sources(sources)
}

fn push_download_base_values(values: &mut Vec<String>, raw: &str) {
    for part in raw.replace(',', "\n").lines() {
        let value = normalize_download_base_url(part);
        if !value.is_empty() {
            values.push(value);
        }
    }
}

fn normalize_download_base_url(value: &str) -> String {
    value.trim().trim_end_matches('/').to_string()
}

fn dedupe_download_sources(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    values
        .into_iter()
        .filter(|value| !value.is_empty())
        .filter(|value| seen.insert(value.clone()))
        .collect()
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
        let parent = base
            .rsplit_once('/')
            .map(|(parent, _)| parent)
            .unwrap_or("");
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

async fn read_release_manifest_from_sources<F>(
    sources: &[String],
    progress: &mut F,
) -> Result<ReleaseManifestSelection>
where
    F: FnMut(DaemonUpgradeProgress),
{
    if sources.is_empty() {
        bail!("daemon download base URL is not configured");
    }
    let routes = release_http_routes();
    let mut errors = Vec::new();
    for (index, source) in sources.iter().enumerate() {
        let url = manifest_url(source);
        emit_upgrade_progress_detailed(
            progress,
            DaemonUpgradeProgress {
                stage: "manifest".to_string(),
                message: "正在获取版本信息".to_string(),
                target_version: None,
                source_url: Some(diagnostic_url(&url)),
                route: None,
                attempt: None,
                source_index: Some(index + 1),
                source_count: Some(sources.len()),
                downloaded_bytes: None,
                total_bytes: None,
                percent: None,
                speed_bytes_per_sec: None,
            },
        );
        if is_local_release_url(&url) {
            match read_release_manifest_async(&url).await {
                Ok(manifest) => {
                    return Ok(ReleaseManifestSelection {
                        manifest,
                        manifest_url: url,
                        download_base_url: source.clone(),
                    });
                }
                Err(error) => errors.push(format!(
                    "{}: {}",
                    diagnostic_url(&url),
                    sanitize_public_error_chain(error.chain())
                )),
            }
            continue;
        }
        for route in &routes {
            let client =
                match release_http_client(Some(route), RELEASE_MANIFEST_HTTP_TIMEOUT, false) {
                    Ok(client) => client,
                    Err(error) => {
                        errors.push(format!(
                            "{}: {}",
                            route.label,
                            sanitize_error(&error.to_string())
                        ));
                        continue;
                    }
                };
            match read_http_url_bytes_with_retries(&client, &url).await {
                Ok(bytes) => {
                    let manifest =
                        serde_json::from_slice(&bytes).context("parse daemon release manifest")?;
                    return Ok(ReleaseManifestSelection {
                        manifest,
                        manifest_url: url,
                        download_base_url: source.clone(),
                    });
                }
                Err(error) => errors.push(format!(
                    "{} via {}: {}",
                    diagnostic_url(&url),
                    route.label,
                    sanitize_public_error_chain(error.chain())
                )),
            }
        }
    }
    bail!(
        "daemon release manifest unavailable from all sources: {}",
        errors.join("; ")
    )
}

async fn rank_package_download_candidates<F>(
    sources: &[String],
    package_path: &str,
    progress: &mut F,
) -> Result<Vec<ReleaseDownloadCandidate>>
where
    F: FnMut(DaemonUpgradeProgress),
{
    if sources.is_empty() {
        bail!("daemon package download source is not configured");
    }
    let routes = release_http_routes();
    let mut candidates = Vec::new();
    for (index, source) in sources.iter().enumerate() {
        let package_url = package_url(source, package_path)?;
        if is_local_release_url(&package_url) {
            candidates.push(ReleaseDownloadCandidate {
                base_url: source.clone(),
                package_url,
                route: None,
                score_bytes_per_sec: Some(u64::MAX),
            });
            continue;
        }
        for route in &routes {
            emit_upgrade_progress_detailed(
                progress,
                DaemonUpgradeProgress {
                    stage: "selecting_source".to_string(),
                    message: "正在测试下载线路".to_string(),
                    target_version: None,
                    source_url: Some(diagnostic_url(source)),
                    route: Some(route.label.clone()),
                    attempt: None,
                    source_index: Some(index + 1),
                    source_count: Some(sources.len()),
                    downloaded_bytes: None,
                    total_bytes: None,
                    percent: None,
                    speed_bytes_per_sec: None,
                },
            );
            let score = probe_package_download(&package_url, route).await.ok();
            candidates.push(ReleaseDownloadCandidate {
                base_url: source.clone(),
                package_url: package_url.clone(),
                route: Some(route.clone()),
                score_bytes_per_sec: score,
            });
        }
    }
    candidates.sort_by(|left, right| {
        right
            .score_bytes_per_sec
            .unwrap_or(0)
            .cmp(&left.score_bytes_per_sec.unwrap_or(0))
    });
    Ok(candidates)
}

async fn probe_package_download(url: &str, route: &ReleaseHttpRoute) -> Result<u64> {
    let client = release_http_client(Some(route), RELEASE_PACKAGE_PROBE_TIMEOUT, true)?;
    let started_at = Instant::now();
    let response = client
        .get(url)
        .header(
            reqwest::header::RANGE,
            format!("bytes=0-{}", RELEASE_PACKAGE_PROBE_BYTES - 1),
        )
        .send()
        .await
        .context("send package probe request")?;
    let response = response.error_for_status().context("HTTP error")?;
    let bytes = response.bytes().await.context("read package probe body")?;
    if bytes.is_empty() {
        bail!("package probe returned empty body");
    }
    let elapsed = started_at.elapsed().as_secs_f64().max(0.001);
    Ok((bytes.len() as f64 / elapsed) as u64)
}

async fn download_package_with_candidates<F>(
    candidates: &[ReleaseDownloadCandidate],
    archive_path: &Path,
    expected_sha256: &str,
    target_version: &str,
    cancel_token: &DaemonUpgradeCancelToken,
    progress: &mut F,
) -> Result<(String, String, Option<String>)>
where
    F: FnMut(DaemonUpgradeProgress),
{
    if candidates.is_empty() {
        bail!("daemon package download candidate list is empty");
    }
    let mut errors = Vec::new();
    for (index, candidate) in candidates.iter().enumerate() {
        cancel_token.check()?;
        let route_label = candidate.route.as_ref().map(|route| route.label.as_str());
        for attempt in 1..=RELEASE_HTTP_MAX_ATTEMPTS {
            cancel_token.check()?;
            emit_upgrade_progress_detailed(
                progress,
                DaemonUpgradeProgress {
                    stage: "downloading".to_string(),
                    message: "正在下载安装包".to_string(),
                    target_version: Some(target_version.to_string()),
                    source_url: Some(diagnostic_url(&candidate.base_url)),
                    route: route_label.map(str::to_string),
                    attempt: Some(attempt),
                    source_index: Some(index + 1),
                    source_count: Some(candidates.len()),
                    downloaded_bytes: Some(0),
                    total_bytes: None,
                    percent: Some(0.0),
                    speed_bytes_per_sec: candidate.score_bytes_per_sec,
                },
            );
            let result = download_package_once(
                candidate,
                archive_path,
                expected_sha256,
                target_version,
                attempt,
                cancel_token,
                progress,
            )
            .await;
            match result {
                Ok(actual_sha) => {
                    return Ok((
                        actual_sha,
                        candidate.base_url.clone(),
                        route_label.map(str::to_string),
                    ));
                }
                Err(error) => {
                    let summary = sanitize_public_error_chain(error.chain());
                    errors.push(format!(
                        "{} via {} attempt {}: {}",
                        diagnostic_url(&candidate.package_url),
                        route_label.unwrap_or("local"),
                        attempt,
                        summary
                    ));
                    let _ = std::fs::remove_file(archive_path);
                    if summary == DAEMON_UPGRADE_CANCELLED_ERROR {
                        return Err(error);
                    }
                    if attempt < RELEASE_HTTP_MAX_ATTEMPTS
                        && release_http_error_is_retryable(&error)
                    {
                        emit_upgrade_progress_detailed(
                            progress,
                            DaemonUpgradeProgress {
                                stage: "retrying_source".to_string(),
                                message: "下载中断，正在重试".to_string(),
                                target_version: Some(target_version.to_string()),
                                source_url: Some(diagnostic_url(&candidate.base_url)),
                                route: route_label.map(str::to_string),
                                attempt: Some(attempt + 1),
                                source_index: Some(index + 1),
                                source_count: Some(candidates.len()),
                                downloaded_bytes: None,
                                total_bytes: None,
                                percent: None,
                                speed_bytes_per_sec: None,
                            },
                        );
                        tokio::time::sleep(RELEASE_HTTP_RETRY_BASE_DELAY * attempt as u32).await;
                        continue;
                    }
                    break;
                }
            }
        }
    }
    bail!(
        "daemon package unavailable from all download sources: {}",
        errors.join("; ")
    )
}

async fn download_package_once<F>(
    candidate: &ReleaseDownloadCandidate,
    archive_path: &Path,
    expected_sha256: &str,
    target_version: &str,
    attempt: usize,
    cancel_token: &DaemonUpgradeCancelToken,
    progress: &mut F,
) -> Result<String>
where
    F: FnMut(DaemonUpgradeProgress),
{
    cancel_token.check()?;
    if let Some(path) = file_url_path(&candidate.package_url) {
        return copy_local_package(
            &path,
            archive_path,
            expected_sha256,
            target_version,
            &candidate.base_url,
            candidate.route.as_ref().map(|route| route.label.as_str()),
            attempt,
            cancel_token,
            progress,
        )
        .await;
    }
    if candidate.package_url.starts_with('/')
        || candidate.package_url.starts_with("./")
        || candidate.package_url.starts_with("../")
    {
        return copy_local_package(
            Path::new(&candidate.package_url),
            archive_path,
            expected_sha256,
            target_version,
            &candidate.base_url,
            candidate.route.as_ref().map(|route| route.label.as_str()),
            attempt,
            cancel_token,
            progress,
        )
        .await;
    }
    let client = release_package_http_client(candidate.route.as_ref())?;
    let route_label = candidate.route.as_ref().map(|route| route.label.as_str());
    let response = client
        .get(&candidate.package_url)
        .send()
        .await
        .context("send HTTP request")?;
    let response = response.error_for_status().context("HTTP error")?;
    let total = response.content_length();
    let mut stream = response.bytes_stream();
    let mut file = tokio::fs::File::create(archive_path)
        .await
        .with_context(|| format!("create daemon package {}", archive_path.display()))?;
    let mut hasher = Sha256::new();
    let mut downloaded = 0u64;
    let started_at = Instant::now();
    let mut last_progress_at = Instant::now() - RELEASE_PROGRESS_MIN_INTERVAL;
    use futures_util::StreamExt;
    while let Some(chunk) = stream.next().await {
        cancel_token.check()?;
        let chunk = chunk.context("read HTTP body")?;
        file.write_all(&chunk)
            .await
            .with_context(|| format!("write daemon package {}", archive_path.display()))?;
        hasher.update(&chunk);
        downloaded += chunk.len() as u64;
        if last_progress_at.elapsed() >= RELEASE_PROGRESS_MIN_INTERVAL
            || total.is_some_and(|value| downloaded >= value)
        {
            last_progress_at = Instant::now();
            emit_download_progress(
                progress,
                target_version,
                &candidate.base_url,
                route_label,
                attempt,
                downloaded,
                total,
                started_at,
            );
        }
    }
    file.flush()
        .await
        .with_context(|| format!("flush daemon package {}", archive_path.display()))?;
    let actual_sha = sha256_digest_to_hex(hasher.finalize());
    if !actual_sha.eq_ignore_ascii_case(expected_sha256.trim()) {
        bail!("daemon package sha256 mismatch");
    }
    Ok(actual_sha)
}

async fn copy_local_package<F>(
    source_path: &Path,
    archive_path: &Path,
    expected_sha256: &str,
    target_version: &str,
    source_url: &str,
    route: Option<&str>,
    attempt: usize,
    cancel_token: &DaemonUpgradeCancelToken,
    progress: &mut F,
) -> Result<String>
where
    F: FnMut(DaemonUpgradeProgress),
{
    cancel_token.check()?;
    let bytes =
        std::fs::read(source_path).with_context(|| format!("read {}", source_path.display()))?;
    cancel_token.check()?;
    std::fs::write(archive_path, &bytes)
        .with_context(|| format!("write daemon package {}", archive_path.display()))?;
    let actual_sha = sha256_hex(&bytes);
    if !actual_sha.eq_ignore_ascii_case(expected_sha256.trim()) {
        bail!("daemon package sha256 mismatch");
    }
    emit_upgrade_progress_detailed(
        progress,
        DaemonUpgradeProgress {
            stage: "downloading".to_string(),
            message: "安装包已准备好".to_string(),
            target_version: Some(target_version.to_string()),
            source_url: Some(diagnostic_url(source_url)),
            route: route.map(str::to_string),
            attempt: Some(attempt),
            source_index: None,
            source_count: None,
            downloaded_bytes: Some(bytes.len() as u64),
            total_bytes: Some(bytes.len() as u64),
            percent: Some(100.0),
            speed_bytes_per_sec: None,
        },
    );
    Ok(actual_sha)
}

fn release_http_client(
    route: Option<&ReleaseHttpRoute>,
    timeout: Duration,
    no_proxy_by_default: bool,
) -> Result<reqwest::Client> {
    release_http_client_with_policy(
        route,
        no_proxy_by_default,
        ReleaseHttpTimeoutPolicy {
            total_timeout: Some(timeout),
            read_timeout: None,
        },
    )
}

fn release_package_http_client(route: Option<&ReleaseHttpRoute>) -> Result<reqwest::Client> {
    release_http_client_with_policy(route, true, release_package_timeout_policy())
}

fn release_package_timeout_policy() -> ReleaseHttpTimeoutPolicy {
    ReleaseHttpTimeoutPolicy {
        total_timeout: None,
        read_timeout: Some(RELEASE_PACKAGE_READ_TIMEOUT),
    }
}

fn release_http_client_with_policy(
    route: Option<&ReleaseHttpRoute>,
    no_proxy_by_default: bool,
    timeout_policy: ReleaseHttpTimeoutPolicy,
) -> Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder().connect_timeout(RELEASE_HTTP_CONNECT_TIMEOUT);
    if let Some(timeout) = timeout_policy.total_timeout {
        builder = builder.timeout(timeout);
    }
    if let Some(timeout) = timeout_policy.read_timeout {
        builder = builder.read_timeout(timeout);
    }
    match route {
        Some(route) => {
            builder = builder.no_proxy();
            if let Some(proxy_url) = &route.proxy_url {
                builder = builder.proxy(reqwest::Proxy::all(proxy_url)?);
            }
        }
        None if no_proxy_by_default => {
            builder = builder.no_proxy();
        }
        None => {}
    }
    builder.build().context("create HTTP client")
}

fn release_http_routes() -> Vec<ReleaseHttpRoute> {
    let mut routes = Vec::new();
    routes.push(ReleaseHttpRoute {
        label: "direct".to_string(),
        proxy_url: None,
    });
    for proxy in environment_proxy_urls() {
        routes.push(ReleaseHttpRoute {
            label: "environment_proxy".to_string(),
            proxy_url: Some(proxy),
        });
    }
    for port in COMMON_LOCAL_HTTP_PROXY_PORTS {
        routes.push(ReleaseHttpRoute {
            label: format!("local_proxy:{port}"),
            proxy_url: Some(format!("http://127.0.0.1:{port}")),
        });
    }
    let mut seen = HashSet::new();
    routes
        .into_iter()
        .filter(|route| seen.insert((route.label.clone(), route.proxy_url.clone())))
        .collect()
}

fn environment_proxy_urls() -> Vec<String> {
    let mut urls = Vec::new();
    for key in [
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
        "ALL_PROXY",
        "all_proxy",
    ] {
        if let Ok(value) = std::env::var(key) {
            let value = value.trim();
            if !value.is_empty() {
                urls.push(value.to_string());
            }
        }
    }
    dedupe_download_sources(urls)
}

fn is_local_release_url(value: &str) -> bool {
    file_url_path(value).is_some()
        || value.starts_with('/')
        || value.starts_with("./")
        || value.starts_with("../")
}

fn emit_upgrade_progress<F>(
    progress: &mut F,
    stage: &str,
    message: &str,
    target_version: Option<String>,
    source: Option<(&str, Option<&str>)>,
) where
    F: FnMut(DaemonUpgradeProgress),
{
    emit_upgrade_progress_detailed(
        progress,
        DaemonUpgradeProgress {
            stage: stage.to_string(),
            message: message.to_string(),
            target_version,
            source_url: source.map(|(value, _)| diagnostic_url(value)),
            route: source.and_then(|(_, route)| route.map(str::to_string)),
            attempt: None,
            source_index: None,
            source_count: None,
            downloaded_bytes: None,
            total_bytes: None,
            percent: None,
            speed_bytes_per_sec: None,
        },
    );
}

fn emit_download_progress<F>(
    progress: &mut F,
    target_version: &str,
    source_url: &str,
    route: Option<&str>,
    attempt: usize,
    downloaded: u64,
    total: Option<u64>,
    started_at: Instant,
) where
    F: FnMut(DaemonUpgradeProgress),
{
    let elapsed = started_at.elapsed().as_secs_f64().max(0.001);
    let percent = total
        .filter(|total| *total > 0)
        .map(|total| ((downloaded as f64 / total as f64) * 100.0).min(100.0));
    emit_upgrade_progress_detailed(
        progress,
        DaemonUpgradeProgress {
            stage: "downloading".to_string(),
            message: "正在下载安装包".to_string(),
            target_version: Some(target_version.to_string()),
            source_url: Some(diagnostic_url(source_url)),
            route: route.map(str::to_string),
            attempt: Some(attempt),
            source_index: None,
            source_count: None,
            downloaded_bytes: Some(downloaded),
            total_bytes: total,
            percent,
            speed_bytes_per_sec: Some((downloaded as f64 / elapsed) as u64),
        },
    );
}

fn emit_upgrade_progress_detailed<F>(progress: &mut F, event: DaemonUpgradeProgress)
where
    F: FnMut(DaemonUpgradeProgress),
{
    progress(event);
}

fn read_release_manifest_status_from_sources(
    sources: &[String],
) -> Result<ReleaseManifestSelection> {
    if tokio::runtime::Handle::try_current().is_ok() {
        let sources = sources.to_vec();
        let join = std::thread::Builder::new()
            .name("awiki-daemon-release-status".to_string())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .context("create daemon release status runtime")?;
                runtime.block_on(read_release_manifest_from_sources(&sources, &mut |_| {}))
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
    runtime.block_on(read_release_manifest_from_sources(sources, &mut |_| {}))
}

async fn read_release_manifest_async(url: &str) -> Result<DaemonReleaseManifest> {
    let manifest_bytes = read_url_bytes_with_timeout(url, RELEASE_MANIFEST_HTTP_TIMEOUT).await?;
    serde_json::from_slice(&manifest_bytes).context("parse daemon release manifest")
}

async fn read_url_bytes_with_timeout(url: &str, timeout: Duration) -> Result<Vec<u8>> {
    if let Some(path) = file_url_path(url) {
        return std::fs::read(&path).with_context(|| format!("read {}", path.display()));
    }
    if url.starts_with('/') || url.starts_with("./") || url.starts_with("../") {
        return std::fs::read(url).with_context(|| format!("read {url}"));
    }
    let client = release_http_client(None, timeout, false)?;
    read_http_url_bytes_with_retries(&client, url).await
}

async fn read_http_url_bytes_with_retries(client: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    for attempt in 1..=RELEASE_HTTP_MAX_ATTEMPTS {
        let error = match read_http_url_bytes_once(client, url).await {
            Ok(bytes) => return Ok(bytes),
            Err(error) => error,
        };
        if attempt >= RELEASE_HTTP_MAX_ATTEMPTS || !release_http_error_is_retryable(&error) {
            return Err(error);
        }
        tokio::time::sleep(RELEASE_HTTP_RETRY_BASE_DELAY * attempt as u32).await;
    }
    unreachable!("release HTTP retry loop must return")
}

async fn read_http_url_bytes_once(client: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    let response = client.get(url).send().await.context("send HTTP request")?;
    let response = response.error_for_status().context("HTTP error")?;
    Ok(response.bytes().await.context("read HTTP body")?.to_vec())
}

fn release_http_error_is_retryable(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        let Some(error) = cause.downcast_ref::<reqwest::Error>() else {
            return false;
        };
        if let Some(status) = error.status() {
            return status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error();
        }
        error.is_timeout() || error.is_connect() || error.is_body() || error.is_request()
    })
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
    sha256_digest_to_hex(digest)
}

fn sha256_digest_to_hex(digest: impl AsRef<[u8]>) -> String {
    digest
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
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

fn sanitize_public_error_chain(chain: anyhow::Chain<'_>) -> String {
    let parts = chain
        .take(4)
        .map(|error| sanitize_error(&error.to_string()))
        .filter(|message| !message.trim().is_empty())
        .collect::<Vec<_>>();
    let mut deduped = Vec::new();
    for part in parts {
        if deduped.last() == Some(&part) {
            continue;
        }
        deduped.push(part);
    }
    let mut summary = deduped.join(": ");
    if summary.chars().count() > 360 {
        summary = summary.chars().take(360).collect();
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};
    use std::thread;

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

    fn write_manifest_with_download_sources(
        root: &Path,
        version: &str,
        package_path: &str,
        sha: &str,
        sources: &[String],
    ) -> PathBuf {
        let releases = root.join("releases");
        std::fs::create_dir_all(&releases).unwrap();
        let manifest = releases.join("manifest.json");
        std::fs::write(
            &manifest,
            serde_json::to_vec_pretty(&serde_json::json!({
                "latest": version,
                "min_supported": "0.1.0",
                "download_base_urls": sources,
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

    struct TestHttpResponse {
        status: u16,
        body: Vec<u8>,
    }

    impl TestHttpResponse {
        fn ok(body: &[u8]) -> Self {
            Self {
                status: 200,
                body: body.to_vec(),
            }
        }

        fn status(status: u16, body: &str) -> Self {
            Self {
                status,
                body: body.as_bytes().to_vec(),
            }
        }
    }

    struct TestHttpServer {
        address: String,
        requests: Arc<Mutex<Vec<String>>>,
        handle: Option<thread::JoinHandle<()>>,
    }

    impl TestHttpServer {
        fn new(responses: Vec<TestHttpResponse>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind test HTTP server");
            let address = format!("http://{}", listener.local_addr().expect("local addr"));
            let requests = Arc::new(Mutex::new(Vec::new()));
            let server_requests = Arc::clone(&requests);
            let handle = thread::spawn(move || {
                for response in responses {
                    let Ok((stream, _)) = listener.accept() else {
                        break;
                    };
                    handle_http_request(stream, &server_requests, response);
                }
            });
            Self {
                address,
                requests,
                handle: Some(handle),
            }
        }

        fn url(&self, path: &str) -> String {
            format!("{}{}", self.address, path)
        }

        fn request_paths(&self) -> Vec<String> {
            self.requests.lock().expect("request paths mutex").clone()
        }
    }

    impl Drop for TestHttpServer {
        fn drop(&mut self) {
            if let Some(handle) = self.handle.take() {
                if let Some(address) = self.address.strip_prefix("http://") {
                    if let Ok(mut stream) = TcpStream::connect(address) {
                        let _ = stream.write_all(
                            b"GET /__shutdown HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
                        );
                    }
                }
                let _ = handle.join();
            }
        }
    }

    fn handle_http_request(
        mut stream: TcpStream,
        requests: &Arc<Mutex<Vec<String>>>,
        response: TestHttpResponse,
    ) {
        let mut reader = BufReader::new(stream.try_clone().expect("clone test stream"));
        let mut request_line = String::new();
        let _ = reader.read_line(&mut request_line);
        if let Some(path) = request_line.split_whitespace().nth(1) {
            requests
                .lock()
                .expect("request paths mutex")
                .push(path.to_string());
        }
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) if line == "\r\n" || line == "\n" => break,
                Ok(_) => {}
            }
        }
        let reason = if response.status == 200 {
            "OK"
        } else {
            "ERROR"
        };
        let raw_headers = format!(
            "HTTP/1.1 {} {}\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            response.status,
            reason,
            response.body.len()
        );
        stream
            .write_all(raw_headers.as_bytes())
            .expect("write response headers");
        stream
            .write_all(&response.body)
            .expect("write response body");
        let _ = stream.flush();
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

    #[test]
    fn release_download_retries_transient_server_error() {
        let server = TestHttpServer::new(vec![
            TestHttpResponse::status(500, "temporary"),
            TestHttpResponse::ok(b"package"),
        ]);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let bytes = runtime
            .block_on(read_url_bytes_with_timeout(
                &server.url("/package.tar.gz"),
                Duration::from_secs(5),
            ))
            .unwrap();

        assert_eq!(bytes, b"package");
        assert_eq!(
            server.request_paths(),
            vec!["/package.tar.gz".to_string(), "/package.tar.gz".to_string()]
        );
    }

    #[test]
    fn release_package_download_uses_stall_timeout_not_total_deadline() {
        let package_policy = release_package_timeout_policy();
        assert_eq!(package_policy.total_timeout, None);
        assert_eq!(
            package_policy.read_timeout,
            Some(RELEASE_PACKAGE_READ_TIMEOUT)
        );

        let manifest_policy = ReleaseHttpTimeoutPolicy {
            total_timeout: Some(RELEASE_MANIFEST_HTTP_TIMEOUT),
            read_timeout: None,
        };
        assert_eq!(
            manifest_policy.total_timeout,
            Some(RELEASE_MANIFEST_HTTP_TIMEOUT)
        );
        assert_eq!(manifest_policy.read_timeout, None);
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
    fn upgrade_downloads_package_from_manifest_download_base_urls() {
        let (manifest_root, config) = fixture();
        let package_root = tempfile::tempdir().unwrap();
        let bin_root = manifest_root.path().join("bin");
        let current_dir = bin_root.join("current");
        let old_dir = bin_root.join("0.1.0");
        std::fs::create_dir_all(&current_dir).unwrap();
        std::fs::create_dir_all(&old_dir).unwrap();
        std::fs::write(old_dir.join("awiki-deamon"), "old").unwrap();
        std::os::unix::fs::symlink("../0.1.0/awiki-deamon", current_dir.join("awiki-deamon"))
            .unwrap();

        let (archive, sha) = create_package(package_root.path(), "0.2.0");
        write_manifest_with_download_sources(
            manifest_root.path(),
            "0.2.0",
            &archive
                .strip_prefix(package_root.path())
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/"),
            &sha,
            &[format!("file://{}", package_root.path().display())],
        );

        let mut progress_events = Vec::new();
        let report = upgrade_daemon_with_progress(
            &config,
            DaemonUpgradeRequest {
                target_version: "latest".to_string(),
                download_base_url: format!("file://{}", manifest_root.path().display()),
                bin_root: bin_root.clone(),
                restart_service: false,
            },
            |event| progress_events.push(event),
        )
        .unwrap();

        assert_eq!(report.target_version, "0.2.0");
        assert_eq!(
            report.download_base_url,
            format!("file://{}", package_root.path().display())
        );
        assert_eq!(
            std::fs::read_link(current_dir.join("awiki-deamon")).unwrap(),
            PathBuf::from("../0.2.0/awiki-deamon")
        );
        assert!(progress_events
            .iter()
            .any(|event| event.stage == "selecting_source"));
        assert!(progress_events
            .iter()
            .any(|event| event.stage == "downloading" && event.percent == Some(100.0)));
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

        assert!(error
            .chain()
            .any(|cause| cause.to_string().contains("sha256")));
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
