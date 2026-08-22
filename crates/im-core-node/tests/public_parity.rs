struct PublicParity {
    capability: &'static str,
    node_method: &'static str,
    core_facade: &'static str,
}

const PUBLIC_PARITY: &[PublicParity] = &[
    PublicParity {
        capability: "identity",
        node_method: "getDefaultIdentity",
        core_facade: "ImCore::identities().default_identity_async",
    },
    PublicParity {
        capability: "registration_otp",
        node_method: "requestRegistrationOtp",
        core_facade: "IdentityRegistry::register_handle_async(OtpSent)",
    },
    PublicParity {
        capability: "registration",
        node_method: "completeRegistration/completeRegistrationWithOutcome",
        core_facade: "IdentityRegistry::register_handle_async",
    },
    PublicParity {
        capability: "profile",
        node_method: "getProfile/updateProfile",
        core_facade: "IdentityService::profile_async/update_profile_async",
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
        capability: "group_read",
        node_method: "getGroup/listGroups",
        core_facade: "GroupService::get_async/list_async",
    },
    PublicParity {
        capability: "group_lifecycle",
        node_method: "joinGroup/leaveGroup",
        core_facade: "GroupService::join_async/leave_async",
    },
    PublicParity {
        capability: "group_members",
        node_method: "listGroupMembers/removeGroupMember",
        core_facade: "GroupService::members_async/remove_member_async",
    },
    PublicParity {
        capability: "group_rebind_recovery",
        node_method: "resumeGroupRebindRecovery",
        core_facade: "GroupService::resume_rebind_recovery_async",
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
        core_facade: "MessageService::local_conversation_timeline_async",
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
        capability: "payload",
        node_method: "sendPayload",
        core_facade: "MessageService::send_conversation_payload_async",
    },
    PublicParity {
        capability: "attachment_send",
        node_method: "sendAttachment",
        core_facade: "AttachmentService::send_conversation_async",
    },
    PublicParity {
        capability: "attachment_download",
        node_method: "downloadAttachment",
        core_facade: "AttachmentService::download_async",
    },
    PublicParity {
        capability: "mail_account",
        node_method: "getMailAccount",
        core_facade: "EmailService::account_async",
    },
    PublicParity {
        capability: "mail_inbox",
        node_method: "listMailInbox",
        core_facade: "EmailService::inbox_async",
    },
    PublicParity {
        capability: "mail_read",
        node_method: "readMail",
        core_facade: "EmailService::read_async",
    },
    PublicParity {
        capability: "mail_mark_read",
        node_method: "markMailRead",
        core_facade: "EmailService::mark_read_async",
    },
    PublicParity {
        capability: "mail_send",
        node_method: "sendMail",
        core_facade: "EmailService::send_async",
    },
    PublicParity {
        capability: "handle_recovery",
        node_method: "request/prepare/activate/status/resume/discardHandleRecovery",
        core_facade: "HandleRecoveryService public state machine",
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
        "identity",
        "registration_otp",
        "registration",
        "profile",
        "directory",
        "display_profiles",
        "group_create",
        "group_member_add",
        "group_read",
        "group_lifecycle",
        "group_members",
        "group_rebind_recovery",
        "sync",
        "realtime",
        "conversation",
        "history",
        "local_timeline",
        "mark_read",
        "text",
        "payload",
        "attachment_send",
        "attachment_download",
        "mail_account",
        "mail_inbox",
        "mail_read",
        "mail_mark_read",
        "mail_send",
        "handle_recovery",
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

#[test]
fn external_provider_bridge_is_async_binary_explicit_and_has_no_raw_secret_fallback() {
    let rust = include_str!("../src/external_identity.rs");
    assert!(rust.contains("ThreadsafeFunction"));
    assert!(rust.contains("call_async_catch"));
    for forbidden in ["block_on", "block_in_place", "shared_secret.to_vec()"] {
        assert!(
            !rust.contains(forbidden),
            "External Provider bridge must not use {forbidden}"
        );
    }

    let start = rust
        .find("async fn derive_shared_secret")
        .expect("External ECDH method");
    let method = &rust[start
        ..rust[start..]
            .find("async fn recover")
            .expect("next provider method")
            + start];
    assert!(method.contains("CapabilityUnavailable"));
    assert!(!method.contains("call("));

    let typescript = include_str!("../../../packages/awiki-im-core-node/src/provider-bridge.ts");
    assert!(typescript.contains("singleBuffer(request.buffers)"));
    assert!(!typescript.contains("ecdhSealed"));
    assert!(!typescript.contains("exportRootKeySealed"));
}
