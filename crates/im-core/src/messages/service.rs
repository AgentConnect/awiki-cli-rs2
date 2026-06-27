pub struct MessageService<'a> {
    client: &'a crate::core::ImClient,
}

#[cfg(all(test, feature = "sqlite"))]
mod direct_e2ee_async_persistence_tests {
    use serde_json::json;

    use crate::internal::secure_direct::send::{
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
                text: "actor persisted secret".to_owned(),
                kind: crate::messages::MessageKind::Text,
                raw: Some(json!({ "accepted": true })),
                local_effect: DirectSecureLocalEffect::PersistOutgoing,
            },
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
                        request: resolved.request,
                        resolved_target_did: resolved.target_did,
                        credentials: None,
                    },
                )?;
                #[cfg(feature = "sqlite")]
                if let Err(err) =
                    crate::internal::message_runtime::local_projection::persist_direct_outgoing_result(
                        self.client,
                        &result.target_did,
                        direct_handle.as_deref(),
                        peer_scope.as_ref(),
                        &result.sdk_result,
                    )
                {
                    result
                        .sdk_result
                        .warnings
                        .push(format!("Failed to persist local message: {err}"));
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
                if let Err(err) =
                    crate::internal::message_runtime::local_projection::persist_group_outgoing_result(
                        self.client,
                        &result.group_did,
                        &result.sdk_result,
                    )
                {
                    result
                        .sdk_result
                        .warnings
                        .push(format!("Failed to persist local group message: {err}"));
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
                    request: resolved.request,
                    resolved_target_did: resolved.target_did,
                    credentials: None,
                })
                .await?;
                #[cfg(feature = "sqlite")]
                if let Err(err) =
                    crate::internal::message_runtime::local_projection::persist_direct_outgoing_result_async(
                        self.client,
                        &result.target_did,
                        direct_handle.as_deref(),
                        peer_scope.as_ref(),
                        &result.sdk_result,
                    )
                    .await
                {
                    result
                        .sdk_result
                        .warnings
                        .push(format!("Failed to persist local message: {err}"));
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
                if let Err(err) =
                    crate::internal::message_runtime::local_projection::persist_group_outgoing_result_async(
                        self.client,
                        &result.group_did,
                        &result.sdk_result,
                    )
                    .await
                {
                    result
                        .sdk_result
                        .warnings
                        .push(format!("Failed to persist local group message: {err}"));
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
        let attachment = attachment_request_from_message_request(request)?;
        self.client
            .attachments()
            .send(target, attachment)
            .map(|result| result.message)
    }

    async fn send_plain_attachment_async(
        &self,
        request: super::SendMessageRequest,
    ) -> crate::ImResult<super::SendMessageResult> {
        let target = request.target.clone();
        let attachment = attachment_request_from_message_request(request)?;
        self.client
            .attachments()
            .send_async(target, attachment)
            .await
            .map(|result| result.message)
    }

    fn send_direct_e2ee(
        &self,
        resolved: ResolvedSendRequest,
    ) -> crate::ImResult<super::SendMessageResult> {
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
                        committed: committed.clone(),
                        local_persistence:
                            crate::internal::secure_direct::send::DirectSecureLocalPersistence::Deferred,
                    },
                )
                .await?;
            if let Some(result) = async_result {
                return persist_deferred_direct_e2ee_attachment_effect(self.client, result).await;
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
                return persist_deferred_direct_e2ee_attachment_effect(self.client, result).await;
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
            ) => return persist_deferred_direct_e2ee_effect(self.client, result).await,
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
            persist_deferred_direct_e2ee_effect(self.client, result).await
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

    pub fn mark_read(
        &self,
        ids: Vec<crate::ids::MessageId>,
    ) -> crate::ImResult<super::MarkReadResult> {
        #[cfg(feature = "blocking")]
        {
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
                crate::internal::message_runtime::mark_read::MarkThreadReadInput { request },
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
            crate::internal::message_runtime::mark_read::MarkThreadReadInput { request },
        )
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
    crate::internal::secure_direct::send::DirectSecureTextSender::new(
        client,
        crate::internal::auth::session::FileSessionProvider::new(client),
        crate::internal::transport::CoreHttpTransport::new(client),
        crate::internal::transport::CoreHttpTransport::new(client),
    )
    .send(crate::internal::secure_direct::send::DirectSecureTextSend {
        request: resolved.request,
        resolved_target_did: resolved.target_did,
        local_persistence,
    })
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
    crate::internal::secure_direct::send::DirectSecureTextSender::new(
        client,
        crate::internal::auth::session::FileSessionProvider::new(client),
        crate::internal::transport::CoreHttpTransport::new(client),
        crate::internal::transport::CoreHttpTransport::new(client),
    )
    .send_attachment(
        crate::internal::secure_direct::send::DirectSecureAttachmentSend {
            request: resolved.request,
            resolved_target_did: resolved.target_did,
            committed,
            local_persistence,
        },
    )
}

#[cfg(feature = "sqlite")]
async fn persist_deferred_direct_e2ee_effect(
    client: &crate::core::ImClient,
    mut result: crate::internal::secure_direct::send::DirectSecureTextSendResult,
) -> crate::ImResult<super::SendMessageResult> {
    match result.local_effect {
        crate::internal::secure_direct::send::DirectSecureLocalEffect::None => {}
        crate::internal::secure_direct::send::DirectSecureLocalEffect::PersistOutgoing => {
            if let Err(err) =
                crate::internal::message_runtime::local_projection::persist_direct_e2ee_outgoing_async(
                    client,
                    &result.target_did,
                    &result.text,
                    &result.kind,
                    &result.sdk_result,
                )
                .await
            {
                result
                    .sdk_result
                    .warnings
                    .push(format!("Failed to persist local secure direct message: {err}"));
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
) -> crate::ImResult<super::SendMessageResult> {
    match result.local_effect {
        crate::internal::secure_direct::send::DirectSecureAttachmentLocalEffect::None => {}
        crate::internal::secure_direct::send::DirectSecureAttachmentLocalEffect::PersistOutgoing => {
            if let Err(err) =
                crate::internal::message_runtime::local_projection::persist_direct_e2ee_attachment_outgoing_async(
                    client,
                    &result.target_did,
                    &result.redacted_manifest,
                    &result.sdk_result,
                )
                .await
            {
                result
                    .sdk_result
                    .warnings
                    .push(format!("Failed to persist local secure direct attachment: {err}"));
            }
        }
    }
    Ok(result.sdk_result)
}

#[derive(Clone)]
struct ResolvedSendRequest {
    request: super::SendMessageRequest,
    target_did: Option<String>,
    peer_scope: Option<crate::internal::local_state::owner_scope::DirectPeerScope>,
}

impl ResolvedSendRequest {
    fn direct_handle(&self) -> Option<&str> {
        self.peer_scope
            .as_ref()
            .map(|scope| scope.full_handle.as_str())
            .or_else(|| direct_handle_from_target(&self.request.target))
    }
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
    let handle = crate::ids::Handle::parse(raw, "")?;
    let lookup = client.directory().lookup_handle(handle)?;
    Ok(ResolvedDirectPeer {
        target_did: Some(lookup.did.as_str().to_owned()),
        peer_scope: Some(
            crate::internal::local_state::owner_scope::DirectPeerScope::new(
                lookup.user_id,
                lookup.handle.as_str().to_owned(),
            )?,
        ),
    })
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
    let handle = crate::ids::Handle::parse(raw, "")?;
    let lookup = client.directory().lookup_handle_async(handle).await?;
    Ok(ResolvedDirectPeer {
        target_did: Some(lookup.did.as_str().to_owned()),
        peer_scope: Some(
            crate::internal::local_state::owner_scope::DirectPeerScope::new(
                lookup.user_id,
                lookup.handle.as_str().to_owned(),
            )?,
        ),
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

fn attachment_request_from_message_request(
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

struct ResolvedHistoryThread {
    thread: super::ThreadRef,
    resolved_did: Option<String>,
    handle_peer: Option<(String, String)>,
    peer_scope: Option<crate::internal::local_state::owner_scope::DirectPeerScope>,
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
    let handle = crate::ids::Handle::parse(peer.as_str(), "")?;
    let lookup = client.directory().lookup_handle(handle)?;
    let did = lookup.did.as_str().to_owned();
    let full_handle = lookup.handle.as_str().to_owned();
    let peer_scope = crate::internal::local_state::owner_scope::DirectPeerScope::new(
        lookup.user_id,
        full_handle.clone(),
    )?;
    Ok(ResolvedHistoryThread {
        thread: super::ThreadRef::Direct(crate::ids::PeerRef::parse(&did, "")?),
        resolved_did: Some(did.clone()),
        handle_peer: Some((full_handle, did)),
        peer_scope: Some(peer_scope),
    })
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
    let handle = crate::ids::Handle::parse(peer.as_str(), "")?;
    let lookup = client.directory().lookup_handle_async(handle).await?;
    let did = lookup.did.as_str().to_owned();
    let full_handle = lookup.handle.as_str().to_owned();
    let peer_scope = crate::internal::local_state::owner_scope::DirectPeerScope::new(
        lookup.user_id,
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
                Ok((stream, _)) => return stream,
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
