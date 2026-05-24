#[test]
fn dart_error_unsupported_has_stable_code() {
    let err = awiki_im_core::dto::error::DartImError::unsupported("relationship-remote-mutation");
    assert_eq!(err.code, "unsupported_capability");
    assert_eq!(
        err.capability.as_deref(),
        Some("relationship-remote-mutation")
    );
}

#[test]
fn retry_message_is_explicitly_unsupported_until_im_core_has_retry_api() {
    let err = awiki_im_core::dto::error::DartImError::unsupported("message-retry");
    assert_eq!(err.code, "unsupported_capability");
    assert_eq!(err.capability.as_deref(), Some("message-retry"));
}

#[test]
fn dart_message_security_exposes_target_independent_e2ee_required() {
    let mode = awiki_im_core::dto::message::DartMessageSecurityMode::E2eeRequired;
    let mapped: im_core::messages::MessageSecurityMode = mode.into();
    assert!(matches!(
        mapped,
        im_core::messages::MessageSecurityMode::E2eeRequired
    ));
}

#[test]
fn secure_outbox_entry_does_not_expose_plaintext_or_crypto_material() {
    let entry = awiki_im_core::dto::secure::DartSecureOutboxEntry {
        id: "outbox-1".to_string(),
        target: awiki_im_core::dto::message::DartMessageTarget::Direct {
            peer: "did:example:bob".to_string(),
        },
        message_kind: "text".to_string(),
        status: awiki_im_core::dto::secure::DartSecureOutboxStatus::Failed,
        attempt_count: 2,
        last_error: Some(awiki_im_core::dto::secure::DartSecureProblem {
            code: awiki_im_core::dto::secure::DartSecureProblemCode::PeerKeysUnavailable,
            message: "peer keys unavailable".to_string(),
            retryable: true,
        }),
        created_at: Some("2026-05-24T00:00:00Z".to_string()),
        updated_at: Some("2026-05-24T00:01:00Z".to_string()),
    };

    assert_eq!(entry.id, "outbox-1");
    assert_eq!(entry.message_kind, "text");
    assert_eq!(entry.attempt_count, 2);
}

#[test]
fn realtime_connect_is_explicitly_unsupported_until_bridge_plan_is_ready() {
    let capability = awiki_im_core::dto::realtime::DartRealtimeCapability {
        status_supported: true,
        connect_supported: false,
        runner_exposed: false,
        reason: Some("Dart SDK v0.1 does not expose realtime runner yet".to_string()),
    };
    assert!(!capability.connect_supported);
    assert!(!capability.runner_exposed);
}

#[test]
fn group_create_service_did_is_required_when_no_default_exists() {
    let request = awiki_im_core::dto::group::DartCreateGroupRequest {
        name: "test".to_string(),
        description: None,
        discoverability: None,
        admission_mode: None,
        message_security_profile: None,
        e2ee: false,
        slug: None,
        goal: None,
        rules: None,
        message_prompt: None,
        doc_url: None,
        attachments_allowed: None,
        max_members: None,
        member_max_messages: None,
        member_max_total_chars: None,
        service_did: None,
    };
    let err = request
        .into_core(None)
        .expect_err("missing service_did must fail");
    assert_eq!(err.code, "invalid_input");
    assert_eq!(err.field.as_deref(), Some("service_did"));
}
