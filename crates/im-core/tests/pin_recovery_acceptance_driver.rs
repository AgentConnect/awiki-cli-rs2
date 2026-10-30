//! Explicit live acceptance scenarios using only the public Core facade.
//! Secrets and Vault initialization are shared with the owning S2 driver.
use awiki_im_core as sdk;
use sdk::pin_recovery::*;
#[path = "support/pin_recovery_live.rs"]
mod live_support;
use live_support::*;
#[path = "support/pin_recovery_acceptance/budget.rs"]
mod budget;
#[path = "support/pin_recovery_acceptance/outages.rs"]
mod outages;
#[cfg(feature = "internal-test-helpers")]
#[path = "support/pin_recovery_acceptance/process.rs"]
mod process;

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
        Err(_) => panic!("acceptance error category mismatch: {step}"),
        Ok(_) => panic!("acceptance unexpectedly succeeded: {step}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "deployment owner must stop exactly two named Realms and allocate the protected scope"]
async fn live_two_realms_unavailable_is_service_failure_not_invalid_pin() {
    outages::two_nodes().await;
}
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a separate not-enabled root and one deployment-owned unavailable Realm"]
async fn live_partial_registration_never_activates_backup() {
    outages::partial_registration().await;
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
