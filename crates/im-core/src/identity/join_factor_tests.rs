use super::*;

#[test]
fn email_factor_is_exact_target_bound_and_legacy_verification_is_not_used() {
    let body = factor_body(" Alice@Example.com ", "alice.example.com").unwrap();
    assert_eq!(
        body,
        json!({"email":"alice@example.com","purpose":"awiki.device.join.v1","target_handle":"alice","target_handle_domain":"example.com"})
    );
    for email in [
        "",
        "@example.com",
        "alice@",
        "a@@example.com",
        "a b@example.com",
    ] {
        assert!(factor_body(email, "alice.example.com").is_err());
    }
    assert!(factor_body("alice@example.com", "Alice.example.com").is_err());
}

#[test]
fn factor_request_debug_redacts_contact_and_code() {
    let request = EmailDeviceJoinRequest {
        email: "private@example.com".into(),
        code: "123456".into(),
        full_handle: "alice.example.com".into(),
        did: crate::ids::Did::parse("did:wba:example.com:alice").unwrap(),
        operation_id: "join-1".into(),
        ttl_seconds: 600,
    };
    let debug = format!("{request:?}");
    assert!(!debug.contains("private"));
    assert!(!debug.contains("123456"));
}
