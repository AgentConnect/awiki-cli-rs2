use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::service::{manage_service, ServiceAction, ServicePlatform, ServiceStatus};
use crate::DaemonConfig;

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
    url: String,
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
    let target_version = normalize_target_version(&request.target_version)?;
    let manifest_url = manifest_url(&request.download_base_url);
    let manifest_bytes = read_url_bytes(&manifest_url)
        .await
        .with_context(|| format!("download daemon manifest {}", public_url(&manifest_url)))?;
    let manifest: DaemonReleaseManifest =
        serde_json::from_slice(&manifest_bytes).context("parse daemon release manifest")?;
    if manifest.latest.trim().is_empty() {
        bail!("daemon release manifest latest version is empty");
    }
    let version = if target_version == "latest" {
        manifest.latest.clone()
    } else {
        target_version
    };
    let package = select_package(&manifest, &version)?;
    let archive_bytes = read_url_bytes(&package.url)
        .await
        .with_context(|| format!("download daemon package {}", public_url(&package.url)))?;
    let actual_sha = sha256_hex(&archive_bytes);
    if !actual_sha.eq_ignore_ascii_case(package.sha256.trim()) {
        bail!("daemon package sha256 mismatch");
    }

    std::fs::create_dir_all(&request.bin_root)
        .with_context(|| format!("create daemon bin root {}", request.bin_root.display()))?;
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
            ServiceAction::Restart,
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
    let _ = std::fs::remove_dir_all(&temp_root);
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

async fn read_url_bytes(url: &str) -> Result<Vec<u8>> {
    if let Some(path) = file_url_path(url) {
        return std::fs::read(&path).with_context(|| format!("read {}", path.display()));
    }
    if url.starts_with('/') || url.starts_with("./") || url.starts_with("../") {
        return std::fs::read(url).with_context(|| format!("read {url}"));
    }
    let response = reqwest::Client::new()
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
        std::fs::write(stage.join("awiki-deamon"), format!("daemon {version}")).unwrap();
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
        let manifest = root.join("manifest.json");
        std::fs::write(
            &manifest,
            serde_json::to_vec_pretty(&serde_json::json!({
                "latest": version,
                "min_supported": "0.1.0",
                "packages": [{
                    "version": version,
                    "os": release_os().unwrap(),
                    "arch": release_arch().unwrap(),
                    "url": format!("file://{}", archive.display()),
                    "sha256": sha,
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        manifest
    }

    #[cfg(unix)]
    #[test]
    fn upgrade_downloads_verifies_extracts_and_swaps_current_symlinks() {
        let (root, config) = fixture();
        let bin_root = root.path().join("bin");
        let old_dir = bin_root.join("0.1.0");
        let current_dir = bin_root.join("current");
        std::fs::create_dir_all(&old_dir).unwrap();
        std::fs::create_dir_all(&current_dir).unwrap();
        std::fs::write(old_dir.join("awiki-deamon"), "old").unwrap();
        std::os::unix::fs::symlink("../0.1.0/awiki-deamon", current_dir.join("awiki-deamon"))
            .unwrap();
        std::os::unix::fs::symlink(
            "../0.1.0/awiki-deamon",
            current_dir.join("awiki-deamon-runtime"),
        )
        .unwrap();
        let (archive, sha) = create_package(root.path(), "0.2.0");
        let manifest = write_manifest(root.path(), "0.2.0", &archive, &sha);

        let report = upgrade_daemon(
            &config,
            DaemonUpgradeRequest {
                target_version: "latest".to_string(),
                download_base_url: format!("file://{}", manifest.display()),
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
        assert_eq!(
            std::fs::read_to_string(bin_root.join("0.2.0").join("awiki-deamon")).unwrap(),
            "daemon 0.2.0"
        );
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
        let manifest = write_manifest(root.path(), "0.2.0", &archive, "deadbeef");

        let error = upgrade_daemon(
            &config,
            DaemonUpgradeRequest {
                target_version: "latest".to_string(),
                download_base_url: format!("file://{}", manifest.display()),
                bin_root,
                restart_service: false,
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("sha256"));
        assert_eq!(
            std::fs::read_link(current_dir.join("awiki-deamon")).unwrap(),
            PathBuf::from("../0.1.0/awiki-deamon")
        );
    }
}
