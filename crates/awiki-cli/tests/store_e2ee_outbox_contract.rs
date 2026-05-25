use awiki_cli::legacy_store::{
    self as store, get_e2ee_outbox, list_e2ee_outbox, mark_e2ee_outbox_failed,
    mark_e2ee_outbox_sent, queue_e2ee_outbox, set_e2ee_outbox_failure_by_id,
    update_e2ee_outbox_status, E2EEOutboxRecord, StoreResult,
};
use rusqlite::Connection;
use serde_json::Value;

#[test]
fn queue_e2ee_outbox_defaults_generated_id_and_normalization_match_go() -> StoreResult<()> {
    let db = memory_store();
    let outbox_id = queue_e2ee_outbox(
        &db,
        E2EEOutboxRecord {
            owner_did: " did:owner ".to_string(),
            peer_did: " did:peer-preserved ".to_string(),
            plaintext: "secret".to_string(),
            metadata: " \n ".to_string(),
            credential_name: " default ".to_string(),
            ..E2EEOutboxRecord::default()
        },
    )?;

    assert!(
        outbox_id.starts_with("local-"),
        "blank outbox_id should generate Go-like local-<unix-nano> id"
    );

    let record = get_e2ee_outbox(&db, &outbox_id, "did:owner", "")?;
    assert_eq!(text(&record, "outbox_id"), outbox_id);
    assert_eq!(text(&record, "owner_did"), "did:owner");
    assert_eq!(text(&record, "peer_did"), " did:peer-preserved ");
    assert_eq!(text(&record, "original_type"), "text");
    assert_eq!(text(&record, "local_status"), "queued");
    assert_eq!(int(&record, "attempt_count"), 0);
    assert_eq!(text(&record, "credential_name"), "default");
    assert!(!text(&record, "created_at").trim().is_empty());
    assert!(!text(&record, "updated_at").trim().is_empty());
    assert_eq!(optional_text(&db, "metadata", &outbox_id), None);

    Ok(())
}

#[test]
fn list_e2ee_outbox_uses_updated_at_desc_for_owner_and_credential_status_paths() -> StoreResult<()>
{
    let db = memory_store();
    queue_fixture(
        &db,
        "owner-old",
        "did:owner",
        "default",
        "queued",
        "2026-01-01T00:00:00Z",
    )?;
    queue_fixture(
        &db,
        "owner-new",
        "did:owner",
        "default",
        "queued",
        "2026-01-03T00:00:00Z",
    )?;
    queue_fixture(
        &db,
        "owner-failed",
        "did:owner",
        "default",
        "failed",
        "2026-01-04T00:00:00Z",
    )?;
    queue_fixture(
        &db,
        "credential-old",
        "did:other",
        "fallback",
        "queued",
        "2026-01-02T00:00:00Z",
    )?;
    queue_fixture(
        &db,
        "credential-new",
        "did:other",
        "fallback",
        "queued",
        "2026-01-05T00:00:00Z",
    )?;

    assert_eq!(
        ids(list_e2ee_outbox(&db, "did:owner", "", "queued")?),
        vec!["owner-new", "owner-old"]
    );
    assert_eq!(
        ids(list_e2ee_outbox(&db, "did:owner", "", "")?),
        vec!["owner-failed", "owner-new", "owner-old"]
    );
    assert_eq!(
        ids(list_e2ee_outbox(&db, "", "fallback", "queued")?),
        vec!["credential-new", "credential-old"]
    );
    assert_eq!(
        ids(list_e2ee_outbox(&db, "", "fallback", "")?),
        vec!["credential-new", "credential-old"]
    );

    Ok(())
}

#[test]
fn get_e2ee_outbox_prefers_owner_path_and_falls_back_to_credential_when_owner_blank(
) -> StoreResult<()> {
    let db = memory_store();
    queue_fixture(
        &db,
        "owner-row",
        "did:owner",
        "default",
        "queued",
        "2026-01-01T00:00:00Z",
    )?;
    queue_fixture(
        &db,
        "credential-row",
        "did:other",
        "fallback",
        "queued",
        "2026-01-02T00:00:00Z",
    )?;

    assert_eq!(
        text(
            &get_e2ee_outbox(&db, "owner-row", " did:owner ", "fallback")?,
            "owner_did"
        ),
        "did:owner"
    );
    assert!(get_e2ee_outbox(&db, "credential-row", "did:owner", "fallback").is_err());
    assert!(matches!(
        get_e2ee_outbox(&db, "missing-row", "did:owner", ""),
        Err(store::StoreError::NotFound(_))
    ));

    let credential = get_e2ee_outbox(&db, "credential-row", " ", " fallback ")?;
    assert_eq!(text(&credential, "owner_did"), "did:other");
    assert_eq!(text(&credential, "credential_name"), "fallback");

    Ok(())
}

#[test]
fn mark_e2ee_outbox_sent_increments_preserves_on_blank_and_clears_failure_fields() -> StoreResult<()>
{
    let db = memory_store();
    let outbox_id = queue_e2ee_outbox(
        &db,
        E2EEOutboxRecord {
            outbox_id: "sent-row".to_string(),
            owner_did: "did:owner".to_string(),
            peer_did: "did:peer".to_string(),
            session_id: "session-old".to_string(),
            plaintext: "secret".to_string(),
            attempt_count: 2,
            sent_msg_id: "sent-old".to_string(),
            sent_server_seq: Some(7),
            last_error_code: "old-error".to_string(),
            retry_hint: "old-retry".to_string(),
            failed_msg_id: "failed-old".to_string(),
            failed_server_seq: Some(8),
            metadata: r#"{"old":true}"#.to_string(),
            credential_name: "default".to_string(),
            ..E2EEOutboxRecord::default()
        },
    )?;

    mark_e2ee_outbox_sent(&db, &outbox_id, " did:owner ", " ", " ", None, " ")?;
    let preserved = get_e2ee_outbox(&db, &outbox_id, "did:owner", "")?;
    assert_eq!(text(&preserved, "local_status"), "sent");
    assert_eq!(int(&preserved, "attempt_count"), 3);
    assert_eq!(text(&preserved, "session_id"), "session-old");
    assert_eq!(text(&preserved, "sent_msg_id"), "sent-old");
    assert_eq!(opt_int(&preserved, "sent_server_seq"), Some(7));
    assert_eq!(text(&preserved, "metadata"), r#"{"old":true}"#);
    assert_eq!(optional_text(&db, "last_error_code", &outbox_id), None);
    assert_eq!(optional_text(&db, "retry_hint", &outbox_id), None);
    assert_eq!(optional_text(&db, "failed_msg_id", &outbox_id), None);
    assert_eq!(optional_i64(&db, "failed_server_seq", &outbox_id), None);

    mark_e2ee_outbox_sent(
        &db,
        &outbox_id,
        "did:owner",
        "session-new",
        "sent-new",
        Some(11),
        r#"{"sent":true}"#,
    )?;
    let updated = get_e2ee_outbox(&db, &outbox_id, "did:owner", "")?;
    assert_eq!(int(&updated, "attempt_count"), 4);
    assert_eq!(text(&updated, "session_id"), "session-new");
    assert_eq!(text(&updated, "sent_msg_id"), "sent-new");
    assert_eq!(opt_int(&updated, "sent_server_seq"), Some(11));
    assert_eq!(text(&updated, "metadata"), r#"{"sent":true}"#);
    assert!(!text(&updated, "last_attempt_at").trim().is_empty());

    Ok(())
}

#[test]
fn mark_e2ee_outbox_failed_sets_failed_and_coalesces_retry_failure_and_metadata() -> StoreResult<()>
{
    let db = memory_store();
    let outbox_id = queue_e2ee_outbox(
        &db,
        E2EEOutboxRecord {
            outbox_id: "failed-row".to_string(),
            owner_did: "did:owner".to_string(),
            peer_did: "did:peer".to_string(),
            plaintext: "secret".to_string(),
            retry_hint: "retry-old".to_string(),
            failed_msg_id: "failed-old".to_string(),
            failed_server_seq: Some(3),
            metadata: r#"{"old":true}"#.to_string(),
            credential_name: "default".to_string(),
            ..E2EEOutboxRecord::default()
        },
    )?;

    mark_e2ee_outbox_failed(
        &db,
        &outbox_id,
        " did:owner ",
        "network",
        " ",
        " ",
        None,
        " ",
    )?;
    let preserved = get_e2ee_outbox(&db, &outbox_id, "did:owner", "")?;
    assert_eq!(text(&preserved, "local_status"), "failed");
    assert_eq!(text(&preserved, "last_error_code"), "network");
    assert_eq!(text(&preserved, "retry_hint"), "retry-old");
    assert_eq!(text(&preserved, "failed_msg_id"), "failed-old");
    assert_eq!(opt_int(&preserved, "failed_server_seq"), Some(3));
    assert_eq!(text(&preserved, "metadata"), r#"{"old":true}"#);

    mark_e2ee_outbox_failed(
        &db,
        &outbox_id,
        "did:owner",
        "remote-rejected",
        "retry-new",
        "failed-new",
        Some(9),
        r#"{"failed":true}"#,
    )?;
    let updated = get_e2ee_outbox(&db, &outbox_id, "did:owner", "")?;
    assert_eq!(text(&updated, "local_status"), "failed");
    assert_eq!(text(&updated, "last_error_code"), "remote-rejected");
    assert_eq!(text(&updated, "retry_hint"), "retry-new");
    assert_eq!(text(&updated, "failed_msg_id"), "failed-new");
    assert_eq!(opt_int(&updated, "failed_server_seq"), Some(9));
    assert_eq!(text(&updated, "metadata"), r#"{"failed":true}"#);

    Ok(())
}

#[test]
fn update_status_and_set_failure_by_id_support_owner_and_credential_fallback_paths(
) -> StoreResult<()> {
    let db = memory_store();
    queue_fixture(
        &db,
        "owner-status",
        "did:owner",
        "default",
        "queued",
        "2026-01-01T00:00:00Z",
    )?;
    queue_fixture(
        &db,
        "credential-status",
        "did:other",
        "fallback",
        "queued",
        "2026-01-02T00:00:00Z",
    )?;

    update_e2ee_outbox_status(&db, "owner-status", " did:owner ", "fallback", "dropped")?;
    update_e2ee_outbox_status(&db, "credential-status", " ", " fallback ", "sent")?;
    assert_eq!(
        text(
            &get_e2ee_outbox(&db, "owner-status", "did:owner", "")?,
            "local_status"
        ),
        "dropped"
    );
    assert_eq!(
        text(
            &get_e2ee_outbox(&db, "credential-status", "", "fallback")?,
            "local_status"
        ),
        "sent"
    );

    set_e2ee_outbox_failure_by_id(
        &db,
        "owner-status",
        "did:owner",
        "fallback",
        "owner-error",
        "owner-retry",
        r#"{"scope":"owner"}"#,
    )?;
    let owner = get_e2ee_outbox(&db, "owner-status", "did:owner", "")?;
    assert_eq!(text(&owner, "local_status"), "failed");
    assert_eq!(text(&owner, "last_error_code"), "owner-error");
    assert_eq!(text(&owner, "retry_hint"), "owner-retry");
    assert_eq!(text(&owner, "metadata"), r#"{"scope":"owner"}"#);

    set_e2ee_outbox_failure_by_id(&db, "owner-status", "did:owner", "", "owner-2", " ", " ")?;
    let owner_preserved = get_e2ee_outbox(&db, "owner-status", "did:owner", "")?;
    assert_eq!(text(&owner_preserved, "last_error_code"), "owner-2");
    assert_eq!(text(&owner_preserved, "retry_hint"), "owner-retry");
    assert_eq!(text(&owner_preserved, "metadata"), r#"{"scope":"owner"}"#);

    set_e2ee_outbox_failure_by_id(
        &db,
        "credential-status",
        "",
        "fallback",
        "credential-error",
        "credential-retry",
        r#"{"scope":"credential"}"#,
    )?;
    let credential = get_e2ee_outbox(&db, "credential-status", "", "fallback")?;
    assert_eq!(text(&credential, "local_status"), "failed");
    assert_eq!(text(&credential, "last_error_code"), "credential-error");
    assert_eq!(text(&credential, "retry_hint"), "credential-retry");
    assert_eq!(text(&credential, "metadata"), r#"{"scope":"credential"}"#);

    Ok(())
}

fn memory_store() -> Connection {
    let db = Connection::open_in_memory().expect("open sqlite memory db");
    store::ensure_schema(&db).expect("ensure schema");
    db
}

fn queue_fixture(
    db: &Connection,
    outbox_id: &str,
    owner_did: &str,
    credential_name: &str,
    local_status: &str,
    updated_at: &str,
) -> StoreResult<String> {
    queue_e2ee_outbox(
        db,
        E2EEOutboxRecord {
            outbox_id: outbox_id.to_string(),
            owner_did: owner_did.to_string(),
            peer_did: "did:peer".to_string(),
            plaintext: format!("payload:{outbox_id}"),
            local_status: local_status.to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: updated_at.to_string(),
            credential_name: credential_name.to_string(),
            ..E2EEOutboxRecord::default()
        },
    )
}

fn ids(records: Vec<Value>) -> Vec<String> {
    records
        .into_iter()
        .map(|record| text(&record, "outbox_id").to_string())
        .collect()
}

fn text<'a>(record: &'a Value, field: &str) -> &'a str {
    record
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("missing string field {field}: {record:?}"))
}

fn int(record: &Value, field: &str) -> i64 {
    record
        .get(field)
        .and_then(Value::as_i64)
        .unwrap_or_else(|| panic!("missing integer field {field}: {record:?}"))
}

fn opt_int(record: &Value, field: &str) -> Option<i64> {
    match record.get(field) {
        Some(Value::Null) | None => None,
        Some(value) => value
            .as_i64()
            .or_else(|| panic!("missing optional integer field {field}: {record:?}")),
    }
}

fn optional_text(db: &Connection, column: &str, outbox_id: &str) -> Option<String> {
    db.query_row(
        &format!("SELECT {column} FROM e2ee_outbox WHERE outbox_id = ?1"),
        [outbox_id],
        |row| row.get::<_, Option<String>>(0),
    )
    .expect("query optional text")
}

fn optional_i64(db: &Connection, column: &str, outbox_id: &str) -> Option<i64> {
    db.query_row(
        &format!("SELECT {column} FROM e2ee_outbox WHERE outbox_id = ?1"),
        [outbox_id],
        |row| row.get::<_, Option<i64>>(0),
    )
    .expect("query optional i64")
}
