use rusqlite::Connection;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) const SCHEMA_VERSION: i64 = 15;

const V6_TABLES_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS contacts (
    owner_identity_id TEXT,
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
    credential_name TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (owner_did, did)
);

CREATE TABLE IF NOT EXISTS messages (
    msg_id          TEXT NOT NULL,
    owner_identity_id TEXT,
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
    owner_identity_id    TEXT,
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

const V7_TABLES_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS groups (
    owner_identity_id TEXT,
    owner_did          TEXT NOT NULL DEFAULT '',
    group_id           TEXT NOT NULL,
    group_did          TEXT,
    name               TEXT,
    group_mode         TEXT NOT NULL DEFAULT 'general',
    slug               TEXT,
    description        TEXT,
    goal               TEXT,
    rules              TEXT,
    message_prompt     TEXT,
    doc_url            TEXT,
    group_owner_did    TEXT,
    group_owner_handle TEXT,
    my_role            TEXT,
    membership_status  TEXT NOT NULL DEFAULT 'active',
    join_enabled       INTEGER,
    join_code          TEXT,
    join_code_expires_at TEXT,
    member_count       INTEGER,
    last_synced_seq    INTEGER,
    last_read_seq      INTEGER,
    last_message_at    TEXT,
    remote_created_at  TEXT,
    remote_updated_at  TEXT,
    stored_at          TEXT NOT NULL,
    metadata           TEXT,
    credential_name    TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (owner_did, group_id)
);

CREATE TABLE IF NOT EXISTS group_members (
    owner_identity_id TEXT,
    owner_did         TEXT NOT NULL DEFAULT '',
    group_id          TEXT NOT NULL,
    user_id           TEXT NOT NULL,
    member_did        TEXT,
    member_handle     TEXT,
    profile_url       TEXT,
    role              TEXT,
    status            TEXT NOT NULL DEFAULT 'active',
    joined_at         TEXT,
    sent_message_count INTEGER NOT NULL DEFAULT 0,
    last_synced_at    TEXT NOT NULL,
    metadata          TEXT,
    credential_name   TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (owner_did, group_id, user_id)
);
"#;

const V8_TABLES_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS relationship_events (
    event_id         TEXT PRIMARY KEY,
    owner_identity_id TEXT,
    owner_did        TEXT NOT NULL DEFAULT '',
    target_did       TEXT NOT NULL,
    target_handle    TEXT,
    event_type       TEXT NOT NULL,
    source_type      TEXT,
    source_name      TEXT,
    source_group_id  TEXT,
    reason           TEXT,
    score            REAL,
    status           TEXT NOT NULL DEFAULT 'pending',
    created_at       TEXT NOT NULL,
    updated_at       TEXT NOT NULL,
    metadata         TEXT,
    credential_name  TEXT NOT NULL DEFAULT ''
);
"#;

const V11_TABLES_SQL: &str = r#"
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

const V12_TABLES_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS contact_handle_bindings (
    owner_identity_id TEXT,
    owner_did        TEXT NOT NULL DEFAULT '',
    handle           TEXT NOT NULL,
    did              TEXT NOT NULL,
    is_current       INTEGER NOT NULL DEFAULT 1,
    first_seen_at    TEXT NOT NULL,
    last_seen_at     TEXT NOT NULL,
    source_type      TEXT,
    source_group_id  TEXT,
    metadata         TEXT,
    credential_name  TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (owner_did, handle, did)
);
"#;

const V14_TABLES_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS direct_e2ee_sessions (
    owner_identity_id TEXT NOT NULL,
    owner_did         TEXT NOT NULL DEFAULT '',
    peer_did          TEXT NOT NULL,
    session_id        TEXT NOT NULL,
    state_blob        BLOB NOT NULL,
    metadata_json     TEXT,
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL,
    PRIMARY KEY (owner_identity_id, peer_did),
    UNIQUE (owner_identity_id, session_id)
);

CREATE TABLE IF NOT EXISTS direct_e2ee_signed_prekeys (
    owner_identity_id TEXT NOT NULL,
    owner_did         TEXT NOT NULL DEFAULT '',
    key_id            TEXT NOT NULL,
    private_key_blob  BLOB NOT NULL,
    public_key_blob   BLOB,
    status            TEXT NOT NULL DEFAULT 'active',
    metadata_json     TEXT,
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL,
    PRIMARY KEY (owner_identity_id, key_id)
);

CREATE TABLE IF NOT EXISTS direct_e2ee_one_time_prekeys (
    owner_identity_id TEXT NOT NULL,
    owner_did         TEXT NOT NULL DEFAULT '',
    key_id            TEXT NOT NULL,
    private_key_blob  BLOB NOT NULL,
    public_key_blob   BLOB,
    status            TEXT NOT NULL DEFAULT 'available',
    metadata_json     TEXT,
    created_at        TEXT NOT NULL,
    consumed_at       TEXT,
    PRIMARY KEY (owner_identity_id, key_id)
);
"#;

const INDEX_STATEMENTS: &[&str] = &[
    "CREATE INDEX IF NOT EXISTS idx_contacts_owner_identity ON contacts(owner_identity_id, last_seen_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_contacts_owner ON contacts(owner_did, last_seen_at DESC)",
    "CREATE UNIQUE INDEX IF NOT EXISTS idx_contact_handle_bindings_owner_identity_handle_current_unique ON contact_handle_bindings(owner_identity_id, handle) WHERE owner_identity_id IS NOT NULL AND is_current = 1",
    "CREATE INDEX IF NOT EXISTS idx_contact_handle_bindings_owner_identity_did ON contact_handle_bindings(owner_identity_id, did, last_seen_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_contact_handle_bindings_owner_identity_handle ON contact_handle_bindings(owner_identity_id, handle, last_seen_at DESC)",
    "CREATE UNIQUE INDEX IF NOT EXISTS idx_contact_handle_bindings_owner_handle_current_unique ON contact_handle_bindings(owner_did, handle) WHERE is_current = 1",
    "CREATE INDEX IF NOT EXISTS idx_contact_handle_bindings_owner_did ON contact_handle_bindings(owner_did, did, last_seen_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_contact_handle_bindings_owner_handle ON contact_handle_bindings(owner_did, handle, last_seen_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_messages_owner_identity_thread ON messages(owner_identity_id, thread_id, sent_at)",
    "CREATE INDEX IF NOT EXISTS idx_messages_owner_identity_thread_seq ON messages(owner_identity_id, thread_id, server_seq)",
    "CREATE INDEX IF NOT EXISTS idx_messages_owner_identity_direction ON messages(owner_identity_id, direction)",
    "CREATE INDEX IF NOT EXISTS idx_messages_owner_identity_sender ON messages(owner_identity_id, sender_did)",
    "CREATE INDEX IF NOT EXISTS idx_messages_owner_identity ON messages(owner_identity_id)",
    "CREATE INDEX IF NOT EXISTS idx_messages_owner_thread ON messages(owner_did, thread_id, sent_at)",
    "CREATE INDEX IF NOT EXISTS idx_messages_owner_thread_seq ON messages(owner_did, thread_id, server_seq)",
    "CREATE INDEX IF NOT EXISTS idx_messages_owner_direction ON messages(owner_did, direction)",
    "CREATE INDEX IF NOT EXISTS idx_messages_owner_sender ON messages(owner_did, sender_did)",
    "CREATE INDEX IF NOT EXISTS idx_messages_owner ON messages(owner_did)",
    "CREATE INDEX IF NOT EXISTS idx_messages_credential ON messages(credential_name)",
    "CREATE INDEX IF NOT EXISTS idx_e2ee_outbox_owner_identity_status ON e2ee_outbox(owner_identity_id, local_status, updated_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_e2ee_outbox_owner_identity_sent_msg ON e2ee_outbox(owner_identity_id, sent_msg_id)",
    "CREATE INDEX IF NOT EXISTS idx_e2ee_outbox_owner_status ON e2ee_outbox(owner_did, local_status, updated_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_e2ee_outbox_owner_sent_msg ON e2ee_outbox(owner_did, sent_msg_id)",
    "CREATE INDEX IF NOT EXISTS idx_e2ee_outbox_owner_sent_seq ON e2ee_outbox(owner_did, peer_did, sent_server_seq)",
    "CREATE INDEX IF NOT EXISTS idx_e2ee_outbox_credential ON e2ee_outbox(credential_name)",
    "CREATE INDEX IF NOT EXISTS idx_groups_owner_identity_status_last_message ON groups(owner_identity_id, membership_status, last_message_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_groups_owner_identity_slug ON groups(owner_identity_id, slug)",
    "CREATE INDEX IF NOT EXISTS idx_groups_owner_identity_updated ON groups(owner_identity_id, remote_updated_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_groups_owner_status_last_message ON groups(owner_did, membership_status, last_message_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_groups_owner_slug ON groups(owner_did, slug)",
    "CREATE INDEX IF NOT EXISTS idx_groups_owner_updated ON groups(owner_did, remote_updated_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_group_members_owner_identity_group_role ON group_members(owner_identity_id, group_id, role)",
    "CREATE INDEX IF NOT EXISTS idx_group_members_owner_identity_group_status ON group_members(owner_identity_id, group_id, status)",
    "CREATE INDEX IF NOT EXISTS idx_group_members_owner_group_role ON group_members(owner_did, group_id, role)",
    "CREATE INDEX IF NOT EXISTS idx_group_members_owner_group_status ON group_members(owner_did, group_id, status)",
    "CREATE INDEX IF NOT EXISTS idx_contacts_owner_identity_source_group ON contacts(owner_identity_id, source_group_id)",
    "CREATE INDEX IF NOT EXISTS idx_contacts_owner_source_group ON contacts(owner_did, source_group_id)",
    "CREATE INDEX IF NOT EXISTS idx_relationship_events_owner_identity_target_time ON relationship_events(owner_identity_id, target_did, created_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_relationship_events_owner_identity_status_time ON relationship_events(owner_identity_id, status, created_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_relationship_events_owner_identity_group ON relationship_events(owner_identity_id, source_group_id)",
    "CREATE INDEX IF NOT EXISTS idx_relationship_events_owner_target_time ON relationship_events(owner_did, target_did, created_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_relationship_events_owner_status_time ON relationship_events(owner_did, status, created_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_relationship_events_owner_group ON relationship_events(owner_did, source_group_id)",
    "CREATE INDEX IF NOT EXISTS idx_e2ee_sessions_owner_updated ON e2ee_sessions(owner_did, updated_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_e2ee_sessions_credential ON e2ee_sessions(credential_name)",
    "CREATE INDEX IF NOT EXISTS idx_direct_e2ee_sessions_owner_updated ON direct_e2ee_sessions(owner_identity_id, updated_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_direct_e2ee_signed_prekeys_owner_status ON direct_e2ee_signed_prekeys(owner_identity_id, status, updated_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_direct_e2ee_one_time_prekeys_owner_status ON direct_e2ee_one_time_prekeys(owner_identity_id, status, created_at ASC)",
];

const VIEW_STATEMENTS: &[&str] = &[
    r#"CREATE VIEW IF NOT EXISTS threads AS
SELECT
    owner_identity_id,
    owner_did,
    thread_id,
    COUNT(*) AS message_count,
    SUM(CASE WHEN is_read = 0 AND direction = 0 THEN 1 ELSE 0 END) AS unread_count,
    MAX(COALESCE(sent_at, stored_at)) AS last_message_at,
    (SELECT m2.content FROM messages m2
     WHERE COALESCE(m2.owner_identity_id, '') = COALESCE(m.owner_identity_id, '')
       AND m2.owner_did = m.owner_did
       AND m2.thread_id = m.thread_id
     ORDER BY COALESCE(m2.sent_at, m2.stored_at) DESC
     LIMIT 1) AS last_content
FROM messages m
GROUP BY owner_identity_id, owner_did, thread_id"#,
    r#"CREATE VIEW IF NOT EXISTS inbox AS
SELECT * FROM messages WHERE direction = 0
ORDER BY owner_did, COALESCE(sent_at, stored_at) DESC"#,
    r#"CREATE VIEW IF NOT EXISTS outbox AS
SELECT * FROM messages WHERE direction = 1
ORDER BY owner_did, COALESCE(sent_at, stored_at) DESC"#,
];

pub(crate) fn ensure_schema(connection: &Connection) -> crate::ImResult<()> {
    let version = current_schema_version(connection)?;
    if version == 0 {
        create_schema(connection)?;
        return set_schema_version(connection, SCHEMA_VERSION);
    }
    if version > SCHEMA_VERSION {
        return Err(crate::ImError::LocalStateUnavailable {
            detail: format!(
                "sqlite schema version {version} is newer than supported {SCHEMA_VERSION}"
            ),
        });
    }
    if version < 6 {
        return Err(crate::ImError::LocalStateUnavailable {
            detail: format!("sqlite schema version {version} is too old for in-place upgrade"),
        });
    }
    create_schema(connection)?;
    set_schema_version(connection, SCHEMA_VERSION)
}

pub(crate) fn current_schema_version(connection: &Connection) -> crate::ImResult<i64> {
    connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(super::local_state_unavailable)
}

fn create_schema(connection: &Connection) -> crate::ImResult<()> {
    for script in [
        V6_TABLES_SQL,
        V7_TABLES_SQL,
        V8_TABLES_SQL,
        V11_TABLES_SQL,
        V12_TABLES_SQL,
        V14_TABLES_SQL,
    ] {
        connection
            .execute_batch(script)
            .map_err(super::local_state_unavailable)?;
    }
    ensure_owner_identity_columns(connection)?;
    backfill_contact_handle_bindings(connection)?;
    for statement in INDEX_STATEMENTS {
        connection
            .execute(statement, [])
            .map_err(super::local_state_unavailable)?;
    }
    for view in ["threads", "inbox", "outbox"] {
        connection
            .execute(&format!("DROP VIEW IF EXISTS {view}"), [])
            .map_err(super::local_state_unavailable)?;
    }
    for statement in VIEW_STATEMENTS {
        connection
            .execute(statement, [])
            .map_err(super::local_state_unavailable)?;
    }
    Ok(())
}

fn ensure_owner_identity_columns(connection: &Connection) -> crate::ImResult<()> {
    for table in [
        "contacts",
        "contact_handle_bindings",
        "messages",
        "e2ee_outbox",
        "groups",
        "group_members",
        "relationship_events",
    ] {
        ensure_column(connection, table, "owner_identity_id", "TEXT")?;
    }
    ensure_column(
        connection,
        "contacts",
        "credential_name",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    Ok(())
}

#[derive(Debug, Clone)]
pub(crate) struct OwnerIdentityBackfill {
    pub(crate) identity_id: String,
    pub(crate) owner_did: String,
    pub(crate) credential_names: Vec<String>,
}

pub(crate) fn backfill_owner_identity_ids(
    connection: &Connection,
    identities: &[OwnerIdentityBackfill],
) -> crate::ImResult<usize> {
    let mut updated = 0;
    for table in [
        "contacts",
        "contact_handle_bindings",
        "messages",
        "e2ee_outbox",
        "groups",
        "group_members",
        "relationship_events",
    ] {
        for identity in identities {
            let identity_id = identity.identity_id.trim();
            if identity_id.is_empty() {
                continue;
            }
            for credential_name in identity.credential_names.iter().map(|value| value.trim()) {
                if credential_name.is_empty() {
                    continue;
                }
                updated += connection
                    .execute(
                        &format!(
                            r#"
UPDATE {table}
SET owner_identity_id = ?1
WHERE (owner_identity_id IS NULL OR TRIM(owner_identity_id) = '')
  AND TRIM(COALESCE(credential_name, '')) = ?2"#
                        ),
                        rusqlite::params![identity_id, credential_name],
                    )
                    .map_err(super::local_state_unavailable)?;
            }
        }
        for identity in identities {
            let identity_id = identity.identity_id.trim();
            let owner_did = identity.owner_did.trim();
            if identity_id.is_empty() || owner_did.is_empty() {
                continue;
            }
            updated += connection
                .execute(
                    &format!(
                        r#"
UPDATE {table}
SET owner_identity_id = ?1
WHERE (owner_identity_id IS NULL OR TRIM(owner_identity_id) = '')
  AND owner_did = ?2"#
                    ),
                    rusqlite::params![identity_id, owner_did],
                )
                .map_err(super::local_state_unavailable)?;
        }
    }
    Ok(updated)
}

fn ensure_column(
    connection: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> crate::ImResult<()> {
    if has_column(connection, table, column)? {
        return Ok(());
    }
    connection
        .execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
            [],
        )
        .map_err(super::local_state_unavailable)?;
    Ok(())
}

fn has_column(connection: &Connection, table: &str, column: &str) -> crate::ImResult<bool> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(super::local_state_unavailable)?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(super::local_state_unavailable)?;
    for row in rows {
        if row.map_err(super::local_state_unavailable)? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn backfill_contact_handle_bindings(connection: &Connection) -> crate::ImResult<()> {
    let now = now_utc_like();
    connection
        .execute(
            r#"
INSERT INTO contact_handle_bindings
    (owner_identity_id, owner_did, handle, did, is_current, first_seen_at, last_seen_at, source_type, source_group_id, metadata, credential_name)
SELECT owner_identity_id,
       owner_did,
       handle,
       did,
       0,
       COALESCE(first_seen_at, ?1),
       COALESCE(last_seen_at, ?1),
       source_type,
       source_group_id,
       metadata,
       credential_name
FROM contacts
WHERE TRIM(COALESCE(handle, '')) <> ''
ON CONFLICT(owner_did, handle, did)
DO UPDATE SET
    owner_identity_id = COALESCE(excluded.owner_identity_id, contact_handle_bindings.owner_identity_id),
    last_seen_at = excluded.last_seen_at,
    source_type = COALESCE(excluded.source_type, contact_handle_bindings.source_type),
    source_group_id = COALESCE(excluded.source_group_id, contact_handle_bindings.source_group_id),
    metadata = COALESCE(excluded.metadata, contact_handle_bindings.metadata),
    credential_name = COALESCE(excluded.credential_name, contact_handle_bindings.credential_name)"#,
            [&now],
        )
        .map_err(super::local_state_unavailable)?;
    connection
        .execute(
            r#"
WITH ranked AS (
    SELECT owner_did,
           handle,
           did,
           ROW_NUMBER() OVER (
               PARTITION BY owner_did, handle
               ORDER BY COALESCE(last_seen_at, first_seen_at, ?1) DESC, did DESC
           ) AS row_num
    FROM contacts
    WHERE TRIM(COALESCE(handle, '')) <> ''
)
UPDATE contact_handle_bindings
SET is_current = CASE
    WHEN EXISTS (
        SELECT 1
        FROM ranked
        WHERE ranked.owner_did = contact_handle_bindings.owner_did
          AND ranked.handle = contact_handle_bindings.handle
          AND ranked.did = contact_handle_bindings.did
          AND ranked.row_num = 1
    ) THEN 1
    ELSE 0
END
WHERE EXISTS (
    SELECT 1
    FROM ranked
    WHERE ranked.owner_did = contact_handle_bindings.owner_did
      AND ranked.handle = contact_handle_bindings.handle
      AND ranked.did = contact_handle_bindings.did
)"#,
            [&now],
        )
        .map_err(super::local_state_unavailable)?;
    Ok(())
}

fn set_schema_version(connection: &Connection, version: i64) -> crate::ImResult<()> {
    connection
        .pragma_update(None, "user_version", version)
        .map_err(super::local_state_unavailable)
}

fn now_utc_like() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{seconds}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_state_schema_creates_legacy_tables_views_and_version() {
        let db = Connection::open_in_memory().unwrap();

        ensure_schema(&db).unwrap();

        assert_eq!(current_schema_version(&db).unwrap(), SCHEMA_VERSION);
        for object in [
            ("table", "contacts"),
            ("table", "contact_handle_bindings"),
            ("table", "messages"),
            ("table", "groups"),
            ("table", "group_members"),
            ("table", "relationship_events"),
            ("table", "e2ee_outbox"),
            ("table", "e2ee_sessions"),
            ("table", "direct_e2ee_sessions"),
            ("table", "direct_e2ee_signed_prekeys"),
            ("table", "direct_e2ee_one_time_prekeys"),
            ("view", "threads"),
            ("view", "inbox"),
            ("view", "outbox"),
        ] {
            assert_schema_object_exists(&db, object.0, object.1);
        }
        assert_index_exists(
            &db,
            "idx_contact_handle_bindings_owner_handle_current_unique",
        );
        assert_index_exists(&db, "idx_messages_owner_thread");
        assert_index_exists(&db, "idx_messages_owner_identity_thread");
        assert_index_exists(&db, "idx_groups_owner_status_last_message");
        assert_index_exists(&db, "idx_groups_owner_identity_status_last_message");
        assert_index_exists(&db, "idx_direct_e2ee_sessions_owner_updated");
        assert_index_exists(&db, "idx_direct_e2ee_signed_prekeys_owner_status");
        assert_index_exists(&db, "idx_direct_e2ee_one_time_prekeys_owner_status");
        for table in [
            "contacts",
            "contact_handle_bindings",
            "messages",
            "groups",
            "group_members",
            "relationship_events",
            "e2ee_outbox",
            "direct_e2ee_sessions",
            "direct_e2ee_signed_prekeys",
            "direct_e2ee_one_time_prekeys",
        ] {
            assert_column_exists(&db, table, "owner_identity_id");
        }
        assert_column_missing(&db, "e2ee_sessions", "owner_identity_id");
    }

    #[test]
    fn local_state_schema_rejects_unsupported_versions() {
        let old = Connection::open_in_memory().unwrap();
        old.pragma_update(None, "user_version", 5).unwrap();
        assert!(matches!(
            ensure_schema(&old),
            Err(crate::ImError::LocalStateUnavailable { detail })
                if detail.contains("too old")
        ));

        let future = Connection::open_in_memory().unwrap();
        future
            .pragma_update(None, "user_version", SCHEMA_VERSION + 1)
            .unwrap();
        assert!(matches!(
            ensure_schema(&future),
            Err(crate::ImError::LocalStateUnavailable { detail })
                if detail.contains("newer than supported")
        ));
    }

    #[test]
    fn local_state_schema_backfills_handle_bindings_for_legacy_contacts() {
        let db = Connection::open_in_memory().unwrap();
        db.pragma_update(None, "user_version", 6).unwrap();
        db.execute_batch(V6_TABLES_SQL).unwrap();
        db.execute(
            r#"
INSERT INTO contacts
    (owner_did, did, handle, first_seen_at, last_seen_at, metadata)
VALUES (?1, ?2, ?3, ?4, ?5, ?6)"#,
            (
                "did:owner",
                "did:peer",
                "alice",
                "2026-01-01T00:00:00Z",
                "2026-01-01T00:00:00Z",
                r#"{"source":"legacy"}"#,
            ),
        )
        .unwrap();

        ensure_schema(&db).unwrap();

        let did = db
            .query_row(
                "SELECT did FROM contact_handle_bindings WHERE owner_did = ?1 AND handle = ?2",
                ("did:owner", "alice"),
                |row| row.get::<_, String>(0),
            )
            .unwrap();
        assert_eq!(did, "did:peer");
        assert_eq!(current_schema_version(&db).unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn local_state_owner_backfills_identity_ids_from_credentials_then_owner_did() {
        let db = Connection::open_in_memory().unwrap();
        ensure_schema(&db).unwrap();
        db.execute(
            r#"
INSERT INTO messages
    (msg_id, owner_did, thread_id, direction, stored_at, credential_name)
VALUES ('by-credential', 'did:old', 'dm:old:bob', 0, '2026-05-21T00:00:00Z', 'alice')"#,
            [],
        )
        .unwrap();
        db.execute(
            r#"
INSERT INTO groups
    (owner_did, group_id, group_mode, membership_status, stored_at, credential_name)
VALUES ('did:alice', 'group-1', 'general', 'active', '2026-05-21T00:00:00Z', '')"#,
            [],
        )
        .unwrap();
        db.execute(
            r#"
INSERT INTO e2ee_outbox
    (outbox_id, owner_did, peer_did, plaintext, created_at, updated_at)
VALUES ('outbox-1', 'did:alice', 'did:bob', 'secret', '2026-05-21T00:00:00Z', '2026-05-21T00:00:00Z')"#,
            [],
        )
        .unwrap();

        let updated = backfill_owner_identity_ids(
            &db,
            &[OwnerIdentityBackfill {
                identity_id: "alice-id".to_string(),
                owner_did: "did:alice".to_string(),
                credential_names: vec!["alice".to_string()],
            }],
        )
        .unwrap();

        assert_eq!(updated, 3);
        assert_eq!(
            string_cell(
                &db,
                "SELECT owner_identity_id FROM messages WHERE msg_id = 'by-credential'"
            ),
            "alice-id"
        );
        assert_eq!(
            string_cell(
                &db,
                "SELECT owner_identity_id FROM groups WHERE group_id = 'group-1'"
            ),
            "alice-id"
        );
        assert_eq!(
            string_cell(
                &db,
                "SELECT owner_identity_id FROM e2ee_outbox WHERE outbox_id = 'outbox-1'"
            ),
            "alice-id"
        );
    }

    fn assert_schema_object_exists(db: &Connection, object_type: &str, name: &str) {
        let count = db
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = ?1 AND name = ?2",
                (object_type, name),
                |row| row.get::<_, i64>(0),
            )
            .unwrap();
        assert_eq!(count, 1, "missing {object_type} {name}");
    }

    fn assert_index_exists(db: &Connection, name: &str) {
        assert_schema_object_exists(db, "index", name);
    }

    fn assert_column_exists(db: &Connection, table: &str, column: &str) {
        assert!(column_exists(db, table, column), "missing {table}.{column}");
    }

    fn assert_column_missing(db: &Connection, table: &str, column: &str) {
        assert!(
            !column_exists(db, table, column),
            "unexpected {table}.{column}"
        );
    }

    fn column_exists(db: &Connection, table: &str, column: &str) -> bool {
        let mut statement = db.prepare(&format!("PRAGMA table_info({table})")).unwrap();
        let mut rows = statement
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap();
        rows.any(|name| name.unwrap() == column)
    }

    fn string_cell(db: &Connection, sql: &str) -> String {
        db.query_row(sql, [], |row| row.get::<_, Option<String>>(0))
            .unwrap()
            .unwrap_or_default()
    }
}
