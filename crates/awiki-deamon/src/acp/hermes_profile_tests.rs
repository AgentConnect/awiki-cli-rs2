use super::*;
use std::collections::BTreeMap;

#[test]
fn seed_keeps_only_model_configuration_and_static_credentials() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    fs::create_dir(&source).unwrap();
    fs::write(
        source.join("config.yaml"),
        r#"
model:
  default: deepseek-flash
  provider: custom
  base_url: https://example.test/v1
  api_key: static-placeholder
  auth_file: /shared/oauth.json
providers:
  custom:
    key_env: CUSTOM_API_KEY
    base_url: https://example.test/v1
    refresh_token: do-not-copy
memory:
  provider: external
  path: /shared/memory
mcp_servers:
  unsafe: {command: run-me}
hooks: [run-me]
"#,
    )
    .unwrap();
    fs::write(source.join(".env"), "DEEPSEEK_API_KEY='fixture-only'\nCUSTOM_API_KEY=custom-placeholder\nHERMES_HOME=/wrong\nMEM0_API_KEY=do-not-copy\nOPENAI_REFRESH_TOKEN=do-not-copy\n").unwrap();
    for name in ["auth.json", "MEMORY.md", "USER.md", "state.db"] {
        fs::write(source.join(name), "old data").unwrap();
    }
    let home = root.path().join("profiles/private");
    initialize(
        &home,
        Some(&source),
        &BTreeMap::from([
            ("OPENAI_API_KEY".into(), "ambient-placeholder".into()),
            ("DEEPSEEK_API_KEY".into(), "overridden".into()),
        ]),
    )
    .unwrap();
    let config: Value =
        serde_json::from_slice(&fs::read(home.join("config.yaml")).unwrap()).unwrap();
    assert_eq!(config["model"]["default"], "deepseek-flash");
    assert_eq!(config["providers"]["custom"]["key_env"], "CUSTOM_API_KEY");
    assert!(
        config["memory"].is_null() && config["hooks"].is_null() && config["mcp_servers"].is_null()
    );
    assert!(
        config["model"]["auth_file"].is_null()
            && config["providers"]["custom"]["refresh_token"].is_null()
    );
    let env = fs::read_to_string(home.join(".env")).unwrap();
    assert!(
        env.contains("fixture-only")
            && env.contains("custom-placeholder")
            && env.contains("ambient-placeholder")
    );
    assert!(
        !env.contains("do-not-copy") && !env.contains("overridden") && !env.contains("HERMES_HOME")
    );
    assert_eq!(fs::read_dir(&home).unwrap().count(), 3);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&home).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(home.join(".env"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[test]
fn initialization_is_atomic_and_never_overwrites_profile_changes() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("profiles/session");
    std::thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| initialize(&home, None, &BTreeMap::new()).unwrap());
        }
    });
    fs::write(home.join("config.yaml"), "user edits").unwrap();
    fs::write(home.join("MEMORY.md"), "session-owned memory").unwrap();
    initialize(
        &home,
        Some(Path::new("/nonexistent/seed")),
        &BTreeMap::new(),
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(home.join("config.yaml")).unwrap(),
        "user edits"
    );
    assert_eq!(
        fs::read_to_string(home.join("MEMORY.md")).unwrap(),
        "session-owned memory"
    );
    assert_eq!(fs::read_dir(home.parent().unwrap()).unwrap().count(), 1);
}

#[test]
fn rejects_bad_seed_without_leaking_values_or_leaving_half_initialized_home() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    fs::create_dir(&source).unwrap();
    let home = root.path().join("profiles/session");
    fs::write(source.join("config.yaml"), "api_key: [secret-never-print").unwrap();
    let error = initialize(&home, Some(&source), &BTreeMap::new()).unwrap_err();
    assert_eq!(error.to_string(), "hermes_seed_invalid_config");
    assert!(!home.exists());
    fs::write(source.join("config.yaml"), "{}").unwrap();
    fs::write(
        source.join(".env"),
        "DEEPSEEK_API_KEY='multiline\nsecret'\n",
    )
    .unwrap();
    assert!(initialize(&home, Some(&source), &BTreeMap::new()).is_err());
    assert!(!home.exists());
    fs::create_dir(&home).unwrap();
    assert!(initialize(&home, Some(&source), &BTreeMap::new()).is_err());
}

#[test]
fn all_canonical_scopes_and_agents_receive_distinct_native_homes() {
    let root = tempfile::tempdir().unwrap();
    let config = crate::DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open_with_root_key_bytes(&config, [9; 32]);
    let source = root.path().join("empty-source");
    fs::create_dir(&source).unwrap();
    let mut cli = CliRuntimeProfileRecord::for_driver("p", "hermes").unwrap();
    cli.config_home = Some(source);
    let mut homes = BTreeSet::new();
    for (agent, scope) in [
        ("a", "private"),
        ("a", "group:1"),
        ("a", "group:2"),
        ("a", "background"),
        ("b", "private"),
    ] {
        let session = super::super::store::Session::for_scope(
            agent,
            "owner",
            scope.into(),
            Some(scope.into()),
            scope.starts_with("group"),
        );
        let profile = for_session(&state, &cli, &session.key).unwrap();
        assert!(homes.insert(profile.config_home.clone()));
        assert_eq!(
            for_session(&state, &cli, &session.key).unwrap().config_home,
            profile.config_home
        );
    }
}

#[test]
fn launch_does_not_put_credentials_in_arguments_or_inherit_behavioral_overrides() {
    use agent_client_protocol::AcpAgentConfig;
    assert!(
        !passthrough("HERMES_HOME")
            && !passthrough("HERMES_PROFILE")
            && !passthrough("OPENAI_API_KEY")
            && !passthrough("HONCHO_API_KEY")
    );
    assert!(passthrough("AWIKI_RUNTIME_RPC_TOKEN") && passthrough("HTTPS_PROXY"));
    let base = AcpAgentConfig::new("/native/hermes")
        .arg("acp")
        .env("HERMES_HOME", "/isolated/home");
    let launch = isolated_launch(base).unwrap();
    assert_eq!(launch.command(), Path::new("/usr/bin/env"));
    assert_eq!(
        &launch.arguments()[launch.arguments().len() - 2..],
        &["/native/hermes", "acp"]
    );
    assert_eq!(launch.environment()["HERMES_HOME"], "/isolated/home");
    assert!(isolated_launch(AcpAgentConfig::new("hermes")).is_err());
}

#[cfg(unix)]
#[test]
fn existing_profile_must_not_be_a_symlink_to_another_scope() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("target");
    initialize(&target, None, &BTreeMap::new()).unwrap();
    let home = root.path().join("linked");
    std::os::unix::fs::symlink(&target, &home).unwrap();
    assert!(initialize(&home, None, &BTreeMap::new()).is_err());
}
