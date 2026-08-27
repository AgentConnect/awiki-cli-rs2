pub const PRODUCT: &str = "awiki-daemon";
pub const RELEASE: &str = match option_env!("AWIKI_DAEMON_RELEASE") {
    Some(release) => release,
    None => "0815",
};
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn client_version_info() -> im_core::ImResult<im_core::ClientVersionInfo> {
    im_core::ClientVersionInfo::new(PRODUCT, RELEASE, VERSION, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daemon_build_info_has_its_own_product_identity() {
        assert_eq!(PRODUCT, "awiki-daemon");
        assert_eq!(
            RELEASE,
            option_env!("AWIKI_DAEMON_RELEASE").unwrap_or("0815")
        );
        assert_eq!(
            client_version_info().unwrap().header_value(),
            format!("awiki-daemon/{RELEASE}/{VERSION}")
        );
    }
}
