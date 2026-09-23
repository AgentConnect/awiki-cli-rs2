//! Unauthenticated Directory discovery must never reach a private network.
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use reqwest::Url;
use serde_json::Value;

const TIMEOUT: Duration = Duration::from_secs(30);
const MAX_BYTES: usize = 1024 * 1024;

fn denied(detail: &str) -> crate::ImError {
    crate::ImError::TransportUnavailable {
        detail: format!("public discovery: {detail}"),
    }
}

fn public_url(raw: &str) -> crate::ImResult<Url> {
    let url = Url::parse(raw).map_err(|_| denied("invalid URL"))?;
    let host = url.host_str().ok_or_else(|| denied("missing host"))?;
    if url.scheme() != "https"
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || host.trim_matches(['[', ']']).parse::<IpAddr>().is_ok()
        || !host.contains('.')
        || host.ends_with('.')
        || host.ends_with(".localhost")
        || host.ends_with(".local")
    {
        return Err(denied("requires a public HTTPS domain on port 443"));
    }
    Ok(url)
}

// Matches the ANP Web resolver's public-address policy. IPv6 is allowlisted to
// global unicast, excluding special-use, documentation and transition ranges.
fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !(a == 0
                || a == 10
                || a == 127
                || a >= 224
                || (a == 100 && (64..=127).contains(&b))
                || (a == 169 && b == 254)
                || (a == 172 && (16..=31).contains(&b))
                || (a == 192 && (b == 168 || (b == 0 && (c == 0 || c == 2))))
                || (a == 198 && (b == 18 || b == 19 || (b == 51 && c == 100)))
                || (a == 203 && b == 0 && c == 113))
        }
        IpAddr::V6(ip) => {
            if let Some(v4) = ip.to_ipv4_mapped() {
                return public_ip(IpAddr::V4(v4));
            }
            let segments = ip.segments();
            (segments[0] & 0xe000) == 0x2000
                && !(segments[0] == 0x2001 && (segments[1] < 0x200 || segments[1] == 0xdb8))
                && segments[0] != 0x2002
                && (segments[0] & 0xfff0) != 0x3ff0
        }
    }
}

fn check_addresses(addresses: &[SocketAddr]) -> crate::ImResult<()> {
    if addresses.is_empty() || addresses.iter().any(|address| !public_ip(address.ip())) {
        return Err(denied("DNS must resolve exclusively to public addresses"));
    }
    Ok(())
}

pub(crate) async fn get(raw: &str, ca_bundle: Option<&str>) -> crate::ImResult<Value> {
    get_with_resolver(raw, ca_bundle, |host, port| async move {
        tokio::net::lookup_host((host.as_str(), port))
            .await
            .map(|addresses| addresses.collect())
            .map_err(|_| denied("DNS resolution failed"))
    })
    .await
}

async fn get_with_resolver<F, Fut>(
    raw: &str,
    ca_bundle: Option<&str>,
    resolve: F,
) -> crate::ImResult<Value>
where
    F: FnOnce(String, u16) -> Fut,
    Fut: std::future::Future<Output = crate::ImResult<Vec<SocketAddr>>>,
{
    tokio::time::timeout(TIMEOUT, async {
        let url = public_url(raw)?;
        let addresses = resolve(url.host_str().unwrap().to_owned(), 443).await?;
        check_addresses(&addresses)?;
        fetch_pinned(&url, &addresses, ca_bundle).await
    })
    .await
    .map_err(|_| denied("request timed out"))?
}

// Only called with vetted DNS addresses in production. Tests use a local TLS
// fixture to exercise the real redirect/body/TLS behavior without Internet IO.
async fn fetch_pinned(
    url: &Url,
    addresses: &[SocketAddr],
    ca_bundle: Option<&str>,
) -> crate::ImResult<Value> {
    let mut builder = reqwest::Client::builder()
        .use_rustls_tls()
        .https_only(true)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(TIMEOUT)
        .connect_timeout(Duration::from_secs(10))
        .resolve_to_addrs(url.host_str().unwrap(), addresses);
    if let Some(path) = ca_bundle {
        let pem = tokio::fs::read(path)
            .await
            .map_err(|_| denied("cannot read CA bundle"))?;
        let certs =
            reqwest::Certificate::from_pem_bundle(&pem).map_err(|_| denied("invalid CA bundle"))?;
        if certs.is_empty() {
            return Err(denied("empty CA bundle"));
        }
        for cert in certs {
            builder = builder.add_root_certificate(cert);
        }
    }
    let client = builder
        .build()
        .map_err(|_| denied("cannot build HTTPS client"))?;
    let mut response = client
        .get(url.clone())
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|_| denied("HTTPS request failed"))?;
    if response.status() != reqwest::StatusCode::OK {
        return Err(denied("expected HTTP 200; redirects are disabled"));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_BYTES as u64)
    {
        return Err(denied("response too large"));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| denied("response read failed"))?
    {
        if body.len() + chunk.len() > MAX_BYTES {
            return Err(denied("response too large"));
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).map_err(|_| denied("invalid JSON response"))
}

pub(crate) fn get_blocking(raw: &str, ca_bundle: Option<&str>) -> crate::ImResult<Value> {
    // This sync API can be reached from async registration; never nest runtimes.
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("im-core-public-discovery".to_owned())
            .spawn_scoped(scope, || {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|_| denied("cannot create runtime"))?
                    .block_on(get(raw, ca_bundle))
            })
            .map_err(|_| denied("cannot create worker"))?
            .join()
            .map_err(|_| denied("worker failed"))?
    })
}

#[cfg(test)]
#[path = "public_discovery_http_tests.rs"]
mod tests;
