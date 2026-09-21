use super::*;
use crate::identity::DidMethod;
use crate::internal::identity_registration_pending::{
    PendingRegistration, PendingRegistrationStore,
};
use crate::internal::identity_wire::web_registration as wire;
use serde_json::json;

fn open_core(root: &std::path::Path) -> crate::ImCore {
    crate::ImCore::new_with_options(
        super::tests::test_config(),
        super::tests::test_paths(root),
        crate::ImCoreOpenOptions::default()
            .with_multi_device_audience("awiki-user-service")
            .with_identity_secret_vault(
                crate::IdentitySecretStoragePolicy::VaultRequired,
                crate::ImCoreSecretVaultOptions::new(
                    crate::vault::DeviceVaultRootKey::from_bytes([0x47; 32]),
                    root.join("vault"),
                    "web-registration-tests",
                    "local-device",
                ),
            ),
    )
    .unwrap()
}

fn request() -> crate::identity::RegisterHandleRequest {
    crate::identity::RegisterHandleRequest {
        did_method: DidMethod::Web,
        local_alias: Some("alice".into()),
        requested_handle: crate::ids::Handle::parse("alice.example.test", "").unwrap(),
        verification: crate::identity::VerificationInput::Phone {
            phone: "+15555550123".into(),
            otp: Some("123456".into()),
        },
        invite_code: None,
        profile: crate::identity::InitialProfile {
            display_name: Some("Alice".into()),
            avatar_url: None,
        },
        make_default: true,
    }
}

fn pending(core: &crate::ImCore) -> PendingRegistration {
    let identity = crate::internal::identity_custody::provision_registration_identity_for_method(
        core,
        "example.test",
        "alice",
        DidMethod::Web,
    )
    .unwrap();
    PendingRegistration::new(
        "alice".into(),
        "example.test".into(),
        "alice".into(),
        "Alice".into(),
        true,
        "phone".into(),
        Some("+15555550123".into()),
        None,
        identity,
    )
    .unwrap()
}

#[tokio::test]
async fn web_registration_current_document_survives_commit_interruption_and_reopen() {
    use crate::internal::identity_wire::web_registration_result::CurrentRegistrationDocument;
    let root = tempfile::tempdir().unwrap();
    let core = open_core(root.path());
    let mut pending = pending(&core);
    let call = register_call(&pending, &request(), None).unwrap();
    sign_web_register_call_async(&core, &mut pending, call)
        .await
        .unwrap();
    verify_web_registration_request(&core, &pending, &request()).unwrap();
    let mut changed_request = request();
    changed_request.profile.avatar_url = Some("https://example.test/another-avatar".to_owned());
    assert!(verify_web_registration_request(&core, &pending, &changed_request).is_err());
    crate::internal::identity_custody::begin_registration_publication_async(
        &core,
        &pending.identity,
    )
    .await
    .unwrap();
    crate::internal::identity_custody::reconcile_registration_publication_async(
        &core,
        &pending.identity,
        true,
    )
    .await
    .unwrap();
    pending.remote_attempted = true;
    let original = pending.identity.clone();
    let mut document = original.did_document.clone();
    document["alsoKnownAs"] = json!(["https://example.test/alice"]);
    let current = CurrentRegistrationDocument {
        checkpoint: crate::internal::identity_device_state::IdentityInternalCheckpoint {
            document_version: 3,
            registry_version: 1,
            document_hash: crate::internal::identity_wire::document::document_hash(&document)
                .unwrap(),
        },
        document,
    };
    let token = super::tests::access_token(&pending, &pending.identity.device_signing_key_id);
    apply_registration_reconciliation(
        &mut pending,
        crate::internal::transport::PendingRegistrationReconciliation::Committed {
            current: Some(current.clone()),
            user_id: "user-1".into(),
            binding_generation: "1".into(),
            access_token: token,
        },
    )
    .unwrap();
    PendingRegistrationStore::from_core(&core)
        .unwrap()
        .save(&pending)
        .unwrap();
    // The process can stop after custody convergence but before business projection.
    crate::internal::identity_custody::adopt_registered_web_document_async(
        &core, &pending, &current,
    )
    .await
    .unwrap();
    drop(core);
    let core = open_core(root.path());
    let (_, pending) = PendingRegistrationStore::from_core(&core)
        .unwrap()
        .load("alice", "example.test")
        .unwrap()
        .unwrap();
    assert_eq!(pending.identity, original);
    let result = commit_pending_registration_async(
        &core,
        &pending,
        crate::identity::RegistrationMethod::Phone,
    )
    .await
    .unwrap();
    assert!(result.sdk_result.identity.is_some());
    let input = registration_save_input(&pending, pending.remote_result.as_ref().unwrap()).unwrap();
    assert_eq!(input.did_document, Some(current.document.clone()));
    assert_eq!(
        input.device_state.unwrap().checkpoint,
        Some(current.checkpoint.clone())
    );
    let mut rollback = current;
    rollback.checkpoint.document_version = 2;
    assert!(
        crate::internal::identity_custody::adopt_registered_web_document_async(
            &core, &pending, &rollback
        )
        .await
        .is_err()
    );
}

#[test]
fn web_registration_result_requires_exact_operation_and_bootstrap_facts() {
    use crate::internal::identity_wire::web_registration_result as result;
    let root = tempfile::tempdir().unwrap();
    let core = open_core(root.path());
    let mut pending = pending(&core);
    pending.registration_request_hash = Some("a".repeat(64));
    let raw = json!({"registration_result": {"state": "committed", "result": {
        "user_id": "user-1", "did": pending.identity.did.as_str(),
        "full_handle": "alice.example.test", "binding_generation": "1",
        "operation_id": pending.registration_operation_id, "request_hash": pending.registration_request_hash,
        "document_hash": pending.document_hash,
        "bootstrap_device": {"device_id": pending.identity.protocol_device_id.as_str(),
            "signing_key_id": pending.identity.device_signing_key_id, "e2ee_key_id": pending.identity.device_e2ee_key_id}
    }}});
    assert_eq!(
        result::take_result(&pending, &mut raw.clone(), "user-1").unwrap(),
        "1"
    );
    for pointer in [
        "/user_id",
        "/did",
        "/full_handle",
        "/operation_id",
        "/request_hash",
        "/document_hash",
        "/bootstrap_device/device_id",
        "/bootstrap_device/signing_key_id",
        "/bootstrap_device/e2ee_key_id",
    ] {
        let mut changed = raw.clone();
        *changed["registration_result"]["result"]
            .pointer_mut(pointer)
            .unwrap() = json!("wrong");
        assert!(
            result::take_result(&pending, &mut changed, "user-1").is_err(),
            "{pointer}"
        );
    }
    for mut invalid in [
        json!({}),
        json!({"registration_result":{"state":"absent","result":null}}),
        json!({"registration_result":{"state":"committed","result":null}}),
    ] {
        assert!(result::take_result(&pending, &mut invalid, "user-1").is_err());
    }
    let mut unknown = raw;
    unknown["registration_result"]["result"]["access_token"] = json!("unexpected");
    assert!(result::take_result(&pending, &mut unknown, "user-1").is_err());
}

#[test]
fn web_registration_current_observation_rejects_removed_or_replaced_bootstrap() {
    use crate::internal::identity_wire::web_registration_result::CurrentRegistrationDocument;
    let root = tempfile::tempdir().unwrap();
    let core = open_core(root.path());
    let pending = pending(&core);
    let current = CurrentRegistrationDocument {
        document: pending.identity.did_document.clone(),
        checkpoint: crate::internal::identity_device_state::IdentityInternalCheckpoint {
            document_version: 1,
            registry_version: 1,
            document_hash: pending.document_hash.clone(),
        },
    };
    current.validate(&pending).unwrap();
    for field in [
        "authentication",
        "assertionMethod",
        "keyAgreement",
        "verificationMethod",
    ] {
        let mut changed = current.clone();
        changed.document[field] = json!([]);
        changed.checkpoint.document_hash =
            crate::internal::identity_wire::document::document_hash(&changed.document).unwrap();
        assert!(changed.validate(&pending).is_err(), "{field}");
    }
    let mut changed = current.clone();
    let other_root = tempfile::tempdir().unwrap();
    let other = self::pending(&open_core(other_root.path()));
    changed.document["verificationMethod"][0]["publicKeyMultibase"] =
        other.identity.did_document["verificationMethod"][0]["publicKeyMultibase"].clone();
    changed.checkpoint.document_hash =
        crate::internal::identity_wire::document::document_hash(&changed.document).unwrap();
    assert!(changed.validate(&pending).is_err());
    let mut stale = current;
    stale.checkpoint.document_hash = format!("sha256:{}", "A".repeat(43));
    assert!(stale.validate(&pending).is_err());
}

#[test]
fn web_bootstrap_matches_frozen_cross_language_bytes() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../../../testdata/did_web_registration_v1.json"
    ))
    .unwrap();
    let business = &fixture["business_projection"];
    let params = json!({"did_document": fixture["did_document"], "registration_operation_id": business["registration_operation_id"],
        "phone": business["account"]["phone"], "email": business["account"]["email"], "name": business["profile"]["name"]});
    let projection = wire::business_projection(
        &params,
        business["audience"].as_str().unwrap(),
        business["full_handle"].as_str().unwrap(),
    )
    .unwrap();
    assert_eq!(projection, *business);
    assert_eq!(
        serde_json_canonicalizer::to_string(&projection).unwrap(),
        fixture["canonical_business_utf8"]
    );
    assert_eq!(
        wire::request_hash(&projection).unwrap(),
        fixture["request_hash"]
    );
    let proof = serde_json::from_value(fixture["bootstrap_proof"].clone()).unwrap();
    let bytes = wire::signing_input(
        business["registration_operation_id"].as_str().unwrap(),
        fixture["request_hash"].as_str().unwrap(),
        business["audience"].as_str().unwrap(),
        &proof,
    )
    .unwrap();
    assert_eq!(
        String::from_utf8(bytes.clone()).unwrap(),
        fixture["canonical_signing_utf8"]
    );
    let method =
        anp::authentication::find_verification_method(&fixture["did_document"], &proof.key_id)
            .unwrap();
    let public =
        crate::internal::identity_wire::document::extract_identity_public_key(&method).unwrap();
    public
        .verify_message(&bytes, &URL_SAFE_NO_PAD.decode(&proof.signature).unwrap())
        .unwrap();
    let mut changed = projection;
    changed["audience"] = json!("another-service");
    assert_ne!(
        wire::request_hash(&changed).unwrap(),
        fixture["request_hash"]
    );
}

#[tokio::test]
async fn web_candidate_and_operation_survive_real_vault_and_sqlite_reopen() {
    let root = tempfile::tempdir().unwrap();
    let core = open_core(root.path());
    let mut pending = pending(&core);
    let original_identity = pending.identity.clone();
    assert!(pending.identity.root_key_id.is_none());
    assert!(pending.identity.did_document.get("proof").is_none());
    assert_eq!(
        pending.identity.did_document["verificationMethod"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(
        pending.identity.did_document["deviceManifest"]["devices"][0]["profiles"]
            .as_array()
            .unwrap()
            .contains(&json!("anp.group.base.v2"))
    );
    let call = register_call(&pending, &request(), None).unwrap();
    let first = sign_web_register_call_async(&core, &mut pending, call)
        .await
        .unwrap();
    pending.remote_attempted = true;
    let store = PendingRegistrationStore::from_core(&core).unwrap();
    store.save(&pending).unwrap();
    let operation_id = pending.registration_operation_id.clone();
    let request_hash = pending.registration_request_hash.clone();
    drop(store);
    drop(core);

    let core = open_core(root.path());
    let store = PendingRegistrationStore::from_core(&core).unwrap();
    let (_, mut restored) = store.load("alice", "example.test").unwrap().unwrap();
    assert_eq!(restored.identity, original_identity);
    assert_eq!(restored.registration_operation_id, operation_id);
    assert_eq!(restored.registration_request_hash, request_hash);
    let mut fresh_request = request();
    fresh_request.verification = crate::identity::VerificationInput::Phone {
        phone: "+15555550123".into(),
        otp: Some("654321".into()),
    };
    let call = register_call(&restored, &fresh_request, None).unwrap();
    let replay = sign_web_register_call_async(&core, &mut restored, call)
        .await
        .unwrap();
    assert_ne!(
        first.params["bootstrap_proof"]["nonce"],
        replay.params["bootstrap_proof"]["nonce"]
    );
    assert_eq!(restored.registration_request_hash, request_hash);
    assert_eq!(restored.identity, original_identity);
    assert!(root.path().join("local/im.sqlite").is_file());
    let store = crate::internal::identity_custody::open_controller_manager(&core).unwrap();
    assert_eq!(store.list().unwrap().len(), 1);
    assert!(core.identities().default_identity().unwrap().is_none());
}

#[test]
fn web_pending_rejects_method_schema_root_and_operation_substitution() {
    let root = tempfile::tempdir().unwrap();
    let core = open_core(root.path());
    let pending = pending(&core);
    let original = serde_json::to_value(&pending).unwrap();
    for (field, value) in [
        ("schema_version", json!(2)),
        ("schema_version", json!(99)),
        ("did_method", json!("wba")),
        ("registration_operation_id", json!("not-a-uuid")),
    ] {
        let mut changed = original.clone();
        changed[field] = value;
        let parsed: PendingRegistration = serde_json::from_value(changed).unwrap();
        assert!(parsed.validate().is_err(), "{field}");
    }
    let mut changed = pending;
    changed.identity.root_key_id = Some(format!("{}#key-1", changed.identity.did.as_str()));
    assert!(changed.validate().is_err());
}

#[test]
fn schema_two_wba_pending_keeps_original_serialized_root_and_keys() {
    let root = tempfile::tempdir().unwrap();
    let core = open_core(root.path());
    let identity = crate::internal::identity_custody::provision_registration_identity(
        &core,
        "example.test",
        "old",
    )
    .unwrap();
    let original = identity.clone();
    let pending = PendingRegistration::new(
        "old".into(),
        "example.test".into(),
        "old".into(),
        "Old".into(),
        false,
        "phone".into(),
        Some("+15555550123".into()),
        None,
        identity,
    )
    .unwrap();
    let mut stored = serde_json::to_value(pending).unwrap();
    stored["schema_version"] = json!(2);
    stored.as_object_mut().unwrap().remove("did_method");
    assert!(stored["identity"]["root_key_id"].is_string());
    let restored: PendingRegistration = serde_json::from_value(stored).unwrap();
    restored.validate().unwrap();
    assert_eq!(restored.identity, original);
    assert_eq!(restored.did_method, DidMethod::Wba);
}

#[tokio::test]
async fn web_retry_binds_business_profile_and_does_not_change_candidate() {
    let root = tempfile::tempdir().unwrap();
    let core = open_core(root.path());
    let mut pending = pending(&core);
    let call = register_call(&pending, &request(), None).unwrap();
    sign_web_register_call_async(&core, &mut pending, call)
        .await
        .unwrap();
    let before = pending.clone();
    let mut changed = request();
    changed.profile.avatar_url = Some("https://example.test/changed.png".into());
    let call = register_call(&pending, &changed, None).unwrap();
    assert!(sign_web_register_call_async(&core, &mut pending, call)
        .await
        .is_err());
    assert_eq!(pending, before);
    assert!(register_call(&pending, &request(), Some("guest-operation")).is_err());
}
