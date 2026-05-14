use awiki_cli::transportcfg::{self, Profile};
use std::sync::Mutex;
use std::time::Duration;

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn transport_resolve_defaults_match_go_contract() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    unset_transport_env();

    let config = transportcfg::resolve();
    assert_eq!(
        config.bridge_health_probe_timeout,
        Duration::from_millis(750)
    );
    assert_eq!(config.bridge_dial_timeout, Duration::from_secs(1));
    assert_eq!(config.bridge_write_timeout, Duration::from_secs(1));
    assert_eq!(config.bridge_read_timeout, Duration::from_secs(3));
    assert_eq!(config.http_dial_timeout, Duration::from_secs(8));
    assert_eq!(config.http_tls_handshake_timeout, Duration::from_secs(8));
    assert_eq!(config.http_response_header_timeout, Duration::from_secs(30));
    assert_eq!(
        config.timeout_for_profile(Profile::AuthRefresh),
        Duration::from_secs(20)
    );
    assert_eq!(
        config.timeout_for_profile(Profile::RpcDefault),
        Duration::from_secs(25)
    );
    let mut empty_profile_config = config.clone();
    empty_profile_config.profile_timeouts.clear();
    assert_eq!(
        empty_profile_config.timeout_for_profile(Profile::RpcDefault),
        Duration::ZERO,
        "Go returns the zero duration if the profile map is empty"
    );
    assert_eq!(
        config.timeout_for_profile(Profile::RpcReadHeavy),
        Duration::from_secs(35)
    );
}

#[test]
fn transport_resolve_honors_env_overrides_like_go() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    unset_transport_env();
    std::env::set_var("AWIKI_CLI_TIMEOUT_HTTP_DIAL", "+11s");
    std::env::set_var("AWIKI_CLI_TIMEOUT_HTTP_RESPONSE_HEADER", "42s");
    std::env::set_var("AWIKI_CLI_TIMEOUT_PROFILE_RPC_DEFAULT", "31s");
    std::env::set_var("AWIKI_CLI_TIMEOUT_PROFILE_RPC_READ_HEAVY", "47000");

    let config = transportcfg::resolve();
    assert_eq!(config.http_dial_timeout, Duration::from_secs(11));
    assert_eq!(config.http_response_header_timeout, Duration::from_secs(42));
    assert_eq!(
        config.timeout_for_profile(Profile::RpcDefault),
        Duration::from_secs(31)
    );
    assert_eq!(
        config.timeout_for_profile(Profile::RpcReadHeavy),
        Duration::from_secs(47)
    );
}

#[test]
fn transport_duration_and_int_parsers_keep_go_fallback_rules() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    unset_transport_env();
    std::env::set_var("AWIKI_CLI_TIMEOUT_HTTP_DIAL", "0");
    std::env::set_var("AWIKI_CLI_TIMEOUT_HTTP_KEEPALIVE", "+.5s");
    std::env::set_var("AWIKI_CLI_TIMEOUT_HTTP_IDLE_CONN", "2m 30s");
    std::env::set_var(
        "AWIKI_CLI_TIMEOUT_HTTP_TLS_HANDSHAKE",
        "9223372036854775808ns",
    );
    std::env::set_var("AWIKI_CLI_HTTP_MAX_IDLE_CONNS", "-1");
    std::env::set_var("AWIKI_CLI_HTTP_MAX_IDLE_CONNS_PER_HOST", "12");

    let config = transportcfg::resolve();
    assert_eq!(config.http_dial_timeout, Duration::from_secs(8));
    assert_eq!(
        config.http_keep_alive,
        Duration::from_millis(500),
        "Go time.ParseDuration accepts positive signed fractional durations"
    );
    assert_eq!(
        config.http_idle_conn_timeout,
        Duration::from_secs(90),
        "Go time.ParseDuration rejects internal spaces"
    );
    assert_eq!(
        config.http_tls_handshake_timeout,
        Duration::from_secs(8),
        "Go time.ParseDuration rejects values above int64 nanoseconds"
    );
    assert_eq!(config.http_max_idle_conns, 32);
    assert_eq!(config.http_max_idle_conns_per_host, 12);
}

#[test]
fn transport_timeout_for_profile_name_falls_back_to_rpc_default() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    unset_transport_env();
    std::env::set_var("AWIKI_CLI_TIMEOUT_PROFILE_RPC_DEFAULT", "31s");
    std::env::set_var("AWIKI_CLI_TIMEOUT_PROFILE_AUTH_REFRESH", "0");

    let config = transportcfg::resolve();
    assert_eq!(
        transportcfg::timeout_for_profile_name(&config, "rpc_default"),
        Duration::from_secs(31)
    );
    assert_eq!(
        transportcfg::timeout_for_profile_name(&config, "unknown"),
        Duration::from_secs(31)
    );
    assert_eq!(
        transportcfg::timeout_for_profile_name(&config, "auth_refresh"),
        Duration::from_secs(20),
        "invalid auth_refresh env falls back to the default before profile lookup"
    );
}

fn unset_transport_env() {
    for key in transportcfg::TRANSPORT_ENV_KEYS {
        std::env::remove_var(key);
    }
}
