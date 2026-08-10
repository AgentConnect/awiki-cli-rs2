use std::time::{SystemTime, UNIX_EPOCH};

use anp::authentication::{create_did_wba_document, DidDocumentOptions, DidProfile, VM_KEY_AUTH};
use awiki_im_core::prelude::*;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde_json::json;

fn core(root: &std::path::Path) -> ImCore {
    ImCore::new(
        ImCoreConfig::new(
            ServiceEndpoint::parse("https://awiki.test").unwrap(),
            "awiki.test",
        )
        .unwrap(),
        ImCorePaths {
            identities: IdentityRegistryPaths {
                identity_root_dir: root.join("identities"),
                registry_path: root.join("identities.json"),
                default_identity_path: Some(root.join("default-identity")),
            },
            local_state: LocalStatePaths {
                sqlite_path: root.join("im-core.sqlite3"),
            },
            runtime: RuntimePaths {
                cache_dir: root.join("cache"),
                temp_dir: root.join("tmp"),
            },
        },
    )
    .unwrap()
}

fn device_access_token(
    bootstrap: &VNextAgentBootstrapMaterial,
    account_id: &str,
    audiences: &[&str],
) -> String {
    device_access_token_with_profile(
        bootstrap,
        account_id,
        audiences,
        1,
        &["device:manage", "device:read", "message:connect"],
    )
}

fn device_access_token_with_profile(
    bootstrap: &VNextAgentBootstrapMaterial,
    account_id: &str,
    audiences: &[&str],
    auth_generation: u64,
    scopes: &[&str],
) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    device_access_token_with_times(
        bootstrap,
        account_id,
        audiences,
        auth_generation,
        scopes,
        now.saturating_sub(1),
        now + 3600,
    )
}

fn device_access_token_with_times(
    bootstrap: &VNextAgentBootstrapMaterial,
    account_id: &str,
    audiences: &[&str],
    auth_generation: u64,
    scopes: &[&str],
    issued_at: u64,
    expires_at: u64,
) -> String {
    let claims = json!({
        "iss": "user-service",
        "aud": audiences,
        "sub": bootstrap.did.as_str(),
        "type": "access",
        "purpose": "awiki.device.access.v1",
        "did": bootstrap.did.as_str(),
        "user_id": account_id,
        "device_id": bootstrap.protocol_device_id.as_str(),
        "key_id": bootstrap.device_signing_key_id,
        "auth_generation": auth_generation,
        "scopes": scopes,
        "iat": issued_at,
        "nbf": issued_at,
        "exp": expires_at,
        "jti": format!("agent-bootstrap-{}", bootstrap.protocol_device_id.as_str()),
    });
    format!(
        "e30.{}.test-signature",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap())
    )
}

fn host_backed_material(
    bootstrap: &VNextAgentBootstrapMaterial,
    account_id: &str,
) -> HostBackedDeviceIdentityMaterial {
    HostBackedDeviceIdentityMaterial {
        identity_id: bootstrap.identity_id.clone(),
        did: bootstrap.did.as_str().to_owned(),
        handle: Some(format!("{}.awiki.test", bootstrap.handle_local_part)),
        display_name: Some(bootstrap.handle_local_part.clone()),
        account_id: account_id.to_owned(),
        binding_generation: "1".to_owned(),
        did_document: bootstrap.did_document.clone(),
        protocol_device_id: bootstrap.protocol_device_id.clone(),
        device_signing_key_id: bootstrap.device_signing_key_id.clone(),
        device_signing_private_key_pem: bootstrap.device_signing_private_key_pem.clone(),
        device_e2ee_key_id: bootstrap.device_e2ee_key_id.clone(),
        device_e2ee_private_key_pem: bootstrap.device_e2ee_private_key_pem.clone(),
        root_key_id: bootstrap.root_key_id.clone(),
        root_private_key_pem: bootstrap.root_private_key_pem.clone(),
        authorization_status: IdentityDeviceAuthorizationStatus::Active,
        role: IdentityDeviceRole::Admin,
        management_ready: true,
        auth_generation: "1".to_owned(),
        access_token: device_access_token(
            bootstrap,
            account_id,
            &["awiki-user-service", "awiki-message-service"],
        ),
    }
}

#[test]
fn unified_agent_builder_uses_typed_path_and_separate_random_device_keys() {
    let root = tempfile::tempdir().unwrap();
    let core = core(root.path());

    for (kind, segment) in [
        (AgentIdentityKind::Skill, "skill"),
        (AgentIdentityKind::Daemon, "daemon"),
        (AgentIdentityKind::Runtime, "runtime"),
    ] {
        let first = core
            .generate_vnext_agent_bootstrap(kind, "Example-Agent")
            .unwrap();
        let second = core
            .generate_vnext_agent_bootstrap(kind, "Example-Agent")
            .unwrap();
        let manifest = anp::authentication::validate_device_manifest(&first.did_document)
            .unwrap()
            .unwrap();

        assert!(first.did.as_str().starts_with(&format!(
            "did:wba:awiki.test:agent:{segment}:example-agent:e1_"
        )));
        assert_eq!(manifest.devices.len(), 1);
        assert_eq!(
            manifest.devices[0].device_id,
            first.protocol_device_id.as_str()
        );
        assert_ne!(first.protocol_device_id.as_str(), "default");
        assert_ne!(first.protocol_device_id, second.protocol_device_id);
        assert_ne!(first.root_key_id, first.device_signing_key_id);
        assert_ne!(
            first.root_private_key_pem,
            first.device_signing_private_key_pem
        );
        assert_ne!(
            first.device_signing_private_key_pem,
            first.device_e2ee_private_key_pem
        );
    }
}

#[tokio::test]
async fn valid_host_backed_device_produces_exact_six_field_sync_binding() {
    let root = tempfile::tempdir().unwrap();
    let core = core(root.path());
    let bootstrap = core
        .generate_vnext_agent_bootstrap(AgentIdentityKind::Daemon, "sync-daemon")
        .unwrap();
    let client = core
        .client_with_device_identity_material(host_backed_material(&bootstrap, "agent-account-1"))
        .unwrap();

    let binding = client.active_sync_account_binding().await.unwrap();

    assert_eq!(binding.owner_identity_id, bootstrap.identity_id);
    assert_eq!(binding.account_id, "agent-account-1");
    assert_eq!(binding.current_did, bootstrap.did.as_str());
    assert_eq!(
        binding.protocol_device_id,
        bootstrap.protocol_device_id.as_str()
    );
    assert_eq!(binding.identity_generation, "1");
    assert_eq!(binding.device_auth_generation, "1");
}

#[test]
fn expired_host_backed_token_preserves_identity_binding_and_requests_refresh() {
    let root = tempfile::tempdir().unwrap();
    let core = core(root.path());
    let bootstrap = core
        .generate_vnext_agent_bootstrap(AgentIdentityKind::Runtime, "expired-runtime")
        .unwrap();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mut material = host_backed_material(&bootstrap, "agent-account-expired");
    material.access_token = device_access_token_with_times(
        &bootstrap,
        "agent-account-expired",
        &["awiki-user-service", "awiki-message-service"],
        1,
        &["device:manage", "device:read", "message:connect"],
        now - 7200,
        now - 3600,
    );

    let client = core
        .client_with_device_identity_material(material)
        .expect("an expired but exactly bound token must not invalidate the identity");
    let status = client.auth().status().unwrap();
    assert!(!status.has_session);
    assert!(status.needs_refresh);
    assert!(matches!(
        client.auth().ensure_session(AuthScope::Messaging),
        Err(ImError::SessionExpired)
    ));
}

#[tokio::test]
async fn generic_hosted_material_keeps_legacy_no_binding_semantics() {
    let root = tempfile::tempdir().unwrap();
    let core = core(root.path());
    let bootstrap = core
        .generate_vnext_agent_bootstrap(AgentIdentityKind::Daemon, "legacy-hosted")
        .unwrap();
    let client = core
        .client_with_identity_material(HostedIdentityMaterial {
            identity_id: bootstrap.identity_id.clone(),
            did: bootstrap.did.as_str().to_owned(),
            handle: Some("legacy-hosted.awiki.test".to_owned()),
            display_name: None,
            did_document: bootstrap.did_document.clone(),
            default_signing_private_key_pem: bootstrap.device_signing_private_key_pem.clone(),
            e2ee_agreement_private_key_pem: Some(bootstrap.device_e2ee_private_key_pem.clone()),
            auth_token: Some(device_access_token(
                &bootstrap,
                "agent-account-hosted",
                &["awiki-user-service", "awiki-message-service"],
            )),
        })
        .unwrap();

    assert!(client.active_sync_account_binding().await.is_err());
}

#[test]
fn host_backed_device_rejects_wrong_account_role_readiness_key_and_audience() {
    let root = tempfile::tempdir().unwrap();
    let core = core(root.path());
    let bootstrap = core
        .generate_vnext_agent_bootstrap(AgentIdentityKind::Runtime, "strict-runtime")
        .unwrap();

    let mut wrong_account = host_backed_material(&bootstrap, "agent-account-1");
    wrong_account.account_id = "agent-account-2".to_owned();
    assert!(core
        .client_with_device_identity_material(wrong_account)
        .is_err());

    let mut wrong_role = host_backed_material(&bootstrap, "agent-account-1");
    wrong_role.role = IdentityDeviceRole::Member;
    assert!(core
        .client_with_device_identity_material(wrong_role)
        .is_err());

    let mut wrong_readiness = host_backed_material(&bootstrap, "agent-account-1");
    wrong_readiness.management_ready = false;
    assert!(core
        .client_with_device_identity_material(wrong_readiness)
        .is_err());

    let other = core
        .generate_vnext_agent_bootstrap(AgentIdentityKind::Runtime, "other-runtime")
        .unwrap();
    let mut wrong_key = host_backed_material(&bootstrap, "agent-account-1");
    wrong_key.device_signing_private_key_pem = other.device_signing_private_key_pem.clone();
    assert!(core
        .client_with_device_identity_material(wrong_key)
        .is_err());

    let mut wrong_root = host_backed_material(&bootstrap, "agent-account-1");
    wrong_root.root_private_key_pem = other.root_private_key_pem;
    assert!(core
        .client_with_device_identity_material(wrong_root)
        .is_err());

    let mut wrong_identity_id = host_backed_material(&bootstrap, "agent-account-1");
    wrong_identity_id.identity_id = "another-owner".to_owned();
    assert!(core
        .client_with_device_identity_material(wrong_identity_id)
        .is_err());

    let mut wrong_handle = host_backed_material(&bootstrap, "agent-account-1");
    wrong_handle.handle = Some("different-runtime.awiki.test".to_owned());
    assert!(core
        .client_with_device_identity_material(wrong_handle)
        .is_err());

    let mut wrong_relationship = host_backed_material(&bootstrap, "agent-account-1");
    wrong_relationship
        .did_document
        .as_object_mut()
        .unwrap()
        .remove("keyAgreement");
    assert!(core
        .client_with_device_identity_material(wrong_relationship)
        .is_err());

    let mut wrong_generation = host_backed_material(&bootstrap, "agent-account-1");
    wrong_generation.auth_generation = "2".to_owned();
    assert!(core
        .client_with_device_identity_material(wrong_generation)
        .is_err());

    let mut wrong_scopes = host_backed_material(&bootstrap, "agent-account-1");
    wrong_scopes.access_token = device_access_token_with_profile(
        &bootstrap,
        "agent-account-1",
        &["awiki-user-service", "awiki-message-service"],
        1,
        &["device:read", "message:connect"],
    );
    assert!(core
        .client_with_device_identity_material(wrong_scopes)
        .is_err());

    let mut missing_audience = host_backed_material(&bootstrap, "agent-account-1");
    missing_audience.access_token =
        device_access_token(&bootstrap, "agent-account-1", &["awiki-user-service"]);
    assert!(core
        .client_with_device_identity_material(missing_audience)
        .is_err());
}

#[test]
fn secret_bearing_public_material_debug_is_redacted() {
    let root = tempfile::tempdir().unwrap();
    let core = core(root.path());
    let bootstrap = core
        .generate_vnext_agent_bootstrap(AgentIdentityKind::Skill, "debug-skill")
        .unwrap();
    let material = host_backed_material(&bootstrap, "agent-account-debug");

    let bootstrap_debug = format!("{bootstrap:?}");
    let material_debug = format!("{material:?}");

    for secret in [
        bootstrap.root_private_key_pem.as_str(),
        bootstrap.device_signing_private_key_pem.as_str(),
        bootstrap.device_e2ee_private_key_pem.as_str(),
        material.access_token.as_str(),
    ] {
        assert!(!bootstrap_debug.contains(secret));
        assert!(!material_debug.contains(secret));
    }
}

#[test]
fn same_did_legacy_upgrade_preserves_root_did_and_handle_and_rejects_cross_kind() {
    let root = tempfile::tempdir().unwrap();
    let core = core(root.path());
    let legacy = create_did_wba_document(
        "awiki.test",
        DidDocumentOptions {
            path_segments: vec![
                "agent".to_owned(),
                "daemon".to_owned(),
                "legacy-daemon".to_owned(),
            ],
            domain: Some("awiki.test".to_owned()),
            challenge: Some("legacy-agent-upgrade".to_owned()),
            services: vec![json!({
                "id": "#handle",
                "type": anp::wns::ANP_HANDLE_SERVICE_TYPE,
                "serviceEndpoint": anp::wns::build_resolution_url(
                    "legacy-daemon",
                    "awiki.test",
                ),
            })],
            did_profile: DidProfile::E1,
            ..DidDocumentOptions::default()
        },
    )
    .unwrap();
    let legacy_did = legacy.did().unwrap().to_owned();
    let root_private = legacy.private_key_pem(VM_KEY_AUTH).unwrap().to_owned();
    let root_public = legacy.public_key_pem(VM_KEY_AUTH).unwrap().to_owned();

    let upgraded = core
        .prepare_vnext_agent_legacy_upgrade(
            AgentIdentityKind::Daemon,
            "legacy-daemon",
            legacy.did_document.clone(),
            root_private.clone(),
        )
        .unwrap();

    assert_eq!(upgraded.did.as_str(), legacy_did);
    assert_eq!(upgraded.root_public_key_pem, root_public);
    assert_eq!(upgraded.did_document["id"], legacy_did);
    assert!(upgraded.did_document["service"]
        .as_array()
        .unwrap()
        .iter()
        .any(|service| service["serviceEndpoint"]
            == anp::wns::build_resolution_url("legacy-daemon", "awiki.test")));
    assert!(core
        .prepare_vnext_agent_legacy_upgrade(
            AgentIdentityKind::Runtime,
            "legacy-daemon",
            legacy.did_document,
            root_private,
        )
        .is_err());
}
