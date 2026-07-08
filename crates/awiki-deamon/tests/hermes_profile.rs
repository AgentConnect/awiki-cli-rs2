use std::sync::{Arc, Mutex};

use awiki_deamon::agent::AgentKind;
use awiki_deamon::cli_wrapper::CliWrapperRequest;
use awiki_deamon::commands::{
    handle_agent_payload_message, setup_daemon_agent, AgentCommandOutcome,
    IncomingAgentPayloadMessage, RuntimeAgentCreateOutcome,
};
use awiki_deamon::outbox::MemoryRuntimeOutbox;
use awiki_deamon::plugins::hermes::{
    repair_hermes_profile_if_needed, AWIKI_SKILLS_VERSION, HERMES_RUNTIME_PLUGIN_ID,
};
use awiki_deamon::registration::{
    AgentInventoryClient, AgentInvocationAuthorization, AgentLatestStatusUpdateItem,
    AgentRegistrationClient, AgentRegistrationExchangeRequest, AgentRegistrationExchangeResult,
    ControllerSenderScope, DidAuthMaterial, RegistrationToken, RegistrationTokenMetadata,
};
use awiki_deamon::state::HermesProfileRecord;
use awiki_deamon::{DaemonConfig, DaemonState};
use rusqlite::Connection;
use serde_json::json;
use std::sync::MutexGuard;

static ENV_LOCK: Mutex<()> = Mutex::new(());

struct EnvGuard {
    _lock: MutexGuard<'static, ()>,
    values: Vec<(&'static str, Option<String>)>,
}

impl EnvGuard {
    fn clear(keys: &[&'static str]) -> Self {
        let lock = ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let values = keys
            .iter()
            .map(|key| {
                let value = std::env::var(key).ok();
                std::env::remove_var(key);
                (*key, value)
            })
            .collect();
        Self {
            _lock: lock,
            values,
        }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (key, value) in &self.values {
            if let Some(value) = value {
                std::env::set_var(key, value);
            } else {
                std::env::remove_var(key);
            }
        }
    }
}

#[derive(Debug, Clone, Default)]
struct MockRegistrationClient {
    requests: Arc<Mutex<Vec<AgentRegistrationExchangeRequest>>>,
}

impl AgentRegistrationClient for MockRegistrationClient {
    fn exchange_token(
        &self,
        request: AgentRegistrationExchangeRequest,
    ) -> anyhow::Result<AgentRegistrationExchangeResult> {
        self.requests.lock().unwrap().push(request.clone());
        let did = request
            .did_document
            .get("id")
            .and_then(serde_json::Value::as_str)
            .unwrap()
            .to_string();
        Ok(AgentRegistrationExchangeResult {
            token_id: format!("agtok_{}_{}", request.agent_kind.as_str(), request.handle),
            did,
            user_id: Some(format!("user_{}", request.handle)),
            agent_kind: request.agent_kind,
            controller_user_id: "user-alice".to_string(),
            controller_full_handle: "alice.anpclaw.com".to_string(),
            controller_did: request.controller_did,
            handle: request.handle,
            status: "registered".to_string(),
            access_token: Some("jwt-agent-secret".to_string()),
        })
    }
}

impl AgentInventoryClient for MockRegistrationClient {
    fn verify_token(
        &self,
        _token: &RegistrationToken,
    ) -> anyhow::Result<RegistrationTokenMetadata> {
        anyhow::bail!("verify_token is not used in hermes profile tests")
    }

    fn sync_controller_scope(
        &self,
        daemon_agent_did: &str,
        _auth: &DidAuthMaterial,
    ) -> anyhow::Result<serde_json::Value> {
        Ok(json!({
            "agent_did": daemon_agent_did,
            "controller_user_id": "user-alice",
            "controller_full_handle": "alice.anpclaw.com",
            "controller_did": "did:human:alice",
            "updated_count": 1,
        }))
    }

    fn verify_controller_sender(
        &self,
        _daemon_agent_did: &str,
        sender_did: &str,
        _auth: &DidAuthMaterial,
    ) -> anyhow::Result<ControllerSenderScope> {
        if sender_did == "did:human:alice" || sender_did == "did:human:alice-new" {
            Ok(ControllerSenderScope {
                controller_user_id: "user-alice".to_string(),
                controller_full_handle: "alice.anpclaw.com".to_string(),
                controller_did: sender_did.to_string(),
                sender_did: sender_did.to_string(),
            })
        } else {
            anyhow::bail!("controller_scope_mismatch")
        }
    }

    fn authorize_agent_invocation(
        &self,
        _daemon_agent_did: &str,
        _agent_did: &str,
        _sender_did: &str,
        _source_conversation_id: Option<&str>,
        _source_message_id: Option<&str>,
        _auth: &DidAuthMaterial,
    ) -> anyhow::Result<AgentInvocationAuthorization> {
        anyhow::bail!("authorize_agent_invocation is not used in hermes profile tests")
    }

    fn update_latest_status(
        &self,
        _daemon_agent_did: &str,
        _statuses: Vec<AgentLatestStatusUpdateItem>,
        _auth: &DidAuthMaterial,
    ) -> anyhow::Result<serde_json::Value> {
        anyhow::bail!("update_latest_status is not used in hermes profile tests")
    }

    fn archive_agent(
        &self,
        _daemon_agent_did: &str,
        _agent_did: &str,
        _auth: &DidAuthMaterial,
    ) -> anyhow::Result<serde_json::Value> {
        Ok(json!({ "archived": [] }))
    }
}

fn fixture() -> (tempfile::TempDir, DaemonConfig, DaemonState) {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open_with_root_key_bytes(&config, [25_u8; 32]);
    state.initialize().unwrap();
    (root, config, state)
}

fn expect_created(outcome: AgentCommandOutcome) -> RuntimeAgentCreateOutcome {
    match outcome {
        AgentCommandOutcome::RuntimeAgentCreated(created) => created,
        other => panic!("expected runtime agent create outcome, got {other:?}"),
    }
}

#[test]
fn hermes_profile_schema_roundtrips_and_migrates_old_db() {
    let (root, config, state) = fixture();
    let record = HermesProfileRecord {
        agent_did: "did:agent:hermes".to_string(),
        runtime_profile_id: "profile_hermes_alice".to_string(),
        hermes_profile: "awiki_alice_hermes".to_string(),
        hermes_home: root.path().join("runtime/hermes/profiles/did_agent_hermes"),
        hermes_version: None,
        awiki_skills_version: AWIKI_SKILLS_VERSION.to_string(),
        status: "ready".to_string(),
    };

    state.upsert_hermes_profile(&record).unwrap();
    assert_eq!(
        state.load_hermes_profile("did:agent:hermes").unwrap(),
        record
    );

    let migrated_root = tempfile::tempdir().unwrap();
    let migrated_config = DaemonConfig::for_state_root(migrated_root.path()).unwrap();
    migrated_config.ensure_state_layout().unwrap();
    let connection = Connection::open(&migrated_config.daemon_db_path).unwrap();
    connection
        .execute_batch(
            r#"
            CREATE TABLE schema_migrations (
                version INTEGER PRIMARY KEY,
                applied_at TEXT NOT NULL
            );
            INSERT INTO schema_migrations (version, applied_at)
            VALUES (5, '2026-05-31T00:00:00.000Z');
            "#,
        )
        .unwrap();

    let summary = DaemonState::open(&migrated_config)
        .unwrap()
        .initialize()
        .unwrap();
    assert_eq!(summary.schema_version, 33);
    let table_count: i64 = Connection::open(&migrated_config.daemon_db_path)
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'hermes_profiles'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(table_count, 1);
    let create_request_table_count: i64 = Connection::open(&migrated_config.daemon_db_path)
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'runtime_agent_create_request'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(create_request_table_count, 1);
    let control_command_table_count: i64 = Connection::open(&migrated_config.daemon_db_path)
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'control_command_state'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(control_command_table_count, 1);

    let current_table_count: i64 = Connection::open(&config.daemon_db_path)
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'hermes_profiles'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(current_table_count, 1);
}

#[test]
fn hermes_profile_runtime_agent_create_installs_profile_and_skills() {
    let _env = EnvGuard::clear(&["AWIKI_HERMES_BASE_CONFIG_PATH", "HOME"]);
    let (root, config, state) = fixture();
    let home = root.path().join("home");
    let base_hermes_home = home.join(".hermes");
    std::fs::create_dir_all(&base_hermes_home).unwrap();
    std::fs::write(
        base_hermes_home.join("config.yaml"),
        "model:\n  provider: custom\n  default: gpt-5.2\n  base_url: https://example.test/v1\n",
    )
    .unwrap();
    std::fs::write(
        base_hermes_home.join(".env"),
        "OPENAI_API_KEY=sk-test-secret\n",
    )
    .unwrap();
    std::env::set_var("HOME", &home);

    let registration = MockRegistrationClient::default();
    let daemon = setup_daemon_agent(
        &config,
        &state,
        &registration,
        "@alice-mac-daemon",
        "did:human:alice",
        RegistrationToken::new("tok_daemon_secret_value").unwrap(),
    )
    .unwrap();
    let outbox = MemoryRuntimeOutbox::default();

    let outcome = handle_agent_payload_message(
        &config,
        &state,
        &registration,
        &outbox,
        IncomingAgentPayloadMessage {
            message_id: "msg_create_hermes".to_string(),
            conversation_id: Some("conv_daemon_hermes".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: daemon.agent_did.clone(),
            content_type: "application/json".to_string(),
            payload: json!({
                "schema": "awiki.agent.command.v1",
                "command_id": "cmd_create_hermes",
                "command": "runtime.agent.create",
                "target_agent_kind": "runtime",
                "args": {
                    "handle": "@alice-hermes",
                    "runtime": "hermes",
                    "controller_did": "did:human:alice",
                    "registration_token": "tok_runtime_secret_value",
                    "display_name": "Alice Hermes"
                }
            }),
        },
    )
    .unwrap();

    let created = expect_created(outcome);
    assert_eq!(created.runtime_plugin_id, HERMES_RUNTIME_PLUGIN_ID);
    assert_eq!(created.runtime_profile_id, "profile_hermes_alice_hermes");

    let runtime_agent = state.load_agent_definition(&created.agent_did).unwrap();
    assert_eq!(runtime_agent.agent_kind, AgentKind::Runtime);
    assert_eq!(
        runtime_agent.runtime_plugin_id.as_deref(),
        Some(HERMES_RUNTIME_PLUGIN_ID)
    );
    let runtime_profile = state
        .load_runtime_agent_profile(&created.agent_did)
        .unwrap();
    assert_eq!(
        runtime_profile.display_name.as_deref(),
        Some("Alice Hermes")
    );

    let hermes = state.load_hermes_profile(&created.agent_did).unwrap();
    assert_eq!(hermes.runtime_profile_id, created.runtime_profile_id);
    assert_eq!(hermes.hermes_profile, "awiki_alice_hermes");
    assert_eq!(hermes.status, "ready");
    assert_eq!(hermes.awiki_skills_version, AWIKI_SKILLS_VERSION);
    assert!(hermes.hermes_home.starts_with(root.path()));
    assert!(hermes.hermes_home.join("config.yaml").exists());
    assert!(hermes.hermes_home.join(".env").exists());

    let soul = std::fs::read_to_string(hermes.hermes_home.join("SOUL.md")).unwrap();
    let profile_json =
        std::fs::read_to_string(hermes.hermes_home.join("awiki-profile.json")).unwrap();
    let runtime_model_config =
        std::fs::read_to_string(hermes.hermes_home.join("config.yaml")).unwrap();
    let outbound_skill = std::fs::read_to_string(
        hermes
            .hermes_home
            .join("skills/awiki-outbound-messaging/SKILL.md"),
    )
    .unwrap();

    assert!(soul.contains("Awiki Hermes Runtime Agent"));
    assert!(soul.contains("始终跟随 controller 的会话语言"));
    assert!(soul.contains("使用 preferred_language"));
    assert!(soul.contains("preferred_language=zh-Hans 表示简体中文"));
    assert!(runtime_model_config.contains("provider: custom"));
    assert!(runtime_model_config.contains("default: gpt-5.2"));
    assert!(profile_json.contains("\"run_capability_token_persisted\": false"));
    assert!(profile_json.contains("process:awiki-deamon-runtime"));
    assert!(profile_json.contains("daemon-managed Hermes PATH"));
    assert!(outbound_skill.contains("awiki-deamon-runtime send"));
    assert!(outbound_skill.contains("--to <handle-or-did>"));
    assert!(outbound_skill.contains("--group"));
    assert!(outbound_skill.contains("--file"));
    assert!(outbound_skill.contains("--display-filename"));
    assert!(outbound_skill.contains("--mime-type"));
    assert!(outbound_skill.contains("same outbound message"));
    assert!(outbound_skill.contains("ordinary Awiki messages"));
    assert!(outbound_skill.contains("Do not use it for your ordinary final answer"));
    assert!(outbound_skill.contains("do not switch local identities"));
    assert!(outbound_skill.contains("Never add, infer, or override a sender identity"));
    assert!(outbound_skill.contains("Do not retry with another local identity"));
    assert!(outbound_skill.contains("Do not call `awiki-cli`"));
    assert!(!outbound_skill.contains("`awiki-cli "));
    assert!(!outbound_skill.contains("--to-handle"));
    assert!(!outbound_skill.contains("security"));
    assert!(!outbound_skill.contains("encryption"));
    assert!(!outbound_skill.contains("direct_e2ee"));
    assert!(!outbound_skill.contains("group_e2ee"));
    assert!(!outbound_skill.contains("secure_direct"));
    assert!(!outbound_skill.contains("secure_group"));
    assert!(!outbound_skill.contains("finish-message"));
    assert!(!outbound_skill.contains("send-message"));
    assert!(!outbound_skill.contains("send-attachment"));
    assert!(!hermes.hermes_home.join("skills/awiki-runtime").exists());
    assert!(!hermes.hermes_home.join("skills/awiki-messaging").exists());
    assert!(!hermes
        .hermes_home
        .join("skills/awiki-collaboration")
        .exists());

    let profile_dump = format!("{soul}\n{profile_json}\n{outbound_skill}");
    assert!(!profile_dump.contains("tok_runtime_secret_value"));
    assert!(!profile_dump.contains("tok_daemon_secret_value"));
    assert!(!profile_dump.contains("sk-test-secret"));
    assert!(!profile_dump.contains("rtok_"));
    assert!(!profile_dump.contains("runtime_rpc_token"));
    assert!(!profile_dump.contains("jwt_token"));
    assert!(!profile_dump.contains("auth_private_key_pem"));
    assert!(!profile_dump.contains("BEGIN PRIVATE KEY"));

    assert!(!hermes
        .hermes_home
        .join("plugins/awiki-runtime/plugin.yaml")
        .exists());
    assert!(!hermes
        .hermes_home
        .join("plugins/awiki-runtime/tools.py")
        .exists());

    let statuses = outbox.agent_statuses();
    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0].payload["state"], "ready");
    assert_eq!(
        statuses[0].payload["result"]["runtime_plugin_id"],
        HERMES_RUNTIME_PLUGIN_ID
    );

    let connection = Connection::open(root.path().join("daemon.db")).unwrap();
    let audit_dump: String = connection
        .query_row(
            "SELECT COALESCE(detail_json, '') FROM audit_log WHERE event_type = 'hermes.profile.initialize' LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(audit_dump.contains("\"status\":\"ready\""));
    assert!(!audit_dump.contains("tok_runtime_secret_value"));
}

#[test]
fn hermes_profile_repair_rewrites_stale_outbound_skill_without_changing_identity() {
    let _env = EnvGuard::clear(&["AWIKI_HERMES_BASE_CONFIG_PATH", "HOME"]);
    let (root, config, state) = fixture();
    let home = root.path().join("home");
    let base_hermes_home = home.join(".hermes");
    std::fs::create_dir_all(&base_hermes_home).unwrap();
    std::fs::write(
        base_hermes_home.join("config.yaml"),
        "model:\n  provider: custom\n  default: gpt-5.2\n",
    )
    .unwrap();
    std::env::set_var("HOME", &home);

    let profile = awiki_deamon::runtime::RuntimeAgentProfile {
        agent_did: "did:agent:stale-hermes".to_string(),
        agent_handle: "alice-hermes".to_string(),
        controller_user_id: "user-alice".to_string(),
        controller_full_handle: "alice.anpclaw.com".to_string(),
        controller_scope_key: "controller-scope:v1:test-alice-anpclaw-com".to_string(),
        controller_did: "did:human:alice".to_string(),
        runtime_profile_id: "profile_hermes_stale".to_string(),
        runtime_plugin_id: HERMES_RUNTIME_PLUGIN_ID.to_string(),
        display_name: Some("Stale Hermes".to_string()),
        preferred_language: "zh-Hans".to_string(),
        workspace_id: None,
        workspace_root: None,
        workspace_mode: None,
    };
    state
        .upsert_runtime_agent_profile_with_handle(&profile, "alice-hermes")
        .unwrap();
    let stale_home = root.path().join("runtime/hermes/profiles/stale");
    std::fs::create_dir_all(stale_home.join("skills/awiki-outbound-messaging")).unwrap();
    std::fs::write(stale_home.join("SOUL.md"), "old soul").unwrap();
    std::fs::write(stale_home.join("awiki-profile.json"), "{}").unwrap();
    std::fs::write(
        stale_home.join("skills/awiki-outbound-messaging/SKILL.md"),
        "Use awiki-deamon-runtime send --to-handle <handle>. Fallback to awiki-cli if needed.",
    )
    .unwrap();
    state
        .upsert_hermes_profile(&HermesProfileRecord {
            agent_did: profile.agent_did.clone(),
            runtime_profile_id: profile.runtime_profile_id.clone(),
            hermes_profile: "awiki_existing_profile".to_string(),
            hermes_home: stale_home.clone(),
            hermes_version: Some("0.15.1".to_string()),
            awiki_skills_version: "awiki-hermes-skills-v2".to_string(),
            status: "ready".to_string(),
        })
        .unwrap();

    let repaired = repair_hermes_profile_if_needed(&config, &state, &profile, "alice-hermes")
        .unwrap()
        .expect("stale Hermes profile should be repaired");

    assert_eq!(repaired.record.agent_did, profile.agent_did);
    assert_eq!(repaired.record.hermes_profile, "awiki_existing_profile");
    assert_eq!(repaired.record.hermes_home, stale_home);
    assert_eq!(repaired.record.status, "ready");
    assert_eq!(repaired.record.awiki_skills_version, AWIKI_SKILLS_VERSION);
    let stored = state.load_hermes_profile(&profile.agent_did).unwrap();
    assert_eq!(stored.awiki_skills_version, AWIKI_SKILLS_VERSION);
    assert_eq!(stored.hermes_profile, "awiki_existing_profile");
    let outbound_skill = std::fs::read_to_string(
        stored
            .hermes_home
            .join("skills/awiki-outbound-messaging/SKILL.md"),
    )
    .unwrap();
    assert!(outbound_skill.contains("awiki-deamon-runtime send"));
    assert!(outbound_skill.contains("--to <handle-or-did>"));
    assert!(outbound_skill.contains("--group"));
    assert!(outbound_skill.contains("Do not call `awiki-cli`"));
    assert!(!outbound_skill.contains("--to-handle"));
    assert!(!outbound_skill.contains("Fallback to awiki-cli"));
    assert!(stored.hermes_home.join("config.yaml").exists());
}

#[test]
fn hermes_profile_smoke_checks_low_risk_cli_wrapper_request_shape() {
    let request = CliWrapperRequest::rpc_ping("run-token-placeholder").into_rpc_request();

    assert_eq!(request.method, "rpc.ping");
    assert_eq!(request.params, json!({}));
    assert_eq!(request.runtime_rpc_token, "run-token-placeholder");
}
