use serde::Serialize;

pub const VERSION: &str = match option_env!("AWIKI_CLI_VERSION") {
    Some(version) => version,
    None => "dev",
};

pub const COMMIT: &str = match option_env!("AWIKI_CLI_COMMIT") {
    Some(commit) => commit,
    None => "unknown",
};

pub const BUILD_DATE: &str = match option_env!("AWIKI_CLI_BUILD_DATE") {
    Some(build_date) => build_date,
    None => "unknown",
};

pub const CGO_ENABLED: &str = match option_env!("AWIKI_CLI_CGO_ENABLED") {
    Some(cgo_enabled) => cgo_enabled,
    None => "unknown",
};

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
        Self::from_metadata(
            VERSION,
            COMMIT,
            BUILD_DATE,
            CGO_ENABLED,
            RuntimeInfo::current(),
        )
    }

    fn from_metadata(
        version: &str,
        commit: &str,
        build_date: &str,
        cgo_enabled: &str,
        runtime: RuntimeInfo,
    ) -> Self {
        Self {
            version: version.to_string(),
            commit: commit.to_string(),
            build_date: build_date.to_string(),
            go_version: runtime.go_version,
            goos: runtime.goos,
            goarch: runtime.goarch,
            compiler: runtime.compiler,
            cgo_enabled: cgo_enabled.to_string(),
        }
    }
}

struct RuntimeInfo {
    go_version: String,
    goos: String,
    goarch: String,
    compiler: String,
}

impl RuntimeInfo {
    fn current() -> Self {
        Self {
            go_version: "rust".to_string(),
            goos: goos_from_rust(std::env::consts::OS).to_string(),
            goarch: goarch_from_rust(std::env::consts::ARCH).to_string(),
            compiler: "rustc".to_string(),
        }
    }
}

fn goos_from_rust(os: &str) -> &str {
    match os {
        "macos" => "darwin",
        "ios" => "ios",
        "linux" => "linux",
        "windows" => "windows",
        "android" => "android",
        "freebsd" => "freebsd",
        "openbsd" => "openbsd",
        "netbsd" => "netbsd",
        "dragonfly" => "dragonfly",
        "solaris" => "solaris",
        "illumos" => "illumos",
        value => value,
    }
}

fn goarch_from_rust(arch: &str) -> &str {
    match arch {
        "x86" => "386",
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        "powerpc64" => "ppc64",
        "powerpc64le" => "ppc64le",
        "riscv64" => "riscv64",
        "s390x" => "s390x",
        "arm" => "arm",
        "wasm32" => "wasm",
        value => value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_metadata_matches_compile_time_env_or_go_defaults() {
        let info = BuildInfo::current();
        assert_eq!(
            info.version,
            option_env!("AWIKI_CLI_VERSION").unwrap_or("dev")
        );
        assert_eq!(
            info.commit,
            option_env!("AWIKI_CLI_COMMIT").unwrap_or("unknown")
        );
        assert_eq!(
            info.build_date,
            option_env!("AWIKI_CLI_BUILD_DATE").unwrap_or("unknown")
        );
        assert_eq!(
            info.cgo_enabled,
            option_env!("AWIKI_CLI_CGO_ENABLED").unwrap_or("unknown")
        );
        assert_eq!(info.compiler, "rustc");
        assert!(!info.go_version.is_empty());
        assert!(!info.goos.is_empty());
        assert!(!info.goarch.is_empty());
    }

    #[test]
    fn default_metadata_values_match_go_buildinfo_contract() {
        let runtime = RuntimeInfo {
            go_version: "rust-test".to_string(),
            goos: "linux".to_string(),
            goarch: "amd64".to_string(),
            compiler: "rustc".to_string(),
        };
        let info = BuildInfo::from_metadata("dev", "unknown", "unknown", "unknown", runtime);

        assert_eq!(info.version, "dev");
        assert_eq!(info.commit, "unknown");
        assert_eq!(info.build_date, "unknown");
        assert_eq!(info.cgo_enabled, "unknown");
    }

    #[test]
    fn injected_metadata_is_copied_into_independent_snapshot() {
        for (version, commit, build_date, cgo_enabled) in [
            ("1.2.3", "abc1234", "2026-04-18T09:00:00Z", "0"),
            ("dev-main", "unknown", "unknown", "true"),
        ] {
            let runtime = RuntimeInfo {
                go_version: "rust-test".to_string(),
                goos: "linux".to_string(),
                goarch: "amd64".to_string(),
                compiler: "rustc".to_string(),
            };
            let info = BuildInfo::from_metadata(version, commit, build_date, cgo_enabled, runtime);

            assert_eq!(info.version, version);
            assert_eq!(info.commit, commit);
            assert_eq!(info.build_date, build_date);
            assert_eq!(info.cgo_enabled, cgo_enabled);
            assert_eq!(info.go_version, "rust-test");
            assert_eq!(info.goos, "linux");
            assert_eq!(info.goarch, "amd64");
            assert_eq!(info.compiler, "rustc");

            let mutated_source = "after".to_string();
            assert_eq!(info.version, version);
            assert_eq!(mutated_source, "after");
        }
    }

    #[test]
    fn rust_target_names_are_rendered_as_go_runtime_names() {
        assert_eq!(goos_from_rust("macos"), "darwin");
        assert_eq!(goos_from_rust("linux"), "linux");
        assert_eq!(goos_from_rust("windows"), "windows");
        assert_eq!(goos_from_rust("custom-os"), "custom-os");

        assert_eq!(goarch_from_rust("x86"), "386");
        assert_eq!(goarch_from_rust("x86_64"), "amd64");
        assert_eq!(goarch_from_rust("aarch64"), "arm64");
        assert_eq!(goarch_from_rust("custom-arch"), "custom-arch");
    }
}
