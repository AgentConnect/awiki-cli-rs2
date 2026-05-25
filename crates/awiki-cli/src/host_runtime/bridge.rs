use crate::cli_http;
use crate::workspace_config::{Paths, Resolved};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::fmt;
use std::fs::{self, DirBuilder};
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;
#[cfg(windows)]
use std::sync::{Arc, Mutex};
use std::time::Duration;
#[cfg(windows)]
use std::time::Instant;

#[cfg(unix)]
pub type BridgeListener = std::os::unix::net::UnixListener;

#[cfg(windows)]
#[derive(Debug)]
pub struct BridgeListener {
    path: String,
    state: Arc<Mutex<BridgeListenerState>>,
}

#[cfg(windows)]
#[derive(Debug, Default)]
struct BridgeListenerState {
    nonblocking: bool,
    pending: Option<windows_sys::Win32::Foundation::HANDLE>,
}

#[cfg(windows)]
impl Drop for BridgeListenerState {
    fn drop(&mut self) {
        if let Some(handle) = self.pending.take() {
            windows_close_handle(handle);
        }
    }
}

#[cfg(unix)]
pub type BridgeStream = std::os::unix::net::UnixStream;

#[cfg(windows)]
#[derive(Debug)]
pub struct BridgeStream {
    handle: windows_sys::Win32::Foundation::HANDLE,
    disconnect_on_drop: bool,
    use_overlapped_io: bool,
    read_deadline: Option<Instant>,
    write_deadline: Option<Instant>,
}

#[cfg(windows)]
impl BridgeStream {
    fn with_deadlines(mut self, write_timeout: Duration, read_timeout: Duration) -> BridgeStream {
        self.write_deadline = Some(Instant::now() + write_timeout);
        self.read_deadline = Some(Instant::now() + read_timeout);
        self
    }
}

#[cfg(windows)]
impl io::Read for BridgeStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.use_overlapped_io {
            return windows_read_handle_overlapped(
                self.handle,
                buf,
                windows_deadline_remaining(self.read_deadline),
            );
        }
        windows_read_handle(self.handle, buf)
    }
}

#[cfg(windows)]
impl io::Write for BridgeStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.use_overlapped_io {
            return windows_write_handle_overlapped(
                self.handle,
                buf,
                windows_deadline_remaining(self.write_deadline),
            );
        }
        windows_write_handle(self.handle, buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        windows_flush_handle(self.handle)
    }
}

#[cfg(windows)]
impl Drop for BridgeStream {
    fn drop(&mut self) {
        unsafe {
            if self.disconnect_on_drop {
                windows_sys::Win32::System::Pipes::DisconnectNamedPipe(self.handle);
            }
        }
        windows_close_handle(self.handle);
    }
}

#[cfg(not(any(unix, windows)))]
#[derive(Debug)]
pub struct BridgeListener;

#[cfg(not(any(unix, windows)))]
#[derive(Debug)]
pub struct BridgeStream;

#[cfg(not(any(unix, windows)))]
impl io::Read for BridgeStream {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        Err(unsupported_bridge_io())
    }
}

#[cfg(not(any(unix, windows)))]
impl io::Write for BridgeStream {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        Err(unsupported_bridge_io())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

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
pub fn bridge_endpoint_available(path: &str) -> bool {
    bridge_health_probe(path, Duration::from_millis(0)).is_ok()
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
    let timeout_config = cli_http::resolve();
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
#[cfg(windows)]
pub fn listen_bridge(path: &str) -> anyhow::Result<BridgeListener> {
    prepare_bridge_endpoint(path)?;
    let pending = windows_create_named_pipe(path, true)?;
    Ok(BridgeListener {
        path: path.to_string(),
        state: Arc::new(Mutex::new(BridgeListenerState {
            nonblocking: false,
            pending: Some(pending),
        })),
    })
}

#[cfg(not(any(unix, windows)))]
pub fn listen_bridge(_path: &str) -> anyhow::Result<BridgeListener> {
    anyhow::bail!(unsupported_bridge_io())
}

#[cfg(unix)]
pub fn set_bridge_listener_nonblocking(
    listener: &BridgeListener,
    nonblocking: bool,
) -> io::Result<()> {
    listener.set_nonblocking(nonblocking)
}

#[cfg(windows)]
pub fn set_bridge_listener_nonblocking(
    listener: &BridgeListener,
    nonblocking: bool,
) -> io::Result<()> {
    listener
        .state
        .lock()
        .map_err(|_| io::Error::new(io::ErrorKind::Other, "bridge listener mutex poisoned"))?
        .nonblocking = nonblocking;
    Ok(())
}

#[cfg(not(any(unix, windows)))]
pub fn set_bridge_listener_nonblocking(
    _listener: &BridgeListener,
    _nonblocking: bool,
) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
pub fn clone_bridge_listener(listener: &BridgeListener) -> io::Result<BridgeListener> {
    listener.try_clone()
}

#[cfg(windows)]
pub fn clone_bridge_listener(listener: &BridgeListener) -> io::Result<BridgeListener> {
    Ok(BridgeListener {
        path: listener.path.clone(),
        state: listener.state.clone(),
    })
}

#[cfg(not(any(unix, windows)))]
pub fn clone_bridge_listener(_listener: &BridgeListener) -> io::Result<BridgeListener> {
    Ok(BridgeListener)
}

#[cfg(unix)]
pub fn accept_bridge(listener: &BridgeListener) -> io::Result<BridgeStream> {
    listener.accept().map(|(stream, _)| stream)
}

#[cfg(windows)]
pub fn accept_bridge(listener: &BridgeListener) -> io::Result<BridgeStream> {
    windows_accept_named_pipe(listener)
}

#[cfg(not(any(unix, windows)))]
pub fn accept_bridge(_listener: &BridgeListener) -> io::Result<BridgeStream> {
    Err(unsupported_bridge_io())
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
#[cfg(windows)]
pub fn bridge_health_probe(path: &str, timeout: Duration) -> anyhow::Result<()> {
    let conn = windows_dial_named_pipe(path, timeout)?;
    drop(conn);
    Ok(())
}

#[cfg(not(any(unix, windows)))]
pub fn bridge_health_probe(_path: &str, _timeout: Duration) -> anyhow::Result<()> {
    anyhow::bail!(unsupported_bridge_io())
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

#[cfg(windows)]
fn call_bridge_once(
    path: &str,
    request: BridgeRequest,
    dial_timeout: Duration,
    write_timeout: Duration,
    read_timeout: Duration,
) -> anyhow::Result<Map<String, Value>> {
    let mut conn = windows_dial_named_pipe(path, dial_timeout)
        .map(|conn| conn.with_deadlines(write_timeout, read_timeout))
        .map_err(|err| {
            BridgeCallError::new(
                "bridge_dial",
                "local websocket bridge unavailable",
                &err.to_string(),
            )
        })?;
    let payload = serde_json::to_vec(&request)?;
    if let Err(err) = conn.write_all(&[payload.as_slice(), b"\n"].concat()) {
        return Err(BridgeCallError::new(
            "bridge_write",
            "write websocket bridge request",
            &err.to_string(),
        )
        .into());
    }
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

#[cfg(not(any(unix, windows)))]
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
        &unsupported_bridge_io().to_string(),
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

#[cfg(windows)]
fn windows_accept_named_pipe(listener: &BridgeListener) -> io::Result<BridgeStream> {
    loop {
        let mut state = listener
            .state
            .lock()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "bridge listener mutex poisoned"))?;
        if state.pending.is_none() {
            state.pending = Some(windows_create_named_pipe(&listener.path, true)?);
        }
        let handle = state.pending.expect("pending pipe exists");
        match windows_try_connect_named_pipe(handle) {
            Ok(WindowsPipeConnectState::Connected) => {
                state.pending = None;
                windows_set_named_pipe_wait(handle)?;
                return Ok(BridgeStream {
                    handle,
                    disconnect_on_drop: true,
                    use_overlapped_io: false,
                    read_deadline: None,
                    write_deadline: None,
                });
            }
            Ok(WindowsPipeConnectState::Listening) if state.nonblocking => {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "no pipe client waiting",
                ));
            }
            Ok(WindowsPipeConnectState::Listening) => {
                drop(state);
                std::thread::sleep(Duration::from_millis(50));
            }
            Ok(WindowsPipeConnectState::Stale) => {
                state.pending = None;
                windows_close_handle(handle);
            }
            Err(err) => {
                state.pending = None;
                windows_close_handle(handle);
                return Err(err);
            }
        }
    }
}

#[cfg(windows)]
fn windows_create_named_pipe(
    path: &str,
    nonblocking: bool,
) -> io::Result<windows_sys::Win32::Foundation::HANDLE> {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Pipes::{
        CreateNamedPipeW, PIPE_ACCESS_DUPLEX, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE,
        PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
    };

    let wait_mode = if nonblocking {
        windows_sys::Win32::System::Pipes::PIPE_NOWAIT
    } else {
        PIPE_WAIT
    };
    let wide = windows_wide_null(path);
    let handle = unsafe {
        CreateNamedPipeW(
            wide.as_ptr(),
            PIPE_ACCESS_DUPLEX,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | wait_mode,
            PIPE_UNLIMITED_INSTANCES,
            64 * 1024,
            64 * 1024,
            0,
            std::ptr::null(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    Ok(handle)
}

#[cfg(windows)]
enum WindowsPipeConnectState {
    Connected,
    Listening,
    Stale,
}

#[cfg(windows)]
fn windows_try_connect_named_pipe(
    handle: windows_sys::Win32::Foundation::HANDLE,
) -> io::Result<WindowsPipeConnectState> {
    let connected = unsafe {
        windows_sys::Win32::System::Pipes::ConnectNamedPipe(handle, std::ptr::null_mut())
    };
    if connected != 0 {
        return Ok(WindowsPipeConnectState::Connected);
    }
    let err = io::Error::last_os_error();
    match err.raw_os_error() {
        Some(code) if code == windows_sys::Win32::Foundation::ERROR_PIPE_CONNECTED as i32 => {
            Ok(WindowsPipeConnectState::Connected)
        }
        Some(code) if code == windows_sys::Win32::Foundation::ERROR_PIPE_LISTENING as i32 => {
            Ok(WindowsPipeConnectState::Listening)
        }
        Some(code) if code == windows_sys::Win32::Foundation::ERROR_NO_DATA as i32 => {
            Ok(WindowsPipeConnectState::Stale)
        }
        _ => Err(err),
    }
}

#[cfg(windows)]
fn windows_dial_named_pipe(path: &str, timeout: Duration) -> io::Result<BridgeStream> {
    let started = Instant::now();
    loop {
        match windows_open_named_pipe(path) {
            Ok(handle) => {
                return Ok(BridgeStream {
                    handle,
                    disconnect_on_drop: false,
                    use_overlapped_io: true,
                    read_deadline: None,
                    write_deadline: None,
                });
            }
            Err(err)
                if err.raw_os_error()
                    == Some(windows_sys::Win32::Foundation::ERROR_PIPE_BUSY as i32)
                    && started.elapsed() < timeout =>
            {
                std::thread::sleep(std::cmp::min(
                    Duration::from_millis(10),
                    timeout.saturating_sub(started.elapsed()),
                ));
            }
            Err(err)
                if err.raw_os_error()
                    == Some(windows_sys::Win32::Foundation::ERROR_PIPE_BUSY as i32) =>
            {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "i/o timeout"));
            }
            Err(err) => return Err(err),
        }
    }
}

#[cfg(windows)]
fn windows_open_named_pipe(path: &str) -> io::Result<windows_sys::Win32::Foundation::HANDLE> {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_OVERLAPPED, FILE_GENERIC_READ,
        FILE_GENERIC_WRITE, OPEN_EXISTING, SECURITY_ANONYMOUS, SECURITY_SQOS_PRESENT,
    };

    let wide = windows_wide_null(path);
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            FILE_GENERIC_READ | FILE_GENERIC_WRITE,
            0,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL
                | FILE_FLAG_OVERLAPPED
                | SECURITY_SQOS_PRESENT
                | SECURITY_ANONYMOUS,
            0,
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    Ok(handle)
}

#[cfg(windows)]
fn windows_set_named_pipe_wait(handle: windows_sys::Win32::Foundation::HANDLE) -> io::Result<()> {
    let mut mode = windows_sys::Win32::System::Pipes::PIPE_READMODE_BYTE
        | windows_sys::Win32::System::Pipes::PIPE_WAIT;
    let ok = unsafe {
        windows_sys::Win32::System::Pipes::SetNamedPipeHandleState(
            handle,
            &mut mode,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(windows)]
fn windows_read_handle_overlapped(
    handle: windows_sys::Win32::Foundation::HANDLE,
    buf: &mut [u8],
    timeout: Duration,
) -> io::Result<usize> {
    windows_io_handle_overlapped(handle, timeout, |overlapped, transferred| unsafe {
        windows_sys::Win32::Storage::FileSystem::ReadFile(
            handle,
            buf.as_mut_ptr().cast(),
            usize_to_u32_len(buf.len()),
            transferred,
            overlapped,
        )
    })
}

#[cfg(windows)]
fn windows_read_handle(
    handle: windows_sys::Win32::Foundation::HANDLE,
    buf: &mut [u8],
) -> io::Result<usize> {
    if buf.is_empty() {
        return Ok(0);
    }
    let mut read = 0u32;
    let ok = unsafe {
        windows_sys::Win32::Storage::FileSystem::ReadFile(
            handle,
            buf.as_mut_ptr().cast(),
            usize_to_u32_len(buf.len()),
            &mut read,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        let err = io::Error::last_os_error();
        if matches!(
            err.raw_os_error(),
            Some(code)
                if code == windows_sys::Win32::Foundation::ERROR_BROKEN_PIPE as i32
                    || code == windows_sys::Win32::Foundation::ERROR_PIPE_NOT_CONNECTED as i32
        ) {
            return Ok(0);
        }
        return Err(err);
    }
    Ok(read as usize)
}

#[cfg(windows)]
fn windows_write_handle(
    handle: windows_sys::Win32::Foundation::HANDLE,
    buf: &[u8],
) -> io::Result<usize> {
    if buf.is_empty() {
        return Ok(0);
    }
    let mut written = 0u32;
    let ok = unsafe {
        windows_sys::Win32::Storage::FileSystem::WriteFile(
            handle,
            buf.as_ptr().cast(),
            usize_to_u32_len(buf.len()),
            &mut written,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(written as usize)
}

#[cfg(windows)]
fn windows_write_handle_overlapped(
    handle: windows_sys::Win32::Foundation::HANDLE,
    buf: &[u8],
    timeout: Duration,
) -> io::Result<usize> {
    windows_io_handle_overlapped(handle, timeout, |overlapped, transferred| unsafe {
        windows_sys::Win32::Storage::FileSystem::WriteFile(
            handle,
            buf.as_ptr().cast(),
            usize_to_u32_len(buf.len()),
            transferred,
            overlapped,
        )
    })
}

#[cfg(windows)]
fn windows_io_handle_overlapped<F>(
    handle: windows_sys::Win32::Foundation::HANDLE,
    timeout: Duration,
    operation: F,
) -> io::Result<usize>
where
    F: FnOnce(
        *mut windows_sys::Win32::System::IO::OVERLAPPED,
        *mut u32,
    ) -> windows_sys::Win32::Foundation::BOOL,
{
    if timeout.is_zero() {
        return Err(io::Error::new(io::ErrorKind::TimedOut, "i/o timeout"));
    }
    let mut overlapped = WindowsOverlapped::new()?;
    let mut transferred = 0u32;
    let ok = operation(overlapped.as_mut_ptr(), &mut transferred);
    if ok != 0 {
        return Ok(transferred as usize);
    }
    let err = io::Error::last_os_error();
    if err.raw_os_error() != Some(windows_sys::Win32::Foundation::ERROR_IO_PENDING as i32) {
        if matches!(
            err.raw_os_error(),
            Some(code)
                if code == windows_sys::Win32::Foundation::ERROR_BROKEN_PIPE as i32
                    || code == windows_sys::Win32::Foundation::ERROR_PIPE_NOT_CONNECTED as i32
        ) {
            return Ok(0);
        }
        return Err(err);
    }
    let wait = unsafe {
        windows_sys::Win32::System::Threading::WaitForSingleObject(
            overlapped.event(),
            duration_millis_u32(timeout),
        )
    };
    match wait {
        windows_sys::Win32::Foundation::WAIT_OBJECT_0 => {
            let mut bytes = 0u32;
            let ok = unsafe {
                windows_sys::Win32::System::IO::GetOverlappedResult(
                    handle,
                    overlapped.as_mut_ptr(),
                    &mut bytes,
                    0,
                )
            };
            if ok == 0 {
                let err = io::Error::last_os_error();
                if matches!(
                    err.raw_os_error(),
                    Some(code)
                        if code == windows_sys::Win32::Foundation::ERROR_BROKEN_PIPE as i32
                            || code
                                == windows_sys::Win32::Foundation::ERROR_PIPE_NOT_CONNECTED as i32
                ) {
                    return Ok(0);
                }
                Err(err)
            } else {
                Ok(bytes as usize)
            }
        }
        windows_sys::Win32::Foundation::WAIT_TIMEOUT => {
            unsafe {
                windows_sys::Win32::System::IO::CancelIoEx(handle, overlapped.as_mut_ptr());
                let mut bytes = 0u32;
                let _ = windows_sys::Win32::System::IO::GetOverlappedResult(
                    handle,
                    overlapped.as_mut_ptr(),
                    &mut bytes,
                    1,
                );
            }
            Err(io::Error::new(io::ErrorKind::TimedOut, "i/o timeout"))
        }
        windows_sys::Win32::Foundation::WAIT_FAILED => Err(io::Error::last_os_error()),
        _ => Err(io::Error::new(
            io::ErrorKind::Other,
            "unexpected wait result for bridge pipe I/O",
        )),
    }
}

#[cfg(windows)]
fn windows_flush_handle(handle: windows_sys::Win32::Foundation::HANDLE) -> io::Result<()> {
    let ok = unsafe { windows_sys::Win32::Storage::FileSystem::FlushFileBuffers(handle) };
    if ok == 0 {
        let err = io::Error::last_os_error();
        if matches!(
            err.raw_os_error(),
            Some(code)
                if code == windows_sys::Win32::Foundation::ERROR_BROKEN_PIPE as i32
                    || code == windows_sys::Win32::Foundation::ERROR_PIPE_NOT_CONNECTED as i32
        ) {
            return Ok(());
        }
        return Err(err);
    }
    Ok(())
}

#[cfg(windows)]
#[derive(Debug)]
struct WindowsOverlapped {
    value: windows_sys::Win32::System::IO::OVERLAPPED,
}

#[cfg(windows)]
impl WindowsOverlapped {
    fn new() -> io::Result<Self> {
        let event = unsafe {
            windows_sys::Win32::System::Threading::CreateEventW(
                std::ptr::null(),
                1,
                0,
                std::ptr::null(),
            )
        };
        if event == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut value: windows_sys::Win32::System::IO::OVERLAPPED = unsafe { std::mem::zeroed() };
        value.hEvent = event;
        Ok(Self { value })
    }

    fn as_mut_ptr(&mut self) -> *mut windows_sys::Win32::System::IO::OVERLAPPED {
        &mut self.value
    }

    fn event(&self) -> windows_sys::Win32::Foundation::HANDLE {
        self.value.hEvent
    }
}

#[cfg(windows)]
impl Drop for WindowsOverlapped {
    fn drop(&mut self) {
        windows_close_handle(self.value.hEvent);
    }
}

#[cfg(windows)]
fn windows_deadline_remaining(deadline: Option<Instant>) -> Duration {
    deadline
        .and_then(|deadline| deadline.checked_duration_since(Instant::now()))
        .unwrap_or(Duration::ZERO)
}

#[cfg(windows)]
fn windows_close_handle(handle: windows_sys::Win32::Foundation::HANDLE) {
    if handle != 0 {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(handle);
        }
    }
}

#[cfg(windows)]
fn windows_wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn duration_millis_u32(timeout: Duration) -> u32 {
    u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX)
}

#[cfg(windows)]
fn usize_to_u32_len(len: usize) -> u32 {
    u32::try_from(len).unwrap_or(u32::MAX)
}

#[cfg(not(any(unix, windows)))]
fn unsupported_bridge_io() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "local websocket bridge I/O is not implemented on this platform in Rust port",
    )
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
