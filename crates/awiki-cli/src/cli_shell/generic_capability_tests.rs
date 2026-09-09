use super::*;
#[test]
fn generic_http_target_is_explicit_and_not_tenant_specific() {
    let mut input = RequestInput {
        origin: "https://api.example".into(),
        method: "PATCH".into(),
        path: "/invoices/42?view=full".into(),
        headers: vec![],
        include_client_metadata: false,
        body_base64: None,
    };
    assert!(target(&input).is_ok());
    for bad in [
        "https://other.example/",
        "//other.example/x",
        "/x#fragment",
        "/\\other.example",
    ] {
        input.path = bad.into();
        assert!(target(&input).is_err());
    }
    for bad in [
        "http://api.example",
        "https://a:b@api.example",
        "https://api.example/path",
    ] {
        assert!(origin(bad).is_err());
    }
}
#[test]
fn signing_envelope_has_no_business_fields_or_scripted_confirmation() {
    let input = serde_json::json!({"object_json":"{}","object_hash":"hash"});
    assert!(serde_json::from_value::<SignInput>(input.clone()).is_ok());
    for field in ["confirmed", "tenant_id", "markdown", "operation_id"] {
        let mut bad = input.clone();
        bad[field] = true.into();
        assert!(serde_json::from_value::<SignInput>(bad).is_err());
    }
}
#[test]
fn generic_http_cannot_inject_sdk_owned_auth_headers() {
    for name in [
        "Host",
        "Content-Length",
        "Transfer-Encoding",
        "Connection",
        "Proxy-Authorization",
    ] {
        assert!(ordinary_headers(vec![HeaderInput {
            name: name.into(),
            value: "injected".into()
        }])
        .is_err());
    }
    for name in [
        "Authorization",
        "Signature",
        "Signature-Input",
        "Content-Digest",
        "X-AWiki-Client-Version",
    ] {
        let h = ExternalHttpHeader::new(name, "injected").unwrap();
        assert!(ExternalHttpRequest::new("https://api.example/", "GET", vec![h], None).is_err());
    }
}
