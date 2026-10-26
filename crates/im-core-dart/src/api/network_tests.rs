use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn native_transport_streams_head_and_cancels_a_pending_body_read() {
    configure_desktop_network(DartNetworkPolicy::Direct, None)
        .await
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/stream", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut bytes = Vec::new();
        while !bytes.ends_with(b"\r\n\r\n") {
            bytes.push(socket.read_u8().await.unwrap());
        }
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100000\r\n\r\nfirst")
            .await
            .unwrap();
        std::future::pending::<()>().await;
    });
    let request = create_network_request().await;
    let response = send_network_request(&request, "GET".into(), url, vec![], vec![], false)
        .await
        .unwrap();
    assert_eq!(network_response_head(&response).await.status_code, 200);
    assert_eq!(network_response_chunk(&response).await.unwrap(), b"first");
    let pending = network_response_chunk(&response);
    let close = async {
        tokio::task::yield_now().await;
        close_network_response(&response).await;
    };
    let (result, _) = tokio::time::timeout(std::time::Duration::from_secs(1), async {
        tokio::join!(pending, close)
    })
    .await
    .unwrap();
    assert!(result.is_err());
    server.abort();
}
