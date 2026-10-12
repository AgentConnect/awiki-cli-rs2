use super::*;

#[test]
fn package_channel_is_not_sent_as_a_numeric_client_version() {
    for package in ["1.0.58", "1.0.58-singapore.20261012", "1.0.58-beta.1+ci.2"] {
        assert_eq!(
            client_version_from_package(package).unwrap().header_value(),
            format!("awiki-cli/{RELEASE}/1.0.58")
        );
    }
}

#[test]
fn malformed_package_versions_fail_without_a_fallback() {
    for package in ["dev", "01.0.58", "1.0", "1.0.58-", "1.0.58-extra space"] {
        assert!(client_version_from_package(package).is_err(), "{package}");
    }
}
