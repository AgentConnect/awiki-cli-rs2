use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HomeDirUnavailable;

impl fmt::Display for HomeDirUnavailable {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(
            "resolve user home: the operating system did not provide a user profile directory",
        )
    }
}

impl std::error::Error for HomeDirUnavailable {}

/// Resolves the current OS user's profile directory without assuming that
/// Windows defines HOME or that Unix always exports it.
pub fn home_dir() -> Result<PathBuf, HomeDirUnavailable> {
    resolve_home_dir(PROFILE_ENV, |key| std::env::var_os(key), std::env::home_dir)
}

/// Resolves the current user's profile when it is an optional discovery input.
pub fn try_home_dir() -> Option<PathBuf> {
    home_dir().ok()
}

/// Expands the current-user forms supported by AWiki configuration values.
pub fn expand_tilde(home: &Path, value: &str) -> PathBuf {
    if value == "~" {
        return home.to_path_buf();
    }
    if let Some(rest) = value.strip_prefix("~/") {
        return home.join(rest);
    }
    #[cfg(windows)]
    if let Some(rest) = value.strip_prefix("~\\") {
        return home.join(rest);
    }
    PathBuf::from(value)
}

pub fn requires_home_expansion(value: &str) -> bool {
    if value == "~" || value.starts_with("~/") {
        return true;
    }
    #[cfg(windows)]
    if value.starts_with("~\\") {
        return true;
    }
    false
}

fn validate_home_dir(candidate: Option<PathBuf>) -> Result<PathBuf, HomeDirUnavailable> {
    candidate
        .filter(|path| !path.as_os_str().is_empty() && path.is_absolute())
        .ok_or(HomeDirUnavailable)
}

#[cfg(windows)]
const PROFILE_ENV: &str = "USERPROFILE";
#[cfg(not(windows))]
const PROFILE_ENV: &str = "HOME";

fn resolve_home_dir(
    profile_env: &str,
    env_lookup: impl FnOnce(&str) -> Option<OsString>,
    system_fallback: impl FnOnce() -> Option<PathBuf>,
) -> Result<PathBuf, HomeDirUnavailable> {
    let environment_home = env_lookup(profile_env).map(PathBuf::from);
    validate_home_dir(environment_home).or_else(|_| validate_home_dir(system_fallback()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_non_empty_home_without_requiring_it_to_exist() {
        let candidate = absolute_test_home().join("profile with spaces/not-created");
        assert_eq!(
            validate_home_dir(Some(candidate.clone())).unwrap(),
            candidate
        );
    }

    #[test]
    fn rejects_missing_or_empty_home() {
        assert_eq!(validate_home_dir(None), Err(HomeDirUnavailable));
        assert_eq!(
            validate_home_dir(Some(PathBuf::new())),
            Err(HomeDirUnavailable)
        );
        assert_eq!(
            validate_home_dir(Some(PathBuf::from("relative/profile"))),
            Err(HomeDirUnavailable)
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_uses_userprofile_and_never_treats_home_as_the_profile() {
        let resolved = resolve_home_dir(
            "USERPROFILE",
            |key| match key {
                "USERPROFILE" => Some(OsString::from("C:\\Users\\Alice")),
                "HOME" => Some(OsString::from("C:\\wrong-home")),
                _ => None,
            },
            || None,
        )
        .unwrap();

        assert_eq!(resolved, PathBuf::from("C:\\Users\\Alice"));
    }

    #[cfg(not(windows))]
    #[test]
    fn unix_uses_home_and_ignores_userprofile() {
        let resolved = resolve_home_dir(
            "HOME",
            |key| match key {
                "HOME" => Some(OsString::from("/Users/alice")),
                "USERPROFILE" => Some(OsString::from("/wrong-profile")),
                _ => None,
            },
            || None,
        )
        .unwrap();

        assert_eq!(resolved, PathBuf::from("/Users/alice"));
    }

    #[test]
    fn empty_environment_value_uses_the_system_fallback() {
        let fallback = absolute_test_home().join("system profile");
        let resolved = resolve_home_dir(
            "USERPROFILE",
            |_| Some(OsString::new()),
            || Some(fallback.clone()),
        )
        .unwrap();

        assert_eq!(resolved, fallback);
    }

    #[test]
    fn missing_environment_and_system_profile_returns_a_clear_error() {
        assert_eq!(
            resolve_home_dir("USERPROFILE", |_| None, || None),
            Err(HomeDirUnavailable)
        );
        assert_eq!(
            resolve_home_dir("HOME", |_| None, || None),
            Err(HomeDirUnavailable)
        );
    }

    #[test]
    fn expands_only_current_user_tilde_forms() {
        let home = Path::new("/profiles/alice");
        assert_eq!(expand_tilde(home, "~"), home);
        assert_eq!(expand_tilde(home, "~/workspace"), home.join("workspace"));
        #[cfg(windows)]
        assert_eq!(expand_tilde(home, "~\\workspace"), home.join("workspace"));
        #[cfg(not(windows))]
        assert_eq!(
            expand_tilde(home, "~\\workspace"),
            PathBuf::from("~\\workspace")
        );
        assert_eq!(
            expand_tilde(home, "~other/workspace"),
            PathBuf::from("~other/workspace")
        );
        assert_eq!(expand_tilde(home, "/absolute"), PathBuf::from("/absolute"));
    }

    #[test]
    fn detects_only_current_user_tilde_forms() {
        assert!(requires_home_expansion("~"));
        assert!(requires_home_expansion("~/workspace"));
        #[cfg(windows)]
        assert!(requires_home_expansion("~\\workspace"));
        #[cfg(not(windows))]
        assert!(!requires_home_expansion("~\\workspace"));
        assert!(!requires_home_expansion("~other/workspace"));
        assert!(!requires_home_expansion("/absolute"));
    }

    fn absolute_test_home() -> PathBuf {
        #[cfg(windows)]
        {
            PathBuf::from(r"C:\awiki-user-dirs-test")
        }
        #[cfg(not(windows))]
        {
            PathBuf::from("/awiki-user-dirs-test")
        }
    }
}
