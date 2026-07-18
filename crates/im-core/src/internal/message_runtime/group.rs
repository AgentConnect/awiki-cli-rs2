use serde_json::Value;

use crate::internal::auth::session::{AsyncSessionProvider, SessionProvider};
use crate::internal::transport::{AsyncAuthenticatedRpcTransport, AuthenticatedRpcTransport};

pub(crate) const MESSAGE_RPC_ENDPOINT: &str = "/im/rpc";

pub(crate) struct GroupTextSender<'a, P, T> {
    client: &'a crate::core::ImClient,
    session_provider: P,
    transport: T,
}

pub(crate) struct GroupTextSend {
    pub request: crate::messages::SendMessageRequest,
    pub credentials: Option<GroupTextCredentials>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GroupTextCredentials {
    pub identity_name: String,
    pub did_document: Option<Value>,
    pub key1_private_pem: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GroupTextSendResult {
    pub sdk_result: crate::messages::SendMessageResult,
    pub group_did: String,
    pub message_type: &'static str,
    pub text: String,
    pub payload: Option<Value>,
    pub raw: Value,
}

impl<'a, P, T> GroupTextSender<'a, P, T>
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

    pub(crate) fn send(mut self, input: GroupTextSend) -> crate::ImResult<GroupTextSendResult> {
        let group = group_target(&input.request.target)?;
        let body = outgoing_body(&input.request.body)?;
        validate_plain_security(&input.request.security)?;

        self.session_provider
            .ensure_session(crate::auth::AuthScope::GroupMessaging)?;

        let message_type = body.message_type();
        let mut payload = build_group_payload(self.client.did().as_str(), group.as_str(), &body)?;
        apply_delivery_overrides(&mut payload.meta, &input.request);
        let credentials = match input.credentials {
            Some(credentials) => credentials,
            None => load_credentials(self.client)?,
        };
        let origin_proof = crate::internal::proof::origin::build_origin_proof(
            &crate::internal::proof::origin::OriginProofIdentity {
                identity_name: credentials.identity_name,
                did_document: credentials.did_document,
                key1_private_pem: credentials.key1_private_pem,
                verification_method: None,
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
        let mut result = group_result_from_value(raw.clone())?;
        fill_group_result_defaults(&mut result, &payload.meta, group.as_str());
        let sdk_result =
            sdk_result_from_group_result(&result, self.client.did().clone(), group.clone(), &body)?;
        Ok(GroupTextSendResult {
            sdk_result,
            group_did: group.as_str().to_string(),
            message_type,
            text: body.text_for_legacy(),
            payload: body.payload_for_result(),
            raw,
        })
    }
}

impl<'a, P, T> GroupTextSender<'a, P, T>
where
    P: AsyncSessionProvider,
    T: AsyncAuthenticatedRpcTransport,
{
    pub(crate) async fn send_async(
        mut self,
        input: GroupTextSend,
    ) -> crate::ImResult<GroupTextSendResult> {
        let group = group_target(&input.request.target)?;
        let body = outgoing_body(&input.request.body)?;
        validate_plain_security(&input.request.security)?;

        self.session_provider
            .ensure_session(crate::auth::AuthScope::GroupMessaging)
            .await?;

        let message_type = body.message_type();
        let mut payload = build_group_payload(self.client.did().as_str(), group.as_str(), &body)?;
        apply_delivery_overrides(&mut payload.meta, &input.request);
        let credentials = match input.credentials {
            Some(credentials) => credentials,
            None => load_credentials_async(self.client).await?,
        };
        let origin_proof = crate::internal::proof::origin::build_origin_proof(
            &crate::internal::proof::origin::OriginProofIdentity {
                identity_name: credentials.identity_name,
                did_document: credentials.did_document,
                key1_private_pem: credentials.key1_private_pem,
                verification_method: None,
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
        let mut result = group_result_from_value(raw.clone())?;
        fill_group_result_defaults(&mut result, &payload.meta, group.as_str());
        let sdk_result =
            sdk_result_from_group_result(&result, self.client.did().clone(), group.clone(), &body)?;
        Ok(GroupTextSendResult {
            sdk_result,
            group_did: group.as_str().to_string(),
            message_type,
            text: body.text_for_legacy(),
            payload: body.payload_for_result(),
            raw,
        })
    }
}

pub(crate) fn load_credentials(
    client: &crate::core::ImClient,
) -> crate::ImResult<GroupTextCredentials> {
    let runtime = client.runtime();
    let did_document = runtime.key_provider.optional_did_document()?;
    let key1_private_pem = runtime.key_provider.device_request_signing_private_pem()?;
    Ok(GroupTextCredentials {
        identity_name: runtime.owner.identity_id.as_str().to_string(),
        did_document,
        key1_private_pem,
    })
}

pub(crate) async fn load_credentials_async(
    client: &crate::core::ImClient,
) -> crate::ImResult<GroupTextCredentials> {
    let runtime = client.runtime();
    let did_document = runtime.key_provider.optional_did_document()?;
    let key1_private_pem = runtime.key_provider.device_request_signing_private_pem()?;
    Ok(GroupTextCredentials {
        identity_name: runtime.owner.identity_id.as_str().to_string(),
        did_document,
        key1_private_pem,
    })
}

pub(crate) fn group_target(
    target: &crate::messages::MessageTarget,
) -> crate::ImResult<crate::ids::GroupRef> {
    let crate::messages::MessageTarget::Group(group) = target else {
        return Err(crate::ImError::unsupported("direct-send"));
    };
    if group.as_str().trim().is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("group".to_string()),
            "group target is required",
        ));
    }
    Ok(group.clone())
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum OutgoingGroupBody {
    Text {
        text: String,
        kind: crate::messages::MessageKind,
    },
    Payload {
        payload: Value,
    },
}

impl OutgoingGroupBody {
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
                crate::internal::message_runtime::state::MessageRetryTarget::GroupText
            }
            Self::Payload { .. } => {
                crate::internal::message_runtime::state::MessageRetryTarget::GroupPayload
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

fn outgoing_body(body: &crate::messages::MessageBody) -> crate::ImResult<OutgoingGroupBody> {
    match body {
        crate::messages::MessageBody::Text { text, kind: _ } if text.trim().is_empty() => {
            Err(crate::ImError::invalid_input(
                Some("text".to_string()),
                "text message must not be empty",
            ))
        }
        crate::messages::MessageBody::Text { text, kind } => Ok(OutgoingGroupBody::Text {
            text: text.clone(),
            kind: kind.clone(),
        }),
        crate::messages::MessageBody::Payload { payload } if !payload.is_object() => {
            Err(crate::ImError::invalid_input(
                Some("payload".to_string()),
                "message payload must be a JSON object",
            ))
        }
        crate::messages::MessageBody::Payload { payload } => Ok(OutgoingGroupBody::Payload {
            payload: payload.clone(),
        }),
        crate::messages::MessageBody::Attachment { .. } => {
            Err(crate::ImError::unsupported("attachments"))
        }
    }
}

pub(crate) fn text_body(
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
        crate::messages::MessageBody::Payload { .. } => {
            Err(crate::ImError::unsupported("group-e2ee-payload"))
        }
        crate::messages::MessageBody::Attachment { .. } => {
            Err(crate::ImError::unsupported("attachments"))
        }
    }
}

fn build_group_payload(
    sender_did: &str,
    group_did: &str,
    body: &OutgoingGroupBody,
) -> crate::ImResult<crate::internal::wire::direct::DirectPayload> {
    match body {
        OutgoingGroupBody::Text { text, kind } => {
            let content_type =
                crate::internal::wire::common::content_type_for_message_kind(kind.clone(), None);
            crate::internal::wire::group::build_group_send_payload(
                sender_did,
                group_did,
                text,
                content_type,
            )
        }
        OutgoingGroupBody::Payload { payload } => {
            crate::internal::wire::group::build_group_json_send_payload(
                sender_did,
                group_did,
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
            Err(crate::ImError::unsupported("group-e2ee"))
        }
        crate::messages::MessageSecurityMode::SecureDirect => {
            Err(crate::ImError::unsupported("secure-direct"))
        }
        crate::messages::MessageSecurityMode::GroupE2ee => {
            Err(crate::ImError::unsupported("group-e2ee"))
        }
    }
}

fn group_result_from_value(value: Value) -> crate::ImResult<GroupRpcResult> {
    serde_json::from_value(value).map_err(|err| crate::ImError::Serialization {
        detail: err.to_string(),
    })
}

fn fill_group_result_defaults(result: &mut GroupRpcResult, meta: &Value, group_did: &str) {
    if result.message_id.is_empty() {
        result.message_id = string_value(meta.get("message_id"));
    }
    if result.operation_id.is_empty() {
        result.operation_id = string_value(meta.get("operation_id"));
    }
    if result.group_did.is_empty() {
        result.group_did = group_did.to_string();
    }
}

fn apply_delivery_overrides(meta: &mut Value, request: &crate::messages::SendMessageRequest) {
    if let Some(message_id) = request.client_message_id.as_ref() {
        meta["message_id"] = Value::String(message_id.as_str().to_string());
    }
    if let Some(idempotency_key) = request.delivery.idempotency_key.as_ref() {
        let value = idempotency_key.trim();
        if !value.is_empty() {
            meta["operation_id"] = Value::String(value.to_string());
        }
    }
}

pub(crate) fn sdk_result_from_group_result(
    result: &GroupRpcResult,
    sender: crate::ids::Did,
    group: crate::ids::GroupRef,
    body: &OutgoingGroupBody,
) -> crate::ImResult<crate::messages::SendMessageResult> {
    let message_id = message_id_from_group_result(group.as_str(), result)?;
    let delivery = delivery_state(result);
    let (send_state, retry_plan) =
        crate::internal::message_runtime::state::send_state_from_delivery(
            &delivery,
            Some(result.operation_id.clone()).filter(|value| !value.trim().is_empty()),
            Some(message_id.clone()),
            Some(result.accepted_at.clone()).filter(|value| !value.trim().is_empty()),
            Some(body.retry_target()),
        );
    let conversation_identity = crate::messages::ConversationIdentity::from_thread_ref(
        &crate::messages::ThreadRef::Group(group.clone()),
    );
    Ok(crate::messages::SendMessageResult {
        message: crate::messages::Message {
            id: message_id,
            thread: crate::messages::ThreadRef::Group(group.clone()),
            direction: crate::messages::MessageDirection::Outgoing,
            sender: crate::ids::PeerRef::parse(sender.as_str(), "")?,
            receiver: None,
            group: Some(group),
            body: body.body_view(),
            sent_at: Some(result.accepted_at.clone()).filter(|value| !value.trim().is_empty()),
            received_at: None,
            metadata: crate::messages::MessageMetadata {
                conversation_identity: Some(conversation_identity),
                operation_id: Some(result.operation_id.clone())
                    .filter(|value| !value.trim().is_empty()),
                delivery_state: Some(
                    crate::internal::message_runtime::state::send_state_label(&send_state.state)
                        .to_string(),
                ),
                send_state: Some(send_state),
                retry_plan,
                server_sequence: result.group_event_seq.trim().parse().ok(),
                content_type: Some(body.content_type().to_string()),
                attributes: metadata_attributes(result),
            },
        },
        delivery,
        warnings: Vec::new(),
    })
}

pub(crate) fn sdk_text_result_from_group_result(
    result: &GroupRpcResult,
    sender: crate::ids::Did,
    group: crate::ids::GroupRef,
    text: &str,
    kind: crate::messages::MessageKind,
) -> crate::ImResult<crate::messages::SendMessageResult> {
    let body = OutgoingGroupBody::Text {
        text: text.to_owned(),
        kind,
    };
    sdk_result_from_group_result(result, sender, group, &body)
}

fn message_id_from_group_result(
    group_did: &str,
    result: &GroupRpcResult,
) -> crate::ImResult<crate::ids::MessageId> {
    if !result.group_did.trim().is_empty() && !result.group_event_seq.trim().is_empty() {
        return crate::ids::MessageId::parse(format!(
            "{}:{}",
            result.group_did.trim(),
            result.group_event_seq.trim()
        ));
    }
    if !result.group_event_seq.trim().is_empty() {
        return crate::ids::MessageId::parse(format!(
            "{}:{}",
            group_did.trim(),
            result.group_event_seq.trim()
        ));
    }
    if !result.message_id.trim().is_empty() {
        return crate::ids::MessageId::parse(&result.message_id);
    }
    crate::ids::MessageId::parse(format!(
        "msg-{}",
        crate::internal::wire::common::generate_operation_id()
    ))
}

fn metadata_attributes(result: &GroupRpcResult) -> Vec<crate::messages::MessageMetadataAttribute> {
    let mut attributes = vec![crate::messages::MessageMetadataAttribute {
        key: "final_acceptance".to_owned(),
        value: result.final_acceptance.to_string(),
    }];
    if !result.message_id.trim().is_empty() {
        attributes.push(crate::messages::MessageMetadataAttribute {
            key: "raw_message_id".to_string(),
            value: result.message_id.clone(),
        });
    }
    if !result.group_event_seq.trim().is_empty() {
        attributes.push(crate::messages::MessageMetadataAttribute {
            key: "group_event_seq".to_string(),
            value: result.group_event_seq.clone(),
        });
    }
    if !result.group_state_version.trim().is_empty() {
        attributes.push(crate::messages::MessageMetadataAttribute {
            key: "group_state_version".to_string(),
            value: result.group_state_version.clone(),
        });
    }
    attributes
}

fn delivery_state(result: &GroupRpcResult) -> crate::messages::DeliveryState {
    if result.accepted || result.final_acceptance {
        crate::messages::DeliveryState::Accepted
    } else {
        crate::messages::DeliveryState::Failed {
            reason: "not accepted".to_string(),
        }
    }
}

pub(crate) fn content_type_for_message_type(message_type: &str) -> &'static str {
    match message_type.trim().to_ascii_lowercase().as_str() {
        "markdown" => "text/markdown",
        _ => "text/plain",
    }
}

pub(crate) fn message_type(kind: &crate::messages::MessageKind) -> &'static str {
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
pub(crate) struct GroupRpcResult {
    #[serde(default)]
    pub accepted: bool,
    #[serde(default)]
    pub final_acceptance: bool,
    #[serde(default)]
    pub group_did: String,
    #[serde(default)]
    pub message_id: String,
    #[serde(default)]
    pub operation_id: String,
    #[serde(default)]
    pub group_event_seq: String,
    #[serde(default)]
    pub group_state_version: String,
    #[serde(default)]
    pub accepted_at: String,
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
    fn messages_group_text_sender_builds_wire_and_maps_result() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let group_did = "did:example:groups:demo";
        let sender = GroupTextSender::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({
                    "accepted": true,
                    "final_acceptance": true,
                    "group_did": group_did,
                    "message_id": "server-message-id",
                    "operation_id": "server-operation-id",
                    "group_event_seq": "42",
                    "group_state_version": "v42",
                    "accepted_at": "2026-05-21T00:00:00Z"
                }),
            },
        );

        let result = sender
            .send(GroupTextSend {
                request: group_text_request(
                    group_did,
                    "hello group",
                    crate::messages::MessageKind::Text,
                ),
                credentials: Some(fixture.credentials()),
            })
            .unwrap();

        assert_eq!(result.group_did, group_did);
        assert_eq!(result.message_type, "text");
        assert_eq!(result.text, "hello group");
        assert_eq!(
            result.sdk_result.message.sender.as_str(),
            "did:example:alice"
        );
        assert_eq!(result.sdk_result.message.receiver, None);
        assert_eq!(
            result.sdk_result.message.group.as_ref().unwrap().as_str(),
            group_did
        );
        assert_eq!(
            result.sdk_result.message.id.as_str(),
            "did:example:groups:demo:42"
        );
        assert_eq!(
            result.sdk_result.message.metadata.operation_id.as_deref(),
            Some("server-operation-id")
        );
        assert_eq!(
            result.sdk_result.message.metadata.delivery_state.as_deref(),
            Some("accepted")
        );
        let send_state = result.sdk_result.message.metadata.send_state.unwrap();
        assert_eq!(
            send_state.state,
            crate::messages::MessageSendStateKind::Accepted
        );
        assert_eq!(
            send_state.operation_id.as_deref(),
            Some("server-operation-id")
        );
        assert_eq!(
            send_state.message_id.unwrap().as_str(),
            result.sdk_result.message.id.as_str()
        );
        assert!(result.sdk_result.message.metadata.retry_plan.is_none());
        assert_eq!(result.sdk_result.message.metadata.server_sequence, Some(42));
        assert!(result
            .sdk_result
            .message
            .metadata
            .attributes
            .iter()
            .any(|attribute| {
                attribute.key == "raw_message_id" && attribute.value == "server-message-id"
            }));
        assert!(result
            .sdk_result
            .message
            .metadata
            .attributes
            .iter()
            .any(|attribute| attribute.key == "final_acceptance" && attribute.value == "true"));
        assert!(matches!(
            result.sdk_result.delivery,
            crate::messages::DeliveryState::Accepted
        ));

        let calls = calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].endpoint, MESSAGE_RPC_ENDPOINT);
        assert_eq!(calls[0].method, "group.send");
        assert_eq!(calls[0].params["meta"]["sender_did"], "did:example:alice");
        assert_eq!(
            calls[0].params["meta"]["target"],
            json!({"kind": "group", "did": group_did})
        );
        assert_eq!(calls[0].params["body"], json!({"text": "hello group"}));
        assert_eq!(
            calls[0].params["auth"]["scheme"],
            crate::internal::proof::origin::ORIGIN_PROOF_SCHEME
        );
    }

    #[test]
    fn messages_group_text_sender_rejects_direct_target() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let sender = GroupTextSender::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::new(RefCell::new(Vec::new())),
                response: json!({}),
            },
        );

        let result = sender.send(GroupTextSend {
            request: direct_text_request("did:example:bob", "hello direct"),
            credentials: Some(fixture.credentials()),
        });

        assert!(matches!(
            result,
            Err(crate::ImError::UnsupportedCapability { capability }) if capability == "direct-send"
        ));
    }

    #[test]
    fn messages_group_payload_sender_builds_body_payload_and_maps_result() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let group_did = "did:example:groups:payload";
        let sender = GroupTextSender::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({
                    "accepted": true,
                    "final_acceptance": true,
                    "group_did": group_did,
                    "message_id": "server-message-id",
                    "operation_id": "op-group-payload",
                    "group_event_seq": "44",
                    "group_state_version": "v44",
                    "accepted_at": "2026-05-21T00:00:00Z"
                }),
            },
        );
        let payload = json!({
            "schema": "awiki.agent.status.v1",
            "state": "running"
        });

        let result = sender
            .send(GroupTextSend {
                request: group_payload_request(group_did, payload.clone()),
                credentials: Some(fixture.credentials()),
            })
            .unwrap();

        assert_eq!(result.group_did, group_did);
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
        assert_eq!(result.sdk_result.message.metadata.server_sequence, Some(44));

        let calls = calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].method, "group.send");
        assert_eq!(calls[0].params["meta"]["content_type"], "application/json");
        assert_eq!(calls[0].params["body"], json!({ "payload": payload }));
    }

    #[tokio::test]
    async fn messages_group_text_sender_async_builds_wire_and_maps_result() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let group_did = "did:example:groups:async";
        let sender = GroupTextSender::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({
                    "accepted": true,
                    "final_acceptance": true,
                    "group_did": group_did,
                    "message_id": "server-message-id",
                    "operation_id": "server-operation-id",
                    "group_event_seq": "43",
                    "group_state_version": "v43",
                    "accepted_at": "2026-05-21T00:00:00Z"
                }),
            },
        );

        let result = sender
            .send_async(GroupTextSend {
                request: group_text_request(
                    group_did,
                    "hello async group",
                    crate::messages::MessageKind::Markdown,
                ),
                credentials: Some(fixture.credentials()),
            })
            .await
            .unwrap();

        assert_eq!(result.group_did, group_did);
        assert_eq!(result.message_type, "markdown");
        assert_eq!(result.text, "hello async group");
        assert_eq!(
            result.sdk_result.message.id.as_str(),
            "did:example:groups:async:43"
        );
        assert_eq!(result.sdk_result.message.metadata.server_sequence, Some(43));
        let calls = calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].endpoint, MESSAGE_RPC_ENDPOINT);
        assert_eq!(calls[0].method, "group.send");
        assert_eq!(calls[0].params["meta"]["sender_did"], "did:example:alice");
        assert_eq!(
            calls[0].params["meta"]["target"],
            json!({"kind": "group", "did": group_did})
        );
        assert_eq!(
            calls[0].params["body"],
            json!({"text": "hello async group"})
        );
    }

    #[derive(Clone)]
    struct ReadySessionProvider;

    impl SessionProvider for ReadySessionProvider {
        fn ensure_session(
            &self,
            scope: crate::auth::AuthScope,
        ) -> crate::ImResult<crate::auth::SessionBundle> {
            assert_eq!(scope, crate::auth::AuthScope::GroupMessaging);
            Ok(crate::auth::SessionBundle {
                subject: crate::ids::Did::parse("did:example:alice")?,
                scope,
                expires_at: None,
                refreshed: false,
                bearer_token: None,
            })
        }

        fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
            unreachable!("group sender should not refresh through the session provider")
        }

        fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
            unreachable!("group sender should not read status")
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

        fn credentials(&self) -> GroupTextCredentials {
            let bundle = anp::authentication::create_did_wba_document(
                "awiki.test",
                anp::authentication::DidDocumentOptions {
                    path_segments: vec!["user".to_string()],
                    domain: Some("awiki.test".to_string()),
                    challenge: Some("group-runtime-test".to_string()),
                    ..anp::authentication::DidDocumentOptions::default()
                },
            )
            .unwrap();
            let key1_private_pem = bundle.private_key_pem("key-1").unwrap().to_string();
            GroupTextCredentials {
                identity_name: "alice".to_string(),
                did_document: Some(bundle.did_document),
                key1_private_pem,
            }
        }
    }

    fn group_text_request(
        group: &str,
        text: &str,
        kind: crate::messages::MessageKind,
    ) -> crate::messages::SendMessageRequest {
        crate::messages::SendMessageRequest {
            target: crate::messages::MessageTarget::Group(
                crate::ids::GroupRef::parse(group).unwrap(),
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

    #[test]
    fn messages_group_sender_uses_client_message_id_as_logical_id() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let group_did = "did:example:groups:client-id";
        let sender = GroupTextSender::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({
                    "accepted": true,
                    "final_acceptance": true,
                    "group_did": group_did,
                    "group_event_seq": "45",
                    "group_state_version": "v45",
                    "accepted_at": "2026-05-21T00:00:00Z"
                }),
            },
        );
        let mut request = group_text_request(
            group_did,
            "hello client id",
            crate::messages::MessageKind::Text,
        );
        request.client_message_id = Some(crate::ids::MessageId::parse("msg-client-group").unwrap());
        request.delivery.idempotency_key = Some("op-client-group".to_owned());

        let result = sender
            .send(GroupTextSend {
                request,
                credentials: Some(fixture.credentials()),
            })
            .unwrap();

        assert_eq!(
            result.sdk_result.message.id.as_str(),
            "did:example:groups:client-id:45"
        );
        assert_eq!(
            result.sdk_result.message.metadata.operation_id.as_deref(),
            Some("op-client-group")
        );
        assert_eq!(result.sdk_result.message.metadata.server_sequence, Some(45));
        let attributes = result.sdk_result.message.metadata.attributes;
        assert!(attributes
            .iter()
            .any(|attribute| { attribute.key == "group_event_seq" && attribute.value == "45" }));
        assert!(attributes.iter().any(|attribute| {
            attribute.key == "raw_message_id" && attribute.value == "msg-client-group"
        }));
        let calls = calls.borrow();
        assert_eq!(calls[0].params["meta"]["message_id"], "msg-client-group");
        assert_eq!(calls[0].params["meta"]["operation_id"], "op-client-group");
    }

    fn group_payload_request(group: &str, payload: Value) -> crate::messages::SendMessageRequest {
        crate::messages::SendMessageRequest {
            target: crate::messages::MessageTarget::Group(
                crate::ids::GroupRef::parse(group).unwrap(),
            ),
            body: crate::messages::MessageBody::Payload { payload },
            security: crate::messages::MessageSecurityMode::Plain,
            client_message_id: None,
            delivery: crate::messages::MessageDeliveryOptions::default(),
            delegated_signing: None,
        }
    }

    fn direct_text_request(peer: &str, text: &str) -> crate::messages::SendMessageRequest {
        crate::messages::SendMessageRequest {
            target: crate::messages::MessageTarget::Direct(
                crate::ids::PeerRef::parse(peer, "").unwrap(),
            ),
            body: crate::messages::MessageBody::Text {
                text: text.to_string(),
                kind: crate::messages::MessageKind::Text,
            },
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
            "im-core-group-runtime-{}-{nanos}",
            std::process::id()
        ))
    }
}
