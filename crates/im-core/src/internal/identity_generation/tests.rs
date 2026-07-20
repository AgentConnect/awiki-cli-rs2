use super::build_agent_anp_message_service;
use anp::proof::{verify_w3c_proof, ProofVerificationOptions};

#[test]
fn agent_message_service_advertises_group_profile() {
    let service = build_agent_anp_message_service(
        "https://community.example/anp-im/rpc",
        "did:wba:community.example",
    )
    .expect("message service entry should build");

    assert_eq!(service["type"], "ANPMessageService");
    assert_eq!(
        service["serviceEndpoint"],
        "https://community.example/anp-im/rpc"
    );
    assert_eq!(service["serviceDid"], "did:wba:community.example");
    assert_eq!(
        service["profiles"],
        serde_json::json!([
            "anp.core.binding.v1",
            "anp.direct.base.v1",
            "anp.group.base.v1",
            "anp.attachment.v1"
        ])
    );
}

#[test]
fn vnext_genesis_has_one_device_and_separate_root_and_device_keys() {
    let generated = super::generate_vnext_handle_identity_with_default_daemon_subkey(
        "awiki.info",
        "alice",
        None,
        None,
    )
    .expect("vNext genesis should build");

    let manifest = anp::authentication::validate_device_manifest(&generated.did_document)
        .expect("Manifest should validate")
        .expect("Manifest should exist");
    assert_eq!(manifest.devices.len(), 1);
    assert_eq!(
        manifest.devices[0].device_id,
        generated.protocol_device_id.as_str()
    );
    assert_eq!(
        manifest.devices[0].signing_key_id,
        generated.device_signing_key_id
    );
    assert_eq!(
        manifest.devices[0].e2ee_key_id,
        generated.device_e2ee_key_id
    );
    assert_ne!(generated.root_key_id, generated.device_signing_key_id);
    assert_ne!(
        generated.root_private_pem,
        generated.device_signing_private_pem
    );
    assert!(generated
        .did
        .as_str()
        .starts_with("did:wba:awiki.info:user:alice:e1_"));
    assert!(anp::authentication::validate_did_document_binding(
        &generated.did_document,
        true
    ));

    let proof = generated.did_document["proof"]
        .as_object()
        .expect("root proof should exist");
    let authority = generated
        .did
        .as_str()
        .strip_prefix("did:wba:")
        .and_then(|value| value.split(':').next())
        .expect("generated DID should have an authority");
    let challenge = proof["challenge"]
        .as_str()
        .filter(|value| !value.is_empty())
        .expect("root proof challenge should be non-empty");
    assert_eq!(proof["domain"].as_str(), Some(authority));
    assert_eq!(proof["type"], "DataIntegrityProof");
    assert_eq!(proof["cryptosuite"], "eddsa-jcs-2022");
    assert_eq!(proof["proofPurpose"], "assertionMethod");

    let root_private_key = anp::PrivateKeyMaterial::from_pem(&generated.root_private_pem).unwrap();
    assert!(verify_w3c_proof(
        &generated.did_document,
        &root_private_key.public_key(),
        ProofVerificationOptions {
            expected_purpose: Some("assertionMethod".to_owned()),
            expected_domain: Some(authority.to_owned()),
            expected_challenge: Some(challenge.to_owned()),
        },
    ));

    let service = generated.did_document["service"]
        .as_array()
        .and_then(|services| services.first())
        .expect("message service should exist");
    assert!(service["profiles"]
        .as_array()
        .expect("profiles should be an array")
        .iter()
        .any(|profile| profile == anp::authentication::PROFILE_DIRECT_E2EE_V2));
    assert_eq!(generated.daemon_subkey_package.user_did, generated.did);
}

#[test]
fn vnext_generated_identity_debug_redacts_private_material() {
    let generated = super::generate_vnext_handle_identity_with_default_daemon_subkey(
        "awiki.info",
        "alice",
        None,
        None,
    )
    .expect("vNext genesis should build");
    let debug = format!("{generated:?}");

    assert!(debug.contains("<redacted-private-key>"));
    assert!(!debug.contains(generated.root_private_pem.trim()));
    assert!(!debug.contains(generated.device_signing_private_pem.trim()));
    assert!(!debug.contains(generated.device_e2ee_private_pem.trim()));
}
