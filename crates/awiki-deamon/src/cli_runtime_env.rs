use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::service::runtime_env_file_path;
use crate::DaemonConfig;

pub const CLI_ENV_PASSTHROUGH_KEY: &str = "AWIKI_DAEMON_CLI_ENV_PASSTHROUGH";

pub const DEFAULT_CLI_ENV_PASSTHROUGH_SELECTORS: &[&str] = &[
    "ANTHROPIC_*",
    "CLAUDE_*",
    "OPENAI_*",
    "CODEX_*",
    "HERMES_*",
    "AWIKI_HERMES_*",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "NO_PROXY",
    "http_proxy",
    "https_proxy",
    "all_proxy",
    "no_proxy",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CliRuntimeEnvCaptureReport {
    pub env_file_path: PathBuf,
    pub captured_variable_names: Vec<String>,
    pub passthrough_selectors: Vec<String>,
    pub path_entry_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RuntimeEnvSelector {
    Exact(String),
    Prefix(String),
}

impl RuntimeEnvSelector {
    fn as_token(&self) -> String {
        match self {
            Self::Exact(value) => value.clone(),
            Self::Prefix(value) => format!("{value}*"),
        }
    }
}

pub fn capture_and_write(config: &DaemonConfig) -> Result<CliRuntimeEnvCaptureReport> {
    let env_file_path = runtime_env_file_path(config);
    let capture = capture_current_process_env();
    let content = render_env_file(&capture);
    write_private_env_file(&env_file_path, &content)?;
    Ok(CliRuntimeEnvCaptureReport {
        env_file_path,
        captured_variable_names: capture.captured_variable_names(),
        passthrough_selectors: capture.passthrough_selectors.clone(),
        path_entry_count: capture.path_entry_count,
    })
}

pub fn cli_child_path() -> Option<OsString> {
    let home = awiki_user_dirs::try_home_dir();
    build_cli_child_path(home.as_deref(), std::env::var_os("PATH").as_deref())
}

pub fn build_cli_child_path(home: Option<&Path>, current_path: Option<&OsStr>) -> Option<OsString> {
    let mut paths = Vec::<PathBuf>::new();
    if let Some(home) = home {
        push_existing_dir(&mut paths, home.join(".local").join("bin"));
        push_existing_dir(&mut paths, home.join(".npm-global").join("bin"));
        push_existing_dir(&mut paths, home.join(".nvm").join("current").join("bin"));
        push_nvm_node_bins(&mut paths, home);
    }
    push_existing_dir(&mut paths, PathBuf::from("/opt/homebrew/bin"));
    push_existing_dir(&mut paths, PathBuf::from("/usr/local/bin"));
    if let Some(current_path) = current_path {
        for path in std::env::split_paths(current_path) {
            push_path(&mut paths, path);
        }
    }
    if paths.is_empty() {
        return None;
    }
    std::env::join_paths(paths).ok()
}

pub fn find_executable_on_path(name: &str, path: &OsStr) -> Option<PathBuf> {
    std::env::split_paths(path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

pub(crate) fn runtime_env_selectors(
    default_selectors: &[&str],
    extra: Option<&str>,
) -> Vec<RuntimeEnvSelector> {
    let mut selectors = Vec::new();
    for token in default_selectors
        .iter()
        .copied()
        .chain(extra.into_iter().flat_map(split_runtime_env_selector_list))
    {
        let Some(selector) = parse_runtime_env_selector(token) else {
            continue;
        };
        if !selectors.contains(&selector) {
            selectors.push(selector);
        }
    }
    selectors
}

pub(crate) fn runtime_env_key_allowed(key: &str, selectors: &[RuntimeEnvSelector]) -> bool {
    selectors.iter().any(|selector| match selector {
        RuntimeEnvSelector::Exact(exact) => key == exact,
        RuntimeEnvSelector::Prefix(prefix) => key.starts_with(prefix),
    })
}

fn capture_current_process_env() -> CapturedCliRuntimeEnv {
    let extra_selectors = std::env::var(CLI_ENV_PASSTHROUGH_KEY).ok();
    let selectors = runtime_env_selectors(
        DEFAULT_CLI_ENV_PASSTHROUGH_SELECTORS,
        extra_selectors.as_deref(),
    );
    let passthrough_selectors = selectors
        .iter()
        .map(RuntimeEnvSelector::as_token)
        .collect::<Vec<_>>();
    let path = cli_child_path().or_else(|| std::env::var_os("PATH"));
    let path_entry_count = path
        .as_deref()
        .map(std::env::split_paths)
        .map(Iterator::count)
        .unwrap_or_default();
    let mut values = BTreeMap::new();
    for (key, value) in std::env::vars_os() {
        let Some(key) = key.to_str() else {
            continue;
        };
        if key == CLI_ENV_PASSTHROUGH_KEY || key == "PATH" {
            continue;
        }
        if !runtime_env_key_allowed(key, &selectors) {
            continue;
        }
        let Some(value) = value.to_str() else {
            continue;
        };
        if !env_file_value_is_supported(value) {
            continue;
        }
        values.insert(key.to_string(), value.to_string());
    }
    CapturedCliRuntimeEnv {
        path,
        passthrough_selectors,
        values,
        path_entry_count,
    }
}

fn render_env_file(capture: &CapturedCliRuntimeEnv) -> String {
    let mut lines = vec![
        "# Managed by awiki-deamon. Do not edit while the daemon is running.".to_string(),
        "# This file captures the user's CLI agent environment for daemon-launched runtimes."
            .to_string(),
    ];
    if let Some(path) = capture.path.as_ref().and_then(|value| value.to_str()) {
        lines.push(format!("PATH={}", quote_env_file_value(path)));
    }
    lines.push(format!(
        "{CLI_ENV_PASSTHROUGH_KEY}={}",
        quote_env_file_value(&capture.passthrough_selectors.join(" "))
    ));
    for (key, value) in &capture.values {
        lines.push(format!("{key}={}", quote_env_file_value(value)));
    }
    lines.push(String::new());
    lines.join("\n")
}

fn write_private_env_file(path: &Path, content: &str) -> Result<()> {
    if let Ok(metadata) = std::fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() {
            bail!(
                "refusing to write daemon runtime env symlink {}",
                path.display()
            );
        }
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create daemon runtime env directory {}", parent.display()))?;
        set_private_dir_permissions(parent)?;
    }
    let mut file = private_file_for_write(path)?;
    file.write_all(content.as_bytes())
        .with_context(|| format!("write daemon runtime env file {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("flush daemon runtime env file {}", path.display()))?;
    set_private_file_permissions(path)?;
    Ok(())
}

fn private_file_for_write(path: &Path) -> Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .with_context(|| format!("open daemon runtime env file {}", path.display()))
}

fn set_private_dir_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .with_context(|| format!("secure daemon runtime env directory {}", path.display()))?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

fn set_private_file_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("secure daemon runtime env file {}", path.display()))?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

fn quote_env_file_value(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
        .replace('`', "\\`");
    format!("\"{escaped}\"")
}

fn env_file_value_is_supported(value: &str) -> bool {
    !value.contains('\n') && !value.contains('\r') && !value.contains('\0')
}

fn split_runtime_env_selector_list(value: &str) -> impl Iterator<Item = &str> {
    value.split(|ch: char| ch == ',' || ch == ';' || ch == ':' || ch.is_ascii_whitespace())
}

fn parse_runtime_env_selector(raw: &str) -> Option<RuntimeEnvSelector> {
    let token = raw.trim();
    if token.is_empty() {
        return None;
    }
    if let Some(prefix) = token.strip_suffix('*') {
        let prefix = prefix.trim();
        if valid_runtime_env_name_prefix(prefix) {
            return Some(RuntimeEnvSelector::Prefix(prefix.to_string()));
        }
        return None;
    }
    if valid_runtime_env_name(token) {
        Some(RuntimeEnvSelector::Exact(token.to_string()))
    } else {
        None
    }
}

fn valid_runtime_env_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphabetic() || byte.is_ascii_digit() || byte == b'_')
}

fn valid_runtime_env_name_prefix(value: &str) -> bool {
    valid_runtime_env_name(value)
}

fn push_nvm_node_bins(paths: &mut Vec<PathBuf>, home: &Path) {
    let versions_dir = home.join(".nvm").join("versions").join("node");
    let Ok(entries) = std::fs::read_dir(versions_dir) else {
        return;
    };
    let mut bins = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("bin"))
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    bins.sort();
    bins.reverse();
    for bin in bins {
        push_path(paths, bin);
    }
}

fn push_existing_dir(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if path.is_dir() {
        push_path(paths, path);
    }
}

fn push_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if path.as_os_str().is_empty() || paths.iter().any(|existing| existing == &path) {
        return;
    }
    paths.push(path);
}

#[derive(Debug, Clone)]
struct CapturedCliRuntimeEnv {
    path: Option<OsString>,
    passthrough_selectors: Vec<String>,
    values: BTreeMap<String, String>,
    path_entry_count: usize,
}

impl CapturedCliRuntimeEnv {
    fn captured_variable_names(&self) -> Vec<String> {
        self.values.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_env_selectors_accept_exact_prefix_and_extra_list() {
        let selectors = runtime_env_selectors(
            &["ANTHROPIC_*", "CLAUDE_CODEX_MODEL", "invalid-lower"],
            Some("OPENAI_API_KEY;OPENAI_BASE_URL CODEX_*:http_proxy"),
        );

        assert!(runtime_env_key_allowed("ANTHROPIC_BASE_URL", &selectors));
        assert!(runtime_env_key_allowed("ANTHROPIC_AUTH_TOKEN", &selectors));
        assert!(runtime_env_key_allowed("CLAUDE_CODEX_MODEL", &selectors));
        assert!(runtime_env_key_allowed("OPENAI_API_KEY", &selectors));
        assert!(runtime_env_key_allowed("OPENAI_BASE_URL", &selectors));
        assert!(runtime_env_key_allowed("CODEX_SANDBOX", &selectors));
        assert!(runtime_env_key_allowed("http_proxy", &selectors));
        assert!(!runtime_env_key_allowed("ANTHROPIC", &selectors));
        assert!(!runtime_env_key_allowed("invalid-lower", &selectors));
        assert!(!runtime_env_key_allowed(
            CLI_ENV_PASSTHROUGH_KEY,
            &selectors
        ));
    }

    #[test]
    fn cli_child_path_adds_common_user_cli_bins_before_service_path() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let v18 = home.join(".nvm/versions/node/v18.1.0/bin");
        let v24 = home.join(".nvm/versions/node/v24.12.0/bin");
        let local_bin = home.join(".local/bin");
        std::fs::create_dir_all(&v18).unwrap();
        std::fs::create_dir_all(&v24).unwrap();
        std::fs::create_dir_all(&local_bin).unwrap();

        let path = build_cli_child_path(Some(&home), Some(OsStr::new("/usr/bin:/bin")))
            .expect("path should be built");
        let entries = std::env::split_paths(&path).collect::<Vec<_>>();

        assert_eq!(entries[0], local_bin);
        assert_eq!(entries[1], v24);
        assert_eq!(entries[2], v18);
        assert!(entries.contains(&PathBuf::from("/usr/bin")));
        assert!(entries.contains(&PathBuf::from("/bin")));
    }

    #[test]
    fn render_env_file_uses_shell_and_systemd_compatible_quotes() {
        let mut values = BTreeMap::new();
        values.insert(
            "ANTHROPIC_BASE_URL".to_string(),
            "http://127.0.0.1:14000/v1".to_string(),
        );
        values.insert("OPENAI_API_KEY".to_string(), "tok_$`\"\\value".to_string());
        let capture = CapturedCliRuntimeEnv {
            path: Some(OsString::from("/usr/local/bin:/usr/bin")),
            passthrough_selectors: vec!["ANTHROPIC_*".to_string(), "OPENAI_*".to_string()],
            values,
            path_entry_count: 2,
        };
        let rendered = render_env_file(&capture);

        assert!(rendered.contains("PATH=\"/usr/local/bin:/usr/bin\""));
        assert!(rendered.contains("AWIKI_DAEMON_CLI_ENV_PASSTHROUGH=\"ANTHROPIC_* OPENAI_*\""));
        assert!(rendered.contains("ANTHROPIC_BASE_URL=\"http://127.0.0.1:14000/v1\""));
        assert!(rendered.contains("OPENAI_API_KEY=\"tok_\\$\\`\\\"\\\\value\""));
    }
}
