use crate::workspace_config::Paths;
use crate::workspace_upgrade::legacy_sqlite::{self as store, LegacyOwnerLookup};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const V6_TABLES_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS contacts (
    owner_did       TEXT NOT NULL DEFAULT '',
    did             TEXT NOT NULL,
    name            TEXT,
    handle          TEXT,
    nick_name       TEXT,
    bio             TEXT,
    profile_md      TEXT,
    tags            TEXT,
    relationship    TEXT,
    source_type     TEXT,
    source_name     TEXT,
    source_group_id TEXT,
    connected_at    TEXT,
    recommended_reason TEXT,
    followed        INTEGER NOT NULL DEFAULT 0,
    messaged        INTEGER NOT NULL DEFAULT 0,
    note            TEXT,
    first_seen_at   TEXT,
    last_seen_at    TEXT,
    metadata        TEXT,
    PRIMARY KEY (owner_did, did)
);

CREATE TABLE IF NOT EXISTS messages (
    msg_id          TEXT NOT NULL,
    owner_did       TEXT NOT NULL DEFAULT '',
    thread_id       TEXT NOT NULL,
    direction       INTEGER NOT NULL DEFAULT 0,
    sender_did      TEXT,
    receiver_did    TEXT,
    group_id        TEXT,
    group_did       TEXT,
    content_type    TEXT DEFAULT 'text',
    content         TEXT,
    title           TEXT,
    server_seq      INTEGER,
    sent_at         TEXT,
    stored_at       TEXT NOT NULL,
    is_e2ee         INTEGER DEFAULT 0,
    is_read         INTEGER DEFAULT 0,
    sender_name     TEXT,
    metadata        TEXT,
    credential_name TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (msg_id, owner_did)
);

CREATE TABLE IF NOT EXISTS e2ee_outbox (
    outbox_id            TEXT PRIMARY KEY,
    owner_did            TEXT NOT NULL DEFAULT '',
    peer_did             TEXT NOT NULL,
    session_id           TEXT,
    original_type        TEXT NOT NULL DEFAULT 'text',
    plaintext            TEXT NOT NULL,
    local_status         TEXT NOT NULL DEFAULT 'queued',
    attempt_count        INTEGER NOT NULL DEFAULT 0,
    sent_msg_id          TEXT,
    sent_server_seq      INTEGER,
    last_error_code      TEXT,
    retry_hint           TEXT,
    failed_msg_id        TEXT,
    failed_server_seq    INTEGER,
    metadata             TEXT,
    last_attempt_at      TEXT,
    created_at           TEXT NOT NULL,
    updated_at           TEXT NOT NULL,
    credential_name      TEXT NOT NULL DEFAULT ''
);
"#;

const V11_EXTRA_TABLES_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS e2ee_sessions (
    owner_did        TEXT NOT NULL DEFAULT '',
    peer_did         TEXT NOT NULL,
    session_id       TEXT NOT NULL,
    is_initiator     INTEGER NOT NULL DEFAULT 0,
    send_chain_key   TEXT NOT NULL,
    recv_chain_key   TEXT NOT NULL,
    send_seq         INTEGER NOT NULL DEFAULT 0,
    recv_seq         INTEGER NOT NULL DEFAULT 0,
    expires_at       REAL,
    created_at       TEXT NOT NULL,
    active_at        TEXT,
    peer_confirmed   INTEGER NOT NULL DEFAULT 0,
    credential_name  TEXT NOT NULL DEFAULT '',
    updated_at       TEXT NOT NULL,
    PRIMARY KEY (owner_did, peer_did),
    UNIQUE (owner_did, session_id)
);
"#;

#[test]
fn import_legacy_database_v11_imports_messages_and_contacts() {
    let temp = TempDir::new("store-import-v11").expect("temp dir");
    let paths = test_paths(temp.path());
    let legacy_db_path = Path::new(&paths.legacy_data_dir)
        .join("database")
        .join("awiki.db");
    std::fs::create_dir_all(legacy_db_path.parent().unwrap()).expect("legacy dir");

    {
        let legacy = rusqlite::Connection::open(&legacy_db_path).expect("open legacy");
        legacy.execute_batch(V6_TABLES_SQL).expect("v6 schema");
        legacy
            .execute_batch(V11_EXTRA_TABLES_SQL)
            .expect("v11 schema");
        legacy
            .pragma_update(None, "user_version", 11)
            .expect("user_version");
        legacy
            .execute(
                r#"
INSERT INTO messages (
    msg_id, owner_did, thread_id, direction, sender_did, receiver_did, content_type,
    content, stored_at, credential_name
) VALUES (?1, ?2, ?3, 0, ?4, ?2, 'text', ?5, ?6, 'legacy')
"#,
                rusqlite::params![
                    "legacy-msg",
                    "did:wba:awiki.ai:user:legacy",
                    "dm:did:wba:awiki.ai:user:legacy:did:wba:awiki.ai:user:peer",
                    "did:wba:awiki.ai:user:peer",
                    "legacy hello",
                    "2026-01-01T00:00:00Z",
                ],
            )
            .expect("insert legacy message");
        legacy
            .execute(
                "INSERT INTO contacts (owner_did, did, name) VALUES (?1, ?2, ?3)",
                rusqlite::params![
                    "did:wba:awiki.ai:user:legacy",
                    "did:wba:awiki.ai:user:peer",
                    "Legacy Peer",
                ],
            )
            .expect("insert legacy contact");
    }

    let mut target = store::open(&paths).expect("open target");
    let owners = LegacyOwnerLookup::from_entries([(
        "legacy".to_string(),
        "did:wba:awiki.ai:user:legacy".to_string(),
        true,
    )]);

    let report =
        store::import_legacy_database(&mut target, &paths, &owners).expect("import legacy");

    assert_eq!(report.imported_rows["messages"], 1);
    assert_eq!(report.imported_rows["contacts"], 1);

    let content: String = target
        .query_row(
            "SELECT content FROM messages WHERE msg_id = ?1 AND owner_did = ?2",
            rusqlite::params!["legacy-msg", "did:wba:awiki.ai:user:legacy"],
            |row| row.get(0),
        )
        .expect("imported message");
    assert_eq!(content, "legacy hello");
}

#[test]
fn import_legacy_database_skips_missing_tables_and_infers_owner_from_credential() {
    let temp = TempDir::new("store-import-v6").expect("temp dir");
    let paths = test_paths(temp.path());
    let legacy_db_path = Path::new(&paths.legacy_data_dir)
        .join("database")
        .join("awiki.db");
    std::fs::create_dir_all(legacy_db_path.parent().unwrap()).expect("legacy dir");

    {
        let legacy = rusqlite::Connection::open(&legacy_db_path).expect("open legacy");
        legacy.execute_batch(V6_TABLES_SQL).expect("v6 schema");
        legacy
            .pragma_update(None, "user_version", 6)
            .expect("user_version");
        legacy
            .execute(
                r#"
INSERT INTO messages
    (msg_id, owner_did, thread_id, direction, sender_did, receiver_did, content_type, content, is_read, credential_name, stored_at)
VALUES (?1, '', '', 0, ?2, '', '', ?3, 0, 'legacy', ?4)
"#,
                rusqlite::params![
                    "legacy-msg",
                    "did:peer",
                    "legacy hello",
                    "2026-01-01T00:00:00Z",
                ],
            )
            .expect("insert legacy message");
        legacy
            .execute(
                r#"
INSERT INTO contacts
    (owner_did, did, handle, first_seen_at, last_seen_at)
VALUES ('', ?1, ?2, ?3, ?4)
"#,
                rusqlite::params![
                    "did:peer",
                    "alice",
                    "2026-01-01T00:00:00Z",
                    "2026-01-02T00:00:00Z",
                ],
            )
            .expect("insert legacy contact");
    }

    let mut target = store::open(&paths).expect("open target");
    let owners =
        LegacyOwnerLookup::from_entries([("legacy".to_string(), "did:owner".to_string(), true)]);

    let report =
        store::import_legacy_database(&mut target, &paths, &owners).expect("import legacy");

    assert_eq!(report.imported_rows["messages"], 1);
    assert_eq!(report.imported_rows["contacts"], 1);
    assert_eq!(
        report.skipped_tables,
        ["group_members", "groups", "relationship_events"]
    );

    let (owner_did, thread_id, content_type): (String, String, String) = target
        .query_row(
            "SELECT owner_did, thread_id, content_type FROM messages WHERE msg_id = ?1",
            ["legacy-msg"],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("imported message");
    assert_eq!(owner_did, "did:owner");
    assert_eq!(thread_id, "dm:did:owner:did:peer");
    assert_eq!(content_type, "text");

    let handle: String = target
        .query_row(
            "SELECT handle FROM contact_handle_bindings WHERE owner_did = ?1 AND did = ?2 AND is_current = 1",
            rusqlite::params!["did:owner", "did:peer"],
            |row| row.get(0),
        )
        .expect("contact handle binding");
    assert_eq!(handle, "alice");
}

#[test]
fn import_legacy_database_uses_identity_unique_id_as_owner_identity_id() {
    let temp = TempDir::new("store-import-owner-identity").expect("temp dir");
    let paths = test_paths(temp.path());
    let legacy_db_path = Path::new(&paths.legacy_data_dir)
        .join("database")
        .join("awiki.db");
    std::fs::create_dir_all(legacy_db_path.parent().unwrap()).expect("legacy dir");

    {
        let legacy = rusqlite::Connection::open(&legacy_db_path).expect("open legacy");
        legacy.execute_batch(V6_TABLES_SQL).expect("v6 schema");
        legacy
            .pragma_update(None, "user_version", 6)
            .expect("user_version");
        legacy
            .execute(
                r#"
INSERT INTO messages
    (msg_id, owner_did, thread_id, direction, sender_did, receiver_did, content_type, content, is_read, credential_name, stored_at)
VALUES (?1, '', '', 0, ?2, '', '', ?3, 0, 'alice', ?4)
"#,
                rusqlite::params![
                    "legacy-msg",
                    "did:peer",
                    "legacy hello",
                    "2026-01-01T00:00:00Z",
                ],
            )
            .expect("insert legacy message");
    }

    let mut target = store::open(&paths).expect("open target");
    let owners = LegacyOwnerLookup::from_identity_entries([(
        "e1_alice".to_string(),
        "alice".to_string(),
        "did:owner:alice".to_string(),
        true,
    )]);

    store::import_legacy_database(&mut target, &paths, &owners).expect("import legacy");

    let (owner_identity_id, owner_did, credential_name): (String, String, String) = target
        .query_row(
            "SELECT owner_identity_id, owner_did, credential_name FROM messages WHERE msg_id = ?1",
            ["legacy-msg"],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("imported message");
    assert_eq!(owner_identity_id, "e1_alice");
    assert_eq!(owner_did, "did:owner:alice");
    assert_eq!(credential_name, "alice");
}

#[test]
fn import_legacy_database_rejects_unknown_explicit_owner_did() {
    let temp = TempDir::new("store-import-unknown-owner").expect("temp dir");
    let paths = test_paths(temp.path());
    let legacy_db_path = Path::new(&paths.legacy_data_dir)
        .join("database")
        .join("awiki.db");
    std::fs::create_dir_all(legacy_db_path.parent().unwrap()).expect("legacy dir");

    {
        let legacy = rusqlite::Connection::open(&legacy_db_path).expect("open legacy");
        legacy.execute_batch(V6_TABLES_SQL).expect("v6 schema");
        legacy
            .pragma_update(None, "user_version", 6)
            .expect("user_version");
        legacy
            .execute(
                r#"
INSERT INTO messages
    (msg_id, owner_did, thread_id, direction, sender_did, receiver_did, content_type, content, is_read, credential_name, stored_at)
VALUES (?1, 'did:owner:unknown', '', 0, ?2, '', '', ?3, 0, '', ?4)
"#,
                rusqlite::params![
                    "legacy-msg",
                    "did:peer",
                    "legacy hello",
                    "2026-01-01T00:00:00Z",
                ],
            )
            .expect("insert legacy message");
    }

    let mut target = store::open(&paths).expect("open target");
    let owners = LegacyOwnerLookup::from_identity_entries([(
        "e1_alice".to_string(),
        "alice".to_string(),
        "did:owner:alice".to_string(),
        true,
    )]);

    let err = store::import_legacy_database(&mut target, &paths, &owners)
        .expect_err("unknown explicit owner must fail closed");
    assert_contains(
        &err.to_string(),
        "legacy row owner_did could not be resolved to owner_identity_id",
    );
}

#[test]
fn import_legacy_database_rejects_pre_v6_schema_without_imported_identity() {
    let temp = TempDir::new("store-import-v5").expect("temp dir");
    let mut paths = test_paths(temp.path());
    let legacy_db_path = temp.path().join("legacy-v5.db");
    {
        let legacy = rusqlite::Connection::open(&legacy_db_path).expect("open legacy");
        legacy
            .pragma_update(None, "user_version", 5)
            .expect("user_version");
    }
    paths.legacy_data_dir = path_string(&legacy_db_path);
    let mut target = store::open(&paths).expect("open target");

    let err = store::import_legacy_database(&mut target, &paths, &LegacyOwnerLookup::default())
        .expect_err("pre-v6 import without identity should fail");

    assert!(
        err.to_string()
            .contains("unsupported legacy sqlite schema version"),
        "error: {err}"
    );
    assert!(
        err.to_string().contains("legacy schema < 6 requires"),
        "error: {err}"
    );
}

fn assert_contains(haystack: &str, needle: &str) {
    assert!(
        haystack.contains(needle),
        "expected {haystack:?} to contain {needle:?}"
    );
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
