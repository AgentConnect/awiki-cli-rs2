use super::*;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
fn hash(value: &Value) -> String {
    format!(
        "{:x}",
        Sha256::digest(serde_json_canonicalizer::to_vec(value).unwrap())
    )
}
#[test]
fn node_vector_and_unrelated_application_objects_use_the_same_facade() {
    let v: Value = serde_json::from_str(include_str!("testdata/vectors.json")).unwrap();
    let review = ObjectProofReview::parse(
        &serde_json::to_vec(&v["snapshot"]).unwrap(),
        v["intent_hash"].as_str().unwrap(),
    )
    .unwrap();
    assert_eq!(review.presentation()["object"], v["snapshot"]);
    let other =
        json!({"type":"Invoice","amount":12.5,"details":{"currency":"CNY","note":"中文 🎉"}});
    let review =
        ObjectProofReview::parse(&serde_json::to_vec(&other).unwrap(), &hash(&other)).unwrap();
    assert_eq!(review.presentation()["object"], other);
}
#[test]
fn whole_top_level_proof_is_excluded_but_nested_proofs_are_data() {
    let base = json!({"payload":{"proof":"signed application data"}});
    for proof in [
        json!({"signature":"old"}),
        json!([{"signature":"a"},{"signature":"b"}]),
    ] {
        let mut signed = base.clone();
        signed["proof"] = proof;
        let review =
            ObjectProofReview::parse(&serde_json::to_vec(&signed).unwrap(), &hash(&base)).unwrap();
        assert_eq!(review.presentation()["object"], base);
    }
}
#[test]
fn duplicates_bad_digests_and_non_objects_are_rejected() {
    for raw in [
        r#"{"a":1,"a":2}"#,
        r#"{"nested":{"a":1,"a":2}}"#,
        r#"{"proof":{"x":1,"x":2}}"#,
    ] {
        let mut permissive: Value = serde_json::from_str(raw).unwrap();
        permissive.as_object_mut().unwrap().remove("proof");
        assert!(ObjectProofReview::parse(raw.as_bytes(), &hash(&permissive)).is_err());
    }
    for raw in [
        "[]",
        "null",
        r#"{"n":9007199254740992}"#,
        r#"{"n":9.007199254740992e15}"#,
    ] {
        let value: Value = serde_json::from_str(raw).unwrap();
        assert!(ObjectProofReview::parse(raw.as_bytes(), &hash(&value)).is_err());
    }
    assert!(ObjectProofReview::parse(b"{}", &"0".repeat(64)).is_err());
}
#[test]
fn device_validation_binds_registry_document_and_assertion_key() {
    use crate::internal::identity_device_join_runtime::{
        DeviceJoinRemoteDeviceSummary, DeviceJoinRemoteRegistry,
    };
    use crate::internal::identity_device_state::{
        DeviceAuthorizationRole, DeviceAuthorizationStatus, IdentityInternalCheckpoint,
    };
    use sha2::{Digest, Sha256};
    let g = crate::internal::identity_generation::generate_vnext_handle_identity_with_default_daemon_subkey("awiki.test", "alice", None, None).unwrap();
    let mut registry = DeviceJoinRemoteRegistry {
        did: g.did.clone(),
        checkpoint: IdentityInternalCheckpoint {
            document_version: 1,
            registry_version: 1,
            document_hash: crate::internal::identity_wire::document::document_hash(&g.did_document)
                .unwrap(),
        },
        devices: vec![DeviceJoinRemoteDeviceSummary {
            device_id: g.protocol_device_id.as_str().to_owned(),
            signing_key_id: g.device_signing_key_id.clone(),
            e2ee_key_id: g.device_e2ee_key_id.clone(),
            status: DeviceAuthorizationStatus::Active,
            role: DeviceAuthorizationRole::Admin,
            management_ready: true,
            auth_generation: 1,
        }],
    };
    let check = |r: &DeviceJoinRemoteRegistry, kid: &str| {
        service::validate_device(
            &g.did_document,
            g.did.as_str(),
            g.protocol_device_id.as_str(),
            kid,
            r,
        )
    };
    assert!(registry.checkpoint.document_hash.starts_with("sha256:"));
    assert!(check(&registry, &g.device_signing_key_id).is_ok());
    let correct = registry.checkpoint.document_hash.clone();
    registry.checkpoint.document_hash = format!(
        "{:x}",
        Sha256::digest(serde_json_canonicalizer::to_vec(&g.did_document).unwrap())
    );
    assert!(check(&registry, &g.device_signing_key_id).is_err());
    registry.checkpoint.document_hash = correct;
    assert!(check(&registry, &g.root_key_id).is_err());
    registry.devices[0].signing_key_id = g.root_key_id.clone();
    assert!(check(&registry, &g.device_signing_key_id).is_err());
    registry.devices[0].signing_key_id = g.device_signing_key_id.clone();
    registry.devices[0].status = DeviceAuthorizationStatus::Revoked;
    assert!(check(&registry, &g.device_signing_key_id).is_err());
    registry.devices[0].status = DeviceAuthorizationStatus::Active;
    registry.checkpoint.document_hash = "0".repeat(64);
    assert!(check(&registry, &g.device_signing_key_id).is_err());
}

#[test]
fn standard_object_proofs_verify_for_generic_and_publication_payloads() {
    use anp::PrivateKeyMaterial;
    let vectors: Value = serde_json::from_str(include_str!("testdata/vectors.json")).unwrap();
    let did_document = &vectors["did_documents"][0];
    let did = did_document["id"].as_str().unwrap();
    let key = PrivateKeyMaterial::Ed25519(ed25519_dalek::SigningKey::from_bytes(&[17; 32]));
    for object in [
        vectors["snapshot"].clone(),
        json!({"type":"Invoice","total":12.5,"items":["服务"]}),
    ] {
        let review =
            ObjectProofReview::parse(&serde_json::to_vec(&object).unwrap(), &hash(&object))
                .unwrap();
        let signed = anp::proof::generate_object_proof(
            &review.object,
            &key,
            &format!("{did}#key-1"),
            did,
            Some("2026-09-08T08:00:00Z".into()),
        )
        .unwrap();
        anp::proof::verify_object_proof(&signed, did, did_document).unwrap();
        let mut changed = signed;
        changed["tampered"] = true.into();
        assert!(anp::proof::verify_object_proof(&changed, did, did_document).is_err());
    }
}
