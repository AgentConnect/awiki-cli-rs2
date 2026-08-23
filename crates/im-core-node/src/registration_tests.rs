use crate::dto::{
    admin_device_join_progress, current_device_summary, device_join_approval_prompt,
    existing_handle_registration_outcome, prepared_registration_join_progress,
    terminal_prepared_registration_join_progress,
};

#[test]
fn existing_handle_registration_preserves_the_core_continuation() {
    let outcome = existing_handle_registration_outcome(
        im_core::identity::HandleRegistrationJoinRequiredPreparation {
            preparation_id: "registration-preparation-1".to_owned(),
            mode: im_core::identity::HandleRegistrationJoinMode::HandleRecoveryRebind,
            requires_user_presence: true,
            expected_did: im_core::ids::Did::parse("did:wba:awiki.info:alice").unwrap(),
            full_handle: im_core::ids::Handle::parse("alice.awiki.info", "awiki.info").unwrap(),
        },
        vec!["safe-warning".to_owned()],
    );

    assert_eq!(outcome.status, "existing_handle");
    assert_eq!(outcome.identity, None);
    assert_eq!(outcome.warnings, ["safe-warning"]);
    let existing = outcome.existing_handle.unwrap();
    assert_eq!(existing.continuation_id, "registration-preparation-1");
    assert_eq!(existing.full_handle, "alice.awiki.info");
    assert_eq!(existing.expected_did, "did:wba:awiki.info:alice");
    assert_eq!(existing.mode, "handle_recovery_rebind");
    assert!(existing.requires_user_presence);
}

#[test]
fn ordinary_existing_handle_registration_does_not_claim_recovery_presence() {
    let outcome = existing_handle_registration_outcome(
        im_core::identity::HandleRegistrationJoinRequiredPreparation {
            preparation_id: "registration-preparation-2".to_owned(),
            mode: im_core::identity::HandleRegistrationJoinMode::Ordinary,
            requires_user_presence: false,
            expected_did: im_core::ids::Did::parse("did:wba:awiki.info:bob").unwrap(),
            full_handle: im_core::ids::Handle::parse("bob.awiki.info", "awiki.info").unwrap(),
        },
        Vec::new(),
    );

    let existing = outcome.existing_handle.unwrap();
    assert_eq!(existing.mode, "ordinary");
    assert!(!existing.requires_user_presence);
}

fn authorized_join_progress() -> im_core::identity::AuthorizedJoinActivationProgress {
    im_core::identity::AuthorizedJoinActivationProgress {
        join: im_core::identity::DeviceJoinProgress {
            session: im_core::identity::DeviceJoinSessionView {
                join_session_id: "join-registration-1".to_owned(),
                did: im_core::ids::Did::parse("did:wba:awiki.info:alice").unwrap(),
                protocol_device_id: im_core::ids::ProtocolDeviceId::parse("dev-registration-1")
                    .unwrap(),
                side: im_core::identity::DeviceJoinSide::NewDevice,
                phase: im_core::identity::DeviceJoinLocalPhase::Authorized,
                expires_at: "2026-08-23T12:00:00Z".to_owned(),
            },
            remote_state: im_core::identity::DeviceJoinRemoteState::Consumed,
            sas: Some("482917".to_owned()),
            authorized_device: None,
        },
        reset_reference: None,
    }
}

#[test]
fn prepared_join_requires_identity_before_completed_and_redacts_sas() {
    let incomplete = prepared_registration_join_progress(authorized_join_progress(), None);
    assert!(!incomplete.completed);
    assert_eq!(incomplete.expires_at, "2026-08-23T12:00:00Z");
    assert_eq!(incomplete.sas.as_deref(), Some("482917"));
    let rendered = format!("{incomplete:?}");
    assert!(rendered.contains("<redacted-sas>"));
    assert!(!rendered.contains("482917"));

    let complete = prepared_registration_join_progress(
        authorized_join_progress(),
        Some(crate::dto::NodeIdentity {
            identity_id: "identity-1".to_owned(),
            did: "did:wba:awiki.info:alice".to_owned(),
            handle: Some("alice.awiki.info".to_owned()),
            display_name: None,
            registered_at_ms: "1".to_owned(),
        }),
    );
    assert!(complete.completed);
}

#[test]
fn current_device_management_requires_admin_ready() {
    let identity = im_core::identity::IdentitySummary {
        id: im_core::ids::IdentityId::parse("identity-1").unwrap(),
        did: im_core::ids::Did::parse("did:wba:awiki.info:alice").unwrap(),
        handle: Some(im_core::ids::Handle::parse("alice.awiki.info", "awiki.info").unwrap()),
        display_name: None,
        local_alias: Some("alice".to_owned()),
        device_id: None,
        is_default: true,
        readiness: im_core::identity::IdentityReadiness {
            ready_for_auth: true,
            ready_for_messaging: true,
            missing: Vec::new(),
        },
    };
    let admin = current_device_summary(im_core::identity::IdentityDeviceSummary {
        identity: identity.clone(),
        mode: im_core::identity::IdentityDeviceMode::VNext,
        protocol_device_id: Some(im_core::ids::ProtocolDeviceId::parse("dev-admin").unwrap()),
        role: Some(im_core::identity::IdentityDeviceRole::Admin),
        signing_key_id: Some("did:wba:awiki.info:alice#sign".to_owned()),
        e2ee_key_id: Some("did:wba:awiki.info:alice#e2ee".to_owned()),
        readiness: im_core::identity::IdentityDeviceReadiness::AdminReady,
        blocked_reason: None,
    });
    assert!(admin.can_manage);

    let member = current_device_summary(im_core::identity::IdentityDeviceSummary {
        identity,
        mode: im_core::identity::IdentityDeviceMode::VNext,
        protocol_device_id: Some(im_core::ids::ProtocolDeviceId::parse("dev-member").unwrap()),
        role: Some(im_core::identity::IdentityDeviceRole::Member),
        signing_key_id: Some("did:wba:awiki.info:alice#member-sign".to_owned()),
        e2ee_key_id: Some("did:wba:awiki.info:alice#member-e2ee".to_owned()),
        readiness: im_core::identity::IdentityDeviceReadiness::MemberReady,
        blocked_reason: None,
    });
    assert!(!member.can_manage);
}

#[test]
fn terminal_join_progress_is_local_and_secret_free() {
    for phase in [
        im_core::identity::DeviceJoinLocalPhase::Cancelled,
        im_core::identity::DeviceJoinLocalPhase::Expired,
    ] {
        let value = terminal_prepared_registration_join_progress(
            im_core::identity::DeviceJoinSessionView {
                join_session_id: "join-terminal".to_owned(),
                did: im_core::ids::Did::parse("did:wba:awiki.info:alice").unwrap(),
                protocol_device_id: im_core::ids::ProtocolDeviceId::parse("dev-terminal").unwrap(),
                side: im_core::identity::DeviceJoinSide::NewDevice,
                phase,
                expires_at: "2026-08-23T12:00:00Z".to_owned(),
            },
        )
        .unwrap();
        assert!(value.sas.is_none());
        assert!(!value.completed);
        assert!(value.identity.is_none());
    }
}

#[test]
fn admin_join_and_approval_debug_redact_sas_and_handle() {
    let progress = admin_device_join_progress(im_core::identity::DeviceJoinProgress {
        session: authorized_join_progress().join.session,
        remote_state: im_core::identity::DeviceJoinRemoteState::ResponseVerified,
        sas: Some("482917".to_owned()),
        authorized_device: None,
    });
    let rendered = format!("{progress:?}");
    assert!(rendered.contains("<redacted-sas>"));
    assert!(!rendered.contains("482917"));

    let prompt = device_join_approval_prompt(im_core::identity::DeviceJoinApprovalPrompt {
        approval_handle: "secret-approval-handle".to_owned(),
        join_session_id: "join-registration-1".to_owned(),
        sas: "482917".to_owned(),
        expires_at: "2026-08-23T12:00:00Z".to_owned(),
    });
    let rendered = format!("{prompt:?}");
    assert!(!rendered.contains("secret-approval-handle"));
    assert!(!rendered.contains("482917"));
}
