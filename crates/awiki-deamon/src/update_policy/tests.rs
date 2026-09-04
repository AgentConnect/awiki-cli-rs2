use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

use serde_json::json;

use super::*;

type TestResponse = (u16, Vec<(String, String)>, Vec<u8>);

struct TestServer {
    origin: String,
    requests: Arc<Mutex<Vec<String>>>,
    handle: Option<thread::JoinHandle<()>>,
}

impl TestServer {
    fn new(build: impl FnOnce(&str) -> Vec<TestResponse>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let responses = build(&origin);
        let requests = Arc::new(Mutex::new(Vec::new()));
        let server_requests = Arc::clone(&requests);
        let handle = thread::spawn(move || {
            for (status, headers, body) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request_line = String::new();
                reader.read_line(&mut request_line).unwrap();
                server_requests.lock().unwrap().push(
                    request_line
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or_default()
                        .to_string(),
                );
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" || line == "\n" || line.is_empty() {
                        break;
                    }
                }
                let reason = if status == 200 { "OK" } else { "Error" };
                write!(
                    stream,
                    "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
                    body.len()
                )
                .unwrap();
                for (name, value) in headers {
                    write!(stream, "{name}: {value}\r\n").unwrap();
                }
                write!(stream, "Connection: close\r\n\r\n").unwrap();
                stream.write_all(&body).unwrap();
                stream.flush().unwrap();
            }
        });
        Self {
            origin,
            requests,
            handle: Some(handle),
        }
    }

    fn paths(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            if !handle.is_finished() {
                if let Some(address) = self.origin.strip_prefix("http://") {
                    if let Ok(mut stream) = TcpStream::connect(address) {
                        let _ = stream.write_all(
                            b"GET /__shutdown HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
                        );
                    }
                }
            }
            let _ = handle.join();
        }
    }
}

fn config(root: &Path, origin: &str) -> DaemonConfig {
    let mut config = DaemonConfig::for_state_root(root).unwrap();
    config.service_base_url = origin.to_string();
    config.user_service_base_url = origin.to_string();
    config.download_base_url = format!("{origin}/daemon");
    config
}

fn server_info(origin: &str, revision: u64, recommended: &str, minimum: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "schema_version": 1,
        "client_versions": {
            "schema_version": 1,
            "channel": "stable",
            "policy_origin": origin,
            "policy_revision": revision,
            "published_at": "2026-09-04T00:00:00Z",
            "products": {
                "daemon": {
                    "enabled": true,
                    "recommended_version": recommended,
                    "minimum_supported_version": minimum,
                    "upgrade_url": format!("{origin}/daemon/install.sh"),
                    "artifact_manifest_url": format!("{origin}/daemon/releases/manifest.json")
                }
            }
        }
    }))
    .unwrap()
}

#[test]
fn requests_registered_tenant_server_info_with_daemon_platform() {
    let server = TestServer::new(|origin| {
        vec![(200, Vec::new(), server_info(origin, 4, "0.1.93", "0.1.92"))]
    });
    let temporary = tempfile::tempdir().unwrap();
    let config = config(temporary.path(), &server.origin);

    let policy = load_daemon_update_policy(&config).unwrap();

    assert_eq!(policy.policy_origin, server.origin);
    assert_eq!(policy.policy_revision, 4);
    assert_eq!(policy.recommended_version.as_deref(), Some("0.1.93"));
    assert_eq!(policy.minimum_supported_version.as_deref(), Some("0.1.92"));
    assert_eq!(policy.source, DaemonUpdatePolicySource::Network);
    assert_eq!(
        server.paths(),
        vec!["/user-service/v1/server-info?client_platform=daemon"]
    );
}

#[test]
fn offline_refresh_uses_only_same_tenant_verified_cache() {
    let server =
        TestServer::new(|origin| vec![(200, Vec::new(), server_info(origin, 8, "0.2.0", "0.1.0"))]);
    let temporary = tempfile::tempdir().unwrap();
    let tenant_config = config(temporary.path(), &server.origin);
    let fresh = load_daemon_update_policy(&tenant_config).unwrap();
    assert_eq!(fresh.source, DaemonUpdatePolicySource::Network);
    drop(server);

    let cached = load_daemon_update_policy(&tenant_config).unwrap();
    assert_eq!(cached.source, DaemonUpdatePolicySource::Cache);
    assert_eq!(cached.policy_revision, 8);
    assert!(cached.refresh_error.is_some());

    let other = TestServer::new(|_| vec![(503, Vec::new(), b"unavailable".to_vec())]);
    let other_config = config(temporary.path(), &other.origin);
    assert!(load_daemon_update_policy(&other_config).is_err());
}

#[test]
fn revision_rollback_and_same_revision_mutation_keep_cached_policy() {
    let server = TestServer::new(|origin| {
        vec![
            (200, Vec::new(), server_info(origin, 9, "0.3.0", "0.1.0")),
            (200, Vec::new(), server_info(origin, 8, "0.4.0", "0.1.0")),
            (200, Vec::new(), server_info(origin, 9, "0.4.0", "0.1.0")),
        ]
    });
    let temporary = tempfile::tempdir().unwrap();
    let config = config(temporary.path(), &server.origin);

    let initial = load_daemon_update_policy(&config).unwrap();
    assert_eq!(initial.recommended_version.as_deref(), Some("0.3.0"));
    let rollback = load_daemon_update_policy(&config).unwrap();
    assert_eq!(rollback.recommended_version.as_deref(), Some("0.3.0"));
    assert!(rollback
        .refresh_error
        .as_deref()
        .unwrap()
        .contains("rolled back"));
    let mutation = load_daemon_update_policy(&config).unwrap();
    assert_eq!(mutation.recommended_version.as_deref(), Some("0.3.0"));
    assert!(mutation
        .refresh_error
        .as_deref()
        .unwrap()
        .contains("without increasing"));
}

#[test]
fn rejects_cross_origin_policy_and_redirect_without_cache() {
    let server = TestServer::new(|_| {
        let body = serde_json::to_vec(&json!({
            "schema_version": 1,
            "client_versions": {
                "schema_version": 1,
                "channel": "stable",
                "policy_origin": "https://other.example",
                "policy_revision": 1,
                "published_at": "2026-09-04T00:00:00Z",
                "products": {"daemon": {"enabled": false}}
            }
        }))
        .unwrap();
        vec![(200, Vec::new(), body)]
    });
    let temporary = tempfile::tempdir().unwrap();
    assert!(load_daemon_update_policy(&config(temporary.path(), &server.origin)).is_err());

    let redirect = TestServer::new(|_| {
        vec![(
            302,
            vec![(
                "Location".to_string(),
                "https://other.example/policy".to_string(),
            )],
            Vec::new(),
        )]
    });
    let temporary = tempfile::tempdir().unwrap();
    let error = load_daemon_update_policy(&config(temporary.path(), &redirect.origin)).unwrap_err();
    assert!(error.to_string().contains("must not redirect"));
}

#[test]
fn disabled_policy_is_valid_and_does_not_invent_an_upgrade() {
    let server = TestServer::new(|origin| {
        let body = serde_json::to_vec(&json!({
            "schema_version": 1,
            "client_versions": {
                "schema_version": 1,
                "channel": "stable",
                "policy_origin": origin,
                "policy_revision": 2,
                "published_at": "2026-09-04T00:00:00Z",
                "products": {"daemon": {"enabled": false}}
            }
        }))
        .unwrap();
        vec![(200, Vec::new(), body)]
    });
    let temporary = tempfile::tempdir().unwrap();
    let policy = load_daemon_update_policy(&config(temporary.path(), &server.origin)).unwrap();
    assert!(!policy.enabled);
    assert!(policy.recommended_version.is_none());
    assert!(!policy.recommends_newer_than("0.0.1"));
}

#[test]
fn rejects_oversized_and_internally_inconsistent_policy() {
    let oversized =
        TestServer::new(|_| vec![(200, Vec::new(), vec![b'x'; POLICY_RESPONSE_MAX_BYTES + 1])]);
    let temporary = tempfile::tempdir().unwrap();
    let error =
        load_daemon_update_policy(&config(temporary.path(), &oversized.origin)).unwrap_err();
    assert!(error.to_string().contains("exceeds 1 MiB"));

    let inconsistent =
        TestServer::new(|origin| vec![(200, Vec::new(), server_info(origin, 4, "0.1.0", "0.2.0"))]);
    let temporary = tempfile::tempdir().unwrap();
    let error =
        load_daemon_update_policy(&config(temporary.path(), &inconsistent.origin)).unwrap_err();
    assert!(error.to_string().contains("minimum version exceeds"));
}

#[test]
fn only_revision_three_can_bridge_the_legacy_missing_artifact_field() {
    let response_without_artifact = |origin: &str, revision: u64| {
        serde_json::to_vec(&json!({
            "schema_version": 1,
            "client_versions": {
                "schema_version": 1,
                "channel": "stable",
                "policy_origin": origin,
                "policy_revision": revision,
                "published_at": "2026-09-04T00:00:00Z",
                "products": {
                    "daemon": {
                        "enabled": true,
                        "recommended_version": "0.1.93",
                        "minimum_supported_version": "0.1.92",
                        "upgrade_url": format!("{origin}/daemon/install.sh")
                    }
                }
            }
        }))
        .unwrap()
    };
    let legacy =
        TestServer::new(|origin| vec![(200, Vec::new(), response_without_artifact(origin, 3))]);
    let temporary = tempfile::tempdir().unwrap();
    let policy = load_daemon_update_policy(&config(temporary.path(), &legacy.origin)).unwrap();
    let expected_manifest = format!("{}/daemon/releases/manifest.json", legacy.origin);
    assert_eq!(
        policy.artifact_manifest_url.as_deref(),
        Some(expected_manifest.as_str())
    );

    let current =
        TestServer::new(|origin| vec![(200, Vec::new(), response_without_artifact(origin, 4))]);
    let temporary = tempfile::tempdir().unwrap();
    let error = load_daemon_update_policy(&config(temporary.path(), &current.origin)).unwrap_err();
    assert!(error.to_string().contains("missing artifact_manifest_url"));
}
