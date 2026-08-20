use super::*;

use crate::internal::identity_device_state::{
    DeviceAuthorizationProjection, DeviceAuthorizationRole, DeviceAuthorizationStatus,
    IdentityDeviceMode, IdentityDeviceState, IdentityInternalCheckpoint,
    IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
};
use crate::vault::{DeviceVaultRootKey, FileSecretVault, FileSecretVaultStore};
use anp::direct_e2ee::{
    build_prekey_bundle_v2, V2OneTimePrekey, V2SignedPrekey, CONTENT_TYPE_DIRECT_INIT_V2,
};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const NOW: &str = "2026-07-20T00:00:00Z";
const ACCEPTED_AT: &str = "2026-07-20T00:00:01Z";

#[test]
fn delivery_failure_code_preserves_safe_local_state_category() {
    assert_eq!(
        failure_code(&crate::ImError::LocalStateUnavailable {
            detail: "database is locked".to_owned(),
        }),
        "local_state_database_locked"
    );
    assert_eq!(
        failure_code(&crate::ImError::LocalStateUnavailable {
            detail: "UNIQUE constraint failed: sessions.id".to_owned(),
        }),
        "local_state_constraint"
    );
    assert_eq!(
        failure_code(&crate::ImError::IdentityVault {
            failure: crate::IdentityVaultFailure::Unavailable,
        }),
        "identity_vault"
    );
}

#[derive(Clone, Copy)]
struct DeviceSpec {
    id: &'static str,
    signing_seed: u8,
    static_seed: u8,
    signed_prekey_seed: u8,
    one_time_prekey_seed: u8,
}

impl DeviceSpec {
    fn signing(&self) -> anp::PrivateKeyMaterial {
        anp::PrivateKeyMaterial::Ed25519(ed25519_dalek::SigningKey::from_bytes(
            &[self.signing_seed; 32],
        ))
    }

    fn static_private(&self) -> x25519_dalek::StaticSecret {
        x25519_dalek::StaticSecret::from([self.static_seed; 32])
    }

    fn signed_prekey_private(&self) -> x25519_dalek::StaticSecret {
        x25519_dalek::StaticSecret::from([self.signed_prekey_seed; 32])
    }

    fn one_time_prekey_private(&self) -> x25519_dalek::StaticSecret {
        x25519_dalek::StaticSecret::from([self.one_time_prekey_seed; 32])
    }

    fn signing_key_id(&self, did: &str) -> String {
        format!("{did}#{}-sign", self.id)
    }

    fn e2ee_key_id(&self, did: &str) -> String {
        format!("{did}#{}-e2ee", self.id)
    }

    fn fetched(&self, did: &str) -> V2GetPrekeyBundleResult {
        let signing = self.signing();
        let signed_prekey_private = self.signed_prekey_private();
        let one_time_prekey_private = self.one_time_prekey_private();
        let signed_prekey = V2SignedPrekey {
            key_id: format!("{}-spk", self.id),
            public_key_b64u: URL_SAFE_NO_PAD
                .encode(x25519_dalek::PublicKey::from(&signed_prekey_private).to_bytes()),
            expires_at: "2035-01-01T00:00:00Z".to_owned(),
        };
        let bundle = build_prekey_bundle_v2(
            &format!("{}-bundle", self.id),
            did,
            self.id,
            &self.e2ee_key_id(did),
            signed_prekey,
            &signing,
            &self.signing_key_id(did),
            Some(NOW),
        )
        .unwrap();
        V2GetPrekeyBundleResult {
            target_did: did.to_owned(),
            target_device_id: self.id.to_owned(),
            prekey_bundle: bundle,
            one_time_prekey: Some(V2OneTimePrekey {
                key_id: format!("{}-opk", self.id),
                public_key_b64u: URL_SAFE_NO_PAD
                    .encode(x25519_dalek::PublicKey::from(&one_time_prekey_private).to_bytes()),
            }),
        }
    }
}

fn profiles() -> Vec<String> {
    vec![
        anp::authentication::PROFILE_CORE_BINDING_V2.to_owned(),
        anp::authentication::PROFILE_IDENTITY_DISCOVERY_V2.to_owned(),
        anp::authentication::PROFILE_DIRECT_BASE_V2.to_owned(),
        anp::authentication::PROFILE_DIRECT_E2EE_V2.to_owned(),
    ]
}

fn public_multibase(codec: [u8; 2], bytes: &[u8]) -> String {
    let mut encoded = codec.to_vec();
    encoded.extend_from_slice(bytes);
    format!("z{}", bs58::encode(encoded).into_string())
}

fn did_document(did: &str, devices: &[DeviceSpec]) -> Value {
    let mut methods = Vec::new();
    let mut authentication = Vec::new();
    let mut assertion = Vec::new();
    let mut agreement = Vec::new();
    let mut manifest = Vec::new();
    for device in devices {
        let signing_key_id = device.signing_key_id(did);
        let e2ee_key_id = device.e2ee_key_id(did);
        let signing_public = match device.signing().public_key() {
            anp::PublicKeyMaterial::Ed25519(key) => key.to_bytes(),
            _ => unreachable!(),
        };
        let e2ee_public = x25519_dalek::PublicKey::from(&device.static_private()).to_bytes();
        methods.push(json!({
            "id": signing_key_id,
            "type": "Multikey",
            "controller": did,
            "publicKeyMultibase": public_multibase([0xed, 0x01], &signing_public),
        }));
        methods.push(json!({
            "id": e2ee_key_id,
            "type": "X25519KeyAgreementKey2019",
            "controller": did,
            "publicKeyMultibase": public_multibase([0xec, 0x01], &e2ee_public),
        }));
        authentication.push(Value::String(signing_key_id.clone()));
        assertion.push(Value::String(signing_key_id.clone()));
        agreement.push(Value::String(e2ee_key_id.clone()));
        manifest.push(json!({
            "device_id": device.id,
            "signing_key_id": signing_key_id,
            "e2ee_key_id": e2ee_key_id,
            "profiles": profiles(),
        }));
    }
    json!({
        "id": did,
        "verificationMethod": methods,
        "authentication": authentication,
        "assertionMethod": assertion,
        "keyAgreement": agreement,
        "deviceManifest": {
            "type": "ANPDeviceManifest",
            "devices": manifest,
        },
    })
}

#[test]
fn fetched_prekey_accepts_join_style_okp_jwk_signing_method() {
    let did = "did:example:joined";
    let device = DeviceSpec {
        id: "joined-device",
        signing_seed: 121,
        static_seed: 122,
        signed_prekey_seed: 123,
        one_time_prekey_seed: 124,
    };
    let mut document = did_document(did, &[device]);
    let signing_key_id = device.signing_key_id(did);
    let signing_public = match device.signing().public_key() {
        anp::PublicKeyMaterial::Ed25519(key) => key.to_bytes(),
        _ => unreachable!(),
    };
    let method = document["verificationMethod"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|method| method["id"] == signing_key_id)
        .unwrap();
    *method = json!({
        "id": signing_key_id,
        "type": "JsonWebKey2020",
        "controller": did,
        "publicKeyJwk": {
            "kty": "OKP",
            "crv": "Ed25519",
            "x": URL_SAFE_NO_PAD.encode(signing_public),
        },
    });
    let target = anp::authentication::validate_device_manifest(&document)
        .unwrap()
        .unwrap()
        .devices
        .into_iter()
        .next()
        .unwrap();

    verify_fetched_prekey(&device.fetched(did), &target, did, &document).unwrap();
}

fn scope(identity_id: &str, did: &str, device: DeviceSpec) -> V2OwnerScope {
    let did_value = crate::ids::Did::parse(did).unwrap();
    V2OwnerScope::from_identity_state(
        &crate::ids::IdentityId::parse(identity_id).unwrap(),
        &did_value,
        &IdentityDeviceState {
            schema_version: IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
            mode: IdentityDeviceMode::VNext,
            authorization: Some(DeviceAuthorizationProjection {
                protocol_device_id: crate::ids::ProtocolDeviceId::parse(device.id).unwrap(),
                signing_key_id: device.signing_key_id(did),
                e2ee_key_id: device.e2ee_key_id(did),
                status: DeviceAuthorizationStatus::Active,
                role: DeviceAuthorizationRole::Member,
                management_ready: false,
                auth_generation: 1,
            }),
            checkpoint: Some(IdentityInternalCheckpoint {
                document_version: 1,
                document_hash: "sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_owned(),
                registry_version: 1,
            }),
        },
    )
    .unwrap()
}

fn context(
    root: &std::path::Path,
    identity_id: &str,
    did: &str,
    device: DeviceSpec,
    vault_seed: u8,
) -> V2DirectProductContext {
    let vault: Arc<dyn crate::vault::SecretVault + Send + Sync> = Arc::new(FileSecretVault::new(
        DeviceVaultRootKey::from_bytes([vault_seed; 32]),
        FileSecretVaultStore::new(root.join(format!("{}-vault", device.id))),
    ));
    V2DirectProductContext {
        owner_identity_id: identity_id.to_owned(),
        local_did: did.to_owned(),
        local_device_id: device.id.to_owned(),
        local_e2ee_key_id: device.e2ee_key_id(did),
        local_static_private: device.static_private(),
        sqlite_path: root.join(format!("{}.sqlite", device.id)),
        vault,
        scope: scope(identity_id, did, device),
    }
}

fn install_bundle(context: &V2DirectProductContext, did: &str, device: DeviceSpec) {
    let fetched = device.fetched(did);
    let connection = context.open_connection().unwrap();
    let store = SqliteV2DirectStateStore::new_with_secret_vault(
        &connection,
        context.vault.clone(),
        context.scope.clone(),
    )
    .unwrap();
    store
        .publish_local_bundle(
            &fetched.prekey_bundle,
            &device.signed_prekey_private(),
            &[(
                fetched.one_time_prekey.clone().unwrap(),
                device.one_time_prekey_private(),
            )],
            NOW,
        )
        .unwrap();
}

#[derive(Default)]
struct FakeHost {
    documents: BTreeMap<String, Value>,
    fetched: BTreeMap<(String, String), V2GetPrekeyBundleResult>,
    fetch_operations: Vec<(String, String, String)>,
    post_attempts: Vec<PreparedV2Outbound>,
    post_attachment_grant_refs: Vec<Value>,
    fail_fetch_once: BTreeSet<(String, String)>,
    fail_post_once: BTreeSet<(String, String)>,
    stale_post_once: BTreeSet<(String, String)>,
    ensure_count: usize,
}

static RUNTIME_WIRE_DOCUMENTS: OnceLock<Mutex<BTreeMap<PathBuf, BTreeMap<String, Value>>>> =
    OnceLock::new();

pub(crate) struct RuntimeP5TestClientFixture {
    root: tempfile::TempDir,
}

impl RuntimeP5TestClientFixture {
    const VAULT_SEED: [u8; 32] = [151_u8; 32];
    const WORKSPACE_ID: &'static str = "runtime-p5-test-workspace";
    const VAULT_DEVICE_ID: &'static str = "runtime-p5-test-vault-device";

    pub(crate) fn new(label: &str) -> Self {
        use crate::internal::identity_store::{
            IdentityStore, SaveIdentityInput, SaveIdentityKeyMode, SaveIdentitySecretStorage,
        };

        let root = tempfile::tempdir().unwrap();
        let paths = Self::paths(root.path());
        std::fs::create_dir_all(&paths.identities.identity_root_dir).unwrap();
        let generated =
            crate::internal::identity_generation::generate_vnext_handle_identity_with_default_daemon_subkey(
                "awiki.test",
                label,
                None,
                None,
            )
            .unwrap();
        let device_state = IdentityDeviceState {
            schema_version: IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
            mode: IdentityDeviceMode::VNext,
            authorization: Some(DeviceAuthorizationProjection {
                protocol_device_id: generated.protocol_device_id.clone(),
                signing_key_id: generated.device_signing_key_id.clone(),
                e2ee_key_id: generated.device_e2ee_key_id.clone(),
                status: DeviceAuthorizationStatus::Active,
                role: DeviceAuthorizationRole::Member,
                management_ready: false,
                auth_generation: 1,
            }),
            checkpoint: Some(IdentityInternalCheckpoint {
                document_version: 1,
                document_hash: "sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_owned(),
                registry_version: 1,
            }),
        };
        let vault = Arc::new(FileSecretVault::new(
            DeviceVaultRootKey::from_bytes(Self::VAULT_SEED),
            FileSecretVaultStore::new(root.path().join("vault")),
        ));
        IdentityStore::new(&paths.identities)
            .save_identity_with_secret_storage(
                SaveIdentityInput {
                    local_alias: "alice".to_owned(),
                    did: generated.did.clone(),
                    unique_id: generated.unique_id.clone(),
                    user_id: format!("{label}-user"),
                    display_name: "Runtime P5 test".to_owned(),
                    handle: label.to_owned(),
                    full_handle: format!("{label}.awiki.test"),
                    binding_generation: Some("1".to_owned()),
                    jwt_token: "runtime-p5-device-token".to_owned(),
                    did_document: Some(generated.did_document.clone()),
                    key_mode: SaveIdentityKeyMode::VNext {
                        root_key_id: generated.root_key_id.clone(),
                        device_signing_key_id: generated.device_signing_key_id.clone(),
                        device_e2ee_key_id: generated.device_e2ee_key_id.clone(),
                    },
                    device_state: Some(device_state),
                    key1_private_pem: generated.root_private_pem,
                    key1_public_pem: generated.root_public_pem,
                    e2ee_signing_private_pem: generated.device_signing_private_pem,
                    e2ee_agreement_private_pem: generated.device_e2ee_private_pem,
                    daemon_subkey_package: Some(generated.daemon_subkey_package),
                    make_default: true,
                },
                SaveIdentitySecretStorage::Vault {
                    workspace_id: Self::WORKSPACE_ID.to_owned(),
                    device_id: Self::VAULT_DEVICE_ID.to_owned(),
                    vault,
                },
            )
            .unwrap();
        Self { root }
    }

    fn paths(root: &Path) -> crate::ImCorePaths {
        crate::ImCorePaths {
            identities: crate::paths::IdentityRegistryPaths {
                identity_root_dir: root.join("identities"),
                registry_path: root.join("identities").join("registry.json"),
                default_identity_path: Some(root.join("identities").join("default")),
            },
            local_state: crate::paths::LocalStatePaths {
                sqlite_path: root.join("local").join("im.sqlite"),
            },
            runtime: crate::paths::RuntimePaths {
                cache_dir: root.join("cache"),
                temp_dir: root.join("tmp"),
            },
        }
    }

    pub(crate) fn client(&self) -> crate::core::ImClient {
        self.client_with_direct_e2ee_enabled(true)
    }

    pub(crate) fn client_with_direct_e2ee_enabled(&self, enabled: bool) -> crate::core::ImClient {
        crate::core::ImCore::new_with_options(
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
            Self::paths(self.root.path()),
            crate::ImCoreOpenOptions::default()
                .with_identity_secret_vault(
                    crate::IdentitySecretStoragePolicy::VaultRequired,
                    crate::ImCoreSecretVaultOptions::new(
                        DeviceVaultRootKey::from_bytes(Self::VAULT_SEED),
                        self.root.path().join("vault"),
                        Self::WORKSPACE_ID,
                        Self::VAULT_DEVICE_ID,
                    ),
                )
                .with_multi_device_direct_e2ee_enabled(enabled),
        )
        .unwrap()
        .client(crate::identity::IdentitySelector::LocalAlias(
            "alice".to_owned(),
        ))
        .unwrap()
    }

    pub(crate) fn sqlite_path(&self) -> PathBuf {
        Self::paths(self.root.path()).local_state.sqlite_path
    }
}

pub(crate) enum RuntimeP5TestBody {
    Text(&'static str),
    Attachment(Value),
}

pub(crate) struct RuntimeP5TestWire {
    pub(crate) peer_did: &'static str,
    pub(crate) peer_device_id: &'static str,
    pub(crate) seed: u8,
    pub(crate) logical_message_id: &'static str,
    pub(crate) server_seq: u64,
    pub(crate) body: RuntimeP5TestBody,
}

pub(crate) async fn prepare_runtime_p5_test_wires(
    client: &crate::core::ImClient,
    requests: Vec<RuntimeP5TestWire>,
) -> Vec<Value> {
    let core = client.core_handle();
    let endpoint = active_local_endpoint_for_client(&core, client).unwrap();
    let recipient_context = V2DirectProductContext::from_client(&core, client).unwrap();
    let local_document = client.runtime().key_provider.did_document().unwrap();
    let signing_private = anp::PrivateKeyMaterial::from_pem(
        &client
            .runtime()
            .key_provider
            .device_request_signing_private_pem()
            .unwrap(),
    )
    .unwrap();
    let publication = {
        let connection = recipient_context.open_connection().unwrap();
        let store = SqliteV2DirectStateStore::new_with_secret_vault(
            &connection,
            recipient_context.vault.clone(),
            recipient_context.scope.clone(),
        )
        .unwrap();
        crate::internal::secure_direct::v2_prekey_runtime::ensure_local_prekey_publication(
            &store,
            crate::internal::secure_direct::v2_prekey_runtime::V2LocalPrekeyIdentity {
                did: &recipient_context.local_did,
                device_id: &recipient_context.local_device_id,
                signing_key_id: &endpoint.signing_key_id,
                e2ee_key_id: &recipient_context.local_e2ee_key_id,
                signing_private: &signing_private,
            },
            chrono::Utc::now(),
        )
        .unwrap()
    };
    assert!(publication.one_time_prekeys.len() >= requests.len());

    let mut peer_devices = BTreeMap::<&'static str, Vec<DeviceSpec>>::new();
    for request in &requests {
        peer_devices
            .entry(request.peer_did)
            .or_default()
            .push(DeviceSpec {
                id: request.peer_device_id,
                signing_seed: request.seed,
                static_seed: request.seed.wrapping_add(1),
                signed_prekey_seed: request.seed.wrapping_add(2),
                one_time_prekey_seed: request.seed.wrapping_add(3),
            });
    }
    let peer_documents = peer_devices
        .iter()
        .map(|(did, devices)| ((*did).to_owned(), did_document(did, devices)))
        .collect::<BTreeMap<_, _>>();
    let mut receiver_documents = peer_documents.clone();
    receiver_documents.insert(recipient_context.local_did.clone(), local_document.clone());
    RUNTIME_WIRE_DOCUMENTS
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .unwrap()
        .insert(recipient_context.sqlite_path.clone(), receiver_documents);

    let sender_root = tempfile::tempdir().unwrap();
    let mut wires = Vec::with_capacity(requests.len());
    for (index, request) in requests.into_iter().enumerate() {
        let device = DeviceSpec {
            id: request.peer_device_id,
            signing_seed: request.seed,
            static_seed: request.seed.wrapping_add(1),
            signed_prekey_seed: request.seed.wrapping_add(2),
            one_time_prekey_seed: request.seed.wrapping_add(3),
        };
        let sender = context(
            sender_root.path(),
            &format!("runtime-wire-sender-{index}"),
            request.peer_did,
            device,
            request.seed.wrapping_add(4),
        );
        let mut host = FakeHost::default();
        host.add_document(&recipient_context.local_did, local_document.clone());
        for (did, document) in &peer_documents {
            host.add_document(did, document.clone());
        }
        host.fetched.insert(
            (
                recipient_context.local_did.clone(),
                recipient_context.local_device_id.clone(),
            ),
            V2GetPrekeyBundleResult {
                target_did: recipient_context.local_did.clone(),
                target_device_id: recipient_context.local_device_id.clone(),
                prekey_bundle: publication.bundle.clone(),
                one_time_prekey: Some(publication.one_time_prekeys[index].clone()),
            },
        );
        let body = match request.body {
            RuntimeP5TestBody::Text(text) => V2OrdinaryBody::Text {
                text: text.to_owned(),
                markdown: false,
            },
            RuntimeP5TestBody::Attachment(full_manifest) => {
                V2OrdinaryBody::AttachmentManifest { full_manifest }
            }
        };
        send_with_host(
            &sender,
            &mut host,
            V2DirectProductSendInput {
                logical_message_id: request.logical_message_id.to_owned(),
                target_did: recipient_context.local_did.clone(),
                conversation_id: Some(format!("conversation-{}", request.logical_message_id)),
                body,
            },
        )
        .await
        .unwrap();
        assert_eq!(host.post_attempts.len(), 1);
        let prepared = host.post_attempts.pop().unwrap();
        let direct_request = prepared.direct_request().unwrap();
        wires.push(json!({
            "id": prepared.metadata.message_id,
            "sender_did": prepared.metadata.sender_did,
            "receiver_did": prepared.metadata.target.did,
            "content_type": prepared.metadata.content_type,
            "server_seq": request.server_seq,
            "meta": direct_request["params"]["meta"].clone(),
            "body": direct_request["params"]["body"].clone(),
        }));
    }
    wires
}

pub(crate) async fn receive_registered_runtime_wire(
    context: &V2DirectProductContext,
    metadata: V2DirectMetadata,
    body: V2DirectBody,
    expected_peer_did: Option<&str>,
) -> Option<crate::ImResult<V2InboundProductOutcome>> {
    let documents = RUNTIME_WIRE_DOCUMENTS
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .unwrap()
        .get(&context.sqlite_path)
        .cloned()?;
    let mut host = FakeHost {
        documents,
        ..FakeHost::default()
    };
    Some(receive_with_host_scoped(context, &mut host, metadata, body, expected_peer_did).await)
}

impl FakeHost {
    fn add_document(&mut self, did: &str, document: Value) {
        self.documents.insert(did.to_owned(), document);
    }

    fn add_bundle(&mut self, did: &str, device: DeviceSpec) {
        self.fetched
            .insert((did.to_owned(), device.id.to_owned()), device.fetched(did));
    }
}

impl V2DirectProductHost for FakeHost {
    async fn resolve_did_document(&mut self, did: &str) -> crate::ImResult<Value> {
        self.documents
            .get(did)
            .cloned()
            .ok_or(crate::ImError::IdentityUnresolved {
                detail: did.to_owned(),
            })
    }

    async fn ensure_local_prekey_published(&mut self) -> crate::ImResult<()> {
        self.ensure_count += 1;
        Ok(())
    }

    async fn fetch_prekey(
        &mut self,
        target_did: &str,
        target_device_id: &str,
        target_did_document: &Value,
        operation_seed: &str,
    ) -> crate::ImResult<V2GetPrekeyBundleResult> {
        assert_eq!(
            self.documents.get(target_did),
            Some(target_did_document),
            "fetch must use the resolved document bound to the target"
        );
        self.fetch_operations.push((
            target_did.to_owned(),
            target_device_id.to_owned(),
            operation_seed.to_owned(),
        ));
        let key = (target_did.to_owned(), target_device_id.to_owned());
        if self.fail_fetch_once.remove(&key) {
            return Err(crate::ImError::TransportUnavailable {
                detail: "transient prekey failure".to_owned(),
            });
        }
        self.fetched
            .get(&key)
            .cloned()
            .ok_or(crate::ImError::PermissionDenied)
    }

    async fn post_direct(
        &mut self,
        prepared: &PreparedV2Outbound,
    ) -> crate::ImResult<V2DirectSendResult> {
        self.post_attempts.push(prepared.clone());
        let key = (
            prepared.metadata.target.did.clone(),
            prepared.metadata.recipient_device_id.clone(),
        );
        if self.fail_post_once.remove(&key) {
            return Err(crate::ImError::TransportUnavailable {
                detail: "transient direct failure".to_owned(),
            });
        }
        if self.stale_post_once.remove(&key) {
            return Err(crate::ImError::Service {
                status_code: None,
                code: Some("anp.invalid_target_binding".to_owned()),
                message: "DID is no longer the active handle binding".to_owned(),
                data: Some(json!({
                    "reason": "stale_did",
                    "json_rpc_code": 1406,
                    "current_did": "did:example:bob-current",
                    "full_handle": "bob.awiki.test"
                })),
            });
        }
        Ok(V2DirectSendResult {
            accepted: true,
            message_id: prepared.metadata.message_id.clone(),
            operation_id: prepared.metadata.operation_id.clone(),
            target_did: prepared.metadata.target.did.clone(),
            recipient_device_id: prepared.metadata.recipient_device_id.clone(),
            accepted_at: ACCEPTED_AT.to_owned(),
        })
    }

    async fn post_direct_attachment(
        &mut self,
        prepared: &PreparedV2Outbound,
        attachment_grant_ref: &Value,
    ) -> crate::ImResult<V2DirectSendResult> {
        self.post_attachment_grant_refs
            .push(attachment_grant_ref.clone());
        self.post_direct(prepared).await
    }
}

fn text_input(id: &str, target_did: &str, text: &str) -> V2DirectProductSendInput {
    V2DirectProductSendInput {
        logical_message_id: id.to_owned(),
        target_did: target_did.to_owned(),
        conversation_id: Some(format!("conversation-{id}")),
        body: V2OrdinaryBody::Text {
            text: text.to_owned(),
            markdown: false,
        },
    }
}

#[tokio::test]
async fn one_send_fans_out_exact_init_to_every_peer_and_sibling_device() {
    let root = tempfile::tempdir().unwrap();
    let alice_did = "did:example:alice";
    let bob_did = "did:example:bob";
    let a1 = DeviceSpec {
        id: "alice-a1",
        signing_seed: 1,
        static_seed: 2,
        signed_prekey_seed: 3,
        one_time_prekey_seed: 4,
    };
    let a2 = DeviceSpec {
        id: "alice-a2",
        signing_seed: 5,
        static_seed: 6,
        signed_prekey_seed: 7,
        one_time_prekey_seed: 8,
    };
    let a3 = DeviceSpec {
        id: "alice-a3",
        signing_seed: 9,
        static_seed: 10,
        signed_prekey_seed: 11,
        one_time_prekey_seed: 12,
    };
    let b1 = DeviceSpec {
        id: "bob-b1",
        signing_seed: 13,
        static_seed: 14,
        signed_prekey_seed: 15,
        one_time_prekey_seed: 16,
    };
    let b2 = DeviceSpec {
        id: "bob-b2",
        signing_seed: 17,
        static_seed: 18,
        signed_prekey_seed: 19,
        one_time_prekey_seed: 20,
    };
    let alice_document = did_document(alice_did, &[a1, a2, a3]);
    let bob_document = did_document(bob_did, &[b1, b2]);
    let alice = context(root.path(), "identity-alice-a1", alice_did, a1, 31);
    let mut host = FakeHost::default();
    host.add_document(alice_did, alice_document.clone());
    host.add_document(bob_did, bob_document.clone());
    for (did, device) in [
        (alice_did, a2),
        (alice_did, a3),
        (bob_did, b1),
        (bob_did, b2),
    ] {
        host.add_bundle(did, device);
    }

    let summary = send_with_host(
        &alice,
        &mut host,
        text_input("logical-1", bob_did, "hello every device"),
    )
    .await
    .unwrap();
    assert!(summary.fully_accepted());
    assert_eq!(summary.target_device_count, 2);
    assert_eq!(summary.own_sync_device_count, 2);
    assert_eq!(summary.attempted_device_count, 4);
    assert_eq!(summary.previously_accepted_device_count, 0);
    assert_eq!(summary.newly_accepted_device_count, 4);
    assert_eq!(summary.accepted_device_count, 4);
    assert_eq!(host.post_attempts.len(), 4);
    assert!(host
        .post_attempts
        .iter()
        .all(|prepared| matches!(prepared.body, V2DirectBody::Init(_))));
    let operation_ids = host
        .post_attempts
        .iter()
        .map(|prepared| prepared.metadata.operation_id.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(operation_ids.len(), 4);
    for prepared in &host.post_attempts {
        let request = prepared.direct_request().unwrap();
        assert_eq!(request["method"], "direct.send");
        assert!(request["params"].get("deliveries").is_none());
        assert_eq!(prepared.metadata.content_type, CONTENT_TYPE_DIRECT_INIT_V2);
    }

    for (did, device, expected_sync) in [
        (bob_did, b1, false),
        (bob_did, b2, false),
        (alice_did, a2, true),
        (alice_did, a3, true),
    ] {
        let recipient = context(
            root.path(),
            &format!("identity-{}", device.id),
            did,
            device,
            40 + device.signing_seed,
        );
        install_bundle(&recipient, did, device);
        let prepared = host
            .post_attempts
            .iter()
            .find(|prepared| {
                prepared.metadata.target.did == did
                    && prepared.metadata.recipient_device_id == device.id
            })
            .unwrap();
        let mut receiver_host = FakeHost::default();
        receiver_host.add_document(alice_did, alice_document.clone());
        receiver_host.add_document(bob_did, bob_document.clone());
        let outcome = receive_with_host(
            &recipient,
            &mut receiver_host,
            prepared.metadata.clone(),
            prepared.body.clone(),
        )
        .await
        .unwrap();
        match (expected_sync, outcome) {
            (false, V2InboundProductOutcome::Business(projection)) => {
                assert_eq!(projection.logical_message_id, "logical-1");
                assert_eq!(
                    projection.body,
                    V2InboundBusinessBody::Text {
                        text: "hello every device".to_owned(),
                        markdown: false
                    }
                );
            }
            (true, V2InboundProductOutcome::OwnSync(projection)) => {
                assert_eq!(projection.logical_message_id, "logical-1");
                assert_eq!(projection.original_sender_did, alice_did);
                assert_eq!(projection.original_sender_device_id, a1.id);
                assert_eq!(projection.target_did, bob_did);
                assert_eq!(
                    projection.body,
                    V2InboundBusinessBody::Text {
                        text: "hello every device".to_owned(),
                        markdown: false
                    }
                );
            }
            (_, other) => panic!("unexpected product outcome: {other:?}"),
        }
        assert_eq!(
            receiver_host.post_attempts.len(),
            1,
            "only hidden session reply is added"
        );
    }

    let db_bytes = std::fs::read(&alice.sqlite_path).unwrap();
    assert!(!db_bytes
        .windows(b"hello every device".len())
        .any(|window| window == b"hello every device"));
    let connection = alice.open_connection().unwrap();
    let columns = connection
        .prepare("PRAGMA table_info(direct_e2ee_v2_delivery_ledger)")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(!columns.iter().any(|column| {
        column.contains("plaintext") || column.contains("ciphertext") || column.contains("private")
    }));
    let distinct_business_intents: i64 = connection
        .query_row(
            "SELECT COUNT(DISTINCT source_digest) FROM direct_e2ee_v2_delivery_ledger",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(distinct_business_intents, 1);
}

pub(crate) async fn fresh_scoped_business_receive_for_projection_test(
) -> (Value, V2InboundProductOutcome) {
    let root = tempfile::tempdir().unwrap();
    let alice_did = "did:example:alice";
    let bob_did = "did:example:bob";
    let mallory_did = "did:example:mallory";
    let alice = DeviceSpec {
        id: "alice-scoped",
        signing_seed: 81,
        static_seed: 82,
        signed_prekey_seed: 83,
        one_time_prekey_seed: 84,
    };
    let mallory = DeviceSpec {
        id: "mallory-scoped",
        signing_seed: 85,
        static_seed: 86,
        signed_prekey_seed: 87,
        one_time_prekey_seed: 88,
    };
    let alice_document = did_document(alice_did, &[alice]);
    let mallory_document = did_document(mallory_did, &[mallory]);
    let sender = context(
        root.path(),
        "identity-mallory-scoped",
        mallory_did,
        mallory,
        89,
    );
    let recipient = context(root.path(), "identity-alice-scoped", alice_did, alice, 90);
    install_bundle(&recipient, alice_did, alice);
    let mut sender_host = FakeHost::default();
    sender_host.add_document(alice_did, alice_document.clone());
    sender_host.add_document(mallory_did, mallory_document.clone());
    sender_host.add_bundle(alice_did, alice);
    send_with_host(
        &sender,
        &mut sender_host,
        text_input("logical-scoped-business", alice_did, "scoped business"),
    )
    .await
    .unwrap();
    let prepared = sender_host.post_attempts.pop().unwrap();
    let mut recipient_host = FakeHost::default();
    recipient_host.add_document(alice_did, alice_document);
    recipient_host.add_document(mallory_did, mallory_document);

    let rejected = receive_with_host_scoped(
        &recipient,
        &mut recipient_host,
        prepared.metadata.clone(),
        prepared.body.clone(),
        Some(bob_did),
    )
    .await
    .unwrap_err();
    assert!(is_scoped_peer_mismatch(&rejected));
    assert!(recipient_host.post_attempts.is_empty());

    let accepted = receive_with_host_scoped(
        &recipient,
        &mut recipient_host,
        prepared.metadata.clone(),
        prepared.body.clone(),
        Some(mallory_did),
    )
    .await
    .unwrap();
    let V2InboundProductOutcome::Business(projection) = &accepted else {
        panic!("expected business projection");
    };
    assert_eq!(projection.sender_did, mallory_did);
    assert_eq!(projection.recipient_did, alice_did);
    assert_eq!(recipient_host.post_attempts.len(), 1);

    let replay = receive_with_host_scoped(
        &recipient,
        &mut recipient_host,
        prepared.metadata.clone(),
        prepared.body.clone(),
        Some(mallory_did),
    )
    .await
    .unwrap();
    assert_eq!(replay, V2InboundProductOutcome::Replay);
    let direct_request = prepared.direct_request().unwrap();
    let wire = json!({
        "id": prepared.metadata.message_id,
        "sender_did": prepared.metadata.sender_did,
        "receiver_did": "did:example:service-forged",
        "content_type": prepared.metadata.content_type,
        "server_seq": 9,
        "meta": direct_request["params"]["meta"].clone(),
        "body": direct_request["params"]["body"].clone(),
    });
    (wire, accepted)
}

#[tokio::test]
async fn scoped_business_receive_rejects_wrong_peer_before_replay_commit() {
    let (wire, outcome) = fresh_scoped_business_receive_for_projection_test().await;
    assert_eq!(wire["receiver_did"], "did:example:service-forged");
    assert!(matches!(outcome, V2InboundProductOutcome::Business(_)));
}

#[tokio::test]
async fn scoped_own_sync_receive_binds_decrypted_target_before_replay_commit() {
    let root = tempfile::tempdir().unwrap();
    let alice_did = "did:example:alice";
    let bob_did = "did:example:bob";
    let mallory_did = "did:example:mallory";
    let alice_sender = DeviceSpec {
        id: "alice-own-sync-sender",
        signing_seed: 91,
        static_seed: 92,
        signed_prekey_seed: 93,
        one_time_prekey_seed: 94,
    };
    let alice_recipient = DeviceSpec {
        id: "alice-own-sync-recipient",
        signing_seed: 95,
        static_seed: 96,
        signed_prekey_seed: 97,
        one_time_prekey_seed: 98,
    };
    let mallory = DeviceSpec {
        id: "mallory-own-sync",
        signing_seed: 99,
        static_seed: 100,
        signed_prekey_seed: 101,
        one_time_prekey_seed: 102,
    };
    let alice_document = did_document(alice_did, &[alice_sender, alice_recipient]);
    let mallory_document = did_document(mallory_did, &[mallory]);
    let sender = context(
        root.path(),
        "identity-alice-own-sync-sender",
        alice_did,
        alice_sender,
        103,
    );
    let recipient = context(
        root.path(),
        "identity-alice-own-sync-recipient",
        alice_did,
        alice_recipient,
        104,
    );
    install_bundle(&recipient, alice_did, alice_recipient);
    let mut sender_host = FakeHost::default();
    sender_host.add_document(alice_did, alice_document.clone());
    sender_host.add_document(mallory_did, mallory_document.clone());
    sender_host.add_bundle(alice_did, alice_recipient);
    sender_host.add_bundle(mallory_did, mallory);
    send_with_host(
        &sender,
        &mut sender_host,
        text_input("logical-scoped-own-sync", mallory_did, "scoped own sync"),
    )
    .await
    .unwrap();
    let prepared = sender_host
        .post_attempts
        .iter()
        .find(|prepared| {
            prepared.metadata.target.did == alice_did
                && prepared.metadata.recipient_device_id == alice_recipient.id
        })
        .unwrap()
        .clone();
    let mut recipient_host = FakeHost::default();
    recipient_host.add_document(alice_did, alice_document);
    recipient_host.add_document(mallory_did, mallory_document);

    let rejected = receive_with_host_scoped(
        &recipient,
        &mut recipient_host,
        prepared.metadata.clone(),
        prepared.body.clone(),
        Some(bob_did),
    )
    .await
    .unwrap_err();
    assert!(is_scoped_peer_mismatch(&rejected));

    let accepted = receive_with_host_scoped(
        &recipient,
        &mut recipient_host,
        prepared.metadata,
        prepared.body,
        Some(mallory_did),
    )
    .await
    .unwrap();
    let V2InboundProductOutcome::OwnSync(projection) = accepted else {
        panic!("expected own-sync projection");
    };
    assert_eq!(projection.target_did, mallory_did);
}

#[tokio::test]
async fn prekey_failure_keeps_wire_unprepared_and_retries_same_operation() {
    let root = tempfile::tempdir().unwrap();
    let alice_did = "did:example:alice";
    let bob_did = "did:example:bob";
    let a1 = DeviceSpec {
        id: "alice-a1",
        signing_seed: 51,
        static_seed: 52,
        signed_prekey_seed: 53,
        one_time_prekey_seed: 54,
    };
    let b1 = DeviceSpec {
        id: "bob-b1",
        signing_seed: 55,
        static_seed: 56,
        signed_prekey_seed: 57,
        one_time_prekey_seed: 58,
    };
    let alice = context(root.path(), "identity-alice-a1", alice_did, a1, 61);
    let mut host = FakeHost::default();
    host.add_document(alice_did, did_document(alice_did, &[a1]));
    host.add_document(bob_did, did_document(bob_did, &[b1]));
    host.add_bundle(bob_did, b1);
    host.fail_fetch_once
        .insert((bob_did.to_owned(), b1.id.to_owned()));

    let first = send_with_host(
        &alice,
        &mut host,
        text_input("logical-retry", bob_did, "retry once"),
    )
    .await
    .unwrap();
    assert_eq!(first.failed_device_count, 1);
    assert_eq!(first.attempted_device_count, 1);
    assert_eq!(first.previously_accepted_device_count, 0);
    assert_eq!(first.newly_accepted_device_count, 0);
    assert_eq!(first.accepted_device_count, 0);
    let connection = alice.open_connection().unwrap();
    let (wire_prepared, operation_id): (i64, String) = connection
        .query_row(
            "SELECT wire_prepared, operation_id FROM direct_e2ee_v2_delivery_ledger",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(wire_prepared, 0);
    drop(connection);

    let second = send_with_host(
        &alice,
        &mut host,
        text_input("logical-retry", bob_did, "retry once"),
    )
    .await
    .unwrap();
    assert!(second.fully_accepted());
    assert_eq!(second.attempted_device_count, 1);
    assert_eq!(second.previously_accepted_device_count, 0);
    assert_eq!(second.newly_accepted_device_count, 1);
    assert_eq!(host.fetch_operations.len(), 2);
    assert_eq!(host.fetch_operations[0].2, operation_id);
    assert_eq!(host.fetch_operations[1].2, operation_id);
    assert_eq!(host.post_attempts.len(), 1);
    assert_eq!(host.post_attempts[0].metadata.operation_id, operation_id);
}

#[tokio::test]
async fn partial_retry_attempts_only_the_failed_device() {
    let root = tempfile::tempdir().unwrap();
    let alice_did = "did:example:alice";
    let bob_did = "did:example:bob";
    let a1 = DeviceSpec {
        id: "alice-a1",
        signing_seed: 201,
        static_seed: 202,
        signed_prekey_seed: 203,
        one_time_prekey_seed: 204,
    };
    let b1 = DeviceSpec {
        id: "bob-b1",
        signing_seed: 205,
        static_seed: 206,
        signed_prekey_seed: 207,
        one_time_prekey_seed: 208,
    };
    let b2 = DeviceSpec {
        id: "bob-b2",
        signing_seed: 209,
        static_seed: 210,
        signed_prekey_seed: 211,
        one_time_prekey_seed: 212,
    };
    let alice = context(root.path(), "identity-alice-a1", alice_did, a1, 213);
    let mut host = FakeHost::default();
    host.add_document(alice_did, did_document(alice_did, &[a1]));
    host.add_document(bob_did, did_document(bob_did, &[b1, b2]));
    host.add_bundle(bob_did, b1);
    host.add_bundle(bob_did, b2);
    host.fail_post_once
        .insert((bob_did.to_owned(), b2.id.to_owned()));

    let input = || text_input("logical-partial", bob_did, "retry failed only");
    let first = send_with_host(&alice, &mut host, input()).await.unwrap();
    assert_eq!(first.attempted_device_count, 2);
    assert_eq!(first.previously_accepted_device_count, 0);
    assert_eq!(first.newly_accepted_device_count, 1);
    assert_eq!(first.accepted_device_count, 1);
    assert_eq!(first.failed_device_count, 1);
    assert_eq!(host.post_attempts.len(), 2);

    let second = send_with_host(&alice, &mut host, input()).await.unwrap();
    assert!(second.fully_accepted());
    assert_eq!(second.attempted_device_count, 1);
    assert_eq!(second.previously_accepted_device_count, 1);
    assert_eq!(second.newly_accepted_device_count, 1);
    assert_eq!(second.accepted_device_count, 2);
    assert_eq!(second.failed_device_count, 0);
    assert_eq!(host.post_attempts.len(), 3);
    assert_eq!(
        host.post_attempts
            .last()
            .unwrap()
            .metadata
            .recipient_device_id,
        b2.id
    );
    assert_eq!(
        host.post_attempts
            .iter()
            .filter(|prepared| prepared.metadata.recipient_device_id == b1.id)
            .count(),
        1
    );
}

#[tokio::test]
async fn stale_target_post_is_recorded_and_propagated_to_rebind_coordinator() {
    let root = tempfile::tempdir().unwrap();
    let alice_did = "did:example:alice";
    let bob_did = "did:example:bob-old";
    let a1 = DeviceSpec {
        id: "alice-a1",
        signing_seed: 221,
        static_seed: 222,
        signed_prekey_seed: 223,
        one_time_prekey_seed: 224,
    };
    let b1 = DeviceSpec {
        id: "bob-b1",
        signing_seed: 225,
        static_seed: 226,
        signed_prekey_seed: 227,
        one_time_prekey_seed: 228,
    };
    let alice = context(root.path(), "identity-alice-a1", alice_did, a1, 229);
    let mut host = FakeHost::default();
    host.add_document(alice_did, did_document(alice_did, &[a1]));
    host.add_document(bob_did, did_document(bob_did, &[b1]));
    host.add_bundle(bob_did, b1);
    host.stale_post_once
        .insert((bob_did.to_owned(), b1.id.to_owned()));

    let error = send_with_host(
        &alice,
        &mut host,
        text_input("logical-stale", bob_did, "retry after rebind"),
    )
    .await
    .unwrap_err();
    assert!(
        crate::internal::service_error::stale_target_binding_from_error(&error, alice_did)
            .is_some()
    );
    let connection = alice.open_connection().unwrap();
    let (phase, failure_code): (String, Option<String>) = connection
        .query_row(
            "SELECT phase, failure_code FROM direct_e2ee_v2_delivery_ledger WHERE delivery_class = 'recipient'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(phase, "failed");
    assert_eq!(failure_code.as_deref(), Some("transport"));
}

#[tokio::test]
async fn logical_message_retry_is_bound_to_body_and_conversation_digest() {
    let root = tempfile::tempdir().unwrap();
    let alice_did = "did:example:alice";
    let bob_did = "did:example:bob";
    let a1 = DeviceSpec {
        id: "alice-a1",
        signing_seed: 63,
        static_seed: 64,
        signed_prekey_seed: 65,
        one_time_prekey_seed: 66,
    };
    let b1 = DeviceSpec {
        id: "bob-b1",
        signing_seed: 67,
        static_seed: 68,
        signed_prekey_seed: 69,
        one_time_prekey_seed: 70,
    };
    let alice = context(root.path(), "identity-alice-a1", alice_did, a1, 62);
    let mut host = FakeHost::default();
    host.add_document(alice_did, did_document(alice_did, &[a1]));
    host.add_document(bob_did, did_document(bob_did, &[b1]));
    host.add_bundle(bob_did, b1);

    let original = text_input("logical-intent-bound", bob_did, "original body");
    assert!(send_with_host(&alice, &mut host, original)
        .await
        .unwrap()
        .fully_accepted());
    assert_eq!(host.post_attempts.len(), 1);

    let changed_body = text_input("logical-intent-bound", bob_did, "different body");
    assert!(matches!(
        send_with_host(&alice, &mut host, changed_body).await,
        Err(crate::ImError::PermissionDenied)
    ));
    let mut changed_conversation = text_input("logical-intent-bound", bob_did, "original body");
    changed_conversation.conversation_id = Some("different-conversation".to_owned());
    assert!(matches!(
        send_with_host(&alice, &mut host, changed_conversation).await,
        Err(crate::ImError::PermissionDenied)
    ));
    assert_eq!(host.post_attempts.len(), 1);

    let connection = alice.open_connection().unwrap();
    let source_digest: String = connection
        .query_row(
            "SELECT source_digest FROM direct_e2ee_v2_delivery_ledger",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(source_digest.starts_with("sha256:"));
    assert!(!source_digest.contains("original body"));
}

#[tokio::test]
async fn reply_failure_is_durable_and_drain_reuses_exact_ciphertext() {
    let root = tempfile::tempdir().unwrap();
    let alice_did = "did:example:alice";
    let bob_did = "did:example:bob";
    let a1 = DeviceSpec {
        id: "alice-a1",
        signing_seed: 71,
        static_seed: 72,
        signed_prekey_seed: 73,
        one_time_prekey_seed: 74,
    };
    let b1 = DeviceSpec {
        id: "bob-b1",
        signing_seed: 75,
        static_seed: 76,
        signed_prekey_seed: 77,
        one_time_prekey_seed: 78,
    };
    let alice_document = did_document(alice_did, &[a1]);
    let bob_document = did_document(bob_did, &[b1]);
    let alice = context(root.path(), "identity-alice-a1", alice_did, a1, 81);
    let bob = context(root.path(), "identity-bob-b1", bob_did, b1, 82);
    install_bundle(&bob, bob_did, b1);

    let mut sender_host = FakeHost::default();
    sender_host.add_document(alice_did, alice_document.clone());
    sender_host.add_document(bob_did, bob_document.clone());
    sender_host.add_bundle(bob_did, b1);
    send_with_host(
        &alice,
        &mut sender_host,
        text_input("logical-reply", bob_did, "reply durable"),
    )
    .await
    .unwrap();
    let init = sender_host.post_attempts[0].clone();

    let mut failing_receiver = FakeHost::default();
    failing_receiver.add_document(alice_did, alice_document.clone());
    failing_receiver.add_document(bob_did, bob_document.clone());
    failing_receiver
        .fail_post_once
        .insert((alice_did.to_owned(), a1.id.to_owned()));
    let outcome = receive_with_host(
        &bob,
        &mut failing_receiver,
        init.metadata.clone(),
        init.body.clone(),
    )
    .await
    .unwrap();
    let V2InboundProductOutcome::Business(projection) = outcome else {
        panic!("Init business must still be delivered");
    };
    assert!(projection.session_reply_pending);
    assert_eq!(failing_receiver.post_attempts.len(), 1);
    let failed_request = failing_receiver.post_attempts[0].direct_request().unwrap();
    let connection = bob.open_connection().unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM direct_e2ee_v2_session_reply_ledger WHERE phase = 'pending'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1
    );
    drop(connection);

    let mut restarted_host = FakeHost::default();
    restarted_host.add_document(alice_did, alice_document);
    restarted_host.add_document(bob_did, bob_document);
    let retry = retry_session_replies_with_host(&bob, &mut restarted_host)
        .await
        .unwrap();
    assert_eq!(
        retry,
        V2SessionReplyRetrySummary {
            attempted: 1,
            accepted: 1,
            failed: 0,
            ineligible: 0
        }
    );
    assert_eq!(restarted_host.post_attempts.len(), 1);
    assert_eq!(
        restarted_host.post_attempts[0].direct_request().unwrap(),
        failed_request
    );
    let second = retry_session_replies_with_host(&bob, &mut restarted_host)
        .await
        .unwrap();
    assert_eq!(second.attempted, 0);
}

#[tokio::test]
async fn established_pair_uses_cipher_after_reply_and_controls_never_project_as_chat() {
    let root = tempfile::tempdir().unwrap();
    let alice_did = "did:example:alice";
    let bob_did = "did:example:bob";
    let a1 = DeviceSpec {
        id: "alice-a1",
        signing_seed: 91,
        static_seed: 92,
        signed_prekey_seed: 93,
        one_time_prekey_seed: 94,
    };
    let b1 = DeviceSpec {
        id: "bob-b1",
        signing_seed: 95,
        static_seed: 96,
        signed_prekey_seed: 97,
        one_time_prekey_seed: 98,
    };
    let alice_document = did_document(alice_did, &[a1]);
    let bob_document = did_document(bob_did, &[b1]);
    let alice = context(root.path(), "identity-alice-a1", alice_did, a1, 101);
    let bob = context(root.path(), "identity-bob-b1", bob_did, b1, 102);
    install_bundle(&bob, bob_did, b1);

    let mut alice_host = FakeHost::default();
    alice_host.add_document(alice_did, alice_document.clone());
    alice_host.add_document(bob_did, bob_document.clone());
    alice_host.add_bundle(bob_did, b1);
    send_with_host(
        &alice,
        &mut alice_host,
        text_input("logical-first", bob_did, "first"),
    )
    .await
    .unwrap();
    assert!(matches!(
        alice_host.post_attempts[0].body,
        V2DirectBody::Init(_)
    ));

    let mut bob_host = FakeHost::default();
    bob_host.add_document(alice_did, alice_document.clone());
    bob_host.add_document(bob_did, bob_document.clone());
    let first = alice_host.post_attempts[0].clone();
    let first_outcome = receive_with_host(&bob, &mut bob_host, first.metadata, first.body)
        .await
        .unwrap();
    assert!(matches!(
        first_outcome,
        V2InboundProductOutcome::Business(_)
    ));
    let reply = bob_host.post_attempts[0].clone();
    let reply_outcome = receive_with_host(&alice, &mut alice_host, reply.metadata, reply.body)
        .await
        .unwrap();
    assert_eq!(reply_outcome, V2InboundProductOutcome::ConsumedControl);

    let before = alice_host.post_attempts.len();
    send_with_host(
        &alice,
        &mut alice_host,
        text_input("logical-second", bob_did, "second"),
    )
    .await
    .unwrap();
    let second = &alice_host.post_attempts[before];
    assert!(matches!(second.body, V2DirectBody::Cipher(_)));

    let metadata = V2DirectMetadata {
        anp_version: None,
        profile: DIRECT_E2EE_PROFILE_V2.to_owned(),
        security_profile: anp::direct_e2ee::DIRECT_E2EE_SECURITY_PROFILE.to_owned(),
        sender_did: alice_did.to_owned(),
        sender_device_id: a1.id.to_owned(),
        target: anp::direct_e2ee::V2Target {
            kind: "agent".to_owned(),
            did: bob_did.to_owned(),
        },
        recipient_device_id: b1.id.to_owned(),
        operation_id: "control-test".to_owned(),
        message_id: "control-test".to_owned(),
        content_type: anp::direct_e2ee::CONTENT_TYPE_DIRECT_CIPHER_V2.to_owned(),
        created_at: None,
    };
    let unknown = V2ApplicationPlaintext {
        application_content_type: "application/json".to_owned(),
        logical_message_id: None,
        conversation_id: None,
        reply_to_message_id: None,
        annotations: None,
        text: None,
        payload: Some(json!({"system_type": "awiki.device.future-control.v9", "data": "hidden"})),
        payload_b64u: None,
    };
    assert!(matches!(
        validate_inbound_plaintext(&unknown, &metadata, None).unwrap(),
        ValidatedInboundPlaintext::SuppressedControl
    ));
    let malformed = V2ApplicationPlaintext {
        payload: Some(json!({"system_type": 7, "otherwise": "looks ordinary"})),
        ..unknown
    };
    assert!(matches!(
        validate_inbound_plaintext(&malformed, &metadata, None).unwrap(),
        ValidatedInboundPlaintext::SuppressedControl
    ));
    assert!(V2OrdinaryBody::Json {
        payload: json!({"system_type": "awiki.device.future-control.v9"})
    }
    .plaintext("logical-control", None)
    .is_err());
    assert!(V2OrdinaryBody::Json {
        payload: json!({"schema": "awiki.agent.command.v1", "value": 1})
    }
    .plaintext("logical-json", None)
    .is_ok());
}

struct FakeAttachmentObjectHost {
    prepare_calls: usize,
    upload_calls: usize,
    commit_calls: usize,
    source_digest: String,
    prepared: V2PreparedAttachmentProduct,
    ciphertext: Vec<u8>,
}

impl FakeAttachmentObjectHost {
    fn new(plaintext: &[u8]) -> Self {
        let encrypted = crate::attachments::manifest::prepare_object_e2ee_attachment_payload(
            "multi-device.txt",
            "text/plain",
            plaintext.to_vec(),
        )
        .unwrap();
        let descriptor = crate::attachments::manifest::AttachmentDescriptor::from_prepared(
            &encrypted.prepared,
            "attachment-multi-1",
            "https://objects.example/attachment-multi-1",
        );
        let full_manifest =
            crate::attachments::manifest::build_attachment_manifest_with_object_e2ee_secrets(
                &descriptor,
                "one encrypted object",
                &encrypted.secrets,
            )
            .unwrap();
        let redacted_manifest = crate::attachments::manifest::build_attachment_manifest(
            &descriptor,
            "one encrypted object",
        )
        .unwrap();
        let grant_ref =
            crate::attachments::manifest::build_attachment_grant_ref(&descriptor).unwrap();
        Self {
            prepare_calls: 0,
            upload_calls: 0,
            commit_calls: 0,
            source_digest: format!(
                "sha256:{}",
                URL_SAFE_NO_PAD.encode(Sha256::digest(plaintext))
            ),
            prepared: V2PreparedAttachmentProduct {
                full_manifest,
                redacted_manifest,
                grant_ref,
            },
            ciphertext: encrypted.prepared.payload,
        }
    }
}

impl V2AttachmentObjectHost for FakeAttachmentObjectHost {
    fn source_digest(&self) -> &str {
        &self.source_digest
    }

    async fn prepare_and_commit_object(&mut self) -> crate::ImResult<V2PreparedAttachmentProduct> {
        self.prepare_calls += 1;
        self.upload_calls += 1;
        self.commit_calls += 1;
        Ok(self.prepared.clone())
    }
}

#[test]
fn attachment_intent_process_lock_serializes_exact_owner_device_and_logical_target() {
    let root = tempfile::tempdir().unwrap();
    let alice_did = "did:example:alice";
    let a1 = DeviceSpec {
        id: "alice-a1",
        signing_seed: 101,
        static_seed: 102,
        signed_prekey_seed: 103,
        one_time_prekey_seed: 104,
    };
    let alice = context(root.path(), "identity-alice-a1", alice_did, a1, 105);
    let input = V2AttachmentFanoutInput {
        logical_message_id: "logical-lock".to_owned(),
        target_did: "did:example:bob".to_owned(),
        conversation_id: None,
    };
    let first = attachment_intent_lock(&alice, &input);
    let second = attachment_intent_lock(&alice, &input);
    assert!(Arc::ptr_eq(&first, &second));
    let guard = first.try_lock().unwrap();
    assert!(second.try_lock().is_err());
    drop(guard);
    assert!(second.try_lock().is_ok());

    let other = attachment_intent_lock(
        &alice,
        &V2AttachmentFanoutInput {
            logical_message_id: "logical-lock-other".to_owned(),
            ..input
        },
    );
    assert!(!Arc::ptr_eq(&first, &other));
}

#[tokio::test]
async fn scoped_attachment_rejection_does_not_commit_replay_before_correct_peer_receive() {
    let root = tempfile::tempdir().unwrap();
    let alice_did = "did:example:alice";
    let bob_did = "did:example:bob";
    let mallory_did = "did:example:mallory";
    let alice = DeviceSpec {
        id: "alice-attachment-scope",
        signing_seed: 181,
        static_seed: 182,
        signed_prekey_seed: 183,
        one_time_prekey_seed: 184,
    };
    let mallory = DeviceSpec {
        id: "mallory-attachment-scope",
        signing_seed: 185,
        static_seed: 186,
        signed_prekey_seed: 187,
        one_time_prekey_seed: 188,
    };
    let alice_document = did_document(alice_did, &[alice]);
    let mallory_document = did_document(mallory_did, &[mallory]);
    let sender = context(
        root.path(),
        "identity-mallory-attachment-scope",
        mallory_did,
        mallory,
        189,
    );
    let recipient = context(
        root.path(),
        "identity-alice-attachment-scope",
        alice_did,
        alice,
        190,
    );
    install_bundle(&recipient, alice_did, alice);
    let attachment = FakeAttachmentObjectHost::new(b"scoped attachment plaintext");
    let mut sender_host = FakeHost::default();
    sender_host.add_document(alice_did, alice_document.clone());
    sender_host.add_document(mallory_did, mallory_document.clone());
    sender_host.add_bundle(alice_did, alice);
    send_with_host(
        &sender,
        &mut sender_host,
        V2DirectProductSendInput {
            logical_message_id: "logical-scoped-attachment".to_owned(),
            target_did: alice_did.to_owned(),
            conversation_id: Some("conversation-scoped-attachment".to_owned()),
            body: V2OrdinaryBody::AttachmentManifest {
                full_manifest: attachment.prepared.full_manifest,
            },
        },
    )
    .await
    .unwrap();
    let prepared = sender_host.post_attempts.pop().unwrap();
    let mut recipient_host = FakeHost::default();
    recipient_host.add_document(alice_did, alice_document);
    recipient_host.add_document(mallory_did, mallory_document);

    let rejected = receive_with_host_scoped(
        &recipient,
        &mut recipient_host,
        prepared.metadata.clone(),
        prepared.body.clone(),
        Some(bob_did),
    )
    .await
    .unwrap_err();
    assert!(is_scoped_peer_mismatch(&rejected));
    assert!(recipient_host.post_attempts.is_empty());

    let accepted = receive_with_host_scoped(
        &recipient,
        &mut recipient_host,
        prepared.metadata,
        prepared.body,
        Some(mallory_did),
    )
    .await
    .unwrap();
    assert!(matches!(
        accepted,
        V2InboundProductOutcome::Business(V2InboundBusinessProjection {
            body: V2InboundBusinessBody::Attachment { .. },
            ..
        })
    ));
}

#[tokio::test]
async fn attachment_object_is_committed_once_and_one_manifest_is_wrapped_per_device() {
    let root = tempfile::tempdir().unwrap();
    let alice_did = "did:example:alice";
    let bob_did = "did:example:bob";
    let a1 = DeviceSpec {
        id: "alice-a1",
        signing_seed: 111,
        static_seed: 112,
        signed_prekey_seed: 113,
        one_time_prekey_seed: 114,
    };
    let a2 = DeviceSpec {
        id: "alice-a2",
        signing_seed: 115,
        static_seed: 116,
        signed_prekey_seed: 117,
        one_time_prekey_seed: 118,
    };
    let b1 = DeviceSpec {
        id: "bob-b1",
        signing_seed: 119,
        static_seed: 120,
        signed_prekey_seed: 121,
        one_time_prekey_seed: 122,
    };
    let b2 = DeviceSpec {
        id: "bob-b2",
        signing_seed: 123,
        static_seed: 124,
        signed_prekey_seed: 125,
        one_time_prekey_seed: 126,
    };
    let alice_document = did_document(alice_did, &[a1, a2]);
    let bob_document = did_document(bob_did, &[b1, b2]);
    let alice = context(root.path(), "identity-alice-a1", alice_did, a1, 127);
    let mut direct_host = FakeHost::default();
    direct_host.add_document(alice_did, alice_document.clone());
    direct_host.add_document(bob_did, bob_document.clone());
    for (did, device) in [(alice_did, a2), (bob_did, b1), (bob_did, b2)] {
        direct_host.add_bundle(did, device);
    }
    let attachment_plaintext = b"one object, three exact device envelopes";
    let mut object_host = FakeAttachmentObjectHost::new(attachment_plaintext);
    let object_key = object_host.prepared.full_manifest["attachments"][0]["encryption_info"]
        ["object_key_b64u"]
        .as_str()
        .unwrap()
        .to_owned();
    let nonce = object_host.prepared.full_manifest["attachments"][0]["encryption_info"]
        ["nonce_b64u"]
        .as_str()
        .unwrap()
        .to_owned();

    let summary = send_attachment_with_hosts(
        &alice,
        &mut direct_host,
        &mut object_host,
        V2AttachmentFanoutInput {
            logical_message_id: "logical-attachment-1".to_owned(),
            target_did: bob_did.to_owned(),
            conversation_id: Some("conversation-attachment".to_owned()),
        },
    )
    .await
    .unwrap();
    assert_eq!(object_host.prepare_calls, 1);
    assert_eq!(object_host.upload_calls, 1);
    assert_eq!(object_host.commit_calls, 1);
    assert_eq!(summary.direct.target_device_count, 2);
    assert_eq!(summary.direct.own_sync_device_count, 1);
    assert_eq!(direct_host.post_attempts.len(), 3);
    assert_eq!(direct_host.post_attachment_grant_refs.len(), 3);
    assert!(direct_host
        .post_attachment_grant_refs
        .iter()
        .all(|grant_ref| grant_ref == &summary.grant_ref));
    assert!(!format!("{summary:?}").contains(&object_key));
    assert!(!format!("{summary:?}").contains(&nonce));

    for prepared in &direct_host.post_attempts {
        let request = prepared.direct_request().unwrap();
        let request_text = request.to_string();
        assert_eq!(request["method"], "direct.send");
        assert!(!request_text.contains(&object_key));
        assert!(!request_text.contains(&nonce));
        let debug = format!("{prepared:?}");
        assert!(!debug.contains(&object_key));
        assert!(!debug.contains(&nonce));
    }

    for (did, device, own_sync) in [
        (bob_did, b1, false),
        (bob_did, b2, false),
        (alice_did, a2, true),
    ] {
        let recipient = context(
            root.path(),
            &format!("identity-{}", device.id),
            did,
            device,
            130 + device.signing_seed % 20,
        );
        install_bundle(&recipient, did, device);
        let prepared = direct_host
            .post_attempts
            .iter()
            .find(|prepared| {
                prepared.metadata.target.did == did
                    && prepared.metadata.recipient_device_id == device.id
            })
            .unwrap();
        let mut receiver_host = FakeHost::default();
        receiver_host.add_document(alice_did, alice_document.clone());
        receiver_host.add_document(bob_did, bob_document.clone());
        let outcome = receive_with_host(
            &recipient,
            &mut receiver_host,
            prepared.metadata.clone(),
            prepared.body.clone(),
        )
        .await
        .unwrap();
        let full_manifest = match (own_sync, outcome) {
            (
                false,
                V2InboundProductOutcome::Business(V2InboundBusinessProjection {
                    body: V2InboundBusinessBody::Attachment { full_manifest },
                    ..
                }),
            ) => full_manifest,
            (
                true,
                V2InboundProductOutcome::OwnSync(V2InboundOwnSyncProjection {
                    body: V2InboundBusinessBody::Attachment { full_manifest },
                    target_did,
                    ..
                }),
            ) => {
                assert_eq!(target_did, bob_did);
                full_manifest
            }
            (_, other) => panic!("unexpected attachment outcome: {other:?}"),
        };
        let parsed =
            crate::attachments::manifest::parse_attachment_manifest_internal(&full_manifest)
                .unwrap();
        let attachment = &parsed.attachments[0];
        let decrypted = crate::internal::attachment_runtime::object_crypto::decrypt_object_e2ee(
            &object_host.ciphertext,
            attachment.object_key_b64u.as_deref().unwrap(),
            attachment.nonce_b64u.as_deref().unwrap(),
        )
        .unwrap();
        assert_eq!(decrypted, attachment_plaintext);
    }

    let db_bytes = std::fs::read(&alice.sqlite_path).unwrap();
    assert!(!db_bytes
        .windows(object_key.len())
        .any(|window| window == object_key.as_bytes()));
    assert!(!db_bytes
        .windows(nonce.len())
        .any(|window| window == nonce.as_bytes()));
}

#[tokio::test]
async fn attachment_partial_retry_reuses_one_object_and_exact_device_ciphertext() {
    let root = tempfile::tempdir().unwrap();
    let alice_did = "did:example:alice";
    let bob_did = "did:example:bob";
    let a1 = DeviceSpec {
        id: "alice-a1",
        signing_seed: 141,
        static_seed: 142,
        signed_prekey_seed: 143,
        one_time_prekey_seed: 144,
    };
    let b1 = DeviceSpec {
        id: "bob-b1",
        signing_seed: 145,
        static_seed: 146,
        signed_prekey_seed: 147,
        one_time_prekey_seed: 148,
    };
    let b2 = DeviceSpec {
        id: "bob-b2",
        signing_seed: 149,
        static_seed: 150,
        signed_prekey_seed: 151,
        one_time_prekey_seed: 152,
    };
    let alice = context(root.path(), "identity-alice-a1", alice_did, a1, 153);
    let mut direct_host = FakeHost::default();
    direct_host.add_document(alice_did, did_document(alice_did, &[a1]));
    direct_host.add_document(bob_did, did_document(bob_did, &[b1, b2]));
    direct_host.add_bundle(bob_did, b1);
    direct_host.add_bundle(bob_did, b2);
    direct_host
        .fail_post_once
        .insert((bob_did.to_owned(), b2.id.to_owned()));
    let mut object_host = FakeAttachmentObjectHost::new(b"persist one encrypted object");

    let input = || V2AttachmentFanoutInput {
        logical_message_id: "logical-attachment-retry".to_owned(),
        target_did: bob_did.to_owned(),
        conversation_id: Some("conversation-attachment-retry".to_owned()),
    };
    let first = send_attachment_with_hosts(&alice, &mut direct_host, &mut object_host, input())
        .await
        .unwrap();
    assert_eq!(first.direct.accepted_device_count, 1);
    assert_eq!(first.direct.failed_device_count, 1);
    assert_eq!(first.direct.attempted_device_count, 2);
    assert_eq!(first.direct.previously_accepted_device_count, 0);
    assert_eq!(first.direct.newly_accepted_device_count, 1);
    assert_eq!(object_host.prepare_calls, 1);
    assert_eq!(object_host.upload_calls, 1);
    assert_eq!(object_host.commit_calls, 1);
    let failed_attempt = direct_host
        .post_attempts
        .iter()
        .find(|prepared| prepared.metadata.recipient_device_id == b2.id)
        .unwrap()
        .clone();

    let second = send_attachment_with_hosts(&alice, &mut direct_host, &mut object_host, input())
        .await
        .unwrap();
    assert!(second.direct.fully_accepted());
    assert_eq!(second.direct.attempted_device_count, 1);
    assert_eq!(second.direct.previously_accepted_device_count, 1);
    assert_eq!(second.direct.newly_accepted_device_count, 1);
    assert_eq!(object_host.prepare_calls, 1);
    assert_eq!(object_host.upload_calls, 1);
    assert_eq!(object_host.commit_calls, 1);
    assert_eq!(direct_host.post_attempts.len(), 3);
    assert_eq!(direct_host.post_attachment_grant_refs.len(), 3);
    let retry = direct_host.post_attempts.last().unwrap();
    assert_eq!(retry.metadata.recipient_device_id, b2.id);
    assert_eq!(retry, &failed_attempt, "retry must reuse exact wire bytes");

    let connection = alice.open_connection().unwrap();
    let attachment_intent_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM direct_e2ee_v2_attachment_intents",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(attachment_intent_count, 1);
    let (redacted_json, grant_json): (String, String) = connection
        .query_row(
            r#"SELECT redacted_manifest_json, grant_ref_json
FROM direct_e2ee_v2_attachment_intents"#,
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert!(!redacted_json.contains("object_key_b64u"));
    assert!(!redacted_json.contains("nonce_b64u"));
    assert!(!grant_json.contains("object_key_b64u"));
    assert!(!grant_json.contains("nonce_b64u"));
}
