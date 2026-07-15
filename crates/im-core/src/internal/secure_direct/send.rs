use serde_json::{Map, Value};

#[cfg(any(feature = "blocking", test))]
use crate::internal::auth::session::SessionProvider;
#[cfg(any(feature = "blocking", test))]
use crate::internal::transport::{AuthenticatedRpcTransport, RpcTransport};

#[cfg(any(feature = "blocking", test))]
use super::client::{prepare_direct_secure_client, MessageServiceDirectSecureClient};

pub(crate) const MESSAGE_RPC_ENDPOINT: &str = "/im/rpc";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DirectSecureTextSend {
    pub(crate) request: crate::messages::SendMessageRequest,
    pub(crate) resolved_target_did: Option<String>,
    pub(crate) target_handle: Option<String>,
    pub(crate) peer_scope: Option<crate::internal::local_state::owner_scope::DirectPeerScope>,
    pub(crate) local_persistence: DirectSecureLocalPersistence,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DirectSecureAttachmentSend {
    pub(crate) request: crate::messages::SendMessageRequest,
    pub(crate) resolved_target_did: Option<String>,
    pub(crate) target_handle: Option<String>,
    pub(crate) peer_scope: Option<crate::internal::local_state::owner_scope::DirectPeerScope>,
    pub(crate) committed: crate::internal::attachment_runtime::upload::PreparedCommittedAttachment,
    pub(crate) local_persistence: DirectSecureLocalPersistence,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DirectSecureTextSendResult {
    pub(crate) sdk_result: crate::messages::SendMessageResult,
    pub(crate) queued_outbox_id: Option<String>,
    pub(crate) target_did: String,
    pub(crate) target_handle: Option<String>,
    pub(crate) peer_scope: Option<crate::internal::local_state::owner_scope::DirectPeerScope>,
    pub(crate) text: String,
    pub(crate) kind: crate::messages::MessageKind,
    pub(crate) raw: Option<Value>,
    pub(crate) local_effect: DirectSecureLocalEffect,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DirectSecureAttachmentSendResult {
    pub(crate) sdk_result: crate::messages::SendMessageResult,
    pub(crate) target_did: String,
    pub(crate) target_handle: Option<String>,
    pub(crate) peer_scope: Option<crate::internal::local_state::owner_scope::DirectPeerScope>,
    pub(crate) redacted_manifest: Value,
    pub(crate) raw: Option<Value>,
    pub(crate) local_effect: DirectSecureAttachmentLocalEffect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DirectSecureLocalPersistence {
    LegacySqlite,
    Deferred,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DirectSecureLocalEffect {
    None,
    PersistOutgoing,
    QueueOutbox(crate::internal::store::e2ee_outbox::E2eeOutboxRecord),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DirectSecureAttachmentLocalEffect {
    None,
    PersistOutgoing,
}

#[cfg(any(feature = "blocking", test))]
pub(crate) struct DirectSecureTextSender<'a, P, A, R> {
    client: &'a crate::core::ImClient,
    session_provider: P,
    message_transport: A,
    directory_transport: R,
}

#[cfg(any(feature = "blocking", test))]
impl<'a, P, A, R> DirectSecureTextSender<'a, P, A, R>
where
    P: SessionProvider,
    A: AuthenticatedRpcTransport + 'a,
    R: RpcTransport + 'a,
{
    pub(crate) fn new(
        client: &'a crate::core::ImClient,
        session_provider: P,
        message_transport: A,
        directory_transport: R,
    ) -> Self {
        Self {
            client,
            session_provider,
            message_transport,
            directory_transport,
        }
    }

    pub(crate) fn send(
        self,
        input: DirectSecureTextSend,
    ) -> crate::ImResult<DirectSecureTextSendResult> {
        let (peer, target_did) = direct_target(&input.request.target, input.resolved_target_did)?;
        let (text, kind) = text_body(&input.request.body)?;
        validate_secure_direct_security(&input.request.security)?;
        self.session_provider
            .ensure_session(crate::auth::AuthScope::Messaging)?;

        let identity_material = super::identity_material::local_identity_material(self.client)?;
        let connection = crate::internal::local_state::open_writable(
            &self.client.core_inner().sdk_paths().local_state.sqlite_path,
        )?;

        let operation_id = input
            .request
            .delivery
            .idempotency_key
            .clone()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                input
                    .request
                    .client_message_id
                    .as_ref()
                    .map(|message_id| message_id.as_str().to_owned())
            })
            .unwrap_or_else(generate_message_id);
        let message_id = input
            .request
            .client_message_id
            .as_ref()
            .map(|message_id| message_id.as_str().to_owned())
            .unwrap_or_else(|| operation_id.clone());
        if operation_id != message_id {
            return Err(crate::ImError::invalid_input(
                Some("delivery.idempotency_key".to_owned()),
                "direct E2EE requires delivery.idempotency_key to match client_message_id",
            ));
        }

        let mut message_transport = self.message_transport;
        let rpc = Box::new(move |method: &str, params: Map<String, Value>| {
            let value = message_transport.authenticated_rpc(
                MESSAGE_RPC_ENDPOINT,
                method,
                Value::Object(params),
            )?;
            object_result(value)
        });
        let mut directory_transport = self.directory_transport;
        let local_document_for_resolver = identity_material.local_did_document.clone();
        let owner_did = self.client.did().as_str().to_owned();
        let identity_paths = self.client.core_inner().sdk_paths().identities.clone();
        let resolver = Box::new(move |did: &str| {
            if did == owner_did {
                return Ok(local_document_for_resolver.clone());
            }
            match resolve_did_document_with_transport(&mut directory_transport, did) {
                Ok(document) => Ok(document),
                Err(err) => {
                    match crate::internal::identity_document_cache::load_local_did_document(
                        &identity_paths,
                        did,
                    ) {
                        Ok(Some(document)) => Ok(document),
                        Ok(None) | Err(_) => Err(err),
                    }
                }
            }
        });
        let mut direct_client = MessageServiceDirectSecureClient::new(
            prepare_direct_secure_client(identity_material.client_input(&connection))?,
            rpc,
            resolver,
        );

        let warnings = publish_prekeys_warnings(&mut direct_client);
        match direct_client.send_text(&target_did, text, &operation_id, &message_id) {
            Ok(raw) => {
                let mut sdk_result = sdk_result_from_secure_result(
                    &raw,
                    self.client.did().clone(),
                    peer,
                    &target_did,
                    text,
                    kind.clone(),
                    warnings,
                )?;
                crate::messages::normalize_direct_send_result_for_peer_scope(
                    &mut sdk_result,
                    input.peer_scope.as_ref(),
                    input.target_handle.as_deref(),
                    Some(target_did.as_str()),
                )?;
                let local_effect = match input.local_persistence {
                    DirectSecureLocalPersistence::LegacySqlite => {
                        match crate::internal::message_runtime::local_projection::persist_direct_e2ee_outgoing(
                            &connection,
                            self.client,
                            &target_did,
                            input.target_handle.as_deref(),
                            input.peer_scope.as_ref(),
                            text,
                            &kind,
                            &sdk_result,
                        ) {
                            Ok(()) => self
                                .client
                                .emit_committed_local_message_projection("local_send"),
                            Err(err) => {
                                sdk_result.warnings.push(format!(
                                    "Failed to persist local secure direct message: {err}"
                                ));
                            }
                        }
                        DirectSecureLocalEffect::None
                    }
                    DirectSecureLocalPersistence::Deferred => {
                        DirectSecureLocalEffect::PersistOutgoing
                    }
                };
                let text = text.to_owned();
                Ok(DirectSecureTextSendResult {
                    sdk_result,
                    queued_outbox_id: None,
                    target_did,
                    target_handle: input.target_handle,
                    peer_scope: input.peer_scope,
                    text,
                    kind,
                    raw: Some(Value::Object(raw)),
                    local_effect,
                })
            }
            Err(err) if super::control::is_pending_confirmation_error(Some(&err.to_string())) => {
                let record = pending_confirmation_outbox_record(
                    self.client,
                    &target_did,
                    &kind,
                    text,
                    &err.to_string(),
                );
                if input.local_persistence == DirectSecureLocalPersistence::LegacySqlite {
                    queue_pending_confirmation_outbox(&connection, record.clone())?;
                }
                let outbox_id = record.outbox_id.clone();
                let mut sdk_result = queued_sdk_result(
                    &outbox_id,
                    self.client.did().clone(),
                    peer,
                    &target_did,
                    text,
                    kind.clone(),
                    operation_id,
                    message_id,
                    warnings,
                )?;
                crate::messages::normalize_direct_send_result_for_peer_scope(
                    &mut sdk_result,
                    input.peer_scope.as_ref(),
                    input.target_handle.as_deref(),
                    Some(target_did.as_str()),
                )?;
                let text = text.to_owned();
                let local_effect = match input.local_persistence {
                    DirectSecureLocalPersistence::LegacySqlite => DirectSecureLocalEffect::None,
                    DirectSecureLocalPersistence::Deferred => {
                        DirectSecureLocalEffect::QueueOutbox(record)
                    }
                };
                Ok(DirectSecureTextSendResult {
                    sdk_result,
                    queued_outbox_id: Some(outbox_id),
                    target_did,
                    target_handle: input.target_handle,
                    peer_scope: input.peer_scope,
                    text,
                    kind,
                    raw: None,
                    local_effect,
                })
            }
            Err(err) => Err(err),
        }
    }

    pub(crate) fn send_attachment(
        self,
        input: DirectSecureAttachmentSend,
    ) -> crate::ImResult<DirectSecureAttachmentSendResult> {
        let (peer, target_did) = direct_target(&input.request.target, input.resolved_target_did)?;
        validate_secure_direct_security(&input.request.security)?;
        self.session_provider
            .ensure_session(crate::auth::AuthScope::Messaging)?;

        let identity_material = super::identity_material::local_identity_material(self.client)?;
        let connection = crate::internal::local_state::open_writable(
            &self.client.core_inner().sdk_paths().local_state.sqlite_path,
        )?;

        let operation_id = input
            .request
            .delivery
            .idempotency_key
            .clone()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                input
                    .request
                    .client_message_id
                    .as_ref()
                    .map(|message_id| message_id.as_str().to_owned())
            })
            .unwrap_or_else(generate_message_id);
        let message_id = input
            .request
            .client_message_id
            .as_ref()
            .map(|message_id| message_id.as_str().to_owned())
            .unwrap_or_else(|| operation_id.clone());
        if operation_id != message_id {
            return Err(crate::ImError::invalid_input(
                Some("delivery.idempotency_key".to_owned()),
                "direct E2EE requires delivery.idempotency_key to match client_message_id",
            ));
        }

        let mut message_transport = self.message_transport;
        let rpc = Box::new(move |method: &str, params: Map<String, Value>| {
            let value = message_transport.authenticated_rpc(
                MESSAGE_RPC_ENDPOINT,
                method,
                Value::Object(params),
            )?;
            object_result(value)
        });
        let mut directory_transport = self.directory_transport;
        let local_document_for_resolver = identity_material.local_did_document.clone();
        let owner_did = self.client.did().as_str().to_owned();
        let identity_paths = self.client.core_inner().sdk_paths().identities.clone();
        let resolver = Box::new(move |did: &str| {
            if did == owner_did {
                return Ok(local_document_for_resolver.clone());
            }
            match resolve_did_document_with_transport(&mut directory_transport, did) {
                Ok(document) => Ok(document),
                Err(err) => {
                    match crate::internal::identity_document_cache::load_local_did_document(
                        &identity_paths,
                        did,
                    ) {
                        Ok(Some(document)) => Ok(document),
                        Ok(None) | Err(_) => Err(err),
                    }
                }
            }
        });
        let mut direct_client = MessageServiceDirectSecureClient::new(
            prepare_direct_secure_client(identity_material.client_input(&connection))?,
            rpc,
            resolver,
        );

        let warnings = publish_prekeys_warnings(&mut direct_client);
        let client_context = attachment_client_context(input.committed.grant_ref.clone());
        match direct_client.send_json_with_client_context(
            &target_did,
            crate::attachments::manifest::attachment_manifest_content_type(),
            input.committed.full_manifest,
            &operation_id,
            &message_id,
            Some(client_context),
        ) {
            Ok(raw) => {
                let mut sdk_result = sdk_result_from_secure_attachment_result(
                    &raw,
                    self.client.did().clone(),
                    peer,
                    &target_did,
                    &input.committed.redacted_manifest,
                    warnings,
                )?;
                crate::messages::normalize_direct_send_result_for_peer_scope(
                    &mut sdk_result,
                    input.peer_scope.as_ref(),
                    input.target_handle.as_deref(),
                    Some(target_did.as_str()),
                )?;
                let local_effect = match input.local_persistence {
                    DirectSecureLocalPersistence::LegacySqlite => {
                        match crate::internal::message_runtime::local_projection::persist_direct_e2ee_attachment_outgoing(
                            &connection,
                            self.client,
                            &target_did,
                            input.target_handle.as_deref(),
                            input.peer_scope.as_ref(),
                            &input.committed.redacted_manifest,
                            &sdk_result,
                        ) {
                            Ok(()) => self
                                .client
                                .emit_committed_local_message_projection("local_send"),
                            Err(err) => {
                                sdk_result.warnings.push(format!(
                                    "Failed to persist local secure direct attachment: {err}"
                                ));
                            }
                        }
                        DirectSecureAttachmentLocalEffect::None
                    }
                    DirectSecureLocalPersistence::Deferred => {
                        DirectSecureAttachmentLocalEffect::PersistOutgoing
                    }
                };
                Ok(DirectSecureAttachmentSendResult {
                    sdk_result,
                    target_did,
                    target_handle: input.target_handle,
                    peer_scope: input.peer_scope,
                    redacted_manifest: input.committed.redacted_manifest,
                    raw: Some(Value::Object(raw)),
                    local_effect,
                })
            }
            Err(err) if super::control::is_pending_confirmation_error(Some(&err.to_string())) => {
                Err(crate::ImError::LocalStateUnavailable {
                    detail: "direct E2EE attachment send cannot be queued pending peer confirmation without persisting attachment keys".to_owned(),
                })
            }
            Err(err) => Err(err),
        }
    }
}

#[cfg(any(feature = "blocking", test))]
fn publish_prekeys_warnings(client: &mut MessageServiceDirectSecureClient<'_>) -> Vec<String> {
    match client.publish_prekey_bundle() {
        Ok(_) => Vec::new(),
        Err(err) => vec![format!("Failed to publish secure prekeys: {err}")],
    }
}

pub(crate) fn direct_target(
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
            peer: peer.as_str().to_owned(),
        });
    }
    Ok((peer.clone(), resolved.to_owned()))
}

pub(crate) fn text_body(
    body: &crate::messages::MessageBody,
) -> crate::ImResult<(&str, crate::messages::MessageKind)> {
    match body {
        crate::messages::MessageBody::Text { text, .. } if text.trim().is_empty() => {
            Err(crate::ImError::invalid_input(
                Some("text".to_owned()),
                "text message must not be empty",
            ))
        }
        crate::messages::MessageBody::Text { text, kind } => Ok((text.as_str(), kind.clone())),
        crate::messages::MessageBody::Payload { .. } => {
            Err(crate::ImError::unsupported("secure-direct-payload"))
        }
        crate::messages::MessageBody::Attachment { .. } => {
            Err(crate::ImError::unsupported("attachments"))
        }
    }
}

pub(crate) fn validate_secure_direct_security(
    security: &crate::messages::MessageSecurityMode,
) -> crate::ImResult<()> {
    match security {
        crate::messages::MessageSecurityMode::E2eeRequired
        | crate::messages::MessageSecurityMode::SecureDirect => Ok(()),
        crate::messages::MessageSecurityMode::DefaultPlain
        | crate::messages::MessageSecurityMode::Plain => {
            Err(crate::ImError::unsupported("plaintext-direct-secure-send"))
        }
        crate::messages::MessageSecurityMode::GroupE2ee => {
            Err(crate::ImError::unsupported("group-e2ee"))
        }
    }
}

pub(crate) fn object_result(value: Value) -> crate::ImResult<Map<String, Value>> {
    match value {
        Value::Object(object) => Ok(object),
        Value::Null => Ok(Map::new()),
        other => Err(crate::ImError::Serialization {
            detail: format!("expected object RPC result, got {other}"),
        }),
    }
}

#[cfg(any(feature = "blocking", test))]
fn resolve_did_document_with_transport(
    transport: &mut impl RpcTransport,
    did: &str,
) -> crate::ImResult<Value> {
    let call = crate::internal::identity_wire::profile::build_profile_resolve_rpc_call(did)?;
    let raw = transport.rpc(call.endpoint, call.method, call.params)?;
    did_document_from_resolve(raw).ok_or_else(|| crate::ImError::PeerNotFound {
        peer: did.to_owned(),
    })
}

pub(crate) fn did_document_from_resolve(value: Value) -> Option<Value> {
    if looks_like_did_document(&value) {
        return Some(value);
    }
    for pointer in [
        "/did_document",
        "/didDocument",
        "/document",
        "/profile/did_document",
        "/profile/didDocument",
        "/result/did_document",
        "/result/didDocument",
    ] {
        let candidate = value.pointer(pointer)?;
        if looks_like_did_document(candidate) {
            return Some(candidate.clone());
        }
    }
    None
}

pub(crate) fn looks_like_did_document(value: &Value) -> bool {
    value
        .get("id")
        .and_then(Value::as_str)
        .is_some_and(|value| value.starts_with("did:"))
        && value.get("verificationMethod").is_some()
}

pub(crate) fn sdk_result_from_secure_result(
    raw: &Map<String, Value>,
    sender: crate::ids::Did,
    peer: crate::ids::PeerRef,
    target_did: &str,
    text: &str,
    kind: crate::messages::MessageKind,
    warnings: Vec<String>,
) -> crate::ImResult<crate::messages::SendMessageResult> {
    let message_id = crate::ids::MessageId::parse(default_string(
        &string_value(raw.get("message_id")),
        &generate_message_id(),
    ))?;
    let operation_id = string_value(raw.get("operation_id"));
    let accepted_at = string_value(raw.get("accepted_at"));
    let delivery_state = string_value(raw.get("delivery_state"));
    let delivery = delivery_state_from_result(raw);
    let (send_state, retry_plan) =
        crate::internal::message_runtime::state::send_state_from_delivery(
            &delivery,
            optional_string(&operation_id),
            Some(message_id.clone()),
            optional_string(&accepted_at),
            Some(crate::internal::message_runtime::state::MessageRetryTarget::DirectText),
        );
    let attributes =
        secure_direct_attributes(target_did, &peer, vec![final_acceptance_attribute(raw)]);
    let conversation_identity = crate::messages::ConversationIdentity::from_thread_ref(
        &crate::messages::ThreadRef::Direct(peer.clone()),
    );
    Ok(crate::messages::SendMessageResult {
        message: crate::messages::Message {
            id: message_id,
            thread: crate::messages::ThreadRef::Direct(peer.clone()),
            direction: crate::messages::MessageDirection::Outgoing,
            sender: crate::ids::PeerRef::parse(sender.as_str(), "")?,
            receiver: Some(peer),
            group: None,
            body: crate::messages::MessageBodyView::Text {
                text: text.to_owned(),
                kind: kind.clone(),
            },
            sent_at: optional_string(&accepted_at),
            received_at: None,
            metadata: crate::messages::MessageMetadata {
                conversation_identity: Some(conversation_identity),
                operation_id: optional_string(&operation_id),
                delivery_state: optional_string(&delivery_state).or_else(|| {
                    Some(
                        crate::internal::message_runtime::state::send_state_label(
                            &send_state.state,
                        )
                        .to_owned(),
                    )
                }),
                send_state: Some(send_state),
                retry_plan,
                server_sequence: raw
                    .get("server_seq")
                    .or_else(|| raw.get("server_sequence"))
                    .and_then(Value::as_i64),
                content_type: Some(content_type_for_kind(&kind).to_owned()),
                attributes,
            },
        },
        delivery,
        warnings,
    })
}

pub(crate) fn sdk_result_from_secure_attachment_result(
    raw: &Map<String, Value>,
    sender: crate::ids::Did,
    peer: crate::ids::PeerRef,
    target_did: &str,
    redacted_manifest: &Value,
    warnings: Vec<String>,
) -> crate::ImResult<crate::messages::SendMessageResult> {
    let message_id = crate::ids::MessageId::parse(default_string(
        &string_value(raw.get("message_id")),
        &generate_message_id(),
    ))?;
    let operation_id = string_value(raw.get("operation_id"));
    let accepted_at = string_value(raw.get("accepted_at"));
    let delivery_state = string_value(raw.get("delivery_state"));
    let delivery = delivery_state_from_result(raw);
    let (send_state, retry_plan) =
        crate::internal::message_runtime::state::send_state_from_delivery(
            &delivery,
            optional_string(&operation_id),
            Some(message_id.clone()),
            optional_string(&accepted_at),
            None,
        );
    let attributes = secure_direct_attributes(
        target_did,
        &peer,
        vec![
            final_acceptance_attribute(raw),
            crate::messages::MessageMetadataAttribute {
                key: "attachment_manifest".to_owned(),
                value: crate::attachments::manifest::manifest_content_string(redacted_manifest),
            },
        ],
    );
    let conversation_identity = crate::messages::ConversationIdentity::from_thread_ref(
        &crate::messages::ThreadRef::Direct(peer.clone()),
    );
    Ok(crate::messages::SendMessageResult {
        message: crate::messages::Message {
            id: message_id,
            thread: crate::messages::ThreadRef::Direct(peer.clone()),
            direction: crate::messages::MessageDirection::Outgoing,
            sender: crate::ids::PeerRef::parse(sender.as_str(), "")?,
            receiver: Some(peer),
            group: None,
            body: crate::messages::MessageBodyView::Unsupported {
                content_type: Some(
                    crate::attachments::manifest::attachment_manifest_content_type().to_owned(),
                ),
            },
            sent_at: optional_string(&accepted_at),
            received_at: None,
            metadata: crate::messages::MessageMetadata {
                conversation_identity: Some(conversation_identity),
                operation_id: optional_string(&operation_id),
                delivery_state: optional_string(&delivery_state).or_else(|| {
                    Some(
                        crate::internal::message_runtime::state::send_state_label(
                            &send_state.state,
                        )
                        .to_owned(),
                    )
                }),
                send_state: Some(send_state),
                retry_plan,
                server_sequence: raw
                    .get("server_seq")
                    .or_else(|| raw.get("server_sequence"))
                    .and_then(Value::as_i64),
                content_type: Some(
                    crate::attachments::manifest::attachment_manifest_content_type().to_owned(),
                ),
                attributes,
            },
        },
        delivery,
        warnings,
    })
}

fn final_acceptance_attribute(
    raw: &Map<String, Value>,
) -> crate::messages::MessageMetadataAttribute {
    crate::messages::MessageMetadataAttribute {
        key: "final_acceptance".to_owned(),
        value: raw
            .get("final_acceptance")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            .to_string(),
    }
}

pub(crate) fn attachment_client_context(grant_ref: Value) -> Value {
    Value::Object(Map::from_iter([(
        "attachment_grant_refs".to_owned(),
        Value::Array(vec![grant_ref]),
    )]))
}

pub(crate) fn queued_sdk_result(
    outbox_id: &str,
    sender: crate::ids::Did,
    peer: crate::ids::PeerRef,
    target_did: &str,
    text: &str,
    kind: crate::messages::MessageKind,
    operation_id: String,
    message_id: String,
    mut warnings: Vec<String>,
) -> crate::ImResult<crate::messages::SendMessageResult> {
    warnings.push("secure direct message queued pending peer confirmation".to_owned());
    let message_id = crate::ids::MessageId::parse(message_id)?;
    let delivery = crate::messages::DeliveryState::StoredLocally;
    let (send_state, retry_plan) =
        crate::internal::message_runtime::state::send_state_from_delivery(
            &delivery,
            Some(operation_id.clone()),
            Some(message_id.clone()),
            None,
            Some(crate::internal::message_runtime::state::MessageRetryTarget::DirectText),
        );
    let attributes = secure_direct_attributes(
        target_did,
        &peer,
        vec![crate::messages::MessageMetadataAttribute {
            key: "secure_outbox_id".to_owned(),
            value: outbox_id.to_owned(),
        }],
    );
    let conversation_identity = crate::messages::ConversationIdentity::from_thread_ref(
        &crate::messages::ThreadRef::Direct(peer.clone()),
    );
    Ok(crate::messages::SendMessageResult {
        message: crate::messages::Message {
            id: message_id,
            thread: crate::messages::ThreadRef::Direct(peer.clone()),
            direction: crate::messages::MessageDirection::Outgoing,
            sender: crate::ids::PeerRef::parse(sender.as_str(), "")?,
            receiver: Some(peer),
            group: None,
            body: crate::messages::MessageBodyView::Text {
                text: text.to_owned(),
                kind: kind.clone(),
            },
            sent_at: None,
            received_at: None,
            metadata: crate::messages::MessageMetadata {
                conversation_identity: Some(conversation_identity),
                operation_id: Some(operation_id),
                delivery_state: Some("queued".to_owned()),
                send_state: Some(send_state),
                retry_plan,
                server_sequence: None,
                content_type: Some(content_type_for_kind(&kind).to_owned()),
                attributes,
            },
        },
        delivery,
        warnings,
    })
}

fn secure_direct_attributes(
    target_did: &str,
    peer: &crate::ids::PeerRef,
    mut extra: Vec<crate::messages::MessageMetadataAttribute>,
) -> Vec<crate::messages::MessageMetadataAttribute> {
    let mut attributes = vec![crate::messages::MessageMetadataAttribute {
        key: "security".to_owned(),
        value: "direct-e2ee".to_owned(),
    }];
    if !target_did.trim().is_empty() && target_did.trim() != peer.as_str().trim() {
        attributes.push(crate::messages::MessageMetadataAttribute {
            key: "resolved_target_did".to_owned(),
            value: target_did.to_owned(),
        });
    }
    attributes.append(&mut extra);
    attributes
}

#[cfg(any(feature = "blocking", test))]
fn queue_pending_confirmation_outbox(
    connection: &rusqlite::Connection,
    record: crate::internal::store::e2ee_outbox::E2eeOutboxRecord,
) -> crate::ImResult<String> {
    crate::internal::store::e2ee_outbox::queue_e2ee_outbox(connection, record)
}

pub(crate) fn pending_confirmation_outbox_record(
    client: &crate::core::ImClient,
    target_did: &str,
    kind: &crate::messages::MessageKind,
    plaintext: &str,
    error: &str,
) -> crate::internal::store::e2ee_outbox::E2eeOutboxRecord {
    let scope = crate::internal::store::e2ee_outbox::E2eeOutboxOwnerScope::for_client(client);
    crate::internal::store::e2ee_outbox::E2eeOutboxRecord {
        outbox_id: generate_outbox_id(),
        owner_identity_id: scope.owner_identity_id,
        owner_did: scope.owner_did,
        credential_name: scope.credential_name,
        peer_did: target_did.to_owned(),
        original_type: message_type_for_kind(kind).to_owned(),
        plaintext: plaintext.to_owned(),
        local_status: "queued".to_owned(),
        last_error_code: error.to_owned(),
        retry_hint: "retry".to_owned(),
        ..Default::default()
    }
}

fn delivery_state_from_result(raw: &Map<String, Value>) -> crate::messages::DeliveryState {
    let delivery_state = string_value(raw.get("delivery_state"));
    if !delivery_state.is_empty() {
        match delivery_state.to_ascii_lowercase().as_str() {
            "sent" => crate::messages::DeliveryState::Sent,
            "stored_locally" | "stored-locally" | "queued" => {
                crate::messages::DeliveryState::StoredLocally
            }
            "failed" => crate::messages::DeliveryState::Failed {
                reason: delivery_state,
            },
            _ => crate::messages::DeliveryState::Accepted,
        }
    } else if bool_value(raw.get("accepted")) || bool_value(raw.get("final_acceptance")) {
        crate::messages::DeliveryState::Accepted
    } else {
        crate::messages::DeliveryState::Failed {
            reason: "not accepted".to_owned(),
        }
    }
}

pub(crate) fn content_type_for_kind(kind: &crate::messages::MessageKind) -> &'static str {
    match kind {
        crate::messages::MessageKind::Markdown => "text/markdown",
        crate::messages::MessageKind::Text => "text/plain",
    }
}

fn message_type_for_kind(kind: &crate::messages::MessageKind) -> &'static str {
    match kind {
        crate::messages::MessageKind::Markdown => "markdown",
        crate::messages::MessageKind::Text => "text",
    }
}

fn bool_value(value: Option<&Value>) -> bool {
    value.and_then(Value::as_bool).unwrap_or(false)
}

fn string_value(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) => value.trim().to_owned(),
        Some(Value::Number(value)) => value.to_string(),
        Some(Value::Bool(value)) => value.to_string(),
        _ => String::new(),
    }
}

fn optional_string(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn default_string<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    let value = value.trim();
    if value.is_empty() {
        fallback
    } else {
        value
    }
}

pub(crate) fn generate_message_id() -> String {
    let mut bytes = [0_u8; 16];
    use rand::RngCore as _;
    rand::thread_rng().fill_bytes(&mut bytes);
    format!(
        "msg-{}",
        bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}

fn generate_outbox_id() -> String {
    let mut bytes = [0_u8; 16];
    use rand::RngCore as _;
    rand::thread_rng().fill_bytes(&mut bytes);
    format!(
        "outbox-{}",
        bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::rc::Rc;

    use serde_json::{json, Value};

    use super::*;

    #[test]
    fn secure_direct_sender_uses_direct_e2ee_rpc_and_queues_plaintext_only_on_pending_confirmation()
    {
        let alice = TestIdentity::new("alice.example", "alice");
        let bob = TestIdentity::new("bob.example", "bob");
        let fixture = Fixture::new(&alice);
        let client = fixture.client();
        let bob_bundle = test_prekey_bundle(&bob);
        let message_calls = Rc::new(RefCell::new(Vec::<RecordedCall>::new()));
        let sender = DirectSecureTextSender::new(
            &client,
            ReadySessionProvider,
            RecordingAuthenticatedTransport {
                calls: message_calls.clone(),
                responses: Rc::new(RefCell::new(vec![
                    json!({}),
                    json!({
                        "prekey_bundle": bob_bundle.bundle,
                        "one_time_prekey": bob_bundle.one_time_prekey,
                    }),
                    json!({
                        "accepted": true,
                        "message_id": "msg-secure-direct",
                        "operation_id": "msg-secure-direct",
                        "target_did": bob.did,
                        "accepted_at": "2026-05-24T00:00:00Z",
                        "delivery_state": "accepted"
                    }),
                ])),
            },
            StaticDirectoryTransport {
                documents: vec![(bob.did.clone(), bob.document.clone())],
            },
        );

        let result = sender
            .send(DirectSecureTextSend {
                request: secure_direct_request(&bob.did, "secret text"),
                resolved_target_did: None,
                target_handle: None,
                peer_scope: None,
                local_persistence: DirectSecureLocalPersistence::LegacySqlite,
            })
            .unwrap();

        assert_eq!(result.queued_outbox_id, None);
        assert!(matches!(
            result.sdk_result.delivery,
            crate::messages::DeliveryState::Accepted
        ));
        assert_eq!(
            result.sdk_result.message.metadata.attributes,
            vec![
                crate::messages::MessageMetadataAttribute {
                    key: "security".to_owned(),
                    value: "direct-e2ee".to_owned(),
                },
                crate::messages::MessageMetadataAttribute {
                    key: "final_acceptance".to_owned(),
                    value: "false".to_owned(),
                },
            ]
        );
        let calls = message_calls.borrow();
        assert_eq!(calls[0].method, "direct.e2ee.publish_prekey_bundle");
        assert_eq!(calls[1].method, "direct.e2ee.get_prekey_bundle");
        assert_eq!(calls[2].method, "direct.send");
        assert_eq!(
            calls[2].params["meta"]["content_type"],
            "application/anp-direct-init+json"
        );
        assert!(!calls[2].params["body"].to_string().contains("secret text"));

        let stored = rusqlite::Connection::open(fixture.root.join("local").join("im.sqlite"))
            .unwrap()
            .query_row(
                r#"
SELECT thread_id, content, is_e2ee, metadata
FROM messages
WHERE owner_did = ?1 AND msg_id = ?2"#,
                rusqlite::params![alice.did, "msg-secure-direct"],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .unwrap();
        assert!(stored.0.starts_with("dm:"));
        assert_eq!(stored.1, "secret text");
        assert_eq!(stored.2, 1);
        let metadata: Value = serde_json::from_str(&stored.3).unwrap();
        assert_eq!(metadata["security"], "direct-e2ee");
        assert_eq!(metadata["contains_sensitive"], false);
        let metadata_text = stored.3;
        for forbidden in [
            "cipher",
            "prekey",
            "session_id",
            "state_blob",
            "private_key",
            "ratchet",
        ] {
            assert!(
                !metadata_text.contains(forbidden),
                "metadata leaked {forbidden}: {metadata_text}"
            );
        }
    }

    #[test]
    fn secure_attachment_send_direct_sender_encrypts_manifest_and_sends_non_secret_grant_ref() {
        let alice = TestIdentity::new("alice.example", "alice");
        let bob = TestIdentity::new("bob.example", "bob");
        let fixture = Fixture::new(&alice);
        let client = fixture.client();
        let bob_bundle = test_prekey_bundle(&bob);
        let committed = committed_attachment("direct-secret.pdf", "direct caption");
        let object_key = committed.full_manifest["attachments"][0]["encryption_info"]
            ["object_key_b64u"]
            .as_str()
            .unwrap()
            .to_owned();
        let nonce = committed.full_manifest["attachments"][0]["encryption_info"]["nonce_b64u"]
            .as_str()
            .unwrap()
            .to_owned();
        let message_calls = Rc::new(RefCell::new(Vec::<RecordedCall>::new()));
        let sender = DirectSecureTextSender::new(
            &client,
            ReadySessionProvider,
            RecordingAuthenticatedTransport {
                calls: message_calls.clone(),
                responses: Rc::new(RefCell::new(vec![
                    json!({}),
                    json!({
                        "prekey_bundle": bob_bundle.bundle,
                        "one_time_prekey": bob_bundle.one_time_prekey,
                    }),
                    json!({
                        "accepted": true,
                        "message_id": "msg-secure-attachment",
                        "operation_id": "msg-secure-attachment",
                        "target_did": bob.did,
                        "accepted_at": "2026-05-24T00:00:00Z",
                        "delivery_state": "accepted"
                    }),
                ])),
            },
            StaticDirectoryTransport {
                documents: vec![(bob.did.clone(), bob.document.clone())],
            },
        );

        let result = sender
            .send_attachment(DirectSecureAttachmentSend {
                request: secure_direct_attachment_request(&bob.did),
                resolved_target_did: None,
                target_handle: None,
                peer_scope: None,
                committed,
                local_persistence: DirectSecureLocalPersistence::LegacySqlite,
            })
            .unwrap();

        assert!(matches!(
            result.sdk_result.delivery,
            crate::messages::DeliveryState::Accepted
        ));
        assert!(matches!(
            result.sdk_result.message.body,
            crate::messages::MessageBodyView::Unsupported { content_type }
                if content_type.as_deref()
                    == Some(crate::attachments::manifest::attachment_manifest_content_type())
        ));

        let calls = message_calls.borrow();
        assert_eq!(calls[2].method, "direct.send");
        assert!(
            !calls[2].params["body"].to_string().contains(&object_key),
            "direct E2EE outer body leaked object key"
        );
        assert!(
            !calls[2].params["body"].to_string().contains(&nonce),
            "direct E2EE outer body leaked nonce"
        );
        let grant_refs = calls[2].params["client"]["attachment_grant_refs"]
            .as_array()
            .unwrap();
        assert_eq!(grant_refs.len(), 1);
        assert_eq!(grant_refs[0]["attachment_id"], "att-secure-1");
        assert_eq!(grant_refs[0]["object_encryption_mode"], "object-e2ee");
        assert_eq!(grant_refs[0]["plaintext_size"], "24");
        let grant_text = serde_json::to_string(&calls[2].params["client"]).unwrap();
        assert!(!grant_text.contains("object_key_b64u"));
        assert!(!grant_text.contains("nonce_b64u"));
        assert!(!grant_text.contains(&object_key));
        assert!(!grant_text.contains(&nonce));

        let stored = rusqlite::Connection::open(fixture.root.join("local").join("im.sqlite"))
            .unwrap()
            .query_row(
                r#"
SELECT content, is_e2ee, metadata
FROM messages
WHERE owner_did = ?1 AND msg_id = ?2"#,
                rusqlite::params![alice.did, "msg-secure-attachment"],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(stored.1, 1);
        assert!(stored.0.contains("object-e2ee"));
        assert!(!stored.0.contains("object_key_b64u"));
        assert!(!stored.0.contains("nonce_b64u"));
        assert!(!stored.0.contains(&object_key));
        assert!(!stored.0.contains(&nonce));
        assert!(!stored.2.contains("object_key_b64u"));
        assert!(!stored.2.contains("nonce_b64u"));
        assert!(!stored.2.contains(&object_key));
        assert!(!stored.2.contains(&nonce));
    }

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
            unreachable!("secure direct sender should not refresh in this unit test")
        }

        fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
            unreachable!("secure direct sender should not read auth status")
        }
    }

    #[derive(Clone)]
    struct RecordedCall {
        method: String,
        params: Value,
    }

    struct RecordingAuthenticatedTransport {
        calls: Rc<RefCell<Vec<RecordedCall>>>,
        responses: Rc<RefCell<Vec<Value>>>,
    }

    impl AuthenticatedRpcTransport for RecordingAuthenticatedTransport {
        fn authenticated_rpc(
            &mut self,
            _endpoint: &str,
            method: &str,
            params: Value,
        ) -> crate::ImResult<Value> {
            self.calls.borrow_mut().push(RecordedCall {
                method: method.to_owned(),
                params,
            });
            Ok(self.responses.borrow_mut().remove(0))
        }
    }

    struct StaticDirectoryTransport {
        documents: Vec<(String, Value)>,
    }

    impl RpcTransport for StaticDirectoryTransport {
        fn rpc(&mut self, _endpoint: &str, _method: &str, params: Value) -> crate::ImResult<Value> {
            let did = params
                .get("did")
                .or_else(|| params.get("target_did"))
                .and_then(Value::as_str)
                .or_else(|| params.pointer("/body/did").and_then(Value::as_str))
                .unwrap_or_default();
            self.documents
                .iter()
                .find(|(candidate, _)| candidate == did)
                .map(|(_, document)| json!({ "did_document": document }))
                .ok_or_else(|| crate::ImError::PeerNotFound {
                    peer: did.to_owned(),
                })
        }
    }

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new(identity: &TestIdentity) -> Self {
            let root = unique_temp_root("im-core-secure-direct-send");
            write_identity_fixture(&root, "alice", identity);
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
                        sqlite_path: self.root.join("local").join("im.sqlite"),
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
    }

    struct TestIdentity {
        did: String,
        document: Value,
        key1_private_pem: String,
        agreement_private_pem: String,
    }

    impl TestIdentity {
        fn new(domain: &str, label: &str) -> Self {
            let service = anp::authentication::build_agent_message_service_with_options(
                "#message",
                format!("https://{domain}/anp-im/rpc"),
                anp::authentication::AnpMessageServiceOptions::default()
                    .with_service_did(format!("did:wba:{domain}")),
            );
            let bundle = anp::authentication::create_did_wba_document(
                domain,
                anp::authentication::DidDocumentOptions {
                    path_segments: vec!["agents".to_owned(), label.to_owned()],
                    domain: Some(domain.to_owned()),
                    challenge: Some(format!("secure-direct-send-{label}")),
                    services: vec![service],
                    did_profile: anp::authentication::DidProfile::E1,
                    ..Default::default()
                },
            )
            .unwrap();
            let did = bundle.did().unwrap().to_owned();
            Self {
                did,
                document: bundle.did_document.clone(),
                key1_private_pem: bundle.private_key_pem("key-1").unwrap().to_owned(),
                agreement_private_pem: bundle.private_key_pem("key-3").unwrap().to_owned(),
            }
        }
    }

    struct TestPrekeyBundle {
        bundle: anp::direct_e2ee::PrekeyBundle,
        one_time_prekey: anp::direct_e2ee::OneTimePrekey,
    }

    fn test_prekey_bundle(identity: &TestIdentity) -> TestPrekeyBundle {
        let signing_private =
            anp::PrivateKeyMaterial::from_pem(&identity.key1_private_pem).unwrap();
        let agreement_private =
            anp::PrivateKeyMaterial::from_pem(&identity.agreement_private_pem).unwrap();
        let signed_prekey_private = generated_x25519_private_key_for_test();
        let one_time_private = generated_x25519_private_key_for_test();
        let signed_prekey = anp::direct_e2ee::SignedPrekey {
            key_id: "spk-bob".to_owned(),
            public_key_b64u: x25519_public_key_b64u_for_test(&signed_prekey_private),
            expires_at: "2030-01-01T00:00:00Z".to_owned(),
        };
        let bundle = anp::direct_e2ee::build_prekey_bundle(
            "bundle-bob",
            &identity.did,
            &format!("{}#key-3", identity.did),
            signed_prekey,
            &signing_private,
            &format!("{}#key-1", identity.did),
            None,
        )
        .unwrap();
        let one_time_prekey = anp::direct_e2ee::OneTimePrekey {
            key_id: "opk-bob".to_owned(),
            public_key_b64u: x25519_public_key_b64u_for_test(&one_time_private),
        };
        let _ = agreement_private;
        TestPrekeyBundle {
            bundle,
            one_time_prekey,
        }
    }

    fn generated_x25519_private_key_for_test() -> anp::PrivateKeyMaterial {
        let bundle = anp::authentication::create_did_wba_document(
            "keys.example",
            anp::authentication::DidDocumentOptions::default(),
        )
        .unwrap();
        bundle.load_private_key("key-3").unwrap()
    }

    fn x25519_public_key_b64u_for_test(private_key: &anp::PrivateKeyMaterial) -> String {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
        match private_key.public_key() {
            anp::PublicKeyMaterial::X25519(bytes) => URL_SAFE_NO_PAD.encode(bytes),
            _ => panic!("expected X25519 key"),
        }
    }

    fn secure_direct_request(target: &str, text: &str) -> crate::messages::SendMessageRequest {
        crate::messages::SendMessageRequest {
            target: crate::messages::MessageTarget::Direct(
                crate::ids::PeerRef::parse(target, "").unwrap(),
            ),
            body: crate::messages::MessageBody::Text {
                text: text.to_owned(),
                kind: crate::messages::MessageKind::Text,
            },
            security: crate::messages::MessageSecurityMode::SecureDirect,
            client_message_id: Some(crate::ids::MessageId::parse("msg-secure-direct").unwrap()),
            delivery: crate::messages::MessageDeliveryOptions {
                idempotency_key: Some("msg-secure-direct".to_owned()),
                wait_for_final_acceptance: true,
            },
            delegated_signing: None,
        }
    }

    fn secure_direct_attachment_request(target: &str) -> crate::messages::SendMessageRequest {
        crate::messages::SendMessageRequest {
            target: crate::messages::MessageTarget::Direct(
                crate::ids::PeerRef::parse(target, "").unwrap(),
            ),
            body: crate::messages::MessageBody::Attachment {
                input: crate::attachments::AttachmentInput::Bytes {
                    filename: Some("direct-secret.pdf".to_owned()),
                    mime_type: Some("application/pdf".to_owned()),
                    bytes: b"direct attachment secret".to_vec(),
                },
                caption: Some("direct caption".to_owned()),
                mention_payload: None,
                mime_type: Some("application/pdf".to_owned()),
                filename: None,
            },
            security: crate::messages::MessageSecurityMode::SecureDirect,
            client_message_id: Some(crate::ids::MessageId::parse("msg-secure-attachment").unwrap()),
            delivery: crate::messages::MessageDeliveryOptions {
                idempotency_key: Some("msg-secure-attachment".to_owned()),
                wait_for_final_acceptance: true,
            },
            delegated_signing: None,
        }
    }

    fn committed_attachment(
        filename: &str,
        caption: &str,
    ) -> crate::internal::attachment_runtime::upload::PreparedCommittedAttachment {
        let e2ee = crate::attachments::manifest::prepare_object_e2ee_attachment_payload(
            filename,
            "application/pdf",
            b"direct attachment secret".to_vec(),
        )
        .unwrap();
        let slot = crate::internal::wire::attachment::AttachmentCreateSlotResult {
            attachment_id: "att-secure-1".to_owned(),
            slot_id: "slot-secure-1".to_owned(),
            upload_uri: "https://upload.example/slot-secure-1".to_owned(),
            upload_headers: serde_json::Map::new(),
            object_uri: "https://objects.example/att-secure-1".to_owned(),
            commit_token: "commit-secure-1".to_owned(),
            expires_at: "2026-05-24T00:00:00Z".to_owned(),
            request_service_did: "did:example:message-service".to_owned(),
        };
        let descriptor = crate::attachments::manifest::AttachmentDescriptor::from_prepared(
            &e2ee.prepared,
            slot.attachment_id.clone(),
            slot.object_uri.clone(),
        );
        let redacted_manifest =
            crate::attachments::manifest::build_attachment_manifest(&descriptor, caption)
                .expect("redacted manifest");
        let full_manifest =
            crate::attachments::manifest::build_attachment_manifest_with_object_e2ee_secrets(
                &descriptor,
                caption,
                &e2ee.secrets,
            )
            .expect("full manifest");
        let grant_ref = crate::attachments::manifest::build_attachment_grant_ref(&descriptor)
            .expect("grant ref");
        crate::internal::attachment_runtime::upload::PreparedCommittedAttachment {
            target_kind: "agent",
            target_did: "did:example:bob".to_owned(),
            prepared: e2ee.prepared,
            slot,
            descriptor,
            redacted_manifest,
            full_manifest,
            grant_ref,
        }
    }

    fn write_identity_fixture(root: &Path, alias: &str, identity: &TestIdentity) {
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
                    "did": identity.did,
                    "local_alias": alias,
                    "ready_for_auth": true,
                    "ready_for_messaging": true,
                    "missing": []
                }]
            })
            .to_string(),
        )
        .unwrap();
        fs::write(
            identity_dir.join("did.json"),
            serde_json::to_vec_pretty(&identity.document).unwrap(),
        )
        .unwrap();
        fs::write(identity_dir.join("private.key"), &identity.key1_private_pem).unwrap();
        fs::write(
            identity_dir.join("e2ee-agreement-private.pem"),
            &identity.agreement_private_pem,
        )
        .unwrap();
        fs::write(
            identity_dir.join("auth.json"),
            r#"{"jwt_token":"test-token"}"#,
        )
        .unwrap();
    }

    fn unique_temp_root(prefix: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()))
    }
}
