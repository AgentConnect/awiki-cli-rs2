//! Explicit fixture preparation through the same public registration/backup API.
use super::*;

pub(super) async fn prepare(with_backup: bool) {
    use std::{io::Write, path::PathBuf};
    let input = read_input();
    assert!(
        input.register_identity,
        "bootstrap only accepts the explicitly allocated registration fixture"
    );
    let expected = std::env::var("AWIKI_PIN_RECOVERY_BOOTSTRAP_HANDLE")
        .expect("coordinator must allocate the exact sacrificial Handle");
    assert!(expected == input.full_handle);
    assert_ne!(
        input.storage_a, input.storage_b,
        "A owner and fresh B paths must be distinct"
    );
    let owner = core(&input, &input.storage_a, "a");
    assert!(pending(&owner, &input, PinRecoveryKind::Recover)
        .await
        .is_none());
    let inventory = take(owner.identities().list(), "bootstrap source inventory");
    assert!(inventory.len() <= 1);
    let identity = if let Some(existing) = inventory.into_iter().next() {
        assert!(existing
            .handle
            .as_ref()
            .is_some_and(|handle| handle.as_str() == input.full_handle));
        existing
    } else {
        let request = |otp| sdk::identity::RegisterHandleRequest {
            did_method: sdk::identity::DidMethod::Wba,
            local_alias: Some("pin-driver-a".into()),
            requested_handle: take(
                sdk::ids::Handle::parse(&input.full_handle, &input.did_domain),
                "bootstrap exact Handle",
            ),
            verification: sdk::identity::VerificationInput::Phone {
                phone: input.phone.clone(),
                otp,
            },
            invite_code: input.invite_code.clone(),
            profile: sdk::identity::InitialProfile {
                display_name: Some("Realm sacrificial acceptance".into()),
                avatar_url: None,
            },
            make_default: true,
        };
        let first = take(
            owner
                .identities()
                .register_handle_async(request(None))
                .await,
            "bootstrap registration OTP send",
        );
        if let Some(identity) = first.identity {
            identity
        } else {
            take(
                owner
                    .identities()
                    .register_handle_async(request(Some(input.otp.clone())))
                    .await,
                "bootstrap registration completion",
            )
            .identity
            .expect("bootstrap active identity")
        }
    };
    let facade = owner.pin_recovery();
    let before = take(
        facade.backup_status(sdk::IdentitySelector::Default).await,
        "bootstrap current backup status",
    );
    assert!(before.error.is_none());
    assert_eq!(before.state, PinRecoveryBackupState::NotEnabled);
    assert!(before.active_epoch.is_none());
    assert!(pending(&owner, &input, PinRecoveryKind::Enable)
        .await
        .is_none());
    let session = phone_session(&owner, &input, None).await;
    assert!(
        session.target.full_handle == input.full_handle
            && session.target.did == identity.did.as_str()
    );
    assert!(
        session.target.can_enable
            && !session.target.can_recover
            && session.target.active_epoch.is_none()
    );
    let target_id = session.target.target_id.0.clone();
    let mut backup_operation: Option<String> = None;
    if with_backup {
        let mut progress = take(
            facade
                .begin_enable(EnablePinRecoveryRequest {
                    session: session.session_handle,
                    identity: sdk::IdentitySelector::Default,
                    user_presence: UserPresence::confirmed(),
                })
                .await,
            "bootstrap begin explicit backup",
        );
        backup_operation = Some(progress.operation_id.0.clone());
        progress = take(
            facade
                .submit_pin(
                    progress.operation_id,
                    take(
                        SecretPin::new(input.pin.as_bytes().to_vec()),
                        "bootstrap registration PIN",
                    ),
                )
                .await,
            "bootstrap three registered Realms",
        );
        if progress.phase == PinRecoveryPhase::AwaitingPin {
            assert_eq!(
                progress
                    .node_progress
                    .iter()
                    .filter(|node| node.phase == "registered")
                    .count(),
                3
            );
            progress = take(
                facade
                    .submit_pin(
                        progress.operation_id,
                        take(
                            SecretPin::new(input.pin.as_bytes().to_vec()),
                            "bootstrap explicit self-check PIN",
                        ),
                    )
                    .await,
                "bootstrap self-check and active backup",
            );
        }
        assert_eq!(progress.phase, PinRecoveryPhase::Completed);
        let status = take(
            facade.backup_status(sdk::IdentitySelector::Default).await,
            "bootstrap active backup status",
        );
        assert_eq!(status.state, PinRecoveryBackupState::Active);
        assert!(status.error.is_none() && status.active_epoch.is_some());
        // The independent System preflight must query authoritative guesses_used=0
        // before last-correct/exhausted/lost-blinding; no DB counter is changed here.
    }
    let source = PathBuf::from(
        std::env::var_os("AWIKI_PIN_RECOVERY_DRIVER_CONFIG").expect("protected bootstrap input"),
    );
    let output = PathBuf::from(
        std::env::var_os("AWIKI_PIN_RECOVERY_BOOTSTRAP_METADATA")
            .expect("private public-metadata output"),
    );
    assert!(output
        .parent()
        .is_some_and(|parent| parent == source.parent().expect("allocated fixture parent")));
    let metadata = serde_json::json!({"schema_version":1,"environment":"awiki-info-testing",
        "target_id":target_id,"full_handle":input.full_handle,"did":identity.did,"device_id":identity.device_id,
        "source_config":source,"source_owner_storage":input.storage_a,"fresh_candidate_storage":input.storage_b,
        "backup_active":with_backup,"backup_operation_id":backup_operation});
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(output).expect("new exact bootstrap metadata");
    file.write_all(
        &serde_json::to_vec_pretty(&metadata).expect("public bootstrap metadata schema"),
    )
    .expect("public bootstrap metadata write");
    file.sync_all().expect("public bootstrap metadata sync");
    println!("PIN-DRIVER bootstrap_root_prepared=true source_identity_active=true backup_active={with_backup}");
}
