//! Shared, account-neutral desktop network runtime.
//!
//! Route probing never sends application headers or bodies. Once submitted,
//! writes belong to the caller's idempotency/recovery contract.
#[path = "network_system_proxy.rs"]
mod system_proxy;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, VecDeque},
    hash::{Hash, Hasher},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const CONNECT_BUDGET: Duration = Duration::from_secs(10);
const DIRECT_DELAY: Duration = Duration::from_millis(750);
const ROUTE_TTL: Duration = Duration::from_secs(300);
const PROXY_COOLDOWN: Duration = Duration::from_secs(30);
const CAPACITY: usize = 128;

/// Unspecified SDK consumers retain their previous transport behavior.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetworkPolicy {
    #[default]
    Legacy,
    SystemPreferred,
    Direct,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetworkStatus {
    SystemProxy,
    #[default]
    Direct,
    ProxyFallback,
}
/// Bounded diagnostics intentionally contain no URLs, credentials or payloads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkDiagnostic {
    pub operation: u64,
    pub stage: String,
    pub elapsed_ms: u64,
    pub at: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkDiagnostics {
    pub policy: NetworkPolicy,
    pub status: NetworkStatus,
    pub generation: u64,
    pub records: Vec<NetworkDiagnostic>,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum Route {
    Legacy,
    Direct,
    Proxy(String),
}
struct Decision {
    route: Route,
    until: Instant,
    fingerprint: u64,
    fallback: bool,
}
struct State {
    policy: NetworkPolicy,
    status: NetworkStatus,
    generation: u64,
    fingerprint: u64,
    routes: BTreeMap<String, Decision>,
    bad_proxies: BTreeMap<String, Instant>,
    clients: BTreeMap<String, reqwest::Client>,
    probes: BTreeMap<String, std::sync::Weak<tokio::sync::Mutex<()>>>,
    diagnostics: VecDeque<NetworkDiagnostic>,
    operation: u64,
    monitor_started: bool,
}
// A cancelled probe (direct won the race) also cools down the proxy. This
// applies across origins so attachments and other devices do not each stall.
struct ProxyAttempt {
    state: Arc<Mutex<State>>,
    proxy: String,
    succeeded: bool,
}
impl Drop for ProxyAttempt {
    fn drop(&mut self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.succeeded {
            state.bad_proxies.remove(&self.proxy);
        } else {
            if state.bad_proxies.len() >= CAPACITY {
                state.bad_proxies.clear();
            }
            state
                .bad_proxies
                .insert(self.proxy.clone(), Instant::now() + PROXY_COOLDOWN);
        }
    }
}
#[cfg(any(target_os = "macos", target_os = "windows"))]
struct MonitorRegistration(std::sync::Weak<Mutex<State>>);
#[cfg(any(target_os = "macos", target_os = "windows"))]
impl Drop for MonitorRegistration {
    fn drop(&mut self) {
        if let Some(state) = self.0.upgrade() {
            state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .monitor_started = false;
        }
    }
}
/// One instance is shared by all identities in a Core; clients have no default auth headers.
pub struct NetworkRuntime {
    state: Arc<Mutex<State>>,
    ca_bundle: Option<String>,
    #[cfg(test)]
    resolver_override: Option<Vec<String>>,
}
impl std::fmt::Debug for NetworkRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NetworkRuntime").finish_non_exhaustive()
    }
}
pub(crate) trait NetworkIo: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> NetworkIo for T {}
pub(crate) type NetworkStream = Box<dyn NetworkIo>;
impl NetworkRuntime {
    pub fn new(ca_bundle: Option<String>) -> Self {
        Self {
            ca_bundle,
            #[cfg(test)]
            resolver_override: None,
            state: Arc::new(Mutex::new(State {
                policy: NetworkPolicy::Legacy,
                status: NetworkStatus::Direct,
                generation: 0,
                fingerprint: 0,
                routes: BTreeMap::new(),
                bad_proxies: BTreeMap::new(),
                clients: BTreeMap::new(),
                probes: BTreeMap::new(),
                diagnostics: VecDeque::new(),
                operation: 0,
                monitor_started: false,
            })),
        }
    }
    /// Shares policy and path state while retaining a Core's explicit CA scope.
    pub fn with_trust(&self, ca_bundle: Option<String>) -> Self {
        Self {
            state: self.state.clone(),
            ca_bundle: ca_bundle.or_else(|| self.ca_bundle.clone()),
            #[cfg(test)]
            resolver_override: self.resolver_override.clone(),
        }
    }
    fn origin_key(&self, url: &reqwest::Url) -> String {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.ca_bundle.hash(&mut hasher);
        format!("{}:{}", url.origin().ascii_serialization(), hasher.finish())
    }
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn start_network_monitor(&self) {
        #[cfg(test)]
        if self.resolver_override.is_some() {
            return;
        }
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        {
            let mut state = self.state();
            if state.monitor_started || state.policy != NetworkPolicy::SystemPreferred {
                return;
            }
            state.monitor_started = true;
        }
        let state = Arc::downgrade(&self.state);
        let registration = MonitorRegistration(state.clone());
        handle.spawn(async move {
            let _registration = registration;
            use n0_watcher::Watcher;
            let monitor = match netwatch::netmon::Monitor::new().await {
                Ok(monitor) => monitor,
                Err(_) => {
                    if let Some(state) = state.upgrade() { state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).monitor_started = false; }
                    return;
                }
            };
            let mut changes = monitor.interface_state();
            loop {
                tokio::select! {
                    result = changes.updated() => {
                        if result.is_err() { break; }
                        let Some(state) = state.upgrade() else { break; };
                        invalidate(&mut state.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
                    }
                    _ = tokio::time::sleep(Duration::from_secs(30)) => { if state.strong_count() == 0 { break; } }
                }
            }
        });
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    fn start_network_monitor(&self) {}
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    pub fn update_policy(&self, policy: NetworkPolicy) {
        let mut s = self.state();
        if s.policy != policy {
            s.policy = policy;
            s.status = NetworkStatus::Direct;
            invalidate(&mut s);
        }
    }
    /// Hosts can also invalidate on a platform network-change notification.
    pub fn network_changed(&self) {
        invalidate(&mut self.state());
    }
    pub fn diagnostics(&self) -> NetworkDiagnostics {
        let s = self.state();
        NetworkDiagnostics {
            policy: s.policy,
            status: s.status,
            generation: s.generation,
            records: s.diagnostics.iter().cloned().collect(),
        }
    }
    pub(crate) fn generation(&self) -> u64 {
        let fingerprint = system_fingerprint();
        let mut s = self.state();
        if s.policy == NetworkPolicy::SystemPreferred && s.fingerprint != fingerprint {
            s.fingerprint = fingerprint;
            invalidate(&mut s);
        }
        s.generation
    }
    pub(crate) fn trace(&self, operation: &str, stage: &str, started: Instant) {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        operation.hash(&mut hasher);
        let operation = hasher.finish();
        let mut s = self.state();
        if s.diagnostics.len() == CAPACITY {
            s.diagnostics.pop_front();
        }
        s.diagnostics.push_back(NetworkDiagnostic {
            at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            operation,
            stage: stage.into(),
            elapsed_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
        });
    }
    fn record(&self, stage: &str, started: Instant) {
        let mut s = self.state();
        s.operation += 1;
        let operation = s.operation;
        if s.diagnostics.len() == CAPACITY {
            s.diagnostics.pop_front();
        }
        s.diagnostics.push_back(NetworkDiagnostic {
            at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            operation,
            stage: stage.into(),
            elapsed_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
        });
    }
    pub(crate) async fn route(&self, url: &str) -> crate::ImResult<Route> {
        self.start_network_monitor();
        let parsed = parse_url(url)?;
        let origin = self.origin_key(&parsed);
        self.generation();
        let lock = {
            let mut s = self.state();
            match s.policy {
                NetworkPolicy::Legacy => return Ok(Route::Legacy),
                NetworkPolicy::Direct => return Ok(Route::Direct),
                _ => {}
            }
            s.probes.retain(|_, lock| lock.strong_count() > 0);
            if let Some(lock) = s.probes.get(&origin).and_then(std::sync::Weak::upgrade) {
                lock
            } else {
                let lock = Arc::new(tokio::sync::Mutex::new(()));
                s.probes.insert(origin.clone(), Arc::downgrade(&lock));
                lock
            }
        };
        let _guard = lock.lock().await;
        let generation = {
            let mut s = self.state();
            match s.policy {
                NetworkPolicy::Legacy => return Ok(Route::Legacy),
                NetworkPolicy::Direct => return Ok(Route::Direct),
                _ => {}
            }
            if let Some(d) = s.routes.get(&origin) {
                if d.until > Instant::now() && d.fingerprint == s.fingerprint {
                    let route = d.route.clone();
                    let fallback = d.fallback;
                    s.status = status(&route, fallback);
                    return Ok(route);
                }
            }
            s.generation
        };
        let started = Instant::now();
        // Resolution and the entire tunnel/TLS handshake race against direct.
        // Neither branch sends a business request, even when the proxy hangs.
        let proxy = async {
            let proxies = self.resolve(parsed.as_str()).await?;
            let mut had_proxy = false;
            for proxy in proxies {
                if proxy == "direct://" {
                    if !had_proxy {
                        return Ok((Route::Direct, false));
                    }
                    continue;
                }
                had_proxy = true;
                if self
                    .state()
                    .bad_proxies
                    .get(&proxy)
                    .is_some_and(|until| *until > Instant::now())
                {
                    continue;
                }
                let mut attempt = ProxyAttempt {
                    state: self.state.clone(),
                    proxy: proxy.clone(),
                    succeeded: false,
                };
                let route = Route::Proxy(proxy);
                if self.open_route(&parsed, &route).await.is_ok() {
                    attempt.succeeded = true;
                    return Ok((route, false));
                }
            }
            if had_proxy {
                Err(unavailable("proxy_unavailable"))
            } else {
                Ok((Route::Direct, false))
            }
        };
        let direct = async {
            tokio::time::sleep(DIRECT_DELAY).await;
            self.open_route(&parsed, &Route::Direct)
                .await
                .map(|_| (Route::Direct, true))
        };
        tokio::pin!(proxy, direct);
        let choice = tokio::time::timeout(CONNECT_BUDGET, async {
            tokio::select! {
                result = &mut proxy => match result {
                    Ok(choice) => Ok(choice),
                    Err(_) => self.open_route(&parsed, &Route::Direct).await.map(|_| (Route::Direct, true)),
                },
                result = &mut direct => match result { Ok(choice) => Ok(choice), Err(_) => proxy.await },
            }
        }).await.map_err(|_| unavailable("network_connect_timeout"))??;
        self.record(
            if choice.1 {
                "route.proxy_fallback"
            } else {
                "route.ready"
            },
            started,
        );
        let changed = {
            let mut s = self.state();
            if s.generation == generation {
                if s.routes.len() >= CAPACITY {
                    s.routes.clear();
                }
                let fingerprint = s.fingerprint;
                s.status = status(&choice.0, choice.1);
                s.routes.insert(
                    origin,
                    Decision {
                        route: choice.0.clone(),
                        until: Instant::now() + if choice.1 { PROXY_COOLDOWN } else { ROUTE_TTL },
                        fingerprint,
                        fallback: choice.1,
                    },
                );
            }
            s.generation != generation
        };
        drop(_guard);
        if changed {
            Box::pin(self.route(url)).await
        } else {
            Ok(choice.0)
        }
    }
    async fn resolve(&self, url: &str) -> crate::ImResult<Vec<String>> {
        #[cfg(test)]
        if let Some(proxies) = &self.resolver_override {
            return Ok(proxies.clone());
        }
        resolve_system(url).await
    }
    pub(crate) async fn alternate(&self, url: &str, previous: &Route) -> crate::ImResult<Route> {
        let parsed = parse_url(url)?;
        let started = Instant::now();
        let route = tokio::time::timeout(CONNECT_BUDGET, async {
            if matches!(previous, Route::Proxy(_)) {
                self.open_route(&parsed, &Route::Direct).await?;
                Ok(Route::Direct)
            } else {
                for proxy in self.resolve(url).await? {
                    if proxy == "direct://" {
                        continue;
                    }
                    if self
                        .state()
                        .bad_proxies
                        .get(&proxy)
                        .is_some_and(|until| *until > Instant::now())
                    {
                        continue;
                    }
                    let route = Route::Proxy(proxy);
                    if self.open_route(&parsed, &route).await.is_ok() {
                        return Ok(route);
                    }
                }
                Err(unavailable("network_no_alternate_route"))
            }
        })
        .await
        .map_err(|_| unavailable("network_connect_timeout"))??;
        let mut s = self.state();
        let fingerprint = s.fingerprint;
        if let Route::Proxy(proxy) = previous {
            s.bad_proxies
                .insert(proxy.clone(), Instant::now() + PROXY_COOLDOWN);
        }
        s.status = status(&route, matches!(route, Route::Direct));
        s.routes.insert(
            self.origin_key(&parsed),
            Decision {
                route: route.clone(),
                until: Instant::now() + PROXY_COOLDOWN,
                fingerprint,
                fallback: matches!(route, Route::Direct),
            },
        );
        drop(s);
        self.record("route.alternate", started);
        Ok(route)
    }
    /// Raw transport for host-owned protocols; authentication is request-owned.
    pub async fn send(
        &self,
        method: &str,
        url: &str,
        headers: BTreeMap<String, String>,
        body: Vec<u8>,
        redirects: bool,
    ) -> crate::ImResult<reqwest::Response> {
        let method = reqwest::Method::from_bytes(method.as_bytes())
            .map_err(|_| unavailable("network_method"))?;
        let connection_started = Instant::now();
        let route = self.route(url).await?;
        let client = self.client_for_after(&route, redirects, true, connection_started)?;
        let build = |client: &reqwest::Client| {
            let mut request = client.request(method.clone(), url).body(body.clone());
            for (name, value) in &headers {
                request = request.header(name, value);
            }
            request
        };
        match tokio::time::timeout(
            crate::internal::http::RESPONSE_TIMEOUT,
            build(&client).send(),
        )
        .await
        .map_err(|_| unavailable("network_response_timeout"))?
        {
            Ok(response) => Ok(response),
            Err(error) => {
                if error.is_builder() {
                    return Err(unavailable("network_request_invalid"));
                }
                self.failed(url);
                if self.diagnostics().policy == NetworkPolicy::SystemPreferred
                    && (error.is_connect()
                        || method == reqwest::Method::GET
                        || method == reqwest::Method::HEAD)
                {
                    let connection_started = Instant::now();
                    let alternate = self.alternate(url, &route).await?;
                    tokio::time::timeout(
                        crate::internal::http::RESPONSE_TIMEOUT,
                        build(&self.client_for_after(
                            &alternate,
                            redirects,
                            true,
                            connection_started,
                        )?)
                        .send(),
                    )
                    .await
                    .map_err(|_| unavailable("network_response_timeout"))?
                    .map_err(|_| unavailable("network_request_failed"))
                } else {
                    Err(unavailable("network_request_failed"))
                }
            }
        }
    }
    pub(crate) async fn client(
        &self,
        url: &str,
        redirects: bool,
        attachment: bool,
    ) -> crate::ImResult<reqwest::Client> {
        let started = Instant::now();
        let route = self.route(url).await?;
        self.client_for_after(&route, redirects, attachment, started)
    }
    pub(crate) fn client_for_after(
        &self,
        route: &Route,
        redirects: bool,
        attachment: bool,
        started: Instant,
    ) -> crate::ImResult<reqwest::Client> {
        if matches!(route, Route::Legacy) {
            return self.client_for(route, redirects, attachment);
        }
        self.client_for_budget(route, redirects, attachment, connect_budget(started)?)
    }
    pub(crate) fn client_for(
        &self,
        route: &Route,
        redirects: bool,
        attachment: bool,
    ) -> crate::ImResult<reqwest::Client> {
        self.client_for_budget(route, redirects, attachment, CONNECT_BUDGET)
    }
    fn client_for_budget(
        &self,
        route: &Route,
        redirects: bool,
        attachment: bool,
        budget: Duration,
    ) -> crate::ImResult<reqwest::Client> {
        // Round down so a pooled client's connect limit never exceeds this
        // operation's remaining budget; keep the pool bounded by a few buckets.
        let budget_ms = (budget.as_millis() / 250 * 250).max(1) as u64;
        let key = format!(
            "{:?}:{}:{redirects}:{attachment}:{budget_ms}",
            self.ca_bundle,
            match route {
                Route::Legacy => "legacy",
                Route::Direct => "direct",
                Route::Proxy(p) => p,
            }
        );
        let mut s = self.state();
        if let Some(client) = s.clients.get(&key) {
            return Ok(client.clone());
        }
        let mut builder = crate::internal::http::async_client_builder(
            self.ca_bundle.as_deref(),
            redirects,
            if attachment {
                None
            } else {
                Some(crate::internal::http::RESPONSE_TIMEOUT)
            },
        )?;
        builder = builder.connect_timeout(Duration::from_millis(budget_ms));
        if !matches!(route, Route::Legacy) {
            builder = builder.no_proxy().retry(reqwest::retry::never());
        }
        if let Route::Proxy(proxy) = route {
            builder = builder.proxy(
                reqwest::Proxy::all(proxy).map_err(|_| unavailable("invalid_system_proxy"))?,
            );
        }
        let client = builder
            .build()
            .map_err(|_| unavailable("network_client_initialization"))?;
        if s.clients.len() >= CAPACITY {
            s.clients.clear();
        }
        s.clients.insert(key, client.clone());
        Ok(client)
    }
    pub(crate) fn failed(&self, url: &str) {
        if let Ok(url) = parse_url(url) {
            self.state().routes.remove(&self.origin_key(&url));
        }
    }
    pub(crate) async fn connect(&self, url: &str) -> crate::ImResult<NetworkStream> {
        let route = self.route(url).await?;
        self.connect_route(url, &route).await
    }
    pub(crate) async fn connect_route_after(
        &self,
        url: &str,
        route: &Route,
        started: Instant,
    ) -> crate::ImResult<NetworkStream> {
        let parsed = parse_url(url)?;
        tokio::time::timeout(connect_budget(started)?, self.open_route(&parsed, route))
            .await
            .map_err(|_| unavailable("network_connect_timeout"))?
    }
    pub(crate) async fn connect_route(
        &self,
        url: &str,
        route: &Route,
    ) -> crate::ImResult<NetworkStream> {
        let parsed = parse_url(url)?;
        tokio::time::timeout(CONNECT_BUDGET, self.open_route(&parsed, route))
            .await
            .map_err(|_| unavailable("network_connect_timeout"))?
    }
    async fn open_route(
        &self,
        url: &reqwest::Url,
        route: &Route,
    ) -> crate::ImResult<NetworkStream> {
        let host = url
            .host_str()
            .ok_or_else(|| unavailable("network_url_host"))?;
        let port = url
            .port_or_known_default()
            .ok_or_else(|| unavailable("network_url_port"))?;
        let mut stream: NetworkStream = match route {
            Route::Proxy(raw) => {
                let proxy =
                    reqwest::Url::parse(raw).map_err(|_| unavailable("invalid_system_proxy"))?;
                if !proxy.username().is_empty() || proxy.password().is_some() {
                    return Err(unavailable("proxy_auth_unsupported"));
                }
                let proxy_host = proxy
                    .host_str()
                    .ok_or_else(|| unavailable("invalid_system_proxy"))?;
                let proxy_port = proxy.port_or_known_default().unwrap_or(1080);
                if matches!(proxy.scheme(), "socks" | "socks5" | "socks5h") {
                    Box::new(
                        tokio_socks::tcp::Socks5Stream::connect(
                            (proxy_host, proxy_port),
                            (host, port),
                        )
                        .await
                        .map_err(|_| unavailable("proxy_socks_connect"))?,
                    )
                } else if matches!(proxy.scheme(), "http" | "https") {
                    let tcp = tokio::net::TcpStream::connect((proxy_host, proxy_port))
                        .await
                        .map_err(|_| unavailable("proxy_connect"))?;
                    let mut stream: NetworkStream = if proxy.scheme() == "https" {
                        self.tls(proxy_host, Box::new(tcp)).await?
                    } else {
                        Box::new(tcp)
                    };
                    let authority = if host.contains(':') {
                        format!("[{host}]:{port}")
                    } else {
                        format!("{host}:{port}")
                    };
                    stream
                        .write_all(
                            format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n\r\n")
                                .as_bytes(),
                        )
                        .await
                        .map_err(|_| unavailable("proxy_tunnel"))?;
                    let mut headers = Vec::new();
                    while !headers.ends_with(b"\r\n\r\n") {
                        if headers.len() >= 8192 {
                            return Err(unavailable("proxy_tunnel_headers"));
                        }
                        headers.push(
                            stream
                                .read_u8()
                                .await
                                .map_err(|_| unavailable("proxy_tunnel"))?,
                        );
                    }
                    let line = std::str::from_utf8(&headers)
                        .map_err(|_| unavailable("proxy_tunnel_response"))?
                        .lines()
                        .next()
                        .unwrap_or("");
                    if line.split_whitespace().nth(1) != Some("200") {
                        return Err(unavailable("proxy_tunnel_rejected"));
                    }
                    stream
                } else {
                    return Err(unavailable("proxy_scheme_unsupported"));
                }
            }
            _ => Box::new(
                tokio::net::TcpStream::connect((host, port))
                    .await
                    .map_err(|_| unavailable("direct_connect"))?,
            ),
        };
        if matches!(url.scheme(), "https" | "wss") {
            stream = self.tls(host, stream).await?;
        }
        Ok(stream)
    }
    async fn tls(&self, host: &str, stream: NetworkStream) -> crate::ImResult<NetworkStream> {
        let config = crate::internal::http::network_tls_config(self.ca_bundle.as_deref())?;
        let name = rustls::pki_types::ServerName::try_from(host.to_owned())
            .map_err(|_| unavailable("network_tls_name"))?;
        Ok(Box::new(
            tokio_rustls::TlsConnector::from(config)
                .connect(name, stream)
                .await
                .map_err(|_| unavailable("network_tls_handshake"))?,
        ))
    }
}
fn connect_budget(started: Instant) -> crate::ImResult<Duration> {
    CONNECT_BUDGET
        .checked_sub(started.elapsed())
        .filter(|budget| !budget.is_zero())
        .ok_or_else(|| unavailable("network_connect_timeout"))
}

fn invalidate(s: &mut State) {
    s.generation = s.generation.wrapping_add(1);
    s.routes.clear();
    s.bad_proxies.clear();
    s.clients.clear();
}
fn status(route: &Route, fallback: bool) -> NetworkStatus {
    if fallback {
        NetworkStatus::ProxyFallback
    } else if matches!(route, Route::Proxy(_)) {
        NetworkStatus::SystemProxy
    } else {
        NetworkStatus::Direct
    }
}
fn parse_url(raw: &str) -> crate::ImResult<reqwest::Url> {
    let url = reqwest::Url::parse(raw).map_err(|_| unavailable("network_url"))?;
    if !matches!(url.scheme(), "http" | "https" | "ws" | "wss")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(unavailable("network_url"));
    }
    Ok(url)
}
fn unavailable(code: &str) -> crate::ImError {
    crate::ImError::TransportUnavailable {
        detail: code.into(),
    }
}
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn system_fingerprint() -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    proxyres::config::auto_discover().hash(&mut h);
    proxyres::config::auto_config_url().hash(&mut h);
    for scheme in ["http", "https", "socks"] {
        proxyres::config::proxy(scheme).hash(&mut h);
    }
    proxyres::config::bypass_list().hash(&mut h);
    if let Ok(interfaces) = if_addrs::get_if_addrs() {
        for interface in interfaces {
            interface.name.hash(&mut h);
            interface.ip().hash(&mut h);
        }
    }
    h.finish()
}
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn system_fingerprint() -> u64 {
    0
}
#[cfg(any(target_os = "macos", target_os = "windows"))]
async fn resolve_system(url: &str) -> crate::ImResult<Vec<String>> {
    // Never modify process-global overrides or enable auto-discovery.
    let mut url = parse_url(url)?;
    if url.scheme() == "wss" {
        let _ = url.set_scheme("https");
    } else if url.scheme() == "ws" {
        let _ = url.set_scheme("http");
    }
    url.set_path("/");
    url.set_query(None);
    url.set_fragment(None);
    tokio::task::spawn_blocking(move || {
        let mut resolver = proxyres::resolver::Resolver::new();
        resolver
            .get_proxies_for_url(url.as_str())
            .map_err(|_| unavailable("system_proxy_resolution"))?;
        if !resolver.wait(Some(CONNECT_BUDGET)) {
            resolver.cancel();
            return Err(unavailable("system_proxy_timeout"));
        }
        if resolver.error().is_some() {
            return Err(unavailable("system_proxy_resolution"));
        }
        let mut proxies = resolver
            .proxies()
            .into_iter()
            .map(|proxy| proxy.replace("socks://", "socks5h://"))
            .collect::<Vec<_>>();
        if proxies.iter().all(|proxy| proxy == "direct://")
            && proxyres::config::proxy(url.scheme()).is_none()
            && proxyres::config::auto_config_url().is_none()
            && !proxyres::config::auto_discover()
            && !system_proxy::bypass(&url, &proxyres::config::bypass_list().unwrap_or_default())
        {
            if let Some(proxy) = proxyres::config::proxy("socks") {
                proxies = vec![format!("socks5h://{proxy}")];
            }
        }
        Ok(proxies)
    })
    .await
    .map_err(|_| unavailable("system_proxy_worker"))?
}
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
async fn resolve_system(_url: &str) -> crate::ImResult<Vec<String>> {
    Ok(vec!["direct://".into()])
}

#[cfg(test)]
#[path = "network_tests.rs"]
mod tests;
