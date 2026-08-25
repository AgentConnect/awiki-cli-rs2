use super::*;

#[test]
fn did_transition_error_typed_1019_1020_1021_are_closed() {
    let superseded = crate::ImError::Service {
        status_code: None,
        code: Some("1019".to_owned()),
        message: "redacted".to_owned(),
        data: Some(serde_json::json!({
            "json_rpc_code": 1019,
            "anp_code": "anp.did_superseded",
            "retryable": true,
            "details": {
                "requested_did": "did:wba:example.com:users:alice:e1_old",
                "current_did": "did:wba:example.com:users:alice:e1_new"
            }
        })),
    };
    assert!(matches!(
        parse_service_error(&superseded),
        Some(DidTransitionServiceError::Superseded { .. })
    ));

    for (code, reason) in [(1020, "invalid_proof"), (1021, "conflict")] {
        let error = crate::ImError::Service {
            status_code: None,
            code: Some(code.to_string()),
            message: "redacted".to_owned(),
            data: Some(serde_json::json!({
                "json_rpc_code": code,
                "anp_code": if code == 1020 {
                    "anp.did_transition_invalid"
                } else {
                    "anp.did_transition_conflict"
                },
                "retryable": false,
                "details": {"reason": reason}
            })),
        };
        assert!(parse_service_error(&error).is_some());
    }

    let leaked = crate::ImError::Service {
        status_code: None,
        code: Some("1019".to_owned()),
        message: "redacted".to_owned(),
        data: Some(serde_json::json!({
            "json_rpc_code": 1019,
            "anp_code": "anp.did_superseded",
            "retryable": true,
            "details": {
                "requested_did": "did:wba:example.com:users:alice:e1_old",
                "current_did": "did:wba:example.com:users:alice:e1_new",
                "proof": {"secret": true}
            }
        })),
    };
    assert_eq!(parse_service_error(&leaked), None);
}

#[test]
fn did_transition_error_http_409_hint_is_untrusted_until_chain_verifies() {
    let body = serde_json::json!({
        "error": "did_superseded",
        "requestedDid": "did:wba:example.com:users:alice:e1_old",
        "currentDid": "did:wba:example.com:users:alice:e1_new"
    });
    let hint = parse_http_409_hint(409, &body).unwrap();
    assert_eq!(hint.current_did, "did:wba:example.com:users:alice:e1_new");
    assert!(parse_http_409_hint(200, &body).is_none());
}

#[test]
fn did_superseded_retry_verified_successor_rebuilds_once_with_stable_ids() {
    let original = DidBoundRetryRequest {
        message_id: "msg-1".to_owned(),
        operation_id: "op-1".to_owned(),
        target_did: "did:wba:example.com:users:alice:e1_old".to_owned(),
        payload: serde_json::json!({"text": "hello"}),
        did_bound_digest: "digest-old".to_owned(),
        signature: "signature-old".to_owned(),
    };
    let rebuilt = rebuild_once_for_verified_successor(
        &original,
        "did:wba:example.com:users:alice:e1_new",
        |did, _| Ok((format!("digest:{did}"), format!("signature:{did}"))),
    )
    .unwrap();
    assert_eq!(rebuilt.message_id, original.message_id);
    assert_eq!(rebuilt.operation_id, original.operation_id);
    assert_eq!(rebuilt.payload, original.payload);
    assert_ne!(rebuilt.did_bound_digest, original.did_bound_digest);
    assert_ne!(rebuilt.signature, original.signature);
}

#[test]
fn canonical_conversation_did_transition_primitive_carries_no_route_or_conversation() {
    let request = DidBoundRetryRequest {
        message_id: "msg-1".to_owned(),
        operation_id: "op-1".to_owned(),
        target_did: "did:wba:example.com:users:alice:e1_old".to_owned(),
        payload: serde_json::json!({"text": "hello"}),
        did_bound_digest: "digest-old".to_owned(),
        signature: "signature-old".to_owned(),
    };
    let value = format!("{request:?}");
    assert!(!value.contains("conversation_id"));
    assert!(!value.contains("persona"));
    assert!(!value.contains("route"));
}

#[test]
fn did_superseded_retry_attachment_retry_preserves_manifest_and_grant() {
    let payload = serde_json::json!({
        "attachments": [{
            "attachment_id": "att-1",
            "object_uri": "https://objects.example/obj-1",
            "digest": {"alg": "sha-256", "value_b64u": "digest"}
        }],
        "grant": {
            "message_id": "msg-attachment-1",
            "message_target_did": "did:wba:example.com:users:alice:e1_old"
        }
    });
    let original = DidBoundRetryRequest {
        message_id: "msg-attachment-1".to_owned(),
        operation_id: "op-attachment-1".to_owned(),
        target_did: "did:wba:example.com:users:alice:e1_old".to_owned(),
        payload: payload.clone(),
        did_bound_digest: "digest-old".to_owned(),
        signature: "signature-old".to_owned(),
    };

    let rebuilt = rebuild_once_for_verified_successor(
        &original,
        "did:wba:example.com:users:alice:e1_new",
        |did, _| Ok((format!("digest:{did}"), format!("signature:{did}"))),
    )
    .expect("verified successor rebuild");

    assert_eq!(rebuilt.payload, payload);
    assert_eq!(
        rebuilt.payload["attachments"],
        original.payload["attachments"]
    );
    assert_eq!(rebuilt.payload["grant"], original.payload["grant"]);
}
