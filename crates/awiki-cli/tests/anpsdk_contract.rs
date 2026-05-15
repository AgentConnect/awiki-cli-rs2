use awiki_cli::anpsdk;
use serde_json::json;

#[test]
fn anpsdk_facade_exposes_go_registry_authentication_symbols() {
    assert_eq!(
        anpsdk::MODULE_PATH,
        "github.com/agent-network-protocol/anp/golang"
    );
    assert_eq!(anpsdk::MODULE_VERSION, "v0.8.7");
    assert_eq!(
        anpsdk::AUTH_MODE_HTTP_SIGNATURES,
        anpsdk::AuthMode::HttpSignatures
    );
    assert_eq!(anpsdk::AUTH_MODE_AUTO, anpsdk::AuthMode::Auto);
    assert_eq!(anpsdk::DID_PROFILE_E1, anpsdk::DidProfile::E1);
    assert_eq!(anpsdk::DID_PROFILE_K1, anpsdk::DidProfile::K1);

    let service = anpsdk::build_anp_message_service(
        "did:wba:example.com:user:alice:e1_alice",
        "https://example.com/anp-im/rpc",
        anpsdk::AnpMessageServiceOptions::default()
            .with_profiles(["anp.core.binding.v1"])
            .with_security_profiles(["transport-protected"]),
    );
    assert_eq!(service["type"], "ANPMessageService");
    assert_eq!(service["serviceEndpoint"], "https://example.com/anp-im/rpc");

    let bundle =
        anpsdk::create_did_wba_document("example.com", anpsdk::DidDocumentOptions::default())
            .expect("create DID document through facade");
    assert!(bundle.did().unwrap_or_default().starts_with("did:wba:"));
    let private_key = anpsdk::PrivateKeyMaterial::from_pem(
        bundle.private_key_pem("key-1").expect("key-1 private key"),
    )
    .expect("private key through facade");
    let public_key = private_key.public_key();
    assert_eq!(public_key.to_string(), "ed25519");
    let _verifier = anpsdk::DidWbaVerifier::new(anpsdk::DidWbaVerifierConfig::default());
    let _auth = anpsdk::DIDWbaAuthHeader::new("", "", anpsdk::AUTH_MODE_HTTP_SIGNATURES);
}

#[test]
fn anpsdk_facade_exposes_go_registry_proof_symbols() {
    let digest = anpsdk::build_im_content_digest(br#"{"hello":"world"}"#);
    assert!(digest.starts_with("sha-256=:"));

    let signature_input = anpsdk::build_im_signature_input(
        "did:wba:example.com:user:alice:e1_alice#key-1",
        anpsdk::ImProofGenerationOptions {
            label: "sig1".to_string(),
            components: vec![
                "@method".to_string(),
                "@target-uri".to_string(),
                "content-digest".to_string(),
            ],
            created: Some(1_712_000_000),
            expires: Some(1_712_000_300),
            nonce: Some("nonce-1".to_string()),
        },
    )
    .expect("build signature input");
    let parsed = anpsdk::parse_im_signature_input(&signature_input).expect("parse input");
    assert_eq!(parsed.label, "sig1");
    assert_eq!(
        parsed.keyid,
        "did:wba:example.com:user:alice:e1_alice#key-1"
    );

    assert_eq!(
        anpsdk::build_logical_target_uri(
            anpsdk::TARGET_KIND_AGENT,
            "did:wba:example.com:user:bob:e1_bob",
        )
        .expect("target uri"),
        "anp://agent/did%3Awba%3Aexample.com%3Auser%3Abob%3Ae1_bob"
    );

    let signed = anpsdk::build_signed_request_object(
        "message.send",
        &json!({
            "target": {
                "kind": "agent",
                "did": "did:wba:example.com:user:bob:e1_bob"
            }
        }),
        &json!({"content": "hello"}),
    )
    .expect("signed request object");
    let canonical = anpsdk::canonicalize_signed_request_object(&signed).expect("canonical request");
    assert!(!canonical.is_empty());

    let base = anpsdk::build_rfc9421_origin_signature_base(
        "message.send",
        "anp://agent/did%3Awba%3Aexample.com%3Auser%3Abob%3Ae1_bob",
        &digest,
        &signature_input,
    )
    .expect("signature base");
    assert!(!base.is_empty());

    assert_eq!(anpsdk::TARGET_KIND_AGENT, anpsdk::TargetKind::Agent);
    assert_eq!(anpsdk::TARGET_KIND_GROUP, anpsdk::TargetKind::Group);
    assert_eq!(anpsdk::TARGET_KIND_SERVICE, anpsdk::TargetKind::Service);
}

#[test]
fn anpsdk_facade_exposes_go_registry_direct_e2ee_state_symbols() {
    let prekey = anpsdk::OneTimePrekey {
        key_id: "opk-1".to_string(),
        public_key_b64u: "public".to_string(),
    };
    assert_eq!(prekey.key_id, "opk-1");

    let bundle = anpsdk::PrekeyBundle {
        bundle_id: "bundle-1".to_string(),
        owner_did: "did:wba:example.com:user:bob:e1_bob".to_string(),
        suite: "ANP-DIRECT-E2EE-X3DH-25519-CHACHA20POLY1305-SHA256-V1".to_string(),
        static_key_agreement_id: "did:wba:example.com:user:bob:e1_bob#key-3".to_string(),
        signed_prekey: anpsdk::SignedPrekey {
            key_id: "spk-1".to_string(),
            public_key_b64u: "signed".to_string(),
            expires_at: "2026-05-15T00:00:00Z".to_string(),
        },
        proof: json!({"type": "DataIntegrityProof"}),
    };
    assert_eq!(bundle.signed_prekey.key_id, "spk-1");

    let session = anpsdk::DirectSessionState {
        session_id: "session-1".to_string(),
        suite: "ANP-DIRECT-E2EE-X3DH-25519-CHACHA20POLY1305-SHA256-V1".to_string(),
        peer_did: "did:wba:example.com:user:bob:e1_bob".to_string(),
        local_key_agreement_id: "did:wba:example.com:user:alice:e1_alice#key-3".to_string(),
        peer_key_agreement_id: "did:wba:example.com:user:bob:e1_bob#key-3".to_string(),
        root_key_b64u: "root".to_string(),
        send_chain_key_b64u: Some("send".to_string()),
        recv_chain_key_b64u: Some("recv".to_string()),
        ratchet_private_key_b64u: "private".to_string(),
        ratchet_public_key_b64u: "public".to_string(),
        peer_ratchet_public_key_b64u: Some("peer-public".to_string()),
        send_n: 1,
        recv_n: 2,
        previous_send_chain_length: 0,
        is_initiator: true,
        status: "pending".to_string(),
        skipped_message_keys: Vec::new(),
    };
    assert_eq!(session.session_id, "session-1");
}
