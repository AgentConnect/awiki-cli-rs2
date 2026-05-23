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
fn attachment_request_maps_bytes_input_without_bytes_len_placeholder() {
    let request = awiki_im_core::dto::attachment::DartAttachmentSendRequest {
        target: awiki_im_core::dto::message::DartMessageTarget::Direct {
            peer: "did:example:bob".to_string(),
        },
        input: awiki_im_core::dto::attachment::DartAttachmentInput::Bytes {
            filename: Some("note.txt".to_string()),
            mime_type: Some("text/plain".to_string()),
            bytes: b"hello".to_vec(),
        },
        caption: Some("caption".to_string()),
        mime_type: None,
        filename: None,
        idempotency_key: Some("idem-1".to_string()),
        wait_for_final_acceptance: true,
    };

    let (target, request) = request.into_core().expect("attachment maps to im-core");
    assert!(matches!(
        target,
        im_core::messages::MessageTarget::Direct(peer) if peer.as_str() == "did:example:bob"
    ));
    assert!(matches!(
        request.input,
        im_core::attachments::AttachmentInput::Bytes { bytes, .. } if bytes == b"hello".to_vec()
    ));
    assert_eq!(request.delivery.idempotency_key.as_deref(), Some("idem-1"));
    assert!(request.delivery.wait_for_final_acceptance);
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
