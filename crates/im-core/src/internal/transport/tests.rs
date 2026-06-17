use super::*;
use serde_json::json;

fn assert_transport_unavailable<T>(result: crate::ImResult<T>, expected: &str) {
    match result {
        Err(crate::ImError::TransportUnavailable { detail }) => {
            assert_eq!(detail, expected);
        }
        Err(err) => panic!("expected transport unavailable, got {err:?}"),
        Ok(_) => panic!("expected transport unavailable, got success"),
    }
}

#[tokio::test]
async fn async_unavailable_transport_errors_match_sync_shape() {
    let mut transport = UnavailableTransport;
    assert_transport_unavailable(
        AsyncAuthenticatedRpcTransport::authenticated_rpc(
            &mut transport,
            "/im/rpc",
            "direct.send",
            json!({}),
        )
        .await,
        "direct.send transport is not configured for /im/rpc",
    );
    assert_transport_unavailable(
        AsyncRpcTransport::rpc(
            &mut transport,
            "/user-service/handle/rpc",
            "lookup",
            json!({}),
        )
        .await,
        "lookup transport is not configured for /user-service/handle/rpc",
    );
    assert_transport_unavailable(
        AsyncRestTransport::rest_post(
            &mut transport,
            "/user-service/auth/email-send",
            "POST",
            json!({}),
        )
        .await,
        "POST transport is not configured for /user-service/auth/email-send",
    );
    assert_transport_unavailable(
        AsyncRestTransport::rest_get(
            &mut transport,
            "/user-service/auth/email-status",
            "GET",
            &BTreeMap::new(),
        )
        .await,
        "GET transport is not configured for /user-service/auth/email-status",
    );
    assert_transport_unavailable(
        AsyncAuthenticatedRestTransport::authenticated_rest_post(
            &mut transport,
            "/user-service/did/profile",
            "PATCH",
            json!({}),
        )
        .await,
        "PATCH transport is not configured for /user-service/did/profile",
    );
    assert_transport_unavailable(
        AsyncAuthenticatedRestTransport::authenticated_rest_get(
            &mut transport,
            "/user-service/did/profile",
            "GET",
            &BTreeMap::new(),
        )
        .await,
        "GET transport is not configured for /user-service/did/profile",
    );
    assert_transport_unavailable(
        AsyncRawJsonTransport::get_json_url(
            &mut transport,
            "https://example.test/did.json",
            BTreeMap::new(),
        )
        .await,
        "GET transport is not configured for https://example.test/did.json",
    );
    assert_transport_unavailable(
        AsyncAttachmentObjectTransport::put_attachment_object(
            &mut transport,
            "https://object.test/upload",
            BTreeMap::new(),
            b"body".to_vec(),
        )
        .await,
        "PUT transport is not configured for https://object.test/upload",
    );
    assert_transport_unavailable(
        AsyncAttachmentObjectTransport::get_attachment_object(
            &mut transport,
            "https://object.test/download",
            "ticket",
        )
        .await,
        "GET transport is not configured for https://object.test/download",
    );
}
