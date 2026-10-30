use super::*;

pub(super) async fn two_nodes() {
    let unavailable = std::env::var("AWIKI_PIN_RECOVERY_UNAVAILABLE_REALM_IDS")
        .expect("two public unavailable Realm IDs are required");
    let ids = unavailable
        .split(',')
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(ids.len(), 2);
    assert!(ids
        .iter()
        .all(|v| v.len() == 32 && v.bytes().all(|b| b.is_ascii_hexdigit())));
    let input = read_input();
    let owner = core(&input, &input.storage_b, "b");
    let before = take(
        owner
            .device_join()
            .registry(sdk::IdentitySelector::Default)
            .await,
        "Registry before two-node outage",
    );
    let candidate = fresh_recovery(&input, "acceptance-two-nodes").await;
    let session = phone_session(&candidate, &input, None).await;
    assert!(session.target.can_recover);
    let facade = candidate.pin_recovery();
    let begun = take(
        facade
            .begin_recover(BeginPinRecoveryRequest {
                session: session.session_handle,
                device_label: "two-node acceptance".into(),
            })
            .await,
        "begin two-node recovery",
    );
    assert!(ids
        .iter()
        .all(|id| begun.node_progress.iter().any(|n| n.realm_id == *id)));
    println!(
        "PIN-DRIVER pending_operation=true operation={}",
        begun.operation_id.0
    );
    expect_error(
        facade
            .submit_pin(
                begun.operation_id.clone(),
                take(SecretPin::new(input.pin.as_bytes().to_vec()), "PIN format"),
            )
            .await,
        &["realm_unavailable"],
        "two nodes unavailable must not be invalid PIN",
    );
    let status = take(
        facade.status(begun.operation_id.clone()).await,
        "two-node authoritative status",
    );
    assert!(status.attempts_used <= begun.attempts_used + 1);
    assert!(status.recovered_identity.is_none() && status.new_device_id.is_none());
    assert!(take(candidate.identities().list(), "two-node local inventory").is_empty());
    assert_eq!(
        before,
        take(
            owner
                .device_join()
                .registry(sdk::IdentitySelector::Default)
                .await,
            "Registry after two-node outage"
        )
    );
    let cancelled = take(
        facade.cancel(begun.operation_id).await,
        "cancel exact two-node operation",
    );
    assert_eq!(cancelled.phase, PinRecoveryPhase::Cancelled);
    println!("PIN-DRIVER two_realms_unavailable=true service_failure_not_pin=true attempt_delta_le_one=true no_identity=true registry_unchanged=true cancelled=true operation={}", cancelled.operation_id.0);
}

pub(super) async fn partial_registration() {
    assert!(
        cfg!(feature = "internal-test-helpers"),
        "partial registration uses only the explicit test-helper build"
    );
    assert_eq!(
        std::env::var("AWIKI_PIN_RECOVERY_TEST_ENABLE").as_deref(),
        Ok("1")
    );
    let _unavailable = std::env::var("AWIKI_PIN_RECOVERY_TEST_BLOCK_REGISTER2_REALM_ID")
        .expect("task-only exact blocked Register2 Realm ID required");
    let input = read_input();
    let owner = core(&input, &input.storage_a, "a");
    let identity = take(
        owner.identities().resolve(sdk::IdentitySelector::Default),
        "unprotected root identity",
    );
    let facade = owner.pin_recovery();
    assert!(pending(&owner, &input, PinRecoveryKind::Enable)
        .await
        .is_none());
    let before = take(
        facade.backup_status(sdk::IdentitySelector::Default).await,
        "not-enabled current status",
    );
    assert_eq!(before.state, PinRecoveryBackupState::NotEnabled);
    assert!(before.error.is_none() && before.active_epoch.is_none());
    let session = phone_session(&owner, &input, None).await;
    require_sacrificial(&input, &session);
    assert!(session.target.can_enable && !session.target.can_recover);
    assert!(session.target.did == identity.did.as_str());
    let begun = take(
        facade
            .begin_enable(EnablePinRecoveryRequest {
                session: session.session_handle,
                identity: sdk::IdentitySelector::Default,
                user_presence: UserPresence::confirmed(),
            })
            .await,
        "begin partial backup",
    );
    println!(
        "PIN-DRIVER pending_operation=true operation={}",
        begun.operation_id.0
    );
    expect_error(
        facade
            .submit_pin(
                begun.operation_id.clone(),
                take(SecretPin::new(input.pin.as_bytes().to_vec()), "PIN format"),
            )
            .await,
        &["realm_unavailable"],
        "partial registration must not activate",
    );
    let status = take(
        facade.status(begun.operation_id.clone()).await,
        "partial registration authoritative status",
    );
    assert_ne!(status.phase, PinRecoveryPhase::Completed);
    assert!(status.target.active_epoch.is_none());
    assert!(
        status
            .node_progress
            .iter()
            .filter(|n| n.phase == "registered")
            .count()
            < 3
    );
    let current = take(
        facade.backup_status(sdk::IdentitySelector::Default).await,
        "partial backup current device status",
    );
    assert!(current.error.is_none() && current.active_epoch.is_none());
    assert_ne!(current.state, PinRecoveryBackupState::Active);
    // This independent System-owned read-only audit runs before cancellation,
    // because physical Delete correctly purges prior encrypted result rows.
    let python = std::env::var_os("AWIKI_REALM_SYSTEM_AUDIT_PYTHON")
        .expect("System owner audit Python required");
    let root = std::env::var_os("AWIKI_REALM_SYSTEM_AUDIT_ROOT")
        .expect("System owner audit root required");
    let audit = std::process::Command::new(python)
        .current_dir(root)
        .args([
            "-m",
            "tests_v2.key_recovery.cleanup_audit",
            "--partial-register",
            &begun.operation_id.0,
        ])
        .output()
        .expect("independent exact registered receipt audit");
    assert!(audit.status.success(), "System owner must independently verify exactly two actual registered receipts before cancel");
    let cancelled = take(
        facade.cancel(begun.operation_id).await,
        "cancel exact partial staging",
    );
    assert_eq!(cancelled.phase, PinRecoveryPhase::Cancelled);
    assert!(
        take(
            owner.identities().resolve(sdk::IdentitySelector::Default),
            "root after failed optional backup"
        )
        .did == identity.did
    );
    println!("PIN-DRIVER partial_registration=true two_registered_receipts_verified=true active_epoch_absent=true root_still_usable=true cancelled=true operation={}", cancelled.operation_id.0);
}
