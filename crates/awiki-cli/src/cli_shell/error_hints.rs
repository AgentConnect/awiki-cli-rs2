use std::fmt;

pub(super) const WINDOWS_DIR_SYNC_COMPATIBILITY_HINT: &str = "This looks like a Windows workspace directory-sync compatibility failure inside awiki-cli rather than a normal write-permission problem. Upgrade to the latest awiki-cli build with the Windows durable-write fix; running as Administrator usually should not be necessary.";

pub(super) fn refine_workspace_write_hint(
    err: impl fmt::Display,
    fallback: impl Into<String>,
) -> String {
    if is_windows_dir_sync_compatibility_error(err) {
        WINDOWS_DIR_SYNC_COMPATIBILITY_HINT.to_string()
    } else {
        fallback.into()
    }
}

fn is_windows_dir_sync_compatibility_error(err: impl fmt::Display) -> bool {
    let message = err.to_string().to_ascii_lowercase();
    if !message.contains("access is denied") {
        return false;
    }
    ["sync config dir", "sync route registry dir", "sync dir"]
        .iter()
        .any(|marker| message.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_dir_sync_compatibility_error_matches_known_patterns() {
        for err in [
            r"write config yaml: sync config dir: sync C:\Users\liuzhuocheng\.awiki-cli: Access is denied.",
            r"write route registry: sync route registry dir: sync C:\Users\liuzhuocheng\.awiki-cli\runtime: Access is denied.",
            r"upgrade workspace: sync dir: sync C:\Users\liuzhuocheng\.awiki-cli: Access is denied.",
        ] {
            assert!(
                is_windows_dir_sync_compatibility_error(err),
                "{err:?} should match the Windows directory-sync compatibility pattern"
            );
        }
    }

    #[test]
    fn windows_dir_sync_compatibility_error_ignores_normal_permission_errors() {
        for err in [
            r"create config dir: mkdir C:\Users\liuzhuocheng\.awiki-cli: Access is denied.",
            "write config yaml: open /tmp/config.yaml: permission denied",
            "write route registry: parse route registry: invalid character 'x'",
        ] {
            assert!(
                !is_windows_dir_sync_compatibility_error(err),
                "{err:?} should not match the Windows directory-sync compatibility pattern"
            );
        }
    }

    #[test]
    fn refine_workspace_write_hint_matches_go_contract() {
        let refined = refine_workspace_write_hint(
            r"upgrade workspace: sync dir: sync C:\Users\liuzhuocheng\.awiki-cli: Access is denied.",
            "fallback hint",
        );
        assert_eq!(refined, WINDOWS_DIR_SYNC_COMPATIBILITY_HINT);

        let fallback = refine_workspace_write_hint(
            r"create config dir: mkdir C:\Users\liuzhuocheng\.awiki-cli: Access is denied.",
            "fallback hint",
        );
        assert_eq!(fallback, "fallback hint");
    }
}
