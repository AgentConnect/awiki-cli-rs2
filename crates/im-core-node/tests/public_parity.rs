struct PublicParity {
    capability: &'static str,
    node_method: &'static str,
    core_facade: &'static str,
}

const PUBLIC_PARITY: &[PublicParity] = &[
    PublicParity {
        capability: "external_http_auth",
        node_method: "prepareExternalHttpRequest",
        core_facade: "ExternalHttpAuthService::prepare_async/handle_response_async",
    },
    PublicParity {
        capability: "identity",
        node_method: "getDefaultIdentity",
        core_facade: "ImCore::identities().default_identity_async",
    },
    PublicParity {
        capability: "registration_otp",
        node_method: "requestRegistrationOtp",
        core_facade: "IdentityRegistry::request_registration_otp_async",
    },
    PublicParity {
        capability: "registration",
        node_method: "completeRegistration",
        core_facade: "IdentityRegistry::register_handle_async",
    },
    PublicParity {
        capability: "profile",
        node_method: "updateDisplayName",
        core_facade: "IdentityService::update_profile_async",
    },
    PublicParity {
        capability: "directory",
        node_method: "resolvePeer",
        core_facade: "DirectoryService::resolve_peer_async",
    },
    PublicParity {
        capability: "display_profiles",
        node_method: "hydrateDisplayProfiles",
        core_facade: "DirectoryService::hydrate_display_profiles_async",
    },
    PublicParity {
        capability: "group_create",
        node_method: "createGroup",
        core_facade: "GroupService::create_async",
    },
    PublicParity {
        capability: "group_member_add",
        node_method: "addGroupMember",
        core_facade: "GroupService::add_member_async",
    },
    PublicParity {
        capability: "sync",
        node_method: "syncNow",
        core_facade: "MessageService::sync_now_async",
    },
    PublicParity {
        capability: "realtime",
        node_method: "startRealtime",
        core_facade: "RealtimeService::start_async / RealtimeSession::subscribe",
    },
    PublicParity {
        capability: "conversation",
        node_method: "listConversations",
        core_facade: "MessageService::conversations_async",
    },
    PublicParity {
        capability: "history",
        node_method: "getHistory",
        core_facade: "MessageService::conversation_history_async",
    },
    PublicParity {
        capability: "local_timeline",
        node_method: "getLocalConversationTimeline",
        core_facade: "MessageService::local_conversation_timeline_async",
    },
    PublicParity {
        capability: "mark_read",
        node_method: "markConversationRead",
        core_facade: "MessageService::mark_conversation_read_async",
    },
    PublicParity {
        capability: "text",
        node_method: "sendText",
        core_facade: "MessageService::send_conversation_text_async",
    },
    PublicParity {
        capability: "attachment_send",
        node_method: "sendAttachment",
        core_facade: "AttachmentService::send_conversation_async",
    },
    PublicParity {
        capability: "attachment_download",
        node_method: "downloadAttachment",
        core_facade: "AttachmentService::download_conversation_async",
    },
    PublicParity {
        capability: "local_reset",
        node_method: "clearLocalData",
        core_facade: "environment lifecycle-owned state root",
    },
    PublicParity {
        capability: "lifecycle",
        node_method: "close",
        core_facade: "environment lifecycle gate",
    },
];

#[test]
fn dsh_required_capabilities_have_one_public_facade_route() {
    let expected = [
        "external_http_auth",
        "identity",
        "registration_otp",
        "registration",
        "profile",
        "directory",
        "display_profiles",
        "group_create",
        "group_member_add",
        "sync",
        "realtime",
        "conversation",
        "history",
        "local_timeline",
        "mark_read",
        "text",
        "attachment_send",
        "attachment_download",
        "local_reset",
        "lifecycle",
    ];
    assert_eq!(
        PUBLIC_PARITY
            .iter()
            .map(|entry| entry.capability)
            .collect::<Vec<_>>(),
        expected
    );
    for entry in PUBLIC_PARITY {
        assert!(!entry.node_method.is_empty());
        assert!(!entry.core_facade.is_empty());
        assert!(!entry.core_facade.contains("internal::"));
        assert!(!entry.core_facade.contains("sqlite"));
        assert!(!entry.core_facade.contains("redb"));
    }
}

#[test]
fn local_timeline_route_uses_only_the_public_local_read_facade() {
    let source = include_str!("../src/client.rs");
    let start = source
        .find("async fn get_local_conversation_timeline_inner")
        .expect("local timeline inner method");
    let tail = &source[start..];
    let end = tail
        .find("    #[napi(catch_unwind)]")
        .expect("next public method boundary");
    let method = &tail[..end];

    assert!(method.contains("local_conversation_timeline_async"));
    for forbidden in [
        "conversation_history_async",
        "sync_now_async",
        "resolve_peer_async",
        "history_async(",
    ] {
        assert!(
            !method.contains(forbidden),
            "local timeline route must not call {forbidden}"
        );
    }
}

#[test]
fn node_facade_source_is_host_neutral_and_uses_no_private_storage_api() {
    let source = concat!(
        include_str!("../src/client.rs"),
        include_str!("../src/dto.rs"),
        include_str!("../src/state.rs"),
    );
    for forbidden in [
        "dsh-awiki",
        "Cordis",
        "deepseek-harness",
        "im_core::internal",
        "rusqlite",
        "redb",
    ] {
        assert!(
            !source.contains(forbidden),
            "Node facade must not contain private or host-specific token {forbidden}"
        );
    }
}
