use anp_identity::{
    CreateIdentityCapabilities, CreateIdentityExtension, CreateIdentityProfile,
    CreateIdentityRequest, DeviceManifestEntryInput, IdentityService, ManagedKeyInput,
    ManagedKeyRole,
};
use serde_json::json;

use super::*;
use crate::internal::key_provider::IdentitySigner;

#[test]
fn anp_identity_signer_routes_typed_crypto_and_file_auth_without_private_exports() {
    let root = tempfile::tempdir().unwrap();
    let mut manager =
        anp_identity::IdentityManager::initialize(anp_identity::IdentityManagerConfig {
            state_root: root.path().join("store"),
            root_key: anp_identity::RootKeySource::Injected(anp_identity::InjectedStoreKey::new(
                "host",
                [41_u8; 32],
            )),
        })
        .unwrap();
    let identity = manager.create(spec()).unwrap();
    let auth_path = root.path().join("identity/auth.json");
    crate::internal::auth::state::persist_jwt_token(&auth_path, "test-token").unwrap();
    let signer = AnpIdentitySigner::new_file(identity, auth_path);
    let did = signer.did_document().unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let device_kid = format!("{did}#device");
    let request_kid = format!("{did}#request");
    let root_kid = format!("{did}#root");
    let agreement_kid = format!("{did}#agreement");

    assert_eq!(signer.request_signing_key_id().unwrap(), request_kid);
    assert_eq!(signer.agreement_key_id().unwrap(), agreement_kid);
    assert_eq!(
        signer
            .sign_device_assertion(&device_kid, b"device assertion")
            .unwrap()
            .len(),
        64
    );
    let signature = signer.sign(&request_kid, b"request").unwrap();
    signer
        .public_key(&request_kid)
        .unwrap()
        .verify_message(b"request", &signature)
        .unwrap();
    let object = json!({"operation": "device.revoke"});
    let signed = signer
        .sign_object_proof(&device_kid, &object, &did, None)
        .unwrap();
    anp::proof::verify_object_proof(&signed, &did, &signer.did_document().unwrap()).unwrap();
    let origin = signer
        .sign_origin_proof(
            "message.send",
            &json!({
                "sender_did": did,
                "timestamp": 1_787_403_600_i64,
                "target": {"kind": "agent", "did": "did:wba:example.com:peer"},
                "operation_id": "operation-1",
                "message_id": "message-1",
                "content_type": "application/json"
            }),
            &json!({"message": "device-signed"}),
            &request_kid,
            anp::proof::Rfc9421OriginProofGenerationOptions::default(),
        )
        .unwrap();
    assert!(origin.signature.starts_with("sig1=:"));
    assert_eq!(
        signer.sign_object_proof(&request_kid, &object, &did, None),
        Err(crate::ImError::PermissionDenied)
    );
    let mut unsigned = signer.did_document().unwrap();
    unsigned.as_object_mut().unwrap().remove("proof");
    let resigned = signer
        .sign_document_proof(
            &unsigned,
            &root_kid,
            anp::proof::ProofGenerationOptions {
                proof_purpose: Some("assertionMethod".to_string()),
                proof_type: Some(anp::proof::PROOF_TYPE_DATA_INTEGRITY.to_string()),
                cryptosuite: Some(anp::proof::CRYPTOSUITE_EDDSA_JCS_2022.to_string()),
                domain: Some("example.com".to_string()),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(anp::authentication::validate_did_document_binding(
        &resigned, true
    ));
    assert_eq!(
        signer.sign_root(&root_kid, b"raw root signing"),
        Err(crate::ImError::PermissionDenied)
    );
    let peer = x25519_dalek::StaticSecret::from([9_u8; 32]);
    signer
        .ecdh(
            &agreement_kid,
            &x25519_dalek::PublicKey::from(&peer).to_bytes(),
        )
        .unwrap();
    let headers = signer
        .http_signature_headers(
            &device_kid,
            "https://example.com/im",
            "POST",
            None,
            Some(br#"{"message":"hello"}"#),
            anp::authentication::HttpSignatureOptions {
                keyid: None,
                nonce: Some("provider-challenge".to_string()),
                created: Some(1_700_000_000),
                expires: Some(1_700_000_300),
                covered_components: None,
            },
        )
        .unwrap();
    assert!(headers["Signature-Input"].contains("nonce=\"provider-challenge\""));
    assert_eq!(
        signer.valid_auth_token().unwrap().as_deref(),
        Some("test-token")
    );
    assert!(!format!("{signer:?}").contains("test-token"));
    signer.reload().unwrap();
}

#[test]
fn anp_identity_signer_reloads_once_after_external_generation_advance() {
    let root = tempfile::tempdir().unwrap();
    let mut external_manager =
        anp_identity::IdentityManager::initialize(anp_identity::IdentityManagerConfig {
            state_root: root.path().join("store"),
            root_key: anp_identity::RootKeySource::Injected(anp_identity::InjectedStoreKey::new(
                "host",
                [42_u8; 32],
            )),
        })
        .unwrap();
    let mut external = external_manager.create(spec()).unwrap();
    let reference = external.reference();
    let did = reference.did.clone();
    let manager = anp_identity::IdentityManager::open(anp_identity::IdentityManagerConfig {
        state_root: root.path().join("store"),
        root_key: anp_identity::RootKeySource::Injected(anp_identity::InjectedStoreKey::new(
            "host",
            [42_u8; 32],
        )),
    })
    .unwrap();
    let identity = manager.get(&reference).unwrap();
    let request_kid = format!("{did}#request");
    let signer = AnpIdentitySigner::new_ephemeral(identity);
    let mut change = external
        .prepare_document_change(anp_identity::DocumentChangeRequest {
            changes: vec![anp_identity::DocumentChange::ReplaceServices {
                services: vec![IdentityService {
                    id: "message-v2".to_owned(),
                    service_type: "ANPMessageService".to_owned(),
                    service_endpoint: "https://example.com/im/v2".to_owned(),
                    service_did: None,
                    profiles: vec!["anp.core.binding.v1".to_owned()],
                    security_profiles: vec!["transport-protected".to_owned()],
                }],
            }],
        })
        .unwrap();
    let candidate = change.candidate().clone();
    let attempt = change.begin_publication().unwrap();
    change
        .complete(
            attempt,
            anp_identity::PublicationResult::Confirmed {
                evidence: anp_identity::VerifiedPublicationEvidence {
                    document_version: 2,
                    registry_version: 2,
                    document_digest: candidate.candidate_digest,
                },
            },
        )
        .unwrap();

    let signature = signer.sign(&request_kid, b"after-update").unwrap();
    signer
        .public_key(&request_kid)
        .unwrap()
        .verify_message(b"after-update", &signature)
        .unwrap();
    assert!(signer.did_document().unwrap()["service"]
        .as_array()
        .unwrap()
        .iter()
        .any(|service| service["id"]
            .as_str()
            .is_some_and(|id| id.ends_with("#message-v2"))));
}

fn spec() -> CreateIdentityRequest {
    CreateIdentityRequest {
        profile: CreateIdentityProfile::E1,
        domain: "example.com".to_string(),
        port: None,
        path_segments: vec!["providers".to_string(), "awiki".to_string()],
        capabilities: CreateIdentityCapabilities { did_wba: true },
        managed_keys: vec![
            managed("root", ManagedKeyRole::RootControl),
            managed("device", ManagedKeyRole::DeviceSigning),
            managed("request", ManagedKeyRole::RequestSigning),
            managed("agreement", ManagedKeyRole::E2eeAgreement),
        ],
        external_keys: Vec::new(),
        services: Vec::new(),
        agent_description_url: None,
        extensions: vec![CreateIdentityExtension::DeviceManifest {
            devices: vec![DeviceManifestEntryInput {
                device_id: "device-a".to_string(),
                signing_key_id: "#device".to_string(),
                e2ee_key_id: "#agreement".to_string(),
                profiles: vec!["anp.core.binding.v1".to_string()],
            }],
        }],
    }
}

fn managed(fragment: &str, role: ManagedKeyRole) -> ManagedKeyInput {
    ManagedKeyInput {
        fragment: fragment.to_string(),
        role,
    }
}
