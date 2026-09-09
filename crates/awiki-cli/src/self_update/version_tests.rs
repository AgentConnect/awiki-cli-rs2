use super::compare_versions;

#[test]
fn build_metadata_never_affects_release_precedence() {
    for (left, right) in [
        ("1.0.0+build.7", "1.0.0"),
        ("1.0.0+one", "1.0.0+two"),
        ("1.0.0-rc.9+build.7", "1.0.0-rc.9"),
    ] {
        assert_eq!(compare_versions(left, right), Some(0), "{left} vs {right}");
    }
    assert_eq!(
        compare_versions("1.0.0-rc.9+build.7", "1.0.0-rc.10"),
        Some(-1)
    );
    assert_eq!(compare_versions("1.0.0+build.7", "1.0.1"), Some(-1));
}

#[test]
fn prereleases_follow_semver_numeric_and_stable_precedence() {
    let ordered = [
        "2.1.0-alpha",
        "2.1.0-alpha.1",
        "2.1.0-alpha.beta",
        "2.1.0-beta.2",
        "2.1.0-beta.11",
        "2.1.0-rc.7",
        "2.1.0-rc.8",
        "2.1.0",
    ];
    for pair in ordered.windows(2) {
        assert_eq!(compare_versions(pair[0], pair[1]), Some(-1));
        assert_eq!(compare_versions(pair[1], pair[0]), Some(1));
    }
    assert_eq!(
        compare_versions(
            "1.0.0-99999999999999999999999999999",
            "1.0.0-999999999999999999999999999999"
        ),
        Some(-1)
    );
}

#[test]
fn keeps_legacy_prefix_and_short_versions() {
    for value in ["1", "1.0", "v1.0.0", " V1.0.0 "] {
        assert_eq!(compare_versions(value, "1.0.0"), Some(0));
    }
}

#[test]
fn rejects_malformed_versions_instead_of_ordering_them() {
    for value in [
        "",
        "dev",
        "1..0",
        "1.0.0-",
        "1.0.0+",
        "01.0.0",
        "1.0.0-rc.01",
        "1.0.0.1",
        "1.0.0+bad!",
        "1.0.0-rc..1",
        "vv1.0.0",
    ] {
        assert_eq!(compare_versions(value, "1.0.0"), None, "{value}");
    }
}
