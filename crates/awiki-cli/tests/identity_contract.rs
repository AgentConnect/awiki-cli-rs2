use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn identity_anp_service_helpers_match_go_contract() {
    assert_eq!(
        awiki_cli::identity::default_anp_service_endpoint(" awiki.ai "),
        "https://awiki.ai/anp-im/rpc"
    );
    assert_eq!(
        awiki_cli::identity::default_anp_service_did(" awiki.ai "),
        "did:wba:awiki.ai"
    );

    awiki_cli::identity::did::validate_anp_service_endpoint("https://awiki.ai/anp-im/rpc")
        .expect("https endpoint");
    awiki_cli::identity::did::validate_anp_service_endpoint("http://api.example/rpc")
        .expect("http endpoint");
    assert_error_contains(
        awiki_cli::identity::did::validate_anp_service_endpoint(" ").expect_err("missing endpoint"),
        "invalid input: anp_service_endpoint is required",
    );
    assert_error_contains(
        awiki_cli::identity::did::validate_anp_service_endpoint("ftp://awiki.ai/rpc")
            .expect_err("bad scheme"),
        "invalid input: anp_service_endpoint must use http or https",
    );
    assert_error_contains(
        awiki_cli::identity::did::validate_anp_service_endpoint("https:///rpc")
            .expect_err("missing hostname"),
        "invalid input: anp_service_endpoint must include a hostname",
    );
    assert_error_contains(
        awiki_cli::identity::did::validate_anp_service_endpoint("https://localhost/anp-im/rpc")
            .expect_err("localhost"),
        "invalid input: anp_service_endpoint must not use localhost",
    );
    assert_error_contains(
        awiki_cli::identity::did::validate_anp_service_endpoint("http://127.0.0.1:9898/rpc")
            .expect_err("loopback"),
        "invalid input: anp_service_endpoint must not use a loopback address",
    );

    awiki_cli::identity::did::validate_anp_service_did("did:wba:awiki.ai")
        .expect("bare service did");
    assert_error_contains(
        awiki_cli::identity::did::validate_anp_service_did(" ").expect_err("missing service did"),
        "invalid input: anp_service_did is required",
    );
    assert_error_contains(
        awiki_cli::identity::did::validate_anp_service_did("did:key:z6Mkwrong")
            .expect_err("wrong did method"),
        "invalid input: anp_service_did must use did:wba",
    );
    assert_error_contains(
        awiki_cli::identity::did::validate_anp_service_did("did:wba:awiki.ai#message")
            .expect_err("fragment"),
        "invalid input: anp_service_did must not include a fragment",
    );
    assert_error_contains(
        awiki_cli::identity::did::validate_anp_service_did("did:wba:").expect_err("missing domain"),
        "invalid input: anp_service_did must include a domain",
    );
    assert_error_contains(
        awiki_cli::identity::did::validate_anp_service_did(
            "did:wba:awiki.ai:services:message:e1_local",
        )
        .expect_err("non-bare service did"),
        "invalid input: anp_service_did must be a bare-domain did:wba DID",
    );

    let service = awiki_cli::identity::did::build_agent_anp_message_service(
        " https://awiki.ai/anp-im/rpc ",
        " did:wba:awiki.ai ",
    )
    .expect("message service");
    assert_eq!(
        service,
        json!({
            "id": "#message",
            "type": "ANPMessageService",
            "serviceEndpoint": "https://awiki.ai/anp-im/rpc",
            "serviceDid": "did:wba:awiki.ai",
            "profiles": [
                "anp.core.binding.v1",
                "anp.direct.base.v1",
                "anp.attachment.v1",
            ],
            "securityProfiles": ["transport-protected"],
        })
    );
}

#[test]
fn identity_handle_input_helpers_match_go_contract() {
    let bare = awiki_cli::identity::normalize_handle_input("Alice", "Tenant.Example.")
        .expect("normalize bare handle");
    assert_eq!(bare.local_part, "alice");
    assert_eq!(bare.full_handle, "alice.tenant.example");
    assert_eq!(bare.effective_domain, "tenant.example");
    assert!(!bare.explicit_domain);

    let full = awiki_cli::identity::normalize_handle_input("Alice.Other.Example", "tenant.example")
        .expect("normalize full handle");
    assert_eq!(full.local_part, "alice");
    assert_eq!(full.full_handle, "alice.other.example");
    assert_eq!(full.effective_domain, "other.example");
    assert!(full.explicit_domain);

    let wba =
        awiki_cli::identity::normalize_handle_input("wba://Alice.Other.Example", "tenant.example")
            .expect("normalize wba handle");
    assert_eq!(wba.full_handle, "alice.other.example");
    assert!(wba.explicit_domain);

    let did_error =
        awiki_cli::identity::normalize_handle_input("did:wba:tenant.example:user:alice:e1", "")
            .expect_err("did input must be rejected");
    assert!(did_error
        .to_string()
        .contains("did values are not supported in handle input"));
    assert_eq!(
        awiki_cli::identity::complete_bare_handle("Alice", "Tenant.Example."),
        "alice.tenant.example"
    );
    assert_eq!(
        awiki_cli::identity::complete_bare_handle("alice.other.example", "tenant.example"),
        "alice.other.example"
    );
    assert_eq!(
        awiki_cli::identity::complete_bare_handle(" Alice.Other.Example ", "tenant.example"),
        "Alice.Other.Example"
    );
    assert_eq!(
        awiki_cli::identity::complete_bare_handle("wba://Alice", "tenant.example"),
        "alice.tenant.example"
    );
    assert_eq!(
        awiki_cli::identity::complete_bare_handle("wba://Alice.Other.Example", "tenant.example"),
        "wba://Alice.Other.Example"
    );
    assert_eq!(
        awiki_cli::identity::complete_bare_handle("Alice", ""),
        "Alice"
    );
    assert_eq!(
        awiki_cli::identity::complete_bare_handle("did:wba:tenant.example:user:alice:e1", "x"),
        "did:wba:tenant.example:user:alice:e1"
    );
    assert_eq!(
        awiki_cli::identity::derive_full_handle_from_did(
            "Alice",
            "did:wba:Tenant.Example:profile:e1_alice",
        ),
        "alice.tenant.example"
    );
    assert_eq!(
        awiki_cli::identity::derive_full_handle_from_did(
            "Alice",
            "did:wba:tenant.example:user:e1_alice",
        ),
        ""
    );
}

#[test]
fn identity_load_backfills_full_handle_from_handle_did_like_go() {
    let workspace = TempDir::new().expect("workspace");
    let manager = identity_manager(workspace.path());
    let record = manager
        .save(awiki_cli::identity::types::SaveInput {
            identity_name: "alice".to_string(),
            did: "did:wba:tenant.example:alice:e1_alice".to_string(),
            unique_id: "e1_alice".to_string(),
            handle: "alice".to_string(),
            ..Default::default()
        })
        .expect("save identity");
    let paths = manager.build_paths(&record.dir_name);

    let mut payload = read_json(&paths.identity_path);
    payload
        .as_object_mut()
        .expect("identity payload object")
        .remove("full_handle");
    std::fs::write(
        &paths.identity_path,
        serde_json::to_vec_pretty(&payload).unwrap(),
    )
    .unwrap();

    let mut index = manager.load_index().expect("load index");
    index
        .credentials
        .get_mut("alice")
        .expect("alice index entry")
        .full_handle
        .clear();
    manager.save_index(index).expect("save index");

    let loaded = manager.load("alice").expect("load identity");
    assert_eq!(loaded.full_handle, "alice.tenant.example");
    assert_eq!(
        read_json(&paths.identity_path)["full_handle"],
        "alice.tenant.example"
    );
    assert_eq!(
        manager
            .load_index()
            .expect("load updated index")
            .credentials["alice"]
            .full_handle,
        "alice.tenant.example"
    );
}

#[test]
fn identity_load_does_not_backfill_full_handle_for_user_did_like_go() {
    let workspace = TempDir::new().expect("workspace");
    let manager = identity_manager(workspace.path());
    let record = manager
        .save(awiki_cli::identity::types::SaveInput {
            identity_name: "alice".to_string(),
            did: "did:wba:tenant.example:user:e1_alice".to_string(),
            unique_id: "e1_alice".to_string(),
            handle: "alice".to_string(),
            ..Default::default()
        })
        .expect("save identity");
    let paths = manager.build_paths(&record.dir_name);

    let payload = read_json(&paths.identity_path);
    assert!(payload.get("full_handle").is_none());

    let loaded = manager.load("alice").expect("load identity");
    assert_eq!(loaded.handle, "alice");
    assert_eq!(loaded.full_handle, "");
    assert!(read_json(&paths.identity_path).get("full_handle").is_none());
    assert_eq!(
        manager
            .load_index()
            .expect("load updated index")
            .credentials["alice"]
            .full_handle,
        ""
    );
}

#[test]
fn identity_create_list_current_use_and_status_match_local_contract() {
    let workspace = TempDir::new().expect("workspace");

    let alice = awiki_cmd(
        &[
            "--migration",
            "id",
            "create",
            "--name",
            "Alice Example",
            "--identity",
            "alice",
        ],
        workspace.path(),
    );
    assert_success(&alice);
    let alice = success_json(&alice);
    assert_eq!(alice["data"]["action"], "create_identity");
    assert_eq!(alice["data"]["identity"]["identity_name"], "alice");
    assert_eq!(alice["data"]["identity"]["is_default"], true);
    assert_eq!(alice["data"]["identity"]["has_key1_private"], true);
    assert!(alice["data"]["identity"].get("user_id").is_none());

    let bob = awiki_cmd(
        &[
            "--migration",
            "id",
            "create",
            "--name",
            "Bob Example",
            "--identity",
            "bob",
        ],
        workspace.path(),
    );
    assert_success(&bob);

    let current = success_json(&awiki_cmd(&["id", "current"], workspace.path()));
    assert_eq!(current["data"]["identity"]["identity_name"], "alice");

    let use_bob = success_json(&awiki_cmd(&["id", "use", "bob"], workspace.path()));
    assert_eq!(use_bob["data"]["action"], "set_default_identity");
    assert_eq!(use_bob["data"]["identity"]["identity_name"], "bob");

    let list = success_json(&awiki_cmd(&["id", "list"], workspace.path()));
    let names: Vec<_> = list["data"]["identities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["identity_name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["alice", "bob"]);
    assert_eq!(list["data"]["default_identity"]["identity_name"], "bob");
    assert!(list["data"]["legacy_scan"].is_object());

    let status = success_json(&awiki_cmd(&["id", "status"], workspace.path()));
    assert_eq!(status["data"]["active_identity"]["identity_name"], "bob");
    assert_eq!(status["data"]["identity_count"], 2);
}

#[test]
fn identity_status_migrates_legacy_config_json_before_store_read_like_go() {
    let workspace = TempDir::new().expect("workspace");
    let workspace_home = workspace.path().join(".awiki-cli");
    std::fs::create_dir_all(&workspace_home).expect("create workspace home");
    let legacy_payload = json!({
        "schema_version": 1,
        "services": {
            "service_base_url": "https://legacy-id-status.example",
            "did_domain": "legacy-id-status.example",
        },
        "runtime": {
            "mode": "http",
        },
    });
    let (legacy_config, legacy_text) = write_legacy_config_json(&workspace_home, legacy_payload);

    let status = success_json(&awiki_cmd(&["id", "status"], workspace.path()));
    assert_eq!(status["summary"], "No default identity is configured yet");
    assert_eq!(status["data"]["active_identity"], Value::Null);
    assert_eq!(status["data"]["identity_count"], 0);

    assert!(
        !legacy_config.exists(),
        "legacy config.json should be removed after workspace upgrade"
    );
    assert_migrated_config(
        &workspace_home,
        "https://legacy-id-status.example",
        "legacy-id-status.example",
    );

    assert_workspace_upgrade_meta(&workspace_home, &legacy_text);
    assert_no_runtime_state(&workspace_home, "id status");
}

#[test]
fn identity_create_validates_name_before_workspace_upgrade_like_go() {
    let workspace = TempDir::new().expect("workspace");
    let workspace_home = workspace.path().join(".awiki-cli");
    std::fs::create_dir_all(&workspace_home).expect("create workspace home");
    let (legacy_config, _) = write_legacy_config_json(
        &workspace_home,
        json!({
            "schema_version": 1,
            "services": {
                "service_base_url": "https://legacy-id-create-invalid.example",
                "did_domain": "legacy-id-create-invalid.example",
            },
            "runtime": {
                "mode": "http",
            },
        }),
    );

    let create = awiki_cmd(&["--migration", "id", "create"], workspace.path());
    assert_code(&create, 2);
    let create = error_json(&create);
    assert_eq!(create["error"]["code"], "invalid_argument");
    assert!(create["error"]["message"]
        .as_str()
        .unwrap()
        .contains("id create requires --name"));
    assert!(
        legacy_config.exists(),
        "missing --name must not trigger workspace upgrade before validation"
    );
    assert!(
        !workspace_home.join("config.yaml").exists(),
        "missing --name must not write migrated config"
    );
    assert!(
        !workspace_home.join("upgrade").join("meta.json").exists(),
        "missing --name must not write upgrade metadata"
    );
}

#[test]
fn identity_create_migrates_legacy_config_json_before_create_like_go() {
    let workspace = TempDir::new().expect("workspace");
    let workspace_home = workspace.path().join(".awiki-cli");
    std::fs::create_dir_all(&workspace_home).expect("create workspace home");
    let legacy_payload = json!({
        "schema_version": 1,
        "services": {
            "service_base_url": "https://legacy-id-create.example",
            "did_domain": "legacy-id-create.example",
        },
        "runtime": {
            "mode": "http",
        },
    });
    let (legacy_config, legacy_text) = write_legacy_config_json(&workspace_home, legacy_payload);

    let create = success_json(&awiki_cmd(
        &[
            "--migration",
            "id",
            "create",
            "--name",
            "Legacy Create",
            "--identity",
            "legacy-create",
        ],
        workspace.path(),
    ));
    assert_eq!(create["data"]["identity"]["identity_name"], "legacy-create");
    assert!(create["data"]["identity"]["did"]
        .as_str()
        .unwrap()
        .starts_with("did:wba:legacy-id-create.example:user:"));
    let current = success_json(&awiki_cmd(&["id", "current"], workspace.path()));
    assert_eq!(
        current["data"]["identity"]["identity_name"],
        "legacy-create"
    );

    assert!(
        !legacy_config.exists(),
        "legacy config.json should be removed before identity creation"
    );
    assert_migrated_config(
        &workspace_home,
        "https://legacy-id-create.example",
        "legacy-id-create.example",
    );

    assert_workspace_upgrade_meta(&workspace_home, &legacy_text);
    assert_no_runtime_state(&workspace_home, "id create");
}

#[test]
fn identity_use_validates_argument_before_workspace_upgrade_like_go() {
    let workspace = TempDir::new().expect("workspace");
    let workspace_home = workspace.path().join(".awiki-cli");
    std::fs::create_dir_all(&workspace_home).expect("create workspace home");
    let (legacy_config, _) = write_legacy_config_json(
        &workspace_home,
        json!({
            "schema_version": 1,
            "services": {
                "service_base_url": "https://legacy-id-use-invalid.example",
                "did_domain": "legacy-id-use-invalid.example",
            },
            "runtime": {
                "mode": "http",
            },
        }),
    );

    let use_result = awiki_cmd(&["id", "use"], workspace.path());
    assert_code(&use_result, 2);
    let use_result = error_json(&use_result);
    assert_eq!(use_result["error"]["code"], "invalid_argument");
    assert!(use_result["error"]["message"]
        .as_str()
        .unwrap()
        .contains("id use requires exactly one identity name"));
    assert!(
        legacy_config.exists(),
        "missing identity arg must not trigger workspace upgrade before validation"
    );
    assert!(
        !workspace_home.join("config.yaml").exists(),
        "missing identity arg must not write migrated config"
    );
    assert!(
        !workspace_home.join("upgrade").join("meta.json").exists(),
        "missing identity arg must not write upgrade metadata"
    );
}

#[test]
fn identity_use_migrates_legacy_config_json_before_switch_like_go() {
    let workspace = TempDir::new().expect("workspace");
    let workspace_home = workspace.path().join(".awiki-cli");
    std::fs::create_dir_all(&workspace_home).expect("create workspace home");
    let manager = identity_manager(&workspace_home);
    let alice = manager
        .save(awiki_cli::identity::types::SaveInput {
            identity_name: "alice".to_string(),
            did: "did:wba:legacy-id-use.example:user:e1_alice".to_string(),
            unique_id: "e1_alice".to_string(),
            display_name: "Alice".to_string(),
            ..Default::default()
        })
        .expect("save alice");
    let bob = manager
        .save(awiki_cli::identity::types::SaveInput {
            identity_name: "bob".to_string(),
            did: "did:wba:legacy-id-use.example:user:e1_bob".to_string(),
            unique_id: "e1_bob".to_string(),
            display_name: "Bob".to_string(),
            ..Default::default()
        })
        .expect("save bob");
    assert_eq!(alice.identity_name, "alice");
    assert_eq!(bob.identity_name, "bob");
    let legacy_payload = json!({
        "schema_version": 1,
        "identity": {
            "active": "alice",
        },
        "services": {
            "service_base_url": "https://legacy-id-use.example",
            "did_domain": "legacy-id-use.example",
        },
        "runtime": {
            "mode": "http",
        },
    });
    let (legacy_config, legacy_text) = write_legacy_config_json(&workspace_home, legacy_payload);

    let use_result = success_json(&awiki_cmd(&["id", "use", "bob"], workspace.path()));
    assert_eq!(use_result["data"]["action"], "set_default_identity");
    assert_eq!(use_result["data"]["identity"]["identity_name"], "bob");
    let current = success_json(&awiki_cmd(&["id", "current"], workspace.path()));
    assert_eq!(current["data"]["identity"]["identity_name"], "bob");

    assert!(
        !legacy_config.exists(),
        "legacy config.json should be removed before identity switch"
    );
    let config_text = assert_migrated_config(
        &workspace_home,
        "https://legacy-id-use.example",
        "legacy-id-use.example",
    );
    assert!(
        config_text.contains("  active: alice\n"),
        "id use should leave migrated config active identity unchanged like Go, got {config_text:?}"
    );

    assert_workspace_upgrade_meta(&workspace_home, &legacy_text);
    assert_no_runtime_state(&workspace_home, "id use");
}

#[test]
fn identity_resolve_migrates_legacy_config_json_before_target_validation_like_go() {
    let workspace = TempDir::new().expect("workspace");
    let workspace_home = workspace.path().join(".awiki-cli");
    std::fs::create_dir_all(&workspace_home).expect("create workspace home");
    let legacy_payload = json!({
        "schema_version": 1,
        "services": {
            "service_base_url": "https://legacy-id-resolve.example",
            "did_domain": "legacy-id-resolve.example",
        },
        "runtime": {
            "mode": "http",
        },
    });
    let (legacy_config, legacy_text) = write_legacy_config_json(&workspace_home, legacy_payload);

    let resolve = awiki_cmd(&["id", "resolve"], workspace.path());
    assert_code(&resolve, 2);
    let resolve = error_json(&resolve);
    assert_eq!(resolve["error"]["code"], "invalid_argument");
    assert!(resolve["error"]["message"]
        .as_str()
        .unwrap()
        .contains("exactly one of handle or did is required"));

    assert!(
        !legacy_config.exists(),
        "legacy config.json should be removed before resolve target validation"
    );
    assert_migrated_config(
        &workspace_home,
        "https://legacy-id-resolve.example",
        "legacy-id-resolve.example",
    );
    assert_workspace_upgrade_meta(&workspace_home, &legacy_text);
    assert_no_runtime_state(&workspace_home, "id resolve validation");
}

#[test]
fn identity_profile_get_migrates_legacy_config_json_before_self_identity_boundary_like_go() {
    assert_identity_boundary_after_legacy_config_upgrade(
        &["id", "profile", "get"],
        "profile",
        "id profile get self boundary",
    );
}

#[test]
fn identity_bind_migrates_legacy_config_json_before_active_identity_boundary_like_go() {
    assert_identity_boundary_after_legacy_config_upgrade(
        &["id", "bind", "--phone", "13800138000"],
        "bind",
        "id bind identity boundary",
    );
}

#[test]
fn identity_refresh_token_migrates_legacy_config_json_before_active_identity_boundary_like_go() {
    assert_identity_boundary_after_legacy_config_upgrade(
        &["id", "refresh-token"],
        "refresh",
        "id refresh-token identity boundary",
    );
}

fn assert_identity_boundary_after_legacy_config_upgrade(args: &[&str], label: &str, state: &str) {
    let workspace = TempDir::new().expect("workspace");
    let workspace_home = workspace.path().join(".awiki-cli");
    std::fs::create_dir_all(&workspace_home).expect("create workspace home");
    let service_base_url = format!("https://legacy-id-{label}.example");
    let did_domain = format!("legacy-id-{label}.example");
    let (legacy_config, legacy_text) = write_legacy_config_json(
        &workspace_home,
        json!({
            "schema_version": 1,
            "services": {
                "service_base_url": service_base_url,
                "did_domain": did_domain,
            },
            "runtime": {
                "mode": "http",
            },
        }),
    );

    let result = awiki_cmd(args, workspace.path());
    assert_code(&result, 5);
    let result = error_json(&result);
    assert_eq!(result["error"]["code"], "not_found");
    assert!(result["error"]["message"]
        .as_str()
        .unwrap()
        .contains("identity not found: no active identity is configured"));

    assert!(!legacy_config.exists());
    assert_migrated_config(&workspace_home, &service_base_url, &did_domain);
    assert_workspace_upgrade_meta(&workspace_home, &legacy_text);
    assert_no_runtime_state(&workspace_home, state);
}

#[test]
fn identity_dry_run_and_validation_contracts_match_go() {
    let workspace = TempDir::new().expect("workspace");
    let create = success_json(&awiki_cmd(
        &[
            "--migration",
            "id",
            "create",
            "--dry-run",
            "--name",
            "Dry Run User",
            "--identity",
            "dry-run-user",
        ],
        workspace.path(),
    ));
    assert_eq!(create["meta"]["dry_run"], true);
    assert_eq!(create["data"]["plan"]["action"], "create_identity");
    assert_eq!(create["data"]["plan"]["identity_name"], "dry-run-user");

    let use_plan = success_json(&awiki_cmd(
        &["id", "use", "--dry-run", "dry-run-user"],
        workspace.path(),
    ));
    assert_eq!(use_plan["data"]["plan"]["action"], "set_default_identity");

    let missing = awiki_cmd(&["id", "use"], workspace.path());
    assert_code(&missing, 2);
    let missing = error_json(&missing);
    assert_eq!(missing["error"]["code"], "invalid_argument");
    assert!(missing["error"]["message"]
        .as_str()
        .unwrap()
        .contains("id use requires exactly one identity name"));

    let refresh = success_json(&awiki_cmd(
        &["--identity", "alice", "id", "refresh-token", "--dry-run"],
        workspace.path(),
    ));
    assert_eq!(refresh["data"]["plan"]["action"], "refresh_token");
    assert_eq!(refresh["data"]["plan"]["identity_name"], "alice");
    assert_eq!(
        refresh["data"]["plan"]["auth_flow"],
        "did_auth_get_me_without_stored_bearer"
    );
    assert!(refresh["data"]["plan"]["remote_calls"]
        .as_array()
        .unwrap()
        .contains(&json!("did-auth.get_me")));
    assert!(refresh["data"]["plan"]["local_writes"]
        .as_array()
        .unwrap()
        .contains(&json!("auth.json")));

    let recover = success_json(&awiki_cmd(
        &[
            "--identity",
            "ignored-name",
            "id",
            "recover",
            "--dry-run",
            "--handle",
            "Alice",
            "--phone",
            "13800138000",
            "--otp",
            "123456",
        ],
        workspace.path(),
    ));
    assert_eq!(recover["data"]["plan"]["action"], "recover_handle");
    assert_eq!(recover["data"]["plan"]["target_handle"], "alice.awiki.ai");
    assert_eq!(recover["data"]["plan"]["identity_name"], "alice");
    assert_eq!(recover["data"]["plan"]["final_identity_name"], "alice");
    assert_eq!(
        recover["data"]["plan"]["temp_identity_name"],
        "alice-recover-tmp"
    );
    assert_eq!(recover["data"]["plan"]["same_handle_candidates"], json!([]));
    assert_eq!(recover["data"]["plan"]["excluded_identities"], json!([]));
    assert!(recover["data"]["plan"]["backup_path"]
        .as_str()
        .unwrap()
        .contains(".legacy-backup/recover-handle/<timestamp>-alice.awiki.ai"));
    assert_eq!(
        recover["data"]["plan"]["remote_calls"],
        json!(["did-auth.recover_handle"])
    );
    assert!(recover["data"]["plan"]["local_writes"]
        .as_array()
        .unwrap()
        .contains(&json!("sqlite.recover_handle_merge")));
    assert!(recover["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|warning| warning
            .as_str()
            .unwrap()
            .contains("--identity flag is ignored")));

    let recover_otp = success_json(&awiki_cmd(
        &[
            "id",
            "recover",
            "--dry-run",
            "--handle",
            "Alice",
            "--phone",
            "13800138000",
        ],
        workspace.path(),
    ));
    assert_eq!(recover_otp["data"]["plan"]["action"], "send_recover_otp");
    assert_eq!(
        recover_otp["data"]["plan"]["remote_calls"],
        json!(["handle.send_otp"])
    );
    assert_eq!(recover_otp["data"]["plan"]["local_writes"], Value::Null);
    assert_eq!(recover_otp["data"]["plan"]["backup_path"], "");

    let missing_recover_phone = awiki_cmd(
        &["id", "recover", "--dry-run", "--handle", "Alice"],
        workspace.path(),
    );
    assert_code(&missing_recover_phone, 2);
    let missing_recover_phone = error_json(&missing_recover_phone);
    assert_eq!(missing_recover_phone["error"]["code"], "invalid_argument");
    assert!(missing_recover_phone["error"]["message"]
        .as_str()
        .unwrap()
        .contains("id recover requires --phone."));

    let schema = success_json(&awiki_cmd(
        &["schema", "id", "replace-did"],
        workspace.path(),
    ));
    assert_eq!(schema["data"]["command"]["hidden"], true);
    assert_eq!(schema["data"]["command"]["side_effect"], true);
    assert_eq!(
        schema["data"]["command"]["cutover"]["status"],
        "diagnostic_only"
    );
    assert_eq!(
        schema["data"]["command"]["cutover"]["default_surface"],
        false
    );
    assert!(schema["data"]["command"]["short"]
        .as_str()
        .unwrap()
        .contains("Danger"));

    let generated = awiki_cli::identity::generate_identity(
        "awiki.ai",
        "https://awiki.ai/anp-im/rpc",
        "did:wba:awiki.ai",
    )
    .expect("replace-did fixture identity");
    identity_manager(&workspace.path().join(".awiki-cli"))
        .save(awiki_cli::identity::types::SaveInput {
            identity_name: "alice".to_string(),
            did: "did:wba:awiki.ai:alice:e1_alice".to_string(),
            unique_id: "e1_alice".to_string(),
            display_name: "Alice".to_string(),
            handle: "alice".to_string(),
            full_handle: "alice.awiki.ai".to_string(),
            jwt_token: "jwt-alice".to_string(),
            did_document: Some(generated.did_document),
            key1_private_pem: generated.key1_private_pem,
            key1_public_pem: generated.key1_public_pem,
            e2ee_signing_private_pem: generated.e2ee_signing_private_pem,
            e2ee_agreement_private_pem: generated.e2ee_agreement_private_pem,
            ..Default::default()
        })
        .expect("save replace-did fixture identity");

    let replace = success_json(&awiki_cmd(
        &[
            "--diagnostic",
            "--identity",
            "alice",
            "id",
            "replace-did",
            "--dry-run",
            "--is-public=false",
            "--role",
            "",
            "--endpoint-url",
            "https://example.com/agent",
        ],
        workspace.path(),
    ));
    assert_eq!(replace["data"]["plan"]["action"], "replace_did");
    assert_eq!(replace["data"]["plan"]["dangerous"], true);
    assert_eq!(replace["data"]["plan"]["identity"]["local_alias"], "alice");
    assert_eq!(
        replace["data"]["plan"]["identity"]["did"],
        "did:wba:awiki.ai:alice:e1_alice"
    );
    assert_eq!(
        replace["data"]["plan"]["remote_replace_did_call_preview"]["params"]["is_public"],
        false
    );
    assert_eq!(
        replace["data"]["plan"]["remote_replace_did_call_preview"]["params"]["role"],
        Value::Null
    );
    assert_eq!(
        replace["data"]["plan"]["remote_replace_did_call_preview"]["params"]["endpoint_url"],
        "https://example.com/agent"
    );
    assert_eq!(
        replace["data"]["plan"]["remote_replace_did_call_preview"]["method"],
        "replace_did"
    );
    assert_eq!(
        replace["data"]["plan"]["backup_plan"]["manifest_preview"]["old_did"],
        "did:wba:awiki.ai:alice:e1_alice"
    );
    assert_eq!(
        replace["data"]["plan"]["local_rebind_plan"]["old_owner_did"],
        "did:wba:awiki.ai:alice:e1_alice"
    );
    assert_eq!(
        replace["data"]["plan"]["local_rebind_plan"]["dry_run_only"],
        true
    );
    assert!(replace["data"]["plan"]["local_writes"]
        .as_array()
        .unwrap()
        .contains(&json!(".legacy-backup/replace-did")));
    assert!(replace["data"]["plan"]["rollback_notes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|note| note.as_str().unwrap().contains("backup manifest")));
    assert!(replace["warnings"][0]
        .as_str()
        .unwrap()
        .contains("Dangerous command"));

    let profile_schema = success_json(&awiki_cmd(&["schema", "id", "profile"], workspace.path()));
    let children: Vec<_> = profile_schema["data"]["children"]
        .as_array()
        .expect("profile children should be an array")
        .iter()
        .map(|child| child["name"].as_str().unwrap())
        .collect();
    assert!(children.contains(&"id.profile.get"));
    assert!(children.contains(&"id.profile.set"));

    let profile_plan = success_json(&awiki_cmd(
        &[
            "--dry-run",
            "id",
            "profile",
            "set",
            "--display-name",
            "Alice Example",
            "--bio",
            "Rust port",
            "--tags",
            "rust,port",
            "--markdown",
            "inline",
        ],
        workspace.path(),
    ));
    assert_eq!(profile_plan["summary"], "Dry run: profile update planned");
    assert_eq!(profile_plan["data"]["plan"]["action"], "update_profile");
    assert_eq!(
        profile_plan["data"]["plan"]["display_name"],
        "Alice Example"
    );
    assert_eq!(profile_plan["data"]["plan"]["bio"], "Rust port");
    assert_eq!(profile_plan["data"]["plan"]["tags"], "rust,port");
    assert_eq!(profile_plan["data"]["plan"]["markdown"], "inline");
    assert_eq!(profile_plan["data"]["plan"]["markdown_file"], "");
    assert!(profile_plan["data"]["plan"]["remote_calls"]
        .as_array()
        .unwrap()
        .contains(&json!("did.profile.update_me")));

    let profile_conflict = awiki_cmd(
        &[
            "id",
            "profile",
            "set",
            "--markdown",
            "inline",
            "--markdown-file",
            "profile.md",
        ],
        workspace.path(),
    );
    assert_code(&profile_conflict, 2);
    let profile_conflict = error_json(&profile_conflict);
    assert_eq!(profile_conflict["error"]["code"], "invalid_argument");
    assert!(profile_conflict["error"]["message"]
        .as_str()
        .unwrap()
        .contains("Use either --markdown or --markdown-file"));
}

#[test]
fn identity_import_v1_flat_legacy_contract() {
    let workspace = TempDir::new().expect("workspace");
    let home = workspace.path().join("home");
    let generated = awiki_cli::identity::generate_identity("example.test", "", "")
        .expect("legacy fixture identity");
    let legacy = home
        .join(".openclaw")
        .join("credentials")
        .join("awiki-agent-id-message");
    std::fs::create_dir_all(&legacy).unwrap();
    std::fs::write(
        legacy.join("legacy-flat.json"),
        serde_json::to_vec_pretty(&json!({
            "did": generated.did,
            "unique_id": generated.unique_id,
            "name": "Legacy Flat",
            "handle": "legacy-flat",
            "jwt_token": "token",
            "private_key_pem": generated.key1_private_pem,
            "public_key_pem": generated.key1_public_pem,
            "did_document": generated.did_document
        }))
        .unwrap(),
    )
    .unwrap();

    let imported = success_json(&awiki_cmd_with_home(
        &["--migration", "id", "import-v1", "--name", "legacy-flat"],
        workspace.path(),
        &home,
    ));
    assert_eq!(
        imported["data"]["result"]["imported"][0]["identity_name"],
        "legacy-flat"
    );
    let current = success_json(&awiki_cmd_with_home(
        &["id", "current"],
        workspace.path(),
        &home,
    ));
    assert_eq!(current["data"]["identity"]["identity_name"], "legacy-flat");
}

#[test]
fn identity_import_v1_migrates_legacy_config_json_before_import_like_go() {
    let workspace = TempDir::new().expect("workspace");
    let workspace_home = workspace.path().join(".awiki-cli");
    std::fs::create_dir_all(&workspace_home).expect("create workspace home");
    let legacy_config = workspace_home.join("config.json");
    let legacy_payload = json!({
        "schema_version": 1,
        "services": {
            "service_base_url": "https://legacy-id-import.example",
            "did_domain": "legacy-id-import.example",
        },
        "runtime": {
            "mode": "http",
        },
    });
    let legacy_text = serde_json::to_string(&legacy_payload).expect("serialize legacy config");
    std::fs::write(&legacy_config, &legacy_text).expect("write legacy config");

    let home = workspace.path().join("home");
    let generated = awiki_cli::identity::generate_identity("legacy-id-import.example", "", "")
        .expect("legacy fixture identity");
    let legacy = home
        .join(".openclaw")
        .join("credentials")
        .join("awiki-agent-id-message");
    std::fs::create_dir_all(&legacy).unwrap();
    std::fs::write(
        legacy.join("legacy-flat.json"),
        serde_json::to_vec_pretty(&json!({
            "did": generated.did,
            "unique_id": generated.unique_id,
            "name": "Legacy Flat",
            "handle": "legacy-flat",
            "jwt_token": "token",
            "private_key_pem": generated.key1_private_pem,
            "public_key_pem": generated.key1_public_pem,
            "did_document": generated.did_document
        }))
        .unwrap(),
    )
    .unwrap();

    let imported = success_json(&awiki_cmd_with_home(
        &["--migration", "id", "import-v1", "--name", "legacy-flat"],
        workspace.path(),
        &home,
    ));
    assert_eq!(
        imported["data"]["result"]["imported"][0]["identity_name"],
        "legacy-flat"
    );
    let current = success_json(&awiki_cmd_with_home(
        &["id", "current"],
        workspace.path(),
        &home,
    ));
    assert_eq!(current["data"]["identity"]["identity_name"], "legacy-flat");

    assert!(
        !legacy_config.exists(),
        "legacy config.json should be removed before import"
    );
    assert_migrated_config(
        &workspace_home,
        "https://legacy-id-import.example",
        "legacy-id-import.example",
    );

    assert_workspace_upgrade_meta(&workspace_home, &legacy_text);
    assert_no_runtime_state(&workspace_home, "id import-v1");
}

fn awiki_cmd(args: &[&str], workspace: &Path) -> Output {
    awiki_cmd_with_home(args, workspace, workspace)
}

fn identity_manager(workspace: &Path) -> awiki_cli::identity::Manager {
    awiki_cli::identity::Manager::new(awiki_cli::config::Paths {
        workspace_home_dir: path_string(workspace),
        root_dir: path_string(workspace),
        config_dir: path_string(&workspace.join("config")),
        data_dir: path_string(&workspace.join("data")),
        state_dir: path_string(&workspace.join("state")),
        cache_dir: path_string(&workspace.join("cache")),
        logs_dir: path_string(&workspace.join("logs")),
        config_file: path_string(&workspace.join("config").join("config.yaml")),
        identity_dir: path_string(&workspace.join("identities")),
        database_file: path_string(&workspace.join("data").join("awiki.db")),
        legacy_credentials_dir: path_string(&workspace.join("legacy")),
        legacy_data_dir: path_string(&workspace.join("legacy-data")),
    })
}

fn write_legacy_config_json(workspace_home: &Path, payload: Value) -> (PathBuf, String) {
    let legacy_config = workspace_home.join("config.json");
    let legacy_text = serde_json::to_string(&payload).expect("serialize legacy config");
    std::fs::write(&legacy_config, &legacy_text).expect("write legacy config");
    (legacy_config, legacy_text)
}

fn assert_migrated_config(
    workspace_home: &Path,
    service_base_url: &str,
    did_domain: &str,
) -> String {
    let config_text =
        std::fs::read_to_string(workspace_home.join("config.yaml")).expect("read migrated config");
    for (needle, label) in [
        ("schema_version: 1\n".to_string(), "config schema"),
        ("  mode: http\n".to_string(), "runtime mode"),
        (
            format!("  service_base_url: {service_base_url}\n"),
            "service URL",
        ),
        (format!("  did_domain: {did_domain}\n"), "DID domain"),
    ] {
        assert!(
            config_text.contains(&needle),
            "migrated config should keep {label}, got {config_text:?}"
        );
    }
    config_text
}

fn assert_workspace_upgrade_meta(workspace_home: &Path, legacy_text: &str) {
    let meta_path = workspace_home.join("upgrade").join("meta.json");
    let meta: Value =
        serde_json::from_slice(&std::fs::read(&meta_path).expect("read upgrade meta"))
            .expect("upgrade meta JSON");
    assert_eq!(meta["workspace_schema_version"], 3);
    assert_non_empty_string(&meta["last_upgrade_id"], "last_upgrade_id");
    assert_non_empty_string(&meta["last_backup_dir"], "last_backup_dir");
    let backup_dir = PathBuf::from(meta["last_backup_dir"].as_str().unwrap());
    assert_eq!(
        backup_dir.parent(),
        Some(workspace_home.join("upgrade").join("backups").as_path())
    );
    assert_eq!(
        std::fs::read_to_string(backup_dir.join("config.json.bak"))
            .expect("read legacy config backup"),
        legacy_text
    );
    assert!(
        !workspace_home
            .join("upgrade")
            .join("upgrade_journal.json")
            .exists(),
        "journal should be cleared after successful upgrade"
    );
}

fn assert_no_runtime_state(workspace_home: &Path, label: &str) {
    assert!(
        !workspace_home.join("data").join("awiki-cli.db").exists(),
        "{label} should not create SQLite state"
    );
    assert!(
        !workspace_home
            .join("runtime")
            .join("message-daemon.sock")
            .exists(),
        "{label} must not create runtime socket artifacts"
    );
    assert!(
        !workspace_home.join("runtime").join("listener.pid").exists(),
        "{label} must not create listener pid artifacts"
    );
}

fn read_json(path: &str) -> Value {
    serde_json::from_slice(&std::fs::read(path).expect("read JSON file")).expect("parse JSON file")
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn awiki_cmd_with_home(args: &[&str], workspace: &Path, home: &Path) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_awiki-cli"));
    command
        .args(args)
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace.join(".awiki-cli"))
        .env("HOME", home)
        .env("AWIKI_CLI_UPDATE_CACHE_ONLY", "1")
        .env_remove("AWIKI_WORKSPACE")
        .env_remove("AWIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_HOME")
        .env_remove("AVIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_FORMAT")
        .env_remove("AVIKI_FORMAT");
    command.output().expect("run awiki-cli")
}

fn success_json(output: &Output) -> Value {
    assert_success(output);
    assert!(
        output.stderr.is_empty(),
        "stderr should be empty: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("success JSON")
}

fn error_json(output: &Output) -> Value {
    serde_json::from_slice(&output.stderr).expect("error JSON")
}

fn assert_success(output: &Output) {
    assert_code(output, 0);
}

fn assert_code(output: &Output, code: i32) {
    assert_eq!(
        output.status.code(),
        Some(code),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_error_contains(error: awiki_cli::identity::IdentityError, needle: &str) {
    let message = error.to_string();
    assert!(
        message.contains(needle),
        "error {message:?} should contain {needle:?}"
    );
}

fn assert_non_empty_string(value: &Value, field: &str) {
    assert!(
        value.as_str().is_some_and(|text| !text.trim().is_empty()),
        "{field} should be a non-empty string: {value:?}"
    );
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> std::io::Result<Self> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "awiki-cli-rs2-id-test-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
