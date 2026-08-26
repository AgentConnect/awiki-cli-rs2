#![cfg(feature = "sqlite")]

use rusqlite::{Connection, OpenFlags};
use sha2::{Digest, Sha256};
use std::fs;

const SNAPSHOT_QUERIES: &[(&str, usize)] = &[
    (
        "SELECT owner_identity_id,current_did,identity_generation,device_auth_generation \
         FROM identity_account_bindings ORDER BY owner_identity_id",
        4,
    ),
    (
        "SELECT conversation_id,thread_kind,thread_id,lifecycle_state,resolution_state \
         FROM conversation_registry ORDER BY conversation_id",
        5,
    ),
    (
        "SELECT conversation_id,CAST(message_count AS TEXT),CAST(unread_count AS TEXT),\
         CAST(unread_mention_count AS TEXT),COALESCE(first_unread_mention_message_id,'') \
         FROM conversation_summaries ORDER BY conversation_id",
        5,
    ),
    (
        "SELECT msg_id,conversation_id,CAST(server_seq AS TEXT),CAST(is_read AS TEXT),\
         CAST(mentions_current_user AS TEXT) FROM messages ORDER BY msg_id",
        5,
    ),
    (
        "SELECT conversation_id,peer_user_id,full_handle,current_did \
         FROM direct_peer_routes ORDER BY conversation_id",
        4,
    ),
    (
        "SELECT outbox_id,local_status,CAST(attempt_count AS TEXT),plaintext \
         FROM e2ee_outbox ORDER BY outbox_id",
        4,
    ),
    (
        "SELECT sync_subject_id,scope,checkpoint_kind,event_seq \
         FROM sync_state ORDER BY sync_subject_id,scope,checkpoint_kind",
        4,
    ),
    (
        "SELECT thread_kind,thread_id,message_id,content \
         FROM attachment_manifest_cache ORDER BY thread_kind,thread_id,message_id",
        4,
    ),
    (
        "SELECT job_id,phase,CAST(attempt_count AS TEXT),group_state_ref_json \
         FROM group_rebind_outbox ORDER BY job_id",
        4,
    ),
];

#[test]
fn locked_0714_schema_36_fixture_migrates_to_37_without_data_drift() {
    let fixture_dir = std::env::var_os("AWIKI_0714_E2EE_FIXTURE_DIR")
        .expect("AWIKI_0714_E2EE_FIXTURE_DIR must name the locked offline fixture");
    let source = std::path::Path::new(&fixture_dir).join("core-schema-36.sqlite");
    let temp = tempfile::tempdir().unwrap();
    let migrated = temp.path().join("core-schema-37.sqlite");
    fs::copy(source, &migrated).unwrap();

    let before = Connection::open_with_flags(&migrated, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    assert_eq!(
        awiki_im_core::compat::local_state::current_schema_version(&before).unwrap(),
        36
    );
    let before_digest = conservation_digest(&before);
    assert_eq!(scalar(&before, "SELECT COUNT(*) FROM messages"), 2);
    assert_eq!(
        scalar(
            &before,
            "SELECT SUM(unread_count) FROM conversation_summaries"
        ),
        1
    );
    assert_eq!(
        scalar(
            &before,
            "SELECT SUM(unread_mention_count) FROM conversation_summaries"
        ),
        1
    );
    assert_eq!(
        scalar(
            &before,
            "SELECT COUNT(*) FROM group_rebind_outbox WHERE phase='awaiting_p6'"
        ),
        1
    );
    drop(before);

    let connection = Connection::open(&migrated).unwrap();
    awiki_im_core::compat::local_state::ensure_schema(&connection).unwrap();
    assert_eq!(
        awiki_im_core::compat::local_state::current_schema_version(&connection).unwrap(),
        37
    );
    assert_eq!(conservation_digest(&connection), before_digest);
    assert_eq!(
        scalar(&connection, "SELECT COUNT(*) FROM did_transition_edges"),
        0
    );
    assert_eq!(
        scalar(
            &connection,
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND \
             (lower(name) LIKE '%conflict%' OR lower(name) LIKE '%reconcile%')",
        ),
        0
    );
    eprintln!(
        "phase2_0714_core_migrated_sha256={:x}",
        Sha256::digest(fs::read(&migrated).unwrap())
    );
}

fn conservation_digest(connection: &Connection) -> String {
    let mut digest = Sha256::new();
    for (sql, columns) in SNAPSHOT_QUERIES {
        digest.update(sql.as_bytes());
        let mut statement = connection.prepare(sql).unwrap();
        let rows = statement
            .query_map([], |row| {
                (0..*columns)
                    .map(|index| row.get::<_, String>(index))
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .unwrap();
        for row in rows {
            for value in row.unwrap() {
                digest.update((value.len() as u64).to_be_bytes());
                digest.update(value.as_bytes());
            }
        }
    }
    format!("{:x}", digest.finalize())
}

fn scalar(connection: &Connection, sql: &str) -> i64 {
    connection.query_row(sql, [], |row| row.get(0)).unwrap()
}
