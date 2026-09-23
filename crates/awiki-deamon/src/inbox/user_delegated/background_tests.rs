use super::*;
use crate::{
    acp::store::{self, Session, Work},
    outbox::MemoryRuntimeOutbox,
    security::runtime_token::RpcMethod,
};

fn acp_fixture() -> (TestFixture, DaemonConfig, RuntimeTask, UserMessageEnvelope) {
    use std::os::unix::fs::PermissionsExt;
    let f = fixture();
    let config = DaemonConfig::for_state_root(f._root.path()).unwrap();
    let binary = f._root.path().join("hermes-fixture.py");
    std::fs::write(
        &binary,
        include_str!("../../../tests/fixtures/acp_agent.py"),
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    let source = f._root.path().join("source-hermes");
    std::fs::create_dir(&source).unwrap();
    let mut cli = f
        .state
        .load_cli_runtime_profile(&f.binding.runtime_profile_id)
        .unwrap();
    cli.binary_path = Some(binary);
    cli.config_home = Some(source);
    f.state.upsert_cli_runtime_profile(&cli).unwrap();
    let mut profile = f
        .state
        .load_runtime_agent_profile(&f.binding.runtime_agent_did)
        .unwrap();
    profile.workspace_id = Some("background-workspace".into());
    profile.workspace_root = Some(f._root.path().join("workspace"));
    profile.workspace_mode = Some(crate::workspace::WorkspaceMode::SharedRoot);
    f.state.upsert_runtime_agent_profile(&profile).unwrap();
    let envelope = user_message_envelope(
        &f.binding,
        &plain_message("msg-acp", "did:human:bob", "summarize"),
        plain_dispatch("summarize"),
    )
    .unwrap();
    let task = runtime_task_from_envelope(&f.state, &f.binding, &envelope).unwrap();
    (f, config, task, envelope)
}

#[test]
fn background_dispatch_retry_uses_one_model_run_and_only_owner_sync() {
    let (f, config, task, envelope) = acp_fixture();
    let dispatcher = RuntimeHostMessageDispatcher::new(&config, &f.state);
    for _ in 0..2 {
        dispatcher
            .dispatch_user_message(&f.binding, task.clone(), &envelope)
            .unwrap();
    }
    let run = f
        .state
        .load_runtime_run(&format!("run_{}", task.task_id))
        .unwrap();
    assert_eq!(run.status, RuntimeRunStatus::Finished);
    let key = Session::new(&task).key;
    let prompts = std::fs::read_to_string(
        f._root
            .path()
            .join("workspace/acp")
            .join(&key)
            .join("prompts.jsonl"),
    )
    .unwrap();
    assert_eq!(prompts.lines().count(), 1);
    assert!(prompts.contains("noninteractive"));
    let sync = f
        .state
        .load_message_sync_outbox(&format!(
            "message-sync:{}:runtime-final:{}",
            f.binding.user_did, run.run_id
        ))
        .unwrap()
        .unwrap();
    assert_eq!(sync.owner_did, f.binding.user_did);
    assert_eq!(sync.payload_json["source_message_id"], "msg-acp");
    let count: i64 = f
        .state
        .connection()
        .unwrap()
        .query_row("SELECT count(*) FROM acp_events", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
    assert!(
        crate::acp::task_records::load(&f.state.connection().unwrap(), &run.run_id)
            .unwrap()
            .unwrap()
            .background
    );
}

#[test]
fn busy_background_message_stays_in_source_queue_without_claiming_a_run() {
    let (f, config, task, envelope) = acp_fixture();
    let session = Session::new(&task);
    let key = session.key.clone();
    let mut other = task.clone();
    other.task_id = "another-message".into();
    store::mutate(&f.state, &key, Some(session), |s| {
        s.submit(Work {
            task: other,
            run_id: "occupied".into(),
        })
    })
    .unwrap();
    let dispatcher = RuntimeHostMessageDispatcher::new(&config, &f.state);
    assert_eq!(
        dispatcher
            .dispatch_user_message(&f.binding, task.clone(), &envelope)
            .unwrap_err()
            .to_string(),
        "background_busy"
    );
    assert!(f
        .state
        .load_runtime_run(&format!("run_{}", task.task_id))
        .is_err());
    assert!(store::load(&f.state, &key).unwrap().waiting.is_none());
    store::mutate(&f.state, &key, None, |s| s.complete("occupied", "finished")).unwrap();
    dispatcher
        .dispatch_user_message(&f.binding, task.clone(), &envelope)
        .unwrap();
    assert_eq!(
        f.state
            .load_runtime_run(&format!("run_{}", task.task_id))
            .unwrap()
            .status,
        RuntimeRunStatus::Finished
    );
}

#[test]
fn failed_background_source_retry_does_not_call_the_model_again() {
    let (f, config, mut task, envelope) = acp_fixture();
    task.text = task.text.replace("summarize", "QUESTION_NATIVE");
    let dispatcher = RuntimeHostMessageDispatcher::new(&config, &f.state);
    for _ in 0..2 {
        dispatcher
            .dispatch_user_message(&f.binding, task.clone(), &envelope)
            .unwrap();
    }
    let run = f
        .state
        .load_runtime_run(&format!("run_{}", task.task_id))
        .unwrap();
    assert_eq!(run.status, RuntimeRunStatus::Failed);
    let prompts = std::fs::read_to_string(
        f._root
            .path()
            .join("workspace/acp")
            .join(Session::new(&task).key)
            .join("prompts.jsonl"),
    )
    .unwrap();
    assert_eq!(prompts.lines().count(), 1);
    let sync = f
        .state
        .load_message_sync_outbox(&format!(
            "message-sync:{}:runtime-status:{}:failed",
            f.binding.user_did, run.run_id
        ))
        .unwrap()
        .unwrap();
    assert_eq!(
        sync.payload_json["last_error_code"],
        "personal_agent_manual_handling_required"
    );
}

#[test]
fn global_final_flush_respects_background_audience_and_binding_revocation() {
    for revoke in [false, true] {
        let (f, _, _, _) = acp_fixture();
        insert_delegated_runtime_task_and_run(
            &f.state,
            &f.binding,
            "task-final",
            "run-final",
            "source-final",
        );
        let profile = f
            .state
            .load_runtime_agent_profile(&f.binding.runtime_agent_did)
            .unwrap();
        let run = f.state.load_runtime_run("run-final").unwrap();
        let record = crate::runtime::host::runtime_final_outbox_record(
            &profile,
            &f.binding.user_did,
            &f.binding.user_did,
            &run,
            Some("direct:did:human:bob"),
            "private summary",
            "acp",
        )
        .unwrap();
        f.state
            .upsert_runtime_final_outbox_pending(&record)
            .unwrap();
        if revoke {
            f.state
                .update_app_personal_agent_binding_status_by_runtime(
                    &f.binding.runtime_agent_did,
                    "personal_agent_disabled",
                    true,
                )
                .unwrap();
        }
        let public_outbox = MemoryRuntimeOutbox::default();
        let delivered =
            crate::runtime::host::flush_runtime_final_outbox(&f.state, &public_outbox, 20).unwrap();
        assert_eq!(delivered, usize::from(!revoke));
        assert!(public_outbox.records().is_empty());
    }
}

#[test]
fn only_active_hermes_background_tasks_receive_app_action_authority() {
    let (f, _, task, _) = acp_fixture();
    let profile = f
        .state
        .load_runtime_agent_profile(&f.binding.runtime_agent_did)
        .unwrap();
    let issued =
        crate::runtime::host::issue_acp_runtime_token(&f.state, &profile, &task, "run-token")
            .unwrap();
    assert_eq!(
        issued.scope.allowed_methods,
        vec![RpcMethod::RpcPing, RpcMethod::AppActionRequest]
    );
    let mut ordinary = task.clone();
    ordinary.trigger_kind = RuntimeTaskTriggerKind::ExternalDirect;
    let ordinary_token =
        crate::runtime::host::issue_acp_runtime_token(&f.state, &profile, &ordinary, "ordinary")
            .unwrap();
    assert!(!ordinary_token
        .scope
        .allowed_methods
        .contains(&RpcMethod::AppActionRequest));
    let mut wrong = task.clone();
    wrong.reply_recipient_did = "did:human:mallory".into();
    assert!(
        crate::runtime::host::issue_acp_runtime_token(&f.state, &profile, &wrong, "wrong").is_err()
    );
    f.state
        .update_app_personal_agent_binding_status_by_runtime(
            &f.binding.runtime_agent_did,
            "personal_agent_disabled",
            true,
        )
        .unwrap();
    assert!(
        crate::runtime::host::issue_acp_runtime_token(&f.state, &profile, &task, "revoked")
            .is_err()
    );
}
