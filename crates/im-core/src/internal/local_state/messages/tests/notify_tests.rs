use super::*;

#[test]
fn notify_realtime_then_hydrated_sync_retains_intent_and_new_metadata() {
    let db = Connection::open_in_memory().unwrap();
    crate::internal::local_state::schema::ensure_schema(&db).unwrap();
    for level in ["normal", "urgent", "invalid"] {
        let mut record = local_inbox_record(
            level,
            "owner",
            "did:example:owner",
            "direct",
            false,
            "2026-09-22T00:00:00Z",
        );
        record.metadata =
            serde_json::json!({"notify_level":level,"realtime_only":"old"}).to_string();
        upsert_message(&db, &record).unwrap();
        record.metadata = r#"{"sync_event_id":"event-1"}"#.to_owned();
        upsert_message(&db, &record).unwrap();
        let raw: String = db
            .query_row(
                "SELECT metadata FROM messages WHERE msg_id = ?1",
                [level],
                |row| row.get(0),
            )
            .unwrap();
        let metadata: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(metadata["notify_level"], level);
        assert_eq!(metadata["sync_event_id"], "event-1");
        assert!(metadata.get("realtime_only").is_none());
    }
}

#[test]
fn notify_preservation_does_not_cross_group_or_secure_boundary() {
    for (kind, secure) in [("group", false), ("direct", true)] {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let mut record = local_inbox_record(
            "other",
            "owner",
            "did:example:owner",
            kind,
            false,
            "2026-09-22T00:00:00Z",
        );
        record.is_e2ee = secure;
        record.metadata = r#"{"notify_level":"urgent"}"#.to_owned();
        upsert_message(&db, &record).unwrap();
        record.metadata = r#"{"sync_event_id":"event-1"}"#.to_owned();
        upsert_message(&db, &record).unwrap();
        let raw: String = db
            .query_row("SELECT metadata FROM messages LIMIT 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert!(serde_json::from_str::<serde_json::Value>(&raw)
            .unwrap()
            .get("notify_level")
            .is_none());
    }
}
