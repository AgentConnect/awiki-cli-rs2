use std::collections::BTreeMap;
use std::time::Duration;

pub mod http;
pub use http::{
    new_http_client, new_http_client_with_proxy_env, HttpClient, HttpClientError, HttpRequest,
    HttpResponse,
};

const MAX_GO_DURATION_NANOS: u128 = i64::MAX as u128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Profile {
    BridgeFastPath,
    HealthProbe,
    AuthRefresh,
    RpcDefault,
    RpcReadHeavy,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub bridge_health_probe_timeout: Duration,
    pub bridge_dial_timeout: Duration,
    pub bridge_write_timeout: Duration,
    pub bridge_read_timeout: Duration,
    pub http_dial_timeout: Duration,
    pub http_keep_alive: Duration,
    pub http_tls_handshake_timeout: Duration,
    pub http_response_header_timeout: Duration,
    pub http_idle_conn_timeout: Duration,
    pub http_max_idle_conns: i64,
    pub http_max_idle_conns_per_host: i64,
    pub profile_timeouts: BTreeMap<Profile, Duration>,
}

const DEFAULT_BRIDGE_HEALTH_PROBE_TIMEOUT_MS: u64 = 750;
const DEFAULT_BRIDGE_DIAL_TIMEOUT_MS: u64 = 1_000;
const DEFAULT_BRIDGE_WRITE_TIMEOUT_MS: u64 = 1_000;
const DEFAULT_BRIDGE_READ_TIMEOUT_MS: u64 = 3_000;

const DEFAULT_HTTP_DIAL_TIMEOUT_MS: u64 = 8_000;
const DEFAULT_HTTP_KEEP_ALIVE_MS: u64 = 30_000;
const DEFAULT_HTTP_TLS_HANDSHAKE_TIMEOUT_MS: u64 = 8_000;
const DEFAULT_HTTP_RESPONSE_HEADER_TIMEOUT_MS: u64 = 30_000;
const DEFAULT_HTTP_IDLE_CONN_TIMEOUT_MS: u64 = 90_000;
const DEFAULT_HTTP_MAX_IDLE_CONNS: i64 = 32;
const DEFAULT_HTTP_MAX_IDLE_CONNS_PER_HOST: i64 = 8;

const DEFAULT_PROFILE_BRIDGE_FAST_PATH_MS: u64 = 1_500;
const DEFAULT_PROFILE_HEALTH_PROBE_MS: u64 = 750;
const DEFAULT_PROFILE_AUTH_REFRESH_MS: u64 = 20_000;
const DEFAULT_PROFILE_RPC_DEFAULT_MS: u64 = 25_000;
const DEFAULT_PROFILE_RPC_READ_HEAVY_MS: u64 = 35_000;

pub const TRANSPORT_ENV_KEYS: &[&str] = &[
    "AWIKI_CLI_TIMEOUT_BRIDGE_HEALTH_PROBE",
    "AWIKI_CLI_TIMEOUT_BRIDGE_DIAL",
    "AWIKI_CLI_TIMEOUT_BRIDGE_WRITE",
    "AWIKI_CLI_TIMEOUT_BRIDGE_READ",
    "AWIKI_CLI_TIMEOUT_HTTP_DIAL",
    "AWIKI_CLI_TIMEOUT_HTTP_KEEPALIVE",
    "AWIKI_CLI_TIMEOUT_HTTP_TLS_HANDSHAKE",
    "AWIKI_CLI_TIMEOUT_HTTP_RESPONSE_HEADER",
    "AWIKI_CLI_TIMEOUT_HTTP_IDLE_CONN",
    "AWIKI_CLI_HTTP_MAX_IDLE_CONNS",
    "AWIKI_CLI_HTTP_MAX_IDLE_CONNS_PER_HOST",
    "AWIKI_CLI_TIMEOUT_PROFILE_BRIDGE_FAST_PATH",
    "AWIKI_CLI_TIMEOUT_PROFILE_HEALTH_PROBE",
    "AWIKI_CLI_TIMEOUT_PROFILE_AUTH_REFRESH",
    "AWIKI_CLI_TIMEOUT_PROFILE_RPC_DEFAULT",
    "AWIKI_CLI_TIMEOUT_PROFILE_RPC_READ_HEAVY",
];

impl Profile {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BridgeFastPath => "bridge_fast_path",
            Self::HealthProbe => "health_probe",
            Self::AuthRefresh => "auth_refresh",
            Self::RpcDefault => "rpc_default",
            Self::RpcReadHeavy => "rpc_read_heavy",
        }
    }
}

pub fn resolve() -> Config {
    let mut profile_timeouts = BTreeMap::new();
    profile_timeouts.insert(
        Profile::BridgeFastPath,
        duration_from_env(
            "AWIKI_CLI_TIMEOUT_PROFILE_BRIDGE_FAST_PATH",
            duration_ms(DEFAULT_PROFILE_BRIDGE_FAST_PATH_MS),
        ),
    );
    profile_timeouts.insert(
        Profile::HealthProbe,
        duration_from_env(
            "AWIKI_CLI_TIMEOUT_PROFILE_HEALTH_PROBE",
            duration_ms(DEFAULT_PROFILE_HEALTH_PROBE_MS),
        ),
    );
    profile_timeouts.insert(
        Profile::AuthRefresh,
        duration_from_env(
            "AWIKI_CLI_TIMEOUT_PROFILE_AUTH_REFRESH",
            duration_ms(DEFAULT_PROFILE_AUTH_REFRESH_MS),
        ),
    );
    profile_timeouts.insert(
        Profile::RpcDefault,
        duration_from_env(
            "AWIKI_CLI_TIMEOUT_PROFILE_RPC_DEFAULT",
            duration_ms(DEFAULT_PROFILE_RPC_DEFAULT_MS),
        ),
    );
    profile_timeouts.insert(
        Profile::RpcReadHeavy,
        duration_from_env(
            "AWIKI_CLI_TIMEOUT_PROFILE_RPC_READ_HEAVY",
            duration_ms(DEFAULT_PROFILE_RPC_READ_HEAVY_MS),
        ),
    );

    Config {
        bridge_health_probe_timeout: duration_from_env(
            "AWIKI_CLI_TIMEOUT_BRIDGE_HEALTH_PROBE",
            duration_ms(DEFAULT_BRIDGE_HEALTH_PROBE_TIMEOUT_MS),
        ),
        bridge_dial_timeout: duration_from_env(
            "AWIKI_CLI_TIMEOUT_BRIDGE_DIAL",
            duration_ms(DEFAULT_BRIDGE_DIAL_TIMEOUT_MS),
        ),
        bridge_write_timeout: duration_from_env(
            "AWIKI_CLI_TIMEOUT_BRIDGE_WRITE",
            duration_ms(DEFAULT_BRIDGE_WRITE_TIMEOUT_MS),
        ),
        bridge_read_timeout: duration_from_env(
            "AWIKI_CLI_TIMEOUT_BRIDGE_READ",
            duration_ms(DEFAULT_BRIDGE_READ_TIMEOUT_MS),
        ),
        http_dial_timeout: duration_from_env(
            "AWIKI_CLI_TIMEOUT_HTTP_DIAL",
            duration_ms(DEFAULT_HTTP_DIAL_TIMEOUT_MS),
        ),
        http_keep_alive: duration_from_env(
            "AWIKI_CLI_TIMEOUT_HTTP_KEEPALIVE",
            duration_ms(DEFAULT_HTTP_KEEP_ALIVE_MS),
        ),
        http_tls_handshake_timeout: duration_from_env(
            "AWIKI_CLI_TIMEOUT_HTTP_TLS_HANDSHAKE",
            duration_ms(DEFAULT_HTTP_TLS_HANDSHAKE_TIMEOUT_MS),
        ),
        http_response_header_timeout: duration_from_env(
            "AWIKI_CLI_TIMEOUT_HTTP_RESPONSE_HEADER",
            duration_ms(DEFAULT_HTTP_RESPONSE_HEADER_TIMEOUT_MS),
        ),
        http_idle_conn_timeout: duration_from_env(
            "AWIKI_CLI_TIMEOUT_HTTP_IDLE_CONN",
            duration_ms(DEFAULT_HTTP_IDLE_CONN_TIMEOUT_MS),
        ),
        http_max_idle_conns: int_from_env(
            "AWIKI_CLI_HTTP_MAX_IDLE_CONNS",
            DEFAULT_HTTP_MAX_IDLE_CONNS,
        ),
        http_max_idle_conns_per_host: int_from_env(
            "AWIKI_CLI_HTTP_MAX_IDLE_CONNS_PER_HOST",
            DEFAULT_HTTP_MAX_IDLE_CONNS_PER_HOST,
        ),
        profile_timeouts,
    }
}

impl Config {
    pub fn timeout_for_profile(&self, profile: Profile) -> Duration {
        self.profile_timeouts
            .get(&profile)
            .copied()
            .filter(|timeout| !timeout.is_zero())
            .or_else(|| self.profile_timeouts.get(&Profile::RpcDefault).copied())
            .unwrap_or_default()
    }
}

pub fn timeout_for_profile_name(config: &Config, profile: &str) -> Duration {
    match profile.trim() {
        "bridge_fast_path" => config.timeout_for_profile(Profile::BridgeFastPath),
        "health_probe" => config.timeout_for_profile(Profile::HealthProbe),
        "auth_refresh" => config.timeout_for_profile(Profile::AuthRefresh),
        "rpc_default" => config.timeout_for_profile(Profile::RpcDefault),
        "rpc_read_heavy" => config.timeout_for_profile(Profile::RpcReadHeavy),
        _ => config.timeout_for_profile(Profile::RpcDefault),
    }
}

pub fn duration_from_env(key: &str, fallback: Duration) -> Duration {
    let raw = std::env::var(key).unwrap_or_default();
    parse_duration(raw.trim()).unwrap_or(fallback)
}

pub fn int_from_env(key: &str, fallback: i64) -> i64 {
    let raw = std::env::var(key).unwrap_or_default();
    match raw.trim().parse::<i64>() {
        Ok(value) if value > 0 => value,
        _ => fallback,
    }
}

pub fn parse_duration(raw: &str) -> Option<Duration> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if let Ok(millis) = raw.parse::<i64>() {
        return (millis > 0).then(|| Duration::from_millis(millis as u64));
    }
    parse_unit_duration(raw)
}

fn parse_unit_duration(raw: &str) -> Option<Duration> {
    let rest = raw.trim();
    let mut rest = if let Some(next) = rest.strip_prefix('+') {
        next
    } else if rest.starts_with('-') {
        return None;
    } else {
        rest
    };
    let mut total_nanos = 0_u128;
    while !rest.is_empty() {
        let (nanos, next) = parse_duration_component(rest)?;
        total_nanos = total_nanos.checked_add(nanos)?;
        if total_nanos > MAX_GO_DURATION_NANOS {
            return None;
        }
        rest = next;
    }
    if total_nanos == 0 {
        None
    } else {
        Some(Duration::from_nanos(total_nanos as u64))
    }
}

fn parse_duration_component(rest: &str) -> Option<(u128, &str)> {
    let mut offset = 0;
    let mut int_value = 0_u128;
    let mut int_digits = 0_usize;
    for ch in rest.chars() {
        if !ch.is_ascii_digit() {
            break;
        }
        int_digits += 1;
        offset += ch.len_utf8();
        int_value = int_value
            .checked_mul(10)?
            .checked_add(ch.to_digit(10)? as u128)?;
    }

    let fraction_start = if rest[offset..].starts_with('.') {
        offset += 1;
        Some(offset)
    } else {
        None
    };
    let mut fraction_end = offset;
    let mut fraction_digits = 0_usize;
    if fraction_start.is_some() {
        for ch in rest[offset..].chars() {
            if !ch.is_ascii_digit() {
                break;
            }
            fraction_digits += 1;
            offset += ch.len_utf8();
            fraction_end = offset;
        }
    }
    if int_digits == 0 && fraction_digits == 0 {
        return None;
    }

    let (unit, next) = read_unit(&rest[offset..])?;
    let unit_nanos = unit_nanos(unit)?;
    let int_nanos = int_value.checked_mul(unit_nanos)?;
    let mut nanos = int_nanos;
    if let Some(start) = fraction_start {
        let mut scale = unit_nanos;
        for ch in rest[start..fraction_end].chars() {
            scale /= 10;
            if scale == 0 {
                continue;
            }
            nanos = nanos.checked_add(ch.to_digit(10)? as u128 * scale)?;
        }
    }
    if nanos > MAX_GO_DURATION_NANOS {
        None
    } else {
        Some((nanos, next))
    }
}

fn read_unit(rest: &str) -> Option<(&str, &str)> {
    for unit in ["ms", "us", "µs", "μs", "ns", "s", "m", "h"] {
        if let Some(next) = rest.strip_prefix(unit) {
            return Some((unit, next));
        }
    }
    None
}

fn unit_nanos(unit: &str) -> Option<u128> {
    match unit {
        "ns" => Some(1),
        "us" | "µs" | "μs" => Some(1_000),
        "ms" => Some(1_000_000),
        "s" => Some(1_000_000_000),
        "m" => Some(60 * 1_000_000_000),
        "h" => Some(60 * 60 * 1_000_000_000),
        _ => None,
    }
}

fn duration_ms(value: u64) -> Duration {
    Duration::from_millis(value)
}
