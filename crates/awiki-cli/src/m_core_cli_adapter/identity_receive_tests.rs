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
