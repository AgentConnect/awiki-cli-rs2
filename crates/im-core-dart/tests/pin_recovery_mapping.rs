use awiki_im_core::dto::pin_recovery::*;
use im_core::pin_recovery as p;

#[test]
fn pin_recovery_mapping_preserves_large_generation_and_epoch() {
    let mapped: DartPinRecoveryTarget = p::PinRecoveryTarget {
        target_id: p::RecoveryTargetId("target".into()),
        full_handle: "alice.awiki.info".into(),
        did: "did:wba:awiki.info:alice".into(),
        binding_generation: u64::MAX.to_string(),
        active_epoch: Some(u64::MAX),
        can_enable: false,
        can_recover: true,
        unavailable_reason: None,
    }
    .into();
    assert_eq!(mapped.active_epoch.as_deref(), Some("18446744073709551615"));
    assert_eq!(mapped.binding_generation, "18446744073709551615");
    assert_eq!(mapped.target_id, "target");
    assert!(mapped.can_recover);
}

#[test]
fn pin_recovery_mapping_preserves_distinct_pending_and_secure_states() {
    assert_eq!(
        DartPinRecoveryPhase::from(p::PinRecoveryPhase::CommitUnknown),
        DartPinRecoveryPhase::CommitUnknown
    );
    assert_eq!(
        DartPinRecoveryPhase::from(p::PinRecoveryPhase::LocalActivationPending),
        DartPinRecoveryPhase::LocalActivationPending
    );
    assert_eq!(
        DartPinRecoveryPhase::from(p::PinRecoveryPhase::ActiveSecurePending),
        DartPinRecoveryPhase::ActiveSecurePending
    );
    assert_eq!(
        DartPinRecoverySecureReadiness::from(p::PinRecoverySecureReadiness::PrekeysPending),
        DartPinRecoverySecureReadiness::PrekeysPending
    );
    let error: DartPinRecoveryError = p::PinRecoveryError {
        code: "invalid_pin".into(),
        retryable: true,
        retry_after_seconds: Some(30),
    }
    .into();
    assert_eq!(error.code, "invalid_pin");
    assert_eq!(error.retry_after_seconds, Some(30));
    assert!(error.retryable);
}

#[test]
fn pin_recovery_trust_config_mapping_retains_host_installed_keys() {
    let mapped: p::PinRecoveryTenantConfig = DartPinRecoveryTenantConfig {
        tenant_id: "tenant".into(),
        deployment_id: "deployment".into(),
        control_base_url: "https://awiki.info".into(),
        config_signing_keys: [("trusted-kid".into(), "public-key".into())]
            .into_iter()
            .collect(),
    }
    .into();
    assert_eq!(
        mapped.config_signing_keys.get("trusted-kid").unwrap(),
        "public-key"
    );
    assert_eq!(mapped.tenant_id, "tenant");
}

#[test]
fn pin_recovery_failures_keep_service_codes_for_app_classification() {
    for code in [
        "invalid_pin",
        "guess_budget_exhausted",
        "realm_unavailable",
        "network_unavailable",
    ] {
        let mapped = awiki_im_core::dto::error::DartImError::from(im_core::ImError::Service {
            status_code: None,
            code: Some(code.into()),
            message: "PIN recovery could not complete".into(),
            data: Some(serde_json::json!({"retryable": code.ends_with("unavailable")})),
        });
        assert_eq!(mapped.code, "service_error");
        assert_eq!(mapped.service_code.as_deref(), Some(code));
        assert!(!mapped.message.contains("1234"));
    }
}

#[test]
fn backup_status_mapping_keeps_unknown_distinct_from_disabled() {
    let status: DartPinRecoveryBackupStatus = p::PinRecoveryBackupStatus {
        did: "did:wba:awiki.info:alice".into(),
        full_handle: None,
        binding_generation: None,
        state: p::PinRecoveryBackupState::Unknown,
        active_epoch: None,
        error: Some(p::PinRecoveryError {
            code: "network_unavailable".into(),
            retryable: true,
            retry_after_seconds: None,
        }),
    }
    .into();
    assert_eq!(status.state, DartPinRecoveryBackupState::Unknown);
    assert_ne!(status.state, DartPinRecoveryBackupState::NotEnabled);
    assert_eq!(status.error.unwrap().code, "network_unavailable");
    for (core, dart) in [
        (
            p::PinRecoveryBackupState::NotEnabled,
            DartPinRecoveryBackupState::NotEnabled,
        ),
        (
            p::PinRecoveryBackupState::Pending,
            DartPinRecoveryBackupState::Pending,
        ),
        (
            p::PinRecoveryBackupState::Active,
            DartPinRecoveryBackupState::Active,
        ),
    ] {
        assert_eq!(DartPinRecoveryBackupState::from(core), dart);
    }
}

#[test]
fn backup_status_mapping_keeps_large_epoch_as_string() {
    let status: DartPinRecoveryBackupStatus = p::PinRecoveryBackupStatus {
        did: "did:wba:awiki.info:alice".into(),
        full_handle: Some("alice.awiki.info".into()),
        binding_generation: Some(u64::MAX.to_string()),
        state: p::PinRecoveryBackupState::Active,
        active_epoch: Some(u64::MAX),
        error: None,
    }
    .into();
    assert_eq!(status.active_epoch, status.binding_generation);
    assert_eq!(status.active_epoch.as_deref(), Some("18446744073709551615"));
}

#[test]
fn cold_resume_progress_preserves_original_device_confirmation() {
    let progress: DartPinRecoveryProgress = p::PinRecoveryProgress {
        operation_id: p::PinRecoveryOperationId("original-operation".into()),
        kind: p::PinRecoveryKind::Recover,
        phase: p::PinRecoveryPhase::AwaitingDeviceConfirmation,
        allowed_actions: vec![
            p::PinRecoveryAction::ConfirmDevice,
            p::PinRecoveryAction::Cancel,
        ],
        target: p::PinRecoveryTarget {
            target_id: p::RecoveryTargetId("target".into()),
            full_handle: "alice.awiki.info".into(),
            did: "did:wba:awiki.info:alice".into(),
            binding_generation: "5".into(),
            active_epoch: Some(7),
            can_enable: false,
            can_recover: true,
            unavailable_reason: None,
        },
        node_progress: vec![],
        attempts_used: 1,
        attempts_limit: 10,
        needs_phone_verification: false,
        needs_user_confirmation: true,
        recovered_identity: None,
        new_device_id: Some("original-device".into()),
        device_label: Some("Second Linux App".into()),
        secure_readiness: p::PinRecoverySecureReadiness::NotStarted,
        cleanup_remaining: 0,
        error: None,
    }
    .into();
    assert_eq!(progress.operation_id, "original-operation");
    assert_eq!(progress.device_label.as_deref(), Some("Second Linux App"));
    assert_eq!(progress.new_device_id.as_deref(), Some("original-device"));
    assert_eq!(
        progress.phase,
        DartPinRecoveryPhase::AwaitingDeviceConfirmation
    );
    assert!(progress.needs_user_confirmation);
    assert_eq!(
        progress.allowed_actions,
        vec![
            DartPinRecoveryAction::ConfirmDevice,
            DartPinRecoveryAction::Cancel
        ]
    );
}
