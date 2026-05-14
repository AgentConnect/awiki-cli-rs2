use super::{StoreError, StoreResult, SCHEMA_VERSION};
use rusqlite::Connection;
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

const V7_TABLES_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS groups (
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

const INDEX_STATEMENTS: &[&str] = &[
    "CREATE INDEX IF NOT EXISTS idx_contacts_owner ON contacts(owner_did, last_seen_at DESC)",
    "CREATE UNIQUE INDEX IF NOT EXISTS idx_contact_handle_bindings_owner_handle_current_unique ON contact_handle_bindings(owner_did, handle) WHERE is_current = 1",
    "CREATE INDEX IF NOT EXISTS idx_contact_handle_bindings_owner_did ON contact_handle_bindings(owner_did, did, last_seen_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_contact_handle_bindings_owner_handle ON contact_handle_bindings(owner_did, handle, last_seen_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_messages_owner_thread ON messages(owner_did, thread_id, sent_at)",
    "CREATE INDEX IF NOT EXISTS idx_messages_owner_thread_seq ON messages(owner_did, thread_id, server_seq)",
    "CREATE INDEX IF NOT EXISTS idx_messages_owner_direction ON messages(owner_did, direction)",
    "CREATE INDEX IF NOT EXISTS idx_messages_owner_sender ON messages(owner_did, sender_did)",
    "CREATE INDEX IF NOT EXISTS idx_messages_owner ON messages(owner_did)",
    "CREATE INDEX IF NOT EXISTS idx_messages_credential ON messages(credential_name)",
    "CREATE INDEX IF NOT EXISTS idx_e2ee_outbox_owner_status ON e2ee_outbox(owner_did, local_status, updated_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_e2ee_outbox_owner_sent_msg ON e2ee_outbox(owner_did, sent_msg_id)",
    "CREATE INDEX IF NOT EXISTS idx_e2ee_outbox_owner_sent_seq ON e2ee_outbox(owner_did, peer_did, sent_server_seq)",
    "CREATE INDEX IF NOT EXISTS idx_e2ee_outbox_credential ON e2ee_outbox(credential_name)",
    "CREATE INDEX IF NOT EXISTS idx_groups_owner_status_last_message ON groups(owner_did, membership_status, last_message_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_groups_owner_slug ON groups(owner_did, slug)",
    "CREATE INDEX IF NOT EXISTS idx_groups_owner_updated ON groups(owner_did, remote_updated_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_group_members_owner_group_role ON group_members(owner_did, group_id, role)",
    "CREATE INDEX IF NOT EXISTS idx_group_members_owner_group_status ON group_members(owner_did, group_id, status)",
    "CREATE INDEX IF NOT EXISTS idx_contacts_owner_source_group ON contacts(owner_did, source_group_id)",
    "CREATE INDEX IF NOT EXISTS idx_relationship_events_owner_target_time ON relationship_events(owner_did, target_did, created_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_relationship_events_owner_status_time ON relationship_events(owner_did, status, created_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_relationship_events_owner_group ON relationship_events(owner_did, source_group_id)",
    "CREATE INDEX IF NOT EXISTS idx_e2ee_sessions_owner_updated ON e2ee_sessions(owner_did, updated_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_e2ee_sessions_credential ON e2ee_sessions(credential_name)",
];

const VIEW_STATEMENTS: &[&str] = &[
    r#"CREATE VIEW IF NOT EXISTS threads AS
SELECT
    owner_did,
    thread_id,
    COUNT(*) AS message_count,
    SUM(CASE WHEN is_read = 0 AND direction = 0 THEN 1 ELSE 0 END) AS unread_count,
    MAX(COALESCE(sent_at, stored_at)) AS last_message_at,
    (SELECT m2.content FROM messages m2
     WHERE m2.owner_did = m.owner_did
       AND m2.thread_id = m.thread_id
     ORDER BY COALESCE(m2.sent_at, m2.stored_at) DESC
     LIMIT 1) AS last_content
FROM messages m
GROUP BY owner_did, thread_id"#,
    r#"CREATE VIEW IF NOT EXISTS inbox AS
SELECT * FROM messages WHERE direction = 0
ORDER BY owner_did, COALESCE(sent_at, stored_at) DESC"#,
    r#"CREATE VIEW IF NOT EXISTS outbox AS
SELECT * FROM messages WHERE direction = 1
ORDER BY owner_did, COALESCE(sent_at, stored_at) DESC"#,
];

pub fn ensure_schema(connection: &Connection) -> StoreResult<()> {
    let version = current_schema_version(connection)?;
    if version == 0 {
        create_schema(connection)?;
        return set_schema_version(connection, SCHEMA_VERSION);
    }
    if version > SCHEMA_VERSION {
        return Err(StoreError::Invalid(format!(
            "sqlite schema version {version} is newer than supported {SCHEMA_VERSION}"
        )));
    }
    if version < 6 {
        return Err(StoreError::Invalid(format!(
            "sqlite schema version {version} is too old for in-place upgrade"
        )));
    }
    create_schema(connection)?;
    set_schema_version(connection, SCHEMA_VERSION)
}

pub fn current_schema_version(connection: &Connection) -> StoreResult<i64> {
    Ok(connection.pragma_query_value(None, "user_version", |row| row.get(0))?)
}

fn create_schema(connection: &Connection) -> StoreResult<()> {
    for script in [
        V6_TABLES_SQL,
        V7_TABLES_SQL,
        V8_TABLES_SQL,
        V11_TABLES_SQL,
        V12_TABLES_SQL,
    ] {
        connection.execute_batch(script)?;
    }
    backfill_contact_handle_bindings(connection)?;
    for statement in INDEX_STATEMENTS {
        connection.execute(statement, [])?;
    }
    for view in ["threads", "inbox", "outbox"] {
        connection.execute(&format!("DROP VIEW IF EXISTS {view}"), [])?;
    }
    for statement in VIEW_STATEMENTS {
        connection.execute(statement, [])?;
    }
    Ok(())
}

fn backfill_contact_handle_bindings(connection: &Connection) -> StoreResult<()> {
    let now = now_utc_like();
    connection.execute(
        r#"
INSERT INTO contact_handle_bindings
    (owner_did, handle, did, is_current, first_seen_at, last_seen_at, source_type, source_group_id, metadata, credential_name)
SELECT owner_did,
       handle,
       did,
       0,
       COALESCE(first_seen_at, ?1),
       COALESCE(last_seen_at, ?1),
       source_type,
       source_group_id,
       metadata,
       ''
FROM contacts
WHERE TRIM(COALESCE(handle, '')) <> ''
ON CONFLICT(owner_did, handle, did)
DO UPDATE SET
    last_seen_at = excluded.last_seen_at,
    source_type = COALESCE(excluded.source_type, contact_handle_bindings.source_type),
    source_group_id = COALESCE(excluded.source_group_id, contact_handle_bindings.source_group_id),
    metadata = COALESCE(excluded.metadata, contact_handle_bindings.metadata),
    credential_name = COALESCE(excluded.credential_name, contact_handle_bindings.credential_name)"#,
        [&now],
    )?;
    connection.execute(
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
    )?;
    Ok(())
}

fn set_schema_version(connection: &Connection, version: i64) -> StoreResult<()> {
    connection.pragma_update(None, "user_version", version)?;
    Ok(())
}

fn now_utc_like() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{seconds}")
}
