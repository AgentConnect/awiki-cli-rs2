use super::*;
use serde_json::json;

fn core(root: &std::path::Path) -> crate::ImCore {
    crate::ImCore::new_with_options(
        crate::ImCoreConfig::new(
            crate::ServiceEndpoint::parse("https://example.test").unwrap(),
            "example.test",
        )
        .unwrap(),
        crate::ImCorePaths {
            identities: crate::IdentityRegistryPaths {
                identity_root_dir: root.join("identities"),
                registry_path: root.join("identities/registry.json"),
                default_identity_path: None,
            },
            local_state: crate::LocalStatePaths {
                sqlite_path: root.join("local.sqlite"),
            },
            runtime: crate::RuntimePaths {
                cache_dir: root.join("cache"),
                temp_dir: root.join("tmp"),
            },
        },
        crate::ImCoreOpenOptions::default().with_identity_secret_vault(
            crate::IdentitySecretStoragePolicy::VaultRequired,
            crate::ImCoreSecretVaultOptions::new(
                crate::internal::platform_secret::DeviceVaultRootKey::from_bytes([41; 32]),
                root.join("vault"),
                "test-workspace",
                "test-device",
            ),
        ),
    )
    .unwrap()
}

fn registry(root: &std::path::Path, store: serde_json::Value) {
    std::fs::create_dir_all(root.join("identities")).unwrap();
    std::fs::write(root.join("identities/registry.json"), serde_json::to_vec(&json!({
        "schema_version": 5, "default_credential_name": "alice", "credentials": {
            "alice": { "credential_name": "alice", "dir_name": "alice-id", "did": "did:wba:example.test:alice",
                "unique_id": "alice-id", "user_id": "user-alice", "name": "Alice", "handle": "alice",
                "full_handle": "alice.example.test", "is_default": true, "identity_custody_backend": "anp_identity",
                "anp_identity_store_id": store, "anp_identity_id": "custody-alice" }
        }
    })).unwrap()).unwrap();
}

#[test]
fn local_reset_collects_exact_registry_binding_and_preserves_it_across_reopen() {
    let root = tempfile::tempdir().unwrap();
    let instance = core(root.path());
    assert!(instance
        .identities()
        .local_provider_identity_references()
        .unwrap()
        .is_empty());
    registry(root.path(), json!("shared-provider"));
    let expected = vec![ProviderIdentityRef {
        store_id: "shared-provider".into(),
        identity_id: "custody-alice".into(),
        did: "did:wba:example.test:alice".into(),
    }];
    assert_eq!(
        instance
            .identities()
            .local_provider_identity_references()
            .unwrap(),
        expected
    );
    drop(instance);
    assert_eq!(
        core(root.path())
            .identities()
            .local_provider_identity_references()
            .unwrap(),
        expected
    );
}

#[test]
fn local_reset_rejects_incomplete_registry_and_corrupt_join_evidence() {
    let root = tempfile::tempdir().unwrap();
    let instance = core(root.path());
    registry(root.path(), serde_json::Value::Null);
    assert!(instance
        .identities()
        .local_provider_identity_references()
        .is_err());
    registry(root.path(), json!("shared-provider"));
    std::fs::create_dir_all(root.path().join("identities/.device-join")).unwrap();
    std::fs::write(
        root.path()
            .join("identities/.device-join/broken.creation-journal"),
        b"{}",
    )
    .unwrap();
    assert!(instance
        .identities()
        .local_provider_identity_references()
        .is_err());
    assert!(root.path().join("identities/registry.json").exists());
}

#[test]
fn local_reset_includes_unprojected_registration_custody_without_enumerating_the_provider() {
    let root = tempfile::tempdir().unwrap();
    let instance = core(root.path());
    let create = crate::internal::identity_generation::vnext_handle_anp_identity_create_spec(
        "example.test",
        "alice",
        None,
        None,
    )
    .unwrap();
    let provider_root = tempfile::tempdir().unwrap();
    let mut manager =
        anp_identity::IdentityManager::initialize(anp_identity::IdentityManagerConfig {
            state_root: provider_root.path().to_owned(),
            root_key: anp_identity::RootKeySource::LocalPrivateFile,
        })
        .unwrap();
    let generated = manager
        .create(crate::internal::identity_custody::native_create_spec(
            create.spec,
        ))
        .unwrap();
    let public = generated.public_identity().unwrap();
    let manifest = anp::authentication::validate_device_manifest(public.document.as_value())
        .unwrap()
        .unwrap();
    let device = &manifest.devices[0];
    let expected = ProviderIdentityRef {
        store_id: public.reference.store_id.clone(),
        identity_id: public.reference.identity_id.clone(),
        did: public.reference.did.clone(),
    };
    let pending = PendingRegistration::new(
        "alice".into(),
        "example.test".into(),
        "alice".into(),
        "Alice".into(),
        true,
        "already_verified".into(),
        None,
        None,
        crate::internal::identity_registration_pending::PendingRegistrationIdentity {
            controller_store_id: expected.store_id.clone(),
            controller_identity_id: expected.identity_id.clone(),
            did: crate::ids::Did::parse(&expected.did).unwrap(),
            did_document: public.document.into_value(),
            protocol_device_id: crate::ids::ProtocolDeviceId::parse(&device.device_id).unwrap(),
            root_key_id: format!("{}#key-1", expected.did),
            device_signing_key_id: device.signing_key_id.clone(),
            device_e2ee_key_id: device.e2ee_key_id.clone(),
            legacy_daemon_authorization: false,
            controller_revision_id: None,
        },
    )
    .unwrap();
    PendingRegistrationStore::from_core(&instance)
        .unwrap()
        .save(&pending)
        .unwrap();
    assert!(instance.identities().list().unwrap().is_empty());
    assert_eq!(
        instance
            .identities()
            .local_provider_identity_references()
            .unwrap(),
        vec![expected]
    );
}

#[test]
fn local_reset_includes_uncommitted_recovery_custody() {
    use crate::internal::identity_handle_recovery_pending::HandleRecoveryIdentityRef;
    let root = tempfile::tempdir().unwrap();
    let instance = core(root.path());
    let generated = crate::internal::identity_generation::generate_handle_recovery_identity(
        "example.invalid",
        "alice",
        None,
        None,
    )
    .unwrap();
    let expected = ProviderIdentityRef {
        store_id: "recovery-store".into(),
        identity_id: "recovery-identity".into(),
        did: generated.did.as_str().to_owned(),
    };
    let identity = HandleRecoveryIdentityRef {
        store_id: expected.store_id.clone(),
        identity_id: expected.identity_id.clone(),
        did: generated.did,
        did_document: generated.did_document,
        protocol_device_id: generated.protocol_device_id,
        root_key_id: generated.root_key_id,
        device_signing_key_id: generated.device_signing_key_id,
        device_e2ee_key_id: generated.device_e2ee_key_id,
    };
    let pending = PendingHandleRecoveryV4::new_pre_otp(
        "op_v4_12345678".into(),
        "owner-1".into(),
        "alice".into(),
        "Alice".into(),
        true,
        false,
        "alice.example.invalid".into(),
        "did:wba:example.invalid:user:alice:old".into(),
        identity,
    )
    .unwrap();
    PendingHandleRecoveryStore::from_core(&instance)
        .unwrap()
        .create_v4(&pending)
        .unwrap();
    assert!(instance.identities().list().unwrap().is_empty());
    assert_eq!(
        instance
            .identities()
            .local_provider_identity_references()
            .unwrap(),
        vec![expected]
    );
}
