use super::*;

fn setup(f: &Fixture) -> RuntimeAgentProfile {
    f.state.upsert_cli_runtime_profile(&f.profile).unwrap();
    let mut profile = f
        .state
        .load_runtime_agent_profile(&f.task.agent_did)
        .unwrap();
    profile.workspace_root = Some(f.root.path().join("configured-work"));
    profile.workspace_id = Some("model-test".into());
    profile.workspace_mode = Some(crate::workspace::WorkspaceMode::SharedRoot);
    f.state.upsert_runtime_agent_profile(&profile).unwrap();
    store::mutate(&f.state, &f.key, None, |s| {
        s.complete("run_a", "finished")?;
        s.update_configuration(json!({"currentModelId":"pro","availableModels":[]}));
        s.selected_model = Some("pro".into());
        Ok(())
    })
    .unwrap();
    profile
}

fn refresh(f: &Fixture, profile: &RuntimeAgentProfile, id: &str) -> Result<Value> {
    crate::acp::model_refresh::control(
        &f.state,
        profile,
        "did:human:alice",
        f.task.conversation_id.clone(),
        id,
        &json!({"action":"refresh_models","session_key":f.key}),
    )
}

#[test]
fn model_refresh_updates_catalog_without_switching_or_prompting_and_replays_command() {
    let f = Fixture::new();
    let profile = setup(&f);
    let cwd = crate::acp::host::workspace(&profile, &f.key).unwrap();
    std::fs::write(
        cwd.join("catalog.json"),
        r#"[{"value":"new-model","name":"New model"}]"#,
    )
    .unwrap();
    let result = refresh(&f, &profile, "refresh").unwrap();
    assert_eq!(result["model_refresh"]["state"], "refreshed");
    let session = &result["sessions"][0];
    assert_eq!(session["model_id"], "pro");
    assert_eq!(session["selected_model_id"], "pro");
    assert_eq!(session["models"][0]["id"], "new-model");
    assert!(session["model_catalog_updated_at_ms"].as_i64().unwrap() > 0);
    assert_eq!(session["model_refresh_supported"], true);
    assert_eq!(result, refresh(&f, &profile, "refresh").unwrap());
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
    // Failure keeps the entire previously confirmed snapshot.
    std::fs::write(cwd.join("catalog.json"), "invalid json").unwrap();
    assert!(refresh(&f, &profile, "bad").is_err());
    assert_eq!(&store::load(&f.state, &f.key).unwrap().snapshot(), session);
}

#[test]
fn model_refresh_defers_busy_and_gemini_checkpoint_without_marking_cache_fresh() {
    let mut f = Fixture::new();
    let profile = setup(&f);
    store::mutate(&f.state, &f.key, None, |s| {
        s.submit(Work {
            task: f.task.clone(),
            run_id: "busy".into(),
        })?;
        Ok(())
    })
    .unwrap();
    let before = store::load(&f.state, &f.key).unwrap().snapshot();
    assert_eq!(
        refresh(&f, &profile, "busy").unwrap()["model_refresh"]["state"],
        "deferred"
    );
    assert_eq!(store::load(&f.state, &f.key).unwrap().snapshot(), before);
    f.profile.driver_id = "gemini".into();
    f.state.upsert_cli_runtime_profile(&f.profile).unwrap();
    store::mutate(&f.state, &f.key, None, |s| {
        s.complete("busy", "finished")?;
        s.native_session_id = Some("native-exact-session".into());
        s.native_created_at_ms = Some(current_time_millis()?);
        Ok(())
    })
    .unwrap();
    let result = refresh(&f, &profile, "checkpoint").unwrap();
    assert_eq!(result["model_refresh"]["state"], "deferred");
    assert!(result["model_refresh"]["retry_after_ms"].as_i64().unwrap() > 0);
}

#[test]
fn concurrent_catalog_reads_are_coalesced() {
    let f = Fixture::new();
    let profile = setup(&f);
    let cwd = crate::acp::host::workspace(&profile, &f.key).unwrap();
    std::fs::write(cwd.join("slow-catalog"), "short").unwrap();
    let results = std::thread::scope(|scope| {
        let first = scope.spawn(|| refresh(&f, &profile, "one"));
        wait_for_file(&cwd.join("catalog.pid"));
        let second = scope.spawn(|| refresh(&f, &profile, "two"));
        [
            first.join().unwrap().unwrap(),
            second.join().unwrap().unwrap(),
        ]
    });
    for result in results {
        assert_eq!(result["model_refresh"]["state"], "refreshed");
    }
    assert_eq!(
        std::fs::read_to_string(cwd.join("protocol.jsonl"))
            .unwrap()
            .lines()
            .count(),
        1
    );
}

fn wait_for_file(path: &std::path::Path) {
    let start = std::time::Instant::now();
    while !path.exists() {
        assert!(start.elapsed().as_secs() < 10, "fixture did not start");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn foreground_admission_cancels_catalog_process_before_acquiring_session() {
    let f = Fixture::new();
    let profile = setup(&f);
    let cwd = crate::acp::host::workspace(&profile, &f.key).unwrap();
    std::fs::write(cwd.join("slow-catalog"), "long").unwrap();
    let before = store::load(&f.state, &f.key).unwrap().snapshot();
    std::thread::scope(|scope| {
        let refresh = scope.spawn(|| refresh(&f, &profile, "preempted"));
        wait_for_file(&cwd.join("catalog.pid"));
        let started = std::time::Instant::now();
        let gate = crate::acp::operations::session_gate(&f.state, &f.key);
        let _guard = gate.lock().unwrap();
        assert!(started.elapsed().as_secs() < 5);
        assert_eq!(
            refresh.join().unwrap().unwrap()["model_refresh"]["state"],
            "deferred"
        );
        for name in ["catalog.pid", "catalog-child.pid"] {
            let pid: i32 = std::fs::read_to_string(cwd.join(name))
                .unwrap()
                .parse()
                .unwrap();
            let start = std::time::Instant::now();
            while unsafe { libc::kill(pid, 0) } == 0 {
                assert!(
                    start.elapsed().as_secs() < 5,
                    "catalog process {pid} survived cancellation"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    });
    assert_eq!(store::load(&f.state, &f.key).unwrap().snapshot(), before);
}
