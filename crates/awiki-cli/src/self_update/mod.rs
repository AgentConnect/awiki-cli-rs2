mod cache;
mod version;

use crate::build_info;
use crate::workspace_config::{self, Resolved};
use std::path::PathBuf;

const DEFAULT_METADATA_CACHE_TTL_SECONDS: i64 = 43_200;

#[cfg(test)]
static TEST_NPM_LATEST_URLS: std::sync::Mutex<Option<Vec<String>>> = std::sync::Mutex::new(None);
#[cfg(test)]
static TEST_NPM_LATEST_URLS_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
#[cfg(test)]
static TEST_CURRENT_VERSION: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Metadata {
    pub latest_version: String,
    pub min_supported_version: String,
    pub installer_url: String,
    pub source: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Decision {
    pub current_version: String,
    pub latest_version: String,
    pub min_supported_version: String,
    pub installer_url: String,
    pub metadata_source: String,
    pub strict_disabled: bool,
    pub dev_build: bool,
    pub has_newer_version: bool,
    pub blocked: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CheckOutcome {
    pub decision: Decision,
    pub error: Option<String>,
}

pub fn check(resolved: &Resolved) -> CheckOutcome {
    check_with_settings(check_settings_from_resolved(resolved), false)
}

pub fn check_fresh(resolved: &Resolved) -> CheckOutcome {
    check_with_settings(check_settings_from_resolved(resolved), true)
}

pub fn check_preflight() -> CheckOutcome {
    check_with_settings(CheckSettings::preflight(), false)
}

#[derive(Debug, Clone, Default)]
struct CheckSettings {
    strict_disabled: bool,
    metadata_cache_ttl_seconds: i64,
    cache_dir: Option<PathBuf>,
}

impl CheckSettings {
    fn preflight() -> Self {
        Self {
            strict_disabled: false,
            metadata_cache_ttl_seconds: 0,
            cache_dir: workspace_config::product_cache_dir()
                .ok()
                .and_then(|path| non_empty_path(&path)),
        }
    }
}

fn check_settings_from_resolved(resolved: &Resolved) -> CheckSettings {
    CheckSettings {
        strict_disabled: resolved.update_disable_strict_version,
        metadata_cache_ttl_seconds: resolved.update_metadata_cache_ttl_seconds,
        cache_dir: update_cache_dir(resolved),
    }
}

fn check_with_settings(settings: CheckSettings, prefer_fresh: bool) -> CheckOutcome {
    let current_version = current_version();
    let dev_build = version::is_dev_version(&current_version);
    let strict_disabled = strict_disabled(settings.strict_disabled);
    let ttl_seconds = metadata_cache_ttl_seconds(settings.metadata_cache_ttl_seconds);
    let mut decision = Decision {
        current_version,
        strict_disabled,
        dev_build,
        ..Decision::default()
    };

    let urls = manifest_urls();
    decision.installer_url =
        installer_url_from_manifest_url(urls.first().map(String::as_str).unwrap_or_default());
    let mut metadata = match cache::load_metadata(
        settings.cache_dir.as_deref(),
        ttl_seconds,
        prefer_fresh,
        update_cache_only_enabled(),
        &urls,
    ) {
        Ok(metadata) => metadata,
        Err(err) => {
            return CheckOutcome {
                decision,
                error: Some(err),
            };
        }
    };
    if metadata.installer_url.trim().is_empty() {
        metadata.installer_url = decision.installer_url.clone();
    }

    decision.latest_version = metadata.latest_version;
    decision.min_supported_version = metadata.min_supported_version;
    decision.installer_url = metadata.installer_url;
    decision.metadata_source = metadata.source;

    if version::compare_versions(&decision.latest_version, &decision.current_version)
        .is_some_and(|ordering| ordering > 0)
    {
        decision.has_newer_version = true;
    }

    if !decision.dev_build
        && !decision.strict_disabled
        && version::compare_versions(&decision.current_version, &decision.min_supported_version)
            .is_some_and(|ordering| ordering < 0)
    {
        decision.blocked = true;
    }

    CheckOutcome {
        decision,
        error: None,
    }
}

fn current_version() -> String {
    #[cfg(test)]
    {
        if let Some(version) = TEST_CURRENT_VERSION
            .lock()
            .expect("test current version mutex")
            .clone()
        {
            return version;
        }
    }
    let current = build_info::VERSION.trim();
    if current.is_empty() {
        "dev".to_string()
    } else {
        current.to_string()
    }
}

fn strict_disabled(configured: bool) -> bool {
    let mut disabled = configured;
    if let Ok(raw) = std::env::var("AWIKI_CLI_DISABLE_STRICT_VERSION") {
        if !raw.trim().is_empty() {
            disabled = parse_bool(&raw);
        }
    }
    disabled
}

fn metadata_cache_ttl_seconds(configured: i64) -> i64 {
    let mut ttl = if configured > 0 {
        configured
    } else {
        DEFAULT_METADATA_CACHE_TTL_SECONDS
    };
    if let Ok(raw) = std::env::var("AWIKI_CLI_UPDATE_CACHE_TTL") {
        if let Ok(parsed) = raw.trim().parse::<i64>() {
            if parsed > 0 {
                ttl = parsed;
            }
        }
    }
    ttl
}

fn update_cache_dir(resolved: &Resolved) -> Option<PathBuf> {
    if std::env::var("AWIKI_CLI_WORKSPACE_HOME_DIR")
        .ok()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
    {
        return workspace_config::product_cache_dir()
            .ok()
            .and_then(|path| non_empty_path(&path))
            .or_else(|| non_empty_path(&resolved.paths.cache_dir));
    }
    non_empty_path(&resolved.paths.cache_dir)
}

fn non_empty_path(value: &str) -> Option<PathBuf> {
    (!value.trim().is_empty()).then(|| PathBuf::from(value))
}

fn update_cache_only_enabled() -> bool {
    std::env::var("AWIKI_CLI_UPDATE_CACHE_ONLY")
        .map(|value| parse_bool(&value))
        .unwrap_or(false)
}

fn parse_bool(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn manifest_urls() -> Vec<String> {
    #[cfg(test)]
    {
        if let Some(urls) = TEST_NPM_LATEST_URLS
            .lock()
            .expect("test urls mutex")
            .clone()
        {
            return urls;
        }
    }
    let configured = std::env::var("AWIKI_CLI_UPDATE_BASE_URL").unwrap_or_default();
    let base = if configured.trim().is_empty() {
        "https://awiki.ai/cli/stable".to_string()
    } else {
        configured.trim().trim_end_matches('/').to_string()
    };
    if base.ends_with(".json") {
        vec![base]
    } else {
        vec![format!("{base}/manifest.json")]
    }
}

fn installer_url_from_manifest_url(manifest_url: &str) -> String {
    let trimmed = manifest_url.trim();
    let base = trimmed
        .strip_suffix("/manifest.json")
        .or_else(|| trimmed.rsplit_once('/').map(|(base, _)| base))
        .unwrap_or(trimmed)
        .trim_end_matches('/');
    if base.is_empty() {
        String::new()
    } else {
        format!("{base}/awiki-cli.tgz")
    }
}

#[cfg(test)]
mod tests {
    use super::{version, TEST_CURRENT_VERSION, TEST_NPM_LATEST_URLS, TEST_NPM_LATEST_URLS_LOCK};
    use crate::workspace_config::{Paths, Resolved};
    use std::fs;
    use std::io::{BufRead, BufReader, Write};
    use std::net::{TcpListener, TcpStream};
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
    use std::sync::{MutexGuard, PoisonError};
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn compare_versions_handles_numeric_prerelease_segments() {
        assert!(version::compare_versions("0.0.1-beta.9", "0.0.1-beta.10").unwrap() < 0);
        assert!(version::compare_versions("0.0.1-beta.10", "0.0.1-beta.9").unwrap() > 0);
    }

    #[test]
    fn dev_versions_match_go_policy() {
        assert!(version::is_dev_version(""));
        assert!(version::is_dev_version("dev"));
        assert!(version::is_dev_version("1.0.0-dev.1"));
        assert!(version::is_dev_version("0.0.0-local"));
        assert!(!version::is_dev_version("1.0.0"));
    }

    #[test]
    fn check_fresh_prefers_network_over_fresh_cache_and_writes_cache() {
        let server = TestServer::new(vec![TestResponse::ok(
            r#"{"version":"1.0.10","awikiCli":{"minSupportedVersion":"1.0.9"}}"#,
        )]);
        let _urls = TestUrls::set(vec![server.url("/latest")]);
        let temp = TempDir::new();
        seed_metadata(temp.path(), "1.0.9", "1.0.9", "");

        let outcome = super::check_fresh(&resolved(temp.path()));

        assert_eq!(outcome.error, None);
        assert_eq!(outcome.decision.latest_version, "1.0.10");
        assert_eq!(outcome.decision.min_supported_version, "1.0.9");
        assert_eq!(outcome.decision.metadata_source, "network");
        let cache = fs::read_to_string(metadata_path(temp.path())).expect("cache write");
        assert!(
            cache.contains("\"latest_version\": \"1.0.10\""),
            "cache = {cache}"
        );
        assert!(cache.contains("\"source\": \"network\""), "cache = {cache}");
        assert_eq!(server.paths(), vec!["/latest"]);
        assert_cache_permissions(temp.path());
    }

    #[test]
    fn manifest_fetch_falls_back_to_secondary_url() {
        let server = TestServer::new(vec![
            TestResponse::status(503, "unavailable"),
            TestResponse::ok(r#"{"version":"1.0.9","awikiCli":{"minSupportedVersion":"1.0.8"}}"#),
        ]);
        let _urls = TestUrls::set(vec![server.url("/primary"), server.url("/secondary")]);
        let temp = TempDir::new();

        let outcome = super::check_fresh(&resolved(temp.path()));

        assert_eq!(outcome.error, None);
        assert_eq!(outcome.decision.latest_version, "1.0.9");
        assert_eq!(outcome.decision.min_supported_version, "1.0.8");
        assert_eq!(outcome.decision.metadata_source, "network");
        assert_eq!(server.paths(), vec!["/primary", "/secondary"]);
    }

    #[test]
    fn manifest_fetch_returns_combined_error_and_keeps_installer_fallback() {
        let server = TestServer::new(vec![
            TestResponse::status(503, "unavailable"),
            TestResponse::status(502, "bad gateway"),
        ]);
        let _urls = TestUrls::set(vec![server.url("/primary"), server.url("/secondary")]);
        let temp = TempDir::new();

        let outcome = super::check_fresh(&resolved(temp.path()));

        let error = outcome.error.expect("combined error");
        assert!(
            error.contains(&server.url("/primary")),
            "error should include primary manifest URL: {error}"
        );
        assert!(
            error.contains(&server.url("/secondary")),
            "error should include secondary manifest URL: {error}"
        );
        assert_eq!(outcome.decision.installer_url, server.url("/awiki-cli.tgz"));
    }

    #[test]
    fn registry_fetch_preserves_go_default_proxy_env_behavior() {
        let proxy = TestServer::new(vec![TestResponse::ok(
            r#"{"version":"1.0.11","awikiCli":{"minSupportedVersion":"1.0.9"}}"#,
        )]);
        let _urls = TestUrls::set(vec!["http://registry.example/latest".to_string()]);
        let _proxy = EnvVar::set("HTTP_PROXY", &proxy.url(""));
        let temp = TempDir::new();

        let outcome = super::check_fresh(&resolved(temp.path()));

        assert_eq!(outcome.error, None);
        assert_eq!(outcome.decision.latest_version, "1.0.11");
        assert_eq!(
            proxy.paths(),
            vec!["http://registry.example/latest".to_string()]
        );
    }

    #[test]
    fn check_fresh_falls_back_to_stale_cache_when_network_fails() {
        let server = TestServer::new(vec![TestResponse::status(503, "unavailable")]);
        let _urls = TestUrls::set(vec![server.url("/latest")]);
        let temp = TempDir::new();
        seed_metadata(temp.path(), "1.0.10", "1.0.9", "");

        let outcome = super::check_fresh(&resolved(temp.path()));

        assert_eq!(outcome.error, None);
        assert_eq!(outcome.decision.latest_version, "1.0.10");
        assert_eq!(outcome.decision.min_supported_version, "1.0.9");
        assert_eq!(outcome.decision.metadata_source, "cache_stale");
    }

    #[test]
    fn cache_only_uses_cached_metadata_without_network() {
        let server = TestServer::new(vec![TestResponse::status(500, "should not be used")]);
        let _urls = TestUrls::set(vec![server.url("/latest")]);
        let _env = EnvVar::set("AWIKI_CLI_UPDATE_CACHE_ONLY", "1");
        let temp = TempDir::new();
        seed_metadata(temp.path(), "1.0.10", "1.0.9", "");

        let outcome = super::check_fresh(&resolved(temp.path()));

        assert_eq!(outcome.error, None);
        assert_eq!(outcome.decision.latest_version, "1.0.10");
        assert_eq!(outcome.decision.metadata_source, "cache");
        assert_eq!(server.paths(), Vec::<String>::new());
    }

    #[test]
    fn check_blocks_non_dev_version_below_minimum_supported() {
        let _version = TestCurrentVersion::set("1.0.0");
        let _env = EnvVar::set("AWIKI_CLI_UPDATE_CACHE_ONLY", "1");
        let temp = TempDir::new();
        seed_metadata(temp.path(), "1.0.2", "1.0.1", "");

        let outcome = super::check(&resolved(temp.path()));

        assert_eq!(outcome.error, None);
        assert_eq!(outcome.decision.current_version, "1.0.0");
        assert_eq!(outcome.decision.latest_version, "1.0.2");
        assert_eq!(outcome.decision.min_supported_version, "1.0.1");
        assert_eq!(outcome.decision.metadata_source, "cache");
        assert_eq!(outcome.decision.dev_build, false);
        assert_eq!(outcome.decision.strict_disabled, false);
        assert_eq!(outcome.decision.has_newer_version, true);
        assert_eq!(outcome.decision.blocked, true);
    }

    #[test]
    fn check_strict_disable_suppresses_blocking() {
        let _version = TestCurrentVersion::set("1.0.0");
        let _env = EnvVar::set("AWIKI_CLI_UPDATE_CACHE_ONLY", "1");
        let _strict = EnvVar::set("AWIKI_CLI_DISABLE_STRICT_VERSION", "1");
        let temp = TempDir::new();
        seed_metadata(temp.path(), "1.0.2", "1.0.1", "");

        let outcome = super::check(&resolved(temp.path()));

        assert_eq!(outcome.error, None);
        assert_eq!(outcome.decision.strict_disabled, true);
        assert_eq!(outcome.decision.blocked, false);
        assert_eq!(outcome.decision.has_newer_version, true);
    }

    fn resolved(root: &Path) -> Resolved {
        let paths = test_paths(root);
        Resolved {
            paths,
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
            user_service_endpoint: "https://awiki.ai".to_string(),
            message_service_endpoint: "https://awiki.ai".to_string(),
            did_domain: "awiki.ai".to_string(),
            anp_service_endpoint: "https://awiki.ai/anp-im/rpc".to_string(),
            anp_service_did: String::new(),
            mail_service_url: "https://awiki.ai".to_string(),
            ca_bundle: String::new(),
            update_disable_strict_version: false,
            update_metadata_cache_ttl_seconds: 43_200,
            config_exists: false,
            config_error: String::new(),
            env_hits: Vec::new(),
            sources: std::collections::BTreeMap::new(),
        }
    }

    fn test_paths(root: &Path) -> Paths {
        let data_dir = root.join("data");
        Paths {
            workspace_home_dir: path_string(root),
            root_dir: path_string(root),
            config_dir: path_string(root),
            data_dir: path_string(&data_dir),
            state_dir: path_string(&root.join("runtime")),
            cache_dir: path_string(&root.join("cache")),
            logs_dir: path_string(&root.join("logs")),
            config_file: path_string(&root.join("config.yaml")),
            identity_dir: path_string(&root.join("identities")),
            database_file: path_string(&data_dir.join("awiki-cli.db")),
            legacy_credentials_dir: path_string(&root.join("legacy").join("credentials")),
            legacy_data_dir: path_string(&root.join("legacy").join("data")),
        }
    }

    fn path_string(path: &Path) -> String {
        path.to_string_lossy().into_owned()
    }

    fn metadata_path(root: &Path) -> PathBuf {
        root.join("cache").join("update").join("metadata.json")
    }

    fn seed_metadata(root: &Path, latest: &str, minimum: &str, retrieved_at: &str) {
        let path = metadata_path(root);
        fs::create_dir_all(path.parent().expect("metadata parent")).expect("create cache dir");
        let raw = format!(
            r#"{{
  "latest_version": "{latest}",
  "min_supported_version": "{minimum}",
  "retrieved_at": "{retrieved_at}",
  "source": "network"
}}"#
        );
        fs::write(path, raw).expect("write metadata");
    }

    #[cfg(unix)]
    fn assert_cache_permissions(root: &Path) {
        use std::os::unix::fs::PermissionsExt;
        let update_dir = root.join("cache").join("update");
        let metadata = metadata_path(root);
        assert_eq!(
            fs::metadata(&update_dir)
                .expect("update dir")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&metadata)
                .expect("metadata file")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    #[cfg(not(unix))]
    fn assert_cache_permissions(_root: &Path) {}

    struct TestUrls {
        _guard: MutexGuard<'static, ()>,
        _no_proxy: EnvVar,
    }

    impl TestUrls {
        fn set(urls: Vec<String>) -> Self {
            let guard = TEST_NPM_LATEST_URLS_LOCK
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            *TEST_NPM_LATEST_URLS.lock().expect("test urls mutex") = Some(urls);
            Self {
                _guard: guard,
                _no_proxy: EnvVar::set("NO_PROXY", "127.0.0.1,localhost,::1"),
            }
        }
    }

    impl Drop for TestUrls {
        fn drop(&mut self) {
            *TEST_NPM_LATEST_URLS.lock().expect("test urls mutex") = None;
        }
    }

    struct TestCurrentVersion {
        _guard: MutexGuard<'static, ()>,
    }

    impl TestCurrentVersion {
        fn set(version: &str) -> Self {
            let guard = TEST_NPM_LATEST_URLS_LOCK
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            *TEST_CURRENT_VERSION
                .lock()
                .expect("test current version mutex") = Some(version.to_string());
            Self { _guard: guard }
        }
    }

    impl Drop for TestCurrentVersion {
        fn drop(&mut self) {
            *TEST_CURRENT_VERSION
                .lock()
                .expect("test current version mutex") = None;
        }
    }

    struct EnvVar {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvVar {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvVar {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    #[derive(Clone)]
    pub(super) struct TestResponse {
        pub(super) status: u16,
        pub(super) body: String,
    }

    impl TestResponse {
        pub(super) fn ok(body: &str) -> Self {
            Self {
                status: 200,
                body: body.to_string(),
            }
        }

        pub(super) fn status(status: u16, body: &str) -> Self {
            Self {
                status,
                body: body.to_string(),
            }
        }
    }

    pub(super) struct TestServer {
        address: String,
        paths: Arc<Mutex<Vec<String>>>,
        handle: Option<thread::JoinHandle<()>>,
    }

    impl TestServer {
        pub(super) fn new(responses: Vec<TestResponse>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
            let address = format!("http://{}", listener.local_addr().expect("local addr"));
            let paths = Arc::new(Mutex::new(Vec::new()));
            let server_paths = paths.clone();
            let handle = thread::spawn(move || {
                for response in responses {
                    let Ok((stream, _)) = listener.accept() else {
                        break;
                    };
                    handle_request(stream, &server_paths, response);
                }
            });
            Self {
                address,
                paths,
                handle: Some(handle),
            }
        }

        pub(super) fn url(&self, path: &str) -> String {
            format!("{}{}", self.address, path)
        }

        fn paths(&self) -> Vec<String> {
            self.paths.lock().expect("paths mutex").clone()
        }
    }

    impl Drop for TestServer {
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

    fn handle_request(
        mut stream: TcpStream,
        paths: &Arc<Mutex<Vec<String>>>,
        response: TestResponse,
    ) {
        let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
        let mut request_line = String::new();
        let _ = reader.read_line(&mut request_line);
        if let Some(path) = request_line.split_whitespace().nth(1) {
            paths.lock().expect("paths mutex").push(path.to_string());
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
        let raw = format!(
            "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            response.status,
            reason,
            response.body.len(),
            response.body
        );
        stream.write_all(raw.as_bytes()).expect("write response");
        let _ = stream.flush();
    }

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new() -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "awiki-cli-rs2-update-unit-{}-{nanos}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("create temp dir");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
