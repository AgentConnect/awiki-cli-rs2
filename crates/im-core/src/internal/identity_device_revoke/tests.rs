use super::*;

use std::collections::VecDeque;
use std::path::Path;

use serde_json::{json, Value};
use x25519_dalek::StaticSecret as X25519StaticSecret;

use crate::internal::identity_device_join_runtime::{
    DeviceJoinRemoteDeviceSummary, DeviceJoinRemoteRegistry,
};
use crate::internal::identity_device_state::{
    DeviceAuthorizationProjection, DeviceAuthorizationRole, DeviceAuthorizationStatus,
    IdentityDeviceMode, IdentityDeviceState, IdentityInternalCheckpoint,
    IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
};
use crate::internal::identity_store::{
    IdentityStore, SaveIdentityInput, SaveIdentityKeyMode, SaveIdentitySecretStorage,
};
use crate::internal::identity_wire::device_revoke::{
    DeviceRevokeRemoteResult, PreparedDeviceRevoke,
};

const LOCAL_ALIAS: &str = "alice";
const TARGET_DEVICE_ID: &str = "dev-target";
const WORKSPACE_ID: &str = "device-revoke-test-workspace";
const VAULT_CONTEXT_DEVICE_ID: &str = "device-revoke-test-context";
const VAULT_KEY: [u8; 32] = [63_u8; 32];

struct Scenario {
    _root: tempfile::TempDir,
    paths: crate::ImCorePaths,
    did: crate::ids::Did,
    document: Value,
    registry: DeviceJoinRemoteRegistry,
    authorizing: DeviceJoinRemoteDeviceSummary,
    target: DeviceJoinRemoteDeviceSummary,
    now: OffsetDateTime,
}

impl Scenario {
    fn open_core(&self, enabled: bool) -> crate::ImCore {
        open_core(self._root.path(), enabled)
    }

    fn local_document(&self) -> Value {
        let store = IdentityStore::new(&self.paths.identities);
        let dir_name = store.load_index().unwrap().credentials[LOCAL_ALIAS]
            .dir_name
            .clone();
        store.load_did_document(&dir_name).unwrap()
    }
}

#[derive(Debug)]
enum RevokeAction {
    Success,
    InvalidSuccess,
    Error(crate::ImError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CapturedCall {
    operation_id: String,
    proof_nonce: String,
    proof_signature: String,
}

struct MockRemote {
    registry: DeviceJoinRemoteRegistry,
    registry_calls: usize,
    revoke_calls: Vec<CapturedCall>,
    actions: VecDeque<RevokeAction>,
}

impl MockRemote {
    fn new(
        registry: DeviceJoinRemoteRegistry,
        actions: impl IntoIterator<Item = RevokeAction>,
    ) -> Self {
        Self {
            registry,
            registry_calls: 0,
            revoke_calls: Vec::new(),
            actions: actions.into_iter().collect(),
        }
    }
}

impl DeviceRevokeRemote for MockRemote {
    async fn registry(
        &mut self,
        _did: &crate::ids::Did,
    ) -> crate::ImResult<DeviceJoinRemoteRegistry> {
        self.registry_calls += 1;
        Ok(self.registry.clone())
    }

    async fn revoke(
        &mut self,
        prepared: &PreparedDeviceRevoke,
        expected_auth_generation: u64,
        expected_checkpoint: &IdentityInternalCheckpoint,
    ) -> crate::ImResult<DeviceRevokeRemoteResult> {
        self.revoke_calls.push(CapturedCall {
            operation_id: prepared.operation_id.clone(),
            proof_nonce: prepared.authorizing_device_proof.nonce.clone(),
            proof_signature: prepared.authorizing_device_proof.signature.clone(),
        });
        match self.actions.pop_front().expect("queued revoke action") {
            RevokeAction::Success => Ok(DeviceRevokeRemoteResult {
                target_device_id: prepared.target_device_id.clone(),
                auth_generation: expected_auth_generation,
                checkpoint: expected_checkpoint.clone(),
            }),
            RevokeAction::InvalidSuccess => Ok(DeviceRevokeRemoteResult {
                target_device_id: prepared.target_device_id.clone(),
                auth_generation: expected_auth_generation,
                checkpoint: IdentityInternalCheckpoint {
                    document_version: expected_checkpoint.document_version + 1,
                    ..expected_checkpoint.clone()
                },
            }),
            RevokeAction::Error(error) => Err(error),
        }
    }
}

struct MockResolver {
    document: Value,
    calls: usize,
}

impl DeviceRevokeDocumentResolver for MockResolver {
    async fn resolve(&mut self, _did: &crate::ids::Did) -> crate::ImResult<Value> {
        self.calls += 1;
        Ok(self.document.clone())
    }
}

#[tokio::test]
async fn member_target_is_revoked_only_after_validated_server_success() {
    let scenario = scenario(
        DeviceAuthorizationRole::Admin,
        DeviceAuthorizationRole::Member,
        false,
    );
    let core = scenario.open_core(true);
    let client = core
        .client(crate::identity::IdentitySelector::Default)
        .unwrap();
    let store = PendingDeviceRevokeStore::from_core(&core).unwrap();
    let mut remote = MockRemote::new(scenario.registry.clone(), [RevokeAction::Success]);
    let mut resolver = MockResolver {
        document: scenario.document.clone(),
        calls: 0,
    };

    let result = execute_with_runtime(
        &core,
        &client,
        &store,
        &scenario.authorizing.device_id,
        &scenario.authorizing.signing_key_id,
        &scenario.target.device_id,
        scenario.now,
        scenario.now,
        &mut remote,
        &mut resolver,
    )
    .await
    .unwrap();

    assert_eq!(result.did, scenario.did);
    assert_eq!(result.target_device_id.as_str(), TARGET_DEVICE_ID);
    assert_eq!(result.status, crate::identity::DeviceRevokeStatus::Revoked);
    assert_eq!(remote.registry_calls, 1);
    assert_eq!(remote.revoke_calls.len(), 1);
    assert_eq!(resolver.calls, 1);
    assert!(store
        .load(&scenario.did, TARGET_DEVICE_ID)
        .unwrap()
        .is_none());
    assert!(!manifest_contains(
        &scenario.local_document(),
        TARGET_DEVICE_ID
    ));
    let state = IdentityStore::new(&scenario.paths.identities)
        .load_index()
        .unwrap()
        .credentials[LOCAL_ALIAS]
        .device_state
        .clone()
        .unwrap();
    assert_eq!(
        state.checkpoint.unwrap().document_version,
        scenario.registry.checkpoint.document_version + 1
    );
}

#[tokio::test]
async fn another_ready_admin_can_be_revoked_when_one_ready_admin_remains() {
    let scenario = scenario(
        DeviceAuthorizationRole::Admin,
        DeviceAuthorizationRole::Admin,
        true,
    );
    let core = scenario.open_core(true);
    let client = core
        .client(crate::identity::IdentitySelector::Default)
        .unwrap();
    let store = PendingDeviceRevokeStore::from_core(&core).unwrap();
    let mut remote = MockRemote::new(scenario.registry.clone(), [RevokeAction::Success]);
    let mut resolver = MockResolver {
        document: scenario.document.clone(),
        calls: 0,
    };

    execute_with_runtime(
        &core,
        &client,
        &store,
        &scenario.authorizing.device_id,
        &scenario.authorizing.signing_key_id,
        &scenario.target.device_id,
        scenario.now,
        scenario.now,
        &mut remote,
        &mut resolver,
    )
    .await
    .unwrap();

    assert!(!manifest_contains(
        &scenario.local_document(),
        TARGET_DEVICE_ID
    ));
    assert!(manifest_contains(
        &scenario.local_document(),
        &scenario.authorizing.device_id
    ));
}

#[tokio::test]
async fn member_caller_self_revoke_and_last_ready_admin_are_rejected() {
    let member = scenario(
        DeviceAuthorizationRole::Member,
        DeviceAuthorizationRole::Member,
        false,
    );
    let member_core = member.open_core(true);
    assert!(matches!(
        crate::internal::identity_device_join::ready_admin_context(
            &member_core,
            &crate::identity::IdentitySelector::Default,
            None,
        ),
        Err(crate::ImError::PermissionDenied)
    ));

    let scenario = scenario(
        DeviceAuthorizationRole::Admin,
        DeviceAuthorizationRole::Member,
        false,
    );
    let core = scenario.open_core(true);
    let client = core
        .client(crate::identity::IdentitySelector::Default)
        .unwrap();
    let store = PendingDeviceRevokeStore::from_core(&core).unwrap();
    let mut remote = MockRemote::new(scenario.registry.clone(), []);
    let mut resolver = MockResolver {
        document: scenario.document.clone(),
        calls: 0,
    };
    assert_eq!(
        execute_with_runtime(
            &core,
            &client,
            &store,
            &scenario.authorizing.device_id,
            &scenario.authorizing.signing_key_id,
            &scenario.authorizing.device_id,
            scenario.now,
            scenario.now,
            &mut remote,
            &mut resolver,
        )
        .await
        .unwrap_err(),
        crate::ImError::PermissionDenied
    );
    assert_eq!(remote.registry_calls, 0);
    assert_eq!(resolver.calls, 0);

    // Exercise the independent last-ready-admin invariant directly. The
    // current product also rejects this shape earlier as self-revocation.
    let mut last_ready_registry = scenario.registry.clone();
    last_ready_registry.devices = vec![scenario.authorizing.clone()];
    assert_eq!(
        prepare_initial_intent(
            &client,
            &scenario.authorizing.device_id,
            &scenario.authorizing.device_id,
            &scenario.authorizing.signing_key_id,
            last_ready_registry,
            scenario.document.clone(),
        )
        .unwrap_err(),
        crate::ImError::PermissionDenied
    );
}

#[tokio::test]
async fn expired_user_presence_fails_before_state_or_network_access() {
    let scenario = scenario(
        DeviceAuthorizationRole::Admin,
        DeviceAuthorizationRole::Member,
        false,
    );
    let core = scenario.open_core(true);
    let client = core
        .client(crate::identity::IdentitySelector::Default)
        .unwrap();
    let store = PendingDeviceRevokeStore::from_core(&core).unwrap();
    let mut remote = MockRemote::new(scenario.registry.clone(), []);
    let mut resolver = MockResolver {
        document: scenario.document.clone(),
        calls: 0,
    };

    assert_eq!(
        execute_with_runtime(
            &core,
            &client,
            &store,
            &scenario.authorizing.device_id,
            &scenario.authorizing.signing_key_id,
            &scenario.target.device_id,
            scenario.now - Duration::seconds(USER_PRESENCE_MAX_AGE_SECONDS + 1),
            scenario.now,
            &mut remote,
            &mut resolver,
        )
        .await
        .unwrap_err(),
        crate::ImError::SessionExpired
    );
    assert_eq!(remote.registry_calls, 0);
    assert_eq!(resolver.calls, 0);
    assert!(store
        .load(&scenario.did, TARGET_DEVICE_ID)
        .unwrap()
        .is_none());
    assert!(manifest_contains(
        &scenario.local_document(),
        TARGET_DEVICE_ID
    ));
}

#[tokio::test]
async fn response_loss_and_restart_reuse_operation_with_fresh_admin_proof() {
    let scenario = scenario(
        DeviceAuthorizationRole::Admin,
        DeviceAuthorizationRole::Member,
        false,
    );
    let mut remote = MockRemote::new(
        scenario.registry.clone(),
        [
            RevokeAction::Error(crate::ImError::TransportUnavailable {
                detail: "response contained secret-shaped diagnostics".to_owned(),
            }),
            RevokeAction::Success,
        ],
    );
    let mut resolver = MockResolver {
        document: scenario.document.clone(),
        calls: 0,
    };

    let first_core = scenario.open_core(true);
    let first_client = first_core
        .client(crate::identity::IdentitySelector::Default)
        .unwrap();
    let first_store = PendingDeviceRevokeStore::from_core(&first_core).unwrap();
    let error = execute_with_runtime(
        &first_core,
        &first_client,
        &first_store,
        &scenario.authorizing.device_id,
        &scenario.authorizing.signing_key_id,
        &scenario.target.device_id,
        scenario.now,
        scenario.now,
        &mut remote,
        &mut resolver,
    )
    .await
    .unwrap_err();
    assert_eq!(
        error,
        crate::ImError::TransportUnavailable {
            detail: "device revoke transport failed".to_owned()
        }
    );
    assert!(first_store
        .load(&scenario.did, TARGET_DEVICE_ID)
        .unwrap()
        .is_some());
    assert!(manifest_contains(
        &scenario.local_document(),
        TARGET_DEVICE_ID
    ));
    drop(first_client);
    drop(first_core);

    let restarted_core = scenario.open_core(true);
    let restarted_client = restarted_core
        .client(crate::identity::IdentitySelector::Default)
        .unwrap();
    let restarted_store = PendingDeviceRevokeStore::from_core(&restarted_core).unwrap();
    execute_with_runtime(
        &restarted_core,
        &restarted_client,
        &restarted_store,
        &scenario.authorizing.device_id,
        &scenario.authorizing.signing_key_id,
        &scenario.target.device_id,
        scenario.now + Duration::seconds(1),
        scenario.now + Duration::seconds(1),
        &mut remote,
        &mut resolver,
    )
    .await
    .unwrap();

    assert_eq!(remote.registry_calls, 1, "retry must use sealed intent");
    assert_eq!(
        resolver.calls, 1,
        "retry must use sealed root-signed document"
    );
    assert_eq!(remote.revoke_calls.len(), 2);
    assert_eq!(
        remote.revoke_calls[0].operation_id,
        remote.revoke_calls[1].operation_id
    );
    assert_ne!(
        remote.revoke_calls[0].proof_nonce,
        remote.revoke_calls[1].proof_nonce
    );
    assert_ne!(
        remote.revoke_calls[0].proof_signature,
        remote.revoke_calls[1].proof_signature
    );
    assert!(restarted_store
        .load(&scenario.did, TARGET_DEVICE_ID)
        .unwrap()
        .is_none());
    assert!(!manifest_contains(
        &scenario.local_document(),
        TARGET_DEVICE_ID
    ));
}

#[tokio::test]
async fn version_conflict_discards_stale_intent_and_redacts_server_data() {
    let scenario = scenario(
        DeviceAuthorizationRole::Admin,
        DeviceAuthorizationRole::Member,
        false,
    );
    let core = scenario.open_core(true);
    let client = core
        .client(crate::identity::IdentitySelector::Default)
        .unwrap();
    let store = PendingDeviceRevokeStore::from_core(&core).unwrap();
    let mut remote = MockRemote::new(
        scenario.registry.clone(),
        [RevokeAction::Error(crate::ImError::Service {
            status_code: Some(409),
            code: Some("device.document_version_conflict".to_owned()),
            message: "leaked-root-proof-shape".to_owned(),
            data: Some(json!({"proof": "leaked-device-proof-shape"})),
        })],
    );
    let mut resolver = MockResolver {
        document: scenario.document.clone(),
        calls: 0,
    };

    let error = execute_with_runtime(
        &core,
        &client,
        &store,
        &scenario.authorizing.device_id,
        &scenario.authorizing.signing_key_id,
        &scenario.target.device_id,
        scenario.now,
        scenario.now,
        &mut remote,
        &mut resolver,
    )
    .await
    .unwrap_err();

    assert_eq!(
        error,
        crate::ImError::Service {
            status_code: Some(409),
            code: Some("device.document_version_conflict".to_owned()),
            message: "device revoke request failed".to_owned(),
            data: None,
        }
    );
    let rendered = format!("{error:?} {error}");
    assert!(!rendered.contains("leaked-root-proof-shape"));
    assert!(!rendered.contains("leaked-device-proof-shape"));
    assert!(store
        .load(&scenario.did, TARGET_DEVICE_ID)
        .unwrap()
        .is_none());
    assert!(manifest_contains(
        &scenario.local_document(),
        TARGET_DEVICE_ID
    ));
}

#[tokio::test]
async fn invalid_success_never_advances_local_state_and_keeps_exact_retry_intent() {
    let scenario = scenario(
        DeviceAuthorizationRole::Admin,
        DeviceAuthorizationRole::Member,
        false,
    );
    let core = scenario.open_core(true);
    let client = core
        .client(crate::identity::IdentitySelector::Default)
        .unwrap();
    let store = PendingDeviceRevokeStore::from_core(&core).unwrap();
    let mut remote = MockRemote::new(scenario.registry.clone(), [RevokeAction::InvalidSuccess]);
    let mut resolver = MockResolver {
        document: scenario.document.clone(),
        calls: 0,
    };

    assert_eq!(
        execute_with_runtime(
            &core,
            &client,
            &store,
            &scenario.authorizing.device_id,
            &scenario.authorizing.signing_key_id,
            &scenario.target.device_id,
            scenario.now,
            scenario.now,
            &mut remote,
            &mut resolver,
        )
        .await
        .unwrap_err(),
        crate::ImError::PermissionDenied
    );
    assert!(store
        .load(&scenario.did, TARGET_DEVICE_ID)
        .unwrap()
        .is_some());
    assert!(manifest_contains(
        &scenario.local_document(),
        TARGET_DEVICE_ID
    ));
}

fn scenario(
    local_role: DeviceAuthorizationRole,
    target_role: DeviceAuthorizationRole,
    target_management_ready: bool,
) -> Scenario {
    let root = tempfile::tempdir().unwrap();
    let paths = test_paths(root.path());
    let generated = crate::internal::identity_generation::generate_vnext_handle_identity_with_default_daemon_subkey(
        "awiki.test",
        LOCAL_ALIAS,
        None,
        None,
    )
    .unwrap();
    let target_device_id = crate::ids::ProtocolDeviceId::parse(TARGET_DEVICE_ID).unwrap();
    let target_signing_private = anp::PrivateKeyMaterial::Ed25519(
        ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng),
    );
    let target_e2ee_private =
        anp::PrivateKeyMaterial::X25519(X25519StaticSecret::random_from_rng(rand::rngs::OsRng));
    let target_signing_key_id = format!(
        "{}#{}-sign",
        generated.did.as_str(),
        target_device_id.as_str()
    );
    let target_e2ee_key_id = format!(
        "{}#{}-e2ee",
        generated.did.as_str(),
        target_device_id.as_str()
    );
    let target_entry = anp::authentication::DeviceManifestEntry {
        device_id: target_device_id.as_str().to_owned(),
        signing_key_id: target_signing_key_id.clone(),
        e2ee_key_id: target_e2ee_key_id.clone(),
        profiles: vnext_profiles(),
    };
    let target_signing_method = json!({
        "id": target_signing_key_id,
        "type": "Multikey",
        "controller": generated.did.as_str(),
        "publicKeyMultibase": public_key_multibase(&target_signing_private.public_key()),
    });
    let target_e2ee_method = json!({
        "id": target_e2ee_key_id,
        "type": "X25519KeyAgreementKey2019",
        "controller": generated.did.as_str(),
        "publicKeyMultibase": public_key_multibase(&target_e2ee_private.public_key()),
    });
    let mut document = anp::authentication::add_device_to_did_document(
        &generated.did_document,
        &generated.root_key_id,
        &target_entry,
        &target_signing_method,
        &target_e2ee_method,
        &[],
    )
    .unwrap();
    crate::internal::identity_daemon_subkey::resign_did_document_with_key1(
        &mut document,
        &generated.did,
        &generated.root_private_pem,
    )
    .unwrap();
    let checkpoint = IdentityInternalCheckpoint {
        document_version: 9,
        document_hash: crate::internal::identity_wire::document::document_hash(&document).unwrap(),
        registry_version: 5,
    };
    let authorizing = DeviceJoinRemoteDeviceSummary {
        device_id: generated.protocol_device_id.as_str().to_owned(),
        signing_key_id: generated.device_signing_key_id.clone(),
        e2ee_key_id: generated.device_e2ee_key_id.clone(),
        status: DeviceAuthorizationStatus::Active,
        role: local_role,
        management_ready: local_role == DeviceAuthorizationRole::Admin,
        auth_generation: 3,
    };
    let target = DeviceJoinRemoteDeviceSummary {
        device_id: target_device_id.as_str().to_owned(),
        signing_key_id: target_signing_key_id,
        e2ee_key_id: target_e2ee_key_id,
        status: DeviceAuthorizationStatus::Active,
        role: target_role,
        management_ready: target_management_ready,
        auth_generation: 4,
    };
    let registry = DeviceJoinRemoteRegistry {
        did: generated.did.clone(),
        checkpoint: checkpoint.clone(),
        devices: vec![authorizing.clone(), target.clone()],
    };
    let vault = std::sync::Arc::new(crate::vault::FileSecretVault::new(
        crate::vault::DeviceVaultRootKey::from_bytes(VAULT_KEY),
        crate::vault::FileSecretVaultStore::new(root.path().join("vault")),
    ));
    IdentityStore::new(&paths.identities)
        .save_identity_with_secret_storage(
            SaveIdentityInput {
                local_alias: LOCAL_ALIAS.to_owned(),
                did: generated.did.clone(),
                unique_id: generated.unique_id.clone(),
                user_id: "user-1".to_owned(),
                display_name: "Alice".to_owned(),
                handle: LOCAL_ALIAS.to_owned(),
                full_handle: "alice.awiki.test".to_owned(),
                jwt_token: "device-token".to_owned(),
                did_document: Some(document.clone()),
                key_mode: SaveIdentityKeyMode::VNext {
                    root_key_id: generated.root_key_id.clone(),
                    device_signing_key_id: generated.device_signing_key_id.clone(),
                    device_e2ee_key_id: generated.device_e2ee_key_id.clone(),
                },
                device_state: Some(IdentityDeviceState {
                    schema_version: IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
                    mode: IdentityDeviceMode::VNext,
                    authorization: Some(DeviceAuthorizationProjection {
                        protocol_device_id: generated.protocol_device_id.clone(),
                        signing_key_id: generated.device_signing_key_id.clone(),
                        e2ee_key_id: generated.device_e2ee_key_id.clone(),
                        status: DeviceAuthorizationStatus::Active,
                        role: local_role,
                        management_ready: local_role == DeviceAuthorizationRole::Admin,
                        auth_generation: authorizing.auth_generation,
                    }),
                    checkpoint: Some(checkpoint),
                }),
                key1_private_pem: if local_role == DeviceAuthorizationRole::Admin {
                    generated.root_private_pem.clone()
                } else {
                    String::new()
                },
                key1_public_pem: generated.root_public_pem.clone(),
                e2ee_signing_private_pem: generated.device_signing_private_pem.clone(),
                e2ee_agreement_private_pem: generated.device_e2ee_private_pem.clone(),
                daemon_subkey_package: None,
                make_default: true,
            },
            SaveIdentitySecretStorage::Vault {
                workspace_id: WORKSPACE_ID.to_owned(),
                device_id: VAULT_CONTEXT_DEVICE_ID.to_owned(),
                vault,
            },
        )
        .unwrap();
    Scenario {
        _root: root,
        paths,
        did: generated.did,
        document,
        registry,
        authorizing,
        target,
        now: OffsetDateTime::from_unix_timestamp(1_784_515_200).unwrap(),
    }
}

fn open_core(root: &Path, enabled: bool) -> crate::ImCore {
    crate::ImCore::new_with_options(
        test_config(),
        test_paths(root),
        crate::ImCoreOpenOptions::default()
            .with_identity_secret_vault(
                crate::IdentitySecretStoragePolicy::VaultRequired,
                crate::ImCoreSecretVaultOptions::new(
                    crate::vault::DeviceVaultRootKey::from_bytes(VAULT_KEY),
                    root.join("vault"),
                    WORKSPACE_ID,
                    VAULT_CONTEXT_DEVICE_ID,
                ),
            )
            .with_multi_device_device_revoke_enabled(enabled),
    )
    .unwrap()
}

fn test_config() -> crate::ImCoreConfig {
    crate::ImCoreConfig {
        service_base_url: crate::ServiceEndpoint::parse("https://example.test").unwrap(),
        did_domain: "awiki.test".to_owned(),
        user_service_endpoint: None,
        message_service_endpoint: None,
        mail_service_endpoint: None,
        anp_service_endpoint: None,
        anp_service_did: None,
        ca_bundle: None,
        transport_policy: crate::MessageTransportPolicy::HttpOnly,
    }
}

fn test_paths(root: &Path) -> crate::ImCorePaths {
    crate::ImCorePaths {
        identities: crate::IdentityRegistryPaths {
            identity_root_dir: root.join("identities"),
            registry_path: root.join("identities").join("registry.json"),
            default_identity_path: Some(root.join("identities").join("default")),
        },
        local_state: crate::LocalStatePaths {
            sqlite_path: root.join("local").join("im.sqlite"),
        },
        runtime: crate::RuntimePaths {
            cache_dir: root.join("cache"),
            temp_dir: root.join("tmp"),
        },
    }
}

fn vnext_profiles() -> Vec<String> {
    [
        anp::authentication::PROFILE_CORE_BINDING_V2,
        anp::authentication::PROFILE_IDENTITY_DISCOVERY_V2,
        anp::authentication::PROFILE_DIRECT_BASE_V2,
        anp::authentication::PROFILE_DIRECT_E2EE_V2,
        anp::authentication::PROFILE_GROUP_BASE_V2,
        anp::authentication::PROFILE_GROUP_E2EE_V2,
    ]
    .into_iter()
    .map(ToOwned::to_owned)
    .collect()
}

fn public_key_multibase(public_key: &anp::PublicKeyMaterial) -> String {
    let (codec, bytes): ([u8; 2], Vec<u8>) = match public_key {
        anp::PublicKeyMaterial::Ed25519(key) => ([0xed, 0x01], key.to_bytes().to_vec()),
        anp::PublicKeyMaterial::X25519(key) => ([0xec, 0x01], key.to_vec()),
        _ => panic!("test only uses Ed25519 and X25519"),
    };
    let mut encoded = codec.to_vec();
    encoded.extend(bytes);
    format!("z{}", bs58::encode(encoded).into_string())
}

fn manifest_contains(document: &Value, device_id: &str) -> bool {
    anp::authentication::validate_device_manifest(document)
        .unwrap()
        .unwrap()
        .devices
        .iter()
        .any(|device| device.device_id == device_id)
}
