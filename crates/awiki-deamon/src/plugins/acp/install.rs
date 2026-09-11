use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use fs2::FileExt as _;

use super::catalog::AcpAgentCatalogEntry;
use super::connection::redact_line;
use crate::plugins::generic_cli::process::ManagedChild;
use crate::DaemonConfig;

const INSTALL_TIMEOUT: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpInstalledProgram {
    pub install_root: PathBuf,
    pub program: PathBuf,
    pub entrypoint: PathBuf,
    pub version: String,
}

pub fn prepare_local(
    catalog: &AcpAgentCatalogEntry,
    checkout: &Path,
) -> Result<AcpInstalledProgram> {
    if !checkout.is_absolute() {
        bail!("ACP local_checkout must be an absolute path");
    }
    let entrypoint = checkout.join(catalog.local_entrypoint);
    if !entrypoint.is_file() {
        bail!(
            "{} ACP entrypoint is missing; {}",
            catalog.display_name,
            catalog.local_build_hint
        );
    }
    let program = runtime_program(catalog)?;
    Ok(AcpInstalledProgram {
        install_root: checkout.to_path_buf(),
        program,
        entrypoint,
        version: "local".to_string(),
    })
}

pub fn install_npm(
    config: &DaemonConfig,
    catalog: &AcpAgentCatalogEntry,
    version: &str,
) -> Result<AcpInstalledProgram> {
    validate_version_pin(version)?;
    let program = runtime_program(catalog)?;
    let npm = executable_on_path("npm").context("npm is required to install ACP runtime")?;
    let install_root = config
        .state_root
        .join("runtime")
        .join("acp")
        .join(catalog.agent_id);
    std::fs::create_dir_all(&install_root)
        .with_context(|| format!("create ACP install root {}", install_root.display()))?;
    let _lock = InstallLock::acquire(&install_root.join(".install.lock"))?;
    let packages = (catalog.npm_packages)(version);
    let mut command = Command::new(&npm);
    command
        .arg("install")
        .arg("--prefix")
        .arg(&install_root)
        .arg("--save-exact")
        .arg("--no-audit")
        .arg("--no-fund")
        .args(&packages)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    apply_safe_install_environment(&mut command, config, catalog)?;
    let output = ManagedChild::spawn(&mut command, "spawn npm ACP install")?
        .wait_timeout("wait for npm ACP install", INSTALL_TIMEOUT)?;
    if !output.output.status.success() {
        let summary = sanitized_output_summary(&output.output.stderr);
        bail!("npm ACP install failed: {summary}");
    }
    let entrypoint = install_root.join(catalog.npm_entrypoint);
    if !entrypoint.is_file() {
        bail!(
            "npm ACP install completed without {} entrypoint",
            catalog.display_name
        );
    }
    Ok(AcpInstalledProgram {
        install_root,
        program,
        entrypoint,
        version: version.to_string(),
    })
}

fn apply_safe_install_environment(
    command: &mut Command,
    config: &DaemonConfig,
    catalog: &AcpAgentCatalogEntry,
) -> Result<()> {
    command.env_clear();
    if let Some(path) =
        crate::cli_runtime_env::cli_child_path().or_else(|| std::env::var_os("PATH"))
    {
        command.env("PATH", path);
    }
    for name in catalog.inherited_process_env_names {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    let cache = config.state_root.join("runtime/acp/.npm-cache");
    std::fs::create_dir_all(&cache)
        .with_context(|| format!("create ACP npm cache {}", cache.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&cache, std::fs::Permissions::from_mode(0o700))?;
    }
    command.env("npm_config_cache", cache);
    Ok(())
}

fn runtime_program(catalog: &AcpAgentCatalogEntry) -> Result<PathBuf> {
    let program = executable_on_path(catalog.program_name)
        .with_context(|| format!("{} is required to run ACP runtime", catalog.program_name))?;
    if let Some(minimum) = catalog.minimum_program_version {
        require_program_version(&program, catalog, minimum)?;
    }
    Ok(program)
}

fn require_program_version(
    program: &Path,
    catalog: &AcpAgentCatalogEntry,
    minimum: (u64, u64),
) -> Result<()> {
    let mut command = Command::new(program);
    command
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = ManagedChild::spawn(&mut command, "spawn ACP program version probe")?
        .wait_timeout("wait for ACP program version probe", Duration::from_secs(5))?;
    if !output.output.status.success() {
        bail!("{} version probe failed", catalog.program_name);
    }
    let version = String::from_utf8_lossy(&output.output.stdout);
    let (major, minor) = parse_program_version(&version, catalog.program_name)?;
    if major < minimum.0 || (major == minimum.0 && minor < minimum.1) {
        bail!(
            "{} requires {} >= {}.{}",
            catalog.display_name,
            catalog.program_name,
            minimum.0,
            minimum.1
        );
    }
    Ok(())
}

fn parse_program_version(value: &str, program_name: &str) -> Result<(u64, u64)> {
    let value = value.trim().trim_start_matches('v');
    let mut parts = value.split('.');
    let major = parts
        .next()
        .with_context(|| format!("{program_name} version is missing major component"))?
        .parse()
        .with_context(|| format!("parse {program_name} major version"))?;
    let minor = parts
        .next()
        .with_context(|| format!("{program_name} version is missing minor component"))?
        .parse()
        .with_context(|| format!("parse {program_name} minor version"))?;
    Ok((major, minor))
}

fn validate_version_pin(version: &str) -> Result<()> {
    let (without_build, build) = version
        .split_once('+')
        .map_or((version, None), |(value, suffix)| (value, Some(suffix)));
    let (core, prerelease) = without_build
        .split_once('-')
        .map_or((without_build, None), |(core, suffix)| (core, Some(suffix)));
    let parts = core.split('.').collect::<Vec<_>>();
    let valid_numeric_identifier = |part: &str| {
        !part.is_empty()
            && part.bytes().all(|byte| byte.is_ascii_digit())
            && (part == "0" || !part.starts_with('0'))
            && part.parse::<u64>().is_ok()
    };
    let valid_core = parts.len() == 3 && parts.iter().all(|part| valid_numeric_identifier(part));
    let valid_prerelease = prerelease.is_none_or(|suffix| {
        suffix.split('.').all(|identifier| {
            !identifier.is_empty()
                && identifier
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                && (!identifier.bytes().all(|byte| byte.is_ascii_digit())
                    || valid_numeric_identifier(identifier))
        })
    });
    let valid_build = build.is_none_or(|suffix| {
        suffix.split('.').all(|identifier| {
            !identifier.is_empty()
                && identifier
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
    });
    if !valid_core || !valid_prerelease || !valid_build {
        bail!("ACP package_version must be an exact SemVer version");
    }
    Ok(())
}

fn executable_on_path(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|directory| directory.join(name))
            .find(|candidate| candidate.is_file())
    })
}

fn sanitized_output_summary(output: &[u8]) -> String {
    let text = String::from_utf8_lossy(output);
    let summary = text
        .lines()
        .rev()
        .take(8)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(redact_line)
        .collect::<Vec<_>>()
        .join(" | ");
    if summary.is_empty() {
        "no diagnostic output".to_string()
    } else {
        summary
    }
}

struct InstallLock {
    file: File,
}

impl InstallLock {
    fn acquire(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(path)
            .with_context(|| format!("open ACP install lock {}", path.display()))?;
        file.lock_exclusive()
            .with_context(|| format!("lock ACP install {}", path.display()))?;
        Ok(Self { file })
    }
}

impl Drop for InstallLock {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.file);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn npm_install_environment_does_not_inherit_parent_secrets() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        let env = executable_on_path("env").expect("env executable");
        let mut command = Command::new(env);
        command.env("AWIKI_ACP_INSTALL_SECRET_TEST", "must-not-leak");
        apply_safe_install_environment(
            &mut command,
            &config,
            &crate::plugins::acp::catalog::deepseek_harness::ENTRY,
        )
        .unwrap();

        let output = command.output().unwrap();
        assert!(output.status.success());
        let environment = String::from_utf8(output.stdout).unwrap();
        assert!(!environment.contains("AWIKI_ACP_INSTALL_SECRET_TEST"));
        assert!(!environment.contains("must-not-leak"));
        assert!(environment.contains("npm_config_cache="));
    }

    #[test]
    fn npm_package_version_requires_an_exact_semver_pin() {
        assert!(validate_version_pin("0.1.0-rc.6").is_ok());
        assert!(validate_version_pin("1.2.3").is_ok());
        assert!(validate_version_pin("1.2.3+build.1").is_ok());
        assert!(validate_version_pin("1.2.3-rc.1+build.7").is_ok());
        for unpinned in [
            "latest",
            "next",
            "1.2",
            "^1.2.3",
            "1.2.x",
            "01.2.3",
            "1.02.3",
            "1.2.03",
            "1.2.3-rc..1",
            "1.2.3-01",
            "1.2.3+",
            "1.2.3+build..1",
            "1.2.3+build+again",
            "",
        ] {
            assert!(
                validate_version_pin(unpinned).is_err(),
                "{unpinned:?} must not be accepted as an exact version"
            );
        }
    }
}
