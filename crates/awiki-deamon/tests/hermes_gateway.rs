use awiki_deamon::plugins::hermes::{
    FakeHermesBehavior, FakeHermesGateway, HermesGateway, HermesPromptSubmitRequest, HermesRunner,
    HermesRuntimeEventKind, HermesRuntimePlugin, HermesSessionCreateRequest, StdioHermesGateway,
    HERMES_RUNTIME_PLUGIN_ID,
};
use awiki_deamon::runtime::{
    RuntimeLaunchContext, RuntimePlugin, RuntimeRun, RuntimeRunStatus, RuntimeTask,
};
use awiki_deamon::security::runtime_token::{issue_runtime_token, RpcMethod, RuntimeTokenScope};
use awiki_deamon::state::HermesProfileRecord;

fn hermes_record() -> HermesProfileRecord {
    HermesProfileRecord {
        agent_did: "did:agent:hermes".to_string(),
        runtime_profile_id: "profile_hermes_alice".to_string(),
        hermes_profile: "awiki_alice_hermes".to_string(),
        hermes_home: std::env::temp_dir().join("awiki-hermes-test-profile"),
        hermes_version: None,
        awiki_skills_version: "awiki-hermes-skills-v1".to_string(),
        status: "ready".to_string(),
    }
}

#[test]
fn hermes_gateway_fake_runner_session_prompt_events_are_deterministic() {
    let gateway = FakeHermesGateway::default();
    let runner = HermesRunner::start(gateway.clone(), &hermes_record()).unwrap();
    let session = runner
        .create_session(HermesSessionCreateRequest {
            route_key: "direct:did:human:alice".to_string(),
            conversation_id: Some("direct:did:human:alice".to_string()),
        })
        .unwrap();
    let outcome = runner
        .submit_prompt(
            &session,
            HermesPromptSubmitRequest {
                run_id: "run_msg_001".to_string(),
                message_id: "task_msg_001".to_string(),
                prompt: "处理这条 controller message".to_string(),
            },
        )
        .unwrap();

    assert_eq!(runner.runner_ref().runner_id, "fake:awiki_alice_hermes");
    assert_eq!(
        session.hermes_session_id,
        "fake-session-direct:did:human:alice"
    );
    assert_eq!(
        outcome
            .events
            .iter()
            .map(|event| event.kind.clone())
            .collect::<Vec<_>>(),
        vec![
            HermesRuntimeEventKind::PromptSubmitted,
            HermesRuntimeEventKind::MessageDelta,
            HermesRuntimeEventKind::MessageComplete,
        ]
    );
    assert_eq!(gateway.observed_events().len(), 5);
}

#[test]
fn hermes_gateway_plugin_launch_observes_complete_without_final_callback() {
    let gateway = FakeHermesGateway::with_behavior(FakeHermesBehavior::ObserveOnly);
    let plugin = HermesRuntimePlugin::new(gateway.clone(), hermes_record());
    let token = issue_runtime_token(
        RuntimeTokenScope::new(
            "did:agent:hermes",
            "profile_hermes_alice",
            "run_task_msg_001",
            vec![RpcMethod::TaskStatus, RpcMethod::TaskFinish],
            None,
            std::time::Duration::from_secs(60),
        )
        .unwrap(),
    )
    .unwrap();
    let outcome = plugin
        .launch_run(RuntimeLaunchContext {
            run: RuntimeRun {
                run_id: "run_task_msg_001".to_string(),
                task_id: "task_msg_001".to_string(),
                agent_did: "did:agent:hermes".to_string(),
                runtime_profile_id: "profile_hermes_alice".to_string(),
                runtime_plugin_id: HERMES_RUNTIME_PLUGIN_ID.to_string(),
                workspace_id: None,
                status: RuntimeRunStatus::Pending,
            },
            task: RuntimeTask {
                task_id: "task_msg_001".to_string(),
                agent_did: "did:agent:hermes".to_string(),
                controller_did: "did:human:alice".to_string(),
                sender_did: "did:human:alice".to_string(),
                conversation_id: Some("direct:did:human:alice".to_string()),
                text: "不要把 complete 直接当 final".to_string(),
            },
            workspace_root: None,
            workspace_instance: None,
            runtime_temp_dir: None,
            runtime_rpc_token: token.token,
            local_socket_path: None,
        })
        .unwrap();

    assert_eq!(plugin.plugin_id(), HERMES_RUNTIME_PLUGIN_ID);
    assert_eq!(outcome.status, RuntimeRunStatus::Running);
    assert!(outcome.callbacks.is_empty());
    assert!(gateway
        .observed_events()
        .iter()
        .any(|event| event.kind == HermesRuntimeEventKind::MessageComplete));
}

#[test]
fn hermes_gateway_plugin_rejects_mismatched_profile_binding() {
    let plugin = HermesRuntimePlugin::new(FakeHermesGateway::default(), hermes_record());
    let token = issue_runtime_token(
        RuntimeTokenScope::new(
            "did:agent:other",
            "profile_hermes_alice",
            "run_task_msg_001",
            vec![RpcMethod::TaskStatus],
            None,
            std::time::Duration::from_secs(60),
        )
        .unwrap(),
    )
    .unwrap();
    let error = plugin
        .launch_run(RuntimeLaunchContext {
            run: RuntimeRun {
                run_id: "run_task_msg_001".to_string(),
                task_id: "task_msg_001".to_string(),
                agent_did: "did:agent:other".to_string(),
                runtime_profile_id: "profile_hermes_alice".to_string(),
                runtime_plugin_id: HERMES_RUNTIME_PLUGIN_ID.to_string(),
                workspace_id: None,
                status: RuntimeRunStatus::Pending,
            },
            task: RuntimeTask {
                task_id: "task_msg_001".to_string(),
                agent_did: "did:agent:other".to_string(),
                controller_did: "did:human:alice".to_string(),
                sender_did: "did:human:alice".to_string(),
                conversation_id: None,
                text: "wrong binding".to_string(),
            },
            workspace_root: None,
            workspace_instance: None,
            runtime_temp_dir: None,
            runtime_rpc_token: token.token,
            local_socket_path: None,
        })
        .unwrap_err();

    assert!(error.to_string().contains("profile binding"));
}

#[test]
fn hermes_gateway_plugin_rejects_mismatched_task_context() {
    let plugin = HermesRuntimePlugin::new(FakeHermesGateway::default(), hermes_record());
    let token = issue_runtime_token(
        RuntimeTokenScope::new(
            "did:agent:hermes",
            "profile_hermes_alice",
            "run_task_msg_001",
            vec![RpcMethod::TaskStatus],
            None,
            std::time::Duration::from_secs(60),
        )
        .unwrap(),
    )
    .unwrap();
    let error = plugin
        .launch_run(RuntimeLaunchContext {
            run: RuntimeRun {
                run_id: "run_task_msg_001".to_string(),
                task_id: "task_msg_001".to_string(),
                agent_did: "did:agent:hermes".to_string(),
                runtime_profile_id: "profile_hermes_alice".to_string(),
                runtime_plugin_id: HERMES_RUNTIME_PLUGIN_ID.to_string(),
                workspace_id: None,
                status: RuntimeRunStatus::Pending,
            },
            task: RuntimeTask {
                task_id: "task_msg_other".to_string(),
                agent_did: "did:agent:hermes".to_string(),
                controller_did: "did:human:alice".to_string(),
                sender_did: "did:human:bob".to_string(),
                conversation_id: None,
                text: "wrong task binding".to_string(),
            },
            workspace_root: None,
            workspace_instance: None,
            runtime_temp_dir: None,
            runtime_rpc_token: token.token,
            local_socket_path: None,
        })
        .unwrap_err();

    assert!(error.to_string().contains("profile binding"));
}

#[test]
fn hermes_gateway_stdio_installation_reports_missing_env_without_failing_tests() {
    let gateway = StdioHermesGateway::from_env();
    let status = gateway.check_installation().unwrap();

    if std::env::var_os("AWIKI_HERMES_BIN").is_none() {
        assert!(!status.installed);
        assert_eq!(
            status.detail.as_deref(),
            Some("AWIKI_HERMES_BIN is not set")
        );
    }
}

#[test]
fn hermes_gateway_prompt_debug_redacts_prompt_text() {
    let request = HermesPromptSubmitRequest {
        run_id: "run_debug".to_string(),
        message_id: "task_debug".to_string(),
        prompt: "secret rtok_debug_secret_value_123456789 private key jwt".to_string(),
    };
    let debug = format!("{request:?}");

    assert!(debug.contains("<redacted>"));
    assert!(!debug.contains("rtok_debug_secret_value_123456789"));
    assert!(!debug.contains("private key"));
    assert!(!debug.contains("jwt"));
}

#[test]
#[ignore]
fn hermes_real_smoke_installation_check_uses_awiki_hermes_bin() {
    let Some(bin) = std::env::var_os("AWIKI_HERMES_BIN") else {
        eprintln!("skipped: AWIKI_HERMES_BIN is not set");
        return;
    };
    let gateway = StdioHermesGateway::new(bin);
    let status = gateway.check_installation().unwrap();

    assert!(status.installed, "Hermes binary was not found: {status:?}");
    eprintln!("Hermes smoke installation detail: {:?}", status.detail);
}
