use serde_json::Value;

use crate::internal::auth::session::SessionProvider;
use crate::internal::transport::AuthenticatedRpcTransport;

pub(crate) const MESSAGE_RPC_ENDPOINT: &str = "/im/rpc";

pub(crate) struct DirectTextSender<'a, P, T> {
    client: &'a crate::core::ImClient,
    session_provider: P,
    transport: T,
}

pub(crate) struct DirectTextSend {
    pub request: crate::messages::SendMessageRequest,
    pub resolved_target_did: Option<String>,
    pub credentials: Option<DirectTextCredentials>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DirectTextCredentials {
    pub identity_name: String,
    pub did_document: Option<Value>,
    pub key1_private_pem: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DirectTextSendResult {
    pub sdk_result: crate::messages::SendMessageResult,
    pub target_did: String,
    pub message_type: &'static str,
    pub text: String,
    pub raw: Value,
}

impl<'a, P, T> DirectTextSender<'a, P, T>
where
    P: SessionProvider,
    T: AuthenticatedRpcTransport,
{
    pub(crate) fn new(
        client: &'a crate::core::ImClient,
        session_provider: P,
        transport: T,
    ) -> Self {
        Self {
            client,
            session_provider,
            transport,
        }
    }

    pub(crate) fn send(mut self, input: DirectTextSend) -> crate::ImResult<DirectTextSendResult> {
        let (peer, target_did) = direct_target(&input.request.target, input.resolved_target_did)?;
        let (text, kind) = text_body(&input.request.body)?;
        validate_plain_security(&input.request.security)?;

        self.session_provider
            .ensure_session(crate::auth::AuthScope::Messaging)?;

        let message_type = message_type(&kind);
        let content_type =
            crate::internal::wire::common::content_type_for_message_kind(kind.clone(), None);
        let payload = crate::internal::wire::direct::build_direct_text_payload(
            self.client.did().as_str(),
            &target_did,
            text,
            content_type,
        )?;
        let credentials = match input.credentials {
            Some(credentials) => credentials,
            None => load_credentials(self.client)?,
        };
        let origin_proof = crate::internal::proof::origin::build_origin_proof(
            &crate::internal::proof::origin::OriginProofIdentity {
                identity_name: credentials.identity_name,
                did_document: credentials.did_document,
                key1_private_pem: credentials.key1_private_pem,
            },
            &payload,
        )?;
        let params = serde_json::json!({
            "meta": payload.meta.clone(),
            "auth": crate::internal::proof::origin::origin_auth_value(&origin_proof),
            "body": payload.body.clone(),
        });
        let raw = self.transport.authenticated_rpc(
            MESSAGE_RPC_ENDPOINT,
            payload.method.as_str(),
            params,
        )?;
        let mut result = direct_result_from_value(raw.clone())?;
        fill_direct_result_defaults(&mut result, &payload.meta, &target_did);
        let sdk_result =
            sdk_result_from_direct_result(&result, self.client.did().clone(), peer, text, kind)?;
        Ok(DirectTextSendResult {
            sdk_result,
            target_did,
            message_type,
            text: text.to_string(),
            raw,
        })
    }
}

fn load_credentials(client: &crate::core::ImClient) -> crate::ImResult<DirectTextCredentials> {
    let runtime = client.runtime();
    let did_document = read_optional_json(&runtime.did_document_path)?;
    let key1_private_pem = std::fs::read_to_string(&runtime.private_key_path).map_err(|err| {
        crate::ImError::CredentialFileUnreadable {
            path_kind: "private_key".to_string(),
            detail: err.to_string(),
        }
    })?;
    Ok(DirectTextCredentials {
        identity_name: runtime.owner.identity_id.as_str().to_string(),
        did_document,
        key1_private_pem,
    })
}

fn read_optional_json(path: &std::path::Path) -> crate::ImResult<Option<Value>> {
    let raw = match std::fs::read(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(crate::ImError::CredentialFileUnreadable {
                path_kind: "did_document".to_string(),
                detail: err.to_string(),
            });
        }
    };
    serde_json::from_slice(&raw)
        .map(Some)
        .map_err(|err| crate::ImError::Serialization {
            detail: err.to_string(),
        })
}

fn direct_target(
    target: &crate::messages::MessageTarget,
    resolved_target_did: Option<String>,
) -> crate::ImResult<(crate::ids::PeerRef, String)> {
    let crate::messages::MessageTarget::Direct(peer) = target else {
        return Err(crate::ImError::unsupported("group-send"));
    };
    let resolved = resolved_target_did
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| peer.as_str().trim());
    if !resolved.starts_with("did:") {
        return Err(crate::ImError::PeerNotFound {
            peer: peer.as_str().to_string(),
        });
    }
    Ok((peer.clone(), resolved.to_string()))
}

fn text_body(
    body: &crate::messages::MessageBody,
) -> crate::ImResult<(&str, crate::messages::MessageKind)> {
    match body {
        crate::messages::MessageBody::Text { text, kind: _ } if text.trim().is_empty() => {
            Err(crate::ImError::invalid_input(
                Some("text".to_string()),
                "text message must not be empty",
            ))
        }
        crate::messages::MessageBody::Text { text, kind } => Ok((text.as_str(), kind.clone())),
        crate::messages::MessageBody::Attachment { .. } => {
            Err(crate::ImError::unsupported("attachments"))
        }
    }
}

fn validate_plain_security(security: &crate::messages::MessageSecurityMode) -> crate::ImResult<()> {
    match security {
        crate::messages::MessageSecurityMode::DefaultPlain
        | crate::messages::MessageSecurityMode::Plain => Ok(()),
        crate::messages::MessageSecurityMode::SecureDirect => {
            Err(crate::ImError::unsupported("secure-direct"))
        }
        crate::messages::MessageSecurityMode::GroupE2ee => {
            Err(crate::ImError::unsupported("group-e2ee"))
        }
    }
}

fn direct_result_from_value(value: Value) -> crate::ImResult<DirectRpcResult> {
    serde_json::from_value(value).map_err(|err| crate::ImError::Serialization {
        detail: err.to_string(),
    })
}

fn fill_direct_result_defaults(result: &mut DirectRpcResult, meta: &Value, target_did: &str) {
    if result.message_id.is_empty() {
        result.message_id = string_value(meta.get("message_id"));
    }
    if result.operation_id.is_empty() {
        result.operation_id = string_value(meta.get("operation_id"));
    }
    if result.target_did.is_empty() {
        result.target_did = target_did.to_string();
    }
}

fn sdk_result_from_direct_result(
    result: &DirectRpcResult,
    sender: crate::ids::Did,
    peer: crate::ids::PeerRef,
    text: &str,
    kind: crate::messages::MessageKind,
) -> crate::ImResult<crate::messages::SendMessageResult> {
    let message_id = crate::ids::MessageId::parse(&result.message_id)?;
    Ok(crate::messages::SendMessageResult {
        message: crate::messages::Message {
            id: message_id,
            thread: crate::messages::ThreadRef::Direct(peer.clone()),
            direction: crate::messages::MessageDirection::Outgoing,
            sender: crate::ids::PeerRef::parse(sender.as_str(), "")?,
            receiver: Some(peer),
            group: None,
            body: crate::messages::MessageBodyView::Text {
                text: text.to_string(),
                kind: kind.clone(),
            },
            sent_at: Some(result.accepted_at.clone()).filter(|value| !value.trim().is_empty()),
            received_at: None,
            metadata: crate::messages::MessageMetadata {
                operation_id: Some(result.operation_id.clone())
                    .filter(|value| !value.trim().is_empty()),
                delivery_state: Some(result.delivery_state.clone())
                    .filter(|value| !value.trim().is_empty()),
                server_sequence: None,
                content_type: Some(content_type_for_message_type(message_type(&kind)).to_string()),
                attributes: Vec::new(),
            },
        },
        delivery: delivery_state(result),
        warnings: Vec::new(),
    })
}

fn delivery_state(result: &DirectRpcResult) -> crate::messages::DeliveryState {
    if !result.delivery_state.trim().is_empty() {
        match result.delivery_state.trim().to_ascii_lowercase().as_str() {
            "sent" => crate::messages::DeliveryState::Sent,
            "stored_locally" | "stored-locally" => crate::messages::DeliveryState::StoredLocally,
            "failed" => crate::messages::DeliveryState::Failed {
                reason: result.delivery_state.clone(),
            },
            _ => crate::messages::DeliveryState::Accepted,
        }
    } else if result.accepted || result.final_acceptance {
        crate::messages::DeliveryState::Accepted
    } else {
        crate::messages::DeliveryState::Failed {
            reason: "not accepted".to_string(),
        }
    }
}

fn content_type_for_message_type(message_type: &str) -> &'static str {
    match message_type.trim().to_ascii_lowercase().as_str() {
        "markdown" => "text/markdown",
        _ => "text/plain",
    }
}

fn message_type(kind: &crate::messages::MessageKind) -> &'static str {
    match kind {
        crate::messages::MessageKind::Text => "text",
        crate::messages::MessageKind::Markdown => "markdown",
    }
}

fn string_value(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize, PartialEq)]
struct DirectRpcResult {
    #[serde(default)]
    accepted: bool,
    #[serde(default)]
    message_id: String,
    #[serde(default)]
    operation_id: String,
    #[serde(default)]
    target_did: String,
    #[serde(default)]
    accepted_at: String,
    #[serde(default)]
    final_acceptance: bool,
    #[serde(default)]
    delivery_state: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::internal::auth::session::SessionProvider;
    use crate::internal::transport::AuthenticatedRpcTransport;
    use serde_json::{json, Value};
    use std::cell::RefCell;
    use std::fs;
    use std::path::PathBuf;
    use std::rc::Rc;

    #[test]
    fn messages_direct_text_sender_builds_wire_and_maps_result() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let sender = DirectTextSender::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({
                    "accepted": true,
                    "accepted_at": "2026-05-21T00:00:00Z",
                    "delivery_state": "accepted"
                }),
            },
        );

        let result = sender
            .send(DirectTextSend {
                request: direct_text_request(
                    "bob.awiki.test",
                    "hello direct",
                    crate::messages::MessageKind::Text,
                ),
                resolved_target_did: Some("did:example:bob".to_string()),
                credentials: Some(fixture.credentials()),
            })
            .unwrap();

        assert_eq!(result.target_did, "did:example:bob");
        assert_eq!(result.message_type, "text");
        assert_eq!(result.text, "hello direct");
        assert_eq!(
            result.sdk_result.message.sender.as_str(),
            "did:example:alice"
        );
        assert_eq!(
            result.sdk_result.message.receiver.unwrap().as_str(),
            "bob.awiki.test"
        );
        assert!(matches!(
            result.sdk_result.delivery,
            crate::messages::DeliveryState::Accepted
        ));
        assert!(result.sdk_result.message.id.as_str().starts_with("msg-"));
        assert!(result
            .sdk_result
            .message
            .metadata
            .operation_id
            .as_deref()
            .unwrap()
            .starts_with("op-"));

        let calls = calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].endpoint, MESSAGE_RPC_ENDPOINT);
        assert_eq!(calls[0].method, "direct.send");
        assert_eq!(calls[0].params["meta"]["sender_did"], "did:example:alice");
        assert_eq!(
            calls[0].params["meta"]["target"],
            json!({"kind": "agent", "did": "did:example:bob"})
        );
        assert_eq!(calls[0].params["body"], json!({"text": "hello direct"}));
        assert_eq!(
            calls[0].params["auth"]["scheme"],
            crate::internal::proof::origin::ORIGIN_PROOF_SCHEME
        );
    }

    #[test]
    fn messages_direct_text_sender_requires_resolved_did_for_handle_peer() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let sender = DirectTextSender::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::new(RefCell::new(Vec::new())),
                response: json!({}),
            },
        );

        let result = sender.send(DirectTextSend {
            request: direct_text_request(
                "bob.awiki.test",
                "hello direct",
                crate::messages::MessageKind::Text,
            ),
            resolved_target_did: None,
            credentials: Some(fixture.credentials()),
        });

        assert!(matches!(
            result,
            Err(crate::ImError::PeerNotFound { peer }) if peer == "bob.awiki.test"
        ));
    }

    #[derive(Clone)]
    struct ReadySessionProvider;

    impl SessionProvider for ReadySessionProvider {
        fn ensure_session(
            &self,
            scope: crate::auth::AuthScope,
        ) -> crate::ImResult<crate::auth::SessionBundle> {
            Ok(crate::auth::SessionBundle {
                subject: crate::ids::Did::parse("did:example:alice")?,
                scope,
                expires_at: None,
                refreshed: false,
            })
        }

        fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
            unreachable!("direct sender should not refresh through the session provider")
        }

        fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
            unreachable!("direct sender should not read status")
        }
    }

    struct RecordingTransport {
        calls: Rc<RefCell<Vec<RecordedCall>>>,
        response: Value,
    }

    impl AuthenticatedRpcTransport for RecordingTransport {
        fn authenticated_rpc(
            &mut self,
            endpoint: &str,
            method: &str,
            params: Value,
        ) -> crate::ImResult<Value> {
            self.calls.borrow_mut().push(RecordedCall {
                endpoint: endpoint.to_string(),
                method: method.to_string(),
                params,
            });
            Ok(self.response.clone())
        }
    }

    struct RecordedCall {
        endpoint: String,
        method: String,
        params: Value,
    }

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let root = unique_temp_root();
            let identities = root.join("identities");
            fs::create_dir_all(&identities).unwrap();
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
            fs::create_dir_all(identities.join("alice")).unwrap();
            Self { root }
        }

        fn client(&self) -> crate::core::ImClient {
            crate::core::ImCore::new(
                crate::ImCoreConfig {
                    service_base_url: crate::ServiceEndpoint::parse("https://example.test")
                        .unwrap(),
                    did_domain: "awiki.test".to_string(),
                    user_service_endpoint: None,
                    message_service_endpoint: None,
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

        fn credentials(&self) -> DirectTextCredentials {
            let bundle = anp::authentication::create_did_wba_document(
                "awiki.test",
                anp::authentication::DidDocumentOptions {
                    path_segments: vec!["user".to_string()],
                    domain: Some("awiki.test".to_string()),
                    challenge: Some("direct-runtime-test".to_string()),
                    ..anp::authentication::DidDocumentOptions::default()
                },
            )
            .unwrap();
            let key1_private_pem = bundle.private_key_pem("key-1").unwrap().to_string();
            DirectTextCredentials {
                identity_name: "alice".to_string(),
                did_document: Some(bundle.did_document),
                key1_private_pem,
            }
        }
    }

    fn direct_text_request(
        peer: &str,
        text: &str,
        kind: crate::messages::MessageKind,
    ) -> crate::messages::SendMessageRequest {
        crate::messages::SendMessageRequest {
            target: crate::messages::MessageTarget::Direct(
                crate::ids::PeerRef::parse(peer, "").unwrap(),
            ),
            body: crate::messages::MessageBody::Text {
                text: text.to_string(),
                kind,
            },
            security: crate::messages::MessageSecurityMode::Plain,
            client_message_id: None,
            delivery: crate::messages::MessageDeliveryOptions::default(),
        }
    }

    fn unique_temp_root() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "im-core-direct-runtime-{}-{nanos}",
            std::process::id()
        ))
    }
}
