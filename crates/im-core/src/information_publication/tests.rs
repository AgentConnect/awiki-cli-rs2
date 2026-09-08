use super::*;
use serde_json::Value;
fn vector() -> Value {
    serde_json::from_str(include_str!("testdata/vectors.json")).unwrap()
}
fn parse(snapshot: &[u8], md: String) -> crate::ImResult<InformationPublicationReview> {
    let v = vector();
    let s = &v["snapshot"];
    InformationPublicationReview::parse(
        snapshot,
        md,
        s["targetServiceId"].as_str().unwrap(),
        s["tenantId"].as_str().unwrap(),
        s["operationId"].as_str().unwrap(),
        v["intent_hash"].as_str().unwrap(),
    )
}
#[test]
fn golden_node_snapshot_keeps_exact_markdown_and_jcs_hash() {
    let v = vector();
    let md = include_str!("testdata/sample.md");
    let review = parse(&serde_json::to_vec(&v["snapshot"]).unwrap(), md.to_owned()).unwrap();
    assert_eq!(review.intent_hash(), v["intent_hash"].as_str().unwrap());
    assert_eq!(review.presentation()["markdown"], md);
    let issued = chrono::DateTime::parse_from_rfc3339(v["snapshot"]["issuedAt"].as_str().unwrap())
        .unwrap()
        .timestamp();
    let expires =
        chrono::DateTime::parse_from_rfc3339(v["snapshot"]["expiresAt"].as_str().unwrap())
            .unwrap()
            .timestamp();
    assert!(review.check_time(issued).is_ok());
    assert!(review.check_time(expires - 1).is_ok());
    assert!(review.check_time(expires).is_err());
    assert!(review.check_time(issued - 31).is_err());
    assert!(parse(
        &serde_json::to_vec(&v["snapshot"]).unwrap(),
        md.replace("\r\n", "\n")
    )
    .is_err());
}
#[test]
fn rejects_nested_duplicates_unknown_fields_profile_target_and_hash_changes() {
    let v = vector();
    let s = serde_json::to_string(&v["snapshot"]).unwrap();
    let md = include_str!("testdata/sample.md").to_owned();
    for bad in [
        s.replacen("\"title\":", "\"title\":\"shadow\",\"title\":", 1),
        s.replacen("\"publisher\":{", "\"publisher\":{\"extra\":true,", 1),
        s.replacen("\"version\":1", "\"version\":1.0", 1),
        s.replace("awiki-information-publish-v1", "generic-object"),
        s.replace("urn:uuid:", "urn:node:"),
        s.replacen("\"operationId\":", "\"proof\":{},\"operationId\":", 1),
    ] {
        assert_ne!(s, bad);
        assert!(parse(bad.as_bytes(), md.clone()).is_err(), "{bad}");
    }
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
            document_hash: format!(
                "{:x}",
                Sha256::digest(serde_json_canonicalizer::to_vec(&g.did_document).unwrap())
            ),
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
    assert!(check(&registry, &g.device_signing_key_id).is_ok());
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
