//! Production Core receive/recovery with real P5/native custody and a synthetic
//! HTTP peer. Hooks exist only in the unit-test binary and match exact fixture
//! roots/origins. Child crash cuts exit the child process, never the test host.

use super::*;
use crate::internal::http::{HttpRequest, HttpResponse};
use crate::internal::identity_device_state::{
    DeviceAuthorizationProjection, IdentityDeviceState, IdentityInternalCheckpoint,
    IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
};
use crate::internal::identity_store::{
    IdentityStore, SaveIdentityInput, SaveIdentityKeyMode, SaveIdentitySecretStorage,
};
use crate::internal::secure_direct::v2_store::V2SessionExpectation;
use crate::vault::{DeviceVaultRootKey, FileSecretVault, FileSecretVaultStore};
use rusqlite::OptionalExtension;
use std::collections::BTreeMap;
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

const CONFIG: &str = "root-v2-integration.json";
const CHILD_ENV: &str = "AWIKI_ROOT_V2_CORE_CHILD";

#[derive(Clone, Serialize, Deserialize)]
struct Config {
    completion_v2: bool,
    domain: String,
    did: String,
    document: Value,
    document_hash: String,
    sender: String,
    sender_signing: String,
    sender_e2ee: String,
    recipient: String,
    recipient_signing: String,
    recipient_e2ee: String,
    metadata: V2DirectMetadata,
    cipher: Value,
    accepted_at: String,
}

#[derive(Default, Serialize, Deserialize)]
struct PeerState {
    clock: i64,
    crash: Option<String>,
    cuts: Vec<String>,
    completed: bool,
    completions: usize,
    commits: usize,
    intent_hash: Option<String>,
    nonce_hash: Option<String>,
    imported_at: Option<String>,
    proof_times: Vec<String>,
    registry_offset: u64,
    reject_next_completion_auth: bool,
    auth_refreshes: usize,
    preflight_delay_once: i64,
    first_preflight_finished: Option<i64>,
}

fn roots() -> &'static Mutex<Vec<PathBuf>> {
    static ROOTS: OnceLock<Mutex<Vec<PathBuf>>> = OnceLock::new();
    ROOTS.get_or_init(Default::default)
}

fn register(root: &Path) {
    let mut values = roots().lock().unwrap();
    if !values.iter().any(|value| value == root) {
        values.push(root.to_path_buf());
    }
}

fn config(root: &Path) -> Config {
    serde_json::from_slice(&std::fs::read(root.join(CONFIG)).unwrap()).unwrap()
}

fn with_state<T>(root: &Path, action: impl FnOnce(&mut PeerState) -> T) -> T {
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join("peer-state.json"))
        .unwrap();
    fs2::FileExt::lock_exclusive(&file).unwrap();
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).unwrap();
    let mut state = if bytes.is_empty() {
        PeerState::default()
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    let result = action(&mut state);
    file.rewind().unwrap();
    file.set_len(0).unwrap();
    file.write_all(&serde_json::to_vec(&state).unwrap())
        .unwrap();
    file.sync_all().unwrap();
    result
}

fn root_for(core: &crate::ImCore) -> Option<PathBuf> {
    let root = core
        .inner()
        .sdk_paths()
        .identities
        .identity_root_dir
        .parent()?
        .parent()?;
    if !root.join(CONFIG).is_file()
        || !roots()
            .lock()
            .unwrap()
            .iter()
            .any(|registered| registered == root)
        || core.inner().sdk_config().did_domain != config(root).domain
    {
        return None;
    }
    Some(root.to_path_buf())
}

pub(crate) fn clock_for(core: &crate::ImCore) -> Option<OffsetDateTime> {
    let root = root_for(core)?;
    Some(OffsetDateTime::from_unix_timestamp(with_state(&root, |state| state.clock)).unwrap())
}

pub(crate) fn crash_cut(core: &crate::ImCore, stage: &str) {
    let Some(root) = root_for(core) else {
        return;
    };
    if with_state(&root, |state| {
        if state.crash.as_deref() != Some(stage) {
            return false;
        }
        state.crash = None;
        state.cuts.push(stage.to_owned());
        true
    }) {
        assert!(
            std::env::var_os(CHILD_ENV).is_some(),
            "crash cuts require the dedicated child process"
        );
        std::process::exit(86);
    }
}

pub(crate) fn response_for(request: &HttpRequest) -> Option<crate::ImResult<HttpResponse>> {
    // A dedicated child may never fall through to a real HTTP destination,
    // including when a future routing regression chooses a different origin.
    if let Some(root) = std::env::var_os(CHILD_ENV) {
        let root = PathBuf::from(root);
        let cfg = config(&root);
        if !request.url.starts_with(&format!("https://{}/", cfg.domain)) {
            return Some(Err(crate::ImError::unsupported(
                "root-integration-network-forbidden",
            )));
        }
        return Some(with_state(&root, |state| response(&cfg, state, request)));
    }
    let paths = roots().lock().unwrap().clone();
    for root in paths {
        if !root.join(CONFIG).is_file() {
            continue;
        }
        let cfg = config(&root);
        if !request.url.starts_with(&format!("https://{}/", cfg.domain)) {
            continue;
        }
        return Some(with_state(&root, |state| response(&cfg, state, request)));
    }
    None
}

fn response(
    cfg: &Config,
    state: &mut PeerState,
    request: &HttpRequest,
) -> crate::ImResult<HttpResponse> {
    if request.method == "GET" {
        return Ok(http_json(cfg.document.clone()));
    }
    let rpc: Value = serde_json::from_slice(&request.body).unwrap();
    let params = &rpc["params"];
    let result = match rpc["method"].as_str().unwrap() {
        "device_registry_get" => {
            state.clock += std::mem::take(&mut state.preflight_delay_once);
            state.first_preflight_finished.get_or_insert(state.clock);
            registry(cfg, state)
        }
        "get_me" => {
            state.auth_refreshes += 1;
            let expected = crate::internal::transport::ExpectedDeviceAccessOwned {
                did: cfg.did.clone(),
                user_id: "root-integration-user".into(),
                device_id: cfg.recipient.clone(),
                key_id: cfg.recipient_signing.clone(),
                auth_generation: if state.completed { 2 } else { 1 },
                role: if state.completed {
                    DeviceAuthorizationRole::Admin
                } else {
                    DeviceAuthorizationRole::Member
                },
                management_ready: state.completed,
            };
            serde_json::json!({"did": cfg.did, "user_id": "root-integration-user", "access_token": super::tests::test_access_token(&expected)})
        }
        "device_root_import_complete" => {
            if state.reject_next_completion_auth {
                state.reject_next_completion_auth = false;
                let mut denied = http_json(
                    serde_json::json!({"jsonrpc":"2.0", "id":rpc["id"], "error":{
                        "code":1401, "message":"authentication required", "data":{"awiki_code":"device.root_import.authentication_required"}
                    }}),
                );
                denied.status_code = 401;
                return Ok(denied);
            }
            assert_eq!(
                params["type"],
                if cfg.completion_v2 {
                    ROOT_COMPLETION_V2
                } else {
                    "awiki.device.root-key-import-complete.v1"
                }
            );
            if !cfg.completion_v2 {
                let statement = params["statement"].as_object().unwrap();
                let mut keys: Vec<_> = statement.keys().map(String::as_str).collect();
                keys.sort_unstable();
                // Closed V1 service contract: no delivery/proof timing extensions.
                let mut expected = vec![
                    "type",
                    "message_id",
                    "did",
                    "sending_device_id",
                    "importing_device_id",
                    "sender_e2ee_key_id",
                    "recipient_e2ee_key_id",
                    "root_key_id",
                    "root_public_key_fingerprint",
                    "document_version",
                    "document_hash",
                    "registry_version",
                    "imported_at",
                    "expires_at",
                    "nonce",
                    "proof",
                ];
                expected.sort_unstable();
                assert_eq!(keys, expected);
            }
            assert_eq!(
                params["statement"]["type"],
                if cfg.completion_v2 {
                    "awiki.device.root-possession.v2"
                } else {
                    "awiki.device.root-possession.v1"
                }
            );
            assert_eq!(params["statement"]["did"], cfg.did);
            assert_eq!(params["statement"]["sending_device_id"], cfg.sender);
            assert_eq!(params["statement"]["importing_device_id"], cfg.recipient);
            anp::proof::verify_object_proof(params, &cfg.did, &cfg.document).unwrap();
            anp::proof::verify_object_proof(&params["statement"], &cfg.did, &cfg.document).unwrap();
            assert!(request
                .headers
                .iter()
                .any(|(name, value)| name.eq_ignore_ascii_case("authorization")
                    && value.starts_with("Bearer ")));
            let statement = &params["statement"];
            let imported = statement["imported_at"].as_str().unwrap();
            let created = if cfg.completion_v2 {
                statement["proof_created_at"].as_str().unwrap()
            } else {
                imported
            };
            assert_eq!(params["proof"]["created"], created);
            assert_eq!(statement["proof"]["created"], created);
            let nonce_hash = digest(statement["nonce"].as_str().unwrap().as_bytes());
            if let Some(prior) = &state.nonce_hash {
                assert_eq!(prior, &nonce_hash);
            }
            if let Some(prior) = &state.imported_at {
                assert_eq!(prior, imported);
            }
            state.nonce_hash = Some(nonce_hash);
            state.imported_at = Some(imported.to_owned());
            state.proof_times.push(created.to_owned());
            state.completions += 1;
            let mut intent = params.clone();
            for proof in ["/proof", "/statement/proof"] {
                let proof = intent.pointer_mut(proof).unwrap().as_object_mut().unwrap();
                proof.remove("created");
                proof.remove("proofValue");
            }
            intent["statement"]
                .as_object_mut()
                .unwrap()
                .remove("proof_created_at");
            intent["statement"]
                .as_object_mut()
                .unwrap()
                .remove("expires_at");
            let hash = digest(&serde_json_canonicalizer::to_vec(&intent).unwrap());
            if state.completed {
                assert_eq!(state.intent_hash.as_deref(), Some(hash.as_str()));
            } else {
                let now = OffsetDateTime::from_unix_timestamp(state.clock).unwrap();
                if now
                    > parse_whole_second_time(
                        "expires_at",
                        statement["expires_at"].as_str().unwrap(),
                    )?
                {
                    return Ok(http_json(
                        serde_json::json!({"jsonrpc":"2.0", "id":rpc["id"], "error": {
                            "code": -32000, "message":"expired", "data":{"awiki_code":"device.root_import.expired","retryable":false}
                        }}),
                    ));
                }
                assert_eq!(statement["document_hash"], cfg.document_hash);
                assert_eq!(statement["registry_version"], 2 + state.registry_offset);
                assert!(parse_whole_second_time("created", created)? <= now);
                assert!(
                    parse_whole_second_time("imported", imported)?
                        <= parse_whole_second_time("created", created)?
                );
                state.completed = true;
                state.commits += 1;
                state.intent_hash = Some(hash);
            }
            serde_json::json!({"did":cfg.did, "device_id":cfg.recipient, "role":"admin", "management_ready":true,
                "auth_generation":2, "registry_version":3, "completed_message_id":cfg.metadata.message_id})
        }
        _ => {
            return Err(crate::ImError::unsupported(
                "unexpected-root-integration-rpc",
            ))
        }
    };
    Ok(http_json(
        serde_json::json!({"jsonrpc":"2.0", "id":rpc["id"], "result":result}),
    ))
}

fn registry(cfg: &Config, state: &PeerState) -> Value {
    serde_json::json!({"did":cfg.did,"checkpoint":{"document_version":2,"document_hash":cfg.document_hash,
        "registry_version":2 + state.registry_offset + u64::from(state.completed)},"devices":[
        {"device_id":cfg.sender,"signing_key_id":cfg.sender_signing,"e2ee_key_id":cfg.sender_e2ee,
         "status":"active","role":"admin","management_ready":true,"auth_generation":1},
        {"device_id":cfg.recipient,"signing_key_id":cfg.recipient_signing,"e2ee_key_id":cfg.recipient_e2ee,
         "status":"active","role":if state.completed {"admin"} else {"member"},"management_ready":state.completed,
         "auth_generation":if state.completed {2} else {1}}
    ]})
}

fn http_json(value: Value) -> HttpResponse {
    HttpResponse {
        status_code: 200,
        headers: BTreeMap::new(),
        body: serde_json::to_vec(&value).unwrap(),
    }
}
fn digest(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(bytes))
}

fn paths(root: &Path) -> crate::ImCorePaths {
    crate::ImCorePaths {
        identities: crate::IdentityRegistryPaths {
            identity_root_dir: root.join("identities"),
            registry_path: root.join("identities/index.json"),
            default_identity_path: Some(root.join("identities/default")),
        },
        local_state: crate::LocalStatePaths {
            sqlite_path: root.join("local/im.sqlite"),
        },
        runtime: crate::RuntimePaths {
            cache_dir: root.join("cache"),
            temp_dir: root.join("tmp"),
        },
    }
}

fn open_actor(root: &Path, domain: &str, actor: &str) -> crate::ImCore {
    let directory = root.join(actor);
    let mut cfg = crate::ImCoreConfig::new(
        crate::ServiceEndpoint::parse(format!("https://{domain}")).unwrap(),
        domain,
    )
    .unwrap();
    cfg.transport_policy = crate::MessageTransportPolicy::HttpOnly;
    crate::ImCore::new_with_options(
        cfg,
        paths(&directory),
        crate::ImCoreOpenOptions::default().with_identity_secret_vault(
            crate::IdentitySecretStoragePolicy::VaultRequired,
            crate::ImCoreSecretVaultOptions::new(
                DeviceVaultRootKey::from_bytes([if actor == "source" { 101 } else { 102 }; 32]),
                directory.join("vault"),
                format!("root-integration-{actor}"),
                format!("vault-{actor}"),
            ),
        ),
    )
    .unwrap()
}

struct Fixture {
    root: tempfile::TempDir,
    cfg: Config,
    root_private_pem: zeroize::Zeroizing<String>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        roots()
            .lock()
            .unwrap()
            .retain(|root| root != self.root.path());
    }
}

async fn fixture(delay: i64) -> Fixture {
    fixture_contract(delay, true).await
}

async fn fixture_contract(delay: i64, completion_v2: bool) -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let domain = format!("root-integration-{}.example.test", rand::random::<u64>());
    let a = crate::internal::identity_generation::generate_vnext_handle_identity_with_default_daemon_subkey(&domain,"source",None,None).unwrap();
    let b = crate::internal::identity_generation::generate_vnext_handle_identity_with_default_daemon_subkey(&domain,"receiver",None,None).unwrap();
    let b_signing = b
        .device_signing_key_id
        .replace(b.did.as_str(), a.did.as_str());
    let b_e2ee = b.device_e2ee_key_id.replace(b.did.as_str(), a.did.as_str());
    let mut document = a.did_document.clone();
    for (old, new) in [
        (&b.device_signing_key_id, &b_signing),
        (&b.device_e2ee_key_id, &b_e2ee),
    ] {
        let mut method = b.did_document["verificationMethod"]
            .as_array()
            .unwrap()
            .iter()
            .find(|method| method["id"] == *old)
            .unwrap()
            .clone();
        method["id"] = serde_json::json!(new);
        method["controller"] = serde_json::json!(a.did.as_str());
        document["verificationMethod"]
            .as_array_mut()
            .unwrap()
            .push(method);
    }
    for relation in ["authentication", "assertionMethod"] {
        document[relation]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!(b_signing));
    }
    document["keyAgreement"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!(b_e2ee));
    let mut member = b.did_document["deviceManifest"]["devices"][0].clone();
    member["signing_key_id"] = serde_json::json!(b_signing);
    member["e2ee_key_id"] = serde_json::json!(b_e2ee);
    document["deviceManifest"]["devices"]
        .as_array_mut()
        .unwrap()
        .push(member);
    crate::internal::identity_daemon_subkey::resign_did_document_with_key1(
        &mut document,
        &a.did,
        &a.root_private_pem,
    )
    .unwrap();
    assert!(anp::authentication::validate_did_document_binding(
        &document, true
    ));
    let hash = crate::internal::identity_wire::document::document_hash(&document).unwrap();
    for (actor, generated, signing, e2ee, admin, byte) in [
        (
            "source",
            &a,
            a.device_signing_key_id.clone(),
            a.device_e2ee_key_id.clone(),
            true,
            101u8,
        ),
        (
            "receiver",
            &b,
            b_signing.clone(),
            b_e2ee.clone(),
            false,
            102u8,
        ),
    ] {
        let directory = root.path().join(actor);
        let vault = Arc::new(FileSecretVault::new(
            DeviceVaultRootKey::from_bytes([byte; 32]),
            FileSecretVaultStore::new(directory.join("vault")),
        ));
        let expected = crate::internal::transport::ExpectedDeviceAccessOwned {
            did: a.did.as_str().into(),
            user_id: "root-integration-user".into(),
            device_id: generated.protocol_device_id.as_str().into(),
            key_id: signing.clone(),
            auth_generation: 1,
            role: if admin {
                DeviceAuthorizationRole::Admin
            } else {
                DeviceAuthorizationRole::Member
            },
            management_ready: admin,
        };
        IdentityStore::new(&paths(&directory).identities)
            .save_identity_with_secret_storage(
                SaveIdentityInput {
                    local_alias: actor.into(),
                    did: a.did.clone(),
                    unique_id: a.unique_id.clone(),
                    user_id: "root-integration-user".into(),
                    display_name: actor.into(),
                    handle: "source".into(),
                    full_handle: format!("source.{domain}"),
                    binding_generation: Some("1".into()),
                    jwt_token: super::tests::test_access_token(&expected),
                    did_document: Some(document.clone()),
                    key_mode: SaveIdentityKeyMode::VNext {
                        root_key_id: a.root_key_id.clone(),
                        device_signing_key_id: signing.clone(),
                        device_e2ee_key_id: e2ee.clone(),
                    },
                    device_state: Some(IdentityDeviceState {
                        schema_version: IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
                        mode: IdentityDeviceMode::VNext,
                        authorization: Some(DeviceAuthorizationProjection {
                            protocol_device_id: generated.protocol_device_id.clone(),
                            signing_key_id: signing,
                            e2ee_key_id: e2ee,
                            status: DeviceAuthorizationStatus::Active,
                            role: expected.role,
                            management_ready: admin,
                            auth_generation: 1,
                        }),
                        checkpoint: Some(IdentityInternalCheckpoint {
                            document_version: 2,
                            document_hash: hash.clone(),
                            registry_version: 2,
                        }),
                    }),
                    key1_private_pem: if admin {
                        a.root_private_pem.clone()
                    } else {
                        String::new()
                    },
                    key1_public_pem: a.root_public_pem.clone(),
                    e2ee_signing_private_pem: generated.device_signing_private_pem.clone(),
                    e2ee_agreement_private_pem: generated.device_e2ee_private_pem.clone(),
                    daemon_subkey_package: None,
                    make_default: true,
                },
                SaveIdentitySecretStorage::Vault {
                    workspace_id: format!("root-integration-{actor}"),
                    device_id: format!("vault-{actor}"),
                    vault,
                },
            )
            .unwrap();
    }
    let source = open_actor(root.path(), &domain, "source");
    let receiver = open_actor(root.path(), &domain, "receiver");
    // Legacy vault fixtures must enter native custody before exercising root import.
    for core in [&source, &receiver] {
        let migration = core
            .identities()
            .migrate_identity_custody_async()
            .await
            .unwrap();
        assert_eq!(
            migration.phase,
            crate::IdentityCustodyMigrationPhase::Cleaned
        );
    }
    let source_client = source
        .client(crate::identity::IdentitySelector::Default)
        .unwrap();
    let receiver_client = receiver
        .client(crate::identity::IdentitySelector::Default)
        .unwrap();
    let (mut send, mut receive) =
        crate::internal::secure_direct::v2_runtime::tests::established_pair();
    send.binding.local_did = a.did.as_str().into();
    send.binding.peer_did = a.did.as_str().into();
    send.binding.local_device_id = a.protocol_device_id.as_str().into();
    send.binding.peer_device_id = b.protocol_device_id.as_str().into();
    send.binding.local_e2ee_key_id = a.device_e2ee_key_id.clone();
    send.binding.peer_e2ee_key_id = b_e2ee.clone();
    receive.binding.local_did = a.did.as_str().into();
    receive.binding.peer_did = a.did.as_str().into();
    receive.binding.local_device_id = b.protocol_device_id.as_str().into();
    receive.binding.peer_device_id = a.protocol_device_id.as_str().into();
    receive.binding.local_e2ee_key_id = b_e2ee.clone();
    receive.binding.peer_e2ee_key_id = a.device_e2ee_key_id.clone();
    let issued = OffsetDateTime::now_utc().replace_nanosecond(0).unwrap();
    for (core, client, session) in [
        (&source, &source_client, &send),
        (&receiver, &receiver_client, &receive),
    ] {
        let db = crate::internal::local_state::open_writable(
            &core.inner().sdk_paths().local_state.sqlite_path,
        )
        .unwrap();
        let entry = local_device_entry(core, client).unwrap();
        let scope = V2OwnerScope::from_identity_state(
            &client.current_identity().id,
            client.did(),
            entry.device_state.as_ref().unwrap(),
        )
        .unwrap();
        let store = SqliteV2DirectStateStore::new_with_secret_vault(
            &db,
            core.inner().identity_vault().unwrap().vault(),
            scope,
        )
        .unwrap();
        store
            .commit_inbound(
                session,
                "setup",
                "sha256:setup",
                None,
                V2SessionExpectation::Absent,
                &format_time(issued).unwrap(),
            )
            .unwrap();
    }
    let key = anp::PrivateKeyMaterial::from_pem(&a.root_private_pem).unwrap();
    let anp::PrivateKeyMaterial::Ed25519(key) = key else {
        panic!("root must be Ed25519")
    };
    let der = Zeroizing::new([ED25519_PKCS8_PREFIX.as_slice(), key.to_bytes().as_slice()].concat());
    let root_method = document["verificationMethod"]
        .as_array()
        .unwrap()
        .iter()
        .find(|method| method["id"] == a.root_key_id)
        .unwrap();
    let public =
        crate::internal::identity_wire::document::extract_identity_public_key(root_method).unwrap();
    let fingerprint = anp::authentication::compute_multikey_fingerprint(&public).unwrap();
    let envelope = Zeroizing::new(RootKeyEnvelope {
        completion_contract: completion_v2.then(|| ROOT_COMPLETION_V2.into()),
        system_type: ROOT_KEY_ENVELOPE_SYSTEM_TYPE.into(),
        message_id: "msg-root-key-integration".into(),
        did: a.did.as_str().into(),
        root_key_id: a.root_key_id.clone(),
        root_public_key_fingerprint: format!("e1_{fingerprint}"),
        root_private_key_pkcs8_b64u: URL_SAFE_NO_PAD.encode(der.as_slice()),
        sender_device_id: a.protocol_device_id.as_str().into(),
        sender_e2ee_key_id: a.device_e2ee_key_id.clone(),
        recipient_device_id: b.protocol_device_id.as_str().into(),
        recipient_e2ee_key_id: b_e2ee.clone(),
        document_version: 2,
        document_hash: hash.clone(),
        registry_version: 2,
        issued_at: format_time(issued).unwrap(),
        expires_at: format_time(issued + Duration::seconds(600)).unwrap(),
    });
    let plaintext = V2SecretJsonPayload::from_canonical_json_object(
        serde_json_canonicalizer::to_vec(&*envelope).unwrap(),
    )
    .unwrap();
    let source_entry = local_device_entry(&source, &source_client).unwrap();
    let scope = V2OwnerScope::from_identity_state(
        &source_client.current_identity().id,
        source_client.did(),
        source_entry.device_state.as_ref().unwrap(),
    )
    .unwrap();
    let packet = with_v2_runtime(&source, &scope, |runtime| {
        runtime.prepare_outbound_secret_json(
            &send.binding,
            &envelope.message_id,
            &plaintext,
            &format_time(issued).unwrap(),
        )
    })
    .unwrap();
    let cfg = Config {
        completion_v2,
        domain,
        did: a.did.as_str().into(),
        document,
        document_hash: hash,
        sender: a.protocol_device_id.as_str().into(),
        sender_signing: a.device_signing_key_id.clone(),
        sender_e2ee: a.device_e2ee_key_id.clone(),
        recipient: b.protocol_device_id.as_str().into(),
        recipient_signing: b_signing,
        recipient_e2ee: b_e2ee,
        metadata: packet.metadata.clone(),
        cipher: serde_json::to_value(packet.cipher_body().unwrap()).unwrap(),
        accepted_at: format!(
            "{}.000000Z",
            format_time(issued + Duration::seconds(1))
                .unwrap()
                .trim_end_matches('Z')
        ),
    };
    std::fs::write(root.path().join(CONFIG), serde_json::to_vec(&cfg).unwrap()).unwrap();
    with_state(root.path(), |state| {
        state.clock = issued.unix_timestamp() + delay;
        state.preflight_delay_once = 7;
    });
    register(root.path());
    Fixture {
        root,
        cfg,
        root_private_pem: zeroize::Zeroizing::new(a.root_private_pem),
    }
}

struct Child(std::process::Child);
impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn child(root: &Path) -> Child {
    Child(
        std::process::Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg(format!(
                "{}::core_child",
                module_path!().split_once("::").unwrap().1
            ))
            .arg("--nocapture")
            .env(CHILD_ENV, root)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    )
}
async fn wait_child(child: &mut Child) -> std::process::ExitStatus {
    tokio::time::timeout(std::time::Duration::from_secs(30), async {
        loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                break status;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn core_child() {
    let Some(root) = std::env::var_os(CHILD_ENV) else {
        return;
    };
    let root = PathBuf::from(root);
    let cfg = config(&root);
    register(&root);
    let core = open_actor(&root, &cfg.domain, "receiver");
    let client = core
        .client(crate::identity::IdentitySelector::Default)
        .unwrap();
    if import_coordinator_exists(&core, &client, &cfg.metadata.message_id).unwrap() {
        recover_root_import_completions(&client).await.unwrap();
    } else {
        let body = V2DirectBody::Cipher(serde_json::from_value(cfg.cipher.clone()).unwrap());
        let delivery = TrustedDirectDeliveryContext::from_stored_message(
            &cfg.metadata,
            Some(cfg.accepted_at.clone()),
            TrustedDirectDeliverySource::Mailbox,
        )
        .unwrap();
        receive_root_envelope_candidate(&core, &client, &cfg.metadata, &body, &delivery, None)
            .await
            .unwrap();
    }
}

async fn assert_promoted(fixture: &Fixture) {
    let core = open_actor(fixture.root.path(), &fixture.cfg.domain, "receiver");
    let client = core
        .client(crate::identity::IdentitySelector::Default)
        .unwrap();
    assert_eq!(
        client
            .runtime()
            .identity_session
            .as_ref()
            .unwrap()
            .host_status()
            .await
            .unwrap()
            .root_capability,
        crate::internal::identity_provider::ProviderRootCapability::Active
    );
    let entry = local_device_entry(&core, &client).unwrap();
    let auth = entry.device_state.unwrap().authorization.unwrap();
    assert_eq!(auth.role, DeviceAuthorizationRole::Admin);
    assert!(auth.management_ready);
    assert_eq!(auth.auth_generation, 2);
    let db = crate::internal::local_state::open_writable(
        &core.inner().sdk_paths().local_state.sqlite_path,
    )
    .unwrap();
    let (phase, count): (String, i64) = db
        .query_row(
            "SELECT phase,COUNT(*) FROM identity_root_import_completion_v1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(phase, "promoted");
    assert_eq!(count, 1);
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM messages", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    with_state(fixture.root.path(), |state| {
        assert!(state.completed);
        assert_eq!(state.commits, 1);
    });
}

#[tokio::test]
async fn completed_root_recovery_preserves_later_device_publications() {
    let fixture = fixture(1).await;
    assert!(wait_child(&mut child(fixture.root.path())).await.success());
    assert_promoted(&fixture).await;
    let core = open_actor(fixture.root.path(), &fixture.cfg.domain, "receiver");
    let client = core
        .client(crate::identity::IdentitySelector::Default)
        .unwrap();
    let entry = local_device_entry(&core, &client).unwrap();
    let mut state = entry.device_state.clone().unwrap();
    let mut checkpoint = state.checkpoint.clone().unwrap();
    let mut document = fixture.cfg.document.clone();
    document["alsoKnownAs"] = serde_json::json!(["https://example.test/after-device-join"]);
    crate::internal::identity_daemon_subkey::resign_did_document_with_key1(
        &mut document,
        client.did(),
        &fixture.root_private_pem,
    )
    .unwrap();
    checkpoint.document_version += 1;
    checkpoint.registry_version += 1;
    checkpoint.document_hash =
        crate::internal::identity_wire::document::document_hash(&document).unwrap();
    crate::internal::identity_custody::adopt_sibling_controller_document_async(
        &core,
        client.did(),
        entry.anp_identity_store_id.as_deref().unwrap(),
        entry.anp_identity_id.as_deref().unwrap(),
        &document,
        &checkpoint,
    )
    .await
    .unwrap();
    let store = IdentityStore::new(&core.inner().sdk_paths().identities);
    let alias = client
        .current_identity()
        .local_alias
        .as_deref()
        .unwrap_or(client.current_identity().id.as_str());
    state.checkpoint = Some(checkpoint.clone());
    store.save_did_document(&entry.dir_name, &document).unwrap();
    store.save_device_state(alias, state.clone()).unwrap();

    // A completed import is still replayed by secure Inbox hydration. Replaying
    // it must preserve a later, verified sibling publication and active custody.
    for _ in 0..2 {
        recover_root_import_completions(&client).await.unwrap();
        let current = local_device_entry(&core, &client).unwrap();
        assert_eq!(
            current.device_state.unwrap().checkpoint,
            Some(checkpoint.clone())
        );
        assert_eq!(
            client.runtime().key_provider.did_document().unwrap(),
            document
        );
    }
    assert_promoted(&fixture).await;

    // A forward Registry counter does not authorize a same-version document
    // replacement, nor may a newer document hide a Registry rollback.
    for invalid in [
        IdentityInternalCheckpoint {
            document_version: checkpoint.document_version - 1,
            ..checkpoint.clone()
        },
        IdentityInternalCheckpoint {
            registry_version: checkpoint.registry_version - 2,
            ..checkpoint.clone()
        },
    ] {
        let mut invalid_state = state.clone();
        invalid_state.checkpoint = Some(invalid);
        store
            .save_device_state(alias, invalid_state.clone())
            .unwrap();
        assert!(recover_root_import_completions(&client).await.is_err());
        assert_eq!(
            local_device_entry(&core, &client).unwrap().device_state,
            Some(invalid_state)
        );
    }

    // The historical import must never reactivate a device whose current
    // authorization is revoked, even if its old root is still locally present.
    state.authorization.as_mut().unwrap().status = DeviceAuthorizationStatus::Revoked;
    state.authorization.as_mut().unwrap().management_ready = false;
    store.save_device_state(alias, state.clone()).unwrap();
    assert!(recover_root_import_completions(&client).await.is_err());
    assert_eq!(
        local_device_entry(&core, &client).unwrap().device_state,
        Some(state)
    );
}

fn persisted_import_time(fixture: &Fixture) -> Option<String> {
    let db = rusqlite::Connection::open_with_flags(
        paths(&fixture.root.path().join("receiver"))
            .local_state
            .sqlite_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let json: Option<String> = db
        .query_row(
            "SELECT plan_json FROM identity_root_import_plan_v2",
            [],
            |row| row.get(0),
        )
        .optional()
        .unwrap();
    json.map(|json| {
        serde_json::from_str::<RootImportSealedPlan>(&json)
            .unwrap()
            .imported_at
    })
}

#[tokio::test]
async fn production_receive_survives_real_process_crashes_and_late_completion() {
    for (cut, delay) in [
        ("before_plan", 900),
        ("after_plan", 900),
        ("after_provider", 86400),
        ("after_handoff", 900),
        ("after_proof", 86400),
        ("after_completion_request", 900),
    ] {
        let fixture = fixture(delay).await;
        let cipher_digest = digest(&serde_json::to_vec(&fixture.cfg.cipher).unwrap());
        let started_clock = with_state(fixture.root.path(), |state| {
            state.crash = Some(cut.into());
            state.clock
        });
        let mut first = child(fixture.root.path());
        assert_eq!(wait_child(&mut first).await.code(), Some(86));
        let first_import = persisted_import_time(&fixture);
        with_state(fixture.root.path(), |state| {
            assert_eq!(state.first_preflight_finished, Some(started_clock + 7));
        });
        if cut == "before_plan" {
            assert!(first_import.is_none());
        } else {
            let expected =
                format_time(OffsetDateTime::from_unix_timestamp(started_clock + 7).unwrap())
                    .unwrap();
            assert_eq!(first_import.as_deref(), Some(expected.as_str()));
        }
        let resumed_clock = with_state(fixture.root.path(), |state| {
            assert_eq!(state.cuts, vec![cut]);
            state.clock += 86400;
            state.reject_next_completion_auth =
                matches!(cut, "after_proof" | "after_completion_request");
            state.clock
        });
        let mut resumed = child(fixture.root.path());
        assert!(wait_child(&mut resumed).await.success());
        assert_promoted(&fixture).await;
        let expected_import = first_import.unwrap_or_else(|| {
            format_time(OffsetDateTime::from_unix_timestamp(resumed_clock).unwrap()).unwrap()
        });
        assert_eq!(
            persisted_import_time(&fixture).as_deref(),
            Some(expected_import.as_str())
        );
        with_state(fixture.root.path(), |state| {
            assert_eq!(state.imported_at.as_deref(), Some(expected_import.as_str()))
        });
        assert_eq!(
            digest(&serde_json::to_vec(&config(fixture.root.path()).cipher).unwrap()),
            cipher_digest
        );
        if matches!(cut, "after_proof" | "after_completion_request") {
            with_state(fixture.root.path(), |state| {
                assert!(state.auth_refreshes > 0)
            });
        }
        if cut == "after_proof" {
            with_state(fixture.root.path(), |state| {
                assert_eq!(state.completions, 2);
                assert_ne!(state.proof_times[0], state.proof_times[1]);
            });
        }
    }
}

#[tokio::test]
async fn two_processes_resume_the_same_pending_root_and_refresh_once() {
    let fixture = fixture(86400).await;
    with_state(fixture.root.path(), |state| {
        state.crash = Some("after_proof".into())
    });
    let mut first = child(fixture.root.path());
    assert_eq!(wait_child(&mut first).await.code(), Some(86));
    with_state(fixture.root.path(), |state| state.clock += 86400);
    let mut one = child(fixture.root.path());
    let mut two = child(fixture.root.path());
    assert!(wait_child(&mut one).await.success());
    assert!(wait_child(&mut two).await.success());
    assert_promoted(&fixture).await;
    with_state(fixture.root.path(), |state| {
        assert_eq!(state.completions, 2)
    });
}

#[tokio::test]
async fn delayed_receive_keeps_strict_checkpoint_and_never_imports_on_drift() {
    let fixture = fixture(86400).await;
    with_state(fixture.root.path(), |state| state.registry_offset = 1);
    let mut receiver = child(fixture.root.path());
    assert!(wait_child(&mut receiver).await.success());
    let core = open_actor(fixture.root.path(), &fixture.cfg.domain, "receiver");
    let client = core
        .client(crate::identity::IdentitySelector::Default)
        .unwrap();
    assert_eq!(
        client
            .runtime()
            .identity_session
            .as_ref()
            .unwrap()
            .host_status()
            .await
            .unwrap()
            .root_capability,
        crate::internal::identity_provider::ProviderRootCapability::Absent
    );
    let db = crate::internal::local_state::open_writable(
        &core.inner().sdk_paths().local_state.sqlite_path,
    )
    .unwrap();
    for table in [
        "identity_root_import_plan_v2",
        "identity_root_import_completion_v1",
        "messages",
    ] {
        assert_eq!(
            db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    with_state(fixture.root.path(), |state| {
        assert_eq!(state.completions, 0);
        assert_eq!(state.commits, 0);
    });
}

#[tokio::test]
async fn v1_receive_promotes_with_a_strict_v1_only_service_contract() {
    let fixture = fixture_contract(2, false).await;
    let mut receiver = child(fixture.root.path());
    assert!(wait_child(&mut receiver).await.success());
    assert_promoted(&fixture).await;
    assert!(persisted_import_time(&fixture).is_none());
    with_state(fixture.root.path(), |state| {
        assert_eq!(state.completions, 1);
        assert_eq!(state.commits, 1);
    });
}
