//! Real Daemon entrypoints against local fake transports, with no host config or models.
#![cfg(unix)]
use axum::{http::HeaderMap, routing::post, Json, Router};
use serde_json::{json, Value};
use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, Command},
};

const TOKEN: &str = "synthetic-subprocess-test-token";
fn command(home: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_awiki-deamon"));
    command
        .env_clear()
        .env("HOME", home)
        .env("PATH", "")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    command
}
async fn line(reader: &mut (impl AsyncBufReadExt + Unpin)) -> Value {
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut line))
        .await
        .unwrap()
        .unwrap();
    serde_json::from_str(&line).unwrap()
}
async fn exit(mut child: Child) {
    assert!(tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .unwrap()
        .unwrap()
        .success());
}

#[tokio::test]
async fn mcp_hidden_entrypoint_preserves_request_id_authentication_and_exits_on_eof() {
    let home = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/mcp", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, Router::new().route("/mcp", post(|headers: HeaderMap, Json(request): Json<Value>| async move {
            assert_eq!(headers["authorization"], format!("Bearer {TOKEN}"));
            Json(json!({"jsonrpc":"2.0", "id":request["id"], "result":{"observed":request["method"]}}))
        }))).await.unwrap();
    });
    let mut child = command(home.path())
        .arg("__acp-mcp-stdio")
        .env("AWIKI_ACP_MCP_ENDPOINT", endpoint)
        .env("AWIKI_ACP_MCP_AUTHORIZATION", format!("Bearer {TOKEN}"))
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    for (id, method) in [
        (json!(1), "initialize"),
        (json!("question-2"), "tools/list"),
    ] {
        input
            .write_all(format!("{}\n", json!({"jsonrpc":"2.0","id":id,"method":method})).as_bytes())
            .await
            .unwrap();
        let response = line(&mut output).await;
        assert_eq!(response["id"], id);
        assert_eq!(response["result"]["observed"], method);
        assert!(!response.to_string().contains(TOKEN));
    }
    drop(input);
    exit(child).await;
    server.abort();
}

#[tokio::test]
async fn background_app_action_entrypoint_forwards_stdin_json_with_environment_token() {
    // /tmp keeps the Unix socket path within the macOS limit.
    let home = tempfile::tempdir_in("/tmp").unwrap();
    let socket = home.path().join("rpc.sock");
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
            .await
            .unwrap()
            .unwrap();
        let mut stream = BufReader::new(stream);
        let request = line(&mut stream).await;
        stream
            .get_mut()
            .write_all(b"{\"ok\":true,\"result\":{\"accepted\":true}}\n")
            .await
            .unwrap();
        request
    });
    let params = json!({"action":"message.create_draft","source_message_id":"source-1","conversation_id":"conversation-1","args":{"draft_text":"review this draft"}});
    let mut child = command(home.path())
        .args(["__runtime-wrapper", "app-action"])
        .env("AWIKI_DAEMON_RPC_SOCKET", &socket)
        .env("AWIKI_RUNTIME_RPC_TOKEN", TOKEN)
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    input
        .write_all(params.to_string().as_bytes())
        .await
        .unwrap();
    drop(input);
    let output = tokio::time::timeout(Duration::from_secs(5), child.wait_with_output())
        .await
        .unwrap()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let request = server.await.unwrap();
    assert_eq!(request["method"], "app.action.request");
    assert_eq!(request["runtime_rpc_token"], TOKEN);
    assert_eq!(request["params"], params);
    assert!(!String::from_utf8_lossy(&output.stdout).contains(TOKEN));
    assert!(!String::from_utf8_lossy(&output.stderr).contains(TOKEN));
}
