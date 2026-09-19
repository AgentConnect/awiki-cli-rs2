use super::*;
use crate::outbox::MemoryRuntimeOutbox;

fn final_record(state: &DaemonState, work: &Work) -> crate::state::RuntimeFinalOutboxRecord {
    let profile = state
        .load_runtime_agent_profile(&work.task.agent_did)
        .unwrap();
    let run = crate::runtime::RuntimeRun {
        run_id: work.run_id.clone(),
        task_id: work.task.task_id.clone(),
        agent_did: profile.agent_did.clone(),
        runtime_profile_id: profile.runtime_profile_id.clone(),
        runtime_plugin_id: "acp".into(),
        workspace_id: profile.workspace_id.clone(),
        status: crate::runtime::RuntimeRunStatus::Running,
    };
    state.insert_runtime_task(&work.task).unwrap();
    state.try_insert_runtime_run(&run).unwrap();
    crate::runtime::host::runtime_final_outbox_record(
        &profile,
        &work.task.controller_did,
        &work.task.reply_recipient_did,
        &run,
        work.task.conversation_id.as_deref(),
        "answer",
        "acp",
    )
    .unwrap()
}

#[test]
fn waiting_alias_cannot_change_active_final_binding() {
    let (_root, state, seed) = fixture();
    let first = final_record(&state, &work("a", false));
    let mut waiting = work("b", false);
    waiting.task.conversation_id = Some("direct:another-device".into());
    waiting.task.reply_recipient_did = "did:human:another-device".into();
    waiting.task.controller_did = waiting.task.reply_recipient_did.clone();
    waiting.task.sender_did = waiting.task.reply_recipient_did.clone();
    waiting.task.requester_did = waiting.task.reply_recipient_did.clone();
    mutate(&state, &seed.key, None, |s| s.submit(waiting.clone())).unwrap();
    assert_eq!(
        load(&state, &seed.key).unwrap().conversation_id,
        waiting.task.conversation_id
    );
    let (cancelled, next) = finish_with_final(&state, &seed.key, &first).unwrap();
    assert!(!cancelled);
    assert_eq!(next.unwrap().run_id, waiting.run_id);
    let stored = state
        .load_runtime_final_outbox_by_run("run_a")
        .unwrap()
        .unwrap();
    assert_eq!(stored.conversation_id, first.conversation_id);
    assert_eq!(stored.recipient_did, first.recipient_did);
    assert!(finish_with_final(&state, &seed.key, &first).is_err());
    let second = final_record(&state, &waiting);
    finish_with_final(&state, &seed.key, &second).unwrap();
    assert!(load(&state, &seed.key).unwrap().active.is_none());
}

#[test]
fn final_binding_rejects_wrong_route_recipient_and_controller() {
    let (_root, state, seed) = fixture();
    let record = final_record(&state, &work("a", false));
    for field in ["conversation", "recipient", "controller", "scope", "agent"] {
        let mut wrong = record.clone();
        match field {
            "conversation" => wrong.conversation_id = Some("other-route".into()),
            "recipient" => wrong.recipient_did = "did:other".into(),
            "controller" => wrong.controller_did = "did:other".into(),
            "scope" => wrong.controller_scope_key = "other-scope".into(),
            _ => wrong.agent_did = "did:other".into(),
        }
        assert!(
            finish_with_final(&state, &seed.key, &wrong).is_err(),
            "{field}"
        );
        assert!(state
            .load_runtime_final_outbox_by_run("run_a")
            .unwrap()
            .is_none());
        assert!(load(&state, &seed.key).unwrap().active_run("run_a"));
    }
}

#[test]
fn failed_event_window_cannot_starve_later_healthy_events() {
    let (_root, state, _seed) = fixture();
    final_record(&state, &work("healthy", false));
    let db = state.connection().unwrap();
    db.execute("DELETE FROM acp_events", []).unwrap();
    for index in 0..64 {
        db.execute("INSERT INTO acp_events(event_id,session_key,run_id,snapshot) VALUES(?1,'bad','missing','{}')", [format!("failed-{index}")]).unwrap();
    }
    db.execute("INSERT INTO acp_events(event_id,session_key,run_id,snapshot) VALUES('healthy','healthy','run_healthy','{}')", []).unwrap();
    let outbox = MemoryRuntimeOutbox::default();
    assert!(crate::acp::host::flush_events(&state, &outbox, 64).is_err());
    let attempts: i64 = db
        .query_row("SELECT SUM(attempt_count) FROM acp_events", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(attempts, 64);
    // Even if every earlier retry becomes due, it cannot jump ahead of an
    // event which has never been tried. No real-time sleep is needed.
    db.execute(
        "UPDATE acp_events SET next_attempt_at_ms=1 WHERE attempt_count>0",
        [],
    )
    .unwrap();
    for _ in 0..2 {
        let _ = crate::acp::host::flush_events(&state, &outbox, 64);
    }
    let sent: i64 = db
        .query_row(
            "SELECT sent FROM acp_events WHERE event_id='healthy'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(sent, 1);
    assert!(outbox
        .records()
        .iter()
        .any(|r| r.idempotency_key.as_deref() == Some("healthy")));
}

#[test]
fn identity_fenced_events_and_finals_release_their_delivery_windows() {
    let (_root, state, seed) = fixture();
    let profile = state.load_runtime_agent_profile(&seed.agent_did).unwrap();
    state
        .upsert_runtime_daemon_binding(
            &profile.agent_did,
            "did:daemon:fenced",
            &profile.controller_user_id,
            &profile.controller_full_handle,
            &profile.controller_scope_key,
            &profile.controller_did,
        )
        .unwrap();
    crate::agent_status::record_controller_identity_changed(&state, "did:daemon:fenced", "test")
        .unwrap_err();
    let mut healthy_profile = profile.clone();
    healthy_profile.agent_did = "did:agent:healthy".into();
    healthy_profile.runtime_profile_id = "healthy-profile".into();
    state
        .upsert_runtime_agent_profile(&healthy_profile)
        .unwrap();
    let db = state.connection().unwrap();
    db.execute("DELETE FROM acp_events", []).unwrap();
    for i in 0..65 {
        let mut task = work(&format!("fenced-{i}"), false);
        if i == 64 {
            task.task.agent_did = healthy_profile.agent_did.clone();
        }
        let record = final_record(&state, &task);
        state.upsert_runtime_final_outbox_pending(&record).unwrap();
        db.execute("INSERT INTO acp_events(event_id,session_key,run_id,snapshot) VALUES(?1,'events',?2,'{}')",
            params![format!("event-{i}"), task.run_id]).unwrap();
    }
    let outbox = MemoryRuntimeOutbox::default();
    assert_eq!(
        crate::acp::host::flush_events(&state, &outbox, 64).unwrap(),
        0
    );
    assert_eq!(
        crate::acp::host::flush_events(&state, &outbox, 64).unwrap(),
        1
    );
    let blocked: i64 = db.query_row("SELECT COUNT(*) FROM acp_events WHERE sent=0 AND blocked_reason='controller_identity_changed'", [], |r| r.get(0)).unwrap();
    assert_eq!(blocked, 64);
    assert_eq!(
        crate::runtime::host::flush_runtime_final_outbox(&state, &outbox, 64).unwrap(),
        0
    );
    assert_eq!(
        crate::runtime::host::flush_runtime_final_outbox(&state, &outbox, 64).unwrap(),
        1
    );
    assert!(state
        .list_due_runtime_final_outbox(i64::MAX, 64)
        .unwrap()
        .is_empty());
    assert_eq!(
        state
            .load_runtime_final_outbox_by_run("run_fenced-0")
            .unwrap()
            .unwrap()
            .status,
        "failed_terminal"
    );
    assert!(outbox
        .records()
        .iter()
        .all(|r| r.agent_did == healthy_profile.agent_did));
    // Reopening cannot put fenced rows back into the ready window.
    let config = DaemonConfig::for_state_root(_root.path()).unwrap();
    let reopened = DaemonState::open_with_root_key_bytes(&config, [7; 32]);
    reopened.initialize().unwrap();
    assert_eq!(
        crate::acp::host::flush_events(&reopened, &outbox, 64).unwrap(),
        0
    );
}
