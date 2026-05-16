use crate::config::{Paths, Resolved};
use crate::transportcfg;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::fmt;
use std::fs::{self, DirBuilder};
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;
use std::time::Duration;

#[cfg(unix)]
pub type BridgeListener = std::os::unix::net::UnixListener;

#[cfg(not(unix))]
#[derive(Debug)]
pub struct BridgeListener;

pub const MODE_HTTP: &str = "http";
pub const MODE_WEBSOCKET: &str = "websocket";
pub const MAX_UNIX_SOCKET_PATH_BYTES: usize = 100;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BridgeRequest {
    #[serde(default)]
    pub method: String,
    #[serde(default)]
    pub params: Map<String, Value>,
    #[serde(default)]
    pub identity_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BridgeResponse {
    #[serde(default)]
    pub ok: bool,
    #[serde(
        default,
        deserialize_with = "deserialize_result_map",
        skip_serializing_if = "Map::is_empty"
    )]
    pub result: Map<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<BridgeError>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BridgeError {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub code: String,
    #[serde(default)]
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
    let fallback_workspace = std::env::temp_dir()
        .join("awiki-cli")
        .to_string_lossy()
        .into_owned();
    windows_default_bridge_endpoint_for_parts(workspace_home_dir, &fallback_workspace)
}

pub fn windows_default_bridge_endpoint_for_parts(
    workspace_home_dir: &str,
    fallback_workspace_home_dir: &str,
) -> String {
    let workspace = if workspace_home_dir.trim().is_empty() {
        fallback_workspace_home_dir
    } else {
        workspace_home_dir
    };
    windows_default_bridge_endpoint_from_workspace(workspace)
}

pub fn windows_default_bridge_endpoint_from_workspace(workspace_home_dir: &str) -> String {
    let digest = Sha256::digest(workspace_home_dir.as_bytes());
    format!(r"\\.\pipe\awiki-cli-{}", &format!("{digest:x}")[..16])
}

#[cfg(not(windows))]
pub fn normalize_bridge_endpoint(path: &str) -> String {
    normalize_socket_path(path)
}

#[cfg(windows)]
pub fn normalize_bridge_endpoint(path: &str) -> String {
    let fallback_workspace = std::env::temp_dir()
        .join("awiki-cli")
        .to_string_lossy()
        .into_owned();
    normalize_windows_bridge_endpoint(path, &fallback_workspace)
}

pub fn normalize_windows_bridge_endpoint(path: &str, fallback_workspace_home_dir: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return windows_default_bridge_endpoint_for_parts("", fallback_workspace_home_dir);
    }
    trimmed.to_string()
}

#[cfg(not(windows))]
pub fn prepare_bridge_endpoint(path: &str) -> anyhow::Result<()> {
    let parent = bridge_parent_dir(path);
    let parent_existed = parent.exists();
    create_bridge_dir(parent)
        .map_err(|err| anyhow::anyhow!("prepare websocket bridge socket dir: {err}"))?;
    if !parent_existed {
        set_dir_mode(parent, 0o700)?;
    }
    Ok(())
}

#[cfg(windows)]
pub fn prepare_bridge_endpoint(path: &str) -> anyhow::Result<()> {
    if !is_windows_named_pipe_endpoint(path) {
        anyhow::bail!("windows websocket bridge socket must use a named pipe path");
    }
    Ok(())
}

pub fn is_windows_named_pipe_endpoint(path: &str) -> bool {
    path.to_ascii_lowercase().starts_with(r"\\.\pipe\")
}

#[cfg(not(windows))]
pub fn bridge_endpoint_available(path: &str) -> bool {
    fs::metadata(path).is_ok()
}

#[cfg(windows)]
pub fn bridge_endpoint_available(_path: &str) -> bool {
    false
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

pub fn call_local_bridge(
    request: BridgeRequest,
    resolved: &Resolved,
) -> anyhow::Result<Map<String, Value>> {
    let bridge = super::resolve(resolved);
    if bridge.mode != MODE_WEBSOCKET {
        anyhow::bail!(
            "runtime mode {} does not use the local websocket bridge",
            bridge.mode
        );
    }
    if bridge.socket_path.trim().is_empty() {
        anyhow::bail!("runtime websocket bridge socket is not configured");
    }
    prepare_bridge_endpoint(&bridge.socket_path)?;
    let timeout_config = transportcfg::resolve();
    if let Err(err) = bridge_health_probe(
        &bridge.socket_path,
        timeout_config.bridge_health_probe_timeout,
    ) {
        return Err(BridgeCallError::new(
            "bridge_health_probe",
            "local websocket bridge unavailable",
            &err.to_string(),
        )
        .into());
    }
    call_bridge_once(
        &bridge.socket_path,
        request,
        timeout_config.bridge_dial_timeout,
        timeout_config.bridge_write_timeout,
        timeout_config.bridge_read_timeout,
    )
}

#[cfg(unix)]
pub fn listen_bridge(path: &str) -> anyhow::Result<BridgeListener> {
    let parent = bridge_parent_dir(path);
    let parent_existed = parent.exists();
    create_bridge_dir(parent)?;
    if !parent_existed {
        set_dir_mode(parent, 0o700)?;
    }
    let _ = fs::remove_file(path);
    std::os::unix::net::UnixListener::bind(path).map_err(|err| anyhow::anyhow!(err))
}

#[cfg(not(unix))]
pub fn listen_bridge(_path: &str) -> anyhow::Result<BridgeListener> {
    anyhow::bail!("windows local websocket bridge I/O is not implemented in Rust port")
}

pub fn handle_bridge_connection_once<RW, F>(stream: RW, dispatch: F) -> io::Result<()>
where
    RW: io::Read + io::Write,
    F: FnOnce(BridgeRequest) -> anyhow::Result<Map<String, Value>>,
{
    let mut reader = BufReader::new(stream);
    let mut line = Vec::new();
    match reader.read_until(b'\n', &mut line) {
        Ok(0) => return write_bridge_response(reader.get_mut(), bridge_error_response("EOF")),
        Ok(_) => {}
        Err(err) => {
            return write_bridge_response(reader.get_mut(), bridge_error_response(&err.to_string()))
        }
    }
    if !line.ends_with(b"\n") {
        return write_bridge_response(reader.get_mut(), bridge_error_response("EOF"));
    }
    let request = match serde_json::from_slice::<BridgeRequest>(&line) {
        Ok(request) => request,
        Err(err) => {
            return write_bridge_response(reader.get_mut(), bridge_error_response(&err.to_string()))
        }
    };
    match dispatch(request) {
        Ok(result) => write_bridge_response(
            reader.get_mut(),
            BridgeResponse {
                ok: true,
                result,
                error: None,
            },
        ),
        Err(err) => {
            write_bridge_response(reader.get_mut(), bridge_error_response(&err.to_string()))
        }
    }
}

fn bridge_error_response(message: &str) -> BridgeResponse {
    BridgeResponse {
        ok: false,
        result: Map::new(),
        error: Some(BridgeError {
            code: String::new(),
            message: message.to_string(),
        }),
    }
}

fn write_bridge_response<W: io::Write>(writer: &mut W, response: BridgeResponse) -> io::Result<()> {
    serde_json::to_writer(&mut *writer, &response)?;
    writer.write_all(b"\n")
}

#[cfg(unix)]
pub fn bridge_health_probe(path: &str, timeout: Duration) -> anyhow::Result<()> {
    let conn = dial_bridge(path, timeout)?;
    drop(conn);
    Ok(())
}

#[cfg(not(unix))]
pub fn bridge_health_probe(_path: &str, _timeout: Duration) -> anyhow::Result<()> {
    anyhow::bail!("windows local websocket bridge I/O is not implemented in Rust port")
}

#[cfg(unix)]
fn call_bridge_once(
    path: &str,
    request: BridgeRequest,
    dial_timeout: Duration,
    write_timeout: Duration,
    read_timeout: Duration,
) -> anyhow::Result<Map<String, Value>> {
    let mut conn = dial_bridge(path, dial_timeout).map_err(|err| {
        BridgeCallError::new(
            "bridge_dial",
            "local websocket bridge unavailable",
            &err.to_string(),
        )
    })?;
    let payload = serde_json::to_vec(&request)?;
    let _ = conn.set_write_timeout(Some(write_timeout));
    if let Err(err) = conn.write_all(&[payload.as_slice(), b"\n"].concat()) {
        return Err(BridgeCallError::new(
            "bridge_write",
            "write websocket bridge request",
            &err.to_string(),
        )
        .into());
    }
    let _ = conn.set_read_timeout(Some(read_timeout));
    let response: BridgeResponse = serde_json::from_reader(&mut conn).map_err(|err| {
        BridgeCallError::new(
            "bridge_read",
            "decode websocket bridge response",
            &err.to_string(),
        )
    })?;
    if !response.ok {
        if let Some(error) = response.error {
            return Err(BridgeCallError::new("bridge_read", &error.message, "").into());
        }
        return Err(BridgeCallError::new(
            "bridge_read",
            "bridge returned failure without details",
            "",
        )
        .into());
    }
    Ok(response.result)
}

#[cfg(not(unix))]
fn call_bridge_once(
    _path: &str,
    _request: BridgeRequest,
    _dial_timeout: Duration,
    _write_timeout: Duration,
    _read_timeout: Duration,
) -> anyhow::Result<Map<String, Value>> {
    Err(BridgeCallError::new(
        "bridge_dial",
        "local websocket bridge unavailable",
        "windows local websocket bridge I/O is not implemented in Rust port",
    )
    .into())
}

#[cfg(unix)]
fn dial_bridge(path: &str, timeout: Duration) -> io::Result<std::os::unix::net::UnixStream> {
    let (sender, receiver) = std::sync::mpsc::channel();
    let path = path.to_string();
    std::thread::spawn(move || {
        let _ = sender.send(std::os::unix::net::UnixStream::connect(path));
    });
    match receiver.recv_timeout(timeout) {
        Ok(result) => result,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            Err(io::Error::new(io::ErrorKind::TimedOut, "i/o timeout"))
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(io::Error::new(
            io::ErrorKind::ConnectionAborted,
            "bridge dial worker disconnected",
        )),
    }
}

#[cfg(not(windows))]
fn bridge_parent_dir(path: &str) -> &Path {
    match Path::new(path).parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    }
}

#[cfg(windows)]
fn bridge_parent_dir(_path: &str) -> &Path {
    Path::new(".")
}

fn deserialize_result_map<'de, D>(deserializer: D) -> Result<Map<String, Value>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<Map<String, Value>>::deserialize(deserializer)?.unwrap_or_default())
}

fn create_bridge_dir(path: &Path) -> io::Result<()> {
    let mut builder = DirBuilder::new();
    builder.recursive(true);
    set_dir_builder_mode(&mut builder, 0o700);
    builder.create(path)
}

#[cfg(unix)]
fn set_dir_builder_mode(builder: &mut DirBuilder, mode: u32) {
    use std::os::unix::fs::DirBuilderExt;
    builder.mode(mode);
}

#[cfg(not(unix))]
fn set_dir_builder_mode(_builder: &mut DirBuilder, _mode: u32) {}

#[cfg(unix)]
fn set_dir_mode(path: &Path, mode: u32) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(|err| anyhow::anyhow!(err))
}

#[cfg(not(unix))]
fn set_dir_mode(_path: &Path, _mode: u32) -> anyhow::Result<()> {
    Ok(())
}
