pub struct MessageService<'a> {
    client: &'a crate::core::ImClient,
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
        match (&request.target, &request.security) {
            (
                super::MessageTarget::Direct(_),
                super::MessageSecurityMode::E2eeRequired | super::MessageSecurityMode::SecureDirect,
            ) => self.send_direct_e2ee(resolve_send_request(self.client, request)?),
            (super::MessageTarget::Direct(_), _) => {
                let resolved = resolve_send_request(self.client, request)?;
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
                    crate::internal::message_runtime::local_projection::persist_direct_outgoing(
                        self.client,
                        &result.target_did,
                        direct_handle_from_result(&result.sdk_result).as_deref(),
                        &result.text,
                        &message_kind_from_result(&result.sdk_result)?,
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
                    crate::internal::message_runtime::local_projection::persist_group_outgoing(
                        self.client,
                        &result.group_did,
                        &result.text,
                        &message_kind_from_result(&result.sdk_result)?,
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

    fn send_direct_e2ee(
        &self,
        resolved: ResolvedSendRequest,
    ) -> crate::ImResult<super::SendMessageResult> {
        #[cfg(feature = "sqlite")]
        {
            crate::internal::secure_direct::send::DirectSecureTextSender::new(
                self.client,
                crate::internal::auth::session::FileSessionProvider::new(self.client),
                crate::internal::transport::CoreHttpTransport::new(self.client),
                crate::internal::transport::CoreHttpTransport::new(self.client),
            )
            .send(crate::internal::secure_direct::send::DirectSecureTextSend {
                request: resolved.request,
                resolved_target_did: resolved.target_did,
            })
            .map(|result| result.sdk_result)
        }
        #[cfg(not(feature = "sqlite"))]
        {
            let _ = resolved;
            Err(crate::ImError::unsupported("secure-direct"))
        }
    }

    #[cfg(feature = "group-e2ee")]
    fn send_group_e2ee(
        &self,
        request: super::SendMessageRequest,
    ) -> crate::ImResult<super::SendMessageResult> {
        let provider =
            crate::internal::group_e2ee::storage::native_provider_for_client(self.client)?;
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

    #[cfg(not(feature = "group-e2ee"))]
    fn send_group_e2ee(
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

    pub fn history(
        &self,
        thread: super::ThreadRef,
        query: super::HistoryQuery,
    ) -> crate::ImResult<crate::ids::Page<super::Message>> {
        self.history_with_metadata(thread, query)
            .map(super::MessagePage::into_page)
    }

    pub fn history_with_metadata(
        &self,
        thread: super::ThreadRef,
        query: super::HistoryQuery,
    ) -> crate::ImResult<super::MessagePage> {
        let (thread, resolved_did, handle_peer) = resolve_history_thread(self.client, thread)?;
        crate::internal::message_runtime::read::MessageReadRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .history(crate::internal::message_runtime::read::HistoryRead {
            thread,
            query,
            resolved_peer_did: resolved_did.clone(),
        })
        .map(|result| {
            let mut page = message_page_from_read_result(result)?;
            #[cfg(feature = "sqlite")]
            if let Some((handle, current_did)) = handle_peer.as_ref() {
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
                if let Some(did) = resolved_did {
                    if let Ok(did) = crate::ids::Did::parse(did) {
                        page.resolved_dids.push(did);
                    }
                }
            }
            Ok(page)
        })?
    }

    pub fn mark_read(
        &self,
        ids: Vec<crate::ids::MessageId>,
    ) -> crate::ImResult<super::MarkReadResult> {
        crate::internal::message_runtime::mark_read::MessageMarkReadRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .mark_read(crate::internal::message_runtime::mark_read::MarkReadInput { message_ids: ids })
        .map(|result| result.sdk_result)
    }

    pub fn conversations(
        &self,
        query: super::ConversationQuery,
    ) -> crate::ImResult<crate::ids::Page<super::Conversation>> {
        crate::internal::message_runtime::conversations::MessageConversationRuntime::new(
            self.client,
        )
        .conversations(query)
    }
}

struct ResolvedSendRequest {
    request: super::SendMessageRequest,
    target_did: Option<String>,
}

fn resolve_send_request(
    client: &crate::core::ImClient,
    request: super::SendMessageRequest,
) -> crate::ImResult<ResolvedSendRequest> {
    let target_did = match &request.target {
        super::MessageTarget::Direct(peer) => resolve_direct_peer_did(client, peer)?,
        super::MessageTarget::Group(_) => None,
    };
    Ok(ResolvedSendRequest {
        request,
        target_did,
    })
}

fn resolve_direct_peer_did(
    client: &crate::core::ImClient,
    peer: &crate::ids::PeerRef,
) -> crate::ImResult<Option<String>> {
    let raw = peer.as_str().trim();
    if raw.is_empty() || raw.starts_with("did:") {
        return Ok(None);
    }
    let handle = crate::ids::Handle::parse(raw, "")?;
    client
        .directory()
        .lookup_handle(handle)
        .map(|lookup| Some(lookup.did.as_str().to_owned()))
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
        super::MessageSecurityMode::E2eeRequired => {
            Err(crate::ImError::unsupported("secure-attachments"))
        }
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
        super::MessageBodyView::Unsupported { content_type } => {
            Err(crate::ImError::unsupported(format!(
                "message-projection-body:{}",
                content_type.as_deref().unwrap_or("unknown")
            )))
        }
    }
}

fn direct_handle_from_result(result: &super::SendMessageResult) -> Option<String> {
    match &result.message.thread {
        super::ThreadRef::Direct(peer) if !peer.as_str().starts_with("did:") => {
            Some(peer.as_str().to_owned())
        }
        _ => None,
    }
}

fn resolve_history_thread(
    client: &crate::core::ImClient,
    thread: super::ThreadRef,
) -> crate::ImResult<(super::ThreadRef, Option<String>, Option<(String, String)>)> {
    let super::ThreadRef::Direct(peer) = thread else {
        return Ok((thread, None, None));
    };
    if peer.as_str().starts_with("did:") {
        return Ok((
            super::ThreadRef::Direct(peer.clone()),
            Some(peer.as_str().to_owned()),
            None,
        ));
    }
    let handle = crate::ids::Handle::parse(peer.as_str(), "")?;
    let lookup = client.directory().lookup_handle(handle)?;
    let did = lookup.did.as_str().to_owned();
    Ok((
        super::ThreadRef::Direct(crate::ids::PeerRef::parse(&did, "")?),
        Some(did.clone()),
        Some((lookup.handle.as_str().to_owned(), did)),
    ))
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

    #[test]
    fn public_group_e2ee_send_uses_native_provider_and_sends_cipher_only() {
        let fixture = Fixture::new();
        let server = RpcTestServer::spawn(vec![
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
            })
            .unwrap();

        assert_eq!(result.message.metadata.server_sequence, Some(91));
        assert!(matches!(
            result.delivery,
            crate::messages::DeliveryState::Accepted
        ));
        let requests = server.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].rpc_method, "group.e2ee.head");
        assert_eq!(requests[1].rpc_method, "group.e2ee.send");
        assert_eq!(
            requests[1].params["meta"]["security_profile"],
            anp::group_e2ee::SECURITY_PROFILE
        );
        assert_eq!(
            requests[1].params["meta"]["content_type"],
            anp::group_e2ee::GROUP_CIPHER_CONTENT_TYPE
        );
        assert_eq!(
            requests[1].params["body"]["group_state_ref"]["group_state_version"],
            "service-state-1"
        );
        assert!(
            requests[1].params["body"]["private_message_b64u"]
                .as_str()
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false),
            "group.e2ee.send must carry an MLS private message"
        );
        let encoded_send = serde_json::to_string(&requests[1].params).unwrap();
        assert!(!encoded_send.contains("public group secret"));
        assert!(!encoded_send.contains("application_plaintext"));
        assert!(!encoded_send.contains("provider"));
        assert!(!encoded_send.contains("StorageProvider"));
        assert!(!encoded_send.contains("mls_state.sqlite"));
        assert!(!encoded_send.contains("openmls_group_id_b64u"));
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
