use rusqlite::Connection;

use super::*;

const OWNER_ID: &str = "owner-agent-message";
const OWNER_DID: &str = "did:example:owner";
const PEER_DID: &str = "did:example:agent";

fn valid_payload(summary: &str) -> String {
    serde_json::json!({
        "schema": crate::messages::AGENT_MESSAGE_SCHEMA_V1,
        "event_id": "event-visible-001",
        "task_name": "Release verification",
        "kind": "task_result",
        "level": "normal",
        "content": {"summary": summary},
        "action": {"type": "open_conversation"}
    })
    .to_string()
}

fn record(msg_id: &str, conversation_id: &str, content: &str, stored_at: &str) -> MessageRecord {
    MessageRecord {
        msg_id: msg_id.to_owned(),
        owner_identity_id: OWNER_ID.to_owned(),
        owner_did: OWNER_DID.to_owned(),
        conversation_id: conversation_id.to_owned(),
        thread_id: conversation_id.to_owned(),
        direction: 0,
        sender_did: PEER_DID.to_owned(),
        receiver_did: OWNER_DID.to_owned(),
        content_type: "application/json".to_owned(),
        content: content.to_owned(),
        sent_at: stored_at.to_owned(),
        stored_at: stored_at.to_owned(),
        ..MessageRecord::default()
    }
}

#[test]
fn valid_and_invalid_exact_schema_both_contribute_summary_and_unread() {
    let db = Connection::open_in_memory().unwrap();
    let conversation_id = "dm:did:example:visible-agent";
    let valid = valid_payload("Build completed");
    let invalid = serde_json::json!({
        "schema": crate::messages::AGENT_MESSAGE_SCHEMA_V1,
        "event_id": "event-visible-002",
        "task_name": "Production release",
        "kind": "alert",
        "level": "urgent",
        "content": {"summary": "token=must-not-render"},
        "action": {"type": "open_conversation"}
    })
    .to_string();

    upsert_message(
        &db,
        &record(
            "msg-visible-valid",
            conversation_id,
            &valid,
            "2026-08-11T00:00:00Z",
        ),
    )
    .unwrap();
    upsert_message(
        &db,
        &record(
            "msg-visible-invalid",
            conversation_id,
            &invalid,
            "2026-08-11T00:00:01Z",
        ),
    )
    .unwrap();

    assert_eq!(message_is_read(&db, "msg-visible-valid"), 0);
    assert_eq!(message_is_read(&db, "msg-visible-invalid"), 0);
    assert_eq!(
        summary(&db, conversation_id),
        (2, 2, "msg-visible-invalid".to_owned())
    );

    super::super::conversation_summaries::rebuild_owner(&db, OWNER_ID).unwrap();
    assert_eq!(
        summary(&db, conversation_id),
        (2, 2, "msg-visible-invalid".to_owned())
    );

    let public = crate::internal::message_runtime::conversations::message_from_record(&record(
        "msg-visible-invalid",
        conversation_id,
        &invalid,
        "2026-08-11T00:00:01Z",
    ))
    .unwrap();
    let crate::messages::MessageBodyView::Payload { payload } = public.body else {
        panic!("invalid visible message must remain a generic visible body");
    };
    assert_eq!(
        payload,
        serde_json::json!({"schema": crate::messages::AGENT_MESSAGE_SCHEMA_V1})
    );
    assert!(!payload.to_string().contains("token"));
}

#[test]
fn other_awiki_schemas_remain_hidden_and_read() {
    let db = Connection::open_in_memory().unwrap();
    for (index, schema) in [
        "awiki.agent.status.v1",
        "awiki.agent.command.v1",
        "awiki.future.unknown.v9",
    ]
    .into_iter()
    .enumerate()
    {
        let msg_id = format!("msg-control-{index}");
        let conversation_id = format!("dm:control:{index}");
        let payload = serde_json::json!({"schema": schema}).to_string();
        upsert_message(
            &db,
            &record(
                &msg_id,
                &conversation_id,
                &payload,
                &format!("2026-08-11T00:01:0{index}Z"),
            ),
        )
        .unwrap();
        assert_eq!(message_is_read(&db, &msg_id), 1);
        assert!(!summary_exists(&db, &conversation_id));
    }
}

#[test]
fn legacy_hidden_exact_schema_becomes_visible_without_synthesizing_unread() {
    let db = Connection::open_in_memory().unwrap();
    crate::internal::local_state::schema::ensure_schema(&db).unwrap();
    let conversation_id = "dm:legacy-visible";
    let payload = valid_payload("Previously hidden result");

    // Simulates a pre-feature row already marked read by the old broad awiki.* classifier.
    db.execute(
        r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, conversation_id, thread_id, direction,
     sender_did, receiver_did, content_type, content, sent_at, stored_at, is_read)
VALUES (?1, ?2, ?3, ?4, ?4, 0, ?5, ?3, 'application/json', ?6, ?7, ?7, 1)"#,
        rusqlite::params![
            "msg-legacy-visible",
            OWNER_ID,
            OWNER_DID,
            conversation_id,
            PEER_DID,
            payload,
            "2026-08-10T23:59:00Z"
        ],
    )
    .unwrap();

    super::super::conversation_summaries::rebuild_owner(&db, OWNER_ID).unwrap();
    assert_eq!(message_is_read(&db, "msg-legacy-visible"), 1);
    assert_eq!(
        summary(&db, conversation_id),
        (1, 0, "msg-legacy-visible".to_owned())
    );
    assert_eq!(repair_control_payload_read_projection(&db).unwrap(), 0);
    assert_eq!(message_is_read(&db, "msg-legacy-visible"), 1);
    assert_eq!(
        summary(&db, conversation_id),
        (1, 0, "msg-legacy-visible".to_owned())
    );
}

fn message_is_read(db: &Connection, msg_id: &str) -> i64 {
    db.query_row(
        "SELECT is_read FROM messages WHERE owner_identity_id = ?1 AND msg_id = ?2",
        (OWNER_ID, msg_id),
        |row| row.get(0),
    )
    .unwrap()
}

fn summary(db: &Connection, conversation_id: &str) -> (i64, i64, String) {
    db.query_row(
        "SELECT message_count, unread_count, last_message_id FROM conversation_summaries WHERE owner_identity_id = ?1 AND conversation_id = ?2",
        (OWNER_ID, conversation_id),
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )
    .unwrap()
}

fn summary_exists(db: &Connection, conversation_id: &str) -> bool {
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM conversation_summaries WHERE owner_identity_id = ?1 AND conversation_id = ?2)",
        (OWNER_ID, conversation_id),
        |row| row.get(0),
    )
    .unwrap()
}
