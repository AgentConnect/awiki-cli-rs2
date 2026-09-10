use super::*;
use crate::agent_network::PROXY_MODE_KEY;

#[test]
fn reinstall_preserves_old_proxy_group_and_mode_when_omitted() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("agent-cli.env");
    std::fs::write(&file, "HTTPS_PROXY=\"http://old.example:443\"\nhttp_proxy='http://old.example:8080'\nNO_PROXY=local.example\nAWIKI_DAEMON_AGENT_PROXY_MODE=inherit\nOPENAI_API_KEY=do-not-import\n").unwrap();
    let mut values = BTreeMap::new();
    preserve_proxy_configuration(&file, &mut values).unwrap();
    assert_eq!(values["HTTPS_PROXY"], "http://old.example:443");
    assert_eq!(values["http_proxy"], "http://old.example:8080");
    assert_eq!(values["NO_PROXY"], "local.example");
    assert_eq!(values[PROXY_MODE_KEY], "inherit");
    assert!(!values.contains_key("OPENAI_API_KEY"));
}

#[test]
fn reinstall_replaces_addresses_as_a_group_and_preserves_exclusions() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("agent-cli.env");
    std::fs::write(&file, "HTTP_PROXY=http://old:1234\nHTTPS_PROXY=http://old:5678\nall_proxy=socks5h://old:9000\nNO_PROXY=direct.example\n").unwrap();
    let mut values =
        BTreeMap::from([("https_proxy".to_string(), "http://[::1]:43561".to_string())]);
    preserve_proxy_configuration(&file, &mut values).unwrap();
    assert!(!values.contains_key("HTTP_PROXY"));
    assert!(!values.contains_key("HTTPS_PROXY"));
    assert!(!values.contains_key("all_proxy"));
    assert_eq!(values["NO_PROXY"], "direct.example");
    assert_eq!(values["https_proxy"], "http://[::1]:43561");
}

#[test]
fn quoted_proxy_credentials_round_trip_without_shell_evaluation() {
    let value = "http://user:p$`\"\\word@proxy.example:65432";
    assert_eq!(
        decode_saved_env_value(&quote_env_file_value(value)).as_deref(),
        Some(value)
    );
    for raw in [
        "$(do-not-run)",
        "`do-not-run`",
        "\"$HOME\"",
        "http://proxy;do-not-run",
    ] {
        assert!(decode_saved_env_value(raw).is_none());
    }
}

#[test]
fn invalid_mode_does_not_overwrite_the_saved_file() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("agent-cli.env");
    std::fs::write(&file, "HTTP_PROXY=http://old:1234\n").unwrap();
    let mut values = BTreeMap::from([(PROXY_MODE_KEY.to_string(), "invalid".to_string())]);
    assert!(preserve_proxy_configuration(&file, &mut values).is_err());
    assert_eq!(
        std::fs::read_to_string(file).unwrap(),
        "HTTP_PROXY=http://old:1234\n"
    );
}
