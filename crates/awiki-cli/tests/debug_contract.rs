use rusqlite::Connection;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn debug_db_handle_history_requires_exactly_one_handle() {
    let workspace = TempDir::new().expect("temp workspace");

    let missing = awiki_cmd_with_workspace(
        &["--diagnostic", "debug", "db", "handle-history"],
        workspace.path(),
    );
    assert_code(&missing, 2);
    let envelope = error_json(&missing);
    assert_eq!(envelope["error"]["code"], "invalid_argument");
    assert_contains(
        &envelope["error"]["message"],
        "debug db handle-history requires exactly one handle",
    );
    assert_contains(
        &envelope["error"]["hint"],
        "awiki-cli debug db handle-history <handle>",
    );

    let extra = awiki_cmd_with_workspace(
        &[
            "--diagnostic",
            "debug",
            "db",
            "handle-history",
            "alice",
            "bob",
        ],
        workspace.path(),
    );
    assert_code(&extra, 2);
    let envelope = error_json(&extra);
    assert_eq!(envelope["error"]["code"], "invalid_argument");
    assert_contains(
        &envelope["error"]["message"],
        "debug db handle-history requires exactly one handle",
    );
}

#[test]
fn debug_db_handle_history_rejects_blank_normalized_handle() {
    let workspace = TempDir::new().expect("temp workspace");
    assert_success(&awiki_cmd_with_workspace(&["init"], workspace.path()));

    let output = awiki_cmd_with_workspace(
        &["--diagnostic", "debug", "db", "handle-history", "   "],
        workspace.path(),
    );
    assert_code(&output, 2);
    let envelope = error_json(&output);

    assert_eq!(envelope["error"]["code"], "invalid_argument");
    assert_eq!(envelope["error"]["message"], "handle is required.");
    assert_eq!(
        envelope["error"]["hint"],
        "Provide a handle local-part or full handle."
    );
}

#[test]
fn debug_db_handle_history_normalizes_handle_and_aggregates_rows_by_owner() {
    let workspace = TempDir::new().expect("temp workspace");
    assert_success(&awiki_cmd_with_workspace(&["init"], workspace.path()));
    seed_handle_history(workspace.path());

    let output = awiki_cmd_with_workspace(
        &[
            "--diagnostic",
            "debug",
            "db",
            "handle-history",
            "  wba://ALICE.awiki.ai  ",
        ],
        workspace.path(),
    );
    assert_success(&output);
    let envelope = success_json(&output);

    assert_eq!(envelope["command"], "awiki-cli debug db handle-history");
    assert_eq!(
        envelope["summary"],
        "Loaded local DID history for handle alice"
    );
    assert_eq!(envelope["data"]["handle"], "alice");
    assert_eq!(
        envelope["data"]["database_file"],
        workspace
            .path()
            .join("data")
            .join("awiki-cli.db")
            .to_string_lossy()
            .as_ref()
    );

    let rows = envelope["data"]["rows"]
        .as_array()
        .expect("rows should be an array");
    assert_eq!(rows.len(), 3, "rows should include every binding: {rows:?}");
    assert_eq!(rows[0]["owner_did"], "did:owner-a");
    assert_eq!(rows[0]["did"], "did:peer-current");
    assert_eq!(rows[0]["is_current"], 1);
    assert_eq!(rows[1]["owner_did"], "did:owner-a");
    assert_eq!(rows[1]["did"], "did:peer-old");
    assert_eq!(rows[1]["is_current"], 0);
    assert_eq!(rows[2]["owner_did"], "did:owner-b");
    assert_eq!(rows[2]["did"], "did:peer-b");

    let owners = envelope["data"]["owners"]
        .as_array()
        .expect("owners should be an array");
    assert_eq!(
        owners.len(),
        2,
        "owners should aggregate by owner: {owners:?}"
    );
    assert_owner(
        &owners[0],
        "did:owner-a",
        "did:peer-current",
        &["did:peer-current", "did:peer-old"],
    );
    assert_owner(&owners[1], "did:owner-b", "did:peer-b", &["did:peer-b"]);
}

#[test]
fn debug_db_handle_history_returns_not_found_for_unknown_normalized_handle() {
    let workspace = TempDir::new().expect("temp workspace");
    assert_success(&awiki_cmd_with_workspace(&["init"], workspace.path()));
    seed_handle_history(workspace.path());

    let output = awiki_cmd_with_workspace(
        &[
            "--diagnostic",
            "debug",
            "db",
            "handle-history",
            "wba://MISSING.example.com",
        ],
        workspace.path(),
    );
    assert_code(&output, 5);
    let envelope = error_json(&output);

    assert_eq!(envelope["error"]["code"], "not_found");
    assert_eq!(envelope["error"]["message"], "sql: no rows in result set");
    assert_contains(
        &envelope["error"]["hint"],
        "No local DID history is stored for handle \"missing\"",
    );
}

#[test]
fn debug_db_query_returns_stable_unsupported_capability_without_opening_store() {
    let workspace = TempDir::new().expect("temp workspace");
    std::fs::write(
        workspace.path().join("config.json"),
        r#"{"schema_version":1,"services":{"service_base_url":"https://legacy.example","did_domain":"legacy.example"},"runtime":{"mode":"http"}}"#,
    )
    .expect("write legacy config");

    let output = awiki_cmd_with_workspace(
        &[
            "debug",
            "db",
            "query",
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'messages'",
        ],
        workspace.path(),
    );
    assert_code(&output, 2);
    let envelope = error_json(&output);

    assert_eq!(envelope["error"]["code"], "unsupported_capability");
    assert_eq!(envelope["error"]["details"]["command"], "debug.db.query");
    assert_eq!(envelope["error"]["details"]["capability"], "raw-sql");
    assert_eq!(
        envelope["error"]["details"]["required_phase"],
        "outside current im-core cutover"
    );
    assert_eq!(
        envelope["error"]["details"]["cutover_status"],
        "unsupported"
    );
    assert!(
        !workspace.path().join("data").join("awiki-cli.db").exists(),
        "unsupported debug db query must not create the local SQLite store"
    );
    assert!(
        !workspace
            .path()
            .join("runtime")
            .join("message-daemon.sock")
            .exists(),
        "unsupported debug db query must not create runtime socket artifacts"
    );
    assert!(
        !workspace
            .path()
            .join("runtime")
            .join("listener.pid")
            .exists(),
        "unsupported debug db query must not create listener pid artifacts"
    );
}

fn seed_handle_history(workspace: &Path) {
    let database_file = workspace.join("data").join("awiki-cli.db");
    let connection = Connection::open(&database_file).expect("open test database");
    connection
        .execute(
            r#"
INSERT INTO contact_handle_bindings (
    owner_did, handle, did, is_current, first_seen_at, last_seen_at,
    source_type, source_group_id, metadata, credential_name
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
"#,
            rusqlite::params![
                "did:owner-a",
                "alice",
                "did:peer-current",
                1,
                "2026-01-01T00:00:00Z",
                "2026-01-03T00:00:00Z",
                "listener.direct_incoming",
                "",
                r#"{"rank":1}"#,
                "owner-a",
            ],
        )
        .expect("insert current binding");
    connection
        .execute(
            r#"
INSERT INTO contact_handle_bindings (
    owner_did, handle, did, is_current, first_seen_at, last_seen_at,
    source_type, source_group_id, metadata, credential_name
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
"#,
            rusqlite::params![
                "did:owner-a",
                "alice",
                "did:peer-old",
                0,
                "2025-12-01T00:00:00Z",
                "2026-01-02T00:00:00Z",
                "listener.direct_incoming",
                "",
                r#"{"rank":2}"#,
                "owner-a",
            ],
        )
        .expect("insert historical binding");
    connection
        .execute(
            r#"
INSERT INTO contact_handle_bindings (
    owner_did, handle, did, is_current, first_seen_at, last_seen_at,
    source_type, source_group_id, metadata, credential_name
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
"#,
            rusqlite::params![
                "did:owner-b",
                "alice",
                "did:peer-b",
                1,
                "2026-01-01T00:00:00Z",
                "2026-01-04T00:00:00Z",
                "listener.group_member",
                "group-1",
                r#"{"rank":3}"#,
                "owner-b",
            ],
        )
        .expect("insert second owner binding");
    connection
        .execute(
            r#"
INSERT INTO contact_handle_bindings (
    owner_did, handle, did, is_current, first_seen_at, last_seen_at,
    credential_name
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
"#,
            rusqlite::params![
                "did:owner-a",
                "bob",
                "did:peer-bob",
                1,
                "2026-01-01T00:00:00Z",
                "2026-01-05T00:00:00Z",
                "owner-a",
            ],
        )
        .expect("insert unrelated handle binding");
}

fn assert_owner(value: &Value, owner_did: &str, current_did: &str, historical_dids: &[&str]) {
    assert_eq!(value["owner_did"], owner_did);
    assert_eq!(value["current_did"], current_did);
    assert_eq!(value["historical_count"], historical_dids.len());
    let actual = value["historical_dids"]
        .as_array()
        .expect("historical_dids should be an array")
        .iter()
        .map(|did| did.as_str().expect("historical did should be a string"))
        .collect::<Vec<_>>();
    assert_eq!(actual, historical_dids);
}

fn awiki_cmd_with_workspace(args: &[&str], workspace: &Path) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_awiki-cli"));
    command
        .args(args)
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace)
        .env("AWIKI_CLI_UPDATE_CACHE_ONLY", "1")
        .env_remove("AWIKI_WORKSPACE")
        .env_remove("AWIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_HOME")
        .env_remove("AVIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_FORMAT")
        .env_remove("AVIKI_FORMAT");
    command.output().expect("run awiki-cli binary")
}

fn assert_success(output: &Output) {
    assert_code(output, 0);
}

fn assert_code(output: &Output, code: i32) {
    assert_eq!(
        output.status.code(),
        Some(code),
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
        serde_json::from_slice(&output.stdout).expect("stdout should be a JSON success envelope");
    assert_eq!(envelope["ok"], true, "success envelope should set ok=true");
    envelope
}

fn error_json(output: &Output) -> Value {
    assert!(
        output.stdout.is_empty(),
        "stdout should be empty, got {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let envelope: Value =
        serde_json::from_slice(&output.stderr).expect("stderr should be a JSON error envelope");
    assert_eq!(envelope["ok"], false, "error envelope should set ok=false");
    envelope
}

fn assert_contains(value: &Value, needle: &str) {
    let haystack = value
        .as_str()
        .unwrap_or_else(|| panic!("expected string containing {needle:?}, got {value:?}"));
    assert!(
        haystack.contains(needle),
        "expected {haystack:?} to contain {needle:?}"
    );
}

struct TempDir {
    path: PathBuf,
}

static NEXT_TEMP_DIR_ID: AtomicU64 = AtomicU64::new(0);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let id = NEXT_TEMP_DIR_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "awiki-cli-rs2-debug-test-{}-{nanos}-{id}",
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
