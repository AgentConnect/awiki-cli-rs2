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
fn dart_profile_mapping_keeps_account_and_wns_versions_independent() {
    let mut core =
        im_core::identity::Profile::new(im_core::ids::Did::parse("did:example:alice").unwrap());
    core.agent_kind = Some("skill".to_owned());
    core.agent_capabilities = vec!["group_membership_v1".to_owned()];
    core.profile_version = Some("18446744073709551616".to_owned());
    core.version_id = Some("wns-profile-7".to_owned());

    let mapped = awiki_im_core::dto::profile::DartUserProfile::from(core);

    assert_eq!(
        mapped.profile_version.as_deref(),
        Some("18446744073709551616")
    );
    assert_eq!(mapped.version_id.as_deref(), Some("wns-profile-7"));
    assert_eq!(mapped.agent_kind.as_deref(), Some("skill"));
    assert_eq!(mapped.agent_capabilities, ["group_membership_v1"]);
}

#[test]
fn dart_handle_recovery_mapping_preserves_closed_progress_and_reset_reference() {
    let mapped = awiki_im_core::dto::identity::DartHandleRecoveryProgress::from(
        im_core::identity::HandleRecoveryProgress {
            operation_id: "operation-1".to_owned(),
            owner_identity_id: im_core::ids::IdentityId::parse("owner-1").unwrap(),
            account_user_id: Some("user-1".to_owned()),
            full_handle: "alice.awiki.info".to_owned(),
            local_previous_did: Some(
                im_core::ids::Did::parse("did:wba:awiki.info:users:alice-old").unwrap(),
            ),
            current_did: im_core::ids::Did::parse("did:wba:awiki.info:users:alice-new").unwrap(),
            binding_generation: Some("8".to_owned()),
            state_root_fingerprint: Some("sha256:test".to_owned()),
            phase: im_core::identity::HandleRecoveryPhase::QuarantinedKeyUnavailable,
            impact: im_core::identity::HandleRecoveryImpact {
                local_ordinary_data_will_migrate: true,
                other_devices_must_rejoin: true,
                unsupported_e2ee_group_count: 2,
                unsupported_did_only_group_count: 3,
            },
            reset_reference: Some(im_core::identity::HandleRecoveryResetReference {
                account_user_id: "user-1".to_owned(),
                owner_identity_id: "owner-1".to_owned(),
                previous_did: im_core::ids::Did::parse("did:wba:awiki.info:users:alice-old")
                    .unwrap(),
                current_did: im_core::ids::Did::parse("did:wba:awiki.info:users:alice-new")
                    .unwrap(),
                binding_generation: "8".to_owned(),
                handle: "alice.awiki.info".to_owned(),
                source_kind: im_core::identity::HandleRecoveryTransitionSourceKind::Initiator,
                source_id: "operation-1".to_owned(),
            }),
            failure_code: Some(im_core::identity::HandleRecoveryErrorCode::LocalKeyUnavailable),
        },
    );

    assert_eq!(
        mapped.phase,
        awiki_im_core::dto::identity::DartHandleRecoveryPhase::QuarantinedKeyUnavailable
    );
    assert_eq!(
        mapped.failure_code,
        Some(awiki_im_core::dto::identity::DartHandleRecoveryErrorCode::LocalKeyUnavailable)
    );
    let reset = mapped.reset_reference.unwrap();
    assert_eq!(reset.owner_identity_id, "owner-1");
    assert_eq!(reset.binding_generation, "8");
    assert_eq!(reset.source_id, "operation-1");
    assert_eq!(mapped.impact.unsupported_e2ee_group_count, 2);
}

#[test]
fn dart_legacy_epoch_adoption_authority_mapping_preserves_opaque_provenance() {
    let mapped = awiki_im_core::dto::identity::DartLegacyRegistryEpochAdoptionAuthority::from(
        im_core::identity::LegacyRegistryEpochAdoptionAuthority {
            owner_identity_id: im_core::ids::IdentityId::parse("owner-1").unwrap(),
            account_user_id: "user-1".to_owned(),
            current_did: im_core::ids::Did::parse("did:wba:awiki.info:users:alice").unwrap(),
            binding_generation: "8".to_owned(),
            protocol_device_id: im_core::ids::ProtocolDeviceId::parse("device-1").unwrap(),
            device_auth_generation: "1".to_owned(),
            provenance_id: "sha256:opaque-local-authority".to_owned(),
        },
    );

    assert_eq!(mapped.owner_identity_id, "owner-1");
    assert_eq!(mapped.protocol_device_id, "device-1");
    assert_eq!(mapped.device_auth_generation, "1");
    assert_eq!(mapped.provenance_id, "sha256:opaque-local-authority");
}

#[test]
fn dart_message_sync_diagnostics_mapping_is_typed_and_redacted() {
    let mapped = awiki_im_core::dto::message::DartMessageSyncDiagnostics::from(
        im_core::messages::MessageSyncDiagnostics {
            last_success_at: Some("2026-07-29T00:00:00Z".to_owned()),
            mode: im_core::messages::MessageSyncMode::Retryable,
            pending_mutation_count: 2,
            dirty_domains: vec![
                im_core::messages::MessageSyncDirtyDomain::Messages,
                im_core::messages::MessageSyncDirtyDomain::ReadState,
            ],
            retry_state: im_core::messages::MessageSyncRetryState::Scheduled,
            next_retry_at: Some("2026-07-29T00:01:00Z".to_owned()),
        },
    );

    assert_eq!(
        mapped.mode,
        awiki_im_core::dto::message::DartMessageSyncMode::Retryable
    );
    assert_eq!(mapped.pending_mutation_count, 2);
    assert_eq!(
        mapped.retry_state,
        awiki_im_core::dto::message::DartMessageSyncRetryState::Scheduled
    );
    assert_eq!(mapped.dirty_domains.len(), 2);
}

#[test]
fn dart_device_revoke_outcome_is_structured() {
    let error =
        awiki_im_core::dto::error::DartImError::from(im_core::ImError::DeviceRevokeOutcome {
            category: im_core::DeviceRevokeOutcomeCategory::OutcomeUnknown,
        });
    assert_eq!(error.code, "device_revoke_outcome");
    assert_eq!(
        error.device_revoke_outcome_category,
        Some(awiki_im_core::dto::error::DartDeviceRevokeOutcomeCategory::OutcomeUnknown)
    );
}

#[test]
fn dart_group_read_result_preserves_member_page_metadata_as_strings() {
    let core: im_core::groups::GroupReadResult = serde_json::from_value(serde_json::json!({
        "group": null,
        "groups": [],
        "members": [],
        "resolved_member": null,
        "messages": {"items":[],"next_cursor":null,"has_more":false},
        "total": 0,
        "next_cursor": "opaque-page-2",
        "has_more": true,
        "page_group": "did:example:group",
        "group_state_version": "9007199254740993",
        "source": null,
        "warnings": []
    }))
    .unwrap();
    let mapped = awiki_im_core::dto::group::DartGroupReadResult::from(core);
    assert!(mapped.has_more);
    assert_eq!(mapped.next_cursor.as_deref(), Some("opaque-page-2"));
    assert_eq!(mapped.page_group_did.as_deref(), Some("did:example:group"));
    assert_eq!(
        mapped.group_state_version.as_deref(),
        Some("9007199254740993")
    );
    assert!(mapped.messages.next_cursor.is_none());
}

#[test]
fn root_key_transfer_result_mapping_exposes_delivery_metadata_only() {
    let mapped = awiki_im_core::dto::identity::DartRootKeyTransferSendResult::from(
        im_core::identity::RootKeyTransferSendResult {
            did: im_core::ids::Did::parse("did:example:alice").unwrap(),
            sender_device_id: im_core::ids::ProtocolDeviceId::parse("device-admin").unwrap(),
            recipient_device_id: im_core::ids::ProtocolDeviceId::parse("device-member").unwrap(),
            message_id: im_core::ids::MessageId::parse("root-transfer-message-1").unwrap(),
            accepted_at: "2026-07-20T01:00:00Z".to_owned(),
        },
    );

    assert_eq!(mapped.message_id, "root-transfer-message-1");
    let debug = format!("{mapped:?}");
    assert!(!debug.contains("root_private_key"));
    assert!(!debug.contains("transport_context"));
    assert!(!debug.contains("completion"));
}

#[test]
fn root_key_transfer_preparation_mapping_redacts_opaque_handle() {
    let handle = serde_json::from_value(serde_json::Value::String(
        "iIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIg".to_owned(),
    ))
    .unwrap();
    let mapped = awiki_im_core::dto::identity::DartRootKeyTransferPreparation::from(
        im_core::identity::RootKeyTransferPreparation {
            authorization_handle: handle,
            recipient: im_core::identity::RootKeyTransferRecipientSummary {
                did: im_core::ids::Did::parse("did:example:alice").unwrap(),
                device_id: im_core::ids::ProtocolDeviceId::parse("device-member").unwrap(),
                signing_key_id: "did:example:alice#device-member-signing".to_owned(),
                e2ee_key_id: "did:example:alice#device-member-e2ee".to_owned(),
                registry_version: 7,
            },
            expires_at: "2026-07-20T01:00:00Z".to_owned(),
        },
    );

    assert_eq!(mapped.recipient.device_id, "device-member");
    assert_eq!(mapped.recipient.registry_version, 7);
    let debug = format!("{mapped:?}");
    assert!(debug.contains("<redacted-authorization-handle>"));
    assert!(!debug.contains("iIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIg"));
    assert!(!debug.contains("root_private_key"));
}

#[test]
fn root_key_transfer_error_mapping_is_closed_and_secret_free() {
    let mapped = awiki_im_core::dto::identity::DartRootKeyTransferError::from(
        im_core::identity::RootKeyTransferError {
            code: im_core::identity::RootKeyTransferErrorCode::PrekeyUnavailable,
            retryable: true,
        },
    );

    assert_eq!(mapped.code, "root_transfer.prekey_unavailable");
    assert!(mapped.retryable);
    assert_eq!(
        format!("{mapped:?}"),
        "DartRootKeyTransferError { code: \"root_transfer.prekey_unavailable\", retryable: true }"
    );
}

#[test]
fn group_secure_repair_mapping_preserves_device_reconciliation_counts() {
    let mapped = awiki_im_core::dto::secure::DartGroupSecureRepairResult::from(
        im_core::secure::GroupSecureRepairResult {
            group: im_core::ids::GroupRef::parse("did:example:group").unwrap(),
            state: im_core::secure::GroupSecureState::NeedsRepair,
            repaired: true,
            added_devices: 2,
            removed_devices: 3,
            remaining_devices: 1,
            problem: None,
            warnings: Vec::new(),
        },
    );

    assert_eq!(mapped.added_devices, 2);
    assert_eq!(mapped.removed_devices, 3);
    assert_eq!(mapped.remaining_devices, 1);
}

#[test]
fn local_state_upgrade_inspection_is_available_before_core_open() {
    let directory = tempfile::tempdir().unwrap();
    let inspection = awiki_im_core::api::local_state_upgrade::inspect_local_state_upgrade(
        awiki_im_core::dto::config::DartImCorePaths {
            identity_root_dir: directory.path().join("identities").display().to_string(),
            registry_path: directory.path().join("registry.json").display().to_string(),
            default_identity_path: None,
            sqlite_path: directory.path().join("im.sqlite").display().to_string(),
            cache_dir: directory.path().join("cache").display().to_string(),
            temp_dir: directory.path().join("tmp").display().to_string(),
        },
    )
    .expect("fresh state inspection");

    assert_eq!(
        inspection.eligibility,
        awiki_im_core::dto::local_state_upgrade::DartLocalStateUpgradeEligibility::NotRequired
    );
    assert_eq!(inspection.source_schema_version, 0);
    assert_eq!(
        inspection.target_schema_version,
        im_core::compat::local_state::SCHEMA_VERSION
    );
}

#[test]
fn local_state_restore_result_preserves_recovery_evidence_for_dart() {
    let dart: awiki_im_core::dto::local_state_upgrade::DartLocalStateRestoreResult =
        im_core::LocalStateRestoreResult {
            restored_schema_version: 27,
            target_safety_copy_available: true,
        }
        .into();

    assert_eq!(dart.restored_schema_version, 27);
    assert!(dart.target_safety_copy_available);
}

#[test]
fn identity_vault_failures_have_stable_redacted_dart_codes() {
    let cases = [
        (
            im_core::IdentityVaultFailure::Unavailable,
            "identity_vault_unavailable",
        ),
        (
            im_core::IdentityVaultFailure::MetadataMissing,
            "identity_vault_metadata_missing",
        ),
        (
            im_core::IdentityVaultFailure::MetadataUnverified,
            "identity_vault_metadata_unverified",
        ),
        (
            im_core::IdentityVaultFailure::WorkspaceMismatch,
            "identity_vault_workspace_mismatch",
        ),
        (
            im_core::IdentityVaultFailure::DeviceMismatch,
            "identity_vault_device_mismatch",
        ),
        (
            im_core::IdentityVaultFailure::RecordOpenFailed,
            "identity_vault_record_open_failed",
        ),
        (
            im_core::IdentityVaultFailure::VerificationFailed,
            "identity_vault_verification_failed",
        ),
    ];

    for (failure, expected_code) in cases {
        let error = awiki_im_core::dto::error::DartImError::from(im_core::ImError::IdentityVault {
            failure,
        });
        assert_eq!(error.code, expected_code);
        assert_eq!(
            error.message,
            format!("identity vault failure: {expected_code}")
        );
        assert!(!error.message.contains("root"));
        assert!(!error.message.contains("SecretRef"));
        assert!(!error.message.contains("private"));
    }
}

#[test]
fn retry_message_is_explicitly_unsupported_until_im_core_has_retry_api() {
    let err = awiki_im_core::dto::error::DartImError::unsupported("message-retry");
    assert_eq!(err.code, "unsupported_capability");
    assert_eq!(err.capability.as_deref(), Some("message-retry"));
}

#[test]
fn service_error_preserves_server_code_and_data_for_dart() {
    let err = awiki_im_core::dto::error::DartImError::from(im_core::ImError::Service {
        status_code: Some(409),
        code: Some("1007".to_string()),
        message: "target did is inactive".to_string(),
        data: Some(serde_json::json!({
            "did": "did:example:old",
            "handle": "alice",
        })),
    });

    assert_eq!(err.code, "service_error");
    assert_eq!(err.status_code, Some(409));
    assert_eq!(err.service_code.as_deref(), Some("1007"));
    assert_eq!(
        err.service_data_json.as_deref(),
        Some(r#"{"did":"did:example:old","handle":"alice"}"#)
    );
}

#[test]
fn skill_onboarding_error_exposes_only_stable_redacted_fields() {
    let secret = "awsk1_must_not_cross_the_bridge";
    let err = awiki_im_core::dto::error::DartImError::from(im_core::ImError::SkillOnboarding {
        code: "token_expired".to_owned(),
        phase: "preflight".to_owned(),
        retryable: false,
    });

    assert_eq!(err.code, "skill_onboarding_error");
    assert_eq!(err.message, "Skill onboarding failed during preflight");
    assert_eq!(err.service_code.as_deref(), Some("token_expired"));
    assert_eq!(
        err.service_data_json.as_deref(),
        Some(r#"{"phase":"preflight","retryable":false}"#)
    );
    assert_eq!(err.device_revoke_outcome_category, None);
    assert!(!err.message.contains(secret));
    assert!(!err.service_data_json.as_deref().unwrap().contains(secret));
}

#[test]
fn thread_mark_read_result_preserves_best_effort_state_for_dart() {
    let core = im_core::messages::MarkThreadReadResult {
        updated_count: 1,
        remote_acknowledged: false,
        partial: true,
        fallback_used: true,
        pending_remote_ack: true,
        effective_watermark: Some(im_core::messages::ReadWatermark {
            last_read_message_id: Some(
                im_core::ids::MessageId::parse("msg-1").expect("message id"),
            ),
            last_read_thread_seq: Some("42".to_string()),
            read_at: None,
        }),
        legacy_message_ids: vec![im_core::ids::MessageId::parse("msg-1").expect("message id")],
        warnings: vec!["Remote read-state mark-read failed".to_string()],
    };

    let dart: awiki_im_core::dto::message::DartMarkThreadReadResult = core.into();

    assert_eq!(dart.updated_count, 1);
    assert!(!dart.remote_acknowledged);
    assert!(dart.partial);
    assert!(dart.fallback_used);
    assert!(dart.pending_remote_ack);
    assert_eq!(dart.legacy_message_ids, vec!["msg-1"]);
    let watermark = dart.effective_watermark.expect("effective watermark");
    assert_eq!(watermark.last_read_message_id.as_deref(), Some("msg-1"));
    assert_eq!(watermark.last_read_thread_seq.as_deref(), Some("42"));
    assert_eq!(dart.warnings, vec!["Remote read-state mark-read failed"]);
}

#[test]
fn sync_delta_request_exposes_only_app_safe_controls() {
    let request = awiki_im_core::dto::message::DartSyncDeltaRequest {
        limit: Some(100),
        device_id: Some("device-main".to_string()),
        reason: Some("app_resumed".to_string()),
    };

    let core: im_core::messages::SyncDeltaRequest = request.into();
    assert_eq!(core.limit, Some(100));
    assert_eq!(core.device_id.as_deref(), Some("device-main"));
    assert_eq!(core.reason.as_deref(), Some("app_resumed"));
}

#[test]
fn sync_delta_result_preserves_diagnostics_without_next_checkpoint_setter() {
    let core = im_core::messages::SyncDeltaResult {
        events_applied: 3,
        pages_fetched: 2,
        last_applied_event_seq: Some("42".to_string()),
        has_more: false,
        snapshot_required: true,
        retention_floor_event_seq: Some("10".to_string()),
        warnings: vec!["snapshot required".to_string()],
    };

    let dart: awiki_im_core::dto::message::DartSyncDeltaResult = core.into();
    assert_eq!(dart.events_applied, 3);
    assert_eq!(dart.pages_fetched, 2);
    assert_eq!(dart.last_applied_event_seq.as_deref(), Some("42"));
    assert!(!dart.has_more);
    assert!(dart.snapshot_required);
    assert_eq!(dart.retention_floor_event_seq.as_deref(), Some("10"));
    assert_eq!(dart.warnings, vec!["snapshot required"]);
}

#[test]
fn sync_now_bridge_exposes_only_high_level_v2_outcome() {
    let request = awiki_im_core::dto::message::DartMessageSyncRequest {
        reason: "websocket_hint".to_owned(),
        limit: Some(100),
    };
    let request_debug = format!("{request:?}");
    assert!(request_debug.contains("websocket_hint"));
    for forbidden in [
        "account_id",
        "device_id",
        "cursor",
        "stream_epoch",
        "scan_seq",
        "recovery_id",
        "token",
        "snapshot_scan_seq",
        "server_cutoff",
        "message_limit",
        "returned_logical_messages",
    ] {
        assert!(!request_debug.contains(forbidden));
    }

    let core = im_core::messages::MessageSyncOutcome {
        status: im_core::messages::MessageSyncStatus::RecoveryRequired,
        events_applied: 0,
        pages_fetched: 1,
        messages_hydrated: 0,
        duplicates_skipped: 0,
        changed_conversation_ids: Vec::new(),
        committed_incoming_messages: Vec::new(),
        error_code: Some("SYNC_RECOVERY_REQUIRED".to_owned()),
        warnings: vec!["recovery details remain Core-private".to_owned()],
    };
    let dart: awiki_im_core::dto::message::DartMessageSyncOutcome = core.into();
    assert!(matches!(
        dart.status,
        awiki_im_core::dto::message::DartMessageSyncStatus::RecoveryRequired
    ));
    assert_eq!(dart.pages_fetched, 1);
    assert_eq!(dart.error_code.as_deref(), Some("SYNC_RECOVERY_REQUIRED"));
    assert!(dart.committed_incoming_messages.is_empty());
}

#[test]
fn registration_recovery_join_sync_prepares_secure_inbox_before_ordinary_sync() {
    let native = include_str!("../../../packages/awiki_im_core/lib/src/awiki_im_core_native.dart");
    let sync_now = native
        .split("Future<MessageSyncOutcome> syncNow(MessageSyncRequest request)")
        .nth(1)
        .expect("native syncNow wrapper");
    let prepare = sync_now
        .find("prepareSecureInboxForSync")
        .expect("closed secure Inbox preparation");
    let ordinary = sync_now
        .find("gen_messages.syncNow")
        .expect("ordinary message sync");
    assert!(prepare < ordinary);
    assert!(sync_now.contains("...secureWarnings"));

    let facade = include_str!("../src/api/messages.rs");
    let prepare = facade
        .split("pub async fn prepare_secure_inbox_for_sync")
        .nth(1)
        .expect("Rust-Dart secure Inbox preparation");
    assert!(prepare.contains("hydrate_exact_device_secure_inbox_async"));
    assert!(prepare.contains("IdentitySelector::Id(identity_id.clone())"));
    assert!(prepare.contains("client.replace_inner(&identity_id, refreshed)"));
}

#[test]
fn sync_thread_after_request_uses_thread_local_sequence_only() {
    let request = awiki_im_core::dto::message::DartSyncThreadAfterRequest {
        thread: awiki_im_core::dto::message::DartThreadRef::Direct {
            peer: "did:example:bob".to_string(),
        },
        after_server_seq: Some("991".to_string()),
        limit: Some(50),
    };

    let core: im_core::messages::SyncThreadAfterRequest =
        request.try_into().expect("sync thread-after maps");
    assert!(matches!(
        core.thread,
        im_core::messages::ThreadRef::Direct(peer) if peer.as_str() == "did:example:bob"
    ));
    assert_eq!(core.after_server_seq.as_deref(), Some("991"));
    assert_eq!(core.limit, Some(50));
}

#[test]
fn sync_conversation_after_request_uses_canonical_conversation_ref() {
    let request = awiki_im_core::dto::message::DartSyncConversationAfterRequest {
        conversation: awiki_im_core::dto::message::DartConversationReadRef {
            conversation_id: "dm:peer-scope:v1:abc".to_string(),
        },
        after_server_seq: Some("992".to_string()),
        limit: Some(25),
    };

    let core: im_core::messages::SyncConversationAfterRequest =
        request.try_into().expect("sync conversation-after maps");
    assert_eq!(core.conversation.conversation_id, "dm:peer-scope:v1:abc");
    assert!(matches!(
        core.conversation.as_thread_ref().expect("conversation thread ref"),
        im_core::messages::ThreadRef::Thread(thread)
            if thread.as_str() == "dm:peer-scope:v1:abc"
    ));
    assert_eq!(core.after_server_seq.as_deref(), Some("992"));
    assert_eq!(core.limit, Some(25));
}

#[test]
fn sync_thread_after_result_preserves_ordered_message_page_metadata() {
    let peer_scope_thread_id =
        im_core::messages::direct_peer_scope_thread_id("user-bob", "bob.example")
            .expect("peer-scope thread id");
    let conversation_identity = im_core::messages::ConversationIdentity::from_thread_ref_for_owner(
        &im_core::messages::ThreadRef::Thread(peer_scope_thread_id),
        "did:example:alice",
    );
    let core = im_core::messages::SyncThreadAfterResult {
        messages: vec![im_core::messages::Message {
            id: im_core::ids::MessageId::parse("msg-1").expect("message id"),
            thread: im_core::messages::ThreadRef::Direct(
                im_core::ids::PeerRef::parse("did:example:bob", "example.com").expect("peer ref"),
            ),
            direction: im_core::messages::MessageDirection::Incoming,
            sender: im_core::ids::PeerRef::parse("did:example:bob", "example.com")
                .expect("sender ref"),
            receiver: Some(
                im_core::ids::PeerRef::parse("did:example:alice", "example.com")
                    .expect("receiver ref"),
            ),
            group: None,
            body: im_core::messages::MessageBodyView::Text {
                text: "hello".to_string(),
                kind: im_core::messages::MessageKind::Text,
            },
            sent_at: None,
            received_at: None,
            metadata: im_core::messages::MessageMetadata {
                server_sequence: Some(992),
                conversation_identity: Some(conversation_identity),
                attributes: vec![im_core::messages::MessageMetadataAttribute {
                    key: "sender_peer_persona_id".to_owned(),
                    value: "persona:v1:bob".to_owned(),
                }],
                ..Default::default()
            },
        }],
        next_after_server_seq: Some("992".to_string()),
        has_more: false,
        warnings: vec![],
    };

    let dart: awiki_im_core::dto::message::DartSyncThreadAfterResult = core.into();
    assert_eq!(dart.messages.len(), 1);
    assert_eq!(dart.messages[0].id, "msg-1");
    assert!(dart.messages[0]
        .conversation_id
        .starts_with("dm:peer-scope:v1:"));
    assert_eq!(
        dart.messages[0].sender_peer_persona_id.as_deref(),
        Some("persona:v1:bob")
    );
    assert_eq!(dart.messages[0].sender_did_snapshot, "did:example:bob");
    assert_eq!(dart.messages[0].metadata.server_sequence, Some(992));
    let identity = dart.messages[0]
        .metadata
        .conversation_identity
        .as_ref()
        .expect("conversation identity");
    assert!(identity.conversation_id.starts_with("dm:peer-scope:v1:"));
    assert_eq!(identity.canonical_thread_kind, "direct");
    assert_eq!(identity.canonical_thread_id, identity.conversation_id);
    assert_eq!(identity.storage_thread_ref.kind, "thread");
    assert!(identity
        .storage_thread_ref
        .id
        .starts_with("dm:peer-scope:v1:"));
    assert_eq!(
        identity.identity_scope,
        awiki_im_core::dto::message::DartConversationIdentityScope::Direct
    );
    assert_eq!(
        identity.migration_state,
        awiki_im_core::dto::message::DartConversationMigrationState::Canonical
    );
    assert!(identity.aliases.is_empty());
    assert_eq!(dart.next_after_server_seq.as_deref(), Some("992"));
    assert!(!dart.has_more);
}

#[test]
fn conversation_store_patches_are_keyed_only_by_canonical_conversation_id() {
    let remove = im_core::messages::ConversationStorePatch::Remove {
        owner_identity_id: "identity-1".to_string(),
        owner_did: "did:example:alice".to_string(),
        version: 7,
        unread_total: 0,
        conversation_id: "dm:persona:bob".to_string(),
    };
    let reorder = im_core::messages::ConversationStorePatch::Reorder {
        owner_identity_id: "identity-1".to_string(),
        owner_did: "did:example:alice".to_string(),
        version: 8,
        unread_total: 0,
        conversation_id: "dm:persona:bob".to_string(),
        index: 1,
    };

    let remove: awiki_im_core::dto::message::DartConversationStorePatch = remove.into();
    let reorder: awiki_im_core::dto::message::DartConversationStorePatch = reorder.into();

    match remove {
        awiki_im_core::dto::message::DartConversationStorePatch::Remove {
            conversation_id, ..
        } => {
            assert_eq!(conversation_id, "dm:persona:bob");
        }
        other => panic!("expected remove patch, got {other:?}"),
    }

    match reorder {
        awiki_im_core::dto::message::DartConversationStorePatch::Reorder {
            conversation_id,
            index,
            ..
        } => {
            assert_eq!(index, 1);
            assert_eq!(conversation_id, "dm:persona:bob");
        }
        other => panic!("expected reorder patch, got {other:?}"),
    }
}

#[test]
fn local_history_query_is_public_core_contract_for_dart_facade() {
    let query = im_core::messages::LocalHistoryQuery {
        limit: im_core::ids::PageLimit::new(50).expect("limit"),
        cursor: Some(im_core::ids::Cursor::parse("local-history:v1:dHM:bXNn").expect("cursor")),
    };

    assert_eq!(query.limit.0, 50);
    assert_eq!(
        query.cursor.as_ref().map(im_core::ids::Cursor::as_str),
        Some("local-history:v1:dHM:bXNn")
    );
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
        mention_payload_json: Some(
            serde_json::json!({
                "text": "@Hermes caption",
                "mentions": [{
                    "id": "men_agent",
                    "range": {"start": 0, "end": 7, "unit": "unicode_code_point"},
                    "target": {"kind": "agent", "did": "did:agent:hermes"},
                    "mention_role": "addressee"
                }]
            })
            .to_string(),
        ),
        mime_type: None,
        filename: None,
        security: awiki_im_core::dto::message::DartMessageSecurityMode::E2eeRequired,
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
    assert_eq!(
        request
            .mention_payload
            .as_ref()
            .and_then(|payload| payload.get("mentions"))
            .and_then(serde_json::Value::as_array)
            .map(Vec::len),
        Some(1)
    );
    assert!(request.delivery.wait_for_final_acceptance);
    assert!(matches!(
        request.security,
        im_core::messages::MessageSecurityMode::E2eeRequired
    ));
}

#[test]
fn conversation_attachment_request_maps_to_core_conversation_contract() {
    let request = awiki_im_core::dto::attachment::DartSendConversationAttachmentRequest {
        conversation: awiki_im_core::dto::message::DartConversationReadRef {
            conversation_id: "dm:did:example:bob".to_string(),
        },
        input: awiki_im_core::dto::attachment::DartAttachmentInput::Bytes {
            filename: Some("note.txt".to_string()),
            mime_type: Some("text/plain".to_string()),
            bytes: b"hello".to_vec(),
        },
        caption: Some("caption".to_string()),
        mention_payload_json: Some(
            serde_json::json!({
                "text": "@Hermes caption",
                "mentions": [{
                    "id": "men_agent",
                    "range": {"start": 0, "end": 7, "unit": "unicode_code_point"},
                    "target": {"kind": "agent", "did": "did:agent:hermes"},
                    "mention_role": "addressee"
                }]
            })
            .to_string(),
        ),
        mime_type: Some("text/plain".to_string()),
        filename: None,
        security: awiki_im_core::dto::message::DartMessageSecurityMode::DefaultPlain,
        client_message_id: Some("msg-client-attachment".to_string()),
        idempotency_key: Some("op-client-attachment".to_string()),
        wait_for_final_acceptance: true,
    };

    let core: im_core::attachments::SendConversationAttachmentRequest =
        request.try_into().expect("conversation attachment maps");
    assert_eq!(core.conversation.conversation_id, "dm:did:example:bob");
    assert!(matches!(
        core.input,
        im_core::attachments::AttachmentInput::Bytes { bytes, .. } if bytes == b"hello".to_vec()
    ));
    assert_eq!(
        core.client_message_id
            .as_ref()
            .map(im_core::ids::MessageId::as_str),
        Some("msg-client-attachment")
    );
    assert_eq!(
        core.idempotency_key.as_deref(),
        Some("op-client-attachment")
    );
    assert!(core.wait_for_final_acceptance);
    assert_eq!(
        core.mention_payload
            .as_ref()
            .and_then(|payload| payload.get("mentions"))
            .and_then(serde_json::Value::as_array)
            .map(Vec::len),
        Some(1)
    );
}

#[test]
fn attachment_send_result_preserves_upload_metadata_for_dart() {
    let core = im_core::attachments::AttachmentSendResult {
        message: im_core::messages::SendMessageResult {
            message: im_core::messages::Message {
                id: im_core::ids::MessageId::parse("msg-1").expect("message id"),
                thread: im_core::messages::ThreadRef::Direct(
                    im_core::ids::PeerRef::parse("did:example:bob", "example.com")
                        .expect("peer ref"),
                ),
                direction: im_core::messages::MessageDirection::Outgoing,
                sender: im_core::ids::PeerRef::parse("did:example:alice", "example.com")
                    .expect("sender ref"),
                receiver: Some(
                    im_core::ids::PeerRef::parse("did:example:bob", "example.com")
                        .expect("receiver ref"),
                ),
                group: None,
                body: im_core::messages::MessageBodyView::Unsupported {
                    content_type: Some(
                        im_core::attachments::attachment_manifest_content_type().to_string(),
                    ),
                },
                sent_at: Some("2026-05-24T00:00:00Z".to_string()),
                received_at: None,
                metadata: im_core::messages::MessageMetadata::default(),
            },
            delivery: im_core::messages::DeliveryState::Sent,
            warnings: vec!["message warning".to_string()],
        },
        target_kind: "direct".to_string(),
        target_did: "did:example:bob".to_string(),
        attachment: im_core::attachments::UploadedAttachment {
            attachment_id: "att-1".to_string(),
            filename: "note.txt".to_string(),
            mime_type: "text/plain".to_string(),
            size_bytes: 5,
            size: "5".to_string(),
            digest_b64u: "digest".to_string(),
            object_uri: "object://att-1".to_string(),
            object_encryption_mode: "object-e2ee".to_string(),
            plaintext_size_bytes: Some(4),
        },
        manifest: serde_json::json!({
            "attachments": [{
                "attachment_id": "att-1",
                "filename": "note.txt",
                "encryption_info": {
                    "mode": "object-e2ee",
                    "object_cipher": "chacha20-poly1305",
                    "plaintext_size": "4"
                }
            }]
        }),
    };

    let dart: awiki_im_core::dto::attachment::DartAttachmentSendResult = core.into();
    assert_eq!(dart.message.message.id, "msg-1");
    assert_eq!(dart.message.delivery_state, "sent");
    assert_eq!(dart.message.warnings, vec!["message warning"]);
    assert_eq!(dart.target_kind, "direct");
    assert_eq!(dart.target_did, "did:example:bob");
    assert_eq!(dart.attachment.attachment_id, "att-1");
    assert_eq!(dart.attachment.filename, "note.txt");
    assert_eq!(dart.attachment.mime_type, "text/plain");
    assert_eq!(dart.attachment.size_bytes, 5);
    assert_eq!(dart.attachment.size, "5");
    assert_eq!(dart.attachment.digest_b64u, "digest");
    assert_eq!(dart.attachment.object_uri, "object://att-1");
    assert_eq!(dart.attachment.object_encryption_mode, "object-e2ee");
    assert_eq!(dart.attachment.plaintext_size_bytes, Some(4));

    let manifest: serde_json::Value =
        serde_json::from_str(&dart.manifest_json).expect("manifest json is preserved");
    assert_eq!(manifest["attachments"][0]["attachment_id"], "att-1");
    assert_eq!(
        manifest["attachments"][0]["encryption_info"]["mode"],
        "object-e2ee"
    );
    assert_eq!(
        manifest["attachments"][0]["encryption_info"].get("object_key_b64u"),
        None
    );
    assert_eq!(
        manifest["attachments"][0]["encryption_info"].get("nonce_b64u"),
        None
    );
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
fn payload_request_and_body_view_preserve_json_for_dart() {
    let request = awiki_im_core::dto::message::DartSendPayloadRequest {
        target: awiki_im_core::dto::message::DartMessageTarget::Direct {
            peer: "did:example:bob".to_string(),
        },
        payload_json: r#"{"schema":"awiki.agent.command.v1","command":"runtime.agent.create"}"#
            .to_string(),
        security: awiki_im_core::dto::message::DartMessageSecurityMode::Plain,
        client_message_id: None,
        idempotency_key: Some("op-payload".to_string()),
        wait_for_final_acceptance: true,
        delegated_signing: None,
    };

    let core: im_core::messages::SendMessageRequest =
        request.try_into().expect("payload request maps");
    assert!(matches!(
        core.body,
        im_core::messages::MessageBody::Payload { ref payload }
            if payload["schema"] == "awiki.agent.command.v1"
                && payload["command"] == "runtime.agent.create"
    ));
    assert_eq!(core.delivery.idempotency_key.as_deref(), Some("op-payload"));
    assert!(core.delivery.wait_for_final_acceptance);

    let dart_body: awiki_im_core::dto::message::DartMessageBodyView =
        im_core::messages::MessageBodyView::Payload {
            payload: serde_json::json!({
                "schema": "awiki.agent.status.v1",
                "state": "running"
            }),
        }
        .into();
    assert!(dart_body.text.is_none());
    assert_eq!(dart_body.kind.as_deref(), Some("payload"));
    let payload: serde_json::Value =
        serde_json::from_str(dart_body.payload_json.as_deref().unwrap()).unwrap();
    assert_eq!(payload["schema"], "awiki.agent.status.v1");
    assert_eq!(payload["state"], "running");
    assert!(dart_body.unsupported_content_type.is_none());
    assert!(dart_body.agent_message.is_none());
}

#[test]
fn agent_message_projection_is_typed_and_invalid_never_exposes_raw_json() {
    use awiki_im_core::dto::message::{
        DartAgentMessageAction, DartAgentMessageKind, DartAgentMessageProjectionState,
        DartAgentMessageRequestedLevel,
    };

    let valid_message: awiki_im_core::dto::message::DartMessage = im_core::messages::Message {
        id: im_core::ids::MessageId::parse("msg-agent-valid").unwrap(),
        thread: im_core::messages::ThreadRef::Direct(
            im_core::ids::PeerRef::parse("did:example:agent", "").unwrap(),
        ),
        direction: im_core::messages::MessageDirection::Incoming,
        sender: im_core::ids::PeerRef::parse("did:example:agent", "").unwrap(),
        receiver: None,
        group: None,
        body: im_core::messages::MessageBodyView::Payload {
            payload: serde_json::json!({
                "schema": "awiki.agent.message.v1",
                "event_id": "event-001",
                "task_name": "Release verification",
                "kind": "task_result",
                "level": "urgent",
                "content": {"summary": "Build completed", "detail": "Checks passed."},
                "action": {"type": "open_conversation"}
            }),
        },
        sent_at: None,
        received_at: Some("2026-08-11T00:00:05Z".to_owned()),
        metadata: im_core::messages::MessageMetadata::default(),
    }
    .into();
    assert_eq!(
        valid_message.authoritative_received_at.as_deref(),
        Some("2026-08-11T00:00:05Z")
    );
    let valid = valid_message.body;
    assert_eq!(valid.kind.as_deref(), Some("agent_message"));
    assert!(valid.payload_json.is_none());
    let projection = valid.agent_message.expect("typed projection");
    assert_eq!(projection.state, DartAgentMessageProjectionState::Valid);
    let message = projection.message.expect("valid message fields");
    assert_eq!(message.event_id, "event-001");
    assert_eq!(message.task_name, "Release verification");
    assert_eq!(message.kind, DartAgentMessageKind::TaskResult);
    assert_eq!(
        message.requested_level,
        DartAgentMessageRequestedLevel::Urgent
    );
    assert_eq!(message.summary, "Build completed");
    assert_eq!(message.detail.as_deref(), Some("Checks passed."));
    assert_eq!(message.action, DartAgentMessageAction::OpenConversation);

    let invalid: awiki_im_core::dto::message::DartMessageBodyView =
        im_core::messages::MessageBodyView::Payload {
            payload: serde_json::json!({
                "schema": "awiki.agent.message.v1",
                "content": {"summary": "token=must-not-cross"},
                "raw": "/private/secret"
            }),
        }
        .into();
    assert_eq!(invalid.kind.as_deref(), Some("agent_message"));
    assert!(invalid.payload_json.is_none());
    let projection = invalid.agent_message.expect("invalid projection");
    assert_eq!(projection.state, DartAgentMessageProjectionState::Invalid);
    assert!(projection.message.is_none());
    assert!(!format!("{projection:?}").contains("must-not-cross"));
    assert!(!format!("{projection:?}").contains("private"));
}

#[test]
fn group_agent_message_payload_is_forced_to_invalid_visible_projection() {
    use awiki_im_core::dto::message::DartAgentMessageProjectionState;

    let group = im_core::ids::GroupRef::parse("did:example:group").unwrap();
    let mapped: awiki_im_core::dto::message::DartMessage = im_core::messages::Message {
        id: im_core::ids::MessageId::parse("msg-group-agent-schema").unwrap(),
        thread: im_core::messages::ThreadRef::Group(group.clone()),
        direction: im_core::messages::MessageDirection::Incoming,
        sender: im_core::ids::PeerRef::parse("did:example:agent", "").unwrap(),
        receiver: None,
        group: Some(group),
        body: im_core::messages::MessageBodyView::Payload {
            payload: serde_json::json!({
                "schema": "awiki.agent.message.v1",
                "event_id": "event-group-001",
                "task_name": "Group task",
                "kind": "alert",
                "level": "urgent",
                "content": {"summary": "Must not drive a Group urgent card"},
                "action": {"type": "open_conversation"}
            }),
        },
        sent_at: None,
        received_at: None,
        metadata: im_core::messages::MessageMetadata::default(),
    }
    .into();
    let projection = mapped.body.agent_message.expect("exact schema projection");
    assert_eq!(projection.state, DartAgentMessageProjectionState::Invalid);
    assert!(projection.message.is_none());
    assert!(mapped.body.payload_json.is_none());
}

#[test]
fn e2ee_agent_message_payload_is_forced_to_invalid_visible_projection() {
    use awiki_im_core::dto::message::DartAgentMessageProjectionState;

    let mapped: awiki_im_core::dto::message::DartMessage = im_core::messages::Message {
        id: im_core::ids::MessageId::parse("msg-e2ee-agent-schema").unwrap(),
        thread: im_core::messages::ThreadRef::Direct(
            im_core::ids::PeerRef::parse("did:example:agent", "").unwrap(),
        ),
        direction: im_core::messages::MessageDirection::Incoming,
        sender: im_core::ids::PeerRef::parse("did:example:agent", "").unwrap(),
        receiver: None,
        group: None,
        body: im_core::messages::MessageBodyView::Payload {
            payload: serde_json::json!({
                "schema": "awiki.agent.message.v1",
                "event_id": "event-e2ee-001",
                "task_name": "E2EE task",
                "kind": "alert",
                "level": "urgent",
                "content": {"summary": "Must not drive an E2EE urgent card"},
                "action": {"type": "open_conversation"}
            }),
        },
        sent_at: None,
        received_at: Some("2026-08-11T00:00:05Z".to_owned()),
        metadata: im_core::messages::MessageMetadata {
            attributes: vec![im_core::messages::MessageMetadataAttribute {
                key: "security".to_owned(),
                value: "direct-e2ee".to_owned(),
            }],
            ..im_core::messages::MessageMetadata::default()
        },
    }
    .into();
    let projection = mapped.body.agent_message.expect("exact schema projection");
    assert_eq!(projection.state, DartAgentMessageProjectionState::Invalid);
    assert!(projection.message.is_none());
    assert!(mapped.body.payload_json.is_none());
}

#[test]
fn snapshot_agent_message_preserves_authoritative_time_and_rejects_e2ee() {
    use awiki_im_core::dto::message::DartAgentMessageProjectionState;

    let mapped: awiki_im_core::dto::message::DartConversationSnapshotMessage =
        im_core::messages::ConversationSnapshotMessage {
            id: "msg-snapshot-e2ee-agent".to_owned(),
            thread_kind: "direct".to_owned(),
            thread_id: "did:example:agent".to_owned(),
            conversation_identity: None,
            direction: "incoming".to_owned(),
            sender: "did:example:agent".to_owned(),
            receiver: None,
            group: None,
            body: im_core::messages::ConversationSnapshotMessageBody {
                text: None,
                kind: Some("payload".to_owned()),
                payload_json: Some(
                    serde_json::json!({
                        "schema": "awiki.agent.message.v1",
                        "event_id": "event-snapshot-e2ee-001",
                        "task_name": "Snapshot task",
                        "kind": "alert",
                        "level": "urgent",
                        "content": {"summary": "Must stay generic"},
                        "action": {"type": "open_conversation"}
                    })
                    .to_string(),
                ),
                unsupported_content_type: None,
            },
            sent_at: Some("2026-08-11T00:00:00Z".to_owned()),
            received_at: Some("2026-08-11T00:00:05Z".to_owned()),
            server_sequence: Some(1),
            content_type: Some("application/json".to_owned()),
            attributes: vec![im_core::messages::MessageMetadataAttribute {
                key: "security".to_owned(),
                value: "direct-e2ee".to_owned(),
            }],
        }
        .into();

    assert_eq!(
        mapped.authoritative_received_at.as_deref(),
        Some("2026-08-11T00:00:05Z")
    );
    let projection = mapped
        .body
        .agent_message
        .expect("exact schema remains visible generic");
    assert_eq!(projection.state, DartAgentMessageProjectionState::Invalid);
    assert!(projection.message.is_none());
    assert!(mapped.body.payload_json.is_none());
}

#[test]
fn committed_incoming_mapping_preserves_core_authoritative_received_time() {
    let message = im_core::messages::Message {
        id: im_core::ids::MessageId::parse("msg-authoritative-time").unwrap(),
        thread: im_core::messages::ThreadRef::Direct(
            im_core::ids::PeerRef::parse("did:example:agent", "").unwrap(),
        ),
        direction: im_core::messages::MessageDirection::Incoming,
        sender: im_core::ids::PeerRef::parse("did:example:agent", "").unwrap(),
        receiver: None,
        group: None,
        body: im_core::messages::MessageBodyView::Text {
            text: "hello".to_owned(),
            kind: im_core::messages::MessageKind::Text,
        },
        sent_at: Some("2026-08-11T00:00:00Z".to_owned()),
        received_at: Some("2026-08-11T00:00:05Z".to_owned()),
        metadata: im_core::messages::MessageMetadata::default(),
    };
    let mapped = awiki_im_core::dto::message::DartCommittedIncomingMessage::from(
        im_core::messages::CommittedIncomingMessage {
            event_id: "sync-event-1".to_owned(),
            logical_message_id: "msg-authoritative-time".to_owned(),
            source: "live_delta".to_owned(),
            direction: im_core::messages::MessageDirection::Incoming,
            authoritative_received_at: Some("2026-08-11T00:00:05Z".to_owned()),
            message,
        },
    );
    assert_eq!(
        mapped.authoritative_received_at.as_deref(),
        Some("2026-08-11T00:00:05Z")
    );
    assert_eq!(
        mapped.message.authoritative_received_at.as_deref(),
        Some("2026-08-11T00:00:05Z")
    );
}

#[test]
fn dart_delegated_message_options_map_to_im_core_optional_params() {
    let request = awiki_im_core::dto::message::DartSendTextRequest {
        target: awiki_im_core::dto::message::DartMessageTarget::Direct {
            peer: "did:example:bob".to_string(),
        },
        text: "hello".to_string(),
        markdown: false,
        security: awiki_im_core::dto::message::DartMessageSecurityMode::DefaultPlain,
        client_message_id: None,
        idempotency_key: None,
        wait_for_final_acceptance: false,
        delegated_signing: Some(awiki_im_core::dto::message::DartDelegatedSigningOptions {
            logical_sender_did: Some("did:example:alice".to_string()),
            signing_verification_method: Some("did:example:alice#daemon-key-1".to_string()),
            signing_key_ref: Some("local:daemon-key-1".to_string()),
            actor_agent_did: Some("did:example:daemon".to_string()),
        }),
    };

    let core: im_core::messages::SendMessageRequest =
        request.try_into().expect("text request maps");
    let delegated = core
        .delegated_signing
        .expect("delegated signing is preserved");
    assert_eq!(
        delegated.logical_sender_did.as_deref(),
        Some("did:example:alice")
    );
    assert_eq!(
        delegated.signing_verification_method.as_deref(),
        Some("did:example:alice#daemon-key-1")
    );
    assert_eq!(
        delegated.signing_key_ref.as_deref(),
        Some("local:daemon-key-1")
    );
    assert_eq!(
        delegated.actor_agent_did.as_deref(),
        Some("did:example:daemon")
    );
}

#[test]
fn dart_inbox_history_options_map_to_im_core_optional_params() {
    let options = awiki_im_core::dto::message::DartInboxHistoryOptions {
        inbox_owner_did: Some("did:example:alice".to_string()),
        inbox_auth_verification_method: Some("did:example:alice#daemon-key-1".to_string()),
        inbox_auth_key_ref: Some("local:daemon-key-1".to_string()),
        inbox_auth: Some(
            awiki_im_core::dto::message::DartInboxAuth::ScopedInboxToken {
                token: awiki_im_core::dto::message::DartScopedInboxToken {
                    token: "token-1".to_string(),
                },
            },
        ),
    };

    let core: im_core::messages::InboxHistoryOptions = options.into();
    assert_eq!(core.inbox_owner_did.as_deref(), Some("did:example:alice"));
    assert_eq!(
        core.inbox_auth_verification_method.as_deref(),
        Some("did:example:alice#daemon-key-1")
    );
    assert_eq!(
        core.inbox_auth_key_ref.as_deref(),
        Some("local:daemon-key-1")
    );
    assert!(matches!(
        core.inbox_auth,
        Some(im_core::messages::InboxAuth::ScopedInboxToken { token })
            if token.token == "token-1"
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
fn config_maps_mail_service_endpoint_into_im_core() {
    let config = awiki_im_core::dto::config::DartImCoreConfig {
        service_base_url: "https://awiki.ai".to_string(),
        did_domain: "awiki.ai".to_string(),
        client_version_info: None,
        user_service_endpoint: None,
        message_service_endpoint: None,
        mail_service_endpoint: Some("https://mail.awiki.ai".to_string()),
        anp_service_endpoint: None,
        anp_service_did: None,
        transport_policy: awiki_im_core::dto::config::DartMessageTransportPolicy::Auto,
    };

    let core: im_core::ImCoreConfig = config
        .try_into()
        .expect("mail endpoint maps into ImCoreConfig");
    assert_eq!(
        core.mail_service_endpoint.unwrap().as_str(),
        "https://mail.awiki.ai"
    );
}

#[test]
fn vault_open_options_map_to_im_core_without_debug_secret_leak() {
    let root_key = awiki_im_core::dto::config::DartDeviceVaultRootKey {
        bytes: vec![7_u8; im_core::vault::DEVICE_VAULT_ROOT_KEY_LEN],
    };
    let debug = format!("{root_key:?}");
    assert!(debug.contains("DartDeviceVaultRootKey"));
    assert!(debug.contains("[REDACTED]"));
    assert!(!debug.contains("7, 7"));

    let options = awiki_im_core::dto::config::DartImCoreOpenOptions {
        identity_secret_storage_policy:
            awiki_im_core::dto::config::DartIdentitySecretStoragePolicy::VaultRequired,
        identity_secret_vault: Some(awiki_im_core::dto::config::DartImCoreSecretVaultOptions {
            root_key,
            vault_dir: "/tmp/awiki-vault".to_string(),
            workspace_id: "workspace-a".to_string(),
            device_id: "device-a".to_string(),
        }),
        multi_device_device_revoke_enabled: true,
        multi_device_direct_e2ee_enabled: true,
        multi_device_group_e2ee_enabled: true,
        multi_device_handle_recovery_enabled: true,
        multi_device_audience: Some("awiki-user-service".to_owned()),
    };

    let mapped: im_core::ImCoreOpenOptions = options.try_into().expect("open options map");
    assert!(matches!(
        mapped.identity_secret_storage_policy,
        im_core::IdentitySecretStoragePolicy::VaultRequired
    ));
    assert!(mapped.multi_device_device_revoke_enabled);
    assert!(mapped.multi_device_direct_e2ee_enabled);
    assert!(mapped.multi_device_group_e2ee_enabled);
    assert!(mapped.multi_device_handle_recovery_enabled);
    let vault = mapped.identity_secret_vault.expect("vault options");
    assert_eq!(
        vault.vault_dir,
        std::path::PathBuf::from("/tmp/awiki-vault")
    );
    assert_eq!(vault.workspace_id, "workspace-a");
    assert_eq!(vault.device_id, "device-a");
    let vault_debug = format!("{vault:?}");
    assert!(vault_debug.contains("<redacted-root-key>"));
    assert!(!vault_debug.contains("7, 7"));
}

#[test]
fn vault_root_key_mapping_rejects_wrong_length_without_echoing_secret() {
    let options = awiki_im_core::dto::config::DartImCoreOpenOptions {
        identity_secret_storage_policy:
            awiki_im_core::dto::config::DartIdentitySecretStoragePolicy::VaultPreferred,
        identity_secret_vault: Some(awiki_im_core::dto::config::DartImCoreSecretVaultOptions {
            root_key: awiki_im_core::dto::config::DartDeviceVaultRootKey {
                bytes: b"short-secret".to_vec(),
            },
            vault_dir: "/tmp/awiki-vault".to_string(),
            workspace_id: "workspace-a".to_string(),
            device_id: "device-a".to_string(),
        }),
        multi_device_device_revoke_enabled: false,
        multi_device_direct_e2ee_enabled: false,
        multi_device_group_e2ee_enabled: false,
        multi_device_handle_recovery_enabled: false,
        multi_device_audience: None,
    };

    let error = im_core::ImCoreOpenOptions::try_from(options).unwrap_err();
    assert_eq!(error.code, "invalid_input");
    assert_eq!(error.field.as_deref(), Some("root_key"));
    assert!(error.message.contains("32 bytes"));
    assert!(!error.message.contains("short-secret"));
}

#[test]
fn device_revoke_result_maps_only_safe_product_state() {
    let result = im_core::identity::DeviceRevokeResult {
        did: im_core::ids::Did::parse("did:wba:example.test:alice").expect("did"),
        target_device_id: im_core::ids::ProtocolDeviceId::parse("device-member")
            .expect("device id"),
        status: im_core::identity::DeviceRevokeStatus::Revoked,
    };

    let dart: awiki_im_core::dto::identity::DartDeviceRevokeResult = result.into();
    assert_eq!(dart.did, "did:wba:example.test:alice");
    assert_eq!(dart.target_device_id, "device-member");
    assert!(matches!(
        dart.status,
        awiki_im_core::dto::identity::DartDeviceRevokeStatus::Revoked
    ));
    let debug = format!("{dart:?}");
    for forbidden in [
        "auth_generation",
        "document_hash",
        "registry_version",
        "root_proof",
        "admin_proof",
    ] {
        assert!(!debug.contains(forbidden));
    }
}

#[test]
fn device_registry_snapshot_maps_decimal_versions_on_the_registry_only_surface() {
    let snapshot = im_core::identity::DeviceJoinRegistrySnapshot {
        did: im_core::ids::Did::parse("did:wba:example.test:alice").expect("did"),
        registry_version: u64::MAX.to_string(),
        devices: vec![im_core::identity::DeviceRegistryAuthorizedDeviceSummary {
            protocol_device_id: im_core::ids::ProtocolDeviceId::parse("device-current")
                .expect("device id"),
            signing_key_id: "did:wba:example.test:alice#device-current-sign".to_owned(),
            e2ee_key_id: "did:wba:example.test:alice#device-current-e2ee".to_owned(),
            status: im_core::identity::DeviceJoinAuthorizationStatus::Active,
            role: im_core::identity::DeviceJoinRole::Admin,
            management_ready: true,
            is_current: true,
            auth_generation: u64::MAX.to_string(),
        }],
    };

    let mapped: awiki_im_core::dto::identity::DartDeviceJoinRegistrySnapshot = snapshot.into();
    assert_eq!(mapped.registry_version, u64::MAX.to_string());
    assert_eq!(mapped.devices.len(), 1);
    assert_eq!(mapped.devices[0].auth_generation, u64::MAX.to_string());
    assert!(mapped.devices[0].is_current);
}

#[test]
fn generated_registry_bridge_keeps_versions_as_strings() {
    let generated = include_str!("../src/frb_generated.rs");
    let snapshot_decoder = generated
        .split("impl SseDecode for crate::dto::identity::DartDeviceJoinRegistrySnapshot")
        .nth(1)
        .and_then(|source| source.split("impl SseDecode").next())
        .expect("generated Registry snapshot decoder");
    assert!(snapshot_decoder.contains("let mut var_registryVersion = <String>::sse_decode"));
    assert!(!snapshot_decoder.contains("var_registryVersion = <u64>::sse_decode"));

    let device_decoder = generated
        .split("impl SseDecode for crate::dto::identity::DartDeviceRegistryAuthorizedDeviceSummary")
        .nth(1)
        .and_then(|source| source.split("impl SseDecode").next())
        .expect("generated Registry device decoder");
    assert!(device_decoder.contains("let mut var_authGeneration = <String>::sse_decode"));
    assert!(!device_decoder.contains("var_authGeneration = <u64>::sse_decode"));
}

#[test]
fn identity_vault_status_maps_without_secret_refs() {
    let core_status = im_core::identity::IdentityVaultStatus {
        identity: im_core::identity::IdentitySummary {
            id: im_core::ids::IdentityId::parse("id-alice").expect("identity id"),
            did: im_core::ids::Did::parse("did:example:alice").expect("did"),
            handle: None,
            display_name: Some("Alice".to_string()),
            local_alias: Some("alice".to_string()),
            device_id: Some("device-a".to_string()),
            is_default: true,
            readiness: im_core::identity::IdentityReadiness {
                ready_for_auth: true,
                ready_for_messaging: true,
                missing: vec![],
            },
        },
        storage_policy: im_core::IdentitySecretStoragePolicy::VaultPreferred,
        selected_backend: im_core::identity::IdentitySecretStorageBackend::Vault,
        vault_available: true,
        vault_metadata_present: true,
        vault_metadata_verified: true,
        workspace_id: Some("workspace-a".to_string()),
        device_id: Some("device-a".to_string()),
        plaintext_compat_retained: Some(false),
        missing: vec![],
        warnings: vec![],
    };

    let dart: awiki_im_core::dto::identity::DartIdentityVaultStatus = core_status.into();
    assert_eq!(dart.identity.did, "did:example:alice");
    assert!(matches!(
        dart.storage_policy,
        awiki_im_core::dto::config::DartIdentitySecretStoragePolicy::VaultPreferred
    ));
    assert!(matches!(
        dart.selected_backend,
        awiki_im_core::dto::identity::DartIdentitySecretStorageBackend::Vault
    ));
    assert!(dart.vault_available);
    assert!(dart.vault_metadata_present);
    assert!(dart.vault_metadata_verified);
    assert_eq!(dart.workspace_id.as_deref(), Some("workspace-a"));
    assert_eq!(dart.device_id.as_deref(), Some("device-a"));
    assert_eq!(dart.plaintext_compat_retained, Some(false));
}

#[test]
fn identity_device_summary_maps_only_safe_product_state() {
    let core_summary = im_core::identity::IdentityDeviceSummary {
        identity: im_core::identity::IdentitySummary {
            id: im_core::ids::IdentityId::parse("id-alice").expect("identity id"),
            did: im_core::ids::Did::parse("did:example:alice").expect("did"),
            handle: None,
            display_name: Some("Alice".to_string()),
            local_alias: Some("alice".to_string()),
            device_id: Some("local-vault-device".to_string()),
            is_default: true,
            readiness: im_core::identity::IdentityReadiness {
                ready_for_auth: true,
                ready_for_messaging: true,
                missing: vec![],
            },
        },
        mode: im_core::identity::IdentityDeviceMode::VNext,
        protocol_device_id: Some(
            im_core::ids::ProtocolDeviceId::parse("protocol-device-a").expect("protocol device id"),
        ),
        role: Some(im_core::identity::IdentityDeviceRole::Admin),
        signing_key_id: Some("did:example:alice#device-signing".to_string()),
        e2ee_key_id: Some("did:example:alice#device-e2ee".to_string()),
        readiness: im_core::identity::IdentityDeviceReadiness::AdminReady,
        blocked_reason: None,
    };

    let dart: awiki_im_core::dto::identity::DartIdentityDeviceSummary = core_summary.into();
    assert_eq!(dart.identity.did, "did:example:alice");
    assert_eq!(
        dart.protocol_device_id.as_deref(),
        Some("protocol-device-a")
    );
    assert!(matches!(
        dart.mode,
        awiki_im_core::dto::identity::DartIdentityDeviceMode::VNext
    ));
    assert!(matches!(
        dart.role,
        Some(awiki_im_core::dto::identity::DartIdentityDeviceRole::Admin)
    ));
    assert!(matches!(
        dart.readiness,
        awiki_im_core::dto::identity::DartIdentityDeviceReadiness::AdminReady
    ));
    let debug = format!("{dart:?}");
    for forbidden in [
        "document_version",
        "document_hash",
        "registry_version",
        "auth_generation",
        "SecretRef",
    ] {
        assert!(!debug.contains(forbidden));
    }
}

#[test]
fn identity_vault_reports_map_without_secret_refs() {
    let core_status = im_core::identity::IdentityVaultStatus {
        identity: im_core::identity::IdentitySummary {
            id: im_core::ids::IdentityId::parse("id-alice").expect("identity id"),
            did: im_core::ids::Did::parse("did:example:alice").expect("did"),
            handle: None,
            display_name: Some("Alice".to_string()),
            local_alias: Some("alice".to_string()),
            device_id: Some("device-a".to_string()),
            is_default: true,
            readiness: im_core::identity::IdentityReadiness {
                ready_for_auth: true,
                ready_for_messaging: true,
                missing: vec![],
            },
        },
        storage_policy: im_core::IdentitySecretStoragePolicy::VaultRequired,
        selected_backend: im_core::identity::IdentitySecretStorageBackend::Vault,
        vault_available: true,
        vault_metadata_present: true,
        vault_metadata_verified: true,
        workspace_id: Some("workspace-a".to_string()),
        device_id: Some("device-a".to_string()),
        plaintext_compat_retained: Some(true),
        missing: vec![],
        warnings: vec!["identity plaintext compatibility files are still retained".to_string()],
    };
    let migration = im_core::identity::IdentityVaultMigrationReport {
        identity: core_status.identity.clone(),
        status: core_status.clone(),
        migrated: true,
        verified: true,
        plaintext_compat_retained: true,
        warnings: core_status.warnings.clone(),
    };
    let verification = im_core::identity::IdentityVaultVerificationReport {
        identity: core_status.identity.clone(),
        status: core_status,
        verified: true,
        warnings: vec!["identity plaintext compatibility files are still retained".to_string()],
    };

    let dart_migration: awiki_im_core::dto::identity::DartIdentityVaultMigrationReport =
        migration.into();
    let dart_verification: awiki_im_core::dto::identity::DartIdentityVaultVerificationReport =
        verification.into();

    assert!(dart_migration.migrated);
    assert!(dart_migration.verified);
    assert!(dart_migration.plaintext_compat_retained);
    assert_eq!(dart_migration.identity.did, "did:example:alice");
    assert!(dart_migration.warnings[0].contains("plaintext compatibility"));
    assert!(dart_verification.verified);
    assert_eq!(
        dart_verification.status.workspace_id.as_deref(),
        Some("workspace-a")
    );
    let debug = format!("{dart_migration:?} {dart_verification:?}");
    assert!(!debug.contains("SecretRef"));
    assert!(!debug.contains("private"));
}

#[test]
fn email_send_bridge_request_maps_to_typed_core_addresses() {
    let request = awiki_im_core::dto::email::DartSendEmailRequest {
        to: vec!["bob@awiki.ai".to_string()],
        cc: vec!["copy@awiki.ai".to_string()],
        subject: "Hello".to_string(),
        body_text: "Body".to_string(),
        body_html: Some("<p>Body</p>".to_string()),
    };

    let core: im_core::email::SendEmailRequest =
        request.try_into().expect("email request maps to im-core");
    assert_eq!(core.to[0].as_str(), "bob@awiki.ai");
    assert_eq!(core.cc[0].as_str(), "copy@awiki.ai");
    assert_eq!(core.body_html.as_deref(), Some("<p>Body</p>"));
}

#[test]
fn realtime_runner_capability_is_exposed_after_bridge_plan_lands() {
    let capability = awiki_im_core::dto::realtime::DartRealtimeCapability {
        status_supported: true,
        connect_supported: true,
        runner_exposed: true,
        reason: None,
    };
    assert!(capability.connect_supported);
    assert!(capability.runner_exposed);
    assert!(capability.reason.is_none());
}

#[test]
fn device_join_bridge_projection_excludes_internal_state_and_redacts_prompt() {
    let session = im_core::identity::DeviceJoinSessionView {
        join_session_id: "join-safe-id".to_owned(),
        did: im_core::ids::Did::parse("did:wba:example.test:alice").unwrap(),
        protocol_device_id: im_core::ids::ProtocolDeviceId::parse("device-new").unwrap(),
        side: im_core::identity::DeviceJoinSide::NewDevice,
        phase: im_core::identity::DeviceJoinLocalPhase::Pending,
        expires_at: "2026-07-19T12:00:00Z".to_owned(),
    };
    let dart: awiki_im_core::dto::identity::DartDeviceJoinSessionSummary = session.into();
    let debug = format!("{dart:?}");
    assert!(debug.contains("join-safe-id"));
    for forbidden in [
        "join_request_hash",
        "challenge_id",
        "document_hash",
        "registry_version",
        "auth_generation",
    ] {
        assert!(!debug.contains(forbidden));
    }

    let prompt = awiki_im_core::dto::identity::DartDeviceJoinApprovalPrompt {
        approval_handle: "approval-secret-handle".to_owned(),
        join_session_id: "join-safe-id".to_owned(),
        sas: "123456".to_owned(),
        expires_at: "2026-07-19T12:00:00Z".to_owned(),
    };
    let debug = format!("{prompt:?}");
    assert!(!debug.contains("approval-secret-handle"));
    assert!(!debug.contains("123456"));
}

#[test]
fn device_join_progress_debug_redacts_short_lived_sas() {
    let progress = awiki_im_core::dto::identity::DartDeviceJoinProgress {
        session: awiki_im_core::dto::identity::DartDeviceJoinSessionSummary {
            join_session_id: "join-safe-id".to_owned(),
            did: "did:wba:example.test:alice".to_owned(),
            protocol_device_id: "device-new".to_owned(),
            side: awiki_im_core::dto::identity::DartDeviceJoinSide::Admin,
            phase: awiki_im_core::dto::identity::DartDeviceJoinPhase::ResponseVerified,
            expires_at: "2026-07-23T12:10:00Z".to_owned(),
        },
        remote_state: awiki_im_core::dto::identity::DartDeviceJoinRemoteState::ResponseVerified,
        sas: Some("123456".to_owned()),
        authorized_device: None,
    };

    let debug = format!("{progress:?}");
    assert!(debug.contains("join-safe-id"));
    assert!(debug.contains("<redacted-sas>"));
    assert!(!debug.contains("123456"));
}

#[test]
fn device_join_request_notice_is_closed_and_secret_free() {
    let notice = im_core::identity::DeviceJoinRequestNotice {
        event_id: "join-event-1".to_owned(),
        join_session_id: "join-safe-id".to_owned(),
        did: im_core::ids::Did::parse("did:wba:example.test:alice").unwrap(),
        protocol_device_id: im_core::ids::ProtocolDeviceId::parse("device-new").unwrap(),
        candidate_key_fingerprint: "fingerprint-safe".to_owned(),
        issued_at: "2026-07-23T12:00:00Z".to_owned(),
        expires_at: "2026-07-23T12:10:00Z".to_owned(),
        state: im_core::identity::DeviceJoinRemoteState::Pending,
        claimed_by_current_device: false,
        can_start_verification: true,
    };
    let dart: awiki_im_core::dto::identity::DartDeviceJoinRequestNotice = notice.into();

    assert_eq!(dart.event_id, "join-event-1");
    assert_eq!(
        dart.state,
        awiki_im_core::dto::identity::DartDeviceJoinRemoteState::Pending
    );
    assert!(dart.can_start_verification);
    let debug = format!("{dart:?}");
    for forbidden in [
        "join_request_proof",
        "admin_proof",
        "challenge_ciphertext",
        "pairing_private_key",
        "shared_secret",
        "token",
        "sas",
    ] {
        assert!(!debug.contains(forbidden));
    }
}

#[test]
fn group_create_bridge_request_no_longer_accepts_per_request_service_did() {
    let request = awiki_im_core::dto::group::DartCreateGroupRequest {
        name: "test".to_string(),
        identity_mode: awiki_im_core::dto::group::DartGroupIdentityMode::DidOnly,
        identity_handle: None,
        description: None,
        avatar_uri: None,
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

#[test]
fn group_create_bridge_preserves_explicit_handle_mode_without_fallback() {
    use awiki_im_core::dto::group::{DartCreateGroupRequest, DartGroupIdentityMode};

    let request = DartCreateGroupRequest {
        name: "handle group".to_owned(),
        identity_mode: DartGroupIdentityMode::Handle,
        identity_handle: Some("alice.example.com".to_owned()),
        description: None,
        avatar_uri: None,
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
        .clone()
        .into_core()
        .expect("Handle mode maps to creator_handle");
    assert_eq!(
        core.creator_handle
            .as_ref()
            .map(im_core::ids::Handle::as_str),
        Some("alice.example.com")
    );

    let mut invalid = request.clone();
    invalid.identity_mode = DartGroupIdentityMode::DidOnly;
    assert!(invalid.into_core().is_err());
    let mut missing = request;
    missing.identity_handle = None;
    assert!(missing.into_core().is_err());
}

#[test]
fn macos_sdk_build_includes_group_e2ee_support() {
    let script = include_str!("../../../scripts/flutter/build-apple.sh");
    assert!(
        script.contains("--features blocking,sqlite,http,macos,group-e2ee"),
        "the macOS XCFramework must contain the feature-gated group E2EE implementation"
    );
}
