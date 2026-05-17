use awiki_cli::anpsdk;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn anpsdk_facade_exposes_go_registry_authentication_symbols() {
    assert_eq!(
        anpsdk::MODULE_PATH,
        "github.com/agent-network-protocol/anp/golang"
    );
    assert_eq!(anpsdk::MODULE_VERSION, "v0.8.7");
    assert_eq!(
        anpsdk::ModulePath,
        "github.com/agent-network-protocol/anp/golang"
    );
    assert_eq!(anpsdk::ModuleVersion, "v0.8.7");
    assert_eq!(
        anpsdk::AUTH_MODE_HTTP_SIGNATURES,
        anpsdk::AuthMode::HttpSignatures
    );
    assert_eq!(anpsdk::AUTH_MODE_AUTO, anpsdk::AuthMode::Auto);
    assert_eq!(anpsdk::DID_PROFILE_E1, anpsdk::DidProfile::E1);
    assert_eq!(anpsdk::DID_PROFILE_K1, anpsdk::DidProfile::K1);
    assert_eq!(
        anpsdk::AuthModeHTTPSignatures,
        anpsdk::AuthMode::HttpSignatures
    );
    assert_eq!(anpsdk::AuthModeAuto, anpsdk::AuthMode::Auto);
    assert_eq!(anpsdk::DidProfileE1, anpsdk::DidProfile::E1);
    assert_eq!(anpsdk::DidProfileK1, anpsdk::DidProfile::K1);

    let service = anpsdk::BuildANPMessageService(
        "did:wba:example.com:user:alice:e1_alice",
        "https://example.com/anp-im/rpc",
        anpsdk::AnpMessageServiceOptions::default()
            .with_profiles(["anp.core.binding.v1"])
            .with_security_profiles(["transport-protected"]),
    );
    assert_eq!(service["type"], "ANPMessageService");
    assert_eq!(service["serviceEndpoint"], "https://example.com/anp-im/rpc");

    let bundle = anpsdk::CreateDidWBADocument("example.com", anpsdk::DidDocumentOptions::default())
        .expect("create DID document through facade");
    assert!(bundle.did().unwrap_or_default().starts_with("did:wba:"));
    let private_key = anpsdk::PrivateKeyMaterial::from_pem(
        bundle.private_key_pem("key-1").expect("key-1 private key"),
    )
    .expect("private key through facade");
    let public_key = private_key.public_key();
    assert_eq!(public_key.to_string(), "ed25519");
    let auth_header =
        anpsdk::GenerateAuthHeader(&bundle.did_document, "example.com", &private_key, "1.1")
            .expect("generate DID-WBA auth header through Go facade");
    assert!(auth_header.starts_with("DIDWba "));

    let http_signature_headers = anpsdk::GenerateHTTPSignatureHeaders(
        &bundle.did_document,
        "https://example.com/anp-im/rpc",
        "POST",
        &private_key,
        None,
        Some(br#"{"jsonrpc":"2.0"}"#),
        anpsdk::HttpSignatureOptions {
            keyid: None,
            covered_components: None,
            created: Some(1_712_000_000),
            expires: Some(1_712_000_300),
            nonce: Some("nonce-1".to_string()),
        },
    )
    .expect("generate HTTP signature headers through Go facade");
    assert!(http_signature_headers.contains_key("Signature"));
    assert!(http_signature_headers.contains_key("Signature-Input"));

    let _resolve_fn = anpsdk::ResolveDidDocument;
    let _resolve_with_options_fn = anpsdk::ResolveDidDocumentWithOptions;
    let _verifier = anpsdk::NewDidWbaVerifier(anpsdk::DidWbaVerifierConfig::default());
    let _auth = anpsdk::NewDIDWbaAuthHeader("", "", anpsdk::AuthModeHTTPSignatures);
}

#[test]
fn anpsdk_facade_exposes_go_key_material_helpers() {
    for key_type in [
        anpsdk::KEY_TYPE_ED25519,
        anpsdk::KEY_TYPE_SECP256K1,
        anpsdk::KEY_TYPE_SECP256R1,
        anpsdk::KEY_TYPE_X25519,
    ] {
        let (private_key, public_key, pair) =
            anpsdk::GenerateKeyPairPEM(key_type).expect("generate key pair through facade");
        assert_eq!(
            first_line(&pair.private_key_pem),
            "-----BEGIN PRIVATE KEY-----"
        );
        assert_eq!(
            first_line(&pair.public_key_pem),
            "-----BEGIN PUBLIC KEY-----"
        );
        assert!(
            !pair.private_key_pem.contains("ANP ") && !pair.public_key_pem.contains("ANP "),
            "generated PEM must not use legacy ANP labels"
        );

        let parsed_private =
            anpsdk::PrivateKeyFromPEM(&pair.private_key_pem).expect("parse private PEM");
        let parsed_public =
            anpsdk::PublicKeyFromPEM(&pair.public_key_pem).expect("parse public PEM");

        assert_eq!(key_type, private_key_type(&private_key));
        assert_eq!(key_type, public_key_type(&public_key));
        assert_eq!(key_type, private_key_type(&parsed_private));
        assert_eq!(key_type, public_key_type(&parsed_public));
        assert_eq!(parsed_private.to_pem(), pair.private_key_pem);
        assert_eq!(parsed_public.to_pem(), pair.public_key_pem);
    }
}

#[test]
fn anpsdk_facade_runtime_pem_parsers_reject_legacy_anp_labels() {
    let legacy_private = concat!(
        "-----BEGIN ANP ED25519 PRIVATE KEY-----\n",
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=\n",
        "-----END ANP ED25519 PRIVATE KEY-----\n"
    );
    let private_error =
        anpsdk::PrivateKeyFromPEM(legacy_private).expect_err("legacy private label rejected");
    assert_eq!(private_error, "Invalid PEM label: ANP ED25519 PRIVATE KEY");

    let legacy_public = concat!(
        "-----BEGIN ANP ED25519 PUBLIC KEY-----\n",
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=\n",
        "-----END ANP ED25519 PUBLIC KEY-----\n"
    );
    let public_error =
        anpsdk::PublicKeyFromPEM(legacy_public).expect_err("legacy public label rejected");
    assert_eq!(public_error, "Invalid PEM label: ANP ED25519 PUBLIC KEY");
}

#[test]
fn anpsdk_facade_exposes_go_registry_proof_symbols() {
    let digest = anpsdk::BuildIMContentDigest(br#"{"hello":"world"}"#);
    assert!(digest.starts_with("sha-256=:"));

    let signature_input = anpsdk::BuildIMSignatureInput(
        "did:wba:example.com:user:alice:e1_alice#key-1",
        anpsdk::IMGenerationOptions {
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
    let parsed: anpsdk::ParsedIMSignatureInput =
        anpsdk::ParseIMSignatureInput(&signature_input).expect("parse input");
    assert_eq!(parsed.label, "sig1");
    assert_eq!(
        parsed.keyid,
        "did:wba:example.com:user:alice:e1_alice#key-1"
    );

    assert_eq!(
        anpsdk::EncodeIMSignature(b"hello", "sig1"),
        "sig1=:aGVsbG8=:"
    );

    assert_eq!(
        anpsdk::BuildLogicalTargetURI(
            anpsdk::TargetKindAgent,
            "did:wba:example.com:user:bob:e1_bob",
        )
        .expect("target uri"),
        "anp://agent/did%3Awba%3Aexample.com%3Auser%3Abob%3Ae1_bob"
    );

    let signed: anpsdk::SignedRequestObject = anpsdk::BuildSignedRequestObject(
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
    let canonical = anpsdk::CanonicalizeSignedRequestObject(&signed).expect("canonical request");
    assert!(!canonical.is_empty());

    let base = anpsdk::BuildRFC9421OriginSignatureBase(
        "message.send",
        "anp://agent/did%3Awba%3Aexample.com%3Auser%3Abob%3Ae1_bob",
        &digest,
        &signature_input,
    )
    .expect("signature base");
    assert!(!base.is_empty());

    let bundle = anpsdk::CreateDidWBADocument("example.com", anpsdk::DidDocumentOptions::default())
        .expect("create proof DID document");
    let private_key = bundle.load_private_key("key-1").expect("load proof key");
    let keyid = format!("{}#key-1", bundle.did().expect("bundle DID"));

    let im_proof: anpsdk::IMProof = anpsdk::GenerateIMProof(
        &canonical,
        &base,
        &private_key,
        &keyid,
        anpsdk::IMGenerationOptions {
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
    .expect("generate IM proof through Go facade");
    anpsdk::VerifyIMProofWithDocument(
        &im_proof,
        &canonical,
        &base,
        &bundle.did_document,
        bundle.did(),
    )
    .expect("verify IM proof through Go facade");

    let origin_proof: anpsdk::RFC9421OriginProof = anpsdk::GenerateRFC9421OriginProof(
        "message.send",
        &json!({
            "target": {
                "kind": "agent",
                "did": "did:wba:example.com:user:bob:e1_bob"
            }
        }),
        &json!({"content": "hello"}),
        &private_key,
        &keyid,
        anpsdk::RFC9421OriginProofGenerationOptions {
            created: Some(1_712_000_000),
            expires: Some(1_712_000_300),
            nonce: Some("nonce-1".to_string()),
            label: Some("sig1".to_string()),
        },
    )
    .expect("generate RFC9421 origin proof through Go facade");
    anpsdk::VerifyRFC9421OriginProof(
        &origin_proof,
        "message.send",
        &json!({
            "target": {
                "kind": "agent",
                "did": "did:wba:example.com:user:bob:e1_bob"
            }
        }),
        &json!({"content": "hello"}),
        anpsdk::RFC9421OriginProofVerificationOptions {
            did_document: Some(bundle.did_document.clone()),
            verification_method: None,
            expected_signer_did: bundle.did().map(str::to_string),
        },
    )
    .expect("verify RFC9421 origin proof through Go facade");

    assert_eq!(anpsdk::TARGET_KIND_AGENT, anpsdk::TargetKind::Agent);
    assert_eq!(anpsdk::TARGET_KIND_GROUP, anpsdk::TargetKind::Group);
    assert_eq!(anpsdk::TARGET_KIND_SERVICE, anpsdk::TargetKind::Service);
    assert_eq!(anpsdk::TargetKindAgent, anpsdk::TargetKind::Agent);
    assert_eq!(anpsdk::TargetKindGroup, anpsdk::TargetKind::Group);
    assert_eq!(anpsdk::TargetKindService, anpsdk::TargetKind::Service);
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

#[test]
fn file_signed_prekey_store_creates_root_and_round_trips_go_files() {
    let workspace = TempDir::new().expect("temp workspace");
    let root = workspace.path().join("signed-prekeys");
    assert!(!root.exists());

    let mut store = anpsdk::NewFileSignedPrekeyStore(&root).expect("create signed prekey store");
    assert!(root.is_dir());

    let private_key = generated_x25519_private_key();
    let metadata = signed_prekey("spk-2026", &private_key);
    store
        .save_signed_prekey(&metadata.key_id, &private_key, &metadata)
        .expect("save signed prekey");

    let pem = std::fs::read_to_string(root.join("spk-2026.pem")).expect("read signed prekey pem");
    assert_eq!(pem, private_key.to_pem());

    let written_json =
        std::fs::read_to_string(root.join("spk-2026.json")).expect("read signed prekey json");
    assert_eq!(
        written_json,
        serde_json::to_string_pretty(&metadata).expect("pretty signed prekey json")
    );
    assert!(!written_json.ends_with('\n'));
    assert_eq!(
        std::fs::read_to_string(root.join("latest.txt")).expect("read latest signed prekey"),
        "spk-2026"
    );

    let (loaded_private_key, loaded_metadata) = store
        .load_signed_prekey("spk-2026")
        .expect("load signed prekey");
    assert_same_private_key(&loaded_private_key, &private_key);
    assert_eq!(loaded_metadata, metadata);

    let (latest_private_key, latest_metadata) = store
        .load_latest_signed_prekey()
        .expect("load latest signed prekey")
        .expect("latest signed prekey exists");
    assert_same_private_key(&latest_private_key, &private_key);
    assert_eq!(latest_metadata, metadata);
}

#[test]
fn file_signed_prekey_store_latest_missing_is_none_and_latest_whitespace_trims() {
    let workspace = TempDir::new().expect("temp workspace");
    let root = workspace.path().join("signed-prekeys");
    let mut store = anpsdk::FileSignedPrekeyStore::new(&root).expect("create signed prekey store");

    assert!(store
        .load_latest_signed_prekey()
        .expect("load missing latest signed prekey")
        .is_none());

    let private_key = generated_x25519_private_key();
    let metadata = signed_prekey("spk-current", &private_key);
    store
        .save_signed_prekey(&metadata.key_id, &private_key, &metadata)
        .expect("save signed prekey");
    std::fs::write(root.join("latest.txt"), "\n\t spk-current \r\n")
        .expect("write whitespace latest pointer");

    let (latest_private_key, latest_metadata) = store
        .load_latest_signed_prekey()
        .expect("load whitespace latest signed prekey")
        .expect("latest signed prekey exists");
    assert_same_private_key(&latest_private_key, &private_key);
    assert_eq!(latest_metadata, metadata);
}

#[test]
fn file_signed_prekey_store_missing_load_maps_to_go_invalid_field_message() {
    let workspace = TempDir::new().expect("temp workspace");
    let store = anpsdk::FileSignedPrekeyStore::new(workspace.path().join("signed-prekeys"))
        .expect("create signed prekey store");

    let error = store
        .load_signed_prekey("missing-spk")
        .expect_err("missing signed prekey should fail");
    assert_direct_invalid_field(error, "signed prekey not found: missing-spk");
}

#[test]
fn file_one_time_prekey_store_creates_root_lists_sorted_and_round_trips_go_files() {
    let workspace = TempDir::new().expect("temp workspace");
    let root = workspace.path().join("one-time-prekeys");
    assert!(!root.exists());

    let mut store = anpsdk::NewFileOneTimePrekeyStore(&root).expect("create one-time prekey store");
    assert!(root.is_dir());

    let private_key_a = generated_x25519_private_key();
    let private_key_b = generated_x25519_private_key();
    let private_key_c = generated_x25519_private_key();
    let metadata_a = one_time_prekey("opk-a", &private_key_a);
    let metadata_b = one_time_prekey("opk-b", &private_key_b);
    let metadata_c = one_time_prekey("opk-c", &private_key_c);

    store
        .save_one_time_prekey(&metadata_c.key_id, &private_key_c, &metadata_c)
        .expect("save opk c");
    store
        .save_one_time_prekey(&metadata_a.key_id, &private_key_a, &metadata_a)
        .expect("save opk a");
    store
        .save_one_time_prekey(&metadata_b.key_id, &private_key_b, &metadata_b)
        .expect("save opk b");

    let pem = std::fs::read_to_string(root.join("opk-b.pem")).expect("read opk pem");
    assert_eq!(pem, private_key_b.to_pem());

    let written_json = std::fs::read_to_string(root.join("opk-b.json")).expect("read opk json");
    assert_eq!(
        written_json,
        serde_json::to_string_pretty(&metadata_b).expect("pretty opk json")
    );
    assert!(!written_json.ends_with('\n'));

    let (loaded_private_key, loaded_metadata) = store
        .load_one_time_prekey("opk-b")
        .expect("load one-time prekey");
    assert_same_private_key(&loaded_private_key, &private_key_b);
    assert_eq!(loaded_metadata, metadata_b);

    assert_eq!(
        store
            .list_one_time_prekeys()
            .expect("list one-time prekeys"),
        vec![metadata_a, metadata_b, metadata_c]
    );
}

#[test]
fn file_one_time_prekey_store_delete_missing_succeeds_and_missing_load_maps_to_go_message() {
    let workspace = TempDir::new().expect("temp workspace");
    let root = workspace.path().join("one-time-prekeys");
    let mut store =
        anpsdk::FileOneTimePrekeyStore::new(&root).expect("create one-time prekey store");

    let private_key = generated_x25519_private_key();
    let metadata = one_time_prekey("opk-delete", &private_key);
    store
        .save_one_time_prekey(&metadata.key_id, &private_key, &metadata)
        .expect("save one-time prekey");
    store
        .delete_one_time_prekey("opk-delete")
        .expect("delete one-time prekey");
    assert!(!root.join("opk-delete.pem").exists());
    assert!(!root.join("opk-delete.json").exists());

    store
        .delete_one_time_prekey("missing-opk")
        .expect("delete missing one-time prekey");
    let error = store
        .load_one_time_prekey("missing-opk")
        .expect_err("missing one-time prekey should fail");
    assert_direct_invalid_field(error, "one-time prekey not found: missing-opk");
}

#[test]
fn file_pending_outbound_store_round_trips_json_and_delete_missing_succeeds() {
    let workspace = TempDir::new().expect("temp workspace");
    let root = workspace.path().join("pending-outbound");
    assert!(!root.exists());

    let mut store =
        anpsdk::NewFilePendingOutboundStore(&root).expect("create pending outbound store");
    assert!(root.is_dir());

    let pending = pending_outbound("op-pending-1");
    store.save_pending(&pending).expect("save pending record");

    let written_json =
        std::fs::read_to_string(root.join("op-pending-1.json")).expect("read pending json");
    assert_eq!(
        written_json,
        serde_json::to_string_pretty(&pending).expect("pretty pending json")
    );
    assert!(!written_json.ends_with('\n'));

    assert_eq!(
        store
            .load_pending("op-pending-1")
            .expect("load pending record"),
        pending
    );

    store
        .delete_pending("op-pending-1")
        .expect("delete pending record");
    assert!(!root.join("op-pending-1.json").exists());
    store
        .delete_pending("missing-pending")
        .expect("delete missing pending record");
}

#[test]
fn file_pending_outbound_store_missing_load_maps_to_pending_not_found() {
    let workspace = TempDir::new().expect("temp workspace");
    let store = anpsdk::FilePendingOutboundStore::new(workspace.path().join("pending-outbound"))
        .expect("create pending outbound store");

    let error = store
        .load_pending("missing-pending")
        .expect_err("missing pending record should fail");
    assert!(
        matches!(error, anpsdk::DirectE2eeError::PendingOutboundNotFound(operation_id) if operation_id == "missing-pending")
    );
}

#[test]
fn file_session_store_creates_root_and_round_trips_pretty_json_without_trailing_newline() {
    let workspace = TempDir::new().expect("temp workspace");
    let root = workspace.path().join("sessions").join("direct");
    assert!(!root.exists());

    let mut store = anpsdk::NewFileSessionStore(&root).expect("create session store");
    assert!(root.is_dir());

    let session = direct_session("session-1", "did:wba:example.com:user:bob:e1_bob");
    store.save_session(&session).expect("save session");

    let session_path = root.join("session-1.json");
    let written = std::fs::read_to_string(&session_path).expect("read session json");
    assert_eq!(
        written,
        serde_json::to_string_pretty(&session).expect("pretty session json")
    );
    assert!(!written.ends_with('\n'));

    let loaded = store.load_session("session-1").expect("load session");
    assert_eq!(loaded, session);
}

#[test]
fn file_session_store_missing_load_maps_to_session_not_found_and_delete_missing_succeeds() {
    let workspace = TempDir::new().expect("temp workspace");
    let mut store =
        anpsdk::FileSessionStore::new(workspace.path().join("sessions")).expect("create store");

    let error = store
        .load_session("missing-session")
        .expect_err("missing session should fail");
    assert!(
        matches!(error, anpsdk::DirectE2eeError::SessionNotFound(session_id) if session_id == "missing-session")
    );

    store
        .delete_session("missing-session")
        .expect("delete missing session");
}

#[test]
fn file_session_store_find_by_peer_did_uses_exact_match_and_first_lexicographic_json_path() {
    let workspace = TempDir::new().expect("temp workspace");
    let root = workspace.path().join("sessions");
    let store = anpsdk::FileSessionStore::new(&root).expect("create store");
    let peer_did = "did:wba:example.com:user:bob:e1_bob";

    write_session_json(
        &root,
        &direct_session("200-matching-session", peer_did),
        "write later matching session",
    );
    write_session_json(
        &root,
        &direct_session("000-prefix-session", &format!("{peer_did}:extra")),
        "write non-exact peer session",
    );
    let first_match = direct_session("100-matching-session", peer_did);
    write_session_json(&root, &first_match, "write first matching session");

    assert_eq!(
        store.find_by_peer_did(peer_did).expect("find peer session"),
        Some(first_match)
    );
    assert_eq!(
        store
            .find_by_peer_did("did:wba:example.com:user:carol:e1_carol")
            .expect("find missing peer session"),
        None
    );

    std::fs::remove_dir_all(&root).expect("remove session root");
    assert_eq!(
        store
            .find_by_peer_did(peer_did)
            .expect("missing root should not fail"),
        None
    );
}

#[test]
fn file_session_store_find_by_peer_did_aborts_on_malformed_json_in_matching_glob() {
    let workspace = TempDir::new().expect("temp workspace");
    let root = workspace.path().join("sessions");
    let store = anpsdk::FileSessionStore::new(&root).expect("create store");
    let peer_did = "did:wba:example.com:user:bob:e1_bob";

    std::fs::write(root.join("000-malformed.json"), "{not json").expect("write malformed json");
    write_session_json(
        &root,
        &direct_session("100-matching-session", peer_did),
        "write matching session",
    );

    assert!(store.find_by_peer_did(peer_did).is_err());
}

fn direct_session(session_id: &str, peer_did: &str) -> anpsdk::DirectSessionState {
    anpsdk::DirectSessionState {
        session_id: session_id.to_string(),
        suite: "ANP-DIRECT-E2EE-X3DH-25519-CHACHA20POLY1305-SHA256-V1".to_string(),
        peer_did: peer_did.to_string(),
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
    }
}

fn write_session_json(root: &Path, session: &anpsdk::DirectSessionState, context: &str) {
    let json = serde_json::to_string_pretty(session).expect("serialize session");
    std::fs::write(root.join(format!("{}.json", session.session_id)), json).expect(context);
}

fn pending_outbound(operation_id: &str) -> anpsdk::PendingOutboundRecord {
    anpsdk::PendingOutboundRecord {
        operation_id: operation_id.to_string(),
        message_id: "message-1".to_string(),
        wire_content_type: "application/anp-direct-cipher+json".to_string(),
        body_json: json!({
            "session_id": "session-1",
            "ratchet_header": {
                "dh_pub_b64u": "dh",
                "pn": "0",
                "n": "1"
            },
            "ciphertext_b64u": "cipher"
        }),
    }
}

fn generated_x25519_private_key() -> anpsdk::PrivateKeyMaterial {
    anpsdk::create_did_wba_document("example.com", anpsdk::DidDocumentOptions::default())
        .expect("create DID document with E2EE key")
        .load_private_key("key-3")
        .expect("load generated X25519 key agreement private key")
}

fn signed_prekey(key_id: &str, private_key: &anpsdk::PrivateKeyMaterial) -> anpsdk::SignedPrekey {
    anpsdk::SignedPrekey {
        key_id: key_id.to_string(),
        public_key_b64u: x25519_public_key_b64u(private_key),
        expires_at: "2027-05-15T00:00:00Z".to_string(),
    }
}

fn one_time_prekey(
    key_id: &str,
    private_key: &anpsdk::PrivateKeyMaterial,
) -> anpsdk::OneTimePrekey {
    anpsdk::OneTimePrekey {
        key_id: key_id.to_string(),
        public_key_b64u: x25519_public_key_b64u(private_key),
    }
}

fn x25519_public_key_b64u(private_key: &anpsdk::PrivateKeyMaterial) -> String {
    match private_key.public_key() {
        anpsdk::PublicKeyMaterial::X25519(bytes) => URL_SAFE_NO_PAD.encode(bytes),
        other => panic!("expected X25519 public key, got {other}"),
    }
}

fn assert_same_private_key(
    actual: &anpsdk::PrivateKeyMaterial,
    expected: &anpsdk::PrivateKeyMaterial,
) {
    assert_eq!(actual.to_pem(), expected.to_pem());
    assert_eq!(actual.public_key().to_pem(), expected.public_key().to_pem());
}

fn private_key_type(private_key: &anpsdk::PrivateKeyMaterial) -> anpsdk::KeyType {
    match private_key {
        anpsdk::PrivateKeyMaterial::Secp256k1(_) => anpsdk::KEY_TYPE_SECP256K1,
        anpsdk::PrivateKeyMaterial::Secp256r1(_) => anpsdk::KEY_TYPE_SECP256R1,
        anpsdk::PrivateKeyMaterial::Ed25519(_) => anpsdk::KeyTypeEd25519,
        anpsdk::PrivateKeyMaterial::X25519(_) => anpsdk::KeyTypeX25519,
    }
}

fn public_key_type(public_key: &anpsdk::PublicKeyMaterial) -> anpsdk::KeyType {
    match public_key {
        anpsdk::PublicKeyMaterial::Secp256k1(_) => anpsdk::KeyTypeSecp256k1,
        anpsdk::PublicKeyMaterial::Secp256r1(_) => anpsdk::KeyTypeSecp256r1,
        anpsdk::PublicKeyMaterial::Ed25519(_) => anpsdk::KEY_TYPE_ED25519,
        anpsdk::PublicKeyMaterial::X25519(_) => anpsdk::KEY_TYPE_X25519,
    }
}

fn first_line(value: &str) -> &str {
    value.split_once('\n').map_or(value, |(line, _)| line)
}

fn assert_direct_invalid_field(error: anpsdk::DirectE2eeError, expected: &str) {
    assert_eq!(error.to_string(), format!("invalid field: {expected}"));
    match error {
        anpsdk::DirectE2eeError::InvalidField(message) => assert_eq!(message, expected),
        other => panic!("expected DirectE2eeError::InvalidField, got {other:?}"),
    }
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> std::io::Result<Self> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "awiki-cli-rs2-anpsdk-contract-test-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
