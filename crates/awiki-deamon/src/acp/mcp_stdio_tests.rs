use super::*;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{sse::Event, IntoResponse, Sse},
    routing::post,
    Json, Router,
};
use std::{
    convert::Infallible,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

const AUTH: &str = "Bearer synthetic-question-test-token";

#[derive(Clone, Default)]
struct FixtureState {
    cancelled: Arc<tokio::sync::Notify>,
    requests: Arc<AtomicUsize>,
}

async fn endpoint(
    State(state): State<FixtureState>,
    headers: HeaderMap,
    Json(request): Json<Value>,
) -> axum::response::Response {
    assert_eq!(headers.get("authorization").unwrap(), AUTH);
    state.requests.fetch_add(1, Ordering::SeqCst);
    if request["method"] == "notifications/cancelled" {
        state.cancelled.notify_one();
        return StatusCode::ACCEPTED.into_response();
    }
    if request["method"] == "tools/call" {
        let progress = json!({"jsonrpc":"2.0","method":"notifications/progress","params":{"progressToken":"p-1","progress":0}});
        let stream = futures_util::stream::unfold(0, move |step| {
            let state = state.clone();
            let progress = progress.clone();
            let id = request["id"].clone();
            async move {
                let data = match step {
                    0 => progress,
                    1 => {
                        state.cancelled.notified().await;
                        json!({"jsonrpc":"2.0","id":id,"result":{"action":"cancel"}})
                    }
                    _ => return None,
                };
                Some((
                    Ok::<_, Infallible>(Event::default().data(data.to_string())),
                    step + 1,
                ))
            }
        });
        return Sse::new(stream).into_response();
    }
    Json(json!({"jsonrpc":"2.0","id":request["id"],"result":{"method":request["method"]}}))
        .into_response()
}

async fn fixture() -> (Transport, FixtureState, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    let state = FixtureState::default();
    let app = Router::new()
        .route("/mcp", post(endpoint))
        .with_state(state.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (Transport::new(&url, AUTH).unwrap(), state, server)
}

async fn response(reader: &mut BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>) -> Value {
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(3), reader.read_line(&mut line))
        .await
        .unwrap()
        .unwrap();
    serde_json::from_str(&line).unwrap()
}

#[test]
fn rejects_credential_exfiltration_endpoints() {
    for url in [
        "https://127.0.0.1:32123/mcp",
        "http://localhost:32123/mcp",
        "http://example.com:32123/mcp",
        "http://127.0.0.2:32123/mcp",
        "http://127.0.0.1/mcp",
        "http://127.0.0.1:0/mcp",
        "http://user@127.0.0.1:32123/mcp",
        "http://127.0.0.1:32123/other",
        "http://127.0.0.1:32123/mcp?q=secret",
        "http://127.0.0.1:32123/mcp#fragment",
    ] {
        assert!(Transport::new(url, AUTH).is_err(), "{url}");
    }
    for auth in [
        "",
        "Bearer ",
        "Basic value",
        "Bearer value\nheader",
        "Bearer two words",
    ] {
        assert!(Transport::new("http://127.0.0.1:32123/mcp", auth).is_err());
    }
}

#[test]
fn transport_selection_preserves_http_and_keeps_stdio_credentials_out_of_arguments() {
    let http = json!({"type":"http","name":"awiki_questions","url":"http://127.0.0.1:32123/mcp","headers":[{"name":"Authorization","value":AUTH}]});
    let binary = Path::new("/a path/daemon");
    assert_eq!(
        server_config(&http, &json!({"mcpCapabilities":{"http":true}}), binary).unwrap(),
        http
    );
    let stdio = server_config(&http, &json!({}), binary).unwrap();
    assert_eq!(stdio["command"], "/a path/daemon");
    assert_eq!(stdio["args"], json!(["__acp-mcp-stdio"]));
    assert_eq!(stdio["env"][1]["value"], AUTH);
    assert!(stdio.get("type").is_none());
}

#[test]
fn parses_multiline_sse_without_losing_unicode_or_comments() {
    let mut decoder = EventDecoder::default();
    for line in [
        ": keepalive\r\n",
        "event: message\r\n",
        "data: {\"jsonrpc\":\"2.0\",\r\n",
        "data: \"id\":1,\"result\":\"中文回答\"}\r\n",
    ] {
        assert!(decoder.line(line.as_bytes()).unwrap().is_none());
    }
    assert_eq!(
        decoder.line(b"\r\n").unwrap().unwrap()["result"],
        "中文回答"
    );
    assert!(decoder.line(b"\n").unwrap().is_none());
}

#[test]
fn sse_response_budget_cannot_be_bypassed_with_many_data_lines() {
    let mut decoder = EventDecoder::default();
    let line = [
        b"data: ".to_vec(),
        vec![b'x'; MAX_RESPONSE / 2],
        b"\n".to_vec(),
    ]
    .concat();
    assert!(decoder.line(&line).is_ok());
    assert!(decoder.line(&line).is_err());
}

#[tokio::test]
async fn waiting_question_does_not_block_ping_or_cancellation() {
    let (transport, state, server) = fixture().await;
    let (client, bridge) = tokio::io::duplex(8192);
    let (input, output) = tokio::io::split(bridge);
    let worker = tokio::spawn(serve(input, output, transport));
    let (reader, mut writer) = tokio::io::split(client);
    let mut reader = BufReader::new(reader);
    writer
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"tools/call\"}\n")
        .await
        .unwrap();
    assert_eq!(
        response(&mut reader).await["params"]["progressToken"],
        "p-1"
    );
    writer
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":8,\"method\":\"ping\"}\n")
        .await
        .unwrap();
    assert_eq!(
        response(&mut reader).await,
        json!({"jsonrpc":"2.0","id":8,"result":{"method":"ping"}})
    );
    writer.write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/cancelled\",\"params\":{\"requestId\":7}}\n").await.unwrap();
    assert_eq!(
        response(&mut reader).await,
        json!({"jsonrpc":"2.0","id":7,"result":{"action":"cancel"}})
    );
    assert_eq!(state.requests.load(Ordering::SeqCst), 3);
    writer.shutdown().await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), worker)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    server.abort();
}

#[tokio::test]
async fn eof_cancels_waiting_transport_without_waiting_for_a_human() {
    let (transport, _, server) = fixture().await;
    let (client, bridge) = tokio::io::duplex(8192);
    let (input, output) = tokio::io::split(bridge);
    let worker = tokio::spawn(serve(input, output, transport));
    let (reader, mut writer) = tokio::io::split(client);
    let mut reader = BufReader::new(reader);
    writer
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\"}\n")
        .await
        .unwrap();
    assert_eq!(
        response(&mut reader).await["method"],
        "notifications/progress"
    );
    writer.shutdown().await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), worker)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    server.abort();
}

#[tokio::test]
async fn oversized_unterminated_input_is_bounded() {
    let (transport, state, server) = fixture().await;
    let result = serve(
        &vec![b'x'; MAX_REQUEST + 1][..],
        tokio::io::sink(),
        transport,
    )
    .await;
    assert_eq!(
        result.unwrap_err().to_string(),
        "question_request_too_large"
    );
    assert_eq!(state.requests.load(Ordering::SeqCst), 0);
    server.abort();
}

#[tokio::test]
async fn http_redirect_cannot_forward_the_task_credential() {
    let (destination, state, destination_server) = fixture().await;
    let location = destination.endpoint.to_string();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    let router = Router::new().route(
        "/mcp",
        post(move || async move { (StatusCode::TEMPORARY_REDIRECT, [("location", location)]) }),
    );
    let server = tokio::spawn(async {
        axum::serve(listener, router).await.unwrap();
    });
    let (sender, mut receiver) = mpsc::channel(4);
    let result = Transport::new(&url, AUTH)
        .unwrap()
        .forward(json!({"jsonrpc":"2.0","id":1,"method":"ping"}), &sender)
        .await;
    assert!(result.is_err());
    assert_eq!(state.requests.load(Ordering::SeqCst), 0);
    assert!(receiver.try_recv().is_err());
    server.abort();
    destination_server.abort();
}
