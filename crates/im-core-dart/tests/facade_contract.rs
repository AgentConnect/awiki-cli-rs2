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
fn group_create_bridge_request_no_longer_accepts_per_request_service_did() {
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
    };
    let core = request
        .into_core()
        .expect("service DID is resolved by ImCoreConfig at create time");
    assert_eq!(core.name, "test");
    assert!(core.discoverability.is_none());
}
