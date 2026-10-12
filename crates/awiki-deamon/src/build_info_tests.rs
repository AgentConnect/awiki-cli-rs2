use super::*;

#[test]
fn package_channel_is_not_sent_as_a_numeric_client_version() {
    for package in [
        "0.1.108",
        "0.1.108-singapore.20261012",
        "0.1.108-beta.1+ci.2",
    ] {
        assert_eq!(
            client_version_from_package(package).unwrap().header_value(),
            format!("awiki-daemon/{RELEASE}/0.1.108")
        );
    }
    assert_eq!(client_version_info().unwrap().product(), PRODUCT);
}

#[test]
fn malformed_package_versions_fail_without_a_fallback() {
    for package in ["dev", "01.1.108", "0.1", "0.1.108-", "0.1.108-extra space"] {
        assert!(client_version_from_package(package).is_err(), "{package}");
    }
}
