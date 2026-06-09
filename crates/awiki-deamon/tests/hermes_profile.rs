use std::sync::{Arc, Mutex};

use awiki_deamon::agent::AgentKind;
use awiki_deamon::cli_wrapper::CliWrapperRequest;
use awiki_deamon::commands::{
    handle_agent_payload_message, setup_daemon_agent, AgentCommandOutcome,
    IncomingAgentPayloadMessage, RuntimeAgentCreateOutcome,
};
use awiki_deamon::outbox::MemoryRuntimeOutbox;
use awiki_deamon::plugins::hermes::{AWIKI_SKILLS_VERSION, HERMES_RUNTIME_PLUGIN_ID};
use awiki_deamon::registration::{
    AgentRegistrationClient, AgentRegistrationExchangeRequest, AgentRegistrationExchangeResult,
    RegistrationToken,
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
        let lock = ENV_LOCK.lock().unwrap();
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
            controller_did: request.controller_did,
            handle: request.handle,
            status: "registered".to_string(),
        })
    }
}

fn fixture() -> (tempfile::TempDir, DaemonConfig, DaemonState) {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open(&config).unwrap();
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
    assert_eq!(summary.schema_version, 18);
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
                    "registration_token": "tok_runtime_secret_value"
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
    assert!(runtime_model_config.contains("provider: custom"));
    assert!(runtime_model_config.contains("default: gpt-5.2"));
    assert!(profile_json.contains("\"run_capability_token_persisted\": false"));
    assert!(profile_json.contains("library:awiki_deamon::cli_wrapper"));
    assert!(profile_json.contains("process wrapper wired in Step 07"));
    assert!(outbound_skill.contains("awiki-deamon-runtime send"));
    assert!(outbound_skill.contains("--to-handle"));
    assert!(outbound_skill.contains("--group"));
    assert!(outbound_skill.contains("--file"));
    assert!(outbound_skill.contains("--display-filename"));
    assert!(outbound_skill.contains("--mime-type"));
    assert!(outbound_skill.contains("same outbound message"));
    assert!(outbound_skill.contains("ordinary Awiki messages"));
    assert!(outbound_skill.contains("Do not use it for your ordinary final answer"));
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
fn hermes_profile_smoke_checks_low_risk_cli_wrapper_request_shape() {
    let request = CliWrapperRequest::rpc_ping("run-token-placeholder").into_rpc_request();

    assert_eq!(request.method, "rpc.ping");
    assert_eq!(request.params, json!({}));
    assert_eq!(request.runtime_rpc_token, "run-token-placeholder");
}
