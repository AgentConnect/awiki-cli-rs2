use super::*;

fn result(state: HandleRegistrationState) -> HandleRegistrationResult {
    HandleRegistrationResult {
        identity: None,
        account_id: Some("account-fixture".to_owned()),
        handle: Handle::parse("bob.awiki.test", "").unwrap(),
        method: RegistrationMethod::AlreadyVerified,
        state,
        join_required: None,
        default_identity_change: None,
        retry_after_seconds: None,
        retry_at: None,
        warnings: Vec::new(),
    }
}

#[test]
fn registration_receive_never_opens_default_or_pending_identity() {
    for state in [
        HandleRegistrationState::OtpSent,
        HandleRegistrationState::EmailSent,
        HandleRegistrationState::EmailPending,
        HandleRegistrationState::JoinRequired,
    ] {
        assert_eq!(registration_receive_selector(&result(state)).unwrap(), None);
    }
    let mut registered = result(HandleRegistrationState::Registered);
    assert_eq!(
        registration_receive_selector(&registered)
            .unwrap_err()
            .detail
            .code,
        "registration_receive_pending"
    );
    let id = im_core::ids::IdentityId::parse("new-local-owner").unwrap();
    registered.identity = Some(im_core::identity::IdentitySummary {
        id: id.clone(),
        did: Did::parse("did:wba:awiki.test:bob:e1_root").unwrap(),
        handle: Some(registered.handle.clone()),
        display_name: None,
        local_alias: Some("bob".to_owned()),
        device_id: None,
        is_default: false,
        readiness: im_core::identity::IdentityReadiness {
            ready_for_auth: true,
            ready_for_messaging: true,
            missing: Vec::new(),
        },
    });
    assert_eq!(
        registration_receive_selector(&registered).unwrap(),
        Some(IdentitySelector::Id(id))
    );
    assert_eq!(
        registration_receive_request().reason,
        "foreground_reconcile"
    );
}

fn sync_outcome(
    status: im_core::messages::MessageSyncStatus,
) -> im_core::messages::MessageSyncOutcome {
    im_core::messages::MessageSyncOutcome {
        status,
        events_applied: 0,
        pages_fetched: 1,
        messages_hydrated: 0,
        duplicates_skipped: 0,
        older_history_excluded: false,
        changed_conversation_ids: vec![],
        committed_incoming_messages: vec![],
        error_code: None,
        warnings: vec![],
    }
}

#[test]
fn registration_receive_preserves_gate_for_all_sync_states_and_budget_exhaustion() {
    use im_core::messages::MessageSyncStatus::*;
    for (status, label) in [
        (RetryableFailure, "retryable_failure"),
        (RecoveryRequired, "recovery_required"),
        (Blocked, "blocked"),
        (AuthRevoked, "auth_revoked"),
    ] {
        let error = registration_receive::require_ready(&sync_outcome(status)).unwrap_err();
        assert_eq!(error.detail.code, "registration_receive_pending");
        assert_eq!(error.detail.details["phase"], "sync_readiness");
        assert_eq!(error.detail.details["sync_status"], label);
        assert!(
            !error.detail.retryable,
            "must not advise retrying registration"
        );
        assert!(error.detail.hint.contains("msg inbox"));
        assert!(error.detail.hint.contains("do not register again"));
    }
    for status in [Idle, Changed] {
        let mut outcome = sync_outcome(status);
        assert!(registration_receive::require_ready(&outcome).is_ok());
        outcome.warnings.push("sync.budget_exhausted".into());
        let error = registration_receive::require_ready(&outcome).unwrap_err();
        assert_eq!(
            error.detail.details["sync_warnings"],
            json!(["sync.budget_exhausted"])
        );
    }
}

#[test]
fn registration_receive_diagnostics_keep_only_known_sync_codes_and_warnings() {
    let mut outcome = sync_outcome(im_core::messages::MessageSyncStatus::RetryableFailure);
    outcome.error_code = Some("SYNC_RETRYABLE_FAILURE".into());
    outcome.warnings = vec![
        "sync.retry.local_state.database_busy".into(),
        "private-token".into(),
    ];
    let error = registration_receive::require_ready(&outcome).unwrap_err();
    assert_eq!(
        error.detail.details["sync_error_code"],
        "SYNC_RETRYABLE_FAILURE"
    );
    assert_eq!(
        error.detail.details["sync_warnings"],
        json!(["sync.retry.local_state.database_busy"])
    );
    assert_eq!(error.detail.details["sync_warning_count"], 2);
    assert_eq!(error.detail.details["sync_warnings_redacted"], 1);
    assert!(!serde_json::to_string(&error.detail)
        .unwrap()
        .contains("private-token"));
    outcome.error_code = Some("private-token".into());
    let error = registration_receive::require_ready(&outcome).unwrap_err();
    assert_eq!(
        error.detail.details["sync_error_code"],
        serde_json::Value::Null
    );
    assert_eq!(error.detail.details["sync_error_code_redacted"], true);
    assert!(!serde_json::to_string(&error.detail)
        .unwrap()
        .contains("private-token"));
}

#[test]
fn registration_receive_core_failure_preserves_stage_without_raw_service_data() {
    let cause = im_core::ImError::Service {
        status_code: Some(503),
        code: Some("private-token".into()),
        message: "private-token".into(),
        data: Some(json!({"token": "private-token"})),
    };
    for phase in ["client_open", "initial_sync"] {
        let error = registration_receive::core_failure(phase, &cause);
        assert_eq!(error.detail.details["phase"], phase);
        assert_eq!(error.detail.details["cause_code"], "service_error");
        assert_eq!(error.detail.details["http_status"], 503);
        assert_eq!(error.detail.details["registration_committed"], true);
        assert!(!serde_json::to_string(&error.detail)
            .unwrap()
            .contains("private-token"));
    }
    let error = registration_receive::core_failure(
        "client_open",
        &im_core::ImError::LocalStateUnavailable {
            detail: "/private/identity-secret".into(),
        },
    );
    assert_eq!(
        error.detail.details["cause_code"],
        "local_state_unavailable"
    );
    assert!(!serde_json::to_string(&error.detail)
        .unwrap()
        .contains("identity-secret"));
    let error =
        registration_receive_selector(&result(HandleRegistrationState::Registered)).unwrap_err();
    assert_eq!(error.detail.details["phase"], "identity_selection");
}
