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
