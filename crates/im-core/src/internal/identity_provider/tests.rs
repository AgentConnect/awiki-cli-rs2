use super::*;

fn parity_fixture() -> serde_json::Value {
    serde_json::from_str(include_str!(
        "../../../../../testdata/identity_provider_parity_v1.json"
    ))
    .unwrap()
}

fn create_spec() -> ProviderCreateIdentityRequest {
    serde_json::from_value(parity_fixture()["create"].clone()).unwrap()
}

#[tokio::test]
async fn direct_session_runs_provider_neutral_hot_paths() {
    let fixture = parity_fixture();
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
    assert_eq!(
        session.host_status().await.unwrap().root_capability,
        ProviderRootCapability::Active
    );
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
            payload: fixture["sign"]["payloadUtf8"]
                .as_str()
                .unwrap()
                .as_bytes()
                .to_vec(),
        })
        .await
        .unwrap();
    assert_eq!(signature.kid, request_kid);
    assert_eq!(signature.bytes.len(), 64);

    let mut origin_meta = fixture["originProof"]["meta"].clone();
    origin_meta["sender_did"] = serde_json::Value::String(reference.did.clone());
    let proof = session
        .sign_origin_proof(ProviderOriginProofRequest {
            method: fixture["originProof"]["method"]
                .as_str()
                .unwrap()
                .to_owned(),
            meta: origin_meta,
            body: fixture["originProof"]["body"].clone(),
            key: ProviderKeySelector::Kid(request_kid.clone()),
            options: ProviderOriginProofOptions::default(),
        })
        .await
        .unwrap();
    assert!(proof.signature.starts_with("sig1=:"));

    let http = session
        .prepare_http_signature(ProviderExactHttpRequest {
            key: ProviderKeySelector::Kid(request_kid),
            url: fixture["httpSignature"]["url"].as_str().unwrap().to_owned(),
            method: fixture["httpSignature"]["method"]
                .as_str()
                .unwrap()
                .to_owned(),
            headers: serde_json::from_value(fixture["httpSignature"]["headers"].clone()).unwrap(),
            body: Some(
                fixture["httpSignature"]["bodyUtf8"]
                    .as_str()
                    .unwrap()
                    .as_bytes()
                    .to_vec(),
            ),
            options: ProviderHttpSigningOptions::default(),
        })
        .await
        .unwrap();
    assert!(http.binding_digest.starts_with("sha256:"));
    assert!(http
        .header_patch
        .iter()
        .any(|header| header.name.eq_ignore_ascii_case("signature")));

    let mut http_header_names = http
        .header_patch
        .iter()
        .map(|header| header.name.to_ascii_lowercase())
        .collect::<Vec<_>>();
    http_header_names.sort();
    let actual = serde_json::json!({
        "didPrefix": if snapshot.reference.did.starts_with("did:wba:example.com:provider-contract:e1_") {
            "did:wba:example.com:provider-contract:e1_"
        } else {
            ""
        },
        "state": snapshot.state,
        "revision": snapshot.revision,
        "didWba": snapshot.did_wba,
        "signatureAlgorithm": signature.algorithm,
        "signatureLength": signature.bytes.len(),
        "originSignaturePrefix": if proof.signature.starts_with("sig1=:") { "sig1=:" } else { "" },
        "httpBindingPrefix": if http.binding_digest.starts_with("sha256:") { "sha256:" } else { "" },
        "httpHeaderNames": http_header_names,
    });
    assert_eq!(actual, fixture["expected"]);

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
    assert_eq!(
        document_change.host_phase().await.unwrap(),
        ProviderDocumentChangePhase::Prepared
    );
    let attempt = document_change.begin_publication().await.unwrap();
    assert_eq!(
        document_change
            .complete(attempt, ProviderPublicationResult::RejectedBeforeAcceptance,)
            .await
            .unwrap(),
        ProviderDocumentChangeOutcome::Aborted
    );

    let successor = custody.create_identity(create_spec()).await.unwrap();
    let successor = successor.public_identity().await.unwrap();
    let transition = custody
        .prepare_identity_transition(ProviderIdentityTransitionRequest {
            expected_current_did: reference.did.clone(),
            operation_id: "provider-transition-response-loss".to_owned(),
            successor: successor.reference.clone(),
            transition_document: None,
            provider_document: None,
        })
        .await
        .unwrap();
    let transition_candidate = transition.candidate().await.unwrap();
    assert_eq!(transition_candidate.successor_did, successor.reference.did);
    let transition_attempt = transition.begin_publication().await.unwrap();
    assert_eq!(
        transition
            .complete(
                transition_attempt,
                ProviderIdentityTransitionPublicationResult::Unknown,
            )
            .await
            .unwrap(),
        ProviderIdentityTransitionOutcome::PublicationUncertain
    );
    let resumed = custody
        .resume_identity_transition(&reference.did)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resumed.candidate().await.unwrap(), transition_candidate);
    assert_eq!(
        resumed
            .reconcile(ProviderIdentityTransitionRemoteObservation::Published {
                predecessor_document: transition_candidate.predecessor_document,
                successor_document: transition_candidate.successor_document,
            })
            .await
            .unwrap(),
        ProviderIdentityTransitionOutcome::Committed {
            current_did: successor.reference.did,
        }
    );

    let enrollment_root = tempfile::tempdir().unwrap();
    let enrollment_manager =
        anp_identity::IdentityManager::initialize(anp_identity::IdentityManagerConfig {
            state_root: enrollment_root.path().to_path_buf(),
            root_key: anp_identity::RootKeySource::Injected(anp_identity::InjectedStoreKey::new(
                "provider-enrollment",
                [0x62; 32],
            )),
        })
        .unwrap();
    let enrollment_custody = direct::DirectAnpIdentityCustody::new(enrollment_manager);
    let enrollment = enrollment_custody
        .begin_device_enrollment(ProviderDeviceEnrollmentRequest {
            remote: ProviderVerifiedRemoteDocument {
                evidence: ProviderPublicationEvidence {
                    document_version: 1,
                    registry_version: 1,
                    document_digest: crate::internal::identity_wire::document::document_hash(
                        &snapshot.document,
                    )
                    .unwrap(),
                },
                document: snapshot.document,
            },
            device_id: "provider-device".to_owned(),
            device_signing_fragment: "provider-device-sign".to_owned(),
            device_agreement_fragment: "provider-device-agreement".to_owned(),
            profiles: vec!["https://anp.example/profiles/device/v1".to_owned()],
            capabilities: ProviderEnrollmentCapabilities { did_wba: true },
        })
        .await
        .unwrap();
    let proposal = enrollment.proposal().await.unwrap();
    let ProviderEnrollmentProposalKind::Device {
        signing_key,
        agreement_key,
        ..
    } = proposal.kind
    else {
        panic!("expected device enrollment proposal")
    };
    assert_eq!(
        enrollment
            .sign_device_assertion(b"provider enrollment".to_vec())
            .await
            .unwrap()
            .len(),
        64
    );
    assert_ne!(
        enrollment
            .derive_device_shared_secret(x25519_dalek::X25519_BASEPOINT_BYTES)
            .await
            .unwrap()
            .as_bytes(),
        &[0; 32]
    );
    assert!(signing_key.kid.ends_with("#provider-device-sign"));
    assert!(agreement_key.kid.ends_with("#provider-device-agreement"));
    enrollment.cancel().await.unwrap();
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
        .split("impl ProviderSharedSecret")
        .next()
        .unwrap();
    assert!(!secret.contains("Serialize"));
    assert!(!secret.contains("Debug"));
    assert!(!secret.contains("Clone"));
}
