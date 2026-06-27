use super::*;
use crate::internal::transport::{AttachmentObjectTransport, AuthenticatedRpcTransport};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;

#[test]
fn attachments_upload_runtime_bytes_direct_runs_create_put_commit_and_send() {
    let fixture = Fixture::new(Some("did:example:message-service"));
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let sessions = Rc::new(RefCell::new(Vec::new()));
    let result = AttachmentUploadRuntime::new(
        &client,
        ReadySessionProvider {
            scopes: Rc::clone(&sessions),
        },
        RecordingTransport {
            calls: Rc::clone(&calls),
        },
    )
    .send(AttachmentSendInput {
        target: crate::messages::MessageTarget::Direct(
            crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
        ),
        request: bytes_request(
            Some("input.txt"),
            Some("text/plain"),
            b"hello".to_vec(),
            Some("override.bin"),
            Some("application/custom"),
            Some("caption"),
        ),
        resolved_target_did: None,
        credentials: Some(fixture.credentials()),
    })
    .unwrap();

    assert_eq!(
        sessions.borrow().as_slice(),
        &[crate::auth::AuthScope::Messaging]
    );
    assert_eq!(result.target_kind, "agent");
    assert_eq!(result.target_did, "did:example:bob");
    assert_eq!(result.prepared.filename, "override.bin");
    assert_eq!(result.prepared.mime_type, "application/custom");
    assert_eq!(result.prepared.payload, b"hello".to_vec());
    assert_eq!(result.manifest["caption"], "caption");
    assert_eq!(
        result.sdk_result.message.metadata.content_type.as_deref(),
        Some(crate::attachments::manifest::attachment_manifest_content_type())
    );
    assert!(matches!(
        result.sdk_result.message.body,
        crate::messages::MessageBodyView::Unsupported { content_type }
            if content_type.as_deref()
                == Some(crate::attachments::manifest::attachment_manifest_content_type())
    ));
    assert_eq!(
        result.sdk_result.message.receiver.unwrap().as_str(),
        "did:example:bob"
    );

    let calls = calls.borrow();
    assert_eq!(calls.len(), 4);
    let create = calls[0].rpc("attachment.create_slot");
    assert_eq!(create.endpoint, MESSAGE_RPC_ENDPOINT);
    assert_eq!(
        create.params["meta"]["target"],
        json!({"kind": "service", "did": "did:example:message-service"})
    );
    assert_eq!(create.params["body"]["filename"], "override.bin");
    assert_eq!(create.params["body"]["mime_type"], "application/custom");
    assert_eq!(create.params["body"]["expected_size"], "5");
    assert_eq!(
        create.params["body"]["intended_target"],
        json!({"kind": "agent", "did": "did:example:bob"})
    );

    let put = calls[1].put("https://upload.example/slot-1");
    assert_eq!(
        put.headers.get("X-Upload-Token").map(String::as_str),
        Some("token-1")
    );
    assert_eq!(put.headers.get("Ignored-Number"), None);
    assert_eq!(put.body.as_slice(), b"hello");

    let commit = calls[2].rpc("attachment.commit_object");
    assert_eq!(commit.params["body"]["attachment_id"], "att-1");
    assert_eq!(commit.params["body"]["slot_id"], "slot-1");
    assert_eq!(
        commit.params["body"]["digest"]["value_b64u"],
        result.prepared.digest_b64u
    );

    let send = calls[3].rpc("direct.send");
    assert_eq!(
        send.params["meta"]["content_type"],
        crate::attachments::manifest::attachment_manifest_content_type()
    );
    assert_eq!(
        send.params["body"]["payload"]["primary_attachment_id"],
        "att-1"
    );
    assert_eq!(send.params["body"]["payload"]["caption"], "caption");
    assert_eq!(
        result.sdk_result.message.id.as_str(),
        send.params["meta"]["message_id"].as_str().unwrap()
    );
    assert_eq!(
        result.sdk_result.message.metadata.operation_id.as_deref(),
        send.params["meta"]["operation_id"].as_str()
    );
}

#[test]
fn attachments_upload_runtime_local_file_reads_only_explicit_path() {
    let fixture = Fixture::new(Some("did:example:message-service"));
    let client = fixture.client();
    let file = fixture.root.join("explicit").join("report.txt");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, b"file bytes").unwrap();

    let calls = Rc::new(RefCell::new(Vec::new()));
    let result = AttachmentUploadRuntime::new(
        &client,
        ReadySessionProvider {
            scopes: Rc::new(RefCell::new(Vec::new())),
        },
        RecordingTransport {
            calls: Rc::clone(&calls),
        },
    )
    .send(AttachmentSendInput {
        target: crate::messages::MessageTarget::Direct(
            crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
        ),
        request: crate::attachments::AttachmentSendRequest {
            input: crate::attachments::AttachmentInput::LocalFile(file.clone()),
            caption: None,
            mention_payload: None,
            mime_type: None,
            filename: None,
            delivery: crate::messages::MessageDeliveryOptions::default(),
            security: crate::messages::MessageSecurityMode::DefaultPlain,
        },
        resolved_target_did: None,
        credentials: Some(fixture.credentials()),
    })
    .unwrap();

    assert_eq!(result.prepared.filename, "report.txt");
    assert_eq!(result.prepared.mime_type, "text/plain; charset=utf-8");
    let calls = calls.borrow();
    assert_eq!(
        calls[1]
            .put("https://upload.example/slot-1")
            .body
            .as_slice(),
        b"file bytes"
    );
}

#[tokio::test]
async fn attachments_upload_runtime_local_file_async_streams_explicit_path() {
    let fixture = Fixture::new(Some("did:example:message-service"));
    let client = fixture.client();
    let file = fixture.root.join("explicit").join("large-report.txt");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, b"file bytes streamed from disk").unwrap();

    let calls = Rc::new(RefCell::new(Vec::new()));
    let result = AttachmentUploadRuntime::new(
        &client,
        ReadySessionProvider {
            scopes: Rc::new(RefCell::new(Vec::new())),
        },
        RecordingTransport {
            calls: Rc::clone(&calls),
        },
    )
    .send_async(AttachmentSendInput {
        target: crate::messages::MessageTarget::Direct(
            crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
        ),
        request: crate::attachments::AttachmentSendRequest {
            input: crate::attachments::AttachmentInput::LocalFile(file.clone()),
            caption: None,
            mention_payload: None,
            mime_type: None,
            filename: None,
            delivery: crate::messages::MessageDeliveryOptions::default(),
            security: crate::messages::MessageSecurityMode::DefaultPlain,
        },
        resolved_target_did: None,
        credentials: Some(fixture.credentials()),
    })
    .await
    .unwrap();

    assert_eq!(result.prepared.filename, "large-report.txt");
    assert_eq!(result.prepared.mime_type, "text/plain; charset=utf-8");
    assert!(result.prepared.payload.is_empty());
    let calls = calls.borrow();
    let create = calls[0].rpc("attachment.create_slot");
    assert_eq!(create.params["body"]["expected_size"], "29");
    let put = calls[1].put_file("https://upload.example/slot-1");
    assert_eq!(put.path, &file);
    assert_eq!(put.len, 29);
    assert_eq!(
        put.content_type.as_deref(),
        Some("text/plain; charset=utf-8")
    );
    assert_eq!(
        put.headers.get("X-Upload-Token").map(String::as_str),
        Some("token-1")
    );
}

#[test]
fn attachments_upload_runtime_prepare_object_e2ee_uploads_ciphertext_only() {
    let fixture = Fixture::new(Some("did:example:message-service"));
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let sessions = Rc::new(RefCell::new(Vec::new()));
    let plaintext = b"attachment plaintext secret".to_vec();

    let result = AttachmentUploadRuntime::new(
        &client,
        ReadySessionProvider {
            scopes: Rc::clone(&sessions),
        },
        RecordingTransport {
            calls: Rc::clone(&calls),
        },
    )
    .prepare_and_commit_object(AttachmentPrepareObjectInput {
        target: crate::messages::MessageTarget::Direct(
            crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
        ),
        request: bytes_request(
            Some("secret.pdf"),
            Some("application/pdf"),
            plaintext.clone(),
            None,
            None,
            Some("secure caption"),
        ),
        resolved_target_did: None,
        message_security_profile: "direct-e2ee",
    })
    .unwrap();

    assert_eq!(
        sessions.borrow().as_slice(),
        &[crate::auth::AuthScope::Messaging]
    );
    assert_eq!(result.target_kind, "agent");
    assert_eq!(result.target_did, "did:example:bob");
    assert_eq!(result.prepared.filename, "secret.pdf");
    assert_eq!(result.prepared.mime_type, "application/pdf");
    assert_eq!(result.prepared.object_encryption_mode(), "object-e2ee");
    assert_eq!(
        result.prepared.plaintext_size_bytes,
        Some(plaintext.len() as u64)
    );
    assert_ne!(result.prepared.payload, plaintext);

    let object_key = result.full_manifest["attachments"][0]["encryption_info"]["object_key_b64u"]
        .as_str()
        .expect("full manifest object key")
        .to_owned();
    let nonce = result.full_manifest["attachments"][0]["encryption_info"]["nonce_b64u"]
        .as_str()
        .expect("full manifest nonce")
        .to_owned();
    assert!(!object_key.is_empty());
    assert!(!nonce.is_empty());

    let redacted_text = result.redacted_manifest.to_string();
    let grant_ref_text = result.grant_ref.to_string();
    assert!(!redacted_text.contains("object_key_b64u"));
    assert!(!redacted_text.contains("nonce_b64u"));
    assert!(!redacted_text.contains(&object_key));
    assert!(!redacted_text.contains(&nonce));
    assert!(!grant_ref_text.contains("object_key_b64u"));
    assert!(!grant_ref_text.contains("nonce_b64u"));
    assert!(!grant_ref_text.contains(&object_key));
    assert!(!grant_ref_text.contains(&nonce));
    assert_eq!(result.grant_ref["object_encryption_mode"], "object-e2ee");
    assert_eq!(
        result.grant_ref["plaintext_size"],
        plaintext.len().to_string()
    );

    let calls = calls.borrow();
    assert_eq!(calls.len(), 3);
    let create = calls[0].rpc("attachment.create_slot");
    assert_eq!(create.endpoint, MESSAGE_RPC_ENDPOINT);
    assert_eq!(
        create.params["body"]["intended_message_security_profile"],
        "direct-e2ee"
    );
    assert_eq!(
        create.params["body"]["object_encryption_mode"],
        "object-e2ee"
    );
    assert_eq!(
        create.params["body"]["expected_size"],
        result.prepared.size_string
    );
    assert_eq!(
        create.params["body"]["expected_digest"]["value_b64u"],
        result.prepared.digest_b64u
    );
    assert_eq!(create.params["body"].get("object_key_b64u"), None);
    assert_eq!(create.params["body"].get("nonce_b64u"), None);

    let put = calls[1].put("https://upload.example/slot-1");
    assert_eq!(put.body.as_slice(), result.prepared.payload.as_slice());
    assert_ne!(put.body.as_slice(), plaintext.as_slice());
    assert_eq!(
        put.headers.get("X-Upload-Token").map(String::as_str),
        Some("token-1")
    );

    let commit = calls[2].rpc("attachment.commit_object");
    assert_eq!(
        commit.params["body"]["object_encryption_mode"],
        "object-e2ee"
    );
    assert_eq!(
        commit.params["body"]["plaintext_size"],
        plaintext.len().to_string()
    );
    assert_eq!(
        commit.params["body"]["digest"]["value_b64u"],
        result.prepared.digest_b64u
    );
    assert_eq!(commit.params["body"].get("object_key_b64u"), None);
    assert_eq!(commit.params["body"].get("nonce_b64u"), None);
}

#[test]
fn attachments_upload_runtime_group_uses_group_scope_and_wire() {
    let fixture = Fixture::new(Some("did:example:message-service"));
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let sessions = Rc::new(RefCell::new(Vec::new()));

    let result = AttachmentUploadRuntime::new(
        &client,
        ReadySessionProvider {
            scopes: Rc::clone(&sessions),
        },
        RecordingTransport {
            calls: Rc::clone(&calls),
        },
    )
    .send(AttachmentSendInput {
        target: crate::messages::MessageTarget::Group(
            crate::ids::GroupRef::parse("did:example:group").unwrap(),
        ),
        request: bytes_request(
            Some("group.txt"),
            None,
            b"group attachment".to_vec(),
            None,
            None,
            None,
        ),
        resolved_target_did: None,
        credentials: Some(fixture.credentials()),
    })
    .unwrap();

    assert_eq!(
        sessions.borrow().as_slice(),
        &[crate::auth::AuthScope::GroupMessaging]
    );
    assert_eq!(result.target_kind, "group");
    assert_eq!(result.target_did, "did:example:group");
    assert_eq!(result.sdk_result.message.id.as_str(), "did:example:group:7");
    assert_eq!(result.sdk_result.message.metadata.server_sequence, Some(7));
    assert!(result
        .sdk_result
        .message
        .metadata
        .attributes
        .iter()
        .any(|attribute| attribute.key == "group_event_seq" && attribute.value == "7"));

    let calls = calls.borrow();
    let create = calls[0].rpc("attachment.create_slot");
    assert_eq!(
        create.params["body"]["intended_target"],
        json!({"kind": "group", "did": "did:example:group"})
    );
    let send = calls[3].rpc("group.send");
    assert_eq!(
        send.params["meta"]["target"],
        json!({"kind": "group", "did": "did:example:group"})
    );
}

#[test]
fn attachments_upload_runtime_bytes_requires_filename_before_transport() {
    let fixture = Fixture::new(Some("did:example:message-service"));
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));

    let err = AttachmentUploadRuntime::new(
        &client,
        ReadySessionProvider {
            scopes: Rc::new(RefCell::new(Vec::new())),
        },
        RecordingTransport {
            calls: Rc::clone(&calls),
        },
    )
    .send(AttachmentSendInput {
        target: crate::messages::MessageTarget::Direct(
            crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
        ),
        request: bytes_request(None, None, b"hello".to_vec(), None, None, None),
        resolved_target_did: None,
        credentials: Some(fixture.credentials()),
    })
    .expect_err("bytes input without filename should fail");

    assert!(matches!(
        err,
        crate::ImError::InvalidInput { field: Some(field), message }
            if field == "filename" && message == "attachment filename is required"
    ));
    assert!(calls.borrow().is_empty());
}

#[test]
fn attachments_upload_runtime_direct_handle_requires_resolved_did() {
    let fixture = Fixture::new(Some("did:example:message-service"));
    let client = fixture.client();

    let err = AttachmentUploadRuntime::new(
        &client,
        ReadySessionProvider {
            scopes: Rc::new(RefCell::new(Vec::new())),
        },
        RecordingTransport {
            calls: Rc::new(RefCell::new(Vec::new())),
        },
    )
    .send(AttachmentSendInput {
        target: crate::messages::MessageTarget::Direct(
            crate::ids::PeerRef::parse("bob.awiki.test", "").unwrap(),
        ),
        request: bytes_request(Some("note.txt"), None, b"hello".to_vec(), None, None, None),
        resolved_target_did: None,
        credentials: Some(fixture.credentials()),
    })
    .expect_err("unresolved direct handle should fail");

    assert!(matches!(
        err,
        crate::ImError::PeerNotFound { peer } if peer == "bob.awiki.test"
    ));
}

#[derive(Clone)]
struct ReadySessionProvider {
    scopes: Rc<RefCell<Vec<crate::auth::AuthScope>>>,
}

impl SessionProvider for ReadySessionProvider {
    fn ensure_session(
        &self,
        scope: crate::auth::AuthScope,
    ) -> crate::ImResult<crate::auth::SessionBundle> {
        self.scopes.borrow_mut().push(scope);
        Ok(crate::auth::SessionBundle {
            subject: crate::ids::Did::parse("did:example:alice")?,
            scope,
            expires_at: None,
            refreshed: false,
            bearer_token: None,
        })
    }

    fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
        unreachable!("attachment upload runtime should not refresh through the test provider")
    }

    fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
        unreachable!("attachment upload runtime should not read status")
    }
}

impl crate::internal::auth::session::AsyncSessionProvider for ReadySessionProvider {
    async fn ensure_session(
        &self,
        scope: crate::auth::AuthScope,
    ) -> crate::ImResult<crate::auth::SessionBundle> {
        self.scopes.borrow_mut().push(scope);
        Ok(crate::auth::SessionBundle {
            subject: crate::ids::Did::parse("did:example:alice")?,
            scope,
            expires_at: None,
            refreshed: false,
            bearer_token: None,
        })
    }

    async fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
        unreachable!("attachment upload runtime should not refresh through the test provider")
    }

    async fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
        unreachable!("attachment upload runtime should not read status")
    }
}

struct RecordingTransport {
    calls: Rc<RefCell<Vec<RecordedCall>>>,
}

impl AuthenticatedRpcTransport for RecordingTransport {
    fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> crate::ImResult<Value> {
        self.calls.borrow_mut().push(RecordedCall::Rpc {
            endpoint: endpoint.to_string(),
            method: method.to_string(),
            params: params.clone(),
        });
        match method {
            "attachment.create_slot" => Ok(json!({
                "attachment_id": "att-1",
                "slot_id": "slot-1",
                "upload_uri": "https://upload.example/slot-1",
                "upload_headers": {
                    "X-Upload-Token": "token-1",
                    "Ignored-Number": 7
                },
                "object_uri": "https://objects.example/att-1",
                "commit_token": "commit-token-1",
                "expires_at": "2026-05-23T01:00:00Z"
            })),
            "attachment.commit_object" => Ok(json!({
                "committed": true,
                "attachment_id": "att-1",
                "object_uri": "https://objects.example/att-1",
                "committed_at": "2026-05-23T00:00:01Z"
            })),
            "direct.send" => Ok(json!({
                "accepted": true,
                "accepted_at": "2026-05-23T00:00:02Z",
                "delivery_state": "accepted"
            })),
            "group.send" => Ok(json!({
                "accepted": true,
                "group_did": "did:example:group",
                "message_id": "group-raw-message-7",
                "operation_id": "group-op-7",
                "group_event_seq": "7",
                "group_state_version": "3",
                "accepted_at": "2026-05-23T00:00:03Z"
            })),
            _ => Err(crate::ImError::TransportUnavailable {
                detail: format!("unexpected method {method} at {endpoint}"),
            }),
        }
    }
}

impl AttachmentObjectTransport for RecordingTransport {
    fn put_attachment_object(
        &mut self,
        upload_uri: &str,
        headers: BTreeMap<String, String>,
        body: Vec<u8>,
    ) -> crate::ImResult<()> {
        self.calls.borrow_mut().push(RecordedCall::Put {
            upload_uri: upload_uri.to_string(),
            headers,
            body,
        });
        Ok(())
    }

    fn get_attachment_object(
        &mut self,
        _object_uri: &str,
        _download_ticket: &str,
    ) -> crate::ImResult<crate::internal::transport::AttachmentObjectResponse> {
        unreachable!("upload runtime should not download objects")
    }
}

impl crate::internal::transport::AsyncAuthenticatedRpcTransport for RecordingTransport {
    async fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> crate::ImResult<Value> {
        AuthenticatedRpcTransport::authenticated_rpc(self, endpoint, method, params)
    }
}

impl crate::internal::transport::AsyncAttachmentObjectTransport for RecordingTransport {
    async fn put_attachment_object(
        &mut self,
        upload_uri: &str,
        headers: BTreeMap<String, String>,
        body: Vec<u8>,
    ) -> crate::ImResult<()> {
        AttachmentObjectTransport::put_attachment_object(self, upload_uri, headers, body)
    }

    async fn get_attachment_object(
        &mut self,
        object_uri: &str,
        download_ticket: &str,
    ) -> crate::ImResult<crate::internal::transport::AttachmentObjectResponse> {
        AttachmentObjectTransport::get_attachment_object(self, object_uri, download_ticket)
    }

    async fn put_attachment_object_stream(
        &mut self,
        upload_uri: &str,
        headers: BTreeMap<String, String>,
        body: crate::internal::transport::AsyncAttachmentObjectBody,
    ) -> crate::ImResult<()> {
        match body {
            crate::internal::transport::AsyncAttachmentObjectBody::Bytes(bytes) => {
                self.calls.borrow_mut().push(RecordedCall::Put {
                    upload_uri: upload_uri.to_string(),
                    headers,
                    body: bytes,
                });
            }
            crate::internal::transport::AsyncAttachmentObjectBody::File {
                path,
                len,
                content_type,
            } => {
                self.calls.borrow_mut().push(RecordedCall::PutFile {
                    upload_uri: upload_uri.to_string(),
                    headers,
                    path,
                    len,
                    content_type,
                });
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
enum RecordedCall {
    Rpc {
        endpoint: String,
        method: String,
        params: Value,
    },
    Put {
        upload_uri: String,
        headers: BTreeMap<String, String>,
        body: Vec<u8>,
    },
    PutFile {
        upload_uri: String,
        headers: BTreeMap<String, String>,
        path: PathBuf,
        len: u64,
        content_type: Option<String>,
    },
}

impl RecordedCall {
    fn rpc(&self, expected_method: &str) -> RecordedRpc<'_> {
        match self {
            Self::Rpc {
                endpoint,
                method,
                params,
            } => {
                assert_eq!(method, expected_method);
                RecordedRpc { endpoint, params }
            }
            Self::Put { .. } => panic!("expected rpc call {expected_method}, got put call"),
            Self::PutFile { .. } => {
                panic!("expected rpc call {expected_method}, got file stream put call")
            }
        }
    }

    fn put(&self, expected_uri: &str) -> RecordedPut<'_> {
        match self {
            Self::Put {
                upload_uri,
                headers,
                body,
            } => {
                assert_eq!(upload_uri, expected_uri);
                RecordedPut { headers, body }
            }
            Self::Rpc { method, .. } => panic!("expected put call, got rpc call {method}"),
            Self::PutFile { .. } => panic!("expected put call, got file stream put call"),
        }
    }

    fn put_file(&self, expected_uri: &str) -> RecordedPutFile<'_> {
        match self {
            Self::PutFile {
                upload_uri,
                headers,
                path,
                len,
                content_type,
            } => {
                assert_eq!(upload_uri, expected_uri);
                RecordedPutFile {
                    headers,
                    path,
                    len: *len,
                    content_type,
                }
            }
            Self::Rpc { method, .. } => {
                panic!("expected file stream put call, got rpc call {method}")
            }
            Self::Put { .. } => panic!("expected file stream put call, got bytes put call"),
        }
    }
}

struct RecordedRpc<'a> {
    endpoint: &'a str,
    params: &'a Value,
}

struct RecordedPut<'a> {
    headers: &'a BTreeMap<String, String>,
    body: &'a Vec<u8>,
}

struct RecordedPutFile<'a> {
    headers: &'a BTreeMap<String, String>,
    path: &'a PathBuf,
    len: u64,
    content_type: &'a Option<String>,
}

struct Fixture {
    root: PathBuf,
    service_did: Option<&'static str>,
}

impl Fixture {
    fn new(service_did: Option<&'static str>) -> Self {
        let root = unique_temp_root();
        let identities = root.join("identities");
        fs::create_dir_all(identities.join("alice")).unwrap();
        fs::write(identities.join("default"), "alice\n").unwrap();
        fs::write(
            identities.join("registry.json"),
            r#"{
                  "default_identity": "alice",
                  "identities": [{
                    "id": "alice-id",
                    "did": "did:example:alice",
                    "local_alias": "alice",
                    "ready_for_auth": true,
                    "ready_for_messaging": true,
                    "missing": []
                  }]
                }"#,
        )
        .unwrap();
        Self { root, service_did }
    }

    fn client(&self) -> crate::core::ImClient {
        crate::core::ImCore::new(
            crate::ImCoreConfig {
                service_base_url: crate::ServiceEndpoint::parse("https://example.test").unwrap(),
                did_domain: "awiki.test".to_string(),
                user_service_endpoint: None,
                message_service_endpoint: None,
                mail_service_endpoint: None,
                anp_service_endpoint: None,
                anp_service_did: self
                    .service_did
                    .map(crate::ids::Did::parse)
                    .transpose()
                    .unwrap(),
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
            "alice".to_string(),
        ))
        .unwrap()
    }

    fn credentials(&self) -> AttachmentUploadCredentials {
        let bundle = anp::authentication::create_did_wba_document(
            "awiki.test",
            anp::authentication::DidDocumentOptions {
                path_segments: vec!["user".to_string()],
                domain: Some("awiki.test".to_string()),
                challenge: Some("attachment-upload-runtime-test".to_string()),
                ..anp::authentication::DidDocumentOptions::default()
            },
        )
        .unwrap();
        AttachmentUploadCredentials {
            identity_name: "alice".to_string(),
            key1_private_pem: bundle.private_key_pem("key-1").unwrap().to_string(),
            did_document: Some(bundle.did_document),
        }
    }
}

fn bytes_request(
    input_filename: Option<&str>,
    input_mime: Option<&str>,
    bytes: Vec<u8>,
    request_filename: Option<&str>,
    request_mime: Option<&str>,
    caption: Option<&str>,
) -> crate::attachments::AttachmentSendRequest {
    crate::attachments::AttachmentSendRequest {
        input: crate::attachments::AttachmentInput::Bytes {
            filename: input_filename.map(ToOwned::to_owned),
            mime_type: input_mime.map(ToOwned::to_owned),
            bytes,
        },
        caption: caption.map(ToOwned::to_owned),
        mention_payload: None,
        mime_type: request_mime.map(ToOwned::to_owned),
        filename: request_filename.map(ToOwned::to_owned),
        delivery: crate::messages::MessageDeliveryOptions::default(),
        security: crate::messages::MessageSecurityMode::DefaultPlain,
    }
}

fn unique_temp_root() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "im-core-attachment-upload-runtime-{}-{nanos}",
        std::process::id()
    ))
}
