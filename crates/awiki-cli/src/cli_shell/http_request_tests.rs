use super::*;
#[test]
fn http_request_target_and_headers_do_not_escape_authority_or_inject_credentials() {
    let input = |path: &str| RequestInput {
        origin: "https://admin.example.org".into(),
        method: "DELETE".into(),
        path: path.into(),
        headers: vec![],
        include_client_metadata: false,
        body_base64: None,
    };
    assert_eq!(
        target(&input("/v1/tenants/tenant/websites/site?after=1"))
            .unwrap()
            .host_str(),
        Some("admin.example.org")
    );
    for path in [
        "https://evil.example/",
        "//evil.example/",
        "/\\evil.example/",
        "/#fragment",
    ] {
        assert!(target(&input(path)).is_err());
    }
    for name in [
        "authorization",
        "Signature",
        "signature-input",
        "content-digest",
        "x-awiki-client-version",
        "Host",
        "Cookie",
        "Content-Length",
        "Transfer-Encoding",
        "Proxy-Authorization",
        "Forwarded",
    ] {
        assert!(
            ordinary_headers(vec![HeaderInput {
                name: name.into(),
                value: "untrusted".into()
            }])
            .is_err(),
            "{name}"
        );
    }
    assert!(ordinary_headers(vec![HeaderInput {
        name: "content-type".into(),
        value: "application/json".into()
    }])
    .is_ok());
    assert!(serde_json::from_str::<RequestInput>(
        r#"{"origin":"https://example.org","method":"GET","path":"/","private_key":"x"}"#
    )
    .is_err());
}
