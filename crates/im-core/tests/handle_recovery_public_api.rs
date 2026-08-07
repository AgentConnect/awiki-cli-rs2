use awiki_im_core::identity::{
    HandleRecoveryActivateRequest, HandleRecoveryErrorCode, HandleRecoveryOtpRequest,
    HandleRecoveryPhase, HandleRecoveryPrepareRequest,
};

#[test]
fn recovery_secret_inputs_are_write_only_in_debug_output() {
    let otp = HandleRecoveryOtpRequest {
        phone: "+8613800000000".to_owned(),
        handle: "alice.example.invalid".to_owned(),
        operation_id: "recover-001".to_owned(),
    };
    let prepare = HandleRecoveryPrepareRequest {
        identity: Some(awiki_im_core::identity::IdentitySelector::Default),
        phone: "+8613800000000".to_owned(),
        code: "123456".to_owned(),
        handle: "alice.example.invalid".to_owned(),
        operation_id: "recover-001".to_owned(),
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
        HandleRecoveryErrorCode::HandleRecoveryOutcomeUnknown.as_str(),
        "handle_recovery_outcome_unknown"
    );
    assert_eq!(
        HandleRecoveryErrorCode::HandleRecoveryTransitionChainUnsupported.as_str(),
        "handle_recovery_transition_chain_unsupported"
    );
}

#[test]
fn activation_requires_an_explicit_user_presence_field() {
    let request = HandleRecoveryActivateRequest {
        recovery_id: "recovery-public-ref".to_owned(),
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
            phone: "+8613800000000".to_owned(),
            handle: "alice.example.invalid".to_owned(),
            operation_id: "recover-001".to_owned(),
        })
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        awiki_im_core::ImError::UnsupportedCapability { capability }
            if capability == "handle-recovery-v1"
    ));
}
