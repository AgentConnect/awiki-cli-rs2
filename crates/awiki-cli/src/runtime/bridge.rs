use crate::config::{Paths, Resolved};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::fmt;
use std::fs;
use std::path::Path;

pub const MODE_HTTP: &str = "http";
pub const MODE_WEBSOCKET: &str = "websocket";
pub const MAX_UNIX_SOCKET_PATH_BYTES: usize = 100;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BridgeRequest {
    pub method: String,
    #[serde(default)]
    pub params: Map<String, Value>,
    pub identity_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BridgeResponse {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub result: Map<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<BridgeError>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BridgeError {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeCallError {
    pub phase: String,
    pub message: String,
    pub cause: String,
}

impl BridgeCallError {
    pub fn new(phase: &str, message: &str, cause: &str) -> Self {
        Self {
            phase: phase.to_string(),
            message: message.to_string(),
            cause: cause.to_string(),
        }
    }
}

impl fmt::Display for BridgeCallError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (!self.message.is_empty(), !self.cause.is_empty()) {
            (true, true) => write!(
                formatter,
                "local websocket bridge request failed: {}: {}",
                self.message, self.cause
            ),
            (true, false) => write!(
                formatter,
                "local websocket bridge request failed: {}",
                self.message
            ),
            (false, true) => write!(
                formatter,
                "local websocket bridge request failed: {}",
                self.cause
            ),
            (false, false) => write!(formatter, "local websocket bridge request failed"),
        }
    }
}

impl std::error::Error for BridgeCallError {}

pub fn resolved_bridge_endpoint(resolved: &Resolved) -> String {
    let configured = resolved.runtime_socket_path.trim();
    if configured.is_empty() {
        normalize_bridge_endpoint(&default_bridge_endpoint(&resolved.paths))
    } else {
        normalize_bridge_endpoint(configured)
    }
}

pub fn default_bridge_endpoint(paths: &Paths) -> String {
    default_bridge_endpoint_for_parts(&paths.workspace_home_dir, &paths.state_dir)
}

#[cfg(not(windows))]
pub fn default_bridge_endpoint_for_parts(workspace_home_dir: &str, state_dir: &str) -> String {
    let state_dir = state_dir.trim();
    let root = if state_dir.is_empty() {
        Path::new(workspace_home_dir).join("runtime")
    } else {
        Path::new(state_dir).to_path_buf()
    };
    root.join("message-daemon.sock")
        .to_string_lossy()
        .into_owned()
}

#[cfg(windows)]
pub fn default_bridge_endpoint_for_parts(workspace_home_dir: &str, _state_dir: &str) -> String {
    let workspace = if workspace_home_dir.trim().is_empty() {
        std::env::temp_dir()
            .join("awiki-cli")
            .to_string_lossy()
            .into_owned()
    } else {
        workspace_home_dir.to_string()
    };
    let digest = Sha256::digest(workspace.as_bytes());
    format!(r"\\.\pipe\awiki-cli-{}", &format!("{digest:x}")[..16])
}

#[cfg(not(windows))]
pub fn normalize_bridge_endpoint(path: &str) -> String {
    normalize_socket_path(path)
}

#[cfg(windows)]
pub fn normalize_bridge_endpoint(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return default_bridge_endpoint_for_parts("", "");
    }
    trimmed.to_string()
}

#[cfg(not(windows))]
pub fn prepare_bridge_endpoint(path: &str) -> anyhow::Result<()> {
    let parent = Path::new(path)
        .parent()
        .ok_or_else(|| anyhow::anyhow!("prepare websocket bridge socket dir: missing parent"))?;
    if !parent.exists() {
        fs::create_dir_all(parent)
            .map_err(|err| anyhow::anyhow!("prepare websocket bridge socket dir: {err}"))?;
        set_dir_mode(parent, 0o700)?;
    }
    Ok(())
}

#[cfg(windows)]
pub fn prepare_bridge_endpoint(path: &str) -> anyhow::Result<()> {
    if !path.to_ascii_lowercase().starts_with(r"\\.\pipe\") {
        anyhow::bail!("windows websocket bridge socket must use a named pipe path");
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn bridge_endpoint_available(path: &str) -> bool {
    fs::metadata(path).is_ok()
}

#[cfg(windows)]
pub fn bridge_endpoint_available(path: &str) -> bool {
    path.trim().to_ascii_lowercase().starts_with(r"\\.\pipe\")
}

#[cfg(not(windows))]
pub fn normalize_socket_path(path: &str) -> String {
    if path.len() <= MAX_UNIX_SOCKET_PATH_BYTES {
        return path.to_string();
    }
    let digest = Sha256::digest(path.as_bytes());
    std::env::temp_dir()
        .join(format!("awiki-cli-{}.sock", &format!("{digest:x}")[..16]))
        .to_string_lossy()
        .into_owned()
}

#[cfg(windows)]
pub fn normalize_socket_path(path: &str) -> String {
    normalize_bridge_endpoint(path)
}

#[cfg(unix)]
fn set_dir_mode(path: &Path, mode: u32) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(|err| anyhow::anyhow!(err))
}

#[cfg(not(unix))]
fn set_dir_mode(_path: &Path, _mode: u32) -> anyhow::Result<()> {
    Ok(())
}
