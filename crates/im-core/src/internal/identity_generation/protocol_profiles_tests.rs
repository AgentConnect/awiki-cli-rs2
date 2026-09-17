use super::*;

#[test]
fn service_discovery_advertises_group_v2_and_preserves_exact_device_bundle() {
    let generated = generate_vnext_handle_identity_with_default_daemon_subkey(
        "example.test",
        "alice",
        None,
        None,
    )
    .unwrap();
    let document = &generated.did_document;
    let manifest = anp::authentication::validate_device_manifest(document)
        .unwrap()
        .unwrap();
    let profiles = &manifest.devices[0].profiles;
    assert!(!profiles
        .iter()
        .any(|profile| profile == "anp.group.base.v2"));
    assert_eq!(profiles.len(), 6);
    for dependency in [
        "anp.core.binding.v1",
        "anp.identity.discovery.v1",
        "anp.group.base.v1",
        "anp.group.e2ee.v2",
        "anp.direct.e2ee.v2",
    ] {
        assert!(profiles.iter().any(|profile| profile == dependency));
    }
    let service = document["service"]
        .as_array()
        .unwrap()
        .iter()
        .find(|value| value["type"] == "ANPMessageService")
        .unwrap();
    assert!(service["profiles"]
        .as_array()
        .unwrap()
        .iter()
        .any(|profile| profile == "anp.group.base.v2"));
    let spec = vnext_handle_anp_identity_create_spec("example.test", "alice", None, None).unwrap();
    let provider_service = spec
        .spec
        .services
        .iter()
        .find(|service| service.service_type == "ANPMessageService")
        .unwrap();
    assert!(provider_service
        .profiles
        .iter()
        .any(|profile| profile == "anp.group.base.v2"));
    assert!(spec
        .spec
        .extensions
        .iter()
        .any(|extension| match extension {
            crate::internal::identity_provider::ProviderIdentityExtension::DeviceManifest {
                devices,
            } => devices
                .iter()
                .all(|device| device.profiles.as_slice() == profiles.as_slice()),
        }));
}
