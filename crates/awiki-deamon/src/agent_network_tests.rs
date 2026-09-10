use super::*;

fn env(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
    entries
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}
fn system(host: &str, port: u32) -> String {
    format!("<dictionary> {{\n HTTPEnable : 1\n HTTPProxy : {host}\n HTTPPort : {port}\n}}")
}

#[test]
fn reads_actual_http_https_socks_addresses_ports_and_exclusions() {
    let values = parse_system_proxy("<dictionary> {\n HTTPEnable : 1\n HTTPProxy : proxy.example\n HTTPPort : 3129\n HTTPSEnable : 1\n HTTPSProxy : 192.0.2.5\n HTTPSPort : 65535\n SOCKSEnable : 1\n SOCKSProxy : ::1\n SOCKSPort : 1085\n ExceptionsList : <array> {\n 0 : *.local\n 1 : 10.0.0.0/8\n }\n}").unwrap();
    assert_eq!(values["HTTP_PROXY"], "http://proxy.example:3129");
    assert_eq!(values["HTTPS_PROXY"], "http://192.0.2.5:65535");
    assert_eq!(values["ALL_PROXY"], "socks5h://[::1]:1085");
    assert_eq!(values["NO_PROXY"], ".local,10.0.0.0/8");
    for (host, port) in [("127.0.0.2", 80), ("::1", 65234), ("[2001:db8::4]", 443)] {
        assert!(parse_system_proxy(&system(host, port)).is_ok());
    }
}

#[test]
fn localhost_can_listen_on_ipv6_only_without_probing_other_ports() {
    let mut addresses = Vec::new();
    let resolved = resolve(
        &env(&[]),
        || parse_system_proxy(&system("localhost", 42317)),
        |address, budget| {
            addresses.push(address);
            assert!(budget <= LOCAL_CHECK_BUDGET);
            address.is_ipv6()
        },
    );
    assert_eq!(resolved.values["HTTP_PROXY"], "http://localhost:42317");
    assert_eq!(
        addresses,
        [
            "127.0.0.1:42317".parse().unwrap(),
            "[::1]:42317".parse().unwrap()
        ]
    );
}

#[test]
fn unrepresentable_system_exclusions_preserve_routing_and_disabled_settings_are_quiet() {
    let raw = system("proxy.example", 34127).replace(
        "<dictionary> {",
        "<dictionary> {\n ExcludeSimpleHostnames : 1",
    );
    assert!(parse_system_proxy(&raw).is_err());
    assert!(
        parse_system_proxy(&raw.replace("HTTPEnable : 1", "HTTPEnable : 0"))
            .unwrap()
            .is_empty()
    );
    let raw = system("proxy.example", 34127).replace(
        "<dictionary> {",
        "<dictionary> {\n ExceptionsList : <array> {\n 0 : host?\n }",
    );
    let resolved = resolve(&env(&[]), || parse_system_proxy(&raw), |_, _| panic!());
    assert!(resolved.values.is_empty());
    assert!(resolved.detail.contains("exclusion"));
}

#[test]
fn disabled_pac_malformed_or_unrecognized_settings_do_not_inject_addresses() {
    assert!(parse_system_proxy(
        &system("localhost", 43210).replace("HTTPEnable : 1", "HTTPEnable : 0")
    )
    .unwrap()
    .is_empty());
    assert!(parse_system_proxy("<dictionary> {\n ProxyAutoConfigEnable : 1\n}").is_err());
    assert!(parse_system_proxy("<dictionary> {\n __SCOPED__ : <dictionary> {\n HTTPEnable : 1\n HTTPPort : 3456\n HTTPProxy : localhost\n }\n}").unwrap().is_empty());
    for port in [0, 65536, u32::MAX] {
        assert!(parse_system_proxy(&system("localhost", port)).is_err());
    }
    for host in [
        "",
        "bad/host",
        "user@host",
        "host:bad",
        "host:80",
        "foo bar",
    ] {
        assert!(parse_system_proxy(&system(host, 4000)).is_err(), "{host}");
    }
}

#[test]
fn explicit_group_wins_without_system_lookup_and_keeps_direct_addresses() {
    let source = env(&[
        ("https_proxy", "http://remote.example:6543"),
        ("NO_PROXY", "private.example,10.0.0.0/8"),
        ("no_proxy", "callback.example"),
    ]);
    let resolved = resolve(
        &source,
        || panic!("explicit proxy must not query system"),
        |_, _| panic!("remote proxies must not be probed"),
    );
    assert_eq!(resolved.values["https_proxy"], source["https_proxy"]);
    assert!(!resolved.values.contains_key("HTTP_PROXY"));
    for host in [
        "private.example",
        "10.0.0.0/8",
        "callback.example",
        "localhost",
        "127.0.0.1",
        "::1",
    ] {
        assert!(resolved.values["NO_PROXY"].split(',').any(|v| v == host));
    }
}

#[test]
fn no_proxy_and_inherit_preserve_original_routing_without_discovery() {
    let result = resolve(
        &env(&[(PROXY_MODE_KEY, "inherit")]),
        || panic!("inherit must not discover"),
        |_, _| panic!(),
    );
    assert!(result.values.is_empty());
    assert!(resolve(&env(&[]), || Ok(BTreeMap::new()), |_, _| panic!())
        .values
        .is_empty());
    let empty_explicit = resolve(
        &env(&[("HTTP_PROXY", "")]),
        || panic!("empty explicit group must not discover"),
        |_, _| panic!(),
    );
    assert_eq!(empty_explicit.values, env(&[("HTTP_PROXY", "")]));
}

#[test]
fn failed_automatic_local_proxy_is_bounded_and_explicit_is_retained() {
    let mut attempts = 0;
    let automatic = resolve(
        &env(&[]),
        || parse_system_proxy(&system("::1", 45678)),
        |address, budget| {
            attempts += 1;
            assert_eq!(address, "[::1]:45678".parse().unwrap());
            assert!(budget <= LOCAL_CHECK_BUDGET);
            false
        },
    );
    assert_eq!(attempts, 1);
    assert!(!automatic.values.contains_key("HTTP_PROXY"));
    assert!(automatic.detail.contains("skipped"));
    let explicit = resolve(
        &env(&[("ALL_PROXY", "socks5h://127.0.0.1:32109")]),
        || panic!(),
        |_, _| false,
    );
    assert_eq!(explicit.values["ALL_PROXY"], "socks5h://127.0.0.1:32109");
    assert!(explicit.detail.contains("configuration retained"));
}

#[test]
fn actual_local_listener_and_system_changes_are_seen_by_new_children_only() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let first = resolve(
        &env(&[]),
        || parse_system_proxy(&system("127.0.0.1", u32::from(port))),
        local_proxy_listening,
    );
    assert_eq!(
        first.values["HTTP_PROXY"],
        format!("http://127.0.0.1:{port}")
    );
    let second = resolve(
        &env(&[]),
        || parse_system_proxy(&system("next.example", 23456)),
        local_proxy_listening,
    );
    assert_eq!(second.values["HTTP_PROXY"], "http://next.example:23456");
    assert_eq!(
        first.values["HTTP_PROXY"],
        format!("http://127.0.0.1:{port}")
    );
    let before: BTreeMap<_, _> = std::env::vars_os().collect();
    let mut command = Command::new("/usr/bin/env");
    command.env_clear();
    first.apply(&mut command);
    let output = command.output().unwrap();
    assert!(String::from_utf8(output.stdout)
        .unwrap()
        .contains(&format!("HTTP_PROXY=http://127.0.0.1:{port}")));
    assert_eq!(before, std::env::vars_os().collect());
}
