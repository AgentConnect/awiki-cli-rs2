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
        None
    );
    assert_eq!(
        http_timing_stage(
            "https://example.invalid/im/rpc",
            Some(&json!({"method":"group.get_info","params":{"token":"secret"}}))
        ),
        Some("http.group.get_info")
    );
}
