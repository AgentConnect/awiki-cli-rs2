use awiki_im_core::ids::{ProtocolDeviceId, VaultContextDeviceId};

#[test]
fn protocol_device_id_is_random_opaque_and_not_legacy_default() {
    let first = ProtocolDeviceId::generate().expect("secure device id");
    let second = ProtocolDeviceId::generate().expect("secure device id");

    assert_ne!(first, second);
    assert!(first.as_str().starts_with("dev-"));
    assert!(ProtocolDeviceId::parse(first.as_str()).is_ok());
    assert!(ProtocolDeviceId::parse("default").is_err());
}

#[test]
fn vault_context_device_id_is_a_distinct_local_type() {
    let vault_context = VaultContextDeviceId::parse("awiki-me.scope-device.v1.profile-a")
        .expect("valid vault context device id");
    let protocol =
        ProtocolDeviceId::parse("dev-public-endpoint").expect("valid protocol device id");

    assert_eq!(vault_context.as_str(), "awiki-me.scope-device.v1.profile-a");
    assert_eq!(protocol.as_str(), "dev-public-endpoint");
    assert!(VaultContextDeviceId::parse("  ").is_err());
}
