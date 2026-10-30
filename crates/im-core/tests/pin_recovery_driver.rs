//! Test-only real Core entry points. Inputs are read from a protected file.
use awiki_im_core as sdk;
use sdk::pin_recovery::*;
use zeroize::Zeroizing;
#[path = "support/pin_recovery_live.rs"]
mod live_support;
use live_support::*;
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires explicit AWIKI_PIN_RECOVERY_DRIVER_CONFIG protected live environment"]
async fn live_pin_root_roundtrip() {
    let input = read_input();
    assert_ne!(
        input.storage_a, input.storage_b,
        "driver requires independent stores"
    );
    let a = core(&input, &input.storage_a, "a");
    let existing = take(a.identities().list(), "source inventory")
        .into_iter()
        .find(|i| {
            i.handle
                .as_ref()
                .is_some_and(|h| h.as_str() == input.full_handle)
        });
    let identity = if let Some(existing) = existing {
        existing
    } else if input.register_identity {
        let request = |otp| sdk::identity::RegisterHandleRequest {
            did_method: sdk::identity::DidMethod::Wba,
            local_alias: Some("pin-driver-a".into()),
            requested_handle: take(
                sdk::ids::Handle::parse(&input.full_handle, &input.did_domain),
                "exact Handle",
            ),
            verification: sdk::identity::VerificationInput::Phone {
                phone: input.phone.clone(),
                otp,
            },
            invite_code: input.invite_code.clone(),
            profile: sdk::identity::InitialProfile {
                display_name: Some("PIN integration A".into()),
                avatar_url: None,
            },
            make_default: true,
        };
        let first = take(
            a.identities().register_handle_async(request(None)).await,
            "registration OTP send",
        );
        if let Some(identity) = first.identity {
            identity
        } else {
            take(
                a.identities()
                    .register_handle_async(request(Some(input.otp.clone())))
                    .await,
                "registration completion",
            )
            .identity
            .expect("driver active identity")
        }
    } else {
        take(
            a.identities().resolve(sdk::IdentitySelector::Default),
            "existing test identity",
        )
    };
    let original_did = identity.did.clone();
    let original_device = identity.device_id.clone();
    let pending_a = pending(&a, &input, PinRecoveryKind::Enable).await;
    let session = phone_session(
        &a,
        &input,
        pending_a.as_ref().map(|p| p.operation_id.clone()),
    )
    .await;
    assert_eq!(
        session.target.did,
        original_did.as_str(),
        "target must equal local identity"
    );
    let facade = a.pin_recovery();
    if pending_a.is_some() || session.target.can_enable {
        let mut backup = if let Some(pending) = pending_a {
            take(
                facade
                    .resume(ResumePinRecoveryRequest {
                        operation_id: pending.operation_id,
                        session: Some(session.session_handle),
                    })
                    .await,
                "resume backup",
            )
        } else {
            take(
                facade
                    .begin_enable(EnablePinRecoveryRequest {
                        session: session.session_handle,
                        identity: sdk::IdentitySelector::Id(identity.id.clone()),
                        user_presence: UserPresence::confirmed(),
                    })
                    .await,
                "begin backup",
            )
        };
        if backup
            .allowed_actions
            .contains(&PinRecoveryAction::SubmitPin)
        {
            backup = take(
                facade
                    .submit_pin(
                        backup.operation_id,
                        take(SecretPin::new(input.pin.as_bytes().to_vec()), "PIN format"),
                    )
                    .await,
                "register, readback and activate backup",
            );
        }
        assert_eq!(
            backup.phase,
            PinRecoveryPhase::Completed,
            "backup must be fully active"
        );
        println!(
            "PIN-DRIVER backup_complete operation={}",
            backup.operation_id.0
        );
    } else {
        assert!(
            session.target.can_recover,
            "same source target must have an active backup"
        );
        println!("PIN-DRIVER backup_active_from_prior_attempt=true");
    }
    drop(facade);
    drop(a); // No original device is used in the recovery path.
    let b = core(&input, &input.storage_b, "b");
    let pending_b = pending(&b, &input, PinRecoveryKind::Recover).await;
    let inventory = take(b.identities().list(), "recovery device inventory");
    if pending_b.is_none() && !inventory.is_empty() {
        assert_eq!(inventory.len(), 1, "B contains only the test device");
        assert_eq!(inventory[0].did, original_did);
        assert_ne!(inventory[0].device_id, original_device);
        take(
            b.client_async(sdk::IdentitySelector::Id(inventory[0].id.clone()))
                .await,
            "previously activated client",
        );
        println!("PIN-DRIVER original_did_preserved=true new_device=true phase=completed prior_attempt=true");
        return;
    }
    let session = phone_session(
        &b,
        &input,
        pending_b.as_ref().map(|p| p.operation_id.clone()),
    )
    .await;
    assert!(
        session.target.can_recover,
        "active backup must be recoverable"
    );
    let facade = b.pin_recovery();
    let mut recovered = if let Some(pending) = pending_b {
        take(
            facade
                .resume(ResumePinRecoveryRequest {
                    operation_id: pending.operation_id,
                    session: Some(session.session_handle),
                })
                .await,
            "resume original-root recovery",
        )
    } else {
        assert!(inventory.is_empty(), "B must initially have no identity");
        take(
            facade
                .begin_recover(BeginPinRecoveryRequest {
                    session: session.session_handle,
                    device_label: "PIN integration B".into(),
                })
                .await,
            "begin original-root recovery",
        )
    };
    if recovered
        .allowed_actions
        .contains(&PinRecoveryAction::SubmitPin)
    {
        recovered = take(
            facade
                .submit_pin(
                    recovered.operation_id,
                    take(SecretPin::new(input.pin.as_bytes().to_vec()), "PIN format"),
                )
                .await,
            "recover key into restricted pending",
        );
    }
    let activated = if recovered
        .allowed_actions
        .contains(&PinRecoveryAction::ConfirmDevice)
    {
        assert!(
            recovered.recovered_identity.is_none(),
            "pending cannot expose an active identity"
        );
        take(
            facade
                .confirm_device(ConfirmRecoveredDeviceRequest {
                    operation_id: recovered.operation_id,
                    user_presence: UserPresence::confirmed(),
                })
                .await,
            "commit new device and activate local custody",
        )
    } else {
        recovered
    };
    assert_eq!(activated.phase, PinRecoveryPhase::Completed);
    let selector = activated.recovered_identity.expect("activated selector");
    let identity = take(
        b.identities().resolve(selector.clone()),
        "activated identity",
    );
    assert_eq!(identity.did, original_did);
    assert_ne!(identity.device_id, original_device);
    let _authenticated_client = take(
        b.client_async(selector).await,
        "normal authenticated client",
    );
    println!(
        "PIN-DRIVER original_did_preserved=true new_device=true phase=completed operation={}",
        activated.operation_id.0
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "explicit live PIN failure and budget evidence against the protected test Handle"]
async fn live_pin_wrong_pin_is_one_attempt_without_device() {
    let input = read_input();
    let b = core(&input, &input.storage_b, "b");
    let before = take(
        b.device_join()
            .registry(sdk::IdentitySelector::Default)
            .await,
        "authenticated Registry before wrong PIN",
    );
    let storage_c = input
        .storage_b
        .parent()
        .expect("test scope parent")
        .join("client-c-wrong-pin");
    let c = core(&input, &storage_c, "c-wrong-pin");
    assert!(
        take(c.identities().list(), "C initial identities").is_empty(),
        "wrong PIN device begins without identity"
    );
    assert!(
        pending(&c, &input, PinRecoveryKind::Recover)
            .await
            .is_none(),
        "reconcile an earlier uncertain failure before explicitly making another PIN attempt"
    );
    let session = phone_session(&c, &input, None).await;
    assert!(
        session.target.can_recover,
        "exact test Handle has active backup"
    );
    let facade = c.pin_recovery();
    let begun = take(
        facade
            .begin_recover(BeginPinRecoveryRequest {
                session: session.session_handle,
                device_label: "PIN failure C".into(),
            })
            .await,
        "begin wrong-PIN operation",
    );
    let used_before = begun.attempts_used;
    let mut wrong = Zeroizing::new(input.pin.as_bytes().to_vec());
    assert_eq!(wrong.len(), 4);
    wrong[3] = b'0' + (wrong[3] - b'0' + 1) % 10;
    let failed = facade
        .submit_pin(
            begun.operation_id.clone(),
            take(SecretPin::new(wrong.to_vec()), "wrong PIN format"),
        )
        .await;
    match failed {
        Err(sdk::ImError::Service {
            code: Some(code), ..
        }) if code == "invalid_pin" => {}
        Err(_) => panic!("wrong PIN did not return stable invalid_pin"),
        Ok(_) => panic!("wrong PIN was accepted"),
    }
    let status = take(
        facade.status(begun.operation_id.clone()).await,
        "authoritative wrong-PIN status",
    );
    assert_eq!(
        status.attempts_used,
        used_before + 1,
        "one explicit PIN submit is one global attempt across Realm requests"
    );
    assert!(status.recovered_identity.is_none());
    assert!(status.new_device_id.is_none());
    assert!(take(c.identities().list(), "C identities after wrong PIN").is_empty());
    let after = take(
        b.device_join()
            .registry(sdk::IdentitySelector::Default)
            .await,
        "authenticated Registry after wrong PIN",
    );
    assert_eq!(
        before, after,
        "wrong PIN must not register any device or change Registry"
    );
    let cancelled = take(
        facade.cancel(begun.operation_id).await,
        "cancel uncommitted wrong-PIN operation",
    );
    assert_eq!(cancelled.phase, PinRecoveryPhase::Cancelled);
    assert!(take(c.identities().list(), "C identities after cancel").is_empty());
    println!("PIN-DRIVER wrong_pin_rejected=true attempt_delta=1 active_identity_count=0 registry_unchanged=true cancelled=true operation={}",cancelled.operation_id.0);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "deployment owner must stop the named Realm and confirm health before this live 2-of-3 test"]
async fn live_pin_recovers_with_one_realm_unavailable() {
    let unavailable = std::env::var("AWIKI_PIN_RECOVERY_UNAVAILABLE_REALM_ID")
        .expect("public unavailable Realm ID is required");
    assert!(
        unavailable.len() == 32 && unavailable.bytes().all(|b| b.is_ascii_hexdigit()),
        "Realm ID format"
    );
    let input = read_input();
    let b = core(&input, &input.storage_b, "b");
    let before = take(
        b.device_join()
            .registry(sdk::IdentitySelector::Default)
            .await,
        "Registry before single-Realm outage",
    );
    let storage_d = input
        .storage_b
        .parent()
        .expect("test scope parent")
        .join("client-d-one-realm");
    let d = core(&input, &storage_d, "d-one-realm");
    assert!(take(d.identities().list(), "D initial identities").is_empty());
    assert!(
        pending(&d, &input, PinRecoveryKind::Recover)
            .await
            .is_none(),
        "reconcile prior D operation before retrying"
    );
    let session = phone_session(&d, &input, None).await;
    let facade = d.pin_recovery();
    let begun = take(
        facade
            .begin_recover(BeginPinRecoveryRequest {
                session: session.session_handle,
                device_label: "PIN outage D".into(),
            })
            .await,
        "begin outage recovery",
    );
    assert!(begun
        .node_progress
        .iter()
        .any(|n| n.realm_id == unavailable));
    let recovered = take(
        facade
            .submit_pin(
                begun.operation_id.clone(),
                take(SecretPin::new(input.pin.as_bytes().to_vec()), "PIN format"),
            )
            .await,
        "two-Realm original-root recovery",
    );
    assert_eq!(
        recovered.phase,
        PinRecoveryPhase::AwaitingDeviceConfirmation
    );
    assert_eq!(
        recovered
            .node_progress
            .iter()
            .filter(|n| n.phase == "verified")
            .count(),
        2
    );
    assert!(recovered
        .node_progress
        .iter()
        .find(|n| n.realm_id == unavailable)
        .is_some_and(|n| n.phase != "verified"));
    assert!(recovered.recovered_identity.is_none());
    assert!(recovered.new_device_id.is_some());
    assert!(take(
        d.identities().list(),
        "D identities while restricted pending"
    )
    .is_empty());
    let after = take(
        b.device_join()
            .registry(sdk::IdentitySelector::Default)
            .await,
        "Registry after restricted outage recovery",
    );
    assert_eq!(before, after, "restricted pending cannot register a device");
    let cancelled = take(
        facade.cancel(begun.operation_id).await,
        "cancel exact outage pending",
    );
    assert_eq!(cancelled.phase, PinRecoveryPhase::Cancelled);
    assert!(take(d.identities().list(), "D identities after pending cleanup").is_empty());
    println!("PIN-DRIVER single_realm_unavailable=true recovered_from_realms=2 original_root_validated=true restricted_pending=true registry_unchanged=true cancelled=true operation={}",cancelled.operation_id.0);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires deployed device-status and the previously accepted A/B test scope"]
async fn live_pin_backup_status_uses_current_device_and_unknown_on_offline() {
    let input = read_input();
    let a = core(&input, &input.storage_a, "a");
    let b = core(&input, &input.storage_b, "b");
    let a_status = take(
        a.pin_recovery()
            .backup_status(sdk::IdentitySelector::Default)
            .await,
        "A authenticated backup status",
    );
    let b_status = take(
        b.pin_recovery()
            .backup_status(sdk::IdentitySelector::Default)
            .await,
        "B authenticated backup status",
    );
    assert_eq!(
        a_status.state,
        PinRecoveryBackupState::Active,
        "A must read authoritative active metadata without new OTP"
    );
    assert_eq!(b_status.state, PinRecoveryBackupState::Active);
    assert_eq!(a_status.did, b_status.did);
    assert_eq!(a_status.active_epoch, b_status.active_epoch);
    assert!(a_status.error.is_none() && b_status.error.is_none());
    let absent_ca = input
        .storage_b
        .join(format!("absent-status-ca-{}.pem", uuid::Uuid::new_v4()));
    let offline = core_with_ca(
        &input,
        &input.storage_b,
        "b",
        Some(absent_ca.to_string_lossy().into_owned()),
    );
    let unavailable = take(
        offline
            .pin_recovery()
            .backup_status(sdk::IdentitySelector::Default)
            .await,
        "offline status remains a typed unknown",
    );
    assert_eq!(unavailable.state, PinRecoveryBackupState::Unknown);
    assert!(unavailable.error.is_some());
    assert!(unavailable.active_epoch.is_none());
    println!("PIN-DRIVER backup_status_authenticated=true same_did=true state_active=true offline_unknown=true no_phone_factor=true");
}

#[cfg(feature = "identity-native-anp")]
#[derive(Default, serde::Serialize, serde::Deserialize)]
struct S4Ids {
    scope: String,
    old_pin_operation: Option<String>,
    v4_operation: Option<String>,
    old_did: Option<String>,
    new_backup_operation: Option<String>,
    new_recovery_operation: Option<String>,
}
#[cfg(feature = "identity-native-anp")]
fn s4_ids_path() -> std::path::PathBuf {
    std::env::var_os("AWIKI_PIN_RECOVERY_S4_EVIDENCE_PATH")
        .map(Into::into)
        .unwrap_or_else(|| {
            std::path::PathBuf::from("target/test-results/pin-recovery/s4-20261031/ids.json")
        })
}
#[cfg(feature = "identity-native-anp")]
fn save_s4_ids(ids: &S4Ids) {
    let path = s4_ids_path();
    std::fs::create_dir_all(path.parent().unwrap()).expect("S4 evidence directory");
    std::fs::write(path, serde_json::to_vec_pretty(ids).unwrap()).expect("public S4 ID evidence");
}
#[cfg(feature = "identity-native-anp")]
fn s4_provider_dids(input: &Input, storage: &std::path::Path, label: &str) -> Vec<String> {
    let manager = anp_identity::IdentityManager::open(anp_identity::IdentityManagerConfig {
        state_root: storage.join("identities/.anp-identity"),
        root_key: anp_identity::RootKeySource::Injected(anp_identity::InjectedStoreKey::new(
            format!("awiki-workspace-vault:pin-driver-{label}"),
            key(storage),
        )),
    })
    .unwrap_or_else(|_| panic!("S4 native custody audit could not open its exact store"));
    let _ = input;
    manager
        .list()
        .unwrap_or_else(|_| panic!("S4 native custody audit failed"))
        .into_iter()
        .map(|i| i.reference.did)
        .collect()
}
#[tokio::test(flavor = "multi_thread")]
#[ignore = "explicit destructive V4 migration of the preserved S2 test Handle; coordinate scope with the owner"]
#[cfg(feature = "identity-native-anp")]
async fn live_v4_retires_old_backup_and_allows_new_backup() {
    let input = read_input();
    assert!(
        input.multi_device_audience.is_some(),
        "actual V4 audience must be installed in protected test input"
    );
    let storage_e = input.storage_b.parent().unwrap().join("client-e-v4");
    let e = core(&input, &storage_e, "e-v4");
    let scope = format!("{}:{}", storage_e.to_string_lossy(), input.full_handle);
    let path = s4_ids_path();
    let mut ids = if path.exists() {
        serde_json::from_slice::<S4Ids>(&std::fs::read(path).unwrap())
            .unwrap_or_else(|_| panic!("public S4 evidence invalid"))
    } else {
        S4Ids {
            scope: scope.clone(),
            ..Default::default()
        }
    };
    assert_eq!(
        ids.scope, scope,
        "never reuse another test scope's operation IDs"
    );
    let mut v4_id = ids.v4_operation.clone();
    if v4_id.is_none() {
        let pending_v4 = take(
            e.handle_recovery()
                .list_pending_handle_recovery_operations(),
            "local V4 journal inspection",
        );
        if let Some(p) = pending_v4
            .into_iter()
            .find(|p| p.full_handle == input.full_handle)
        {
            v4_id = Some(p.operation_id);
        } else if !take(e.identities().list(), "E local inventory").is_empty() {
            let all = take(
                e.handle_recovery()
                    .list_handle_recovery_operations(sdk::IdentitySelector::Default)
                    .await,
                "E V4 history",
            );
            v4_id = all
                .into_iter()
                .find(|p| p.full_handle == input.full_handle)
                .map(|p| p.operation_id);
        }
    }
    if v4_id.is_none() {
        let existing = pending(&e, &input, PinRecoveryKind::Recover).await;
        let session = phone_session(
            &e,
            &input,
            existing.as_ref().map(|p| p.operation_id.clone()),
        )
        .await;
        let mut recovered = if let Some(p) = existing {
            take(
                e.pin_recovery()
                    .resume(ResumePinRecoveryRequest {
                        operation_id: p.operation_id,
                        session: Some(session.session_handle),
                    })
                    .await,
                "resume old restricted root",
            )
        } else {
            take(
                e.pin_recovery()
                    .begin_recover(BeginPinRecoveryRequest {
                        session: session.session_handle,
                        device_label: "V4 old restricted pending E".into(),
                    })
                    .await,
                "begin old restricted root",
            )
        };
        ids.old_pin_operation = Some(recovered.operation_id.0.clone());
        ids.old_did = Some(recovered.target.did.clone());
        save_s4_ids(&ids);
        if recovered
            .allowed_actions
            .contains(&PinRecoveryAction::SubmitPin)
        {
            recovered = take(
                e.pin_recovery()
                    .submit_pin(
                        recovered.operation_id,
                        take(SecretPin::new(input.pin.as_bytes().to_vec()), "PIN format"),
                    )
                    .await,
                "recover old root before explicit V4",
            );
        }
        assert_eq!(
            recovered.phase,
            PinRecoveryPhase::AwaitingDeviceConfirmation
        );
        assert!(recovered.recovered_identity.is_none());
        assert!(take(e.identities().list(), "E has no projected old identity").is_empty());
        assert!(
            s4_provider_dids(&input, &storage_e, "e-v4").contains(ids.old_did.as_ref().unwrap())
        );
        println!(
            "PIN-DRIVER v4_old_pending_ready=true operation={}",
            ids.old_pin_operation.as_ref().unwrap()
        );
        let otp = take(
            e.handle_recovery()
                .request_handle_recovery_otp(sdk::identity::HandleRecoveryOtpRequest {
                    identity: None,
                    full_handle: input.full_handle.clone(),
                    phone: input.phone.clone(),
                })
                .await,
            "V4 scoped phone challenge",
        );
        v4_id = Some(otp.operation_id);
    }
    ids.v4_operation = v4_id;
    save_s4_ids(&ids);
    let v4_id = ids.v4_operation.clone().unwrap();
    let mut v4 = take(
        e.handle_recovery().handle_recovery_status(&v4_id),
        "V4 local status",
    );
    if v4.phase == sdk::identity::HandleRecoveryPhase::AwaitingFactor {
        v4 = take(
            e.handle_recovery()
                .prepare_handle_recovery(sdk::identity::HandleRecoveryPrepareRequest {
                    operation_id: v4_id.clone(),
                    phone: input.phone.clone(),
                    code: input.otp.clone(),
                })
                .await,
            "V4 factor and immutable new identity",
        );
    }
    if v4.phase == sdk::identity::HandleRecoveryPhase::ReadyToCommit {
        v4 = take(
            e.handle_recovery()
                .activate_handle_recovery(sdk::identity::HandleRecoveryActivateRequest {
                    operation_id: v4_id.clone(),
                    user_presence_confirmed: true,
                })
                .await,
            "explicit V4 remote commit and local activation",
        );
    } else if v4.phase != sdk::identity::HandleRecoveryPhase::Applied {
        v4 = take(
            e.handle_recovery()
                .resume_handle_recovery(sdk::identity::HandleRecoveryResumeRequest {
                    operation_id: v4_id.clone(),
                })
                .await,
            "reconcile same V4 operation",
        );
    }
    if v4.phase == sdk::identity::HandleRecoveryPhase::Applied
        && v4
            .allowed_actions
            .contains(&sdk::identity::HandleRecoveryAction::Resume)
    {
        v4 = take(
            e.handle_recovery()
                .resume_handle_recovery(sdk::identity::HandleRecoveryResumeRequest {
                    operation_id: v4_id.clone(),
                })
                .await,
            "finish exact old PIN cleanup",
        );
    }
    assert_eq!(v4.phase, sdk::identity::HandleRecoveryPhase::Applied);
    let new_did = v4.current_did.as_str().to_owned();
    let old_did = ids
        .old_did
        .clone()
        .or_else(|| {
            v4.local_previous_did
                .as_ref()
                .map(|d| d.as_str().to_owned())
        })
        .expect("old DID evidence");
    assert_ne!(new_did, old_did, "V4 must create a different root identity");
    let old_pin = ids
        .old_pin_operation
        .clone()
        .expect("old PIN operation evidence");
    let old_status = take(
        e.pin_recovery()
            .status(PinRecoveryOperationId(old_pin.clone()))
            .await,
        "old PIN journal is superseded locally",
    );
    assert_eq!(old_status.phase, PinRecoveryPhase::Cancelled);
    assert!(old_status.recovered_identity.is_none());
    assert!(
        !take(
            e.handle_recovery()
                .list_pending_handle_recovery_operations(),
            "V4 cleanup completion"
        )
        .iter()
        .any(|p| p.operation_id == v4_id),
        "old PIN cleanup debt must not be hidden"
    );
    let namespaces = s4_provider_dids(&input, &storage_e, "e-v4");
    assert!(
        !namespaces.contains(&old_did),
        "unprojected old namespace is physically absent"
    );
    assert!(namespaces.contains(&new_did));
    let status = take(
        e.pin_recovery()
            .backup_status(sdk::IdentitySelector::Default)
            .await,
        "new identity backup status after skip",
    );
    if ids.new_backup_operation.is_none() {
        assert_eq!(
            status.state,
            PinRecoveryBackupState::NotEnabled,
            "explicit skip preserves a usable new identity without creating a backup"
        );
        assert!(
            take(
                e.client_async(sdk::IdentitySelector::Default).await,
                "V4 new identity usable before backup"
            )
            .did()
            .as_str()
                == new_did
        );
        println!("PIN-DRIVER new_root_skip_preserved=true v4_operation={v4_id}");
    }
    let backup_pending = pending(&e, &input, PinRecoveryKind::Enable).await;
    let phone = phone_session(
        &e,
        &input,
        backup_pending.as_ref().map(|p| p.operation_id.clone()),
    )
    .await;
    assert_eq!(phone.target.did, new_did);
    if phone.target.can_enable || backup_pending.is_some() {
        let mut backup = if let Some(p) = backup_pending {
            take(
                e.pin_recovery()
                    .resume(ResumePinRecoveryRequest {
                        operation_id: p.operation_id,
                        session: Some(phone.session_handle),
                    })
                    .await,
                "resume new-root backup",
            )
        } else {
            take(
                e.pin_recovery()
                    .begin_enable(EnablePinRecoveryRequest {
                        session: phone.session_handle,
                        identity: sdk::IdentitySelector::Default,
                        user_presence: UserPresence::confirmed(),
                    })
                    .await,
                "explicitly enable new-root backup",
            )
        };
        ids.new_backup_operation = Some(backup.operation_id.0.clone());
        save_s4_ids(&ids);
        if backup
            .allowed_actions
            .contains(&PinRecoveryAction::SubmitPin)
        {
            backup = take(
                e.pin_recovery()
                    .submit_pin(
                        backup.operation_id,
                        take(SecretPin::new(input.pin.as_bytes().to_vec()), "PIN format"),
                    )
                    .await,
                "backup new root with fresh slot and K",
            );
        }
        assert_eq!(backup.phase, PinRecoveryPhase::Completed);
    } else {
        assert!(phone.target.can_recover);
    }
    let storage_f = input.storage_b.parent().unwrap().join("client-f-new-root");
    let f = core(&input, &storage_f, "f-new-root");
    let pending_f = pending(&f, &input, PinRecoveryKind::Recover).await;
    let phone = phone_session(
        &f,
        &input,
        pending_f.as_ref().map(|p| p.operation_id.clone()),
    )
    .await;
    assert_eq!(phone.target.did, new_did);
    let mut recovered = if let Some(p) = pending_f {
        take(
            f.pin_recovery()
                .resume(ResumePinRecoveryRequest {
                    operation_id: p.operation_id,
                    session: Some(phone.session_handle),
                })
                .await,
            "resume exact new-root recovery",
        )
    } else {
        take(
            f.pin_recovery()
                .begin_recover(BeginPinRecoveryRequest {
                    session: phone.session_handle,
                    device_label: "V4 new-root device F".into(),
                })
                .await,
            "begin new-root recovery",
        )
    };
    ids.new_recovery_operation = Some(recovered.operation_id.0.clone());
    save_s4_ids(&ids);
    if recovered
        .allowed_actions
        .contains(&PinRecoveryAction::SubmitPin)
    {
        recovered = take(
            f.pin_recovery()
                .submit_pin(
                    recovered.operation_id,
                    take(SecretPin::new(input.pin.as_bytes().to_vec()), "PIN format"),
                )
                .await,
            "recover new root",
        );
    }
    if recovered
        .allowed_actions
        .contains(&PinRecoveryAction::ConfirmDevice)
    {
        recovered = take(
            f.pin_recovery()
                .confirm_device(ConfirmRecoveredDeviceRequest {
                    operation_id: recovered.operation_id,
                    user_presence: UserPresence::confirmed(),
                })
                .await,
            "activate new-root F device",
        );
    }
    assert_eq!(recovered.phase, PinRecoveryPhase::Completed);
    let identity = take(
        f.identities()
            .resolve(recovered.recovered_identity.unwrap()),
        "F new identity projection",
    );
    assert_eq!(identity.did.as_str(), new_did);
    println!("PIN-DRIVER v4_applied=true old_pending_cancelled=true old_namespace_absent=true new_root_skip_preserved=true new_root_backup=true new_root_recovered=true original_root_changed=true v4_operation={} old_pin_operation={} new_backup_operation={} new_recovery_operation={}",v4_id,old_pin,ids.new_backup_operation.unwrap_or_default(),ids.new_recovery_operation.unwrap_or_default());
}

#[cfg(feature = "identity-native-anp")]
fn export_s4_public_metadata(input: &Input) {
    use sdk::vault::SecretVault;
    let storage = input.storage_b.parent().unwrap().join("client-e-v4");
    let ids: S4Ids =
        serde_json::from_slice(&std::fs::read(s4_ids_path()).expect("public S4 IDs")).unwrap();
    let old_id = ids.old_pin_operation.as_ref().expect("old operation ID");
    let vault = sdk::vault::FileSecretVault::new(
        sdk::vault::DeviceVaultRootKey::from_bytes(key(&storage)),
        sdk::vault::FileSecretVaultStore::new(storage.join("vault")),
    );
    let references = take(vault.list(), "S4 public metadata Vault references");
    let reference = references
        .iter()
        .find(|r| {
            r.kind == sdk::vault::SecretKind::IdentityPinRecoveryPending
                && r.key_id == format!("pin-recovery/operation/{old_id}")
        })
        .expect("exact old operation record");
    let bytes = take(vault.open(reference), "old operation audit");
    let operation: serde_json::Value = serde_json::from_slice(bytes.expose_secret())
        .unwrap_or_else(|_| panic!("old operation audit schema"));
    assert!(
        operation["wrapping_key_b64u"].is_null()
            && operation["recovered_root_b64u"].is_null()
            && operation["attempt"].is_null()
    );
    assert_eq!(operation["registration_payloads"], serde_json::json!({}));
    assert_eq!(operation["v4_cleanup"]["core_cleared"], true);
    let old_did = operation["target"]["did"].as_str().unwrap();
    let mut old_sessions = 0;
    for reference in references.iter().filter(|r| {
        r.kind == sdk::vault::SecretKind::IdentityPinRecoveryPending
            && r.key_id.starts_with("pin-recovery/session/")
    }) {
        let bytes = take(vault.open(reference), "session secret cleanup audit");
        let s: serde_json::Value = serde_json::from_slice(bytes.expose_secret())
            .unwrap_or_else(|_| panic!("session audit schema"));
        if s["selected"]["did"].as_str() == Some(old_did)
            || s["resume_operation_id"].as_str() == Some(old_id)
        {
            old_sessions += 1;
        }
        assert!(
            !s["exact_requests"]
                .as_object()
                .unwrap()
                .values()
                .any(|r| r["operation_id"].as_str() == Some(old_id)),
            "no old exact secret payload remains in a live session"
        );
    }
    assert_eq!(
        old_sessions, 0,
        "all old original/resumed credentials are removed"
    );
    let remote = &operation["remote"];
    let metadata = serde_json::json!({"schema_version":1,"environment":"awiki-info-testing","v4_operation_id":ids.v4_operation,"old_pin_operation_id":old_id,"account_user_id":operation["account_user_id"],"target_id":operation["target"]["target_id"],"did":old_did,"full_handle":operation["target"]["full_handle"],"binding_generation":operation["target"]["binding_generation"],"recovery_id":remote["recovery_id"],"slot_id":remote["slot_id"],"backup_epoch":remote["backup_epoch"],"registration_version":remote["registration_version"],"backup_hash":remote["backup_hash"],"config_hash":remote["config_hash"],"old_session_count":old_sessions,"old_core_secret_fields_cleared":true,"new_backup_operation_id":ids.new_backup_operation,"new_recovery_operation_id":ids.new_recovery_operation});
    let path = s4_ids_path()
        .parent()
        .unwrap()
        .join("old-backup-metadata.json");
    std::fs::write(path, serde_json::to_vec_pretty(&metadata).unwrap())
        .expect("public old-slot audit metadata");
    println!("PIN-DRIVER old_core_secret_fields_cleared=true old_session_count=0 public_old_slot_metadata_written=true");
}
#[cfg(feature = "identity-native-anp")]
#[tokio::test]
#[ignore = "read-only exact S4 local metadata export; no OTP/PIN/network or new operation"]
async fn live_v4_export_public_metadata_audit() {
    export_s4_public_metadata(&read_input());
}
