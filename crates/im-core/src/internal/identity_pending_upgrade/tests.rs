use super::*;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};

use crate::internal::identity_device_join_runtime::{
    DeviceJoinRemoteAuthorization, DeviceJoinRemoteDeviceSummary,
};
use crate::internal::identity_device_state::{
    DeviceAuthorizationRole, DeviceAuthorizationStatus, IdentityInternalCheckpoint,
};
use crate::vault::{
    DeviceVaultRootKey, FileSecretVault, FileSecretVaultStore, SealSecretRequest,
    SecretAccessPolicy, SecretBytes, SecretMetadata, SecretVault,
};

#[tokio::test]
async fn pre_attempt_registration_is_discarded_but_unknown_outcome_is_blocked() {
    let fixture = Fixture::new();
    let safe = fixture.seal(
        SecretKind::IdentityRegistrationPending,
        "registration-safe",
        serde_json::json!({
            "schema_version": 1,
            "remote_attempted": false,
            "remote_result": null,
            "phase": "prepared"
        }),
        None,
    );
    let unknown = fixture.seal(
        SecretKind::IdentityRegistrationPending,
        "registration-unknown",
        serde_json::json!({
            "schema_version": 1,
            "remote_attempted": true,
            "remote_result": null,
            "phase": "prepared"
        }),
        None,
    );

    let dry_run = converge(&fixture.core, true).await.unwrap();
    assert!(fixture.vault.open(&safe).is_ok());
    assert!(dry_run
        .warnings
        .iter()
        .any(|warning| warning.contains("safe to discard")));

    let applied = converge(&fixture.core, false).await.unwrap();
    assert!(fixture.vault.open(&safe).is_err());
    assert!(fixture.vault.open(&unknown).is_ok());
    assert!(applied
        .blockers
        .iter()
        .any(|blocker| blocker.contains("unreadable")));
}

#[tokio::test]
async fn attempted_registration_imports_exact_identity_for_reconciliation() {
    let fixture = Fixture::new();
    let generated =
        crate::internal::identity_generation::generate_vnext_handle_identity_with_default_daemon_subkey(
            "awiki.test",
            "registration-upgrade",
            None,
            None,
        )
        .unwrap();
    let handle = "registration-upgrade";
    let domain = "awiki.test";
    let key_id = format!(
        "registration-{}",
        URL_SAFE_NO_PAD.encode(Sha256::digest(format!("{handle}@{domain}").as_bytes()))
    );
    fixture.seal(
        SecretKind::IdentityRegistrationPending,
        &key_id,
        serde_json::to_value(LegacyRegistrationPending {
            schema_version: 1,
            target_handle: handle.to_owned(),
            target_domain: domain.to_owned(),
            local_alias: handle.to_owned(),
            display_name: "Registration Upgrade".to_owned(),
            make_default: true,
            verification_kind: "already_verified".to_owned(),
            verification_target: None,
            invite_code: None,
            document_hash: crate::internal::identity_wire::document::document_hash(
                &generated.did_document,
            )
            .unwrap(),
            generated,
            phase: PendingRegistrationPhase::Prepared,
            remote_attempted: true,
            remote_result: None,
        })
        .unwrap(),
        None,
    );

    let outcome = converge(&fixture.core, false).await.unwrap();

    assert!(outcome
        .warnings
        .iter()
        .any(|warning| warning.contains("upgraded attempted legacy registration")));
    let current = PendingRegistrationStore::from_core(&fixture.core)
        .unwrap()
        .load(handle, domain)
        .unwrap()
        .unwrap()
        .1;
    assert!(current.remote_attempted);
    assert!(current.identity.legacy_daemon_authorization);
    assert!(!serde_json::to_string(&current)
        .unwrap()
        .contains("PRIVATE KEY"));
    let store = crate::internal::identity_custody::open_controller_store(&fixture.core).unwrap();
    assert_eq!(
        store
            .open_identity(current.identity.did.as_str())
            .unwrap()
            .root_capability(),
        anp_identity::RootCapabilityState::Active
    );
}

#[tokio::test]
async fn pre_commit_recovery_is_discarded_while_root_import_is_retained_for_cutover() {
    let fixture = Fixture::new();
    let recovery = fixture.seal(
        SecretKind::IdentityHandleRecoveryPending,
        "recovery-safe",
        serde_json::json!({
            "schema_version": 1,
            "commit_attempted": false,
            "remote_result": null
        }),
        None,
    );
    let root = fixture.seal(
        SecretKind::IdentityRootImportPending,
        "root-import-pending",
        serde_json::json!({"schema_version": 1}),
        None,
    );

    let outcome = converge(&fixture.core, false).await.unwrap();

    assert!(fixture.vault.open(&recovery).is_err());
    assert!(fixture.vault.open(&root).is_ok());
    assert!(outcome
        .warnings
        .iter()
        .any(|warning| warning.contains("root-import pending")));
}

#[tokio::test]
async fn legacy_join_keys_are_imported_and_pending_record_becomes_secret_free() {
    let fixture = Fixture::new();
    let generated =
        crate::internal::identity_generation::generate_vnext_handle_identity_with_default_daemon_subkey(
            "awiki.test",
            "join-upgrade",
            None,
            None,
        )
        .unwrap();
    let join_session_id = "join-session-legacy";
    let key_id = format!(
        "join-activation-{}",
        URL_SAFE_NO_PAD.encode(Sha256::digest(join_session_id.as_bytes()))
    );
    let authorization = DeviceJoinRemoteAuthorization {
        checkpoint: IdentityInternalCheckpoint {
            document_version: 2,
            registry_version: 3,
            document_hash: crate::internal::identity_wire::document::document_hash(
                &generated.did_document,
            )
            .unwrap(),
        },
        device: DeviceJoinRemoteDeviceSummary {
            device_id: generated.protocol_device_id.as_str().to_owned(),
            signing_key_id: generated.device_signing_key_id.clone(),
            e2ee_key_id: generated.device_e2ee_key_id.clone(),
            status: DeviceAuthorizationStatus::Active,
            role: DeviceAuthorizationRole::Member,
            management_ready: false,
            auth_generation: 1,
        },
    };
    fixture.seal(
        SecretKind::IdentityJoinActivationPending,
        &key_id,
        serde_json::json!({
            "schema_version": 1,
            "join_session_id": join_session_id,
            "did": generated.did,
            "resolved_document": generated.did_document,
            "authorization": authorization,
            "signing_private_pem": generated.device_signing_private_pem,
            "e2ee_private_pem": generated.device_e2ee_private_pem,
            "access_result": null
        }),
        Some(generated.did.as_str()),
    );

    let outcome = converge(&fixture.core, false).await.unwrap();

    assert!(outcome
        .warnings
        .iter()
        .any(|warning| warning.contains("upgraded legacy Join")));
    let current = PendingJoinActivationStore::from_core(&fixture.core)
        .unwrap()
        .load(join_session_id, &generated.did)
        .unwrap()
        .unwrap()
        .1;
    assert_eq!(
        current.custody.enrollment_id,
        crate::internal::identity_custody::LEGACY_IMPORTED_ACTIVE_ENROLLMENT_ID
    );
    let encoded = serde_json::to_string(&current).unwrap();
    assert!(!encoded.contains("PRIVATE KEY"));
    crate::internal::identity_custody::sign_join_enrollment(
        &fixture.core,
        &generated.did,
        &current.custody,
        &generated.device_signing_key_id,
        b"legacy join retry",
    )
    .unwrap();
}

#[tokio::test]
async fn legacy_upgrade_pending_reuses_exact_imported_device_keys() {
    let fixture = Fixture::new();
    let legacy =
        crate::internal::identity_generation::generate_handle_identity_with_default_daemon_subkey(
            "awiki.test",
            "legacy-upgrade",
            None,
            None,
        )
        .unwrap();
    let generated = crate::internal::identity_legacy_upgrade::build_legacy_upgrade(
        &legacy.identity.did_document,
        &legacy.identity.key1_private_pem,
    )
    .unwrap();
    let root_ref = fixture
        .vault
        .seal(SealSecretRequest {
            metadata: SecretMetadata {
                workspace_id: "pending-upgrade-workspace".to_owned(),
                device_id: "pending-upgrade-device".to_owned(),
                identity_id: Some(legacy.identity.unique_id.clone()),
                did: Some(legacy.identity.did.as_str().to_owned()),
                kind: SecretKind::IdentityRootPrivate,
                key_id: "key-1".to_owned(),
                key_version: 1,
                policy: SecretAccessPolicy::no_prompt_local_secret(),
            },
            plaintext: SecretBytes::from_vec(legacy.identity.key1_private_pem.as_bytes().to_vec()),
        })
        .unwrap();
    let alias = "legacy-upgrade";
    let key_id = format!(
        "legacy-upgrade-{}",
        URL_SAFE_NO_PAD.encode(Sha256::digest(alias.as_bytes()))
    );
    fixture.seal(
        SecretKind::IdentityLegacyUpgradePending,
        &key_id,
        serde_json::to_value(LegacyUpgradePending {
            schema_version: 1,
            local_alias: alias.to_owned(),
            source_document_hash: crate::internal::identity_wire::document::document_hash(
                &legacy.identity.did_document,
            )
            .unwrap(),
            root_ref: root_ref.clone(),
            generated,
            phase: PendingLegacyUpgradePhase::Prepared,
            attempt: PendingLegacyUpgradeAttempt::Running,
            last_attempt_at: "2026-08-21T00:00:00Z".to_owned(),
            failure_code: None,
            checkpoint: None,
            access_token: None,
        })
        .unwrap(),
        Some(legacy.identity.did.as_str()),
    );

    let outcome = converge(&fixture.core, false).await.unwrap();

    assert!(outcome
        .warnings
        .iter()
        .any(|warning| warning.contains("upgraded Legacy Upgrade")));
    let current = PendingLegacyUpgradeStore::from_core(&fixture.core)
        .unwrap()
        .load(alias)
        .unwrap()
        .unwrap()
        .1;
    assert_eq!(current.root_ref, root_ref);
    assert_eq!(
        current.identity.custody.enrollment_id,
        crate::internal::identity_custody::LEGACY_IMPORTED_ACTIVE_ENROLLMENT_ID
    );
    crate::internal::identity_custody::pending_join_identity(
        &fixture.core,
        &current.identity.did,
        &current.identity.custody,
    )
    .unwrap();
    assert!(!serde_json::to_string(&current)
        .unwrap()
        .contains("PRIVATE KEY"));
}

struct Fixture {
    _root: tempfile::TempDir,
    core: crate::ImCore,
    vault: FileSecretVault,
}

impl Fixture {
    const ROOT_KEY: [u8; 32] = [101_u8; 32];

    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let vault_dir = root.path().join("vault");
        let vault = FileSecretVault::new(
            DeviceVaultRootKey::from_bytes(Self::ROOT_KEY),
            FileSecretVaultStore::new(&vault_dir),
        );
        let core = crate::ImCore::new_with_options(
            crate::ImCoreConfig {
                service_base_url: crate::ServiceEndpoint::parse("https://example.test").unwrap(),
                did_domain: "awiki.test".to_owned(),
                client_version_info: None,
                user_service_endpoint: None,
                message_service_endpoint: None,
                mail_service_endpoint: None,
                anp_service_endpoint: None,
                anp_service_did: None,
                ca_bundle: None,
                transport_policy: crate::MessageTransportPolicy::HttpOnly,
            },
            crate::ImCorePaths {
                identities: crate::paths::IdentityRegistryPaths {
                    identity_root_dir: root.path().join("identities"),
                    registry_path: root.path().join("identities/index.json"),
                    default_identity_path: Some(root.path().join("identities/default")),
                },
                local_state: crate::paths::LocalStatePaths {
                    sqlite_path: root.path().join("local/im.sqlite"),
                },
                runtime: crate::paths::RuntimePaths {
                    cache_dir: root.path().join("cache"),
                    temp_dir: root.path().join("tmp"),
                },
            },
            crate::ImCoreOpenOptions::default().with_identity_secret_vault(
                crate::IdentitySecretStoragePolicy::VaultRequired,
                crate::ImCoreSecretVaultOptions::new(
                    DeviceVaultRootKey::from_bytes(Self::ROOT_KEY),
                    vault_dir,
                    "pending-upgrade-workspace",
                    "pending-upgrade-device",
                ),
            ),
        )
        .unwrap();
        Self {
            _root: root,
            core,
            vault,
        }
    }

    fn seal(
        &self,
        kind: SecretKind,
        key_id: &str,
        value: serde_json::Value,
        did: Option<&str>,
    ) -> SecretRef {
        self.vault
            .seal(SealSecretRequest {
                metadata: SecretMetadata {
                    workspace_id: "pending-upgrade-workspace".to_owned(),
                    device_id: "pending-upgrade-device".to_owned(),
                    identity_id: None,
                    did: did.map(ToOwned::to_owned),
                    kind,
                    key_id: key_id.to_owned(),
                    key_version: 1,
                    policy: SecretAccessPolicy::no_prompt_local_secret(),
                },
                plaintext: SecretBytes::from_vec(serde_json::to_vec(&value).unwrap()),
            })
            .unwrap()
    }
}
