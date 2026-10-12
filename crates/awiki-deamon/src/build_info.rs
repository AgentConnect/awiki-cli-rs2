pub const PRODUCT: &str = "awiki-daemon";
pub const RELEASE: &str = match option_env!("AWIKI_DAEMON_RELEASE") {
    Some(release) => release,
    None => "0815",
};
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn client_version_info() -> im_core::ImResult<im_core::ClientVersionInfo> {
    client_version_info_for_package(VERSION)
}

fn client_version_info_for_package(
    package_version: &str,
) -> im_core::ImResult<im_core::ClientVersionInfo> {
    // Package labels belong to download/update identity. The product protocol
    // accepts only a numeric compatibility version; validate SemVer before
    // extracting it so malformed labels cannot be silently truncated.
    let package = semver::Version::parse(package_version).map_err(|_| {
        im_core::ImError::invalid_input(
            Some("daemon.package_version".to_owned()),
            "daemon package version must be valid SemVer",
        )
    })?;
    let protocol_version = format!("{}.{}.{}", package.major, package.minor, package.patch);
    im_core::ClientVersionInfo::new(PRODUCT, RELEASE, protocol_version, None)
}

#[cfg(test)]
#[path = "build_info_tests.rs"]
mod tests;
