//! Static SOCKS fallback when the platform has no per-HTTP proxy or PAC.
//! PAC and native HTTP bypass processing stay in the system resolver.
use reqwest::Url;

pub(super) fn bypass(url: &Url, rules: &str) -> bool {
    let host = url
        .host_str()
        .unwrap_or("")
        .trim_matches(['[', ']'])
        .to_ascii_lowercase();
    let rules = rules.split([',', ';']).map(str::trim).collect::<Vec<_>>();
    let loopback = host == "localhost"
        || host.ends_with(".localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback());
    if loopback && !rules.contains(&"<-loopback>") {
        return true;
    }
    for rule in rules {
        let rule = rule.to_ascii_lowercase();
        if rule == "<-loopback>" {
            continue;
        }
        if rule == "<local>" && !host.contains('.') && host.parse::<std::net::IpAddr>().is_err() {
            return true;
        }
        if let Ok(net) = rule.parse::<ipnet::IpNet>() {
            if host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| net.contains(&ip))
            {
                return true;
            }
            continue;
        }
        let (scheme, rule) = rule
            .split_once("://")
            .map_or((None, rule.as_str()), |(scheme, rule)| (Some(scheme), rule));
        if scheme.is_some_and(|scheme| scheme != url.scheme()) {
            continue;
        }
        let (pattern, port) = if rule.starts_with('[') {
            if let Some(end) = rule.find(']') {
                (&rule[1..end], rule[end + 1..].strip_prefix(':'))
            } else {
                continue;
            }
        } else if rule.matches(':').count() == 1 {
            rule.rsplit_once(':')
                .map_or((rule, None), |(host, port)| (host, Some(port)))
        } else {
            (rule, None)
        };
        if port.is_some_and(|port| port.parse::<u16>().ok() != url.port_or_known_default()) {
            continue;
        }
        if let Some(suffix) = pattern.strip_prefix('.') {
            if host.ends_with(&format!(".{suffix}")) {
                return true;
            }
        } else if glob(pattern.as_bytes(), host.as_bytes()) {
            return true;
        }
    }
    false
}
fn glob(pattern: &[u8], value: &[u8]) -> bool {
    let (mut p, mut v, mut star, mut checkpoint) = (0, 0, None, 0);
    while v < value.len() {
        if p < pattern.len() && (pattern[p] == b'?' || pattern[p] == value[v]) {
            p += 1;
            v += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star = Some(p);
            p += 1;
            checkpoint = v;
        } else if let Some(s) = star {
            checkpoint += 1;
            v = checkpoint;
            p = s + 1;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }
    p == pattern.len()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn socks_exceptions_keep_domains_ports_cidr_and_local_rules() {
        for (url, rule, expected) in [
            ("https://api.example.test", "*.example.test", true),
            ("https://example.test", ".example.test", false),
            ("https://api.example.test:8443", "*.example.test:443", false),
            ("https://api.example.test:8443", "*.example.test:8443", true),
            ("http://10.2.3.4", "10.0.0.0/8", true),
            ("http://[fd00::1]", "fd00::/8", true),
            ("http://intranet", "<local>", true),
            ("http://localhost", "", true),
            ("http://localhost", "<-loopback>", false),
            ("https://api.example.test", "http://*.example.test", false),
        ] {
            assert_eq!(
                bypass(&Url::parse(url).unwrap(), rule),
                expected,
                "{url} {rule}"
            );
        }
    }
}
