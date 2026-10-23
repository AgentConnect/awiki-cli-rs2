use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::net::TcpListener;

async fn origin(
    status: u16,
    lose_response: bool,
) -> (
    String,
    Arc<AtomicUsize>,
    Arc<AtomicUsize>,
    tokio::task::JoinHandle<()>,
) {
    origin_recorded(status, lose_response, None).await
}
async fn origin_recorded(
    status: u16,
    lose_response: bool,
    captured: Option<Arc<Mutex<Vec<String>>>>,
) -> (
    String,
    Arc<AtomicUsize>,
    Arc<AtomicUsize>,
    tokio::task::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/rpc", listener.local_addr().unwrap());
    let requests = Arc::new(AtomicUsize::new(0));
    let connections = Arc::new(AtomicUsize::new(0));
    let (r, c) = (requests.clone(), connections.clone());
    let task = tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            c.fetch_add(1, Ordering::SeqCst);
            let r = r.clone();
            let captured = captured.clone();
            tokio::spawn(async move {
                loop {
                    let Some(headers) = head(&mut stream).await else {
                        break;
                    };
                    if let Some(captured) = &captured {
                        captured.lock().unwrap().push(headers.clone());
                    }
                    let len = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|v| v.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    let mut body = vec![0; len];
                    if stream.read_exact(&mut body).await.is_err() {
                        break;
                    }
                    r.fetch_add(1, Ordering::SeqCst);
                    if lose_response {
                        break;
                    }
                    if stream
                        .write_all(
                            format!("HTTP/1.1 {status} test\r\nContent-Length: 2\r\n\r\nok")
                                .as_bytes(),
                        )
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            });
        }
    });
    (url, requests, connections, task)
}
async fn head(stream: &mut tokio::net::TcpStream) -> Option<String> {
    let mut bytes = Vec::new();
    while !bytes.ends_with(b"\r\n\r\n") {
        bytes.push(stream.read_u8().await.ok()?);
        if bytes.len() > 16384 {
            return None;
        }
    }
    String::from_utf8(bytes).ok()
}
fn runtime(proxies: Vec<String>) -> NetworkRuntime {
    let mut runtime = NetworkRuntime::new(None);
    runtime.resolver_override = Some(proxies);
    runtime.update_policy(NetworkPolicy::SystemPreferred);
    runtime
}

#[tokio::test]
async fn direct_pool_reuses_connections_without_sharing_authentication() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let (url, requests, connections, task) =
        origin_recorded(200, false, Some(captured.clone())).await;
    let runtime = runtime(vec!["direct://".into()]);
    for token in ["owner-a", "owner-b"] {
        let response = runtime
            .send(
                "POST",
                &url,
                BTreeMap::from([("authorization".into(), token.into())]),
                b"business".to_vec(),
                false,
            )
            .await
            .unwrap();
        assert_eq!(response.text().await.unwrap(), "ok");
    }
    assert_eq!(requests.load(Ordering::SeqCst), 2);
    assert_eq!(connections.load(Ordering::SeqCst), 1);
    let captured = captured.lock().unwrap();
    assert!(captured[0].contains("authorization: owner-a"));
    assert!(!captured[0].contains("owner-b"));
    assert!(captured[1].contains("authorization: owner-b"));
    assert!(!captured[1].contains("owner-a"));
    task.abort();
}

#[tokio::test]
async fn hanging_proxy_falls_back_once_and_cools_down_without_sending_business() {
    let (url, requests, _, task) = origin(200, false).await;
    let proxy = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let runtime = runtime(vec![format!("http://{}", proxy.local_addr().unwrap())]);
    let proxy_calls = Arc::new(AtomicUsize::new(0));
    let calls = proxy_calls.clone();
    let proxy_task = tokio::spawn(async move {
        loop {
            let (mut stream, _) = proxy.accept().await.unwrap();
            calls.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async move {
                let headers = head(&mut stream).await.unwrap();
                assert!(headers.starts_with("CONNECT "));
                assert!(!headers.to_lowercase().contains("authorization"));
                tokio::time::sleep(Duration::from_secs(2)).await;
            });
        }
    });
    let start = Instant::now();
    runtime
        .send(
            "POST",
            &url,
            BTreeMap::from([("authorization".into(), "secret".into())]),
            b"business".to_vec(),
            false,
        )
        .await
        .unwrap();
    assert!(start.elapsed() < Duration::from_secs(2));
    let first = proxy_calls.load(Ordering::SeqCst);
    runtime
        .send("POST", &url, BTreeMap::new(), b"second".to_vec(), false)
        .await
        .unwrap();
    let (second_url, _, _, second_task) = origin(200, false).await;
    let second_started = Instant::now();
    runtime
        .send("POST", &second_url, BTreeMap::new(), vec![], false)
        .await
        .unwrap();
    assert!(
        second_started.elapsed() < Duration::from_millis(500),
        "cooldown must cover other origins too"
    );
    second_task.abort();
    assert_eq!(proxy_calls.load(Ordering::SeqCst), first);
    assert_eq!(requests.load(Ordering::SeqCst), 2);
    assert_eq!(runtime.diagnostics().status, NetworkStatus::ProxyFallback);
    task.abort();
    proxy_task.abort();
}

#[tokio::test]
async fn concurrent_probe_is_coalesced_and_proxy_rejection_is_safe() {
    let (url, requests, _, task) = origin(200, false).await;
    let proxy = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let runtime = Arc::new(runtime(vec![format!(
        "http://{}",
        proxy.local_addr().unwrap()
    )]));
    let count = Arc::new(AtomicUsize::new(0));
    let calls = count.clone();
    let proxy_task = tokio::spawn(async move {
        loop {
            let (mut stream, _) = proxy.accept().await.unwrap();
            calls.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async move {
                let _ = head(&mut stream).await;
                stream
                    .write_all(
                        b"HTTP/1.1 407 Proxy Authentication Required\r\nContent-Length: 0\r\n\r\n",
                    )
                    .await
                    .unwrap();
            });
        }
    });
    let mut sends = Vec::new();
    for _ in 0..4 {
        let runtime = runtime.clone();
        let url = url.clone();
        sends.push(tokio::spawn(async move {
            runtime
                .send("POST", &url, BTreeMap::new(), vec![], false)
                .await
                .unwrap();
        }));
    }
    for send in sends {
        send.await.unwrap();
    }
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert_eq!(requests.load(Ordering::SeqCst), 4);
    task.abort();
    proxy_task.abort();
}

#[tokio::test]
async fn accepted_write_with_lost_response_is_never_transparently_replayed() {
    let (url, requests, _, task) = origin(200, true).await;
    let runtime = runtime(vec!["direct://".into()]);
    assert!(runtime
        .send(
            "POST",
            &url,
            BTreeMap::new(),
            b"otp-registration-message".to_vec(),
            false
        )
        .await
        .is_err());
    assert_eq!(requests.load(Ordering::SeqCst), 1);
    task.abort();
}

#[tokio::test]
async fn authentication_statuses_do_not_invalidate_or_change_route() {
    for status in [401, 403] {
        let (url, requests, _, task) = origin(status, false).await;
        let runtime = runtime(vec!["direct://".into()]);
        let response = runtime
            .send("POST", &url, BTreeMap::new(), vec![], false)
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), status);
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        assert_eq!(runtime.state().routes.len(), 1);
        assert!(runtime
            .send(
                "GET",
                &url,
                BTreeMap::from([("bad-header".into(), "invalid\nvalue".into())]),
                vec![],
                false
            )
            .await
            .is_err());
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        assert_eq!(
            runtime.state().routes.len(),
            1,
            "local validation is not a proxy fault"
        );

        task.abort();
    }
}

#[test]
fn diagnostics_are_bounded_and_policy_changes_invalidate() {
    let runtime = runtime(vec![]);
    for _ in 0..200 {
        runtime.record("route.ready", Instant::now());
    }
    let before = runtime.diagnostics();
    assert_eq!(before.records.len(), CAPACITY);
    runtime.update_policy(NetworkPolicy::Direct);
    assert!(runtime.diagnostics().generation > before.generation);
    assert_eq!(runtime.diagnostics().policy, NetworkPolicy::Direct);
    assert!(runtime.state().routes.is_empty());
    assert!(runtime.state().clients.is_empty());
}

#[tokio::test]
async fn usable_proxy_remains_available_when_direct_is_unavailable() {
    let dead = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target = format!("http://{}/rpc", dead.local_addr().unwrap());
    drop(dead);
    let proxy = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let runtime = runtime(vec![format!("http://{}", proxy.local_addr().unwrap())]);
    let requests = Arc::new(AtomicUsize::new(0));
    let calls = requests.clone();
    let task = tokio::spawn(async move {
        loop {
            let (mut stream, _) = proxy.accept().await.unwrap();
            let calls = calls.clone();
            tokio::spawn(async move {
                let headers = head(&mut stream).await.unwrap();
                if headers.starts_with("CONNECT ") {
                    assert!(!headers.to_lowercase().contains("authorization"));
                    stream
                        .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                        .await
                        .unwrap();
                    let mut bytes = [0; 1];
                    let _ = stream.read(&mut bytes).await;
                } else {
                    calls.fetch_add(1, Ordering::SeqCst);
                    stream
                        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                        .await
                        .unwrap();
                }
            });
        }
    });
    let response = runtime
        .send(
            "POST",
            &target,
            BTreeMap::new(),
            b"business".to_vec(),
            false,
        )
        .await
        .unwrap();
    assert_eq!(response.text().await.unwrap(), "ok");
    assert_eq!(requests.load(Ordering::SeqCst), 1);
    assert_eq!(runtime.diagnostics().status, NetworkStatus::SystemProxy);
    task.abort();
}

#[test]
fn scoped_trust_keeps_ca_policy_separate_and_shared_network_switches() {
    let runtime = runtime(vec![]);
    let scoped = runtime.with_trust(Some("/missing/explicit-ca.pem".into()));
    assert!(scoped.client_for(&Route::Direct, false, false).is_err());
    assert!(runtime.client_for(&Route::Direct, false, false).is_ok());
    scoped.update_policy(NetworkPolicy::Direct);
    assert_eq!(runtime.diagnostics().policy, NetworkPolicy::Direct);
    assert!(runtime.state().clients.is_empty());
}

#[test]
fn probe_and_business_connection_share_one_ten_second_budget() {
    let runtime = runtime(vec![]);
    let started = Instant::now() - Duration::from_secs(11);
    assert!(runtime
        .client_for_after(&Route::Direct, false, false, started)
        .is_err());
    assert!(runtime.state().clients.is_empty());
}
