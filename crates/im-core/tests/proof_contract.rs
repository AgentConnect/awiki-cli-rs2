use anp::authentication::{create_did_wba_document, DidDocumentOptions};
use anp::proof::{
    build_im_content_digest, build_signed_request_object, canonicalize_signed_request_object,
    verify_rfc9421_origin_proof, Rfc9421OriginProofVerificationOptions,
};
use im_core::compat;
use serde_json::json;

#[test]
fn origin_proof_matches_rfc9421_contract_and_uses_auth_verification_method() {
    let generated = generated_identity();
    let payload = compat::wire::build_direct_text_payload(
        &generated.did,
        "did:wba:awiki.ai:user:bob:e1_bob",
        "hello",
        "text/plain",
    )
    .expect("direct payload");

    let key_id = compat::proof::verification_method_id_from_document(&generated.did_document)
        .expect("verification method id");
    assert_eq!(
        key_id,
        generated.did_document["authentication"][0]
            .as_str()
            .expect("authentication method")
    );

    let identity = compat::proof::OriginProofIdentity {
        identity_name: "alice".to_string(),
        did_document: Some(generated.did_document.clone()),
        key1_private_pem: generated.key1_private_pem.clone(),
    };
    let origin_proof =
        compat::proof::build_origin_proof(&identity, &payload).expect("origin proof should build");
    let auth = compat::proof::origin_auth_value(&origin_proof);
    assert_eq!(auth["scheme"], compat::proof::ORIGIN_PROOF_SCHEME);
    assert!(auth["origin_proof"]["contentDigest"]
        .as_str()
        .expect("content digest")
        .starts_with("sha-256=:"));
    assert!(auth["origin_proof"]["signatureInput"]
        .as_str()
        .expect("signature input")
        .contains("\"@method\""));
    assert!(auth["origin_proof"]["signatureInput"]
        .as_str()
        .expect("signature input")
        .contains("\"@target-uri\""));
    assert!(auth["origin_proof"]["signatureInput"]
        .as_str()
        .expect("signature input")
        .contains("\"content-digest\""));
    assert!(auth["origin_proof"]["signature"]
        .as_str()
        .expect("signature")
        .starts_with("sig1=:"));

    let signed_request = build_signed_request_object(&payload.method, &payload.meta, &payload.body)
        .expect("signed request");
    let canonical = canonicalize_signed_request_object(&signed_request).expect("canonical");
    assert_eq!(
        origin_proof.content_digest,
        build_im_content_digest(&canonical)
    );
    verify_rfc9421_origin_proof(
        &origin_proof,
        &payload.method,
        &payload.meta,
        &payload.body,
        Rfc9421OriginProofVerificationOptions {
            did_document: Some(generated.did_document),
            expected_signer_did: Some(generated.did),
            ..Rfc9421OriginProofVerificationOptions::default()
        },
    )
    .expect("origin proof verifies against did document");
}

#[test]
fn origin_proof_reports_missing_verification_method_like_go_contract() {
    let generated = generated_identity();
    let payload = compat::wire::build_direct_text_payload(
        &generated.did,
        "did:wba:awiki.ai:user:bob:e1_bob",
        "hello",
        "text/plain",
    )
    .expect("direct payload");

    let broken = compat::proof::OriginProofIdentity {
        identity_name: "broken".to_string(),
        did_document: Some(json!({ "id": generated.did })),
        key1_private_pem: generated.key1_private_pem.clone(),
    };
    let error = compat::proof::build_origin_proof(&broken, &payload).unwrap_err();
    assert_eq!(
        error.to_string(),
        "serialization error: identity broken is missing an authentication verification method"
    );

    let fallback = json!({
        "verificationMethod": [{
            "id": "did:wba:awiki.ai:user:alice:e1#key-1"
        }]
    });
    assert_eq!(
        compat::proof::verification_method_id_from_document(&fallback).as_deref(),
        Some("did:wba:awiki.ai:user:alice:e1#key-1")
    );

    let empty_auth_takes_precedence = json!({
        "authentication": [""],
        "verificationMethod": [{
            "id": "did:wba:awiki.ai:user:alice:e1#key-1"
        }]
    });
    assert_eq!(
        compat::proof::verification_method_id_from_document(&empty_auth_takes_precedence)
            .as_deref(),
        Some("")
    );
    let empty_auth = compat::proof::OriginProofIdentity {
        identity_name: "empty-auth".to_string(),
        did_document: Some(empty_auth_takes_precedence),
        key1_private_pem: generated.key1_private_pem,
    };
    let error = compat::proof::build_origin_proof(&empty_auth, &payload).unwrap_err();
    assert_eq!(
        error.to_string(),
        "serialization error: identity empty-auth is missing an authentication verification method"
    );
}

struct GeneratedIdentity {
    did: String,
    did_document: serde_json::Value,
    key1_private_pem: String,
}

fn generated_identity() -> GeneratedIdentity {
    let bundle = create_did_wba_document(
        "awiki.ai",
        DidDocumentOptions {
            path_segments: vec!["user".to_string()],
            domain: Some("awiki.ai".to_string()),
            challenge: Some("contract-test".to_string()),
            ..DidDocumentOptions::default()
        },
    )
    .expect("generated did document");
    let did = bundle.did().expect("generated did").to_string();
    let key1_private_pem = bundle
        .private_key_pem("key-1")
        .expect("key-1 private pem")
        .to_string();
    GeneratedIdentity {
        did,
        did_document: bundle.did_document,
        key1_private_pem,
    }
}
