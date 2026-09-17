use super::*;
use serde_json::json;

fn document() -> Value {
    json!({"id":"did:wba:example.test:user:alice:e1_fixture",
        "verificationMethod":[{"id":"root","publicKeyMultibase":"public-root"}],
        "deviceManifest":{"type":"ANPDeviceManifest","devices":[{"device_id":"only-device","profiles":["unchanged"]}]},
        "service":[{"id":"did:wba:example.test:user:alice:e1_fixture#message","type":"ANPMessageService","serviceEndpoint":"https://example.test/anp-im/rpc","serviceDid":"did:wba:example.test","profiles":["anp.group.base.v1"],"securityProfiles":["transport-protected"]},
                   {"id":"#handle","type":"ANPHandleService","serviceEndpoint":"https://example.test/.well-known/handle/alice"}],
        "proof":{"proofValue":"old-proof"}})
}

#[test]
fn community_document_patch_changes_only_service_profiles() {
    let original = document();
    let upgraded = upgraded_document(&original).unwrap();
    assert_eq!(upgraded["deviceManifest"], original["deviceManifest"]);
    assert_eq!(
        upgraded["verificationMethod"],
        original["verificationMethod"]
    );
    assert_eq!(upgraded["id"], original["id"]);
    assert_eq!(
        upgraded["service"][0]["profiles"],
        json!(["anp.group.base.v1", GROUP_V2])
    );
    assert_eq!(upgraded_document(&upgraded).unwrap(), upgraded);
    let request = service_change(&upgraded).unwrap();
    assert_eq!(request["changes"][0]["change"], "replace_services");
    assert_eq!(request["changes"][0]["services"][0]["id"], "message");
    assert_eq!(request["changes"][0]["services"][1]["id"], "handle");
}

#[test]
fn community_document_guard_does_not_accept_key_device_or_endpoint_changes() {
    let expected = upgraded_document(&document()).unwrap();
    let mut signed = expected.clone();
    signed["proof"] = json!({"proofValue":"new-proof"});
    assert!(same_unsigned(&expected, &signed));
    for pointer in [
        "/id",
        "/verificationMethod/0/publicKeyMultibase",
        "/deviceManifest/devices/0/device_id",
        "/service/0/serviceEndpoint",
    ] {
        let mut invalid = signed.clone();
        *invalid.pointer_mut(pointer).unwrap() = json!("replacement");
        assert!(!same_unsigned(&expected, &invalid));
    }
    let mut extended = expected.clone();
    extended["service"][0]["customPolicy"] = json!({"preserve":true});
    assert!(service_change(&extended).is_err());
}
