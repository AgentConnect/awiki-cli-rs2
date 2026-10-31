use super::*;
use serde::Deserialize;
use std::{path::PathBuf, process::Command};

#[derive(Deserialize)]
struct Marker {
    schema_version: u32,
    cutpoint: String,
    operation_id: String,
}
fn scope(input: &Input, cut: &str) -> PathBuf {
    input
        .storage_b
        .parent()
        .expect("private scope parent")
        .join(format!("acceptance-restart-{cut}"))
}
fn marker(input: &Input, cut: &str) -> Marker {
    let value: Marker = serde_json::from_slice(
        &std::fs::read(scope(input, cut).join("cut.json")).expect("durable public cut marker"),
    )
    .expect("public marker schema");
    assert_eq!(value.schema_version, 1);
    assert!(value.cutpoint == cut && uuid::Uuid::parse_str(&value.operation_id).is_ok());
    value
}
fn cut_child(input: &Input, cut: &str, test: &str) {
    let path = scope(input, cut);
    assert!(
        !path.exists(),
        "use a new restart scenario directory; do not erase prior uncertain assets"
    );
    std::fs::create_dir_all(&path).expect("private restart scope");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
            .expect("private restart scope permissions");
    }
    let result = child_evidence::run(
        input,
        "restart-cut",
        Command::new(std::env::current_exe().expect("native acceptance executable"))
            .args(["--exact", test, "--ignored", "--nocapture"])
            .env("AWIKI_PIN_RECOVERY_TEST_ENABLE", "1")
            .env("AWIKI_PIN_RECOVERY_TEST_CUT", cut)
            .env("AWIKI_PIN_RECOVERY_TEST_MARKER", path.join("cut.json")),
        86,
    );
    assert_eq!(
        result.status.code(),
        Some(86),
        "child must exit at the exact actual boundary"
    );
}
pub(super) async fn register_parent() {
    let input = read_input();
    let cut = "registration_payloads_sealed";
    let owner = core(&input, &input.storage_a, "a");
    let original = take(
        owner.identities().resolve(sdk::IdentitySelector::Default),
        "original not-protected root",
    );
    let initial = take(
        owner
            .pin_recovery()
            .backup_status(sdk::IdentitySelector::Default)
            .await,
        "fresh root backup status",
    );
    assert_eq!(initial.state, PinRecoveryBackupState::NotEnabled);
    assert!(initial.error.is_none());
    let before = owner_registry(&owner, "Registry before registration restart").await;
    drop(owner);
    cut_child(&input, cut, "acceptance_child_register_until_sealed_cut");
    let marker = marker(&input, cut);
    let owner = core(&input, &input.storage_a, "a");
    let pending = pending(&owner, &input, PinRecoveryKind::Enable)
        .await
        .expect("same sealed registration pending survives process exit");
    assert!(pending.operation_id.0 == marker.operation_id);
    let facade = owner.pin_recovery();
    // No PIN is supplied to resume. Registration can only use the three exact
    // payloads sealed by the previous process, not regenerate SDK shares.
    let resumed = take(
        facade
            .resume(ResumePinRecoveryRequest {
                operation_id: pending.operation_id.clone(),
                session: None,
            })
            .await,
        "resume exact sealed registration without PIN",
    );
    assert_eq!(resumed.phase, PinRecoveryPhase::AwaitingPin);
    assert_eq!(
        resumed
            .node_progress
            .iter()
            .filter(|n| n.phase == "registered")
            .count(),
        3
    );
    let completed = take(
        facade
            .submit_pin(
                pending.operation_id,
                take(
                    SecretPin::new(input.pin.as_bytes().to_vec()),
                    "explicit self-check PIN",
                ),
            )
            .await,
        "self-check and activate same resumed backup",
    );
    assert_eq!(completed.phase, PinRecoveryPhase::Completed);
    assert!(completed.operation_id.0 == marker.operation_id);
    let active = take(
        facade.backup_status(sdk::IdentitySelector::Default).await,
        "current backup after restart",
    );
    assert_eq!(active.state, PinRecoveryBackupState::Active);
    assert!(active.error.is_none() && active.active_epoch.is_some());
    assert!(
        take(
            owner.identities().resolve(sdk::IdentitySelector::Default),
            "same original root"
        )
        .did == original.did
    );
    assert_eq!(
        before,
        owner_registry(&owner, "Registry after registration restart").await
    );
    println!("PIN-DRIVER registration_process_exit=true sealed_payloads_resumed_without_pin=true same_operation=true three_registered=true self_check_active=true registry_unchanged=true operation={}", marker.operation_id);
}
pub(super) async fn register_child() {
    assert_eq!(
        std::env::var("AWIKI_PIN_RECOVERY_TEST_CUT").as_deref(),
        Ok("registration_payloads_sealed")
    );
    let input = read_input();
    let owner = core(&input, &input.storage_a, "a");
    let session = phone_session(&owner, &input, None).await;
    require_sacrificial(&input, &session);
    assert!(session.target.can_enable && session.target.active_epoch.is_none());
    let facade = owner.pin_recovery();
    let begun = take(
        facade
            .begin_enable(EnablePinRecoveryRequest {
                session: session.session_handle,
                identity: sdk::IdentitySelector::Default,
                user_presence: UserPresence::confirmed(),
            })
            .await,
        "begin restart registration",
    );
    println!(
        "PIN-DRIVER pending_operation=true operation={}",
        begun.operation_id.0
    );
    let _ = take(
        facade
            .submit_pin(
                begun.operation_id,
                take(
                    SecretPin::new(input.pin.as_bytes().to_vec()),
                    "registration PIN",
                ),
            )
            .await,
        "registration until all exact payloads are sealed",
    );
    panic!("registration sealed cut did not terminate child");
}
pub(super) async fn blinding_parent() {
    let input = read_input();
    let cut = "recover2_response_received";
    let owner = core(&input, &input.storage_a, "a");
    let before = owner_registry(&owner, "Registry before lost blinding factor").await;
    cut_child(
        &input,
        cut,
        "acceptance_child_recover_until_first_charged_cut",
    );
    let marker = marker(&input, cut);
    let candidate = core(
        &input,
        &scope(&input, cut).join("store"),
        "acceptance-blinding",
    );
    let facade = candidate.pin_recovery();
    let id = PinRecoveryOperationId(marker.operation_id.clone());
    let charged = take(
        facade.status(id.clone()).await,
        "authoritative charged attempt after process exit",
    );
    assert_eq!(charged.attempts_used, 1);
    assert!(charged.recovered_identity.is_none() && charged.new_device_id.is_none());
    let resumed = take(
        facade
            .resume(ResumePinRecoveryRequest {
                operation_id: id.clone(),
                session: None,
            })
            .await,
        "reconcile lost-blinding attempt without guessing",
    );
    assert_eq!(resumed.phase, PinRecoveryPhase::AwaitingPin);
    assert_eq!(
        resumed.attempts_used, 1,
        "resume does not refund or create a guess"
    );
    let recovered = take(
        facade
            .submit_pin(
                id.clone(),
                take(
                    SecretPin::new(input.pin.as_bytes().to_vec()),
                    "new explicit recovery PIN",
                ),
            )
            .await,
        "new explicit recovery attempt after blinding loss",
    );
    assert_eq!(
        recovered.phase,
        PinRecoveryPhase::AwaitingDeviceConfirmation
    );
    assert_eq!(recovered.attempts_used, 2);
    assert!(take(
        candidate.identities().list(),
        "restricted fresh recovery after blinding loss"
    )
    .is_empty());
    assert_eq!(
        before,
        owner_registry(&owner, "Registry unchanged after resumed recovery").await
    );
    let cancelled = take(
        facade.cancel(id).await,
        "cancel exact restricted recovery after blinding loss",
    );
    assert_eq!(cancelled.phase, PinRecoveryPhase::Cancelled);
    println!("PIN-DRIVER charged_recover2_process_exit=true lost_blinding_budget_not_refunded=true resume_did_not_guess=true explicit_new_attempt_delta=1 restricted_pending=true registry_unchanged=true cancelled=true operation={}", marker.operation_id);
}
pub(super) async fn blinding_child() {
    assert_eq!(
        std::env::var("AWIKI_PIN_RECOVERY_TEST_CUT").as_deref(),
        Ok("recover2_response_received")
    );
    let input = read_input();
    let candidate = core(
        &input,
        &scope(&input, "recover2_response_received").join("store"),
        "acceptance-blinding",
    );
    assert!(take(
        candidate.identities().list(),
        "fresh lost-blinding candidate"
    )
    .is_empty());
    assert!(pending(&candidate, &input, PinRecoveryKind::Recover)
        .await
        .is_none());
    let session = phone_session(&candidate, &input, None).await;
    require_sacrificial(&input, &session);
    assert!(session.target.can_recover);
    let facade = candidate.pin_recovery();
    let begun = take(
        facade
            .begin_recover(BeginPinRecoveryRequest {
                session: session.session_handle,
                device_label: "lost blinding acceptance".into(),
            })
            .await,
        "begin lost-blinding recovery",
    );
    assert_eq!(begun.attempts_used, 0);
    println!(
        "PIN-DRIVER pending_operation=true operation={}",
        begun.operation_id.0
    );
    let _ = take(
        facade
            .submit_pin(
                begun.operation_id,
                take(
                    SecretPin::new(input.pin.as_bytes().to_vec()),
                    "PIN before actual charged cut",
                ),
            )
            .await,
        "Recover2 until actual charged response",
    );
    panic!("charged Recover2 cut did not terminate child");
}

pub(super) async fn self_check_failure() {
    use zeroize::Zeroizing;
    let input = read_input();
    let cut = "registration_payloads_sealed";
    let owner = core(&input, &input.storage_a, "a");
    let identity = take(
        owner.identities().resolve(sdk::IdentitySelector::Default),
        "self-check original identity",
    );
    let status = take(
        owner
            .pin_recovery()
            .backup_status(sdk::IdentitySelector::Default)
            .await,
        "self-check starts without active backup",
    );
    assert_eq!(status.state, PinRecoveryBackupState::NotEnabled);
    assert!(status.active_epoch.is_none() && status.error.is_none());
    drop(owner);
    cut_child(&input, cut, "acceptance_child_register_until_sealed_cut");
    let marker = marker(&input, cut);
    let owner = core(&input, &input.storage_a, "a");
    let facade = owner.pin_recovery();
    let progress = take(
        facade
            .resume(ResumePinRecoveryRequest {
                operation_id: PinRecoveryOperationId(marker.operation_id.clone()),
                session: None,
            })
            .await,
        "resume all three registrations without self-check PIN",
    );
    assert_eq!(progress.phase, PinRecoveryPhase::AwaitingPin);
    assert_eq!(
        progress
            .node_progress
            .iter()
            .filter(|node| node.phase == "registered")
            .count(),
        3
    );
    let mut wrong = Zeroizing::new(input.pin.as_bytes().to_vec());
    wrong[3] = b'0' + (wrong[3] - b'0' + 1) % 10;
    expect_error(
        facade
            .submit_pin(
                progress.operation_id.clone(),
                take(
                    SecretPin::new(wrong.to_vec()),
                    "explicit wrong self-check PIN",
                ),
            )
            .await,
        &["invalid_pin"],
        "3/3 registration with failing self-check must never activate",
    );
    let status = take(
        facade.status(progress.operation_id.clone()).await,
        "failed self-check authoritative state",
    );
    assert!(status.target.active_epoch.is_none());
    assert_ne!(status.phase, PinRecoveryPhase::Completed);
    let current = take(
        facade.backup_status(sdk::IdentitySelector::Default).await,
        "failed self-check current device state",
    );
    assert!(current.active_epoch.is_none());
    assert_ne!(current.state, PinRecoveryBackupState::Active);
    assert_eq!(
        take(
            facade.cancel(progress.operation_id.clone()).await,
            "cancel exact failed-self-check staging"
        )
        .phase,
        PinRecoveryPhase::Cancelled
    );
    assert_eq!(
        take(
            owner.identities().resolve(sdk::IdentitySelector::Default),
            "original identity remains usable"
        )
        .did,
        identity.did
    );
    println!("PIN-DRIVER three_realms_registered=true wrong_self_check_rejected=true active_epoch_absent=true root_still_usable=true cancelled=true operation={}",progress.operation_id.0);
}
