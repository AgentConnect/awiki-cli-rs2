use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn doctor_empty_workspace_reports_go_check_names_and_counts() {
    let workspace = TempDir::new().expect("temp workspace");
    let output = awiki_cmd_with_workspace(&["doctor"], workspace.path());
    assert_success(&output);
    let envelope = success_json(&output);

    assert_eq!(envelope["command"], "awiki-cli doctor");
    assert_eq!(envelope["summary"], "Doctor found warnings");
    assert_eq!(
        check_names(&envelope),
        vec![
            "build",
            "config_file",
            "environment",
            "anp_service",
            "identity_vault",
            "runtime",
            "identity_store",
            "sqlite",
            "anp_mls",
            "workspace_upgrade",
            "legacy_paths",
        ]
    );
    assert_eq!(status_of(&envelope, "build"), "ok");
    assert_eq!(status_of(&envelope, "config_file"), "warn");
    assert_eq!(status_of(&envelope, "environment"), "ok");
    assert_eq!(status_of(&envelope, "anp_service"), "ok");
    assert_eq!(status_of(&envelope, "identity_vault"), "ok");
    assert_eq!(status_of(&envelope, "runtime"), "warn");
    assert_eq!(status_of(&envelope, "identity_store"), "warn");
    assert_eq!(status_of(&envelope, "sqlite"), "info");
    assert_eq!(status_of(&envelope, "anp_mls"), "info");
    assert_eq!(status_of(&envelope, "workspace_upgrade"), "ok");
    assert_eq!(status_of(&envelope, "legacy_paths"), "info");
    assert_eq!(envelope["data"]["counts"]["ok"], 5);
    assert_eq!(envelope["data"]["counts"]["warn"], 3);
    assert_eq!(envelope["data"]["counts"]["error"], 0);
    assert_eq!(envelope["data"]["counts"]["info"], 3);

    let env_check = check_by_name(&envelope, "environment");
    assert_eq!(
        env_check["details"]["hits"][0]["key"],
        "AWIKI_CLI_WORKSPACE_HOME_DIR"
    );
    let sqlite = check_by_name(&envelope, "sqlite");
    assert_eq!(sqlite["details"]["contact_handle_bindings_exists"], false);
    assert_eq!(sqlite["details"]["contact_handle_bindings_count"], 0);
    assert_eq!(sqlite["details"]["owner_identity_invariant_count"], 0);
    assert_eq!(
        sqlite["details"]["legacy_secure_tables"]["e2ee_sessions_count"],
        0
    );
    let workspace_upgrade = check_by_name(&envelope, "workspace_upgrade");
    assert_eq!(
        workspace_upgrade["details"]["detection"]["current_version"],
        4
    );
    assert_eq!(
        workspace_upgrade["details"]["detection"]["current_version_source"],
        "default_empty"
    );
    assert_eq!(workspace_upgrade["details"]["detection"]["empty"], true);
    assert_eq!(
        workspace_upgrade["details"]["detection"]["has_workspace"],
        false
    );
}

#[test]
fn doctor_identity_vault_reports_root_key_availability_without_secret_value() {
    let workspace = TempDir::new().expect("temp workspace");
    let root_key = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    std::fs::write(
        workspace.path().join("config.yaml"),
        "secret_storage:\n  mode: vault_required\n",
    )
    .expect("write vault config");

    let mut command = awiki_command(&["doctor"], workspace.path());
    command.env("AWIKI_IM_CORE_VAULT_ROOT_KEY_B64", root_key);
    let output = command.output().expect("run doctor");
    assert_success(&output);
    let envelope = success_json(&output);

    let vault = check_by_name(&envelope, "identity_vault");
    assert_eq!(vault["status"], "ok");
    assert_eq!(vault["details"]["mode"], "vault_required");
    assert_eq!(vault["details"]["root_key"]["available"], true);
    assert_eq!(
        vault["details"]["root_key"]["source"],
        "AWIKI_IM_CORE_VAULT_ROOT_KEY_B64"
    );
    let encoded = serde_json::to_string(&envelope).expect("doctor json");
    assert!(
        !encoded.contains(root_key),
        "doctor must not expose vault root key: {encoded}"
    );
}

#[test]
fn doctor_initialized_workspace_reports_sqlite_and_identity_details() {
    let workspace = TempDir::new().expect("temp workspace");
    assert_success(&awiki_cmd_with_workspace(&["init"], workspace.path()));
    assert_success(&awiki_cmd_with_workspace(
        &[
            "--migration",
            "id",
            "create",
            "--name",
            "Alice",
            "--identity",
            "alice",
        ],
        workspace.path(),
    ));
    seed_contact_handle_binding(workspace.path());

    let output = awiki_cmd_with_workspace(&["doctor"], workspace.path());
    assert_success(&output);
    let envelope = success_json(&output);

    assert_eq!(status_of(&envelope, "config_file"), "ok");
    assert_eq!(status_of(&envelope, "identity_store"), "warn");
    assert_eq!(status_of(&envelope, "sqlite"), "ok");
    let identity = check_by_name(&envelope, "identity_store");
    assert_eq!(
        identity["details"]["default_identity"]["identity_name"],
        "alice"
    );
    assert_eq!(
        identity["details"]["user_state"]["ready_for_messaging"],
        false
    );
    let sqlite = check_by_name(&envelope, "sqlite");
    assert_eq!(
        sqlite["details"]["schema_version"],
        im_core::compat::local_state::SCHEMA_VERSION
    );
    assert_eq!(
        sqlite["details"]["target_schema_version"],
        im_core::compat::local_state::SCHEMA_VERSION
    );
    assert_eq!(sqlite["details"]["contact_handle_bindings_exists"], true);
    assert_eq!(sqlite["details"]["contact_handle_bindings_count"], 1);
    assert_eq!(sqlite["details"]["owner_identity_invariant_count"], 0);
}

#[test]
fn doctor_reports_owner_invariant_summary_without_secure_plaintext() {
    let workspace = TempDir::new().expect("temp workspace");
    assert_success(&awiki_cmd_with_workspace(&["init"], workspace.path()));
    seed_owner_invariant_violation_with_sentinels(workspace.path());

    let output = awiki_cmd_with_workspace(&["doctor"], workspace.path());
    assert_success(&output);
    let envelope = success_json(&output);

    assert_eq!(status_of(&envelope, "sqlite"), "warn");
    let sqlite = check_by_name(&envelope, "sqlite");
    assert_eq!(sqlite["details"]["owner_identity_invariant_count"], 1);
    assert_eq!(
        sqlite["details"]["owner_identity_invariants"][0]["table"],
        "messages"
    );
    assert_eq!(
        sqlite["details"]["owner_identity_invariants"][0]["invariant"],
        "conversation_id_must_not_include_owner_did"
    );
    assert_eq!(
        sqlite["details"]["owner_identity_invariants"][0]["row_count"],
        1
    );
    let encoded = serde_json::to_string(sqlite).expect("sqlite json");
    for forbidden in [
        "plaintext-sentinel-do-not-leak",
        "private-key-sentinel-do-not-leak",
        "jwt-token-sentinel-do-not-leak",
        "raw-ciphertext-sentinel-do-not-leak",
    ] {
        assert!(
            !encoded.contains(forbidden),
            "doctor sqlite details must not expose sentinel {forbidden}: {encoded}"
        );
    }
}

#[test]
fn doctor_reports_invalid_config_and_anp_service_as_blocking() {
    let workspace = TempDir::new().expect("temp workspace");
    let config = workspace.path().join("config.yaml");
    std::fs::write(
        &config,
        "schema_version: 1\nservices:\n  service_base_url: https://127.0.0.1\n  anp_service_endpoint: https://127.0.0.1/anp-im/rpc\n  anp_service_did: did:key:z6Mkwrong\n",
    )
    .expect("write invalid ANP config");

    let output = awiki_cmd_with_workspace(&["doctor"], workspace.path());
    assert_success(&output);
    let envelope = success_json(&output);

    assert_eq!(envelope["summary"], "Doctor found blocking issues");
    assert_eq!(status_of(&envelope, "config_file"), "ok");
    assert_eq!(status_of(&envelope, "anp_service"), "error");
    assert!(
        check_by_name(&envelope, "anp_service")["details"]["endpoint_error"]
            .as_str()
            .is_some_and(|value| value.contains("loopback"))
    );
    assert!(
        check_by_name(&envelope, "anp_service")["details"]["service_did_error"]
            .as_str()
            .is_some_and(|value| value.contains("did:wba"))
    );
}

#[test]
fn doctor_anp_mls_probe_and_state_details_match_go_contract() {
    let workspace = TempDir::new().expect("temp workspace");
    let bin_dir = TempDir::new().expect("temp bin");
    write_fake_anp_mls(
        &bin_dir.path().join("anp-mls"),
        r#"{"ok":true,"api_version":"anp-mls/v1","request_id":"doctor-system-version","result":{"api_version":"anp-mls/v1","binary_name":"anp-mls","binary_version":"test","supported_commands":["system version","key-package generate"]}}"#,
    );
    let mls_dir = workspace.path().join("mls");
    std::fs::create_dir_all(mls_dir.join("agents").join("agent-a").join("device-1"))
        .expect("create scoped MLS state");
    std::fs::write(mls_dir.join("state.db"), b"root").expect("write root state");
    std::fs::write(
        mls_dir
            .join("agents")
            .join("agent-a")
            .join("device-1")
            .join("state.db"),
        b"scoped",
    )
    .expect("write scoped state");

    let mut command = awiki_command(&["doctor"], workspace.path());
    command.env("AWIKI_ANP_MLS_BINARY", bin_dir.path().join("anp-mls"));
    let output = command.output().expect("run doctor");
    assert_success(&output);
    let envelope = success_json(&output);

    let anp_mls = check_by_name(&envelope, "anp_mls");
    assert_eq!(anp_mls["status"], "ok");
    assert_eq!(anp_mls["details"]["version"]["api_version"], "anp-mls/v1");
    assert_eq!(
        anp_mls["details"]["version"]["supports_system_version"],
        true
    );
    assert_eq!(anp_mls["details"]["binary_available"], true);
    assert_eq!(anp_mls["details"]["data_dir_status"], "ok");
    assert_eq!(anp_mls["details"]["state_db_status"], "ok");
    assert_eq!(anp_mls["details"]["scoped_state_count"], 1);
    assert_eq!(anp_mls["details"]["scoped_state_db_count"], 1);
    let encoded = serde_json::to_string(anp_mls).expect("anp_mls json");
    assert!(
        !encoded.contains(bin_dir.path().to_string_lossy().as_ref()),
        "doctor anp_mls details must not expose provider binary paths: {encoded}"
    );
    assert!(
        !encoded.contains(workspace.path().to_string_lossy().as_ref()),
        "doctor anp_mls details must not expose MLS state paths: {encoded}"
    );
    assert!(
        !encoded.contains("state.db") && !encoded.contains("state.lock"),
        "doctor anp_mls details must not expose MLS state file paths: {encoded}"
    );
}

fn awiki_cmd_with_workspace(args: &[&str], workspace: &Path) -> Output {
    awiki_command(args, workspace)
        .output()
        .expect("run awiki-cli binary")
}

fn awiki_command(args: &[&str], workspace: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_awiki-cli"));
    command
        .args(args)
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace)
        .env("HOME", workspace.join("home"))
        .env("USERPROFILE", workspace.join("home"))
        .env("PATH", "/usr/bin:/bin")
        .env("AWIKI_CLI_UPDATE_CACHE_ONLY", "1")
        .env_remove("AWIKI_WORKSPACE")
        .env_remove("AWIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_HOME")
        .env_remove("AVIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_FORMAT")
        .env_remove("AVIKI_FORMAT")
        .env_remove("AWIKI_ANP_MLS_BINARY")
        .env_remove("AWIKI_IM_CORE_VAULT_ROOT_KEY_B64");
    command
}

fn seed_contact_handle_binding(workspace: &Path) {
    let database = workspace.join("data").join("awiki-cli.db");
    let connection = rusqlite::Connection::open(&database).expect("open sqlite database");
    im_core::compat::local_state::ensure_schema(&connection).expect("ensure schema");
    connection
        .execute(
            "INSERT INTO contact_handle_bindings (owner_identity_id, owner_did, handle, did, first_seen_at, last_seen_at, credential_name) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                "alice",
                "did:wba:alice.example",
                "bob",
                "did:wba:bob.example",
                "2026-04-18T09:00:00Z",
                "2026-04-18T09:00:00Z",
                "alice",
            ],
        )
        .expect("seed contact handle binding");
}

fn seed_owner_invariant_violation_with_sentinels(workspace: &Path) {
    let database = workspace.join("data").join("awiki-cli.db");
    let connection = rusqlite::Connection::open(&database).expect("open sqlite database");
    im_core::compat::local_state::ensure_schema(&connection).expect("ensure schema");
    connection
        .execute(
            "INSERT INTO identity_did_history (owner_identity_id, did, status, first_seen_at, last_seen_at)
             VALUES (?1, ?2, 'current', ?3, ?3)",
            rusqlite::params![
                "alice",
                "did:wba:alice.example",
                "2026-05-31T00:00:00Z",
            ],
        )
        .expect("seed identity DID history");
    connection
        .execute(
            "INSERT INTO messages (msg_id, owner_identity_id, owner_did, conversation_id, thread_id, content, metadata, stored_at)
             VALUES (?1, ?2, ?3, ?4, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                "msg-owner-invariant",
                "alice",
                "did:wba:alice.example",
                "dm:did:wba:alice.example:did:wba:bob.example",
                "plaintext-sentinel-do-not-leak",
                r#"{"private_key":"private-key-sentinel-do-not-leak","jwt_token":"jwt-token-sentinel-do-not-leak"}"#,
                "2026-05-31T00:00:00Z",
            ],
        )
        .expect("seed invariant message");
    connection
        .execute(
            "INSERT INTO e2ee_outbox (outbox_id, owner_identity_id, owner_did, peer_did, plaintext, metadata, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
            rusqlite::params![
                "outbox-owner-invariant",
                "alice",
                "did:wba:alice.example",
                "did:wba:bob.example",
                "plaintext-sentinel-do-not-leak",
                r#"{"ciphertext":"raw-ciphertext-sentinel-do-not-leak"}"#,
                "2026-05-31T00:00:00Z",
            ],
        )
        .expect("seed secure outbox sentinel");
}

fn check_names(envelope: &Value) -> Vec<&str> {
    envelope["data"]["checks"]
        .as_array()
        .expect("checks array")
        .iter()
        .map(|check| check["name"].as_str().expect("check name"))
        .collect()
}

fn status_of<'a>(envelope: &'a Value, name: &str) -> &'a str {
    check_by_name(envelope, name)["status"]
        .as_str()
        .expect("check status")
}

fn check_by_name<'a>(envelope: &'a Value, name: &str) -> &'a Value {
    envelope["data"]["checks"]
        .as_array()
        .expect("checks array")
        .iter()
        .find(|check| check["name"] == name)
        .unwrap_or_else(|| panic!("missing check {name}"))
}

fn assert_success(output: &Output) {
    assert_eq!(
        output.status.code(),
        Some(0),
        "unexpected exit status; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn success_json(output: &Output) -> Value {
    assert!(
        output.stderr.is_empty(),
        "stderr should be empty, got {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be JSON envelope");
    assert_eq!(envelope["ok"], true);
    envelope
}

fn write_fake_anp_mls(path: &Path, response: &str) {
    let body = format!(
        "#!/bin/sh\ncat >/dev/null\nprintf '%s\\n' '{}'\n",
        response.replace('\'', "'\\''")
    );
    std::fs::write(path, body).expect("write fake anp-mls");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
            .expect("chmod fake anp-mls");
    }
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let thread_id = format!("{:?}", std::thread::current().id())
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric())
            .collect::<String>();
        let path = std::env::temp_dir().join(format!(
            "awiki-cli-rs2-test-{}-{nanos}-{thread_id}-{counter}",
            std::process::id()
        ));
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
