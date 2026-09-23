use super::*;
use rustls::pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

#[tokio::test]
async fn rejects_unsafe_initial_wba_urls_on_sync_and_async_paths() {
    for did in [
        "did:wba:127.0.0.1:peer",
        "did:wba:169.254.169.254:peer",
        "did:wba:localhost:peer",
        "did:wba:router.local:peer",
        "did:wba:remote.test%3A8443:peer",
        "did:wba:user%40remote.test:peer",
    ] {
        let url = crate::internal::discovery::did_document::did_document_url(did).unwrap();
        assert!(get(&url, None).await.is_err(), "{did}");
        assert!(get_blocking(&url, None).is_err(), "{did}");
    }
}

#[tokio::test]
async fn rejects_empty_private_and_mixed_dns_without_connecting() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    for addresses in [
        vec![],
        vec![listener.local_addr().unwrap()],
        vec![
            "8.8.8.8:443".parse().unwrap(),
            listener.local_addr().unwrap(),
        ],
    ] {
        let result =
            get_with_resolver("https://public-looking.test/did.json", None, |_, _| async {
                Ok(addresses)
            })
            .await;
        assert!(result.is_err());
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }
    for ip in [
        "0.0.0.0",
        "10.1.2.3",
        "100.64.0.1",
        "127.0.0.1",
        "169.254.169.254",
        "172.16.0.1",
        "192.168.1.1",
        "192.0.2.1",
        "198.18.0.1",
        "198.51.100.1",
        "203.0.113.1",
        "224.0.0.1",
        "240.0.0.1",
        "::",
        "::1",
        "fc00::1",
        "fe80::1",
        "::ffff:127.0.0.1",
        "2002:7f00:1::",
        "2001:db8::1",
        "3fff::1",
    ] {
        assert!(!public_ip(ip.parse().unwrap()), "{ip}");
    }
    assert!(check_addresses(&[
        "8.8.8.8:443".parse().unwrap(),
        "[2606:4700:4700::1111]:443".parse().unwrap()
    ])
    .is_ok());
}

struct TlsFixture {
    address: SocketAddr,
    reads: Arc<AtomicUsize>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl TlsFixture {
    fn new(response: Vec<u8>) -> Self {
        let certs = CertificateDer::pem_slice_iter(include_bytes!(
            "test_fixtures/public_discovery/cert.pem"
        ))
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
        let key =
            PrivateKeyDer::from_pem_slice(include_bytes!("test_fixtures/public_discovery/key.pem"))
                .unwrap();
        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let reads = Arc::new(AtomicUsize::new(0));
        let count = reads.clone();
        listener.set_nonblocking(true).unwrap();
        let thread = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            let socket = loop {
                if let Ok((socket, _)) = listener.accept() {
                    break socket;
                }
                if std::time::Instant::now() >= deadline {
                    return;
                }
                std::thread::sleep(Duration::from_millis(5));
            };
            socket.set_nonblocking(false).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            socket
                .set_write_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let connection = rustls::ServerConnection::new(Arc::new(config)).unwrap();
            let mut stream = rustls::StreamOwned::new(connection, socket);
            let mut buf = [0u8; 4096];
            if let Ok(size) = stream.read(&mut buf) {
                if size > 0 {
                    count.fetch_add(1, Ordering::SeqCst);
                    let request = String::from_utf8_lossy(&buf[..size]).to_lowercase();
                    assert!(!request.contains("authorization:"));
                    let _ = stream.write_all(&response);
                    let _ = stream.flush();
                }
            }
        });
        Self {
            address,
            reads,
            thread: Some(thread),
        }
    }
    async fn fetch(&self, trust: bool) -> crate::ImResult<Value> {
        let ca = format!(
            "{}/src/internal/test_fixtures/public_discovery/cert.pem",
            env!("CARGO_MANIFEST_DIR")
        );
        // Only the private lower-level test fixture bypasses the public-IP gate.
        // .test has no public DNS: a successful read also proves DNS pinning.
        fetch_pinned(
            &Url::parse("https://discovery-fixture.test/did.json").unwrap(),
            &[self.address],
            trust.then_some(ca.as_str()),
        )
        .await
    }
}
impl Drop for TlsFixture {
    fn drop(&mut self) {
        self.thread.take().unwrap().join().unwrap();
    }
}

#[tokio::test]
async fn pinned_tls_reads_json_but_rejects_untrusted_certificate() {
    let fixture = TlsFixture::new(
        b"HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"ok\":true}".to_vec(),
    );
    assert_eq!(fixture.fetch(true).await.unwrap()["ok"], true);
    assert_eq!(fixture.reads.load(Ordering::SeqCst), 1);
    let untrusted = TlsFixture::new(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}".to_vec());
    assert!(untrusted.fetch(false).await.is_err());
    assert_eq!(untrusted.reads.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn redirects_never_contact_the_destination() {
    let target = TcpListener::bind("127.0.0.1:0").unwrap();
    target.set_nonblocking(true).unwrap();
    for status in [301, 302, 303, 307, 308] {
        let fixture = TlsFixture::new(format!("HTTP/1.1 {status} Redirect\r\nLocation: http://{}/private\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", target.local_addr().unwrap()).into_bytes());
        assert!(fixture.fetch(true).await.is_err());
        assert_eq!(fixture.reads.load(Ordering::SeqCst), 1);
        assert_eq!(
            target.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }
}

#[tokio::test]
async fn rejects_oversized_declared_and_streamed_responses() {
    let declared = TlsFixture::new(
        format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            MAX_BYTES + 1
        )
        .into_bytes(),
    );
    assert!(declared
        .fetch(true)
        .await
        .unwrap_err()
        .to_string()
        .contains("too large"));
    let mut response =
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n".to_vec();
    for _ in 0..=MAX_BYTES / 4096 {
        response.extend_from_slice(b"1000\r\n");
        response.extend(std::iter::repeat_n(b' ', 4096));
        response.extend_from_slice(b"\r\n");
    }
    response.extend_from_slice(b"0\r\n\r\n");
    let streamed = TlsFixture::new(response);
    assert!(streamed
        .fetch(true)
        .await
        .unwrap_err()
        .to_string()
        .contains("too large"));
}
