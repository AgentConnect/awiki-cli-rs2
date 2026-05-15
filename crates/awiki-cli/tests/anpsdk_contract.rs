use awiki_cli::anpsdk;
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

#[test]
fn file_session_store_creates_root_and_round_trips_pretty_json_without_trailing_newline() {
    let workspace = TempDir::new().expect("temp workspace");
    let root = workspace.path().join("sessions").join("direct");
    assert!(!root.exists());

    let mut store = anpsdk::FileSessionStore::new(&root).expect("create session store");
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
