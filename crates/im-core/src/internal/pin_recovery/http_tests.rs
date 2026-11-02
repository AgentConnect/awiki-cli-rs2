use super::super::tests::{session, target};
use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn exact(session: &Session, method: &str, suffix: &str) -> ExactRequest {
    ExactRequest {
        method: method.into(),
        url: format!("{}/{}", session.config.control_base_url, suffix),
        operation_id: None,
        body_b64u: String::new(),
        idempotency_key: "test-key".into(),
    }
}

#[test]
fn only_authenticated_target_discovery_get_receives_the_larger_budget() {
    let s = session();
    assert_eq!(
        response_body_limit(&s, &exact(&s, "GET", "targets"), false),
        512 * 1024
    );
    assert_eq!(
        response_body_limit(&s, &exact(&s, "POST", "targets"), false),
        32768
    );
    assert_eq!(
        response_body_limit(&s, &exact(&s, "GET", "targets/extra"), false),
        32768
    );
    assert_eq!(
        response_body_limit(&s, &exact(&s, "GET", "operations"), false),
        32768
    );
    assert_eq!(
        response_body_limit(&s, &exact(&s, "GET", "targets"), true),
        8192
    );
    let mut other_origin = exact(&s, "GET", "targets");
    other_origin.url = "https://untrusted.test/user-service/v1/key-recovery/targets".into();
    assert_eq!(response_body_limit(&s, &other_origin, false), 32768);
}

async fn serve(body: Vec<u8>, chunked: bool) -> (reqwest::Response, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            let mut byte = [0u8; 1];
            if stream.read_exact(&mut byte).await.is_err() {
                return;
            }
            request.push(byte[0]);
            assert!(request.len() < 8192);
        }
        let header = if chunked {
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n".to_owned()
        } else {
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
        };
        if stream.write_all(header.as_bytes()).await.is_err() {
            return;
        }
        if chunked
            && stream
                .write_all(format!("{:x}\r\n", body.len()).as_bytes())
                .await
                .is_err()
        {
            return;
        }
        // An oversized advertised body may be rejected before the peer reads it.
        if stream.write_all(&body).await.is_err() {
            return;
        }
        if chunked {
            let _ = stream.write_all(b"\r\n0\r\n\r\n").await;
        }
    });
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .get(format!("http://{address}/targets"))
        .send()
        .await
        .unwrap();
    (response, server)
}

#[tokio::test]
async fn legal_256_target_discovery_exceeds_old_budget_and_is_read_completely() {
    let s = session();
    let targets = (0..256)
        .map(|i| {
            let mut t = target();
            t.target_id = format!("target-{i}");
            t.full_handle = format!("handle-{i}.control.test");
            t
        })
        .collect();
    let body = serde_json::to_vec(&super::super::wire::TargetsResponse { targets }).unwrap();
    assert!(body.len() > 32768);
    let limit = response_body_limit(&s, &exact(&s, "GET", "targets"), false);
    for chunked in [false, true] {
        let (response, server) = serve(body.clone(), chunked).await;
        let bytes = read_response_bytes(response, limit).await.unwrap();
        assert_eq!(bytes.as_slice(), body.as_slice());
        let decoded: super::super::wire::TargetsResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded.targets.len(), 256);
        server.await.unwrap();
    }
}

#[tokio::test]
async fn every_budget_accepts_exact_limit_and_rejects_one_more_with_both_framings() {
    for limit in [8192, 32768, 512 * 1024] {
        for chunked in [false, true] {
            for extra in [0, 1] {
                let (response, server) = serve(vec![b'x'; limit + extra], chunked).await;
                let result = read_response_bytes(response, limit).await;
                assert_eq!(
                    result.is_ok(),
                    extra == 0,
                    "limit={limit}, chunked={chunked}, extra={extra}"
                );
                server.await.unwrap();
            }
        }
    }
}
