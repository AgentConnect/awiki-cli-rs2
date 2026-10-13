use super::ClientVersionInfo;

#[test]
fn client_version_info_encodes_the_only_wire_format() {
    assert_eq!(
        ClientVersionInfo::new("awiki-me", "0714", "1.0.31", Some(214))
            .unwrap()
            .header_value(),
        "awiki-me/0714/1.0.31+214"
    );
    assert_eq!(
        ClientVersionInfo::new("awiki-cli", "0714", "1.0.29", None)
            .unwrap()
            .header_value(),
        "awiki-cli/0714/1.0.29"
    );
}

#[test]
fn client_version_info_rejects_unknown_products_and_ambiguous_segments() {
    for result in [
        ClientVersionInfo::new("custom", "0714", "1.0.0", None),
        ClientVersionInfo::new("awiki-cli", "714", "1.0.0", None),
        ClientVersionInfo::new("awiki-cli", "0714", "1.0.0+other", None),
        ClientVersionInfo::new("awiki-cli", "0714", "dev", None),
        ClientVersionInfo::new("awiki-cli", "0714", "01.0.0", None),
        ClientVersionInfo::new("awiki-cli", "0714", "1.0.0.0.1", None),
        ClientVersionInfo::new("awiki-cli", "0714", "1.0.", None),
    ] {
        assert!(result.is_err());
    }
}

#[test]
fn client_version_info_keeps_each_product_in_its_own_version_namespace() {
    for (product, version) in [
        ("awiki-me", "1.0.58"),
        ("awiki-cli", "1.0.58"),
        ("awiki-daemon", "0.1.93"),
        ("awiki-dsh", "0.3.17"),
        ("awiki-gateway", "0.1.0"),
    ] {
        let value = ClientVersionInfo::new(product, "0815", version, None).unwrap();
        assert_eq!(value.product(), product);
        assert_eq!(value.version(), version);
        assert_eq!(value.header_value(), format!("{product}/0815/{version}"));
    }
}

#[test]
fn client_version_info_rejects_product_aliases_and_header_injection() {
    for product in [
        "awiki-guest",
        "AWIKI-DSH",
        " awiki-dsh",
        "awiki-gateway ",
        "awiki-dsh/0815",
        "awiki-gateway\r\nx-injected: value",
    ] {
        assert!(ClientVersionInfo::new(product, "0815", "0.1.0", None).is_err());
    }
}
