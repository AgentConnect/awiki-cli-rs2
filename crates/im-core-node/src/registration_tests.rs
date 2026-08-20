use crate::dto::existing_handle_registration_outcome;

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
