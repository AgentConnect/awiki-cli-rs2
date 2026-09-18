use super::*;

/// The owner supplies a disposable official CLI config directory with a catalog
/// change. Credentials stay in that protected directory, never in this report.
#[tokio::test]
#[ignore = "requires AWIKI_ACP_REAL_FIXTURE and real configured clients"]
async fn real_client_catalog_change_and_model_resume() {
    let settings: Value = serde_json::from_slice(
        &std::fs::read(std::env::var("AWIKI_ACP_REAL_FIXTURE").unwrap()).unwrap(),
    )
    .unwrap();
    let mut f = Fixture::new();
    f.profile.driver_id = settings["driver_id"].as_str().unwrap().into();
    f.profile.binary_path = Some(settings["binary_path"].as_str().unwrap().into());
    let home = std::path::PathBuf::from(settings["config_home"].as_str().unwrap())
        .canonicalize()
        .unwrap();
    let catalog = std::path::PathBuf::from(settings["catalog_file"].as_str().unwrap())
        .canonicalize()
        .unwrap();
    assert!(catalog.starts_with(&home));
    f.profile.config_home = Some(home);
    f.state.upsert_cli_runtime_profile(&f.profile).unwrap();
    let mut profile = f
        .state
        .load_runtime_agent_profile(&f.task.agent_did)
        .unwrap();
    profile.workspace_root = Some(f.root.path().join("configured-work"));
    profile.workspace_id = Some("real-model-test".into());
    profile.workspace_mode = Some(crate::workspace::WorkspaceMode::SharedRoot);
    f.state.upsert_runtime_agent_profile(&profile).unwrap();
    store::mutate(&f.state, &f.key, None, |s| {
        s.complete("run_a", "finished")?;
        Ok(())
    })
    .unwrap();
    let probe = inspect(&f.profile).await.unwrap();
    let mut cases = vec![];
    let call = |id: &str, args: Value| {
        crate::acp::session_configuration::control(
            &f.state,
            &profile,
            "did:human:alice",
            f.task.conversation_id.clone(),
            id,
            &args,
        )
    };
    let initial = call("prepare", json!({"action":"prepare_session"})).unwrap();
    let current = settings["initial_model"].as_str().unwrap();
    let new_model = settings["new_model"].as_str().unwrap();
    let initial_valid = initial["model_id"] == current
        && !initial["models"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["id"] == new_model);
    cases.push(json!({"case":"initial_catalog","pass":initial_valid}));
    assert!(
        initial_valid,
        "initial model/catalog mismatch: current={} models={}",
        initial["model_id"], initial["models"]
    );
    let cwd = crate::acp::host::workspace(&profile, &f.key).unwrap();
    let nonce = format!("MODEL_{}", rand::random::<u64>());
    let mut expected_native = None;
    std::fs::write(cwd.join("proof.txt"), &nonce).unwrap();
    for (index, prompt) in [
        "Read proof.txt with a file tool. Reply with its exact contents only, without other text.",
        "Repeat the exact contents of proof.txt from our previous conversation. Do not use tools. Reply only with that content.",
    ].into_iter().enumerate() {
        let run_id = format!("real_model_{index}");
        let mut task = f.task.clone(); task.task_id = run_id.clone();
        store::mutate(&f.state,&f.key,None,|s| {s.submit(Work {task,run_id:run_id.clone()})?;Ok(())}).unwrap();
        let mut turn = f.turn(prompt);turn.cwd=cwd.clone();
        let output = run(turn).await;
        let snapshot = store::load(&f.state,&f.key).unwrap();
        let pass = output.as_ref().is_ok_and(|o|o.text.trim()==nonce)
            && snapshot.model.as_deref()==Some(if index==0 {current} else {new_model})
            && (index==0 || snapshot.native_session_id==expected_native);
        expected_native=snapshot.native_session_id.clone();
        cases.push(json!({"case":if index==0 {"initial_tools"} else {"switched_resume"},"pass":pass,
            "model_id":snapshot.model,"exact_output":output.as_ref().is_ok_and(|o|o.text.trim()==nonce),
            "error":output.err().map(|e|e.to_string())}));
        store::mutate(&f.state,&f.key,None,|s|{s.complete(&run_id,"finished")?;Ok(())}).unwrap();
        if index == 0 {
            let native = snapshot.native_session_id.clone();
            std::fs::copy(settings["updated_catalog_file"].as_str().unwrap(),&catalog).unwrap();
            let refreshed = loop {
                let result = crate::acp::model_refresh::control(&f.state,&profile,"did:human:alice",f.task.conversation_id.clone(),"refresh",
                    &json!({"action":"refresh_models","session_key":f.key})).unwrap();
                if result["model_refresh"]["state"]=="refreshed" {break result;}
                let ms = result["model_refresh"]["retry_after_ms"].as_u64().expect("unexpected defer");
                assert!(ms<=65000);
                tokio::time::sleep(Duration::from_millis(ms)).await;
            };
            let refreshed = &refreshed["sessions"][0];
            cases.push(json!({"case":"catalog_added_without_switch","pass":
                refreshed["model_id"]==current && refreshed["models"].as_array().unwrap().iter().any(|m|m["id"]==new_model)
                && store::load(&f.state,&f.key).unwrap().native_session_id==native}));
            let selected = call("select",json!({"action":"set_model","session_key":f.key,"revision":refreshed["revision"],"model_id":new_model})).unwrap();
            cases.push(json!({"case":"confirmed_switch","pass":selected["model_id"]==new_model && selected["selected_model_id"]==new_model}));
            assert_eq!(store::load(&f.state,&f.key).unwrap().native_session_id,native);
        }
    }
    let report = json!({"driver_id":f.profile.driver_id,"version":probe["binaryVersion"],"adapter_version":probe["agentInfo"]["version"],"platform":std::env::consts::OS,"cases":cases});
    std::fs::write(
        settings["report_path"].as_str().unwrap(),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    assert!(
        cases.iter().all(|case| case["pass"] == true),
        "real model catalog flow failed; see sanitized report"
    );
}
