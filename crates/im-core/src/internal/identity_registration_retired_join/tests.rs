use super::*;

fn test_config() -> crate::ImCoreConfig {
    crate::ImCoreConfig {
        service_base_url: crate::ServiceEndpoint::parse("https://example.invalid").unwrap(),
        did_domain: "example.invalid".to_owned(),
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

fn save_registry_entry(paths: &crate::ImCorePaths, record: &RetiredJoinRollover, device_id: &str) {
    use crate::internal::identity_device_state::{
        DeviceAuthorizationProjection, DeviceAuthorizationRole, DeviceAuthorizationStatus,
        IdentityDeviceMode, IdentityDeviceState, IdentityInternalCheckpoint,
        IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
    };

    write_retirement_marker(paths, record);
    let store = crate::internal::identity_store::IdentityStore::new(&paths.identities);
    let lock = store.lock_index_mutation().unwrap();
    let mut index = store.load_index().unwrap();
    index.credentials.insert(
        "alice".to_owned(),
        crate::internal::identity_store::IndexEntry {
            credential_name: "alice".to_owned(),
            dir_name: "owner-alice".to_owned(),
            did: record.current_did.clone(),
            unique_id: record.owner_identity_id.clone(),
            user_id: record.account_user_id.clone(),
            name: "Alice".to_owned(),
            handle: "alice".to_owned(),
            full_handle: record.handle.clone(),
            binding_generation: Some(record.current_binding_generation.clone()),
            is_default: true,
            device_state: Some(IdentityDeviceState {
                schema_version: IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
                mode: IdentityDeviceMode::VNext,
                authorization: Some(DeviceAuthorizationProjection {
                    protocol_device_id: crate::ids::ProtocolDeviceId::parse(device_id).unwrap(),
                    signing_key_id: format!("{}#signing", record.current_did),
                    e2ee_key_id: format!("{}#agreement", record.current_did),
                    status: DeviceAuthorizationStatus::Active,
                    role: DeviceAuthorizationRole::Member,
                    management_ready: false,
                    auth_generation: 4,
                }),
                checkpoint: Some(IdentityInternalCheckpoint {
                    document_version: 1,
                    document_hash: format!("sha256:{}", "A".repeat(43)),
                    registry_version: 1,
                }),
            }),
            ..Default::default()
        },
    );
    index.default_credential_name = "alice".to_owned();
    store.save_index_locked(&lock, index).unwrap();
}

fn write_retirement_marker(paths: &crate::ImCorePaths, record: &RetiredJoinRollover) {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use sha2::{Digest as _, Sha256};

    let directory = paths
        .identities
        .identity_root_dir
        .join(".identity-retirements");
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join(format!(
        "{}.json",
        URL_SAFE_NO_PAD.encode(Sha256::digest(record.owner_identity_id.as_bytes()))
    ));
    std::fs::write(
        path,
        serde_json::to_vec(&serde_json::json!({
            "schema_version": 1,
            "identity_id": record.owner_identity_id,
            "did": record.retired_did,
            "local_alias": "alice",
            "identity_dir_name": "owner-alice",
            "protocol_device_id": record.retired_device_id,
            "phase": "completed"
        }))
        .unwrap(),
    )
    .unwrap();
}

#[test]
fn retired_registration_join_journal_round_trips_prepared_evidence() {
    let root = tempfile::tempdir().unwrap();
    let sqlite_path = root.path().join("im.sqlite");
    let transition =
        crate::internal::identity_registration_join_preparation::RegistrationJoinTransition {
            account_user_id: "account-alice".to_owned(),
            previous_did: "did:wba:example.invalid:user:alice:e1_old".to_owned(),
            current_did: "did:wba:example.invalid:user:alice:e1_new".to_owned(),
            binding_generation: "8".to_owned(),
        };
    let evidence = crate::internal::identity_local_owner_matcher::RetiredOwnerEvidence {
        owner_identity_id: "owner-alice".to_owned(),
        retired_did: transition.previous_did.clone(),
        retired_protocol_device_id: "dev-retired".to_owned(),
        retired_binding_generation: "7".to_owned(),
        epoch_relation:
            crate::internal::identity_local_owner_matcher::RetiredOwnerEpochRelation::DirectPrevious,
    };
    let record = RetiredJoinRollover::prepared(
        "join-session",
        &transition.account_user_id,
        "alice.example.invalid",
        &transition,
        &evidence,
        "dev-new",
        "2099-08-28T12:00:00Z",
    )
    .unwrap();

    insert_prepared(&sqlite_path, &record).unwrap();
    assert_eq!(load(&sqlite_path, "join-session").unwrap(), Some(record));
}

fn insert_retired_binding(sqlite_path: &Path, record: &RetiredJoinRollover) {
    let connection = crate::internal::local_state::open_writable(sqlite_path).unwrap();
    connection
        .execute(
            r#"INSERT INTO identity_account_bindings
(owner_identity_id,account_id,handle_scope,current_did,device_id,
 identity_generation,device_auth_generation,created_at,updated_at)
VALUES (?1,?2,?3,?4,?5,?6,'3',1,1)"#,
            rusqlite::params![
                record.owner_identity_id,
                record.account_user_id,
                record.handle,
                record.retired_did,
                record.retired_device_id,
                record.retired_binding_generation,
            ],
        )
        .unwrap();
}

fn prepared_record(retired_current: bool) -> RetiredJoinRollover {
    let transition =
        crate::internal::identity_registration_join_preparation::RegistrationJoinTransition {
            account_user_id: "account-alice".to_owned(),
            previous_did: "did:wba:example.invalid:user:alice:e1_old".to_owned(),
            current_did: "did:wba:example.invalid:user:alice:e1_new".to_owned(),
            binding_generation: "8".to_owned(),
        };
    let evidence = crate::internal::identity_local_owner_matcher::RetiredOwnerEvidence {
        owner_identity_id: "owner-alice".to_owned(),
        retired_did: if retired_current {
            transition.current_did.clone()
        } else {
            transition.previous_did.clone()
        },
        retired_protocol_device_id: "dev-retired".to_owned(),
        retired_binding_generation: if retired_current { "8" } else { "7" }.to_owned(),
        epoch_relation: if retired_current {
            crate::internal::identity_local_owner_matcher::RetiredOwnerEpochRelation::Current
        } else {
            crate::internal::identity_local_owner_matcher::RetiredOwnerEpochRelation::DirectPrevious
        },
    };
    RetiredJoinRollover::prepared(
        "join-session",
        &transition.account_user_id,
        "alice.example.invalid",
        &transition,
        &evidence,
        "dev-new",
        "2099-08-28T12:00:00Z",
    )
    .unwrap()
}

#[test]
fn retired_registration_join_accepts_same_did_same_generation() {
    prepared_record(true).validate().unwrap();
}

#[test]
fn retired_registration_join_rejects_same_did_mismatched_generation() {
    let mut record = prepared_record(true);
    record.retired_binding_generation = "7".to_owned();
    assert!(matches!(
        record.validate(),
        Err(crate::ImError::PermissionDenied)
    ));
}

#[test]
fn retired_registration_join_rolls_current_binding_to_new_device() {
    let root = tempfile::tempdir().unwrap();
    let sqlite_path = root.path().join("im.sqlite");
    let record = prepared_record(true);
    insert_retired_binding(&sqlite_path, &record);
    insert_prepared(&sqlite_path, &record).unwrap();

    converge_after_registry_save(&sqlite_path, &record, 4).unwrap();

    let connection = crate::internal::local_state::open_writable(&sqlite_path).unwrap();
    let binding = connection
        .query_row(
            "SELECT owner_identity_id,current_did,device_id,identity_generation,device_auth_generation FROM identity_account_bindings",
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?, row.get::<_, String>(4)?)),
        )
        .unwrap();
    assert_eq!(
        binding,
        (
            "owner-alice".to_owned(),
            record.current_did.clone(),
            "dev-new".to_owned(),
            "8".to_owned(),
            "4".to_owned(),
        )
    );
    assert_eq!(
        load(&sqlite_path, "join-session").unwrap().unwrap().phase,
        RetiredJoinRolloverPhase::Completed
    );
}

#[test]
fn retired_registration_join_reuses_stable_owner_identity_id() {
    let root = tempfile::tempdir().unwrap();
    let sqlite_path = root.path().join("im.sqlite");
    let record = prepared_record(false);
    insert_retired_binding(&sqlite_path, &record);
    insert_prepared(&sqlite_path, &record).unwrap();

    converge_after_registry_save(&sqlite_path, &record, 4).unwrap();

    let connection = crate::internal::local_state::open_writable(&sqlite_path).unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT owner_identity_id FROM identity_account_bindings",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        record.owner_identity_id
    );
}

#[test]
fn retired_registration_join_retires_old_write_state_and_preserves_business_rows() {
    let root = tempfile::tempdir().unwrap();
    let sqlite_path = root.path().join("im.sqlite");
    let record = prepared_record(false);
    insert_retired_binding(&sqlite_path, &record);
    let connection = crate::internal::local_state::open_writable(&sqlite_path).unwrap();
    connection
        .execute(
            "INSERT INTO contacts(owner_identity_id,owner_did,did,credential_name) VALUES (?1,?2,'did:wba:example.invalid:user:bob:e1_peer','alice')",
            rusqlite::params![record.owner_identity_id, record.retired_did],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO direct_e2ee_signed_prekeys(owner_identity_id,owner_did,key_id,private_key_blob,status,created_at,updated_at) VALUES (?1,?2,'key-old',X'01','active','old','old')",
            rusqlite::params![record.owner_identity_id, record.retired_did],
        )
        .unwrap();
    drop(connection);
    insert_prepared(&sqlite_path, &record).unwrap();

    converge_after_registry_save(&sqlite_path, &record, 4).unwrap();

    let connection = crate::internal::local_state::open_writable(&sqlite_path).unwrap();
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM contacts", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT status FROM direct_e2ee_signed_prekeys WHERE key_id='key-old'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "retired"
    );
}

#[test]
fn retired_registration_join_rolls_previous_binding_to_current_epoch() {
    let root = tempfile::tempdir().unwrap();
    let sqlite_path = root.path().join("im.sqlite");
    let record = prepared_record(false);
    insert_retired_binding(&sqlite_path, &record);
    insert_prepared(&sqlite_path, &record).unwrap();

    converge_after_registry_save(&sqlite_path, &record, 4).unwrap();

    let connection = crate::internal::local_state::open_writable(&sqlite_path).unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT current_did || '|' || identity_generation FROM identity_account_bindings",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        format!("{}|8", record.current_did)
    );
}

#[test]
fn retired_registration_join_recovers_after_registry_save_before_binding_commit() {
    let root = tempfile::tempdir().unwrap();
    let paths = test_paths(root.path());
    let record = prepared_record(false);
    insert_retired_binding(&paths.local_state.sqlite_path, &record);
    insert_prepared(&paths.local_state.sqlite_path, &record).unwrap();
    save_registry_entry(&paths, &record, &record.new_device_id);

    let core = crate::ImCore::new(test_config(), paths.clone()).unwrap();
    drop(core);

    let completed = load(&paths.local_state.sqlite_path, &record.join_session_id)
        .unwrap()
        .unwrap();
    assert_eq!(completed.phase, RetiredJoinRolloverPhase::Completed);
    let connection =
        crate::internal::local_state::open_writable(&paths.local_state.sqlite_path).unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT current_did || '|' || device_id || '|' || identity_generation FROM identity_account_bindings WHERE owner_identity_id='owner-alice'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        format!("{}|{}|8", record.current_did, record.new_device_id)
    );
}

#[test]
fn retired_registration_join_selects_unique_registry_device_winner() {
    let root = tempfile::tempdir().unwrap();
    let paths = test_paths(root.path());
    let loser = prepared_record(false);
    let mut winner = loser.clone();
    winner.join_session_id = "join-winner".to_owned();
    winner.new_device_id = "dev-winner".to_owned();
    winner.validate().unwrap();
    insert_retired_binding(&paths.local_state.sqlite_path, &winner);
    insert_prepared(&paths.local_state.sqlite_path, &loser).unwrap();
    insert_prepared(&paths.local_state.sqlite_path, &winner).unwrap();
    save_registry_entry(&paths, &winner, &winner.new_device_id);

    let core = crate::ImCore::new(test_config(), paths.clone()).unwrap();
    drop(core);

    assert_eq!(
        load(&paths.local_state.sqlite_path, &winner.join_session_id)
            .unwrap()
            .unwrap()
            .phase,
        RetiredJoinRolloverPhase::Completed
    );
    assert_eq!(
        load(&paths.local_state.sqlite_path, &loser.join_session_id)
            .unwrap()
            .unwrap()
            .phase,
        RetiredJoinRolloverPhase::Prepared
    );
}

#[test]
fn retired_registration_join_cleans_terminal_or_expired_orphan() {
    let root = tempfile::tempdir().unwrap();
    let paths = test_paths(root.path());
    let mut record = prepared_record(false);
    record.join_expires_at = "2020-01-01T00:00:00Z".to_owned();
    record.validate().unwrap();
    insert_retired_binding(&paths.local_state.sqlite_path, &record);
    insert_prepared(&paths.local_state.sqlite_path, &record).unwrap();
    write_retirement_marker(&paths, &record);

    let core = crate::ImCore::new(test_config(), paths.clone()).unwrap();
    drop(core);

    assert!(
        load(&paths.local_state.sqlite_path, &record.join_session_id)
            .unwrap()
            .is_none()
    );
}

#[test]
fn retired_registration_join_rejects_zero_or_multiple_winner() {
    for multiple in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let paths = test_paths(root.path());
        let record = prepared_record(false);
        insert_retired_binding(&paths.local_state.sqlite_path, &record);
        insert_prepared(&paths.local_state.sqlite_path, &record).unwrap();
        if multiple {
            let mut duplicate = record.clone();
            duplicate.join_session_id = "join-duplicate".to_owned();
            insert_prepared(&paths.local_state.sqlite_path, &duplicate).unwrap();
            save_registry_entry(&paths, &record, &record.new_device_id);
        } else {
            save_registry_entry(&paths, &record, "dev-other");
        }

        assert!(crate::ImCore::new(test_config(), paths).is_err());
    }
}

#[test]
fn retired_registration_join_completed_journal_supersedes_exact_old_tombstone() {
    let root = tempfile::tempdir().unwrap();
    let paths = test_paths(root.path());
    let record = prepared_record(false);
    insert_retired_binding(&paths.local_state.sqlite_path, &record);
    insert_prepared(&paths.local_state.sqlite_path, &record).unwrap();
    save_registry_entry(&paths, &record, &record.new_device_id);
    let core = crate::ImCore::new(test_config(), paths).unwrap();

    assert!(completed_rollover_supersedes_retirement(
        &core,
        &record.owner_identity_id,
        &record.retired_did,
        &record.retired_device_id,
    )
    .unwrap());
}

#[test]
fn retired_registration_join_old_journal_does_not_supersede_new_retirement() {
    let root = tempfile::tempdir().unwrap();
    let paths = test_paths(root.path());
    let record = prepared_record(false);
    insert_retired_binding(&paths.local_state.sqlite_path, &record);
    insert_prepared(&paths.local_state.sqlite_path, &record).unwrap();
    save_registry_entry(&paths, &record, &record.new_device_id);
    let core = crate::ImCore::new(test_config(), paths).unwrap();

    assert!(!completed_rollover_supersedes_retirement(
        &core,
        &record.owner_identity_id,
        &record.current_did,
        &record.new_device_id,
    )
    .unwrap());
    let paths = core.inner().sdk_paths().clone();
    let store = crate::internal::identity_store::IdentityStore::new(&paths.identities);
    let lock = store.lock_index_mutation().unwrap();
    store
        .save_index_locked(
            &lock,
            crate::internal::identity_store::IndexPayload::default(),
        )
        .unwrap();
    let mut new_retirement = record.clone();
    new_retirement.retired_did = record.current_did.clone();
    new_retirement.retired_device_id = record.new_device_id.clone();
    write_retirement_marker(&paths, &new_retirement);
    drop(core);

    crate::ImCore::new(test_config(), paths).unwrap();
}

#[test]
fn retired_registration_join_preserves_sibling_identity_state() {
    let root = tempfile::tempdir().unwrap();
    let paths = test_paths(root.path());
    let record = prepared_record(false);
    insert_retired_binding(&paths.local_state.sqlite_path, &record);
    let connection =
        crate::internal::local_state::open_writable(&paths.local_state.sqlite_path).unwrap();
    connection
        .execute(
            "INSERT INTO identity_account_bindings(owner_identity_id,account_id,handle_scope,current_did,device_id,identity_generation,device_auth_generation,created_at,updated_at) VALUES ('owner-sibling','account-sibling','sibling.example.invalid','did:wba:example.invalid:user:sibling:e1_current','dev-sibling','3','2',1,1)",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO contacts(owner_identity_id,owner_did,did,credential_name) VALUES ('owner-sibling','did:wba:example.invalid:user:sibling:e1_current','did:wba:example.invalid:user:peer:e1_peer','sibling')",
            [],
        )
        .unwrap();
    drop(connection);
    insert_prepared(&paths.local_state.sqlite_path, &record).unwrap();
    save_registry_entry(&paths, &record, &record.new_device_id);
    let store = crate::internal::identity_store::IdentityStore::new(&paths.identities);
    let lock = store.lock_index_mutation().unwrap();
    let mut index = store.load_index().unwrap();
    index.credentials.insert(
        "sibling".to_owned(),
        crate::internal::identity_store::IndexEntry {
            did: "did:wba:example.invalid:user:sibling:e1_current".to_owned(),
            unique_id: "owner-sibling".to_owned(),
            user_id: "account-sibling".to_owned(),
            full_handle: "sibling.example.invalid".to_owned(),
            binding_generation: Some("3".to_owned()),
            ..Default::default()
        },
    );
    store.save_index_locked(&lock, index).unwrap();

    let core = crate::ImCore::new(test_config(), paths.clone()).unwrap();
    drop(core);

    let committed_index = store.load_index().unwrap();
    assert_eq!(
        committed_index.credentials["sibling"].unique_id,
        "owner-sibling"
    );
    let connection =
        crate::internal::local_state::open_writable(&paths.local_state.sqlite_path).unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT current_did || '|' || device_id || '|' || identity_generation FROM identity_account_bindings WHERE owner_identity_id='owner-sibling'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "did:wba:example.invalid:user:sibling:e1_current|dev-sibling|3"
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM contacts WHERE owner_identity_id='owner-sibling'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1
    );
}
