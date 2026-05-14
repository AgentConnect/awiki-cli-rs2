use serde::Serialize;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Serialize)]
pub struct BuildInfo {
    pub version: String,
    pub commit: String,
    pub build_date: String,
    pub go_version: String,
    pub goos: String,
    pub goarch: String,
    pub compiler: String,
    pub cgo_enabled: String,
}

impl BuildInfo {
    pub fn current() -> Self {
        Self {
            version: VERSION.to_string(),
            commit: option_env!("AWIKI_CLI_COMMIT").unwrap_or("dev").to_string(),
            build_date: option_env!("AWIKI_CLI_BUILD_DATE")
                .unwrap_or("dev")
                .to_string(),
            go_version: "rust".to_string(),
            goos: std::env::consts::OS.to_string(),
            goarch: std::env::consts::ARCH.to_string(),
            compiler: "rustc".to_string(),
            cgo_enabled: "false".to_string(),
        }
    }
}
