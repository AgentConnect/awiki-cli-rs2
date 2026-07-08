use anp::authentication::{create_did_wba_document, DidDocumentOptions};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

mod support;

use support::{set_secret_storage_mode, write_ready_identity, TestIdentityOptions};

#[test]
fn identity_create_list_current_use_and_status_match_local_contract() {
    let workspace = TempDir::new().expect("workspace");
    let workspace_home = workspace.path().join(".awiki-cli");
    write_file_compat_config(&workspace_home);

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
    assert!(list["data"].get("legacy_scan").is_none());

    let status = success_json(&awiki_cmd(&["id", "status"], workspace.path()));
    assert_eq!(status["data"]["active_identity"]["identity_name"], "bob");
    assert_eq!(status["data"]["identity_count"], 2);
}

#[test]
fn identity_create_refuses_legacy_plaintext_storage_by_default() {
    let workspace = TempDir::new().expect("workspace");
    let workspace_home = workspace.path().join(".awiki-cli");

    let create = awiki_cmd(
        &[
            "--migration",
            "id",
            "create",
            "--name",
            "Vault Required",
            "--identity",
            "vault-required",
        ],
        workspace.path(),
    );
    assert_code(&create, 3);
    let create = error_json(&create);
    assert_eq!(
        create["error"]["code"],
        "legacy_plaintext_identity_storage_disabled"
    );
    assert!(
        !workspace_home
            .join("identities")
            .join("vault-required")
            .join("key-1-private.pem")
            .exists(),
        "default vault_required mode must not create plaintext identity private key files"
    );
}

#[test]
fn identity_vault_required_creates_local_root_key_without_env() {
    let workspace = TempDir::new().expect("workspace");
    let workspace_home = workspace.path().join(".awiki-cli");
    write_ready_identity(
        &workspace_home,
        TestIdentityOptions {
            identity_name: "alice",
            handle: "alice",
            display_name: "Alice",
            jwt_token: "jwt-alice",
            make_default: true,
        },
    );
    std::fs::write(
        workspace_home.join("config.yaml"),
        "secret_storage:\n  mode: vault_required\n",
    )
    .expect("write vault config");
    let local_root_key_file = workspace_home
        .join("data")
        .join("identity-vault")
        .join("root-key.b64u");

    let status = awiki_cmd_with_vault_root(&["id", "vault", "status"], workspace.path(), None);
    assert_success(&status);
    let status = success_json(&status);
    assert_eq!(
        status["data"]["vault"]["open_options"]["mode"],
        "vault_required"
    );
    assert_eq!(
        status["data"]["vault"]["open_options"]["root_key"]["available"],
        true
    );
    assert_eq!(
        status["data"]["vault"]["open_options"]["root_key"]["source"],
        "local_private_file_pending_create"
    );
    assert_eq!(
        status["data"]["vault"]["status_context"]["checked_without_vault_context"],
        true
    );
    assert!(
        !local_root_key_file.exists(),
        "vault status must not create the no-prompt local root key file"
    );

    let plan = awiki_cmd_with_vault_root(
        &["--dry-run", "--migration", "id", "vault", "migrate"],
        workspace.path(),
        None,
    );
    assert_success(&plan);
    let plan = success_json(&plan);
    assert_eq!(
        plan["data"]["plan"]["open_options"]["root_key"]["source"],
        "local_private_file_pending_create"
    );
    assert!(
        !local_root_key_file.exists(),
        "dry-run vault mutation must not create the no-prompt local root key file"
    );

    let migrate = awiki_cmd_with_vault_root(
        &["--migration", "id", "vault", "migrate"],
        workspace.path(),
        None,
    );
    assert_success(&migrate);
    assert!(
        local_root_key_file.exists(),
        "vault mutation should create no-prompt local root key file"
    );
    assert!(
        workspace_home
            .join("identities")
            .join("alice")
            .join("key-1-private.pem")
            .exists(),
        "migration currently retains plaintext compatibility files until cleanup API is enabled"
    );
}

#[test]
fn identity_vault_status_and_mutation_plans_redact_root_key_material() {
    let workspace = TempDir::new().expect("workspace");
    let workspace_home = workspace.path().join(".awiki-cli");
    let root_key = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    write_ready_identity(
        &workspace_home,
        TestIdentityOptions {
            identity_name: "alice",
            handle: "alice",
            display_name: "Alice",
            jwt_token: "jwt-alice",
            make_default: true,
        },
    );
    std::fs::write(
        workspace_home.join("config.yaml"),
        "secret_storage:\n  mode: vault_preferred\n  workspace_id: test-workspace\n  device_id: test-device\n",
    )
    .expect("write vault config");

    let status = success_json(&awiki_cmd_with_vault_root(
        &["id", "vault", "status"],
        workspace.path(),
        Some(root_key),
    ));
    assert_eq!(
        status["data"]["vault"]["open_options"]["root_key"]["available"],
        true
    );
    assert_eq!(
        status["data"]["vault"]["open_options"]["root_key"]["source"],
        "AWIKI_IM_CORE_VAULT_ROOT_KEY_B64"
    );
    assert_eq!(
        status["data"]["vault"]["identity"]["selected_backend"],
        "file_compat"
    );
    assert_eq!(
        status["data"]["vault"]["identity"]["missing"],
        json!(["identity_vault_metadata"])
    );
    assert_redacted_output(&status, root_key);

    let plan = success_json(&awiki_cmd_with_vault_root(
        &["--dry-run", "--migration", "id", "vault", "migrate"],
        workspace.path(),
        Some(root_key),
    ));
    assert_eq!(
        plan["data"]["plan"]["action"],
        "migrate_identity_secrets_to_vault"
    );
    assert_eq!(plan["data"]["plan"]["root_key_material"], "[redacted]");
    assert_redacted_output(&plan, root_key);
}

#[test]
fn identity_current_migrates_legacy_anp_pem_before_im_core_store_read() {
    let workspace = TempDir::new().expect("workspace");
    let workspace_home = workspace.path().join(".awiki-cli");
    write_file_compat_config(&workspace_home);
    let identity = write_ready_identity(
        &workspace_home,
        TestIdentityOptions {
            identity_name: "legacy-pem",
            handle: "legacy-pem",
            display_name: "Legacy PEM",
            jwt_token: "jwt-legacy",
            make_default: true,
        },
    );
    let key_paths = [
        identity.identity_dir.join("key-1-private.pem"),
        identity.identity_dir.join("e2ee-signing-private.pem"),
        identity.identity_dir.join("e2ee-agreement-private.pem"),
    ];
    std::fs::write(
        &key_paths[0],
        legacy_private_pem("ANP ED25519 PRIVATE KEY", &[1; 32]),
    )
    .expect("write legacy key-1");
    std::fs::write(
        &key_paths[1],
        legacy_private_pem("ANP SECP256R1 PRIVATE KEY", &[1]),
    )
    .expect("write legacy e2ee signing");
    std::fs::write(
        &key_paths[2],
        legacy_private_pem("ANP X25519 PRIVATE KEY", &[2; 32]),
    )
    .expect("write legacy e2ee agreement");

    let current = success_json(&awiki_cmd(&["id", "current"], workspace.path()));

    assert_eq!(current["data"]["identity"]["identity_name"], "legacy-pem");
    assert_eq!(current["data"]["identity"]["has_key1_private"], true);
    assert_eq!(
        current["data"]["identity"]["has_e2ee_signing_private"],
        true
    );
    assert_eq!(
        current["data"]["identity"]["has_e2ee_agreement_private"],
        true
    );
    for path in key_paths {
        assert_standard_private_key_pem(&path);
    }
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

    let create = awiki_cmd(
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
    );
    assert_code(&create, 3);
    let create = error_json(&create);
    assert_eq!(
        create["error"]["code"],
        "legacy_plaintext_identity_storage_disabled"
    );

    assert!(
        !legacy_config.exists(),
        "legacy config.json should still be migrated before the storage policy gate"
    );
    assert_migrated_config(
        &workspace_home,
        "https://legacy-id-create.example",
        "legacy-id-create.example",
    );
    assert_workspace_upgrade_meta(&workspace_home, &legacy_text);
    assert!(
        !workspace_home
            .join("identities")
            .join("legacy-create")
            .join("key-1-private.pem")
            .exists(),
        "vault_required migration gate must not create plaintext private key files"
    );
    assert_no_runtime_state(&workspace_home, "id create");
}

#[test]
fn identity_create_allows_legacy_plaintext_storage_when_file_compat_is_explicit() {
    let workspace = TempDir::new().expect("workspace");
    let workspace_home = workspace.path().join(".awiki-cli");
    write_file_compat_config(&workspace_home);

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
        .starts_with("did:wba:awiki.ai:user:"));
    let unique_id = create["data"]["identity"]["unique_id"]
        .as_str()
        .expect("identity unique_id");
    let current = success_json(&awiki_cmd(&["id", "current"], workspace.path()));
    assert_eq!(
        current["data"]["identity"]["identity_name"],
        "legacy-create"
    );
    assert!(
        workspace_home
            .join("identities")
            .join(unique_id)
            .join("key-1-private.pem")
            .exists(),
        "file_compat is the explicit legacy plaintext identity storage escape hatch"
    );
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
    let alice = write_ready_identity(
        &workspace_home,
        TestIdentityOptions {
            identity_name: "alice",
            handle: "alice",
            display_name: "Alice",
            jwt_token: "",
            make_default: true,
        },
    );
    let bob = write_ready_identity(
        &workspace_home,
        TestIdentityOptions {
            identity_name: "bob",
            handle: "bob",
            display_name: "Bob",
            jwt_token: "",
            make_default: false,
        },
    );
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
    let workspace_home = workspace.path().join(".awiki-cli");
    write_file_compat_config(&workspace_home);
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

    let alice = write_ready_identity(
        &workspace.path().join(".awiki-cli"),
        TestIdentityOptions {
            identity_name: "alice",
            handle: "alice",
            display_name: "Alice",
            jwt_token: "jwt-alice",
            make_default: true,
        },
    );
    set_secret_storage_mode(&workspace_home, "vault_required");
    assert_success(&awiki_cmd_with_vault_root(
        &["--migration", "id", "vault", "migrate"],
        workspace.path(),
        None,
    ));

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
    assert_eq!(replace["data"]["plan"]["identity"]["did"], alice.did);
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
        alice.did
    );
    assert_eq!(
        replace["data"]["plan"]["local_rebind_plan"]["old_owner_did"],
        alice.did
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
    assert!(replace["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|warning| warning.as_str().unwrap().contains("Dangerous command")));

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
            "--avatar-uri",
            "https://example.com/alice.png",
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
    assert_eq!(
        profile_plan["data"]["plan"]["avatar_uri"],
        "https://example.com/alice.png"
    );
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
    let workspace_home = workspace.path().join(".awiki-cli");
    write_file_compat_config(&workspace_home);
    let home = workspace.path().join("home");
    let generated = generated_legacy_identity("example.test", "legacy-flat");
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
    let generated = generated_legacy_identity("legacy-id-import.example", "legacy-flat");
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

    let imported = awiki_cmd_with_home(
        &["--migration", "id", "import-v1", "--name", "legacy-flat"],
        workspace.path(),
        &home,
    );
    assert_code(&imported, 3);
    let imported = error_json(&imported);
    assert_eq!(
        imported["error"]["code"],
        "legacy_plaintext_identity_storage_disabled"
    );

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

fn awiki_cmd_with_vault_root(args: &[&str], workspace: &Path, root_key: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_awiki-cli"));
    command
        .args(args)
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace.join(".awiki-cli"))
        .env("HOME", workspace)
        .env("AWIKI_CLI_UPDATE_CACHE_ONLY", "1")
        .env_remove("AWIKI_WORKSPACE")
        .env_remove("AWIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_HOME")
        .env_remove("AVIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_FORMAT")
        .env_remove("AVIKI_FORMAT")
        .env_remove("AWIKI_IM_CORE_VAULT_ROOT_KEY_B64");
    if let Some(root_key) = root_key {
        command.env("AWIKI_IM_CORE_VAULT_ROOT_KEY_B64", root_key);
    }
    command.output().expect("run awiki-cli")
}

fn write_legacy_config_json(workspace_home: &Path, payload: Value) -> (PathBuf, String) {
    let legacy_config = workspace_home.join("config.json");
    let legacy_text = serde_json::to_string(&payload).expect("serialize legacy config");
    std::fs::write(&legacy_config, &legacy_text).expect("write legacy config");
    (legacy_config, legacy_text)
}

fn write_file_compat_config(workspace_home: &Path) {
    std::fs::create_dir_all(workspace_home).expect("create workspace home");
    std::fs::write(
        workspace_home.join("config.yaml"),
        "secret_storage:\n  mode: file_compat\n",
    )
    .expect("write file_compat config");
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
    assert_eq!(meta["workspace_schema_version"], 4);
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

struct GeneratedLegacyIdentity {
    did: String,
    unique_id: String,
    key1_private_pem: String,
    key1_public_pem: String,
    did_document: Value,
}

fn generated_legacy_identity(domain: &str, handle: &str) -> GeneratedLegacyIdentity {
    let unique_id = format!("e1_{}", sanitize_component(handle));
    let bundle = create_did_wba_document(
        domain,
        DidDocumentOptions {
            path_segments: vec!["user".to_string(), handle.to_string(), unique_id.clone()],
            domain: Some(domain.to_string()),
            challenge: Some(format!("{handle}-legacy-fixture")),
            ..DidDocumentOptions::default()
        },
    )
    .expect("generate legacy fixture DID document");
    GeneratedLegacyIdentity {
        did: bundle.did().expect("generated DID").to_string(),
        unique_id,
        key1_private_pem: bundle
            .private_key_pem("key-1")
            .expect("key-1 private")
            .to_string(),
        key1_public_pem: bundle
            .public_key_pem("key-1")
            .expect("key-1 public")
            .to_string(),
        did_document: bundle.did_document,
    }
}

fn sanitize_component(raw: &str) -> String {
    raw.trim()
        .to_ascii_lowercase()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches(['.', '_', '-'])
        .to_string()
}

fn legacy_private_pem(label: &str, contents: &[u8]) -> String {
    let encoded = STANDARD.encode(contents);
    let mut wrapped = String::new();
    for chunk in encoded.as_bytes().chunks(64) {
        wrapped.push_str(std::str::from_utf8(chunk).expect("base64 chunk"));
        wrapped.push('\n');
    }
    format!("-----BEGIN {label}-----\n{wrapped}-----END {label}-----\n")
}

fn assert_standard_private_key_pem(path: &Path) {
    let value = std::fs::read_to_string(path).expect("read private key");
    assert!(
        !value.contains("BEGIN ANP "),
        "{path:?} still uses legacy ANP PEM label"
    );
    assert!(
        value.starts_with("-----BEGIN PRIVATE KEY-----"),
        "{path:?} starts with {:?}",
        value.lines().next().unwrap_or_default()
    );
    anp::PrivateKeyMaterial::from_pem(&value).expect("standard private key parses");
}

fn assert_redacted_output(value: &Value, forbidden: &str) {
    let encoded = serde_json::to_string(value).expect("json output");
    assert!(
        !encoded.contains(forbidden),
        "CLI output must not contain secret material {forbidden:?}: {encoded}"
    );
    assert!(
        !encoded.contains("-----BEGIN PRIVATE KEY-----"),
        "CLI output must not contain private PEM: {encoded}"
    );
    assert!(
        !encoded.contains("jwt-alice"),
        "CLI output must not contain JWT material: {encoded}"
    );
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
        static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let thread_id = format!("{:?}", std::thread::current().id())
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric())
            .collect::<String>();
        let path = std::env::temp_dir().join(format!(
            "awiki-cli-rs2-id-test-{}-{nanos}-{thread_id}-{counter}",
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
