use std::path::PathBuf;

use anyhow::Result;
use awiki_deamon::agent::{resolve_runtime, ACP_RUNTIME_PLUGIN_ID, DEEPSEEK_HARNESS_ACP_AGENT_ID};
use awiki_deamon::plugins::acp::catalog::deepseek_harness::{
    deepseek_harness_npm_packages, DEEPSEEK_HARNESS_NPM_VERSION, ENTRY,
};
use awiki_deamon::plugins::acp::{initialize_acp_profile, AcpProfileInitRequest};
use awiki_deamon::runtime::RuntimeAgentProfile;
use awiki_deamon::state::{AcpNativeSessionRecord, AcpRuntimeProfileRecord};
use awiki_deamon::{DaemonConfig, DaemonState};
use rusqlite::Connection;
use serde_json::json;

const EXPECTED_DAEMON_SCHEMA_VERSION: i64 = 36;

fn fixture() -> Result<(tempfile::TempDir, DaemonConfig, DaemonState)> {
    let root = tempfile::tempdir()?;
    let config = DaemonConfig::for_state_root(root.path())?;
    config.ensure_state_layout()?;
    let state = DaemonState::open_with_root_key_bytes(&config, [36_u8; 32]);
    state.initialize()?;
    Ok((root, config, state))
}

fn profile(root: PathBuf) -> AcpRuntimeProfileRecord {
    AcpRuntimeProfileRecord {
        runtime_profile_id: "profile_acp_alice".to_string(),
        agent_did: "did:agent:acp-alice".to_string(),
        acp_agent_id: DEEPSEEK_HARNESS_ACP_AGENT_ID.to_string(),
        install_mode: "local".to_string(),
        install_root: root.join("deepseek-harness"),
        entry_command_json: json!({
            "program": "node",
            "args": ["packages/examples/acp-demo/lib/bin.js", "--config", "cordis.yml"]
        }),
        config_path: root.join("runtime/acp/profile_acp_alice/cordis.yml"),
        cwd_root: root.join("runtime/acp/profile_acp_alice/workspace"),
        credential_env_names: vec![
            "DEEPSEEK_API_KEY".to_string(),
            "DEEPSEEK_BASE_URL".to_string(),
        ],
        permission_policy: "allow-once".to_string(),
        installed_version: Some("local".to_string()),
        status: "ready".to_string(),
    }
}

#[test]
fn acp_schema_migrates_v35_and_profile_roundtrips_without_secret_values() -> Result<()> {
    let (root, config, state) = fixture()?;
    let summary = state.initialize()?;
    assert_eq!(summary.schema_version, EXPECTED_DAEMON_SCHEMA_VERSION);

    let record = profile(root.path().to_path_buf());
    state.upsert_acp_runtime_profile(&record)?;
    assert_eq!(
        state.load_acp_runtime_profile(&record.runtime_profile_id)?,
        record
    );

    let database = Connection::open(&config.daemon_db_path)?;
    for table in ["acp_runtime_profile", "acp_native_sessions"] {
        let count: i64 = database.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [table],
            |row| row.get(0),
        )?;
        assert_eq!(count, 1, "missing table {table}");
    }
    let stored_names: String = database.query_row(
        "SELECT credential_env_names_json FROM acp_runtime_profile WHERE runtime_profile_id = ?1",
        [&record.runtime_profile_id],
        |row| row.get(0),
    )?;
    assert!(stored_names.contains("DEEPSEEK_API_KEY"));
    assert!(!stored_names.contains("secret-value"));

    let migrated_root = tempfile::tempdir()?;
    let migrated_config = DaemonConfig::for_state_root(migrated_root.path())?;
    migrated_config.ensure_state_layout()?;
    Connection::open(&migrated_config.daemon_db_path)?.execute_batch(
        r#"
        CREATE TABLE schema_migrations (
            version INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL
        );
        INSERT INTO schema_migrations (version, applied_at)
        VALUES (35, '2026-08-13T00:00:00.000Z');
        "#,
    )?;
    let migrated = DaemonState::open_with_root_key_bytes(&migrated_config, [37_u8; 32]);
    assert_eq!(
        migrated.initialize()?.schema_version,
        EXPECTED_DAEMON_SCHEMA_VERSION
    );
    Ok(())
}

#[test]
fn acp_profile_rejects_secret_values_in_credential_name_list() -> Result<()> {
    let (root, _config, state) = fixture()?;
    let mut record = profile(root.path().to_path_buf());
    record.credential_env_names = vec!["DEEPSEEK_API_KEY=secret-value".to_string()];

    let error = state
        .upsert_acp_runtime_profile(&record)
        .expect_err("credential list must contain names only")
        .to_string();
    assert!(error.contains("credential env name"));
    assert!(!error.contains("secret-value"));
    Ok(())
}

#[test]
fn acp_native_session_epoch_marks_previous_process_sessions_stale() -> Result<()> {
    let (root, _config, state) = fixture()?;
    let profile = profile(root.path().to_path_buf());
    state.upsert_acp_runtime_profile(&profile)?;
    let route_key =
        "acp:did:agent:acp-alice:controller-scope:controller_private:controller:alice:conversation";
    let first = AcpNativeSessionRecord {
        route_key: route_key.to_string(),
        agent_did: profile.agent_did.clone(),
        runtime_profile_id: profile.runtime_profile_id.clone(),
        acp_session_id: "session-epoch-1".to_string(),
        connection_epoch: 1,
        status: "active".to_string(),
        created_at_ms: 100,
        updated_at_ms: 100,
    };
    state.store_acp_native_session(&first)?;
    assert_eq!(
        state
            .load_active_acp_session_by_route(route_key, 1)?
            .expect("active session"),
        first
    );

    assert_eq!(
        state.mark_acp_sessions_stale_before_epoch(&profile.runtime_profile_id, 2)?,
        1
    );
    assert!(state
        .load_active_acp_session_by_route(route_key, 2)?
        .is_none());

    let second = AcpNativeSessionRecord {
        acp_session_id: "session-epoch-2".to_string(),
        connection_epoch: 2,
        created_at_ms: 200,
        updated_at_ms: 200,
        ..first
    };
    state.store_acp_native_session(&second)?;
    assert_eq!(
        state
            .load_active_acp_session_by_route(route_key, 2)?
            .expect("new epoch session")
            .acp_session_id,
        "session-epoch-2"
    );
    Ok(())
}

#[test]
fn acp_runtime_resolution_supports_family_and_deepseek_alias() -> Result<()> {
    let defaulted = resolve_runtime("acp", None)?;
    assert_eq!(defaulted.runtime_plugin_id, ACP_RUNTIME_PLUGIN_ID);
    assert_eq!(
        defaulted.driver_id.as_deref(),
        Some(DEEPSEEK_HARNESS_ACP_AGENT_ID)
    );
    assert!(defaulted.defaulted_driver_id);

    let explicit = resolve_runtime("acp", Some("deepseek-harness"))?;
    assert_eq!(explicit.runtime_plugin_id, ACP_RUNTIME_PLUGIN_ID);
    assert_eq!(
        explicit.driver_id.as_deref(),
        Some(DEEPSEEK_HARNESS_ACP_AGENT_ID)
    );
    assert!(!explicit.defaulted_driver_id);

    let alias = resolve_runtime("deepseek-harness", None)?;
    assert_eq!(alias.runtime_plugin_id, ACP_RUNTIME_PLUGIN_ID);
    assert_eq!(
        alias.driver_id.as_deref(),
        Some(DEEPSEEK_HARNESS_ACP_AGENT_ID)
    );
    assert!(resolve_runtime("deepseek-harness", Some("other-agent")).is_err());
    assert!(resolve_runtime("hermes", Some("deepseek-harness")).is_err());
    Ok(())
}

fn runtime_profile() -> RuntimeAgentProfile {
    RuntimeAgentProfile {
        agent_did: "did:agent:acp-alice".to_string(),
        agent_handle: "alice-acp".to_string(),
        controller_user_id: "user-alice".to_string(),
        controller_full_handle: "alice.anpclaw.com".to_string(),
        controller_scope_key: "controller-scope:v1:alice".to_string(),
        controller_did: "did:human:alice".to_string(),
        runtime_profile_id: "profile_acp_alice".to_string(),
        runtime_plugin_id: ACP_RUNTIME_PLUGIN_ID.to_string(),
        display_name: Some("Alice ACP".to_string()),
        preferred_language: "zh-Hans".to_string(),
        workspace_id: None,
        workspace_root: None,
        workspace_mode: None,
    }
}

fn write_stub_local_harness(checkout: &std::path::Path) -> Result<()> {
    let bin = checkout.join("packages/examples/acp-demo/lib/bin.js");
    std::fs::create_dir_all(bin.parent().expect("bin parent"))?;
    std::fs::write(
        bin,
        r#"import readline from 'node:readline';
const lines = readline.createInterface({ input: process.stdin });
for await (const line of lines) {
  const request = JSON.parse(line);
  if (request.method === 'initialize') {
    process.stdout.write(JSON.stringify({jsonrpc:'2.0',id:request.id,result:{protocolVersion:1,agentInfo:{name:'stub',version:'1'},agentCapabilities:{},authMethods:[]}}) + '\n');
  }
}
"#,
    )?;
    Ok(())
}

#[test]
fn local_deepseek_harness_profile_initializes_private_config_and_credentials() -> Result<()> {
    let (root, config, state) = fixture()?;
    let checkout = root.path().join("deepseek-harness");
    write_stub_local_harness(&checkout)?;
    let runtime_profile = runtime_profile();

    let installed = initialize_acp_profile(
        &config,
        &state,
        &runtime_profile,
        AcpProfileInitRequest {
            acp_agent_id: DEEPSEEK_HARNESS_ACP_AGENT_ID,
            driver_config: Some(&json!({
                "install_mode": "local",
                "local_checkout": checkout,
                "permission_policy": "reject-once"
            })),
            secrets: Some(&json!({
                "DEEPSEEK_API_KEY": "unit-test-secret",
                "DEEPSEEK_BASE_URL": "https://example.invalid"
            })),
        },
    )?;

    assert_eq!(installed.record.install_mode, "local");
    assert!(installed.installed_packages.is_empty());
    assert_eq!(installed.record.permission_policy, "reject-once");
    assert_eq!(installed.record.status, "ready");
    assert_eq!(
        installed.record.entry_command_json["cwd"],
        installed.record.install_root.display().to_string()
    );
    assert!(installed.record.config_path.is_file());
    assert!(installed.dotenv_path.is_file());
    let config_text = std::fs::read_to_string(&installed.record.config_path)?;
    assert!(config_text.contains("@deepseek-ai/dsh-acp-demo"));
    assert!(config_text.contains("@deepseek-ai/dsh-tool-bash"));
    assert!(config_text.contains(installed.record.cwd_root.to_string_lossy().as_ref()));
    assert!(!config_text.contains("subagent"));
    assert!(!config_text.contains("hooks-"));
    let dotenv = std::fs::read_to_string(&installed.dotenv_path)?;
    assert!(dotenv.contains("DEEPSEEK_API_KEY="));
    assert!(dotenv.contains("unit-test-secret"));
    assert_eq!(
        state
            .load_acp_runtime_profile(&runtime_profile.runtime_profile_id)?
            .credential_env_names,
        vec![
            "DEEPSEEK_API_KEY".to_string(),
            "DEEPSEEK_BASE_URL".to_string()
        ]
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&installed.profile_dir)?
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(&installed.dotenv_path)?
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    Ok(())
}

#[test]
fn local_deepseek_harness_requires_prebuilt_acp_demo() -> Result<()> {
    let (root, config, state) = fixture()?;
    let checkout = root.path().join("deepseek-harness");
    std::fs::create_dir_all(&checkout)?;
    let error = initialize_acp_profile(
        &config,
        &state,
        &runtime_profile(),
        AcpProfileInitRequest {
            acp_agent_id: DEEPSEEK_HARNESS_ACP_AGENT_ID,
            driver_config: Some(&json!({
                "install_mode": "local",
                "local_checkout": checkout
            })),
            secrets: Some(&json!({"DEEPSEEK_API_KEY": "unit-test-secret"})),
        },
    )
    .expect_err("missing local build must fail")
    .to_string();
    assert!(error.contains("pnpm install && pnpm run build"));
    assert_eq!(
        state.load_acp_runtime_profile("profile_acp_alice")?.status,
        "failed"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn acp_profile_refuses_to_write_credentials_through_a_symlink() -> Result<()> {
    let (root, config, state) = fixture()?;
    let checkout = root.path().join("deepseek-harness");
    write_stub_local_harness(&checkout)?;
    let profile_dir = root.path().join("runtime/acp/profiles/profile_acp_alice");
    std::fs::create_dir_all(&profile_dir)?;
    let victim = root.path().join("must-not-contain-acp-secret");
    std::fs::write(&victim, "unchanged")?;
    std::os::unix::fs::symlink(&victim, profile_dir.join(".env"))?;

    let error = initialize_acp_profile(
        &config,
        &state,
        &runtime_profile(),
        AcpProfileInitRequest {
            acp_agent_id: DEEPSEEK_HARNESS_ACP_AGENT_ID,
            driver_config: Some(&json!({
                "install_mode": "local",
                "local_checkout": checkout
            })),
            secrets: Some(&json!({"DEEPSEEK_API_KEY": "must-not-be-written"})),
        },
    )
    .expect_err("ACP credential writer must reject a pre-existing symlink");

    assert!(error.to_string().contains("symlink"));
    assert_eq!(std::fs::read_to_string(victim)?, "unchanged");
    Ok(())
}

#[test]
fn deepseek_harness_npm_recipe_is_fully_pinned() -> Result<()> {
    let packages = deepseek_harness_npm_packages(DEEPSEEK_HARNESS_NPM_VERSION);
    assert!(packages
        .iter()
        .any(|package| package.starts_with("@deepseek-ai/dsh-acp-demo@")));
    assert!(packages
        .iter()
        .any(|package| package.starts_with("@deepseek-ai/dsh-tool-fs@")));
    assert!(packages
        .iter()
        .any(|package| package.starts_with("@deepseek-ai/dsh-tool-bash@")));
    assert!(packages
        .iter()
        .all(|package| package.ends_with(DEEPSEEK_HARNESS_NPM_VERSION)));
    Ok(())
}

#[test]
fn deepseek_harness_catalog_owns_its_launch_install_and_environment_spec() {
    assert_eq!(ENTRY.display_name, "DeepSeek Harness");
    assert_eq!(ENTRY.program_name, "node");
    assert_eq!(ENTRY.minimum_program_version, Some((22, 19)));
    assert_eq!(
        ENTRY.npm_entrypoint,
        "node_modules/@deepseek-ai/dsh-acp-demo/lib/bin.js"
    );
    assert!(ENTRY
        .local_package_paths
        .iter()
        .all(|(link, _)| link.starts_with("@deepseek-ai/")));
    assert!(ENTRY.inherited_process_env_names.contains(&"NO_PROXY"));
    assert_eq!(
        ENTRY.launch_cwd(
            std::path::Path::new("/opt/acp"),
            std::path::Path::new("/state/profile"),
            std::path::Path::new("/state/profile/workspace"),
        ),
        std::path::PathBuf::from("/opt/acp")
    );

    let args = ENTRY.launch_args(
        std::path::Path::new("/opt/acp/bin.js"),
        std::path::Path::new("/state/profile/cordis.yml"),
    );
    assert_eq!(
        args,
        vec![
            "/opt/acp/bin.js".to_string(),
            "--config".to_string(),
            "/state/profile/cordis.yml".to_string(),
        ]
    );
}
