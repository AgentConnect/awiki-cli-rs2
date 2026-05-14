mod cache;
mod version;

use crate::buildinfo;
use crate::config::Resolved;

const DEFAULT_METADATA_CACHE_TTL_SECONDS: i64 = 43_200;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Metadata {
    pub latest_version: String,
    pub min_supported_version: String,
    pub source: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Decision {
    pub current_version: String,
    pub latest_version: String,
    pub min_supported_version: String,
    pub metadata_source: String,
    pub strict_disabled: bool,
    pub dev_build: bool,
    pub has_newer_version: bool,
    pub blocked: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CheckOutcome {
    pub decision: Decision,
    pub error: Option<String>,
}

pub fn check(resolved: &Resolved) -> CheckOutcome {
    check_inner(resolved, false)
}

pub fn check_fresh(resolved: &Resolved) -> CheckOutcome {
    check_inner(resolved, true)
}

fn check_inner(resolved: &Resolved, prefer_fresh: bool) -> CheckOutcome {
    let current_version = current_version();
    let dev_build = version::is_dev_version(&current_version);
    let strict_disabled = strict_disabled(resolved);
    let ttl_seconds = metadata_cache_ttl_seconds(resolved);
    let mut decision = Decision {
        current_version,
        strict_disabled,
        dev_build,
        ..Decision::default()
    };

    let metadata = match cache::load_metadata(
        &resolved.paths.cache_dir,
        ttl_seconds,
        prefer_fresh,
        update_cache_only_enabled(),
    ) {
        Ok(metadata) => metadata,
        Err(err) => {
            return CheckOutcome {
                decision,
                error: Some(err),
            };
        }
    };

    decision.latest_version = metadata.latest_version;
    decision.min_supported_version = metadata.min_supported_version;
    decision.metadata_source = metadata.source;

    if version::compare_versions(&decision.latest_version, &decision.current_version)
        .is_some_and(|ordering| ordering > 0)
    {
        decision.has_newer_version = true;
    }

    if !decision.dev_build
        && !decision.strict_disabled
        && version::compare_versions(&decision.current_version, &decision.min_supported_version)
            .is_some_and(|ordering| ordering < 0)
    {
        decision.blocked = true;
    }

    CheckOutcome {
        decision,
        error: None,
    }
}

fn current_version() -> String {
    let current = buildinfo::VERSION.trim();
    if current.is_empty() {
        "dev".to_string()
    } else {
        current.to_string()
    }
}

fn strict_disabled(resolved: &Resolved) -> bool {
    let mut disabled = resolved.update_disable_strict_version;
    if let Ok(raw) = std::env::var("AWIKI_CLI_DISABLE_STRICT_VERSION") {
        if !raw.trim().is_empty() {
            disabled = parse_bool(&raw);
        }
    }
    disabled
}

fn metadata_cache_ttl_seconds(resolved: &Resolved) -> i64 {
    let mut ttl = if resolved.update_metadata_cache_ttl_seconds > 0 {
        resolved.update_metadata_cache_ttl_seconds
    } else {
        DEFAULT_METADATA_CACHE_TTL_SECONDS
    };
    if let Ok(raw) = std::env::var("AWIKI_CLI_UPDATE_CACHE_TTL") {
        if let Ok(parsed) = raw.trim().parse::<i64>() {
            if parsed > 0 {
                ttl = parsed;
            }
        }
    }
    ttl
}

fn update_cache_only_enabled() -> bool {
    std::env::var("AWIKI_CLI_UPDATE_CACHE_ONLY")
        .map(|value| parse_bool(&value))
        .unwrap_or(false)
}

fn parse_bool(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

#[cfg(test)]
mod tests {
    use super::version;

    #[test]
    fn compare_versions_handles_numeric_prerelease_segments() {
        assert!(version::compare_versions("0.0.1-beta.9", "0.0.1-beta.10").unwrap() < 0);
        assert!(version::compare_versions("0.0.1-beta.10", "0.0.1-beta.9").unwrap() > 0);
    }

    #[test]
    fn dev_versions_match_go_policy() {
        assert!(version::is_dev_version(""));
        assert!(version::is_dev_version("dev"));
        assert!(version::is_dev_version("1.0.0-dev.1"));
        assert!(version::is_dev_version("0.0.0-local"));
        assert!(!version::is_dev_version("1.0.0"));
    }
}
