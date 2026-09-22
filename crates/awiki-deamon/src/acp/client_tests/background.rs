use super::*;

fn background_fixture() -> Fixture {
    let mut f = Fixture::new();
    f.profile.driver_id = "hermes".into();
    f.profile.config_home = Some(f.root.path().join("hermes-home"));
    f.task.trigger_kind = RuntimeTaskTriggerKind::DelegatedDirect;
    f.task.invocation_authority = RuntimeInvocationAuthority::Requester;
    f.task.conversation_scope =
        RuntimeConversationScope::direct("user-bob", "bob.example.com").unwrap();
    f.task.requester_user_id = Some("user-bob".into());
    f.task.requester_full_handle = Some("bob.example.com".into());
    f.task.sender_did = "did:human:bob".into();
    f.task.requester_did = f.task.sender_did.clone();
    f.task.task_id = "background-task".into();
    let session = Session::new(&f.task);
    assert_ne!(f.key, session.key);
    f.key = session.key.clone();
    store::mutate(&f.state, &f.key, Some(session), |s| {
        s.submit(Work {
            task: f.task.clone(),
            run_id: "background-run".into(),
        })
    })
    .unwrap();
    f
}

#[tokio::test]
async fn background_uses_acp_without_question_capabilities_or_chat_events() {
    let f = background_fixture();
    let output = run(f.turn("summarize the message")).await.unwrap();
    assert_eq!(output.text, "FIXTURE_RESPONSE");
    let caps: Value = serde_json::from_str(
        &std::fs::read_to_string(f.root.path().join("work/client-capabilities.json")).unwrap(),
    )
    .unwrap();
    assert!(caps.get("elicitation").is_none());
    let protocol: Value = serde_json::from_str(
        std::fs::read_to_string(f.root.path().join("work/protocol.jsonl"))
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(protocol["mcp_count"], 0);
    store::mutate(&f.state, &f.key, None, |s| {
        s.complete("background-run", "finished")
    })
    .unwrap();
    let db = f.state.connection().unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM acp_events WHERE session_key=?1",
            [&f.key],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    let record = crate::acp::task_records::load(&db, "background-run")
        .unwrap()
        .unwrap();
    assert!(record.background);
    assert_eq!(record.text, "FIXTURE_RESPONSE");
    assert!(record.questions.is_empty());
}

#[tokio::test]
async fn unexpected_background_question_ends_without_waiting_for_a_human() {
    for request in ["QUESTION_NATIVE", "QUESTION_PERMISSION"] {
        let f = background_fixture();
        let result = tokio::time::timeout(Duration::from_secs(5), run(f.turn(request)))
            .await
            .unwrap();
        assert!(result.is_err(), "{request}");
        assert!(store::load(&f.state, &f.key).unwrap().questions.is_empty());
    }
}
