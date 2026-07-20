use super::*;
use crate::internal::identity_device_state::{
    DeviceAuthorizationProjection, IdentityDeviceState, IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
};
use crate::internal::identity_generation::GeneratedVNextIdentityWithDaemonSubkey;
use crate::internal::identity_store::{SaveIdentityInput, SaveIdentityKeyMode};
use crate::internal::key_provider::KeyMaterialProvider;
use crate::internal::platform_secret::DeviceVaultRootKey;
use crate::internal::secret_vault::{FileSecretVault, FileSecretVaultStore};
use serde_json::json;
use x25519_dalek::StaticSecret as X25519StaticSecret;

const LOCAL_ALIAS: &str = "alice";
const MESSAGE_ID: &str = "root-message-123";

struct LocalVaultIdentity {
    _root: tempfile::TempDir,
    paths: crate::paths::IdentityRegistryPaths,
    storage: SaveIdentitySecretStorage,
    vault: Arc<FileSecretVault>,
}

struct Scenario {
    generated: GeneratedVNextIdentityWithDaemonSubkey,
    document: Value,
    registry: DeviceJoinRemoteRegistry,
    sender: LocalVaultIdentity,
    receiver: LocalVaultIdentity,
    recipient_device_id: String,
    recipient_signing_key_id: String,
    recipient_e2ee_key_id: String,
    recipient_signing_private_pem: String,
    recipient_e2ee_private_pem: String,
    now: OffsetDateTime,
    expires_at: OffsetDateTime,
}

impl Scenario {
    fn sender_core(&self, enabled: bool) -> RootKeyTransferCore<'_> {
        RootKeyTransferCore::for_test(&self.sender.paths, self.sender.storage.clone(), enabled)
    }

    fn receiver_core(&self, enabled: bool) -> RootKeyTransferCore<'_> {
        RootKeyTransferCore::for_test(&self.receiver.paths, self.receiver.storage.clone(), enabled)
    }

    fn prepare(&self) -> PreparedRootKeyEnvelope {
        self.sender_core(true)
            .prepare_envelope(RootKeyEnvelopePrepareInput {
                local_alias: LOCAL_ALIAS,
                did_document: &self.document,
                registry: &self.registry,
                recipient_device_id: &self.recipient_device_id,
                message_id: MESSAGE_ID,
                user_presence_at: self.now,
                now: self.now,
                expires_at: self.expires_at,
            })
            .unwrap()
    }

    fn direct_binding(&self) -> RootControlDirectBinding {
        self.direct_binding_for(
            MESSAGE_ID,
            self.generated.protocol_device_id.as_str(),
            &self.generated.device_e2ee_key_id,
            7,
        )
    }

    fn direct_binding_for(
        &self,
        message_id: &str,
        peer_device_id: &str,
        peer_e2ee_key_id: &str,
        session_byte: u8,
    ) -> RootControlDirectBinding {
        RootControlDirectBinding {
            message_id: message_id.to_owned(),
            session_id: URL_SAFE_NO_PAD.encode([session_byte; 16]),
            session_binding: anp::direct_e2ee::V2SessionBinding {
                local_did: self.generated.did.as_str().to_owned(),
                local_device_id: self.recipient_device_id.clone(),
                peer_did: self.generated.did.as_str().to_owned(),
                peer_device_id: peer_device_id.to_owned(),
                suite: anp::direct_e2ee::MTI_DIRECT_E2EE_SUITE_V2.to_owned(),
                local_e2ee_key_id: self.recipient_e2ee_key_id.clone(),
                peer_e2ee_key_id: peer_e2ee_key_id.to_owned(),
            },
        }
    }

    fn import(
        &self,
        prepared: &PreparedRootKeyEnvelope,
        direct: &RootControlDirectBinding,
        context: &RootImportTransportContext,
        current_document: &Value,
        registry: &DeviceJoinRemoteRegistry,
        now: OffsetDateTime,
    ) -> crate::ImResult<ImportedRootKeyAck> {
        self.receiver_core(true).import_envelope(
            prepared.plaintext(),
            RootKeyEnvelopeImportInput {
                local_alias: LOCAL_ALIAS,
                direct_binding: direct,
                transport_context: context,
                current_did_document: current_document,
                current_registry: registry,
                now,
            },
        )
    }

    fn receiver_save_input(&self) -> SaveIdentityInput {
        let state = IdentityStore::new(&self.receiver.paths)
            .load_index()
            .unwrap()
            .credentials[LOCAL_ALIAS]
            .device_state
            .clone();
        SaveIdentityInput {
            local_alias: LOCAL_ALIAS.to_owned(),
            did: self.generated.did.clone(),
            unique_id: "receiver-identity".to_owned(),
            user_id: "user-1".to_owned(),
            display_name: "Alice".to_owned(),
            handle: "alice".to_owned(),
            full_handle: "alice.awiki.test".to_owned(),
            jwt_token: "device-token".to_owned(),
            did_document: Some(self.document.clone()),
            key_mode: SaveIdentityKeyMode::VNext {
                root_key_id: self.generated.root_key_id.clone(),
                device_signing_key_id: self.recipient_signing_key_id.clone(),
                device_e2ee_key_id: self.recipient_e2ee_key_id.clone(),
            },
            device_state: state,
            key1_private_pem: String::new(),
            key1_public_pem: self.generated.root_public_pem.clone(),
            e2ee_signing_private_pem: self.recipient_signing_private_pem.clone(),
            e2ee_agreement_private_pem: self.recipient_e2ee_private_pem.clone(),
            daemon_subkey_package: None,
            make_default: true,
        }
    }
}

#[test]
fn rollout_gate_defaults_off_before_root_is_opened() {
    let scenario = scenario();
    let error = scenario
        .sender_core(false)
        .prepare_envelope(RootKeyEnvelopePrepareInput {
            local_alias: LOCAL_ALIAS,
            did_document: &scenario.document,
            registry: &scenario.registry,
            recipient_device_id: &scenario.recipient_device_id,
            message_id: MESSAGE_ID,
            user_presence_at: scenario.now,
            now: scenario.now,
            expires_at: scenario.expires_at,
        })
        .unwrap_err();

    assert_eq!(
        error,
        crate::ImError::UnsupportedCapability {
            capability: "awiki-root-key-transfer-disabled".to_owned()
        }
    );

    let prepared = scenario.prepare();
    let import_error = scenario
        .receiver_core(false)
        .import_envelope(
            prepared.plaintext(),
            RootKeyEnvelopeImportInput {
                local_alias: LOCAL_ALIAS,
                direct_binding: &scenario.direct_binding(),
                transport_context: prepared.transport_context(),
                current_did_document: &scenario.document,
                current_registry: &scenario.registry,
                now: scenario.now,
            },
        )
        .unwrap_err();
    assert_eq!(
        import_error,
        crate::ImError::UnsupportedCapability {
            capability: "awiki-root-key-transfer-disabled".to_owned()
        }
    );
    assert!(IdentityStore::new(&scenario.receiver.paths)
        .load_index()
        .unwrap()
        .credentials[LOCAL_ALIAS]
        .root_key_import
        .is_none());
}

#[test]
fn transport_expiry_compares_rfc3339_instants_not_spelling() {
    let scenario = scenario();
    let prepared = scenario.prepare();
    let envelope: RootKeyEnvelope =
        serde_json::from_slice(prepared.plaintext().expose_secret()).unwrap();
    let mut context = prepared.transport_context().clone();

    assert!(context.expires_at.ends_with('Z'));
    context.expires_at = context.expires_at.trim_end_matches('Z').to_owned() + "+00:00";
    context.validate_for_envelope(&envelope).unwrap();
}

#[test]
fn sender_expiry_survives_postgres_precision_round_trip_without_relaxing_instant() {
    let scenario = scenario();
    let expires_at = scenario.expires_at.replace_nanosecond(987_654_321).unwrap();
    let prepared = scenario
        .sender_core(true)
        .prepare_envelope(RootKeyEnvelopePrepareInput {
            local_alias: LOCAL_ALIAS,
            did_document: &scenario.document,
            registry: &scenario.registry,
            recipient_device_id: &scenario.recipient_device_id,
            message_id: MESSAGE_ID,
            user_presence_at: scenario.now,
            now: scenario.now,
            expires_at,
        })
        .unwrap();
    let envelope: RootKeyEnvelope =
        serde_json::from_slice(prepared.plaintext().expose_secret()).unwrap();
    let canonical = parse_time("envelope.expires_at", &envelope.expires_at).unwrap();
    assert_eq!(canonical.nanosecond(), 0);

    let mut context = prepared.transport_context().clone();
    context.expires_at = format!("{}.000000+00:00", envelope.expires_at.trim_end_matches('Z'));
    context.validate_for_envelope(&envelope).unwrap();

    context.expires_at = format_time(canonical + Duration::microseconds(1)).unwrap();
    assert_eq!(
        context.validate_for_envelope(&envelope).unwrap_err(),
        crate::ImError::PermissionDenied
    );
}

#[test]
fn prepare_envelope_requires_ready_admin_recent_presence_and_redacts_debug() {
    let scenario = scenario();
    let prepared = scenario.prepare();
    let envelope: RootKeyEnvelope = decode_strict_json(prepared.plaintext()).unwrap();

    assert_eq!(envelope.system_type, ROOT_KEY_ENVELOPE_SYSTEM_TYPE);
    assert_eq!(envelope.message_id, MESSAGE_ID);
    assert_eq!(envelope.did, scenario.generated.did.as_str());
    assert_eq!(envelope.root_key_id, scenario.generated.root_key_id);
    let validated_root = validate_root_document(
        &scenario.document,
        &scenario.generated.did,
        &scenario.generated.root_key_id,
    )
    .unwrap();
    assert!(validate_root_private(&envelope.root_private_key, &validated_root).is_ok());
    assert_eq!(
        prepared.transport_context(),
        &RootImportTransportContext {
            message_id: MESSAGE_ID.to_owned(),
            delivery_class: ROOT_KEY_CONTROL_DELIVERY_CLASS.to_owned(),
            sender_device_id: scenario.generated.protocol_device_id.as_str().to_owned(),
            recipient_device_id: scenario.recipient_device_id.clone(),
            expires_at: format_time(scenario.expires_at).unwrap(),
        }
    );
    let debug = format!("{prepared:?}");
    assert!(!debug.contains("BEGIN PRIVATE KEY"));
    assert!(!debug.contains(&scenario.generated.root_private_pem));

    let stale_presence = scenario
        .sender_core(true)
        .prepare_envelope(RootKeyEnvelopePrepareInput {
            local_alias: LOCAL_ALIAS,
            did_document: &scenario.document,
            registry: &scenario.registry,
            recipient_device_id: &scenario.recipient_device_id,
            message_id: "root-stale-presence",
            user_presence_at: scenario.now - Duration::minutes(6),
            now: scenario.now,
            expires_at: scenario.expires_at,
        })
        .unwrap_err();
    assert_eq!(stale_presence, crate::ImError::PermissionDenied);
    let maximum_time = time::PrimitiveDateTime::MAX.assume_utc();
    assert_eq!(
        validate_user_presence(maximum_time, maximum_time).unwrap_err(),
        crate::ImError::PermissionDenied
    );

    let mut sender_not_ready = scenario.registry.clone();
    sender_not_ready.devices[0].management_ready = false;
    let error = scenario
        .sender_core(true)
        .prepare_envelope(RootKeyEnvelopePrepareInput {
            local_alias: LOCAL_ALIAS,
            did_document: &scenario.document,
            registry: &sender_not_ready,
            recipient_device_id: &scenario.recipient_device_id,
            message_id: "root-sender-not-ready",
            user_presence_at: scenario.now,
            now: scenario.now,
            expires_at: scenario.expires_at,
        })
        .unwrap_err();
    assert_eq!(error, crate::ImError::PermissionDenied);
}

#[test]
fn import_seals_root_and_emits_one_identical_signed_completion() {
    let scenario = scenario();
    let prepared = scenario.prepare();
    let context = prepared.transport_context().clone();
    let direct = scenario.direct_binding();

    let imported = scenario
        .import(
            &prepared,
            &direct,
            &context,
            &scenario.document,
            &scenario.registry,
            scenario.now + Duration::seconds(10),
        )
        .unwrap();

    assert!(!imported.replayed());
    assert_eq!(imported.transport_context(), &context);
    assert_eq!(
        imported.completion().completion_type,
        ROOT_KEY_IMPORTED_SYSTEM_TYPE
    );
    assert_eq!(imported.completion().ack_for_message_id, MESSAGE_ID);
    assert_eq!(
        imported.completion().root_public_key_fingerprint,
        scenario.generated.did.as_str().rsplit(':').next().unwrap()
    );
    let inner: RootKeyImportedInner = decode_strict_json(imported.plaintext()).unwrap();
    assert_eq!(inner.system_type, ROOT_KEY_IMPORTED_SYSTEM_TYPE);
    assert_eq!(inner.completion, *imported.completion());
    let importer =
        unique_registry_device(&scenario.registry, &scenario.recipient_device_id).unwrap();
    verify_completion_signature(imported.completion(), &scenario.document, importer).unwrap();

    let store = IdentityStore::new(&scenario.receiver.paths);
    let index = store.load_index().unwrap();
    assert_eq!(index.schema_version, 5);
    let entry = &index.credentials[LOCAL_ALIAS];
    let root_ref = entry
        .vault_migration
        .as_ref()
        .unwrap()
        .vnext_refs
        .as_ref()
        .unwrap()
        .did_document_root_private
        .as_ref()
        .unwrap();
    assert_eq!(root_ref.kind, SecretKind::IdentityRootPrivate);
    assert_eq!(root_ref.key_id, scenario.generated.root_key_id);
    assert!(scenario.vault_root_matches(
        &scenario.receiver,
        root_ref,
        scenario.generated.root_private_pem.as_bytes()
    ));
    assert!(
        !entry
            .device_state
            .as_ref()
            .unwrap()
            .authorization
            .as_ref()
            .unwrap()
            .management_ready
    );
    assert_eq!(
        entry.root_key_import.as_ref().unwrap().completion,
        *imported.completion()
    );
    assert!(!scenario
        .receiver
        .paths
        .identity_root_dir
        .join("receiver-identity")
        .join(ROOT_IMPORT_PENDING_FILE)
        .exists());

    let registry_text = std::fs::read_to_string(&scenario.receiver.paths.registry_path).unwrap();
    assert!(!registry_text.contains("BEGIN PRIVATE KEY"));
    assert!(!registry_text.contains(&scenario.generated.root_private_pem));
    let vault_record =
        std::fs::read_to_string(scenario.receiver.vault.store().record_path(root_ref)).unwrap();
    assert!(!vault_record.contains("BEGIN PRIVATE KEY"));
    assert!(!vault_record.contains(&scenario.generated.root_private_pem));
    let debug = format!("{imported:?}");
    assert!(!debug.contains("BEGIN PRIVATE KEY"));
    assert!(!debug.contains(&scenario.generated.root_private_pem));
}

#[test]
fn ack_retry_canonicalizes_projected_expiry_to_the_persisted_reservation() {
    let scenario = scenario();
    let prepared = scenario.prepare();
    let mut projected_context = prepared.transport_context().clone();
    projected_context.expires_at = format!(
        "{}.000000+00:00",
        projected_context.expires_at.trim_end_matches('Z')
    );

    let imported = scenario
        .import(
            &prepared,
            &scenario.direct_binding(),
            &projected_context,
            &scenario.document,
            &scenario.registry,
            scenario.now + Duration::seconds(10),
        )
        .unwrap();
    assert_eq!(imported.transport_context(), prepared.transport_context());

    let resumed = scenario
        .receiver_core(true)
        .resume_imported_ack(RootKeyAckResumeInput {
            local_alias: LOCAL_ALIAS,
            message_id: MESSAGE_ID,
            current_did_document: &scenario.document,
            current_registry: &scenario.registry,
        })
        .unwrap();
    assert_eq!(resumed.transport_context(), imported.transport_context());
    assert_eq!(resumed.completion(), imported.completion());
    assert_eq!(
        resumed.plaintext().expose_secret(),
        imported.plaintext().expose_secret()
    );
}

#[test]
fn fresh_import_accepts_newer_current_document_without_historical_snapshot() {
    let scenario = scenario();
    let prepared = scenario.prepare();
    let context = prepared.transport_context().clone();
    let direct = scenario.direct_binding();
    let (current_document, current_registry, _) = add_ready_admin(&scenario, "dev-later-admin");

    let imported = scenario
        .import(
            &prepared,
            &direct,
            &context,
            &current_document,
            &current_registry,
            scenario.now + Duration::seconds(10),
        )
        .unwrap();

    assert!(!imported.replayed());
    assert_eq!(imported.completion().document_version, 2);
    assert_eq!(
        IdentityStore::new(&scenario.receiver.paths)
            .load_index()
            .unwrap()
            .credentials[LOCAL_ALIAS]
            .device_state
            .as_ref()
            .unwrap()
            .checkpoint
            .as_ref()
            .unwrap(),
        &current_registry.checkpoint
    );
}

#[test]
fn imported_root_resave_is_rootless_same_identity_only_and_concurrency_safe() {
    let scenario = scenario();
    let prepared = scenario.prepare();
    scenario
        .import(
            &prepared,
            &scenario.direct_binding(),
            prepared.transport_context(),
            &scenario.document,
            &scenario.registry,
            scenario.now + Duration::seconds(5),
        )
        .unwrap();
    let store = IdentityStore::new(&scenario.receiver.paths);
    let baseline = store.load_index().unwrap();
    let baseline_entry = &baseline.credentials[LOCAL_ALIAS];
    let baseline_root_ref = baseline_entry
        .vault_migration
        .as_ref()
        .unwrap()
        .vnext_refs
        .as_ref()
        .unwrap()
        .did_document_root_private
        .as_ref()
        .unwrap()
        .clone();
    let baseline_import = baseline_entry.root_key_import.clone().unwrap();
    let root_record_path = scenario
        .receiver
        .vault
        .store()
        .record_path(&baseline_root_ref);
    let root_record_before = std::fs::read(&root_record_path).unwrap();

    for _ in 0..2 {
        store
            .save_identity_with_secret_storage(
                scenario.receiver_save_input(),
                scenario.receiver.storage.clone(),
            )
            .unwrap();
    }
    let sequential = store.load_index().unwrap();
    let sequential_entry = &sequential.credentials[LOCAL_ALIAS];
    assert_eq!(
        sequential_entry
            .vault_migration
            .as_ref()
            .unwrap()
            .vnext_refs
            .as_ref()
            .unwrap()
            .did_document_root_private
            .as_ref()
            .unwrap(),
        &baseline_root_ref
    );
    assert_eq!(
        sequential_entry.root_key_import.as_ref().unwrap(),
        &baseline_import
    );
    assert!(std::fs::read(&root_record_path).unwrap() == root_record_before);

    let barrier = Arc::new(std::sync::Barrier::new(3));
    let mut workers = Vec::new();
    for _ in 0..2 {
        let paths = scenario.receiver.paths.clone();
        let storage = scenario.receiver.storage.clone();
        let input = scenario.receiver_save_input();
        let barrier = barrier.clone();
        workers.push(std::thread::spawn(move || {
            barrier.wait();
            IdentityStore::new(&paths).save_identity_with_secret_storage(input, storage)
        }));
    }
    barrier.wait();
    for worker in workers {
        worker.join().unwrap().unwrap();
    }
    let concurrent = store.load_index().unwrap();
    let concurrent_entry = &concurrent.credentials[LOCAL_ALIAS];
    assert_eq!(
        concurrent_entry
            .vault_migration
            .as_ref()
            .unwrap()
            .vnext_refs
            .as_ref()
            .unwrap()
            .did_document_root_private
            .as_ref()
            .unwrap(),
        &baseline_root_ref
    );
    assert_eq!(
        concurrent_entry.root_key_import.as_ref().unwrap(),
        &baseline_import
    );
    assert!(std::fs::read(&root_record_path).unwrap() == root_record_before);
    let all_vault_records_before = vault_record_images(&scenario.receiver.vault);
    let identity_file = scenario
        .receiver
        .paths
        .identity_root_dir
        .join("receiver-identity")
        .join("identity.json");
    let identity_before = std::fs::read(&identity_file).unwrap();

    let mut forbidden_inputs = Vec::new();
    let mut same_root_bytes = scenario.receiver_save_input();
    same_root_bytes.key1_private_pem = scenario.generated.root_private_pem.clone();
    forbidden_inputs.push((same_root_bytes, scenario.receiver.storage.clone()));
    let mut different_root = scenario.receiver_save_input();
    different_root.key1_private_pem = anp::PrivateKeyMaterial::Ed25519(
        ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng),
    )
    .to_pem();
    forbidden_inputs.push((different_root, scenario.receiver.storage.clone()));
    let mut renamed = scenario.receiver_save_input();
    renamed.local_alias = "alice-renamed".to_owned();
    forbidden_inputs.push((renamed, scenario.receiver.storage.clone()));
    let mut renormalized = scenario.receiver_save_input();
    renormalized.local_alias = "Alice".to_owned();
    forbidden_inputs.push((renormalized, scenario.receiver.storage.clone()));
    let mut changed_unique = scenario.receiver_save_input();
    changed_unique.unique_id = "receiver-identity-changed".to_owned();
    forbidden_inputs.push((changed_unique, scenario.receiver.storage.clone()));
    let mut changed_did = scenario.receiver_save_input();
    changed_did.did = crate::ids::Did::parse("did:example:different").unwrap();
    changed_did.device_state = None;
    forbidden_inputs.push((changed_did, scenario.receiver.storage.clone()));
    let mut changed_signing_secret = scenario.receiver_save_input();
    changed_signing_secret.e2ee_signing_private_pem = anp::PrivateKeyMaterial::Ed25519(
        ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng),
    )
    .to_pem();
    forbidden_inputs.push((changed_signing_secret, scenario.receiver.storage.clone()));
    let mut changed_e2ee_secret = scenario.receiver_save_input();
    changed_e2ee_secret.e2ee_agreement_private_pem =
        anp::PrivateKeyMaterial::X25519(X25519StaticSecret::random_from_rng(rand::rngs::OsRng))
            .to_pem();
    forbidden_inputs.push((changed_e2ee_secret, scenario.receiver.storage.clone()));
    let mut changed_token = scenario.receiver_save_input();
    changed_token.jwt_token = "different-device-token".to_owned();
    forbidden_inputs.push((changed_token, scenario.receiver.storage.clone()));
    let wrong_context = SaveIdentitySecretStorage::Vault {
        workspace_id: "different-workspace".to_owned(),
        device_id: "vault-context-receiver-identity".to_owned(),
        vault: scenario.receiver.vault.clone(),
    };
    forbidden_inputs.push((scenario.receiver_save_input(), wrong_context));

    for (input, storage) in forbidden_inputs {
        let rejected = store.save_identity_with_secret_storage(input, storage);
        assert_eq!(rejected.unwrap_err(), crate::ImError::PermissionDenied);
        assert!(std::fs::read(&root_record_path).unwrap() == root_record_before);
        assert!(vault_record_images(&scenario.receiver.vault) == all_vault_records_before);
        assert!(std::fs::read(&identity_file).unwrap() == identity_before);
    }
    let file_compat = store.save_identity(scenario.receiver_save_input());
    assert_eq!(file_compat.unwrap_err(), crate::ImError::PermissionDenied);
    assert!(std::fs::read(&root_record_path).unwrap() == root_record_before);

    let mut dir_drift = store.load_index().unwrap();
    dir_drift.credentials.get_mut(LOCAL_ALIAS).unwrap().dir_name =
        "unexpected-imported-dir".to_owned();
    let lock = store.lock_index_mutation().unwrap();
    store.save_index_locked(&lock, dir_drift).unwrap();
    drop(lock);
    let mut would_mutate_file = scenario.receiver_save_input();
    would_mutate_file.display_name = "Must Not Be Written".to_owned();
    let rejected = store
        .save_identity_with_secret_storage(would_mutate_file, scenario.receiver.storage.clone());
    assert!(rejected.is_err());
    assert!(std::fs::read(&identity_file).unwrap() == identity_before);
    assert!(std::fs::read(&root_record_path).unwrap() == root_record_before);
    assert!(vault_record_images(&scenario.receiver.vault) == all_vault_records_before);
}

#[test]
fn exact_replay_after_expiry_and_sender_revoke_reuses_completion_and_vault_record() {
    let scenario = scenario();
    let prepared = scenario.prepare();
    let context = prepared.transport_context().clone();
    let direct = scenario.direct_binding();
    let first = scenario
        .import(
            &prepared,
            &direct,
            &context,
            &scenario.document,
            &scenario.registry,
            scenario.now + Duration::seconds(5),
        )
        .unwrap();
    let index = IdentityStore::new(&scenario.receiver.paths)
        .load_index()
        .unwrap();
    let root_ref = index.credentials[LOCAL_ALIAS]
        .vault_migration
        .as_ref()
        .unwrap()
        .vnext_refs
        .as_ref()
        .unwrap()
        .did_document_root_private
        .as_ref()
        .unwrap()
        .clone();
    let record_path = scenario.receiver.vault.store().record_path(&root_ref);
    let record_before = std::fs::read(&record_path).unwrap();

    let mut converged_registry = scenario.registry.clone();
    converged_registry.devices[0].status = DeviceAuthorizationStatus::Revoked;
    converged_registry.devices[0].management_ready = false;
    converged_registry.devices[1].management_ready = true;
    let restarted = scenario.receiver_core(true);
    let replay = restarted
        .resume_imported_ack(RootKeyAckResumeInput {
            local_alias: LOCAL_ALIAS,
            message_id: MESSAGE_ID,
            current_did_document: &scenario.document,
            current_registry: &converged_registry,
        })
        .unwrap();

    assert!(replay.replayed());
    assert_eq!(replay.completion(), first.completion());
    assert!(replay.plaintext().expose_secret() == first.plaintext().expose_secret());
    assert_eq!(std::fs::read(record_path).unwrap(), record_before);

    let exact_envelope_replay = restarted
        .import_envelope(
            prepared.plaintext(),
            RootKeyEnvelopeImportInput {
                local_alias: LOCAL_ALIAS,
                direct_binding: &direct,
                transport_context: &context,
                current_did_document: &scenario.document,
                current_registry: &converged_registry,
                now: scenario.expires_at + Duration::minutes(5),
            },
        )
        .unwrap();
    assert_eq!(exact_envelope_replay.completion(), first.completion());
}

#[test]
fn expired_import_allows_new_sender_message_without_resealing_root() {
    let scenario = scenario();
    let original = scenario.prepare();
    let original_context = original.transport_context().clone();
    let original_direct = scenario.direct_binding();
    let first = scenario
        .import(
            &original,
            &original_direct,
            &original_context,
            &scenario.document,
            &scenario.registry,
            scenario.now + Duration::seconds(5),
        )
        .unwrap();
    let store = IdentityStore::new(&scenario.receiver.paths);
    let imported_index = store.load_index().unwrap();
    let root_ref = imported_index.credentials[LOCAL_ALIAS]
        .vault_migration
        .as_ref()
        .unwrap()
        .vnext_refs
        .as_ref()
        .unwrap()
        .did_document_root_private
        .as_ref()
        .unwrap()
        .clone();
    let record_path = scenario.receiver.vault.store().record_path(&root_ref);
    let record_before = std::fs::read(&record_path).unwrap();

    let (current_document, current_registry, second_sender) =
        add_ready_admin(&scenario, "dev-second-sender");
    let retry_now = scenario.expires_at + Duration::seconds(1);
    let retry_expires_at = retry_now + Duration::minutes(2);
    let retry_message_id = "root-message-after-missed-ack";
    let mut retry_envelope: RootKeyEnvelope = decode_root_envelope(original.plaintext()).unwrap();
    retry_envelope.message_id = retry_message_id.to_owned();
    retry_envelope.sender_device_id = second_sender.device_id.clone();
    retry_envelope.expires_at = format_time(retry_expires_at).unwrap();
    let retry_plaintext = encode_zeroizing_json(&retry_envelope).unwrap();
    let retry_context = RootImportTransportContext {
        message_id: retry_message_id.to_owned(),
        delivery_class: ROOT_KEY_CONTROL_DELIVERY_CLASS.to_owned(),
        sender_device_id: second_sender.device_id.clone(),
        recipient_device_id: scenario.recipient_device_id.clone(),
        expires_at: retry_envelope.expires_at.clone(),
    };
    let retry_direct = scenario.direct_binding_for(
        retry_message_id,
        &second_sender.device_id,
        &second_sender.e2ee_key_id,
        8,
    );

    let before_expiry = scenario.receiver_core(true).import_envelope(
        &retry_plaintext,
        RootKeyEnvelopeImportInput {
            local_alias: LOCAL_ALIAS,
            direct_binding: &retry_direct,
            transport_context: &retry_context,
            current_did_document: &current_document,
            current_registry: &current_registry,
            now: scenario.expires_at - Duration::seconds(1),
        },
    );
    assert_eq!(before_expiry.unwrap_err(), crate::ImError::PermissionDenied);

    let resumed_old = scenario
        .receiver_core(true)
        .resume_imported_ack(RootKeyAckResumeInput {
            local_alias: LOCAL_ALIAS,
            message_id: MESSAGE_ID,
            current_did_document: &current_document,
            current_registry: &current_registry,
        })
        .unwrap();
    assert!(resumed_old.replayed());
    assert_eq!(resumed_old.completion(), first.completion());

    let alternate_private = anp::PrivateKeyMaterial::Ed25519(ed25519_dalek::SigningKey::generate(
        &mut rand::rngs::OsRng,
    ));
    let mut wrong_root_envelope: RootKeyEnvelope = decode_root_envelope(&retry_plaintext).unwrap();
    wrong_root_envelope.root_private_key = alternate_private.to_pem();
    let wrong_root_plaintext = encode_zeroizing_json(&wrong_root_envelope).unwrap();
    let wrong_root = scenario.receiver_core(true).import_envelope(
        &wrong_root_plaintext,
        RootKeyEnvelopeImportInput {
            local_alias: LOCAL_ALIAS,
            direct_binding: &retry_direct,
            transport_context: &retry_context,
            current_did_document: &current_document,
            current_registry: &current_registry,
            now: retry_now,
        },
    );
    assert_eq!(wrong_root.unwrap_err(), crate::ImError::PermissionDenied);
    assert!(std::fs::read(&record_path).unwrap() == record_before);

    let replaced = scenario
        .receiver_core(true)
        .import_envelope(
            &retry_plaintext,
            RootKeyEnvelopeImportInput {
                local_alias: LOCAL_ALIAS,
                direct_binding: &retry_direct,
                transport_context: &retry_context,
                current_did_document: &current_document,
                current_registry: &current_registry,
                now: retry_now,
            },
        )
        .unwrap();
    assert!(!replaced.replayed());
    assert_eq!(replaced.completion().ack_for_message_id, retry_message_id);
    assert!(std::fs::read(&record_path).unwrap() == record_before);
    let replaced_index = store.load_index().unwrap();
    assert_eq!(
        replaced_index.credentials[LOCAL_ALIAS]
            .root_key_import
            .as_ref()
            .unwrap()
            .reservation
            .message_id,
        retry_message_id
    );

    let old_ack = scenario
        .receiver_core(true)
        .resume_imported_ack(RootKeyAckResumeInput {
            local_alias: LOCAL_ALIAS,
            message_id: MESSAGE_ID,
            current_did_document: &current_document,
            current_registry: &current_registry,
        });
    assert_eq!(old_ack.unwrap_err(), crate::ImError::PermissionDenied);
    let exact_new = scenario
        .receiver_core(true)
        .import_envelope(
            &retry_plaintext,
            RootKeyEnvelopeImportInput {
                local_alias: LOCAL_ALIAS,
                direct_binding: &retry_direct,
                transport_context: &retry_context,
                current_did_document: &current_document,
                current_registry: &current_registry,
                now: retry_expires_at + Duration::minutes(5),
            },
        )
        .unwrap();
    assert!(exact_new.replayed());
    assert_eq!(exact_new.completion(), replaced.completion());
}

#[test]
fn replay_and_resume_reject_local_device_identity_or_key_drift() {
    let scenario = scenario();
    let prepared = scenario.prepare();
    let context = prepared.transport_context().clone();
    let direct = scenario.direct_binding();
    scenario
        .import(
            &prepared,
            &direct,
            &context,
            &scenario.document,
            &scenario.registry,
            scenario.now + Duration::seconds(5),
        )
        .unwrap();
    let store = IdentityStore::new(&scenario.receiver.paths);
    let baseline = store.load_index().unwrap();

    for drift in 0..5 {
        let mut changed = baseline.clone();
        let authorization = changed
            .credentials
            .get_mut(LOCAL_ALIAS)
            .unwrap()
            .device_state
            .as_mut()
            .unwrap()
            .authorization
            .as_mut()
            .unwrap();
        match drift {
            0 => {
                authorization.protocol_device_id =
                    crate::ids::ProtocolDeviceId::parse("dev-switched-local").unwrap();
            }
            1 => {
                authorization.signing_key_id =
                    format!("{}#drifted-sign", scenario.generated.did.as_str());
            }
            2 => {
                authorization.e2ee_key_id =
                    format!("{}#drifted-e2ee", scenario.generated.did.as_str());
            }
            3 => authorization.status = DeviceAuthorizationStatus::Revoked,
            _ => authorization.role = DeviceAuthorizationRole::Member,
        }
        let lock = store.lock_index_mutation().unwrap();
        store.save_index_locked(&lock, changed).unwrap();
        drop(lock);

        let resumed = scenario
            .receiver_core(true)
            .resume_imported_ack(RootKeyAckResumeInput {
                local_alias: LOCAL_ALIAS,
                message_id: MESSAGE_ID,
                current_did_document: &scenario.document,
                current_registry: &scenario.registry,
            });
        assert_eq!(resumed.unwrap_err(), crate::ImError::PermissionDenied);
        let replayed = scenario.receiver_core(true).import_envelope(
            prepared.plaintext(),
            RootKeyEnvelopeImportInput {
                local_alias: LOCAL_ALIAS,
                direct_binding: &direct,
                transport_context: &context,
                current_did_document: &scenario.document,
                current_registry: &scenario.registry,
                now: scenario.expires_at + Duration::minutes(1),
            },
        );
        assert_eq!(replayed.unwrap_err(), crate::ImError::PermissionDenied);
    }
}

#[test]
fn same_message_conflict_is_rejected_without_replacing_root() {
    let scenario = scenario();
    let prepared = scenario.prepare();
    let context = prepared.transport_context().clone();
    let direct = scenario.direct_binding();
    scenario
        .import(
            &prepared,
            &direct,
            &context,
            &scenario.document,
            &scenario.registry,
            scenario.now + Duration::seconds(5),
        )
        .unwrap();
    let index = IdentityStore::new(&scenario.receiver.paths)
        .load_index()
        .unwrap();
    let root_ref = index.credentials[LOCAL_ALIAS]
        .vault_migration
        .as_ref()
        .unwrap()
        .vnext_refs
        .as_ref()
        .unwrap()
        .did_document_root_private
        .as_ref()
        .unwrap()
        .clone();
    let record_path = scenario.receiver.vault.store().record_path(&root_ref);
    let record_before = std::fs::read(&record_path).unwrap();

    let mut conflict: RootKeyEnvelope = decode_strict_json(prepared.plaintext()).unwrap();
    conflict.expires_at = format_time(scenario.expires_at + Duration::seconds(30)).unwrap();
    let conflict_plaintext = encode_zeroizing_json(&conflict).unwrap();
    let mut conflict_context = context.clone();
    conflict_context.expires_at = conflict.expires_at.clone();
    let error = scenario
        .receiver_core(true)
        .import_envelope(
            &conflict_plaintext,
            RootKeyEnvelopeImportInput {
                local_alias: LOCAL_ALIAS,
                direct_binding: &direct,
                transport_context: &conflict_context,
                current_did_document: &scenario.document,
                current_registry: &scenario.registry,
                now: scenario.now + Duration::seconds(6),
            },
        )
        .unwrap_err();

    assert_eq!(error, crate::ImError::PermissionDenied);
    assert_eq!(std::fs::read(record_path).unwrap(), record_before);
}

#[test]
fn corrupted_index_directory_cannot_escape_root_or_remove_external_pending() {
    let scenario = scenario();
    let prepared = scenario.prepare();
    let context = prepared.transport_context().clone();
    let direct = scenario.direct_binding();
    let store = IdentityStore::new(&scenario.receiver.paths);
    let original_index = store.load_index().unwrap();
    let original_dir = original_index.credentials[LOCAL_ALIAS].dir_name.clone();
    let outside_dir = scenario
        .receiver
        .paths
        .identity_root_dir
        .parent()
        .unwrap()
        .join("outside-identity");
    let outside_pending = outside_dir.join(ROOT_IMPORT_PENDING_FILE);

    let mut traversing = original_index.clone();
    traversing
        .credentials
        .get_mut(LOCAL_ALIAS)
        .unwrap()
        .dir_name = "../outside-identity".to_owned();
    let lock = store.lock_index_mutation().unwrap();
    store.save_index_locked(&lock, traversing).unwrap();
    drop(lock);
    let rejected = scenario.import(
        &prepared,
        &direct,
        &context,
        &scenario.document,
        &scenario.registry,
        scenario.now + Duration::seconds(1),
    );
    assert!(rejected.is_err());
    assert!(!outside_pending.exists());

    let mut restored = store.load_index().unwrap();
    restored.credentials.get_mut(LOCAL_ALIAS).unwrap().dir_name = original_dir;
    let lock = store.lock_index_mutation().unwrap();
    store.save_index_locked(&lock, restored).unwrap();
    drop(lock);
    scenario
        .import(
            &prepared,
            &direct,
            &context,
            &scenario.document,
            &scenario.registry,
            scenario.now + Duration::seconds(2),
        )
        .unwrap();

    std::fs::create_dir_all(&outside_dir).unwrap();
    let envelope = decode_root_envelope(prepared.plaintext()).unwrap();
    let external_pending = PendingRootKeyImport {
        schema_version: ROOT_KEY_IMPORT_SCHEMA_VERSION,
        reservation: envelope.reservation(),
    };
    std::fs::write(
        &outside_pending,
        serde_json::to_vec(&external_pending).unwrap(),
    )
    .unwrap();
    let sentinel_before = std::fs::read(&outside_pending).unwrap();
    let mut traversing = store.load_index().unwrap();
    traversing
        .credentials
        .get_mut(LOCAL_ALIAS)
        .unwrap()
        .dir_name = "../outside-identity".to_owned();
    let lock = store.lock_index_mutation().unwrap();
    store.save_index_locked(&lock, traversing).unwrap();
    drop(lock);

    let replay = scenario.receiver_core(true).import_envelope(
        prepared.plaintext(),
        RootKeyEnvelopeImportInput {
            local_alias: LOCAL_ALIAS,
            direct_binding: &direct,
            transport_context: &context,
            current_did_document: &scenario.document,
            current_registry: &scenario.registry,
            now: scenario.expires_at + Duration::minutes(1),
        },
    );
    assert!(replay.is_err());
    assert!(std::fs::read(&outside_pending).unwrap() == sentinel_before);
}

#[test]
fn restart_resumes_pending_vault_seal_without_reencrypting_root() {
    let scenario = scenario();
    let prepared = scenario.prepare();
    let context = prepared.transport_context().clone();
    let direct = scenario.direct_binding();
    let envelope: RootKeyEnvelope = decode_strict_json(prepared.plaintext()).unwrap();
    let reservation = envelope.reservation();
    let store = IdentityStore::new(&scenario.receiver.paths);
    let index = store.load_index().unwrap();
    let entry = &index.credentials[LOCAL_ALIAS];
    let vault_context = require_vault_context(entry, &scenario.receiver.storage).unwrap();
    let identity_dir = scenario
        .receiver
        .paths
        .identity_root_dir
        .join(&entry.dir_name);
    ensure_pending_reservation(&identity_dir, &reservation, scenario.now).unwrap();
    let root_ref = expected_root_ref(&vault_context, &envelope.root_key_id);
    let sealed = vault_context
        .vault
        .seal(SealSecretRequest {
            metadata: SecretMetadata {
                workspace_id: root_ref.workspace_id.clone(),
                device_id: root_ref.device_id.clone(),
                identity_id: root_ref.identity_id.clone(),
                did: root_ref.did.clone(),
                kind: root_ref.kind.clone(),
                key_id: root_ref.key_id.clone(),
                key_version: root_ref.key_version,
                policy: SecretAccessPolicy::no_prompt_local_secret(),
            },
            plaintext: SecretBytes::from_vec(envelope.root_private_key.as_bytes().to_vec()),
        })
        .unwrap();
    assert_eq!(sealed, root_ref);
    let record_path = scenario.receiver.vault.store().record_path(&root_ref);
    let record_before = std::fs::read(&record_path).unwrap();

    let imported = scenario
        .import(
            &prepared,
            &direct,
            &context,
            &scenario.document,
            &scenario.registry,
            scenario.now + Duration::seconds(10),
        )
        .unwrap();

    assert!(!imported.replayed());
    assert_eq!(std::fs::read(record_path).unwrap(), record_before);
    assert!(!identity_dir.join(ROOT_IMPORT_PENDING_FILE).exists());
    let committed = store.load_index().unwrap();
    assert_eq!(
        committed.credentials[LOCAL_ALIAS]
            .root_key_import
            .as_ref()
            .unwrap()
            .completion,
        *imported.completion()
    );
}

#[test]
fn expired_pending_with_sealed_orphan_allows_fresh_message_without_root_overwrite() {
    let scenario = scenario();
    let stale_prepared = scenario.prepare();
    let stale_envelope: RootKeyEnvelope = decode_strict_json(stale_prepared.plaintext()).unwrap();
    let stale_reservation = stale_envelope.reservation();
    let store = IdentityStore::new(&scenario.receiver.paths);
    let index = store.load_index().unwrap();
    let entry = &index.credentials[LOCAL_ALIAS];
    assert!(entry.root_key_import.is_none());
    let vault_context = require_vault_context(entry, &scenario.receiver.storage).unwrap();
    let identity_dir = scenario
        .receiver
        .paths
        .identity_root_dir
        .join(&entry.dir_name);
    ensure_pending_reservation(&identity_dir, &stale_reservation, scenario.now).unwrap();
    let root_ref = expected_root_ref(&vault_context, &stale_envelope.root_key_id);
    vault_context
        .vault
        .seal(SealSecretRequest {
            metadata: SecretMetadata {
                workspace_id: root_ref.workspace_id.clone(),
                device_id: root_ref.device_id.clone(),
                identity_id: root_ref.identity_id.clone(),
                did: root_ref.did.clone(),
                kind: root_ref.kind.clone(),
                key_id: root_ref.key_id.clone(),
                key_version: root_ref.key_version,
                policy: SecretAccessPolicy::no_prompt_local_secret(),
            },
            plaintext: SecretBytes::from_vec(stale_envelope.root_private_key.as_bytes().to_vec()),
        })
        .unwrap();
    let record_path = scenario.receiver.vault.store().record_path(&root_ref);
    let record_before = std::fs::read(&record_path).unwrap();

    let unexpired_message_id = "root-message-conflict-before-expiry";
    let unexpired_prepared = scenario
        .sender_core(true)
        .prepare_envelope(RootKeyEnvelopePrepareInput {
            local_alias: LOCAL_ALIAS,
            did_document: &scenario.document,
            registry: &scenario.registry,
            recipient_device_id: &scenario.recipient_device_id,
            message_id: unexpired_message_id,
            user_presence_at: scenario.now,
            now: scenario.now,
            expires_at: scenario.expires_at,
        })
        .unwrap();
    let unexpired_direct = RootControlDirectBinding {
        message_id: unexpired_message_id.to_owned(),
        ..scenario.direct_binding()
    };
    assert_eq!(
        scenario
            .receiver_core(true)
            .import_envelope(
                unexpired_prepared.plaintext(),
                RootKeyEnvelopeImportInput {
                    local_alias: LOCAL_ALIAS,
                    direct_binding: &unexpired_direct,
                    transport_context: unexpired_prepared.transport_context(),
                    current_did_document: &scenario.document,
                    current_registry: &scenario.registry,
                    now: scenario.now + Duration::seconds(1),
                },
            )
            .unwrap_err(),
        crate::ImError::PermissionDenied
    );
    assert_eq!(std::fs::read(&record_path).unwrap(), record_before);

    let fresh_message_id = "root-message-fresh-after-expiry";
    let fresh_now = scenario.expires_at + Duration::seconds(1);
    let fresh_expires_at = fresh_now + Duration::minutes(2);
    let fresh_prepared = scenario
        .sender_core(true)
        .prepare_envelope(RootKeyEnvelopePrepareInput {
            local_alias: LOCAL_ALIAS,
            did_document: &scenario.document,
            registry: &scenario.registry,
            recipient_device_id: &scenario.recipient_device_id,
            message_id: fresh_message_id,
            user_presence_at: fresh_now,
            now: fresh_now,
            expires_at: fresh_expires_at,
        })
        .unwrap();
    let fresh_direct = RootControlDirectBinding {
        message_id: fresh_message_id.to_owned(),
        ..scenario.direct_binding()
    };
    let imported = scenario
        .receiver_core(true)
        .import_envelope(
            fresh_prepared.plaintext(),
            RootKeyEnvelopeImportInput {
                local_alias: LOCAL_ALIAS,
                direct_binding: &fresh_direct,
                transport_context: fresh_prepared.transport_context(),
                current_did_document: &scenario.document,
                current_registry: &scenario.registry,
                now: fresh_now + Duration::seconds(1),
            },
        )
        .unwrap();

    assert_eq!(imported.completion().ack_for_message_id, fresh_message_id);
    assert_eq!(std::fs::read(record_path).unwrap(), record_before);
    let committed = store.load_index().unwrap();
    assert_eq!(
        committed.credentials[LOCAL_ALIAS]
            .root_key_import
            .as_ref()
            .unwrap()
            .reservation
            .message_id,
        fresh_message_id
    );
    assert!(!identity_dir.join(ROOT_IMPORT_PENDING_FILE).exists());
}

#[test]
fn import_fails_closed_for_wrong_direction_role_expiry_current_document_and_root() {
    let scenario = scenario();
    let prepared = scenario.prepare();
    let context = prepared.transport_context().clone();
    let direct = scenario.direct_binding();

    let mut wrong_direct = direct.clone();
    wrong_direct.session_binding.peer_device_id = scenario.recipient_device_id.clone();
    assert_eq!(
        scenario
            .import(
                &prepared,
                &wrong_direct,
                &context,
                &scenario.document,
                &scenario.registry,
                scenario.now + Duration::seconds(1),
            )
            .unwrap_err(),
        crate::ImError::PermissionDenied
    );

    let mut wrong_recipient = direct.clone();
    wrong_recipient.session_binding.local_did = "did:example:mallory".to_owned();
    wrong_recipient.session_binding.local_device_id = "dev-not-local".to_owned();
    assert_eq!(
        scenario
            .import(
                &prepared,
                &wrong_recipient,
                &context,
                &scenario.document,
                &scenario.registry,
                scenario.now + Duration::seconds(1),
            )
            .unwrap_err(),
        crate::ImError::PermissionDenied
    );

    let mut wrong_session = direct.clone();
    wrong_session.session_id = "not-a-p5-v2-session".to_owned();
    assert_eq!(
        scenario
            .import(
                &prepared,
                &wrong_session,
                &context,
                &scenario.document,
                &scenario.registry,
                scenario.now + Duration::seconds(1),
            )
            .unwrap_err(),
        crate::ImError::PermissionDenied
    );

    let mut stale_local_key = direct.clone();
    stale_local_key.session_binding.local_e2ee_key_id =
        format!("{}#stale-local-e2ee", scenario.generated.did.as_str());
    assert_eq!(
        scenario
            .import(
                &prepared,
                &stale_local_key,
                &context,
                &scenario.document,
                &scenario.registry,
                scenario.now + Duration::seconds(1),
            )
            .unwrap_err(),
        crate::ImError::PermissionDenied
    );

    let mut stale_peer_key = direct.clone();
    stale_peer_key.session_binding.peer_e2ee_key_id =
        format!("{}#stale-peer-e2ee", scenario.generated.did.as_str());
    assert_eq!(
        scenario
            .import(
                &prepared,
                &stale_peer_key,
                &context,
                &scenario.document,
                &scenario.registry,
                scenario.now + Duration::seconds(1),
            )
            .unwrap_err(),
        crate::ImError::PermissionDenied
    );

    let mut sender_revoked = scenario.registry.clone();
    sender_revoked.devices[0].status = DeviceAuthorizationStatus::Revoked;
    assert_eq!(
        scenario
            .import(
                &prepared,
                &direct,
                &context,
                &scenario.document,
                &sender_revoked,
                scenario.now + Duration::seconds(1),
            )
            .unwrap_err(),
        crate::ImError::PermissionDenied
    );

    let mut sender_not_ready = scenario.registry.clone();
    sender_not_ready.devices[0].management_ready = false;
    assert_eq!(
        scenario
            .import(
                &prepared,
                &direct,
                &context,
                &scenario.document,
                &sender_not_ready,
                scenario.now + Duration::seconds(1),
            )
            .unwrap_err(),
        crate::ImError::PermissionDenied
    );

    let mut member_registry = scenario.registry.clone();
    member_registry.devices[1].role = DeviceAuthorizationRole::Member;
    assert_eq!(
        scenario
            .import(
                &prepared,
                &direct,
                &context,
                &scenario.document,
                &member_registry,
                scenario.now + Duration::seconds(2),
            )
            .unwrap_err(),
        crate::ImError::PermissionDenied
    );

    let mut revoked_registry = scenario.registry.clone();
    revoked_registry.devices[1].status = DeviceAuthorizationStatus::Revoked;
    assert_eq!(
        scenario
            .import(
                &prepared,
                &direct,
                &context,
                &scenario.document,
                &revoked_registry,
                scenario.now + Duration::seconds(3),
            )
            .unwrap_err(),
        crate::ImError::PermissionDenied
    );

    let mut rolled_back_registry = scenario.registry.clone();
    rolled_back_registry.checkpoint.registry_version = 1;
    assert_eq!(
        scenario
            .import(
                &prepared,
                &direct,
                &context,
                &scenario.document,
                &rolled_back_registry,
                scenario.now + Duration::seconds(3),
            )
            .unwrap_err(),
        crate::ImError::PermissionDenied
    );

    assert_eq!(
        scenario
            .import(
                &prepared,
                &direct,
                &context,
                &scenario.document,
                &scenario.registry,
                scenario.expires_at,
            )
            .unwrap_err(),
        crate::ImError::SessionExpired
    );

    let mut excessive_ttl: RootKeyEnvelope = decode_strict_json(prepared.plaintext()).unwrap();
    excessive_ttl.expires_at = format_time(scenario.now + Duration::hours(1)).unwrap();
    let excessive_ttl_plaintext = encode_zeroizing_json(&excessive_ttl).unwrap();
    let mut excessive_ttl_context = context.clone();
    excessive_ttl_context.expires_at = excessive_ttl.expires_at.clone();
    assert_eq!(
        scenario
            .receiver_core(true)
            .import_envelope(
                &excessive_ttl_plaintext,
                RootKeyEnvelopeImportInput {
                    local_alias: LOCAL_ALIAS,
                    direct_binding: &direct,
                    transport_context: &excessive_ttl_context,
                    current_did_document: &scenario.document,
                    current_registry: &scenario.registry,
                    now: scenario.now,
                },
            )
            .unwrap_err(),
        crate::ImError::SessionExpired
    );

    let mut wrong_snapshot = scenario.document.clone();
    wrong_snapshot["x-wrong-snapshot"] = json!(true);
    assert_eq!(
        scenario
            .import(
                &prepared,
                &direct,
                &context,
                &wrong_snapshot,
                &scenario.registry,
                scenario.now + Duration::seconds(4),
            )
            .unwrap_err(),
        crate::ImError::PermissionDenied
    );

    let mut device_drift_document = anp::authentication::remove_device_from_did_document(
        &scenario.document,
        &scenario.generated.root_key_id,
        &scenario.recipient_device_id,
    )
    .unwrap();
    crate::internal::identity_daemon_subkey::resign_did_document_with_key1(
        &mut device_drift_document,
        &scenario.generated.did,
        &scenario.generated.root_private_pem,
    )
    .unwrap();
    let mut device_drift_registry = scenario.registry.clone();
    device_drift_registry.checkpoint.document_version += 1;
    device_drift_registry.checkpoint.registry_version += 1;
    device_drift_registry.checkpoint.document_hash =
        crate::internal::identity_wire::device_genesis::document_hash(&device_drift_document)
            .unwrap();
    assert_eq!(
        scenario
            .import(
                &prepared,
                &direct,
                &context,
                &device_drift_document,
                &device_drift_registry,
                scenario.now + Duration::seconds(4),
            )
            .unwrap_err(),
        crate::ImError::PermissionDenied
    );

    let alternate_root = anp::PrivateKeyMaterial::Ed25519(ed25519_dalek::SigningKey::generate(
        &mut rand::rngs::OsRng,
    ));
    let mut root_drift_document = scenario.document.clone();
    root_drift_document
        .get_mut("verificationMethod")
        .and_then(Value::as_array_mut)
        .unwrap()
        .iter_mut()
        .find(|method| {
            method.get("id").and_then(Value::as_str)
                == Some(scenario.generated.root_key_id.as_str())
        })
        .unwrap()["publicKeyMultibase"] =
        Value::String(public_key_multibase(&alternate_root.public_key()));
    crate::internal::identity_daemon_subkey::resign_did_document_with_key1(
        &mut root_drift_document,
        &scenario.generated.did,
        &alternate_root.to_pem(),
    )
    .unwrap();
    let mut root_drift_registry = scenario.registry.clone();
    root_drift_registry.checkpoint.document_version += 1;
    root_drift_registry.checkpoint.registry_version += 1;
    root_drift_registry.checkpoint.document_hash =
        crate::internal::identity_wire::device_genesis::document_hash(&root_drift_document)
            .unwrap();
    assert_eq!(
        scenario
            .import(
                &prepared,
                &direct,
                &context,
                &root_drift_document,
                &root_drift_registry,
                scenario.now + Duration::seconds(4),
            )
            .unwrap_err(),
        crate::ImError::PermissionDenied
    );

    let mut wrong_root: RootKeyEnvelope = decode_strict_json(prepared.plaintext()).unwrap();
    wrong_root.root_private_key = anp::PrivateKeyMaterial::Ed25519(
        ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng),
    )
    .to_pem();
    let wrong_root_plaintext = encode_zeroizing_json(&wrong_root).unwrap();
    assert_eq!(
        scenario
            .receiver_core(true)
            .import_envelope(
                &wrong_root_plaintext,
                RootKeyEnvelopeImportInput {
                    local_alias: LOCAL_ALIAS,
                    direct_binding: &direct,
                    transport_context: &context,
                    current_did_document: &scenario.document,
                    current_registry: &scenario.registry,
                    now: scenario.now + Duration::seconds(5),
                },
            )
            .unwrap_err(),
        crate::ImError::PermissionDenied
    );

    let index = IdentityStore::new(&scenario.receiver.paths)
        .load_index()
        .unwrap();
    let entry = &index.credentials[LOCAL_ALIAS];
    assert!(entry.root_key_import.is_none());
    assert!(entry
        .vault_migration
        .as_ref()
        .unwrap()
        .vnext_refs
        .as_ref()
        .unwrap()
        .did_document_root_private
        .is_none());
}

#[test]
fn completion_signature_is_jcs_bound_to_every_claim() {
    let scenario = scenario();
    let prepared = scenario.prepare();
    let context = prepared.transport_context().clone();
    let direct = scenario.direct_binding();
    let imported = scenario
        .import(
            &prepared,
            &direct,
            &context,
            &scenario.document,
            &scenario.registry,
            scenario.now + Duration::seconds(5),
        )
        .unwrap();
    let importer =
        unique_registry_device(&scenario.registry, &scenario.recipient_device_id).unwrap();
    let mut changed = imported.completion().clone();
    changed.document_version += 1;

    assert_eq!(
        verify_completion_signature(&changed, &scenario.document, importer).unwrap_err(),
        crate::ImError::PermissionDenied
    );
    let signature = URL_SAFE_NO_PAD
        .decode(&imported.completion().device_signature)
        .unwrap();
    assert_eq!(signature.len(), 64);
    assert_eq!(
        URL_SAFE_NO_PAD.encode(signature),
        imported.completion().device_signature
    );
}

#[test]
fn strict_inner_json_rejects_unknown_fields_without_persistence() {
    let scenario = scenario();
    let prepared = scenario.prepare();
    let mut raw: Value = serde_json::from_slice(prepared.plaintext().expose_secret()).unwrap();
    raw["transport_context"] = json!({"must_not": "enter-p5-inner-contract"});
    let plaintext = SecretBytes::from_vec(serde_json::to_vec(&raw).unwrap());
    let error = scenario
        .receiver_core(true)
        .import_envelope(
            &plaintext,
            RootKeyEnvelopeImportInput {
                local_alias: LOCAL_ALIAS,
                direct_binding: &scenario.direct_binding(),
                transport_context: prepared.transport_context(),
                current_did_document: &scenario.document,
                current_registry: &scenario.registry,
                now: scenario.now + Duration::seconds(1),
            },
        )
        .unwrap_err();

    assert!(matches!(error, crate::ImError::Serialization { .. }));
    let index = IdentityStore::new(&scenario.receiver.paths)
        .load_index()
        .unwrap();
    assert!(index.credentials[LOCAL_ALIAS].root_key_import.is_none());
}

#[test]
fn inner_size_and_document_version_bounds_fail_before_persistence() {
    let scenario = scenario();
    let oversized = SecretBytes::from_vec(vec![b' '; ROOT_CONTROL_MAX_INNER_BYTES + 1]);
    let oversized_result = scenario.receiver_core(true).import_envelope(
        &oversized,
        RootKeyEnvelopeImportInput {
            local_alias: LOCAL_ALIAS,
            direct_binding: &scenario.direct_binding(),
            transport_context: scenario.prepare().transport_context(),
            current_did_document: &scenario.document,
            current_registry: &scenario.registry,
            now: scenario.now,
        },
    );
    assert!(matches!(
        oversized_result,
        Err(crate::ImError::InvalidInput { .. })
    ));

    let prepared = scenario.prepare();
    let mut unsafe_version = decode_root_envelope(prepared.plaintext()).unwrap();
    unsafe_version.document_version = MAX_DOCUMENT_VERSION + 1;
    let unsafe_plaintext = encode_zeroizing_json(&unsafe_version).unwrap();
    let unsafe_result = scenario.receiver_core(true).import_envelope(
        &unsafe_plaintext,
        RootKeyEnvelopeImportInput {
            local_alias: LOCAL_ALIAS,
            direct_binding: &scenario.direct_binding(),
            transport_context: prepared.transport_context(),
            current_did_document: &scenario.document,
            current_registry: &scenario.registry,
            now: scenario.now,
        },
    );
    assert_eq!(unsafe_result.unwrap_err(), crate::ImError::PermissionDenied);
    assert!(IdentityStore::new(&scenario.receiver.paths)
        .load_index()
        .unwrap()
        .credentials[LOCAL_ALIAS]
        .root_key_import
        .is_none());
}

#[test]
fn root_import_token_operation_is_persisted_and_rotated_with_compare_and_swap() {
    let scenario = scenario();
    let imported = import_receiver_root(&scenario);
    let store = IdentityStore::new(&scenario.receiver.paths);

    let first = store
        .reserve_root_import_management_token_operation(
            LOCAL_ALIAS,
            &imported.completion().ack_for_message_id,
            "root-ready-first",
        )
        .unwrap();
    assert_eq!(first, "root-ready-first");
    assert_eq!(
        store
            .reserve_root_import_management_token_operation(
                LOCAL_ALIAS,
                &imported.completion().ack_for_message_id,
                "root-ready-unused",
            )
            .unwrap(),
        first
    );

    let rotated = store
        .rotate_root_import_management_token_operation(
            LOCAL_ALIAS,
            &imported.completion().ack_for_message_id,
            &first,
            "root-ready-second",
        )
        .unwrap();
    assert_eq!(rotated, "root-ready-second");
    assert_eq!(
        store
            .rotate_root_import_management_token_operation(
                LOCAL_ALIAS,
                &imported.completion().ack_for_message_id,
                &first,
                "root-ready-loser",
            )
            .unwrap(),
        rotated
    );
    let rotated_again = store
        .rotate_root_import_management_token_operation(
            LOCAL_ALIAS,
            &imported.completion().ack_for_message_id,
            &rotated,
            "root-ready-third",
        )
        .unwrap();
    let persisted = store.load_index().unwrap().credentials[LOCAL_ALIAS]
        .root_key_import
        .clone()
        .unwrap();
    assert_eq!(
        persisted.management_token_operation_id.as_deref(),
        Some(rotated_again.as_str())
    );
    assert!(store
        .reserve_root_import_management_token_operation(
            LOCAL_ALIAS,
            &imported.completion().ack_for_message_id,
            &"x".repeat(129),
        )
        .is_err());
}

#[test]
fn management_ready_convergence_commits_versioned_token_ref_and_state_together() {
    let scenario = scenario();
    let imported = import_receiver_root(&scenario);
    let store = IdentityStore::new(&scenario.receiver.paths);
    let before = store.load_index().unwrap();
    let before_entry = &before.credentials[LOCAL_ALIAS];
    let old_auth_ref = before_entry
        .vault_migration
        .as_ref()
        .and_then(|metadata| metadata.vnext_refs.as_ref())
        .map(|refs| refs.auth_jwt.clone())
        .unwrap();
    let checkpoint = ready_checkpoint(&scenario);

    assert!(store
        .converge_root_import_management_ready(
            LOCAL_ALIAS,
            "wrong-completed-message",
            2,
            &checkpoint,
            "new-access-token",
            "new-refresh-token",
            "2099-07-20T01:00:00Z",
            &scenario.receiver.storage,
        )
        .is_err());
    let still_before = store.load_index().unwrap();
    assert_eq!(
        still_before.credentials[LOCAL_ALIAS]
            .vault_migration
            .as_ref()
            .unwrap()
            .vnext_refs
            .as_ref()
            .unwrap()
            .auth_jwt,
        old_auth_ref
    );

    store
        .converge_root_import_management_ready(
            LOCAL_ALIAS,
            &imported.completion().ack_for_message_id,
            2,
            &checkpoint,
            "new-access-token",
            "new-refresh-token",
            "2099-07-20T01:00:00Z",
            &scenario.receiver.storage,
        )
        .unwrap();

    let converged = store.load_index().unwrap();
    let entry = &converged.credentials[LOCAL_ALIAS];
    let authorization = entry
        .device_state
        .as_ref()
        .and_then(|state| state.authorization.as_ref())
        .unwrap();
    assert!(authorization.management_ready);
    assert_eq!(authorization.auth_generation, 2);
    assert_eq!(
        entry.device_state.as_ref().unwrap().checkpoint,
        Some(checkpoint)
    );
    let metadata = entry.vault_migration.as_ref().unwrap();
    let new_auth_ref = metadata.vnext_refs.as_ref().unwrap().auth_jwt.clone();
    assert_eq!(metadata.refs.auth_jwt, new_auth_ref);
    assert_eq!(new_auth_ref.key_version, old_auth_ref.key_version + 1);
    // The superseded encrypted record remains readable until live providers
    // have advanced; only the new ref is authoritative in the index.
    assert!(scenario.receiver.vault.open(&old_auth_ref).is_ok());
    let opened = scenario.receiver.vault.open(&new_auth_ref).unwrap();
    let auth = crate::internal::auth::state::parse_auth_state(opened.expose_secret()).unwrap();
    assert_eq!(auth.bearer_token.as_deref(), Some("new-access-token"));
    assert_eq!(auth.refresh_token.as_deref(), Some("new-refresh-token"));

    store
        .converge_root_import_management_ready(
            LOCAL_ALIAS,
            &imported.completion().ack_for_message_id,
            2,
            entry
                .device_state
                .as_ref()
                .unwrap()
                .checkpoint
                .as_ref()
                .unwrap(),
            "ignored-access-token",
            "ignored-refresh-token",
            "2099-07-20T01:00:00Z",
            &scenario.receiver.storage,
        )
        .unwrap();
    let idempotent = store.load_index().unwrap();
    assert_eq!(
        idempotent.credentials[LOCAL_ALIAS]
            .vault_migration
            .as_ref()
            .unwrap()
            .vnext_refs
            .as_ref()
            .unwrap()
            .auth_jwt,
        new_auth_ref
    );
}

#[test]
fn concurrent_management_ready_convergence_cannot_overwrite_newer_token_generation() {
    let scenario = scenario();
    let imported = import_receiver_root(&scenario);
    let checkpoint = ready_checkpoint(&scenario);
    let barrier = Arc::new(std::sync::Barrier::new(3));
    let mut workers = Vec::new();
    for _ in 0..2 {
        let paths = scenario.receiver.paths.clone();
        let storage = scenario.receiver.storage.clone();
        let message_id = imported.completion().ack_for_message_id.clone();
        let checkpoint = checkpoint.clone();
        let barrier = barrier.clone();
        workers.push(std::thread::spawn(move || {
            barrier.wait();
            IdentityStore::new(&paths).converge_root_import_management_ready(
                LOCAL_ALIAS,
                &message_id,
                2,
                &checkpoint,
                "concurrent-access-token",
                "concurrent-refresh-token",
                "2099-07-20T01:00:00Z",
                &storage,
            )
        }));
    }
    barrier.wait();
    for worker in workers {
        worker.join().unwrap().unwrap();
    }

    let index = IdentityStore::new(&scenario.receiver.paths)
        .load_index()
        .unwrap();
    let entry = &index.credentials[LOCAL_ALIAS];
    let authorization = entry
        .device_state
        .as_ref()
        .and_then(|state| state.authorization.as_ref())
        .unwrap();
    assert!(authorization.management_ready);
    assert_eq!(authorization.auth_generation, 2);
    assert_eq!(
        entry
            .vault_migration
            .as_ref()
            .unwrap()
            .vnext_refs
            .as_ref()
            .unwrap()
            .auth_jwt
            .key_version,
        2
    );
    let current_auth_ref = entry
        .vault_migration
        .as_ref()
        .unwrap()
        .vnext_refs
        .as_ref()
        .unwrap()
        .auth_jwt
        .clone();
    assert!(IdentityStore::new(&scenario.receiver.paths)
        .converge_root_import_management_ready(
            LOCAL_ALIAS,
            &imported.completion().ack_for_message_id,
            1,
            &checkpoint,
            "stale-access-token",
            "stale-refresh-token",
            "2099-07-20T01:00:00Z",
            &scenario.receiver.storage,
        )
        .is_err());
    let after_stale = IdentityStore::new(&scenario.receiver.paths)
        .load_index()
        .unwrap();
    assert_eq!(
        after_stale.credentials[LOCAL_ALIAS]
            .vault_migration
            .as_ref()
            .unwrap()
            .vnext_refs
            .as_ref()
            .unwrap()
            .auth_jwt,
        current_auth_ref
    );
    let opened = scenario.receiver.vault.open(&current_auth_ref).unwrap();
    assert_eq!(
        crate::internal::auth::state::parse_auth_state(opened.expose_secret())
            .unwrap()
            .bearer_token
            .as_deref(),
        Some("concurrent-access-token")
    );
}

#[test]
fn exact_retry_repairs_live_provider_after_index_commit_and_first_ref_advance_failure() {
    let scenario = scenario();
    let imported = import_receiver_root(&scenario);
    let store = IdentityStore::new(&scenario.receiver.paths);
    let before = store.load_index().unwrap();
    let old_refs = before.credentials[LOCAL_ALIAS]
        .vault_migration
        .as_ref()
        .unwrap()
        .vnext_refs
        .clone()
        .unwrap();
    let old_auth_ref = old_refs.auth_jwt.clone();
    let live_vault = Arc::new(FailOnceTargetOpenVault {
        inner: scenario.receiver.vault.clone(),
        target: std::sync::Mutex::new(None),
        armed: std::sync::atomic::AtomicBool::new(false),
    });
    let provider = crate::internal::key_provider::vault::VaultBackedKeyMaterialProvider::new_vnext(
        scenario
            .receiver
            .paths
            .identity_root_dir
            .join("receiver-identity"),
        live_vault.clone(),
        old_refs,
    );
    assert_eq!(
        provider.valid_auth_token().unwrap().as_deref(),
        Some("device-token")
    );

    let committed = store
        .converge_root_import_management_ready(
            LOCAL_ALIAS,
            &imported.completion().ack_for_message_id,
            2,
            &ready_checkpoint(&scenario),
            "retry-access-token",
            "retry-refresh-token",
            "2099-07-20T01:00:00Z",
            &scenario.receiver.storage,
        )
        .unwrap();
    *live_vault.target.lock().unwrap() = Some(committed.clone());
    live_vault
        .armed
        .store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(provider.advance_vault_auth_ref(&committed).is_err());
    assert_eq!(
        provider.valid_auth_token().unwrap().as_deref(),
        Some("device-token")
    );

    let retry_ref = store
        .committed_root_import_management_auth_ref(
            LOCAL_ALIAS,
            &imported.completion().ack_for_message_id,
            2,
            &scenario.receiver.storage,
        )
        .unwrap();
    provider.advance_vault_auth_ref(&retry_ref).unwrap();
    assert_eq!(
        provider.valid_auth_token().unwrap().as_deref(),
        Some("retry-access-token")
    );
    // The old encrypted ref is retained for other live providers but is no
    // longer authoritative in the identity index.
    assert!(scenario.receiver.vault.open(&old_auth_ref).is_ok());
}

#[test]
fn exact_retry_repairs_live_provider_after_committed_access_token_expires() {
    let scenario = scenario();
    let imported = import_receiver_root(&scenario);
    let store = IdentityStore::new(&scenario.receiver.paths);
    let old_refs = store.load_index().unwrap().credentials[LOCAL_ALIAS]
        .vault_migration
        .as_ref()
        .unwrap()
        .vnext_refs
        .clone()
        .unwrap();
    let live_vault = Arc::new(FailOnceTargetOpenVault {
        inner: scenario.receiver.vault.clone(),
        target: std::sync::Mutex::new(None),
        armed: std::sync::atomic::AtomicBool::new(false),
    });
    let provider = crate::internal::key_provider::vault::VaultBackedKeyMaterialProvider::new_vnext(
        scenario
            .receiver
            .paths
            .identity_root_dir
            .join("receiver-identity"),
        live_vault.clone(),
        old_refs,
    );

    let committed = store
        .converge_root_import_management_ready(
            LOCAL_ALIAS,
            &imported.completion().ack_for_message_id,
            2,
            &ready_checkpoint(&scenario),
            "expired-access-token",
            "refresh-still-valid",
            "2000-01-01T00:00:00Z",
            &scenario.receiver.storage,
        )
        .unwrap();
    *live_vault.target.lock().unwrap() = Some(committed.clone());
    live_vault
        .armed
        .store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(provider.advance_vault_auth_ref(&committed).is_err());

    let retry_ref = store
        .committed_root_import_management_auth_ref(
            LOCAL_ALIAS,
            &imported.completion().ack_for_message_id,
            2,
            &scenario.receiver.storage,
        )
        .unwrap();
    provider.advance_vault_auth_ref(&retry_ref).unwrap();
    let repaired = provider.auth_state().unwrap();
    assert!(repaired.has_token);
    assert!(!repaired.has_valid_token);
    assert_eq!(
        repaired.refresh_token.as_deref(),
        Some("refresh-still-valid")
    );
}

#[test]
fn failed_index_commit_keeps_old_token_ref_and_authorization_state() {
    let scenario = scenario();
    let imported = import_receiver_root(&scenario);
    let store = IdentityStore::new(&scenario.receiver.paths);
    let before = store.load_index().unwrap();
    let old_auth_ref = before.credentials[LOCAL_ALIAS]
        .vault_migration
        .as_ref()
        .unwrap()
        .vnext_refs
        .as_ref()
        .unwrap()
        .auth_jwt
        .clone();
    let backup_path = scenario
        .receiver
        .paths
        .registry_path
        .with_extension("before-failed-commit");
    let failing_vault = Arc::new(FailIndexCommitAfterTokenStageVault {
        inner: scenario.receiver.vault.clone(),
        registry_path: scenario.receiver.paths.registry_path.clone(),
        backup_path: backup_path.clone(),
        armed: std::sync::atomic::AtomicBool::new(true),
    });
    let storage = SaveIdentitySecretStorage::Vault {
        workspace_id: "workspace-root-transfer".to_owned(),
        device_id: "vault-context-receiver-identity".to_owned(),
        vault: failing_vault,
    };

    let result = store.converge_root_import_management_ready(
        LOCAL_ALIAS,
        &imported.completion().ack_for_message_id,
        2,
        &ready_checkpoint(&scenario),
        "failed-access-token",
        "failed-refresh-token",
        "2099-07-20T01:00:00Z",
        &storage,
    );
    assert!(scenario.receiver.paths.registry_path.is_dir());
    std::fs::remove_dir(&scenario.receiver.paths.registry_path).unwrap();
    std::fs::rename(&backup_path, &scenario.receiver.paths.registry_path).unwrap();
    assert!(result.is_err());

    let after = store.load_index().unwrap();
    let entry = &after.credentials[LOCAL_ALIAS];
    let authorization = entry
        .device_state
        .as_ref()
        .and_then(|state| state.authorization.as_ref())
        .unwrap();
    assert!(!authorization.management_ready);
    assert_eq!(authorization.auth_generation, 1);
    assert_eq!(
        entry
            .vault_migration
            .as_ref()
            .unwrap()
            .vnext_refs
            .as_ref()
            .unwrap()
            .auth_jwt,
        old_auth_ref
    );
    assert!(scenario.receiver.vault.open(&old_auth_ref).is_ok());
    assert!(!scenario
        .receiver
        .vault
        .list()
        .unwrap()
        .iter()
        .any(|secret_ref| {
            secret_ref.kind == SecretKind::AuthJwt && secret_ref.key_version == 2
        }));
}

fn import_receiver_root(scenario: &Scenario) -> ImportedRootKeyAck {
    let prepared = scenario.prepare();
    scenario
        .import(
            &prepared,
            &scenario.direct_binding(),
            prepared.transport_context(),
            &scenario.document,
            &scenario.registry,
            scenario.now + Duration::seconds(1),
        )
        .unwrap()
}

fn ready_checkpoint(scenario: &Scenario) -> IdentityInternalCheckpoint {
    let mut checkpoint = scenario.registry.checkpoint.clone();
    checkpoint.registry_version += 1;
    checkpoint
}

#[derive(Debug)]
struct FailIndexCommitAfterTokenStageVault {
    inner: Arc<FileSecretVault>,
    registry_path: std::path::PathBuf,
    backup_path: std::path::PathBuf,
    armed: std::sync::atomic::AtomicBool,
}

#[derive(Debug)]
struct FailOnceTargetOpenVault {
    inner: Arc<FileSecretVault>,
    target: std::sync::Mutex<Option<SecretRef>>,
    armed: std::sync::atomic::AtomicBool,
}

impl SecretVault for FailOnceTargetOpenVault {
    fn seal(&self, request: SealSecretRequest) -> crate::ImResult<SecretRef> {
        self.inner.seal(request)
    }

    fn seal_if_absent(&self, request: SealSecretRequest) -> crate::ImResult<SealIfAbsentResult> {
        self.inner.seal_if_absent(request)
    }

    fn open(&self, secret_ref: &SecretRef) -> crate::ImResult<SecretBytes> {
        let fail = self
            .target
            .lock()
            .map_err(|_| crate::ImError::PermissionDenied)?
            .as_ref()
            == Some(secret_ref)
            && self.armed.swap(false, std::sync::atomic::Ordering::SeqCst);
        if fail {
            return Err(crate::ImError::LocalStateUnavailable {
                detail: "injected live provider ref advance failure".to_owned(),
            });
        }
        self.inner.open(secret_ref)
    }

    fn delete(&self, secret_ref: &SecretRef) -> crate::ImResult<()> {
        self.inner.delete(secret_ref)
    }

    fn list(&self) -> crate::ImResult<Vec<SecretRef>> {
        self.inner.list()
    }
}

impl SecretVault for FailIndexCommitAfterTokenStageVault {
    fn seal(&self, request: SealSecretRequest) -> crate::ImResult<SecretRef> {
        let is_staged_auth = request.metadata.kind == SecretKind::AuthJwt
            && request.metadata.key_version > 1
            && self.armed.swap(false, std::sync::atomic::Ordering::SeqCst);
        let secret_ref = self.inner.seal(request)?;
        if is_staged_auth {
            std::fs::rename(&self.registry_path, &self.backup_path)?;
            std::fs::create_dir(&self.registry_path)?;
        }
        Ok(secret_ref)
    }

    fn seal_if_absent(&self, request: SealSecretRequest) -> crate::ImResult<SealIfAbsentResult> {
        self.inner.seal_if_absent(request)
    }

    fn open(&self, secret_ref: &SecretRef) -> crate::ImResult<SecretBytes> {
        self.inner.open(secret_ref)
    }

    fn delete(&self, secret_ref: &SecretRef) -> crate::ImResult<()> {
        self.inner.delete(secret_ref)
    }

    fn list(&self) -> crate::ImResult<Vec<SecretRef>> {
        self.inner.list()
    }
}

impl Scenario {
    fn vault_root_matches(
        &self,
        identity: &LocalVaultIdentity,
        root_ref: &SecretRef,
        expected: &[u8],
    ) -> bool {
        let opened = identity.vault.open(root_ref).unwrap();
        opened.expose_secret() == expected
    }
}

fn scenario() -> Scenario {
    let generated = crate::internal::identity_generation::generate_vnext_handle_identity_with_default_daemon_subkey(
        "awiki.test",
        "alice",
        None,
        None,
    )
    .unwrap();
    let recipient_device_id = crate::ids::ProtocolDeviceId::parse("dev-recipient").unwrap();
    let recipient_signing_private = anp::PrivateKeyMaterial::Ed25519(
        ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng),
    );
    let recipient_e2ee_private =
        anp::PrivateKeyMaterial::X25519(X25519StaticSecret::random_from_rng(rand::rngs::OsRng));
    let recipient_signing_key_id = format!(
        "{}#{}-sign",
        generated.did.as_str(),
        recipient_device_id.as_str()
    );
    let recipient_e2ee_key_id = format!(
        "{}#{}-e2ee",
        generated.did.as_str(),
        recipient_device_id.as_str()
    );
    let recipient_signing_method = json!({
        "id": recipient_signing_key_id,
        "type": "Multikey",
        "controller": generated.did.as_str(),
        "publicKeyMultibase": public_key_multibase(&recipient_signing_private.public_key()),
    });
    let recipient_e2ee_method = json!({
        "id": recipient_e2ee_key_id,
        "type": "X25519KeyAgreementKey2019",
        "controller": generated.did.as_str(),
        "publicKeyMultibase": public_key_multibase(&recipient_e2ee_private.public_key()),
    });
    let profiles = vec![
        anp::authentication::PROFILE_CORE_BINDING_V2.to_owned(),
        anp::authentication::PROFILE_IDENTITY_DISCOVERY_V2.to_owned(),
        anp::authentication::PROFILE_DIRECT_BASE_V2.to_owned(),
        anp::authentication::PROFILE_DIRECT_E2EE_V2.to_owned(),
        anp::authentication::PROFILE_GROUP_BASE_V2.to_owned(),
        anp::authentication::PROFILE_GROUP_E2EE_V2.to_owned(),
    ];
    let mut document = anp::authentication::add_device_to_did_document(
        &generated.did_document,
        &generated.root_key_id,
        &anp::authentication::DeviceManifestEntry {
            device_id: recipient_device_id.as_str().to_owned(),
            signing_key_id: recipient_signing_key_id.clone(),
            e2ee_key_id: recipient_e2ee_key_id.clone(),
            profiles,
        },
        &recipient_signing_method,
        &recipient_e2ee_method,
        &[],
    )
    .unwrap();
    crate::internal::identity_daemon_subkey::resign_did_document_with_key1(
        &mut document,
        &generated.did,
        &generated.root_private_pem,
    )
    .unwrap();
    assert!(anp::authentication::validate_did_document_binding(
        &document, true
    ));
    let document_hash =
        crate::internal::identity_wire::device_genesis::document_hash(&document).unwrap();
    let checkpoint = IdentityInternalCheckpoint {
        document_version: 2,
        document_hash,
        registry_version: 2,
    };
    let registry = DeviceJoinRemoteRegistry {
        did: generated.did.clone(),
        checkpoint: checkpoint.clone(),
        devices: vec![
            DeviceJoinRemoteDeviceSummary {
                device_id: generated.protocol_device_id.as_str().to_owned(),
                signing_key_id: generated.device_signing_key_id.clone(),
                e2ee_key_id: generated.device_e2ee_key_id.clone(),
                status: DeviceAuthorizationStatus::Active,
                role: DeviceAuthorizationRole::Admin,
                management_ready: true,
                auth_generation: 1,
            },
            DeviceJoinRemoteDeviceSummary {
                device_id: recipient_device_id.as_str().to_owned(),
                signing_key_id: recipient_signing_key_id.clone(),
                e2ee_key_id: recipient_e2ee_key_id.clone(),
                status: DeviceAuthorizationStatus::Active,
                role: DeviceAuthorizationRole::Admin,
                management_ready: false,
                auth_generation: 1,
            },
        ],
        pending_join_requests: Vec::new(),
    };
    let sender = local_identity(
        "sender-identity",
        &generated,
        &document,
        &checkpoint,
        generated.protocol_device_id.clone(),
        generated.device_signing_key_id.clone(),
        generated.device_e2ee_key_id.clone(),
        generated.device_signing_private_pem.clone(),
        generated.device_e2ee_private_pem.clone(),
        Some(generated.root_private_pem.clone()),
        true,
        31,
    );
    let recipient_signing_private_pem = recipient_signing_private.to_pem();
    let recipient_e2ee_private_pem = recipient_e2ee_private.to_pem();
    let receiver = local_identity(
        "receiver-identity",
        &generated,
        &document,
        &checkpoint,
        recipient_device_id.clone(),
        recipient_signing_key_id.clone(),
        recipient_e2ee_key_id.clone(),
        recipient_signing_private_pem.clone(),
        recipient_e2ee_private_pem.clone(),
        None,
        false,
        47,
    );
    let now = OffsetDateTime::parse("2026-07-19T02:00:00Z", &Rfc3339).unwrap();
    Scenario {
        generated,
        document,
        registry,
        sender,
        receiver,
        recipient_device_id: recipient_device_id.as_str().to_owned(),
        recipient_signing_key_id,
        recipient_e2ee_key_id,
        recipient_signing_private_pem,
        recipient_e2ee_private_pem,
        now,
        expires_at: now + Duration::minutes(2),
    }
}

fn add_ready_admin(
    scenario: &Scenario,
    device_id: &str,
) -> (
    Value,
    DeviceJoinRemoteRegistry,
    DeviceJoinRemoteDeviceSummary,
) {
    let signing_private = anp::PrivateKeyMaterial::Ed25519(ed25519_dalek::SigningKey::generate(
        &mut rand::rngs::OsRng,
    ));
    let e2ee_private =
        anp::PrivateKeyMaterial::X25519(X25519StaticSecret::random_from_rng(rand::rngs::OsRng));
    let signing_key_id = format!("{}#{device_id}-sign", scenario.generated.did.as_str());
    let e2ee_key_id = format!("{}#{device_id}-e2ee", scenario.generated.did.as_str());
    let entry = anp::authentication::DeviceManifestEntry {
        device_id: device_id.to_owned(),
        signing_key_id: signing_key_id.clone(),
        e2ee_key_id: e2ee_key_id.clone(),
        profiles: vec![
            anp::authentication::PROFILE_CORE_BINDING_V2.to_owned(),
            anp::authentication::PROFILE_IDENTITY_DISCOVERY_V2.to_owned(),
            anp::authentication::PROFILE_DIRECT_BASE_V2.to_owned(),
            anp::authentication::PROFILE_DIRECT_E2EE_V2.to_owned(),
            anp::authentication::PROFILE_GROUP_BASE_V2.to_owned(),
            anp::authentication::PROFILE_GROUP_E2EE_V2.to_owned(),
        ],
    };
    let signing_method = json!({
        "id": signing_key_id,
        "type": "Multikey",
        "controller": scenario.generated.did.as_str(),
        "publicKeyMultibase": public_key_multibase(&signing_private.public_key()),
    });
    let e2ee_method = json!({
        "id": e2ee_key_id,
        "type": "X25519KeyAgreementKey2019",
        "controller": scenario.generated.did.as_str(),
        "publicKeyMultibase": public_key_multibase(&e2ee_private.public_key()),
    });
    let mut document = anp::authentication::add_device_to_did_document(
        &scenario.document,
        &scenario.generated.root_key_id,
        &entry,
        &signing_method,
        &e2ee_method,
        &[],
    )
    .unwrap();
    crate::internal::identity_daemon_subkey::resign_did_document_with_key1(
        &mut document,
        &scenario.generated.did,
        &scenario.generated.root_private_pem,
    )
    .unwrap();
    let device = DeviceJoinRemoteDeviceSummary {
        device_id: device_id.to_owned(),
        signing_key_id,
        e2ee_key_id,
        status: DeviceAuthorizationStatus::Active,
        role: DeviceAuthorizationRole::Admin,
        management_ready: true,
        auth_generation: 1,
    };
    let mut registry = scenario.registry.clone();
    registry.devices.push(device.clone());
    registry.checkpoint.document_version += 1;
    registry.checkpoint.registry_version += 1;
    registry.checkpoint.document_hash =
        crate::internal::identity_wire::device_genesis::document_hash(&document).unwrap();
    (document, registry, device)
}

#[allow(clippy::too_many_arguments)]
fn local_identity(
    unique_id: &str,
    generated: &GeneratedVNextIdentityWithDaemonSubkey,
    document: &Value,
    checkpoint: &IdentityInternalCheckpoint,
    protocol_device_id: crate::ids::ProtocolDeviceId,
    signing_key_id: String,
    e2ee_key_id: String,
    signing_private_pem: String,
    e2ee_private_pem: String,
    root_private_pem: Option<String>,
    management_ready: bool,
    vault_key_byte: u8,
) -> LocalVaultIdentity {
    let root = tempfile::tempdir().unwrap();
    let paths = crate::paths::IdentityRegistryPaths {
        identity_root_dir: root.path().join("identities"),
        registry_path: root.path().join("identities").join("index.json"),
        default_identity_path: Some(root.path().join("identities").join("default")),
    };
    let vault = Arc::new(FileSecretVault::new(
        DeviceVaultRootKey::from_bytes([vault_key_byte; 32]),
        FileSecretVaultStore::new(root.path().join("vault")),
    ));
    let storage = SaveIdentitySecretStorage::Vault {
        workspace_id: "workspace-root-transfer".to_owned(),
        device_id: format!("vault-context-{unique_id}"),
        vault: vault.clone(),
    };
    IdentityStore::new(&paths)
        .save_identity_with_secret_storage(
            SaveIdentityInput {
                local_alias: LOCAL_ALIAS.to_owned(),
                did: generated.did.clone(),
                unique_id: unique_id.to_owned(),
                user_id: "user-1".to_owned(),
                display_name: "Alice".to_owned(),
                handle: "alice".to_owned(),
                full_handle: "alice.awiki.test".to_owned(),
                jwt_token: "device-token".to_owned(),
                did_document: Some(document.clone()),
                key_mode: SaveIdentityKeyMode::VNext {
                    root_key_id: generated.root_key_id.clone(),
                    device_signing_key_id: signing_key_id.clone(),
                    device_e2ee_key_id: e2ee_key_id.clone(),
                },
                device_state: Some(IdentityDeviceState {
                    schema_version: IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
                    mode: IdentityDeviceMode::VNext,
                    authorization: Some(DeviceAuthorizationProjection {
                        protocol_device_id,
                        signing_key_id,
                        e2ee_key_id,
                        status: DeviceAuthorizationStatus::Active,
                        role: DeviceAuthorizationRole::Admin,
                        management_ready,
                        auth_generation: 1,
                    }),
                    checkpoint: Some(checkpoint.clone()),
                }),
                key1_private_pem: root_private_pem.unwrap_or_default(),
                key1_public_pem: generated.root_public_pem.clone(),
                e2ee_signing_private_pem: signing_private_pem,
                e2ee_agreement_private_pem: e2ee_private_pem,
                daemon_subkey_package: None,
                make_default: true,
            },
            storage.clone(),
        )
        .unwrap();
    LocalVaultIdentity {
        _root: root,
        paths,
        storage,
        vault,
    }
}

fn public_key_multibase(public_key: &anp::PublicKeyMaterial) -> String {
    let (codec, bytes): ([u8; 2], Vec<u8>) = match public_key {
        anp::PublicKeyMaterial::Ed25519(key) => ([0xed, 0x01], key.to_bytes().to_vec()),
        anp::PublicKeyMaterial::X25519(key) => ([0xec, 0x01], key.to_vec()),
        _ => panic!("test uses only Ed25519/X25519"),
    };
    let mut encoded = codec.to_vec();
    encoded.extend(bytes);
    format!("z{}", bs58::encode(encoded).into_string())
}

fn vault_record_images(vault: &FileSecretVault) -> Vec<Vec<u8>> {
    vault
        .list()
        .unwrap()
        .into_iter()
        .map(|secret_ref| std::fs::read(vault.store().record_path(&secret_ref)).unwrap())
        .collect()
}
