//! Compile-time update routing for the isolated Singapore test batch.
use std::path::PathBuf;

pub(super) fn cache_dir(path: Option<PathBuf>) -> Option<PathBuf> {
    if option_env!("AWIKI_TEST_SOURCE_BUILD").is_some() {
        path.map(|path| path.join("singapore-release-20261012"))
    } else {
        path
    }
}

pub(super) fn policy_url(origin: &str) -> String {
    let path = if option_env!("AWIKI_TEST_SOURCE_BUILD").is_some() {
        "/downloads/singapore-release-20261012/cli-policy.json"
    } else {
        "/user-service/v1/server-info?client_platform=cli"
    };
    format!("{}{path}", origin.trim_end_matches('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routing_preserves_selected_tenant() {
        let a = policy_url("https://anpclaw.com/");
        let b = policy_url("https://customer.example");
        assert!(a.starts_with("https://anpclaw.com/"));
        assert!(b.starts_with("https://customer.example/"));
        if option_env!("AWIKI_TEST_SOURCE_BUILD").is_some() {
            assert_eq!(
                a,
                "https://anpclaw.com/downloads/singapore-release-20261012/cli-policy.json"
            );
        } else {
            assert_eq!(
                a,
                "https://anpclaw.com/user-service/v1/server-info?client_platform=cli"
            );
        }
    }

    #[test]
    fn test_cache_never_reads_existing_formal_cache() {
        let root = PathBuf::from("existing-tenant-cache");
        let actual = cache_dir(Some(root.clone())).unwrap();
        if option_env!("AWIKI_TEST_SOURCE_BUILD").is_some() {
            assert_ne!(actual, root);
            assert_eq!(actual.parent(), Some(root.as_path()));
        } else {
            assert_eq!(actual, root);
        }
        assert_eq!(cache_dir(None), None);
    }
}
