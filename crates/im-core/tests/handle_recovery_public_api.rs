use awiki_im_core::identity::{
    HandleRecoveryActivateRequest, HandleRecoveryErrorCode, HandleRecoveryOtpRequest,
    HandleRecoveryPhase, HandleRecoveryPrepareRequest,
};

#[test]
fn recovery_secret_inputs_are_write_only_in_debug_output() {
    let otp = HandleRecoveryOtpRequest {
        identity: Some(awiki_im_core::identity::IdentitySelector::Default),
        full_handle: "alice.awiki.info".to_owned(),
        phone: "+8613800000000".to_owned(),
    };
    let prepare = HandleRecoveryPrepareRequest {
        operation_id: "recover-001".to_owned(),
        phone: "+8613800000000".to_owned(),
        code: "123456".to_owned(),
    };
    assert!(!format!("{otp:?}").contains("13800000000"));
    assert!(!format!("{prepare:?}").contains("123456"));
    assert!(!format!("{prepare:?}").contains("13800000000"));
}

#[test]
fn recovery_facade_uses_the_frozen_phase_and_error_vocabulary() {
    assert_eq!(
        serde_json::to_string(&HandleRecoveryPhase::IdentityTransitionPending).unwrap(),
        "\"identity_transition_pending\""
    );
    assert_eq!(
        HandleRecoveryErrorCode::OutcomeUnknown.as_str(),
        "outcome_unknown"
    );
    assert_eq!(
        HandleRecoveryErrorCode::LocalMigrationUnsupported.as_str(),
        "local_migration_unsupported"
    );
}

#[test]
fn recovery_v4_error_retryability_is_a_closed_table() {
    let cases = [
        (HandleRecoveryErrorCode::FactorRetryRequired, true),
        (HandleRecoveryErrorCode::ResultAbsent, true),
        (HandleRecoveryErrorCode::OutcomeUnknown, true),
        (HandleRecoveryErrorCode::LocalKeyUnavailable, false),
        (HandleRecoveryErrorCode::LocalTransitionPending, true),
        (HandleRecoveryErrorCode::LocalMigrationUnsupported, false),
        (HandleRecoveryErrorCode::UnknownEpoch, false),
    ];
    for (code, retryable) in cases {
        assert_eq!(code.retryable(), retryable, "{}", code.as_str());
    }
}

#[test]
fn schema_35_without_recovery_tables_upgrades_through_the_compat_boundary() {
    let db = rusqlite::Connection::open_in_memory().unwrap();
    awiki_im_core::compat::local_state::ensure_schema(&db).unwrap();
    db.execute_batch(
        r#"
DROP TABLE handle_recovery_operations_v4;
DROP TABLE identity_transition_pending;
PRAGMA user_version=35;
"#,
    )
    .unwrap();

    awiki_im_core::compat::local_state::ensure_schema(&db).unwrap();

    assert_eq!(
        awiki_im_core::compat::local_state::current_schema_version(&db).unwrap(),
        awiki_im_core::compat::local_state::SCHEMA_VERSION,
    );
    for table in [
        "identity_transition_pending",
        "handle_recovery_operations_v4",
    ] {
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            1,
        );
    }
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('identity_transition_pending') WHERE name='metadata_json'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        1,
    );
}

#[test]
fn activation_requires_an_explicit_user_presence_field() {
    let request = HandleRecoveryActivateRequest {
        operation_id: "recovery-public-ref".to_owned(),
        user_presence_confirmed: false,
    };
    assert!(!request.user_presence_confirmed);
}

#[tokio::test]
async fn recovery_execution_gate_defaults_off() {
    let temporary = tempfile::tempdir().unwrap();
    let core = awiki_im_core::ImCore::open(
        awiki_im_core::ImCoreConfig::new(
            awiki_im_core::ServiceEndpoint::parse("https://example.invalid").unwrap(),
            "example.invalid",
        )
        .unwrap(),
        awiki_im_core::ImCorePaths {
            identities: awiki_im_core::IdentityRegistryPaths {
                identity_root_dir: temporary.path().join("identities"),
                registry_path: temporary.path().join("identities/registry.json"),
                default_identity_path: Some(temporary.path().join("identities/default")),
            },
            local_state: awiki_im_core::LocalStatePaths {
                sqlite_path: temporary.path().join("local/im.sqlite"),
            },
            runtime: awiki_im_core::RuntimePaths {
                cache_dir: temporary.path().join("cache"),
                temp_dir: temporary.path().join("tmp"),
            },
        },
    )
    .await
    .unwrap();
    let error = core
        .handle_recovery()
        .request_handle_recovery_otp(HandleRecoveryOtpRequest {
            identity: None,
            full_handle: "alice.example.invalid".to_owned(),
            phone: "+8613800000000".to_owned(),
        })
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        awiki_im_core::ImError::UnsupportedCapability { capability }
            if capability == "handle-recovery-v4"
    ));
}
