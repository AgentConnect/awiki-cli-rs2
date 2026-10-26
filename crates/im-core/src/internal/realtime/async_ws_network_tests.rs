use super::*;
use crate::network::{NetworkPolicy, NetworkRuntime};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

async fn read_head(stream: &mut tokio::net::TcpStream) {
    let mut bytes = Vec::new();
    while !bytes.ends_with(b"\r\n\r\n") {
        bytes.push(stream.read_u8().await.unwrap());
    }
}

#[tokio::test]
async fn direct_upgrade_errors_never_contact_a_configured_proxy() {
    for status in [401, 403, 503] {
        let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}/im/ws", origin.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut stream, _) = origin.accept().await.unwrap();
            read_head(&mut stream).await;
            stream
                .write_all(
                    format!("HTTP/1.1 {status} test\r\nContent-Length: 0\r\n\r\n").as_bytes(),
                )
                .await
                .unwrap();
        });
        let proxy = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let network = Arc::new(NetworkRuntime::with_test_proxies(vec![format!(
            "http://{}",
            proxy.local_addr().unwrap()
        )]));
        network.update_policy(NetworkPolicy::Direct);
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let proxy_server = tokio::spawn(async move {
            loop {
                let (mut stream, _) = proxy.accept().await.unwrap();
                observed.fetch_add(1, Ordering::SeqCst);
                read_head(&mut stream).await;
                let _ = stream
                    .write_all(b"HTTP/1.1 502 test\r\nContent-Length: 0\r\n\r\n")
                    .await;
            }
        });
        let error = AsyncWsTransport::connect(
            &url,
            "fixture-token",
            None,
            false,
            None,
            None,
            Some(network),
        )
        .await
        .err()
        .unwrap();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "Direct must not dial a proxy after Upgrade {status}"
        );
        assert_eq!(
            error.status_code,
            Some(status),
            "retain the original handshake error"
        );
        server.await.unwrap();
        proxy_server.abort();
    }
}

async fn connected(
    frame: Option<Message>,
) -> (
    AsyncWsTransport,
    Arc<NetworkRuntime>,
    tokio::task::JoinHandle<()>,
) {
    connected_with_policy(frame, NetworkPolicy::Direct).await
}

async fn connected_with_policy(
    frame: Option<Message>,
    policy: NetworkPolicy,
) -> (
    AsyncWsTransport,
    Arc<NetworkRuntime>,
    tokio::task::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/im/ws", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
        loop {
            tokio::time::sleep(Duration::from_millis(10)).await;
            if let Some(frame) = &frame {
                if socket.send(frame.clone()).await.is_err() {
                    break;
                }
            }
        }
    });
    let network = Arc::new(NetworkRuntime::with_test_proxies(vec!["direct://".into()]));
    network.update_policy(policy);
    let transport = AsyncWsTransport::connect(
        &url,
        "fixture-token",
        None,
        false,
        None,
        None,
        Some(network.clone()),
    )
    .await
    .unwrap();
    (transport, network, server)
}

#[tokio::test]
async fn native_configuration_poll_deadline_survives_cancelled_idle_and_busy_reads() {
    for frame in [None, Some(Message::Pong(vec![1].into()))] {
        let (mut transport, network, server) =
            connected_with_policy(frame, NetworkPolicy::SystemPreferred).await;
        network.simulate_unobserved_configuration_change();
        let observed = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if let Ok(Err(error)) =
                    tokio::time::timeout(Duration::from_millis(250), transport.read_json_message())
                        .await
                {
                    break error;
                }
            }
        })
        .await;
        server.abort();
        assert!(
            matches!(observed, Ok(crate::ImError::TransportUnavailable { ref detail }) if detail == "network_policy_changed")
        );
    }
}

#[tokio::test]
async fn policy_changed_during_upgrade_cannot_stamp_an_old_socket_with_a_new_generation() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/im/ws", listener.local_addr().unwrap());
    let network = Arc::new(NetworkRuntime::with_test_proxies(vec!["direct://".into()]));
    network.update_policy(NetworkPolicy::Direct);
    let switched = network.clone();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        tokio_tungstenite::accept_hdr_async(stream, move |_: &Request<()>, response| {
            switched.update_policy(NetworkPolicy::SystemPreferred);
            Ok(response)
        })
        .await
        .unwrap()
    });
    let error = AsyncWsTransport::connect(
        &url,
        "fixture-token",
        None,
        false,
        None,
        None,
        Some(network),
    )
    .await
    .err()
    .expect("stale connection must be rejected");
    assert!(error.message.contains("network_policy_changed"));
    let _ = server.await.unwrap();
}

#[tokio::test]
async fn policy_change_is_observed_with_idle_250ms_runner_reads() {
    assert_policy_change(None).await;
}

#[tokio::test]
async fn policy_change_is_observed_with_continuous_text_and_control_frames() {
    for frame in [
        Message::Text(r#"{"method":"fixture.event","params":{}}"#.into()),
        Message::Ping(vec![1].into()),
        Message::Pong(vec![2].into()),
    ] {
        assert_policy_change(Some(frame)).await;
    }
}

async fn assert_policy_change(frame: Option<Message>) {
    let (mut transport, network, server) = connected(frame).await;
    network.update_policy(NetworkPolicy::SystemPreferred);
    let observed = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            // Same cancellation boundary as AsyncDefaultRunnerTransport.
            if let Ok(Err(error)) =
                tokio::time::timeout(Duration::from_millis(250), transport.read_json_message())
                    .await
            {
                break error;
            }
        }
    })
    .await;
    server.abort();
    assert!(
        matches!(observed, Ok(crate::ImError::TransportUnavailable { ref detail }) if detail == "network_policy_changed"),
        "policy changes must survive read cancellation and frame traffic: {observed:?}"
    );
}
