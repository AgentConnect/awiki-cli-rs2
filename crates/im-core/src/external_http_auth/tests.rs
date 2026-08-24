use std::collections::BTreeMap;

use super::*;

struct Fixture {
    _root: tempfile::TempDir,
    client: crate::ImClient,
    did_document: serde_json::Value,
}

impl Fixture {
    fn new(allow_loopback_http: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let config = crate::ImCoreConfig {
            service_base_url: crate::ServiceEndpoint::parse("https://example.test").unwrap(),
            did_domain: "example.test".to_owned(),
            client_version_info: None,
            user_service_endpoint: None,
            message_service_endpoint: None,
            mail_service_endpoint: None,
            anp_service_endpoint: None,
            anp_service_did: None,
            ca_bundle: None,
            transport_policy: crate::MessageTransportPolicy::HttpOnly,
        };
        let paths = crate::ImCorePaths {
            identities: crate::IdentityRegistryPaths {
                identity_root_dir: root.path().join("identities"),
                registry_path: root.path().join("identities/registry.json"),
                default_identity_path: None,
            },
            local_state: crate::LocalStatePaths {
                sqlite_path: root.path().join("local/im.sqlite"),
            },
            runtime: crate::RuntimePaths {
                cache_dir: root.path().join("cache"),
                temp_dir: root.path().join("tmp"),
            },
        };
        let core = crate::ImCore::new_with_options(
            config,
            paths,
            crate::ImCoreOpenOptions::default()
                .with_external_http_allow_insecure_loopback_for_testing(allow_loopback_http),
        )
        .unwrap();
        let bundle = anp::authentication::create_did_wba_document(
            "example.test",
            anp::authentication::DidDocumentOptions::default(),
        )
        .unwrap();
        let did = bundle.did_document["id"].as_str().unwrap().to_owned();
        let client = core
            .client_with_identity_material(crate::identity::HostedIdentityMaterial {
                identity_id: "external-http-auth-identity".to_owned(),
                did,
                handle: None,
                display_name: None,
                did_document: bundle.did_document.clone(),
                default_signing_private_key_pem: bundle.keys["key-1"].private_key_pem.clone(),
                e2ee_agreement_private_key_pem: None,
                auth_token: None,
            })
            .unwrap();
        Self {
            _root: root,
            client,
            did_document: bundle.did_document,
        }
    }
}

#[test]
fn initial_request_generates_fixed_http_signature_and_body_digest() {
    let fixture = Fixture::new(false);
    let url = "https://api.example.com/orders?view=full";
    let body = br#"{"product_id":"123"}"#.to_vec();
    let original = vec![ExternalHttpHeader::new("Content-Type", "application/json").unwrap()];
    let attempt = fixture
        .client
        .external_http_auth()
        .prepare(
            ExternalHttpRequest::new(url, "POST", original.clone(), Some(body.clone())).unwrap(),
        )
        .unwrap();

    let headers = merged_headers(&original, attempt.header_patch());
    let metadata = anp::authentication::verify_http_message_signature(
        &fixture.did_document,
        "POST",
        url,
        &headers,
        Some(&body),
    )
    .unwrap();
    assert_eq!(
        metadata.components,
        vec!["@method", "@target-uri", "@authority", "content-digest"]
    );
    assert!(header_value(attempt.header_patch(), "Content-Digest").is_some());
    assert!(header_value(attempt.header_patch(), "Authorization").is_none());
    assert_eq!(attempt.retry_count(), 0);
}

#[test]
fn explicitly_empty_body_is_still_digest_bound() {
    let fixture = Fixture::new(false);
    let url = "https://api.example.com/empty";
    let original = vec![ExternalHttpHeader::new("Content-Type", "text/plain").unwrap()];
    let attempt = fixture
        .client
        .external_http_auth()
        .prepare(ExternalHttpRequest::new(url, "POST", original.clone(), Some(Vec::new())).unwrap())
        .unwrap();
    let headers = merged_headers(&original, attempt.header_patch());
    let metadata = anp::authentication::verify_http_message_signature(
        &fixture.did_document,
        "POST",
        url,
        &headers,
        Some(&[]),
    )
    .unwrap();
    assert!(metadata.components.contains(&"content-digest".to_owned()));
    let expected_digest = anp::authentication::build_content_digest(&[]);
    assert_eq!(
        header_value(attempt.header_patch(), "Content-Digest"),
        Some(expected_digest.as_str())
    );
}

#[test]
fn response_token_is_reused_only_for_the_same_normalized_origin() {
    let fixture = Fixture::new(false);
    let service = fixture.client.external_http_auth();
    let initial = service
        .prepare(get("https://api.example.com:443/first"))
        .unwrap();
    assert!(matches!(
        service
            .handle_response(initial, token_response("cached-token", 3600))
            .unwrap(),
        ExternalHttpAuthDecision::Complete
    ));

    let same_origin = service
        .prepare(get("https://api.example.com/second"))
        .unwrap();
    assert_eq!(
        header_value(same_origin.header_patch(), "Authorization"),
        Some("Bearer cached-token")
    );
    let different_port = service
        .prepare(get("https://api.example.com:8443/second"))
        .unwrap();
    assert!(header_value(different_port.header_patch(), "Authorization").is_none());
    assert!(header_value(different_port.header_patch(), "Signature").is_some());
}

#[test]
fn only_successful_authentication_info_bearer_tokens_are_cached() {
    let fixture = Fixture::new(false);
    let service = fixture.client.external_http_auth();

    let authorization_only = service.prepare(get("https://one.example/a")).unwrap();
    service
        .handle_response(
            authorization_only,
            response(
                200,
                vec![ExternalHttpHeader::new("Authorization", "Bearer ignored").unwrap()],
            ),
        )
        .unwrap();
    assert_signature(&service.prepare(get("https://one.example/b")).unwrap());

    let wrong_type = service.prepare(get("https://two.example/a")).unwrap();
    service
        .handle_response(
            wrong_type,
            response(
                200,
                vec![ExternalHttpHeader::new(
                    "Authentication-Info",
                    r#"access_token="ignored", token_type="DPoP", expires_in=3600"#,
                )
                .unwrap()],
            ),
        )
        .unwrap();
    assert_signature(&service.prepare(get("https://two.example/b")).unwrap());

    let error_response = service.prepare(get("https://three.example/a")).unwrap();
    service
        .handle_response(
            error_response,
            token_response_with_status(500, "ignored", 3600),
        )
        .unwrap();
    assert_signature(&service.prepare(get("https://three.example/b")).unwrap());

    let expired = service.prepare(get("https://four.example/a")).unwrap();
    service
        .handle_response(expired, token_response("expired", 0))
        .unwrap();
    assert_signature(&service.prepare(get("https://four.example/b")).unwrap());
}

#[test]
fn bearer_401_compare_and_clear_preserves_a_concurrently_replaced_token() {
    let fixture = Fixture::new(false);
    let service = fixture.client.external_http_auth();
    let url = "https://api.example.com/orders";

    let initial = service.prepare(post(url)).unwrap();
    service
        .handle_response(initial, token_response("token-a", 3600))
        .unwrap();
    let stale = service.prepare(post(url)).unwrap();
    let updater = service.prepare(post(url)).unwrap();
    service
        .handle_response(updater, token_response("token-b", 3600))
        .unwrap();

    let retry = service
        .handle_response(stale, recoverable_401(None))
        .unwrap();
    let ExternalHttpAuthDecision::Retry(retry) = retry else {
        panic!("expected one signature retry");
    };
    assert_signature(&retry);
    let current = service.prepare(post(url)).unwrap();
    assert_eq!(
        header_value(current.header_patch(), "Authorization"),
        Some("Bearer token-b")
    );
}

#[test]
fn signature_401_retries_once_with_fresh_or_server_nonce() {
    let fixture = Fixture::new(false);
    let service = fixture.client.external_http_auth();
    let url = "https://api.example.com/orders";
    let first = service.prepare(post(url)).unwrap();
    let first_nonce = signature_metadata(&fixture, &first).nonce.unwrap();

    let ExternalHttpAuthDecision::Retry(fresh_retry) = service
        .handle_response(first, recoverable_401(None))
        .unwrap()
    else {
        panic!("expected fresh signature retry");
    };
    let fresh_nonce = signature_metadata(&fixture, &fresh_retry).nonce.unwrap();
    assert_ne!(fresh_nonce, first_nonce);
    assert_eq!(fresh_retry.retry_count(), 1);
    assert!(matches!(
        service
            .handle_response(fresh_retry, recoverable_401(None))
            .unwrap(),
        ExternalHttpAuthDecision::Complete
    ));

    let challenged = service.prepare(post(url)).unwrap();
    let ExternalHttpAuthDecision::Retry(challenge_retry) = service
        .handle_response(challenged, recoverable_401(Some("server-nonce-42")))
        .unwrap()
    else {
        panic!("expected challenge signature retry");
    };
    assert_eq!(
        signature_metadata(&fixture, &challenge_retry)
            .nonce
            .as_deref(),
        Some("server-nonce-42")
    );
}

#[test]
fn terminal_or_incompatible_challenges_do_not_retry() {
    let fixture = Fixture::new(false);
    let service = fixture.client.external_http_auth();
    for headers in [
        vec![ExternalHttpHeader::new(
            "WWW-Authenticate",
            r#"DIDWba realm="api.example.com", error="invalid_did""#,
        )
        .unwrap()],
        vec![
            ExternalHttpHeader::new("WWW-Authenticate", r#"Basic realm="api.example.com""#)
                .unwrap(),
        ],
        vec![
            ExternalHttpHeader::new(
                "WWW-Authenticate",
                r#"DIDWba realm="other.example", error="invalid_signature""#,
            )
            .unwrap(),
            fixed_accept_signature(),
        ],
        vec![
            ExternalHttpHeader::new(
                "WWW-Authenticate",
                r#"DIDWba realm="api.example.com", error="invalid_signature""#,
            )
            .unwrap(),
            ExternalHttpHeader::new(
                "Accept-Signature",
                r#"sig1=("@method" "cookie");created;nonce;keyid"#,
            )
            .unwrap(),
        ],
    ] {
        let attempt = service
            .prepare(post("https://api.example.com/orders"))
            .unwrap();
        assert!(matches!(
            service
                .handle_response(attempt, response(401, headers))
                .unwrap(),
            ExternalHttpAuthDecision::Complete
        ));
    }
}

#[test]
fn request_validation_rejects_unsafe_or_ambiguous_inputs_before_signing() {
    let fixture = Fixture::new(false);
    let service = fixture.client.external_http_auth();
    for url in [
        "http://api.example.com/orders",
        "https://user:password@api.example.com/orders",
        "https://api.example.com/orders#fragment",
        "relative/path",
    ] {
        assert!(service.prepare(get(url)).is_err(), "accepted {url}");
    }
    for managed in [
        "Authorization",
        "Signature-Input",
        "Signature",
        "Content-Digest",
    ] {
        assert!(ExternalHttpRequest::new(
            "https://api.example.com/orders",
            "GET",
            vec![ExternalHttpHeader::new(managed, "secret").unwrap()],
            None,
        )
        .is_err());
    }
    assert!(ExternalHttpRequest::new(
        "https://api.example.com/orders",
        "GET",
        vec![
            ExternalHttpHeader::new("X-Test", "one").unwrap(),
            ExternalHttpHeader::new("x-test", "two").unwrap(),
        ],
        None,
    )
    .is_err());
    assert!(ExternalHttpRequest::new(
        "https://api.example.com/orders",
        "POST",
        Vec::new(),
        Some(vec![0; EXTERNAL_HTTP_AUTH_MAX_BODY_BYTES + 1]),
    )
    .is_err());
}

#[test]
fn explicit_test_policy_allows_only_literal_loopback_http() {
    let fixture = Fixture::new(true);
    let service = fixture.client.external_http_auth();
    assert_signature(&service.prepare(get("http://localhost:3000/a")).unwrap());
    assert_signature(&service.prepare(get("http://127.0.0.1:3000/a")).unwrap());
    assert_signature(&service.prepare(get("http://[::1]:3000/a")).unwrap());
    assert!(service.prepare(get("http://api.example.com/a")).is_err());
}

#[test]
fn cloned_client_shares_tokens_but_new_client_lifecycle_does_not() {
    let fixture = Fixture::new(false);
    let url = "https://api.example.com/orders";
    let first = fixture
        .client
        .external_http_auth()
        .prepare(get(url))
        .unwrap();
    fixture
        .client
        .external_http_auth()
        .handle_response(first, token_response("shared", 3600))
        .unwrap();
    let clone = fixture.client.clone();
    assert_eq!(
        header_value(
            clone
                .external_http_auth()
                .prepare(get(url))
                .unwrap()
                .header_patch(),
            "Authorization",
        ),
        Some("Bearer shared")
    );
    fixture
        .client
        .external_http_auth()
        .clear_cached_tokens()
        .unwrap();
    assert_signature(&clone.external_http_auth().prepare(get(url)).unwrap());

    let reopened = Fixture::new(false);
    assert_signature(
        &reopened
            .client
            .external_http_auth()
            .prepare(get(url))
            .unwrap(),
    );
}

#[test]
fn debug_output_redacts_request_body_and_authentication_values() {
    let fixture = Fixture::new(false);
    let attempt = fixture
        .client
        .external_http_auth()
        .prepare(post("https://api.example.com/private?token=query-secret"))
        .unwrap();
    let debug = format!("{attempt:?}");
    assert!(!debug.contains("query-secret"));
    assert!(!debug.contains("hello"));

    let header = ExternalHttpHeader::new("Authorization", "Bearer secret-token").unwrap();
    let debug = format!("{header:?}");
    assert!(!debug.contains("secret-token"));
    assert!(debug.contains("<redacted>"));
}

fn get(url: &str) -> ExternalHttpRequest {
    ExternalHttpRequest::new(url, "GET", Vec::new(), None).unwrap()
}

fn post(url: &str) -> ExternalHttpRequest {
    ExternalHttpRequest::new(
        url,
        "POST",
        vec![ExternalHttpHeader::new("Content-Type", "text/plain").unwrap()],
        Some(b"hello".to_vec()),
    )
    .unwrap()
}

fn response(status: u16, headers: Vec<ExternalHttpHeader>) -> ExternalHttpResponse {
    ExternalHttpResponse::new(status, headers).unwrap()
}

fn token_response(token: &str, expires_in: u64) -> ExternalHttpResponse {
    token_response_with_status(200, token, expires_in)
}

fn token_response_with_status(status: u16, token: &str, expires_in: u64) -> ExternalHttpResponse {
    response(
        status,
        vec![ExternalHttpHeader::new(
            "Authentication-Info",
            format!(r#"access_token="{token}", token_type="Bearer", expires_in={expires_in}"#),
        )
        .unwrap()],
    )
}

fn recoverable_401(nonce: Option<&str>) -> ExternalHttpResponse {
    let nonce = nonce
        .map(|value| format!(r#", nonce="{value}""#))
        .unwrap_or_default();
    response(
        401,
        vec![
            ExternalHttpHeader::new(
                "WWW-Authenticate",
                format!(r#"DIDWba realm="api.example.com", error="invalid_signature"{nonce}"#),
            )
            .unwrap(),
            fixed_accept_signature(),
        ],
    )
}

fn fixed_accept_signature() -> ExternalHttpHeader {
    ExternalHttpHeader::new(
        "Accept-Signature",
        r#"sig1=("@method" "@target-uri" "@authority" "content-digest");created;expires;nonce;keyid"#,
    )
    .unwrap()
}

fn header_value<'a>(headers: &'a [ExternalHttpHeader], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|header| header.name().eq_ignore_ascii_case(name))
        .map(ExternalHttpHeader::value)
}

fn assert_signature(attempt: &ExternalHttpAuthAttempt) {
    assert!(header_value(attempt.header_patch(), "Signature-Input").is_some());
    assert!(header_value(attempt.header_patch(), "Signature").is_some());
    assert!(header_value(attempt.header_patch(), "Authorization").is_none());
}

fn merged_headers(
    original: &[ExternalHttpHeader],
    patch: &[ExternalHttpHeader],
) -> BTreeMap<String, String> {
    original
        .iter()
        .chain(patch)
        .map(|header| (header.name().to_owned(), header.value().to_owned()))
        .collect()
}

fn signature_metadata(
    fixture: &Fixture,
    attempt: &ExternalHttpAuthAttempt,
) -> anp::authentication::SignatureMetadata {
    let originals = attempt
        .request
        .headers
        .iter()
        .map(|(name, value)| ExternalHttpHeader::new(name, value).unwrap())
        .collect::<Vec<_>>();
    let headers = merged_headers(&originals, attempt.header_patch());
    anp::authentication::verify_http_message_signature(
        &fixture.did_document,
        &attempt.request.method,
        &attempt.request.url,
        &headers,
        attempt.request.body.as_deref(),
    )
    .unwrap()
}
