use serde_json::Value;

use crate::internal::auth::session::{AsyncSessionProvider, SessionProvider};
use crate::internal::transport::{AsyncAuthenticatedRpcTransport, AuthenticatedRpcTransport};

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
    pub verification_method: Option<String>,
    pub logical_sender_did: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DirectTextSendResult {
    pub sdk_result: crate::messages::SendMessageResult,
    pub target_did: String,
    pub message_type: &'static str,
    pub text: String,
    pub payload: Option<Value>,
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
        let body = outgoing_body(&input.request.body)?;
        validate_plain_security(&input.request.security)?;

        self.session_provider
            .ensure_session(crate::auth::AuthScope::Messaging)?;

        let credentials = match input.credentials {
            Some(credentials) => credentials,
            None => load_credentials(self.client, input.request.delegated_signing.as_ref())?,
        };
        let sender_did = credentials
            .logical_sender_did
            .as_deref()
            .unwrap_or_else(|| self.client.did().as_str());
        let message_type = body.message_type();
        let payload = build_direct_payload(sender_did, &target_did, &body)?;
        let origin_proof = crate::internal::proof::origin::build_origin_proof(
            &crate::internal::proof::origin::OriginProofIdentity {
                identity_name: credentials.identity_name,
                did_document: credentials.did_document,
                key1_private_pem: credentials.key1_private_pem,
                verification_method: credentials.verification_method,
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
        let sdk_result = sdk_result_from_direct_result(&result, sender_did, peer, &body)?;
        Ok(DirectTextSendResult {
            sdk_result,
            target_did,
            message_type,
            text: body.text_for_legacy(),
            payload: body.payload_for_result(),
            raw,
        })
    }
}

impl<'a, P, T> DirectTextSender<'a, P, T>
where
    P: AsyncSessionProvider,
    T: AsyncAuthenticatedRpcTransport,
{
    pub(crate) async fn send_async(
        mut self,
        input: DirectTextSend,
    ) -> crate::ImResult<DirectTextSendResult> {
        let (peer, target_did) = direct_target(&input.request.target, input.resolved_target_did)?;
        let body = outgoing_body(&input.request.body)?;
        validate_plain_security(&input.request.security)?;

        self.session_provider
            .ensure_session(crate::auth::AuthScope::Messaging)
            .await?;

        let credentials = match input.credentials {
            Some(credentials) => credentials,
            None => {
                load_credentials_async(self.client, input.request.delegated_signing.as_ref())
                    .await?
            }
        };
        let sender_did = credentials
            .logical_sender_did
            .as_deref()
            .unwrap_or_else(|| self.client.did().as_str());
        let message_type = body.message_type();
        let payload = build_direct_payload(sender_did, &target_did, &body)?;
        let origin_proof = crate::internal::proof::origin::build_origin_proof(
            &crate::internal::proof::origin::OriginProofIdentity {
                identity_name: credentials.identity_name,
                did_document: credentials.did_document,
                key1_private_pem: credentials.key1_private_pem,
                verification_method: credentials.verification_method,
            },
            &payload,
        )?;
        let params = serde_json::json!({
            "meta": payload.meta.clone(),
            "auth": crate::internal::proof::origin::origin_auth_value(&origin_proof),
            "body": payload.body.clone(),
        });
        let raw = self
            .transport
            .authenticated_rpc(MESSAGE_RPC_ENDPOINT, payload.method.as_str(), params)
            .await?;
        let mut result = direct_result_from_value(raw.clone())?;
        fill_direct_result_defaults(&mut result, &payload.meta, &target_did);
        let sdk_result = sdk_result_from_direct_result(&result, sender_did, peer, &body)?;
        Ok(DirectTextSendResult {
            sdk_result,
            target_did,
            message_type,
            text: body.text_for_legacy(),
            payload: body.payload_for_result(),
            raw,
        })
    }
}

fn load_credentials(
    client: &crate::core::ImClient,
    delegated: Option<&crate::messages::DelegatedSigningOptions>,
) -> crate::ImResult<DirectTextCredentials> {
    let runtime = client.runtime();
    let did_document = read_optional_json(&runtime.did_document_path)?;
    if let Some(delegated) = delegated {
        return delegated_credentials(client, delegated, did_document);
    }
    let key1_private_pem = read_default_private_key(client)?;
    Ok(DirectTextCredentials {
        identity_name: runtime.owner.identity_id.as_str().to_string(),
        did_document,
        key1_private_pem,
        verification_method: None,
        logical_sender_did: None,
    })
}

async fn load_credentials_async(
    client: &crate::core::ImClient,
    delegated: Option<&crate::messages::DelegatedSigningOptions>,
) -> crate::ImResult<DirectTextCredentials> {
    let runtime = client.runtime();
    let did_document = read_optional_json_async(runtime.did_document_path.clone()).await?;
    if let Some(delegated) = delegated {
        return delegated_credentials_async(client, delegated, did_document).await;
    }
    let key1_private_pem = tokio::fs::read_to_string(&runtime.private_key_path)
        .await
        .map_err(|err| crate::ImError::CredentialFileUnreadable {
            path_kind: "private_key".to_string(),
            detail: err.to_string(),
        })?;
    Ok(DirectTextCredentials {
        identity_name: runtime.owner.identity_id.as_str().to_string(),
        did_document,
        key1_private_pem,
        verification_method: None,
        logical_sender_did: None,
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

async fn read_optional_json_async(path: std::path::PathBuf) -> crate::ImResult<Option<Value>> {
    let raw = match tokio::fs::read(path).await {
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

fn read_default_private_key(client: &crate::core::ImClient) -> crate::ImResult<String> {
    std::fs::read_to_string(&client.runtime().private_key_path).map_err(|err| {
        crate::ImError::CredentialFileUnreadable {
            path_kind: "private_key".to_string(),
            detail: err.to_string(),
        }
    })
}

fn delegated_credentials(
    client: &crate::core::ImClient,
    delegated: &crate::messages::DelegatedSigningOptions,
    current_did_document: Option<Value>,
) -> crate::ImResult<DirectTextCredentials> {
    let owner = required_delegated_field(
        delegated.logical_sender_did.as_deref(),
        "logical_sender_did",
    )?;
    let method = required_delegated_field(
        delegated.signing_verification_method.as_deref(),
        "signing_verification_method",
    )?;
    let key_ref =
        required_delegated_field(delegated.signing_key_ref.as_deref(), "signing_key_ref")?;
    crate::internal::delegated_identity::require_method_owner(
        &owner,
        &method,
        "logical_sender_did",
        "signing_verification_method",
    )?;
    let did_document = crate::internal::delegated_identity::load_did_document_for_owner(
        client,
        &owner,
        current_did_document,
    )?;
    crate::internal::delegated_identity::require_authentication_method(
        &did_document,
        &method,
        "signing_verification_method",
    )?;
    let key1_private_pem =
        crate::internal::delegated_identity::load_private_key_ref(client, &key_ref)?;
    Ok(DirectTextCredentials {
        identity_name: format!(
            "{}:delegated:{}",
            client.current_identity().id.as_str(),
            method
        ),
        did_document: Some(did_document),
        key1_private_pem,
        verification_method: Some(method),
        logical_sender_did: Some(owner),
    })
}

async fn delegated_credentials_async(
    client: &crate::core::ImClient,
    delegated: &crate::messages::DelegatedSigningOptions,
    current_did_document: Option<Value>,
) -> crate::ImResult<DirectTextCredentials> {
    let owner = required_delegated_field(
        delegated.logical_sender_did.as_deref(),
        "logical_sender_did",
    )?;
    let method = required_delegated_field(
        delegated.signing_verification_method.as_deref(),
        "signing_verification_method",
    )?;
    let key_ref =
        required_delegated_field(delegated.signing_key_ref.as_deref(), "signing_key_ref")?;
    crate::internal::delegated_identity::require_method_owner(
        &owner,
        &method,
        "logical_sender_did",
        "signing_verification_method",
    )?;
    let did_document = crate::internal::delegated_identity::load_did_document_for_owner_async(
        client,
        &owner,
        current_did_document,
    )
    .await?;
    crate::internal::delegated_identity::require_authentication_method(
        &did_document,
        &method,
        "signing_verification_method",
    )?;
    let key1_private_pem =
        crate::internal::delegated_identity::load_private_key_ref_async(client, &key_ref).await?;
    Ok(DirectTextCredentials {
        identity_name: format!(
            "{}:delegated:{}",
            client.current_identity().id.as_str(),
            method
        ),
        did_document: Some(did_document),
        key1_private_pem,
        verification_method: Some(method),
        logical_sender_did: Some(owner),
    })
}

fn required_delegated_field(value: Option<&str>, field: &str) -> crate::ImResult<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| {
            crate::ImError::invalid_input(
                Some(field.to_owned()),
                format!("{field} is required when delegated signing is set"),
            )
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

#[derive(Debug, Clone, PartialEq)]
enum OutgoingDirectBody {
    Text {
        text: String,
        kind: crate::messages::MessageKind,
    },
    Payload {
        payload: Value,
    },
}

impl OutgoingDirectBody {
    fn message_type(&self) -> &'static str {
        match self {
            Self::Text { kind, .. } => message_type(kind),
            Self::Payload { .. } => "payload",
        }
    }

    fn content_type(&self) -> &'static str {
        match self {
            Self::Text { kind, .. } => content_type_for_message_type(message_type(kind)),
            Self::Payload { .. } => "application/json",
        }
    }

    fn retry_target(&self) -> crate::internal::message_runtime::state::MessageRetryTarget {
        match self {
            Self::Text { .. } => {
                crate::internal::message_runtime::state::MessageRetryTarget::DirectText
            }
            Self::Payload { .. } => {
                crate::internal::message_runtime::state::MessageRetryTarget::DirectPayload
            }
        }
    }

    fn body_view(&self) -> crate::messages::MessageBodyView {
        match self {
            Self::Text { text, kind } => crate::messages::MessageBodyView::Text {
                text: text.clone(),
                kind: kind.clone(),
            },
            Self::Payload { payload } => crate::messages::MessageBodyView::Payload {
                payload: payload.clone(),
            },
        }
    }

    fn text_for_legacy(&self) -> String {
        match self {
            Self::Text { text, .. } => text.clone(),
            Self::Payload { .. } => String::new(),
        }
    }

    fn payload_for_result(&self) -> Option<Value> {
        match self {
            Self::Text { .. } => None,
            Self::Payload { payload } => Some(payload.clone()),
        }
    }
}

fn outgoing_body(body: &crate::messages::MessageBody) -> crate::ImResult<OutgoingDirectBody> {
    match body {
        crate::messages::MessageBody::Text { text, kind: _ } if text.trim().is_empty() => {
            Err(crate::ImError::invalid_input(
                Some("text".to_string()),
                "text message must not be empty",
            ))
        }
        crate::messages::MessageBody::Text { text, kind } => Ok(OutgoingDirectBody::Text {
            text: text.clone(),
            kind: kind.clone(),
        }),
        crate::messages::MessageBody::Payload { payload } if !payload.is_object() => {
            Err(crate::ImError::invalid_input(
                Some("payload".to_string()),
                "message payload must be a JSON object",
            ))
        }
        crate::messages::MessageBody::Payload { payload } => Ok(OutgoingDirectBody::Payload {
            payload: payload.clone(),
        }),
        crate::messages::MessageBody::Attachment { .. } => {
            Err(crate::ImError::unsupported("attachments"))
        }
    }
}

fn build_direct_payload(
    sender_did: &str,
    target_did: &str,
    body: &OutgoingDirectBody,
) -> crate::ImResult<crate::internal::wire::direct::DirectPayload> {
    match body {
        OutgoingDirectBody::Text { text, kind } => {
            let content_type =
                crate::internal::wire::common::content_type_for_message_kind(kind.clone(), None);
            crate::internal::wire::direct::build_direct_text_payload(
                sender_did,
                target_did,
                text,
                content_type,
            )
        }
        OutgoingDirectBody::Payload { payload } => {
            crate::internal::wire::direct::build_direct_json_payload(
                sender_did,
                target_did,
                payload.clone(),
            )
        }
    }
}

fn validate_plain_security(security: &crate::messages::MessageSecurityMode) -> crate::ImResult<()> {
    match security {
        crate::messages::MessageSecurityMode::DefaultPlain
        | crate::messages::MessageSecurityMode::Plain => Ok(()),
        crate::messages::MessageSecurityMode::E2eeRequired => {
            Err(crate::ImError::unsupported("secure-direct"))
        }
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
    sender_did: &str,
    peer: crate::ids::PeerRef,
    body: &OutgoingDirectBody,
) -> crate::ImResult<crate::messages::SendMessageResult> {
    let message_id = crate::ids::MessageId::parse(&result.message_id)?;
    let delivery = delivery_state(result);
    let (send_state, retry_plan) =
        crate::internal::message_runtime::state::send_state_from_delivery(
            &delivery,
            Some(result.operation_id.clone()).filter(|value| !value.trim().is_empty()),
            Some(message_id.clone()),
            Some(result.accepted_at.clone()).filter(|value| !value.trim().is_empty()),
            Some(body.retry_target()),
        );
    let delivery_state = Some(result.delivery_state.clone())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            crate::internal::message_runtime::state::send_state_label(&send_state.state).to_string()
        });
    let attributes = resolved_target_attributes(&result.target_did, &peer);
    Ok(crate::messages::SendMessageResult {
        message: crate::messages::Message {
            id: message_id,
            thread: crate::messages::ThreadRef::Direct(peer.clone()),
            direction: crate::messages::MessageDirection::Outgoing,
            sender: crate::ids::PeerRef::parse(sender_did, "")?,
            receiver: Some(peer),
            group: None,
            body: body.body_view(),
            sent_at: Some(result.accepted_at.clone()).filter(|value| !value.trim().is_empty()),
            received_at: None,
            metadata: crate::messages::MessageMetadata {
                operation_id: Some(result.operation_id.clone())
                    .filter(|value| !value.trim().is_empty()),
                delivery_state: Some(delivery_state),
                send_state: Some(send_state),
                retry_plan,
                server_sequence: None,
                content_type: Some(body.content_type().to_string()),
                attributes,
            },
        },
        delivery,
        warnings: Vec::new(),
    })
}

fn resolved_target_attributes(
    target_did: &str,
    peer: &crate::ids::PeerRef,
) -> Vec<crate::messages::MessageMetadataAttribute> {
    if target_did.trim().is_empty() || target_did.trim() == peer.as_str().trim() {
        return Vec::new();
    }
    vec![crate::messages::MessageMetadataAttribute {
        key: "resolved_target_did".to_string(),
        value: target_did.to_string(),
    }]
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
        let send_state = result.sdk_result.message.metadata.send_state.unwrap();
        assert_eq!(
            send_state.state,
            crate::messages::MessageSendStateKind::Accepted
        );
        assert!(send_state
            .operation_id
            .as_deref()
            .unwrap()
            .starts_with("op-"));
        assert!(send_state.message_id.unwrap().as_str().starts_with("msg-"));
        assert!(result.sdk_result.message.metadata.retry_plan.is_none());

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
    fn message_state_direct_failed_result_has_retry_plan() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let sender = DirectTextSender::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::new(RefCell::new(Vec::new())),
                response: json!({
                    "accepted": false,
                    "message_id": "msg-failed",
                    "operation_id": "op-failed",
                    "target_did": "did:example:bob",
                    "delivery_state": "failed"
                }),
            },
        );

        let result = sender
            .send(DirectTextSend {
                request: direct_text_request(
                    "did:example:bob",
                    "hello direct",
                    crate::messages::MessageKind::Text,
                ),
                resolved_target_did: None,
                credentials: Some(fixture.credentials()),
            })
            .unwrap();

        assert!(matches!(
            result.sdk_result.delivery,
            crate::messages::DeliveryState::Failed { .. }
        ));
        let send_state = result.sdk_result.message.metadata.send_state.unwrap();
        assert_eq!(
            send_state.state,
            crate::messages::MessageSendStateKind::Failed
        );
        assert_eq!(send_state.operation_id.as_deref(), Some("op-failed"));
        assert_eq!(send_state.message_id.unwrap().as_str(), "msg-failed");
        let retry_plan = result.sdk_result.message.metadata.retry_plan.unwrap();
        assert!(retry_plan.retryable);
        assert_eq!(
            retry_plan.action,
            crate::messages::MessageRetryAction::RetryDirectText
        );
        assert_eq!(retry_plan.operation_id.as_deref(), Some("op-failed"));
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

    #[test]
    fn messages_direct_payload_sender_builds_body_payload_and_maps_result() {
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
                    "message_id": "msg-payload-direct",
                    "operation_id": "op-payload-direct",
                    "target_did": "did:example:bob",
                    "accepted_at": "2026-05-21T00:00:00Z",
                    "delivery_state": "accepted"
                }),
            },
        );
        let payload = json!({
            "schema": "awiki.agent.command.v1",
            "command": "runtime.agent.create"
        });

        let result = sender
            .send(DirectTextSend {
                request: direct_payload_request("did:example:bob", payload.clone()),
                resolved_target_did: None,
                credentials: Some(fixture.credentials()),
            })
            .unwrap();

        assert_eq!(result.target_did, "did:example:bob");
        assert_eq!(result.message_type, "payload");
        assert!(result.text.is_empty());
        assert_eq!(result.payload, Some(payload.clone()));
        assert_eq!(
            result.sdk_result.message.body,
            crate::messages::MessageBodyView::Payload {
                payload: payload.clone()
            }
        );
        assert_eq!(
            result.sdk_result.message.metadata.content_type.as_deref(),
            Some("application/json")
        );

        let calls = calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].method, "direct.send");
        assert_eq!(calls[0].params["meta"]["content_type"], "application/json");
        assert_eq!(calls[0].params["body"], json!({ "payload": payload }));
    }

    #[tokio::test]
    async fn messages_direct_text_sender_async_builds_wire_and_maps_result() {
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
                    "message_id": "msg-async-direct",
                    "operation_id": "op-async-direct",
                    "target_did": "did:example:bob",
                    "accepted_at": "2026-05-21T00:00:00Z",
                    "delivery_state": "accepted"
                }),
            },
        );

        let result = sender
            .send_async(DirectTextSend {
                request: direct_text_request(
                    "bob.awiki.test",
                    "hello async direct",
                    crate::messages::MessageKind::Markdown,
                ),
                resolved_target_did: Some("did:example:bob".to_string()),
                credentials: Some(fixture.credentials()),
            })
            .await
            .unwrap();

        assert_eq!(result.target_did, "did:example:bob");
        assert_eq!(result.message_type, "markdown");
        assert_eq!(result.text, "hello async direct");
        assert_eq!(result.sdk_result.message.id.as_str(), "msg-async-direct");
        let calls = calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].endpoint, MESSAGE_RPC_ENDPOINT);
        assert_eq!(calls[0].method, "direct.send");
        assert_eq!(calls[0].params["meta"]["sender_did"], "did:example:alice");
        assert_eq!(
            calls[0].params["meta"]["target"],
            json!({"kind": "agent", "did": "did:example:bob"})
        );
        assert_eq!(
            calls[0].params["body"],
            json!({"text": "hello async direct"})
        );
    }

    #[test]
    fn messages_direct_text_sender_uses_delegated_signing_options() {
        let fixture = Fixture::new();
        let delegated = fixture.write_delegated_identity();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let sender = DirectTextSender::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({
                    "accepted": true,
                    "message_id": "msg-delegated-direct",
                    "operation_id": "op-delegated-direct",
                    "target_did": "did:example:bob",
                    "accepted_at": "2026-05-21T00:00:00Z",
                    "delivery_state": "accepted"
                }),
            },
        );
        let mut request = direct_text_request(
            "did:example:bob",
            "hello delegated",
            crate::messages::MessageKind::Text,
        );
        request.delegated_signing = Some(crate::messages::DelegatedSigningOptions {
            logical_sender_did: Some(delegated.user_did.clone()),
            signing_verification_method: Some(delegated.verification_method.clone()),
            signing_key_ref: Some(format!("file:{}", delegated.private_key_path.display())),
            actor_agent_did: Some("did:example:agent:daemon".to_owned()),
        });

        let result = sender
            .send(DirectTextSend {
                request,
                resolved_target_did: None,
                credentials: None,
            })
            .unwrap();

        assert_eq!(
            result.sdk_result.message.sender.as_str(),
            delegated.user_did
        );
        let calls = calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].params["meta"]["sender_did"], delegated.user_did);
        assert!(calls[0].params["auth"]["origin_proof"]["signatureInput"]
            .as_str()
            .expect("signature input")
            .contains(&format!("keyid=\"{}\"", delegated.verification_method)));
    }

    #[test]
    fn messages_direct_text_sender_rejects_wrong_delegated_owner_locally() {
        let fixture = Fixture::new();
        let delegated = fixture.write_delegated_identity();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let sender = DirectTextSender::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({}),
            },
        );
        let mut request = direct_text_request(
            "did:example:bob",
            "hello delegated",
            crate::messages::MessageKind::Text,
        );
        request.delegated_signing = Some(crate::messages::DelegatedSigningOptions {
            logical_sender_did: Some("did:example:other".to_owned()),
            signing_verification_method: Some(delegated.verification_method),
            signing_key_ref: Some(format!("file:{}", delegated.private_key_path.display())),
            actor_agent_did: None,
        });

        let error = sender
            .send(DirectTextSend {
                request,
                resolved_target_did: None,
                credentials: None,
            })
            .unwrap_err();

        assert!(matches!(
            error,
            crate::ImError::InvalidInput {
                field: Some(field),
                ..
            } if field == "logical_sender_did"
        ));
        assert!(calls.borrow().is_empty());
    }

    #[test]
    fn messages_direct_text_sender_rejects_missing_delegated_key_locally() {
        let fixture = Fixture::new();
        let delegated = fixture.write_delegated_identity();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let sender = DirectTextSender::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({}),
            },
        );
        let mut request = direct_text_request(
            "did:example:bob",
            "hello delegated",
            crate::messages::MessageKind::Text,
        );
        request.delegated_signing = Some(crate::messages::DelegatedSigningOptions {
            logical_sender_did: Some(delegated.user_did),
            signing_verification_method: Some(delegated.verification_method),
            signing_key_ref: Some("local:missing-daemon-key".to_owned()),
            actor_agent_did: None,
        });

        let error = sender
            .send(DirectTextSend {
                request,
                resolved_target_did: None,
                credentials: None,
            })
            .unwrap_err();

        assert!(matches!(
            error,
            crate::ImError::CredentialFileUnreadable { path_kind, .. }
                if path_kind == "delegated_private_key"
        ));
        assert!(calls.borrow().is_empty());
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
                bearer_token: None,
            })
        }

        fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
            unreachable!("direct sender should not refresh through the session provider")
        }

        fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
            unreachable!("direct sender should not read status")
        }
    }

    impl crate::internal::auth::session::AsyncSessionProvider for ReadySessionProvider {
        async fn ensure_session(
            &self,
            scope: crate::auth::AuthScope,
        ) -> crate::ImResult<crate::auth::SessionBundle> {
            SessionProvider::ensure_session(self, scope)
        }

        async fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
            SessionProvider::refresh_session(self)
        }

        async fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
            SessionProvider::status(self)
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

    struct RecordedCall {
        endpoint: String,
        method: String,
        params: Value,
    }

    struct Fixture {
        root: PathBuf,
    }

    struct DelegatedIdentityFixture {
        user_did: String,
        verification_method: String,
        private_key_path: PathBuf,
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
                verification_method: None,
                logical_sender_did: None,
            }
        }

        fn write_delegated_identity(&self) -> DelegatedIdentityFixture {
            let bundle = anp::authentication::create_did_wba_document(
                "awiki.test",
                anp::authentication::DidDocumentOptions {
                    path_segments: vec!["user".to_string()],
                    domain: Some("awiki.test".to_string()),
                    challenge: Some("direct-delegated-test".to_string()),
                    ..anp::authentication::DidDocumentOptions::default()
                },
            )
            .unwrap();
            let user_did = bundle.did().unwrap().to_string();
            let delegated_private_key = bundle.private_key_pem("key-1").unwrap().to_string();
            let verification_method = format!("{user_did}#daemon-key-1");
            let mut did_document = bundle.did_document;
            let mut delegated_method = did_document["verificationMethod"][0].clone();
            delegated_method["id"] = json!(verification_method);
            did_document["verificationMethod"]
                .as_array_mut()
                .unwrap()
                .push(delegated_method);
            did_document["authentication"]
                .as_array_mut()
                .unwrap()
                .push(json!(verification_method));
            let identity_dir = self.root.join("identities").join("alice");
            fs::write(
                identity_dir.join("did.json"),
                serde_json::to_vec_pretty(&did_document).unwrap(),
            )
            .unwrap();
            let private_key_path = identity_dir.join("daemon-key-1.pem");
            fs::write(&private_key_path, delegated_private_key).unwrap();
            fs::write(
                self.root.join("identities").join("registry.json"),
                json!({
                    "default_identity": "alice",
                    "identities": [{
                        "id": "alice-id",
                        "did": user_did,
                        "local_alias": "alice",
                        "ready_for_auth": true,
                        "ready_for_messaging": true,
                        "missing": []
                    }]
                })
                .to_string(),
            )
            .unwrap();
            DelegatedIdentityFixture {
                user_did,
                verification_method,
                private_key_path,
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
            delegated_signing: None,
        }
    }

    fn direct_payload_request(peer: &str, payload: Value) -> crate::messages::SendMessageRequest {
        crate::messages::SendMessageRequest {
            target: crate::messages::MessageTarget::Direct(
                crate::ids::PeerRef::parse(peer, "").unwrap(),
            ),
            body: crate::messages::MessageBody::Payload { payload },
            security: crate::messages::MessageSecurityMode::Plain,
            client_message_id: None,
            delivery: crate::messages::MessageDeliveryOptions::default(),
            delegated_signing: None,
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
