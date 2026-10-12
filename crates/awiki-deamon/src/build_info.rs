pub const PRODUCT: &str = "awiki-daemon";
pub const RELEASE: &str = match option_env!("AWIKI_DAEMON_RELEASE") {
    Some(release) => release,
    None => "0815",
};
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn client_version_info() -> im_core::ImResult<im_core::ClientVersionInfo> {
    client_version_from_package(VERSION)
}

fn client_version_from_package(package: &str) -> im_core::ImResult<im_core::ClientVersionInfo> {
    let version = semver::Version::parse(package).map_err(|_| {
        im_core::ImError::invalid_input(
            Some("package_version".to_owned()),
            "package version must be valid SemVer",
        )
    })?;
    // Package channels are not part of the numeric product-admission version.
    let numeric = format!("{}.{}.{}", version.major, version.minor, version.patch);
    im_core::ClientVersionInfo::new(PRODUCT, RELEASE, numeric, None)
}

#[cfg(test)]
#[path = "build_info_tests.rs"]
mod tests;
