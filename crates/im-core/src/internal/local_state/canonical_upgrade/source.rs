use std::path::Path;

use rusqlite::{Connection, OpenFlags};
use sha2::{Digest as _, Sha256};

use super::{
    upgrade_failed, CanonicalUpgradeDetection, CanonicalUpgradeEligibility,
    RELEASE_0710_SCHEMA_VERSION,
};

const REQUIRED_TABLES: &[&str] = &[
    "contacts",
    "messages",
    "conversation_summaries",
    "conversation_registry",
    "groups",
    "group_members",
    "e2ee_outbox",
    "identity_did_history",
    "direct_peer_routes",
    "thread_read_state",
];

const REQUIRED_COLUMNS: &[(&str, &str)] = &[
    ("messages", "owner_identity_id"),
    ("messages", "conversation_id"),
    ("messages", "thread_id"),
    ("messages", "sender_did"),
    ("messages", "receiver_did"),
    ("messages", "group_did"),
    ("conversation_registry", "conversation_id"),
    ("conversation_registry", "thread_kind"),
    ("direct_peer_routes", "peer_user_id"),
    ("direct_peer_routes", "full_handle"),
    ("direct_peer_routes", "current_did"),
    ("group_members", "member_did"),
];

const FORBIDDEN_TABLES: &[&str] = &[
    "peer_personas",
    "peer_identifiers",
    "peer_profiles",
    "conversation_aliases",
    "inbound_resolution_backlog",
];

const FORBIDDEN_COLUMNS: &[(&str, &str)] = &[
    ("messages", "wire_thread_kind"),
    ("messages", "wire_thread_ref"),
    ("conversation_registry", "peer_persona_id"),
    ("conversation_registry", "lifecycle_state"),
    ("group_members", "membership_id"),
    ("direct_peer_routes", "peer_persona_id"),
];

pub(super) fn detect(path: &Path) -> crate::ImResult<CanonicalUpgradeDetection> {
    let connection = open_read_only(path, "detect")?;
    let source_schema_version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|_| upgrade_failed("detect", "schema_version_unreadable"))?;
    let source_fingerprint = schema_fingerprint(&connection)?;
    if source_schema_version == super::super::schema::SCHEMA_VERSION {
        return Ok(CanonicalUpgradeDetection {
            eligibility: CanonicalUpgradeEligibility::NotRequired,
            source_schema_version,
            target_schema_version: super::super::schema::SCHEMA_VERSION,
            source_fingerprint,
        });
    }
    if source_schema_version != RELEASE_0710_SCHEMA_VERSION {
        return Err(upgrade_failed("detect", "unsupported_source_schema"));
    }
    verify_connection(&connection, None)?;
    Ok(CanonicalUpgradeDetection {
        eligibility: CanonicalUpgradeEligibility::Eligible,
        source_schema_version,
        target_schema_version: super::super::schema::SCHEMA_VERSION,
        source_fingerprint,
    })
}

pub(super) fn verify_release_0710_source(
    path: &Path,
    expected_fingerprint: Option<&str>,
) -> crate::ImResult<String> {
    let connection = open_read_only(path, "preflight")?;
    verify_connection(&connection, expected_fingerprint)
}

fn verify_connection(
    connection: &Connection,
    expected_fingerprint: Option<&str>,
) -> crate::ImResult<String> {
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|_| upgrade_failed("preflight", "schema_version_unreadable"))?;
    if version != RELEASE_0710_SCHEMA_VERSION {
        return Err(upgrade_failed("preflight", "source_schema_changed"));
    }
    let integrity: String = connection
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .map_err(|_| upgrade_failed("preflight", "source_integrity_check_failed"))?;
    if integrity != "ok" {
        return Err(upgrade_failed("preflight", "source_integrity_check_failed"));
    }
    for table in REQUIRED_TABLES {
        if !has_table(connection, table)? {
            return Err(upgrade_failed("preflight", "required_table_missing"));
        }
    }
    for (table, column) in REQUIRED_COLUMNS {
        if !has_column(connection, table, column)? {
            return Err(upgrade_failed("preflight", "required_column_missing"));
        }
    }
    for table in FORBIDDEN_TABLES {
        if has_table(connection, table)? {
            return Err(upgrade_failed(
                "preflight",
                "partial_target_schema_detected",
            ));
        }
    }
    for (table, column) in FORBIDDEN_COLUMNS {
        if has_column(connection, table, column)? {
            return Err(upgrade_failed(
                "preflight",
                "partial_target_schema_detected",
            ));
        }
    }
    let fingerprint = schema_fingerprint(connection)?;
    if expected_fingerprint.is_some_and(|expected| expected != fingerprint) {
        return Err(upgrade_failed("preflight", "source_fingerprint_changed"));
    }
    Ok(fingerprint)
}

fn open_read_only(path: &Path, phase: &str) -> crate::ImResult<Connection> {
    Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| upgrade_failed(phase, "source_open_failed"))
}

fn has_table(connection: &Connection, table: &str) -> crate::ImResult<bool> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?1)",
            [table],
            |row| row.get::<_, i64>(0),
        )
        .map(|value| value != 0)
        .map_err(|_| upgrade_failed("preflight", "schema_catalog_unreadable"))
}

fn has_column(connection: &Connection, table: &str, column: &str) -> crate::ImResult<bool> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|_| upgrade_failed("preflight", "schema_catalog_unreadable"))?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|_| upgrade_failed("preflight", "schema_catalog_unreadable"))?;
    for row in rows {
        if row.map_err(|_| upgrade_failed("preflight", "schema_catalog_unreadable"))? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn schema_fingerprint(connection: &Connection) -> crate::ImResult<String> {
    let mut statement = connection
        .prepare(
            r#"SELECT type, name, COALESCE(sql, '') FROM sqlite_schema
WHERE name NOT LIKE 'sqlite_%'
ORDER BY type, name"#,
        )
        .map_err(|_| upgrade_failed("preflight", "schema_catalog_unreadable"))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|_| upgrade_failed("preflight", "schema_catalog_unreadable"))?;
    let mut digest = Sha256::new();
    for row in rows {
        let (kind, name, sql) =
            row.map_err(|_| upgrade_failed("preflight", "schema_catalog_unreadable"))?;
        digest.update(kind.as_bytes());
        digest.update([0]);
        digest.update(name.as_bytes());
        digest.update([0]);
        for token in sql.split_whitespace() {
            digest.update(token.as_bytes());
            digest.update([b' ']);
        }
        digest.update([b'\n']);
    }
    Ok(format!("sha256:{:x}", digest.finalize()))
}

#[cfg(test)]
pub(super) fn create_minimal_release_0710_fixture(path: &Path) {
    let db = Connection::open(path).unwrap();
    db.execute_batch(
        r#"
CREATE TABLE contacts(owner_identity_id TEXT, did TEXT);
CREATE TABLE messages(owner_identity_id TEXT, owner_did TEXT, msg_id TEXT,
 conversation_id TEXT, thread_id TEXT, sender_did TEXT, receiver_did TEXT,
 group_did TEXT, PRIMARY KEY(owner_identity_id, msg_id));
CREATE TABLE conversation_summaries(owner_identity_id TEXT, conversation_id TEXT);
CREATE TABLE conversation_registry(owner_identity_id TEXT, conversation_id TEXT,
 thread_kind TEXT, thread_id TEXT);
CREATE TABLE groups(owner_identity_id TEXT, group_id TEXT, group_did TEXT);
CREATE TABLE group_members(owner_identity_id TEXT, group_id TEXT, user_id TEXT, member_did TEXT);
CREATE TABLE e2ee_outbox(owner_identity_id TEXT, outbox_id TEXT);
CREATE TABLE identity_did_history(owner_identity_id TEXT, did TEXT);
CREATE TABLE direct_peer_routes(owner_identity_id TEXT, conversation_id TEXT,
 peer_user_id TEXT, full_handle TEXT, current_did TEXT);
CREATE TABLE thread_read_state(owner_identity_id TEXT, conversation_id TEXT);
PRAGMA user_version = 27;
"#,
    )
    .unwrap();
}

#[cfg(test)]
pub(super) fn create_full_release_0710_fixture(path: &Path) {
    let db = Connection::open(path).unwrap();
    super::super::schema::ensure_schema(&db).unwrap();
    let direct_canonical = super::super::owner_scope::direct_conversation_id_for_peer_scope(
        &super::super::owner_scope::DirectPeerScope::new("peer-user", "peer.awiki.info").unwrap(),
    );
    db.execute_batch(&format!(
        r#"
INSERT INTO identity_did_history
(owner_identity_id, did, status, first_seen_at, last_seen_at)
VALUES ('owner', 'did:example:owner', 'current', '1', '1');
INSERT INTO contacts(owner_identity_id, owner_did, did, handle)
VALUES ('owner', 'did:example:owner', 'did:example:peer', 'peer.awiki.info');
INSERT INTO direct_peer_routes
(owner_identity_id, conversation_id, peer_user_id, full_handle, current_did, updated_at)
VALUES ('owner', '{direct_canonical}', 'peer-user', 'peer.awiki.info', 'did:example:peer', '1');
INSERT INTO conversation_registry
(owner_identity_id, owner_did, conversation_id, thread_kind, thread_id,
 activity_at, created_at, updated_at, is_active)
VALUES
('owner', 'did:example:owner', '{direct_canonical}', 'direct', '{direct_canonical}', '1', '1', '1', 1),
('owner', 'did:example:owner', 'dm:did:example:peer', 'thread', 'dm:did:example:peer', '2', '2', '2', 1),
('owner', 'did:example:owner', 'group:local-group', 'group', 'local-group', '3', '3', '3', 1),
('owner', 'did:example:owner', 'group:empty-local', 'group', 'empty-local', '4', '4', '4', 1);
INSERT INTO groups
(owner_identity_id, owner_did, group_id, group_did, name, stored_at)
VALUES
('owner', 'did:example:owner', 'local-group', 'did:example:group', 'Group', '3'),
('owner', 'did:example:owner', 'empty-local', 'did:example:empty-group', 'Empty', '4');
INSERT INTO group_members
(owner_identity_id, owner_did, group_id, user_id, member_did, member_handle,
 status, last_synced_at)
VALUES ('owner', 'did:example:owner', 'local-group', 'peer-user',
        'did:example:peer', 'peer.awiki.info', 'active', '3');
INSERT INTO messages
(msg_id, owner_identity_id, owner_did, conversation_id, thread_id, direction,
 sender_did, receiver_did, content_type, content, stored_at, is_read)
VALUES
('direct-1', 'owner', 'did:example:owner', 'dm:did:example:peer',
 'dm:did:example:peer', 0, 'did:example:peer', 'did:example:owner',
 'text/plain', 'direct body', '2', 0),
('group-1', 'owner', 'did:example:owner', 'group:local-group',
 'group:local-group', 0, 'did:example:peer', 'did:example:owner',
 'text/plain', 'group body', '3', 1);
UPDATE messages SET group_id = 'local-group', group_did = 'did:example:group'
WHERE msg_id = 'group-1';
INSERT INTO thread_read_state
(owner_identity_id, owner_did, thread_scope, thread_id, conversation_id,
 read_watermark_message_id, read_watermark_seq, pending_remote_ack, updated_at)
VALUES ('owner', 'did:example:owner', 'direct', 'dm:did:example:peer',
        'dm:did:example:peer', 'direct-1', '1', 0, '2');
INSERT INTO e2ee_outbox
(outbox_id, owner_identity_id, owner_did, peer_did, plaintext, local_status,
 created_at, updated_at)
VALUES ('outbox-1', 'owner', 'did:example:owner', 'did:example:peer',
        'queued body', 'queued', '2', '2');
"#
    ))
    .unwrap();
    db.execute_batch(
        r#"
DROP INDEX IF EXISTS idx_conversation_registry_active_direct_persona;
DROP INDEX IF EXISTS idx_conversation_registry_active_group_did;
DROP INDEX IF EXISTS idx_group_members_owner_membership;
ALTER TABLE group_members RENAME TO group_members_target;
CREATE TABLE group_members (
    owner_identity_id TEXT NOT NULL,
    owner_did TEXT NOT NULL DEFAULT '',
    group_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    member_did TEXT,
    member_handle TEXT,
    profile_url TEXT,
    role TEXT,
    status TEXT NOT NULL DEFAULT 'active',
    joined_at TEXT,
    sent_message_count INTEGER NOT NULL DEFAULT 0,
    last_synced_at TEXT NOT NULL,
    metadata TEXT,
    credential_name TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (owner_identity_id, group_id, user_id)
);
INSERT INTO group_members
(owner_identity_id, owner_did, group_id, user_id, member_did, member_handle,
 profile_url, role, status, joined_at, sent_message_count, last_synced_at,
 metadata, credential_name)
SELECT owner_identity_id, owner_did, group_id, user_id, member_did, member_handle,
 profile_url, role, status, joined_at, sent_message_count, last_synced_at,
 metadata, credential_name
FROM group_members_target;
DROP TABLE group_members_target;
DROP TABLE inbound_resolution_backlog;
DROP TABLE conversation_aliases;
DROP TABLE peer_profiles;
DROP TABLE peer_identifiers;
DROP TABLE peer_personas;
ALTER TABLE contacts DROP COLUMN peer_persona_id;
ALTER TABLE direct_peer_routes DROP COLUMN authority_namespace;
ALTER TABLE direct_peer_routes DROP COLUMN peer_persona_id;
ALTER TABLE conversation_registry DROP COLUMN merged_into_conversation_id;
ALTER TABLE conversation_registry DROP COLUMN resolution_state;
ALTER TABLE conversation_registry DROP COLUMN lifecycle_state;
ALTER TABLE conversation_registry DROP COLUMN canonical_group_did;
ALTER TABLE conversation_registry DROP COLUMN peer_persona_id;
ALTER TABLE messages DROP COLUMN wire_identity_resolution_state;
ALTER TABLE messages DROP COLUMN wire_thread_ref;
ALTER TABLE messages DROP COLUMN wire_thread_kind;
PRAGMA user_version = 27;
"#,
    )
    .unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_only_clean_release_0710_shape() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("im.sqlite");
        create_minimal_release_0710_fixture(&path);

        let detection = detect(&path).unwrap();
        assert_eq!(detection.eligibility, CanonicalUpgradeEligibility::Eligible);
        assert_eq!(detection.source_schema_version, 27);
        assert!(detection.source_fingerprint.starts_with("sha256:"));
    }

    #[test]
    fn rejects_partial_target_shape_without_writing_source() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("im.sqlite");
        create_minimal_release_0710_fixture(&path);
        let db = Connection::open(&path).unwrap();
        db.execute_batch("CREATE TABLE peer_personas(id TEXT);")
            .unwrap();
        drop(db);

        assert!(matches!(
            detect(&path),
            Err(crate::ImError::LocalStateUpgradeFailed { phase, code })
                if phase == "preflight" && code == "partial_target_schema_detected"
        ));
        assert_eq!(
            Connection::open(&path)
                .unwrap()
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            27
        );
    }
}
