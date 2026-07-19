use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use anp::authentication::verification_methods::extract_public_key;
use anp::authentication::{create_did_wba_document, DidDocumentOptions};
use anp::proof::{verify_w3c_proof, ProofVerificationOptions};
use awiki_im_core::identity::{IdentityDeviceMode, IdentityDeviceReadiness, IdentityDeviceRole};
use awiki_im_core::prelude::*;
use awiki_im_core::vault::{
    DeviceVaultRootKey, FileSecretVault, FileSecretVaultStore, SecretKind, SecretVault,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

static TEMP_ROOT_COUNTER: AtomicU64 = AtomicU64::new(0);

#[test]
fn identity_registry_lists_default_and_resolves_selectors() {
    let fixture = Fixture::new();
    let core = fixture.core();

    let identities = core.identities().list().unwrap();
    assert_eq!(identities.len(), 2);

    let default = core.identities().default_identity().unwrap().unwrap();
    assert_eq!(default.local_alias.as_deref(), Some("alice"));
    assert!(default.is_default);

    let alice = core
        .identities()
        .resolve(IdentitySelector::LocalAlias("alice".to_string()))
        .unwrap();
    assert_eq!(alice.did.as_str(), "did:example:alice");

    let bob = core
        .identities()
        .resolve(IdentitySelector::Did(
            Did::parse("did:example:bob").unwrap(),
        ))
        .unwrap();
    assert_eq!(bob.local_alias.as_deref(), Some("bob"));

    let by_handle = core
        .identities()
        .resolve(IdentitySelector::Handle(
            Handle::parse("bob.awiki.test", "").unwrap(),
        ))
        .unwrap();
    assert_eq!(by_handle.did.as_str(), "did:example:bob");
}

#[tokio::test]
async fn identity_registry_async_lists_default_and_resolves_selectors() {
    let fixture = Fixture::new();
    let core = fixture.core_async().await;

    let identities = core.identities().list_async().await.unwrap();
    assert_eq!(identities.len(), 2);

    let default = core
        .identities()
        .default_identity_async()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(default.local_alias.as_deref(), Some("alice"));
    assert!(default.is_default);

    let bob = core
        .identities()
        .resolve_async(IdentitySelector::Handle(
            Handle::parse("bob.awiki.test", "").unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(bob.did.as_str(), "did:example:bob");

    let change = core
        .identities()
        .plan_default_identity_change_async(IdentitySelector::LocalAlias("bob".to_string()))
        .await
        .unwrap();
    assert_eq!(
        change.previous.unwrap().local_alias.as_deref(),
        Some("alice")
    );
    assert_eq!(change.next.local_alias.as_deref(), Some("bob"));
}

#[test]
fn default_identity_file_overrides_registry_default_flags() {
    let fixture = Fixture::new();
    fs::write(fixture.root.join("identities").join("default"), "bob\n").unwrap();
    let core = fixture.core();

    let default = core.identities().default_identity().unwrap().unwrap();
    assert_eq!(default.local_alias.as_deref(), Some("bob"));
    assert!(default.is_default);

    let identities = core.identities().list().unwrap();
    let alice = identities
        .iter()
        .find(|identity| identity.local_alias.as_deref() == Some("alice"))
        .unwrap();
    let bob = identities
        .iter()
        .find(|identity| identity.local_alias.as_deref() == Some("bob"))
        .unwrap();
    assert!(!alice.is_default);
    assert!(bob.is_default);
}

#[test]
fn plan_default_identity_change_returns_previous_and_next() {
    let fixture = Fixture::new();
    let core = fixture.core();

    let change = core
        .identities()
        .plan_default_identity_change(IdentitySelector::LocalAlias("bob".to_string()))
        .unwrap();

    assert_eq!(
        change.previous.unwrap().local_alias.as_deref(),
        Some("alice")
    );
    assert_eq!(change.next.local_alias.as_deref(), Some("bob"));
    assert!(change.requires_default_identity_write);
}

#[tokio::test]
async fn register_handle_returns_identity_and_default_change() {
    let server = TestServer::spawn(vec![ExpectedHttp::rpc_result(json!({
        "did": "did:wba:awiki.test:carol:e1_registered",
        "user_id": "user-carol",
        "handle": "carol",
        "full_handle": "carol.awiki.test",
        "access_token": "jwt-carol"
    }))]);
    let fixture = Fixture::new();
    let base_url = server.base_url().to_owned();
    let core = fixture.core_async_with_base_url(&base_url).await;

    let result = core
        .identities()
        .register_handle_async(RegisterHandleRequest {
            local_alias: Some("carol".to_string()),
            requested_handle: Handle::parse("carol.awiki.test", "").unwrap(),
            verification: VerificationInput::AlreadyVerified,
            invite_code: None,
            profile: InitialProfile {
                display_name: Some("Carol".to_string()),
                avatar_url: None,
            },
            make_default: true,
        })
        .await
        .unwrap();

    assert_eq!(result.state, HandleRegistrationState::Registered);
    assert_eq!(result.method, RegistrationMethod::AlreadyVerified);
    assert_eq!(result.handle.as_str(), "carol.awiki.test");
    let identity = result.identity.unwrap();
    assert_eq!(identity.local_alias.as_deref(), Some("carol"));
    assert_eq!(identity.handle.unwrap().as_str(), "carol.awiki.test");
    assert_eq!(identity.display_name.as_deref(), Some("Carol"));
    assert!(identity.readiness.ready_for_auth);
    assert!(result.default_identity_change.is_some());

    let requests = server.join();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/user-service/did-auth/rpc");
    let body = requests[0].json_body();
    assert_eq!(body["method"], "register");
    assert_eq!(body["params"]["handle"], "carol");
    assert!(body["params"]["did_document"].is_object());
    assert!(!requests[0].headers.contains_key("authorization"));
}

#[cfg(feature = "mcp-trusted-registration")]
#[tokio::test]
async fn register_handle_with_service_bearer_adds_authorization_header() {
    let server = TestServer::spawn(vec![ExpectedHttp::rpc_result(json!({
        "did": "did:wba:awiki.test:mcp:e1_registered",
        "user_id": "user-mcp",
        "handle": "mcp",
        "full_handle": "mcp.awiki.test",
        "access_token": "jwt-mcp"
    }))]);
    let fixture = Fixture::new();
    let base_url = server.base_url().to_owned();
    let core = fixture.core_async_with_base_url(&base_url).await;

    let result = core
        .identities()
        .register_handle_with_service_bearer_async(
            RegisterHandleRequest {
                local_alias: Some("mcp".to_string()),
                requested_handle: Handle::parse("mcp.awiki.test", "").unwrap(),
                verification: VerificationInput::AlreadyVerified,
                invite_code: None,
                profile: InitialProfile {
                    display_name: Some("MCP".to_string()),
                    avatar_url: None,
                },
                make_default: true,
            },
            "internal-token",
        )
        .await
        .unwrap();

    assert_eq!(result.state, HandleRegistrationState::Registered);
    let requests = server.join();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].path, "/user-service/did-auth/rpc");
    assert_eq!(
        requests[0].headers.get("authorization").map(String::as_str),
        Some("Bearer internal-token")
    );
    assert_eq!(requests[0].json_body()["method"], "register");
}

#[tokio::test]
async fn register_handle_async_returns_identity_and_default_change() {
    let server = TestServer::spawn(vec![ExpectedHttp::rpc_result(json!({
        "did": "did:wba:awiki.test:dana:e1_registered",
        "user_id": "user-dana",
        "handle": "dana",
        "full_handle": "dana.awiki.test",
        "access_token": "jwt-dana"
    }))]);
    let fixture = Fixture::new();
    let base_url = server.base_url();
    let core = fixture.core_async_with_base_url(base_url).await;

    let result = core
        .identities()
        .register_handle_async(RegisterHandleRequest {
            local_alias: Some("dana".to_string()),
            requested_handle: Handle::parse("dana.awiki.test", "").unwrap(),
            verification: VerificationInput::AlreadyVerified,
            invite_code: None,
            profile: InitialProfile {
                display_name: Some("Dana".to_string()),
                avatar_url: None,
            },
            make_default: true,
        })
        .await
        .unwrap();

    assert_eq!(result.state, HandleRegistrationState::Registered);
    assert_eq!(result.method, RegistrationMethod::AlreadyVerified);
    assert_eq!(result.handle.as_str(), "dana.awiki.test");
    let identity = result.identity.unwrap();
    assert_eq!(identity.local_alias.as_deref(), Some("dana"));
    assert_eq!(identity.handle.unwrap().as_str(), "dana.awiki.test");
    assert_eq!(identity.display_name.as_deref(), Some("Dana"));
    assert!(identity.readiness.ready_for_auth);
    assert!(result.default_identity_change.is_some());

    let requests = server.join();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/user-service/did-auth/rpc");
    let body = requests[0].json_body();
    assert_eq!(body["method"], "register");
    assert_eq!(body["params"]["handle"], "dana");
    assert!(body["params"]["did_document"].is_object());
}

#[tokio::test]
async fn register_handle_generates_and_saves_daemon_subkey_package() {
    let server = TestServer::spawn(vec![ExpectedHttp::rpc_result(json!({
        "user_id": "user-daemon",
        "handle": "daemon",
        "full_handle": "daemon.awiki.test",
        "access_token": "jwt-daemon"
    }))]);
    let fixture = Fixture::new();
    let base_url = server.base_url().to_owned();
    let core = fixture.core_async_with_base_url(&base_url).await;

    let result = core
        .identities()
        .register_handle_async(RegisterHandleRequest {
            local_alias: Some("daemon".to_string()),
            requested_handle: Handle::parse("daemon.awiki.test", "").unwrap(),
            verification: VerificationInput::AlreadyVerified,
            invite_code: None,
            profile: InitialProfile {
                display_name: Some("Daemon User".to_string()),
                avatar_url: None,
            },
            make_default: true,
        })
        .await
        .unwrap();

    let identity = result.identity.unwrap();
    let package = core
        .identities()
        .load_daemon_subkey_package_async(IdentitySelector::LocalAlias("daemon".to_string()))
        .await
        .unwrap();
    assert_eq!(package.schema, "awiki.daemon.user_subkey_package.v2");
    assert_eq!(package.key_type, "Multikey/Ed25519");
    assert_eq!(package.key_algorithm.as_deref(), Some("Ed25519"));
    assert_eq!(package.private_key_encoding, "pem");
    assert_eq!(package.user_did, identity.did);
    assert_eq!(
        package.verification_method,
        format!("{}#daemon-key-1", package.user_did.as_str())
    );
    assert!(package.public_key_multibase.starts_with('z'));
    assert!(package
        .private_key_pem
        .starts_with("-----BEGIN PRIVATE KEY-----"));

    let requests = server.join();
    let body = requests[0].json_body();
    let did_document = &body["params"]["did_document"];
    assert_eq!(did_document["id"].as_str(), Some(package.user_did.as_str()));
    let method = did_document["verificationMethod"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"].as_str() == Some(package.verification_method.as_str()))
        .expect("daemon verification method");
    assert_eq!(method["type"].as_str(), Some("Multikey"));
    assert_eq!(
        method["controller"].as_str(),
        Some(package.user_did.as_str())
    );
    assert_eq!(
        method["publicKeyMultibase"].as_str(),
        Some(package.public_key_multibase.as_str())
    );
    assert!(did_document["authentication"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item.as_str() == Some(package.verification_method.as_str())));

    let proof_method = did_document["proof"]["verificationMethod"]
        .as_str()
        .expect("did document proof verification method");
    assert_eq!(proof_method, format!("{}#key-1", package.user_did.as_str()));
    let proof_public_key = did_document["verificationMethod"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"].as_str() == Some(proof_method))
        .and_then(|item| extract_public_key(item).ok())
        .expect("proof public key");
    assert!(
        verify_w3c_proof(
            did_document,
            &proof_public_key,
            ProofVerificationOptions::default()
        ),
        "DID Document proof must remain valid after APP-side daemon subkey registration"
    );
}

#[tokio::test]
async fn register_handle_vault_required_persists_identity_without_plaintext() {
    let server = TestServer::spawn(vec![ExpectedHttp::rpc_result(json!({
        "user_id": "user-secure",
        "handle": "secure",
        "full_handle": "secure.awiki.test",
        "access_token": "jwt-secure-register"
    }))]);
    let fixture = Fixture::new();
    let base_url = server.base_url().to_owned();
    let core = fixture
        .core_async_with_base_url_vault_required(&base_url, [42_u8; 32])
        .await;

    let result = core
        .identities()
        .register_handle_async(RegisterHandleRequest {
            local_alias: Some("secure".to_string()),
            requested_handle: Handle::parse("secure.awiki.test", "").unwrap(),
            verification: VerificationInput::AlreadyVerified,
            invite_code: None,
            profile: InitialProfile {
                display_name: Some("Secure User".to_string()),
                avatar_url: None,
            },
            make_default: true,
        })
        .await
        .unwrap();

    let identity = result.identity.unwrap();
    let status = core
        .identities()
        .vault_status_async(IdentitySelector::LocalAlias("secure".to_string()))
        .await
        .unwrap();
    assert_eq!(status.selected_backend, IdentitySecretStorageBackend::Vault);
    assert!(status.vault_metadata_verified);
    assert_eq!(status.plaintext_compat_retained, Some(false));
    assert!(status.missing.is_empty(), "{:?}", status.missing);

    let package = core
        .identities()
        .load_daemon_subkey_package_async(IdentitySelector::LocalAlias("secure".to_string()))
        .await
        .unwrap();
    assert_eq!(package.user_did, identity.did);
    assert!(package
        .private_key_pem
        .starts_with("-----BEGIN PRIVATE KEY-----"));
    assert_secure_identity_dir_has_no_plaintext(
        &fixture.root.join("identities").join(identity.id.as_str()),
        "jwt-secure-register",
    );

    let requests = server.join();
    assert_eq!(requests.len(), 1);
}

#[tokio::test]
async fn recover_handle_async_without_otp_sends_recover_otp() {
    let server = TestServer::spawn(vec![ExpectedHttp::rpc_result(json!({ "sent": true }))]);
    let fixture = Fixture::new();
    let base_url = server.base_url();
    let core = fixture.core_async_with_base_url(base_url).await;

    let result = core
        .identities()
        .recover_handle_async(RecoverHandleRequest {
            handle: Handle::parse("alice.awiki.test", "").unwrap(),
            raw_handle: None,
            phone: "+15551234567".to_string(),
            otp: None,
            generated_identity: None,
            local_finalize: None,
        })
        .await
        .unwrap();

    assert_eq!(result.state, RecoverHandleState::OtpSent);
    assert_eq!(result.phone, "+15551234567");
    assert!(result.recovered_identity.is_none());

    let requests = server.join();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/user-service/handle/rpc");
    let body = requests[0].json_body();
    assert_eq!(body["method"], "send_otp");
    assert_eq!(body["params"], json!({ "phone": "+15551234567" }));
}

#[tokio::test]
async fn recover_handle_async_with_otp_recovers_and_persists_identity() {
    let server = TestServer::spawn(vec![ExpectedHttp::rpc_result(json!({
        "user_id": "user-erin",
        "handle": "erin",
        "full_handle": "erin.awiki.test",
        "access_token": "jwt-erin"
    }))]);
    let fixture = Fixture::new();
    let base_url = server.base_url();
    let core = fixture.core_async_with_base_url(base_url).await;

    let result = core
        .identities()
        .recover_handle_async(RecoverHandleRequest {
            handle: Handle::parse("erin", "").unwrap(),
            raw_handle: None,
            phone: "+15551234567".to_string(),
            otp: Some("654321".to_string()),
            generated_identity: None,
            local_finalize: None,
        })
        .await
        .unwrap();

    assert_eq!(result.state, RecoverHandleState::Recovered);
    assert_eq!(result.handle.as_str(), "erin.awiki.test");
    let recovered = result.recovered_identity.unwrap();
    assert_eq!(recovered.identity.local_alias.as_deref(), Some("erin"));
    let recovered_did = recovered.identity.did.as_str().to_string();
    assert!(recovered.access_token_present);
    let default = core
        .identities()
        .default_identity_async()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(default.handle.unwrap().as_str(), "alice.awiki.test");
    let persisted = core
        .identities()
        .resolve_async(IdentitySelector::LocalAlias("erin".to_string()))
        .await
        .unwrap();
    assert_eq!(persisted.did.as_str(), recovered_did);

    let requests = server.join();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/user-service/did-auth/rpc");
    let body = requests[0].json_body();
    assert_eq!(body["method"], "recover_handle");
    assert_eq!(body["params"]["handle"], "erin.awiki.test");
    assert_eq!(body["params"]["phone"], "+15551234567");
    assert_eq!(body["params"]["otp_code"], "654321");
    assert!(body["params"]["did_document"].is_object());
    assert_eq!(
        body["params"]["did_document"]["id"].as_str(),
        Some(recovered_did.as_str())
    );
}

#[tokio::test]
async fn recover_same_handle_preserves_owner_history_and_enqueues_group_rebind() {
    let server = TestServer::spawn(vec![ExpectedHttp::rpc_result(json!({
        "user_id": "user-erin",
        "handle": "erin",
        "full_handle": "erin.awiki.test",
        "access_token": "jwt-erin-recovered",
        "binding_generation": "2"
    }))]);
    let fixture = Fixture::new();
    let previous = fixture.write_generated_identity_with_daemon_key("erin", true, true);
    let previous_did = previous.did.clone();
    let stable_owner_identity_id = "erin-id";
    let sqlite_path = fixture.root.join("local").join("im.sqlite");
    let base_url = server.base_url().to_owned();
    let core = fixture.core_async_with_base_url(&base_url).await;
    core.bootstrap()
        .initialize_local_state_async()
        .await
        .unwrap();

    let connection = rusqlite::Connection::open(&sqlite_path).unwrap();
    connection
        .execute(
            r#"INSERT INTO messages
                (msg_id, owner_identity_id, owner_did, thread_id, direction,
                 content_type, content, stored_at, credential_name)
               VALUES (?1, ?2, ?3, ?4, 0, 'text', 'before recovery', ?5, 'erin')"#,
            rusqlite::params![
                "msg-before-recovery",
                stable_owner_identity_id,
                previous_did,
                "dm:peer-scope:v1:test",
                "2026-07-13T10:00:00Z"
            ],
        )
        .unwrap();
    connection
        .execute(
            r#"INSERT INTO groups
                (owner_identity_id, owner_did, group_id, group_did, name,
                 my_role, membership_status, stored_at, credential_name)
               VALUES (?1, ?2, ?3, ?3, 'Recovery group', 'member', 'active', ?4, 'erin')"#,
            rusqlite::params![
                stable_owner_identity_id,
                previous_did,
                "did:wba:awiki.test:groups:recovery-test",
                "2026-07-13T10:00:00Z"
            ],
        )
        .unwrap();
    connection
        .execute(
            r#"INSERT INTO group_members
                (owner_identity_id, owner_did, group_id, user_id, member_did,
                 member_handle, anchor_kind, anchor_value, role, status,
                 handle_binding_generation, last_synced_at, credential_name)
               VALUES (?1, ?2, ?3, 'user-erin', ?2, 'erin',
                       'handle', 'erin', 'member', 'active', '1', ?4, 'erin')"#,
            rusqlite::params![
                stable_owner_identity_id,
                previous_did,
                "did:wba:awiki.test:groups:recovery-test",
                "2026-07-13T10:00:00Z"
            ],
        )
        .unwrap();
    drop(connection);

    let result = core
        .identities()
        .recover_handle_async(RecoverHandleRequest {
            handle: Handle::parse("erin.awiki.test", "").unwrap(),
            raw_handle: Some("erin.awiki.test".to_string()),
            phone: "+15551234567".to_string(),
            otp: Some("654321".to_string()),
            generated_identity: None,
            local_finalize: None,
        })
        .await
        .unwrap();

    let recovered = result.recovered_identity.as_ref().unwrap();
    let recovered_did = recovered.identity.did.as_str();
    assert_eq!(recovered.identity.id.as_str(), stable_owner_identity_id);
    assert_ne!(recovered_did, previous_did);
    let local = result.local_recovery.as_ref().unwrap();
    assert_eq!(local.identity.unique_id, stable_owner_identity_id);
    assert_eq!(local.identity.did, recovered_did);

    let connection = rusqlite::Connection::open(&sqlite_path).unwrap();
    let message_owner: (String, String) = connection
        .query_row(
            "SELECT owner_identity_id, owner_did FROM messages WHERE msg_id=?1",
            ["msg-before-recovery"],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(message_owner.0, stable_owner_identity_id);
    assert_eq!(message_owner.1, recovered_did);
    let group_owner: (String, String) = connection
        .query_row(
            "SELECT owner_identity_id, owner_did FROM groups WHERE group_id=?1",
            ["did:wba:awiki.test:groups:recovery-test"],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(group_owner.0, stable_owner_identity_id);
    assert_eq!(group_owner.1, recovered_did);

    let history = connection
        .prepare(
            "SELECT did, status FROM identity_did_history WHERE owner_identity_id=?1 ORDER BY status, did",
        )
        .unwrap()
        .query_map([stable_owner_identity_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(history.contains(&(previous_did.clone(), "previous".to_string())));
    assert!(history.contains(&(recovered_did.to_string(), "current".to_string())));
    assert_eq!(history.len(), 2);

    let rebind_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM group_rebind_outbox", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(rebind_count, 1);
    let rebind: (String, String, String, String, String, String) = connection
        .query_row(
            r#"SELECT owner_identity_id, group_did, member_handle,
                      previous_member_did, new_member_did, binding_generation
               FROM group_rebind_outbox"#,
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(rebind.0, stable_owner_identity_id);
    assert_eq!(rebind.1, "did:wba:awiki.test:groups:recovery-test");
    assert_eq!(rebind.2, "erin.awiki.test");
    assert_eq!(rebind.3, previous_did);
    assert_eq!(rebind.4, recovered_did);
    assert_eq!(rebind.5, "2");

    let requests = server.join();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].json_body()["method"], "recover_handle");
}

#[tokio::test]
async fn resume_rebind_recovery_reconciles_missing_job_from_authoritative_lookup() {
    let fixture = Fixture::new();
    let current = fixture.write_generated_identity_with_daemon_key("erin", true, true);
    fs::write(
        fixture
            .root
            .join("identities")
            .join("erin-id")
            .join("auth.json"),
        r#"{"jwt_token":"test-token-for-erin","expires_at":"2099-05-21T00:00:00Z"}"#,
    )
    .unwrap();
    let previous_did = "did:wba:awiki.test:erin:e1_previous";
    let group_did = "did:wba:awiki.test:groups:reconcile-test";
    let server = TestServer::spawn(vec![
        ExpectedHttp::json(json!({
            "did": current.did.clone(),
            "user_id": "user-erin",
            "handle": "erin",
            "full_handle": "erin.awiki.test",
            "domain": "awiki.test",
            "status": "active",
            "binding_generation": "3"
        })),
        ExpectedHttp::rpc_result(json!({
            "accepted": true,
            "group_did": group_did,
            "group_state_version": "11"
        })),
        ExpectedHttp::rpc_result(json!({
            "group_snapshot": {
                "group_did": group_did,
                "name": "Recovered group",
                "member_role": "member",
                "member_status": "active",
                "member_count": 3,
                "required_security_profile": "transport-protected"
            }
        })),
    ]);
    let core = fixture.core_async_with_base_url(server.base_url()).await;
    core.bootstrap()
        .initialize_local_state_async()
        .await
        .unwrap();
    let sqlite_path = fixture.root.join("local").join("im.sqlite");
    let db = rusqlite::Connection::open(&sqlite_path).unwrap();
    db.execute(
        r#"INSERT INTO identity_did_history
           (owner_identity_id,did,status,first_seen_at,last_seen_at)
           VALUES ('erin-id',?1,'previous','now','now'),
                  ('erin-id',?2,'current','now','now')"#,
        rusqlite::params![previous_did, &current.did],
    )
    .unwrap();
    db.execute(
        r#"INSERT INTO groups
           (owner_identity_id,owner_did,group_id,group_did,name,my_role,
            membership_status,metadata,stored_at,credential_name)
           VALUES ('erin-id',?1,?2,?2,'Recovered group','member','active',?3,'now','erin-id')"#,
        rusqlite::params![&current.did, group_did, r#"{}"#],
    )
    .unwrap();
    db.execute(
        r#"INSERT INTO group_members
           (owner_identity_id,owner_did,group_id,user_id,member_did,member_handle,
            anchor_kind,anchor_value,handle_binding_generation,role,status,last_synced_at,credential_name)
           VALUES ('erin-id',?1,?2,'user-erin',?3,'erin','handle','erin','2','member','active','now','erin-id')"#,
        rusqlite::params![&current.did, group_did, previous_did],
    )
    .unwrap();
    drop(db);

    let client = core
        .client_async(IdentitySelector::LocalAlias("erin".to_owned()))
        .await
        .unwrap();
    let summary = client
        .groups()
        .resume_rebind_recovery_async(10)
        .await
        .unwrap();

    assert_eq!(summary.processed, 1);
    assert_eq!(summary.completed, 1);
    assert_eq!(summary.pending, 0);
    assert_eq!(summary.blocked, 0);
    assert!(summary
        .warnings
        .iter()
        .any(|warning| warning.contains("reconciled 1 missing")));
    let db = rusqlite::Connection::open(&sqlite_path).unwrap();
    let persisted: (String, String, String) = db
        .query_row(
            "SELECT member_handle,binding_generation,phase FROM group_rebind_outbox",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        persisted,
        (
            "erin.awiki.test".to_owned(),
            "3".to_owned(),
            "complete".to_owned()
        )
    );
    let projected_member: (String, String, String, String) = db
        .query_row(
            r#"SELECT member_did,member_handle,anchor_value,handle_binding_generation
               FROM group_members WHERE owner_identity_id='erin-id' AND group_id=?1"#,
            [group_did],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(
        projected_member,
        (
            current.did.clone(),
            "erin.awiki.test".to_owned(),
            "erin.awiki.test".to_owned(),
            "3".to_owned(),
        )
    );

    let requests = server.join();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].path, "/.well-known/handle/erin");
    assert_eq!(requests[1].path, "/im/rpc");
    let rebind = requests[1].json_body();
    assert_eq!(rebind["method"], "group.rebind_member");
    assert_eq!(rebind["params"]["body"]["member_handle"], "erin.awiki.test");
    assert_eq!(
        rebind["params"]["body"]["previous_member_did"],
        previous_did
    );
    assert_eq!(rebind["params"]["body"]["new_member_did"], current.did);
    assert_eq!(rebind["params"]["body"]["handle_binding_generation"], "3");
    assert!(rebind["params"]["auth"]["origin_proof"].is_object());
    assert_eq!(requests[2].json_body()["method"], "group.get");
}

#[tokio::test]
async fn resume_rebind_recovery_completes_transport_job_after_authoritative_group_refresh() {
    let fixture = Fixture::new();
    let current = fixture.write_generated_identity_with_daemon_key("erin", true, true);
    fs::write(
        fixture
            .root
            .join("identities")
            .join("erin-id")
            .join("auth.json"),
        r#"{"jwt_token":"test-token-for-erin","expires_at":"2099-05-21T00:00:00Z"}"#,
    )
    .unwrap();
    let previous_did = "did:wba:awiki.test:erin:e1_previous";
    let group_did = "did:wba:awiki.test:groups:transport-reconcile-test";
    let server = TestServer::spawn(vec![
        ExpectedHttp::json(json!({
            "did": current.did.clone(),
            "user_id": "user-erin",
            "handle": "erin",
            "full_handle": "erin.awiki.test",
            "domain": "awiki.test",
            "status": "active",
            "binding_generation": "3"
        })),
        ExpectedHttp::rpc_result(json!({
            "group_snapshot": {
                "group_did": group_did,
                "name": "Recovered group",
                "member_role": "member",
                "member_status": "active",
                "member_count": 3,
                "required_security_profile": "transport-protected"
            }
        })),
    ]);
    let core = fixture.core_async_with_base_url(server.base_url()).await;
    core.bootstrap()
        .initialize_local_state_async()
        .await
        .unwrap();
    let sqlite_path = fixture.root.join("local").join("im.sqlite");
    let db = rusqlite::Connection::open(&sqlite_path).unwrap();
    db.execute(
        r#"INSERT INTO identity_did_history
           (owner_identity_id,did,status,first_seen_at,last_seen_at)
           VALUES ('erin-id',?1,'previous','now','now'),
                  ('erin-id',?2,'current','now','now')"#,
        rusqlite::params![previous_did, &current.did],
    )
    .unwrap();
    db.execute(
        r#"INSERT INTO groups
           (owner_identity_id,owner_did,group_id,group_did,name,my_role,
            membership_status,metadata,stored_at,credential_name)
           VALUES ('erin-id',?1,?2,?2,'Recovered group','member','active','{}','now','erin-id')"#,
        rusqlite::params![&current.did, group_did],
    )
    .unwrap();
    db.execute(
        r#"INSERT INTO group_members
           (owner_identity_id,owner_did,group_id,user_id,member_did,member_handle,
            anchor_kind,anchor_value,handle_binding_generation,role,status,last_synced_at,credential_name)
           VALUES ('erin-id',?1,?2,'user-erin',?3,'erin','handle','erin','2','member','active','now','erin-id')"#,
        rusqlite::params![&current.did, group_did, previous_did],
    )
    .unwrap();
    db.execute(
        r#"INSERT INTO group_rebind_outbox
           (job_id,owner_identity_id,group_did,member_handle,previous_member_did,
            new_member_did,binding_generation,phase,created_at,updated_at)
           VALUES ('existing-job','erin-id',?1,'erin.awiki.test',?2,?3,'3','awaiting_p6','now','now')"#,
        rusqlite::params![group_did, previous_did, &current.did],
    )
    .unwrap();
    drop(db);

    let client = core
        .client_async(IdentitySelector::LocalAlias("erin".to_owned()))
        .await
        .unwrap();
    let summary = client
        .groups()
        .resume_rebind_recovery_async(10)
        .await
        .unwrap();

    assert_eq!(summary.processed, 1);
    assert_eq!(summary.completed, 1);
    assert_eq!(summary.pending, 0);
    assert_eq!(summary.blocked, 0);
    assert!(summary.send_paused_groups.is_empty());
    let db = rusqlite::Connection::open(&sqlite_path).unwrap();
    let persisted: (String, String) = db
        .query_row(
            "SELECT phase,metadata FROM group_rebind_outbox JOIN groups USING(group_did) WHERE job_id='existing-job'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(persisted.0, "complete");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&persisted.1).unwrap()
            ["required_security_profile"],
        "transport-protected"
    );

    let requests = server.join();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[1].json_body()["method"], "group.get");
}

#[tokio::test]
async fn recover_handle_async_with_otp_persists_daemon_subkey_package() {
    let server = TestServer::spawn(vec![ExpectedHttp::rpc_result(json!({
        "user_id": "user-erin",
        "handle": "erin",
        "full_handle": "erin.awiki.test",
        "access_token": "jwt-erin"
    }))]);
    let fixture = Fixture::new();
    let base_url = server.base_url();
    let core = fixture.core_async_with_base_url(base_url).await;

    let result = core
        .identities()
        .recover_handle_async(RecoverHandleRequest {
            handle: Handle::parse("erin", "").unwrap(),
            raw_handle: None,
            phone: "+15551234567".to_string(),
            otp: Some("654321".to_string()),
            generated_identity: None,
            local_finalize: None,
        })
        .await
        .unwrap();

    let recovered = result.recovered_identity.unwrap();
    let package = core
        .identities()
        .load_daemon_subkey_package_async(IdentitySelector::LocalAlias("erin".to_string()))
        .await
        .unwrap();
    assert_eq!(package.user_did, recovered.identity.did);
    assert_eq!(
        package.verification_method,
        format!("{}#daemon-key-1", recovered.identity.did.as_str())
    );

    let requests = server.join();
    let did_document = &requests[0].json_body()["params"]["did_document"];
    assert_eq!(did_document["id"].as_str(), Some(package.user_did.as_str()));
    assert!(did_document["authentication"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item.as_str() == Some(package.verification_method.as_str())));
    let method = did_document["verificationMethod"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"].as_str() == Some(package.verification_method.as_str()))
        .unwrap();
    assert_eq!(
        method["publicKeyMultibase"].as_str(),
        Some(package.public_key_multibase.as_str())
    );
}

#[tokio::test]
async fn recover_handle_vault_required_persists_identity_without_plaintext() {
    let server = TestServer::spawn(vec![ExpectedHttp::rpc_result(json!({
        "user_id": "user-secure-recover",
        "handle": "secure-recover",
        "full_handle": "secure-recover.awiki.test",
        "access_token": "jwt-secure-recover"
    }))]);
    let fixture = Fixture::new();
    let base_url = server.base_url().to_owned();
    let core = fixture
        .core_async_with_base_url_vault_required(&base_url, [43_u8; 32])
        .await;

    let result = core
        .identities()
        .recover_handle_async(RecoverHandleRequest {
            handle: Handle::parse("secure-recover", "").unwrap(),
            raw_handle: None,
            phone: "+15551234567".to_string(),
            otp: Some("654321".to_string()),
            generated_identity: None,
            local_finalize: None,
        })
        .await
        .unwrap();

    let recovered = result.recovered_identity.unwrap();
    let status = core
        .identities()
        .vault_status_async(IdentitySelector::LocalAlias("secure-recover".to_string()))
        .await
        .unwrap();
    assert_eq!(status.selected_backend, IdentitySecretStorageBackend::Vault);
    assert!(status.vault_metadata_verified);
    assert_eq!(status.plaintext_compat_retained, Some(false));
    assert_secure_identity_dir_has_no_plaintext(
        &fixture
            .root
            .join("identities")
            .join(recovered.identity.id.as_str()),
        "jwt-secure-recover",
    );

    let requests = server.join();
    assert_eq!(requests.len(), 1);
}

#[tokio::test]
async fn ensure_daemon_subkey_package_updates_signed_did_document_for_legacy_identity() {
    let fixture = Fixture::new();
    let identity = fixture.write_generated_identity_without_daemon_key("legacy", true);
    let server = TestServer::spawn(vec![ExpectedHttp::rpc_result(json!({
        "did": identity.did,
        "user_id": "user-legacy",
        "message": "DID document updated",
        "access_token": "jwt-legacy-updated"
    }))]);
    let base_url = server.base_url().to_owned();
    let core = fixture.core_async_with_base_url(&base_url).await;

    let package = core
        .identities()
        .ensure_daemon_subkey_package_async(IdentitySelector::LocalAlias("legacy".to_string()))
        .await
        .unwrap();

    assert_eq!(package.user_did.as_str(), identity.did);
    assert_eq!(
        package.verification_method,
        format!("{}#daemon-key-1", identity.did)
    );
    let requests = server.join();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].path, "/user-service/did-auth/rpc");
    let body = requests[0].json_body();
    assert_eq!(body["method"], "update_document");
    assert_eq!(body["params"].get("is_public"), None);
    assert_eq!(body["params"].get("is_agent"), None);
    let did_document = &body["params"]["did_document"];
    assert_eq!(did_document["id"].as_str(), Some(identity.did.as_str()));
    assert_ne!(
        did_document["proof"]["challenge"].as_str(),
        identity.did_document["proof"]["challenge"].as_str()
    );
    assert!(did_document["authentication"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item.as_str() == Some(package.verification_method.as_str())));
    let method = did_document["verificationMethod"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"].as_str() == Some(package.verification_method.as_str()))
        .unwrap();
    assert_eq!(
        method["publicKeyMultibase"].as_str(),
        Some(package.public_key_multibase.as_str())
    );
    let proof_method = did_document["proof"]["verificationMethod"]
        .as_str()
        .expect("proof method");
    let proof_public_key = did_document["verificationMethod"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"].as_str() == Some(proof_method))
        .and_then(|item| extract_public_key(item).ok())
        .expect("proof public key");
    assert!(verify_w3c_proof(
        did_document,
        &proof_public_key,
        ProofVerificationOptions::default()
    ));

    let saved = fs::read_to_string(
        fixture
            .root
            .join("identities")
            .join("legacy-id")
            .join("daemon-subkey-package.json"),
    )
    .unwrap();
    assert!(saved.contains("daemon-key-1"));
}

#[tokio::test]
async fn revoke_daemon_subkey_authorization_updates_signed_did_document() {
    let fixture = Fixture::new();
    let identity = fixture.write_generated_identity_with_daemon_key("revokee", true, true);
    let server = TestServer::spawn(vec![ExpectedHttp::rpc_result(json!({
        "did": identity.did,
        "user_id": "user-revokee",
        "message": "DID document updated",
        "access_token": "jwt-revokee-updated"
    }))]);
    let base_url = server.base_url().to_owned();
    let core = fixture.core_async_with_base_url(&base_url).await;

    let result = core
        .identities()
        .revoke_daemon_subkey_authorization_async(IdentitySelector::LocalAlias(
            "revokee".to_string(),
        ))
        .await
        .unwrap();

    assert!(result.updated);
    assert_eq!(result.user_did.as_str(), identity.did);
    assert_eq!(
        result.verification_method,
        format!("{}#daemon-key-1", identity.did)
    );
    let requests = server.join();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].path, "/user-service/did-auth/rpc");
    let body = requests[0].json_body();
    assert_eq!(body["method"], "update_document");
    let did_document = &body["params"]["did_document"];
    assert_eq!(did_document["id"].as_str(), Some(identity.did.as_str()));
    assert_ne!(
        did_document["proof"]["challenge"].as_str(),
        identity.did_document["proof"]["challenge"].as_str()
    );
    assert!(!did_document_references(
        &result.verification_method,
        did_document
    ));
    let proof_method = did_document["proof"]["verificationMethod"]
        .as_str()
        .expect("proof method");
    let proof_public_key = did_document["verificationMethod"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"].as_str() == Some(proof_method))
        .and_then(|item| extract_public_key(item).ok())
        .expect("proof public key");
    assert!(verify_w3c_proof(
        did_document,
        &proof_public_key,
        ProofVerificationOptions::default()
    ));

    let saved = fs::read_to_string(
        fixture
            .root
            .join("identities")
            .join("revokee-id")
            .join("did_document.json"),
    )
    .unwrap();
    let saved_document: Value = serde_json::from_str(&saved).unwrap();
    assert!(!did_document_references(
        &result.verification_method,
        &saved_document
    ));
}

#[tokio::test]
async fn revoke_daemon_subkey_authorization_is_idempotent_when_key_is_absent() {
    let fixture = Fixture::new();
    let identity = fixture.write_generated_identity_without_daemon_key("already", true);
    let server = TestServer::spawn(Vec::new());
    let base_url = server.base_url().to_owned();
    let core = fixture.core_async_with_base_url(&base_url).await;

    let result = core
        .identities()
        .revoke_daemon_subkey_authorization_async(IdentitySelector::LocalAlias(
            "already".to_string(),
        ))
        .await
        .unwrap();

    assert!(!result.updated);
    assert_eq!(result.user_did.as_str(), identity.did);
    assert_eq!(
        result.verification_method,
        format!("{}#daemon-key-1", identity.did)
    );
    assert!(server.join().is_empty());
}

#[test]
fn ensure_daemon_subkey_package_fails_closed_when_document_has_daemon_key_without_private_package()
{
    let fixture = Fixture::new();
    fixture.write_generated_identity_with_daemon_key_without_package("stale");
    let core = fixture.core();

    let err = core
        .identities()
        .ensure_daemon_subkey_package(IdentitySelector::LocalAlias("stale".to_string()))
        .unwrap_err();

    assert!(err.to_string().contains("daemon_subkey_private_missing"));
}

#[tokio::test]
async fn register_phone_without_otp_returns_pending_otp_state() {
    let server = TestServer::spawn(vec![ExpectedHttp::rpc_result(json!({ "sent": true }))]);
    let fixture = Fixture::new();
    let base_url = server.base_url().to_owned();
    let core = fixture.core_async_with_base_url(&base_url).await;

    let result = core
        .identities()
        .register_handle_async(RegisterHandleRequest {
            local_alias: Some("carol".to_string()),
            requested_handle: Handle::parse("carol.awiki.test", "").unwrap(),
            verification: VerificationInput::Phone {
                phone: "+15551234567".to_string(),
                otp: None,
            },
            invite_code: Some("invite-1".to_string()),
            profile: InitialProfile {
                display_name: Some("Carol".to_string()),
                avatar_url: None,
            },
            make_default: true,
        })
        .await
        .unwrap();

    assert_eq!(result.state, HandleRegistrationState::OtpSent);
    assert_eq!(result.method, RegistrationMethod::Phone);
    assert_eq!(result.handle.as_str(), "carol.awiki.test");
    assert!(result.identity.is_none());
    assert!(result.default_identity_change.is_none());

    let requests = server.join();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/user-service/handle/rpc");
    let body = requests[0].json_body();
    assert_eq!(body["method"], "send_otp");
    assert_eq!(body["params"], json!({ "phone": "+15551234567" }));
}

#[tokio::test]
async fn vnext_register_phone_without_otp_uses_sms_endpoint_and_requires_vault() {
    let fixture = Fixture::new();
    let core_without_vault = fixture
        .core_async_with_base_url_multi_device("https://example.test")
        .await;
    let request = vnext_phone_request("carol", None);
    assert!(matches!(
        core_without_vault
            .identities()
            .register_handle_async(request.clone())
            .await,
        Err(ImError::LocalStateUnavailable { .. })
    ));

    let server = TestServer::spawn(vec![ExpectedHttp::json(json!({"sent": true}))]);
    let core = fixture
        .core_async_with_base_url_vault_required_multi_device(server.base_url(), [51; 32])
        .await;
    let result = core
        .identities()
        .register_handle_async(request)
        .await
        .unwrap();
    assert_eq!(result.state, HandleRegistrationState::OtpSent);
    let requests = server.join();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].path, "/user-service/auth/sms-codes");
    assert_eq!(requests[0].json_body(), json!({"phone": "+15551234567"}));
}

#[tokio::test]
async fn vnext_genesis_persists_admin_identity_and_rotating_tokens_in_vault() {
    let server = TestServer::spawn(vec![
        ExpectedHttp::json(json!({
            "account_verification_token": "account-grant-secret",
            "purpose": "awiki.device.genesis.v1",
            "expires_at": future_rfc3339(300),
        })),
        ExpectedHttp::dynamic_rpc(genesis_response_from_request),
    ]);
    let fixture = Fixture::new();
    let root_key = [52; 32];
    let core = fixture
        .core_async_with_base_url_vault_required_multi_device(server.base_url(), root_key)
        .await;
    let result = core
        .identities()
        .register_handle_async(vnext_phone_request("carol", Some("123456")))
        .await
        .unwrap();

    assert_eq!(result.state, HandleRegistrationState::Registered);
    let identity = result.identity.as_ref().unwrap();
    assert!(identity
        .did
        .as_str()
        .starts_with("did:wba:awiki.test:user:carol:e1_"));
    assert!(identity
        .device_id
        .as_deref()
        .is_some_and(|value| value.starts_with("dev-")));
    let device = core
        .identities()
        .device_summary_async(IdentitySelector::LocalAlias("carol".to_owned()))
        .await
        .unwrap();
    assert_eq!(device.mode, IdentityDeviceMode::VNext);
    assert_eq!(device.role, Some(IdentityDeviceRole::Admin));
    assert_eq!(device.readiness, IdentityDeviceReadiness::AdminReady);

    let requests = server.join();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[0].path,
        "/user-service/auth/account-verification/exchange"
    );
    let exchange = requests[0].json_body();
    assert_eq!(exchange["provider"], "sms");
    assert_eq!(exchange["purpose"], "awiki.device.genesis.v1");
    assert_eq!(exchange["target_handle"], "carol");
    assert_eq!(exchange["target_handle_domain"], "awiki.test");
    assert!(exchange["idempotency_scope"].as_str().is_some());
    assert_eq!(requests[1].path, "/user-service/did-auth/rpc");
    let genesis = requests[1].json_body();
    assert_eq!(genesis["method"], "device_genesis");
    assert_eq!(
        genesis["params"]["account_verification_token"],
        "account-grant-secret"
    );
    assert_eq!(
        genesis["params"]["bootstrap_device_proof"]["key_id"],
        genesis["params"]["did_document"]["deviceManifest"]["devices"][0]["signing_key_id"]
    );
    verify_genesis_request_proof(&genesis["params"]);

    assert_secure_identity_dir_has_no_plaintext(
        &fixture.root.join("identities").join(identity.id.as_str()),
        "device-access-secret",
    );
    assert_secure_identity_dir_has_no_plaintext(
        &fixture.root.join("identities").join(identity.id.as_str()),
        "device-refresh-secret",
    );
    let vault = FileSecretVault::new(
        DeviceVaultRootKey::from_bytes(root_key),
        FileSecretVaultStore::new(fixture.root.join("identity-vault")),
    );
    let refs = vault.list().unwrap();
    assert!(!refs
        .iter()
        .any(|secret_ref| secret_ref.kind == SecretKind::IdentityGenesisPending));
    let auth_ref = refs
        .iter()
        .find(|secret_ref| secret_ref.kind == SecretKind::AuthJwt)
        .unwrap();
    let auth: Value =
        serde_json::from_slice(vault.open(auth_ref).unwrap().expose_secret()).unwrap();
    assert!(auth["jwt_token"]
        .as_str()
        .unwrap()
        .ends_with(".device-access-secret"));
    assert!(auth["refresh_token"]
        .as_str()
        .unwrap()
        .ends_with(".device-refresh-secret"));
}

#[tokio::test]
async fn vnext_genesis_retries_exact_rpc_without_recreating_keys_or_proof() {
    let server = TestServer::spawn(vec![
        ExpectedHttp::json(json!({
            "account_verification_token": "account-grant-secret",
            "purpose": "awiki.device.genesis.v1",
            "expires_at": future_rfc3339(300),
        })),
        ExpectedHttp::status(503, json!({"error": "temporary"})),
        ExpectedHttp::dynamic_rpc(genesis_response_from_request),
    ]);
    let fixture = Fixture::new();
    let core = fixture
        .core_async_with_base_url_vault_required_multi_device(server.base_url(), [53; 32])
        .await;
    let request = vnext_phone_request("retry", Some("123456"));
    assert!(core
        .identities()
        .register_handle_async(request.clone())
        .await
        .is_err());
    let result = core
        .identities()
        .register_handle_async(request)
        .await
        .unwrap();
    assert_eq!(result.state, HandleRegistrationState::Registered);

    let requests = server.join();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[1].body, requests[2].body);
    assert_eq!(requests[1].json_body()["method"], "device_genesis");
}

#[tokio::test]
async fn vnext_remote_success_survives_local_commit_failure_and_resumes_offline() {
    let fixture = Fixture::new();
    let identities_dir = fixture.root.join("identities");
    let blocked_backup = fixture.root.join("identities-blocked-backup");
    let response_identities_dir = identities_dir.clone();
    let response_blocked_backup = blocked_backup.clone();
    let server = TestServer::spawn(vec![
        ExpectedHttp::json(json!({
            "account_verification_token": "account-grant-secret",
            "purpose": "awiki.device.genesis.v1",
            "expires_at": future_rfc3339(300),
        })),
        ExpectedHttp::dynamic_rpc(move |request| {
            let response = genesis_response_from_request(request);
            fs::rename(&response_identities_dir, &response_blocked_backup).unwrap();
            fs::write(&response_identities_dir, b"block identity directory").unwrap();
            response
        }),
    ]);
    let core = fixture
        .core_async_with_base_url_vault_required_multi_device(server.base_url(), [54; 32])
        .await;
    let request = vnext_phone_request("resume", Some("123456"));
    let first = core
        .identities()
        .register_handle_async(request.clone())
        .await;
    fs::remove_file(&identities_dir).unwrap();
    fs::rename(&blocked_backup, &identities_dir).unwrap();
    assert!(first.is_err());
    assert_eq!(server.join().len(), 2);

    // The remote result is in the Vault pending record, so this retry performs
    // only the local commit even though the test server has already stopped.
    let resumed = core
        .identities()
        .register_handle_async(request)
        .await
        .unwrap();
    assert_eq!(resumed.state, HandleRegistrationState::Registered);
}

#[tokio::test]
async fn vnext_genesis_rejects_cross_domain_and_non_phone_verification() {
    let fixture = Fixture::new();
    let core = fixture
        .core_async_with_base_url_vault_required_multi_device("https://example.test", [55; 32])
        .await;
    let mut cross_domain = vnext_phone_request("carol", Some("123456"));
    cross_domain.requested_handle = Handle::parse("carol.other.test", "").unwrap();
    assert!(matches!(
        core.identities().register_handle_async(cross_domain).await,
        Err(ImError::UnsupportedCapability { .. })
    ));
    let mut unsupported = vnext_phone_request("carol", Some("123456"));
    unsupported.verification = VerificationInput::AlreadyVerified;
    assert!(matches!(
        core.identities().register_handle_async(unsupported).await,
        Err(ImError::UnsupportedCapability { .. })
    ));
}

#[tokio::test]
async fn register_email_without_wait_returns_email_sent_state() {
    let server = TestServer::spawn(vec![
        ExpectedHttp::json(json!({ "verified": false })),
        ExpectedHttp::json(json!({ "sent": true })),
    ]);
    let fixture = Fixture::new();
    let base_url = server.base_url().to_owned();
    let core = fixture.core_async_with_base_url(&base_url).await;

    let result = core
        .identities()
        .register_handle_async(RegisterHandleRequest {
            local_alias: Some("carol".to_string()),
            requested_handle: Handle::parse("carol.awiki.test", "").unwrap(),
            verification: VerificationInput::Email {
                email: "carol@example.test".to_string(),
                wait_for_verification: false,
            },
            invite_code: None,
            profile: InitialProfile {
                display_name: Some("Carol".to_string()),
                avatar_url: None,
            },
            make_default: true,
        })
        .await
        .unwrap();

    assert_eq!(result.state, HandleRegistrationState::EmailSent);
    assert_eq!(result.method, RegistrationMethod::Email);
    assert_eq!(result.handle.as_str(), "carol.awiki.test");
    assert!(result.identity.is_none());
    assert!(result.default_identity_change.is_none());

    let requests = server.join();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(
        requests[0].path,
        "/user-service/auth/email-status?email=carol%40example.test&handle=carol.awiki.test"
    );
    assert_eq!(requests[1].method, "POST");
    assert_eq!(requests[1].path, "/user-service/auth/email-send");
    assert_eq!(
        requests[1].json_body(),
        json!({ "email": "carol@example.test", "handle": "carol.awiki.test" })
    );
}

#[test]
fn auth_service_returns_stable_structures() {
    let fixture = Fixture::new();
    let core = fixture.core();
    let client = core
        .client(IdentitySelector::LocalAlias("alice".to_string()))
        .unwrap();

    let login = client.auth().login().unwrap();
    assert_eq!(login.subject.as_str(), "did:example:alice");
    assert_eq!(login.scope, AuthScope::UserProfile);

    let ensured = client.auth().ensure_session(AuthScope::Messaging).unwrap();
    assert_eq!(ensured.subject.as_str(), "did:example:alice");
    assert_eq!(ensured.scope, AuthScope::Messaging);

    let status = client.auth().status().unwrap();
    assert_eq!(status.subject.as_str(), "did:example:alice");
    assert!(status.has_session);
    assert!(!status.needs_refresh);
}

#[tokio::test]
async fn async_open_client_bootstrap_and_auth_use_async_entrypoints() {
    let fixture = Fixture::new();
    let core = fixture.core_async().await;

    let path_report = core.bootstrap().validate_paths_async().await.unwrap();
    assert_eq!(path_report.checked.len(), 6);

    let client = core
        .client_async(IdentitySelector::LocalAlias("alice".to_string()))
        .await
        .unwrap();
    let login = client.auth().login_async().await.unwrap();
    assert_eq!(login.subject.as_str(), "did:example:alice");
    assert_eq!(login.scope, AuthScope::UserProfile);

    let ensured = client
        .auth()
        .ensure_session_async(AuthScope::Messaging)
        .await
        .unwrap();
    assert_eq!(ensured.subject.as_str(), "did:example:alice");
    assert_eq!(ensured.scope, AuthScope::Messaging);

    let status = client.auth().status_async().await.unwrap();
    assert_eq!(status.subject.as_str(), "did:example:alice");
    assert!(status.has_session);
    assert!(!status.needs_refresh);
}

#[tokio::test]
async fn async_bootstrap_initializes_and_migrates_local_state() {
    let fixture = Fixture::new();
    let core = fixture.core_async().await;

    let status = core
        .bootstrap()
        .initialize_local_state_async()
        .await
        .unwrap();
    assert!(status.initialized);
    assert!(status.schema_version.is_some());

    let report = core.bootstrap().migrate_local_state_async().await.unwrap();
    assert_eq!(report.sqlite_path, status.sqlite_path);
    assert_eq!(report.to_version, status.schema_version.unwrap_or_default());
}

fn vnext_phone_request(handle: &str, otp: Option<&str>) -> RegisterHandleRequest {
    RegisterHandleRequest {
        local_alias: Some(handle.to_owned()),
        requested_handle: Handle::parse(format!("{handle}.awiki.test"), "").unwrap(),
        verification: VerificationInput::Phone {
            phone: "+15551234567".to_owned(),
            otp: otp.map(ToOwned::to_owned),
        },
        invite_code: None,
        profile: InitialProfile {
            display_name: Some(handle.to_owned()),
            avatar_url: None,
        },
        make_default: true,
    }
}

fn future_rfc3339(after_seconds: i64) -> String {
    (time::OffsetDateTime::now_utc() + time::Duration::seconds(after_seconds))
        .replace_nanosecond(0)
        .unwrap()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap()
}

fn genesis_response_from_request(request: &CapturedHttp) -> Value {
    let body = request.json_body();
    assert_eq!(body["method"], "device_genesis");
    let params = &body["params"];
    let document = &params["did_document"];
    let did = document["id"].as_str().unwrap();
    let device = &document["deviceManifest"]["devices"][0];
    let device_id = params["bootstrap_device_id"].as_str().unwrap();
    assert_eq!(device["device_id"], device_id);
    let signing_key_id = device["signing_key_id"].as_str().unwrap();
    let e2ee_key_id = device["e2ee_key_id"].as_str().unwrap();
    let canonical = serde_json_canonicalizer::to_vec(document).unwrap();
    let document_hash = format!(
        "sha256:{}",
        URL_SAFE_NO_PAD.encode(Sha256::digest(canonical))
    );
    let access_exp = time::OffsetDateTime::now_utc()
        .replace_nanosecond(0)
        .unwrap()
        + time::Duration::hours(1);
    let refresh_exp = access_exp + time::Duration::days(7);
    let access_token = server_device_token(
        did,
        device_id,
        signing_key_id,
        "access",
        "awiki.device.access.v1",
        access_exp,
        "device-access-secret",
    );
    let refresh_token = server_device_token(
        did,
        device_id,
        signing_key_id,
        "refresh",
        "awiki.device.refresh.v1",
        refresh_exp,
        "device-refresh-secret",
    );
    json!({
        "jsonrpc": "2.0",
        "id": "req-1",
        "result": {
            "did": did,
            "user_id": "user-genesis",
            "checkpoint": {
                "document_version": 1,
                "document_hash": document_hash,
                "registry_version": 1,
            },
            "device": {
                "device_id": device_id,
                "signing_key_id": signing_key_id,
                "e2ee_key_id": e2ee_key_id,
                "status": "active",
                "role": "admin",
                "management_ready": true,
                "auth_generation": 1,
            },
            "access_token": access_token,
            "refresh_token": refresh_token,
            "token_expires_at": access_exp
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap(),
        }
    })
}

fn server_device_token(
    did: &str,
    device_id: &str,
    signing_key_id: &str,
    token_type: &str,
    purpose: &str,
    expires_at: time::OffsetDateTime,
    signature: &str,
) -> String {
    let issued_at = time::OffsetDateTime::now_utc().unix_timestamp();
    let payload = json!({
        "profile": "awiki-device-token-v1",
        "purpose": purpose,
        "type": token_type,
        "sub": did,
        "did": did,
        "user_id": "user-genesis",
        "device_id": device_id,
        "key_id": signing_key_id,
        "auth_generation": 1,
        "jti": format!("jti-{token_type}"),
        "iat": issued_at,
        "nbf": issued_at,
        "scopes": ["device:manage", "device:read", "message:connect"],
        "exp": expires_at.unix_timestamp(),
    });
    format!(
        "e30.{}.{signature}",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).unwrap())
    )
}

fn verify_genesis_request_proof(params: &Value) {
    let proof = &params["bootstrap_device_proof"];
    let signed_params = json!({
        "operation_id": params["operation_id"],
        "did_document": params["did_document"],
        "bootstrap_device_id": params["bootstrap_device_id"],
    });
    let transcript = json!({
        "type": proof["type"],
        "purpose": "awiki.device.genesis.v1",
        "method": "device_genesis",
        "key_id": proof["key_id"],
        "created_at": proof["created_at"],
        "expires_at": proof["expires_at"],
        "nonce": proof["nonce"],
        "params": signed_params,
    });
    let bytes = serde_json_canonicalizer::to_vec(&transcript).unwrap();
    let key_id = proof["key_id"].as_str().unwrap();
    let method = params["did_document"]["verificationMethod"]
        .as_array()
        .unwrap()
        .iter()
        .find(|method| method["id"].as_str() == Some(key_id))
        .unwrap();
    anp::authentication::create_verification_method(method)
        .unwrap()
        .verify_signature(&bytes, proof["signature"].as_str().unwrap())
        .unwrap();
}

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = unique_temp_root();
        let identities = root.join("identities");
        fs::create_dir_all(&identities).unwrap();
        fs::write(identities.join("default"), "alice\n").unwrap();
        fs::write(
            identities.join("registry.json"),
            r#"{
              "default_identity": "alice",
              "identities": [
                {
                  "id": "alice-id",
                  "did": "did:example:alice",
                  "handle": "alice.awiki.test",
                  "display_name": "Alice",
                  "local_alias": "alice",
                  "device_id": "device-a",
                  "ready_for_auth": true,
                  "ready_for_messaging": true,
                  "missing": []
                },
                {
                  "id": "bob-id",
                  "did": "did:example:bob",
                  "handle": "bob.awiki.test",
                  "display_name": "Bob",
                  "local_alias": "bob",
                  "ready_for_auth": true,
                  "ready_for_messaging": true,
                  "missing": []
                }
              ]
            }"#,
        )
        .unwrap();
        write_identity_runtime(
            &identities,
            "alice",
            "did:example:alice",
            "2099-05-21T00:00:00Z",
        );
        write_identity_runtime(
            &identities,
            "bob",
            "did:example:bob",
            "2099-05-21T00:00:00Z",
        );
        Self { root }
    }

    fn core(&self) -> ImCore {
        self.core_with_base_url("https://example.test")
    }

    fn core_with_base_url(&self, base_url: &str) -> ImCore {
        ImCore::new(
            ImCoreConfig {
                service_base_url: ServiceEndpoint::parse(base_url).unwrap(),
                did_domain: "awiki.test".to_string(),
                user_service_endpoint: None,
                message_service_endpoint: None,
                mail_service_endpoint: None,
                anp_service_endpoint: None,
                anp_service_did: None,
                ca_bundle: None,
                transport_policy: MessageTransportPolicy::HttpOnly,
            },
            ImCorePaths {
                identities: IdentityRegistryPaths {
                    identity_root_dir: self.root.join("identities"),
                    registry_path: self.root.join("identities").join("registry.json"),
                    default_identity_path: Some(self.root.join("identities").join("default")),
                },
                local_state: LocalStatePaths {
                    sqlite_path: self.root.join("local").join("im.sqlite"),
                },
                runtime: RuntimePaths {
                    cache_dir: self.root.join("cache"),
                    temp_dir: self.root.join("tmp"),
                },
            },
        )
        .unwrap()
    }

    async fn core_async(&self) -> ImCore {
        self.core_async_with_base_url("https://example.test").await
    }

    async fn core_async_with_base_url(&self, base_url: &str) -> ImCore {
        ImCore::open(self.config(base_url), self.paths())
            .await
            .unwrap()
    }

    async fn core_async_with_base_url_multi_device(&self, base_url: &str) -> ImCore {
        ImCore::open_with_options(
            self.config(base_url),
            self.paths(),
            ImCoreOpenOptions::default().with_multi_device_join_enabled(true),
        )
        .await
        .unwrap()
    }

    async fn core_async_with_base_url_vault_required(
        &self,
        base_url: &str,
        root_key: [u8; 32],
    ) -> ImCore {
        ImCore::open_with_options(
            self.config(base_url),
            self.paths(),
            ImCoreOpenOptions::default().with_identity_secret_vault(
                IdentitySecretStoragePolicy::VaultRequired,
                ImCoreSecretVaultOptions::new(
                    DeviceVaultRootKey::from_bytes(root_key),
                    self.root.join("identity-vault"),
                    "phase1c-workspace",
                    "phase1c-device",
                ),
            ),
        )
        .await
        .unwrap()
    }

    async fn core_async_with_base_url_vault_required_multi_device(
        &self,
        base_url: &str,
        root_key: [u8; 32],
    ) -> ImCore {
        ImCore::open_with_options(
            self.config(base_url),
            self.paths(),
            ImCoreOpenOptions::default()
                .with_identity_secret_vault(
                    IdentitySecretStoragePolicy::VaultRequired,
                    ImCoreSecretVaultOptions::new(
                        DeviceVaultRootKey::from_bytes(root_key),
                        self.root.join("identity-vault"),
                        "phase1c-workspace",
                        "phase1c-device",
                    ),
                )
                .with_multi_device_join_enabled(true),
        )
        .await
        .unwrap()
    }

    fn write_generated_identity_without_daemon_key(
        &self,
        alias: &str,
        make_default: bool,
    ) -> GeneratedTestIdentity {
        let generated = generated_test_identity(alias);
        write_generated_identity(
            &self.root.join("identities"),
            alias,
            &generated,
            make_default,
            false,
            false,
        );
        generated
    }

    fn write_generated_identity_with_daemon_key_without_package(
        &self,
        alias: &str,
    ) -> GeneratedTestIdentity {
        self.write_generated_identity_with_daemon_key(alias, false, false)
    }

    fn write_generated_identity_with_daemon_key(
        &self,
        alias: &str,
        make_default: bool,
        include_daemon_package: bool,
    ) -> GeneratedTestIdentity {
        let generated = generated_test_identity(alias);
        write_generated_identity(
            &self.root.join("identities"),
            alias,
            &generated,
            make_default,
            true,
            include_daemon_package,
        );
        generated
    }

    fn config(&self, base_url: &str) -> ImCoreConfig {
        ImCoreConfig {
            service_base_url: ServiceEndpoint::parse(base_url).unwrap(),
            did_domain: "awiki.test".to_string(),
            user_service_endpoint: None,
            message_service_endpoint: None,
            mail_service_endpoint: None,
            anp_service_endpoint: None,
            anp_service_did: None,
            ca_bundle: None,
            transport_policy: MessageTransportPolicy::HttpOnly,
        }
    }

    fn paths(&self) -> ImCorePaths {
        ImCorePaths {
            identities: IdentityRegistryPaths {
                identity_root_dir: self.root.join("identities"),
                registry_path: self.root.join("identities").join("registry.json"),
                default_identity_path: Some(self.root.join("identities").join("default")),
            },
            local_state: LocalStatePaths {
                sqlite_path: self.root.join("local").join("im.sqlite"),
            },
            runtime: RuntimePaths {
                cache_dir: self.root.join("cache"),
                temp_dir: self.root.join("tmp"),
            },
        }
    }
}

fn did_document_references(verification_method: &str, did_document: &Value) -> bool {
    serde_json::to_string(did_document)
        .unwrap()
        .contains(verification_method)
}

fn assert_secure_identity_dir_has_no_plaintext(identity_dir: &Path, token_marker: &str) {
    for name in [
        "auth.json",
        "private.key",
        "key-1-private.pem",
        "key-3-private.pem",
        "e2ee-signing-private.pem",
        "e2ee-agreement-private.pem",
        "daemon-key-1-private.pem",
    ] {
        assert!(
            !identity_dir.join(name).exists(),
            "{} should not exist in secure identity dir",
            identity_dir.join(name).display()
        );
    }
    let package_path = identity_dir.join("daemon-subkey-package.json");
    if package_path.exists() {
        let package_text = fs::read_to_string(&package_path).unwrap();
        assert!(package_text.contains(r#""private_key_storage": "vault""#));
        assert!(!package_text.contains("private_key_pem"));
        assert!(!package_text.contains("private_key_multibase"));
        assert!(!package_text.contains("-----BEGIN PRIVATE KEY-----"));
    }
    let persisted_text = collect_text_files(identity_dir);
    for marker in [
        token_marker,
        "-----BEGIN PRIVATE KEY-----",
        "private_key_pem",
        "private_key_multibase",
    ] {
        assert!(
            !persisted_text.contains(marker),
            "secure identity dir leaked marker {marker}"
        );
    }
}

fn collect_text_files(root: &Path) -> String {
    let mut out = String::new();
    collect_text_files_inner(root, &mut out);
    out
}

fn collect_text_files_inner(root: &Path, out: &mut String) {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if metadata.is_dir() {
            collect_text_files_inner(&path, out);
        } else if metadata.is_file() {
            if let Ok(text) = fs::read_to_string(&path) {
                out.push_str(&text);
                out.push('\n');
            }
        }
    }
}

fn write_identity_runtime(identities: &std::path::Path, alias: &str, did: &str, expires_at: &str) {
    let identity_dir = identities.join(alias);
    fs::create_dir_all(&identity_dir).unwrap();
    fs::write(
        identity_dir.join("did.json"),
        format!(r#"{{"id":"{did}","controller":"{did}"}}"#),
    )
    .unwrap();
    fs::write(
        identity_dir.join("private.key"),
        format!("test-private-key-for-{alias}\n"),
    )
    .unwrap();
    fs::write(
        identity_dir.join("auth.json"),
        format!(r#"{{"jwt_token":"test-token-for-{alias}","expires_at":"{expires_at}"}}"#),
    )
    .unwrap();
}

struct GeneratedTestIdentity {
    did: String,
    did_document: Value,
    key1_private_pem: String,
    key1_public_pem: String,
}

fn generated_test_identity(alias: &str) -> GeneratedTestIdentity {
    let bundle = create_did_wba_document(
        "awiki.test",
        DidDocumentOptions {
            path_segments: vec![alias.to_string(), format!("e1_{alias}")],
            domain: Some("awiki.test".to_string()),
            challenge: Some(format!("phase1c-{alias}")),
            did_profile: anp::authentication::DidProfile::E1,
            ..DidDocumentOptions::default()
        },
    )
    .unwrap();
    GeneratedTestIdentity {
        did: bundle.did().unwrap().to_string(),
        did_document: bundle.did_document.clone(),
        key1_private_pem: bundle.private_key_pem("key-1").unwrap().to_string(),
        key1_public_pem: bundle.public_key_pem("key-1").unwrap().to_string(),
    }
}

fn write_generated_identity(
    identities: &Path,
    alias: &str,
    generated: &GeneratedTestIdentity,
    make_default: bool,
    include_daemon_key: bool,
    include_daemon_package: bool,
) {
    let dir_name = format!("{alias}-id");
    let identity_dir = identities.join(&dir_name);
    fs::create_dir_all(&identity_dir).unwrap();
    let mut did_document = generated.did_document.clone();
    let daemon_package = if include_daemon_key {
        let private_key = anp::PrivateKeyMaterial::Ed25519(ed25519_dalek::SigningKey::generate(
            &mut rand::rngs::OsRng,
        ));
        let public_key = match private_key.public_key() {
            anp::PublicKeyMaterial::Ed25519(key) => {
                let mut bytes = vec![0xed, 0x01];
                bytes.extend_from_slice(&key.to_bytes());
                format!("z{}", bs58::encode(bytes).into_string())
            }
            _ => unreachable!("test key must be Ed25519"),
        };
        let verification_method = format!("{}#daemon-key-1", generated.did);
        did_document["verificationMethod"]
            .as_array_mut()
            .unwrap()
            .push(json!({
                "id": verification_method,
                "type": "Multikey",
                "controller": generated.did,
                "publicKeyMultibase": public_key,
            }));
        did_document["authentication"]
            .as_array_mut()
            .unwrap()
            .push(Value::String(verification_method.clone()));
        Some(json!({
            "schema": "awiki.daemon.user_subkey_package.v1",
            "user_did": generated.did,
            "verification_method": verification_method,
            "key_type": "Multikey/Ed25519",
            "public_key_multibase": public_key,
            "private_key_multibase": private_key.to_pem(),
        }))
    } else {
        None
    };
    fs::write(
        identity_dir.join("identity.json"),
        json!({
            "did": generated.did,
            "unique_id": dir_name,
            "created_at": "2026-05-21T00:00:00Z",
            "user_id": format!("user-{alias}"),
            "name": alias,
            "handle": alias,
            "full_handle": format!("{alias}.awiki.test"),
        })
        .to_string(),
    )
    .unwrap();
    fs::write(
        identity_dir.join("did_document.json"),
        serde_json::to_vec_pretty(&did_document).unwrap(),
    )
    .unwrap();
    fs::write(
        identity_dir.join("key-1-private.pem"),
        &generated.key1_private_pem,
    )
    .unwrap();
    fs::write(
        identity_dir.join("key-1-public.pem"),
        &generated.key1_public_pem,
    )
    .unwrap();
    fs::write(
        identity_dir.join("auth.json"),
        format!(r#"{{"jwt_token":"test-token-for-{alias}","expires_at":"2026-05-21T00:00:00Z"}}"#),
    )
    .unwrap();
    if include_daemon_package {
        let package = daemon_package.expect("daemon package");
        fs::write(
            identity_dir.join("daemon-subkey-package.json"),
            serde_json::to_vec_pretty(&package).unwrap(),
        )
        .unwrap();
    }
    fs::write(
        identities.join("registry.json"),
        json!({
            "default_identity": if make_default { alias } else { "alice" },
            "identities": [
                {
                    "id": "alice-id",
                    "did": "did:example:alice",
                    "handle": "alice.awiki.test",
                    "display_name": "Alice",
                    "local_alias": "alice",
                    "dir_name": "alice",
                    "ready_for_auth": true,
                    "ready_for_messaging": true,
                    "missing": []
                },
                {
                    "id": dir_name,
                    "did": generated.did,
                    "handle": format!("{alias}.awiki.test"),
                    "display_name": alias,
                    "local_alias": alias,
                    "dir_name": dir_name,
                    "ready_for_auth": true,
                    "ready_for_messaging": true,
                    "missing": []
                }
            ]
        })
        .to_string(),
    )
    .unwrap();
}

fn unique_temp_root() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let counter = TEMP_ROOT_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "im-core-phase1c-{}-{nanos}-{counter}",
        std::process::id()
    ))
}

struct TestServer {
    base_url: String,
    handle: thread::JoinHandle<Vec<CapturedHttp>>,
}

impl TestServer {
    fn spawn(responses: Vec<ExpectedHttp>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let handle = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut captured = Vec::new();
            for response in responses {
                let mut stream = accept_before_deadline(&listener, deadline);
                let request = read_http_request(&mut stream);
                let body = response
                    .responder
                    .as_ref()
                    .map_or_else(|| response.body.clone(), |responder| responder(&request));
                write_json_response(&mut stream, response.status_code, &body);
                captured.push(request);
            }
            captured
        });
        Self { base_url, handle }
    }

    fn base_url(&self) -> &str {
        &self.base_url
    }

    fn join(self) -> Vec<CapturedHttp> {
        self.handle.join().unwrap()
    }
}

struct ExpectedHttp {
    status_code: u16,
    body: Value,
    responder: Option<Box<dyn Fn(&CapturedHttp) -> Value + Send>>,
}

impl ExpectedHttp {
    fn json(body: Value) -> Self {
        Self {
            status_code: 200,
            body,
            responder: None,
        }
    }

    fn rpc_result(result: Value) -> Self {
        Self::json(json!({
            "jsonrpc": "2.0",
            "id": "req-1",
            "result": result,
        }))
    }

    fn status(status_code: u16, body: Value) -> Self {
        Self {
            status_code,
            body,
            responder: None,
        }
    }

    fn dynamic_rpc<F>(responder: F) -> Self
    where
        F: Fn(&CapturedHttp) -> Value + Send + 'static,
    {
        Self {
            status_code: 200,
            body: Value::Null,
            responder: Some(Box::new(responder)),
        }
    }
}

#[derive(Debug)]
struct CapturedHttp {
    method: String,
    path: String,
    headers: std::collections::BTreeMap<String, String>,
    body: Vec<u8>,
}

impl CapturedHttp {
    fn json_body(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap()
    }
}

fn accept_before_deadline(listener: &TcpListener, deadline: Instant) -> TcpStream {
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).unwrap();
                return stream;
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < deadline, "timed out waiting for request");
                thread::sleep(Duration::from_millis(10));
            }
            Err(err) => panic!("accept request: {err}"),
        }
    }
}

fn read_http_request(stream: &mut TcpStream) -> CapturedHttp {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut raw = Vec::new();
    let mut buffer = [0_u8; 4096];
    let header_end = loop {
        let count = stream.read(&mut buffer).unwrap();
        assert!(count > 0, "request closed before headers");
        raw.extend_from_slice(&buffer[..count]);
        if let Some(index) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
            break index;
        }
    };
    let headers_text = std::str::from_utf8(&raw[..header_end]).unwrap();
    let mut lines = headers_text.lines();
    let request_line = lines.next().unwrap();
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().unwrap().to_string();
    let path = request_parts.next().unwrap().to_string();
    let headers = lines
        .filter_map(|line| {
            let (key, value) = line.split_once(':')?;
            Some((key.trim().to_ascii_lowercase(), value.trim().to_string()))
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let content_length = headers
        .get("content-length")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let body_start = header_end + 4;
    while raw.len() < body_start + content_length {
        let count = stream.read(&mut buffer).unwrap();
        assert!(count > 0, "request closed before body");
        raw.extend_from_slice(&buffer[..count]);
    }
    CapturedHttp {
        method,
        path,
        headers,
        body: raw[body_start..body_start + content_length].to_vec(),
    }
}

fn write_json_response(stream: &mut TcpStream, status_code: u16, body: &Value) {
    let body = body.to_string();
    write!(
        stream,
        "HTTP/1.1 {status_code} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
    .unwrap();
}
