use awiki_im_core::network::{NetworkPolicy, NetworkRuntime};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
use std::time::{Duration, Instant};

#[tokio::main]
async fn main() {
    let runtime = NetworkRuntime::new(None);
    for line in io::stdin().lock().lines() {
        let command: Value =
            serde_json::from_str(&line.expect("fixture input")).expect("fixture JSON");
        if let Some(policy) = command.get("policy").and_then(Value::as_str) {
            runtime.update_policy(match policy {
                "system" => NetworkPolicy::SystemPreferred,
                "direct" => NetworkPolicy::Direct,
                _ => panic!("unknown fixture policy"),
            });
        }
        let started = Instant::now();
        let mut result = json!({});
        if let Some(url) = command.get("url").and_then(Value::as_str) {
            let method = command
                .get("method")
                .and_then(Value::as_str)
                .unwrap_or("GET");
            let body = if method == "POST" {
                b"fixture-write".to_vec()
            } else {
                Vec::new()
            };
            let future = async {
                let response = runtime
                    .send(
                        method,
                        url,
                        [("x-awiki-fixture".into(), "business-only".into())].into(),
                        body,
                        false,
                    )
                    .await
                    .map_err(|_| ())?;
                let status = response.status().as_u16();
                let body = response.text().await.map_err(|_| ())?;
                Ok::<_, ()>((status, body))
            };
            result = match tokio::time::timeout(Duration::from_secs(20), future).await {
                Ok(Ok((status, body))) => json!({"ok": true, "status": status, "body": body}),
                Ok(Err(())) => json!({"ok": false, "error": "transport"}),
                Err(_) => json!({"ok": false, "error": "fixture_deadline"}),
            };
        }
        result["elapsedMs"] = json!(started.elapsed().as_millis());
        result["diagnostics"] = serde_json::to_value(runtime.diagnostics()).unwrap();
        println!("{}", result);
        io::stdout().flush().unwrap();
    }
}
