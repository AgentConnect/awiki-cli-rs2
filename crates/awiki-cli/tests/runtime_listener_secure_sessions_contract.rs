use awiki_cli::config::{Paths, Resolved};
use awiki_cli::legacy_identity::Manager;
use awiki_cli::runtime_legacy::listener_secure_sessions::{
    pending_confirmation_peer_dids, pending_confirmation_peer_dids_in_identity_dir, read_json_file,
};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn pending_confirmation_peer_dids_scan_matches_go_file_boundaries() {
    let workspace = TempDir::new("listener-secure-sessions-scan").expect("workspace");
    let identity_dir = workspace.path().join("identities").join("alice-dir");
    let sessions = identity_dir.join("p5-e2ee-sessions");
    std::fs::create_dir_all(&sessions).expect("sessions dir");
    write_json(
        &sessions.join("01-bob.json"),
        &json!({"peer_did": "did:peer:bob", "status": "pending-confirmation"}),
    );
    write_json(
        &sessions.join("02-carol.json"),
        &json!({"peer_did": "did:peer:carol", "status": "established"}),
    );
    write_json(
        &sessions.join("03-blank.json"),
        &json!({"peer_did": "   ", "status": "pending-confirmation"}),
    );
    write_json(
        &sessions.join("04-duplicate.json"),
        &json!({"peer_did": "did:peer:bob", "status": "pending-confirmation"}),
    );
    write_json(
        &sessions.join("05-numeric-peer.json"),
        &json!({"peer_did": 42, "status": "pending-confirmation"}),
    );
    std::fs::write(sessions.join("06-bad.json"), b"{not-json").expect("bad json");
    write_json(
        &sessions.join("07-dave.txt"),
        &json!({"peer_did": "did:peer:dave", "status": "pending-confirmation"}),
    );
    write_json(
        &sessions.join("08-eve.json"),
        &json!({"peer_did": " did:peer:eve ", "status": "pending-confirmation"}),
    );

    assert_eq!(
        pending_confirmation_peer_dids_in_identity_dir(&identity_dir),
        vec!["did:peer:bob", " did:peer:eve "]
    );
}

#[test]
fn pending_confirmation_peer_dids_returns_empty_for_missing_inputs_like_go() {
    let workspace = TempDir::new("listener-secure-sessions-empty").expect("workspace");
    let identity_dir = workspace.path().join("identities").join("alice-dir");

    assert!(pending_confirmation_peer_dids(None, "alice").is_empty());
    assert!(pending_confirmation_peer_dids_in_identity_dir(&identity_dir).is_empty());
}

#[test]
fn pending_confirmation_peer_dids_uses_identity_manager_paths_like_go() {
    let workspace = TempDir::new("listener-secure-sessions-manager").expect("workspace");
    let identity_root = workspace.path().join("identities");
    let alice_dir = identity_root.join("alice-dir");
    let sessions = alice_dir.join("p5-e2ee-sessions");
    std::fs::create_dir_all(&sessions).expect("sessions dir");
    std::fs::create_dir_all(&identity_root).expect("identity root");
    std::fs::write(
        identity_root.join("index.json"),
        r#"{"schema_version":3,"default_credential_name":"alice","credentials":{"alice":{"credential_name":"alice","dir_name":"alice-dir","did":"did:alice","unique_id":"alice-dir","is_default":true}}}"#,
    )
    .expect("index");
    write_json(
        &sessions.join("peer.json"),
        &json!({"peer_did": "did:peer:bob", "status": "pending-confirmation"}),
    );
    let manager = Manager::new(resolved_with_identity_root(&identity_root).paths);

    assert_eq!(
        pending_confirmation_peer_dids(Some(&manager), "alice"),
        vec!["did:peer:bob"]
    );
    assert_eq!(
        pending_confirmation_peer_dids(Some(&manager), "default"),
        vec!["did:peer:bob"]
    );
    assert!(pending_confirmation_peer_dids(Some(&manager), "missing").is_empty());
    assert!(pending_confirmation_peer_dids(Some(&manager), "   ").is_empty());
}

#[test]
fn read_json_file_matches_go_read_then_unmarshal_boundary() {
    let workspace = TempDir::new("listener-secure-sessions-read-json").expect("workspace");
    let valid = workspace.path().join("valid.json");
    write_json(&valid, &json!({"peer_did": "did:peer:bob"}));
    assert_eq!(
        read_json_file(&valid).expect("valid")["peer_did"],
        "did:peer:bob"
    );

    let malformed = workspace.path().join("malformed.json");
    std::fs::write(&malformed, b"{not-json").expect("malformed");
    assert!(read_json_file(&malformed).is_err());
    assert!(read_json_file(&workspace.path().join("missing.json")).is_err());
}

fn write_json(path: &Path, value: &serde_json::Value) {
    std::fs::write(path, serde_json::to_vec(value).expect("json")).expect("write json");
}

fn resolved_with_identity_root(identity_root: &Path) -> Resolved {
    Resolved {
        paths: Paths {
            workspace_home_dir: String::new(),
            root_dir: String::new(),
            config_dir: String::new(),
            data_dir: String::new(),
            state_dir: String::new(),
            cache_dir: String::new(),
            logs_dir: String::new(),
            config_file: String::new(),
            identity_dir: identity_root.to_string_lossy().into_owned(),
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
        host_notify_sink: "log".to_string(),
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
        sources: Default::default(),
    }
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> std::io::Result<Self> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("{prefix}-{}-{nonce}", std::process::id()));
        std::fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
