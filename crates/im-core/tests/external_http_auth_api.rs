use awiki_im_core::{
    ExternalHttpAuthDecision, ExternalHttpAuthService, ExternalHttpHeader, ExternalHttpRequest,
    ExternalHttpResponse, ImClient, EXTERNAL_HTTP_AUTH_MAX_BODY_BYTES,
};

#[test]
fn public_external_http_dtos_preserve_the_validated_contract() {
    let request = ExternalHttpRequest::new(
        "https://api.example.test/orders",
        "POST",
        vec![ExternalHttpHeader::new("Content-Type", "application/json").unwrap()],
        Some(br#"{"ok":true}"#.to_vec()),
    );
    assert!(request.is_ok());
    assert!(ExternalHttpResponse::new(
        200,
        vec![ExternalHttpHeader::new("Authentication-Info", "scope=read").unwrap()],
    )
    .is_ok());
    assert_eq!(EXTERNAL_HTTP_AUTH_MAX_BODY_BYTES, 4 * 1024 * 1024);
}

#[test]
fn public_external_http_request_rejects_sdk_managed_headers() {
    let result = ExternalHttpRequest::new(
        "https://api.example.test/orders",
        "GET",
        vec![ExternalHttpHeader::new("Authorization", "Bearer secret").unwrap()],
        None,
    );
    assert!(result.is_err());
}

#[allow(dead_code)]
fn public_service_and_decision_types_are_reachable(client: &ImClient) {
    let _: ExternalHttpAuthService<'_> = client.external_http_auth();
    let _: Option<ExternalHttpAuthDecision> = None;
}
