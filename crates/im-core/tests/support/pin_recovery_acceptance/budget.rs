use super::*;
use zeroize::Zeroizing;

async fn run_wrong_guesses(
    input: &Input,
    name: &str,
    count: u16,
) -> (sdk::ImCore, PinRecoveryOperationId) {
    let candidate = fresh_recovery(input, name).await;
    let session = phone_session(&candidate, input, None).await;
    require_sacrificial(input, &session);
    assert!(session.target.can_recover);
    let facade = candidate.pin_recovery();
    let begun = take(
        facade
            .begin_recover(BeginPinRecoveryRequest {
                session: session.session_handle,
                device_label: "sacrificial budget acceptance".into(),
            })
            .await,
        "begin sacrificial recovery",
    );
    println!(
        "PIN-DRIVER pending_operation=true operation={}",
        begun.operation_id.0
    );
    assert_eq!(
        begun.attempts_used, 0,
        "sacrificial backup starts with unused global budget"
    );
    assert_eq!(begun.attempts_limit, 10);
    let mut wrong = Zeroizing::new(input.pin.as_bytes().to_vec());
    assert_eq!(wrong.len(), 4);
    wrong[3] = b'0' + (wrong[3] - b'0' + 1) % 10;
    for number in 1..=count {
        let expected = if number == 10 {
            "guess_budget_exhausted"
        } else {
            "invalid_pin"
        };
        expect_error(
            facade
                .submit_pin(
                    begun.operation_id.clone(),
                    take(SecretPin::new(wrong.to_vec()), "wrong PIN format"),
                )
                .await,
            &[expected],
            "one explicit wrong guess",
        );
        let status = take(
            facade.status(begun.operation_id.clone()).await,
            "global budget after explicit guess",
        );
        assert_eq!(
            status.attempts_used, number,
            "one submit consumes exactly one shared attempt"
        );
        assert!(status.recovered_identity.is_none() && status.new_device_id.is_none());
        assert!(take(
            candidate.identities().list(),
            "no identity from wrong guesses"
        )
        .is_empty());
    }
    drop(facade);
    (candidate, begun.operation_id)
}

pub(super) async fn last_correct() {
    let input = read_input();
    let (candidate, operation_id) = run_wrong_guesses(&input, "acceptance-last-correct", 9).await;
    drop(candidate);
    // Reopen the same sealed scope, obtain a real new PST with the original key,
    // and prove that neither process restart nor reauthentication refunds guesses.
    let path = input
        .storage_b
        .parent()
        .expect("scope parent")
        .join("acceptance-last-correct");
    let reopened = core(&input, &path, "acceptance-last-correct");
    let session = phone_session(&reopened, &input, Some(operation_id.clone())).await;
    let facade = reopened.pin_recovery();
    let resumed = take(
        facade
            .resume(ResumePinRecoveryRequest {
                operation_id: operation_id.clone(),
                session: Some(session.session_handle),
            })
            .await,
        "reauthenticate same ninth-guess operation",
    );
    assert_eq!(resumed.attempts_used, 9);
    let recovered = take(
        facade
            .submit_pin(
                operation_id.clone(),
                take(
                    SecretPin::new(input.pin.as_bytes().to_vec()),
                    "correct last PIN format",
                ),
            )
            .await,
        "last allowed correct PIN",
    );
    assert_eq!(
        recovered.phase,
        PinRecoveryPhase::AwaitingDeviceConfirmation
    );
    assert!(recovered.recovered_identity.is_none() && recovered.new_device_id.is_some());
    assert_eq!(recovered.attempts_used, 10);
    assert!(take(
        reopened.identities().list(),
        "restricted last-guess pending"
    )
    .is_empty());
    let cancelled = take(
        facade.cancel(operation_id).await,
        "cancel exact last-guess pending",
    );
    assert_eq!(cancelled.phase, PinRecoveryPhase::Cancelled);
    println!("PIN-DRIVER nine_wrong_guesses=true reauthentication_preserves_budget=true last_correct_pin_recovered=true restricted_pending=true cancelled=true operation={}", cancelled.operation_id.0);
}

pub(super) async fn exhausted() {
    let input = read_input();
    let owner = core(&input, &input.storage_b, "b");
    let before = take(
        owner
            .device_join()
            .registry(sdk::IdentitySelector::Default)
            .await,
        "Registry before sacrificial exhaustion",
    );
    let (candidate, first_id) = run_wrong_guesses(&input, "acceptance-exhausted", 10).await;
    let cancelled = take(
        candidate.pin_recovery().cancel(first_id).await,
        "cancel exhausted operation",
    );
    assert_eq!(cancelled.phase, PinRecoveryPhase::Cancelled);
    drop(candidate);
    let path = input
        .storage_b
        .parent()
        .expect("scope parent")
        .join("acceptance-exhausted");
    let reopened = core(&input, &path, "acceptance-exhausted");
    let session = phone_session(&reopened, &input, None).await;
    require_sacrificial(&input, &session);
    let facade = reopened.pin_recovery();
    let begun = take(
        facade
            .begin_recover(BeginPinRecoveryRequest {
                session: session.session_handle,
                device_label: "exhausted fresh session".into(),
            })
            .await,
        "new session after global exhaustion",
    );
    println!(
        "PIN-DRIVER pending_operation=true operation={}",
        begun.operation_id.0
    );
    assert_eq!(begun.attempts_used, 10);
    expect_error(
        facade
            .submit_pin(
                begun.operation_id.clone(),
                take(
                    SecretPin::new(input.pin.as_bytes().to_vec()),
                    "correct PIN after exhaustion",
                ),
            )
            .await,
        &["guess_budget_exhausted"],
        "reauthentication cannot refund global budget",
    );
    let status = take(
        facade.status(begun.operation_id.clone()).await,
        "persistent exhausted status",
    );
    assert_eq!(status.attempts_used, 10);
    assert!(take(reopened.identities().list(), "exhausted no active identity").is_empty());
    assert_eq!(
        before,
        take(
            owner
                .device_join()
                .registry(sdk::IdentitySelector::Default)
                .await,
            "Registry after sacrificial exhaustion"
        )
    );
    let cancelled = take(
        facade.cancel(begun.operation_id).await,
        "cancel exact exhausted new-session operation",
    );
    assert_eq!(cancelled.phase, PinRecoveryPhase::Cancelled);
    println!("PIN-DRIVER budget_exhausted=true new_session_preserves_budget=true correct_pin_rejected_after_exhaustion=true no_identity=true registry_unchanged=true cancelled=true operation={}", cancelled.operation_id.0);
}
