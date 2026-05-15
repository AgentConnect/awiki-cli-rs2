use awiki_cli::config::{self, Overrides, Paths, Resolved, ValueSource};
use awiki_cli::runtime;
use awiki_cli::runtime::hermes_host_notify;
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn build_hermes_notify_signature_matches_go_hmac_contract() {
    let body = br#"{"data":{"recipient_did":"did:wba:test:bob","sender_did":"did:wba:test:alice"},"id":"msg-001","received_at":"2026-04-12T10:30:00Z","topic":"im.message.received","version":"1.0"}"#;
    let timestamp = "1776508200";

    assert_eq!(
        hermes_host_notify::build_hermes_notify_signature(body, timestamp, "test-secret"),
        "46960b21fb7917d276a23f172e327de4012b479c9c5a2699b1e6edbf562056ee"
    );
    assert_eq!(
        hermes_host_notify::build_hermes_notify_signature_header(body, timestamp, "test-secret"),
        "sha256=46960b21fb7917d276a23f172e327de4012b479c9c5a2699b1e6edbf562056ee"
    );
    assert_eq!(
        hermes_host_notify::NOTIFY_TIMESTAMP_HEADER,
        "X-Notify-Timestamp"
    );
    assert_eq!(
        hermes_host_notify::NOTIFY_SIGNATURE_HEADER,
        "X-Notify-Signature"
    );
}

#[test]
fn validate_hermes_notify_url_matches_go_scheme_and_host_rules() {
    hermes_host_notify::validate_hermes_notify_url("http://example.com").expect("http");
    hermes_host_notify::validate_hermes_notify_url("https://example.com/notify/host-event")
        .expect("https");
    hermes_host_notify::validate_hermes_notify_url("HTTP://example.com/notify")
        .expect("Go net/url normalizes uppercase schemes");
    hermes_host_notify::validate_hermes_notify_url("http://10.0.0.5:8765/notify")
        .expect("remote hosts are allowed for Hermes");
    hermes_host_notify::validate_hermes_notify_url("http://[::1]:8765/notify").expect("ipv6");
    hermes_host_notify::validate_hermes_notify_url("http://example.com:/notify")
        .expect("Go allows empty port");
    hermes_host_notify::validate_hermes_notify_url("http://example.com:65536/notify")
        .expect("Go parses high numeric ports");

    let scheme = hermes_host_notify::validate_hermes_notify_url("ftp://example.com/notify")
        .expect_err("ftp rejected");
    assert!(scheme.to_string().contains("must use http or https"));

    let missing_host =
        hermes_host_notify::validate_hermes_notify_url("https:///notify").expect_err("host");
    assert!(missing_host.to_string().contains("must include a host"));

    for raw_url in [
        "http://[::1",
        "http://example.com:bad",
        "http:// ex.com",
        "http://example.com\\path",
    ] {
        let err = hermes_host_notify::validate_hermes_notify_url(raw_url)
            .expect_err("parse boundary rejected");
        assert!(
            err.to_string()
                .contains("parse runtime.host_notify.hermes.notify_url"),
            "{raw_url}: {err}"
        );
    }
}

#[test]
fn resolve_hermes_notify_secret_prefers_config_then_env_like_go() {
    let _env_lock = ENV_LOCK.lock().expect("env lock");
    let workspace = TempDir::new().expect("temp workspace");
    let config_path = workspace.path().join("config.yaml");
    std::fs::write(
        &config_path,
        "runtime:\n  host_notify:\n    hermes:\n      secret: hermes-config\n    webhook:\n      secret: legacy-config\n",
    )
    .expect("write config");
    let resolved = resolved_for_config(&config_path);
    EnvGuard::remove(hermes_host_notify::HERMES_NOTIFY_SECRET_ENV);
    EnvGuard::remove(hermes_host_notify::LEGACY_WEBHOOK_NOTIFY_SECRET_ENV);
    let _new_env = EnvGuard::set(hermes_host_notify::HERMES_NOTIFY_SECRET_ENV, "hermes-env");
    let _legacy_env = EnvGuard::set(
        hermes_host_notify::LEGACY_WEBHOOK_NOTIFY_SECRET_ENV,
        "legacy-env",
    );

    assert_eq!(
        hermes_host_notify::resolve_hermes_notify_secret(Some(&resolved)),
        "hermes-config"
    );

    std::fs::write(
        &config_path,
        "runtime:\n  host_notify:\n    hermes:\n      secret: '   '\n    webhook:\n      secret: legacy-config\n",
    )
    .expect("write legacy config");
    assert_eq!(
        hermes_host_notify::resolve_hermes_notify_secret(Some(&resolved)),
        "legacy-config"
    );
}

#[test]
fn resolve_hermes_notify_secret_falls_back_to_new_then_legacy_env() {
    let _env_lock = ENV_LOCK.lock().expect("env lock");
    let _new_removed = EnvGuard::remove(hermes_host_notify::HERMES_NOTIFY_SECRET_ENV);
    let _legacy_removed = EnvGuard::remove(hermes_host_notify::LEGACY_WEBHOOK_NOTIFY_SECRET_ENV);
    let new_env = EnvGuard::set(
        hermes_host_notify::HERMES_NOTIFY_SECRET_ENV,
        "  hermes-env  ",
    );
    let legacy_env = EnvGuard::set(
        hermes_host_notify::LEGACY_WEBHOOK_NOTIFY_SECRET_ENV,
        "legacy-env",
    );

    assert_eq!(
        hermes_host_notify::resolve_hermes_notify_secret(None),
        "hermes-env"
    );

    drop(new_env);
    assert_eq!(
        hermes_host_notify::resolve_hermes_notify_secret(None),
        "legacy-env"
    );

    drop(legacy_env);
    assert_eq!(hermes_host_notify::resolve_hermes_notify_secret(None), "");
}

#[test]
fn host_notify_config_view_uses_go_legacy_secret_env_name() {
    let _env_lock = ENV_LOCK.lock().expect("env lock");
    let workspace = TempDir::new().expect("temp workspace");
    let _home = EnvGuard::set("HOME", workspace.path().to_str().unwrap_or_default());
    let _workspace_home = EnvGuard::set(
        "AWIKI_CLI_WORKSPACE_HOME_DIR",
        workspace.path().to_str().unwrap_or_default(),
    );
    let resolved = config::resolve(Overrides::default()).expect("resolve config");
    let view = runtime::host_notify_config_view(&resolved).expect("host notify view");

    assert_eq!(
        view["hermes"]["secret_env_fallback"],
        hermes_host_notify::HERMES_NOTIFY_SECRET_ENV
    );
    assert_eq!(
        view["hermes"]["secret_env_legacy"],
        hermes_host_notify::LEGACY_WEBHOOK_NOTIFY_SECRET_ENV
    );
}

fn resolved_for_config(path: &std::path::Path) -> Resolved {
    Resolved {
        paths: Paths {
            workspace_home_dir: String::new(),
            root_dir: String::new(),
            config_dir: String::new(),
            data_dir: String::new(),
            state_dir: String::new(),
            cache_dir: String::new(),
            logs_dir: String::new(),
            config_file: path.to_string_lossy().into_owned(),
            identity_dir: String::new(),
            database_file: String::new(),
            legacy_credentials_dir: String::new(),
            legacy_data_dir: String::new(),
        },
        config_schema_version: 1,
        active_identity: String::new(),
        runtime_mode: String::new(),
        runtime_socket_path: String::new(),
        runtime_listener_enabled: true,
        runtime_listener_auto_install: true,
        runtime_listener_auto_start: true,
        host_notify_enabled: true,
        host_notify_sink: "hermes".to_string(),
        host_notify_file_path: String::new(),
        host_notify_openclaw_hook_url: String::new(),
        host_notify_openclaw_agent_id: String::new(),
        host_notify_openclaw_hook_name: String::new(),
        host_notify_hermes_notify_url: String::new(),
        host_notify_hermes_deliver: String::new(),
        output_format: String::new(),
        no_color: false,
        service_base_url: String::new(),
        did_domain: String::new(),
        anp_service_endpoint: String::new(),
        anp_service_did: String::new(),
        mail_service_url: String::new(),
        ca_bundle: String::new(),
        update_disable_strict_version: false,
        update_metadata_cache_ttl_seconds: 0,
        config_exists: true,
        config_error: String::new(),
        env_hits: Vec::new(),
        sources: BTreeMap::<String, ValueSource>::new(),
    }
}

struct EnvGuard {
    key: &'static str,
    original: Option<String>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let original = std::env::var(key).ok();
        std::env::set_var(key, value);
        Self { key, original }
    }

    fn remove(key: &'static str) -> Self {
        let original = std::env::var(key).ok();
        std::env::remove_var(key);
        Self { key, original }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(value) = self.original.as_ref() {
            std::env::set_var(self.key, value);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

struct TempDir {
    path: std::path::PathBuf,
}

impl TempDir {
    fn new() -> std::io::Result<Self> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "awiki-cli-rs2-hermes-host-notify-test-{}-{nonce}",
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
