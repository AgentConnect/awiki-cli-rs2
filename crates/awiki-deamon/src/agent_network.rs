//! Resolve one proxy group per agent process; never mutate the daemon environment.
use std::collections::{BTreeMap, BTreeSet};
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const PROXY_MODE_KEY: &str = "AWIKI_DAEMON_AGENT_PROXY_MODE";
pub(crate) const PROXY_ADDRESS_KEYS: &[&str] = &[
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "http_proxy",
    "https_proxy",
    "all_proxy",
];
pub(crate) const PROXY_EXCLUSION_KEYS: &[&str] = &["NO_PROXY", "no_proxy"];
const LOCAL_CHECK_BUDGET: Duration = Duration::from_millis(250);

pub(crate) struct AgentNetworkEnv {
    values: BTreeMap<String, String>,
    pub(crate) detail: &'static str,
}

impl AgentNetworkEnv {
    pub(crate) fn current() -> Self {
        let env = std::env::vars_os()
            .filter_map(|(key, value)| Some((key.into_string().ok()?, value.into_string().ok()?)))
            .collect();
        resolve(&env, read_system_proxy, local_proxy_listening)
    }

    pub(crate) fn apply(&self, command: &mut Command) {
        for key in PROXY_ADDRESS_KEYS.iter().chain(PROXY_EXCLUSION_KEYS) {
            command.env_remove(key);
        }
        command.envs(&self.values);
    }
}

pub(crate) fn apply_agent_network(command: &mut Command) {
    let network = AgentNetworkEnv::current();
    // Diagnostics contain neither addresses nor credentials.
    if !matches!(
        network.detail,
        "inherited environment and system routing" | "system proxy applied to agent child"
    ) {
        eprintln!("agent network: {}", network.detail);
    }
    network.apply(command);
}

fn resolve(
    env: &BTreeMap<String, String>,
    system: impl FnOnce() -> Result<BTreeMap<String, String>, &'static str>,
    mut listening: impl FnMut(SocketAddr, Duration) -> bool,
) -> AgentNetworkEnv {
    let mut values = BTreeMap::new();
    for &key in PROXY_ADDRESS_KEYS.iter().chain(PROXY_EXCLUSION_KEYS) {
        if let Some(value) = env.get(key) {
            values.insert(key.to_string(), value.clone());
        }
    }
    let explicit = PROXY_ADDRESS_KEYS.iter().any(|key| env.contains_key(*key));
    let mode = env
        .get(PROXY_MODE_KEY)
        .map(String::as_str)
        .unwrap_or("auto");
    let mut detail = "inherited environment and system routing";
    let mut automatic = false;
    if !explicit && mode == "auto" {
        match system() {
            Ok(discovered) if !discovered.is_empty() => {
                // Exclusions from both sources are additive, even when addresses are automatic.
                for (key, value) in discovered {
                    if PROXY_EXCLUSION_KEYS.contains(&key.as_str()) {
                        let current = values.entry(key).or_insert_with(String::new);
                        if !current.is_empty() {
                            current.push(',');
                        }
                        current.push_str(&value);
                    } else {
                        values.insert(key, value);
                    }
                }
                automatic = true;
                detail = "system proxy applied to agent child";
            }
            Ok(_) => {}
            Err(reason) => detail = reason,
        }
    } else if !matches!(mode, "auto" | "inherit") {
        detail = "unknown agent proxy mode; preserving inherited environment and routing";
    }
    let deadline = Instant::now() + LOCAL_CHECK_BUDGET;
    let mut checked = BTreeSet::new();
    for &key in PROXY_ADDRESS_KEYS {
        let Some(value) = values.get(key).filter(|v| !v.is_empty()) else {
            continue;
        };
        let Some(addresses) = local_proxy_addresses(value) else {
            continue;
        };
        if checked.insert(addresses.clone())
            && !addresses.into_iter().any(|address| {
                listening(address, deadline.saturating_duration_since(Instant::now()))
            })
        {
            if automatic {
                values = env
                    .iter()
                    .filter(|(key, _)| PROXY_EXCLUSION_KEYS.contains(&key.as_str()))
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect();
                detail = "automatic local proxy is not listening; automatic injection skipped, system routing retained";
            } else {
                detail = "explicit local proxy connection failed; configuration retained, check the configured proxy; no alternate route or task replay";
            }
            break;
        }
    }
    if values.values().any(|value| !value.is_empty()) {
        protect_callbacks(&mut values);
    }
    AgentNetworkEnv { values, detail }
}

fn protect_callbacks(values: &mut BTreeMap<String, String>) {
    let mut exclusions = vec![
        "localhost".to_string(),
        "127.0.0.1".to_string(),
        "127.0.0.0/8".to_string(),
        "::1".to_string(),
        "[::1]".to_string(),
    ];
    for address in local_interface_addresses() {
        let address = address.to_string();
        if !exclusions.contains(&address) {
            exclusions.push(address);
        }
    }
    for &key in PROXY_EXCLUSION_KEYS {
        for entry in values
            .get(key)
            .into_iter()
            .flat_map(|v| v.split(','))
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            if !exclusions.iter().any(|v| v == entry) {
                exclusions.push(entry.to_string());
            }
        }
    }
    for &key in PROXY_EXCLUSION_KEYS {
        values.insert(key.to_string(), exclusions.join(","));
    }
}

fn local_proxy_addresses(value: &str) -> Option<Vec<SocketAddr>> {
    let url = reqwest::Url::parse(value).ok()?;
    let host = url
        .host_str()?
        .trim_start_matches('[')
        .trim_end_matches(']');
    let port = url.port_or_known_default()?;
    if host.trim_end_matches('.').eq_ignore_ascii_case("localhost") {
        return Some(vec![
            SocketAddr::from(([127, 0, 0, 1], port)),
            SocketAddr::new(std::net::Ipv6Addr::LOCALHOST.into(), port),
        ]);
    }
    let ip: IpAddr = host.parse().ok()?;
    (ip.is_loopback() || local_interface_addresses().contains(&ip))
        .then_some(vec![SocketAddr::new(ip, port)])
}

fn local_proxy_listening(address: SocketAddr, budget: Duration) -> bool {
    !budget.is_zero() && TcpStream::connect_timeout(&address, budget).is_ok()
}

#[cfg(unix)]
fn local_interface_addresses() -> Vec<IpAddr> {
    let mut addresses = Vec::new();
    unsafe {
        let mut first = std::ptr::null_mut();
        if libc::getifaddrs(&mut first) != 0 {
            return addresses;
        }
        let mut entry = first;
        while !entry.is_null() {
            let addr = (*entry).ifa_addr;
            if !addr.is_null() {
                match i32::from((*addr).sa_family) {
                    libc::AF_INET => {
                        let addr = &*(addr as *const libc::sockaddr_in);
                        addresses.push(IpAddr::V4(std::net::Ipv4Addr::from(
                            addr.sin_addr.s_addr.to_ne_bytes(),
                        )));
                    }
                    libc::AF_INET6 => {
                        let addr = &*(addr as *const libc::sockaddr_in6);
                        addresses
                            .push(IpAddr::V6(std::net::Ipv6Addr::from(addr.sin6_addr.s6_addr)));
                    }
                    _ => {}
                }
            }
            entry = (*entry).ifa_next;
        }
        libc::freeifaddrs(first);
    }
    addresses
}

#[cfg(not(unix))]
fn local_interface_addresses() -> Vec<IpAddr> {
    Vec::new()
}

fn read_system_proxy() -> Result<BTreeMap<String, String>, &'static str> {
    if !cfg!(target_os = "macos") {
        return Ok(BTreeMap::new());
    }
    use crate::plugins::generic_cli::process::ManagedChild;
    let mut command = Command::new("/usr/sbin/scutil");
    command
        .arg("--proxy")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = ManagedChild::spawn(&mut command, "read macOS system proxy")
        .and_then(|child| child.wait_timeout("read macOS system proxy", Duration::from_secs(1)))
        .map_err(|_| {
            "system proxy lookup unavailable; inherited environment and routing retained"
        })?;
    if !output.output.status.success() {
        return Err("system proxy lookup failed; inherited environment and routing retained");
    }
    parse_system_proxy(&String::from_utf8_lossy(&output.output.stdout))
}

fn parse_system_proxy(raw: &str) -> Result<BTreeMap<String, String>, &'static str> {
    let mut fields = BTreeMap::new();
    let mut exceptions = Vec::new();
    let mut depth = 0usize;
    let mut in_exceptions = false;
    for line in raw.lines().map(str::trim) {
        if depth == 1 {
            if let Some((key, value)) = line.split_once(" : ") {
                if key == "ExceptionsList" {
                    in_exceptions = true;
                } else {
                    fields.insert(key, value);
                }
            }
        } else if depth == 2 && in_exceptions {
            if let Some((_, value)) = line.split_once(" : ") {
                exceptions.push(value);
            }
        }
        if line.ends_with('{') {
            depth += 1;
        }
        if line == "}" {
            depth = depth.saturating_sub(1);
            if depth == 1 {
                in_exceptions = false;
            }
        }
    }
    if fields.get("ProxyAutoConfigEnable") == Some(&"1")
        || fields.get("ProxyAutoDiscoveryEnable") == Some(&"1")
    {
        return Err(
            "PAC/WPAD proxy discovery is not supported; inherited environment and routing retained",
        );
    }
    if ["HTTPEnable", "HTTPSEnable", "SOCKSEnable"]
        .iter()
        .all(|key| fields.get(key) != Some(&"1"))
    {
        return Ok(BTreeMap::new());
    }
    if fields.get("ExcludeSimpleHostnames") == Some(&"1") {
        return Err("system proxy excludes simple hostnames that NO_PROXY cannot represent; automatic injection skipped");
    }
    let exceptions = exceptions.into_iter().map(|value| {
        let value = value.strip_prefix("*.").map(|suffix| format!(".{suffix}")).unwrap_or_else(|| value.to_string());
        if value != "*" && (value.contains('*') || value.contains('?')) {
            return Err("system proxy exclusion cannot be represented by NO_PROXY; automatic injection skipped");
        }
        Ok(value)
    }).collect::<Result<Vec<_>, _>>()?;
    let mut values = BTreeMap::new();
    for (kind, scheme, key) in [
        ("HTTP", "http", "HTTP_PROXY"),
        ("HTTPS", "http", "HTTPS_PROXY"),
        ("SOCKS", "socks5h", "ALL_PROXY"),
    ] {
        if fields.get(format!("{kind}Enable").as_str()) != Some(&"1") {
            continue;
        }
        let host = fields
            .get(format!("{kind}Proxy").as_str())
            .ok_or("enabled system proxy is missing its host; automatic injection skipped")?;
        let host = host.trim_start_matches('[').trim_end_matches(']');
        if host.is_empty()
            || host
                .chars()
                .any(|c| c.is_whitespace() || matches!(c, '/' | '@' | '?' | '#'))
        {
            return Err("enabled system proxy has an invalid host; automatic injection skipped");
        }
        let port = fields
            .get(format!("{kind}Port").as_str())
            .and_then(|p| p.parse::<u16>().ok())
            .filter(|p| *p > 0)
            .ok_or("enabled system proxy has an invalid port; automatic injection skipped")?;
        let host = if host.parse::<std::net::Ipv6Addr>().is_ok() {
            format!("[{host}]")
        } else {
            host.to_string()
        };
        let value = format!("{scheme}://{host}:{port}");
        let url = reqwest::Url::parse(&value).map_err(|_| {
            "enabled system proxy has an invalid address; automatic injection skipped"
        })?;
        if url.host_str().is_none()
            || url.port() != Some(port) && url.port_or_known_default() != Some(port)
        {
            return Err("enabled system proxy has an invalid address; automatic injection skipped");
        }
        values.insert(key.to_string(), value.clone());
        values.insert(key.to_ascii_lowercase(), value);
    }
    if !values.is_empty() && !exceptions.is_empty() {
        values.insert("NO_PROXY".to_string(), exceptions.join(","));
    }
    Ok(values)
}

#[cfg(test)]
#[path = "agent_network_tests.rs"]
mod tests;
