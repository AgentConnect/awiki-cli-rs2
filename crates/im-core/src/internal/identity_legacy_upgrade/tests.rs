use super::*;
use anp::proof::{verify_w3c_proof, ProofVerificationOptions};

const PUBLISHED_APP_REF: &str = "c19a01a5e434ac41ead73915ef7fcbc2a27e3a5a";
const PUBLISHED_CORE_REF: &str = "d7c853a986a29e0c0457284a6b2c3d81ec637e10";

fn published_e1_legacy_identity(
) -> crate::internal::identity_generation::GeneratedIdentityWithDaemonSubkey {
    // The published 0.1.5+14 Core generated this exact E1 + key-2/key-3 +
    // daemon-key-1 structure. Keep the source refs beside the fixture builder
    // so future compatibility work cannot silently broaden the release boundary.
    assert!(!PUBLISHED_APP_REF.is_empty());
    assert!(!PUBLISHED_CORE_REF.is_empty());
    crate::internal::identity_generation::generate_handle_identity_with_default_daemon_subkey(
        "example.test",
        "published-user",
        None,
        None,
    )
    .unwrap()
}

#[test]
fn published_e1_upgrade_builds_exact_canonical_vnext_document() {
    let generated = published_e1_legacy_identity();
    let mut legacy = generated.identity;
    legacy.did_document["x-awiki-test-extension"] = json!({"preserved": true});
    let source_service = legacy.did_document.get("service").cloned();
    let root_key_id = format!("{}#key-1", legacy.did.as_str());
    let daemon_key_id = format!("{}#daemon-key-1", legacy.did.as_str());
    let source_root = unique_verification_method(&legacy.did_document, &root_key_id)
        .unwrap()
        .clone();
    let source_daemon = unique_verification_method(&legacy.did_document, &daemon_key_id)
        .unwrap()
        .clone();
    let old_challenge = legacy.did_document["proof"]["challenge"]
        .as_str()
        .unwrap()
        .to_owned();

    let upgrade = build_legacy_upgrade(&legacy.did_document, &legacy.key1_private_pem).unwrap();

    assert_eq!(upgrade.did, legacy.did);
    assert_eq!(
        upgrade.target_document.get("service").cloned(),
        source_service
    );
    assert_eq!(
        upgrade.target_document["x-awiki-test-extension"],
        json!({"preserved": true})
    );
    assert_eq!(
        unique_verification_method(&upgrade.target_document, &root_key_id).unwrap(),
        &source_root
    );
    assert_eq!(
        unique_verification_method(&upgrade.target_document, &daemon_key_id).unwrap(),
        &source_daemon
    );

    let method_ids = upgrade.target_document["verificationMethod"]
        .as_array()
        .unwrap()
        .iter()
        .map(|method| method["id"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(method_ids.len(), 4);
    assert!(method_ids.contains(&root_key_id.as_str()));
    assert!(method_ids.contains(&upgrade.signing_key_id.as_str()));
    assert!(method_ids.contains(&upgrade.e2ee_key_id.as_str()));
    assert!(method_ids.contains(&daemon_key_id.as_str()));
    assert!(!method_ids.contains(&format!("{}#key-2", legacy.did.as_str()).as_str()));
    assert!(!method_ids.contains(&format!("{}#key-3", legacy.did.as_str()).as_str()));

    assert_eq!(
        upgrade.target_document["authentication"],
        json!([upgrade.signing_key_id, daemon_key_id])
    );
    assert_eq!(
        upgrade.target_document["assertionMethod"],
        json!([root_key_id, upgrade.signing_key_id])
    );
    assert_eq!(
        upgrade.target_document["keyAgreement"],
        json!([upgrade.e2ee_key_id])
    );
    let manifest = anp::authentication::validate_device_manifest(&upgrade.target_document)
        .unwrap()
        .unwrap();
    assert_eq!(manifest.devices.len(), 1);
    assert_eq!(
        manifest.devices[0].device_id,
        upgrade.protocol_device_id.as_str()
    );
    assert_eq!(
        manifest.devices[0].profiles,
        [
            "anp.core.binding.v1",
            "anp.identity.discovery.v1",
            "anp.direct.base.v1",
            "anp.direct.e2ee.v2",
            "anp.group.base.v1",
            "anp.group.e2ee.v2",
        ]
    );

    let proof = &upgrade.target_document["proof"];
    assert_eq!(proof["proofPurpose"], "assertionMethod");
    assert_eq!(proof["domain"], "example.test");
    assert_ne!(proof["challenge"], old_challenge);
    let root_public = crate::internal::identity_wire::document::extract_identity_public_key(
        unique_verification_method(&upgrade.target_document, &root_key_id).unwrap(),
    )
    .unwrap();
    assert!(verify_w3c_proof(
        &upgrade.target_document,
        &root_public,
        ProofVerificationOptions {
            expected_purpose: Some("assertionMethod".to_owned()),
            expected_domain: Some("example.test".to_owned()),
            expected_challenge: proof["challenge"].as_str().map(ToOwned::to_owned),
        },
    ));
}

#[test]
fn upgrade_rejects_a_root_private_key_that_does_not_match_the_document() {
    let generated = published_e1_legacy_identity();
    let other =
        crate::internal::identity_generation::generate_handle_identity_with_default_daemon_subkey(
            "example.test",
            "other-user",
            None,
            None,
        )
        .unwrap();

    assert!(matches!(
        build_legacy_upgrade(
            &generated.identity.did_document,
            &other.identity.key1_private_pem,
        ),
        Err(crate::ImError::PermissionDenied)
    ));
}

#[test]
fn upgrade_rejects_malformed_daemon_delegation_instead_of_dropping_authority() {
    let generated = published_e1_legacy_identity();
    let mut document = generated.identity.did_document.clone();
    let daemon_key_id = format!("{}#daemon-key-1", generated.identity.did.as_str());
    document["assertionMethod"]
        .as_array_mut()
        .unwrap()
        .push(Value::String(daemon_key_id));

    assert!(matches!(
        build_legacy_upgrade(&document, &generated.identity.key1_private_pem),
        Err(crate::ImError::PermissionDenied)
    ));
}

#[test]
fn k1_jwk_root_is_compared_semantically_and_gets_the_matching_fresh_proof_suite() {
    let bundle = anp::authentication::create_did_wba_document(
        "example.test",
        anp::authentication::DidDocumentOptions {
            path_segments: vec!["user".to_owned(), "k1-user".to_owned()],
            domain: Some("example.test".to_owned()),
            challenge: Some("published-k1-challenge".to_owned()),
            did_profile: anp::authentication::DidProfile::K1,
            ..anp::authentication::DidDocumentOptions::default()
        },
    )
    .unwrap();
    let root_private_pem = bundle
        .private_key_pem(anp::authentication::VM_KEY_AUTH)
        .unwrap();

    let upgrade = build_legacy_upgrade(&bundle.did_document, root_private_pem).unwrap();

    assert_eq!(
        upgrade.target_document["proof"]["cryptosuite"],
        anp::proof::CRYPTOSUITE_DIDWBA_SECP256K1_2025
    );
    let did = upgrade.did.as_str();
    let root_key_id = format!("{did}#key-1");
    assert!(upgrade.target_document["verificationMethod"]
        .as_array()
        .unwrap()
        .iter()
        .any(|method| method["id"] == root_key_id && method.get("publicKeyJwk").is_some()));
}

#[test]
fn proven_legacy_retry_rebuilds_only_the_document_and_reuses_device_keys() {
    let generated = published_e1_legacy_identity();
    let legacy = generated.identity;
    let mut upgrade = build_legacy_upgrade(&legacy.did_document, &legacy.key1_private_pem).unwrap();
    let original_device_id = upgrade.protocol_device_id.clone();
    let original_signing_key_id = upgrade.signing_key_id.clone();
    let original_signing_private = upgrade.signing_private_pem.clone();
    let original_e2ee_key_id = upgrade.e2ee_key_id.clone();
    let original_e2ee_private = upgrade.e2ee_private_pem.clone();
    let original_hash = upgrade.target_document_hash.clone();
    let original_challenge = upgrade.target_document["proof"]["challenge"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut current_remote_legacy = legacy.did_document.clone();
    current_remote_legacy["x-awiki-server-extension"] = json!({"revision": 2});

    rebuild_legacy_upgrade_target(
        &mut upgrade,
        &current_remote_legacy,
        &legacy.key1_private_pem,
    )
    .unwrap();

    assert_eq!(upgrade.protocol_device_id, original_device_id);
    assert_eq!(upgrade.signing_key_id, original_signing_key_id);
    assert_eq!(upgrade.signing_private_pem, original_signing_private);
    assert_eq!(upgrade.e2ee_key_id, original_e2ee_key_id);
    assert_eq!(upgrade.e2ee_private_pem, original_e2ee_private);
    assert_ne!(upgrade.target_document_hash, original_hash);
    assert_ne!(
        upgrade.target_document["proof"]["challenge"],
        original_challenge
    );
    assert_eq!(
        upgrade.target_document["x-awiki-server-extension"],
        json!({"revision": 2})
    );
}
