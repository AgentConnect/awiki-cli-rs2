use super::*;

use std::sync::Arc;

use crate::internal::identity_device_state::{
    DeviceAuthorizationProjection, DeviceAuthorizationRole, IdentityDeviceState,
    IdentityInternalCheckpoint, IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
};
use crate::internal::identity_store::{
    SaveIdentityInput, SaveIdentityKeyMode, SaveIdentitySecretStorage,
};
use crate::internal::secret_vault::record::SecretKind;
use crate::vault::{
    DeviceVaultRootKey, FileSecretVault, FileSecretVaultStore, SealSecretRequest,
    SecretAccessPolicy, SecretBytes, SecretMetadata, SecretVault,
};

#[test]
fn dry_run_is_secret_free_and_does_not_create_the_target_store() {
    let fixture = Fixture::file(&[("alice", true), ("bob", false)]);
    let before = fs::read(&fixture.paths.identities.registry_path).unwrap();

    let report = inspect(&fixture.core).unwrap();

    assert_eq!(report.phase, IdentityCustodyMigrationPhase::Eligible);
    assert_eq!(report.identities.len(), 2);
    assert!(report.identities.iter().all(|identity| identity.eligible));
    assert!(!serde_json::to_string(&report)
        .unwrap()
        .contains("PRIVATE KEY"));
    assert_eq!(
        fs::read(&fixture.paths.identities.registry_path).unwrap(),
        before
    );
    assert!(!fixture
        .paths
        .identities
        .identity_root_dir
        .join(".anp-identity")
        .exists());
    fixture.assert_legacy_private_files_exist();
}

#[test]
fn multi_identity_copy_failure_retries_before_atomic_cutover() {
    let fixture = Fixture::file(&[("alice", true), ("bob", false)]);

    assert!(run(&fixture.core, false, Some(FailurePoint::AfterCopied(1))).is_err());
    let staged = crate::internal::identity_custody::open_controller_store(&fixture.core).unwrap();
    assert_eq!(staged.list_identities().unwrap().len(), 1);
    let before_cutover = IdentityStore::new(&fixture.paths.identities)
        .load_index()
        .unwrap();
    assert_eq!(before_cutover.schema_version, 5);
    assert!(before_cutover.identity_custody_cutover.is_none());
    assert!(before_cutover
        .credentials
        .values()
        .all(|entry| entry.identity_custody_backend.is_none()));
    fixture.assert_legacy_private_files_exist();

    let report = migrate(&fixture.core).unwrap();

    assert_eq!(report.phase, IdentityCustodyMigrationPhase::Cleaned);
    assert_eq!(report.verified_count, 2);
    let index = IdentityStore::new(&fixture.paths.identities)
        .load_index()
        .unwrap();
    assert_eq!(
        index.schema_version,
        IDENTITY_CUSTODY_CUTOVER_INDEX_SCHEMA_VERSION
    );
    assert!(
        index
            .identity_custody_cutover
            .as_ref()
            .unwrap()
            .cleanup_complete
    );
    assert!(index.credentials.values().all(|entry| {
        entry.identity_custody_backend.as_deref() == Some(BACKEND)
            && entry.vault_migration.is_none()
    }));
    fixture.assert_legacy_private_files_removed();
    for alias in &fixture.aliases {
        fixture
            .core
            .client(crate::identity::IdentitySelector::LocalAlias(alias.clone()))
            .unwrap();
    }
}

#[test]
fn crash_after_marker_keeps_new_backend_live_and_cleanup_is_idempotent() {
    let fixture = Fixture::file(&[("alice", true)]);

    assert!(run(&fixture.core, false, Some(FailurePoint::AfterCutover)).is_err());
    let cutover = IdentityStore::new(&fixture.paths.identities)
        .load_index()
        .unwrap();
    assert_eq!(
        cutover.schema_version,
        IDENTITY_CUSTODY_CUTOVER_INDEX_SCHEMA_VERSION
    );
    assert!(
        !cutover
            .identity_custody_cutover
            .as_ref()
            .unwrap()
            .cleanup_complete
    );
    fixture.assert_legacy_private_files_exist();
    fixture
        .core
        .client(crate::identity::IdentitySelector::Default)
        .unwrap();

    let report = migrate(&fixture.core).unwrap();
    assert_eq!(report.phase, IdentityCustodyMigrationPhase::Cleaned);
    fixture.assert_legacy_private_files_removed();
    assert_eq!(
        migrate(&fixture.core).unwrap().phase,
        IdentityCustodyMigrationPhase::Cleaned
    );
}

#[test]
fn source_key_change_after_copy_prevents_cutover_and_keeps_legacy_files() {
    let fixture = Fixture::file(&[("alice", true)]);
    assert!(run(&fixture.core, false, Some(FailurePoint::AfterCopied(1))).is_err());
    let index = IdentityStore::new(&fixture.paths.identities)
        .load_index()
        .unwrap();
    let entry = &index.credentials["alice"];
    let replacement =
        anp::PrivateKeyMaterial::Ed25519(ed25519_dalek::SigningKey::from_bytes(&[99_u8; 32]))
            .to_pem();
    fs::write(
        fixture
            .paths
            .identities
            .identity_root_dir
            .join(&entry.dir_name)
            .join(SIGNING_PRIVATE_FILE),
        replacement,
    )
    .unwrap();

    let report = migrate(&fixture.core).unwrap();

    assert_eq!(report.phase, IdentityCustodyMigrationPhase::Blocked);
    assert!(IdentityStore::new(&fixture.paths.identities)
        .load_index()
        .unwrap()
        .identity_custody_cutover
        .is_none());
    fixture.assert_legacy_private_files_exist();
}

#[test]
fn vault_cleanup_preserves_auth_and_removes_only_identity_key_records() {
    let fixture = Fixture::vault(&[("alice", true), ("bob", false)]);
    let vault = fixture.vault.as_ref().unwrap();
    assert!(vault
        .list()
        .unwrap()
        .iter()
        .any(|item| item.kind == SecretKind::AuthJwt));

    let report = migrate(&fixture.core).unwrap();

    assert_eq!(report.phase, IdentityCustodyMigrationPhase::Cleaned);
    let remaining = vault.list().unwrap();
    assert!(remaining
        .iter()
        .any(|item| item.kind == SecretKind::AuthJwt));
    assert!(!remaining.iter().any(|item| {
        matches!(
            &item.kind,
            SecretKind::IdentityRootPrivate
                | SecretKind::IdentityDeviceSigningPrivate
                | SecretKind::IdentityE2eeSigningPrivate
                | SecretKind::IdentityE2eeAgreementPrivate
                | SecretKind::IdentityDaemonPrivate
        )
    }));
    fixture.assert_legacy_private_files_removed();
}

#[test]
fn unresolved_pending_record_blocks_cutover_without_deleting_keys() {
    let fixture = Fixture::vault(&[("alice", true)]);
    let vault = fixture.vault.as_ref().unwrap();
    vault
        .seal(SealSecretRequest {
            metadata: SecretMetadata {
                workspace_id: Fixture::WORKSPACE_ID.to_owned(),
                device_id: Fixture::VAULT_DEVICE_ID.to_owned(),
                identity_id: None,
                did: None,
                kind: SecretKind::IdentityHandleRecoveryPending,
                key_id: "pending-recovery".to_owned(),
                key_version: 1,
                policy: SecretAccessPolicy::no_prompt_local_secret(),
            },
            plaintext: SecretBytes::from_vec(b"pending".to_vec()),
        })
        .unwrap();

    let report = migrate(&fixture.core).unwrap();

    assert_eq!(report.phase, IdentityCustodyMigrationPhase::Blocked);
    assert!(report
        .blockers
        .iter()
        .any(|blocker| blocker.contains("identity.handle_recovery.pending")));
    let index = IdentityStore::new(&fixture.paths.identities)
        .load_index()
        .unwrap();
    assert!(index.identity_custody_cutover.is_none());
    fixture.assert_vault_identity_keys_exist();
}

struct Fixture {
    _root: tempfile::TempDir,
    paths: crate::ImCorePaths,
    core: crate::ImCore,
    aliases: Vec<String>,
    vault: Option<Arc<FileSecretVault>>,
}

impl Fixture {
    const VAULT_SEED: [u8; 32] = [98_u8; 32];
    const WORKSPACE_ID: &'static str = "identity-custody-migration-workspace";
    const VAULT_DEVICE_ID: &'static str = "identity-custody-migration-device";

    fn file(identities: &[(&str, bool)]) -> Self {
        Self::new(identities, false)
    }

    fn vault(identities: &[(&str, bool)]) -> Self {
        Self::new(identities, true)
    }

    fn new(identities: &[(&str, bool)], use_vault: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let paths = test_paths(root.path());
        let vault = use_vault.then(|| {
            Arc::new(FileSecretVault::new(
                DeviceVaultRootKey::from_bytes(Self::VAULT_SEED),
                FileSecretVaultStore::new(root.path().join("vault")),
            ))
        });
        for (index, (alias, rootful)) in identities.iter().enumerate() {
            save_legacy_identity(&paths, vault.clone(), alias, *rootful, index == 0);
        }
        let options = if use_vault {
            crate::ImCoreOpenOptions::default().with_identity_secret_vault(
                crate::IdentitySecretStoragePolicy::VaultRequired,
                crate::ImCoreSecretVaultOptions::new(
                    DeviceVaultRootKey::from_bytes(Self::VAULT_SEED),
                    root.path().join("vault"),
                    Self::WORKSPACE_ID,
                    Self::VAULT_DEVICE_ID,
                ),
            )
        } else {
            crate::ImCoreOpenOptions::file_compat()
        };
        let core = crate::ImCore::new_with_options(test_config(), paths.clone(), options).unwrap();
        Self {
            _root: root,
            paths,
            core,
            aliases: identities
                .iter()
                .map(|(alias, _)| (*alias).to_owned())
                .collect(),
            vault,
        }
    }

    fn assert_legacy_private_files_exist(&self) {
        let index = IdentityStore::new(&self.paths.identities)
            .load_index()
            .unwrap();
        for entry in index.credentials.values() {
            if self.vault.is_none() {
                let dir = self
                    .paths
                    .identities
                    .identity_root_dir
                    .join(&entry.dir_name);
                assert!(dir.join(SIGNING_PRIVATE_FILE).is_file());
                assert!(dir.join(AGREEMENT_PRIVATE_FILES[0]).is_file());
            }
        }
    }

    fn assert_legacy_private_files_removed(&self) {
        let index = IdentityStore::new(&self.paths.identities)
            .load_index()
            .unwrap();
        for entry in index.credentials.values() {
            let dir = self
                .paths
                .identities
                .identity_root_dir
                .join(&entry.dir_name);
            for name in ROOT_PRIVATE_FILES
                .iter()
                .chain([SIGNING_PRIVATE_FILE].iter())
                .chain(AGREEMENT_PRIVATE_FILES.iter())
                .chain(DAEMON_PRIVATE_FILES.iter())
            {
                assert!(!dir.join(name).exists(), "legacy file remains: {name}");
            }
        }
    }

    fn assert_vault_identity_keys_exist(&self) {
        assert!(self
            .vault
            .as_ref()
            .unwrap()
            .list()
            .unwrap()
            .iter()
            .any(|item| {
                matches!(
                    &item.kind,
                    SecretKind::IdentityRootPrivate
                        | SecretKind::IdentityDeviceSigningPrivate
                        | SecretKind::IdentityE2eeAgreementPrivate
                )
            }));
    }
}

fn save_legacy_identity(
    paths: &crate::ImCorePaths,
    vault: Option<Arc<FileSecretVault>>,
    alias: &str,
    rootful: bool,
    make_default: bool,
) {
    let generated =
        crate::internal::identity_generation::generate_vnext_handle_identity_with_default_daemon_subkey(
            "awiki.test",
            alias,
            None,
            None,
        )
        .unwrap();
    let document_hash =
        crate::internal::identity_wire::document::document_hash(&generated.did_document).unwrap();
    let input = SaveIdentityInput {
        local_alias: alias.to_owned(),
        did: generated.did.clone(),
        unique_id: generated.unique_id.clone(),
        user_id: format!("user-{alias}"),
        display_name: alias.to_owned(),
        handle: alias.to_owned(),
        full_handle: format!("{alias}.awiki.test"),
        binding_generation: Some("1".to_owned()),
        jwt_token: format!("token-{alias}"),
        did_document: Some(generated.did_document.clone()),
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
                role: if rootful {
                    DeviceAuthorizationRole::Admin
                } else {
                    DeviceAuthorizationRole::Member
                },
                management_ready: rootful,
                auth_generation: 1,
            }),
            checkpoint: Some(IdentityInternalCheckpoint {
                document_version: 1,
                document_hash,
                registry_version: 1,
            }),
        }),
        key1_private_pem: if rootful {
            generated.root_private_pem
        } else {
            String::new()
        },
        key1_public_pem: generated.root_public_pem,
        e2ee_signing_private_pem: generated.device_signing_private_pem,
        e2ee_agreement_private_pem: generated.device_e2ee_private_pem,
        daemon_subkey_package: Some(generated.daemon_subkey_package),
        make_default,
    };
    let storage = match vault {
        Some(vault) => SaveIdentitySecretStorage::Vault {
            workspace_id: Fixture::WORKSPACE_ID.to_owned(),
            device_id: Fixture::VAULT_DEVICE_ID.to_owned(),
            vault,
        },
        None => SaveIdentitySecretStorage::FileCompat,
    };
    IdentityStore::new(&paths.identities)
        .save_identity_with_secret_storage(input, storage)
        .unwrap();
}

fn test_paths(root: &Path) -> crate::ImCorePaths {
    crate::ImCorePaths {
        identities: crate::paths::IdentityRegistryPaths {
            identity_root_dir: root.join("identities"),
            registry_path: root.join("identities").join("index.json"),
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

fn test_config() -> crate::ImCoreConfig {
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
    }
}
