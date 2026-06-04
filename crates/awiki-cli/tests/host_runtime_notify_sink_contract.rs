use awiki_cli::host_runtime;
use awiki_cli::host_runtime::host_notify::{HostNotificationData, HostNotificationEvent};
use awiki_cli::host_runtime::host_notify_sink::{
    new_file_host_notify_sink, new_host_notify_sink, HostNotifySink, HostNotifySinkImpl,
};
use awiki_cli::workspace_config::{Paths, Resolved, ValueSource};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

static CURRENT_DIR_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn file_host_notify_sink_rejects_blank_path_like_go() {
    let err = new_file_host_notify_sink("  ").expect_err("blank path error");
    assert_eq!(
        err.to_string(),
        "host notify file sink requires a file path"
    );
}

#[test]
fn file_host_notify_sink_creates_parent_and_appends_json_lines_like_go() {
    let workspace = TempDir::new().expect("temp workspace");
    let sink_path = workspace.path().join("nested").join("host-events.jsonl");
    std::fs::write(&sink_path, "existing\n").expect_err("parent missing");

    let sink = new_file_host_notify_sink(&path_string(&sink_path)).expect("sink");
    sink.notify(&host_event("msg-001")).expect("first notify");
    sink.notify(&host_event("msg-002")).expect("second notify");
    sink.close().expect("close");
    sink.close().expect("second close no-op");

    let raw = std::fs::read_to_string(&sink_path).expect("read sink file");
    let lines = raw.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 2);
    let first: serde_json::Value = serde_json::from_str(lines[0]).expect("first json");
    let second: serde_json::Value = serde_json::from_str(lines[1]).expect("second json");
    assert_eq!(first["id"], "msg-001");
    assert_eq!(first["topic"], "im.message.received");
    assert_eq!(second["id"], "msg-002");
    assert_eq!(raw.as_bytes().last(), Some(&b'\n'));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let dir_mode = std::fs::metadata(sink_path.parent().expect("parent"))
            .expect("dir metadata")
            .permissions()
            .mode()
            & 0o777;
        let file_mode = std::fs::metadata(&sink_path)
            .expect("file metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(dir_mode, 0o700);
        assert_eq!(file_mode, 0o600);
    }
}

#[test]
fn file_host_notify_sink_appends_to_existing_file_like_go() {
    let workspace = TempDir::new().expect("temp workspace");
    let sink_path = workspace.path().join("events.jsonl");
    std::fs::write(&sink_path, "existing\n").expect("seed file");

    let sink = new_file_host_notify_sink(&path_string(&sink_path)).expect("sink");
    sink.notify(&host_event("msg-003")).expect("notify");
    sink.close().expect("close");

    let raw = std::fs::read_to_string(&sink_path).expect("read sink file");
    let lines = raw.lines().collect::<Vec<_>>();
    assert_eq!(lines[0], "existing");
    assert_eq!(lines.len(), 2);
    let appended: serde_json::Value = serde_json::from_str(lines[1]).expect("json");
    assert_eq!(appended["id"], "msg-003");
}

#[test]
fn file_host_notify_sink_accepts_bare_relative_file_name_like_go() {
    let _guard = CURRENT_DIR_LOCK.lock().expect("current dir lock");
    let workspace = TempDir::new().expect("temp workspace");
    let previous_dir = std::env::current_dir().expect("current dir");
    std::env::set_current_dir(workspace.path()).expect("chdir workspace");
    let result = (|| {
        let sink = new_file_host_notify_sink("events.jsonl")?;
        sink.notify(&host_event("msg-relative"))?;
        sink.close()
    })();
    std::env::set_current_dir(previous_dir).expect("restore current dir");
    result.expect("relative file sink");

    let raw = std::fs::read_to_string(workspace.path().join("events.jsonl")).expect("read file");
    let payload: serde_json::Value = serde_json::from_str(raw.trim()).expect("json");
    assert_eq!(payload["id"], "msg-relative");
}

#[test]
fn new_host_notify_sink_disabled_returns_noop_and_status_like_go() {
    let workspace = TempDir::new().expect("temp workspace");
    let mut resolved = test_resolved(workspace.path());
    resolved.host_notify_enabled = false;
    resolved.host_notify_sink = "file".to_string();
    resolved.host_notify_file_path = path_string(&workspace.path().join("events.jsonl"));

    let (sink, status) = new_host_notify_sink(&resolved).expect("sink");
    assert!(matches!(sink, HostNotifySinkImpl::Noop(_)));
    assert!(!status.enabled);
    assert_eq!(status.sink, "file");
    assert_eq!(status.file_path, resolved.host_notify_file_path);
    sink.notify(&host_event("disabled")).expect("noop notify");
    sink.close().expect("noop close");
    assert!(!std::path::Path::new(&resolved.host_notify_file_path).exists());
}

#[test]
fn new_host_notify_sink_file_returns_status_and_writes_like_go() {
    let workspace = TempDir::new().expect("temp workspace");
    let sink_path = workspace.path().join("runtime").join("events.jsonl");
    let mut resolved = test_resolved(workspace.path());
    resolved.host_notify_sink = "file".to_string();
    resolved.host_notify_file_path = path_string(&sink_path);

    let (sink, status) = new_host_notify_sink(&resolved).expect("sink");
    assert!(matches!(sink, HostNotifySinkImpl::File(_)));
    assert!(status.enabled);
    assert_eq!(status.sink, "file");
    assert_eq!(status.file_path, path_string(&sink_path));
    assert_eq!(status.hook_url, "");
    assert_eq!(status.notify_url, "");

    sink.notify(&host_event("msg-file")).expect("notify");
    sink.close().expect("close");
    let raw = std::fs::read_to_string(&sink_path).expect("read sink file");
    let payload: serde_json::Value = serde_json::from_str(raw.trim()).expect("json line");
    assert_eq!(payload["id"], "msg-file");
}

#[test]
fn new_host_notify_sink_log_noop_and_unsupported_match_go_dispatch() {
    let workspace = TempDir::new().expect("temp workspace");
    let mut resolved = test_resolved(workspace.path());

    resolved.host_notify_sink = String::new();
    let (sink, status) = new_host_notify_sink(&resolved).expect("default log");
    assert!(matches!(sink, HostNotifySinkImpl::Log(_)));
    assert_eq!(status.sink, "log");

    resolved.host_notify_sink = "noop".to_string();
    let (sink, status) = new_host_notify_sink(&resolved).expect("noop");
    assert!(matches!(sink, HostNotifySinkImpl::Noop(_)));
    assert_eq!(status.sink, "noop");

    resolved.host_notify_sink = "log".to_string();
    let (sink, status) = new_host_notify_sink(&resolved).expect("log");
    assert!(matches!(sink, HostNotifySinkImpl::Log(_)));
    assert_eq!(status.sink, "log");

    resolved.host_notify_sink = "unknown".to_string();
    let err = new_host_notify_sink(&resolved).expect_err("unsupported");
    assert_eq!(err.to_string(), "unsupported host notify sink \"unknown\"");
}

#[test]
fn new_host_notify_sink_normalizes_webhook_alias_to_hermes_like_go() {
    let workspace = TempDir::new().expect("temp workspace");
    let mut resolved = test_resolved(workspace.path());
    resolved.host_notify_sink = "webhook".to_string();
    resolved.host_notify_hermes_notify_url = "http://127.0.0.1:8765/notify/host-event".to_string();

    let err = new_host_notify_sink(&resolved).expect_err("missing secret");
    assert!(err
        .to_string()
        .contains("hermes host notify requires runtime.host_notify.hermes.secret"));
    let status = host_runtime::listener_status(&resolved, false, false);
    assert_eq!(status["host_notify"]["sink"], "hermes");
}

#[test]
fn new_host_notify_sink_hermes_status_and_constructor_errors_match_go() {
    let workspace = TempDir::new().expect("temp workspace");
    let mut resolved = test_resolved(workspace.path());
    resolved.host_notify_sink = "hermes".to_string();
    resolved.host_notify_hermes_notify_url = "http://127.0.0.1:8765/notify/host-event".to_string();
    resolved.host_notify_hermes_deliver = "feishu".to_string();

    let err = new_host_notify_sink(&resolved).expect_err("missing secret");
    assert_eq!(
        err.to_string(),
        "hermes host notify requires runtime.host_notify.hermes.secret or AWIKI_HOST_NOTIFY_HERMES_SECRET (legacy: AWIKI_HOST_NOTIFY_WEBHOOK_SECRET)"
    );
    let status = host_runtime::listener_status(&resolved, false, false);
    assert_eq!(
        status["host_notify"]["notify_url"],
        "http://127.0.0.1:8765/notify/host-event"
    );
}

#[test]
fn new_host_notify_sink_openclaw_returns_delivery_sink_and_status_like_go() {
    let workspace = TempDir::new().expect("temp workspace");
    let mut resolved = test_resolved(workspace.path());
    resolved.host_notify_sink = "openclaw".to_string();
    resolved.host_notify_openclaw_hook_url = "http://127.0.0.1:18789/hooks/agent".to_string();
    resolved.host_notify_openclaw_agent_id = "agent-1".to_string();
    resolved.host_notify_openclaw_hook_name = "Hook Name".to_string();
    resolved.sources.insert(
        "host_notify_openclaw_hook_url".to_string(),
        ValueSource {
            source: "config_file".to_string(),
            key: String::new(),
            value: resolved.host_notify_openclaw_hook_url.clone(),
        },
    );

    let (sink, status) = new_host_notify_sink(&resolved).expect("openclaw sink");
    assert!(matches!(sink, HostNotifySinkImpl::OpenClaw(_)));
    assert_eq!(status.hook_url, "http://127.0.0.1:18789/hooks/agent");
    assert_eq!(status.agent_id, "agent-1");
    assert_eq!(status.hook_name, "Hook Name");
}

fn host_event(id: &str) -> HostNotificationEvent {
    HostNotificationEvent {
        version: "1.0".to_string(),
        id: id.to_string(),
        topic: "im.message.received".to_string(),
        received_at: "2026-04-12T10:30:00Z".to_string(),
        data: Some(HostNotificationData::Direct(Default::default())),
    }
}

fn test_resolved(root: &std::path::Path) -> Resolved {
    Resolved {
        paths: Paths {
            workspace_home_dir: path_string(root),
            root_dir: path_string(root),
            config_dir: path_string(root),
            data_dir: path_string(&root.join("data")),
            state_dir: path_string(&root.join("runtime")),
            cache_dir: path_string(&root.join("cache")),
            logs_dir: path_string(&root.join("logs")),
            config_file: path_string(&root.join("config.yaml")),
            identity_dir: path_string(&root.join("identities")),
            database_file: path_string(&root.join("data").join("awiki-cli.db")),
            legacy_credentials_dir: path_string(&root.join("credentials")),
            legacy_data_dir: path_string(&root.join("legacy-data")),
        },
        config_schema_version: 1,
        active_identity: String::new(),
        runtime_mode: "websocket".to_string(),
        runtime_socket_path: String::new(),
        runtime_listener_enabled: true,
        runtime_listener_auto_install: true,
        runtime_listener_auto_start: true,
        host_notify_enabled: true,
        host_notify_sink: "log".to_string(),
        host_notify_file_path: String::new(),
        host_notify_openclaw_hook_url: String::new(),
        host_notify_openclaw_agent_id: String::new(),
        host_notify_openclaw_hook_name: String::new(),
        host_notify_hermes_notify_url: String::new(),
        host_notify_hermes_deliver: String::new(),
        output_format: "json".to_string(),
        no_color: false,
        service_base_url: "https://awiki.ai".to_string(),
        did_domain: "awiki.ai".to_string(),
        anp_service_endpoint: "https://awiki.ai/anp-im/rpc".to_string(),
        anp_service_did: "did:wba:awiki.ai".to_string(),
        mail_service_url: "https://awiki.ai".to_string(),
        ca_bundle: String::new(),
        update_disable_strict_version: false,
        update_metadata_cache_ttl_seconds: 0,
        config_exists: false,
        config_error: String::new(),
        env_hits: Vec::new(),
        sources: BTreeMap::<String, ValueSource>::new(),
    }
}

fn path_string(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}

struct TempDir {
    path: std::path::PathBuf,
}

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let thread_id = format!("{:?}", std::thread::current().id())
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric())
            .collect::<String>();
        let path = std::env::temp_dir().join(format!(
            "awiki-cli-rs2-host-notify-sink-test-{}-{nonce}-{thread_id}-{counter}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
