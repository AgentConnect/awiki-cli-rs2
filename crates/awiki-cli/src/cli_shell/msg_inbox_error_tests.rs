use super::*;

#[test]
fn inbox_storage_contention_is_retryable() {
    for detail in [
        "database is locked",
        "database table is locked",
        "database is busy",
    ] {
        let error = inbox_read_exit(MessageAdapterError::LocalStateUnavailable(
            detail.to_owned(),
        ));
        assert_eq!(error.exit_code, 5);
        assert_eq!(error.detail.code, "local_state_unavailable");
        assert!(error.detail.retryable);
        assert!(error.detail.hint.contains("inbox query"));
    }
}

#[test]
fn inbox_permanent_storage_errors_are_not_retryable() {
    for detail in [
        "file is not a database",
        "database disk image is malformed",
        "attempt to write a readonly database",
        "identity vault is locked",
    ] {
        let error = inbox_read_exit(MessageAdapterError::LocalStateUnavailable(
            detail.to_owned(),
        ));
        assert_eq!(error.detail.code, "local_state_unavailable");
        assert!(!error.detail.retryable);
    }
}

#[test]
fn inbox_authorization_errors_remain_terminal() {
    let denied = inbox_read_exit(MessageAdapterError::PermissionDenied);
    assert_eq!(denied.detail.code, "permission_denied");
    assert!(!denied.detail.retryable);
    let identity = inbox_read_exit(MessageAdapterError::IdentityRequired(
        "unavailable".to_owned(),
    ));
    assert_eq!(identity.detail.code, "identity_required");
    assert!(!identity.detail.retryable);
}

#[test]
fn write_contention_does_not_enable_blind_retries() {
    let error = message_exit(
        MessageAdapterError::LocalStateUnavailable("database is locked".to_owned()),
        "Reconcile the original write before retrying.",
    );
    assert_eq!(error.detail.code, "local_state_unavailable");
    assert!(!error.detail.retryable);
}

#[test]
fn foreground_inbox_pending_states_have_structured_bounded_retry_guidance() {
    for (budget_exhausted, error_code, warnings, reason) in [
        (
            true,
            None,
            vec!["sync.budget_exhausted"],
            "budget_exhausted",
        ),
        (false, None, vec![], "receive_pending"),
        (
            false,
            Some("SYNC_RETRYABLE_FAILURE"),
            vec!["sync.retry.transport_unavailable"],
            "retryable_failure",
        ),
    ] {
        let pending = MessageAdapterError::ForegroundSyncPending {
            budget_exhausted,
            error_code: error_code.map(str::to_owned),
            warnings: warnings
                .iter()
                .map(|warning| (*warning).to_owned())
                .collect(),
        };
        let error = inbox_read_exit(pending.clone());
        assert_eq!(error.detail.code, "transport_unavailable");
        assert_eq!(error.exit_code, 1);
        assert!(error.detail.retryable);
        assert_eq!(error.detail.details["phase"], "inbox_reconciliation");
        assert_eq!(error.detail.details["sync_reason"], reason);
        assert_eq!(error.detail.details["sync_error_code"], json!(error_code));
        assert_eq!(error.detail.details["sync_warnings"], json!(warnings));
        assert!(
            !message_exit(pending, "Reconcile the operation.")
                .detail
                .retryable
        );
    }
}

#[test]
fn foreground_storage_and_unknown_failures_do_not_advertise_query_retries() {
    for warning in [
        "sync.retry.local_state.constraint_failed",
        "sync.retry.local_state.schema_unavailable",
        "sync.retry.local_state.storage_unavailable",
        "sync.retry.local_state.codec_unavailable",
        "sync.retry.local_state.other",
        "sync.retry.service_unavailable",
    ] {
        let error = inbox_read_exit(MessageAdapterError::ForegroundSyncPending {
            budget_exhausted: false,
            error_code: Some("SYNC_RETRYABLE_FAILURE".into()),
            warnings: vec![warning.into()],
        });
        assert!(!error.detail.retryable, "{warning}");
        assert_eq!(error.detail.details["sync_warnings"], json!([warning]));
    }
    let error = inbox_read_exit(MessageAdapterError::TransportUnavailable("unknown".into()));
    assert!(!error.detail.retryable);
}

#[test]
fn pending_sync_json_does_not_expose_arbitrary_error_or_warning_details() {
    let error = inbox_read_exit(MessageAdapterError::ForegroundSyncPending {
        budget_exhausted: false,
        error_code: Some("Bearer secret-value".into()),
        warnings: vec![
            "did:wba:private:fixture".into(),
            "sync.retry.transport_unavailable".into(),
        ],
    });
    let rendered = serde_json::to_string(&error.detail).unwrap();
    assert!(!rendered.contains("secret-value"));
    assert!(!rendered.contains("private:fixture"));
    assert!(!error.detail.retryable);
    assert!(error.detail.details["sync_error_code"].is_null());
}
