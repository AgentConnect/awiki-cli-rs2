//! Explicit live acceptance scenarios using only the public Core facade.
//! Secrets and Vault initialization are shared with the owning S2 driver.
use awiki_im_core as sdk;
use sdk::pin_recovery::*;
#[path = "support/pin_recovery_live.rs"]
mod live_support;
use live_support::*;
fn take<T>(result: sdk::ImResult<T>, step: &str) -> T {
    match result {
        Ok(value) => value,
        Err(error) => {
            let code = match &error {
                sdk::ImError::IdentityVault { failure } => failure.code(),
                sdk::ImError::IdentityRequired => "identity_required",
                sdk::ImError::IdentityNotFound { .. } => "identity_not_found",
                sdk::ImError::DefaultIdentityMissing => "default_identity_missing",
                sdk::ImError::AuthRequired => "auth_required",
                sdk::ImError::SessionExpired => "session_expired",
                sdk::ImError::LocalIdentityRecoveryRequired => "local_identity_recovery_required",
                sdk::ImError::Service {
                    code: Some(code), ..
                } if code.len() < 100
                    && code.bytes().all(|c| {
                        c.is_ascii_lowercase()
                            || c.is_ascii_digit()
                            || matches!(c, b'_' | b'.' | b'-')
                    }) =>
                {
                    code
                }
                sdk::ImError::Internal { .. } => "internal",
                _ => "other_core_error",
            };
            let variant = match &error {
                sdk::ImError::InvalidInput { .. } => "InvalidInput",
                sdk::ImError::IdentityRequired => "IdentityRequired",
                sdk::ImError::IdentityNotFound { .. } => "IdentityNotFound",
                sdk::ImError::DefaultIdentityMissing => "DefaultIdentityMissing",
                sdk::ImError::IdentityNotReady { .. } => "IdentityNotReady",
                sdk::ImError::IdentityVault { .. } => "IdentityVault",
                sdk::ImError::AuthRequired => "AuthRequired",
                sdk::ImError::SessionExpired => "SessionExpired",
                sdk::ImError::PermissionDenied => "PermissionDenied",
                sdk::ImError::LocalIdentityRecoveryRequired => "LocalIdentityRecoveryRequired",
                sdk::ImError::PeerNotFound { .. } => "PeerNotFound",
                sdk::ImError::GroupNotFound { .. } => "GroupNotFound",
                sdk::ImError::CursorInvalid => "CursorInvalid",
                sdk::ImError::CursorStale => "CursorStale",
                sdk::ImError::InventoryIncomplete => "InventoryIncomplete",
                sdk::ImError::InventoryTooLarge => "InventoryTooLarge",
                sdk::ImError::DeviceRevokeOutcome { .. } => "DeviceRevokeOutcome",
                sdk::ImError::MessageNotFound { .. } => "MessageNotFound",
                sdk::ImError::TransportUnavailable { .. } => "TransportUnavailable",
                sdk::ImError::AttachmentPreparation { .. } => "AttachmentPreparation",
                sdk::ImError::AttachmentTransfer { .. } => "AttachmentTransfer",
                sdk::ImError::UnsupportedCapability { .. } => "UnsupportedCapability",
                sdk::ImError::LocalStateUnavailable { .. } => "LocalStateUnavailable",
                sdk::ImError::LocalStateUpgradeRequired { .. } => "LocalStateUpgradeRequired",
                sdk::ImError::LocalStateUpgradeInProgress => "LocalStateUpgradeInProgress",
                sdk::ImError::LocalStateUpgradeFailed { .. } => "LocalStateUpgradeFailed",
                sdk::ImError::IdentityUnresolved { .. } => "IdentityUnresolved",
                sdk::ImError::IdentityBindingConflict { .. } => "IdentityBindingConflict",
                sdk::ImError::ConversationAliasConflict { .. } => "ConversationAliasConflict",
                sdk::ImError::MessageWireIdentityConflict { .. } => "MessageWireIdentityConflict",
                sdk::ImError::CanonicalGroupIdentityMissing { .. } => {
                    "CanonicalGroupIdentityMissing"
                }
                sdk::ImError::LocalProjectionUnavailable { .. } => "LocalProjectionUnavailable",
                sdk::ImError::PathUnavailable { .. } => "PathUnavailable",
                sdk::ImError::CredentialFileUnreadable { .. } => "CredentialFileUnreadable",
                sdk::ImError::SkillOnboarding { .. } => "SkillOnboarding",
                sdk::ImError::Service { .. } => "Service",
                sdk::ImError::Serialization { .. } => "Serialization",
                sdk::ImError::Io { .. } => "Io",
                sdk::ImError::Internal { .. } => "Internal",
            };
            let path_kind = match &error {
                sdk::ImError::CredentialFileUnreadable { path_kind, .. }
                | sdk::ImError::PathUnavailable { path_kind, .. }
                    if path_kind.len() < 100
                        && path_kind.bytes().all(|c| {
                            c.is_ascii_lowercase()
                                || c.is_ascii_digit()
                                || matches!(c, b'_' | b'.' | b'-')
                        }) =>
                {
                    path_kind.as_str()
                }
                _ => "none",
            };
            panic!("Acceptance public API failed: {step}; code={code}; variant={variant}; path_kind={path_kind}")
        }
    }
}
#[path = "support/pin_recovery_acceptance/bootstrap.rs"]
mod bootstrap;
#[path = "support/pin_recovery_acceptance/budget.rs"]
mod budget;
#[path = "support/pin_recovery_acceptance/child_evidence.rs"]
mod child_evidence;
#[path = "support/pin_recovery_acceptance/outages.rs"]
mod outages;
#[cfg(feature = "internal-test-helpers")]
#[path = "support/pin_recovery_acceptance/process.rs"]
mod process;
#[cfg(feature = "internal-test-helpers")]
#[path = "support/pin_recovery_acceptance/safety.rs"]
mod safety;
#[cfg(feature = "internal-test-helpers")]
#[path = "support/pin_recovery_acceptance/v4_three_phase.rs"]
mod v4_three_phase;

fn require_sacrificial(input: &Input, session: &PhoneSessionSummary) {
    let expected = std::env::var("AWIKI_PIN_RECOVERY_SACRIFICIAL_TARGET_ID")
        .expect("an independently allocated sacrificial target ID is required");
    assert!(
        session.target.target_id.0 == expected,
        "exact sacrificial target binding"
    );
    assert!(
        session.target.full_handle == input.full_handle,
        "exact fixture Handle"
    );
}

async fn ensure_client(
    core: &sdk::ImCore,
    selector: sdk::IdentitySelector,
    step: &str,
) -> sdk::ImClient {
    let client = take(
        core.client_async(selector).await,
        "normal public client construction",
    );
    let status = take(
        client.auth().status_async().await,
        "normal public auth status",
    );
    println!(
        "PIN-DRIVER public_auth_status has_session={} needs_refresh={}",
        status.has_session, status.needs_refresh
    );
    let expiry = status
        .expires_at
        .as_deref()
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok());
    println!(
        "PIN-DRIVER public_auth_expiry known={} expired={}",
        expiry.is_some(),
        expiry
            .as_ref()
            .is_some_and(|value| value.timestamp() <= chrono::Utc::now().timestamp())
    );
    let session = take(
        client
            .auth()
            .ensure_session_async(sdk::auth::AuthScope::UserProfile)
            .await,
        step,
    );
    let refreshed = session.refreshed;
    drop(session);
    println!("PIN-DRIVER public_auth_ensure refreshed={refreshed}");
    client
}

async fn owner_registry(
    owner: &sdk::ImCore,
    step: &str,
) -> sdk::identity::DeviceJoinRegistrySnapshot {
    let _client = ensure_client(
        owner,
        sdk::IdentitySelector::Default,
        "normal owner session ensure",
    )
    .await;
    take(
        owner
            .device_join()
            .registry(sdk::IdentitySelector::Default)
            .await,
        step,
    )
}
// Keep the orchestration explicit: the helper below is asynchronous because the
// public pending inventory is asynchronous, with no direct Store/Vault access.
async fn fresh_recovery(input: &Input, name: &str) -> sdk::ImCore {
    let path = input
        .storage_b
        .parent()
        .expect("private fixture parent")
        .join(name);
    let device = core(input, &path, name);
    assert!(take(
        device.identities().list(),
        "fresh acceptance identity inventory"
    )
    .is_empty());
    assert!(
        pending(&device, input, PinRecoveryKind::Recover)
            .await
            .is_none(),
        "reconcile an earlier uncertain acceptance operation before retrying"
    );
    device
}
fn expect_error<T>(result: sdk::ImResult<T>, expected: &[&str], step: &str) {
    match result {
        Err(sdk::ImError::Service {
            code: Some(code), ..
        }) if expected.contains(&code.as_str()) => {}
        Err(error) => {
            let (code, status) = match &error {
                sdk::ImError::Service {
                    code: Some(code),
                    status_code,
                    ..
                } if code.len() < 80
                    && code
                        .bytes()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_') =>
                {
                    (code.as_str(), status_code.unwrap_or(0))
                }
                _ => ("core_error", 0),
            };
            panic!("acceptance error category mismatch: {step}; code={code}; status={status}")
        }
        Ok(_) => panic!("acceptance unexpectedly succeeded: {step}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "coordinator-allocated fresh sacrificial root; no PIN backup is created"]
async fn live_prepare_sacrificial_root_without_backup() {
    bootstrap::prepare(false).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "coordinator-allocated fresh root and its first active backup; preserve all private assets"]
async fn live_prepare_sacrificial_root_with_backup() {
    bootstrap::prepare(true).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "only a budget acceptance parent selects this actual child"]
async fn acceptance_child_budget_wrong_guesses() {
    budget::wrong_child().await;
}
#[tokio::test(flavor = "multi_thread")]
#[ignore = "fresh OS child reopens the exact original Vault/key"]
async fn acceptance_child_budget_reauthenticate() {
    budget::reauthenticate_child().await;
}

#[cfg(feature = "internal-test-helpers")]
#[tokio::test(flavor = "multi_thread")]
#[ignore = "3/3 resumed registration followed by a deliberately wrong explicit self-check PIN"]
async fn live_failed_self_check_after_three_registrations_never_activates() {
    restart::self_check_failure().await;
}
#[cfg(feature = "internal-test-helpers")]
#[tokio::test(flavor = "multi_thread")]
#[ignore = "controlled real document/Registry revoke while the same recovery proposal is pending"]
async fn live_concurrent_document_update_preserves_revocation_and_same_recovery_proposal() {
    safety::concurrent_document().await;
}
#[cfg(feature = "internal-test-helpers")]
#[tokio::test(flavor = "multi_thread")]
#[ignore = "actual committed device is revoked through public Core before fresh-process local activation"]
async fn live_revoked_committed_device_never_activates_locally() {
    safety::revoked_committed_parent().await;
}
#[cfg(feature = "internal-test-helpers")]
#[tokio::test(flavor = "multi_thread")]
#[ignore = "only selected by the revoked-device acceptance parent"]
async fn acceptance_child_revoked_device_resume() {
    safety::revoked_resume_child().await;
}

#[cfg(feature = "internal-test-helpers")]
#[tokio::test(flavor = "multi_thread")]
#[ignore = "public P/V/Q flow with coordinator-held physical cleanup and read-only authority gates"]
async fn live_v4_three_phase_revokes_old_authorization_before_delete() {
    v4_three_phase::parent().await;
}
#[cfg(feature = "internal-test-helpers")]
#[tokio::test(flavor = "multi_thread")]
#[ignore = "only selected by the public three-phase V4 parent"]
async fn acceptance_child_v4_old_pending() {
    v4_three_phase::pending_child().await;
}
#[cfg(feature = "internal-test-helpers")]
#[tokio::test(flavor = "multi_thread")]
#[ignore = "only selected by the public three-phase V4 parent"]
async fn acceptance_child_v4_new_identity() {
    v4_three_phase::v4_child().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "deployment owner must stop exactly two named Realms and allocate the protected scope"]
async fn live_two_realms_unavailable_is_service_failure_not_invalid_pin() {
    outages::two_nodes().await;
}
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires an independent unprotected root and a task-only third Register2 transport block"]
async fn live_partial_registration_never_activates_backup() {
    outages::partial_registration().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "exact failed-case continuation; audit two actual registrations then public Core cancel original staging"]
async fn live_reconcile_partial_registration_audit_and_cancel() {
    outages::reconcile_partial().await;
}
#[tokio::test(flavor = "multi_thread")]
#[ignore = "consumes nine wrong guesses and the final correct guess in an independent sacrificial slot"]
async fn live_last_allowed_pin_succeeds_after_reauthentication() {
    budget::last_correct().await;
}
#[tokio::test(flavor = "multi_thread")]
#[ignore = "exhausts a separately allocated sacrificial slot and preserves its exact recovery assets"]
async fn live_exhausted_budget_survives_new_session_and_process() {
    budget::exhausted().await;
}
#[cfg(feature = "internal-test-helpers")]
#[tokio::test(flavor = "multi_thread")]
#[ignore = "real successful device commit is interrupted by the task-only process cutpoint"]
async fn live_device_commit_response_loss_reconciles_same_device_across_processes() {
    process::parent("device_commit_response_received", false).await;
}
#[cfg(feature = "internal-test-helpers")]
#[tokio::test(flavor = "multi_thread")]
#[ignore = "real remote commit succeeds before a task-only local activation process cutpoint"]
async fn live_local_activation_pending_restarts_without_second_device() {
    process::parent("local_activation_pending_saved", false).await;
}
#[cfg(feature = "internal-test-helpers")]
#[tokio::test(flavor = "multi_thread")]
#[ignore = "internal child selected only by a live acceptance parent"]
async fn acceptance_child_commit_with_cut() {
    process::commit_child().await;
}
#[cfg(feature = "internal-test-helpers")]
#[tokio::test(flavor = "multi_thread")]
#[ignore = "internal child selected only by a live acceptance parent"]
async fn acceptance_child_resume_same_operation() {
    process::resume_child().await;
}

#[cfg(feature = "internal-test-helpers")]
#[tokio::test(flavor = "multi_thread")]
#[ignore = "waits for the real 600-second PST expiry before same-key phone reauthentication"]
async fn live_committed_device_resumes_after_real_pst_expiry() {
    process::parent("device_commit_response_received", true).await;
}

#[cfg(feature = "internal-test-helpers")]
#[path = "support/pin_recovery_acceptance/restart.rs"]
mod restart;
#[cfg(feature = "internal-test-helpers")]
#[tokio::test(flavor = "multi_thread")]
#[ignore = "independent unprotected root; real registration process exits after sealing all three exact payloads"]
async fn live_sealed_registration_resumes_without_regenerating_shares() {
    restart::register_parent().await;
}
#[cfg(feature = "internal-test-helpers")]
#[tokio::test(flavor = "multi_thread")]
#[ignore = "independent zero-budget-used slot; real Recover2 is charged before the process loses its blinding factor"]
async fn live_lost_blinding_factor_preserves_charge_and_requires_new_pin_attempt() {
    restart::blinding_parent().await;
}
#[cfg(feature = "internal-test-helpers")]
#[tokio::test(flavor = "multi_thread")]
#[ignore = "internal child selected only by the actual registration restart acceptance parent"]
async fn acceptance_child_register_until_sealed_cut() {
    restart::register_child().await;
}
#[cfg(feature = "internal-test-helpers")]
#[tokio::test(flavor = "multi_thread")]
#[ignore = "internal child selected only by the actual lost-blinding acceptance parent"]
async fn acceptance_child_recover_until_first_charged_cut() {
    restart::blinding_child().await;
}

#[cfg(feature = "internal-test-helpers")]
#[tokio::test(flavor = "multi_thread")]
#[ignore = "real PST expiry plus guarded exact committed-operation deadline advancement; no shared TTL change"]
async fn live_committed_device_resumes_after_pst_and_operation_deadlines() {
    assert_eq!(
        std::env::var("AWIKI_PIN_RECOVERY_ACCEPTANCE_EXPIRE_OPERATION").as_deref(),
        Ok("1")
    );
    process::parent("device_commit_response_received", true).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "exact local public-state diagnosis and cancellation of the known two-node test operation only"]
async fn acceptance_diagnose_and_cancel_two_node_operation() {
    let expected = std::env::var("AWIKI_PIN_RECOVERY_DIAGNOSTIC_OPERATION_ID")
        .expect("exact known test operation ID required");
    assert!(uuid::Uuid::parse_str(&expected).is_ok());
    let input = read_input();
    let path = input
        .storage_b
        .parent()
        .expect("private scope parent")
        .join("acceptance-two-nodes");
    let candidate = core(&input, &path, "acceptance-two-nodes");
    assert!(take(
        candidate.identities().list(),
        "diagnostic candidate inventory"
    )
    .is_empty());
    let progress = take(
        candidate.pin_recovery().list_pending().await,
        "local public pending diagnostic",
    )
    .into_iter()
    .find(|p| p.operation_id.0 == expected)
    .expect("exact known pending operation must exist");
    let code = progress
        .error
        .as_ref()
        .map(|e| e.code.as_str())
        .unwrap_or("none");
    assert!(
        code.len() <= 80
            && code
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    );
    println!(
        "PIN-DRIVER diagnostic_error_code={code} attempts_used={} operation={expected}",
        progress.attempts_used
    );
    if std::env::var("AWIKI_PIN_RECOVERY_DIAGNOSTIC_CANCEL").as_deref() == Ok("1") {
        let cancelled = take(
            candidate.pin_recovery().cancel(progress.operation_id).await,
            "precise known two-node operation cancel",
        );
        assert_eq!(cancelled.phase, PinRecoveryPhase::Cancelled);
        assert!(take(
            candidate.pin_recovery().list_pending().await,
            "pending inventory after exact cancel"
        )
        .is_empty());
        assert!(take(
            candidate.identities().list(),
            "identity inventory after exact cancel"
        )
        .is_empty());
        println!("PIN-DRIVER diagnostic_cancelled=true local_pending_count=0 local_identity_count=0 operation={expected}");
    }
}
