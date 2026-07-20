use super::*;

use std::collections::VecDeque;
use std::path::Path;
use std::sync::{Arc, Mutex};

const FIXED_DOCUMENT_HASH: &str = "sha256:UD5TmycQ6gS539AFNjM5cGoQUmeq2fQGPpwD00lMPlg";
const FIXED_CHALLENGE_HASH: &str = "sha256:CNkA2F600Hf0nbZLaSSALDLOq6wK2OC7fXmxIhYAbzs";

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

fn open_vault_core(root: &Path) -> crate::ImCore {
    open_vault_core_with_prekey_flags(root, false, false)
}

fn open_vault_core_with_prekey_flags(
    root: &Path,
    direct_e2ee_enabled: bool,
    root_transfer_enabled: bool,
) -> crate::ImCore {
    open_vault_core_with_messaging_flags(root, direct_e2ee_enabled, root_transfer_enabled, false)
}

fn open_vault_core_with_messaging_flags(
    root: &Path,
    direct_e2ee_enabled: bool,
    root_transfer_enabled: bool,
    group_e2ee_enabled: bool,
) -> crate::ImCore {
    crate::ImCore::new_with_options(
        test_config(),
        test_paths(root),
        crate::ImCoreOpenOptions::default()
            .with_identity_secret_vault(
                crate::IdentitySecretStoragePolicy::VaultRequired,
                crate::ImCoreSecretVaultOptions::new(
                    crate::vault::DeviceVaultRootKey::from_bytes([47_u8; 32]),
                    root.join("vault"),
                    "join-test-workspace",
                    "join-test-vault-device",
                ),
            )
            .with_multi_device_direct_e2ee_enabled(direct_e2ee_enabled)
            .with_multi_device_root_transfer_enabled(root_transfer_enabled)
            .with_multi_device_group_e2ee_enabled(group_e2ee_enabled),
    )
    .unwrap()
}

#[derive(Clone)]
struct ActivationTokenRemote {
    calls: Arc<Mutex<Vec<(String, String, u64)>>>,
    results: Arc<
        Mutex<
            VecDeque<
                crate::ImResult<
                    crate::internal::identity_wire::device_genesis::DeviceTokenIssueResult,
                >,
            >,
        >,
    >,
}

impl crate::internal::identity_device_join_runtime::DeviceJoinNewDeviceRemote
    for ActivationTokenRemote
{
    async fn create(
        &mut self,
        _request: crate::internal::identity_device_join_runtime::DeviceJoinRemoteCreateRequest<'_>,
    ) -> crate::ImResult<crate::internal::identity_device_join_runtime::DeviceJoinRemoteCreateResult>
    {
        panic!("pending activation must not create another Join session")
    }

    async fn status(
        &mut self,
        _expected_join_session_id: &str,
        _join_session_token: &SecretBytes,
    ) -> crate::ImResult<
        crate::internal::identity_device_join_runtime::DeviceJoinRemoteNewDeviceStatus,
    > {
        panic!("pending activation must not poll Join status")
    }

    async fn submit_response(
        &mut self,
        _request: crate::internal::identity_device_join_runtime::DeviceJoinRemoteResponseRequest<
            '_,
        >,
    ) -> crate::ImResult<
        crate::internal::identity_device_join_runtime::DeviceJoinRemoteTransitionResult,
    > {
        panic!("pending activation must not resubmit the Join response")
    }

    async fn issue_device_token(
        &mut self,
        prepared: &crate::internal::identity_wire::device_genesis::PreparedDeviceTokenIssue,
        expected_auth_generation: u64,
    ) -> crate::ImResult<crate::internal::identity_wire::device_genesis::DeviceTokenIssueResult>
    {
        self.calls.lock().unwrap().push((
            prepared.operation_id.clone(),
            prepared.authorization.clone(),
            expected_auth_generation,
        ));
        self.results
            .lock()
            .unwrap()
            .pop_front()
            .expect("queued token issue result")
    }
}

struct PanicDidResolverTransport;

impl crate::internal::transport::AsyncRawJsonTransport for PanicDidResolverTransport {
    async fn get_json_url(
        &mut self,
        _url: &str,
        _headers: std::collections::BTreeMap<String, String>,
    ) -> crate::ImResult<Value> {
        panic!("pending activation must use its persisted DID Document")
    }
}

#[derive(Default)]
struct RecordingJoinPrekeyTransport {
    fail_next: bool,
    calls: Vec<(String, String, Value)>,
}

impl crate::internal::transport::AsyncAuthenticatedRpcTransport for RecordingJoinPrekeyTransport {
    async fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> crate::ImResult<Value> {
        self.calls
            .push((endpoint.to_owned(), method.to_owned(), params.clone()));
        if self.fail_next {
            self.fail_next = false;
            return Err(crate::ImError::TransportUnavailable {
                detail: "simulated PreKey publish failure".to_owned(),
            });
        }
        let request = serde_json::json!({"method": method, "params": params});
        let (_, body) = anp::direct_e2ee::parse_publish_prekey_bundle_request_v2(&request)
            .map_err(|_| crate::ImError::PermissionDenied)?;
        Ok(serde_json::json!({
            "published": true,
            "owner_did": body.prekey_bundle.owner_did,
            "owner_device_id": body.prekey_bundle.owner_device_id,
            "bundle_id": body.prekey_bundle.bundle_id,
            "published_at": "2026-07-20T00:00:01Z",
            "published_opk_count": body.one_time_prekeys.len(),
        }))
    }
}

struct RecordingJoinPrekeyPublisher {
    transport: RecordingJoinPrekeyTransport,
}

impl crate::internal::identity_device_join_runtime::DeviceJoinPrekeyPublisher
    for RecordingJoinPrekeyPublisher
{
    async fn publish(
        &mut self,
        core: &crate::core::ImCore,
        client: &crate::core::ImClient,
    ) -> crate::ImResult<()> {
        crate::internal::secure_direct::v2_prekey_runtime::ensure_local_prekey_published_from_authorized_document_with_transport(
            core,
            client,
            &mut self.transport,
        )
        .await?;
        Ok(())
    }
}

struct PanicJoinPrekeyPublisher;

impl crate::internal::identity_device_join_runtime::DeviceJoinPrekeyPublisher
    for PanicJoinPrekeyPublisher
{
    async fn publish(
        &mut self,
        _core: &crate::core::ImCore,
        _client: &crate::core::ImClient,
    ) -> crate::ImResult<()> {
        panic!("disabled Join PreKey publication must not invoke the publisher")
    }
}

#[derive(Default)]
struct RecordingJoinGroupKeyPackagePublisher {
    fail_next: bool,
    calls: Vec<crate::internal::identity_device_join_runtime::DeviceJoinGroupKeyPackagePublish>,
}

impl crate::internal::identity_device_join_runtime::DeviceJoinGroupKeyPackagePublisher
    for RecordingJoinGroupKeyPackagePublisher
{
    async fn publish(
        &mut self,
        _client: &crate::core::ImClient,
        publish: &crate::internal::identity_device_join_runtime::DeviceJoinGroupKeyPackagePublish,
    ) -> crate::ImResult<()> {
        self.calls.push(publish.clone());
        if self.fail_next {
            self.fail_next = false;
            return Err(crate::ImError::TransportUnavailable {
                detail: "simulated Group KeyPackage publish failure".to_owned(),
            });
        }
        Ok(())
    }
}

struct PanicJoinGroupKeyPackagePublisher;

impl crate::internal::identity_device_join_runtime::DeviceJoinGroupKeyPackagePublisher
    for PanicJoinGroupKeyPackagePublisher
{
    async fn publish(
        &mut self,
        _client: &crate::core::ImClient,
        _publish: &crate::internal::identity_device_join_runtime::DeviceJoinGroupKeyPackagePublish,
    ) -> crate::ImResult<()> {
        panic!("disabled Join Group KeyPackage publication must not invoke the publisher")
    }
}

fn open_vnext_identity_core(
    root: &Path,
    role: crate::internal::identity_device_state::DeviceAuthorizationRole,
) -> (crate::ImCore, Value, crate::ids::Did) {
    open_vnext_identity_core_with_prekey_flags(root, role, false, false)
}

fn open_vnext_identity_core_with_prekey_flags(
    root: &Path,
    role: crate::internal::identity_device_state::DeviceAuthorizationRole,
    direct_e2ee_enabled: bool,
    root_transfer_enabled: bool,
) -> (crate::ImCore, Value, crate::ids::Did) {
    open_vnext_identity_core_with_messaging_flags(
        root,
        role,
        direct_e2ee_enabled,
        root_transfer_enabled,
        false,
    )
}

fn open_vnext_identity_core_with_messaging_flags(
    root: &Path,
    role: crate::internal::identity_device_state::DeviceAuthorizationRole,
    direct_e2ee_enabled: bool,
    root_transfer_enabled: bool,
    group_e2ee_enabled: bool,
) -> (crate::ImCore, Value, crate::ids::Did) {
    use crate::internal::identity_device_state::{
        DeviceAuthorizationProjection, DeviceAuthorizationStatus, IdentityDeviceMode,
        IdentityDeviceState, IdentityInternalCheckpoint, IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
    };

    let generated =
        crate::internal::identity_generation::generate_vnext_handle_identity_with_default_daemon_subkey(
            "awiki.test",
            "alice",
            None,
            None,
        )
        .unwrap();
    let document_hash = canonical_hash(&generated.did_document).unwrap();
    let paths = test_paths(root);
    let vault = Arc::new(crate::vault::FileSecretVault::new(
        crate::vault::DeviceVaultRootKey::from_bytes([47_u8; 32]),
        crate::vault::FileSecretVaultStore::new(root.join("vault")),
    ));
    crate::internal::identity_store::IdentityStore::new(&paths.identities)
        .save_identity_with_secret_storage(
            crate::internal::identity_store::SaveIdentityInput {
                local_alias: "alice".to_owned(),
                did: generated.did.clone(),
                unique_id: generated.unique_id.clone(),
                user_id: "user-1".to_owned(),
                display_name: "Alice".to_owned(),
                handle: "alice".to_owned(),
                full_handle: "alice.awiki.test".to_owned(),
                jwt_token: "device-token".to_owned(),
                did_document: Some(generated.did_document.clone()),
                key_mode: crate::internal::identity_store::SaveIdentityKeyMode::VNext {
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
                        role,
                        management_ready: role
                            == crate::internal::identity_device_state::DeviceAuthorizationRole::Admin,
                        auth_generation: 1,
                    }),
                    checkpoint: Some(IdentityInternalCheckpoint {
                        document_version: 7,
                        document_hash,
                        registry_version: 3,
                    }),
                }),
                key1_private_pem: if role
                    == crate::internal::identity_device_state::DeviceAuthorizationRole::Admin
                {
                    generated.root_private_pem
                } else {
                    String::new()
                },
                key1_public_pem: generated.root_public_pem,
                e2ee_signing_private_pem: generated.device_signing_private_pem,
                e2ee_agreement_private_pem: generated.device_e2ee_private_pem,
                daemon_subkey_package: None,
                make_default: true,
            },
            crate::internal::identity_store::SaveIdentitySecretStorage::Vault {
                workspace_id: "join-test-workspace".to_owned(),
                device_id: "join-test-vault-device".to_owned(),
                vault,
            },
        )
        .unwrap();
    let did = generated.did;
    let document = generated.did_document;
    (
        open_vault_core_with_messaging_flags(
            root,
            direct_e2ee_enabled,
            root_transfer_enabled,
            group_e2ee_enabled,
        ),
        document,
        did,
    )
}

#[tokio::test]
async fn direct_only_join_activation_publishes_prekey_and_retries_safely() {
    use crate::internal::identity_device_join_runtime::publish_v2_prekeys_after_activation_with_publisher;
    use crate::internal::identity_device_state::DeviceAuthorizationRole;

    let root = tempfile::tempdir().unwrap();
    let (core, document, did) = open_vnext_identity_core_with_prekey_flags(
        root.path(),
        DeviceAuthorizationRole::Member,
        true,
        false,
    );
    let manifest = anp::authentication::validate_device_manifest(&document)
        .unwrap()
        .unwrap();
    let session = crate::identity::DeviceJoinSessionSummary {
        join_session_id: "join-direct-prekey".to_owned(),
        did,
        protocol_device_id: crate::ids::ProtocolDeviceId::parse(
            manifest.devices[0].device_id.clone(),
        )
        .unwrap(),
        side: DeviceJoinSide::NewDevice,
        phase: DeviceJoinLocalPhase::Authorized,
        join_request_hash: "sha256:authorized-join".to_owned(),
        challenge_id: Some("challenge-direct-prekey".to_owned()),
        expires_at: format_time(OffsetDateTime::now_utc() + Duration::minutes(5)).unwrap(),
    };
    let mut publisher = RecordingJoinPrekeyPublisher {
        transport: RecordingJoinPrekeyTransport {
            fail_next: true,
            calls: Vec::new(),
        },
    };

    let error = publish_v2_prekeys_after_activation_with_publisher(&core, &session, &mut publisher)
        .await
        .unwrap_err();
    assert!(matches!(error, crate::ImError::TransportUnavailable { .. }));
    let after_failure = core
        .identities()
        .device_summary(crate::identity::IdentitySelector::Default)
        .unwrap();
    assert_eq!(
        after_failure.readiness,
        crate::identity::IdentityDeviceReadiness::MemberReady
    );

    publish_v2_prekeys_after_activation_with_publisher(&core, &session, &mut publisher)
        .await
        .unwrap();
    assert_eq!(publisher.transport.calls.len(), 2);
    assert_eq!(publisher.transport.calls[0].0, "/im/rpc");
    assert_eq!(
        publisher.transport.calls[0].1,
        "direct.e2ee.publish_prekey_bundle"
    );
    assert_eq!(
        publisher.transport.calls[0].2, publisher.transport.calls[1].2,
        "retry must reuse the persisted PreKey bundle and operation"
    );

    let off_root = tempfile::tempdir().unwrap();
    let (off_core, off_document, off_did) =
        open_vnext_identity_core(off_root.path(), DeviceAuthorizationRole::Member);
    let off_manifest = anp::authentication::validate_device_manifest(&off_document)
        .unwrap()
        .unwrap();
    let mut off_session = session;
    off_session.did = off_did;
    off_session.protocol_device_id =
        crate::ids::ProtocolDeviceId::parse(off_manifest.devices[0].device_id.clone()).unwrap();
    let mut panic_publisher = PanicJoinPrekeyPublisher;
    publish_v2_prekeys_after_activation_with_publisher(
        &off_core,
        &off_session,
        &mut panic_publisher,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn group_v2_join_activation_publishes_key_package_and_retries_safely() {
    use crate::internal::identity_device_join_runtime::publish_v2_group_key_package_after_activation_with_publisher;
    use crate::internal::identity_device_state::DeviceAuthorizationRole;

    let root = tempfile::tempdir().unwrap();
    let (core, _document, did) = open_vnext_identity_core_with_messaging_flags(
        root.path(),
        DeviceAuthorizationRole::Member,
        false,
        false,
        true,
    );
    let current_device_id = core
        .identities()
        .device_summary(crate::identity::IdentitySelector::Default)
        .unwrap()
        .protocol_device_id
        .unwrap();
    let session = crate::identity::DeviceJoinSessionSummary {
        join_session_id: "join-group-key-package".to_owned(),
        did,
        protocol_device_id: current_device_id,
        side: DeviceJoinSide::NewDevice,
        phase: DeviceJoinLocalPhase::Authorized,
        join_request_hash: "sha256:authorized-group-join".to_owned(),
        challenge_id: Some("challenge-group-key-package".to_owned()),
        expires_at: format_time(OffsetDateTime::now_utc() + Duration::minutes(5)).unwrap(),
    };
    let mut publisher = RecordingJoinGroupKeyPackagePublisher {
        fail_next: true,
        calls: Vec::new(),
    };

    let error = publish_v2_group_key_package_after_activation_with_publisher(
        &core,
        &session,
        &mut publisher,
    )
    .await
    .unwrap_err();
    assert!(
        matches!(error, crate::ImError::TransportUnavailable { .. }),
        "unexpected P6 Join publication error: {error:?}"
    );
    let after_failure = core
        .identities()
        .device_summary(crate::identity::IdentitySelector::Default)
        .unwrap();
    assert_eq!(
        after_failure.readiness,
        crate::identity::IdentityDeviceReadiness::MemberReady
    );

    publish_v2_group_key_package_after_activation_with_publisher(&core, &session, &mut publisher)
        .await
        .unwrap();
    assert_eq!(publisher.calls.len(), 2);
    assert_eq!(publisher.calls[0], publisher.calls[1]);
    assert_eq!(
        publisher.calls[0].expected_device_id,
        session.protocol_device_id.as_str()
    );
    assert!(publisher.calls[0]
        .operation_id
        .starts_with("join-p6-publish-"));
    assert!(publisher.calls[0].key_package_id.starts_with("join-kp-"));

    let mut mismatched_session = session.clone();
    mismatched_session.protocol_device_id =
        crate::ids::ProtocolDeviceId::parse("dev-not-the-current-device").unwrap();
    let mut mismatch_publisher = RecordingJoinGroupKeyPackagePublisher::default();
    assert!(matches!(
        publish_v2_group_key_package_after_activation_with_publisher(
            &core,
            &mismatched_session,
            &mut mismatch_publisher,
        )
        .await,
        Err(crate::ImError::InvalidInput { .. })
    ));
    assert!(mismatch_publisher.calls.is_empty());

    let off_root = tempfile::tempdir().unwrap();
    let (off_core, off_document, off_did) =
        open_vnext_identity_core(off_root.path(), DeviceAuthorizationRole::Member);
    let off_manifest = anp::authentication::validate_device_manifest(&off_document)
        .unwrap()
        .unwrap();
    let mut off_session = session;
    off_session.did = off_did;
    off_session.protocol_device_id =
        crate::ids::ProtocolDeviceId::parse(off_manifest.devices[0].device_id.clone()).unwrap();
    let mut panic_publisher = PanicJoinGroupKeyPackagePublisher;
    publish_v2_group_key_package_after_activation_with_publisher(
        &off_core,
        &off_session,
        &mut panic_publisher,
    )
    .await
    .unwrap();
}

fn sample_proof() -> DeviceProof {
    DeviceProof {
        proof_type: DEVICE_PROOF_TYPE.to_owned(),
        key_id: "did:wba:awiki.test:alice#device-a-sign".to_owned(),
        created_at: "2026-07-19T01:02:03Z".to_owned(),
        expires_at: "2026-07-19T01:07:03Z".to_owned(),
        nonce: "proof-nonce-fixed".to_owned(),
        signature: "signature-is-not-part-of-proof-bytes".to_owned(),
    }
}

#[test]
fn challenge_plaintext_canonical_bytes_and_hash_fixture_are_frozen() {
    let checkpoint = InternalCheckpoint {
        document_version: 7,
        document_hash: FIXED_DOCUMENT_HASH.to_owned(),
    };
    let canonical =
        encode_challenge_plaintext(&[0xa5_u8; JOIN_CHALLENGE_LEN], &checkpoint).unwrap();
    let expected = concat!(
        r#"{"document_hash":"sha256:UD5TmycQ6gS539AFNjM5cGoQUmeq2fQGPpwD00lMPlg","document_version":7,"random_challenge_b64u":"paWlpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaU","type":"awiki.device.join.challenge-plaintext.v1"}"#,
    );

    assert_eq!(canonical.as_slice(), expected.as_bytes());
    assert_eq!(
        hash_bytes(canonical.as_slice()),
        FIXED_CHALLENGE_HASH,
        "cross-repository challenge plaintext fixture changed"
    );
    let parsed = parse_challenge_plaintext(SecretBytes::from_vec(canonical.to_vec())).unwrap();
    assert_eq!(parsed.checkpoint, checkpoint);
    assert_eq!(
        parsed.canonical_plaintext.expose_secret(),
        expected.as_bytes()
    );
}

#[test]
fn challenge_plaintext_rejects_noncanonical_unknown_or_invalid_fields() {
    let noncanonical = format!(
        "{{\"type\":\"{JOIN_CHALLENGE_PLAINTEXT_TYPE}\",\"random_challenge_b64u\":\"{}\",\"document_version\":7,\"document_hash\":\"{FIXED_DOCUMENT_HASH}\"}}",
        URL_SAFE_NO_PAD.encode([0xa5_u8; JOIN_CHALLENGE_LEN])
    );
    assert!(matches!(
        parse_challenge_plaintext(SecretBytes::from_vec(noncanonical.into_bytes())),
        Err(crate::ImError::PermissionDenied)
    ));

    let unknown = canonical_bytes(&json!({
        "type": JOIN_CHALLENGE_PLAINTEXT_TYPE,
        "random_challenge_b64u": URL_SAFE_NO_PAD.encode([0xa5_u8; JOIN_CHALLENGE_LEN]),
        "document_version": 7,
        "document_hash": FIXED_DOCUMENT_HASH,
        "unexpected": true,
    }))
    .unwrap();
    assert!(matches!(
        parse_challenge_plaintext(SecretBytes::from_vec(unknown)),
        Err(crate::ImError::PermissionDenied)
    ));

    let invalid_random = canonical_bytes(&json!({
        "type": JOIN_CHALLENGE_PLAINTEXT_TYPE,
        "random_challenge_b64u": URL_SAFE_NO_PAD.encode([0xa5_u8; JOIN_CHALLENGE_LEN - 1]),
        "document_version": 7,
        "document_hash": FIXED_DOCUMENT_HASH,
    }))
    .unwrap();
    assert!(matches!(
        parse_challenge_plaintext(SecretBytes::from_vec(invalid_random)),
        Err(crate::ImError::PermissionDenied)
    ));
}

#[test]
fn challenge_plaintext_checkpoint_must_match_local_did_resolution() {
    let encrypted_checkpoint = InternalCheckpoint {
        document_version: 7,
        document_hash: FIXED_DOCUMENT_HASH.to_owned(),
    };
    let local_checkpoint = InternalCheckpoint {
        document_version: 8,
        document_hash: FIXED_DOCUMENT_HASH.to_owned(),
    };

    assert!(matches!(
        ensure_challenge_checkpoint(&encrypted_checkpoint, &local_checkpoint),
        Err(crate::ImError::PermissionDenied)
    ));
    ensure_challenge_checkpoint(&encrypted_checkpoint, &encrypted_checkpoint).unwrap();
}

#[test]
fn device_proof_canonical_bytes_and_hash_fixture_are_frozen() {
    let params = json!({
        "z": 3,
        "operation_id": "op-fixed",
        "nested": {"b": 2, "a": 1},
    });
    let canonical = device_proof_bytes(
        &sample_proof(),
        JOIN_CHALLENGE_PURPOSE,
        JOIN_CHALLENGE_METHOD,
        &params,
    )
    .unwrap();
    let expected = concat!(
        r#"{"created_at":"2026-07-19T01:02:03Z","expires_at":"2026-07-19T01:07:03Z","key_id":"did:wba:awiki.test:alice#device-a-sign","method":"device_join_challenge","nonce":"proof-nonce-fixed","params":{"nested":{"a":1,"b":2},"operation_id":"op-fixed","z":3},"purpose":"awiki.device.join.challenge.v1","type":"awiki-device-signature-v1"}"#,
    );

    assert_eq!(canonical, expected.as_bytes());
    assert_eq!(
        hash_bytes(&canonical),
        "sha256:MbTQijG_NDem8bMN06IFyaZ7Etu-AR87dZersdKmDwg",
        "cross-repository proof fixture changed"
    );
}

#[test]
fn join_transcript_hash_and_sas_fixture_are_frozen() {
    let transcript = json!({
        "type": "awiki.device.join.transcript.v1",
        "did": "did:wba:awiki.test:alice",
        "join_session_id": "join-fixed",
        "admin_device_id": "admin-a",
        "new_device_id": "new-b",
        "join_request_hash": "sha256:join-fixed",
        "challenge_id": "challenge-fixed",
        "challenge_hash": FIXED_CHALLENGE_HASH,
        "new_pairing_public_key": "new-pairing-fixed",
        "admin_pairing_public_key": "admin-pairing-fixed",
        "new_signing_public_key": {
            "type": "Multikey",
            "id": "did:wba:awiki.test:alice#new-b-sign",
            "controller": "did:wba:awiki.test:alice",
            "publicKeyMultibase": "zSigningFixed"
        },
        "new_e2ee_public_key": {
            "type": "X25519KeyAgreementKey2019",
            "id": "did:wba:awiki.test:alice#new-b-e2ee",
            "controller": "did:wba:awiki.test:alice",
            "publicKeyMultibase": "zE2eeFixed"
        },
        "document_version": 7,
        "document_hash": FIXED_DOCUMENT_HASH
    });
    let canonical = canonical_bytes(&transcript).unwrap();

    assert_eq!(
        hash_bytes(&canonical),
        "sha256:EE0Bm2QXgeNqSCSmkBcKXWIZLzhm4HHBga21ipRqPnc",
        "cross-repository transcript fixture changed"
    );
    assert_eq!(
        derive_sas(&[0x42_u8; 32], &transcript).unwrap(),
        "791912",
        "both devices must derive the same six-digit SAS from the frozen transcript"
    );

    let mut tampered = transcript;
    tampered["new_device_id"] = Value::String("attacker-device".to_owned());
    assert_ne!(derive_sas(&[0x42_u8; 32], &tampered).unwrap(), "791912");
}

#[test]
fn encrypted_challenge_round_trips_and_binds_aad() {
    let did = "did:wba:awiki.test:alice";
    let signing_private =
        anp::PrivateKeyMaterial::Ed25519(ed25519_dalek::SigningKey::from_bytes(&[3_u8; 32]));
    let new_e2ee_private = anp::PrivateKeyMaterial::X25519(X25519StaticSecret::from([5_u8; 32]));
    let new_pairing_private = anp::PrivateKeyMaterial::X25519(X25519StaticSecret::from([7_u8; 32]));
    let admin_pairing_private =
        anp::PrivateKeyMaterial::X25519(X25519StaticSecret::from([11_u8; 32]));
    let admin_pairing_public_key = x25519_public_b64u(&admin_pairing_private.public_key()).unwrap();
    let join_request = DeviceJoinRequest {
        request_type: DEVICE_JOIN_REQUEST_TYPE.to_owned(),
        did: did.to_owned(),
        join_session_id: "join-fixed".to_owned(),
        device_id: "new-b".to_owned(),
        signing_public_key: verification_method(
            did,
            &format!("{did}#new-b-sign"),
            "Multikey",
            &signing_private.public_key(),
        )
        .unwrap(),
        e2ee_public_key: verification_method(
            did,
            &format!("{did}#new-b-e2ee"),
            "X25519KeyAgreementKey2019",
            &new_e2ee_private.public_key(),
        )
        .unwrap(),
        pairing_public_key: x25519_public_b64u(&new_pairing_private.public_key()).unwrap(),
        profiles: DEVICE_JOIN_VNEXT_PROFILES
            .iter()
            .map(|value| (*value).to_owned())
            .collect(),
        requested_role: "member".to_owned(),
        issued_at: "2026-07-19T01:02:03Z".to_owned(),
        expires_at: "2026-07-19T01:12:03Z".to_owned(),
        signature: "unused-in-this-crypto-fixture".to_owned(),
    };
    let join_request_hash = canonical_hash(&serde_json::to_value(&join_request).unwrap()).unwrap();
    let checkpoint = InternalCheckpoint {
        document_version: 7,
        document_hash: FIXED_DOCUMENT_HASH.to_owned(),
    };
    let challenge_plaintext =
        encode_challenge_plaintext(&[0xa5_u8; JOIN_CHALLENGE_LEN], &checkpoint).unwrap();
    let encrypted = encrypt_challenge(
        &admin_pairing_private,
        &join_request,
        &join_request_hash,
        "challenge-fixed",
        "admin-a",
        &admin_pairing_public_key,
        "2026-07-19T01:07:03Z",
        challenge_plaintext.as_slice(),
    )
    .unwrap();
    let challenge = DeviceJoinChallenge {
        operation_id: "op-fixed".to_owned(),
        join_session_id: join_request.join_session_id.clone(),
        challenge_id: "challenge-fixed".to_owned(),
        admin_device_id: "admin-a".to_owned(),
        admin_pairing_public_key,
        ciphertext: encrypted,
        challenge_expires_at: "2026-07-19T01:07:03Z".to_owned(),
        authorizing_device_proof: sample_proof(),
    };

    let decrypted = decrypt_challenge(
        &new_e2ee_private,
        &join_request,
        &join_request_hash,
        &challenge,
    )
    .unwrap();
    assert_eq!(
        decrypted.canonical_plaintext.expose_secret(),
        challenge_plaintext.as_slice()
    );
    assert_eq!(decrypted.checkpoint, checkpoint);
    let serialized_challenge = serde_json::to_string(&challenge).unwrap();
    assert!(!serialized_challenge.contains("random_challenge_b64u"));
    assert!(!serialized_challenge.contains(FIXED_DOCUMENT_HASH));

    let mut tampered = challenge;
    tampered.admin_device_id = "attacker-admin".to_owned();
    assert!(matches!(
        decrypt_challenge(
            &new_e2ee_private,
            &join_request,
            &join_request_hash,
            &tampered,
        ),
        Err(crate::ImError::PermissionDenied)
    ));
}

#[test]
fn two_devices_derive_the_same_sas_and_approval_is_restart_safe() {
    use crate::internal::identity_device_state::{
        DeviceAuthorizationRole, DeviceAuthorizationStatus, IdentityInternalCheckpoint,
    };

    let root = tempfile::tempdir().unwrap();
    let (core, document, did) =
        open_vnext_identity_core(root.path(), DeviceAuthorizationRole::Admin);
    let document_hash = canonical_hash(&document).unwrap();
    let started = core
        .device_join()
        .start(DeviceJoinStartRequest {
            operation_id: "start-full-pairing".to_owned(),
            did,
            ttl_seconds: 300,
        })
        .unwrap();
    let prepared = core
        .device_join()
        .prepare_admin_challenge(DeviceJoinAdminPrepareRequest {
            admin_identity: crate::identity::IdentitySelector::Default,
            operation_id: "claim-and-challenge-full-pairing".to_owned(),
            join_request: started.join_request,
            challenge_ttl_seconds: 180,
            document_version: 7,
            document_hash: document_hash.clone(),
        })
        .unwrap();
    let responded = core
        .device_join()
        .respond_as_new_device(DeviceJoinNewDeviceRespondRequest {
            operation_id: "respond-full-pairing".to_owned(),
            challenge: prepared.challenge,
            admin_did_document: document,
            document_version: 7,
            document_hash: document_hash.clone(),
        })
        .unwrap();

    assert_eq!(prepared.sas, responded.sas);
    assert_eq!(prepared.sas.len(), 6);
    assert!(prepared.sas.bytes().all(|value| value.is_ascii_digit()));

    let mut tampered = responded.response.clone();
    tampered.pairing_transcript_hash =
        "sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_owned();
    assert!(matches!(
        core.device_join()
            .verify_response_as_admin(DeviceJoinAdminVerifyRequest {
                operation_id: "verify-tampered-response".to_owned(),
                join_session_id: started.session.join_session_id.clone(),
                response: tampered,
            }),
        Err(crate::ImError::PermissionDenied)
    ));

    let verify_request = DeviceJoinAdminVerifyRequest {
        operation_id: "verify-full-pairing".to_owned(),
        join_session_id: started.session.join_session_id.clone(),
        response: responded.response,
    };
    let verified = core
        .device_join()
        .verify_response_as_admin(verify_request.clone())
        .unwrap();
    assert_eq!(verified.sas, responded.sas);
    assert_eq!(
        core.device_join()
            .verify_response_as_admin(verify_request)
            .unwrap(),
        verified,
        "an exact response replay is an idempotent retry"
    );
    assert!(matches!(
        core.device_join()
            .verify_response_as_admin(DeviceJoinAdminVerifyRequest {
                operation_id: "verify-conflicting-replay".to_owned(),
                join_session_id: started.session.join_session_id.clone(),
                response: verified_response_for_conflict(&core, &started.session.join_session_id),
            }),
        Err(crate::ImError::InvalidInput {
            field: Some(field),
            ..
        }) if field == "operation_id"
    ));

    let checkpoint = IdentityInternalCheckpoint {
        document_version: 7,
        document_hash,
        registry_version: 3,
    };
    let now = format_time(OffsetDateTime::now_utc()).unwrap();
    assert!(matches!(
        prepare_admin_approval(
            &core,
            "approve-full-pairing",
            &started.session.join_session_id,
            &checkpoint,
            DeviceAuthorizationRole::Member,
            &now,
            false,
        ),
        Err(crate::ImError::PermissionDenied)
    ));
    let approval = prepare_admin_approval(
        &core,
        "approve-full-pairing",
        &started.session.join_session_id,
        &checkpoint,
        DeviceAuthorizationRole::Member,
        &now,
        true,
    )
    .unwrap();
    assert_eq!(approval.role, DeviceAuthorizationRole::Member);
    assert!(approval.pairing_confirmation.sas_confirmed);
    assert_eq!(
        prepare_admin_approval(
            &core,
            "approve-full-pairing",
            &started.session.join_session_id,
            &checkpoint,
            DeviceAuthorizationRole::Member,
            &now,
            true,
        )
        .unwrap(),
        approval,
        "a persisted approval intent survives an exact retry"
    );

    drop(core);
    let restarted = open_vault_core(root.path());
    assert_eq!(
        load_prepared_admin_approval(&restarted, &started.session.join_session_id)
            .unwrap()
            .unwrap(),
        approval
    );

    let store = JoinStateStore::new(&restarted);
    let mut persisted = store
        .load(&started.session.join_session_id, DeviceJoinSide::Admin)
        .unwrap()
        .unwrap();
    persisted
        .approval
        .as_mut()
        .unwrap()
        .authorizing_device_proof
        .expires_at = "2020-01-01T00:00:00Z".to_owned();
    store.save(&persisted).unwrap();
    drop(restarted);

    let restarted = open_vault_core(root.path());
    assert!(reset_expired_admin_approval_after_remote_poll(
        &restarted,
        &started.session.join_session_id,
        &started.session.expires_at,
        OffsetDateTime::now_utc(),
    )
    .unwrap());
    assert!(
        load_prepared_admin_approval(&restarted, &started.session.join_session_id)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        restarted
            .device_join()
            .session(&started.session.join_session_id, DeviceJoinSide::Admin)
            .unwrap()
            .phase,
        DeviceJoinLocalPhase::ResponseVerified
    );
    let renewed_at = format_time(OffsetDateTime::now_utc()).unwrap();
    let renewed = prepare_admin_approval(
        &restarted,
        "approve-full-pairing-renewed",
        &started.session.join_session_id,
        &checkpoint,
        DeviceAuthorizationRole::Member,
        &renewed_at,
        true,
    )
    .unwrap();
    assert_eq!(renewed.operation_id, "approve-full-pairing-renewed");
    assert_eq!(renewed.pairing_confirmation.user_presence_at, renewed_at);
    assert_eq!(renewed.new_document["proof"]["domain"], "awiki.test");

    let manifest = anp::authentication::validate_device_manifest(&renewed.new_document)
        .unwrap()
        .unwrap();
    let added = manifest
        .devices
        .iter()
        .find(|device| device.device_id == started.session.protocol_device_id.as_str())
        .unwrap();
    let authorization =
        crate::internal::identity_device_join_runtime::DeviceJoinRemoteAuthorization {
            checkpoint: IdentityInternalCheckpoint {
                document_version: 8,
                document_hash: canonical_hash(&renewed.new_document).unwrap(),
                registry_version: 4,
            },
            device: crate::internal::identity_device_join_runtime::DeviceJoinRemoteDeviceSummary {
                device_id: added.device_id.clone(),
                signing_key_id: added.signing_key_id.clone(),
                e2ee_key_id: added.e2ee_key_id.clone(),
                status: DeviceAuthorizationStatus::Active,
                role: DeviceAuthorizationRole::Member,
                management_ready: false,
                auth_generation: 1,
            },
        };
    let store = JoinStateStore::new(&restarted);
    let mut expired = store
        .load(&started.session.join_session_id, DeviceJoinSide::Admin)
        .unwrap()
        .unwrap();
    expired.phase = DeviceJoinLocalPhase::Expired;
    store.save(&expired).unwrap();

    let authorized = mark_admin_approval_consumed_after_remote_poll(
        &restarted,
        &started.session.join_session_id,
        &started.session.expires_at,
        &authorization,
    )
    .unwrap();
    assert_eq!(authorized.phase, DeviceJoinLocalPhase::Authorized);
    assert_eq!(
        load_prepared_admin_approval(&restarted, &started.session.join_session_id)
            .unwrap()
            .unwrap()
            .authorizing_device_proof,
        renewed.authorizing_device_proof,
        "remote consumed reconciliation must not regenerate an approval proof"
    );
}

fn verified_response_for_conflict(
    core: &crate::ImCore,
    join_session_id: &str,
) -> DeviceJoinChallengeResponse {
    JoinStateStore::new(core)
        .load(join_session_id, DeviceJoinSide::Admin)
        .unwrap()
        .unwrap()
        .response
        .unwrap()
}

#[test]
fn ordinary_member_cannot_prepare_an_admin_claim() {
    use crate::internal::identity_device_state::DeviceAuthorizationRole;

    let root = tempfile::tempdir().unwrap();
    let (core, _, _) = open_vnext_identity_core(root.path(), DeviceAuthorizationRole::Member);
    let session_expires_at =
        format_time(OffsetDateTime::now_utc() + Duration::seconds(300)).unwrap();

    assert!(matches!(
        prepare_admin_claim_intent(
            &core,
            crate::identity::IdentitySelector::Default,
            "member-cannot-claim",
            "join-member-denied",
            &session_expires_at,
        ),
        Err(crate::ImError::PermissionDenied)
    ));
    assert!(
        !JoinStateStore::new(&core)
            .claim_intent_path("join-member-denied")
            .exists(),
        "a member denial must happen before persisting an approval intent"
    );
}

#[test]
fn pending_join_is_restart_safe_idempotent_and_stores_secrets_only_in_vault() {
    let root = tempfile::tempdir().unwrap();
    let did = crate::ids::Did::parse("did:wba:awiki.test:alice").unwrap();
    let core = open_vault_core(root.path());
    let request = DeviceJoinStartRequest {
        operation_id: "start-fixed-operation".to_owned(),
        did: did.clone(),
        ttl_seconds: 300,
    };
    let started = core.device_join().start(request.clone()).unwrap();

    assert_eq!(started.session.side, DeviceJoinSide::NewDevice);
    assert_eq!(started.session.phase, DeviceJoinLocalPhase::Pending);
    assert_eq!(started.join_request.requested_role, "member");
    validate_join_request(&started.join_request, OffsetDateTime::now_utc()).unwrap();

    let state_store = JoinStateStore::new(&core);
    let state_path = state_store.path(&started.session.join_session_id, DeviceJoinSide::NewDevice);
    let state_raw = fs::read(&state_path).unwrap();
    let state_text = std::str::from_utf8(&state_raw).unwrap();
    assert!(!state_text.contains("PRIVATE KEY"));
    assert!(!state_text.contains("BEGIN"));
    let stored = state_store
        .load(&started.session.join_session_id, DeviceJoinSide::NewDevice)
        .unwrap()
        .unwrap();
    assert!(stored.signing_private_ref.is_some());
    assert!(stored.e2ee_private_ref.is_some());
    assert_eq!(
        stored.pairing_private_ref.kind,
        SecretKind::IdentityJoinPairingPrivate
    );

    let vault = crate::vault::FileSecretVault::new(
        crate::vault::DeviceVaultRootKey::from_bytes([47_u8; 32]),
        crate::vault::FileSecretVaultStore::new(root.path().join("vault")),
    );
    let secret_refs = crate::vault::SecretVault::list(&vault).unwrap();
    assert_eq!(secret_refs.len(), 3);
    assert!(secret_refs
        .iter()
        .any(|value| value.kind == SecretKind::IdentityDeviceSigningPrivate));
    assert!(secret_refs
        .iter()
        .any(|value| value.kind == SecretKind::IdentityE2eeAgreementPrivate));
    assert!(secret_refs
        .iter()
        .any(|value| value.kind == SecretKind::IdentityJoinPairingPrivate));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&state_path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    drop(core);
    let restarted = open_vault_core(root.path());
    assert_eq!(
        restarted
            .device_join()
            .session(&started.session.join_session_id, DeviceJoinSide::NewDevice,)
            .unwrap(),
        started.session
    );
    let retried = restarted.device_join().start(request.clone()).unwrap();
    assert_eq!(retried, started);

    let error = restarted
        .device_join()
        .start(DeviceJoinStartRequest {
            ttl_seconds: 301,
            ..request
        })
        .unwrap_err();
    assert!(matches!(
        error,
        crate::ImError::InvalidInput {
            field: Some(field),
            ..
        } if field == "operation_id"
    ));
}

#[test]
fn remote_session_token_is_sealed_and_cancel_is_restart_safe() {
    let root = tempfile::tempdir().unwrap();
    let core = open_vault_core(root.path());
    let started = core
        .device_join()
        .start(DeviceJoinStartRequest {
            operation_id: "start-remote-session".to_owned(),
            did: crate::ids::Did::parse("did:wba:awiki.test:alice").unwrap(),
            ttl_seconds: 300,
        })
        .unwrap();
    let token = SecretBytes::from_vec(b"join-token-must-stay-in-vault".to_vec());

    let bound = bind_new_device_remote_session(
        &core,
        &started.session.join_session_id,
        &token,
        &started.join_request.expires_at,
    )
    .unwrap();
    assert_eq!(bound, started.session);
    assert_eq!(
        open_new_device_remote_session_token(&core, &started.session.join_session_id)
            .unwrap()
            .expose_secret(),
        token.expose_secret()
    );

    let state_path = JoinStateStore::new(&core)
        .path(&started.session.join_session_id, DeviceJoinSide::NewDevice);
    let public_state = fs::read_to_string(&state_path).unwrap();
    assert!(!public_state.contains("join-token-must-stay-in-vault"));
    assert!(!public_state.contains("PRIVATE KEY"));

    let cancelled = cancel_join(
        &core,
        &started.session.join_session_id,
        DeviceJoinSide::NewDevice,
    )
    .unwrap();
    assert_eq!(cancelled.phase, DeviceJoinLocalPhase::Cancelled);
    assert_eq!(
        cancel_join(
            &core,
            &started.session.join_session_id,
            DeviceJoinSide::NewDevice,
        )
        .unwrap(),
        cancelled
    );

    let vault = crate::vault::FileSecretVault::new(
        crate::vault::DeviceVaultRootKey::from_bytes([47_u8; 32]),
        crate::vault::FileSecretVaultStore::new(root.path().join("vault")),
    );
    assert!(crate::vault::SecretVault::list(&vault).unwrap().is_empty());
    drop(core);

    let restarted = open_vault_core(root.path());
    assert_eq!(
        restarted
            .device_join()
            .session(&started.session.join_session_id, DeviceJoinSide::NewDevice)
            .unwrap(),
        cancelled
    );
    assert!(matches!(
        open_new_device_remote_session_token(&restarted, &started.session.join_session_id),
        Err(crate::ImError::LocalStateUnavailable { .. })
    ));
}

#[test]
fn expired_challenge_marks_session_expired_and_deletes_all_pending_secrets() {
    let root = tempfile::tempdir().unwrap();
    let core = open_vault_core(root.path());
    let started = core
        .device_join()
        .start(DeviceJoinStartRequest {
            operation_id: "start-expiring-challenge".to_owned(),
            did: crate::ids::Did::parse("did:wba:awiki.test:alice").unwrap(),
            ttl_seconds: 300,
        })
        .unwrap();
    let store = JoinStateStore::new(&core);
    let mut stored = store
        .load(&started.session.join_session_id, DeviceJoinSide::NewDevice)
        .unwrap()
        .unwrap();
    stored.phase = DeviceJoinLocalPhase::ResponsePrepared;
    stored.challenge = Some(DeviceJoinChallenge {
        operation_id: "admin-challenge-operation".to_owned(),
        join_session_id: started.session.join_session_id.clone(),
        challenge_id: "expired-challenge".to_owned(),
        admin_device_id: "admin-a".to_owned(),
        admin_pairing_public_key: URL_SAFE_NO_PAD.encode([9_u8; 32]),
        ciphertext: EncryptedJoinChallenge {
            algorithm: DEVICE_JOIN_CHALLENGE_ALGORITHM.to_owned(),
            nonce_b64u: URL_SAFE_NO_PAD.encode([8_u8; JOIN_NONCE_LEN]),
            ciphertext_b64u: URL_SAFE_NO_PAD.encode([7_u8; 48]),
        },
        challenge_expires_at: format_time(OffsetDateTime::now_utc() - Duration::seconds(1))
            .unwrap(),
        authorizing_device_proof: sample_proof(),
    });
    store.save(&stored).unwrap();

    let summary = core
        .device_join()
        .session(&started.session.join_session_id, DeviceJoinSide::NewDevice)
        .unwrap();
    assert_eq!(summary.phase, DeviceJoinLocalPhase::Expired);

    let vault = crate::vault::FileSecretVault::new(
        crate::vault::DeviceVaultRootKey::from_bytes([47_u8; 32]),
        crate::vault::FileSecretVaultStore::new(root.path().join("vault")),
    );
    assert!(crate::vault::SecretVault::list(&vault).unwrap().is_empty());

    let stored = store
        .load(&started.session.join_session_id, DeviceJoinSide::NewDevice)
        .unwrap()
        .unwrap();
    assert_eq!(stored.phase, DeviceJoinLocalPhase::Expired);
    assert!(stored.signing_private_ref.is_some());
    assert!(stored.e2ee_private_ref.is_some());
    assert_eq!(
        stored.pairing_private_ref.kind,
        SecretKind::IdentityJoinPairingPrivate
    );
}

#[test]
fn pending_join_refuses_to_generate_secrets_without_secret_vault() {
    let root = tempfile::tempdir().unwrap();
    let core = crate::ImCore::new(test_config(), test_paths(root.path())).unwrap();
    let error = core
        .device_join()
        .start(DeviceJoinStartRequest {
            operation_id: "start-without-vault".to_owned(),
            did: crate::ids::Did::parse("did:wba:awiki.test:alice").unwrap(),
            ttl_seconds: 300,
        })
        .unwrap_err();

    assert!(matches!(
        error,
        crate::ImError::IdentityVault {
            failure: crate::IdentityVaultFailure::Unavailable,
        }
    ));
}

#[test]
fn new_device_activation_survives_local_commit_failure_and_promotes_rootless_member() {
    use crate::internal::identity_device_state::{
        DeviceAuthorizationRole, DeviceAuthorizationStatus, IdentityInternalCheckpoint,
    };

    let root = tempfile::tempdir().unwrap();
    let generated = crate::internal::identity_generation::generate_vnext_handle_identity_with_default_daemon_subkey(
        "awiki.test", "alice", None, None,
    ).unwrap();
    let core = open_vault_core(root.path());
    let started = core
        .device_join()
        .start(DeviceJoinStartRequest {
            operation_id: "start-activation-member".to_owned(),
            did: generated.did.clone(),
            ttl_seconds: 300,
        })
        .unwrap();
    let mut stored = JoinStateStore::new(&core)
        .load(&started.session.join_session_id, DeviceJoinSide::NewDevice)
        .unwrap()
        .unwrap();
    stored.phase = DeviceJoinLocalPhase::ResponsePrepared;
    JoinStateStore::new(&core).save(&stored).unwrap();

    let device = anp::authentication::DeviceManifestEntry {
        device_id: started.join_request.device_id.clone(),
        signing_key_id: method_id(&started.join_request.signing_public_key, "signing")
            .unwrap()
            .to_owned(),
        e2ee_key_id: method_id(&started.join_request.e2ee_public_key, "e2ee")
            .unwrap()
            .to_owned(),
        profiles: started.join_request.profiles.clone(),
    };
    let mut final_document = anp::authentication::add_device_to_did_document(
        &generated.did_document,
        &generated.root_key_id,
        &device,
        &started.join_request.signing_public_key,
        &started.join_request.e2ee_public_key,
        &[],
    )
    .unwrap();
    crate::internal::identity_daemon_subkey::resign_did_document_with_key1(
        &mut final_document,
        &generated.did,
        &generated.root_private_pem,
    )
    .unwrap();
    let authorization =
        crate::internal::identity_device_join_runtime::DeviceJoinRemoteAuthorization {
            checkpoint: IdentityInternalCheckpoint {
                document_version: 2,
                document_hash: canonical_hash(&final_document).unwrap(),
                registry_version: 2,
            },
            device: crate::internal::identity_device_join_runtime::DeviceJoinRemoteDeviceSummary {
                device_id: device.device_id.clone(),
                signing_key_id: device.signing_key_id.clone(),
                e2ee_key_id: device.e2ee_key_id.clone(),
                status: DeviceAuthorizationStatus::Active,
                role: DeviceAuthorizationRole::Member,
                management_ready: false,
                auth_generation: 1,
            },
        };
    let mut mismatched_authorization = authorization.clone();
    mismatched_authorization.checkpoint.document_hash =
        "sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_owned();
    assert_eq!(
        prepare_new_device_activation(
            &core,
            &started.session.join_session_id,
            &mismatched_authorization,
            &final_document,
        ),
        Err(crate::ImError::PermissionDenied)
    );
    assert!(
        load_pending_new_device_activation(&core, &started.session.join_session_id)
            .unwrap()
            .is_none()
    );
    let pending = prepare_new_device_activation(
        &core,
        &started.session.join_session_id,
        &authorization,
        &final_document,
    )
    .unwrap();
    assert_eq!(pending.prepared_token_issue.device_id, device.device_id);
    let pending = record_new_device_token_result(
        &core,
        &started.session.join_session_id,
        crate::internal::identity_wire::device_genesis::DeviceTokenIssueResult {
            access_token: "member-access-token".to_owned(),
            refresh_token: "member-refresh-token".to_owned(),
            expires_at: format_time(OffsetDateTime::now_utc() + Duration::hours(1)).unwrap(),
            user_id: "user-member".to_owned(),
            device_id: device.device_id.clone(),
            auth_generation: 1,
            scopes: vec!["device:read".to_owned(), "message:connect".to_owned()],
        },
    )
    .unwrap();
    assert!(pending.token_result.is_some());

    let identity_dir = root
        .path()
        .join("identities")
        .join(crate::internal::identity_join_activation_pending::identity_suffix(&generated.did));
    std::fs::write(&identity_dir, b"force identity directory failure").unwrap();
    assert!(finalize_new_device_activation(&core, &started.session.join_session_id).is_err());
    assert!(
        load_pending_new_device_activation(&core, &started.session.join_session_id)
            .unwrap()
            .unwrap()
            .token_result
            .is_some()
    );
    std::fs::remove_file(&identity_dir).unwrap();
    drop(core);

    let restarted = open_vault_core(root.path());
    let summary =
        finalize_new_device_activation(&restarted, &started.session.join_session_id).unwrap();
    assert_eq!(summary.phase, DeviceJoinLocalPhase::Authorized);
    let device_summary = restarted
        .identities()
        .device_summary(crate::identity::IdentitySelector::Default)
        .unwrap();
    assert_eq!(
        device_summary.role,
        Some(crate::identity::IdentityDeviceRole::Member)
    );
    assert_eq!(
        device_summary.readiness,
        crate::identity::IdentityDeviceReadiness::MemberReady
    );
    let index = crate::internal::identity_store::IdentityStore::new(
        &restarted.inner().sdk_paths().identities,
    )
    .load_index()
    .unwrap();
    let entry = index.credentials.get("alice").unwrap();
    assert!(entry
        .vault_migration
        .as_ref()
        .unwrap()
        .vnext_refs
        .as_ref()
        .unwrap()
        .did_document_root_private
        .is_none());
    let auth_ref = &entry
        .vault_migration
        .as_ref()
        .unwrap()
        .vnext_refs
        .as_ref()
        .unwrap()
        .auth_jwt;
    let auth = restarted
        .inner()
        .identity_vault()
        .unwrap()
        .vault()
        .open(auth_ref)
        .unwrap();
    let auth = crate::internal::auth::state::parse_auth_state(auth.expose_secret()).unwrap();
    assert_eq!(auth.bearer_token.as_deref(), Some("member-access-token"));
    assert_eq!(auth.refresh_token.as_deref(), Some("member-refresh-token"));
    let vault_refs = restarted
        .inner()
        .identity_vault()
        .unwrap()
        .vault()
        .list()
        .unwrap();
    assert!(vault_refs.iter().all(|secret_ref| !matches!(
        secret_ref.kind,
        SecretKind::IdentityJoinActivationPending
            | SecretKind::IdentityJoinPairingPrivate
            | SecretKind::IdentityJoinSessionToken
    )));
    assert!(vault_refs
        .iter()
        .filter(|secret_ref| {
            matches!(
                secret_ref.kind,
                SecretKind::IdentityDeviceSigningPrivate | SecretKind::IdentityE2eeAgreementPrivate
            )
        })
        .all(|secret_ref| secret_ref.identity_id.is_some()));
}

#[tokio::test]
async fn admin_join_activation_remains_not_ready_without_root_import() {
    use crate::internal::identity_device_state::{
        DeviceAuthorizationRole, DeviceAuthorizationStatus, IdentityInternalCheckpoint,
    };

    let root = tempfile::tempdir().unwrap();
    let generated = crate::internal::identity_generation::generate_vnext_handle_identity_with_default_daemon_subkey(
        "awiki.test", "alice", None, None,
    ).unwrap();
    let core = open_vault_core_with_messaging_flags(root.path(), false, false, true);
    let started = core
        .device_join()
        .start(DeviceJoinStartRequest {
            operation_id: "start-activation-admin".to_owned(),
            did: generated.did.clone(),
            ttl_seconds: 300,
        })
        .unwrap();
    let store = JoinStateStore::new(&core);
    let mut stored = store
        .load(&started.session.join_session_id, DeviceJoinSide::NewDevice)
        .unwrap()
        .unwrap();
    stored.phase = DeviceJoinLocalPhase::ResponsePrepared;
    store.save(&stored).unwrap();
    let device = anp::authentication::DeviceManifestEntry {
        device_id: started.join_request.device_id.clone(),
        signing_key_id: method_id(&started.join_request.signing_public_key, "signing")
            .unwrap()
            .to_owned(),
        e2ee_key_id: method_id(&started.join_request.e2ee_public_key, "e2ee")
            .unwrap()
            .to_owned(),
        profiles: started.join_request.profiles.clone(),
    };
    let mut document = anp::authentication::add_device_to_did_document(
        &generated.did_document,
        &generated.root_key_id,
        &device,
        &started.join_request.signing_public_key,
        &started.join_request.e2ee_public_key,
        &[],
    )
    .unwrap();
    crate::internal::identity_daemon_subkey::resign_did_document_with_key1(
        &mut document,
        &generated.did,
        &generated.root_private_pem,
    )
    .unwrap();
    let authorization =
        crate::internal::identity_device_join_runtime::DeviceJoinRemoteAuthorization {
            checkpoint: IdentityInternalCheckpoint {
                document_version: 2,
                document_hash: canonical_hash(&document).unwrap(),
                registry_version: 2,
            },
            device: crate::internal::identity_device_join_runtime::DeviceJoinRemoteDeviceSummary {
                device_id: device.device_id.clone(),
                signing_key_id: device.signing_key_id,
                e2ee_key_id: device.e2ee_key_id,
                status: DeviceAuthorizationStatus::Active,
                role: DeviceAuthorizationRole::Admin,
                management_ready: false,
                auth_generation: 1,
            },
        };
    prepare_new_device_activation(
        &core,
        &started.session.join_session_id,
        &authorization,
        &document,
    )
    .unwrap();
    record_new_device_token_result(
        &core,
        &started.session.join_session_id,
        crate::internal::identity_wire::device_genesis::DeviceTokenIssueResult {
            access_token: "admin-awaiting-access".to_owned(),
            refresh_token: "admin-awaiting-refresh".to_owned(),
            expires_at: format_time(OffsetDateTime::now_utc() + Duration::hours(1)).unwrap(),
            user_id: "user-admin".to_owned(),
            device_id: device.device_id,
            auth_generation: 1,
            scopes: vec!["device:read".to_owned(), "message:connect".to_owned()],
        },
    )
    .unwrap();
    let session = finalize_new_device_activation(&core, &started.session.join_session_id).unwrap();
    let summary = core
        .identities()
        .device_summary(crate::identity::IdentitySelector::Default)
        .unwrap();
    assert_eq!(
        summary.role,
        Some(crate::identity::IdentityDeviceRole::Admin)
    );
    assert_eq!(
        summary.readiness,
        crate::identity::IdentityDeviceReadiness::AdminAwaitingRoot
    );

    let mut publisher = RecordingJoinGroupKeyPackagePublisher {
        fail_next: true,
        calls: Vec::new(),
    };
    assert!(matches!(
        crate::internal::identity_device_join_runtime::publish_v2_group_key_package_after_activation_with_publisher(
            &core,
            &session,
            &mut publisher,
        )
        .await,
        Err(crate::ImError::TransportUnavailable { .. })
    ));
    assert_eq!(publisher.calls.len(), 1);
    assert_eq!(
        core.identities()
            .device_summary(crate::identity::IdentitySelector::Default)
            .unwrap()
            .readiness,
        crate::identity::IdentityDeviceReadiness::AdminAwaitingRoot,
        "P6 publication failure must not roll authorized admin readiness back"
    );
}

#[tokio::test]
async fn token_response_loss_retries_same_operation_with_fresh_device_authorization() {
    use crate::internal::identity_device_join_runtime::{
        DeviceJoinDidResolver, DeviceJoinNewDeviceRuntime, DeviceJoinRemoteAuthorization,
        DeviceJoinRemoteDeviceSummary, DeviceJoinRuntimeGate,
    };
    use crate::internal::identity_device_state::{
        DeviceAuthorizationRole, DeviceAuthorizationStatus, IdentityInternalCheckpoint,
    };

    let root = tempfile::tempdir().unwrap();
    let generated = crate::internal::identity_generation::generate_vnext_handle_identity_with_default_daemon_subkey(
        "awiki.test", "alice", None, None,
    ).unwrap();
    let core = open_vault_core(root.path());
    let started = core
        .device_join()
        .start(DeviceJoinStartRequest {
            operation_id: "start-token-response-loss".to_owned(),
            did: generated.did.clone(),
            ttl_seconds: 300,
        })
        .unwrap();
    let store = JoinStateStore::new(&core);
    let mut stored = store
        .load(&started.session.join_session_id, DeviceJoinSide::NewDevice)
        .unwrap()
        .unwrap();
    stored.phase = DeviceJoinLocalPhase::ResponsePrepared;
    store.save(&stored).unwrap();

    let device = anp::authentication::DeviceManifestEntry {
        device_id: started.join_request.device_id.clone(),
        signing_key_id: method_id(&started.join_request.signing_public_key, "signing")
            .unwrap()
            .to_owned(),
        e2ee_key_id: method_id(&started.join_request.e2ee_public_key, "e2ee")
            .unwrap()
            .to_owned(),
        profiles: started.join_request.profiles.clone(),
    };
    let mut document = anp::authentication::add_device_to_did_document(
        &generated.did_document,
        &generated.root_key_id,
        &device,
        &started.join_request.signing_public_key,
        &started.join_request.e2ee_public_key,
        &[],
    )
    .unwrap();
    crate::internal::identity_daemon_subkey::resign_did_document_with_key1(
        &mut document,
        &generated.did,
        &generated.root_private_pem,
    )
    .unwrap();
    let authorization = DeviceJoinRemoteAuthorization {
        checkpoint: IdentityInternalCheckpoint {
            document_version: 2,
            document_hash: canonical_hash(&document).unwrap(),
            registry_version: 2,
        },
        device: DeviceJoinRemoteDeviceSummary {
            device_id: device.device_id.clone(),
            signing_key_id: device.signing_key_id,
            e2ee_key_id: device.e2ee_key_id,
            status: DeviceAuthorizationStatus::Active,
            role: DeviceAuthorizationRole::Member,
            management_ready: false,
            auth_generation: 1,
        },
    };
    let pending = prepare_new_device_activation(
        &core,
        &started.session.join_session_id,
        &authorization,
        &document,
    )
    .unwrap();
    let operation_id = pending.prepared_token_issue.operation_id.clone();
    let initial_authorization = pending.prepared_token_issue.authorization.clone();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let results = Arc::new(Mutex::new(VecDeque::from([Err(
        crate::ImError::TransportUnavailable {
            detail: "token response was lost".to_owned(),
        },
    )])));
    let remote = ActivationTokenRemote {
        calls: calls.clone(),
        results: results.clone(),
    };
    let mut runtime = DeviceJoinNewDeviceRuntime::new(
        &core,
        remote,
        DeviceJoinDidResolver::new(PanicDidResolverTransport),
        DeviceJoinRuntimeGate::from_rollout_flag(true),
    );
    let error = runtime
        .advance(&started.session.join_session_id)
        .await
        .unwrap_err();
    assert!(matches!(error, crate::ImError::TransportUnavailable { .. }));
    assert!(
        load_pending_new_device_activation(&core, &started.session.join_session_id)
            .unwrap()
            .is_some()
    );
    drop(runtime);
    drop(core);

    results.lock().unwrap().extend([
        Err(crate::ImError::AuthRequired),
        Ok(
            crate::internal::identity_wire::device_genesis::DeviceTokenIssueResult {
                access_token: "response-loss-access".to_owned(),
                refresh_token: "response-loss-refresh".to_owned(),
                expires_at: format_time(OffsetDateTime::now_utc() + Duration::hours(1)).unwrap(),
                user_id: "user-response-loss".to_owned(),
                device_id: device.device_id,
                auth_generation: 1,
                scopes: vec!["device:read".to_owned(), "message:connect".to_owned()],
            },
        ),
    ]);
    let restarted = open_vault_core(root.path());
    let remote = ActivationTokenRemote {
        calls: calls.clone(),
        results,
    };
    let mut runtime = DeviceJoinNewDeviceRuntime::new(
        &restarted,
        remote,
        DeviceJoinDidResolver::new(PanicDidResolverTransport),
        DeviceJoinRuntimeGate::from_rollout_flag(true),
    );
    let completed = runtime
        .advance(&started.session.join_session_id)
        .await
        .unwrap();
    assert_eq!(completed.session.phase, DeviceJoinLocalPhase::Authorized);

    let calls = calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 3);
    assert!(calls.iter().all(|call| call.0 == operation_id));
    assert!(calls.iter().all(|call| call.2 == 1));
    assert_eq!(calls[0].1, initial_authorization);
    assert_eq!(calls[1].1, initial_authorization);
    assert_ne!(calls[2].1, initial_authorization);
    assert!(
        load_pending_new_device_activation(&restarted, &started.session.join_session_id,)
            .unwrap()
            .is_none()
    );
    let summary = restarted
        .identities()
        .device_summary(crate::identity::IdentitySelector::Default)
        .unwrap();
    assert_eq!(
        summary.role,
        Some(crate::identity::IdentityDeviceRole::Member)
    );
    assert_eq!(
        summary.readiness,
        crate::identity::IdentityDeviceReadiness::MemberReady
    );
}
