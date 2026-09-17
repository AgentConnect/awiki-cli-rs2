use super::*;
#[path = "client_tests/gemini.rs"]
mod gemini;
#[path = "client_tests/models.rs"]
mod models;
#[path = "client_tests/real_questions.rs"]
mod real_questions;
use crate::{
    acp::store::{Session, Work},
    runtime::{
        RuntimeAgentProfile, RuntimeConversationScope, RuntimeInvocationAuthority, RuntimeTask,
        RuntimeTaskTriggerKind,
    },
    DaemonConfig,
};

struct Fixture {
    root: tempfile::TempDir,
    state: DaemonState,
    profile: CliRuntimeProfileRecord,
    key: String,
    task: RuntimeTask,
}
impl Fixture {
    fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        let state = DaemonState::open_with_root_key_bytes(&config, [9; 32]);
        state.initialize().unwrap();
        std::fs::create_dir_all(root.path().join("work")).unwrap();
        let executable = root.path().join("agent.py");
        std::fs::write(
            &executable,
            include_str!("../../tests/fixtures/acp_agent.py"),
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut profile = CliRuntimeProfileRecord::for_driver("profile-acp", "kimi").unwrap();
        profile.binary_path = Some(executable);
        let task = RuntimeTask {
            task_id: "task_a".into(),
            agent_did: "did:agent:fixture".into(),
            agent_handle: "fixture".into(),
            controller_user_id: "user-alice".into(),
            controller_full_handle: "alice.awiki.info".into(),
            controller_scope_key: "controller-scope:v1:alice".into(),
            controller_did: "did:human:alice".into(),
            sender_did: "did:human:alice".into(),
            requester_did: "did:human:alice".into(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: RuntimeTaskTriggerKind::ControllerDirect,
            conversation_scope: RuntimeConversationScope::ControllerPrivate {
                controller_scope_key: "controller-scope:v1:alice".into(),
            },
            invocation_authority: RuntimeInvocationAuthority::Controller,
            reply_recipient_did: "did:human:alice".into(),
            conversation_id: Some("direct:fixture".into()),
            text: "fixture".into(),
        };
        state
            .upsert_runtime_agent_profile(&RuntimeAgentProfile {
                agent_did: task.agent_did.clone(),
                agent_handle: task.agent_handle.clone(),
                controller_user_id: task.controller_user_id.clone(),
                controller_full_handle: task.controller_full_handle.clone(),
                controller_scope_key: task.controller_scope_key.clone(),
                controller_did: task.controller_did.clone(),
                runtime_profile_id: profile.runtime_profile_id.clone(),
                runtime_plugin_id: "acp".into(),
                display_name: None,
                preferred_language: "en".into(),
                workspace_id: None,
                workspace_root: None,
                workspace_mode: None,
            })
            .unwrap();
        let session = Session::new(&task);
        let key = session.key.clone();
        store::mutate(&state, &key, Some(session), |s| {
            s.submit(Work {
                task: task.clone(),
                run_id: "run_a".into(),
            })
        })
        .unwrap();
        Self {
            root,
            state,
            profile,
            key,
            task,
        }
    }
    fn turn(&self, text: &str) -> Turn {
        Turn {
            state: self.state.clone(),
            key: self.key.clone(),
            run_id: store::load(&self.state, &self.key)
                .unwrap()
                .active
                .unwrap()
                .run_id,
            profile: self.profile.clone(),
            cwd: self.root.path().join("work"),
            prompt: vec![ContentBlock::Text(TextContent::new(text))],
            environment: vec![],
        }
    }
    fn next(&self) {
        store::mutate(&self.state, &self.key, None, |s| {
            let previous = s.active.as_ref().unwrap().run_id.clone();
            s.complete(&previous, "finished")?;
            let next = format!("run_{}", rand::random::<u128>());
            let mut task = self.task.clone();
            task.task_id = format!("task_{next}");
            s.submit(Work { task, run_id: next })
        })
        .unwrap();
    }
}

#[tokio::test]
async fn native_resume_uses_exact_id_and_drops_replay_notifications() {
    let f = Fixture::new();
    assert_eq!(run(f.turn("hello")).await.unwrap().text, "FIXTURE_RESPONSE");
    f.next();
    assert_eq!(run(f.turn("again")).await.unwrap().text, "FIXTURE_RESPONSE");
    let log = std::fs::read_to_string(f.root.path().join("work/protocol.jsonl")).unwrap();
    assert!(log.contains("session/resume"));
    assert_eq!(log.matches("session/new").count(), 1);
}

#[test]
fn gemini_replay_hook_waits_for_history_and_preserves_other_versions() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let package = root.path().join("@google/gemini-cli");
    let bundle = package.join("bundle");
    std::fs::create_dir_all(&bundle).unwrap();
    let module = bundle.join("gemini-fixture.js");
    let source = r#"
const events = [];
const session = { async streamHistory() {
  await new Promise(resolve => setTimeout(resolve, 30));
  events.push('history');
}};
const sessionData = {messages: []};
const manager = { async loadSession() {
    session.streamHistory(sessionData.messages);
  events.push('response');
}};
await manager.loadSession();
await new Promise(resolve => setTimeout(resolve, 60));
process.stdout.write(events.join(','));
"#;
    std::fs::write(&module, source).unwrap();
    let (launch, guard) = configure_gemini_replay(
        Brand::Gemini,
        AcpAgentConfig::new("node").env("NODE_OPTIONS", "--no-warnings"),
    )
    .unwrap();
    let hook_path = guard.as_ref().unwrap().path().to_path_buf();
    assert_eq!(
        std::fs::metadata(&hook_path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(launch.environment()["NODE_OPTIONS"].starts_with("--no-warnings "));
    for (version, expected) in [
        ("0.59.0", "history,response"),
        ("0.60.0", "history,response"),
        ("0.61.0", "response,history"),
    ] {
        std::fs::write(
            package.join("package.json"),
            json!({"name":"@google/gemini-cli","version":version,"type":"module"}).to_string(),
        )
        .unwrap();
        let output = std::process::Command::new("node")
            .arg(&module)
            .envs(launch.environment())
            .output()
            .expect("Node is required for Gemini compatibility tests");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
        assert_eq!(std::fs::read_to_string(&module).unwrap(), source);
    }
    std::fs::write(
        package.join("package.json"),
        json!({"name":"@google/gemini-cli","version":"0.59.0","type":"module"}).to_string(),
    )
    .unwrap();
    std::fs::write(
        &module,
        source.replace(
            "session.streamHistory(sessionData.messages);",
            "session.streamHistory([]);",
        ),
    )
    .unwrap();
    let mismatch = std::process::Command::new("node")
        .arg(&module)
        .envs(launch.environment())
        .output()
        .unwrap();
    assert!(!mismatch.status.success());
    assert!(String::from_utf8_lossy(&mismatch.stderr)
        .contains("awiki_gemini_replay_compatibility_mismatch"));
    drop(guard);
    assert!(!hook_path.exists());
    for brand in [Brand::OpenCode, Brand::Kimi, Brand::DeepseekHarness] {
        let (launch, guard) =
            configure_gemini_replay(brand, AcpAgentConfig::new("fixture")).unwrap();
        assert!(guard.is_none());
        assert!(!launch.environment().contains_key("NODE_OPTIONS"));
    }
}

#[tokio::test]
async fn missing_native_context_never_creates_replacement_without_confirmation() {
    let f = Fixture::new();
    run(f.turn("hello")).await.unwrap();
    f.next();
    std::fs::write(f.root.path().join("work/missing"), "").unwrap();
    assert!(run(f.turn("again")).await.is_err());
    assert!(store::load(&f.state, &f.key).unwrap().context_lost);
    let log = std::fs::read_to_string(f.root.path().join("work/protocol.jsonl")).unwrap();
    assert_eq!(log.matches("session/new").count(), 1);
    store::mutate(&f.state, &f.key, None, |s| {
        let run = s.active.as_ref().unwrap().run_id.clone();
        s.complete(&run, "failed")
    })
    .unwrap();
    let before = store::load(&f.state, &f.key).unwrap();
    let reset = json!({"action":"reset_context","session_key":f.key,"revision":before.revision,"confirmed":false});
    assert!(store::control(
        &f.state,
        &f.task.agent_did,
        &f.task.requester_did,
        "unconfirmed-reset",
        &reset,
    )
    .is_err());
    assert!(store::load(&f.state, &f.key).unwrap().context_lost);
    let mut reset = reset;
    reset["confirmed"] = json!(true);
    store::control(
        &f.state,
        &f.task.agent_did,
        &f.task.requester_did,
        "confirmed-reset",
        &reset,
    )
    .unwrap();
    let reset = store::load(&f.state, &f.key).unwrap();
    assert!(!reset.context_lost);
    assert!(reset.native_session_id.is_none());
    assert_eq!(reset.history, before.history);
    std::fs::remove_file(f.root.path().join("work/missing")).unwrap();
    store::mutate(&f.state, &f.key, None, |s| {
        s.submit(Work {
            task: f.task.clone(),
            run_id: "run_rebuilt".into(),
        })
    })
    .unwrap();
    assert_eq!(
        run(f.turn("new context")).await.unwrap().text,
        "FIXTURE_RESPONSE"
    );
    let log = std::fs::read_to_string(f.root.path().join("work/protocol.jsonl")).unwrap();
    assert_eq!(log.matches("session/new").count(), 2);
}

#[tokio::test]
async fn gemini_restore_protects_exact_session_and_reports_native_loss() {
    let mut f = Fixture::new();
    f.profile.driver_id = "gemini".into();
    run(f.turn("hello")).await.unwrap();
    f.next();
    store::mutate(&f.state, &f.key, None, |s| {
        s.native_created_at_ms = Some(0);
        Ok(())
    })
    .unwrap();
    let launch = launch_in_workspace(
        &f.profile,
        &f.root.path().join("work"),
        Some("native-exact-session"),
    )
    .unwrap();
    assert!(launch
        .arguments()
        .ends_with(&["--resume".into(), "native-exact-session".into()]));
    std::fs::write(f.root.path().join("work/missing-gemini"), "").unwrap();
    assert!(run(f.turn("again")).await.is_err());
    assert!(store::load(&f.state, &f.key).unwrap().context_lost);
    let log = std::fs::read_to_string(f.root.path().join("work/protocol.jsonl")).unwrap();
    assert_eq!(log.matches("session/new").count(), 1);
    assert!(!missing_native_context(
        &acp::Error::internal_error(),
        Brand::Gemini,
        "native-exact-session"
    ));
    assert!(!missing_native_context(
        &acp::Error::invalid_params(),
        Brand::Gemini,
        "native-exact-session"
    ));
}

#[tokio::test]
async fn client_specific_recovery_errors_require_conclusive_session_loss() {
    for (brand, mode, lost) in [
        ("kimi", "kimi", true),
        ("deepseek-harness", "dsh", true),
        ("opencode", "list-absent", true),
        ("opencode", "list-present", false),
        ("opencode", "list-malformed", false),
        ("opencode", "list-bad-cursor", false),
        ("opencode", "list-error", false),
        ("opencode", "list-loop", false),
        ("gemini", "startup-missing", true),
        ("gemini", "startup-other-id", false),
        ("gemini", "startup-error", false),
    ] {
        let mut f = Fixture::new();
        // Establish a session with the ordinary fixture before exercising the
        // real client's failure shape (DSH's MCP overlay is not a mock server).
        run(f.turn("hello")).await.unwrap();
        f.next();
        f.profile.driver_id = brand.into();
        store::mutate(&f.state, &f.key, None, |s| {
            s.native_created_at_ms = Some(0);
            Ok(())
        })
        .unwrap();
        std::fs::write(f.root.path().join("work/recovery-mode"), mode).unwrap();
        assert!(run(f.turn("again")).await.is_err(), "{mode}");
        let after = store::load(&f.state, &f.key).unwrap();
        assert_eq!(after.context_lost, lost, "{mode}");
        assert_eq!(
            after.native_session_id.as_deref(),
            Some("native-exact-session")
        );
        let log = std::fs::read_to_string(f.root.path().join("work/protocol.jsonl")).unwrap();
        assert_eq!(log.matches("session/new").count(), 1, "{mode}");
        if mode == "list-absent" || mode == "list-present" {
            assert_eq!(log.matches("session/list").count(), 2, "{mode}");
        }
    }
}

#[test]
fn context_loss_cannot_modify_a_stopped_or_replaced_run() {
    let f = Fixture::new();
    store::mutate(&f.state, &f.key, None, |s| {
        s.native_session_id = Some("native".into());
        Ok(())
    })
    .unwrap();
    mark_context_lost(&f.state, &f.key, "old-run", "native");
    mark_context_lost(&f.state, &f.key, "run_a", "wrong-session");
    assert!(!store::load(&f.state, &f.key).unwrap().context_lost);
    store::mutate(&f.state, &f.key, None, |s| {
        s.stopping = true;
        Ok(())
    })
    .unwrap();
    mark_context_lost(&f.state, &f.key, "run_a", "native");
    assert!(!store::load(&f.state, &f.key).unwrap().context_lost);
}

async fn answer_case(mode: &str, field: &str) {
    let f = Fixture::new();
    let turn = tokio::spawn(run(f.turn(mode)));
    let question = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let s = store::load(&f.state, &f.key).unwrap();
            if let Some(q) = s.questions.first() {
                break q.clone();
            };
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert!(
        !turn.is_finished(),
        "the client must wait for a real answer"
    );
    let mut content = json!({});
    content[field] = json!("blue");
    let snapshot = store::load(&f.state, &f.key).unwrap();
    store::control(&f.state,&f.task.agent_did,&f.task.requester_did,"answer-1",&json!({"action":"answer","session_key":f.key,"revision":snapshot.revision,"run_id":"run_a","question_id":question.id,"response":{"action":"accept","content":content}})).unwrap();
    assert_eq!(turn.await.unwrap().unwrap().text, "ANSWER_blue");
}
#[tokio::test]
async fn native_question_waits_for_user() {
    answer_case("QUESTION_NATIVE", "color").await;
}
#[tokio::test]
async fn shared_question_tool_waits_for_user() {
    answer_case("QUESTION_MCP", "color").await;
}

#[tokio::test]
async fn stopping_pending_questions_is_cancellation_and_preserves_waiting_work() {
    for mode in ["QUESTION_NATIVE", "QUESTION_MCP"] {
        let f = Fixture::new();
        let running = tokio::spawn(run(f.turn(mode)));
        tokio::time::timeout(Duration::from_secs(10), async {
            while store::load(&f.state, &f.key).unwrap().questions.is_empty() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        store::mutate(&f.state, &f.key, None, |s| {
            let mut task = f.task.clone();
            task.task_id = "task_b".into();
            s.submit(Work {
                task,
                run_id: "run_b".into(),
            })?;
            s.stopping = true;
            Ok(())
        })
        .unwrap();
        let result = tokio::time::timeout(Duration::from_secs(15), running)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(result.cancelled, "{mode}: stopped question must not fail");
        assert!(store::load(&f.state, &f.key)
            .unwrap()
            .interaction_error
            .is_none());
        store::mutate(&f.state, &f.key, None, |s| {
            assert!(s.complete("run_a", "cancelled")?.is_none());
            assert_eq!(s.waiting.as_ref().unwrap().run_id, "run_b");
            assert!(s.waiting_paused);
            Ok(())
        })
        .unwrap();
    }
}
#[test]
fn kimi_child_timeout_covers_human_answer_expiry_without_changing_other_clients() {
    let mut profile = CliRuntimeProfileRecord::for_driver("question-timeout", "kimi").unwrap();
    let config = launch_config(&profile).unwrap();
    assert_eq!(
        config
            .environment()
            .get("KIMI_MCP_TOOL_TIMEOUT_MS")
            .map(String::as_str),
        Some("960000")
    );
    for driver in ["opencode", "gemini", "deepseek-harness"] {
        profile.driver_id = driver.into();
        assert!(!launch_config(&profile)
            .unwrap()
            .environment()
            .contains_key("KIMI_MCP_TOOL_TIMEOUT_MS"));
    }
}
#[tokio::test]
async fn ending_a_prompt_with_an_unanswered_question_is_a_failure() {
    let f = Fixture::new();
    assert!(run(f.turn("QUESTION_NATIVE_ABANDON")).await.is_err());
    assert!(store::load(&f.state, &f.key).unwrap().questions[0]
        .response
        .is_none());
}

#[tokio::test]
async fn question_progress_stream_keeps_waiting_and_returns_only_the_real_answer() {
    let f = Fixture::new();
    store::mutate(&f.state, &f.key, None, |s| {
        s.native_session_id = Some("native-exact-session".into());
        Ok(())
    })
    .unwrap();
    let tool = crate::acp::question_tool::QuestionTool::start(
        f.state.clone(),
        f.key.clone(),
        "run_a".into(),
    )
    .await
    .unwrap();
    let client = reqwest::Client::new();
    let body = json!({"jsonrpc":"2.0","id":42,"method":"tools/call","params":{"_meta":{"progressToken":"human-wait"},"name":"request_user_input","arguments":{"message":"Choose","schema_json":json!({"type":"object","required":["color"],"properties":{"color":{"type":"string","enum":["red","blue"]}}}).to_string()}}});
    let mut response = client
        .post(tool.config["url"].as_str().unwrap())
        .header(
            "Authorization",
            tool.config["headers"][0]["value"].as_str().unwrap(),
        )
        .header("Accept", "application/json, text/event-stream")
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(response.headers()["content-type"], "text/event-stream");
    let mut output = String::new();
    while !output.contains("notifications/progress") {
        output.push_str(
            &String::from_utf8(response.chunk().await.unwrap().unwrap().to_vec()).unwrap(),
        );
    }
    assert!(!output.contains("\"result\""));
    assert!(output.contains("human-wait"));
    let session = store::load(&f.state, &f.key).unwrap();
    let q = &session.questions[0];
    assert!(q.response.is_none());
    // An unrelated cancellation must not invalidate this question.
    client
        .post(tool.config["url"].as_str().unwrap())
        .header(
            "Authorization",
            tool.config["headers"][0]["value"].as_str().unwrap(),
        )
        .header("Content-Type", "application/json")
        .body(
            json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":99}})
                .to_string(),
        )
        .send()
        .await
        .unwrap();
    assert!(store::load(&f.state, &f.key)
        .unwrap()
        .interaction_error
        .is_none());
    store::control(&f.state,&f.task.agent_did,&f.task.requester_did,"stream-answer",&json!({"action":"answer","session_key":f.key,"revision":session.revision,"run_id":"run_a","question_id":q.id,"response":{"action":"accept","content":{"color":"blue"}}})).unwrap();
    while let Some(chunk) = response.chunk().await.unwrap() {
        output.push_str(&String::from_utf8(chunk.to_vec()).unwrap());
    }
    let events = output
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .map(|data| serde_json::from_str::<Value>(data).unwrap())
        .collect::<Vec<_>>();
    let replies = events.iter().filter(|e| e["id"] == 42).collect::<Vec<_>>();
    assert_eq!(replies.len(), 1);
    let answer: Value =
        serde_json::from_str(replies[0]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(
        answer,
        json!({"action":"accept","content":{"color":"blue"},"answer_format":"awiki.answer.v2","mode":"structured"})
    );
}
#[tokio::test]
async fn permission_question_never_auto_selects_first_option() {
    let f = Fixture::new();
    let result = run(f.turn("QUESTION_PERMISSION")).await;
    assert!(result.is_err());
    assert!(store::load(&f.state, &f.key).unwrap().questions.is_empty());
}

#[tokio::test]
async fn cancellation_ignores_late_output_and_kills_descendants() {
    let f = Fixture::new();
    let task = tokio::spawn(run(f.turn("CANCEL_CHILD")));
    tokio::time::timeout(Duration::from_secs(10), async {
        while !f.root.path().join("work/child.pid").exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    store::mutate(&f.state, &f.key, None, |s| {
        s.submit(Work {
            task: f.task.clone(),
            run_id: "waiting-after-shutdown".into(),
        })
    })
    .unwrap();
    store::request_shutdown(&f.state).unwrap();
    assert!(task.await.unwrap().unwrap().cancelled);
    store::mutate(&f.state, &f.key, None, |s| {
        assert!(s.complete("run_a", "cancelled")?.is_none());
        assert!(s.waiting_paused);
        Ok(())
    })
    .unwrap();
    assert!(!store::load(&f.state, &f.key)
        .unwrap()
        .text
        .contains("LATE_OUTPUT"));
    let pid = std::fs::read_to_string(f.root.path().join("work/child.pid")).unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let output = std::process::Command::new("ps")
                .args(["-p", pid.trim(), "-o", "stat="])
                .output()
                .unwrap();
            let status = String::from_utf8_lossy(&output.stdout);
            if !output.status.success() || status.trim().starts_with('Z') {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn unknown_interaction_and_refusal_cannot_be_reported_as_success() {
    for prompt in ["UNKNOWN_INTERACTION", "REFUSAL"] {
        let f = Fixture::new();
        assert!(run(f.turn(prompt)).await.is_err());
    }
}

#[test]
fn attachment_origin_is_durable_and_changed_bytes_are_rejected() {
    let f = Fixture::new();
    let path = f.root.path().join("file.txt");
    std::fs::write(&path, "original").unwrap();
    let item = crate::acp::attachments::AuthorizedAttachment::from_download(
        path.clone(),
        "file.txt".into(),
        "text/plain".into(),
    )
    .unwrap();
    crate::acp::attachments::remember(&f.state, &f.task.agent_did, "a", &[item]).unwrap();
    assert_eq!(
        crate::acp::attachments::prompt_blocks(&f.state, &f.task)
            .unwrap()
            .len(),
        1
    );
    std::fs::write(path, "changed").unwrap();
    assert!(crate::acp::attachments::prompt_blocks(&f.state, &f.task).is_err());
    let mut other = f.task.clone();
    other.agent_did = "did:agent:other".into();
    assert!(crate::acp::attachments::prompt_blocks(&f.state, &other)
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn final_and_status_delivery_retries_do_not_run_the_model_again() {
    use crate::outbox::*;
    use crate::state::AuthorizedRuntimeContext;
    struct Unavailable;
    impl RuntimeOutbox for Unavailable {
        fn send_status(
            &self,
            _: &AuthorizedRuntimeContext,
            _: &str,
            _: Option<&str>,
        ) -> Result<()> {
            bail!("offline")
        }
        fn send_final(&self, _: &AuthorizedRuntimeContext, _: Option<&str>) -> Result<()> {
            bail!("offline")
        }
        fn send_message(
            &self,
            _: &AuthorizedRuntimeContext,
            _: &RuntimeMessageSend,
        ) -> Result<RuntimeMessageSendResult> {
            bail!("offline")
        }
        fn send_attachment(
            &self,
            _: &AuthorizedRuntimeContext,
            _: &RuntimeAttachmentSend,
        ) -> Result<RuntimeAttachmentSendResult> {
            bail!("offline")
        }
    }
    let mut f = Fixture::new();
    f.task.text = "ACP_HOST_USER_REQUEST — 多行正文\n第二行".into();
    let mut profile = f
        .state
        .load_runtime_agent_profile(&f.task.agent_did)
        .unwrap();
    profile.workspace_root = Some(f.root.path().join("work"));
    profile.workspace_id = Some("workspace-test".into());
    profile.workspace_mode = Some(crate::workspace::WorkspaceMode::SharedRoot);
    f.state.upsert_runtime_agent_profile(&profile).unwrap();
    f.state.upsert_cli_runtime_profile(&f.profile).unwrap();
    f.state.insert_runtime_task(&f.task).unwrap();
    let run = crate::runtime::RuntimeRun {
        run_id: "run_a".into(),
        task_id: f.task.task_id.clone(),
        agent_did: profile.agent_did.clone(),
        runtime_profile_id: profile.runtime_profile_id.clone(),
        runtime_plugin_id: "acp".into(),
        workspace_id: profile.workspace_id.clone(),
        status: crate::runtime::RuntimeRunStatus::Pending,
    };
    f.state.try_insert_runtime_run(&run).unwrap();
    f.state.connection().unwrap().execute("INSERT INTO acp_events(event_id,session_key,run_id,snapshot,sent) VALUES('broken-event','broken-session','missing-run','{}',0)",[]).unwrap();
    crate::acp::host::execute(
        &f.state,
        &profile,
        &Unavailable,
        &f.key,
        Work {
            task: f.task.clone(),
            run_id: "run_a".into(),
        },
        Some(&f.root.path().join("daemon.sock")),
    )
    .unwrap();
    let prompts = std::fs::read_to_string(
        f.root
            .path()
            .join("work/acp")
            .join(&f.key)
            .join("prompts.jsonl"),
    )
    .unwrap();
    let prompt: Vec<Value> = serde_json::from_str(prompts.lines().next().unwrap()).unwrap();
    assert_eq!(
        prompt.last().unwrap()["text"],
        format!("[User request]\n{}", f.task.text)
    );
    assert_eq!(prompts.matches("ACP_HOST_USER_REQUEST").count(), 1);
    let environment: Value = serde_json::from_slice(
        &std::fs::read(
            f.root
                .path()
                .join("work/acp")
                .join(&f.key)
                .join("wrapper-environment.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        environment,
        json!({"token_present":true,"socket_present":true,"executable_present":true})
    );
    let final_record = f
        .state
        .load_runtime_final_outbox_by_run("run_a")
        .unwrap()
        .unwrap();
    assert_eq!(final_record.status, "pending");
    assert_eq!(
        store::load(&f.state, &f.key).unwrap().last_task["state"],
        "finished"
    );
    f.state
        .connection()
        .unwrap()
        .execute("UPDATE runtime_final_outbox SET next_attempt_at_ms=0", [])
        .unwrap();
    let available = MemoryRuntimeOutbox::default();
    crate::runtime::host::flush_runtime_final_outbox(&f.state, &available, 20).unwrap();
    assert!(crate::acp::host::flush_events(&f.state, &available, 64).is_err());
    let pending: i64 = f
        .state
        .connection()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM acp_events WHERE sent=0", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(
        pending, 1,
        "one broken event must not block healthy sessions"
    );
    f.state
        .connection()
        .unwrap()
        .execute("DELETE FROM acp_events WHERE event_id='broken-event'", [])
        .unwrap();
    crate::acp::host::flush_events(&f.state, &available, 64).unwrap();
    crate::acp::host::run(
        &f.state,
        &profile,
        &available,
        f.task.clone(),
        "run_a".into(),
        None,
    )
    .unwrap();
    assert_eq!(
        f.state
            .load_runtime_final_outbox_by_run("run_a")
            .unwrap()
            .unwrap()
            .status,
        "sent"
    );
    let log = std::fs::read_to_string(
        profile
            .workspace_root
            .unwrap()
            .join("acp")
            .join(&f.key)
            .join("protocol.jsonl"),
    )
    .unwrap();
    assert_eq!(
        log.matches("session/new").count(),
        1,
        "delivery retry must not spawn another model call"
    );
    let records = available.records();
    let status = records
        .iter()
        .filter_map(|r| r.metadata.as_ref())
        .find(|m| m["acp"].is_object())
        .unwrap();
    assert_eq!(
        status["acp"]["conversation_id"],
        f.task.conversation_id.unwrap()
    );
    assert_eq!(status["acp"]["last_task"]["source_message_id"], "a");
}

#[test]
fn failed_attachment_download_is_not_dropped_from_the_prompt() {
    let f = Fixture::new();
    crate::acp::attachments::remember_failure(&f.state, &f.task.agent_did, "a").unwrap();
    assert_eq!(
        crate::acp::attachments::prompt_blocks(&f.state, &f.task)
            .unwrap_err()
            .to_string(),
        "attachment_download_failed"
    );
}

#[test]
fn attachment_preparation_failure_survives_storage_without_transport_details() {
    let f = Fixture::new();
    let failure = crate::acp::attachments::AttachmentFailure::from_core(
        &im_core::ImError::AttachmentPreparation {
            stage: im_core::AttachmentPreparationStage::Discovery,
            retryable: true,
            cause: Box::new(im_core::ImError::TransportUnavailable {
                detail: "https://private.invalid/secret?token=test".into(),
            }),
        },
    );
    crate::acp::attachments::remember_failure_details(&f.state, &f.task.agent_did, "a", &failure)
        .unwrap();
    let error = crate::acp::attachments::prompt_blocks(&f.state, &f.task).unwrap_err();
    let stored = error
        .downcast_ref::<crate::acp::attachments::AttachmentFailure>()
        .unwrap();
    assert_eq!(stored, &failure);
    assert_eq!(stored.stage, "discovery");
    assert!(stored.retryable);
    assert_eq!(stored.code, "attachment_download_network");
    assert!(!serde_json::to_string(stored).unwrap().contains("secret"));
}

#[tokio::test]
async fn question_tool_rejects_foreign_origins_and_stale_tasks_and_closes() {
    let f = Fixture::new();
    let tool = crate::acp::question_tool::QuestionTool::start(
        f.state.clone(),
        f.key.clone(),
        "run_a".into(),
    )
    .await
    .unwrap();
    let url = tool.config["url"].as_str().unwrap().to_owned();
    let token = tool.config["headers"][0]["value"].as_str().unwrap();
    let client = reqwest::Client::new();
    let body = json!({"jsonrpc":"2.0","id":1,"method":"tools/list"});
    assert_eq!(
        client
            .post(&url)
            .header("Content-Type", "application/json")
            .body(body.to_string())
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    assert_eq!(
        client
            .post(&url)
            .header("Authorization", token)
            .header("Origin", "https://foreign.example")
            .header("Content-Type", "application/json")
            .body(body.to_string())
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    assert_eq!(
        client
            .post(&url)
            .header("Authorization", token)
            .header("Host", "foreign.example")
            .header("Content-Type", "application/json")
            .body(body.to_string())
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    let response = client
        .post(&url)
        .header("Authorization", token)
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    let response: Value = serde_json::from_slice(&response).unwrap();
    assert_eq!(response["result"]["tools"][0]["name"], "request_user_input");
    store::mutate(&f.state, &f.key, None, |s| s.complete("run_a", "cancelled")).unwrap();
    let response = client
        .post(&url)
        .header("Authorization", token)
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    let response: Value = serde_json::from_slice(&response).unwrap();
    assert!(response["error"].is_object());
    drop(tool);
    tokio::task::yield_now().await;
    assert!(client
        .post(&url)
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .send()
        .await
        .is_err());
}

#[tokio::test]
async fn concurrent_questions_keep_their_answers_and_device_race_has_one_winner() {
    let f = Fixture::new();
    store::mutate(&f.state, &f.key, None, |session| {
        session.native_session_id = Some("native-concurrent-session".into());
        Ok(())
    })
    .unwrap();
    let tool = crate::acp::question_tool::QuestionTool::start(
        f.state.clone(),
        f.key.clone(),
        "run_a".into(),
    )
    .await
    .unwrap();
    let request = |id: u64, message: &str| {
        let body = json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"request_user_input","arguments":{"message":message,"schema_json":json!({"type":"object","required":["value"],"properties":{"value":{"type":"string","enum":["one","two"]}}}).to_string()}}});
        let builder = reqwest::Client::new()
            .post(tool.config["url"].as_str().unwrap())
            .header(
                "Authorization",
                tool.config["headers"][0]["value"].as_str().unwrap(),
            )
            .header("Content-Type", "application/json")
            .body(body.to_string());
        tokio::spawn(async move {
            let bytes = builder.send().await.unwrap().bytes().await.unwrap();
            serde_json::from_slice::<Value>(&bytes).unwrap()
        })
    };
    let first = request(1, "First question");
    let second = request(2, "Second question");
    let session = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let session = store::load(&f.state, &f.key).unwrap();
            if session.questions.len() == 2 {
                break session;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(session.waiting.is_none());
    assert!(!first.is_finished() && !second.is_finished());
    let question = |message: &str| {
        session
            .questions
            .iter()
            .find(|q| q.request["message"] == message)
            .unwrap()
    };
    let answer = |id: &str, value: &str| json!({"action":"answer","session_key":f.key,"revision":session.revision,"run_id":"run_a","question_id":id,"response":{"action":"accept","content":{"value":value}}});
    let second_id = &question("Second question").id;
    let args_a = answer(second_id, "one");
    let args_b = answer(second_id, "two");
    let (a, b) = std::thread::scope(|scope| {
        let a = scope.spawn(|| {
            store::control(
                &f.state,
                &f.task.agent_did,
                &f.task.requester_did,
                "device-a",
                &args_a,
            )
        });
        let b = scope.spawn(|| {
            store::control(
                &f.state,
                &f.task.agent_did,
                &f.task.requester_did,
                "device-b",
                &args_b,
            )
        });
        (a.join().unwrap(), b.join().unwrap())
    });
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    let (winner, args, result, loser) = if a.is_ok() {
        ("device-a", &args_a, a.unwrap(), b.unwrap_err())
    } else {
        ("device-b", &args_b, b.unwrap(), a.unwrap_err())
    };
    assert!(loser.to_string().contains("stale"));
    let retry = store::control(
        &f.state,
        &f.task.agent_did,
        &f.task.requester_did,
        winner,
        args,
    )
    .unwrap();
    assert_eq!(retry.0, result.0);
    assert!(retry.1.is_none());
    let reply = tokio::time::timeout(Duration::from_secs(3), second)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reply["id"], 2);
    let content: Value =
        serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(content["content"], args["response"]["content"]);
    assert_eq!(content["answer_format"], "awiki.answer.v2");
    assert_eq!(content["mode"], "structured");
    assert!(!first.is_finished());
    let first_args = answer(&question("First question").id, "one");
    store::control(
        &f.state,
        &f.task.agent_did,
        &f.task.requester_did,
        "first-answer",
        &first_args,
    )
    .unwrap();
    let reply = tokio::time::timeout(Duration::from_secs(3), first)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reply["id"], 1);
    let content: Value =
        serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(content["content"], first_args["response"]["content"]);
    assert_eq!(content["answer_format"], "awiki.answer.v2");
}

/// Explicit opt-in: this invokes a real installed client/model. The fixture and
/// credentials live outside the repository; reports contain only verdicts.
#[cfg(target_os = "linux")]
fn tool_host_pid(native_pid: u32, cwd: &std::path::Path) -> Option<u32> {
    // DSH's Linux sandbox gives the shell a PID in its own namespace. Match
    // both the namespace PID and this fixture's unique working directory;
    // never mistake namespace PID 2 for the host kernel's PID 2.
    let cwd = cwd.canonicalize().ok()?;
    let mut matches = vec![];
    for entry in std::fs::read_dir("/proc").ok()?.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|s| s.parse::<u32>().ok())
        else {
            continue;
        };
        let path = entry.path();
        let Ok(status) = std::fs::read_to_string(path.join("status")) else {
            continue;
        };
        let reported = status.lines().find_map(|line| {
            line.strip_prefix("NSpid:")
                .and_then(|value| value.split_whitespace().last())
                .and_then(|value| value.parse::<u32>().ok())
        });
        if reported == Some(native_pid)
            && std::fs::read_link(path.join("cwd")).is_ok_and(|p| p == cwd)
        {
            matches.push(pid);
        }
    }
    (matches.len() == 1).then(|| matches[0])
}

#[cfg(not(target_os = "linux"))]
fn tool_host_pid(native_pid: u32, _cwd: &std::path::Path) -> Option<u32> {
    (native_pid > 1).then_some(native_pid)
}

#[cfg(target_os = "linux")]
#[test]
fn real_tool_pid_resolution_requires_the_exact_working_directory() {
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    let native_pid = status
        .lines()
        .find_map(|line| line.strip_prefix("NSpid:"))
        .unwrap()
        .split_whitespace()
        .last()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(
        tool_host_pid(native_pid, &std::env::current_dir().unwrap()),
        Some(std::process::id())
    );
    assert_eq!(
        tool_host_pid(native_pid, tempfile::tempdir().unwrap().path()),
        None
    );
}

/// Explicit opt-in: reports include native tool process termination.
#[tokio::test]
#[ignore = "requires AWIKI_ACP_REAL_FIXTURE and a configured real model"]
async fn real_client_missing_context_requires_confirmed_reset() {
    let settings: Value = serde_json::from_slice(
        &std::fs::read(std::env::var("AWIKI_ACP_REAL_FIXTURE").unwrap()).unwrap(),
    )
    .unwrap();
    let mut f = Fixture::new();
    f.profile.driver_id = settings["driver_id"].as_str().unwrap().into();
    f.profile.binary_path = Some(settings["binary_path"].as_str().unwrap().into());
    let first = run(f.turn("Reply exactly ACP_READY. Do not use tools."))
        .await
        .unwrap();
    assert_eq!(first.text.trim(), "ACP_READY");
    f.next();
    let native = store::load(&f.state, &f.key)
        .unwrap()
        .native_session_id
        .unwrap();
    assert!(native.is_ascii() && native.len() > 12);
    // A same-shape nonexistent ID exercises real client recovery without
    // deleting or rewriting any native history (including the original).
    let missing = format!(
        "{}{}",
        &native[..native.len() - 12],
        native[native.len() - 12..]
            .chars()
            .map(|c| if c == '7' { '8' } else { '7' })
            .collect::<String>()
    );
    store::mutate(&f.state, &f.key, None, |s| {
        s.native_session_id = Some(missing.clone());
        s.native_created_at_ms = Some(0);
        Ok(())
    })
    .unwrap();
    let failed = run(f.turn("This request must fail before any model prompt."))
        .await
        .is_err();
    let lost = store::load(&f.state, &f.key).unwrap();
    let preserved = lost.native_session_id.as_deref() == Some(missing.as_str());
    store::mutate(&f.state, &f.key, None, |s| {
        let run = s.active.as_ref().unwrap().run_id.clone();
        s.complete(&run, "failed")
    })
    .unwrap();
    let before = store::load(&f.state, &f.key).unwrap();
    let mut reset = json!({"action":"reset_context","session_key":f.key,"revision":before.revision,"confirmed":false});
    let denied = store::control(
        &f.state,
        &f.task.agent_did,
        &f.task.requester_did,
        "unconfirmed",
        &reset,
    )
    .is_err();
    reset["confirmed"] = json!(true);
    let confirmed = store::control(
        &f.state,
        &f.task.agent_did,
        &f.task.requester_did,
        "confirmed",
        &reset,
    )
    .is_ok();
    let after = store::load(&f.state, &f.key).unwrap();
    let history = after.history == before.history;
    let mut rebuilt = false;
    if confirmed {
        store::mutate(&f.state, &f.key, None, |s| {
            s.submit(Work {
                task: f.task.clone(),
                run_id: "run_rebuilt".into(),
            })
        })
        .unwrap();
        rebuilt = run(f.turn("Reply exactly ACP_REBUILT. Do not use tools."))
            .await
            .is_ok_and(|r| r.text.trim() == "ACP_REBUILT")
            && store::load(&f.state, &f.key)
                .unwrap()
                .native_session_id
                .as_deref()
                .is_some_and(|id| id != missing && id != native);
    }
    let report = json!({"driver":f.profile.driver_id,"failed_before_prompt":failed,"context_lost":lost.context_lost,"native_id_preserved":preserved,"unconfirmed_denied":denied,"confirmed":confirmed,"history_preserved":history,"rebuilt":rebuilt});
    let report_path =
        PathBuf::from(settings["report_path"].as_str().unwrap()).with_extension("recovery.json");
    std::fs::write(report_path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    println!("recovery: {report}");
    assert!(failed && lost.context_lost && preserved && denied && confirmed && history && rebuilt);
}

#[tokio::test]
#[ignore = "requires AWIKI_ACP_REAL_FIXTURE and a configured real model"]
async fn real_client_model_tools_resume_image_question_and_cancel() {
    let path = std::env::var("AWIKI_ACP_REAL_FIXTURE").expect("real client fixture path");
    let settings: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let expected_image_text = settings["image_expected_text"]
        .as_str()
        .expect("fixture requires an eight-character code present only in image pixels");
    assert!(
        expected_image_text.len() == 8
            && expected_image_text
                .bytes()
                .all(|c| b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789".contains(&c))
    );
    let mut f = Fixture::new();
    f.profile.driver_id = settings["driver_id"].as_str().unwrap().into();
    f.profile.binary_path = Some(settings["binary_path"].as_str().unwrap().into());
    let probe = inspect(&f.profile).await.unwrap();
    let mut cases = vec![];
    for (name, prompt, expected) in [
        (
            "text",
            "Reply exactly ACP_READY. Do not use tools.",
            "ACP_READY",
        ),
        (
            "tools",
            "Use tools to create proof.txt in the current directory containing exactly ACP_TOOL_OK. Then read it with a tool and reply with the contents.",
            "ACP_TOOL_OK",
        ),
        (
            "resume",
            "From the previous conversation, what were the exact contents of the file you created? Reply with the contents only. Do not use tools.",
            "ACP_TOOL_OK",
        ),
    ] {
        let output = run(f.turn(prompt)).await;
        let pass = output.as_ref().is_ok_and(|o| if name == "tools" { o.text.contains(expected) } else { o.text.trim() == expected })
            && (name != "tools"
                || std::fs::read_to_string(f.root.path().join("work/proof.txt"))
                    .is_ok_and(|s| s.trim() == "ACP_TOOL_OK"));
        cases.push(json!({"case":name,"pass":pass,"error":output.err().map(|e|e.to_string())}));
        println!("{name}: {pass}");
        f.next();
    }
    // The expected code never appears in the prompt, filename or tool output.
    // Exact text recognition exercises image content without a one-word guess.
    let mut turn = f.turn("Read the eight-character code printed in the attached image. Reply with only that code, preserving case. Do not use tools.");
    turn.prompt.push(
        serde_json::from_value(
            json!({"type":"image","mimeType":"image/png","data":settings["image_base64"]}),
        )
        .unwrap(),
    );
    let image = run(turn).await;
    cases.push(json!({"case":"image","oracle":"pixel_text_v1","pass":image.as_ref().is_ok_and(|o|o.text.trim() == expected_image_text),"error":image.err().map(|e|e.to_string())}));
    f.next();
    let path = f.root.path().join("work/incoming.txt");
    std::fs::write(&path, "ATTACHMENT_PROOF_72491").unwrap();
    let mut turn = f.turn("Read the attached file using a tool and reply with its exact contents.");
    turn.prompt.push(serde_json::from_value(json!({"type":"resource_link","uri":reqwest::Url::from_file_path(path).unwrap().as_str(),"name":"incoming.txt","mimeType":"text/plain"})).unwrap());
    let file = run(turn).await;
    cases.push(json!({"case":"file","pass":file.as_ref().is_ok_and(|o|o.text.contains("ATTACHMENT_PROOF_72491")),"error":file.err().map(|e|e.to_string())}));
    f.next();
    let question_task=tokio::spawn(run(f.turn("Use the awiki_questions request_user_input tool to ask me to choose red or blue. Use a required color string field with enum red and blue. Wait for my answer. After the answer, reply exactly ACP_ANSWER_BLUE if I chose blue. Do not choose on my behalf.")));
    let question = tokio::time::timeout(Duration::from_secs(100), async {
        loop {
            let s = store::load(&f.state, &f.key).unwrap();
            if let Some(q) = s.questions.first() {
                break Some(q.clone());
            };
            if question_task.is_finished() {
                break None;
            };
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .ok()
    .flatten();
    // Human answers routinely take longer than the MCP SDK's 60-second
    // default. Exercise real clients beyond that boundary before answering.
    let waiting_started = std::time::Instant::now();
    if question.is_some() {
        for _ in 0..65 {
            if question_task.is_finished() {
                break;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
    let waited = question.is_some() && !question_task.is_finished();
    let waited_ms = waiting_started.elapsed().as_millis();
    if let Some(q) = question.filter(|_| waited) {
        let properties = q.request["requestedSchema"]["properties"]
            .as_object()
            .unwrap();
        let mut content = json!({});
        for (name, p) in properties {
            let choices = p["enum"].as_array().cloned().unwrap_or_else(|| {
                p["oneOf"]
                    .as_array()
                    .unwrap_or(&vec![])
                    .iter()
                    .map(|v| v["const"].clone())
                    .collect()
            });
            let blue = choices
                .iter()
                .find(|v| v.as_str().is_some_and(|s| s.eq_ignore_ascii_case("blue")))
                .cloned()
                .unwrap_or(json!("blue"));
            content[name] = blue;
        }
        store::mutate(&f.state,&f.key,None,|s|s.command("answer",&json!({"run_id":q.run_id,"question_id":q.id,"response":{"action":"accept","content":content}}),&f.task.requester_did,current_time_millis()?)).unwrap();
    } else {
        store::mutate(&f.state, &f.key, None, |s| {
            s.stopping = true;
            Ok(())
        })
        .unwrap();
    }
    let question = question_task.await.unwrap();
    cases.push(json!({"case":"question","waited_for_answer":waited,"waited_ms":waited_ms,"pass":waited && question.as_ref().is_ok_and(|o|o.text.contains("ACP_ANSWER_BLUE")),"error":question.err().map(|e|e.to_string())}));
    f.next();
    let cancel_task = tokio::spawn(run(f.turn(
        "Use a shell tool to run exactly: echo $$ > cancel.pid; exec sleep 30\nOnly after it finishes, reply WAIT_DONE. Do not run another command."
    )));
    // Wait for an actual tool process; cancelling during model startup does
    // not prove that the official client's tool descendants are terminated.
    let child = tokio::time::timeout(Duration::from_secs(90), async {
        loop {
            if let Some(pid) = std::fs::read_to_string(f.root.path().join("work/cancel.pid"))
                .ok()
                .and_then(|s| s.trim().parse::<u32>().ok())
                .filter(|pid| *pid > 1)
                .and_then(|pid| tool_host_pid(pid, &f.root.path().join("work")))
            {
                break Some(pid);
            }
            if cancel_task.is_finished() {
                break None;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .ok()
    .flatten();
    store::mutate(&f.state, &f.key, None, |s| {
        s.stopping = true;
        Ok(())
    })
    .unwrap();
    let cancel = cancel_task.await.unwrap();
    let child_terminated = if let Some(pid) = child {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let result = std::process::Command::new("ps")
                    .args(["-p", &pid.to_string(), "-o", "stat="])
                    .output()
                    .unwrap();
                if !result.status.success()
                    || String::from_utf8_lossy(&result.stdout)
                        .trim()
                        .starts_with('Z')
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .is_ok()
    } else {
        false
    };
    cases.push(json!({"case":"cancel","tool_started":child.is_some(),"child_terminated":child_terminated,"pass":child.is_some() && child_terminated && cancel.as_ref().is_ok_and(|o|o.cancelled && !o.text.contains("WAIT_DONE")),"error":cancel.err().map(|e|e.to_string())}));
    let passed = cases.iter().all(|v| v["pass"] == true);
    std::fs::write(settings["report_path"].as_str().unwrap(),serde_json::to_vec_pretty(&json!({"driver_id":f.profile.driver_id,"version":probe["binaryVersion"],"platform":std::env::consts::OS,"cases":cases})).unwrap()).unwrap();
    assert!(
        passed,
        "one or more real client cases failed; see the sanitized report"
    );
}

// Executed only by the crash test in its own OS process, never a model call.
#[test]
#[ignore = "subprocess fixture for daemon_crash_terminates_acp_descendants"]
fn crash_parent_fixture() {
    let marker = std::env::var("AWIKI_ACP_CRASH_FIXTURE_FILE").unwrap();
    let f = Fixture::new();
    std::fs::write(marker, f.root.path().to_string_lossy().as_bytes()).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(run(f.turn("CANCEL_CHILD")))
        .unwrap();
}

#[test]
fn daemon_crash_terminates_acp_descendants() {
    struct ChildGuard(std::process::Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("fixture-root");
    let mut parent = ChildGuard(
        std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "acp::client::tests::crash_parent_fixture",
            ])
            .env("AWIKI_ACP_CRASH_FIXTURE_FILE", &marker)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    let (fixture_root, pid) = loop {
        if let Ok(path) = std::fs::read_to_string(&marker) {
            let path = PathBuf::from(path);
            if let Ok(pid) = std::fs::read_to_string(path.join("work/child.pid")) {
                break (path, pid);
            }
        }
        assert!(
            std::time::Instant::now() < deadline,
            "fixture did not start"
        );
        std::thread::sleep(Duration::from_millis(25));
    };
    parent.0.kill().unwrap();
    parent.0.wait().unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let output = std::process::Command::new("ps")
            .args(["-p", pid.trim(), "-o", "stat="])
            .output()
            .unwrap();
        if !output.status.success()
            || String::from_utf8_lossy(&output.stdout)
                .trim()
                .starts_with('Z')
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "ACP descendant survived its daemon"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    // The killed helper cannot run TempDir::drop.
    std::fs::remove_dir_all(fixture_root).unwrap();
}

#[test]
fn dsh_question_overlay_is_private_ephemeral_and_keeps_credentials_in_environment() {
    use std::os::unix::fs::PermissionsExt;
    let question = json!({"url":"http://127.0.0.1:12345/mcp", "headers":[{"name":"Authorization","value":"Bearer task-only-secret"}]});
    let (launch, patch) = configure_question_tool(
        Brand::DeepseekHarness,
        AcpAgentConfig::new("dsh"),
        &question,
    )
    .unwrap();
    assert_eq!(patch.len(), 2);
    let path = patch[0].path().to_owned();
    let module = patch[1].path().to_owned();
    let code = std::fs::read_to_string(&module).unwrap();
    assert!(code.contains("ctx.shellEnv.register"));
    assert!(code.contains("DSH_AWIKI_RUNTIME_RPC_TOKEN: value"));
    assert!(!code.contains("task-only-secret"));
    assert!(!code.contains("DEEPSEEK_API_KEY"));
    let contents = std::fs::read_to_string(&path).unwrap();
    assert!(contents.contains("toolCallTimeoutMs: 960000"));
    assert!(!contents.contains("task-only-secret"));
    assert!(!contents.contains("12345"));
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::metadata(&module).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(contents.contains(module.to_str().unwrap()));
    assert_eq!(
        launch.environment().get("AWIKI_ACP_QUESTION_AUTH").unwrap(),
        "Bearer task-only-secret"
    );
    assert_eq!(
        launch.arguments(),
        &["--patch".to_owned(), path.to_string_lossy().into_owned()]
    );
    drop(patch);
    assert!(!path.exists());
    assert!(!module.exists());
    let (launch, patch) =
        configure_question_tool(Brand::OpenCode, AcpAgentConfig::new("opencode"), &question)
            .unwrap();
    assert!(patch.is_empty());
    assert!(launch.arguments().is_empty());
    assert!(launch.environment().is_empty());
}

#[test]
fn dsh_file_wrapper_restores_only_the_managed_task_credential_after_scrub() {
    let command = format!(
        "{} -c 'test -n \"$AWIKI_RUNTIME_RPC_TOKEN\" && test \"$AWIKI_RUNTIME_RPC_TOKEN\" = \"$EXPECTED\"'",
        super::super::host::file_wrapper_command(Brand::DeepseekHarness)
    );
    let value = "test-only-credential-with-$(false)-and-'quotes'";
    let status = std::process::Command::new("/bin/sh")
        .args(["-c", &command])
        .env_clear()
        .env("AWIKI_DAEMON_EXECUTABLE", "/bin/sh")
        .env("DSH_AWIKI_RUNTIME_RPC_TOKEN", value)
        .env("EXPECTED", value)
        .status()
        .unwrap();
    assert!(status.success());
    for brand in [Brand::OpenCode, Brand::Gemini, Brand::Kimi] {
        assert_eq!(
            super::super::host::file_wrapper_command(brand),
            r#""$AWIKI_DAEMON_EXECUTABLE""#
        );
    }
}
