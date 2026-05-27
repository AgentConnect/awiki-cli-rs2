use crate::workspace_config::Paths;
use crate::workspace_upgrade::legacy_sqlite as store;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn rebind_local_identity_state_returns_zero_counts_when_database_missing() {
    let temp = TempDir::new("store-rebind-missing").expect("temp dir");
    let paths = test_paths(temp.path());

    let (store_rebind, e2ee_cleanup) =
        store::rebind_local_identity_state(&paths, "did:old", "did:new")
            .expect("missing database is a soft no-op");

    assert_eq!(store_rebind["messages"], 0);
    assert_eq!(store_rebind["contacts"], 0);
    assert_eq!(store_rebind["relationship_events"], 0);
    assert_eq!(store_rebind["groups"], 0);
    assert_eq!(store_rebind["group_members"], 0);
    assert!(
        !store_rebind.contains_key("contact_handle_bindings"),
        "Go RebindLocalIdentityState missing-DB no-op initializes only the five legacy store tables"
    );
    assert_eq!(e2ee_cleanup["e2ee_outbox"], 0);
    assert_eq!(e2ee_cleanup["e2ee_sessions"], 0);
}

#[test]
fn rebind_local_identity_state_rebinds_database_and_cleans_e2ee_data() {
    let temp = TempDir::new("store-rebind-main").expect("temp dir");
    let paths = test_paths(temp.path());
    let connection = store::open(&paths).expect("open database");
    store::ensure_schema(&connection).expect("schema");
    seed_rebind_rows(&connection).expect("seed rows");
    drop(connection);

    let (store_rebind, e2ee_cleanup) =
        store::rebind_local_identity_state(&paths, " did:old ", " did:new ")
            .expect("rebind identity state");

    assert_eq!(store_rebind["messages"], 1);
    assert_eq!(store_rebind["contacts"], 1);
    assert_eq!(store_rebind["contact_handle_bindings"], 1);
    assert_eq!(store_rebind["relationship_events"], 1);
    assert_eq!(store_rebind["groups"], 1);
    assert_eq!(store_rebind["group_members"], 1);
    assert_eq!(e2ee_cleanup["e2ee_outbox"], 1);
    assert_eq!(e2ee_cleanup["e2ee_sessions"], 1);

    let verify = store::open_read_only(&paths.database_file).expect("open read-only database");
    assert_owner_count(&verify, "messages", "did:new", 1);
    assert_owner_count(&verify, "contacts", "did:new", 1);
    assert_owner_count(&verify, "contact_handle_bindings", "did:new", 1);
    assert_owner_count(&verify, "relationship_events", "did:new", 1);
    assert_owner_count(&verify, "groups", "did:new", 1);
    assert_owner_count(&verify, "group_members", "did:new", 1);
    assert_owner_count(&verify, "messages", "did:old", 0);
    assert_owner_count(&verify, "e2ee_outbox", "did:old", 0);
    assert_owner_count(&verify, "e2ee_sessions", "did:old", 0);
    assert_owner_count(&verify, "e2ee_outbox", "did:new", 1);
    assert_owner_count(&verify, "e2ee_sessions", "did:new", 1);

    let content: String = verify
        .query_row(
            "SELECT content FROM messages WHERE msg_id = ?1 AND owner_did = ?2",
            rusqlite::params!["msg-1", "did:new"],
            |row| row.get(0),
        )
        .expect("rebinding keeps message content");
    assert_eq!(content, "hello");
}

#[test]
fn rebind_owner_did_counts_old_rows_before_update_or_ignore_conflicts() {
    let temp = TempDir::new("store-rebind-conflict").expect("temp dir");
    let paths = test_paths(temp.path());
    let mut connection = store::open(&paths).expect("open database");
    store::ensure_schema(&connection).expect("schema");
    seed_rebind_rows(&connection).expect("seed old rows");
    seed_new_owner_conflict_rows(&connection).expect("seed new owner conflicts");

    let store_rebind =
        store::rebind_owner_did(&mut connection, "did:old", "did:new").expect("rebind owner");

    assert_eq!(store_rebind["messages"], 1);
    assert_eq!(store_rebind["contacts"], 1);
    assert_eq!(store_rebind["contact_handle_bindings"], 1);
    assert_eq!(store_rebind["relationship_events"], 1);
    assert_eq!(store_rebind["groups"], 1);
    assert_eq!(store_rebind["group_members"], 1);

    assert_owner_count(&connection, "messages", "did:old", 1);
    assert_owner_count(&connection, "contacts", "did:old", 1);
    assert_owner_count(&connection, "contact_handle_bindings", "did:old", 1);
    assert_owner_count(&connection, "relationship_events", "did:old", 0);
    assert_owner_count(&connection, "groups", "did:old", 1);
    assert_owner_count(&connection, "group_members", "did:old", 1);
    assert_owner_count(&connection, "messages", "did:new", 1);
    assert_owner_count(&connection, "relationship_events", "did:new", 2);
}

#[test]
fn rebind_owner_did_and_e2ee_cleanup_are_noops_for_empty_or_same_owner() {
    let temp = TempDir::new("store-rebind-noop").expect("temp dir");
    let paths = test_paths(temp.path());
    let mut connection = store::open(&paths).expect("open database");
    store::ensure_schema(&connection).expect("schema");
    seed_rebind_rows(&connection).expect("seed rows");

    let same =
        store::rebind_owner_did(&mut connection, " did:old ", "did:old").expect("same owner");
    assert!(same.values().all(|count| *count == 0));
    assert_owner_count(&connection, "messages", "did:old", 1);

    let empty = store::clear_owner_e2ee_data(&connection, " ").expect("empty owner cleanup");
    assert!(empty.values().all(|count| *count == 0));
    assert_owner_count(&connection, "e2ee_outbox", "did:old", 1);
    assert_owner_count(&connection, "e2ee_sessions", "did:old", 1);
}

#[test]
fn rebind_local_identity_state_existing_empty_database_propagates_missing_table_error() {
    let temp = TempDir::new("store-rebind-empty-db").expect("temp dir");
    let paths = test_paths(temp.path());
    std::fs::create_dir_all(Path::new(&paths.database_file).parent().unwrap()).expect("data dir");
    drop(rusqlite::Connection::open(&paths.database_file).expect("create empty sqlite"));

    let err = store::rebind_local_identity_state(&paths, "did:old", "did:new")
        .expect_err("existing DB without store tables should fail like Go");

    assert!(
        err.to_string().contains("no such table: messages"),
        "error: {err}"
    );
}

fn seed_rebind_rows(connection: &rusqlite::Connection) -> rusqlite::Result<()> {
    connection.execute(
        r#"
INSERT INTO messages (
    msg_id, owner_did, thread_id, direction, sender_did, receiver_did, content_type,
    content, stored_at, credential_name
) VALUES (?1, ?2, ?3, 0, ?4, ?2, 'text', ?5, ?6, 'default')
"#,
        rusqlite::params![
            "msg-1",
            "did:old",
            "dm:did:old:did:peer",
            "did:peer",
            "hello",
            "2026-01-01T00:00:00Z",
        ],
    )?;
    connection.execute(
        r#"
INSERT INTO contacts (
    owner_did, did, handle, first_seen_at, last_seen_at
) VALUES (?1, ?2, ?3, ?4, ?5)
"#,
        rusqlite::params![
            "did:old",
            "did:peer",
            "alice",
            "2026-01-01T00:00:00Z",
            "2026-01-02T00:00:00Z",
        ],
    )?;
    connection.execute(
        r#"
INSERT INTO contact_handle_bindings (
    owner_did, handle, did, is_current, first_seen_at, last_seen_at, credential_name
) VALUES (?1, ?2, ?3, 1, ?4, ?5, 'default')
"#,
        rusqlite::params![
            "did:old",
            "alice",
            "did:peer",
            "2026-01-01T00:00:00Z",
            "2026-01-02T00:00:00Z",
        ],
    )?;
    connection.execute(
        r#"
INSERT INTO relationship_events (
    event_id, owner_did, target_did, event_type, status, created_at, updated_at, credential_name
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'default')
"#,
        rusqlite::params![
            "event-1",
            "did:old",
            "did:peer",
            "recommended",
            "pending",
            "2026-01-01T00:00:00Z",
            "2026-01-01T00:00:00Z",
        ],
    )?;
    connection.execute(
        r#"
INSERT INTO groups (
    owner_did, group_id, name, group_mode, membership_status, stored_at, credential_name
) VALUES (?1, ?2, ?3, 'general', 'active', ?4, 'default')
"#,
        rusqlite::params!["did:old", "group-1", "Group One", "2026-01-01T00:00:00Z",],
    )?;
    connection.execute(
        r#"
INSERT INTO group_members (
    owner_did, group_id, user_id, member_did, status, last_synced_at, credential_name
) VALUES (?1, ?2, ?3, ?4, 'active', ?5, 'default')
"#,
        rusqlite::params![
            "did:old",
            "group-1",
            "user-1",
            "did:peer",
            "2026-01-01T00:00:00Z",
        ],
    )?;
    connection.execute(
        r#"
INSERT INTO e2ee_outbox (
    outbox_id, owner_did, peer_did, plaintext, created_at, updated_at, credential_name
) VALUES (?1, ?2, ?3, ?4, ?5, ?5, 'default')
"#,
        rusqlite::params![
            "out-1",
            "did:old",
            "did:peer",
            "secret",
            "2026-01-01T00:00:00Z",
        ],
    )?;
    connection.execute(
        r#"
INSERT INTO e2ee_sessions (
    owner_did, peer_did, session_id, is_initiator, send_chain_key, recv_chain_key,
    send_seq, recv_seq, expires_at, created_at, active_at, peer_confirmed,
    credential_name, updated_at
) VALUES (?1, ?2, ?3, 1, ?4, ?5, 0, 0, NULL, ?6, NULL, 0, 'default', ?6)
"#,
        rusqlite::params![
            "did:old",
            "did:peer",
            "session-1",
            "send-key",
            "recv-key",
            "2026-01-01T00:00:00Z",
        ],
    )?;
    connection.execute(
        r#"
INSERT INTO e2ee_outbox (
    outbox_id, owner_did, peer_did, plaintext, created_at, updated_at, credential_name
) VALUES (?1, ?2, ?3, ?4, ?5, ?5, 'default')
"#,
        rusqlite::params![
            "out-new",
            "did:new",
            "did:peer",
            "new secret",
            "2026-01-01T00:00:00Z",
        ],
    )?;
    connection.execute(
        r#"
INSERT INTO e2ee_sessions (
    owner_did, peer_did, session_id, is_initiator, send_chain_key, recv_chain_key,
    send_seq, recv_seq, expires_at, created_at, active_at, peer_confirmed,
    credential_name, updated_at
) VALUES (?1, ?2, ?3, 1, ?4, ?5, 0, 0, NULL, ?6, NULL, 0, 'default', ?6)
"#,
        rusqlite::params![
            "did:new",
            "did:peer",
            "session-new",
            "send-key-new",
            "recv-key-new",
            "2026-01-01T00:00:00Z",
        ],
    )?;
    Ok(())
}

fn seed_new_owner_conflict_rows(connection: &rusqlite::Connection) -> rusqlite::Result<()> {
    connection.execute(
        r#"
INSERT INTO messages (
    msg_id, owner_did, thread_id, direction, sender_did, receiver_did, content_type,
    content, stored_at, credential_name
) VALUES (?1, ?2, ?3, 0, ?4, ?2, 'text', ?5, ?6, 'default')
"#,
        rusqlite::params![
            "msg-1",
            "did:new",
            "dm:did:new:did:peer",
            "did:peer",
            "new owner message",
            "2026-01-01T00:00:00Z",
        ],
    )?;
    connection.execute(
        "INSERT INTO contacts (owner_did, did, handle) VALUES (?1, ?2, ?3)",
        rusqlite::params!["did:new", "did:peer", "alice-new"],
    )?;
    connection.execute(
        r#"
INSERT INTO contact_handle_bindings (
    owner_did, handle, did, is_current, first_seen_at, last_seen_at, credential_name
) VALUES (?1, ?2, ?3, 1, ?4, ?5, 'default')
"#,
        rusqlite::params![
            "did:new",
            "alice",
            "did:peer",
            "2026-01-01T00:00:00Z",
            "2026-01-02T00:00:00Z",
        ],
    )?;
    connection.execute(
        r#"
INSERT INTO relationship_events (
    event_id, owner_did, target_did, event_type, status, created_at, updated_at, credential_name
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'default')
"#,
        rusqlite::params![
            "event-2",
            "did:new",
            "did:peer",
            "recommended",
            "pending",
            "2026-01-01T00:00:00Z",
            "2026-01-01T00:00:00Z",
        ],
    )?;
    connection.execute(
        r#"
INSERT INTO groups (
    owner_did, group_id, name, group_mode, membership_status, stored_at, credential_name
) VALUES (?1, ?2, ?3, 'general', 'active', ?4, 'default')
"#,
        rusqlite::params![
            "did:new",
            "group-1",
            "Group One New",
            "2026-01-01T00:00:00Z",
        ],
    )?;
    connection.execute(
        r#"
INSERT INTO group_members (
    owner_did, group_id, user_id, member_did, status, last_synced_at, credential_name
) VALUES (?1, ?2, ?3, ?4, 'active', ?5, 'default')
"#,
        rusqlite::params![
            "did:new",
            "group-1",
            "user-1",
            "did:peer",
            "2026-01-01T00:00:00Z",
        ],
    )?;
    Ok(())
}

fn assert_owner_count(connection: &rusqlite::Connection, table: &str, owner_did: &str, want: i64) {
    let got: i64 = connection
        .query_row(
            &format!("SELECT COUNT(*) FROM {table} WHERE owner_did = ?1"),
            rusqlite::params![owner_did],
            |row| row.get(0),
        )
        .expect("owner count");
    assert_eq!(got, want, "owner count for {table}/{owner_did}");
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
