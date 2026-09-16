use super::*;
use crate::{
    runtime::{RuntimeConversationScope, RuntimeInvocationAuthority, RuntimeTaskTriggerKind},
    DaemonConfig,
};

fn work(id: &str, group: bool) -> Work {
    Work {
        run_id: format!("run_{id}"),
        task: RuntimeTask {
            task_id: format!("task_{id}"),
            agent_did: "did:agent:coder".into(),
            agent_handle: "coder".into(),
            controller_user_id: "user-alice".into(),
            controller_full_handle: "alice.awiki.info".into(),
            controller_scope_key: "controller-scope:v1:alice".into(),
            controller_did: "did:human:alice".into(),
            sender_did: "did:human:alice".into(),
            requester_did: "did:human:alice".into(),
            requester_user_id: Some("user-alice".into()),
            requester_full_handle: Some("alice.awiki.info".into()),
            trigger_kind: if group {
                RuntimeTaskTriggerKind::GroupMention
            } else {
                RuntimeTaskTriggerKind::ControllerDirect
            },
            conversation_scope: if group {
                RuntimeConversationScope::GroupVisible {
                    group_key: "group:1".into(),
                }
            } else {
                RuntimeConversationScope::ControllerPrivate {
                    controller_scope_key: "controller-scope:v1:alice".into(),
                }
            },
            invocation_authority: RuntimeInvocationAuthority::Controller,
            reply_recipient_did: "did:human:alice".into(),
            conversation_id: Some(
                if group {
                    "conversation:group"
                } else {
                    "conversation:private"
                }
                .into(),
            ),
            text: id.into(),
        },
    }
}
fn fixture() -> (tempfile::TempDir, DaemonState, Session) {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open_with_root_key_bytes(&config, [7; 32]);
    state.initialize().unwrap();
    let task = work("a", false).task;
    state
        .upsert_runtime_agent_profile(&crate::runtime::RuntimeAgentProfile {
            agent_did: task.agent_did.clone(),
            agent_handle: task.agent_handle.clone(),
            controller_user_id: task.controller_user_id.clone(),
            controller_full_handle: task.controller_full_handle.clone(),
            controller_scope_key: task.controller_scope_key.clone(),
            controller_did: task.controller_did.clone(),
            runtime_profile_id: "profile-acp".into(),
            runtime_plugin_id: "acp".into(),
            display_name: None,
            preferred_language: "zh-Hans".into(),
            workspace_id: Some("acp-work".into()),
            workspace_root: Some(root.path().join("work")),
            workspace_mode: Some(crate::workspace::WorkspaceMode::SharedRoot),
        })
        .unwrap();
    let s = Session::new(&work("a", false).task);
    mutate(&state, &s.key, Some(s.clone()), |s| {
        s.submit(work("a", false))
    })
    .unwrap();
    (root, state, s)
}

#[test]
fn only_one_waiting_slot_survives_concurrent_submissions() {
    let (_root, state, s) = fixture();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(9));
    let threads = (0..8)
        .map(|n| {
            let state = state.clone();
            let key = s.key.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                mutate(&state, &key, None, |s| {
                    s.submit(work(&n.to_string(), false))
                })
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();
    assert_eq!(
        threads
            .into_iter()
            .filter(|t| t.thread().id() != std::thread::current().id())
            .map(|t| t.join().unwrap())
            .filter(Result::is_ok)
            .count(),
        1
    );
    let s = load(&state, &s.key).unwrap();
    assert_eq!(s.active.unwrap().run_id, "run_a");
    assert!(s.waiting.is_some());
}

#[test]
fn normal_completion_runs_waiting_but_stop_and_failure_pause_it() {
    for outcome in ["finished", "failed", "cancelled"] {
        let mut s = Session::new(&work("a", false).task);
        s.submit(work("a", false)).unwrap();
        s.submit(work("b", false)).unwrap();
        let next = s.complete("run_a", outcome).unwrap();
        assert_eq!(next.is_some(), outcome == "finished");
        assert_eq!(s.waiting_paused, outcome != "finished");
    }
    let mut s = Session::new(&work("a", false).task);
    s.submit(work("a", false)).unwrap();
    s.submit(work("b", false)).unwrap();
    s.command("stop", &json!({"run_id":"run_a"}), "did:human:alice", 0)
        .unwrap();
    assert!(s.complete("run_a", "finished").unwrap().is_none());
    assert!(s.waiting_paused);
}

#[test]
fn execute_waiting_waits_for_confirmed_stop_and_rejects_late_completion() {
    let mut s = Session::new(&work("a", false).task);
    s.submit(work("a", false)).unwrap();
    s.submit(work("b", false)).unwrap();
    assert!(s
        .command(
            "execute_waiting",
            &json!({"run_id":"run_b"}),
            "did:human:alice",
            0
        )
        .unwrap()
        .is_none());
    assert!(s.active_run("run_a"));
    assert!(s.stopping);
    assert_eq!(
        s.complete("run_a", "cancelled").unwrap().unwrap().run_id,
        "run_b"
    );
    assert!(s.complete("run_a", "finished").is_err());
    assert!(s.active_run("run_b"));
}

#[test]
fn execute_waiting_also_proceeds_if_active_finished_before_cancel_arrived() {
    let mut s = Session::new(&work("a", false).task);
    s.submit(work("a", false)).unwrap();
    s.submit(work("b", false)).unwrap();
    s.command(
        "execute_waiting",
        &json!({"run_id":"run_b"}),
        "did:human:alice",
        0,
    )
    .unwrap();
    assert_eq!(
        s.complete("run_a", "finished").unwrap().unwrap().run_id,
        "run_b"
    );
}

#[test]
fn group_rejects_competition_and_private_controls() {
    let mut s = Session::new(&work("a", true).task);
    s.submit(work("a", true)).unwrap();
    assert_eq!(
        s.submit(work("b", true)).unwrap_err().to_string(),
        "group_busy"
    );
    assert!(s
        .command("stop", &json!({"run_id":"run_a"}), "did:human:alice", 0)
        .is_err());
    assert!(s.waiting.is_none());
}

#[test]
fn restart_preserves_paused_waiting_and_native_context_without_replaying_a() {
    let (_root, state, s) = fixture();
    mutate(&state, &s.key, None, |s| {
        s.native_session_id = Some("native-original".into());
        s.submit(work("b", false))
    })
    .unwrap();
    recover(&state).unwrap();
    let recovered = load(&state, &s.key).unwrap();
    assert!(recovered.active.is_none());
    assert!(recovered.waiting_paused);
    assert_eq!(recovered.waiting.unwrap().run_id, "run_b");
    assert_eq!(
        recovered.native_session_id.as_deref(),
        Some("native-original")
    );
    assert_eq!(recovered.last_task["state"], "interrupted");
}

#[test]
fn command_retry_is_idempotent_and_stream_revisions_do_not_invalidate_stop() {
    let (_root, state, s) = fixture();
    let args = json!({"action":"stop","session_key":s.key,"revision":1,"run_id":"run_a"});
    mutate(&state, &s.key, None, |s| {
        s.text.push_str("stream");
        Ok(())
    })
    .unwrap();
    let first = control(&state, &s.agent_did, "did:human:alice", "cmd-1", &args).unwrap();
    let again = control(&state, &s.agent_did, "did:human:alice", "cmd-1", &args).unwrap();
    assert_eq!(first.0, again.0);
    assert!(again.1.is_none());
    let mut other = args.clone();
    other["run_id"] = json!("run_b");
    assert_eq!(
        control(&state, &s.agent_did, "did:human:alice", "cmd-1", &other)
            .unwrap_err()
            .to_string(),
        "command_id_conflict"
    );
}

#[test]
fn questions_require_real_valid_answers_from_requester_once_before_expiry() {
    let mut s = Session::new(&work("a", true).task);
    s.submit(work("a", true)).unwrap();
    let request = json!({"mode":"form","requestedSchema":{"type":"object","required":["choice"],"properties":{"choice":{"type":"string","oneOf":[{"const":"yes","title":"Yes"}]}}}});
    s.questions.push(Question {
        id: "q1".into(),
        run_id: "run_a".into(),
        expires_at_ms: 100,
        request,
        response: None,
    });
    let mut args = json!({"run_id":"run_a","question_id":"q1","response":{"action":"accept","content":{"choice":"no"}}});
    assert!(s.command("answer", &args, "did:human:alice", 1).is_err());
    args["response"]["content"]["choice"] = json!("yes");
    assert!(s.command("answer", &args, "did:human:bob", 1).is_err());
    assert!(s.command("answer", &args, "did:human:alice", 100).is_err());
    s.command("answer", &args, "did:human:alice", 99).unwrap();
    assert!(s.command("answer", &args, "did:human:alice", 99).is_err());
}

#[test]
fn answer_constraints_and_unknown_question_types_fail_closed() {
    let q = json!({"requestedSchema":{"type":"object","properties":{"n":{"type":"integer","minimum":2},"s":{"type":"string","pattern":"^[A-Z]+$"},"many":{"type":"array","minItems":1,"maxItems":2,"items":{"anyOf":[{"const":"a","title":"A"},{"const":"b","title":"B"}]}}}}});
    for content in [
        json!({"n":1}),
        json!({"s":"lower"}),
        json!({"many":[]}),
        json!({"many":["a","a"]}),
        json!({"many":["x"]}),
    ] {
        assert!(validate_answer(&q, &json!({"action":"accept","content":content})).is_err());
    }
    assert!(validate_answer(
        &q,
        &json!({"action":"accept","content":{"s":"OK","many":["a"]}})
    )
    .is_ok());
    assert!(super::super::questions::validate_schema(&json!({"mode":"url"})).is_err());
}

#[test]
fn model_selection_is_idle_only_and_never_accepts_a_mode_as_model() {
    let mut s = Session::new(&work("a", false).task);
    s.options = json!([{"id":"mode","options":[{"value":"danger"}]},{"id":"model","options":[{"value":"deepseek-flash","name":"Flash"}]}]);
    assert!(s
        .command(
            "set_model",
            &json!({"model_id":"danger"}),
            "did:human:alice",
            0
        )
        .is_err());
    s.command(
        "set_model",
        &json!({"model_id":"deepseek-flash"}),
        "did:human:alice",
        0,
    )
    .unwrap();
    s.submit(work("a", false)).unwrap();
    assert!(s
        .command(
            "set_model",
            &json!({"model_id":"deepseek-flash"}),
            "did:human:alice",
            0
        )
        .is_err());
}

#[test]
fn scopes_and_snapshots_do_not_leak_private_task_or_native_identifier() {
    let mut s = Session::new(&work("secret", false).task);
    s.submit(work("secret", false)).unwrap();
    s.native_session_id = Some("native-secret".into());
    let public = s.snapshot().to_string();
    assert!(!public.contains("native-secret"));
    assert!(!public.contains("\"text\":\"secret\""));
    assert_ne!(
        s.key,
        session_key(&s.agent_did, "another-controller", &s.scope)
    );
    assert_ne!(s.key, Session::new(&work("a", true).task).key);
}

#[test]
fn native_session_scope_survives_transport_alias_change_but_rejects_other_personas() {
    let mut first = work("a", false);
    let mut session = Session::new(&first.task);
    session.submit(first.clone()).unwrap();
    session.complete(&first.run_id, "finished").unwrap();
    first.run_id = "rotated".into();
    first.task.conversation_id = Some("direct:did:human:alice-new-device".into());
    assert!(session.submit(first.clone()).unwrap());
    session.complete(&first.run_id, "finished").unwrap();
    first.task.conversation_scope = RuntimeConversationScope::Direct {
        requester_user_id: "another-user".into(),
        requester_full_handle: "another.awiki.info".into(),
    };
    assert_eq!(
        session.submit(first).unwrap_err().to_string(),
        "conversation_mismatch"
    );
    assert!(session.active.is_none());
}

#[test]
fn final_completion_and_stop_have_one_atomic_winner() {
    for order in 0..18 {
        let (_root, state, s) = fixture();
        mutate(&state, &s.key, None, |s| s.submit(work("b", false))).unwrap();
        let task = work("a", false).task;
        let profile = state.load_runtime_agent_profile(&s.agent_did).unwrap();
        let run = crate::runtime::RuntimeRun {
            run_id: "run_a".into(),
            task_id: task.task_id.clone(),
            agent_did: task.agent_did.clone(),
            runtime_profile_id: profile.runtime_profile_id.clone(),
            runtime_plugin_id: "acp".into(),
            workspace_id: profile.workspace_id.clone(),
            status: crate::runtime::RuntimeRunStatus::Running,
        };
        let record = crate::runtime::host::runtime_final_outbox_record(
            &profile,
            &task.controller_did,
            &task.reply_recipient_did,
            &run,
            task.conversation_id.as_deref(),
            "late final",
            "acp",
        )
        .unwrap();
        let stop = || {
            control(
                &state,
                &s.agent_did,
                &task.requester_did,
                "stop_a",
                &json!({
                    "session_key":s.key,"run_id":"run_a","action":"stop","revision":1
                }),
            )
        };
        let finish = || finish_with_final(&state, &s.key, &record);
        let (stopped, completed) = if order == 0 {
            (stop(), finish())
        } else if order == 1 {
            let completed = finish();
            (stop(), completed)
        } else {
            let barrier = std::sync::Barrier::new(2);
            std::thread::scope(|threads| {
                let stop_thread = threads.spawn(|| {
                    barrier.wait();
                    stop()
                });
                barrier.wait();
                let completed = finish();
                (stop_thread.join().unwrap(), completed)
            })
        };
        let (cancelled, next) = completed.unwrap();
        let session = load(&state, &s.key).unwrap();
        let outbox = state.load_runtime_final_outbox_by_run("run_a").unwrap();
        assert_eq!(cancelled, stopped.is_ok());
        assert_eq!(outbox.is_none(), cancelled);
        assert_eq!(next.is_none(), cancelled);
        if cancelled {
            assert_eq!(session.last_task["state"], "cancelled");
            assert!(session.waiting_paused);
            assert!(session.active.is_none());
        } else {
            assert_eq!(stopped.unwrap_err().to_string(), "stale_task");
            assert!(session.active_run("run_b"));
            assert_eq!(session.last_task["state"], "finished");
        }
        assert!(
            finish().is_err(),
            "duplicate completion must not deliver again"
        );
    }
}
