use super::*;

#[test]
fn absent_recipient_prekey_is_retryable_without_relaxing_invalid_prekeys() {
    let wire_error = crate::internal::json_rpc::decode_response(
        br#"{"jsonrpc":"2.0","id":"fixture","error":{"code":4000,"message":"no available prekey bundle","data":{"anp_code":"anp.direct.e2ee.bundle_not_found"}}}"#,
    )
    .unwrap_err();
    let normalized = map_preflight_error(wire_error);
    assert_eq!(normalized.code, RootTransferErrorCode::PrekeyUnavailable);
    assert!(normalized.retryable);

    let missing = map_preflight_error(crate::ImError::Service {
        status_code: Some(200),
        code: Some("4000".to_owned()),
        message: "no available prekey bundle for the selected device".to_owned(),
        data: None,
    });
    assert_eq!(missing.code, RootTransferErrorCode::PrekeyUnavailable);
    assert!(missing.retryable);

    for code in [
        "4001",
        "4002",
        "4006",
        "1401",
        "1403",
        "anp.direct.e2ee.bundle_invalid",
        "anp.direct.e2ee.bundle_expired",
        "anp.direct.e2ee.session_conflict",
        "anp.unauthorized",
    ] {
        let invalid = map_preflight_error(crate::ImError::Service {
            status_code: Some(200),
            code: Some(code.to_owned()),
            message: "no available prekey bundle for the selected device".to_owned(),
            data: None,
        });
        assert_eq!(invalid.code, RootTransferErrorCode::PrekeyInvalid);
        assert!(!invalid.retryable);
    }
    let invalid = map_preflight_error(crate::ImError::PermissionDenied);
    assert_eq!(invalid.code, RootTransferErrorCode::PrekeyInvalid);
    assert!(!invalid.retryable);
}
