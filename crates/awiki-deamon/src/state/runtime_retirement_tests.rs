use super::*;
use crate::{
    runtime::{RuntimeAgentProfile, RuntimeRun, RuntimeRunStatus},
    DaemonConfig,
};

fn profile(id: &str, plugin: &str) -> RuntimeAgentProfile {
    RuntimeAgentProfile {
        agent_did: format!("did:agent:{id}"),
        agent_handle: id.into(),
        controller_user_id: "alice".into(),
        controller_full_handle: "alice.example.com".into(),
        controller_scope_key: "controller-scope:v1:alice:alice.example.com".into(),
        controller_did: "did:human:alice".into(),
        runtime_profile_id: format!("profile-{id}"),
        runtime_plugin_id: plugin.into(),
        display_name: Some(id.into()),
        preferred_language: "zh-Hans".into(),
        workspace_id: None,
        workspace_root: None,
        workspace_mode: None,
    }
}

#[test]
fn retirement_is_durable_one_way_and_does_not_change_acp_agents() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open_with_root_key_bytes(&config, [12; 32]);
    state.initialize().unwrap();
    for (id, plugin) in [
        ("hermes-old", "runtime.hermes"),
        ("codex-old", "generic-cli"),
        ("claude-old", "runtime.cli.claude-code"),
        ("opencode", "acp"),
        ("gemini", "acp"),
        ("kimi", "acp"),
        ("deepseek-harness", "acp"),
    ] {
        state
            .upsert_runtime_agent_profile(&profile(id, plugin))
            .unwrap();
    }
    for _ in 0..2 {
        state.initialize().unwrap();
    }
    for id in ["hermes-old", "codex-old", "claude-old"] {
        let did = format!("did:agent:{id}");
        assert_eq!(state.load_agent_definition(&did).unwrap().status, "retired");
        assert_eq!(
            state.runtime_retirement_reason(&did).unwrap().as_deref(),
            Some(LEGACY_RUNTIME_DISABLED)
        );
        let old = state.load_runtime_agent_profile(&did).unwrap();
        assert!(state.upsert_runtime_agent_profile(&old).is_err());
        let mut disguised = old;
        disguised.runtime_plugin_id = "acp".into();
        assert!(state.upsert_runtime_agent_profile(&disguised).is_err());
        let mut definition = state.load_agent_definition(&did).unwrap();
        definition.status = "active".into();
        assert!(state.upsert_agent_definition(&definition).is_err());
    }
    let active = state.list_runtime_agent_definitions().unwrap();
    assert_eq!(active.len(), 4);
    assert!(active
        .iter()
        .all(|a| a.runtime_plugin_id.as_deref() == Some("acp")));
    assert_eq!(
        state
            .connection()
            .unwrap()
            .query_row("SELECT count(*) FROM runtime_retirement", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        3
    );
}

#[test]
fn retirement_fences_pending_execution_tokens_and_final_without_erasing_content() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open_with_root_key_bytes(&config, [13; 32]);
    state.initialize().unwrap();
    let profile = profile("old", "runtime.hermes");
    state.upsert_runtime_agent_profile(&profile).unwrap();
    let run = RuntimeRun {
        run_id: "run-old".into(),
        task_id: "task-old".into(),
        agent_did: profile.agent_did.clone(),
        runtime_profile_id: profile.runtime_profile_id.clone(),
        runtime_plugin_id: profile.runtime_plugin_id.clone(),
        workspace_id: None,
        status: RuntimeRunStatus::Running,
    };
    state.insert_runtime_run(&run).unwrap();
    let record = crate::runtime::host::runtime_final_outbox_record(
        &profile,
        &profile.controller_did,
        &profile.controller_did,
        &run,
        Some("direct:old"),
        "old final body",
        "legacy",
    )
    .unwrap();
    state.upsert_runtime_final_outbox_pending(&record).unwrap();
    let issued = crate::security::runtime_token::issue_runtime_token(
        crate::security::runtime_token::RuntimeTokenScope::new(
            profile.agent_did.clone(),
            profile.runtime_profile_id.clone(),
            run.run_id.clone(),
            vec![crate::security::runtime_token::RpcMethod::RpcPing],
            None,
            std::time::Duration::from_secs(60),
        )
        .unwrap(),
    )
    .unwrap();
    state.store_runtime_token(&issued).unwrap();
    state.initialize().unwrap();
    assert_eq!(
        state.load_runtime_run(&run.run_id).unwrap().status,
        RuntimeRunStatus::Failed
    );
    assert!(state
        .insert_runtime_retry_request(
            &state.load_runtime_run(&run.run_id).unwrap(),
            "retry-command"
        )
        .is_err());
    let db = state.connection().unwrap();
    let (status, text, reason): (String,String,String) = db.query_row("SELECT status,final_text,last_error_code FROM runtime_final_outbox WHERE idempotency_key=?1", [&record.idempotency_key], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
    assert_eq!(
        (status.as_str(), text.as_str(), reason.as_str()),
        ("failed_terminal", "old final body", LEGACY_RUNTIME_DISABLED)
    );
    let revoked: bool = db
        .query_row(
            "SELECT revoked_at_ms IS NOT NULL FROM runtime_rpc_tokens WHERE token_id=?1",
            [&issued.token_id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(revoked);
    assert!(state
        .list_due_runtime_final_outbox(i64::MAX, 10)
        .unwrap()
        .is_empty());
}
