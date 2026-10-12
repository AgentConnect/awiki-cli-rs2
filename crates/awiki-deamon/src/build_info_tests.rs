use super::*;

#[test]
fn daemon_build_info_has_its_own_product_identity() {
    assert_eq!(PRODUCT, "awiki-daemon");
    assert_eq!(
        RELEASE,
        option_env!("AWIKI_DAEMON_RELEASE").unwrap_or("0815")
    );
    let package = semver::Version::parse(VERSION).unwrap();
    assert_eq!(
        client_version_info().unwrap().header_value(),
        format!(
            "awiki-daemon/{RELEASE}/{}.{}.{}",
            package.major, package.minor, package.patch
        )
    );
}

#[test]
fn test_package_labels_do_not_enter_the_numeric_protocol_version() {
    for package_version in [
        "0.1.107",
        "0.1.107-singapore.20261012",
        "0.1.107+source.abcdef",
        "0.1.107-singapore.20261012+source.abcdef",
    ] {
        let info = client_version_info_for_package(package_version).unwrap();
        assert_eq!(
            info.header_value(),
            format!("awiki-daemon/{RELEASE}/0.1.107")
        );
        assert_eq!(info.build(), None);
    }
}

#[test]
fn invalid_package_versions_are_not_silently_normalized() {
    for package_version in [
        "dev",
        "01.1.107",
        "0.1",
        "0.1.107.1",
        "0.1.107-",
        "0.1.107+",
        " 0.1.107",
    ] {
        assert!(
            client_version_info_for_package(package_version).is_err(),
            "{package_version}"
        );
    }
}
