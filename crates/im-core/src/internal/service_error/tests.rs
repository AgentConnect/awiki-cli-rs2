use serde_json::json;

fn service_error(code: &str, data: serde_json::Value) -> crate::ImError {
    crate::ImError::Service {
        status_code: None,
        code: Some(code.to_owned()),
        message: "invalid target binding".to_owned(),
        data: Some(data),
    }
}

#[test]
fn classifies_stable_public_stale_binding_code() {
    let error = service_error(
        "anp.invalid_target_binding",
        json!({
            "reason": "stale_did",
            "json_rpc_code": 1406,
            "current_did": "did:example:bob-current",
            "full_handle": "bob.awiki.test"
        }),
    );

    let hint = super::stale_target_binding_from_error(&error, "did:example:alice").unwrap();
    assert_eq!(hint.current_did.as_deref(), Some("did:example:bob-current"));
    assert_eq!(hint.full_handle.as_deref(), Some("bob.awiki.test"));
}

#[test]
fn classifies_numeric_json_rpc_compatibility_code() {
    let error = service_error(
        "service.invalid_target",
        json!({"reason": "stale_did", "json_rpc_code": "1406"}),
    );

    assert!(super::stale_target_binding_from_error(&error, "did:example:alice").is_some());
}

#[test]
fn classifies_legacy_numeric_public_code() {
    let error = service_error(
        "1406",
        json!({"reason": "stale_did", "full_handle": "bob.awiki.test"}),
    );

    assert!(super::stale_target_binding_from_error(&error, "did:example:alice").is_some());
}

#[test]
fn rejects_wrong_reason_and_unrelated_service_errors() {
    for error in [
        service_error(
            "anp.invalid_target_binding",
            json!({"reason": "proof_mismatch", "json_rpc_code": 1406}),
        ),
        service_error(
            "anp.rate_limited",
            json!({"reason": "stale_did", "json_rpc_code": 429}),
        ),
    ] {
        assert!(super::stale_target_binding_from_error(&error, "did:example:alice").is_none());
    }
}

#[test]
fn invalid_or_owner_current_did_is_only_an_ignored_hint() {
    for current_did in ["not-a-did", "did:example:alice"] {
        let error = service_error(
            "anp.invalid_target_binding",
            json!({
                "reason": "stale_did",
                "current_did": current_did,
                "full_handle": "bob.awiki.test"
            }),
        );
        let hint = super::stale_target_binding_from_error(&error, "did:example:alice").unwrap();
        assert_eq!(hint.current_did, None);
        assert_eq!(hint.full_handle.as_deref(), Some("bob.awiki.test"));
    }
}
