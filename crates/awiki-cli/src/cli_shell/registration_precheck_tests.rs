use super::*;

fn response(decision: &str, required: bool, status: &str) -> Value {
    json!({"jsonrpc":"2.0", "id":"registration-check", "error":null,
        "result":{"full_handle":"abc.registration.test", "decision":decision,
            "invite_required":required, "invite_status":status}})
}

#[test]
fn registration_precheck_allows_corrected_invite_and_existing_short_account() {
    for status in ["required", "invalid"] {
        let error =
            evaluate(&response("register", true, status), "abc.registration.test").unwrap_err();
        assert_eq!(error.detail.code, "registration_invite_required");
    }
    assert!(evaluate(
        &response("register", true, "valid"),
        "abc.registration.test"
    )
    .is_ok());
    assert!(evaluate(
        &response("existing", false, "not_required"),
        "abc.registration.test"
    )
    .is_ok());
    assert!(evaluate(
        &response("register", false, "not_required"),
        "abc.registration.test"
    )
    .is_ok());
    assert_eq!(
        evaluate(
            &response("unavailable", false, "not_required"),
            "abc.registration.test"
        )
        .unwrap_err()
        .detail
        .code,
        "handle_unavailable"
    );
}

#[test]
fn registration_precheck_rejects_mismatched_or_contradictory_server_results() {
    let good = response("register", true, "valid");
    for (pointer, replacement) in [
        ("/id", json!("other")),
        ("/jsonrpc", json!("1.0")),
        ("/error", json!({"message":"private server detail"})),
        ("/result/full_handle", json!("other.registration.test")),
        ("/result/decision", json!("unknown")),
        ("/result/invite_status", Value::Null),
        ("/result/invite_required", json!(false)),
    ] {
        let mut value = good.clone();
        *value.pointer_mut(pointer).unwrap() = replacement;
        let error = evaluate(&value, "abc.registration.test").unwrap_err();
        assert_eq!(error.detail.code, "registration_check_unavailable");
        assert!(!error.detail.message.contains("private server detail"));
    }
    assert!(evaluate(
        &response("existing", true, "valid"),
        "abc.registration.test"
    )
    .is_err());
    assert!(evaluate(
        &response("register", true, "not_required"),
        "abc.registration.test"
    )
    .is_err());
    assert!(evaluate(&Value::Null, "abc.registration.test").is_err());
}

#[test]
fn registration_precheck_uses_read_only_rpc_and_accepts_service_null_error() {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut reader = BufReader::new(&stream);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert_eq!(line, "POST /user-service/v1/handle/rpc HTTP/1.1\r\n");
        let mut length = 0;
        let mut client_version = None;
        loop {
            line.clear();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" {
                break;
            }
            let lower = line.to_ascii_lowercase();
            if let Some(value) = lower.strip_prefix("content-length:") {
                length = value.trim().parse::<usize>().unwrap();
            }
            if lower.starts_with("x-awiki-client-version:") {
                client_version = line
                    .split_once(':')
                    .map(|(_, value)| value.trim().to_owned());
            }
        }
        assert_eq!(client_version.as_deref(), Some("awiki-cli/0910/1.2.3"));
        let mut bytes = vec![0; length];
        reader.read_exact(&mut bytes).unwrap();
        let request: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(request["method"], "registration_check");
        assert_eq!(request["params"]["invite_code"], "test-only-invitation");
        assert!(request["params"].get("otp").is_none());
        let body = response("register", true, "valid").to_string();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
    });
    let result = check_with_client_version(
        (
            format!("http://{address}/user-service/v1/handle/rpc"),
            String::new(),
            json!({"handle":"abc","domain":"registration.test","check_invite":true,"invite_code":"test-only-invitation"}),
            "abc.registration.test".to_owned(),
        ),
        Some(im_core::ClientVersionInfo::new("awiki-cli", "0910", "1.2.3", None).unwrap()),
    );
    server.join().unwrap();
    assert!(result.is_ok());
}
