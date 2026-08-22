use super::*;

fn create_spec() -> anp_identity::CreateIdentityRequest {
    anp_identity::CreateIdentityRequest {
        profile: anp_identity::DidProfile::E1,
        domain: "example.com".to_owned(),
        port: None,
        path_segments: vec!["provider-contract".to_owned()],
        capabilities: anp_identity::Capabilities { did_wba: true },
        managed_keys: vec![
            anp_identity::ManagedKeySpec {
                fragment: "root".to_owned(),
                role: anp_identity::KeyRole::RootControl,
            },
            anp_identity::ManagedKeySpec {
                fragment: "request".to_owned(),
                role: anp_identity::KeyRole::RequestSigning,
            },
            anp_identity::ManagedKeySpec {
                fragment: "agreement".to_owned(),
                role: anp_identity::KeyRole::E2eeAgreement,
            },
        ],
        external_keys: Vec::new(),
        services: Vec::new(),
        agent_description_url: None,
        extensions: Vec::new(),
    }
}

#[tokio::test]
async fn direct_session_runs_provider_neutral_hot_paths() {
    let root = tempfile::tempdir().unwrap();
    let mut manager =
        anp_identity::IdentityManager::initialize(anp_identity::IdentityManagerConfig {
            state_root: root.path().to_path_buf(),
            root_key: anp_identity::RootKeySource::Injected(anp_identity::InjectedStoreKey::new(
                "provider-contract",
                [0x61; 32],
            )),
        })
        .unwrap();
    let identity = manager.create(create_spec()).unwrap();
    let reference: ProviderIdentityRef = identity.reference().into();
    let custody = direct::DirectAnpIdentityCustody::new(manager);

    assert_eq!(custody.store_info().await.unwrap().identity_count, 1);
    assert_eq!(custody.list_identities().await.unwrap().len(), 1);
    let session = custody.open_identity(&reference).await.unwrap();
    let snapshot = session.public_identity().await.unwrap();
    assert_eq!(snapshot.reference, reference);
    assert_eq!(snapshot.state, ProviderIdentityState::Active);

    let request_kid = snapshot
        .active_keys
        .iter()
        .find(|key| {
            key.purposes.contains(&ProviderKeyPurpose::Authentication)
                && key
                    .purposes
                    .contains(&ProviderKeyPurpose::ApplicationAssertion)
        })
        .unwrap()
        .kid
        .clone();
    let signature = session
        .sign(ProviderSignRequest {
            purpose: ProviderSigningPurpose::Authentication,
            key: ProviderKeySelector::Kid(request_kid.clone()),
            payload: b"provider contract".to_vec(),
        })
        .await
        .unwrap();
    assert_eq!(signature.kid, request_kid);
    assert_eq!(signature.bytes.len(), 64);

    let proof = session
        .sign_origin_proof(ProviderOriginProofRequest {
            method: "message.send".to_owned(),
            meta: serde_json::json!({
                "sender_did": reference.did,
                "timestamp": 1_787_403_600_i64,
                "target": {
                    "kind": "agent",
                    "did": "did:wba:example.com:agents:recipient"
                },
                "operation_id": "provider-contract-operation",
                "message_id": "provider-contract-message",
                "content_type": "application/json"
            }),
            body: serde_json::json!({"message": "provider contract"}),
            key: ProviderKeySelector::Kid(request_kid.clone()),
            options: ProviderOriginProofOptions::default(),
        })
        .await
        .unwrap();
    assert!(proof.signature.starts_with("sig1=:"));

    let http = session
        .prepare_http_signature(ProviderExactHttpRequest {
            key: ProviderKeySelector::Kid(request_kid),
            url: "https://example.com/rpc".to_owned(),
            method: "POST".to_owned(),
            headers: vec![ProviderHttpHeader {
                name: "content-type".to_owned(),
                value: "application/json".to_owned(),
            }],
            body: Some(b"{}".to_vec()),
            options: ProviderHttpSigningOptions::default(),
        })
        .await
        .unwrap();
    assert!(http.binding_digest.starts_with("sha256:"));
    assert!(http
        .header_patch
        .iter()
        .any(|header| header.name.eq_ignore_ascii_case("signature")));

    let shared = session
        .derive_shared_secret(ProviderKeyAgreementRequest {
            key: ProviderKeySelector::Default,
            peer_public: x25519_dalek::X25519_BASEPOINT_BYTES,
        })
        .await
        .unwrap();
    assert_ne!(shared.as_bytes(), &[0; 32]);
}

#[test]
fn provider_contract_dtos_are_versioned_and_secret_free() {
    assert_eq!(IDENTITY_PROVIDER_PROTOCOL, "anp-identity-provider-ts/1");
    assert_eq!(CAP_STORE_READ, "store.read");
    assert_eq!(CAP_IDENTITY_SIGN, "identity.sign");
    assert_eq!(CAP_ORIGIN_PROOF, "identity.origin-proof");
    assert_eq!(CAP_HTTP_SIGN, "host.http-sign");
    assert_eq!(CAP_KEY_AGREEMENT, "host.key-agreement");

    let source = include_str!("mod.rs");
    let secret = source
        .split("pub(crate) struct ProviderSharedSecret")
        .nth(1)
        .unwrap()
        .split("#[async_trait]")
        .next()
        .unwrap();
    assert!(!secret.contains("Serialize"));
    assert!(!secret.contains("Debug"));
    assert!(!secret.contains("Clone"));
}
