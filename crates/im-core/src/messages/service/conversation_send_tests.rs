use crate::messages::MessageSecurityMode;

#[test]
fn secure_conversation_modes_use_existing_security_runtime() {
    for security in [
        MessageSecurityMode::E2eeRequired,
        MessageSecurityMode::SecureDirect,
        MessageSecurityMode::GroupE2ee,
    ] {
        assert!(super::conversation_send_uses_security_runtime(&security));
    }
}

#[test]
fn plain_conversation_modes_keep_durable_local_echo_runtime() {
    for security in [
        MessageSecurityMode::DefaultPlain,
        MessageSecurityMode::Plain,
    ] {
        assert!(!super::conversation_send_uses_security_runtime(&security));
    }
}

#[cfg(feature = "sqlite")]
mod stale_direct_rebind_http_tests {
    use std::fs;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::{Duration, Instant};

    use serde_json::{json, Value};

    const HANDLE: &str = "bob.awiki.info";
    const OLD_DID: &str = "did:wba:awiki.info:user:bob:e1-old";
    const NEW_DID: &str = "did:wba:awiki.info:user:bob:e1-new";
    const MESSAGE_ID: &str = "msg-direct-rebind";
    const OPERATION_ID: &str = "op-direct-rebind";
    const TEXT: &str = "one logical message across a stale route";

    #[tokio::test]
    async fn stale_direct_send_refreshes_authority_once_and_reuses_logical_message() {
        let fixture = Fixture::new("success");
        let server = HttpTestServer::spawn(vec![
            ExpectedHttp::rpc_result(directory_lookup(OLD_DID)),
            ExpectedHttp::stale_binding_error(),
            ExpectedHttp::rpc_result(directory_lookup(NEW_DID)),
            ExpectedHttp::json(public_binding(NEW_DID, "2")),
            ExpectedHttp::rpc_result(accepted(NEW_DID)),
            ExpectedHttp::rpc_result(accepted(NEW_DID)),
        ]);
        let client = fixture.client(server.base_url()).await;
        let initial = client
            .directory()
            .lookup_handle_async(crate::ids::Handle::parse(HANDLE, "").unwrap())
            .await
            .unwrap();
        let conversation_id = initial.direct_conversation_id();
        let before = binding_snapshot(&fixture.sqlite_path(), &conversation_id);

        let result = client
            .messages()
            .send_conversation_text_async(crate::messages::SendConversationTextRequest {
                conversation: crate::messages::ConversationReadRef::new(&conversation_id).unwrap(),
                text: TEXT.to_owned(),
                markdown: false,
                security: crate::messages::MessageSecurityMode::DefaultPlain,
                client_message_id: Some(crate::ids::MessageId::parse(MESSAGE_ID).unwrap()),
                idempotency_key: Some(OPERATION_ID.to_owned()),
                wait_for_final_acceptance: true,
                delegated_signing: None,
            })
            .await
            .unwrap();

        assert_eq!(result.message.id.as_str(), MESSAGE_ID);
        assert_eq!(
            result
                .message
                .metadata
                .conversation_identity
                .as_ref()
                .map(|identity| identity.conversation_id.as_str()),
            Some(conversation_id.as_str())
        );
        assert_eq!(
            result.message.metadata.operation_id.as_deref(),
            Some(OPERATION_ID)
        );
        assert!(result.warnings.is_empty());

        tokio::time::sleep(Duration::from_millis(1_100)).await;

        let replay = client
            .messages()
            .send_conversation_text_async(crate::messages::SendConversationTextRequest {
                conversation: crate::messages::ConversationReadRef::new(&conversation_id).unwrap(),
                text: TEXT.to_owned(),
                markdown: false,
                security: crate::messages::MessageSecurityMode::DefaultPlain,
                client_message_id: Some(crate::ids::MessageId::parse(MESSAGE_ID).unwrap()),
                idempotency_key: Some(OPERATION_ID.to_owned()),
                wait_for_final_acceptance: true,
                delegated_signing: None,
            })
            .await
            .unwrap();
        assert_eq!(replay.message.id.as_str(), MESSAGE_ID);
        assert_eq!(
            replay.message.metadata.operation_id.as_deref(),
            Some(OPERATION_ID)
        );
        assert!(replay.warnings.is_empty());

        let after = binding_snapshot(&fixture.sqlite_path(), &conversation_id);
        assert_eq!(after.peer_persona_id, before.peer_persona_id);
        assert_eq!(after.conversation_id, before.conversation_id);
        assert_eq!(after.full_handle, before.full_handle);
        assert_eq!(after.current_did, NEW_DID);
        assert_eq!(after.binding_generation.as_deref(), Some("2"));
        assert_eq!(
            message_projection(&fixture.sqlite_path(), MESSAGE_ID),
            MessageProjection {
                count: 1,
                conversation_id: conversation_id.clone(),
                receiver_did: OLD_DID.to_owned(),
                content: TEXT.to_owned(),
                operation_id: OPERATION_ID.to_owned(),
                delivery_state: "accepted".to_owned(),
                current_target_did: NEW_DID.to_owned(),
            }
        );

        let requests = server.join();
        assert_eq!(requests.len(), 6);
        assert_request_sequence(
            &requests,
            &[
                "lookup",
                "direct.send",
                "lookup",
                "GET",
                "direct.send",
                "direct.send",
            ],
        );
        assert_eq!(requests[3].path, "/.well-known/handle/bob");
        let sends = [&requests[1], &requests[4], &requests[5]];
        assert_eq!(sends[0].params()["meta"]["target"]["did"], OLD_DID);
        assert_eq!(sends[1].params()["meta"]["target"]["did"], NEW_DID);
        assert_eq!(sends[2].params()["meta"]["target"]["did"], NEW_DID);
        for request in sends {
            assert_eq!(request.params()["meta"]["message_id"], MESSAGE_ID);
            assert_eq!(request.params()["meta"]["operation_id"], OPERATION_ID);
            assert_eq!(request.params()["body"]["text"], TEXT);
        }
        assert_eq!(
            sends[1].params()["meta"]["created_at"],
            sends[2].params()["meta"]["created_at"],
            "logical replay must preserve the first wire timestamp across seconds"
        );
    }

    #[tokio::test]
    async fn second_stale_failure_is_terminal_without_a_third_send() {
        let fixture = Fixture::new("bounded");
        let server = HttpTestServer::spawn(vec![
            ExpectedHttp::rpc_result(directory_lookup(OLD_DID)),
            ExpectedHttp::stale_binding_error(),
            ExpectedHttp::rpc_result(directory_lookup(NEW_DID)),
            ExpectedHttp::json(public_binding(NEW_DID, "2")),
            ExpectedHttp::stale_binding_error(),
        ]);
        let client = fixture.client(server.base_url()).await;
        let initial = client
            .directory()
            .lookup_handle_async(crate::ids::Handle::parse(HANDLE, "").unwrap())
            .await
            .unwrap();
        let conversation_id = initial.direct_conversation_id();

        let error = client
            .messages()
            .send_conversation_text_async(crate::messages::SendConversationTextRequest {
                conversation: crate::messages::ConversationReadRef::new(&conversation_id).unwrap(),
                text: TEXT.to_owned(),
                markdown: false,
                security: crate::messages::MessageSecurityMode::DefaultPlain,
                client_message_id: Some(crate::ids::MessageId::parse(MESSAGE_ID).unwrap()),
                idempotency_key: Some(OPERATION_ID.to_owned()),
                wait_for_final_acceptance: true,
                delegated_signing: None,
            })
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            crate::ImError::Service { code: Some(code), .. }
                if code == "anp.invalid_target_binding"
        ));
        let projection = message_projection(&fixture.sqlite_path(), MESSAGE_ID);
        assert_eq!(projection.count, 1);
        assert_eq!(projection.conversation_id, conversation_id);
        assert_eq!(projection.receiver_did, OLD_DID);
        assert_eq!(projection.operation_id, OPERATION_ID);
        assert_eq!(
            binding_snapshot(&fixture.sqlite_path(), &projection.conversation_id).current_did,
            NEW_DID
        );

        let requests = server.join();
        assert_eq!(requests.len(), 5);
        assert_request_sequence(
            &requests,
            &["lookup", "direct.send", "lookup", "GET", "direct.send"],
        );
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.rpc_method().as_deref() == Some("direct.send"))
                .count(),
            2
        );
    }

    #[tokio::test]
    async fn successful_direct_send_uses_cached_route_without_authority_refresh() {
        let fixture = Fixture::new("normal");
        let server = HttpTestServer::spawn(vec![
            ExpectedHttp::rpc_result(directory_lookup(OLD_DID)),
            ExpectedHttp::rpc_result(accepted(OLD_DID)),
        ]);
        let client = fixture.client(server.base_url()).await;
        let initial = client
            .directory()
            .lookup_handle_async(crate::ids::Handle::parse(HANDLE, "").unwrap())
            .await
            .unwrap();

        client
            .messages()
            .send_conversation_text_async(crate::messages::SendConversationTextRequest {
                conversation: crate::messages::ConversationReadRef::new(
                    initial.direct_conversation_id(),
                )
                .unwrap(),
                text: TEXT.to_owned(),
                markdown: false,
                security: crate::messages::MessageSecurityMode::DefaultPlain,
                client_message_id: Some(crate::ids::MessageId::parse(MESSAGE_ID).unwrap()),
                idempotency_key: Some(OPERATION_ID.to_owned()),
                wait_for_final_acceptance: true,
                delegated_signing: None,
            })
            .await
            .unwrap();

        let requests = server.join();
        assert_eq!(requests.len(), 2);
        assert_request_sequence(&requests, &["lookup", "direct.send"]);
        assert!(requests.iter().all(|request| request.method != "GET"));
    }

    fn directory_lookup(did: &str) -> Value {
        json!({
            "handle": "bob",
            "full_handle": HANDLE,
            "did": did,
            "user_id": "user-bob",
            "domain": "awiki.info",
            "status": "active"
        })
    }

    fn public_binding(did: &str, generation: &str) -> Value {
        json!({
            "status": "active",
            "handle": HANDLE,
            "did": did,
            "binding_generation": generation
        })
    }

    fn accepted(target_did: &str) -> Value {
        json!({
            "accepted": true,
            "final_acceptance": true,
            "message_id": MESSAGE_ID,
            "operation_id": OPERATION_ID,
            "target_did": target_did,
            "accepted_at": "2026-08-19T00:00:00Z",
            "delivery_state": "accepted"
        })
    }

    #[derive(Debug, PartialEq, Eq)]
    struct BindingSnapshot {
        peer_persona_id: String,
        conversation_id: String,
        full_handle: String,
        current_did: String,
        binding_generation: Option<String>,
    }

    fn binding_snapshot(sqlite_path: &Path, conversation_id: &str) -> BindingSnapshot {
        let connection = rusqlite::Connection::open(sqlite_path).unwrap();
        connection
            .query_row(
                r#"SELECT p.peer_persona_id, r.conversation_id, p.full_handle,
                          r.current_did, p.binding_generation
FROM peer_personas p
JOIN direct_peer_routes r
  ON r.owner_identity_id = p.owner_identity_id
 AND r.peer_persona_id = p.peer_persona_id
WHERE p.owner_identity_id = 'alice-id' AND r.conversation_id = ?1"#,
                [conversation_id],
                |row| {
                    Ok(BindingSnapshot {
                        peer_persona_id: row.get(0)?,
                        conversation_id: row.get(1)?,
                        full_handle: row.get(2)?,
                        current_did: row.get(3)?,
                        binding_generation: row.get(4)?,
                    })
                },
            )
            .unwrap()
    }

    #[derive(Debug, PartialEq, Eq)]
    struct MessageProjection {
        count: i64,
        conversation_id: String,
        receiver_did: String,
        content: String,
        operation_id: String,
        delivery_state: String,
        current_target_did: String,
    }

    fn message_projection(sqlite_path: &Path, message_id: &str) -> MessageProjection {
        let connection = rusqlite::Connection::open(sqlite_path).unwrap();
        let count = connection
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE owner_identity_id = 'alice-id' AND msg_id = ?1",
                [message_id],
                |row| row.get(0),
            )
            .unwrap();
        connection
            .query_row(
                r#"SELECT conversation_id, receiver_did, content, metadata
FROM messages WHERE owner_identity_id = 'alice-id' AND msg_id = ?1"#,
                [message_id],
                |row| {
                    let metadata: String = row.get(3)?;
                    let metadata: Value = serde_json::from_str(&metadata).unwrap();
                    Ok(MessageProjection {
                        count,
                        conversation_id: row.get(0)?,
                        receiver_did: row.get(1)?,
                        content: row.get(2)?,
                        operation_id: metadata["operation_id"]
                            .as_str()
                            .unwrap_or_default()
                            .to_owned(),
                        delivery_state: metadata["delivery_state"]
                            .as_str()
                            .unwrap_or_default()
                            .to_owned(),
                        current_target_did: metadata["peer_current_did"]
                            .as_str()
                            .unwrap_or_default()
                            .to_owned(),
                    })
                },
            )
            .unwrap()
    }

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new(label: &str) -> Self {
            let root = unique_temp_root(label);
            write_identity(&root);
            Self { root }
        }

        async fn client(&self, base_url: &str) -> crate::core::ImClient {
            let core = crate::core::ImCore::open(self.config(base_url), self.paths())
                .await
                .unwrap();
            core.client_async(crate::identity::IdentitySelector::LocalAlias(
                "alice".to_owned(),
            ))
            .await
            .unwrap()
        }

        fn config(&self, base_url: &str) -> crate::ImCoreConfig {
            crate::ImCoreConfig {
                service_base_url: crate::ServiceEndpoint::parse(base_url).unwrap(),
                did_domain: "awiki.info".to_owned(),
                client_version_info: None,
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
                    sqlite_path: self.sqlite_path(),
                },
                runtime: crate::RuntimePaths {
                    cache_dir: self.root.join("cache"),
                    temp_dir: self.root.join("tmp"),
                },
            }
        }

        fn sqlite_path(&self) -> PathBuf {
            self.root.join("local").join("im.sqlite")
        }
    }

    fn write_identity(root: &Path) {
        let identity_root = root.join("identities");
        let identity_dir = identity_root.join("alice");
        fs::create_dir_all(&identity_dir).unwrap();
        fs::write(identity_root.join("default"), "alice\n").unwrap();
        fs::write(
            identity_root.join("registry.json"),
            json!({
                "default_identity": "alice",
                "identities": [{
                    "id": "alice-id",
                    "did": "did:example:alice",
                    "handle": "alice.awiki.info",
                    "local_alias": "alice",
                    "ready_for_auth": true,
                    "ready_for_messaging": true,
                    "missing": []
                }]
            })
            .to_string(),
        )
        .unwrap();
        let bundle = anp::authentication::create_did_wba_document(
            "awiki.info",
            anp::authentication::DidDocumentOptions {
                path_segments: vec!["user".to_owned()],
                domain: Some("awiki.info".to_owned()),
                challenge: Some("direct-rebind-http-test".to_owned()),
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

    fn unique_temp_root(label: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "im-core-direct-rebind-{label}-{}-{nanos}",
            std::process::id()
        ))
    }

    struct ExpectedHttp {
        status_code: u16,
        content_type: &'static str,
        body: Vec<u8>,
    }

    impl ExpectedHttp {
        fn json(body: Value) -> Self {
            Self {
                status_code: 200,
                content_type: "application/json",
                body: body.to_string().into_bytes(),
            }
        }

        fn rpc_result(result: Value) -> Self {
            Self::json(json!({"jsonrpc": "2.0", "id": "req-1", "result": result}))
        }

        fn stale_binding_error() -> Self {
            Self::json(json!({
                "jsonrpc": "2.0",
                "id": "req-1",
                "error": {
                    "code": 1406,
                    "message": "target binding is stale",
                    "data": {
                        "anp_code": "anp.invalid_target_binding",
                        "reason": "stale_did",
                        "current_did": NEW_DID,
                        "full_handle": HANDLE
                    }
                }
            }))
        }
    }

    #[derive(Debug, Clone)]
    struct CapturedHttp {
        method: String,
        path: String,
        body: Vec<u8>,
    }

    impl CapturedHttp {
        fn json_body(&self) -> Value {
            serde_json::from_slice(&self.body).unwrap_or(Value::Null)
        }

        fn rpc_method(&self) -> Option<String> {
            self.json_body()
                .get("method")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        }

        fn params(&self) -> Value {
            self.json_body()
                .get("params")
                .cloned()
                .unwrap_or(Value::Null)
        }
    }

    struct HttpTestServer {
        base_url: String,
        requests: Arc<Mutex<Vec<CapturedHttp>>>,
        join: thread::JoinHandle<()>,
    }

    impl HttpTestServer {
        fn spawn(responses: Vec<ExpectedHttp>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let base_url = format!("http://{}", listener.local_addr().unwrap());
            let requests = Arc::new(Mutex::new(Vec::new()));
            let captured = Arc::clone(&requests);
            let join = thread::spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(15);
                for response in responses {
                    let mut stream = accept_before_deadline(&listener, deadline);
                    captured.lock().unwrap().push(read_request(&mut stream));
                    write_response(&mut stream, response);
                }
            });
            Self {
                base_url,
                requests,
                join,
            }
        }

        fn base_url(&self) -> &str {
            &self.base_url
        }

        fn join(self) -> Vec<CapturedHttp> {
            self.join.join().unwrap();
            Arc::try_unwrap(self.requests)
                .unwrap()
                .into_inner()
                .unwrap()
        }
    }

    fn assert_request_sequence(requests: &[CapturedHttp], expected: &[&str]) {
        let actual = requests
            .iter()
            .map(|request| {
                request
                    .rpc_method()
                    .unwrap_or_else(|| request.method.clone())
            })
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
    }

    fn accept_before_deadline(listener: &TcpListener, deadline: Instant) -> TcpStream {
        loop {
            match listener.accept() {
                Ok((stream, _)) => return stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "timed out waiting for request");
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("accept request: {error}"),
            }
        }
    }

    fn read_request(stream: &mut TcpStream) -> CapturedHttp {
        stream
            .set_read_timeout(Some(Duration::from_millis(250)))
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut raw = Vec::new();
        let mut buffer = [0_u8; 4096];
        let header_end = loop {
            let count = read_before_deadline(stream, &mut buffer, deadline);
            assert!(count > 0, "request closed before headers");
            raw.extend_from_slice(&buffer[..count]);
            if let Some(index) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
                break index;
            }
        };
        let header = std::str::from_utf8(&raw[..header_end]).unwrap();
        let mut lines = header.lines();
        let mut request_line = lines.next().unwrap().split_whitespace();
        let method = request_line.next().unwrap().to_owned();
        let path = request_line.next().unwrap().to_owned();
        let content_length = lines
            .filter_map(|line| line.split_once(':'))
            .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
            .and_then(|(_, value)| value.trim().parse::<usize>().ok())
            .unwrap_or(0);
        let body_start = header_end + 4;
        while raw.len() < body_start + content_length {
            let count = read_before_deadline(stream, &mut buffer, deadline);
            assert!(count > 0, "request closed before body");
            raw.extend_from_slice(&buffer[..count]);
        }
        CapturedHttp {
            method,
            path,
            body: raw[body_start..body_start + content_length].to_vec(),
        }
    }

    fn read_before_deadline(stream: &mut TcpStream, buffer: &mut [u8], deadline: Instant) -> usize {
        loop {
            match stream.read(buffer) {
                Ok(count) => return count,
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock
                            | std::io::ErrorKind::TimedOut
                            | std::io::ErrorKind::Interrupted
                    ) =>
                {
                    assert!(Instant::now() < deadline, "timed out reading request");
                }
                Err(error) => panic!("read request: {error}"),
            }
        }
    }

    fn write_response(stream: &mut TcpStream, response: ExpectedHttp) {
        let head = format!(
            "HTTP/1.1 {} OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            response.status_code,
            response.content_type,
            response.body.len()
        );
        stream.write_all(head.as_bytes()).unwrap();
        stream.write_all(&response.body).unwrap();
        stream.flush().unwrap();
    }
}
