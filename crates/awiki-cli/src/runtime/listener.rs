use crate::config::Resolved;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

const EXPECTED_BOOT_ID_FILE_NAME: &str = "listener.expected-boot-id";
const SERVICE_NAME_PREFIX: &str = "awiki-cli-listener";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionStatus {
    pub identity_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub did: String,
    pub connected: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub last_error: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HostNotifyStatus {
    pub enabled: bool,
    pub sink: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub file_path: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub hook_url: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub agent_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub hook_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notify_url: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub last_error: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Status {
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub installed: bool,
    #[serde(default)]
    pub running: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub boot_id: String,
    #[serde(default, skip_serializing_if = "is_zero_i64")]
    pub pid: i64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub pid_file: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub socket_path: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub log_file: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub status_file: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub service_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub service_platform: String,
    #[serde(default)]
    pub bridge_available: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub started_at: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sessions: Vec<SessionStatus>,
    #[serde(default)]
    pub host_notify: HostNotifyStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

pub struct RuntimePaths {
    pub pid_file: String,
    pub log_file: String,
    pub status_file: String,
    pub socket_path: String,
}

pub fn paths(resolved: &Resolved) -> anyhow::Result<RuntimePaths> {
    let state_root = state_root(resolved);
    fs::create_dir_all(&state_root)
        .map_err(|err| anyhow::anyhow!("create runtime state dir: {err}"))?;
    let log_dir = log_root(resolved, &state_root);
    fs::create_dir_all(&log_dir).map_err(|err| anyhow::anyhow!("create runtime log dir: {err}"))?;
    Ok(RuntimePaths {
        pid_file: path_string(&state_root.join("listener.pid")),
        log_file: path_string(&log_dir.join("listener.log")),
        status_file: path_string(&state_root.join("listener.status.json")),
        socket_path: resolved.runtime_socket_path.clone(),
    })
}

pub fn boot_id_path(resolved: &Resolved) -> anyhow::Result<String> {
    let state_root = state_root(resolved);
    fs::create_dir_all(&state_root)
        .map_err(|err| anyhow::anyhow!("create runtime state dir: {err}"))?;
    Ok(path_string(&state_root.join(EXPECTED_BOOT_ID_FILE_NAME)))
}

pub fn write_pid(path: &str, pid: i64) -> anyhow::Result<()> {
    write_restricted_file(path, pid.to_string().as_bytes())
}

pub fn read_pid(path: &str) -> anyhow::Result<i64> {
    let raw = fs::read_to_string(path).map_err(|err| anyhow::anyhow!(err))?;
    raw.trim()
        .parse::<i64>()
        .map_err(|err| anyhow::anyhow!(err))
}

pub fn write_status(path: &str, status: &Status) -> anyhow::Result<()> {
    let raw = serde_json::to_vec_pretty(status)?;
    write_restricted_file(path, &raw)
}

pub fn read_status(path: &str) -> anyhow::Result<Status> {
    let raw = fs::read(path).map_err(|err| anyhow::anyhow!(err))?;
    serde_json::from_slice(&raw).map_err(|err| anyhow::anyhow!(err))
}

pub fn write_expected_boot_id(path: &str, boot_id: &str) -> anyhow::Result<()> {
    write_restricted_file(path, boot_id.trim().as_bytes())
}

pub fn read_expected_boot_id(path: &str) -> anyhow::Result<String> {
    let raw = fs::read_to_string(path).map_err(|err| anyhow::anyhow!(err))?;
    Ok(raw.trim().to_string())
}

pub fn status_for(resolved: &Resolved, installed: bool, running: bool) -> anyhow::Result<Status> {
    let runtime_paths = paths(resolved)?;
    let runtime = super::resolve(resolved);
    let mut status = Status {
        mode: runtime.mode.clone(),
        installed,
        running,
        pid_file: runtime_paths.pid_file,
        socket_path: runtime_paths.socket_path,
        log_file: runtime_paths.log_file,
        status_file: runtime_paths.status_file,
        service_name: service_name_for(resolved),
        service_platform: "rust-local".to_string(),
        host_notify: listener_host_notify_status(resolved),
        ..Status::default()
    };
    if let Ok(pid) = read_pid(&status.pid_file) {
        status.pid = pid;
    }
    if let Ok(saved) = read_status(&status.status_file) {
        merge_saved_runtime_status(&mut status, saved);
    }
    if runtime.listener.enabled && !status.installed {
        status
            .warnings
            .push("listener service is not installed".to_string());
    }
    if runtime.listener.enabled && !status.running {
        status
            .warnings
            .push("listener service is not running".to_string());
    }
    if runtime.listener.enabled {
        status.bridge_available = bridge_endpoint_available(&status.socket_path);
        if !status.bridge_available {
            status
                .warnings
                .push("listener socket is not available".to_string());
        }
    } else {
        status.bridge_available = false;
        status
            .warnings
            .push("listener is disabled by configuration".to_string());
    }
    status.warnings.extend(session_warnings(&status.sessions));
    Ok(status)
}

pub fn merge_saved_runtime_status(status: &mut Status, saved: Status) {
    if status.pid != 0 && saved.pid != 0 && saved.pid != status.pid {
        return;
    }
    status.started_at = saved.started_at;
    status.sessions = saved.sessions;
    if status.pid == 0 {
        status.pid = saved.pid;
    }
    status.boot_id = saved.boot_id;
    status.host_notify.last_error = saved.host_notify.last_error;
    if !status.running {
        return;
    }
    status.host_notify.enabled = saved.host_notify.enabled;
    let sink = saved.host_notify.sink.trim();
    if !sink.is_empty() {
        status.host_notify.sink = sink.to_string();
    }
    status.host_notify.file_path = saved.host_notify.file_path;
    let hook_url = saved.host_notify.hook_url.trim();
    if !hook_url.is_empty() {
        status.host_notify.hook_url = hook_url.to_string();
    }
}

pub fn session_warnings(sessions: &[SessionStatus]) -> Vec<String> {
    let mut warnings = Vec::new();
    for session in sessions {
        if session.connected {
            continue;
        }
        if !session.last_error.is_empty() {
            warnings.push(format!(
                "websocket session for identity {} is disconnected: {}",
                session.identity_name, session.last_error
            ));
            continue;
        }
        warnings.push(format!(
            "websocket session for identity {} is disconnected",
            session.identity_name
        ));
    }
    warnings
}

pub fn has_disconnected_sessions(sessions: &[SessionStatus]) -> bool {
    sessions.iter().any(|session| !session.connected)
}

pub fn service_name_for(resolved: &Resolved) -> String {
    let workspace = resolved.paths.workspace_home_dir.trim();
    if workspace.is_empty() {
        return SERVICE_NAME_PREFIX.to_string();
    }
    let digest = Sha256::digest(workspace.as_bytes());
    format!("{SERVICE_NAME_PREFIX}-{}", &format!("{digest:x}")[..12])
}

pub fn bridge_endpoint_available(path: &str) -> bool {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return false;
    }
    #[cfg(windows)]
    {
        trimmed.starts_with(r"\\.\pipe\")
    }
    #[cfg(not(windows))]
    {
        Path::new(trimmed).exists()
    }
}

pub fn to_value(status: Status) -> Value {
    serde_json::to_value(status).unwrap_or_else(|_| serde_json::json!({}))
}

fn listener_host_notify_status(resolved: &Resolved) -> HostNotifyStatus {
    let runtime = super::resolve(resolved);
    let settings = super::effective_openclaw_settings(resolved);
    HostNotifyStatus {
        enabled: runtime.host_notify.enabled,
        sink: runtime.host_notify.sink,
        file_path: runtime.host_notify.file_path,
        hook_url: settings.hook_url,
        agent_id: resolved.host_notify_openclaw_agent_id.clone(),
        hook_name: resolved.host_notify_openclaw_hook_name.clone(),
        notify_url: resolved.host_notify_hermes_notify_url.clone(),
        last_error: String::new(),
    }
}

fn state_root(resolved: &Resolved) -> PathBuf {
    let state_dir = resolved.paths.state_dir.trim();
    if state_dir.is_empty() {
        Path::new(&resolved.paths.workspace_home_dir).join("runtime")
    } else {
        PathBuf::from(state_dir)
    }
}

fn log_root(resolved: &Resolved, state_root: &Path) -> PathBuf {
    let logs_dir = resolved.paths.logs_dir.trim();
    if logs_dir.is_empty() {
        state_root.to_path_buf()
    } else {
        PathBuf::from(logs_dir)
    }
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn is_zero_i64(value: &i64) -> bool {
    *value == 0
}

fn write_restricted_file(path: &str, content: &[u8]) -> anyhow::Result<()> {
    fs::write(path, content).map_err(|err| anyhow::anyhow!(err))?;
    set_file_mode(Path::new(path), 0o600)
}

#[cfg(unix)]
fn set_file_mode(path: &Path, mode: u32) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(|err| anyhow::anyhow!(err))
}

#[cfg(not(unix))]
fn set_file_mode(_path: &Path, _mode: u32) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Paths;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn session_warnings_reports_disconnected_sessions() {
        let warnings = session_warnings(&[
            SessionStatus {
                identity_name: "alice".to_string(),
                connected: true,
                ..SessionStatus::default()
            },
            SessionStatus {
                identity_name: "bob".to_string(),
                connected: false,
                last_error: "refresh websocket session JWT: unauthorized".to_string(),
                ..SessionStatus::default()
            },
            SessionStatus {
                identity_name: "carol".to_string(),
                connected: false,
                ..SessionStatus::default()
            },
        ]);
        assert_eq!(warnings.len(), 2);
        assert_eq!(
            warnings[0],
            "websocket session for identity bob is disconnected: refresh websocket session JWT: unauthorized"
        );
        assert_eq!(
            warnings[1],
            "websocket session for identity carol is disconnected"
        );
    }

    #[test]
    fn has_disconnected_sessions_matches_go_contract() {
        assert!(!has_disconnected_sessions(&[SessionStatus {
            identity_name: "alice".to_string(),
            connected: true,
            ..SessionStatus::default()
        }]));
        assert!(has_disconnected_sessions(&[SessionStatus {
            identity_name: "bob".to_string(),
            connected: false,
            ..SessionStatus::default()
        }]));
    }

    #[test]
    fn merge_saved_runtime_status_prefers_running_host_notify_state() {
        let mut status = Status {
            running: true,
            pid: 30656,
            host_notify: HostNotifyStatus {
                enabled: true,
                sink: "openclaw".to_string(),
                hook_url: "http://127.0.0.1:18789/hooks/agent".to_string(),
                ..HostNotifyStatus::default()
            },
            ..Status::default()
        };
        let saved = Status {
            started_at: "2026-04-17T05:18:13Z".to_string(),
            pid: 30656,
            boot_id: "boot-new".to_string(),
            sessions: vec![SessionStatus {
                identity_name: "zhuocheng".to_string(),
                connected: true,
                ..SessionStatus::default()
            }],
            host_notify: HostNotifyStatus {
                enabled: true,
                sink: "log".to_string(),
                file_path: "/tmp/host-notify.events.jsonl".to_string(),
                hook_url: "http://127.0.0.1:9999/hooks/agent".to_string(),
                last_error: "sink boom".to_string(),
                ..HostNotifyStatus::default()
            },
            ..Status::default()
        };

        merge_saved_runtime_status(&mut status, saved);

        assert_eq!(status.started_at, "2026-04-17T05:18:13Z");
        assert_eq!(status.pid, 30656);
        assert_eq!(status.boot_id, "boot-new");
        assert_eq!(status.sessions.len(), 1);
        assert_eq!(status.sessions[0].identity_name, "zhuocheng");
        assert_eq!(status.host_notify.sink, "log");
        assert_eq!(
            status.host_notify.file_path,
            "/tmp/host-notify.events.jsonl"
        );
        assert_eq!(
            status.host_notify.hook_url,
            "http://127.0.0.1:9999/hooks/agent"
        );
        assert_eq!(status.host_notify.last_error, "sink boom");
    }

    #[test]
    fn merge_saved_runtime_status_keeps_configured_host_notify_when_not_running() {
        let mut status = Status {
            running: false,
            host_notify: HostNotifyStatus {
                enabled: true,
                sink: "openclaw".to_string(),
                hook_url: "http://127.0.0.1:18789/hooks/agent".to_string(),
                ..HostNotifyStatus::default()
            },
            ..Status::default()
        };
        let saved = Status {
            host_notify: HostNotifyStatus {
                enabled: true,
                sink: "log".to_string(),
                hook_url: "http://127.0.0.1:9999/hooks/agent".to_string(),
                ..HostNotifyStatus::default()
            },
            ..Status::default()
        };

        merge_saved_runtime_status(&mut status, saved);

        assert_eq!(status.host_notify.sink, "openclaw");
        assert_eq!(
            status.host_notify.hook_url,
            "http://127.0.0.1:18789/hooks/agent"
        );
    }

    #[test]
    fn merge_saved_runtime_status_skips_mismatched_pid() {
        let mut status = Status {
            running: true,
            pid: 200,
            host_notify: HostNotifyStatus {
                enabled: true,
                sink: "openclaw".to_string(),
                hook_url: "http://127.0.0.1:18789/hooks/agent".to_string(),
                ..HostNotifyStatus::default()
            },
            ..Status::default()
        };
        let saved = Status {
            pid: 100,
            boot_id: "boot-old".to_string(),
            host_notify: HostNotifyStatus {
                enabled: true,
                sink: "log".to_string(),
                hook_url: "http://127.0.0.1:9999/hooks/agent".to_string(),
                ..HostNotifyStatus::default()
            },
            ..Status::default()
        };

        merge_saved_runtime_status(&mut status, saved);

        assert_eq!(status.pid, 200);
        assert!(status.boot_id.is_empty());
        assert_eq!(status.host_notify.sink, "openclaw");
    }

    #[test]
    fn file_helpers_round_trip_status_pid_and_boot_id() {
        let resolved = test_resolved();
        let runtime_paths = paths(&resolved).expect("runtime paths");
        write_pid(&runtime_paths.pid_file, 42).expect("write pid");
        assert_eq!(read_pid(&runtime_paths.pid_file).expect("read pid"), 42);

        let status = Status {
            mode: "websocket".to_string(),
            pid: 42,
            started_at: "2026-04-17T05:18:13Z".to_string(),
            host_notify: HostNotifyStatus {
                enabled: true,
                sink: "log".to_string(),
                ..HostNotifyStatus::default()
            },
            ..Status::default()
        };
        write_status(&runtime_paths.status_file, &status).expect("write status");
        let loaded = read_status(&runtime_paths.status_file).expect("read status");
        assert_eq!(loaded.mode, "websocket");
        assert_eq!(loaded.pid, 42);

        let boot_path = boot_id_path(&resolved).expect("boot id path");
        write_expected_boot_id(&boot_path, " boot-new ").expect("write boot id");
        assert_eq!(
            read_expected_boot_id(&boot_path).expect("read boot id"),
            "boot-new"
        );
    }

    fn test_resolved() -> Resolved {
        let root = std::env::temp_dir().join(format!(
            "awiki-cli-rs2-listener-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let _ = fs::create_dir_all(&root);
        Resolved {
            paths: Paths {
                workspace_home_dir: path_string(&root),
                root_dir: path_string(&root),
                config_dir: path_string(&root),
                data_dir: path_string(&root.join("data")),
                state_dir: path_string(&root.join("runtime")),
                cache_dir: path_string(&root.join("cache")),
                logs_dir: path_string(&root.join("logs")),
                config_file: path_string(&root.join("config.yaml")),
                identity_dir: path_string(&root.join("identities")),
                database_file: path_string(&root.join("data").join("awiki.db")),
                legacy_credentials_dir: path_string(&root.join("credentials")),
                legacy_data_dir: path_string(&root.join("legacy-data")),
            },
            runtime_mode: "websocket".to_string(),
            runtime_socket_path: path_string(&root.join("runtime").join("message-daemon.sock")),
            runtime_listener_enabled: true,
            runtime_listener_auto_install: true,
            runtime_listener_auto_start: true,
            host_notify_enabled: true,
            host_notify_sink: "log".to_string(),
            config_schema_version: 0,
            active_identity: String::new(),
            host_notify_file_path: String::new(),
            host_notify_openclaw_hook_url: String::new(),
            host_notify_openclaw_agent_id: String::new(),
            host_notify_openclaw_hook_name: String::new(),
            host_notify_hermes_notify_url: String::new(),
            host_notify_hermes_deliver: String::new(),
            output_format: "json".to_string(),
            no_color: false,
            service_base_url: String::new(),
            did_domain: String::new(),
            anp_service_endpoint: String::new(),
            anp_service_did: String::new(),
            mail_service_url: String::new(),
            ca_bundle: String::new(),
            update_disable_strict_version: false,
            update_metadata_cache_ttl_seconds: 0,
            config_exists: false,
            config_error: String::new(),
            env_hits: Vec::new(),
            sources: Default::default(),
        }
    }
}
