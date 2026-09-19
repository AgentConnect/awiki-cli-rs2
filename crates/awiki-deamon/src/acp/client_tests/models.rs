use super::*;

#[test]
fn session_configuration_is_idempotent_private_and_committed_only_after_confirmation() {
    let f = Fixture::new();
    f.state
        .connection()
        .unwrap()
        .execute("DELETE FROM acp_sessions", [])
        .unwrap();
    f.state.upsert_cli_runtime_profile(&f.profile).unwrap();
    let mut profile = f
        .state
        .load_runtime_agent_profile(&f.task.agent_did)
        .unwrap();
    profile.workspace_root = Some(f.root.path().join("configured-work"));
    profile.workspace_id = Some("model-test".into());
    profile.workspace_mode = Some(crate::workspace::WorkspaceMode::SharedRoot);
    f.state.upsert_runtime_agent_profile(&profile).unwrap();
    let prepare = json!({"action":"prepare_session"});
    let call = |command: &str, args: &Value| {
        crate::acp::session_configuration::control(
            &f.state,
            &profile,
            "did:human:alice",
            Some("core-direct".into()),
            command,
            args,
        )
    };
    let prepared = call("prepare-1", &prepare).unwrap();
    assert_eq!(prepared["model_id"], "flash");
    assert!(prepared["selected_model_id"].is_null());
    assert_eq!(prepared["conversation_id"], "core-direct");
    assert_eq!(call("prepare-1", &prepare).unwrap(), prepared);
    let cwd = profile
        .workspace_root
        .as_ref()
        .unwrap()
        .join("acp")
        .join(&f.key);
    assert_eq!(
        std::fs::read_to_string(cwd.join("protocol.jsonl"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    assert!(!cwd.join("prompts.jsonl").exists());
    assert!(store::load(&f.state, &f.key)
        .unwrap()
        .native_session_id
        .is_none());
    let select = json!({"action":"set_model","session_key":f.key,"revision":prepared["revision"],"model_id":"pro"});
    let selected = call("select-1", &select).unwrap();
    assert_eq!(selected["model_id"], "pro");
    assert_eq!(selected["selected_model_id"], "pro");
    assert_eq!(call("select-1", &select).unwrap(), selected);
    assert!(call("select-conflict", &select)
        .unwrap_err()
        .to_string()
        .contains("stale_revision"));
    assert!(call("select-1", &json!({"action":"prepare_session"}))
        .unwrap_err()
        .to_string()
        .contains("command_id_conflict"));
    assert!(call(
        "wrong-session",
        &json!({"action":"set_model","session_key":"foreign","revision":1,"model_id":"pro"})
    )
    .is_err());
    std::fs::write(cwd.join("model-mode"), "reject").unwrap();
    let failure = json!({"action":"set_model","session_key":f.key,"revision":selected["revision"],"model_id":"pro"});
    assert!(call("select-failure", &failure).is_err());
    assert_eq!(store::load(&f.state, &f.key).unwrap().snapshot(), selected);
    assert!(!cwd.join("prompts.jsonl").exists());
}

#[test]
fn concurrent_model_changes_have_one_revision_winner() {
    let f = Fixture::new();
    store::mutate(&f.state, &f.key, None, |s| {
        s.complete("run_a", "finished")?;
        Ok(())
    })
    .unwrap();
    f.state.upsert_cli_runtime_profile(&f.profile).unwrap();
    let mut profile = f
        .state
        .load_runtime_agent_profile(&f.task.agent_did)
        .unwrap();
    profile.workspace_root = Some(f.root.path().join("configured-work"));
    profile.workspace_id = Some("model-test".into());
    profile.workspace_mode = Some(crate::workspace::WorkspaceMode::SharedRoot);
    f.state.upsert_runtime_agent_profile(&profile).unwrap();
    let prepared = crate::acp::session_configuration::control(
        &f.state,
        &profile,
        "did:human:alice",
        Some("core-direct".into()),
        "p",
        &json!({"action":"prepare_session"}),
    )
    .unwrap();
    let results = std::thread::scope(|scope| {
        let handles = ["flash","pro"].map(|model| {
            let state=&f.state; let profile=&profile; let key=&f.key; let revision=prepared["revision"].clone();
            scope.spawn(move || crate::acp::session_configuration::control(state,profile,"did:human:alice",Some("core-direct".into()),model,&json!({"action":"set_model","session_key":key,"revision":revision,"model_id":model})))
        });
        handles.map(|h| h.join().unwrap())
    });
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert!(results
        .iter()
        .filter_map(|r| r.as_ref().err())
        .all(|e| e.to_string() == "stale_revision"));
}

#[tokio::test]
async fn prepare_configuration_never_prompts_or_persists_a_probe_native_session() {
    let f = Fixture::new();
    let prepared = prepare_configuration(
        f.profile.clone(),
        f.root.path().join("work"),
        None,
        Some("pro".into()),
    )
    .await
    .unwrap();
    assert_eq!(current_model(&prepared.options).as_deref(), Some("pro"));
    assert!(!f.root.path().join("work/prompts.jsonl").exists());
    assert!(store::load(&f.state, &f.key)
        .unwrap()
        .native_session_id
        .is_none());
    run(f.turn("hello")).await.unwrap();
    let log = std::fs::read_to_string(f.root.path().join("work/protocol.jsonl")).unwrap();
    assert_eq!(log.matches("session/new").count(), 2);
    let native = store::load(&f.state, &f.key).unwrap().native_session_id;
    prepare_configuration(f.profile, f.root.path().join("work"), native, None)
        .await
        .unwrap();
    let prompts = std::fs::read_to_string(f.root.path().join("work/prompts.jsonl")).unwrap();
    assert_eq!(prompts.lines().count(), 1);
    assert_eq!(
        store::load(&f.state, &f.key).unwrap().text,
        "FIXTURE_RESPONSE"
    );
}

#[tokio::test]
async fn default_model_and_native_configuration_updates_are_reported() {
    let f = Fixture::new();
    run(f.turn("hello")).await.unwrap();
    let session = store::load(&f.state, &f.key).unwrap();
    assert_eq!(session.model.as_deref(), Some("flash"));
    assert_eq!(session.model_selection(), None);
    f.next();
    run(f.turn("CONFIG_UPDATE")).await.unwrap();
    let session = store::load(&f.state, &f.key).unwrap();
    assert_eq!(session.model.as_deref(), Some("pro"));
    assert_eq!(session.model_selection(), None);
}

#[tokio::test]
async fn selected_model_requires_native_confirmation_before_prompt() {
    for mode in ["", "legacy", "reject", "wrong-current"] {
        let f = Fixture::new();
        std::fs::write(f.root.path().join("work/model-mode"), mode).unwrap();
        store::mutate(&f.state, &f.key, None, |session| {
            session.selected_model = Some("pro".into());
            session.model = Some("pro".into());
            Ok(())
        })
        .unwrap();
        let result = run(f.turn("hello")).await;
        let succeeds = mode == "" || mode == "legacy";
        assert_eq!(
            result.is_ok(),
            succeeds,
            "{mode}: {:?}",
            result.as_ref().err()
        );
        assert_eq!(f.root.path().join("work/prompts.jsonl").exists(), succeeds);
        assert_eq!(
            store::load(&f.state, &f.key).unwrap().model.as_deref(),
            Some("pro")
        );
    }
}

#[test]
fn legacy_model_selection_is_migrated_without_turning_reported_default_into_intent() {
    let f = Fixture::new();
    let mut session = store::load(&f.state, &f.key).unwrap();
    session.configuration_version = 0;
    session.model = Some("pro".into());
    assert_eq!(session.model_selection().as_deref(), Some("pro"));
    session.update_configuration(json!({"currentModelId":"flash","availableModels":[]}));
    assert_eq!(session.model_selection().as_deref(), Some("pro"));
    assert_eq!(session.model.as_deref(), Some("flash"));
    assert_eq!(
        current_model(&json!([{"id":"mode","currentValue":"auto"}])),
        None
    );
    assert_eq!(
        current_model(&json!([{"id":"model","currentValue":""}])),
        None
    );
}
