pub struct MessageService<'a> {
    client: &'a crate::core::ImClient,
}

pub const LOCAL_INCOMING_RECOVERY_LIMIT_MAX: u32 = 1_000;

#[cfg(test)]
mod direct_send_result_identity_tests {
    use crate::internal::local_state::owner_scope::{
        direct_conversation_id_for_peer_scope, DirectPeerScope,
    };

    #[test]
    fn direct_send_result_normalization_uses_peer_scope_thread() {
        let scope = DirectPeerScope::new("user-bob", "Bob.AWiki.Test").expect("valid peer scope");
        let expected_thread_id = direct_conversation_id_for_peer_scope(&scope);
        let mut result = send_result("msg-peer-scope");

        super::normalize_direct_send_result_for_peer_scope(
            &mut result,
            Some(&scope),
            Some("bob.awiki.test"),
            Some("did:example:bob-current"),
        )
        .expect("normalize");

        match &result.message.thread {
            crate::messages::ThreadRef::Thread(thread) => {
                assert_eq!(thread.as_str(), expected_thread_id);
            }
            other => panic!("expected thread ref, got {other:?}"),
        }
        let identity = result
            .message
            .metadata
            .conversation_identity
            .as_ref()
            .expect("normalized send result should expose canonical identity");
        assert_eq!(identity.conversation_id, expected_thread_id);
        assert_eq!(identity.canonical_thread_id, expected_thread_id);
        assert_eq!(identity.storage_thread_ref.kind, "thread");
        assert_eq!(identity.storage_thread_ref.id, expected_thread_id);
        assert_eq!(
            attribute(&result, "peer_user_id").as_deref(),
            Some("user-bob")
        );
        assert_eq!(
            attribute(&result, "peer_full_handle").as_deref(),
            Some("bob.awiki.test")
        );
        assert_eq!(
            attribute(&result, "target_handle").as_deref(),
            Some("bob.awiki.test")
        );
        assert_eq!(
            attribute(&result, "resolved_target_did").as_deref(),
            Some("did:example:bob-current")
        );
        assert_eq!(
            attribute(&result, "peer_current_did").as_deref(),
            Some("did:example:bob-current")
        );
        assert_eq!(
            result.message.receiver.as_ref().map(|peer| peer.as_str()),
            Some("bob.awiki.test")
        );
    }

    #[test]
    fn direct_target_preserves_cross_domain_handle_subject() {
        let resolved = super::resolved_direct_peer_from_handle(
            crate::internal::handle_discovery::DirectHandleResolution {
                target_did: "did:wba:remote.example:user:peer:e1".to_owned(),
                full_handle: "peer.remote.example".to_owned(),
                authority_subject_id: "peer.remote.example".to_owned(),
            },
        )
        .unwrap();

        assert_eq!(resolved.peer_scope.unwrap().user_id, "peer.remote.example");
    }

    #[test]
    fn direct_send_result_normalization_without_peer_scope_is_noop() {
        let mut result = send_result("msg-direct-did");

        super::normalize_direct_send_result_for_peer_scope(
            &mut result,
            None,
            Some("bob.awiki.test"),
            Some("did:example:bob"),
        )
        .expect("normalize");

        match &result.message.thread {
            crate::messages::ThreadRef::Direct(peer) => {
                assert_eq!(peer.as_str(), "did:example:bob");
            }
            other => panic!("expected direct ref, got {other:?}"),
        }
        assert!(attribute(&result, "peer_user_id").is_none());
    }

    fn send_result(message_id: &str) -> crate::messages::SendMessageResult {
        crate::messages::SendMessageResult {
            message: crate::messages::Message {
                id: crate::ids::MessageId::parse(message_id).unwrap(),
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                ),
                direction: crate::messages::MessageDirection::Outgoing,
                sender: crate::ids::PeerRef::parse("did:example:alice", "").unwrap(),
                receiver: Some(crate::ids::PeerRef::parse("bob.awiki.test", "").unwrap()),
                group: None,
                body: crate::messages::MessageBodyView::Text {
                    text: "hello".to_owned(),
                    kind: crate::messages::MessageKind::Text,
                },
                sent_at: Some("2026-07-04T00:00:00Z".to_owned()),
                received_at: None,
                metadata: crate::messages::MessageMetadata {
                    conversation_identity: Some(
                        crate::messages::ConversationIdentity::from_thread_ref(
                            &crate::messages::ThreadRef::Direct(
                                crate::ids::PeerRef::parse("bob.awiki.test", "").unwrap(),
                            ),
                        ),
                    ),
                    ..crate::messages::MessageMetadata::default()
                },
            },
            delivery: crate::messages::DeliveryState::Accepted,
            warnings: Vec::new(),
        }
    }

    fn attribute(result: &crate::messages::SendMessageResult, key: &str) -> Option<String> {
        result
            .message
            .metadata
            .attributes
            .iter()
            .find(|attribute| attribute.key == key)
            .map(|attribute| attribute.value.clone())
    }
}

#[cfg(all(test, feature = "sqlite"))]
mod incoming_recovery_tests {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use serde_json::json;
    use std::fs;

    #[tokio::test]
    async fn exact_client_reads_only_its_hydrated_incoming_projection_oldest_first() {
        let fixture = Fixture::new();
        let bootstrap = fixture
            .core
            .generate_vnext_agent_bootstrap(
                crate::identity::AgentIdentityKind::Daemon,
                "daemon-recovery",
            )
            .unwrap();
        let owner_identity_id = bootstrap.identity_id.clone();
        let owner_did = bootstrap.did.as_str().to_owned();
        let client = fixture
            .core
            .client_with_device_identity_material(host_material(
                &bootstrap,
                "daemon-recovery.awiki.info",
                "daemon-account",
            ))
            .unwrap();
        for record in [
            record(
                "msg-later",
                &owner_identity_id,
                &owner_did,
                0,
                "2026-08-01T00:02:00Z",
                crate::internal::local_state::messages::MessageHydrationState::Hydrated,
            ),
            record(
                "msg-oldest",
                &owner_identity_id,
                &owner_did,
                0,
                "2026-08-01T00:01:00Z",
                crate::internal::local_state::messages::MessageHydrationState::Hydrated,
            ),
            record(
                "msg-outgoing",
                &owner_identity_id,
                &owner_did,
                1,
                "2026-08-01T00:00:00Z",
                crate::internal::local_state::messages::MessageHydrationState::Hydrated,
            ),
            record(
                "msg-discovered",
                &owner_identity_id,
                &owner_did,
                0,
                "2026-08-01T00:00:00Z",
                crate::internal::local_state::messages::MessageHydrationState::Discovered,
            ),
            record(
                "msg-other-owner",
                "other-owner",
                &owner_did,
                0,
                "2026-08-01T00:00:00Z",
                crate::internal::local_state::messages::MessageHydrationState::Hydrated,
            ),
            record(
                "msg-other-did",
                &owner_identity_id,
                "did:wba:awiki.info:agent:daemon:other:e1_other",
                0,
                "2026-08-01T00:00:00Z",
                crate::internal::local_state::messages::MessageHydrationState::Hydrated,
            ),
        ] {
            fixture.seed(record);
        }

        let first = client
            .messages()
            .local_hydrated_incoming_recovery_async(crate::messages::IncomingMessageRecoveryQuery {
                limit: 1,
                page_token: None,
            })
            .await
            .unwrap();
        assert_eq!(first.items[0].logical_message_id, "msg-oldest");
        assert!(first.has_more);
        let first_token = first.next_page_token.clone().unwrap();
        let second = client
            .messages()
            .local_hydrated_incoming_recovery_async(crate::messages::IncomingMessageRecoveryQuery {
                limit: 1,
                page_token: first.next_page_token,
            })
            .await
            .unwrap();
        assert_eq!(second.items[0].logical_message_id, "msg-later");
        assert!(!second.has_more);
        assert!(second.next_page_token.is_none());

        let other_bootstrap = fixture
            .core
            .generate_vnext_agent_bootstrap(
                crate::identity::AgentIdentityKind::Daemon,
                "daemon-other",
            )
            .unwrap();
        let other_client = fixture
            .core
            .client_with_device_identity_material(host_material(
                &other_bootstrap,
                "daemon-other.awiki.info",
                "other-account",
            ))
            .unwrap();
        assert!(matches!(
            other_client
                .messages()
                .local_hydrated_incoming_recovery_async(
                    crate::messages::IncomingMessageRecoveryQuery {
                        limit: 1,
                        page_token: Some(first_token),
                    },
                )
                .await,
            Err(crate::ImError::IdentityBindingConflict { .. })
        ));
        assert_eq!(
            client
                .messages()
                .local_hydrated_incoming_recovery_async(
                    crate::messages::IncomingMessageRecoveryQuery {
                        limit: 1,
                        page_token: None,
                    },
                )
                .await
                .unwrap()
                .items[0]
                .logical_message_id,
            "msg-oldest"
        );

        for _ in 0..2 {
            let inbox = client
                .messages()
                .local_inbox_projection_with_metadata_async(crate::messages::InboxQuery {
                    scope: crate::messages::InboxScope::DirectOnly,
                    limit: crate::ids::PageLimit(2),
                    cursor: None,
                    unread_only: false,
                    inbox_history_options: None,
                })
                .await
                .unwrap();
            assert_eq!(
                inbox
                    .items
                    .iter()
                    .map(|message| message.id.as_str())
                    .collect::<Vec<_>>(),
                vec!["msg-later", "msg-oldest"]
            );
            assert_eq!(inbox.source.as_deref(), Some("local_projection"));
        }
    }

    #[tokio::test]
    async fn recovery_limit_is_closed_and_hosted_client_has_no_binding() {
        let fixture = Fixture::new();
        let bootstrap = fixture
            .core
            .generate_vnext_agent_bootstrap(
                crate::identity::AgentIdentityKind::Daemon,
                "daemon-hosted",
            )
            .unwrap();
        let hosted = fixture
            .core
            .client_with_identity_material(crate::identity::HostedIdentityMaterial {
                identity_id: bootstrap.identity_id.clone(),
                did: bootstrap.did.as_str().to_owned(),
                handle: Some("daemon-hosted.awiki.info".to_owned()),
                display_name: Some("Hosted daemon".to_owned()),
                did_document: bootstrap.did_document.clone(),
                default_signing_private_key_pem: bootstrap.root_private_key_pem.clone(),
                e2ee_agreement_private_key_pem: Some(bootstrap.device_e2ee_private_key_pem.clone()),
                auth_token: None,
            })
            .unwrap();

        assert!(matches!(
            hosted
                .messages()
                .local_hydrated_incoming_recovery_async(
                    crate::messages::IncomingMessageRecoveryQuery {
                        limit: 1,
                        page_token: None,
                    },
                )
                .await,
            Err(crate::ImError::UnsupportedCapability { capability })
                if capability == "active-sync-account-binding"
        ));
        let legacy = fixture.legacy_client();
        assert!(matches!(
            legacy
                .messages()
                .local_hydrated_incoming_recovery_async(
                    crate::messages::IncomingMessageRecoveryQuery {
                        limit: 1,
                        page_token: None,
                    },
                )
                .await,
            Err(crate::ImError::UnsupportedCapability { capability })
                if capability == "active-sync-account-binding"
        ));
        assert!(matches!(
            legacy
                .messages()
                .local_inbox_projection_with_metadata_async(crate::messages::InboxQuery {
                    scope: crate::messages::InboxScope::All,
                    limit: crate::ids::PageLimit(1),
                    cursor: None,
                    unread_only: false,
                    inbox_history_options: None,
                })
                .await,
            Err(crate::ImError::UnsupportedCapability { capability })
                if capability == "active-sync-account-binding"
        ));
        for invalid in [0, super::LOCAL_INCOMING_RECOVERY_LIMIT_MAX + 1] {
            assert!(matches!(
                hosted
                    .messages()
                    .local_hydrated_incoming_recovery_async(
                        crate::messages::IncomingMessageRecoveryQuery {
                            limit: invalid,
                            page_token: None,
                        },
                    )
                    .await,
                Err(crate::ImError::InvalidInput { field: Some(field), .. }) if field == "limit"
            ));
        }
    }

    struct Fixture {
        _root: tempfile::TempDir,
        core: crate::core::ImCore,
        sqlite_path: std::path::PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let workspace = root.path().join("workspace");
            for directory in ["identities", "local", "cache", "tmp"] {
                fs::create_dir_all(workspace.join(directory)).unwrap();
            }
            let sqlite_path = workspace.join("local/im.sqlite");
            let core = crate::core::ImCore::new(
                crate::ImCoreConfig {
                    service_base_url: crate::ServiceEndpoint::parse("https://awiki.info").unwrap(),
                    did_domain: "awiki.info".to_owned(),
                    user_service_endpoint: None,
                    message_service_endpoint: None,
                    mail_service_endpoint: None,
                    anp_service_endpoint: None,
                    anp_service_did: None,
                    ca_bundle: None,
                    transport_policy: crate::MessageTransportPolicy::HttpOnly,
                },
                crate::ImCorePaths {
                    identities: crate::IdentityRegistryPaths {
                        identity_root_dir: workspace.join("identities"),
                        registry_path: workspace.join("identities/registry.json"),
                        default_identity_path: Some(workspace.join("identities/default")),
                    },
                    local_state: crate::LocalStatePaths {
                        sqlite_path: sqlite_path.clone(),
                    },
                    runtime: crate::RuntimePaths {
                        cache_dir: workspace.join("cache"),
                        temp_dir: workspace.join("tmp"),
                    },
                },
            )
            .unwrap();
            Self {
                _root: root,
                core,
                sqlite_path,
            }
        }

        fn seed(&self, record: crate::internal::local_state::messages::MessageRecord) {
            let connection =
                crate::internal::local_state::open_writable(&self.sqlite_path).unwrap();
            crate::internal::local_state::messages::upsert_message(&connection, &record).unwrap();
        }

        fn legacy_client(&self) -> crate::core::ImClient {
            let identity_root = &self.core.inner().sdk_paths().identities.identity_root_dir;
            fs::create_dir_all(identity_root.join("legacy")).unwrap();
            fs::write(
                identity_root.join("registry.json"),
                json!({
                    "default_identity": "legacy",
                    "identities": [{
                        "id": "legacy-id",
                        "did": "did:wba:awiki.info:agent:daemon:legacy:e1_legacy",
                        "local_alias": "legacy",
                        "ready_for_auth": true,
                        "ready_for_messaging": true,
                        "missing": []
                    }]
                })
                .to_string(),
            )
            .unwrap();
            self.core
                .client(crate::identity::IdentitySelector::LocalAlias(
                    "legacy".to_owned(),
                ))
                .unwrap()
        }
    }

    fn host_material(
        bootstrap: &crate::identity::VNextAgentBootstrapMaterial,
        handle: &str,
        account_id: &str,
    ) -> crate::identity::HostBackedDeviceIdentityMaterial {
        crate::identity::HostBackedDeviceIdentityMaterial {
            identity_id: bootstrap.identity_id.clone(),
            did: bootstrap.did.as_str().to_owned(),
            handle: Some(handle.to_owned()),
            display_name: Some("Recovery daemon".to_owned()),
            account_id: account_id.to_owned(),
            binding_generation: "1".to_owned(),
            did_document: bootstrap.did_document.clone(),
            protocol_device_id: bootstrap.protocol_device_id.clone(),
            device_signing_key_id: bootstrap.device_signing_key_id.clone(),
            device_signing_private_key_pem: bootstrap.device_signing_private_key_pem.clone(),
            device_e2ee_key_id: bootstrap.device_e2ee_key_id.clone(),
            device_e2ee_private_key_pem: bootstrap.device_e2ee_private_key_pem.clone(),
            root_key_id: bootstrap.root_key_id.clone(),
            root_private_key_pem: bootstrap.root_private_key_pem.clone(),
            authorization_status: crate::identity::IdentityDeviceAuthorizationStatus::Active,
            role: crate::identity::IdentityDeviceRole::Admin,
            management_ready: true,
            auth_generation: "1".to_owned(),
            access_token: access_token(bootstrap, account_id),
        }
    }

    fn access_token(
        bootstrap: &crate::identity::VNextAgentBootstrapMaterial,
        account_id: &str,
    ) -> String {
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let claims = json!({
            "iss": "user-service",
            "aud": ["awiki-user-service", "awiki-message-service"],
            "sub": bootstrap.did.as_str(),
            "type": "access",
            "purpose": "awiki.device.access.v1",
            "did": bootstrap.did.as_str(),
            "user_id": account_id,
            "device_id": bootstrap.protocol_device_id.as_str(),
            "key_id": bootstrap.device_signing_key_id,
            "auth_generation": 1,
            "scopes": ["device:manage", "device:read", "message:connect"],
            "iat": now,
            "nbf": now,
            "exp": now + 300,
            "jti": "daemon-recovery-token"
        });
        format!(
            "e30.{}.signature",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap())
        )
    }

    fn record(
        message_id: &str,
        owner_identity_id: &str,
        owner_did: &str,
        direction: i64,
        timestamp: &str,
        hydration_state: crate::internal::local_state::messages::MessageHydrationState,
    ) -> crate::internal::local_state::messages::MessageRecord {
        crate::internal::local_state::messages::MessageRecord {
            msg_id: message_id.to_owned(),
            owner_identity_id: owner_identity_id.to_owned(),
            owner_did: owner_did.to_owned(),
            conversation_id: "dm:did:wba:awiki.info:user:peer:e1_peer".to_owned(),
            wire_thread_kind: "direct".to_owned(),
            wire_thread_ref: "did:wba:awiki.info:user:peer:e1_peer".to_owned(),
            wire_identity_resolution_state: "resolved".to_owned(),
            thread_id: "dm:did:wba:awiki.info:user:peer:e1_peer".to_owned(),
            direction,
            sender_did: if direction == 0 {
                "did:wba:awiki.info:user:peer:e1_peer".to_owned()
            } else {
                owner_did.to_owned()
            },
            receiver_did: if direction == 0 {
                owner_did.to_owned()
            } else {
                "did:wba:awiki.info:user:peer:e1_peer".to_owned()
            },
            content_type: "text/plain".to_owned(),
            content: message_id.to_owned(),
            hydration_state,
            sent_at: timestamp.to_owned(),
            stored_at: timestamp.to_owned(),
            ..crate::internal::local_state::messages::MessageRecord::default()
        }
    }
}

#[cfg(test)]
mod conversation_send_tests;

#[cfg(test)]
mod stale_delivery_target_tests {
    use serde_json::json;

    #[test]
    fn stale_delivery_target_from_error_extracts_binding_but_ignores_private_subject() {
        let err = crate::ImError::Service {
            status_code: None,
            code: Some("1406".to_owned()),
            message: "stale did".to_owned(),
            data: Some(json!({
                "reason": "stale_did",
                "target_did": "did:example:bob-old",
                "current_did": "did:example:bob-current",
                "full_handle": "bob.awiki.test",
                "user_id": "user-bob"
            })),
        };

        let stale = super::stale_delivery_target_from_error(&err, "did:example:alice")
            .expect("stale target");

        assert_eq!(
            stale.current_did.as_deref(),
            Some("did:example:bob-current")
        );
        assert_eq!(stale.full_handle.as_deref(), Some("bob.awiki.test"));
    }

    #[test]
    fn stale_delivery_target_from_error_ignores_plain_invalid_binding() {
        let err = crate::ImError::Service {
            status_code: None,
            code: Some("1406".to_owned()),
            message: "invalid target".to_owned(),
            data: Some(json!({
                "reason": "proof_mismatch",
                "current_did": "did:example:bob-current"
            })),
        };

        assert!(super::stale_delivery_target_from_error(&err, "did:example:alice").is_none());
    }
}

#[cfg(all(test, feature = "sqlite"))]
mod conversation_mark_read_request_tests {
    use serde_json::json;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn mark_conversation_read_maps_direct_storage_and_remote_thread_separately() {
        let fixture = Fixture::new("mark-read-direct");
        let client = fixture.client();
        let request = crate::messages::MarkConversationReadRequest {
            conversation: crate::messages::ConversationReadRef::new("dm:did:example:bob").unwrap(),
            watermark: None,
            fallback_max_message_ids: Some(25),
        };

        let mapped = super::mark_read_input_for_conversation(&client, request).unwrap();

        match mapped.request.thread {
            crate::messages::ThreadRef::Thread(thread) => {
                assert_eq!(thread.as_str(), "dm:did:example:bob");
            }
            other => panic!("expected storage thread ref, got {other:?}"),
        }
        match mapped.remote_thread.expect("remote thread") {
            crate::messages::ThreadRef::Direct(peer) => {
                assert_eq!(peer.as_str(), "did:example:bob");
            }
            other => panic!("expected remote direct ref, got {other:?}"),
        }
        assert!(mapped.request.watermark.is_none());
        assert_eq!(mapped.request.fallback_max_message_ids, Some(25));
    }

    #[test]
    fn mark_conversation_read_maps_peer_scope_to_remote_direct_from_projection() {
        let fixture = Fixture::new("mark-read-peer-scope");
        let client = fixture.client();
        let peer_scope = crate::internal::local_state::owner_scope::DirectPeerScope::new(
            "user-bob",
            "bob.awiki.test",
        )
        .unwrap();
        let conversation_id =
            crate::internal::local_state::owner_scope::direct_conversation_id_for_peer_scope(
                &peer_scope,
            );
        fixture.seed_message(crate::internal::local_state::messages::MessageRecord {
            msg_id: "msg-peer-scope-read".to_owned(),
            conversation_id: conversation_id.clone(),
            thread_id: conversation_id.clone(),
            sender_did: "did:example:bob-current".to_owned(),
            receiver_did: "did:example:alice".to_owned(),
            metadata: json!({
                "peer_user_id": "user-bob",
                "peer_full_handle": "bob.awiki.test",
                "peer_current_did": "did:example:bob-current"
            })
            .to_string(),
            ..Fixture::message_record_defaults()
        });
        let request = crate::messages::MarkConversationReadRequest {
            conversation: crate::messages::ConversationReadRef::new(conversation_id.clone())
                .unwrap(),
            watermark: Some(crate::messages::ReadWatermark {
                last_read_message_id: None,
                last_read_thread_seq: Some("42".to_owned()),
                read_at: None,
            }),
            fallback_max_message_ids: None,
        };

        let mapped = super::mark_read_input_for_conversation(&client, request).unwrap();

        match mapped.request.thread {
            crate::messages::ThreadRef::Thread(thread) => {
                assert_eq!(thread.as_str(), conversation_id);
            }
            other => panic!("expected storage thread ref, got {other:?}"),
        }
        match mapped.remote_thread.expect("remote thread") {
            crate::messages::ThreadRef::Direct(peer) => {
                assert_eq!(peer.as_str(), "did:example:bob-current");
            }
            other => panic!("expected remote direct ref, got {other:?}"),
        }
        assert_eq!(
            mapped
                .request
                .watermark
                .as_ref()
                .and_then(|watermark| watermark.last_read_thread_seq.as_deref()),
            Some("42")
        );
    }

    #[test]
    fn mark_conversation_read_maps_group_storage_and_remote_thread_separately() {
        let fixture = Fixture::new("mark-read-group");
        let client = fixture.client();
        let request = crate::messages::MarkConversationReadRequest {
            conversation: crate::messages::ConversationReadRef::new("group:did:example:group")
                .unwrap(),
            watermark: None,
            fallback_max_message_ids: None,
        };

        let mapped = super::mark_read_input_for_conversation(&client, request).unwrap();

        match mapped.request.thread {
            crate::messages::ThreadRef::Thread(thread) => {
                assert_eq!(thread.as_str(), "group:did:example:group");
            }
            other => panic!("expected group storage thread ref, got {other:?}"),
        }
        match mapped.remote_thread.expect("remote thread") {
            crate::messages::ThreadRef::Group(group) => {
                assert_eq!(group.as_str(), "did:example:group");
            }
            other => panic!("expected remote group ref, got {other:?}"),
        }
    }

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new(prefix: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "im-core-mark-read-{prefix}-{}-{nanos}",
                std::process::id()
            ));
            let identity_root = root.join("identities");
            let identity_dir = identity_root.join("alice");
            fs::create_dir_all(&identity_dir).unwrap();
            fs::write(identity_root.join("default"), "alice\n").unwrap();
            fs::write(
                identity_root.join("registry.json"),
                r#"{
                  "default_identity": "alice",
                  "identities": [{
                    "id": "alice-id",
                    "did": "did:example:alice",
                    "local_alias": "alice",
                    "ready_for_auth": true,
                    "ready_for_messaging": true,
                    "device_id": "device-alice",
                    "missing": []
                  }]
                }"#,
            )
            .unwrap();
            Self { root }
        }

        fn client(&self) -> crate::core::ImClient {
            crate::core::ImCore::new(
                crate::ImCoreConfig {
                    service_base_url: crate::ServiceEndpoint::parse("https://example.test")
                        .unwrap(),
                    did_domain: "awiki.test".to_owned(),
                    user_service_endpoint: None,
                    message_service_endpoint: None,
                    mail_service_endpoint: None,
                    anp_service_endpoint: None,
                    anp_service_did: None,
                    ca_bundle: None,
                    transport_policy: crate::MessageTransportPolicy::HttpOnly,
                },
                crate::ImCorePaths {
                    identities: crate::paths::IdentityRegistryPaths {
                        identity_root_dir: self.root.join("identities"),
                        registry_path: self.root.join("identities").join("registry.json"),
                        default_identity_path: Some(self.root.join("identities").join("default")),
                    },
                    local_state: crate::paths::LocalStatePaths {
                        sqlite_path: self.root.join("local").join("im.sqlite"),
                    },
                    runtime: crate::paths::RuntimePaths {
                        cache_dir: self.root.join("cache"),
                        temp_dir: self.root.join("tmp"),
                    },
                },
            )
            .unwrap()
            .client(crate::identity::IdentitySelector::LocalAlias(
                "alice".to_owned(),
            ))
            .unwrap()
        }

        fn seed_message(&self, record: crate::internal::local_state::messages::MessageRecord) {
            let connection = crate::internal::local_state::open_writable(
                &self.root.join("local").join("im.sqlite"),
            )
            .unwrap();
            crate::internal::local_state::messages::upsert_message(&connection, &record).unwrap();
        }

        fn message_record_defaults() -> crate::internal::local_state::messages::MessageRecord {
            crate::internal::local_state::messages::MessageRecord {
                owner_identity_id: "alice-id".to_owned(),
                owner_did: "did:example:alice".to_owned(),
                direction: 0,
                content_type: "text/plain".to_owned(),
                content: "hello from projection".to_owned(),
                sent_at: "2026-07-05T00:00:00Z".to_owned(),
                stored_at: "2026-07-05T00:00:00Z".to_owned(),
                ..crate::internal::local_state::messages::MessageRecord::default()
            }
        }
    }
}

#[cfg(all(test, feature = "sqlite"))]
mod conversation_read_model_tests {
    use serde_json::json;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn local_conversation_timeline_reads_projection_by_conversation_id() {
        let fixture = Fixture::new("conversation-timeline");
        let client = fixture.client();
        let connection =
            crate::internal::local_state::open_writable(&fixture.sqlite_path()).unwrap();
        crate::internal::local_state::messages::upsert_message(
            &connection,
            &crate::internal::local_state::messages::MessageRecord {
                msg_id: "msg-conversation-local".to_owned(),
                owner_identity_id: "alice-id".to_owned(),
                owner_did: "did:example:alice".to_owned(),
                conversation_id: "dm:did:example:bob".to_owned(),
                thread_id: "dm:did:example:bob".to_owned(),
                direction: 0,
                sender_did: "did:example:bob".to_owned(),
                receiver_did: "did:example:alice".to_owned(),
                content_type: "text/plain".to_owned(),
                content: "hello from projection".to_owned(),
                sent_at: "2026-07-05T00:00:00Z".to_owned(),
                stored_at: "2026-07-05T00:00:00Z".to_owned(),
                server_seq: Some(7),
                ..crate::internal::local_state::messages::MessageRecord::default()
            },
        )
        .unwrap();

        let page = client
            .messages()
            .local_conversation_timeline_with_metadata(
                crate::messages::ConversationReadRef::new("dm:did:example:bob").unwrap(),
                crate::messages::LocalHistoryQuery {
                    limit: crate::ids::PageLimit(20),
                    cursor: None,
                },
            )
            .unwrap();

        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].id.as_str(), "msg-conversation-local");
        assert_eq!(
            page.items[0].metadata.server_sequence,
            Some(7),
            "conversation timeline should read committed projection metadata"
        );
        assert!(!page.has_more);
    }

    #[test]
    fn sync_conversation_after_resolves_direct_conversation_to_syncable_thread() {
        let fixture = Fixture::new("conversation-sync-direct");
        let client = fixture.client();

        let resolved = super::resolve_service_conversation_thread(
            &client,
            &crate::messages::ConversationReadRef::new("dm:did:example:bob").unwrap(),
        )
        .unwrap();

        match resolved.thread {
            crate::messages::ThreadRef::Direct(peer) => {
                assert_eq!(peer.as_str(), "did:example:bob");
            }
            other => panic!("expected direct thread, got {other:?}"),
        }
        assert_eq!(resolved.resolved_did.as_deref(), Some("did:example:bob"));
        assert!(resolved.peer_scope.is_none());
    }

    #[test]
    fn sync_conversation_after_rejects_non_canonical_direct_alias() {
        let fixture = Fixture::new("conversation-sync-direct-alias");
        let client = fixture.client();

        let err = super::resolve_service_conversation_thread(
            &client,
            &crate::messages::ConversationReadRef::new("dm:bob.awiki.test").unwrap(),
        )
        .unwrap_err();

        assert!(matches!(
            err,
            crate::ImError::InvalidInput {
                field: Some(ref field),
                ..
            } if field == "conversation_id"
        ));
    }

    #[test]
    fn sync_conversation_after_rejects_legacy_sorted_direct_alias() {
        let fixture = Fixture::new("conversation-sync-sorted-alias");
        let client = fixture.client();

        let err = super::resolve_service_conversation_thread(
            &client,
            &crate::messages::ConversationReadRef::new("dm:did:example:alice:did:example:bob")
                .unwrap(),
        )
        .unwrap_err();

        assert!(matches!(
            err,
            crate::ImError::InvalidInput {
                field: Some(ref field),
                ..
            } if field == "conversation_id"
        ));
    }

    #[test]
    fn sync_conversation_after_public_path_does_not_route_as_raw_thread() {
        let fixture = Fixture::new("conversation-sync-public-path");
        let client = fixture.client();

        let err = client
            .messages()
            .sync_conversation_after(crate::messages::SyncConversationAfterRequest {
                conversation: crate::messages::ConversationReadRef::new("dm:did:example:bob")
                    .unwrap(),
                after_server_seq: Some("0".to_owned()),
                limit: Some(1),
            })
            .unwrap_err();

        assert!(
            !matches!(
                err,
                crate::ImError::UnsupportedCapability { ref capability }
                    if capability == "sync-thread-after-raw-thread"
            ),
            "sync_conversation_after must resolve conversation_id before calling thread-after"
        );
    }

    #[test]
    fn sync_conversation_after_resolves_group_conversation_to_syncable_thread() {
        let fixture = Fixture::new("conversation-sync-group");
        let client = fixture.client();

        let resolved = super::resolve_service_conversation_thread(
            &client,
            &crate::messages::ConversationReadRef::new("group:did:example:group").unwrap(),
        )
        .unwrap();

        match resolved.thread {
            crate::messages::ThreadRef::Group(group) => {
                assert_eq!(group.as_str(), "did:example:group");
            }
            other => panic!("expected group thread, got {other:?}"),
        }
        assert!(resolved.resolved_did.is_none());
        assert!(resolved.peer_scope.is_none());
    }

    #[test]
    fn sync_conversation_after_resolves_peer_scope_from_projection_metadata() {
        let fixture = Fixture::new("conversation-sync-peer-scope");
        let client = fixture.client();
        let peer_scope = crate::internal::local_state::owner_scope::DirectPeerScope::new(
            "user-bob",
            "Bob.AWiki.Test",
        )
        .unwrap();
        let conversation_id =
            crate::internal::local_state::owner_scope::direct_conversation_id_for_peer_scope(
                &peer_scope,
            );
        fixture.seed_message(crate::internal::local_state::messages::MessageRecord {
            msg_id: "msg-peer-scope-sync".to_owned(),
            conversation_id: conversation_id.clone(),
            thread_id: conversation_id.clone(),
            sender_did: "did:example:bob-current".to_owned(),
            receiver_did: "did:example:alice".to_owned(),
            metadata: json!({
                "peer_user_id": "user-bob",
                "peer_full_handle": "bob.awiki.test",
                "peer_current_did": "did:example:bob-current"
            })
            .to_string(),
            ..Fixture::message_record_defaults()
        });

        let resolved = super::resolve_service_conversation_thread(
            &client,
            &crate::messages::ConversationReadRef::new(conversation_id).unwrap(),
        )
        .unwrap();

        match resolved.thread {
            crate::messages::ThreadRef::Direct(peer) => {
                assert_eq!(peer.as_str(), "did:example:bob-current");
            }
            other => panic!("expected direct thread, got {other:?}"),
        }
        assert_eq!(
            resolved.resolved_did.as_deref(),
            Some("did:example:bob-current")
        );
        let resolved_scope = resolved.peer_scope.expect("peer scope");
        assert_eq!(resolved_scope.user_id, "user-bob");
        assert_eq!(resolved_scope.full_handle, "bob.awiki.test");
    }

    #[test]
    fn peer_scope_conversation_resolves_from_directory_route_before_first_message() {
        let fixture = Fixture::new("conversation-route-before-first-message");
        let client = fixture.client();
        let peer_scope = crate::internal::local_state::owner_scope::DirectPeerScope::new(
            "user-bob",
            "Bob.AWiki.Test",
        )
        .unwrap();
        let conversation_id =
            crate::internal::local_state::owner_scope::direct_conversation_id_for_peer_scope(
                &peer_scope,
            );
        fixture.seed_route(&conversation_id, &peer_scope, "did:example:bob-current");

        let resolved = super::resolve_service_conversation_thread(
            &client,
            &crate::messages::ConversationReadRef::new(conversation_id.clone()).unwrap(),
        )
        .expect("directory route should resolve a canonical empty conversation");

        match resolved.thread {
            crate::messages::ThreadRef::Direct(peer) => {
                assert_eq!(peer.as_str(), "did:example:bob-current");
            }
            other => panic!("expected direct thread, got {other:?}"),
        }
        assert_eq!(
            resolved.resolved_did.as_deref(),
            Some("did:example:bob-current")
        );
        assert_eq!(resolved.peer_scope, Some(peer_scope));
        let connection =
            crate::internal::local_state::open_writable(&fixture.sqlite_path()).unwrap();
        let message_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
            .unwrap();
        assert_eq!(
            message_count, 0,
            "directory routing must not invent a message"
        );
    }

    #[tokio::test]
    async fn async_ensure_conversation_persists_before_returning() {
        let fixture = Fixture::new("conversation-async-ensure-persistence");
        let client = fixture.client();
        let peer_scope = crate::internal::local_state::owner_scope::DirectPeerScope::new(
            "user-bob",
            "bob.awiki.test",
        )
        .unwrap();
        let conversation_id =
            crate::internal::local_state::owner_scope::direct_conversation_id_for_peer_scope(
                &peer_scope,
            );
        fixture.seed_route(&conversation_id, &peer_scope, "did:example:bob-current");

        client
            .messages()
            .ensure_conversation_async(
                crate::messages::ConversationReadRef::new(conversation_id.clone()).unwrap(),
            )
            .await
            .unwrap();

        let connection = rusqlite::Connection::open(fixture.sqlite_path()).unwrap();
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM conversation_registry WHERE owner_identity_id = 'alice-id' AND conversation_id = ?1",
                [conversation_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn directory_route_is_shared_by_text_payload_and_attachment_conversation_surfaces() {
        let fixture = Fixture::new("conversation-route-all-send-surfaces");
        let client = fixture.client();
        let peer_scope = crate::internal::local_state::owner_scope::DirectPeerScope::new(
            "user-bob",
            "bob.awiki.test",
        )
        .unwrap();
        let conversation_id =
            crate::internal::local_state::owner_scope::direct_conversation_id_for_peer_scope(
                &peer_scope,
            );
        fixture.seed_route(&conversation_id, &peer_scope, "did:example:bob-current");

        for body in [
            crate::messages::MessageBody::Text {
                text: "hello".to_owned(),
                kind: crate::messages::MessageKind::Text,
            },
            crate::messages::MessageBody::Payload {
                payload: json!({"text": "hello"}),
            },
        ] {
            let resolved = super::conversation_send_request(
                &client,
                crate::messages::ConversationReadRef::new(conversation_id.clone()).unwrap(),
                body,
                crate::messages::MessageSecurityMode::DefaultPlain,
                None,
                None,
                false,
                None,
            )
            .unwrap();
            assert_eq!(
                resolved.target_did.as_deref(),
                Some("did:example:bob-current")
            );
            assert_eq!(resolved.target_handle.as_deref(), Some("bob.awiki.test"));
            assert_eq!(resolved.peer_scope.as_ref(), Some(&peer_scope));
        }

        let attachment_target = super::resolve_conversation_send_target(
            &client,
            &crate::messages::ConversationReadRef::new(conversation_id).unwrap(),
        )
        .unwrap();
        assert_eq!(
            attachment_target.target_did.as_deref(),
            Some("did:example:bob-current")
        );
        assert_eq!(attachment_target.peer_scope.as_ref(), Some(&peer_scope));
    }

    #[test]
    fn conversation_send_surfaces_reject_legacy_did_conversation() {
        let fixture = Fixture::new("conversation-send-legacy-did");
        let client = fixture.client();
        let conversation = crate::messages::ConversationReadRef::new("dm:did:example:bob").unwrap();

        let text_error = match super::conversation_send_request(
            &client,
            conversation.clone(),
            crate::messages::MessageBody::Text {
                text: "must not send".to_owned(),
                kind: crate::messages::MessageKind::Text,
            },
            crate::messages::MessageSecurityMode::DefaultPlain,
            None,
            None,
            false,
            None,
        ) {
            Ok(_) => panic!("legacy DID conversation must not be sendable"),
            Err(error) => error,
        };
        assert!(matches!(
            text_error,
            crate::ImError::InvalidInput {
                field: Some(ref field),
                ..
            } if field == "conversation_id"
        ));

        let attachment_error = match super::resolve_conversation_send_target(&client, &conversation)
        {
            Ok(_) => panic!("legacy DID conversation must not accept attachments"),
            Err(error) => error,
        };
        assert!(matches!(
            attachment_error,
            crate::ImError::InvalidInput {
                field: Some(ref field),
                ..
            } if field == "conversation_id"
        ));
    }

    #[tokio::test]
    async fn first_canonical_send_projection_creates_only_peer_scope_message_and_summary() {
        let fixture = Fixture::new("conversation-route-first-send-projection");
        let client = fixture.client();
        let peer_scope = crate::internal::local_state::owner_scope::DirectPeerScope::new(
            "user-bob",
            "bob.awiki.test",
        )
        .unwrap();
        let conversation_id =
            crate::internal::local_state::owner_scope::direct_conversation_id_for_peer_scope(
                &peer_scope,
            );
        fixture.seed_route(&conversation_id, &peer_scope, "did:example:bob-current");
        let resolved = super::conversation_send_request(
            &client,
            crate::messages::ConversationReadRef::new(conversation_id.clone()).unwrap(),
            crate::messages::MessageBody::Text {
                text: "first canonical message".to_owned(),
                kind: crate::messages::MessageKind::Text,
            },
            crate::messages::MessageSecurityMode::DefaultPlain,
            Some(crate::ids::MessageId::parse("msg-first-canonical").unwrap()),
            Some("op-first-canonical".to_owned()),
            false,
            None,
        )
        .unwrap();

        crate::internal::message_runtime::local_projection::persist_send_projection_async(
            &client,
            &resolved.request.target,
            &resolved.request.body,
            resolved.request.client_message_id.as_ref().unwrap(),
            resolved.request.delivery.idempotency_key.as_deref(),
            crate::messages::DeliveryState::StoredLocally,
            resolved.target_did.as_deref(),
            resolved.target_handle.as_deref(),
            resolved.peer_scope.as_ref(),
        )
        .await
        .unwrap();

        let connection = rusqlite::Connection::open(fixture.sqlite_path()).unwrap();
        let stored_conversation_id: String = connection
            .query_row(
                "SELECT conversation_id FROM messages WHERE owner_identity_id = 'alice-id' AND msg_id = 'msg-first-canonical'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored_conversation_id, conversation_id);
        let canonical_summary_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM conversation_summaries WHERE owner_identity_id = 'alice-id' AND conversation_id = ?1",
                [conversation_id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(canonical_summary_count, 1);
        let legacy_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE owner_identity_id = 'alice-id' AND conversation_id LIKE 'dm:did:%'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(legacy_count, 0);
    }

    #[test]
    fn corrupt_directory_route_fails_closed_before_message_fallback() {
        let fixture = Fixture::new("conversation-route-corrupt");
        let client = fixture.client();
        let requested_scope = crate::internal::local_state::owner_scope::DirectPeerScope::new(
            "user-bob",
            "bob.awiki.test",
        )
        .unwrap();
        let conversation_id =
            crate::internal::local_state::owner_scope::direct_conversation_id_for_peer_scope(
                &requested_scope,
            );
        fixture.seed_message(crate::internal::local_state::messages::MessageRecord {
            msg_id: "msg-valid-fallback-must-not-mask-corruption".to_owned(),
            conversation_id: conversation_id.clone(),
            thread_id: conversation_id.clone(),
            sender_did: "did:example:bob-current".to_owned(),
            receiver_did: "did:example:alice".to_owned(),
            metadata: json!({
                "peer_user_id": "user-bob",
                "peer_full_handle": "bob.awiki.test",
                "peer_current_did": "did:example:bob-current"
            })
            .to_string(),
            ..Fixture::message_record_defaults()
        });
        let connection =
            crate::internal::local_state::open_writable(&fixture.sqlite_path()).unwrap();
        connection
            .execute(
                r#"
INSERT INTO direct_peer_routes
    (owner_identity_id, conversation_id, peer_user_id, full_handle, current_did, updated_at)
VALUES ('alice-id', ?1, 'user-mallory', 'mallory.awiki.test', 'did:example:mallory', '0')
"#,
                [conversation_id.as_str()],
            )
            .unwrap();

        let err = super::resolve_service_conversation_thread(
            &client,
            &crate::messages::ConversationReadRef::new(conversation_id).unwrap(),
        )
        .unwrap_err();

        assert!(matches!(err, crate::ImError::LocalStateUnavailable { .. }));
        assert!(!err.to_string().contains("mallory"));
    }

    #[test]
    fn sync_conversation_after_peer_scope_prefers_metadata_did_over_legacy_outgoing_rows() {
        let fixture = Fixture::new("conversation-sync-peer-scope-rotation");
        let client = fixture.client();
        let peer_scope = crate::internal::local_state::owner_scope::DirectPeerScope::new(
            "user-bob",
            "bob.awiki.test",
        )
        .unwrap();
        let conversation_id =
            crate::internal::local_state::owner_scope::direct_conversation_id_for_peer_scope(
                &peer_scope,
            );
        fixture.seed_message(crate::internal::local_state::messages::MessageRecord {
            msg_id: "msg-legacy-latest".to_owned(),
            conversation_id: conversation_id.clone(),
            thread_id: conversation_id.clone(),
            direction: 1,
            sender_did: "did:example:alice".to_owned(),
            receiver_did: "did:example:bob-old".to_owned(),
            sent_at: "2026-07-05T00:02:00Z".to_owned(),
            stored_at: "2026-07-05T00:02:00Z".to_owned(),
            ..Fixture::message_record_defaults()
        });
        fixture.seed_message(crate::internal::local_state::messages::MessageRecord {
            msg_id: "msg-metadata-older".to_owned(),
            conversation_id: conversation_id.clone(),
            thread_id: conversation_id.clone(),
            direction: 1,
            sender_did: "did:example:alice".to_owned(),
            receiver_did: "did:example:bob-old".to_owned(),
            sent_at: "2026-07-05T00:01:00Z".to_owned(),
            stored_at: "2026-07-05T00:01:00Z".to_owned(),
            metadata: json!({
                "peer_user_id": "user-bob",
                "peer_full_handle": "bob.awiki.test",
                "peer_current_did": "did:example:bob-current"
            })
            .to_string(),
            ..Fixture::message_record_defaults()
        });

        let resolved = super::resolve_service_conversation_thread(
            &client,
            &crate::messages::ConversationReadRef::new(conversation_id).unwrap(),
        )
        .unwrap();

        match resolved.thread {
            crate::messages::ThreadRef::Direct(peer) => {
                assert_eq!(peer.as_str(), "did:example:bob-current");
            }
            other => panic!("expected direct thread, got {other:?}"),
        }
        assert_eq!(
            resolved.resolved_did.as_deref(),
            Some("did:example:bob-current")
        );
    }

    #[test]
    fn sync_conversation_after_peer_scope_prefers_latest_incoming_did_over_stale_metadata() {
        let fixture = Fixture::new("conversation-sync-peer-scope-latest-incoming");
        let client = fixture.client();
        let peer_scope = crate::internal::local_state::owner_scope::DirectPeerScope::new(
            "user-bob",
            "bob.awiki.test",
        )
        .unwrap();
        let conversation_id =
            crate::internal::local_state::owner_scope::direct_conversation_id_for_peer_scope(
                &peer_scope,
            );
        fixture.seed_message(crate::internal::local_state::messages::MessageRecord {
            msg_id: "msg-stale-metadata-latest".to_owned(),
            conversation_id: conversation_id.clone(),
            thread_id: conversation_id.clone(),
            sender_did: "did:example:bob-current".to_owned(),
            receiver_did: "did:example:alice".to_owned(),
            sent_at: "2026-07-05T00:02:00Z".to_owned(),
            stored_at: "2026-07-05T00:02:00Z".to_owned(),
            metadata: json!({
                "peer_user_id": "user-bob",
                "peer_full_handle": "bob.awiki.test",
                "peer_current_did": "did:example:bob-old"
            })
            .to_string(),
            ..Fixture::message_record_defaults()
        });

        let resolved = super::resolve_service_conversation_thread(
            &client,
            &crate::messages::ConversationReadRef::new(conversation_id).unwrap(),
        )
        .unwrap();

        match resolved.thread {
            crate::messages::ThreadRef::Direct(peer) => {
                assert_eq!(peer.as_str(), "did:example:bob-current");
            }
            other => panic!("expected direct thread, got {other:?}"),
        }
        assert_eq!(
            resolved.resolved_did.as_deref(),
            Some("did:example:bob-current")
        );
    }

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new(prefix: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "im-core-service-{prefix}-{}-{nanos}",
                std::process::id()
            ));
            let identity_root = root.join("identities");
            let identity_dir = identity_root.join("alice");
            fs::create_dir_all(&identity_dir).unwrap();
            fs::create_dir_all(root.join("local")).unwrap();
            fs::write(identity_root.join("default"), "alice\n").unwrap();
            fs::write(
                identity_root.join("registry.json"),
                json!({
                    "default_identity": "alice",
                    "identities": [{
                        "id": "alice-id",
                        "did": "did:example:alice",
                        "local_alias": "alice",
                        "ready_for_auth": true,
                        "ready_for_messaging": true,
                        "missing": []
                    }]
                })
                .to_string(),
            )
            .unwrap();
            fs::write(identity_dir.join("did.json"), "{}").unwrap();
            Self { root }
        }

        fn client(&self) -> crate::core::ImClient {
            crate::core::ImCore::new(
                crate::ImCoreConfig {
                    service_base_url: crate::ServiceEndpoint::parse("https://example.test")
                        .unwrap(),
                    did_domain: "awiki.test".to_owned(),
                    user_service_endpoint: None,
                    message_service_endpoint: None,
                    mail_service_endpoint: None,
                    anp_service_endpoint: None,
                    anp_service_did: None,
                    ca_bundle: None,
                    transport_policy: crate::MessageTransportPolicy::HttpOnly,
                },
                crate::ImCorePaths {
                    identities: crate::IdentityRegistryPaths {
                        identity_root_dir: self.root.join("identities"),
                        registry_path: self.root.join("identities").join("registry.json"),
                        default_identity_path: Some(self.root.join("identities").join("default")),
                    },
                    local_state: crate::LocalStatePaths {
                        sqlite_path: self.sqlite_path(),
                    },
                    runtime: crate::RuntimePaths {
                        cache_dir: self.root.join("cache"),
                        temp_dir: self.root.join("tmp"),
                    },
                },
            )
            .unwrap()
            .client(crate::identity::IdentitySelector::LocalAlias(
                "alice".to_owned(),
            ))
            .unwrap()
        }

        fn sqlite_path(&self) -> PathBuf {
            self.root.join("local").join("im.sqlite")
        }

        fn seed_message(&self, record: crate::internal::local_state::messages::MessageRecord) {
            let connection =
                crate::internal::local_state::open_writable(&self.sqlite_path()).unwrap();
            crate::internal::local_state::messages::upsert_message(&connection, &record).unwrap();
        }

        fn seed_route(
            &self,
            conversation_id: &str,
            peer_scope: &crate::internal::local_state::owner_scope::DirectPeerScope,
            current_did: &str,
        ) {
            let connection =
                crate::internal::local_state::open_writable(&self.sqlite_path()).unwrap();
            let authority = peer_scope
                .full_handle
                .split_once('.')
                .map(|(_, domain)| domain)
                .unwrap();
            let persona = crate::internal::canonical_identity::PeerPersona::from_verified_handle(
                authority,
                &peer_scope.user_id,
                &peer_scope.full_handle,
                Some("active"),
            )
            .unwrap();
            assert_eq!(persona.direct_conversation_id(), conversation_id);
            crate::internal::local_state::peer_personas::upsert(
                &connection,
                &crate::internal::local_state::peer_personas::PeerPersonaRecord {
                    owner_identity_id: "alice-id".to_owned(),
                    persona: persona.clone(),
                    binding_generation: Some("1".to_owned()),
                    subject_type: "human".to_owned(),
                    source: "test_authority".to_owned(),
                    authority_revision: None,
                    verified_at: "2026-07-14T00:00:00Z".to_owned(),
                },
            )
            .unwrap();
            crate::internal::local_state::peer_identifiers::bind(
                &connection,
                &crate::internal::local_state::peer_identifiers::PeerIdentifierRecord {
                    owner_identity_id: "alice-id".to_owned(),
                    peer_persona_id: persona.peer_persona_id.clone(),
                    identifier_kind: "did".to_owned(),
                    identifier_value: current_did.to_owned(),
                    is_current: true,
                    binding_generation: Some("1".to_owned()),
                    source: "test_authority".to_owned(),
                    verified_at: "2026-07-14T00:00:00Z".to_owned(),
                },
            )
            .unwrap();
            let record = crate::internal::local_state::direct_peer_routes::DirectPeerRouteRecord::from_verified_persona(
                "alice-id",
                &persona,
                current_did,
            )
            .unwrap();
            crate::internal::local_state::direct_peer_routes::upsert(&connection, &record).unwrap();
        }

        fn message_record_defaults() -> crate::internal::local_state::messages::MessageRecord {
            crate::internal::local_state::messages::MessageRecord {
                owner_identity_id: "alice-id".to_owned(),
                owner_did: "did:example:alice".to_owned(),
                direction: 0,
                content_type: "text/plain".to_owned(),
                content: "hello from projection".to_owned(),
                sent_at: "2026-07-05T00:00:00Z".to_owned(),
                stored_at: "2026-07-05T00:00:00Z".to_owned(),
                ..crate::internal::local_state::messages::MessageRecord::default()
            }
        }
    }
}

#[cfg(all(test, feature = "sqlite"))]
mod direct_e2ee_async_persistence_tests {
    use serde_json::json;

    use crate::internal::local_state::owner_scope::DirectPeerScope;
    use crate::internal::secure_direct::send::{
        DirectSecureAttachmentLocalEffect, DirectSecureAttachmentSendResult,
        DirectSecureLocalEffect, DirectSecureTextSendResult,
    };

    #[tokio::test]
    async fn deferred_direct_e2ee_success_projection_uses_db_actor() {
        let fixture = Fixture::new("direct-e2ee-success-actor");
        let client = fixture.client();
        let sdk_result = sdk_result("msg-secure-actor", "accepted", Some(17));

        let result = super::persist_deferred_direct_e2ee_effect(
            &client,
            DirectSecureTextSendResult {
                sdk_result,
                queued_outbox_id: None,
                target_did: "did:example:bob".to_owned(),
                target_handle: None,
                peer_scope: None,
                text: "actor persisted secret".to_owned(),
                kind: crate::messages::MessageKind::Text,
                raw: Some(json!({ "accepted": true })),
                local_effect: DirectSecureLocalEffect::PersistOutgoing,
            },
            None,
            None,
        )
        .await
        .unwrap();

        assert_eq!(result.message.id.as_str(), "msg-secure-actor");
        let db = rusqlite::Connection::open(fixture.sqlite_path()).unwrap();
        let stored = db
            .query_row(
                r#"
SELECT content, is_e2ee, server_seq, metadata
FROM messages
WHERE owner_identity_id = 'alice-id' AND msg_id = 'msg-secure-actor'"#,
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(stored.0, "actor persisted secret");
        assert_eq!(stored.1, 1);
        assert_eq!(stored.2, Some(17));
        let metadata: serde_json::Value = serde_json::from_str(&stored.3).unwrap();
        assert_eq!(metadata["security"], "direct-e2ee");
        assert_eq!(metadata["contains_sensitive"], false);
    }

    #[tokio::test]
    async fn deferred_direct_e2ee_peer_scope_projection_and_result_use_same_thread() {
        let fixture = Fixture::new("direct-e2ee-peer-scope");
        let client = fixture.client();
        let peer_scope = DirectPeerScope::new("user-bob", "bob.awiki.test").unwrap();
        let sdk_result = sdk_result("msg-secure-peer-scope", "accepted", Some(18));

        let result = super::persist_deferred_direct_e2ee_effect(
            &client,
            DirectSecureTextSendResult {
                sdk_result,
                queued_outbox_id: None,
                target_did: "did:example:bob-current".to_owned(),
                target_handle: Some("bob.awiki.test".to_owned()),
                peer_scope: Some(peer_scope),
                text: "peer scoped secret".to_owned(),
                kind: crate::messages::MessageKind::Text,
                raw: Some(json!({ "accepted": true })),
                local_effect: DirectSecureLocalEffect::PersistOutgoing,
            },
            None,
            None,
        )
        .await
        .unwrap();
        let result_thread = match &result.message.thread {
            crate::messages::ThreadRef::Thread(thread) => thread.as_str().to_owned(),
            other => panic!("expected peer-scope thread result, got {other:?}"),
        };

        let db = rusqlite::Connection::open(fixture.sqlite_path()).unwrap();
        let stored = db
            .query_row(
                r#"
SELECT conversation_id, receiver_did, metadata
FROM messages
WHERE owner_identity_id = 'alice-id' AND msg_id = 'msg-secure-peer-scope'"#,
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(stored.0, result_thread);
        assert_eq!(stored.1, "did:example:bob-current");
        let metadata: serde_json::Value = serde_json::from_str(&stored.2).unwrap();
        assert_eq!(metadata["security"], "direct-e2ee");
        assert_eq!(metadata["target_handle"], "bob.awiki.test");
        assert_eq!(metadata["peer_user_id"], "user-bob");
        assert_eq!(metadata["peer_full_handle"], "bob.awiki.test");
        assert_eq!(metadata["resolved_target_did"], "did:example:bob-current");
    }

    #[tokio::test]
    async fn deferred_direct_e2ee_attachment_peer_scope_projection_and_result_use_same_thread() {
        let fixture = Fixture::new("direct-e2ee-attachment-peer-scope");
        let client = fixture.client();
        let peer_scope = DirectPeerScope::new("user-bob", "bob.awiki.test").unwrap();
        let sdk_result =
            sdk_attachment_result("msg-secure-attachment-peer-scope", "accepted", Some(19));
        let redacted_manifest = json!({
            "primary_attachment_id": "att-peer",
            "attachments": [{
                "attachment_id": "att-peer",
                "filename": "peer.txt",
                "mime_type": "text/plain",
                "size": "4",
                "object_uri": "awiki://object/peer"
            }]
        });

        let result = super::persist_deferred_direct_e2ee_attachment_effect(
            &client,
            DirectSecureAttachmentSendResult {
                sdk_result,
                target_did: "did:example:bob-current".to_owned(),
                target_handle: Some("bob.awiki.test".to_owned()),
                peer_scope: Some(peer_scope),
                redacted_manifest,
                raw: Some(json!({ "accepted": true })),
                local_effect: DirectSecureAttachmentLocalEffect::PersistOutgoing,
            },
            None,
            None,
        )
        .await
        .unwrap();
        let result_thread = match &result.message.thread {
            crate::messages::ThreadRef::Thread(thread) => thread.as_str().to_owned(),
            other => panic!("expected peer-scope thread result, got {other:?}"),
        };

        let db = rusqlite::Connection::open(fixture.sqlite_path()).unwrap();
        let stored = db
            .query_row(
                r#"
SELECT conversation_id, receiver_did, metadata
FROM messages
WHERE owner_identity_id = 'alice-id' AND msg_id = 'msg-secure-attachment-peer-scope'"#,
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(stored.0, result_thread);
        assert_eq!(stored.1, "did:example:bob-current");
        let metadata: serde_json::Value = serde_json::from_str(&stored.2).unwrap();
        assert_eq!(metadata["security"], "direct-e2ee");
        assert_eq!(metadata["target_handle"], "bob.awiki.test");
        assert_eq!(metadata["peer_user_id"], "user-bob");
        assert_eq!(metadata["peer_full_handle"], "bob.awiki.test");
        assert_eq!(metadata["resolved_target_did"], "did:example:bob-current");
        assert_eq!(metadata["attachment_id"], "att-peer");
    }

    #[tokio::test]
    async fn deferred_direct_e2ee_pending_outbox_uses_db_actor() {
        let fixture = Fixture::new("direct-e2ee-outbox-actor");
        let client = fixture.client();
        let sdk_result = sdk_result("msg-secure-queued", "queued", None);
        let scope = crate::internal::store::e2ee_outbox::E2eeOutboxOwnerScope::for_client(&client);

        super::persist_deferred_direct_e2ee_effect(
            &client,
            DirectSecureTextSendResult {
                sdk_result,
                queued_outbox_id: Some("outbox-actor".to_owned()),
                target_did: "did:example:bob".to_owned(),
                target_handle: None,
                peer_scope: None,
                text: "queued actor secret".to_owned(),
                kind: crate::messages::MessageKind::Markdown,
                raw: None,
                local_effect: DirectSecureLocalEffect::QueueOutbox(
                    crate::internal::store::e2ee_outbox::E2eeOutboxRecord {
                        outbox_id: "outbox-actor".to_owned(),
                        owner_identity_id: scope.owner_identity_id,
                        owner_did: scope.owner_did,
                        credential_name: scope.credential_name,
                        peer_did: "did:example:bob".to_owned(),
                        original_type: "markdown".to_owned(),
                        plaintext: "queued actor secret".to_owned(),
                        local_status: "queued".to_owned(),
                        last_error_code: "pending-confirmation".to_owned(),
                        retry_hint: "retry".to_owned(),
                        ..Default::default()
                    },
                ),
            },
            None,
            None,
        )
        .await
        .unwrap();

        let db = rusqlite::Connection::open(fixture.sqlite_path()).unwrap();
        let stored = db
            .query_row(
                r#"
SELECT peer_did, original_type, plaintext, local_status, last_error_code
FROM e2ee_outbox
WHERE owner_identity_id = 'alice-id' AND outbox_id = 'outbox-actor'"#,
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(stored.0, "did:example:bob");
        assert_eq!(stored.1, "markdown");
        assert_eq!(stored.2, "queued actor secret");
        assert_eq!(stored.3, "queued");
        assert_eq!(stored.4, "pending-confirmation");
    }

    fn sdk_result(
        message_id: &str,
        delivery_state: &str,
        server_sequence: Option<i64>,
    ) -> crate::messages::SendMessageResult {
        crate::messages::SendMessageResult {
            message: crate::messages::Message {
                id: crate::ids::MessageId::parse(message_id).unwrap(),
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                ),
                direction: crate::messages::MessageDirection::Outgoing,
                sender: crate::ids::PeerRef::parse("did:example:alice", "").unwrap(),
                receiver: Some(crate::ids::PeerRef::parse("did:example:bob", "").unwrap()),
                group: None,
                body: crate::messages::MessageBodyView::Text {
                    text: "redacted from db builder".to_owned(),
                    kind: crate::messages::MessageKind::Text,
                },
                sent_at: Some("2026-05-24T00:00:00Z".to_owned()),
                received_at: None,
                metadata: crate::messages::MessageMetadata {
                    operation_id: Some(message_id.to_owned()),
                    delivery_state: Some(delivery_state.to_owned()),
                    send_state: None,
                    retry_plan: None,
                    server_sequence,
                    content_type: Some("text/plain".to_owned()),
                    conversation_identity: None,
                    attributes: vec![crate::messages::MessageMetadataAttribute {
                        key: "security".to_owned(),
                        value: "direct-e2ee".to_owned(),
                    }],
                },
            },
            delivery: crate::messages::DeliveryState::Accepted,
            warnings: Vec::new(),
        }
    }

    fn sdk_attachment_result(
        message_id: &str,
        delivery_state: &str,
        server_sequence: Option<i64>,
    ) -> crate::messages::SendMessageResult {
        crate::messages::SendMessageResult {
            message: crate::messages::Message {
                id: crate::ids::MessageId::parse(message_id).unwrap(),
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                ),
                direction: crate::messages::MessageDirection::Outgoing,
                sender: crate::ids::PeerRef::parse("did:example:alice", "").unwrap(),
                receiver: Some(crate::ids::PeerRef::parse("did:example:bob", "").unwrap()),
                group: None,
                body: crate::messages::MessageBodyView::Unsupported {
                    content_type: Some(
                        crate::attachments::manifest::attachment_manifest_content_type().to_owned(),
                    ),
                },
                sent_at: Some("2026-05-24T00:00:00Z".to_owned()),
                received_at: None,
                metadata: crate::messages::MessageMetadata {
                    operation_id: Some(message_id.to_owned()),
                    delivery_state: Some(delivery_state.to_owned()),
                    send_state: None,
                    retry_plan: None,
                    server_sequence,
                    content_type: Some(
                        crate::attachments::manifest::attachment_manifest_content_type().to_owned(),
                    ),
                    conversation_identity: None,
                    attributes: vec![crate::messages::MessageMetadataAttribute {
                        key: "security".to_owned(),
                        value: "direct-e2ee".to_owned(),
                    }],
                },
            },
            delivery: crate::messages::DeliveryState::Accepted,
            warnings: Vec::new(),
        }
    }

    struct Fixture {
        root: std::path::PathBuf,
    }

    impl Fixture {
        fn new(prefix: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir()
                .join(format!("im-core-{prefix}-{}-{nanos}", std::process::id()));
            let identity_root = root.join("identities");
            let identity_dir = identity_root.join("alice");
            std::fs::create_dir_all(&identity_dir).unwrap();
            std::fs::create_dir_all(root.join("local")).unwrap();
            std::fs::write(identity_root.join("default"), "alice\n").unwrap();
            std::fs::write(
                identity_root.join("registry.json"),
                json!({
                    "default_identity": "alice",
                    "identities": [{
                        "id": "alice-id",
                        "did": "did:example:alice",
                        "local_alias": "alice",
                        "ready_for_auth": true,
                        "ready_for_messaging": true,
                        "missing": []
                    }]
                })
                .to_string(),
            )
            .unwrap();
            std::fs::write(identity_dir.join("did.json"), "{}").unwrap();
            Self { root }
        }

        fn client(&self) -> crate::core::ImClient {
            crate::core::ImCore::new(
                crate::ImCoreConfig {
                    service_base_url: crate::ServiceEndpoint::parse("https://example.test")
                        .unwrap(),
                    did_domain: "awiki.test".to_owned(),
                    user_service_endpoint: None,
                    message_service_endpoint: None,
                    mail_service_endpoint: None,
                    anp_service_endpoint: None,
                    anp_service_did: None,
                    ca_bundle: None,
                    transport_policy: crate::MessageTransportPolicy::HttpOnly,
                },
                crate::ImCorePaths {
                    identities: crate::IdentityRegistryPaths {
                        identity_root_dir: self.root.join("identities"),
                        registry_path: self.root.join("identities").join("registry.json"),
                        default_identity_path: Some(self.root.join("identities").join("default")),
                    },
                    local_state: crate::LocalStatePaths {
                        sqlite_path: self.sqlite_path(),
                    },
                    runtime: crate::RuntimePaths {
                        cache_dir: self.root.join("cache"),
                        temp_dir: self.root.join("tmp"),
                    },
                },
            )
            .unwrap()
            .client(crate::identity::IdentitySelector::LocalAlias(
                "alice".to_owned(),
            ))
            .unwrap()
        }

        fn sqlite_path(&self) -> std::path::PathBuf {
            self.root.join("local").join("im.sqlite")
        }
    }
}

fn sync_diagnostics_from_state(
    state: crate::internal::local_state::sync_v2::SyncDiagnosticsState,
) -> crate::messages::MessageSyncDiagnostics {
    use crate::messages::{MessageSyncDirtyDomain, MessageSyncMode, MessageSyncRetryState};

    let mode = match state.recovery_status.as_deref() {
        Some("recovering" | "downloading" | "applying") => MessageSyncMode::Recovering,
        Some("retryable") => MessageSyncMode::Retryable,
        Some("permanent_failure") => MessageSyncMode::Blocked,
        _ => match state.bootstrap_state.as_deref() {
            None | Some("uninitialized") => MessageSyncMode::Uninitialized,
            Some("recovering") => MessageSyncMode::Recovering,
            Some("blocked") => MessageSyncMode::Blocked,
            _ => MessageSyncMode::Idle,
        },
    };
    let retry_state = if state.in_flight_count > 0 {
        MessageSyncRetryState::InFlight
    } else if state.retryable_count > 0 {
        MessageSyncRetryState::Scheduled
    } else if state.pending_count > 0 {
        MessageSyncRetryState::Pending
    } else if state.permanent_failure_count > 0 {
        MessageSyncRetryState::PermanentFailure
    } else {
        MessageSyncRetryState::None
    };
    let mut dirty_domains = Vec::new();
    if !matches!(mode, MessageSyncMode::Idle) {
        dirty_domains.push(MessageSyncDirtyDomain::Messages);
    }
    if state.pending_mutation_count > 0 || state.permanent_failure_count > 0 {
        dirty_domains.push(MessageSyncDirtyDomain::ReadState);
    }
    crate::messages::MessageSyncDiagnostics {
        last_success_at: state.last_success_at.and_then(unix_seconds_to_rfc3339),
        mode,
        pending_mutation_count: state.pending_mutation_count,
        dirty_domains,
        retry_state,
        next_retry_at: state.next_retry_at.and_then(unix_seconds_to_rfc3339),
    }
}

fn unix_seconds_to_rfc3339(value: i64) -> Option<String> {
    chrono::DateTime::<chrono::Utc>::from_timestamp(value, 0)
        .map(|value| value.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

#[cfg(test)]
mod sync_diagnostics_tests {
    use crate::internal::local_state::sync_v2::SyncDiagnosticsState;
    use crate::messages::{MessageSyncMode, MessageSyncRetryState};

    #[test]
    fn diagnostics_mode_covers_every_public_state() {
        let cases = [
            (None, None, MessageSyncMode::Uninitialized),
            (Some("active"), None, MessageSyncMode::Idle),
            (
                Some("active"),
                Some("applying"),
                MessageSyncMode::Recovering,
            ),
            (
                Some("active"),
                Some("retryable"),
                MessageSyncMode::Retryable,
            ),
            (
                Some("active"),
                Some("permanent_failure"),
                MessageSyncMode::Blocked,
            ),
        ];

        for (bootstrap_state, recovery_status, expected) in cases {
            let diagnostics = super::sync_diagnostics_from_state(SyncDiagnosticsState {
                bootstrap_state: bootstrap_state.map(str::to_owned),
                recovery_status: recovery_status.map(str::to_owned),
                ..Default::default()
            });
            assert_eq!(
                diagnostics.mode, expected,
                "unexpected mode for bootstrap={bootstrap_state:?}, recovery={recovery_status:?}"
            );
        }
    }

    #[test]
    fn diagnostics_retry_state_has_explicit_precedence() {
        let cases = [
            ((0, 0, 0, 0), MessageSyncRetryState::None),
            ((1, 0, 0, 1), MessageSyncRetryState::Pending),
            ((1, 0, 1, 1), MessageSyncRetryState::Scheduled),
            ((1, 1, 1, 1), MessageSyncRetryState::InFlight),
            ((0, 0, 0, 1), MessageSyncRetryState::PermanentFailure),
        ];

        for ((pending, in_flight, retryable, permanent), expected) in cases {
            let diagnostics = super::sync_diagnostics_from_state(SyncDiagnosticsState {
                bootstrap_state: Some("active".to_owned()),
                pending_mutation_count: pending + in_flight + retryable,
                pending_count: pending,
                in_flight_count: in_flight,
                retryable_count: retryable,
                permanent_failure_count: permanent,
                ..Default::default()
            });
            assert_eq!(
                diagnostics.retry_state, expected,
                "unexpected retry state for pending={pending}, in_flight={in_flight}, \
                 retryable={retryable}, permanent={permanent}"
            );
        }
    }

    #[test]
    fn diagnostics_are_typed_and_explicitly_redacted() {
        let diagnostics = super::sync_diagnostics_from_state(SyncDiagnosticsState {
            last_success_at: Some(1_784_800_000),
            bootstrap_state: Some("active".to_owned()),
            recovery_status: Some("retryable".to_owned()),
            pending_mutation_count: 2,
            pending_count: 1,
            retryable_count: 1,
            next_retry_at: Some(1_784_800_060),
            ..Default::default()
        });

        assert_eq!(
            diagnostics.dirty_domains,
            vec![
                crate::messages::MessageSyncDirtyDomain::Messages,
                crate::messages::MessageSyncDirtyDomain::ReadState,
            ]
        );
        let encoded = serde_json::to_string(&diagnostics).unwrap();
        for forbidden in [
            "cursor",
            "stream_epoch",
            "scan_seq",
            "owner_identity_id",
            "recovery_id_hash",
            "message_content",
            "account_id",
            "device_id",
            "payload",
            "token",
            "message_body",
        ] {
            assert!(
                !encoded.contains(forbidden),
                "diagnostics leaked forbidden field {forbidden}"
            );
        }
    }
}

impl<'a> MessageService<'a> {
    pub(crate) fn new(client: &'a crate::core::ImClient) -> Self {
        Self { client }
    }

    pub fn send(
        &self,
        request: super::SendMessageRequest,
    ) -> crate::ImResult<super::SendMessageResult> {
        validate_body(&request.body)?;
        validate_send_mode(&request.target, &request.security)?;
        validate_attachment_security(&request.body, &request.security)?;
        validate_delegated_send_scope(&request)?;
        match (&request.target, &request.security) {
            (
                super::MessageTarget::Direct(_),
                super::MessageSecurityMode::E2eeRequired | super::MessageSecurityMode::SecureDirect,
            ) => self.send_direct_e2ee(resolve_send_request(self.client, request)?),
            (
                super::MessageTarget::Direct(_),
                super::MessageSecurityMode::DefaultPlain | super::MessageSecurityMode::Plain,
            ) if matches!(request.body, super::MessageBody::Attachment { .. }) => {
                self.send_plain_attachment(request)
            }
            (super::MessageTarget::Direct(_), _) => {
                let resolved = resolve_send_request(self.client, request)?;
                let direct_handle = resolved.direct_handle().map(str::to_owned);
                let peer_scope = resolved.peer_scope.clone();
                let mut result = crate::internal::message_runtime::direct::DirectTextSender::new(
                    self.client,
                    crate::internal::auth::session::FileSessionProvider::new(self.client),
                    crate::internal::transport::CoreHttpTransport::new(self.client),
                )
                .send(
                    crate::internal::message_runtime::direct::DirectTextSend {
                        request: resolved.request.clone(),
                        resolved_target_did: resolved.target_did.clone(),
                        credentials: None,
                    },
                )?;
                normalize_resolved_direct_send_result(&mut result.sdk_result, &resolved)?;
                #[cfg(feature = "sqlite")]
                match
                    crate::internal::message_runtime::local_projection::persist_direct_outgoing_result(
                        self.client,
                        &result.target_did,
                        direct_handle.as_deref(),
                        peer_scope.as_ref(),
                        &result.sdk_result,
                    )
                {
                    Ok(()) => self
                        .client
                        .emit_committed_local_message_projection("local_send"),
                    Err(err) => {
                        result
                            .sdk_result
                            .warnings
                            .push(format!("Failed to persist local message: {err}"));
                    }
                }
                Ok(result.sdk_result)
            }
            (
                super::MessageTarget::Group(_),
                super::MessageSecurityMode::E2eeRequired | super::MessageSecurityMode::GroupE2ee,
            ) => self.send_group_e2ee(request),
            (
                super::MessageTarget::Group(_),
                super::MessageSecurityMode::DefaultPlain | super::MessageSecurityMode::Plain,
            ) if matches!(request.body, super::MessageBody::Attachment { .. }) => {
                self.send_plain_attachment(request)
            }
            (super::MessageTarget::Group(_), _) => {
                let mut result = crate::internal::message_runtime::group::GroupTextSender::new(
                    self.client,
                    crate::internal::auth::session::FileSessionProvider::new(self.client),
                    crate::internal::transport::CoreHttpTransport::new(self.client),
                )
                .send(crate::internal::message_runtime::group::GroupTextSend {
                    request,
                    credentials: None,
                })?;
                #[cfg(feature = "sqlite")]
                match
                    crate::internal::message_runtime::local_projection::persist_group_outgoing_result(
                        self.client,
                        &result.group_did,
                        &result.sdk_result,
                    )
                {
                    Ok(()) => self
                        .client
                        .emit_committed_local_message_projection("local_send"),
                    Err(err) => {
                        result
                            .sdk_result
                            .warnings
                            .push(format!("Failed to persist local group message: {err}"));
                    }
                }
                Ok(result.sdk_result)
            }
        }
    }

    pub(crate) fn send_secure_attachment(
        &self,
        request: super::SendMessageRequest,
    ) -> crate::ImResult<super::SendMessageResult> {
        validate_body(&request.body)?;
        validate_send_mode(&request.target, &request.security)?;
        validate_secure_attachment_request(&request.body, &request.security)?;
        match (&request.target, &request.security) {
            (
                super::MessageTarget::Direct(_),
                super::MessageSecurityMode::E2eeRequired | super::MessageSecurityMode::SecureDirect,
            ) => self.send_direct_e2ee(resolve_send_request(self.client, request)?),
            (
                super::MessageTarget::Group(_),
                super::MessageSecurityMode::E2eeRequired | super::MessageSecurityMode::GroupE2ee,
            ) => self.send_group_e2ee(request),
            _ => Err(crate::ImError::unsupported("secure-attachment")),
        }
    }

    pub async fn send_async(
        &self,
        request: super::SendMessageRequest,
    ) -> crate::ImResult<super::SendMessageResult> {
        validate_body(&request.body)?;
        validate_send_mode(&request.target, &request.security)?;
        validate_attachment_security(&request.body, &request.security)?;
        validate_delegated_send_scope(&request)?;
        match (&request.target, &request.security) {
            (
                super::MessageTarget::Direct(_),
                super::MessageSecurityMode::E2eeRequired | super::MessageSecurityMode::SecureDirect,
            ) => {
                self.send_direct_e2ee_async(resolve_send_request_async(self.client, request).await?)
                    .await
            }
            (
                super::MessageTarget::Direct(_),
                super::MessageSecurityMode::DefaultPlain | super::MessageSecurityMode::Plain,
            ) if matches!(request.body, super::MessageBody::Attachment { .. }) => {
                self.send_plain_attachment_async(request).await
            }
            (super::MessageTarget::Direct(_), _) => {
                let resolved = resolve_send_request_async(self.client, request).await?;
                let direct_handle = resolved.direct_handle().map(str::to_owned);
                let peer_scope = resolved.peer_scope.clone();
                let mut result = crate::internal::message_runtime::direct::DirectTextSender::new(
                    self.client,
                    crate::internal::auth::session::FileSessionProvider::new(self.client),
                    crate::internal::transport::CoreHttpTransport::new(self.client),
                )
                .send_async(crate::internal::message_runtime::direct::DirectTextSend {
                    request: resolved.request.clone(),
                    resolved_target_did: resolved.target_did.clone(),
                    credentials: None,
                })
                .await?;
                normalize_resolved_direct_send_result(&mut result.sdk_result, &resolved)?;
                #[cfg(feature = "sqlite")]
                match
                    crate::internal::message_runtime::local_projection::persist_direct_outgoing_result_async(
                        self.client,
                        &result.target_did,
                        direct_handle.as_deref(),
                        peer_scope.as_ref(),
                        &result.sdk_result,
                    )
                    .await
                {
                    Ok(()) => self
                        .client
                        .emit_committed_local_message_projection("local_send"),
                    Err(err) => {
                        result
                            .sdk_result
                            .warnings
                            .push(format!("Failed to persist local message: {err}"));
                    }
                }
                Ok(result.sdk_result)
            }
            (
                super::MessageTarget::Group(_),
                super::MessageSecurityMode::E2eeRequired | super::MessageSecurityMode::GroupE2ee,
            ) => self.send_group_e2ee_async(request).await,
            (
                super::MessageTarget::Group(_),
                super::MessageSecurityMode::DefaultPlain | super::MessageSecurityMode::Plain,
            ) if matches!(request.body, super::MessageBody::Attachment { .. }) => {
                self.send_plain_attachment_async(request).await
            }
            (super::MessageTarget::Group(_), _) => {
                let mut result = crate::internal::message_runtime::group::GroupTextSender::new(
                    self.client,
                    crate::internal::auth::session::FileSessionProvider::new(self.client),
                    crate::internal::transport::CoreHttpTransport::new(self.client),
                )
                .send_async(crate::internal::message_runtime::group::GroupTextSend {
                    request,
                    credentials: None,
                })
                .await?;
                #[cfg(feature = "sqlite")]
                match
                    crate::internal::message_runtime::local_projection::persist_group_outgoing_result_async(
                        self.client,
                        &result.group_did,
                        &result.sdk_result,
                    )
                    .await
                {
                    Ok(()) => self
                        .client
                        .emit_committed_local_message_projection("local_send"),
                    Err(err) => {
                        result
                            .sdk_result
                            .warnings
                            .push(format!("Failed to persist local group message: {err}"));
                    }
                }
                Ok(result.sdk_result)
            }
        }
    }

    pub async fn send_conversation_text_async(
        &self,
        request: super::SendConversationTextRequest,
    ) -> crate::ImResult<super::SendMessageResult> {
        let body = super::MessageBody::Text {
            text: request.text,
            kind: if request.markdown {
                super::MessageKind::Markdown
            } else {
                super::MessageKind::Text
            },
        };
        let send_request = conversation_send_request(
            self.client,
            request.conversation,
            body,
            request.security,
            request.client_message_id,
            request.idempotency_key,
            request.wait_for_final_acceptance,
            request.delegated_signing,
        )?;
        self.send_conversation_request_async(send_request).await
    }

    pub async fn send_conversation_payload_async(
        &self,
        request: super::SendConversationPayloadRequest,
    ) -> crate::ImResult<super::SendMessageResult> {
        let send_request = conversation_send_request(
            self.client,
            request.conversation,
            super::MessageBody::Payload {
                payload: request.payload,
            },
            request.security,
            request.client_message_id,
            request.idempotency_key,
            request.wait_for_final_acceptance,
            request.delegated_signing,
        )?;
        self.send_conversation_request_async(send_request).await
    }

    async fn send_conversation_request_async(
        &self,
        resolved: ResolvedConversationSendRequest,
    ) -> crate::ImResult<super::SendMessageResult> {
        if conversation_send_uses_security_runtime(&resolved.request.security) {
            return self.send_async(resolved.request).await;
        }
        validate_plain_conversation_send(&resolved.request)?;
        let message_id = resolved
            .request
            .client_message_id
            .as_ref()
            .expect("conversation send request should always have a client message id")
            .clone();
        let operation_id = resolved.request.delivery.idempotency_key.as_deref();
        let target_did = resolved.target_did.as_deref();
        let target_handle = resolved.target_handle.as_deref();
        let peer_scope = resolved.peer_scope.as_ref();

        crate::internal::message_runtime::local_projection::persist_send_projection_async(
            self.client,
            &resolved.request.target,
            &resolved.request.body,
            &message_id,
            operation_id,
            super::DeliveryState::StoredLocally,
            target_did,
            target_handle,
            peer_scope,
        )
        .await?;
        self.client
            .emit_committed_local_message_projection("local_send_pending");

        match self.send_plain_conversation_network_async(&resolved).await {
            Ok(result) => Ok(result),
            Err(err) => {
                let mut failed_resolved = resolved.clone();
                let err = match self
                    .stale_delivery_retry_target_async(&resolved, &err)
                    .await
                {
                    Ok(Some(retry_resolved)) => {
                        match self
                            .send_plain_conversation_network_async(&retry_resolved)
                            .await
                        {
                            Ok(result) => return Ok(result),
                            Err(retry_err) => {
                                failed_resolved = retry_resolved;
                                retry_err
                            }
                        }
                    }
                    Ok(None) => err,
                    Err(resolve_err) => resolve_err,
                };
                self.persist_failed_conversation_send_projection_async(&failed_resolved, &err)
                    .await;
                Err(err)
            }
        }
    }

    async fn stale_delivery_retry_target_async(
        &self,
        resolved: &ResolvedConversationSendRequest,
        err: &crate::ImError,
    ) -> crate::ImResult<Option<ResolvedConversationSendRequest>> {
        let Some(stale) = stale_delivery_target_from_error(err, self.client.did().as_str()) else {
            return Ok(None);
        };
        let super::MessageTarget::Direct(_) = &resolved.request.target else {
            return Ok(None);
        };

        let mut current_did = stale.current_did;
        let mut full_handle = stale
            .full_handle
            .or_else(|| resolved.target_handle.clone())
            .or_else(|| {
                resolved
                    .peer_scope
                    .as_ref()
                    .map(|scope| scope.full_handle.clone())
            });
        let mut authority_subject_id = None;

        if let Some(handle) = full_handle.clone() {
            // The retry scope must come from the same authoritative Handle
            // route as a normal send. Never trust deployment-private IDs from
            // an error payload, especially for a cross-domain peer.
            let lookup = crate::internal::handle_discovery::resolve_direct_handle_async(
                self.client,
                &handle,
            )
            .await?;
            current_did = Some(lookup.target_did);
            full_handle = Some(lookup.full_handle);
            authority_subject_id = Some(lookup.authority_subject_id);
        }

        let Some(current_did) = current_did
            .as_deref()
            .and_then(|did| canonical_peer_did_value(did, self.client.did().as_str()))
        else {
            return Ok(None);
        };
        if resolved
            .target_did
            .as_deref()
            .is_some_and(|did| did.trim() == current_did)
        {
            return Ok(None);
        }

        let full_handle = full_handle
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());
        let peer_scope = match (authority_subject_id, full_handle.as_ref()) {
            (Some(authority_subject_id), Some(handle)) => Some(
                crate::internal::local_state::owner_scope::DirectPeerScope::new(
                    authority_subject_id,
                    handle.clone(),
                )?,
            ),
            _ => None,
        };
        let target_peer = match full_handle.as_deref() {
            Some(handle) => crate::ids::PeerRef::parse(handle, "")?,
            None => crate::ids::PeerRef::parse(&current_did, "")?,
        };

        let mut retry = resolved.clone();
        retry.request.target = super::MessageTarget::Direct(target_peer);
        retry.target_did = Some(current_did.clone());
        retry.target_handle = full_handle;
        retry.peer_scope = peer_scope;
        #[cfg(feature = "sqlite")]
        if retry.conversation_id.starts_with("dm:peer-scope:") {
            let Some(peer_scope) = retry.peer_scope.as_ref() else {
                return Err(unresolvable_service_conversation(&retry.conversation_id));
            };
            let route =
                crate::internal::local_state::direct_peer_routes::DirectPeerRouteRecord::for_client(
                    self.client,
                    &retry.conversation_id,
                    peer_scope,
                    &current_did,
                )?;
            self.client
                .core_inner()
                .local_state_db()
                .await?
                .upsert_direct_peer_route(route)
                .await?;
        }
        Ok(Some(retry))
    }

    async fn persist_failed_conversation_send_projection_async(
        &self,
        resolved: &ResolvedConversationSendRequest,
        err: &crate::ImError,
    ) {
        let Some(message_id) = resolved.request.client_message_id.as_ref() else {
            return;
        };
        let failed =
            crate::internal::message_runtime::local_projection::persist_send_projection_async(
                self.client,
                &resolved.request.target,
                &resolved.request.body,
                message_id,
                resolved.request.delivery.idempotency_key.as_deref(),
                super::DeliveryState::Failed {
                    reason: err.to_string(),
                },
                resolved.target_did.as_deref(),
                resolved.target_handle.as_deref(),
                resolved.peer_scope.as_ref(),
            )
            .await;
        match failed {
            Ok(_) => self
                .client
                .emit_committed_local_message_projection("local_send_failed"),
            Err(_persist_err) => {}
        }
    }

    async fn send_plain_conversation_network_async(
        &self,
        resolved: &ResolvedConversationSendRequest,
    ) -> crate::ImResult<super::SendMessageResult> {
        match &resolved.request.target {
            super::MessageTarget::Direct(_) => {
                let mut result = crate::internal::message_runtime::direct::DirectTextSender::new(
                    self.client,
                    crate::internal::auth::session::FileSessionProvider::new(self.client),
                    crate::internal::transport::CoreHttpTransport::new(self.client),
                )
                .send_async(crate::internal::message_runtime::direct::DirectTextSend {
                    request: resolved.request.clone(),
                    resolved_target_did: resolved.target_did.clone(),
                    credentials: None,
                })
                .await?;
                normalize_direct_send_result_for_peer_scope(
                    &mut result.sdk_result,
                    resolved.peer_scope.as_ref(),
                    resolved.target_handle.as_deref(),
                    Some(result.target_did.as_str()),
                )?;
                #[cfg(feature = "sqlite")]
                match crate::internal::message_runtime::local_projection::persist_direct_outgoing_result_async(
                    self.client,
                    &result.target_did,
                    resolved.target_handle.as_deref(),
                    resolved.peer_scope.as_ref(),
                    &result.sdk_result,
                )
                .await
                {
                    Ok(()) => self
                        .client
                        .emit_committed_local_message_projection("local_send"),
                    Err(err) => {
                        result
                            .sdk_result
                            .warnings
                            .push(format!("Failed to persist local conversation message: {err}"));
                    }
                }
                Ok(result.sdk_result)
            }
            super::MessageTarget::Group(_) => {
                let mut result = crate::internal::message_runtime::group::GroupTextSender::new(
                    self.client,
                    crate::internal::auth::session::FileSessionProvider::new(self.client),
                    crate::internal::transport::CoreHttpTransport::new(self.client),
                )
                .send_async(crate::internal::message_runtime::group::GroupTextSend {
                    request: resolved.request.clone(),
                    credentials: None,
                })
                .await?;
                #[cfg(feature = "sqlite")]
                match crate::internal::message_runtime::local_projection::persist_group_outgoing_result_async(
                    self.client,
                    &result.group_did,
                    &result.sdk_result,
                )
                .await
                {
                    Ok(()) => self
                        .client
                        .emit_committed_local_message_projection("local_send"),
                    Err(err) => {
                        result
                            .sdk_result
                            .warnings
                            .push(format!("Failed to persist local conversation group message: {err}"));
                    }
                }
                Ok(result.sdk_result)
            }
        }
    }

    pub(crate) async fn send_secure_attachment_async(
        &self,
        request: super::SendMessageRequest,
    ) -> crate::ImResult<super::SendMessageResult> {
        validate_body(&request.body)?;
        validate_send_mode(&request.target, &request.security)?;
        validate_secure_attachment_request(&request.body, &request.security)?;
        match (&request.target, &request.security) {
            (
                super::MessageTarget::Direct(_),
                super::MessageSecurityMode::E2eeRequired | super::MessageSecurityMode::SecureDirect,
            ) => {
                self.send_direct_e2ee_async(resolve_send_request_async(self.client, request).await?)
                    .await
            }
            (
                super::MessageTarget::Group(_),
                super::MessageSecurityMode::E2eeRequired | super::MessageSecurityMode::GroupE2ee,
            ) => self.send_group_e2ee_async(request).await,
            _ => Err(crate::ImError::unsupported("secure-attachment")),
        }
    }

    fn send_plain_attachment(
        &self,
        request: super::SendMessageRequest,
    ) -> crate::ImResult<super::SendMessageResult> {
        let target = request.target.clone();
        let client_message_id = request.client_message_id.clone();
        let attachment = attachment_request_from_message_request(request)?;
        match client_message_id {
            Some(client_message_id) => self.client.attachments().send_with_client_message_id(
                target,
                attachment,
                client_message_id,
            ),
            None => self.client.attachments().send(target, attachment),
        }
        .map(|result| result.message)
    }

    async fn send_plain_attachment_async(
        &self,
        request: super::SendMessageRequest,
    ) -> crate::ImResult<super::SendMessageResult> {
        let target = request.target.clone();
        let client_message_id = request.client_message_id.clone();
        let attachment = attachment_request_from_message_request(request)?;
        match client_message_id {
            Some(client_message_id) => {
                self.client
                    .attachments()
                    .send_with_client_message_id_async(target, attachment, client_message_id)
                    .await
            }
            None => {
                self.client
                    .attachments()
                    .send_async(target, attachment)
                    .await
            }
        }
        .map(|result| result.message)
    }

    fn send_direct_e2ee(
        &self,
        resolved: ResolvedSendRequest,
    ) -> crate::ImResult<super::SendMessageResult> {
        if super::v2_product::direct_enabled_for_client(self.client)? {
            return super::v2_product::send_direct(self.client, resolved);
        }
        #[cfg(all(feature = "sqlite", feature = "blocking"))]
        {
            send_direct_e2ee_with_client(self.client, resolved)
        }
        #[cfg(all(feature = "sqlite", not(feature = "blocking")))]
        {
            let _ = resolved;
            Err(crate::ImError::unsupported("sync-secure-direct-send"))
        }
        #[cfg(not(feature = "sqlite"))]
        {
            let _ = resolved;
            Err(crate::ImError::unsupported("secure-direct"))
        }
    }

    #[cfg(feature = "sqlite")]
    async fn send_direct_e2ee_async(
        &self,
        resolved: ResolvedSendRequest,
    ) -> crate::ImResult<super::SendMessageResult> {
        if super::v2_product::direct_enabled_for_client(self.client)? {
            return super::v2_product::send_direct_async(self.client, resolved).await;
        }
        let target_handle = resolved.direct_handle().map(str::to_owned);
        let peer_scope = resolved.peer_scope.clone();
        if matches!(resolved.request.body, super::MessageBody::Attachment { .. }) {
            let committed =
                crate::internal::attachment_runtime::upload::AttachmentUploadRuntime::new(
                    self.client,
                    crate::internal::auth::session::FileSessionProvider::new(self.client),
                    crate::internal::transport::CoreHttpTransport::new(self.client),
                )
                .prepare_and_commit_object_async(
                    crate::internal::attachment_runtime::upload::AttachmentPrepareObjectInput {
                        target: resolved.request.target.clone(),
                        request: attachment_request_from_message_request(resolved.request.clone())?,
                        resolved_target_did: resolved.target_did.clone(),
                        message_security_profile: "direct-e2ee",
                    },
                )
                .await?;
            let async_result =
                crate::internal::secure_direct::async_send::AsyncDirectSecureTextSender::new(
                    self.client,
                    crate::internal::auth::session::FileSessionProvider::new(self.client),
                    crate::internal::transport::CoreHttpTransport::new(self.client),
                    crate::internal::transport::CoreHttpTransport::new(self.client),
                )
                .send_attachment_async_if_ready(
                    crate::internal::secure_direct::send::DirectSecureAttachmentSend {
                        request: resolved.request.clone(),
                        resolved_target_did: resolved.target_did.clone(),
                        target_handle: target_handle.clone(),
                        peer_scope: peer_scope.clone(),
                        committed: committed.clone(),
                        local_persistence:
                            crate::internal::secure_direct::send::DirectSecureLocalPersistence::Deferred,
                    },
                )
                .await?;
            if let Some(result) = async_result {
                return persist_deferred_direct_e2ee_attachment_effect(
                    self.client,
                    result,
                    target_handle.as_deref(),
                    peer_scope.as_ref(),
                )
                .await;
            }
            #[cfg(feature = "blocking")]
            {
                let client = self.client.clone();
                let result = crate::internal::runtime::worker::run_blocking(move || {
                    send_direct_e2ee_attachment_with_client_and_persistence(
                        &client,
                        resolved,
                        committed,
                        crate::internal::secure_direct::send::DirectSecureLocalPersistence::Deferred,
                    )
                })
                .await
                .map_err(|err| crate::ImError::Internal {
                    message: err.to_string(),
                })??;
                return persist_deferred_direct_e2ee_attachment_effect(
                    self.client,
                    result,
                    target_handle.as_deref(),
                    peer_scope.as_ref(),
                )
                .await;
            }
            #[cfg(not(feature = "blocking"))]
            {
                let _ = committed;
                return Err(crate::ImError::LocalStateUnavailable {
                    detail: "direct E2EE async attachment send requires an established local session; sync compatibility fallback is disabled".to_owned(),
                });
            }
        }
        let async_input = crate::internal::secure_direct::send::DirectSecureTextSend {
            request: resolved.request.clone(),
            resolved_target_did: resolved.target_did.clone(),
            target_handle: target_handle.clone(),
            peer_scope: peer_scope.clone(),
            local_persistence:
                crate::internal::secure_direct::send::DirectSecureLocalPersistence::Deferred,
        };
        match crate::internal::secure_direct::async_send::AsyncDirectSecureTextSender::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .send_async_if_ready(async_input)
        .await?
        {
            crate::internal::secure_direct::async_send::AsyncDirectSecureSendOutcome::Sent(
                result,
            ) => {
                return persist_deferred_direct_e2ee_effect(
                    self.client,
                    result,
                    target_handle.as_deref(),
                    peer_scope.as_ref(),
                )
                .await
            }
            crate::internal::secure_direct::async_send::AsyncDirectSecureSendOutcome::Fallback(
                crate::internal::secure_direct::async_send::AsyncDirectSecureSendFallback::NoEstablishedSession,
            ) => {}
        }

        #[cfg(feature = "blocking")]
        {
            let client = self.client.clone();
            let result = crate::internal::runtime::worker::run_blocking(move || {
                send_direct_e2ee_with_client_and_persistence(
                    &client,
                    resolved,
                    crate::internal::secure_direct::send::DirectSecureLocalPersistence::Deferred,
                )
            })
            .await
            .map_err(|err| crate::ImError::Internal {
                message: err.to_string(),
            })??;
            persist_deferred_direct_e2ee_effect(
                self.client,
                result,
                target_handle.as_deref(),
                peer_scope.as_ref(),
            )
            .await
        }
        #[cfg(not(feature = "blocking"))]
        {
            let _ = resolved;
            Err(crate::ImError::LocalStateUnavailable {
                detail: "direct E2EE async send requires an established local session; sync compatibility fallback is disabled".to_owned(),
            })
        }
    }

    #[cfg(not(feature = "sqlite"))]
    async fn send_direct_e2ee_async(
        &self,
        resolved: ResolvedSendRequest,
    ) -> crate::ImResult<super::SendMessageResult> {
        let _ = resolved;
        Err(crate::ImError::unsupported("secure-direct"))
    }

    #[cfg(feature = "group-e2ee")]
    fn send_group_e2ee(
        &self,
        request: super::SendMessageRequest,
    ) -> crate::ImResult<super::SendMessageResult> {
        if self.client.core_inner().group_e2ee_v2_enabled() {
            return super::v2_product::send_group(self.client, request);
        }
        #[cfg(feature = "blocking")]
        {
            let session_provider =
                crate::internal::auth::session::FileSessionProvider::new(self.client);
            let mut transport = crate::internal::transport::CoreHttpTransport::new(self.client);
            crate::internal::group_e2ee::lifecycle::ensure_group_e2ee_service_available(
                self.client,
                &session_provider,
                &mut transport,
                crate::internal::group_e2ee::lifecycle::GroupE2eeServiceAvailabilityInput {
                    credentials: None,
                    service_did: None,
                    check_key_package: false,
                },
            )?;
            let provider =
                crate::internal::group_e2ee::storage::native_provider_for_client(self.client)?;
            if matches!(request.body, super::MessageBody::Attachment { .. }) {
                let committed =
                    crate::internal::attachment_runtime::upload::AttachmentUploadRuntime::new(
                        self.client,
                        crate::internal::auth::session::FileSessionProvider::new(self.client),
                        crate::internal::transport::CoreHttpTransport::new(self.client),
                    )
                    .prepare_and_commit_object(
                        crate::internal::attachment_runtime::upload::AttachmentPrepareObjectInput {
                            target: request.target.clone(),
                            request: attachment_request_from_message_request(request.clone())?,
                            resolved_target_did: None,
                            message_security_profile: "group-e2ee",
                        },
                    )?;
                return crate::internal::group_e2ee::runtime::GroupE2eeTextSender::new(
                    self.client,
                    crate::internal::auth::session::FileSessionProvider::new(self.client),
                    crate::internal::transport::CoreHttpTransport::new(self.client),
                    provider,
                )
                .send_attachment(
                    crate::internal::group_e2ee::runtime::GroupE2eeAttachmentSend {
                        request,
                        group_state_ref: None,
                        credentials: None,
                        committed,
                    },
                )
                .map(|result| result.sdk_result);
            }
            crate::internal::group_e2ee::runtime::GroupE2eeTextSender::new(
                self.client,
                crate::internal::auth::session::FileSessionProvider::new(self.client),
                crate::internal::transport::CoreHttpTransport::new(self.client),
                provider,
            )
            .send(crate::internal::group_e2ee::runtime::GroupE2eeTextSend {
                request,
                group_state_ref: None,
                credentials: None,
            })
            .map(|result| result.sdk_result)
        }
        #[cfg(not(feature = "blocking"))]
        {
            let _ = request;
            Err(crate::ImError::unsupported("sync-group-e2ee-send"))
        }
    }

    #[cfg(not(feature = "group-e2ee"))]
    fn send_group_e2ee(
        &self,
        _request: super::SendMessageRequest,
    ) -> crate::ImResult<super::SendMessageResult> {
        Err(crate::ImError::unsupported("group-e2ee"))
    }

    #[cfg(feature = "group-e2ee")]
    async fn send_group_e2ee_async(
        &self,
        request: super::SendMessageRequest,
    ) -> crate::ImResult<super::SendMessageResult> {
        if self.client.core_inner().group_e2ee_v2_enabled() {
            return super::v2_product::send_group_async(self.client, request).await;
        }
        let provider =
            crate::internal::group_e2ee::storage::native_provider_for_client(self.client)?;
        if matches!(request.body, super::MessageBody::Attachment { .. }) {
            let committed =
                crate::internal::attachment_runtime::upload::AttachmentUploadRuntime::new(
                    self.client,
                    crate::internal::auth::session::FileSessionProvider::new(self.client),
                    crate::internal::transport::CoreHttpTransport::new(self.client),
                )
                .prepare_and_commit_object_async(
                    crate::internal::attachment_runtime::upload::AttachmentPrepareObjectInput {
                        target: request.target.clone(),
                        request: attachment_request_from_message_request(request.clone())?,
                        resolved_target_did: None,
                        message_security_profile: "group-e2ee",
                    },
                )
                .await?;
            return crate::internal::group_e2ee::runtime::GroupE2eeTextSender::new(
                self.client,
                crate::internal::auth::session::FileSessionProvider::new(self.client),
                crate::internal::transport::CoreHttpTransport::new(self.client),
                provider,
            )
            .send_attachment_async(
                crate::internal::group_e2ee::runtime::GroupE2eeAttachmentSend {
                    request,
                    group_state_ref: None,
                    credentials: None,
                    committed,
                },
            )
            .await
            .map(|result| result.sdk_result);
        }
        match crate::internal::group_e2ee::runtime::GroupE2eeTextSender::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
            provider,
        )
        .send_async(crate::internal::group_e2ee::runtime::GroupE2eeTextSend {
            request: request.clone(),
            group_state_ref: None,
            credentials: None,
        })
        .await
        {
            Ok(result) => Ok(result.sdk_result),
            Err(err)
                if crate::internal::group_e2ee::runtime::is_group_e2ee_epoch_mismatch(&err) =>
            {
                #[cfg(feature = "blocking")]
                {
                    let client = self.client.clone();
                    crate::internal::runtime::worker::run_blocking(move || {
                        client.messages().send_group_e2ee(request)
                    })
                    .await
                    .map_err(|join| crate::ImError::Internal {
                        message: join.to_string(),
                    })?
                }
                #[cfg(not(feature = "blocking"))]
                {
                    let _ = request;
                    Err(err)
                }
            }
            Err(err) => Err(err),
        }
    }

    #[cfg(not(feature = "group-e2ee"))]
    async fn send_group_e2ee_async(
        &self,
        _request: super::SendMessageRequest,
    ) -> crate::ImResult<super::SendMessageResult> {
        Err(crate::ImError::unsupported("group-e2ee"))
    }

    pub fn inbox(
        &self,
        query: super::InboxQuery,
    ) -> crate::ImResult<crate::ids::Page<super::Message>> {
        self.inbox_with_metadata(query)
            .map(super::MessagePage::into_page)
    }

    pub async fn inbox_async(
        &self,
        query: super::InboxQuery,
    ) -> crate::ImResult<crate::ids::Page<super::Message>> {
        self.inbox_with_metadata_async(query)
            .await
            .map(super::MessagePage::into_page)
    }

    pub fn inbox_with_metadata(
        &self,
        query: super::InboxQuery,
    ) -> crate::ImResult<super::MessagePage> {
        crate::internal::message_runtime::read::MessageReadRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .inbox(crate::internal::message_runtime::read::InboxRead { query })
        .map(message_page_from_read_result)?
    }

    pub async fn inbox_with_metadata_async(
        &self,
        query: super::InboxQuery,
    ) -> crate::ImResult<super::MessagePage> {
        crate::internal::message_runtime::read::MessageReadRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .inbox_async(crate::internal::message_runtime::read::InboxRead { query })
        .await
        .map(message_page_from_read_result)?
    }

    /// Reads the exact active vNext owner's committed local incoming Inbox.
    /// This method never performs a remote Inbox or synchronization request.
    pub async fn local_inbox_projection_with_metadata_async(
        &self,
        query: super::InboxQuery,
    ) -> crate::ImResult<super::MessagePage> {
        if query.cursor.is_some() {
            return Err(crate::ImError::invalid_input(
                Some("cursor".to_owned()),
                "local inbox projection does not accept a remote cursor",
            ));
        }
        if query.inbox_history_options.is_some() {
            return Err(crate::ImError::unsupported(
                "delegated-local-inbox-projection",
            ));
        }
        if query.limit.0 == 0 {
            return Err(crate::ImError::invalid_input(
                Some("limit".to_owned()),
                "limit must be greater than zero",
            ));
        }
        let binding = self.client.active_sync_account_binding().await?;
        if binding.owner_identity_id != self.client.current_identity().id.as_str()
            || binding.current_did != self.client.did().as_str()
        {
            return Err(crate::ImError::IdentityBindingConflict {
                detail: "active sync binding does not match the local Inbox owner".to_owned(),
            });
        }
        #[cfg(feature = "sqlite")]
        {
            let requested_limit = i64::from(query.limit.0);
            let mut records = self
                .client
                .core_inner()
                .local_state_db()
                .await?
                .list_local_inbox_messages(
                    binding.owner_identity_id,
                    binding.current_did,
                    query.scope,
                    query.unread_only,
                    requested_limit.saturating_add(1),
                )
                .await?;
            let has_more = records.len() > usize::try_from(requested_limit).unwrap_or(usize::MAX);
            if has_more {
                records.truncate(usize::try_from(requested_limit).unwrap_or(usize::MAX));
            }
            let items = records
                .iter()
                .map(crate::internal::message_runtime::conversations::message_from_record)
                .collect::<crate::ImResult<Vec<_>>>()?;
            return Ok(super::MessagePage {
                items,
                next_cursor: None,
                has_more,
                source: Some("local_projection".to_owned()),
                resolved_dids: Vec::new(),
                warnings: Vec::new(),
            });
        }
        #[cfg(not(feature = "sqlite"))]
        {
            let _ = binding;
            Err(crate::ImError::unsupported("local-inbox-projection"))
        }
    }

    /// Reconciles exact-device P5 Direct messages into the local projection.
    ///
    /// The operation is a complement to ordinary account Sync v2, not a
    /// fallback for it. When the host-local P5 gate is disabled it performs no
    /// network request. When enabled it requires an exact active vNext account
    /// and device binding and asks the Home Message Service for only
    /// `direct-e2ee` Inbox rows.
    #[doc(hidden)]
    pub async fn hydrate_exact_device_secure_inbox_async(
        &self,
        limit: crate::ids::PageLimit,
    ) -> crate::ImResult<Vec<String>> {
        if !self.client.core_inner().direct_e2ee_v2_enabled() {
            return Ok(Vec::new());
        }
        let binding = self.client.active_sync_account_binding().await?;
        if binding.owner_identity_id != self.client.current_identity().id.as_str()
            || binding.current_did != self.client.did().as_str()
        {
            return Err(crate::ImError::IdentityBindingConflict {
                detail: "active sync binding does not match the secure Inbox owner".to_owned(),
            });
        }
        #[cfg(feature = "sqlite")]
        {
            let mut transport = crate::internal::transport::CoreHttpTransport::new(self.client);
            let mut directory_transport =
                crate::internal::transport::CoreHttpTransport::new(self.client);
            return crate::internal::message_runtime::read::hydrate_exact_device_secure_inbox_async(
                self.client,
                &mut transport,
                &mut directory_transport,
                limit.0,
            )
            .await;
        }
        #[cfg(not(feature = "sqlite"))]
        {
            let _ = (binding, limit);
            Err(crate::ImError::unsupported(
                "exact-device-secure-inbox-hydration",
            ))
        }
    }

    pub fn history(
        &self,
        thread: super::ThreadRef,
        query: super::HistoryQuery,
    ) -> crate::ImResult<crate::ids::Page<super::Message>> {
        self.history_with_metadata(thread, query)
            .map(super::MessagePage::into_page)
    }

    pub async fn history_async(
        &self,
        thread: super::ThreadRef,
        query: super::HistoryQuery,
    ) -> crate::ImResult<crate::ids::Page<super::Message>> {
        self.history_with_metadata_async(thread, query)
            .await
            .map(super::MessagePage::into_page)
    }

    pub fn history_with_metadata(
        &self,
        thread: super::ThreadRef,
        query: super::HistoryQuery,
    ) -> crate::ImResult<super::MessagePage> {
        let resolved = resolve_history_thread(self.client, thread)?;
        crate::internal::message_runtime::read::MessageReadRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .history(crate::internal::message_runtime::read::HistoryRead {
            thread: resolved.thread,
            query,
            resolved_peer_did: resolved.resolved_did.clone(),
            peer_scope: resolved.peer_scope.clone(),
        })
        .map(|result| {
            let mut page = message_page_from_read_result(result)?;
            #[cfg(feature = "sqlite")]
            if let Some((handle, current_did)) = resolved.handle_peer.as_ref() {
                if let Ok(dids) =
                    crate::internal::message_runtime::local_projection::peer_dids_for_handle(
                        self.client,
                        handle,
                        current_did,
                    )
                {
                    page.resolved_dids = dids
                        .into_iter()
                        .filter_map(|did| crate::ids::Did::parse(did).ok())
                        .collect();
                }
            }
            if page.resolved_dids.is_empty() {
                if let Some(did) = resolved.resolved_did {
                    if let Ok(did) = crate::ids::Did::parse(did) {
                        page.resolved_dids.push(did);
                    }
                }
            }
            Ok(page)
        })?
    }

    pub async fn history_with_metadata_async(
        &self,
        thread: super::ThreadRef,
        query: super::HistoryQuery,
    ) -> crate::ImResult<super::MessagePage> {
        let resolved = resolve_history_thread_async(self.client, thread).await?;
        let mut page = crate::internal::message_runtime::read::MessageReadRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .history_async(crate::internal::message_runtime::read::HistoryRead {
            thread: resolved.thread,
            query,
            resolved_peer_did: resolved.resolved_did.clone(),
            peer_scope: resolved.peer_scope.clone(),
        })
        .await
        .map(message_page_from_read_result)??;
        #[cfg(feature = "sqlite")]
        if let Some((handle, current_did)) = resolved.handle_peer.as_ref() {
            if let Ok(dids) =
                crate::internal::message_runtime::local_projection::peer_dids_for_handle_async(
                    self.client,
                    handle,
                    current_did,
                )
                .await
            {
                page.resolved_dids = dids
                    .into_iter()
                    .filter_map(|did| crate::ids::Did::parse(did).ok())
                    .collect();
            }
        }
        if page.resolved_dids.is_empty() {
            if let Some(did) = resolved.resolved_did {
                if let Ok(did) = crate::ids::Did::parse(did) {
                    page.resolved_dids.push(did);
                }
            }
        }
        Ok(page)
    }

    pub fn local_history(
        &self,
        thread: super::ThreadRef,
        query: super::LocalHistoryQuery,
    ) -> crate::ImResult<crate::ids::Page<super::Message>> {
        self.local_history_with_metadata(thread, query)
            .map(super::MessagePage::into_page)
    }

    pub async fn local_history_async(
        &self,
        thread: super::ThreadRef,
        query: super::LocalHistoryQuery,
    ) -> crate::ImResult<crate::ids::Page<super::Message>> {
        self.local_history_with_metadata_async(thread, query)
            .await
            .map(super::MessagePage::into_page)
    }

    pub fn local_history_with_metadata(
        &self,
        thread: super::ThreadRef,
        query: super::LocalHistoryQuery,
    ) -> crate::ImResult<super::MessagePage> {
        crate::internal::message_runtime::read::MessageReadRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .local_history(crate::internal::message_runtime::read::LocalHistoryRead { thread, query })
        .map(message_page_from_read_result)?
    }

    pub async fn local_history_with_metadata_async(
        &self,
        thread: super::ThreadRef,
        query: super::LocalHistoryQuery,
    ) -> crate::ImResult<super::MessagePage> {
        crate::internal::message_runtime::read::MessageReadRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .local_history_async(crate::internal::message_runtime::read::LocalHistoryRead {
            thread,
            query,
        })
        .await
        .map(message_page_from_read_result)?
    }

    pub fn local_conversation_timeline(
        &self,
        conversation: super::ConversationReadRef,
        query: super::LocalHistoryQuery,
    ) -> crate::ImResult<crate::ids::Page<super::Message>> {
        self.local_conversation_timeline_with_metadata(conversation, query)
            .map(super::MessagePage::into_page)
    }

    pub async fn local_conversation_timeline_async(
        &self,
        conversation: super::ConversationReadRef,
        query: super::LocalHistoryQuery,
    ) -> crate::ImResult<crate::ids::Page<super::Message>> {
        self.local_conversation_timeline_with_metadata_async(conversation, query)
            .await
            .map(super::MessagePage::into_page)
    }

    pub fn local_conversation_timeline_with_metadata(
        &self,
        conversation: super::ConversationReadRef,
        query: super::LocalHistoryQuery,
    ) -> crate::ImResult<super::MessagePage> {
        self.local_history_with_metadata(conversation.as_thread_ref()?, query)
    }

    pub async fn local_conversation_timeline_with_metadata_async(
        &self,
        conversation: super::ConversationReadRef,
        query: super::LocalHistoryQuery,
    ) -> crate::ImResult<super::MessagePage> {
        self.local_history_with_metadata_async(conversation.as_thread_ref()?, query)
            .await
    }

    /// Reads one bounded, deterministic oldest-first page of hydrated incoming
    /// messages for this exact active vNext client. The owner/account scope is
    /// derived by Core; continuation uses an opaque, owner-bound local token.
    pub async fn local_hydrated_incoming_recovery_async(
        &self,
        query: super::IncomingMessageRecoveryQuery,
    ) -> crate::ImResult<super::IncomingMessageRecoveryPage> {
        if !(1..=LOCAL_INCOMING_RECOVERY_LIMIT_MAX).contains(&query.limit) {
            return Err(crate::ImError::invalid_input(
                Some("limit".to_owned()),
                format!("limit must be between 1 and {LOCAL_INCOMING_RECOVERY_LIMIT_MAX}"),
            ));
        }
        let binding = self.client.active_sync_account_binding().await?;
        if binding.owner_identity_id != self.client.current_identity().id.as_str()
            || binding.current_did != self.client.did().as_str()
        {
            return Err(crate::ImError::IdentityBindingConflict {
                detail: "active sync binding does not match the client owner".to_owned(),
            });
        }
        if query.page_token.as_ref().is_some_and(|token| {
            token.owner_identity_id != binding.owner_identity_id
                || token.account_id != binding.account_id
                || token.current_did != binding.current_did
                || token.protocol_device_id != binding.protocol_device_id
                || token.identity_generation != binding.identity_generation
                || token.device_auth_generation != binding.device_auth_generation
        }) {
            return Err(crate::ImError::IdentityBindingConflict {
                detail: "incoming recovery page token does not match the active binding".to_owned(),
            });
        }
        #[cfg(feature = "sqlite")]
        {
            let after = query.page_token.as_ref().map(|token| {
                crate::internal::local_state::messages::HydratedIncomingMessageCursor {
                    timestamp: token.timestamp.clone(),
                    server_sequence_key: token.server_sequence_key,
                    message_id: token.logical_message_id.clone(),
                }
            });
            let mut records = self
                .client
                .core_inner()
                .local_state_db()
                .await?
                .list_hydrated_incoming_messages(
                    binding.owner_identity_id.clone(),
                    binding.current_did.clone(),
                    i64::from(query.limit).saturating_add(1),
                    after,
                )
                .await?;
            let has_more = records.len() > usize::try_from(query.limit).unwrap_or(usize::MAX);
            if has_more {
                records.truncate(usize::try_from(query.limit).unwrap_or(usize::MAX));
            }
            let next_page_token = if has_more {
                records.last().map(|record| {
                    let cursor =
                        crate::internal::local_state::messages::hydrated_incoming_message_cursor(
                            record,
                        );
                    super::IncomingMessageRecoveryPageToken {
                        owner_identity_id: binding.owner_identity_id.clone(),
                        account_id: binding.account_id.clone(),
                        current_did: binding.current_did.clone(),
                        protocol_device_id: binding.protocol_device_id.clone(),
                        identity_generation: binding.identity_generation.clone(),
                        device_auth_generation: binding.device_auth_generation.clone(),
                        timestamp: cursor.timestamp,
                        server_sequence_key: cursor.server_sequence_key,
                        logical_message_id: cursor.message_id,
                    }
                })
            } else {
                None
            };
            let items = records
                .iter()
                .map(|record| {
                    let message =
                        crate::internal::message_runtime::conversations::message_from_record(
                            record,
                        )?;
                    if message.direction != super::MessageDirection::Incoming {
                        return Err(crate::ImError::IdentityBindingConflict {
                            detail: "local incoming recovery projection changed direction"
                                .to_owned(),
                        });
                    }
                    Ok(super::IncomingMessageRecoveryItem {
                        logical_message_id: message.id.as_str().to_owned(),
                        message,
                    })
                })
                .collect::<crate::ImResult<Vec<_>>>()?;
            Ok(super::IncomingMessageRecoveryPage {
                items,
                next_page_token,
                has_more,
            })
        }
        #[cfg(not(feature = "sqlite"))]
        {
            let _ = binding;
            Err(crate::ImError::unsupported(
                "local-hydrated-incoming-recovery",
            ))
        }
    }

    pub fn mark_read(
        &self,
        ids: Vec<crate::ids::MessageId>,
    ) -> crate::ImResult<super::MarkReadResult> {
        #[cfg(feature = "blocking")]
        {
            if self.client.realtime_requires_sync_changed_v2()? {
                validate_cached_mark_read_v2_binding(self.client)?;
                return mark_message_ids_read_v2(self.client, ids);
            }
            crate::internal::message_runtime::mark_read::MessageMarkReadRuntime::new(
                self.client,
                crate::internal::auth::session::FileSessionProvider::new(self.client),
                crate::internal::transport::CoreHttpTransport::new(self.client),
            )
            .mark_read(crate::internal::message_runtime::mark_read::MarkReadInput {
                message_ids: ids,
            })
            .map(|result| result.sdk_result)
        }
        #[cfg(not(feature = "blocking"))]
        {
            let _ = ids;
            Err(crate::ImError::unsupported("sync-message-mark-read"))
        }
    }

    pub async fn mark_read_async(
        &self,
        ids: Vec<crate::ids::MessageId>,
    ) -> crate::ImResult<super::MarkReadResult> {
        if self.client.realtime_requires_sync_changed_v2()? {
            self.client.active_sync_account_binding().await?;
            return mark_message_ids_read_v2_async(self.client, ids).await;
        }
        crate::internal::message_runtime::mark_read::MessageMarkReadRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .mark_read_async(crate::internal::message_runtime::mark_read::MarkReadInput {
            message_ids: ids,
        })
        .await
        .map(|result| result.sdk_result)
    }

    pub fn mark_thread_read(
        &self,
        request: super::MarkThreadReadRequest,
    ) -> crate::ImResult<super::MarkThreadReadResult> {
        #[cfg(feature = "blocking")]
        {
            crate::internal::message_runtime::mark_read::MessageMarkReadRuntime::new(
                self.client,
                crate::internal::auth::session::FileSessionProvider::new(self.client),
                crate::internal::transport::CoreHttpTransport::new(self.client),
            )
            .mark_thread_read(
                crate::internal::message_runtime::mark_read::MarkThreadReadInput {
                    request,
                    remote_thread: None,
                },
            )
            .map(|result| result.sdk_result)
        }
        #[cfg(not(feature = "blocking"))]
        {
            let _ = request;
            Err(crate::ImError::unsupported("sync-message-mark-thread-read"))
        }
    }

    pub async fn mark_thread_read_async(
        &self,
        request: super::MarkThreadReadRequest,
    ) -> crate::ImResult<super::MarkThreadReadResult> {
        crate::internal::message_runtime::mark_read::MessageMarkReadRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .mark_thread_read_async(
            crate::internal::message_runtime::mark_read::MarkThreadReadInput {
                request,
                remote_thread: None,
            },
        )
        .await
        .map(|result| result.sdk_result)
    }

    pub fn mark_conversation_read(
        &self,
        request: super::MarkConversationReadRequest,
    ) -> crate::ImResult<super::MarkThreadReadResult> {
        #[cfg(feature = "blocking")]
        {
            let mapped = mark_read_input_for_conversation(self.client, request)?;
            crate::internal::message_runtime::mark_read::MessageMarkReadRuntime::new(
                self.client,
                crate::internal::auth::session::FileSessionProvider::new(self.client),
                crate::internal::transport::CoreHttpTransport::new(self.client),
            )
            .mark_thread_read(mapped)
            .map(|result| result.sdk_result)
        }
        #[cfg(not(feature = "blocking"))]
        {
            let _ = request;
            Err(crate::ImError::unsupported("sync-message-mark-thread-read"))
        }
    }

    pub async fn mark_conversation_read_async(
        &self,
        request: super::MarkConversationReadRequest,
    ) -> crate::ImResult<super::MarkThreadReadResult> {
        let mapped = mark_read_input_for_conversation_async(self.client, request).await?;
        crate::internal::message_runtime::mark_read::MessageMarkReadRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .mark_thread_read_async(mapped)
        .await
        .map(|result| result.sdk_result)
    }

    pub fn sync_thread_after(
        &self,
        request: super::SyncThreadAfterRequest,
    ) -> crate::ImResult<super::SyncThreadAfterResult> {
        #[cfg(feature = "blocking")]
        {
            let resolved = resolve_history_thread(self.client, request.thread.clone())?;
            crate::internal::message_runtime::sync::MessageSyncRuntime::new(
                self.client,
                crate::internal::auth::session::FileSessionProvider::new(self.client),
                crate::internal::transport::CoreHttpTransport::new(self.client),
                crate::internal::transport::CoreHttpTransport::new(self.client),
            )
            .sync_thread_after(
                crate::internal::message_runtime::sync::SyncThreadAfterInput {
                    request: super::SyncThreadAfterRequest {
                        thread: resolved.thread,
                        ..request
                    },
                    resolved_peer_did: resolved.resolved_did,
                    peer_scope: resolved.peer_scope,
                },
            )
        }
        #[cfg(not(feature = "blocking"))]
        {
            let _ = request;
            Err(crate::ImError::unsupported("sync-thread-after"))
        }
    }

    pub async fn sync_thread_after_async(
        &self,
        request: super::SyncThreadAfterRequest,
    ) -> crate::ImResult<super::SyncThreadAfterResult> {
        let resolved = resolve_history_thread_async(self.client, request.thread.clone()).await?;
        crate::internal::message_runtime::sync::MessageSyncRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .sync_thread_after_async(
            crate::internal::message_runtime::sync::SyncThreadAfterInput {
                request: super::SyncThreadAfterRequest {
                    thread: resolved.thread,
                    ..request
                },
                resolved_peer_did: resolved.resolved_did,
                peer_scope: resolved.peer_scope,
            },
        )
        .await
    }

    pub fn sync_conversation_after(
        &self,
        request: super::SyncConversationAfterRequest,
    ) -> crate::ImResult<super::SyncThreadAfterResult> {
        #[cfg(feature = "blocking")]
        {
            let resolved = resolve_service_conversation_thread(self.client, &request.conversation)?;
            crate::internal::message_runtime::sync::MessageSyncRuntime::new(
                self.client,
                crate::internal::auth::session::FileSessionProvider::new(self.client),
                crate::internal::transport::CoreHttpTransport::new(self.client),
                crate::internal::transport::CoreHttpTransport::new(self.client),
            )
            .sync_thread_after(
                crate::internal::message_runtime::sync::SyncThreadAfterInput {
                    request: super::SyncThreadAfterRequest {
                        thread: resolved.thread,
                        after_server_seq: request.after_server_seq,
                        limit: request.limit,
                    },
                    resolved_peer_did: resolved.resolved_did,
                    peer_scope: resolved.peer_scope,
                },
            )
        }
        #[cfg(not(feature = "blocking"))]
        {
            let _ = request;
            Err(crate::ImError::unsupported("sync-conversation-after"))
        }
    }

    pub async fn sync_conversation_after_async(
        &self,
        request: super::SyncConversationAfterRequest,
    ) -> crate::ImResult<super::SyncThreadAfterResult> {
        let resolved =
            resolve_service_conversation_thread_async(self.client, &request.conversation).await?;
        crate::internal::message_runtime::sync::MessageSyncRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .sync_thread_after_async(
            crate::internal::message_runtime::sync::SyncThreadAfterInput {
                request: super::SyncThreadAfterRequest {
                    thread: resolved.thread,
                    after_server_seq: request.after_server_seq,
                    limit: request.limit,
                },
                resolved_peer_did: resolved.resolved_did,
                peer_scope: resolved.peer_scope,
            },
        )
        .await
    }

    pub fn sync_delta(
        &self,
        request: super::SyncDeltaRequest,
    ) -> crate::ImResult<super::SyncDeltaResult> {
        #[cfg(feature = "blocking")]
        {
            crate::internal::message_runtime::sync::MessageSyncRuntime::new(
                self.client,
                crate::internal::auth::session::FileSessionProvider::new(self.client),
                crate::internal::transport::CoreHttpTransport::new(self.client),
                crate::internal::transport::CoreHttpTransport::new(self.client),
            )
            .sync_delta(crate::internal::message_runtime::sync::SyncDeltaInput { request })
        }
        #[cfg(not(feature = "blocking"))]
        {
            let _ = request;
            Err(crate::ImError::unsupported("sync-delta"))
        }
    }

    pub async fn sync_delta_async(
        &self,
        request: super::SyncDeltaRequest,
    ) -> crate::ImResult<super::SyncDeltaResult> {
        crate::internal::message_runtime::sync::MessageSyncRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .sync_delta_async(crate::internal::message_runtime::sync::SyncDeltaInput { request })
        .await
    }

    pub fn sync_now(
        &self,
        request: super::MessageSyncRequest,
    ) -> crate::ImResult<super::MessageSyncOutcome> {
        #[cfg(feature = "blocking")]
        {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| crate::ImError::Internal {
                    message: format!("build message sync runtime: {error}"),
                })?;
            runtime.block_on(self.sync_now_async(request))
        }
        #[cfg(not(feature = "blocking"))]
        {
            let _ = request;
            Err(crate::ImError::unsupported("sync-now"))
        }
    }

    pub async fn sync_now_async(
        &self,
        request: super::MessageSyncRequest,
    ) -> crate::ImResult<super::MessageSyncOutcome> {
        let result = crate::internal::message_runtime::sync_v2::MessageSyncRuntimeV2::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .sync_now(request)
        .await;
        match result {
            Ok(outcome) => Ok(outcome),
            Err(error) => {
                if let Some(outcome) =
                    crate::internal::message_runtime::sync_v2::failure_outcome(&error)
                {
                    Ok(outcome)
                } else {
                    Err(error)
                }
            }
        }
    }

    pub fn sync_diagnostics(&self) -> crate::ImResult<super::MessageSyncDiagnostics> {
        #[cfg(feature = "blocking")]
        {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| crate::ImError::Internal {
                    message: format!("build message sync diagnostics runtime: {error}"),
                })?;
            runtime.block_on(self.sync_diagnostics_async())
        }
        #[cfg(not(feature = "blocking"))]
        {
            Err(crate::ImError::unsupported("sync-diagnostics"))
        }
    }

    pub async fn sync_diagnostics_async(&self) -> crate::ImResult<super::MessageSyncDiagnostics> {
        let owner_identity_id = self.client.current_identity().id.as_str().to_owned();
        let db = self.client.core_inner().local_state_db().await?;
        let state = db.load_sync_diagnostics(owner_identity_id).await?;
        Ok(sync_diagnostics_from_state(state))
    }

    pub fn conversations(
        &self,
        query: super::ConversationQuery,
    ) -> crate::ImResult<crate::ids::Page<super::Conversation>> {
        #[cfg(feature = "blocking")]
        {
            crate::internal::message_runtime::conversations::MessageConversationRuntime::new(
                self.client,
            )
            .conversations(query)
        }
        #[cfg(not(feature = "blocking"))]
        {
            let _ = query;
            Err(crate::ImError::unsupported("sync-message-conversations"))
        }
    }

    pub async fn conversations_async(
        &self,
        query: super::ConversationQuery,
    ) -> crate::ImResult<crate::ids::Page<super::Conversation>> {
        crate::internal::message_runtime::conversations::MessageConversationRuntime::new(
            self.client,
        )
        .conversations_async(query)
        .await
    }

    /// Durably records that a canonical conversation belongs in the recent
    /// conversation list, even before its first message exists.
    pub fn ensure_conversation(
        &self,
        conversation: super::ConversationReadRef,
    ) -> crate::ImResult<()> {
        ensure_conversation_registry(self.client, &conversation)
    }

    pub async fn ensure_conversation_async(
        &self,
        conversation: super::ConversationReadRef,
    ) -> crate::ImResult<()> {
        let owner = crate::internal::local_state::owner_scope::OwnerScope::for_client(self.client)?;
        self.client
            .core_inner()
            .local_state_db()
            .await?
            .ensure_conversation(
                owner.owner_identity_id,
                owner.owner_did,
                conversation.conversation_id,
            )
            .await
    }

    pub fn load_conversation_snapshot(
        &self,
    ) -> crate::ImResult<Option<super::ConversationListSnapshot>> {
        crate::internal::snapshot::conversation_snapshot::load_for_client(self.client)
    }

    pub async fn load_conversation_snapshot_async(
        &self,
    ) -> crate::ImResult<Option<super::ConversationListSnapshot>> {
        self.load_conversation_snapshot()
    }

    pub fn clear_conversation_snapshot(&self) -> crate::ImResult<()> {
        crate::internal::snapshot::conversation_snapshot::clear_for_client(self.client)
    }

    pub async fn clear_conversation_snapshot_async(&self) -> crate::ImResult<()> {
        self.clear_conversation_snapshot()
    }

    pub fn watch_conversation_patches(&self) -> crate::ImResult<super::ConversationPatchSession> {
        self.client
            .conversation_store()
            .watch_for_client(self.client)
    }

    pub async fn watch_conversation_patches_async(
        &self,
    ) -> crate::ImResult<super::ConversationPatchSession> {
        self.watch_conversation_patches()
    }

    pub fn repair_conversation_store(&self) -> crate::ImResult<super::ConversationStorePatch> {
        self.client
            .conversation_store()
            .repair_from_client(self.client)
    }

    pub async fn repair_conversation_store_async(
        &self,
    ) -> crate::ImResult<super::ConversationStorePatch> {
        self.repair_conversation_store()
    }

    pub fn watch_thread_patches(
        &self,
        thread: super::ThreadRef,
        limit: Option<u32>,
    ) -> crate::ImResult<super::ThreadMessagePatchSession> {
        self.client
            .message_store()
            .watch_for_client(self.client, thread, limit)
    }

    pub async fn watch_thread_patches_async(
        &self,
        thread: super::ThreadRef,
        limit: Option<u32>,
    ) -> crate::ImResult<super::ThreadMessagePatchSession> {
        self.watch_thread_patches(thread, limit)
    }

    pub fn watch_conversation_timeline_patches(
        &self,
        conversation: super::ConversationReadRef,
        limit: Option<u32>,
    ) -> crate::ImResult<super::ThreadMessagePatchSession> {
        self.watch_thread_patches(conversation.as_thread_ref()?, limit)
    }

    pub async fn watch_conversation_timeline_patches_async(
        &self,
        conversation: super::ConversationReadRef,
        limit: Option<u32>,
    ) -> crate::ImResult<super::ThreadMessagePatchSession> {
        self.watch_conversation_timeline_patches(conversation, limit)
    }

    pub fn repair_thread_store(
        &self,
        thread: super::ThreadRef,
        limit: Option<u32>,
    ) -> crate::ImResult<super::ThreadMessageStorePatch> {
        self.client.message_store().repair_thread_from_client(
            self.client,
            thread,
            limit.unwrap_or(100),
        )
    }

    pub async fn repair_thread_store_async(
        &self,
        thread: super::ThreadRef,
        limit: Option<u32>,
    ) -> crate::ImResult<super::ThreadMessageStorePatch> {
        self.repair_thread_store(thread, limit)
    }

    pub fn repair_conversation_timeline_store(
        &self,
        conversation: super::ConversationReadRef,
        limit: Option<u32>,
    ) -> crate::ImResult<super::ThreadMessageStorePatch> {
        self.repair_thread_store(conversation.as_thread_ref()?, limit)
    }

    pub async fn repair_conversation_timeline_store_async(
        &self,
        conversation: super::ConversationReadRef,
        limit: Option<u32>,
    ) -> crate::ImResult<super::ThreadMessageStorePatch> {
        self.repair_conversation_timeline_store(conversation, limit)
    }
}

#[cfg(all(feature = "sqlite", feature = "blocking"))]
fn validate_cached_mark_read_v2_binding(client: &crate::core::ImClient) -> crate::ImResult<()> {
    let context = client.sync_account_context()?;
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    let binding = crate::internal::local_state::sync_v2::load_identity_account_binding(
        &connection,
        client.current_identity().id.as_str(),
    )?
    .ok_or_else(|| crate::ImError::unsupported("active-sync-account-binding"))?;
    if binding.owner_identity_id != client.current_identity().id.as_str()
        || binding.account_id != context.account_id
        || binding.current_did != client.did().as_str()
        || binding.protocol_device_id != context.protocol_device_id
        || binding.device_auth_generation != context.device_auth_generation
    {
        return Err(crate::ImError::IdentityBindingConflict {
            detail: "cached Sync V2 binding does not match the active client".to_owned(),
        });
    }
    crate::internal::local_state::sync_v2::validate_positive_decimal(
        "identity_generation",
        &binding.identity_generation,
    )
}

#[cfg(all(not(feature = "sqlite"), feature = "blocking"))]
fn validate_cached_mark_read_v2_binding(_client: &crate::core::ImClient) -> crate::ImResult<()> {
    Err(crate::ImError::unsupported("active-sync-account-binding"))
}

#[cfg(all(feature = "sqlite", feature = "blocking"))]
fn mark_message_ids_read_v2(
    client: &crate::core::ImClient,
    ids: Vec<crate::ids::MessageId>,
) -> crate::ImResult<super::MarkReadResult> {
    if ids.is_empty() {
        return Err(crate::ImError::MessageNotFound {
            message_id: "message_ids".to_owned(),
        });
    }
    let requested_ids = ids
        .iter()
        .map(|id| id.as_str().to_owned())
        .collect::<Vec<_>>();
    let plans = resolve_mark_read_v2_watermarks(client, &requested_ids)?;
    let mut updated_count = 0_u32;
    let mut warnings = Vec::new();
    for plan in plans {
        let conversation = super::ConversationReadRef::new(plan.conversation_id)?;
        let remote_thread = resolve_service_conversation_thread(client, &conversation)?.thread;
        let result = crate::internal::message_runtime::mark_read::MessageMarkReadRuntime::new(
            client,
            crate::internal::auth::session::FileSessionProvider::new(client),
            crate::internal::transport::CoreHttpTransport::new(client),
        )
        .mark_thread_read(
            crate::internal::message_runtime::mark_read::MarkThreadReadInput {
                request: super::MarkThreadReadRequest {
                    thread: conversation.as_thread_ref()?,
                    watermark: Some(super::ReadWatermark {
                        last_read_message_id: Some(crate::ids::MessageId::parse(&plan.message_id)?),
                        last_read_thread_seq: Some(plan.server_seq),
                        read_at: Some(chrono::Utc::now()),
                    }),
                    fallback_max_message_ids: None,
                },
                remote_thread: Some(remote_thread),
            },
        )?
        .sdk_result;
        updated_count = updated_count.saturating_add(result.updated_count);
        warnings.extend(result.warnings);
    }
    Ok(super::MarkReadResult {
        updated_count,
        message_ids: ids,
        warnings,
    })
}

#[cfg(all(not(feature = "sqlite"), feature = "blocking"))]
fn mark_message_ids_read_v2(
    _client: &crate::core::ImClient,
    _ids: Vec<crate::ids::MessageId>,
) -> crate::ImResult<super::MarkReadResult> {
    Err(crate::ImError::unsupported("message-mark-read-sync-v2"))
}

#[cfg(feature = "sqlite")]
async fn mark_message_ids_read_v2_async(
    client: &crate::core::ImClient,
    ids: Vec<crate::ids::MessageId>,
) -> crate::ImResult<super::MarkReadResult> {
    if ids.is_empty() {
        return Err(crate::ImError::MessageNotFound {
            message_id: "message_ids".to_owned(),
        });
    }
    let requested_ids = ids
        .iter()
        .map(|id| id.as_str().to_owned())
        .collect::<Vec<_>>();
    let plans = resolve_mark_read_v2_watermarks(client, &requested_ids)?;
    let mut updated_count = 0_u32;
    let mut warnings = Vec::new();
    for plan in plans {
        let conversation = super::ConversationReadRef::new(plan.conversation_id)?;
        let remote_thread = resolve_service_conversation_thread_async(client, &conversation)
            .await?
            .thread;
        let result = crate::internal::message_runtime::mark_read::MessageMarkReadRuntime::new(
            client,
            crate::internal::auth::session::FileSessionProvider::new(client),
            crate::internal::transport::CoreHttpTransport::new(client),
        )
        .mark_thread_read_async(
            crate::internal::message_runtime::mark_read::MarkThreadReadInput {
                request: super::MarkThreadReadRequest {
                    thread: conversation.as_thread_ref()?,
                    watermark: Some(super::ReadWatermark {
                        last_read_message_id: Some(crate::ids::MessageId::parse(&plan.message_id)?),
                        last_read_thread_seq: Some(plan.server_seq),
                        read_at: Some(chrono::Utc::now()),
                    }),
                    fallback_max_message_ids: None,
                },
                remote_thread: Some(remote_thread),
            },
        )
        .await?
        .sdk_result;
        updated_count = updated_count.saturating_add(result.updated_count);
        warnings.extend(result.warnings);
    }
    Ok(super::MarkReadResult {
        updated_count,
        message_ids: ids,
        warnings,
    })
}

#[cfg(not(feature = "sqlite"))]
async fn mark_message_ids_read_v2_async(
    _client: &crate::core::ImClient,
    _ids: Vec<crate::ids::MessageId>,
) -> crate::ImResult<super::MarkReadResult> {
    Err(crate::ImError::unsupported("message-mark-read-sync-v2"))
}

#[cfg(feature = "sqlite")]
fn resolve_mark_read_v2_watermarks(
    client: &crate::core::ImClient,
    message_ids: &[String],
) -> crate::ImResult<Vec<crate::internal::local_state::messages::V2MarkReadWatermark>> {
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    crate::internal::local_state::messages::resolve_v2_mark_read_watermarks_for_owner_identity(
        &connection,
        client.current_identity().id.as_str(),
        message_ids,
    )
}

#[cfg(all(feature = "sqlite", feature = "blocking"))]
fn send_direct_e2ee_with_client(
    client: &crate::core::ImClient,
    resolved: ResolvedSendRequest,
) -> crate::ImResult<super::SendMessageResult> {
    send_direct_e2ee_with_client_attachment_aware(
        client,
        resolved,
        crate::internal::secure_direct::send::DirectSecureLocalPersistence::LegacySqlite,
    )
}

#[cfg(all(feature = "sqlite", feature = "blocking"))]
fn send_direct_e2ee_with_client_and_persistence(
    client: &crate::core::ImClient,
    resolved: ResolvedSendRequest,
    local_persistence: crate::internal::secure_direct::send::DirectSecureLocalPersistence,
) -> crate::ImResult<crate::internal::secure_direct::send::DirectSecureTextSendResult> {
    if matches!(resolved.request.body, super::MessageBody::Attachment { .. }) {
        return Err(crate::ImError::unsupported(
            "direct-e2ee-attachment-text-result",
        ));
    }
    let target_handle = resolved.direct_handle().map(str::to_owned);
    let peer_scope = resolved.peer_scope.clone();
    let mut result = crate::internal::secure_direct::send::DirectSecureTextSender::new(
        client,
        crate::internal::auth::session::FileSessionProvider::new(client),
        crate::internal::transport::CoreHttpTransport::new(client),
        crate::internal::transport::CoreHttpTransport::new(client),
    )
    .send(crate::internal::secure_direct::send::DirectSecureTextSend {
        request: resolved.request,
        resolved_target_did: resolved.target_did,
        target_handle: target_handle.clone(),
        peer_scope: peer_scope.clone(),
        local_persistence,
    })?;
    apply_peer_scope_to_direct_secure_text_result(
        &mut result,
        target_handle.as_deref(),
        peer_scope.as_ref(),
    )?;
    Ok(result)
}

#[cfg(all(feature = "sqlite", feature = "blocking"))]
fn send_direct_e2ee_with_client_attachment_aware(
    client: &crate::core::ImClient,
    resolved: ResolvedSendRequest,
    local_persistence: crate::internal::secure_direct::send::DirectSecureLocalPersistence,
) -> crate::ImResult<super::SendMessageResult> {
    if matches!(resolved.request.body, super::MessageBody::Attachment { .. }) {
        let committed = crate::internal::attachment_runtime::upload::AttachmentUploadRuntime::new(
            client,
            crate::internal::auth::session::FileSessionProvider::new(client),
            crate::internal::transport::CoreHttpTransport::new(client),
        )
        .prepare_and_commit_object(
            crate::internal::attachment_runtime::upload::AttachmentPrepareObjectInput {
                target: resolved.request.target.clone(),
                request: attachment_request_from_message_request(resolved.request.clone())?,
                resolved_target_did: resolved.target_did.clone(),
                message_security_profile: "direct-e2ee",
            },
        )?;
        return send_direct_e2ee_attachment_with_client_and_persistence(
            client,
            resolved,
            committed,
            local_persistence,
        )
        .map(|result| result.sdk_result);
    }
    send_direct_e2ee_with_client_and_persistence(client, resolved, local_persistence)
        .map(|result| result.sdk_result)
}

#[cfg(all(feature = "sqlite", feature = "blocking"))]
fn send_direct_e2ee_attachment_with_client_and_persistence(
    client: &crate::core::ImClient,
    resolved: ResolvedSendRequest,
    committed: crate::internal::attachment_runtime::upload::PreparedCommittedAttachment,
    local_persistence: crate::internal::secure_direct::send::DirectSecureLocalPersistence,
) -> crate::ImResult<crate::internal::secure_direct::send::DirectSecureAttachmentSendResult> {
    let target_handle = resolved.direct_handle().map(str::to_owned);
    let peer_scope = resolved.peer_scope.clone();
    let mut result = crate::internal::secure_direct::send::DirectSecureTextSender::new(
        client,
        crate::internal::auth::session::FileSessionProvider::new(client),
        crate::internal::transport::CoreHttpTransport::new(client),
        crate::internal::transport::CoreHttpTransport::new(client),
    )
    .send_attachment(
        crate::internal::secure_direct::send::DirectSecureAttachmentSend {
            request: resolved.request,
            resolved_target_did: resolved.target_did,
            target_handle: target_handle.clone(),
            peer_scope: peer_scope.clone(),
            committed,
            local_persistence,
        },
    )?;
    apply_peer_scope_to_direct_secure_attachment_result(
        &mut result,
        target_handle.as_deref(),
        peer_scope.as_ref(),
    )?;
    Ok(result)
}

#[cfg(feature = "sqlite")]
async fn persist_deferred_direct_e2ee_effect(
    client: &crate::core::ImClient,
    mut result: crate::internal::secure_direct::send::DirectSecureTextSendResult,
    target_handle: Option<&str>,
    peer_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
) -> crate::ImResult<super::SendMessageResult> {
    apply_peer_scope_to_direct_secure_text_result(&mut result, target_handle, peer_scope)?;
    match result.local_effect {
        crate::internal::secure_direct::send::DirectSecureLocalEffect::None => {}
        crate::internal::secure_direct::send::DirectSecureLocalEffect::PersistOutgoing => {
            match
                crate::internal::message_runtime::local_projection::persist_direct_e2ee_outgoing_async(
                    client,
                    &result.target_did,
                    result.target_handle.as_deref(),
                    result.peer_scope.as_ref(),
                    &result.text,
                    &result.kind,
                    &result.sdk_result,
                )
                .await
            {
                Ok(()) => client.emit_committed_local_message_projection("local_send"),
                Err(err) => {
                    result
                        .sdk_result
                        .warnings
                        .push(format!("Failed to persist local secure direct message: {err}"));
                }
            }
        }
        crate::internal::secure_direct::send::DirectSecureLocalEffect::QueueOutbox(record) => {
            client
                .core_inner()
                .local_state_db()
                .await?
                .queue_e2ee_outbox(record)
                .await?;
        }
    }
    Ok(result.sdk_result)
}

#[cfg(feature = "sqlite")]
async fn persist_deferred_direct_e2ee_attachment_effect(
    client: &crate::core::ImClient,
    mut result: crate::internal::secure_direct::send::DirectSecureAttachmentSendResult,
    target_handle: Option<&str>,
    peer_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
) -> crate::ImResult<super::SendMessageResult> {
    apply_peer_scope_to_direct_secure_attachment_result(&mut result, target_handle, peer_scope)?;
    match result.local_effect {
        crate::internal::secure_direct::send::DirectSecureAttachmentLocalEffect::None => {}
        crate::internal::secure_direct::send::DirectSecureAttachmentLocalEffect::PersistOutgoing => {
            match
                crate::internal::message_runtime::local_projection::persist_direct_e2ee_attachment_outgoing_async(
                    client,
                    &result.target_did,
                    result.target_handle.as_deref(),
                    result.peer_scope.as_ref(),
                    &result.redacted_manifest,
                    &result.sdk_result,
                )
                .await
            {
                Ok(()) => client.emit_committed_local_message_projection("local_send"),
                Err(err) => {
                    result
                        .sdk_result
                        .warnings
                        .push(format!("Failed to persist local secure direct attachment: {err}"));
                }
            }
        }
    }
    Ok(result.sdk_result)
}

fn apply_peer_scope_to_direct_secure_text_result(
    result: &mut crate::internal::secure_direct::send::DirectSecureTextSendResult,
    target_handle: Option<&str>,
    peer_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
) -> crate::ImResult<()> {
    let handle = target_handle
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if result.target_handle.is_none() {
        result.target_handle = handle.map(str::to_owned);
    }
    if result.peer_scope.is_none() {
        result.peer_scope = peer_scope.cloned();
    }
    normalize_direct_send_result_for_peer_scope(
        &mut result.sdk_result,
        result.peer_scope.as_ref(),
        result.target_handle.as_deref(),
        Some(result.target_did.as_str()),
    )
}

fn apply_peer_scope_to_direct_secure_attachment_result(
    result: &mut crate::internal::secure_direct::send::DirectSecureAttachmentSendResult,
    target_handle: Option<&str>,
    peer_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
) -> crate::ImResult<()> {
    let handle = target_handle
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if result.target_handle.is_none() {
        result.target_handle = handle.map(str::to_owned);
    }
    if result.peer_scope.is_none() {
        result.peer_scope = peer_scope.cloned();
    }
    normalize_direct_send_result_for_peer_scope(
        &mut result.sdk_result,
        result.peer_scope.as_ref(),
        result.target_handle.as_deref(),
        Some(result.target_did.as_str()),
    )
}

#[derive(Clone)]
pub(super) struct ResolvedSendRequest {
    pub(super) request: super::SendMessageRequest,
    pub(super) target_did: Option<String>,
    pub(super) peer_scope: Option<crate::internal::local_state::owner_scope::DirectPeerScope>,
}

#[derive(Clone)]
struct ResolvedConversationSendRequest {
    conversation_id: String,
    request: super::SendMessageRequest,
    target_did: Option<String>,
    target_handle: Option<String>,
    peer_scope: Option<crate::internal::local_state::owner_scope::DirectPeerScope>,
}

impl ResolvedSendRequest {
    pub(super) fn direct_handle(&self) -> Option<&str> {
        self.peer_scope
            .as_ref()
            .map(|scope| scope.full_handle.as_str())
            .or_else(|| direct_handle_from_target(&self.request.target))
    }
}

pub(crate) fn normalize_direct_send_result_for_peer_scope(
    result: &mut super::SendMessageResult,
    peer_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
    target_handle: Option<&str>,
    target_did: Option<&str>,
) -> crate::ImResult<()> {
    let Some(scope) = peer_scope else {
        return Ok(());
    };
    let thread_id = crate::messages::direct_peer_scope_thread_id(
        scope.user_id.as_str(),
        scope.full_handle.as_str(),
    )?;
    result.message.thread = super::ThreadRef::Thread(thread_id);
    result.message.metadata.conversation_identity = Some(
        crate::messages::ConversationIdentity::from_thread_ref(&result.message.thread),
    );
    upsert_message_attribute(
        &mut result.message.metadata.attributes,
        "peer_user_id",
        scope.user_id.as_str(),
    );
    upsert_message_attribute(
        &mut result.message.metadata.attributes,
        "peer_full_handle",
        scope.full_handle.as_str(),
    );
    if let Some(handle) = target_handle
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        upsert_message_attribute(
            &mut result.message.metadata.attributes,
            "target_handle",
            handle,
        );
    }
    if let Some(did) = target_did.map(str::trim).filter(|value| !value.is_empty()) {
        upsert_message_attribute(
            &mut result.message.metadata.attributes,
            "resolved_target_did",
            did,
        );
        upsert_message_attribute(
            &mut result.message.metadata.attributes,
            "peer_current_did",
            did,
        );
    }
    Ok(())
}

fn conversation_send_uses_security_runtime(security: &super::MessageSecurityMode) -> bool {
    matches!(
        security,
        super::MessageSecurityMode::E2eeRequired
            | super::MessageSecurityMode::SecureDirect
            | super::MessageSecurityMode::GroupE2ee
    )
}

fn conversation_send_request(
    client: &crate::core::ImClient,
    conversation: super::ConversationReadRef,
    body: super::MessageBody,
    security: super::MessageSecurityMode,
    client_message_id: Option<crate::ids::MessageId>,
    idempotency_key: Option<String>,
    wait_for_final_acceptance: bool,
    delegated_signing: Option<super::DelegatedSigningOptions>,
) -> crate::ImResult<ResolvedConversationSendRequest> {
    ensure_conversation_registry(client, &conversation)?;
    let conversation_id = conversation.conversation_id.clone();
    let resolved = resolve_service_conversation_thread(client, &conversation)?;
    let client_message_id = match client_message_id {
        Some(id) => id,
        None => crate::ids::MessageId::parse(format!(
            "msg-{}",
            crate::internal::wire::common::generate_operation_id()
        ))?,
    };
    let idempotency_key = idempotency_key
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .or_else(|| Some(format!("op-{}", client_message_id.as_str())));
    let resolved_target = conversation_send_target(resolved)?;
    let request = super::SendMessageRequest {
        target: resolved_target.target,
        body,
        security,
        client_message_id: Some(client_message_id),
        delivery: super::MessageDeliveryOptions {
            idempotency_key,
            wait_for_final_acceptance,
        },
        delegated_signing,
    };
    Ok(ResolvedConversationSendRequest {
        conversation_id,
        request,
        target_did: resolved_target.target_did,
        target_handle: resolved_target.target_handle,
        peer_scope: resolved_target.peer_scope,
    })
}

fn conversation_send_target(
    resolved: ResolvedConversationServiceThread,
) -> crate::ImResult<ResolvedConversationSendTarget> {
    match resolved.thread {
        super::ThreadRef::Direct(peer) => {
            let target_did = resolved.resolved_did.clone().or_else(|| {
                peer.as_str()
                    .starts_with("did:")
                    .then(|| peer.as_str().to_owned())
            });
            let target_handle = resolved
                .peer_scope
                .as_ref()
                .map(|scope| scope.full_handle.clone());
            let target_peer = target_handle
                .as_deref()
                .map(|handle| crate::ids::PeerRef::parse(handle, ""))
                .transpose()?
                .unwrap_or(peer);
            Ok(ResolvedConversationSendTarget {
                target: super::MessageTarget::Direct(target_peer),
                target_did,
                target_handle,
                peer_scope: resolved.peer_scope,
            })
        }
        super::ThreadRef::Group(group) => Ok(ResolvedConversationSendTarget {
            target: super::MessageTarget::Group(group),
            target_did: None,
            target_handle: None,
            peer_scope: None,
        }),
        super::ThreadRef::Thread(_) => Err(crate::ImError::invalid_input(
            Some("conversation_id".to_owned()),
            "send_conversation requires a canonical dm: or group: conversation_id",
        )),
    }
}

pub(crate) fn resolve_conversation_send_target(
    client: &crate::core::ImClient,
    conversation: &super::ConversationReadRef,
) -> crate::ImResult<ResolvedConversationSendTarget> {
    ensure_conversation_registry(client, conversation)?;
    conversation_send_target(resolve_service_conversation_thread(client, conversation)?)
}

fn validate_plain_conversation_send(request: &super::SendMessageRequest) -> crate::ImResult<()> {
    validate_body(&request.body)?;
    if matches!(request.body, super::MessageBody::Attachment { .. }) {
        return Err(crate::ImError::unsupported("conversation-attachment-send"));
    }
    match request.security {
        super::MessageSecurityMode::DefaultPlain | super::MessageSecurityMode::Plain => {}
        super::MessageSecurityMode::E2eeRequired
        | super::MessageSecurityMode::SecureDirect
        | super::MessageSecurityMode::GroupE2ee => {
            return Err(crate::ImError::unsupported(
                "secure-conversation-send-local-echo",
            ));
        }
    }
    validate_send_mode(&request.target, &request.security)?;
    validate_delegated_send_scope(request)
}

fn normalize_resolved_direct_send_result(
    result: &mut super::SendMessageResult,
    resolved: &ResolvedSendRequest,
) -> crate::ImResult<()> {
    normalize_direct_send_result_for_peer_scope(
        result,
        resolved.peer_scope.as_ref(),
        resolved.direct_handle(),
        resolved.target_did.as_deref(),
    )
}

fn upsert_message_attribute(
    attributes: &mut Vec<super::MessageMetadataAttribute>,
    key: &str,
    value: &str,
) {
    let value = value.trim();
    if value.is_empty() {
        return;
    }
    if let Some(attribute) = attributes.iter_mut().find(|attribute| attribute.key == key) {
        attribute.value = value.to_owned();
        return;
    }
    attributes.push(super::MessageMetadataAttribute {
        key: key.to_owned(),
        value: value.to_owned(),
    });
}

fn resolve_send_request(
    client: &crate::core::ImClient,
    request: super::SendMessageRequest,
) -> crate::ImResult<ResolvedSendRequest> {
    let (target_did, peer_scope) = match &request.target {
        super::MessageTarget::Direct(peer) => {
            let resolved = resolve_direct_peer(client, peer)?;
            (resolved.target_did, resolved.peer_scope)
        }
        super::MessageTarget::Group(_) => (None, None),
    };
    Ok(ResolvedSendRequest {
        request,
        target_did,
        peer_scope,
    })
}

async fn resolve_send_request_async(
    client: &crate::core::ImClient,
    request: super::SendMessageRequest,
) -> crate::ImResult<ResolvedSendRequest> {
    let (target_did, peer_scope) = match &request.target {
        super::MessageTarget::Direct(peer) => {
            let resolved = resolve_direct_peer_async(client, peer).await?;
            (resolved.target_did, resolved.peer_scope)
        }
        super::MessageTarget::Group(_) => (None, None),
    };
    Ok(ResolvedSendRequest {
        request,
        target_did,
        peer_scope,
    })
}

struct ResolvedDirectPeer {
    target_did: Option<String>,
    peer_scope: Option<crate::internal::local_state::owner_scope::DirectPeerScope>,
}

fn resolve_direct_peer(
    client: &crate::core::ImClient,
    peer: &crate::ids::PeerRef,
) -> crate::ImResult<ResolvedDirectPeer> {
    let raw = peer.as_str().trim();
    if raw.is_empty() || raw.starts_with("did:") {
        return Ok(ResolvedDirectPeer {
            target_did: None,
            peer_scope: None,
        });
    }
    let lookup = crate::internal::handle_discovery::resolve_direct_handle(client, raw)?;
    resolved_direct_peer_from_handle(lookup)
}

async fn resolve_direct_peer_async(
    client: &crate::core::ImClient,
    peer: &crate::ids::PeerRef,
) -> crate::ImResult<ResolvedDirectPeer> {
    let raw = peer.as_str().trim();
    if raw.is_empty() || raw.starts_with("did:") {
        return Ok(ResolvedDirectPeer {
            target_did: None,
            peer_scope: None,
        });
    }
    let lookup =
        crate::internal::handle_discovery::resolve_direct_handle_async(client, raw).await?;
    resolved_direct_peer_from_handle(lookup)
}

fn resolved_direct_peer_from_handle(
    lookup: crate::internal::handle_discovery::DirectHandleResolution,
) -> crate::ImResult<ResolvedDirectPeer> {
    let peer_scope = lookup.peer_scope()?;
    Ok(ResolvedDirectPeer {
        target_did: Some(lookup.target_did),
        peer_scope: Some(peer_scope),
    })
}

fn validate_send_mode(
    target: &super::MessageTarget,
    security: &super::MessageSecurityMode,
) -> crate::ImResult<()> {
    match (target, security) {
        (_, super::MessageSecurityMode::DefaultPlain | super::MessageSecurityMode::Plain) => Ok(()),
        (
            super::MessageTarget::Direct(_),
            super::MessageSecurityMode::E2eeRequired | super::MessageSecurityMode::SecureDirect,
        ) => Ok(()),
        (super::MessageTarget::Group(_), super::MessageSecurityMode::SecureDirect) => {
            Err(crate::ImError::unsupported("secure-direct"))
        }
        (
            super::MessageTarget::Group(_),
            super::MessageSecurityMode::E2eeRequired | super::MessageSecurityMode::GroupE2ee,
        ) => validate_group_e2ee_security(),
        (super::MessageTarget::Direct(_), super::MessageSecurityMode::GroupE2ee) => {
            Err(crate::ImError::unsupported("group-e2ee"))
        }
    }
}

fn validate_body(body: &super::MessageBody) -> crate::ImResult<()> {
    match body {
        super::MessageBody::Text { text, .. } if text.trim().is_empty() => {
            Err(crate::ImError::invalid_input(
                Some("text".to_string()),
                "text message must not be empty",
            ))
        }
        super::MessageBody::Text { .. } => Ok(()),
        super::MessageBody::Payload { payload } if !payload.is_object() => {
            Err(crate::ImError::invalid_input(
                Some("payload".to_string()),
                "message payload must be a JSON object",
            ))
        }
        super::MessageBody::Payload { .. } => Ok(()),
        super::MessageBody::Attachment { .. } => Ok(()),
    }
}

fn validate_attachment_security(
    body: &super::MessageBody,
    security: &super::MessageSecurityMode,
) -> crate::ImResult<()> {
    if !matches!(body, super::MessageBody::Attachment { .. }) {
        return Ok(());
    }
    match security {
        super::MessageSecurityMode::DefaultPlain
        | super::MessageSecurityMode::Plain
        | super::MessageSecurityMode::E2eeRequired
        | super::MessageSecurityMode::SecureDirect
        | super::MessageSecurityMode::GroupE2ee => Ok(()),
    }
}

fn validate_delegated_send_scope(request: &super::SendMessageRequest) -> crate::ImResult<()> {
    if request.delegated_signing.is_none() {
        return Ok(());
    }
    if !matches!(request.target, super::MessageTarget::Direct(_)) {
        return Err(crate::ImError::unsupported("delegated-group-send"));
    }
    if matches!(request.body, super::MessageBody::Attachment { .. }) {
        return Err(crate::ImError::unsupported("delegated-attachment-send"));
    }
    match request.security {
        super::MessageSecurityMode::DefaultPlain | super::MessageSecurityMode::Plain => Ok(()),
        super::MessageSecurityMode::E2eeRequired | super::MessageSecurityMode::SecureDirect => {
            Err(crate::ImError::unsupported("delegated-e2ee-send"))
        }
        super::MessageSecurityMode::GroupE2ee => {
            Err(crate::ImError::unsupported("delegated-group-e2ee-send"))
        }
    }
}

fn validate_secure_attachment_request(
    body: &super::MessageBody,
    security: &super::MessageSecurityMode,
) -> crate::ImResult<()> {
    if !matches!(body, super::MessageBody::Attachment { .. }) {
        return Err(crate::ImError::unsupported("secure-attachment"));
    }
    match security {
        super::MessageSecurityMode::E2eeRequired
        | super::MessageSecurityMode::SecureDirect
        | super::MessageSecurityMode::GroupE2ee => Ok(()),
        super::MessageSecurityMode::DefaultPlain | super::MessageSecurityMode::Plain => {
            Err(crate::ImError::unsupported("secure-attachment"))
        }
    }
}

pub(super) fn attachment_request_from_message_request(
    request: super::SendMessageRequest,
) -> crate::ImResult<crate::attachments::AttachmentSendRequest> {
    match request.body {
        super::MessageBody::Attachment {
            input,
            caption,
            mention_payload,
            mime_type,
            filename,
        } => Ok(crate::attachments::AttachmentSendRequest {
            input,
            caption,
            mention_payload,
            mime_type,
            filename,
            delivery: request.delivery,
            security: crate::messages::MessageSecurityMode::DefaultPlain,
        }),
        _ => Err(crate::ImError::unsupported("attachments")),
    }
}

#[cfg(feature = "group-e2ee")]
fn validate_group_e2ee_security() -> crate::ImResult<()> {
    Ok(())
}

#[cfg(not(feature = "group-e2ee"))]
fn validate_group_e2ee_security() -> crate::ImResult<()> {
    Err(crate::ImError::unsupported("group-e2ee"))
}

fn message_kind_from_result(
    result: &super::SendMessageResult,
) -> crate::ImResult<super::MessageKind> {
    match &result.message.body {
        super::MessageBodyView::Text { kind, .. } => Ok(kind.clone()),
        super::MessageBodyView::Payload { .. } => Err(crate::ImError::unsupported(
            "message-projection-body:application/json",
        )),
        super::MessageBodyView::Unsupported { content_type } => {
            Err(crate::ImError::unsupported(format!(
                "message-projection-body:{}",
                content_type.as_deref().unwrap_or("unknown")
            )))
        }
    }
}

fn direct_handle_from_target(target: &super::MessageTarget) -> Option<&str> {
    match target {
        super::MessageTarget::Direct(peer) if !peer.as_str().starts_with("did:") => {
            Some(peer.as_str())
        }
        _ => None,
    }
}

struct StaleDeliveryTarget {
    current_did: Option<String>,
    full_handle: Option<String>,
}

fn stale_delivery_target_from_error(
    err: &crate::ImError,
    owner_did: &str,
) -> Option<StaleDeliveryTarget> {
    let crate::ImError::Service {
        code: Some(code),
        data: Some(data),
        ..
    } = err
    else {
        return None;
    };
    if code != "1406" {
        return None;
    }
    if data.get("reason").and_then(serde_json::Value::as_str) != Some("stale_did") {
        return None;
    }
    let current_did = data
        .get("current_did")
        .and_then(serde_json::Value::as_str)
        .and_then(|did| canonical_peer_did_value(did, owner_did));
    let full_handle = data
        .get("full_handle")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    (current_did.is_some() || full_handle.is_some()).then_some(StaleDeliveryTarget {
        current_did,
        full_handle,
    })
}

struct ResolvedHistoryThread {
    thread: super::ThreadRef,
    resolved_did: Option<String>,
    handle_peer: Option<(String, String)>,
    peer_scope: Option<crate::internal::local_state::owner_scope::DirectPeerScope>,
}

#[derive(Debug)]
struct ResolvedConversationServiceThread {
    thread: super::ThreadRef,
    resolved_did: Option<String>,
    peer_scope: Option<crate::internal::local_state::owner_scope::DirectPeerScope>,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedConversationSendTarget {
    pub(crate) target: super::MessageTarget,
    pub(crate) target_did: Option<String>,
    pub(crate) target_handle: Option<String>,
    pub(crate) peer_scope: Option<crate::internal::local_state::owner_scope::DirectPeerScope>,
}

fn resolve_history_thread(
    client: &crate::core::ImClient,
    thread: super::ThreadRef,
) -> crate::ImResult<ResolvedHistoryThread> {
    let super::ThreadRef::Direct(peer) = thread else {
        return Ok(ResolvedHistoryThread {
            thread,
            resolved_did: None,
            handle_peer: None,
            peer_scope: None,
        });
    };
    if peer.as_str().starts_with("did:") {
        return Ok(ResolvedHistoryThread {
            thread: super::ThreadRef::Direct(peer.clone()),
            resolved_did: Some(peer.as_str().to_owned()),
            handle_peer: None,
            peer_scope: None,
        });
    }
    let lookup = crate::internal::handle_discovery::resolve_direct_handle(client, peer.as_str())?;
    let did = lookup.target_did;
    let full_handle = lookup.full_handle;
    let peer_scope = crate::internal::local_state::owner_scope::DirectPeerScope::new(
        lookup.authority_subject_id,
        full_handle.clone(),
    )?;
    Ok(ResolvedHistoryThread {
        thread: super::ThreadRef::Direct(crate::ids::PeerRef::parse(&did, "")?),
        resolved_did: Some(did.clone()),
        handle_peer: Some((full_handle, did)),
        peer_scope: Some(peer_scope),
    })
}

#[cfg(feature = "sqlite")]
fn ensure_conversation_registry(
    client: &crate::core::ImClient,
    conversation: &super::ConversationReadRef,
) -> crate::ImResult<()> {
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    let owner = crate::internal::local_state::owner_scope::OwnerScope::for_client(client)?;
    crate::internal::local_state::conversation_registry::ensure_validated(
        &connection,
        &owner.owner_identity_id,
        &owner.owner_did,
        &conversation.conversation_id,
    )
}

#[cfg(not(feature = "sqlite"))]
fn ensure_conversation_registry(
    _client: &crate::core::ImClient,
    _conversation: &super::ConversationReadRef,
) -> crate::ImResult<()> {
    Err(crate::ImError::unsupported("ensure-conversation"))
}

fn resolve_service_conversation_thread(
    client: &crate::core::ImClient,
    conversation: &super::ConversationReadRef,
) -> crate::ImResult<ResolvedConversationServiceThread> {
    let conversation_id = conversation.conversation_id.trim();
    if let Some(group) = conversation_id.strip_prefix("group:") {
        let group = crate::ids::GroupRef::parse(group)?;
        return Ok(ResolvedConversationServiceThread {
            thread: super::ThreadRef::Group(group),
            resolved_did: None,
            peer_scope: None,
        });
    }
    if let Some(peer) = conversation_id.strip_prefix("dm:") {
        if peer.starts_with("peer-scope:") {
            return resolve_peer_scope_service_conversation_thread(client, conversation);
        }
        if !is_single_did_ref(peer) {
            return Err(unresolvable_service_conversation(
                &conversation.conversation_id,
            ));
        }
        let peer = crate::ids::PeerRef::parse(peer, "")?;
        let resolved_did = peer
            .as_str()
            .starts_with("did:")
            .then(|| peer.as_str().to_owned());
        return Ok(ResolvedConversationServiceThread {
            thread: super::ThreadRef::Direct(peer),
            resolved_did,
            peer_scope: None,
        });
    }
    Err(crate::ImError::invalid_input(
        Some("conversation_id".to_owned()),
        "conversation service operations require a canonical dm: or group: conversation_id",
    ))
}

fn mark_read_input_for_conversation(
    client: &crate::core::ImClient,
    request: super::MarkConversationReadRequest,
) -> crate::ImResult<crate::internal::message_runtime::mark_read::MarkThreadReadInput> {
    let remote_thread = resolve_service_conversation_thread(client, &request.conversation)?.thread;
    Ok(
        crate::internal::message_runtime::mark_read::MarkThreadReadInput {
            request: mark_thread_read_request_for_conversation(request)?,
            remote_thread: Some(remote_thread),
        },
    )
}

fn mark_thread_read_request_for_conversation(
    request: super::MarkConversationReadRequest,
) -> crate::ImResult<super::MarkThreadReadRequest> {
    Ok(super::MarkThreadReadRequest {
        thread: request.conversation.as_thread_ref()?,
        watermark: request.watermark,
        fallback_max_message_ids: request.fallback_max_message_ids,
    })
}

async fn resolve_service_conversation_thread_async(
    client: &crate::core::ImClient,
    conversation: &super::ConversationReadRef,
) -> crate::ImResult<ResolvedConversationServiceThread> {
    resolve_service_conversation_thread(client, conversation)
}

async fn mark_read_input_for_conversation_async(
    client: &crate::core::ImClient,
    request: super::MarkConversationReadRequest,
) -> crate::ImResult<crate::internal::message_runtime::mark_read::MarkThreadReadInput> {
    let remote_thread = resolve_service_conversation_thread_async(client, &request.conversation)
        .await?
        .thread;
    Ok(
        crate::internal::message_runtime::mark_read::MarkThreadReadInput {
            request: mark_thread_read_request_for_conversation(request)?,
            remote_thread: Some(remote_thread),
        },
    )
}

#[cfg(feature = "sqlite")]
fn resolve_peer_scope_service_conversation_thread(
    client: &crate::core::ImClient,
    conversation: &super::ConversationReadRef,
) -> crate::ImResult<ResolvedConversationServiceThread> {
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    let owner = crate::internal::local_state::owner_scope::OwnerScope::for_client(client)?;
    if let Some(route) = crate::internal::local_state::direct_peer_routes::get(
        &connection,
        &owner.owner_identity_id,
        &conversation.conversation_id,
    )? {
        let peer_did = canonical_peer_did(&route.current_did, client.did().as_str())
            .ok_or_else(|| unresolvable_service_conversation(&conversation.conversation_id))?;
        return Ok(ResolvedConversationServiceThread {
            thread: super::ThreadRef::Direct(crate::ids::PeerRef::parse(peer_did, "")?),
            resolved_did: Some(peer_did.to_owned()),
            peer_scope: Some(route.peer_scope()),
        });
    }
    let records = conversation_projection_records(client, conversation)?;
    let Some(peer_did) = peer_current_did_from_records(&records, client.did().as_str()) else {
        return Err(unresolvable_service_conversation(
            &conversation.conversation_id,
        ));
    };
    Ok(ResolvedConversationServiceThread {
        thread: super::ThreadRef::Direct(crate::ids::PeerRef::parse(&peer_did, "")?),
        resolved_did: Some(peer_did),
        peer_scope: records.iter().find_map(peer_scope_from_record),
    })
}

#[cfg(not(feature = "sqlite"))]
fn resolve_peer_scope_service_conversation_thread(
    _client: &crate::core::ImClient,
    conversation: &super::ConversationReadRef,
) -> crate::ImResult<ResolvedConversationServiceThread> {
    Err(unresolvable_service_conversation(
        &conversation.conversation_id,
    ))
}

#[cfg(feature = "sqlite")]
fn conversation_projection_records(
    client: &crate::core::ImClient,
    conversation: &super::ConversationReadRef,
) -> crate::ImResult<Vec<crate::internal::local_state::messages::MessageRecord>> {
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    let page =
        crate::internal::local_state::messages::list_messages_for_thread_ref_for_owner_identity(
            &connection,
            client.current_identity().id.as_str(),
            client.did().as_str(),
            &conversation.as_thread_ref()?,
            20,
            None,
        )?;
    Ok(page.records)
}

#[cfg(feature = "sqlite")]
fn peer_current_did_from_records(
    records: &[crate::internal::local_state::messages::MessageRecord],
    owner_did: &str,
) -> Option<String> {
    records
        .iter()
        .find_map(|record| incoming_participant_peer_did_from_record(record, owner_did))
        .or_else(|| {
            records
                .iter()
                .find_map(|record| metadata_peer_current_did_from_record(record, owner_did))
        })
        .or_else(|| {
            records
                .iter()
                .find_map(|record| participant_peer_did_from_record(record, owner_did))
        })
}

#[cfg(feature = "sqlite")]
fn metadata_peer_current_did_from_record(
    record: &crate::internal::local_state::messages::MessageRecord,
    owner_did: &str,
) -> Option<String> {
    let metadata = parse_message_metadata(&record.metadata);
    for candidate in [
        metadata_value(&metadata, "peer_current_did"),
        metadata_value(&metadata, "resolved_target_did"),
    ] {
        if let Some(candidate) = canonical_peer_did(candidate?, owner_did) {
            return Some(candidate.to_owned());
        }
    }
    None
}

#[cfg(feature = "sqlite")]
fn incoming_participant_peer_did_from_record(
    record: &crate::internal::local_state::messages::MessageRecord,
    owner_did: &str,
) -> Option<String> {
    if record.direction != 0 {
        return None;
    }
    participant_peer_did_from_record(record, owner_did)
}

#[cfg(feature = "sqlite")]
fn participant_peer_did_from_record(
    record: &crate::internal::local_state::messages::MessageRecord,
    owner_did: &str,
) -> Option<String> {
    for candidate in [record.sender_did.as_str(), record.receiver_did.as_str()] {
        if let Some(candidate) = canonical_peer_did(candidate, owner_did) {
            return Some(candidate.to_owned());
        }
    }
    None
}

fn canonical_peer_did<'a>(candidate: &'a str, owner_did: &str) -> Option<&'a str> {
    let candidate = candidate.trim();
    (is_single_did_ref(candidate) && candidate != owner_did).then_some(candidate)
}

fn canonical_peer_did_value(candidate: &str, owner_did: &str) -> Option<String> {
    canonical_peer_did(candidate, owner_did).map(ToOwned::to_owned)
}

fn is_single_did_ref(value: &str) -> bool {
    let value = value.trim();
    value.starts_with("did:") && !value["did:".len()..].contains(":did:")
}

#[cfg(feature = "sqlite")]
fn peer_scope_from_record(
    record: &crate::internal::local_state::messages::MessageRecord,
) -> Option<crate::internal::local_state::owner_scope::DirectPeerScope> {
    let metadata = parse_message_metadata(&record.metadata);
    crate::internal::local_state::owner_scope::DirectPeerScope::new(
        metadata_value(&metadata, "peer_user_id")?,
        metadata_value(&metadata, "peer_full_handle")?,
    )
    .ok()
}

#[cfg(feature = "sqlite")]
fn parse_message_metadata(metadata: &str) -> serde_json::Map<String, serde_json::Value> {
    serde_json::from_str::<serde_json::Value>(metadata)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default()
}

#[cfg(feature = "sqlite")]
fn metadata_value<'a>(
    metadata: &'a serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Option<&'a str> {
    metadata
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn unresolvable_service_conversation(conversation_id: &str) -> crate::ImError {
    crate::ImError::invalid_input(
        Some("conversation_id".to_owned()),
        format!(
            "conversation_id {conversation_id} cannot be resolved to a service direct or group thread"
        ),
    )
}

async fn resolve_history_thread_async(
    client: &crate::core::ImClient,
    thread: super::ThreadRef,
) -> crate::ImResult<ResolvedHistoryThread> {
    let super::ThreadRef::Direct(peer) = thread else {
        return Ok(ResolvedHistoryThread {
            thread,
            resolved_did: None,
            handle_peer: None,
            peer_scope: None,
        });
    };
    if peer.as_str().starts_with("did:") {
        return Ok(ResolvedHistoryThread {
            thread: super::ThreadRef::Direct(peer.clone()),
            resolved_did: Some(peer.as_str().to_owned()),
            handle_peer: None,
            peer_scope: None,
        });
    }
    let lookup =
        crate::internal::handle_discovery::resolve_direct_handle_async(client, peer.as_str())
            .await?;
    let did = lookup.target_did;
    let full_handle = lookup.full_handle;
    let peer_scope = crate::internal::local_state::owner_scope::DirectPeerScope::new(
        lookup.authority_subject_id,
        full_handle.clone(),
    )?;
    Ok(ResolvedHistoryThread {
        thread: super::ThreadRef::Direct(crate::ids::PeerRef::parse(&did, "")?),
        resolved_did: Some(did.clone()),
        handle_peer: Some((full_handle, did)),
        peer_scope: Some(peer_scope),
    })
}

fn message_page_from_read_result(
    result: crate::internal::message_runtime::read::ReadPageResult,
) -> crate::ImResult<super::MessagePage> {
    let source = result
        .raw
        .get("source")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let resolved_dids = result
        .raw
        .get("resolved_dids")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str())
        .filter_map(|did| crate::ids::Did::parse(did).ok())
        .collect();
    let warnings = result
        .raw
        .get("warnings")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str())
        .map(ToOwned::to_owned)
        .collect();
    Ok(super::MessagePage {
        items: result.page.items,
        next_cursor: result.page.next_cursor,
        has_more: result.page.has_more,
        source,
        resolved_dids,
        warnings,
    })
}

#[cfg(all(test, feature = "group-e2ee"))]
mod group_e2ee_public_send_tests {
    use std::fs;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::{Duration, Instant};

    use anp::group_e2ee::operations::{CreateGroupInput, FinalizeCommitInput};
    use serde_json::{json, Value};

    use crate::internal::group_e2ee::provider::GroupMlsProvider;

    #[cfg(feature = "blocking")]
    #[test]
    fn public_group_e2ee_send_uses_native_provider_and_sends_cipher_only() {
        let fixture = Fixture::new();
        let server = RpcTestServer::spawn(vec![
            json!({
                "group_state_ref": {
                    "group_did": "did:wba:awiki.test:groups:group-e2ee-preflight",
                    "group_state_version": "preflight-state"
                }
            }),
            json!({
                "group_state_ref": {
                    "group_did": fixture.group_did,
                    "group_state_version": "service-state-1"
                }
            }),
            json!({
                "accepted": true,
                "final_acceptance": true,
                "group_did": fixture.group_did,
                "message_id": "server-message-id",
                "operation_id": "op-public-group-e2ee",
                "group_event_seq": "91",
                "group_state_version": "service-state-2",
                "accepted_at": "2026-05-21T00:00:00Z"
            }),
        ]);
        let core =
            crate::core::ImCore::new(fixture.config(server.base_url()), fixture.paths()).unwrap();
        let client = core
            .client(crate::identity::IdentitySelector::LocalAlias(
                "alice".to_owned(),
            ))
            .unwrap();
        prepare_local_mls_group(&client, &fixture.group_did);

        let result = client
            .messages()
            .send(crate::messages::SendMessageRequest {
                target: crate::messages::MessageTarget::Group(
                    crate::ids::GroupRef::parse(&fixture.group_did).unwrap(),
                ),
                body: crate::messages::MessageBody::Text {
                    text: "public group secret".to_owned(),
                    kind: crate::messages::MessageKind::Text,
                },
                security: crate::messages::MessageSecurityPolicy::E2eeRequired,
                client_message_id: Some(
                    crate::ids::MessageId::parse("msg-public-group-e2ee").unwrap(),
                ),
                delivery: crate::messages::MessageDeliveryOptions {
                    idempotency_key: Some("op-public-group-e2ee".to_owned()),
                    wait_for_final_acceptance: true,
                },
                delegated_signing: None,
            })
            .unwrap();

        assert_eq!(result.message.metadata.server_sequence, Some(91));
        assert!(matches!(
            result.delivery,
            crate::messages::DeliveryState::Accepted
        ));
        let requests = server.requests();
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0].rpc_method, "group.e2ee.head");
        assert_eq!(
            requests[0].params["body"]["group_did"],
            "did:wba:awiki.test:groups:group-e2ee-preflight"
        );
        assert_eq!(requests[1].rpc_method, "group.e2ee.head");
        assert_eq!(requests[1].params["body"]["group_did"], fixture.group_did);
        assert_eq!(requests[2].rpc_method, "group.e2ee.send");
        assert_eq!(
            requests[2].params["meta"]["security_profile"],
            anp::group_e2ee::SECURITY_PROFILE
        );
        assert_eq!(
            requests[2].params["meta"]["content_type"],
            anp::group_e2ee::GROUP_CIPHER_CONTENT_TYPE
        );
        assert_eq!(
            requests[2].params["body"]["group_state_ref"]["group_state_version"],
            "service-state-1"
        );
        assert!(
            requests[2].params["body"]["private_message_b64u"]
                .as_str()
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false),
            "group.e2ee.send must carry an MLS private message"
        );
        let encoded_send = serde_json::to_string(&requests[2].params).unwrap();
        assert!(!encoded_send.contains("public group secret"));
        assert!(!encoded_send.contains("application_plaintext"));
        assert!(!encoded_send.contains("provider"));
        assert!(!encoded_send.contains("StorageProvider"));
        assert!(!encoded_send.contains("mls_state.sqlite"));
        assert!(!encoded_send.contains("openmls_group_id_b64u"));
    }

    #[cfg(not(feature = "blocking"))]
    #[test]
    fn public_group_e2ee_sync_send_fails_closed_by_default() {
        let fixture = Fixture::new();
        let core = crate::core::ImCore::new(
            fixture.config("https://example.test".to_owned()),
            fixture.paths(),
        )
        .unwrap();
        let client = core
            .client(crate::identity::IdentitySelector::LocalAlias(
                "alice".to_owned(),
            ))
            .unwrap();

        let result = client.messages().send(crate::messages::SendMessageRequest {
            target: crate::messages::MessageTarget::Group(
                crate::ids::GroupRef::parse(&fixture.group_did).unwrap(),
            ),
            body: crate::messages::MessageBody::Text {
                text: "public group secret".to_owned(),
                kind: crate::messages::MessageKind::Text,
            },
            security: crate::messages::MessageSecurityPolicy::E2eeRequired,
            client_message_id: Some(
                crate::ids::MessageId::parse("msg-public-group-e2ee-sync").unwrap(),
            ),
            delivery: crate::messages::MessageDeliveryOptions::default(),
            delegated_signing: None,
        });

        assert!(matches!(
            result,
            Err(crate::ImError::UnsupportedCapability { capability }) if capability == "sync-group-e2ee-send"
        ));
    }

    #[tokio::test]
    async fn public_group_e2ee_send_async_uses_async_transport_and_db_actor_projection() {
        let fixture = Fixture::new();
        let server = RpcTestServer::spawn(vec![
            json!({
                "group_state_ref": {
                    "group_did": fixture.group_did,
                    "group_state_version": "service-state-async-1"
                }
            }),
            json!({
                "accepted": true,
                "final_acceptance": true,
                "group_did": fixture.group_did,
                "message_id": "server-message-async-id",
                "operation_id": "op-public-group-e2ee-async",
                "group_event_seq": "92",
                "group_state_version": "service-state-async-2",
                "accepted_at": "2026-05-21T00:00:00Z"
            }),
        ]);
        let core = crate::core::ImCore::open(fixture.config(server.base_url()), fixture.paths())
            .await
            .unwrap();
        let client = core
            .client_async(crate::identity::IdentitySelector::LocalAlias(
                "alice".to_owned(),
            ))
            .await
            .unwrap();
        prepare_local_mls_group(&client, &fixture.group_did);

        let result = client
            .messages()
            .send_async(crate::messages::SendMessageRequest {
                target: crate::messages::MessageTarget::Group(
                    crate::ids::GroupRef::parse(&fixture.group_did).unwrap(),
                ),
                body: crate::messages::MessageBody::Text {
                    text: "async group secret".to_owned(),
                    kind: crate::messages::MessageKind::Text,
                },
                security: crate::messages::MessageSecurityPolicy::E2eeRequired,
                client_message_id: Some(
                    crate::ids::MessageId::parse("msg-public-group-e2ee-async").unwrap(),
                ),
                delivery: crate::messages::MessageDeliveryOptions {
                    idempotency_key: Some("op-public-group-e2ee-async".to_owned()),
                    wait_for_final_acceptance: true,
                },
                delegated_signing: None,
            })
            .await
            .unwrap();

        assert_eq!(result.message.metadata.server_sequence, Some(92));
        let requests = server.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].rpc_method, "group.e2ee.head");
        assert_eq!(requests[0].params["body"]["group_did"], fixture.group_did);
        assert_eq!(requests[1].rpc_method, "group.e2ee.send");
        assert_eq!(
            requests[1].params["meta"]["content_type"],
            anp::group_e2ee::GROUP_CIPHER_CONTENT_TYPE
        );
        let encoded_send = serde_json::to_string(&requests[1].params).unwrap();
        assert!(!encoded_send.contains("async group secret"));

        let stored = stored_group_message(&fixture, &client, result.message.id.as_str());
        assert_eq!(stored.thread_id, format!("group:{}", fixture.group_did));
        assert_eq!(stored.group_did, fixture.group_did);
        assert_eq!(stored.content, "async group secret");
        assert!(stored.is_e2ee);
        assert_eq!(stored.server_seq, Some(92));
        let metadata: Value = serde_json::from_str(&stored.metadata).unwrap();
        assert_eq!(metadata["security"], "group-e2ee");
    }

    #[test]
    fn public_group_e2ee_mode_still_rejects_direct_targets() {
        let fixture = Fixture::new();
        let core = crate::core::ImCore::new(
            fixture.config("https://example.test".to_owned()),
            fixture.paths(),
        )
        .unwrap();
        let client = core
            .client(crate::identity::IdentitySelector::LocalAlias(
                "alice".to_owned(),
            ))
            .unwrap();

        let result = client.messages().send(crate::messages::SendMessageRequest {
            target: crate::messages::MessageTarget::Direct(
                crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
            ),
            body: crate::messages::MessageBody::Text {
                text: "not group".to_owned(),
                kind: crate::messages::MessageKind::Text,
            },
            security: crate::messages::MessageSecurityMode::GroupE2ee,
            client_message_id: None,
            delivery: crate::messages::MessageDeliveryOptions::default(),
            delegated_signing: None,
        });

        assert!(matches!(
            result,
            Err(crate::ImError::UnsupportedCapability { capability }) if capability == "group-e2ee"
        ));
    }

    fn prepare_local_mls_group(client: &crate::core::ImClient, group_did: &str) {
        let provider = crate::internal::group_e2ee::storage::native_provider_for_client(client)
            .expect("native provider");
        let prepared = provider
            .create_group_prepare(CreateGroupInput {
                creator_did: client.did().as_str().to_owned(),
                device_id: crate::internal::group_e2ee::DEFAULT_GROUP_MLS_DEVICE_ID.to_owned(),
                group_did: group_did.to_owned(),
                operation_id: "op-public-group-e2ee-create".to_owned(),
                request_id: "req-public-group-e2ee-create".to_owned(),
                pending_commit_id: Some("pc-public-group-e2ee-create".to_owned()),
            })
            .expect("create group");
        provider
            .finalize_commit(FinalizeCommitInput {
                pending_commit_id: prepared.pending_commit_id,
                request_id: "req-public-group-e2ee-finalize".to_owned(),
            })
            .expect("finalize group");
    }

    struct Fixture {
        root: PathBuf,
        group_did: String,
    }

    impl Fixture {
        fn new() -> Self {
            let root = unique_temp_root();
            write_identity_fixture(&root, "alice", "did:example:alice");
            Self {
                root,
                group_did: "did:example:groups:public-e2ee".to_owned(),
            }
        }

        fn config(&self, base_url: String) -> crate::ImCoreConfig {
            crate::ImCoreConfig {
                service_base_url: crate::ServiceEndpoint::parse(base_url).unwrap(),
                did_domain: "awiki.test".to_owned(),
                user_service_endpoint: None,
                message_service_endpoint: None,
                mail_service_endpoint: None,
                anp_service_endpoint: None,
                anp_service_did: None,
                ca_bundle: None,
                transport_policy: crate::MessageTransportPolicy::HttpOnly,
            }
        }

        fn paths(&self) -> crate::ImCorePaths {
            crate::ImCorePaths {
                identities: crate::IdentityRegistryPaths {
                    identity_root_dir: self.root.join("identities"),
                    registry_path: self.root.join("identities").join("registry.json"),
                    default_identity_path: Some(self.root.join("identities").join("default")),
                },
                local_state: crate::LocalStatePaths {
                    sqlite_path: self.root.join("local").join("im.sqlite"),
                },
                runtime: crate::RuntimePaths {
                    cache_dir: self.root.join("cache"),
                    temp_dir: self.root.join("tmp"),
                },
            }
        }
    }

    fn write_identity_fixture(root: &Path, alias: &str, did: &str) {
        let identity_root = root.join("identities");
        let identity_dir = identity_root.join(alias);
        fs::create_dir_all(&identity_dir).unwrap();
        fs::write(identity_root.join("default"), format!("{alias}\n")).unwrap();
        fs::write(
            identity_root.join("registry.json"),
            json!({
                "default_identity": alias,
                "identities": [{
                    "id": "alice-id",
                    "did": did,
                    "local_alias": alias,
                    "device_id": crate::internal::group_e2ee::DEFAULT_GROUP_MLS_DEVICE_ID,
                    "ready_for_auth": true,
                    "ready_for_messaging": true,
                    "missing": []
                }]
            })
            .to_string(),
        )
        .unwrap();
        let bundle = anp::authentication::create_did_wba_document(
            "awiki.test",
            anp::authentication::DidDocumentOptions {
                path_segments: vec!["user".to_owned()],
                domain: Some("awiki.test".to_owned()),
                challenge: Some("group-e2ee-public-send-test".to_owned()),
                ..anp::authentication::DidDocumentOptions::default()
            },
        )
        .unwrap();
        fs::write(
            identity_dir.join("did.json"),
            serde_json::to_vec_pretty(&bundle.did_document).unwrap(),
        )
        .unwrap();
        fs::write(
            identity_dir.join("private.key"),
            bundle.private_key_pem("key-1").unwrap(),
        )
        .unwrap();
        fs::write(
            identity_dir.join("auth.json"),
            r#"{"jwt_token":"test-token"}"#,
        )
        .unwrap();
    }

    #[derive(Debug)]
    struct StoredGroupMessage {
        thread_id: String,
        group_did: String,
        content: String,
        is_e2ee: bool,
        server_seq: Option<i64>,
        metadata: String,
    }

    fn stored_group_message(
        fixture: &Fixture,
        client: &crate::core::ImClient,
        message_id: &str,
    ) -> StoredGroupMessage {
        let connection =
            rusqlite::Connection::open(fixture.root.join("local").join("im.sqlite")).unwrap();
        connection
            .query_row(
                r#"
SELECT thread_id, group_did, content, is_e2ee, server_seq, metadata
FROM messages
WHERE owner_identity_id = ?1 AND owner_did = ?2 AND msg_id = ?3
"#,
                rusqlite::params![
                    client.current_identity().id.as_str(),
                    client.did().as_str(),
                    message_id
                ],
                |row| {
                    Ok(StoredGroupMessage {
                        thread_id: row.get(0)?,
                        group_did: row.get(1)?,
                        content: row.get(2)?,
                        is_e2ee: row.get::<_, i64>(3)? != 0,
                        server_seq: row.get(4)?,
                        metadata: row.get(5)?,
                    })
                },
            )
            .unwrap()
    }

    #[derive(Debug, Clone)]
    struct CapturedRpc {
        rpc_method: String,
        params: Value,
    }

    struct RpcTestServer {
        address: String,
        requests: Arc<Mutex<Vec<CapturedRpc>>>,
        join: Option<thread::JoinHandle<()>>,
    }

    impl RpcTestServer {
        fn spawn(responses: Vec<Value>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let address = format!("http://{}", listener.local_addr().unwrap());
            let requests = Arc::new(Mutex::new(Vec::new()));
            let server_requests = Arc::clone(&requests);
            let join = thread::spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(10);
                for response in responses {
                    let mut stream = accept_before_deadline(&listener, deadline);
                    let request = read_rpc_request(&mut stream);
                    let id = request.id.clone();
                    server_requests.lock().unwrap().push(CapturedRpc {
                        rpc_method: request.rpc_method,
                        params: request.params,
                    });
                    write_rpc_response(&mut stream, id, response);
                }
            });
            Self {
                address,
                requests,
                join: Some(join),
            }
        }

        fn base_url(&self) -> String {
            self.address.clone()
        }

        fn requests(&self) -> Vec<CapturedRpc> {
            self.requests.lock().unwrap().clone()
        }
    }

    impl Drop for RpcTestServer {
        fn drop(&mut self) {
            if let Some(join) = self.join.take() {
                let _ = join.join();
            }
        }
    }

    #[derive(Debug)]
    struct RawRpcRequest {
        rpc_method: String,
        params: Value,
        id: Value,
    }

    fn accept_before_deadline(listener: &TcpListener, deadline: Instant) -> TcpStream {
        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    stream.set_nonblocking(false).unwrap();
                    return stream;
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "timed out waiting for RPC");
                    thread::sleep(Duration::from_millis(10));
                }
                Err(err) => panic!("accept RPC request: {err}"),
            }
        }
    }

    fn read_rpc_request(stream: &mut TcpStream) -> RawRpcRequest {
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut raw = Vec::new();
        let mut buffer = [0_u8; 4096];
        let header_end = loop {
            let count = stream.read(&mut buffer).unwrap();
            assert!(count > 0, "RPC request closed before headers");
            raw.extend_from_slice(&buffer[..count]);
            if let Some(index) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
                break index;
            }
        };
        let headers_text = std::str::from_utf8(&raw[..header_end]).unwrap();
        let headers = headers_text
            .lines()
            .skip(1)
            .filter_map(|line| {
                let (key, value) = line.split_once(':')?;
                Some((key.trim().to_ascii_lowercase(), value.trim().to_owned()))
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        let content_length = headers
            .get("content-length")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or_default();
        let body_start = header_end + 4;
        while raw.len() < body_start + content_length {
            let count = stream.read(&mut buffer).unwrap();
            assert!(count > 0, "RPC request closed before body");
            raw.extend_from_slice(&buffer[..count]);
        }
        let payload: Value = serde_json::from_slice(&raw[body_start..body_start + content_length])
            .expect("json rpc request body");
        RawRpcRequest {
            rpc_method: payload["method"].as_str().unwrap().to_owned(),
            params: payload["params"].clone(),
            id: payload["id"].clone(),
        }
    }

    fn write_rpc_response(stream: &mut TcpStream, id: Value, result: Value) {
        let body = json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        })
        .to_string();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
    }

    fn unique_temp_root() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "im-core-group-e2ee-public-send-{}-{nanos}",
            std::process::id()
        ))
    }
}
