use super::*;
use crate::acp::task_records;

#[test]
fn history_pages_filter_exact_sources_and_bound_total_bytes_without_truncation() {
    let (_root, state, seed) = fixture();
    mutate(&state, &seed.key, None, |s| {
        s.text = "a".repeat(300 * 1024);
        s.complete("run_a", "finished")?;
        s.submit(work("b", false))?;
        s.text = "b".repeat(300 * 1024);
        s.complete("run_b", "finished")?;
        Ok(())
    })
    .unwrap();
    let db = state.connection().unwrap();
    let first = task_records::page(&db, &seed.key, None, 20).unwrap();
    assert_eq!(first["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(
        first["tasks"][0]["text"].as_str().unwrap().len(),
        300 * 1024
    );
    assert_eq!(first["has_more"], true);
    let second = task_records::page(&db, &seed.key, first["next_cursor"].as_i64(), 20).unwrap();
    assert_eq!(second["tasks"][0]["run_id"], "run_a");
    assert_eq!(second["has_more"], false);
    let source = second["tasks"][0]["source_message_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let filtered =
        task_records::page_for_sources(&db, &seed.key, None, 20, Some(&[source])).unwrap();
    assert_eq!(filtered["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(filtered["tasks"][0]["run_id"], "run_a");
    assert_eq!(filtered["has_more"], false);
    assert_eq!(
        task_records::page_for_sources(&db, &seed.key, None, 20, Some(&["unknown".into()]))
            .unwrap()["tasks"],
        json!([])
    );
}

#[test]
fn finished_record_survives_auto_start_stream_coalescing_and_reopen() {
    let (_root, state, seed) = fixture();
    mutate(&state, &seed.key, None, |s| {
        s.text = "answer A".into();
        s.tools =
            vec![json!({"id":"read-1","kind":"read","title":"notes.md","status":"completed"})];
        s.submit(work("b", false))?;
        assert!(s.complete("run_a", "finished")?.is_some());
        Ok(())
    })
    .unwrap();
    for n in 0..3 {
        mutate(&state, &seed.key, None, |s| {
            s.text = format!("B chunk {n}");
            Ok(())
        })
        .unwrap();
    }
    let a = task_records::load(&state.connection().unwrap(), "run_a")
        .unwrap()
        .unwrap();
    assert_eq!(a.text, "answer A");
    assert_eq!(a.tools.len(), 1);
    assert_eq!(a.state, "finished");
    assert_eq!(a.delivery.state, "pending");
    let db = state.connection().unwrap();
    let event: String = db
        .query_row(
            "SELECT snapshot FROM acp_events WHERE event_id='acp-task:run_a:terminal'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&event).unwrap()["text"],
        "answer A"
    );
    let snapshots: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM acp_events WHERE event_kind='snapshot' AND sent=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(snapshots, 1);
    let page = task_records::page(&db, &seed.key, None, 1).unwrap();
    assert_eq!(page["tasks"][0]["run_id"], "run_b");
    let older = task_records::page(&db, &seed.key, page["next_cursor"].as_i64(), 1).unwrap();
    assert_eq!(older["tasks"][0]["text"], "answer A");
    assert_eq!(
        task_records::page(&db, "foreign", None, 20).unwrap()["tasks"],
        json!([])
    );
}

#[test]
fn interrupted_cancelled_and_failed_records_keep_partial_text_and_pause_waiting() {
    for outcome in ["interrupted", "cancelled", "failed"] {
        let (_root, state, seed) = fixture();
        mutate(&state, &seed.key, None, |s| {
            s.text = "useful partial answer".into();
            s.submit(work("b", false))?;
            Ok(())
        })
        .unwrap();
        if outcome == "interrupted" {
            recover(&state).unwrap();
        } else {
            mutate(&state, &seed.key, None, |s| s.complete("run_a", outcome)).unwrap();
        }
        let record = task_records::load(&state.connection().unwrap(), "run_a")
            .unwrap()
            .unwrap();
        assert_eq!(record.text, "useful partial answer");
        assert_eq!(record.state, outcome);
        assert_eq!(record.delivery.state, "none");
        let session = load(&state, &seed.key).unwrap();
        assert!(session.waiting_paused);
        assert!(session.active.is_none());
        let mut late = record.clone();
        late.state = "running".into();
        late.text = "late chunk".into();
        late.revision += 20;
        task_records::persist(&state.connection().unwrap(), &late).unwrap();
        assert_eq!(
            task_records::load(&state.connection().unwrap(), "run_a")
                .unwrap()
                .unwrap()
                .text,
            "useful partial answer"
        );
    }
}

#[test]
fn accepted_question_event_is_immutable_and_survives_later_snapshots() {
    let (_root, state, seed) = fixture();
    mutate(&state, &seed.key, None, |s| {
        s.questions.push(Question {
            id: "q1".into(),
            run_id: "run_a".into(),
            expires_at_ms: i64::MAX,
            request: json!({"message":"choose"}),
            response: None,
            interaction: None,
            end_reason: None,
        });
        Ok(())
    })
    .unwrap();
    mutate(&state, &seed.key, None, |s| {
        s.questions[0].response = Some(json!({"action":"accept","content":{"choice":"yes"}}));
        Ok(())
    })
    .unwrap();
    mutate(&state, &seed.key, None, |s| {
        s.text = "continued".into();
        Ok(())
    })
    .unwrap();
    mutate(&state, &seed.key, None, |s| s.complete("run_a", "finished")).unwrap();
    let db = state.connection().unwrap();
    let raw: String = db
        .query_row(
            "SELECT snapshot FROM acp_events WHERE event_id='acp-question:run_a:q1:terminal'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let event: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(event["questions"][0]["status"], "answered");
    assert_eq!(
        event["questions"][0]["response"]["content"]["choice"],
        "yes"
    );
    assert_eq!(event["text"], "");
    assert_eq!(
        task_records::load(&db, "run_a").unwrap().unwrap().questions[0]["status"],
        "answered"
    );
}

#[test]
fn event_schema_migration_preserves_existing_pending_rows() {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch("CREATE TABLE acp_events(event_id TEXT PRIMARY KEY, session_key TEXT NOT NULL, run_id TEXT NOT NULL, snapshot TEXT NOT NULL, sent INTEGER NOT NULL DEFAULT 0);
        INSERT INTO acp_events VALUES('old','s','r','{}',0);").unwrap();
    initialize(&db).unwrap();
    initialize(&db).unwrap();
    let kind: String = db
        .query_row(
            "SELECT event_kind FROM acp_events WHERE event_id='old'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(kind, "snapshot");
}

#[test]
fn final_delivery_and_task_event_commit_together_without_reexecution() {
    let (_root, state, seed) = fixture();
    let task = work("a", false).task;
    state.insert_runtime_task(&task).unwrap();
    let profile = state.load_runtime_agent_profile(&task.agent_did).unwrap();
    let run = crate::runtime::RuntimeRun {
        run_id: "run_a".into(),
        task_id: task.task_id.clone(),
        agent_did: task.agent_did.clone(),
        runtime_profile_id: profile.runtime_profile_id.clone(),
        runtime_plugin_id: "acp".into(),
        workspace_id: profile.workspace_id.clone(),
        status: crate::runtime::RuntimeRunStatus::Running,
    };
    state.try_insert_runtime_run(&run).unwrap();
    mutate(&state, &seed.key, None, |s| {
        s.text = "complete reply".into();
        Ok(())
    })
    .unwrap();
    let final_record = crate::runtime::host::runtime_final_outbox_record(
        &profile,
        &task.controller_did,
        &task.reply_recipient_did,
        &run,
        task.conversation_id.as_deref(),
        "complete reply",
        "acp",
    )
    .unwrap();
    finish_with_final(&state, &seed.key, &final_record).unwrap();
    state
        .mark_runtime_final_outbox_sending(&final_record.idempotency_key)
        .unwrap();
    let db = state.connection().unwrap();
    db.execute_batch(
        "CREATE TRIGGER reject_delivery_event BEFORE INSERT ON acp_events
        WHEN NEW.event_id LIKE '%:delivery:%' BEGIN SELECT RAISE(ABORT,'test disk failure'); END;",
    )
    .unwrap();
    assert!(state
        .mark_runtime_final_outbox_sent(&final_record.idempotency_key, Some("reply-id"))
        .is_err());
    assert_eq!(
        state
            .load_runtime_final_outbox_by_run("run_a")
            .unwrap()
            .unwrap()
            .status,
        "sending"
    );
    assert_eq!(
        task_records::load(&db, "run_a")
            .unwrap()
            .unwrap()
            .delivery
            .state,
        "pending"
    );
    db.execute_batch("DROP TRIGGER reject_delivery_event")
        .unwrap();
    assert!(state
        .mark_runtime_final_outbox_sent(&final_record.idempotency_key, Some("reply-id"))
        .unwrap());
    assert!(!state
        .mark_runtime_final_outbox_sent(&final_record.idempotency_key, Some("reply-id"))
        .unwrap());
    let record = task_records::load(&db, "run_a").unwrap().unwrap();
    assert_eq!(record.delivery.state, "sent");
    assert_eq!(record.delivery.message_id.as_deref(), Some("reply-id"));
    assert_eq!(record.text, "complete reply");
    assert!(load(&state, &seed.key).unwrap().active.is_none());
    let count: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM acp_events WHERE event_id='acp-task:run_a:delivery:sent'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}
