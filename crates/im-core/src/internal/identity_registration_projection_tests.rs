use super::tests::{test_config, test_paths};
use crate::internal::identity_registration_pending::{
    PendingRegistration, PendingRegistrationStore,
};

#[tokio::test]
async fn registration_resume_projection_survives_reopen_and_excludes_secrets_and_other_domains() {
    let root = tempfile::tempdir().unwrap();
    let core = open_core(root.path());
    let identity = crate::internal::identity_custody::provision_registration_identity_for_method(
        &core,
        "example.test",
        "alice",
        crate::identity::DidMethod::Web,
    )
    .unwrap();
    let pending = PendingRegistration::new(
        "alice".into(),
        "example.test".into(),
        "alice".into(),
        "Alice".into(),
        true,
        "phone".into(),
        Some("test-contact-not-public".into()),
        Some("test-invite-not-public".into()),
        identity,
    )
    .unwrap();
    let store = PendingRegistrationStore::from_core(&core).unwrap();
    let reference = store.save(&pending).unwrap();
    let mut foreign = pending.clone();
    foreign.target_domain = "other.example".into();
    let foreign_reference = store.save(&foreign).unwrap();
    drop(store);
    drop(core);

    let reopened = open_core(root.path());
    let summaries = reopened
        .identities()
        .pending_registrations_async()
        .await
        .unwrap();
    assert_eq!(summaries.len(), 1);
    let public = &summaries[0];
    assert_eq!(public.did, pending.identity.did.as_str());
    assert_eq!(public.full_handle, "alice.example.test");
    assert_eq!(public.method, crate::identity::DidMethod::Web);
    assert_eq!(public.phase, "prepared");
    assert_eq!(public.verification_kind, "phone");
    let value = serde_json::to_value(public).unwrap();
    let keys = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    assert_eq!(
        keys,
        [
            "did",
            "displayName",
            "fullHandle",
            "method",
            "phase",
            "verificationKind"
        ]
    );
    let text = value.to_string();
    for secret in [
        "test-contact-not-public",
        "test-invite-not-public",
        &pending.identity.controller_store_id,
        pending.registration_operation_id.as_deref().unwrap(),
    ] {
        assert!(!text.contains(secret));
    }
    let store = PendingRegistrationStore::from_core(&reopened).unwrap();
    store.delete(&reference).unwrap();
    assert!(reopened
        .identities()
        .pending_registrations_async()
        .await
        .unwrap()
        .is_empty());
    // Reading a tenant projection never removes another domain's operation.
    assert!(store.load("alice", "other.example").unwrap().is_some());
    store.delete(&foreign_reference).unwrap();
}

fn open_core(root: &std::path::Path) -> crate::ImCore {
    crate::ImCore::new_with_options(
        test_config(),
        test_paths(root),
        crate::ImCoreOpenOptions::default().with_identity_secret_vault(
            crate::IdentitySecretStoragePolicy::VaultRequired,
            crate::ImCoreSecretVaultOptions::new(
                crate::vault::DeviceVaultRootKey::from_bytes([0x79; 32]),
                root.join("vault"),
                "registration-projection-test",
                "local-test-device",
            ),
        ),
    )
    .unwrap()
}
