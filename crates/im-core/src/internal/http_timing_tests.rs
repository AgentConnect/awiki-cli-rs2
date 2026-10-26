use super::http_timing_stage;
use serde_json::json;

#[test]
fn http_timing_labels_do_not_expose_arbitrary_input() {
    let private_url = "https://user:secret@example.invalid/private-person/did.json?token=hidden";
    assert_eq!(
        http_timing_stage(private_url, None),
        Some("http.did.resolve")
    );
    assert_eq!(
        http_timing_stage(
            "https://example.invalid/private-token",
            Some(&json!({"method":"secret-identifier"}))
        ),
        Some("http.other")
    );
    assert_eq!(
        http_timing_stage(
            "https://example.invalid/im/rpc",
            Some(&json!({"method":"group.get_info","params":{"token":"secret"}}))
        ),
        Some("http.group.get_info")
    );
}

#[tokio::test]
async fn http_timing_records_failed_attempt_without_sensitive_input() {
    use super::*;
    let runtime = std::sync::Arc::new(crate::network::NetworkRuntime::new(None));
    runtime.update_policy(crate::network::NetworkPolicy::Direct);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/private-path", listener.local_addr().unwrap());
    drop(listener);
    let client = HttpClient::with_runtime(None, false, Some(runtime.clone()));
    let result = client.execute_async(HttpRequest {
        method: "POST".into(), url, headers: BTreeMap::new(),
        body: serde_json::to_vec(&json!({"method":"group.get", "id":"private-operation", "params":{"token":"private-token"}})).unwrap(),
    }).await;
    assert!(result.is_err());
    let diagnostics = runtime.diagnostics();
    assert_eq!(
        diagnostics
            .records
            .iter()
            .filter(|r| r.stage == "http.group.get")
            .count(),
        1
    );
    let serialized = serde_json::to_string(&diagnostics).unwrap();
    assert!(!serialized.contains("private-"));
}
