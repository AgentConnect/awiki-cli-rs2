use super::*;

fn create_spec() -> ProviderCreateIdentityRequest {
    ProviderCreateIdentityRequest {
        profile: ProviderDidProfile::E1,
        domain: "example.com".to_owned(),
        port: None,
        path_segments: vec!["provider-contract".to_owned()],
        capabilities: ProviderCapabilities { did_wba: true },
        managed_keys: vec![
            ProviderManagedKeySpec {
                fragment: "root".to_owned(),
                role: ProviderManagedKeyRole::RootControl,
            },
            ProviderManagedKeySpec {
                fragment: "request".to_owned(),
                role: ProviderManagedKeyRole::RequestSigning,
            },
            ProviderManagedKeySpec {
                fragment: "agreement".to_owned(),
                role: ProviderManagedKeyRole::E2eeAgreement,
            },
        ],
        services: Vec::new(),
        agent_description_url: None,
        extensions: Vec::new(),
    }
}

#[tokio::test]
async fn direct_session_runs_provider_neutral_hot_paths() {
    let root = tempfile::tempdir().unwrap();
    let manager = anp_identity::IdentityManager::initialize(anp_identity::IdentityManagerConfig {
        state_root: root.path().to_path_buf(),
        root_key: anp_identity::RootKeySource::Injected(anp_identity::InjectedStoreKey::new(
            "provider-contract",
            [0x61; 32],
        )),
    })
    .unwrap();
    let custody = direct::DirectAnpIdentityCustody::new(manager);

    let session = custody.create_identity(create_spec()).await.unwrap();
    let snapshot = session.public_identity().await.unwrap();
    let reference = snapshot.reference.clone();
    assert_eq!(snapshot.reference, reference);
    assert_eq!(snapshot.state, ProviderIdentityState::Active);
    assert_eq!(custody.store_info().await.unwrap().identity_count, 1);
    assert_eq!(custody.list_identities().await.unwrap().len(), 1);

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

    let document_change = session
        .prepare_document_change(serde_json::json!({
            "changes": [{"change": "replace_services", "services": []}]
        }))
        .await
        .unwrap();
    let candidate = document_change.candidate().await.unwrap();
    assert!(!candidate.operation_id.is_empty());
    let attempt = document_change.begin_publication().await.unwrap();
    assert_eq!(
        document_change
            .complete(attempt, ProviderPublicationResult::RejectedBeforeAcceptance,)
            .await
            .unwrap(),
        ProviderDocumentChangeOutcome::Aborted
    );
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
        .split("pub struct ProviderSharedSecret")
        .nth(1)
        .unwrap()
        .split("#[async_trait]")
        .next()
        .unwrap();
    assert!(!secret.contains("Serialize"));
    assert!(!secret.contains("Debug"));
    assert!(!secret.contains("Clone"));
}
